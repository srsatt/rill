async fn reader_preferences(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let principal = match reader_or_browser_principal(&state, &headers).await {
        Ok(principal) => principal,
        Err(_) => return Redirect::to("/reader/pair").into_response(),
    };
    let streams = match state.intelligence.list_streams(&principal.user.id) {
        Ok(streams) => streams,
        Err(error) => return intelligence_error(error),
    };
    let active_stream = if principal.kind == SessionKind::Reader {
        let auth = state.auth.clone();
        let user_id = principal.user.id.clone();
        let session_id = principal.session_id.clone();
        tokio::task::spawn_blocking(move || auth.reader_selected_stream(&user_id, &session_id))
            .await
            .ok()
            .and_then(Result::ok)
            .flatten()
            .unwrap_or_else(|| "all".to_owned())
    } else {
        "all".to_owned()
    };
    let csrf = cookie(&headers, principal_csrf_cookie(&principal)).unwrap_or_default();
    render_page(
        state,
        "reader-settings",
        RenderMode::Reader,
        ReaderPreferencesPageModel {
            title: "Reader settings".to_owned(),
            username: principal.user.username,
            streams: streams
                .into_iter()
                .map(|stream| StreamLink {
                    name: stream.name,
                    slug: stream.slug,
                })
                .collect(),
            active_stream,
        },
        &csrf,
        false,
    )
    .await
}

async fn reader_logout(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<CsrfForm>,
) -> Response {
    let principal = match reader_or_browser_write_principal(&state, &headers, &form.csrf_token).await {
        Ok(principal) => principal,
        Err(response) => return response,
    };
    if principal.kind == SessionKind::Browser {
        return Redirect::to("/stream/all").into_response();
    }
    let auth = state.auth.clone();
    let user_id = principal.user.id;
    let session_id = principal.session_id;
    match tokio::task::spawn_blocking(move || auth.revoke_reader_device(&user_id, &session_id))
        .await
    {
        Ok(Ok(_)) => {
            let mut response = Redirect::to("/reader/pair").into_response();
            clear_reader_session_cookies(&state, &mut response);
            no_store(response)
        }
        Ok(Err(error)) => auth_error(error),
        Err(failure) => {
            error!(error = %failure, "reader logout task failed");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "logout unavailable")
        }
    }
}

async fn reader_story_feedback(
    State(state): State<AppState>,
    Path(story_id): Path<String>,
    headers: HeaderMap,
    Form(form): Form<ReaderFeedbackForm>,
) -> Response {
    if !valid_origin(&headers, &state.public_origin) {
        return api_error(StatusCode::FORBIDDEN, "origin rejected");
    }
    let principal = match reader_or_browser_write_principal(&state, &headers, &form.csrf_token).await {
        Ok(principal) => principal,
        Err(response) => return response,
    };
    let intelligence = state.intelligence.clone();
    let actions = state.actions.clone();
    let user_id = principal.user.id;
    let feedback = form.feedback;
    match tokio::task::spawn_blocking(move || {
        let event_id = intelligence.record_feedback(&user_id, &story_id, feedback, "reader")?;
        if feedback == FeedbackKind::Favorite
            && let Err(failure) = actions.enqueue_favorite(&user_id, &event_id)
        {
            warn!(error = %failure, "favorite persisted but action enqueue failed");
        }
        Ok::<_, IntelligenceError>(event_id)
    })
    .await
    {
        Ok(Ok(_)) => Redirect::to("/reader").into_response(),
        Ok(Err(error)) => intelligence_error(error),
        Err(failure) => {
            error!(error = %failure, "reader feedback task failed");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "feedback unavailable")
        }
    }
}

async fn reader_pair_page(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if reader_principal(&state, &headers).await.is_ok() {
        return Redirect::to("/reader").into_response();
    }
    let csrf = match new_secret() {
        Ok(secret) => secret,
        Err(_) => {
            return api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "secure randomness unavailable",
            );
        }
    };
    let mut response = render_reader_pair(state.clone(), None, csrf.expose()).await;
    append_cookie(
        &mut response,
        cookie_header(
            pair_csrf_cookie(),
            csrf.expose(),
            false,
            true,
            state.secure_cookies,
            600,
        ),
    );
    no_store(response)
}

async fn reader_pair_form(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Form(form): Form<PairForm>,
) -> Response {
    if !valid_origin(&headers, &state.public_origin)
        || !constant_time_equal(
            cookie(&headers, pair_csrf_cookie()).as_deref(),
            &form.csrf_token,
        )
    {
        return api_error(StatusCode::FORBIDDEN, "pairing request rejected");
    }
    let user_agent = user_agent(&headers);
    let ip_summary = ip_summary(peer.ip());
    let attempt_key = peer.ip().to_string();
    let auth = state.auth.clone();
    let code = form.code;
    let result = tokio::task::spawn_blocking(move || {
        auth.consume_pairing_code(
            &code,
            &attempt_key,
            user_agent.as_deref(),
            Some(&ip_summary),
        )
    })
    .await;
    match result {
        Ok(Ok(session)) => reader_session_redirect(&state, session),
        Ok(Err(error)) => {
            state.metrics.observe("pairing", Duration::ZERO, false, 0);
            let message = match error {
                AuthError::PairingExpired => "That pairing code expired.",
                AuthError::PairingReplay => "That pairing code was already used.",
                AuthError::RateLimited => "Too many attempts. Try later.",
                _ => "Invalid pairing code.",
            };
            no_store(render_reader_pair(state, Some(message), &form.csrf_token).await)
        }
        Err(failure) => {
            error!(error = %failure, "reader pairing task failed");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "pairing unavailable")
        }
    }
}

