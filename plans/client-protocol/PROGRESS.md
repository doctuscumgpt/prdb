# Client Protocol Progress

## DS-SQL-27: local FK target in `tables.create` v2 — verified

- Added version 2 decoding for parallel FK source-column, target-constraint-ID, and target-column arrays. An all-zero constraint ID is accepted only with non-empty local target names; version 1 continues to encode existing/external targets.
- Regression: the strict decoder accepts a `parent_id REFERENCES` target represented by the same CREATE's `id` key. Focused decoder test passes. The protocol spec documents the sentinel and compatibility contract.
- Full serial delegated-cgroup workspace suite passes with zero ignored tests. Strict v1 compatibility and v2 decoding are covered; see [DDL progress](../ddl-schema/PROGRESS.md).

## DP-SQL-3 follow-up: scoped policy identity read — verified

- Added the read-only `metadata.policy_identities.resolve` operation with exact normalized arguments and nonzero schema/table IDs. The existing `metadata.identities.resolve` contract remains unchanged.
- A protocol decoder regression covers successful scope decoding and malformed/zero-table rejection. The delegated process lifecycle passes when a table owner creates row and cell policies.
- Verification: locked workspace build, formatting check, warning-denied all-target Clippy, maintained TinyCC SDK tests/fixture, and full serial delegated-cgroup workspace tests passed with zero ignored tests. `git diff --check` passed and Cargo.lock is unchanged. Commit is the remaining handoff step.

## DP-SQL-4: scoped policy-name read — verified

- Added `metadata.policies.resolve` for one row/cell policy name inside a nonzero schema/table scope. The real process regression exercises the read and subsequent typed drop as a table owner; decoder coverage checks the exact kind/scope shape.
- Focused parser, protocol, and delegated process tests pass. Locked workspace build, format check, warning-denied Clippy, TinyCC SDK tests/fixture, and full serial delegated-cgroup suite also pass with zero ignored tests. `git diff --check` passes; Cargo.lock is unchanged.

## DP-SQL-5: policy-authorized metadata shape — verified

- `metadata.relations.list` and `metadata.columns.list` now include a schema-visible table when the caller owns it or holds scoped `MANAGE_POLICIES`; ordinary grant-based results are preserved. The process regression confirms policy CREATE and DROP work as table owner with schema USAGE and no SELECT.
- Focused delegated process test passed. Locked workspace build, format check, warning-denied Clippy, TinyCC SDK tests/fixture, and full delegated-cgroup workspace suite passed with zero ignored tests; `git diff --check` passed and Cargo.lock is unchanged.

## NP-1 dependency: bounded version-1 frame codec

- Behavior: `ClientFrameDecoder` incrementally accepts fragmented/coalesced bytes, validates the fixed PDBN 1.0 header and per-kind payload limits before allocation, rejects nonzero flags/invalid sequences/zero IDs, and reports partial-frame EOF. `ClientFrame::encode` emits the fixed little-endian header. Request-ID and streamed-response sequence helpers enforce monotonic request IDs and consecutive RESULT_DATA/TERMINAL sequence numbers.
- Contract: [client network protocol](../../specs/client-protocol.md).
- Changed code: [frame codec and sequence state](../../engine/src/client_protocol.rs) and [module export](../../engine/src/lib.rs).
- Verification: workspace format, build, warning-denied Clippy, and `git diff --check` pass in the repository Nix shell. Tests were not run.
- Remaining: strict JSON/base64 operation decoding, live TCP/gateway integration, fuzzing and golden fragmented-frame fixtures.

## NP-2 dependency: per-connection HELLO/AUTH state

- Behavior: `ClientConnectionSession` accepts one HELLO, requires storage authentication before authenticated operations, enforces query/control request-kind allowlists, derives query staging ownership from storage-authenticated identity, and revalidates token/login/role authority before each execution or control request. Its connection-scoped bearer-secret copy is zeroized on drop.
- Changed code and contract: [client session state](../../engine/src/client_protocol.rs) and [HELLO/AUTH mode rules](../../specs/client-protocol.md).
- Verification: repository Nix-shell formatting, workspace build, warning-denied Clippy, and `git diff --check` pass. Tests were not run.
- Remaining: strict JSON/base64 wire decoding and TCP listener/service integration.

## NP-2 verification: real-redb session revocation

- Evidence: the focused commit integration test authenticates a seeded token, checks HELLO/AUTH ordering and query mode, revalidates a protected request, then revokes the credential and confirms the next protected request fails while the session ID remains stable.
- Test: [real-redb session regression](../../engine/tests/commit.rs).
- Remaining: wire JSON/base64 decoding and network listener integration.

## NP-2 dependency: strict HELLO and AUTH decoding

