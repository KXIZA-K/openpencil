//! Track A of the interactive-preview plan — deterministic screen/nav wiring.
//!
//! The preview engine (PreviewSession + jian-core `ScreenRouter` App Mode)
//! already understands multi-screen documents: it looks for top-level
//! `FrameNode.screen` markers and `events.onTap` navigation actions. What is
//! missing is generation-side wiring — AI-produced documents may omit these
//! fields, so `project_screens` finds nothing and preview degrades to a single
//! scrolling page. This module fills the screen/nav gap deterministically,
//! while cleanup-only interaction backfill owns strict back/card actions (see
//! `openpencil-docs/openpencil/generation/preview-interactive-app-mode-0712.md`,
//! "Track A contract v2").
//!
//! Hard contract (violating any of these breaks the preview engine, not just
//! this pass's own tests):
//! 1. A navigate action body is a Tier-1 EXPRESSION source, not a bare path —
//!    `Expression::compile` lexes an unquoted `/x` as a division token and
//!    fails to compile. The body must be the JSON string `"\"/path\""` (i.e.
//!    the string VALUE is itself `"/path"` including the quote characters),
//!    so `push`/`replace` bind to a string-literal expression. `pop` takes no
//!    body (`null`). Verified against `jian-core/tests/action_navigation.rs`.
//! 2. `route` is never written — it is schema-only surface metadata that the
//!    gesture dispatcher does not consume; only `events.onTap` drives runtime
//!    navigation.
//! 3. `screen` only ever marks a top-level (page-root-level) frame — the
//!    projection pass (`jian_ops_schema::screen_projection`) only scans
//!    top-level children per page (or per-document when pageless).
//! 4. Idempotent + additive-only: an authored `screen` marker or `events`
//!    collection is never overwritten. Running the pass twice must be a no-op
//!    the second time.
//! 5. Zero new schema fields — only the existing `screen` / `events` fields
//!    are ever written.
//!
//! ## Callers (`pub`, not `pub(crate)`)
//!
//! 1. `crate::cleanup::run_cleanup_passes` — the in-crate generation-pipeline
//!    caller (orchestrator per-subtask cleanup + the agentic loop's whole-doc
//!    finalize), which invokes this after cleanup-only interaction backfill.
//! 2. `op_host_native::preview::auto_wire` (Track C-1) — enter-preview
//!    auto-wiring. When a document carries no authored `screen` marker at
//!    all, the preview host runs this SAME pass over a JSON-cloned
//!    `EditorState` before building the runtime, so a hand-drawn or
//!    pre-Track-A multi-screen document still enters App Mode preview with
//!    zero model cooperation. The saved document is never touched — see
//!    `op-host-native/src/preview/mod.rs`'s "never mutates the saved doc"
//!    invariant.

use std::collections::{BTreeSet, HashMap};

use jian_ops_schema::node::{PenNode, TextContent};
use op_editor_core::{EditorCommand, EditorState, NodeId, PenNodeExt};

use crate::types::DocSink;

/// "Screen-shaped" top-level frame width bands: phone/narrow-tablet portrait
/// widths, or desktop-and-up. Chosen to gate OUT ordinary section widths
/// (e.g. a 600-900px card row) that are not standalone app screens.
const MOBILE_SCREEN_WIDTH: std::ops::RangeInclusive<f64> = 320.0..=480.0;
const DESKTOP_SCREEN_MIN_WIDTH: f64 = 1024.0;

/// Compatibility band for shared-nav's shipped detail-page exemption.
/// Interaction backfill uses its own stricter geometry fact.
const LEGACY_HEADER_REGION_MAX_Y: f64 = 140.0;

const ENTRY_NAME_HINTS: [&str; 4] = ["home", "main", "dashboard", "index"];

/// One screen-shaped top-level frame collected from the active page.
///
/// `pub(crate)` (fields too) — `unify_shared_nav.rs` reuses this SAME
/// screen-shape detection to find the reference/target screens its
/// cross-screen nav unification pass operates on, so "what counts as a
/// screen" can never drift between the two passes.
#[derive(Clone)]
pub(crate) struct ScreenCandidate {
    pub(crate) id: String,
    /// Display name used for slugging + navbar label matching; falls back to
    /// the node id when unnamed.
    pub(crate) name: String,
    /// Pre-existing authored `screen` marker, if any — never overwritten.
    pub(crate) existing_path: Option<String>,
}

