# Preview Client and Host Adapters Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Connect canonical PreviewInput, host-capability, and PreviewEffect contracts to iOS, Android, Harmony, Desktop Native, and Web without platform-specific action semantics.

**Architecture:** FFI clients use additive, tail-growable v2 structs while the legacy ABI remains intact. Native/Web Rust hosts dispatch directly. Every adapter maps platform facts into shared logical coordinates/metadata, declares capabilities, starts activation-sensitive effects inside the originating input callback, completes each effect exactly once, and emits structured Unsupported/Failed results.

**Tech Stack:** Rust C ABI, Swift/UIKit/XCUITest, Kotlin/Android/JUnit/Instrumentation, ArkTS/NAPI/Harmony, Winit, CanvasKit/Web APIs, headless Chrome CDP.

---

### Task H0: Shared Mobile-Safe Native Shell Conduit

**Files:**
- Create: `crates/op-host-native/src/preview_contract.rs`
- Modify: `crates/op-host-native/src/lib.rs`
- Modify: `crates/op-host-native/src/widget_host.rs`
- Create: `crates/op-host-native/src/widget_host/preview_host_config.rs`
- Modify: `crates/op-host-native/src/widget_host/preview_input.rs`
- Create: `crates/op-host-native/src/widget_host/preview_effects.rs`
- Create: `crates/op-host-native/src/widget_host/preview_capabilities.rs`
- Create: `crates/op-host-native/src/widget_host/preview_input_tests.rs`
- Create: `crates/op-host-native/src/widget_host/preview_effect_tests.rs`
- Modify: `crates/op-host-native/src/widget_host/mode_transition_host.rs`

- [ ] **Step 1: Write failing shell-gate and wake tests**

Using the mobile-safe `widget-host` feature, configure capabilities before Preview entry, then feed full pointer/key/IME metadata into top chrome, bottom toolbar, More, Close, and Preview canvas. Assert chrome-first consumption, exactly one delivery, stable capture, effect drain/complete, entry lifecycle effects see the pending capabilities, and a minimum timed wake fires lone Tap/LongPress/delay without later input. E7 later adds the Debugger-specific chrome assertion on the same conduit.

- [ ] **Step 2: Verify RED**

```bash
cargo test -p op-host-native --features widget-host preview_input
cargo test -p op-host-native --features widget-host preview_effect
```

Expected: FAIL because WidgetHostNative has only legacy mouse-like Preview helpers.

- [ ] **Step 3: Implement one shared shell input/effect conduit**

Add `WidgetHostNative::apply_shell_input_v2`: existing editor chrome/capture hit testing runs first; only an unconsumed Preview-canvas event reaches `PreviewSession::dispatch_input`. `preview_contract.rs` publicly re-exports the complete shell-input/capability/effect/debug DTO surface from Core for FFI/JNI/NAPI consumers, so those crates use their existing op-host-native dependency instead of an illegal transitive import. Add `PreviewHostConfig` to persist pending capabilities before entry and consume them in `mode_transition_host.rs::enter_preview` so launch/mount actions are gated correctly. Expose mobile-safe effect drain/complete and `next_preview_wake_deadline_ms`, but no OS clipboard/OpenURL/share implementation. Register focused siblings beneath the existing `preview_input` module; shorten/move the large inline Preview field documentation while adding the config field so `widget_host.rs` ends below 800 lines.

- [ ] **Step 4: Run and commit**

```bash
cargo test -p op-host-native --features widget-host preview
cargo test -p op-host-native --features gl-host preview
git add crates/op-host-native/src/preview_contract.rs crates/op-host-native/src/lib.rs crates/op-host-native/src/widget_host.rs crates/op-host-native/src/widget_host/preview_host_config.rs crates/op-host-native/src/widget_host/preview_input.rs crates/op-host-native/src/widget_host/preview_effects.rs crates/op-host-native/src/widget_host/preview_capabilities.rs crates/op-host-native/src/widget_host/preview_input_tests.rs crates/op-host-native/src/widget_host/preview_effect_tests.rs crates/op-host-native/src/widget_host/mode_transition_host.rs
git commit -m "feat(editor): add shared native preview conduit"
```

### Task H1: Additive FFI Preview Input, Capability, Lifecycle, and Effect ABI

**Files:**
- Modify: `crates/op-engine-ffi/include/op_engine.h`
- Modify: `crates/op-engine-ffi/src/desc.rs`
- Create: `crates/op-engine-ffi/src/preview_input.rs`
- Create: `crates/op-engine-ffi/src/preview_effect.rs`
- Create: `crates/op-engine-ffi/src/preview_wake.rs`
- Modify: `crates/op-engine-ffi/src/lifecycle.rs`
- Modify: `crates/op-engine-ffi/src/lib.rs`
- Create: `crates/op-engine-ffi/src/preview_input_tests.rs`
- Create: `crates/op-engine-ffi/src/preview_effect_tests.rs`
- Modify: `crates/op-engine-ffi/tests/abi.rs`

