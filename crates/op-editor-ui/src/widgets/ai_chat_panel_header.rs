//! Header conversation selector for [`super::ai_chat_panel::AIChatPlaceholder`].
//!
//! Extracted from `ai_chat_panel.rs` (which was approaching the 800-line cap)
//! to keep that file under the limit — same pattern as `ai_chat_panel_footer.rs`.
//!
//! ## Layout (left→right inside the header)
//!
//! ```text
//! [chevron][ active conversation · N chats ▾ ][maximize][+]
//! ```
//!
//! Tab-row zone: `x = panel_left + PAD + CHEVRON_W + PILL_GAP` to
//!               `x = right_edge - NEW_CHAT_D - MAXIMIZE_GAP - MAXIMIZE_W - PILL_RIGHT_GAP`
//!
//! Clicking the active conversation opens a scrollable list below the header.
//! This keeps titles readable even when a shared room contains many threads.
//!
//! ## Hit geometry (mirrored in `ai_chat_panel_hit.rs`)
//!
//! Selector/picker geometry is shared by paint and hit-test so the dropdown
//! rows stay pixel-perfect while scrolled.

use super::ai_chat_panel::{ChatTabInfo, HEADER_HEIGHT, PAD};
use crate::theme::Theme;
use crate::widgets::ai_chat_panel_controls::chat_neutral_feedback_color;
use crate::widgets::icons::{draw_icon, Icon};
use crate::widgets::text_metrics;
use crate::widgets::PaintCx;
use crate::{Color, Point2D, Rect, TextLayout};

// ── Layout constants ─────────────────────────────────────────────────────────

/// Width of the collapse chevron icon.
pub(crate) const CHEVRON_W: f32 = 18.0;
/// Gap between chevron and the first tab.
pub(crate) const PILL_GAP: f32 = 6.0;
/// Gap between the last tab and the maximize button.
pub(crate) const PILL_RIGHT_GAP: f32 = 8.0;
/// Diameter of the new-chat "+" circle.
pub(crate) const NEW_CHAT_D: f32 = 24.0;
/// Gap between the "+" circle and the maximize icon.
pub(crate) const MAXIMIZE_GAP: f32 = 6.0;
/// Width of the maximize / minimize icon.
pub(crate) const MAXIMIZE_W: f32 = 18.0;
/// Height of the active-tab pill.
const PILL_H: f32 = 26.0;
/// Corner radius of the active-tab pill.
const PILL_RADIUS: f32 = 8.0;
/// Horizontal text padding inside a tab on each side.
const TAB_PAD_X: f32 = 8.0;
/// Font size for tab title text.
pub(crate) const TAB_FONT_SIZE: f32 = 12.0;
/// Diameter of the running-spinner inside the active tab.
const SPINNER_D: f32 = 10.0;

/// Height of the conversation picker title strip.
pub(crate) const THREAD_PICKER_HEADER_H: f32 = 32.0;
/// Height of one conversation row.
pub(crate) const THREAD_PICKER_ROW_H: f32 = 36.0;
/// Maximum height of the conversation picker.
pub(crate) const THREAD_PICKER_MAX_H: f32 = 260.0;
const THREAD_PICKER_GAP: f32 = 4.0;
const THREAD_PICKER_PAD_Y: f32 = 6.0;

fn display_thread_title(tabs: &[ChatTabInfo], index: usize) -> String {
    let Some(tab) = tabs.get(index) else {
        return String::new();
    };
    let duplicate_count = tabs.iter().filter(|item| item.title == tab.title).count();
    if duplicate_count <= 1 {
        return tab.title.clone();
    }
    let ordinal = tabs[..=index]
        .iter()
        .filter(|item| item.title == tab.title)
        .count();
    format!("{} · {}", tab.title, ordinal)
}

// ── Computed tab-row extents ─────────────────────────────────────────────────

/// Left x-coordinate where the tab row starts (just right of the chevron).
pub(crate) fn tab_row_left(rect: Rect) -> f32 {
    rect.origin.x + PAD + CHEVRON_W + PILL_GAP
}

/// Right x-coordinate where the tab row ends (just left of the maximize icon).
pub(crate) fn tab_row_right(rect: Rect) -> f32 {
    let right_edge = rect.origin.x + rect.size.x - PAD;
    right_edge - NEW_CHAT_D - MAXIMIZE_GAP - MAXIMIZE_W - PILL_RIGHT_GAP
}

