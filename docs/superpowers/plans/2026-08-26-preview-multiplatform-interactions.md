# Preview Multiplatform Interactions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver one backwards-compatible Preview interaction system whose gestures, actions, bindings, animations, authoring UI, diagnostics, and system effects behave consistently on Desktop, Web, iOS, Android, and Harmony.

**Architecture:** Extend the existing Jian schema, Gesture Arena, and ActionRegistry rather than creating an OpenPencil-only interpreter. `op-preview-core` becomes the shared projection, invalidation, animation, trace, and effect-drain layer; every host is a thin InputAdapter/EffectAdapter. The editor Interact Tab, AI, and MCP all write the same validated schema.

**Tech Stack:** Rust/Cargo, Jian (`jian-ops-schema`, `jian-core`), OpenPencil Rust editor/preview crates, Swift/UIKit, Kotlin/Android, ArkTS/Harmony, CanvasKit/Web APIs.

---

## Plan Set and Dependency Order

1. [Runtime Foundation](2026-08-26-preview-runtime-foundation.md)
2. [Interact Tab and Preview Debugger](2026-08-26-preview-interact-debugger.md)
3. [Client and Host Adapters](2026-08-26-preview-client-host-adapters.md)
4. [Parity, Lifecycle, and Rollout](2026-08-26-preview-parity-rollout.md)

Resolve Parity task P1 first so the pre-existing DSH candidate no longer rides in the worktree without ownership. Then use these dependency gates:

- R0 first creates room in the PreviewSession spine without behavior change; R1-R4 then freeze schema, gesture, policy/effect, capability, and input contracts.
- R5-R9 complete the safe action catalog, binding projection, animation, transition input, and Debug APIs.
- E1 starts after R1. E2 requires R5; E3 requires E1/E2/R3; E4-E6 follow E3 and E6 also requires R7. E7 requires R9; E8 requires E1/E2/R7; E9 follows all authoring/Debugger widgets.
- H0 starts after R3/R4/R7/R9 and freezes the shared mobile-safe shell conduit. H1 then layers FFI/redacted trace ABI over H0. P2 starts after R1-R9. H2/H3 require H1/P2/E3-E7 for their complete client authoring/Debugger gates; H4 requires H1/P2; H5 requires H0/P2; H6 requires R9/P2. H2-H6 may otherwise run in parallel. H7 runs after all five hosts register their capability/effect adapters; E7 extends H0's chrome gate before the iOS/Android full UI gates.
- P3 follows H7; P4 follows R/E compatibility work; P5 follows E/H rendering; P6 follows R9 plus Rust hosts; P7 follows P6 plus H2-H4; P8 follows R3/R6/R7/R9; P9 follows every gate.

## Shared Ownership Rules

- Main agent owns public types, cross-plan API changes, integration, conflicts, and final verification.
- One worker owns one file family; workers must not edit another worker's files without a handoff.
- DSH workers implement bounded tasks from the linked plans and commit only their owned files.
- The existing uncommitted `close_preview_owned_overlays()` candidate is not assumed correct. It is reviewed in P1 and must never be bundled into unrelated Runtime work.
- Every behavior change uses RED -> GREEN -> REFACTOR. A task does not start production edits until its named failing test has been observed.
- Commits follow the exact commit message shown in each task unless integration requires one conflict-resolution commit.

## Jian Submodule Commit Protocol

`vendor/jian` is a mode-160000 Git submodule pinned at `c3308344943fb598058d00678e412ed5195608f8`, currently detached. Before R1, create the implementation branch inside the submodule:

```bash
git -C vendor/jian status --short
git -C vendor/jian switch -c codex/preview-interactions-20260826 c3308344943fb598058d00678e412ed5195608f8
```

For every task that changes Jian:

