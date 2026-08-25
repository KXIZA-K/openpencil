# Preview Interact Tab and Debugger Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the existing experimental onTap summary into a responsive, undoable, forward-compatible interaction editor and bounded Preview Debugger, with AI and MCP authoring the identical schema.

**Architecture:** Typed owner/trigger commands and a lossless ActionBuilder live in `op-editor-core`. Platform-free widgets consume Jian action metadata plus DTOs from the leaf `op-preview-contracts` crate; hosts map PreviewSession data into those DTOs without creating a Core/UI dependency cycle. AI and MCP invoke the same builder and commands and never maintain a second action vocabulary.

**Tech Stack:** Rust, OpenPencil EditorState/EditorCommand, Jian action metadata, op-editor-ui RenderBackend/accesskit, op-mcp/op-chat-agent/op-host-services, op-i18n.

---

### Task E1: Lossless Interaction Owners, Triggers, and Commands

**Files:**
- Create: `crates/op-editor-core/src/interactions.rs`
- Create: `crates/op-editor-core/src/interaction_command_tests.rs`
- Modify: `crates/op-editor-core/src/lib.rs`
- Modify: `crates/op-editor-core/src/command.rs`
- Modify: `crates/op-editor-core/src/command_apply.rs`
- Create: `crates/op-editor-core/src/command_apply/interactions.rs`
- Modify: `crates/op-editor-core/src/command_batch.rs`
- Modify: `crates/op-editor-core/src/collab_gate.rs`

- [ ] **Step 1: Write failing command and round-trip tests**

Build App, Page, and Node fixtures containing known handlers, lifecycle hooks, `disabledEvents`, `interactionOrder`, unknown triggers, unknown actions, and known action bodies with future fields. Edit one known action, reorder/disable/duplicate/delete it, then save/reload.

```rust
let owner = InteractionOwner::Node {
    page_id: "page-1".to_owned(),
    node_id: NodeId::new("button"),
};
let command = EditorCommand::SetInteractionActions {
    owner: owner.clone(),
    trigger: InteractionTrigger::Tap,
    actions: Some(vec![action("pop", serde_json::Value::Null)]),
};
assert!(state.apply(command));
assert_eq!(events_of(&state, "button").on_change, original.on_change);
assert_eq!(events_of(&state, "button").extra["onFutureGesture"], original.extra["onFutureGesture"]);
```

Assert one successful command creates one undo entry; redo restores it exactly. A multi-selection “clear common interactions” command removes only triggers present on every selected node, in one undo step, and never broadcasts replacement ActionLists.

- [ ] **Step 2: Verify RED**

```bash
cargo test -p op-editor-core interaction_command_tests
```

Expected: FAIL because only shallow `PatchNodeData` exists.

- [ ] **Step 3: Add typed owners, triggers, and commands**

```rust
pub enum InteractionOwner {
    App,
    Page { page_id: String },
    Node { page_id: String, node_id: NodeId },
}

pub enum InteractionTrigger {
    Tap, DoubleTap, LongPress, PressStart, PressEnd, PressCancel,
    PanStart, PanUpdate, PanEnd, Swipe,
    ScaleStart, ScaleUpdate, ScaleEnd,
    RotateStart, RotateUpdate, RotateEnd,
    HoverEnter, HoverLeave, ContextMenu,
    Change, Submit, Focus, Blur, Key, Scroll, ReachEnd,
    AppLaunch, AppResume, AppBackground, AppTerminate,
    PageEnter, PageLeave, PageForeground, PageBackground,
    NodeMount, NodeUnmount,
    Unknown(String),
}
```

Add `SetInteractionActions`, `MoveInteractionAction`, `SetInteractionEnabled`, `SetInteractionOrder`, and `ClearCommonNodeInteractions`. Known commands update one owner/trigger plus its metadata and preserve all sibling/unknown JSON. `Unknown(String)` is readable but typed mutation returns `InteractionCommandError::UnknownReadOnly`. Commands include page identity as `String`, participate in batch/exhaustive matches, pass collaboration gating, and reject duplicate/out-of-range order entries.

Expose a typed `EditorState::apply_interaction_command(...) -> Result<bool, InteractionCommandError>`; the existing `EditorState::apply() -> bool` delegates and maps errors to `false` for compatibility. The exactly-800-line `command_apply.rs` only delegates; all new apply logic lives in `command_apply/interactions.rs`, and the spine must not grow. Only the core helper records history. UI/host dispatch must not call an extra history commit, which the tests pin as one undo entry.

- [ ] **Step 4: Run and commit**

