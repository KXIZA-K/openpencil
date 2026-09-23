//! `background-size` / `background-position` → image-fill placement.
//!
//! `ImageFillBody` can express more than the mode enum suggests. Its
//! `transform` maps normalized node coordinates to normalized source UV, so a
//! `no-repeat` layer with an explicit size and any position is exactly
//! representable: the transform's unit square is the visible image window and
//! the renderer decals everything outside it, which is precisely what
//! `background-repeat: no-repeat` paints.
//!
//! What stays out of reach is anything that needs the bitmap's intrinsic size,
//! which the importer does not decode: `background-size: auto`, a single-axis
//! size, and `cover` / `contain` combined with a non-centred position. Those
//! keep the closest mode and report the drop.

use crate::import_warning::ImportWarning;
use jian_ops_schema::style::{ImageFillMode, ImageTransform};

use crate::css::cascade::ComputedStyle;
use crate::length::parse_length;
use crate::mapper::MapCtx;

use super::{layer_value, length_context, split_top_level};

/// Placement resolved for one `background-image` layer.
pub(super) struct LayerGeometry {
    pub mode: Option<ImageFillMode>,
    pub transform: Option<ImageTransform>,
    /// The layer's `background-position` could not be honoured.
    pub position_dropped: bool,
}

/// Resolve one `url()` background layer against the element's used box.
///
/// `definite` reports, per axis, whether `node` really is the element's used
/// size rather than the containing block standing in for an unresolved `auto`.
pub(super) fn image_layer(
    style: &ComputedStyle,
    index: usize,
    node: (f64, f64),
    definite: (bool, bool),
    context: &mut MapCtx<'_>,
) -> LayerGeometry {
    let size = layer_property(style, "background-size", index, "auto");
    let position = layer_property(style, "background-position", index, "0% 0%");
    let repeat = layer_property(style, "background-repeat", index, "repeat");
    if repeat != "no-repeat" {
        if repeat != "repeat" {
            context.warn_once(ImportWarning::BackgroundRepeatApproximated);
        }
        if !matches!(size.as_str(), "auto" | "auto auto") {
            context.warn_once(ImportWarning::BackgroundTileSizeIgnored);
        }
        return LayerGeometry {
            mode: Some(if repeat == "repeat" && position_is_default(&position)
                && matches!(size.as_str(), "auto" | "auto auto") {
                ImageFillMode::CssRepeat
            } else {
                ImageFillMode::Tile
            }),
            transform: None,
            position_dropped: !position_is_default(&position),
        };
    }
    match size.as_str() {
        "cover" | "contain" => {
            let mode = if size == "cover" {
                ImageFillMode::Fill
            } else {
                ImageFillMode::Fit
            };
            // Both keywords centre the scaled bitmap, which is also what the
            // renderer does for `Fill` / `Fit`. Only an authored, non-centred
            // position is lost — the scale factor it would be measured against
            // needs the bitmap's intrinsic aspect ratio.
            LayerGeometry {
                mode: Some(mode),
                transform: None,
                position_dropped: !position_is_default(&position)
                    && !position_is_centered(&position),
            }
        }
        _ => match explicit_size(&size, node, style, context) {
            // The crop transform maps the NODE's box onto the bitmap, so it is
            // only the layer the author wrote when that box is the element's
            // real used size. On an `auto` axis — which is nearly every
            // element's height — `node` carries the containing block instead,
            // and the resulting decal window lands far outside the node: the
            // icon simply never paints. Fall back to the closest whole-box
            // placement and report it, exactly as an unresolvable size does.
            Some(image) if definite == (true, true) => {
                sized_layer(&position, node, image, style, context)
            }
            Some(_) => {
                context.warn_once(ImportWarning::BackgroundSizeAutoBox);
                LayerGeometry {
                    mode: Some(ImageFillMode::Fill),
                    transform: None,
                    position_dropped: !position_is_default(&position),
                }
            }
            None => {
                if !matches!(size.as_str(), "auto" | "auto auto") {
                    context.warn_once(ImportWarning::BackgroundSizeNeedsIntrinsicSize);
                }
                LayerGeometry {
                    mode: Some(ImageFillMode::Fill),
                    transform: None,
                    position_dropped: !position_is_default(&position),
                }
            }
        },
    }
}

/// An explicitly sized, non-repeating layer: exactly representable.
fn sized_layer(
    position: &str,
    node: (f64, f64),
    image: (f64, f64),
    style: &ComputedStyle,
    context: &mut MapCtx<'_>,
) -> LayerGeometry {
    let Some(origin) = resolve_position(position, node, image, style, context) else {
        context.warn_once(ImportWarning::BackgroundPositionUnsupported);
        return LayerGeometry {
            mode: Some(ImageFillMode::Stretch),
            transform: None,
            position_dropped: true,
        };
    };
    let transform = ImageTransform {
        m00: (node.0 / image.0) as f32,
        m01: 0.0,
        m02: (-origin.0 / image.0) as f32,
        m10: 0.0,
        m11: (node.1 / image.1) as f32,
        m12: (-origin.1 / image.1) as f32,
    };
    if !transform_is_finite(&transform) {
        return LayerGeometry {
            mode: Some(ImageFillMode::Fill),
            transform: None,
            position_dropped: true,
        };
    }
    // `background-size: 100% 100%` at the origin is a plain stretch; keeping
    // the identity transform out of the document leaves the fill editable as
    // an ordinary placement rather than a crop.
    if is_identity(&transform) {
        return LayerGeometry {
            mode: Some(ImageFillMode::Stretch),
            transform: None,
            position_dropped: false,
        };
    }
    LayerGeometry {
        mode: Some(ImageFillMode::Crop),
        transform: Some(transform),
        position_dropped: false,
    }
}

