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

#[test]
fn headings_emphasis_and_nested_strong_have_distinct_styles() {
    use crate::widgets::ai_chat_transcript_richtext::{layout_rich, SpanStyle};
    let lines = layout_rich("## โครงการ\n\n*emphasis* and ***both***", 60);
    assert_eq!(lines[0].spans[0].text, "โครงการ");
    assert_eq!(lines[0].spans[0].style, SpanStyle::Heading);
    assert!(lines
        .iter()
        .flat_map(|l| &l.spans)
        .any(|s| s.text == "emphasis" && s.style == SpanStyle::Emphasis));
    assert!(lines
        .iter()
        .flat_map(|l| &l.spans)
        .any(|s| s.text == "both" && s.style == SpanStyle::StrongEmphasis));
}

#[test]
fn fenced_code_preserves_indentation_blank_lines_and_literal_markers() {
    use crate::widgets::ai_chat_transcript_richtext::{layout_rich, SpanStyle};
    let lines = layout_rich("````ts\n  const label = \"**not bold**\";\n\n```\n````", 60);
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0].spans[0].text, "  const label = \"**not bold**\";");
    assert!(lines[1].spans.is_empty());
    assert_eq!(lines[2].spans[0].text, "```");
    assert!(lines.iter().all(|l| l.code_block_width > 0.0));
    assert!(lines
        .iter()
        .flat_map(|l| &l.spans)
        .all(|s| s.style == SpanStyle::Code));
}

#[test]
fn streaming_unclosed_fence_is_code_until_the_end() {
    use crate::widgets::ai_chat_transcript_richtext::{layout_rich, SpanStyle};
    let lines = layout_rich("```rust\n  let x = 1;", 60);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].spans[0].text, "  let x = 1;");
    assert_eq!(lines[0].spans[0].style, SpanStyle::Code);
}

#[test]
fn ordered_lists_quotes_escapes_and_html_remain_readable_without_execution() {
    use crate::widgets::ai_chat_transcript_richtext::layout_rich;
    let lines = layout_rich(
        "3. Third\n4. Fourth\n\n> Quoted\n\n\\*literal\\* <script>alert(1)</script>",
        60,
    );
    let text: String = lines
        .iter()
        .flat_map(|l| &l.spans)
        .map(|s| s.text.as_str())
        .collect();
    assert!(text.contains("3. Third"));
    assert!(text.contains("4. Fourth"));
    assert!(text.contains("*literal* <script>alert(1)</script>"));
    assert!(lines.iter().any(|l| l.quote));
}
