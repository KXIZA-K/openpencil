//! `fix_invisible_text_band` (the glm banner-fill floor) and the
//! dominant-design-accent tally it shares with the nav rounding pass.

use super::*;

// ── fix_invisible_text_band (glm banner-fill floor, not a TS pass) ───────────
//
// glm intermittently designs a promo banner with WHITE text but omits the
// colored/gradient fill it intended (gen2/3/5: "Get 30% Off" white-on-cream =
// invisible). Deterministic floor: on a LIGHT page, a fill-less container whose
// text descendants are ALL white/light → it was meant to sit on a colored
// surface → stamp `$color-accent` so the copy becomes readable. Conservative:
// fires only when every text is light (no dark text to contradict) and the
// container truly has no renderable fill.

// Light text tokens/hexes that vanish on a light page — white + the neutral
// surface tints (a banner headline written in any of these needs a colored
// surface beneath it).
pub(super) const LIGHT_TEXT_REFS: &[&str] = &[
    "$color-surface",
    "$color-surface-2",
    "$color-surface-3",
    "$color-bg-deep",
];
pub(super) const LIGHT_TEXT_HEXES: &[&str] = &["#ffffff", "#fff", "#fefefe", "#fdfdfd", "white"];

pub(super) fn is_light_text(color: &str) -> bool {
    if LIGHT_TEXT_REFS.contains(&color) {
        return true;
    }
    LIGHT_TEXT_HEXES.contains(&normalize_hex(color).as_str())
}

/// Tally text colors that sit DIRECTLY on this container's (unfilled) surface.
/// Do NOT descend into a child that carries its own renderable fill — a button
/// / avatar / chip has its own surface, so its text colour says nothing about
/// whether THIS container needs a fill (e.g. a promo banner's white headline +
/// an orange-text "Order Now" button on a white pill: only the headline counts).
pub(super) fn tally_surface_text_colors(node: &Value, light: &mut usize, dark: &mut usize) {
    for child in children_of(node) {
        if child.get("type").and_then(Value::as_str) == Some("text") {
            if let Some(c) = first_solid_color(child) {
                if is_light_text(&c) {
                    *light += 1;
                } else {
                    *dark += 1;
                }
            }
        } else if !has_renderable_fill(child) {
            tally_surface_text_colors(child, light, dark);
        }
    }
}

/// The emphasis color the design ACTUALLY uses (glm often repurposes a chart
/// token as the brand accent because the palette's `$color-accent` defaults to
/// blue — wrong for a warm app). Returns the first chart/accent/primary token
/// found in the subtree, so the band matches the rest of the screen instead of
/// stamping a clashing default blue.
pub(super) fn find_design_accent(node: &Value) -> Option<String> {
    if let Some(c) = first_solid_color(node) {
        if c == "$color-accent"
            || c == "$color-primary"
            || c == "$color-brand"
            || c.starts_with("$color-chart-")
        {
            return Some(c);
        }
    }
    for child in children_of(node) {
        if let Some(c) = find_design_accent(child) {
            return Some(c);
        }
    }
    None
}

/// True when a solid fill color reads as a light page/surface tone — light/white
/// text on it is invisible. Covers the neutral surface variable refs (binding
/// hasn't resolved them at this pass) plus white-ish hexes.
pub(super) fn is_light_surface_color(color: &str) -> bool {
    matches!(
        color,
        "$color-surface" | "$color-surface-2" | "$color-surface-3" | "$color-bg-deep"
    ) || SAFE_LIGHT_HEXES.contains(&normalize_hex(color).as_str())
}

pub(super) fn fix_invisible_text_band(node: &mut Value, light_theme: bool, design_accent: &str) {
    if !light_theme {
        return; // white text on a dark page is fine
    }
    if node.get("type").and_then(Value::as_str) != Some("frame") {
        return;
    }
    // Exempt status bars (role or name/id aliases, including Chinese): they are
    // fill-less by contract and their foreground is already derived from the root's
    // luminance, so "light text on no fill" is a false positive here. Stamping the
    // accent would turn OS chrome into a coloured band.
    if crate::cleanup::is_status_bar_from_json(node) {
        return;
    }
    // Skip only when the node ALREADY paints a non-light surface (a colored or
    // dark solid, or a gradient / image) — light text reads fine there. A node
    // with NO fill, OR a LIGHT-SURFACE solid fill (`$color-surface`, white), is a
    // band where light text is invisible. The latter is the broken-promo-banner
    // case: glm gives the card white text + a dark CTA + a translucent-white
    // badge (all implying a colored background) yet fills the card with
    // `$color-surface`, so the headline vanishes. Repaint with the design accent.
    if has_renderable_fill(node) {
        match first_solid_color(node) {
            Some(c) if is_light_surface_color(&c) => {} // light surface → still invisible
            _ => return, // colored/dark solid or gradient → real surface, text fine
        }
    }
    let (mut light, mut dark) = (0usize, 0usize);
    tally_surface_text_colors(node, &mut light, &mut dark);
    if light >= 1 && dark == 0 {
        if let Some(obj) = node.as_object_mut() {
            obj.insert(
                "fill".to_string(),
                json!([{ "type": "solid", "color": design_accent }]),
            );
        }
    }
}

