//! Floating-overlay rect getters for the web `WidgetHost`.
//!
//! Ported from the native host's `widget_host/overlay_rects.rs` (and
//! the StatusBar placement helpers formerly in `press.rs`) so the web
//! paint pass + press dispatch share one source of truth for every
//! overlay's on-screen rect. Keeping these in a sibling module keeps
//! `press.rs` under the repo's 800-line cap.

use super::WidgetHost;
use op_editor_ui::widgets::host_canvas_geometry as canvas_geometry;
use op_editor_ui::widgets::host_overlay_geometry as overlay_geometry;
use op_editor_ui::{Point2D, Rect};

impl WidgetHost {
    /// The floating bottom-left StatusBar pill rect, or `None` when
    /// the canvas is too narrow to float it (matches the paint guard).
    pub(in crate::widget_host) fn status_bar_rect(
        &self,
        viewport_w: f32,
        viewport_h: f32,
    ) -> Option<Rect> {
        let mut rect = canvas_geometry::status_bar_rect(&self.editor_state, viewport_w, viewport_h)?;
        let (left, top, _, _) = self.canvas_region(viewport_w, viewport_h);
        rect.origin.x = left + canvas_geometry::STATUS_INSET;
        // Narrow canvases (or a dragged chat) must keep zoom controls usable.
        if let Some(chat) = self.ai_chat_rect(viewport_w, viewport_h) {
            if !self.editor_state.chat.maximized
                && rect.origin.x < chat.origin.x + chat.size.x
                && rect.origin.x + rect.size.x > chat.origin.x
                && rect.origin.y < chat.origin.y + chat.size.y
                && rect.origin.y + rect.size.y > chat.origin.y
            {
                rect.origin.y = chat.origin.y - rect.size.y - 8.0;
            }
        }
        (rect.origin.y >= top).then_some(rect)
    }

    /// Step the canvas zoom from a StatusBar `[-]` / `[+]` click,
    /// anchored at the canvas-region centre so the visible content
    /// scales in place (≈ ±20 % per click via `Viewport::zoom_at`).
    pub(in crate::widget_host) fn status_bar_zoom(
        &mut self,
        zoom_in: bool,
        viewport_w: f32,
        viewport_h: f32,
    ) {
        overlay_geometry::status_bar_zoom(&mut self.editor_state, zoom_in, viewport_w, viewport_h);
        self.mark_dirty();
    }

    /// Zoom + pan so the active page's content is framed within the canvas.
    pub(in crate::widget_host) fn zoom_to_fit(&mut self, viewport_w: f32, viewport_h: f32) {
        self.refresh_layout_scene();
        overlay_geometry::zoom_to_fit(
            &mut self.editor_state,
            &self.layout_scene,
            viewport_w,
            viewport_h,
        );
        self.mark_dirty();
    }

    /// Shape-picker dropdown rect — anchored to the right of the
    /// toolbar's shape slot. Mirrors the native host so paint, press
    /// dispatch, and hover updates can't drift apart.
    pub(in crate::widget_host) fn shape_picker_rect(
        &self,
        viewport_w: f32,
        viewport_h: f32,
    ) -> Rect {
        overlay_geometry::shape_picker_rect(&self.editor_state, viewport_w, viewport_h)
    }

    /// The open File-menu dropdown rect, or `None` when closed.
    pub(in crate::widget_host) fn file_menu_rect(&self, viewport_w: f32) -> Option<Rect> {
        use op_editor_ui::widgets::file_menu::FileMenu;
        if !self.editor_state.editor_ui.file_menu_open {
            return None;
        }
        let top_bar_rect = self.top_bar_rect(viewport_w);
        let anchor = self.top_bar().file_menu_rect_for(top_bar_rect);
        let menu = FileMenu::from_editor_ui(&self.editor_state.editor_ui, self.wall_now_secs);
        Some(menu.rect_at(anchor))
    }

    /// The export quick-menu dropdown rect — anchored under the TopBar
    /// download button. Shared with paint + hit-test through
    /// `host_overlay_geometry` so the three can never drift.
    pub(in crate::widget_host) fn export_quick_menu_rect(&self, viewport_w: f32) -> Rect {
        overlay_geometry::export_quick_menu_rect(&self.editor_state, viewport_w)
    }

    /// Whether `point` is inside an open chrome dropdown that paints
    /// above floating panels. These dropdowns are visually topmost, so
    /// their hover/click handling must win over the Variables panel
    /// when their rects overlap.
    pub(in crate::widget_host) fn over_dropdown_overlay(
        &self,
        x: f32,
        y: f32,
        viewport_w: f32,
        viewport_h: f32,
    ) -> bool {
        let p = Point2D::new(x, y);
        let ui = &self.editor_state.editor_ui;
        (ui.shape_picker.open && (self.shape_picker_rect(viewport_w, viewport_h)).contains(p))
            || (ui.locale_picker.open && (self.locale_picker_rect(viewport_w)).contains(p))
            || (ui.import_menu_open && (self.import_menu_rect(viewport_w, viewport_h)).contains(p))
            || self
                .file_menu_rect(viewport_w)
                .is_some_and(|r| (r).contains(p))
            || (ui.export_quick_menu_open && (self.export_quick_menu_rect(viewport_w)).contains(p))
    }

