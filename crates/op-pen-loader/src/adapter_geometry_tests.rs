//! Geometry / path / fill adapter tests — authored positions, taffy
//! fill resolution, image + gradient fill payloads, flip flags, path
//! anchors + bezier bounds, corner radii, and canvas-offset stacking.
//! Carved off `adapter_tests.rs` to keep every file under the 800-line
//! cap.

use super::*;

#[test]
fn css_layout_preserves_fractional_positions_and_flex_sizing() {
    let loaded = load(
        r#"{"version":"1.0.0","conversion":{"entries":[
      {"kind":"screen","key":"html-snapshot:v1:fixture","nodeId":"root"}
    ]},"children":[{
      "type":"frame","id":"root","x":10.25,"y":20.5,
      "width":301.5,"height":200,"layout":"none","children":[
        {"type":"frame","id":"positioned","x":15.25,"y":42.8,
         "width":101.5,"height":30.25,"layout":"horizontal","children":[
          {"type":"rectangle","id":"fill","width":"fill_container","height":12.5}
        ]}
      ]}]}"#,
    );
    let parent = &loaded.payload.pages[0].children[0].children[0];
    assert!((parent.x - 25.5).abs() < 0.001, "x={}", parent.x);
    assert!((parent.y - 63.3).abs() < 0.001, "y={}", parent.y);
    assert!((parent.w - 101.5).abs() < 0.001);
    let fill = &parent.children[0];
    assert!((fill.w - 101.5).abs() < 0.001, "fill width={}", fill.w);
    assert!((fill.h - 12.5).abs() < 0.001);
}

#[test]
fn css_layout_requires_exact_versioned_screen_and_root_mapping() {
    for (kind, key, id, enabled) in [
        ("screen", "html-snapshot:v1:fixture", "root", true),
        ("component", "html-snapshot:v1:fixture", "root", false),
        ("screen", "html-snapshot:v2:fixture", "root", false),
        ("screen", "html-snapshot:v1:", "root", false),
        ("screen", "route:/dashboard", "root", false),
        ("screen", "html-snapshot:v1:fixture", "other", false),
    ] {
        let doc = serde_json::json!({
            "version":"1.0.0",
            "conversion":{"entries":[{"kind":kind,"key":key,"nodeId":id}]},
            "children":[{"type":"frame","id":"root","width":100.5,"height":60.25}]
        });
        let loaded = load(&doc.to_string());
        let root = &loaded.payload.pages[0].children[0];
        assert_eq!(
            root.w,
            if enabled { 100.5 } else { 101.0 },
            "{kind}/{key}/{id}"
        );
        assert_eq!(
            root.h,
            if enabled { 60.25 } else { 60.0 },
            "{kind}/{key}/{id}"
        );
    }
}

#[test]
fn preserving_geometry_keeps_authored_nested_positions() {
    let src = r##"{
      "version":"1.0.0",
      "pages":[{
        "id":"p1","name":"Page 1",
        "children":[{
          "type":"frame","id":"root","x":100,"y":200,"width":300,"height":200,
          "layout":"horizontal","gap":99,
          "children":[
            {"type":"rectangle","id":"r1","x":10,"y":20,"width":30,"height":40,
             "fill":[{"type":"solid","color":"#000000"}]},
            {"type":"line","id":"l1","x":5,"y":6,"x2":10,"y2":0}
          ]
        }]
      }],
      "children":[]
    }"##;
    let r = jian_ops_schema::load_str(src).unwrap();
    let loaded = pen_document_to_payload_preserving_geometry(&r.value);
    let root = &loaded.payload.pages[0].children[0];
    let rect = &root.children[0];
    let line = &root.children[1];

    assert_eq!(
        (root.x, root.y, root.w, root.h),
        (100.0, 200.0, 300.0, 200.0)
    );
    assert_eq!((rect.x, rect.y, rect.w, rect.h), (110.0, 220.0, 30.0, 40.0));
    assert_eq!((line.x, line.y, line.w, line.h), (105.0, 206.0, 10.0, 0.0));
}

