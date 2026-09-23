//! Conversion only: no network, model calls, document mutation or second painter.
use serde_json::{json, Value};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn convert_snapshot(snapshot: &str) -> Result<String, JsValue> {
    convert(snapshot).map_err(|error| JsValue::from_str(&error.to_string()))
}

#[derive(Debug)]
enum ConversionError { Serialization(serde_json::Error) }
impl std::fmt::Display for ConversionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self { Self::Serialization(error) => write!(f, "snapshot serialization: {error}") }
    }
}

fn convert(snapshot: &str) -> Result<String, ConversionError> {
    let imported = op_html::import_snapshot(snapshot, &op_html::HtmlImportOptions::default());
    let diagnostics: Vec<Value> = imported.diagnostics.iter()
        .map(|warning| json!({ "code": warning.code(), "message": warning.to_string() })).collect();
    serde_json::to_string(&json!({
        "document": { "version": "1.0", "children": imported.nodes },
        "diagnostics": diagnostics,
        "valid": !imported.nodes.is_empty() && !imported.diagnostics.iter().any(|d| matches!(
            d, op_html::ImportWarning::SnapshotRejected { .. } | op_html::ImportWarning::SnapshotTruncated | op_html::ImportWarning::SnapshotNodeLimit
        )),
    })).map_err(ConversionError::Serialization)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn snapshot() -> Value {
        json!({ "version": 1, "title": "Round trip", "root": {
            "kind": "element", "tag": "body", "sourceNodeId": "desktop_h1",
            "rect": { "x": 0, "y": 0, "w": 1200, "h": 900 }, "children": [{
                "kind": "text", "sourceNodeId": "desktop_h2_text0", "text": "Hello",
                "rect": { "x": 12, "y": 12, "w": 100, "h": 20 }, "styles": { "font-size": "16px" }
            }]
        }})
    }
    #[test]
    fn stable_ids_survive_canonical_import_and_repeat() {
        let input = snapshot().to_string();
        let converted = convert(&input).unwrap();
        assert_eq!(converted, convert(&input).unwrap());
        let value: Value = serde_json::from_str(&converted).unwrap();
        assert_eq!(value["valid"], true);
        assert_eq!(value["document"]["children"][0]["id"], "studio_desktop_h1");
        assert_eq!(value["document"]["children"][0]["children"][0]["id"], "studio_desktop_h2_text0");
    }
    #[test]
    fn duplicate_source_identity_is_not_a_valid_conversion() {
        let mut input = snapshot();
        input["root"]["children"][0]["sourceNodeId"] = json!("desktop_h1");
        let value: Value = serde_json::from_str(&convert(&input.to_string()).unwrap()).unwrap();
        assert_eq!(value["valid"], false);
        assert_eq!(value["diagnostics"][0]["code"], "snapshot.rejected");
    }
    #[test]
    fn invalid_json_produces_a_diagnostic_not_a_document() {
        let value: Value = serde_json::from_str(&convert("not json").unwrap()).unwrap();
        assert_eq!(value["valid"], false);
    }
}
