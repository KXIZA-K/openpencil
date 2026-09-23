use super::*;
#[test]
fn prepare_turn_after_paging_uses_only_active_thread_and_bounded_model_context() {
    use op_editor_core::chat::ChatMessage;
    let mut state = EditorState::new();
    let messages = (0..500).map(|i| {
        if i % 2 == 0 { ChatMessage::user(format!("A request {i}")) }
        else { ChatMessage::assistant(format!("A reply {i}")) }
    }).collect();
    state.chat.hydrate_managed_threads(vec![
        ("pthr_a".into(), "A".into(), messages),
        ("pthr_b".into(), "B".into(), vec![ChatMessage::user("B must not leak")]),
    ]);
    state.chat.switch_to(1);
    state.chat.set_input_text("B unsent draft");
    state.chat.switch_to(0);
    state.chat.available_models = vec![op_editor_core::chat::ModelEntry::builtin_with_display_name(
        op_editor_core::chat::AgentProvider::CodexCli, "daemon-builtin:fixture", "Fixture",
        "builtin:fixture:exact-model", "Fixture model",
    )];
    state.chat.history_before = Some(1);
    state.chat.set_input_text("A current request");
    assert!(state.chat.begin_send());
    let prepared = prepare_turn(&mut state).expect("pending send");
    let body: serde_json::Value = serde_json::from_str(&prepared.body_json).unwrap();
    assert_eq!(body["model"], "builtin:fixture:exact-model");
    assert_eq!(body["user"], "A current request");
    let history = body["history"].as_array().unwrap();
    // The existing policy keeps the first user message plus ten recent rows.
    assert_eq!(history.len(), DEFAULT_MAX_MESSAGES + 1);
    assert_eq!(history[0]["content"], "A request 0");
    assert_eq!(history.last().unwrap()["content"], "A reply 499");
    assert!(!body["history"].to_string().contains("current request"));
    assert!(!prepared.body_json.contains("B must not leak"));
    assert!(!prepared.body_json.contains("B unsent draft"));
    assert_eq!(state.chat.messages.len(), 502, "Context trimming must not delete visible history");
    assert_eq!(state.chat.thread_id.as_deref(), Some("pthr_a"));
    state.chat.switch_to(1);
    assert_eq!(state.chat.input.text(), "B unsent draft");
    assert_eq!(state.chat.messages.len(), 1);
}
    use op_editor_core::chat::{AgentProvider, ModelEntry};

    fn state_with_queued_send(text: &str) -> EditorState {
        let mut state = EditorState::new();
        state.chat.set_input_text(text);
        assert!(state.chat.begin_send());
        state
    }

    // Returns the whole `ChatSessions` (the `.chat` field is the multi-tab
    // container since the multi-chat-tabs change); callers drive it through its
    // `DerefMut<Target = ChatState>` so `&mut chat` still coerces to the
    // `&mut ChatState` that `apply_event_to_chat` expects.
    fn chat_with_queued_send(text: &str) -> op_editor_core::ChatSessions {
        state_with_queued_send(text).chat
    }

    #[test]
    fn transient_gateway_stream_failures_are_retryable() {
        for status in [0, 429, 502, 503, 504] {
            assert!(is_retryable_turn_error(&format!(
                "AI stream ended unexpectedly (status {status})"
            )));
        }
        assert!(!is_retryable_turn_error(
            "AI stream ended unexpectedly (status 400)"
        ));
        assert!(!is_retryable_turn_error("provider rejected the request"));
    }

    #[test]
    fn prepare_turn_carries_model_and_message() {
        let mut state = state_with_queued_send("design a login page");
        state.editor_ui.preserve_authored_geometry = true;
        state.chat.available_models = vec![ModelEntry::builtin_with_display_name(
            AgentProvider::ClaudeCode,
            "daemon-builtin:server-1",
            "Server API Key",
            "builtin:server-1:claude-sonnet-4-5",
            "Claude Sonnet 4.5",
        )];
        state.chat.selected_model = 0;
        state.chat.agent_team_size = 3;
        let prepared = prepare_turn(&mut state).expect("send was pending");
        assert_eq!(prepared.endpoint, "/api/ai/standard");
        let body: serde_json::Value =
            serde_json::from_str(&prepared.body_json).expect("body is JSON");
        assert_eq!(body["provider"], "claude-code");
        assert_eq!(body["model"], "builtin:server-1:claude-sonnet-4-5");
        assert_eq!(body["user"], "design a login page");
        assert_eq!(body["max_output_tokens"], 4096);
        assert_eq!(body["agent_team_size"], 3);
        assert!(body["document"].is_object());
        assert_eq!(body["editorMeta"]["activePageIndex"], 0);
        assert_eq!(body["editorMeta"]["preserveAuthoredGeometry"], true);
        assert!(body["skills"].as_array().is_some_and(Vec::is_empty));
        // The drain consumed the flag — a second drain is idle.
        assert!(prepare_turn(&mut state).is_none());
    }

    #[test]
    fn prepare_turn_carries_prior_history_without_current_turn() {
        let mut state = EditorState::new();
        state
            .chat
            .messages
            .push(op_editor_core::ChatMessage::user("previous request"));
        state
            .chat
            .messages
            .push(op_editor_core::ChatMessage::assistant("previous answer"));
        state.chat.set_input_text("current request");
        assert!(state.chat.begin_send());

        let prepared = prepare_turn(&mut state).expect("send was pending");
        let body: serde_json::Value =
            serde_json::from_str(&prepared.body_json).expect("body is JSON");
        let history = body["history"].as_array().expect("history array");

        assert_eq!(history.len(), 2, "{history:?}");
        assert_eq!(history[0]["role"], "user");
        assert_eq!(history[0]["content"], "previous request");
        assert_eq!(history[1]["role"], "assistant");
        assert_eq!(history[1]["content"], "previous answer");
        assert!(
            !history
                .iter()
                .any(|item| item["content"] == "current request"),
            "current user message rides `user`, not history: {history:?}"
        );
    }

    #[test]
    fn prepare_turn_defaults_model_when_catalog_empty() {
        let mut state = state_with_queued_send("hi");
        let thinking = state.chat.thinking_mode;
        let effort = state.chat.effort_level;
        let prepared = prepare_turn(&mut state).expect("send was pending");
        let body: serde_json::Value =
            serde_json::from_str(&prepared.body_json).expect("body is JSON");
        assert!(body["provider"].is_null());
        assert_eq!(body["model"], "default");
        assert_eq!(body["thinking"], thinking.as_str());
        assert_eq!(body["effort"], effort.as_str());
    }

    #[test]
    fn prepare_turn_clears_staged_attachments() {
        let mut state = EditorState::new();
        state.chat.set_input_text("look at this");
        state
            .chat
            .pending_attachments
            .push(op_editor_core::chat::ChatAttachment {
                name: "brief.pdf".into(),
                media_type: "application/pdf".into(),
                data: vec![1, 2, 3],
            });
        assert!(state.chat.begin_send());
        let _ = prepare_turn(&mut state).expect("send was pending");
        assert!(state.chat.pending_attachments.is_empty());
    }

    #[test]
    fn prepare_turn_carries_staged_attachments_on_the_wire() {
        let mut state = EditorState::new();
        state.chat.set_input_text("look at this");
        state
            .chat
            .pending_attachments
            .push(op_editor_core::chat::ChatAttachment {
                name: "brief.pdf".into(),
                media_type: "application/pdf".into(),
                data: vec![1, 2, 3],
            });
        assert!(state.chat.begin_send());

        let prepared = prepare_turn(&mut state).expect("send was pending");
        let body: serde_json::Value =
            serde_json::from_str(&prepared.body_json).expect("body is JSON");
        let attachments = body["attachments"].as_array().expect("attachments array");

        assert_eq!(attachments.len(), 1, "{attachments:?}");
        assert_eq!(attachments[0]["name"], "brief.pdf");
        assert_eq!(attachments[0]["media_type"], "application/pdf");
        assert_eq!(attachments[0]["data_base64"], "AQID");
        assert!(state.chat.pending_attachments.is_empty());
    }

    #[test]
    fn streamed_events_fold_into_streaming_bubble() {
        let mut chat = chat_with_queued_send("hello");
        assert!(!apply_event_to_chat(
            &mut chat,
            &AiEvent::AgentIdentity {
                name: "Mochi".into(),
                color: "#4ECDC4".into(),
            }
        ));
        assert!(!apply_event_to_chat(
            &mut chat,
            &AiEvent::Delta("Hi ".into())
        ));
        assert!(!apply_event_to_chat(
            &mut chat,
            &AiEvent::Thinking("consider…".into())
        ));
        assert!(!apply_event_to_chat(
            &mut chat,
            &AiEvent::Delta("there".into())
        ));
        assert!(apply_event_to_chat(&mut chat, &AiEvent::Done));
        let msg = chat.messages.last().expect("assistant bubble");
        assert_eq!(msg.content, "Hi there");
        assert_eq!(msg.thinking, "consider…");
        assert_eq!(msg.agent_name.as_deref(), Some("Mochi"));
        assert_eq!(msg.agent_color.as_deref(), Some("#4ECDC4"));
        assert!(!msg.streaming, "Done clears the streaming flag");
    }

    #[test]
    fn error_replaces_bubble_body_and_ends_turn() {
        let mut chat = chat_with_queued_send("hello");
        let _ = apply_event_to_chat(&mut chat, &AiEvent::Delta("partial".into()));
        assert!(apply_event_to_chat(
            &mut chat,
            &AiEvent::Error("no model configured".into())
        ));
        let msg = chat.messages.last().expect("assistant bubble");
        assert_eq!(msg.content, "error: no model configured");
        assert!(!msg.streaming);
    }

    #[test]
    fn late_events_without_streaming_bubble_are_dropped() {
        let mut chat = chat_with_queued_send("hello");
        assert!(chat.stop_streaming());
        let before = chat.messages.clone();
        assert!(!apply_event_to_chat(
            &mut chat,
            &AiEvent::Delta("late".into())
        ));
        assert!(apply_event_to_chat(&mut chat, &AiEvent::Done));
        assert_eq!(chat.messages, before, "stopped transcript is untouched");
    }
    #[test]
    fn parse_models_json_reads_string_arrays_leniently() {
        let models = parse_models_json(r#"["claude-sonnet-4-5","gpt-5.5"]"#);
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].value, "claude-sonnet-4-5");
        assert_eq!(models[1].value, "gpt-5.5");
        let models = parse_models_json(r#"["", "  ", 3, "gemini-2.5-pro"]"#);
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].value, "gemini-2.5-pro");
        assert!(parse_models_json("{}").is_empty());
        assert!(parse_models_json("not json").is_empty());
    }

    #[test]
    fn structured_catalog_groups_managed_models_by_provider_and_drops_cli_rows() {
        let models = parse_models_json(
            r#"[
                {"provider":"codex-cli","value":"builtin:server-1:gpt-5.4","displayName":"GPT-5.4","providerGroup":"codex","providerDisplayName":"Server OpenAI"},
                {"provider":"codex-cli","value":"builtin:server-2:gpt-5.5","displayName":"GPT-5.5","providerGroup":"codex","providerDisplayName":"Server OpenAI"},
                {"provider":"grok-build","value":"default","displayName":"Default"}
            ]"#,
        );
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].provider, AgentProvider::CodexCli);
        assert_eq!(models[0].value, "builtin:server-1:gpt-5.4");
        assert_eq!(
            models[0].builtin_provider_id.as_deref(),
            Some("daemon-builtin:group:codex")
        );
        assert_eq!(
            models[0].builtin_provider_id, models[1].builtin_provider_id,
            "models rendered under one managed provider heading share its picker group id"
        );
        assert_ne!(models[0].value, models[1].value);
    }

    #[test]
    fn apply_models_populates_picker_and_preserves_selection() {
        let mut state = EditorState::new();
        let ids = vec![
            "claude-sonnet-4-5".to_string(),
            "gemini-2.5-pro".to_string(),
        ];
        apply_models(&mut state, &ids);
        assert_eq!(state.chat.available_models.len(), 2);
        assert_eq!(state.chat.discovered_models.len(), 2);
        assert_eq!(state.chat.selected_model, 0);
        assert!(state
            .chat
            .available_models
            .iter()
            .all(|entry| entry.builtin_provider_id.is_some()));
        assert_eq!(
            state.chat.available_models[1].provider,
            AgentProvider::CodexCli
        );
        // Select the second model, then re-apply a re-ordered catalog — the
        // selection follows the entry by identity.
        state.chat.selected_model = 1;
        let ids2 = vec![
            "gemini-2.5-pro".to_string(),
            "claude-sonnet-4-5".to_string(),
        ];
        apply_models(&mut state, &ids2);
        assert_eq!(state.chat.selected_model, 0);
        assert_eq!(
            state.chat.selected_model_entry().map(|m| m.value.as_str()),
            Some("gemini-2.5-pro")
        );
    }

    #[test]
    fn provider_heuristic_groups_known_id_shapes() {
        assert_eq!(
            provider_for_model_id("claude-sonnet-4-5"),
            AgentProvider::ClaudeCode
        );
        assert_eq!(provider_for_model_id("gpt-5.5"), AgentProvider::CodexCli);
        assert_eq!(
            provider_for_model_id("gemini-2.5-pro"),
            AgentProvider::CodexCli
        );
        assert_eq!(
            provider_for_model_id("copilot-fast"),
            AgentProvider::GithubCopilot
        );
        assert_eq!(
            provider_for_model_id("minimax-m2"),
            AgentProvider::ClaudeCode
        );
    }
