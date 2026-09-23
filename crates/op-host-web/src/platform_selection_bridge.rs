//! On-demand, read-only selection bridge for managed Studio inspectors.
//! No document data or credentials cross this channel; the parent resolves IDs
//! against its own authenticated source checkpoint.

use std::{cell::RefCell, rc::Rc};
use wasm_bindgen::{closure::Closure, JsCast, JsValue};
use web_sys::MessageEvent;

use crate::repaint_ctx::RepaintContext;

fn request_id(raw: &str) -> Option<String> {
    correlated_request_id(raw, "ids-agent:openpencil-selection-read")
}

fn correlated_request_id(raw: &str, kind: &str) -> Option<String> {
    if raw.len() > 512 {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(raw).ok()?;
    if value.get("type")?.as_str()? != kind {
        return None;
    }
    let id = value.get("requestId")?.as_str()?;
    if id.is_empty() || id.len() > 80 || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return None;
    }
    Some(id.to_owned())
}

pub(crate) fn install<C: RepaintContext + 'static>(inner: &Rc<RefCell<C>>) {
    if !crate::platform_chat_bridge::studio_managed() {
        return;
    }
    let Some(window) = web_sys::window() else {
        return;
    };
    let inner = Rc::downgrade(inner);
    let listener =
        Closure::<dyn FnMut(MessageEvent)>::wrap(Box::new(move |event: MessageEvent| {
            let Some(parent) = web_sys::window().and_then(|w| w.parent().ok().flatten()) else {
                return;
            };
            let Some(source) = event.source() else {
                return;
            };
            if !js_sys::Object::is(source.as_ref(), parent.as_ref()) || event.origin() == "null" {
                return;
            }
            let Some(raw) = event.data().as_string() else {
                return;
            };
            if let Some(id) = correlated_request_id(&raw, "ids-agent:openpencil-document-readiness") {
                let authority = crate::live_sync_glue::applied_document_authority()
                    .and_then(|json| serde_json::from_str::<serde_json::Value>(&json).ok());
                let payload = serde_json::json!({
                    "type": "ids-agent:openpencil-document-readiness-result",
                    "requestId": id,
                    "authority": authority,
                });
                let _ = parent.post_message(&JsValue::from_str(&payload.to_string()), &event.origin());
                return;
            }
            let Some(id) = request_id(&raw) else {
                return;
            };
            let Some(inner) = inner.upgrade() else {
                return;
            };
            let Ok(shell) = inner.try_borrow() else {
                return;
            };
            let selected = &shell.host().editor_state().selection.set;
            let ids: Vec<&str> = selected.iter().take(128).map(|id| id.as_str()).collect();
            let payload = serde_json::json!({
                "type": "ids-agent:openpencil-selection-result",
                "requestId": id,
                "selectedIds": ids,
                "truncated": selected.len() > 128,
            });
            let _ = parent.post_message(&JsValue::from_str(&payload.to_string()), &event.origin());
        }));
    if window
        .add_event_listener_with_callback("message", listener.as_ref().unchecked_ref())
        .is_ok()
    {
        listener.forget();
    }
}

#[cfg(test)]
mod tests {
    use super::{correlated_request_id, request_id};
    #[test]
    fn readiness_requests_are_separate_and_bounded() {
        let kind = "ids-agent:openpencil-document-readiness";
        let raw = r#"{"type":"ids-agent:openpencil-document-readiness","requestId":"abc-123"}"#;
        assert_eq!(correlated_request_id(raw, kind), Some("abc-123".into()));
        assert!(request_id(raw).is_none());
        assert!(correlated_request_id(&"x".repeat(513), kind).is_none());
        assert!(correlated_request_id(&raw.replace("abc-123", "bad/id"), kind).is_none());
    }
    #[test]
    fn selection_reads_require_bounded_correlated_requests() {
        assert_eq!(
            request_id(r#"{"type":"ids-agent:openpencil-selection-read","requestId":"abc-123"}"#),
            Some("abc-123".into())
        );
        for raw in [
            "{}",
            r#"{"type":"ids-agent:openpencil-selection-read","requestId":""}"#,
            r#"{"type":"ids-agent:openpencil-selection-read","requestId":"bad/id"}"#,
            r#"{"type":"other","requestId":"abc"}"#,
        ] {
            assert!(request_id(raw).is_none());
        }
        assert!(request_id(&"x".repeat(513)).is_none());
    }
}
