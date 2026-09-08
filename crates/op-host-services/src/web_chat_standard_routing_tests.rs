use super::*;

fn compact_fixture() -> EditorState {
    EditorState::from_document(
        serde_json::from_value(serde_json::json!({
            "version":"1.0", "children":[
                {"type":"frame","id":"mobile","width":390,"height":1300,"children":[
                    {"type":"text","id":"body","content":"Keep this content"},
                    {"type":"frame","id":"duplicate-sidebar","width":260,"height":900,"children":[]}
                ]},
                {"type":"frame","id":"desktop","width":1200,"height":900,"children":[]}
            ]
        }))
        .unwrap(),
    )
}

#[test]
fn compact_modify_deletes_only_requested_sidebar_and_preserves_content() {
    let mut state = compact_fixture();
    let ops = crate::chat_intent::parse_modify_nodes(
        r#"```json
        [{"op":"delete","id":"duplicate-sidebar"},{"op":"update","id":"mobile","data":{"height":844}}]
    ```"#,
    );
    assert_eq!(ops.len(), 2);
    assert_eq!(
        crate::chat_canvas_tools::apply_design_modification(&mut state, &ops, &["mobile".into()]),
        (2, true)
    );
    let doc = serde_json::to_value(&state.doc).unwrap();
    assert_eq!(doc["children"][0]["height"].as_f64(), Some(844.0));
    assert_eq!(doc["children"][0]["children"].as_array().unwrap().len(), 1);
    assert_eq!(
        doc["children"][0]["children"][0]["content"],
        "Keep this content"
    );
    assert_eq!(doc["children"][1]["id"], "desktop");
}

#[test]
fn compact_modify_rejects_out_of_scope_or_structural_patches_atomically() {
    for response in [
        r#"[{"op":"delete","id":"duplicate-sidebar"},{"op":"delete","id":"desktop"}]"#,
        r#"[{"op":"delete","id":"duplicate-sidebar"},{"op":"update","id":"mobile","data":{"children":[]}}]"#,
        r#"[{"op":"delete","id":"duplicate-sidebar"},{"op":"update","id":"mobile","data":{"type":"text"}}]"#,
    ] {
        let mut state = compact_fixture();
        let before = serde_json::to_value(&state.doc).unwrap();
        let ops = crate::chat_intent::parse_modify_nodes(response);
        assert_eq!(
            crate::chat_canvas_tools::apply_design_modification(
                &mut state,
                &ops,
                &["mobile".into()]
            ),
            (0, false)
        );
        assert_eq!(serde_json::to_value(&state.doc).unwrap(), before);
    }
}

#[test]
fn modify_without_selected_frames_never_becomes_new_design() {
    use crate::chat_intent::DesignIntent::{Chat, Modify, New};
    assert_eq!(resolve_standard_route(Modify, false, false), None);
    assert_eq!(resolve_standard_route(Modify, true, false), None);
    assert_eq!(resolve_standard_route(Modify, false, true), Some(Modify));
    assert_eq!(resolve_standard_route(New, false, false), Some(New));
    assert_eq!(resolve_standard_route(Chat, false, false), Some(Chat));
    assert_eq!(
        crate::chat_intent::classify_by_keywords(
            "เช็ค mobile design หน่อย sidebar ค่อนข้าง mess up แยกออกมาเป็นอันเดียวตางหากได้ไหม"
        ),
        Modify
    );
}
