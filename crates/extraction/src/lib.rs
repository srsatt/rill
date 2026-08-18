use ammonia::Builder as Sanitizer;
use chrono::DateTime;
use rill_dedup::{canonicalize_url, content_checksum};
use rill_domain::NormalizedDocument;
use rill_source_api::{BoundedHttpClient, ConditionalHeaders, FetchError};
use scraper::{ElementRef, Html, Selector};
use thiserror::Error;
use url::Url;

#[derive(Debug, Error)]
pub enum ExtractionError {
    #[error("article fetch failed: {0}")]
    Fetch(#[from] FetchError),
    #[error("page is unsupported: {0}")]
    Unsupported(&'static str),
    #[error("article has no meaningful body")]
    Empty,
    #[error("canonical URL is invalid: {0}")]
    Canonical(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedImage {
    pub url: String,
    pub alt: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct ExtractedArticle {
    pub document: NormalizedDocument,
    pub images: Vec<ExtractedImage>,
    pub content_checksum: [u8; 32],
}

#[derive(Clone)]
pub struct ArticleExtractor {
    http: BoundedHttpClient,
}

impl ArticleExtractor {
    pub fn new(http: BoundedHttpClient) -> Self {
        Self { http }
    }

    pub async fn extract_url(
        &self,
        url: &Url,
        visibility_scope: &str,
    ) -> Result<ExtractedArticle, ExtractionError> {
        if let Some((channel, message_id)) = telegram_post_identity(url) {
            let mut preview = Url::parse(&format!("https://t.me/s/{channel}"))
                .map_err(|error| ExtractionError::Canonical(error.to_string()))?;
            preview
                .query_pairs_mut()
                .append_pair("before", &message_id.saturating_add(1).to_string());
            let response = self
                .http
                .get(&preview, &ConditionalHeaders::default())
                .await?;
            return extract_telegram_post(
                url,
                &String::from_utf8_lossy(&response.body),
                visibility_scope,
                &channel,
                message_id,
            );
        }
        let response = self.http.get(url, &ConditionalHeaders::default()).await?;
        if response.not_modified {
            return Err(ExtractionError::Empty);
        }
        if response.content_type.as_deref().is_some_and(|value| {
            !value.starts_with("text/html") && !value.starts_with("application/xhtml+xml")
        }) {
            return Err(ExtractionError::Unsupported("response is not HTML"));
        }
        let html = String::from_utf8_lossy(&response.body);
        extract_html(&response.final_url, &html, visibility_scope)
    }
}

fn telegram_post_identity(url: &Url) -> Option<(String, u64)> {
    if !matches!(url.host_str(), Some("t.me" | "telegram.me")) {
        return None;
    }
    let segments = url
        .path_segments()?
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    if segments.len() != 2 || matches!(segments[0], "s" | "joinchat" | "share") {
        return None;
    }
    let message_id = segments[1].parse().ok()?;
    Some((segments[0].to_owned(), message_id))
}

fn extract_telegram_post(
    canonical_url: &Url,
    html: &str,
    visibility_scope: &str,
    channel: &str,
    message_id: u64,
) -> Result<ExtractedArticle, ExtractionError> {
    let document = Html::parse_document(html);
    let expected = format!("{channel}/{message_id}");
    let post_selector = Selector::parse("[data-post]").expect("static selector");
    let post = document
        .select(&post_selector)
        .find(|post| post.value().attr("data-post") == Some(expected.as_str()))
        .ok_or(ExtractionError::Empty)?;
    let text_selector = Selector::parse(".tgme_widget_message_text").expect("static selector");
    let text = post
        .select(&text_selector)
        .next()
        .ok_or(ExtractionError::Empty)?;
    let body_text = normalize(&text.text().collect::<Vec<_>>().join(" "));
    if body_text.chars().count() < 40 {
        return Err(ExtractionError::Empty);
    }
    let title = telegram_title(&body_text);
    let sanitized_html = Sanitizer::default().clean(&text.inner_html()).to_string();
    let published_at = post
        .select(&Selector::parse("time[datetime]").expect("static selector"))
        .find_map(|time| time.value().attr("datetime"))
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|date| date.timestamp());
    let images = extract_images(&post, canonical_url);
    let checksum = content_checksum(&title, &body_text);
    Ok(ExtractedArticle {
        document: NormalizedDocument {
            visibility_scope: visibility_scope.to_owned(),
            title,
            body_text,
            sanitized_html: Some(sanitized_html),
            author: Some(format!("@{channel}")),
            publisher: Some(format!("t.me/{channel}")),
            canonical_url: Some(canonical_url.to_string()),
            language: None,
            published_at,
        },
        images,
        content_checksum: checksum,
    })
}

fn telegram_title(body_text: &str) -> String {
    let sentence = body_text
        .split(['.', '!', '?', '\n'])
        .map(str::trim)
        .find(|value| value.chars().count() >= 12)
        .unwrap_or(body_text);
    let mut characters = sentence.chars();
    let mut title = characters.by_ref().take(160).collect::<String>();
    if characters.next().is_some() {
        title.push('…');
    }
    title
}

pub fn extract_html(
    fetched_url: &Url,
    html: &str,
    visibility_scope: &str,
) -> Result<ExtractedArticle, ExtractionError> {
    let document = Html::parse_document(html);
    reject_obvious_gate(&document)?;
    let root = first(&document, &["article", "main", "[role=main]", "body"])
        .ok_or(ExtractionError::Empty)?;
    let content_selector =
        Selector::parse("h1, h2, h3, p, blockquote, pre, li, figure").expect("static selector");
    let blocks = root
        .select(&content_selector)
        .filter(|element| !is_chrome(element))
        .collect::<Vec<_>>();
    let body_text = if blocks.is_empty() {
        normalize(&root.text().collect::<Vec<_>>().join(" "))
    } else {
        blocks
            .iter()
            .map(|element| normalize(&element.text().collect::<Vec<_>>().join(" ")))
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n")
    };
    if body_text.chars().count() < 80 {
        return Err(ExtractionError::Empty);
    }
    let raw_main_html = blocks
        .iter()
        .map(ElementRef::html)
        .collect::<Vec<_>>()
        .join("\n");
    let sanitized_html = Sanitizer::default().clean(&raw_main_html).to_string();
    let title = metadata_content(&document, "meta[property='og:title']")
        .or_else(|| metadata_content(&document, "meta[name='twitter:title']"))
        .or_else(|| first_text(&document, &["article h1", "main h1", "h1", "title"]))
        .filter(|title| !title.is_empty())
        .ok_or(ExtractionError::Empty)?;
    let author = metadata_content(&document, "meta[name='author']")
        .or_else(|| metadata_content(&document, "meta[property='article:author']"))
        .or_else(|| first_text(&document, &["[rel=author]", ".byline", ".author"]));
    let published_at = metadata_content(&document, "meta[property='article:published_time']")
        .or_else(|| attribute(&document, "time[datetime]", "datetime"))
        .and_then(|value| DateTime::parse_from_rfc3339(&value).ok())
        .map(|date| date.timestamp());
    let canonical = attribute(&document, "link[rel=canonical]", "href")
        .and_then(|value| fetched_url.join(&value).ok())
        .unwrap_or_else(|| fetched_url.clone());
    let canonical_url = canonicalize_url(canonical.as_str())
        .map_err(|error| ExtractionError::Canonical(error.to_string()))?;
    let publisher = Url::parse(&canonical_url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned));
    let language = attribute(&document, "html[lang]", "lang").map(|value| {
        value
            .split(['-', '_'])
            .next()
            .unwrap_or(&value)
            .to_ascii_lowercase()
    });
    let images = extract_images(&root, fetched_url);
    let checksum = content_checksum(&title, &body_text);
    Ok(ExtractedArticle {
        document: NormalizedDocument {
            visibility_scope: visibility_scope.to_owned(),
            title,
            body_text,
            sanitized_html: Some(sanitized_html),
            author,
            publisher,
            canonical_url: Some(canonical_url),
            language,
            published_at,
        },
        images,
        content_checksum: checksum,
    })
}

fn reject_obvious_gate(document: &Html) -> Result<(), ExtractionError> {
    let title = first_text(document, &["title", "h1"])
        .unwrap_or_default()
        .to_ascii_lowercase();
    let text = normalize(
        &document
            .root_element()
            .text()
            .take(200)
            .collect::<Vec<_>>()
            .join(" "),
    )
    .to_ascii_lowercase();
    let combined = format!("{title} {text}");
    if [
        "verify you are human",
        "checking your browser",
        "enable javascript to continue",
        "access denied",
    ]
    .iter()
    .any(|marker| combined.contains(marker))
    {
        return Err(ExtractionError::Unsupported("challenge page"));
    }
    if [
        "sign in to continue",
        "log in to continue",
        "subscribe to continue",
    ]
    .iter()
    .any(|marker| combined.contains(marker))
    {
        return Err(ExtractionError::Unsupported("login or paywall page"));
    }
    Ok(())
}

fn is_chrome(element: &ElementRef<'_>) -> bool {
    let classes = element
        .value()
        .attr("class")
        .unwrap_or_default()
        .to_ascii_lowercase();
    let id = element
        .value()
        .attr("id")
        .unwrap_or_default()
        .to_ascii_lowercase();
    [
        "nav",
        "menu",
        "cookie",
        "consent",
        "footer",
        "subscribe",
        "advert",
        "social",
        "share",
    ]
    .iter()
    .any(|marker| classes.contains(marker) || id.contains(marker))
}

fn extract_images(root: &ElementRef<'_>, base: &Url) -> Vec<ExtractedImage> {
    let selector = Selector::parse("img[src]").expect("static selector");
    root.select(&selector)
        .filter_map(|image| {
            let url = base.join(image.value().attr("src")?).ok()?;
            if !matches!(url.scheme(), "http" | "https") {
                return None;
            }
            Some(ExtractedImage {
                url: url.to_string(),
                alt: image
                    .value()
                    .attr("alt")
                    .map(normalize)
                    .filter(|value| !value.is_empty()),
                width: image
                    .value()
                    .attr("width")
                    .and_then(|value| value.parse().ok()),
                height: image
                    .value()
                    .attr("height")
                    .and_then(|value| value.parse().ok()),
            })
        })
        .take(20)
        .collect()
}

fn first<'a>(document: &'a Html, selectors: &[&str]) -> Option<ElementRef<'a>> {
    selectors.iter().find_map(|selector| {
        Selector::parse(selector)
            .ok()
            .and_then(|selector| document.select(&selector).next())
    })
}

fn first_text(document: &Html, selectors: &[&str]) -> Option<String> {
    first(document, selectors)
        .map(|element| normalize(&element.text().collect::<Vec<_>>().join(" ")))
}

fn metadata_content(document: &Html, selector: &str) -> Option<String> {
    attribute(document, selector, "content").map(|value| normalize(&value))
}

fn attribute(document: &Html, selector: &str, attribute: &str) -> Option<String> {
    let selector = Selector::parse(selector).ok()?;
    document
        .select(&selector)
        .next()?
        .value()
        .attr(attribute)
        .map(str::to_owned)
}

fn normalize(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const ARTICLE: &str = r#"<!doctype html><html lang="en-US"><head>
      <title>Fallback title</title><meta property="og:title" content="SQLite gets faster">
      <meta name="author" content="Ada Reader"><meta property="article:published_time" content="2026-08-17T10:00:00Z">
      <link rel="canonical" href="/article/?utm_source=newsletter#part"></head><body>
      <nav><p>Home Products Pricing and many unrelated navigation words that should disappear.</p></nav>
      <article><h1>SQLite gets faster</h1><p>SQLite has a new query planner that reduces work for several common joins.</p>
      <p>This second paragraph contains enough useful article content to pass the bounded extractor threshold.</p>
      <img src="/image.jpg" alt="Query plan" width="640" height="480"><script>alert(1)</script></article>
      <div class="cookie-banner"><p>Accept all cookies and read our privacy policy.</p></div></body></html>"#;

    #[test]
    fn extracts_article_and_sanitizes_main_html() {
        let article = extract_html(
            &Url::parse("https://example.com/original").unwrap(),
            ARTICLE,
            "public",
        )
        .unwrap();
        assert_eq!(article.document.title, "SQLite gets faster");
        assert_eq!(article.document.author.as_deref(), Some("Ada Reader"));
        assert_eq!(
            article.document.canonical_url.as_deref(),
            Some("https://example.com/article")
        );
        assert!(!article.document.body_text.contains("Products Pricing"));
        assert!(
            !article
                .document
                .sanitized_html
                .as_deref()
                .unwrap()
                .contains("script")
        );
        assert_eq!(article.images[0].url, "https://example.com/image.jpg");
    }

    #[test]
    fn rejects_challenge_page() {
        let error = extract_html(
            &Url::parse("https://example.com").unwrap(),
            "<html><title>Just a moment</title><body><h1>Verify you are human</h1><p>Checking your browser before accessing the site.</p></body></html>",
            "public",
        ).unwrap_err();
        assert!(matches!(
            error,
            ExtractionError::Unsupported("challenge page")
        ));
    }

    #[test]
    fn extracts_telegram_post_content_instead_of_widget_script() {
        let html = r#"<html><body>
          <div class="tgme_widget_message" data-post="channel_name/42">
            <div class="tgme_widget_message_text">Rust gained a new compiler backend with measurable build-time improvements for large workspaces.</div>
            <time datetime="2026-08-17T10:00:00+00:00"></time>
          </div>
          <script>TelegramWebviewProxy.postEvent('widget_ready')</script>
        </body></html>"#;
        let article = extract_telegram_post(
            &Url::parse("https://t.me/channel_name/42").unwrap(),
            html,
            "public",
            "channel_name",
            42,
        )
        .unwrap();
        assert!(article.document.body_text.contains("compiler backend"));
        assert!(!article.document.body_text.contains("TelegramWebviewProxy"));
        assert_eq!(
            article.document.publisher.as_deref(),
            Some("t.me/channel_name")
        );
        assert_eq!(article.document.published_at, Some(1_786_960_800));
    }
}
