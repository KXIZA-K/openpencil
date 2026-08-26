# Preview Parity, Lifecycle, and Rollout Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prove the rich Preview interaction system is backwards-compatible, lifecycle-safe, visually correct, performant, and semantically equivalent across all supported hosts.

**Architecture:** A source-controlled interactive `.op` fixture plus canonical JSON input traces and expected normalized outputs are replayed by Core and every adapter. Preview cancellation is shared; platform UI cancellation remains explicit. Deterministic tests, rendered snapshots, simulator/emulator runs, and real-device evidence are separate gates.

**Tech Stack:** Rust/Cargo, JSON/.op fixtures, Skia/CanvasKit snapshots, Swift/Kotlin/ArkTS contracts, iOS Simulator, Android Emulator, Harmony/real-device QA.

---

### Task P1: Review and Land or Reject the Existing Preview-Entry Candidate

**Files:**
- Review current uncommitted changes in: `crates/op-editor-core/src/host_escape_transitions.rs`
- Review current uncommitted changes in: `crates/op-host-native/src/widget_host/preview_slideshow.rs`
- Review current uncommitted changes in: `crates/op-host-web/src/widget_host/preview_slideshow.rs`
- Test: `crates/op-editor-core/src/host_escape_transitions.rs`
- Modify: `crates/op-host-native/src/widget_host/mode_transition_host.rs`
- Modify: `crates/op-host-native/src/widget_host/preview_slideshow_tests.rs`
- Test: `crates/op-host-web/src/widget_host/preview_slideshow.rs`
- Modify: `crates/op-host-web/src/widget_host/press_chrome_tiers.rs`
- Modify: `crates/op-host-web/src/widget_host/slides_panel.rs`
- Modify: `crates/op-host-web/src/widget_host/slides_panel_tests.rs`
- Modify: `crates/op-host-web/src/widget_host/preview_frame.rs`
- Create: `crates/op-host-web/src/widget_host/preview_frame_teardown.rs`
- Modify: `crates/op-host-web/src/widget_host.rs`
- Modify: `crates/op-host-web/src/figma_temp_bridge.rs`
- Modify: `crates/op-host-desktop/src/app_handler/redraw.rs`
- Modify: `crates/op-host-desktop/src/figma_import_session/tests.rs`

- [ ] **Step 1: Inventory the external DSH patch before any other implementation**

```bash
git diff -- crates/op-editor-core/src/host_escape_transitions.rs crates/op-host-native/src/widget_host/preview_slideshow.rs crates/op-host-web/src/widget_host/preview_slideshow.rs
```

Classify every closed field as transient or persistent. Persistent theme, locale, pinned style, account snapshot, document state, panel sizes, and authored Preview data must survive.

- [ ] **Step 2: Add safety tests before correcting the candidate**

Assert cleanup runs before every ordinary/deck Preview runtime build, after committing valid rename/text/property drafts. Cover transient settings/import/export/file menus, shape/icon/asset/prompt/font surfaces, login/account/collab overlays, text/property focus, keyboard ownership, drag/capture, and orphan hover/composition. Native performs real auth/touch cancellation and queues one Figma Cancel; Web queues its real daemon auth cancel, invalidates the import generation, and does not claim a native side effect. Cmd+P drains Figma Cancel before the worker pump. Repeated entry is idempotent, failed builds release ownership, and exit/re-entry has no stale focus/IME/capture.

- [ ] **Step 3: Run the evidence tests**

```bash
cargo test -p op-editor-core preview_cleanup
cargo test -p op-host-native --features gl-host preview_slideshow_tests
cargo test -p op-host-web --features canvaskit preview_slideshow
cargo test -p op-host-web --features canvaskit slides_panel
cargo test -p op-host-desktop cmd_p_preview_cancels_late_figma_import_before_pump
```

Expected: either a focused failure identifies an incomplete/over-broad candidate, or all tests pass and no production correction is needed. Do not manufacture a RED by changing a correct candidate.

- [ ] **Step 4: Make only evidence-required corrections and commit**