/// One shared-navigation surface and the actual row of interactive tab
/// items inside it. Generated mobile chrome commonly nests a horizontal
/// tab row inside a full-width surface wrapper; callers that replace shared
/// chrome need the outer `surface`, while callers that inspect labels or
/// bind events must use `tab_row`.
pub(crate) struct NavParts<'a> {
    pub(crate) surface: &'a PenNode,
    pub(crate) tab_row: &'a PenNode,
    /// Child-index path from `surface` to `tab_row` (empty when the surface
    /// itself directly owns the tabs). The unifier reuses this path on an
    /// owned clone so active-state retargeting cannot drift from selection.
    pub(crate) tab_row_path: Vec<usize>,
}

/// Entry point: mark screen-shaped top-level frames with a `screen` route
/// path and wire each screen's bottom-nav / sidebar-nav tabs to `events.onTap`
/// navigation actions. No-ops when the document
/// has fewer than two screen-shaped top-level frames (single-screen docs
/// keep today's scrolling-page preview — zero regression surface).
pub fn wire_screen_navigation(sink: &mut dyn DocSink) {
    let screens = ensure_screen_routes(sink);
    if screens.len() < 2 {
        return;
    }

    let screen_paths: Vec<(String, String)> = screens
        .iter()
        .filter_map(|screen| {
            screen
                .existing_path
                .as_ref()
                .map(|path| (screen.name.clone(), path.clone()))
        })
        .collect();

    wire_nav_tabs(sink, &screens, &screen_paths);
}

/// Persist routes without wiring interactions. Generation cleanup uses this
/// before its document-writing interaction backfill; preview fallback keeps
/// calling [`wire_screen_navigation`] on a clone and only gets route/nav wiring.
pub(crate) fn ensure_screen_routes(sink: &mut dyn DocSink) -> Vec<ScreenCandidate> {
    let screens = screen_route_inventory::collect_prompt_live_candidates(sink.state());
    if screens.len() < 2 {
        return screens;
    }
    for (node_id, path) in assign_screen_paths(&screens) {
        sink.apply(EditorCommand::PatchNodeData {
            node_id: NodeId::new(node_id),
            patch_json: format!(r#"{{"screen":"{path}"}}"#),
            page_id: None,
        });
    }
    screen_route_inventory::collect_prompt_live_candidates(sink.state())
}

#[path = "screen_route_inventory.rs"]
mod screen_route_inventory;
pub(crate) use screen_route_inventory::{
    ensure_planned_screen_routes, prompt_screen_route_inventory,
};

/// Scan the active page's top-level `Frame` children for screen-shaped
/// candidates (numeric width AND height, width in a mobile or desktop band).
/// `pub(crate)` for `unify_shared_nav`'s reuse — see [`ScreenCandidate`].
pub(crate) fn collect_screen_candidates(state: &EditorState) -> Vec<ScreenCandidate> {
    state
        .active_children()
        .iter()
        .filter_map(|node| {
            let PenNode::Frame(frame) = node else {
                return None;
            };
            let width = node.width_px()?;
            let height = node.height_px()?;
            if height <= 0.0 {
                return None;
            }
            if !(MOBILE_SCREEN_WIDTH.contains(&width) || width >= DESKTOP_SCREEN_MIN_WIDTH) {
                return None;
            }
            Some(ScreenCandidate {
                id: frame.base.id.clone(),
                name: frame
                    .base
                    .name
                    .clone()
                    .unwrap_or_else(|| frame.base.id.clone()),
                existing_path: frame.screen.clone(),
            })
        })
        .collect()
}

/// Assign a unique `/slug` path to every candidate that lacks an authored
/// `screen` marker. Exactly one candidate becomes the `"/"` entry — unless an
/// authored marker already claims `"/"`, in which case no new entry is
/// picked (an authored marker is never touched, even to satisfy the
/// single-entry rule; see contract point 4). Returns `(node_id, path)`
/// pairs to patch.
fn assign_screen_paths(candidates: &[ScreenCandidate]) -> Vec<(String, String)> {
    let mut used: BTreeSet<String> = candidates
        .iter()
        .filter_map(|c| c.existing_path.clone())
        .collect();
    let unmarked: Vec<usize> = candidates
        .iter()
        .enumerate()
        .filter(|(_, c)| c.existing_path.is_none())
        .map(|(i, _)| i)
        .collect();
    if unmarked.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    let entry_idx = if used.contains("/") {
        None
    } else {
        Some(
            unmarked
                .iter()
                .copied()
                .find(|&i| is_entry_name(&candidates[i].name))
                .unwrap_or(unmarked[0]),
        )
    };
    if let Some(idx) = entry_idx {
        used.insert("/".to_string());
        out.push((candidates[idx].id.clone(), "/".to_string()));
    }

    let mut fallback_index = 0usize;
    for &i in &unmarked {
        if Some(i) == entry_idx {
            continue;
        }
        fallback_index += 1;
        let slug = normalize_slug(&candidates[i].name);
        let path = unique_path(&slug, fallback_index, &mut used);
        out.push((candidates[i].id.clone(), path));
    }
    out
}

fn is_entry_name(name: &str) -> bool {
    let slug = normalize_slug(name);
    ENTRY_NAME_HINTS.iter().any(|hint| slug.contains(hint))
}

/// Lowercase ASCII-alnum slug with single hyphens between runs — non-ASCII
/// (CJK, emoji) and punctuation are stripped, not transliterated. An empty
/// result (all-non-ASCII name) falls back to `screen-N` in [`unique_path`].
fn normalize_slug(name: &str) -> String {
    let mut out = String::new();
    let mut pending_sep = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            if pending_sep && !out.is_empty() {
                out.push('-');
            }
            out.push(ch.to_ascii_lowercase());
            pending_sep = false;
        } else {
            pending_sep = true;
        }
    }
    out
}

