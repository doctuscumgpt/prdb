# Client SDK Progress

## DP-SQL-4: DROP ROW/CELL POLICY SQL entry point — verified

- Added `AdminClient::execute_drop_policy_sql` and `admin-sql` dispatch for the bounded qualified DROP syntax. The client resolves the policy name through the scoped authenticated read and submits the stable ID to the existing typed drop operation.
- Parser, protocol, and delegated gateway/supervisor lifecycle tests pass. Locked workspace build, format check, warning-denied Clippy, TinyCC SDK tests/fixture, and the full delegated-cgroup workspace suite pass with zero ignored tests. `git diff --check` passes; Cargo.lock is unchanged.

## SDK-16: expose transaction identity and age — verified

- Contract: administrator runtime invocation pages include an opaque internal transaction ID and its age at the page observation time, alongside the existing state, write-set usage, and snapshot pin count.
- Implementation: admission assigns a nonzero random transaction ID unique among active entries. The bounded runtime page derives age from its trusted observation time, and the typed client and `runtime-list` JSON expose both fields.
- Regression: the TCP client fixture decodes two distinct IDs and observed ages; storage and gateway protocol assertions cover live projections; the delegated gateway/supervisor CLI lifecycle checks the fields.
- Verification: locked workspace build, formatting, warning-denied Clippy, TinyCC tests and fixture, release-profile validation, and full serial workspace suite passed in the delegated service with zero ignored tests. The cgroup probe passed and `git diff --check` passed.
- Remaining: additional transaction detail, wait/fence, job, memory/history, and counter registries remain under OM-2.

## SDK-15: expose invocation snapshot pin count — verified

- Contract: [client SDK](../../specs/client-sdk.md), [client protocol](../../specs/client-protocol.md), and [operational metadata](../../specs/operational-metadata.md). Administrator invocation records report the number of invocation-owned redb read snapshots retained (0 or 1).
- Implementation: the admission registry records a pin as soon as `begin_read` succeeds and clears it after the snapshot is released on abort, unstarted-admission discard, or commit finalization. Pin changes invalidate administrator page continuations. The typed client, protocol JSON, and `runtime-list` CLI expose the value.
- Regression: the TCP client fixture checks active and committing records; storage/protocol assertions and the delegated gateway/supervisor CLI test confirm live records report one pin. The regression first rejected the newly added response field, then passed after implementation.
- Verification: locked workspace build, formatting, warning-denied Clippy, TinyCC tests and sandbox fixture, release-profile validation, and the full serial workspace suite passed in a delegated `systemd --user` unit with zero ignored tests. The cgroup probe reported writable `cpu memory pids`; `git diff --check` passed.
- Remaining: transaction, wait, job, memory, and counter runtime registries remain open under OM-2.

## SDK-14: expose staged invocation usage — verified

- Contract: [client SDK](../../specs/client-sdk.md), [client protocol](../../specs/client-protocol.md), and [operational metadata](../../specs/operational-metadata.md). Administrator runtime records show the current staged row count and payload bytes without exposing row contents or index bookkeeping.
- Implementation: the invocation overlay reports staged rows and payload bytes to the admission registry; admin pages, typed decoding, and `prdb-client runtime-list` expose both values. Usage changes invalidate admin page cursors.
- Regression: the TCP client fixture decodes nonzero usage; storage tests verify stage, replacement, delete-before-commit, and cursor invalidation; server/CLI integration asserts the live page output.
- Verification: the intended decoder failure was observed. Focused client, storage/protocol, and delegated process tests passed. Workspace build, formatting, warning-denied Clippy, TinyCC SDK tests/fixture, release-profile validation, and the full serial delegated-cgroup workspace suite passed with zero ignored tests. `git diff --check` passed.
- Remaining: transaction, wait, job, memory, and counter runtime registries remain open under OM-2.

## SDK-13: expose invocation workload class — verified

- Contract: [client SDK](../../specs/client-sdk.md), [client protocol](../../specs/client-protocol.md), and [operational metadata](../../specs/operational-metadata.md). Administrator runtime invocation records identify each invocation as `operational` or `analytical` without exposing query contents or runtime internals.
- Implementation: the admission registry's workload class is returned in the administrative page, strictly decoded into `RuntimeWorkloadClass`, and printed by `prdb-client runtime-list`.
- Regression: the TCP fixture decodes both workload classes; storage/protocol coverage checks the serialized operational class; the real gateway/supervisor lifecycle runs `runtime-list` while a query is active and checks the class in CLI JSON.
- Verification: the first regression run failed because the client rejected the newly specified field. Focused client and storage/protocol tests, the production-process test in a delegated unit, workspace build, formatting, warning-denied Clippy, TinyCC SDK tests/fixture, and release-profile validation passed. The full serial workspace suite passed in the delegated service with zero ignored tests. `git diff --check` passed.
- Remaining: transaction, wait, job, memory, and counter runtime registries remain open under OM-2.

## SDK-12: administrator runtime invocation CLI — verified

- Contract: [client SDK](../../specs/client-sdk.md) and [client protocol](../../specs/client-protocol.md). The authenticated administrator reads bounded invocation pages with observation time, best-effort consistency, and a process-local cursor; each page rechecks administrator authority.
- Implementation: added typed `AdminClient::list_runtime_invocations_page` decoding and `prdb-client runtime-list`, default limit 64, maximum 128, JSON output without query values, paths, or pointers.
- Regression: TCP client paging coverage and the real gateway/supervisor CLI lifecycle are added. Focused tests passed before final validation.
- Verification: the focused TCP paging test passed. `cargo build --workspace --locked`, `cargo fmt --all -- --check`, and warning-denied workspace Clippy passed. `make -f engine/sdk/Makefile test` and `sandbox-query-fixture` passed; `release-profile` built the release workspace and validated 9 engine binaries and 1 query fixture. The cgroup probe found writable delegated `cpu memory pids`; the full serial workspace suite, including process and sandbox integrations, passed with zero ignored tests. `git diff --check` passed.
- Remaining: other administrative runtime registries and repeated authority-race stress remain open under OM-2.

## SDK-11: dependency metadata CLI — verified

- Contract: [client SDK](../../specs/client-sdk.md) and [client protocol](../../specs/client-protocol.md). The CLI authenticates with a private credential file and prints one bounded dependency page without widening endpoint visibility.
- Implementation: `prdb-client dependency-list` accepts an object ID, optional 1–128 page limit, and opaque cursor, then prints only edge IDs/kinds and the next cursor as JSON.
- Regression: the real gateway/supervisor metadata lifecycle invokes the CLI for a foreign-key edge and verifies its kind appears in output; the API regression independently checks schema-owner hidden-target omission.
- Verification: focused CLI-backed production-process test passed. `cargo build --workspace --locked`, `cargo fmt --all -- --check`, warning-denied workspace Clippy, maintained TinyCC SDK tests and sandbox fixture, and the release-profile validator passed. The full serial workspace suite passed inside the delegated `systemd --user` service with sandbox/process tests enabled and zero ignored.
- Remaining: additional administrative metadata CLI sections and repeated authority-race stress remain open.

## SDK-10 / MD-6: visible administrative dependency descriptions — verified

- Contract: [client protocol](../../specs/client-protocol.md), [client SDK](../../specs/client-sdk.md), and [metadata discovery](../../specs/metadata-discovery.md). Dependency reads return bounded direct edges only when both endpoints are currently visible; hidden endpoint edges and their counts are omitted.
- Implementation: `metadata.dependencies.list` uses the bidirectional dependency index, validates edge keys, resolves endpoint visibility through typed table ownership where needed, sorts and caps results at 4,096 records and 1 MiB, and uses the current-authority metadata cursor flow. `AdminClient::list_dependencies_page` validates kinds, IDs, ordering, and response shape.
- Regression: fake-TCP coverage checks the exact request and typed response. The real gateway/supervisor metadata lifecycle confirms that an administrator sees a foreign-key target edge while a source schema owner receives an empty page when the target is hidden.
- Verification: focused client and delegated production-process tests passed. `cargo build --workspace --locked`, `cargo fmt --all -- --check`, warning-denied workspace Clippy, maintained TinyCC SDK tests and sandbox fixture, and the release-profile validator passed. The full serial workspace suite passed inside the delegated `systemd --user` service with sandbox/process tests enabled and zero ignored.
- Remaining: metadata SQL binding/execution and continuations across process restart remain open under MD-4/MD-6.

## SDK-9: administrative policy summaries — verified

- Contract: [client protocol](../../specs/client-protocol.md), [client SDK](../../specs/client-sdk.md), and [metadata discovery](../../specs/metadata-discovery.md). Policy expression IDs and bodies remain private; schema owners receive no recipient IDs when principal endpoints may be hidden.
- Implementation: `metadata.policies.list` validates the current administrative schema authority, table policy index, policy records, expression bindings, and target records in one request snapshot. Database administrators receive recipient IDs; schema owners receive `null`. The client validates the typed policy shape and page order.
- Regression: fake-client exact request/typed response coverage and a production gateway/supervisor process test verify administrator recipient visibility, schema-owner redaction, and hidden-table empty results.
- Verification: focused fake-client and production gateway/supervisor process tests passed. The full serial workspace suite passed inside the delegated `systemd --user` service with sandbox/process tests enabled and zero ignored. Workspace build, rustfmt check, warning-denied Clippy, and diff checks passed.
- Remaining: privileged dependency descriptions remain open under MD-6.

## SDK-8: administrative direct grant description — verified

- Contract: [client protocol](../../specs/client-protocol.md), [client SDK](../../specs/client-sdk.md), and [metadata discovery](../../specs/metadata-discovery.md). Direct table and column grants are visible only to a current schema owner or database administrator; output retains grantee/grantor IDs and does not expand roles.
- Implementation: `metadata.grants.list` uses the snapshot's object-grant index, validates direct grant records and table/column targets, and bounds output to 4,096 records and 1 MiB. `AdminClient::list_grants_page` validates grant fields and stable ordering.
- Regression: fake-client exact-request/typed-response coverage and a production gateway/supervisor process test verify direct column grants, grantor provenance, and empty results for a non-admin hidden table.
- Verification: the focused fake-client test and production gateway/supervisor process test passed. The full serial workspace suite passed inside the delegated `systemd --user` service with sandbox/process tests enabled and zero ignored. Workspace build, rustfmt check, warning-denied Clippy, and diff checks passed.
- Remaining: privileged policy and dependency descriptions remain open under MD-6.

## SDK-7: administrative index description — verified

- Contract: [client protocol](../../specs/client-protocol.md), [client SDK](../../specs/client-sdk.md), and [metadata discovery](../../specs/metadata-discovery.md). Only logical index IDs, ordered column IDs, and uniqueness are exposed to a current schema owner or database administrator.
- Implementation: `metadata.indexes.list` captures a bounded request-snapshot result after revalidating the bearer token and administrative schema authority. Session cursors compare the currently visible index IDs before serving the next retained page. `AdminClient::list_indexes_page` validates IDs, key order, uniqueness shape, and page cursor syntax.
- Implementation: `metadata.constraints.list` returns typed logical constraint IDs, kinds, and source columns to a current schema owner or database administrator. It omits CHECK bodies and redacts foreign-key target IDs unless the caller also has administrative visibility over the referenced schema. `AdminClient::list_constraints_page` validates ordered IDs and kind-specific response shapes.
- Verification: fake-client protocol coverage accepts redacted FK targets; the production gateway/supervisor integration test verifies visible target metadata for the database administrator and redaction for a schema owner, alongside the existing metadata cursor lifecycle.
- Regression: fake protocol coverage checks the typed response and exact request; the real gateway/supervisor process test verifies database-admin and schema-owner visibility, empty results for a non-admin hidden table, and cursor invalidation after an index is dropped between pages.
- Verification: focused client and process tests passed. The full serial workspace suite passed inside the delegated `systemd --user` service with sandbox and process tests enabled and zero ignored. Workspace build, formatting, warning-denied Clippy, and diff checks passed.
- Remaining: other privileged catalog sections remain open under MD-6.

## SDK-6: administrative metadata page API — verified

- Contract: [client SDK](../../specs/client-sdk.md) and [metadata discovery](../../specs/metadata-discovery.md). Administrative visibility remains a read-only control-channel capability and does not grant query access.
- Implementation: `AdminClient` now exposes typed, bounded page methods for schemas, relations, and declared columns. These use the existing session-bound metadata cursors and validate IDs, ordering, scalar types, and response shape.
- Regression: the client protocol test follows a schema page cursor through `AdminClient` and checks typed IDs/names and terminal cursor handling. The production process test also pages as a schema owner, transfers ownership through a separate administrator session, and verifies the owner's retained cursor is rejected.
- Verification: focused `client_admin` page test and the schema-owner gateway/supervisor process test passed. The full serial workspace suite passed in a delegated `systemd --user` service with sandbox and process tests enabled and zero ignored. Workspace build, rustfmt check, warning-denied Clippy, and diff checks passed.
- Remaining: add privileged catalog sections described by `admin_describe_object`.

## SDK-5 / CP-1: schema, table, key, and object-grant DDL through real processes — verified

- Contract: [client SDK](../../specs/client-sdk.md), [client protocol](../../specs/client-protocol.md), and [schema DDL](../../specs/ddl-schema.md). Authenticated AdminClient DDL must cross the TCP gateway and supervisor service channel, return validated committed IDs, publish catalog metadata, and make grant/revocation changes visible to an authenticated member.
- Regression: `authenticated_metadata_and_admin_lifecycle_cross_gateway_and_supervisor_processes` creates a schema, table, and unique constraint through AdminClient, then grants schema USAGE plus table and column SELECT to a member. The member's authenticated MetadataClient discovers the schema, table, and column; after all three grants are revoked, the same client can no longer discover the schema. The test checks commit-sequence ordering, cleans up the table/schema through ADMIN, and runs alongside identity/token, SQL metadata, and role lifecycle process checks.
- Verification: the focused real-process integration passed inside a delegated `systemd --user` service with `cpu`, `memory`, and `pids` enabled. It also ran the confined restricted-metadata SQL artifact and confirmed the durable COMMITTED result. Workspace build, formatting, warning-denied Clippy, and the full serial delegated-cgroup workspace suite passed; all sandbox and process cases ran with zero ignored.
- Remaining: broaden multi-process ADMIN DDL coverage to foreign keys, CHECK constraints, policies, and index lifecycle; add the same operation families to client CLI acceptance where needed.

## SQLSO-6: client-supplied SQL library uploads — implemented

