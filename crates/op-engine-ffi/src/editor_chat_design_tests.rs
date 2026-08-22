//! Engine-thread tests for the mobile design agent loop: a builtin-provider
//! DESIGN request must run the REAL shared tool loop and land real nodes in
//! the open document (not HTML prose in the transcript), with the tool
//! cards folded into the bound tab's assistant bubble.

use super::*;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use op_editor_core::{BuiltinAgentKind, PenNodeExt};

fn host_with_builtin_provider(base_url: &str) -> WidgetHostNative {
    let mut host = WidgetHostNative::new();
    let settings = &mut host.editor_state_mut().editor_ui.agent_settings;
    settings.builtin_agents.clear();
    settings.add_builtin_agent_config(
        "DeepSeek",
        "sk-mobile-design",
        "deepseek-chat",
        BuiltinAgentKind::OpenAiCompat,
        base_url,
    );
    host.editor_state_mut().rebuild_chat_models();
    let chat = &mut host.editor_state_mut().chat;
    let builtin_index = chat
        .available_models
        .iter()
        .position(|entry| entry.builtin_provider_id.is_some())
        .expect("a ready builtin agent must surface a chat model entry");
    chat.selected_model = builtin_index;
    host
}

fn send_user_message(host: &mut WidgetHostNative, text: &str) {
    let chat = &mut host.editor_state_mut().chat;
    chat.set_input_text(text);
    assert!(chat.begin_send(), "begin_send must queue the turn");
}

/// Serve one canned HTTP response per accepted connection, in order,
/// recording every raw request into the returned log. The serving thread is
/// detached: the shared loop's corrective budgets make the exact request
/// count choreography-dependent, so unused canned responses must never hang
/// the test on a join.
fn spawn_sequential_chat_server(responses: Vec<String>) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind local chat server");
    let address = listener.local_addr().expect("local chat address");
    let requests = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&requests);
    std::thread::spawn(move || {
        for response in responses {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut request = Vec::new();
            let mut chunk = [0_u8; 8192];
            loop {
                let Ok(length) = stream.read(&mut chunk) else {
                    return;
                };
                request.extend_from_slice(&chunk[..length]);
                if length == 0 || request_complete(&request) {
                    break;
                }
            }
            log.lock()
                .expect("request log lock")
                .push(String::from_utf8_lossy(&request).into_owned());
            let _ = stream.write_all(response.as_bytes());
        }
    });
    (format!("http://{address}"), requests)
}

/// True once the buffered request holds its whole `Content-Length` body.
fn request_complete(raw: &[u8]) -> bool {
    let text = String::from_utf8_lossy(raw);
    let Some(header_end) = text.find("\r\n\r\n") else {
        return false;
    };
    let content_length = text
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())?
        })
        .unwrap_or(0);
    raw.len() >= header_end + 4 + content_length
}

fn sse_ok_response(events: &[&str]) -> String {
    let body: String = events
        .iter()
        .map(|event| format!("data: {event}\n\n"))
        .collect();
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
}

/// A voluntary model stop with no tool calls.
fn stop_response() -> String {
    sse_ok_response(&[
        r#"{"choices":[{"delta":{"content":"Done — the login screen is on the canvas."}}]}"#,
        r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#,
        "[DONE]",
    ])
}

fn pump_to_completion(chat_host: &mut MobileChatHost, host: &mut WidgetHostNative) {
    let started = Instant::now();
    let mut now_ms = 10;
    loop {
        let wake = chat_host.pump(host, now_ms);
        if wake.is_none() {
            return;
        }
        assert!(
            started.elapsed() < Duration::from_secs(60),
            "design turn did not complete within the test deadline"
        );
        std::thread::sleep(Duration::from_millis(5));
        now_ms += STREAM_POLL_INTERVAL_MS;
    }
}

fn last_assistant(host: &WidgetHostNative) -> op_editor_core::ChatMessage {
    host.editor_state()
        .chat
        .messages
        .iter()
        .rev()
        .find(|message| message.role == ChatRole::Assistant)
        .expect("transcript holds an assistant bubble")
        .clone()
}

