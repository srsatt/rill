use rill_source_plugin_sdk::{Batch, Metadata, RawItem, to_json};
use serde_json::json;

wit_bindgen::generate!({
    path: "wit",
    world: "plugin",
});

struct Example;

impl exports::rill::source_plugin::source::Guest for Example {
    fn metadata() -> String {
        to_json(&Metadata {
            id: "example-rust".into(),
            name: "Example Rust source".into(),
            version: "1.0.0".into(),
            description: "Minimal Rill Component Model source".into(),
            requested_permissions: Vec::new(),
        })
        .unwrap_or_else(|_| "{}".into())
    }

    fn config_schema() -> String {
        json!({"type":"object","additionalProperties":false}).to_string()
    }

    fn validate_config(config_json: String) -> Result<String, String> {
        let value: serde_json::Value =
            serde_json::from_str(&config_json).map_err(|error| error.to_string())?;
        if value.as_object().is_some_and(serde_json::Map::is_empty) {
            Ok(config_json)
        } else {
            Err("configuration must be an empty object".into())
        }
    }

    fn poll(
        _config_json: String,
        _cursor_json: Option<String>,
        limit: u32,
    ) -> Result<String, String> {
        let items = (limit > 0)
            .then(|| RawItem {
                external_id: "welcome".into(),
                item_kind: "article".into(),
                title: Some("Welcome from Rust".into()),
                body_text: Some("A typed Rill source plugin item.".into()),
                body_html: None,
                author: Some("Rill example".into()),
                source_url: Some("https://example.com/rill-component".into()),
                published_at: None,
                metadata: json!({"example":true}),
            })
            .into_iter()
            .collect();
        to_json(&Batch {
            items,
            cursor: Some(json!({"page":1})),
            not_modified: false,
        })
    }
}

export!(Example);