- [ ] **Step 1: Write failing ABI/round-trip tests**

Pin every existing symbol and discriminant. Assert new tail-growable structs accept the minimum known size, ignore a larger tail, reject undersized/non-finite/invalid enums, preserve two pointer ids and all metadata, carry Wheel phase, route Text/IME/Lifecycle, register host capabilities, round-trip structured effect failures, and complete an effect exactly once. Add shell-gate tests: Preview canvas input reaches Preview once; top/bottom chrome, More, and Close are consumed by editor chrome and never reach Preview. E7/H2 add the later Debugger-specific assertion. After a lone Tap, verify the existing `OpNeedsRedraw(has_next_wake,next_wake_ms)` callback reports the double-tap timeout and `op_frame` at that wake pumps onTap without another input.

- [ ] **Step 2: Verify RED**

```bash
cargo test -p op-engine-ffi --features editor preview_input
cargo test -p op-engine-ffi --features editor preview_effect
cargo test -p op-engine-ffi --test abi preview
```

Expected: FAIL because the v2 Preview ABI does not exist.

- [ ] **Step 3: Define stable, additive ABI structs**

```c
typedef enum OpPreviewPointerPhase {
  OpPreviewPointerPhase_Down = 0,
  OpPreviewPointerPhase_Move = 1,
  OpPreviewPointerPhase_Up = 2,
  OpPreviewPointerPhase_Cancel = 3,
  OpPreviewPointerPhase_Hover = 4
} OpPreviewPointerPhase;

typedef enum OpPreviewPointerKind {
  OpPreviewPointerKind_Touch = 0,
  OpPreviewPointerKind_Mouse = 1,
  OpPreviewPointerKind_Pen = 2,
  OpPreviewPointerKind_Stylus = 3,
  OpPreviewPointerKind_Trackpad = 4
} OpPreviewPointerKind;

typedef struct OpPreviewPointerEvent {
  size_t size;
  uint32_t id;
  int32_t phase;
  int32_t kind;
  float x, y, pressure;
  uint32_t buttons, modifiers;
  float tilt_x_radians, tilt_y_radians;
  bool has_tilt;
  uint64_t time_ms;
  uint64_t activation_id;
} OpPreviewPointerEvent;
```

Add equally versioned `OpPreviewWheelEvent`, `OpPreviewKeyEvent`, `OpPreviewLifecycleEvent`, `OpPreviewHostCapabilities`, and `OpPreviewEffectResult`. Wheel carries Began/Changed/Momentum/Ended/Cancelled. Effect result carries id, result kind, `PreviewEffectFailureCode`, and bounded UTF-8 detail. Add:

```c
OpStatus op_preview_set_host_capabilities(OpEngine *, const OpPreviewHostCapabilities *);
OpStatus op_input_pointer_v2(OpEngine *, const OpPreviewPointerEvent *);
OpStatus op_input_wheel_v2(OpEngine *, const OpPreviewWheelEvent *);
OpStatus op_input_key_v2(OpEngine *, const OpPreviewKeyEvent *);
OpStatus op_input_text_v2(OpEngine *, const OpPreviewTextEvent *);
OpStatus op_input_ime_v2(OpEngine *, const OpPreviewImeEvent *);
OpStatus op_input_lifecycle_v2(OpEngine *, const OpPreviewLifecycleEvent *);
OpStatus op_preview_copy_next_effect(OpEngine *, uint8_t *, size_t, size_t *);
OpStatus op_preview_complete_effect(OpEngine *, const OpPreviewEffectResult *);
OpStatus op_preview_copy_trace_json(OpEngine *, uint8_t *, size_t, size_t *);
OpStatus op_preview_copy_debug_snapshot_json(OpEngine *, uint8_t *, size_t, size_t *);
```

`OpPreviewTextEvent` and `OpPreviewImeEvent` are tail-growable structs containing bounded UTF-8 ptr/len, monotonic time, activation id, IME phase, and UTF-8 selection where applicable. This lets trusted beforeinput/IME commit carry activation into any resulting ActionList; absent/expired activation remains explicit. The two read-only Debug APIs expose the R9 redacted normalized trace/snapshot for adapter conformance and Preview Debugger clients; use the same non-consuming two-phase copy rule.

The platform calls the general `op_input_*_v2` surface, not a direct Preview pointer surface. Rust's existing editor/shell hit-test and capture gate runs first; only an unconsumed Preview-canvas event is converted to `PreviewInputEnvelope`. Chrome and Preview never both receive the same event. Keep legacy `op_pointer`/editor IME symbols as wrappers into this gate with legacy metadata.

- [ ] **Step 4: Route ABI to shared contracts**