- Behavior: HELLO/AUTH accept exact flat string schemas, reject duplicate/unknown fields and malformed JSON, and validate lowercase nonzero token IDs plus canonical padded base64 for the fixed-size credential. Decoded secrets zeroize on drop and do not implement `Debug`.
- Changed code and fixtures: [HELLO/AUTH decoder](../../engine/src/client_protocol.rs) and [control decoder regressions](../../engine/tests/client_protocol.rs).
- Verification: focused protocol suite passed: 4 tests, including duplicate/unknown keys, escapes, trailing bytes, unpaired surrogates, lowercase IDs, and canonical base64.
- Remaining: decode other operation payloads and connect the session state to a live gateway TCP listener/service.

## NP-2 dependency: handshake frame dispatcher

- Behavior: the connection session accepts HELLO as the first request, validates increasing request IDs, answers HELLO/AUTH/PING with bounded sequence-zero responses, and rejects pre-HELLO PING and repeated/out-of-order operations.
- Changed code: [handshake dispatcher](../../engine/src/client_protocol.rs) and [client session integration regression](../../engine/tests/commit.rs).
- Verification: focused real-redb handshake regression and full workspace tests pass; workspace format, build, and warning-denied Clippy also pass.
- Remaining: dispatch query/control operation schemas and connect to TCP and the supervisor service channel.

## NP-2 / SU-1 integration: frame AUTH through storage

- Behavior: the frame-level AUTH dispatcher decodes canonical credentials, calls storage, and returns only the opaque session ID; the real-redb fixture verifies the response correlates to the accepted request and contains no principal/token secret.
- Test: [real-redb HELLO/AUTH/PING frame flow](../../engine/tests/commit.rs).
- Verification: focused handshake regression passes; workspace formatting, build, warning-denied Clippy, and diff checks pass.
- Remaining: network listener and gateway-service IPC integration.

## NP-3 dependency: authenticated binary chunk dispatch

- Behavior: chunk payloads decode only for nonzero invocation IDs and 1–256 KiB data, preserving the little-endian offset. Authenticated query frames route artifact versus parameter chunks through the connection-bound staging owner and return the next accepted offset.
- Changed code and tests: [chunk decoder and dispatcher](../../engine/src/client_protocol.rs), [boundary tests](../../engine/tests/client_protocol.rs), and [real-redb AUTH/upload stream test](../../engine/tests/commit.rs).
- Verification: focused chunk decoder and handshake/upload tests pass in the repository Nix shell.
- Remaining: strict PREPARE/SEAL/START JSON schemas, TCP listener, and gateway-service IPC integration.

## NP-3 dependency: PREPARE and SEAL dispatch

- Behavior: PREPARE enforces exact scalar fields, canonical u64 strings, lowercase SHA-256, canonical base64, protocol byte ceilings, and a supported workload name before reserving authenticated staging. SEAL accepts only one canonical invocation ID and returns `prepared` after staging verifies lengths, digests, and versioned metadata.
- Changed code and tests: [PREPARE/SEAL decoders and handlers](../../engine/src/client_protocol.rs), [schema regressions](../../engine/tests/client_protocol.rs), and [real-redb session upload lifecycle](../../engine/tests/commit.rs).
- Verification: focused protocol tests and HELLO/AUTH/PREPARE/chunk/SEAL integration regression pass in the repository Nix shell.
- Remaining: START frame response dispatch, STATUS/CANCEL, TCP listener, and gateway-service IPC integration.

## NP-4 dependency: one-shot START frame acceptance

- Behavior: START accepts only the strict invocation-ID envelope, routes through the sealed staging transition and current storage-authority revalidation, then returns sequence-zero `accepted` with the admitted supervisor invocation.
- Changed code and test: [START dispatcher](../../engine/src/client_protocol.rs) and [real-redb deadline/revocation regression](../../engine/tests/commit.rs).
- Verification: focused real-redb START regression passes in the repository Nix shell.
- Remaining: launch and supervise the worker before streaming RESULT_DATA and a trusted terminal.

## NP-5 dependency: owner STATUS and CANCEL

- Behavior: query STATUS revalidates current authority and returns an owner's ephemeral prepared state or durable/in-process storage state, while keeping unknown, expired, and foreign IDs non-enumerable. CANCEL revalidates authority, frees unstarted staging without an outcome, or marks an active owned invocation cancelled under the commit barrier.
- Changed code and tests: [owner status and cancellation dispatch](../../engine/src/client_protocol.rs), [storage admission cancellation](../../engine/src/storage_manager.rs), and [real-redb authenticated lifecycle](../../engine/tests/commit.rs).
- Verification: focused authenticated status/cancel, active cancellation, and handshake regressions pass; warning-denied Clippy passes in the repository Nix shell.
- Gateway-role status/cancel operation authorization and non-enumerating dispatch are tracked in the owning [IPC-6 progress entry](../ipc-protocol/PROGRESS.md).
- Remaining: control-mode administrator operations, provisioned gateway-service socket/session lifecycle, and the TCP connection server.

## NP-3/NP-4/NP-5 integration: submission operations through gateway IPC

