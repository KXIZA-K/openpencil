use crate::theme::Theme;
use crate::widgets::icons::{draw_icon, Icon};
use crate::widgets::PaintCx;
use crate::{Point2D, Rect, TextLayout};
use jian_ops_schema::node::PenNode;
use op_editor_core::PenNodeExt;
use std::collections::BTreeMap;

pub(crate) const DESIGN_BLOCK_H: f32 = 32.0;
const DESIGN_ICON_LEFT: f32 = 12.0;
const DESIGN_ICON_BG: f32 = 16.0;
const DESIGN_ICON_SIZE: f32 = 10.0;
const DESIGN_ICON_GAP: f32 = 10.0;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PendingDesignBlock {
    pub element_count: usize,
    pub label: String,
    pub streaming: bool,
    pub applied: bool,
    pub code: String,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DesignBlock {
    pub rect: Rect,
    pub header: Rect,
    pub copy: Rect,
    pub body: Rect,
    pub apply: Option<Rect>,
    pub expanded: bool,
    pub element_count: usize,
    pub label: String,
    pub streaming: bool,
    pub applied: bool,
    pub code: String,
    pub code_lines: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DesignExtraction {
    pub visible_text: String,
    pub blocks: Vec<PendingDesignBlock>,
}

/// Fold only complete, acknowledged edit protocol from historical assistant
/// turns. Ordinary JSON and unacknowledged proposals remain visible as-is.
pub(crate) fn extract_applied_edit_json(text: &str) -> Option<DesignExtraction> {
    let trimmed = text.trim();
    let code = trimmed.strip_prefix("```json")
        .and_then(|rest| rest.strip_suffix("```"))
        .unwrap_or(trimmed).trim();
    let value: serde_json::Value = serde_json::from_str(code).ok()?;
    let items = value.as_array()?;
    if items.is_empty() || !items.iter().all(|item| {
        let Some(object) = item.as_object() else { return false };
        if !object.get("id").and_then(serde_json::Value::as_str)
            .is_some_and(|id| !id.is_empty()) { return false; }
        match object.get("op").and_then(serde_json::Value::as_str) {
            Some("update") => object.get("data").is_some_and(serde_json::Value::is_object),
            Some("delete") => true,
            None => object_is_design_node(object),
            _ => false,
        }
    }) { return None; }
    Some(DesignExtraction {
        visible_text: String::new(),
        blocks: vec![PendingDesignBlock {
            element_count: items.len(), label: String::new(), streaming: false,
            applied: true, code: code.to_owned(),
        }],
    })
}

pub(crate) fn extract_design_json_blocks(text: &str, is_streaming: bool) -> DesignExtraction {
    let mut visible = Vec::new();
    let mut blocks = Vec::new();
    let mut in_code = false;
    let mut code_lang = String::new();
    let mut code = String::new();

    for line in text.lines() {
        if line.starts_with("```") && !in_code {
            in_code = true;
            code_lang = line[3..].trim().to_string();
            code.clear();
            continue;
        }
        if line.starts_with("```") && in_code {
            finish_code_block(&mut visible, &mut blocks, &code_lang, &code, false);
            in_code = false;
            continue;
        }
        if in_code {
            if !code.is_empty() {
                code.push('\n');
            }
            code.push_str(line);
        } else {
            visible.push(line.to_string());
        }
    }

    if in_code {
        finish_code_block(&mut visible, &mut blocks, &code_lang, &code, is_streaming);
    }

    DesignExtraction {
        visible_text: collapse_blank_lines(&visible.join("\n")).trim().to_string(),
        blocks,
    }
}

fn finish_code_block(
    visible: &mut Vec<String>,
    blocks: &mut Vec<PendingDesignBlock>,
    code_lang: &str,
    code: &str,
    streaming: bool,
) {
    if code_lang == "json" && is_design_json(code) {
        let element_count = design_element_count(code);
        let label = if streaming {
            "Generating design...".to_string()
        } else {
            format!(
                "{element_count} design element{}",
                if element_count == 1 { "" } else { "s" }
            )
        };
        blocks.push(PendingDesignBlock {
            element_count,
            label,
            streaming,
            applied: false,
            code: code.trim_end().to_string(),
        });
    } else {
        visible.push(format!("```{code_lang}"));
        visible.extend(code.lines().map(str::to_string));
        visible.push("```".to_string());
    }
}

fn is_design_json(code: &str) -> bool {
    let trimmed = code.trim_start();
    if !(trimmed.starts_with('[') || trimmed.starts_with('{')) || !code.contains("\"type\"") {
        return false;
    }
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(code) {
        return value_contains_design_node(&value);
    }
    extract_json_objects(trimmed)
        .map(|objects| objects.iter().any(value_contains_design_node))
        .unwrap_or(false)
}

fn value_contains_design_node(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Array(items) => items.iter().any(value_contains_design_node),
        serde_json::Value::Object(object) => object_is_design_node(object),
        _ => false,
    }
}

fn object_is_design_node(object: &serde_json::Map<String, serde_json::Value>) -> bool {
    let Some(node_type) = object.get("type").and_then(serde_json::Value::as_str) else {
        return false;
    };
    object.contains_key("_parent")
        || is_known_design_node_type(node_type)
        || has_design_props(object)
}

fn is_known_design_node_type(node_type: &str) -> bool {
    let normalized = node_type
        .chars()
        .filter(|ch| *ch != '_' && *ch != '-' && !ch.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect::<String>();
    matches!(
        normalized.as_str(),
        "frame"
            | "group"
            | "rectangle"
            | "rect"
            | "ellipse"
            | "line"
            | "polygon"
            | "path"
            | "text"
            | "textinput"
            | "textarea"
            | "image"
            | "iconfont"
            | "select"
            | "switch"
            | "checkbox"
            | "slider"
            | "radiogroup"
            | "numberinput"
            | "progress"
            | "tabs"
            | "ref"
    )
}

fn has_design_props(object: &serde_json::Map<String, serde_json::Value>) -> bool {
    const DESIGN_PROPS: &[&str] = &[
        "x",
        "y",
        "width",
        "height",
        "children",
        "fill",
        "stroke",
        "cornerRadius",
        "fontSize",
        "fontFamily",
        "fontWeight",
        "lineHeight",
        "letterSpacing",
        "textAlign",
        "content",
        "opacity",
        "rotation",
        "src",
        "points",
        "layoutMode",
        "primaryAxisSizingMode",
        "counterAxisSizingMode",
    ];
    object
        .keys()
        .any(|key| DESIGN_PROPS.contains(&key.as_str()))
}

pub(crate) fn applied_design_block_label(
    locale: op_editor_core::Locale,
    element_count: usize,
) -> String {
    let prefix = op_i18n::translate(locale, "ai.modificationApplied");
    let count = i64::try_from(element_count).unwrap_or(i64::MAX);
    let unit_key = match op_i18n::plural_category(locale, count) {
        op_i18n::PluralCategory::One => "ai.designElement",
        op_i18n::PluralCategory::Few => "ai.designElementsFew",
        _ => "ai.designElements",
    };
    let unit = op_i18n::translate(locale, unit_key);
    format!("{prefix} · {element_count} {unit}")
}

fn design_element_count(code: &str) -> usize {
    match serde_json::from_str::<serde_json::Value>(code) {
        Ok(serde_json::Value::Array(items)) => items.len(),
        Ok(serde_json::Value::Object(_)) => 1,
        _ if code.contains("\"_parent\"") => code
            .lines()
            .filter(|line| line.trim_start().starts_with('{'))
            .count(),
        _ => 0,
    }
}

/// A rejected AI design-JSON payload.
///
/// `Display` reproduces the previous ad-hoc `String` messages byte for byte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesignParseError {
    /// The code block is blank.
    Empty,
    /// A JSON value did not deserialize into a `PenNode`.
    Deserialize(String),
    /// A JSONL slice was not valid JSON.
    Json(String),
    /// The payload is neither a node object nor a node array.
    NotObjectOrArray,
    /// The payload parsed but carries no nodes.
    EmptyNodeArray,
    /// The JSONL scan found no `{...}` objects.
    NoJsonObjects,
    /// A JSONL object is truncated (unbalanced braces).
    IncompleteJsonObject,
    /// Every JSONL node resolved as a child, leaving no root.
    EmptyNodeTree,
}

impl std::fmt::Display for DesignParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DesignParseError::Empty => write!(f, "empty design JSON"),
            DesignParseError::Deserialize(e) => write!(f, "deserialize: {e}"),
            DesignParseError::Json(e) => write!(f, "json: {e}"),
            DesignParseError::NotObjectOrArray => {
                write!(f, "design JSON must be an object or array")
            }
            DesignParseError::EmptyNodeArray => write!(f, "empty design node array"),
            DesignParseError::NoJsonObjects => write!(f, "no JSON objects found"),
            DesignParseError::IncompleteJsonObject => write!(f, "incomplete JSON object"),
            DesignParseError::EmptyNodeTree => write!(f, "empty design node tree"),
        }
    }
}

