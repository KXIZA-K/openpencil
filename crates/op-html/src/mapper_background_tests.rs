use super::*;
use crate::css::cascade::StyleRule;
use std::collections::BTreeMap;

fn computed(properties: &[(&str, &str)]) -> ComputedStyle {
    ComputedStyle {
        props: properties
            .iter()
            .map(|(name, value)| ((*name).into(), (*value).into()))
            .collect::<BTreeMap<_, _>>(),
        font_size: 16.0,
    }
}

fn geometry(properties: &[(&str, &str)], node: (f64, f64)) -> (LayerGeometry, Vec<String>) {
    geometry_for(properties, node, (true, true))
}

fn geometry_for(
    properties: &[(&str, &str)],
    node: (f64, f64),
    definite: (bool, bool),
) -> (LayerGeometry, Vec<String>) {
    let options = crate::HtmlImportOptions::default();
    let rules = Vec::<StyleRule>::new();
    let mut context = MapCtx {
        opts: &options,
        rules: &rules,
        warnings: Vec::new(),
        warned: Default::default(),
        next_id: 0,
        node_count: 0,
        containing_width: options.viewport_width,
        containing_height: options.viewport_height(),
        containing_width_is_definite: true,
        positioned_width: options.viewport_width,
        positioned_height: options.viewport_height(),
        auto_margin_handled_by_parent: false,
        pending_base_outcome: Default::default(),
    };
    let output = image_layer(&computed(properties), 0, node, definite, &mut context);
    (output, crate::render_warnings(&context.warnings))
}

#[test]
fn cover_and_contain_map_to_the_matching_placement_modes() {
    let (cover, warnings) = geometry(
        &[
            ("background-size", "cover"),
            ("background-repeat", "no-repeat"),
        ],
        (400.0, 300.0),
    );
    assert_eq!(cover.mode, Some(ImageFillMode::Fill));
    assert!(cover.transform.is_none());
    assert!(!cover.position_dropped);
    assert!(warnings.is_empty(), "{warnings:?}");

    let (contain, _) = geometry(
        &[
            ("background-size", "contain"),
            ("background-repeat", "no-repeat"),
            ("background-position", "center"),
        ],
        (400.0, 300.0),
    );
    assert_eq!(contain.mode, Some(ImageFillMode::Fit));
    assert!(!contain.position_dropped, "cover/contain already centre");
}

#[test]
fn cover_with_a_corner_position_reports_the_drop() {
    let (layer, _) = geometry(
        &[
            ("background-size", "cover"),
            ("background-repeat", "no-repeat"),
            ("background-position", "right bottom"),
        ],
        (400.0, 300.0),
    );
    assert_eq!(layer.mode, Some(ImageFillMode::Fill));
    assert!(layer.position_dropped);
}

