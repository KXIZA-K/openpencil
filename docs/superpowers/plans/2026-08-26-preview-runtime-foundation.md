# Preview Runtime Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Finish the backwards-compatible Jian/`op-preview-core` contracts for rich gestures, safe actions, dynamic bindings, animations, effects, and diagnostics.

**Architecture:** Jian remains the only gesture and action runtime. Schema fields are additive and preserve unknown data; `op-preview-core` maps screen/device coordinates into `PreviewInput`, applies runtime overlays with classified invalidation, owns animation/transition scheduling, and exposes a bounded trace/effect API to hosts.

**Tech Stack:** Rust, serde/schemars, Jian Gesture Arena and ActionRegistry, OpenPencil `op-preview-core`, Cargo tests.

---

### Task R0: Split the PreviewSession Spine Without Behavior Change

**Files:**
- Create: `crates/op-preview-core/src/session.rs`
- Create: `crates/op-preview-core/src/session_paint.rs`
- Modify: `crates/op-preview-core/src/lib.rs`

- [ ] **Step 1: Record the green baseline**

```bash
cargo test -p op-preview-core
```

Expected: PASS before code motion.

- [ ] **Step 2: Move one responsibility per sibling**

Move `RootFrame`, `PreviewSession`, retained source/session fields, and entry/accessor methods to `session.rs`. Move Preview scene overlay/paint methods to `session_paint.rs`. Keep `lib.rs` as module declarations plus stable re-exports; do not change public signatures, serialization, input, paint output, or tests. Keep every resulting file below 800 lines.

- [ ] **Step 3: Prove behavior-identical and commit**

```bash
cargo test -p op-preview-core
git diff --check
git add crates/op-preview-core/src/session.rs crates/op-preview-core/src/session_paint.rs crates/op-preview-core/src/lib.rs
git commit -m "refactor(editor): split preview session spine"
```

### Task R1: Additive Event and Gesture Schema With Unknown-Field Preservation

**Files:**
- Modify: `vendor/jian/crates/jian-ops-schema/src/events.rs`
- Modify: `vendor/jian/crates/jian-ops-schema/src/gestures.rs`
- Modify: `vendor/jian/crates/jian-ops-schema/src/lifecycle.rs`
- Modify: `vendor/jian/crates/jian-core/src/expression/aot.rs`
- Create: `vendor/jian/crates/jian-core/src/expression/aot_tests.rs`
- Modify: `vendor/jian/crates/jian-core/src/runtime/async_runtime.rs`
- Test: `vendor/jian/crates/jian-ops-schema/src/events.rs`
- Test: `vendor/jian/crates/jian-ops-schema/src/gestures.rs`
- Test: `vendor/jian/crates/jian-ops-schema/src/lifecycle.rs`

- [ ] **Step 1: Write failing round-trip tests**

First record the Jian expression tests green and move `aot.rs`'s inline tests to `aot_tests.rs` without behavior change, leaving the touched production file below 800 lines. Then add tests that assert new and future fields survive deserialize/serialize:

```rust
#[test]
fn rich_event_hooks_and_future_fields_round_trip() {
    let input = serde_json::json!({
        "onPressStart": [{"set":{"$app.down":"true"}}],
        "onPressEnd": [{"set":{"$app.down":"false"}}],
        "onPressCancel": [{"set":{"$app.cancelled":"true"}}],
        "onSwipe": [{"set":{"$app.direction":"$event.direction"}}],
        "onContextMenu": [{"toast":"`Context`"}],
        "onFutureGesture": [{"futureAction":{"value":1}}]
    });
    let decoded: EventHandlers = serde_json::from_value(input.clone()).unwrap();
    let output = serde_json::to_value(decoded).unwrap();
    assert_eq!(output, input);
}
```

```rust
#[test]
fn rich_gesture_overrides_round_trip() {
    let input = serde_json::json!({
        "doubleTapTimeout": 280,
        "doubleTapSlop": 12,
        "swipeMinDistance": 48,
        "swipeMinVelocity": 320,
        "axisLock": "horizontal",
        "disabledEvents": ["onHoverEnter"],
        "interactionOrder": ["onSwipe", "onTap"],
        "futureThreshold": 7
    });
    let decoded: GestureOverrides = serde_json::from_value(input.clone()).unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(), input);
}
```

Add the same lossless test for app/page/node lifecycle objects, including a future hook and known hook with a future action body field:

```rust
#[test]
fn lifecycle_hooks_and_future_fields_round_trip() {
    let input = serde_json::json!({
        "onMount": [{"animate":{"target":"$self","to":{"opacity":1},"futureCurve":"spring-v2"}}],
        "disabledEvents": ["onUnmount"],
        "interactionOrder": ["onMount", "onUnmount"],
        "onFutureVisibility": [{"futureAction":{"value":1}}]
    });
    let decoded: NodeLifecycleHooks = serde_json::from_value(input.clone()).unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(), input);
}
```

- [ ] **Step 2: Run tests and confirm RED**

Run:

```bash
cargo test --manifest-path vendor/jian/Cargo.toml -p jian-ops-schema rich_event_hooks_and_future_fields_round_trip
cargo test --manifest-path vendor/jian/Cargo.toml -p jian-ops-schema rich_gesture_overrides_round_trip
cargo test --manifest-path vendor/jian/Cargo.toml -p jian-ops-schema lifecycle_hooks_and_future_fields_round_trip
```

Expected: FAIL because the new fields and unknown keys are dropped.

- [ ] **Step 3: Add schema fields and flattened extras**

Add these fields to `EventHandlers`:

```rust
pub on_press_start: Option<ActionList>,
pub on_press_end: Option<ActionList>,
pub on_press_cancel: Option<ActionList>,
pub on_swipe: Option<ActionList>,
pub on_context_menu: Option<ActionList>,
#[serde(default, flatten)]
pub extra: BTreeMap<String, serde_json::Value>,
```

Add `AxisLock` and these optional fields to `GestureOverrides`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AxisLock { Auto, Horizontal, Vertical }

