//! Wrap / viewport / caret regressions for the chat draft input.

use super::*;
use crate::widgets::test_capture_backend::CaptureBackend;

/// A prompt long enough to wrap past the box's row cap at `INPUT_RECT`'s
/// width — the shape of the real report (a pasted multi-line prompt).
const LONG_PROMPT: &str = "line one of the prompt\nline two of the prompt\nline three of the prompt\nline four of the prompt\nline five of the prompt\nline six of the prompt\nline seven of the prompt";

fn chat_with(text: &str) -> ChatState {
    let mut chat = ChatState::default();
    chat.set_input_text(text);
    chat.focused = true;
    chat
}

/// Text area of a 360×520 panel — the default chat panel geometry.
fn input_rect(rows: usize) -> Rect {
    Rect::xywh(20.0, 100.0, 328.0, input_area_height(rows))
}

/// Scroll offsets accumulate a row's worth of f32 rounding, so compare
/// them the way pixels are compared, not the way integers are.
#[track_caller]
fn assert_close(actual: f32, expected: f32, what: &str) {
    assert!(
        (actual - expected).abs() < 0.01,
        "{what}: expected {expected}, got {actual}"
    );
}

#[test]
fn line_height_matches_the_component_that_paints_it() {
    // The growth step and the scroll step are the same advance jian's
    // `TextArea` lays lines out on; a mismatch drifts a fraction of a line
    // per row and shears the last one.
    assert_eq!(INPUT_LINE_H, INPUT_FONT * 1.35);
}

#[test]
fn scrolled_draft_inner_clip_still_covers_visible_rows() {
    let text = (0..40)
        .map(|i| format!("ข้อความทดสอบบรรทัด {i}\n"))
        .collect::<String>();
    let chat = chat_with(&text);
    let rect = input_rect(INPUT_MAX_LINES);
    let view = measured_input_text_view(&chat, rect, rect.size.y);
    assert!(view.scroll > rect.size.y);
    // TextArea clips to text_rect internally, inside our viewport clip.
    // The full block must reach the bottom of the visible band after scrolling.
    assert!(
        view.text_rect.origin.y + view.text_rect.size.y
            >= view.clip_rect.origin.y + view.clip_rect.size.y
    );
}

#[test]
fn input_grows_to_the_cap_then_stops() {
    let panel_h = 520.0;
    let width = 328.0;
    assert_eq!(visible_input_line_count("one line", width, panel_h), 1);
    assert_eq!(visible_input_line_count("a\nb\nc", width, panel_h), 3);
    // Seven authored lines, capped at six.
    assert_eq!(
        visible_input_line_count(LONG_PROMPT, width, panel_h),
        INPUT_MAX_LINES
    );
}

#[test]
fn a_short_panel_caps_the_input_lower_than_the_hard_ceiling() {
    // The transcript has to survive: a panel dragged to its minimum height
    // gets a smaller input than a default-sized one.
    assert_eq!(max_input_lines(520.0), INPUT_MAX_LINES);
    assert!(max_input_lines(250.0) < INPUT_MAX_LINES);
    assert!(max_input_lines(250.0) >= 1);
}

#[test]
fn overflowing_text_scrolls_and_reveals_the_caret_line() {
    let rows = INPUT_MAX_LINES;
    let rect = input_rect(rows);
    let mut chat = chat_with(LONG_PROMPT);

    // Caret at the very end (where `set_input_text` leaves it): the view is
    // scrolled to the bottom so the row being typed on is on screen.
    let view = measured_input_text_view(&chat, rect, rect.size.y);
    assert!(
        view.lines.len() > view.visible_rows,
        "test fixture must overflow: {} lines in {} rows",
        view.lines.len(),
        view.visible_rows
    );
    assert!(view.max_scroll > 0.0);
    assert_close(
        view.scroll,
        view.max_scroll,
        "starts pinned to the caret row",
    );
    let caret_line = view.caret_line(&chat.input);
    assert!(
        caret_line >= view.visible_rows,
        "caret must start off-screen"
    );

    // Move the caret home: the view follows it back to the top.
    chat.input.set_caret(0, 0);
    let view = measured_input_text_view(&chat, rect, rect.size.y);
    assert_eq!(view.scroll, 0.0);
}

#[test]
fn caret_rect_stays_inside_the_visible_band() {
    let rows = INPUT_MAX_LINES;
    let rect = input_rect(rows);
    let mut chat = chat_with(LONG_PROMPT);
    let band = measured_input_text_view(&chat, rect, rect.size.y).clip_rect;

    // Caret at the end of a prompt that overflows.
    let caret = input_caret_rect(&chat, rect, rect.size.y);
    assert!(
        caret.origin.y >= band.origin.y - 0.01
            && caret.origin.y + caret.size.y <= band.origin.y + band.size.y + 0.01,
        "caret {caret:?} escaped the visible band {band:?}"
    );

    // And after a wheel that scrolls the caret's row off screen, the rect
    // the IME anchors to is still inside the box.
    assert!(chat.set_input_scroll(0.0));
    let caret = input_caret_rect(&chat, rect, rect.size.y);
    assert!(
        caret.origin.y >= band.origin.y - 0.01
            && caret.origin.y + caret.size.y <= band.origin.y + band.size.y + 0.01,
        "scrolled-away caret {caret:?} escaped the visible band {band:?}"
    );
}

