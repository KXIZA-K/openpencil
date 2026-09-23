//! Unit tests for the canvas text-node painter — slice ranges,
//! justify spread, greeking, styled runs, decorations, and IME
//! composition. Carved off `canvas_viewport_text.rs` to keep every
//! file under the 800-line cap.

use super::*;
use crate::layout_scene::NodeKind;
use crate::widgets::PaintCx;

fn run(start: usize, end: usize) -> SceneTextRun {
    SceneTextRun {
        start,
        end,
        font_size: 0.0,
        font_weight: 0,
        fill: None,
        italic: false,
        underline: false,
        strikethrough: false,
    }
}

#[test]
fn slice_ranges_without_runs_is_one_default_slice() {
    assert_eq!(slice_ranges(&[], 3, 9), vec![(3, 9, None)]);
    assert!(slice_ranges(&[], 5, 5).is_empty());
}

#[test]
fn slice_ranges_splits_a_line_at_run_boundaries() {
    // Runs: [0,5) [5,11). Line covers [3,9) → two slices.
    let runs = [run(0, 5), run(5, 11)];
    assert_eq!(
        slice_ranges(&runs, 3, 9),
        vec![(3, 5, Some(0)), (5, 9, Some(1))]
    );
}

#[test]
fn slice_ranges_line_inside_one_run_is_one_slice() {
    // A wrapped mid-segment line stays a single styled slice.
    let runs = [run(0, 20)];
    assert_eq!(slice_ranges(&runs, 6, 13), vec![(6, 13, Some(0))]);
}

#[test]
fn slice_ranges_covers_gaps_with_node_style() {
    // Run only over [4,8); line [0,12) → gap, run, gap.
    let runs = [run(4, 8)];
    assert_eq!(
        slice_ranges(&runs, 0, 12),
        vec![(0, 4, None), (4, 8, Some(0)), (8, 12, None)]
    );
}

#[test]
fn for_each_slice_range_matches_slice_ranges() {
    let runs = [run(4, 8), run(10, 14)];
    let mut iterated = Vec::new();

    for_each_slice_range(&runs, 0, 16, |start, end, run| {
        iterated.push((start, end, run));
    });

    assert_eq!(iterated, slice_ranges(&runs, 0, 16));
}

#[test]
fn justify_spreads_residual_across_space_gaps() {
    // "ab cd ef" has 2 spaces → 12 / 2 = 6 per gap.
    assert_eq!(justify_extra_per_gap(12.0, "ab cd ef"), 6.0);
    // No gaps (CJK / single word) → no distribution.
    assert_eq!(justify_extra_per_gap(12.0, "汉字汉字"), 0.0);
    // Nothing to distribute.
    assert_eq!(justify_extra_per_gap(0.0, "ab cd"), 0.0);
    assert_eq!(justify_extra_per_gap(-3.0, "ab cd"), 0.0);
}

#[derive(Default)]
struct CaptureBackend {
    grayscale: bool,
    text_modes: Vec<bool>,
    origins: Vec<Point2D>,
    contents: Vec<String>,
    weights: Vec<u16>,
    italics: Vec<bool>,
    lines: Vec<(Point2D, Point2D, Color, f32)>,
    fill_rects: Vec<Rect>,
    round_rects: Vec<Rect>,
    requested_baseline: Option<f32>,
}