- Contract: [separate SQL library plan](SEPARATE_SQL_LIBRARY.md), [client protocol](../../specs/client-protocol.md), and [client SDK](../../specs/client-sdk.md). The server's configured SDK release remains the ABI/SONAME contract; client library bytes are untrusted and optional for backwards compatibility.
- Implementation: the packager accepts a client-selected shared library without a server release metadata file and packages `sql.so` with an independent digest sidecar. PREPARE supports paired optional size/digest fields; kind 12 streams the library under the independent 128 MiB limit and it counts against staging quotas. SEAL verifies length, digest, versioned SONAME, runtime dependencies, and absent RPATH/RUNPATH without loading the bytes. START bypasses registry resolution for uploaded bytes and the executor stages them read/execute-only next to query.so. Older clients without the fields retain the registry path.
- Regression: protocol decoding covers uploaded and legacy PREPARE forms plus rejection of an explicitly empty library; package/client tests check sidecars and independent upload offsets. A confined cgroup executor acceptance supplies library bytes without configuring a registry, loads the library after confinement, executes its SQL callback, and records the durable expected abort. The existing registry-backed acceptance remains.
- Verification: the SQL-library upload integration passed inside a delegated `systemd --user` cgroup with enabled `cpu memory pids` and no configured SDK registry. The full serial workspace suite then passed in the same delegated setup with zero ignored tests, including 145 commit tests, the client-upload and registry sandbox paths, and both process tests. `cargo fmt --all -- --check`, warning-denied workspace Clippy, debug and release workspace builds, `make -f engine/sdk/Makefile test`, and `make -f engine/sdk/Makefile sandbox-query-fixture` passed. The sandbox probe reported a writable cgroup-v2 parent with `cpu memory pids` available and enabled.
- Compatibility: protocol 1.0 PREPARE fields remain optional for older clients; new packager submissions always include the client library. The supported uploaded-library profile is Linux x86_64 with the service-configured SDK release/SONAME and runtime allowlist. Uploading a different SONAME, ABI contract, or target requires a server compatibility-profile change.

## SQLSO-5: shared-release packaging and migration cutoff — implemented

- Contract: [separate SQL library plan](SEPARATE_SQL_LIBRARY.md), [client SDK](../../specs/client-sdk.md), [toolchain](../../specs/toolchain.md), and [systemd deployment guide](../../contrib/systemd/README.md). The validated deployment profile is Linux x86_64, `x86_64-v1`, NixOS glibc 2.42.
- Implementation: release-profile packaging installs `libprdb_sql.so.1`, canonical compatibility metadata, checksum sidecar, headers, and a shared-linked query. The static archive and static fixtures are removed from the release tree. The validator checks release identity/digest, ELF SONAME/dependencies/stack flags, query SONAME dependency and exact `$ORIGIN` RUNPATH, plus the packaged supervisor. Deployment docs configure installed release directories, the bounded writable cache, dependency allowlists, and Landlock paths. Static packages now fail closed unless `--allow-legacy-static-sdk-release 1` is explicitly set; SDK 2 is the documented cutoff and removes that temporary option.
- Regression: compatibility tests cover default static rejection and explicit release-1 acceptance. The supervisor process test starts with the SDK release directory from the install tree. Registry tests cover cache deduplication, concurrency, corruption, and pin-aware eviction; the shared sandbox test verifies active pin lifetime.
- Verification: locked release build and `make -f engine/sdk/Makefile release-profile-package` passed; package SHA-256 verified and tar listing confirmed the shared library/metadata/query are present and `libprdb_sdk.a` is absent. The full delegated `systemd --user` workspace suite passed serially with zero ignored tests, including 144 commit tests, 74 transaction tests, both shared-SDK sandbox tests, metadata processes, and supervisor startup. `make -f engine/sdk/Makefile test sdk-shared-tests`, locked workspace build, warning-denied Clippy, rustfmt, and `git diff --check` passed. The release-profile validator and packaged supervisor `--help` check passed.
- Scope: the release profile remains Linux x86_64 on the pinned NixOS glibc 2.42 toolchain; other targets/runtime profiles are not claimed. Atomic cache publication requires a filesystem supporting `renameat2(RENAME_NOREPLACE)`; the delegated acceptance used `/tmp` because the workspace NFS mount returns `EINVAL`.

## SQLSO-4: private SDK staging and confined loading — implemented

- Contract: [separate SQL library plan](SEPARATE_SQL_LIBRARY.md) and [sandbox acceptance instructions](../sql-library/SANDBOX_TEST.md). The supervisor and storage process still inspect SDK/query bytes only; execution remains in the post-confinement executor.
- Implementation: executor staging copies the pinned SDK library into the invocation's private artifact directory under its SONAME, read/execute-only, beside query.so. The private-directory owner retains the RAII cache pin through worker exit and drops both staged resources and pin on cleanup. The maintained fixture target now builds a shared-linked query, a test SDK release with a hostile constructor, and a missing-runtime-dependency query.
- Regression: the enabled delegated-cgroup acceptance resolves PMF1, loads the separate SQL library, executes the SDK SQL callback, verifies query and SDK constructors cannot write outside scratch, checks the cache cannot evict while the executor is active, and confirms eviction after exit. A second confined acceptance proves missing dependency load failure produces a durable ABORTED outcome before constructors or query entry execute. The packager validator rejects a query whose RUNPATH is changed from exactly `$ORIGIN` to `$ORIGIN:/tmp`.
- Verification: shared SDK and missing-dependency acceptances each passed inside delegated `systemd --user` services with enabled `cpu memory pids`; `make -f engine/sdk/Makefile sandbox-query-fixture`, all three client-package tests, warning-denied workspace Clippy, formatting, and `git diff --check` passed. The workspace NFS cache path returned `AtomicPublishUnsupported`; rerunning with the test cache on `/tmp` passed. Full serial cgroup workspace acceptance will be repeated after release/migration work.
- Remaining: release packaging, supervisor deployment configuration, and an explicit static-to-shared compatibility cutoff.

## SQLSO-3: supervisor SDK registry and START resolution — implemented

- Contract: [separate SQL library plan](SEPARATE_SQL_LIBRARY.md), [client SDK](../../specs/client-sdk.md), and [SQL library](../../specs/sql-library.md). Supervisor startup accepts explicit installed release directories and a bounded cache location; query uploads cannot supply or install SDK libraries.
- Implementation: the registry validates canonical release metadata, compatibility fields, SONAME, actual dependencies, and SHA-256 before atomically publishing read-only cache entries keyed by digest and compatibility identity. It resolves the PMF release/ABI and profile, rechecks cached bytes for each resolution, and returns RAII pins. The supervisor binary adds repeatable `--sdk-release-dir`, `--sdk-cache-dir`, and bounded `--sdk-cache-max-bytes` configuration. START resolves shared SQL dependencies before the storage-admission callback and fails closed if resolution fails. Eviction removes only unpinned entries.
- Regression: registry tests cover release resolution, absent and mismatched profiles/releases, release and cached-byte corruption, cache limits, pin-aware eviction, concurrent resolution, and concurrent atomic publication. A START regression confirms resolution failure skips the admission callback and discards the reservation. Supervisor help coverage includes the registry options.
- Verification: focused registry tests (4), START failure test, supervisor help test, locked workspace build, warning-denied Clippy, rustfmt check, and `git diff --check` passed in the locked Nix shell. No cgroup-backed integration test was ignored or used as evidence for this registry increment; the most recent full delegated-cgroup suite evidence remains under SQLSO-2 and will be refreshed for the final acceptance run.
- Remaining: stage the selected pinned library beside query.so and prove SQL execution through the confined executor; finalize release configuration and migration policy.

## SQLSO-2: package queries against the shared SDK — implemented

- Contract: [separate SQL library plan](SEPARATE_SQL_LIBRARY.md) and [client SDK spec](../../specs/client-sdk.md). The package path now consumes a release library plus its compatibility metadata; it never accepts the transition archive.
- Implementation: `prdb-packager` compiles source to a bounded TinyCC PIC object, then invokes a bounded fixed shared-link command with the selected SDK `.so` and `-z defs`. Nix linker search paths are stripped from the resulting loader path with `patchelf`; the packager then reads the ELF as data and requires exactly `$ORIGIN`, no RPATH, the expected versioned SQL SONAME, and valid ABI exports. It checks canonical release metadata against the library's SHA-256, SONAME, target, runtime profile, CPU baseline, and ABI before compiling. The temporary object is removed before atomic publication.
- Regression: the aborting-constructor package fixture succeeds without running its constructor and verifies its final `$ORIGIN` RUNPATH and `libprdb_sql.so.1` dependency. The maintained optimizer fixture also packages successfully against the shared release. Existing package sidecar, PMF, PSC1, digest, and no-overwrite assertions remain active.
- Verification: `make -f engine/sdk/Makefile test sdk-shared-tests`, `cargo build --workspace --locked`, formatting, warning-denied workspace Clippy, and the serial delegated-cgroup workspace suite passed in the locked Nix shell. The cgroup probe reported writable `cpu memory pids` controllers. The full suite included the enabled sandbox and process tests and reported zero ignored tests; the commit target passed 142 tests and the transaction target passed 74. Package tests verify the SDK dependency, undefined optimizer import, exact `$ORIGIN` RUNPATH, no RPATH, no constructor execution, and rejection of wrong SONAME, ABI, and release metadata.
- Remaining: supervisor release resolution, cache pinning, worker staging, and release-profile migration remain future increments.

## SQLSO-1: build a versioned SQL SDK shared release — implemented

- Contract: [separate SQL library plan](SEPARATE_SQL_LIBRARY.md), [SQL library spec](../../specs/sql-library.md), and [toolchain spec](../../specs/toolchain.md). Keep the static archive for transition tests while producing a separately loadable SQL SDK release.
- Implementation: `make -f engine/sdk/Makefile sdk-shared-release` builds dedicated TinyCC `-fPIC` objects, links `libprdb_sql.so.<release>` with the GNU linker, a versioned SONAME, defined symbols, and a non-executable stack. The metadata writer inspects ELF type, machine, SONAME, dependencies, exports, relocation classes, and stack permissions, then atomically writes compatibility JSON and a SHA-256 sidecar. Release/profile/ABI inputs are explicit Make variables.
- Regression: `sdk-shared-tests` adds maintained value, SQL v20 parser, and optimizer UPDATE fixtures linked against the shared library with an `$ORIGIN` runpath. Metadata tests verify compatibility fields and digest and reject a mismatched SONAME and noncanonical ABI lists. Existing static SDK tests remain available during transition.
- Verification: `make -f engine/sdk/Makefile sdk-shared-release` and `make -f engine/sdk/Makefile sdk-shared-tests` passed under the locked Nix shell. `readelf` confirmed SONAME `libprdb_sql.so.1`, `DT_NEEDED` only `libc.so.6`, exported SQL/manifest/scope/optimizer symbols, position-independent relocations, and RW `GNU_STACK`; the checksum sidecar verified.
- Remaining: supervisor resolution, worker staging, and release-profile migration remain future increments.

## SDK-3/SDK-5: authenticated metadata discovery CLI — implemented

- Contract: [Client SDK](../../specs/client-sdk.md). `prdb-client discover` lists only authenticated visible schemas, relations, and declared columns, with optional exact-name schema/relation filters. Relation filtering requires a schema. Output is tab-separated, uses lowercase IDs, preserves ordinal order, and includes the declared scalar type and nullability.
- Red/green evidence: a unit fixture checks every initial scalar type spelling and parameterized DECIMAL/VARCHAR metadata. The real gateway/supervisor process test runs the compiled CLI over the authenticated control path for `information_schema.schemata` and checks the schema, relation, and first ordered column records.
- Verification: focused `prdb-client` unit tests and `authenticated_metadata_and_admin_lifecycle_cross_gateway_and_supervisor_processes` passed. Locked workspace build, format check, warning-denied Clippy, serial workspace tests, and `make -f engine/sdk/Makefile test` also passed; the commit suite reported 129 passed and 3 cgroup-dependent tests ignored.
- Remaining: separate-process ADMIN families for foreign keys, CHECK constraints, policies, and index operations; SDK packaging/release gates; confined artifact execution remains governed by the Linux sandbox host requirements.

## SDK-3 dependency: ABI v4 PMF1 and query packaging — implemented

- Behavior: PMF1 encode/decode, ELF export inspection, TinyCC packaging, and the C PMF1 encoder accept ABI versions 1–5. Each ABI requires its exact descriptor OBJECT and entry FUNC exports. The packager still inspects ELF as data and never loads the query.
- Regression: Rust tests cover v4 and v5 manifest roundtrips, correct ELF symbols, mismatched entry rejection, and package builds using their respective exports. The maintained optimizer SQL fixture still checks v4 without loading it. The package fixture's aborting constructor remains unexecuted. The maintained C manifest fixture checks v4/v5 symbol derivation and version 6 rejection.
- Verification: the focused Rust manifest and packaging tests, full workspace build/format/Clippy/tests, and `make -f engine/sdk/Makefile test` passed. Three sandbox START tests remain ignored because this host's cgroup2 mount remains read-only even after the authorized remount attempt.
- Filesystem compatibility: publication uses `renameat2(RENAME_NOREPLACE)`. The repository's NFSv4.2 workspace mount returns `EINVAL`; the packager removes its staging directory and now reports `AtomicPublishUnsupported`. The same maintained fixture packages successfully to `/tmp` on ext4.
- Remaining: execute the ABI v4 optimizer callback in the confined acceptance harness on a conforming cgroup host.

## SDK-3 dependency: ABI v5 operation-selected optimizer callback — implemented

- Contract: [Client SDK ABI](../../specs/client-sdk.md) and [query worker ABI](../../specs/query-worker.md). ABI v5 retains the complete v4 host prefix and adds operation-selected optimizer submission; the trusted executor enforces the `OptimizerSubmission` allowlist.
- Implementation: generated C/Rust layouts and descriptor symbols, executor v5 preference/validation, ABI v5 PMF1 symbol derivation, and the C SDK callback record.
- Regression: Rust layout/descriptor tests cover v5 offsets and invalid size/version records; Rust/C manifest tests accept exact v5 symbols and reject version 6; executor tests prove `OptimizeDelete` succeeds and unrelated operations are denied. TinyCC independently compiles and calls the v5 callback with `OptimizeDelete`.
- Verification: full workspace build, formatting, warning-denied Clippy, and locked tests passed; TinyCC SDK test suite and ABI generator consistency passed. Cgroup-dependent sandbox acceptance remains host-gated.
- Remaining: submit a real DELETE request and execute its returned plan through the standard authorized executor channel.

## SDK-1 dependency: canonical Rust PMF1 encoder — implemented

- Behavior: `encode_manifest_v1` emits the canonical PMF1 representation only for supported ABI/symbol pairs and bounded fields with sorted, unique dependency names. It round-trips through the decoder to keep the public encoder aligned with admission constraints.
- Regression: the manifest fixture round-trips through the public encoder/decoder and rejects unsorted dependency lists. The real TinyCC TCP submission regression uses the encoder to prepare a valid SEAL request.
- Verification: the encoder unit test, full workspace build/format/Clippy/tests and maintained TinyCC SDK test suite passed.
- Remaining: the current Rust packager handles client uploads; a C-side transport helper could reuse the encoder but is not implemented.

## SDK-1 dependency: bounded C PMF1 encoder — implemented

- Behavior: `prdb_sdk_manifest_encode_v1` writes the canonical 28-byte PMF1 header and body into caller-owned storage, derives ABI symbols from the native ABI version, validates profile/compiler/dependency fields, and requires dependencies in strict bytewise order. It supports size queries and returns the required length without partial output when capacity is short.
- Red/green evidence: the maintained fixture first failed to link because the encoder symbol was missing. It now compares ABI-v3 output byte-for-byte with a hand-checked 124-byte PMF1 fixture and covers size query, insufficient capacity, invalid ABI, empty SDK release, unsorted dependencies, and invalid arguments.
- Verification: the focused fixture and full TinyCC SDK suite passed with the pinned Nix-shell TinyCC. The Rust manifest encoder test remains the independent decoder-roundtrip check.
- Remaining: the bounded Rust packager now inspects ELF and invokes the Rust encoder; the C encoder remains available to future C-side tooling.