/// The design's DOMINANT accent token across already-generated siblings — glm
/// uses a chart token as the de-facto brand accent (the palette's
/// `$color-accent` often defaults to a clashing blue). Counting across the
/// assembled-so-far page (passed by the caller from the doc sink) picks e.g.
/// `$color-chart-6` when it's used 9× vs `$color-accent` 1×, so an injected
/// banner band matches the rest of the screen.
pub fn dominant_design_accent(nodes: &[PenNode]) -> Option<String> {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for n in nodes {
        if let Ok(v) = serde_json::to_value(n) {
            tally_accent(&v, &mut counts);
        }
    }
    counts.into_iter().max_by_key(|(_, n)| *n).map(|(c, _)| c)
}

pub(super) fn tally_accent(node: &Value, counts: &mut Vec<(String, usize)>) {
    if let Some(c) = first_solid_color(node) {
        if c == "$color-accent"
            || c == "$color-primary"
            || c == "$color-brand"
            || c.starts_with("$color-chart-")
        {
            if let Some(e) = counts.iter_mut().find(|(k, _)| *k == c) {
                e.1 += 1;
            } else {
                counts.push((c, 1));
            }
        }
    }
    for child in children_of(node) {
        tally_accent(child, counts);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Status bar with role="status-bar" and white 9:41 text under a dark root
    /// should NOT get the design_accent stamp.
    #[test]
    fn test_status_bar_role_exempt_from_text_band_stamp() {
        let mut bar = json!({
            "type": "frame",
            "id": "status-bar-001",
            "role": "status-bar",
            "children": [
                {
                    "type": "text",
                    "content": "9:41",
                    "fill": [{"type": "solid", "color": "#ffffffff"}]
                }
            ]
        });

        // Apply the pass: light_theme = true, design_accent = "$color-accent" (blue).
        fix_invisible_text_band(&mut bar, true, "$color-accent");

        // Status bar should have NO fill (exempt by role).
        assert!(
            bar.get("fill").is_none(),
            "Status bar with role should not receive fill stamp"
        );
    }

    /// Status bar identified by name only (no role) should also be exempt.
    #[test]
    fn test_status_bar_name_exempt_from_text_band_stamp() {
        let mut bar = json!({
            "type": "frame",
            "id": "bar-001",
            "name": "Status Bar",
            "children": [
                {
                    "type": "text",
                    "content": "9:41",
                    "fill": [{"type": "solid", "color": "#ffffffff"}]
                }
            ]
        });

        fix_invisible_text_band(&mut bar, true, "$color-accent");

        assert!(
            bar.get("fill").is_none(),
            "Status bar identified by name should not receive fill stamp"
        );
    }

    /// Non-status-bar frame with the same light-text shape SHOULD get the stamp.
    #[test]
    fn test_non_status_bar_with_light_text_gets_stamp() {
        let mut banner = json!({
            "type": "frame",
            "id": "banner-001",
            "name": "Promo Banner",
            "children": [
                {
                    "type": "text",
                    "content": "Get 30% Off",
                    "fill": [{"type": "solid", "color": "#ffffff"}]
                }
            ]
        });

        fix_invisible_text_band(&mut banner, true, "$color-accent");

        // Non-status-bar frame SHOULD get the accent fill.
        assert!(
            banner.get("fill").is_some(),
            "Non-status-bar frame with light text should receive fill stamp"
        );
        assert_eq!(
            banner["fill"][0]["color"], "$color-accent",
            "Fill should be the design accent"
        );
    }

    /// Status bar with Chinese alias (e.g., 顶部状态栏) should be exempt.
    #[test]
    fn test_status_bar_chinese_name_exempt() {
        let mut bar = json!({
            "type": "frame",
            "id": "bar-001",
            "name": "顶部状态栏",
            "children": [
                {
                    "type": "text",
                    "content": "9:41",
                    "fill": [{"type": "solid", "color": "#ffffffff"}]
                }
            ]
        });

        fix_invisible_text_band(&mut bar, true, "$color-accent");

        assert!(
            bar.get("fill").is_none(),
            "Status bar with Chinese name should not receive fill stamp"
        );
    }
}
