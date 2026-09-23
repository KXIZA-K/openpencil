//! Regression tests for folding an inline formatting context into one
//! positioned styled text node (the rich-inline-text overlap fix).

use super::*;
use crate::HtmlImportOptions;
use jian_ops_schema::node::PenNode;

#[test]
fn captured_borders_do_not_add_padding_to_positioned_children() {
    for styles in [
        serde_json::json!({"border":"1px solid rgb(0, 0, 0)","padding":"12px"}),
        serde_json::json!({"border-left-width":"3px","border-left-style":"solid",
            "border-left-color":"rgb(0, 0, 0)","border-top-width":"2px",
            "border-top-style":"solid","border-top-color":"rgb(0, 0, 0)"}),
    ] {
        let json = serde_json::json!({"version":1,"root":{
            "kind":"element","tag":"div","rect":{"x":10,"y":20,"w":200,"h":100},
            "styles":styles,"children":[{
                "kind":"element","tag":"button","rect":{"x":30,"y":40,"w":150,"h":50},
                "styles":{"border":"1px solid rgb(0, 0, 0)"},"children":[{
                    "kind":"text","text":"Label","lines":1,
                    "rect":{"x":75,"y":56,"w":60,"h":18},"styles":{"font-size":"15px"}
                }]
            }]
        }});
        let result = import_snapshot(&json.to_string(), &HtmlImportOptions::default());
        let PenNode::Frame(root) = &result.nodes[0] else {
            panic!("frame")
        };
        let PenNode::Frame(button) = &root.children.as_ref().unwrap()[0] else {
            panic!("button")
        };
        let PenNode::Text(text) = &button.children.as_ref().unwrap()[0] else {
            panic!("text")
        };
        assert!(root.container.padding.is_none());
        assert!(button.container.padding.is_none());
        assert!(button.container.stroke.is_some(), "Keep the painted border");
        assert_eq!((button.base.x, button.base.y), (Some(20.0), Some(20.0)));
        assert_eq!((text.base.x, text.base.y), (Some(45.0), Some(16.0)));
    }
}

#[test]
fn captured_single_line_alignment_is_not_applied_twice() {
    for (lines, expected) in [(1, "left"), (2, "center")] {
        let json = serde_json::json!({
            "version": 1,
            "root": { "kind": "element", "tag": "button",
                "rect": { "x": 100, "y": 0, "w": 180, "h": 40 },
                "children": [{ "kind": "text", "text": "New Incident Report",
                    "rect": { "x": 142, "y": 11, "w": 123, "h": 17 },
                    "lines": lines,
                    "styles": { "font-size": "14px", "text-align": "center" }
                }]
            }
        });
        let result = import_snapshot(&json.to_string(), &HtmlImportOptions::default());
        let PenNode::Frame(root) = &result.nodes[0] else {
            panic!("frame")
        };
        let PenNode::Text(text) = &root.children.as_ref().unwrap()[0] else {
            panic!("text")
        };
        assert_eq!(text.text_align, parse_text_align(expected));
        assert_eq!(text.base.x, Some(42.0));
    }
}

/// The regression this fix targets: a paragraph whose children are all inline
/// (bare text + `<a>` + `<code>`) that wraps across several lines. The
/// extractor now folds it into ONE positioned text node carrying styled
/// `segments`, so the block imports as a single wrapped run instead of one node
/// per inline child stacked at the block origin. Before the fix each wrapped
/// child's rect was the union of its line boxes — a full-width, multi-line box
/// whose top-left was the block's left edge — so consecutive children shared an
/// origin and the text rendered as an overlapping smear.
#[test]
fn folded_inline_block_is_one_styled_text_node_without_overlap() {
    // Two wrapped runs that, under the old per-child capture, carried the
    // SAME rect origin (0, 0) — the exact shape that produced the smear.
    let json = r#"{
      "version": 1,
      "root": {
        "kind": "element", "tag": "p",
        "rect": { "x": 0, "y": 0, "w": 300, "h": 80 },
        "styles": { "font-size": "16px" },
        "children": [
          { "kind": "text", "rect": { "x": 0, "y": 0, "w": 300, "h": 80 },
            "lines": 4,
            "text": "The rendering engine paints onto a Painter surface.",
            "styles": { "font-size": "16px", "color": "rgb(31, 35, 40)" },
            "segments": [
              { "text": "The ", "styles": { "color": "rgb(31, 35, 40)" } },
              { "text": "rendering", "styles": { "color": "rgb(9, 105, 218)",
                "text-decoration-line": "underline" }, "href": "https://example.com/j" },
              { "text": " engine paints onto a ", "styles": { "color": "rgb(31, 35, 40)" } },
              { "text": "Painter", "styles": { "font-family": "ui-monospace",
                "font-size": "13.6px" } },
              { "text": " surface.", "styles": { "color": "rgb(31, 35, 40)" } }
            ] }
        ]
      }
    }"#;
    let result = import_snapshot(json, &HtmlImportOptions::default());
    let PenNode::Frame(root) = &result.nodes[0] else {
        panic!()
    };
    let children = root.children.as_ref().unwrap();
    // ONE text node for the whole inline context — not a node per inline run.
    assert_eq!(
        children.len(),
        1,
        "the block must fold to a single text node"
    );
    let PenNode::Text(text) = &children[0] else {
        panic!("folded inline content must be a text node")
    };
    use jian_ops_schema::node::text::{TextContent, TextGrowth};
    use jian_ops_schema::sizing::SizingBehavior;
    let TextContent::Styled(segments) = &text.content else {
        panic!("mixed inline content must import as styled runs, not plain")
    };
    assert_eq!(segments.len(), 5);
    // The link run keeps its colour, underline, and href.
    let link = &segments[1];
    assert_eq!(link.text, "rendering");
    assert_eq!(link.href.as_deref(), Some("https://example.com/j"));
    assert_eq!(link.fill.as_deref(), Some("#0969da"));
    assert_eq!(link.underline, Some(true));
    // The code run keeps its monospace family and smaller size.
    let code = &segments[3];
    assert_eq!(code.text, "Painter");
    assert_eq!(code.font_family.as_deref(), Some("ui-monospace"));
    assert!(
        code.font_size
            .is_some_and(|size| (size - 13.6).abs() < 0.01)
    );
    // Multi-line → wrapped mode: keep the captured width, grow the height.
    assert!(matches!(text.width, Some(SizingBehavior::Number(w)) if w == 300.0));
    assert_eq!(text.text_growth, Some(TextGrowth::FixedWidth));
}