```bash
cargo test -p op-editor-core interaction_command_tests
git add crates/op-editor-core/src/interactions.rs crates/op-editor-core/src/interaction_command_tests.rs crates/op-editor-core/src/lib.rs crates/op-editor-core/src/command.rs crates/op-editor-core/src/command_apply.rs crates/op-editor-core/src/command_apply/interactions.rs crates/op-editor-core/src/command_batch.rs crates/op-editor-core/src/collab_gate.rs
git commit -m "feat(editor): add typed interaction commands"
```

### Task E2: Shared Lossless ActionBuilder and Validation

**Files:**
- Create: `crates/op-editor-core/src/interaction_builder.rs`
- Create: `crates/op-editor-core/src/interaction_builder_tests.rs`
- Modify: `crates/op-editor-core/src/lib.rs`

- [ ] **Step 1: Write failing builder tests**

Cover every `preview_action_descriptors()` entry from R5, unsafe legacy actions, writable/read-only variable scopes, target and route existence, URL schemes, condition expressions, animation values, capability diagnostics, and nested action lists. Parse and edit a known action that contains `futureCurve` and prove the field survives.

```rust
let mut draft = ActionDraft::from_action(json_action({
    "animate": {
        "target": "$self",
        "to": {"opacity": 1},
        "durationMs": 180,
        "futureCurve": "spring-v2"
    }
}))?;
draft.set_known_field("durationMs", json!(240))?;
assert_eq!(build_action(&draft)?.body()["futureCurve"], "spring-v2");
```

Unknown actions remain `ActionDraft::Unknown { original }` and are returned byte-semantically unchanged; Save is disabled until the user deliberately replaces them with a known action.

- [ ] **Step 2: Verify RED**

```bash
cargo test -p op-editor-core interaction_builder_tests
```

Expected: FAIL because no shared builder/validator exists.

- [ ] **Step 3: Implement one descriptor-driven builder**

```rust
pub enum ActionDraft {
    Known(KnownActionDraft),
    Unknown { original: jian_ops_schema::events::Action },
}

pub struct KnownActionDraft {
    pub name: String,
    pub original_body: serde_json::Value,
    pub body: KnownActionBodyDraft,
}

pub enum KnownActionBodyDraft {
    Null,
    Scalar(serde_json::Value),
    Object {
        known_fields: BTreeMap<String, serde_json::Value>,
        extra_fields: BTreeMap<String, serde_json::Value>,
    },
    NestedLists(serde_json::Value),
}

pub fn validate_action(
    action: &jian_ops_schema::events::Action,
    context: &InteractionContext,
) -> Vec<InteractionDiagnostic>;

pub fn build_action(
    draft: &ActionDraft,
) -> Result<jian_ops_schema::events::Action, ActionBuildError>;
```

Use Jian `ActionDescriptor` metadata as the vocabulary. The shape-specific draft must represent null, scalar/string, object, and nested action-list bodies; merge edits into `original_body` so future body fields survive. Validate only proven facts: target/route ids, writable `$app/$page/$state/$self` paths (R5 makes `$state` an explicit alias of `$app`), read-only `$event`, finite ranges, safe URL schemes, and Preview policy/capability requirements. Never strip an action merely because the current host lacks a capability.

- [ ] **Step 4: Run and commit**

```bash
cargo test -p op-editor-core interaction_builder_tests
git add crates/op-editor-core/src/interaction_builder.rs crates/op-editor-core/src/interaction_builder_tests.rs crates/op-editor-core/src/lib.rs
git commit -m "feat(editor): add shared interaction action builder"
```

### Task E3: Interact Tab Snapshot, Cards, and Undoable List Operations

**Files:**
- Modify: `crates/op-editor-ui/Cargo.toml`
- Modify: `Cargo.lock`
- Modify: `crates/op-editor-ui/src/widgets/property_panel_interactions.rs`
- Create: `crates/op-editor-ui/src/widgets/property_panel_interact_cards.rs`
- Create: `crates/op-editor-ui/src/widgets/property_panel_interact_layout.rs`
- Modify: `crates/op-editor-ui/src/widgets/property_panel_snapshot.rs`
- Modify: `crates/op-editor-ui/src/widgets/property_panel_snapshot/build.rs`
- Modify: `crates/op-editor-ui/src/widgets/property_panel/build.rs`
- Modify: `crates/op-editor-ui/src/widgets/property_panel/paint.rs`
- Modify: `crates/op-editor-ui/src/widgets/property_panel/hit.rs`
- Modify: `crates/op-editor-ui/src/widgets/property_panel_action.rs`
- Modify: `crates/op-editor-ui/src/widgets/property_panel_dispatch.rs`
- Modify: `crates/op-editor-ui/src/widgets/property_panel_collab.rs`
- Modify: `crates/op-editor-ui/src/widgets/property_panel_interactions_tests.rs`
- Modify: `crates/op-host-native/src/widget_host/property_panel_interactions_tests.rs`
- Create: `crates/op-host-web/tests/property_panel_interact_contract.rs`

