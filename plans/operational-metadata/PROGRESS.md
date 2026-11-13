# Operational Metadata Progress

## OM-2 transaction identity and age — verified slice

- Contract: the administrator runtime projection reports a live transaction's opaque ID, state, age at observation, staged write-set usage, and retained snapshot pin count.
- Implementation: the existing bounded invocation registry is the authoritative lifecycle registry for its one managed transaction. Admission assigns a nonzero random transaction ID unique among live entries; each page computes age from its observation timestamp without retaining extra history.
- Regression: tests cover distinct IDs, live age projection, typed decoding, serialized protocol output, and the real `runtime-list` process path.
- Verification: focused client/storage checks and the full serial delegated-cgroup workspace suite passed with zero ignored tests. Workspace build/fmt/Clippy, TinyCC tests/fixture, release-profile validation, and `git diff --check` passed.
- Remaining: separate wait/fence, job, memory/history, and counter registry families plus broader pressure/authority-race tests remain open.

## OM-2 invocation snapshot pin count — verified slice

- Contract: administrator invocation pages report `snapshot_pin_count`, 0 or 1 for the redb read transaction retained by each live invocation.
- Implementation: pin lifecycle updates follow read-transaction acquisition and release. Global administrator cursor generation advances when a pin count changes; owner-only cursors remain scoped to owner-visible state.
- Regression: storage and protocol checks verify active invocation pin counts, the TCP client decodes the field, and the delegated process CLI reports one for a running invocation.
- Verification: the focused regressions and full serial delegated-cgroup workspace suite passed with zero ignored tests. Workspace build/fmt/Clippy, TinyCC tests/fixture, release-profile validation, and `git diff --check` passed.
- Remaining: OM-2 still includes transaction, wait, job, memory, and counter registries plus broader pressure and authority-race tests.

## OM-2 invocation registry pages — verified slice

- Contract: [operational metadata](../../specs/operational-metadata.md), [client protocol](../../specs/client-protocol.md), and [client SDK](../../specs/client-sdk.md). Database administrators receive bounded invocation registry pages with an observation time and `best_effort` consistency label. Cursors are bound to the administrator and process-local registry generation; each page rechecks administrator authority.
- Implementation: the server's version-1 `runtime.invocations.list` operation is exposed through typed `AdminClient::list_runtime_invocations_page` and `prdb-client runtime-list`. The CLI defaults to 64 records, accepts 1–128, and emits only IDs, state, timestamps, cancellation metadata, and the opaque cursor.
- Regression: the fake TCP client checks exact request encoding and two-page decoding; the gateway/supervisor process test exercises CLI output. Storage coverage checks administrator denial, cursor binding, invalidation after registry changes, and authority revocation on a later page.
- Verification: the focused TCP paging test and the production gateway/supervisor lifecycle test passed. Workspace build, formatting, warning-denied Clippy, maintained TinyCC SDK tests, sandbox fixture build, and release-profile validation passed. The delegated cgroup probe reported writable `cpu memory pids`; the full serial workspace suite passed in the delegated service with zero ignored tests. `git diff --check` passed.
- Remaining: OM-2 still includes transaction, wait, job, memory, and counter registries plus broader pressure and authority-race tests.

## OM-2 invocation workload classification — verified slice

- Contract: administrator invocation projections include workload class as bounded scheduling metadata, and only database administrators receive the global registry.
- Implementation: `operational`/`analytical` is projected from the live admission record through the server protocol, typed client, and runtime-list JSON output.
- Regression: TCP decoding covers both classes, storage/protocol output checks the registry value, and the delegated gateway/supervisor process test queries an active invocation through the CLI.
- Verification: the intended decoder failure was observed. Focused regressions and the full serial delegated-cgroup suite passed with zero ignored tests; workspace build/fmt/Clippy, TinyCC SDK tests, sandbox fixture, release-profile validation, and `git diff --check` passed.
- Remaining: the rest of OM-2's registry families and broader pressure testing remain unimplemented.

## OM-2 staged invocation usage — verified slice

- Contract: administrators can inspect current staged row count and row-payload bytes for live invocations. Payload bytes exclude row IDs and index metadata; no row values are returned.
- Implementation: overlay usage is copied into the bounded admission registry after each successful stage/replacement/removal and cleared on abort. Administrative pagination serializes these values and invalidates continuations when usage changes.
- Regression: storage verifies staged, replaced, and deleted usage values plus stale cursor rejection. The typed client fixture decodes nonzero values, and a delegated gateway/supervisor process test checks the CLI page.
- Verification: the expected decoder rejection was observed before implementation. Workspace build/fmt/Clippy, TinyCC tests and fixture, release-profile validation, focused regressions, and full serial delegated-cgroup tests passed with zero ignored tests. `git diff --check` passed.
- Remaining: dedicated transaction ID/state/pin reporting and wait/job/memory/counter registries remain open.
