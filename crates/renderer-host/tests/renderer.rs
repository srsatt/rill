use std::{collections::BTreeMap, path::PathBuf};

use anyhow::{Context, Result};
use rill_contracts::{RENDER_PROTOCOL_VERSION, RenderMode, RenderRequest};
use rill_renderer_host::{Renderer, RendererLimits, WasiRenderer};
use serde_json::{Value, json};

fn renderer() -> Result<WasiRenderer> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../artifacts/ui-renderer.wasm");
    WasiRenderer::load(&path, RendererLimits::default())
        .with_context(|| format!("build renderer first: {}", path.display()))
}

fn request(template: &str, mode: RenderMode) -> RenderRequest {
    RenderRequest {
        version: RENDER_PROTOCOL_VERSION,
        template: template.to_owned(),
        mode,
        locale: "en".to_owned(),
        render_id: "test-".to_owned(),
        props: json!({
            "title": "Rill <News>",
            "activeStream": "home",
            "streams": [{ "name": "Home", "slug": "home" }],
            "stories": [{
                "id": "story-1",
                "title": "<script>alert(1)</script>",
                "summary": "Useful & concise",
                "source": "Example",
                "curator": "Curator",
                "tags": ["ai safety", "rust"],
                "publishedAt": "2026-08-17T00:00:00Z",
                "coverageCount": 2,
                "readingMinutes": 4
            }]
        }),
        assets: BTreeMap::new(),
        csrf_token: "csrf\"<unsafe>".to_owned(),
    }
}

#[test]
fn renders_modern_page_deterministically_and_escapes_content() -> Result<()> {
    let renderer = renderer()?;
    let request = request("modern-feed", RenderMode::Modern);
    let first = renderer.render(&request)?;
    let second = renderer.render(&request)?;

    assert_eq!(first, second);
    assert_eq!(first.status, 200);
    assert!(first.head_html.contains("Rill &lt;News&gt;"));
    assert!(first.body_html.contains("&lt;script>alert(1)&lt;/script>"));
    assert!(
        first
            .body_html
            .contains("href=\"/search?topic=ai%20safety\"")
    );
    assert!(!first.body_html.contains("How this stream works"));
    assert!(!first.body_html.contains("<script>"));
    assert_eq!(first.hydration_state, request.props);
    Ok(())
}

#[test]
fn reader_page_contains_safe_form_and_no_hydration_state() -> Result<()> {
    let mut request = request("reader-feed", RenderMode::Reader);
    request.props["stories"][0]["canonicalUrl"] = json!("https://t.me/genau/42");
    request.props["stories"][0]["source"] = json!("t.me");
    let response = renderer()?.render(&request)?;

    assert_eq!(response.status, 200);
    assert!(response.body_html.contains("method=\"post\""));
    assert!(response.body_html.contains("csrf&quot;&lt;unsafe>"));
    assert!(response.body_html.contains("@genau"));
    assert!(response.body_html.contains("#send"));
    assert!(!response.body_html.contains(">t.me</a>"));
    assert!(response.hydration_state.is_null());
    Ok(())
}

#[test]
fn unknown_template_is_normal_response() -> Result<()> {
    let response = renderer()?.render(&request("missing", RenderMode::Modern))?;

    assert_eq!(response.status, 404);
    assert!(response.body_html.contains("Unknown template"));
    Ok(())
}

#[test]
fn renders_empty_reader_settings_page() -> Result<()> {
    let mut request = request("modern-reader-settings", RenderMode::Modern);
    request.props = json!({
        "title": "Reader devices",
        "username": "alice",
        "devices": [],
        "newPairingCode": null,
        "pairingExpiresAt": null
    });

    let response = renderer()?.render(&request)?;
    assert_eq!(response.status, 200);
    assert!(response.body_html.contains("No paired readers"));
    assert!(response.body_html.contains("name=\"csrf_token\""));
    assert_eq!(response.hydration_state, request.props);
    Ok(())
}

