//! Mobile-editor chat pump — drains `chat.pending_send` and runs the turn.
//!
//! Mobile shells share the desktop chat UI (`begin_send` pushes the user
//! message plus a streaming assistant bubble and raises `pending_send`),
//! but until this pump existed NOTHING on iOS / Android drained that flag:
//! the assistant bubble sat on "Thinking…" forever. This is the mobile
//! counterpart of `op-host-desktop`'s `chat_session::{launch_if_pending,
//! pump}`, scoped to what a phone can actually run:
//!
//! - Built-in (API-key) provider selections stream a plain chat turn over
//!   HTTPS (`editor_chat_turn.rs`) — no canvas-tool agent loop, no design
//!   orchestrator.
//! - Every other selection (external CLI agents, ACP servers — none of
//!   which exist on a phone) writes an honest error into the assistant
//!   bubble instead of hanging.
//! - Provider failures always land as `error: …` transcript text: the
//!   worker terminates every turn with a `Done` delta and the pump clears
//!   the bubble's streaming flag.
//!
//! Worker tasks run on a dedicated Tokio runtime and only own a value
//! snapshot plus an `mpsc::Sender`; deltas land on the engine thread during
//! `op_frame`, mirroring `editor_model_discovery`.

use std::sync::mpsc;
use std::sync::OnceLock;

use op_ai::chat_history::{trim_chat_history, DEFAULT_MAX_CHARS, DEFAULT_MAX_MESSAGES};
use op_ai::chat_provider::{ChatDelta, EffortLevel};
use op_editor_core::{ChatRole, EditorState};
use op_editor_host_core::chat::{apply_poll_to_message, chat_history_from_transcript, ChatSession};
use op_host_native::WidgetHostNative;
use tokio::runtime::{Builder, Runtime};
use tokio::task::AbortHandle;

use crate::editor_chat_turn::{run_builtin_turn, BuiltinChatTurn};
use crate::lifecycle::Session;

/// Engine-thread repoll cadence while a turn streams (~30 fps), matching
/// the desktop winit loop's wake rate during a chat turn.
const STREAM_POLL_INTERVAL_MS: u64 = 33;

/// Desktop parity: plain chat turns cap the reply budget at 4096 tokens.
const CHAT_MAX_OUTPUT_TOKENS: u32 = 4096;

struct ChatTurnJob {
    session: ChatSession,
    /// Tab this turn is bound to. Deltas keep landing there even after the
    /// user switches tabs mid-stream (desktop `running_tab` parity);
    /// `run_tab_mut` falls back to the active tab if the index went stale.
    running_tab: usize,
    abort: Option<AbortHandle>,
}

/// Runtime-only chat jobs stay out of serializable editor state. Dropping
/// the engine aborts the in-flight request so a torn-down editor does not
/// keep a credential-bearing stream open until the network timeout.
#[derive(Default)]
pub(crate) struct MobileChatHost {
    turn: Option<ChatTurnJob>,
}

impl Drop for MobileChatHost {
    fn drop(&mut self) {
        self.drop_turn();
    }
}

impl MobileChatHost {
    fn drop_turn(&mut self) {
        if let Some(job) = self.turn.take() {
            if let Some(abort) = job.abort {
                abort.abort();
            }
        }
    }

    /// Drain widget-raised chat flags, launch a newly requested turn, and
    /// fold streamed deltas into the transcript. Returns the next
    /// engine-thread poll deadline while a turn is in flight.
    pub(crate) fn pump(&mut self, host: &mut WidgetHostNative, now_ms: u64) -> Option<u64> {
        let mut changed = self.drain_new_chat_and_stop(host);
        changed |= self.launch_if_pending(host);
        changed |= self.poll_into(host);
        if changed {
            host.mark_editor_state_dirty();
        }
        self.turn
            .as_ref()
            .map(|_| now_ms.saturating_add(STREAM_POLL_INTERVAL_MS))
    }

