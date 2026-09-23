//! Browser adapter over the canonical design preview. No generated HTML, no
//! second layout/painter, and no writes back to the authored document.
use crate::preview::PreviewSession;
use jian_core::gesture::pointer::{Modifiers, PointerPhase};
use op_editor_ui::{Color, Rect, RenderBackend};
use op_host_web::canvaskit::{init_backend, CanvasKitBackend};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct PrototypePlayer {
    backend: CanvasKitBackend,
    session: PreviewSession,
    source: String,
    width: f32,
    height: f32,
    zoom: f32,
    scroll: f32,
    screen: Option<usize>,
    design_scene: op_editor_ui::layout_scene::LayoutScene,
    design_reference: bool,
}

#[wasm_bindgen]
impl PrototypePlayer {
    pub fn install_icon_catalog(json: &str) {
        op_editor_ui::set_core_catalog(json);
    }
    pub async fn mount(
        canvas_id: String,
        source: String,
        width: f32,
        height: f32,
        dpr: f32,
    ) -> Result<PrototypePlayer, JsValue> {
        let mut viewer = crate::Viewer::new();
        viewer
            .load(&source)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        let design_scene = viewer
            .scene()
            .cloned()
            .ok_or_else(|| JsValue::from_str("No design scene"))?;
        let backend = init_backend(&canvas_id, dpr, width as u32, height as u32).await?;
        let session = Self::session(&source, &backend)?;
        let screen = session.current_screen_index();
        let mut player = Self {
            backend,
            session,
            source,
            width,
            height,
            zoom: 1.0,
            scroll: 0.0,
            screen,
            design_scene,
            design_reference: false,
        };
        player.use_authored_scene();
        Ok(player)
    }

    pub fn resize(&mut self, width: f32, height: f32, dpr: f32) {
        self.width = width.max(1.0);
        self.height = height.max(1.0);
        self.backend
            .resize_for_display(self.width as u32, self.height as u32, dpr);
    }

    pub fn draw(&mut self, now: f64) {
        self.session.set_now_ms(now as u64);
        self.session.reconcile(now as u64);
        let screen = self.session.current_screen_index();
        if screen != self.screen {
            self.scroll = 0.0;
            self.screen = screen;
            self.use_authored_scene();
        }
        let rect = self.bounds();
        self.zoom = (self.width / rect.size.x.max(1.0)).min(1.0);
        self.scroll = self
            .scroll
            .clamp(0.0, (rect.size.y * self.zoom - self.height).max(0.0));
        let pan = (
            (self.width - rect.size.x * self.zoom) / 2.0 - rect.origin.x * self.zoom,
            -rect.origin.y * self.zoom - self.scroll,
        );
        self.backend.begin_frame();
        self.backend.fill_rect(
            Rect::xywh(0.0, 0.0, self.width, self.height),
            Color::rgba_u8(255, 255, 255, 1.0),
        );
        if self.design_reference {
            if let Some((id, _)) = self.session.framed_root() {
                crate::viewer_host::paint_authored_root(
                    &mut self.backend,
                    &self.design_scene,
                    &id,
                    self.width,
                    self.height,
                    self.zoom,
                    self.scroll,
                );
            }
        } else {
            self.session.paint_scene(
                &mut self.backend,
                Rect::xywh(0.0, 0.0, self.width, self.height),
                pan,
                self.zoom,
                now as u64,
            );
        }
        self.backend.end_frame();
    }

    pub fn pointer(&mut self, phase: &str, x: f32, y: f32) {
        let rect = self.bounds();
        let sx = (x - (self.width - rect.size.x * self.zoom) / 2.0) / self.zoom + rect.origin.x;
        let sy = (y + self.scroll) / self.zoom + rect.origin.y;
        let phase = match phase {
            "down" => PointerPhase::Down,
            "up" => PointerPhase::Up,
            _ => PointerPhase::Move,
        };
        self.session.dispatch_pointer_phase(sx, sy, phase);
    }

    pub fn text(&mut self, text: &str) {
        self.session.dispatch_text(text);
    }
    pub fn key(&mut self, key: &str, shift: bool, ctrl: bool, meta: bool) {
        let mut modifiers = Modifiers::empty();
        modifiers.set(Modifiers::SHIFT, shift);
        modifiers.set(Modifiers::CTRL, ctrl);
        modifiers.set(Modifiers::CMD, meta);
        self.session.dispatch_key(key, modifiers);
    }
    pub fn scroll_by(&mut self, dy: f32) {
        self.scroll += dy;
    }
    pub fn navigate(&mut self, path: &str) {
        self.session.navigate_to_screen(path);
    }
    /// A read-only reference using the original editor scene, for visual QA.
    pub fn set_design_reference(&mut self, enabled: bool) {
        self.design_reference = enabled;
    }
    pub fn reset(&mut self) -> Result<(), JsValue> {
        self.session = Self::session(&self.source, &self.backend)?;
        self.use_authored_scene();
        self.scroll = 0.0;
        Ok(())
    }
    pub fn info(&self) -> String {
        let r = self.bounds();
        serde_json::json!({ "screens": self.session.screen_switcher_entries(), "active": self.session.current_screen_index(),
            "width": r.size.x, "height": r.size.y, "zoom": self.zoom, "scroll": self.scroll,
            "warnings": self.session.warnings(), "controls": self.session.interaction_snapshot() }).to_string()
    }
}

impl PrototypePlayer {
    fn use_authored_scene(&mut self) {
        let Some((id, _)) = self.session.framed_root() else {
            return;
        };
        let Some(index) = self
            .design_scene
            .pages
            .iter()
            .position(|p| p.children.iter().any(|n| n.id == id))
        else {
            return;
        };
        let mut scene = self.design_scene.clone();
        scene.active_page_index = index;
        let node = scene.pages[index].find(&id).unwrap();
        let origin = node.bounds.origin;
        scene.translate_nodes(&[id.clone()], -origin.x, -origin.y);
        scene.pages[index].children.retain(|n| n.id == id);
        self.session.use_authored_scene(scene);
    }
    fn session(source: &str, backend: &CanvasKitBackend) -> Result<PreviewSession, JsValue> {
        let mut viewer = crate::Viewer::new();
        viewer
            .load(source)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        let doc = viewer
            .doc
            .as_ref()
            .ok_or_else(|| JsValue::from_str("No document"))?;
        PreviewSession::enter(
            doc,
            (1200.0, 900.0),
            &Default::default(),
            viewer.active_page,
            viewer.preserve_authored_geometry,
            false,
            backend.preview_measure(),
            0,
        )
        .map_err(|e| JsValue::from_str(&e.to_string()))
    }
    fn bounds(&self) -> Rect {
        // Painting uses the normalized authored scene, whose flex root can grow
        // beyond its declared height. The runtime's root rect still describes
        // the declared viewport and would clamp away visible bottom controls.
        // Use the same scene for paint, pan, pointer translation and scrolling.
        self.session
            .framed_root()
            .map(|r| r.1)
            .or_else(|| self.session.current_screen_scene_rect())
            .unwrap_or(Rect::xywh(0.0, 0.0, 1200.0, 900.0))
    }
}
