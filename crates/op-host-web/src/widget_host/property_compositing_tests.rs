//! Web twin of native compositing/page property dispatch coverage.

use super::WidgetHost;
use jian_ops_schema::node::base::MaskType;
use jian_ops_schema::style::BlendMode;
use op_editor_core::{CompositingPickerTarget, NodeId, PropertyFocus};
use op_editor_ui::widgets::PropertyPanelAction;

const VIEWPORT_W: f32 = 1200.0;
const VIEWPORT_H: f32 = 800.0;

fn seed(host: &mut WidgetHost, json: &str) {
    let doc = jian_ops_schema::load_str(json)
        .expect("fixture JSON parses")
        .value;
    host.editor_state = op_editor_core::EditorState::from_document(doc);
}

fn commit_focus(host: &mut WidgetHost, focus: PropertyFocus, draft: &str) {
    host.editor_state.ui.property_focus = Some(focus);
    host.editor_state.ui.property_input.set_text(draft);
    host.commit_property_focus_if_any();
}

fn focus_tile_scale(host: &mut WidgetHost, draft: &str) {
    host.editor_state.editor_ui.image_fill_popover_open = true;
    host.editor_state.ui.property_focus = Some(PropertyFocus::ImageTileScale);
    host.editor_state.ui.property_input.set_text(draft);
    host.editor_state.ui.property_input_draft = draft.to_string();
}

fn selected_tile_scale(host: &WidgetHost) -> f32 {
    op_editor_core::first_image_fill_summary(host.editor_state.selected_node().unwrap())
        .unwrap()
        .tile_scale
        .unwrap()
}

#[test]
fn web_compositing_actions_are_noop_safe_single_undo_steps() {
    let mut host = WidgetHost::new();
    seed(
        &mut host,
        r##"{"version":"1.0.0","children":[{"type":"rectangle","id":"r","fill":[{"type":"solid","color":"#123456"}]}]}"##,
    );
    host.editor_state.set_single_selection(NodeId::new("r"));

    host.apply_property_action(PropertyPanelAction::SetNodeBlendMode(Some(
        BlendMode::Multiply,
    )));
    assert_eq!(
        host.editor_state.selected_node_blend_mode(),
        Some(BlendMode::Multiply)
    );
    assert_eq!(host.editor_state.history.past.len(), 1);
    host.apply_property_action(PropertyPanelAction::SetNodeBlendMode(Some(
        BlendMode::Multiply,
    )));
    assert_eq!(host.editor_state.history.past.len(), 1);

    host.apply_property_action(PropertyPanelAction::SetFillBlendMode {
        index: 0,
        mode: Some(BlendMode::Screen),
    });
    assert_eq!(host.editor_state.history.past.len(), 2);
    assert_eq!(
        op_editor_core::fill_blend_mode_at(host.editor_state.selected_node().unwrap(), 0),
        Some(BlendMode::Screen)
    );
    assert!(host.editor_state.undo());
    assert_eq!(
        op_editor_core::fill_blend_mode_at(host.editor_state.selected_node().unwrap(), 0),
        None
    );
}

#[test]
fn web_mask_page_rgba_and_tile_scale_match_native_history() {
    let mut host = WidgetHost::new();
    seed(
        &mut host,
        r#"{"version":"1.0.0","children":[{"type":"path","id":"p","mask":true}]}"#,
    );
    host.editor_state.set_single_selection(NodeId::new("p"));
    host.apply_property_action(PropertyPanelAction::SetNodeMaskType(None));
    assert_eq!(host.editor_state.selected_node_mask_type(), None);
    assert_eq!(host.editor_state.history.past.len(), 1);
    assert!(host.editor_state.undo());
    assert_eq!(
        host.editor_state.selected_node_mask_type(),
        Some(MaskType::Alpha)
    );

    seed(
        &mut host,
        r#"{"version":"1.0.0","children":[{"type":"rectangle","id":"r","fill":[{"type":"image","url":"asset.png","mode":"tile"}]}]}"#,
    );
    commit_focus(&mut host, PropertyFocus::PageBackgroundHex, "#1a2b3c80");
    assert_eq!(
        host.editor_state.active_page_background_color(),
        Some("#1A2B3C80")
    );
    assert_eq!(host.editor_state.history.past.len(), 1);
    assert!(host.editor_state.undo());
    assert!(host.editor_state.doc.pages.is_none());

    commit_focus(&mut host, PropertyFocus::PageBackgroundHex, "");
    assert!(host.editor_state.history.past.is_empty());
    host.editor_state.set_single_selection(NodeId::new("r"));
    commit_focus(&mut host, PropertyFocus::ImageTileScale, "0.38618907");
    assert_eq!(host.editor_state.history.past.len(), 1);
    assert_eq!(
        op_editor_core::first_image_fill_summary(host.editor_state.selected_node().unwrap())
            .unwrap()
            .tile_scale,
        Some(0.38618907)
    );
}

