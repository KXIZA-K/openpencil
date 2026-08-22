use super::*;

#[test]
fn simplify_search_query_mirrors_the_desktop_adapter() {
    assert_eq!(
        simplify_search_query("A beautiful sunset over the mountains"),
        "beautiful sunset over mountains"
    );
    // Artifact words drop only when aesthetic words remain.
    assert_eq!(
        simplify_search_query("synthwave album cover neon"),
        "synthwave neon"
    );
    assert_eq!(simplify_search_query("logo"), "logo");
    // Empty keyword set falls back to a 30-char prefix.
    assert_eq!(simplify_search_query("の"), "の");
}

#[test]
fn parse_search_request_reads_query_and_prefers_request_credentials() {
    let mut state = op_editor_core::EditorState::default();
    state.editor_ui.agent_settings.openverse_client_id = "persisted-id".into();
    state.editor_ui.agent_settings.openverse_client_secret = "persisted-secret".into();
    let (query, cred) = parse_search_request(
        r#"{"query":"cat","openverse":{"client_id":"req-id","client_secret":"req-secret"}}"#,
        &state,
    )
    .expect("parses");
    assert_eq!(query, "cat");
    assert_eq!(cred.expect("cred").client_id, "req-id");
    // No request credential → daemon-persisted fallback.
    let (_, cred) = parse_search_request(r#"{"query":"cat"}"#, &state).expect("parses");
    assert_eq!(cred.expect("cred").client_id, "persisted-id");
    // Neither → anonymous.
    let empty = op_editor_core::EditorState::default();
    let (_, cred) = parse_search_request(r#"{"query":"cat"}"#, &empty).expect("parses");
    assert!(cred.is_none());
}

#[test]
fn parse_search_request_rejects_bad_bodies() {
    let state = op_editor_core::EditorState::default();
    assert!(parse_search_request("", &state).is_err());
    assert!(parse_search_request("{}", &state).is_err());
    assert!(parse_search_request(r#"{"query":"  "}"#, &state).is_err());
}

#[test]
fn parse_openverse_results_maps_thumbnail_license_and_cap() {
    let json = serde_json::json!({
        "results": [
            {"id": "a", "thumbnail": "https://x/a.jpg", "attribution": "By A"},
            {"id": "b", "url": "https://x/b.jpg", "license": "cc0", "license_version": "1.0"},
            {"id": "c"},
            {"id": "d", "thumbnail": "https://x/d.jpg"},
            {"id": "e", "thumbnail": "https://x/e.jpg"},
            {"id": "f", "thumbnail": "https://x/f.jpg"},
            {"id": "g", "thumbnail": "https://x/g.jpg"}
        ]
    });
    let hits = parse_openverse_results(&json);
    assert_eq!(hits.len(), SEARCH_RESULT_COUNT); // "c" dropped, capped at 5
    assert_eq!(hits[0].id, "a");
    assert_eq!(hits[0].attribution, "By A");
    assert_eq!(hits[1].thumb_url, "https://x/b.jpg");
    assert_eq!(hits[1].attribution, "cc0 1.0");
}

#[test]
fn parse_wikimedia_results_maps_thumburl_and_license() {
    let json = serde_json::json!({
        "query": {"pages": {
            "1": {"pageid": 1, "imageinfo": [{
                "thumburl": "https://c/w1.jpg",
                "extmetadata": {"LicenseShortName": {"value": "CC BY-SA 4.0"}}
            }]},
            "2": {"pageid": 2, "imageinfo": [{"url": "https://c/w2.jpg"}]},
            "3": {"pageid": 3}
        }}
    });
    let mut hits = parse_wikimedia_results(&json);
    hits.sort_by(|a, b| a.id.cmp(&b.id));
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].thumb_url, "https://c/w1.jpg");
    assert_eq!(hits[0].attribution, "CC BY-SA 4.0");
    assert_eq!(hits[1].thumb_url, "https://c/w2.jpg");
}

#[test]
fn search_outcome_json_shape() {
    let outcome = WebImageSearchOutcome {
        results: vec![WebImageSearchHit {
            id: "a".into(),
            thumb_data_url: "data:image/png;base64,AA==".into(),
            attribution: "By A".into(),
        }],
        source: Some("openverse"),
    };
    let json: serde_json::Value =
        serde_json::from_str(&search_outcome_to_json(&outcome)).expect("valid json");
    assert_eq!(json["ok"], true);
    assert_eq!(json["source"], "openverse");
    assert_eq!(json["results"][0]["id"], "a");
    assert_eq!(
        json["results"][0]["thumb_data_url"],
        "data:image/png;base64,AA=="
    );
    let empty = WebImageSearchOutcome {
        results: Vec::new(),
        source: None,
    };
    let json: serde_json::Value =
        serde_json::from_str(&search_outcome_to_json(&empty)).expect("valid json");
    assert!(json["source"].is_null());
}

#[test]
fn image_job_slot_caps_concurrency_and_releases_on_drop() {
    let held: Vec<_> = (0..MAX_IN_FLIGHT_IMAGE_JOBS)
        .map(|_| ImageJobSlot::acquire().expect("slot under the cap"))
        .collect();
    assert!(
        ImageJobSlot::acquire().is_none(),
        "cap reached — acquire must fail"
    );
    drop(held);
    assert!(
        ImageJobSlot::acquire().is_some(),
        "drop must release the slots"
    );
}

#[test]
fn sniff_image_mime_recognizes_the_embeddable_formats() {
    assert_eq!(sniff_image_mime(b"\x89PNG\r\n\x1A\nxx"), Some("image/png"));
    assert_eq!(sniff_image_mime(b"\xFF\xD8\xFFxx"), Some("image/jpeg"));
    assert_eq!(sniff_image_mime(b"GIF89a"), Some("image/gif"));
    assert_eq!(
        sniff_image_mime(b"RIFF\0\0\0\0WEBPVP8 "),
        Some("image/webp")
    );
    assert_eq!(sniff_image_mime(b"<svg>"), None);
    assert_eq!(
        normalize_image_mime_header("image/jpg"),
        Some("image/jpeg".into())
    );
    assert_eq!(normalize_image_mime_header("image/svg+xml"), None);
    assert_eq!(normalize_image_mime_header("text/html"), None);
}
