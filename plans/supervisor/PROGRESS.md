# Supervisor Progress

## SU-9 follow-up: reconcile terminal storage errors without blocking — verified

- Regression: a cancellation accepted during a slow result relay can race with successful worker exit, causing the subsequent commit request to fail with `CancellationRequested` after the transaction records ABORTED. The terminal helper now joins the storage handler before reading its response, so a handler error cannot leave the caller waiting forever for a frame that was never sent; the gateway then confirms and relays the durable outcome. Observation errors also attempt the normal storage abort path before outcome lookup.
- Verification: five sequential fresh-database delegated-cgroup runs of `sandboxed_v3_sql_dml_sequence_is_visible_and_commits` passed (`1 passed, 0 ignored` each). In each, cancellation arrived during 4-MiB output, all 256 provisional chunks were received, and the terminal was ABORTED within 506–511 ms. The regression reproduced the previous hang before the handler-join fix.
- Scope: this covers local supervisor/gateway IPC and durable terminal reconciliation on the delegated Linux x86_64 host. TCP client backpressure and target-hardware behavior remain open.

## SU-7 follow-up: preserve START progress across principal quota blocks — verified

- Behavior: a sealed START retry keeps its original queue ticket when the caller's per-principal active quota is full. A later ticket from another principal may use independently available class capacity; when the earlier ticket becomes eligible, it retains priority over later retries.
- Regression: `start_queue_skips_principal_blocked_ticket_without_losing_its_turn` holds one operational slot for a principal, queues that principal's second invocation, starts another principal in the remaining class slot, then releases the first invocation and verifies the queued retry starts before completion cleanup.
- Verification: all 17 `supervisor::tests` passed with zero ignored; workspace formatting, warning-denied Clippy, and `git diff --check` passed.
- Scope: this covers deterministic staging-level queue fairness. Cross-process saturation and end-to-end fairness under live multi-client analytics remain open.

## SU-9 follow-up: keep delegated sandbox acceptances in the normal test suite — verified

- Correction: the four Linux x86_64 sandbox acceptances in `engine/tests/commit.rs` and the sandbox process acceptance in `engine/tests/metadata_process.rs` are active `#[test]` cases with no ignore attributes. Updated the supervisor specification to state that they run in the normal workspace suite when delegated cgroup and runtime prerequisites are configured.
- Verification: the delegated cgroup probe reported a writable cgroup v2 parent with `cpu`, `memory`, and `pids` available and enabled. `cargo test --workspace --locked -- --test-threads=1` passed through `engine/tools/with_delegated_cgroup.sh`; the commit integration target reported 142 passed and 0 ignored, including all four sandbox acceptances. The full workspace run completed successfully with no ignored tests.

## SU-2: per-class cgroup resource profiles — implemented

- Behavior: deployments may configure an optional analytical per-invocation cgroup profile with CPU quota/period, memory, and process-count ceilings. Sealed `workload_class` selects the profile before cgroup creation; operational remains the required default and analytical falls back to it when no separate profile is configured.
- Security/consistency: the class comes from immutable sealed submission metadata, and START still revalidates reservation ownership/state after cgroup setup. The class selects a trusted configured ceiling; it does not grant additional storage authority or override any limit.
- Interface: `prdb-supervisor` accepts the complete optional flag set `--analytical-cpu-quota-us`, `--analytical-cpu-period-us`, `--analytical-memory-max-bytes`, and `--analytical-max-processes`; partial sets are rejected.
- Verification: `cargo test -p prdb-engine --lib supervisor_ipc::worker_result_event_tests::workload_class_selects_the_configured_cgroup_profile --locked --offline -- --exact` passed. Locked workspace build, rustfmt check, warning-denied Clippy, and serial workspace tests passed in the documented Nix shell; cgroup-dependent sandbox tests remained ignored.
- Remaining: class-wide active invocation quotas are implemented below. Separate queue reservations/depth limits and analytical prefetch/cache controls remain unimplemented.

## SU-2 / SU-7: class-wide and per-principal active invocation quotas — implemented

- Behavior: START atomically reserves a slot by immutable sealed workload class, counting `Starting` and `Started` reservations. Global defaults are 64 operational and 8 analytical active invocations; per-principal defaults are 16 operational and 2 analytical. Deployments configure the four limits with `--max-active-operational`, `--max-active-analytical`, `--max-active-operational-per-principal`, and `--max-active-analytical-per-principal`. At either cap, START returns a resource-limit error before storage admission and preserves the sealed reservation for retry. Slots release when a started reservation reaches durable finish/cleanup.
- Changed code and contract: [staging state machine](../../engine/src/supervisor.rs), [supervisor CLI](../../engine/src/bin/prdb-supervisor.rs), and [workload profile](../../specs/workload-profile.md).
- Verification: deterministic staging tests check same-class rejection, per-principal active rejection, independent principal/class capacity, prepared retention, and retry after slot release; supervisor help coverage checks all four flags. In the pinned Nix shell, locked workspace build, format check, warning-denied Clippy, and `cargo test --workspace --locked --offline` passed; commit integration reported 128 passed and 3 cgroup-gated ignored tests.
- Remaining: bounded wait queues and analytical prefetch/cache controls remain open.

## SU-2 / SU-7: per-principal staging reservation quota — implemented

- Behavior: PREPARE atomically enforces a live reservation ceiling per authenticated principal in addition to the global count and byte quotas. The default is 128 per principal; `--max-principal-reservations` configures it. All live staging and active states count. A denied PREPARE creates no reservation, and cancellation/expiry/disconnect/terminal cleanup releases capacity.
- Changed code and contract: [staging state machine](../../engine/src/supervisor.rs), [supervisor CLI](../../engine/src/bin/prdb-supervisor.rs), and [supervisor admission contract](../../specs/supervisor.md).
- Verification: the deterministic principal-isolation/cancellation regression and supervisor help test passed. In the pinned Nix shell, locked workspace build, format check, warning-denied Clippy, and `cargo test --workspace --locked --offline` passed; commit integration reported 128 passed and 3 cgroup-gated ignored tests.
- Remaining: execution wait-queue scheduling/fair progress, overall planner search budgets, and analytical prefetch/cache controls remain open. The storage estimate allowance is tracked in the [statistics plan progress](../statistics-service/PROGRESS.md).

## SU-2 / SU-7: class-bounded pending submission backlog — implemented