/// Available width for all tabs combined.
pub(crate) fn tab_row_width(rect: Rect) -> f32 {
    (tab_row_right(rect) - tab_row_left(rect)).max(0.0)
}

/// Single, readable active-thread control replacing the squeezed tab strip.
pub(crate) fn thread_selector_rect(rect: Rect) -> Rect {
    Rect {
        origin: Point2D::new(
            tab_row_left(rect),
            rect.origin.y + (HEADER_HEIGHT - PILL_H) / 2.0,
        ),
        size: Point2D::new(tab_row_width(rect), PILL_H),
    }
}

pub(crate) fn thread_picker_rect(rect: Rect, tab_count: usize) -> Rect {
    let selector = thread_selector_rect(rect);
    let content_h =
        THREAD_PICKER_HEADER_H + THREAD_PICKER_PAD_Y * 2.0 + tab_count as f32 * THREAD_PICKER_ROW_H;
    let available_h = (rect.size.y - HEADER_HEIGHT - THREAD_PICKER_GAP - 8.0).max(0.0);
    Rect {
        origin: Point2D::new(
            selector.origin.x,
            rect.origin.y + HEADER_HEIGHT + THREAD_PICKER_GAP,
        ),
        size: Point2D::new(
            selector.size.x,
            content_h.min(THREAD_PICKER_MAX_H).min(available_h),
        ),
    }
}

pub(crate) fn thread_picker_max_scroll(rect: Rect, tab_count: usize) -> f32 {
    let picker = thread_picker_rect(rect, tab_count);
    let view_h = (picker.size.y - THREAD_PICKER_HEADER_H - THREAD_PICKER_PAD_Y * 2.0).max(0.0);
    (tab_count as f32 * THREAD_PICKER_ROW_H - view_h).max(0.0)
}

pub(crate) fn thread_picker_row_at(
    rect: Rect,
    tab_count: usize,
    scroll: f32,
    point: Point2D,
) -> Option<usize> {
    let picker = thread_picker_rect(rect, tab_count);
    if !picker.contains(point) {
        return None;
    }
    let rows_top = picker.origin.y + THREAD_PICKER_HEADER_H + THREAD_PICKER_PAD_Y;
    let rows_bottom = picker.origin.y + picker.size.y - THREAD_PICKER_PAD_Y;
    if point.y < rows_top || point.y > rows_bottom {
        return None;
    }
    let idx = ((point.y - rows_top + scroll) / THREAD_PICKER_ROW_H).floor() as usize;
    (idx < tab_count).then_some(idx)
}

// ── Tooltip geometry ─────────────────────────────────────────────────────────

/// Height of the "New Chat ⌘T" tooltip box.
pub(crate) const TOOLTIP_H: f32 = 28.0;
/// Width of the "New Chat ⌘T" tooltip box.
pub(crate) const TOOLTIP_W: f32 = 100.0;

/// Rect of the tooltip that appears below-left of the new-chat "+" circle
/// when it is hovered.
pub(crate) fn new_chat_tooltip_rect(rect: Rect) -> Rect {
    let right_edge = rect.origin.x + rect.size.x - PAD;
    let new_chat_x = right_edge - NEW_CHAT_D;
    // Bottom of the "+" circle, but at least at the header bottom so the
    // tooltip is always fully below the header row (required even when
    // NEW_CHAT_D is small enough that the circle's bottom sits above HEADER_HEIGHT).
    let circle_bottom = rect.origin.y + (HEADER_HEIGHT + NEW_CHAT_D) / 2.0;
    let new_chat_bottom = circle_bottom.max(rect.origin.y + HEADER_HEIGHT);
    Rect {
        origin: Point2D::new(new_chat_x + NEW_CHAT_D - TOOLTIP_W, new_chat_bottom + 4.0),
        size: Point2D::new(TOOLTIP_W, TOOLTIP_H),
    }
}

// ── Paint ─────────────────────────────────────────────────────────────────────