/// Cross-version: a capture from the currently-installed extension carries no
/// `segments`, so each inline child is still its own text node. Those payloads
/// must keep importing unchanged — the folded shape is additive.
#[test]
fn text_node_without_segments_stays_plain() {
    let json = r#"{
      "version": 1,
      "root": {
        "kind": "element", "tag": "p",
        "rect": { "x": 0, "y": 0, "w": 300, "h": 20 },
        "children": [
          { "kind": "text", "rect": { "x": 0, "y": 0, "w": 120, "h": 20 },
            "text": "plain run", "lines": 1, "styles": {} }
        ]
      }
    }"#;
    let result = import_snapshot(json, &HtmlImportOptions::default());
    let PenNode::Frame(root) = &result.nodes[0] else {
        panic!()
    };
    let PenNode::Text(text) = &root.children.as_ref().unwrap()[0] else {
        panic!("text run")
    };
    use jian_ops_schema::node::text::TextContent;
    assert_eq!(text.content, TextContent::Plain("plain run".to_string()));
}

/// A single unstyled run (a lone `<span>` with no overrides) reduces to plain
/// text — no need to pay for styled content the run does not use.
#[test]
fn single_trivial_segment_reduces_to_plain_text() {
    let json = r#"{
      "version": 1,
      "root": {
        "kind": "element", "tag": "p",
        "rect": { "x": 0, "y": 0, "w": 300, "h": 20 },
        "children": [
          { "kind": "text", "rect": { "x": 0, "y": 0, "w": 120, "h": 20 },
            "text": "just text", "lines": 1, "styles": {},
            "segments": [ { "text": "just text", "styles": {} } ] }
        ]
      }
    }"#;
    let result = import_snapshot(json, &HtmlImportOptions::default());
    let PenNode::Frame(root) = &result.nodes[0] else {
        panic!()
    };
    let PenNode::Text(text) = &root.children.as_ref().unwrap()[0] else {
        panic!("text run")
    };
    use jian_ops_schema::node::text::TextContent;
    assert_eq!(text.content, TextContent::Plain("just text".to_string()));
}

/// The gradient-text idiom (`background-clip: text` + transparent glyphs):
/// the box's gradient must NOT paint as a bar over invisible text — it moves
/// off the box and colours the glyphs (first stop) instead.
#[test]
fn background_clip_text_moves_the_gradient_onto_the_glyphs() {
    let json = r#"{
      "version": 1,
      "root": {
        "kind": "element", "tag": "section",
        "rect": { "x": 0, "y": 0, "w": 800, "h": 200 },
        "children": [
          { "kind": "element", "tag": "h1",
            "rect": { "x": 100, "y": 40, "w": 600, "h": 90 },
            "styles": {
              "background-image": "linear-gradient(135deg, rgb(15, 23, 42), rgb(100, 116, 139))",
              "background-clip": "text"
            },
            "children": [
              { "kind": "text",
                "rect": { "x": 100, "y": 45, "w": 600, "h": 80 },
                "text": "可编辑的产品界面。",
                "lines": 1,
                "styles": { "font-size": "72px", "color": "rgba(0, 0, 0, 0)" } }
            ] }
        ]
      }
    }"#;
    let result = import_snapshot(json, &HtmlImportOptions::default());
    let PenNode::Frame(root) = &result.nodes[0] else {
        panic!()
    };
    let PenNode::Frame(heading) = &root.children.as_ref().unwrap()[0] else {
        panic!("heading frame")
    };
    assert!(
        heading.container.fill.is_none(),
        "the clipped background must not paint as a bar: {:?}",
        heading.container.fill
    );
    let PenNode::Text(text) = &heading.children.as_ref().unwrap()[0] else {
        panic!("text child")
    };
    let fill = text.fill.as_ref().expect("glyphs take the moved colour");
    let jian_ops_schema::style::PenFill::Solid(solid) = &fill[0] else {
        panic!("solid glyph colour")
    };
    assert!(
        !solid.color.starts_with("#000000"),
        "glyphs must not stay transparent black, got {}",
        solid.color
    );
}

