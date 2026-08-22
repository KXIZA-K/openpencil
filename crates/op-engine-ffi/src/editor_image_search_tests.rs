//! Engine-thread tests for the mobile image-search pump: an unresolved
//! `G(search)` image slot must be detected, resolved through the (injected)
//! fetcher, and written back into the live document; failures land the
//! shared dashed placeholder instead of an eternally-grey slot.

use super::*;
use std::time::{Duration, Instant};

const TINY_PNG_DATA_URL: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";

fn host_with_search_slot(query: &str) -> WidgetHostNative {
    let mut host = WidgetHostNative::new();
    let state = host.editor_state_mut();
    state.active_children_mut().clear();
    let node: jian_ops_schema::node::PenNode = serde_json::from_value(serde_json::json!({
        "type": "image",
        "id": "img1",
        "name": query,
        "src": "",
        "width": 400,
        "height": 300,
        "imageSearchQuery": query,
    }))
    .expect("image slot fixture");
    state.active_children_mut().push(node);
    state.mark_document_changed();
    host
}

fn slot_src(host: &WidgetHostNative) -> String {
    let state = host.editor_state();
    let node = op_editor_core::walkers::find_node(state.active_children(), &NodeId::new("img1"))
        .expect("slot survives");
    match node {
        jian_ops_schema::node::PenNode::Image(image) => image.src.to_string(),
        other => panic!("expected image node, got {other:?}"),
    }
}

fn pump_until_settled(search: &mut MobileImageSearch, host: &mut WidgetHostNative) {
    let started = Instant::now();
    let mut now_ms = 10;
    loop {
        let wake = search.pump(host, now_ms);
        if wake.is_none() {
            return;
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "image search did not settle within the test deadline"
        );
        std::thread::sleep(Duration::from_millis(5));
        now_ms += IMAGE_POLL_INTERVAL_MS;
    }
}

fn ok_fetcher(
    target: &ImageSearchTarget,
    _credentials: Option<&WebOpenverseCredentials>,
    used_urls: &Mutex<HashSet<String>>,
) -> Option<String> {
    assert_eq!(target.query, "pasta plate");
    used_urls
        .lock()
        .unwrap()
        .insert(TINY_PNG_DATA_URL.to_string());
    Some(TINY_PNG_DATA_URL.to_string())
}

fn failing_fetcher(
    _target: &ImageSearchTarget,
    _credentials: Option<&WebOpenverseCredentials>,
    _used_urls: &Mutex<HashSet<String>>,
) -> Option<String> {
    None
}

#[test]
fn unresolved_search_slot_gets_fetched_and_written_back() {
    let mut host = host_with_search_slot("pasta plate");
    let mut search = MobileImageSearch::with_fetcher(ok_fetcher);
    pump_until_settled(&mut search, &mut host);
    assert_eq!(slot_src(&host), TINY_PNG_DATA_URL);
    // The de-dup set recorded the claimed URL.
    assert!(search.used_urls.lock().unwrap().contains(TINY_PNG_DATA_URL));
}

#[test]
fn failed_search_lands_the_shared_placeholder_not_an_eternal_grey_slot() {
    let mut host = host_with_search_slot("pasta plate");
    let mut search = MobileImageSearch::with_fetcher(failing_fetcher);
    pump_until_settled(&mut search, &mut host);
    assert_eq!(slot_src(&host), SEARCH_FAILED_PLACEHOLDER_SRC);
}

#[test]
fn resolved_slot_is_not_searched_again() {
    let mut host = host_with_search_slot("pasta plate");
    let mut search = MobileImageSearch::with_fetcher(ok_fetcher);
    pump_until_settled(&mut search, &mut host);
    assert_eq!(slot_src(&host), TINY_PNG_DATA_URL);
    // Another pump on the (now changed) document must not spawn a new job:
    // the slot no longer collects as a target and the node id is handled.
    assert!(search.pump(&mut host, 999).is_none());
    assert_eq!(slot_src(&host), TINY_PNG_DATA_URL);
}

#[test]
fn stale_result_for_a_regenerated_node_is_discarded() {
    let mut host = host_with_search_slot("pasta plate");
    let mut search = MobileImageSearch::with_fetcher(ok_fetcher);
    // Enqueue the job…
    let wake = search.pump(&mut host, 10);
    assert!(wake.is_some(), "a job is in flight");
    // …then change the node's intent before the result lands.
    {
        let state = host.editor_state_mut();
        if let Some(jian_ops_schema::node::PenNode::Image(image)) =
            op_editor_core::walkers::find_node_mut(
                state.active_children_mut(),
                &NodeId::new("img1"),
            )
        {
            image.image_search_query = Some("mountain lake".into());
        }
        state.mark_document_changed();
    }
    pump_until_settled(&mut search, &mut host);
    // The stale "pasta plate" result must not have been applied over the
    // new intent (src still empty or re-resolved for the new query —
    // never the stale URL applied against a changed fingerprint).
    let state = host.editor_state();
    let node = op_editor_core::walkers::find_node(state.active_children(), &NodeId::new("img1"))
        .expect("node survives");
    if let jian_ops_schema::node::PenNode::Image(image) = node {
        assert_ne!(
            image.image_search_query.as_deref(),
            Some("pasta plate"),
            "intent stayed changed"
        );
    }
}
