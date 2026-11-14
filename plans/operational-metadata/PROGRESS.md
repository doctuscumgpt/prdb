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
- Remaining: asynchronous external sink delivery, job registries, retained-byte/cache telemetry, additional wait categories, and broader pressure testing remain open.

## OM-5 rate-limited authentication-failure audit — implemented slice

- Contract: failed token authentication records a denied AUD1 `AuthenticationFailure` event with no actor ID. A nonzero attempted token ID is the sole target; a zero token ID has no targets. A shared process-local limiter persists at most four events per token ID per rolling minute and tracks at most 4,096 token IDs. Later failures remain denied without individual audit records.
- Implementation: `StorageManager::authenticate_query_session` writes the failure event synchronously before returning the same `AuthenticationFailed` result. Random event and correlation IDs are generated by the engine. An audit write error marks the database handle uncertain and fails authentication closed.
- Regression: storage coverage submits five invalid-secret attempts for a real token and finds exactly four actor-free denied events, then checks the zero-token event has no target. The authenticated gateway/supervisor process test performs a failed AUTH and reads the event through the administrator audit API.
- Verification: focused storage and delegated process tests; workspace build/fmt/Clippy; release build/profile validation; TinyCC SDK suite and sandbox fixture; full serial delegated-cgroup workspace suite (service exited successfully); and `git diff --check`.
- Remaining: asynchronous external sink delivery, job registries, retained-byte/cache telemetry, additional wait categories, and broader pressure testing remain open.

## OM-5 bounded append-only audit capacity — implemented slice

- Contract: the AUDIT table accepts at most 16,384 events (approximately 65 MiB of raw AUD1 payload at the maximum encoded event size, before redb overhead). Audit history remains append-only; once full, any operation that must persist an audit event fails closed with an explicit capacity error. Startup rejects stores already beyond the bound.
- Implementation: the common commit path and bootstrap/credential rotation, authentication-failure, and managed-cancellation audit writers check capacity before insertion. No event is silently evicted; recovery of capacity awaits a separately specified retention/archive mechanism.
- Regression: the audit capacity helper checks the exact boundary and integer overflow. The real-redb authorization test fills the table to the bound and verifies the next failed-authentication audit returns the capacity error without changing its authentication behavior before the bound.
- Verification: workspace build, formatting, warning-denied Clippy, release build/profile validation, TinyCC SDK tests and sandbox fixture, and the full serial delegated-cgroup workspace suite passed (service exited successfully, zero ignored tests). `git diff --check` passed.
- Remaining: external sink delivery, job registries, retained-byte/cache telemetry, additional wait categories, and broader pressure testing remain open.

## OM-5 offline audit archive and capacity reclamation — implemented slice

- Contract: [operational metadata](../../specs/operational-metadata.md) defines PRA1 v1 as a sorted AUD1 export with a SHA-256 corruption check. Offline archival requires the exclusive instance lock and a destination outside the database directory. Audit rows are removed only after a mode-0600 archive and its parent directory are synced; an existing matching archive supports retry after a crash between archive publication and redb pruning.
- Implementation: `StorageInstance::archive_audit_events` validates every AUD1 record and key, creates a no-overwrite hard-linked archive from a synced temporary file, verifies retries against the exact current table, and removes the captured records in one writer transaction. `prdb-maintain audit-archive --instance-dir PATH --output PATH` exposes this after services stop.
- Regression: the initial process test failed because `prdb-maintain` rejected `audit-archive`. Tests now verify archive creation and private permissions, table pruning, idempotent retry, rejection of output paths inside the instance, malformed destination preservation, and refusal while the instance lock is held. Codec tests cover round-trip, event ordering, and digest corruption.
- Verification: focused archive codec and process tests passed. Workspace build, fmt, warning-denied Clippy, release build, TinyCC tests/fixture, release-profile validation (10 engine binaries and 1 query fixture), delegated cgroup probe, and the full serial delegated-cgroup workspace suite passed. The full service exited successfully with zero ignored tests; `git diff --check` passed.
- Remaining: external asynchronous sink delivery, job registries, retained-byte/cache telemetry, additional wait categories, and broader pressure testing remain open.

## OM-5 PRA1 offline archive verification — implemented slice

