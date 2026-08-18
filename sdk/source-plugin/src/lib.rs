use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const ABI_VERSION: &str = "rill:source-plugin@1.0.0";

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Metadata {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    #[serde(default)]
    pub requested_permissions: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RawItem {
    pub external_id: String,
    pub item_kind: String,
    pub title: Option<String>,
    pub body_text: Option<String>,
    pub body_html: Option<String>,
    pub author: Option<String>,
    pub source_url: Option<String>,
    pub published_at: Option<i64>,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Batch {
    pub items: Vec<RawItem>,
    pub cursor: Option<Value>,
    pub not_modified: bool,
}

pub fn to_json<T: Serialize>(value: &T) -> Result<String, String> {
    serde_json::to_string(value).map_err(|error| error.to_string())
}

pub fn from_json<T: serde::de::DeserializeOwned>(value: &str) -> Result<T, String> {
    serde_json::from_str(value).map_err(|error| error.to_string())
}