fn unique_path(slug: &str, fallback_index: usize, used: &mut BTreeSet<String>) -> String {
    let base = if slug.is_empty() {
        format!("screen-{fallback_index}")
    } else {
        slug.to_string()
    };
    let mut path = format!("/{base}");
    let mut suffix = 2;
    while used.contains(&path) {
        path = format!("/{base}-{suffix}");
        suffix += 1;
    }
    used.insert(path.clone());
    path
}

// ── Navbar wiring ───────────────────────────────────────────────────────

/// Bind every screen's bottom-tab-bar / sidebar-nav tab items to `replace`
/// navigation toward the screen whose name matches the tab's label —
/// including a screen's own tab pointing back at itself (each screen wires
/// its navbar independently). A tab already carrying `events` is left alone.
fn wire_nav_tabs(
    sink: &mut dyn DocSink,
    screens: &[ScreenCandidate],
    screen_paths: &[(String, String)],
) {
    // Raw (un-normalized) names — `labels_match` normalizes + tokenizes
    // internally, so a brand-prefixed screen name ("Wander — Trips") still
    // matches a bare tab label ("Trips").
    let screen_names: Vec<(&str, &str)> = screen_paths
        .iter()
        .filter(|(_, path)| !path.is_empty())
        .map(|(name, path)| (name.as_str(), path.as_str()))
        .collect();

    let mut patches: Vec<(String, String)> = Vec::new();
    for screen in screens {
        let Some(root) = op_editor_core::walkers::find_node(
            sink.state().active_children(),
            &NodeId::new(screen.id.clone()),
        ) else {
            continue;
        };
        let mut navs = Vec::new();
        collect_nav_parts(root, &mut navs);
        for nav in navs {
            let Some(items) = nav.tab_row.children() else {
                continue;
            };
            for item in items {
                if subtree_has_events(item) {
                    continue;
                }
                let Some(label) = first_text_content(item) else {
                    continue;
                };
                let matched_path = screen_names
                    .iter()
                    .find(|(screen_name, _)| labels_match(label, screen_name))
                    .map(|(_, path)| path.to_string());
                let Some(path) = matched_path else {
                    continue;
                };
                patches.push((item.id_str().to_string(), navigate_patch("replace", &path)));
            }
        }
    }
    for (node_id, patch_json) in patches {
        sink.apply(EditorCommand::PatchNodeData {
            node_id: NodeId::new(node_id),
            patch_json,
            page_id: None,
        });
    }
}

/// Collect outer shared-nav surfaces together with their actual tab rows.
/// Once a matching outer surface is found its nested nav-shaped descendants
/// are consumed as part of the same [`NavParts`] instead of being reported
/// as duplicate navs.
pub(crate) fn collect_nav_parts<'a>(node: &'a PenNode, out: &mut Vec<NavParts<'a>>) {
    if is_nav_container(node) {
        if let Some(tab_row_path) = best_tab_row_path(node) {
            if let Some(tab_row) = node_at_path(node, &tab_row_path) {
                out.push(NavParts {
                    surface: node,
                    tab_row,
                    tab_row_path,
                });
                return;
            }
        }
    }
    for child in node.children().into_iter().flatten() {
        collect_nav_parts(child, out);
    }
}