1. Run Jian tests with `--manifest-path vendor/jian/Cargo.toml` from the root.
2. Stage and commit Jian-owned paths with `git -C vendor/jian add ...` and `git -C vendor/jian commit ...`.
3. From the OpenPencil root, stage only the `vendor/jian` gitlink plus OpenPencil-owned files; never run `git add vendor/jian/crates/...` from the root.
4. Keep the paired Jian/OpenPencil commits adjacent and record the Jian SHA in review evidence.
5. Do not push the Jian branch or any OpenPencil commit that references an unreachable Jian SHA without separate user authorization. Before an authorized OpenPencil push/PR, push Jian first, verify the SHA is reachable on its remote, then push the root gitlink commit.

## Integration Checkpoints

### Checkpoint A: Contract Freeze

Required tasks: R0-R4.

Run:

```bash
cargo test --manifest-path vendor/jian/Cargo.toml -p jian-ops-schema
cargo test --manifest-path vendor/jian/Cargo.toml -p jian-core gesture
cargo test --manifest-path vendor/jian/Cargo.toml -p jian-core action
cargo test -p op-preview-core
```

Expected: all tests pass; new public types are documented; old schema fixtures round-trip as lossless semantic JSON.

### Checkpoint B: End-to-End Authoring

Required tasks: E1-E9 plus R5-R9.

Run:

```bash
cargo test -p op-editor-core interaction
cargo test -p op-editor-ui property_panel_interaction
cargo test -p op-mcp
cargo test -p op-chat-agent interaction
cargo test -p op-preview-core
```

Expected: Interact Tab creates, edits, reorders, disables, and preserves actions; Preview executes the resulting document without a format adapter.

### Checkpoint C: Client Parity

Required tasks: H0-H7.

Run the exact commands in the adapter plan, then replay the shared trace corpus on every host.

Expected: semantic event/state/route/effect outputs match the canonical trace; platform-only capabilities produce declared adaptations or structured rejection.

### Checkpoint D: Release Gate

Required tasks: all parity tasks.

Run:

```bash
cargo fmt --all -- --check
cargo fmt --manifest-path vendor/jian/Cargo.toml --all -- --check
cargo test --manifest-path vendor/jian/Cargo.toml -p jian-ops-schema -p jian-core
cargo test -p op-preview-contracts -p op-preview-core
cargo test -p op-editor-core -p op-editor-ui
cargo test -p op-mcp -p op-chat-agent -p op-ai-skills
cargo test -p op-host-native --features gl-host preview
cargo test -p op-host-web --features canvaskit preview
cargo test -p op-engine-ffi --features editor preview
bash packaging/ios/Tests/validate_sources.sh
```

Then run the Android and Harmony commands in the host-adapter plan.

Expected: zero failures, no unknown-field loss, bounded trace/debug queues, and a completed support matrix with evidence links.

## Commit Sequence

The intended sequence is:

1. Review and land or reject the isolated Preview-entry hygiene candidate (P1).
2. Behavior-neutral PreviewSession spine split (R0).
3. Schema compatibility, lifecycle ownership, and event catalog.
4. Gesture semantics, event payloads, and deterministic timers.
5. Preview action policy, capability, effect, and activation contracts.
6. Canonical PreviewInput (R4).
7. Safe action catalog, bindings/invalidation, animation, transition input, and Debug API (R5-R9).
8. Shared mobile-safe native shell conduit (H0).
9. Additive FFI plus redacted trace ABI (H1).
10. Interaction document commands and lossless ActionBuilder.
11. Canonical cross-platform trace corpus (P2).
12. Interact Tab structured editors and responsive Preview Debugger.
13. AI/MCP authoring parity and localization/accessibility.
14. iOS adapter and simulator tests.
15. Android adapter and instrumentation tests.
16. Harmony adapter and device contract.
17. Desktop/Web adapter parity and declared capability matrix.
18. Host trace conformance plus lifecycle cancellation.
19. Visual, performance, real-device, and support-matrix gates.

Do not squash until all review evidence is preserved; each commit is independently revertible.
