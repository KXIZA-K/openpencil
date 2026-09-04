//! Managed Prototype Studio turn admission and durable transcript helpers.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use op_editor_core::chat::ChatState;

use crate::repaint_ctx::RepaintContext;
use crate::web_ai_transport::AiEvent;
use crate::web_chat::PreparedTurn;

thread_local! {
    static NEXT_CLIENT_RUN_ID: Cell<u64> = const { Cell::new(1) };
}

pub(crate) fn launch<C: RepaintContext + 'static>(
    inner: &Rc<RefCell<C>>,
    prepared: PreparedTurn,
    tab: Option<usize>,
) {
    crate::web_chat::abort_active_turn(true);
    let inner_for_admission = inner.clone();
    let prepared_for_admission = prepared.clone();
    let thread_id = inner.try_borrow().ok().and_then(|shell| {
        let chat = &shell.host().editor_state().chat;
        chat.tabs()
            .get(tab.unwrap_or_else(|| chat.active_index()))
            .and_then(|session| session.thread_id.clone())
    });
    let managed = crate::platform_chat_bridge::request_turn(
        &prepared.client_run_id,
        thread_id.as_deref(),
        &prepared.user_text,
        &prepared.model,
        Box::new(move |outcome| match outcome {
            Ok(()) => crate::web_chat::launch_turn_attempt(
                &inner_for_admission,
                prepared_for_admission,
                tab,
                0,
            ),
            Err(error) => show_admission_error(&inner_for_admission, tab, error),
        }),
    );
    if !managed {
        crate::web_chat::launch_turn_attempt(inner, prepared, tab, 0);
    }
}

fn show_admission_error<C: RepaintContext + 'static>(
    inner: &Rc<RefCell<C>>,
    tab: Option<usize>,
    error: String,
) {
    let Ok(mut shell) = inner.try_borrow_mut() else {
        return;
    };
    let target = shell.host_mut().editor_state_mut().chat.run_tab_mut(tab);
    let _ = crate::web_chat::apply_event_to_chat(target, &AiEvent::Error(error));
    shell.host_mut().mark_editor_state_dirty();
    let _ = shell.repaint();
}

pub(crate) fn stamp_messages(chat: &mut ChatState) -> String {
    let sequence = NEXT_CLIENT_RUN_ID.with(|next| {
        let current = next.get();
        next.set(current + 1);
        current
    });
    // The local counter only disambiguates turns inside one iframe. Include
    // browser entropy so two designers submitting in the same millisecond do
    // not collide on the room-wide idempotency key.
    let entropy = (js_sys::Math::random() * u32::MAX as f64) as u32;
    let id = format!(
        "op-{:x}-{entropy:08x}-{sequence}",
        js_sys::Date::now() as u64
    );
    let count = chat.messages.len();
    if count >= 2 {
        chat.messages[count - 2].external_id = Some(format!("{id}:user"));
        chat.messages[count - 1].external_id = Some(format!("{id}:assistant"));
    }
    id
}

pub(crate) fn terminal_payload(
    chat: &ChatState,
    event: &AiEvent,
    client_run_id: &str,
) -> (String, Option<String>) {
    let error = match event {
        AiEvent::Error(error) => Some(error.clone()),
        _ => None,
    };
    let assistant_id = format!("{client_run_id}:assistant");
    let content = chat
        .messages
        .iter()
        .rev()
        .find(|message| message.external_id.as_deref() == Some(assistant_id.as_str()))
        .map(|message| message.content.clone())
        .unwrap_or_default();
    (content, error)
}
