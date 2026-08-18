use std::collections::HashSet;

use rusqlite::{OptionalExtension, params};
use serde::Serialize;
use serde_json::json;

use crate::{
    IntelligenceError, IntelligenceService, RankedStory, streams::affinity_score, unix_now,
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CuratorPathView {
    pub kind: String,
    pub curator_id: String,
    pub source_instance_id: Option<String>,
    pub source_name: Option<String>,
    pub curator_commentary: Option<String>,
    pub parent_title: Option<String>,
    pub parent_url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoryVariantView {
    pub document_id: String,
    pub title: String,
    pub summary: String,
    pub body_text: String,
    pub sanitized_html: Option<String>,
    pub canonical_url: Option<String>,
    pub original_url: Option<String>,
    pub author: Option<String>,
    pub publisher: Option<String>,
    pub language: Option<String>,
    pub published_at: Option<i64>,
    pub updated_at: i64,
    pub curators: Vec<CuratorPathView>,
    pub selected: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoryDetailView {
    pub story_id: String,
    pub representative: StoryVariantView,
    pub variants: Vec<StoryVariantView>,
    pub coverage_count: u32,
    pub first_published_at: Option<i64>,
    pub latest_published_at: Option<i64>,
    pub read: bool,
    pub favorite: bool,
    pub explicit_feedback: Option<String>,
}

impl IntelligenceService {
    pub fn topic_stories(
        &self,
        user_id: &str,
        topic: &str,
        limit: usize,
    ) -> Result<Vec<RankedStory>, IntelligenceError> {
        let topic = topic.trim();
        if topic.is_empty() || topic.chars().count() > 80 {
            return Err(IntelligenceError::Invalid("topic is invalid".into()));
        }
        let private_scope = format!("user:{user_id}");
        let sql_limit = i64::try_from(limit.min(200)).unwrap_or(200);
        let connection = self.pool.connection()?;
        let mut statement = connection.prepare(
            "SELECT st.story_id FROM story_topics st
             WHERE st.topic=?1 COLLATE NOCASE AND EXISTS (
               SELECT 1 FROM story_memberships sm JOIN documents d ON d.id=sm.document_id
               WHERE sm.story_id=st.story_id AND (d.visibility_scope=?2 OR EXISTS (
                 SELECT 1 FROM document_access da WHERE da.document_id=d.id
                   AND (da.user_id IS NULL OR da.user_id=?3))))
             ORDER BY (
               SELECT max(coalesce(d.published_at, d.created_at))
               FROM story_memberships sm JOIN documents d ON d.id=sm.document_id
               WHERE sm.story_id=st.story_id
             ) DESC LIMIT ?4",
        )?;
        let rows = statement
            .query_map(params![topic, private_scope, user_id, sql_limit], |row| {
                row.get(0)
            })?;
        let story_ids = rows.collect::<rusqlite::Result<Vec<String>>>()?;
        drop(statement);
        drop(connection);
        self.story_summaries(user_id, &story_ids, "topic")
    }

    pub fn library_stories(
        &self,
        user_id: &str,
        kind: &str,
        limit: usize,
    ) -> Result<Vec<RankedStory>, IntelligenceError> {
        let predicate = match kind {
            "favorites" => "favorite=1",
            "history" => "read_at IS NOT NULL",
            _ => {
                return Err(IntelligenceError::Invalid("unknown library kind".into()));
            }
        };
        let connection = self.pool.connection()?;
        let sql = format!(
            "SELECT story_id FROM user_story_state WHERE user_id=?1 AND {predicate}
             ORDER BY updated_at DESC LIMIT ?2"
        );
        let mut statement = connection.prepare(&sql)?;
        let sql_limit = i64::try_from(limit.min(200)).unwrap_or(200);
        let rows = statement.query_map(params![user_id, sql_limit], |row| row.get(0))?;
        let story_ids = rows.collect::<rusqlite::Result<Vec<String>>>()?;
        drop(statement);
        drop(connection);
        self.story_summaries(user_id, &story_ids, kind)
    }

    pub fn story_summaries(
        &self,
        user_id: &str,
        story_ids: &[String],
        context: &str,
    ) -> Result<Vec<RankedStory>, IntelligenceError> {
        let mut stories = Vec::with_capacity(story_ids.len());
        let mut seen = HashSet::new();
        for story_id in story_ids.iter().take(200) {
            if !seen.insert(story_id.clone()) {
                continue;
            }
            let detail = match self.story_detail(user_id, story_id) {
                Ok(detail) => detail,
                Err(IntelligenceError::NotFound) => continue,
                Err(error) => return Err(error),
            };
            let topics = self.story_topics(&detail.story_id)?;
            stories.push(RankedStory {
                story_id: detail.story_id,
                document_id: detail.representative.document_id,
                title: detail.representative.title,
                summary: detail.representative.summary,
                canonical_url: detail.representative.canonical_url,
                publisher: detail.representative.publisher,
                published_at: detail.representative.published_at,
                coverage: detail.coverage_count,
                topics,
                score: 0.0,
                explanation: json!({"context": context}),
            });
        }
        Ok(stories)
    }

    pub fn story_detail(
        &self,
        user_id: &str,
        story_id: &str,
    ) -> Result<StoryDetailView, IntelligenceError> {
        let private_scope = format!("user:{user_id}");
        let connection = self.pool.connection()?;
        let state = connection
            .query_row(
                "SELECT uss.selected_document_id, uss.read_at IS NOT NULL,
                 coalesce(uss.favorite, 0), uss.explicit_feedback
                 FROM stories s LEFT JOIN user_story_state uss
                   ON uss.user_id=?1 AND uss.story_id=s.id
                 WHERE s.id=?2 AND EXISTS (\n\
                   SELECT 1 FROM story_memberships visible_sm\n\
                   JOIN documents visible_d ON visible_d.id=visible_sm.document_id\n\
                   WHERE visible_sm.story_id=s.id AND (visible_d.visibility_scope=?3 OR EXISTS (\n\
                     SELECT 1 FROM document_access da WHERE da.document_id=visible_d.id\n\
                       AND (da.user_id IS NULL OR da.user_id=?1))))",
                params![user_id, story_id, private_scope],
                |row| {
                    Ok((
                        row.get::<_, Option<String>>(0)?,
                        row.get::<_, i64>(1)? != 0,
                        row.get::<_, i64>(2)? != 0,
                        row.get::<_, Option<String>>(3)?,
                    ))
                },
            )
            .optional()?
            .ok_or(IntelligenceError::NotFound)?;
        let (preferred_document_id, read, favorite, explicit_feedback) = state;

        let mut statement = connection.prepare(
            "SELECT d.id, d.title,
             coalesce((SELECT su.summary_text FROM summaries su
               WHERE su.entity_type='document' AND su.entity_id=d.id
                 AND su.input_checksum=d.exact_content_hash
               ORDER BY su.created_at DESC LIMIT 1), substr(d.body_text, 1, 600)),
             d.body_text, d.sanitized_html, d.canonical_url,
             (SELECT cu.original_url FROM canonical_urls cu
               WHERE cu.document_id=d.id ORDER BY cu.created_at LIMIT 1),
             d.author, d.publisher, d.language, d.published_at, d.updated_at
             FROM story_memberships sm JOIN documents d ON d.id=sm.document_id
             WHERE sm.story_id=?1 AND (d.visibility_scope=?2 OR EXISTS (
               SELECT 1 FROM document_access da WHERE da.document_id=d.id
                 AND (da.user_id IS NULL OR da.user_id=?3)))
             ORDER BY coalesce(d.published_at, d.created_at), d.id",
        )?;
        let rows = statement.query_map(params![story_id, private_scope, user_id], |row| {
            Ok(StoryVariantView {
                document_id: row.get(0)?,
                title: row.get(1)?,
                summary: row.get(2)?,
                body_text: row.get(3)?,
                sanitized_html: row.get(4)?,
                canonical_url: row.get(5)?,
                original_url: row.get(6)?,
                author: row.get(7)?,
                publisher: row.get(8)?,
                language: row.get(9)?,
                published_at: row.get(10)?,
                updated_at: row.get(11)?,
                curators: Vec::new(),
                selected: false,
            })
        })?;
        let mut variants = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        drop(statement);
        if variants.is_empty() {
            return Err(IntelligenceError::NotFound);
        }
        for variant in &mut variants {
            variant.curators = load_curators(&connection, user_id, &variant.document_id)?;
        }
        drop(connection);

        let representative_index = choose_representative(
            &self.pool,
            user_id,
            &variants,
            preferred_document_id.as_deref(),
        )?;
        variants[representative_index].selected = true;
        let representative = variants[representative_index].clone();
        let first_published_at = variants.iter().filter_map(|item| item.published_at).min();
        let latest_published_at = variants.iter().filter_map(|item| item.published_at).max();
        Ok(StoryDetailView {
            story_id: story_id.to_owned(),
            representative,
            coverage_count: u32::try_from(variants.len()).unwrap_or(u32::MAX),
            variants,
            first_published_at,
            latest_published_at,
            read,
            favorite,
            explicit_feedback,
        })
    }

    pub fn representative_document_id(
        &self,
        user_id: &str,
        story_id: &str,
    ) -> Result<String, IntelligenceError> {
        Ok(self
            .story_detail(user_id, story_id)?
            .representative
            .document_id)
    }

    pub fn select_story_variant(
        &self,
        user_id: &str,
        story_id: &str,
        document_id: &str,
    ) -> Result<(), IntelligenceError> {
        self.ensure_visible_membership(user_id, story_id, document_id)?;
        self.pool.with_connection(|connection| {
            let transaction = connection.unchecked_transaction()?;
            transaction.execute(
                "INSERT INTO user_story_state(user_id, story_id, selected_document_id)
                 VALUES (?1, ?2, ?3) ON CONFLICT(user_id, story_id) DO UPDATE SET
                 selected_document_id=excluded.selected_document_id, updated_at=unixepoch()",
                params![user_id, story_id, document_id],
            )?;
            transaction.execute(
                "DELETE FROM recommendation_runs WHERE user_id=?1",
                [user_id],
            )?;
            transaction.commit()
        })?;
        Ok(())
    }

    pub fn set_story_read(
        &self,
        user_id: &str,
        story_id: &str,
        read: bool,
    ) -> Result<(), IntelligenceError> {
        self.representative_document_id(user_id, story_id)?;
        self.pool.with_connection(|connection| {
            let transaction = connection.unchecked_transaction()?;
            transaction.execute(
                "INSERT INTO user_story_state(user_id, story_id, selected_document_id, read_at)
                 VALUES (?1, ?2, NULL, CASE WHEN ?3 THEN unixepoch() END)
                 ON CONFLICT(user_id, story_id) DO UPDATE SET
                 read_at=CASE WHEN ?3 THEN unixepoch() END, updated_at=unixepoch()",
                params![user_id, story_id, read],
            )?;
            transaction.execute(
                "DELETE FROM recommendation_runs WHERE user_id=?1",
                [user_id],
            )?;
            transaction.commit()
        })?;
        Ok(())
    }

    fn ensure_visible_membership(
        &self,
        user_id: &str,
        story_id: &str,
        document_id: &str,
    ) -> Result<(), IntelligenceError> {
        let private_scope = format!("user:{user_id}");
        let exists: bool = self.pool.with_connection(|connection| {
            connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM stories s JOIN story_memberships sm ON sm.story_id=s.id
                 JOIN documents d ON d.id=sm.document_id WHERE s.id=?1 AND d.id=?2
                 AND (d.visibility_scope=?3 OR EXISTS (
                   SELECT 1 FROM document_access da WHERE da.document_id=d.id
                     AND (da.user_id IS NULL OR da.user_id=?4))))",
                params![story_id, document_id, private_scope, user_id],
                |row| row.get(0),
            )
        })?;
        if exists {
            Ok(())
        } else {
            Err(IntelligenceError::NotFound)
        }
    }
}

fn load_curators(
    connection: &rusqlite::Connection,
    user_id: &str,
    document_id: &str,
) -> rusqlite::Result<Vec<CuratorPathView>> {
    let mut statement = connection.prepare(
        "SELECT dc.curator_kind, dc.curator_id, dc.source_instance_id, si.name,
         ce.commentary, parent.title, parent.source_url
         FROM document_curators dc
         LEFT JOIN source_instances si ON si.id=dc.source_instance_id
         LEFT JOIN collection_entries ce ON ce.id=dc.collection_entry_id
         LEFT JOIN raw_items parent ON parent.id=ce.parent_raw_item_id
         WHERE dc.document_id=?1 AND (dc.source_instance_id IS NULL OR EXISTS (
           SELECT 1 FROM source_access sa WHERE sa.source_instance_id=dc.source_instance_id
             AND (sa.user_id IS NULL OR sa.user_id=?2)))
         ORDER BY dc.created_at, dc.curator_kind, dc.curator_id",
    )?;
    let rows = statement.query_map(params![document_id, user_id], |row| {
        Ok(CuratorPathView {
            kind: row.get(0)?,
            curator_id: row.get(1)?,
            source_instance_id: row.get(2)?,
            source_name: row.get(3)?,
            curator_commentary: row.get(4)?,
            parent_title: row.get(5)?,
            parent_url: row.get(6)?,
        })
    })?;
    rows.collect()
}

fn choose_representative(
    pool: &rill_db::DbPool,
    user_id: &str,
    variants: &[StoryVariantView],
    preferred_document_id: Option<&str>,
) -> Result<usize, IntelligenceError> {
    if let Some(index) = preferred_document_id.and_then(|preferred| {
        variants
            .iter()
            .position(|item| item.document_id == preferred)
    }) {
        return Ok(index);
    }
    let latest = variants
        .iter()
        .filter_map(|variant| variant.published_at)
        .max()
        .unwrap_or_else(unix_now);
    let mut best = (0_usize, f32::NEG_INFINITY);
    for (index, variant) in variants.iter().enumerate() {
        let sources = variant
            .curators
            .iter()
            .filter_map(|path| path.source_instance_id.clone())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let curators = variant
            .curators
            .iter()
            .map(|path| path.curator_id.clone())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let affinity = affinity_score(
            pool,
            user_id,
            variant.publisher.as_deref(),
            &sources,
            &curators,
        )?;
        let completeness = (variant.body_text.chars().count().min(8_000) as f32) / 8_000.0;
        let freshness = variant.published_at.map_or(0.0, |published| {
            1.0 - (latest.saturating_sub(published).max(0) as f32 / (30.0 * 86_400.0)).min(1.0)
        });
        let direct = if variant
            .curators
            .iter()
            .any(|path| path.parent_title.is_none())
        {
            0.12
        } else {
            0.0
        };
        let readability = if variant.sanitized_html.is_some() {
            0.10
        } else {
            0.0
        };
        let stable_url = if variant
            .canonical_url
            .as_deref()
            .is_some_and(|url| url.starts_with("https://"))
        {
            0.08
        } else {
            0.0
        };
        let score = affinity * 0.55
            + completeness * 0.15
            + freshness * 0.10
            + direct
            + readability
            + stable_url;
        if score > best.1 {
            best = (index, score);
        }
    }
    Ok(best.0)
}
