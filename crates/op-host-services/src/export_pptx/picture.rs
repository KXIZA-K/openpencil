//! Image nodes as `<p:pic>` — an embedded bitmap placed in the box the
//! layout gave it.
//!
//! CSS gets `background-size: cover` as a keyword. DrawingML has no
//! keyword: filling a box while keeping the aspect ratio means stating
//! the crop rectangle yourself, in percentages of the SOURCE image. That
//! is why the emitter needs the bitmap's real pixel size and why a
//! source it cannot measure goes down the raster path instead — a
//! guessed crop is a visibly wrong photograph, and the fallback renders
//! the same pixels the canvas does.

use op_editor_ui::layout_scene::{SceneImageFit, SceneNode};
use op_editor_ui::Rect;
use op_util::xml_escape::escape_xml;
use std::fmt::Write as _;

use super::units::{pct_1000, signed_pct_1000};
use super::xml::{effect_list, line_element, prst_geom, sp_pr, xfrm, Geom};

/// Emit an image node. `rel_id` is the slide-local relationship id of
/// the already-interned media part, `source_px` its decoded dimensions.
pub fn emit(
    out: &mut String,
    node: &SceneNode,
    rect: Rect,
    alpha: f32,
    rel_id: &str,
    source_px: (f32, f32),
    id: u32,
) {
    let line = node.stroke.and_then(|s| line_element(s, alpha));
    let _ = write!(
        out,
        "<p:pic><p:nvPicPr><p:cNvPr id=\"{id}\" name=\"{}\"/><p:cNvPicPr>\
<a:picLocks noChangeAspect=\"0\"/></p:cNvPicPr><p:nvPr/></p:nvPicPr>{}{}</p:pic>",
        escape_xml(&node.id),
        blip_fill(node, rect, alpha, rel_id, source_px),
        sp_pr(
            &xfrm(rect, node),
            &prst_geom(node, Geom::Rect, rect),
            "",
            line.as_deref(),
            &effect_list(node, alpha)
        )
    );
}

/// Emit a raster image that fills its rect exactly — the shape used by
/// the fallback path, where the PNG was rendered AT the rect.
pub fn emit_raster(out: &mut String, node_id: &str, rect: Rect, rel_id: &str, id: u32) {
    use super::xml::xfrm_plain;
    let _ = write!(
        out,
        "<p:pic><p:nvPicPr><p:cNvPr id=\"{id}\" name=\"{}\"/><p:cNvPicPr/><p:nvPr/></p:nvPicPr>\
<p:blipFill><a:blip r:embed=\"{rel_id}\"/><a:stretch><a:fillRect/></a:stretch></p:blipFill>\
<p:spPr>{}<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></p:spPr></p:pic>",
        escape_xml(node_id),
        xfrm_plain(rect)
    );
}

/// `<p:blipFill>`: the bitmap, an optional source crop, and how the
/// (possibly cropped) source maps onto the shape.
fn blip_fill(
    node: &SceneNode,
    rect: Rect,
    alpha: f32,
    rel_id: &str,
    source_px: (f32, f32),
) -> String {
    // DrawingML has no picture opacity property either; `alphaModFix` on
    // the blip is the one place an inherited composite opacity can land.
    let fade = if alpha < 0.999 {
        format!("<a:alphaModFix amt=\"{}\"/>", pct_1000(alpha))
    } else {
        String::new()
    };
    let blip = if fade.is_empty() {
        format!("<a:blip r:embed=\"{rel_id}\"/>")
    } else {
        format!("<a:blip r:embed=\"{rel_id}\">{fade}</a:blip>")
    };
    let (src_rect, fill_rect) = placement(node.image_fit, rect, source_px);
    format!("<p:blipFill rotWithShape=\"1\">{blip}{src_rect}<a:stretch>{fill_rect}</a:stretch></p:blipFill>")
}