- Behavior: PREPARE counts uploading, sealing, sealed, and discard-requested reservations against independent operational and analytical pending limits. Defaults are 256 operational and 64 analytical; `--max-pending-operational` and `--max-pending-analytical` configure them. At the class limit PREPARE returns a resource-limit error without creating a reservation. Starting/started work uses the active invocation quotas above.
- Changed code and contract: [staging state machine](../../engine/src/supervisor.rs), [supervisor CLI](../../engine/src/bin/prdb-supervisor.rs), and [workload profile](../../specs/workload-profile.md).
- Verification: deterministic staging test checks cap rejection, class independence, cancellation release, and successful retry; supervisor help coverage checks both flags. In the pinned Nix shell, locked workspace build, format check, warning-denied Clippy, and `cargo test --workspace --locked --offline` passed; commit integration reported 128 passed and 3 cgroup-gated ignored tests.
- Remaining: execution wait-queue scheduling/fair progress, overall planner search budgets, and analytical prefetch/cache controls remain open. The storage estimate allowance is tracked in the [statistics plan progress](../statistics-service/PROGRESS.md).

## SU-2 dependency: bounded submission staging

- Behavior: `StagingStore` binds each reservation to an authenticated connection/principal, enforces one reservation per connection, caps artifact/parameter sizes and chunk lengths, reserves global/per-principal quotas without preallocating declared bytes, requires exact contiguous offsets, validates content lengths and SHA-256 at seal, and exposes immutable bytes only during one-shot start admission. Disconnect reclaims unstarted reservations and returns started IDs for cancellation. Unstarted reservations have configurable expiry and an explicit reaper.
- Contract: [client upload protocol](../../specs/client-protocol.md) and [supervisor lifecycle](../../specs/supervisor.md).
- Changed code: [bounded staging state machine](../../engine/src/supervisor.rs) and [engine module export](../../engine/src/lib.rs).
- Decisions: protocol hard limits remain 64 MiB artifact, 1 MiB parameters, 16 KiB manifest, 8 KiB scope, and 256 KiB chunks. Staging defaults to 256 MiB global reserved bytes, 65 MiB plus metadata limit per principal, 1,024 global reservations, 128 reservations per principal, and five-minute unstarted TTL; deployments may lower byte limits or adjust TTL.
- Verification: Nix-shell format, workspace build, warning-denied Clippy, and `git diff --check` pass. Tests were not run.
- Remaining: provide a concrete manifest/scope validator, connect authenticated gateway sessions, integrate START revalidation with storage admission and sandbox launch, and run upload/disconnect/expiry race coverage when tests are authorized.

## SU-2 dependency: prepared status and cancellation

- Behavior: owner-scoped staging STATUS distinguishes upload/seal/prepared/start states while hiding foreign IDs. CANCEL removes an unstarted reservation without creating a storage transaction and marks an in-progress seal for discard; active work is returned to the storage cancellation path.
- Changed code and contract: [staging state machine](../../engine/src/supervisor.rs) and [supervisor staging semantics](../../specs/supervisor.md).
- Verification: workspace format, build, warning-denied Clippy, and `git diff --check` pass in the repository Nix shell. Tests were not run.
- Remaining: connect STATUS/CANCEL to authenticated gateway frames and cover concurrent seal/cancel behavior in runtime tests when tests are authorized.

## SU-5 verification: drain paired worker channels before commit

- Evidence: a real-redb test closes planner/executor Unix peers, serves both channels, then sends FINALIZE through the supervisor-only terminal stream and verifies the durable owner outcome is COMMITTED.
- Test: [paired worker drain and terminal commit](../../engine/tests/commit.rs).
- Remaining: worker process launch/exit observation must decide whether successful user return justifies FINALIZE; clean socket EOF by itself is not evidence of process success.

## SU-2 verification: staging state-machine lifecycle and races

- Behavior: internal tests cover owner isolation, exact chunk offsets, seal digest/metadata acceptance, one-shot START, quota release after finish, prepared cancellation, disconnect and TTL reclamation, and cancellation while metadata validation is blocked outside the staging lock.
- Changed fixture: [staging unit tests](../../engine/src/supervisor.rs).
- Verification: `cargo test -p prdb-engine --lib supervisor::tests --locked` passed all 4 staging tests in the repository Nix shell.
- Remaining: bind the authenticated gateway protocol to staging and exercise quota limits under concurrent connections.

## SU-2 dependency: preserve workload class and deadline

- Behavior: PREPARE now stores `deadline_ms` and operational/analytical `workload_class` in the sealed submission metadata. It rejects zero or over-ceiling deadlines; the default ceiling is 15 minutes and is configurable.
- Changed code and contract: [staging metadata](../../engine/src/supervisor.rs) and [supervisor admission contract](../../specs/supervisor.md).
- Verification: staging tests cover deadline ceiling rejection and preservation of the accepted class/deadline through START.
- Remaining: queue reservations/depth limits and analytical prefetch/cache controls remain open; per-invocation cgroup selection and active invocation quotas are implemented above.

## SU-3 dependency: token-bound START and deadline enforcement

- Behavior: a sealed owner-scoped submission invokes storage token admission exactly once; the storage invocation receives the submitted deadline, rechecks it during idle-channel polling, and commits no effects for a revoked pre-START credential.
- Changed code and fixture: [client START callback](../../engine/src/client_protocol.rs), [storage deadline owner](../../engine/src/storage_manager.rs), and [START regression](../../engine/tests/commit.rs).
- Verification: focused real-redb test covers no snapshot/status before START, deadline ABORT after admission, and revoked token rejection with no outcome.
- Remaining: actual worker process launch, process exit observation, and result relay.

## SU-3 / SB-1: sandbox capability preflight before storage admission — implemented

- Behavior: `StagingStore::start_with_sandbox_preflight` probes the configured cgroup parent and checks the complete required sandbox profile before calling storage admission. If the profile is unsupported, the callback is not entered, no storage snapshot is created, and the sealed reservation remains prepared for a later retry.
- Regression: [supervisor staging test](../../engine/src/supervisor.rs) points the preflight at an ordinary temporary directory and verifies it rejects before the callback and preserves the prepared state.
- Contract: [supervisor START lifecycle](../../specs/supervisor.md) distinguishes capability preflight from installing confinement.
- Verification: focused `sandbox_preflight_rejects_before_storage_admission` test passed in the configured Nix shell.
- Remaining: the real START path does not yet use a worker launcher; passing this probe alone does not establish confinement. Implement namespace, Landlock, seccomp, cgroup, clean descriptor/environment setup, and process launch before opening any artifact.