## SDK-1 dependency: bounded C PSC1 scope encoder — implemented

- Behavior: `prdb_sdk_scope_encode_v1` emits sorted canonical PSC1 capability records for schema USAGE and table/column SELECT/INSERT/UPDATE/DELETE. It rejects zero IDs, invalid target/operation pairs, duplicate or unsorted records, and counts outside 1–200. Size queries report the required length and undersized buffers remain untouched.
- Red/green evidence: the maintained fixture first failed to link because the encoder was absent. It now matches a hand-checked PSC1 byte fixture, exercises invalid pairs/IDs/order/count, checks no-partial-output behavior, and accepts the maximum 200-column capability scope at 6,806 bytes.
- Verification: the focused TinyCC fixture and forced full SDK archive/test/fixture build passed. Rust workspace build, formatting, warning-denied Clippy, and all workspace tests also passed; the suite reports 109 commit tests passing and two sandbox START tests ignored for missing cgroup delegation.
- Remaining: client-side discovery/selection must supply current object IDs; scope encoding is only a requested ceiling and grants no authority.

## SDK-3 dependency: derive PMF1 inputs from a compiled ELF — implemented

- Behavior: `manifest_v1_from_elf` walks the artifact's ELF data without loading it, validates the selected ABI's descriptor OBJECT and entry FUNC exports, derives strict sorted unique `DT_NEEDED` names, combines them with caller-supplied profile fields, and validates the result through the canonical PMF1 encoder.
- Red/green evidence: the real TinyCC supervisor/gateway submission regression initially failed to compile because the manifest discovery API was absent; it now uses that API to build the PMF1 payload and successfully seals the same artifact. Unit coverage rejects unsupported ABI and malformed ELF.
- Verification: the focused TinyCC TCP regression and complete Rust workspace build, formatting, warning-denied Clippy, and tests passed. The full maintained TinyCC archive/test/fixture build also passed.
- Remaining: the separate bounded packaging dependency is tracked below; this ELF parser/encoder API remains reusable by the package helper and TCP admission path.

## SDK-3 dependency: bounded TinyCC client packager — implemented

- Behavior: `prdb-packager` compiles one C source against the selected shared SDK release in cleared-environment subprocesses with CPU, wall-time, address-space, output-file, core-dump, and descriptor limits. It validates PSC1, derives and encodes PMF1 from the final linked ELF, hashes the artifact, then atomically publishes `query.so`, `manifest.pmf1`, `requested_scope.psc1`, and `artifact.sha256` in a private new directory without loading the query.
- Limits: source 1 MiB, SDK archive 128 MiB, PSC1 8 KiB, compiled artifact 64 MiB, diagnostic capture 8 KiB. Package files are mode 0600 and the directory is mode 0700. Existing output paths fail closed.
- Red/green evidence: the end-to-end test builds the maintained TinyCC SDK archive, invokes the actual packager binary, decodes PMF1, verifies exact PSC1 and SHA-256 output, proves a constructor that aborts is not run, and checks that an existing package path is not overwritten.
- Remaining: network scope selection through authorized metadata discovery; package generation alone does not grant authority.

## SDK-3 dependency: authenticated upload and terminal client — implemented

- Behavior: `SubmissionClient` negotiates query-mode protocol v1.0, authenticates with the token ID/secret, validates the private package, PREPAREs it, uploads artifact and parameter chunks in order, verifies every acknowledged offset, and SEALs it. It can then START once, persist a no-overwrite invocation receipt before transmission, stream provisional data, parse the matching supervisor terminal event, and query STATUS on a new authenticated connection after an uncertain START reply.
- Security: caller token-secret memory and temporary AUTH frames are zeroized. Package inputs must be private regular files; PMF1, PSC1, sidecar digest, and size bounds are checked before the network request. START is never automatically replayed.
- Regression: a mock TCP peer verifies handshake/auth, PMF1/PSC1 and SHA-256 preparation fields, a compiled query upload, 300 KB parameters split across chunks, exact offsets, SEAL, receipt-before-START ordering, coalesced START/data/terminal frames, committed status, and zeroized token-secret input. The existing real-binary TCP test remains the gateway/supervisor admission evidence.
- Remaining: broaden live client coverage and finish SDK packaging/release gates; successful START acceptance requires the delegated-cgroup sandbox host described in the Linux sandbox plan.

## SDK-3 dependency: build PSC1 from discovery records — implemented

- Behavior: `RequestedScopeBuilder` accepts authorized schema/relation/column metadata records, adds schema USAGE for selected data objects, encodes table- or column-level operations in canonical PSC1 byte order, deduplicates repeated selections, and enforces the 200-record limit without partial updates.
- Red/green evidence: tests compare a mixed schema/table/column selection byte-for-byte, decode the result independently, reject invalid operation/zero-ID/empty selections, and prove a selection that would exceed the record limit leaves the previous builder state unchanged.
- Remaining: the builder accepts only records supplied by the authenticated `MetadataClient`; the new `discover` CLI exposes those records, and neither component grants authority.

## SDK-1 initial borrowed value view — implemented

- Contract: [`prdb_sdk_value_v1`](../../engine/include/prdb_sdk_value_v1.h) exposes a sized/versioned view with VALUE, NULL, and REDACTED states. Value type codes and canonical bytes match the executor batch protocol. The view borrows bytes from an immutable source batch; it never allocates or frees them.
- Validation: [`value_v1.c`](../../engine/sdk/value_v1.c) checks layout version/size, state/type, the 1 MiB bound, fixed-width types, Boolean values, null/redacted empty representations, and UTF-8 strings. Invalid pointers remain a native caller error and cannot be made safe by this validator.
- Regression: [`sdk_value_v1.c`](../../engine/tests/fixtures/sdk_value_v1.c) covers all states, pointer borrowing, integer widths, malformed null/redacted records, invalid Boolean, invalid UTF-8, unsupported version, and undersized records.
- Red/green evidence: before implementation, TinyCC compilation failed because `prdb_sdk_value_v1.h` did not exist. After implementation, `make -f engine/sdk/Makefile test` passed using TinyCC 0.9.27-unstable-2025-01-06, including archive creation with `tcc -ar` and execution of the maintained test.
- Contract: [`prdb_sdk_batch_v1`](../../engine/include/prdb_sdk_batch_v1.h) opens the executor next-batch wire payload in place, validates it in full, and provides zero-copy row/cell iterators. The caller retains the unchanged response bytes for the lifetime of all views; it owns and releases that buffer.
- Regression: [`sdk_batch_v1.c`](../../engine/tests/fixtures/sdk_batch_v1.c) covers a hand-checked row with VALUE/NULL/REDACTED cells, row/cell iteration, borrowed data, continuation cursor, truncation, unsupported wire version, invalid handle kind and oversized cell count. The test was first compiled before implementation and failed because `prdb_sdk_batch_v1.h` did not exist; it now passes against the TinyCC archive.
- Typed accessors: value state, Boolean, signed 16/32/64-bit scalars, Date, TimestampTz and borrowed raw payload accessors validate state/type and preserve NULL versus REDACTED. Signed conversion handles negative values and minimum representable boundaries without out-of-range signed casts.
- Regression: the expanded value fixture initially failed to compile because `prdb_sdk_value_state_v1` and the typed getters were absent; after implementation it passes against the TinyCC archive, including negative/minimum signed values, no-value statuses, type mismatch, and borrowed text bytes.
- Query host ABI v3: [`prdb_query_abi_v3.h`](../../engine/include/prdb_query_abi_v3.h) adds a sticky SQL statement-failure callback while preserving v1/v2 entry points. The trusted executor latches the callback and forces a nonzero worker exit after user entry returns, so a caught SQL API error still reaches supervisor abort handling. Rust/C layout and descriptor checks and the SDK SELECT fixture cover the new boundary.
- Remaining: add generated bindings before claiming SDK-1 complete and complete the C SQL/runtime result format. Client cancellation and authenticated metadata discovery are implemented in later increments.

## SDK-1: owned batch-result lifetime — implemented

- Contract: [`prdb_sdk_owned_batch_v1`](../../engine/include/prdb_sdk_owned_batch_v1.h) optionally copies a bounded executor batch into SDK-owned memory. The copy is validated before publication; opened batch/row/cell views remain borrowed from it and become invalid on release.
- Red/green evidence: the maintained batch fixture first failed to compile because the owned-batch header was absent. It now checks copy isolation, full validation, borrowed views into the owned copy, active-wrapper rejection, malformed/unsupported payload rejection, release invalidation, idempotent release, and wrapper reuse.
- Implementation: `owned_batch_v1.c` owns the copy and exposes copy/open/release functions; release clears the wrapper and permits reuse. The API reuses the existing batch decoder so owned and directly borrowed payloads receive identical validation.
- Verification: the maintained TinyCC SDK suite passed with `make -f engine/sdk/Makefile BUILD_DIR=/tmp/prdb-sdk-owned-batch test` under TinyCC 0.9.27-unstable-2025-01-06. Rust 1.95.0 workspace build, format, warning-denied Clippy, and full tests also passed; the commit integration target reported 124 passed and 3 cgroup-dependent ignored.
- Remaining: generated bindings and the C SQL/runtime result format remain open. Client cancellation and authenticated metadata discovery are implemented in later increments.

## SDK-5 dependency: typed ordinary-index AdminClient — implemented

- Contract: [client SDK](../../specs/client-sdk.md) keeps administrative credentials on an authenticated control connection, provides typed allowlisted operations, and never retries a START whose result is uncertain.
- Implementation: `AdminClient` uses control-mode authentication and exposes index create, rebuild, drop, and status methods. It validates normalized names, nonzero IDs, and ordered distinct column lists locally; START errors that may conceal a committed result return the server-issued operation ID for reconciliation.
- Regression: `client_admin.rs` verifies control HELLO/AUTH, exact create/rebuild/drop PREPARE and START routing, typed response decoding, operation status lookup, token-secret zeroization, and lost START reply behavior. The validation unit test rejects zero IDs, empty/duplicate columns, and malformed names.
- Verification: focused mock-TCP AdminClient tests passed. `cargo build --workspace --locked`, `cargo fmt --all -- --check`, warning-denied workspace Clippy, and `cargo test --workspace --locked` passed in the configured Nix shell; the commit integration target reported 124 passed and 3 cgroup-dependent ignored. An initial full-suite run had one non-reproducible `InvalidAudit` failure in the CHECK lifecycle test; that test passed alone and in the complete rerun.
- Remaining: typed client helpers for other administrative operations, CLI/credential-file lifecycle, and live ADMIN operations through the real multi-process gateway path remain open.

## SDK-5 dependency: typed user and role AdminClient operations — implemented

- Contract: [client protocol](../../specs/client-protocol.md) defines the strict PREPARE argument and START result shapes for users and roles; [client SDK](../../specs/client-sdk.md) keeps these credentials on the authenticated control connection and never retries uncertain STARTs.
- Implementation: `AdminClient` exposes user/role create, rename, and drop, user login enable/disable, and role membership grant/revoke. Creation returns the assigned identity ID and commit sequence; user creation does not issue a credential; NOLOGIN is enforced during token issuance. IDs and canonical lowercase identity names are validated locally. Shared PREPARE/START decoding verifies the operation ID, committed outcome, exact result shape, and canonical commit sequence.
- Regression: mock-TCP coverage checks all eight operation request schemas, PREPARE/START routing, identity result decoding, and existing lost-START reconciliation. Local validation tests cover identity names and nonzero IDs.
- Verification: focused `client_admin` integration tests passed. `cargo build --workspace --locked`, `cargo fmt --all -- --check`, warning-denied workspace Clippy, and the repeated `cargo test --workspace --locked` run passed in the Nix shell; the commit integration target reported 124 passed and 3 cgroup-dependent ignored. The first full-suite run hit the known intermittent `InvalidAudit` failure in the CHECK lifecycle test; that test passed alone and the complete rerun passed.
- Remaining: typed helpers for schema rename, tables, constraints and foreign keys; CLI/credential-file lifecycle; and live ADMIN operations through the real multi-process gateway path remain open.

## SDK-5 dependency: typed schema create and drop AdminClient operations — implemented

- Contract: [client protocol](../../specs/client-protocol.md) defines owner-bearing schema creation and ID-based schema removal, with generated stable schema IDs returned by START.
- Implementation: `AdminClient::create_schema` validates the normalized name and owner ID and returns the generated schema ID; `drop_schema` validates the target and checks the echoed schema ID in the committed result.
- Regression: mock-TCP coverage verifies exact create/drop arguments, PREPARE/START flow, generated ID, and committed response decoding.
- Verification: focused `client_admin` tests passed. `cargo build --workspace --locked`, `cargo fmt --all -- --check`, warning-denied workspace Clippy, and `cargo test --workspace --locked` passed in the Nix shell; the commit integration target reported 124 passed and 3 cgroup-dependent ignored.
- Remaining: typed constraint and foreign-key helpers; CLI/credential-file lifecycle; and live ADMIN operations through the real multi-process gateway path remain open.

## SDK-5 dependency: typed table create AdminClient operation — implemented

- Contract: [client protocol](../../specs/client-protocol.md) requires ordered typed column definitions and returns the generated table ID and ordered column IDs in START.
- Implementation: `AdminClient::create_table` accepts typed column definitions with SQL scalar types and literal, NULL, or current-principal defaults. It validates names, duplicate columns, UUID identity rules, default type compatibility, decimal parameters, and varchar bounds, then encodes canonical protocol JSON. Its result decoder validates the exact operation response and preserves column ID order.
- Regression: mock-TCP coverage checks an exact table-create request with generated UUID, escaped text default, fixed-scale decimal default, and current-principal UUID default; it checks the ordered returned IDs. Unit coverage rejects invalid column/default combinations.
- Verification: focused `client_admin` and column-encoder tests passed. `cargo build --workspace --locked`, `cargo fmt --all -- --check`, warning-denied workspace Clippy, and `cargo test --workspace --locked` passed in the Nix shell; the commit integration target reported 124 passed and 3 cgroup-dependent ignored.
- Remaining: typed constraint and foreign-key helpers; CLI/credential-file lifecycle; and live ADMIN operations through the real multi-process gateway path remain open.

## SDK-5 dependency: typed table alteration AdminClient operations — implemented

- Contract: [client protocol](../../specs/client-protocol.md) defines separate owner, column add/rename/drop, nullability, and default operations, each with exact PREPARE arguments and durable START results.
- Implementation: `AdminClient` exposes table-owner transfer, column add/rename/drop, nullability changes, and column-default set/drop. Column addition reuses typed column/default encoding while enforcing backfill restrictions. The generalized result validator checks echoed IDs, names, booleans, positive schema versions, operation IDs, and commit sequences.
- Regression: mock-TCP coverage verifies exact wire arguments and START schemas for add/rename/drop column, owner transfer, nullability, and default set/drop. Existing table-create validation exercises the shared column/default encoder.
- Verification: focused `client_admin` tests passed. `cargo build --workspace --locked`, `cargo fmt --all -- --check`, warning-denied workspace Clippy, and `cargo test --workspace --locked` passed in the Nix shell; the commit integration target reported 124 passed and 3 cgroup-dependent ignored.
- Remaining: typed constraint and foreign-key helpers; CLI/credential-file lifecycle; and live ADMIN operations through the real multi-process gateway path remain open.