Convert every struct to `op_host_native::preview_contract` shell input/capability/effect DTOs through H1's existing op-host-native dependency; H1 adds no new direct dependency. `op_input_*_v2` always uses the single shell gate and internally delegates unconsumed Preview-canvas events to `PreviewInputEnvelope`; clients never implement fallback or direct-first routing. Fold `PreviewSession::next_wake_deadline_ms()` into the existing redraw callback after input and every `op_frame`; the callback is the one timer contract for LongPress, delayed Tap, action delay, animation, transition, and caret. Move wake folding into `preview_wake.rs` and make `lifecycle.rs` net smaller than 800 lines. Effect copy is non-consuming until the complete JSON payload fits; completion rejects unknown/already-completed ids. `op_preview_set_host_capabilities` is valid before Preview entry and caches the declaration on the engine for the next/recreated session. Preview effect/debug calls return NotReady while inactive. Return InvalidArg for invalid UTF-8/ranges/enums.

- [ ] **Step 5: Run and commit**

```bash
cargo test -p op-engine-ffi --features editor preview
cargo test -p op-engine-ffi --test abi
git add crates/op-engine-ffi/include/op_engine.h crates/op-engine-ffi/src/desc.rs crates/op-engine-ffi/src/preview_input.rs crates/op-engine-ffi/src/preview_effect.rs crates/op-engine-ffi/src/preview_wake.rs crates/op-engine-ffi/src/lifecycle.rs crates/op-engine-ffi/src/preview_input_tests.rs crates/op-engine-ffi/src/preview_effect_tests.rs crates/op-engine-ffi/src/lib.rs crates/op-engine-ffi/tests/abi.rs
git commit -m "feat(editor): expose preview input and effect abi"
```

### Task H2: iOS and iPad Preview Adapter

**Files:**
- Create: `packaging/ios/Sources/PreviewInputMapping.swift`
- Create: `packaging/ios/Sources/PreviewEffectMapping.swift`
- Create: `packaging/ios/Sources/PreviewInputAdapter.swift`
- Create: `packaging/ios/Sources/PreviewEffectAdapter.swift`
- Create: `packaging/ios/Sources/OpPlayerView+PreviewInput.swift`
- Create: `packaging/ios/Sources/OpPlayerView+PreviewIme.swift`
- Create: `packaging/ios/Sources/OpEngineHost+Preview.swift`
- Modify: `packaging/ios/Sources/OpPlayerView.swift`
- Modify: `packaging/ios/project.yml`
- Modify: `packaging/ios/OpenPencilPlayer.xcodeproj/project.pbxproj`
- Modify: `packaging/ios/Tests/validate_sources.sh`
- Create: `packaging/ios/Tests/PreviewInputMappingTests.swift`
- Create: `packaging/ios/Tests/PreviewEffectMappingTests.swift`
- Create: `packaging/ios/UITests/PreviewInteractionUITests.swift`
- Create: `packaging/ios/Tests/run_preview_simulator_tests.sh`

- [ ] **Step 1: Write failing pure-mapping and simulator UI tests**

Pure tests cover stable touch ids, logical coordinates, phase/type, normalized Pencil `force / maximumPossibleForce`, radians tilt, two-finger streams, Cancel, key modifiers, Chinese IME preedit/commit/cancel with UTF-8 selection, lifecycle, capabilities, every effect/result, and redraw-deadline scheduling. Add the XCUITest target to `project.yml`, regenerate the tracked pbxproj, and make `validate_sources.sh` assert both stay in sync. XCUITest covers Interact add/edit/save/readback, iPhone bottom-sheet Debugger, iPad side rail, Tap/LongPress/Scale/Rotate, Chinese input, Copy/Share/Haptic/OpenURL, and top/bottom/More/Close/buttons outside the chat/input surface. It asserts chrome and Preview gestures coexist without double delivery, and a lone Tap/LongPress/delay fires while the screen is otherwise idle.

- [ ] **Step 2: Verify RED**

```bash
bash packaging/ios/Tests/validate_sources.sh
bash packaging/ios/Tests/run_preview_simulator_tests.sh
```

Expected: FAIL because adapters, pure runners, and the UI-test target do not exist.

- [ ] **Step 3: Implement input, IME, lifecycle, and capability mapping**

Move/reuse the existing touch-id table instead of creating a second identity map. Keep `OpPlayerView.swift` below 800 lines by delegating Preview touch/IME work to the new extension files; keep the already-800-line `OpEngineHost.swift` unchanged and add Preview logic in its extension. Forward raw UITouch facts through H1's single shell input gate so Rust chrome consumes first and Jian owns only unconsumed Preview-canvas gestures. The existing needs-redraw callback schedules/cancels one main-thread timer or display-link wake at `next_wake_ms`, then calls `op_frame`. Register explicit iPhone/iPad capabilities and send app background/foreground/terminate.

- [ ] **Step 4: Implement every effect safely**

Run on main thread. Map Copy, Share, Haptic, OpenURL, Focus/Blur/DismissKeyboard, Toast, Alert, and Confirm; return exactly one typed result. On iPad set `popoverPresentationController.sourceView/sourceRect` before presenting Share. Cancel or teardown dismisses owned controllers and completes pending ids as Cancelled.

- [ ] **Step 5: Run and commit**