    /// New Chat / Stop only need the worker dropped host-side — the widget
    /// layer already opened the fresh tab / cleared the streaming flags.
    fn drain_new_chat_and_stop(&mut self, host: &mut WidgetHostNative) -> bool {
        let chat = &mut host.editor_state_mut().chat;
        let new_chat = std::mem::take(&mut chat.pending_new_chat);
        let stop = std::mem::take(&mut chat.pending_stop_chat);
        if new_chat || stop {
            self.drop_turn();
        }
        new_chat || stop
    }

    /// Drain `chat.pending_send` (raised by `ChatState::begin_send`) and
    /// route it: built-in providers launch a streaming turn, anything else
    /// gets the honest mobile-unavailable error.
    fn launch_if_pending(&mut self, host: &mut WidgetHostNative) -> bool {
        let Some(user_text) = host.editor_state_mut().chat.pending_send.take() else {
            return false;
        };
        // This turn consumes the staged attachments. The mobile transport is
        // text-only today: the images already ride the user bubble via
        // `begin_send`, they just do not reach the provider.
        host.editor_state_mut().chat.pending_attachments.clear();
        let running_tab = host.editor_state().chat.active_index();
        // A send fired mid-turn replaces the in-flight turn (desktop
        // parity) — the old worker drains harmlessly once its receiver
        // drops.
        self.drop_turn();
        match prepare_builtin_turn(host.editor_state(), &user_text) {
            Some(turn) => match start_turn(turn, running_tab) {
                Ok(job) => self.turn = Some(job),
                Err(message) => write_turn_error(host, running_tab, message),
            },
            None => {
                let label = selected_provider_label(host.editor_state());
                write_turn_error(
                    host,
                    running_tab,
                    format!(
                        "error: {label} chat is not available on this device — \
                         external CLI and ACP agents require the desktop app. \
                         Configure an API-key provider in Settings → Agents \
                         and pick one of its models via the model chip."
                    ),
                );
            }
        }
        true
    }

    /// Fold everything the worker streamed since the last frame into the
    /// bound tab's trailing assistant bubble. Errors land as `error: …`
    /// content and `finished` clears the bubble's streaming flag — the two
    /// halves of "never stuck at Thinking…".
    fn poll_into(&mut self, host: &mut WidgetHostNative) -> bool {
        let Some(job) = self.turn.as_mut() else {
            return false;
        };
        let poll = job.session.poll();
        let mut changed = false;
        if !poll.is_idle() {
            let messages = &mut host
                .editor_state_mut()
                .chat
                .run_tab_mut(Some(job.running_tab))
                .messages;
            if let Some(index) = messages
                .iter()
                .rposition(|message| message.role == ChatRole::Assistant)
            {
                apply_poll_to_message(&mut messages[index], &poll);
                changed = true;
            }
        }
        if poll.finished {
            self.drop_turn();
        }
        changed
    }
}

impl Session {
    pub(crate) fn pump_editor_chat(&mut self, now_ms: u64) -> Option<u64> {
        let Session { editor, chat, .. } = self;
        editor.as_mut().and_then(|host| chat.pump(host, now_ms))
    }
}

/// Spawn the worker task for one prepared turn and park its session.
fn start_turn(turn: BuiltinChatTurn, running_tab: usize) -> Result<ChatTurnJob, String> {
    let runtime = chat_runtime().map_err(|error| format!("error: {error}"))?;
    let (tx, rx) = mpsc::channel::<ChatDelta>();
    let task = runtime.spawn(run_builtin_turn(turn, tx));
    Ok(ChatTurnJob {
        session: ChatSession::from_channels(rx, None),
        running_tab,
        abort: Some(task.abort_handle()),
    })
}

