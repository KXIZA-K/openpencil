//! Mobile design tool surface: the shared desktop toolset
//! (`op_chat_agent::design_agent_tools`) minus the host arms a phone does
//! not have, plus the transcript tool-card result attachment.

use op_ai::chat_provider::{ChatToolDef, ChatToolResult};
use op_editor_core::ChatState;

/// Desktop design tools whose host arms are unavailable on mobile:
/// `get_screenshot` / `export_nodes` need the render/export registrar the
/// daemon installs, and `spawn_agents` needs the desktop sub-agent session
/// host. Everything else — `batch_design` (script mode included), reads,
/// variables, `ToolSearch` — is the shared implementation.
const MOBILE_UNAVAILABLE_TOOLS: &[&str] = &["get_screenshot", "export_nodes", "spawn_agents"];

/// Tool definitions the mobile design loop advertises — the shared defs
/// filtered to what this host can execute.
pub(crate) fn mobile_design_tool_defs() -> Vec<ChatToolDef> {
    op_chat_agent::design_agent_tools::design_tool_defs()
        .into_iter()
        .filter(|def| !MOBILE_UNAVAILABLE_TOOLS.contains(&def.name.as_str()))
        .collect()
}

/// Record an executed tool call's result on its transcript card — ported
/// from `op-host-desktop::chat_session::attach_tool_result_to_transcript`
/// (single-agent form: mobile runs no sub-agents).
pub(crate) fn attach_tool_result_to_transcript(
    chat: &mut ChatState,
    name: &str,
    result: &ChatToolResult,
) -> bool {
    let Some(index) = chat
        .messages
        .iter()
        .rposition(|message| message.role == op_editor_core::ChatRole::Assistant)
    else {
        return false;
    };
    let msg = &mut chat.messages[index];
    for call in msg.tool_calls.iter_mut().rev() {
        if call.name != name {
            continue;
        }
        let Ok(mut envelope) = serde_json::from_str::<serde_json::Value>(&call.args) else {
            continue;
        };
        let Some(obj) = envelope.as_object_mut() else {
            continue;
        };
        if obj.get("status").and_then(serde_json::Value::as_str) != Some("running") {
            continue;
        }
        let result_value = serde_json::from_str::<serde_json::Value>(&result.content)
            .unwrap_or_else(|_| serde_json::Value::String(result.content.clone()));
        obj.insert("result".into(), result_value);
        let status = if result.is_error { "error" } else { "done" };
        obj.insert(
            "status".into(),
            serde_json::Value::String(status.to_string()),
        );
        call.args = envelope.to_string();
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mobile_defs_are_the_shared_set_minus_host_arms() {
        let defs = mobile_design_tool_defs();
        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        for unavailable in MOBILE_UNAVAILABLE_TOOLS {
            assert!(
                !names.contains(unavailable),
                "{unavailable} must be filtered"
            );
        }
        for required in [
            "get_editor_state",
            "get_guidelines",
            "get_style_guide",
            "batch_design",
            "snapshot_layout",
            "ToolSearch",
        ] {
            assert!(names.contains(&required), "{required} must be advertised");
        }
        // Script mode is live on mobile (rquickjs builds via bindgen), so the
        // advertised batch_design schema must carry the script arm — byte-
        // equal to the MCP catalog the desktop advertises.
        let batch = defs.iter().find(|d| d.name == "batch_design").unwrap();
        assert!(batch.input_schema_json.contains("\"script\""));
    }
}