#[test]
fn wheel_offset_survives_until_the_caret_moves() {
    let rows = INPUT_MAX_LINES;
    let rect = input_rect(rows);
    let mut chat = chat_with(LONG_PROMPT);
    let max = measured_input_text_view(&chat, rect, rect.size.y).max_scroll;

    // A wheel to the top holds, even though the caret sits at the bottom.
    assert!(chat.set_input_scroll(0.0));
    let view = measured_input_text_view(&chat, rect, rect.size.y);
    assert_eq!(view.scroll, 0.0);

    // The moment the caret moves, the caret wins again.
    chat.input.move_left(false, 0);
    let view = measured_input_text_view(&chat, rect, rect.size.y);
    assert_close(view.scroll, max, "the caret row is revealed again");
}

#[test]
fn vertical_caret_walks_visual_rows_and_holds_its_column() {
    let text = "abcdefgh\nijklmnop\nqrstuvwx";
    let rect = input_rect(3);
    let mut chat = chat_with(text);
    // Column 3 of the middle row.
    let middle = text.find("ijkl").expect("fixture") + 3;
    chat.input.set_caret(middle, 0);

    let up = vertical_caret_offset(&chat, rect, rect.size.y, false).expect("up");
    assert_eq!(&text[..up], "abc");

    let last_start = text.find("qrst").expect("fixture");
    let down = vertical_caret_offset(&chat, rect, rect.size.y, true).expect("down");
    assert_eq!(&text[last_start..down], "qrs");
}

#[test]
fn vertical_caret_collapses_at_the_first_and_last_row() {
    let text = "abcdefgh\nijklmnop";
    let rect = input_rect(2);
    let mut chat = chat_with(text);

    chat.input.set_caret(3, 0);
    assert_eq!(
        vertical_caret_offset(&chat, rect, rect.size.y, false),
        Some(0)
    );

    chat.input.set_caret(text.len() - 2, 0);
    assert_eq!(
        vertical_caret_offset(&chat, rect, rect.size.y, true),
        Some(text.len())
    );
}

#[test]
fn vertical_caret_crosses_a_soft_wrap_not_just_a_newline() {
    // No `\n` at all: the rows exist only because the text wrapped.
    let text = "the quick brown fox jumps over the lazy dog and keeps running well past the edge";
    let rect = input_rect(3);
    let mut chat = chat_with(text);
    let view = measured_input_text_view(&chat, rect, rect.size.y);
    assert!(view.lines.len() >= 2, "fixture must soft-wrap");

    let second = view.lines[1].start;
    chat.input.set_caret(second + 2, 0);
    let up = vertical_caret_offset(&chat, rect, rect.size.y, false).expect("up");
    assert!(
        up < second,
        "moving up from row 1 must land on row 0 ({up} vs row start {second})"
    );
}

#[test]
fn focused_input_text_preserves_original_first_baseline() {
    let rect = input_rect(1);
    let chat = chat_with("hello");

    let mut backend = CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    paint_input_text_area(&mut cx, &Theme::light(), &chat, rect, rect.size.y, 0, "");

    let expected_baseline_y =
        rect.origin.y + (rect.size.y - INPUT_LINE_H) / 2.0 + INPUT_BASELINE_ASCENT;
    let (_, first_origin) = backend.texts.first().expect("input text should paint");
    assert!(
        (first_origin.y - expected_baseline_y).abs() < 0.01,
        "expected first text baseline at {expected_baseline_y}, got {}",
        first_origin.y
    );
}

#[test]
fn input_hit_testing_matches_painted_wrapping_for_chinese_text() {
    let text = "设计一个现代的移动端登录页面，包含邮箱输入框";
    let rect = Rect::xywh(20.0, 100.0, 202.0, input_area_height(3));
    let chat = chat_with(text);

    let mut backend = CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    paint_input_text_area(&mut cx, &Theme::light(), &chat, rect, rect.size.y, 0, "");

    assert!(
        backend.texts.len() >= 2,
        "test input should paint across multiple lines: {:?}",
        backend.texts
    );
    let second_painted_origin = backend.texts[1].1;
    let click_near_start_of_second_line = Point2D::new(
        second_painted_origin.x + 1.0,
        second_painted_origin.y - INPUT_BASELINE_ASCENT + 1.0,
    );
    let view = measured_input_text_view(&chat, rect, rect.size.y);
    assert_eq!(
        input_text_offset_at(&chat, rect, click_near_start_of_second_line),
        Some(view.lines[1].start)
    );
}

/// Paint places the caret's row inside the visible band and pushes the
/// scrolled-away rows above it. (`CaptureBackend` ignores `clip_rect`, so
/// this asserts on where each row landed rather than on which ones survived
/// the clip.)
#[test]
fn scrolled_paint_puts_the_caret_row_inside_the_band() {
    let rect = input_rect(INPUT_MAX_LINES);
    let chat = chat_with(LONG_PROMPT);
    let band = measured_input_text_view(&chat, rect, rect.size.y).clip_rect;

    let mut backend = CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    paint_input_text_area(&mut cx, &Theme::light(), &chat, rect, rect.size.y, 0, "");

    let row_top = |needle: &str| {
        backend
            .texts
            .iter()
            .find(|(content, _)| content.contains(needle))
            .map(|(_, origin)| origin.y - INPUT_BASELINE_ASCENT)
            .unwrap_or_else(|| panic!("row {needle:?} never painted: {:?}", backend.texts))
    };
    let band_bottom = band.origin.y + band.size.y;
    assert!(
        row_top("seven") >= band.origin.y - 0.01 && row_top("seven") < band_bottom,
        "the caret's row must sit inside the band (top={} vs {}..{band_bottom})",
        row_top("seven"),
        band.origin.y
    );
    assert!(
        row_top("line one") < band.origin.y,
        "the scrolled-away first row must land above the band (top={} vs {})",
        row_top("line one"),
        band.origin.y
    );
}