/// `pub` because `op-smoke`'s cross-crate audit reuses nav detection. A
/// confidently identified nested nav returns its actual tab row, never both
/// wrapper and row. Shapes that cannot be label-bound (icon-only, one item,
/// repeated labels) fall back to the authored nav surface so audits retain
/// their historical visibility even though writers safely abstain.
pub fn collect_nav_containers<'a>(node: &'a PenNode, out: &mut Vec<&'a PenNode>) {
    if is_nav_container(node) {
        if let Some(tab_row) = best_tab_row_path(node).and_then(|path| node_at_path(node, &path)) {
            out.push(tab_row);
        } else {
            // Preserve this public collector's historical visibility for
            // icon-only, single-item, repeated-label, or otherwise
            // non-bindable navs. Writers abstain without a confident row,
            // but audit callers should still see the authored nav surface.
            out.push(node);
        }
        return;
    }
    for child in node.children().into_iter().flatten() {
        collect_nav_containers(child, out);
    }
}

fn best_tab_row_path(surface: &PenNode) -> Option<Vec<usize>> {
    #[derive(Clone)]
    struct Candidate {
        path: Vec<usize>,
        score: (bool, usize, usize),
    }

    fn visit(
        node: &PenNode,
        path: &mut Vec<usize>,
        best: &mut Option<Candidate>,
        ambiguous: &mut bool,
    ) {
        if let Some(children) = node.children() {
            let labels: Option<Vec<String>> = if children.len() >= 2 {
                children
                    .iter()
                    .map(|child| {
                        if child.children().is_some_and(|nested| !nested.is_empty()) {
                            first_text_content(child)
                                .map(str::trim)
                                .filter(|label| !label.is_empty())
                                .map(str::to_lowercase)
                        } else {
                            None
                        }
                    })
                    .collect()
            } else {
                None
            };
            if let Some(labels) = labels {
                let distinct = labels.iter().collect::<BTreeSet<_>>().len();
                if distinct == labels.len() {
                    let role = node.base().role.as_deref().unwrap_or("");
                    let identity = identity_haystack(node);
                    let explicitly_tab_shaped = matches!(
                        role,
                        "tab-row" | "tab-bar" | "bottom-tab-bar" | "nav" | "sidebar"
                    ) || identity.contains("tab row")
                        || identity.contains("tab-row")
                        || identity.contains("tab bar")
                        || identity.contains("tab-bar");
                    let candidate = Candidate {
                        path: path.clone(),
                        // Explicit tab identity outranks raw item count: a
                        // five-card content list nested in a nav/sidebar
                        // must not steal selection from a four-item tab row.
                        score: (explicitly_tab_shaped, labels.len(), path.len()),
                    };
                    match best {
                        Some(current) if candidate.score > current.score => {
                            *best = Some(candidate);
                            *ambiguous = false;
                        }
                        Some(current) if candidate.score == current.score => {
                            *ambiguous = true;
                        }
                        None => *best = Some(candidate),
                        _ => {}
                    }
                }
            }

            for (index, child) in children.iter().enumerate() {
                path.push(index);
                visit(child, path, best, ambiguous);
                path.pop();
            }
        }
    }

    let mut best = None;
    let mut ambiguous = false;
    visit(surface, &mut Vec::new(), &mut best, &mut ambiguous);
    if ambiguous {
        None
    } else {
        best.map(|candidate| candidate.path)
    }
}

fn node_at_path<'a>(mut node: &'a PenNode, path: &[usize]) -> Option<&'a PenNode> {
    for &index in path {
        node = node.children()?.get(index)?;
    }
    Some(node)
}

fn is_nav_container(node: &PenNode) -> bool {
    let role = node
        .base()
        .role
        .as_deref()
        .unwrap_or("")
        .to_ascii_lowercase();
    if matches!(
        role.as_str(),
        "nav" | "tab-bar" | "bottom-tab-bar" | "tab-row" | "sidebar" | "side-nav" | "nav-rail"
    ) {
        return true;
    }
    let hay = identity_haystack(node);
    [
        "bottom nav",
        "bottom-nav",
        "bottom navigation",
        "bottom-navigation",
        "tab bar",
        "tab-bar",
        "sidebar",
        "side nav",
        "side-nav",
        "nav rail",
        "nav-rail",
    ]
    .iter()
    .any(|needle| hay.contains(needle))
}