/// Paint one legible active conversation selector in the header. The complete
/// thread list lives in a dropdown instead of competing for horizontal space.
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_thread_selector(
    cx: &mut PaintCx<'_>,
    theme: &Theme,
    rect: Rect,
    tabs: &[ChatTabInfo],
    active_index: usize,
    hovered: bool,
    pressed: bool,
    open: bool,
    is_running: bool,
) {
    if tabs.get(active_index).is_none() {
        return;
    }
    let selector = thread_selector_rect(rect);
    if selector.size.x <= 0.0 {
        return;
    }
    let fill = if pressed {
        chat_neutral_feedback_color(theme, true)
    } else if hovered || open {
        chat_neutral_feedback_color(theme, false)
    } else {
        theme.secondary
    };
    cx.backend.fill_round_rect(selector, PILL_RADIUS, fill);
    cx.backend
        .stroke_round_rect(selector, PILL_RADIUS, theme.border, 1.0);

    let count_label = if tabs.len() == 1 {
        "1 chat".to_string()
    } else {
        format!("{} chats", tabs.len())
    };
    let count_font = 10.0;
    let count_text_w = text_metrics::measure_chrome(cx.backend, &count_label, count_font);
    let count_w = count_text_w + 12.0;
    let count_rect = Rect::xywh(
        selector.origin.x + selector.size.x - count_w - 24.0,
        selector.origin.y + 5.0,
        count_w,
        16.0,
    );
    cx.backend
        .fill_round_rect(count_rect, 8.0, theme.muted.with_alpha(0.8));
    let count_layout = TextLayout::single_run(
        &count_label,
        "system-ui",
        count_font,
        theme.muted_foreground.to_jian(),
        Point2D::new(0.0, 0.0),
    );
    cx.backend.draw_text(
        &count_layout,
        Point2D::new(count_rect.origin.x + 6.0, count_rect.origin.y + 11.5),
    );

    let spinner_w = if is_running { SPINNER_D + 7.0 } else { 0.0 };
    let title_max_w =
        (count_rect.origin.x - selector.origin.x - TAB_PAD_X - spinner_w - 6.0).max(0.0);
    let active_title = display_thread_title(tabs, active_index);
    let title = crate::util::ellipsize_to_width(&active_title, title_max_w, |s| {
        text_metrics::measure_chrome(cx.backend, s, TAB_FONT_SIZE)
    });
    let title_layout = TextLayout::single_run(
        &title,
        "system-ui",
        TAB_FONT_SIZE,
        theme.foreground.to_jian(),
        Point2D::new(0.0, 0.0),
    );
    cx.backend.draw_text(
        &title_layout,
        Point2D::new(
            selector.origin.x + TAB_PAD_X + spinner_w,
            selector.origin.y + PILL_H / 2.0 + TAB_FONT_SIZE * 0.35,
        ),
    );

    if is_running {
        let spinner_cx = selector.origin.x + TAB_PAD_X + SPINNER_D / 2.0;
        let spinner_cy = selector.origin.y + PILL_H / 2.0;
        cx.backend.stroke_oval(
            Rect::xywh(
                spinner_cx - SPINNER_D / 2.0,
                spinner_cy - SPINNER_D / 2.0,
                SPINNER_D,
                SPINNER_D,
            ),
            theme.primary.with_alpha(0.35),
            1.0,
        );
    }
    draw_icon(
        cx.backend,
        Icon::ChevronDown,
        Point2D::new(
            selector.origin.x + selector.size.x - 18.0,
            selector.origin.y + 7.0,
        ),
        12.0,
        theme.muted_foreground,
        1.4,
    );
}