```bash
cargo test -p op-editor-core preview_cleanup
cargo test -p op-host-native --features gl-host preview_slideshow_tests
cargo test -p op-host-web --features canvaskit preview_slideshow
cargo test -p op-host-web --features canvaskit slides_panel
cargo test -p op-host-desktop cmd_p_preview_cancels_late_figma_import_before_pump
git add crates/op-editor-core/src/host_escape_transitions.rs crates/op-host-native/src/widget_host/mode_transition_host.rs crates/op-host-native/src/widget_host/preview_slideshow.rs crates/op-host-native/src/widget_host/preview_slideshow_tests.rs crates/op-host-web/src/figma_temp_bridge.rs crates/op-host-web/src/widget_host.rs crates/op-host-web/src/widget_host/press_chrome_tiers.rs crates/op-host-web/src/widget_host/preview_frame.rs crates/op-host-web/src/widget_host/preview_frame_teardown.rs crates/op-host-web/src/widget_host/preview_slideshow.rs crates/op-host-web/src/widget_host/slides_panel.rs crates/op-host-web/src/widget_host/slides_panel_tests.rs crates/op-host-desktop/src/app_handler/redraw.rs crates/op-host-desktop/src/figma_import_session/tests.rs docs/superpowers/plans/2026-08-26-preview-parity-rollout.md
git commit -m "fix(editor): clear editing surfaces on preview entry"
```

If review rejects all three production edits, restore only those exact candidate hunks, retain the regression tests in a test-only commit, and record why.

### Task P2: Canonical Document and Cross-Platform Trace Corpus

**Files:**
- Create: `crates/op-preview-core/tests/fixtures/interaction-catalog.op`
- Create: `crates/op-preview-core/tests/fixtures/input_traces/tap-double-long.json`
- Create: `crates/op-preview-core/tests/fixtures/input_traces/pan-swipe.json`
- Create: `crates/op-preview-core/tests/fixtures/input_traces/scale-rotate.json`
- Create: `crates/op-preview-core/tests/fixtures/input_traces/hover-context.json`
- Create: `crates/op-preview-core/tests/fixtures/input_traces/keyboard-ime.json`
- Create: `crates/op-preview-core/tests/fixtures/input_traces/scroll-reach-end.json`
- Create: `crates/op-preview-core/tests/fixtures/input_traces/lifecycle-effects.json`
- Create: `crates/op-preview-core/src/trace_fixture.rs`
- Modify: `crates/op-preview-core/src/lib.rs`
- Create: `crates/op-preview-core/tests/trace_replay.rs`

- [ ] **Step 1: Write the failing fixture runner**

Every trace names the same source document and scenario root:

```json
{
  "document": "../interaction-catalog.op",
  "scenario": "tap-counter",
  "viewport": [390, 844],
  "hostCapabilities": "ios-touch",
  "inputs": [
    {"at":0,"pointer":{"id":1,"kind":"touch","phase":"down","x":100,"y":100}},
    {"at":32,"pointer":{"id":1,"kind":"touch","phase":"up","x":100,"y":100}}
  ],
  "expect": {
    "events":[{"name":"onPressStart","event":{"pointerType":"touch"}},{"name":"onPressEnd"},{"name":"onTap"}],
    "state":{"$app.count":1},
    "route":["/"],
    "animations":[],
    "effects":[]
  }
}
```

Define named explicit profiles for iOS touch, Android touch, Harmony touch, Desktop pointer, and Web pointer in the fixture runner; no profile uses a default. Cover exact event payloads, ordering, state diffs, routes, animations, effects/results, capability adaptations, lifecycle, Back/Enter, Chinese IME, and delayed timers.

- [ ] **Step 2: Verify RED**

```bash
cargo test -p op-preview-core --test trace_replay
```

Expected: FAIL because the public fixture parser/replayer does not exist.

- [ ] **Step 3: Implement public fixture parsing/replay**

Register `trace_fixture` in `lib.rs`. Use only public PreviewInput/debug APIs. Resolve document paths relative to the trace file. Reject non-monotonic timestamps, duplicate live Down, Up without Down, unknown inputs, missing scenario nodes, and expected fields not supported by the declared capability matrix with path-specific errors.