#[test]
fn renders_reader_pair_form_without_javascript() -> Result<()> {
    let mut request = request("reader-pair", RenderMode::Reader);
    request.props = json!({ "title": "Pair this reader", "error": null });

    let response = renderer()?.render(&request)?;
    assert_eq!(response.status, 200);
    assert!(response.body_html.contains("action=\"/reader/pair\""));
    assert!(response.body_html.contains("csrf&quot;&lt;unsafe>"));
    assert!(response.hydration_state.is_null());
    Ok(())
}

fn story_props(reader: bool) -> Value {
    json!({
        "title": "Story <detail>",
        "storyId": "story-1",
        "representative": {
            "documentId": "document-1",
            "title": "Primary <story>",
            "summary": "Summary & context",
            "bodyText": "Body with </script><script>alert(1)</script>",
            "canonicalUrl": "https://example.test/story",
            "links": [
                { "url": "https://example.test/story", "relation": "alternate", "title": null },
                { "url": "https://forum.example/story", "relation": "replies", "title": "Thread" }
            ],
            "author": "Alice",
            "publisher": "Example",
            "language": "en",
            "publishedAt": "2026-08-17T00:00:00Z",
            "curators": [{
                "kind": "telegram",
                "curatorId": "curator-1",
                "sourceName": "Roundup",
                "curatorCommentary": "Worth reading",
                "parentTitle": "Morning links",
                "parentUrl": "https://example.test/roundup"
            }],
            "selected": true
        },
        "variants": [],
        "coverageCount": 1,
        "read": false,
        "favorite": false,
        "explicitFeedback": null,
        "reader": reader
    })
}

#[test]
fn renders_long_story_body_within_limits() -> Result<()> {
    let mut request = request("modern-story", RenderMode::Modern);
    request.props = story_props(false);
    request.props["representative"]["bodyText"] = Value::String("a".repeat(20_000));

    let response = renderer()?.render(&request)?;

    assert_eq!(response.status, 200);
    assert!(response.body_html.contains(&"a".repeat(20_000)));
    Ok(())
}

#[test]
fn omits_duplicate_story_summary() -> Result<()> {
    let mut request = request("modern-story", RenderMode::Modern);
    request.props = story_props(false);
    request.props["representative"]["summary"] = json!("Repeated article text");
    request.props["representative"]["bodyText"] = json!("Repeated article text");

    let response = renderer()?.render(&request)?;

    assert_eq!(
        response.body_html.matches("Repeated article text").count(),
        1
    );
    assert!(!response.body_html.contains("story-deck"));
    Ok(())
}

#[test]
fn omits_redundant_single_source_coverage() -> Result<()> {
    let mut request = request("modern-story", RenderMode::Modern);
    request.props = story_props(false);

    let response = renderer()?.render(&request)?;

    assert!(!response.body_html.contains("Coverage map"));
    assert!(!response.body_html.contains("Coverage (1"));
    Ok(())
}

#[test]
fn renders_original_and_discussion_links() -> Result<()> {
    let mut request = request("reader-story", RenderMode::Reader);
    request.props = story_props(true);
    let response = renderer()?.render(&request)?;
    assert!(response.body_html.contains("Open original</a>"));
    assert!(response.body_html.contains("Discussion</a>"));
    assert!(response.body_html.contains("https://forum.example/story"));
    Ok(())
}

