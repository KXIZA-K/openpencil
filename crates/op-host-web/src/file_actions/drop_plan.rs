//! Browser file classification and HTML-project batch routing.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropKind {
    Document,
    Figma,
    Html,
    HtmlResource,
    Zip,
    Svg,
    Image,
    Pdf,
    Unsupported,
}

/// Classify a file name for the drop / picker router.
pub fn drop_kind(name: &str) -> DropKind {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".op") || lower.ends_with(".pen") {
        DropKind::Document
    } else if lower.ends_with(".fig") {
        DropKind::Figma
    } else if lower.ends_with(".html") || lower.ends_with(".htm") {
        DropKind::Html
    } else if [
        ".css",
        ".js",
        ".mjs",
        ".cjs",
        ".json",
        ".webmanifest",
        ".map",
        ".xml",
        ".txt",
        ".wasm",
        ".woff",
        ".woff2",
        ".ttf",
        ".otf",
        ".eot",
    ]
    .iter()
    .any(|ext| lower.ends_with(ext))
    {
        DropKind::HtmlResource
    } else if lower.ends_with(".zip") {
        DropKind::Zip
    } else if lower.ends_with(".svg") {
        DropKind::Svg
    } else if lower.ends_with(".pdf") {
        DropKind::Pdf
    } else if [
        ".png", ".jpg", ".jpeg", ".gif", ".webp", ".avif", ".ico", ".bmp",
    ]
    .iter()
    .any(|ext| lower.ends_with(ext))
    {
        DropKind::Image
    } else {
        DropKind::Unsupported
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropBatchPlan {
    Individual,
    HtmlProject,
    HtmlZip,
    InvalidHtmlMix,
    InvalidZipMix,
}

/// ZIPs are exclusive; loose projects reject document-like or unknown siblings
/// instead of silently swallowing them as page resources.
pub fn drop_batch_plan(kinds: &[DropKind]) -> DropBatchPlan {
    if kinds.contains(&DropKind::Zip) {
        return if kinds.len() == 1 {
            DropBatchPlan::HtmlZip
        } else {
            DropBatchPlan::InvalidZipMix
        };
    }
    if !kinds.contains(&DropKind::Html) {
        return DropBatchPlan::Individual;
    }
    if kinds.iter().all(|kind| {
        matches!(
            kind,
            DropKind::Html | DropKind::HtmlResource | DropKind::Svg | DropKind::Image
        )
    }) {
        DropBatchPlan::HtmlProject
    } else {
        DropBatchPlan::InvalidHtmlMix
    }
}