- [ ] **Step 4: Run and commit**

```bash
cargo test -p op-preview-core --test trace_replay
git add crates/op-preview-core/tests/fixtures/interaction-catalog.op crates/op-preview-core/tests/fixtures/input_traces crates/op-preview-core/src/trace_fixture.rs crates/op-preview-core/src/lib.rs crates/op-preview-core/tests/trace_replay.rs
git commit -m "test(editor): add canonical preview interaction traces"
```

### Task P3: Five-Host Trace Conformance

**Files:**
- Create: `crates/op-host-native/tests/preview_trace_contract.rs`
- Create: `crates/op-host-web/tests/preview_trace_contract.rs`
- Modify: `packaging/ios/Tests/PreviewInputMappingTests.swift`
- Modify: `packaging/ios/Tests/PreviewEffectMappingTests.swift`
- Modify: `packaging/ios/UITests/PreviewInteractionUITests.swift`
- Modify: `packaging/android/app/src/test/kotlin/tech/zseven/openpencil/PreviewInputMappingTest.kt`
- Modify: `packaging/android/app/src/test/kotlin/tech/zseven/openpencil/PreviewEffectMappingTest.kt`
- Modify: `packaging/android/app/src/androidTest/kotlin/tech/zseven/openpencil/PreviewInteractionInstrumentedTest.kt`
- Modify: `crates/op-engine-jni/src/preview_contract.rs`
- Modify: `crates/op-engine-napi/src/preview_contract.rs`
- Modify: `packaging/harmony/Tests/PreviewAdapterContractTests.rb`

- [ ] **Step 1: Add host comparisons**

Deterministic conformance feeds P2 primitive platform samples through native/web adapters, Swift/Kotlin pure mappers, and host-testable JNI/NAPI contract mappers; it compares normalized input JSON, then replays that JSON through Core and compares exact semantic payload/state/route/animation/effect output via H1's redacted trace API. XCUITest/Instrumentation/device gestures are separate UI/effect evidence and are not used as a deterministic UITouch/MotionEvent clock. Differences are allowed only when the capability/adaptation table says Adapted or Unsupported; expected traces are not edited to excuse an adapter defect.

- [ ] **Step 2: Run host conformance**

```bash
cargo test -p op-host-native --features gl-host --test preview_trace_contract
cargo test -p op-host-web --features canvaskit --test preview_trace_contract
cargo test -p op-engine-jni preview_contract
cargo test -p op-engine-napi preview_contract
bash packaging/ios/Tests/validate_sources.sh
./packaging/android/gradlew -p packaging/android testDebugUnitTest --tests '*Preview*'
bash tools/check-preview-web-interactions.sh
bash packaging/ios/Tests/run_preview_simulator_tests.sh
bash packaging/android/Tests/run_preview_emulator_tests.sh
ruby packaging/harmony/Tests/PreviewAdapterContractTests.rb
bash packaging/harmony/Tests/run_preview_build.sh
```

Expected: PASS on installed host/simulator/emulator lanes. A missing DevEco/device prerequisite exits explicitly and leaves Harmony build/device cells Untested, never Complete.

- [ ] **Step 3: Correct adapters, rerun, and commit conformance tests**

```bash
cargo test -p op-host-native --features gl-host --test preview_trace_contract
cargo test -p op-host-web --features canvaskit --test preview_trace_contract
cargo test -p op-engine-jni preview_contract
cargo test -p op-engine-napi preview_contract
bash packaging/ios/Tests/validate_sources.sh
./packaging/android/gradlew -p packaging/android testDebugUnitTest --tests '*Preview*'
bash tools/check-preview-web-interactions.sh
bash packaging/ios/Tests/run_preview_simulator_tests.sh
bash packaging/android/Tests/run_preview_emulator_tests.sh
ruby packaging/harmony/Tests/PreviewAdapterContractTests.rb
bash packaging/harmony/Tests/run_preview_build.sh
git add crates/op-host-native/tests/preview_trace_contract.rs crates/op-host-web/tests/preview_trace_contract.rs packaging/ios/Tests/PreviewInputMappingTests.swift packaging/ios/Tests/PreviewEffectMappingTests.swift packaging/ios/UITests/PreviewInteractionUITests.swift packaging/android/app/src/test/kotlin/tech/zseven/openpencil/PreviewInputMappingTest.kt packaging/android/app/src/test/kotlin/tech/zseven/openpencil/PreviewEffectMappingTest.kt packaging/android/app/src/androidTest/kotlin/tech/zseven/openpencil/PreviewInteractionInstrumentedTest.kt crates/op-engine-jni/src/preview_contract.rs crates/op-engine-napi/src/preview_contract.rs packaging/harmony/Tests/PreviewAdapterContractTests.rb
git commit -m "test(editor): enforce preview host input parity"
```

