//! The only module permitted to call `op_editor_ui::widgets::*` (widget-boundary rule).
//!
//! Provides the sole paint entry point for the read-only viewer. All widget
//! logic is contained here; `render.rs` calls through this module without
//! touching `op_editor_ui::widgets` directly.
//!
//! `paint_scene` is called only from the `canvaskit`-feature render path.
#![allow(dead_code)]

use op_editor_core::Viewport as DocViewport;
use op_editor_ui::layout_scene::LayoutScene;
use op_editor_ui::theme::Theme;
use op_editor_ui::widgets::canvas_viewport::CanvasViewport;
use op_editor_ui::widgets::{PaintCx, Widget};
use op_editor_ui::{Rect, RenderBackend};

// glue: read-only viewer paint pass.
pub fn paint_scene(
    backend: &mut dyn RenderBackend,
    scene: &LayoutScene,
    viewport: DocViewport,
    theme: Theme,
    w: f32,
    h: f32,
) {
    let view = CanvasViewport::from_scene(scene, viewport, theme);
    let mut cx = PaintCx { backend };
    view.paint(&mut cx, Rect::xywh(0.0, 0.0, w, h));
}

/// Paint one authored root through the exact editor painter, with no preview
/// projection or widget promotion. This is the independent visual QA baseline.
pub fn paint_authored_root(backend: &mut dyn RenderBackend, scene: &LayoutScene, id: &str, w: f32, h: f32, zoom: f32, scroll: f32) {
    use op_editor_ui::{Point2D, layout_scene::ScenePage};
    use op_editor_ui::widgets::paint_scene_page;
    let Some(node) = scene.pages.iter().flat_map(|page| &page.children).find(|node| node.id == id) else { return };
    let pan = Point2D::new((w - node.bounds.size.x * zoom) / 2.0, -scroll);
    let mut normalized = scene.clone();
    normalized.active_page_index = scene.pages.iter().position(|p| p.children.iter().any(|n| n.id == id)).unwrap_or(0);
    normalized.translate_nodes(&[id.to_string()], -node.bounds.origin.x, -node.bounds.origin.y);
    let node = normalized.active_page().and_then(|p| p.find(id)).unwrap();
    let page = ScenePage { id: "reference".into(), name: "Design reference".into(), children: vec![node.clone()] };
    backend.save();
    backend.clip_rect(Rect::xywh(0.0, 0.0, w, h));
    paint_scene_page(&mut PaintCx { backend: &mut *backend }, &page, pan, zoom, Rect::xywh(-64.0, -64.0, w + 128.0, h + 128.0));
    backend.restore();
}