- [ ] **Step 1: Write failing card/layout/dispatch tests**

Assert one card per trigger in explicit `interactionOrder`; handlers absent from metadata append in shared catalog order. Add Interaction, edit, move, duplicate, enable/disable, and delete hit regions must match paint geometry and emit exactly one E1 command. Unknown trigger/action cards render an “advanced/unknown” read-only row without data loss. Multi-select shows summary plus Clear Common only. Desktop/Web/iPad retain the property side rail; phone presents the same Interact snapshot/actions in a full-height sheet with safe-area padding and 44pt controls.

Add an owner selector: selected node defaults to Node, active page without node defaults to Page, and App is always selectable. This makes App/Page lifecycle hooks reachable without inventing a fake node selection.

- [ ] **Step 2: Verify RED**

```bash
cargo test -p op-editor-ui property_panel_interaction
```

Expected: FAIL because Interact still renders the Design snapshot and summarizes only onTap.

- [ ] **Step 3: Build the shared snapshot and card walker**

```rust
pub struct InteractionCardSnapshot {
    pub trigger: InteractionTrigger,
    pub enabled: bool,
    pub actions: Vec<ActionRowSnapshot>,
    pub platform: PlatformAvailability,
    pub diagnostics: Vec<InteractionDiagnostic>,
    pub read_only_unknown: bool,
}

pub struct InteractSnapshot {
    pub owner: InteractionOwner,
    pub cards: Vec<InteractionCardSnapshot>,
    pub routes: Vec<RouteOption>,
    pub variables: Vec<VariableOption>,
    pub targets: Vec<TargetOption>,
}
```

Use one layout walker for paint and hit testing. Shared dispatch converts every action into an E1 command; remove Interact's raw `PatchNodeData` path and update collaboration/exhaustive matches.

- [ ] **Step 4: Run and commit**

```bash
cargo test -p op-editor-ui property_panel_interaction
cargo test -p op-host-native --features gl-host property_panel_interact
cargo test -p op-host-web --features canvaskit --test property_panel_interact_contract
git add Cargo.lock crates/op-editor-ui/Cargo.toml crates/op-editor-ui/src/widgets/property_panel_interactions.rs crates/op-editor-ui/src/widgets/property_panel_interact_cards.rs crates/op-editor-ui/src/widgets/property_panel_interact_layout.rs crates/op-editor-ui/src/widgets/property_panel_snapshot.rs crates/op-editor-ui/src/widgets/property_panel_snapshot/build.rs crates/op-editor-ui/src/widgets/property_panel/paint.rs crates/op-editor-ui/src/widgets/property_panel/hit.rs crates/op-editor-ui/src/widgets/property_panel_action.rs crates/op-editor-ui/src/widgets/property_panel_dispatch.rs crates/op-editor-ui/src/widgets/property_panel_collab.rs crates/op-editor-ui/src/widgets/property_panel_interactions_tests.rs crates/op-host-native/src/widget_host/property_panel_interactions_tests.rs crates/op-host-web/tests/property_panel_interact_contract.rs
git commit -m "feat(panels): build the interaction card editor"
```

### Task E4: Trigger and Action Pickers

**Files:**
- Create: `crates/op-editor-ui/src/widgets/interact_trigger_picker.rs`
- Create: `crates/op-editor-ui/src/widgets/interact_action_picker.rs`
- Create: `crates/op-editor-ui/src/widgets/interact_action_form.rs`
- Create: `crates/op-editor-core/src/editor_ui_state/interactions.rs`
- Modify: `crates/op-editor-core/src/editor_ui_state.rs`
- Modify: `crates/op-editor-ui/src/widgets/mod.rs`
- Modify: `crates/op-editor-ui/src/widgets/property_panel/build.rs`
- Modify: `crates/op-editor-ui/src/widgets/property_panel/paint.rs`
- Modify: `crates/op-editor-ui/src/widgets/property_panel/hit.rs`
- Modify: `crates/op-editor-ui/src/widgets/property_panel_action.rs`
- Modify: `crates/op-editor-ui/src/widgets/property_panel_dispatch.rs`
- Create: `crates/op-editor-ui/src/widgets/interact_picker_tests.rs`

