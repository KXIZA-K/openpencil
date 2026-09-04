//! Durable team-chat bridge for the IDS Prototype Studio embed.
//!
//! OpenPencil remains standalone by default. A managed Studio URL carries an
//! embed ticket; only there do we ask the authenticated parent shell to admit
//! canvas-writing turns and mirror the visible transcript into team storage.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use op_editor_core::chat::ChatMessage;
use serde::Deserialize;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::MessageEvent;

use crate::repaint_ctx::RepaintContext;

type Admission = Box<dyn FnOnce(Result<(), String>)>;

thread_local! {
    static PENDING: RefCell<HashMap<String, Admission>> = RefCell::new(HashMap::new());
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DurableMessage {
    id: String,
    role: String,
    content: String,
    #[serde(default)]
    agent_name: Option<String>,
    #[serde(default)]
    status: String,
}

#[derive(Deserialize)]
struct DurableThread {
    id: String,
    title: String,
    #[serde(default)]
    messages: Vec<DurableMessage>,
}

fn studio_managed() -> bool {
    web_sys::window()
        .and_then(|window| window.location().search().ok())
        .is_some_and(|query| managed_query(&query))
}

fn managed_query(query: &str) -> bool {
    query.split('&').any(|part| {
        matches!(
            part.trim_start_matches('?'),
            "idsStudio=1" | "idsStudio=true"
        ) || part.trim_start_matches('?').starts_with("ticket=")
    })
}

fn post(payload: serde_json::Value) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Ok(Some(parent)) = window.parent() else {
        return;
    };
    let _ = parent.post_message(&JsValue::from_str(&payload.to_string()), "*");
}

pub(crate) fn install<C: RepaintContext + 'static>(inner: &Rc<RefCell<C>>) {
    if !studio_managed() {
        return;
    }
    let Some(window) = web_sys::window() else {
        return;
    };
    if let Ok(mut shell) = inner.try_borrow_mut() {
        shell
            .host_mut()
            .editor_state_mut()
            .chat
            .enable_managed_thread_mode("Team Chat");
        shell.host_mut().mark_editor_state_dirty();
    }
    let inner = inner.clone();
    let listener =
        Closure::<dyn FnMut(MessageEvent)>::wrap(Box::new(move |event: MessageEvent| {
            let parent = web_sys::window().and_then(|window| window.parent().ok().flatten());
            let source_ok = match (event.source(), parent) {
                (Some(source), Some(parent)) => {
                    js_sys::Object::is(source.as_ref(), parent.as_ref())
                }
                _ => false,
            };
            if !source_ok {
                return;
            }
            let Some(raw) = event.data().as_string() else {
                return;
            };
            let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) else {
                return;
            };
            match value.get("type").and_then(serde_json::Value::as_str) {
                Some("ids-agent:openpencil-chat-run-admission") => {
                    let Some(id) = value.get("clientRunId").and_then(serde_json::Value::as_str)
                    else {
                        return;
                    };
                    if value.get("admitted").and_then(serde_json::Value::as_bool) == Some(true) {
                        if let Some(callback) =
                            PENDING.with(|pending| pending.borrow_mut().remove(id))
                        {
                            callback(Ok(()));
                        }
                    } else if let Some(error) =
                        value.get("error").and_then(serde_json::Value::as_str)
                    {
                        if let Some(callback) =
                            PENDING.with(|pending| pending.borrow_mut().remove(id))
                        {
                            callback(Err(error.to_string()));
                        }
                    }
                }
                Some("ids-agent:openpencil-chat-hydrate") => {
                    let Some(threads) = value.get("threads") else {
                        return;
                    };
                    let Ok(threads) = serde_json::from_value::<Vec<DurableThread>>(threads.clone())
                    else {
                        return;
                    };
                    let Ok(mut shell) = inner.try_borrow_mut() else {
                        return;
                    };
                    let hydrated = threads
                        .into_iter()
                        .map(|thread| {
                            let messages = thread
                                .messages
                                .into_iter()
                                .filter_map(|durable| {
                                    if durable.role == "system" || durable.content.trim().is_empty()
                                    {
                                        return None;
                                    }
                                    let is_assistant = durable.role == "assistant";
                                    let mut message = if is_assistant {
                                        ChatMessage::assistant(durable.content)
                                    } else {
                                        ChatMessage::user(durable.content)
                                    };
                                    message.external_id = Some(durable.id);
                                    message.agent_name = durable.agent_name;
                                    message.streaming = is_assistant
                                        && !matches!(
                                            durable.status.as_str(),
                                            "complete" | "failed" | "interrupted"
                                        );
                                    Some(message)
                                })
                                .collect();
                            (thread.id, thread.title, messages)
                        })
                        .collect();
                    shell
                        .host_mut()
                        .editor_state_mut()
                        .chat
                        .hydrate_managed_threads(hydrated);
                    shell.host_mut().mark_editor_state_dirty();
                    let _ = shell.repaint();
                }
                Some("ids-agent:openpencil-chat-thread-created") => {
                    let Some(thread) = value.get("thread") else {
                        return;
                    };
                    let Some(id) = thread.get("id").and_then(serde_json::Value::as_str) else {
                        return;
                    };
                    let title = thread
                        .get("title")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("New Chat");
                    let Ok(mut shell) = inner.try_borrow_mut() else {
                        return;
                    };
                    let chat = shell.host_mut().editor_state_mut().chat.active_mut();
                    if chat.thread_id.is_none() {
                        chat.thread_id = Some(id.to_string());
                        chat.title = title.to_string();
                    }
                    shell.host_mut().mark_editor_state_dirty();
                    let _ = shell.repaint();
                }
                _ => {}
            }
        }));
    let _ = window.add_event_listener_with_callback("message", listener.as_ref().unchecked_ref());
    listener.forget();
    post(serde_json::json!({ "type": "ids-agent:openpencil-chat-ready" }));
}

