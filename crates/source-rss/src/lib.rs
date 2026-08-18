use std::collections::{BTreeMap, HashMap};

use async_trait::async_trait;
use feed_rs::{
    model::{Entry, Text},
    parser,
};
use quick_xml::{Reader, XmlVersion, events::Event};
use rill_domain::{ExternalLink, LinkRelation, RawSourceItem, SourceKind};
use rill_source_api::{
    ConditionalHeaders, ConnectorContext, ConnectorError, ConnectorMetadata, SourceBatch,
    SourceConnector, ValidationResult,
};
use scraper::Html;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use url::Url;

#[derive(Debug, Clone, Default)]
pub struct RssConnector;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RssConfig {
    pub url: String,
    #[serde(default = "default_poll_seconds")]
    pub poll_interval_seconds: u64,
    #[serde(default = "enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub shared: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RssCursor {
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredFeed {
    pub url: Url,
    pub page_title: Option<String>,
}

#[async_trait]
impl SourceConnector for RssConnector {
    fn kind(&self) -> SourceKind {
        SourceKind::Rss
    }

    fn metadata(&self) -> ConnectorMetadata {
        ConnectorMetadata {
            display_name: "RSS / Atom".to_owned(),
            supports_backfill: true,
            supports_push: false,
        }
    }

    fn config_schema(&self) -> Value {
        json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["url"],
            "properties": {
                "url": { "type": "string", "format": "uri", "pattern": "^https?://" },
                "pollIntervalSeconds": { "type": "integer", "minimum": 60 },
                "enabled": { "type": "boolean" },
                "shared": { "type": "boolean" }
            }
        })
    }

    async fn validate(
        &self,
        _context: &ConnectorContext,
        config: &Value,
    ) -> Result<ValidationResult, ConnectorError> {
        let config = parse_config(config)?;
        let url = Url::parse(&config.url)
            .map_err(|error| ConnectorError::InvalidConfig(format!("url: {error}")))?;
        let valid_scheme = matches!(url.scheme(), "http" | "https");
        let valid_interval = config.poll_interval_seconds >= 60;
        let mut messages = Vec::new();
        if !valid_scheme {
            messages.push("url must use http or https".to_owned());
        }
        if !valid_interval {
            messages.push("pollIntervalSeconds must be at least 60".to_owned());
        }
        Ok(ValidationResult {
            valid: valid_scheme && valid_interval,
            messages,
        })
    }

    async fn poll(
        &self,
        context: &ConnectorContext,
        config: &Value,
        cursor: Option<&Value>,
        limit: usize,
    ) -> Result<SourceBatch, ConnectorError> {
        let config = parse_config(config)?;
        if !config.enabled {
            return Ok(SourceBatch {
                items: Vec::new(),
                cursor: cursor.cloned(),
                not_modified: true,
            });
        }
        let url = Url::parse(&config.url)
            .map_err(|error| ConnectorError::InvalidConfig(format!("url: {error}")))?;
        let cursor: RssCursor = cursor
            .cloned()
            .map(serde_json::from_value)
            .transpose()
            .map_err(|error| ConnectorError::InvalidConfig(format!("cursor: {error}")))?
            .unwrap_or_default();
        let response = context
            .http
            .get(
                &url,
                &ConditionalHeaders {
                    etag: cursor.etag.clone(),
                    last_modified: cursor.last_modified.clone(),
                },
            )
            .await?;
        let next_cursor = serde_json::to_value(RssCursor {
            etag: response.etag.or(cursor.etag),
            last_modified: response.last_modified.or(cursor.last_modified),
        })
        .map_err(|error| ConnectorError::Parse(error.to_string()))?;
        if response.not_modified {
            return Ok(SourceBatch {
                items: Vec::new(),
                cursor: Some(next_cursor),
                not_modified: true,
            });
        }
        let items = parse_feed(&response.body, &response.final_url, limit)?;
        Ok(SourceBatch {
            items,
            cursor: Some(next_cursor),
            not_modified: false,
        })
    }
}