- [ ] **Step 1: Write failing picker tests**

Assert every event/lifecycle trigger and Preview-authorable action appears exactly once in catalog order; unsafe actions never appear. Assert search/filter, 44pt minimum targets, keyboard traversal, Escape/outside dismissal, UTF-8 input, platform/capability badges, and read-only unknown rows. For nested actions, assert add/edit/reorder/delete in `if.then/else`, `parallel.actions`, and `confirm.onConfirm/onCancel`; every nested row uses the same descriptor picker and one Save produces one parent ActionDraft update.

- [ ] **Step 2: Verify RED**

```bash
cargo test -p op-editor-ui interact_picker_tests
```

Expected: FAIL because the pickers do not exist.

- [ ] **Step 3: Implement descriptor-driven pickers**

Trigger rows come from the shared trigger catalog and are filtered by `InteractionOwner`. Action rows come only from Jian `preview_action_descriptors()`. The form edits `ActionDraft`; only Save calls E2 `build_action`. Its recursive ActionList editor handles `if`, `parallel`, and `confirm` branches with bounded nesting diagnostics. Put open/search/hover/draft state in `editor_ui_state/interactions.rs`, wire card actions through PropertyPanel build/paint/hit/dispatch, and consume the same row rects for paint and hit testing.

- [ ] **Step 4: Run and commit**

```bash
cargo test -p op-editor-ui interact_picker_tests
git add crates/op-editor-ui/src/widgets/interact_trigger_picker.rs crates/op-editor-ui/src/widgets/interact_action_picker.rs crates/op-editor-ui/src/widgets/interact_action_form.rs crates/op-editor-ui/src/widgets/interact_picker_tests.rs crates/op-editor-ui/src/widgets/mod.rs crates/op-editor-ui/src/widgets/property_panel/build.rs crates/op-editor-ui/src/widgets/property_panel/paint.rs crates/op-editor-ui/src/widgets/property_panel/hit.rs crates/op-editor-ui/src/widgets/property_panel_action.rs crates/op-editor-ui/src/widgets/property_panel_dispatch.rs crates/op-editor-core/src/editor_ui_state.rs crates/op-editor-core/src/editor_ui_state/interactions.rs
git commit -m "feat(panels): add interaction trigger and action pickers"
```

### Task E5: Target, Route, and Variable Pickers

**Files:**
- Create: `crates/op-editor-ui/src/widgets/interact_target_picker.rs`
- Create: `crates/op-editor-ui/src/widgets/interact_route_picker.rs`
- Create: `crates/op-editor-ui/src/widgets/interact_variable_picker.rs`
- Create: `crates/op-editor-ui/src/widgets/interact_reference_picker_tests.rs`
- Modify: `crates/op-editor-core/src/editor_ui_state/interactions.rs`
- Modify: `crates/op-editor-core/src/editor_ui_state.rs`
- Modify: `crates/op-editor-ui/src/widgets/mod.rs`
- Modify: `crates/op-editor-ui/src/widgets/property_panel/build.rs`
- Modify: `crates/op-editor-ui/src/widgets/property_panel/paint.rs`
- Modify: `crates/op-editor-ui/src/widgets/property_panel/hit.rs`
- Modify: `crates/op-editor-ui/src/widgets/property_panel_action.rs`
- Modify: `crates/op-editor-ui/src/widgets/property_panel_dispatch.rs`
- Modify: `crates/op-host-native/src/widget_host/press.rs`
- Modify: `crates/op-host-web/src/widget_host/press.rs`
- Modify: `crates/op-host-native/src/widget_host/property_panel_interactions_tests.rs`
- Modify: `crates/op-host-web/tests/property_panel_interact_contract.rs`

- [ ] **Step 1: Write failing reference-picker tests**

Assert target selection from canvas/layer rows, route selection from existing screens, scoped variable grouping, read-only `$event`, writable scope rejection, missing-reference diagnostics, 44pt targets, keyboard traversal, Escape/outside dismissal, and Phone/Tablet/Desktop geometry.

- [ ] **Step 2: Verify RED**

```bash
cargo test -p op-editor-ui interact_reference_picker_tests
```

Expected: FAIL because structured reference pickers and their UI state do not exist.

- [ ] **Step 3: Implement focused picker state and widgets**

