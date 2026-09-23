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
            Err(error) => show_admission_error(&inner_for_admission, tab, &prepared_for_admission, error),
        }),
    );
    if !managed {
        crate::web_chat::launch_turn_attempt(inner, prepared, tab, 0);
    }
}

fn show_admission_error<C: RepaintContext + 'static>(
    inner: &Rc<RefCell<C>>,
    tab: Option<usize>,
    prepared: &PreparedTurn,
    error: String,
) {
    let Ok(mut shell) = inner.try_borrow_mut() else {
        return;
    };
    let target = shell.host_mut().editor_state_mut().chat.run_tab_mut(tab);
    restore_rejected_draft(target, &prepared.user_text, &prepared.retry_attachments);
    let _ = crate::web_chat::apply_event_to_chat(target, &AiEvent::Error(error));
    shell.host_mut().mark_editor_state_dirty();
    let _ = shell.repaint();
}

// Admission happens before inference. Keep a rejected request editable without
// overwriting a newer draft the designer may have typed while waiting.
fn restore_rejected_draft(
    chat: &mut ChatState,
    prompt: &str,
    attachments: &[op_editor_core::chat::ChatAttachment],
) {
    if chat.input.text().is_empty() && chat.pending_attachments.is_empty() {
        chat.set_input_text(prompt);
        chat.pending_attachments = attachments.to_vec();
    }
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
    // Host-side unit tests cannot invoke JavaScript imports. Only that test
    // target uses fixed clock/entropy inputs; shipped browser builds retain
    // their real browser clock and randomness, including wasm test builds.
    #[cfg(all(test, not(target_arch = "wasm32")))]
    let (entropy, now) = (0x12345678u32, 1_700_000_000_000u64);
    #[cfg(not(all(test, not(target_arch = "wasm32"))))]
    let (entropy, now) = (
        (js_sys::Math::random() * u32::MAX as f64) as u32,
        js_sys::Date::now() as u64,
    );
    let id = format!(
        "op-{:x}-{entropy:08x}-{sequence}",
        now
    );
    let count = chat.messages.len();
    if count >= 2 {
        chat.messages[count - 2].external_id = Some(format!("{id}:user"));
        chat.messages[count - 1].external_id = Some(format!("{id}:assistant"));
    }
    id
}

#[cfg(test)]
mod tests {
    use super::*;
    use op_editor_core::chat::ChatMessage;

    #[test]
    fn rejected_admission_restores_empty_composer_without_overwriting_new_draft() {
        let mut chat = ChatState::default();
        restore_rejected_draft(&mut chat, "retry this request", &[]);
        assert_eq!(chat.input.text(), "retry this request");
        chat.set_input_text("newer unsent draft");
        restore_rejected_draft(&mut chat, "older rejected request", &[]);
        assert_eq!(chat.input.text(), "newer unsent draft");
        assert!(chat.pending_send.is_none());
    }

    #[test]
    fn rejected_admission_restores_attachment_bytes_without_replacing_new_upload() {
        let attachment = op_editor_core::chat::ChatAttachment {
            name: "reference.png".into(), media_type: "image/png".into(), data: vec![1, 2, 3],
        };
        let mut chat = ChatState::default();
        restore_rejected_draft(&mut chat, "use this reference", &[attachment.clone()]);
        assert_eq!(chat.pending_attachments[0].data, vec![1, 2, 3]);
        chat.set_input_text("");
        chat.pending_attachments[0].name = "newer-upload.png".into();
        restore_rejected_draft(&mut chat, "older prompt", &[attachment]);
        assert!(chat.input.text().is_empty());
        assert_eq!(chat.pending_attachments[0].name, "newer-upload.png");
    }

    #[test]
    fn stamping_after_loaded_history_preserves_older_ids_and_separates_turns() {
        let mut chat = ChatState::default();
        let mut old = ChatMessage::assistant("historical reply");
        old.external_id = Some("old:assistant".into());
        chat.messages.push(old);
        chat.set_input_text("new request");
        assert!(chat.begin_send());
        let first = stamp_messages(&mut chat);
        assert_eq!(chat.messages[0].external_id.as_deref(), Some("old:assistant"));
        assert_eq!(chat.messages[1].external_id.as_deref(), Some(format!("{first}:user").as_str()));
        assert_eq!(chat.messages[2].external_id.as_deref(), Some(format!("{first}:assistant").as_str()));
        chat.messages[2].streaming = false;
        chat.set_input_text("second request");
        assert!(chat.begin_send());
        let second = stamp_messages(&mut chat);
        assert_ne!(first, second);
        assert_eq!(chat.messages[2].external_id.as_deref(), Some(format!("{first}:assistant").as_str()));
        assert_eq!(chat.messages[4].external_id.as_deref(), Some(format!("{second}:assistant").as_str()));
    }
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
