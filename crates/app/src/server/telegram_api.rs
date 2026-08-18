async fn api_create_telegram_source(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateTelegramSourceRequest>,
) -> Response {
    let principal = match write_principal(&state, &headers).await {
        Ok(principal) => principal,
        Err(response) => return response,
    };
    let username = match rill_source_telegram::normalize_username(&request.username) {
        Ok(username) => username,
        Err(_) => return api_error(StatusCode::BAD_REQUEST, "invalid Telegram channel username"),
    };
    let ingestion = state.ingestion.clone();
    let user_id = principal.user.id;
    match tokio::task::spawn_blocking(move || {
        ingestion.ensure_telegram_subscription(&user_id, &username, None, None)
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
                Json(SourceRegistrationResponse {
                    id: result.source.id,
                    visibility_scope: result.source.visibility_scope,
                }),
            )
                .into_response(),
        ),
        Ok(Err(error)) => ingestion_error(error),
        Err(error) => {
            error!(error = %error, "Telegram subscription task failed");
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Telegram subscription unavailable",
            )
        }
    }
}

async fn api_telegram_binding(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let principal = match browser_principal(&state, &headers).await {
        Ok(principal) => principal,
        Err(error) => return auth_error(error),
    };
    match state.telegram.binding(&principal.user.id) {
        Ok(binding) => no_store(Json(binding).into_response()),
        Err(error) => {
            error!(error = %error, "Telegram binding status failed");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "Telegram unavailable")
        }
    }
}

async fn api_telegram_binding_challenge(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    let principal = match write_principal(&state, &headers).await {
        Ok(principal) => principal,
        Err(response) => return response,
    };
    match state.telegram.create_binding_challenge(&principal.user.id) {
        Ok(challenge) => no_store((StatusCode::CREATED, Json(challenge)).into_response()),
        Err(error) => {
            warn!(error = %error, "Telegram binding challenge unavailable");
            api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "Telegram bot is not configured",
            )
        }
    }
}

async fn api_telegram_unbind(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let principal = match write_principal(&state, &headers).await {
        Ok(principal) => principal,
        Err(response) => return response,
    };
    match state.telegram.unbind(&principal.user.id) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => {
            error!(error = %error, "Telegram unbind failed");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "Telegram unavailable")
        }
    }
}