Extend `editor_ui_state/interactions.rs`; keep the 768-line spine limited to module declarations/re-exports so it stays below 800 lines. Pickers receive immutable `TargetOption`, `RouteOption`, and `VariableOption` snapshots and return an updated field to `ActionDraft`. Wire them through PropertyPanel paint/hit/dispatch. Target Picker enters a shared canvas/layer pick mode; native and web press paths consume exactly one eligible node click, update the draft, and cancel on Escape/outside close without changing document selection.

- [ ] **Step 4: Run and commit**

```bash
cargo test -p op-editor-ui interact_reference_picker_tests
cargo test -p op-host-native --features gl-host property_panel_interactions
cargo test -p op-host-web --features canvaskit --test property_panel_interact_contract
git add crates/op-editor-ui/src/widgets/interact_target_picker.rs crates/op-editor-ui/src/widgets/interact_route_picker.rs crates/op-editor-ui/src/widgets/interact_variable_picker.rs crates/op-editor-ui/src/widgets/interact_reference_picker_tests.rs crates/op-editor-ui/src/widgets/mod.rs crates/op-editor-ui/src/widgets/property_panel/build.rs crates/op-editor-ui/src/widgets/property_panel/paint.rs crates/op-editor-ui/src/widgets/property_panel/hit.rs crates/op-editor-ui/src/widgets/property_panel_action.rs crates/op-editor-ui/src/widgets/property_panel_dispatch.rs crates/op-editor-core/src/editor_ui_state.rs crates/op-editor-core/src/editor_ui_state/interactions.rs crates/op-host-native/src/widget_host/press.rs crates/op-host-web/src/widget_host/press.rs crates/op-host-native/src/widget_host/property_panel_interactions_tests.rs crates/op-host-web/tests/property_panel_interact_contract.rs
git commit -m "feat(panels): add interaction reference pickers"
```

### Task E6: Condition and Animation Editors

**Files:**
- Create: `crates/op-editor-ui/src/widgets/interact_condition_editor.rs`
- Create: `crates/op-editor-ui/src/widgets/interact_animation_editor.rs`
- Create: `crates/op-editor-ui/src/widgets/interact_advanced_editor_tests.rs`
- Modify: `crates/op-editor-ui/src/widgets/mod.rs`
- Modify: `crates/op-editor-core/src/editor_ui_state/interactions.rs`
- Modify: `crates/op-editor-ui/src/widgets/property_panel/build.rs`
- Modify: `crates/op-editor-ui/src/widgets/property_panel/paint.rs`
- Modify: `crates/op-editor-ui/src/widgets/property_panel/hit.rs`
- Modify: `crates/op-editor-ui/src/widgets/property_panel_action.rs`
- Modify: `crates/op-editor-ui/src/widgets/property_panel_dispatch.rs`

- [ ] **Step 1: Write failing structured-editor tests**

Condition tests cover field/operator/value rows, nested all/any, expression-mode round-trip, invalid expression diagnostics, and UTF-8. Animation tests cover property, target, from/to, duration, delay, easing, iterations, direction, fill mode, current-value sampling, and the width/height discrete-layout warning.

- [ ] **Step 2: Verify RED**

```bash
cargo test -p op-editor-ui interact_advanced_editor_tests
```

Expected: FAIL because the editors do not exist.

- [ ] **Step 3: Implement editors over ActionDraft**

Both widgets are pure view-model/layout/paint/hit components and never write document JSON directly. Condition expression mode preserves the last structured draft until Save. Animation fields use R7 descriptors/ranges and emit capability/performance diagnostics without inventing platform-specific semantics. Wire open/edit/save/cancel actions through the same PropertyPanel paths and state module as E4/E5.

- [ ] **Step 4: Run and commit**

```bash
cargo test -p op-editor-ui interact_advanced_editor_tests
git add crates/op-editor-ui/src/widgets/interact_condition_editor.rs crates/op-editor-ui/src/widgets/interact_animation_editor.rs crates/op-editor-ui/src/widgets/interact_advanced_editor_tests.rs crates/op-editor-ui/src/widgets/mod.rs crates/op-editor-core/src/editor_ui_state/interactions.rs crates/op-editor-ui/src/widgets/property_panel/build.rs crates/op-editor-ui/src/widgets/property_panel/paint.rs crates/op-editor-ui/src/widgets/property_panel/hit.rs crates/op-editor-ui/src/widgets/property_panel_action.rs crates/op-editor-ui/src/widgets/property_panel_dispatch.rs
git commit -m "feat(panels): add condition and animation editors"
```

### Task E7: Responsive Preview Debugger