- Behavior: the gateway IPC adapter carries AUTH, PREPARE, typed artifact/parameter chunks, SEAL, START, STATUS, and CANCEL through one client-bound session. START returns a typed trusted launch event and only exposes the accepted response for network delivery.
- Changed code and spec: [gateway IPC adapter](../../engine/src/supervisor_ipc.rs) and [payload mapping](../../specs/ipc-protocol.md).
- Verification: the real-redb handshake/submission regression covers the dispatcher lifecycle.
- Remaining: real service socket provisioning, TCP listener, result relay, and process lifecycle.

## NP-3 integration: authenticated TCP upload and seal — verified

- Behavior: the real supervisor and gateway binaries now carry HELLO, bootstrap-token AUTH, PMF1/PSC1 PREPARE, a TinyCC artifact chunk, and SEAL over one external TCP connection. The service validates artifact hashes, compatibility, requested scope and ELF dependencies, then reports `prepared` without loading the artifact.
- Regression: `supervisor_and_gateway_binaries_authenticate_upload_and_seal_tinycc_query_over_tcp` compiles a minimal ABI-v3 shared object with TinyCC and exercises the complete authenticated submission path through both binaries.
- Verification: `cargo test --workspace --locked` passed; this integration test was among the three passing supervisor binary tests. The two sandboxed START tests remain ignored because this host has no writable delegated cgroup.
- Remaining: exercise START, confined loading, result relay and durable terminal reconciliation on a host with a writable delegated cgroup.

## NP-5 integration: prepared STATUS and CANCEL over TCP — verified

- Behavior: through the same authenticated real-binary connection, STATUS reports a sealed reservation as `prepared`, CANCEL releases the unstarted reservation with `accepted`, and subsequent STATUS returns the same `unknown` response used for unavailable invocations.
- Regression: the TinyCC TCP submission test now covers that state transition and confirms the connection remains usable afterward with PING.
- Verification: focused supervisor binary regression, workspace build/format/warning-denied Clippy/tests, and the maintained TinyCC SDK suite passed. Workspace tests reported 109 commit tests passing and two cgroup-gated tests ignored.
- Verification update: the ignored `disconnected_submission_is_reconciled_after_gateway_restart` test now starts a confined query owned by a separately created user. A bootstrap-administrator control connection observes its `active` status and receives `accepted` from CANCEL; the test then kills the gateway before terminal delivery. A replacement gateway reads ABORTED, the supervisor/storage process restarts, and a new query-owner connection reads the same retained result again. The generated artifact is TinyCC-compiled and GNU-linked with `-z noexecstack`, matching the validated sandbox link profile.
- Verification update: `control_cancel_over_tcp_reports_too_late_during_durable_commit` pauses a real redb commit after the COMMITTING transition, submits authenticated control-mode CANCEL through the TCP gateway and supervisor IPC, then releases the commit so the required denied audit can be persisted. It verifies the external `too_late` response, the denied audit record, and the committed terminal outcome. The focused test passes alongside the existing real-redb second-connection regression and delegated-cgroup acceptance (`sandboxed_v3_sql_dml_sequence_is_visible_and_commits` and `disconnected_submission_is_reconciled_after_gateway_restart`).
- Remaining: the general workspace and client-protocol plan still tracks its other listed extensions; positive sandbox START and recovery acceptance depends on running the ignored tests on a writable delegated-cgroup host.

## NP-4 integration: START sandbox preflight rejection over TCP — verified

- Behavior: a valid sealed TinyCC submission cannot enter storage admission when the configured cgroup parent is an ordinary directory. The real supervisor/gateway path returns the generic `unavailable` error, and a new authenticated connection sees the invocation as `unknown`.
- Regression: the binary test uses its private temporary directory as the deliberately invalid cgroup parent, making the preflight failure deterministic without depending on host cgroup permissions. The artifact is never loaded.
- Verification: the focused real-binary preflight test passes alongside full workspace build/format/Clippy/tests and maintained TinyCC SDK tests. Positive sandbox tests remain ignored in the default suite because they require writable cgroup delegation; the four delegated-cgroup acceptances are run explicitly as recorded below.
- Verification update: the positive delegated-cgroup path is now exercised by the ignored SQL commit, SQL durable-abort, authenticated metadata TCP, and disconnected-submission gateway-restart acceptances. These confirm confined START plus terminal outcome relay and recovery on a provisioned Linux host. Full locked workspace build, format, warning-denied Clippy, serial tests, and maintained TinyCC tests pass.
- Remaining: continue SDK and gateway process-boundary coverage for the separately managed client APIs and remaining protocol plan items.

## NP-6: authenticated ADMIN ordinary-index operations — implemented

