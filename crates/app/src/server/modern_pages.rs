const MODERN_STORY_LIMIT: usize = 5;

async fn metrics_endpoint(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
) -> Response {
    if !peer.ip().is_loopback() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let metrics = state.metrics.clone();
    let pool = state.pool.clone();
    match tokio::task::spawn_blocking(move || metrics.render(&pool)).await {
        Ok(Ok(body)) => {
            let mut response = Response::new(Body::from(body));
            response.headers_mut().insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/plain; version=0.0.4; charset=utf-8"),
            );
            response
        }
        Ok(Err(error)) => {
            error!(error = %error, "metrics database query failed");
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
        Err(error) => {
            error!(error = %error, "metrics task failed");
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
    }
}

async fn health_live(State(state): State<AppState>) -> Response {
    let mut response = StatusCode::NO_CONTENT.into_response();
    if let Ok(value) = HeaderValue::from_str(&state.instance_id) {
        response.headers_mut().insert("x-rill-instance", value);
    }
    no_store(response)
}

async fn health_ready(State(state): State<AppState>) -> StatusCode {
    let pool = state.pool.clone();
    match tokio::task::spawn_blocking(move || {
        pool.with_connection(|connection| {
            connection.query_row("SELECT 1", [], |row| row.get::<_, i64>(0))
        })
    })
    .await
    {
        Ok(Ok(1)) => StatusCode::NO_CONTENT,
        _ => StatusCode::SERVICE_UNAVAILABLE,
    }
}

async fn modern_feed(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let principal = match browser_principal(&state, &headers).await {
        Ok(principal) => principal,
        Err(_) => return Redirect::to("/login").into_response(),
    };
    let page = match load_stream_feed(
        &state,
        &principal.user.id,
        &principal.user.username,
        "all",
        "modern",
        1,
        MODERN_STORY_LIMIT,
    )
    .await
    {
        Ok(page) => page,
        Err(response) => return response,
    };
    render_page(state, "modern-feed", RenderMode::Modern, page, "", true).await
}

async fn modern_admin(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let principal = match browser_principal(&state, &headers).await {
        Ok(principal) => principal,
        Err(_) => return Redirect::to("/login").into_response(),
    };
    if principal.require_admin().is_err() {
        return api_error(StatusCode::FORBIDDEN, "permission denied");
    }
    render_page(
        state,
        "modern-admin",
        RenderMode::Modern,
        AdminPageModel {
            title: "Rill administration".into(),
            username: principal.user.username,
        },
        "",
        true,
    )
    .await
}

async fn modern_stream(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    headers: HeaderMap,
) -> Response {
    let principal = match browser_principal(&state, &headers).await {
        Ok(principal) => principal,
        Err(_) => return Redirect::to("/login").into_response(),
    };
    let page = match load_stream_feed(
        &state,
        &principal.user.id,
        &principal.user.username,
        &slug,
        "modern",
        1,
        MODERN_STORY_LIMIT,
    )
    .await
    {
        Ok(page) => page,
        Err(response) => return response,
    };
    render_page(state, "modern-feed", RenderMode::Modern, page, "", true).await
}

async fn modern_story(
    State(state): State<AppState>,
    Path(story_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let principal = match browser_principal(&state, &headers).await {
        Ok(principal) => principal,
        Err(_) => return Redirect::to("/login").into_response(),
    };
    render_story_page(state, principal, story_id, false, "").await
}

async fn modern_search(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<LibraryQuery>,
) -> Response {
    let principal = match browser_principal(&state, &headers).await {
        Ok(principal) => principal,
        Err(_) => return Redirect::to("/login").into_response(),
    };
    let normalized = query
        .q
        .map(|query| query.trim().chars().take(200).collect::<String>())
        .filter(|query| !query.is_empty());
    let topic = query
        .topic
        .map(|topic| topic.trim().chars().take(80).collect::<String>())
        .filter(|topic| !topic.is_empty());
    let stories = if let Some(topic) = topic.as_deref() {
        let intelligence = state.intelligence.clone();
        let user_id = principal.user.id.clone();
        let topic = topic.to_owned();
        match tokio::task::spawn_blocking(move || {
            intelligence.topic_stories(&user_id, &topic, MODERN_STORY_LIMIT)
        })
        .await
        {
            Ok(Ok(stories)) => stories,
            Ok(Err(error)) => return intelligence_error(error),
            Err(_) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, "topic unavailable"),
        }
    } else if let Some(query) = normalized.as_deref() {
        let ingestion = state.ingestion.clone();
        let intelligence = state.intelligence.clone();
        let user_id = principal.user.id.clone();
        let query = query.to_owned();
        match tokio::task::spawn_blocking(move || -> Result<Vec<RankedStory>, String> {
            let ids = ingestion
                .search(&user_id, &query, MODERN_STORY_LIMIT)
                .map_err(|error| error.to_string())?
                .into_iter()
                .map(|hit| hit.story_id)
                .collect::<Vec<_>>();
            intelligence
                .story_summaries(&user_id, &ids, "search")
                .map_err(|error| error.to_string())
        })
        .await
        {
            Ok(Ok(stories)) => stories,
            Ok(Err(error)) => {
                warn!(%error, "search failed");
                return api_error(StatusCode::INTERNAL_SERVER_ERROR, "search unavailable");
            }
            Err(_) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, "search unavailable"),
        }
    } else {
        Vec::new()
    };
    let title = topic
        .as_deref()
        .map_or_else(|| "Search".to_owned(), |topic| format!("Topic: {topic}"));
    render_library(
        state,
        principal,
        &title,
        "search",
        topic.or(normalized),
        stories,
    )
    .await
}

