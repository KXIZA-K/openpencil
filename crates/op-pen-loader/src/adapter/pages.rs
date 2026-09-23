//! Page assembly: per-page payload build, root-frame clip marking and
//! the taffy layout pass that harvests computed rects.

use super::*;
use op_editor_core::PenNodeExt;

/// Resolve one page into the paint payload used by the editor scene.
///
/// The full-document adapters above remain available to export/validation
/// callers. Interactive hosts use this focused entry point so switching pages
/// does not lay out every inactive page or retain their duplicate payloads.
pub(crate) fn pen_roots_to_page_payload(
    id: &str,
    name: &str,
    roots: &[PenNode],
    page_idx: usize,
    preserve_authored_geometry: bool,
    conversion: Option<&jian_ops_schema::conversion::ConversionSpec>,
) -> PagePayload {
    if preserve_authored_geometry {
        build_page_preserving_geometry(id, name, roots, conversion)
    } else {
        build_page(id, name, roots, page_idx, conversion)
    }
}

pub(super) fn build_page(
    id: &str,
    name: &str,
    roots: &[PenNode],
    page_idx: usize,
    conversion: Option<&jian_ops_schema::conversion::ConversionSpec>,
) -> PagePayload {
    let mut layout_rects: BTreeMap<String, [f32; 4]> = BTreeMap::new();
    for root in roots {
        compute_layout(root, &mut layout_rects, conversion);
    }
    let _ = page_idx;
    let mut children: Vec<NodePayload> = roots
        .iter()
        .map(|n| node_to_payload(n, &layout_rects))
        .collect();
    mark_root_frame_clips(roots, &mut children);
    mark_css_paint_origins(roots, &mut children, conversion);
    PagePayload {
        id: id.to_string(),
        name: name.to_string(),
        children,
    }
}

pub(super) fn mark_css_paint_origins(roots: &[PenNode], children: &mut [NodePayload], conversion: Option<&jian_ops_schema::conversion::ConversionSpec>) {
    fn mark(node: &mut NodePayload, origin: [f32; 2]) {
        // Transformed subtrees keep vector paint; CSS transform rasterization
        // is a separate policy, not screen-space rounding of rotated bounds.
        if node.rotation != 0.0 || node.flip_x || node.flip_y { return; }
        node.css_paint_origin = Some(origin);
        for child in &mut node.children { mark(child, origin); }
    }
    for (root, child) in roots.iter().zip(children) {
        if uses_css_layout(root, conversion) {
            mark(child, [child.x, child.y]);
        }
    }
}

pub(super) fn build_page_preserving_geometry(
    id: &str,
    name: &str,
    roots: &[PenNode],
    conversion: Option<&jian_ops_schema::conversion::ConversionSpec>,
) -> PagePayload {
    let rects = crate::authored_geometry::rects_for_roots(roots);
    let mut children: Vec<NodePayload> = roots.iter().map(|n| node_to_payload(n, &rects)).collect();
    mark_root_frame_clips(roots, &mut children);
    mark_css_paint_origins(roots, &mut children, conversion);
    PagePayload {
        id: id.to_string(),
        name: name.to_string(),
        children,
    }
}

/// Legacy TS flattener parity (`document-flattener.ts`): ROOT frames whose
/// canonical `clipContent` field is absent clip like artboards. An explicit
/// `clipContent: false` is authored geometry and must remain open. Pair the
/// source roots with their payloads so the DTO can keep its compact `bool`.
pub(super) fn mark_root_frame_clips(roots: &[PenNode], children: &mut [NodePayload]) {
    for (root, child) in roots.iter().zip(children) {
        if matches!(
            root,
            PenNode::Frame(frame) if frame.container.clip_content.is_none()
        ) && child.kind == "frame"
        {
            child.clip_content = true;
        }
    }
}

/// Run jian-core's `LayoutEngine` on `root` and harvest absolute
/// rects per schema id into `out`. Each page root gets its own
/// `LayoutEngine` instance — OpenPencil's canvas is infinite, so
/// roots don't share a coordinate frame.
///
/// Merge note (responsive-m1a into main): `LayoutEngine::node_rect`
/// now bakes a root's own authored `(base.x, base.y)` into its
/// returned absolute rect itself (`root_origins` / `is_origin_normalized`
/// in jian-core's `layout/mod.rs`, added for the responsive runtime's
/// multi-root canvas) — this used to be this function's OWN job (see
/// git history for the prior manual `+ root_ox + root_oy` add here).
/// Doing both doubled every harvested rect's origin (measured: a root
/// authored at doc (400, 60) resolved to scene (800, 120) — root_ox/
/// root_oy added on top of jian-core's own addition). `root_authored_
/// origin` stays exported for `op-host-native`'s Canvas Preview tap
/// translation, which still needs the authored origin as a standalone
/// value (not baked into a rect).
pub(super) fn compute_layout(
    root: &PenNode,
    out: &mut BTreeMap<String, [f32; 4]>,
    conversion: Option<&jian_ops_schema::conversion::ConversionSpec>,
) {
    let (root_w, root_h) = root_available_size(root);
    let mut tree = NodeTree::new();
    tree.insert_subtree(root.clone(), None);
    // Real-skia text measurement via jian-skia's `SkiaMeasure`
    // (paragraph shaper). The default `EstimateBackend` is a
    // character-count heuristic accurate to ~10% — for any
    // `fit_content` frame whose size depends on text length the
    // 10% error cascades through every flex parent. SkiaMeasure
    // matches what the canvas painter actually draws so the
    // engine + paint agree on widths.
    let mut engine = LayoutEngine::with_backend(layout_measure_backend());
    let Ok(taffy_roots) = engine.build(&tree) else {
        return;
    };
    let Some(root_id) = taffy_roots.first() else {
        return;
    };
    // Only captured CSS roots opt in. Ordinary vector documents retain their
    // existing layout; source layout coordinates stay distinct from paint.
    if uses_css_layout(root, conversion) {
        engine.preserve_subpixel_layout();
    }
    if engine.compute(*root_id, (root_w, root_h)).is_err() {
        return;
    }
    // Walk every node by SlotMap iteration, looking up its absolute
    // rect via `node_rect`. Keyed back by the schema id we stashed
    // in `tree.by_id` during insertion. `node_rect` already carries
    // the root's authored canvas offset (see the merge note above),
    // so each design sits where the file placed it without any
    // further adjustment here.
    for (id_str, node_key) in tree.by_id.iter() {
        if let Some(rect) = engine.node_rect(*node_key) {
            out.insert(
                id_str.clone(),
                [
                    rect.origin.x,
                    rect.origin.y,
                    rect.size.width,
                    rect.size.height,
                ],
            );
        }
    }
    crate::layout_repair::repair_fit_content_layout(root, out);
}

/// Existing ledger fields survive older editor and native-server serializers.
/// Only an explicit, versioned screen mapping enables CSS layout semantics.
fn uses_css_layout(
    root: &PenNode,
    conversion: Option<&jian_ops_schema::conversion::ConversionSpec>,
) -> bool {
    conversion.is_some_and(|spec| {
        spec.entries.iter().any(|entry| {
            entry.kind == jian_ops_schema::conversion::ConversionKind::Screen
                && entry.node_id.as_deref() == Some(root.base().id.as_str())
                && entry
                    .key
                    .strip_prefix("html-snapshot:v1:")
                    .is_some_and(|key| !key.is_empty())
        })
    })
}

fn layout_measure_backend() -> Rc<dyn MeasureBackend> {
    LAYOUT_MEASURE_BACKEND.with(Rc::clone)
}