#[test]
fn web_compositing_picker_escape_keeps_selection() {
    let mut host = WidgetHost::new();
    host.editor_state.set_single_selection(NodeId::new("n10"));
    host.apply_property_action(PropertyPanelAction::ToggleCompositingPicker(
        CompositingPickerTarget::NodeBlend,
    ));
    assert!(host.editor_state.editor_ui.compositing_picker.open);
    assert!(host.apply_escape());
    assert!(!host.editor_state.editor_ui.compositing_picker.open);
    assert_eq!(host.editor_state.selection.anchor.as_str(), "n10");
}

#[test]
fn web_tile_scale_popover_input_owns_press_and_keeps_popup_open() {
    let mut host = WidgetHost::new();
    seed(
        &mut host,
        r#"{"version":"1.0.0","children":[{"type":"rectangle","id":"r","fill":[{"type":"image","url":"asset.png","mode":"tile","tileScale":0.38618907}]}]}"#,
    );
    host.editor_state.set_single_selection(NodeId::new("r"));
    host.editor_state.editor_ui.image_fill_popover_open = true;
    let panel = op_editor_ui::widgets::PropertyPanel::for_selection(&host.editor_state).unwrap();
    let width = host.editor_state.editor_ui.property_panel_width;
    let rect = op_editor_ui::Rect::xywh(
        VIEWPORT_W - width,
        op_editor_ui::widgets::TOP_BAR_HEIGHT,
        width,
        VIEWPORT_H - op_editor_ui::widgets::TOP_BAR_HEIGHT,
    );
    let point = find_tile_scale_input(&panel, rect);

    assert!(host.apply_press(point.x, point.y, VIEWPORT_W, VIEWPORT_H));
    assert_eq!(
        host.editor_state.ui.property_focus,
        Some(PropertyFocus::ImageTileScale)
    );
    assert_eq!(host.editor_state.ui.property_input.text(), "0.38618907");
    assert!(host.editor_state.editor_ui.image_fill_popover_open);

    host.commit_property_focus_if_any();
    assert!(host.editor_state.history.past.is_empty());
    assert_eq!(
        op_editor_core::first_image_fill_summary(host.editor_state.selected_node().unwrap())
            .unwrap()
            .tile_scale,
        Some(0.38618907)
    );
}

#[test]
fn web_image_fill_popover_owns_status_bar_overlap() {
    let mut host = WidgetHost::new();
    seed(
        &mut host,
        r#"{"version":"1.0.0","children":[{"type":"rectangle","id":"r","fill":[{"type":"image","url":"asset.png","mode":"tile","tileScale":0.5}]}]}"#,
    );
    host.editor_state.set_single_selection(NodeId::new("r"));
    host.editor_state.editor_ui.image_fill_popover_open = true;

    let panel = op_editor_ui::widgets::PropertyPanel::for_selection(&host.editor_state).unwrap();
    // At a narrow viewport the left-docked zoom still overlaps this popover.
    let viewport_w = 800.0;
    let mut property_rect = property_rect(&host);
    property_rect.origin.x = viewport_w - property_rect.size.x;
    let status_rect = host.status_bar_rect(viewport_w, VIEWPORT_H).unwrap();
    let status = op_editor_ui::widgets::StatusBar::for_editor(&host.editor_state);
    let point = find_status_bar_popover_overlap(&panel, property_rect, &status, status_rect);
    let zoom_before = host.editor_state.viewport.zoom;

    assert!(host.apply_press(point.x, point.y, viewport_w, VIEWPORT_H));
    assert!(host.editor_state.editor_ui.image_fill_popover_open);
    assert_eq!(host.editor_state.viewport.zoom, zoom_before);
    assert!(!matches!(
        host.editor_state.editor_ui.pressed_button,
        Some(op_editor_core::ButtonPressTarget::StatusBar(_))
    ));
}