/// Paint the scrollable conversation list below the active selector.
pub(crate) fn paint_thread_picker(
    cx: &mut PaintCx<'_>,
    theme: &Theme,
    rect: Rect,
    tabs: &[ChatTabInfo],
    active_index: usize,
    hover: Option<usize>,
    scroll: f32,
) {
    if tabs.is_empty() {
        return;
    }
    let picker = thread_picker_rect(rect, tabs.len());
    if picker.size.x <= 0.0 || picker.size.y <= 0.0 {
        return;
    }
    cx.backend.fill_round_rect(picker, 10.0, theme.card);
    cx.backend
        .stroke_round_rect(picker, 10.0, theme.border, 1.0);

    let heading = "Conversations";
    let heading_layout = TextLayout::single_run(
        heading,
        "system-ui",
        11.0,
        theme.muted_foreground.to_jian(),
        Point2D::new(0.0, 0.0),
    );
    cx.backend.draw_text(
        &heading_layout,
        Point2D::new(picker.origin.x + 12.0, picker.origin.y + 20.0),
    );
    let divider_y = picker.origin.y + THREAD_PICKER_HEADER_H - 0.5;
    cx.backend.fill_rect(
        Rect::xywh(picker.origin.x, divider_y, picker.size.x, 1.0),
        theme.border,
    );

    let list_rect = Rect::xywh(
        picker.origin.x,
        picker.origin.y + THREAD_PICKER_HEADER_H,
        picker.size.x,
        (picker.size.y - THREAD_PICKER_HEADER_H).max(0.0),
    );
    let rows_top = list_rect.origin.y + THREAD_PICKER_PAD_Y;
    cx.backend.save();
    cx.backend.clip_rect(list_rect);
    cx.backend.translate(Point2D::new(0.0, -scroll));
    for (idx, _tab) in tabs.iter().enumerate() {
        let y = rows_top + idx as f32 * THREAD_PICKER_ROW_H;
        let row = Rect::xywh(
            picker.origin.x + 4.0,
            y + 1.0,
            picker.size.x - 8.0,
            THREAD_PICKER_ROW_H - 2.0,
        );
        if idx == active_index {
            cx.backend.fill_round_rect(row, 7.0, theme.muted);
        } else if hover == Some(idx) {
            cx.backend
                .fill_round_rect(row, 7.0, chat_neutral_feedback_color(theme, false));
        }
        if idx == active_index {
            draw_icon(
                cx.backend,
                Icon::Check,
                Point2D::new(row.origin.x + 8.0, row.origin.y + 10.0),
                14.0,
                theme.foreground,
                1.6,
            );
        }
        let title_max_w = (row.size.x - 42.0).max(0.0);
        let display_title = display_thread_title(tabs, idx);
        let title = crate::util::ellipsize_to_width(&display_title, title_max_w, |s| {
            text_metrics::measure_chrome(cx.backend, s, 12.0)
        });
        let label = TextLayout::single_run(
            &title,
            "system-ui",
            12.0,
            if idx == active_index {
                theme.foreground.to_jian()
            } else {
                theme.muted_foreground.to_jian()
            },
            Point2D::new(0.0, 0.0),
        );
        cx.backend.draw_text(
            &label,
            Point2D::new(row.origin.x + 30.0, row.origin.y + 21.0),
        );
    }
    cx.backend.restore();

    let content_h = tabs.len() as f32 * THREAD_PICKER_ROW_H + THREAD_PICKER_PAD_Y * 2.0;
    let view_h = list_rect.size.y;
    let track_h = (view_h - 8.0).max(0.0);
    if let Some(thumb) =
        (jian_core::scroll::ScrollState { offset: scroll }).thumb(track_h, content_h, view_h, 24.0)
    {
        cx.backend.fill_round_rect(
            Rect::xywh(
                picker.origin.x + picker.size.x - 6.0,
                list_rect.origin.y + 4.0 + thumb.offset,
                3.0,
                thumb.len,
            ),
            1.5,
            theme.muted_foreground,
        );
    }
}

