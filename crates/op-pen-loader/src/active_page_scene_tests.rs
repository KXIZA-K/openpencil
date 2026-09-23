use crate::{editor_state_to_active_page_layout_scene, editor_state_to_layout_scene};
use jian_ops_schema::variable::{VariableKind, VariableScalar};
use op_editor_core::{EditorState, NodeId, PenNodeExt};

fn state_from(src: &str) -> EditorState {
    EditorState::from_document(
        jian_ops_schema::load_str(src)
            .expect("fixture parses")
            .value,
    )
}

#[test]
fn source_text_rasterization_survives_save_and_both_scene_paths() {
    let state = state_from(r#"{"version":"1.0.0","children":[
      {"id":"gray","type":"text","content":"Same label","textRasterization":"grayscale"},
      {"id":"auto","type":"text","content":"Same label"}
    ]}"#);
    let saved = serde_json::to_string(&state.doc).unwrap();
    let restored = state_from(&saved);
    for scene in [editor_state_to_layout_scene(&restored), editor_state_to_active_page_layout_scene(&restored)] {
        assert!(scene.pages[0].children[0].text_grayscale);
        assert!(!scene.pages[0].children[1].text_grayscale);
    }
    assert_eq!(serde_json::to_string(&restored.doc).unwrap(), saved);
}

#[test]
fn html_ledger_layout_is_consistent_across_editor_and_preview_without_affecting_vectors() {
    let state = state_from(
        r#"{
      "version":"1.0.0",
      "conversion":{"entries":[{"kind":"screen","key":"html-snapshot:v1:fixture","nodeId":"html"}]},
      "pages":[{"id":"page","name":"Mixed","children":[
        {"id":"html","type":"frame","width":100.5,"height":60.25},
        {"id":"vector","type":"frame","x":200,"width":100.5,"height":60.25}
      ]}]
    }"#,
    );
    let original = serde_json::to_string(&state.doc).unwrap();
    for scene in [
        editor_state_to_layout_scene(&state),
        editor_state_to_active_page_layout_scene(&state),
    ] {
        assert_eq!(scene.pages[0].children[0].bounds.size.x, 100.5);
        assert_eq!(scene.pages[0].children[0].bounds.size.y, 60.25);
        assert_eq!(scene.pages[0].children[1].bounds.size.x, 101.0);
        assert_eq!(scene.pages[0].children[1].bounds.size.y, 60.0);
        assert_eq!(scene.pages[0].children[0].css_paint_origin, Some(op_editor_core::render_backend::Point2D::ZERO));
        assert_eq!(scene.pages[0].children[1].css_paint_origin, None);
    }
    let preview =
        crate::adapter::pen_documents_to_payload_for_preview(&state.doc, &state.doc, false);
    assert_eq!(preview.payload.pages[0].children[0].w, 100.5);
    assert_eq!(preview.payload.pages[0].children[1].w, 101.0);
    assert_eq!(
        serde_json::to_string(&state.doc).unwrap(),
        original,
        "Projection must not mutate the source document"
    );
}

#[test]
fn css_paint_origin_is_root_relative_and_stops_at_transformed_subtrees() {
    let state = state_from(r#"{
      "version":"1.0.0",
      "conversion":{"entries":[{"kind":"screen","key":"html-snapshot:v1:fixture","nodeId":"html"}]},
      "children":[{"id":"html","type":"frame","x":10.25,"y":20.5,"width":300,"height":200,"children":[
        {"id":"plain","type":"rectangle","x":4.2,"y":8.6,"width":20.5,"height":10.25},
        {"id":"rotated","type":"frame","rotation":20,"width":100,"height":50,"children":[
          {"id":"nested","type":"rectangle","width":20,"height":10}
        ]}
      ]}]
    }"#);
    for preserve in [false, true] {
        let mut state = state.clone();
        state.editor_ui.preserve_authored_geometry = preserve;
        for scene in [editor_state_to_layout_scene(&state), editor_state_to_active_page_layout_scene(&state)] {
            let root = &scene.pages[0].children[0];
            assert_eq!(root.css_paint_origin, Some(op_editor_core::render_backend::Point2D::new(10.25, 20.5)));
            assert_eq!(root.children[0].css_paint_origin, root.css_paint_origin);
            assert_eq!(root.children[1].css_paint_origin, None);
            assert_eq!(root.children[1].children[0].css_paint_origin, None);
        }
    }
}

#[test]
fn interactive_scene_keeps_page_indices_but_builds_only_active_children() {
    let mut state = state_from(
        r#"{
          "version":"1.0.0",
          "pages":[
            {"id":"a","name":"A","children":[
              {"type":"rectangle","id":"a-rect","x":0,"y":0,"width":10,"height":10}
            ]},
            {"id":"b","name":"B","children":[
              {"type":"rectangle","id":"b-rect","x":20,"y":30,"width":40,"height":50}
            ]}
          ]
        }"#,
    );
    state.ui.active_page_index = 1;

    let scene = editor_state_to_active_page_layout_scene(&state);

    assert_eq!(scene.pages.len(), 2);
    assert_eq!(scene.active_page_index, 1);
    assert!(scene.pages[0].children.is_empty());
    assert_eq!(scene.pages[1].children.len(), 1);
    assert_eq!(scene.pages[1].children[0].id, "b-rect");
    assert!(scene.content_bounds().is_some());
}

