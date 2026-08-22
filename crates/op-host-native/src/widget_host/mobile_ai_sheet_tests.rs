//! Mobile AI-sheet regressions: the keyboard-shrunk sheet's internal
//! layout (empty-state suggestions vs. bottom-anchored composer) and the
//! header chevron closing the sheet coherently (sheet gone, input blurred,
//! IME released) instead of leaving a desktop-style minimized bar floating
//! inside an empty sheet.

use super::*;
use op_editor_ui::widgets::{AIChatHit, AIChatPlaceholder};

const COMPACT_VIEWPORT: (f32, f32) = (390.0, 844.0);

fn compact_ai_host() -> WidgetHostNative {
    let mut host = touch_host(EditorSizeClass::Compact);
    host.toggle_mobile_sheet(MobileSheetKind::Ai);
    assert_eq!(
        host.editor_state().editor_ui.mobile_sheet,
        Some(MobileSheetKind::Ai)
    );
    assert!(host.editor_state().chat.focused);
    host
}

/// The chevron's hit target: `expanded_header_title_rect` spans
/// `x ∈ [rect.x + 16, rect.x + 34]`, `y ∈ [rect.y + 5, rect.y + 31]`.
/// The point is validated against the panel's own hit-test before use.
fn chevron_point(rect: Rect) -> Point2D {
    Point2D::new(rect.origin.x + 25.0, rect.origin.y + 18.0)
}

#[test]
fn keyboard_shrunk_ai_sheet_keeps_empty_state_above_composer() {
    let (width, height) = COMPACT_VIEWPORT;
    for keyboard in [0.0_f32, 300.0, 500.0] {
        let mut host = compact_ai_host();
        if keyboard > 0.0 {
            assert!(host.set_keyboard_occlusion(keyboard));
        }
        let rect = host.ai_chat_rect(width, height).expect("AI sheet rect");
        let panel = AIChatPlaceholder::from_editor(host.editor_state());
        let region = panel.empty_state_region(rect);
        let input = panel.input_rect(rect);
        let region_bottom = region.origin.y + region.size.y;
        assert!(region.size.y >= 0.0);
        assert!(
            region_bottom <= input.origin.y,
            "empty-state region bottom ({region_bottom}) must stay above the \
             composer top ({}) at keyboard height {keyboard}",
            input.origin.y
        );
        // Probe the composer band: no suggestion pill may claim a point
        // there — pills that no longer fit are dropped from hit-testing
        // exactly like they are dropped from paint.
        let cx = rect.origin.x + rect.size.x / 2.0;
        for y in [
            region_bottom + 2.0,
            input.origin.y + 8.0,
            rect.origin.y + rect.size.y - 20.0,
        ] {
            let point = Point2D::new(cx, y);
            assert!(
                !matches!(panel.hit_test(rect, point), Some(AIChatHit::Example { .. })),
                "no suggestion pill may own a composer-band point (y {y}) at \
                 keyboard height {keyboard}"
            );
            assert_eq!(panel.example_hover_at(rect, point), None);
        }
    }
}

#[test]
fn ai_sheet_chevron_with_keyboard_up_closes_sheet_and_blurs_input() {
    let (width, height) = COMPACT_VIEWPORT;
    let mut host = compact_ai_host();
    assert!(host.set_keyboard_occlusion(300.0));
    let rect = host.ai_chat_rect(width, height).expect("AI sheet rect");
    let point = chevron_point(rect);
    // Guard the geometry assumption: this point is the collapse chevron.
    {
        let panel = AIChatPlaceholder::from_editor(host.editor_state());
        assert_eq!(
            panel.hit_test(rect, point),
            Some(AIChatHit::ToggleCollapse),
            "test point must land on the header chevron"
        );
    }

    assert!(host.apply_press(point.x, point.y, width, height));

    let state = host.editor_state();
    assert_eq!(
        state.editor_ui.mobile_sheet, None,
        "the chevron closes the mobile AI sheet"
    );
    assert!(
        !state.chat.focused,
        "the chat input blurs so the shell ends the IME session"
    );
    assert_eq!(
        host.ai_chat_rect(width, height),
        None,
        "no leftover sheet rect — the canvas chrome is back"
    );
}

#[test]
fn toggling_ai_sheet_closed_blurs_the_focused_chat_input() {
    let mut host = compact_ai_host();
    assert!(host.editor_state().chat.focused);

    // Close via the same toggle the dock / More entry drives.
    host.toggle_mobile_sheet(MobileSheetKind::Ai);

    let state = host.editor_state();
    assert_eq!(state.editor_ui.mobile_sheet, None);
    assert!(
        !state.chat.focused,
        "closing the AI sheet must not leave the chat input owning the keyboard"
    );
}