#[test]
fn an_explicit_size_and_position_become_an_exact_crop_transform() {
    // A 100×50 bitmap painted at (25, 10) inside a 200×100 box.
    let (layer, warnings) = geometry(
        &[
            ("background-size", "100px 50px"),
            ("background-repeat", "no-repeat"),
            ("background-position", "25px 10px"),
        ],
        (200.0, 100.0),
    );
    assert_eq!(layer.mode, Some(ImageFillMode::Crop));
    let transform = layer.transform.expect("crop transform");
    assert_eq!(transform.m00, 2.0);
    assert_eq!(transform.m11, 2.0);
    assert_eq!(transform.m02, -0.25);
    assert_eq!(transform.m12, -0.2);
    assert!(!layer.position_dropped);
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[test]
fn percentage_sizes_and_positions_resolve_against_the_used_box() {
    // 50%×50% of a 200×100 box is 100×50; `center` then leaves 50px / 25px.
    let (layer, _) = geometry(
        &[
            ("background-size", "50% 50%"),
            ("background-repeat", "no-repeat"),
            ("background-position", "center"),
        ],
        (200.0, 100.0),
    );
    let transform = layer.transform.expect("crop transform");
    assert_eq!(transform.m00, 2.0);
    assert_eq!(transform.m02, -0.5);
    assert_eq!(transform.m12, -0.5);
}

#[test]
fn a_full_bleed_explicit_size_stays_a_plain_stretch() {
    let (layer, _) = geometry(
        &[
            ("background-size", "100% 100%"),
            ("background-repeat", "no-repeat"),
        ],
        (200.0, 100.0),
    );
    assert_eq!(layer.mode, Some(ImageFillMode::Stretch));
    assert!(layer.transform.is_none());
    assert!(!layer.position_dropped);
}

#[test]
fn a_single_axis_size_needs_the_intrinsic_bitmap_and_is_reported() {
    let (layer, warnings) = geometry(
        &[
            ("background-size", "120px"),
            ("background-repeat", "no-repeat"),
        ],
        (200.0, 100.0),
    );
    assert_eq!(layer.mode, Some(ImageFillMode::Fill));
    assert!(
        warnings
            .iter()
            .any(|warning| warning.contains("intrinsic size")),
        "{warnings:?}"
    );
}

#[test]
fn repeating_layers_stay_tiles() {
    let (layer, warnings) = geometry(
        &[
            ("background-repeat", "repeat"),
            ("background-size", "40px 40px"),
        ],
        (200.0, 100.0),
    );
    assert_eq!(layer.mode, Some(ImageFillMode::Tile));
    assert!(
        warnings.iter().any(|warning| warning.contains("tile size")),
        "{warnings:?}"
    );
}

#[test]
fn css_default_repeat_retains_top_left_sampling_contract() {
    let (layer, warnings) = geometry(&[("background-repeat", "repeat")], (390.0, 340.0));
    assert_eq!(layer.mode, Some(ImageFillMode::CssRepeat));
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(!layer.position_dropped);
}

/// An `auto` axis means `node` is the containing block, not the element's box.
/// A crop transform derived from it decals the bitmap outside the node, so the
/// layer must degrade to a whole-box fill instead of vanishing.
#[test]
fn an_indefinite_axis_refuses_the_crop_transform_and_is_reported() {
    for definite in [(true, false), (false, true), (false, false)] {
        let (layer, warnings) = geometry_for(
            &[
                ("background-size", "16px 16px"),
                ("background-repeat", "no-repeat"),
                ("background-position", "center"),
            ],
            (200.0, 900.0),
            definite,
        );
        assert_eq!(layer.mode, Some(ImageFillMode::Fill), "{definite:?}");
        assert!(layer.transform.is_none(), "{definite:?}");
        assert!(
            warnings
                .iter()
                .any(|warning| warning.contains("automatic width or height")),
            "{definite:?}: {warnings:?}"
        );
    }
}

/// The `no-repeat center / 16px 16px` icon idiom on an auto-height padded div:
/// Phase A painted it as a `Fill`, and that is still better than a crop window
/// computed from the viewport height.
#[test]
fn an_auto_height_icon_background_stays_a_fill() {
    let result = crate::import_html(
        "<div style=\"width:240px;padding:12px 12px 12px 36px;\
         background:url(check.svg) no-repeat center / 16px 16px\">Done</div>",
        &crate::HtmlImportOptions::default(),
    );
    let mut fills = Vec::new();
    collect_image_fills(&result.nodes, &mut fills);
    assert_eq!(fills.len(), 1, "one background layer");
    assert_eq!(
        fills[0].0,
        Some(jian_ops_schema::style::ImageFillMode::Fill)
    );
    assert!(
        fills[0].1.is_none(),
        "an auto height cannot produce a crop window: {:?}",
        fills[0].1
    );
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains("automatic width or height")),
        "{:?}",
        result.warnings
    );
}

/// A definite box keeps the exact crop, so the guard is not a blanket refusal.
#[test]
fn a_definite_box_still_gets_the_exact_crop_window() {
    let result = crate::import_html(
        "<div style=\"width:200px;height:100px;\
         background:url(badge.png) no-repeat 25px 10px / 100px 50px\">x</div>",
        &crate::HtmlImportOptions::default(),
    );
    let mut fills = Vec::new();
    collect_image_fills(&result.nodes, &mut fills);
    assert_eq!(fills.len(), 1);
    assert_eq!(
        fills[0].0,
        Some(jian_ops_schema::style::ImageFillMode::Crop)
    );
    let transform = fills[0].1.as_ref().expect("crop transform");
    assert_eq!((transform.m00, transform.m11), (2.0, 2.0));
}

#[allow(clippy::type_complexity)]
fn collect_image_fills(
    nodes: &[jian_ops_schema::node::PenNode],
    out: &mut Vec<(Option<ImageFillMode>, Option<ImageTransform>)>,
) {
    use jian_ops_schema::node::PenNode;
    use jian_ops_schema::style::PenFill;
    for node in nodes {
        if let PenNode::Frame(frame) = node {
            for fill in frame.container.fill.as_deref().unwrap_or_default() {
                if let PenFill::Image(image) = fill {
                    out.push((image.mode.clone(), image.transform.clone()));
                }
            }
            collect_image_fills(frame.children.as_deref().unwrap_or_default(), out);
        }
    }
}

#[test]
fn only_authored_positions_count_as_dropped() {
    assert!(position_is_default(""));
    assert!(position_is_default("0% 0%"));
    assert!(position_is_default("LEFT TOP"));
    assert!(!position_is_default("center"));
    assert!(position_is_centered("50% 50%"));
    assert!(!position_is_centered("right"));
}