impl RenderBackend for CaptureBackend {
    fn set_text_grayscale(&mut self, value: bool) { self.grayscale = value; }
    fn begin_frame(&mut self) {}
    fn end_frame(&mut self) {}
    fn fill_rect(&mut self, rect: Rect, _: Color) {
        self.fill_rects.push(rect);
    }
    fn stroke_rect(&mut self, _: Rect, _: Color, _: f32) {}
    fn draw_text(&mut self, layout: &TextLayout, origin: Point2D) {
        self.text_modes.push(self.grayscale);
        self.origins.push(origin);
        self.italics.push(layout.italic());
        if let Some(run) = layout.runs().first() {
            self.contents.push(run.content.clone());
            self.weights.push(run.font_weight);
        }
    }
    fn clip_rect(&mut self, _: Rect) {}
    fn save(&mut self) {}
    fn restore(&mut self) {}
    fn translate(&mut self, _: Point2D) {}
    fn stroke_line(&mut self, from: Point2D, to: Point2D, color: Color, width: f32) {
        self.lines.push((from, to, color, width));
    }
    fn fill_round_rect(&mut self, rect: Rect, _: f32, _: Color) {
        self.round_rects.push(rect);
    }
    fn stroke_round_rect(&mut self, _: Rect, _: f32, _: Color, _: f32) {}
    fn stroke_svg_path(&mut self, _: &str, _: Point2D, _: f32, _: Color, _: f32) {}
    fn resize(&mut self, _: u32, _: u32) {}
    fn dpi_scale(&self) -> f32 {
        1.0
    }
    fn measure_text_weighted(&mut self, text: &str, _: f32, _: u16) -> f32 {
        text.chars().count() as f32 * 10.0
    }
    fn text_first_baseline(&mut self, request: &TextBaselineRequest<'_>) -> f32 {
        if let Some(baseline) = self.requested_baseline {
            assert_eq!(request.text, "🧥 new");
            assert_eq!(request.font_family, "Inter, system-ui, sans-serif");
            assert_eq!((request.font_size, request.font_weight), (24.0, 700));
            assert!(request.italic);
            assert_eq!(request.line_height, 1.5);
            baseline
        } else {
            request.font_size * 0.8
        }
    }
}

fn text_node(content: &str) -> SceneNode {
    let mut node = SceneNode::leaf("t", NodeKind::Text);
    node.bounds = Rect::xywh(0.0, 0.0, 200.0, 80.0);
    node.text = Some(content.to_string());
    node.font_size = 20.0;
    node.line_height = 1.0;
    node
}

#[test]
fn tiny_on_screen_text_greeks_to_a_bar_instead_of_shaping() {
    // 20 px font at 5% zoom is a ~1 px smudge on screen: the full
    // layout (CJK wrap measuring, per-run typeface segmentation)
    // must be skipped — a zoomed-out text-dense page pays it for
    // every text node on every panned frame.
    let node = text_node("hello world");
    let mut backend = CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };

    paint_text_node(&mut cx, &node, node.bounds, 0.05, &None);

    assert!(
        backend.contents.is_empty(),
        "greeked text must not shape or draw glyph runs"
    );
    assert_eq!(backend.fill_rects, vec![node.bounds]);
}

#[test]
fn source_grayscale_is_scoped_to_one_text_node() {
    let mut node = text_node("Same label");
    node.text_grayscale = true;
    let mut backend = CaptureBackend::default();
    paint_text_node(&mut PaintCx { backend: &mut backend }, &node, node.bounds, 1.0, &None);
    assert!(!backend.text_modes.is_empty());
    assert!(backend.text_modes.iter().all(|mode| *mode));
    assert!(!backend.grayscale, "Node paint must restore the default for editor chrome");
    backend.text_modes.clear();
    node.text_grayscale = false;
    paint_text_node(&mut PaintCx { backend: &mut backend }, &node, node.bounds, 1.0, &None);
    assert!(backend.text_modes.iter().all(|mode| !*mode));
}

#[test]
fn editing_text_never_greeks_even_at_tiny_zoom() {
    let node = text_node("ab");
    let edit = EditCaret {
        editing: "t".to_string(),
        input: jian_core::text_input::TextInputState::with_text("ab"),
        now_ms: 0,
        selection_color: Color::BLUE,
    };
    let mut backend = CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };

    paint_text_node(&mut cx, &node, node.bounds, 0.05, &Some(edit));

    assert!(
        !backend.contents.is_empty(),
        "the edited node keeps exact glyph paint for caret parity"
    );
}

