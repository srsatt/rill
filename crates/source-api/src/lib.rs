use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::{
    Client, Method, StatusCode,
    header::{self, HeaderMap, HeaderValue},
};
use rill_domain::{RawSourceItem, SourceKind};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use url::Url;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorMetadata {
    pub display_name: String,
    pub supports_backfill: bool,
    pub supports_push: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationResult {
    pub valid: bool,
    pub messages: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceBatch {
    pub items: Vec<RawSourceItem>,
    pub cursor: Option<Value>,
    pub not_modified: bool,
}

#[derive(Clone)]
pub struct ConnectorContext {
    pub http: Arc<BoundedHttpClient>,
}

#[derive(Debug, Error)]
pub enum ConnectorError {
    #[error("invalid source configuration: {0}")]
    InvalidConfig(String),
    #[error("source fetch failed: {0}")]
    Fetch(#[from] FetchError),
    #[error("source response could not be parsed: {0}")]
    Parse(String),
}

#[async_trait]
pub trait SourceConnector: Send + Sync {
    fn kind(&self) -> SourceKind;
    fn metadata(&self) -> ConnectorMetadata;
    fn config_schema(&self) -> Value;

    async fn validate(
        &self,
        context: &ConnectorContext,
        config: &Value,
    ) -> Result<ValidationResult, ConnectorError>;

    async fn poll(
        &self,
        context: &ConnectorContext,
        config: &Value,
        cursor: Option<&Value>,
        limit: usize,
    ) -> Result<SourceBatch, ConnectorError>;
}

#[derive(Debug, Clone)]
pub struct FetchPolicy {
    pub timeout: Duration,
    pub max_redirects: usize,
    pub max_response_bytes: usize,
    pub allow_private_networks: bool,
}

impl Default for FetchPolicy {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(15),
            max_redirects: 5,
            max_response_bytes: 4 * 1024 * 1024,
            allow_private_networks: false,
        }
    }
}

#[derive(Clone)]
pub struct BoundedHttpClient {
    policy: FetchPolicy,
}

#[derive(Debug, Clone, Default)]
pub struct ConditionalHeaders {
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

#[derive(Debug, Clone)]
pub struct FetchResponse {
    pub final_url: Url,
    pub body: Vec<u8>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub not_modified: bool,
    pub content_type: Option<String>,
}

#[derive(Debug, Error)]
pub enum FetchError {
    #[error("URL is not allowed: {0}")]
    UrlPolicy(String),
    #[error("HTTP client could not be built: {0}")]
    Client(String),
    #[error("HTTP request failed: {0}")]
    Request(String),
    #[error("host name could not be resolved: {0}")]
    Resolve(String),
    #[error("HTTP response was {0}")]
    Status(u16),
    #[error("HTTP response exceeds {limit} byte limit")]
    TooLarge { limit: usize },
}

impl BoundedHttpClient {
    pub fn new(policy: FetchPolicy) -> Result<Self, FetchError> {
        if policy.timeout.is_zero() || policy.max_response_bytes == 0 {
            return Err(FetchError::Client("fetch limits must be positive".into()));
        }
        Ok(Self { policy })
    }

    pub async fn get(
        &self,
        url: &Url,
        conditional: &ConditionalHeaders,
    ) -> Result<FetchResponse, FetchError> {
        let mut headers = HeaderMap::new();
        if let Some(etag) = &conditional.etag {
            headers.insert(
                header::IF_NONE_MATCH,
                HeaderValue::from_str(etag)
                    .map_err(|_| FetchError::Request("invalid ETag".into()))?,
            );
        }
        if let Some(last_modified) = &conditional.last_modified {
            headers.insert(
                header::IF_MODIFIED_SINCE,
                HeaderValue::from_str(last_modified)
                    .map_err(|_| FetchError::Request("invalid Last-Modified value".into()))?,
            );
        }
        self.request(Method::GET, url, headers, None).await
    }

    pub async fn send_json(
        &self,
        method: Method,
        url: &Url,
        mut headers: HeaderMap,
        body: Vec<u8>,
    ) -> Result<FetchResponse, FetchError> {
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        self.request(method, url, headers, Some(body)).await
    }

    async fn request(
        &self,
        method: Method,
        url: &Url,
        headers: HeaderMap,
        body: Option<Vec<u8>>,
    ) -> Result<FetchResponse, FetchError> {
        validate_outbound_url(url, self.policy.allow_private_networks)?;
        let original_host = url.host_str().unwrap_or_default().to_ascii_lowercase();
        let mut current = url.clone();
        let mut redirects = 0usize;
        let response = loop {
            let (host, addresses) =
                resolve_public(&current, self.policy.allow_private_networks).await?;
            let client = Client::builder()
                .timeout(self.policy.timeout)
                .redirect(reqwest::redirect::Policy::none())
                .resolve_to_addrs(&host, &addresses)
                .user_agent(concat!("Rill/", env!("CARGO_PKG_VERSION")))
                .build()
                .map_err(|error| FetchError::Client(error.to_string()))?;
            let mut request = client
                .request(method.clone(), current.clone())
                .headers(headers.clone());
            if let Some(body) = &body {
                request = request.body(body.clone());
            }
            let response = request
                .send()
                .await
                .map_err(|error| FetchError::Request(error.without_url().to_string()))?;
            if !response.status().is_redirection() {
                break response;
            }
            if redirects >= self.policy.max_redirects {
                return Err(FetchError::Request("too many redirects".into()));
            }
            if method != Method::GET
                && !matches!(
                    response.status(),
                    StatusCode::TEMPORARY_REDIRECT | StatusCode::PERMANENT_REDIRECT
                )
            {
                return Err(FetchError::Status(response.status().as_u16()));
            }
            let location = response
                .headers()
                .get(header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or_else(|| FetchError::Request("redirect location is missing".into()))?;
            let next = current
                .join(location)
                .map_err(|error| FetchError::Request(error.to_string()))?;
            validate_outbound_url(&next, self.policy.allow_private_networks)?;
            if method != Method::GET
                && next.host_str().unwrap_or_default().to_ascii_lowercase() != original_host
            {
                return Err(FetchError::UrlPolicy(
                    "action redirects may not change host".into(),
                ));
            }
            current = next;
            redirects += 1;
        };
        let status = response.status();
        let final_url = current;
        validate_outbound_url(&final_url, self.policy.allow_private_networks)?;
        let etag = response
            .headers()
            .get(header::ETAG)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let last_modified = response
            .headers()
            .get(header::LAST_MODIFIED)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        if status == StatusCode::NOT_MODIFIED {
            return Ok(FetchResponse {
                final_url,
                body: Vec::new(),
                etag,
                last_modified,
                not_modified: true,
                content_type,
            });
        }
        if !status.is_success() {
            return Err(FetchError::Status(status.as_u16()));
        }
        if response.content_length().is_some_and(|length| {
            length > u64::try_from(self.policy.max_response_bytes).unwrap_or(u64::MAX)
        }) {
            return Err(FetchError::TooLarge {
                limit: self.policy.max_response_bytes,
            });
        }
        let mut body = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|error| FetchError::Request(error.to_string()))?;
            if body.len().saturating_add(chunk.len()) > self.policy.max_response_bytes {
                return Err(FetchError::TooLarge {
                    limit: self.policy.max_response_bytes,
                });
            }
            body.extend_from_slice(&chunk);
        }
        Ok(FetchResponse {
            final_url,
            body,
            etag,
            last_modified,
            not_modified: false,
            content_type,
        })
    }
}

async fn resolve_public(
    url: &Url,
    allow_private_networks: bool,
) -> Result<(String, Vec<SocketAddr>), FetchError> {
    let host = url
        .host_str()
        .ok_or_else(|| FetchError::UrlPolicy("host is required".into()))?
        .to_owned();
    let port = url
        .port_or_known_default()
        .ok_or_else(|| FetchError::UrlPolicy("URL port is unknown".into()))?;
    let mut addresses = tokio::net::lookup_host((host.as_str(), port))
        .await
        .map_err(|error| FetchError::Resolve(error.to_string()))?
        .collect::<Vec<_>>();
    addresses.sort_unstable();
    addresses.dedup();
    if addresses.is_empty() {
        return Err(FetchError::Resolve("host has no addresses".into()));
    }
    if !allow_private_networks && addresses.iter().any(|address| is_private_ip(address.ip())) {
        return Err(FetchError::UrlPolicy(
            "host resolves to a private or non-routable address".into(),
        ));
    }
    Ok((host, addresses))
}

pub fn validate_outbound_url(url: &Url, allow_private_networks: bool) -> Result<(), FetchError> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err(FetchError::UrlPolicy(
            "only http and https are supported".to_owned(),
        ));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(FetchError::UrlPolicy(
            "URL credentials are forbidden".to_owned(),
        ));
    }
    let host = url
        .host_str()
        .ok_or_else(|| FetchError::UrlPolicy("host is required".to_owned()))?;
    if !allow_private_networks && host.parse::<IpAddr>().is_ok_and(is_private_ip) {
        return Err(FetchError::UrlPolicy(
            "private network address is forbidden".to_owned(),
        ));
    }
    Ok(())
}

