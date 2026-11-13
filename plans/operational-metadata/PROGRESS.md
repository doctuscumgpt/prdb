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

## OM-2 DDL fence waits — verified slice

- Contract: administrator runtime pages report current blocked DDL fence acquisitions as bounded best-effort metadata. Wait, resource, and blocker identifiers are opaque; no SQL, table names, or client addresses are exposed.
- Implementation: the fence manager tracks active writer/exclusive leases and blocked exclusive/writer requests. It rejects blocked acquisition growth beyond 128 live waiters, bounds each tracked request to 128 normalized resources, returns up to 16 waits with up to 8 resource IDs and 8 blocker IDs each, and signals each projection cap with truncation flags. Wait pages use the same administrator authorization path as invocation pages; each wait snapshot is sampled independently and is not pinned by an invocation cursor.
- Regression: tests verify blocker relationships, cleanup, bounded admission and response truncation, typed protocol decoding, and the authorized runtime page.
- Verification: focused regressions, workspace build/fmt/Clippy, TinyCC tests and fixture, release-profile validation, delegated cgroup probe, and full serial delegated-cgroup tests passed with zero ignored tests. `git diff --check` passed.
- Remaining: other wait/fence categories, job, memory/history, and counter registries plus broader pressure and authority-race testing remain open.

## OM-2 pinned-history summary — verified slice

- Contract: administrator runtime pages include a best-effort aggregate count of active invocation-owned redb snapshots and the oldest pin age at observation. This reports live snapshot pins only and does not estimate retained page bytes or memory.
- Implementation: the bounded invocation registry records a monotonic pin start when its snapshot count transitions to one and clears it when released. Each authorized runtime page copies the global pin count and oldest age independently of page records; the summary is not retained by a cursor.
- Regression: the production gateway/supervisor output assertion failed before implementation because the summary was absent, then passed after implementation. Storage and typed TCP fixtures verify counts, age, and serialization.
- Verification: focused storage and TCP tests, workspace build/fmt/Clippy, TinyCC tests and sandbox fixture, release-profile validation, delegated cgroup probe, full serial delegated-cgroup suite (zero ignored), and `git diff --check` passed.
- Remaining: redb retained-byte/cache telemetry, job and counter registries, additional wait categories, and broader pressure/authority-race testing remain open.

## OM-2 admission and conflict counters — verified slice

- Contract: administrator runtime pages report successful admissions and invocation commits rejected with `CommitError::Conflict` in the current process-local registry. Both counters saturate at `u64::MAX` and reset if the registry is recreated; neither is a durable lifetime total.
- Implementation: successful registry insertion increments admissions; the invocation commit failure path increments conflicts only for the explicit conflict variant. The bounded runtime projection carries both counters through protocol, typed client, and CLI.
- Regression: the authorized runtime response failed when the conflict field was omitted, then passed after restoration. A storage race test checks the count after real conflicting commits, the TCP fixture verifies decoding, and a unit test verifies saturation of both counters.
- Verification: focused commit-race, runtime-page, typed TCP, and saturation tests; workspace build/fmt/Clippy; TinyCC tests and sandbox fixture; release-profile validation; delegated cgroup probe; full serial delegated-cgroup workspace suite (service exited successfully); and `git diff --check` passed.
- Remaining: byte and latency counters; job registries; redb retained-byte/cache telemetry; additional wait categories; and broader pressure/authority-race testing remain open.

## OM-2 audited administrator cancellation counter — verified slice

- Contract: `admin_cancellations_accepted_total` counts only the first transition to cancellation-requested after its accepted audit is durably committed. Denials, too-late outcomes, and repeat accepted requests do not increment it; it saturates and resets with the process-local invocation registry.
- Implementation: the cancellation path increments the registry counter only after an accepted, changed cancellation survives the audit boundary. The authorized runtime page, strict typed TCP client, and CLI JSON carry the field.
- Regression: the runtime-field assertion failed before implementation. Lifecycle coverage checks one first cancellation plus an accepted repeat remains at one; the saturation test includes the cancellation counter, and the typed client fixture decodes it.
- Verification: focused storage, client, and saturation tests; workspace build/fmt/Clippy; TinyCC tests and sandbox fixture; release-profile validation; delegated cgroup probe; full serial delegated-cgroup workspace suite (all tests passed with zero ignored); and `git diff --check` passed.
- Remaining: byte and latency counters; job registries; retained-byte/cache telemetry; additional wait categories; and broader pressure/authority-race tests remain open.

## OM-2 audited administrator cancellation denial counter — verified slice