#[test]
fn empty_text_paints_nothing_when_greeked() {
    let node = text_node("");
    let mut backend = CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };

    paint_text_node(&mut cx, &node, node.bounds, 0.05, &None);

    assert!(backend.contents.is_empty());
    assert!(backend.fill_rects.is_empty());
}

#[test]
fn composition_paints_inline_preedit_underline_and_caret() {
    let node = text_node("ab");
    let mut input = jian_core::text_input::TextInputState::with_text("ab");
    input.set_caret(1, 0);
    input.set_composition("你", "你".len(), 0);
    let edit = EditCaret {
        editing: "t".to_string(),
        input,
        now_ms: 0,
        selection_color: Color::BLUE,
    };
    let mut backend = CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };

    paint_text_node(&mut cx, &node, node.bounds, 1.0, &Some(edit));

    assert_eq!(backend.contents, vec!["a你b".to_string()]);
    assert_eq!(backend.lines.len(), 1);
    let (from, to, _, _) = backend.lines[0];
    assert_eq!((from.x, to.x), (10.0, 20.0));
    assert_eq!((from.y, to.y), (22.4, 22.4));
    assert_eq!(backend.fill_rects, vec![Rect::xywh(20.0, 2.0, 1.0, 23.0)]);
}

#[test]
fn styled_runs_paint_separate_slices_with_run_weight() {
    let mut node = text_node("boldplain");
    node.text_runs = vec![
        SceneTextRun {
            font_weight: 700,
            ..run(0, 4)
        },
        run(4, 9),
    ];
    let mut backend = CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };

    paint_text_node(&mut cx, &node, node.bounds, 1.0, &None);

    assert_eq!(
        backend.contents,
        vec!["bold".to_string(), "plain".to_string()]
    );
    assert_eq!(backend.weights, vec![700, 400]);
    // Second slice starts after the first's 4-glyph advance.
    assert_eq!(backend.origins[1].x, 40.0);
}

#[test]
fn italic_run_sets_layout_italic_flag() {
    let mut node = text_node("ab");
    node.text_runs = vec![SceneTextRun {
        italic: true,
        ..run(0, 2)
    }];
    let mut backend = CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };

    paint_text_node(&mut cx, &node, node.bounds, 1.0, &None);

    assert_eq!(backend.italics, vec![true]);
}

#[test]
fn authored_line_height_requests_a_text_aware_first_baseline() {
    let mut node = text_node("🧥 new");
    node.font_family = "Inter, system-ui, sans-serif".into();
    node.font_size = 24.0;
    node.font_weight = 700;
    node.italic = true;
    node.line_height = 1.5;
    let mut backend = CaptureBackend {
        requested_baseline: Some(19.25),
        ..CaptureBackend::default()
    };
    let mut cx = PaintCx {
        backend: &mut backend,
    };

    paint_text_node(&mut cx, &node, node.bounds, 1.0, &None);

    assert_eq!(backend.origins[0].y, 19.25);
}

#[test]
fn css_baselines_snap_relative_to_source_root_not_viewport_zoom() {
    for zoom in [0.5, 1.0, 1.25, 2.0] {
        let mut node = SceneNode::leaf("css-text", NodeKind::Text);
        node.text = Some("🧥 new".into());
        node.font_family = "Inter, system-ui, sans-serif".into();
        node.font_size = 24.0;
        node.font_weight = 700;
        node.italic = true;
        node.line_height = 1.5;
        node.bounds.origin = Point2D::new(15.25, 42.8);
        node.bounds.size = Point2D::new(100.0, 30.0);
        node.css_paint_origin = Some(Point2D::new(10.25, 20.5));
        let original_bounds = node.bounds;
        let mut backend = CaptureBackend { requested_baseline: Some(19.25), ..Default::default() };
        let mut cx = PaintCx { backend: &mut backend };
        paint_text_node(&mut cx, &node, node.bounds, zoom, &None);
        assert_eq!(backend.origins[0], Point2D::new(15.25, 62.5));
        assert_eq!(node.bounds, original_bounds);
    }
}