**Files:**
- Create: `crates/op-editor-ui/src/widgets/preview_debugger.rs`
- Create: `crates/op-editor-ui/src/widgets/preview_debugger_layout.rs`
- Create: `crates/op-editor-ui/src/widgets/preview_debugger_tests.rs`
- Modify: `crates/op-editor-ui/src/widgets/mod.rs`
- Create: `crates/op-editor-core/src/editor_ui_state/preview_debugger.rs`
- Modify: `crates/op-editor-core/src/editor_ui_state.rs`
- Modify: `crates/op-host-native/src/widget_host/paint.rs`
- Modify: `crates/op-host-native/src/widget_host/press.rs`
- Create: `crates/op-host-native/src/widget_host/preview_debugger_host.rs`
- Modify: `crates/op-host-native/src/widget_host/keyboard.rs`
- Modify: `crates/op-host-native/src/widget_host/shortcuts.rs`
- Modify: `crates/op-host-native/src/widget_host/preview_slideshow.rs`
- Modify: `crates/op-host-native/src/widget_host/preview_slideshow_tests.rs`
- Modify: `crates/op-host-web/src/widget_host/paint.rs`
- Modify: `crates/op-host-web/src/widget_host/press.rs`
- Create: `crates/op-host-web/src/widget_host/preview_debugger_host.rs`
- Modify: `crates/op-host-web/src/widget_host/keyboard.rs`
- Modify: `crates/op-host-web/src/widget_host/preview_slideshow.rs`

- [ ] **Step 1: Write failing view/control tests**

Assert Desktop/Web/iPad side rail and Phone bottom sheet geometry, Run/Pause/Reset, route/focus/capture/gesture summaries, 256-row event flow, scoped state plus last-writer source, redacted effects, structured diagnostics, node highlight, scroll bounds, and 44pt controls. Assert Preview chrome exposes a Debug toggle (phone: More menu item) and Cmd/Ctrl+Alt+D opens/closes it; Debugger remains closed by default. Add a shortcut-table conflict test so the existing Cmd/Ctrl+Shift+D Design-MD command remains unchanged.

- [ ] **Step 2: Verify RED**

```bash
cargo test -p op-editor-ui preview_debugger
```

Expected: FAIL because the Debugger surface does not exist.

- [ ] **Step 3: Implement the dependency-safe Debugger widget**

The widget depends on leaf `op-preview-contracts`, never `op-preview-core`. It consumes `PreviewDebugSnapshot` and `&[PreviewTraceEntry]`, then emits `PreviewDebuggerAction::{Run, Pause, Reset, SelectTrace, Close}`. Put persistent chrome state in `editor_ui_state/preview_debugger.rs` so the spine remains below 800 lines.

Native/Web map actions to R9 session controls. SelectTrace highlights a preview target without mutating the document. Add a shared Preview Debug entry in slideshow chrome plus the keyboard shortcut. Register the new host sibling beneath the existing Preview/slideshow module so the 798-line native spine does not grow. Move the existing Preview paint block plus new Debugger paint/hit code out of Web's 911-line `paint.rs` into `preview_debugger_host.rs`, leaving `paint.rs` below 800 lines. The shared viewport classifier selects side rail for desktop/web/tablet and a bottom sheet for phones; FFI clients render this same widget once H2-H4 supply input.

- [ ] **Step 4: Run and commit**

```bash
cargo test -p op-editor-ui preview_debugger
cargo test -p op-host-native --features gl-host preview_debugger
cargo test -p op-host-web --features canvaskit preview_debugger
git add crates/op-editor-ui/src/widgets/preview_debugger.rs crates/op-editor-ui/src/widgets/preview_debugger_layout.rs crates/op-editor-ui/src/widgets/preview_debugger_tests.rs crates/op-editor-ui/src/widgets/mod.rs crates/op-editor-core/src/editor_ui_state.rs crates/op-editor-core/src/editor_ui_state/preview_debugger.rs crates/op-host-native/src/widget_host/paint.rs crates/op-host-native/src/widget_host/press.rs crates/op-host-native/src/widget_host/preview_debugger_host.rs crates/op-host-native/src/widget_host/keyboard.rs crates/op-host-native/src/widget_host/shortcuts.rs crates/op-host-native/src/widget_host/preview_slideshow.rs crates/op-host-native/src/widget_host/preview_slideshow_tests.rs crates/op-host-web/src/widget_host/paint.rs crates/op-host-web/src/widget_host/press.rs crates/op-host-web/src/widget_host/preview_debugger_host.rs crates/op-host-web/src/widget_host/keyboard.rs crates/op-host-web/src/widget_host/preview_slideshow.rs
git commit -m "feat(editor): add the interaction debugger"
```