/// SSE events for one model turn that calls `batch_design` with an
/// `operations` DSL program building a small but FILLED login screen (an
/// empty screen-shaped frame would trip the loop's promise-delivery fill
/// rounds and stretch the choreography).
fn tool_call_turn_events() -> Vec<String> {
    let operations = concat!(
        r##"root=I(null,{"type":"frame","name":"Login","width":390,"height":844,"fill":"#111318","layout":"vertical","padding":24,"gap":16})"##,
        "\n",
        r##"title=I(root,{"type":"text","content":"Welcome back","fontSize":28,"fontWeight":"700","textColor":"#FFFFFF"})"##,
        "\n",
        r##"sub=I(root,{"type":"text","content":"Sign in to continue","fontSize":15,"textColor":"#9CA3AF"})"##,
        "\n",
        r##"btn=I(root,{"type":"frame","name":"SignIn","width":"fill_container","height":48,"fill":"#3B82F6","layout":"vertical","alignItems":"center","justifyContent":"center"})"##,
        "\n",
        r##"btnLabel=I(btn,{"type":"text","content":"Sign in","fontSize":16,"fontWeight":"600","textColor":"#FFFFFF"})"##,
    );
    let args = serde_json::json!({ "operations": operations }).to_string();
    let call = serde_json::json!({
        "choices": [{
            "delta": {
                "tool_calls": [{
                    "index": 0,
                    "id": "call_design_1",
                    "function": { "name": "batch_design", "arguments": args },
                }],
            },
        }],
    });
    vec![
        r#"{"choices":[{"delta":{"content":"Building the login screen."}}]}"#.to_string(),
        call.to_string(),
        r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#.to_string(),
        "[DONE]".to_string(),
    ]
}

#[test]
fn design_request_runs_tool_loop_and_inserts_nodes() {
    let turn_one = tool_call_turn_events();
    // Turn 1 calls batch_design; turn 2 stops. Extra stop responses absorb
    // any corrective rounds (fill / blocker nudges) the shared loop decides
    // to spend — unused ones are never served.
    let mut responses = vec![sse_ok_response(
        &turn_one.iter().map(String::as_str).collect::<Vec<_>>(),
    )];
    responses.extend(std::iter::repeat_with(stop_response).take(7));
    let (base_url, requests) = spawn_sequential_chat_server(responses);
    let mut host = host_with_builtin_provider(&base_url);
    send_user_message(&mut host, "设计一个暗色的登录页面");

    let mut chat_host = MobileChatHost::default();
    pump_to_completion(&mut chat_host, &mut host);

    // The design landed as REAL document nodes, not HTML text.
    let login = host
        .editor_state()
        .active_children()
        .iter()
        .find(|node| node.base().name.as_deref() == Some("Login"))
        .expect("batch_design must insert the Login frame into the document");
    assert!(matches!(login, jian_ops_schema::node::PenNode::Frame(_)));
    assert!(
        login
            .children()
            .is_some_and(|children| !children.is_empty()),
        "the login screen keeps its children"
    );

    // Transcript: narration + a finished (not running) batch_design card.
    let reply = last_assistant(&host);
    assert!(!reply.streaming, "a finished design turn stops streaming");
    assert!(reply.content.contains("Building the login screen."));
    assert!(reply
        .content
        .contains("Done — the login screen is on the canvas."));
    let card = reply
        .tool_calls
        .iter()
        .find(|call| call.name == "batch_design")
        .expect("the batch_design call rides the transcript as a tool card");
    let envelope: serde_json::Value = serde_json::from_str(&card.args).expect("card envelope");
    assert_eq!(envelope["status"], "done");
    assert_eq!(envelope["result"]["success"], true);
    // Design-loop turns interleave cards into the narration timeline.
    assert!(card.content_offset.is_some());

    // The designing header cleared once the loop retired.
    assert_eq!(host.editor_state().chat.agents_running, (0, 0));

    let requests = requests.lock().expect("request log lock");
    assert!(requests.len() >= 2, "tool round trip takes two turns");
    // Turn 1 advertises the shared toolset + the design-agent system prompt.
    assert!(requests[0].contains("\"tools\""));
    assert!(requests[0].contains("batch_design"));
    assert!(requests[0].contains("product designer"));
    // DeepSeek's model profile marks thinking_disabled for design turns and
    // its family is on the wire-control whitelist.
    assert!(requests[0].contains("\"thinking\":{\"type\":\"disabled\"}"));
    // Turn 2 replays the tool result correlated by id.
    assert!(requests[1].contains("call_design_1"));
    assert!(requests[1].contains("\"role\":\"tool\""));
}