## SU-3 / SB-1: provision worker cgroup before storage admission — implemented

- Behavior: `StagingStore::start_with_worker_cgroup` validates the prepared reservation, checks the host profile, and creates/configures the invocation cgroup before entering the one-shot START callback. Setup errors leave the reservation prepared and do not enter storage admission. The callback receives ownership of the cgroup and must complete confinement and process launch.
- Regression: an ordinary directory used as the cgroup parent is rejected before callback entry; the prepared reservation remains available. This verifies the fail-closed setup boundary only, not successful cgroup provisioning or process launch.
- Contract: [supervisor START lifecycle](../../specs/supervisor.md).
- Verification: the focused regression passed. In the repository Nix shell, `cargo build --workspace --locked`, `cargo fmt --all -- --check`, warning-denied workspace Clippy, all workspace tests, and `git diff --check` passed.
- Remaining: connect the callback to executor command construction/spawn and process-exit observation; verify successful launch on a writable delegated cgroup host before treating the worker path as integrated.

## SU-3 / EX-1: prepare and spawn the executor after admission — implemented, IPC provisioning pending

- Behavior: `start_and_launch_executor` creates the cgroup before consuming START, materializes the artifact privately, seals the parameter memfd, prepares Landlock and pre-exec restrictions, calls the supplied admission callback, then spawns only the trusted executor host. The returned `RunningExecutor` kills/reaps on drop and exposes process-exit observation that delegates cgroup emptiness and success authorization to `SupervisorInvocation`.
- Regression: artifact staging checks read/execute-only permissions and directory removal; parameter tests verify all required memfd seals, rewind, and write denial. An unsupported cgroup profile rejects before the storage callback and retains the prepared reservation. No artifact was loaded.
- Contract: [supervisor lifecycle](../../specs/supervisor.md) and [executor lifecycle](../../specs/query-worker.md).
- Verification: launcher unit tests, authenticated START fail-closed regression, and gateway listener integration passed; after routing START, workspace build, format check, warning-denied Clippy, all workspace tests, and `git diff --check` passed in the repository Nix shell.
- Limitation: this machine's cgroup mount is read only, so successful process launch, real cgroup cleanup, and dynamic loading remain unverified. Runtime filesystem paths are explicit caller configuration and need a pinned deployment profile.
- Remaining: gateway START and paired worker channel provisioning are integrated. Add documented host callbacks to a new query ABI version, result relay, then validate a real launch on a writable delegated cgroup host before claiming process-level integration.

## SU-5 / EX-1: bind worker storage channels to the executor lifetime — implemented, ABI callbacks pending

- Behavior: each prepared executor owns separate planner and executor Unix socket pairs. The launcher passes only the worker endpoints through the cleaned descriptor list and command line; `RunningExecutor::observe_exit` serves the trusted endpoints concurrently through `SupervisorInvocation`, waits for both streams to drain, then reaps the process and checks cgroup emptiness before authorizing success.
- Regression: launcher unit coverage verifies both connected pairs and distinct descriptors. The existing storage-manager drain/finalize regression covers concurrent handler drain and refusal to finalize before worker exit observation. No user library was loaded.
- Contract: [supervisor lifecycle](../../specs/supervisor.md), [query worker](../../specs/query-worker.md), and [storage manager](../../specs/storage-manager.md).
- Limitation: successful cgroup-backed spawn remains unavailable on this host; ABI v1 has no callbacks, so SDK query code cannot make documented requests over these channels.
- Remaining: define and implement a new sized query ABI with bounded executor/planner operations and output relay, then exercise the actual C callback path inside the complete sandbox harness.

## SU-2 / NP-3: real gateway submission upload and seal — verified

- Behavior: the supervised service accepts authenticated TCP submission traffic from the real gateway binary, stages TinyCC artifact bytes and confirms PMF1/PSC1/ELF validation at SEAL.
- Regression: the binary integration compiles an ABI-v3 TinyCC fixture, authenticates using the bootstrap credential, uploads it through PREPARE/chunk, and verifies SEAL returns `prepared`. It does not load or execute the fixture.
- Verification: full workspace build, formatting, warning-denied Clippy, tests and maintained TinyCC SDK tests passed.
- Remaining: real START and confined process execution are gated on a host with a writable delegated cgroup; this machine's cgroup v2 mount is read only.

## SU-2 / NP-5: prepared reservation control over TCP — verified

- Behavior: the authenticated real gateway path reports a sealed upload as prepared, cancels it without creating an invocation outcome, then hides it as unknown on STATUS.
- Regression and verification: included in the real supervisor/gateway TinyCC upload test; workspace build, format, Clippy, Rust tests and SDK tests pass.
- Verification update: the confined SQL acceptance verifies the query itself receives a positive SDK cancellation poll response and relays `CANCEL_OBSERVED` before ABORTED. The supervisor now preserves executor channels for a 500 ms cooperative exit window, then forcefully reaps a remaining worker; hard deadlines remain active during that window. The delegated-cgroup TCP process test starts an invocation for a separately created query user, observes it as active through an authenticated bootstrap-administrator control session, accepts administrative CANCEL, kills the gateway before terminal delivery, and reconfirms ABORTED after restarting the supervisor/storage process. Its non-cooperative query reaches durable ABORTED within the two-second acceptance bound.
- Verification correction: `control_cancel_over_tcp_reports_too_late_during_durable_commit` now explicitly covers the control-mode TCP response after the writer enters its durable commit boundary. The focused test passes; the full locked workspace suite also passed (129 passed, 4 cgroup-dependent ignored).
- Remaining: broaden process-level worker failure and resource cleanup coverage.

## SU-3 / SB-1: process-boundary START preflight failure — verified

