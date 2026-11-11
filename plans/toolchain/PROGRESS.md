# Toolchain Progress

## TC-5: signed release-profile archive workflow — implemented

- Contract: a normalized release archive must be independently checkable by checksum and verifiable against an operator-supplied public key; no release signing secret belongs in the repository.
- Implementation: the locked development shell now includes Minisign. `release-profile-sign` signs the packaged tarball using `SIGNING_SECRET_KEY`; `release-profile-verify` checks both SHA-256 and the detached `.minisig` file using `SIGNING_PUBLIC_KEY`. Signature replacement uses a temporary file so a signing error does not overwrite an existing signature.
- Verification: a fresh temporary key pair signed the actual validated glibc 2.42 release-profile archive; the Make verification target accepted it, and verification rejected an archive modified by appending one byte. Release profile validation reported 7 engine binaries and 3 query fixtures.
- Remaining: production signing-key custody/public-key distribution, pinned TinyCC runtime-support closure, additional runtime profiles, and confined compatibility acceptance remain open.

## TC-2: TinyCC optimizer uses generated plan-tail decoder — implemented

- Contract: successful optimizer responses expose a borrowed plan span only after the version/status/length prefix and exact tail length validate.
- Implementation: the generated C raw-byte tail decoder returns a borrowed, length-checked slice. The TinyCC optimizer SDK now uses it to populate the borrowed plan view while retaining maximum-plan-size checks.
- Regression: the maintained C fixture checks the returned view, exact length, and rejection of inconsistent/truncated lengths without changing output parameters. Generator tests assert the C helper.
- Verification: schema drift and generator tests and the complete maintained TinyCC SDK suite passed with warnings treated as errors in the pinned Nix shell.
- Remaining: additional C SDK serializer migrations and TC-2 release/profile acceptance work remain open.

## TC-2: TinyCC planner and executor relation-name tails — implemented

- Contract: the TinyCC SDK emits the same bounded version/length/name request bytes as the Rust IPC codecs.
- Implementation: planner and executor SDK request paths now serialize schema and relation names with the generated C UTF-8 tail helpers; identifier validation and request-size limits remain in their callers.
- Regression and verification: existing maintained planner/executor callback fixtures check exact request bytes. Generator drift/schema tests and the complete TinyCC SDK build-and-run suite passed with `-Wall -Wextra -Werror` in the pinned Nix shell.
- Remaining: other C SDK variable tails and the outstanding TC-2 release and cross-profile acceptance work remain open.

## TC-2: generated executor relation-name tail codecs — implemented

- Contract: relation-resolution requests carry exact UTF-8 schema and relation names whose byte lengths appear in the generated prefix.
- Implementation: the generator emits bounded Rust and C UTF-8 tail helpers. Rust executor request encoding and decoding use them while retaining nonempty-name, payload-size, exact-tail, and UTF-8 checks.
- Regression: an exact Rust wire-vector test checks round-trip, invalid UTF-8, and trailing-byte rejection. The TinyCC fixture checks both C name helpers and byte content; generator tests assert each helper pair.
- Verification: generator drift/schema tests, focused Rust request regression, warning-denied workspace Clippy, and full maintained TinyCC SDK suite passed in the pinned Nix shell.
- Remaining: additional executor/planner/supervisor payload migrations and release-profile, signature, runtime-support, timing, and sandbox acceptance gates remain open.

## TC-2: generated optimizer plan-byte tail codec — implemented

- Contract: a successful optimizer response carries a generated version/status/length prefix and exactly the declared opaque plan bytes; plan structure remains validated by the optimizer-plan codec.
- Implementation: the generator emits bounded Rust and C raw-byte tail helpers. Optimizer Rust response encoding and decoding use them while retaining plan-size, exact-length, and structural validation.
- Regression: existing Rust optimizer response tests cover valid and malformed plans. The TinyCC fixture checks raw tail round-trip, short output capacity without partial writes, and truncated input; generator tests assert both language helpers.
- Verification: generator drift/schema tests, focused validated-plan response regression, warning-denied workspace Clippy, and full maintained TinyCC SDK suite passed in the pinned Nix shell.
- Remaining: other hand-coded executor/planner/supervisor tails and release-profile, signature, runtime-support, timing, and sandbox acceptance gates remain open.

## TC-2: generated executor open-scan column-ID codec — implemented

- Contract: executor open-scan requests carry a versioned prefix followed by exactly `column_count` 16-byte column IDs.
- Implementation: the executor Rust request encoder and decoder now consume the schema-generated bounded `bytes16_array` codec. Existing relation-handle, zero-ID, duplicate-ID, maximum-column, and exact-tail checks remain in place.
- Regression: a request-level Rust test checks the prefix, IDs, round-trip, truncated input, and trailing bytes. The TinyCC fixture exercises both generated C collection helpers, and generator tests assert their presence.
- Verification: generated-file drift/schema tests, focused Rust request regression, warning-denied workspace Clippy, and the full maintained TinyCC SDK suite passed in the pinned Nix shell.
- Remaining: other hand-coded executor/planner/supervisor collection serializers and release-profile, signature, runtime-support, timing, and sandbox acceptance gates remain open.

## TC-2: generated planner column-ID collection codec — implemented

- Contract: planner estimate requests carry a versioned prefix and exactly `column_count` 16-byte column IDs.
- Implementation: the ABI generator now emits bounded Rust and C `bytes16_array` codecs; C decoding requires caller output capacity. The planner request encoder and decoder use the generated Rust tail codec while retaining duplicate, zero-ID, and maximum-column validation.
- Regression: the Rust and TinyCC fixtures check round-trip, exact bytes, truncation, and insufficient C output capacity; generator tests require both language helpers.
- Verification: generator drift/schema tests passed, the focused Rust wire-vector test passed, warning-denied workspace Clippy passed, and the full maintained TinyCC SDK suite passed in the pinned Nix shell.
- Remaining: other hand-coded executor/planner/supervisor collection serializers and release-profile, signature, runtime-support, timing, and sandbox acceptance gates remain open.

## TC-2: generated planner population tail codecs — implemented

- Contract: planner estimate responses carry a versioned fixed prefix followed by exactly `column_count` network-byte-order triples of readable, null, and redacted populations.
- Implementation: the shared ABI generator now emits bounded Rust and C triplet-array codecs. Planner Rust encoding and decoding use those generated codecs instead of hand-written u64 loops; the decoder still enforces the tuple column limit.
- Regression: generator tests check both languages' helpers. The maintained TinyCC fixture checks exact two-column bytes, round-trip, insufficient encode capacity without partial output, and truncated-tail rejection.
- Verification: generator drift/schema tests passed; the Rust wire-vector regression passed; the full locked workspace build, format check, warning-denied Clippy, and serial workspace suite passed (129 passed, 4 cgroup-dependent ignored), with the new Rust regression separately passing. The full maintained TinyCC SDK suite, including the C wire-vector fixture, passed in the configured Nix shell.
- Remaining: other hand-coded IPC layouts and release-profile, signature, runtime-support, timing, and sandbox acceptance gates remain open.

## TC-2: remove duplicate supervisor control version parsing — implemented

- Contract: supervisor status and administrative-cancel payload versions and exact lengths are validated by their generated operation-specific decoders.
- Implementation: the shared dispatcher now routes request validation directly through the generated decoders; its previous manual shared-prefix version read is removed. The public payload-version constant remains available to existing clients and fixtures.
- Regression and verification: owner-scoped client status/cancel and bounded control-socket regressions pass. Locked workspace build, format check, warning-denied Clippy, serial workspace tests (129 passed, 3 cgroup-dependent ignored), generated ABI checks, and maintained TinyCC SDK suite passed.
- Remaining: other remaining payload migrations and release-profile, signed artifact, runtime-support, cross-profile, and sandbox acceptance gates are still open.

## TC-1 / TC-3: initial SDK and query build timing sample — measured

- Profile and commands: pinned TinyCC 0.9.27-unstable-2025-01-06 on NixOS x86_64/glibc 2.42. Three separate `make -f engine/sdk/Makefile BUILD_DIR=<fresh-dir> all` runs built the SDK archive; a repeated no-op make measured warm incremental overhead. A maintained `sql_v3_write_read.c` fixture was compiled seven times with TinyCC `-Wall -Wextra -Werror -fPIC`, then linked seven times with the SDK archive using GNU `cc -shared -Wl,--no-eh-frame-hdr,-z,noexecstack`.
- Results: cold SDK archive builds 217.3, 208.8, 220.8 ms; warm no-op 3.1 ms; per-query compilation 6.6–8.5 ms; per-query linking 32.8–40.6 ms. Each query measurement excludes SDK archive construction; load, execution, and commit are not included.
- Limitation: `make` emitted a 21 ms clock-skew warning for a fresh build directory despite the timestamp normalization attempt. Treat this as a host-local sample, not a performance guarantee; rerun after clock synchronization for clean timing evidence. No latency target is set.
- Remaining: broader compiler/runtime profiles, runtime support packaging, signed artifacts, and confined compatibility gates remain open.

## TC-2: generated supervisor terminal request encoders — implemented

- Contract: finalize and abort requests carry exactly the shared supervisor payload version.
- Implementation: the gateway terminal relay now encodes the request through the generated operation-specific helpers and rejects an unrelated operation in the helper.
- Regression and verification: exact request bytes and unrelated-operation rejection are covered. Focused terminal request tests, locked workspace build, format check, warning-denied Clippy, serial workspace tests (129 passed, 3 cgroup-dependent ignored), and maintained TinyCC SDK suite passed.
- Remaining: other remaining IPC layouts and release-profile, signing, runtime-support, timing, and sandbox acceptance gates remain open.

## TC-2: generated response codec C/Rust parity — verified

- Behavior: generated C helpers for executor relation-resolution response variants and planner unknown estimates must encode/decode the same wire fields as the Rust codecs.
- Regression: the maintained TinyCC IPC payload fixture checks exact version/status/identity/handle bytes, round-trips both executor variants, round-trips the planner unknown response, and rejects a non-exact unavailable payload length.
- Verification: generator drift/schema tests and the full maintained TinyCC SDK suite passed with `-Wall -Wextra -Werror` in the configured shell.
- Remaining: generated-codec parity is not end-to-end process interoperability; other payloads, release gates, and confined integration remain open.

## TC-2: generated planner unknown-estimate response — implemented

- Contract: planner estimate responses use an exact versioned header for either unknown status or an exact estimate prefix followed by bounded column populations.
- Implementation: the shared ABI schema now generates the unknown-status response codec; Rust planner encoding and decoding consume it instead of hand-building and inspecting the version/status bytes.
- Regression: `planner_stream_estimate_allowance_falls_back_without_aborting_invocation` exercises the unknown fallback alongside the exact estimate path.
- Verification: ABI drift/schema tests, focused fallback regression, locked workspace build, format check, warning-denied Clippy, serial workspace tests (129 passed, 3 cgroup-dependent ignored), and maintained TinyCC SDK suite passed in the configured Nix shell.
- Remaining: migrate other remaining hand-coded IPC layouts and complete release-profile, signature, runtime-support, timing, and sandbox acceptance gates.

## TC-2: generated executor relation response prefixes — implemented

- Contract: executor relation resolution returns either an exact unavailable response or a successful fixed prefix containing relation identity and connection-scoped handle, followed by bounded relation metadata.
- Implementation: the shared ABI schema generates Rust/C codecs for both response prefixes. The executor encoder and decoder now use those codecs while retaining semantic status/handle validation and the existing bounded name/column tail parser.
- Regression: a maintained Rust fixture checks exact available and unavailable bytes, round-trips the resolved response, and rejects malformed unavailable variants.
- Verification: schema drift tests, focused executor IPC regression, locked workspace build, format check, warning-denied Clippy, serial workspace tests (129 passed, 3 cgroup-dependent ignored), and full TinyCC SDK suite passed in the configured Nix shell.
- Remaining: migrate the other remaining hand-coded IPC payload layouts and complete release-profile, signature, runtime-support, timing, and sandbox acceptance gates.

## TC-2: supervisor terminal response decoder integration — implemented