/// Script-mode `batch_design` — the desktop generation protocol — must
/// execute on the mobile host too (rquickjs via bindgen on the mobile
/// targets; the same code path runs host-side in this test).
#[test]
fn design_request_executes_script_mode_batch_design() {
    let script = r##"
        const cards = [["Recently played", 4], ["Made for you", 6]];
        const root = I(null, {type:"frame", name:"ScriptHome", width:390, height:844, fill:"#0B0B10", layout:"vertical", padding:20, gap:12});
        for (const [label, count] of cards) {
            const section = I(root, {type:"frame", layout:"vertical", width:"fill_container", gap:8});
            I(section, {type:"text", content:label, fontSize:18, fontWeight:"700", textColor:"#FFFFFF"});
            for (let i = 0; i < count; i++) {
                I(section, {type:"text", content:"Track " + (i + 1), fontSize:14, textColor:"#A1A1AA"});
            }
        }
    "##;
    let args = serde_json::json!({ "script": script }).to_string();
    let call = serde_json::json!({
        "choices": [{
            "delta": {
                "tool_calls": [{
                    "index": 0,
                    "id": "call_script_1",
                    "function": { "name": "batch_design", "arguments": args },
                }],
            },
        }],
    });
    let turn_one = [
        call.to_string(),
        r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#.to_string(),
        "[DONE]".to_string(),
    ];
    let mut responses = vec![sse_ok_response(
        &turn_one.iter().map(String::as_str).collect::<Vec<_>>(),
    )];
    responses.extend(std::iter::repeat_with(stop_response).take(7));
    let (base_url, _requests) = spawn_sequential_chat_server(responses);
    let mut host = host_with_builtin_provider(&base_url);
    send_user_message(&mut host, "设计一个音乐首页");

    let mut chat_host = MobileChatHost::default();
    pump_to_completion(&mut chat_host, &mut host);

    let home = host
        .editor_state()
        .active_children()
        .iter()
        .find(|node| node.base().name.as_deref() == Some("ScriptHome"))
        .expect("script-mode batch_design must insert the ScriptHome frame");
    // The JS loop emitted 2 sections; each carries a title + its tracks.
    let sections = home.children().expect("script home has sections");
    assert_eq!(sections.len(), 2);
    assert_eq!(sections[0].children().map(|c| c.len()), Some(1 + 4));
    assert_eq!(sections[1].children().map(|c| c.len()), Some(1 + 6));
    let card = last_assistant(&host)
        .tool_calls
        .iter()
        .find(|call| call.name == "batch_design")
        .cloned()
        .expect("script call rides the transcript");
    let envelope: serde_json::Value = serde_json::from_str(&card.args).expect("card envelope");
    assert_eq!(envelope["status"], "done");
    assert_eq!(envelope["result"]["success"], true);
}

#[test]
fn plain_chat_request_keeps_the_plain_streaming_path() {
    let (base_url, requests) = spawn_sequential_chat_server(vec![sse_ok_response(&[
        r#"{"choices":[{"delta":{"content":"A frame is a container."}}]}"#,
        "[DONE]",
    ])]);
    let mut host = host_with_builtin_provider(&base_url);
    send_user_message(&mut host, "what is a frame?");

    let mut chat_host = MobileChatHost::default();
    pump_to_completion(&mut chat_host, &mut host);

    assert_eq!(last_assistant(&host).content, "A frame is a container.");
    let requests = requests.lock().expect("request log lock");
    // Plain turns advertise no tools (and carry no design prompt).
    assert!(!requests[0].contains("\"tools\""));
    assert!(!requests[0].contains("product designer"));
}

