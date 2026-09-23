use super::WidgetHost;

#[test]
fn opening_chat_tab_moves_composer_into_the_sidebar() {
    let mut host = WidgetHost::new();
    host.editor_state.editor_ui.entry_surface = op_editor_core::EntrySurface::Canvas;
    let before = host.ai_chat_rect(1200.0, 800.0).expect("composer");
    host.editor_state.editor_ui.enter_chat_tab();
    let after = host.ai_chat_rect(1200.0, 800.0).expect("conversation rail");
    assert_eq!(after, host.layers_content_rect(800.0));
    assert!(after.size.y > before.size.y);
    assert!(!host.editor_state.chat.maximized);
    host.editor_state.editor_ui.sidebar_open = false;
    assert!(host.editor_state.editor_ui.chat_composer_only());
    assert!(host.ai_chat_rect(1200.0, 800.0).is_some());
}

#[test]
fn vscode_embed_has_no_chat_rect() {
    let mut host = WidgetHost::new();
    host.editor_state.editor_ui.entry_surface = op_editor_core::EntrySurface::Canvas;
    host.editor_state.editor_ui.embed = op_editor_core::EmbedHost::VsCode;
    assert!(host.ai_chat_rect(1600.0, 1000.0).is_none());
}

#[test]
fn ai_chat_collapse_click_stays_expanded_while_streaming_like_ts() {
    let mut host = WidgetHost::new();
    host.editor_state.editor_ui.entry_surface = op_editor_core::EntrySurface::Canvas;
    host.editor_state
        .chat
        .messages
        .push(op_editor_core::ChatMessage::assistant_streaming());
    let viewport_w = 1200.0;
    let viewport_h = 800.0;
    let rect = host
        .ai_chat_rect(viewport_w, viewport_h)
        .expect("chat panel visible");

    assert!(host.apply_click(
        rect.origin.x + 18.0,
        rect.origin.y + 16.0,
        viewport_w,
        viewport_h
    ));

    assert!(
        !host.editor_state.chat.is_minimized(),
        "TS immediately reopens a minimized chat while a response is streaming"
    );
}