- Contract: terminal finalize/abort acknowledgements must have the exact generated payload shape, supported version, and success status before the gateway relays a terminal event.
- Red evidence: the new regression did not compile because the relay had no typed response validator. The relay now dispatches to the schema-generated finalize/abort decoders; malformed, unsupported-version, nonzero-status, and trailing-byte payloads are rejected.
- Verification: focused `supervisor_terminal_responses_match_wire_bytes` passed. Locked workspace build, format check, warning-denied Clippy, serial workspace tests (129 passed, 3 cgroup-dependent ignored), generated ABI drift/schema tests, and the full maintained TinyCC SDK suite passed in the configured Nix shell.
- Remaining: replace other remaining hand-coded IPC payload validation and complete release-profile, signature, runtime-support, timing, and sandbox acceptance gates.

## TC-1: locked release workspace build — verified

- Contract: the workspace builds in release mode using the repository's locked dependency set and pinned development toolchain.
- Verification: `XDG_CACHE_HOME=/tmp/prdb-nix-cache nix develop --offline --command cargo build --workspace --release --locked` passed; Cargo finished the optimized release build in 45.55 seconds.
- Limitation: this validates Rust release compilation only. Release artifact packaging, supported Linux/libc profile selection, TinyCC shared-library compatibility, and end-to-end confined loading remain open plan items.

## TC-2: generated executor close-cursor request layout — implemented

- Contract: close-cursor requests carry payload version and the connection-scoped cursor handle in the exact open-scan request shape.
- Implementation: a dedicated shared schema layout now drives the Rust and TinyCC request codecs instead of reusing the open-scan response layout.
- Regression: the executor socket-loop verifies the close acknowledgement; TinyCC executor fixture validates cursor close success and failures.
- Verification: focused executor socket-loop regression, TinyCC SDK suite, generator drift, locked workspace build/format/warning-denied Clippy/serial tests pass. Commit tests report 129 passed; three cgroup-dependent tests remain ignored.
- Remaining: generate layouts/codecs for remaining supervisor event payloads and other internal IPC payloads.

## TC-2: generated executor cancellation-check response layout — implemented

- Contract: cancellation checks return exactly payload version plus a boolean cancelled byte.
- Implementation: Rust encoding and TinyCC SDK response validation use generated header sizes and offsets; the maintained C fixture also uses the generated fields.
- Regression: Rust exact-byte tests cover both boolean values; the TinyCC fixture covers false, true, and invalid values.
- Verification: generator drift, focused regression, locked workspace build/format/warning-denied Clippy/serial tests, and full maintained TinyCC SDK suite pass. Commit tests report 129 passed; three cgroup-dependent tests remain ignored.
- Remaining: generate layouts/codecs for remaining supervisor event payloads and other internal IPC payloads.

## TC-2: generated supervisor terminal decision request layouts — implemented

- Contract: finalize and abort requests contain exactly the supervisor payload version and reject unsupported versions or trailing bytes.
- Implementation: shared layouts now describe each request and dispatch validates through their generated header sizes and version offsets.
- Regression: valid finalize/abort bytes and truncated, unsupported-version, and trailing-byte payloads are covered.
- Verification: malformed/valid request tests, generator drift, locked workspace build/format/warning-denied Clippy/serial tests, and full maintained TinyCC SDK suite pass. Commit tests report 129 passed; three cgroup-dependent tests remain ignored.
- Remaining: generate layouts/codecs for remaining supervisor event payloads and other internal IPC payloads.

## TC-2: generated supervisor terminal decision response layouts — implemented

- Contract: finalize returns version, status 0, and commit sequence; abort returns version and status 0.
- Implementation: schema layouts and generated-offset encoders cover both acknowledgements; independent exact-byte tests are added.
- Verification: exact-byte fixtures, generator drift, locked workspace build/format/warning-denied Clippy/serial tests, and full maintained TinyCC SDK suite pass. Commit tests report 129 passed; three cgroup-dependent tests remain ignored.
- Remaining: generate layouts/codecs for remaining supervisor event payloads and other internal IPC payloads.

## TC-2: generated supervisor status response variants — implemented

- Contract: supervisor status returns unknown, active/committing with optional deadline, or terminal status with observation time, commit sequence, and retention deadline.
- Implementation: the shared schema declares all four fixed response shapes. The Rust supervisor encoder allocates the matching generated header and writes values at generated offsets.
- Regression: exact-byte coverage checks unknown, active without deadline, committing with deadline, and committed terminal forms.
- Verification: generator drift, focused exact-byte regression, locked workspace build/format/warning-denied Clippy/serial tests, and full maintained TinyCC SDK suite pass. Commit tests report 129 passed; three cgroup-dependent tests remain ignored.
- Remaining: generate layouts/codecs for remaining supervisor result and event payloads.

## TC-2: generated supervisor cancellation result variants — implemented

- Contract: cancellation returns a short accepted/too-late/unknown status or a terminal result with outcome status, commit sequence, and retention deadline.
- Implementation: the shared schema declares simple and terminal response layouts. The supervisor encoder writes both variants using generated header sizes and field offsets.
- Regression: client status/cancel and authenticated multi-process ADMIN lifecycle tests pass for the result path.
- Verification: generator drift, focused IPC/process regressions, workspace build/format/warning-denied Clippy/serial tests, and TinyCC SDK suite passed.
- Remaining: generate layouts/codecs for remaining supervisor result and event payloads.

## TC-2: generated supervisor control request layouts — implemented

- Contract: invocation status/cancel payloads bind authority to the provisioned channel; status/client cancel carry version plus invocation ID, while administrative cancel also carries the reason code.
- Implementation: the shared schema declares status/client control and administrative cancellation request fields. Supervisor Rust dispatchers validate lengths, versions, invocation IDs, and reason codes using generated header offsets/sizes.
- Regression: metadata/admin process integration, exact client status/cancel non-enumeration, and TCP cancellation-during-commit regressions pass.
- Verification: generator drift, focused control tests, full workspace build/format/warning-denied Clippy/serial tests, and complete TinyCC SDK suite passed.
- Remaining: generate layouts/codecs for remaining supervisor result and event payloads.

## TC-2: generated optimizer response variants — implemented

- Contract: optimizer responses use a version/status failure header, or a success header with u32 plan length followed by validated plan bytes.
- Implementation: the shared schema declares both variants and the raw byte tail. The Rust optimizer response codec consumes generated offsets and header sizes for both paths.
- Regression: optimizer response tests pass for validated plans, statuses, malformed lengths, malformed status values, and invalid plans.
- Verification: generator drift, focused optimizer codec tests, workspace build/format/warning-denied Clippy/serial tests, and full TinyCC SDK suite passed; 3 cgroup-dependent commit tests remain ignored by default.
- Remaining: generate layouts/codecs for remaining executor/planner/supervisor payloads.

## TC-2: generated stable plan-relation response prefixes — implemented

- Contract: optimizer metadata receives either version/status-only unavailability or a success prefix containing schema identity/version, table epoch, and relation identity before the variable relation description.
- Implementation: the shared schema declares both response prefixes. The Rust planner codec uses generated offsets and header sizes for encoding and decoding; the bounded relation-name and column codec remains explicit.
- Regression: planner estimate-session and concurrent planner/executor drain integration regressions pass through available and unavailable response handling.
- Verification: generator drift, focused planner integration tests, workspace build/format/warning-denied Clippy/serial tests, and full TinyCC SDK suite passed; 3 cgroup-dependent commit tests remain ignored by default.
- Remaining: generate layouts and codecs for remaining executor/planner/supervisor payloads.

## TC-2: generated planner relation-description response prefixes — implemented

- Contract: planner relation description returns either a version/status-only unavailable response or a success prefix with relation ID and planning handle before the variable name and column records.
- Implementation: the shared schema declares both response prefixes. Rust encoding/decoding and the TinyCC planner decoder consume their generated version/status/identity offsets and header sizes; the variable name and column records retain their bounded codec.
- Regression: `planner_stream_estimate_allowance_falls_back_without_aborting_invocation` and `supervisor_drains_both_worker_channels_before_durable_finalize` pass; the maintained TinyCC planner fixture passes.
- Verification: schema drift and negative schema tests, workspace build/format/warning-denied Clippy/serial tests, and full TinyCC SDK suite passed. Three cgroup-dependent commit tests remain ignored by default.
- Remaining: generate layouts and codecs for remaining planner/executor/supervisor payloads.

## TC-2: maintained payload schema validation tests — implemented

- Behavior: the SDK test target checks that the shared payload schema renders and rejects invalid version/count field widths.
- Implementation: `engine/tools/test_generate_query_abi.py` provides three standard-library unit tests; `abi-constants-test` is part of the maintained TinyCC `test` target.
- Regression: the focused test script and the Make target both pass all three cases.
- Verification: full locked Rust workspace checks and maintained TinyCC SDK suite pass with the new target enabled.
- Remaining: complete serializer generation and cover remaining IPC payloads.

## TC-2: validate payload version and collection count fields — implemented

- Contract: every payload version is a u16 field first in the payload and references a u16 version constant; count-delimited fixed-width collections use a u16 count.
- Implementation: the schema generator now rejects mismatched version declarations and collection count widths before emitting C/Rust constants.
- Regression: negative schema mutations with a u32 version field and a u32 collection count are both rejected; current generated files pass `--check`.
- Verification: locked workspace build/format/warning-denied Clippy/serial tests and TinyCC SDK suite passed using a temporary writable Nix cache after the default cache database returned read-only errors.
- Remaining: generate layouts and collection codecs for remaining executor/planner/supervisor payloads.

## TC-2: generated planner estimate collection declarations — implemented

- Contract: estimate requests carry a count-delimited array of 16-byte column IDs; exact estimate responses carry a count-delimited array of three 64-bit population values per column.
- Implementation: the shared payload schema now declares both trailing arrays. The generator accepts the population triple array encoding alongside its existing fixed-width ID array encoding.
- Regression: generator drift check and the planner estimate-session regression pass.
- Verification: generator `--check`, full workspace build/format/warning-denied Clippy/serial tests, and TinyCC SDK suite passed; commit integration tests reported 129 passed and 3 cgroup-dependent tests ignored.
- Remaining: generated collection codecs and layouts for remaining executor/planner/supervisor payloads.

## TC-2: generated executor next-batch response variants — implemented

- Contract: [internal IPC](../../specs/ipc-protocol.md) encodes next-batch responses with distinct finished and continuation headers; continuation includes a cursor before the row count.
- Implementation: the shared schema declares both fixed header variants. The Rust response codec chooses generated offsets when encoding and decoding each variant; row/cell records keep their existing bounded codec.
- Regression: `executor_relation_socket_loop_ends_session_without_finalizing` passes through both response paths.
- Verification: generator drift, exact focused integration test, full workspace build/format/warning-denied Clippy/serial tests, and the full TinyCC SDK suite passed. Commit integration results: 129 passed, 3 cgroup-dependent tests ignored.
- Remaining: generate layouts for remaining executor/planner/supervisor payloads and serialization helpers.

## TC-2: generated planner estimate-scan response header — implemented

- Contract: [internal IPC](../../specs/ipc-protocol.md) defines exact estimate responses as planner payload version/status, visible-row count, column count, then bounded per-column population triples; UNKNOWN responses end after status.
- Implementation: the shared schema now supports 64-bit fixed fields and declares the exact-response header. Rust response encoding/decoding uses generated offsets and header size while preserving the compact UNKNOWN form and explicit population-array codec.
- Regression: the planner estimate-session integration exercises exact and UNKNOWN response decoding.
- Verification: generator drift, focused planner regression, full workspace build/format/warning-denied Clippy/serial tests, and the full TinyCC SDK suite passed. Commit integration results: 129 passed, 3 cgroup-dependent tests ignored.
- Remaining: generate layouts for remaining executor/planner/supervisor payloads and serialization helpers.

## TC-2: generated mutation acknowledgement validation — implemented

- Contract: [internal IPC](../../specs/ipc-protocol.md) uses the same version/status acknowledgement for cursor close, row update, and row delete.
- Implementation: the TinyCC executor SDK validates update/delete and close-cursor acknowledgements through generated field offsets and header size. Rust close-cursor, update, and delete dispatch now all use `MutationAcknowledgement`.
- Regression: the maintained TinyCC executor fixture checks successful and rejected acknowledgements; the executor socket-loop test checks the exact close-cursor response bytes.
- Verification: full workspace build/format/warning-denied Clippy/serial tests pass (129 commit tests passed; 3 cgroup-dependent tests ignored). The TinyCC SDK suite passed with the earlier C-codec conversion.
- Remaining: generate layouts for remaining executor/planner/supervisor payloads and serialization helpers.

## TC-2: generated planner estimate-scan request header — implemented