#[test]
fn design_loop_provider_error_lands_in_bubble_and_clears_designing_header() {
    let (base_url, _requests) = spawn_sequential_chat_server(vec![
        "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            .to_string(),
    ]);
    let mut host = host_with_builtin_provider(&base_url);
    send_user_message(&mut host, "design a landing page");

    let mut chat_host = MobileChatHost::default();
    pump_to_completion(&mut chat_host, &mut host);

    let reply = last_assistant(&host);
    assert!(
        reply.content.starts_with("error: "),
        "provider failure must surface, got {:?}",
        reply.content
    );
    assert!(reply.content.contains("http 500"));
    assert!(!reply.streaming);
    assert_eq!(host.editor_state().chat.agents_running, (0, 0));
}

/// Real end-to-end run against DeepSeek. Ignored by default; run with
/// `OPENPENCIL_TEST_DEEPSEEK_KEY=sk-… cargo test -p op-engine-ffi \
/// --features editor real_deepseek -- --ignored --nocapture`.
#[test]
#[ignore = "needs a real DeepSeek API key + network"]
fn real_deepseek_design_turn_inserts_nodes() {
    let Ok(key) = std::env::var("OPENPENCIL_TEST_DEEPSEEK_KEY") else {
        panic!("set OPENPENCIL_TEST_DEEPSEEK_KEY to run this test");
    };
    let mut host = WidgetHostNative::new();
    let settings = &mut host.editor_state_mut().editor_ui.agent_settings;
    settings.builtin_agents.clear();
    settings.add_builtin_agent_config(
        "DeepSeek",
        &key,
        "deepseek-chat",
        BuiltinAgentKind::OpenAiCompat,
        "https://api.deepseek.com",
    );
    host.editor_state_mut().rebuild_chat_models();
    let chat = &mut host.editor_state_mut().chat;
    let builtin_index = chat
        .available_models
        .iter()
        .position(|entry| entry.builtin_provider_id.is_some())
        .expect("builtin model entry");
    chat.selected_model = builtin_index;
    send_user_message(&mut host, "设计一个暗色音乐流媒体 App 的首页(390x844)");

    let mut chat_host = MobileChatHost::default();
    let started = Instant::now();
    let mut now_ms = 10;
    loop {
        if chat_host.pump(&mut host, now_ms).is_none() {
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(600),
            "real design turn did not complete within 10 minutes"
        );
        std::thread::sleep(Duration::from_millis(20));
        now_ms += STREAM_POLL_INTERVAL_MS;
    }

    let reply = last_assistant(&host);
    let mut script_calls = 0usize;
    let mut operations_calls = 0usize;
    for call in &reply.tool_calls {
        if call.name != "batch_design" {
            continue;
        }
        let args: serde_json::Value = serde_json::from_str(&call.args).unwrap_or_default();
        let inner = &args["args"];
        if inner.get("script").is_some() {
            script_calls += 1;
        }
        if inner.get("operations").is_some() {
            operations_calls += 1;
        }
    }
    let tool_names: Vec<&str> = reply
        .tool_calls
        .iter()
        .map(|call| call.name.as_str())
        .collect();
    eprintln!("--- transcript content ---\n{}", reply.content);
    eprintln!("--- tool calls: {tool_names:?}");
    eprintln!(
        "--- batch_design script calls: {script_calls}, operations calls: {operations_calls}"
    );
    eprintln!(
        "--- top-level nodes: {:?}",
        host.editor_state()
            .active_children()
            .iter()
            .map(|node| node.base().name.clone())
            .collect::<Vec<_>>()
    );
    assert!(
        !host.editor_state().active_children().is_empty(),
        "a real design turn must land nodes in the document"
    );
    assert!(
        tool_names.contains(&"batch_design"),
        "the model must build through batch_design, got {tool_names:?}"
    );
    assert!(
        !reply.content.contains("<html") && !reply.content.contains("<!DOCTYPE"),
        "the transcript must not be an HTML page"
    );
}