```bash
bash packaging/ios/Tests/validate_sources.sh
cargo test -p op-engine-ffi --features editor preview
bash packaging/ios/Tests/run_preview_simulator_tests.sh
git add packaging/ios/Sources/PreviewInputMapping.swift packaging/ios/Sources/PreviewEffectMapping.swift packaging/ios/Sources/PreviewInputAdapter.swift packaging/ios/Sources/PreviewEffectAdapter.swift packaging/ios/Sources/OpPlayerView+PreviewInput.swift packaging/ios/Sources/OpPlayerView+PreviewIme.swift packaging/ios/Sources/OpEngineHost+Preview.swift packaging/ios/Sources/OpPlayerView.swift packaging/ios/project.yml packaging/ios/OpenPencilPlayer.xcodeproj/project.pbxproj packaging/ios/Tests/validate_sources.sh packaging/ios/Tests/PreviewInputMappingTests.swift packaging/ios/Tests/PreviewEffectMappingTests.swift packaging/ios/UITests/PreviewInteractionUITests.swift packaging/ios/Tests/run_preview_simulator_tests.sh
git commit -m "feat(editor): connect ios preview interactions"
```

### Task H3: Android Preview Adapter

**Files:**
- Create: `packaging/android/app/src/main/kotlin/tech/zseven/openpencil/PreviewInputMapping.kt`
- Create: `packaging/android/app/src/main/kotlin/tech/zseven/openpencil/PreviewInputAdapter.kt`
- Create: `packaging/android/app/src/main/kotlin/tech/zseven/openpencil/PreviewEffectAdapter.kt`
- Create: `packaging/android/app/src/main/kotlin/tech/zseven/openpencil/PreviewLifecycleAdapter.kt`
- Create: `packaging/android/app/src/main/kotlin/tech/zseven/openpencil/OpSurfaceInputDelegate.kt`
- Create: `packaging/android/app/src/main/kotlin/tech/zseven/openpencil/OpSurfaceLifecycleDelegate.kt`
- Modify: `packaging/android/app/src/main/kotlin/tech/zseven/openpencil/OpSurfaceView.kt`
- Modify: `packaging/android/app/src/main/kotlin/tech/zseven/openpencil/OpInputConnection.kt`
- Modify: `packaging/android/app/src/main/kotlin/tech/zseven/openpencil/OpNative.kt`
- Modify: `packaging/android/app/build.gradle.kts`
- Modify: `packaging/android/gradle/verification-metadata.xml`
- Create: `packaging/android/app/src/test/kotlin/tech/zseven/openpencil/PreviewInputMappingTest.kt`
- Create: `packaging/android/app/src/test/kotlin/tech/zseven/openpencil/PreviewEffectMappingTest.kt`
- Create: `packaging/android/app/src/androidTest/kotlin/tech/zseven/openpencil/PreviewInteractionInstrumentedTest.kt`
- Create: `packaging/android/Tests/run_preview_emulator_tests.sh`
- Create: `crates/op-engine-jni/src/bindings_preview.rs`
- Create: `crates/op-engine-jni/src/preview_contract.rs`
- Modify: `crates/op-engine-jni/src/bindings.rs`
- Modify: `crates/op-engine-jni/src/lib.rs`

- [ ] **Step 1: Write failing pure JVM, JNI, and instrumentation tests**

First record the Android unit/instrumentation baseline, then move existing input and lifecycle blocks from the 1,139-line `OpSurfaceView.kt` into `OpSurfaceInputDelegate.kt` and `OpSurfaceLifecycleDelegate.kt` without behavior change; leave the view below 800 lines. Add `testInstrumentationRunner` plus pinned AndroidX test dependencies and their strict verification metadata. Pure mapping tests use DTOs, not Android `MotionEvent` stubs. `op-engine-jni/preview_contract.rs` is host-testable and serializes the exact canonical envelope expected by P2. Instrumentation creates real MotionEvent/InputConnection sequences for two pointers, tool type, pressure, hover, eventTime, Cancel, hardware keys, composing/commit/selection, lifecycle, all effects, Interact add/edit/save/readback, Preview chrome/Debugger buttons, and idle deadline wake. Assert Share cancellation and URL/permission failures are structured, shell chrome/canvas input are never double-delivered, and a lone Tap/LongPress/delay fires through Choreographer without later input.

- [ ] **Step 2: Verify RED**

```bash
./packaging/android/gradlew -p packaging/android testDebugUnitTest --tests '*Preview*'
cargo test -p op-engine-jni preview
bash packaging/android/Tests/run_preview_emulator_tests.sh
```

Expected: FAIL in the named JVM/JNI/instrumentation contracts because mapping/adapters/JNI Preview methods do not exist.

- [ ] **Step 3: Implement adapters and JNI surface**

