# Client Gateway Progress

Migrated component-owned progress entries from [storage-manager progress](../storage-manager/PROGRESS.md); evidence is preserved below.

## GW-1 dependency: loopback-default TCP listener

- Behavior: `TcpGatewayListener` binds loopback by default and refuses a non-loopback plaintext address unless `allow_non_loopback_plaintext` is explicitly enabled. Its API exposes the bound address and TCP accept operation without storage access.
- Contract and implementation: [gateway deployment boundary](../../specs/client-gateway.md), [client gateway plan](../client-gateway/README.md), and [listener](../../engine/src/gateway.rs).
- Changed tests: [gateway listener policy](../../engine/tests/gateway.rs).
- Observed red: focused compile failed because `prdb_engine::gateway` did not exist before the implementation.
- Commands actually run in the configured Nix shell: `cargo fmt --all`; focused `cargo test -p prdb-engine --test gateway --locked` (3 passed); and `cargo fmt --all -- --check && cargo build --workspace --locked && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked && git diff --check`. All passed.
- Remaining at that increment: bounded accepted-connection handling and frame read/write deadlines. Those controls are recorded in the later gateway resource entry; session processing and a separately runnable gateway process remain open.

## GW-2 / GW-3 dependency: TCP client to gateway-service frame bridge

- Behavior: the TCP adapter decodes bounded client frames, translates the supported HELLO/AUTH/PREPARE/chunk/SEAL/START/STATUS/CANCEL/PING kinds to the dedicated gateway IPC operation allowlist, adds an explicit chunk-kind byte, and returns only correlated sequence-zero RESPONSE frames. Unsupported kinds, malformed/pipelined frames, service EOF, and mismatched IPC responses return errors for the caller to handle by closing the connection.
- Contract and implementation: [client protocol](../../specs/client-protocol.md), [gateway service IPC](../../specs/ipc-protocol.md), and [TCP adapter](../../engine/src/gateway.rs).
- Changed regression: [loopback TCP to Unix IPC bridge](../../engine/tests/gateway.rs) checks HELLO and PING correlation across real sockets.
- End-to-end regression: [real-redb gateway submission lifecycle](../../engine/tests/commit.rs) now drives HELLO, AUTH, PREPARE, upload, SEAL, STATUS, START, and CANCEL from a loopback TCP client through the frame bridge and per-client Unix service loop, then verifies disconnect cleanup and durable ABORTED.
- Observed red: the initial focused test failed to compile because the TCP bridge and `PingClientSession` IPC operation were absent.
- Commands actually run in the configured Nix shell: `cargo fmt --all`; focused `cargo test -p prdb-engine --test gateway --locked tcp_client_hello_and_ping_bridge_over_the_gateway_service_channel` passed; and `cargo fmt --all -- --check && cargo build --workspace --locked && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked && git diff --check`. All passed.
- Remaining: exercise the gateway and supervisor binaries together as separate processes and verify reserved control-path progress during saturated output; later channel-scoped session behavior replaces serialized generation/connection IDs.

## GW-1 dependency: bounded gateway connection admission

- Behavior: a shared atomic limiter issues RAII permits up to a positive configured connection maximum; bounded accept drops peers above that cap. The TCP/Unix stream serving helper sets positive idle read/write timeouts before frame processing.
- Changed implementation and regression: [listener limits and timeout bridge](../../engine/src/gateway.rs) and [shared connection-cap test](../../engine/tests/gateway.rs).
- Observed red: the focused test initially failed to compile because the shared limiter and bounded accept API were absent; a subsequent compile exposed and corrected the test's tuple destructuring.
- Commands actually run in the configured Nix shell: `cargo fmt --all`; focused `cargo test -p prdb-engine --test gateway --locked` (5 passed); and `cargo fmt --all -- --check && cargo build --workspace --locked && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked && git diff --check`. All passed.
- Remaining at that increment: add an absolute deadline across the complete pre-authentication phase. Per-frame deadlines, per-IP connection rate limiting, and the fixed bounded listener worker pool are implemented in later entries; the phase deadline is recorded below.