pub fn parse_feed(
    bytes: &[u8],
    base_url: &Url,
    limit: usize,
) -> Result<Vec<RawSourceItem>, ConnectorError> {
    let base = base_url.to_string();
    let parser = parser::Builder::new()
        .base_uri(Some(&base))
        .id_generator(move |links, title, feed_id| {
            let mut hash = Sha256::new();
            hash.update(feed_id.unwrap_or(&base).as_bytes());
            if let Some(link) = links.first() {
                hash.update(link.href.as_bytes());
            }
            if let Some(title) = title {
                hash.update(title.content.as_bytes());
            }
            format!("fallback:{:x}", hash.finalize())
        })
        .build();
    let feed = parser
        .parse(bytes)
        .map_err(|error| ConnectorError::Parse(error.to_string()))?;
    let comments = rss_comments(bytes, base_url)?;
    let mut items = feed
        .entries
        .into_iter()
        .take(limit)
        .map(entry_to_item)
        .collect::<Vec<_>>();
    for item in &mut items {
        let discussion = comments
            .get(&item.external_id)
            .or_else(|| item.source_url.as_ref().and_then(|url| comments.get(url)));
        if let Some(url) = discussion
            && !item.external_urls.iter().any(|link| link.url == *url)
            && item.external_urls.len() < 16
        {
            item.external_urls.push(ExternalLink {
                url: url.clone(),
                relation: LinkRelation::replies(),
                title: None,
                ordinal: u32::try_from(item.external_urls.len()).unwrap_or(u32::MAX),
            });
        }
    }
    Ok(items)
}

pub fn discover_feed(html: &str, base_url: &Url) -> Option<DiscoveredFeed> {
    let document = Html::parse_document(html);
    let selector = scraper::Selector::parse("link[href], a[href]").expect("static selector");
    let title_selector = scraper::Selector::parse("title").expect("static selector");
    let page_title = document
        .select(&title_selector)
        .next()
        .map(|title| title.text().collect::<Vec<_>>().join(" "))
        .map(|title| normalize_whitespace(&title))
        .filter(|title| !title.is_empty())
        .map(|title| title.chars().take(120).collect());
    for candidate in document.select(&selector).take(128) {
        let Some(href) = candidate.value().attr("href") else {
            continue;
        };
        let name = candidate.value().name();
        let link_type = candidate
            .value()
            .attr("type")
            .unwrap_or_default()
            .to_ascii_lowercase();
        let rel_is_alternate = candidate.value().attr("rel").is_some_and(|rel| {
            rel.split_ascii_whitespace()
                .any(|item| item.eq_ignore_ascii_case("alternate"))
        });
        let label = candidate
            .text()
            .collect::<Vec<_>>()
            .join(" ")
            .trim()
            .to_ascii_lowercase();
        let feed_link = name == "link"
            && rel_is_alternate
            && matches!(
                link_type.as_str(),
                "application/rss+xml" | "application/atom+xml" | "application/xml" | "text/xml"
            )
            || name == "a" && matches!(label.as_str(), "rss" | "atom" | "feed" | "rss feed");
        if feed_link
            && let Ok(url) = base_url.join(href)
            && matches!(url.scheme(), "http" | "https")
        {
            return Some(DiscoveredFeed {
                url,
                page_title: page_title.clone(),
            });
        }
    }
    None
}

