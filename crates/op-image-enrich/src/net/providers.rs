//! Browser image-search route for the web daemon.
//!
//! `POST /api/ai/image/search` mirrors the desktop image panel's Search
//! popover backend (`op-host-desktop/src/image_panel_host.rs`: Openverse →
//! two-keyword retry → Wikimedia, thumbnails embedded as `data:` URLs) so
//! the wasm shell can drain its `search_epoch` through the daemon instead
//! of leaving the popover loading forever. Openverse credentials come from
//! the request body (browser-held) or fall back to the daemon's persisted
//! agent settings. Openverse / Wikimedia are product-constant public hosts
//! — the same operator-trust tier as the desktop path — so they dial with
//! a plain client; nothing in this route dials a browser-supplied URL.
//!
//! Unlike the desktop, fetched thumbnails are NOT re-encoded/down-scaled
//! here: `image_downscale` needs skia and this crate must stay GL-free for
//! `op-host-web-server`. The 4 MiB per-image cap still bounds what can be
//! embedded.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use reqwest::header::CONTENT_TYPE;

/// Cap on concurrently running image jobs (search + generate combined).
/// Each job blocks one connection thread for up to minutes of provider
/// network; without a ceiling a page could exhaust the daemon's threads.
const MAX_IN_FLIGHT_IMAGE_JOBS: usize = 4;

static IN_FLIGHT_IMAGE_JOBS: AtomicUsize = AtomicUsize::new(0);

/// RAII slot for one running image job. `acquire` fails once
/// [`MAX_IN_FLIGHT_IMAGE_JOBS`] jobs are running (route answers 429).
pub(crate) struct ImageJobSlot(());

impl ImageJobSlot {
    pub(crate) fn acquire() -> Option<Self> {
        IN_FLIGHT_IMAGE_JOBS
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < MAX_IN_FLIGHT_IMAGE_JOBS).then_some(n + 1)
            })
            .ok()
            .map(|_| Self(()))
    }
}

impl Drop for ImageJobSlot {
    fn drop(&mut self) {
        IN_FLIGHT_IMAGE_JOBS.fetch_sub(1, Ordering::AcqRel);
    }
}

/// TS popover requests `count: 5` (desktop parity).
const SEARCH_RESULT_COUNT: usize = 5;
const MAX_EMBEDDED_IMAGE_BYTES: usize = 4 * 1024 * 1024;

/// Design-artifact words that are pure noise against a photo corpus (see
/// the desktop `image_search_session.rs` for the measurement notes).
const IMAGE_SEARCH_ARTIFACT_WORDS: &[&str] = &[
    "album",
    "cover",
    "playlist",
    "artwork",
    "poster",
    "thumbnail",
    "logo",
    "icon",
    "banner",
    "mockup",
    "screenshot",
    "wallpaper",
];

const IMAGE_SEARCH_STOP_WORDS: &[&str] = &[
    "a",
    "an",
    "the",
    "and",
    "or",
    "but",
    "in",
    "on",
    "at",
    "to",
    "for",
    "of",
    "with",
    "by",
    "from",
    "is",
    "are",
    "was",
    "were",
    "be",
    "been",
    "being",
    "have",
    "has",
    "had",
    "do",
    "does",
    "did",
    "will",
    "would",
    "could",
    "should",
    "may",
    "might",
    "shall",
    "can",
    "that",
    "this",
    "these",
    "those",
    "it",
    "its",
    "very",
    "really",
    "just",
    "also",
    "about",
    "above",
    "after",
    "before",
    "between",
    "into",
    "through",
    "during",
    "each",
    "some",
    "such",
    "no",
    "not",
    "only",
    "same",
    "so",
    "than",
    "too",
    "up",
    "out",
    "if",
    "then",
    "once",
    "here",
    "there",
    "when",
    "where",
    "how",
    "all",
    "both",
    "few",
    "more",
    "most",
    "other",
    "any",
    "as",
    "while",
    "using",
    "showing",
    "featuring",
    "looking",
    "style",
    "styled",
    "inspired",
    "based",
];