async fn render_reader_pair(state: AppState, error: Option<&str>, csrf: &str) -> Response {
    render_page(
        state,
        "reader-pair",
        RenderMode::Reader,
        ReaderPairPageModel {
            title: "Pair this reader".to_owned(),
            error: error.map(str::to_owned),
        },
        csrf,
        false,
    )
    .await
}

async fn reader_settings(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let principal = match browser_principal(&state, &headers).await {
        Ok(principal) => principal,
        Err(_) => return Redirect::to("/login").into_response(),
    };
    let csrf = cookie(&headers, browser_csrf_cookie()).unwrap_or_default();
    no_store(render_reader_settings(state, principal, &csrf, None).await)
}

async fn settings_pair_reader(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<PairDeviceForm>,
) -> Response {
    if !valid_origin(&headers, &state.public_origin) {
        return api_error(StatusCode::FORBIDDEN, "origin rejected");
    }
    let principal = match browser_principal(&state, &headers).await {
        Ok(principal) => principal,
        Err(_) => return Redirect::to("/login").into_response(),
    };
    if validate_csrf(&state, &principal, &form.csrf_token)
        .await
        .is_err()
    {
        return api_error(StatusCode::FORBIDDEN, "CSRF validation failed");
    }
    if !state
        .pairing_generation_limiter
        .attempt(&principal.user.id, 5, Duration::from_secs(600))
    {
        return api_error(StatusCode::TOO_MANY_REQUESTS, "pairing limit reached");
    }
    let auth = state.auth.clone();
    let user_id = principal.user.id.clone();
    let label = form.label;
    let pairing =
        tokio::task::spawn_blocking(move || auth.create_pairing_code(&user_id, &label)).await;
    match pairing {
        Ok(Ok(pairing)) => no_store(
            render_reader_settings(
                state,
                principal,
                &form.csrf_token,
                Some((pairing.code.expose().to_owned(), pairing.expires_at)),
            )
            .await,
        ),
        Ok(Err(error)) => auth_error(error),
        Err(failure) => {
            error!(error = %failure, "pairing generation task failed");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "pairing unavailable")
        }
    }
}

async fn settings_change_password(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<PasswordForm>,
) -> Response {
    if !valid_origin(&headers, &state.public_origin) {
        return api_error(StatusCode::FORBIDDEN, "origin rejected");
    }
    let principal = match browser_principal(&state, &headers).await {
        Ok(principal) => principal,
        Err(_) => return Redirect::to("/login").into_response(),
    };
    if validate_csrf(&state, &principal, &form.csrf_token)
        .await
        .is_err()
    {
        return api_error(StatusCode::FORBIDDEN, "CSRF validation failed");
    }
    let old_password = Zeroizing::new(form.old_password);
    let new_password = Zeroizing::new(form.new_password);
    let auth = state.auth.clone();
    let user_id = principal.user.id;
    match tokio::task::spawn_blocking(move || {
        auth.change_password(&user_id, old_password.as_str(), new_password.as_str())
    })
    .await
    {
        Ok(Ok(())) => {
            let mut response = Redirect::to("/login").into_response();
            clear_browser_session_cookies(&state, &mut response);
            no_store(response)
        }
        Ok(Err(error)) => auth_error(error),
        Err(_) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "password change unavailable",
        ),
    }
}

async fn settings_revoke_reader(
    State(state): State<AppState>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
    Form(form): Form<CsrfForm>,
) -> Response {
    if !valid_origin(&headers, &state.public_origin) {
        return api_error(StatusCode::FORBIDDEN, "origin rejected");
    }
    let principal = match browser_principal(&state, &headers).await {
        Ok(principal) => principal,
        Err(_) => return Redirect::to("/login").into_response(),
    };
    if validate_csrf(&state, &principal, &form.csrf_token)
        .await
        .is_err()
    {
        return api_error(StatusCode::FORBIDDEN, "CSRF validation failed");
    }
    let auth = state.auth.clone();
    let user_id = principal.user.id;
    match tokio::task::spawn_blocking(move || auth.revoke_reader_device(&user_id, &device_id)).await
    {
        Ok(Ok(_)) => Redirect::to("/settings/readers").into_response(),
        Ok(Err(error)) => auth_error(error),
        Err(failure) => {
            error!(error = %failure, "reader revocation task failed");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "revocation unavailable")
        }
    }
}

async fn render_reader_settings(
    state: AppState,
    principal: Principal,
    csrf: &str,
    pairing: Option<(String, i64)>,
) -> Response {
    let auth = state.auth.clone();
    let user_id = principal.user.id.clone();
    let devices = tokio::task::spawn_blocking(move || auth.list_reader_devices(&user_id)).await;
    let devices = match devices {
        Ok(Ok(devices)) => devices.into_iter().map(device_model).collect(),
        Ok(Err(error)) => return auth_error(error),
        Err(failure) => {
            error!(error = %failure, "device listing task failed");
            return api_error(StatusCode::INTERNAL_SERVER_ERROR, "devices unavailable");
        }
    };
    let (new_pairing_code, pairing_expires_at) =
        pairing.map_or((None, None), |(code, expires)| (Some(code), Some(expires)));
    render_page(
        state,
        "modern-reader-settings",
        RenderMode::Modern,
        ReaderSettingsPageModel {
            title: "Account settings".to_owned(),
            username: principal.user.username,
            devices,
            new_pairing_code,
            pairing_expires_at,
        },
        csrf,
        true,
    )
    .await
}