- Contract: [internal IPC](../../specs/ipc-protocol.md) defines estimate-scan as planner payload version, connection-local planning handle, column count, and a fixed-width column-ID list.
- Implementation: the shared schema declares the fixed header fields. Rust planner request encoding and decoding consume generated offsets and header size; the trailing ID array retains its explicit bounded codec.
- Regression: the planner estimate-budget session regression passes through the request codec.
- Verification: generator drift, focused planner regression, full workspace build/format/warning-denied Clippy/serial tests, and the full TinyCC SDK suite passed. Commit integration results: 129 passed, 3 cgroup-dependent tests ignored.
- Remaining: generate layouts for remaining executor/planner/supervisor payloads and serialization helpers.

## TC-2: generated executor stage-insert request header — implemented

- Contract: [internal IPC](../../specs/ipc-protocol.md) defines stage-insert as the batch payload version, relation handle, value count, and bounded value records.
- Implementation: the shared schema declares the fixed header fields. Rust and TinyCC encoders/decoders consume generated offsets and header size; variable value records retain their existing codec.
- Regression: Rust mutation codec round-trip and maintained TinyCC executor fixture verify stage-insert request bytes.
- Verification: generator drift, focused Rust mutation tests, full TinyCC SDK suite, and workspace build/format/warning-denied Clippy/serial tests (129 commit tests passed; 3 cgroup-dependent tests ignored).
- Remaining: generate layouts for remaining executor/planner/supervisor payloads and serialization helpers.

## TC-2: generated executor stage-update request header — implemented

- Contract: [internal IPC](../../specs/ipc-protocol.md) defines stage-update as the batch payload version, row handle, patch count, and bounded patch records.
- Implementation: the shared schema declares the fixed header fields. Rust and TinyCC encoders/decoders use generated offsets and header size; patch records remain explicitly encoded by their existing codecs.
- Regression: Rust mutation codec round-trip and maintained TinyCC executor fixture verify update request bytes.
- Verification: generator drift, focused Rust mutation tests, full TinyCC SDK suite, and workspace build/format/warning-denied Clippy/serial tests (129 commit tests passed; 3 cgroup-dependent tests ignored).
- Remaining: generate layouts for remaining executor/planner/supervisor payloads and serialization helpers.

## TC-2: generated executor stage-insert response schema — implemented

- Contract: [internal IPC](../../specs/ipc-protocol.md) defines the stage-insert success response as the batch payload version followed by an opaque row handle.
- Implementation: the shared schema declares the fixed response layout. Rust and TinyCC response codecs use generated offsets and header size.
- Regression: the Rust mutation codec round-trip and maintained TinyCC executor fixture check the response shape and row-handle bytes.
- Verification: generator drift, focused Rust mutation tests, workspace build/format/warning-denied Clippy/serial tests (129 commit tests passed; 3 cgroup-dependent tests ignored), and the full TinyCC SDK suite passed.
- Remaining: generate layouts for remaining executor/planner/supervisor payloads and serialization helpers.

## TC-2: generated executor stage-delete request schema — implemented

- Contract: [internal IPC](../../specs/ipc-protocol.md) defines stage-delete as the batch payload version followed by an opaque row handle.
- Implementation: the shared schema declares the fixed-width request layout. Rust and TinyCC request encoders use generated offsets and header size.
- Regression: the Rust mutation codec round-trip and maintained TinyCC executor fixture check the request bytes against independent expected values.
- Verification: generator drift, workspace build/format/warning-denied Clippy, serial Rust tests (129 commit tests passed; 3 cgroup-dependent tests ignored), and the full TinyCC SDK suite passed.
- Remaining: generate layouts for remaining executor/planner/supervisor payloads and serialization helpers.

## TC-2: generated executor next-batch request schema — implemented

- Contract: [internal IPC](../../specs/ipc-protocol.md) defines the next-batch request as version, opaque cursor handle, scan limit, and byte limit.
- Implementation: the shared schema now describes the fixed-width request fields, including the 32-bit byte limit. Rust and TinyCC encoders use generated offsets and header size.
- Regression: the Rust relation socket-loop regression and maintained TinyCC executor fixture exercise the request encoding.
- Verification: generator drift, workspace build/format/warning-denied Clippy, serial Rust tests (129 commit tests passed; 3 cgroup-dependent tests ignored), and the full TinyCC SDK suite passed.
- Remaining: generate layouts for remaining executor/planner/supervisor payloads and serialization helpers.

## TC-2: generated executor close-cursor payload schema — implemented

- Contract: [internal IPC](../../specs/ipc-protocol.md) defines close-cursor as version plus opaque cursor handle and a version/status acknowledgement.
- Implementation: the shared payload schema now supports one-byte status fields and declares the mutation acknowledgement. The Rust close-cursor request reuses the generated open-scan cursor-handle layout, and Rust/C acknowledgement codecs consume generated offsets and sizes.
- Regression: the maintained executor IPC round-trip and TinyCC executor fixture verify close request and acknowledgement bytes.
- Verification: generator drift, workspace build/format/warning-denied Clippy, serial Rust tests (129 commit tests passed; 3 cgroup-dependent tests ignored), and the full TinyCC SDK suite passed.
- Remaining: add schemas for remaining executor/planner/supervisor payloads and generated serialization helpers.

## TC-2: generated executor open-scan request/response schemas — implemented

- Contract: [internal IPC](../../specs/ipc-protocol.md) defines the open-scan request as version, relation handle, column count, and optional column-ID list; its response is version plus cursor handle.
- Implementation: the shared schema now models 16-byte fields and length-counted arrays in addition to u16 fields and UTF-8 tails. The generator emits Rust/C offsets and header lengths. Rust request/response codecs and TinyCC SDK request/response checks consume those generated constants.
- Regression: `executor_relation_socket_loop_ends_session_without_finalizing` exercises the Rust request path; the maintained `sdk_executor_v1.c` fixture checks exact request and response bytes.
- Verification: focused Rust/C regressions, generator drift, full workspace build/format/Clippy/serial tests (129 commit tests passed; 3 cgroup-dependent tests ignored), and the complete maintained TinyCC SDK suite passed.
- Remaining: field-level generation still covers only qualified relation resolution and open-scan payloads. Other IPC messages and serializer generation remain open.

## TC-2: generated executor relation request field schema — implemented

- Contract: [internal IPC](../../specs/ipc-protocol.md) and [toolchain](../../specs/toolchain.md). The variable-length executor relation-resolution request has a six-byte fixed header: big-endian version, schema-name length, and relation-name length, followed by the two UTF-8 names in that order.
- Implementation: `engine/abi/query_abi_constants.json` declares the fixed and length-delimited fields. The generator validates supported encodings, unique names, and length-field references, then emits matching Rust/C offsets and header size. Rust and TinyCC encoders use those generated values.
- Regression: the Rust and C executor fixtures retain hand-checked request bytes, independently checking the generated layout. Generator `--check` verifies outputs have no drift.
- Verification: generator drift check, locked workspace build, format, warning-denied Clippy, serial workspace tests (129 commit tests passed; 3 cgroup-dependent tests ignored), and the full maintained TinyCC SDK suite passed.
- Follow-up: the C planner `describe_relation` helper shares the same generated request field offsets and version instead of maintaining its own literals. Its TinyCC fixture compares the emitted request against an independent exact-byte example.
- Follow-up verification: the focused `sdk-planner-v1` TinyCC fixture passes after rebuilding the SDK archive. The Rust request codec is shared with the executor and its exact-byte transaction regression passes.
- Remaining: extend the schema and generation to other IPC request/response payloads and codecs. This increment does not generate serializers.

## TC-2: generated IPC payload version registry — implemented

- Contract: [Toolchain](../../specs/toolchain.md). The shared interface schema now owns the executor relation/batch, planner, supervisor, and optimizer response payload-version constants.
- Implementation: the existing generator emits matching `u16` values for Rust and C. The Rust executor, planner, supervisor, and optimizer codecs import the generated constants rather than defining local version values. Payload fields and serializers remain in their Rust modules; this increment does not claim full IPC schema generation.
- Verification: `make -f engine/sdk/Makefile abi-constants-check`, locked workspace build, format check, warning-denied Clippy, serial workspace tests, and `make -f engine/sdk/Makefile test` passed in the locked Nix development shell. The commit integration suite reported 129 passed and 3 cgroup-dependent tests ignored.
- Remaining: define/generate IPC payload field schemas and serializers, complete C/Rust serialization parity, and validate the release Linux/libc/runtime profile and release artifacts. TC-2 remains partial.

## TC-2: generated fixed supervisor control codec — implemented

- Contract: [internal IPC](../../specs/ipc-protocol.md) and [toolchain](../../specs/toolchain.md). The supervisor invocation-control request is exactly a big-endian v1 payload version followed by a 16-byte invocation ID; truncated, extended, and unsupported-version payloads are rejected.
- Red/green evidence: the byte-exact codec regression first failed to compile because generated encoder/decoder functions were absent. The generator now emits opt-in Rust codecs for fixed-width schema payloads, and both supervisor and gateway control dispatch use the generated decoder.
- Regression: the independent wire example checks exact encoded bytes, round-trip decoding, invalid version rejection, and truncation rejection. Generator tests assert the helper output and reject attempts to generate a fixed codec for trailing fields.
- Verification: `make -f engine/sdk/Makefile abi-constants-check abi-constants-test`, the focused codec test, full locked workspace build/format/warning-denied Clippy/serial Rust tests, and `make -f engine/sdk/Makefile test` all passed in the pinned Nix shell. Serial workspace tests included 132 commit integration tests; three cgroup-dependent commit tests and one cgroup-dependent metadata-process test remained ignored by default. Their explicit delegated-cgroup acceptances were previously run and recorded in the Linux sandbox and supervisor progress files.
- Remaining: extend field-level schema and generated codecs to other payloads, add C serializer parity, and complete the release profile and artifact gates.

## TC-2: C/Rust invocation-control codec parity — implemented

- Contract: [internal IPC](../../specs/ipc-protocol.md) defines the fixed invocation-control payload shared by Rust supervisor code and C consumers. Both implementations must encode version 1 and the 16-byte invocation ID identically, and reject malformed lengths or versions on decode.
- Implementation: the shared schema can now opt fixed payloads into C codec generation for u8, u16, and bytes16 fields. The generated C header includes the generated constants header and uses the C-prefixed version constant. Rust fixed codecs also support signed and unsigned 64-bit fields.
- Red/green evidence: the maintained TinyCC fixture initially failed to compile because generated code referred to the Rust constant name instead of its C macro. Correcting the C name mapping made compilation succeed; the fixture now verifies an independent 18-byte wire example, round-trip data, unsupported version, and truncated length.
- Verification: generator drift/schema tests, locked workspace build, format check, warning-denied Clippy, serial Rust tests, and the complete maintained TinyCC SDK suite passed in the pinned Nix shell.
- Remaining: extend generated field codecs and cross-language parity to more payloads, especially variable-length collections, and complete release profile and artifact gates.

## TC-2: C/Rust invocation-control codec parity — verified

- The generated C and Rust codecs now consume the same fixed-width schema for the supervisor invocation-control request. The maintained TinyCC C fixture and Rust regression independently compare both implementations with the same 18-byte wire example, including exact decode, truncation rejection, and unsupported-version rejection.
- Verification: `make -f engine/sdk/Makefile abi-constants-check abi-constants-test`, locked workspace build, rustfmt check, warning-denied Clippy, serial workspace tests, and the complete maintained TinyCC suite passed. The SDK suite now compiles and runs `ipc-payload-codec-v1` with the pinned TinyCC build.
- Remaining: extend generated C codec parity to other fixed payloads and define safe bounded helpers for collections and variable-length payloads; complete release profile and artifact gates.

## TC-2: C/Rust simple cancellation-result codec parity — verified

- The generated C header now covers the fixed simple cancellation result as well, exercising a single u8 status field after the version. Its bytes match the Rust encoder and the existing internal IPC contract.
- Regression: the maintained TinyCC fixture checks the independent three-byte example and typed status round trip, alongside invocation-control and administrative-cancel request parity.
- Verification: generator drift/schema tests, locked workspace build, format, warning-denied Clippy, serial workspace tests, and the complete maintained TinyCC suite passed.
- Remaining: extend generated C codec parity to other fixed payloads and define safe bounded helpers for collections and variable-length payloads; complete release profile and artifact gates.