#[test]
fn path_mask_marker_reaches_payload() {
    let src = r#"{
      "version":"1.0.0","pages":[{"id":"p","name":"P","children":[
        {"type":"path","id":"mask","width":10,"height":10,
         "d":"M0 0H10V10H0Z","mask":true}
      ]}],"children":[]
    }"#;
    let loaded = load(src);
    let mask = &loaded.payload.pages[0].children[0];
    assert!(mask.is_mask);
    assert_eq!(mask.mask_type, Some(jian_ops_schema::node::MaskType::Alpha));
}

#[test]
fn shared_mask_type_reaches_payload_on_a_frame() {
    let src = r#"{
      "version":"1.0.0","pages":[{"id":"p","name":"P","children":[
        {"type":"frame","id":"mask","width":10,"height":10,
         "maskType":"luminance","children":[]}
      ]}],"children":[]
    }"#;
    let loaded = load(src);
    let mask = &loaded.payload.pages[0].children[0];
    assert!(mask.is_mask);
    assert_eq!(
        mask.mask_type,
        Some(jian_ops_schema::node::MaskType::Luminance)
    );
}

#[test]
fn shared_node_blend_mode_reaches_payload() {
    let src = r#"{
      "version":"1.0.0","pages":[{"id":"p","name":"P","children":[
        {"type":"frame","id":"blend","width":10,"height":10,
         "blendMode":"soft_light","children":[]}
      ]}],"children":[]
    }"#;
    let loaded = load(src);
    assert_eq!(
        loaded.payload.pages[0].children[0].blend_mode,
        Some(jian_ops_schema::style::BlendMode::SoftLight)
    );
}

#[test]
fn minimal_empty_doc() {
    let r = load(r#"{"version":"1.0.0","children":[]}"#);
    assert_eq!(r.payload.pages.len(), 1);
    assert!(r.payload.pages[0].children.is_empty());
}

#[test]
fn fill_container_resolves_via_taffy() {
    let src = r##"{
      "version":"1.0.0",
      "pages":[{
        "id":"p1","name":"Page 1",
        "children":[{
          "type":"frame","id":"root","width":375,"height":812,
          "layout":"vertical","gap":16,
          "children":[
            {"type":"rectangle","id":"r1","width":"fill_container","height":40,
             "fill":[{"type":"solid","color":"#000000"}]}
          ]
        }]
      }],
      "children":[]
    }"##;
    let r = load(src);
    let root = &r.payload.pages[0].children[0];
    let inner = &root.children[0];
    assert_eq!(inner.w, 375.0, "fill_container should stretch via taffy");
    assert_eq!(inner.h, 40.0);
}

#[test]
fn image_fill_payload_carries_fit_mode() {
    let src = r##"{
      "version":"1.0.0",
      "pages":[{
        "id":"p","name":"P",
        "children":[{
          "type":"rectangle","id":"r","width":360,"height":240,
          "fill":[{"type":"image","url":"data:image/png;base64,AA==","mode":"tile"}]
        }]
      }],
      "children":[]
    }"##;
    let r = load(src);
    let n = &r.payload.pages[0].children[0];
    assert_eq!(n.image_src.as_deref(), Some("data:image/png;base64,AA=="));
    assert_eq!(n.image_fit.as_deref(), Some("tile"));
    assert_eq!(n.image_tile_scale, None);
}

#[test]
fn image_fill_payload_carries_only_positive_tile_scale() {
    let load_scale = |scale: &str| {
        let src = format!(
            r#"{{
              "version":"1.0.0","pages":[{{"id":"p","name":"P","children":[{{
                "type":"rectangle","id":"r","width":220,"height":220,
                "fill":[{{"type":"image","url":"data:image/png;base64,AA==",
                  "mode":"tile","originalSize":{{"width":4096,"height":2048}},
                  "tileScale":{scale}}}]
              }}]}}],"children":[]
            }}"#
        );
        let payload = &load(&src).payload.pages[0].children[0];
        assert_eq!(payload.image_original_size, Some([4096.0, 2048.0]));
        payload.image_tile_scale
    };

    assert_eq!(load_scale("0.38618907"), Some(0.38618907));
    assert_eq!(load_scale("0"), None);
    assert_eq!(load_scale("-2"), None);
}

