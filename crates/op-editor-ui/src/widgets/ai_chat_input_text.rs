//! Chat input text geometry: wrapping, the scrolled viewport, hit-testing,
//! caret placement, and the text-area paint.
//!
//! Every one of those reads the SAME wrap through [`input_text_view`].
//! Paint hands it the live backend, the interaction paths hand it the
//! measure-only stub they have always used; within each path the wrap that
//! positions the block is the wrap that positions the caret, so the two can
//! not drift against each other by half a line.

use crate::theme::Theme;
// `INPUT_AREA_HEIGHT` — the single-line height of the text area, before any
// growth. Owned by the panel so its layout constants stay in one table.
use crate::widgets::ai_chat_panel::INPUT_AREA_HEIGHT;
use crate::widgets::text_input_backend::BaselineAdjustingBackend;
use crate::widgets::{PaintCx, text_metrics};
use crate::{Color, Point2D, Rect, RenderBackend, TextLayout};
use jian_core::text_input::{TextInputState, prev_char_boundary};
use jian_widgets::Tokens;
use jian_widgets::components::text_area::{TextArea, TextLine};
use op_editor_core::chat::ChatState;

/// Family the jian `TextArea` draws its runs in — measurement must name it.
const INPUT_FONT_FAMILY: &str = "Inter";
/// jian `TextArea::LINE_HEIGHT_MULT`. The input's per-line growth and its
/// scroll arithmetic must use the same advance the component paints with,
/// otherwise a scrolled block lands a fraction of a line off per row.
const INPUT_LINE_HEIGHT_MULT: f32 = 1.35;

pub(crate) const INPUT_FONT: f32 = 14.0;
pub(crate) const INPUT_LINE_H: f32 = INPUT_FONT * INPUT_LINE_HEIGHT_MULT;
pub(crate) const INPUT_TEXT_X_PAD: f32 = 8.0;
pub(crate) const INPUT_BASELINE_ASCENT: f32 = 14.0;
/// Hard ceiling on how tall the input grows before it starts scrolling.
pub(crate) const INPUT_MAX_LINES: usize = 6;
/// Share of the panel the grown input may claim. A user who drags the panel
/// down to [`AI_CHAT_MIN_HEIGHT`] still gets a readable transcript instead of
/// an input box that swallows it.
///
/// [`AI_CHAT_MIN_HEIGHT`]: crate::widgets::ai_chat_panel::AI_CHAT_MIN_HEIGHT
const INPUT_HEIGHT_BUDGET_RATIO: f32 = 0.4;
const TEXT_AREA_PAD_Y: f32 = 4.0;

/// Rows the input is allowed to grow to inside a panel `panel_h` tall.
pub(crate) fn max_input_lines(panel_h: f32) -> usize {
    let budget = panel_h * INPUT_HEIGHT_BUDGET_RATIO - INPUT_AREA_HEIGHT;
    let extra = (budget / INPUT_LINE_H).floor().max(0.0) as usize;
    (1 + extra).clamp(1, INPUT_MAX_LINES)
}

/// Rows of text the box shows for `text` at `input_rect_width`, capped by
/// what fits in a panel `panel_h` tall.
pub(crate) fn visible_input_line_count(text: &str, input_rect_width: f32, panel_h: f32) -> usize {
    let cap = max_input_lines(panel_h);
    if text.is_empty() {
        return 1;
    }
    let mut backend = MeasureOnlyBackend;
    let text_width = (input_rect_width - INPUT_TEXT_X_PAD * 2.0).max(0.0);
    TextArea::layout_lines(&mut backend, text, INPUT_FONT, text_width)
        .len()
        .clamp(1, cap)
}

/// Height of the text area showing `rows` rows.
pub(crate) fn input_area_height(rows: usize) -> f32 {
    INPUT_AREA_HEIGHT + (rows.saturating_sub(1) as f32) * INPUT_LINE_H
}

/// Inverse of [`input_area_height`] — how many rows a laid-out area shows.
fn rows_in_area(input_area_h: f32) -> usize {
    let extra = ((input_area_h - INPUT_AREA_HEIGHT) / INPUT_LINE_H).round();
    1 + extra.max(0.0) as usize
}

