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
    if let Some(template) = &config.body_template {
        if serde_json::to_vec(template)?.len() > 16 * 1024 {
            return Err(ActionError::InvalidConfig("body template is too large".into()));
        }
        render_template(template, &template_fixture())?;
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

fn validate_header_env(headers: &BTreeMap<String, HeaderEnvValue>) -> Result<(), ActionError> {
    if headers.len() > 32 {
        return Err(ActionError::InvalidConfig("too many headers".into()));
    }
    for (name, value) in headers {
        HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| ActionError::InvalidConfig("invalid header name".into()))?;
        if value.env.is_empty()
            || value.env.len() > 128
            || !value
                .env
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
            || value.prefix.len() > 64
            || value.prefix.contains(['\r', '\n'])
        {
            return Err(ActionError::InvalidConfig(
                "invalid header environment reference".into(),
            ));
        }
    }
    Ok(())
}

fn resolve_header_env(
    headers: &BTreeMap<String, HeaderEnvValue>,
    lookup: impl Fn(&str) -> Result<String, env::VarError>,
) -> Result<BTreeMap<String, String>, ActionError> {
    headers
        .iter()
        .map(|(name, value)| {
            let secret = lookup(&value.env).map_err(|_| {
                ActionError::InvalidConfig("header environment variable is unavailable".into())
            })?;
            Ok((name.clone(), format!("{}{}", value.prefix, secret)))
        })
        .collect()
}

fn render_body(config: &HttpActionConfig, event: &Value) -> Result<Value, ActionError> {
    let body = match &config.body_template {
        Some(template) => render_template(template, event)?,
        None => event.clone(),
    };
    if serde_json::to_vec(&body)?.len() > 64 * 1024 {
        return Err(ActionError::InvalidConfig("rendered body is too large".into()));
    }
    Ok(body)
}

fn render_template(template: &Value, event: &Value) -> Result<Value, ActionError> {
    match template {
        Value::Array(values) => values
            .iter()
            .map(|value| render_template(value, event))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        Value::Object(values) => values
            .iter()
            .map(|(key, value)| Ok((key.clone(), render_template(value, event)?)))
            .collect::<Result<serde_json::Map<_, _>, ActionError>>()
            .map(Value::Object),
        Value::String(value) => render_string(value, event),
        value => Ok(value.clone()),
    }
}

fn render_string(template: &str, event: &Value) -> Result<Value, ActionError> {
    if template.starts_with("${") && template.ends_with('}') && template.matches("${").count() == 1
    {
        return placeholder(event, &template[2..template.len() - 1]).cloned();
    }
    let mut output = String::new();
    let mut rest = template;
    while let Some(start) = rest.find("${") {
        output.push_str(&rest[..start]);
        let tail = &rest[start + 2..];
        let end = tail.find('}').ok_or_else(|| {
            ActionError::InvalidConfig("unterminated body placeholder".into())
        })?;
        let value = placeholder(event, &tail[..end])?;
        match value {
            Value::Null => {}
            Value::String(value) => output.push_str(value),
            Value::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
            Value::Number(value) => output.push_str(&value.to_string()),
            _ => {
                return Err(ActionError::InvalidConfig(
                    "non-scalar placeholder must occupy the whole string".into(),
                ));
            }
        }
        rest = &tail[end + 1..];
    }
    if rest.contains("${") {
        return Err(ActionError::InvalidConfig(
            "unterminated body placeholder".into(),
        ));
    }
    output.push_str(rest);
    Ok(Value::String(output))
}

fn placeholder<'a>(event: &'a Value, name: &str) -> Result<&'a Value, ActionError> {
    let value = match name {
        "event" => event.get("event"),
        "eventId" => event.get("eventId"),
        "story.id" => event.pointer("/story/id"),
        "story.title" => event.pointer("/story/title"),
        "story.summary" => event.pointer("/story/summary"),
        "story.url" => event.pointer("/story/url"),
        "story.source" => event.pointer("/story/source"),
        "story.curator" => event.pointer("/story/curator"),
        "story.publishedAt" => event.pointer("/story/publishedAt"),
        "story.relatedLinks" => event.pointer("/story/relatedLinks"),
        _ => None,
    };
    value.ok_or_else(|| ActionError::InvalidConfig(format!("unknown body placeholder: {name}")))
}

fn template_fixture() -> Value {
    json!({
        "event": "story.favorite",
        "eventId": "event",
        "story": {
            "id": "story", "title": "title", "summary": "summary",
            "url": "https://example.com/story", "source": "example.com",
            "curator": "feed", "publishedAt": 0, "relatedLinks": []
        }
    })
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