#[test]
fn active_page_refs_can_still_resolve_masters_from_an_inactive_page() {
    let mut state = state_from(
        r#"{
          "version":"1.0.0",
          "pages":[
            {"id":"library","name":"Library","children":[
              {"type":"frame","id":"button","name":"Button","reusable":true,
               "x":0,"y":0,"width":100,"height":40,"children":[
                 {"type":"rectangle","id":"button-bg","x":0,"y":0,"width":100,"height":40}
               ]}
            ]},
            {"id":"screen","name":"Screen","children":[
              {"type":"ref","id":"button-instance","ref":"button",
               "x":50,"y":60,"width":100,"height":40}
            ]}
          ]
        }"#,
    );
    state.ui.active_page_index = 1;

    let scene = editor_state_to_active_page_layout_scene(&state);
    let instance = scene.pages[1]
        .find("button-instance")
        .expect("instance expands from inactive-page master");

    assert_eq!(instance.children.len(), 1);
    assert_eq!(instance.children[0].id, "button-instance__button-bg");
    assert!(scene.pages[0].children.is_empty());
}

#[test]
fn active_page_refs_keep_legacy_non_reusable_target_fallback() {
    let mut state = state_from(
        r#"{
          "version":"1.0.0",
          "pages":[
            {"id":"library","name":"Library","children":[
              {"type":"frame","id":"legacy-button","name":"Legacy Button",
               "x":0,"y":0,"width":90,"height":36,"children":[
                 {"type":"rectangle","id":"legacy-bg","x":0,"y":0,
                  "width":90,"height":36}
               ]}
            ]},
            {"id":"screen","name":"Screen","children":[
              {"type":"ref","id":"legacy-instance","ref":"legacy-button",
               "x":30,"y":40,"width":90,"height":36}
            ]}
          ]
        }"#,
    );
    state.ui.active_page_index = 1;
    assert!(
        state.components.is_empty(),
        "target is intentionally not reusable"
    );

    let scene = editor_state_to_active_page_layout_scene(&state);
    let instance = scene.pages[1]
        .find("legacy-instance")
        .expect("legacy non-reusable target resolves through the document fallback");

    assert_eq!(instance.children.len(), 1);
    assert_eq!(instance.children[0].id, "legacy-instance__legacy-bg");
}

#[test]
fn edited_component_master_uses_live_document_instead_of_stale_registry_snapshot() {
    let mut state = state_from(
        r#"{
          "version":"1.0.0",
          "pages":[
            {"id":"library","name":"Library","children":[
              {"type":"frame","id":"card","name":"Card","reusable":true,
               "x":0,"y":0,"width":100,"height":40,"children":[
                 {"type":"rectangle","id":"card-bg","x":0,"y":0,
                  "width":100,"height":40}
               ]}
            ]},
            {"id":"screen","name":"Screen","children":[
              {"type":"ref","id":"card-instance","ref":"card",
               "x":20,"y":30,"width":100,"height":40}
            ]}
          ]
        }"#,
    );
    state.ui.active_page_index = 1;

    let pages = state.doc.pages.as_mut().expect("fixture has pages");
    let master =
        op_editor_core::walkers::find_node_mut(&mut pages[0].children, &NodeId::new("card-bg"))
            .expect("master child exists");
    master.set_width_px(175.0);
    state.mark_document_changed();

    // The runtime component registry intentionally remains the load-time
    // prototype. An edited document must therefore bypass it and match the
    // canonical full builder.
    let full = editor_state_to_layout_scene(&state);
    let active = editor_state_to_active_page_layout_scene(&state);
    assert_eq!(active.pages[1], full.pages[1]);
}

#[test]
fn active_page_matches_full_builder_with_refs_variables_and_both_layout_modes() {
    let mut state = state_from(
        r##"{
          "version":"1.0.0",
          "pages":[
            {"id":"library","name":"Library","children":[
              {"type":"frame","id":"card","name":"Card","reusable":true,
               "x":5,"y":7,"width":120,"height":60,"children":[
                 {"type":"rectangle","id":"card-bg","x":2,"y":3,
                  "width":116,"height":54,
                  "fill":[{"type":"solid","color":"#112233"}]}
               ]}
            ]},
            {"id":"screen","name":"Screen","children":[
              {"type":"ref","id":"card-instance","ref":"card",
               "x":40,"y":50,"width":120,"height":60},
              {"type":"rectangle","id":"accent","x":200,"y":80,
               "width":30,"height":20,
               "fill":[{"type":"solid","color":"#000000"}]}
            ]}
          ]
        }"##,
    );
    state.ui.active_page_index = 1;
    state.create_variable(
        "brand",
        VariableKind::Color,
        VariableScalar::Str("#ff8800".into()),
    );
    state
        .ui
        .variables
        .fill_refs
        .insert(NodeId::new("accent"), "brand".into());

    for preserve in [false, true] {
        state.editor_ui.preserve_authored_geometry = preserve;
        let full = editor_state_to_layout_scene(&state);
        let active = editor_state_to_active_page_layout_scene(&state);

        assert_eq!(active.active_page_index, full.active_page_index);
        assert_eq!(active.pages.len(), full.pages.len());
        assert_eq!(active.pages[0].id, full.pages[0].id);
        assert_eq!(active.pages[0].name, full.pages[0].name);
        assert!(active.pages[0].children.is_empty());
        assert_eq!(active.pages[1], full.pages[1]);
    }
}