- Contract: [client protocol](../../specs/client-protocol.md) defines exact PREPARE/START argument schemas for ordinary index create, rebuild, and drop. Operations remain bound to one authenticated control connection and START must use current storage authorization.
- Implementation: `ClientConnectionSession` strictly decodes `indexes.create`, `indexes.rebuild`, and `indexes.drop`, binds each PREPARE to a random pending operation ID, then routes START to the typed `StorageManager` index lifecycle methods with a correlated audit event.
- Regression: decoder coverage checks canonical requests for all three operations. The real-redb authenticated gateway lifecycle test prepares, starts, and commits create/rebuild/drop through the gateway dispatch path, then validates persisted catalog state.
- Verification: full workspace build, formatting, warning-denied Clippy, and tests passed in the configured Nix shell; the commit integration target reported 124 passed and 3 cgroup-dependent ignored.
- Remaining: expose a user-facing client API for these ADMIN commands and verify them through the separately managed process boundary; that client work remains in the SDK and gateway plans.

## NP-2 / SM-6 dependency: handshake request dispatch

- Behavior: the request dispatcher handles HELLO/AUTH/PING frame state, rejects pre-HELLO requests and duplicate HELLO, and returns correlated sequence-zero responses. AUTH and operation revalidation use the storage session API.
- Changed code and fixture: [client handshake state](../../engine/src/client_protocol.rs) and [Unix-stream/storage integration regressions](../../engine/tests/commit.rs).
- Verification: full `cargo test --workspace --locked` passes; `cargo fmt --all -- --check`, workspace build, warning-denied Clippy, and `git diff --check` pass in the repository Nix shell.
- Remaining: implement TCP/gateway service and trusted gateway-to-supervisor IPC; launch confined worker processes and reconcile their outcomes.

## NP-3 dependency: frame upload into authenticated staging

- Behavior: after storage AUTH, binary artifact and parameter chunk frames are bound to the session's staging owner; chunk IDs, offsets, data lengths, and acknowledgements follow the v1 wire contract.
- Changed code and tests: [frame dispatcher](../../engine/src/client_protocol.rs), [protocol bounds](../../engine/tests/client_protocol.rs), and [real-redb auth/upload integration](../../engine/tests/commit.rs).
- Verification: focused protocol and handshake/upload tests pass.
- Remaining: wire PREPARE/SEAL schemas to staging and expose the flow through a live gateway connection.

## NP-3 dependency: prepare and seal through the authenticated session

- Behavior: strict PREPARE decoding validates exact fields, u32 version fields, byte declarations, canonical lowercase SHA-256/base64, workload class, and metadata bounds before staging reservation. SEAL validates a canonical invocation ID and drives staging digest, count, and admission-metadata checks before returning `prepared`.
- Changed code and tests: [client request handlers](../../engine/src/client_protocol.rs), [protocol regressions](../../engine/tests/client_protocol.rs), and [real-redb handshake/upload integration](../../engine/tests/commit.rs).
- Verification: focused protocol suite and real-redb HELLO/AUTH/PREPARE/chunk/SEAL regression pass in the repository Nix shell.
- Remaining: START frame dispatch, actual supervisor/gateway IPC, TCP transport, worker process lifecycle, and sandboxed execution evidence.

## NP-4 dependency: START admission response

- Behavior: one strict START frame claims a sealed reservation, revalidates storage credentials/current authority before creating a snapshot, and returns sequence-zero `accepted` alongside the supervisor-owned invocation.
- Changed code and test: [START frame path](../../engine/src/client_protocol.rs) and [current-credential/deadline real-redb regression](../../engine/tests/commit.rs).
- Verification: focused START regression passes in the repository Nix shell.
- Remaining: process launch/exit observation, streamed result and trusted terminal delivery, and gateway integration.

## NP-5 dependency: session-bound status and prepared cancellation

- Behavior: PREPARE, chunk, SEAL, STATUS, and CANCEL requests revalidate the storage session. Owner STATUS consults staging then live/durable storage status; CANCEL releases pre-start staging or marks an active owned invocation cancelled under the storage commit barrier. Foreign and unknown IDs share unavailable/unknown behavior.
- Changed code and regressions: [client request revalidation and dispatch](../../engine/src/client_protocol.rs), [owner cancellation transition](../../engine/src/storage_manager.rs), and [real-redb authenticated flow](../../engine/tests/commit.rs).
- Verification: focused status/cancel and active cancellation regressions plus warning-denied Clippy pass in the repository Nix shell.
- Remaining: admin control mode, TCP gateway and supervisor IPC, worker process/result/finalization lifecycle.

## SM-5 / NP-5 dependency: administrator point status

