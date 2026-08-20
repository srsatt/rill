use rill_model_api::ModelIdentity;
use rusqlite::params;
use uuid::Uuid;

use crate::{
    IntelligenceError, IntelligenceService, StoredDocument, cosine, decode_vector, unix_now,
};

pub(crate) fn cluster_document(
    service: &IntelligenceService,
    document: &StoredDocument,
    vector: &[f32],
    identity: &ModelIdentity,
) -> Result<(), IntelligenceError> {
    let connection = service.pool.connection()?;
    let manual: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM manual_cluster_overrides WHERE document_id = ?1)",
        [&document.id],
        |row| row.get::<_, i64>(0).map(|value| value != 0),
    )?;
    if manual {
        return Ok(());
    }
    let current_story: String = connection.query_row(
        "SELECT story_id FROM story_memberships WHERE document_id = ?1",
        [&document.id],
        |row| row.get(0),
    )?;
    let cutoff = document
        .published_at
        .unwrap_or_else(unix_now)
        .saturating_sub(service.cluster_window_seconds);
    let ceiling = document
        .published_at
        .unwrap_or_else(unix_now)
        .saturating_add(service.cluster_window_seconds);
    let mut statement = connection.prepare(
        "SELECT s.id, s.anchor_document_id, er.vector_f32le
         FROM stories s
         JOIN documents d ON d.id = s.anchor_document_id
         JOIN embedding_records er ON er.entity_type = 'document' AND er.entity_id = d.id
         WHERE s.id != ?1 AND s.visibility_scope = ?2
           AND er.provider = ?3 AND er.model = ?4 AND er.model_version = ?5
           AND coalesce(d.published_at, d.created_at) >= ?6
           AND (d.language = ?7 OR d.language IS NULL OR ?7 IS NULL)
           AND coalesce(d.published_at, d.created_at) <= ?8
           AND (?9 IS NULL OR NOT EXISTS (
             SELECT 1 FROM story_memberships same_sm
             JOIN documents same_d ON same_d.id=same_sm.document_id
             WHERE same_sm.story_id=s.id AND lower(same_d.publisher)=lower(?9)
           ))
         ORDER BY coalesce(d.published_at, d.created_at) DESC LIMIT 500",
    )?;
    let rows = statement.query_map(
        params![
            current_story,
            document.visibility_scope,
            identity.provider,
            identity.model,
            identity.version,
            cutoff,
            document.language,
            ceiling,
            document.publisher,
        ],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Vec<u8>>(2)?,
            ))
        },
    )?;
    let mut best: Option<(String, String, f32)> = None;
    for row in rows {
        let (story_id, anchor_id, bytes) = row?;
        let Some(candidate) = decode_vector(&bytes) else {
            continue;
        };
        let Some(similarity) = cosine(vector, &candidate) else {
            continue;
        };
        if best
            .as_ref()
            .is_none_or(|(_, _, best_score)| similarity > *best_score)
        {
            best = Some((story_id, anchor_id, similarity));
        }
    }
    drop(statement);
    drop(connection);

    let Some((target_story, anchor_document, similarity)) = best else {
        return Ok(());
    };
    let matched = similarity >= service.cluster_threshold;
    let mut connection = service.pool.connection()?;
    let transaction = connection.transaction()?;
    transaction.execute(
        "INSERT INTO cluster_evidence(id, story_id, document_id, anchor_document_id,
         provider, model, model_version, similarity, decision)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            Uuid::new_v4().to_string(),
            target_story,
            document.id,
            anchor_document,
            identity.provider,
            identity.model,
            identity.version,
            similarity,
            if matched {
                "cluster"
            } else {
                "below-threshold"
            }
        ],
    )?;
    if matched {
        transaction.execute(
            "DELETE FROM story_memberships WHERE story_id = ?1 AND document_id = ?2",
            params![current_story, document.id],
        )?;
        transaction.execute(
            "INSERT INTO story_memberships(story_id, document_id, similarity)
             VALUES (?1, ?2, ?3) ON CONFLICT(story_id, document_id)
             DO UPDATE SET similarity=excluded.similarity",
            params![target_story, document.id, similarity],
        )?;
        let remaining: i64 = transaction.query_row(
            "SELECT count(*) FROM story_memberships WHERE story_id = ?1",
            [&current_story],
            |row| row.get(0),
        )?;
        if remaining == 0 {
            transaction.execute("DELETE FROM stories WHERE id = ?1", [&current_story])?;
        } else {
            transaction.execute(
                "UPDATE stories SET anchor_document_id=(
                   SELECT sm.document_id FROM story_memberships sm
                   JOIN documents d ON d.id=sm.document_id
                   WHERE sm.story_id=?1
                   ORDER BY coalesce(d.published_at, d.created_at) DESC, d.id LIMIT 1
                 ), updated_at=unixepoch()
                 WHERE id=?1 AND anchor_document_id=?2",
                params![current_story, document.id],
            )?;
        }
    }
    transaction.commit()?;
    Ok(())
}

