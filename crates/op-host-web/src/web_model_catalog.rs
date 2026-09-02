//! Built-in-only model catalog for the browser host.
//!
//! The daemon may report provider identity for an operator-owned built-in,
//! but host CLI and ACP entries are never made selectable on the web surface.

use std::cell::RefCell;
use std::rc::Rc;

use op_editor_core::{AgentProvider, EditorState, ModelEntry};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;

use crate::repaint_ctx::RepaintContext;

const DAEMON_BUILTIN_PREFIX: &str = "daemon-builtin:";

/// Fetch the daemon catalog once and populate the browser model picker.
pub(crate) fn fetch_models<C: RepaintContext + 'static>(inner: &Rc<RefCell<C>>) {
    let url = crate::daemon_base::daemon_url("/api/ai/models");
    let Ok(xhr) = web_sys::XmlHttpRequest::new() else {
        return;
    };
    if xhr.open_with_async("GET", &url, true).is_err() {
        return;
    }
    crate::live_sync::attach_daemon_headers(&xhr, &url);
    let xhr_cb = xhr.clone();
    let inner_cb = inner.clone();
    let onloadend = Closure::<dyn FnMut()>::once_into_js(move || {
        let Ok(Some(text)) = xhr_cb.response_text() else {
            return;
        };
        let models = parse_models_json(&text);
        if models.is_empty() {
            return;
        }
        let Ok(mut host) = inner_cb.try_borrow_mut() else {
            return;
        };
        apply_model_entries(host.host_mut().editor_state_mut(), &models);
        host.host_mut().mark_editor_state_dirty();
        let _ = host.repaint();
    });
    xhr.set_onloadend(Some(onloadend.unchecked_ref()));
    let _ = xhr.send();
}

/// Parse the catalog while retaining exact identity for structured built-ins.
/// Legacy string entries remain accepted. Structured CLI rows are discarded.
pub(crate) fn parse_models_json(body: &str) -> Vec<ModelEntry> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return Vec::new();
    };
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(parse_model_entry)
        .collect()
}

fn parse_model_entry(value: &serde_json::Value) -> Option<ModelEntry> {
    if let Some(id) = value.as_str().map(str::trim).filter(|id| !id.is_empty()) {
        return Some(legacy_daemon_model(id));
    }
    let object = value.as_object()?;
    let provider = object
        .get("provider")
        .and_then(serde_json::Value::as_str)
        .and_then(AgentProvider::from_wire_id)?;
    let value = object
        .get("value")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    parse_builtin_model_key(value)?;
    let display_name = object
        .get("displayName")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|label| !label.is_empty())
        .unwrap_or(value);
    let provider_name = object
        .get("providerDisplayName")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|label| !label.is_empty())
        .unwrap_or("Server API Key");
    let provider_group = object
        .get("providerGroup")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|group| !group.is_empty())
        .map(normalize_provider_group)
        .filter(|group| !group.is_empty())
        .unwrap_or_else(|| normalize_provider_group(provider_name));
    // The daemon value already carries the exact credential/model identity.
    // Keep the picker identity at provider-section granularity so every
    // managed model painted under the same heading remains one group even in
    // hosts that still compare built-in ids when laying out model rows.
    Some(ModelEntry::builtin_with_display_name(
        provider,
        format!("{DAEMON_BUILTIN_PREFIX}group:{provider_group}"),
        provider_name,
        value,
        display_name,
    ))
}

fn normalize_provider_group(value: &str) -> String {
    value
        .trim()
        .chars()
        .filter_map(|ch| {
            if ch.is_ascii_alphanumeric() {
                Some(ch.to_ascii_lowercase())
            } else if ch.is_whitespace() || matches!(ch, '-' | '_' | '·') {
                Some('-')
            } else {
                None
            }
        })
        .fold(String::new(), |mut out, ch| {
            if ch != '-' || !out.ends_with('-') {
                out.push(ch);
            }
            out
        })
        .trim_matches('-')
        .to_owned()
}

fn parse_builtin_model_key(value: &str) -> Option<(&str, &str)> {
    let mut parts = value.trim().splitn(3, ':');
    if parts.next()? != "builtin" {
        return None;
    }
    let id = parts.next()?.trim();
    let model = parts.next()?.trim();
    (!id.is_empty() && !model.is_empty()).then_some((id, model))
}