impl std::error::Error for DesignParseError {}

impl From<DesignParseError> for String {
    fn from(error: DesignParseError) -> String {
        error.to_string()
    }
}

pub fn parse_design_json_nodes(code: &str) -> Result<Vec<PenNode>, DesignParseError> {
    let trimmed = code.trim();
    if trimmed.is_empty() {
        return Err(DesignParseError::Empty);
    }
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
        return parse_design_value(value);
    }
    parse_jsonl_nodes(trimmed)
}

fn parse_design_value(mut value: serde_json::Value) -> Result<Vec<PenNode>, DesignParseError> {
    normalize_design_json(&mut value);
    let nodes = match value {
        serde_json::Value::Array(_) => serde_json::from_value::<Vec<PenNode>>(value)
            .map_err(|e| DesignParseError::Deserialize(e.to_string()))?,
        serde_json::Value::Object(_) => vec![serde_json::from_value::<PenNode>(value)
            .map_err(|e| DesignParseError::Deserialize(e.to_string()))?],
        _ => return Err(DesignParseError::NotObjectOrArray),
    };
    if nodes.is_empty() {
        Err(DesignParseError::EmptyNodeArray)
    } else {
        Ok(nodes)
    }
}

fn normalize_design_json(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Array(items) => {
            for item in items {
                normalize_design_json(item);
            }
        }
        serde_json::Value::Object(object) => {
            object.remove("_parent");
            if object
                .get("type")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|ty| ty == "image")
                && matches!(object.get("src"), None | Some(serde_json::Value::Null))
            {
                object.insert("src".into(), serde_json::Value::String(String::new()));
            }
            for child in object.values_mut() {
                normalize_design_json(child);
            }
        }
        _ => {}
    }
}