## TC-2: C/Rust terminal supervisor response codec parity — verified

- The shared schema now generates C codecs for terminal cancellation results and terminal STATUS responses. The C generator supports fixed u32/u64/i64 fields, including portable signed two's-complement decoding, and correctly copies 16-byte fields.
- Regression: the maintained TinyCC fixture checks both independent wire examples byte-for-byte and round-trips negative timestamps/deadlines, unsigned commit sequences, and status bytes. Generator tests assert both helper pairs are emitted.
- Verification: generated-file drift and schema tests, locked workspace build, format check, warning-denied Clippy, serial workspace tests (129 commit tests passed; 3 cgroup-dependent tests ignored by default), and the maintained TinyCC SDK suite passed in the pinned Nix shell.
- Remaining: extend generated C codec parity to other fixed payloads and define safe bounded helpers for collections and variable-length payloads; complete release profile and artifact gates.

## TC-2: C/Rust supervisor FINALIZE response codec parity — verified

- The shared schema generates the fixed FINALIZE response codec in Rust and C, and supervisor response encoding now calls the generated Rust helper.
- Regression: the Rust unit test retains its independent 11-byte wire example. The TinyCC fixture checks those bytes and decodes both the status and a multi-byte commit sequence; generator tests assert the C helper pair.
- Verification: generated-file drift and schema tests, locked workspace build, format check, warning-denied Clippy, serial workspace tests (129 commit tests passed; 3 cgroup-dependent tests ignored by default), and the maintained TinyCC SDK suite passed in the pinned Nix shell.
- Remaining: extend generated C codec parity to other fixed payloads and define safe bounded helpers for collections and variable-length payloads; complete release profile and artifact gates.

## TC-2: C/Rust supervisor ABORT response codec parity — verified

- The shared schema generates the fixed ABORT response codec in Rust and C, and supervisor response encoding now calls the generated Rust helper.
- Regression: the Rust terminal-response test checks the independent three-byte wire example. The TinyCC fixture checks matching bytes and decodes the status; generator tests assert both C helpers are emitted.
- Verification: generated-file drift and schema tests, locked workspace build, format check, warning-denied Clippy, serial workspace tests (129 commit tests passed; 3 cgroup-dependent tests ignored by default), and the maintained TinyCC SDK suite passed in the pinned Nix shell.
- Remaining: extend generated C codec parity to other fixed payloads and define safe bounded helpers for collections and variable-length payloads; complete release profile and artifact gates.

## TC-2: C/Rust supervisor UNKNOWN STATUS codec parity — verified

- The shared schema generates the compact fixed UNKNOWN STATUS codec in Rust and C, and the supervisor's no-snapshot response now calls the generated Rust helper.
- Regression: the existing Rust status-variant wire test checks the independent three-byte example. The TinyCC fixture checks identical bytes and status decoding; generator tests assert both C helpers.
- Verification: generated-file drift and schema tests, locked workspace build, format check, warning-denied Clippy, serial workspace tests (129 commit tests passed; 3 cgroup-dependent tests ignored by default), and the maintained TinyCC SDK suite passed in the pinned Nix shell.
- Remaining: extend generated C codec parity to other fixed payloads and define safe bounded helpers for collections and variable-length payloads; complete release profile and artifact gates.

## TC-2: C/Rust active STATUS codec parity — verified

- The shared schema generates both fixed active STATUS variants with and without a deadline. Supervisor response encoding now calls the generated Rust helpers for both paths.
- Regression: existing Rust status wire examples still match. The TinyCC fixture checks both independent byte layouts and round-trips negative observed/deadline times, start times, deadline flags, and cancellation flags.
- Verification: generated-file drift and schema tests, locked workspace build, format check, warning-denied Clippy, serial workspace tests (129 commit tests passed; 3 cgroup-dependent tests ignored by default), and the maintained TinyCC SDK suite passed in the pinned Nix shell.
- Remaining: extend generated C codec parity to other fixed payloads and define safe bounded helpers for collections and variable-length payloads; complete release profile and artifact gates.

## TC-2: C/Rust optimizer failure response codec parity — verified

- The shared schema generates the fixed optimizer failure response codec in Rust and C. Rust failure encoding now uses the generated helper; variable success-plan bytes remain length-bounded by their existing codec.
- Regression: the existing optimizer response round-trip test still passes. The TinyCC fixture checks the three-byte independent example and typed status round trip; generator tests assert both C helpers.
- Verification: generated-file drift and schema tests, locked workspace build, format check, warning-denied Clippy, serial workspace tests (129 commit tests passed; 3 cgroup-dependent tests ignored by default), and the maintained TinyCC SDK suite passed in the pinned Nix shell.
- Remaining: add C parity for other suitable fixed IPC payloads and define safe bounded helpers for collections and variable-length payloads; complete release profile and artifact gates.

## TC-2: C/Rust executor cancellation-check response codec parity — verified

- The shared schema generates the fixed executor cancellation-check response codec in Rust and C. Executor encoding now calls the generated Rust helper.
- Regression: the Rust test checks both cancellation states against their three-byte wire examples. The TinyCC fixture checks the cancelled form byte-for-byte and round-trips its status; generator tests assert both C helpers.
- Verification: generated-file drift and schema tests, locked workspace build, format check, warning-denied Clippy, serial workspace tests (129 commit tests passed; 3 cgroup-dependent tests ignored by default), and the maintained TinyCC SDK suite passed in the pinned Nix shell.
- Remaining: add C parity for other suitable fixed IPC payloads and define safe bounded helpers for collections and variable-length payloads; complete release profile and artifact gates.

## TC-2: C/Rust executor mutation acknowledgement codec parity — verified

- The shared schema generates the fixed mutation acknowledgement codec in Rust and C. Rust acknowledgement encoding and success-status decoding now use the generated helpers.
- Regression: the existing executor mutation round-trip test passes. The TinyCC fixture checks the independent three-byte example and typed status round trip; generator tests assert both C helpers.
- Verification: generated-file drift and schema tests, locked workspace build, format check, warning-denied Clippy, serial workspace tests (129 commit tests passed; 3 cgroup-dependent tests ignored by default), and the maintained TinyCC SDK suite passed in the pinned Nix shell.
- Remaining: add C parity for other suitable fixed IPC payloads and define safe bounded helpers for collections and variable-length payloads; complete release profile and artifact gates.

## TC-2: C/Rust executor OPEN SCAN response codec parity — verified

- The shared schema generates the fixed OPEN SCAN response codec in Rust and C. Executor response encoding and exact-length/version decoding now use the generated Rust helpers before validating the opaque cursor kind.
- Regression: a maintained Rust test checks the versioned cursor bytes, round trip, and truncation rejection. The TinyCC fixture checks byte-for-byte encoding and 16-byte cursor decoding; generator tests assert both helper pairs.
- Verification: generated-file drift and schema tests, locked workspace build, format check, warning-denied Clippy, serial workspace tests (129 commit tests passed; 3 cgroup-dependent tests ignored by default), and the maintained TinyCC SDK suite passed in the pinned Nix shell.
- Remaining: add C parity for other suitable fixed IPC payloads and define safe bounded helpers for collections and variable-length payloads; complete release profile and artifact gates.

## TC-2: C/Rust executor STAGE INSERT response codec parity — verified

- The shared schema generates the fixed STAGE INSERT response codec in Rust and C. Rust response encoding and exact-length/version decoding now use generated helpers before validating the opaque row-handle kind.
- Regression: the existing Rust mutation round-trip test covers the response. The TinyCC fixture checks the independent 18-byte response and 16-byte handle round trip; generator tests assert both C helper names.
- Verification: generated-file drift and schema tests, locked workspace build, format check, warning-denied Clippy, serial workspace tests (129 commit tests passed; 3 cgroup-dependent tests ignored by default), and the maintained TinyCC SDK suite passed in the pinned Nix shell.
- Remaining: add C parity for other suitable fixed IPC payloads and define safe bounded helpers for collections and variable-length payloads; complete release profile and artifact gates.

## TC-2: C/Rust executor STAGE DELETE request codec parity — verified

- The shared schema generates the fixed STAGE DELETE request codec in Rust and C. Rust request encoding and exact-length/version decoding now use the generated helpers before validating the row-handle kind.
- Regression: the existing Rust mutation round-trip test covers the request. The TinyCC fixture checks the independent 18-byte request and row-handle round trip; generator tests assert both C helper names.
- Verification: generated-file drift and schema tests, locked workspace build, format check, warning-denied Clippy, serial workspace tests (129 commit tests passed; 3 cgroup-dependent tests ignored by default), and the maintained TinyCC SDK suite passed in the pinned Nix shell.
- Remaining: add C parity for other suitable fixed IPC payloads and define safe bounded helpers for collections and variable-length payloads; complete release profile and artifact gates.

## TC-2: C/Rust planner unavailable-response codec parity — verified

- The shared schema generates fixed unavailable responses for both relation-description variants in Rust and C. Planner response encoding now uses the generated Rust helpers; variable available-response fields keep their existing bounded codecs.
- Regression: the planner fallback integration test passes. The TinyCC fixture checks both independent three-byte unavailable examples and decoded status values; generator tests assert both C helper pairs.
- Verification: generated-file drift and schema tests, locked workspace build, format check, warning-denied Clippy, serial workspace tests (129 commit tests passed; 3 cgroup-dependent tests ignored by default), and the maintained TinyCC SDK suite passed in the pinned Nix shell.
- Remaining: add C parity for other suitable fixed IPC payloads and define safe bounded helpers for collections and variable-length payloads; complete release profile and artifact gates.

## TC-2: generated C/Rust planner response prefix codecs — verified

- The generator now supports explicit prefix codecs: encoders emit only the schema fields, while decoders require the full fixed prefix and version but allow the caller-validated tail to follow. The shared schema opts both available planner relation responses into prefix generation.
- Planner Rust encoders and decoders use the generated prefix helpers before processing bounded names and column records. The TinyCC fixture checks both fixed headers and confirms C decoders accept a following tail; generator tests cover schema opt-in, trailing-field mode, and prefix length checks.
- Verification: generated-file drift and six schema tests, focused planner integration regression, fresh TinyCC prefix fixture, locked workspace build, format check, warning-denied Clippy, serial workspace tests (129 commit tests passed; 3 cgroup-dependent tests ignored by default), and the maintained TinyCC SDK suite passed in the pinned Nix shell.
- Remaining: generate serializers for additional variable payloads and bounded collections; complete release profile and artifact gates.

## TC-2: generated optimizer success response prefix codec — verified

- The optimizer success schema now generates the fixed status and u32 plan-length prefix in Rust and C, allowing the validated plan bytes to follow without weakening bounded plan decoding.
- Rust success response encoding and decoding use the generated prefix helper. The TinyCC fixture checks the seven-byte prefix, exact plan length, and successful decoding with an appended plan tail; generator tests assert the opt-in helpers.
- Verification: generated-file drift and six schema tests, focused optimizer response round-trip, fresh TinyCC codec fixture, locked workspace build, format check, warning-denied Clippy, serial workspace tests (129 commit tests passed; 3 cgroup-dependent tests ignored by default), and maintained TinyCC SDK suite passed in the pinned Nix shell.
- Remaining: generate serializers for additional variable payloads and bounded collections; complete release profile and artifact gates.

## TC-2: generated executor mutation request prefix codecs — verified

- The schema now generates fixed prefixes for STAGE INSERT (relation handle and value count) and STAGE UPDATE (row handle and patch count) in Rust and C.
- Rust mutation encoding and decoding use the generated prefix helpers, then retain the existing bounded value and patch record codecs. The TinyCC fixture verifies both prefixes, field round trips, and acceptance of appended mutation bytes; existing executor mutation tests cover the Rust record round trips.
- Verification: generated-file drift and six schema tests, focused executor mutation regression, fresh TinyCC prefix fixture, locked workspace build, format check, warning-denied Clippy, serial workspace tests (129 commit tests passed; 3 cgroup-dependent tests ignored by default), and the maintained TinyCC SDK suite passed in the pinned Nix shell.
- Remaining: generate codecs for additional variable payloads and bounded collections; complete release profile and artifact gates.

## TC-2: generated planner estimate prefix codecs — verified

