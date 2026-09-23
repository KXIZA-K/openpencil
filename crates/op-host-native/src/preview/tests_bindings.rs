//! Task C1 tests: compiling non-`bind:value` bindings at preview `enter`.
//!
//! Split from `tests.rs` (which sits at the 800-line cap) rather than
//! appended there. Carries the shared `counter_doc()` fixture both
//! Task C1 (this file — compile-time site collection) and Task C2
//! (overlay re-evaluation, added on top of the same fixture) rely on.

#![cfg(test)]

use super::PreviewSession;
use op_editor_ui::layout_scene::{LayoutScene, SceneNode};

/// The default (empty) active-theme map — mirrors `tests.rs`'s helper of
/// the same name (duplicated here rather than making it `pub(super)` in
/// `tests.rs`, since these are sibling `#[cfg(test)]` modules, not nested
/// ones, and the helper is two lines).
fn default_theme() -> std::collections::BTreeMap<String, String> {
    std::collections::BTreeMap::new()
}

/// Find a node by id on the scene's active page — mirrors `tests.rs`'s
/// helper of the same name (duplicated for the same sibling-module reason
/// as `default_theme` above).
fn find<'a>(scene: &'a LayoutScene, id: &str) -> Option<&'a SceneNode> {
    scene.active_page().and_then(|p| p.find(id))
}

/// Counter fixture: doc-root `$app.count`, a text node whose `content`
/// binds the count, and a switch whose onTap increments it — the
/// canonical "events must be visible" Spec-2 acceptance shape.
fn counter_doc() -> jian_ops_schema::PenDocument {
    let src = r##"{
        "version": "1.1", "formatVersion": "1.1", "id": "x",
        "app": { "name": "x", "version": "1", "id": "x" },
        "state": { "count": { "type": "int", "default": 2 } },
        "children": [
            { "type": "frame", "id": "root", "width": 400, "height": 200, "children": [
                { "type": "text", "id": "label", "content": "Count: -",
                  "width": 200, "height": 24,
                  "bindings": { "content": "\"Count: \" + $app.count" } },
                { "type": "switch", "id": "sw", "width": 44, "height": 24,
                  "events": { "onTap": [ { "set": { "$app.count": "$app.count + 1" } } ] } }
            ]}
        ]
    }"##;
    jian_ops_schema::load_str(src)
        .expect("parse counter doc")
        .value
}

#[test]
fn enter_compiles_binding_sites() {
    let doc = counter_doc();
    let session = PreviewSession::enter(&doc, (800.0, 600.0), &default_theme(), 0, false, false)
        .expect("enter");
    assert_eq!(
        session.binding_sites_len_for_test(),
        1,
        "one content binding compiled"
    );
}

#[test]
fn invalid_binding_becomes_warning_not_error() {
    let src = r##"{
        "version": "1.1", "formatVersion": "1.1", "id": "x",
        "app": { "name": "x", "version": "1", "id": "x" },
        "children": [
            { "type": "text", "id": "bad", "content": "x", "width": 100, "height": 20,
              "bindings": { "content": "1 +" } }
        ]
    }"##;
    let doc = jian_ops_schema::load_str(src).expect("parse").value;
    let session = PreviewSession::enter(&doc, (800.0, 600.0), &default_theme(), 0, false, false)
        .expect("enter still ok");
    assert_eq!(session.binding_sites_len_for_test(), 0);
    assert!(
        session
            .warnings()
            .iter()
            .any(|w| w.contains("InvalidBinding")),
        "compile failure must surface as a warning, got {:?}",
        session.warnings()
    );
}

// --- Task C2: overlay re-evaluates bindings each paint -------------

#[test]
fn binding_content_resolves_on_enter() {
    // Even before any interaction, a bound text node must show the
    // expression's value over the doc-root default state.
    let doc = counter_doc();
    let session = PreviewSession::enter(&doc, (800.0, 600.0), &default_theme(), 0, false, false)
        .expect("enter");
    let scene = session.preview_scene_for_test();
    let label = find(&scene, "label").expect("label in scene");
    assert_eq!(label.text.as_deref(), Some("Count: 2"));
}

#[test]
fn tap_event_updates_bound_text() {
    // The Spec-2 acceptance loop: tap switch → onTap set $app.count →
    // bound label repaints with the new value.
    let doc = counter_doc();
    let mut session =
        PreviewSession::enter(&doc, (800.0, 600.0), &default_theme(), 0, false, false)
            .expect("enter");
    session.set_now_ms(0);
    let (x, y, w, h) = session.node_rect("sw").expect("switch rect");
    session.dispatch_tap(x + w / 2.0, y + h / 2.0);
    let scene = session.preview_scene_for_test();
    assert_eq!(
        find(&scene, "label").expect("label").text.as_deref(),
        Some("Count: 3"),
        "fired event must be visible in the overlaid scene"
    );
}