## SDK-5 dependency: typed table rename and drop AdminClient operations — implemented

- Contract: [client protocol](../../specs/client-protocol.md) defines stable table ID arguments and requires START to echo the table ID for both rename and drop.
- Implementation: `AdminClient::rename_table` validates the normalized new name; `drop_table` validates the nonzero table ID. Both use the shared PREPARE/START path and reject success responses with mismatched table IDs.
- Regression: mock-TCP coverage checks exact rename/drop argument shapes and typed committed-result validation.
- Verification: focused `client_admin` tests passed. `cargo build --workspace --locked`, `cargo fmt --all -- --check`, warning-denied workspace Clippy, and `cargo test --workspace --locked` passed in the Nix shell; the commit integration target reported 124 passed and 3 cgroup-dependent ignored.
- Remaining: typed constraint and foreign-key helpers; CLI/credential-file lifecycle; and live ADMIN operations through the real multi-process gateway path remain open.

## SDK-5 dependency: typed constraint and foreign-key AdminClient operations — implemented

- Contract: [client protocol](../../specs/client-protocol.md) defines ordered distinct key columns, EIR1 row-local CHECK expressions and dependencies, generated constraint/index/expression/FK IDs, and ID-echoing drop results.
- Implementation: `AdminClient` exposes unique and primary-key create/drop, CHECK create/drop, and foreign-key create/drop. Creation uses the canonical expression encoder and the shared exact PREPARE/START response validator; generated IDs and commit sequences are decoded into typed results. CHECK dependency lists may be empty but remain bounded and reject zero or repeated IDs.
- Regression: mock-TCP coverage verifies all eight wire schemas, canonical EIR1 bytes, generated IDs, committed sequence decoding, and empty CHECK dependencies.
- Verification: the focused `client_admin` regression passed. In the pinned Nix shell, `cargo build --workspace --locked`, `cargo fmt --all -- --check`, warning-denied workspace Clippy, and `cargo test --workspace --locked` passed; the commit integration target reported 124 passed and 3 cgroup-dependent ignored.
- Remaining: CLI/credential-file lifecycle and live ADMIN operations through the real multi-process gateway path remain open.

## SDK-5 dependency: private client credential-file lifecycle — implemented

- Contract: [client SDK](../../specs/client-sdk.md) keeps bearer credentials in a private client-only file, rejects unsafe file ownership/permissions, and does not overwrite an existing credential path.
- Implementation: `client_credentials` reads canonical `PDB-CREDENTIALS-1` token ID/secret files, rejects symlinks and non-owner-only modes, atomically creates a private file with no-overwrite publication, and removes owned files. Secret-bearing temporary byte buffers are zeroized.
- Regression: module tests cover round trip, mode 0600, no-overwrite, removal, symlink rejection, and broad-permission rejection. The initial run caught an `O_NOFOLLOW` symlink error mapping issue; the corrected path now returns the documented unsafe-file result.
- Verification: `cargo build --workspace --locked`, `cargo fmt --all -- --check`, warning-denied workspace Clippy, and `cargo test --workspace --locked` passed in the pinned Nix shell. The first full-suite attempt hit the known intermittent CHECK lifecycle `InvalidAudit`; the isolated CHECK test and full rerun passed. The commit integration target reported 124 passed and 3 cgroup-dependent ignored.
- Remaining: user-facing CLI commands and live ADMIN operations through the real multi-process gateway path remain open.

### Credential removal CLI — implemented

- Behavior: `prdb-client credential-remove --credentials FILE` removes a valid owner-only credential file without requiring a network connection. It reuses the credential module's ownership, regular-file, and mode-0600 checks and reports local file removal without exposing token material.
- Contract: [client SDK](../../specs/client-sdk.md) now distinguishes local credential-file deletion from server-side `token-revoke`.
- Regression: the maintained CLI unit test creates a private credential file, runs the removal command handler, and confirms the file is gone; the authenticated gateway/supervisor process test invokes the compiled `prdb-client credential-remove` command after token revocation and confirms the file is removed. Credential module tests separately cover symlink and unsafe-mode rejection.
- Verification: the focused `authenticated_metadata_and_admin_lifecycle_cross_gateway_and_supervisor_processes` test and rustfmt checks pass.
- Remaining: CLI credential lifecycle now supports issue, private save, read, and local removal. Admin CLI breadth and real multi-process ADMIN path coverage continue below.

## SDK-3 dependency: basic submit/status CLI — implemented

- Contract: [client SDK](../../specs/client-sdk.md) requires status reconciliation after uncertain START, a durable no-overwrite receipt before START, and provisional output until a committed terminal outcome.
- Implementation: `prdb-client submit` reads a private credential file, validates a package through `SubmissionClient`, uploads/seals, persists the requested receipt, starts once, streams provisional bytes, and exits successfully only after COMMITTED. `prdb-client status` opens a fresh authenticated connection and queries the supplied invocation ID. Parameters are bounded to the protocol's 1 MiB limit.
- Regression: unit coverage checks canonical nonzero invocation-ID parsing; `prdb-client --help` was exercised to inspect the user-facing commands.
- Verification: `cargo build --workspace --locked`, `cargo fmt --all -- --check`, warning-denied workspace Clippy, and `cargo test --workspace --locked` passed in the pinned Nix shell. The commit integration target reported 124 passed and 3 cgroup-dependent ignored. The focused binary test and `prdb-client --help` check also passed.
- Remaining: credential creation/rotation UX, authenticated metadata-based scope selection, admin CLI commands, and live ADMIN multi-process path tests remain open.

## SDK-3 dependency: authenticated metadata scope CLI — implemented

- Contract: [client SDK](../../specs/client-sdk.md) requires scope selection from authorized metadata records; PSC1 is a requested ceiling and never grants authority.
- Implementation: `prdb-client scope` resolves exact schema/relation/column names using `MetadataClient`, selects schema USAGE and requested table/column operations through `RequestedScopeBuilder`, and atomically publishes a private no-overwrite PSC1 file.
- Regression: CLI tests cover valid operation and column lists, duplicate/unsupported inputs, nonzero invocation parsing, and private no-overwrite PSC1 output. Existing metadata client and scope-builder suites cover protocol response validation and independent PSC1 decoding.
- Verification: focused `prdb-client` tests passed. `cargo build --workspace --locked`, `cargo fmt --all -- --check`, warning-denied workspace Clippy, and `cargo test --workspace --locked` passed in the pinned Nix shell; the commit integration target reported 124 passed and 3 cgroup-dependent ignored. The existing authenticated metadata process test passed, and updated CLI help was inspected.
- Remaining: credential creation/rotation UX, admin CLI commands, and live ADMIN multi-process path tests remain open.

## SDK-5 dependency: token issue/revoke CLI and credential rotation — implemented

- Contract: [client protocol](../../specs/client-protocol.md) returns token secrets exactly once; [identity DDL](../../specs/ddl-identity.md) requires the caller to preserve a usable administrator credential and never log bearer material.
- Implementation: `prdb-client token-issue` calls authenticated `AdminClient::issue_token`, computes a bounded absolute expiration from a positive TTL, and atomically writes the one-time result to a new private credential file without printing the secret. `token-revoke` exposes explicit revocation. If issuance commits but saving fails, the CLI reports the operation/token IDs and directs the operator to revoke before retrying. Rotation keeps the old credential until the new one is saved and verified.
- Regression: CLI tests cover zero/overflow TTL rejection; credential file tests cover the private no-overwrite publication path; existing AdminClient mock-TCP tests cover the one-time issue/revoke protocol and malformed secret handling.
- Verification: focused CLI/credential tests and the existing one-time `AdminClient` token issue/revoke mock-TCP regression passed. `cargo build --workspace --locked`, `cargo fmt --all -- --check`, warning-denied workspace Clippy, and `cargo test --workspace --locked` passed in the pinned Nix shell; the commit integration target reported 124 passed and 3 cgroup-dependent ignored. CLI help was inspected.
- Remaining: general admin DDL CLI commands and live ADMIN multi-process path tests remain open.

## SDK-5 dependency: identity and membership admin CLI — implemented

- Contract: [client protocol](../../specs/client-protocol.md) defines user/role lifecycle, login state, and membership PREPARE/START operations; storage revalidates authority and protects the last usable administrator credential.
- Implementation: `prdb-client` exposes user/role create, rename, and drop, user login enable/disable, and role membership grant/revoke. It uses typed `AdminClient` methods, validates IDs and boolean flags before sending, and reports generated IDs and durable operation metadata.
- Regression: CLI parser tests cover canonical login booleans; existing mock-TCP integration covers each identity/membership operation shape and server response validation.
- Verification: focused CLI tests passed. `cargo build --workspace --locked`, `cargo fmt --all -- --check`, warning-denied workspace Clippy, and `cargo test --workspace --locked` passed in the pinned Nix shell. The commit integration target reported 124 passed and 3 cgroup-dependent ignored. The initial full-suite attempt hit the intermittent CHECK lifecycle `InvalidAudit`; the isolated regression and full rerun passed. Final CLI help was inspected.
- Remaining: table/schema/constraint/policy admin CLI commands and live ADMIN multi-process path tests remain open.

## SDK-5 / CP-1: real multi-process ADMIN identity and token lifecycle — verified

- Contract: the control-mode AdminClient must cross the dedicated gateway and supervisor process boundary; storage must independently enforce authority, token persistence, and revocation.
- Regression: `authenticated_metadata_and_admin_lifecycle_cross_gateway_and_supervisor_processes` now creates/renames/drops a role through `AdminClient`, creates a user, issues a token, writes and reloads the private credential file, authenticates a fresh MetadataClient through the processes, revokes the token, then proves that token cannot authenticate again. The test also checks secret input buffers are zeroized.
- Verification: the focused real-process test passed. Full workspace build, format check, warning-denied Clippy, and tests passed; the commit integration target reported 124 passed and 3 cgroup-dependent ignored. One separate sandbox-dependent process test remains ignored.
- Remaining: extend multi-process ADMIN coverage to foreign keys, CHECK constraints, policies, and index lifecycle; add acceptance coverage for the related admin CLI commands.

## SDK-5 dependency: typed schema rename AdminClient operation — implemented

- Contract: [client protocol](../../specs/client-protocol.md) requires `schemas.rename` START to echo the stable schema ID and normalized new name.
- Implementation: `AdminClient::rename_schema` validates ID and name and accepts success only when both echoed values, the operation ID, committed outcome, and commit sequence match. The shared response validator now supports this exact echoed-name shape.
- Regression: schema mock-TCP coverage verifies create, rename, and drop requests and checks the rename response includes the expected schema ID and name.
- Verification: focused schema lifecycle mock-TCP test passed. `cargo build --workspace --locked`, `cargo fmt --all -- --check`, warning-denied workspace Clippy, and `cargo test --workspace --locked` passed in the Nix shell; the commit integration target reported 124 passed and 3 cgroup-dependent ignored.
- Remaining: typed table, constraint and foreign-key helpers; CLI/credential-file lifecycle; and live ADMIN operations through the real multi-process gateway path remain open.

## SDK-5 dependency: typed system privilege AdminClient operations — implemented

- Contract: [client protocol](../../specs/client-protocol.md) allowlists the closed database-scoped privilege set: `manage_users`, `manage_roles`, and `create_schema`.
- Implementation: `AdminClient` exposes grant/revoke methods taking a typed `AdminSystemPrivilege`; the shared administrative operation path enforces exact START response fields and uncertain-outcome handling.
- Regression: mock-TCP coverage verifies exact grant/revoke operation names and wire arguments for representative privilege values.
- Verification: focused `client_admin` tests passed. `cargo build --workspace --locked`, `cargo fmt --all -- --check`, warning-denied workspace Clippy, and `cargo test --workspace --locked` passed in the Nix shell; the commit integration target reported 124 passed and 3 cgroup-dependent ignored.
- Remaining: typed helpers for schemas, tables, constraints and foreign keys; CLI/credential-file lifecycle; and live ADMIN operations through the real multi-process gateway path remain open.

## SDK-5 dependency: typed token lifecycle AdminClient operations — implemented

- Contract: [client protocol](../../specs/client-protocol.md) returns issued bearer material exactly once and never through STATUS. A lost issue result requires reconciliation followed by explicit revoke/reissue.
- Implementation: `AdminClient::issue_token` and `revoke_token` use the authenticated control PREPARE/START path. Issue validates the user ID, UTF-8 label byte bound, and positive expiration; JSON-escapes labels; validates the committed response; and decodes only canonical fixed-width base64. `AdminTokenResult` omits secret material from Debug and zeroizes the secret on drop. The response frame, receive buffer, and partially buffered decoder payload are zeroized.
- Regression: mock-TCP coverage verifies exact issue/revoke schemas, escaped label content, one-time secret decoding, token ID and sequence, plus malformed base64 rejection.
- Verification: focused `client_admin` and secret-decoder tests passed. `cargo build --workspace --locked`, `cargo fmt --all -- --check`, warning-denied workspace Clippy, and the repeated `cargo test --workspace --locked` run passed in the Nix shell; the commit integration target reported 124 passed and 3 cgroup-dependent ignored. The initial full-suite run hit the known intermittent `InvalidAudit` CHECK lifecycle failure; the test passed alone and the complete rerun passed.
- Remaining: typed helpers for administrative privileges, schemas, tables, constraints and foreign keys; CLI/credential-file lifecycle; and live ADMIN operations through the real multi-process gateway path remain open.

## SDK-5: schema/table DDL CLI and process coverage — implemented

- Contract: the client SDK routes schema/table administration through authenticated `AdminClient` PREPARE/START operations; query packages never receive administrative credentials.
- Implementation: `prdb-client` exposes schema create/rename/drop and table create/rename/drop. Table columns use a semicolon-separated typed declaration syntax, validated locally before the existing typed table-create request.
- Regression: CLI tests check the parameterized type parser and invalid schema shapes. The real gateway/supervisor process test now creates a schema and table, renames and drops the table, then drops the schema.
- Verification: focused CLI tests and the focused multi-process ADMIN lifecycle passed. The final locked workspace build, format check, warning-denied Clippy, and full tests passed; the commit integration target had 124 passed and 3 cgroup-dependent ignored. CLI help was inspected after correcting its continuation formatting.
- Remaining: general DDL CLI commands for columns, constraints, indexes, privileges, and policies; additional multi-process coverage; and confined artifact submission remain open.

## SDK-5: column DDL CLI and process coverage — implemented

