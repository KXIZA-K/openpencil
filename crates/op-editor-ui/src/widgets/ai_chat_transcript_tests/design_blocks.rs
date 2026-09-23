//! Design-JSON code fences: folded cards, expansion and code previews.
//!
//! Split out of `ai_chat_transcript_tests.rs` to keep that file under the
//! 800-line cap.

use super::*;

#[test]
fn historical_applied_edit_protocol_is_folded_without_changing_history() {
    let code = r#"[{"op":"update","id":"title","data":{"content":"สวัสดี"}},{"op":"delete","id":"sidebar"}]"#;
    for payload in [code.to_string(), format!("```json\n{code}\n```")] {
        let original = format!("<step title=\"Checking guidelines\">Working</step>\n{payload}\n<!-- APPLIED -->");
        let mut message = ChatMessage::assistant(&original);
        let items = build_transcript(std::slice::from_ref(&message), body(), op_editor_core::Locale::EnUs);
        assert_eq!(message.content, original);
        assert_eq!(items[0].design_blocks.len(), 1);
        let block = &items[0].design_blocks[0];
        assert!(block.applied && !block.expanded && block.apply.is_none());
        assert_eq!(block.element_count, 2);
        assert_eq!(block.code, code);
        message.design_block_expanded_overrides = vec![Some(true)];
        let expanded = build_transcript(&[message], body(), op_editor_core::Locale::EnUs);
        assert!(expanded[0].design_blocks[0].expanded);
        assert!(expanded[0].design_blocks[0].apply.is_none());
        assert!(expanded[0].design_blocks[0].code_lines.join("\n").contains("สวัสดี"));
    }
}

#[test]
fn historical_edit_folding_does_not_hide_unacknowledged_or_ordinary_json() {
    for content in [
        r#"[{"op":"update","id":"title","data":{"content":"Example"}}]"#,
        "[{\"op\":\"query\",\"id\":\"example\"}]\n<!-- APPLIED -->",
        "[{\"type\":\"audit\",\"id\":\"example\"}]\n<!-- APPLIED -->",
        "[{\"op\":\"update\",\"id\":\"title\",\"data\":\n<!-- APPLIED -->",
    ] {
        let message = ChatMessage::assistant(content);
        let items = build_transcript(&[message], body(), op_editor_core::Locale::EnUs);
        assert!(items[0].design_blocks.is_empty(), "{content}");
        assert!(items[0].bubble.as_ref().is_some_and(|b| !b.lines.is_empty()));
    }
    let message = ChatMessage::user("[{\"op\":\"delete\",\"id\":\"example\"}]\n<!-- APPLIED -->");
    let items = build_transcript(&[message], body(), op_editor_core::Locale::EnUs);
    assert!(items[0].design_blocks.is_empty());
}

