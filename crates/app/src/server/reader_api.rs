async fn api_create_pairing_code(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<PairDeviceRequest>,
) -> Response {
    if !valid_origin(&headers, &state.trusted_origins) {
        return api_error(StatusCode::FORBIDDEN, "origin rejected");
    }
    let principal = match browser_principal(&state, &headers).await {
        Ok(principal) => principal,
        Err(error) => return auth_error(error),
    };
    if principal.require_browser().is_err() {
        return api_error(StatusCode::FORBIDDEN, "permission denied");
    }
    let Some(csrf) = csrf_header(&headers) else {
        return api_error(StatusCode::FORBIDDEN, "CSRF validation failed");
    };
    if validate_csrf(&state, &principal, &csrf).await.is_err() {
        return api_error(StatusCode::FORBIDDEN, "CSRF validation failed");
    }
    if !state
        .pairing_generation_limiter
        .attempt(&principal.user.id, 5, Duration::from_secs(600))
    {
        return api_error(StatusCode::TOO_MANY_REQUESTS, "pairing limit reached");
    }
    let auth = state.auth.clone();
    let user_id = principal.user.id;
    match tokio::task::spawn_blocking(move || auth.create_pairing_code(&user_id, &request.label))
        .await
    {
        Ok(Ok(pairing)) => no_store(
            Json(PairingResponse {
                code: pairing.code.expose().to_owned(),
                expires_at: pairing.expires_at,
            })
            .into_response(),
        ),
        Ok(Err(error)) => auth_error(error),
        Err(failure) => {
            error!(error = %failure, "pairing generation task failed");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "pairing unavailable")
        }
    }
}

async fn api_reader_devices(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let principal = match browser_principal(&state, &headers).await {
        Ok(principal) => principal,
        Err(error) => return auth_error(error),
    };
    let auth = state.auth.clone();
    let user_id = principal.user.id;
    match tokio::task::spawn_blocking(move || auth.list_reader_devices(&user_id)).await {
        Ok(Ok(devices)) => no_store(
            Json(devices.into_iter().map(device_model).collect::<Vec<_>>()).into_response(),
        ),
        Ok(Err(error)) => auth_error(error),
        Err(failure) => {
            error!(error = %failure, "device listing task failed");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "devices unavailable")
        }
    }
}

async fn api_revoke_reader_device(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    if !valid_origin(&headers, &state.trusted_origins) {
        return api_error(StatusCode::FORBIDDEN, "origin rejected");
    }
    let principal = match browser_principal(&state, &headers).await {
        Ok(principal) => principal,
        Err(error) => return auth_error(error),
    };
    let Some(csrf) = csrf_header(&headers) else {
        return api_error(StatusCode::FORBIDDEN, "CSRF validation failed");
    };
    if validate_csrf(&state, &principal, &csrf).await.is_err() {
        return api_error(StatusCode::FORBIDDEN, "CSRF validation failed");
    }
    let auth = state.auth.clone();
    let user_id = principal.user.id;
    match tokio::task::spawn_blocking(move || auth.revoke_reader_device(&user_id, &device_id)).await
    {
        Ok(Ok(true)) => StatusCode::NO_CONTENT.into_response(),
        Ok(Ok(false)) => api_error(StatusCode::NOT_FOUND, "device not found"),
        Ok(Err(error)) => auth_error(error),
        Err(failure) => {
            error!(error = %failure, "reader revocation task failed");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "revocation unavailable")
        }
    }
}