#[test]
fn image_fill_payload_carries_adjustments() {
    let src = r##"{
      "version":"1.0.0",
      "pages":[{
        "id":"p","name":"P",
        "children":[{
          "type":"rectangle","id":"r","width":360,"height":240,
          "fill":[{"type":"image","url":"data:image/png;base64,AA==",
            "mode":"fit","exposure":100,"contrast":-100,"saturation":50,
            "temperature":25,"tint":-25,"highlights":75,"shadows":-75}]
        }]
      }],
      "children":[]
    }"##;
    let r = load(src);
    let a = r.payload.pages[0].children[0]
        .image_adjustments
        .expect("image adjustments must be carried");
    assert_eq!(a.exposure, 100.0);
    assert_eq!(a.contrast, -100.0);
    assert_eq!(a.saturation, 50.0);
    assert_eq!(a.temperature, 25.0);
    assert_eq!(a.tint, -25.0);
    assert_eq!(a.highlights, 75.0);
    assert_eq!(a.shadows, -75.0);
}

#[test]
fn payload_preserves_node_flip_flags() {
    let src = r##"{
      "version":"1.0.0",
      "pages":[{
        "id":"p","name":"P",
        "children":[{
          "type":"rectangle","id":"r","width":100,"height":50,
          "flipX":true,"flipY":true,
          "fill":[{"type":"solid","color":"#000000"}]
        }]
      }],
      "children":[]
    }"##;
    let r = load(src);
    let n = &r.payload.pages[0].children[0];
    assert!(n.flip_x, "flipX must survive into the paint payload");
    assert!(n.flip_y, "flipY must survive into the paint payload");
}

#[test]
fn path_anchors_absolutize_to_canvas_coords() {
    // Canonical `PathNode.anchors` are authored local to the
    // path's `base.x`/`base.y` *and* scaled to its `width`/
    // `height` per `pen-renderer/node-renderer.ts::drawPath`.
    // The shell renderer treats `Node.points` as canvas-abs,
    // so the adapter must apply `(x + (anchor.x - min_x) * sx,
    // …)` — pure translate misses both non-zero anchor minima
    // and explicit-size scaling.
    let src = r##"{
      "version":"1.0.0","pages":[{"id":"p","name":"P","children":[
        {"type":"frame","id":"root","x":-1000,"y":2000,"width":400,"height":200,
         "children":[
           {"type":"path","id":"p1","x":20,"y":30,"width":100,"height":50,
            "anchors":[
              {"x":0,"y":0,"handleIn":null,"handleOut":null},
              {"x":100,"y":50,"handleIn":null,"handleOut":null}
            ]}
         ]}
      ]}],"children":[]
    }"##;
    let r = load(src);
    let path_node = &r.payload.pages[0].children[0].children[0];
    assert_eq!(path_node.x, -980.0);
    assert_eq!(path_node.y, 2030.0);
    // anchor bbox (0..100, 0..50) == size (100, 50), so sx=sy=1.
    assert_eq!(path_node.points[0], [-980.0, 2030.0]);
    assert_eq!(path_node.points[1], [-880.0, 2080.0]);
}

#[test]
fn path_payload_carries_even_odd_fill_rule() {
    let src = r#"{
      "version":"1.0.0","pages":[{"id":"p","name":"P","children":[
        {"type":"path","id":"ring","width":100,"height":100,
         "d":"M0 0H100V100H0Z M25 25H75V75H25Z","fillRule":"evenodd"}
      ]}],"children":[]
    }"#;
    let loaded = load(src);
    assert!(loaded.payload.pages[0].children[0].even_odd_fill);
}