### Task E8: AI and MCP Authoring Parity

**Files:**
- Modify: `crates/op-ai-skills/skills/phases/generation/interactivity.md`
- Create: `crates/op-ai-skills/src/lib_tests.rs`
- Create: `crates/op-ai-skills/src/interactivity_catalog_tests.rs`
- Modify: `crates/op-ai-skills/src/lib.rs`
- Modify: `crates/op-chat-agent/src/tool_schemas.rs`
- Modify: `crates/op-chat-agent/src/design_agent_tools.rs`
- Create: `crates/op-mcp/src/interaction_tools.rs`
- Create: `crates/op-mcp/src/interaction_tools_tests.rs`
- Modify: `crates/op-mcp/src/lib.rs`
- Modify: `crates/op-mcp/src/write_tools.rs`
- Modify: `crates/op-mcp/src/batch_program_interactivity_tests.rs`
- Modify: `crates/op-host-services/src/mcp_serve/registry.rs`
- Modify: `crates/op-host-services/src/mcp_serve/schemas.rs`
- Modify: `crates/op-host-services/src/mcp_serve/tools_list.rs`
- Modify: `crates/op-host-services/src/mcp_serve/tool_profile.rs`
- Modify: `crates/op-host-services/src/mcp_serve/tool_profile_tests.rs`
- Modify: `crates/op-host-services/src/mcp_serve/tests.rs`

- [ ] **Step 1: Write failing public-registry parity tests**

First record `cargo test -p op-ai-skills` green, move the 868-line `lib.rs` inline test module to `lib_tests.rs`, rerun unchanged tests, and leave the spine below 800 lines. Then assert documentation/tool schemas mention only descriptor-backed Preview actions, `toggle` and `animate` are registered before they are advertised, `bind:value` examples use runtime-supported writable scopes, structured tools call E2, and rich batch-program actions preserve future fields. The existing batch interactivity module is already wired; extend its behavior instead of using module registration as the RED.

Assert all new tools appear in MCP schemas, registry, ToolSearch, tool profiles/counts, and design-agent tool selection. `list_interactions` is a read tool allowed to read-only credentials; set/move/remove are write tools and must be denied without write scope.

- [ ] **Step 2: Verify RED**

```bash
cargo test -p op-mcp
cargo test -p op-chat-agent interaction
cargo test -p op-ai-skills preview_interactivity_catalog
cargo test -p op-host-services interaction_tool
```

Expected: FAIL because structured tools and public registrations do not exist and current docs/runtime vocabulary has drifted.

- [ ] **Step 3: Add structured tools through shared commands**

Expose `list_interactions`, `set_interaction`, `move_interaction_action`, and `remove_interaction`. Inputs use `InteractionOwner`, `InteractionTrigger`, and `ActionDraft`; outputs return known/unknown data losslessly. Writes return E1 EditorCommands and are classified as in-memory MCP write tools.

- [ ] **Step 4: Correct AI corpus and register every public surface**

Generate/validate action names from Jian descriptors. Correct scope examples, add normalized event payload examples, and forbid Network/script/system commands. Register the tools in op-mcp, host-service schema/registry/profile/ToolSearch, and chat-agent schemas/tool selection; update pinned counts in the same commit.

- [ ] **Step 5: Run and commit**

```bash
cargo test -p op-mcp
cargo test -p op-chat-agent interaction
cargo test -p op-ai-skills preview_interactivity_catalog
cargo test -p op-host-services interaction_tool
git add crates/op-ai-skills/skills/phases/generation/interactivity.md crates/op-ai-skills/src/lib_tests.rs crates/op-ai-skills/src/interactivity_catalog_tests.rs crates/op-ai-skills/src/lib.rs crates/op-chat-agent/src/tool_schemas.rs crates/op-chat-agent/src/design_agent_tools.rs crates/op-mcp/src/interaction_tools.rs crates/op-mcp/src/interaction_tools_tests.rs crates/op-mcp/src/batch_program_interactivity_tests.rs crates/op-mcp/src/lib.rs crates/op-mcp/src/write_tools.rs crates/op-host-services/src/mcp_serve/registry.rs crates/op-host-services/src/mcp_serve/schemas.rs crates/op-host-services/src/mcp_serve/tools_list.rs crates/op-host-services/src/mcp_serve/tool_profile.rs crates/op-host-services/src/mcp_serve/tool_profile_tests.rs crates/op-host-services/src/mcp_serve/tests.rs
git commit -m "feat(mcp): author preview interactions through shared schema"
```