fn parse_jsonl_nodes(text: &str) -> Result<Vec<PenNode>, DesignParseError> {
    let objects = extract_json_objects(text)?;
    if objects.is_empty() {
        return Err(DesignParseError::NoJsonObjects);
    }

    let mut nodes_by_id = BTreeMap::<String, PenNode>::new();
    let mut children_by_parent = BTreeMap::<String, Vec<String>>::new();
    let mut parents = Vec::<(String, Option<String>)>::new();

    for mut value in objects {
        let parent = value
            .as_object_mut()
            .and_then(|object| object.remove("_parent"))
            .and_then(|value| value.as_str().map(str::to_string));
        normalize_design_json(&mut value);
        let node = serde_json::from_value::<PenNode>(value)
            .map_err(|e| DesignParseError::Deserialize(e.to_string()))?;
        let id = node.id_str().to_string();
        parents.push((id.clone(), parent));
        nodes_by_id.insert(id, node);
    }

    let mut root_ids = Vec::new();
    for (id, parent) in parents {
        if let Some(parent_id) = parent.filter(|parent_id| nodes_by_id.contains_key(parent_id)) {
            children_by_parent.entry(parent_id).or_default().push(id);
        } else {
            root_ids.push(id);
        }
    }

    let mut roots = Vec::new();
    for id in root_ids {
        if let Some(node) = build_jsonl_tree(&id, &mut nodes_by_id, &mut children_by_parent) {
            roots.push(node);
        }
    }
    if roots.is_empty() {
        Err(DesignParseError::EmptyNodeTree)
    } else {
        Ok(roots)
    }
}

fn extract_json_objects(text: &str) -> Result<Vec<serde_json::Value>, DesignParseError> {
    let bytes = text.as_bytes();
    let mut values = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        while i < bytes.len() && bytes[i] != b'{' {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let start = i;
        let mut depth = 0i32;
        let mut in_str = false;
        let mut esc = false;
        while i < bytes.len() {
            let c = bytes[i];
            if in_str {
                if esc {
                    esc = false;
                } else if c == b'\\' {
                    esc = true;
                } else if c == b'"' {
                    in_str = false;
                }
                i += 1;
                continue;
            }
            match c {
                b'"' => in_str = true,
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        let slice = &text[start..=i];
                        let value = serde_json::from_str::<serde_json::Value>(slice)
                            .map_err(|e| DesignParseError::Json(e.to_string()))?;
                        values.push(value);
                        i += 1;
                        break;
                    }
                }
                _ => {}
            }
            i += 1;
        }
        if depth > 0 {
            return Err(DesignParseError::IncompleteJsonObject);
        }
    }
    Ok(values)
}