#[derive(Clone, PartialEq, Eq)]
pub struct WebOpenverseCredentials {
    pub client_id: String,
    pub client_secret: String,
}

impl WebOpenverseCredentials {
    /// `None` unless both parts are non-empty after trimming.
    pub fn from_parts(client_id: &str, client_secret: &str) -> Option<Self> {
        let client_id = client_id.trim();
        let client_secret = client_secret.trim();
        if client_id.is_empty() || client_secret.is_empty() {
            None
        } else {
            Some(Self {
                client_id: client_id.to_string(),
                client_secret: client_secret.to_string(),
            })
        }
    }
}

/// One search hit ready for the JSON reply / the desktop popover.
pub struct WebImageSearchHit {
    pub id: String,
    pub thumb_data_url: String,
    pub attribution: String,
}

pub struct WebImageSearchOutcome {
    pub results: Vec<WebImageSearchHit>,
    /// `"openverse"` / `"wikimedia"`, `None` when nothing landed.
    pub source: Option<&'static str>,
}

/// Why a `POST /api/ai/image/search` body was refused. Both variants answer
/// HTTP 400; the enum exists so the route reports WHICH client mistake was
/// made instead of matching on prose, and `Display` reproduces the exact
/// sentence the JSON reply already carried.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SearchRequestError {
    /// The body is not JSON, or is JSON but not an object.
    InvalidBody,
    /// The body is a valid object but carries no non-blank `query`.
    MissingQuery,
}

impl std::fmt::Display for SearchRequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SearchRequestError::InvalidBody => f.write_str("invalid request body"),
            SearchRequestError::MissingQuery => f.write_str("missing query"),
        }
    }
}

impl std::error::Error for SearchRequestError {}

/// Parse the request body and snapshot the daemon-side credential fallback.
/// Returns `(query, credentials)` or the reason for the 400 reply.
pub(crate) fn parse_search_request(
    body: &str,
    state: &op_editor_core::EditorState,
) -> Result<(String, Option<WebOpenverseCredentials>), SearchRequestError> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|_| SearchRequestError::InvalidBody)?;
    let obj = value.as_object().ok_or(SearchRequestError::InvalidBody)?;
    let query = obj
        .get("query")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|q| !q.is_empty())
        .ok_or(SearchRequestError::MissingQuery)?;
    // Browser-held credential wins; the daemon's persisted settings are the
    // fallback (both are optional — anonymous Openverse works, rate-limited).
    let request_credentials = obj
        .get("openverse")
        .and_then(serde_json::Value::as_object)
        .and_then(|cred| {
            WebOpenverseCredentials::from_parts(
                cred.get("client_id")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or(""),
                cred.get("client_secret")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or(""),
            )
        });
    let credentials = request_credentials.or_else(|| {
        let settings = &state.editor_ui.agent_settings;
        WebOpenverseCredentials::from_parts(
            &settings.openverse_client_id,
            &settings.openverse_client_secret,
        )
    });
    Ok((query.to_string(), credentials))
}

/// JSON reply body for a finished search.
pub(crate) fn search_outcome_to_json(outcome: &WebImageSearchOutcome) -> String {
    let results: Vec<serde_json::Value> = outcome
        .results
        .iter()
        .map(|hit| {
            serde_json::json!({
                "id": hit.id,
                "thumb_data_url": hit.thumb_data_url,
                "attribution": hit.attribution,
            })
        })
        .collect();
    serde_json::json!({
        "ok": true,
        "results": results,
        "source": outcome.source,
    })
    .to_string()
}

/// Run the full search ladder on the calling thread (the connection's own
/// thread — the caller must NOT hold the state lock).
pub(crate) fn run_search_blocking(
    query: &str,
    credentials: Option<&WebOpenverseCredentials>,
) -> WebImageSearchOutcome {
    // A private runtime here would panic the moment this sync helper is
    // reached from a tokio worker; `block_on_anywhere` runs the ladder on the
    // shared (enable_all) runtime instead — same IO/timer drivers, no
    // runtime-in-runtime hazard.
    crate::chat_runtime::block_on_anywhere(run_search(query, credentials))
}

