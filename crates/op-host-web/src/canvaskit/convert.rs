//! Small pure conversion helpers for the CanvasKit backend.
//!
//! Split out of `canvaskit.rs`: DPR resolution, gradient-stop flattening, and
//! the enum → wire-code mappings the flat `OpCk` FFI takes as scalars.

use op_editor_ui::{Color, ImageBlendMode, ImageDrawMode};

/// Bounded backing-store floor while zoomed out.
const MIN_WEB_RENDER_DPR: f32 = 2.0;

/// Shared editor/player resolution policy: native DPR at zoom >= 1, and a
/// bounded 2x quality floor below it. Never render below the physical display.
pub fn display_dpr(native_dpr: f32, zoom: f32) -> f32 {
    if !native_dpr.is_finite() || native_dpr < 1.0 || !zoom.is_finite() || zoom <= 0.0 {
        return MIN_WEB_RENDER_DPR;
    }
    // At native-size or larger, avoid compositor downsampling of browser text
    // masks. Below 100%, preserve the measured 2x zoom-out quality floor.
    // Never allocate a backing store proportional to 1/zoom.
    native_dpr.max(if zoom < 1.0 { MIN_WEB_RENDER_DPR } else { 1.0 })
}

pub(super) fn flatten_gradient_stops(stops: &[(f32, Color)]) -> Vec<f32> {
    let mut flat = Vec::with_capacity(stops.len() * 5);
    for (offset, color) in stops {
        flat.extend([*offset, color.r, color.g, color.b, color.a]);
    }
    flat
}

pub(super) fn flatten_gradient_colors(colors: &[Color]) -> Vec<f32> {
    let mut flat = Vec::with_capacity(colors.len() * 4);
    for color in colors {
        flat.extend([color.r, color.g, color.b, color.a]);
    }
    flat
}

pub(super) fn image_draw_mode_code(mode: ImageDrawMode) -> u8 {
    match mode {
        ImageDrawMode::Fill => 0,
        ImageDrawMode::Fit => 1,
        ImageDrawMode::Crop => 2,
        ImageDrawMode::Tile => 3,
        ImageDrawMode::Stretch => 4,
        ImageDrawMode::CssRepeat => 5,
    }
}

pub(super) fn normalized_tile_scale(scale: f32) -> f32 {
    if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    }
}

pub(super) fn valid_original_size(size: Option<[f32; 2]>) -> Option<[f32; 2]> {
    size.filter(|[width, height]| {
        width.is_finite() && height.is_finite() && *width > 0.0 && *height > 0.0
    })
}

pub(super) fn image_blend_mode_code(mode: ImageBlendMode) -> u8 {
    match mode {
        ImageBlendMode::Normal => 0,
        ImageBlendMode::Darken => 1,
        ImageBlendMode::Multiply => 2,
        ImageBlendMode::Screen => 3,
        ImageBlendMode::Overlay => 4,
        ImageBlendMode::Lighten => 5,
        ImageBlendMode::Difference => 6,
        ImageBlendMode::Hue => 7,
        ImageBlendMode::Saturation => 8,
        ImageBlendMode::Color => 9,
        ImageBlendMode::Luminosity => 10,
        ImageBlendMode::SoftLight => 11,
        ImageBlendMode::ColorDodge => 12,
        ImageBlendMode::ColorBurn => 13,
        ImageBlendMode::HardLight => 14,
        ImageBlendMode::Exclusion => 15,
    }
}

pub(super) fn svg_path_even_odd(d: &str) -> bool {
    d.matches(['Z', 'z']).count() > 1
}
