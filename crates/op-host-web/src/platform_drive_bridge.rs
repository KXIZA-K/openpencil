//! Drive references originate in the Rust composer; the authenticated parent
//! owns Finder and resolves file contents immediately before turn admission.
use crate::{repaint_ctx::RepaintContext, web_chat::PreparedTurn};
use std::{cell::RefCell, collections::HashMap, rc::Rc};
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use web_sys::MessageEvent;

type Resolved = Box<dyn FnOnce(Result<PreparedTurn, String>)>;
struct Pick {
    id: String,
    thread: Option<String>,
    draft: String,
    start: usize,
    end: usize,
}
thread_local! {
    static PICK: RefCell<Option<Pick>> = const { RefCell::new(None) };
    static DISMISSED: RefCell<String> = const { RefCell::new(String::new()) };
    static RESOLVE: RefCell<HashMap<String, (PreparedTurn, Resolved)>> = RefCell::new(HashMap::new());
}
fn post(value: serde_json::Value) {
    if let Some(parent) = web_sys::window().and_then(|w| w.parent().ok().flatten()) {
        let _ = parent.post_message(&JsValue::from_str(&value.to_string()), "*");
    }
}
fn id() -> String {
    format!("drive-{}-{}", js_sys::Date::now(), js_sys::Math::random())
}
fn mention(text: &str, caret: usize) -> Option<(usize, usize, &str)> {
    let prefix = text.get(..caret)?;
    let start = prefix.rfind('@')?;
    if start > 0 && !prefix[..start].chars().last()?.is_whitespace() {
        return None;
    }
    let query = &prefix[start + 1..];
    if query
        .chars()
        .any(|c| c.is_whitespace() || matches!(c, '@' | '[' | ']'))
    {
        return None;
    }
    Some((start, caret, query))
}
pub(crate) fn poll<C: RepaintContext + 'static>(inner: &Rc<RefCell<C>>) {
    if !crate::platform_chat_bridge::studio_managed() || PICK.with(|v| v.borrow().is_some()) {
        return;
    }
    let Ok(shell) = inner.try_borrow() else {
        return;
    };
    let chat = &shell.host().editor_state().chat;
    if !chat.focused || chat.input.composition().is_some() {
        return;
    }
    let draft = chat.input.text();
    if DISMISSED.with(|v| *v.borrow() == draft) {
        return;
    }
    let Some((start, end, query)) = mention(draft, chat.input.caret()) else {
        return;
    };
    let request_id = id();
    PICK.with(|v| {
        *v.borrow_mut() = Some(Pick {
            id: request_id.clone(),
            thread: chat.thread_id.clone(),
            draft: draft.into(),
            start,
            end,
        })
    });
    post(
        serde_json::json!({ "type": "ids-agent:openpencil-drive-find", "requestId": request_id, "query": query }),
    );
}

pub(crate) fn cancel() {
    RESOLVE.with(|v| v.borrow_mut().clear());
}