async fn run_search(
    query: &str,
    credentials: Option<&WebOpenverseCredentials>,
) -> WebImageSearchOutcome {
    let Ok(client) = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .user_agent(concat!("openpencil-web-daemon/", env!("CARGO_PKG_VERSION")))
        .build()
    else {
        return WebImageSearchOutcome {
            results: Vec::new(),
            source: None,
        };
    };
    run_search_with_fetcher(&client, query, credentials, |url: String| {
        let client = client.clone();
        async move { fetch_image_data_url(&client, &url).await }
    })
    .await
}

/// The full search ladder over a caller-supplied client + thumbnail
/// materializer. Shared by this daemon route (plain embed) and the desktop
/// popover (its own user-agent + skia down-scale pass on each thumbnail).
///
/// `fetch_data_url` downloads one thumbnail URL into a `data:` URL; hits
/// whose thumbnails fail to download are dropped.
pub async fn run_search_with_fetcher<F, Fut>(
    client: &reqwest::Client,
    query: &str,
    credentials: Option<&WebOpenverseCredentials>,
    fetch_data_url: F,
) -> WebImageSearchOutcome
where
    F: Fn(String) -> Fut,
    Fut: std::future::Future<Output = Option<String>>,
{
    // Simplify verbose prompts into keywords (TS simplifySearchQuery).
    let query = simplify_search_query(query);

    // Openverse first; a zero-result answer retries with the first two
    // keywords before falling through to Wikimedia.
    let mut hits = fetch_openverse_list(client, &query, credentials).await;
    if hits.as_ref().is_some_and(Vec::is_empty) {
        if let Some(truncated) = two_keyword_retry(&query) {
            if let Some(retry) = fetch_openverse_list(client, &truncated, credentials).await {
                if !retry.is_empty() {
                    hits = Some(retry);
                }
            }
        }
    }
    if let Some(urls) = hits.filter(|h| !h.is_empty()) {
        let results = materialize_thumbs(urls, &fetch_data_url).await;
        if !results.is_empty() {
            return WebImageSearchOutcome {
                results,
                source: Some("openverse"),
            };
        }
    }
    let mut wiki = fetch_wikimedia_list(client, &query).await;
    if wiki.is_empty() {
        if let Some(truncated) = two_keyword_retry(&query) {
            wiki = fetch_wikimedia_list(client, &truncated).await;
        }
    }
    let results = materialize_thumbs(wiki, &fetch_data_url).await;
    let source = (!results.is_empty()).then_some("wikimedia");
    WebImageSearchOutcome { results, source }
}

fn two_keyword_retry(query: &str) -> Option<String> {
    let words: Vec<&str> = query.split_whitespace().filter(|w| !w.is_empty()).collect();
    (words.len() > 2).then(|| words[..2].join(" "))
}

pub(crate) struct RawHit {
    id: String,
    thumb_url: String,
    attribution: String,
}

/// `None` = request-level failure (429 / network), `Some([])` = the
/// catalogue answered with zero hits (the ladder distinguishes the two).
async fn fetch_openverse_list(
    client: &reqwest::Client,
    query: &str,
    credentials: Option<&WebOpenverseCredentials>,
) -> Option<Vec<RawHit>> {
    let url = reqwest::Url::parse_with_params(
        "https://api.openverse.org/v1/images/",
        &[
            ("q", query),
            ("page_size", &SEARCH_RESULT_COUNT.to_string()),
        ],
    )
    .ok()?;
    let mut request = client.get(url);
    if let Some(credentials) = credentials {
        if let Some(token) = fetch_openverse_token(client, credentials).await {
            request = request.bearer_auth(token);
        }
    }
    let resp = request.send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let json = read_json_capped(resp).await?;
    Some(parse_openverse_results(&json))
}

/// Catalogue-list bodies are small JSON; 4 MiB bounds a misbehaving reply.
async fn read_json_capped(resp: reqwest::Response) -> Option<serde_json::Value> {
    let bytes = read_capped(resp, MAX_EMBEDDED_IMAGE_BYTES).await?;
    serde_json::from_slice(&bytes).ok()
}

