impl HostState {
    fn new(service: &PluginService) -> Self {
        Self {
            limits: StoreLimitsBuilder::new()
                .memory_size(service.limits.memory_bytes)
                .build(),
            secrets: service.secrets.clone(),
            named_secrets: BTreeMap::new(),
            allowed_hosts: BTreeSet::new(),
            secret_values: Vec::new(),
            http: service.http.clone(),
            maximum_http_bytes: service.limits.maximum_http_bytes,
        }
    }

    fn redact(&self, message: &str) -> String {
        let mut redacted = message.chars().take(2_000).collect::<String>();
        for secret in &self.secret_values {
            if !secret.is_empty() {
                redacted = redacted.replace(secret, "[REDACTED]");
            }
        }
        redacted
    }
}

impl rill::source_plugin::host::Host for HostState {
    async fn log(&mut self, level: String, message: String) {
        let message = self.redact(&message);
        match level.as_str() {
            "error" => tracing::error!(target: "rill_plugin", %message),
            "warn" => tracing::warn!(target: "rill_plugin", %message),
            "debug" => tracing::debug!(target: "rill_plugin", %message),
            _ => tracing::info!(target: "rill_plugin", %message),
        }
    }

    async fn get_secret(&mut self, name: String) -> Result<String, String> {
        let id = self
            .named_secrets
            .get(&name)
            .ok_or_else(|| format!("secret capability {name} is denied"))?;
        let value = self
            .secrets
            .as_ref()
            .ok_or_else(|| "secret storage is unavailable".to_owned())?
            .get(id)
            .map_err(|_| "secret is unavailable".to_owned())?;
        let value = String::from_utf8(value).map_err(|_| "secret is not UTF-8".to_owned())?;
        self.secret_values.push(value.clone());
        Ok(value)
    }

    async fn http_get(&mut self, url: String, maximum_bytes: u32) -> Result<String, String> {
        let url = Url::parse(&url).map_err(|_| "invalid URL".to_owned())?;
        let host = url
            .host_str()
            .map(str::to_ascii_lowercase)
            .ok_or_else(|| "URL host is required".to_owned())?;
        if !self.allowed_hosts.contains(&host) {
            return Err("HTTP host capability is denied".into());
        }
        let response = self
            .http
            .get(&url, &ConditionalHeaders::default())
            .await
            .map_err(|error| error.to_string())?;
        let maximum = usize::try_from(maximum_bytes)
            .unwrap_or(usize::MAX)
            .min(self.maximum_http_bytes);
        if response.body.len() > maximum {
            return Err("HTTP response exceeds plugin limit".into());
        }
        serde_json::to_string(&json!({
            "url": response.final_url.to_string(),
            "contentType": response.content_type,
            "bodyBase64": general_purpose::STANDARD.encode(response.body),
        }))
        .map_err(|_| "HTTP response serialization failed".to_owned())
    }
}