/// The one wrap + viewport every chat-input path resolves against.
pub(crate) struct InputTextView {
    pub(crate) lines: Vec<TextLine>,
    /// Rows on screen. Fewer than `lines.len()` means the box scrolls.
    pub(crate) visible_rows: usize,
    /// Resolved offset in px from the first wrapped line.
    pub(crate) scroll: f32,
    /// Largest meaningful [`scroll`]; `0.0` when nothing overflows.
    ///
    /// [`scroll`]: InputTextView::scroll
    pub(crate) max_scroll: f32,
    /// Rect the WHOLE wrapped block is laid into, already shifted up by
    /// `scroll`. Handed to `TextArea` for paint and for hit-testing alike.
    pub(crate) text_rect: Rect,
    /// Band the block is clipped to — the rows actually on screen.
    pub(crate) clip_rect: Rect,
}

impl InputTextView {
    /// Visual line the caret sits on.
    pub(crate) fn caret_line(&self, input: &TextInputState) -> usize {
        let caret = prev_char_boundary(input.text(), input.caret());
        caret_line_index(&self.lines, caret).unwrap_or(self.lines.len().saturating_sub(1))
    }
}

/// Resolve the wrap and the scrolled viewport for `chat`'s draft.
///
/// `input_rect` is the text area's own rect (origin at its top-left,
/// `input_area_h` tall).
pub(crate) fn input_text_view(
    backend: &mut dyn RenderBackend,
    chat: &ChatState,
    input_rect: Rect,
    input_area_h: f32,
) -> InputTextView {
    let content_w = (input_rect.size.x - INPUT_TEXT_X_PAD * 2.0).max(0.0);
    let lines = TextArea::layout_lines(backend, chat.input.text(), INPUT_FONT, content_w);
    let visible_rows = rows_in_area(input_area_h).clamp(1, lines.len().max(1));
    let band_h = visible_rows as f32 * INPUT_LINE_H;
    let max_scroll = (lines.len().saturating_sub(visible_rows) as f32) * INPUT_LINE_H;
    let scroll = resolved_scroll(chat, &lines, visible_rows, max_scroll);

    let band_top = input_rect.origin.y + ((input_area_h - band_h) / 2.0).max(0.0);
    let text_rect = Rect {
        origin: Point2D::new(input_rect.origin.x, band_top - TEXT_AREA_PAD_Y - scroll),
        // TextArea adds its own clip. After translating the whole block for
        // scrolling, a viewport-sized inner clip would hide the final rows
        // (or the entire draft). Only clip_rect owns the visible viewport.
        size: Point2D::new(
            input_rect.size.x,
            input_rect
                .size
                .y
                .max(lines.len() as f32 * INPUT_LINE_H + TEXT_AREA_PAD_Y * 2.0),
        ),
    };
    // Nothing overflows: keep the historical full-rect clip so a tall
    // single-row area can't shave a descender. Once rows are hidden the
    // clip has to hug the band, or the scrolled-away neighbours bleed into
    // the box's generous vertical padding.
    let clip_rect = if max_scroll <= 0.0 {
        input_rect
    } else {
        Rect {
            origin: Point2D::new(input_rect.origin.x, band_top),
            size: Point2D::new(input_rect.size.x, band_h),
        }
    };

    InputTextView {
        lines,
        visible_rows,
        scroll,
        max_scroll,
        text_rect,
        clip_rect,
    }
}

/// The stored wheel offset, overridden by the caret line whenever the caret
/// has moved since that offset was taken.
fn resolved_scroll(
    chat: &ChatState,
    lines: &[TextLine],
    visible_rows: usize,
    max_scroll: f32,
) -> f32 {
    let stored = chat.input_scroll.clamp(0.0, max_scroll);
    if max_scroll <= 0.0 || !chat.input_scroll_follows_caret() {
        return stored;
    }
    let caret = prev_char_boundary(chat.input.text(), chat.input.caret());
    let line = caret_line_index(lines, caret).unwrap_or(lines.len().saturating_sub(1));
    let line_top = line as f32 * INPUT_LINE_H;
    let line_bottom = line_top + INPUT_LINE_H;
    stored
        .min(line_top)
        .max(line_bottom - visible_rows as f32 * INPUT_LINE_H)
        .clamp(0.0, max_scroll)
}