Forward every pointer in actionMasked/actionIndex order and cancel all active pointers on ACTION_CANCEL. Preserve ids, tool type, normalized pressure, buttons/modifiers, hover, and eventTime. Reuse InputConnection surrounding-text/selection rules through the v2 shell gate. Put new JNI exports in `bindings_preview.rs` so `bindings_editor.rs` stays below 800 lines. Rust chrome consumes first and only unconsumed Preview-canvas input reaches Preview. Map needs-redraw to one Choreographer/timed callback at the minimum deadline and cancel stale callbacks by generation. Drain every effect on the UI thread and report exact results.

- [ ] **Step 4: Run and commit**

```bash
./packaging/android/gradlew -p packaging/android testDebugUnitTest --tests '*Preview*'
cargo test -p op-engine-jni
bash packaging/android/Tests/run_preview_emulator_tests.sh
git add packaging/android/app/src/main/kotlin/tech/zseven/openpencil/PreviewInputMapping.kt packaging/android/app/src/main/kotlin/tech/zseven/openpencil/PreviewInputAdapter.kt packaging/android/app/src/main/kotlin/tech/zseven/openpencil/PreviewEffectAdapter.kt packaging/android/app/src/main/kotlin/tech/zseven/openpencil/PreviewLifecycleAdapter.kt packaging/android/app/src/main/kotlin/tech/zseven/openpencil/OpSurfaceInputDelegate.kt packaging/android/app/src/main/kotlin/tech/zseven/openpencil/OpSurfaceLifecycleDelegate.kt packaging/android/app/src/main/kotlin/tech/zseven/openpencil/OpSurfaceView.kt packaging/android/app/src/main/kotlin/tech/zseven/openpencil/OpInputConnection.kt packaging/android/app/src/main/kotlin/tech/zseven/openpencil/OpNative.kt packaging/android/app/build.gradle.kts packaging/android/gradle/verification-metadata.xml packaging/android/app/src/test/kotlin/tech/zseven/openpencil/PreviewInputMappingTest.kt packaging/android/app/src/test/kotlin/tech/zseven/openpencil/PreviewEffectMappingTest.kt packaging/android/app/src/androidTest/kotlin/tech/zseven/openpencil/PreviewInteractionInstrumentedTest.kt packaging/android/Tests/run_preview_emulator_tests.sh crates/op-engine-jni/src/bindings_preview.rs crates/op-engine-jni/src/preview_contract.rs crates/op-engine-jni/src/bindings.rs crates/op-engine-jni/src/lib.rs
git commit -m "feat(editor): connect android preview interactions"
```

### Task H4: Harmony Preview Adapter

**Files:**
- Create: `packaging/harmony/entry/src/main/ets/common/PreviewInputAdapter.ets`
- Create: `packaging/harmony/entry/src/main/ets/common/PreviewEffectAdapter.ets`
- Create: `packaging/harmony/entry/src/main/ets/common/PreviewLifecycleAdapter.ets`
- Modify: `packaging/harmony/entry/src/main/ets/common/PointerRouter.ets`
- Modify: `packaging/harmony/entry/src/main/ets/common/ImeConduit.ets`
- Modify: `packaging/harmony/entry/src/main/ets/common/ImeProxyBridge.ets`
- Modify: `packaging/harmony/entry/src/main/ets/common/OpNative.ets`
- Modify: `packaging/harmony/entry/src/main/ets/pages/Index.ets`
- Modify: `packaging/harmony/entry/src/main/module.json5`
- Modify: `packaging/harmony/entry/src/main/cpp/types/libopenpencil/index.d.ts`
- Create: `packaging/harmony/Tests/PreviewAdapterContractTests.rb`
- Create: `packaging/harmony/Tests/run_preview_build.sh`
- Create: `crates/op-engine-napi/src/preview_contract.rs`
- Modify: `crates/op-engine-napi/src/bindings_input.rs`
- Modify: `crates/op-engine-napi/src/lib.rs`
- Modify: `crates/op-engine-napi/src/module.rs`
- Modify: `crates/op-engine-napi/README.md`

- [ ] **Step 1: Write failing ArkTS/NAPI source contracts**

Assert stable multi-touch identity, pressure/hover/Cancel, KeyEvent, IME composition/commit/selection/cancel, lifecycle/capabilities, Pasteboard/Share/Vibrator/Want URL, focus/keyboard/feedback effects, structured completion, timed wake scheduling, NAPI exported names, TypeScript declarations, and README API table. The host-testable `preview_contract.rs` converts primitive NAPI arguments to canonical JSON for P2 comparison without requiring an OHOS target.

- [ ] **Step 2: Verify RED**

```bash
ruby packaging/harmony/Tests/PreviewAdapterContractTests.rb
cargo test -p op-engine-napi preview
```

Expected: FAIL because Preview adapters and NAPI exports are absent.

- [ ] **Step 3: Implement in the existing input seams**

Map raw facts in PointerRouter/ImeConduit/ImeProxyBridge through the v2 Rust shell gate; do not add more code to the already-800-line EngineHost and do not direct-first Preview. PreviewLifecycleAdapter maps needs-redraw to one ArkUI timed/frame wake and cancels stale generations. Declare the VIBRATE permission in `module.json5`. Keep business semantics in Rust, return Unsupported for absent abilities, and complete every drained id. Register new exports in `bindings_input.rs`, `module.rs`, `lib.rs::EXPORTED_NAMES`, declarations, and README.

