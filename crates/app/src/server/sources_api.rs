async fn api_sources(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let principal = match browser_principal(&state, &headers).await {
        Ok(principal) => principal,
        Err(error) => return auth_error(error),
    };
    let ingestion = state.ingestion.clone();
    let user_id = principal.user.id;
    let is_admin = principal.user.role == Role::Admin;
    match tokio::task::spawn_blocking(move || ingestion.list_sources(&user_id, is_admin)).await {
        Ok(Ok(sources)) => no_store(Json(sources).into_response()),
        Ok(Err(error)) => ingestion_error(error),
        Err(failure) => {
            error!(error = %failure, "source listing task failed");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "sources unavailable")
        }
    }
}

async fn api_quick_add_source(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<QuickAddSourceRequest>,
) -> Response {
    let principal = match write_principal(&state, &headers).await {
        Ok(principal) => principal,
        Err(response) => return response,
    };
    let input = request.input.trim();
    if input.is_empty() || input.chars().count() > 2_048 {
        return api_error(StatusCode::BAD_REQUEST, "paste a valid source link");
    }
    if let Some(username) = telegram_username_from_input(input) {
        let ingestion = state.ingestion.clone();
        let user_id = principal.user.id;
        let task_username = username.clone();
        return match tokio::task::spawn_blocking(move || {
            ingestion.ensure_telegram_subscription(&user_id, &task_username, None, None)
        })
        .await
        {
            Ok(Ok(result)) => no_store(
                (
                    if result.created_subscription {
                        StatusCode::CREATED
                    } else {
                        StatusCode::OK
                    },
                    Json(QuickAddSourceResponse {
                        id: result.source.id,
                        kind: "telegram".into(),
                        name: format!("@{username}"),
                        url: format!("https://t.me/{username}"),
                    }),
                )
                    .into_response(),
            ),
            Ok(Err(error)) => ingestion_error(error),
            Err(error) => {
                error!(error = %error, "quick Telegram subscription task failed");
                api_error(StatusCode::INTERNAL_SERVER_ERROR, "source registration unavailable")
            }
        };
    }

    let url = match Url::parse(input) {
        Ok(url) if matches!(url.scheme(), "http" | "https") => url,
        _ => return api_error(StatusCode::BAD_REQUEST, "paste an RSS, website, or Telegram link"),
    };
    let response = match state
        .connector_context
        .http
        .get(&url, &ConditionalHeaders::default())
        .await
    {
        Ok(response) if !response.not_modified => response,
        _ => return api_error(StatusCode::BAD_REQUEST, "source link could not be fetched"),
    };
    let direct_feed = parse_feed(&response.body, &response.final_url, 1).is_ok();
    let (feed_url, page_title) = if direct_feed {
        (response.final_url, None)
    } else {
        let html = String::from_utf8_lossy(&response.body);
        let Some(discovered) = discover_feed(&html, &response.final_url) else {
            return api_error(StatusCode::BAD_REQUEST, "no RSS or Atom feed found on that page");
        };
        let feed_response = match state
            .connector_context
            .http
            .get(&discovered.url, &ConditionalHeaders::default())
            .await
        {
            Ok(response) if !response.not_modified => response,
            _ => return api_error(StatusCode::BAD_REQUEST, "discovered feed could not be fetched"),
        };
        if parse_feed(&feed_response.body, &feed_response.final_url, 1).is_err() {
            return api_error(StatusCode::BAD_REQUEST, "discovered link is not a valid feed");
        }
        (feed_response.final_url, discovered.page_title)
    };
    let name = page_title.unwrap_or_else(|| source_name_from_url(&feed_url));
    let ingestion = state.ingestion.clone();
    let user_id = principal.user.id;
    let is_admin = principal.user.role == Role::Admin;
    let lookup_user_id = user_id.clone();
    let lookup_ingestion = ingestion.clone();
    if let Ok(Ok(existing)) = tokio::task::spawn_blocking(move || {
        lookup_ingestion.list_rss_feeds(&lookup_user_id, is_admin)
    })
    .await
        && let Some(existing) = existing
            .into_iter()
            .find(|existing| existing.xml_url == feed_url.as_str())
    {
        return no_store(
            (
                StatusCode::OK,
                Json(QuickAddSourceResponse {
                    id: existing.source_id,
                    kind: "rss".into(),
                    name: existing.name,
                    url: existing.xml_url,
                }),
            )
                .into_response(),
        );
    }
    let config = serde_json::json!({
        "url": feed_url,
        "pollIntervalSeconds": default_poll_interval(),
        "enabled": true,
        "shared": false,
    });
    let task_name = name.clone();
    let registration = tokio::task::spawn_blocking(move || {
        ingestion.register_source("rss", &task_name, Some(&user_id), false, &config)
    })
    .await;
    match registration {
        Ok(Ok(registration)) => no_store(
            (
                StatusCode::CREATED,
                Json(QuickAddSourceResponse {
                    id: registration.id,
                    kind: "rss".into(),
                    name,
                    url: feed_url.to_string(),
                }),
            )
                .into_response(),
        ),
        Ok(Err(error)) => ingestion_error(error),
        Err(error) => {
            error!(error = %error, "quick RSS registration task failed");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "source registration unavailable")
        }
    }
}

