use rill_db::{DbError, DbPool};
use rill_domain::NormalizedDocument;
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use sha2::{Digest, Sha256};
use thiserror::Error;
use url::Url;
use uuid::Uuid;

const TRACKING_PARAMETERS: &[&str] = &[
    "fbclid", "gclid", "dclid", "mc_cid", "mc_eid", "mkt_tok", "ref_src", "igshid",
];

#[derive(Debug, Error)]
pub enum DedupError {
    #[error("invalid canonical URL: {0}")]
    Url(String),
    #[error("database error: {0}")]
    Database(#[from] DbError),
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

#[derive(Debug, Clone)]
pub struct CuratorProvenance {
    pub curator_kind: String,
    pub curator_id: String,
    pub source_instance_id: Option<String>,
    pub raw_item_id: Option<String>,
    pub collection_entry_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DedupResult {
    pub document_id: String,
    pub story_id: String,
    pub created: bool,
}

#[derive(Clone)]
pub struct DedupService {
    pool: DbPool,
}

impl DedupService {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }

    pub fn upsert_document(
        &self,
        document: &NormalizedDocument,
        provenance: &CuratorProvenance,
    ) -> Result<DedupResult, DedupError> {
        let canonical_url = document
            .canonical_url
            .as_deref()
            .map(canonicalize_url)
            .transpose()?;
        let content_hash = content_checksum(&document.title, &document.body_text);
        let mut connection = self.pool.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let by_url = canonical_url.as_deref().and_then(|url| {
            transaction.query_row(
                "SELECT document_id FROM canonical_urls WHERE normalized_url = ?1 AND visibility_scope = ?2",
                params![url, document.visibility_scope], |row| row.get(0),
            ).optional().transpose()
        }).transpose()?;
        let existing = match by_url {
            Some(id) => Some(id),
            None => transaction.query_row(
                "SELECT id FROM documents WHERE visibility_scope = ?1 AND exact_content_hash = ?2",
                params![document.visibility_scope, content_hash.as_slice()], |row| row.get(0),
            ).optional()?,
        };
        let (document_id, created) = match existing {
            Some(id) => (id, false),
            None => {
                let id = Uuid::new_v4().to_string();
                transaction.execute(
                    "INSERT INTO documents(id, visibility_scope, exact_content_hash, title, body_text,\n\
                     sanitized_html, author, publisher, canonical_url, language, published_at)\n\
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                    params![id, document.visibility_scope, content_hash.as_slice(), document.title,
                        document.body_text, document.sanitized_html, document.author, document.publisher,
                        canonical_url, document.language, document.published_at],
                )?;
                (id, true)
            }
        };
        if !created {
            transaction.execute(
                "UPDATE documents SET\n\
                 title = CASE WHEN length(?2) > length(title) THEN ?2 ELSE title END,\n\
                 body_text = CASE WHEN length(?3) > length(body_text) THEN ?3 ELSE body_text END,\n\
                 sanitized_html = coalesce(?4, sanitized_html), author = coalesce(?5, author),\n\
                 publisher = coalesce(?6, publisher), canonical_url = coalesce(?7, canonical_url),\n\
                 language = coalesce(?8, language), published_at = coalesce(?9, published_at),\n\
                 updated_at = unixepoch() WHERE id = ?1",
                params![document_id, document.title, document.body_text, document.sanitized_html,
                    document.author, document.publisher, canonical_url, document.language,
                    document.published_at],
            )?;
        }
        if let Some(url) = &canonical_url {
            transaction.execute(
                "INSERT INTO canonical_urls(normalized_url, visibility_scope, document_id, original_url)\n\
                 VALUES (?1, ?2, ?3, ?4) ON CONFLICT(normalized_url, visibility_scope) DO NOTHING",
                params![url, document.visibility_scope, document_id,
                    document.canonical_url.as_deref().unwrap_or(url)],
            )?;
        }
        transaction.execute(
            "INSERT INTO document_curators(document_id, curator_kind, curator_id, source_instance_id,\n\
             raw_item_id, collection_entry_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6)\n\
             ON CONFLICT(document_id, curator_kind, curator_id) DO UPDATE SET\n\
             source_instance_id = coalesce(excluded.source_instance_id, source_instance_id),\n\
             raw_item_id = coalesce(excluded.raw_item_id, raw_item_id),\n\
             collection_entry_id = coalesce(excluded.collection_entry_id, collection_entry_id)",
            params![document_id, provenance.curator_kind, provenance.curator_id,
                provenance.source_instance_id, provenance.raw_item_id, provenance.collection_entry_id],
        )?;
        let story_id: Option<String> = transaction.query_row(
            "SELECT story_id FROM story_memberships WHERE document_id = ?1 ORDER BY added_at LIMIT 1",
            [&document_id], |row| row.get(0),
        ).optional()?;
        let story_id = match story_id {
            Some(id) => id,
            None => {
                let id = Uuid::new_v4().to_string();
                transaction.execute(
                    "INSERT INTO stories(id, visibility_scope, cluster_version, anchor_document_id)\n\
                     VALUES (?1, ?2, 'exact-v1', ?3)",
                    params![id, document.visibility_scope, document_id],
                )?;
                transaction.execute(
                    "INSERT INTO story_memberships(story_id, document_id, similarity) VALUES (?1, ?2, 1.0)",
                    params![id, document_id],
                )?;
                id
            }
        };
        transaction.commit()?;
        Ok(DedupResult {
            document_id,
            story_id,
            created,
        })
    }
}

pub fn canonicalize_url(value: &str) -> Result<String, DedupError> {
    let mut url = Url::parse(value).map_err(|error| DedupError::Url(error.to_string()))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(DedupError::Url(
            "URL must be absolute http or https".to_owned(),
        ));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(DedupError::Url("URL credentials are forbidden".to_owned()));
    }
    url.set_fragment(None);
    if (url.scheme() == "http" && url.port() == Some(80))
        || (url.scheme() == "https" && url.port() == Some(443))
    {
        url.set_port(None)
            .map_err(|_| DedupError::Url("invalid port".to_owned()))?;
    }
    let mut pairs = url
        .query_pairs()
        .filter(|(key, _)| !is_tracking_parameter(key))
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();
    pairs.sort();
    url.set_query(None);
    if !pairs.is_empty() {
        url.query_pairs_mut().extend_pairs(pairs);
    }
    let path = url.path().to_owned();
    if path.len() > 1 && path.ends_with('/') {
        url.set_path(path.trim_end_matches('/'));
    }
    Ok(url.to_string())
}

