async fn reader_feed(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let principal = match reader_or_browser_principal(&state, &headers).await {
        Ok(principal) => principal,
        Err(_) => return Redirect::to("/reader/pair").into_response(),
    };
    let csrf = cookie(&headers, principal_csrf_cookie(&principal)).unwrap_or_default();
    let slug = if principal.kind == SessionKind::Reader {
        let auth = state.auth.clone();
        let user_id = principal.user.id.clone();
        let session_id = principal.session_id.clone();
        tokio::task::spawn_blocking(move || auth.reader_selected_stream(&user_id, &session_id))
            .await
            .ok()
            .and_then(Result::ok)
            .flatten()
            .unwrap_or_else(|| "home".to_owned())
    } else {
        "home".to_owned()
    };
    let page = match load_stream_feed(
        &state,
        &principal.user.id,
        &principal.user.username,
        &slug,
        "reader",
        1,
        20,
    )
    .await
    {
        Ok(page) => page,
        Err(response) => return response,
    };
    render_page(state, "reader-feed", RenderMode::Reader, page, &csrf, false).await
}

async fn login_page(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if browser_principal(&state, &headers).await.is_ok() {
        return Redirect::to("/stream/home").into_response();
    }
    no_store(render_login(state, None).await)
}

async fn login_form(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Form(form): Form<LoginForm>,
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
        return with_status(
            no_store(render_login(state, Some("Too many attempts. Try later.")).await),
            StatusCode::TOO_MANY_REQUESTS,
        );
    }
    let user_agent = user_agent(&headers);
    let ip_summary = ip_summary(peer.ip());
    let auth = state.auth.clone();
    let login = form.login;
    let password = Zeroizing::new(form.password);
    let result = tokio::task::spawn_blocking(move || {
        auth.authenticate(&login, &password, user_agent.as_deref(), Some(&ip_summary))
    })
    .await;
    match result {
        Ok(Ok(session)) => {
            state.login_limiter.clear(&rate_key);
            session_redirect(&state, session)
        }
        Ok(Err(_)) => {
            state
                .metrics
                .observe("authentication", Duration::ZERO, false, 0);
            no_store(render_login(state, Some("Invalid username or password.")).await)
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

async fn render_login(state: AppState, error: Option<&str>) -> Response {
    render_page(
        state,
        "modern-login",
        RenderMode::Modern,
        LoginPageModel {
            title: "Sign in to Rill".to_owned(),
            error: error.map(str::to_owned),
        },
        "",
        false,
    )
    .await
}

async fn reader_stream(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    headers: HeaderMap,
) -> Response {
    let principal = match reader_or_browser_principal(&state, &headers).await {
        Ok(principal) => principal,
        Err(_) => return Redirect::to("/reader/pair").into_response(),
    };
    let csrf = cookie(&headers, principal_csrf_cookie(&principal)).unwrap_or_default();
    let page = match load_stream_feed(
        &state,
        &principal.user.id,
        &principal.user.username,
        &slug,
        "reader",
        1,
        20,
    )
    .await
    {
        Ok(page) => page,
        Err(response) => return response,
    };
    if principal.kind == SessionKind::Reader {
        let auth = state.auth.clone();
        let user_id = principal.user.id;
        let session_id = principal.session_id;
        let selected_slug = slug.clone();
        if let Ok(Err(error)) = tokio::task::spawn_blocking(move || {
            auth.set_reader_selected_stream(&user_id, &session_id, &selected_slug)
        })
        .await
        {
            return auth_error(error);
        }
    }
    render_page(state, "reader-feed", RenderMode::Reader, page, &csrf, false).await
}

async fn reader_page(
    State(state): State<AppState>,
    Path(page): Path<u32>,
    headers: HeaderMap,
) -> Response {
    if page == 0 || page > 5 {
        return api_error(StatusCode::NOT_FOUND, "reader page not found");
    }
    let principal = match reader_or_browser_principal(&state, &headers).await {
        Ok(principal) => principal,
        Err(_) => return Redirect::to("/reader/pair").into_response(),
    };
    let slug = if principal.kind == SessionKind::Reader {
        let auth = state.auth.clone();
        let user_id = principal.user.id.clone();
        let session_id = principal.session_id.clone();
        tokio::task::spawn_blocking(move || auth.reader_selected_stream(&user_id, &session_id))
            .await
            .ok()
            .and_then(Result::ok)
            .flatten()
            .unwrap_or_else(|| "home".to_owned())
    } else {
        "home".to_owned()
    };
    let csrf = cookie(&headers, principal_csrf_cookie(&principal)).unwrap_or_default();
    let page_model = match load_stream_feed(
        &state,
        &principal.user.id,
        &principal.user.username,
        &slug,
        "reader",
        page,
        20,
    )
    .await
    {
        Ok(page) => page,
        Err(response) => return response,
    };
    render_page(
        state,
        "reader-feed",
        RenderMode::Reader,
        page_model,
        &csrf,
        false,
    )
    .await
}

async fn reader_story(
    State(state): State<AppState>,
    Path(story_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let principal = match reader_or_browser_principal(&state, &headers).await {
        Ok(principal) => principal,
        Err(_) => return Redirect::to("/reader/pair").into_response(),
    };
    let csrf = cookie(&headers, principal_csrf_cookie(&principal)).unwrap_or_default();
    render_story_page(state, principal, story_id, true, &csrf).await
}

async fn render_story_page(
    state: AppState,
    principal: Principal,
    story_id: String,
    reader: bool,
    csrf: &str,
) -> Response {
    let intelligence = state.intelligence.clone();
    let user_id = principal.user.id;
    let detail =
        tokio::task::spawn_blocking(move || intelligence.story_detail(&user_id, &story_id)).await;
    let detail = match detail {
        Ok(Ok(detail)) => detail,
        Ok(Err(error)) => return intelligence_error(error),
        Err(failure) => {
            error!(error = %failure, "story loading task failed");
            return api_error(StatusCode::INTERNAL_SERVER_ERROR, "story unavailable");
        }
    };
    let template = if reader {
        "reader-story"
    } else {
        "modern-story"
    };
    let mode = if reader {
        RenderMode::Reader
    } else {
        RenderMode::Modern
    };
    render_page(
        state,
        template,
        mode,
        story_page_model(detail, reader),
        csrf,
        !reader,
    )
    .await
}

async fn reader_story_read(
    State(state): State<AppState>,
    Path(story_id): Path<String>,
    headers: HeaderMap,
    Form(form): Form<ReaderReadForm>,
) -> Response {
    let principal = match reader_or_browser_write_principal(&state, &headers, &form.csrf_token).await {
        Ok(principal) => principal,
        Err(response) => return response,
    };
    let intelligence = state.intelligence.clone();
    let user_id = principal.user.id;
    let task_story_id = story_id.clone();
    let result = tokio::task::spawn_blocking(move || {
        intelligence.set_story_read(&user_id, &task_story_id, form.read)
    })
    .await;
    match result {
        Ok(Ok(())) => Redirect::to(&format!("/reader/story/{story_id}")).into_response(),
        Ok(Err(error)) => intelligence_error(error),
        Err(failure) => {
            error!(error = %failure, "reader read-state task failed");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "read state unavailable")
        }
    }
}

async fn reader_story_variant(
    State(state): State<AppState>,
    Path(story_id): Path<String>,
    headers: HeaderMap,
    Form(form): Form<ReaderVariantForm>,
) -> Response {
    let principal = match reader_or_browser_write_principal(&state, &headers, &form.csrf_token).await {
        Ok(principal) => principal,
        Err(response) => return response,
    };
    let intelligence = state.intelligence.clone();
    let user_id = principal.user.id;
    let document_id = form.document_id;
    let task_story_id = story_id.clone();
    let result = tokio::task::spawn_blocking(move || {
        intelligence.select_story_variant(&user_id, &task_story_id, &document_id)
    })
    .await;
    match result {
        Ok(Ok(())) => Redirect::to(&format!("/reader/story/{story_id}")).into_response(),
        Ok(Err(error)) => intelligence_error(error),
        Err(failure) => {
            error!(error = %failure, "reader variant task failed");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "variant unavailable")
        }
    }
}