fn telegram_username_from_input(input: &str) -> Option<String> {
    if input.starts_with('@') {
        return rill_source_telegram::normalize_username(input).ok();
    }
    let url = Url::parse(input).ok()?;
    let host = url.host_str()?.trim_start_matches("www.");
    if !matches!(host, "t.me" | "telegram.me") {
        return None;
    }
    let mut segments = url.path_segments()?.filter(|segment| !segment.is_empty());
    let first = segments.next()?;
    let username = if first == "s" { segments.next()? } else { first };
    rill_source_telegram::normalize_username(username).ok()
}

fn source_name_from_url(url: &Url) -> String {
    url.host_str()
        .map(|host| host.trim_start_matches("www.").to_owned())
        .filter(|host| !host.is_empty())
        .unwrap_or_else(|| "New feed".into())
}

async fn api_create_rss_source(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateRssSourceRequest>,
) -> Response {
    let principal = match write_principal(&state, &headers).await {
        Ok(principal) => principal,
        Err(response) => return response,
    };
    if request.shared && principal.user.role != Role::Admin {
        return api_error(
            StatusCode::FORBIDDEN,
            "admin role required for shared sources",
        );
    }
    let config = serde_json::json!({
        "url": request.url,
        "pollIntervalSeconds": request.poll_interval_seconds,
        "enabled": true,
        "shared": request.shared,
    });
    let validation = match RssConnector
        .validate(&state.connector_context, &config)
        .await
    {
        Ok(validation) => validation,
        Err(error) => {
            error!(error = %error, "RSS source validation failed");
            return api_error(StatusCode::BAD_REQUEST, "invalid RSS source");
        }
    };
    if !validation.valid {
        return api_error(StatusCode::BAD_REQUEST, "invalid RSS source");
    }
    let ingestion = state.ingestion.clone();
    let user_id = principal.user.id;
    let name = request.name;
    let shared = request.shared;
    match tokio::task::spawn_blocking(move || {
        ingestion.register_source("rss", &name, Some(&user_id), shared, &config)
    })
    .await
    {
        Ok(Ok(registration)) => no_store(
            (
                StatusCode::CREATED,
                Json(SourceRegistrationResponse {
                    id: registration.id,
                    visibility_scope: registration.visibility_scope,
                }),
            )
                .into_response(),
        ),
        Ok(Err(error)) => ingestion_error(error),
        Err(failure) => {
            error!(error = %failure, "source registration task failed");
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "source registration unavailable",
            )
        }
    }
}

async fn api_export_opml(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let principal = match browser_principal(&state, &headers).await {
        Ok(principal) => principal,
        Err(error) => return auth_error(error),
    };
    let ingestion = state.ingestion.clone();
    let user_id = principal.user.id;
    let is_admin = principal.user.role == Role::Admin;
    let feeds =
        tokio::task::spawn_blocking(move || ingestion.list_rss_feeds(&user_id, is_admin)).await;
    let feeds = match feeds {
        Ok(Ok(feeds)) => feeds,
        Ok(Err(error)) => return ingestion_error(error),
        Err(failure) => {
            error!(error = %failure, "OPML export task failed");
            return api_error(StatusCode::INTERNAL_SERVER_ERROR, "OPML export unavailable");
        }
    };
    let document = export_opml(
        "Rill subscriptions",
        &feeds
            .into_iter()
            .map(|feed| OpmlFeed {
                title: feed.name,
                xml_url: feed.xml_url,
                html_url: None,
            })
            .collect::<Vec<_>>(),
    );
    let mut response = Response::new(Body::from(document));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/x-opml; charset=utf-8"),
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("attachment; filename=\"rill-subscriptions.opml\""),
    );
    no_store(response)
}

