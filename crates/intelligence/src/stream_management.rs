impl IntelligenceService {
    pub async fn update_stream(
        &self,
        user_id: &str,
        slug: &str,
        input: &UpdateStreamInput,
    ) -> Result<StreamView, IntelligenceError> {
        validate_stream(&input.name, slug)?;
        let stream_id = self.pool.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT id FROM streams WHERE owner_user_id=?1 AND slug=?2 AND enabled=1",
                    params![user_id, slug],
                    |row| row.get::<_, String>(0),
                )
                .optional()
        })?;
        let stream_id = stream_id.ok_or(IntelligenceError::NotFound)?;
        let filter_json = serde_json::to_string(&input.filter)?;
        self.pool.with_connection(|connection| {
            let transaction = connection.unchecked_transaction()?;
            transaction.execute(
                "UPDATE streams SET name=?3, icon=?4, filter_json=?5, definition_text=?6,
                 ranking_instruction=?7, updated_at=unixepoch()
                 WHERE owner_user_id=?1 AND slug=?2",
                params![
                    user_id,
                    slug,
                    input.name.trim(),
                    input.icon,
                    filter_json,
                    input.semantic_description,
                    input.ranking_instruction
                ],
            )?;
            transaction.execute(
                "DELETE FROM embedding_records WHERE entity_type='stream' AND entity_id=?1",
                [&stream_id],
            )?;
            transaction.execute(
                "DELETE FROM recommendation_runs WHERE user_id=?1",
                [user_id],
            )?;
            transaction.commit()
        })?;
        if let Some(description) = input
            .semantic_description
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            && let Err(error) = self.enqueue_stream_embedding(&stream_id, description)
        {
            warn!(error = %error, %stream_id, "semantic stream embedding could not be queued");
        }
        self.load_stream(user_id, slug)
    }

    pub fn delete_stream(&self, user_id: &str, slug: &str) -> Result<(), IntelligenceError> {
        if slug == "all" {
            return Err(IntelligenceError::Invalid(
                "built-in streams cannot be deleted".into(),
            ));
        }
        self.pool.with_connection(|connection| {
            let transaction = connection.unchecked_transaction()?;
            let stream_id = transaction
                .query_row(
                    "SELECT id FROM streams WHERE owner_user_id=?1 AND slug=?2",
                    params![user_id, slug],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            let stream_id = stream_id.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
            transaction.execute(
                "UPDATE device_sessions SET selected_stream_id=NULL WHERE user_id=?1
                 AND selected_stream_id=?2",
                params![user_id, stream_id],
            )?;
            transaction.execute(
                "DELETE FROM embedding_records WHERE entity_type='stream' AND entity_id=?1",
                [&stream_id],
            )?;
            transaction.execute("DELETE FROM streams WHERE id=?1", [&stream_id])?;
            transaction.execute(
                "DELETE FROM recommendation_runs WHERE user_id=?1",
                [user_id],
            )?;
            transaction.commit()
        })?;
        Ok(())
    }

    pub fn reorder_streams(
        &self,
        user_id: &str,
        slugs: &[String],
    ) -> Result<(), IntelligenceError> {
        let current = self
            .list_streams(user_id)?
            .into_iter()
            .map(|stream| stream.slug)
            .collect::<Vec<_>>();
        let requested = slugs.iter().collect::<std::collections::BTreeSet<_>>();
        let expected = current.iter().collect::<std::collections::BTreeSet<_>>();
        if slugs.len() != current.len() || requested.len() != slugs.len() || requested != expected {
            return Err(IntelligenceError::Invalid(
                "stream order must contain every stream exactly once".into(),
            ));
        }
        if slugs.first().is_none_or(|slug| slug != "all") {
            return Err(IntelligenceError::Invalid(
                "the All stream must remain first".into(),
            ));
        }
        self.pool.with_connection(|connection| {
            let transaction = connection.unchecked_transaction()?;
            for (position, slug) in slugs.iter().enumerate() {
                transaction.execute(
                    "UPDATE streams SET sort_order=?3, updated_at=unixepoch()
                     WHERE owner_user_id=?1 AND slug=?2",
                    params![
                        user_id,
                        slug,
                        i64::try_from(position).unwrap_or(i64::MAX)
                    ],
                )?;
            }
            transaction.execute(
                "DELETE FROM recommendation_runs WHERE user_id=?1",
                [user_id],
            )?;
            transaction.commit()
        })?;
        Ok(())
    }
}