### Task E9: Localization and Accessibility

**Files:**
- Modify: `crates/op-i18n/src/i18n/*_collab.rs` for all 15 locales
- Modify: `crates/op-i18n/src/i18n/catalog_integrity_tests.rs`
- Create: `crates/op-i18n/src/i18n/preview_interaction_key_tests.rs`
- Modify: `crates/op-i18n/src/i18n/mod.rs`
- Modify: `crates/op-editor-ui/src/accessibility_regions.rs`
- Modify: `crates/op-editor-ui/src/widgets/mod.rs`
- Modify: `crates/op-editor-ui/src/widgets/property_panel_interact_cards.rs`
- Modify: `crates/op-editor-ui/src/widgets/interact_trigger_picker.rs`
- Modify: `crates/op-editor-ui/src/widgets/interact_action_picker.rs`
- Modify: `crates/op-editor-ui/src/widgets/interact_action_form.rs`
- Modify: `crates/op-editor-ui/src/widgets/interact_target_picker.rs`
- Modify: `crates/op-editor-ui/src/widgets/interact_route_picker.rs`
- Modify: `crates/op-editor-ui/src/widgets/interact_variable_picker.rs`
- Modify: `crates/op-editor-ui/src/widgets/interact_condition_editor.rs`
- Modify: `crates/op-editor-ui/src/widgets/interact_animation_editor.rs`
- Modify: `crates/op-editor-ui/src/widgets/preview_debugger.rs`
- Create: `crates/op-editor-ui/src/widgets/preview_interaction_accessibility_tests.rs`
- Modify: `crates/op-host-native/src/widget_host/a11y.rs`
- Modify: `crates/op-host-web/src/widget_host/a11y_bridge.rs`

- [ ] **Step 1: Write failing key/tree/action tests**

Require keys for card controls, every trigger/action, capability badge, field label, validation error, Debugger control/trace kind, and platform diagnostic. Assert identical key/placeholder sets across all locales. Assemble native and web accessibility trees and assert every card, row, picker, field, Debugger control, and trace selection has a stable id, role, localized label, state/value, bounds, and routed action.

- [ ] **Step 2: Verify RED**

```bash
cargo test -p op-i18n preview_interaction_key_tests
cargo test -p op-editor-ui preview_interaction_accessibility
cargo test -p op-host-native --features gl-host accessibility
cargo test -p op-host-web --features canvaskit accessibility
```

Expected: FAIL with missing keys and missing child accessibility nodes.

- [ ] **Step 3: Add locale shards and accessibility wiring**

Add the same keys to every terminal `*_collab.rs`, preserve placeholders, bump catalog count, and register the feature-key test. Implement `access_node()`/child placement for new widgets in `accessibility_regions.rs`; wire the existing native AccessKit and hidden web DOM bridges to the same action router.

- [ ] **Step 4: Run and commit**

```bash
cargo test -p op-i18n
cargo test -p op-editor-ui preview_interaction_accessibility
cargo test -p op-host-native --features gl-host accessibility
cargo test -p op-host-web --features canvaskit accessibility
git add crates/op-i18n/src/i18n/*_collab.rs crates/op-i18n/src/i18n/catalog_integrity_tests.rs crates/op-i18n/src/i18n/preview_interaction_key_tests.rs crates/op-i18n/src/i18n/mod.rs crates/op-editor-ui/src/accessibility_regions.rs crates/op-editor-ui/src/widgets/mod.rs crates/op-editor-ui/src/widgets/property_panel_interact_cards.rs crates/op-editor-ui/src/widgets/interact_trigger_picker.rs crates/op-editor-ui/src/widgets/interact_action_picker.rs crates/op-editor-ui/src/widgets/interact_action_form.rs crates/op-editor-ui/src/widgets/interact_target_picker.rs crates/op-editor-ui/src/widgets/interact_route_picker.rs crates/op-editor-ui/src/widgets/interact_variable_picker.rs crates/op-editor-ui/src/widgets/interact_condition_editor.rs crates/op-editor-ui/src/widgets/interact_animation_editor.rs crates/op-editor-ui/src/widgets/preview_debugger.rs crates/op-editor-ui/src/widgets/preview_interaction_accessibility_tests.rs crates/op-host-native/src/widget_host/a11y.rs crates/op-host-web/src/widget_host/a11y_bridge.rs
git commit -m "feat(i18n): localize accessible interaction authoring"
```
