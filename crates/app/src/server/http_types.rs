#[derive(Deserialize)]
struct LoginForm {
    login: String,
    password: String,
}

#[derive(Deserialize)]
struct PairForm {
    code: String,
    csrf_token: String,
}

#[derive(Deserialize)]
struct PairDeviceForm {
    label: String,
    csrf_token: String,
}

#[derive(Deserialize)]
struct PasswordForm {
    old_password: String,
    new_password: String,
    csrf_token: String,
}

#[derive(Deserialize)]
struct CsrfForm {
    csrf_token: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PairDeviceRequest {
    label: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateRssSourceRequest {
    name: String,
    url: String,
    #[serde(default)]
    shared: bool,
    #[serde(default = "default_poll_interval")]
    poll_interval_seconds: u64,
}

#[derive(Deserialize)]
struct QuickAddSourceRequest {
    input: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateEmailSourceRequest {
    name: String,
    host: String,
    #[serde(default = "default_imap_port")]
    port: u16,
    username: String,
    password: String,
    #[serde(default = "default_mailbox")]
    mailbox: String,
    #[serde(default)]
    folders: Vec<String>,
    #[serde(default)]
    mark_as_read: bool,
    #[serde(default = "default_poll_interval")]
    poll_interval_seconds: u64,
}

#[derive(Deserialize)]
struct SearchQuery {
    q: String,
    limit: Option<usize>,
}

#[derive(Deserialize, Default)]
struct LibraryQuery {
    q: Option<String>,
    topic: Option<String>,
}

#[derive(Deserialize)]
struct LimitQuery {
    limit: Option<usize>,
}

#[derive(Deserialize, Default)]
struct SharedQuery {
    #[serde(default)]
    shared: bool,
}

#[derive(Deserialize)]
struct SourceEnabledRequest {
    enabled: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateStreamRequest {
    name: String,
    slug: String,
    icon: Option<String>,
    #[serde(default)]
    filter: StreamFilter,
    semantic_description: Option<String>,
    ranking_instruction: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateStreamRequest {
    name: String,
    icon: Option<String>,
    #[serde(default)]
    filter: StreamFilter,
    semantic_description: Option<String>,
    ranking_instruction: Option<String>,
}

#[derive(Deserialize)]
struct ReorderStreamsRequest {
    slugs: Vec<String>,
}

#[derive(Deserialize)]
struct FeedbackRequest {
    feedback: FeedbackKind,
}

#[derive(Deserialize)]
struct ReaderFeedbackForm {
    feedback: FeedbackKind,
    csrf_token: String,
}

#[derive(Deserialize)]
struct ReaderReadForm {
    read: bool,
    csrf_token: String,
}

#[derive(Deserialize)]
struct ReaderVariantForm {
    document_id: String,
    csrf_token: String,
}

#[derive(Deserialize)]
struct ReadStateRequest {
    read: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SelectVariantRequest {
    document_id: String,
}

#[derive(Deserialize)]
struct CollectionControlRequest {
    mode: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JobResponse {
    job_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateTelegramSourceRequest {
    username: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SourceRegistrationResponse {
    id: String,
    visibility_scope: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct QuickAddSourceResponse {
    id: String,
    kind: String,
    name: String,
    url: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EventResponse {
    event_id: String,
}

#[derive(Deserialize)]
struct ActionEnabledRequest {
    enabled: bool,
}

#[derive(Deserialize)]
struct PluginEnabledRequest {
    enabled: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreatePluginSourceRequest {
    name: String,
    #[serde(default)]
    config: serde_json::Value,
    #[serde(default = "default_poll_interval")]
    poll_interval_seconds: u64,
    #[serde(default)]
    shared: bool,
}

const fn default_poll_interval() -> u64 {
    900
}

const fn default_imap_port() -> u16 {
    993
}

fn default_mailbox() -> String {
    "INBOX".into()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LoginResponse {
    user: rill_domain::User,
    csrf_token: String,
    expires_at: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PairingResponse {
    code: String,
    expires_at: i64,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: &'static str,
}
