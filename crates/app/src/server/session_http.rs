pub(crate) async fn browser_principal(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Principal, AuthError> {
    principal_from_cookie(state, headers, SessionKind::Browser).await
}

async fn reader_principal(state: &AppState, headers: &HeaderMap) -> Result<Principal, AuthError> {
    principal_from_cookie(state, headers, SessionKind::Reader).await
}

async fn reader_or_browser_principal(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Principal, AuthError> {
    match reader_principal(state, headers).await {
        Ok(principal) => Ok(principal),
        Err(_) => browser_principal(state, headers).await,
    }
}

fn principal_csrf_cookie(principal: &Principal) -> &'static str {
    match principal.kind {
        SessionKind::Browser => browser_csrf_cookie(),
        SessionKind::Reader => reader_csrf_cookie(),
    }
}

async fn principal_from_cookie(
    state: &AppState,
    headers: &HeaderMap,
    kind: SessionKind,
) -> Result<Principal, AuthError> {
    let name = match kind {
        SessionKind::Browser => browser_session_cookie(state.secure_cookies),
        SessionKind::Reader => reader_session_cookie(state.secure_cookies),
    };
    let token = cookie(headers, name).ok_or(AuthError::InvalidSession)?;
    let auth = state.auth.clone();
    tokio::task::spawn_blocking(move || match kind {
        SessionKind::Browser => auth.browser_principal(&token),
        SessionKind::Reader => auth.reader_principal(&token),
    })
    .await
    .map_err(|_| AuthError::InvalidSession)?
}

async fn validate_csrf(
    state: &AppState,
    principal: &Principal,
    token: &str,
) -> Result<(), AuthError> {
    let auth = state.auth.clone();
    let principal = principal.clone();
    let token = token.to_owned();
    tokio::task::spawn_blocking(move || auth.validate_csrf(&principal, &token))
        .await
        .map_err(|_| AuthError::InvalidSession)?
}

pub(crate) async fn write_principal(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Principal, Response> {
    if !valid_origin(headers, &state.public_origin) {
        return Err(api_error(StatusCode::FORBIDDEN, "origin rejected"));
    }
    let principal = browser_principal(state, headers)
        .await
        .map_err(auth_error)?;
    let csrf = csrf_header(headers)
        .ok_or_else(|| api_error(StatusCode::FORBIDDEN, "CSRF validation failed"))?;
    validate_csrf(state, &principal, &csrf)
        .await
        .map_err(|_| api_error(StatusCode::FORBIDDEN, "CSRF validation failed"))?;
    Ok(principal)
}

async fn reader_or_browser_write_principal(
    state: &AppState,
    headers: &HeaderMap,
    csrf: &str,
) -> Result<Principal, Response> {
    if !valid_origin(headers, &state.public_origin) {
        return Err(api_error(StatusCode::FORBIDDEN, "origin rejected"));
    }
    let principal = reader_or_browser_principal(state, headers)
        .await
        .map_err(auth_error)?;
    validate_csrf(state, &principal, csrf)
        .await
        .map_err(|_| api_error(StatusCode::FORBIDDEN, "CSRF validation failed"))?;
    Ok(principal)
}

fn session_redirect(state: &AppState, session: rill_auth::BrowserSession) -> Response {
    let mut response = Redirect::to("/stream/home").into_response();
    set_browser_session_cookies(state, &mut response, &session);
    no_store(response)
}

fn reader_session_redirect(state: &AppState, session: rill_auth::ReaderSession) -> Response {
    let mut response = Redirect::to("/reader").into_response();
    append_cookie(
        &mut response,
        cookie_header(
            reader_session_cookie(state.secure_cookies),
            session.token.expose(),
            true,
            true,
            state.secure_cookies,
            (session.expires_at - unix_now()).max(0),
        ),
    );
    append_cookie(
        &mut response,
        cookie_header(
            reader_csrf_cookie(),
            session.csrf_token.expose(),
            false,
            true,
            state.secure_cookies,
            (session.expires_at - unix_now()).max(0),
        ),
    );
    append_cookie(
        &mut response,
        expired_cookie(pair_csrf_cookie(), state.secure_cookies),
    );
    no_store(response)
}

fn set_browser_session_cookies(
    state: &AppState,
    response: &mut Response,
    session: &rill_auth::BrowserSession,
) {
    let maximum_age = (session.expires_at - unix_now()).max(0);
    append_cookie(
        response,
        cookie_header(
            browser_session_cookie(state.secure_cookies),
            session.token.expose(),
            true,
            false,
            state.secure_cookies,
            maximum_age,
        ),
    );
    append_cookie(
        response,
        cookie_header(
            browser_csrf_cookie(),
            session.csrf_token.expose(),
            false,
            true,
            state.secure_cookies,
            maximum_age,
        ),
    );
}

pub(crate) fn clear_browser_session_cookies(state: &AppState, response: &mut Response) {
    append_cookie(
        response,
        expired_cookie(
            browser_session_cookie(state.secure_cookies),
            state.secure_cookies,
        ),
    );
    append_cookie(
        response,
        expired_cookie(browser_csrf_cookie(), state.secure_cookies),
    );
}

fn clear_reader_session_cookies(state: &AppState, response: &mut Response) {
    append_cookie(
        response,
        expired_cookie(
            reader_session_cookie(state.secure_cookies),
            state.secure_cookies,
        ),
    );
    append_cookie(
        response,
        expired_cookie(reader_csrf_cookie(), state.secure_cookies),
    );
}

fn browser_session_cookie(secure: bool) -> &'static str {
    if secure {
        "__Host-rill_session"
    } else {
        "rill_session"
    }
}

fn reader_session_cookie(secure: bool) -> &'static str {
    if secure {
        "__Host-rill_reader"
    } else {
        "rill_reader"
    }
}

