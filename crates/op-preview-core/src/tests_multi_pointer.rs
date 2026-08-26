//! R4 Canonical PreviewInput — multi-pointer identity through the product
//! preview path.
//!
//! Before R4 the preview session synthesized a single `id=1` Mouse stream,
//! so Scale/Rotate could never claim through the product preview panel and
//! two concurrent pointers shared ONE capture anchor. These tests drive
//! real two-finger streams through
//! [`PreviewSession::dispatch_pointer_for_id_at`] and assert the
//! engine-side transform families actually fire, plus the bookkeeping
//! seams: per-id anchor lifetime, teardown [`PreviewSession::cancel_pointer`],
//! and the legacy synthetic-id wrappers staying compatible.

#![cfg(test)]

use super::{test_measure, PreviewSession};
use jian_core::gesture::pointer::{PointerKind, PointerPhase};

fn transform_doc() -> jian_ops_schema::PenDocument {
    let src = r##"{
        "version": "1.1", "formatVersion": "1.1", "id": "x",
        "app": { "name": "x", "version": "1", "id": "x" },
        "state": {
            "ss": { "type": "int", "default": 0 },
            "se": { "type": "int", "default": 0 },
            "rs": { "type": "int", "default": 0 },
            "re": { "type": "int", "default": 0 },
            "taps": { "type": "int", "default": 0 }
        },
        "children": [
            { "type": "frame", "id": "screen", "width": 400, "height": 400,
              "events": {
                "onScaleStart":  [ { "set": { "$app.ss": "$app.ss + 1" } } ],
                "onScaleEnd":    [ { "set": { "$app.se": "$app.se + 1" } } ],
                "onRotateStart": [ { "set": { "$app.rs": "$app.rs + 1" } } ],
                "onRotateEnd":   [ { "set": { "$app.re": "$app.re + 1" } } ],
                "onTap":         [ { "set": { "$app.taps": "$app.taps + 1" } } ]
              },
              "children": [
                  { "type": "rectangle", "id": "stage", "x": 40, "y": 40,
                    "width": 320, "height": 320 }
              ] }
        ]
    }"##;
    jian_ops_schema::load_str(src)
        .expect("parse transform doc")
        .value
}

fn default_theme() -> std::collections::BTreeMap<String, String> {
    std::collections::BTreeMap::new()
}

fn enter() -> PreviewSession {
    PreviewSession::enter(
        &transform_doc(),
        (800.0, 600.0),
        &default_theme(),
        0,
        false,
        false,
        test_measure(),
    )
    .expect("enter preview")
}

fn counter(session: &PreviewSession, key: &str) -> i64 {
    session
        .app_state_value_for_test(key)
        .and_then(|v| v.as_i64())
        .unwrap_or(0)
}

/// The R4 acceptance microcosm: two pointers dispatched under their own
/// ids cross BOTH transform thresholds against each other — something the
/// former synthetic-single-id path can never produce.
#[test]
fn two_pointer_ids_claim_scale_and_rotate_through_one_session() {
    let mut s = enter();
    const TOUCH: PointerKind = PointerKind::Touch;
    // Two fingers land wide apart on the stage (scene == runtime space).
    let _ = s.dispatch_pointer_for_id_at(1, TOUCH, 100.0, 100.0, PointerPhase::Down, 0);
    let _ = s.dispatch_pointer_for_id_at(2, TOUCH, 300.0, 300.0, PointerPhase::Down, 10);
    assert_eq!(
        s.anchored_pointer_ids_for_test(),
        vec![1, 2],
        "each finger owns its capture anchor"
    );

    // Spread apart past 5%: distance 282.8 -> 311.1. Scale claims.
    let spread1 = s.dispatch_pointer_for_id_at(1, TOUCH, 90.0, 90.0, PointerPhase::Move, 20);
    let spread2 = s.dispatch_pointer_for_id_at(2, TOUCH, 310.0, 310.0, PointerPhase::Move, 30);
    assert!(
        spread1 || spread2 || counter(&s, "ss") > 0,
        "scale claim surfaced"
    );
    assert_eq!(counter(&s, "ss"), 1, "ScaleStart fired exactly once");

    // Twist around the midpoint (~45 deg -> ~30 deg): Rotate claims too —
    // the co-win that requires TWO distinct live pointer streams.
    let _ = s.dispatch_pointer_for_id_at(1, TOUCH, 130.0, 230.0, PointerPhase::Move, 40);
    let twist2 = s.dispatch_pointer_for_id_at(2, TOUCH, 270.0, 170.0, PointerPhase::Move, 50);
    assert!(twist2 || counter(&s, "rs") > 0, "rotate claim surfaced");
    assert_eq!(counter(&s, "rs"), 1, "RotateStart fired exactly once");

    // Symmetric teardown: settle exactly one End per family overall.
    let _ = s.cancel_pointer(2, 60);
    let _ = s.dispatch_pointer_for_id_at(1, TOUCH, 130.0, 230.0, PointerPhase::Up, 70);
    assert_eq!(counter(&s, "se"), 1, "one ScaleEnd");
    assert_eq!(counter(&s, "re"), 1, "one RotateEnd");
    assert!(
        s.anchored_pointer_ids_for_test().is_empty(),
        "every anchor released"
    );
}