pub double_tap_timeout: Option<u32>,
pub double_tap_slop: Option<f64>,
pub swipe_min_distance: Option<f64>,
pub swipe_min_velocity: Option<f64>,
pub axis_lock: Option<AxisLock>,
pub disabled_events: Option<Vec<String>>,
pub interaction_order: Option<Vec<String>>,
#[serde(default, flatten)]
pub extra: BTreeMap<String, serde_json::Value>,
```

Every optional field uses `#[serde(default, skip_serializing_if = "Option::is_none")]`. Add `disabled_events`, `interaction_order`, and a flattened `extra: BTreeMap<String, serde_json::Value>` to `AppLifecycleHooks`, `PageLifecycleHooks`, and `NodeLifecycleHooks` too. `disabledEvents` is the additive, per-trigger storage used by the Interact Tab; `interactionOrder` stores explicit card order because typed handler structs cannot preserve JSON key order. Preserve vector order, reject duplicates/unknown duplicate references in authoring validation, append handlers missing from `interactionOrder` in shared catalog order, and never rewrite an ActionList to simulate disabled state.

Update Jian's exhaustive AOT/async traversals in the same commit: compile and discover every new known EventHandlers field and every existing app/page/node lifecycle ActionList. Flattened unknown hooks remain opaque/pass-through and are never executed by an older runtime.

- [ ] **Step 4: Run schema tests and compatibility tests**

Run:

```bash
cargo test --manifest-path vendor/jian/Cargo.toml -p jian-ops-schema
cargo test --manifest-path vendor/jian/Cargo.toml -p jian-core expression
```

Expected: PASS; existing fixtures serialize unchanged when no new fields are present.

- [ ] **Step 5: Commit**

```bash
git -C vendor/jian add crates/jian-ops-schema/src/events.rs crates/jian-ops-schema/src/gestures.rs crates/jian-ops-schema/src/lifecycle.rs crates/jian-core/src/expression/aot.rs crates/jian-core/src/expression/aot_tests.rs crates/jian-core/src/runtime/async_runtime.rs
git -C vendor/jian commit -m "feat(types): extend preview interaction schema"
git add vendor/jian
git commit -m "chore(renderer): update jian interaction schema"
```

### Task R2: Press, Swipe, Context Menu, and Correct Double-Tap Arbitration

**Files:**
- Create: `vendor/jian/crates/jian-core/src/gesture/recognizers/press.rs`
- Create: `vendor/jian/crates/jian-core/src/gesture/recognizers/swipe.rs`
- Modify: `vendor/jian/crates/jian-core/src/gesture/recognizers/mod.rs`
- Modify: `vendor/jian/crates/jian-core/src/gesture/recognizer.rs`
- Modify: `vendor/jian/crates/jian-core/src/gesture/arena.rs`
- Modify: `vendor/jian/crates/jian-core/src/gesture/recognizers/pan.rs`
- Modify: `vendor/jian/crates/jian-core/src/gesture/recognizers/long_press.rs`
- Modify: `vendor/jian/crates/jian-core/src/gesture/recognizers/tap.rs`
- Modify: `vendor/jian/crates/jian-core/src/gesture/recognizers/hover.rs`
- Modify: `vendor/jian/crates/jian-core/src/gesture/recognizers/scale.rs`
- Modify: `vendor/jian/crates/jian-core/src/gesture/recognizers/rotate.rs`
- Modify: `vendor/jian/crates/jian-core/src/gesture/semantic.rs`
- Modify: `vendor/jian/crates/jian-core/src/gesture/router.rs`
- Modify: `vendor/jian/crates/jian-core/src/gesture/dispatcher.rs`
- Modify: `vendor/jian/crates/jian-core/src/runtime/async_runtime.rs`
- Modify: `vendor/jian/crates/jian-core/src/runtime/pointer_input.rs`
- Test: `vendor/jian/crates/jian-core/tests/gesture_rich_events.rs`
- Test: `vendor/jian/crates/jian-core/tests/gesture_event_payloads.rs`
- Modify: `vendor/jian/crates/jian-core/tests/gesture_tap_counter.rs`

- [ ] **Step 1: Write failing semantic trace tests**

Create a runtime fixture whose node declares Press, Swipe, ContextMenu, Tap, and DoubleTap handlers. Assert:

```rust
assert_eq!(names(down), ["onPressStart"]);
assert_eq!(names(up), ["onPressEnd", "onTap"]);
assert_eq!(names(cancel), ["onPressCancel"]);
assert_eq!(names(horizontal_fast_drag), ["onSwipe"]);
assert_eq!(names(right_mouse_down), ["onContextMenu"]);
assert_eq!(names(second_tap_up), ["onDoubleTap"]);
```

Use `PointerEvent` with real `kind`, `buttons`, and `t_ms`. Also assert:

- a Tap-only node dispatches immediately on Up;
- a Tap+DoubleTap node delays the first Tap until `doubleTapTimeout`, dispatches only DoubleTap on a matching second Tap, and dispatches the delayed Tap exactly once when the window expires;
- a delayed Tap fires at its deadline even when no second input arrives;
- LongPress and Pan cancel Press and Tap; `Cancel` emits PressCancel exactly once;
- Pan wins over Swipe whenever any Pan hook exists; otherwise Swipe honors distance, velocity, and `axisLock`;
- Scale and Rotate can both update in one two-pointer session;
- `gestures.disabled`, `rawPointer`, `dragThreshold`, `longPressDuration`, and every new override change behavior, not only serialization;
- Touch never dispatches HoverEnter/HoverLeave, while Mouse/Pen hover does;
- right-click and touch LongPress follow the exclusive ContextMenu fallback rule.

The double-tap assertion must prove the second tap does not also execute `onTap` when `onDoubleTap` exists.

In `gesture_event_payloads.rs`, snapshot exact `$event` JSON for pointer local/global coordinates and kind/button/pressure/modifiers; Pan start/current/delta/translation/velocity; Swipe direction/distance/velocity; and Scale/Rotate focal plus absolute/delta values. Key/control/Scroll/lifecycle payloads belong to R4/R6 tests. Do not compare only handler names.

- [ ] **Step 2: Verify RED**

Run:

```bash
cargo test --manifest-path vendor/jian/Cargo.toml -p jian-core --test gesture_rich_events
cargo test --manifest-path vendor/jian/Cargo.toml -p jian-core --test gesture_event_payloads
```

Expected: FAIL because the new semantic variants/handlers do not exist and DoubleTap currently emits beside Tap.

- [ ] **Step 3: Add semantic variants and handler keys**

Add:

```rust
PressStart { node: NodeKey, position: Point },
PressEnd { node: NodeKey, position: Point },
PressCancel { node: NodeKey, position: Point },
Swipe { node: NodeKey, direction: SwipeDirection, distance: f32, velocity: Point },
ContextMenu { node: NodeKey, position: Point },
```

Map them to `onPressStart`, `onPressEnd`, `onPressCancel`, `onSwipe`, and `onContextMenu` in `handler_key()`.

Add a normalized `SemanticEventEnvelope { event, pointer_facts }` and one `payload(document)` path. Recognizers retain real PointerEvent facts instead of reconstructing them later; dispatcher computes node-local coordinates from the target layout and passes the object unchanged into ActionContext `$event` through `runtime/async_runtime.rs`. Every gesture variant must populate the standard fields named by the design; missing facts serialize as absent, never guessed values.

- [ ] **Step 4: Implement recognizers and router selection**

`PressRecognizer` emits Start on Down, End on an unclaimed Up, and Cancel when another arena member wins or the pointer cancels. Extend the recognizer/arena rejection callback so cancellation receives an `ArenaHandle`; do not silently call `reject()` where PressCancel would be lost. `SwipeRecognizer` claims only when no Pan handler exists on that node and the authored/default distance and velocity thresholds pass.

Update router discovery so recognizers are installed only when the hit path declares the corresponding enabled handler. Read thresholds from the hit node's `GestureOverrides`; make Pan/LongPress consume authored values and enforce `disabledEvents`. Right mouse Down emits ContextMenu. A touch LongPress emits explicit `onLongPress` when present; otherwise it falls back to ContextMenu.

Replace the router constants with a pending-tap state integrated into the existing `PointerRouter::tick(now_ms)` and `next_wake_ms()`. When both handlers exist, buffer one Tap through the authored/default window; replace it with DoubleTap on a matching second Tap, otherwise flush it exactly once when Runtime pump reaches the deadline, even with no further input. Exercise the R1 AOT/async wiring with delayed and nested actions in new handlers so those lists schedule exactly like existing handlers.

- [ ] **Step 5: Run gesture tests**

Run:

```bash
cargo test --manifest-path vendor/jian/Cargo.toml -p jian-core --test gesture_rich_events
cargo test --manifest-path vendor/jian/Cargo.toml -p jian-core --test gesture_event_payloads
cargo test --manifest-path vendor/jian/Cargo.toml -p jian-core --test gesture_tap_counter
cargo test --manifest-path vendor/jian/Cargo.toml -p jian-core gesture
```

Expected: PASS with deterministic event order.

- [ ] **Step 6: Commit**

```bash
git -C vendor/jian add crates/jian-core/src/gesture crates/jian-core/src/runtime/async_runtime.rs crates/jian-core/src/runtime/pointer_input.rs crates/jian-core/tests/gesture_rich_events.rs crates/jian-core/tests/gesture_event_payloads.rs crates/jian-core/tests/gesture_tap_counter.rs
git -C vendor/jian commit -m "feat(renderer): complete rich gesture semantics"
git add vendor/jian
git commit -m "chore(renderer): update jian gesture runtime"
```

### Task R3: Preview Action Policy and Host Effect Queue

**Files:**
- Create: `vendor/jian/crates/jian-core/src/action/policy.rs`
- Create: `vendor/jian/crates/jian-core/src/action/services/effect_sink.rs`
- Modify: `vendor/jian/crates/jian-core/src/action/services/mod.rs`
- Modify: `vendor/jian/crates/jian-core/src/action/context.rs`
- Modify: `vendor/jian/crates/jian-core/src/action/action_trait.rs`
- Modify: `vendor/jian/crates/jian-core/src/runtime.rs`
- Modify: `vendor/jian/crates/jian-core/src/runtime/construction.rs`
- Modify: `vendor/jian/crates/jian-core/src/runtime/async_runtime.rs`
- Modify: `vendor/jian/crates/jian-core/src/action/executor.rs`
- Modify: `vendor/jian/crates/jian-core/src/action/registry.rs`
- Modify: `vendor/jian/crates/jian-core/src/action/error.rs`
- Modify: `vendor/jian/crates/jian-core/src/action/mod.rs`
- Modify: `vendor/jian/crates/jian-core/src/action/actions/navigation.rs`
- Modify: `vendor/jian/crates/jian-core/src/action/actions/clipboard.rs`
- Modify: `vendor/jian/crates/jian-core/src/action/actions/platform.rs`
- Modify: `vendor/jian/crates/jian-core/src/action/actions/feedback.rs`
- Modify: `vendor/jian/crates/jian-core/src/action/actions/network.rs`
- Modify: `vendor/jian/crates/jian-core/src/action/actions/storage_ops.rs`
- Modify: `vendor/jian/crates/jian-core/src/action/actions/logic.rs`
- Test: `vendor/jian/crates/jian-core/tests/action_policy.rs`
- Create: `crates/op-preview-contracts/Cargo.toml`
- Create: `crates/op-preview-contracts/src/lib.rs`
- Create: `crates/op-preview-contracts/src/capability.rs`
- Create: `crates/op-preview-contracts/src/platform_support.rs`
- Create: `crates/op-preview-contracts/src/effect.rs`
- Test: `crates/op-preview-contracts/src/tests.rs`
- Create: `crates/op-preview-core/src/effects.rs`
- Modify: `crates/op-preview-core/Cargo.toml`
- Modify: `crates/op-preview-core/src/lib.rs`
- Test: `crates/op-preview-core/src/tests_effects.rs`
- Modify: `Cargo.lock`

- [ ] **Step 1: Write failing policy and effect tests**

Assert that `fetch`, every WebSocket action, storage wipe, notify, paste, race, and `call` are rejected by Preview policy even when their capabilities are declared. Assert the approved action catalog is the exact allowlist and that the already-registered `open_url`, `copy`, `share`, `haptic`, `focus`, `blur`, `toast`, `alert`, and `confirm` actions create ordered effects. R5 adds and tests `dismiss_keyboard` with the other missing actions.

```rust
let policy = AllowListPolicy::new(PreviewActionPolicy::ALLOWED.iter().copied());
assert!(policy.check("open_url").is_ok());
assert!(matches!(policy.check("fetch"), Err(ActionError::PolicyRejected { .. })));
```