#[test]
fn per_corner_radius_payload_keeps_array_and_legacy_maximum() {
    let src = r#"{
      "version":"1.0.0","pages":[{"id":"p","name":"P","children":[
        {"type":"rectangle","id":"r","width":100,"height":50,
         "cornerRadius":[8,0,6,2]}
      ]}],"children":[]
    }"#;
    let loaded = load(src);
    let rect = &loaded.payload.pages[0].children[0];
    assert_eq!(rect.corner_radii, Some([8.0, 0.0, 6.0, 2.0]));
    assert_eq!(rect.corner_radius, 8.0);
}

#[test]
fn path_bounds_include_bezier_curve_extrema() {
    // Two anchors at y=0 with NEGATIVE-y handles (handle.y=-60).
    // The cubic Bezier reaches y=-45 at t=0.5, so native bbox is
    // min_y=-45, max_y=0, native_h=45. Authored height=90 →
    // sy = 90/45 = 2.0. Anchor at native y=0 shifts by
    // (0 - (-45)) = 45, scales by 2 = 90, plus path.y=100 = 190.
    //
    // Endpoint-only bounds would give native_h=0 → sy=1 →
    // anchor.y = path.y = 100. So asserting 190 forces the
    // curve-extrema path through `cubic_derivative_roots`.
    let src = r##"{
      "version":"1.0.0","pages":[{"id":"p","name":"P","children":[
        {"type":"path","id":"curve","x":0,"y":100,"width":100,"height":90,
         "anchors":[
           {"x":0,"y":0,"handleIn":null,"handleOut":{"x":0,"y":-60}},
           {"x":100,"y":0,"handleIn":{"x":0,"y":-60},"handleOut":null}
         ]}
      ]}],"children":[]
    }"##;
    let r = load(src);
    let n = &r.payload.pages[0].children[0];
    // `points` stays 1:1 with the two schema anchors; the curve-aware
    // bounds come from `absolutize_path_anchors`'s native span.
    let p0 = n.points[0];
    let p1 = n.points[1];
    // x span [0, 100] → sx=1. Anchors keep their x.
    assert!((p0[0] - 0.0).abs() < 0.01, "anchor[0].x");
    assert!((p1[0] - 100.0).abs() < 0.01, "anchor[1].x");
    // y: 190 with extrema-aware bounds, 100 with endpoint-only.
    assert!(
        (p0[1] - 190.0).abs() < 0.5,
        "anchor[0].y expected ~190 (was {}). Endpoint-only bbox would give 100.",
        p0[1]
    );
    assert!(
        (p1[1] - 190.0).abs() < 0.5,
        "anchor[1].y expected ~190 (was {}). Endpoint-only bbox would give 100.",
        p1[1]
    );
}

#[test]
fn path_anchors_honor_nonzero_minima_and_scale() {
    // Anchors at (10, 20) → (30, 80) with authored width=200,
    // height=300. Native span = (20, 60), so sx=10, sy=5.
    // First anchor maps to path.x/y; second anchor maps to
    // path.x + 200, path.y + 300.
    let src = r##"{
      "version":"1.0.0","pages":[{"id":"p","name":"P","children":[
        {"type":"path","id":"p1","x":50,"y":60,"width":200,"height":300,
         "anchors":[
           {"x":10,"y":20,"handleIn":null,"handleOut":null},
           {"x":30,"y":80,"handleIn":null,"handleOut":null}
         ]}
      ]}],"children":[]
    }"##;
    let r = load(src);
    let n = &r.payload.pages[0].children[0];
    assert_eq!(n.points[0], [50.0, 60.0], "first anchor maps to origin");
    assert_eq!(n.points[1], [250.0, 360.0], "scaled to width/height");
}