- The shared schema generates the estimate request prefix (planning handle and column count) and exact response prefix (status, visible-row estimate, and population count) in Rust and C.
- Planner Rust encoders and decoders use the generated helpers, then keep the existing count, size, duplicate-ID, and triplet-array validation. The TinyCC fixture checks both header layouts, decoded typed values, and acceptance of appended collection bytes.
- Verification: generated-file drift and six schema tests, focused planner estimate integration regression, fresh TinyCC prefix fixture, locked workspace build, format check, warning-denied Clippy, serial workspace tests (129 commit tests passed; 3 cgroup-dependent tests ignored by default), and maintained TinyCC SDK suite passed in the pinned Nix shell.
- Remaining: generate codecs for additional variable payloads and bounded collections; complete release profile and artifact gates.

## TC-2: generated executor OPEN SCAN request prefix codec — verified

- The shared schema generates the OPEN SCAN request prefix containing the resolved relation handle and column count in Rust and C.
- Executor Rust encoding and decoding use the generated prefix helper, then retain existing bounded column-ID array, duplicate, zero-ID, and exact-length checks. The maintained TinyCC fixture checks relation-handle/count decoding with an appended column ID; executor socket and information-schema scan regressions pass.
- Verification: generated-file drift and six schema tests, focused executor scan socket regressions, fresh TinyCC prefix fixture, locked workspace build, format check, warning-denied Clippy, serial workspace tests (129 commit tests passed; 3 cgroup-dependent tests ignored by default), and maintained TinyCC SDK suite passed in the pinned Nix shell.
- Remaining: generate codecs for additional variable payloads and bounded collections; complete release profile and artifact gates.

## TC-2: C/Rust administrative-cancel codec parity — verified

- The C codec generator now also covers the administrative-cancel request with invocation ID and big-endian u16 reason code. Both the fixed 18-byte invocation-control request and 20-byte cancel request are emitted from the shared schema.
- Regression: the TinyCC fixture independently checks the administrative-cancel request bytes, decoded invocation ID, and decoded reason code. Rust has matching hand-checked tests for both payloads.
- Verification: generator drift/schema tests, locked workspace build, format, warning-denied Clippy, full serial workspace tests (129 commit tests passed; 3 cgroup-dependent cases ignored by default), and the maintained TinyCC suite passed.
- Remaining: extend generated C codec parity to other fixed payloads and define safe bounded helpers for collections and variable-length payloads; complete release profile and artifact gates.

## TC-2: generated terminal STATUS response codec — implemented

- Contract: [internal IPC](../../specs/ipc-protocol.md) defines terminal STATUS as version, status, signed observed time, unsigned commit sequence, and signed retention deadline.
- Red/green evidence: the signed timestamp regression initially failed to compile because this response lacked generated helpers. The schema now opts in the fixed-width codec, and terminal status responses use its encoder.
- Regression: the independent wire example covers negative observation/retention times and a positive commit sequence, checking exact bytes and typed round-trip decoding.
- Verification: generator drift/schema tests, locked workspace build, format check, warning-denied Clippy, serial workspace tests (96 library tests), and the maintained TinyCC SDK suite passed in the pinned Nix shell.
- Remaining: extend field-level schema and generated codecs to other payloads, add C serializer parity, and complete the release profile and artifact gates.

## TC-2: generated terminal cancellation-result codec — implemented

- Contract: [internal IPC](../../specs/ipc-protocol.md) specifies a terminal cancellation result as version, status, outcome status, `u64` commit sequence, and signed `i64` retention deadline. The shared schema had incorrectly declared the deadline `u64`; it now agrees with the wire contract and existing storage type without changing the field width or bytes.
- Red/green evidence: the signed-value regression initially failed to compile because the terminal result had no generated codec. The schema now opts into codec generation, and supervisor terminal cancellation responses use the generated encoder.
- Regression: the hand-checked example includes a negative retention value and verifies its two's-complement wire bytes plus signed round-trip decoding.
- Verification: generator drift/schema tests, locked workspace build, format check, warning-denied Clippy, serial workspace tests (95 library tests), and maintained TinyCC SDK tests passed in the pinned Nix shell.
- Remaining: extend field-level schema and generated codecs to other payloads, add C serializer parity, and complete the release profile and artifact gates.

## TC-2: generated simple cancellation-result codec — implemented

- Contract: [internal IPC](../../specs/ipc-protocol.md) and [toolchain](../../specs/toolchain.md). A simple cancellation result is the v1 payload version plus one status byte. The generated codec validates exact length and version; role-specific code remains responsible for interpreting the status set.
- Red/green evidence: the codec regression initially failed to compile because the schema had no generated helper. The schema now opts in the simple result payload, and supervisor cancellation response encoding uses the generated function for accepted, too-late, and unknown results.
- Regression: an independent three-byte example checks encoding, decoding, and truncated-payload rejection. Generator render tests assert helper generation for all opted-in fixed payloads.
- Verification: generator drift/schema tests, locked workspace build, format check, warning-denied Clippy, serial workspace tests (including 94 library tests), and the full maintained TinyCC SDK suite passed in the pinned Nix shell.
- Remaining: extend field-level schema and generated codecs to other payloads, add C serializer parity, and complete the release profile and artifact gates.

## TC-2: generated administrative-cancel codec — implemented

- Contract: [internal IPC](../../specs/ipc-protocol.md) and [toolchain](../../specs/toolchain.md). The administrative cancel request is the big-endian v1 payload version, a 16-byte invocation ID, and a big-endian u16 reason code; the decoder rejects invalid length or version.
- Red/green evidence: the new multi-field wire regression first failed to compile because this payload had no generated codec. The schema now opts it into the fixed-width Rust codec generator, and supervisor administrative-cancel dispatch uses the generated decoder.
- Regression: an independent 20-byte wire example checks encoding, tuple decoding, and truncated-payload rejection. Generator tests check both generated helper names and reject fixed-codec generation for trailing fields.
- Verification: generator drift and schema tests, locked workspace build, rustfmt check, warning-denied Clippy, serial workspace tests, and the maintained TinyCC SDK suite all passed in the pinned Nix shell.
- Remaining: extend field-level schema and generated codecs to other payloads, add C serializer parity, and complete the release profile and artifact gates.

## TC-2: generated executor relation-resolution request prefix codec — implemented

- Contract: [internal IPC](../../specs/ipc-protocol.md) and [toolchain](../../specs/toolchain.md). The request has a fixed v1 prefix containing the schema-name and relation-name byte lengths, followed by the bounded UTF-8 names. The generated Rust and C codecs handle the fixed version-and-length prefix; role-specific code validates and consumes the variable UTF-8 tail.
- Regression: the maintained TinyCC C fixture checks the exact prefix bytes, decodes both lengths, and appends the UTF-8 tail. The executor socket regression covers relation resolution through the request path.
- Verification: schema generation and drift tests, locked workspace build, rustfmt check, warning-denied Clippy, serial workspace tests (129 passed; 3 cgroup-gated tests ignored), and the full maintained TinyCC SDK suite passed in the pinned Nix shell.
- Remaining: extend generated schemas and codecs to the remaining IPC payloads, then complete release profile and artifact gates.

## TC-2: generated executor close-cursor request codec — implemented

- Contract: [internal IPC](../../specs/ipc-protocol.md) and [toolchain](../../specs/toolchain.md). The fixed request consists of the batch payload version and opaque 16-byte cursor handle.
- Implementation: the shared schema now generates exact Rust and C encoders and decoders. The Rust request codec and TinyCC executor SDK encoder call the generated helpers.
- Regression: generator tests require both generated C helpers; the executor socket-loop regression covers the Rust request path, and the maintained TinyCC executor fixture exercises the SDK close call.
- Verification: generator drift/schema tests, locked workspace build, rustfmt check, warning-denied Clippy, serial workspace tests (129 passed; 3 cgroup-gated tests ignored), and the complete maintained TinyCC SDK suite passed in the pinned Nix shell.
- Remaining: generate serializers for remaining executor/planner/supervisor payloads, then complete release profile and artifact gates.

## TC-2: generated executor next-batch request codec — implemented

- Contract: [internal IPC](../../specs/ipc-protocol.md) and [toolchain](../../specs/toolchain.md). The fixed request contains the batch payload version, cursor handle, maximum scanned rows, and maximum bytes; policy bounds remain validated by the Rust and C callers.
- Implementation: the shared schema generates exact Rust and C codecs. Both the Rust request type and TinyCC SDK encoder use the generated serializer; Rust uses the generated decoder before applying semantic limits.
- Regression: generator tests require both helpers, the executor socket-loop regression exercises Rust request handling, and the maintained TinyCC executor fixture exercises the SDK path.
- Verification: generator drift/schema tests, locked workspace build, rustfmt check, warning-denied Clippy, serial workspace tests (129 passed; 3 cgroup-gated tests ignored), and the complete maintained TinyCC SDK suite passed in the pinned Nix shell.
- Remaining: generate serializers for remaining executor/planner/supervisor payloads, then complete release profile and artifact gates.

## TC-2: generated TinyCC open-scan response decoder — implemented

- Contract: [internal IPC](../../specs/ipc-protocol.md) and [toolchain](../../specs/toolchain.md). The open-scan response is the batch payload version plus an opaque cursor handle.
- Implementation: the maintained TinyCC executor SDK now uses the generated C decoder to validate exact length and version and extract the handle before checking its executor cursor kind.
- Regression: the rebuilt TinyCC executor fixture exercises successful and invalid host responses; the Rust executor socket-loop regression continues to cover the matching server path.
- Verification: generator drift/schema checks, locked workspace build, rustfmt check, warning-denied Clippy, serial workspace tests, and the full maintained TinyCC SDK suite passed in the pinned Nix shell.
- Remaining: migrate the remaining hand-coded executor/planner/supervisor payload serializers and complete release profile and artifact gates.

## TC-2: generated TinyCC planner relation response codecs — implemented

- Contract: [internal IPC](../../specs/ipc-protocol.md) and [toolchain](../../specs/toolchain.md). Planner relation description returns either an exact unavailable result or an available response prefix followed by bounded relation metadata.
- Implementation: the TinyCC planner SDK now decodes the unavailable form and available fixed prefix with generated C helpers. It retains semantic validation for unsupported versions, handle kinds, identifiers, column metadata, and exact tail consumption.
- Regression: generator tests assert both helpers; the maintained TinyCC planner fixture covers valid, unavailable, malformed, and unsupported-version responses.
- Verification: generator drift/schema tests, locked workspace build, rustfmt check, warning-denied Clippy, serial workspace tests, and the complete maintained TinyCC SDK suite passed in the pinned Nix shell.
- Remaining: migrate the remaining hand-coded executor/planner/supervisor payload serializers and complete release profile and artifact gates.

## TC-2: generated TinyCC stage-delete request encoder — implemented

- Contract: [internal IPC](../../specs/ipc-protocol.md) and [toolchain](../../specs/toolchain.md). Stage-delete carries the batch payload version and opaque row handle.
- Implementation: the maintained TinyCC executor SDK now builds this request with the generated C encoder; Rust already uses the generated request codec.
- Regression: generator tests assert the C encoder and decoder; the maintained TinyCC executor fixture covers the SDK mutation path, and the executor socket-loop regression covers the Rust request path.
- Verification: generator drift/schema tests, locked workspace build, rustfmt check, warning-denied Clippy, serial workspace tests, and the complete maintained TinyCC SDK suite passed in the pinned Nix shell.
- Remaining: migrate the remaining hand-coded executor/planner/supervisor payload serializers and complete release profile and artifact gates.

## TC-2: generated TinyCC cancellation-check response decoder — implemented

- Contract: [internal IPC](../../specs/ipc-protocol.md) and [toolchain](../../specs/toolchain.md). Cancellation checks return the batch payload version and one boolean byte.
- Implementation: the TinyCC executor SDK now validates the exact response length and version and extracts the cancellation byte through the generated C decoder, then applies the boolean-domain check.
- Regression: generator tests assert the helper; the maintained TinyCC executor fixture checks false, true, and invalid boolean responses, and the Rust wire test checks the response bytes.
- Verification: generator drift/schema tests, locked workspace build, rustfmt check, warning-denied Clippy, serial workspace tests, and the complete maintained TinyCC SDK suite passed in the pinned Nix shell.
- Remaining: migrate the remaining hand-coded executor/planner/supervisor payload serializers and complete release profile and artifact gates.

## TC-2: generated TinyCC stage-insert response decoder — implemented

