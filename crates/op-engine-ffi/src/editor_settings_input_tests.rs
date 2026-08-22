//! Mobile-shell reproduction of the "API key cannot be deleted" report,
//! driven through the real FFI surface the iOS/Android shells call:
//! `op_editor_press` / `op_editor_release` for the tap on the API-key
//! field, `op_editor_text` for typed characters, and
//! `op_editor_key(KEY_BACKSPACE)` for the system keyboard's delete key.

use crate::desc::{Callbacks, CreateOptions};
use crate::editor::{
    op_editor_key, op_editor_press, op_editor_release, op_editor_text, KEY_BACKSPACE, KEY_DELETE,
};
use crate::editor_ime::{op_editor_ime_commit, op_editor_ime_preedit};
use crate::lifecycle::{OpEngine, Session};
use crate::OpStatus;
use op_editor_core::agent_settings::{BuiltinAgentField, SettingsFocus};

const SAMPLE_DOC: &str =
    include_str!("../../op-editor-core/assets/scene_templates/daily-sign-card.op");

const PHONE_W: f32 = 390.0;
const PHONE_H: f32 = 844.0;

fn phone_engine() -> OpEngine {
    OpEngine::new(
        Session::new(CreateOptions {
            document: SAMPLE_DOC.to_owned(),
            width: PHONE_W,
            height: PHONE_H,
            dpr: 1.0,
            callbacks: Callbacks::default(),
            asset_base: None,
            editor_mode: true,
        })
        .expect("editor session"),
    )
}

fn settings_input_text(engine: &mut OpEngine) -> String {
    engine
        .session_mut_for_test()
        .editor_mut()
        .unwrap()
        .editor_state()
        .editor_ui
        .settings_input
        .text()
        .to_owned()
}

fn settings_focus(engine: &mut OpEngine) -> Option<SettingsFocus> {
    engine
        .session_mut_for_test()
        .editor_mut()
        .unwrap()
        .editor_state()
        .editor_ui
        .agent_settings
        .focus
}

/// Reproduce the report end to end: open the settings sheet, expand the
/// provider card, tap the API-key input, type, then delete every
/// character through the shell's backspace key path.
#[test]
fn mobile_api_key_field_typing_and_backspace_round_trip() {
    let mut engine = phone_engine();
    let pointer = &mut engine as *mut OpEngine;

    // Open the settings modal with one saved provider and enter its edit
    // form (the card's edit press focuses DisplayName and expands it).
    {
        let host = engine.session_mut_for_test().editor_mut().unwrap();
        let ui = host.editor_state_mut();
        ui.editor_ui.agent_settings_open = true;
        ui.editor_ui
            .agent_settings
            .add_builtin_agent_with_defaults("Provider", "sk-old", "model-0");
        ui.editor_ui.agent_settings.focus = Some(SettingsFocus::BuiltinAgent {
            index: 0,
            field: BuiltinAgentField::DisplayName,
        });
        op_editor_core::host_ui_transitions::set_settings_input_text(
            &mut ui.editor_ui,
            "Provider".into(),
            0,
        );
    }

    // Locate the API-key input in the expanded card exactly where paint
    // lays it out, then tap it through the real press/release FFI pair.
    let (px, py) = {
        let session = engine.session_mut_for_test();
        let (vw, vh) = session.editor_viewport();
        let host = session.editor_mut().unwrap();
        let restore = host.editor_state().editor_ui.agent_settings.focus;
        host.editor_state_mut().editor_ui.agent_settings.focus =
            Some(SettingsFocus::BuiltinAgent {
                index: 0,
                field: BuiltinAgentField::ApiKey,
            });
        // No keyboard occlusion in this fixture, so the panel rect matches
        // the host's keyboard-aware geometry exactly.
        let panel = op_editor_ui::widgets::agent_settings_panel::AgentSettingsPanel::for_editor_at(
            host.editor_state(),
            0,
        );
        let panel_rect = panel.rect(vw, vh);
        let mut input = panel
            .focused_input_rect(panel_rect)
            .expect("expanded card exposes the api-key input");
        input.origin.y -= panel.effective_scroll(panel_rect);
        let point = (input.origin.x + 10.0, input.origin.y + input.size.y / 2.0);
        drop(panel);
        host.editor_state_mut().editor_ui.agent_settings.focus = restore;
        point
    };
    assert_eq!(unsafe { op_editor_press(pointer, px, py) }, OpStatus::Ok);
    assert_eq!(unsafe { op_editor_release(pointer, px, py) }, OpStatus::Ok);
    assert_eq!(
        settings_focus(&mut engine),
        Some(SettingsFocus::BuiltinAgent {
            index: 0,
            field: BuiltinAgentField::ApiKey,
        }),
        "tap must focus the api-key input"
    );
    assert_eq!(settings_input_text(&mut engine), "sk-old");

    // Type through the shell text path.
    let typed = "xy";
    assert_eq!(
        unsafe { op_editor_text(pointer, typed.as_ptr(), typed.len()) },
        OpStatus::Ok
    );
    assert_eq!(settings_input_text(&mut engine), "sk-oldxy");

    // Delete through the shell key path — one char per backspace.
    for expected in ["sk-oldx", "sk-old", "sk-ol", "sk-o", "sk-", "sk", "s", ""] {
        assert_eq!(
            unsafe { op_editor_key(pointer, KEY_BACKSPACE) },
            OpStatus::Ok
        );
        assert_eq!(settings_input_text(&mut engine), expected);
    }

    // Close the settings modal; the emptied draft must commit as empty.
    {
        let host = engine.session_mut_for_test().editor_mut().unwrap();
        assert!(host.apply_toggle_agent_settings());
        assert_eq!(
            host.editor_state().editor_ui.agent_settings.builtin_agents[0].api_key,
            "",
            "an emptied api-key draft must commit as empty (the key is deletable)"
        );
    }
}

