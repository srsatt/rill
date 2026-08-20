use std::time::Duration;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use rill_domain::Role;
use rill_jobs::JobQueue;
use serde::Deserialize;
use zeroize::Zeroizing;

use crate::global_settings::ModelSettingInput;
use crate::server::{
    AppState, api_error, auth_error, browser_principal, clear_browser_session_cookies, no_store,
    write_principal,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CreateUserRequest {
    username: String,
    email: Option<String>,
    password: String,
    role: Role,
}

#[derive(Deserialize)]
pub(crate) struct DisabledRequest {
    disabled: bool,
}

#[derive(Deserialize)]
pub(crate) struct RoleRequest {
    role: Role,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChangePasswordRequest {
    current_password: String,
    new_password: String,
}

#[derive(Deserialize)]
pub(crate) struct TelegramBotTokenRequest {
    token: String,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionQuery {
    user_id: Option<String>,
    limit: Option<usize>,
}

#[derive(Deserialize, Default)]
pub(crate) struct ListQuery {
    limit: Option<usize>,
    status: Option<String>,
}

pub(crate) async fn api_users(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = read_admin(&state, &headers).await {
        return response;
    }
    let auth = state.auth.clone();
    match tokio::task::spawn_blocking(move || auth.list_users()).await {
        Ok(Ok(users)) => no_store(Json(users).into_response()),
        Ok(Err(error)) => auth_error(error),
        Err(_) => api_error(StatusCode::INTERNAL_SERVER_ERROR, "users unavailable"),
    }
}

pub(crate) async fn api_create_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<CreateUserRequest>,
) -> Response {
    let principal = match write_admin(&state, &headers).await {
        Ok(principal) => principal,
        Err(response) => return response,
    };
    if !allow_admin_mutation(&state, &principal.user.id) {
        return api_error(StatusCode::TOO_MANY_REQUESTS, "admin rate limit exceeded");
    }
    let auth = state.auth.clone();
    let password = Zeroizing::new(request.password);
    match tokio::task::spawn_blocking(move || {
        auth.create_user(
            &request.username,
            request.email.as_deref(),
            &password,
            request.role,
        )
    })
    .await
    {
        Ok(Ok(user)) => (StatusCode::CREATED, Json(user)).into_response(),
        Ok(Err(error)) => auth_error(error),
        Err(_) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "user creation unavailable",
        ),
    }
}

pub(crate) async fn api_user_disabled(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
    headers: HeaderMap,
    Json(request): Json<DisabledRequest>,
) -> Response {
    let principal = match write_admin(&state, &headers).await {
        Ok(principal) => principal,
        Err(response) => return response,
    };
    if !allow_admin_mutation(&state, &principal.user.id) {
        return api_error(StatusCode::TOO_MANY_REQUESTS, "admin rate limit exceeded");
    }
    let auth = state.auth.clone();
    let actor = principal.user.id;
    let session = principal.session_id;
    match tokio::task::spawn_blocking(move || {
        auth.set_user_disabled(&actor, &session, &user_id, request.disabled)
    })
    .await
    {
        Ok(Ok(true)) => StatusCode::NO_CONTENT.into_response(),
        Ok(Ok(false)) => api_error(StatusCode::NOT_FOUND, "user not found or unchanged"),
        Ok(Err(error)) => auth_error(error),
        Err(_) => api_error(StatusCode::INTERNAL_SERVER_ERROR, "user update unavailable"),
    }
}

pub(crate) async fn api_user_role(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
    headers: HeaderMap,
    Json(request): Json<RoleRequest>,
) -> Response {
    let principal = match write_admin(&state, &headers).await {
        Ok(principal) => principal,
        Err(response) => return response,
    };
    if !allow_admin_mutation(&state, &principal.user.id) {
        return api_error(StatusCode::TOO_MANY_REQUESTS, "admin rate limit exceeded");
    }
    let auth = state.auth.clone();
    let actor = principal.user.id;
    let session = principal.session_id;
    match tokio::task::spawn_blocking(move || {
        auth.set_user_role(&actor, &session, &user_id, request.role)
    })
    .await
    {
        Ok(Ok(true)) => StatusCode::NO_CONTENT.into_response(),
        Ok(Ok(false)) => api_error(StatusCode::NOT_FOUND, "user not found or unchanged"),
        Ok(Err(error)) => auth_error(error),
        Err(_) => api_error(StatusCode::INTERNAL_SERVER_ERROR, "role update unavailable"),
    }
}

pub(crate) async fn api_sessions(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<SessionQuery>,
) -> Response {
    if let Err(response) = read_admin(&state, &headers).await {
        return response;
    }
    let auth = state.auth.clone();
    match tokio::task::spawn_blocking(move || {
        auth.list_browser_sessions(query.user_id.as_deref(), query.limit.unwrap_or(100))
    })
    .await
    {
        Ok(Ok(sessions)) => no_store(Json(sessions).into_response()),
        Ok(Err(error)) => auth_error(error),
        Err(_) => api_error(StatusCode::INTERNAL_SERVER_ERROR, "sessions unavailable"),
    }
}

pub(crate) async fn api_revoke_session(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let principal = match write_admin(&state, &headers).await {
        Ok(principal) => principal,
        Err(response) => return response,
    };
    if !allow_admin_mutation(&state, &principal.user.id) {
        return api_error(StatusCode::TOO_MANY_REQUESTS, "admin rate limit exceeded");
    }
    let auth = state.auth.clone();
    let actor = principal.user.id;
    let actor_session = principal.session_id;
    match tokio::task::spawn_blocking(move || {
        auth.admin_revoke_session(&actor, &actor_session, &session_id)
    })
    .await
    {
        Ok(Ok(true)) => StatusCode::NO_CONTENT.into_response(),
        Ok(Ok(false)) => api_error(StatusCode::NOT_FOUND, "session not found"),
        Ok(Err(error)) => auth_error(error),
        Err(_) => api_error(StatusCode::INTERNAL_SERVER_ERROR, "revocation unavailable"),
    }
}

pub(crate) async fn api_audit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> Response {
    if let Err(response) = read_admin(&state, &headers).await {
        return response;
    }
    let auth = state.auth.clone();
    match tokio::task::spawn_blocking(move || auth.list_audit_events(query.limit.unwrap_or(200)))
        .await
    {
        Ok(Ok(events)) => no_store(Json(events).into_response()),
        Ok(Err(error)) => auth_error(error),
        Err(_) => api_error(StatusCode::INTERNAL_SERVER_ERROR, "audit log unavailable"),
    }
}

pub(crate) async fn api_jobs(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> Response {
    if let Err(response) = read_admin(&state, &headers).await {
        return response;
    }
    let queue = JobQueue::new(state.pool.clone());
    match tokio::task::spawn_blocking(move || {
        queue.list(query.status.as_deref(), query.limit.unwrap_or(100))
    })
    .await
    {
        Ok(Ok(jobs)) => no_store(Json(jobs).into_response()),
        _ => api_error(StatusCode::INTERNAL_SERVER_ERROR, "jobs unavailable"),
    }
}

pub(crate) async fn api_models(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(response) = read_admin(&state, &headers).await {
        return response;
    }
    let settings = state.global_settings.clone();
    match tokio::task::spawn_blocking(move || settings.list_models()).await {
        Ok(Ok(models)) => no_store(Json(models).into_response()),
        _ => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "model settings unavailable",
        ),
    }
}

pub(crate) async fn api_put_model(
    State(state): State<AppState>,
    Path(slot): Path<String>,
    headers: HeaderMap,
    Json(mut request): Json<ModelSettingInput>,
) -> Response {
    let principal = match write_admin(&state, &headers).await {
        Ok(principal) => principal,
        Err(response) => return response,
    };
    if !allow_admin_mutation(&state, &principal.user.id) {
        return api_error(StatusCode::TOO_MANY_REQUESTS, "admin rate limit exceeded");
    }
    let api_key = request.api_key.take().map(Zeroizing::new);
    let settings = state.global_settings.clone();
    let actor = principal.user.id;
    let session = principal.session_id;
    let requested_slot = slot.clone();
    match tokio::task::spawn_blocking(move || {
        settings.put_model(
            &actor,
            &session,
            &requested_slot,
            &request,
            api_key.as_deref().map(String::as_str),
        )
    })
    .await
    {
        Ok(Ok(model)) => no_store(Json(model).into_response()),
        Ok(Err(error)) => {
            tracing::warn!(error = %error, slot = %slot, "model setting rejected");
            api_error(StatusCode::BAD_REQUEST, "invalid model setting")
        }
        Err(_) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "model settings unavailable",
        ),
    }
}

