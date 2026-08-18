async fn render_page(
    state: AppState,
    template: &'static str,
    mode: RenderMode,
    props: impl Serialize,
    csrf_token: &str,
    hydrate: bool,
) -> Response {
    let render_id = format!("r{}-", Uuid::new_v4().simple());
    let props = match serde_json::to_value(props) {
        Ok(props) => props,
        Err(failure) => {
            error!(error = %failure, "page model serialization failed");
            return error_page();
        }
    };
    let request = RenderRequest {
        version: RENDER_PROTOCOL_VERSION,
        template: template.to_owned(),
        mode,
        locale: "en".to_owned(),
        render_id: render_id.clone(),
        props,
        assets: BTreeMap::new(),
        csrf_token: csrf_token.to_owned(),
    };
    let renderer = state.renderer.clone();
    let metrics = state.metrics.clone();
    let started = Instant::now();
    match tokio::task::spawn_blocking(move || renderer.render(&request)).await {
        Ok(Ok(rendered)) => {
            metrics.observe("renderer", started.elapsed(), true, 1);
            document_response(
                rendered,
                &render_id,
                &state.assets,
                hydrate,
                state.dev_reload,
            )
        }
        Ok(Err(failure)) => {
            metrics.observe("renderer", started.elapsed(), false, 0);
            error!(error = %failure, "renderer failed");
            error_page()
        }
        Err(failure) => {
            metrics.observe("renderer", started.elapsed(), false, 0);
            error!(error = %failure, "renderer task failed");
            error_page()
        }
    }
}

fn document_response(
    rendered: RenderResponse,
    render_id: &str,
    assets: &BrowserAssets,
    hydrate: bool,
    dev_reload: bool,
) -> Response {
    let status = StatusCode::from_u16(rendered.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let hydration = match serde_json::to_string(&rendered.hydration_state) {
        Ok(value) => escape_json_for_html(&value),
        Err(failure) => {
            error!(error = %failure, "hydration state serialization failed");
            return error_page();
        }
    };
    let styles = assets
        .css
        .iter()
        .map(|path| format!(r#"<link rel="stylesheet" href="/static/{path}">"#))
        .collect::<String>();
    let client = if hydrate {
        format!(
            r#"<script src="/static/hydration.js"></script><script id="rill-hydration" type="application/json">{hydration}</script><script type="module" src="/static/{}"></script>{}"#,
            assets.modern_entry,
            if dev_reload { r#"<script src="/static/dev-reload.js"></script>"# } else { "" },
        )
    } else {
        String::new()
    };
    let document = format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">{styles}{}</head><body><div id=\"rill-root\" data-render-id=\"{render_id}\">{}</div>{client}</body></html>",
        rendered.head_html, rendered.body_html,
    );
    html_response(status, document)
}

fn error_page() -> Response {
    html_response(
        StatusCode::INTERNAL_SERVER_ERROR,
        "<!doctype html><html lang=\"en\"><title>Rill error</title><body><h1>Page could not be rendered</h1></body></html>".to_owned(),
    )
}

fn html_response(status: StatusCode, document: String) -> Response {
    let mut response = Response::new(Body::from(document));
    *response.status_mut() = status;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    response
}

pub(crate) fn auth_error(error: AuthError) -> Response {
    let (status, message) = match error {
        AuthError::InvalidCredentials | AuthError::InvalidSession => {
            (StatusCode::UNAUTHORIZED, "authentication required")
        }
        AuthError::Forbidden => (StatusCode::FORBIDDEN, "permission denied"),
        AuthError::RateLimited => (StatusCode::TOO_MANY_REQUESTS, "rate limit reached"),
        AuthError::InvalidInput
        | AuthError::InvalidPairingCode
        | AuthError::PairingExpired
        | AuthError::PairingReplay => (StatusCode::BAD_REQUEST, "invalid request"),
        AuthError::Conflict => (StatusCode::CONFLICT, "resource already exists"),
        AuthError::Disabled => (StatusCode::UNAUTHORIZED, "authentication required"),
        _ => {
            error!(error = %error, "authentication subsystem failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "authentication unavailable",
            )
        }
    };
    api_error(status, message)
}

fn ingestion_error(error: IngestionError) -> Response {
    if matches!(error, IngestionError::Invalid(_)) {
        return api_error(StatusCode::BAD_REQUEST, "invalid source data");
    }
    error!(error = %error, "ingestion subsystem failed");
    api_error(StatusCode::INTERNAL_SERVER_ERROR, "ingestion unavailable")
}

fn intelligence_error(error: IntelligenceError) -> Response {
    match error {
        IntelligenceError::Invalid(_) => {
            api_error(StatusCode::BAD_REQUEST, "invalid intelligence request")
        }
        IntelligenceError::NotFound => api_error(StatusCode::NOT_FOUND, "resource not found"),
        error => {
            error!(error = %error, "intelligence subsystem failed");
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "intelligence unavailable",
            )
        }
    }
}

fn action_error(error: ActionError) -> Response {
    match error {
        ActionError::NotFound => api_error(StatusCode::NOT_FOUND, "action not found"),
        ActionError::InvalidConfig(_) | ActionError::SecretsUnavailable => {
            api_error(StatusCode::BAD_REQUEST, "invalid action configuration")
        }
        error => {
            error!(error = %error, "action subsystem failed");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "action unavailable")
        }
    }
}

fn plugin_error(error: PluginError) -> Response {
    match error {
        PluginError::NotFound => api_error(StatusCode::NOT_FOUND, "plugin not found"),
        PluginError::Disabled | PluginError::InUse => {
            api_error(StatusCode::CONFLICT, "plugin state conflict")
        }
        PluginError::Invalid(_) | PluginError::CapabilityDenied(_) => {
            api_error(StatusCode::BAD_REQUEST, "invalid plugin request")
        }
        PluginError::OutputTooLarge(_) => {
            api_error(StatusCode::PAYLOAD_TOO_LARGE, "plugin output too large")
        }
        error => {
            error!(error = %error, "plugin subsystem failed");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "plugin unavailable")
        }
    }
}

