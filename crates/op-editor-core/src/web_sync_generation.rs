//! Generation-aware authority decisions, paired with the numeric sync counter.
use super::{WebSyncClient, WebSyncError};

/// One observed server incarnation and counter; never mix independently read fields.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WebSyncAuthority {
    generation: Option<String>,
    pub version: u64,
}

impl WebSyncClient {
    /// Add the applied baseline to a trusted serialized Save envelope without
    /// reparsing the potentially large document or changing embedded metadata.
    pub fn fence_save_envelope(&self, envelope: &str) -> Result<String, WebSyncError> {
        let rest = envelope.strip_prefix('{')
            .ok_or_else(|| WebSyncError::ResponseParse("Save envelope must be an object".into()))?;
        let generation = self.applied_generation.as_ref()
            .map(|g| format!(r#""baseGeneration":"{g}","#)).unwrap_or_default();
        let separator = if rest.trim() == "}" { "" } else { "," };
        Ok(format!(r#"{{{generation}"baseVersion":{}{separator}{}"#, self.last_version(), rest))
    }
    pub fn parse_authority(&self, body: &str) -> Result<WebSyncAuthority, WebSyncError> {
        let value: serde_json::Value = serde_json::from_str(body)
            .map_err(|e| WebSyncError::ResponseParse(e.to_string()))?;
        let version = value.get("version").and_then(|v| v.as_u64())
            .ok_or_else(|| WebSyncError::ResponseParse("Missing authority version".into()))?;
        let generation = Self::response_generation(&value)?;
        if self.applied_generation.is_some() && generation.is_none() {
            return Err(WebSyncError::ResponseParse("Missing authority generation".into()));
        }
        Ok(WebSyncAuthority { generation, version })
    }

    /// Call only after a successful conditional push against this exact authority.
    pub fn acknowledge_push_authority(&mut self, authority: &WebSyncAuthority) {
        self.applied_generation = authority.generation.clone();
    }

    pub fn wrap_push_with_authority(doc_json: &str, authority: &WebSyncAuthority, active_page_index: usize, preserve: bool) -> String {
        let body = Self::wrap_push_body_with_base_and_editor_meta(doc_json, authority.version, active_page_index, preserve);
        match &authority.generation {
            Some(generation) => format!(r#"{{"baseGeneration":"{generation}",{}"#, &body[1..]),
            None => body,
        }
    }
    pub(super) fn response_generation(value: &serde_json::Value) -> Result<Option<String>, WebSyncError> {
        let Some(raw) = value.get("generation") else { return Ok(None); };
        let Some(value) = raw.as_str().filter(|g| g.len() == 32 && g.bytes().all(|b| b.is_ascii_hexdigit())) else {
            return Err(WebSyncError::ResponseParse("Invalid document authority generation".into()));
        };
        Ok(Some(value.to_owned()))
    }

    /// A new incarnation must be fetched even if its counter is smaller.
    /// The host still checks its dirty/pull gate before applying that document.
    pub fn wants_version_response(&self, body: &str) -> bool {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else { return false; };
        let Some(version) = value.get("version").and_then(|v| v.as_u64()) else { return false; };
        let Ok(generation) = Self::response_generation(&value) else { return false; };
        if self.applied_generation.is_some() && generation.is_none() { return false; }
        generation != self.applied_generation || self.wants_version(version)
    }

    pub fn wrap_push_for_authority(&self, doc_json: &str, active_page_index: usize, preserve: bool, metadata_only: bool) -> String {
        let body = Self::wrap_push_body_with_base_editor_meta_and_mode(doc_json, self.last_version(), active_page_index, preserve, metadata_only);
        match &self.applied_generation {
            Some(generation) => format!(r#"{{"baseGeneration":"{generation}",{}"#, &body[1..]),
            None => body,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    fn response(generation: &str, version: u64) -> String {
        format!(r#"{{"generation":"{generation}","version":{version},"document":{{"version":"1.0","children":[]}}}}"#)
    }
    #[test]
    fn file_save_uses_applied_pair_and_preserves_serialized_document() {
        let mut client = WebSyncClient::new();
        client.sync(&response(A, 9), |_, _| true).unwrap();
        let envelope = r#"{"document":{"children":[],"editorMeta":{"activePageIndex":2}},"activePageIndex":2}"#;
        let fenced = client.fence_save_envelope(envelope).unwrap();
        assert!(fenced.ends_with(&envelope[1..]));
        let parsed: serde_json::Value = serde_json::from_str(&fenced).unwrap();
        assert_eq!(parsed["baseGeneration"], A);
        assert_eq!(parsed["baseVersion"], 9);
        assert_eq!(parsed["document"]["editorMeta"]["activePageIndex"], 2);
        assert!(client.fence_save_envelope("[]").is_err());
        assert!(serde_json::from_str::<serde_json::Value>(&client.fence_save_envelope("{}").unwrap()).is_ok());
    }
    #[test]
    fn explicit_authority_is_not_adopted_until_push_acknowledgement() {
        let mut client = WebSyncClient::new();
        assert!(client.sync(&response(A, 7), |_, _| true).unwrap());
        let observed = client.parse_authority(&response(B, 1)).unwrap();
        let body: serde_json::Value = serde_json::from_str(
            &WebSyncClient::wrap_push_with_authority("{}", &observed, 2, true),
        ).unwrap();
        assert_eq!(body["baseGeneration"], B);
        assert_eq!(body["baseVersion"], 1);
        assert_eq!(body["activePageIndex"], 2);
        let local: serde_json::Value = serde_json::from_str(
            &client.wrap_push_for_authority("{}", 0, false, false),
        ).unwrap();
        assert_eq!(local["baseGeneration"], A);
        assert_eq!(local["baseVersion"], 7);
        assert!(client.parse_authority(r#"{"version":2}"#).is_err());
        assert!(client.parse_authority(&response("invalid", 2)).is_err());
        assert!(client.parse_authority(r#"{"generation":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","version":-1}"#).is_err());
        client.acknowledge_push_authority(&observed);
        client.mark_applied(2);
        let accepted: serde_json::Value = serde_json::from_str(
            &client.wrap_push_for_authority("{}", 0, false, false),
        ).unwrap();
        assert_eq!(accepted["baseGeneration"], B);
        assert_eq!(accepted["baseVersion"], 2);
    }
    #[test]
    fn restart_is_new_authority_and_only_successful_apply_commits_it() {
        let mut client = WebSyncClient::new();
        assert!(client.sync(&response(A, 30), |_, _| true).unwrap());
        assert!(client.wants_version_response(&response(B, 1)));
        assert!(!client.sync(&response(B, 1), |_, _| false).unwrap());
        let old: serde_json::Value = serde_json::from_str(&client.wrap_push_for_authority("{}", 0, false, false)).unwrap();
        assert_eq!(old["baseGeneration"], A); assert_eq!(old["baseVersion"], 30);
        assert!(client.sync(&response(B, 1), |_, _| true).unwrap());
        let new: serde_json::Value = serde_json::from_str(&client.wrap_push_for_authority("{}", 0, true, true)).unwrap();
        assert_eq!(new["baseGeneration"], B); assert_eq!(new["baseVersion"], 1);
        assert_eq!(new["metadataOnly"], true); assert_eq!(new["preserveAuthoredGeometry"], true);
        assert!(!client.wants_version_response(&response(B, 1)));
        assert!(!client.wants_version_response(r#"{"version":100}"#));
        assert!(client.next_document(r#"{"version":100,"document":{}}"#).is_err());
        assert!(client.next_document(&response("invalid", 100)).is_err());
    }
}