- Contract: [internal IPC](../../specs/ipc-protocol.md) and [toolchain](../../specs/toolchain.md). Stage-insert returns the batch payload version and opaque staged-row handle.
- Implementation: the TinyCC executor SDK uses the generated C decoder for exact length, version, and handle extraction, then checks the handle kind before writing the caller's output.
- Regression: generator tests assert the helper, and the maintained TinyCC executor fixture verifies the stage-insert response path and invalid handle behavior.
- Verification: generator drift/schema tests, locked workspace build, rustfmt check, warning-denied Clippy, serial workspace tests, and the complete maintained TinyCC SDK suite passed in the pinned Nix shell.
- Remaining: migrate the remaining hand-coded executor/planner/supervisor payload serializers and complete release profile and artifact gates.

## TC-2: generated TinyCC stage-update request prefix — implemented

- Contract: [internal IPC](../../specs/ipc-protocol.md) and [toolchain](../../specs/toolchain.md). Stage-update begins with the batch payload version, row handle, and u16 patch count, followed by bounded patch records.
- Implementation: the TinyCC SDK now uses the generated C prefix encoder; it retains existing bounds and validation while appending the variable patch records. Rust already uses the generated prefix codec.
- Regression: generator tests assert the helper; the maintained TinyCC executor fixture exercises mutation requests and the executor socket-loop regression covers the Rust path.
- Verification: generator drift/schema tests, locked workspace build, rustfmt check, warning-denied Clippy, serial workspace tests, and the complete maintained TinyCC SDK suite passed in the pinned Nix shell.
- Remaining: migrate remaining hand-coded executor/planner/supervisor payload serializers and complete release profile and artifact gates.

## TC-2: generated TinyCC stage-insert request prefix — implemented

- Contract: [internal IPC](../../specs/ipc-protocol.md) and [toolchain](../../specs/toolchain.md). Stage-insert begins with the batch payload version, relation handle, and u16 value count, followed by bounded value records.
- Implementation: the TinyCC SDK now uses the generated C prefix encoder and retains its existing validation and variable-value serialization. Rust already uses the generated prefix codec.
- Regression: generator tests assert the helper; the maintained TinyCC executor fixture exercises insert requests and the Rust mutation tests cover the corresponding payload.
- Verification: generator drift/schema tests, locked workspace build, rustfmt check, warning-denied Clippy, serial workspace tests, and the complete maintained TinyCC SDK suite passed in the pinned Nix shell.
- Remaining: migrate remaining hand-coded executor/planner/supervisor payload serializers and complete release profile and artifact gates.

## TC-2: generated optimizer SELECT request prefix — implemented

- Contract: [internal IPC](../../specs/ipc-protocol.md) and [toolchain](../../specs/toolchain.md). Optimizer SELECT requests begin with version, statement tag, and zero flags before variable relation, projection, predicate, and limit data.
- Implementation: the shared schema now generates exact Rust and C prefix codecs and supports the u8 registry constants used by the header. Rust consumes the generated decoder after preserving its unsupported-version error; the TinyCC SDK uses the generated encoder before appending the variable request tail.
- Regression: generator tests assert both helpers; all four Rust optimizer request parser cases cover unsupported version/tag, invalid flags, malformed names, and size/depth/collection bounds; the maintained TinyCC optimizer fixture checks SDK encoding.
- Verification: generator drift/schema tests, locked workspace build, rustfmt check, warning-denied Clippy, serial workspace tests, and the full maintained TinyCC SDK suite passed in the pinned Nix shell.
- Remaining: migrate remaining hand-coded executor/planner/supervisor payload serializers and complete release profile and artifact gates.

## TC-2: generated version-only supervisor terminal request codecs — implemented

- Contract: [internal IPC](../../specs/ipc-protocol.md) requires finalize and abort requests to contain exactly the supervisor payload version.
- Implementation: the generator now supports fixed codecs with no non-version fields, producing Rust `Option<()>` decoders and C validation helpers. Supervisor dispatch uses the generated Rust decoders for both operations.
- Regression: generator tests assert both C helpers; the C wire fixture checks exact v1 bytes, round trips, and unsupported-version rejection. Rust malformed-request coverage still checks truncation, version, and trailing bytes.
- Verification: generator drift/schema tests, locked workspace build, rustfmt check, warning-denied Clippy, serial workspace tests, and the full maintained TinyCC SDK suite passed in the pinned Nix shell.
- Remaining: migrate remaining hand-coded executor/planner/supervisor payload serializers and complete release profile and artifact gates.

## TC-2: generated TinyCC planner relation-resolution request prefix — implemented

- Contract: [internal IPC](../../specs/ipc-protocol.md) and [toolchain](../../specs/toolchain.md). Planner relation requests share the fixed version-and-two-length prefix followed by bounded schema and relation names.
- Implementation: the TinyCC planner SDK now uses the generated C prefix encoder before appending its validated identifier bytes; Rust executor and optimizer paths already consume the same schema-generated prefix.
- Regression: the maintained TinyCC planner fixture checks exact request bytes and lengths through its mock callback.
- Verification: generator drift/schema tests, locked workspace build, rustfmt check, warning-denied Clippy, serial workspace tests, and the complete maintained TinyCC SDK suite passed in the pinned Nix shell.
- Remaining: migrate remaining hand-coded executor/planner/supervisor payload serializers and complete release profile and artifact gates.

## TC-2: generated TinyCC executor fixed request and acknowledgement codecs — implemented

- Contract: [internal IPC](../../specs/ipc-protocol.md) and [toolchain](../../specs/toolchain.md). Executor relation resolution and open scan use generated request prefixes; close, update, and delete return the common version/status acknowledgement.
- Implementation: the TinyCC SDK now uses generated helpers for relation-resolution and open-scan request prefixes and validates mutation acknowledgements through the generated decoder. Existing bounded UTF-8 tails and semantic status checks remain in the SDK.
- Regression: maintained TinyCC executor fixtures check request bytes, accepted acknowledgements, and rejected statuses; generator tests require the helpers.
- Verification: generator drift/schema tests, locked workspace build, rustfmt check, warning-denied Clippy, serial workspace tests, and full maintained TinyCC SDK suite passed in the pinned Nix shell.
- Remaining: migrate remaining hand-coded supervisor/executor payload serializers and complete release profile and artifact gates.

## TC-2: generated TinyCC optimizer relation codecs — implemented

- Contract: [internal IPC](../../specs/ipc-protocol.md) and [toolchain](../../specs/toolchain.md). The optimizer resolves plan relations with the same version-and-length prefix and accepts either an exact unavailable response or an available fixed prefix followed by relation metadata.
- Implementation: the TinyCC optimizer SDK now uses the generated resolver-request prefix encoder and generated planner response decoders. It retains semantic identity matching and bounded relation metadata parsing.
- Regression: the maintained TinyCC optimizer-plan fixture checks request bytes, unavailable and available responses, malformed payloads, and identity mismatch.
- Verification: generator drift/schema tests, locked workspace build, rustfmt check, warning-denied Clippy, serial workspace tests, and full maintained TinyCC SDK suite passed in the pinned Nix shell.
- Remaining: migrate remaining hand-coded supervisor/executor payload serializers and complete release profile and artifact gates.

## TC-2: generated TinyCC optimizer response decoder — implemented

- Contract: [internal IPC](../../specs/ipc-protocol.md) defines a failure response as version/status and a success response as version/status/plan length followed by the plan bytes.
- Implementation: the TinyCC optimizer SDK now uses generated C decoders for both fixed response variants, then applies status-domain, plan-size, and exact-tail checks before exposing borrowed plan bytes.
- Regression: generator tests assert both decoders, the maintained TinyCC optimizer fixture covers success and failure response forms, and Rust response tests cover malformed lengths and invalid plans.
- Verification: generator drift/schema tests, locked workspace build, rustfmt check, warning-denied Clippy, serial workspace tests, and the full maintained TinyCC SDK suite passed in the pinned Nix shell.
- Remaining: migrate remaining hand-coded supervisor/executor payload serializers and complete release profile and artifact gates.

## TC-2: generated executor next-batch response codecs — implemented

- Contract: [internal IPC](../../specs/ipc-protocol.md) defines separate finished and continuation headers, with the cursor handle present only in the continuation form and row/cell records following either header.
- Implementation: the schema now generates Rust and C prefix codecs for both response variants. Rust selects and decodes the matching header through generated helpers before parsing bounded row/cell tails.
- Regression: generator tests assert both variants, the TinyCC wire fixture checks exact fields and accepts appended tail bytes, and the executor socket-loop integration exercises finished and continuation paths.
- Verification: generator drift/schema tests, locked workspace build, rustfmt check, warning-denied Clippy, serial workspace tests, and the full maintained TinyCC SDK suite passed in the pinned Nix shell.
- Remaining: migrate remaining hand-coded supervisor/executor payload serializers and complete release profile and artifact gates.

## TC-1 / TC-3: initial NixOS release-validation profile — implemented

- Selected profile: `nixos-x86_64-glibc-2.42`, CPU baseline `x86_64-v1`, with the pinned Nixpkgs glibc 2.42-100 runtime. This is evidence for the exact NixOS profile tested on this host, not a claim for older glibc or other distributions.
- Implementation: root Cargo release builds use `panic=abort`. `make -f engine/sdk/Makefile release-profile` stages seven engine binaries, the TinyCC SDK archive and headers, and three maintained query shared objects under `target/release-profile`; it removes the Nix shell RUNPATH from engine binaries and validates ELF64 x86_64, exact DT_NEEDED sets, GLIBC symbol ceilings, and non-executable query GNU_STACK. It writes `PROFILE.txt` and SHA-256 checksums for all staged files.
- Verification: clean locked Cargo release build passed; all staged artifacts passed the validator, every checksum verified with `sha256sum -c`, four binaries accepted `--help`, and executor/optimizer binaries loaded and reported usage when required arguments were absent. The query fixtures depend only on `libc.so.6` and require at most GLIBC_2.14; engine binaries depend on `libgcc_s.so.1`, `libc.so.6`, and `ld-linux-x86-64.so.2` and require at most GLIBC_2.39.
- Remaining: provide the production signing-key custody and public-key distribution process, package the pinned TinyCC runtime-support closure, measure clean SDK and per-query build timings, validate other runtime profiles, and complete sandbox compatibility/mismatch acceptance for the staged output.

## TC-5: deterministic checkable release-profile archive — implemented

- Implementation: `make -f engine/sdk/Makefile release-profile-package` writes a normalized tarball from the validated profile staging tree and a detached SHA-256 checksum file. Tar entry order, timestamps, numeric ownership, and gzip timestamps are fixed for repeatable output.
- Verification: the release-profile validator passed on the selected NixOS x86_64/glibc 2.42 host; checksum verification and repeat-build byte comparison are part of the acceptance for this archive.
- Remaining: signing is available through the separate TC-5 workflow and is intentionally not automatic; package the pinned compiler/runtime support closure and complete cross-profile release acceptance.

## TC-1: locked Nix development shell — implemented

- Behavior: `nix develop --locked` provides the pinned x86_64-linux package set used for the engine and SDK checks. [`flake.nix`](../../flake.nix) and [`flake.lock`](../../flake.lock) pin Nixpkgs revision `718f670e845328e9c2cecdd832835712b2cbd065` with source NAR hash `sha256-Ee5BWo8sh3pGt70QUdRajnaginqFmWvrtdBc4gJzhks=`.
- Package versions: Rust/Cargo/Clippy 1.95.0, rustfmt 1.9.0, TinyCC `0.9.27-unstable-2025-01-06`, GCC 15.3.0, binutils 2.46, GNU Make 4.4.1, and Minisign 0.12. `nix develop --offline --command` reproduced the locked shell tools.
- Verification: inside the locked shell, the maintained TinyCC SDK suite passed, `sandbox-query-fixture` built all three GNU-linked artifacts, and workspace build, format, warning-denied Clippy, and serial tests passed. The four cgroup-gated acceptances that the default suite reports as ignored were also run individually and passed with the pinned runtime profile and a temporary delegated cgroup parent; see [Linux sandbox progress](../linux-sandbox/PROGRESS.md).
- Changed contract: [`specs/toolchain.md`](../../specs/toolchain.md) records the development profile; [`AGENTS.md`](../../AGENTS.md) points checks to the locked shell and maintained SDK targets.
- TinyCC provenance: the pinned recipe records upstream commit `f6385c05308f715bdd2c06336801193a21d69b50`, source hash `sha256-tO3N+NplYy8QUOC2N3x0CO5Ui75j9bQzLSZQF1HQyhY=`, and Nix NAR hashes for the compiler, runtime-support library, and development/header outputs. These are recorded in [`specs/toolchain.md`](../../specs/toolchain.md).
- Remaining: package the pinned compiler/runtime-support closure and complete cross-profile acceptance; the selected release profile and Rust panic strategy are validated, while the locked shell remains a development environment rather than a distributable runtime package.