- Behavior: a storage administrator can look up any invocation's live or retained terminal status after current database-administrator authority is checked from a fresh catalog snapshot. Ordinary owner lookup remains owner-scoped. Control-mode client STATUS uses this path; query-mode STATUS continues to consult only the caller's staging and storage state.
- Contract and implementation: [storage status authority](../../specs/storage-manager.md), [control protocol status](../../specs/client-protocol.md), [StorageManager lookup](../../engine/src/storage_manager.rs), and [control-mode session handler](../../engine/src/client_protocol.rs).
- Regression evidence: the real-redb administrative runtime test verifies a current administrator can see another owner's active invocation and retained ABORTED outcome, a non-administrator is denied, and a missing ID remains unavailable. The same test exercises both point statuses through an authenticated control-mode session.
- Verification: focused real-redb test passed in the configured Nix shell.
- Remaining at that increment: control-mode CANCEL was still unwired; the following increment records its audited implementation. ADMIN operations, explicit sanitized ERROR responses, and process-separated service integration remain open.

## SM-5 / NP-5 dependency: administrator control-mode cancellation

- Behavior: control-mode CANCEL revalidates the authenticated session, creates a fresh audit event, and delegates to `StorageManager::cancel_invocation`, which checks current administrator authority, applies the per-actor request limit, serializes against COMMITTING, and persists the cancellation decision. Query-mode prepared/owner cancellation keeps its existing owner-scoped path.
- Contract and implementation: [client cancellation semantics](../../specs/client-protocol.md), [audited storage cancellation](../../specs/storage-manager.md), and [control session handler](../../engine/src/client_protocol.rs).
- Regression evidence: the real-redb control-mode test cancels a live invocation and confirms its administrative status exposes `cancellation_requested = true`.
- Verification: focused test and full configured Nix-shell validation passed: format check, workspace build, warning-denied Clippy, all workspace tests, and `git diff --check`.
- Remaining: `ADMIN` operation envelopes/dispatch, sanitized protocol ERROR responses, and the separate supervisor process remain open.

## SM-5 / NP-5 dependency: read-only token inventory ADMIN operation

- Behavior: control-mode ADMIN accepts the strict version-1 `tokens.list` read envelope, revalidates the connection before execution, checks owner/MANAGE_USERS/database-administrator authority in storage, and returns at most 64 token metadata records with an ID cursor. Verifier and bearer-secret material are omitted.
- Contract and implementation: [client protocol](../../specs/client-protocol.md), [bounded token page](../../engine/src/storage_manager.rs), [ADMIN decoder and session handler](../../engine/src/client_protocol.rs), and gateway IPC kind mapping.
- Regression evidence: decoder coverage rejects prepare-phase mutation envelopes and unknown arguments; a real-redb authenticated control-mode session exercises the gateway IPC ADMIN dispatch for another user's inventory and verifies secret material is absent.
- Remaining at that increment: runtime invocation listing and other ADMIN operations, and the separate supervisor process remained open. Runtime invocation listing is recorded below.

## SM-5 / NP-5 dependency: prepared token revocation ADMIN operation

- Behavior: ADMIN PREPARE reserves one random operation ID and token ID on the control connection; START must present that ID on the same connection. START consumes the reservation before calling the audited `StorageManager::revoke_token` path. The storage layer performs the current `MANAGE_USERS` check and commits the revocation with its audit and durable operation outcome. Unknown, already revoked, and unauthorized tokens retain the storage API's shared failure.
- Contract and implementation: [tokens.revoke ADMIN contract](../../specs/client-protocol.md), [strict ADMIN command decoder and per-session operation reservation](../../engine/src/client_protocol.rs), and gateway operation dispatch.
- Regression evidence: decoder tests cover both phase schemas; a real-redb gateway-dispatch regression uses a separate `MANAGE_USERS` principal to prepare/start revocation and confirms the target metadata becomes revoked.
- Remaining: reservation behavior after failures before a durable outcome, other ADMIN operations, and the separate supervisor/worker process remain open.

## SM-5 / NP-5 dependency: one-time token issuance ADMIN operation

- Behavior: `tokens.issue` uses the connection-owned PREPARE/START reservation. START revalidates the session and invokes audited `StorageManager::issue_token`. The committed response returns token ID and padded base64 secret once; the encoding, JSON construction buffer, TCP frame buffer, and gateway-service response buffer are zeroized after handoff.
- Contract and implementation: [one-time issuance contract](../../specs/client-protocol.md), [strict command schema and issuance handler](../../engine/src/client_protocol.rs), [service response cleanup](../../engine/src/supervisor_ipc.rs), and [TCP frame cleanup](../../engine/src/gateway.rs).
- Regression evidence: the real-redb ADMIN dispatch issues a credential, decodes it through the regular AUTH schema, authenticates a new session with it, and confirms stored inventory contains metadata only.
- Remaining at that increment: lost-response reconciliation for token mutations is recorded below; other ADMIN operations and separate supervisor/worker integration remain open.

## SM-5 / NP-5 dependency: administrator live invocation pages

