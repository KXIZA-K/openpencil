//! Validate interaction syntax before a model edit reaches the document.
//! Reuse the runtime parser; accepting a serde map is not proof it can execute.
use jian_core::action::{default_registry, ActionError};
use serde_json::Value;

pub(crate) const GUIDANCE: &str = r#"
Native interactions use events.onTap as an array of SINGLE-KEY actions, for example:
{"events":{"onTap":[{"set":{"$app.count":"($app.count ?? 0) + 1"}}]}}
Text can use {"bindings":{"content":"'Count: ' + ($app.count ?? 0)"}}.
For navigation use {"events":{"onTap":[{"push":"'/details'"}]}} or [{"pop":null}].
Action values are expressions: a route string must include expression quotes as shown.
Declare destination routes with screen on TOP-LEVEL frames. Do not nest a screen inside another screen.
State declarations use state (singular), not states/initialState. Do not emit type:set_state, mapping,
demoOnly or noNetwork properties: they are not runtime interaction syntax. Keep mock flows local.
Use $app for state shared by a button and its child label; $state is not inherited from ancestor frames.
"#;

pub(crate) fn validate_node(node: &Value) -> Result<(), ActionError> {
    for key in ["states", "initialState", "demoOnly", "noNetwork"] {
        if node.get(key).is_some() {
            return Err(ActionError::Custom(format!("Unsupported interaction property '{key}'")));
        }
    }
    if let Some(events) = node.get("events").filter(|v| !v.is_null()) {
        let events = events.as_object().ok_or_else(|| ActionError::Custom("events must be an object".into()))?;
        let registry = default_registry();
        for (hook, actions) in events {
            // Ask the canonical schema whether it recognizes the hook, including
            // null removals. Serde otherwise silently ignores misspelled hooks.
            let probe = serde_json::json!({hook: []});
            let parsed: jian_ops_schema::events::EventHandlers = serde_json::from_value(probe)
                .map_err(|_| ActionError::Custom("Invalid event hook".into()))?;
            let known = serde_json::to_value(&parsed).map_err(|_| ActionError::Custom("Invalid event hook".into()))?;
            if parsed.extra.contains_key(hook) || known.get(hook).is_none() {
                return Err(ActionError::Custom(format!("Unknown event hook '{hook}'")));
            }
            if !actions.is_null() { registry.borrow().parse_list(actions)?; }
        }
    }
    if let Some(bindings) = node.get("bindings").filter(|v| !v.is_null()) {
        let bindings = bindings.as_object().ok_or_else(|| ActionError::Custom("bindings must be an object".into()))?;
        for (property, expression) in bindings {
            let source = expression.as_str().ok_or_else(|| ActionError::Custom(format!("Binding '{property}' must be an expression string")))?;
            jian_core::expression::Expression::compile(source)
                .map_err(|_| ActionError::Custom(format!("Invalid expression in binding '{property}'")))?;
        }
    }
    if let Some(children) = node.get("children").and_then(Value::as_array) {
        for child in children { validate_node(child)?; }
    }
    Ok(())
}

pub(crate) fn validate_modifications(ops: &[crate::chat_canvas_tools::DesignModificationOp]) -> Result<(), ActionError> {
    for (_, node) in ops {
        validate_node(if node.get("op").and_then(Value::as_str) == Some("update") {
            &node["data"]
        } else { node })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn accepts_runtime_actions_bindings_and_explicit_removals() {
        for value in [
            json!({"events":{"onTap":[{"set":{"$app.step":"($app.step ?? 0) + 1"}}]},"bindings":{"content":"'Step: ' + ($app.step ?? 0)"}}),
            json!({"events":{"onTap":[{"push":"'/details'"},{"pop":null}]}}),
            json!({"events":null,"bindings":null}),
            json!({"events":{"onTap":null}}),
        ] { validate_node(&value).unwrap(); }
    }

    #[test]
    fn rejects_silently_ignored_or_unexecutable_interactions() {
        for value in [
            json!({"events":{"onTap":[{"type":"set_state","target":"label","mapping":{"idle":"confirm"}}]}}),
            json!({"events":{"onClick":[{"pop":null}]}}),
            json!({"events":{"onClick":null}}),
            json!({"events":{"onTap":[{"set_state":"confirm"}]}}),
            json!({"bindings":{"content":"1 +"}}),
            json!({"states":{"idle":{}}}),
            json!({"children":[{"events":{"onTap":[{"unknown":null}]}}]}),
        ] { assert!(validate_node(&value).is_err(), "must reject {value}"); }
    }
}
