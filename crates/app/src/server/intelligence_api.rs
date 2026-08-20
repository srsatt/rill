async fn api_streams(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let principal = match browser_principal(&state, &headers).await {
        Ok(principal) => principal,
        Err(error) => return auth_error(error),
    };
    let intelligence = state.intelligence.clone();
    let user_id = principal.user.id;
    match tokio::task::spawn_blocking(move || intelligence.list_streams(&user_id)).await {
        Ok(Ok(streams)) => no_store(Json(streams).into_response()),
        Ok(Err(error)) => intelligence_error(error),
        Err(failure) => {
            error!(error = %failure, "stream listing task failed");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "streams unavailable")
        }
    }
}

async fn api_preferences(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let principal = match browser_principal(&state, &headers).await {
        Ok(principal) => principal,
        Err(error) => return auth_error(error),
    };
    let intelligence = state.intelligence.clone();
    let user_id = principal.user.id;
    match tokio::task::spawn_blocking(move || intelligence.user_preferences(&user_id)).await {
        Ok(Ok(preferences)) => no_store(Json(preferences).into_response()),
        Ok(Err(error)) => intelligence_error(error),
        Err(error) => {
            tracing::error!(%error, "preference listing task failed");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "preferences unavailable")
        }
    }
}

async fn api_update_preferences(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(preferences): Json<UserPreferences>,
) -> Response {
    let principal = match write_principal(&state, &headers).await {
        Ok(principal) => principal,
        Err(response) => return response,
    };
    let intelligence = state.intelligence.clone();
    let user_id = principal.user.id;
    match tokio::task::spawn_blocking(move || {
        intelligence.update_user_preferences(&user_id, &preferences)
    })
    .await
    {
        Ok(Ok(preferences)) => no_store(Json(preferences).into_response()),
        Ok(Err(error)) => intelligence_error(error),
        Err(error) => {
            tracing::error!(%error, "preference update task failed");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "preferences unavailable")
        }
    }
}

async fn api_create_stream(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateStreamRequest>,
) -> Response {
    let principal = match write_principal(&state, &headers).await {
        Ok(principal) => principal,
        Err(response) => return response,
    };
    match state
        .intelligence
        .create_stream(
            &principal.user.id,
            &rill_intelligence::CreateStreamInput {
                name: request.name,
                slug: request.slug,
                icon: request.icon,
                filter: request.filter,
                semantic_description: request.semantic_description,
                ranking_instruction: request.ranking_instruction,
            },
        )
        .await
    {
        Ok(stream) => no_store((StatusCode::CREATED, Json(stream)).into_response()),
        Err(error) => intelligence_error(error),
    }
}

async fn api_update_stream(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    headers: HeaderMap,
    Json(request): Json<UpdateStreamRequest>,
) -> Response {
    let principal = match write_principal(&state, &headers).await {
        Ok(principal) => principal,
        Err(response) => return response,
    };
    match state
        .intelligence
        .update_stream(
            &principal.user.id,
            &slug,
            &rill_intelligence::UpdateStreamInput {
                name: request.name,
                icon: request.icon,
                filter: request.filter,
                semantic_description: request.semantic_description,
                ranking_instruction: request.ranking_instruction,
            },
        )
        .await
    {
        Ok(stream) => no_store(Json(stream).into_response()),
        Err(error) => intelligence_error(error),
    }
}

async fn api_delete_stream(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    headers: HeaderMap,
) -> Response {
    let principal = match write_principal(&state, &headers).await {
        Ok(principal) => principal,
        Err(response) => return response,
    };
    match state
        .intelligence
        .delete_stream(&principal.user.id, &slug)
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => intelligence_error(error),
    }
}

async fn api_reorder_streams(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ReorderStreamsRequest>,
) -> Response {
    let principal = match write_principal(&state, &headers).await {
        Ok(principal) => principal,
        Err(response) => return response,
    };
    match state
        .intelligence
        .reorder_streams(&principal.user.id, &request.slugs)
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => intelligence_error(error),
    }
}