#[test]
fn path_d_carries_to_payload_for_svg_painting() {
    let src = r##"{
      "version":"1.0.0","pages":[{"id":"p","name":"P","children":[
        {"type":"path","id":"arc","x":10,"y":20,"width":80,"height":40,
         "d":"M0 20 A40 40 0 0 1 80 20 Z",
         "fill":[{"type":"solid","color":"#000000"}]}
      ]}],"children":[]
    }"##;
    let r = load(src);
    let n = &r.payload.pages[0].children[0];
    assert_eq!(n.svg_path.as_deref(), Some("M0 20 A40 40 0 0 1 80 20 Z"));
    assert!(n.points.is_empty());
    assert!(n.path_anchors.is_empty());
}

#[test]
fn shape_inside_offset_root_inherits_canvas_offset() {
    // The zero-size shape fallback used to keep `(x, y)` from
    // `base_payload` (= authored local coords inside the parent
    // frame), so an ellipse at local (20, 30) inside a root at
    // canvas (-1000, 2000) painted at world (20, 30) instead of
    // (-980, 2030) — detaching it from its parent design.
    let src = r##"{
      "version":"1.0.0","pages":[{"id":"p","name":"P","children":[
        {"type":"frame","id":"root","x":-1000,"y":2000,"width":400,"height":200,
         "children":[
           {"type":"ellipse","id":"dot","x":20,"y":30,"width":40,"height":40}
         ]}
      ]}],"children":[]
    }"##;
    let r = load(src);
    let root = &r.payload.pages[0].children[0];
    assert_eq!((root.x, root.y), (-1000.0, 2000.0));
    let dot = &root.children[0];
    assert_eq!(dot.x, -980.0, "ellipse x must reflect root offset");
    assert_eq!(dot.y, 2030.0, "ellipse y must reflect root offset");
    assert_eq!((dot.w, dot.h), (40.0, 40.0), "authored size preserved");
}

#[test]
fn multi_root_designs_use_authored_canvas_offset() {
    // pencil-demo.op's 14 designs sit at distinct (x, y) on the
    // infinite canvas. Each root's taffy layout starts at (0, 0)
    // so we must offset the harvested rects by the root's
    // authored `base.x` / `base.y` — otherwise every design
    // collapses to origin and they all overlap.
    let src = r##"{
      "version":"1.0.0","pages":[{"id":"p","name":"P","children":[
        {"type":"frame","id":"a","x":100,"y":50,"width":200,"height":100,
         "fill":[{"type":"solid","color":"#FF0000"}]},
        {"type":"frame","id":"b","x":-500,"y":2000,"width":200,"height":100,
         "fill":[{"type":"solid","color":"#00FF00"}]}
      ]}],"children":[]
    }"##;
    let r = load(src);
    let kids = &r.payload.pages[0].children;
    assert_eq!(kids[0].x, 100.0);
    assert_eq!(kids[0].y, 50.0);
    assert_eq!(kids[1].x, -500.0);
    assert_eq!(kids[1].y, 2000.0);
}

#[test]
fn space_between_preserves_authored_positions() {
    // Mirror pencil-demo.op's sidebar: vertical flex with
    // space_between authored on the parent. Authored positions
    // on children (Top at y=0, Bottom at y=600) must survive
    // the layout pass — jian-core's `node_to_style` treats
    // explicit x/y as `Position::Absolute`.
    let src = r##"{
      "version":"1.0.0","pages":[{"id":"p","name":"P","children":[{
        "type":"frame","id":"sidebar","width":240,"height":800,
        "layout":"vertical","justifyContent":"space_between",
        "children":[
          {"type":"frame","id":"top","width":240,"height":100,"x":0,"y":0},
          {"type":"frame","id":"bot","width":240,"height":100,"x":0,"y":700}
        ]
      }]}],"children":[]
    }"##;
    let r = load(src);
    let sidebar = &r.payload.pages[0].children[0];
    let top = &sidebar.children[0];
    let bot = &sidebar.children[1];
    assert_eq!(top.y, 0.0, "top authored at y=0");
    assert_eq!(
        bot.y, 700.0,
        "bottom authored at y=700 must NOT be restacked"
    );
}