async fn modern_favorites(State(state): State<AppState>, headers: HeaderMap) -> Response {
    modern_library(state, headers, "Favorites", "favorites").await
}

async fn modern_history(State(state): State<AppState>, headers: HeaderMap) -> Response {
    modern_library(state, headers, "Reading history", "history").await
}

async fn modern_sources(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let principal = match browser_principal(&state, &headers).await {
        Ok(principal) => principal,
        Err(_) => return Redirect::to("/login").into_response(),
    };
    let page = SourcesPageModel {
        title: "Sources and streams".into(),
        username: principal.user.username,
        email_available: state.secrets.is_some(),
        telegram_available: state
            .telegram
            .bot_view()
            .is_ok_and(|view| view.active),
    };
    render_page(state, "modern-sources", RenderMode::Modern, page, "", true).await
}

async fn modern_library(
    state: AppState,
    headers: HeaderMap,
    title: &'static str,
    kind: &'static str,
) -> Response {
    let principal = match browser_principal(&state, &headers).await {
        Ok(principal) => principal,
        Err(_) => return Redirect::to("/login").into_response(),
    };
    let intelligence = state.intelligence.clone();
    let user_id = principal.user.id.clone();
    let stories = match tokio::task::spawn_blocking(move || {
        intelligence.library_stories(&user_id, kind, MODERN_STORY_LIMIT)
    })
    .await
    {
        Ok(Ok(stories)) => stories,
        Ok(Err(error)) => return intelligence_error(error),
        Err(_) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, "library unavailable"),
    };
    render_library(state, principal, title, kind, None, stories).await
}

async fn render_library(
    state: AppState,
    principal: Principal,
    title: &str,
    kind: &str,
    query: Option<String>,
    stories: Vec<RankedStory>,
) -> Response {
    render_page(
        state,
        "modern-library",
        RenderMode::Modern,
        LibraryPageModel {
            title: title.into(),
            username: principal.user.username,
            kind: kind.into(),
            query,
            stories: stories.into_iter().map(story_card).collect(),
        },
        "",
        true,
    )
    .await
}