`run_preview_build.sh` first locates the OHOS NDK required by `scripts/build-ohos.sh`, then locates a real `hvigorw` from PATH or `DEVECO_SDK_HOME`, builds the Rust library and debug HAP, and exits 2 with an explicit “Harmony build untested: OHOS NDK/DevEco/Hvigor missing” message when unavailable. Never turn a missing SDK into a passing device/build claim; P9 records that cell as Untested.

- [ ] **Step 4: Run and commit**

```bash
ruby packaging/harmony/Tests/PreviewAdapterContractTests.rb
bash packaging/harmony/Tests/run_preview_build.sh
cargo test -p op-engine-napi
git add packaging/harmony/entry/src/main/ets/common/PreviewInputAdapter.ets packaging/harmony/entry/src/main/ets/common/PreviewEffectAdapter.ets packaging/harmony/entry/src/main/ets/common/PreviewLifecycleAdapter.ets packaging/harmony/entry/src/main/ets/common/PointerRouter.ets packaging/harmony/entry/src/main/ets/common/ImeConduit.ets packaging/harmony/entry/src/main/ets/common/ImeProxyBridge.ets packaging/harmony/entry/src/main/ets/common/OpNative.ets packaging/harmony/entry/src/main/ets/pages/Index.ets packaging/harmony/entry/src/main/module.json5 packaging/harmony/entry/src/main/cpp/types/libopenpencil/index.d.ts packaging/harmony/Tests/PreviewAdapterContractTests.rb packaging/harmony/Tests/run_preview_build.sh crates/op-engine-napi/src/preview_contract.rs crates/op-engine-napi/src/bindings_input.rs crates/op-engine-napi/src/lib.rs crates/op-engine-napi/src/module.rs crates/op-engine-napi/README.md
git commit -m "feat(editor): connect harmony preview interactions"
```

### Task H5: Desktop Native Preview Adapter

**Files:**
- Create: `crates/op-host-desktop/src/preview_effect_adapter.rs`
- Modify: `crates/op-host-desktop/src/main.rs`
- Modify: `crates/op-host-desktop/src/frame.rs`
- Modify: `crates/op-host-desktop/src/app_handler.rs`
- Modify: `crates/op-host-desktop/src/app_handler/pointer_events.rs`
- Modify: `crates/op-host-desktop/src/app_handler/keyboard_events.rs`

- [ ] **Step 1: Write failing native contract tests**

Assert every fact exposed by current winit/casement is preserved: mouse id/buttons/modifiers/hover, trackpad Wheel phase/mode, key repeat, logical coordinates, lifecycle, explicit capabilities, all effect mappings, Haptic Unsupported, activation expiry, exactly-once completion, and event-loop timed wake. A lone Tap/LongPress/delay must fire without a later winit event. If the event source cannot prove Pen kind/pressure/tilt, declare those capabilities false and emit a structured diagnostic; do not fabricate Pen metadata.

- [ ] **Step 2: Verify RED**

```bash
cargo test -p op-host-desktop preview
```

Expected: FAIL because the desktop event loop has not yet mapped winit facts, timed wake, or OS effects onto H0.

- [ ] **Step 3: Implement thin adapters**

Feed full winit facts into H0 while preserving existing transforms. Preserve the full button set, physical/logical key, repeat, modifiers, and trackpad facts by extending the app-handler match instead of reconstructing them downstream. The desktop event loop schedules `request_redraw`/wake at the session minimum deadline and pumps even while idle. The desktop-only adapter owns OS clipboard/OpenURL/Share fallback and drains immediately after input dispatch. Mobile-safe `op-host-native` remains free of desktop OS services.

- [ ] **Step 4: Run and commit**

```bash
cargo test -p op-host-native --features gl-host preview
cargo test -p op-host-desktop preview
git add crates/op-host-desktop/src/preview_effect_adapter.rs crates/op-host-desktop/src/main.rs crates/op-host-desktop/src/frame.rs crates/op-host-desktop/src/app_handler.rs crates/op-host-desktop/src/app_handler/pointer_events.rs crates/op-host-desktop/src/app_handler/keyboard_events.rs
git commit -m "feat(desktop): connect rich preview inputs and effects"
```

### Task H6: Web Preview Adapter and Trusted Browser Smoke