pub(crate) fn parse_openverse_results(json: &serde_json::Value) -> Vec<RawHit> {
    let Some(results) = json.get("results").and_then(serde_json::Value::as_array) else {
        return Vec::new();
    };
    results
        .iter()
        .filter_map(|r| {
            let thumb = r
                .get("thumbnail")
                .and_then(serde_json::Value::as_str)
                .or_else(|| r.get("url").and_then(serde_json::Value::as_str))?;
            let license = format!(
                "{} {}",
                r.get("license")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or(""),
                r.get("license_version")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or(""),
            );
            Some(RawHit {
                id: r
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                thumb_url: thumb.to_string(),
                attribution: r
                    .get("attribution")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| license.trim().to_string()),
            })
        })
        .take(SEARCH_RESULT_COUNT)
        .collect()
}

async fn fetch_wikimedia_list(client: &reqwest::Client, query: &str) -> Vec<RawHit> {
    let Ok(url) = reqwest::Url::parse_with_params(
        "https://commons.wikimedia.org/w/api.php",
        &[
            ("action", "query"),
            ("generator", "search"),
            ("gsrsearch", query),
            ("gsrnamespace", "6"),
            ("gsrlimit", &SEARCH_RESULT_COUNT.to_string()),
            ("prop", "imageinfo"),
            ("iiprop", "url|size|mime|extmetadata"),
            ("iiurlwidth", "800"),
            ("format", "json"),
            ("origin", "*"),
        ],
    ) else {
        return Vec::new();
    };
    let Ok(resp) = client.get(url).send().await else {
        return Vec::new();
    };
    if !resp.status().is_success() {
        return Vec::new();
    }
    let Some(json) = read_json_capped(resp).await else {
        return Vec::new();
    };
    parse_wikimedia_results(&json)
}