## GW-1 / GW-2 dependency: bounded TCP gateway worker pool

- Behavior: one listener process accepts TCP peers into a bounded queue served by a fixed number of workers. Each worker opens one private Unix service stream, applies idle read/write timeouts to both ends, and forwards the client session. Shutdown stops accepting, drains queued clients, and joins all workers.
- Changed implementation and integration regression: [gateway runtime](../../engine/src/gateway.rs) and [TCP/Unix worker-pool test](../../engine/tests/gateway.rs).
- Observed red: initial runtime test compilation failed because `GatewayRuntimeConfig` and `run_gateway` were absent.
- Commands actually run in the configured Nix shell: `cargo fmt --all`; focused `cargo test -p prdb-engine --test gateway --locked gateway_worker_pool_opens_one_service_stream_per_client` passed; and `cargo fmt --all -- --check && cargo build --workspace --locked && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked && git diff --check`. All passed.
- Remaining at that increment: verify the Unix service peer and bind service connections to gateway generation; add pre-auth absolute deadlines, rate limits, ADMIN forwarding, result relay, and process startup configuration. Peer UID verification is recorded in the following entry.

## GW-2 dependency: Linux Unix-service peer UID verification

- Behavior: `UnixGatewayServiceConnector` connects only when Linux `SO_PEERCRED` reports the configured expected UID; mismatched peers fail with `PermissionDenied` before the connection is handed to the frame bridge.
- Changed implementation and regression: [peer-checked Unix connector](../../engine/src/gateway.rs) and [local Unix socket identity test](../../engine/tests/gateway.rs).
- Observed red: the focused test did not compile before the connector API existed. A later compile caught a test assertion using equality on `std::os::unix::net::SocketAddr`; the assertion was removed because successful connector verification is the behavior under test.
- Commands actually run in the configured Nix shell: `cargo fmt --all`; focused `cargo test -p prdb-engine --test gateway --locked unix_gateway_connector_checks_peer_uid` passed; and `cargo fmt --all -- --check && cargo build --workspace --locked && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked && git diff --check`. All passed.
- Remaining: bind service connections to gateway generation and per-client connection ID; verify socket filesystem ownership/mode; add authenticated handoff on non-Linux platforms if they enter the supported profile.

## GW-3 / SU-1 / SU-2 validation: real-redb TCP admission and upload path

- Behavior: a loopback TCP client now drives the real-redb session and staging state through the gateway adapter and its Unix IPC service channel. The regression covers HELLO, AUTH, PREPARE, artifact upload, SEAL, STATUS, START, active CANCEL, EOF cleanup, and durable ABORTED status.
- Changed regression: [gateway lifecycle integration](../../engine/tests/commit.rs).
- Focused command: `cargo test -p prdb-engine --test commit --locked client_handshake_requires_hello_first_and_sequences_responses` passed.
- Full configured Nix-shell checks: `cargo fmt --all -- --check && cargo build --workspace --locked && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked && git diff --check` all passed.
- Remaining: worker process launch/reap, result data and terminal relay, sandbox enforcement, and status reconciliation after gateway/process failure.

## GW-1 dependency: runnable gateway process wrapper

- Behavior: `prdb-gateway` parses a loopback-default bind address, requires a private service socket and expected peer UID, configures bounded worker/queue/idle values, installs graceful SIGINT/SIGTERM shutdown, and refuses plaintext wildcard binding without an explicit flag.
- Changed binary and CLI regressions: [gateway process](../../engine/src/bin/prdb-gateway.rs) and [gateway CLI tests](../../engine/tests/gateway.rs).
- Observed red: the CLI test initially failed to compile because Cargo had no `prdb-gateway` target.
- Commands actually run in the configured Nix shell: `cargo fmt --all`; focused `cargo test -p prdb-engine --test gateway --locked` (9 passed); and `cargo fmt --all -- --check && cargo build --workspace --locked && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked && git diff --check`. All passed.
- Remaining at that increment: provide the supervisor-side Unix listener and bind gateway generation/session context; connect result relay and control ADMIN operations. The listener is recorded in the following entry.

