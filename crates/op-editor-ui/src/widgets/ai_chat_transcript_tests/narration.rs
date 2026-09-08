//! Image thumbnails and markdown narration rendering.
//!
//! Split out of `ai_chat_transcript_tests.rs` to keep that file under the
//! 800-line cap.

use super::*;

#[test]
fn user_message_images_get_one_thumbnail_rect_each() {
    let mut m = ChatMessage::user("look");
    for i in 0..3 {
        m.images.push(op_editor_core::ChatImage {
            id: i,
            name: format!("{i}.png"),
            media_type: "image/png".into(),
            data: vec![1],
        });
    }
    let items = build_transcript(
        std::slice::from_ref(&m),
        body(),
        op_editor_core::Locale::EnUs,
    );
    assert_eq!(items[0].images.len(), 3, "one thumbnail rect per image");
    // Thumbnails do not overlap.
    let (a, b) = (items[0].images[0], items[0].images[1]);
    assert!(a.origin.x != b.origin.x || a.origin.y != b.origin.y);
}

#[test]
fn narration_preserves_markdown_delimiters_without_guessing_line_breaks() {
    use super::normalize_narration_markdown;
    let raw = "**Project:**\n**Batch 1****Batch 2**\nThe design features:**Header**\n`value:**`";
    let out = normalize_narration_markdown(raw);
    assert_eq!(out, raw);
}

#[test]
fn bold_labels_ending_in_colons_render_after_normalization() {
    use crate::widgets::ai_chat_transcript_richtext::{layout_rich, SpanStyle};
    let text = normalize_narration_markdown(
        "**Project:**\n- A calm Thai elder-care dashboard\n- **Two clearly separated screens**\n**Mood:**\n- calm — soft mint\n**โครงการ:** ดูแลผู้สูงอายุ",
    );
    for budget in [24, 60] {
        let lines = layout_rich(&text, budget);
        for label in ["Project:", "Mood:", "โครงการ:"] {
            assert!(lines
                .iter()
                .flat_map(|line| &line.spans)
                .any(|span| span.style == SpanStyle::Strong && span.text == label));
        }
        assert!(lines
            .iter()
            .flat_map(|line| &line.spans)
            .all(|span| !span.text.contains("**")));
        assert_eq!(lines.iter().filter(|line| line.bullet).count(), 3);
    }
}

#[test]
fn narration_renders_as_typed_markdown_not_a_grey_wall() {
    use crate::widgets::ai_chat_transcript_richtext::{layout_rich, SpanStyle};

    let lines = layout_rich(
        "**Layout** — a page (`#F4F5F7`) with a card\n- 5-tab bottom navigation",
        60,
    );
    let first = &lines[0];
    assert_eq!(first.spans[0].text, "Layout");
    assert_eq!(first.spans[0].style, SpanStyle::Strong, "the label is bold");
    assert!(
        first
            .spans
            .iter()
            .any(|s| s.style == SpanStyle::Code && s.text == "#F4F5F7"),
        "the hex reads as code: {:?}",
        first.spans
    );
    let bullet = lines.iter().find(|l| l.bullet).expect("a bullet line");
    assert!(bullet.inset > 0.0, "bullet text hangs off the dot");
    assert!(
        bullet.spans[0].text.starts_with("5-tab"),
        "the dash marker is consumed by the bullet, not printed: {:?}",
        bullet.spans
    );
}

#[test]
fn an_unclosed_marker_stays_literal() {
    use crate::widgets::ai_chat_transcript_richtext::{parse_spans, SpanStyle};
    let spans = parse_spans("a ** dangling marker");
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].style, SpanStyle::Body);
    assert_eq!(spans[0].text, "a ** dangling marker");
}