fn browser_csrf_cookie() -> &'static str {
    "rill_csrf"
}
fn reader_csrf_cookie() -> &'static str {
    "rill_reader_csrf"
}
fn pair_csrf_cookie() -> &'static str {
    "rill_pair_csrf"
}

fn cookie_header(
    name: &str,
    value: &str,
    http_only: bool,
    strict: bool,
    secure: bool,
    maximum_age: i64,
) -> String {
    let mut cookie = format!(
        "{name}={value}; Path=/; Max-Age={maximum_age}; SameSite={}",
        if strict { "Strict" } else { "Lax" },
    );
    if http_only {
        cookie.push_str("; HttpOnly");
    }
    if secure {
        cookie.push_str("; Secure");
    }
    cookie
}

fn expired_cookie(name: &str, secure: bool) -> String {
    cookie_header(name, "", true, true, secure, 0)
}

fn append_cookie(response: &mut Response, cookie: String) {
    if let Ok(value) = HeaderValue::from_str(&cookie) {
        response.headers_mut().append(header::SET_COOKIE, value);
    }
}

fn cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|header| header.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|part| part.trim().split_once('='))
        .find_map(|(key, value)| (key == name).then(|| value.to_owned()))
}

fn csrf_header(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-csrf-token")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

fn user_agent(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::USER_AGENT)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

fn valid_origin(headers: &HeaderMap, expected: &str) -> bool {
    if let Some(origin) = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
    {
        return origin == expected;
    }
    headers
        .get(header::REFERER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| Url::parse(value).ok())
        .and_then(|url| origin(&url).ok())
        .is_some_and(|actual| actual == expected)
}

fn origin(url: &Url) -> Result<String> {
    let host = url.host_str().context("public URL has no host")?;
    let mut value = format!("{}://{host}", url.scheme());
    if let Some(port) = url.port() {
        value.push_str(&format!(":{port}"));
    }
    Ok(value)
}

fn ip_summary(ip: IpAddr) -> String {
    match ip {
        IpAddr::V4(ip) => {
            let octets = ip.octets();
            format!("{}.{}.{}.0/24", octets[0], octets[1], octets[2])
        }
        IpAddr::V6(ip) => {
            let segments = ip.segments();
            format!(
                "{:x}:{:x}:{:x}:{:x}::/64",
                segments[0], segments[1], segments[2], segments[3]
            )
        }
    }
}

fn constant_time_equal(left: Option<&str>, right: &str) -> bool {
    let Some(left) = left else {
        return false;
    };
    left.len() == right.len() && bool::from(left.as_bytes().ct_eq(right.as_bytes()))
}
