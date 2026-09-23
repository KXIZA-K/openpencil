//! Parsing a whole-document push, off the state lock.
//!
//! Split out of the `web_canvas_server` spine at the 800-line cap. The point
//! of the type is WHERE it is built — see [`PendingDocumentPush`].

use super::{Result, ServeMode, WebCanvasError, WebCanvasState, WebReply};

/// A whole-document push, parsed and validated but not yet installed.
///
/// The point of the type is WHERE it is built: JSON parse, schema load and
/// structural validation are the expensive, entirely self-contained part of a
/// push, and they used to run with the state mutex held — so one client
/// uploading a large document with embedded images stalled every other
/// request to that tenant, including the SSE version probe. Building this
/// outside the lock leaves only the install under it.
pub(crate) struct PendingDocumentPush {
    pub(super) base_version: Option<u64>,
    pub(super) base_generation: Option<String>,
    pub(super) editor_meta: op_pen_loader::EditorMeta,
    /// `None` for a metadata-only push (an active-page switch), which carries
    /// no document to install.
    pub(super) prepared: Option<op_editor_core::PreparedDocument>,
}

impl PendingDocumentPush {
    /// Take the editor metadata, leaving an empty one behind.
    ///
    /// A method because the type owns a `Drop`, which forbids moving fields
    /// out of it directly.
    pub(super) fn take_editor_meta(&mut self) -> op_pen_loader::EditorMeta {
        std::mem::take(&mut self.editor_meta)
    }
}

impl Drop for PendingDocumentPush {
    /// Release the pending thumbnail seed of a push that was parsed and then
    /// dropped — a stale `baseVersion`, a collaboration refusal, or a
    /// connection that went away between parse and install.
    ///
    /// `load_canonical` registers the seed in a process-global side table
    /// keyed by the document, and only an activation consumes it. A push that
    /// never installs would leave it there until bounded eviction pushed it
    /// out, and in the meantime a *different* document that happened to reuse
    /// the key could activate someone else's thumbnails.
    fn drop(&mut self) {
        if let Some(prepared) = self.prepared.as_ref() {
            jian_ops_schema::image_thumbs::discard_for_document(prepared.document());
        }
    }
}

impl PendingDocumentPush {
    /// Parse and validate a push body. **Call this OUTSIDE the state lock.**
    ///
    /// `mode` decides only whether the document's thumbnails may be published
    /// to the process-global registry; nothing else here reads live state,
    /// which is what makes it safe to run unlocked.
    pub(crate) fn parse(body: &str, mode: ServeMode) -> Result<Self> {
        let request = crate::mcp_serve::parse_document_sync_request(body)?;
        let base_version = request.base_version;
        let editor_meta = request.resolved_editor_meta(request.embedded_editor_meta.clone());
        let base_generation = request.base_generation;
        if request.metadata_only {
            return Ok(Self {
                base_version,
                base_generation,
                editor_meta,
                prepared: None,
            });
        }
        // Load via the same proven path as desktop file-open. A load failure
        // is a client fault → 400, like the TS validation 400s.
        let loaded = op_pen_loader::load_canonical(request.document_json)
            .map_err(|e| WebCanvasError::Document(e.to_string()))?;
        if !mode.allows_image_thumb_registry() {
            // The thumbnail registry is a process-global map that a document
            // activation replaces WHOLESALE, so in a shared process this
            // push would drop every other account's thumbnails and rebind
            // their ids to these bytes. Dropping the pending seed here makes
            // the activation downstream a strict no-op, and image nodes
            // paint their placeholder. See
            // `ServeMode::allows_image_thumb_registry` for the real fix.
            jian_ops_schema::image_thumbs::discard_for_document(&loaded.value);
        }
        for w in &loaded.warnings {
            eprintln!("openpencil-desktop --serve-web: schema warning: {w:?}");
        }
        // Structural validation happens here, before any state changes, so the
        // in-session install is infallible and cannot leave a half-applied
        // document inside an open edit capture.
        let prepared = op_editor_core::PreparedDocument::prepare(loaded.value)
            .map_err(|e| WebCanvasError::Document(e.to_string()))?;
        Ok(Self {
            base_version,
            base_generation,
            editor_meta,
            prepared: Some(prepared),
        })
    }
}

/// Render the reply for an already-parsed document push.
///
/// Shares its verdict shape with the in-handler route below, so the two paths
/// cannot answer the same outcome differently.
pub(crate) fn document_push_reply(
    parsed: Result<PendingDocumentPush>,
    state: &mut WebCanvasState,
) -> WebReply {
    match parsed.and_then(|push| state.apply_prepared_document_push(push, None)) {
        Ok(outcome) if outcome.applied => WebReply {
            status: "200 OK",
            body: crate::mcp_serve::document_sync_ok(outcome.current_version),
        },
        Ok(outcome) => WebReply {
            // Stale baseVersion: reject without writing, plus the current
            // version so the caller can decide whether to refetch and retry.
            status: "409 Conflict",
            body: serde_json::json!({
                "ok": false,
                "error": "version-conflict",
                "version": outcome.current_version,
                "generation": state.generation,
            })
            .to_string(),
        },
        Err(error) => collab_aware_error_reply(&error),
    }
}

#[cfg(test)]
pub(crate) fn collab_aware_error_reply_for_test(error: &WebCanvasError) -> WebReply {
    collab_aware_error_reply(error)
}

pub(super) fn collab_aware_error_reply(error: &WebCanvasError) -> WebReply {
    // An ingest rejection must carry the authoritative version: it is what the
    // browser refetches from, and without it the conflict recovery never runs.
    if let WebCanvasError::IngestRejected(_, version) = error {
        return WebReply {
            status: error.http_status(),
            body: serde_json::json!({
                "ok": false,
                "error": error.error_code().unwrap_or("version-conflict"),
                "version": version,
                "message": error.to_string(),
            })
            .to_string(),
        };
    }
    match error.error_code() {
        Some(code) => WebReply {
            status: error.http_status(),
            body: serde_json::json!({
                "ok": false,
                "error": code,
                "message": error.to_string(),
            })
            .to_string(),
        },
        None => WebReply {
            status: error.http_status(),
            body: crate::mcp_serve::rest_error_body(&error.to_string()),
        },
    }
}