```rust
let effects = session.drain_effects();
assert_eq!(effects.iter().map(|e| e.kind()).collect::<Vec<_>>(),
           ["open_url", "copy", "haptic"]);
```

Cover queue capacity, unique ids, exactly-once completion, stale/expired user activation, missing capability, unsupported host result, invalid URL schemes, and continuation semantics. A rejected/unsupported action must produce a structured diagnostic and then follow only the ActionList's declared success/error continuation; it must not crash, execute twice, or swallow unrelated later safe actions.

- [ ] **Step 2: Verify RED**

Run:

```bash
cargo test --manifest-path vendor/jian/Cargo.toml -p jian-core --test action_policy
cargo test -p op-preview-core tests_effects
```

Expected: FAIL because no ActionPolicy/effect queue exists.

- [ ] **Step 3: Implement ActionPolicy**

Define:

```rust
pub trait ActionPolicy {
    fn check(&self, action: &str) -> Result<(), ActionError>;
}

pub struct AllowListPolicy { allowed: BTreeSet<String> }
```

Factories still parse the full body and nested continuations. Wrap each parsed `ActionImpl::execute` in a policy guard inside `ActionChain`: policy runs before the action's CapabilityGate or side effect, but after parse-time structure is preserved. Extend ActionImpl with a default no-op `on_policy_rejected(ctx)`; actions with authored `on_error` run that already-parsed branch. The guard emits `ActionError::PolicyRejected { action }` as a structured diagnostic, runs the optional rejection branch, then returns Ok so later safe siblings continue. Unknown/invalid syntax remains a parse error and is not disguised as a policy rejection. The fixed Preview allowlist is:

```rust
pub const ALLOWED: &[&str] = &[
    "set", "toggle", "delete", "reset", "if", "delay", "parallel",
    "push", "replace", "pop", "show", "hide", "toggle_visibility",
    "focus", "blur", "scroll_to", "animate", "toast", "alert", "confirm",
    "open_url", "copy", "share", "haptic", "dismiss_keyboard",
];
```

An ActionList is already sequential, so there is no separate `sequential` action name.

- [ ] **Step 4: Implement PreviewEffect queue**

Define the dependency-light public DTO contract in `op-preview-contracts` and re-export it from `op-preview-core`:

```rust
pub enum PreviewEffect {
    OpenUrl { id: u64, url: String, source: EffectSource },
    Copy { id: u64, text: String, source: EffectSource },
    Share { id: u64, payload: SharePayload, source: EffectSource },
    Haptic { id: u64, style: HapticStyle, source: EffectSource },
    FocusNode { id: u64, node_id: String, source: EffectSource },
    BlurFocus { id: u64, source: EffectSource },
    DismissKeyboard { id: u64, source: EffectSource },
    Toast { id: u64, message: String, source: EffectSource },
    Alert { id: u64, title: String, message: String, source: EffectSource },
    Confirm { id: u64, title: String, message: String, source: EffectSource },
}

pub struct EffectSource {
    pub node_id: String,
    pub event: String,
    pub activation: Option<UserActivationId>,
    pub required_capability: PreviewCapability,
}

pub enum PreviewEffectFailureCode {
    InvalidPayload, InvalidUrlScheme, PermissionDenied, ActivationExpired,
    PresentationFailed, PlatformFailure,
}

pub struct PreviewEffectFailure {
    pub code: PreviewEffectFailureCode,
    pub detail: Option<String>,
}

pub enum PreviewEffectResult {
    Success,
    Cancelled,
    Unsupported,
    Failed(PreviewEffectFailure),
}
```

Define `PreviewCapability`, `PreviewHostCapabilities`, platform identifiers, Effect DTOs, and activation ids in `op-preview-contracts`; it depends only on serde/serde_json/thiserror and contains no runtime or UI code. `platform_support.rs` defines the approved Complete/Adapted/Unsupported authoring table (including Hover -> Pressed/Focus and ContextMenu -> LongPress touch adaptations) used by E3/E4 badges. Every capability field is explicit and the struct has no `Default`. `op-preview-core`, `op-editor-ui`, FFI, and host adapters may depend on this leaf without creating the existing Core -> UI dependency cycle. H7 verifies live adapters against this frozen table rather than introducing it late.

Use a bounded FIFO, monotonically increasing ids, and a `drain_effects()` API in `op-preview-core`. Validate `http`, `https`, `mailto`, and `tel` URL schemes before enqueue. The Jian `EffectSink` stays platform-neutral; add it to Runtime construction and every `make_action_ctx` path, with a no-op diagnostic sink for non-Preview Runtime users. `op-preview-core` injects its queue adapter and maps requests into PreviewEffect with source, capability, and activation. Effect results resume only the declared continuation.

- [ ] **Step 5: Run tests**

Run:

```bash
cargo test --manifest-path vendor/jian/Cargo.toml -p jian-core --test action_policy
cargo test -p op-preview-contracts
cargo test -p op-preview-core tests_effects
```

Expected: PASS; denied actions appear in diagnostics, not effects.

- [ ] **Step 6: Commit**

```bash
git -C vendor/jian add crates/jian-core/src/action crates/jian-core/src/runtime.rs crates/jian-core/src/runtime/construction.rs crates/jian-core/src/runtime/async_runtime.rs
git -C vendor/jian commit -m "feat(renderer): add preview policy and effect sink"
git add vendor/jian Cargo.lock crates/op-preview-contracts crates/op-preview-core/Cargo.toml crates/op-preview-core/src/effects.rs crates/op-preview-core/src/lib.rs crates/op-preview-core/src/tests_effects.rs
git commit -m "feat(editor): add safe preview action policy and effects"
```

### Task R4: Unified PreviewInput and Multi-Pointer Dispatch