pub(crate) fn parse_wikimedia_results(json: &serde_json::Value) -> Vec<RawHit> {
    let Some(pages) = json
        .get("query")
        .and_then(|q| q.get("pages"))
        .and_then(serde_json::Value::as_object)
    else {
        return Vec::new();
    };
    pages
        .values()
        .filter_map(|page| {
            let info = page.get("imageinfo")?.as_array()?.first()?;
            let thumb = info
                .get("thumburl")
                .and_then(serde_json::Value::as_str)
                .or_else(|| info.get("url").and_then(serde_json::Value::as_str))?;
            Some(RawHit {
                id: page
                    .get("pageid")
                    .map(|v| v.to_string())
                    .unwrap_or_default(),
                thumb_url: thumb.to_string(),
                attribution: info
                    .get("extmetadata")
                    .and_then(|m| m.get("LicenseShortName"))
                    .and_then(|l| l.get("value"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
                    .to_string(),
            })
        })
        .take(SEARCH_RESULT_COUNT)
        .collect()
}

/// Download each hit's thumbnail into a `data:` URL through the caller's
/// materializer. Hits whose thumbnails fail to download are dropped.
async fn materialize_thumbs<F, Fut>(hits: Vec<RawHit>, fetch_data_url: &F) -> Vec<WebImageSearchHit>
where
    F: Fn(String) -> Fut,
    Fut: std::future::Future<Output = Option<String>>,
{
    let mut out = Vec::with_capacity(hits.len());
    for hit in hits {
        if let Some(data_url) = fetch_data_url(hit.thumb_url.clone()).await {
            out.push(WebImageSearchHit {
                id: hit.id,
                thumb_data_url: data_url,
                attribution: hit.attribution,
            });
        }
    }
    out
}

/// Simplify a verbose prompt into provider keywords. Shared by the desktop
/// image pipeline and the web daemon route.
pub fn simplify_search_query(prompt: &str) -> String {
    let mut normalized = String::with_capacity(prompt.len());
    for ch in prompt.to_lowercase().chars() {
        if ch.is_ascii_alphanumeric() || ch.is_ascii_whitespace() || ch == '-' {
            normalized.push(ch);
        } else {
            normalized.push(' ');
        }
    }
    let keywords: Vec<&str> = normalized
        .split_whitespace()
        .filter(|word| word.len() > 2 && !IMAGE_SEARCH_STOP_WORDS.contains(word))
        .take(6)
        .collect();
    // Drop artifact words ONLY when aesthetic words remain — "logo" alone
    // must not become an empty query.
    let non_artifact: Vec<&str> = keywords
        .iter()
        .copied()
        .filter(|word| !IMAGE_SEARCH_ARTIFACT_WORDS.contains(word))
        .collect();
    let keywords: Vec<&str> = if non_artifact.is_empty() {
        keywords
    } else {
        non_artifact
    }
    .into_iter()
    .take(4)
    .collect();
    if keywords.is_empty() {
        prompt.chars().take(30).collect()
    } else {
        keywords.join(" ")
    }
}

pub async fn fetch_openverse_token(
    client: &reqwest::Client,
    credentials: &WebOpenverseCredentials,
) -> Option<String> {
    let resp = client
        .post("https://api.openverse.org/v1/auth_tokens/token/")
        .form(&[
            ("grant_type", "client_credentials"),
            ("client_id", credentials.client_id.as_str()),
            ("client_secret", credentials.client_secret.as_str()),
        ])
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let json = read_json_capped(resp).await?;
    json.get("access_token")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(str::to_string)
}

/// Download `url` and embed it as a `data:` URL, subject to the 4 MiB cap.
pub(crate) async fn fetch_image_data_url(client: &reqwest::Client, url: &str) -> Option<String> {
    let (mime, bytes) = fetch_image_bytes(client, url, MAX_EMBEDDED_IMAGE_BYTES).await?;
    use base64::engine::general_purpose::STANDARD as B64;
    use base64::Engine as _;
    Some(format!("data:{mime};base64,{}", B64.encode(&bytes)))
}

/// Download `url` and return its normalized image mime + raw bytes, subject
/// to `cap` (streaming abort). `None` for failures, empty bodies, and
/// non-embeddable mimes. Shared with the desktop, which layers its skia
/// down-scale pass on the bytes before embedding.
pub async fn fetch_image_bytes(
    client: &reqwest::Client,
    url: &str,
    cap: usize,
) -> Option<(String, Vec<u8>)> {
    let resp = client.get(url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let header_mime = resp
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(normalize_image_mime_header);
    let bytes = read_capped(resp, cap).await?;
    if bytes.is_empty() {
        return None;
    }
    let mime = header_mime.or_else(|| sniff_image_mime(&bytes).map(str::to_string))?;
    Some((mime, bytes))
}

/// Read a response body, aborting as soon as it exceeds `cap` — the cap must
/// hold with or without a Content-Length header, and an over-cap body must
/// never be fully buffered first (a chunked response could otherwise stream
/// gigabytes into memory before a post-hoc length check).
pub(crate) async fn read_capped(mut resp: reqwest::Response, cap: usize) -> Option<Vec<u8>> {
    if resp.content_length().is_some_and(|len| len > cap as u64) {
        return None;
    }
    let mut bytes: Vec<u8> = Vec::new();
    while let Some(chunk) = resp.chunk().await.ok()? {
        if bytes.len() + chunk.len() > cap {
            return None;
        }
        bytes.extend_from_slice(&chunk);
    }
    Some(bytes)
}

/// Normalize a Content-Type header into an embeddable `image/*` mime
/// (`image/jpg` alias folded, SVG rejected).
pub fn normalize_image_mime_header(value: &str) -> Option<String> {
    let mime = value.split(';').next()?.trim().to_ascii_lowercase();
    if mime == "image/jpg" {
        return Some("image/jpeg".to_string());
    }
    if mime.starts_with("image/") && mime != "image/svg+xml" {
        Some(mime)
    } else {
        None
    }
}

/// Magic-byte sniff for the embeddable raster formats.
pub fn sniff_image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1A\n") {
        return Some("image/png");
    }
    if bytes.starts_with(b"\xFF\xD8\xFF") {
        return Some("image/jpeg");
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("image/gif");
    }
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    None
}

#[cfg(test)]
#[path = "web_image_search_tests.rs"]
mod tests;