fn build_jsonl_tree(
    id: &str,
    nodes_by_id: &mut BTreeMap<String, PenNode>,
    children_by_parent: &mut BTreeMap<String, Vec<String>>,
) -> Option<PenNode> {
    let mut node = nodes_by_id.remove(id)?;
    if let Some(child_ids) = children_by_parent.remove(id) {
        if let Some(children) = node.children_mut() {
            for child_id in child_ids {
                if let Some(child) = build_jsonl_tree(&child_id, nodes_by_id, children_by_parent) {
                    children.push(child);
                }
            }
        }
    }
    Some(node)
}

fn collapse_blank_lines(input: &str) -> String {
    let mut out = Vec::new();
    let mut blank = false;
    for line in input.lines().map(str::trim_end) {
        if line.trim().is_empty() {
            if !blank && !out.is_empty() {
                out.push(String::new());
            }
            blank = true;
        } else {
            out.push(line.to_string());
            blank = false;
        }
    }
    out.join("\n")
}

pub(crate) fn place_design_blocks(
    pending: Vec<PendingDesignBlock>,
    x: f32,
    mut y: f32,
    width: f32,
    gap: f32,
    expanded_overrides: &[Option<bool>],
) -> (Vec<DesignBlock>, f32) {
    const BODY_TOP_GAP: f32 = 4.0;
    const BODY_PAD_Y: f32 = 8.0;
    const BODY_LINE_H: f32 = 13.0;
    const APPLY_H: f32 = 32.0;
    const MAX_BODY_LINES: usize = 12;
    let mut blocks = Vec::new();
    for (index, pending) in pending.into_iter().enumerate() {
        let expanded = expanded_overrides
            .get(index)
            .copied()
            .flatten()
            .unwrap_or(pending.streaming);
        let mut code_lines = Vec::new();
        if expanded {
            code_lines.extend(
                pending
                    .code
                    .lines()
                    .take(MAX_BODY_LINES)
                    .map(str::to_string),
            );
            if pending.code.lines().count() > MAX_BODY_LINES {
                code_lines.push("…".to_string());
            }
        }
        let body_content_h = if code_lines.is_empty() {
            0.0
        } else {
            BODY_PAD_Y * 2.0 + BODY_LINE_H * code_lines.len() as f32
        };
        let body_h = if body_content_h > 0.0 {
            BODY_TOP_GAP + body_content_h
        } else {
            0.0
        };
        let apply = if expanded && !pending.streaming && !pending.applied && body_content_h > 0.0 {
            Some(Rect::xywh(
                x,
                y + DESIGN_BLOCK_H + BODY_TOP_GAP + body_content_h,
                width,
                APPLY_H,
            ))
        } else {
            None
        };
        let apply_h = apply.map(|rect| rect.size.y).unwrap_or(0.0);
        let rect = Rect::xywh(x, y, width, DESIGN_BLOCK_H + body_h + apply_h);
        let header = Rect::xywh(x, y, width, DESIGN_BLOCK_H);
        let copy = Rect::xywh(x + width - 48.0, y + 6.0, 20.0, 20.0);
        let body = Rect::xywh(x, y + DESIGN_BLOCK_H + BODY_TOP_GAP, width, body_content_h);
        blocks.push(DesignBlock {
            rect,
            header,
            copy,
            body,
            apply,
            expanded,
            element_count: pending.element_count,
            label: pending.label,
            streaming: pending.streaming,
            applied: pending.applied,
            code: pending.code,
            code_lines,
        });
        y += DESIGN_BLOCK_H + body_h + apply_h + gap;
    }
    (blocks, y)
}