## GW-2 dependency: bounded supervisor-side gateway service listener

- Behavior: the supervisor binds a private mode-0600 Unix socket, accepts only the configured Linux peer UID, caps active sessions, applies idle read/write timeouts, creates a fresh `ClientConnectionSession` per stream, and returns to request handling after the START callback hands admission to trusted scheduling code.
- Changed implementation and integration test: [supervisor gateway listener](../../engine/src/supervisor_ipc.rs) and [real-redb HELLO over the private listener](../../engine/tests/commit.rs).
- Observed red: the focused listener test failed to compile because the listener configuration, binder, and accept loop did not exist.
- Commands actually run in the configured Nix shell: `cargo fmt --all`; focused `cargo test -p prdb-engine --test commit --locked gateway_service_listener_binds_peer_and_serves_hello` passed; and `cargo fmt --all -- --check && cargo build --workspace --locked && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked && git diff --check`. All passed.
- Remaining: bind both sides to a gateway generation and connection ID, exercise listener through `prdb-gateway` in a separate process, and implement worker scheduling/result relay/control ADMIN.

## GW-1 dependency: absolute client-frame deadline

- Behavior: when serving a network connection with configured timeouts, the TCP adapter bounds each complete request frame by an absolute deadline, resetting the deadline only after a complete request has been processed. Partial-frame clients cannot extend the deadline by sending occasional bytes.
- Changed implementation and regression: [gateway frame reader](../../engine/src/gateway.rs) and [slow partial-frame test](../../engine/tests/gateway.rs).
- Observed red: focused slow-client coverage was added before the deadline path; it previously had only idle socket timeouts and no whole-frame deadline.
- Commands actually run in the configured Nix shell: focused `cargo test -p prdb-engine --test gateway --locked incomplete_tcp_client_frame_hits_absolute_read_deadline` passed; and `cargo fmt --all -- --check && cargo build --workspace --locked && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked && git diff --check`. All passed.
- Remaining at that increment: enforce one absolute unauthenticated phase deadline across HELLO/AUTH, and rate-limit by source address. The per-IP connection cap is implemented in the following entry.

## GW-1 dependency: bounded per-IP connection rate

- Behavior: the gateway applies a rolling-window connection-attempt limit per source IP before queueing a socket. Its remembered source map is capped at 4096 entries and prunes expired histories; when full, new sources fail closed. The process wrapper exposes the per-IP count.
- Changed implementation and regression: [gateway peer limiter and runtime integration](../../engine/src/gateway.rs), [CLI option](../../engine/src/bin/prdb-gateway.rs), and [deterministic window/source-cap test](../../engine/tests/gateway.rs).
- Observed red: focused test compilation failed because `GatewayPeerRateLimiter` did not exist. The first run then exposed a test expectation inconsistent with the one-source cap; the corrected test passes.
- Commands actually run in the configured Nix shell: `cargo fmt --all`; focused `cargo test -p prdb-engine --test gateway --locked per_ip_gateway_rate_limit_is_bounded_and_expires` passed; and `cargo fmt --all -- --check && cargo build --workspace --locked && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked && git diff --check`. All passed.
- Remaining at that increment: add an absolute deadline across the complete unauthenticated HELLO/AUTH phase and track auth failures without retaining credentials. The phase deadline is recorded in the following entry.

## GW-1 dependency: absolute unauthenticated phase deadline

- Behavior: the TCP bridge starts one deadline when client serving begins and applies it across HELLO, any pre-auth PING requests, and AUTH. Successful server-side AUTH ends this deadline; per-frame and idle limits continue afterward.
- Changed implementation and regression: [gateway bridge pre-auth state](../../engine/src/gateway.rs) and [HELLO plus phase-expiry socket test](../../engine/tests/gateway.rs).
- Observed red: the new regression failed to compile because the bridge had no combined idle/frame and pre-auth timeout API.
- Commands actually run in the configured Nix shell: `cargo fmt --all`; focused `cargo test -p prdb-engine --test gateway --locked unauthenticated_gateway_phase_has_one_absolute_deadline` passed; and `cargo fmt --all -- --check && cargo build --workspace --locked && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked && git diff --check`. All passed.
- Remaining: failed-AUTH throttling and live session restart/reconciliation are implemented and verified in the following component records; only the broader GW-4 backpressure and complete service/process acceptance work remains open.