async fn api_import_opml(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<SharedQuery>,
    body: Bytes,
) -> Response {
    let principal = match write_principal(&state, &headers).await {
        Ok(principal) => principal,
        Err(response) => return response,
    };
    if query.shared && principal.user.role != Role::Admin {
        return api_error(
            StatusCode::FORBIDDEN,
            "admin role required for shared sources",
        );
    }
    let feeds = match import_opml(&body, 500) {
        Ok(feeds) if !feeds.is_empty() => feeds,
        Ok(_) => return api_error(StatusCode::BAD_REQUEST, "OPML contains no feeds"),
        Err(error) => {
            warn!(error = %error, "OPML import rejected");
            return api_error(StatusCode::BAD_REQUEST, "invalid OPML document");
        }
    };
    let mut registrations = Vec::with_capacity(feeds.len());
    for feed in feeds {
        let config = serde_json::json!({
            "url": feed.xml_url,
            "pollIntervalSeconds": default_poll_interval(),
            "enabled": true,
            "shared": query.shared,
        });
        match RssConnector
            .validate(&state.connector_context, &config)
            .await
        {
            Ok(validation) if validation.valid => {}
            _ => return api_error(StatusCode::BAD_REQUEST, "OPML contains an invalid feed URL"),
        }
        let ingestion = state.ingestion.clone();
        let user_id = principal.user.id.clone();
        let name = feed.title;
        let shared = query.shared;
        let registration = tokio::task::spawn_blocking(move || {
            ingestion.register_source("rss", &name, Some(&user_id), shared, &config)
        })
        .await;
        match registration {
            Ok(Ok(registration)) => registrations.push(SourceRegistrationResponse {
                id: registration.id,
                visibility_scope: registration.visibility_scope,
            }),
            Ok(Err(error)) => return ingestion_error(error),
            Err(failure) => {
                error!(error = %failure, "OPML source registration task failed");
                return api_error(StatusCode::INTERNAL_SERVER_ERROR, "OPML import unavailable");
            }
        }
    }
    no_store((StatusCode::CREATED, Json(registrations)).into_response())
}

async fn api_source_enabled(
    State(state): State<AppState>,
    Path(source_id): Path<String>,
    headers: HeaderMap,
    Json(request): Json<SourceEnabledRequest>,
) -> Response {
    let principal = match write_principal(&state, &headers).await {
        Ok(principal) => principal,
        Err(response) => return response,
    };
    let ingestion = state.ingestion.clone();
    let user_id = principal.user.id;
    let is_admin = principal.user.role == Role::Admin;
    match tokio::task::spawn_blocking(move || {
        ingestion.set_source_enabled(&source_id, &user_id, is_admin, request.enabled)
    })
    .await
    {
        Ok(Ok(())) => StatusCode::NO_CONTENT.into_response(),
        Ok(Err(error)) => ingestion_error(error),
        Err(failure) => {
            error!(error = %failure, "source state task failed");
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "source update unavailable",
            )
        }
    }
}

async fn api_source_processing_prompt(
    State(state): State<AppState>,
    Path(source_id): Path<String>,
    headers: HeaderMap,
    Json(request): Json<SourceProcessingPromptRequest>,
) -> Response {
    let principal = match write_principal(&state, &headers).await {
        Ok(principal) => principal,
        Err(response) => return response,
    };
    let ingestion = state.ingestion.clone();
    let user_id = principal.user.id;
    let is_admin = principal.user.role == Role::Admin;
    match tokio::task::spawn_blocking(move || {
        ingestion.set_source_processing_prompt(
            &source_id,
            &user_id,
            is_admin,
            &request.prompt,
        )
    })
    .await
    {
        Ok(Ok(())) => StatusCode::NO_CONTENT.into_response(),
        Ok(Err(error)) => ingestion_error(error),
        Err(failure) => {
            error!(error = %failure, "source processing instructions task failed");
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "source instructions update unavailable",
            )
        }
    }
}