- Behavior: with a regular directory configured as the cgroup parent, the real binaries reject START with the sanitized `unavailable` error before storage admission; a fresh authenticated session reports `unknown` for the invocation.
- Regression: covered in the TinyCC upload/status/cancel binary test. The fixture is compiled and sealed, then discarded without loading or execution.
- Verification: the focused integration and full Rust/TinyCC validation pass. This proves the fail-closed boundary on an invalid host profile, not successful worker launch.
- Verification update: successful cgroup provisioning, confinement installation, and execution now pass on a delegated-cgroup Linux host. The configured SQL commit, SQL durable-abort, authenticated metadata TCP, and disconnected-submission gateway-restart acceptances all ran through confined query execution. During acceptance, supervisor cleanup exposed an ordering bug: the optimizer child was still a zombie when cgroup emptiness was checked. The supervisor now waits for and reaps the optimizer before checking the worker cgroup, with bounded kill-and-reap handling on timeout. The dedicated optimizer reaping unit test and all four ignored process acceptances pass. Full locked workspace build, format check, warning-denied Clippy, serial workspace tests, and maintained TinyCC tests pass.
- Remaining: continue process-level worker failure and resource cleanup coverage; broaden sandbox acceptance across the remaining SQL and cancellation behaviors.

## SU-5 / SM-6: metadata control reads across real service processes — verified

- Behavior: an authenticated `MetadataClient` can read the public schema, relations, and column descriptions through separate `prdb-gateway` and `prdb-supervisor` processes backed by a READY storage instance.
- Regression: [`metadata_process.rs`](../../engine/tests/metadata_process.rs) exercises the real binaries and bearer-token control path. It does not start or load a query worker.
- Verification: `cargo test --locked --test metadata_process` passed in the configured Nix shell.
- Remaining: this covers metadata control reads only; process-level query execution still depends on successful cgroup provisioning and the executor callback ABI path.
## WI-7 / SM-5: reserve storage snapshots from analytical saturation — implemented

- Behavior: storage admission tracks the immutable sealed workload class and enforces a shared analytical snapshot cap (default 8) in addition to the global active snapshot cap (default 128). Operational admissions do not consume the analytical allowance and can use the remaining global slots. The two limits are shared across manager handles for one database object.
- Regression: `analytical_admission_cap_preserves_operational_capacity` proves the second analytical admission is rejected at a cap of one while an operational admission still succeeds under a global cap of three.
- Contract: [workload profile](../../specs/workload-profile.md) and [storage manager](../../specs/storage-manager.md).
- Limitation: this is an admission reserve, not a fair queue. Operational requests can still be rejected when the global cap is full; queue depth, wait scheduling, and progress guarantees under saturation remain open under WI-7.

## WI-7 / SM-5: configure storage snapshot caps in the service — implemented

- Behavior: `prdb-supervisor` exposes positive `--max-storage-active-snapshots` and `--max-storage-active-analytical-snapshots` settings and passes them to every storage manager created by the service, including after recovery/reopen. Defaults remain 128 total and 8 analytical snapshots.
- Regression: `supervisor_help_documents_explicit_runtime_and_compatibility_configuration` checks both options are documented in `--help`. Supervisor unit tests verify defaults, explicit CLI values, and rejection of zero for either limit.
- Contract: [supervisor CLI](../../engine/src/bin/prdb-supervisor.rs), [storage manager](../../specs/storage-manager.md), and [workload profile](../../specs/workload-profile.md).

## WI-7 / SU-7: bounded fair START retry queue — implemented

- Behavior: the first START attempt assigns a ticket to its sealed reservation. A retry can consume the next available active slot only if it is the earliest eligible ticket. Entries blocked by a full class or per-principal quota are skipped, so other configured allowances make progress. Tickets are removed when a reservation is cancelled, expires, disconnects, or starts. The queue is bounded by existing live-reservation and class pending quotas; capacity responses remain retryable resource-limit results and do not block a gateway-service thread.
- Red/green evidence: `start_retries_preserve_fifo_order_within_eligible_capacity` failed before the queue implementation because a later reservation took the freed slot. It now verifies non-barging order. `canceling_a_queued_start_releases_its_turn` checks cancellation does not strand a later eligible submission; the per-principal quota regression checks an unrelated principal can use otherwise available class capacity.
- Contract: [supervisor](../../specs/supervisor.md) and [workload profile](../../specs/workload-profile.md).
- Limitation: admission is retry-driven; the service does not asynchronously wake queued clients. This does not guarantee latency for storage commits, point reads, cancellation, or reclamation under host-wide resource saturation.

## WI-7 / SM-5: validate staging and storage admission capacity together — implemented

- Behavior: supervisor startup rejects a storage analytical snapshot cap below the configured analytical START cap or a total storage cap below the combined operational and analytical START caps. This prevents a configured staging slot from being consumed by a guaranteed storage-level admission rejection.
- Regression: `storage_caps_must_cover_configured_supervisor_active_admissions` accepts defaults and rejects undersized analytical and total storage caps.
- Contract: [supervisor](../../specs/supervisor.md), [storage manager](../../specs/storage-manager.md), and [workload profile](../../specs/workload-profile.md).

## SM-4 / SU-5 dependency: supervisor terminal storage channel

- Behavior: role-checked Supervisor handlers finalize or abort one out-of-band-bound invocation, look up owner-scoped status, and request audited administrative cancellation with the administrator bound outside the payload. Terminal operations consume the invocation, finalize returns the durable commit sequence, and losing the terminal channel before a decision durably aborts. Executor EOF only closes its session.
- Changed code and contract: [supervisor IPC handlers](../../engine/src/supervisor_ipc.rs), [executor stream lifecycle](../../engine/src/executor_ipc.rs), and [IPC protocol](../../specs/ipc-protocol.md).
- Verification: format, workspace build, warning-denied Clippy, and `git diff --check` pass. Tests were not run in this increment.
- Remaining: implement worker exit observation, planner/executor multiplexing, and gateway cancellation wiring to the audited admin operation.

## SU-1 / SU-5 dependency: token-bound supervisor invocation owner

- Behavior: `StorageManager::admit_supervisor_invocation` verifies a database token, pins identity and role authority, and returns an owner that binds planner/executor channels to one invocation and exposes terminal and control channels. Dropping the owner without finalization attempts a durable abort.
- Changed code and contract: [storage manager admission wrapper](../../engine/src/storage_manager.rs) and [storage manager lifecycle contract](../../specs/storage-manager.md).
- Verification: format, workspace build, warning-denied Clippy, and `git diff --check` pass. Tests were not run in this increment.
- Remaining: launch/reap a confined worker, observe worker completion, and wire gateway cancellation; those remain supervisor scheduler responsibilities.

