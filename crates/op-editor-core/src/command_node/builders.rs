//! Leaf / widget node builders for the raw-node command family.
//!
//! `build_leaf_node` resolves an editor `kind` string into a canonical
//! `jian_ops_schema::PenNode`; `build_widget_node` covers the ten
//! form-widget kinds. `WIDGET_KINDS` + `kind_is_valid` are the shared
//! validation surface the pre-validate-then-mutate command paths call
//! before touching the tree. Carved off `command_node.rs` to keep every
//! file under the 800-line cap.

use jian_ops_schema::node::{
    BoolOrExpression, CheckboxNode, ContainerProps, EllipseNode, FrameNode, GroupNode, LineNode,
    NumberInputNode, NumberOrExpression, PathNode, PenNode, PenNodeBase, PolygonNode, ProgressNode,
    RadioGroupNode, RectangleNode, SelectNode, SliderNode, SwitchNode, TabsNode, TextAreaNode,
    TextContent, TextInputNode, TextNode,
};
use jian_ops_schema::sizing::SizingBehavior;

/// Resolve an editor `kind` arg into a canonical-schema leaf node.
/// Accepts the same lowercase strings the read-side tools emit
/// (`frame` / `group` / `rect` / `ellipse` / `polygon` / `line` /
/// `text` / `path`). `None` for an unknown kind.
///
/// `width` / `height` write the variant's literal `SizingBehavior`;
/// `(x, y)` write `base`. Container kinds (`frame` / `group`) start
/// with an empty `children` so a follow-up `MoveNode` can reparent
/// into them.
pub fn build_leaf_node(
    kind: &str,
    id: &str,
    name: &str,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> Option<PenNode> {
    let base = PenNodeBase {
        id: id.to_string(),
        name: Some(name.to_string()),
        x: Some(x as f64),
        y: Some(y as f64),
        ..Default::default()
    };
    let w = SizingBehavior::Number(width.max(0) as f64);
    let h = SizingBehavior::Number(height.max(0) as f64);
    let node = match kind {
        "frame" => PenNode::Frame(FrameNode {
            base,
            container: ContainerProps {
                width: Some(w),
                height: Some(h),
                ..Default::default()
            },
            children: Some(Vec::new()),
            image_search_query: None,
            reusable: None,
            screen: None,
            slot: None,
            state: None,
            bindings: None,
            events: None,
            lifecycle: None,
            semantics: None,
            gestures: None,
            route: None,
            breakpoint: None,
        }),
        "group" => PenNode::Group(GroupNode {
            base,
            container: ContainerProps {
                width: Some(w),
                height: Some(h),
                ..Default::default()
            },
            children: Some(Vec::new()),
            state: None,
            bindings: None,
            events: None,
            lifecycle: None,
            semantics: None,
            gestures: None,
            route: None,
        }),
        "rect" => PenNode::Rectangle(RectangleNode {
            base,
            container: ContainerProps {
                width: Some(w),
                height: Some(h),
                ..Default::default()
            },
            children: None,
            state: None,
            bindings: None,
            events: None,
            lifecycle: None,
            semantics: None,
            gestures: None,
            route: None,
        }),
        "ellipse" => PenNode::Ellipse(EllipseNode {
            base,
            width: Some(w),
            height: Some(h),
            corner_radius: None,
            inner_radius: None,
            start_angle: None,
            sweep_angle: None,
            fill: None,
            stroke: None,
            effects: None,
            state: None,
            bindings: None,
            events: None,
            lifecycle: None,
            semantics: None,
            gestures: None,
            route: None,
            limits: Default::default(),
        }),
        "polygon" => PenNode::Polygon(PolygonNode {
            base,
            polygon_count: 3,
            width: Some(w),
            height: Some(h),
            corner_radius: None,
            fill: None,
            stroke: None,
            effects: None,
            state: None,
            bindings: None,
            events: None,
            lifecycle: None,
            semantics: None,
            gestures: None,
            route: None,
            limits: Default::default(),
        }),
        "line" => PenNode::Line(LineNode {
            base,
            x2: Some((x + width) as f64),
            y2: Some((y + height) as f64),
            stroke: None,
            effects: None,
            state: None,
            bindings: None,
            events: None,
            lifecycle: None,
            semantics: None,
            gestures: None,
            route: None,
        }),
        "text" => PenNode::Text(TextNode {
            text_rasterization: None,
            base,
            width: Some(w),
            height: Some(h),
            content: TextContent::Plain(name.to_string()),
            font_family: None,
            font_size: None,
            font_weight: None,
            font_style: None,
            letter_spacing: None,
            line_height: None,
            text_align: None,
            text_align_vertical: None,
            text_growth: None,
            underline: None,
            strikethrough: None,
            fill: None,
            effects: None,
            state: None,
            bindings: None,
            events: None,
            lifecycle: None,
            semantics: None,
            gestures: None,
            route: None,
            limits: Default::default(),
        }),
        "path" => PenNode::Path(PathNode {
            base,
            icon_id: None,
            d: None,
            anchors: Some(Vec::new()),
            closed: None,
            fill_rule: None,
            mask: None,
            width: Some(w),
            height: Some(h),
            fill: None,
            stroke: None,
            effects: None,
            state: None,
            bindings: None,
            events: None,
            lifecycle: None,
            semantics: None,
            gestures: None,
            route: None,
            limits: Default::default(),
        }),
        // Form-widget kinds (Phase D2) build their own default props.
        _ => return build_widget_node(kind, base, w, h),
    };
    Some(node)
}

/// Resolve a form-widget `kind` into a canonical-schema widget node
/// with sensible default props. `base` already carries the id / name /
/// `(x, y)`; `w` / `h` are the caller's literal `SizingBehavior` (the
/// widget's default box when minted via a tool, or the requested box
/// from an `InsertNode` / MCP call). Returns `None` for an unknown
/// kind so [`build_leaf_node`]'s fall-through can reject it.
///
/// Each struct derives `Default`, so `..Default::default()` zeroes
/// every optional field (fill / stroke / events / bindings / …) and we
/// only set `base` + width / height + the per-kind props the table in
/// the Phase D2 spec calls for.
fn build_widget_node(
    kind: &str,
    base: PenNodeBase,
    w: SizingBehavior,
    h: SizingBehavior,
) -> Option<PenNode> {
    let node = match kind {
        "text_input" => PenNode::TextInput(TextInputNode {
            base,
            width: Some(w),
            height: Some(h),
            placeholder: Some("Enter text".to_string()),
            ..Default::default()
        }),
        "text_area" => PenNode::TextArea(TextAreaNode {
            base,
            width: Some(w),
            height: Some(h),
            placeholder: Some("Enter text".to_string()),
            ..Default::default()
        }),
        "number_input" => PenNode::NumberInput(NumberInputNode {
            base,
            width: Some(w),
            height: Some(h),
            placeholder: Some("0".to_string()),
            ..Default::default()
        }),
        "select" => PenNode::Select(SelectNode {
            base,
            width: Some(w),
            height: Some(h),
            placeholder: Some("Select\u{2026}".to_string()),
            options: Some(Vec::new()),
            ..Default::default()
        }),
        "radio_group" => PenNode::RadioGroup(RadioGroupNode {
            base,
            width: Some(w),
            height: Some(h),
            options: Some(Vec::new()),
            ..Default::default()
        }),
        "switch" => PenNode::Switch(SwitchNode {
            base,
            width: Some(w),
            height: Some(h),
            checked: Some(BoolOrExpression::Bool(false)),
            ..Default::default()
        }),
        "checkbox" => PenNode::Checkbox(CheckboxNode {
            base,
            width: Some(w),
            height: Some(h),
            checked: Some(BoolOrExpression::Bool(false)),
            label: Some("Label".to_string()),
            ..Default::default()
        }),
        "slider" => PenNode::Slider(SliderNode {
            base,
            width: Some(w),
            height: Some(h),
            min: Some(0.0),
            max: Some(100.0),
            step: Some(1.0),
            value: Some(NumberOrExpression::Number(50.0)),
            ..Default::default()
        }),
        "progress" => PenNode::Progress(ProgressNode {
            base,
            width: Some(w),
            height: Some(h),
            value: Some(NumberOrExpression::Number(40.0)),
            max: Some(100.0),
            ..Default::default()
        }),
        "tabs" => PenNode::Tabs(TabsNode {
            base,
            width: Some(w),
            height: Some(h),
            tabs: Some(Vec::new()),
            children: Some(Vec::new()),
            ..Default::default()
        }),
        _ => return None,
    };
    Some(node)
}

/// The ten form-widget kind strings, in spec order. Single source of
/// truth shared by [`kind_is_valid`] and the default-size table.
pub const WIDGET_KINDS: [&str; 10] = [
    "text_input",
    "text_area",
    "number_input",
    "select",
    "radio_group",
    "switch",
    "checkbox",
    "slider",
    "progress",
    "tabs",
];

/// True when `kind` resolves to a buildable leaf node. Used by the
/// `BatchInsert` pre-validation pass. Covers the original shape /
/// container / text kinds plus the ten form-widget kinds.
pub fn kind_is_valid(kind: &str) -> bool {
    matches!(
        kind,
        "frame" | "group" | "rect" | "ellipse" | "polygon" | "line" | "text" | "path"
    ) || WIDGET_KINDS.contains(&kind)
}