Commit any adapter correction separately in its H-task file family before this test commit.

### Task P4: Legacy and Future-Data Compatibility Gate

**Files:**
- Create: `crates/op-preview-core/tests/fixtures/legacy_interactions/no-interactions.op`
- Create: `crates/op-preview-core/tests/fixtures/legacy_interactions/tap-navigation.op`
- Create: `crates/op-preview-core/tests/fixtures/legacy_interactions/forms-tabs-counters.op`
- Create: `crates/op-preview-core/tests/fixtures/legacy_interactions/future-fields.op`
- Create: `crates/op-preview-core/tests/legacy_interactions.rs`
- Modify: `crates/op-editor-core/src/interaction_command_tests.rs`

- [ ] **Step 1: Add compatibility goldens**

Load, Preview, edit one unrelated property, save, reload, and compare semantic JSON. Cover documents with no interactions; legacy onTap/navigation/forms/tabs/counters; unknown event/lifecycle/gesture/action; and a known action body with future fields plus `interactionOrder/disabledEvents`.

- [ ] **Step 2: Run the post-R1/E1 regression gate**

```bash
cargo test -p op-preview-core --test legacy_interactions
cargo test -p op-editor-core interaction_unknown
```

Expected: PASS because compatibility fixes already landed in R1/E1/E2. Any failure is a regression; fix the owning seam and rerun rather than weakening the golden.

- [ ] **Step 3: Commit the gate**

```bash
git add crates/op-preview-core/tests/fixtures/legacy_interactions crates/op-preview-core/tests/legacy_interactions.rs crates/op-editor-core/src/interaction_command_tests.rs
git commit -m "test(editor): preserve preview interaction data"
```

### Task P5: Runtime, Authoring, Debugger, and Client Visual Evidence

**Files:**
- Create: `crates/op-preview-core/tests/preview_scene_snapshots.rs`
- Create: `crates/op-preview-core/tests/snapshots/preview_scene/`
- Create: `crates/op-editor-ui/src/widgets/preview_interaction_visual_tests.rs`
- Create: `crates/op-editor-ui/src/widgets/snapshots/preview_paint_ops/`
- Modify: `crates/op-editor-ui/src/widgets/mod.rs`
- Create: `crates/op-host-native/tests/preview_interaction_capture.rs`
- Create: `crates/op-host-native/tests/snapshots/preview_interactions/`
- Create: `crates/op-host-web/tests/snapshots/preview_interactions/`
- Create: `tools/capture-preview-interactions.sh`
- Create: `tools/package-preview-evidence.sh`

- [ ] **Step 1: Add deterministic visual cases**

Pin viewport, scale, font registry, and clock. Core snapshots normalized scene/runtime overlay JSON; op-editor-ui snapshots deterministic RenderBackend paint operations; the native GL/raster integration test owns reviewed PNGs. The CDP capture path saves and compares CanvasKit Web PNGs against `op-host-web/tests/snapshots/preview_interactions/`. Cover Idle/Hover/Pressed/Focus/LongPress/Pan/touch fallback; overlay/effect feedback; animation 0/50/100%; push/pop/fade/modal; Interact cards/pickers/editors; Debugger side rail and phone sheet. Core/UI never acquire a native raster dependency.

- [ ] **Step 2: Verify missing/mismatched snapshots**

```bash
cargo test -p op-preview-core --test preview_scene_snapshots
cargo test -p op-editor-ui preview_interaction_visual
cargo test -p op-host-native --features gl-host --test preview_interaction_capture
```