- Contract: column changes use the authenticated ADMIN control channel and storage's typed, fenced schema-evolution operations.
- Implementation: `prdb-client` exposes add, rename, and drop column commands. Add accepts one nullable column declaration; rename/drop use explicit table and column IDs.
- Regression: parser tests reject unsupported non-null, identity, and multi-column add forms. The real gateway/supervisor integration now exercises nullable add, rename, and drop.
- Verification: focused CLI and process tests passed; final workspace build, formatting, warning-denied Clippy, and all workspace tests passed. The commit integration target reported 124 passed and 3 cgroup-dependent ignored.
- Remaining: CLI support for nullability/default operations, indexes, constraints, privileges, policies, more real-process DDL, and confined artifact execution remains open.

## SDK-5: column nullability/default CLI and process coverage — implemented

- Contract: the client exposes typed nullability/default changes through the authenticated ADMIN protocol; storage validates current rows and schema before commit.
- Implementation: CLI commands set column nullability and set/drop defaults. Default literals are parsed using the declared scalar type, including SQL NULL, `current-principal-id`, decimal coefficients, lowercase BYTEA hex, UUIDs, dates, and timestamps.
- Regression: parser tests cover decimal scaling and invalid scale, BYTEA decoding, principal UUID defaults, and type mismatch. The real-process ADMIN test exercises nullability changes and setting/dropping a literal default.
- Verification: focused CLI and process tests passed; final workspace build, formatting, warning-denied Clippy, and full test suite passed after isolating and rerunning the known intermittent CHECK lifecycle test. Commit integration had 124 passed and 3 cgroup-dependent ignored.
- Remaining: index, constraint, privilege, and policy CLI commands; broader live-process DDL coverage; and confined artifact execution remain open.

## SDK-5: ordinary index CLI and process coverage — implemented

- Contract: index administration stays on the authenticated AdminClient channel and uses ordered, distinct nonzero column IDs.
- Implementation: CLI commands now expose ordinary index create, rebuild, and drop; the uniqueness flag defaults to false.
- Regression: command parsing checks canonical IDs and duplicate detection. The real gateway/supervisor process test invokes `prdb-client index-create`, `index-rebuild` with a changed uniqueness profile, and `index-drop` using a private credential file; the typed-client and ADMIN dispatcher regressions also cover rebuild.
- Verification: `cargo test --locked --test metadata_process authenticated_metadata_and_admin_lifecycle_cross_gateway_and_supervisor_processes -- --exact --nocapture` passed (1 test). `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets --locked -- -D warnings` passed in the configured Nix shell.
- Remaining: broaden process-level CLI coverage for other DDL families and complete remaining SDK packaging/release gates.

## SDK-5 / SM-5: repeat same-profile index rebuild — implemented

- Contract: the typed `indexes.rebuild` operation is useful even when the replacement profile equals the existing one; it must validate and republish memberships under the authorized table fence.
- Red evidence: repeating the same profile first returned a sanitized `unavailable` from the process path; the real-redb regression exposed `Commit(InvalidCatalog)`.
- Implementation: explicit unchanged index-definition writes now flow through affected-table fencing, current-authority checks, membership rebuild, durable audit/outcome, and write-epoch advance.
- Regression: a populated index is rebuilt twice with the same definition and checks its durable outcome/sequence. The real gateway/supervisor lifecycle also repeats an unchanged profile after changing uniqueness.
- Verification: focused redb and process tests passed; full workspace build, format, warning-denied Clippy, and tests passed; the commit integration target reported 124 passed and 3 cgroup-dependent ignored.

## SDK-5: unique and primary-key constraint CLI — implemented

- Contract: key constraints use ordered, distinct nonzero column IDs and typed AdminClient PREPARE/START operations; storage remains authoritative for schema, nullability, authority, dependencies, backfill, and uniqueness.
- Implementation: prdb-client exposes unique-create and primary-key-create with --table-id and --columns, and matching drop commands with --constraint-id. Creation reports generated constraint and backing-index IDs.
- Red evidence: the first real-process run after column addition failed with `Commit(InvalidCatalog)`; the generated backing index had conflated its fixed format version with the table's schema version.
- Regression: CLI parser coverage accepts ordered IDs and rejects empty, duplicate, and zero IDs. The real gateway/supervisor ADMIN lifecycle now invokes `unique-create`, `unique-drop`, `primary-key-create`, and `primary-key-drop` through `prdb-client`; the primary-key flow first makes the column non-null and restores nullability after dropping the key. Typed AdminClient coverage continues to exercise constraint dependencies and backfill validation.
- Verification: `cargo test --locked --test metadata_process authenticated_metadata_and_admin_lifecycle_cross_gateway_and_supervisor_processes -- --exact --nocapture` passed (1 test). `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets --locked -- -D warnings` passed in the configured Nix shell.
- Remaining: broaden process-level CLI coverage for other DDL families and complete remaining SDK packaging/release gates.

## SDK-5: system privilege grant/revoke CLI — implemented

- Contract: only the closed database-scoped privilege set (manage_users, manage_roles, create_schema) is accepted; the authenticated AdminClient and storage revalidate administrator authority.
- Implementation: prdb-client exposes system-privilege-grant and system-privilege-revoke with grantee ID and typed privilege value.
- Regression: the parser accepts all three values and rejects case changes and unsupported privilege names. The real gateway/supervisor ADMIN lifecycle exercises typed `AdminClient` grant/revoke and invokes `system-privilege-grant`/`system-privilege-revoke` through `prdb-client` for a role.
- Verification: `cargo test --locked --test metadata_process authenticated_metadata_and_admin_lifecycle_cross_gateway_and_supervisor_processes -- --exact --nocapture` passed (1 test). `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets --locked -- -D warnings` passed in the configured Nix shell.
- Remaining: broaden process-level CLI coverage for remaining DDL families and complete remaining SDK packaging/release gates.

## SDK-5: object grant/revoke CLI process path — verified

- Contract: object and column privileges use the authenticated ADMIN control channel with a closed privilege set and an explicit object or column target; storage rechecks the actor's current authority.
- Regression: the real gateway/supervisor ADMIN lifecycle invokes `object-grant` and `object-revoke` through `prdb-client` for a non-admin user and a column-scoped SELECT privilege. The command output confirms each durable operation completed.
- Verification: `cargo test --locked --test metadata_process authenticated_metadata_and_admin_lifecycle_cross_gateway_and_supervisor_processes -- --exact --nocapture` passed (1 test). `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets --locked -- -D warnings` passed in the configured Nix shell.
- Remaining: extend process-level CLI coverage to other DDL families and complete remaining SDK packaging/release gates.

## SDK-5: foreign-key create/drop CLI — implemented

- Contract: foreign-key administration uses ordered, distinct source column IDs and a target PRIMARY KEY/UNIQUE constraint ID; storage validates column correspondence, types, authority, existing rows, and dependencies.
- Implementation: prdb-client exposes foreign-key-create and foreign-key-drop through typed AdminClient calls and reports the generated foreign-key ID.
- Regression: foreign-key source IDs use the same strict nonempty, distinct ID-list parser as key constraints. The real gateway/supervisor process test grants column-level REFERENCES, creates and drops the foreign key, then revokes REFERENCES.
- Verification: focused decoder, parser, CLI help, and real-process checks passed. The full workspace build, format check, warning-denied Clippy, and rerun of the full test suite passed; the commit integration target reported 124 passed and 3 cgroup-dependent ignored. The first full test run had one intermittent existing CHECK lifecycle `InvalidAudit` failure, and that test passed alone.
- Remaining: policy CLI, broader live ADMIN DDL coverage, and confined artifact execution remain open.

## SDK-5: CHECK constraint CLI — implemented

- Contract: CHECK creation accepts a canonical EIR1 row-local Boolean expression and its declared dependency columns; storage validates binding, type, current rows, and authority.
- Implementation: prdb-client exposes check-create and check-drop. It decodes strict lowercase even-length hex locally, accepts an empty dependency list for constant expressions, then delegates typed validation and publication to AdminClient.
- Regression: parser coverage accepts canonical hex and empty dependencies while rejecting empty/odd/uppercase/nonhex input. The real gateway/supervisor ADMIN lifecycle creates and drops a constant-true CHECK after column addition.
- Verification: focused parser and real-process tests, CLI help inspection, format check, full workspace build, warning-denied Clippy, and tests passed under Rust/Cargo 1.95.0; the commit integration target reported 124 passed and 3 cgroup-dependent ignored.
- Remaining: policy CLI, broader live ADMIN DDL coverage, and confined artifact execution remain open.

## SDK-5: row/cell policy ADMIN and CLI — implemented

- Contract: policy creation uses a closed row/cell kind, operation, ALLOW/RESTRICT effect, typed user/role recipients, optional cell-column targets, and operation-required EIR1 clauses. Storage binds the expressions to current schema and checks current `MANAGE_POLICIES` authority.
- Implementation: `AdminClient` exposes typed create/drop policy methods; `prdb-client policy-create/policy-drop` uses the authenticated control channel. The CLI parses canonical recipient IDs and EIR1 hex without exposing policy bodies through metadata discovery.
- Regression: strict decoder coverage checks a canonical row SELECT policy and drop shape. The real gateway/supervisor lifecycle creates and drops row SELECT/INSERT/UPDATE/DELETE plus cell SELECT/INSERT/UPDATE policies, checks committed sequences, binds cell targets to a live column, and sends the operation-required USING/WITH CHECK clauses. It also invokes `policy-create`/`policy-drop` through `prdb-client` for a row SELECT policy with canonical EIR1 Boolean expression bytes; parser coverage rejects unknown kinds, uppercase IDs, and zero IDs.
- Verification: `cargo test --locked --test metadata_process authenticated_metadata_and_admin_lifecycle_cross_gateway_and_supervisor_processes -- --exact --nocapture` passed (1 test). `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets --locked -- -D warnings` passed in the configured Nix shell.
- Remaining: other live ADMIN DDL families and confined artifact execution remain open.

## SDK-5: table-owner transfer CLI and process coverage — implemented