**Files:**
- Modify: `vendor/jian/crates/jian-core/src/gesture/pointer.rs`
- Modify: `vendor/jian/crates/jian-core/src/gesture/semantic.rs`
- Modify: `vendor/jian/crates/jian-core/src/runtime.rs`
- Modify: `vendor/jian/crates/jian-core/src/runtime/pointer_input.rs`
- Modify: `vendor/jian/crates/jian-core/src/runtime/keyboard_input.rs`
- Modify: `vendor/jian/crates/jian-core/src/runtime/text_input.rs`
- Modify: `vendor/jian/crates/jian-core/src/runtime/async_runtime.rs`
- Create: `vendor/jian/crates/jian-core/src/runtime/lifecycle_dispatch.rs`
- Create: `crates/op-preview-core/src/input_event.rs`
- Create: `crates/op-preview-core/src/interaction_state.rs`
- Modify: `crates/op-preview-core/src/session.rs`
- Modify: `crates/op-preview-core/src/input.rs`
- Modify: `crates/op-preview-core/src/app_mode.rs`
- Modify: `crates/op-preview-core/src/session.rs`
- Modify: `crates/op-preview-core/src/lib.rs`
- Test: `crates/op-preview-core/src/tests_input_trace.rs`
- Test: `crates/op-preview-core/src/tests_event_payloads.rs`
- Test: `crates/op-preview-core/src/tests_interaction_state.rs`

- [ ] **Step 1: Write failing canonical input trace test**

Feed two touch pointers, a key, and an IME sequence through one API:

```rust
let scale = session.dispatch_input(envelope(PreviewInput::Pointer(pointer(1, Down, 10.0, 10.0, 0))));
session.dispatch_input(envelope(PreviewInput::Pointer(pointer(2, Down, 110.0, 10.0, 1))));
let scale = session.dispatch_input(envelope(PreviewInput::Pointer(pointer(2, Move, 150.0, 40.0, 16))));
assert!(scale.semantic_handlers.contains(&"onScaleStart"));

session.dispatch_input(envelope(PreviewInput::ImePreedit { text: "ni".into(), selection: 2..2 }));
session.dispatch_input(envelope(PreviewInput::ImeCommit { text: "你".into() }));
assert_eq!(session.runtime_mut().focused_editable_snapshot().unwrap().text, "你");
```

Use a fixture that focuses an editable field before the IME sequence. Add Wheel began/changed/momentum/ended, Text, key repeat, focus traversal, and Foreground/Background/Terminate cases. Pump the reported wake after a lone Tap and prove the delayed onTap fires without further input. In `tests_event_payloads.rs`, snapshot exact key/code/repeat/modifiers; Change/Submit value/checked/selectedValue; Scroll offset/delta/max/direction/phase; and lifecycle previous/next/reason payloads. Assert `PreviewDispatchOutcome`, Runtime state, and lifecycle handler output directly; this task must not depend on the R9 trace API.

- [ ] **Step 2: Verify RED**

Run:

```bash
cargo test -p op-preview-core tests_input_trace
```

Expected: FAIL because Preview only exposes pointer phase with implicit id and no IME input enum.

- [ ] **Step 3: Define PreviewInput**

```rust
pub enum PreviewInput {
    Pointer(jian_core::gesture::PointerEvent),
    Wheel { event: jian_core::gesture::pointer::WheelEvent, phase: ScrollPhase },
    Key { key: String, code: String, repeat: bool, modifiers: Modifiers },
    Text(String),
    ImePreedit { text: String, selection: Range<usize> },
    ImeCommit { text: String },
    ImeCancel,
    FocusNext,
    FocusPrevious,
    Back { source: BackSource },
    Lifecycle(PreviewLifecycle),
}

pub struct PreviewInputEnvelope {
    pub input: PreviewInput,
    pub activation: Option<UserActivationId>,
}

pub struct PreviewDispatchOutcome {
    pub semantic_handlers: Vec<&'static str>,
    pub needs_redraw: bool,
    pub effects_enqueued: usize,
}
```

Add `ScrollPhase::{Began, Changed, Momentum, Ended, Cancelled}` and typed lifecycle payloads carrying the factual reason plus previous/next route when known. Implement `PreviewSession::dispatch_input(PreviewInputEnvelope) -> PreviewDispatchOutcome` as the sole public input entry. Keep existing helpers as compatibility wrappers that construct an envelope with no activation. Store activation only for the synchronous ActionList spawned by that input and expire it before delayed/async work.

Add `PreviewSession::pump(now_ms) -> PreviewDispatchOutcome` and `next_wake_deadline_ms() -> Option<u64>`. Initially the deadline is Runtime's gesture/caret/action-task wake; R7/R8 add animation/transition sources. A host must schedule the minimum deadline and call pump even if no new input or redraw arrives.

- [ ] **Step 4: Preserve pointer identity through document/device transforms**

Only transform coordinates; keep id, kind, pressure, buttons, modifiers, tilt, and timestamp unchanged. Replace the single `gesture_mapping` slot with a map keyed by pointer id so two pointers preserve independent capture transforms. Route multi-pointer events directly to Jian Runtime.

Track runtime `InteractionState` by node/pointer. Mouse/Pen Hover may set hover and dispatch authored Hover handlers. Touch Down sets Pressed, never dispatches Hover, and clears to Focused/Idle on Up; arena loss, Cancel, transition, or lifecycle exit clears Pressed. Expose the state to Preview paint so authored/derived `WidgetStates` produce the approved touch fallback.

Add Jian lifecycle dispatch for app launch/resume/background/terminate, page enter/leave/foreground/background, and node mount/unmount; route reconciliation in `app_mode.rs` invokes those hooks in deterministic leave/unmount then enter/mount order with normalized `$event`. Respect each owner's `disabledEvents`; `interactionOrder` affects authoring presentation only, not lifecycle execution order. Define the narrow `PreviewSession::cancel_input_ownership(reason)` here to clear pointer capture, gesture arenas, Pressed state, focus, and IME on Background/Terminate. P6 later composes this into comprehensive `cancel_all` for tasks/effects/animations/deferred input.

Add `PreviewSession::enter_with_capabilities(..., PreviewHostCapabilities)` and migrate new hosts to it. Keep the legacy `enter(...)` wrapper source-compatible but supply an explicit all-false capability set, so absence is fail-closed for effects instead of silently allowed.

- [ ] **Step 5: Run tests**

Run:

```bash
cargo test -p op-preview-core tests_input_trace
cargo test -p op-preview-core tests_event_payloads
cargo test -p op-preview-core tests_interaction_state
cargo test -p op-preview-core
```

Expected: PASS; legacy pointer wrapper tests remain green.

- [ ] **Step 6: Commit**

