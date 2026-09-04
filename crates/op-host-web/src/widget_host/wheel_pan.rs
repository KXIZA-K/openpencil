//! Wheel / trackpad-pan routing on the web `WidgetHost`, plus the
//! transcript scroll hand-off they share.
//!
//! Split out of the `widget_host.rs` spine to keep it under the repo's
//! 800-line cap.

use super::*;

impl WidgetHost {
    /// Scroll the conversation dropdown before the transcript or canvas sees
    /// the wheel. The room cap is larger than the visible list by design.
    fn try_scroll_chat_thread_picker(
        &mut self,
        x: f32,
        y: f32,
        delta: f32,
        viewport_width: f32,
        viewport_height: f32,
    ) -> bool {
        if !self.editor_state.editor_ui.chat_thread_picker_open {
            return false;
        }
        let Some(chat_rect) = self.ai_chat_rect(viewport_width, viewport_height) else {
            return false;
        };
        let panel = op_editor_ui::widgets::AIChatPlaceholder::from_editor_at(
            &self.editor_state,
            self.now_ms,
        );
        let Some(picker) = panel.thread_picker_bounds(chat_rect) else {
            return false;
        };
        if !picker.contains(Point2D::new(x, y)) {
            return false;
        }
        let max = panel.thread_picker_scroll_max(chat_rect);
        let scroll = &mut self.editor_state.editor_ui.chat_thread_picker_scroll;
        let next = (scroll.offset - delta).clamp(0.0, max);
        if next != scroll.offset {
            scroll.offset = next;
            self.editor_state.editor_ui.chat_tab_hover = None;
            self.mark_dirty();
        }
        true
    }

    /// Scroll the open chat model picker before the event can reach the chat
    /// input, side rails, or canvas. Browser mouse wheels enter `apply_wheel`
    /// while trackpad gestures enter `apply_pan_gesture`, so both ladders must
    /// call this owner check (the native host already does the same).
    fn try_scroll_chat_model_picker(
        &mut self,
        x: f32,
        y: f32,
        delta: f32,
        viewport_width: f32,
        viewport_height: f32,
    ) -> bool {
        if !self.editor_state.editor_ui.chat_model_picker.open {
            return false;
        }
        let Some(picker) =
            self.ai_chat_rect(viewport_width, viewport_height)
                .and_then(|chat_rect| {
                    op_editor_ui::widgets::AIChatPlaceholder::from_editor_at(
                        &self.editor_state,
                        self.now_ms,
                    )
                    .model_picker_bounds(chat_rect)
                })
        else {
            return false;
        };
        if !picker.contains(Point2D::new(x, y)) {
            return false;
        }

        let max = op_editor_ui::widgets::ai_chat_model_picker::max_picker_scroll(
            &self.editor_state.chat.available_models,
            self.editor_state.editor_ui.chat_model_picker_input.text(),
        );
        let picker = &mut self.editor_state.editor_ui.chat_model_picker;
        let next = (picker.scroll.offset - delta).clamp(0.0, max);
        if next != picker.scroll.offset {
            picker.scroll.offset = next;
            picker.hover = None;
            self.mark_dirty();
        }
        true
    }

    /// Scroll the chat transcript message list when a wheel / trackpad
    /// pan lands over the panel body — pinned-to-bottom auto-follow
    /// resumes once the user scrolls back to the bottom. Mirrors the
    /// native host's `try_scroll_chat_transcript`.
    fn try_scroll_chat_transcript(
        &mut self,
        x: f32,
        y: f32,
        delta: f32,
        viewport_width: f32,
        viewport_height: f32,
    ) -> bool {
        if self.editor_state.chat.messages.is_empty() {
            return false;
        }
        let point = Point2D::new(x, y);
        let Some(chat_rect) = self.ai_chat_rect(viewport_width, viewport_height) else {
            return false;
        };
        let (body, max) = {
            // `transcript_scroll_max` resolves the cache; owner-stamp so a rebuild
            // here tags the slot with this host's panel rather than UNOWNED.
            let panel = op_editor_ui::widgets::AIChatPlaceholder::from_editor_at(
                &self.editor_state,
                self.now_ms,
            )
            .owned_by(self.chat_panel_owner);
            (
                panel.body_rect(chat_rect),
                panel.transcript_scroll_max(chat_rect),
            )
        };
        if !(body).contains(point) {
            return false;
        }
        let chat = &mut self.editor_state.chat;
        if max <= 0.0 {
            if !chat.transcript_pinned || chat.transcript_scroll.offset != 0.0 {
                chat.transcript_pinned = true;
                chat.transcript_scroll.offset = 0.0;
                self.mark_dirty();
            }
            return true;
        }
        let cur = if chat.transcript_pinned {
            max
        } else {
            chat.transcript_scroll.offset.clamp(0.0, max)
        };
        let next = (cur - delta).clamp(0.0, max);
        let pinned = next >= max - 0.5;
        if (next - chat.transcript_scroll.offset).abs() > f32::EPSILON
            || chat.transcript_pinned != pinned
        {
            chat.transcript_scroll.offset = next;
            chat.transcript_pinned = pinned;
            self.mark_dirty();
        }
        true
    }