pub fn manual_merge(
    service: &IntelligenceService,
    actor_user_id: &str,
    document_id: &str,
    target_story_id: &str,
) -> Result<(), IntelligenceError> {
    let mut connection = service.pool.connection()?;
    let transaction = connection.transaction()?;
    let current_story: String = transaction.query_row(
        "SELECT story_id FROM story_memberships WHERE document_id = ?1",
        [document_id],
        |row| row.get(0),
    )?;
    let target_exists: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM stories WHERE id = ?1)",
        [target_story_id],
        |row| row.get::<_, i64>(0).map(|value| value != 0),
    )?;
    if !target_exists {
        return Err(IntelligenceError::NotFound);
    }
    transaction.execute(
        "DELETE FROM story_memberships WHERE story_id = ?1 AND document_id = ?2",
        params![current_story, document_id],
    )?;
    transaction.execute(
        "INSERT INTO story_memberships(story_id, document_id) VALUES (?1, ?2)",
        params![target_story_id, document_id],
    )?;
    transaction.execute(
        "INSERT INTO manual_cluster_overrides(id, document_id, target_story_id, operation,
         actor_user_id) VALUES (?1, ?2, ?3, 'merge', ?4)",
        params![
            Uuid::new_v4().to_string(),
            document_id,
            target_story_id,
            actor_user_id
        ],
    )?;
    transaction.execute(
        "DELETE FROM stories WHERE id = ?1 AND NOT EXISTS(
         SELECT 1 FROM story_memberships WHERE story_id = ?1)",
        [&current_story],
    )?;
    transaction.commit()?;
    Ok(())
}

pub fn manual_split(
    service: &IntelligenceService,
    actor_user_id: &str,
    document_id: &str,
) -> Result<String, IntelligenceError> {
    let document = service.load_document(document_id)?;
    let mut connection = service.pool.connection()?;
    let transaction = connection.transaction()?;
    let current_story: String = transaction.query_row(
        "SELECT story_id FROM story_memberships WHERE document_id = ?1",
        [document_id],
        |row| row.get(0),
    )?;
    let new_story = Uuid::new_v4().to_string();
    transaction.execute(
        "INSERT INTO stories(id, visibility_scope, cluster_version, anchor_document_id)
         VALUES (?1, ?2, 'manual', ?3)",
        params![new_story, document.visibility_scope, document_id],
    )?;
    transaction.execute(
        "DELETE FROM story_memberships WHERE story_id = ?1 AND document_id = ?2",
        params![current_story, document_id],
    )?;
    transaction.execute(
        "INSERT INTO story_memberships(story_id, document_id) VALUES (?1, ?2)",
        params![new_story, document_id],
    )?;
    transaction.execute(
        "INSERT INTO manual_cluster_overrides(id, document_id, target_story_id, operation,
         actor_user_id) VALUES (?1, ?2, ?3, 'split', ?4)",
        params![
            Uuid::new_v4().to_string(),
            document_id,
            new_story,
            actor_user_id
        ],
    )?;
    transaction.commit()?;
    Ok(new_story)
}
