#[cfg(test)]
mod tests {
    use super::*;
    use base64::{Engine as _, engine::general_purpose};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    fn config(url: String) -> HttpActionConfig {
        HttpActionConfig {
            url,
            method: "POST".into(),
            timeout_seconds: 2,
            maximum_response_bytes: 1024,
            maximum_attempts: 2,
            body_template: None,
        }
    }

    #[test]
    fn headers_are_encrypted_and_redacted_from_views() {
        let pool = DbPool::open_in_memory().unwrap();
        pool.with_connection(|connection| {
            connection.execute(
                "INSERT INTO users(id, username, role) VALUES ('u','u','user')",
                [],
            )
        })
        .unwrap();
        let key = general_purpose::URL_SAFE_NO_PAD.encode([9_u8; 32]);
        let store = SecretStore::from_base64(pool.clone(), &key, 1).unwrap();
        let service = ActionService::new(pool.clone(), Some(store), false);
        let view = service
            .create_http(
                "u",
                CreateHttpAction {
                    name: "Save".into(),
                    config: HttpActionConfig {
                        url: "https://example.com/hooks".into(),
                        method: "POST".into(),
                        timeout_seconds: 5,
                        maximum_response_bytes: 1024,
                        maximum_attempts: 3,
                        body_template: None,
                    },
                    headers: BTreeMap::from([("Authorization".into(), "secret".into())]),
                    header_env: BTreeMap::new(),
                    enabled: true,
                },
            )
            .unwrap();
        assert!(view.has_headers);
        assert!(
            !serde_json::to_string(&service.list("u").unwrap())
                .unwrap()
                .contains("secret")
        );
        let raw: Vec<u8> = pool
            .with_connection(|connection| {
                connection.query_row(
                    "SELECT ciphertext FROM encrypted_secrets LIMIT 1",
                    [],
                    |row| row.get(0),
                )
            })
            .unwrap();
        assert!(!String::from_utf8_lossy(&raw).contains("secret"));
    }

    #[test]
    fn rejects_private_action_target() {
        let config = HttpActionConfig {
            url: "http://127.0.0.1/hook".into(),
            method: "POST".into(),
            timeout_seconds: 5,
            maximum_response_bytes: 1024,
            maximum_attempts: 2,
            body_template: None,
        };
        assert!(validate_config(&config, false).is_err());
    }

    #[test]
    fn favorite_enqueue_is_durable_and_idempotent() {
        let pool = DbPool::open_in_memory().unwrap();
        pool.with_connection(|connection| {
            connection.execute(
                "INSERT INTO users(id, username, role) VALUES ('u','u','user')",
                [],
            )
        })
        .unwrap();
        let service = ActionService::new(pool.clone(), None, false);
        service
            .create_http(
                "u",
                CreateHttpAction {
                    name: "Save".into(),
                    config: HttpActionConfig {
                        url: "https://example.com/hooks".into(),
                        method: "POST".into(),
                        timeout_seconds: 5,
                        maximum_response_bytes: 1024,
                        maximum_attempts: 3,
                        body_template: None,
                    },
                    headers: BTreeMap::new(),
                    header_env: BTreeMap::new(),
                    enabled: true,
                },
            )
            .unwrap();

        assert_eq!(service.enqueue_favorite("u", "event-1").unwrap(), 1);
        assert_eq!(service.enqueue_favorite("u", "event-1").unwrap(), 0);
        let counts = pool
            .with_connection(|connection| {
                Ok((
                    connection.query_row("SELECT count(*) FROM action_executions", [], |row| {
                        row.get::<_, i64>(0)
                    })?,
                    connection.query_row(
                        "SELECT count(*) FROM jobs WHERE kind='ExecuteAction'",
                        [],
                        |row| row.get::<_, i64>(0),
                    )?,
                ))
            })
            .unwrap();
        assert_eq!(counts, (1, 1));
    }

    #[test]
    fn body_template_substitutes_values_and_json_escapes_strings() {
        let event = json!({
            "event": "story.favorite",
            "eventId": "event-1",
            "story": {
                "id": "story-1", "title": "A \"quoted\" title\n", "summary": null,
                "url": "https://example.com/a", "source": "example.com", "curator": "feed",
                "publishedAt": 1, "relatedLinks": ["https://forum.example/a"]
            }
        });
        let template = json!({
            "type": "link",
            "title": "${story.title}",
            "note": "Saved: ${story.title}",
            "links": "${story.relatedLinks}"
        });
        let rendered = render_template(&template, &event).unwrap();
        assert_eq!(rendered["title"], event["story"]["title"]);
        assert_eq!(rendered["links"], event["story"]["relatedLinks"]);
        assert!(serde_json::to_string(&rendered).unwrap().contains("\\\"quoted\\\""));
        assert!(render_template(&json!("${story.unknown}"), &event).is_err());
    }

    #[test]
    fn environment_header_reference_adds_prefix_without_exposing_value() {
        let references = BTreeMap::from([(
            "Authorization".into(),
            HeaderEnvValue {
                env: "KARAKEEP_API_TOKEN".into(),
                prefix: "Bearer ".into(),
            },
        )]);
        let headers = resolve_header_env(&references, |_| Ok("private-token".into())).unwrap();
        assert_eq!(headers["Authorization"], "Bearer private-token");
        assert!(!serde_json::to_string(&references).unwrap().contains("private-token"));
    }