Expected: FAIL until reviewed scene/paint-op snapshots and native PNGs exist.

- [ ] **Step 3: Capture clients and review originals**

```bash
bash tools/capture-preview-interactions.sh
```

The script captures CanvasKit Web, iPhone and iPad Simulator, plus any booted Android Emulator into `out/preview-interactions/web-canvaskit/`, `ios-iphone/`, `ios-ipad/`, and `android-emulator/` with viewport/device metadata. Inspect every PNG at original resolution. Captures are evidence, not golden replacements; do not approve hit/layout drift. `package-preview-evidence.sh` later copies reviewed representative frames/log summaries into the source-controlled P9 evidence tree and writes SHA-256 metadata.

- [ ] **Step 4: Run and commit reviewed goldens**

```bash
cargo test -p op-preview-core --test preview_scene_snapshots
cargo test -p op-editor-ui preview_interaction_visual
cargo test -p op-host-native --features gl-host --test preview_interaction_capture
bash tools/capture-preview-interactions.sh
git add crates/op-preview-core/tests/preview_scene_snapshots.rs crates/op-preview-core/tests/snapshots/preview_scene crates/op-editor-ui/src/widgets/preview_interaction_visual_tests.rs crates/op-editor-ui/src/widgets/snapshots/preview_paint_ops crates/op-editor-ui/src/widgets/mod.rs crates/op-host-native/tests/preview_interaction_capture.rs crates/op-host-native/tests/snapshots/preview_interactions crates/op-host-web/tests/snapshots/preview_interactions tools/capture-preview-interactions.sh tools/package-preview-evidence.sh
git commit -m "test(editor): cover rich preview interaction visuals"
```

### Task P6: Core and Rust-Host Lifecycle Cancellation

**Files:**
- Modify: `vendor/jian/crates/jian-core/src/runtime.rs`
- Modify: `vendor/jian/crates/jian-core/src/runtime/async_runtime.rs`
- Modify: `vendor/jian/crates/jian-core/src/action/task_queue.rs`
- Modify: `vendor/jian/crates/jian-core/src/gesture/router.rs`
- Create: `crates/op-preview-core/src/tests_lifecycle_rich.rs`
- Modify: `crates/op-preview-core/src/lib.rs`
- Modify: `crates/op-preview-core/src/input.rs`
- Modify: `crates/op-preview-core/src/effects.rs`
- Modify: `crates/op-preview-core/src/animation.rs`
- Modify: `crates/op-preview-core/src/transition.rs`
- Modify: `crates/op-preview-core/src/debug_trace.rs`
- Modify: `crates/op-host-native/src/widget_host/mode_transition_host.rs`
- Modify: `crates/op-host-native/src/widget_host/scene_state.rs`
- Modify: `crates/op-host-native/src/widget_host/preview_slideshow.rs`
- Modify: `crates/op-host-native/src/widget_host/preview_slideshow_tests.rs`
- Modify: `crates/op-host-web/src/widget_host/preview_slideshow.rs`
- Modify: `crates/op-host-web/src/widget_host/preview_frame.rs`

- [ ] **Step 1: Write failing cancellation tests**

Start active pointers/capture, animation, delay, confirm, IME composition, deferred transition input, route navigation, and Debug trace. Exercise exit, document replacement, Background, Terminate, and destruction. Assert tasks/effects/focus/capture/gesture/animation/deferred input cancel exactly once; re-entry begins at defaults and entry route.

- [ ] **Step 2: Verify RED**

```bash
cargo test -p op-preview-core lifecycle_rich
cargo test -p op-host-native --features gl-host preview_lifecycle
cargo test -p op-host-web --features canvaskit preview_lifecycle
```

Expected: FAIL on any lingering owner.

- [ ] **Step 3: Implement one shared cancellation sink**

`Runtime::cancel_all(reason)` clears gesture arenas/timers and action tasks. `PreviewSession::cancel_all(reason)` increments a session generation, calls Runtime cancellation, clears animations/deferred input/IME/focus, completes pending effects as Cancelled, and is called from every core/host exit path. It does not mutate the source document or persistent editor settings.