pub(crate) fn request_turn(
    client_run_id: &str,
    thread_id: Option<&str>,
    prompt: &str,
    model: &str,
    on_admitted: Admission,
) -> bool {
    if !studio_managed() {
        return false;
    }
    PENDING.with(|pending| {
        pending
            .borrow_mut()
            .insert(client_run_id.to_string(), on_admitted);
    });
    post(serde_json::json!({
        "type": "ids-agent:openpencil-chat-turn-request",
        "clientRunId": client_run_id,
        "threadId": thread_id,
        "prompt": prompt,
        "model": model,
    }));
    true
}

pub(crate) fn create_thread() -> Option<String> {
    if !studio_managed() {
        return None;
    }
    let entropy = (js_sys::Math::random() * u32::MAX as f64) as u32;
    let id = format!("pthr_op_{:x}_{entropy:08x}", js_sys::Date::now() as u64);
    post(serde_json::json!({
        "type": "ids-agent:openpencil-chat-thread-create",
        "threadId": id,
    }));
    Some(id)
}

pub(crate) fn finish_turn(client_run_id: &str, assistant_content: &str, error: Option<&str>) {
    if !studio_managed() {
        return;
    }
    post(serde_json::json!({
        "type": "ids-agent:openpencil-chat-turn-finished",
        "clientRunId": client_run_id,
        "status": if error.is_some() { "failed" } else { "succeeded" },
        "assistantContent": assistant_content,
        "error": error,
    }));
}

pub(crate) fn interrupt_turn(client_run_id: &str) {
    if !studio_managed() {
        return;
    }
    post(serde_json::json!({
        "type": "ids-agent:openpencil-chat-turn-finished",
        "clientRunId": client_run_id,
        "status": "interrupted",
        "error": "Stopped before completion.",
    }));
}

pub(crate) fn cancel_pending_turns() {
    if !studio_managed() {
        return;
    }
    let ids = PENDING.with(|pending| {
        pending
            .borrow_mut()
            .drain()
            .map(|(id, _)| id)
            .collect::<Vec<_>>()
    });
    for id in ids {
        post(serde_json::json!({
            "type": "ids-agent:openpencil-chat-turn-finished",
            "clientRunId": id,
            "status": "canceled",
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durable_message_accepts_platform_camel_case_shape() {
        let parsed: DurableMessage = serde_json::from_str(
            r#"{"id":"run:user","role":"user","content":"hello","agentName":null,"status":"complete"}"#,
        )
        .expect("message parses");
        assert_eq!(parsed.id, "run:user");
        assert_eq!(parsed.role, "user");
    }

    #[test]
    fn durable_threads_keep_each_transcript_separate() {
        let parsed: Vec<DurableThread> = serde_json::from_str(
            r#"[{"id":"pthr_general","title":"General","messages":[{"id":"run-a:user","role":"user","content":"hello","status":"complete"}]},{"id":"pthr_mobile","title":"Mobile","messages":[]}]"#,
        )
        .expect("threads parse");
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].messages.len(), 1);
        assert_eq!(parsed[1].id, "pthr_mobile");
    }

    #[test]
    fn managed_marker_survives_ticket_exchange_redirect() {
        assert!(managed_query("?idsStudio=1"));
        assert!(managed_query("?ticket=short-lived"));
        assert!(!managed_query("?locale=en-US"));
    }
}