#[test]
fn renders_every_page_template() -> Result<()> {
    let cases = [
        (
            "modern-feed",
            RenderMode::Modern,
            request("modern-feed", RenderMode::Modern).props,
            "Rill &lt;News>",
        ),
        (
            "modern-login",
            RenderMode::Modern,
            json!({ "title": "Sign in", "error": null }),
            "Sign in",
        ),
        (
            "modern-library",
            RenderMode::Modern,
            json!({ "title": "Favorites", "username": "alice", "kind": "favorites", "query": null, "stories": [] }),
            "No favorites yet",
        ),
        (
            "modern-sources",
            RenderMode::Modern,
            json!({ "title": "Sources", "username": "alice", "emailAvailable": true, "telegramAvailable": true }),
            "Configured sources",
        ),
        (
            "modern-story",
            RenderMode::Modern,
            story_props(false),
            "Primary &lt;story>",
        ),
        (
            "modern-admin",
            RenderMode::Modern,
            json!({ "title": "Administration", "username": "admin" }),
            "Source health",
        ),
        (
            "reader-feed",
            RenderMode::Reader,
            request("reader-feed", RenderMode::Reader).props,
            "Full Rill",
        ),
        (
            "reader-pair",
            RenderMode::Reader,
            json!({ "title": "Pair reader", "error": null }),
            "Pairing code",
        ),
        (
            "reader-story",
            RenderMode::Reader,
            story_props(true),
            "Story controls",
        ),
        (
            "reader-settings",
            RenderMode::Reader,
            json!({ "title": "Reader settings", "username": "alice", "streams": [{ "name": "Home", "slug": "home" }], "activeStream": "home" }),
            "Exit reader mode",
        ),
        (
            "modern-reader-settings",
            RenderMode::Modern,
            json!({ "title": "Reader devices", "username": "alice", "devices": [], "newPairingCode": null, "pairingExpiresAt": null }),
            "Change password",
        ),
    ];

    let renderer = renderer()?;
    for (template, mode, props, needle) in cases {
        let mut request = request(template, mode);
        request.props = props;
        let response = renderer
            .render(&request)
            .with_context(|| format!("render template {template}"))?;
        assert_eq!(response.status, 200, "template {template}");
        assert!(response.body_html.contains(needle), "template {template}");
        if template == "modern-library" {
            assert!(!response.body_html.contains("Library views"));
        }
        if mode == RenderMode::Reader {
            assert!(response.hydration_state.is_null(), "template {template}");
        }
    }
    Ok(())
}

#[test]
fn renders_large_feed_within_default_limits() -> Result<()> {
    let mut request = request("modern-feed", RenderMode::Modern);
    let story = request.props["stories"][0].clone();
    request.props["stories"] = Value::Array(
        (0..50)
            .map(|index| {
                let mut story = story.clone();
                story["id"] = json!(format!("story-{index}"));
                story
            })
            .collect(),
    );

    let response = renderer()?.render(&request)?;
    assert_eq!(response.status, 200);
    assert_eq!(
        response.body_html.matches("class=\"story-row\"").count(),
        50
    );
    assert_eq!(
        response.hydration_state["stories"].as_array().map(Vec::len),
        Some(50)
    );
    Ok(())
}

#[test]
fn renders_live_sized_initial_feed_within_default_limits() -> Result<()> {
    let mut request = request("modern-feed", RenderMode::Modern);
    let story = request.props["stories"][0].clone();
    request.props["stories"] = Value::Array(
        (0..5)
            .map(|index| {
                let mut story = story.clone();
                story["id"] = json!(format!("live-story-{index}"));
                story["title"] = json!(format!(
                    "Story {index}: {}",
                    "international source signal ".repeat(8)
                ));
                story["summary"] = json!(
                    "Detailed multilingual context: café, Berlin, 東京, Telegram, RSS. ".repeat(13)
                );
                story["tags"] = json!(
                    (0..8)
                        .map(|tag| format!("reader topic {index} {tag} with detail"))
                        .collect::<Vec<_>>()
                );
                story
            })
            .collect(),
    );

    let response = renderer()?.render(&request)?;
    assert_eq!(response.status, 200);
    assert_eq!(response.body_html.matches("class=\"story-row\"").count(), 5);
    Ok(())
}

#[test]
fn renders_full_reader_page_within_default_limits() -> Result<()> {
    let mut request = request("reader-feed", RenderMode::Reader);
    let story = request.props["stories"][0].clone();
    request.props["stories"] = Value::Array(
        (0..20)
            .map(|index| {
                let mut story = story.clone();
                story["id"] = json!(format!("reader-story-{index}"));
                story["title"] = json!(format!(
                    "Story {index}: {}",
                    "international source signal ".repeat(24)
                ));
                story["summary"] = json!(
                    "Detailed multilingual context: café, Berlin, 東京, Telegram, RSS. ".repeat(36)
                );
                story["tags"] = json!(
                    (0..8)
                        .map(|tag| format!("reader topic {index} {tag} with detail"))
                        .collect::<Vec<_>>()
                );
                story
            })
            .collect(),
    );

    let response = renderer()?.render(&request)?;
    assert_eq!(response.status, 200);
    assert_eq!(response.body_html.matches("<article").count(), 20);
    Ok(())
}