/// Resolve the selected chat model into a runnable built-in turn snapshot.
/// `None` when the selection is not a ready built-in (API-key) entry.
fn prepare_builtin_turn(state: &EditorState, user_text: &str) -> Option<BuiltinChatTurn> {
    let entry = state.chat.selected_model_entry()?;
    let id = entry.builtin_provider_id.as_deref()?;
    let selected_model = entry.builtin_model_id()?;
    let config = state
        .editor_ui
        .agent_settings
        .builtin_agents
        .iter()
        .find(|agent| agent.id == id && agent.ready())?;
    if !config.has_model(selected_model) {
        return None;
    }
    let base_url = if config.base_url.trim().is_empty() {
        config.kind.default_base_url().to_string()
    } else {
        config.base_url.trim().to_string()
    };
    // Per-turn knobs the chat panel carries. The BuiltIn wire has no
    // separate effort channel, so a non-default effort rides in-band as a
    // leading directive (desktop parity).
    let mut prompt = user_text.to_string();
    let effort = state.chat.effort_level;
    if effort != EffortLevel::Low {
        prompt = format!("Apply {} reasoning effort.\n\n{prompt}", effort.as_str());
    }
    // Prior transcript turns, trimmed by the shared sliding-window policy.
    // `begin_send` already pushed this turn's user message + empty
    // assistant bubble; the transcript mapper excludes both.
    let history = trim_chat_history(
        &chat_history_from_transcript(&state.chat.messages),
        DEFAULT_MAX_MESSAGES,
        DEFAULT_MAX_CHARS,
    );
    Some(BuiltinChatTurn {
        kind: config.kind,
        api_key: config.api_key.trim().to_string(),
        model: selected_model.to_string(),
        base_url,
        // No mobile system prompt yet: the desktop's context-rich prompt
        // builder lives in op-host-services, which mobile deliberately
        // does not link. A plain turn works without one.
        system_prompt: String::new(),
        history,
        prompt,
        max_output_tokens: CHAT_MAX_OUTPUT_TOKENS,
    })
}

/// Display name for the honest-error path, mirroring the desktop's
/// `selected_provider_label`.
fn selected_provider_label(state: &EditorState) -> String {
    if let Some(entry) = state.chat.selected_model_entry() {
        if let Some(id) = entry.builtin_provider_id.as_deref() {
            if let Some(agent) = state
                .editor_ui
                .agent_settings
                .builtin_agents
                .iter()
                .find(|agent| agent.id == id)
            {
                return agent.display_name.clone();
            }
        }
        if let Some(id) = entry.acp_agent_id() {
            if let Some(agent) = state
                .editor_ui
                .agent_settings
                .acp_agents
                .iter()
                .find(|agent| agent.id == id)
            {
                return agent.display_name.clone();
            }
        }
    }
    op_editor_core::AgentProvider::ALL
        .get(state.editor_ui.chat_selected_agent)
        .map(|agent| agent.name().to_string())
        .unwrap_or_else(|| "This agent".into())
}

/// Land an aborted turn's message in the bound tab's assistant bubble and
/// stop its streaming animation (desktop honest-error parity).
fn write_turn_error(host: &mut WidgetHostNative, running_tab: usize, message: String) {
    let chat = host.editor_state_mut().chat.run_tab_mut(Some(running_tab));
    if let Some(msg) = chat
        .messages
        .iter_mut()
        .rev()
        .find(|message| message.role == ChatRole::Assistant)
    {
        msg.content = message;
        msg.streaming = false;
    }
}

/// Dedicated single-worker runtime for chat turns. Kept separate from the
/// model-discovery runtime so a long streaming turn and a catalog refresh
/// never queue behind each other's blocking phases.
fn chat_runtime() -> Result<&'static Runtime, &'static str> {
    static RUNTIME: OnceLock<Result<Runtime, ()>> = OnceLock::new();
    RUNTIME
        .get_or_init(|| {
            Builder::new_multi_thread()
                .worker_threads(1)
                .enable_all()
                .thread_name("op-mobile-chat")
                .build()
                .map_err(|_| ())
        })
        .as_ref()
        .map_err(|_| "chat runtime is unavailable")
}

#[cfg(test)]
#[path = "editor_chat_tests.rs"]
mod tests;
