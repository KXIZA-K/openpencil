//! Unit tests for the CanvasKit backend + JS bridge contract.
//!
//! Split out of `canvaskit.rs`. The `include_str!` paths are relative to this
//! file, so the bridge JS is reached through `../` and the backend source
//! (`image_resident`) through its own sibling module.

use op_editor_ui::{Color, ImageBlendMode};

use super::convert::{
    display_dpr, flatten_gradient_colors, flatten_gradient_stops, image_blend_mode_code,
    normalized_tile_scale, valid_original_size,
};

#[test]
fn image_decode_budget_is_shared_by_all_backend_consumers() {
    let backend = include_str!("backend.rs");
    let frame = backend.split("fn begin_frame(&mut self)").nth(1).unwrap();
    let frame = frame.split("fn end_frame").next().unwrap();
    assert!(frame.contains("self.drain_pending_decodes(2)"));
    assert!(frame.find("drain_pending_decodes").unwrap() < frame.find("self.ck.begin_frame").unwrap());
    assert!(!include_str!("inner.rs").contains("drain_pending_decodes"));
}

#[test]
fn drop_shadow_uses_isolated_blur_paint() {
    let backend = include_str!("backend.rs");
    assert!(backend.contains("fn fill_drop_shadow(&mut self"));
    let bridge = include_str!("../op_ck_bridge.js");
    let shadow = bridge.split("fillDropShadow(").nth(1).unwrap().split("\n    },").next().unwrap();
    assert!(shadow.contains("allocFillPaint("));
    assert!(shadow.contains("MaskFilter.MakeBlur"));
    assert!(shadow.contains("blur) * 0.5"));
    assert!(shadow.contains("paint.delete()"));
    assert!(shadow.contains("mask.delete()"));
    assert!(!shadow.contains("= fillPaint("));
}

#[test]
fn rounded_gradient_ffi_preserves_stop_and_vertex_alpha() {
    let transparent = Color {
        r: 1.0,
        g: 1.0,
        b: 1.0,
        a: 0.0,
    };
    let opaque = Color {
        r: 0.25,
        g: 0.5,
        b: 0.75,
        a: 1.0,
    };

    assert_eq!(
        flatten_gradient_stops(&[(0.0, transparent), (1.0, opaque)]),
        vec![0.0, 1.0, 1.0, 1.0, 0.0, 1.0, 0.25, 0.5, 0.75, 1.0]
    );
    assert_eq!(
        flatten_gradient_colors(&[transparent, opaque]),
        vec![1.0, 1.0, 1.0, 0.0, 0.25, 0.5, 0.75, 1.0]
    );
}

#[test]
fn canvaskit_draw_path_only_reads_predecoded_images() {
    let bridge = include_str!("../op_ck_bridge.js");
    let draw_start = bridge
        .find("drawImageWithOptions(")
        .expect("draw bridge method");
    let draw_end = bridge[draw_start..]
        .find("\n    },")
        .expect("draw bridge end")
        + draw_start;
    let draw = &bridge[draw_start..draw_end];
    assert!(draw.contains("imageCaches.fullImage(imageIdLo, imageIdHi)"));
    assert!(!draw.contains("MakeImageFromEncoded"));
}

#[test]
fn tile_scale_is_forwarded_and_bridge_bounds_repetition() {
    assert_eq!(normalized_tile_scale(0.38618907), 0.38618907);
    assert_eq!(normalized_tile_scale(0.0), 1.0);
    assert_eq!(normalized_tile_scale(f32::NAN), 1.0);
    assert_eq!(normalized_tile_scale(f32::INFINITY), 1.0);
    assert_eq!(
        valid_original_size(Some([4096.0, 2048.0])),
        Some([4096.0, 2048.0])
    );
    assert_eq!(valid_original_size(Some([0.0, 2048.0])), None);
    assert_eq!(valid_original_size(Some([f32::NAN, 2048.0])), None);

    let bridge = include_str!("../op_ck_bridge.js");
    let start = bridge
        .find("drawImageWithOptions(")
        .expect("image bridge method");
    let end = bridge[start..].find("\n    },").expect("image bridge end") + start;
    let method = &bridge[start..end];
    assert!(method.contains("mode === 3 ? null : figmaImageLocalMatrix"));
    assert!(method.contains("Number.isFinite(tileScale) && tileScale > 0"));
    assert!(method.contains("originalWidth > 0 ? originalWidth : imageW"));
    assert!(method.contains("originalHeight > 0 ? originalHeight : imageH"));
    assert!(method.contains("const maxRepeatsPerAxis = 128"));
    assert!(method.contains("const tileW = sourceW * safeTileScale"));
    assert!(method.contains("const tileH = sourceH * safeTileScale"));
    assert!(method.contains("(w - tileW) / 2"));
    assert!(method.contains("if (!(nextX > ix)) break"));
    assert!(method.contains("if (!(nextY > iy)) break"));
}