## SU-5 dependency: shared planner/executor invocation scheduling

- Behavior: planner and executor IPC streams may run concurrently against one supervisor-owned invocation. Each request takes the shared transaction lock only for dispatch, and per-stream opaque handle/cursor state survives between requests. Executor clean EOF releases its scans without deciding the invocation outcome; protocol failures abort.
- Changed code and contract: [shared IPC handlers](../../engine/src/planner_ipc.rs), [executor session state](../../engine/src/executor_ipc.rs), [transaction connection state](../../engine/src/transaction.rs), [supervisor invocation owner](../../engine/src/storage_manager.rs), and [storage-manager contract](../../specs/storage-manager.md).
- Verification: `cargo fmt --all`, workspace build, warning-denied Clippy, and `git diff --check` pass in the repository Nix shell. Tests were not run.
- Remaining: the supervisor must stop and drain worker channels before terminal finalization; process scheduling, worker exit observation, and gateway cancellation remain open.

## SU-5 dependency: fail-closed shared invocation lock poisoning

- Behavior: if a planner or executor request panics while holding the shared invocation lock, planner/executor handlers and the terminal owner consume the poisoned invocation and attempt durable abort; terminal handling cannot recover the lock and commit interrupted state.
- Changed code: [shared planner/executor handlers](../../engine/src/planner_ipc.rs), [executor request loop](../../engine/src/executor_ipc.rs), and [terminal owner](../../engine/src/storage_manager.rs).
- Verification: `cargo fmt --all`, workspace build, warning-denied Clippy, and `git diff --check` pass in the repository Nix shell. Tests were not run.
- Remaining: runtime tests should cover panic poisoning and abort outcome when test execution is authorized.

## SU-5 dependency: drain worker channels before terminal decisions

- Behavior: the supervisor-owned storage context tracks active planner and executor streams. A terminal request while either stream remains open returns `WorkerChannelsActive`; once terminal handling begins, no new worker stream is admitted.
- Changed code and contract: [supervisor invocation lifecycle](../../engine/src/storage_manager.rs) and [storage-manager process boundary](../../specs/storage-manager.md).
- Verification: `cargo fmt --all`, workspace build, warning-denied Clippy, and `git diff --check` pass in the repository Nix shell. Tests were not run.
- Remaining: supervisor worker-exit observation must close these streams before requesting commit or abort; runtime race coverage awaits an authorized test run.

## SU-5 dependency: poll idle invocation deadlines

- Behavior: `SupervisorInvocation::check_deadline` lets the trusted scheduler enforce transaction age and accepted cancellation while planner/executor streams are idle; expiration or cancellation aborts through the managed transaction and releases its snapshot/admission permit.
- Changed code and contract: [supervisor invocation API](../../engine/src/storage_manager.rs) and [storage-manager lifecycle contract](../../specs/storage-manager.md).
- Verification: `cargo fmt --all`, workspace build, warning-denied Clippy, and `git diff --check` pass in the repository Nix shell. Tests were not run.
- Remaining: an implemented supervisor scheduler must call this method while worker channels are idle; the storage API cannot make that scheduling call itself.

## SU-5 dependency: paired worker-channel service loop

- Behavior: `SupervisorInvocation::serve_worker_channels` serves provisioned planner and executor Unix streams on scoped threads, polls deadline/cancellation every 100 ms while waiting for messages, closes the sibling endpoint after handler failure or panic, aborts on a handler panic, and joins both handlers before returning.
- Changed code and contract: [storage-side worker channel runner](../../engine/src/storage_manager.rs) and [storage-manager process boundary](../../specs/storage-manager.md).
- Verification: workspace formatting, build, warning-denied Clippy, and `git diff --check` pass in the repository Nix shell. Tests were not run.
- Remaining: the supervisor must create/provision these streams to the correct worker processes, observe worker exits, and request the terminal decision only after a successful worker result.

## SU-1 dependency: read-only token session authentication

- Behavior: `StorageManager::authenticate_query_session` verifies bearer-token validity and the current LOGIN/role closure on one read snapshot, returning a non-secret identity context without allocating an invocation or outcome. Execution START still revalidates through token-bound supervisor admission.
- Changed code and contract: [storage-manager authentication path](../../engine/src/storage_manager.rs) and [access contract](../../specs/storage-manager.md).
- Verification: workspace formatting, build, warning-denied Clippy, and `git diff --check` pass in the repository Nix shell. Tests were not run.
- Remaining: connect the method to a live gateway TCP service.

## SU-1 / SU-2 dependency: bind staging owner to authenticated session

- Behavior: successful storage AUTH now generates the opaque connection/session ID, and its identity context creates a staging owner bound to that ID and the storage-resolved principal. Callers cannot construct staging owners from arbitrary principal IDs outside the engine crate.
- Changed code and contracts: [storage authentication and session identity](../../engine/src/storage_manager.rs), [staging owner](../../engine/src/supervisor.rs), [storage access contract](../../specs/storage-manager.md), and [client AUTH contract](../../specs/client-protocol.md).
- Verification: Nix-shell formatting, workspace build, warning-denied Clippy, and `git diff --check` pass. Tests were not run.
- Remaining: implement strict HELLO/AUTH request decoding and connect the authenticated session to a gateway TCP connection.

## SU-1 dependency: real-redb AUTH session and revocation regression

- Behavior: a query connection authenticates against a real redb catalog, receives a stable nonzero session identity and staging owner, and revalidates before a protected operation. Revoking the token causes the next operation to fail authentication without changing the connection ID.
- Changed fixture: [client session AUTH regression](../../engine/tests/commit.rs).
- Verification: focused `cargo test -p prdb-engine --test commit client_session_authenticates_and_revalidates_before_operations --locked` passed in the repository Nix shell; formatting, workspace build, and warning-denied Clippy passed after the session implementation.
- Remaining: live gateway TCP service, supervisor process provisioning, and sandboxed end-to-end persistence/reconciliation evidence.

## SU-1 dependency: strict AUTH wire credentials

- Behavior: strict HELLO/AUTH JSON decoding validates the only token credential representation accepted by storage session authentication and prevents duplicate/unknown fields or alternate token encodings.
- Changed code and fixtures: [gateway protocol decoder](../../engine/src/client_protocol.rs) and [decoder tests](../../engine/tests/client_protocol.rs).
- Verification: focused `client_protocol` integration suite passed all 4 tests in the repository Nix shell.
- Remaining: live gateway service, supervisor process provisioning, and sandboxed end-to-end persistence/reconciliation evidence.