- Contract: `prdb-maintain audit-verify --archive PATH` validates a mode-0600 regular PRA1 file up to the format bound without opening an instance or exposing event contents.
- Implementation: the verifier opens with `O_NOFOLLOW|O_CLOEXEC`, bounds the read to the maximum PRA1 size, checks the complete format digest, sorted event IDs, AUD1 payloads, and key/value identity, then reports only the event count.

- Regression: the process test verifies an independently generated valid archive and rejects a modified digest while preserving the archive bytes. CLI help includes the verifier operation.
- Verification: the focused process test and full `bootstrap_binary` target passed. Workspace build/fmt/Clippy, release build/profile validation (10 engine binaries and 1 query fixture), TinyCC tests/fixture, delegated cgroup capability probe, full serial delegated-cgroup workspace suite (zero ignored tests), release-profile CLI help, and `git diff --check` passed.
- Remaining: external asynchronous sink delivery, job registries, retained-byte/cache telemetry, additional wait categories, and broader pressure testing remain open.

## OM-6 optional local JSONL audit sink — implemented slice

- Contract: when `PRDB_AUDIT_JSONL_PATH` is configured with an absolute path, the supervisor drains the durable AUDIT outbox in bounded batches. It syncs each mode-0600 JSONL line before recording its STATS delivery marker. Failures retain pending AUDIT events; a crash between sink sync and marker commit can replay a duplicate keyed by `event_id`. Path or sink-file identity changes replay retained history, and offline archival removes corresponding markers atomically.
- Implementation: added a private, no-follow, single-link sink with nonblocking file locking and recovery of a partial final line. Supervisor retries every second without changing committed request outcomes. JSON contains only IDs, timestamp and numeric action/result codes. Updated [operational metadata](../../specs/operational-metadata.md) with format, retry, security and Landlock behavior.
- Regression: first observed the focused integration test fail because `audit_sink` did not exist; a file-removal test then exposed immediate inode reuse when identity used only device/inode. Real-redb tests cover durable delivery, repeat suppression, replay on path or file replacement, sink failure retention, partial-line recovery, private permissions and absolute-path validation. A real supervisor/gateway process test checks delivery of a failed-authentication event; archive coverage verifies pruning removes markers atomically.
- Verification: pinned toolchain versions matched; workspace build, fmt, warning-denied Clippy, and release build passed. TinyCC SDK tests and the sandbox fixture passed; release profile validated 10 engine binaries and 1 query fixture. The delegated cgroup probe passed with writable `cpu memory pids`; the full serial delegated-cgroup workspace suite passed with zero ignored tests. Focused sink tests, archive marker test, release-profile validation, and `git diff --check` passed.
- Remaining: remote sink integrations, job registries, retained-byte/cache telemetry, additional wait categories, and broader pressure testing remain open.

## OM-2 asynchronous job boundary — clarified

- Contract refinement: no asynchronous maintenance jobs are exposed today. DDL/index builds are synchronous control operations; redb compaction is an offline `prdb-maintain compact` operation under the exclusive instance lock. The registry remains deferred until a supported background operation can provide real queued/running/completed phases and recovery semantics.
- Specification: replaced the obsolete checkpoint/reclamation job examples in [operational metadata](../../specs/operational-metadata.md) with the actual redb ownership and maintenance behavior. The plan now records why an empty or synthetic job registry would be misleading.
- Verification: documentation diff and link/consistency checks only; no Rust behavior changed.
- Remaining: define a job API and registry alongside the first supported asynchronous operation; job tracking itself is not implemented.

## OM-2 redb cache usage — withdrawn after pressure regression

- Initial implementation: pinned redb 4.4.0 `cache_metrics` was exposed as runtime operation version 3 through storage, TCP client, protocol and CLI.
- Withdrawal evidence: adding runtime-page sampling to the concurrent writer-pressure test reproduced a panic inside redb 4.4.0 `cached_file.rs`: `read_total - read_hits` underflows when independent atomic counters are sampled during concurrent reads. This optional metric cannot be allowed to crash a monitoring request, so the feature flag, version-3 field and dependent code were removed. Runtime operation version 2 remains unchanged.
- Verification: the pressure test failed before withdrawal with the redb panic. The exact redb source and subtraction were inspected. The delegated workspace suite had passed before this additional concurrent scenario; rerun after withdrawal is recorded in the next OM-7 entry.
- Remaining: exact cache/retained-history bytes remain unavailable until redb offers a safe concurrent read API; job registry, other waits, and broader pressure/authority-race work remain open.