pub(crate) async fn api_test_model(
    State(state): State<AppState>,
    Path(slot): Path<String>,
    headers: HeaderMap,
    Json(mut request): Json<ModelSettingInput>,
) -> Response {
    let principal = match write_admin(&state, &headers).await {
        Ok(principal) => principal,
        Err(response) => return response,
    };
    if !allow_admin_mutation(&state, &principal.user.id) {
        return api_error(StatusCode::TOO_MANY_REQUESTS, "admin rate limit exceeded");
    }
    let api_key = request.api_key.take().map(Zeroizing::new);
    let settings = state.global_settings.clone();
    let requested_slot = slot.clone();
    let registry = match tokio::task::spawn_blocking(move || {
        settings.test_model(
            &requested_slot,
            &request,
            api_key.as_deref().map(String::as_str),
        )
    })
    .await
    {
        Ok(Ok(registry)) => registry,
        Ok(Err(error)) => {
            tracing::warn!(error = %error, slot = %slot, "model test configuration rejected");
            return api_error(StatusCode::BAD_REQUEST, "invalid model setting");
        }
        Err(_) => return api_error(StatusCode::INTERNAL_SERVER_ERROR, "model test unavailable"),
    };
    match registry.health(&slot).await {
        Ok(health) => no_store(Json(health).into_response()),
        Err(error) => {
            tracing::warn!(error = %error, slot = %slot, "model test failed");
            api_error(StatusCode::BAD_GATEWAY, "model test failed")
        }
    }
}

