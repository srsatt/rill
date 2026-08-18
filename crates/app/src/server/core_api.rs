async fn api_login(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(request): Json<LoginForm>,
) -> Response {
    if !valid_origin(&headers, &state.public_origin) {
        return api_error(StatusCode::FORBIDDEN, "origin rejected");
    }
    let rate_key = peer.ip().to_string();
    if !state
        .login_limiter
        .attempt(&rate_key, 5, Duration::from_secs(600))
    {
        state
            .metrics
            .observe("authentication", Duration::ZERO, false, 0);
        return api_error(StatusCode::TOO_MANY_REQUESTS, "login limit reached");
    }
    let auth = state.auth.clone();
    let user_agent = user_agent(&headers);
    let ip_summary = ip_summary(peer.ip());
    let login = request.login;
    let password = Zeroizing::new(request.password);
    let result = tokio::task::spawn_blocking(move || {
        auth.authenticate(&login, &password, user_agent.as_deref(), Some(&ip_summary))
    })
    .await;
    match result {
        Ok(Ok(session)) => {
            state.login_limiter.clear(&rate_key);
            let mut response = Json(LoginResponse {
                user: session.user.clone(),
                csrf_token: session.csrf_token.expose().to_owned(),
                expires_at: session.expires_at,
            })
            .into_response();
            set_browser_session_cookies(&state, &mut response, &session);
            no_store(response)
        }
        Ok(Err(_)) => {
            state
                .metrics
                .observe("authentication", Duration::ZERO, false, 0);
            api_error(StatusCode::UNAUTHORIZED, "invalid credentials")
        }
        Err(failure) => {
            error!(error = %failure, "authentication task failed");
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "authentication unavailable",
            )
        }
    }
}

async fn api_logout(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !valid_origin(&headers, &state.public_origin) {
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
    let session_id = principal.session_id;
    match tokio::task::spawn_blocking(move || auth.revoke_session(&user_id, &session_id)).await {
        Ok(Ok(_)) => {}
        Ok(Err(error)) => return auth_error(error),
        Err(failure) => {
            error!(error = %failure, "logout task failed");
            return api_error(StatusCode::INTERNAL_SERVER_ERROR, "logout unavailable");
        }
    }
    let mut response = StatusCode::NO_CONTENT.into_response();
    clear_browser_session_cookies(&state, &mut response);
    no_store(response)
}

async fn api_me(State(state): State<AppState>, headers: HeaderMap) -> Response {
    match browser_principal(&state, &headers).await {
        Ok(principal) => no_store(Json(principal.user).into_response()),
        Err(error) => auth_error(error),
    }
}

async fn api_feed(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let principal = match browser_principal(&state, &headers).await {
        Ok(principal) => principal,
        Err(error) => return auth_error(error),
    };
    let ingestion = state.ingestion.clone();
    let user_id = principal.user.id;
    match tokio::task::spawn_blocking(move || ingestion.latest_feed(&user_id, 50)).await {
        Ok(Ok(stories)) => no_store(Json(stories).into_response()),
        Ok(Err(error)) => ingestion_error(error),
        Err(failure) => {
            error!(error = %failure, "feed task failed");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "feed unavailable")
        }
    }
}

async fn api_search(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<SearchQuery>,
) -> Response {
    let principal = match browser_principal(&state, &headers).await {
        Ok(principal) => principal,
        Err(error) => return auth_error(error),
    };
    if query.q.trim().is_empty() {
        return api_error(StatusCode::BAD_REQUEST, "search query is required");
    }
    let ingestion = state.ingestion.clone();
    let user_id = principal.user.id;
    let limit = query.limit.unwrap_or(20);
    match tokio::task::spawn_blocking(move || ingestion.search(&user_id, &query.q, limit)).await {
        Ok(Ok(stories)) => no_store(Json(stories).into_response()),
        Ok(Err(error)) => ingestion_error(error),
        Err(failure) => {
            error!(error = %failure, "search task failed");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "search unavailable")
        }
    }
}

async fn api_collection_debug(
    State(state): State<AppState>,
    Path(raw_item_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let principal = match browser_principal(&state, &headers).await {
        Ok(principal) => principal,
        Err(error) => return auth_error(error),
    };
    let ingestion = state.ingestion.clone();
    let user_id = principal.user.id;
    let is_admin = principal.user.role == Role::Admin;
    match tokio::task::spawn_blocking(move || {
        ingestion.collection_debug(&raw_item_id, &user_id, is_admin)
    })
    .await
    {
        Ok(Ok(debug)) => no_store(Json(debug).into_response()),
        Ok(Err(error)) => ingestion_error(error),
        Err(failure) => {
            error!(error = %failure, "collection debug task failed");
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "collection debug unavailable",
            )
        }
    }
}

async fn api_collection_control(
    State(state): State<AppState>,
    Path(raw_item_id): Path<String>,
    headers: HeaderMap,
    Json(request): Json<CollectionControlRequest>,
) -> Response {
    let principal = match write_principal(&state, &headers).await {
        Ok(principal) => principal,
        Err(response) => return response,
    };
    let ingestion = state.ingestion.clone();
    let user_id = principal.user.id;
    let is_admin = principal.user.role == Role::Admin;
    let result = tokio::task::spawn_blocking(move || match request.mode.as_str() {
        "rerun" => ingestion.rerun_collection_detection(&raw_item_id, &user_id, is_admin),
        "force_collection" => ingestion.set_collection_override(
            &raw_item_id,
            &user_id,
            is_admin,
            DetectionMode::ForceCollection,
        ),
        "force_single" => ingestion.set_collection_override(
            &raw_item_id,
            &user_id,
            is_admin,
            DetectionMode::ForceSingle,
        ),
        "auto" => ingestion.clear_collection_override(&raw_item_id, &user_id, is_admin),
        _ => Err(IngestionError::Invalid(
            "collection control mode is invalid".into(),
        )),
    })
    .await;
    match result {
        Ok(Ok(job_id)) => {
            no_store((StatusCode::ACCEPTED, Json(JobResponse { job_id })).into_response())
        }
        Ok(Err(error)) => ingestion_error(error),
        Err(failure) => {
            error!(error = %failure, "collection control task failed");
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "collection control unavailable",
            )
        }
    }
}