## TC-2: generated query ABI constants — partial

- Behavior: [`query_abi_constants.json`](../../engine/abi/query_abi_constants.json) is the common source for query ABI versions, symbol names, callback failure code, parameter/result bounds, IPC operation tags, IPC frame magic/version/header size/field offsets, payload cap, malformed-frame status values, the canonical gateway error payload strings, and v1-v3 native record declarations, sizes, alignments, and field offsets. [`generate_query_abi.py`](../../engine/tools/generate_query_abi.py) emits C record headers, Rust records, constants, layout assertions, and C operation-tag cases. Rust and C frame codecs consume the generated layout and operation tags. `make -f engine/sdk/Makefile abi-constants` regenerates them; the maintained SDK `test` target checks drift.
- Compatibility: The v3 invocation remains an alias of the v2 record. Generated assertions fail C and Rust builds if native layouts diverge from the schema; descriptor tests also reject unsupported versions/sizes.
- Verification: generated-file drift check and the full maintained TinyCC SDK suite passed, including the TinyCC-built C frame codec fixture with exact-byte, coalesced-frame, cap, malformed-input, no-partial-output, generated frame error-status, and canonical gateway error-payload checks. Rust tests check the same generated payload strings and frame status values. Workspace build, formatting, warning-denied Clippy, and serial workspace tests passed; the serial suite reported 3 cgroup-gated commit tests and 1 cgroup-gated metadata process test as ignored. The four ignored-by-default acceptance tests were separately run and passed; see [Linux sandbox progress](../linux-sandbox/PROGRESS.md).
- Remaining: move the remaining IPC payload schemas and serializers into the shared interface schema. TC-2 remains partial.

## TC-2 follow-up: generated relation-description name and count tails — implemented

- Contract: executor and planner relation-description payloads must use one shared source for their versioned fixed fields, length-prefixed relation name, and declared column count, while retaining the existing bounded column-record codecs.
- Red evidence: the generator regression initially failed because the available relation-description schemas emitted no relation-name or column-count tail helpers.
- Implementation: the schema now declares relation-name and column-count tails for executor resolution, planner description, and stable plan-relation description. The generator adds bounded length-prefixed UTF-8 and u16 tail codecs in C and Rust. Rust response encoders and decoders now use those generated helpers instead of hand-written name/count framing.
- Compatibility: wire bytes are unchanged. Empty relation names remain invalid; malformed or truncated UTF-8/count tails fail before column records are read.
- Verification: generator schema/drift tests, Rust generated-tail boundary and executor response round-trip tests, planner stream regression, and the forced TinyCC IPC payload codec fixture passed. The full forced TinyCC SDK suite, locked workspace build, format check, warning-denied Clippy, serial tests, and `git diff --check` passed; the commit integration reported 138 passed and 4 cgroup-dependent tests ignored.
- Remaining: other structured IPC tails and full shared-schema parity remain open.

## TC-2 follow-up: generated visible-column wire record — implemented

- Contract: relation-description column entries have identical Rust/C field order, widths, endianness, and name framing, while remaining a nested versionless record inside versioned payloads.
- Red evidence: generator tests initially failed because the shared schema had no `VISIBLE_COLUMN_RECORD` declaration or generated fixed-prefix/name-tail helpers.
- Implementation: the schema now declares the versionless column record and its length-prefixed name. The generator supports versionless records and fixed four-byte fields. Rust executor/planner IPC now uses generated codecs for column ID, ordinal, nullability, scalar type code/parameters, and column name; scalar type semantic validation remains in the executor codec.
- Compatibility: encoded column bytes are unchanged. Empty names, invalid nullable flags, malformed UTF-8, unsupported scalar codes/parameters, and truncated records still fail closed.
- Verification: generated schema/drift tests, Rust codec and executor IPC round-trip/malformed-input tests, and the forced TinyCC IPC payload codec fixture passed. The full forced TinyCC SDK suite, locked workspace build, format check, warning-denied Clippy, serial tests, and `git diff --check` passed; the commit integration reported 138 passed and 4 cgroup-dependent tests ignored.
- Remaining: additional structured IPC tails and full shared-schema parity remain open.

## TC-2 follow-up: generated scalar-value wire record — implemented

- Contract: IPC scalar VALUE entries share the type-code, u32 length, and raw value framing across executor insert/update requests and batch responses, while NULL, DEFAULT, and REDACTED state tags retain their existing surrounding forms.
- Red evidence: temporarily removing the scalar-value record declaration made the maintained generator test fail because `SCALAR_VALUE_RECORD` constants and codecs were absent.
- Implementation: the schema now declares a versionless type/length prefix and bounded raw-byte tail. Rust executor encoders and decoders use the generated helpers for mutation values and sanitized batch values. The C SDK's INSERT and UPDATE encoders use the generated scalar-value prefix. Rust raw-byte encode/decode helpers borrow rather than allocate a second copy.
- Compatibility: the on-wire bytes and state tags are unchanged. Existing scalar type, value-width, UTF-8, payload-size, and batch-budget validation remains in place.
- Verification: generator schema/drift tests (7 tests), the forced TinyCC SDK fixture suite, locked workspace build, format check, warning-denied Clippy, `git diff --check`, and the full locked workspace test suite passed. The commit integration suite reported 138 passed and 4 cgroup-dependent tests ignored; the other workspace integration suites also passed.
- Remaining: patch ordinal framing and other nested IPC records are still hand-coded; full shared-schema parity remains open.

## TC-2 follow-up: generated UPDATE patch ordinal record — implemented

- Contract: each UPDATE patch starts with one zero-based big-endian u32 ordinal, as specified in [internal IPC](../../specs/ipc-protocol.md); the four bytes remain unchanged on the wire.
- Red evidence: the generator regression failed because `PATCH_ORDINAL_RECORD` had no schema declaration or Rust/C codec helpers.
- Implementation: the shared schema declares the versionless four-byte ordinal record. Rust mutation encoders and decoders and the TinyCC SDK UPDATE encoder now use generated helpers; the removed handwritten u32 readers/writers had no other call sites.
- Compatibility: patch order, ordinal validation, duplicate detection, mutation state tags, and scalar value bytes are unchanged.
- Verification: generator schema/drift tests (7 tests), generated Rust codec boundary test, all 6 executor IPC tests, forced TinyCC SDK suite, locked workspace build, formatting check, warning-denied Clippy, `git diff --check`, and full locked workspace tests passed. The commit suite reported 138 passed and 4 cgroup-dependent tests ignored; the metadata process suite reported 1 passed and 1 cgroup-dependent test ignored.
- Remaining: other nested IPC records and full shared-schema parity remain open.

## TC-2 follow-up: generated executor batch-row record — implemented

- Contract: each `next_batch` row begins with a versionless row-handle[16]/cell-count-u16 prefix, followed by the declared sanitized cells, as described in [internal IPC](../../specs/ipc-protocol.md).
- Red evidence: the generator regression failed because `EXECUTOR_BATCH_ROW_RECORD` and its Rust/C helpers were absent.
- Implementation: the shared schema now defines the prefix. Rust batch response encoding/decoding and the TinyCC SDK batch parser use generated codecs; cell validation, row-handle kind checks, candidate limits, and response byte limits remain explicit.
- Compatibility: batch bytes are unchanged. Both decoders accept following cell data after the fixed row prefix and reject truncated prefixes.
- Verification: generator schema/drift tests (7 tests), generated Rust codec boundary test, all 6 executor IPC tests, forced TinyCC SDK suite, locked workspace build, formatting check, warning-denied Clippy, `git diff --check`, and full locked workspace tests passed. The commit suite reported 138 passed and 4 cgroup-dependent tests ignored; the metadata process suite reported 1 passed and 1 cgroup-dependent test ignored.
- Remaining: additional nested IPC records and full shared-schema parity remain open.

## TC-2 follow-up: generated sanitized-cell state tag — implemented

- Contract: each sanitized batch cell begins with exactly one state byte: VALUE 0, NULL 1, or REDACTED 2; only VALUE carries a scalar value record.
- Red evidence: the generator regression failed because `SANITIZED_CELL_TAG_RECORD` did not exist in the shared schema.
- Implementation: the schema generates Rust and C one-byte encoders/decoders. Rust batch response encoding/decoding and the TinyCC SDK batch parser now use them; state-specific validation remains at each caller.
- Compatibility: all state values and batch bytes are unchanged. Unknown values, missing tags, and truncated value records remain rejected.
- Verification: generator schema/drift tests (7 tests), generated Rust state-tag tests, all 6 executor IPC tests, forced TinyCC SDK suite, locked workspace build, formatting check, warning-denied Clippy, `git diff --check`, and full locked workspace tests passed. The commit suite reported 138 passed and 4 cgroup-dependent tests ignored; the metadata process suite reported 1 passed and 1 cgroup-dependent test ignored.
- Remaining: additional nested IPC records and full shared-schema parity remain open.

## TC-2 follow-up: generated C batch scalar-value decoder — implemented

- Contract: a VALUE cell carries the generated type-code/u32-length prefix followed by exactly the declared scalar bytes; the C SDK exposes a borrowed view into the batch payload.
- Implementation: the TinyCC batch parser now decodes the shared scalar-value prefix and length-checked byte tail instead of reading the type and length with local integer helpers. The borrowed view retains the original input-buffer lifetime.
- Compatibility: scalar tags, widths, cell boundaries, and value bytes are unchanged; malformed/truncated values remain rejected by bounded parsing and scalar validation.
- Verification: the forced TinyCC SDK suite passed, including the batch fixture, as did the locked workspace build, formatting check, warning-denied Clippy, `git diff --check`, and full locked workspace tests. The commit suite reported 138 passed and 4 cgroup-dependent tests ignored; the metadata process suite reported 1 passed and 1 cgroup-dependent test ignored.
- Remaining: additional nested IPC records and full shared-schema parity remain open.

## TC-2 follow-up: generated C scalar-value byte-tail encoders — implemented

- Contract: INSERT and UPDATE VALUE records append exactly their declared raw scalar bytes after the generated type/length prefix, within the caller's bounded request buffer.
- Implementation: TinyCC SDK mutation encoders now use the generated bounded byte-tail codec instead of direct copies; caller validation and the precomputed request-capacity checks remain in place.
- Compatibility: request bytes and caller-buffer ownership are unchanged; zero-length values remain valid and insufficient buffers fail before invoking the host.
- Verification: the focused SQL DML fixture, complete SDK suite, and full workspace gates passed.
- Remaining: additional nested IPC records and full shared-schema parity remain open.

## TC-2 follow-up: generated mutation cell state tags — implemented

- Contract: INSERT and UPDATE cells begin with one state byte: VALUE 0, NULL 1, and INSERT DEFAULT 2; VALUE carries the shared scalar-value record.
- Red evidence: the generator regression failed because `MUTATION_CELL_TAG_RECORD` and its Rust/C helpers were absent.
- Implementation: Rust INSERT/UPDATE encoders and decoders and TinyCC INSERT/UPDATE encoders use generated tag codecs. Rust INSERT values now use the shared scalar-value record encoder as well.
- Compatibility: state meanings, patch order, DEFAULT restrictions, and all encoded bytes remain unchanged.
- Verification: generator schema/drift tests (7 tests), all 6 executor IPC tests, forced TinyCC SDK suite, locked workspace build, formatting check, warning-denied Clippy, `git diff --check`, and full locked workspace tests passed. The commit suite reported 138 passed and 4 cgroup-dependent tests ignored; the metadata process suite reported 1 passed and 1 cgroup-dependent test ignored.
- Remaining: additional nested IPC records and full shared-schema parity remain open.

## TC-2 follow-up: generated C next-batch response header — implemented