**Files:**
- Modify: `crates/op-host-web/Cargo.toml`
- Create: `crates/op-host-web/src/preview_input_adapter.rs`
- Create: `crates/op-host-web/src/preview_effect_adapter.rs`
- Create: `crates/op-host-web/src/preview_capabilities.rs`
- Modify: `crates/op-host-web/src/lib.rs`
- Modify: `crates/op-host-web/src/event/pointer.rs`
- Modify: `crates/op-host-web/src/canvaskit/mount.rs`
- Modify: `crates/op-host-web/src/canvaskit/mount_keyboard.rs`
- Modify: `crates/op-host-web/src/widget_host.rs`
- Create: `crates/op-host-web/src/widget_host/preview_frame_input.rs`
- Modify: `crates/op-host-web/src/widget_host/preview_frame.rs`
- Modify: `crates/op-host-web/src/widget_host/keyboard_ime.rs`
- Create: `crates/op-host-web/tests/preview_input_contract.rs`
- Create: `crates/op-host-web/tests/preview_effect_contract.rs`
- Create: `tools/check-preview-web-interactions.sh`
- Create: `tools/preview-web-interactions-smoke.mjs`

- [ ] **Step 1: Write failing Rust and trusted-browser tests**

Rust tests assert PointerEvent metadata, capture/release/lostpointercapture, touch multi-pointer, wheel sign/mode/phase, key/code/repeat/modifiers, CompositionEvent preedit/commit plus empty compositionend Cancel, lifecycle/capabilities, effect results, and timeout scheduling. The CDP smoke performs trusted mouse/key input against the real CanvasKit page and proves event/state output, popup/clipboard activation behavior, Unsupported Web Share, composition, an activation-expired delayed effect, and lone Tap/LongPress/delay completion without another browser event.

- [ ] **Step 2: Verify RED**

```bash
cargo test -p op-host-web --features canvaskit --test preview_input_contract
cargo test -p op-host-web --features canvaskit --test preview_effect_contract
bash tools/check-preview-web-interactions.sh
```

Expected: FAIL because browser Preview still uses mouse-like host methods.

- [ ] **Step 3: Implement adapters with synchronous activation drain**

Add the required web-sys ShareData/UserActivation features. Use browser Pointer Events, preserve pointerId/type/pressure/buttons/modifiers/timeStamp, and call set/releasePointerCapture; lostpointercapture emits Cancel. `mount_keyboard.rs` owns key/code/repeat and composition wiring. Composition events map to Preview IME; beforeinput is the direct-commit fallback and empty compositionend cancels. Schedule one `setTimeout`/rAF wake for the session minimum deadline and reschedule after pump. Clipboard, Web Share, and `window.open` must start before the original trusted listener returns; only their Promise completion may be asynchronous. Do not defer initial effect drain to a later rAF. Register `preview_frame_input` in `widget_host.rs`, move logic out of the already-over-cap `preview_frame.rs`, and reduce its line count.

- [ ] **Step 4: Run and commit**

```bash
cargo test -p op-host-web --features canvaskit --test preview_input_contract
cargo test -p op-host-web --features canvaskit --test preview_effect_contract
cargo check --target wasm32-unknown-unknown -p op-host-web --no-default-features --features canvaskit
bash tools/check-preview-web-interactions.sh
git add crates/op-host-web/Cargo.toml crates/op-host-web/src/preview_input_adapter.rs crates/op-host-web/src/preview_effect_adapter.rs crates/op-host-web/src/preview_capabilities.rs crates/op-host-web/src/lib.rs crates/op-host-web/src/event/pointer.rs crates/op-host-web/src/canvaskit/mount.rs crates/op-host-web/src/canvaskit/mount_keyboard.rs crates/op-host-web/src/widget_host.rs crates/op-host-web/src/widget_host/preview_frame_input.rs crates/op-host-web/src/widget_host/preview_frame.rs crates/op-host-web/src/widget_host/keyboard_ime.rs crates/op-host-web/tests/preview_input_contract.rs crates/op-host-web/tests/preview_effect_contract.rs tools/check-preview-web-interactions.sh tools/preview-web-interactions-smoke.mjs
git commit -m "feat(web): connect rich preview inputs and effects"
```

### Task H7: Declared Capability and Effect Conformance

**Files:**
- Modify: `crates/op-preview-contracts/src/capability.rs`
- Modify: `crates/op-preview-contracts/src/platform_support.rs`
- Modify: `crates/op-preview-contracts/src/tests.rs`
- Create: `crates/op-preview-core/src/tests_host_capabilities.rs`
- Modify: `crates/op-preview-core/src/lib.rs`
- Modify: `crates/op-engine-ffi/src/preview_input.rs`
- Modify: `crates/op-engine-ffi/src/preview_input_tests.rs`
- Modify: `packaging/ios/Sources/PreviewInputAdapter.swift`
- Modify: `packaging/ios/Sources/PreviewEffectAdapter.swift`
- Modify: `packaging/ios/Sources/OpEngineHost+Preview.swift`
- Modify: `packaging/ios/Tests/PreviewInputMappingTests.swift`
- Modify: `packaging/ios/Tests/PreviewEffectMappingTests.swift`
- Modify: `packaging/android/app/src/main/kotlin/tech/zseven/openpencil/PreviewInputAdapter.kt`
- Modify: `packaging/android/app/src/main/kotlin/tech/zseven/openpencil/PreviewEffectAdapter.kt`
- Modify: `packaging/android/app/src/main/kotlin/tech/zseven/openpencil/PreviewLifecycleAdapter.kt`
- Modify: `packaging/android/app/src/test/kotlin/tech/zseven/openpencil/PreviewInputMappingTest.kt`
- Modify: `packaging/android/app/src/test/kotlin/tech/zseven/openpencil/PreviewEffectMappingTest.kt`
- Modify: `packaging/harmony/entry/src/main/ets/common/PreviewInputAdapter.ets`
- Modify: `packaging/harmony/entry/src/main/ets/common/PreviewEffectAdapter.ets`
- Modify: `packaging/harmony/entry/src/main/ets/common/PreviewLifecycleAdapter.ets`
- Modify: `packaging/harmony/Tests/PreviewAdapterContractTests.rb`
- Modify: `crates/op-host-native/src/widget_host/preview_capabilities.rs`
- Modify: `crates/op-host-native/src/widget_host/preview_effects.rs`
- Modify: `crates/op-host-native/src/widget_host/preview_effect_tests.rs`
- Modify: `crates/op-host-native/src/widget_host/mode_transition_host.rs`
- Modify: `crates/op-host-web/src/preview_capabilities.rs`
- Modify: `crates/op-host-web/src/preview_effect_adapter.rs`
- Modify: `crates/op-host-web/src/preview_host.rs`
- Modify: `crates/op-host-web/tests/preview_effect_contract.rs`