pub fn content_checksum(title: &str, body: &str) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(normalize_text(title).as_bytes());
    hash.update(b"\n\0\n");
    hash.update(normalize_text(body).as_bytes());
    hash.finalize().into()
}

fn normalize_text(value: &str) -> String {
    value
        .split_whitespace()
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_tracking_parameter(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key.starts_with("utm_") || TRACKING_PARAMETERS.contains(&key.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document(scope: &str, url: &str) -> NormalizedDocument {
        NormalizedDocument {
            visibility_scope: scope.into(),
            title: "A story".into(),
            body_text: "The same body".into(),
            sanitized_html: None,
            author: None,
            publisher: Some("example.com".into()),
            canonical_url: Some(url.into()),
            language: Some("en".into()),
            published_at: None,
        }
    }

    fn provenance(id: &str) -> CuratorProvenance {
        CuratorProvenance {
            curator_kind: "source".into(),
            curator_id: id.into(),
            source_instance_id: None,
            raw_item_id: None,
            collection_entry_id: None,
        }
    }

    #[test]
    fn canonicalization_removes_tracking_and_sorts_query() {
        assert_eq!(
            canonicalize_url("https://Example.com:443/article/?utm_source=x&b=2&a=1#section")
                .unwrap(),
            "https://example.com/article?a=1&b=2",
        );
    }

    #[test]
    fn same_article_converges_but_preserves_curators() {
        let pool = DbPool::open_in_memory().unwrap();
        let service = DedupService::new(pool.clone());
        let first = service
            .upsert_document(
                &document("public", "https://example.com/a?utm_source=rss"),
                &provenance("rss"),
            )
            .unwrap();
        let second = service
            .upsert_document(
                &document("public", "https://example.com/a#telegram"),
                &provenance("telegram"),
            )
            .unwrap();
        assert_eq!(first.document_id, second.document_id);
        assert!(first.created);
        assert!(!second.created);
        let count: i64 = pool
            .with_connection(|connection| {
                connection.query_row(
                    "SELECT count(*) FROM document_curators WHERE document_id = ?1",
                    [&first.document_id],
                    |row| row.get(0),
                )
            })
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn private_visibility_scopes_never_converge() {
        let pool = DbPool::open_in_memory().unwrap();
        let service = DedupService::new(pool);
        let first = service
            .upsert_document(
                &document("user:a", "https://example.com/a"),
                &provenance("a"),
            )
            .unwrap();
        let second = service
            .upsert_document(
                &document("user:b", "https://example.com/a"),
                &provenance("b"),
            )
            .unwrap();
        assert_ne!(first.document_id, second.document_id);
    }
}