- Contract: the C SDK validates the generated versioned finished/continuation response variants, including the optional cursor and row count, before walking batch rows.
- Implementation: `prdb_sdk_batch_open_v1` now consumes the generated C decoders and header offsets instead of manually parsing the version, finished flag, cursor, and row count. It preserves the distinct unsupported-version result.
- Compatibility: response layout and continuation pointer remain unchanged; invalid finished values, cursor kinds, truncated prefixes, and unsupported versions still fail.
- Verification: the maintained TinyCC batch fixture covers finished and continuation variants, cursor view, malformed headers, and unsupported versions. The locked workspace build, formatting check, warning-denied Clippy, `git diff --check`, and full locked workspace tests also passed. The commit suite reported 138 passed and 4 cgroup-dependent tests ignored; the metadata process suite reported 1 passed and 1 cgroup-dependent test ignored.
- Remaining: additional nested IPC records and full shared-schema parity remain open.

## TC-1 / TC-3: maintained TinyCC SDK and shared-artifact link profile — partial

- Observed development tools: Rust/Cargo 1.95.0 with rustfmt and Clippy; TinyCC `0.9.27-unstable-2025-01-06` on x86_64 Linux. The SDK archive and C fixtures build with TinyCC `-Wall -Wextra -Werror`.
- Red evidence: `tcc -shared` linked the archive, but its output omitted `PT_GNU_STACK`; the confined executor's glibc loader rejected it with `cannot enable executable stack as shared object requires`.
- Implementation: maintained query C objects and the archive remain TinyCC-built. The fixture link step now names `SHARED_LINKER` explicitly and applies GNU linker's `-z noexecstack` option. The Makefile documents why the linker differs from TinyCC.
- Verification: `make -f engine/sdk/Makefile test`, `make -f engine/sdk/Makefile sandbox-query-fixture`, and the DML/SQL and restricted-metadata artifacts linked with the maintained GNU linker passed their confined acceptance tests. `readelf` confirms `GNU_STACK` is RW, with no execute permission. The direct TinyCC-linked shared artifact was rejected at `dlopen` in the sandbox; it was not loaded in a trusted process.
- Remaining: define the release Linux/libc profile and measure clean SDK builds and per-query compile/link separately. The locked Nix shell makes development checks reproducible but is not the release artifact package. TC-2 tracks generated-interface parity separately.

## TC-3 / WI-7: reproducible SDK and per-query build timing harness — measured

- Harness: [`benchmark_sdk_build.py`](../../engine/tools/benchmark_sdk_build.py) builds the maintained SDK archive in separate empty output directories, times a no-op archive rebuild, then measures TinyCC source compilation and GNU shared-object linking independently. It reports all samples, medians, first versus subsequent query builds, and tool versions as JSON. Temporary outputs are removed after each run.
- Profile and result: three runs in the locked Nix development shell on Linux x86_64 with TinyCC `0.9.27-unstable-2025-01-06` and GCC `15.3.0`: clean archive build median 112.237 ms (110.608–121.462), no-op rebuild 2.696 ms, TinyCC query compilation first/subsequent median 2.828/3.127 ms, GNU link first/subsequent median 41.281/41.832 ms.
- Scope: first samples are not guaranteed cold because the OS cache is uncontrolled. These numbers are a local harness smoke measurement, not a deployment or target-hardware claim; SDK release packaging, client-side process startup, longer samples, and other runtime profiles remain open.
- Verification: `nix develop --offline --command python3 engine/tools/benchmark_sdk_build.py --runs 3` completed successfully with the pinned flake toolchain.

## TC-5: pinned TinyCC runtime/support closure — implemented

- Implementation: release staging resolves the active TinyCC executable to its Nix store path, copies that store path and its full closure into a local Nix binary cache inside `toolchain/tcc-cache`, and records the exact root and closure paths. The release validator checks the manifests against the cache metadata and includes all closure files in `SHA256SUMS`. `restore-tcc-closure.sh` verifies those profile checksums before importing the compiler closure into a local Nix store.
- Observed pinned closure: nine Nix store paths, 12 MiB as the generated binary cache, including TinyCC, its support library, glibc, development headers, and required runtime libraries.
- Trust: Nix signature checks are disabled only for the local import because the caller is required to authenticate the release archive and the copied NARs retain Nix hash verification. The production signing-key custody and public-key distribution process remains open.
- Verification: `nix develop --offline --command make -f engine/sdk/Makefile release-profile-package` built and validated all seven binaries and three query fixtures, checked that the cache metadata matches all nine unique closure paths, and generated `SHA256SUMS`. A second package build was byte-identical; `sha256sum -c` accepted the archive checksum and every staged profile checksum. After extraction, the restore script verified the profile checksums and imported all nine closure paths into an isolated temporary Nix store; `nix path-info --store ... --all` reported nine valid paths.
- Hardening: the release validator also requires cache metadata to reference exactly the compressed NAR files present, rejecting missing, escaping, or unreferenced NAR objects before packaging.
- Verification: a fresh `release-profile-package` run passed the new complete-cache validation and regenerated the archive and detached checksum.
- Negative acceptance: removed one NAR referenced by a copied cache's `.narinfo`; `validate_release_profile.py` rejected the profile with the missing-object path before checksum generation.

## TC-2 follow-up: generated external client frame header — implemented

- Contract: the external TCP frame retains its 32-byte `PDBN` v1.0 header with little-endian numeric fields as specified in [client protocol](../../specs/client-protocol.md).
- Red evidence: the generator schema regression failed because no shared fixed-header record covered the external frame; `ClientFrame::encode` and the streaming decoder each contained separate handwritten field encoders/decoders.
- Implementation: the schema now declares `CLIENT_FRAME_HEADER_RECORD`; the generator supports unsigned little-endian fixed-width fields in Rust and C codecs. The Rust encoder/decoder use the generated 32-byte record while keeping semantic checks for magic, protocol version, flags, kind, length, request ID, and sequence at the protocol layer.
- Compatibility: the maintained Rust regression checks the exact header bytes and full streaming round-trip; a TinyCC fixture checks the generated C codec against the same exact bytes and decoded values. No network frame bytes or validation rules changed.
- Verification: generator tests (7 passed), generated-file drift check, focused client-frame test, forced TinyCC SDK suite, locked workspace build, format check, warning-denied Clippy, full locked workspace tests (138 passed; 4 delegated-cgroup tests ignored), and `git diff --check` passed in the required Nix toolchain.
- Remaining: schema generation covers additional nested IPC payloads, but not every external protocol or client JSON operation; TC-2 parity and TC-5 release packaging/profile acceptance remain open.

## TC-2 follow-up: generated internal IPC frame header — implemented

- Contract: the internal `PRDI` v1 frame keeps its 20-byte big-endian header and existing semantic validation rules.
- Red evidence: the generator regression failed because `IPC_FRAME_HEADER_RECORD` and its Rust/C codecs were absent.
- Implementation: the schema now declares the internal header record; generated Rust and C codecs encode/decode the header used by `ipc.rs` and `ipc_frame_v1.c`. The generator checks that record field order, widths, offsets, and total length match the canonical IPC frame constants. It also treats a record's ordinary `version` field as data rather than a versioned-payload constant.
- Compatibility: the maintained Rust IPC and TinyCC C fixtures continue to match the hand-checked wire bytes and reject malformed headers; operation, request ID, and payload-size checks remain in the protocol layer.
- Verification: generator tests (8 passed), generated-file drift check, focused TinyCC IPC fixture, full TinyCC SDK suite, locked workspace build, formatting check, warning-denied Clippy, full locked workspace tests (138 passed; 4 delegated-cgroup tests ignored), and `git diff --check` passed in the required Nix toolchain.
- Remaining: additional nested IPC payload codecs and full shared-schema parity remain open.

## TC-2 follow-up: generated optimizer plan header — implemented

- Contract: the SELECT plan retains its four-byte header: big-endian version 1, kind 1, and reserved byte 0.
- Red evidence: generator regression failed because `OPTIMIZER_PLAN_HEADER_RECORD` and its Rust/C codecs were absent.
- Implementation: the shared schema now defines the plan header. Rust plan encoding and decoding and the TinyCC SDK decoder use generated codecs; semantic validation of supported version, SELECT kind, and zero reserved byte stays in their respective decoders. The Makefile tracks the generated codec header for the plan decoder object.
- Compatibility: the existing Rust golden header assertion and TinyCC maintained optimizer-plan fixture pass with the unchanged bytes and malformed-plan handling.
- Verification: generator tests (8 passed), generated-file drift check, full TinyCC SDK suite, locked workspace build, formatting check, warning-denied Clippy, full locked workspace tests (138 passed; 4 delegated-cgroup tests ignored), and `git diff --check` passed in the required Nix toolchain.
- Remaining: additional nested optimizer plan records and full shared-schema parity remain open.

## TC-2 follow-up: generated optimizer plan column record — implemented

- Contract: each optimizer plan column keeps its fixed 26-byte encoding: column ID, ordinal, scalar type code and parameters, and nullability.
- Red evidence: generator regression failed because `OPTIMIZER_PLAN_COLUMN_RECORD` and its Rust/C codecs were absent.
- Implementation: the shared schema now defines the record; Rust plan encoding/decoding and the TinyCC plan decoder use the generated codecs. Scalar type, nullable value, and nonzero column identity validation remain in the plan layer.
- Compatibility: the existing plan golden-byte round trip and maintained TinyCC optimizer-plan fixture pass with unchanged bytes and validation behavior.
- Verification: generator tests (8 passed), generated-file drift check, full TinyCC SDK suite, locked workspace build, formatting check, warning-denied Clippy, full locked workspace tests (138 passed; 4 delegated-cgroup tests ignored), and `git diff --check` passed in the required Nix toolchain.
- Remaining: additional nested optimizer plan records and full shared-schema parity remain open.

## TC-2 follow-up: generated optimizer plan identity records — implemented

- Contract: plan relation references retain separate 24-byte schema and relation identity records around their length-prefixed names.
- Red evidence: generator regression failed because the schema/reference identity records and generated codecs were absent.
- Implementation: the shared schema now defines the schema-ID/version and relation-ID/epoch records. Rust plan encoding/decoding and the TinyCC SDK decoder use their generated codecs; nonzero IDs and versions/epochs remain semantically checked by the plan layer.
- Compatibility: the canonical plan round-trip and TinyCC optimizer-plan fixture pass with unchanged bytes and invalid identity rejection.
- Verification: generator tests (8 passed), generated-file drift check, full TinyCC SDK suite, locked workspace build, formatting check, warning-denied Clippy, full locked workspace tests (138 passed; 4 delegated-cgroup tests ignored), and `git diff --check` passed in the required Nix toolchain.
- Remaining: additional nested optimizer plan records and full shared-schema parity remain open.

## TC-2 follow-up: generated optimizer plan column-list headers — implemented

- Contract: Scan and Project node headers contain their one-byte node tag followed by a big-endian u16 column count.
- Red evidence: generator regression failed because `OPTIMIZER_PLAN_COLUMN_LIST_HEADER_RECORD` and its Rust/C codecs were absent.
- Implementation: the shared schema defines the node tag/count record. Rust plan encoding and decoding and the TinyCC decoder now use the generated codec; node-order and count-limit checks remain in the plan layer.
- Compatibility: the hand-checked plan header/payload round trip and maintained TinyCC optimizer-plan fixture pass with unchanged bytes and malformed-count handling.
- Verification: generator tests (8 passed), generated-file drift check, full TinyCC SDK suite, locked workspace build, formatting check, warning-denied Clippy, full locked workspace tests (138 passed; 4 delegated-cgroup tests ignored), and `git diff --check` passed in the required Nix toolchain.
- Remaining: other nested optimizer plan records and full shared-schema parity remain open.

## TC-2 follow-up: generated optimizer plan Limit node — implemented

- Contract: the Limit node retains its one-byte tag and big-endian u32 row count.
- Red evidence: generator regression failed because `OPTIMIZER_PLAN_LIMIT_NODE_RECORD` and its Rust/C codecs were absent.
- Implementation: the shared schema defines the Limit node record. Rust plan encoding/decoding and the TinyCC decoder now use generated codecs; node ordering and row-limit semantics remain in the plan layer.
- Compatibility: the canonical plan round-trip and TinyCC optimizer-plan fixture pass with unchanged bytes and malformed-plan handling.
- Verification: generator tests (8 passed), generated-file drift check, full TinyCC SDK suite, locked workspace build, formatting check, warning-denied Clippy, full locked workspace tests (138 passed; 4 delegated-cgroup tests ignored), and `git diff --check` passed in the required Nix toolchain.
- Remaining: predicate, string, and other nested optimizer plan records and full shared-schema parity remain open.