#[test]
fn assistant_design_json_code_fence_renders_compact_design_block() {
    let message = ChatMessage::assistant(
        r#"Here is the design:
```json
[{"id":"frame-1","type":"Frame"},{"id":"text-1","type":"Text"}]
```
Applied to canvas."#,
    );

    let items = build_transcript(
        std::slice::from_ref(&message),
        body(),
        op_editor_core::Locale::EnUs,
    );

    assert_eq!(items[0].design_blocks.len(), 1);
    assert_eq!(items[0].design_blocks[0].element_count, 2);
    assert_eq!(items[0].design_blocks[0].label, "2 design elements");
    let visible_text = items[0].bubble.as_ref().unwrap().lines.join("\n");
    assert!(visible_text.contains("Here is the design:"));
    assert!(visible_text.contains("Applied to canvas."));
    assert!(!visible_text.contains(r#""type":"Frame""#));
}

#[test]
fn assistant_applied_modify_json_without_ids_renders_localized_folded_card() {
    let mut message = ChatMessage::assistant(
        r#"```json
[{"type":"text","name":"Caption","content":"Updated"}]
```
<!-- APPLIED -->"#,
    );

    let items = build_transcript(
        std::slice::from_ref(&message),
        body(),
        op_editor_core::Locale::ZhCn,
    );
    let block = &items[0].design_blocks[0];

    assert_eq!(items[0].design_blocks.len(), 1);
    assert_eq!(block.element_count, 1);
    assert_eq!(block.label, "已修改 · 1 元素");
    assert!(block.apply.is_none(), "applied cards must not offer Apply");
    assert!(!block.expanded, "applied cards are folded by default");

    message.design_block_expanded_overrides = vec![Some(true)];
    let expanded = build_transcript(
        std::slice::from_ref(&message),
        body(),
        op_editor_core::Locale::ZhCn,
    );
    let block = &expanded[0].design_blocks[0];
    assert!(block.expanded, "applied cards remain expandable");
    assert!(block.body.size.y > 0.0);
    assert!(
        block.apply.is_none(),
        "expanded applied cards still omit Apply"
    );
    assert!(block.code_lines.iter().any(|line| line.contains("Caption")));
}

#[test]
fn assistant_plain_json_with_type_is_not_a_design_block() {
    let message = ChatMessage::assistant(
        r#"```json
{"id":"event-1","type":"audit","payload":{"ok":true}}
```"#,
    );

    let items = build_transcript(
        std::slice::from_ref(&message),
        body(),
        op_editor_core::Locale::EnUs,
    );

    assert!(items[0].design_blocks.is_empty());
    let visible_text = items[0].bubble.as_ref().unwrap().lines.join("\n");
    assert!(visible_text.contains(r#""type":"audit""#));
}

#[test]
fn expanded_design_json_block_reserves_body_and_surfaces_code_like_ts() {
    let mut message = ChatMessage::assistant(
        r#"```json
[{"id":"frame-1","type":"Frame"}]
```"#,
    );
    message.design_block_expanded_overrides = vec![Some(true)];

    let items = build_transcript(
        std::slice::from_ref(&message),
        body(),
        op_editor_core::Locale::EnUs,
    );
    let block = &items[0].design_blocks[0];

    assert!(block.expanded);
    assert!(block.rect.size.y > 32.0);
    assert!(block.body.size.y > 0.0);
    assert!(
        block.apply.is_some(),
        "generation cards keep the Apply button"
    );
    assert!(
        (block.body.origin.y - (block.header.origin.y + block.header.size.y + 4.0)).abs() < 1e-4,
        "TS expanded design cards put the JSON preview in a separate mt-1 body box"
    );
    assert!(block
        .code_lines
        .iter()
        .any(|line| line.contains(r#""type":"Frame""#)));
}

#[test]
fn paint_design_json_block_shows_expand_affordance_like_ts() {
    let message = ChatMessage::assistant(
        r#"```json
[{"id":"frame-1","type":"Frame"}]
```"#,
    );
    let body = body();
    let mut backend = TranscriptPaintBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };

    paint_transcript(
        &mut cx,
        &crate::Theme::dark(),
        body,
        &[message],
        0,
        op_editor_core::Locale::EnUs,
    );

    assert!(
        backend
            .svg_strokes
            .iter()
            .any(|(point, size)| *size == 12.0 && point.x >= body.origin.x + body.size.x - 26.0),
        "TS design JSON blocks carry a right-side chevron affordance"
    );
}

#[test]
fn paint_expanded_design_json_block_draws_code_preview_like_ts() {
    let mut message = ChatMessage::assistant(
        r#"```json
[{"id":"frame-1","type":"Frame"}]
```"#,
    );
    message.design_block_expanded_overrides = vec![Some(true)];
    let mut backend = TranscriptPaintBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };

    paint_transcript(
        &mut cx,
        &crate::Theme::dark(),
        body(),
        &[message],
        0,
        op_editor_core::Locale::EnUs,
    );

    assert!(
        backend
            .texts
            .iter()
            .any(|line| line.contains(r#""type":"Frame""#)),
        "expanded TS design cards show a JSON preview"
    );
}

#[test]
fn paint_expanded_design_json_block_draws_separate_body_box_like_ts() {
    let mut message = ChatMessage::assistant(
        r#"```json
[{"id":"frame-1","type":"Frame"}]
```"#,
    );
    message.design_block_expanded_overrides = vec![Some(true)];
    let body = body();
    let expected = build_transcript(
        std::slice::from_ref(&message),
        body,
        op_editor_core::Locale::EnUs,
    )[0]
    .design_blocks[0]
        .body;
    let mut backend = TranscriptPaintBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };

    paint_transcript(
        &mut cx,
        &crate::Theme::dark(),
        body,
        &[message],
        0,
        op_editor_core::Locale::EnUs,
    );

    assert!(
        backend.round_rects.iter().any(|(rect, radius)| {
            (rect.origin.x - expected.origin.x).abs() < 1e-4
                && (rect.origin.y - expected.origin.y).abs() < 1e-4
                && (rect.size.x - expected.size.x).abs() < 1e-4
                && (rect.size.y - expected.size.y).abs() < 1e-4
                && (*radius - 6.0).abs() < 1e-4
        }),
        "expanded TS design cards paint the JSON preview in its own rounded body box"
    );
}

#[test]
fn streaming_design_json_shows_no_design_card() {
    let mut message = ChatMessage::assistant_streaming();
    message.content = r#"```json
[{"id":"frame-1","type":"Frame"}]"#
        .into();

    let items = build_transcript(
        std::slice::from_ref(&message),
        body(),
        op_editor_core::Locale::EnUs,
    );

    // Streaming design cards are suppressed — no "Generating design..." card
    // while the turn streams (the design JSON is not shown as a bubble either).
    assert!(items[0].design_blocks.is_empty());
    assert!(items[0].bubble.is_none());
}
