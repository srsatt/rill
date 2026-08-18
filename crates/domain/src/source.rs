use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    Rss,
    Telegram,
    Email,
    Plugin,
}

impl SourceKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Rss => "rss",
            Self::Telegram => "telegram",
            Self::Email => "email",
            Self::Plugin => "plugin",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawSourceItem {
    pub external_id: String,
    pub item_kind: String,
    pub title: Option<String>,
    pub body_text: Option<String>,
    pub body_html: Option<String>,
    pub author: Option<String>,
    pub source_url: Option<String>,
    pub published_at: Option<i64>,
    #[serde(default)]
    pub edited_at: Option<i64>,
    #[serde(default)]
    pub deleted_at: Option<i64>,
    #[serde(default)]
    pub external_urls: Vec<String>,
    #[serde(default)]
    pub media: Vec<RawMedia>,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawMedia {
    pub kind: String,
    pub url: Option<String>,
    pub mime_type: Option<String>,
    pub size_bytes: Option<u64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub group_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedDocument {
    pub visibility_scope: String,
    pub title: String,
    pub body_text: String,
    pub sanitized_html: Option<String>,
    pub author: Option<String>,
    pub publisher: Option<String>,
    pub canonical_url: Option<String>,
    pub language: Option<String>,
    pub published_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionEntryCandidate {
    pub url: String,
    pub title_hint: Option<String>,
    pub commentary: Option<String>,
    pub author_hint: Option<String>,
    pub published_at_hint: Option<i64>,
    pub ordinal: usize,
    pub confidence: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ItemShape {
    Single,
    Collection {
        confidence: f32,
        entries: Vec<CollectionEntryCandidate>,
    },
}