pub(crate) async fn api_delete_model(
    State(state): State<AppState>,
    Path(slot): Path<String>,
    headers: HeaderMap,
) -> Response {
    let principal = match write_admin(&state, &headers).await {
        Ok(principal) => principal,
        Err(response) => return response,
    };
    if !allow_admin_mutation(&state, &principal.user.id) {
        return api_error(StatusCode::TOO_MANY_REQUESTS, "admin rate limit exceeded");
    }
    let settings = state.global_settings.clone();
    let actor = principal.user.id;
    let session = principal.session_id;
    match tokio::task::spawn_blocking(move || settings.delete_model(&actor, &session, &slot)).await
    {
        Ok(Ok(())) => StatusCode::NO_CONTENT.into_response(),
        Ok(Err(_)) => api_error(StatusCode::BAD_REQUEST, "unknown model setting"),
        Err(_) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "model settings unavailable",
        ),
    }
}

pub(crate) async fn api_telegram_bot(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    if let Err(response) = read_admin(&state, &headers).await {
        return response;
    }
    match state.telegram.bot_view() {
        Ok(view) => no_store(Json(view).into_response()),
        Err(_) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Telegram settings unavailable",
        ),
    }
}

pub(crate) async fn api_put_telegram_bot(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<TelegramBotTokenRequest>,
) -> Response {
    let principal = match write_admin(&state, &headers).await {
        Ok(principal) => principal,
        Err(response) => return response,
    };
    if !allow_admin_mutation(&state, &principal.user.id) {
        return api_error(StatusCode::TOO_MANY_REQUESTS, "admin rate limit exceeded");
    }
    let token = Zeroizing::new(request.token);
    match state
        .telegram
        .configure_bot(&principal.user.id, &principal.session_id, token.to_string())
        .await
    {
        Ok(view) => no_store(Json(view).into_response()),
        Err(error) => {
            tracing::warn!(error = %error, "Telegram bot setting rejected");
            api_error(StatusCode::BAD_REQUEST, "invalid Telegram bot token")
        }
    }
}

pub(crate) async fn api_delete_telegram_bot(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    let principal = match write_admin(&state, &headers).await {
        Ok(principal) => principal,
        Err(response) => return response,
    };
    if !allow_admin_mutation(&state, &principal.user.id) {
        return api_error(StatusCode::TOO_MANY_REQUESTS, "admin rate limit exceeded");
    }
    match state
        .telegram
        .delete_bot(&principal.user.id, &principal.session_id)
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(_) => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Telegram settings unavailable",
        ),
    }
}

pub(crate) async fn api_retry_job(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    control_job(state, headers, job_id, true).await
}

pub(crate) async fn api_cancel_job(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    control_job(state, headers, job_id, false).await
}

pub(crate) async fn api_change_password(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<ChangePasswordRequest>,
) -> Response {
    let principal = match write_principal(&state, &headers).await {
        Ok(principal) => principal,
        Err(response) => return response,
    };
    let auth = state.auth.clone();
    let user_id = principal.user.id;
    let current = Zeroizing::new(request.current_password);
    let new = Zeroizing::new(request.new_password);
    match tokio::task::spawn_blocking(move || auth.change_password(&user_id, &current, &new)).await
    {
        Ok(Ok(())) => {
            let mut response = StatusCode::NO_CONTENT.into_response();
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

async fn control_job(state: AppState, headers: HeaderMap, job_id: String, retry: bool) -> Response {
    let principal = match write_admin(&state, &headers).await {
        Ok(principal) => principal,
        Err(response) => return response,
    };
    if !allow_admin_mutation(&state, &principal.user.id) {
        return api_error(StatusCode::TOO_MANY_REQUESTS, "admin rate limit exceeded");
    }
    let queue = JobQueue::new(state.pool.clone());
    match tokio::task::spawn_blocking(move || {
        if retry {
            queue.retry_dead(&job_id)
        } else {
            queue.cancel_queued(&job_id)
        }
    })
    .await
    {
        Ok(Ok(true)) => StatusCode::NO_CONTENT.into_response(),
        Ok(Ok(false)) => api_error(StatusCode::CONFLICT, "job state does not allow operation"),
        _ => api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "job operation unavailable",
        ),
    }
}

async fn read_admin(state: &AppState, headers: &HeaderMap) -> Result<(), Response> {
    let principal = browser_principal(state, headers)
        .await
        .map_err(auth_error)?;
    principal.require_admin().map_err(auth_error)
}

async fn write_admin(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<rill_auth::Principal, Response> {
    let principal = write_principal(state, headers).await?;
    principal.require_admin().map_err(auth_error)?;
    Ok(principal)
}

fn allow_admin_mutation(state: &AppState, user_id: &str) -> bool {
    state
        .admin_limiter
        .attempt(user_id, 60, Duration::from_secs(60))
}
