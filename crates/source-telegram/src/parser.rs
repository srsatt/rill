use std::collections::HashSet;

use chrono::DateTime;
use rill_domain::{ExternalLink, LinkRelation, RawMedia, RawSourceItem};
use rill_source_api::ConnectorError;
use scraper::{ElementRef, Html, Selector};
use serde_json::json;
use url::Url;

#[derive(Debug, Clone)]
pub struct ParsedPost {
    pub message_id: u64,
    pub item: RawSourceItem,
}

pub fn parse_channel_html(
    bytes: &[u8],
    expected_username: &str,
) -> Result<Vec<ParsedPost>, ConnectorError> {
    let html = std::str::from_utf8(bytes)
        .map_err(|error| ConnectorError::Parse(format!("Telegram HTML is not UTF-8: {error}")))?;
    let document = Html::parse_document(html);
    let message_selector = selector("[data-post]");
    Ok(document
        .select(&message_selector)
        .filter_map(|element| parse_post(element, expected_username))
        .collect())
}

fn parse_post(element: ElementRef<'_>, expected_username: &str) -> Option<ParsedPost> {
    let (post_username, message_id) = parse_post_id(element.value().attr("data-post")?)?;
    if !post_username.eq_ignore_ascii_case(expected_username) {
        return None;
    }

    let text_element = element
        .select(&selector(".tgme_widget_message_text"))
        .next();
    let body_html = text_element
        .as_ref()
        .map(ElementRef::inner_html)
        .map(|html| ammonia::clean(&html))
        .filter(|html| !html.trim().is_empty());
    let body_text = body_html
        .as_deref()
        .map(Html::parse_fragment)
        .map(|html| normalize_whitespace(&html.root_element().text().collect::<Vec<_>>().join(" ")))
        .filter(|text| !text.is_empty());
    let external_urls = text_element
        .as_ref()
        .map(extract_links)
        .unwrap_or_default()
        .into_iter()
        .take(16)
        .enumerate()
        .map(|(ordinal, url)| ExternalLink {
            url,
            relation: LinkRelation::new("related").expect("static relation"),
            title: None,
            ordinal: u32::try_from(ordinal).unwrap_or(u32::MAX),
        })
        .collect();
    let mut media = extract_media(element);
    if body_text.is_none() && media.is_empty() {
        return None;
    }

    let group_id = (media.len() > 1).then(|| format!("{expected_username}:{message_id}"));
    for entry in &mut media {
        entry.group_id.clone_from(&group_id);
    }
    let canonical_url = format!("https://t.me/{expected_username}/{message_id}");
    let published_at = element
        .select(&selector("time[datetime]"))
        .find_map(|time| time.value().attr("datetime"))
        .and_then(parse_timestamp);
    let forwarded_from = element
        .select(&selector(".tgme_widget_message_forwarded_from"))
        .next()
        .map(|forwarded| normalize_whitespace(&forwarded.text().collect::<Vec<_>>().join(" ")))
        .filter(|value| !value.is_empty());

    Some(ParsedPost {
        message_id,
        item: RawSourceItem {
            external_id: format!("telegram:{expected_username}:{message_id}"),
            item_kind: "message".to_owned(),
            title: None,
            body_text,
            body_html,
            author: Some(format!("@{expected_username}")),
            source_url: Some(canonical_url),
            published_at,
            edited_at: None,
            deleted_at: None,
            external_urls,
            media,
            metadata: json!({
                "telegram": {
                    "channel": expected_username,
                    "messageId": message_id,
                    "forwardedFrom": forwarded_from
                }
            }),
        },
    })
}

fn parse_post_id(value: &str) -> Option<(&str, u64)> {
    let (username, id) = value.rsplit_once('/')?;
    Some((username, id.parse().ok()?))
}

fn extract_links(element: &ElementRef<'_>) -> Vec<String> {
    let mut seen = HashSet::new();
    element
        .select(&selector("a[href]"))
        .filter_map(|link| link.value().attr("href"))
        .filter_map(public_url)
        .filter(|url| seen.insert(url.clone()))
        .collect()
}

fn extract_media(element: ElementRef<'_>) -> Vec<RawMedia> {
    let mut media = Vec::new();
    let mut seen = HashSet::new();

    for photo in element.select(&selector("a.tgme_widget_message_photo_wrap")) {
        let url = photo
            .value()
            .attr("style")
            .and_then(css_url)
            .and_then(public_url)
            .or_else(|| photo.value().attr("href").and_then(public_url));
        push_media(&mut media, &mut seen, "image", url);
    }
    for video in element.select(&selector("video[src], video source[src]")) {
        let url = video.value().attr("src").and_then(public_url);
        push_media(&mut media, &mut seen, "video", url);
    }
    for document in element.select(&selector("a.tgme_widget_message_document_wrap[href]")) {
        let url = document.value().attr("href").and_then(public_url);
        push_media(&mut media, &mut seen, "file", url);
    }
    media
}

fn push_media(
    media: &mut Vec<RawMedia>,
    seen: &mut HashSet<String>,
    kind: &str,
    url: Option<String>,
) {
    if url.as_ref().is_some_and(|url| !seen.insert(url.clone())) {
        return;
    }
    if url.is_some() {
        media.push(RawMedia {
            kind: kind.to_owned(),
            url,
            mime_type: None,
            size_bytes: None,
            width: None,
            height: None,
            group_id: None,
        });
    }
}

fn public_url(value: &str) -> Option<String> {
    let base = Url::parse("https://t.me").ok()?;
    let url = if value.starts_with("//") {
        Url::parse(&format!("https:{value}")).ok()?
    } else {
        base.join(value).ok()?
    };
    (matches!(url.scheme(), "http" | "https")
        && url.username().is_empty()
        && url.password().is_none())
    .then(|| url.to_string())
}

fn css_url(style: &str) -> Option<&str> {
    let start = style.find("url(")? + 4;
    let rest = &style[start..];
    let end = rest.find(')')?;
    Some(rest[..end].trim().trim_matches(['\'', '"']))
}

fn parse_timestamp(value: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|date| date.timestamp())
}

fn normalize_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn selector(value: &str) -> Selector {
    Selector::parse(value).expect("static Telegram selector must be valid")
}