/// The iOS shell forwards a backspace that lands mid-text in its IME
/// conduit as `KEY_DELETE`. While a settings field is focused that key
/// must forward-delete in the field — it used to fall through the host
/// ladder and silently delete the selected canvas node behind the modal.
#[test]
fn mobile_key_delete_edits_settings_field_and_never_deletes_nodes() {
    use op_editor_core::PenNodeExt;

    let mut engine = phone_engine();
    let pointer = &mut engine as *mut OpEngine;
    let (node_count, first) = {
        let host = engine.session_mut_for_test().editor_mut().unwrap();
        let children = host.editor_state().active_children();
        (children.len(), children[0].base().id.clone())
    };
    {
        let host = engine.session_mut_for_test().editor_mut().unwrap();
        let state = host.editor_state_mut();
        state.set_single_selection(op_editor_core::NodeId::new(first));
        state.editor_ui.agent_settings_open = true;
        state
            .editor_ui
            .agent_settings
            .add_builtin_agent_with_defaults("Provider", "sk-old", "model-0");
        state.editor_ui.agent_settings.focus = Some(SettingsFocus::BuiltinAgent {
            index: 0,
            field: BuiltinAgentField::ApiKey,
        });
        op_editor_core::host_ui_transitions::set_settings_input_text(
            &mut state.editor_ui,
            "sk-old".into(),
            0,
        );
        // Caret to the start so forward deletion has a visible effect.
        for _ in 0.."sk-old".len() {
            assert!(host.apply_settings_caret(false));
        }
    }

    assert_eq!(unsafe { op_editor_key(pointer, KEY_DELETE) }, OpStatus::Ok);
    assert_eq!(settings_input_text(&mut engine), "k-old");
    let host = engine.session_mut_for_test().editor_mut().unwrap();
    assert_eq!(
        host.editor_state().active_children().len(),
        node_count,
        "KEY_DELETE in a settings field must never remove canvas nodes"
    );
}

/// Focus a settings field with `text` seeded into the shared draft, the
/// way the modal's edit press does it on device.
fn focus_settings_field(engine: &mut OpEngine, field: BuiltinAgentField, text: &str) {
    let host = engine.session_mut_for_test().editor_mut().unwrap();
    let state = host.editor_state_mut();
    state.editor_ui.agent_settings_open = true;
    state
        .editor_ui
        .agent_settings
        .add_builtin_agent_with_defaults("Provider", "sk-old", "model-0");
    state.editor_ui.agent_settings.focus = Some(SettingsFocus::BuiltinAgent { index: 0, field });
    op_editor_core::host_ui_transitions::set_settings_input_text(
        &mut state.editor_ui,
        text.into(),
        0,
    );
}