## OM-7 bounded invocation-page copies — implemented slice

- Contract: owner and administrator invocation pages inspect the live registry without copying/sorting every matching record under the registry mutex. Page materialization is bounded to `page_limit` records plus one ID to determine whether a continuation exists; page limits remain capped at 128.
- Implementation: selection retains the smallest eligible `limit + 1` invocation IDs in a bounded max-heap, sorts that prefix, and copies only the requested records. The registry scan remains proportional to active invocations, but temporary page memory and record projection are bounded by the page limit. Administrator authority snapshots are released before registry access; formatting follows projection after the mutex is released.
- Regression: a new unit test checks sorted-prefix selection after a cursor, the extra continuation candidate, and a zero-sized helper request. Existing own-page and administrator-page integration tests verify authorization, ordering, continuation, generation invalidation, and projection. The retained-snapshot writer-pressure regression now samples administrator pages during the write burst and confirms the retained invocation remains visible.
- Verification: initial selector test compile failed as expected; owner-page and administrator-page integration tests passed. Sampling the redb cache metric during the writer-pressure test exposed an internal panic; after withdrawing that metric and restoring protocol version 2, the pressure test and typed TCP paging test passed. Workspace build, fmt, warning-denied Clippy, and the full serial delegated-cgroup suite passed with zero ignored tests; `git diff --check` passed.
- Additional pressure regression: `denied_monitoring_pressure_does_not_starve_commit_or_cancellation` continuously issues unauthorized runtime-page and audit-page reads while committing a real invocation and accepting and observing cancellation of another. It verifies at least 512 denials for each API and the durable ABORTED outcome.
- Verification: the standalone integration test passed against real redb storage in 0.07 seconds. Formatting, workspace build, warning-denied Clippy, and the full serial delegated-cgroup workspace suite passed; the suite included both the pressure regression and the existing authorization/discovery integration test, with zero ignored tests. The cgroup probe reported a writable parent with `cpu memory pids` enabled, and `git diff --check` passed.
- Remaining: broader pressure coverage for other monitoring APIs and OM-2 registry families; synchronous runtime/audit denial pressure during commit and cancellation is covered.

## OM-6 bounded JSONL outbox batches — verified follow-up

- Contract: each sink tick materializes and decodes no more than its requested pending-event batch. A malformed event beyond that batch does not block earlier events; it fails closed when a later tick reaches it.
- Red/green: the integration test seeded 64 valid events followed by one malformed AUD1 row. Before the fix, delivery failed before writing any line because the helper decoded the full audit table; after the fix, the first 64 events deliver and the next batch rejects the malformed row.
- Implementation: pending-event collection now takes the batch limit and stops once that many undelivered records are collected, keeping temporary event memory bounded by the sink's 64-record maximum.
- Verification: all five `audit_sink` integration tests passed; `cargo build --workspace --locked`, `cargo fmt --all -- --check`, warning-denied workspace Clippy, the maintained TinyCC SDK suite and sandbox fixture passed. The full serial workspace suite passed in the delegated user service, including all sandbox and process tests with zero ignored tests. `git diff --check` passed.
- Remaining: remote sink integrations, job registries, retained-byte/cache telemetry, additional wait categories, and broader pressure testing remain open.

## OM-6 large-file partial-line recovery — verified follow-up

- Contract: a crash-truncated final JSONL line is recoverable even when the complete sink file exceeds the per-line bound; recovery examines only a bounded suffix and rejects a partial line longer than that bound.
- Red/green: a sink with a 2,000-line complete prefix and a short partial tail failed before the change with `InvalidSinkFile`; it now preserves the full prefix, removes the partial bytes, and appends the pending event.
- Implementation: recovery seeks to at most `MAX_LINE_LENGTH` bytes from EOF, finds the last newline in that suffix, truncates after it, syncs the repair, and resumes appending. A suffix with no newline is rejected only when the partial line exceeds the configured bound.
- Verification: the focused regression passed after failing before implementation; all six audit-sink integration tests passed. Workspace build, formatting, warning-denied Clippy, and the full serial delegated-cgroup workspace suite passed, including sandbox and process tests with zero ignored tests. `git diff --check` passed.
- Remaining: remote sink integrations, job registries, retained-byte/cache telemetry, additional wait categories, and broader pressure testing remain open.