    pub(in crate::widget_host) fn import_menu_rect(
        &self,
        viewport_w: f32,
        viewport_h: f32,
    ) -> Rect {
        overlay_geometry::import_menu_rect(&self.editor_state, viewport_w, viewport_h)
    }

    /// Floating Design-MD panel rect — `None` when the panel is
    /// closed. The top-left comes from `editor_ui.design_md_panel.pos`
    /// (centred on open), clamped so the header bar stays reachable
    /// after a viewport resize. Mirrors the native host.
    pub(in crate::widget_host) fn design_md_panel_rect(
        &self,
        viewport_w: f32,
        viewport_h: f32,
    ) -> Option<Rect> {
        overlay_geometry::design_md_panel_rect(&self.editor_state, viewport_w, viewport_h)
    }

    /// Floating Component-Browser panel rect — `None` when closed.
    /// Same centred-on-open + clamped placement as the Design-MD panel.
    pub(in crate::widget_host) fn component_browser_panel_rect(
        &self,
        viewport_w: f32,
        viewport_h: f32,
    ) -> Option<Rect> {
        overlay_geometry::component_browser_panel_rect(&self.editor_state, viewport_w, viewport_h)
    }

    /// Asset Center gallery rect — `None` when closed.
    pub(in crate::widget_host) fn scene_template_panel_rect(
        &self,
        viewport_w: f32,
        viewport_h: f32,
    ) -> Option<Rect> {
        overlay_geometry::scene_template_panel_rect(&self.editor_state, viewport_w, viewport_h)
    }

    /// The gallery's dimming layer — `None` when closed.
    pub(in crate::widget_host) fn scene_template_scrim_rect(
        &self,
        viewport_w: f32,
        viewport_h: f32,
    ) -> Option<Rect> {
        overlay_geometry::scene_template_scrim_rect(&self.editor_state, viewport_w, viewport_h)
    }

    pub(in crate::widget_host) fn prompt_center_panel_rect(
        &self,
        viewport_w: f32,
        viewport_h: f32,
    ) -> Option<Rect> {
        overlay_geometry::prompt_center_panel_rect(&self.editor_state, viewport_w, viewport_h)
    }

    /// Floating Icon-picker panel rect — `None` when closed. Centred
    /// compact searchable panel, same placement as the native host.
    pub(in crate::widget_host) fn icon_picker_panel_rect(
        &self,
        viewport_w: f32,
        viewport_h: f32,
    ) -> Option<Rect> {
        overlay_geometry::icon_picker_panel_rect(&self.editor_state, viewport_w, viewport_h)
    }

    /// The post-import HTML diagnostics card — `None` while it is
    /// dismissed or the last import degraded nothing. The whole card
    /// counts, not just the scrollable rows viewport: the header is
    /// opaque chrome and a wheel / press over it must not reach the
    /// canvas underneath. Mirrors the native host.
    pub(in crate::widget_host) fn html_import_diagnostics_rect(
        &self,
        viewport_w: f32,
        viewport_h: f32,
    ) -> Option<Rect> {
        op_editor_ui::widgets::html_import_diagnostics_flow::panel_rect(
            &self.editor_state,
            viewport_w,
            viewport_h,
        )
    }

    /// Whether `point` is inside ANY top-most floating panel
    /// (Design-MD / Variables / Icon picker / Component browser). Used
    /// to suppress lower-layer input updates under overlapping panels.
    ///
    /// The HTML-import diagnostics card joins them: it is non-modal (an
    /// outside press still reaches the canvas) but it is opaque, so
    /// input landing ON it belongs to it.
    pub(in crate::widget_host) fn over_topmost_panel(
        &self,
        x: f32,
        y: f32,
        viewport_w: f32,
        viewport_h: f32,
    ) -> bool {
        let p = Point2D::new(x, y);
        self.html_import_diagnostics_rect(viewport_w, viewport_h)
            .is_some_and(|r| (r).contains(p))
            || self
                .design_md_panel_rect(viewport_w, viewport_h)
                .is_some_and(|r| (r).contains(p))
            || self
                .variables_panel_rect(viewport_w, viewport_h)
                .is_some_and(|r| (r).contains(p))
            || self
                .icon_picker_panel_rect(viewport_w, viewport_h)
                .is_some_and(|r| (r).contains(p))
            || self
                .prompt_center_panel_rect(viewport_w, viewport_h)
                .is_some_and(|r| (r).contains(p))
            || self
                .component_browser_panel_rect(viewport_w, viewport_h)
                .is_some_and(|r| (r).contains(p))
    }

    /// Panels painted above Chat. Variables is intentionally excluded:
    /// it sits below Chat even though [`Self::over_topmost_panel`] groups
    /// it with floating chrome for canvas suppression.
    pub(in crate::widget_host) fn over_true_topmost_panel(
        &self,
        point: Point2D,
        viewport_w: f32,
        viewport_h: f32,
    ) -> bool {
        self.design_md_panel_rect(viewport_w, viewport_h)
            .is_some_and(|rect| rect.contains(point))
            || self
                .icon_picker_panel_rect(viewport_w, viewport_h)
                .is_some_and(|rect| rect.contains(point))
            || self
                .prompt_center_panel_rect(viewport_w, viewport_h)
                .is_some_and(|rect| rect.contains(point))
            || self
                .component_browser_panel_rect(viewport_w, viewport_h)
                .is_some_and(|rect| rect.contains(point))
    }
}
