# Statistics Service Progress

## ST-2 / WI-7: repeatable authorized-estimate cost harness — measured

- Artifact: [`statistics_estimate.rs`](../../engine/examples/statistics_estimate.rs) seeds a temporary READY redb database with a deterministic 8,192-row INTEGER relation, constructs an explicitly authorized projection, runs 11 estimates over fresh invocation snapshots, verifies each exact summary, and reports median/p95 call latency. Bootstrap and seed writes are outside the timed interval.
- Measurement: on Rust 1.95.0, redb 4.4.0, x86_64 Linux, release build: median 8.655 ms and p95 12.073 ms. The run measured the storage estimate API only; planner IPC, SQL planning, compilation, and query execution were excluded. This is a single host-local sample, not a performance target.
- Verification: `cargo run --release -p prdb-engine --example statistics_estimate --locked --offline` completed and verified the row/cell counts. Locked workspace build, format check, warning-denied Clippy, serial workspace tests (129 passed, 3 cgroup-dependent ignored), and maintained TinyCC SDK suite passed.
- Remaining: broader estimate-cost variation, plan-quality evidence, mixed-workload benchmarks, cache validity, and maintenance collection remain open.

## ST-2 / WI-7: estimate-cost scaling sample — measured

- Harness: [`statistics_estimate.rs`](../../engine/examples/statistics_estimate.rs) now seeds three authorized INTEGER relations with 1,024, 8,192, and 16,383 rows, then runs 11 exact authorized estimates per relation. Setup and seeding remain outside each timed call; all returned counts are checked.
- Measurement: locked Rust 1.95.0 / redb 4.4.0 on x86_64 Linux. Medians were 0.891 ms, 7.152 ms, and 14.653 ms respectively; p95 values were 1.401 ms, 8.126 ms, and 16.754 ms. The largest relation is one row below the 16,384 candidate budget.
- Verification: `cargo run --release -p prdb-engine --example statistics_estimate --locked --offline`, workspace formatting check, and warning-denied Clippy across all targets passed in the locked Nix shell.
- Scope: one host-local sample per size; OS cache state is uncontrolled. This is scaling evidence for the storage estimate call, not planner, query, service-level, or target-hardware performance.

## ST-4: candidate-budget boundary publishes no partial estimate — verified

- Contract: authorized scan estimates may return an exact summary below the 16,384-candidate budget, but must return `Unknown` once the budget is reached, without exposing partial counts.
- Regression: a real-redb fixture commits 16,383 readable rows and verifies their exact VALUE population; it then commits the 16,384th row and verifies a fresh snapshot receives `Unknown`.
- Verification: the focused boundary regression passed. Locked workspace build, formatting, warning-denied Clippy, serial workspace tests (129 passed, 3 cgroup-dependent ignored), and maintained TinyCC SDK suite passed in the configured Nix shell.
- Remaining: asynchronous planner-task draining, cache validity, bounded maintenance collection, and mixed-workload evidence remain open.

## ST-4: cumulative planner estimate allowance — implemented

- Behavior: each planner metadata stream may request at most 16 storage estimates. The counter survives detach/reattach of planner metadata session state. Once exhausted, a valid estimate request returns `Unknown` without aborting the invocation, allowing fixed optimizer heuristics. The separate per-estimate scan bound remains 16,384 candidate rows; cancellation and deadline errors remain terminal.
- Changed code and contract: [planner metadata session](../../engine/src/transaction.rs), [statistics service](../../specs/statistics-service.md), and [planner budget](../../specs/planner-compiler.md).
- Verification: the budget exhaustion unit test confirms the allowance rejects the next estimate without advancing the counter. The planner IPC integration test builds a typed table through storage DDL, requests 16 exact summaries over `serve_planner`, receives `Unknown` on estimate 17, then successfully describes the relation again and confirms the invocation remains active. Locked workspace build, format check, warning-denied Clippy, and serial full workspace tests passed; commit integration reported 129 passed and 3 cgroup-gated ignored tests. A parallel full-test run reproduced the existing `InvalidAudit` flake in `storage_manager_creates_and_drops_check_constraint`; the serial rerun passed.
- Remaining: overall planner search budgets, asynchronous task draining, cache validity, and bounded maintenance collection remain open. Supervisor provisioning of the planner stream is also not implemented.

## ST-3: hidden redacted cell values do not change estimate output — verified

- Regression: the authorized-estimate fixture now creates two concurrent invocation overlays with identical policy-visible data and row IDs, but different contents in a column redacted from the caller. A row-value-dependent visibility policy forces both results to `Unknown`; the test asserts the normalized outputs are identical.
- Verification: `nix develop --offline --command cargo test --test transaction authorized_scan_estimate_counts_sanitized_cells_and_private_writes --locked` passed. This checks returned estimate contents only; timing/cache noninterference is not established.
- Remaining: test hidden values and invisible rows through paired authorized reference views, then address planner task draining, cache validity, and bounded maintenance collection.

## ST-3 follow-up: invisible candidates preserve the authorized reference view — verified

- Contract: adding rows hidden by a value-dependent row policy must not change the planner-visible estimate when the authorized view is unchanged; unsupported policy shapes return `Unknown` without partial counts.
- Regression: `authorized_scan_estimate_counts_sanitized_cells_and_private_writes` compares two invocation snapshots with different hidden candidate counts. Both estimates are `Unknown`, and independent paged authorized scans return the same single visible row with its hidden cell represented only as `REDACTED`.
- Verification: the focused real-redb transaction test passed (`1 passed, 0 failed`).
- Scope: checks normalized estimate results and reference rows, not timing or resource-use noninterference. Planner task draining, cache validity, and bounded maintenance collection remain open.