/// Anchor hygiene across interleaved lifecycles: a Cancel frees only its
/// own pointer's anchor, a lone Move without a Down stores nothing, and
/// the next pairing anchors fresh ids.
#[test]
fn cancel_and_stray_moves_manage_anchors_per_pointer() {
    let mut s = enter();
    const TOUCH: PointerKind = PointerKind::Touch;
    let _ = s.dispatch_pointer_for_id_at(1, TOUCH, 100.0, 100.0, PointerPhase::Down, 0);
    let _ = s.dispatch_pointer_for_id_at(2, TOUCH, 260.0, 260.0, PointerPhase::Down, 10);
    assert_eq!(s.anchored_pointer_ids_for_test(), vec![1, 2]);

    // Cancelling pointer 2 must not disturb pointer 1's anchor. The
    // return is false here because this fixture declares no press
    // handlers (nothing to emit) — consumption still settles pointer 2's
    // arena timers and releases its anchor.
    let _ = s.cancel_pointer(2, 20);
    assert_eq!(s.anchored_pointer_ids_for_test(), vec![1]);
    // A stray Move for an unknown pointer resolves but stores nothing.
    let _ = s.dispatch_pointer_for_id_at(9, TOUCH, 50.0, 50.0, PointerPhase::Move, 30);
    assert_eq!(s.anchored_pointer_ids_for_test(), vec![1]);

    // Pointer 1 lifts cleanly; the session is ready for a fresh pairing.
    let _ = s.dispatch_pointer_for_id_at(1, TOUCH, 110.0, 110.0, PointerPhase::Up, 40);
    assert!(s.anchored_pointer_ids_for_test().is_empty());
    let _ = s.dispatch_pointer_for_id_at(3, TOUCH, 80.0, 80.0, PointerPhase::Down, 50);
    let _ = s.dispatch_pointer_for_id_at(4, TOUCH, 240.0, 240.0, PointerPhase::Down, 60);
    assert_eq!(s.anchored_pointer_ids_for_test(), vec![3, 4]);
}

/// Legacy compatibility pin: the synthetic id-1 Mouse wrappers keep
/// working unchanged (the `dispatch_tap` route), intermixed with
/// explicit-id traffic in the SAME session.
#[test]
fn legacy_wrappers_still_dispatch_the_synthetic_mouse_stream() {
    let mut s = enter();
    const TOUCH: PointerKind = PointerKind::Touch;
    let down = s.dispatch_pointer_phase_at(160.0, 160.0, PointerPhase::Down, 0);
    let up = s.dispatch_pointer_phase_at(160.0, 160.0, PointerPhase::Up, 20);
    assert!(down || up, "legacy tap consumed");
    assert_eq!(counter(&s, "taps"), 1);

    // Explicit ids coexist with the legacy stream afterwards — their own
    // completed tap also bubbles to the frame handler.
    let _ = s.dispatch_pointer_for_id_at(2, TOUCH, 260.0, 260.0, PointerPhase::Down, 40);
    let _ = s.dispatch_pointer_for_id_at(2, TOUCH, 260.0, 260.0, PointerPhase::Up, 50);
    assert_eq!(counter(&s, "taps"), 2, "explicit-id tap delivered");
    // And another legacy tap keeps counting for the same stream.
    let _ = s.dispatch_pointer_phase_at(160.0, 160.0, PointerPhase::Down, 60);
    let _ = s.dispatch_pointer_phase_at(160.0, 160.0, PointerPhase::Up, 70);
    assert_eq!(counter(&s, "taps"), 3, "second legacy tap delivered");
}