/// The view resolved through the measure-only backend — the interaction
/// paths' entry point (paint has a live backend and calls
/// [`input_text_view`] directly).
pub(crate) fn measured_input_text_view(
    chat: &ChatState,
    input_rect: Rect,
    input_area_h: f32,
) -> InputTextView {
    let mut backend = MeasureOnlyBackend;
    input_text_view(&mut backend, chat, input_rect, input_area_h)
}

pub(crate) fn input_text_offset_at(
    chat: &ChatState,
    input_rect: Rect,
    point: Point2D,
) -> Option<usize> {
    if !(input_rect).contains(point) {
        return None;
    }
    let text = chat.input.text();
    if text.is_empty() {
        return Some(0);
    }
    let view = measured_input_text_view(chat, input_rect, input_rect.size.y);
    let mut backend = MeasureOnlyBackend;
    let area = TextArea {
        state: &chat.input,
        placeholder: "",
        focused: true,
        font_size: INPUT_FONT,
        now_ms: 0,
        pad_x: INPUT_TEXT_X_PAD,
        // The view already windowed the block; let `TextArea` address every
        // wrapped line so the shifted rect alone decides what a click hits.
        max_visible_lines: 0,
    };
    Some(area.byte_offset_at(
        &mut backend,
        view.text_rect,
        point,
        &tokens_from_theme(&Theme::dark()),
    ))
}

/// Byte offset the caret lands on when it steps one visual line up / down.
///
/// Returns `None` only when there is nothing to move within. At the first
/// row `up` collapses to the text start and at the last row `down` collapses
/// to the text end — the same edge behaviour a browser textarea has, and
/// what keeps the key from ever falling through to canvas nudge.
pub(crate) fn vertical_caret_offset(
    chat: &ChatState,
    input_rect: Rect,
    input_area_h: f32,
    down: bool,
) -> Option<usize> {
    let text = chat.input.text();
    let view = measured_input_text_view(chat, input_rect, input_area_h);
    let mut backend = MeasureOnlyBackend;
    let caret = prev_char_boundary(text, chat.input.caret());
    let line_i = caret_line_index(&view.lines, caret)?;
    let target = match if down {
        line_i.checked_add(1)
    } else {
        line_i.checked_sub(1)
    } {
        // Off the top: collapse to the text start.
        None => return Some(0),
        Some(target) => target,
    };
    let Some(target_line) = view.lines.get(target) else {
        // Off the bottom: collapse to the text end.
        return Some(text.len());
    };
    let line = &view.lines[line_i];
    let rel = prev_char_boundary(&line.text, caret.saturating_sub(line.start));
    let caret_x = text_metrics::measure_in_family(
        &mut backend,
        &line.text[..rel],
        INPUT_FONT,
        INPUT_FONT_FAMILY,
    );
    Some(offset_at_x(&mut backend, target_line, caret_x))
}

/// Byte offset in `line` nearest to `x` px from the line's left edge.
fn offset_at_x(backend: &mut dyn RenderBackend, line: &TextLine, x: f32) -> usize {
    if x <= 0.0 || line.text.is_empty() {
        return line.start;
    }
    let mut cursor_x = 0.0;
    for (byte, ch) in line.text.char_indices() {
        let mut buf = [0; 4];
        let glyph = ch.encode_utf8(&mut buf);
        let width = text_metrics::measure_in_family(backend, glyph, INPUT_FONT, INPUT_FONT_FAMILY);
        if x < cursor_x + width / 2.0 {
            return line.start + byte;
        }
        cursor_x += width;
    }
    line.end
}