fn entry_to_item(entry: Entry) -> RawSourceItem {
    let link = entry
        .links
        .iter()
        .find(|link| link.rel.as_deref().is_none_or(|rel| rel == "alternate"))
        .or_else(|| entry.links.first())
        .map(|link| link.href.clone());
    let summary = entry.summary.as_ref().map(|text| text.content.clone());
    let content = entry
        .content
        .as_ref()
        .and_then(|content| content.body.clone());
    let body_html = content
        .as_ref()
        .filter(|_| {
            entry
                .content
                .as_ref()
                .is_some_and(|content| content.content_type.as_str().contains("html"))
        })
        .cloned()
        .or_else(|| {
            entry
                .summary
                .as_ref()
                .filter(|text| is_html(text))
                .map(|text| text.content.clone())
        });
    let body_text_source = content.or(summary);
    let body_text = body_text_source.as_deref().map(html_to_text);
    let categories = entry
        .categories
        .iter()
        .map(|category| category.term.clone())
        .collect::<Vec<_>>();
    let external_urls = entry
        .links
        .iter()
        .filter(|link| safe_http_url(&link.href))
        .take(16)
        .enumerate()
        .map(|(ordinal, link)| ExternalLink {
            url: link.href.clone(),
            relation: link
                .rel
                .as_deref()
                .and_then(LinkRelation::new)
                .unwrap_or_else(LinkRelation::alternate),
            title: link
                .title
                .as_deref()
                .map(|title| title.chars().take(120).collect()),
            ordinal: u32::try_from(ordinal).unwrap_or(u32::MAX),
        })
        .collect();
    RawSourceItem {
        external_id: entry.id,
        item_kind: "article".to_owned(),
        title: entry.title.map(|title| title.content),
        body_text,
        body_html,
        author: entry.authors.first().map(|author| author.name.clone()),
        source_url: link.clone(),
        published_at: entry
            .published
            .or(entry.updated)
            .map(|date| date.timestamp()),
        edited_at: None,
        deleted_at: None,
        external_urls,
        media: Vec::new(),
        metadata: json!({ "categories": categories, "language": entry.language }),
    }
}

fn safe_http_url(value: &str) -> bool {
    Url::parse(value).is_ok_and(|url| {
        value.len() <= 2048
            && matches!(url.scheme(), "http" | "https")
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
    })
}

#[derive(Default)]
struct SupplementaryItem {
    guid: String,
    link: String,
    comments: String,
}

fn rss_comments(bytes: &[u8], base_url: &Url) -> Result<HashMap<String, String>, ConnectorError> {
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(true);
    let mut item = None::<SupplementaryItem>;
    let mut field = None::<Vec<u8>>;
    let mut output = HashMap::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(element)) if element.name().as_ref() == b"item" => {
                item = Some(SupplementaryItem::default());
            }
            Ok(Event::Start(element)) if item.is_some() => {
                let name = element.name();
                if matches!(name.as_ref(), b"guid" | b"link" | b"comments") {
                    field = Some(name.as_ref().to_vec());
                }
            }
            Ok(Event::Text(text)) if field.is_some() => {
                let value = text
                    .xml_content(XmlVersion::Implicit1_0)
                    .map_err(|error| ConnectorError::Parse(error.to_string()))?;
                assign_rss_field(item.as_mut(), field.as_deref(), &value);
            }
            Ok(Event::CData(text)) if field.is_some() => {
                let value = text
                    .decode()
                    .map_err(|error| ConnectorError::Parse(error.to_string()))?;
                assign_rss_field(item.as_mut(), field.as_deref(), &value);
            }
            Ok(Event::End(element)) if element.name().as_ref() == b"item" => {
                if let Some(item) = item.take()
                    && let Ok(comments) = base_url.join(item.comments.trim())
                    && safe_http_url(comments.as_str())
                {
                    let comments = comments.to_string();
                    if !item.guid.trim().is_empty() {
                        output.insert(item.guid.trim().to_owned(), comments.clone());
                    }
                    if let Ok(link) = base_url.join(item.link.trim())
                        && safe_http_url(link.as_str())
                    {
                        output.insert(link.to_string(), comments);
                    }
                }
                field = None;
            }
            Ok(Event::End(_)) => field = None,
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => return Err(ConnectorError::Parse(error.to_string())),
        }
    }
    Ok(output)
}