#[test]
fn web_image_fill_popover_owns_variables_panel_overlap() {
    let mut host = WidgetHost::new();
    seed(
        &mut host,
        r#"{"version":"1.0.0","children":[{"type":"rectangle","id":"r","fill":[{"type":"image","url":"asset.png","mode":"tile","tileScale":0.5}]}]}"#,
    );
    host.editor_state.set_single_selection(NodeId::new("r"));
    host.editor_state.editor_ui.image_fill_popover_open = true;
    host.editor_state.editor_ui.variables_panel_open = true;

    let panel = op_editor_ui::widgets::PropertyPanel::for_selection(&host.editor_state).unwrap();
    let property_rect = property_rect(&host);
    let variables_rect = host.variables_panel_rect(VIEWPORT_W, VIEWPORT_H).unwrap();
    let variables =
        op_editor_ui::widgets::variables_panel::VariablesPanel::for_editor(&host.editor_state);
    let point =
        find_variables_resize_popover_overlap(&panel, property_rect, &variables, variables_rect);

    assert!(host.apply_press(point.x, point.y, VIEWPORT_W, VIEWPORT_H));
    assert!(host.editor_state.editor_ui.image_fill_popover_open);
    assert!(host.editor_state.editor_ui.variables_panel_open);
    assert!(host.variables_resize.is_none());
    assert!(!matches!(
        host.editor_state.editor_ui.pressed_button,
        Some(op_editor_core::ButtonPressTarget::VariablesPanel(_))
    ));
}

#[test]
fn web_tile_scale_draft_commits_before_popup_actions_and_outside_dismiss() {
    let mut host = WidgetHost::new();
    seed(
        &mut host,
        r#"{"version":"1.0.0","children":[{"type":"rectangle","id":"r","fill":[{"type":"image","url":"asset.png","mode":"tile","tileScale":0.5}]}]}"#,
    );
    host.editor_state.set_single_selection(NodeId::new("r"));

    focus_tile_scale(&mut host, "0.75");
    host.apply_property_action(PropertyPanelAction::SetImageFillMode(
        op_editor_core::ImageFillMode::Fit,
    ));
    assert_eq!(selected_tile_scale(&host), 0.75);
    assert_eq!(host.editor_state.ui.property_focus, None);
    assert!(host.editor_state.ui.property_input.text().is_empty());
    assert!(host.editor_state.ui.property_input_draft.is_empty());

    host.apply_property_action(PropertyPanelAction::SetImageFillMode(
        op_editor_core::ImageFillMode::Tile,
    ));
    focus_tile_scale(&mut host, "0.625");
    host.apply_property_action(PropertyPanelAction::CloseImageFillPopover);
    assert_eq!(selected_tile_scale(&host), 0.625);
    assert!(!host.editor_state.editor_ui.image_fill_popover_open);
    assert_eq!(host.editor_state.ui.property_focus, None);

    focus_tile_scale(&mut host, "0.875");
    let panel = op_editor_ui::widgets::PropertyPanel::for_selection(&host.editor_state).unwrap();
    let width = host.editor_state.editor_ui.property_panel_width;
    let rect = op_editor_ui::Rect::xywh(
        VIEWPORT_W - width,
        op_editor_ui::widgets::TOP_BAR_HEIGHT,
        width,
        VIEWPORT_H - op_editor_ui::widgets::TOP_BAR_HEIGHT,
    );
    let outside =
        op_editor_ui::Point2D::new(rect.origin.x + 20.0, rect.origin.y + rect.size.y - 20.0);
    assert!(!panel.image_fill_popover_contains(rect, outside));
    assert!(host.apply_press(outside.x, outside.y, VIEWPORT_W, VIEWPORT_H));
    assert_eq!(selected_tile_scale(&host), 0.875);
    assert!(!host.editor_state.editor_ui.image_fill_popover_open);
    assert_eq!(host.editor_state.ui.property_focus, None);
    assert!(host.editor_state.ui.property_input_draft.is_empty());
}

