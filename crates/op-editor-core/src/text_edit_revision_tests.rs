use crate::{node_id::NodeId, state::EditorState, sync_gate::SyncGate};

fn editing() -> EditorState {
    let doc = serde_json::from_str(
        r#"{"version":"1.0","children":[{"id":"label","type":"text","content":"Original"}]}"#,
    )
    .unwrap();
    let mut state = EditorState::from_document(doc);
    assert!(state.start_text_edit(NodeId::new("label")));
    assert!(state.text_edit_select_all_now(1));
    state
}

#[test]
fn coalesced_typing_after_ack_stays_dirty_and_pushable() {
    let mut state = editing();
    assert!(state.text_edit_insert("Updated", 100));
    let pair = (state.document_generation(), state.document_revision());
    let mut gate = SyncGate::default();
    gate.note_synced(pair.0, pair.1);
    assert!(state.mark_saved_revision_at(pair.0, pair.1));
    assert!(state.text_edit_insert(" contact", 110));
    assert!(state.document_revision() > pair.1);
    assert!(state.is_dirty());
    assert!(gate.needs_push((state.document_generation(), state.document_revision())));
    assert!(!gate.pull_allowed((state.document_generation(), state.document_revision())));
    assert!(state.text_edit_commit());
    assert!(state.undo());
    assert!(state.start_text_edit(NodeId::new("label")));
    assert_eq!(state.text_edit_content(), Some("Original"));
    assert!(
        !state.undo(),
        "The complete typing burst must remain one undo entry"
    );
}

#[test]
fn coalesced_deletes_advance_revision_but_noop_insert_does_not() {
    let mut state = editing();
    assert!(state.text_edit_insert("ABC", 100));
    let first = state.document_revision();
    assert!(state.text_edit_backspace(110));
    assert!(state.document_revision() > first);
    let second = state.document_revision();
    assert!(state.text_edit_set_caret(0, false, 115));
    assert!(state.text_edit_delete_forward(120));
    assert!(state.document_revision() > second);
    let third = state.document_revision();
    assert!(state.text_edit_insert("", 125));
    assert_eq!(state.document_revision(), third);
    assert_eq!(state.text_edit_content(), Some("B"));
}

#[test]
fn coalesced_ime_commit_advances_only_canonical_changes() {
    let mut state = editing();
    assert!(state.text_edit_insert("A", 100));
    let first = state.document_revision();
    assert!(state.text_edit_set_composition("ไทย", 3, 110));
    assert_eq!(
        state.document_revision(),
        first,
        "Uncommitted IME is only a preview"
    );
    assert!(state.text_edit_commit_composition(120));
    assert!(state.document_revision() > first);
    assert_eq!(state.text_edit_content(), Some("Aไทย"));
    assert!(state.text_edit_select_all_now(125));
    let committed = state.document_revision();
    assert!(state.text_edit_backspace(130));
    assert!(state.document_revision() > committed);
    assert!(state.text_edit_commit());
    assert!(state.undo());
    assert!(state.start_text_edit(NodeId::new("label")));
    assert_eq!(state.text_edit_content(), Some("Original"));
}