- Behavior: control-mode ADMIN accepts `runtime.invocations.list` with a bounded page limit and an opaque cursor bound to the current administrator and process-local registry generation. Every page revalidates the control session and database-administrator authority. Results expose bounded invocation metadata and best-effort consistency; cursors become invalid after registry changes or restart.
- Contract and implementation: [runtime listing protocol](../../specs/client-protocol.md), [cursor encoding and bounded storage page](../../engine/src/storage_manager.rs), and [ADMIN decoder and response handler](../../engine/src/client_protocol.rs).
- Regression evidence: decoder coverage round-trips a canonical cursor. The real-redb test exercises first and second ADMIN pages through gateway dispatch, checks the next invocation and cancellation state, and separately covers unauthorized callers, stale cursors, page limits, and terminal status.
- Remaining: remaining ADMIN operations and separate supervisor/worker process integration.

## SM-5 / NP-5 dependency: reconcile a lost ADMIN mutation reply

- Behavior: after a prepared token revocation commits, an authorized database administrator can query STATUS with the operation ID and retrieve its durable committed outcome. The client does not repeat START when the original response is unavailable.
- Regression evidence: the real-redb ADMIN lifecycle test discards the revocation response payload, then queries the operation ID from both a separate token-manager session and a database-administrator control session; each receives `committed`.
- Remaining: other ADMIN operations and separate supervisor/worker process integration.

## SM-5 / CP-1: authenticated ADMIN dispatch for ordinary indexes — implemented