/// Paint the "New Chat ⌘T" dark tooltip below-left of the "+" button.
/// Appears only when the "+" button is hovered.
pub(crate) fn paint_new_chat_tooltip(cx: &mut PaintCx<'_>, theme: &Theme, rect: Rect) {
    let tooltip = new_chat_tooltip_rect(rect);
    const TOOLTIP_RADIUS: f32 = 6.0;

    // Dark tooltip background (dark even in light theme — matches standard tooltip style).
    let bg = Color {
        r: 0.12,
        g: 0.12,
        b: 0.14,
        a: 0.97,
    };
    cx.backend.fill_round_rect(tooltip, TOOLTIP_RADIUS, bg);

    // "New Chat" label on the left.
    let label = "New Chat";
    let font_sz = 11.0;
    let label_layout = TextLayout::single_run(
        label,
        "system-ui",
        font_sz,
        Color::WHITE.to_jian(),
        Point2D::new(0.0, 0.0),
    );
    let label_x = tooltip.origin.x + 8.0;
    let label_baseline_y = tooltip.origin.y + TOOLTIP_H / 2.0 + font_sz * 0.35;
    cx.backend
        .draw_text(&label_layout, Point2D::new(label_x, label_baseline_y));

    // Keycap chips: ⌘ and T, right-aligned inside the tooltip.
    let chip_h: f32 = 16.0;
    let chip_radius = 3.0;
    let chip_font = 10.0;
    let chip_bg = Color {
        r: 0.28,
        g: 0.28,
        b: 0.32,
        a: 1.0,
    };
    let chip_text_color = Color {
        r: 0.85,
        g: 0.85,
        b: 0.88,
        a: 1.0,
    };
    let chip_y = tooltip.origin.y + (TOOLTIP_H - chip_h) / 2.0;
    let pad_x = 5.0;
    let gap = 3.0;

    let cmd_sym = "⌘";
    let t_sym = "T";
    let cmd_text_w = text_metrics::measure_chrome(cx.backend, cmd_sym, chip_font);
    let t_text_w = text_metrics::measure_chrome(cx.backend, t_sym, chip_font);
    let cmd_w = cmd_text_w + pad_x * 2.0;
    let t_w = t_text_w + pad_x * 2.0;
    let chips_total = cmd_w + gap + t_w;
    let chips_right = tooltip.origin.x + tooltip.size.x - 6.0;
    let cmd_x = chips_right - chips_total;
    let t_x = cmd_x + cmd_w + gap;

    // ⌘ chip.
    let cmd_rect = Rect::xywh(cmd_x, chip_y, cmd_w, chip_h);
    cx.backend.fill_round_rect(cmd_rect, chip_radius, chip_bg);
    let cmd_layout = TextLayout::single_run(
        cmd_sym,
        "system-ui",
        chip_font,
        chip_text_color.to_jian(),
        Point2D::new(0.0, 0.0),
    );
    cx.backend.draw_text(
        &cmd_layout,
        Point2D::new(cmd_x + pad_x, chip_y + chip_h / 2.0 + chip_font * 0.35),
    );

    // T chip.
    let t_rect = Rect::xywh(t_x, chip_y, t_w, chip_h);
    cx.backend.fill_round_rect(t_rect, chip_radius, chip_bg);
    let t_layout = TextLayout::single_run(
        t_sym,
        "system-ui",
        chip_font,
        chip_text_color.to_jian(),
        Point2D::new(0.0, 0.0),
    );
    cx.backend.draw_text(
        &t_layout,
        Point2D::new(t_x + pad_x, chip_y + chip_h / 2.0 + chip_font * 0.35),
    );

    // Suppress unused-variable warning for `theme` — it is kept as a param
    // for future styling.
    let _ = theme;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::ai_chat_panel::AI_CHAT_HEIGHT;
    use crate::widgets::ai_chat_panel::AI_CHAT_WIDTH;

    fn panel_rect() -> Rect {
        Rect::xywh(0.0, 0.0, AI_CHAT_WIDTH, AI_CHAT_HEIGHT)
    }

    #[test]
    fn selector_uses_the_full_header_zone() {
        let rect = panel_rect();
        let selector = thread_selector_rect(rect);
        assert_eq!(selector.origin.x, tab_row_left(rect));
        assert_eq!(selector.size.x, tab_row_width(rect));
        assert_eq!(selector.size.y, PILL_H);
    }

    #[test]
    fn picker_rows_hit_correctly_before_and_after_scroll() {
        let rect = panel_rect();
        let picker = thread_picker_rect(rect, 12);
        let first_y = picker.origin.y + THREAD_PICKER_HEADER_H + THREAD_PICKER_PAD_Y + 4.0;
        let point = Point2D::new(picker.origin.x + 20.0, first_y);
        assert_eq!(thread_picker_row_at(rect, 12, 0.0, point), Some(0));
        assert_eq!(
            thread_picker_row_at(rect, 12, THREAD_PICKER_ROW_H * 3.0, point),
            Some(3)
        );
        assert!(thread_picker_max_scroll(rect, 12) > 0.0);
    }

    #[test]
    fn duplicate_thread_titles_receive_stable_ordinals() {
        let tabs = vec![
            ChatTabInfo {
                title: "Team Chat".into(),
            },
            ChatTabInfo {
                title: "New Chat".into(),
            },
            ChatTabInfo {
                title: "New Chat".into(),
            },
        ];
        assert_eq!(display_thread_title(&tabs, 0), "Team Chat");
        assert_eq!(display_thread_title(&tabs, 1), "New Chat · 1");
        assert_eq!(display_thread_title(&tabs, 2), "New Chat · 2");
    }

    #[test]
    fn new_chat_tooltip_rect_is_below_header_and_within_panel() {
        let rect = panel_rect();
        let tip = new_chat_tooltip_rect(rect);
        // The tooltip must appear below the header row, not just below the
        // panel top edge — "below header" means y >= rect.origin.y + HEADER_HEIGHT.
        assert!(
            tip.origin.y >= rect.origin.y + HEADER_HEIGHT,
            "tooltip must be below the header (y={} < rect.y={} + HEADER_HEIGHT={})",
            tip.origin.y,
            rect.origin.y,
            HEADER_HEIGHT,
        );
        assert!(
            tip.origin.x + tip.size.x <= rect.origin.x + rect.size.x + 2.0,
            "tooltip must not exceed panel right edge"
        );
        assert!(tip.size.x > 0.0 && tip.size.y > 0.0);
    }
}