#[test]
fn bindings_do_not_mutate_document() {
    let doc = counter_doc();
    let before = serde_json::to_string(&doc).expect("before");
    {
        let mut session =
            PreviewSession::enter(&doc, (800.0, 600.0), &default_theme(), 0, false, false)
                .expect("enter");
        session.set_now_ms(0);
        let (x, y, w, h) = session.node_rect("sw").expect("switch rect");
        session.dispatch_tap(x + w / 2.0, y + h / 2.0);
    }
    let after = serde_json::to_string(&doc).expect("after");
    assert_eq!(
        before, after,
        "binding overlay must never touch the saved doc"
    );
}

#[test]
fn frame_button_tap_cycles_bound_child_text_without_widget_promotion() {
    let doc = jian_ops_schema::load_str(r##"{
      "version":"1.1","children":[{"type":"frame","id":"root","x":80,"y":40,"width":390,"height":844,"children":[
        {"type":"frame","id":"sos","role":"button","width":342,"height":64,"layout":"horizontal",
         "events":{"onTap":[{"set":{"$app.sosStep":"($app.sosStep ?? 0) == 2 ? 0 : ($app.sosStep ?? 0) + 1"}}]},
         "children":[{"type":"text","id":"label","width":300,"height":30,"content":"Demo SOS",
           "bindings":{"content":"($app.sosStep ?? 0) == 1 ? 'Confirm' : (($app.sosStep ?? 0) == 2 ? 'Sent' : 'Demo SOS')"}}]}
      ]}]}"##).unwrap().value;
    let mut session =
        PreviewSession::enter(&doc, (800.0, 900.0), &default_theme(), 0, false, false).unwrap();
    // Exercise the authored scene and the origin-normalized browser scene.
    // Their runtime spatial index stays in authored coordinates in both cases.
    for normalized in [false, true] {
        if normalized {
            session
                .scene
                .translate_nodes(&["root".to_owned()], -80.0, -40.0);
        }
        for target in ["label", "sos"] {
            let bounds = find(&session.scene, target).unwrap().bounds;
            for expected in ["Confirm", "Sent", "Demo SOS"] {
                let point = if target == "sos" {
                    (bounds.origin.x + 330.0, bounds.origin.y + 50.0)
                } else {
                    (
                        bounds.origin.x + bounds.size.x / 2.0,
                        bounds.origin.y + bounds.size.y / 2.0,
                    )
                };
                session.dispatch_tap(point.0, point.1);
                let scene = session.preview_scene_for_test();
                assert_eq!(
                    find(&scene, "label").unwrap().text.as_deref(),
                    Some(expected),
                    "target={target} normalized={normalized}"
                );
            }
        }
    }
}

#[test]
fn conditional_panel_binding_starts_hidden_and_opens_on_tap() {
    // Mirrors Studio v17's map panel: the document retains its design-time
    // contents, but Run must respect the app-state visibility condition.
    let doc = jian_ops_schema::load_str(
        r##"{
      "version":"1.1","children":[{"type":"frame","id":"root","width":390,"height":844,
        "layout":"vertical","children":[
        {"type":"frame","id":"open","width":300,"height":44,
          "events":{"onTap":[{"set":{"$app.mapOpen":"true"}}]}},
        {"type":"frame","id":"map","width":300,"height":260,
          "bindings":{"visible":"($app.mapOpen ?? false)"},
          "children":[{"type":"text","id":"pin","width":100,"height":24,"content":"Clinic"}]}
      ]}]}"##,
    )
    .unwrap()
    .value;
    let before = serde_json::to_string(&doc).unwrap();
    let mut session =
        PreviewSession::enter(&doc, (390.0, 844.0), &default_theme(), 0, false, false).unwrap();
    assert!(
        find(&session.preview_scene_for_test(), "map")
            .unwrap()
            .hidden,
        "A false visibility binding must hide the entire map subtree in Run"
    );
    let (x, y, w, h) = session.node_rect("open").unwrap();
    session.dispatch_tap(x + w / 2.0, y + h / 2.0);
    assert!(
        !find(&session.preview_scene_for_test(), "map")
            .unwrap()
            .hidden
    );
    assert_eq!(serde_json::to_string(&doc).unwrap(), before);
}

