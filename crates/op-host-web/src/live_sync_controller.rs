//! The per-editor sync controller and the identity pairs every gating
//! decision is keyed on.
//!
//! Split out of `live_sync_glue.rs` at the 800-line cap — pure code motion.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use op_editor_core::sync_gate::SyncGate;
use op_editor_core::web_sync::{WebSyncAuthority, WebSyncClient};

use crate::repaint_ctx::RepaintContext;

use super::ACTIVE_SYNC;

/// Shared sync state for one mounted editor.
///
/// The gate and the client must see the same document identity, so they live
/// in one struct rather than as two independently-owned pieces.
pub(crate) struct SyncController {
    pub gate: SyncGate,
    pub client: WebSyncClient,
    pub push_busy: bool,
    pub conflict_authority: Option<WebSyncAuthority>,
    /// A document identity already measured above the periodic push limit.
    /// WASM linear memory does not shrink after a giant temporary JSON string,
    /// so do not rebuild the same oversized snapshot every two seconds.
    pub(super) oversize_identity: Option<(u64, u64, u64)>,
}

impl SyncController {
    pub(crate) fn new() -> Self {
        Self {
            gate: SyncGate::default(),
            client: WebSyncClient::new(),
            push_busy: false,
            conflict_authority: None,
            oversize_identity: None,
        }
    }

    pub(crate) fn note_conflict_response(&mut self, body: &str, version: u64) {
        self.conflict_authority = self.client.parse_authority(body).ok()
            .filter(|authority| authority.version == version);
        self.gate.note_conflict(version);
    }
}

pub(crate) type SharedSync = Rc<RefCell<SyncController>>;

/// The baseline belongs to a document successfully applied and repainted,
/// not to a version probe or a document still being fetched.
pub(crate) fn applied_document_authority() -> Option<String> {
    ACTIVE_SYNC.with(|slot| {
        let sync = slot.borrow().as_ref()?.upgrade()?;
        let sync = sync.try_borrow().ok()?;
        if !sync.client.initialized() { return None; }
        sync.client.fence_save_envelope("{}").ok()
    })
}

pub(crate) fn fence_daemon_save(body: &str) -> Option<String> {
    ACTIVE_SYNC.with(|slot| {
        let sync = slot.borrow().as_ref()?.upgrade()?;
        let sync = sync.try_borrow().ok()?;
        sync.client.fence_save_envelope(body).ok()
    })
}

pub(crate) fn note_daemon_save_conflict(response: &str) {
    let Some(version) = WebSyncClient::parse_push_conflict(response) else { return; };
    ACTIVE_SYNC.with(|slot| {
        let Some(sync) = slot.borrow().as_ref().and_then(Weak::upgrade) else { return; };
        if let Ok(mut sync) = sync.try_borrow_mut() {
            sync.note_conflict_response(response, version);
        };
    });
}

#[cfg(test)]
mod authority_tests {
    use super::*;

    #[test]
    fn malformed_conflict_cannot_reuse_a_previous_authority() {
        let mut sync = SyncController::new();
        sync.client.sync(
            r#"{"generation":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","version":7,"document":{"version":"1.0.0","children":[]}}"#,
            |_, _| true,
        ).unwrap();
        sync.note_conflict_response(
            r#"{"generation":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","version":1}"#, 1,
        );
        assert_eq!(sync.conflict_authority.as_ref().unwrap().version, 1);
        assert_eq!(sync.gate.conflict(), Some(1));
        sync.note_conflict_response(r#"{"version":1}"#, 1);
        assert!(sync.conflict_authority.is_none());
        assert_eq!(sync.gate.conflict(), Some(1));
        sync.note_conflict_response(
            r#"{"generation":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","version":2}"#, 1,
        );
        assert!(sync.conflict_authority.is_none());
    }
}

/// Commit a successful daemon Save as a sync acknowledgement.
///
/// The daemon has already installed exactly the saved snapshot. Recording its
/// version prevents the next probe from downloading and replacing the same
/// potentially huge document, while the snapshot pair reopens the pull gate.
/// A later local edit still differs from this pair and remains eligible for a
/// normal push (or another explicit Save for oversized documents).
pub(crate) fn acknowledge_daemon_save(
    version: u64,
    generation: u64,
    revision: u64,
    active_page_index: usize,
    preserve_authored_geometry: bool,
) {
    ACTIVE_SYNC.with(|slot| {
        let Some(sync) = slot.borrow().as_ref().and_then(Weak::upgrade) else {
            return;
        };
        let mut sync = sync.borrow_mut();
        sync.client.mark_applied(version);
        sync.client
            .note_applied_snapshot_without_hash(active_page_index, preserve_authored_geometry);
        sync.gate.note_synced(generation, revision);
    });
}

/// The document-identity pair every gating decision is keyed on. Read fresh
/// from the live editor state at each decision point — never cached — so an
/// edit that lands between a tick firing and its async response landing is
/// always observed.
pub(super) fn current_pair<C: RepaintContext>(b: &C) -> (u64, u64) {
    let s = b.host().editor_state();
    (s.document_generation(), s.document_revision())
}

pub(super) fn current_oversize_identity<C: RepaintContext>(b: &C) -> (u64, u64, u64) {
    let host = b.host();
    let state = host.editor_state();
    let doc = state;
    (
        host.document_epoch(),
        doc.document_generation(),
        doc.document_revision(),
    )
}