- [ ] **Step 1: Write failing five-host matrix tests**

For iOS, Android, Harmony, Desktop, and Web assert Hover/ContextMenu/MultiTouch/Haptic/Share/Clipboard/OpenURL/HardwareKeyboard/IME values, touch fallback, right-click/long-press adaptation, and fail-closed unknown capability. For every PreviewEffect assert Supported starts one platform action and completes once; Unsupported produces one diagnostic and does not block later safe actions. Activation-sensitive effects must carry the matching input activation or fail as ActivationExpired.

- [ ] **Step 2: Verify RED**

```bash
cargo test -p op-preview-contracts capability
cargo test -p op-preview-core tests_host_capabilities
```

Expected: FAIL until every adapter registers an explicit matrix.

- [ ] **Step 3: Register capabilities through every host boundary**

Keep `PreviewHostCapabilities` without `Default`. Native/Web pass it to `enter_with_capabilities`; iOS/Android/Harmony call H1 through Swift/JNI/NAPI during engine setup and lifecycle re-creation. Platform availability badges use the same capability ids and declared adaptation table, not host-specific strings.

- [ ] **Step 4: Run and commit**

```bash
cargo test -p op-preview-contracts capability
cargo test -p op-preview-core tests_host_capabilities
bash packaging/ios/Tests/validate_sources.sh
./packaging/android/gradlew -p packaging/android testDebugUnitTest --tests '*Preview*'
ruby packaging/harmony/Tests/PreviewAdapterContractTests.rb
cargo test -p op-host-native --features gl-host preview
cargo test -p op-host-web --features canvaskit preview
git add crates/op-preview-contracts/src/capability.rs crates/op-preview-contracts/src/platform_support.rs crates/op-preview-contracts/src/tests.rs crates/op-preview-core/src/tests_host_capabilities.rs crates/op-preview-core/src/lib.rs crates/op-engine-ffi/src/preview_input.rs crates/op-engine-ffi/src/preview_input_tests.rs packaging/ios/Sources/PreviewInputAdapter.swift packaging/ios/Sources/PreviewEffectAdapter.swift packaging/ios/Sources/OpEngineHost+Preview.swift packaging/ios/Tests/PreviewInputMappingTests.swift packaging/ios/Tests/PreviewEffectMappingTests.swift packaging/android/app/src/main/kotlin/tech/zseven/openpencil/PreviewInputAdapter.kt packaging/android/app/src/main/kotlin/tech/zseven/openpencil/PreviewEffectAdapter.kt packaging/android/app/src/main/kotlin/tech/zseven/openpencil/PreviewLifecycleAdapter.kt packaging/android/app/src/test/kotlin/tech/zseven/openpencil/PreviewInputMappingTest.kt packaging/android/app/src/test/kotlin/tech/zseven/openpencil/PreviewEffectMappingTest.kt packaging/harmony/entry/src/main/ets/common/PreviewInputAdapter.ets packaging/harmony/entry/src/main/ets/common/PreviewEffectAdapter.ets packaging/harmony/entry/src/main/ets/common/PreviewLifecycleAdapter.ets packaging/harmony/Tests/PreviewAdapterContractTests.rb crates/op-host-native/src/widget_host/preview_capabilities.rs crates/op-host-native/src/widget_host/preview_effects.rs crates/op-host-native/src/widget_host/preview_effect_tests.rs crates/op-host-native/src/widget_host/mode_transition_host.rs crates/op-host-web/src/preview_capabilities.rs crates/op-host-web/src/preview_effect_adapter.rs crates/op-host-web/src/preview_host.rs crates/op-host-web/tests/preview_effect_contract.rs
git commit -m "feat(editor): declare preview host capabilities"
```