pub(crate) fn input_caret_rect(chat: &ChatState, input_rect: Rect, input_area_h: f32) -> Rect {
    let view = measured_input_text_view(chat, input_rect, input_area_h);
    let mut backend = MeasureOnlyBackend;
    let caret = prev_char_boundary(chat.input.text(), chat.input.caret());
    let line_i = view.caret_line(&chat.input);
    let line = &view.lines[line_i.min(view.lines.len().saturating_sub(1))];
    let rel = prev_char_boundary(&line.text, caret.saturating_sub(line.start));
    let x = text_metrics::measure_in_family(
        &mut backend,
        &line.text[..rel],
        INPUT_FONT,
        INPUT_FONT_FAMILY,
    );
    // Clamped into the on-screen band: the platform anchors the IME
    // candidate window here, and a window placed against a scrolled-away
    // line would float outside the panel.
    let band_top = view.text_rect.origin.y + TEXT_AREA_PAD_Y + view.scroll;
    let y = (view.text_rect.origin.y + TEXT_AREA_PAD_Y + line_i as f32 * INPUT_LINE_H).clamp(
        band_top,
        band_top + view.visible_rows.saturating_sub(1) as f32 * INPUT_LINE_H,
    );
    Rect::xywh(
        view.text_rect.origin.x + INPUT_TEXT_X_PAD + x,
        y,
        1.5,
        INPUT_FONT + 3.0,
    )
}

fn caret_line_index(lines: &[TextLine], caret: usize) -> Option<usize> {
    lines.iter().enumerate().position(|(i, line)| {
        if line.start == line.end {
            return caret == line.start;
        }
        let end_is_soft_wrap = lines.get(i + 1).is_some_and(|next| next.start == line.end);
        if end_is_soft_wrap {
            caret >= line.start && caret < line.end
        } else {
            caret >= line.start && caret <= line.end
        }
    })
}

pub(crate) fn paint_input_text_area(
    cx: &mut PaintCx<'_>,
    theme: &Theme,
    state: &ChatState,
    input_rect: Rect,
    input_area_h: f32,
    now_ms: u64,
    placeholder: &str,
) {
    let mut backend = BaselineAdjustingBackend {
        inner: cx.backend,
        baseline_delta_y: INPUT_BASELINE_ASCENT,
    };
    let view = input_text_view(&mut backend, state, input_rect, input_area_h);
    let area = TextArea {
        state: &state.input,
        placeholder,
        focused: state.focused,
        font_size: INPUT_FONT,
        now_ms,
        pad_x: INPUT_TEXT_X_PAD,
        // The view owns the window; `TextArea` paints the whole block into
        // the rect it was already shifted by, and the clip below crops it.
        max_visible_lines: 0,
    };
    backend.save();
    backend.clip_rect(view.clip_rect);
    area.paint(&mut backend, view.text_rect, &tokens_from_theme(theme));
    backend.restore();
}

fn tokens_from_theme(theme: &Theme) -> Tokens {
    crate::widgets::button::tokens_from_theme(theme)
}

pub(crate) struct MeasureOnlyBackend;

impl RenderBackend for MeasureOnlyBackend {
    fn begin_frame(&mut self) {}
    fn end_frame(&mut self) {}
    fn fill_rect(&mut self, _: Rect, _: Color) {}
    fn stroke_rect(&mut self, _: Rect, _: Color, _: f32) {}
    fn draw_text(&mut self, _: &TextLayout, _: Point2D) {}
    fn clip_rect(&mut self, _: Rect) {}
    fn stroke_line(&mut self, _: Point2D, _: Point2D, _: Color, _: f32) {}
    fn fill_round_rect(&mut self, _: Rect, _: f32, _: Color) {}
    fn stroke_round_rect(&mut self, _: Rect, _: f32, _: Color, _: f32) {}
    fn stroke_svg_path(&mut self, _: &str, _: Point2D, _: f32, _: Color, _: f32) {}
    fn save(&mut self) {}
    fn restore(&mut self) {}
    fn translate(&mut self, _: Point2D) {}
    fn resize(&mut self, _: u32, _: u32) {}
    fn dpi_scale(&self) -> f32 {
        1.0
    }
}

#[cfg(test)]
#[path = "ai_chat_input_text_tests.rs"]
mod ai_chat_input_text_tests;
