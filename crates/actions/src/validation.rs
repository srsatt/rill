fn validate_config(
    config: &HttpActionConfig,
    allow_private_networks: bool,
) -> Result<(), ActionError> {
    let url =
        Url::parse(&config.url).map_err(|error| ActionError::InvalidConfig(error.to_string()))?;
    validate_outbound_url(&url, allow_private_networks).map_err(fetch_error)?;
    if !matches!(
        config.method.to_ascii_uppercase().as_str(),
        "POST" | "PUT" | "PATCH"
    ) {
        return Err(ActionError::InvalidConfig(
            "method must be POST, PUT, or PATCH".into(),
        ));
    }
    if config.timeout_seconds == 0
        || config.timeout_seconds > 120
        || config.maximum_response_bytes == 0
        || config.maximum_response_bytes > 4 * 1024 * 1024
        || config.maximum_attempts == 0
        || config.maximum_attempts > 10
    {
        return Err(ActionError::InvalidConfig(
            "timeout, response limit, or retry count is out of range".into(),
        ));
    }
    Ok(())
}

fn validate_headers(headers: &BTreeMap<String, String>) -> Result<(), ActionError> {
    if headers.len() > 32 {
        return Err(ActionError::InvalidConfig("too many headers".into()));
    }
    for (name, value) in headers {
        HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| ActionError::InvalidConfig("invalid header name".into()))?;
        HeaderValue::from_str(value)
            .map_err(|_| ActionError::InvalidConfig("invalid header value".into()))?;
    }
    Ok(())
}

fn fetch_error(error: FetchError) -> ActionError {
    ActionError::InvalidConfig(error.to_string())
}

fn redact_error(error: &str) -> String {
    error.chars().take(500).collect()
}

fn default_method() -> String {
    "POST".into()
}
const fn default_timeout() -> u64 {
    15
}
const fn default_response_limit() -> usize {
    64 * 1024
}
const fn default_attempts() -> u32 {
    5
}
const fn enabled() -> bool {
    true
}