```bash
git -C vendor/jian add crates/jian-core/src/gesture/pointer.rs crates/jian-core/src/gesture/semantic.rs crates/jian-core/src/runtime.rs crates/jian-core/src/runtime/pointer_input.rs crates/jian-core/src/runtime/keyboard_input.rs crates/jian-core/src/runtime/text_input.rs crates/jian-core/src/runtime/async_runtime.rs crates/jian-core/src/runtime/lifecycle_dispatch.rs
git -C vendor/jian commit -m "feat(renderer): unify preview input and lifecycle dispatch"
git add vendor/jian crates/op-preview-core/src/input_event.rs crates/op-preview-core/src/interaction_state.rs crates/op-preview-core/src/input.rs crates/op-preview-core/src/app_mode.rs crates/op-preview-core/src/session.rs crates/op-preview-core/src/lib.rs crates/op-preview-core/src/tests_input_trace.rs crates/op-preview-core/src/tests_event_payloads.rs crates/op-preview-core/src/tests_interaction_state.rs
git commit -m "feat(editor): unify preview pointer keyboard and ime input"
```

### Task R5: Missing Safe Runtime Actions

**Files:**
- Create: `vendor/jian/crates/jian-core/src/action/catalog.rs`
- Create: `vendor/jian/crates/jian-core/src/action/services/ui_mutation_sink.rs`
- Create: `vendor/jian/crates/jian-core/src/action/actions/visibility.rs`
- Create: `vendor/jian/crates/jian-core/src/action/actions/scroll.rs`
- Modify: `vendor/jian/crates/jian-core/src/action/actions/state.rs`
- Modify: `vendor/jian/crates/jian-core/src/state/path.rs`
- Modify: `vendor/jian/crates/jian-core/src/state/scope.rs`
- Modify: `vendor/jian/crates/jian-core/src/action/actions/platform.rs`
- Modify: `vendor/jian/crates/jian-core/src/action/actions/mod.rs`
- Modify: `vendor/jian/crates/jian-core/src/action/services/mod.rs`
- Modify: `vendor/jian/crates/jian-core/src/action/context.rs`
- Modify: `vendor/jian/crates/jian-core/src/action/mod.rs`
- Modify: `vendor/jian/crates/jian-core/src/runtime.rs`
- Modify: `vendor/jian/crates/jian-core/src/runtime/construction.rs`
- Modify: `vendor/jian/crates/jian-core/src/runtime/async_runtime.rs`
- Test: `vendor/jian/crates/jian-core/tests/action_preview_safe.rs`
- Create: `crates/op-preview-core/src/ui_actions.rs`
- Modify: `crates/op-preview-core/src/lib.rs`
- Test: `crates/op-preview-core/src/tests_ui_actions.rs`

- [ ] **Step 1: Write failing action tests**

Cover `toggle`, `show`, `hide`, `toggle_visibility`, `scroll_to`, and `dismiss_keyboard`. Assert exact state/effect output. Add `$state.foo` read/write/reset/delete tests proving it is the documented alias of `$app.foo`, matching existing expression/widget binding behavior. Add a catalog test that enumerates the complete authorable vocabulary from the design: state, control-flow, navigation, UI, feedback, and system-effect actions. Assert unsafe/legacy actions are registered for compatibility where appropriate but never marked Preview-authorable.

- [ ] **Step 2: Verify RED**

```bash
cargo test --manifest-path vendor/jian/Cargo.toml -p jian-core --test action_preview_safe
cargo test -p op-preview-core tests_ui_actions
```

Expected: FAIL because actions are unregistered.

- [ ] **Step 3: Implement actions using existing state/effect services**

Define a shared `ActionDescriptor { name, category, body_shape, required_capability, preview_authorable }` catalog in Jian and export ordered `preview_action_descriptors()` from `action/mod.rs`. Add a platform-neutral `UiMutationSink` to ActionContext/Runtime construction, inject it from `op-preview-core`, and use a no-op diagnostic implementation for non-Preview Runtime users.

`Scope::parse_prefix("$state")` maps to App while `Scope::as_prefix()` remains canonical `$app`; this is an additive input alias, not a new storage scope. `toggle` accepts one writable bool path. Visibility actions emit typed runtime-node mutations rather than document JSON. `scroll_to` validates target id/alignment and emits a typed scroll request. `op-preview-core/ui_actions.rs` applies both to preview-only state and returns redraw/hit-test work; R6 folds that state into the unified overlay. `dismiss_keyboard` uses the R3 EffectSink. Keep default ActionList execution sequential; do not register a duplicate `sequential` action. `animate` has an authorable descriptor here and receives its runtime factory in R7.

- [ ] **Step 4: Run and commit**

```bash
cargo test --manifest-path vendor/jian/Cargo.toml -p jian-core --test action_preview_safe
cargo test -p op-preview-core tests_ui_actions
git -C vendor/jian add crates/jian-core/src/action crates/jian-core/src/state/path.rs crates/jian-core/src/state/scope.rs crates/jian-core/src/runtime.rs crates/jian-core/src/runtime/construction.rs crates/jian-core/src/runtime/async_runtime.rs crates/jian-core/tests/action_preview_safe.rs
git -C vendor/jian commit -m "feat(renderer): add safe preview actions"
git add vendor/jian crates/op-preview-core/src/ui_actions.rs crates/op-preview-core/src/lib.rs crates/op-preview-core/src/tests_ui_actions.rs
git commit -m "feat(renderer): add safe preview actions"
```

### Task R6: Binding Overlay and Invalidation Classification

**Files:**
- Modify: `crates/op-preview-core/src/binding_sites.rs`
- Create: `crates/op-preview-core/src/binding_overlay.rs`
- Create: `crates/op-preview-core/src/invalidation.rs`
- Modify: `crates/op-preview-core/src/ui_actions.rs`
- Modify: `crates/op-preview-core/src/lib.rs`
- Modify: `crates/op-preview-core/src/app_mode.rs`
- Test: `crates/op-preview-core/src/tests_binding_overlay.rs`

- [ ] **Step 1: Write failing binding tests**

Create a fixture binding `content`, value/checked/selectedValue, `visible`, opacity, fill/stroke/textColor, x/y, width/height, rotation, scale, component variant, and active state. Assert `InvalidationKind` and fresh hit geometry after state changes. Add a navigation action assertion so the Navigation variant has a real producer.