## SU-5 verification: paired worker drain before terminal commit

- Behavior: one token-admitted supervisor invocation serves planner and executor Unix streams concurrently; a planner `describe_relation` request for `information_schema.tables` receives a correlated successful description through the provisioned channel, and clean EOF on both drains the channels before the supervisor terminal request commits and storage records a durable COMMITTED outcome.
- Changed fixture: [real-redb paired worker stream regression](../../engine/tests/commit.rs).
- Verification: the focused regression passes in the repository Nix shell.
- Remaining: provision the streams to launched workers, observe their exit status, and require the worker process result before terminal commit.

## SU-2 validation: staging lifecycle and cancellation race

- Evidence: four unit tests cover chunk/owner rejection, immutable sealing, one-shot START, release, cancellation during metadata validation, disconnect and expiry cleanup.
- Test: [staging state-machine tests](../../engine/src/supervisor.rs).
- Verification: `cargo test -p prdb-engine --lib supervisor::tests --locked` passed.
- Remaining: live gateway upload flow and concurrency/quota stress under multiple sessions.

## SU-1 integration: protocol AUTH dispatch into storage

- Behavior: the HELLO/AUTH/PING frame handler routes AUTH through `StorageManager`, correlates a sequence-zero response, returns a nonsecret server session reference, and honors the strict JSON/base64 decoder.
- Changed code and test: [client protocol session dispatcher](../../engine/src/client_protocol.rs) and [real-redb AUTH handshake regression](../../engine/tests/commit.rs).
- Verification: focused handshake regression, workspace format/build, warning-denied Clippy, and `git diff --check` pass.
- Remaining: use the dispatcher in a real loopback TCP gateway and provision authenticated gateway-service IPC.

## SU-2 storage dependency: retain admission budget fields

- Behavior: staged submissions preserve client-requested workload class and deadline through sealing and one-shot START, with a positive configured ceiling.
- Changed code and contract: [staging request state](../../engine/src/supervisor.rs) and [supervisor resource policy](../../specs/supervisor.md).
- Verification: focused staging test rejects an over-ceiling deadline and observes the accepted analytical class and deadline at START.
- Remaining: apply admission budgets to live worker and planner scheduling.

## SM-6 / SU-3 dependency: failed pre-execution admission has no outcome

- Behavior: failed token authentication during invocation admission now releases its snapshot and admission slot without persisting ABORTED. Revocation after successful admission remains guarded through commit and conflicts durably.
- Changed code and contracts: [transaction admission cleanup](../../engine/src/transaction.rs), [storage admission](../../engine/src/storage_manager.rs), [client START contract](../../specs/client-protocol.md), and [token admission contract](../../specs/ddl-identity.md).
- Verification: focused `token_admission_checks_expiry_secret_revocation_and_commit_guards` passes in the repository Nix shell, confirming no outcome for rejected admissions and durable abort after post-admission revocation.
- Remaining: exercise the no-outcome rule through the sealed submission START flow and gateway status path.
- Verification: `cargo fmt --all -- --check`, workspace build, warning-denied Clippy, full `cargo test --workspace --locked`, and `git diff --check` pass in the repository Nix shell.

## SU-3 dependency: bind sealed START to token admission and deadline

- Behavior: query `start_submission` moves a sealed staging reservation through its one-shot START callback, revalidates the token, then opens a storage invocation with the sealed deadline. Scheduler polling aborts an expired invocation. Revoked credentials reject START, delete staged bytes, and create no invocation status or durable outcome.
- Changed code and fixture: [client start integration](../../engine/src/client_protocol.rs), [deadline-bound storage admission](../../engine/src/storage_manager.rs), and [real-redb staging/start regression](../../engine/tests/commit.rs).
- Verification: focused `sealed_start_admits_only_current_credentials_and_enforces_request_deadline` passes in the repository Nix shell.
- Remaining: launch/reap a confined worker, require its successful exit before finalization, and connect real gateway result/status flow.
- Full verification: `cargo fmt --all -- --check`, workspace build, warning-denied Clippy, full `cargo test --workspace --locked`, and `git diff --check` pass in the repository Nix shell.

## SU-3 dependency: immutable submission handoff after START

- Behavior: the staging store returns artifact and parameter bytes only as an owner-checked sealed handle after START succeeds. The handle shares immutable buffers and does not load or execute native code.
- Changed code and regression: [started submission handoff](../../engine/src/supervisor.rs) and [real-redb START lifecycle](../../engine/tests/commit.rs).
- Verification: focused START/handoff regression passes in the repository Nix shell.
- Remaining: pass the handle to a supervisor process launcher that establishes the required sandbox before loading the artifact.

## SU-1 / IPC-6 dependency: HELLO over gateway session IPC

- Behavior: the first request on a provisioned per-client gateway stream is `OpenClientSession` (`0x140a`), which runs the strict HELLO handler and binds the selected mode to the stream's session before AUTH or submission operations. Identity and staging ownership continue to come from authenticated server-side state.
- Contract and implementation: [gateway IPC protocol](../../specs/ipc-protocol.md), [operation authorization and dispatch](../../engine/src/ipc.rs), and [gateway service dispatcher](../../engine/src/supervisor_ipc.rs).
- Changed regression: [UnixStream handshake and upload lifecycle](../../engine/tests/commit.rs) exercises HELLO, AUTH, PREPARE, upload, SEAL, STATUS, START, and CANCEL through the real service loop.
- Commands actually run in the configured Nix shell: `cargo fmt --all`; focused `cargo test -p prdb-engine --test commit --locked client_handshake_requires_hello_first_and_sequences_responses`; and `cargo fmt --all -- --check && cargo build --workspace --locked && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked && git diff --check`. All passed.
- Remaining: provision authenticated socket identity/generation, monitor network disconnects during worker execution, and implement the TCP listener, launcher, sandbox, and result relay.

## SU-5 dependency: worker exit observation and reaping

