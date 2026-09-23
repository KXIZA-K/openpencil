use super::*;
use crate::widget_host::WidgetHost;
use wasm_bindgen::JsValue;

struct TestContext {
    host: WidgetHost,
    repaints: usize,
}

impl RepaintContext for TestContext {
    fn host(&self) -> &WidgetHost {
        &self.host
    }

    fn host_mut(&mut self) -> &mut WidgetHost {
        &mut self.host
    }

    fn viewport_size(&self) -> (f32, f32) {
        (1440.0, 900.0)
    }

    fn register_system_font(&mut self, _family: &str, _bytes: &[u8]) -> bool {
        false
    }

    fn register_imported_font(&mut self, _family: &str, _bytes: &[u8]) -> bool {
        false
    }

    fn register_imported_font_from_bytes(&mut self, _bytes: &[u8]) -> Option<String> {
        None
    }

    fn imported_family_list(&self) -> Vec<String> {
        Vec::new()
    }

    fn remove_imported_font(&mut self, _family: &str) {}

    fn repaint(&mut self) -> Result<(), JsValue> {
        self.repaints += 1;
        Ok(())
    }
}

fn minimal_document() -> op_editor_core::PenDocument {
    serde_json::from_str(r#"{"version":"1.0","children":[]}"#)
        .expect("minimal document should deserialize")
}

#[test]
fn capped_serializer_matches_regular_json_for_small_documents() {
    let doc = minimal_document();
    let expected = serde_json::to_string(&doc).expect("regular serialization should succeed");

    match serialize_sync_document(&doc) {
        SyncDocumentJson::Ready(actual) => assert_eq!(actual, expected),
        SyncDocumentJson::Oversize => panic!("small document should fit under the cap"),
        SyncDocumentJson::Failed => panic!("small document should serialize"),
    }
}

#[test]
fn capped_serializer_stops_oversized_documents_at_the_limit() {
    let mut doc = minimal_document();
    doc.name = Some("x".repeat(SYNC_MAX_BODY_BYTES));

    assert!(matches!(
        serialize_sync_document(&doc),
        SyncDocumentJson::Oversize
    ));
}

#[test]
fn capped_writer_accepts_exact_limit_then_rejects_another_byte() {
    let mut writer = CappedJsonWriter::new();
    let exact_limit = vec![b'x'; SYNC_MAX_BODY_BYTES];
    writer
        .write_all(&exact_limit)
        .expect("the exact cap should be accepted");
    assert_eq!(writer.bytes.len(), SYNC_MAX_BODY_BYTES);

    assert!(writer.write_all(b"x").is_err());
    assert!(writer.exceeded);
    assert_eq!(writer.bytes.len(), SYNC_MAX_BODY_BYTES);
}

#[test]
fn active_page_only_change_is_push_due_without_a_revision_change() {
    let mut sync = SyncController::new();
    let pair = (7, 11);
    sync.gate.note_synced(pair.0, pair.1);
    sync.client.mark_applied(3);
    sync.client.note_applied_snapshot_without_hash(0, true);

    assert!(!push_reasons(&sync, pair, 0, true).any());
    assert!(
        push_reasons(&sync, pair, 1, true).editor_meta,
        "page metadata must bypass the unchanged revision pair"
    );

    sync.gate.note_conflict(4);
    assert!(
        !push_reasons(&sync, pair, 1, true).any(),
        "metadata pushes must still honor the conflict latch"
    );
}

#[test]
fn bootstrap_pull_fits_restored_content_into_view() {
    let inner = Rc::new(RefCell::new(TestContext {
        host: WidgetHost::new(),
        repaints: 0,
    }));
    let sync = Rc::new(RefCell::new(SyncController::new()));
    let last_selection_key = Rc::new(RefCell::new(None));
    let response = serde_json::json!({
        "document": {
            "version": "1.0",
            "children": [{
                "type": "frame",
                "id": "restored",
                "name": "Restored screen",
                "x": 1_500,
                "y": 400,
                "width": 1_200,
                "height": 900,
                "children": []
            }]
        },
        "version": 1
    })
    .to_string();

    apply_document_response(&inner, &response, &sync, &last_selection_key);

    let context = inner.borrow();
    assert_eq!(context.repaints, 1);
    assert_eq!(context.host.editor_state().doc.children.len(), 1);
    assert!(
        context.host.editor_state().viewport.pan_x < 0.0,
        "off-screen restored content should be centered on the first pull"
    );
    assert!(
        context.host.editor_state().viewport.zoom < 1.0,
        "restored content should fit the available canvas"
    );
}