## GW-1 / SU-1 dependency: failed-AUTH throttling — implemented

- Contract: failed AUTH attempts are bounded per source IP in a rolling 60-second window, with an in-flight reservation, bounded source history, and no retained credential bytes. The IP is only a throttle key.
- Implementation and contract: [`GatewayAuthFailureLimiter`](../../engine/src/gateway.rs), gateway configuration, and [authentication limits](../../specs/client-gateway.md). The original cross-component evidence is recorded in [supervisor progress](../supervisor/PROGRESS.md#su-1-dependency-gateway-failed-authentication-throttling).
- Regression evidence: `failed_auth_budget_is_bounded_per_ip_and_expires` and `gateway_stops_forwarding_auth_after_failed_attempt_budget` cover deterministic window/source-cap behavior and confirm a denied retry does not reach the supervisor service.
- Verification: the original implementation commit `c9cc716` records passing focused limiter, live gateway, CLI, full workspace, formatting, Clippy, and diff checks.
- Remaining: throttle history is process-local and resets on gateway restart; the documented cross-process live-session reconciliation is covered by `disconnected_submission_is_reconciled_after_gateway_restart` in [client protocol progress](../client-protocol/PROGRESS.md). Wider backpressure and deployment-level network denial-of-service controls remain open.

## GW-4 / SU-4 dependency: START result and terminal event relay

- Behavior: after START acknowledgement, the TCP bridge accepts only request-correlated result-data or terminal IPC events, maps them to RESULT_DATA/TERMINAL, and assigns increasing network sequence numbers. Supervisor helpers bound provisional data and emit TERMINAL only after storage's correlated commit/abort response succeeds; ambiguous failures emit no terminal claim.
- Contract and implementation: [gateway event IPC](../../specs/ipc-protocol.md), [TCP event relay](../../engine/src/gateway.rs), and [storage-confirmed terminal sender](../../engine/src/supervisor_ipc.rs).
- Changed tests: [network event ordering/correlation](../../engine/tests/gateway.rs) and [real-redb terminal evidence](../../engine/tests/commit.rs).
- Focused verification: `cargo test -p prdb-engine --test gateway --locked start_gateway_bridge_relays_provisional_data_then_terminal`, `cargo test -p prdb-engine --test commit --locked gateway_terminal_event_requires_a_storage_confirmed_decision`, and the real TCP/IPC lifecycle test `cargo test -p prdb-engine --test commit --locked client_handshake_requires_hello_first_and_sequences_responses` passed.
- Full validation in the configured Nix shell passed: `cargo fmt --all -- --check`, `cargo build --workspace --locked`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked`, and `git diff --check`.
- Remaining at that increment: real confined output, bounded backpressure across TCP and service streams, and durable cancellation after blocked writes are now covered by the later follow-up entries; streaming restart reconciliation and separate-process acceptance remain open.

## GW-4 dependency: cumulative gateway result budget — implemented

- Contract: [client gateway result routing](../../specs/client-gateway.md). The gateway must independently bound provisional output for each invocation at every hop and stop forwarding before exceeding the output quota.
- Red/green evidence: added a small-budget relay regression that sends two RESULT_DATA events totaling one byte over the limit, followed by a terminal event. The relay forwards only the first in-budget event, rejects the second, and never forwards the terminal claim after the stream exceeds its output budget.
- Implementation: the TCP relay tracks cumulative RESULT_DATA payload bytes with checked arithmetic and enforces the shared 64 MiB invocation result limit. Terminal control payloads remain outside that data budget.
- Verification: focused unit regression passed; the locked workspace build, format check, warning-denied Clippy, full workspace tests, and `git diff --check` passed under the prescribed Nix tool shell. The run reported 138 library tests passed and 4 cgroup-dependent tests ignored; the remaining integration targets passed.

## GW-4 follow-up: TCP client backpressure reaches the service producer — covered

- Contract: [client result routing and backpressure](../../specs/client-gateway.md) specifies synchronous result writes in the initial TCP bridge. A blocked client socket stops reads from that client's gateway service stream, which propagates pressure to the supervisor result relay and executor output channel.
- Regression: `slow_tcp_result_consumer_backpressures_supervisor_service_writer` constrains loopback TCP socket buffers, leaves the client unread for 100 ms, and sends sixteen 256-KiB RESULT_DATA frames through the real TCP/Unix-socket gateway bridge. The supervisor writer remains incomplete during the unread interval; after client draining starts, every frame arrives in sequence and the terminal event follows.
- Verification: the focused regression passed five consecutive runs (`1 passed, 0 ignored`, 0.11 seconds each); the complete gateway target passed (`19 passed, 0 ignored`); workspace formatting, build, warning-denied Clippy, and diff checks passed. The serial full workspace suite passed inside the delegated `systemd --user` service with the cgroup probe reporting `cpu memory pids` available and enabled; sandbox and process tests were enabled. The host Nix 2.34.9 CLI rejects the documented `nix develop --locked` option, so checks used `nix develop --no-update-lock-file` with Cargo `--locked`.
- Scope: this proves backpressure propagation across the TCP and service-socket boundary. It does not verify control-path progress while result output is blocked, disconnect/restart races during streaming, or concurrent saturation of the gateway worker pool.

## GW-4 follow-up: stalled TCP result writes close the service stream — covered

- Regression: `stalled_tcp_result_write_times_out_and_closes_service_stream` configures 100-ms gateway idle/write deadlines, constrains the TCP send buffer, and has the supervisor-side service write a 4-MiB result while the client stops reading. The gateway returns a timed-out write error and closing its service stream causes the producer write to fail.
- Focused verification: the exact test passed (`1 passed, 0 ignored`) in 0.21 seconds. This transport-only check confirms gateway timeout and service-stream closure; durable abort is covered by the confined acceptance below.
- End-to-end verification: the enabled confined acceptance `sandboxed_v3_sql_dml_sequence_is_visible_and_commits` now drives a third real 4-MiB worker through a loopback TCP gateway with a 150-ms write deadline and an unread client. The timeout closes the service stream, the result relay fails, and the storage lookup confirms durable ABORTED even though the terminal event cannot reach the client. Three sequential fresh-fixture delegated runs passed (`1 passed, 0 ignored` each); the serial full workspace suite passed in the delegated `systemd --user` unit with all sandbox/process tests enabled, along with format, workspace build, warning-denied Clippy, and diff checks.

## GW-2 follow-up: ADMIN TCP-to-service forwarding — covered

- Regression: `gateway_forwards_admin_request_and_correlated_response_over_tcp` sends an ADMIN frame through a loopback TCP socket, verifies the private Unix service receives `SubmitAdministrativeOperation` with the same request ID and payload, and checks the correlated response returns as a sequence-zero client RESPONSE.
- Verification: the focused test passed (`1 passed, 0 ignored`); all 21 gateway integration tests passed; format, workspace build, warning-denied Clippy, and diff checks passed. The serial full workspace suite passed in the delegated `systemd --user` service with all sandbox/process tests enabled. The bridge forwards the operation; authorization and audit remain supervisor/storage responsibilities. Separate-process deployment and reserved control-path progress during result saturation are still open.

## GW-4 follow-up: another connection can use spare gateway capacity — covered

- Regression: `status_on_another_gateway_connection_progresses_while_result_worker_is_blocked` starts an 8-MiB result stream over one TCP/Unix worker pair and leaves its client unread until the supervisor writer blocks. A second TCP connection sends STATUS through the second worker and receives its correlated response before the first client begins draining.
- Scope at this increment: this proved progress with one of two ordinary workers occupied. Dedicated control-path capacity is verified below.
- Verification: all 22 gateway integration tests passed; workspace formatting, build, warning-denied Clippy, and diff checks passed; the full serial workspace suite passed in the delegated `systemd --user` service with all sandbox and process tests enabled.

## GW-1 / GW-2 follow-up: gateway binary uses the private Unix service — covered

- Regression: `gateway_binary_relays_tcp_hello_to_its_private_unix_service` starts the real `prdb-gateway` binary on an ephemeral loopback port, accepts its peer-UID-checked Unix connection, returns HELLO over that channel, and verifies the external TCP client receives the correlated response. It also sends SIGTERM and verifies graceful process exit.
- Verification: the focused test passed (`1 passed, 0 ignored`); all 23 gateway integration tests passed; formatting, workspace build, warning-denied Clippy, and diff checks passed. The serial full workspace suite passed in the delegated `systemd --user` unit with sandbox/process tests enabled. This uses a test service endpoint; running the gateway and supervisor binaries together against real storage remains open.

## GW-2 / GW-3 / GW-5 follow-up: confined query commits across gateway and supervisor processes — covered

- Regression: `supervisor_and_gateway_binaries_commit_a_confined_submission` starts the actual `prdb-supervisor` and `prdb-gateway` binaries against a READY instance, submits a TinyCC ABI-v3 artifact over loopback TCP, and streams 16 MiB while the ordinary client is unread. A second authenticated client uses the dedicated control endpoint to observe ACTIVE before draining and COMMITTED afterward. The worker fixture is compiled with TinyCC and linked with the system linker using a non-executable stack.
- Verification: the focused process test passed three sequential delegated-service runs with pinned runtime paths and a writable cgroup parent. Workspace formatting/build/warning-denied Clippy and the full serial workspace suite passed in the delegated service with sandbox/process tests enabled; no tests were ignored.
- Scope: proves confined result backpressure does not consume reserved control capacity across the actual gateway and supervisor binaries. Broader SQL acceptance remains open.

## GW-4 follow-up: status progresses through reserved control capacity — covered

- Contract: [client gateway backpressure](../../specs/client-gateway.md). The ordinary query/result pool cannot consume the worker or queue capacity reserved for status, cancellation, administration, and PING.
- Implementation: `run_gateway_with_control` starts independent bounded ordinary and control pools with shared per-IP and failed-AUTH limiters. The `prdb-gateway` binary exposes `--control-bind`, `--control-workers` (default 2), and `--control-queue-capacity` (default 8); the control listener rejects submission upload/seal/START requests.
- Regression: `status_on_another_gateway_connection_progresses_while_result_worker_is_blocked` now uses a single ordinary worker occupied by an 8-MiB result stream and verifies STATUS returns on the control listener before the client drains. `gateway_binary_relays_tcp_hello_to_its_private_unix_service` verifies CLI startup exposes both listeners and the control listener rejects START.
- Observed red: the updated saturation regression first failed to compile because the control-pool API and configuration did not exist.
- Verification: all 23 gateway tests passed; workspace formatting, build, warning-denied Clippy, and the serial full workspace suite passed. The full suite ran inside the delegated `systemd --user` service with sandbox and process tests enabled.
- Scope: clients must use the configured control address for reserved progress. Full authenticated process coverage is recorded above.

## GW-2 follow-up: client sessions are bound to one gateway channel — specified

- Contract: [gateway session scope](../../specs/client-gateway.md) and [gateway service channel](../../specs/ipc-protocol.md). Each TCP connection owns one private Unix service stream and one supervisor-local `ClientConnectionSession`; EOF/restart drops that context and requires a fresh HELLO/AUTH.
- Regression: `gateway_worker_pool_opens_one_service_stream_per_client` now opens two simultaneous TCP clients that both use request ID 1, verifies each gets a distinct service stream, and checks their HELLO responses return only to the matching client.
- Decision: the one-to-one stream pair is the v1 generation/connection/session capability. The service protocol does not serialize reusable IDs because no session state can move between streams. A version that multiplexes/reuses channels must introduce explicit trusted IDs and cross-session rejection.
- Verification: focused connection-isolation test passed; all 23 gateway tests passed; workspace format/build/warning-denied Clippy passed; and the full serial workspace suite passed in the delegated `systemd --user` service with sandbox/process tests enabled.
