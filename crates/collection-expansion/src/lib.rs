use regex::Regex;
use rill_dedup::canonicalize_url;
use rill_domain::{CollectionEntryCandidate, ItemShape, RawSourceItem};
use rill_model_api::{CollectionParseRequest, CollectionParseResponse, ModelError};
use scraper::{ElementRef, Html, Selector};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, sync::OnceLock};
use url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectionMode {
    Auto,
    ForceSingle,
    ForceCollection,
}

impl DetectionMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::ForceSingle => "force_single",
            Self::ForceCollection => "force_collection",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ParentDisplayPolicy {
    ChildrenOnly,
    ParentAndChildren,
    ParentOnly,
}

impl ParentDisplayPolicy {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ChildrenOnly => "children_only",
            Self::ParentAndChildren => "parent_and_children",
            Self::ParentOnly => "parent_only",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IgnoredLink {
    pub url: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionDetection {
    pub shape: ItemShape,
    pub ignored_links: Vec<IgnoredLink>,
    pub mode: String,
}

#[derive(Debug, Clone)]
pub struct CollectionPolicy {
    pub threshold: f32,
    pub maximum_fan_out: usize,
    pub parent_display_policy: ParentDisplayPolicy,
    pub excluded_hosts: Vec<String>,
    pub excluded_path_fragments: Vec<String>,
}

impl Default for CollectionPolicy {
    fn default() -> Self {
        Self {
            threshold: 0.65,
            maximum_fan_out: 25,
            parent_display_policy: ParentDisplayPolicy::ChildrenOnly,
            excluded_hosts: Vec::new(),
            excluded_path_fragments: Vec::new(),
        }
    }
}

pub fn detect_collection(
    item: &RawSourceItem,
    base_url: Option<&Url>,
    mode: DetectionMode,
    policy: &CollectionPolicy,
) -> ItemShape {
    detect_collection_with_diagnostics(item, base_url, mode, policy).shape
}

pub fn detect_collection_with_diagnostics(
    item: &RawSourceItem,
    base_url: Option<&Url>,
    mode: DetectionMode,
    policy: &CollectionPolicy,
) -> CollectionDetection {
    if mode == DetectionMode::ForceSingle {
        return CollectionDetection {
            shape: ItemShape::Single,
            ignored_links: collect_ignored_links(item, base_url, policy, &BTreeSet::new()),
            mode: mode.as_str().to_owned(),
        };
    }
    let mut candidates = Vec::new();
    let mut structured = false;
    if let Some(html) = &item.body_html {
        let parsed = parse_html_candidates(html, base_url, policy);
        structured = parsed.1;
        candidates.extend(parsed.0);
    }
    if let Some(text) = &item.body_text {
        let parsed = parse_text_candidates(text, base_url, policy);
        structured |= parsed.1;
        candidates.extend(parsed.0);
    }
    let telegram = parse_telegram_entity_candidates(item, policy);
    structured |= telegram.1;
    candidates.extend(telegram.0);
    let mut seen = BTreeSet::new();
    candidates.retain(|candidate| seen.insert(candidate.url.clone()));
    candidates.truncate(policy.maximum_fan_out);
    for (ordinal, candidate) in candidates.iter_mut().enumerate() {
        candidate.ordinal = ordinal;
    }
    let hint_ratio = if candidates.is_empty() {
        0.0
    } else {
        candidates
            .iter()
            .filter(|candidate| candidate.title_hint.is_some())
            .count() as f32
            / candidates.len() as f32
    };
    let confidence = (0.15
        + (candidates.len().saturating_sub(1).min(4) as f32 * 0.1)
        + if structured { 0.25 } else { 0.0 }
        + hint_ratio * 0.2)
        .min(0.99);
    let shape = if mode == DetectionMode::ForceCollection
        || (candidates.len() >= 3 && confidence >= policy.threshold)
    {
        for candidate in &mut candidates {
            candidate.confidence = confidence;
        }
        ItemShape::Collection {
            confidence,
            entries: candidates,
        }
    } else {
        ItemShape::Single
    };
    let selected = match &shape {
        ItemShape::Collection { entries, .. } => entries
            .iter()
            .map(|entry| entry.url.clone())
            .collect::<BTreeSet<_>>(),
        ItemShape::Single => BTreeSet::new(),
    };
    CollectionDetection {
        shape,
        ignored_links: collect_ignored_links(item, base_url, policy, &selected),
        mode: mode.as_str().to_owned(),
    }
}

include!("provider_request.rs");
fn parse_html_candidates(
    html: &str,
    base_url: Option<&Url>,
    policy: &CollectionPolicy,
) -> (Vec<CollectionEntryCandidate>, bool) {
    let document = Html::parse_fragment(html);
    let link_selector = Selector::parse("a[href]").expect("static selector");
    let container_selector = Selector::parse("li, article, section, tr").expect("static selector");
    let heading_selector = Selector::parse("h1, h2, h3, h4, strong, b").expect("static selector");
    let structured = document.select(&container_selector).count() >= 2;
    let mut candidates = Vec::new();
    for link in document.select(&link_selector) {
        let Some(href) = link.value().attr("href") else {
            continue;
        };
        let Some(url) = resolve_url(href, base_url) else {
            continue;
        };
        if !meaningful_link(&url, &link.text().collect::<Vec<_>>().join(" "), policy) {
            continue;
        }
        let container = nearest_container(link);
        let anchor = normalize(&link.text().collect::<Vec<_>>().join(" "));
        let inline_commentary = container.is_none().then(|| preceding_line(link)).flatten();
        let nearby_heading = container.as_ref().and_then(|container| {
            container
                .select(&heading_selector)
                .next()
                .map(|heading| normalize(&heading.text().collect::<Vec<_>>().join(" ")))
                .filter(|title| !title.is_empty())
        });
        let title_hint = if inline_commentary.as_deref().is_some_and(anchor_is_title) {
            inline_commentary
                .as_deref()
                .map(|commentary| truncate(commentary, 180))
        } else if anchor_is_title(&anchor) {
            Some(anchor.clone())
        } else {
            nearby_heading
        };
        let commentary = container
            .map(|container| normalize(&container.text().collect::<Vec<_>>().join(" ")))
            .filter(|text| !text.is_empty())
            .map(|text| truncate(&text, 500))
            .or_else(|| inline_commentary.map(|text| truncate(&text, 500)));
        candidates.push(CollectionEntryCandidate {
            url: url.to_string(),
            title_hint,
            commentary,
            author_hint: None,
            published_at_hint: None,
            ordinal: candidates.len(),
            confidence: 0.0,
        });
    }
    (candidates, structured)
}

fn preceding_line(link: ElementRef<'_>) -> Option<String> {
    let mut pieces = Vec::new();
    let mut sibling = link.prev_sibling();
    let mut length = 0;
    while let Some(node) = sibling {
        if ElementRef::wrap(node).is_some_and(|element| element.value().name() == "br") {
            break;
        }
        let piece = ElementRef::wrap(node)
            .map(|element| normalize(&element.text().collect::<Vec<_>>().join(" ")))
            .or_else(|| node.value().as_text().map(|text| normalize(text)))
            .unwrap_or_default();
        if !piece.is_empty() {
            length += piece.chars().count();
            pieces.push(piece);
        }
        if length >= 500 {
            break;
        }
        sibling = node.prev_sibling();
    }
    pieces.reverse();
    let commentary = normalize(&pieces.join(" "));
    let commentary = commentary.trim_end_matches(|character: char| {
        character.is_whitespace()
            || matches!(character, '(' | '[' | '{' | '-' | '–' | '—' | ':' | '|')
    });
    (!commentary.is_empty()).then(|| truncate(commentary, 500))
}

fn nearest_container(link: ElementRef<'_>) -> Option<ElementRef<'_>> {
    link.ancestors()
        .filter_map(ElementRef::wrap)
        .find(|element| {
            matches!(
                element.value().name(),
                "li" | "article" | "section" | "tr" | "div"
            )
        })
}

fn parse_text_candidates(
    text: &str,
    base_url: Option<&Url>,
    policy: &CollectionPolicy,
) -> (Vec<CollectionEntryCandidate>, bool) {
    let expression = url_regex();
    let lines = text.lines().collect::<Vec<_>>();
    let structured = lines
        .iter()
        .filter(|line| {
            let trimmed = line.trim_start();
            trimmed.starts_with("• ")
                || trimmed.starts_with("- ")
                || trimmed
                    .chars()
                    .next()
                    .is_some_and(|character| character.is_ascii_digit())
                    && trimmed.contains(". ")
        })
        .count()
        >= 2;
    let mut candidates = Vec::new();
    for (line_index, line) in lines.iter().enumerate() {
        for found in expression.find_iter(line) {
            let raw = found
                .as_str()
                .trim_end_matches(['.', ',', ';', ':', '!', '?', ')', ']']);
            let Some(url) = resolve_url(raw, base_url) else {
                continue;
            };
            let previous = lines[..line_index]
                .iter()
                .rev()
                .map(|line| normalize(line))
                .find(|line| !line.is_empty() && !url_regex().is_match(line));
            if !meaningful_link(&url, previous.as_deref().unwrap_or(""), policy) {
                continue;
            }
            candidates.push(CollectionEntryCandidate {
                url: url.to_string(),
                title_hint: previous
                    .as_deref()
                    .filter(|value| anchor_is_title(value))
                    .map(strip_list_marker),
                commentary: previous.map(|value| truncate(&value, 500)),
                author_hint: None,
                published_at_hint: None,
                ordinal: candidates.len(),
                confidence: 0.0,
            });
        }
    }
    (candidates, structured)
}

fn resolve_url(value: &str, base_url: Option<&Url>) -> Option<Url> {
    let parsed = Url::parse(value)
        .or_else(|_| {
            base_url
                .ok_or(url::ParseError::RelativeUrlWithoutBase)?
                .join(value)
        })
        .ok()?;
    let canonical = canonicalize_url(parsed.as_str()).ok()?;
    Url::parse(&canonical).ok()
}

fn meaningful_link(url: &Url, label: &str, policy: &CollectionPolicy) -> bool {
    rejection_reason(url, label, policy).is_none()
}

fn rejection_reason(url: &Url, label: &str, policy: &CollectionPolicy) -> Option<&'static str> {
    if !matches!(url.scheme(), "http" | "https") {
        return Some("unsupported URL scheme");
    }
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    if policy.excluded_hosts.iter().any(|blocked| {
        let blocked = blocked.to_ascii_lowercase();
        host == blocked || host.ends_with(&format!(".{blocked}"))
    }) {
        return Some("excluded hostname");
    }
    if [
        "facebook.com",
        "instagram.com",
        "linkedin.com",
        "twitter.com",
        "x.com",
    ]
    .iter()
    .any(|blocked| host == *blocked || host.ends_with(&format!(".{blocked}")))
        || ((host == "t.me" || host == "telegram.me") && !is_telegram_post_url(url))
    {
        return Some("social profile");
    }
    let haystack = format!(
        "{} {}",
        url.path().to_ascii_lowercase(),
        label.to_ascii_lowercase()
    );
    if [
        "unsubscribe",
        "privacy",
        "terms",
        "signup",
        "sign-up",
        "login",
        "advert",
        "share",
        "newsletter-preferences",
    ]
    .iter()
    .any(|blocked| haystack.contains(blocked))
    {
        return Some("non-content link");
    }
    if policy.excluded_path_fragments.iter().any(|blocked| {
        url.path()
            .to_ascii_lowercase()
            .contains(&blocked.to_ascii_lowercase())
    }) {
        return Some("excluded path");
    }
    if [".jpg", ".jpeg", ".png", ".gif", ".webp", ".svg", ".ico"]
        .iter()
        .any(|extension| url.path().to_ascii_lowercase().ends_with(extension))
    {
        return Some("image asset");
    }
    None
}

fn is_telegram_post_url(url: &Url) -> bool {
    let segments = url
        .path_segments()
        .map(|segments| {
            segments
                .filter(|segment| !segment.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    segments.len() == 2
        && !matches!(segments[0], "s" | "joinchat" | "share" | "addstickers")
        && segments[1].bytes().all(|byte| byte.is_ascii_digit())
}

fn collect_ignored_links(
    item: &RawSourceItem,
    base_url: Option<&Url>,
    policy: &CollectionPolicy,
    selected: &BTreeSet<String>,
) -> Vec<IgnoredLink> {
    let mut raw = Vec::<(String, String)>::new();
    if let Some(text) = &item.body_text {
        raw.extend(
            url_regex()
                .find_iter(text)
                .map(|found| (found.as_str().to_owned(), String::new())),
        );
    }
    if let Some(html) = &item.body_html {
        let document = Html::parse_fragment(html);
        let selector = Selector::parse("a[href]").expect("static selector");
        raw.extend(document.select(&selector).filter_map(|link| {
            Some((
                link.value().attr("href")?.to_owned(),
                normalize(&link.text().collect::<Vec<_>>().join(" ")),
            ))
        }));
    }
    if let Some(entities) = item
        .metadata
        .get("entities")
        .and_then(|value| value.as_array())
    {
        raw.extend(entities.iter().filter_map(|entity| {
            Some((
                entity.get("url")?.as_str()?.to_owned(),
                entity
                    .get("label")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default()
                    .to_owned(),
            ))
        }));
    }
    let mut seen = BTreeSet::new();
    raw.into_iter()
        .filter_map(|(raw_url, label)| {
            let url = resolve_url(&raw_url, base_url)?;
            let canonical = url.to_string();
            if !seen.insert(canonical.clone()) || selected.contains(&canonical) {
                return None;
            }
            Some(IgnoredLink {
                url: canonical,
                reason: rejection_reason(&url, &label, policy)
                    .unwrap_or("insufficient collection evidence")
                    .to_owned(),
            })
        })
        .collect()
}

pub fn validate_provider_response(
    request: &CollectionParseRequest,
    response: CollectionParseResponse,
    maximum_fan_out: usize,
) -> Result<CollectionParseResponse, ModelError> {
    if !response.confidence.is_finite() || !(0.0..=1.0).contains(&response.confidence) {
        return Err(ModelError::InvalidOutput(
            "collection confidence is outside 0..=1".into(),
        ));
    }
    let allowed = request
        .allowed_urls
        .iter()
        .filter_map(|url| canonicalize_url(url).ok())
        .collect::<BTreeSet<_>>();
    let mut seen = BTreeSet::new();
    for entry in &response.entries {
        let canonical = canonicalize_url(&entry.url)
            .map_err(|_| ModelError::InvalidOutput("provider returned an invalid URL".into()))?;
        if !allowed.contains(&canonical) {
            return Err(ModelError::InvalidOutput(
                "provider invented a URL absent from the source item".into(),
            ));
        }
        if !entry.confidence.is_finite() || !(0.0..=1.0).contains(&entry.confidence) {
            return Err(ModelError::InvalidOutput(
                "entry confidence is outside 0..=1".into(),
            ));
        }
        seen.insert(canonical);
    }
    if response.entries.len() > maximum_fan_out || seen.len() != response.entries.len() {
        return Err(ModelError::InvalidOutput(
            "provider returned too many or duplicate entries".into(),
        ));
    }
    Ok(response)
}

fn anchor_is_title(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    value.len() >= 4
        && value.len() <= 180
        && ![
            "read",
            "read more",
            "read article",
            "continue reading",
            "view article",
            "click here",
            "source",
            "link",
            "open",
        ]
        .contains(&lower.as_str())
}

fn strip_list_marker(value: &str) -> String {
    let value = value.trim_start_matches(|character: char| {
        character.is_ascii_digit() || matches!(character, '.' | ')' | '-' | '•' | ' ')
    });
    value.to_owned()
}

fn normalize(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}
fn truncate(value: &str, maximum: usize) -> String {
    value.chars().take(maximum).collect()
}

fn url_regex() -> &'static Regex {
    static EXPRESSION: OnceLock<Regex> = OnceLock::new();
    EXPRESSION.get_or_init(|| Regex::new(r#"https?://[^\s<>\"']+"#).expect("static URL expression"))
}

include!("tests.rs");