- Behavior: after planner and executor streams have drained, `SupervisorInvocation::observe_worker_exit` polls the associated process while checking cancellation and age. The sandbox path uses `observe_worker_exit_in_cgroup`, which kills descendants and waits for an empty cgroup before authorizing terminal storage. `serve_terminal` enforces that the worker exited successfully before commit, and no new worker channel can open after exit observation starts or succeeds. A nonzero exit or process-inspection error durably aborts; deadline failure kills and reaps the worker.
- Contract and implementation: [supervisor lifecycle contract](../../specs/storage-manager.md) and [worker process observation](../../engine/src/storage_manager.rs).
- Changed real-redb regression: [failed worker exit records ABORTED and is reaped](../../engine/tests/commit.rs).
- Observed red: the terminal-authority regression first failed to compile because no `WorkerExitNotObserved` state existed. The existing worker-observation regression also established nonzero exit as a durable ABORTED result.
- Verification: focused worker-failure and worker-channel/terminal tests passed. Full configured Nix-shell validation passed after cgroup cleanup integration: `cargo fmt --all -- --check`, `cargo build --workspace --locked`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked`, and `git diff --check`.
- Remaining: launch a worker with a verified sandbox profile, bind worker channels to that process, relay its result bytes, and test cancellation/expiry during the process wait.

## SU-5 validation: cancellation and expiry during worker wait

- Behavior: accepted owner cancellation and the admitted execution deadline are both observed while a worker process is idle; the supervisor kills/reaps the worker, and storage records ABORTED before terminal handling can commit.
- Changed real-redb regression: [cancellation during process observation](../../engine/tests/commit.rs).
- Verification: focused `supervisor_cancellation_and_deadline_during_worker_wait_abort` passed for both paths. Full configured Nix-shell validation passed: format check, workspace build, warning-denied Clippy, workspace tests, and `git diff --check`.
- Remaining: exercise these paths with cgroup descendant cleanup and live cgroup availability.

## SU-6 validation: reconcile after terminal delivery loss

- Behavior: storage commits before the supervisor writes TERMINAL; if the gateway side disconnects and terminal delivery fails, the durable COMMITTED outcome remains available to the authenticated owner through `get_invocation_status`.
- Changed regression: [gateway disconnect after storage commit](../../engine/tests/commit.rs).
- Verification: focused real-redb `gateway_disconnect_after_storage_commit_is_reconcilable` passed. Full configured Nix-shell validation passed: `cargo fmt --all -- --check`, `cargo build --workspace --locked`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked`, and `git diff --check`.
- Remaining: exercise the same reconciliation across separate gateway/supervisor processes and restart, and prove no automatic replay occurs.

## SU-1 / IPC-6 dependency: gateway submission IPC dispatcher

- Behavior: a role-checked gateway dispatcher maps AUTH, PREPARE, chunk, SEAL, START, STATUS, and CANCEL operation tags to the strict per-client session handlers. Chunk IPC prepends an explicit artifact/parameter kind byte. START returns the network acknowledgement plus a typed storage invocation and immutable submission handle to trusted supervisor code; those authority-bearing objects are never serialized.
- Contract: [gateway submission IPC payloads](../../specs/ipc-protocol.md).
- Changed code and integration regression: [gateway request dispatcher](../../engine/src/supervisor_ipc.rs) and [real-redb full submission path](../../engine/tests/commit.rs).
- Verification: focused real-redb HELLO/AUTH and IPC submission lifecycle test passes in the repository Nix shell.
- Remaining: run this dispatcher from a provisioned, bounded Unix service loop and keep launch/results in a separate supervisor process.

## SU-1 dependency: gateway failed-authentication throttling

- Behavior: the gateway allows a configurable number of failed AUTH attempts per source IP in a rolling 60-second window (default 10). It reserves in-flight attempts before forwarding AUTH, releases reservations only on successful authentication, caps retained source histories at 4,096, and fails closed when the cap is full. The source address is only a throttle key; credential bytes are not retained.
- Contract and implementation: [gateway authentication and connection limits](../../specs/client-gateway.md), [storage authentication boundary](../../specs/storage-manager.md), [gateway limiter and runtime](../../engine/src/gateway.rs), and [gateway configuration](../../engine/src/bin/prdb-gateway.rs).
- Regression evidence: deterministic limiter tests verify threshold, source-map cap, and expiry; a real TCP/Unix-socket gateway regression confirms the second failed AUTH is closed before reaching the supervisor service. The CLI exposes the configured limit.
- Verification: focused limiter, live gateway, and CLI tests passed. Full configured Nix-shell validation passed: format check, workspace build, warning-denied Clippy, all workspace tests, and `git diff --check`.
- Remaining: gateway restart loses in-memory throttle history; invalid-authentication throttling is per source address, while network-level denial-of-service protection remains a deployment concern. The gateway/supervisor process startup and sandboxed worker launch are still incomplete.

## SU-1 / GW-2 dependency: sanitized client ERROR responses

- Behavior: failed gateway request dispatches produce fixed-category, fixed-message ERROR payloads over a dedicated supervisor-to-gateway IPC response event. The gateway maps them to client frame kind 101 and closes the connection. Locally rejected client kinds and authentication throttles return similarly fixed responses. Raw internal errors are not serialized.
- Contract and implementation: [client ERROR frame](../../specs/client-protocol.md), [gateway service event](../../specs/ipc-protocol.md), [gateway bridge](../../engine/src/gateway.rs), and [supervisor error classifier](../../engine/src/supervisor_ipc.rs).
- Regression evidence: gateway bridge regression verifies the error kind, request ID, fixed payload, and close behavior.
- Remaining: deliver errors for malformed frame headers where a trustworthy request ID is unavailable, map every transport failure category, and exercise real service errors across separate gateway/supervisor processes.

## SU-6: release staging reservation after START callback — implemented

- Behavior and normative contract: [supervisor lifecycle](../../specs/supervisor.md). Once the trusted START callback returns, the completed invocation's immutable artifact and parameters must stop consuming per-connection and per-principal staging quota before the same connection is serviced again.
- Implementation: the gateway service captures the authenticated staging owner and invocation ID before handing control to the START callback, then calls `StagingStore::finish_started` on callback return. A staging cleanup error closes that service stream.
- Red/green evidence: `started_callback_releases_reservation_before_same_session_resumes` first failed because a second PREPARE on the same connection returned `InvalidState`; after the callback cleanup path was added, the next PREPARE succeeds with a new invocation ID.
- Verification: focused lifecycle regression passed. Workspace build, format check, warning-denied Clippy, all workspace tests, and `git diff --check` passed. The engine library had 35 passing unit tests; the commit target had 109 passing and 2 pre-existing cgroup/TinyCC fixture tests ignored.
- Remaining: run end-to-end worker execution on a host with the required writable delegated cgroup controllers. Broader SQL, DDL, pressure, and analytical stages remain open. The production callback is wired by `prdb-supervisor` to launch the sandboxed executor, relay its result, and finalize the durable outcome.