/// The Google-footer regression: a one-line run inside a `line-height: 40px`
/// container imports with its leading clamped to the captured glyph box, so
/// paint does not push it ~12px below neighbours that carry a normal
/// line-height.
#[test]
fn a_single_line_run_with_oversized_leading_imports_clamped() {
    let json = r#"{
      "version": 1,
      "root": {
        "kind": "element", "tag": "div",
        "rect": { "x": 0, "y": 0, "w": 400, "h": 40 },
        "children": [
          { "kind": "text",
            "rect": { "x": 14, "y": 12, "w": 116, "h": 15.5 },
            "text": "广州市 中国广东省",
            "lines": 1,
            "styles": { "font-size": "14px", "line-height": "40px" } }
        ]
      }
    }"#;
    let result = import_snapshot(json, &HtmlImportOptions::default());
    let PenNode::Frame(root) = &result.nodes[0] else {
        panic!()
    };
    let PenNode::Text(text) = &root.children.as_ref().unwrap()[0] else {
        panic!("text run")
    };
    let leading = text.line_height.expect("line height kept");
    assert!(
        (leading - 15.5 / 14.0).abs() < 1e-9,
        "leading clamped to the glyph box, got {leading}"
    );
}

#[test]
fn captured_thai_line_boxes_remove_negative_leading_but_keep_multiline_stride() {
    for (lines, height, css, expected) in [
        (1, 54.0, "46.8px", 1.5),
        (1, 54.0, "normal", 1.5),
        (3, 147.6, "46.8px", 1.3),
    ] {
        let json = serde_json::json!({
            "version": 1,
            "root": { "kind": "element", "tag": "div",
                "rect": { "x": 0, "y": 0, "w": 390, "h": 200 },
                "children": [{ "kind": "text", "text": "ดูแลผู้สูงอายุอย่าง",
                    "lines": lines,
                    "rect": { "x": 15, "y": -4, "w": 275, "h": height },
                    "styles": { "font-size": "36px", "line-height": css }
                }]
            }
        });
        let result = import_snapshot(&json.to_string(), &HtmlImportOptions::default());
        let PenNode::Frame(root) = &result.nodes[0] else {
            panic!("root frame")
        };
        let PenNode::Text(text) = &root.children.as_ref().unwrap()[0] else {
            panic!("text run")
        };
        assert_eq!(text.base.y, Some(-4.0));
        assert!((text.line_height.unwrap() - expected).abs() < 1e-9);
    }
}

/// Source-level guards for the inline-fold contract (see
/// `folded_inline_block_is_one_styled_text_node_without_overlap`). The
/// extractor needs a live DOM, so the invariants are pinned where they are
/// written.
#[test]
fn snapshot_extractor_pins_its_inline_fold_contract() {
    // Consecutive inline-flow children of a block-like parent fold into one
    // text node per run instead of a node per inline child — and only under
    // block-like displays, where inline children genuinely flow as text
    // (a flex / grid parent lays them out as items).
    assert!(
        SNAPSHOT_EXTRACTOR_JS.contains("blockLikeDisplay(computed)"),
        "run folding must be gated on a block-like parent display"
    );
    assert!(
        SNAPSHOT_EXTRACTOR_JS.contains("buildInlineRun(element, run, computed)"),
        "consecutive inline children must fold as runs"
    );
    // The folded node is positioned at the run's own box (a range spanning
    // exactly the run's siblings), so every line box is counted — never one
    // union box per child stacked at the block origin.
    assert!(
        SNAPSHOT_EXTRACTOR_JS.contains("range.setStartBefore(nodes[0])")
            && SNAPSHOT_EXTRACTOR_JS.contains("range.setEndAfter(nodes[nodes.length - 1])"),
        "the folded box must come from the run's own range, not per-child rects"
    );
    // Per-run styling (link colour + href, code's monospace family) rides on
    // `segments`, so folding does not flatten the paragraph to one style.
    assert!(
        SNAPSHOT_EXTRACTOR_JS.contains("segments: emitted"),
        "folded inline runs must carry their styled segments"
    );
}