/// Paint one design block. `copy_visible` reveals the per-block copy icon on
/// hover; it is a paint-time flag resolved by the caller against the live
/// design-hover, deliberately kept out of the cached layout.
pub(crate) fn paint_design_block(
    cx: &mut PaintCx<'_>,
    theme: &Theme,
    block: &DesignBlock,
    copy_visible: bool,
) {
    let fill = if block.expanded {
        theme.muted.with_alpha(0.40)
    } else {
        theme.background.with_alpha(0.40)
    };
    let border = theme
        .border
        .with_alpha(if block.expanded { 0.60 } else { 0.30 });
    cx.backend.fill_round_rect(block.rect, 6.0, fill);
    cx.backend.stroke_round_rect(block.rect, 6.0, border, 1.0);

    let icon_bg = theme.primary.with_alpha(0.10);
    let icon_bg_rect = Rect::xywh(
        block.rect.origin.x + DESIGN_ICON_LEFT,
        block.rect.origin.y + (DESIGN_BLOCK_H - DESIGN_ICON_BG) / 2.0,
        DESIGN_ICON_BG,
        DESIGN_ICON_BG,
    );
    cx.backend.fill_round_rect(icon_bg_rect, 8.0, icon_bg);
    draw_icon(
        cx.backend,
        if block.applied {
            Icon::Check
        } else {
            Icon::Wand2
        },
        Point2D::new(
            icon_bg_rect.origin.x + (DESIGN_ICON_BG - DESIGN_ICON_SIZE) / 2.0,
            icon_bg_rect.origin.y + (DESIGN_ICON_BG - DESIGN_ICON_SIZE) / 2.0,
        ),
        DESIGN_ICON_SIZE,
        theme.primary,
        1.5,
    );
    let color = if block.streaming {
        theme.muted_foreground
    } else {
        theme.foreground.with_alpha(0.90)
    };
    let label_x = icon_bg_rect.origin.x + DESIGN_ICON_BG + DESIGN_ICON_GAP;
    cx.backend.save();
    cx.backend.clip_rect(Rect::xywh(
        label_x,
        block.header.origin.y,
        (block.header.origin.x + block.header.size.x - label_x - 56.0).max(1.0),
        block.header.size.y,
    ));
    let layout = TextLayout::single_run(
        &block.label,
        "system-ui",
        11.0,
        (color).to_jian(),
        Point2D::new(0.0, 0.0),
    );
    cx.backend
        .draw_text(&layout, Point2D::new(label_x, block.header.origin.y + 20.0));
    cx.backend.restore();

    if copy_visible {
        let copy_color = theme.muted_foreground.with_alpha(0.30);
        draw_icon(
            cx.backend,
            Icon::Copy,
            Point2D::new(block.copy.origin.x + 4.0, block.copy.origin.y + 4.0),
            12.0,
            copy_color,
            1.5,
        );
    }

    let chevron_color = theme.muted_foreground.with_alpha(0.30);
    draw_icon(
        cx.backend,
        if block.expanded {
            Icon::ChevronUp
        } else {
            Icon::ChevronDown
        },
        Point2D::new(
            block.header.origin.x + block.header.size.x - 20.0,
            block.header.origin.y + 10.0,
        ),
        12.0,
        chevron_color,
        1.5,
    );

    if block.body.size.y > 0.0 {
        let mut body_fill = theme.card;
        body_fill.a *= 0.5;
        let mut body_border = theme.border;
        body_border.a *= 0.3;
        cx.backend.fill_round_rect(block.body, 6.0, body_fill);
        cx.backend
            .stroke_round_rect(block.body, 6.0, body_border, 1.0);
        cx.backend.save();
        cx.backend.clip_rect(block.body);
        let mut baseline = block.body.origin.y + 18.0;
        for line in &block.code_lines {
            let layout = TextLayout::single_run(
                line,
                "monospace",
                9.0,
                (theme.muted_foreground).to_jian(),
                Point2D::new(0.0, 0.0),
            );
            cx.backend
                .draw_text(&layout, Point2D::new(block.body.origin.x + 10.0, baseline));
            baseline += 13.0;
        }
        cx.backend.restore();
    }

    if let Some(apply) = block.apply {
        let mut apply_fill = theme.muted;
        apply_fill.a *= 0.35;
        let mut apply_border = theme.border;
        apply_border.a *= 0.3;
        cx.backend.fill_round_rect(apply, 6.0, apply_fill);
        cx.backend.stroke_round_rect(apply, 6.0, apply_border, 1.0);
        let layout = TextLayout::single_run(
            "Apply to Canvas",
            "system-ui",
            10.0,
            (theme.muted_foreground).to_jian(),
            Point2D::new(0.0, 0.0),
        );
        let text_x = apply.origin.x + (apply.size.x - 84.0).max(0.0) / 2.0;
        cx.backend
            .draw_text(&layout, Point2D::new(text_x, apply.origin.y + 20.0));
    }
}

#[cfg(test)]
mod applied_design_label_tests {
    use super::applied_design_block_label;
    use op_editor_core::Locale;

    #[test]
    fn russian_uses_the_correct_counted_noun_form() {
        assert_eq!(
            applied_design_block_label(Locale::Ru, 1),
            "Изменено · 1 элемент"
        );
        assert_eq!(
            applied_design_block_label(Locale::Ru, 2),
            "Изменено · 2 элемента"
        );
        assert_eq!(
            applied_design_block_label(Locale::Ru, 5),
            "Изменено · 5 элементов"
        );
        assert_eq!(
            applied_design_block_label(Locale::Ru, 21),
            "Изменено · 21 элемент"
        );
        assert_eq!(
            applied_design_block_label(Locale::Ru, 22),
            "Изменено · 22 элемента"
        );
    }
}
