impl PluginService {
    pub fn new(
        pool: DbPool,
        secrets: Option<SecretStore>,
        http: Arc<BoundedHttpClient>,
        limits: PluginLimits,
    ) -> Result<Self, PluginError> {
        let mut config = Config::new();
        config.consume_fuel(true);
        config.epoch_interruption(true);
        let engine = Arc::new(
            Engine::new(&config).map_err(|error| PluginError::Invalid(error.to_string()))?,
        );
        let weak = Arc::downgrade(&engine);
        thread::spawn(move || {
            loop {
                thread::sleep(EPOCH_TICK);
                let Some(engine) = weak.upgrade() else {
                    break;
                };
                engine.increment_epoch();
            }
        });
        Ok(Self {
            pool,
            secrets,
            http,
            engine,
            limits,
            cache: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub async fn install(&self, bytes: &[u8]) -> Result<PluginView, PluginError> {
        if bytes.is_empty() || bytes.len() > self.limits.maximum_component_bytes {
            return Err(PluginError::Invalid(
                "component size is out of range".into(),
            ));
        }
        let component = Arc::new(
            Component::new(&self.engine, bytes)
                .map_err(|error| PluginError::Invalid(error.to_string()))?,
        );
        let (metadata, config_schema) = self.inspect_component(&component).await?;
        validate_metadata(&metadata)?;
        let hash = format!("{:x}", Sha256::digest(bytes));
        let installation_id = Uuid::new_v4().to_string();
        let manifest = json!({ "metadata": metadata, "configSchema": config_schema });
        let mut connection = self.pool.connection()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO plugin_installations(id, name, version, wasm_sha256, manifest_json)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                installation_id,
                metadata.name,
                metadata.version,
                hash,
                serde_json::to_string(&manifest)?
            ],
        )?;
        transaction.execute(
            "INSERT INTO plugin_components(plugin_installation_id, component_bytes)
             VALUES (?1, ?2)",
            params![installation_id, bytes],
        )?;
        transaction.execute(
            "INSERT INTO plugin_health(plugin_installation_id) VALUES (?1)",
            [&installation_id],
        )?;
        transaction.commit()?;
        drop(connection);
        self.cache
            .lock()
            .map_err(|_| PluginError::Trap("component cache lock poisoned".into()))?
            .insert(installation_id.clone(), component);
        self.get(&installation_id)
    }

    pub fn list(&self) -> Result<Vec<PluginView>, PluginError> {
        let connection = self.pool.connection()?;
        let mut statement = connection
            .prepare("SELECT pi.id FROM plugin_installations pi ORDER BY pi.installed_at, pi.id")?;
        let ids = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(statement);
        drop(connection);
        ids.iter().map(|id| self.get(id)).collect()
    }

    pub fn get(&self, id: &str) -> Result<PluginView, PluginError> {
        type Row = (String, String, bool, Option<i64>, u32, Option<String>);
        let record: Option<Row> = self.pool.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT pi.manifest_json, pi.wasm_sha256, pi.enabled, ph.last_success_at,
                     coalesce(ph.consecutive_failures,0), ph.last_error_message
                     FROM plugin_installations pi LEFT JOIN plugin_health ph
                     ON ph.plugin_installation_id=pi.id WHERE pi.id=?1",
                    [id],
                    |row| {
                        Ok((
                            row.get(0)?,
                            row.get(1)?,
                            row.get::<_, i64>(2)? != 0,
                            row.get(3)?,
                            row.get(4)?,
                            row.get(5)?,
                        ))
                    },
                )
                .optional()
        })?;
        let (raw_manifest, hash, enabled, last_success_at, failures, last_error_message) =
            record.ok_or(PluginError::NotFound)?;
        let manifest: Value = serde_json::from_str(&raw_manifest)?;
        let metadata = serde_json::from_value(
            manifest
                .get("metadata")
                .cloned()
                .ok_or_else(|| PluginError::Invalid("metadata is missing".into()))?,
        )?;
        let config_schema = manifest
            .get("configSchema")
            .cloned()
            .unwrap_or_else(|| json!({}));
        Ok(PluginView {
            installation_id: id.into(),
            metadata,
            component_sha256: hash,
            config_schema,
            enabled,
            granted_permissions: self.permissions(id)?,
            last_success_at,
            consecutive_failures: failures,
            last_error_message,
        })
    }

    pub fn grant_permission(
        &self,
        id: &str,
        permission: PluginPermission,
    ) -> Result<(), PluginError> {
        let view = self.get(id)?;
        if !view
            .metadata
            .requested_permissions
            .contains(&permission.capability)
        {
            return Err(PluginError::Invalid(
                "plugin did not request this capability".into(),
            ));
        }
        validate_permission(&permission)?;
        self.pool.with_connection(|connection| {
            connection.execute(
                "INSERT INTO plugin_permissions(plugin_installation_id, capability,
                 constraint_json) VALUES (?1, ?2, ?3)
                 ON CONFLICT(plugin_installation_id, capability) DO UPDATE SET
                 constraint_json=excluded.constraint_json, granted_at=unixepoch()",
                params![
                    id,
                    permission.capability,
                    serde_json::to_string(&permission.constraint).map_err(to_sql_error)?
                ],
            )
        })?;
        Ok(())
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<(), PluginError> {
        if enabled {
            let view = self.get(id)?;
            let granted = view
                .granted_permissions
                .iter()
                .map(|permission| permission.capability.as_str())
                .collect::<BTreeSet<_>>();
            if view
                .metadata
                .requested_permissions
                .iter()
                .any(|permission| !granted.contains(permission.as_str()))
            {
                return Err(PluginError::CapabilityDenied(
                    "all requested capabilities must be granted before enabling".into(),
                ));
            }
        }
        let changed = self.pool.with_connection(|connection| {
            connection.execute(
                "UPDATE plugin_installations SET enabled=?2 WHERE id=?1",
                params![id, enabled],
            )
        })?;
        if changed == 1 {
            Ok(())
        } else {
            Err(PluginError::NotFound)
        }
    }

    pub fn remove(&self, id: &str) -> Result<(), PluginError> {
        let in_use = self.pool.with_connection(|connection| {
            connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM source_instances
                 WHERE json_extract(config_json, '$.pluginInstallationId')=?1)",
                [id],
                |row| row.get::<_, bool>(0),
            )
        })?;
        if in_use {
            return Err(PluginError::InUse);
        }
        let changed = self.pool.with_connection(|connection| {
            connection.execute("DELETE FROM plugin_installations WHERE id=?1", [id])
        })?;
        if changed != 1 {
            return Err(PluginError::NotFound);
        }
        self.cache
            .lock()
            .map_err(|_| PluginError::Trap("component cache lock poisoned".into()))?
            .remove(id);
        Ok(())
    }

    pub async fn validate_source_config(
        &self,
        owner_user_id: &str,
        config: &PluginSourceConfig,
    ) -> Result<Value, PluginError> {
        if !config.enabled {
            return Ok(config.plugin_config.clone());
        }
        let component = self.component(&config.plugin_installation_id, true)?;
        let mut invocation = self
            .instantiate(&config.plugin_installation_id, owner_user_id, &component)
            .await?;
        let config_json = serde_json::to_string(&config.plugin_config)?;
        let result = invocation
            .plugin
            .rill_source_plugin_source()
            .call_validate_config(&mut invocation.store, &config_json)
            .await
            .map_err(trap)?
            .map_err(PluginError::Invalid)?;
        self.bounded_json(&result)
    }

    pub async fn poll(
        &self,
        source_id: &str,
        config: &PluginSourceConfig,
        cursor: Option<&Value>,
        limit: usize,
    ) -> Result<SourceBatch, PluginError> {
        if !config.enabled {
            return Ok(SourceBatch {
                items: Vec::new(),
                cursor: cursor.cloned(),
                not_modified: true,
            });
        }
        let batch: Result<SourceBatch, PluginError> = async {
            let owner: String = self.pool.with_connection(|connection| {
                connection.query_row(
                    "SELECT owner_user_id FROM source_instances WHERE id=?1",
                    [source_id],
                    |row| row.get(0),
                )
            })?;
            let component = self.component(&config.plugin_installation_id, true)?;
            let mut invocation = self
                .instantiate(&config.plugin_installation_id, &owner, &component)
                .await?;
            let config_json = serde_json::to_string(&config.plugin_config)?;
            let cursor_json = cursor.map(serde_json::to_string).transpose()?;
            let result = invocation
                .plugin
                .rill_source_plugin_source()
                .call_poll(
                    &mut invocation.store,
                    &config_json,
                    cursor_json.as_deref(),
                    u32::try_from(limit.min(500)).unwrap_or(500),
                )
                .await
                .map_err(trap)?
                .map_err(PluginError::Invalid)?;
            self.bounded_json(&result)
        }
        .await;
        match batch {
            Ok(batch) => {
                self.record_health(&config.plugin_installation_id, None)?;
                Ok(batch)
            }
            Err(error) => {
                self.record_health(&config.plugin_installation_id, Some(&error.to_string()))?;
                Err(error)
            }
        }
    }

    async fn inspect_component(
        &self,
        component: &Component,
    ) -> Result<(PluginMetadata, Value), PluginError> {
        let mut invocation = self.instantiate_unprivileged(component).await?;
        let source = invocation.plugin.rill_source_plugin_source();
        let metadata = source
            .call_metadata(&mut invocation.store)
            .await
            .map_err(trap)?;
        let schema = source
            .call_config_schema(&mut invocation.store)
            .await
            .map_err(trap)?;
        Ok((self.bounded_json(&metadata)?, self.bounded_json(&schema)?))
    }

    async fn instantiate_unprivileged(
        &self,
        component: &Component,
    ) -> Result<Invocation, PluginError> {
        self.instantiate_with_state(component, HostState::new(self))
            .await
    }

    async fn instantiate(
        &self,
        id: &str,
        owner_user_id: &str,
        component: &Component,
    ) -> Result<Invocation, PluginError> {
        let mut state = HostState::new(self);
        for permission in self.permissions(id)? {
            if permission.capability == "http" {
                if let Some(hosts) = permission.constraint.get("hosts").and_then(Value::as_array) {
                    state.allowed_hosts.extend(
                        hosts
                            .iter()
                            .filter_map(Value::as_str)
                            .map(|host| host.to_ascii_lowercase()),
                    );
                }
            } else if let Some(name) = permission.capability.strip_prefix("secret:") {
                let secret_id = permission
                    .constraint
                    .get("secretId")
                    .and_then(Value::as_str)
                    .ok_or_else(|| PluginError::Invalid("secretId constraint is missing".into()))?;
                let owned = self.pool.with_connection(|connection| {
                    connection.query_row(
                        "SELECT owner_user_id=?2 FROM encrypted_secrets WHERE id=?1",
                        params![secret_id, owner_user_id],
                        |row| row.get::<_, bool>(0),
                    )
                })?;
                if !owned {
                    return Err(PluginError::CapabilityDenied(
                        "secret does not belong to source owner".into(),
                    ));
                }
                state.named_secrets.insert(name.into(), secret_id.into());
            }
        }
        self.instantiate_with_state(component, state).await
    }

    async fn instantiate_with_state(
        &self,
        component: &Component,
        state: HostState,
    ) -> Result<Invocation, PluginError> {
        let mut store = Store::new(&self.engine, state);
        store.limiter(|state| &mut state.limits);
        store.set_fuel(self.limits.fuel).map_err(trap)?;
        let timeout_ticks = self
            .limits
            .timeout
            .as_nanos()
            .div_ceil(EPOCH_TICK.as_nanos())
            .max(1);
        store.set_epoch_deadline(u64::try_from(timeout_ticks).unwrap_or(u64::MAX));
        let mut linker = Linker::new(&self.engine);
        Plugin::add_to_linker::<_, HasSelf<_>>(&mut linker, |state| state).map_err(trap)?;
        let plugin = Plugin::instantiate_async(&mut store, component, &linker)
            .await
            .map_err(trap)?;
        Ok(Invocation { plugin, store })
    }

    fn component(&self, id: &str, require_enabled: bool) -> Result<Arc<Component>, PluginError> {
        if let Some(component) = self
            .cache
            .lock()
            .map_err(|_| PluginError::Trap("component cache lock poisoned".into()))?
            .get(id)
            .cloned()
        {
            if !require_enabled || self.get(id)?.enabled {
                return Ok(component);
            }
            return Err(PluginError::Disabled);
        }
        let record: Option<(Vec<u8>, bool)> = self.pool.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT pc.component_bytes, pi.enabled FROM plugin_components pc
                     JOIN plugin_installations pi ON pi.id=pc.plugin_installation_id
                     WHERE pc.plugin_installation_id=?1",
                    [id],
                    |row| Ok((row.get(0)?, row.get::<_, i64>(1)? != 0)),
                )
                .optional()
        })?;
        let (bytes, enabled) = record.ok_or(PluginError::NotFound)?;
        if require_enabled && !enabled {
            return Err(PluginError::Disabled);
        }
        let component = Arc::new(
            Component::new(&self.engine, bytes)
                .map_err(|error| PluginError::Invalid(error.to_string()))?,
        );
        self.cache
            .lock()
            .map_err(|_| PluginError::Trap("component cache lock poisoned".into()))?
            .insert(id.into(), component.clone());
        Ok(component)
    }

    fn permissions(&self, id: &str) -> Result<Vec<PluginPermission>, PluginError> {
        let connection = self.pool.connection()?;
        let mut statement = connection.prepare(
            "SELECT capability, constraint_json FROM plugin_permissions
             WHERE plugin_installation_id=?1 ORDER BY capability",
        )?;
        let rows = statement.query_map([id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.map(|row| {
            let (capability, raw) = row?;
            Ok(PluginPermission {
                capability,
                constraint: serde_json::from_str(&raw)?,
            })
        })
        .collect()
    }

    fn bounded_json<T: serde::de::DeserializeOwned>(&self, raw: &str) -> Result<T, PluginError> {
        if raw.len() > self.limits.maximum_output_bytes {
            return Err(PluginError::OutputTooLarge(
                self.limits.maximum_output_bytes,
            ));
        }
        Ok(serde_json::from_str(raw)?)
    }

    fn record_health(&self, id: &str, error: Option<&str>) -> Result<(), PluginError> {
        self.pool.with_connection(|connection| match error {
            None => connection.execute(
                "INSERT INTO plugin_health(plugin_installation_id, last_success_at,
                 consecutive_failures, updated_at) VALUES (?1, unixepoch(), 0, unixepoch())
                 ON CONFLICT(plugin_installation_id) DO UPDATE SET last_success_at=unixepoch(),
                 consecutive_failures=0, last_error_message=NULL, updated_at=unixepoch()",
                [id],
            ),
            Some(error) => connection.execute(
                "INSERT INTO plugin_health(plugin_installation_id, consecutive_failures,
                 last_error_message, updated_at) VALUES (?1, 1, ?2, unixepoch())
                 ON CONFLICT(plugin_installation_id) DO UPDATE SET
                 consecutive_failures=plugin_health.consecutive_failures+1,
                 last_error_message=excluded.last_error_message, updated_at=unixepoch()",
                params![id, error.chars().take(500).collect::<String>()],
            ),
        })?;
        Ok(())
    }
}

struct Invocation {
    plugin: Plugin,
    store: Store<HostState>,
}