/// The painted size of the layer in CSS px, when both axes are authored.
fn explicit_size(
    value: &str,
    node: (f64, f64),
    style: &ComputedStyle,
    context: &MapCtx<'_>,
) -> Option<(f64, f64)> {
    let tokens: Vec<&str> = value.split_whitespace().collect();
    let [width, height] = tokens.as_slice() else {
        return None;
    };
    if width.eq_ignore_ascii_case("auto") || height.eq_ignore_ascii_case("auto") {
        return None;
    }
    let width = axis_length(width, node.0, style, context)?;
    let height = axis_length(height, node.1, style, context)?;
    (width > 0.0 && height > 0.0 && width.is_finite() && height.is_finite())
        .then_some((width, height))
}

/// Top-left corner of the painted bitmap inside the element's box.
fn resolve_position(
    value: &str,
    node: (f64, f64),
    image: (f64, f64),
    style: &ComputedStyle,
    context: &MapCtx<'_>,
) -> Option<(f64, f64)> {
    let tokens: Vec<String> = value
        .split_whitespace()
        .map(|token| token.to_ascii_lowercase())
        .collect();
    let (horizontal, vertical) = match tokens.as_slice() {
        [single] => (single.clone(), "center".to_string()),
        [first, second] => {
            // `top left` is as valid as `left top`; only a keyword pair may be
            // written in either order, so the swap is keyed on the first token.
            if matches!(first.as_str(), "top" | "bottom") {
                (second.clone(), first.clone())
            } else {
                (first.clone(), second.clone())
            }
        }
        // The three- and four-value `right 10px bottom 20px` edge syntax is
        // rare enough to report rather than implement.
        _ => return None,
    };
    Some((
        axis_position(&horizontal, node.0 - image.0, false, style, context)?,
        axis_position(&vertical, node.1 - image.1, true, style, context)?,
    ))
}

/// One axis of `background-position`. Percentages align the same relative point
/// of the image and the box, so they resolve against the free space.
fn axis_position(
    token: &str,
    free: f64,
    vertical: bool,
    style: &ComputedStyle,
    context: &MapCtx<'_>,
) -> Option<f64> {
    match token {
        "left" if !vertical => return Some(0.0),
        "right" if !vertical => return Some(free),
        "top" if vertical => return Some(0.0),
        "bottom" if vertical => return Some(free),
        "center" => return Some(free * 0.5),
        "left" | "right" | "top" | "bottom" => return None,
        _ => {}
    }
    let length = parse_length(token, &length_context(style, context))?;
    Some(length.resolve(free)).filter(|value| value.is_finite())
}

fn axis_length(
    token: &str,
    reference: f64,
    style: &ComputedStyle,
    context: &MapCtx<'_>,
) -> Option<f64> {
    let length = parse_length(token, &length_context(style, context))?;
    Some(length.resolve(reference)).filter(|value| value.is_finite())
}

/// The property's initial value, i.e. the author positioned nothing. Kept
/// distinct from "centred" so an unauthored layer never reports a drop.
pub(super) fn position_is_default(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "" | "0% 0%" | "0 0" | "left top" | "top left" | "0px 0px" | "initial"
    )
}

fn position_is_centered(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "center" | "50%" | "center center" | "50% 50%"
    )
}

/// Whether a `background-position` value on a layer this importer could not
/// place is worth reporting.
pub(super) fn position_is_dropped(style: &ComputedStyle, index: usize) -> bool {
    !position_is_default(&layer_property(
        style,
        "background-position",
        index,
        "0% 0%",
    ))
}

fn layer_property(style: &ComputedStyle, name: &str, index: usize, initial: &str) -> String {
    style
        .get(name)
        .and_then(|value| layer_value(value, index))
        .unwrap_or(initial)
        .trim()
        .to_ascii_lowercase()
}

fn transform_is_finite(transform: &ImageTransform) -> bool {
    [
        transform.m00,
        transform.m01,
        transform.m02,
        transform.m10,
        transform.m11,
        transform.m12,
    ]
    .iter()
    .all(|value| value.is_finite())
        && transform.m00.abs() > f32::EPSILON
        && transform.m11.abs() > f32::EPSILON
}

fn is_identity(transform: &ImageTransform) -> bool {
    const EPSILON: f32 = 1e-6;
    (transform.m00 - 1.0).abs() <= EPSILON
        && (transform.m11 - 1.0).abs() <= EPSILON
        && transform.m02.abs() <= EPSILON
        && transform.m12.abs() <= EPSILON
}

/// Whether any `background-size` layer would need the bitmap's intrinsic size.
/// Gradients have no intrinsic size at all, so this is what the gradient
/// warning in `mapper_visual` keys on.
pub(super) fn gradient_size_is_dropped(style: &ComputedStyle) -> bool {
    style.get("background-size").is_some_and(|value| {
        split_top_level(value, ',').into_iter().any(|layer| {
            !matches!(
                layer.trim().to_ascii_lowercase().as_str(),
                "auto" | "auto auto"
            )
        })
    })
}

#[cfg(test)]
#[path = "mapper_background_tests.rs"]
mod tests;