- Contract: [`tables.set_owner`](../../specs/client-protocol.md) accepts a table ID and destination live user/role ID through the authenticated ADMIN channel. The current owner or a database administrator may transfer a non-system table; the new owner receives table-level DDL authority and no data privileges.
- Red evidence: the parser regression first failed to compile because `take_table_owner_args` and the `table-owner-set` command did not exist.
- Implementation: `prdb-client table-owner-set --table-id HEX --owner-id HEX` parses strict nonzero lowercase IDs, rejects extra fields, calls `AdminClient::set_table_owner`, and reports the operation ID and commit sequence.
- Regression: the CLI parser accepts canonical IDs and rejects zero, uppercase, short, and extra values. `authenticated_metadata_and_admin_lifecycle_cross_gateway_and_supervisor_processes` transfers a table from the administrator to a newly credentialed non-admin user, then invokes the compiled `prdb-client table-owner-set` command with that user's private credential to transfer it back. The temporary token and credential file are revoked and removed before shutdown.
- Verification: focused CLI parser and real multi-process ADMIN tests passed. `cargo build --workspace --locked --offline`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`, and `cargo test --workspace --locked --offline -- --test-threads=1` passed in the pinned Nix shell. CLI help displays the new command. The full suite reported 3 cgroup-gated commit tests and 1 cgroup-gated metadata process test as ignored by default.
- Remaining: broaden separate-process DDL coverage and finish confined artifact execution and the remaining SDK packaging/release gates.


## SDK-5: schema-owner transfer client and CLI — implemented

- Contract: [`schemas.set_owner`](../../specs/client-protocol.md) lets the current schema owner or a database administrator transfer a non-system schema to a live user or role. The transfer does not grant data privileges.
- Red evidence: `client_admin` failed to compile because `AdminClient::set_schema_owner` did not exist.
- Implementation: `AdminClient::set_schema_owner` validates both IDs and validates the returned schema and owner ID echoes. `prdb-client schema-owner-set --schema-id HEX --owner-id HEX` applies strict ID parsing and reports the committed operation.
- Regression: the AdminClient mock verifies request and response fields. CLI parser checks reject zero, uppercase, short, and extra IDs. The real gateway/supervisor lifecycle transfers a schema to a newly credentialed non-admin user and invokes the compiled CLI with that user's credential to transfer it back.
- Verification: the focused AdminClient mock, CLI parser, and real multi-process CLI regressions passed. Rust 1.95.0 workspace build, formatting, warning-denied Clippy, and full serial workspace tests passed. The commit suite reported 127 passed and 3 cgroup-dependent ignored; the metadata-process suite reported 1 passed and 1 cgroup-dependent ignored.
- Remaining: broader separate-process DDL coverage and confined artifact execution remain open.


## SDK-4: owner-scoped cancellation client and CLI — implemented

- Contract: [`CANCEL`](../../specs/client-protocol.md) accepts a caller-owned prepared or active invocation and returns accepted, too-late, terminal, or unknown state. Acknowledgement is not a transaction terminal event.
- Red evidence: the new protocol regression failed to compile because `CancellationResult` and `SubmissionClient::cancel` did not exist.
- Implementation: `SubmissionClient::cancel` sends kind 9 once, validates the response shape and closed state set, and rejects a zero invocation ID. `prdb-client cancel` authenticates from the private credential file and prints the server acknowledgement.
- Regression: mock TCP coverage verifies kind, invocation ID, secret zeroization, and all five valid state results. The real gateway/supervisor lifecycle cancels an already committed owned invocation through both the Rust client and compiled CLI, confirming committed terminal status is returned.
- Verification: the focused cancellation mock and real multi-process lifecycle passed. Rust 1.95.0 workspace build, format check, warning-denied Clippy, and full serial tests passed; the commit suite reported 127 passed and 3 cgroup-dependent ignored, and the metadata-process suite reported 1 passed and 1 cgroup-dependent ignored. `make -f engine/sdk/Makefile test` also passed with TinyCC 0.9.27-unstable-2025-01-06.
- Remaining: generated bindings and other SDK/package release gates remain open.


## SDK-5: administrator-authorized invocation cancellation — implemented

- Contract: [`CANCEL`](../../specs/client-protocol.md) in control mode requires current database-administrator authority and reports the invocation cancellation decision.
- Red evidence: the control-mode regression failed to compile because `AdminClient::cancel_invocation` did not exist.
- Implementation: `AdminClient::cancel_invocation` sends the typed cancellation request on its existing authenticated control connection. `prdb-client admin-cancel` loads a private administrator credential and reports the returned decision.
- Regression: the mock verifies control HELLO/AUTH and kind-9 invocation cancellation. The live gateway/supervisor lifecycle uses both `AdminClient` and the compiled `admin-cancel` command on a committed invocation, verifying terminal reconciliation through the production control path.
- Verification: the focused control-channel mock and real multi-process ADMIN/CLI regressions passed. Rust 1.95.0 workspace build, format check, warning-denied Clippy, and full serial workspace tests passed; the commit suite reported 127 passed and 3 cgroup-dependent ignored, and the metadata-process suite reported 1 passed and 1 cgroup-dependent ignored.
- Remaining: generated bindings and other SDK/package release gates remain open.

## SDK-1 dependency: C executor cancellation checkpoint — implemented

- Contract: executor IPC `0x1113` returns a versioned cancellation/deadline state; storage aborts the invocation before reporting cancellation.
- Implementation: `prdb_sdk_executor_check_cancelled_v1` provides a bounded C SDK call for query runners to poll between work batches and validates exact response length, version, and boolean value.
- Regression: the maintained TinyCC executor fixture verifies active and cancelled values, malformed boolean rejection, and unchanged output on error. A Rust storage/IPC regression checks the state before and after administrator cancellation.
- Verification: focused fixture passed; full Rust and SDK checks recorded with the feature commit.
- Remaining: verify cancellation latency in the sandbox harness, and complete generated bindings and other SDK/package release gates.
- Additional detail migrated from the storage integration record: default inputs use an explicit scalar type and cover SQL NULL, current-principal UUID, DECIMAL, VARCHAR, BYTEA hex, UUID, DATE epoch days, and TIMESTAMPTZ epoch microseconds. Its full-suite rerun reported 124 commit tests passed and 3 cgroup-dependent tests ignored after an intermittent CHECK-lifecycle `InvalidAudit` passed in isolation.

## SM-5 / SDK-5: cross-process authenticated ADMIN identity and token lifecycle — verified

- Contract: client control operations must traverse the gateway and supervisor boundary, then reach storage's authorized DDL/token lifecycle instead of a test-only storage call.
- Regression: the real-process metadata integration now issues role create/rename/drop, user creation, one-time token issue, credential-file save/load, fresh token-authenticated metadata discovery, and token revoke through `AdminClient`. A post-revocation authentication attempt is rejected. The test confirms one-time client secret buffers are zeroized.
- Verification: the focused integration test passed and `cargo test --workspace --locked` passed with 124 commit tests and 3 cgroup-dependent ignored. Full build, format, and warning-denied Clippy passed.
- Remaining: multi-process coverage for the other ADMIN DDL families and the confined artifact submission path remains open.

## SDK-5: schema/table DDL CLI and multi-process coverage — implemented

- Contract: [client SDK](../../specs/client-sdk.md) requires schema/table administrative work to use the authenticated control channel and typed `AdminClient` operations; query artifacts receive no administrative credentials.
- Implementation: `prdb-client` now exposes schema create/rename/drop and table create/rename/drop. Table creation parses semicolon-separated typed column declarations, bounds column count, validates scalar parameters, duplicate names, nullability, and the non-null UUID identity modifier, then delegates to the existing typed client request. Literal/default expressions remain available through `AdminClient` but are not yet part of this CLI syntax.
- Regression: parser tests cover parameterized DECIMAL and VARCHAR types, UUID identity columns, and reject invalid precision/scale, zero VARCHAR length, duplicates, missing defaults for non-null non-identity columns, and invalid identity combinations. The real-process metadata integration now creates a schema and table, renames/drops the table, and drops the schema through the gateway and supervisor.
- Verification: focused CLI parser tests passed (6); the focused authenticated metadata/admin process test passed. Final `cargo build --workspace --locked`, `cargo fmt --all -- --check`, warning-denied workspace Clippy, and `cargo test --workspace --locked` passed in the Nix shell. The commit integration target reported 124 passed and 3 cgroup-dependent ignored. Updated `prdb-client --help` was inspected.
- Remaining: column add/rename/drop, defaults/nullability, indexes, constraints, privileges, policies, and broader multi-process ADMIN DDL coverage remain for later slices; confined artifact submission still requires the sandbox integration environment.

## SDK-5: column add/rename/drop CLI and process coverage — implemented

- Contract: column DDL uses authenticated typed ADMIN operations, and storage applies schema evolution and its write-epoch fences atomically.
- Implementation: `prdb-client` now exposes `column-add`, `column-rename`, and `column-drop`. Column addition accepts one nullable typed column and returns the generated column ID; rename/drop require the table and column IDs. Storage's existing AdminClient API still supports broader add-column defaults.
- Regression: the parser rejects non-null or identity columns for the CLI add form and rejects multiple declarations. The authenticated gateway/supervisor process test exercises add, rename, and drop on the created table.
- Verification: focused CLI parser and real-process tests passed. Workspace build, format, warning-denied Clippy, and full tests passed under the prescribed Nix shell; the commit integration target reported 124 passed and 3 cgroup-dependent ignored.
- Remaining: nullability/default changes, indexes, constraints, privileges, policies, additional multi-process DDL coverage, and confined artifact submission remain open.

## SDK-5: ordinary index CLI and process coverage — partial

- Contract: authenticated index create/rebuild/drop requests use the existing typed AdminClient and server-side authorization/fencing path.
- Implementation: the CLI exposes create, rebuild, and drop with canonical ordered column IDs; uniqueness defaults to false and is an optional boolean flag.
- Regression: parser coverage checks canonical nonzero ID lists and duplicate rejection. The real-process ADMIN lifecycle now covers index create, rebuild with a changed uniqueness profile, and drop; existing typed-client and ADMIN dispatcher tests cover rebuild request construction.
- Verification: focused CLI parsing and real multi-process index lifecycle passed. Full workspace build, format check, warning-denied Clippy, and tests passed; the commit integration target reported 124 passed and 3 cgroup-dependent ignored.
- Remaining: constraint, privilege, and policy CLI commands remain open; confined artifact execution still needs a delegated-cgroup integration environment.

## SM-5 / SDK-5: authenticated policy ADMIN and CLI — implemented

- Contract: strict PREPARE/START accepts canonical row/cell policy shape, recipient IDs, cell-column targets, and nullable EIR1 USING/WITH CHECK expressions. START delegates to typed StorageManager policy create/drop and revalidates authority at serialized commit.
- Implementation: `policies.create` and `policies.drop` are connection-owned ADMIN operations. `AdminClient` encodes typed policy enums, recipients, target columns and expressions; `prdb-client policy-create/policy-drop` exposes the same path.
- Regression: protocol decoder tests accept canonical row-policy create/drop and reject malformed shapes; the real gateway/supervisor process lifecycle creates and drops row SELECT/INSERT/UPDATE/DELETE and cell SELECT/INSERT/UPDATE ALLOW policies, using required USING/WITH CHECK clauses and a live cell target. CLI recipient parser accepts only user/role plus canonical nonzero lowercase IDs.
- Verification: focused protocol, multi-process, and CLI parser tests passed; updated `prdb-client --help` was inspected. Rust 1.95.0 workspace build, formatting, warning-denied Clippy, and full tests passed; the commit integration target reported 124 passed and 3 cgroup-dependent ignored.
- Remaining: broader authenticated ADMIN families, repeated race stress, and confined artifact execution remain open.

## SDK-5: CHECK constraint CLI through authenticated processes — verified

- Regression: the gateway/supervisor metadata lifecycle now creates and drops a CHECK constraint through `prdb-client check-create` and `check-drop`, using the CLI's EIR1 input and generated constraint ID. This replaces the test's direct `AdminClient` calls for that lifecycle and verifies the user-facing path reaches storage.
- Verification: the focused `authenticated_metadata_and_admin_lifecycle_cross_gateway_and_supervisor_processes` test passed. The full locked workspace build, formatting check, warning-denied Clippy, serial workspace tests, and `git diff --check` passed; the suite reported 138 commit tests passed with 4 delegated-cgroup tests ignored and 1 authenticated metadata-process test passed.
- Remaining: process-level coverage for additional authenticated ADMIN families, repeated race stress, and confined artifact execution remain open.

## SDK-5 follow-up: role membership CLI through separate services — verified

- Regression: the real metadata/admin process lifecycle issues `role-grant` and `role-revoke` through `prdb-client` over the TCP gateway and private supervisor service. A user inherits `MANAGE_ROLES` from the granted role and successfully creates a role. After membership revoke, the same token still authenticates but no longer authorizes `role-create`; the referenced role can then be dropped.
- Verification: `authenticated_metadata_and_admin_lifecycle_cross_gateway_and_supervisor_processes` passed with separate supervisor and gateway binaries.
- Remaining: broader authenticated ADMIN families, repeated race stress, and confined query execution remain open.

## SDK-5: foreign-key CLI through authenticated processes — verified

- Regression: the gateway/supervisor metadata lifecycle now creates and drops a foreign key through `prdb-client foreign-key-create` and `foreign-key-drop`, using a source table, the existing REFERENCES grant, and a unique target constraint. The test verifies the CLI command path traverses authenticated ADMIN handling and reaches storage.
- Verification: the focused `authenticated_metadata_and_admin_lifecycle_cross_gateway_and_supervisor_processes` test passed. The full locked workspace build, formatting check, warning-denied Clippy, serial workspace tests, and `git diff --check` passed; the commit suite reported 138 passed and 4 cgroup-dependent ignored, and the metadata-process test reported 1 passed and 1 cgroup-dependent ignored.
- Remaining: process-level coverage for additional authenticated ADMIN families, repeated race stress, and confined artifact execution remain open.

## SDK-5: column nullability and default CLI process coverage — verified

- Regression: the authenticated gateway/supervisor lifecycle now exercises `column-nullability` in both directions and `column-default-set`/`column-default-drop` through `prdb-client`, using its private credential file and the existing typed text column.
- Verification: the focused `authenticated_metadata_and_admin_lifecycle_cross_gateway_and_supervisor_processes` test passed. The full locked workspace build, formatting check, warning-denied Clippy, serial workspace tests, and `git diff --check` passed; the commit suite reported 138 passed and 4 cgroup-dependent ignored, and the metadata-process test reported 1 passed and 1 cgroup-dependent ignored.
- Remaining: process-level coverage for additional authenticated ADMIN families, repeated race stress, and confined artifact execution remain open.

## SDK-5 follow-up: user lifecycle CLI through separate services — verified

- Regression: the real metadata/admin process lifecycle now exercises `user-rename`, `user-login`, and `user-drop` through `prdb-client` over the TCP gateway and private supervisor service. It confirms disabling login revokes the issued token, re-enabling login does not restore it, a replacement token can be issued, and user drop revokes that replacement.
- Verification: `authenticated_metadata_and_admin_lifecycle_cross_gateway_and_supervisor_processes` passed with separate supervisor and gateway binaries.
- Remaining: broader authenticated ADMIN families, repeated race stress, and confined query execution remain open.
## SDK-5 follow-up: token lifecycle and inventory CLI — verified

- Behavior: authenticated control clients can request a user's token inventory by stable token-ID continuation. Responses are strictly decoded into non-secret metadata, ordered and capped at 64 records; malformed fields, inconsistent revocation timestamps, invalid order, and malformed continuations fail closed.
- Implementation: `AdminClient::list_tokens_page` wraps the existing version-1 `tokens.list` ADMIN read operation. `prdb-client token-list` prints one JSON page and accepts `--after-token-id`; it emits only token IDs, labels, issue/expiry times, revocation state, and the continuation ID.
- Regression: the TCP client fixture checks the exact request with a supplied cursor, decodes a 64-token page and its continuation, then requests and decodes the next page. The CLI JSON string encoder has an escaping regression.
- Process evidence: `authenticated_metadata_and_admin_lifecycle_cross_gateway_and_supervisor_processes` issues a token through `prdb-client token-issue` into a private credentials file, lists it through `token-list`, then revokes through `token-revoke` and verifies the listed revocation state. The credential authenticates a fresh metadata connection before revocation; CLI output and inventory omit the bearer secret.
- Contracts: [client SDK](../../specs/client-sdk.md) and [client protocol](../../specs/client-protocol.md).
- Verification: the focused TCP client test and CLI escaping test passed; the focused gateway/supervisor process test passed in the delegated `systemd --user` service. Workspace build, formatting, warning-denied Clippy, maintained TinyCC SDK suite, and the full serial delegated-cgroup workspace suite passed with zero ignored tests. Commit integration reported 145 passed; transaction integration reported 74 passed. `git diff --check` passed.
- Remaining: broader authenticated CLI coverage for additional administrator operations and repeated authority-race stress remain open.

## SDK-17 / OM-2: live DDL fence wait projection — verified slice

- Contract: administrator runtime invocation pages include bounded best-effort DDL fence waits, with opaque wait/resource/blocker IDs, category, duration, and explicit truncation flags. The projection is available only through the existing administrator-authorized page path.
- Implementation: the fence manager tracks blocked acquisitions and the held leases that block them. It caps the live waiter registry at 128, returned pages at 16 waits, and each wait at 8 resource IDs and 8 blocker IDs. The typed client strictly validates these records and `prdb-client runtime-list` includes them in JSON output.
- Regression: unit coverage exercises blockers, cleanup, waiter/resource caps, and truncation; protocol and TCP fixtures cover serialization/decoding; storage and gateway/supervisor checks preserve authorization and report empty pages when no waits exist.
- Verification: focused regressions, workspace build, formatting, warning-denied Clippy, maintained TinyCC SDK tests and sandbox fixture, release-profile validation, the delegated cgroup probe, and the full serial delegated-cgroup workspace suite passed with zero ignored tests. `git diff --check` passed.
- Remaining: additional wait categories, job/memory/history/counter registries, and broader pressure/authority-race testing remain open.

## SDK-17 / OM-2: pinned-history summary projection — verified slice

- Behavior: `runtime-list` pages include `pinned_snapshot_count` and `oldest_snapshot_pin_age_us`, sampled from live invocation-owned redb snapshot pins. The summary is independent of page records and excludes retained-byte or total-memory estimates.
- Implementation: the existing authorized server page, protocol encoder, strict typed `AdminClient` decoder, and CLI JSON projection carry both fields.
- Regression: the gateway/supervisor process assertion failed when the fields were absent and passed after implementation; storage and TCP fixtures verify the aggregate and decoding.
- Verification: focused storage/TCP regressions, workspace build/fmt/Clippy, TinyCC tests and sandbox fixture, release-profile validation, delegated cgroup probe, full serial workspace suite in delegated `systemd --user` with zero ignored tests, and `git diff --check` passed.
- Remaining: additional operational registries, wait categories, and broader pressure/authority-race tests remain open under OM-2.

## SDK-17 / OM-2: admission and conflict counter projection — verified slice

- Behavior: `runtime-list` reports `invocations_admitted_total` and `invocation_conflicts_total` as saturating process-local counters, reset when the process-local registry is recreated; they are not durable lifetime counts.
- Implementation: successful registry admission increments the first counter, and invocation commit returning `CommitError::Conflict` increments the second. The authorized page encoder, typed TCP decoder, and CLI JSON output carry both values.
- Regression: the runtime response assertion failed when the conflict field was omitted, then passed with it restored. A storage race verifies an actual conflict increments the counter; the client fixture verifies decoding and unit coverage verifies saturation.
- Verification: focused regressions, workspace build/fmt/Clippy, maintained TinyCC tests and sandbox fixture, release-profile validation, delegated cgroup probe, and the full serial delegated-cgroup workspace command all succeeded; `git diff --check` passed.
- Remaining: further runtime registries and counters remain open under OM-2.

## SDK-17 / OM-2: audited cancellation counter projection — verified slice

- Behavior: `runtime-list` includes `admin_cancellations_accepted_total`, counting only the first audited transition to cancellation-requested. Denied, too-late, and repeated accepted requests are excluded; the process-local counter saturates and resets with the invocation registry.
- Implementation: the server runtime projection and serializer, typed TCP decoder, and CLI JSON output carry the new counter.
- Regression: storage lifecycle coverage verifies a first accepted cancellation increments once and an accepted repeat leaves the count unchanged. The TCP fixture verifies typed decoding, and saturation coverage checks `u64::MAX` behavior.
- Verification: focused storage/client/saturation checks, workspace build/fmt/Clippy, TinyCC tests and sandbox fixture, release-profile validation, delegated cgroup probe, full serial delegated-cgroup workspace suite with zero ignored tests, and `git diff --check` passed.
- Remaining: additional runtime counters and registries remain open under OM-2.

## SDK-17 / OM-2: audited cancellation denial counter projection — verified slice

- Behavior: `runtime-list` exposes `admin_cancellations_denied_total` for cancellation requests with a durably committed `Denied` AUD1 event. Unauthorized, rate-limited, unaudited late requests, and audit failures do not increment it; the process-local counter saturates and resets with the registry.
- Implementation: the storage runtime page, protocol JSON encoder, typed client, and CLI JSON output carry the counter.
- Regression: runtime-page output was missing the counter before implementation. Storage lifecycle checks cover denied audits for terminal and unknown targets, an accepted request leaves the denied count unchanged, and a rate-limited request remains uncounted. TCP and saturation tests verify decoding and bounds.
- Verification: focused storage, runtime, client and saturation tests; workspace build/fmt/Clippy; TinyCC suite and fixture; release-profile validation; delegated cgroup probe; full serial delegated-cgroup workspace suite with zero ignored tests; and `git diff --check` passed.
- Remaining: additional runtime counters and registries remain open under OM-2.

## SDK-17 / OM-2: invocation policy denial counter projection — verified slice

- Behavior: `runtime-list` exposes `invocation_policy_denials_total`, counting each managed invocation executor IPC request that returns explicit `AuthorizationError::Denied`. Hidden rows/cells, unavailable relations, malformed requests, and policy evaluation failures are excluded. The process-local counter saturates and resets with the registry.
- Implementation: the executor dispatch boundary increments the global registry counter through a weak metrics handle, preserving immediate admission-slot release on abort.
- Regression: a SELECT-only principal resolves a visible relation, then receives `AuthorizationError::Denied` for `StageInsert`; the authorized runtime page reports one. The typed TCP fixture decodes the counter, and saturation coverage verifies its bound.
- Verification: observed the expected runtime assertion failure before implementation; focused storage/runtime, TCP and saturation tests; workspace build/fmt/Clippy; TinyCC tests and fixture; release-profile validation; delegated cgroup probe; full serial delegated-cgroup workspace suite with zero ignored tests; and `git diff --check` passed.
- Remaining: additional runtime counters and registries remain open under OM-2.

## SDK-17 / OM-2: cumulative staged payload byte counter — verified slice

- Behavior: `runtime-list` exposes `invocation_staged_value_bytes_total`, the saturating cumulative encoded payload bytes accepted by successful managed overlay puts. Replacements count again; deletes and failed writes do not count. The total resets with the process-local registry.
- Implementation: managed overlay writes update the shared admission registry after a successful put; the admin page, protocol encoder, strict typed client, and CLI JSON carry the field.
- Regression: the authorized runtime response lacked the field before implementation. Storage checks the initial put, replacement, and delete semantics; TCP decoding and saturation coverage verify the projection.
- Verification: focused runtime/protocol/TCP/saturation checks; workspace build/fmt/Clippy; TinyCC SDK tests and sandbox fixture; release-profile validation; delegated cgroup probe; full serial delegated-cgroup workspace suite with zero ignored tests; and `git diff --check` passed.
- Remaining: other OM-2 counters and registries remain open.

## SDK-17 / OM-2: managed commit latency buckets — verified slice

- Behavior: administrator `runtime-list` pages include the six-count `invocation_commit_latency_buckets` array with fixed noncumulative ranges from `<=1,000us` through `>100,000us`. It counts managed commit API calls on success and error, uses monotonic elapsed time, saturates, and resets with the shared registry.
- Implementation: commit duration is recorded after the managed commit returns; runtime protocol JSON, strict typed decoding, and CLI JSON expose the array.
- Regression: the authorized runtime response lacked the field before implementation. Boundary coverage checks all ranges and sub-microsecond precision; storage integration verifies an errored managed commit increments one bucket; TCP decoding and saturation tests cover the wire contract. The authenticated gateway/supervisor CLI test now asserts the policy-denial, staged-byte, and latency-bucket fields in a real `runtime-list` response.
- Verification: focused storage/protocol/TCP tests and the delegated multi-process metadata test; workspace build/fmt/Clippy; TinyCC tests and fixture; release-profile validation; delegated cgroup probe; full serial delegated-cgroup workspace suite with zero ignored tests; and `git diff --check` passed.
- Remaining: further operational metadata items remain open.

## SDK value validation: DATE and TIMESTAMPTZ ranges — implemented

- Behavior: the borrowed C value validator and SQL parameter binder reject DATE days outside `-719162..=2932896` and TIMESTAMPTZ microseconds outside `-62135596800000000..=253402300799999999`, matching the storage tuple contract. Public bound macros expose the canonical limits.
- Regression: maintained TinyCC fixtures cover both endpoints and one value beyond each endpoint for direct validation and SQL parameter binding. The new out-of-range DATE assertion failed before the validator change.
- Contracts: [client SDK](../../specs/client-sdk.md), [DDL and schema](../../specs/ddl-schema.md).
- Verification: `make -f engine/sdk/Makefile test`, sandbox query fixture build, release-profile validation, and the full serial delegated-cgroup workspace suite passed; the latter reported zero ignored tests. Formatting, workspace build, warning-denied Clippy, and `git diff --check` passed.
- Remaining: no additional DATE/TIMESTAMPTZ scalar range gap is known in SDK input validation.

## SDK-3 manifest ELF inspection: GNU hash support — implemented

- Behavior: manifest inspection bounds dynamic symbols using SysV `DT_HASH` or GNU `DT_GNU_HASH`. GNU bucket chains must terminate within the symbol and traversal bounds. When both tags exist, each table is validated independently and their symbol counts must agree. Duplicate hash tags and malformed tables are rejected; artifacts are never loaded for inspection.
- Regression: a focused GNU-hash unit fixture checks a two-symbol chain and rejects a missing terminator. Freshly compiled packager artifacts exercise GNU-only and dual-hash ELF forms; two packager integration tests pass with dual-hash output.
- Contract: [client SDK](../../specs/client-sdk.md).
- Verification: focused GNU-hash and packager tests, formatting, workspace build, warning-denied Clippy, and the full serial delegated-cgroup workspace suite passed. All sandbox and process tests were enabled; the suite reported zero ignored tests. `git diff --check` passed.
- Remaining: ELF inspection remains intentionally bounded and supports the repository's current little-endian ELF64 Linux x86_64 profile.

## SDK value validation: SQL binder boundary matrix — verified

- Regression: SQL parameter binding now checks both DATE endpoints and adjacent out-of-range values, plus both TIMESTAMPTZ endpoints and adjacent out-of-range values. This closes the binder-side coverage gap; direct value validation already had the full matrix.
- Verification: `nix develop --no-update-lock-file -c make -f engine/sdk/Makefile test` passed, including the TinyCC SQL parameter fixture and all maintained SDK executables.

## SDK-3 manifest ELF inspection: GNU bloom mapping validation — implemented

- Regression: a synthetic ELF split its GNU-hash header and bucket/chain bytes across load segments while leaving the advertised bloom-filter virtual range unmapped. The parser initially accepted it; the focused test reproduced `Ok(2)` instead of `InvalidArtifact`.
- Behavior: GNU-hash inspection now requires the entire bloom-filter byte range to map to file-backed load-segment bytes before reading buckets and chains.
- Verification: all 14 submission-manifest unit tests passed; workspace formatting, locked build, warning-denied Clippy, and the full serial delegated-cgroup workspace suite passed with zero ignored tests. `git diff --check` passed.


## SDK-5 follow-up: typed defaults through the SQL DDL helper — verified

- Contract: [client SDK](../../specs/client-sdk.md). SQL DDL is exposed only by the authenticated `AdminClient`; no administrative credential or capability is added to query artifacts.
- Implementation/evidence: `AdminClient::execute_create_table_sql` maps exact typed SQL defaults to `AdminColumnDefinition` and uses the existing PREPARE/START client method. The owning schema DDL progress records parser and control-session coverage.
- Verification: workspace build, formatting, warning-denied Clippy, maintained TinyCC tests, release-profile validation, delegated-cgroup preflight, and the complete serial delegated-cgroup workspace suite passed; zero tests were ignored.
- Follow-up: `prdb-client admin-sql` now exposes the bounded helper through a local SQL-file/stdin command. Its process tests verify typed control-mode PREPARE/START and reject unsupported SQL before credentials or network; see [client SQL command progress](../client-sql-command/PROGRESS.md).

## SDK-5 follow-up: generated UUID identity through SQL DDL — implemented

- Contract: [client SDK](../../specs/client-sdk.md) and [schema DDL](../../specs/ddl-schema.md). The SQL helper represents storage-owned identity using the existing typed `tables.create` field.
- Regression: the authenticated mock control session checks `generated_uuid_identity: true` in PREPARE arguments, and the CLI process test now uses an identity column while verifying the committed four-column result.
- Verification: the authenticated AdminClient control-session regression passed and asserted the identity flag in the typed PREPARE request. Workspace formatting, locked offline build, warning-denied all-target Clippy, and the complete serial delegated-cgroup workspace suite passed; sandbox and process tests ran with zero ignored tests. Component-level SQL parser and storage boundaries are recorded in [schema DDL progress](../ddl-schema/PROGRESS.md).

## SDK-5 follow-up: explicit-owner CREATE SCHEMA SQL — implemented

- Contract: [client SDK](../../specs/client-sdk.md), [schema DDL](../../specs/ddl-schema.md), and [client SQL command](../../specs/client-sql-command.md). `AdminClient::execute_create_schema_sql` maps bounded local SQL to the existing authenticated `schemas.create` operation.
- Regression: mock control-session coverage verifies UUID owner text becomes the same typed owner ID used by `create_schema`; the CLI process test verifies authenticated PREPARE/START and the committed schema ID.
- Verification: the authenticated AdminClient schema lifecycle control-session test passed with SQL owner UUID mapped to the expected typed owner ID. Workspace formatting, locked offline build, warning-denied all-target Clippy, and the complete serial delegated-cgroup workspace suite passed; sandbox and process tests ran with zero ignored tests. Details are in [schema DDL progress](../ddl-schema/PROGRESS.md).

## SDK-5 follow-up: schema rename and drop through SQL — implemented

- Contract: [client SDK](../../specs/client-sdk.md) and [schema DDL](../../specs/ddl-schema.md). SQL entry points resolve only schemas visible through the authenticated control metadata API and dispatch the existing typed operations.
- Regression: mock authenticated control coverage resolves the schema twice, verifies `schemas.rename` and `schemas.drop` arguments, and checks their committed outcomes.
- Verification: the authenticated AdminClient TCP rename/drop regression passed. Workspace formatting, locked offline build, warning-denied all-target Clippy, and the complete serial delegated-cgroup workspace suite passed; sandbox and process tests ran with zero ignored tests. Details are in [schema DDL progress](../ddl-schema/PROGRESS.md).

## SDK-5 follow-up: table rename and restricted drop through SQL — implemented

- Contract: [client SDK](../../specs/client-sdk.md) and [schema DDL](../../specs/ddl-schema.md). The SQL helper resolves an authorized visible schema/relation and then uses existing typed table lifecycle requests.
- Regression: mock TCP coverage resolves schema and relation pages, asserts `tables.rename` and `tables.drop` PREPARE arguments, and checks committed operation results.
- Verification: the authenticated AdminClient TCP rename/drop regression passed after resolving metadata pages. Workspace formatting, locked offline build, warning-denied all-target Clippy, and the complete serial delegated-cgroup workspace suite passed; sandbox and process tests ran with zero ignored tests. Details are in [schema DDL progress](../ddl-schema/PROGRESS.md).

## SDK-5: CREATE TABLE SQL with a table-level PRIMARY KEY — implemented

- Contract: [client SDK](../../specs/client-sdk.md) and [schema DDL](../../specs/ddl-schema.md). `AdminClient::execute_create_table_sql` parses an optional table-level PK, implies NOT NULL on its columns, and sends ordered names in the existing typed `tables.create` PREPARE request. `AdminClient::create_table_with_primary_key` is also available for structured callers and validates the supplied columns before sending.
- Regression: authenticated TCP coverage asserts the typed `primary_key_columns` field; SQL parser and strict decoder tests cover composite ordering, implied nullability, nullable rejection, and duplicates.
- Verification: focused parser/decoder/AdminClient tests, locked workspace build, format check, warning-denied Clippy, full serial delegated-cgroup workspace suite with sandbox/process tests enabled and zero ignored tests, and `git diff --check` passed.

## SDK-5: CREATE TABLE SQL with table-level UNIQUE — implemented

- Contract: [client SDK](../../specs/client-sdk.md) and [schema DDL](../../specs/ddl-schema.md). SQL parsing accepts multiple final table-level UNIQUE clauses; `AdminClient` serializes them in the typed `tables.create` request, and `create_table_with_constraints` supports structured callers.
- Regression: authenticated TCP coverage checks nested UNIQUE arrays alongside a PRIMARY KEY; parser tests cover composite/multiple definitions, while decoder tests cover structure and invalid references.
- Verification: focused parser/decoder/AdminClient tests, locked workspace build, format check, warning-denied Clippy, full serial delegated-cgroup workspace suite with sandbox/process tests enabled and zero ignored tests, and `git diff --check` passed.

## SDK-5: ALTER TABLE SQL for PRIMARY KEY and UNIQUE — implemented

- Contract: [client SDK](../../specs/client-sdk.md) and [schema DDL](../../specs/ddl-schema.md). `AdminClient::execute_add_key_constraint_sql` parses qualified names, resolves the visible relation and ordered columns on the same authenticated control connection, and dispatches existing typed PRIMARY KEY or UNIQUE creation.
- Regression: the mock TCP integration verifies schema/relation/column discovery, ordered ID mapping, typed UNIQUE PREPARE arguments, and generated constraint/index IDs. Parser tests cover supported and rejected forms.
- Verification: focused parser/AdminClient tests, locked workspace build, format check, warning-denied Clippy, complete serial delegated-cgroup workspace suite with sandbox/process tests enabled and zero ignored tests, and `git diff --check` passed.

## Administrative SQL: bounded ALTER TABLE CHECK — implemented

- Contract: qualified `ALTER TABLE schema.table ADD CHECK (...)` resolves names over authenticated metadata and submits EIR1 through the existing typed CHECK operation. The accepted expression subset includes Boolean fields, `IS [NOT] NULL`, `NOT`, `AND`, `OR`, parentheses, and typed scalar comparisons/literals.
- Binding: fields use the metadata column ordinal in EIR1 and their stable column IDs as dependency declarations. Storage continues to validate expression type, dependencies, authority, and existing rows.
- Limit: expressions remain bounded to deterministic row-local EIR1; named constraints and FOREIGN KEY SQL remain unsupported.
- Verification: focused authenticated TCP coverage verified metadata resolution, ordinal binding, stable dependency IDs, typed CHECK PREPARE, and generated results. Locked workspace build, format check, warning-denied all-target Clippy, full serial delegated-cgroup workspace suite (sandbox/process tests enabled), and `git diff --check` passed.

## Administrative SQL: ALTER COLUMN SET/DROP NOT NULL — implemented

- `execute_alter_column_nullability_sql` parses the two qualified forms, resolves schema/table/column over the authenticated control connection, and invokes `tables.alter_column_nullability` with the stable IDs.
- Authenticated TCP regression verifies metadata requests and typed PREPARE fields. The CLI test verifies supported SQL is recognized before credentials/network.
- Storage remains responsible for NOT NULL backfill validation and DDL fencing.
- Verification: focused parser, AdminClient TCP, and CLI process tests passed. Locked workspace build, format check, warning-denied all-target Clippy, complete serial delegated-cgroup workspace suite with sandbox/process tests enabled, and `git diff --check` passed.

## Administrative SQL: ALTER TABLE ADD COLUMN — implemented

- `execute_add_column_sql` parses one bounded typed column definition, resolves the qualified table through authenticated metadata, and submits `tables.add_column` with the existing validated column encoder.
- The SQL path rejects generated identities and context-dependent backfill defaults; NOT NULL requires a non-null literal. The storage path remains authoritative for row limits, index/locator rewrites, fencing, and authorization.
- Parser, authenticated TCP, and CLI pre-credential recognition regressions added.
- Verification: focused parser, AdminClient TCP, and CLI process tests passed. Locked workspace build, format check, warning-denied all-target Clippy, full serial delegated-cgroup workspace suite with sandbox/process tests enabled, and `git diff --check` passed.

## Administrative SQL: RENAME COLUMN — implemented

- `execute_rename_column_sql` parses the qualified form, resolves schema/table/column using authenticated metadata, and submits `tables.rename_column` with the stable column ID.
- Added parser, TCP binding/typed-operation, and CLI pre-credential recognition regressions.
- Verification: focused parser, AdminClient TCP, and CLI process tests passed. Locked workspace build, format check, warning-denied all-target Clippy, full serial delegated-cgroup workspace suite with sandbox/process tests enabled, and `git diff --check` passed.

## Administrative SQL: DROP COLUMN RESTRICT — implemented

- `execute_drop_column_sql` parses qualified RESTRICT syntax, resolves the stable column ID on the authenticated control connection, and calls `tables.drop_column`.
- Added parser, authenticated TCP, and CLI pre-credential recognition regressions. Dependency validation and bounded rewrites remain in storage.
- Verification: focused parser, AdminClient TCP, and CLI process tests passed. Locked workspace build, format check, warning-denied all-target Clippy, full serial delegated-cgroup workspace suite with sandbox/process tests enabled, and `git diff --check` passed.

## Administrative SQL: atomic CREATE TABLE CHECK — implemented

- `execute_create_table_sql` now accepts bounded final table-level CHECK clauses, validates referenced names against declared columns, binds them to column ordinals, and encodes EIR1 in the existing typed `tables.create` operation. Storage writes check objects and dependency edges in the same atomic create commit.
- Added parser, client protocol, authenticated AdminClient, CLI, and storage regressions. Typed scalar comparisons and literals are supported; named constraints and FOREIGN KEY SQL remain outside this subset.
- Focused regressions, locked workspace build, format check, warning-denied all-target Clippy, full serial delegated-cgroup workspace tests (including sandbox/process tests with zero ignored), maintained TinyCC SDK tests, sandbox fixture build, and `git diff --check` passed.


## Administrative SQL: typed CHECK comparisons and literals — implemented

- CREATE TABLE and ALTER TABLE CHECK now share schema-aware expression binding for all six comparison operators and typed scalar literals. ALTER binds names/types through authenticated column metadata and derives stable dependency IDs from the resulting EIR tree.
- DECIMAL literals are parsed exactly at the compared column's scale; incompatible types, overflow, and lossy scale reduction reject before submission.
- Parser, expression binding, authenticated AdminClient CREATE/ALTER, CLI, and storage regressions passed. Locked workspace build, format check, warning-denied all-target Clippy, complete serial delegated-cgroup workspace suite (sandbox/process tests enabled; zero ignored), and `git diff --check` passed.


## Administrative SQL: foreign-key create/drop — implemented

- `ALTER TABLE ... ADD FOREIGN KEY` resolves source and target metadata, chooses the visible ordered PK/UNIQUE target, and uses the existing typed operation. `DROP CONSTRAINT 'generated-id'` confirms the FK belongs to the selected relation before removal.
- Authenticated TCP and CLI syntax recognition regressions passed. Locked workspace build, format check, warning-denied all-target Clippy, complete serial delegated-cgroup workspace suite (sandbox/process tests enabled; zero ignored), and `git diff --check` passed.


## SDK: atomic CREATE TABLE FOREIGN KEY SQL — implemented

- `AdminClient::execute_create_table_sql` resolves each referenced schema, table, ordered columns, and visible PRIMARY KEY/UNIQUE constraint through the authenticated metadata channel, then sends table and FK definitions in one typed create request.
- Storage validates `REFERENCES` authority and atomically publishes FK records, target dependencies, and referenced-table write-epoch changes alongside the new table.
- Authenticated TCP and storage regressions passed. Locked workspace build, formatting, warning-denied all-target Clippy, TinyCC SDK tests and sandbox fixture build, and the complete serial delegated-cgroup workspace suite passed with zero ignored tests; `git diff --check` passed.


## SDK: ALTER COLUMN SET/DROP DEFAULT SQL — implemented

- Added an authenticated SQL helper that resolves the relation and column metadata, binds a context-typed default literal, and calls the existing typed set/drop operation. Existing rows are not rewritten.
- Parser/binder, authenticated TCP, CLI recognition, locked workspace build, formatting, warning-denied all-target Clippy, TinyCC SDK tests/fixture, and complete serial delegated-cgroup workspace suite passed with zero ignored tests; `git diff --check` passed.


## SDK: DROP CONSTRAINT SQL for generated key/check/FK IDs — implemented

- `execute_drop_constraint_sql` now resolves the generated ID within the selected table and dispatches to the typed PK, UNIQUE, CHECK, or FK drop operation.
- The primary-key authenticated TCP regression observed the prior rejection before implementation and now verifies typed PK drop dispatch. Existing FK drop remains covered. Focused TCP/CLI tests, locked build/fmt/Clippy, TinyCC checks, and the full serial delegated-cgroup suite passed with zero ignored tests; `git diff --check` passed.

## SDK: CREATE INDEX administrative SQL — implemented

- Added `AdminClient::execute_create_index_sql` to resolve qualified schema/table names and ordered columns from authenticated metadata, then invoke the existing typed index-create operation.
- Parser, authenticated TCP argument-binding, and CLI syntax-recognition regressions passed. Locked workspace build, format check, warning-denied all-target Clippy, maintained TinyCC SDK tests/fixture, complete serial delegated-cgroup workspace suite (sandbox/process tests enabled; zero ignored), and `git diff --check` passed; see the DDL component progress entry. DROP INDEX remains a separate follow-up requiring metadata index names.


## SDK: SQL DROP INDEX — implemented

- Authenticated administrative index metadata now exposes logical names. The SQL route resolves `DROP INDEX schema.name RESTRICT` across visible relations and pages, then dispatches the existing typed drop operation by stable generated ID.
- Parser, metadata decoding, authenticated TCP dispatch, and CLI recognition regressions passed. Focused parser, metadata decoding, authenticated TCP dispatch, CLI recognition, and process-level metadata lifecycle tests passed. Locked workspace build, format check, warning-denied all-target Clippy, maintained TinyCC SDK tests/fixture, full serial delegated-cgroup workspace suite (sandbox/process tests enabled; zero ignored), and `git diff --check` passed; see the DDL component progress entry.

## SDK: CREATE USER/ROLE administrative SQL — implemented

- Added a bounded parser/client helper for `CREATE USER name LOGIN|NOLOGIN` and `CREATE ROLE name`, dispatching the existing typed identity control operations. User creation does not issue a credential; NOLOGIN is enforced during token issuance.
- Parser, authenticated TCP typed-dispatch, and CLI pre-credential recognition regressions passed. Locked workspace build, format check, warning-denied all-target Clippy, maintained TinyCC SDK tests/fixture, full serial delegated-cgroup workspace suite (sandbox/process tests enabled; zero ignored), and `git diff --check` passed; see the identity DDL progress entry.
## ID-SQL-2: identity rename SQL

- Added `AdminClient::execute_rename_identity_sql` and routed `admin-sql` ALTER USER/ROLE RENAME to existing typed operations after exact authenticated name resolution. Parser, decoder, CLI pre-credential, and delegated gateway/supervisor process regressions pass; full workspace, Clippy, and TinyCC checks pass.

## ID-SQL-3: identity drop SQL

- Added `AdminClient::execute_drop_identity_sql` and routed `admin-sql` DROP USER/ROLE RESTRICT to the existing typed lifecycle. Parser and delegated gateway/supervisor process coverage pass, as do workspace build, format, Clippy, TinyCC SDK and fixture checks, and the full serial delegated-cgroup workspace suite.

## ID-SQL-4: user LOGIN state SQL

- Added `AdminClient::execute_set_user_login_sql` and routed `admin-sql` ALTER USER LOGIN/NOLOGIN to the existing typed mutation after name resolution. Parser and delegated process regressions pass, along with workspace build, format, Clippy, TinyCC SDK and fixture checks, and the full serial delegated-cgroup workspace suite.

## ID-SQL-5: role membership SQL

- Added `AdminClient::execute_role_membership_sql` and routed GRANT/REVOKE role membership syntax through name resolution to existing typed operations. Parser and delegated process regressions pass, along with workspace build, format, Clippy, TinyCC SDK and fixture checks, and the full serial delegated-cgroup workspace suite.

## ID-SQL-6: system privilege SQL

- Added `AdminClient::execute_system_privilege_sql` and routed bounded database privilege GRANT/REVOKE statements through identity name resolution to existing typed operations. Parser and delegated process regressions pass, along with workspace build, format, Clippy, TinyCC SDK and fixture checks, and the full serial delegated-cgroup workspace suite.

## DP-SQL-1: object-level grant SQL

- Added `AdminClient::execute_object_privilege_sql` for one schema/table object-level grant or revoke by SQL names. Column-level requests remain available through typed methods. Parser, production process, workspace build, format, Clippy, TinyCC and full delegated suite pass.

## DP-SQL-2: single-column grant SQL

- Added one-column SELECT/INSERT/UPDATE/REFERENCES grant resolution through authenticated `metadata.columns.list` and the existing typed column grant. Multi-column SQL remains pending. Parser and delegated process grant/revoke tests pass; workspace build, format, warning-denied Clippy, TinyCC SDK tests/fixture, and full serial delegated-cgroup workspace suite pass with sandbox/process tests enabled and zero ignored.

## DS-SQL-17: ownership transfer SQL — implemented

- Added AdminClient helpers that resolve schema/table names and a typed USER/ROLE identity name, then invoke existing `schemas.owner`/`tables.owner` mutations. Parser, delegated process, workspace build, format, Clippy, TinyCC, and full delegated suite checks pass.

## DP-SQL-3: bounded policy creation SQL — implemented

- Added `AdminClient::execute_create_policy_sql` to bind field names/ordinals, resolve recipient and `HAS_ROLE` names through authenticated identity metadata, and invoke `policies.create`. Parser, delegated process, workspace build, format, Clippy, TinyCC, and full delegated suite checks pass. The current resolver requires the corresponding identity-management privilege or database-admin membership; DROP policy SQL remains pending.

## Admin runtime supervisor queue summary — implemented

- `AdminClient::list_runtime_invocations_page` requests runtime-list version 3 and decodes the six-field supervisor queue aggregate. `prdb-client runtime-list` includes the same summary as JSON; fields are bounded counts and reserved upload bytes without identifiers.
- Typed pagination and decoding tests and the real gateway/supervisor process test pass. Workspace build, format, warning-denied Clippy, TinyCC SDK tests and sandbox fixture, and the full serial delegated-cgroup workspace suite pass with zero ignored tests.

## Admin runtime oldest queued START age — implemented slice

- Each queued START ticket records its monotonic queue-entry time. The identifier-free runtime summary reports the age of the oldest queued ticket, or zero for an empty queue; it does not affect scheduling.
- The field is exposed only in runtime-list protocol version 4 and decoded by the typed administrator client and CLI JSON projection.
- Focused staging-summary and client TCP paging tests passed. The delegated gateway/supervisor lifecycle also passed and verified the CLI output for an empty queue. Workspace build, format check, warning-denied all-target Clippy, TinyCC SDK tests/fixture, and the full serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests.

## Admin runtime relayed result bytes — implemented slice

- `AdminClient::list_runtime_invocations_page` requests runtime-list version 5 and decodes the supervisor's process-local result-payload byte counter. `prdb-client runtime-list` emits the same counter in JSON.
- Focused typed client and production gateway/supervisor CLI checks passed. The full serial delegated-cgroup workspace suite also passed with sandbox/process tests enabled; workspace format, build, warning-denied all-target Clippy, TinyCC SDK tests/fixture, and `git diff --check` passed.

## Admin runtime gateway client-write result bytes — implemented slice

- `AdminClient::list_runtime_invocations_page` requests runtime-list version 6 and decodes both supervisor-relayed and gateway-written result byte totals. `prdb-client runtime-list` emits both process-local counters.
- Focused typed page decoding and real gateway/supervisor CLI coverage passed, including a lower bound from the actual `QUERY_READY` payload. Workspace format, build, warning-denied all-target Clippy, TinyCC SDK tests/fixture, delegated cgroup probe, full serial delegated-cgroup workspace suite, and `git diff --check` passed. Cargo.lock is unchanged.

## AdminClient conditional table-drop SQL — verified

- Added `execute_drop_table_if_exists_sql`; resolved tables use the existing typed drop path, while unresolved schema/table names are suppressed only after an authenticated administrator-only runtime metadata check. Other failures propagate.
- Parser and gateway/supervisor process coverage include the administrator no-op and non-administrator denial. Workspace build, format, warning-denied Clippy, TinyCC SDK tests/fixture, delegated cgroup probe, and full serial delegated-cgroup suite passed; see [DDL progress](../ddl-schema/PROGRESS.md). Cargo.lock is unchanged.

## AdminClient conditional schema-drop SQL — verified

- Added `execute_drop_schema_if_exists_sql`; resolved schemas follow the existing typed drop path, and absence is suppressed only after an authenticated database-administrator check. The focused parser and delegated process regressions, workspace build, format, warning-denied Clippy, and full serial delegated-cgroup workspace suite passed; see [DDL progress](../ddl-schema/PROGRESS.md). Cargo.lock is unchanged.

## AdminClient object-grant SQL identity scope — implemented slice

- `execute_object_privilege_sql` now resolves the schema/table and recipient together through the scoped endpoint. Owners and `MANAGE_GRANTS` holders do not need global user/role administration or ordinary schema/relation visibility to target object grants. The server-side grant mutation remains authoritative.
- Workspace build, formatting, warning-denied all-target Clippy, and `git diff --check` passed; `Cargo.lock` is unchanged. Focused client/process regressions remain to be added; no test suite was run for this increment.