- [ ] **Step 4: Run and commit**

```bash
cargo test -p op-preview-core lifecycle_rich
cargo test -p op-host-native --features gl-host preview_lifecycle
cargo test -p op-host-web --features canvaskit preview_lifecycle
git -C vendor/jian add crates/jian-core/src/runtime.rs crates/jian-core/src/runtime/async_runtime.rs crates/jian-core/src/action/task_queue.rs crates/jian-core/src/gesture/router.rs
git -C vendor/jian commit -m "fix(renderer): cancel interaction owners together"
git add vendor/jian crates/op-preview-core/src/tests_lifecycle_rich.rs crates/op-preview-core/src/lib.rs crates/op-preview-core/src/input.rs crates/op-preview-core/src/effects.rs crates/op-preview-core/src/animation.rs crates/op-preview-core/src/transition.rs crates/op-preview-core/src/debug_trace.rs crates/op-host-native/src/widget_host/mode_transition_host.rs crates/op-host-native/src/widget_host/scene_state.rs crates/op-host-native/src/widget_host/preview_slideshow.rs crates/op-host-native/src/widget_host/preview_slideshow_tests.rs crates/op-host-web/src/widget_host/preview_slideshow.rs crates/op-host-web/src/widget_host/preview_frame.rs
git commit -m "fix(editor): cancel preview runtime across lifecycle changes"
```

### Task P7: Client Lifecycle and Platform-UI Cancellation

**Files:**
- Modify: `crates/op-engine-ffi/src/preview_input.rs`
- Modify: `crates/op-engine-ffi/src/preview_effect.rs`
- Modify: `crates/op-engine-ffi/src/lifecycle.rs`
- Create: `crates/op-engine-ffi/src/preview_lifecycle.rs`
- Modify: `crates/op-engine-ffi/src/lib.rs`
- Modify: `packaging/ios/Sources/OpEngineHost+Preview.swift`
- Modify: `packaging/ios/Sources/PreviewInputAdapter.swift`
- Modify: `packaging/ios/Sources/PreviewEffectAdapter.swift`
- Modify: `packaging/ios/Tests/PreviewEffectMappingTests.swift`
- Modify: `packaging/android/app/src/main/kotlin/tech/zseven/openpencil/PreviewLifecycleAdapter.kt`
- Modify: `packaging/android/app/src/main/kotlin/tech/zseven/openpencil/PreviewEffectAdapter.kt`
- Modify: `packaging/android/app/src/androidTest/kotlin/tech/zseven/openpencil/PreviewInteractionInstrumentedTest.kt`
- Modify: `packaging/harmony/entry/src/main/ets/common/PreviewLifecycleAdapter.ets`
- Modify: `packaging/harmony/entry/src/main/ets/common/PreviewEffectAdapter.ets`
- Modify: `packaging/harmony/Tests/PreviewAdapterContractTests.rb`

- [ ] **Step 1: Add platform cancellation assertions**

Background/destroy during Share/Alert/Confirm/keyboard/IME/multi-touch must dismiss platform ownership, send lifecycle/cancel input, and complete effect ids exactly once as Cancelled. Recreate/foreground must register capabilities again and start at defaults. Put new Preview cancellation logic in `preview_lifecycle.rs` and move an existing lifecycle block out so the 799-line `lifecycle.rs` becomes a smaller delegating spine.

- [ ] **Step 2: Run client tests**

```bash
cargo test -p op-engine-ffi --features editor preview_lifecycle
bash packaging/ios/Tests/run_preview_simulator_tests.sh
bash packaging/android/Tests/run_preview_emulator_tests.sh
ruby packaging/harmony/Tests/PreviewAdapterContractTests.rb
```

Expected: PASS on installed lanes; missing real Harmony environment remains Untested.

- [ ] **Step 3: Commit**