- Contract: `admin_cancellations_denied_total` counts only administrative cancellation attempts whose `Denied` AUD1 record was durably committed. Unauthorized, rate-limited, unaudited late requests, and failed audit commits are excluded. It is process-local, saturating, and resets with the invocation registry.
- Implementation: the existing COMMITTING and absent-invocation branches increment the counter after the durable denied audit succeeds; the runtime page, strict client decoder, and CLI expose it.
- Regression: the authorized runtime response failed before the new field existed. Lifecycle coverage verifies accepted cancellation leaves denials at zero, terminal and unknown targets with committed denied audits increment it, and rate limiting does not.
- Verification: focused cancellation lifecycle, runtime page, typed TCP and saturation tests; workspace build/fmt/Clippy; TinyCC suite and sandbox fixture; release-profile validation; delegated cgroup probe; full serial delegated-cgroup workspace suite with zero ignored tests; and `git diff --check` passed.
- Remaining: byte and latency counters; job registries; retained-byte/cache telemetry; additional wait categories; and broader pressure/authority-race tests remain open.

## OM-2 invocation policy denial counter — verified slice

- Contract: `invocation_policy_denials_total` counts each managed-invocation executor IPC request that returns explicit `AuthorizationError::Denied`. Hidden rows/cells, unavailable relations, malformed requests, and policy evaluation failures are excluded. It is administrator-only, process-local, saturating, and resets with the invocation registry.
- Implementation: the executor dispatch boundary records the denial through a weak metrics handle, so an authorization error can be counted after its transaction aborts and releases the admission permit without extending registry lifetime.
- Regression: an ordinary principal with SELECT but no INSERT resolves a visible relation and receives 64 real denied `StageInsert` requests across sequential managed invocations; the administrator runtime response reports all 64, and the page confirms denied invocations release their admission entries. TCP decoding and saturation coverage exercise the field.
- Verification: observed the runtime assertion fail before implementation; focused 64-denial executor/runtime, TCP, and saturation tests; workspace build/fmt/Clippy; TinyCC suite and fixture; release-profile validation; delegated cgroup probe; full serial delegated-cgroup workspace suite with zero ignored tests; and `git diff --check` passed.
- Remaining: byte and latency counters; job registries; retained-byte/cache telemetry; additional wait categories; and broader pressure/authority-race tests remain open.

## OM-2 cumulative staged payload byte counter — verified slice

- Contract: `invocation_staged_value_bytes_total` adds encoded payload bytes for every successful managed-invocation overlay put. Replacements add the replacement payload again; deletes and failed writes add nothing. It is cumulative accepted staging volume, distinct from the current per-invocation `staged_value_bytes` gauge. The global counter is administrator-only, saturating, process-local, and resets with the registry.
- Implementation: successful `stage_put_inner` operations report payload length through their admission permit into the shared registry. The admin runtime page, protocol JSON, strict typed client, and CLI JSON expose the counter.
- Regression: the expected runtime field failed before implementation. Storage coverage checks initial put, replacement, and delete semantics; TCP decoding and saturation tests cover the client field and `u64::MAX` bound.
- Verification: focused storage/runtime, protocol JSON, TCP client and saturation tests; workspace build/fmt/Clippy; TinyCC SDK tests and sandbox fixture; release-profile validation; delegated cgroup probe; full serial delegated-cgroup workspace suite (zero ignored tests); and `git diff --check` passed.
- Remaining: latency buckets, job registries, retained-byte/cache telemetry, additional wait categories, and broader pressure/authority-race tests remain open.

## OM-2 managed commit latency buckets — verified slice

- Contract: `invocation_commit_latency_buckets` reports six noncumulative counts for managed `InvocationTransaction::commit` calls, whether the call succeeds or errors. Durations run from API entry through return and use monotonic time; bucket boundaries are `<=1,000us`, `(1,000us,5,000us]`, `(5,000us,10,000us]`, `(10,000us,50,000us]`, `(50,000us,100,000us]`, and `>100,000us`. Uncoordinated transactions are excluded. Counts saturate and reset with the process-local registry.
- Implementation: the managed commit wrapper reports the complete call duration through a weak admission metrics handle; the registry classifies full `Duration` values, and the admin runtime page, JSON protocol, typed client, and CLI carry the fixed six-count array.
- Regression: the expected field was absent before implementation. Storage tests cover each range and a sub-microsecond boundary, saturation tests include the histogram, and the integration page confirms an errored managed commit increments one bucket. The authenticated gateway/supervisor CLI test checks the new counter fields through the real process boundary.
- Verification: focused runtime, client, boundary and saturation tests plus the delegated metadata-process test; workspace build/fmt/Clippy; TinyCC SDK tests and sandbox fixture; release-profile validation; delegated cgroup probe; full serial delegated-cgroup workspace suite (zero ignored tests); and `git diff --check` passed.
- Remaining: job registries, retained-byte/cache telemetry, additional wait categories, and broader pressure/authority-race tests remain open.