    /// Scroll the chat panel's draft input when the wheel lands over a
    /// prompt too long for the box. Runs ahead of the transcript handler
    /// (which bails on an empty message list) so a long first prompt is
    /// scrollable before any turn has been sent.
    fn try_scroll_chat_input(
        &mut self,
        x: f32,
        y: f32,
        delta: f32,
        viewport_width: f32,
        viewport_height: f32,
    ) -> bool {
        let chat_rect = self.ai_chat_rect(viewport_width, viewport_height);
        let Some(dirty) = op_editor_ui::widgets::scroll_flow::scroll_chat_input(
            &mut self.editor_state,
            chat_rect,
            self.now_ms,
            Point2D::new(x, y),
            delta,
        ) else {
            return false;
        };
        if dirty {
            self.mark_dirty();
        }
        true
    }

    /// Wheel zoom centered on the cursor when over the canvas.
    pub fn apply_wheel(
        &mut self,
        x: f32,
        y: f32,
        delta_y: f32,
        viewport_width: f32,
        viewport_height: f32,
    ) -> bool {
        self.last_viewport_w = viewport_width;
        self.last_viewport_h = viewport_height;
        if self.try_scroll_missing_fonts_picker(x, y, delta_y, viewport_width, viewport_height) {
            return true;
        }
        if self.try_scroll_html_import_diagnostics(x, y, delta_y, viewport_width, viewport_height) {
            return true;
        }
        if self.try_scroll_settings_font_picker(x, y, delta_y, viewport_width, viewport_height) {
            return true;
        }
        if self.editor_state.editor_ui.agent_settings_open {
            use op_editor_ui::widgets::agent_settings_panel::AgentSettingsPanel;
            let panel_rect = AgentSettingsPanel::for_web_editor(&self.editor_state)
                .rect(viewport_width, viewport_height);
            let point = Point2D::new(x, y);
            if (panel_rect).contains(point) {
                if let Some(max) = AgentSettingsPanel::for_web_editor(&self.editor_state)
                    .builtin_preset_scroll_max_at(panel_rect, point)
                {
                    let settings = &mut self.editor_state.editor_ui.agent_settings;
                    settings
                        .builtin_preset_menu_scroll
                        .scroll_by(-delta_y, max, 0.0);
                    self.mark_dirty();
                    return true;
                }
                let panel = AgentSettingsPanel::for_web_editor(&self.editor_state);
                let total = panel.content_total_height();
                let viewport_h_inner = panel_rect.size.y
                    - op_editor_ui::widgets::agent_settings_panel::CONTENT_VERTICAL_INSET;
                let max_scroll = (total - viewport_h_inner).max(0.0);
                self.editor_state
                    .editor_ui
                    .agent_settings
                    .scroll_y
                    .scroll_by(-delta_y, max_scroll, 0.0);
                // Content moved under a stationary cursor — re-derive
                // the hover state so buttons don't keep a stale wash.
                self.update_agent_settings_hover(x, y);
                self.mark_dirty();
                return true;
            }
        }
        if self.try_scroll_design_md_panel(x, y, delta_y, viewport_width, viewport_height) {
            return true;
        }
        if self.try_scroll_icon_picker(x, y, delta_y, viewport_width, viewport_height) {
            return true;
        }
        if self.try_scroll_scene_template_center(x, y, delta_y, viewport_width, viewport_height) {
            return true;
        }
        if self.try_scroll_prompt_center(x, y, delta_y, viewport_width, viewport_height) {
            return true;
        }
        if self.try_scroll_chat_thread_picker(x, y, delta_y, viewport_width, viewport_height) {
            return true;
        }
        if self.try_scroll_chat_model_picker(x, y, delta_y, viewport_width, viewport_height) {
            return true;
        }
        if self.try_scroll_chat_input(x, y, delta_y, viewport_width, viewport_height) {
            return true;
        }
        if self.try_scroll_chat_transcript(x, y, delta_y, viewport_width, viewport_height) {
            return true;
        }
        // Floating VariablesPanel owns the wheel over its rect.
        if self.try_scroll_variables_panel(x, y, delta_y, viewport_width, viewport_height) {
            return true;
        }
        if self.try_scroll_locale_picker(x, y, delta_y, viewport_width) {
            return true;
        }
        // Side rails scroll their panels instead of zooming the
        // canvas (`widget_host/scroll.rs`).
        if self.try_scroll_property_panel(x, y, delta_y, viewport_width, viewport_height) {
            return true;
        }
        if self.try_scroll_layer_panel(x, y, 0.0, delta_y, viewport_height) {
            return true;
        }
        if !self.over_canvas(x, y, viewport_width, viewport_height) {
            return false;
        }
        // Cursor is in canvas-local coords — subtract the live
        // canvas-region offset (sidebar collapse aware).
        let (cx0, cy0, _cw, _ch) = self.canvas_region(viewport_width, viewport_height);
        let cursor = Point2D::new(x - cx0, y - cy0);
        self.editor_state.viewport.zoom_at(cursor, delta_y);
        // A wheel zoom only changes the viewport (camera); the document-space
        // layout scene is unchanged, so keep the layout cache intact — no
        // `mark_dirty()` (matches native `scroll.rs`). The wheel listener
        // repaints off this `true` return.
        true
    }