```bash
git add crates/op-engine-ffi/src/preview_input.rs crates/op-engine-ffi/src/preview_effect.rs crates/op-engine-ffi/src/lifecycle.rs crates/op-engine-ffi/src/preview_lifecycle.rs crates/op-engine-ffi/src/lib.rs packaging/ios/Sources/OpEngineHost+Preview.swift packaging/ios/Sources/PreviewInputAdapter.swift packaging/ios/Sources/PreviewEffectAdapter.swift packaging/ios/Tests/PreviewEffectMappingTests.swift packaging/android/app/src/main/kotlin/tech/zseven/openpencil/PreviewLifecycleAdapter.kt packaging/android/app/src/main/kotlin/tech/zseven/openpencil/PreviewEffectAdapter.kt packaging/android/app/src/androidTest/kotlin/tech/zseven/openpencil/PreviewInteractionInstrumentedTest.kt packaging/harmony/entry/src/main/ets/common/PreviewLifecycleAdapter.ets packaging/harmony/entry/src/main/ets/common/PreviewEffectAdapter.ets packaging/harmony/Tests/PreviewAdapterContractTests.rb
git commit -m "fix(editor): cancel preview clients across lifecycle changes"
```

### Task P8: Performance and Bounded-Work Evidence

**Files:**
- Create: `crates/op-preview-core/benches/interaction_dispatch.rs`
- Create: `crates/op-preview-core/benches/animation_tick.rs`
- Modify: `crates/op-preview-core/Cargo.toml`
- Modify: `Cargo.lock`
- Create: `crates/op-preview-core/src/tests_performance_invariants.rs`
- Modify: `crates/op-preview-core/src/lib.rs`
- Create: `docs/performance/preview-interactions.md`
- Create: `tools/capture-preview-frame-budget.sh`

- [ ] **Step 1: Add benchmarks and structural tests**

Add Criterion as a dev-dependency and explicit `[[bench]] ... harness = false` entries. Register the invariant test module in the slim `lib.rs`. Measure PaintOnly, HitTest, Relayout, 60Hz Pan, two-pointer Scale/Rotate, 100 tracks, trace wrap, and effect queue bounds. Unit tests assert PaintOnly never relayouts, every queue is bounded, and one session exposes one minimum wake deadline across gesture/action/caret/animation/transition sources.

- [ ] **Step 2: Run baselines**

```bash
cargo bench -p op-preview-core --bench interaction_dispatch
cargo bench -p op-preview-core --bench animation_tick
cargo test -p op-preview-core performance_invariants
bash tools/capture-preview-frame-budget.sh
```

Record median/p95, reference hardware, fixture, and target frame budget. The capture script replays the same 60Hz Pan/Scale/animation scenario and collects iOS frame/Time Profiler evidence, Android `dumpsys gfxinfo` frame stats, and Harmony frame data through available DevEco/hidumper tooling into `out/preview-interactions/performance/`. Missing attached devices are explicit Untested outputs. Do not add flaky wall-clock CI assertions.

- [ ] **Step 3: Commit evidence**

```bash
git add Cargo.lock crates/op-preview-core/benches/interaction_dispatch.rs crates/op-preview-core/benches/animation_tick.rs crates/op-preview-core/Cargo.toml crates/op-preview-core/src/tests_performance_invariants.rs crates/op-preview-core/src/lib.rs tools/capture-preview-frame-budget.sh
git add -f docs/performance/preview-interactions.md
git commit -m "perf(editor): bound preview interaction and animation work"
```

### Task P9: Evidence-Backed Support Matrix and Final Verification

**Files:**
- Create: `docs/preview-interaction-support.md`
- Create: `docs/testing/preview-interactions/evidence/README.md`
- Create: `docs/testing/preview-interactions/evidence/manifest.json`
- Create: `docs/testing/preview-interactions/evidence/web-canvaskit.png`
- Create: `docs/testing/preview-interactions/evidence/ios-iphone-simulator.png`
- Create: `docs/testing/preview-interactions/evidence/ios-ipad-simulator.png`
- Create: `docs/testing/preview-interactions/evidence/android-emulator.png`
- Create: `docs/testing/preview-interactions/evidence/frame-budget-summary.json`

- [ ] **Step 1: Build the support matrix from evidence**