/// Heuristic provider tag for a legacy daemon built-in model id.
pub(crate) fn provider_for_model_id(id: &str) -> AgentProvider {
    let lower = id.to_ascii_lowercase();
    if lower.contains("antigravity") {
        AgentProvider::Antigravity
    } else if lower.contains("grok") {
        AgentProvider::GrokBuild
    } else if lower.contains("gpt") || lower.contains("codex") || lower.contains("gemini") {
        // Legacy bare model ids have no transport identity. Gemini API
        // built-ins use the generic OpenAI-compatible tag; they must not be
        // mislabeled as the retired Gemini CLI or as Antigravity.
        AgentProvider::CodexCli
    } else if lower.contains("copilot") {
        AgentProvider::GithubCopilot
    } else {
        AgentProvider::ClaudeCode
    }
}

/// Merge legacy model ids into the same built-in-only catalog.
#[cfg(test)]
pub(crate) fn apply_models(state: &mut EditorState, ids: &[String]) {
    let entries = ids
        .iter()
        .map(|id| legacy_daemon_model(id))
        .collect::<Vec<_>>();
    apply_model_entries(state, &entries);
}

fn legacy_daemon_model(id: &str) -> ModelEntry {
    ModelEntry::builtin_with_display_name(
        provider_for_model_id(id),
        format!("{DAEMON_BUILTIN_PREFIX}{id}"),
        "Server API Key",
        id,
        id,
    )
}

fn apply_model_entries(state: &mut EditorState, entries: &[ModelEntry]) {
    state.chat.discovered_models = entries.to_vec();
    reconcile_models(state);
}

/// Tell an embedding product shell which managed model the user selected.
///
/// OpenPencil itself intentionally has no knowledge of the room database. The
/// parent shell owns that durable shared preference, so the browser host emits
/// a narrow postMessage after an explicit picker selection. Standalone opens
/// harmlessly post to themselves; consumers must validate the source window
/// and origin before accepting this message.
#[cfg(target_arch = "wasm32")]
pub(crate) fn post_selected_model_to_parent(state: &EditorState) {
    let Some(model) = state.chat.selected_model_entry() else {
        return;
    };
    let Some(window) = web_sys::window() else {
        return;
    };
    let Ok(Some(parent)) = window.parent() else {
        return;
    };
    let message = serde_json::json!({
        "type": "ids-agent:openpencil-model-selected",
        "value": model.value,
        "displayName": model.display_name,
    })
    .to_string();
    let _ = parent.post_message(&wasm_bindgen::JsValue::from_str(&message), "*");
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn post_selected_model_to_parent(_state: &EditorState) {}

/// Reconcile daemon and browser-local built-ins while hiding every CLI/ACP.
pub(crate) fn reconcile_models(state: &mut EditorState) {
    let previous = state
        .chat
        .available_models
        .get(state.chat.selected_model)
        .cloned();
    let local_agents = state
        .editor_ui
        .agent_settings
        .builtin_agents
        .iter()
        .filter(|agent| agent.ready())
        .collect::<Vec<_>>();
    let local_model_ids = local_agents
        .iter()
        .map(|agent| agent.model.trim())
        .collect::<std::collections::HashSet<_>>();
    let mut available_models = state
        .chat
        .discovered_models
        .iter()
        .filter(|entry| {
            entry
                .builtin_provider_id
                .as_deref()
                .is_some_and(|id| id.starts_with(DAEMON_BUILTIN_PREFIX))
                && !local_model_ids.contains(
                    parse_builtin_model_key(&entry.value)
                        .map(|(_, model)| model)
                        .unwrap_or_else(|| entry.value.trim()),
                )
        })
        .cloned()
        .collect::<Vec<_>>();
    available_models.extend(local_agents.into_iter().map(|agent| {
        ModelEntry::builtin_with_display_name(
            agent.kind.model_provider(),
            agent.id.clone(),
            agent.display_name.clone(),
            format!("builtin:{}:{}", agent.id, agent.model),
            agent.model.clone(),
        )
    }));
    if state.chat.available_models == available_models {
        return;
    }
    state.chat.available_models = available_models;
    state.chat.selected_model = previous
        .and_then(|prior| {
            state.chat.available_models.iter().position(|model| {
                model.provider == prior.provider
                    && model.value == prior.value
                    && model.builtin_provider_id == prior.builtin_provider_id
            })
        })
        .unwrap_or(0);
}
