# Operational Metadata Progress

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