Run `bash tools/package-preview-evidence.sh` after reviewed captures. It copies representative Web/iPhone/iPad/Android frames plus the frame-budget summary into the listed source-controlled paths and writes a manifest containing SHA-256, device/OS, viewport, scenario, commit SHA, source test, status, and CI artifact URL for full raw logs. Hash verification is part of the script. Missing real-device evidence is a manifest Untested row, not a fabricated file.

```bash
bash tools/package-preview-evidence.sh
```

Rows are every trigger, action, binding target, system effect, lifecycle hook, and Debugger/authoring feature. Columns are Core, Desktop, Web, iOS Simulator, iOS Device, Android Emulator, Android Device, Harmony Build, and Harmony Device. Cells are Complete, Adapted, Unsupported, or Untested and link to a deterministic test plus the durable evidence manifest/capture where UI/device proof is required. Simulator never satisfies a Device cell.

- [ ] **Step 2: Run the full deterministic gate**

```bash
cargo fmt --manifest-path vendor/jian/Cargo.toml --all -- --check
cargo test --manifest-path vendor/jian/Cargo.toml -p jian-ops-schema -p jian-core
cargo fmt --all -- --check
cargo test -p op-preview-contracts -p op-preview-core
cargo test -p op-editor-core -p op-editor-ui
cargo test -p op-mcp -p op-chat-agent -p op-ai-skills
cargo test -p op-host-native --features gl-host preview
cargo test -p op-host-web --features canvaskit preview
cargo test -p op-engine-ffi --features editor preview
cargo check --target wasm32-unknown-unknown -p op-preview-contracts -p op-preview-core -p op-editor-ui
cargo check --target wasm32-unknown-unknown -p op-host-web --no-default-features --features canvaskit
bash tools/check-widget-boundary.sh
bash tools/check-wasm-bundle.sh
wc -l vendor/jian/crates/jian-core/src/expression/aot.rs crates/op-editor-core/src/command_apply.rs crates/op-editor-core/src/editor_ui_state.rs crates/op-preview-core/src/lib.rs crates/op-engine-ffi/src/lifecycle.rs crates/op-ai-skills/src/lib.rs crates/op-host-native/src/widget_host.rs crates/op-host-web/src/widget_host/paint.rs crates/op-host-web/src/widget_host/preview_frame.rs packaging/ios/Sources/OpEngineHost.swift packaging/ios/Sources/OpPlayerView.swift packaging/android/app/src/main/kotlin/tech/zseven/openpencil/OpSurfaceView.kt packaging/harmony/entry/src/main/ets/common/EngineHost.ets | awk '$2 != "total" && $1 > 800 { print; bad=1 } END { exit bad }'
bash tools/check-preview-web-interactions.sh
bash packaging/ios/Tests/validate_sources.sh
bash packaging/ios/Tests/run_preview_simulator_tests.sh
./packaging/android/gradlew -p packaging/android testDebugUnitTest --tests '*Preview*'
bash packaging/android/Tests/run_preview_emulator_tests.sh
ruby packaging/harmony/Tests/PreviewAdapterContractTests.rb
bash packaging/harmony/Tests/run_preview_build.sh
```

Expected: zero failures on available lanes. Exit-2 missing SDK/device prerequisites are recorded as Untested with command output, never silently ignored.

- [ ] **Step 3: Perform real-device QA**

Replay P2 on iPhone/iPad, Android phone/tablet, and a Harmony device. Capture normalized input/state/effect logs and final frames. Verify Chinese IME, hardware keyboard where available, multi-touch, Share/Haptic, lifecycle interruption, safe-area layout, and buttons outside the chat field.

On Harmony, also perform Interact add/edit/reorder/save/reopen/readback and Debugger Run/Pause/Reset. If no capable device/build is available, all Harmony authoring cells remain Untested rather than inheriting shared-UI test status.

Run `bash tools/capture-preview-frame-budget.sh` with each target device attached and link its 60Hz evidence in the corresponding Device cells.

- [ ] **Step 4: Commit the matrix**

```bash
git add -f docs/preview-interaction-support.md docs/testing/preview-interactions/evidence
git commit -m "docs(editor): publish preview interaction support matrix"
```