## OM-6 sink-sync/marker crash window — verified

- Contract: if the JSONL line is synced but its STATS delivery marker is not committed, retry may append the same audit event again; each line retains the stable `event_id` deduplication key.
- Regression: `synced_line_before_marker_commit_is_replayed_after_database_reopen` injects an interruption immediately after `sync_all`, confirms the complete private line exists, closes and reopens the redb database, then retries delivery. The event is appended a second time with identical contents, demonstrating the documented at-least-once behavior across recovered durable state.
- Implementation: the public delivery function uses a private generic post-sync continuation; production proceeds directly to the marker commit, while the unit test returns an injected interruption at the exact durability boundary.
- Verification: the focused Rust regression passed; workspace build, format check, warning-denied Clippy, maintained TinyCC SDK tests, sandbox-query fixture build, and the full serial delegated-cgroup workspace suite passed with zero ignored tests. The delegated probe reported writable cgroup v2 with `cpu`, `memory`, and `pids` available and enabled. `git diff --check` passed.
- Scope: the injected interruption and database reopen exercise the durable boundary without killing the test process or simulating power loss. Full process/power-loss behavior remains limited by the documented filesystem durability contract. Remote sink integrations, job registries, retained-byte/cache telemetry, additional wait categories, and broader pressure testing remain open.

## OM-6 supervisor retry after sink-directory recovery — verified

- Contract: a local JSONL sink I/O failure does not change the authentication/request result or remove the durable AUDIT event; the supervisor retries pending delivery after the configured sink becomes writable.
- Regression: the real gateway/supervisor process test configures a sink under a directory that does not exist, sends an invalid authentication attempt, confirms the generic authentication failure and corresponding durable audit event while no sink file exists, then creates the directory and waits for the supervisor to deliver the event.
- Verification: the focused delegated process test passed; workspace build, formatting, warning-denied Clippy, maintained TinyCC SDK tests, sandbox fixture, and release-profile validation (10 engine binaries and one query fixture) passed. The delegated cgroup probe reported writable `cpu memory pids`; the full serial delegated workspace suite passed (223 engine unit, 151 commit integration, and all other workspace targets, zero failures or ignored tests). `git diff --check` passed.

## OM-6 systemd JSONL sink deployment — documented

- Contract: the optional sink must be usable under the maintained supervisor units without weakening their default filesystem confinement; alternate locations require matching systemd and Landlock write grants.
- Documentation: the user and system installation examples now configure a mode-0600 sink in their existing writable state directory. They explain that the parent must exist and that external paths need `ReadWritePaths=`, a supervisor `--writable-env PRDB_AUDIT_JSONL_PATH` grant, and traversal permission for missing ancestors.
- Verification: checked the documented default locations against both supervisor unit `ReadWritePaths` entries and `prdb-landlock-run --writable` arguments; `git diff --check` passed. This is deployment documentation, not a new runtime behavior.

## OM-7 authorized monitoring pressure — extended

- Contract: authorized own-invocation, administrator runtime, and administrator audit pages remain bounded and available while writes and cancellation proceed; repeated denied monitoring requests also cannot starve those operations.
- Regression: extended `denied_monitoring_pressure_does_not_starve_commit_or_cancellation` to issue at least 512 calls on each of five paths: owner invocation pages, authorized administrator runtime pages, authorized administrator audit pages, and denied runtime/audit pages. A durable invocation commit and managed cancellation complete during the pressure loop, and the cancellation outcome is confirmed ABORTED.
- Red/green: the first run correctly rejected the test's use of the built-in owner role ID as an administrator principal. The fixture now creates a real administrator principal and membership; the focused test passes.
- Verification: locked workspace build, rustfmt check, warning-denied all-target Clippy, TinyCC SDK tests and sandbox fixture, release-profile validation (10 engine binaries and one query fixture), delegated cgroup probe with writable `cpu memory pids`, and full serial delegated-cgroup workspace tests all passed with zero ignored tests. `git diff --check` passed.
- Remaining: broader pressure coverage for other monitoring APIs and OM-2 registry families; exact redb retained-byte/cache telemetry remains unavailable, and there is no asynchronous job API to monitor.