- Contract: [client protocol](../../specs/client-protocol.md) exposes connection-owned PREPARE/START for index create, rebuild, and drop. START must revalidate the authenticated principal and delegate to action-specific storage authorization; a native query receives no administrative credentials.
- Implementation: the strict ADMIN decoder accepts `indexes.create`, `indexes.rebuild`, and `indexes.drop`. PREPARE binds each request to the connection's pending operation ID; START revalidates session state and routes through the typed `StorageManager` request with an audit event correlated to that ID.
- Regression: `admin_revoke_decoder_requires_prepare_then_start_schemas` checks all three canonical decoder shapes. `storage_manager_runtime_and_metadata_reads_enforce_authorization` exercises authenticated gateway PREPARE/START for create, rebuild, and drop and verifies committed responses; startup catalog validation succeeds after the broader DDL lifecycle.
- Verification: focused decoder and authenticated gateway ADMIN lifecycle tests passed. `cargo build --workspace --locked`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo test --workspace --locked` all passed in the prescribed Nix shell; the commit integration target reported 124 passed and 3 cgroup-dependent ignored.
- Remaining: SDK-facing SQL DDL compilation and a separately packaged TinyCC sandbox integration harness are still outside this increment.

## NP-6 follow-up: ADMIN reservation after a failed START — verified

- Contract: every valid ADMIN START consumes its connection-owned reservation before storage dispatch. If storage returns an error before committing an outcome, the same operation ID still cannot be replayed; STATUS may be `unknown` or durably `aborted`, and a deliberate retry uses a new PREPARE and operation ID after confirming no commit.
- Regression: the real-redb administrator lifecycle test prepares `tokens.issue` for an invalid target, starts it and observes the storage error, verifies a repeated START is rejected, then completes a separate valid token issue through a fresh PREPARE. The existing issue success path verifies STATUS is committed and the secret is delivered once and never recoverable from STATUS.
- Verification: the focused `storage_manager_runtime_and_metadata_reads_enforce_authorization` integration test passed. `cargo build --workspace --locked`, `cargo fmt --all -- --check`, warning-denied workspace Clippy, both maintained TinyCC Make targets, and the full serial delegated-cgroup workspace suite passed; the suite reported zero ignored tests. `git diff --check` passed.
- Remaining: broader concurrent TCP connections, codec/state-machine fuzzing, and supervisor/process boundary coverage remain open.

## NP-1 / NP-7: fragmented frame and bounded mutation coverage — verified slice

- Contract: the 32-byte PDBN v1.0 frame header accepts arbitrary TCP fragmentation without changing the decoded request; malformed control payloads and frame state must fail without panicking or allocating beyond their protocol bounds.
- Regression: an independent hand-checked HELLO frame is split at every byte boundary and decoded to the same request. A fixed-seed bounded mutation corpus changes every byte of valid HELLO, AUTH, SEAL, and ADMIN payloads, then exercises the control decoders, chunk/frame decoders, and request/response sequence guards; additional bounded random inputs cover the same entry points.
- Verification: both focused regressions passed. Workspace build, format check, warning-denied Clippy, and the full serial delegated-cgroup workspace suite passed; the suite reported zero ignored tests. `git diff --check` passed.
- Remaining: coverage-guided fuzzing with sustained corpora, randomized multi-step session transitions, and broader concurrent process-boundary tests remain open under NP-7.

## NP-7: coverage-guided protocol and sequence fuzzing — verified slice

- Harness: the isolated `fuzz/` crate adds `client_decoders` for the strict JSON/base64 decoders, chunk payloads, and incremental frame decoding under varied chunk widths; `client_responses` for the bounded server-response JSON parser; and `client_sequence_state` for bounded randomized sequences of request-ID and streamed-response events. The seed corpus includes canonical control payloads, hand-checked frame inputs, valid and malformed nested response JSON, and a valid response stream. The harness is pinned to nightly independently of the stable production toolchain; `engine/tools/fuzz_client_protocol.sh` selects that pinned compiler from the locked development shell.
- Campaign evidence: the original targets built. `client_decoders` completed campaigns of 655,538 runs and 240,862 runs (the latter with the protocol dictionary and a 1 MiB maximum input); `client_sequence_state` completed 3,769,480 runs. A helper-driven repeat completed 842,222 sequence-state runs in six seconds. The new `client_responses` target completed 10,000 runs with a 1 MiB maximum input, reaching 539 coverage counters. No crashes or sanitizer findings occurred. Fuzzer-generated non-crash inputs were not retained; the reviewed seed corpus remains checked in.
- Validation: `cargo fmt --all -- --check`, `cargo build --workspace --locked`, warning-denied workspace Clippy, and the full serial delegated-cgroup workspace test suite passed. The suite reported zero ignored tests. `git diff --check` passed.
- Remaining: sustained CI fuzzing, crash minimization into regression tests if discoveries occur, broader concurrent TCP/slow-client coverage, and supervisor/process boundary work remain open under NP-7 and related plan items.

## `tables.create` optional primary-key column list — implemented

- Contract: [client protocol](../../specs/client-protocol.md). PREPARE accepts optional `primary_key_columns` as an ordered string array. Decoder validation requires a non-empty, distinct list of declared non-nullable columns; START routes the list to the atomic storage table-create path.
- Regression: the decoder accepts a valid key and rejects nullable or repeated key columns; the full protocol decoder and commit integration targets passed.
- Verification: locked workspace build, format check, warning-denied Clippy, complete serial delegated-cgroup workspace suite with sandbox/process tests enabled and zero ignored tests, and `git diff --check` passed.

## `tables.create` optional UNIQUE constraint arrays — implemented

- Contract: [client protocol](../../specs/client-protocol.md). PREPARE accepts optional `unique_constraints` as an array of ordered name arrays. It validates non-empty distinct members against declared columns; nullable columns are permitted. START routes the optional primary key and all UNIQUE definitions to atomic storage creation.
- Regression: decoder coverage accepts a valid nested array and rejects an unknown column; the full protocol and commit test targets passed.
- Verification: locked workspace build, format check, warning-denied Clippy, complete serial delegated-cgroup workspace suite with sandbox/process tests enabled and zero ignored tests, and `git diff --check` passed.
## ID-SQL-2: authenticated principal name lookup

- Added `metadata.identities.resolve`, a bounded single-name read for user/role identity resolution. The storage call reauthenticates the control token, checks the matching management privilege or database-admin membership, and returns null for both hidden and absent identities.
- Specified in [client protocol](../../specs/client-protocol.md). Exact-shape decoder tests pass, and the delegated gateway/supervisor lifecycle test exercises the lookup through `admin-sql`. Full serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests.

## ID-SQL-3: DROP identity SQL dispatch

- Reuses the exact authenticated `metadata.identities.resolve` read and existing `users.drop`/`roles.drop` mutations; storage remains authoritative for RESTRICT and tombstone checks. The full cross-process lifecycle and serial delegated-cgroup workspace suite pass with sandbox/process tests enabled and zero ignored tests.

## ID-SQL-4: LOGIN state SQL dispatch

- Reuses the exact authenticated user-name resolver and existing `users.login` mutation. The delegated process lifecycle covers SQL LOGIN/NOLOGIN and token invalidation; the full serial delegated-cgroup workspace suite passes with sandbox/process tests enabled and zero ignored tests.

## ID-SQL-5: role membership SQL dispatch

- Resolves each membership endpoint using the authenticated identity read before dispatching `roles.grant` or `roles.revoke`. The delegated process lifecycle and full serial delegated-cgroup workspace suite pass with sandbox/process tests enabled and zero ignored tests.

## ID-SQL-6: system privilege SQL dispatch

- Resolves the grantee by kind and name before dispatching `privileges.grant`/`privileges.revoke`; storage keeps system privilege mutations database-admin-only. The delegated process lifecycle and full serial delegated-cgroup workspace suite pass with sandbox/process tests enabled and zero ignored tests.

## Bounded all-columns grant target — verified

- Added an `all_columns` PREPARE target with no ID array, keeping table-wide grants bounded on the wire even when a table approaches the engine's column limit. Storage expands the target at START; explicit `columns` requests remain bounded to 2–64 distinct IDs.
- Verification: strict decoder regression and delegated SQL process lifecycle passed. Locked workspace build, format check, warning-denied all-target Clippy, TinyCC SDK tests/fixture, release-profile validation, cgroup probe, and full serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored. `git diff --check` passed; Cargo.lock is unchanged.

## Atomic policy replacement operation — verified

- `metadata.policies.resolve` accepts the bounded `any` kind for authorized same-name replacement lookup. `policies.replace` carries the full typed successor definition and old policy ID in one PREPARE/START lifecycle; START returns the new ID after storage commits the replacement atomically.
- The decoder test covers the `any` lookup shape; the delegated gateway/supervisor lifecycle exercises `policies.replace` across row/cell kinds. Full serial delegated-cgroup workspace tests passed with zero ignored tests, as did build, format, warning-denied Clippy, TinyCC, and release-profile validation.

## Runtime-list version 3 supervisor queue summary — verified

- Added optional protocol operation version 3 with a six-field aggregate of pending, starting, and started staging submissions; queued starts by operational/analytical workload class; and reserved upload bytes. Version 1 and 2 response shapes remain supported at that increment.
- Decoder/version coverage, typed client pagination tests, and the delegated gateway/supervisor process regression pass. The full serial delegated-cgroup workspace suite passed with zero ignored tests; format, build, warning-denied Clippy, TinyCC, and `git diff --check` passed.

## Runtime-list version 4 queued START age — implemented slice

- Version 4 extends the identifier-free supervisor queue aggregate with `oldest_queued_start_wait_us`; prior versions retain their existing response shape.
- The request decoder accepts version 4 and the typed client uses it. Protocol decoder, typed TCP paging, and the real gateway/supervisor administrative lifecycle passed, including CLI JSON verification for an empty queue. Workspace build, format, warning-denied Clippy, maintained TinyCC tests/fixture, and full serial delegated-cgroup suite passed with sandbox/process tests enabled and zero ignored tests.

## Runtime-list version 5 supervisor result bytes — implemented slice

- Version 5 adds an administrator-only process-local total of result payload bytes successfully written from supervisor to gateway channels; prior operation versions preserve their response shapes.
- The request decoder accepts version 5, while the typed client requires and decodes the new field. Focused decoder, TCP client, and gateway/supervisor process checks passed.
- Full serial delegated-cgroup workspace validation passed with sandbox/process tests enabled. Workspace format, build, warning-denied all-target Clippy, TinyCC SDK tests/fixture, and `git diff --check` passed.

## Runtime-list version 6 gateway client-write result bytes — implemented slice

- Version 6 retains the supervisor relay counter and adds a gateway-local result payload byte counter to successful administrator pages. The gateway appends the field after the supervisor response; unauthorized responses and versions 1–5 preserve their prior shapes.
- The strict request decoder, typed client page decoder, gateway helper, and production process lifecycle cover the added field. Workspace format, build, warning-denied all-target Clippy, TinyCC SDK tests/fixture, delegated cgroup probe, full serial delegated-cgroup workspace suite, and `git diff --check` passed. Cargo.lock is unchanged.

## Scoped object-grant identity resolution — implemented slice

- Added `metadata.grant_identities.resolve` to the authenticated control decoder and session dispatch. It resolves the schema/table target plus one user/role and returns their IDs only to the target owner, inherited `MANAGE_GRANTS` holder, or database administrator; absent and unauthorized targets share an all-null response.
- SQL target lookup now uses this scoped endpoint instead of ordinary schema/relation pages, so scoped grant administration does not require global identity-management or ordinary visibility. Workspace build, formatting, warning-denied all-target Clippy, and `git diff --check` passed; `Cargo.lock` is unchanged. Focused protocol and process regressions remain to be added; no test suite was run for this increment.
- The authenticated columns page also permits declared-shape reads for the target table owner or inherited `MANAGE_GRANTS` holder without schema USAGE, enabling explicit-column grant binding.
- The same authenticated metadata operations now expose target schema/relation and logical index pages to table owners and inherited table-scoped `INDEX` grantees for index DDL, including without schema USAGE. Workspace build, format, warning-denied Clippy, and diff checks pass; focused authorization/process regressions remain outstanding.

## DS-SQL-48 tables.truncate control operation — verified slice

- Added a strict version-1 `tables.truncate` PREPARE request with a table ID, connection-owned pending action, START dispatch to fenced storage, and a committed response carrying the operation ID, target ID, and commit sequence.
- The authenticated gateway/supervisor lifecycle passes using production binaries; storage acceptance verifies durable completion and rejection behavior. The complete serial delegated-cgroup workspace suite passes with sandbox/process tests enabled (281 library tests and 177 commit tests passed, zero ignored). Workspace build, format check, warning-denied all-target Clippy, and `git diff --check` pass; Cargo.lock is unchanged.

## DS-SQL-49 named CHECK protocol — verified

- `constraints.create_check` version 2 accepts an optional normalized constraint name while version 1 retains its prior payload. The strict decoder and connection-owned PREPARE/START action carry the name to authorized storage. `metadata.constraints.list` now returns the catalog object name with each stable constraint ID.
- Protocol decoding is covered for both payload versions; the production gateway/supervisor named CHECK create/list/drop flow and full workspace suite pass. Cargo.lock is unchanged.
