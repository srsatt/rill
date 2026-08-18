use serde_json::Value;

use super::*;

const CASES: &[(&str, &str)] = &[
    (
        include_str!("../../../fixtures/telegram/text.html"),
        include_str!("../../../fixtures/telegram/text.json"),
    ),
    (
        include_str!("../../../fixtures/telegram/links.html"),
        include_str!("../../../fixtures/telegram/links.json"),
    ),
    (
        include_str!("../../../fixtures/telegram/media-grouped.html"),
        include_str!("../../../fixtures/telegram/media-grouped.json"),
    ),
    (
        include_str!("../../../fixtures/telegram/forwarded.html"),
        include_str!("../../../fixtures/telegram/forwarded.json"),
    ),
    (
        include_str!("../../../fixtures/telegram/malformed-empty.html"),
        include_str!("../../../fixtures/telegram/malformed-empty.json"),
    ),
];

#[test]
fn parser_matches_sanitized_golden_fixtures() {
    for (html, expected) in CASES {
        let actual = parse_channel_html(html.as_bytes(), "examplechannel")
            .unwrap()
            .into_iter()
            .map(|post| serde_json::to_value(post.item).unwrap())
            .collect::<Vec<_>>();
        let expected: Vec<Value> = serde_json::from_str(expected).unwrap();
        assert_eq!(actual, expected);
    }
}

#[test]
fn username_is_normalized_and_strictly_validated() {
    assert_eq!(
        normalize_username(" @Example_Channel ").unwrap(),
        "example_channel"
    );
    for invalid in ["four", "1channel", "bad-name", "", "@"] {
        assert!(normalize_username(invalid).is_err(), "accepted {invalid:?}");
    }
}

#[test]
fn selection_reemits_only_a_bounded_recent_overlap() {
    let posts = (1..=20)
        .map(|id| {
            (
                id,
                RawSourceItem {
                    external_id: format!("telegram:examplechannel:{id}"),
                    item_kind: "message".to_owned(),
                    title: None,
                    body_text: Some(id.to_string()),
                    body_html: None,
                    author: None,
                    source_url: None,
                    published_at: None,
                    edited_at: None,
                    deleted_at: None,
                    external_urls: Vec::new(),
                    media: Vec::new(),
                    metadata: Value::Null,
                },
            )
        })
        .collect();
    let (items, cursor) = select_items(posts, 15, 12);
    let ids = items
        .iter()
        .map(|item| item.external_id.rsplit(':').next().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        ids,
        [
            "9", "10", "11", "12", "13", "14", "15", "16", "17", "18", "19", "20"
        ]
    );
    assert_eq!(cursor, 20);
}

#[test]
fn selection_does_not_advance_past_a_limited_batch() {
    let posts = (10..=15)
        .map(|id| {
            let parsed = parse_channel_html(
                format!(
                    r#"<div data-post="examplechannel/{id}"><div class="tgme_widget_message_text">{id}</div></div>"#
                )
                .as_bytes(),
                "examplechannel",
            )
            .unwrap()
            .pop()
            .unwrap();
            (id, parsed.item)
        })
        .collect();
    let (items, cursor) = select_items(posts, 9, 3);
    assert_eq!(items.len(), 3);
    assert_eq!(cursor, 12);
}

#[test]
fn empty_preview_distinguishes_valid_channel_from_missing_or_broken_markup() {
    assert!(validate_empty_preview(b"<div class=\"tgme_channel_info\"></div>", "genau").is_ok());
    assert!(validate_empty_preview(b"<html>contact Telegram</html>", "genau").is_err());
    assert!(
        validate_empty_preview(
            b"<div data-post=\"genau/42\"><div>changed markup</div></div>",
            "genau",
        )
        .is_err()
    );
}
