impl IngestionService {
    pub fn new(pool: DbPool, maximum_collection_fan_out: usize) -> Self {
        Self {
            jobs: JobQueue::new(pool.clone()),
            dedup: DedupService::new(pool.clone()),
            pool,
            collection_parser: None,
            collection_policy: CollectionPolicy {
                maximum_fan_out: maximum_collection_fan_out.max(1),
                ..CollectionPolicy::default()
            },
        }
    }

    pub fn configure_collection_parser(
        mut self,
        provider: Option<Arc<dyn CollectionParserProvider>>,
    ) -> Self {
        self.collection_parser = provider;
        self
    }

    pub fn configure_collection_policy(
        mut self,
        threshold: f32,
        parent_display_policy: ParentDisplayPolicy,
        excluded_hosts: Vec<String>,
        excluded_path_fragments: Vec<String>,
    ) -> Self {
        self.collection_policy.threshold = threshold.clamp(0.0, 1.0);
        self.collection_policy.parent_display_policy = parent_display_policy;
        self.collection_policy.excluded_hosts = excluded_hosts;
        self.collection_policy.excluded_path_fragments = excluded_path_fragments;
        self
    }

    pub fn register_source(
        &self,
        kind: &str,
        name: &str,
        owner_user_id: Option<&str>,
        shared: bool,
        config: &Value,
    ) -> Result<SourceRegistration, IngestionError> {
        self.register_source_inner(kind, name, owner_user_id, shared, config, true)
    }

    pub fn register_plugin_source(
        &self,
        name: &str,
        owner_user_id: &str,
        shared: bool,
        config: &Value,
    ) -> Result<SourceRegistration, IngestionError> {
        self.register_source_inner("plugin", name, Some(owner_user_id), shared, config, false)
    }

    fn register_source_inner(
        &self,
        kind: &str,
        name: &str,
        owner_user_id: Option<&str>,
        shared: bool,
        config: &Value,
        built_in: bool,
    ) -> Result<SourceRegistration, IngestionError> {
        if name.trim().is_empty() {
            return Err(IngestionError::Invalid("source name is required".into()));
        }
        let visibility_scope = if shared {
            "public".to_owned()
        } else {
            format!(
                "user:{}",
                owner_user_id.ok_or_else(|| {
                    IngestionError::Invalid("private source requires an owner".into())
                })?
            )
        };
        let definition_id = if built_in {
            format!("builtin:{kind}")
        } else {
            "plugin:component".to_owned()
        };
        let source_id = Uuid::new_v4().to_string();
        let mut connection = self.pool.connection()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO source_definitions(id, kind, display_name, built_in) VALUES (?1, ?2, ?3, ?4)\n\
             ON CONFLICT(kind) DO NOTHING",
            params![definition_id, kind, name, built_in],
        )?;
        let persisted_definition: String = transaction.query_row(
            "SELECT id FROM source_definitions WHERE kind = ?1",
            [kind],
            |row| row.get(0),
        )?;
        transaction.execute(
            "INSERT INTO source_instances(id, definition_id, owner_user_id, name, visibility,\n\
             visibility_scope, config_json, audience) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                source_id,
                persisted_definition,
                owner_user_id,
                name.trim(),
                if shared { "public" } else { "private" },
                visibility_scope,
                serde_json::to_string(config)?,
                if shared { "global" } else { "owner" }
            ],
        )?;
        if let Some(user_id) = owner_user_id {
            transaction.execute(
                "INSERT INTO source_subscriptions(user_id, source_instance_id) VALUES (?1, ?2)",
                params![user_id, source_id],
            )?;
        }
        transaction.commit()?;
        drop(connection);
        self.schedule_poll(&source_id, None)?;
        Ok(SourceRegistration {
            id: source_id,
            visibility_scope,
        })
    }

    pub async fn poll_source(
        &self,
        connector: &dyn SourceConnector,
        context: &ConnectorContext,
        source_instance_id: &str,
        limit: usize,
    ) -> Result<IngestReport, IngestionError> {
        let (config, cursor) = self.load_source_poll_state(source_instance_id)?;
        let batch = connector
            .poll(context, &config, cursor.as_ref(), limit)
            .await;
        match batch {
            Ok(batch) => {
                let report = self.ingest_batch(source_instance_id, &batch)?;
                self.record_poll_success(source_instance_id, batch.cursor.as_ref())?;
                Ok(report)
            }
            Err(error) => {
                self.record_poll_failure(source_instance_id, &error.to_string())?;
                Err(error.into())
            }
        }
    }

    pub fn ingest_batch(
        &self,
        source_instance_id: &str,
        batch: &SourceBatch,
    ) -> Result<IngestReport, IngestionError> {
        let (visibility_scope, curator_id) = self.source_identity(source_instance_id)?;
        let (policy, source_mode) = self.collection_settings_for_source(source_instance_id)?;
        let mut report = IngestReport::default();
        for item in &batch.items {
            let raw_id = self.upsert_raw_item(source_instance_id, &visibility_scope, item)?;
            report.raw_items += 1;
            if item.deleted_at.is_some() {
                self.remove_deleted_item_content(&raw_id)?;
                continue;
            }
            self.enqueue_raw_jobs(&raw_id, item)?;
            let mode = self.manual_detection_mode(&raw_id)?.unwrap_or(source_mode);
            let base_url = item
                .source_url
                .as_deref()
                .and_then(|url| Url::parse(url).ok());
            let detection =
                detect_collection_with_diagnostics(item, base_url.as_ref(), mode, &policy);
            let diagnostics = json!({
                "mode": detection.mode,
                "ignoredLinks": detection.ignored_links,
            });
            let is_collection = matches!(detection.shape, ItemShape::Collection { .. });
            let show_parent = !is_collection
                || matches!(
                    policy.parent_display_policy,
                    ParentDisplayPolicy::ParentAndChildren | ParentDisplayPolicy::ParentOnly
                );
            if show_parent && let Some(document) = normalize_feed_item(item, &visibility_scope)? {
                let result = self.dedup.upsert_document(
                    &document,
                    &CuratorProvenance {
                        curator_kind: "source".into(),
                        curator_id: curator_id.clone(),
                        source_instance_id: Some(source_instance_id.to_owned()),
                        raw_item_id: Some(raw_id.clone()),
                        collection_entry_id: None,
                    },
                )?;
                report.documents_created += usize::from(result.created);
                self.enqueue_intelligence(&result.document_id, &document)?;
            }
            if let ItemShape::Collection {
                confidence,
                entries,
            } = detection.shape
            {
                report.collection_parents += 1;
                report.collection_children += self.store_collection(CollectionStorage {
                    source_id: source_instance_id,
                    raw_id: &raw_id,
                    parent: item,
                    confidence,
                    entries: &entries,
                    display_policy: policy.parent_display_policy,
                    diagnostics: &diagnostics,
                    parser_kind: "deterministic",
                    parser_version: "1",
                    extraction_method: "deterministic",
                })?;
            } else {
                self.record_collection_rejection(
                    &raw_id,
                    policy.parent_display_policy,
                    &diagnostics,
                )?;
            }
        }
        Ok(report)
    }
}