#[test]
fn composite_layer_bridge_is_bounded_and_never_degrades_to_plain_save() {
    let bridge = include_str!("../op_ck_bridge.js");
    let start = bridge
        .find("pushCompositeLayer(")
        .expect("composite layer bridge method");
    let end = bridge[start..]
        .find("\n    },")
        .expect("composite layer bridge end")
        + start;
    let method = &bridge[start..end];
    assert!(method.contains("paint.setAlphaf("));
    assert!(method.contains("paint.setBlendMode("));
    assert!(method.contains("canvas.saveLayer(paint, CK.LTRBRect("));
    assert!(method.contains("paint.delete()"));
    assert!(!method.contains("canvas.save();"));
}

#[test]
fn blend_mode_codes_are_stable_and_extended_modes_match_the_bridge() {
    let modes = [
        ImageBlendMode::Normal,
        ImageBlendMode::Darken,
        ImageBlendMode::Multiply,
        ImageBlendMode::Screen,
        ImageBlendMode::Overlay,
        ImageBlendMode::Lighten,
        ImageBlendMode::Difference,
        ImageBlendMode::Hue,
        ImageBlendMode::Saturation,
        ImageBlendMode::Color,
        ImageBlendMode::Luminosity,
        ImageBlendMode::SoftLight,
        ImageBlendMode::ColorDodge,
        ImageBlendMode::ColorBurn,
        ImageBlendMode::HardLight,
        ImageBlendMode::Exclusion,
    ];
    assert_eq!(
        modes.map(image_blend_mode_code),
        [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]
    );

    let bridge = include_str!("../op_ck_bridge.js");
    let start = bridge
        .find("const blendModeForCode =")
        .expect("blend mode bridge table");
    let end = bridge[start..]
        .find("][blendMode]")
        .expect("blend mode bridge table end")
        + start;
    let table = &bridge[start..end];
    let mut cursor = 0;
    for name in [
        "CK.BlendMode.SoftLight",
        "CK.BlendMode.ColorDodge",
        "CK.BlendMode.ColorBurn",
        "CK.BlendMode.HardLight",
        "CK.BlendMode.Exclusion",
    ] {
        let offset = table[cursor..].find(name).expect(name);
        cursor += offset + name.len();
    }
}

#[test]
fn mask_source_bridge_uses_dst_in_and_optional_luma_filter() {
    let bridge = include_str!("../op_ck_bridge.js");
    let start = bridge
        .find("pushMaskSourceLayer(")
        .expect("mask source bridge method");
    let end = bridge[start..]
        .find("\n    },")
        .expect("mask source bridge end")
        + start;
    let method = &bridge[start..end];
    assert!(method.contains("CK.BlendMode.DstIn"));
    assert!(method.contains("CK.ColorFilter.MakeLuma()"));
    assert!(method.contains("canvas.saveLayer(paint)"));
    assert!(!method.contains("canvas.clipPath"));
}

#[test]
fn svg_path_fitting_prefers_tight_bounds_with_a_compatibility_fallback() {
    let bridge = include_str!("../op_ck_bridge.js");
    let start = bridge
        .find("const fitPathToRect =")
        .expect("SVG path fitting helper");
    let end = bridge[start..]
        .find("\n\n  // Parsed-SVG-path cache")
        .expect("SVG path fitting helper end")
        + start;
    let helper = &bridge[start..end];
    let tight = helper
        .find("path.computeTightBounds()")
        .expect("tight bounds call");
    let loose = helper
        .find("path.getBounds()")
        .expect("loose bounds fallback");

    assert!(tight < loose, "tight bounds must be attempted first");
    assert!(helper.contains("if (!pathIsFinite(bounds))"));
}

#[test]
fn image_residency_uses_the_real_canvaskit_cache() {
    let backend = include_str!("backend.rs");
    let start = backend
        .find("fn image_resident(&mut self, image_id: u64)")
        .expect("CanvasKit residency override");
    let method = &backend[start..start + 360.min(backend.len() - start)];
    assert!(method.contains(".image_decoded("));
    assert!(method.contains(", 0)"));
}

#[test]
fn display_dpr_uses_native_resolution_above_zoom_out_floor() {
    for native in [1.0, 1.25, 1.5, 2.0, 3.0] {
        for zoom in [0.01, 0.5, 0.99] {
            assert_eq!(display_dpr(native, zoom), native.max(2.0));
        }
        for zoom in [1.0, 1.01, 2.0, 100.0] {
            assert_eq!(display_dpr(native, zoom), native);
        }
    }
}

#[test]
fn display_dpr_sanitizes_invalid_or_sub_one_values() {
    assert_eq!(display_dpr(f32::NAN, 1.0), 2.0);
    assert_eq!(display_dpr(0.0, 1.0), 2.0);
    assert_eq!(display_dpr(0.75, 1.0), 2.0);
    assert_eq!(display_dpr(1.0, f32::NAN), 2.0);
    assert_eq!(display_dpr(1.0, 0.0), 2.0);
}
