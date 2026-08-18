async fn api_actions(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let principal = match browser_principal(&state, &headers).await {
        Ok(principal) => principal,
        Err(error) => return auth_error(error),
    };
    let actions = state.actions.clone();
    let user_id = principal.user.id;
    match tokio::task::spawn_blocking(move || actions.list(&user_id)).await {
        Ok(Ok(actions)) => no_store(Json(actions).into_response()),
        Ok(Err(error)) => action_error(error),
        Err(failure) => {
            error!(error = %failure, "action list task failed");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "actions unavailable")
        }
    }
}

async fn api_create_action(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateHttpAction>,
) -> Response {
    let principal = match write_principal(&state, &headers).await {
        Ok(principal) => principal,
        Err(response) => return response,
    };
    let actions = state.actions.clone();
    let user_id = principal.user.id;
    match tokio::task::spawn_blocking(move || actions.create_http(&user_id, request)).await {
        Ok(Ok(action)) => (StatusCode::CREATED, Json(action)).into_response(),
        Ok(Err(error)) => action_error(error),
        Err(failure) => {
            error!(error = %failure, "action create task failed");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "action unavailable")
        }
    }
}

async fn api_set_action_enabled(
    State(state): State<AppState>,
    Path(action_id): Path<String>,
    headers: HeaderMap,
    Json(request): Json<ActionEnabledRequest>,
) -> Response {
    let principal = match write_principal(&state, &headers).await {
        Ok(principal) => principal,
        Err(response) => return response,
    };
    let actions = state.actions.clone();
    let user_id = principal.user.id;
    match tokio::task::spawn_blocking(move || {
        actions.set_enabled(&user_id, &action_id, request.enabled)
    })
    .await
    {
        Ok(Ok(())) => StatusCode::NO_CONTENT.into_response(),
        Ok(Err(error)) => action_error(error),
        Err(failure) => {
            error!(error = %failure, "action update task failed");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "action unavailable")
        }
    }
}

async fn api_remove_action(
    State(state): State<AppState>,
    Path(action_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let principal = match write_principal(&state, &headers).await {
        Ok(principal) => principal,
        Err(response) => return response,
    };
    let actions = state.actions.clone();
    let user_id = principal.user.id;
    match tokio::task::spawn_blocking(move || actions.remove(&user_id, &action_id)).await {
        Ok(Ok(())) => StatusCode::NO_CONTENT.into_response(),
        Ok(Err(error)) => action_error(error),
        Err(failure) => {
            error!(error = %failure, "action remove task failed");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "action unavailable")
        }
    }
}

async fn api_plugins(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let principal = match browser_principal(&state, &headers).await {
        Ok(principal) => principal,
        Err(error) => return auth_error(error),
    };
    if principal.require_admin().is_err() {
        return api_error(StatusCode::FORBIDDEN, "permission denied");
    }
    let plugins = state.plugins.clone();
    match tokio::task::spawn_blocking(move || plugins.list()).await {
        Ok(Ok(plugins)) => no_store(Json(plugins).into_response()),
        Ok(Err(error)) => plugin_error(error),
        Err(failure) => {
            error!(error = %failure, "plugin list task failed");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "plugins unavailable")
        }
    }
}

async fn api_plugin(
    State(state): State<AppState>,
    Path(plugin_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let principal = match browser_principal(&state, &headers).await {
        Ok(principal) => principal,
        Err(error) => return auth_error(error),
    };
    if principal.require_admin().is_err() {
        return api_error(StatusCode::FORBIDDEN, "permission denied");
    }
    let plugins = state.plugins.clone();
    match tokio::task::spawn_blocking(move || plugins.get(&plugin_id)).await {
        Ok(Ok(plugin)) => no_store(Json(plugin).into_response()),
        Ok(Err(error)) => plugin_error(error),
        Err(failure) => {
            error!(error = %failure, "plugin inspect task failed");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "plugin unavailable")
        }
    }
}

async fn api_install_plugin(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let principal = match write_principal(&state, &headers).await {
        Ok(principal) => principal,
        Err(response) => return response,
    };
    if principal.require_admin().is_err() {
        return api_error(StatusCode::FORBIDDEN, "permission denied");
    }
    match state.plugins.install(&body).await {
        Ok(plugin) => (StatusCode::CREATED, Json(plugin)).into_response(),
        Err(error) => plugin_error(error),
    }
}

