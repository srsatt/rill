//! Versioned contracts crossing Rill's renderer boundary.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::{Config, TS};

pub const RENDER_PROTOCOL_VERSION: u16 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RenderRequest {
    pub version: u16,
    pub template: String,
    pub mode: RenderMode,
    pub locale: String,
    pub render_id: String,
    #[ts(type = "unknown")]
    pub props: Value,
    pub assets: BTreeMap<String, String>,
    pub csrf_token: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum RenderMode {
    Modern,
    Reader,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RenderResponse {
    pub version: u16,
    pub status: u16,
    pub head_html: String,
    pub body_html: String,
    #[ts(type = "unknown")]
    pub hydration_state: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct StreamLink {
    pub name: String,
    pub slug: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct StoryCardModel {
    pub id: String,
    pub title: String,
    pub summary: String,
    pub source: String,
    pub curator: Option<String>,
    pub published_at: String,
    pub coverage_count: u32,
    pub reading_minutes: u32,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct FeedPageModel {
    pub title: String,
    pub active_stream: String,
    pub streams: Vec<StreamLink>,
    pub stories: Vec<StoryCardModel>,
    pub username: String,
    pub page: u32,
    pub previous_page: Option<u32>,
    pub next_page: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LibraryPageModel {
    pub title: String,
    pub username: String,
    pub kind: String,
    pub query: Option<String>,
    pub stories: Vec<StoryCardModel>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SourcesPageModel {
    pub title: String,
    pub username: String,
    pub email_available: bool,
    pub telegram_available: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CuratorPathModel {
    pub kind: String,
    pub curator_id: String,
    pub source_name: Option<String>,
    pub curator_commentary: Option<String>,
    pub parent_title: Option<String>,
    pub parent_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct StoryVariantModel {
    pub document_id: String,
    pub title: String,
    pub summary: String,
    pub body_text: String,
    pub canonical_url: Option<String>,
    pub author: Option<String>,
    pub publisher: Option<String>,
    pub language: Option<String>,
    pub published_at: Option<String>,
    pub curators: Vec<CuratorPathModel>,
    pub selected: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct StoryPageModel {
    pub title: String,
    pub story_id: String,
    pub representative: StoryVariantModel,
    pub variants: Vec<StoryVariantModel>,
    pub coverage_count: u32,
    pub read: bool,
    pub favorite: bool,
    pub explicit_feedback: Option<String>,
    pub reader: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ReaderPreferencesPageModel {
    pub title: String,
    pub username: String,
    pub streams: Vec<StreamLink>,
    pub active_stream: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LoginPageModel {
    pub title: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ReaderPairPageModel {
    pub title: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ReaderDeviceModel {
    pub id: String,
    pub label: String,
    #[ts(type = "number")]
    pub created_at: i64,
    #[ts(type = "number")]
    pub last_used_at: i64,
    #[ts(type = "number")]
    pub expires_at: i64,
    pub user_agent: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ReaderSettingsPageModel {
    pub title: String,
    pub username: String,
    pub devices: Vec<ReaderDeviceModel>,
    pub new_pairing_code: Option<String>,
    #[ts(type = "number | null")]
    pub pairing_expires_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AdminPageModel {
    pub title: String,
    pub username: String,
}

pub fn typescript_bindings() -> String {
    let config = Config::default();
    let declarations = [
        RenderMode::decl(&config),
        RenderRequest::decl(&config),
        RenderResponse::decl(&config),
        StreamLink::decl(&config),
        StoryCardModel::decl(&config),
        FeedPageModel::decl(&config),
        LibraryPageModel::decl(&config),
        SourcesPageModel::decl(&config),
        CuratorPathModel::decl(&config),
        StoryVariantModel::decl(&config),
        StoryPageModel::decl(&config),
        ReaderPreferencesPageModel::decl(&config),
        LoginPageModel::decl(&config),
        ReaderPairPageModel::decl(&config),
        ReaderDeviceModel::decl(&config),
        ReaderSettingsPageModel::decl(&config),
        AdminPageModel::decl(&config),
    ];
    format!(
        "// @generated from rill-contracts by cargo xtask generate-contracts. Do not edit.\n\n{}\n",
        declarations
            .map(|declaration| format!("export {declaration}"))
            .join("\n\n"),
    )
}