#[test]
fn hidden_bound_subtree_cannot_receive_taps_or_focus() {
    let doc = jian_ops_schema::load_str(r##"{
      "version":"1.1","children":[{"type":"frame","id":"root","width":390,"height":600,"layout":"vertical","children":[
        {"type":"frame","id":"open","width":300,"height":44,"events":{"onTap":[{"set":{"$app.open":"true"}}]}},
        {"type":"frame","id":"panel","width":300,"height":160,"layout":"vertical","bindings":{"visible":"$app.open ?? false"},"children":[
          {"type":"frame","id":"close","width":300,"height":44,"events":{"onTap":[{"set":{"$app.hits":"($app.hits ?? 0) + 1","$app.open":"false"}}]}},
          {"type":"text_input","id":"field","width":200,"height":40,"value":""}
        ]},
        {"type":"text","id":"hits","width":200,"height":30,"content":"0","bindings":{"content":"$app.hits ?? 0"}}
      ]}]}"##).unwrap().value;
    let mut session =
        PreviewSession::enter(&doc, (390.0, 600.0), &default_theme(), 0, false, false).unwrap();
    let tap = |s: &mut PreviewSession, id: &str| {
        let bounds = find(&s.scene, id).unwrap().bounds;
        s.dispatch_tap(
            bounds.origin.x + bounds.size.x / 2.0,
            bounds.origin.y + bounds.size.y / 2.0,
        );
    };
    tap(&mut session, "close");
    assert_eq!(
        find(&session.preview_scene_for_test(), "hits")
            .unwrap()
            .text
            .as_deref(),
        Some("0")
    );
    session.focus_next();
    assert_ne!(session.focused_schema_id().as_deref(), Some("field"));
    tap(&mut session, "open");
    tap(&mut session, "field");
    assert_eq!(session.focused_schema_id().as_deref(), Some("field"));
    tap(&mut session, "close");
    session.focus_next();
    assert_ne!(session.focused_schema_id().as_deref(), Some("field"));
    tap(&mut session, "close");
    assert_eq!(
        find(&session.preview_scene_for_test(), "hits")
            .unwrap()
            .text
            .as_deref(),
        Some("1")
    );
}

#[test]
fn checkbox_state_binding_updates_computed_progress() {
    let doc = jian_ops_schema::load_str(r##"{
      "version":"1.1","children":[{"type":"frame","id":"root","width":390,"height":300,"layout":"vertical","children":[
        {"type":"checkbox","id":"check","width":200,"height":44,"checked":false,
          "bindings":{"checked":"$app.ready ?? false"},"events":{"onTap":[{"set":{"$app.ready":"!($app.ready ?? false)"}}]}},
        {"type":"progress","id":"progress","width":200,"height":10,"value":0,"max":4,
          "bindings":{"value":"($app.ready ?? false) ? 3 : 0"}}
      ]}]}"##).unwrap().value;
    let mut session =
        PreviewSession::enter(&doc, (390.0, 300.0), &default_theme(), 0, false, false).unwrap();
    let (x, y, w, h) = session.node_rect("check").unwrap();
    for expected in [true, false] {
        session.dispatch_tap(x + w / 2.0, y + h / 2.0);
        let scene = session.preview_scene_for_test();
        assert_eq!(
            find(&scene, "check")
                .unwrap()
                .widget
                .as_ref()
                .unwrap()
                .checked,
            Some(expected)
        );
        assert_eq!(
            find(&scene, "progress")
                .unwrap()
                .widget
                .as_ref()
                .unwrap()
                .value_num,
            Some(if expected { 3.0 } else { 0.0 })
        );
    }
}

#[test]
fn front_overlay_close_wins_over_overlapping_background() {
    let doc = jian_ops_schema::load_str(r##"{
      "version":"1.1","children":[{"type":"frame","id":"root","x":1960,"y":40,"width":390,"height":844,"layout":"vertical","children":[
        {"type":"frame","id":"map","role":"overlay","x":16,"y":230,"width":358,"height":260,"layout":"none","bindings":{"visible":"$app.open ?? true"},"children":[
          {"type":"frame","id":"close","x":306,"y":10,"width":40,"height":40,"events":{"onTap":[{"set":{"$app.open":"false"}}]}},
          {"type":"rectangle","id":"road","x":0,"y":30,"width":358,"height":12}
        ]},
        {"type":"frame","id":"background","width":390,"height":600}
      ]}]}"##).unwrap().value;
    for normalized in [false, true] {
        let mut session =
            PreviewSession::enter(&doc, (390.0, 844.0), &default_theme(), 0, false, false).unwrap();
        if normalized {
            session
                .scene
                .translate_nodes(&["root".to_owned()], -1960.0, -40.0);
        }
        let bounds = find(&session.scene, "close").unwrap().bounds;
        session.dispatch_tap(bounds.origin.x + 20.0, bounds.origin.y + 20.0);
        assert!(
            find(&session.preview_scene_for_test(), "map")
                .unwrap()
                .hidden,
            "front overlay close must receive the tap; normalized={normalized}"
        );
    }
}