    #[tokio::test]
    async fn http_action_sends_method_headers_template_and_idempotency_key() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = vec![0_u8; 8192];
            let mut length = 0;
            loop {
                let read = stream.read(&mut request[length..]).await.unwrap();
                length += read;
                let text = String::from_utf8_lossy(&request[..length]);
                let Some(headers_end) = text.find("\r\n\r\n") else {
                    continue;
                };
                let content_length = text[..headers_end]
                    .lines()
                    .find_map(|line| line.to_ascii_lowercase().strip_prefix("content-length: ")?.parse::<usize>().ok())
                    .unwrap_or(0);
                if length >= headers_end + 4 + content_length {
                    break;
                }
            }
            stream
                .write_all(b"HTTP/1.1 201 Created\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                .await
                .unwrap();
            String::from_utf8(request[..length].to_vec()).unwrap()
        });
        let mut action = config(format!("http://{address}/api/v1/bookmarks"));
        action.body_template = Some(json!({
            "type": "link", "url": "${story.url}", "title": "${story.title}"
        }));
        let event = json!({
            "event": "story.favorite", "eventId": "event-1",
            "story": { "id": "story-1", "title": "Useful", "summary": null,
                "url": "https://example.com/a", "source": "example.com", "curator": null,
                "publishedAt": null, "relatedLinks": [] }
        });
        let body = render_body(&action, &event).unwrap();
        send_http(
            &action,
            &BTreeMap::from([("Authorization".into(), "Bearer token".into())]),
            "action:a:event-1",
            &body,
            true,
        )
        .await
        .unwrap();
        let request = server.await.unwrap();
        assert!(request.starts_with("POST /api/v1/bookmarks HTTP/1.1"));
        assert!(request.to_ascii_lowercase().contains("authorization: bearer token"));
        assert!(request.to_ascii_lowercase().contains("idempotency-key: action:a:event-1"));
        let body: Value = serde_json::from_str(request.split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert_eq!(body, json!({"type":"link","url":"https://example.com/a","title":"Useful"}));
    }

    #[tokio::test]
    async fn http_action_enforces_timeout_and_response_limit() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request).await;
            let body = "x".repeat(2048);
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
        });
        let mut action = config(format!("http://{address}/large"));
        action.maximum_response_bytes = 32;
        assert!(send_http(&action, &BTreeMap::new(), "key", &json!({}), true).await.is_err());
        server.await.unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (_stream, _) = listener.accept().await.unwrap();
            tokio::time::sleep(Duration::from_secs(2)).await;
        });
        let mut action = config(format!("http://{address}/slow"));
        action.timeout_seconds = 1;
        assert!(send_http(&action, &BTreeMap::new(), "key", &json!({}), true).await.is_err());
        server.abort();
    }

    #[tokio::test]
    async fn failed_delivery_keeps_favorite_and_records_retry_then_terminal_failure() {
        let pool = DbPool::open_in_memory().unwrap();
        pool.with_connection(|connection| {
            connection.execute_batch(
                "INSERT INTO users(id, username, role) VALUES ('u','u','user');
                 INSERT INTO documents(id, visibility_scope, exact_content_hash, title, body_text,
                   canonical_url) VALUES ('d','public',x'01','Story','Body','https://example.com/a');
                 INSERT INTO stories(id, visibility_scope, cluster_version, anchor_document_id)
                   VALUES ('s','public','exact-v1','d');
                 INSERT INTO story_memberships(story_id, document_id, similarity) VALUES ('s','d',1);
                 INSERT INTO feedback_events(id,user_id,story_id,document_id,feedback,source)
                   VALUES ('event','u','s','d','favorite','test');
                 INSERT INTO user_story_state(user_id,story_id,favorite) VALUES ('u','s',1);",
            )
        })
        .unwrap();
        let service = ActionService::new(pool.clone(), None, true);
        service
            .create_http(
                "u",
                CreateHttpAction {
                    name: "Unavailable".into(),
                    config: config("http://127.0.0.1:9/fail".into()),
                    headers: BTreeMap::new(),
                    header_env: BTreeMap::new(),
                    enabled: true,
                },
            )
            .unwrap();
        service.enqueue_favorite("u", "event").unwrap();
        let execution_id: String = pool
            .with_connection(|connection| {
                connection.query_row("SELECT id FROM action_executions", [], |row| row.get(0))
            })
            .unwrap();
        let payload = ExecuteActionPayload { execution_id };
        assert!(service.execute(&payload, 1, 2).await.is_err());
        assert!(service.execute(&payload, 2, 2).await.is_err());
        let state: (i64, String, Vec<String>) = pool
            .with_connection(|connection| {
                let favorite = connection.query_row(
                    "SELECT favorite FROM user_story_state WHERE user_id='u' AND story_id='s'",
                    [],
                    |row| row.get(0),
                )?;
                let status = connection.query_row(
                    "SELECT status FROM action_executions WHERE id=?1",
                    [&payload.execution_id],
                    |row| row.get(0),
                )?;
                let mut statement = connection.prepare(
                    "SELECT status FROM action_attempts WHERE execution_id=?1 ORDER BY attempt_number",
                )?;
                let attempts = statement
                    .query_map([&payload.execution_id], |row| row.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok::<_, rusqlite::Error>((favorite, status, attempts))
            })
            .unwrap();
        assert_eq!(state, (1, "failed".into(), vec!["retrying".into(), "failed".into()]));
    }
}