/// The `<a:srcRect>` / `<a:fillRect>` pair for a placement mode.
///
/// - **Fill / Crop** cover the box: the SOURCE is cropped to the box's
///   aspect ratio and centred, then stretched edge to edge.
/// - **Fit** contains: the whole source is kept and the FILL AREA is
///   inset so the image sits centred inside the box with the leftover
///   space empty.
/// - **Stretch** ignores the aspect ratio, which is what it means.
fn placement(fit: SceneImageFit, rect: Rect, source_px: (f32, f32)) -> (String, String) {
    let (sw, sh) = source_px;
    let box_aspect = rect.size.x / rect.size.y.max(0.001);
    let src_aspect = sw / sh.max(0.001);
    if !box_aspect.is_finite() || !src_aspect.is_finite() || box_aspect <= 0.0 || src_aspect <= 0.0
    {
        return (String::new(), "<a:fillRect/>".to_string());
    }
    match fit {
        SceneImageFit::Stretch | SceneImageFit::Tile | SceneImageFit::CssRepeat => {
            (String::new(), "<a:fillRect/>".to_string())
        }
        SceneImageFit::Fill | SceneImageFit::Crop => {
            let (mut side, mut top) = (0.0f32, 0.0f32);
            if src_aspect > box_aspect {
                side = (1.0 - box_aspect / src_aspect) / 2.0;
            } else {
                top = (1.0 - src_aspect / box_aspect) / 2.0;
            }
            if side < 0.0005 && top < 0.0005 {
                return (String::new(), "<a:fillRect/>".to_string());
            }
            (
                format!(
                    "<a:srcRect l=\"{}\" t=\"{}\" r=\"{}\" b=\"{}\"/>",
                    pct_1000(side),
                    pct_1000(top),
                    pct_1000(side),
                    pct_1000(top)
                ),
                "<a:fillRect/>".to_string(),
            )
        }
        SceneImageFit::Fit => {
            let (mut side, mut top) = (0.0f32, 0.0f32);
            if src_aspect > box_aspect {
                top = (1.0 - box_aspect / src_aspect) / 2.0;
            } else {
                side = (1.0 - src_aspect / box_aspect) / 2.0;
            }
            if side < 0.0005 && top < 0.0005 {
                return (String::new(), "<a:fillRect/>".to_string());
            }
            (
                String::new(),
                format!(
                    "<a:fillRect l=\"{}\" t=\"{}\" r=\"{}\" b=\"{}\"/>",
                    signed_pct_1000(side),
                    signed_pct_1000(top),
                    signed_pct_1000(side),
                    signed_pct_1000(top)
                ),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wide_box() -> Rect {
        Rect::xywh(0.0, 0.0, 400.0, 100.0)
    }

    #[test]
    fn covering_a_wide_box_with_a_square_source_crops_top_and_bottom() {
        let (src, fill) = placement(SceneImageFit::Fill, wide_box(), (500.0, 500.0));
        // Box is 4:1, source 1:1 — keep a quarter of the height,
        // 37.5% off each end.
        assert_eq!(src, "<a:srcRect l=\"0\" t=\"37500\" r=\"0\" b=\"37500\"/>");
        assert_eq!(fill, "<a:fillRect/>");
    }

    #[test]
    fn fitting_a_square_source_into_a_wide_box_insets_the_fill_area() {
        let (src, fill) = placement(SceneImageFit::Fit, wide_box(), (500.0, 500.0));
        assert_eq!(src, "");
        // The square keeps the box height and takes a quarter of its
        // width, so 37.5% is left empty each side.
        assert_eq!(
            fill,
            "<a:fillRect l=\"37500\" t=\"0\" r=\"37500\" b=\"0\"/>"
        );
    }

    #[test]
    fn a_source_that_already_matches_needs_no_crop_at_all() {
        let (src, fill) = placement(SceneImageFit::Fill, wide_box(), (800.0, 200.0));
        assert_eq!(src, "");
        assert_eq!(fill, "<a:fillRect/>");
    }

    #[test]
    fn stretch_ignores_the_aspect_ratio_by_definition() {
        let (src, fill) = placement(SceneImageFit::Stretch, wide_box(), (500.0, 500.0));
        assert_eq!(src, "");
        assert_eq!(fill, "<a:fillRect/>");
    }
}