```rust
assert_eq!(session.set_state("color", json!("#ff0000")), InvalidationKind::PaintOnly);
assert_eq!(session.set_state("visible", json!(false)), InvalidationKind::HitTest);
assert_eq!(session.set_state("x", json!(24)), InvalidationKind::HitTest);
assert_eq!(session.set_state("width", json!(240)), InvalidationKind::Relayout);
assert_eq!(session.dispatch_action(push("/detail")), InvalidationKind::Navigation);
```

- [ ] **Step 2: Verify RED**

```bash
cargo test -p op-preview-core tests_binding_overlay
```

Expected: FAIL because only content/value overlay is applied.

- [ ] **Step 3: Implement typed BindingSite targets**

Replace string-only targets with:

```rust
pub enum BindingTarget {
    Content, Value, Checked, SelectedValue, Visible,
    Opacity, Fill, Stroke, TextColor,
    X, Y, Width, Height, Rotation, ScaleX, ScaleY, Variant, ActiveState,
}

pub enum InvalidationKind { None, PaintOnly, HitTest, Relayout, Navigation }
```

Treat the enum as ordered work: PaintOnly repaints; HitTest implies repaint + spatial rebuild; Relayout implies layout + spatial rebuild + repaint; Navigation reconciles route/page mount. Color/opacity are PaintOnly; visibility and transforms are HitTest; content, width/height, and structural variant/active-state swaps are Relayout. Apply values to a runtime overlay document and preserve the authored document.

- [ ] **Step 4: Run tests and commit**

```bash
cargo test -p op-preview-core tests_binding_overlay
cargo test -p op-preview-core tests_bindings
git add crates/op-preview-core/src/binding_sites.rs crates/op-preview-core/src/binding_overlay.rs crates/op-preview-core/src/invalidation.rs crates/op-preview-core/src/ui_actions.rs crates/op-preview-core/src/lib.rs crates/op-preview-core/src/app_mode.rs crates/op-preview-core/src/tests_binding_overlay.rs
git commit -m "feat(editor): apply dynamic preview bindings"
```

### Task R7: Structured Animate Action and Bounded Timeline

**Files:**
- Create: `vendor/jian/crates/jian-core/src/action/actions/animate.rs`
- Create: `vendor/jian/crates/jian-core/src/action/services/animation_sink.rs`
- Modify: `vendor/jian/crates/jian-core/src/action/actions/mod.rs`
- Modify: `vendor/jian/crates/jian-core/src/action/services/mod.rs`
- Modify: `vendor/jian/crates/jian-core/src/action/context.rs`
- Test: `vendor/jian/crates/jian-core/tests/action_animate.rs`
- Create: `crates/op-preview-core/src/animation.rs`
- Modify: `crates/op-preview-core/src/invalidation.rs`
- Modify: `crates/op-preview-core/src/session.rs`
- Modify: `crates/op-preview-core/src/lib.rs`
- Test: `crates/op-preview-core/src/tests_animation.rs`

- [ ] **Step 1: Write failing parser and timeline tests**

Parse every approved field (`target`, optional `from`, `to`, `durationMs`, `delayMs`, `easing`, `iterations`, `direction`, `fillMode`) and reject zero/overflow iterations, non-finite values, unknown properties, and invalid easing with typed errors. Assert values at delay boundary and 0/50/100%, alternate/reverse iterations, fill modes, replacement/cancellation, and `from` sampling the current runtime value.

Cover opacity, x/y, rotation, scaleX/scaleY, fill, stroke, cornerRadius, plus width/height as a discrete layout-transition track. Assert continuous Pan/Scale/Rotate updates never start width/height relayout animation.

- [ ] **Step 2: Verify RED**

```bash
cargo test --manifest-path vendor/jian/Cargo.toml -p jian-core --test action_animate
cargo test -p op-preview-core tests_animation
```

Expected: FAIL because `animate` has catalog metadata but no factory, sink, or timeline.

- [ ] **Step 3: Implement the structured request and one session timeline**

Define typed `AnimationRequest`, `AnimationProperty`, `Easing`, `AnimationDirection`, and `AnimationFillMode` in Jian. Add AnimationSink to Runtime construction and every ActionContext path, with a diagnostic no-op for non-Preview users; Preview injects its timeline adapter. The action sends a request through the sink; `op-preview-core` owns `AnimationTimeline { tracks, next_deadline_ms }`. Fold this deadline into `PreviewSession::next_wake_deadline_ms()` with Runtime's gesture/action wake. One session schedules one host wake regardless of track count. A tick applies overlay values through R6 invalidation: visual properties are PaintOnly, transforms HitTest, width/height Relayout.

- [ ] **Step 4: Run and commit**

```bash
cargo test --manifest-path vendor/jian/Cargo.toml -p jian-core --test action_animate
cargo test -p op-preview-core tests_animation
git -C vendor/jian add crates/jian-core/src/action/actions/animate.rs crates/jian-core/src/action/actions/mod.rs crates/jian-core/src/action/services/animation_sink.rs crates/jian-core/src/action/services/mod.rs crates/jian-core/src/action/context.rs crates/jian-core/src/runtime.rs crates/jian-core/src/runtime/construction.rs crates/jian-core/src/runtime/async_runtime.rs crates/jian-core/tests/action_animate.rs
git -C vendor/jian commit -m "feat(renderer): add structured animate action"
git add vendor/jian crates/op-preview-core/src/animation.rs crates/op-preview-core/src/invalidation.rs crates/op-preview-core/src/session.rs crates/op-preview-core/src/lib.rs crates/op-preview-core/src/tests_animation.rs
git commit -m "feat(editor): add structured preview animations"
```

### Task R8: Transition Input Arbitration

**Files:**
- Modify: `crates/op-preview-core/src/transition.rs`
- Modify: `crates/op-preview-core/src/input.rs`
- Modify: `crates/op-preview-core/src/session.rs`
- Modify: `crates/op-preview-core/src/lib.rs`
- Test: `crates/op-preview-core/src/tests_transition_input.rs`

- [ ] **Step 1: Write failing transition-input tests**

Start a route transition with active Press/Pan/Scale, then feed Tap, Submit, Back, a newer Tap, Wheel, Pan, Scale, Rotate, Text, IME, and an activation-bearing system effect. Assert all captures/continuous gestures cancel, the newer safe discrete input replaces the older slot, continuous/text/IME input is discarded, and no queued input can enqueue an Effect early.

