use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LinkRelation(String);

impl LinkRelation {
    pub fn new(value: &str) -> Option<Self> {
        let value = value.trim().to_ascii_lowercase();
        (!value.is_empty()
            && value.len() <= 32
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')))
        .then_some(Self(value))
    }

    pub fn alternate() -> Self {
        Self("alternate".into())
    }

    pub fn replies() -> Self {
        Self("replies".into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalLink {
    pub url: String,
    pub relation: LinkRelation,
    pub title: Option<String>,
    pub ordinal: u32,
}

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
    #[serde(default, deserialize_with = "deserialize_external_links")]
    pub external_urls: Vec<ExternalLink>,
    #[serde(default)]
    pub media: Vec<RawMedia>,
    pub metadata: serde_json::Value,
}

fn deserialize_external_links<'de, D>(deserializer: D) -> Result<Vec<ExternalLink>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StoredLink {
        Typed(ExternalLink),
        Legacy(String),
    }

    Vec::<StoredLink>::deserialize(deserializer).map(|links| {
        links
            .into_iter()
            .enumerate()
            .map(|(ordinal, link)| match link {
                StoredLink::Typed(link) => link,
                StoredLink::Legacy(url) => ExternalLink {
                    url,
                    relation: LinkRelation::new("other").expect("static relation"),
                    title: None,
                    ordinal: u32::try_from(ordinal).unwrap_or(u32::MAX),
                },
            })
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_item_reads_legacy_untyped_links() {
        let item: RawSourceItem = serde_json::from_value(serde_json::json!({
            "externalId": "1", "itemKind": "article", "title": "One",
            "bodyText": "Body", "bodyHtml": null, "author": null,
            "sourceUrl": "https://example.com/one", "publishedAt": null,
            "externalUrls": ["https://example.com/legacy"], "media": [], "metadata": {}
        }))
        .unwrap();
        assert_eq!(item.external_urls[0].relation.as_str(), "other");
    }
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
    #[serde(default)]
    pub links: Vec<ExternalLink>,
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