## OM-2 durable-outcome maintenance counters — implemented slice

- Contract: `outcome_maintenance_passes_total` counts every bounded pruning call, `outcome_maintenance_failures_total` counts calls returning an error, and `outcomes_pruned_total` adds removals only from successful calls. They are administrator-only, process-local, saturating counters that reset with the runtime registry and expose neither failure details nor outcome contents.
- Implementation: the storage manager records maintenance results in the shared process registry; runtime-list operation version 2 carries the three counters through the authenticated page, strict TCP client, and CLI. Version 1 keeps its previous strict response shape. No cache or retained-page estimate is inferred from redb APIs.
- Regression: a real-redb unit test prunes one expired outcome, submits an invalid batch size, and verifies two passes, one failure, and one removal. Unit coverage checks saturation and successful/failed accounting; JSON and typed-client fixtures verify the wire fields; the production CLI acceptance confirms the service's startup pruning pass, zero failures, and a numeric removal total. The first CLI assertion incorrectly expected zero passes; the service starts its first maintenance pass immediately, so the regression now checks observed maintenance instead.
- Verification: focused real-redb accounting, saturation, runtime JSON, typed TCP, storage runtime authorization, and delegated CLI tests; workspace build/fmt/Clippy; TinyCC tests and sandbox fixture; release-profile validation; delegated cgroup probe; full serial delegated-cgroup workspace suite; and `git diff --check`.
- Remaining: job status registries, retained-byte/cache telemetry, additional wait categories, and broader pressure/authority-race tests remain open.

## OM-5/OM-6 administrator audit inspection — implemented slice

- Contract: `audit.events.list` returns exclusive event-ID ordered pages of 1–128 AUD1 projections. Each request checks current database-administrator authority in the same redb snapshot as the audit scan; the cursor is administrator-bound. Independent pages are best-effort, and random event IDs do not promise chronological order. A malformed AUD1 value or key/value event-ID mismatch fails the page closed. The response contains only event/actor/correlation IDs, timestamp, bounded action/result codes, and target IDs.
- Implementation: added the bounded storage listing, strict version-1 control request decoder, JSON projection, typed `AdminClient` page API, and `prdb-client audit-list`. The decoder rejects unknown fields, invalid limits, noncanonical cursors, and unsupported operation versions. No audit bytes, secret material, diagnostics, query contents, or row values are returned.
- Regression: storage tests cover non-admin denial, cursor ordering/round-trip/admin binding, limit bounds, authority revocation between pages, and key/value mismatch. Protocol tests cover request version, bounds, and cursor shape; typed TCP tests cover response decoding and continuation; the delegated process test exercises `audit-list` through the real authenticated gateway and supervisor.
- Verification: focused storage, protocol, and typed-client tests; workspace build/fmt/Clippy; TinyCC SDK tests and sandbox fixture; release build and release-profile validation; delegated cgroup probe; full serial delegated-cgroup workspace suite (service exited successfully, including sandbox and process tests); and `git diff --check`.
- Remaining: audit retention and sink delivery, job registries, retained-byte/cache telemetry, additional wait categories, and broader pressure testing remain open.

## OM-5 rate-limited authentication-failure audit — implemented slice

- Contract: failed token authentication records a denied AUD1 `AuthenticationFailure` event with no actor ID. A nonzero attempted token ID is the sole target; a zero token ID has no targets. A shared process-local limiter persists at most four events per token ID per rolling minute and tracks at most 4,096 token IDs. Later failures remain denied without individual audit records.
- Implementation: `StorageManager::authenticate_query_session` writes the failure event synchronously before returning the same `AuthenticationFailed` result. Random event and correlation IDs are generated by the engine. An audit write error marks the database handle uncertain and fails authentication closed.
- Regression: storage coverage submits five invalid-secret attempts for a real token and finds exactly four actor-free denied events, then checks the zero-token event has no target. The authenticated gateway/supervisor process test performs a failed AUTH and reads the event through the administrator audit API.
- Verification: focused storage and delegated process tests; workspace build/fmt/Clippy; release build/profile validation; TinyCC SDK suite and sandbox fixture; full serial delegated-cgroup workspace suite (service exited successfully); and `git diff --check`.
- Remaining: audit retention and sink delivery, job registries, retained-byte/cache telemetry, additional wait categories, and broader pressure testing remain open.
