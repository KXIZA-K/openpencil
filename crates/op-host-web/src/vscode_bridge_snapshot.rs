//! VS Code bridge document snapshot with separate sync and disk representations.
//!
//! Live sync hashes the typed `PenDocument` bytes plus editor metadata as
//! separate fields. The extension's on-disk snapshot embeds the same metadata
//! into `editorMeta`. Keeping those representations separate prevents the
//! bridge from poisoning the periodic-sync baseline with differently shaped
//! JSON and triggering a redundant whole-document push.

use std::rc::Rc;

use op_editor_core::web_sync::{WebSyncAuthority, WebSyncClient};
use op_editor_core::EditorState;
use op_pen_loader::EditorMeta;

#[derive(Clone)]
pub(super) struct BridgeDocumentSnapshot {
    pair: (u64, u64),
    document_json: Rc<str>,
    editor_meta: EditorMeta,
}

impl BridgeDocumentSnapshot {
    pub(super) fn capture(state: &EditorState) -> Option<Self> {
        Some(Self {
            pair: (state.document_generation(), state.document_revision()),
            document_json: serde_json::to_string(&state.doc).ok()?.into(),
            editor_meta: EditorMeta::from_state(state),
        })
    }

    pub(super) fn pair(&self) -> (u64, u64) {
        self.pair
    }

    pub(super) fn push_body_with_authority(&self, authority: &WebSyncAuthority) -> String {
        WebSyncClient::wrap_push_with_authority(
            self.document_json.as_ref(), authority,
            self.editor_meta.active_page_index, self.editor_meta.preserve_authored_geometry,
        )
    }

    /// Ordinary Save must retain the authority that supplied the local draft.
    /// Probing a newer server version here would silently turn Save into overwrite.
    pub(super) fn push_body_for_client(&self, client: &WebSyncClient) -> String {
        client.wrap_push_for_authority(
            self.document_json.as_ref(),
            self.editor_meta.active_page_index,
            self.editor_meta.preserve_authored_geometry,
            false,
        )
    }

    pub(super) fn mark_pushed(&self, client: &mut WebSyncClient, version: u64) {
        client.mark_pushed_with_editor_meta(
            self.document_json.as_ref(),
            self.editor_meta.active_page_index,
            self.editor_meta.preserve_authored_geometry,
            version,
        );
    }

    pub(super) fn externalized_json(&self) -> String {
        crate::document_json::externalize_for_disk(
            self.document_json.as_ref(),
            self.editor_meta.clone(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_retains_applied_authority_after_observing_a_restart() {
        let mut client = WebSyncClient::new();
        let response = r#"{"generation":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","version":7,"document":{"version":"1.0.0","children":[]}}"#;
        assert!(client.sync(response, |_, _| true).unwrap());
        assert!(client.wants_version_response(
            r#"{"generation":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","version":1}"#
        ));
        let doc = op_pen_loader::load_canonical(
            r#"{"version":"1.0.0","children":[]}"#,
        ).unwrap().value;
        let snapshot = BridgeDocumentSnapshot::capture(&EditorState::from_document(doc)).unwrap();
        let body: serde_json::Value = serde_json::from_str(&snapshot.push_body_for_client(&client)).unwrap();
        assert_eq!(body["baseGeneration"], "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        assert_eq!(body["baseVersion"], 7);
        assert!(body["document"].is_object());
    }

    #[test]
    fn sync_baseline_uses_typed_bytes_while_disk_snapshot_embeds_metadata() {
        let doc = op_pen_loader::load_canonical(
            r#"{"version":"1.0.0","children":[],"pages":[
              {"id":"p1","name":"One","children":[]},
              {"id":"p2","name":"Two","children":[]},
              {"id":"p3","name":"Three","children":[]}
            ]}"#,
        )
        .expect("document")
        .value;
        let mut state = EditorState::from_document(doc);
        assert!(state.set_active_page(2));
        state.editor_ui.preserve_authored_geometry = true;
        let snapshot = BridgeDocumentSnapshot::capture(&state).expect("snapshot");
        let mut client = WebSyncClient::new();

        snapshot.mark_pushed(&mut client, 7);

        assert_eq!(client.last_version(), 7);
        assert!(!client.should_push_with_editor_meta(snapshot.document_json.as_ref(), 2, true));
        assert_eq!(
            op_pen_loader::extract_editor_meta(&snapshot.externalized_json()),
            Some(EditorMeta {
                active_page_index: 2,
                preserve_authored_geometry: true,
                scenario: None,
                pinned_style_guide: None,
            })
        );
    }
}