fn ime_preedit(pointer: *mut OpEngine, text: &str) {
    let sel = text.len();
    assert_eq!(
        unsafe { op_editor_ime_preedit(pointer, text.as_ptr(), text.len(), sel, sel) },
        OpStatus::Ok
    );
}

fn ime_commit(pointer: *mut OpEngine, text: &str) {
    assert_eq!(
        unsafe { op_editor_ime_commit(pointer, text.as_ptr(), text.len()) },
        OpStatus::Ok
    );
}

/// The exact iOS Chinese-IME sequence: preedit updates stream while the
/// user types pinyin, the chosen candidate lands as an IME commit, and a
/// following backspace key must delete exactly the committed character.
/// Green here means the engine's IME path into settings inputs is healthy
/// and the on-device "cannot delete" defect lives in the platform bridge.
#[test]
fn mobile_ime_preedit_commit_then_backspace_deletes_in_settings_field() {
    let mut engine = phone_engine();
    let pointer = &mut engine as *mut OpEngine;
    focus_settings_field(&mut engine, BuiltinAgentField::DisplayName, "Provider");

    ime_preedit(pointer, "s");
    ime_preedit(pointer, "si");
    assert_eq!(
        settings_input_text(&mut engine),
        "Provider",
        "preedit must not mutate the settings draft"
    );

    ime_commit(pointer, "四");
    assert_eq!(settings_input_text(&mut engine), "Provider四");

    assert_eq!(
        unsafe { op_editor_key(pointer, KEY_BACKSPACE) },
        OpStatus::Ok
    );
    assert_eq!(
        settings_input_text(&mut engine),
        "Provider",
        "one backspace after an IME commit must delete exactly one char"
    );
}

/// Interleaved shell-text and IME-commit input into the API-key field —
/// the way a Chinese keyboard mixes its ASCII passthrough with candidate
/// commits — followed by a full backspace teardown.
#[test]
fn mobile_interleaved_text_and_ime_commit_backspace_teardown() {
    let mut engine = phone_engine();
    let pointer = &mut engine as *mut OpEngine;
    focus_settings_field(&mut engine, BuiltinAgentField::ApiKey, "");

    let typed = "sk";
    assert_eq!(
        unsafe { op_editor_text(pointer, typed.as_ptr(), typed.len()) },
        OpStatus::Ok
    );
    ime_preedit(pointer, "si");
    ime_commit(pointer, "四");
    let typed = "x";
    assert_eq!(
        unsafe { op_editor_text(pointer, typed.as_ptr(), typed.len()) },
        OpStatus::Ok
    );
    assert_eq!(settings_input_text(&mut engine), "sk四x");

    for expected in ["sk四", "sk", "s", "", ""] {
        assert_eq!(
            unsafe { op_editor_key(pointer, KEY_BACKSPACE) },
            OpStatus::Ok
        );
        assert_eq!(settings_input_text(&mut engine), expected);
    }
}

/// A cancelled composition arrives as an empty IME commit (iOS) or an
/// empty preedit (Android). Neither may disturb the draft, and backspace
/// must keep working afterwards.
#[test]
fn mobile_cancelled_composition_leaves_settings_backspace_healthy() {
    let mut engine = phone_engine();
    let pointer = &mut engine as *mut OpEngine;
    focus_settings_field(&mut engine, BuiltinAgentField::DisplayName, "Provider");

    ime_preedit(pointer, "si");
    ime_commit(pointer, "");
    assert_eq!(settings_input_text(&mut engine), "Provider");

    ime_preedit(pointer, "si");
    ime_preedit(pointer, "");
    assert_eq!(settings_input_text(&mut engine), "Provider");

    assert_eq!(
        unsafe { op_editor_key(pointer, KEY_BACKSPACE) },
        OpStatus::Ok
    );
    assert_eq!(settings_input_text(&mut engine), "Provide");
}