- [ ] **Step 2: Verify RED**

```bash
cargo test -p op-preview-core tests_transition_input
```

Expected: FAIL because transition input is currently discarded wholesale.

- [ ] **Step 3: Implement the one-slot policy**

Define payload-bearing variants:

```rust
pub enum DeferredDiscreteInput {
    Tap { position: Point, pointer: PointerFacts, activation: Option<UserActivationId>, generation: u64 },
    Submit { key: String, code: String, modifiers: Modifiers, activation: Option<UserActivationId>, generation: u64 },
    Back { source: BackSource, activation: Option<UserActivationId>, generation: u64 },
}
```

A transition-local tracker observes raw Pointer Down/Up and creates Tap only when id/slop/duration match; it never queues raw pointer phases. Enter on the focused submit-capable control creates Submit; platform Back/Escape maps through R4 `PreviewInput::Back`. Store at most one value and replace older entries. Fold transition deadlines into the same session next-wake minimum. On transition completion, rebuild layout/spatial state, re-resolve the target/focus, verify generation and activation rules, then replay only a still-valid input. Clear tracker/slot on cancel, Preview exit, document replacement, or lifecycle suspension.

- [ ] **Step 4: Run and commit**

```bash
cargo test -p op-preview-core tests_transition_input
cargo test -p op-preview-core tests_transition
git add crates/op-preview-core/src/transition.rs crates/op-preview-core/src/input.rs crates/op-preview-core/src/session.rs crates/op-preview-core/src/lib.rs crates/op-preview-core/src/tests_transition_input.rs
git commit -m "feat(editor): arbitrate input during preview transitions"
```

### Task R9: Bounded Trace, State Provenance, and Debug Controls

**Files:**
- Create: `crates/op-preview-contracts/src/debug.rs`
- Modify: `crates/op-preview-contracts/src/lib.rs`
- Create: `vendor/jian/crates/jian-core/src/action/services/observer.rs`
- Modify: `vendor/jian/crates/jian-core/src/action/services/mod.rs`
- Modify: `vendor/jian/crates/jian-core/src/action/context.rs`
- Modify: `vendor/jian/crates/jian-core/src/action/action_trait.rs`
- Modify: `vendor/jian/crates/jian-core/src/action/executor.rs`
- Modify: `vendor/jian/crates/jian-core/src/runtime.rs`
- Modify: `vendor/jian/crates/jian-core/src/runtime/construction.rs`
- Modify: `vendor/jian/crates/jian-core/src/runtime/async_runtime.rs`
- Create: `crates/op-preview-core/src/debug_trace.rs`
- Modify: `crates/op-preview-core/src/input.rs`
- Modify: `crates/op-preview-core/src/effects.rs`
- Modify: `crates/op-preview-core/src/animation.rs`
- Modify: `crates/op-preview-core/src/app_mode.rs`
- Modify: `crates/op-preview-core/src/lib.rs`
- Test: `crates/op-preview-core/src/tests_debug_trace.rs`
- Test: `crates/op-preview-core/src/tests_debug_controls.rs`

- [ ] **Step 1: Write failing trace/provenance/control tests**

Assert 257 entries retain newest 256; an authored interaction records exact Input -> SemanticEvent -> Action -> StateDiff -> Route/Animation/Effect -> EffectResult order; each state value is grouped by `$app`/`$page`/`$state`/`$self` and carries its last writer node/event/action/sequence. Clipboard/share bodies, activation ids, credentials, and private effect payloads must be redacted.

Pause a live session and prove input, action tasks, animation ticks, and effect draining stop without losing the current snapshot. Resume and prove exactly-once continuation. Reset and prove all captures/tasks/effects/focus/state/routes/animations clear and the document defaults plus entry route are rebuilt.

- [ ] **Step 2: Verify RED**

```bash
cargo test -p op-preview-core tests_debug_trace
cargo test -p op-preview-core tests_debug_controls
```

Expected: FAIL because tracing, provenance, pause/resume, and reset APIs do not exist.

- [ ] **Step 3: Implement DTOs, instrumentation, and controls**

Put serializable `PreviewTraceKind`, `PreviewTraceEntry`, `PreviewDiagnostic`, scoped state/provenance rows, and `PreviewDebugSnapshot` in the leaf `op-preview-contracts` crate. Add a Jian `ActionObserver` around each `ActionImpl::execute` start/result in `ActionChain::run_serial` (including nested lists), not around parse-time factories. Wire it through Runtime construction/ActionContext with a no-op default and inject Core's trace observer. Core records input, semantic payload, action, before/after state diff, route, animation, effect/result, and typed diagnostics into a 256-entry `VecDeque`.

Expose `debug_snapshot()`, `trace_entries()`, `pause()`, `resume()`, and `reset()` on PreviewSession. Paused sessions report no scheduled host wake until Resume; Resume re-arms the minimum Runtime/animation/transition deadline without double-running elapsed tasks. Snapshot includes run state, route stack/current screen, focused node, captured pointers, active gestures, scoped state with last writes, queue counts, and current diagnostics. Reset uses the retained source snapshot, never mutates the editor document.

- [ ] **Step 4: Run and commit**

```bash
cargo test -p op-preview-contracts
cargo test --manifest-path vendor/jian/Cargo.toml -p jian-core action
cargo test -p op-preview-core tests_debug_trace
cargo test -p op-preview-core tests_debug_controls
git -C vendor/jian add crates/jian-core/src/action crates/jian-core/src/runtime.rs crates/jian-core/src/runtime/construction.rs crates/jian-core/src/runtime/async_runtime.rs
git -C vendor/jian commit -m "feat(renderer): add runtime action observer"
git add vendor/jian crates/op-preview-contracts/src/debug.rs crates/op-preview-contracts/src/lib.rs crates/op-preview-core/src/debug_trace.rs crates/op-preview-core/src/input.rs crates/op-preview-core/src/effects.rs crates/op-preview-core/src/animation.rs crates/op-preview-core/src/app_mode.rs crates/op-preview-core/src/session.rs crates/op-preview-core/src/lib.rs crates/op-preview-core/src/tests_debug_trace.rs crates/op-preview-core/src/tests_debug_controls.rs
git commit -m "feat(editor): expose bounded preview diagnostics and controls"
```