## SU-5 / GW-2: provide the live storage manager to START callbacks — implemented

- Behavior and normative contracts: [supervisor lifecycle](../../specs/supervisor.md), steps 4–6; [gateway service IPC](../../specs/ipc-protocol.md), START and terminal events. Trusted START handling must confirm outcomes through the same validated storage instance owned by the recovery loop.
- Implementation: `serve_gateway_service_listener` and `run_gateway_service_with_recovery` pass the current `StorageManager` reference into their START callback. When storage reports an ambiguous commit and the recovery loop reopens the database, later callbacks receive the new validated manager rather than reopening a second database handle.
- Regression/build evidence: all in-repository listener and recovery callback call sites compile against the new signature; focused supervisor IPC unit tests passed. Workspace build, format check, warning-denied Clippy, all workspace tests, and `git diff --check` passed. The engine library had 35 passing unit tests; the commit target had 109 passing and 2 existing cgroup/TinyCC sandbox tests ignored.
- Remaining: run sandboxed query execution on a host with a writable delegated cgroup with the required controllers. Separate gateway/supervisor process coverage exists for metadata and outcome recovery; `prdb-supervisor` configures the compatibility profile and wires the callback to executor result relay and durable finalization.

## SM-6 / SU-5: authenticated metadata discovery across gateway and supervisor processes — verified

- Behavior and contracts: [metadata discovery](../../specs/metadata-discovery.md), [client protocol](../../specs/client-protocol.md), and [supervisor service](../../specs/supervisor.md). A real authenticated control client can read schema, relation, and column descriptions through the external TCP gateway and private supervisor service, backed by the READY storage instance.
- Regression: [`metadata_process.rs`](../../engine/tests/metadata_process.rs) launches the actual `prdb-supervisor` and `prdb-gateway` executables, authenticates the bootstrap token through `MetadataClient`, checks the public schema, enumerates its relations, and describes `information_schema.schemata` columns. No query artifact is loaded and no worker is launched.
- Verification: the focused `cargo test --locked --test metadata_process` process-boundary test passed in the prescribed Nix shell.
- Limit: this closes the metadata control-read process path only. Successful worker launch and query execution still require a writable delegated cgroup host and the remaining executor callback ABI integration.

## SM-6 / SU-5: recover a disconnected invocation outcome after supervisor restart — verified

- Contract: [redb storage](../../specs/redb-storage.md), outcome recovery; [client protocol](../../specs/client-protocol.md), gateway disconnect and STATUS. An accepted invocation whose gateway disappears before terminal delivery is cancelled, receives a durable ABORTED outcome, and remains owner-queryable after the supervisor closes and reopens the same READY instance.
- Regression: [`metadata_process.rs`](../../engine/tests/metadata_process.rs) compiles a TinyCC ABI-v3 package, submits it through `SubmissionClient` to separate gateway and supervisor processes, kills the gateway after START acceptance, queries ABORTED through a replacement gateway, restarts the supervisor on the same database, then confirms STATUS still returns ABORTED through a new gateway.
- Observed behavior: the supervisor reports a broken pipe while relaying the lost terminal frame; storage nevertheless persists ABORTED. An initial expectation of COMMITTED was rejected by the observed result and corrected to match the specified disconnect cancellation contract.
- Verification: `cargo fmt --all -- --check`, `cargo clippy -p prdb-engine --test metadata_process --locked -- -D warnings`, and `cargo test -p prdb-engine --test metadata_process --locked` passed. The ignored sandbox test passed when run directly under `sudo -n` with the configured delegated cgroup and runtime paths. This is Linux x86_64 process-restart evidence, not power-loss evidence.
- Follow-up: a real sandboxed SQL regression now reaches the irreversible commit boundary and checks durable COMMITTED after gateway terminal delivery fails; rerunning it requires a delegated cgroup with enabled `cpu`, `memory`, and `pids`. Storage-file fault points and other supported durability profiles remain open. The broader SQL, DDL, pressure and analytical stages remain open.

## SM-6 / SU-5: recover COMMITTED status after terminal loss through gateway and supervisor processes — verified

- Contract: [redb storage](../../specs/redb-storage.md), outcome recovery; [client protocol](../../specs/client-protocol.md), terminal delivery and STATUS.
- Regression: `authenticated_metadata_reads_cross_gateway_and_supervisor_processes` creates a token-owned invocation in a real READY redb instance, observes a successful worker exit, commits through the production terminal helper with the receiving socket closed, closes the storage instance, then launches the actual supervisor and gateway binaries. An authenticated `SubmissionClient` STATUS returns COMMITTED after the database is reopened by the service process.
- Verification: `cargo fmt --all -- --check` and `cargo test -p prdb-engine --test metadata_process --locked` passed (1 passed, 1 delegated-cgroup test ignored).
- Scope: proves durable COMMITTED outcome recovery through process startup and the authenticated network status path after terminal delivery fails. The sandboxed query boundary regression is now implemented in `sandboxed_v3_sql_dml_sequence_is_visible_and_commits`; its execution is pending a delegated cgroup with the required controllers.

## SU-5 / GW-2: commit a confined query across the gateway and supervisor binaries — verified

- Regression: [`supervisor_binary.rs`](../../engine/tests/supervisor_binary.rs) starts both production binaries against a READY instance, stages a TinyCC ABI-v3 query over loopback TCP, emits 16 MiB while its client is unread, and uses the public `AdminClient` over the control lane to observe ACTIVE, then COMMITTED, and list token metadata through ADMIN.
- Verification: the focused process test passed with the typed `AdminClient` path in the delegated `systemd --user` service with the pinned runtime allowlist. Workspace format/build/warning-denied Clippy and the full serial workspace suite passed with sandbox/process tests enabled.
- Scope: verifies successful confined worker execution, result backpressure isolation, and durable finalization across both process boundaries. Broader SQL acceptance remains separate work.
