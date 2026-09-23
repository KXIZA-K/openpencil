//! Stroke-alignment inset/outset geometry tests for `canvas_viewport_paint.rs`.
//!
//! Split out of `canvas_viewport_paint_tests.rs` to keep every file
//! under the repository's 800-line cap.

use crate::layout_scene::{NodeKind, SceneNode, SceneStroke, SceneStrokeAlign};
use crate::widgets::canvas_viewport_paint::paint_node_with_options;
use crate::widgets::PaintCx;
use crate::{Color, Point2D, Rect, RenderBackend, TextLayout};

#[derive(Default)]
struct StrokeCaptureBackend {
    strokes: Vec<(f32, f32, f32, f32, f32)>,
    lines: Vec<(f32, f32, f32, f32, f32)>,
}

impl RenderBackend for StrokeCaptureBackend {
    fn begin_frame(&mut self) {}
    fn end_frame(&mut self) {}
    fn fill_rect(&mut self, _: Rect, _: Color) {}
    fn stroke_rect(&mut self, rect: Rect, _: Color, w: f32) {
        self.strokes
            .push((rect.origin.x, rect.origin.y, rect.size.x, rect.size.y, w));
    }
    fn draw_text(&mut self, _: &TextLayout, _: Point2D) {}
    fn clip_rect(&mut self, _: Rect) {}
    fn save(&mut self) {}
    fn restore(&mut self) {}
    fn translate(&mut self, _: Point2D) {}
    fn stroke_line(&mut self, from: Point2D, to: Point2D, _: Color, width: f32) {
        self.lines.push((from.x, from.y, to.x, to.y, width));
    }
    fn fill_round_rect(&mut self, _: Rect, _: f32, _: Color) {}
    fn stroke_round_rect(&mut self, _: Rect, _: f32, _: Color, _: f32) {}
    fn stroke_svg_path(&mut self, _: &str, _: Point2D, _: f32, _: Color, _: f32) {}
    fn resize(&mut self, _: u32, _: u32) {}
    fn dpi_scale(&self) -> f32 {
        1.0
    }
}

fn stroked_rect(align: SceneStrokeAlign) -> SceneNode {
    let mut n = SceneNode::leaf("s", NodeKind::Rect);
    n.bounds = Rect::xywh(0.0, 0.0, 100.0, 50.0);
    n.stroke = Some(SceneStroke {
        color: Color::BLACK,
        width: 4.0,
        sides: None,
        align,
    });
    n
}

fn paint(node: &SceneNode) -> Vec<(f32, f32, f32, f32, f32)> {
    let mut backend = StrokeCaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };
    let _ = paint_node_with_options(
        &mut cx,
        node,
        Point2D::ZERO,
        1.0,
        None,
        Rect::xywh(0.0, 0.0, 4000.0, 4000.0),
        None,
        None,
        None,
        None,
    );
    backend.strokes
}

/// Figma's default stroke is INSIDE: the painted band must sit
/// entirely within the node bounds, so the centered stroke_rect
/// call gets a half-width inset.
#[test]
fn inside_stroke_insets_by_half_width() {
    let strokes = paint(&stroked_rect(SceneStrokeAlign::Inside));
    assert_eq!(strokes, vec![(2.0, 2.0, 96.0, 46.0, 4.0)]);
}

/// OUTSIDE strokes outset by half a width.
#[test]
fn outside_stroke_outsets_by_half_width() {
    let strokes = paint(&stroked_rect(SceneStrokeAlign::Outside));
    assert_eq!(strokes, vec![(-2.0, -2.0, 104.0, 54.0, 4.0)]);
}

/// CENTER keeps the authored rect.
#[test]
fn center_stroke_keeps_rect() {
    let strokes = paint(&stroked_rect(SceneStrokeAlign::Center));
    assert_eq!(strokes, vec![(0.0, 0.0, 100.0, 50.0, 4.0)]);
}

#[test]
fn asymmetric_stroke_sides_respect_alignment_at_each_zoom() {
    for zoom in [0.5, 1.0, 1.25, 2.0] {
        for (align, direction) in [
            (SceneStrokeAlign::Inside, 1.0),
            (SceneStrokeAlign::Center, 0.0),
            (SceneStrokeAlign::Outside, -1.0),
        ] {
            let mut node = stroked_rect(align);
            node.stroke.as_mut().unwrap().sides = Some([2.0, 4.0, 6.0, 8.0]);
            let original = node.bounds;
            let mut backend = StrokeCaptureBackend::default();
            let mut cx = PaintCx { backend: &mut backend };
            paint_node_with_options(&mut cx, &node, Point2D::new(10.0, 20.0), zoom,
                None, Rect::xywh(-100.0, -100.0, 4000.0, 4000.0), None, None, None, None);
            let x0 = 10.0;
            let y0 = 20.0;
            let x1 = x0 + 100.0 * zoom;
            let y1 = y0 + 50.0 * zoom;
            assert_eq!(backend.lines, vec![
                (x0, y0 + direction * zoom, x1, y0 + direction * zoom, 2.0 * zoom),
                (x1 - direction * 2.0 * zoom, y0, x1 - direction * 2.0 * zoom, y1, 4.0 * zoom),
                (x0, y1 - direction * 3.0 * zoom, x1, y1 - direction * 3.0 * zoom, 6.0 * zoom),
                (x0 + direction * 4.0 * zoom, y0, x0 + direction * 4.0 * zoom, y1, 8.0 * zoom),
            ], "alignment={align:?}, zoom={zoom}");
            assert!(backend.strokes.is_empty());
            assert_eq!(node.bounds, original);
        }
    }
}

#[test]
fn absent_stroke_sides_do_not_paint() {
    let mut node = stroked_rect(SceneStrokeAlign::Center);
    node.stroke.as_mut().unwrap().sides = Some([2.0, 0.0, 0.0, 0.0]);
    let mut backend = StrokeCaptureBackend::default();
    let mut cx = PaintCx { backend: &mut backend };
    paint_node_with_options(&mut cx, &node, Point2D::ZERO, 1.0,
        None, Rect::xywh(-100.0, -100.0, 4000.0, 4000.0), None, None, None, None);
    assert_eq!(backend.lines, vec![(0.0, 0.0, 100.0, 0.0, 2.0)]);
}

#[test]
fn css_box_edges_snap_before_zoom_without_rewriting_editable_bounds() {
    for zoom in [0.5, 1.0, 1.25, 2.0] {
        let mut node = stroked_rect(SceneStrokeAlign::Center);
        node.bounds = Rect::xywh(15.5, 42.8, 100.5, 60.25);
        node.css_paint_origin = Some(Point2D::new(10.25, 20.5));
        let original = node.bounds;
        let mut backend = StrokeCaptureBackend::default();
        let mut cx = PaintCx { backend: &mut backend };
        paint_node_with_options(&mut cx, &node, Point2D::new(13.25, 17.5), zoom,
            None, Rect::xywh(0.0, 0.0, 4000.0, 4000.0), None, None, None, None);
        assert_eq!(backend.strokes, vec![(13.25 + 15.25 * zoom, 17.5 + 42.5 * zoom, 101.0 * zoom, 61.0 * zoom, 4.0 * zoom)]);
        assert_eq!(node.bounds, original);
    }
}