fn is_private_ip(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => is_private_v4(address),
        IpAddr::V6(address) => {
            address.to_ipv4_mapped().is_some_and(is_private_v4)
                || address.is_loopback()
                || address.is_unspecified()
                || is_unique_local(address)
                || is_ipv6_link_local(address)
                || address.is_multicast()
                || (address.segments()[0] == 0x2001 && address.segments()[1] == 0x0db8)
        }
    }
}

fn is_private_v4(address: Ipv4Addr) -> bool {
    let [first, second, _, _] = address.octets();
    address.is_private()
        || address.is_loopback()
        || address.is_link_local()
        || address.is_unspecified()
        || address.is_broadcast()
        || address.is_multicast()
        || address.is_documentation()
        || first == 0
        || (first == 100 && (64..=127).contains(&second))
        || (first == 192 && second == 0)
        || (first == 198 && matches!(second, 18 | 19))
        || first >= 240
}

fn is_unique_local(address: Ipv6Addr) -> bool {
    address.octets()[0] & 0xfe == 0xfc
}

fn is_ipv6_link_local(address: Ipv6Addr) -> bool {
    let octets = address.octets();
    octets[0] == 0xfe && octets[1] & 0xc0 == 0x80
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outbound_policy_rejects_credentials_and_private_literals() {
        assert!(validate_outbound_url(&Url::parse("http://127.0.0.1/a").unwrap(), false).is_err());
        assert!(
            validate_outbound_url(&Url::parse("https://user@example.com/a").unwrap(), false)
                .is_err()
        );
        assert!(
            validate_outbound_url(&Url::parse("https://example.com/a").unwrap(), false).is_ok()
        );
    }

    #[tokio::test]
    async fn outbound_policy_resolves_and_rejects_private_dns_answers() {
        let client = BoundedHttpClient::new(FetchPolicy::default()).unwrap();
        let error = client
            .get(
                &Url::parse("http://localhost/internal").unwrap(),
                &ConditionalHeaders::default(),
            )
            .await
            .unwrap_err();
        assert!(matches!(error, FetchError::UrlPolicy(_)));
    }
}