    /// 2-finger trackpad pan — translate viewport by `(dx, dy)`.
    /// Same Figma-style separation as the native host: trackpad
    /// swipe pans, pinch / Cmd+wheel / mouse-wheel zooms. Public
    /// host API — a future trackpad-gesture runner wires this.
    #[allow(dead_code)]
    pub fn apply_pan_gesture(
        &mut self,
        x: f32,
        y: f32,
        dx: f32,
        dy: f32,
        viewport_width: f32,
        viewport_height: f32,
    ) -> bool {
        if self.try_scroll_missing_fonts_picker(x, y, dy, viewport_width, viewport_height) {
            return true;
        }
        if self.try_scroll_html_import_diagnostics(x, y, dy, viewport_width, viewport_height) {
            return true;
        }
        self.last_viewport_w = viewport_width;
        self.last_viewport_h = viewport_height;
        if self.try_scroll_settings_font_picker(x, y, dy, viewport_width, viewport_height) {
            return true;
        }
        if self.try_scroll_variables_panel(x, y, dy, viewport_width, viewport_height) {
            return true;
        }
        if self.try_scroll_locale_picker(x, y, dy, viewport_width) {
            return true;
        }
        if self.try_scroll_design_md_panel(x, y, dy, viewport_width, viewport_height) {
            return true;
        }
        if self.try_scroll_icon_picker(x, y, dy, viewport_width, viewport_height) {
            return true;
        }
        if self.try_scroll_scene_template_center(x, y, dy, viewport_width, viewport_height) {
            return true;
        }
        if self.try_scroll_prompt_center(x, y, dy, viewport_width, viewport_height) {
            return true;
        }
        if self.try_scroll_chat_thread_picker(x, y, dy, viewport_width, viewport_height) {
            return true;
        }
        if self.try_scroll_chat_model_picker(x, y, dy, viewport_width, viewport_height) {
            return true;
        }
        if self.try_scroll_chat_input(x, y, dy, viewport_width, viewport_height) {
            return true;
        }
        if self.try_scroll_chat_transcript(x, y, dy, viewport_width, viewport_height) {
            return true;
        }
        if self.try_scroll_property_panel(x, y, dy, viewport_width, viewport_height) {
            return true;
        }
        if self.try_scroll_layer_panel(x, y, dx, dy, viewport_height) {
            return true;
        }
        if !self.over_canvas(x, y, viewport_width, viewport_height) {
            return false;
        }
        if dx == 0.0 && dy == 0.0 {
            return false;
        }
        self.editor_state.viewport.pan(dx, dy);
        // Trackpad pan only translates the viewport; keep the layout cache
        // intact — no `mark_dirty()` (matches the canvas pan-drag + native).
        true
    }

    pub fn apply_pan_press(
        &mut self,
        x: f32,
        y: f32,
        viewport_width: f32,
        viewport_height: f32,
    ) -> bool {
        self.last_viewport_w = viewport_width;
        self.last_viewport_h = viewport_height;
        if self.over_topmost_panel(x, y, viewport_width, viewport_height)
            || !self.over_canvas(x, y, viewport_width, viewport_height)
        {
            return false;
        }
        self.drag = Some(DragState {
            last_x: x,
            last_y: y,
        });
        true
    }
}
