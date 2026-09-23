use super::*;

struct ModifyStreamProvider {
    stop: StopReason,
}

struct InvalidInteractionProvider;

impl ChatProvider for InvalidInteractionProvider {
    fn provider_label(&self) -> &str { "invalid-interaction-regression" }
    fn send(&self, _: ChatRequest) -> Box<dyn Iterator<Item = ChatDelta> + Send> {
        Box::new([
            ChatDelta::TextDelta(r#"[{"op":"update","id":"mobile","data":{"events":{"onClick":[{"pop":null}]}}}]"#.into()),
            ChatDelta::Done { stop_reason: StopReason::EndTurn },
        ].into_iter())
    }
}

#[test]
fn modify_route_reports_invalid_interaction_without_applied_or_mutation() {
    let state = Mutex::new(WebCanvasState::new(compact_fixture(), 3100));
    let before = serde_json::to_value(&state.lock().unwrap().editor.doc).unwrap();
    let mut out = Vec::new();
    stream_modify_route(
        &mut out,
        crate::chat_intent::ModifyPlan {
            system_prompt: String::new(), user_message: "wire button".into(),
            target_frame_ids: vec!["mobile".into()],
        },
        &InvalidInteractionProvider, &state, &SseHub::default(), None,
    ).unwrap();
    assert_eq!(serde_json::to_value(&state.lock().unwrap().editor.doc).unwrap(), before);
    let output = String::from_utf8(out).unwrap();
    assert!(output.contains("Invalid design interaction"), "{output}");
    assert!(output.contains("No changes were applied"), "{output}");
    assert!(!output.contains("APPLIED"), "{output}");
}

impl ChatProvider for ModifyStreamProvider {
    fn provider_label(&self) -> &str {
        "modify-regression"
    }
    fn send(&self, request: ChatRequest) -> Box<dyn Iterator<Item = ChatDelta> + Send> {
        assert_eq!(
            request.thinking,
            op_ai::chat_provider::ThinkingMode::Disabled
        );
        Box::new(
            [
                ChatDelta::TextDelta(
                    r#"[{"op":"update","id":"mobile","data":{"name":"Edited"}}]"#.into(),
                ),
                ChatDelta::Done {
                    stop_reason: self.stop,
                },
            ]
            .into_iter(),
        )
    }
}

#[test]
fn modify_route_disables_reasoning_and_applies_only_complete_streams() {
    for stop in [
        StopReason::EndTurn,
        StopReason::MaxTokens,
        StopReason::Aborted,
    ] {
        let state = Mutex::new(WebCanvasState::new(compact_fixture(), 3100));
        let before = serde_json::to_value(&state.lock().unwrap().editor.doc).unwrap();
        let mut out = Vec::new();
        stream_modify_route(
            &mut out,
            crate::chat_intent::ModifyPlan {
                system_prompt: String::new(),
                user_message: "rename".into(),
                target_frame_ids: vec!["mobile".into()],
            },
            &ModifyStreamProvider { stop },
            &state,
            &SseHub::default(),
            None,
        )
        .unwrap();
        let after = serde_json::to_value(&state.lock().unwrap().editor.doc).unwrap();
        if stop == StopReason::EndTurn {
            assert_eq!(after["children"][0]["name"], "Edited");
            let output = String::from_utf8(out).unwrap();
            assert!(output.contains("APPLIED"));
            assert!(output.contains("Applied 1 design change(s)"));
            assert!(!output.contains("Edited"), "Raw model JSON must not appear in chat: {output}");
        } else {
            assert_eq!(
                before, after,
                "incomplete output must not mutate the canvas"
            );
            assert!(!String::from_utf8(out).unwrap().contains("APPLIED"));
        }
    }
}

#[test]
fn rejected_modify_does_not_publish_raw_json_or_success_summary() {
    let state = Mutex::new(WebCanvasState::new(compact_fixture(), 3100));
    let before = serde_json::to_value(&state.lock().unwrap().editor.doc).unwrap();
    let mut out = Vec::new();
    stream_modify_route(
        &mut out,
        crate::chat_intent::ModifyPlan {
            system_prompt: String::new(), user_message: "rename".into(),
            target_frame_ids: vec!["desktop".into()],
        },
        &ModifyStreamProvider { stop: StopReason::EndTurn },
        &state, &SseHub::default(), None,
    ).unwrap();
    let output = String::from_utf8(out).unwrap();
    assert!(output.contains("No changes were applied"));
    assert!(!output.contains("Applied 1 design"));
    assert!(!output.contains("APPLIED"));
    assert!(!output.contains("Edited"));
    assert_eq!(serde_json::to_value(&state.lock().unwrap().editor.doc).unwrap(), before);
}

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