async fn api_poll_source(
    State(state): State<AppState>,
    Path(source_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let principal = match write_principal(&state, &headers).await {
        Ok(principal) => principal,
        Err(response) => return response,
    };
    let ingestion = state.ingestion.clone();
    let user_id = principal.user.id;
    let is_admin = principal.user.role == Role::Admin;
    match tokio::task::spawn_blocking(move || {
        if !ingestion.can_manage_source(&source_id, &user_id, is_admin)? {
            return Ok(false);
        }
        ingestion.schedule_poll(&source_id, None)?;
        Ok(true)
    })
    .await
    {
        Ok(Ok(true)) => StatusCode::ACCEPTED.into_response(),
        Ok(Ok(false)) => api_error(StatusCode::NOT_FOUND, "source not found"),
        Ok(Err(error)) => ingestion_error(error),
        Err(error) => {
            error!(error = %error, "source polling task failed");
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "source polling unavailable",
            )
        }
    }
}

async fn api_remove_source(
    State(state): State<AppState>,
    Path(source_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let principal = match write_principal(&state, &headers).await {
        Ok(principal) => principal,
        Err(response) => return response,
    };
    let ingestion = state.ingestion.clone();
    let user_id = principal.user.id;
    let is_admin = principal.user.role == Role::Admin;
    let removed = tokio::task::spawn_blocking(move || {
        ingestion.remove_source(&source_id, &user_id, is_admin)
    })
    .await;
    match removed {
        Ok(Ok(secret_id)) => {
            if let (Some(secrets), Some(secret_id)) = (&state.secrets, secret_id)
                && let Err(error) = secrets.delete(&secret_id)
            {
                warn!(error = %error, "source removed but orphan secret cleanup failed");
            }
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(Err(error)) => ingestion_error(error),
        Err(failure) => {
            error!(error = %failure, "source removal task failed");
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "source removal unavailable",
            )
        }
    }
}

async fn api_create_email_source(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateEmailSourceRequest>,
) -> Response {
    let principal = match write_principal(&state, &headers).await {
        Ok(principal) => principal,
        Err(response) => return response,
    };
    if request.poll_interval_seconds < 60
        || request.password.is_empty()
        || request.host.trim().is_empty()
        || request.username.trim().is_empty()
    {
        return api_error(StatusCode::BAD_REQUEST, "invalid email source");
    }
    let Some(secrets) = state.secrets.clone() else {
        return api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "encrypted secret storage is not configured",
        );
    };
    let ingestion = state.ingestion.clone();
    let user_id = principal.user.id;
    let password = Zeroizing::new(request.password);
    let result = tokio::task::spawn_blocking(move || {
        let secret_id = secrets
            .put(Some(&user_id), "email-password", password.as_bytes())
            .map_err(|error| error.to_string())?;
        let config = serde_json::json!({
            "host": request.host,
            "port": request.port,
            "username": request.username,
            "mailbox": request.mailbox,
            "folders": request.folders,
            "markAsRead": request.mark_as_read,
            "passwordSecretId": secret_id,
            "pollIntervalSeconds": request.poll_interval_seconds,
            "enabled": true,
        });
        match ingestion.register_source("email", &request.name, Some(&user_id), false, &config) {
            Ok(registration) => Ok(SourceRegistrationResponse {
                id: registration.id,
                visibility_scope: registration.visibility_scope,
            }),
            Err(error) => {
                let _ = secrets.delete(&secret_id);
                Err(error.to_string())
            }
        }
    })
    .await;
    match result {
        Ok(Ok(registration)) => no_store((StatusCode::CREATED, Json(registration)).into_response()),
        Ok(Err(failure)) => {
            error!(error = %failure, "email source registration failed");
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "email source registration unavailable",
            )
        }
        Err(failure) => {
            error!(error = %failure, "email source registration task failed");
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "email source registration unavailable",
            )
        }
    }
}