fn find_tile_scale_input(
    panel: &op_editor_ui::widgets::PropertyPanel,
    rect: op_editor_ui::Rect,
) -> op_editor_ui::Point2D {
    let mut y = rect.origin.y;
    while y < rect.origin.y + rect.size.y {
        let mut x = (rect.origin.x - 320.0).max(0.0);
        while x < rect.origin.x + rect.size.x {
            let point = op_editor_ui::Point2D::new(x, y);
            if panel.image_fill_popover_input_at(rect, point) == Some(PropertyFocus::ImageTileScale)
            {
                return point;
            }
            x += 2.0;
        }
        y += 2.0;
    }
    panic!("tile-scale input is not reachable in the open popover");
}

fn property_rect(host: &WidgetHost) -> op_editor_ui::Rect {
    let width = host.editor_state.editor_ui.property_panel_width;
    op_editor_ui::Rect::xywh(
        VIEWPORT_W - width,
        op_editor_ui::widgets::TOP_BAR_HEIGHT,
        width,
        VIEWPORT_H - op_editor_ui::widgets::TOP_BAR_HEIGHT,
    )
}

fn find_status_bar_popover_overlap(
    panel: &op_editor_ui::widgets::PropertyPanel,
    property_rect: op_editor_ui::Rect,
    status: &op_editor_ui::widgets::StatusBar,
    status_rect: op_editor_ui::Rect,
) -> op_editor_ui::Point2D {
    let mut y = status_rect.origin.y;
    while y < status_rect.origin.y + status_rect.size.y {
        let mut x = status_rect.origin.x;
        while x < status_rect.origin.x + status_rect.size.x {
            let point = op_editor_ui::Point2D::new(x, y);
            let closes_popup = matches!(
                panel.hit_test_action(property_rect, point),
                Some(PropertyPanelAction::CloseImageFillPopover)
            );
            if panel.image_fill_popover_contains(property_rect, point)
                && status.control_at(status_rect, point).is_some()
                && !closes_popup
            {
                return point;
            }
            x += 1.0;
        }
        y += 1.0;
    }
    panic!("image-fill popover does not overlap a StatusBar control");
}

fn find_variables_resize_popover_overlap(
    panel: &op_editor_ui::widgets::PropertyPanel,
    property_rect: op_editor_ui::Rect,
    variables: &op_editor_ui::widgets::variables_panel::VariablesPanel,
    variables_rect: op_editor_ui::Rect,
) -> op_editor_ui::Point2D {
    let mut y = variables_rect.origin.y;
    while y < variables_rect.origin.y + variables_rect.size.y {
        let mut x = variables_rect.origin.x;
        while x < variables_rect.origin.x + variables_rect.size.x {
            let point = op_editor_ui::Point2D::new(x, y);
            let variables_resize = matches!(
                variables.hit_test(variables_rect, point),
                Some(op_editor_ui::widgets::variables_panel::VariablesPanelHit::Resize(_))
            );
            let closes_popup = matches!(
                panel.hit_test_action(property_rect, point),
                Some(PropertyPanelAction::CloseImageFillPopover)
            );
            if variables_resize
                && panel.image_fill_popover_contains(property_rect, point)
                && !closes_popup
            {
                return point;
            }
            x += 1.0;
        }
        y += 1.0;
    }
    panic!("image-fill popover does not overlap the VariablesPanel resize edge");
}