async fn api_set_plugin_enabled(
    State(state): State<AppState>,
    Path(plugin_id): Path<String>,
    headers: HeaderMap,
    Json(request): Json<PluginEnabledRequest>,
) -> Response {
    let principal = match write_principal(&state, &headers).await {
        Ok(principal) => principal,
        Err(response) => return response,
    };
    if principal.require_admin().is_err() {
        return api_error(StatusCode::FORBIDDEN, "permission denied");
    }
    let plugins = state.plugins.clone();
    match tokio::task::spawn_blocking(move || plugins.set_enabled(&plugin_id, request.enabled))
        .await
    {
        Ok(Ok(())) => StatusCode::NO_CONTENT.into_response(),
        Ok(Err(error)) => plugin_error(error),
        Err(failure) => {
            error!(error = %failure, "plugin update task failed");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "plugin unavailable")
        }
    }
}

async fn api_grant_plugin_permission(
    State(state): State<AppState>,
    Path(plugin_id): Path<String>,
    headers: HeaderMap,
    Json(permission): Json<PluginPermission>,
) -> Response {
    let principal = match write_principal(&state, &headers).await {
        Ok(principal) => principal,
        Err(response) => return response,
    };
    if principal.require_admin().is_err() {
        return api_error(StatusCode::FORBIDDEN, "permission denied");
    }
    let plugins = state.plugins.clone();
    match tokio::task::spawn_blocking(move || plugins.grant_permission(&plugin_id, permission))
        .await
    {
        Ok(Ok(())) => StatusCode::NO_CONTENT.into_response(),
        Ok(Err(error)) => plugin_error(error),
        Err(failure) => {
            error!(error = %failure, "plugin permission task failed");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "plugin unavailable")
        }
    }
}

async fn api_create_plugin_source(
    State(state): State<AppState>,
    Path(plugin_id): Path<String>,
    headers: HeaderMap,
    Json(request): Json<CreatePluginSourceRequest>,
) -> Response {
    let principal = match write_principal(&state, &headers).await {
        Ok(principal) => principal,
        Err(response) => return response,
    };
    if principal.require_admin().is_err() {
        return api_error(StatusCode::FORBIDDEN, "permission denied");
    }
    let user_id = principal.user.id;
    let config = PluginSourceConfig {
        plugin_installation_id: plugin_id,
        plugin_config: request.config,
        poll_interval_seconds: request.poll_interval_seconds,
        enabled: true,
    };
    if let Err(error) = state
        .plugins
        .validate_source_config(&user_id, &config)
        .await
    {
        return plugin_error(error);
    }
    let ingestion = state.ingestion.clone();
    let name = request.name;
    let shared = request.shared;
    match tokio::task::spawn_blocking(move || {
        ingestion.register_plugin_source(&name, &user_id, shared, &serde_json::to_value(config)?)
    })
    .await
    {
        Ok(Ok(source)) => (
            StatusCode::CREATED,
            Json(SourceRegistrationResponse {
                id: source.id,
                visibility_scope: source.visibility_scope,
            }),
        )
            .into_response(),
        Ok(Err(error)) => ingestion_error(error),
        Err(failure) => {
            error!(error = %failure, "plugin source registration task failed");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "source unavailable")
        }
    }
}

async fn api_remove_plugin(
    State(state): State<AppState>,
    Path(plugin_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let principal = match write_principal(&state, &headers).await {
        Ok(principal) => principal,
        Err(response) => return response,
    };
    if principal.require_admin().is_err() {
        return api_error(StatusCode::FORBIDDEN, "permission denied");
    }
    let plugins = state.plugins.clone();
    match tokio::task::spawn_blocking(move || plugins.remove(&plugin_id)).await {
        Ok(Ok(())) => StatusCode::NO_CONTENT.into_response(),
        Ok(Err(error)) => plugin_error(error),
        Err(failure) => {
            error!(error = %failure, "plugin removal task failed");
            api_error(StatusCode::INTERNAL_SERVER_ERROR, "plugin unavailable")
        }
    }
}