fn assign_rss_field(item: Option<&mut SupplementaryItem>, field: Option<&[u8]>, value: &str) {
    let Some(item) = item else {
        return;
    };
    match field {
        Some(b"guid") => item.guid.push_str(value),
        Some(b"link") => item.link.push_str(value),
        Some(b"comments") => item.comments.push_str(value),
        _ => {}
    }
}

fn is_html(text: &Text) -> bool {
    text.content_type.as_str().contains("html")
}

fn html_to_text(value: &str) -> String {
    let fragment = Html::parse_fragment(value);
    normalize_whitespace(&fragment.root_element().text().collect::<Vec<_>>().join(" "))
}

fn normalize_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn parse_config(config: &Value) -> Result<RssConfig, ConnectorError> {
    serde_json::from_value(config.clone())
        .map_err(|error| ConnectorError::InvalidConfig(error.to_string()))
}

fn default_poll_seconds() -> u64 {
    900
}
fn enabled() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpmlFeed {
    pub title: String,
    pub xml_url: String,
    pub html_url: Option<String>,
}

pub fn import_opml(bytes: &[u8], maximum: usize) -> Result<Vec<OpmlFeed>, ConnectorError> {
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(true);
    let mut feeds = Vec::new();
    loop {
        match reader.read_event() {
            Ok(Event::Empty(element)) | Ok(Event::Start(element))
                if element.name().as_ref() == b"outline" =>
            {
                let mut attributes = BTreeMap::new();
                for attribute in element.attributes().with_checks(true) {
                    let attribute =
                        attribute.map_err(|error| ConnectorError::Parse(error.to_string()))?;
                    let key = String::from_utf8_lossy(attribute.key.as_ref()).into_owned();
                    let value = attribute
                        .decoded_and_normalized_value(XmlVersion::Implicit1_0, reader.decoder())
                        .map_err(|error| ConnectorError::Parse(error.to_string()))?
                        .into_owned();
                    attributes.insert(key, value);
                }
                if let Some(xml_url) = attributes.get("xmlUrl") {
                    Url::parse(xml_url).map_err(|error| {
                        ConnectorError::Parse(format!("invalid OPML URL: {error}"))
                    })?;
                    feeds.push(OpmlFeed {
                        title: attributes
                            .get("title")
                            .or_else(|| attributes.get("text"))
                            .cloned()
                            .unwrap_or_else(|| xml_url.clone()),
                        xml_url: xml_url.clone(),
                        html_url: attributes.get("htmlUrl").cloned(),
                    });
                    if feeds.len() >= maximum {
                        break;
                    }
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => return Err(ConnectorError::Parse(error.to_string())),
        }
    }
    Ok(feeds)
}

pub fn export_opml(title: &str, feeds: &[OpmlFeed]) -> String {
    let mut output = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><opml version=\"2.0\"><head><title>{}</title></head><body>",
        xml_escape(title),
    );
    for feed in feeds {
        output.push_str(&format!(
            "<outline type=\"rss\" text=\"{}\" title=\"{}\" xmlUrl=\"{}\"",
            xml_escape(&feed.title),
            xml_escape(&feed.title),
            xml_escape(&feed.xml_url),
        ));
        if let Some(html_url) = &feed.html_url {
            output.push_str(&format!(" htmlUrl=\"{}\"", xml_escape(html_url)));
        }
        output.push_str("/>");
    }
    output.push_str("</body></opml>");
    output
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;

    const RSS: &str = r#"<?xml version="1.0"?><rss version="2.0"><channel>
      <title>Example</title><link>https://example.com</link><description>Test</description>
      <item><guid>story-1</guid><title>One</title><link>https://example.com/one?utm_source=x</link>
      <comments>https://forum.example/topics/one</comments>
      <description><![CDATA[<p>Useful <b>story</b>.</p>]]></description><pubDate>Sun, 17 Aug 2025 12:00:00 GMT</pubDate></item>
    </channel></rss>"#;

    const ATOM: &str = r#"<?xml version="1.0"?><feed xmlns="http://www.w3.org/2005/Atom">
      <id>feed-1</id><title>Example</title><updated>2025-08-17T12:00:00Z</updated>
      <entry><id>atom-1</id><title>Atom one</title><updated>2025-08-17T12:00:00Z</updated>
      <link href="https://example.com/atom"/><link rel="replies" href="https://forum.example/atom"/>
      <content type="html">&lt;p&gt;Atom body&lt;/p&gt;</content></entry>
    </feed>"#;

    #[test]
    fn parses_rss_guid_and_atom_id() {
        let base = Url::parse("https://example.com/feed").unwrap();
        let rss = parse_feed(RSS.as_bytes(), &base, 10).unwrap();
        let atom = parse_feed(ATOM.as_bytes(), &base, 10).unwrap();
        assert_eq!(rss[0].external_id, "story-1");
        assert_eq!(rss[0].body_text.as_deref(), Some("Useful story ."));
        assert_eq!(atom[0].external_id, "atom-1");
        assert_eq!(
            atom[0].source_url.as_deref(),
            Some("https://example.com/atom")
        );
        assert_eq!(rss[0].external_urls[1].relation.as_str(), "replies");
        assert_eq!(
            rss[0].external_urls[1].url,
            "https://forum.example/topics/one"
        );
        assert!(atom[0].external_urls.iter().any(|link| {
            link.relation.as_str() == "replies" && link.url == "https://forum.example/atom"
        }));
    }

    #[test]
    fn unsafe_rss_comments_url_is_ignored() {
        let input = r#"<rss version="2.0"><channel><title>X</title><link>https://example.com</link>
          <description>X</description><item><guid>x</guid><title>X</title>
          <link>https://example.com/x</link><comments>javascript:alert(1)</comments></item>
          </channel></rss>"#;
        let items = parse_feed(
            input.as_bytes(),
            &Url::parse("https://example.com/feed").unwrap(),
            1,
        )
        .unwrap();
        assert_eq!(items[0].external_urls.len(), 1);
    }

    #[test]
    fn recorded_development_feed_contains_direct_and_roundup_items() {
        let base = Url::parse("http://127.0.0.1:3011/rss.xml").unwrap();
        let items =
            parse_feed(include_bytes!("../../../fixtures/rss/feed.xml"), &base, 10).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].external_id, "fixture-direct-1");
        assert!(
            items[1]
                .body_html
                .as_deref()
                .unwrap()
                .contains("WASI rendering limits")
        );
    }

    #[test]
    fn missing_id_fallback_is_stable() {
        let input = r#"<rss version="2.0"><channel><title>X</title><link>https://example.com</link><description>X</description><item><title>Same</title><description>Body</description></item></channel></rss>"#;
        let base = Url::parse("https://example.com/feed").unwrap();
        let first = parse_feed(input.as_bytes(), &base, 1).unwrap();
        let second = parse_feed(input.as_bytes(), &base, 1).unwrap();
        assert_eq!(first[0].external_id, second[0].external_id);
        assert!(first[0].external_id.starts_with("fallback:"));
    }

    #[test]
    fn opml_round_trip_preserves_feed_urls() {
        let feeds = vec![OpmlFeed {
            title: "Example & news".into(),
            xml_url: "https://example.com/feed.xml".into(),
            html_url: Some("https://example.com/".into()),
        }];
        let xml = export_opml("Rill", &feeds);
        assert_eq!(import_opml(xml.as_bytes(), 10).unwrap(), feeds);
    }

    #[test]
    fn discovers_relative_feed_link_and_page_title() {
        let discovered = discover_feed(
            r#"<html><head><title>Small useful blog</title><link rel="alternate" type="application/atom+xml" href="/feed.atom"></head></html>"#,
            &Url::parse("https://example.com/articles/one").unwrap(),
        )
        .unwrap();
        assert_eq!(discovered.url.as_str(), "https://example.com/feed.atom");
        assert_eq!(discovered.page_title.as_deref(), Some("Small useful blog"));
    }
}
