#[derive(Default)]
struct CircuitState {
    consecutive_failures: u32,
    open_until: Option<Instant>,
}

#[derive(Clone)]
struct BoundedJsonClient {
    config: HttpProviderConfig,
    client: Client,
    circuit: Arc<Mutex<CircuitState>>,
}

impl BoundedJsonClient {
    fn new(mut config: HttpProviderConfig) -> Result<Self, ModelError> {
        config.validate()?;
        if !config.base_url.path().ends_with('/') {
            let path = format!("{}/", config.base_url.path());
            config.base_url.set_path(&path);
        }
        let mut client = Client::builder()
            .timeout(config.timeout)
            .redirect(reqwest::redirect::Policy::none());
        if let Some(pem) = config.root_certificate_pem.as_deref() {
            let certificate = reqwest::Certificate::from_pem(pem)
                .map_err(|error| ModelError::Request(error.to_string()))?;
            client = client.add_root_certificate(certificate);
        }
        let client = client
            .build()
            .map_err(|error| ModelError::Request(error.to_string()))?;
        Ok(Self {
            config,
            client,
            circuit: Arc::new(Mutex::new(CircuitState::default())),
        })
    }

    async fn post(&self, path: &str, body: &Value) -> Result<Value, ModelError> {
        let bytes = serde_json::to_vec(body)
            .map_err(|error| ModelError::InvalidOutput(error.to_string()))?;
        if bytes.len() > self.config.maximum_request_bytes {
            return Err(ModelError::InvalidOutput(format!(
                "model request exceeds {} byte limit",
                self.config.maximum_request_bytes
            )));
        }
        self.request(Method::POST, path, Some(bytes)).await
    }

    async fn get(&self, path: &str) -> Result<Value, ModelError> {
        self.request(Method::GET, path, None).await
    }

    async fn request(
        &self,
        method: Method,
        path: &str,
        body: Option<Vec<u8>>,
    ) -> Result<Value, ModelError> {
        self.check_circuit()?;
        let endpoint = self
            .config
            .base_url
            .join(path.trim_start_matches('/'))
            .map_err(|error| ModelError::InvalidOutput(error.to_string()))?;
        let mut last_error = None;
        for attempt in 0..=self.config.retries {
            match self
                .request_once(method.clone(), endpoint.clone(), body.clone())
                .await
            {
                Ok(value) => {
                    self.record_success();
                    return Ok(value);
                }
                Err(error) => {
                    let should_retry = retryable(&error);
                    last_error = Some(error);
                    if !should_retry {
                        break;
                    }
                    if attempt < self.config.retries {
                        sleep(Duration::from_millis(
                            100_u64.saturating_mul(1 << attempt.min(5)),
                        ))
                        .await;
                    }
                }
            }
        }
        self.record_failure();
        Err(last_error.unwrap_or_else(|| ModelError::Unavailable("request failed".into())))
    }

    async fn request_once(
        &self,
        method: Method,
        endpoint: Url,
        body: Option<Vec<u8>>,
    ) -> Result<Value, ModelError> {
        let mut request = self.client.request(method, endpoint);
        if let Some(api_key) = self.config.api_key.as_deref() {
            request = request.bearer_auth(api_key);
        }
        if let Some(body) = body {
            request = request
                .header("content-type", "application/json")
                .body(body);
        }
        let response = request
            .send()
            .await
            .map_err(|error| ModelError::Request(error.to_string()))?;
        let status = response.status();
        if !status.is_success() {
            let class = if status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
                "temporarily unavailable"
            } else {
                "rejected"
            };
            return Err(ModelError::Request(format!(
                "model endpoint {class} ({status})"
            )));
        }
        if response
            .content_length()
            .is_some_and(|length| length > self.config.maximum_response_bytes as u64)
        {
            return Err(ModelError::InvalidOutput(
                "model response exceeds byte limit".into(),
            ));
        }
        let mut stream = response.bytes_stream();
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|error| ModelError::Request(error.to_string()))?;
            if bytes.len().saturating_add(chunk.len()) > self.config.maximum_response_bytes {
                return Err(ModelError::InvalidOutput(
                    "model response exceeds byte limit".into(),
                ));
            }
            bytes.extend_from_slice(&chunk);
        }
        if bytes.is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_slice(&bytes)
            .map_err(|error| ModelError::InvalidOutput(format!("invalid JSON: {error}")))
    }

    fn check_circuit(&self) -> Result<(), ModelError> {
        let mut state = self
            .circuit
            .lock()
            .map_err(|_| ModelError::Unavailable("model circuit lock is poisoned".into()))?;
        if state.open_until.is_some_and(|until| until > Instant::now()) {
            return Err(ModelError::Unavailable("model circuit is open".into()));
        }
        if state.open_until.is_some() {
            state.open_until = None;
            state.consecutive_failures = 0;
        }
        Ok(())
    }

    fn record_success(&self) {
        if let Ok(mut state) = self.circuit.lock() {
            state.consecutive_failures = 0;
            state.open_until = None;
        }
    }

    fn record_failure(&self) {
        if let Ok(mut state) = self.circuit.lock() {
            state.consecutive_failures = state.consecutive_failures.saturating_add(1);
            if state.consecutive_failures >= self.config.circuit_failure_threshold {
                state.open_until = Some(Instant::now() + self.config.circuit_cooldown);
            }
        }
    }

    async fn health(&self) -> Result<ModelHealth, ModelError> {
        self.get("models").await?;
        Ok(ModelHealth {
            ready: true,
            detail: "HTTP provider ready".into(),
        })
    }
}

fn retryable(error: &ModelError) -> bool {
    match error {
        ModelError::Request(message) => !message.contains("endpoint rejected"),
        ModelError::Unavailable(_) => true,
        ModelError::InvalidOutput(_) => false,
    }
}
