//! The narration's own typography: bold labels, real bullets, code chips.
//!
//! The model writes markdown ("**Layout** — a soft neutral page (`#F4F5F7`)…",
//! "- 5-tab bottom navigation"), and the panel used to flatten all of it to one
//! grey wall of plain text: markers stripped, bullets faked with a "•" glued
//! into the string, code indistinguishable from prose. The reference transcript
//! renders the same content as a typed document — label runs in bold on the
//! foreground colour, body in the muted tone, `code` in a tinted chip, and
//! bullets with a hanging indent so wrapped lines line up under the text rather
//! than under the dot.
//!
//! CommonMark is parsed into text-only widget runs. No HTML is executed and
//! links/images never fetch resources while a transcript is being rendered.

use crate::theme::Theme;
use crate::widgets::PaintCx;
use crate::{Point2D, Rect, TextLayout};

use super::ai_chat_transcript::{BODY_FONT, CHAR_UNIT_PX, LINE_H};
use super::ai_chat_transcript_text::char_display_units;
#[path = "ai_chat_markdown.rs"]
mod markdown;
pub(crate) use markdown::layout_rich;
#[cfg(test)]
pub(crate) use markdown::parse_spans;

/// Left inset of a bullet's text — the dot sits in the gutter and wrapped
/// lines align under the text, not under the dot.
pub(crate) const BULLET_INDENT: f32 = 14.0;
/// Padding around an inline code chip.
const CODE_PAD_X: f32 = 3.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SpanStyle {
    Body,
    Strong,
    Code,
    Emphasis,
    StrongEmphasis,
    Heading,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Span {
    pub text: String,
    pub style: SpanStyle,
}

/// One laid-out line: its spans, already wrapped, with an x inset.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RichLine {
    pub spans: Vec<Span>,
    pub inset: f32,
    /// Paint a bullet dot in this line's gutter (the first line of a bullet).
    pub bullet: bool,
    pub code_block_width: f32,
    pub quote: bool,
}

/// Greedy word wrap that carries the style across the break.
pub(super) fn wrap_spans(spans: &[Span], budget: u32) -> Vec<Vec<Span>> {
    let mut lines: Vec<Vec<Span>> = Vec::new();
    let mut current: Vec<Span> = Vec::new();
    let mut used = 0u32;
    for span in spans {
        let mut pending = String::new();
        for word in split_keeping_spaces(&span.text) {
            let width: u32 = word.chars().map(char_display_units).sum();
            let fits = used + width <= budget || (used == 0 && pending.is_empty());
            if !fits && !word.trim().is_empty() {
                if !pending.is_empty() {
                    current.push(Span {
                        text: std::mem::take(&mut pending),
                        style: span.style,
                    });
                }
                lines.push(std::mem::take(&mut current));
                used = 0;
                pending.push_str(word.trim_start());
                used += word
                    .trim_start()
                    .chars()
                    .map(char_display_units)
                    .sum::<u32>();
            } else {
                pending.push_str(word);
                used += width;
            }
        }
        if !pending.is_empty() {
            current.push(Span {
                text: pending,
                style: span.style,
            });
        }
    }
    if !current.is_empty() || lines.is_empty() {
        lines.push(current);
    }
    lines
}

/// Words with their trailing space attached, so wrapping keeps spacing intact.
fn split_keeping_spaces(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b' ' {
            // Absorb the run of spaces into the preceding word.
            while i < bytes.len() && bytes[i] == b' ' {
                i += 1;
            }
            out.push(&text[start..i]);
            start = i;
        } else {
            i += 1;
        }
    }
    if start < text.len() {
        out.push(&text[start..]);
    }
    out
}

/// Total height of a rich block.
pub(crate) fn rich_height(lines: &[RichLine]) -> f32 {
    lines.len() as f32 * LINE_H
}

/// Paint rich lines from `origin` (top-left of the text block).
pub(crate) fn paint_rich(cx: &mut PaintCx<'_>, theme: &Theme, lines: &[RichLine], origin: Point2D) {
    let mut baseline = origin.y + 11.0;
    for line in lines {
        if line.code_block_width > 0.0 {
            cx.backend.fill_rect(
                Rect::xywh(
                    origin.x + line.inset,
                    baseline - 11.0,
                    line.code_block_width,
                    LINE_H,
                ),
                theme.muted,
            );
        }
        if line.quote {
            cx.backend.fill_rect(
                Rect::xywh(origin.x + 2.0, baseline - 11.0, 2.0, LINE_H),
                theme.muted_foreground,
            );
        }
        if line.bullet {
            let r = 1.6;
            cx.backend.fill_oval(
                Rect::xywh(origin.x + 3.0, baseline - 4.5 - r, r * 2.0, r * 2.0),
                theme.muted_foreground,
            );
        }
        let mut x = origin.x + line.inset;
        for span in &line.spans {
            let (color, weight) = match span.style {
                SpanStyle::Body => (theme.muted_foreground, 400),
                SpanStyle::Strong => (theme.foreground, 700),
                SpanStyle::Code => (theme.foreground, 400),
                SpanStyle::Emphasis => (theme.muted_foreground, 400),
                SpanStyle::StrongEmphasis | SpanStyle::Heading => (theme.foreground, 700),
            };
            let italic = matches!(span.style, SpanStyle::Emphasis | SpanStyle::StrongEmphasis);
            let font_size = if span.style == SpanStyle::Heading {
                BODY_FONT + 2.0
            } else {
                BODY_FONT
            };
            let family = if span.style == SpanStyle::Code {
                "monospace"
            } else {
                "system-ui"
            };
            // Advance by the REAL glyph width, not the wrap estimate: paint
            // used the same 6.6px-per-unit budget the wrapper does, so every
            // span after the first sat at a slightly wrong x — code chips
            // drifted off their words and the sentence read as fragments
            // (user report 2026-07-12). Wrapping stays estimate-based (it must
            // be backend-free and deterministic); only paint measures.
            let measured = cx
                .backend
                .measure_text_family_styled(&span.text, font_size, family, weight, italic);
            let width = if measured > 0.0 {
                measured
            } else {
                span.text.chars().map(char_display_units).sum::<u32>() as f32 * CHAR_UNIT_PX
            };
            if span.style == SpanStyle::Code && line.code_block_width == 0.0 {
                cx.backend.fill_round_rect(
                    Rect::xywh(
                        x - CODE_PAD_X,
                        baseline - 10.0,
                        width + 2.0 * CODE_PAD_X,
                        15.0,
                    ),
                    3.0,
                    theme.muted,
                );
            }
            let layout = TextLayout::single_run(
                &span.text,
                family,
                font_size,
                color.to_jian(),
                Point2D::new(0.0, 0.0),
            )
            .with_font_weight(weight)
            .with_italic(italic);
            cx.backend.draw_text(&layout, Point2D::new(x, baseline));
            x += width;
        }
        baseline += LINE_H;
    }
}

/// Display width of a rich line — the streaming caret rides its end.
pub(crate) fn rich_line_width(line: &RichLine) -> f32 {
    let units: u32 = line
        .spans
        .iter()
        .flat_map(|s| s.text.chars())
        .map(char_display_units)
        .sum();
    line.inset + units as f32 * CHAR_UNIT_PX
}