pub(crate) fn resolve(prepared: PreparedTurn, callback: Resolved) {
    if !crate::platform_chat_bridge::studio_managed()
        || !prepared.user_text.contains("](drive:tart_")
    {
        callback(Ok(prepared));
        return;
    }
    let request_id = id();
    post(
        serde_json::json!({ "type": "ids-agent:openpencil-drive-resolve", "requestId": request_id, "prompt": prepared.user_text }),
    );
    let timeout_id = request_id.clone();
    RESOLVE.with(|v| {
        v.borrow_mut().insert(request_id, (prepared, callback));
    });
    let timeout = Closure::<dyn FnMut()>::once(move || {
        let pending = RESOLVE.with(|v| v.borrow_mut().remove(&timeout_id));
        if let Some((_, callback)) = pending {
            callback(Err("Drive references timed out. Try again.".into()));
        }
    });
    if let Some(window) = web_sys::window() {
        let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(
            timeout.as_ref().unchecked_ref(),
            30_000,
        );
        timeout.forget();
    }
}
fn apply_context(prepared: &mut PreparedTurn, context: &serde_json::Value) -> Result<(), String> {
    let prompt = context
        .get("prompt")
        .and_then(|v| v.as_str())
        .ok_or("Drive context is missing its prompt")?;
    let attachments = context
        .get("attachments")
        .and_then(|v| v.as_array())
        .ok_or("Drive context is missing its attachments")?;
    let mut body: serde_json::Value =
        serde_json::from_str(&prepared.body_json).map_err(|_| "Invalid prepared turn")?;
    body["intentUser"] = prepared.user_text.clone().into();
    body["user"] = prompt.into();
    let existing = body["attachments"]
        .as_array_mut()
        .ok_or("Invalid prepared attachments")?;
    existing.extend(attachments.iter().cloned());
    prepared.body_json = body.to_string();
    Ok(())
}
pub(crate) fn install<C: RepaintContext + 'static>(inner: &Rc<RefCell<C>>) {
    if !crate::platform_chat_bridge::studio_managed() {
        return;
    }
    let Some(window) = web_sys::window() else {
        return;
    };
    let inner = inner.clone();
    let listener = Closure::<dyn FnMut(MessageEvent)>::wrap(Box::new(
        move |event: MessageEvent| {
            let parent = web_sys::window().and_then(|w| w.parent().ok().flatten());
            if !matches!((event.source(), parent), (Some(source), Some(parent)) if js_sys::Object::is(source.as_ref(), parent.as_ref()))
            {
                return;
            }
            let Some(raw) = event.data().as_string() else {
                return;
            };
            let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) else {
                return;
            };
            let Some(request_id) = value.get("requestId").and_then(|v| v.as_str()) else {
                return;
            };
            match value.get("type").and_then(|v| v.as_str()) {
                Some("ids-agent:openpencil-drive-selected") => {
                    let pick = PICK.with(|v| {
                        let mut slot = v.borrow_mut();
                        if slot.as_ref().is_some_and(|p| p.id == request_id) {
                            slot.take()
                        } else {
                            None
                        }
                    });
                    let Some(pick) = pick else {
                        return;
                    };
                    let Ok(mut shell) = inner.try_borrow_mut() else {
                        return;
                    };
                    let chat = &mut shell.host_mut().editor_state_mut().chat;
                    if chat.thread_id != pick.thread || chat.input.text() != pick.draft {
                        return;
                    }
                    DISMISSED.with(|v| *v.borrow_mut() = pick.draft);
                    if let Some(token) = value
                        .get("token")
                        .and_then(|v| v.as_str())
                        .filter(|t| t.len() < 1024 && t.starts_with("@["))
                    {
                        chat.input
                            .replace_range(pick.start, pick.end, &format!("{token} "), 0);
                    }
                    chat.focused = true;
                    shell.host_mut().mark_editor_state_dirty();
                    let _ = shell.repaint();
                }
                Some("ids-agent:openpencil-drive-context") => {
                    let pending = RESOLVE.with(|v| v.borrow_mut().remove(request_id));
                    if let Some((mut prepared, callback)) = pending {
                        let result =
                            if let Some(error) = value.get("error").and_then(|v| v.as_str()) {
                                Err(error.to_string())
                            } else {
                                apply_context(&mut prepared, &value["context"]).map(|_| prepared)
                            };
                        callback(result);
                    }
                }
                _ => {}
            }
        },
    ));
    let _ = window.add_event_listener_with_callback("message", listener.as_ref().unchecked_ref());
    listener.forget();
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mentions_are_utf8_safe_and_ignore_email_and_tokens() {
        assert_eq!(
            mention("อ่าน @brief", "อ่าน @brief".len()),
            Some(("อ่าน ".len(), "อ่าน @brief".len(), "brief"))
        );
        assert!(mention("a@b", 3).is_none());
        assert!(mention("@[file]", 7).is_none());
        assert!(mention("ไทย", 1).is_none());
    }
    #[test]
    fn resolved_files_expand_only_provider_request_and_preserve_original_text() {
        let mut turn = PreparedTurn {
            endpoint: "/api/ai/standard",
            body_json:
                serde_json::json!({"user": "Read @file", "attachments": [{"name": "local.png"}]})
                    .to_string(),
            client_run_id: "run".into(),
            user_text: "Read @file".into(),
            model: "fixture".into(),
            retry_attachments: vec![],
        };
        apply_context(&mut turn, &serde_json::json!({"prompt": "Read @file\nSOURCE_MARKER", "attachments": [{"name": "reference.pdf", "data_base64": "JVBERg=="}]})).unwrap();
        let body: serde_json::Value = serde_json::from_str(&turn.body_json).unwrap();
        assert_eq!(turn.user_text, "Read @file");
        assert!(body["user"].as_str().unwrap().contains("SOURCE_MARKER"));
        assert_eq!(body["attachments"].as_array().unwrap().len(), 2);
        assert!(apply_context(&mut turn, &serde_json::json!({})).is_err());
    }
}
