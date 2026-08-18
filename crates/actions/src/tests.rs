#[cfg(test)]
mod tests {
    use super::*;
    use base64::{Engine as _, engine::general_purpose};

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
                    },
                    headers: BTreeMap::from([("Authorization".into(), "secret".into())]),
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
                    },
                    headers: BTreeMap::new(),
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
}
