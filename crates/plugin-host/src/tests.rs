#[cfg(test)]
mod tests {
    use super::*;
    use rill_source_api::FetchPolicy;

    fn service(pool: DbPool) -> PluginService {
        PluginService::new(
            pool,
            None,
            Arc::new(BoundedHttpClient::new(FetchPolicy::default()).unwrap()),
            PluginLimits {
                memory_bytes: 8 * 1024 * 1024,
                fuel: 1_000_000,
                timeout: Duration::from_secs(1),
                maximum_output_bytes: 64 * 1024,
                maximum_component_bytes: 1024 * 1024,
                maximum_http_bytes: 64 * 1024,
            },
        )
        .unwrap()
    }

    #[tokio::test]
    async fn example_component_installs_and_polls_under_limits() {
        let pool = DbPool::open_in_memory().unwrap();
        let service = service(pool.clone());
        let component = wat::parse_str(include_str!(
            "../../../plugins/example-static/component.wat"
        ))
        .unwrap();
        let view = service
            .install(&component)
            .await
            .unwrap();
        assert_eq!(view.metadata.id, "example-static");
        assert!(!view.enabled);
        service.set_enabled(&view.installation_id, true).unwrap();
        pool.with_connection(|connection| {
            connection.execute_batch(
                "INSERT INTO users(id, username, role) VALUES ('u','u','user');
                 INSERT INTO source_definitions(id, kind, display_name, built_in)
                 VALUES ('plugin','plugin','Example',0);
                 INSERT INTO source_instances(id, definition_id, owner_user_id, name, visibility,
                 visibility_scope, config_json) VALUES
                 ('s','plugin','u','Example','private','user:u','{}');",
            )
        })
        .unwrap();
        let config = PluginSourceConfig {
            plugin_installation_id: view.installation_id,
            plugin_config: json!({}),
            poll_interval_seconds: 900,
            enabled: true,
        };
        let batch = service.poll("s", &config, None, 10).await.unwrap();
        assert_eq!(batch.items.len(), 1);
        assert_eq!(batch.items[0].external_id, "welcome");
    }

    #[tokio::test]
    async fn poll_trap_is_isolated_and_recorded_in_health() {
        let pool = DbPool::open_in_memory().unwrap();
        let service = service(pool.clone());
        let source = include_str!("../../../plugins/example-static/component.wat");
        let trapping = source.replace(
            "(func $poll (param i32 i32 i32 i32 i32 i32) (result i32)\n      i32.const 32 i32.const 0 i32.store\n      i32.const 36 i32.const 1344 i32.store\n      i32.const 40 i32.const 361 i32.store\n      i32.const 32)",
            "(func $poll (param i32 i32 i32 i32 i32 i32) (result i32) unreachable)",
        );
        assert_ne!(trapping, source);
        let component = wat::parse_str(trapping).unwrap();
        let view = service.install(&component).await.unwrap();
        service.set_enabled(&view.installation_id, true).unwrap();
        pool.with_connection(|connection| {
            connection.execute_batch(
                "INSERT INTO users(id, username, role) VALUES ('u','u','user');
                 INSERT INTO source_definitions(id, kind, display_name, built_in)
                 VALUES ('plugin','plugin','Example',0);
                 INSERT INTO source_instances(id, definition_id, owner_user_id, name, visibility,
                 visibility_scope, config_json) VALUES
                 ('s','plugin','u','Example','private','user:u','{}');",
            )
        })
        .unwrap();
        let config = PluginSourceConfig {
            plugin_installation_id: view.installation_id.clone(),
            plugin_config: json!({}),
            poll_interval_seconds: 900,
            enabled: true,
        };

        assert!(service.poll("s", &config, None, 10).await.is_err());
        let health = service.get(&view.installation_id).unwrap();
        assert_eq!(health.consecutive_failures, 1);
        assert!(health.last_error_message.is_some());
    }

    #[test]
    fn permission_constraints_are_explicit() {
        assert!(
            validate_permission(&PluginPermission {
                capability: "http".into(),
                constraint: json!({"hosts": []}),
            })
            .is_err()
        );
        assert!(
            validate_permission(&PluginPermission {
                capability: "filesystem".into(),
                constraint: json!({}),
            })
            .is_err()
        );
    }
}