#[test]
fn underline_and_strikethrough_stroke_at_metrics_offsets() {
    let mut node = text_node("abc");
    node.text_runs = vec![SceneTextRun {
        underline: true,
        strikethrough: true,
        ..run(0, 3)
    }];
    let mut backend = CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };

    paint_text_node(&mut cx, &node, node.bounds, 1.0, &None);

    // Default ascent = 0.8 * 20 = 16; underline at +2.4, strike at -6.
    assert_eq!(backend.lines.len(), 2);
    let (u_from, u_to, _, _) = backend.lines[0];
    assert_eq!((u_from.y, u_to.y), (18.4, 18.4));
    assert_eq!((u_from.x, u_to.x), (0.0, 30.0));
    let (s_from, _, _, _) = backend.lines[1];
    assert_eq!(s_from.y, 10.0);
}

#[test]
fn justify_paints_per_char_with_spread_gaps_except_last_line() {
    let mut node = text_node("aa bb\ncc");
    node.text_align = SceneTextAlign::Justify;
    // align_width = 200; line 0 "aa bb" = 50 px → residual 150
    // over 1 gap. Line 1 is the last → left-aligned whole-slice.
    let mut backend = CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };

    paint_text_node(&mut cx, &node, node.bounds, 1.0, &None);

    // Line 0 paints per-char (5 chars), line 1 as one slice.
    assert_eq!(backend.contents.len(), 6);
    // 'b' after the spread space: chars a(0) a(10) ' '(20) → next
    // starts at 30 + 150 = 180.
    assert_eq!(backend.origins[3].x, 180.0);
    assert_eq!(backend.origins[4].x, 190.0);
    // Last line unjustified, starts at the left edge.
    assert_eq!(backend.origins[5].x, 0.0);
    assert_eq!(backend.contents[5], "cc");
}

#[test]
fn negative_letter_spacing_offsets_glyphs_and_line_alignment() {
    let mut node = text_node("NOVA");
    node.letter_spacing = -2.0;
    node.text_align = SceneTextAlign::Center;
    node.bounds.size.x = 100.0;
    let mut backend = CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };

    paint_text_node(&mut cx, &node, node.bounds, 1.0, &None);

    assert_eq!(backend.contents, vec!["N", "O", "V", "A"]);
    // Width = 4*10 + 3*(-2) = 34, centered in 100 => x=33.
    assert_eq!(
        backend.origins.iter().map(|p| p.x).collect::<Vec<_>>(),
        vec![33.0, 41.0, 49.0, 57.0]
    );
}

#[test]
fn segment_boundary_maps_onto_wrapped_lines() {
    // 11-char content wraps at ~6 chars (60 px + tolerance);
    // runs split at byte 8 — the SECOND wrapped line must split
    // into two styled slices at the run boundary.
    let mut node = text_node("hello world");
    node.bounds = Rect::xywh(0.0, 0.0, 60.0, 80.0);
    node.text_wrap = true;
    node.text_runs = vec![
        run(0, 8),
        SceneTextRun {
            font_weight: 700,
            ..run(8, 11)
        },
    ];
    let mut backend = CaptureBackend::default();
    let mut cx = PaintCx {
        backend: &mut backend,
    };

    paint_text_node(&mut cx, &node, node.bounds, 1.0, &None);

    // Wrap → "hello " + "world"; line 1 covers bytes 6..11 and
    // crosses the run boundary at 8.
    assert_eq!(
        backend.contents,
        vec!["hello ".to_string(), "wo".to_string(), "rld".to_string()]
    );
    assert_eq!(backend.weights, vec![400, 400, 700]);
    // "rld" starts after "wo"'s 2-glyph advance on line 1.
    assert_eq!(backend.origins[2].x, 20.0);
}