## ST-2 / ST-3 follow-up: resolve statically empty policy populations — implemented

- Contract: exact estimates are safe when row inclusion is independent of stored values. Literal Boolean policies can establish either the full row population or an empty one; unsupported policy expressions remain `Unknown`.
- Red evidence: a real-redb estimate for an applicable literal-FALSE table ALLOW returned `Unknown`, despite storage being able to prove the authorized population was empty. After adding constant-policy recognition, a false RESTRICT followed by a value-dependent policy incorrectly returned an exact empty estimate; the implementation now checks every applicable policy and falls back to `Unknown` if any policy is unsupported.
- Implementation: estimates return zero visible rows and zero cell populations without scanning when no applicable literal-TRUE ALLOW exists or a literal-FALSE RESTRICT applies. The full scan path remains limited to the all-rows case.
- Regression: the shared estimate fixture compares the exact empty summary with two independent authorized reference scans over different hidden candidate counts. It also confirms a constant denial cannot mask a later value-dependent policy.
- Verification: focused real-redb transaction test passed (`1 passed, 0 failed`).
- Scope: all other row-value-dependent or unsupported policies still return `Unknown`; timing/resource noninterference, planner task draining, cache validity, and bounded maintenance collection remain open.

## ST-2 follow-up: serialize exact empty estimates over the planner stream — verified

- Regression: `planner_stream_estimate_allowance_falls_back_without_aborting_invocation` stages a row in the invocation overlay, applies a constant-FALSE row ALLOW policy, and requests estimates over the real planner IPC stream. The decoded exact response reports zero visible rows and zero cell populations; request 17 still returns `Unknown` without aborting the invocation.
- Verification: focused real-redb planner-stream integration test passed (`1 passed, 0 failed`).
- Scope: verifies planner request/response encoding and the shared estimate allowance. Isolated optimizer consumption and heuristic use of estimates remain open.

## ST-1 / ST-2 dependency: planner metadata and estimate dispatch

- Behavior: planner-only connection handles resolve and revalidate authorized relation descriptions, and estimate requests return bounded visible/readable/null/redacted summaries or UNKNOWN. Planner projections derive read grants and policies without loading write constraints or indexes; executor handles cannot be decoded as planner handles.
- Changed code and contracts: [planner IPC](../../engine/src/planner_ipc.rs), [planner connection state](../../engine/src/transaction.rs), [planner handle](../../engine/src/metadata_discovery.rs), [IPC protocol](../../specs/ipc-protocol.md), and [statistics service](../../specs/storage-manager.md).
- Verification: workspace build, format check, warning-denied Clippy, and `git diff --check` pass. Tests were not run in this increment.
- Remaining: planner stream provisioning, planner task lifecycle/budget integration, and predicate/access-path estimates remain open.

## ST-1 / ST-2 dependency: planner stream lifecycle

- Behavior: `serve_planner` loops over one provisioned planner stream, enforces unique request IDs and a 256-request cap, and preserves kind-9 handle state across requests. Clean EOF closes only the planner session so execution can continue; malformed frames and protocol violations durably abort the invocation.
- Changed code and contract: [planner stream and dispatcher](../../engine/src/planner_ipc.rs) and [planner IPC contract](../../specs/ipc-protocol.md).
- Verification: format, workspace build, and warning-denied Clippy pass. Tests were not run in this increment.
- Remaining: supervisor provisioning, shared planner/executor invocation scheduling, and cancellation-aware planner task draining remain open.

## ST-1 / WI-7 dependency: estimate cost across candidate populations — measured

- Harness: [`statistics_estimate.rs`](../../engine/examples/statistics_estimate.rs) seeds 1,024, 8,192, and 16,383 deterministic authorized INTEGER rows and times 11 exact estimate calls per dataset, excluding bootstrap, seeding, compilation, and planner IPC.
- Profile and result: Rust/Cargo 1.95.0, redb 4.4.0, TinyCC 0.9.27-unstable-2025-01-06, Linux x86_64. Median/p95 call latency was 0.901/0.975 ms at 1,024 rows, 7.847/11.258 ms at 8,192 rows, and 14.202/16.845 ms at 16,383 rows, just below the 16,384 candidate ceiling.
- Scope: one local run on one host. These figures show scaling of the storage estimate call for this simple authorized shape; they are not planner, service-level, mixed-workload, or target-hardware performance claims. Repeated host/filesystem samples and broader policy/workload shapes remain open.
- Verification: `nix-shell -p rustc cargo clippy rustfmt tinycc --run 'cargo run -p prdb-engine --release --example statistics_estimate --locked'` completed successfully.

## ST-1 / WI-7 follow-up: estimate-cost run-to-run sample — measured

- Method: ran the already-built release harness three times sequentially; each run built and removed a fresh temporary database and took 11 estimate samples at each configured population size.
- Run-level median latency across the three repetitions: 0.921 ms at 1,024 rows (range 0.899–0.930), 7.460 ms at 8,192 rows (7.124–7.587), and 15.943 ms at 16,383 rows (14.979–23.795). Median of run-level p95 values: 1.406, 9.170, and 16.738 ms respectively; the 16,383-row p95 range was 15.835–27.299 ms.
- Scope: repetitions share one host and filesystem; cache state and competing system load are uncontrolled. The wide largest-dataset range confirms this is local comparative evidence, not a stable service target or target-hardware profile.
- Verification: all three executions of `target/release/examples/statistics_estimate` exited successfully.