pub(crate) fn api_error(status: StatusCode, message: &'static str) -> Response {
    no_store((status, Json(ErrorResponse { error: message })).into_response())
}

pub(crate) fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

fn with_status(mut response: Response, status: StatusCode) -> Response {
    *response.status_mut() = status;
    response
}

async fn request_correlation(
    mut request: axum::extract::Request,
    next: middleware::Next,
) -> Response {
    let request_id = request
        .headers()
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .filter(|value| {
            !value.is_empty()
                && value.len() <= 64
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        })
        .map(str::to_owned)
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    request.extensions_mut().insert(request_id.clone());
    let span = tracing::info_span!("http_request", %request_id, %method, %path);
    let mut response = next.run(request).instrument(span).await;
    if let Ok(value) = HeaderValue::from_str(&request_id) {
        response.headers_mut().insert("x-request-id", value);
    }
    response
}

async fn security_headers(mut response: Response) -> Response {
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("default-src 'self'; script-src 'self'; style-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'"),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("same-origin"),
    );
    headers.insert("x-frame-options", HeaderValue::from_static("DENY"));
    response
}

fn load_assets(static_dir: &FilePath) -> Result<BrowserAssets> {
    let manifest_path = static_dir.join(".vite/manifest.json");
    let manifest = fs::read_to_string(&manifest_path)
        .with_context(|| format!("read asset manifest: {}", manifest_path.display()))?;
    let entries: BTreeMap<String, ManifestEntry> = serde_json::from_str(&manifest)?;
    let entry = entries
        .get("src/modern-client.tsx")
        .context("asset manifest lacks src/modern-client.tsx")?;
    Ok(BrowserAssets {
        modern_entry: entry.file.clone(),
        css: entry.css.clone(),
    })
}

fn escape_json_for_html(value: &str) -> String {
    value
        .replace('<', "\\u003c")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_secs()).ok())
        .unwrap_or(0)
}

async fn shutdown() {
    #[cfg(unix)]
    {
        let mut terminate = match tokio::signal::unix::signal(
            tokio::signal::unix::SignalKind::terminate(),
        ) {
            Ok(signal) => signal,
            Err(failure) => {
                error!(error = %failure, "failed to install SIGTERM handler");
                let _ = tokio::signal::ctrl_c().await;
                return;
            }
        };
        tokio::select! {
            result = tokio::signal::ctrl_c() => {
                if let Err(failure) = result {
                    error!(error = %failure, "failed to install interrupt handler");
                }
            }
            _ = terminate.recv() => {}
        }
    }
    #[cfg(not(unix))]
    if let Err(failure) = tokio::signal::ctrl_c().await {
        error!(error = %failure, "failed to install interrupt handler");
    }
}