async fn api_stream_feed(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    headers: HeaderMap,
    Query(query): Query<LimitQuery>,
) -> Response {
    let principal = match browser_principal(&state, &headers).await {
        Ok(principal) => principal,
        Err(error) => return auth_error(error),
    };
    let intelligence = state.intelligence.clone();
    let user_id = principal.user.id;
    let limit = query.limit.unwrap_or(25).clamp(1, 50);
    let offset = query.offset.min(100);
    let requested = offset.saturating_add(limit).saturating_add(1).min(101);
    match tokio::task::spawn_blocking(move || {
        intelligence.rank_stream_now(&user_id, &slug, requested, "modern")
    })
    .await
    {
        Ok(Ok(stories)) => {
            let has_more = stories.len() > offset.saturating_add(limit);
            let stories = stories
                .into_iter()
                .skip(offset)
                .take(limit)
                .map(story_card)
                .collect();
            no_store(Json(StreamFeedResponse { stories, has_more }).into_response())
        }
        Ok(Err(error)) => intelligence_error(error),
        Err(error) => {
            error!(error = %error, "stream API task failed");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "stream unavailable")
        }
    }
}

async fn api_story_feedback(
    State(state): State<AppState>,
    Path(story_id): Path<String>,
    headers: HeaderMap,
    Json(request): Json<FeedbackRequest>,
) -> Response {
    let principal = match write_principal(&state, &headers).await {
        Ok(principal) => principal,
        Err(response) => return response,
    };
    let intelligence = state.intelligence.clone();
    let actions = state.actions.clone();
    let user_id = principal.user.id;
    let feedback = request.feedback;
    match tokio::task::spawn_blocking(move || {
        let event_id = intelligence.record_feedback(&user_id, &story_id, feedback, "modern")?;
        if feedback == FeedbackKind::Favorite
            && let Err(failure) = actions.enqueue_favorite(&user_id, &event_id)
        {
            warn!(error = %failure, "favorite persisted but action enqueue failed");
        }
        Ok::<_, IntelligenceError>(event_id)
    })
    .await
    {
        Ok(Ok(event_id)) => no_store(Json(EventResponse { event_id }).into_response()),
        Ok(Err(error)) => intelligence_error(error),
        Err(failure) => {
            error!(error = %failure, "feedback task failed");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "feedback unavailable")
        }
    }
}

async fn api_story(
    State(state): State<AppState>,
    Path(story_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let principal = match browser_principal(&state, &headers).await {
        Ok(principal) => principal,
        Err(error) => return auth_error(error),
    };
    let intelligence = state.intelligence.clone();
    let user_id = principal.user.id;
    match tokio::task::spawn_blocking(move || intelligence.story_detail(&user_id, &story_id)).await
    {
        Ok(Ok(story)) => no_store(Json(story).into_response()),
        Ok(Err(error)) => intelligence_error(error),
        Err(failure) => {
            error!(error = %failure, "story API task failed");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "story unavailable")
        }
    }
}

async fn api_story_read_state(
    State(state): State<AppState>,
    Path(story_id): Path<String>,
    headers: HeaderMap,
    Json(request): Json<ReadStateRequest>,
) -> Response {
    let principal = match write_principal(&state, &headers).await {
        Ok(principal) => principal,
        Err(response) => return response,
    };
    let intelligence = state.intelligence.clone();
    let user_id = principal.user.id;
    match tokio::task::spawn_blocking(move || {
        intelligence.set_story_read(&user_id, &story_id, request.read)
    })
    .await
    {
        Ok(Ok(())) => StatusCode::NO_CONTENT.into_response(),
        Ok(Err(error)) => intelligence_error(error),
        Err(failure) => {
            error!(error = %failure, "read-state API task failed");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "read state unavailable")
        }
    }
}

async fn api_story_representative(
    State(state): State<AppState>,
    Path(story_id): Path<String>,
    headers: HeaderMap,
    Json(request): Json<SelectVariantRequest>,
) -> Response {
    let principal = match write_principal(&state, &headers).await {
        Ok(principal) => principal,
        Err(response) => return response,
    };
    let intelligence = state.intelligence.clone();
    let user_id = principal.user.id;
    match tokio::task::spawn_blocking(move || {
        intelligence.select_story_variant(&user_id, &story_id, &request.document_id)
    })
    .await
    {
        Ok(Ok(())) => StatusCode::NO_CONTENT.into_response(),
        Ok(Err(error)) => intelligence_error(error),
        Err(failure) => {
            error!(error = %failure, "representative API task failed");
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "representative unavailable",
            )
        }
    }
}