fn identity_haystack(node: &PenNode) -> String {
    format!(
        "{} {}",
        node.id_str().to_ascii_lowercase(),
        node.base()
            .name
            .as_deref()
            .unwrap_or("")
            .to_ascii_lowercase()
    )
}

/// First non-empty plain-text content found via depth-first search — the
/// tab-item label (icon + label rows are the universal shape; styled/rich
/// text is skipped rather than guessed at). `pub(crate)` for the `navIssues`
/// echo scan — see [`collect_nav_containers`].
pub(crate) fn first_text_content(node: &PenNode) -> Option<&str> {
    if let PenNode::Text(text) = node {
        return match &text.content {
            TextContent::Plain(s) if !s.trim().is_empty() => Some(s.as_str()),
            _ => None,
        };
    }
    node.children()?.iter().find_map(first_text_content)
}

/// `pub(crate)` for the `navIssues` echo scan — see [`collect_nav_containers`].
/// NOT `is_ascii_alphanumeric`: that dropped every CJK codepoint, so a
/// Chinese app normalized every label and screen name to the EMPTY string and
/// `labels_match` refused them all. The whole tab-navigation layer therefore
/// no-opped on such documents — measured on `0808-k3-2.op`, a two-screen
/// Chinese star-gazing app whose screen ROUTES were written (that path never
/// looks at labels) while not one tab received an `onTap` action.
pub(crate) fn normalize_label(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// Split a label/screen name on the separators an authored app commonly
/// uses for a brand prefix, a hyphenated subtitle, or a breadcrumb (space,
/// em dash, en dash, hyphen, colon, middle dot, pipe) into normalized,
/// non-empty word tokens. "Wander — Trips" → `["wander", "trips"]`.
fn label_tokens(s: &str) -> Vec<String> {
    s.split([' ', '—', '–', '-', ':', '·', '|'])
        .map(normalize_label)
        .filter(|t| !t.is_empty())
        .collect()
}

/// Match iff the fully-normalized forms are equal or one is a prefix of the
/// other (covers "Profile" vs "Profile Screen" → "profile" /
/// "profilescreen") — OR, failing that, the two labels share a whole word
/// TOKEN once split on the usual separators (covers a brand-prefixed screen
/// name, "Wander — Trips", matching a bare tab label "Trips": neither
/// normalized form is a prefix of the other — "trips" vs "wandertrips" —
/// but "trips" is a token of both). Token matching compares WHOLE tokens
/// only, never a bare substring — "Roadtrips" tokenizes to `["roadtrips"]`,
/// which never equals the "trips" token, so it correctly stays unmatched.
/// Ambiguous (neither) never binds — a wrong navigate is worse than a dead
/// tap. Takes RAW (un-normalized) label/name text — both checks normalize
/// internally, so every caller passes its text straight through instead of
/// pre-normalizing (pre-normalizing would destroy the word boundaries the
/// token check needs). `pub(crate)` for the `navIssues` echo scan — see
/// [`collect_nav_containers`].
pub(crate) fn labels_match(a: &str, b: &str) -> bool {
    let (na, nb) = (normalize_label(a), normalize_label(b));
    if na.is_empty() || nb.is_empty() {
        return false;
    }
    if na == nb || na.starts_with(&nb) || nb.starts_with(&na) {
        return true;
    }
    let ta = label_tokens(a);
    let tb = label_tokens(b);
    ta.iter().any(|t| tb.contains(t))
}

// ── Shared-nav legacy detail signal ──────────────────────────────────────

/// Preserve shared-nav's shipped detail-page exemption. This intentionally
/// remains broader than M1 interaction backfill: it may recognize legacy
/// direct icons, roles, and authored names, but it never writes an event.
/// The strict cleanup repair and diagnostic predicate lives in
/// `geometry_interaction_backfill`.
pub(crate) fn screen_has_back_control_in_header(
    sink: &dyn DocSink,
    screen: &ScreenCandidate,
) -> bool {
    if crate::geometry_validation::screen_has_back_control_shape(sink.state(), &screen.id) {
        return true;
    }
    let Some(root) = op_editor_core::walkers::find_node(
        sink.state().active_children(),
        &NodeId::new(screen.id.clone()),
    ) else {
        return false;
    };
    let y_offsets = resolved_y_offsets(sink.state());
    let screen_top = y_offsets.get(&screen.id).copied().unwrap_or(0.0);
    let mut hits = Vec::new();
    collect_legacy_back_controls(root, screen_top, &y_offsets, &mut hits);
    !hits.is_empty()
}

fn collect_legacy_back_controls(
    node: &PenNode,
    screen_top: f64,
    y_offsets: &HashMap<String, f64>,
    out: &mut Vec<String>,
) {
    if node_has_events(node) {
        return;
    }
    if is_legacy_back_control(node) {
        let within_header = y_offsets
            .get(node.id_str())
            .is_some_and(|y| (y - screen_top) <= LEGACY_HEADER_REGION_MAX_Y);
        if within_header {
            out.push(node.id_str().to_string());
            return;
        }
    }
    for child in node.children().into_iter().flatten() {
        collect_legacy_back_controls(child, screen_top, y_offsets, out);
    }
}

fn is_legacy_back_control(node: &PenNode) -> bool {
    if matches!(
        node.base().role.as_deref(),
        Some("back" | "back-button" | "nav-back")
    ) {
        return true;
    }
    let mut hay = compact_lower(node.base().name.as_deref().unwrap_or(""));
    hay.push(' ');
    hay.push_str(&compact_lower(node.id_str()));
    if let PenNode::IconFont(icon) = node {
        hay.push(' ');
        hay.push_str(&compact_lower(&icon.icon_font_name));
    }
    hay.contains("back") || hay.contains("arrowleft") || hay.contains("chevronleft")
}

fn compact_lower(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(|character| character.to_lowercase())
        .collect()
}

// ── Shared helpers ──────────────────────────────────────────────────────

/// Does this node already carry a (non-empty) `events` block? Checked via
/// JSON rather than a per-variant match since every actionable node variant
/// (Frame/Group/Rectangle/Text/IconFont/…) carries the same optional field —
/// idempotency only needs "is it present", not which handler. `pub` for the
/// in-crate `navIssues` echo scan AND `op-smoke`'s `audit_rubric`
/// (`navBoundTabs`) — see [`collect_nav_containers`].
pub fn node_has_events(node: &PenNode) -> bool {
    serde_json::to_value(node)
        .ok()
        .and_then(|v| v.get("events").cloned())
        .is_some()
}

/// Whether `node` or any descendant already owns an authored interaction.
/// A nav tab whose inner icon/button carries the action is already bound;
/// adding a second event to the tab root would double-dispatch the tap.
pub fn subtree_has_events(node: &PenNode) -> bool {
    node_has_events(node)
        || node
            .children()
            .into_iter()
            .flatten()
            .any(subtree_has_events)
}

/// Build the `events.onTap` navigate patch JSON. `path` must already be a
/// `/`-rooted route path; the JSON string VALUE is the literal
/// `"<path>"` (quotes included) so it compiles as a Tier-1 string-literal
/// expression — see the module doc, contract point 1.
pub(crate) fn navigate_patch(verb: &str, path: &str) -> String {
    let body = serde_json::to_string(path).unwrap_or_default(); // -> "\"/path\""
    let escaped_body = serde_json::to_string(&body).unwrap_or_default(); // -> "\"\\\"/path\\\"\""
    format!(r#"{{"events":{{"onTap":[{{"{verb}":{escaped_body}}}]}}}}"#)
}

fn resolved_y_offsets(state: &EditorState) -> HashMap<String, f64> {
    let scene = op_pen_loader::editor_state_to_active_page_layout_scene(state);
    let mut out = HashMap::new();
    if let Some(page) = scene.active_page() {
        collect_y_offsets(&page.children, &mut out);
    }
    out
}

fn collect_y_offsets(
    nodes: &[jian_scene::layout_scene::SceneNode],
    out: &mut HashMap<String, f64>,
) {
    for node in nodes {
        let bounds = node.aggregate_bounds();
        out.insert(node.id.clone(), f64::from(bounds.origin.y));
        collect_y_offsets(&node.children, out);
    }
}

#[cfg(test)]
#[path = "wire_screen_navigation_tests.rs"]
mod tests;
