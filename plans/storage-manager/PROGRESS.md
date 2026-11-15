# Storage Manager Progress

## SM-2 follow-up: bounded ordered index range pages — storage primitive verified

- Contract: [index-key encoding v1](../../specs/index-key-encoding.md). A pinned redb snapshot accepts inclusive/exclusive tuple bounds for one nonzero index ID, returns ordered validated locators in pages capped at 1,024, and resumes strictly after an in-range continuation key. The row UUID suffix is included or excluded with the complete tuple boundary.
- Red/green: the first focused test failed to compile because the range-bound type and snapshot API were absent. Two real-redb regressions now cover inclusive bounds across pages, exclusive boundaries, an empty inverted range, and rejection of a continuation key from another index.
- Implementation: `RowSnapshot::scan_index_range` constructs prefix-aware lower/upper search bounds, caps candidate count, validates persisted locator/key agreement, and rejects cursors outside the index/range. It returns candidate locators only; it grants no authorization or executor capability.
- Verification: `nix develop --no-update-lock-file --no-write-lock-file -c cargo test -p prdb-engine --lib snapshot::tests:: --locked --offline -- --nocapture` passed (2 passed, 0 failed, 0 ignored).
- Remaining: transaction-layer MVCC/overlay reconciliation, authorized row/cell projection, executor IPC, planner hinting, SDK execution, and optimized/reference result and redaction-error equivalence are not implemented by this storage primitive.

## SM-2 follow-up: catalog-authorized range reconciliation — verified

- Contract: [index-key encoding v1](../../specs/index-key-encoding.md). A leading-column range over a pinned catalog index must validate every key column's unconditional readability, verify snapshot locators and derived index keys, merge the private overlay, and return only row-authorized sanitized projections. A bounded path must return `Unavailable` instead of partial results.
- Red/green: the maintained transaction integration regression initially failed to compile because the catalog range API and result records were absent. The regression now exercises a non-unique index, hidden and conditional key denial, staged key movement, staged deletion, and duplicate staged values through a separate non-unique index.
- Implementation: `InvocationTransaction::scan_catalog_index_range` validates the pinned schema/index, scans bounded snapshot pages, checks row versions and full derived keys, reconciles inserts/updates/deletes, filters NULL leading keys, caps candidate count and accounted bytes, then applies the standard authorized row/cell path. It falls back when the selected path is unsafe or too broad.
- Verification: the focused transaction integration regression passed (`1 passed, 0 failed, 0 ignored`). Locked workspace build, format check, warning-denied Clippy, and full serial workspace tests passed inside the delegated `systemd --user` service; the library target reported 192 passed, commit integration 148 passed, metadata process integration 2 passed, and transaction integration 74 passed, all with zero ignored. The maintained TinyCC suite and sandbox query fixture build also passed in this work session.
- Remaining: executor IPC, planner hinting, SDK execution, and optimized/reference result and redaction-error equivalence remain open.

## SM-1 follow-up: retained-reader reclamation after release — verified

- Contract: [redb storage](../../specs/redb-storage.md). redb's compactor must refuse while an old read snapshot is pinned; after the last reader closes, an explicit compaction must reclaim obsolete copy-on-write pages without losing live data.
- Regression: [`retained_reader_blocks_compaction_until_release_then_reclaims_deleted_pages`](../../engine/tests/redb_foundation.rs) retains 64 100-KiB rows, deletes 58 while the snapshot is live, verifies the old snapshot still reads deleted rows, and confirms compaction returns `TransactionInProgress`. After dropping the reader, compaction reduces both the file size and `DatabaseStats::allocated_pages`; all six surviving rows remain intact.
- Verification: all four `redb_foundation` integration tests passed with redb 4.4.0 on x86_64 Linux using Immediate durability and two-phase commit. Workspace build, format check, warning-denied Clippy, and full serial delegated-cgroup workspace tests passed; commit integration reported 142 passed/0 ignored and transaction integration 74 passed/0 ignored.
- Remaining: measure operational contention during compaction, add service-level maintenance scheduling if the redb ownership model supports it, and test larger database shapes and target filesystems.

## SM-1 follow-up: managed compaction entry point — covered

- Contract: `StorageInstance::compact` makes one explicit redb compaction attempt while the caller holds the exclusive instance handle. The trusted caller owns scheduling and retry policy; compaction does not alter logical commit state.
- Regression: `ready_storage_instance_can_run_explicit_compaction` constructs a validated READY instance, creates and drops its storage manager, then invokes compaction through the still-locked instance. The redb foundation test continues to verify that active snapshots block compaction and released snapshots permit physical page reclamation.
- Verification: the focused locked/offline bootstrap target passed (`12 passed, 0 failed, 0 ignored`). The full serial workspace suite also passed inside the delegated `systemd --user` service; `cargo build --workspace --locked --offline`, `cargo fmt --all -- --check`, and warning-denied workspace Clippy passed.
- Scope: managed compaction scheduling, latency, operational contention, larger database shapes, and other target filesystems remain open.

## SM-1 follow-up: operator-facing offline compaction — implemented

- Behavior: `prdb-maintain compact --instance-dir PATH` opens the validated READY instance, holds its exclusive lock through one compaction attempt, and reports whether redb compacted plus elapsed time. If a supervisor or another owner holds the instance lock, the command fails before touching the database.
- Regression: `maintenance_compaction_requires_exclusive_instance_lock` invokes the real binary, verifies it fails while the fixture's READY `StorageInstance` owns the lock, then succeeds after that instance is dropped and confirms the database can reopen.
- Operational guidance: the systemd README lists the binary for user/system installations and gives the stop-maintain-start sequence.
- Verification: focused locked/offline CLI test passed; the `bootstrap_binary` target passed (`5 passed, 0 failed, 0 ignored`). Workspace build, format check, warning-denied Clippy, and full serial delegated workspace suite passed; the suite reported 188 library tests and 145 commit tests, with zero ignored in those targets.
- Scope: this is offline maintenance. The supervisor does not schedule compaction while serving traffic; online contention, larger profiles, and target filesystems remain open.

## SM-1 follow-up: compaction after mixed storage pressure — measured

- Harness: [`storage_pressure.rs`](../../engine/examples/storage_pressure.rs) now measures `StorageInstance::compact` after the bounded workload completes, its retained reader aborts, and the `StorageManager` is dropped. It records compaction duration and database file size before and after reclaiming redb history.
- Result: five sequential fresh release runs on the local x86_64 Linux profile compacted 28.3–37.8 MB databases to 0.40–0.46 MB. Compaction latency ranged 12.937–17.152 ms (median 15.337 ms). These runs used four writers, 256 durable commits, retained authorized scans/point reads, and bounded outcome pruning before the quiescent compaction phase.
- Verification: all five `cargo run --release --locked --offline --example storage_pressure` executions completed; locked workspace build, format check, warning-denied Clippy, and the full serial workspace suite passed inside the delegated service. The suite reported 188 library tests and 145 commit integration tests, with zero ignored in those targets.
- Scope: compaction occurs after operational and analytical work is quiescent because redb 4.4 requires exclusive mutable access to the database handle. This does not measure contention during compaction or implement service scheduling; larger database shapes and other filesystems remain open.

## SM-3 follow-up: catalog-derived authorized point reads — verified

- Behavior: `InvocationTransaction::read_catalog_authorized_row` takes only a table ID and row ID, loads the pinned schema, role closure, grants, and policies internally, and returns the row through the existing VALUE/NULL/REDACTED projection path. Zero table or row IDs abort with `InvalidContext`; absent or row-filtered rows return `None`.
- Regression: the catalog-authorized scan fixture now verifies an allowed point read returns the expected value and an absent row remains `None`. The confined SQL workload harness exercises this API across baseline and live-analytics phases.
- Verification: the focused real-redb transaction regression, five delegated-cgroup latency runs, workspace build, formatting, warning-denied Clippy, full serial delegated workspace tests, and `git diff --check` passed.

## SM-2: persisted index scan matches the row reference scan — verified

- Contract: persisted index entries must identify exactly the committed rows in a reference table scan, preserve the indexed value order, point to the current row version, and rederive from the tuple.
- Red/green: the persisted index integration fixture first failed to compile because `RowSnapshot::scan_index_prefix` was absent. The new bounded prefix reader validates each ILC1 locator, rejects malformed prefixes/cursors/keys, limits candidate count and copied key bytes, and resumes strictly after the complete last key. The real-redb fixture pages candidates, confirms EOF, checks each locator against the pinned row version, rederives each key, and compares the naturally sorted results with a separate row scan. It also retains the old index snapshot across an indexed-value update and proves the old snapshot sees its old membership/version while a fresh snapshot sees the removed and new memberships.
- Verification: focused persisted scan and composite-key tests pass. After adding the reader and retained-snapshot assertion, `cargo build --workspace --locked`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo test --workspace --locked` all pass in the configured Nix development shell. The commit integration target reports 129 passed and three cgroup-dependent tests ignored.
- Remaining: transaction-layer overlay reconciliation, catalog-selected index access, authorized row/cell projection, and authorization-error parity remain open; this fixture validates persisted candidate membership only.

## SM-2 dependency: authorized unique equality probe — partial

- Contract: a unique, non-NULL equality probe may use a catalog-loaded index only when the indexed cells are unconditionally readable. Candidates must resolve against the same pinned row snapshot and private overlay, and pass the authorized row/cell projection before being returned.
- Red/green: `unique_constraint_uses_its_backing_index_for_invocation_writes` first failed to compile because the authorized probe API was absent. It now verifies a persisted candidate, a staged value moving off and onto the key, a staged tombstone suppressing the snapshot candidate, and a duplicate staged key causing `Unavailable` so the caller can use the reference scan. Hidden key grants and conditional cell policies also return `Unavailable` without probing.
- Implementation: `probe_catalog_unique_index` validates the pinned schema and loaded unique-index definition, checks locator row versions, bounds snapshot and overlay work, derives staged keys, resolves the final candidate through `read_authorized`, and returns only sanitized cells. `probe_catalog_unique_index_for_principal` builds its projection from the pinned catalog. The projection-taking helper remains a trusted engine API.
- Verification: the focused transaction regression and full workspace `cargo test --workspace --locked` pass; `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets --locked -- -D warnings` pass in the configured Nix shell. The workspace run reports 129 transaction tests passed and three cgroup-dependent tests ignored.
- Remaining: no executor/optimizer route calls this probe yet; non-unique and range scans, paginated candidate production, richer safe-cell-policy analysis, and authorization-error equivalence across reference versus optimized SQL remain open.

## SM-6 dependency: C executor stage-delete bridge — partial

- Contract: [IPC protocol](../../specs/ipc-protocol.md) mutation payload v1 and [client SDK](../../specs/client-sdk.md). A C query can submit a connection-local kind-8 row handle for deletion and must accept only the version/status-0 acknowledgement. Authority and row-version checks remain in trusted storage.
- Red/green evidence: the maintained executor fixture first failed to compile because the operation constant and SDK function were absent. The bridge now encodes version plus the 16-byte handle, calls operation `0x1112`, and rejects malformed acknowledgements and non-row tokens.
- Verification: `nix-shell -p rustc cargo clippy rustfmt tinycc --run 'make -f engine/sdk/Makefile test'` passed with TinyCC 0.9.27-unstable-2025-01-06. Base PATH lacked the toolchain; initial Nix access was denied by the sandbox and the authorized Nix shell command succeeded.
- Limits/next: this is only the delete bridge; INSERT and UPDATE C encoders are still absent. The test uses a host callback fixture and does not claim a live IPC/storage or sandbox integration. Next implement full-shape INSERT with VALUE/NULL/DEFAULT encoding and response handle validation.

## SM-6 dependency: C executor stage-insert bridge — partial

- Contract: [IPC protocol](../../specs/ipc-protocol.md) mutation payload v1 and [client SDK](../../specs/client-sdk.md). INSERT sends every declared field in schema order; each is VALUE, NULL, or DEFAULT, and successful storage returns a connection-local row handle. Storage still derives defaults, permissions, constraints, and indexes from its pinned catalog.
- Red/green evidence: the executor fixture first failed to compile because the INSERT input record and bridge were absent. It now compares the exact request bytes for a typed INTEGER 42, NULL, and DEFAULT, rejects a short caller buffer before IPC, and rejects a response with the wrong handle kind without publishing it to the caller.
- Verification: `nix-shell -p rustc cargo clippy rustfmt tinycc --run 'make -f engine/sdk/Makefile test'` passed with TinyCC 0.9.27-unstable-2025-01-06. This is an SDK callback fixture, not live storage IPC.
- Limits/next: UPDATE's C bridge and end-to-end sequential SQL mutation execution remain open. Next add UPDATE patch encoding with unique ordinal checks and ack validation; later SQL-4 needs fixed target materialization before staging writes.

## SM-6 dependency: C executor stage-update bridge — partial

- Contract: [IPC protocol](../../specs/ipc-protocol.md) mutation payload v1 and [client SDK](../../specs/client-sdk.md). UPDATE sends one row handle and unique zero-based ordinals, each with VALUE or NULL; DEFAULT and REDACTED are invalid. The existing Rust dispatcher and transaction path enforce row authority, old/new update policy checks, and snapshot/conflict rules.
- Red/green evidence: the executor fixture first failed to compile because the patch record and bridge were absent. It now verifies exact bytes for a NULL patch and an INTEGER 99 patch, rejects a non-success acknowledgement, duplicate ordinal, out-of-range ordinal, and undersized request without another host callback.
- Verification: `nix-shell -p rustc cargo clippy rustfmt tinycc --run 'make -f engine/sdk/Makefile test'` passed with TinyCC 0.9.27-unstable-2025-01-06 after correcting fixture ordinal/value offsets found by the byte comparison.
- Limits/next: low-level C insert/update/delete bridges now exist, but SQL still does not produce mutation requests and no live SDK-to-storage mutation stream is tested. Next close the SQL execution gap with fixed target materialization and sequential own-write visibility, retaining statement-error abort behavior.

## Signed index components — in progress

- Contract: [index-key encoding v1](../../specs/index-key-encoding.md), SQL tuple components and normative examples. Non-null signed 16/32/64-bit values use marker `01`, sign-bit normalization, and big-endian bytes. These bytes are comparable only within the same index definition.
- Red evidence: `cargo test --test index_key` ran against compiling zero-filled placeholder functions and both the example and numeric-order assertions failed. The placeholder was then replaced.
- Implementation: the Rust workspace encodes these three component widths. This is a partial codec, not an index or storage implementation.
- Verification: on x86_64 NixOS, Rust 1.95.0 and Cargo 1.95.0, `cargo build --workspace --locked`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked`, and `cargo build --workspace --release --locked` passed. Two integration tests cover spec examples and selected i64 ordering boundaries.
- Limitations at that increment: no decoder, remaining component types, tuple validation, length limit, redb integration, or scan equivalence tests existed. The ordering test is not exhaustive. TinyCC 0.9.27-unstable-2025-01-06 was available for inspection, but no SDK or C test exists and its build and target profile are not pinned or validated.

## SM-1: redb foundation — in progress

- Pinned redb 4.4.0 and its transitive dependency in `Cargo.lock`; redb declares Rust 1.90 minimum and MIT OR Apache-2.0 licensing. The workspace uses Rust 1.95.0.
- Disposable real-redb tests check explicit Immediate/two-phase commit followed by reopen, explicit abort, retained snapshot visibility after a later commit, and unsigned lexicographic byte-key iteration. These are API experiments, not an engine adapter or a crash/power-loss test. Local filesystem: ext4 at `/tmp`, x86_64 NixOS.
- `cargo test --test redb_foundation --locked` passed both experiments; the workspace build, formatting check, Clippy, tests and release build also passed with this dependency.
- Reconciled stale custom-page index and catalog text with the redb table mapping. Logical value encodings, limits, migration and production filesystem policy still need decisions and tests.

## SM-2: variable-length index components — in progress

- Contract: index-key v1 non-null BYTEA escaping and prefix bounds. A compiling placeholder produced an assertion failure for the empty-value encoding; after implementation, tests cover empty and embedded-zero bytes, prefix ordering, decoding a component from a longer input, invalid markers/escapes and truncated inputs. A separate placeholder failed the finite successor example before implementation.
- This is not a complete key codec: nullable and typed component validation, limits, tuple decoding, and comparator properties remain. Callers must bound input and validate UTF-8 before encoding text.

## SM-2: fixed-width index components — in progress

- Added Boolean, UUID, and signed 128-bit coefficient encoding/decoding and signed 16/32/64-bit decoding. The focused test first failed against compiling placeholders, then passed. It checks decimal examples, integer extremes, malformed booleans, truncated payloads, and markers. Decimal precision/scale and complete-tuple validation belong to a schema-aware layer that does not yet exist.

## SM-2: bounded persisted index keys — implemented

- Contract: [index-key encoding v1](../../specs/index-key-encoding.md). Complete SQL secondary-index keys are capped at `2 * MAX_ROW_RECORD_BYTES + MAX_COLUMNS + 32` (2,101,280 bytes at the current limits), sufficient for worst-case escaping and fixed prefixes while bounded well below redb 4.4.0's 3 GiB key limit.
- Regression: a compiling real-redb commit test first accepted a key one byte above the proposed limit and committed its row. It now verifies rejection as `InvalidIndex`, unchanged commit sequence, and no row publication.
- Implementation: transaction staging and the serialized storage commit both reject over-limit keys, so direct trusted commit callers cannot bypass the logical bound. The low-level key composer remains an encoding helper and may construct bytes that cannot be persisted.
- Verification: the focused real-redb regression passed both the over-limit rejection and exact-limit acceptance cases. The full TinyCC suite and `cargo build --workspace --locked`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked`, and `git diff --check` all passed in the configured Nix shell. The commit integration target passed all 106 tests.
- Remaining: persisted index-scan equivalence remains open; independent composite/null/suffix coverage is recorded above.

## SM-2: typed component ordering evidence — verified

- Contract: [index-key encoding v1](../../specs/index-key-encoding.md). Within one index definition, encoded component bytes must preserve the declared scalar's SQL order.
- Regression: the first test run was rejected during setup because `i128::MIN` exceeds `DECIMAL(38)` precision. Replacing it with signed 38-digit values made the focused property pass.
- Evidence: the integration regression independently sorts representative BOOLEAN, signed integer, DECIMAL(38), TEXT, VARCHAR, BYTEA, UUID, DATE and TIMESTAMPTZ values by their native Rust order, then checks the encoded key bytes increase strictly.
- Verification: all 11 tests in `cargo test -p prdb-engine --test index_key --locked` passed; `cargo fmt --all -- --check`, workspace Clippy with warnings denied, and `git diff --check` also passed. The regression adds ordering evidence only; it changes no runtime implementation.
- Remaining: additional edge-property cases and persisted index-scan equivalence remain open.

## SM-3: bounded private overlay primitive — in progress

- Contract: [memory storage](../../specs/memory-storage.md), Transaction Memory. A compiling placeholder failed both staging and limit tests; the implementation now copies staged values, counts distinct row IDs and encoded value bytes, and clears and terminally aborts after a limit failure. Replacement and deletion update accounting. Tests also check independent overlays and explicit abort.
- This is a foundational container, not an authorized transaction: it has no snapshot reads, field patches, row versions, schema/policy checks, commit, or outcome integration. The limits do not cover map overhead or invocation-wide budgets. Do not attach it to executor requests as a write path.

## SM-3: own-write read resolution — in progress

- Contract: [concurrency/MVCC](../../specs/concurrency-mvcc.md), Version Visibility, and [memory storage](../../specs/memory-storage.md), Transaction Memory. Staged values shadow the invocation's pinned snapshot; staged deletes hide a snapshot row; absent overlay entries read from that snapshot. The resolver accepts the snapshot lookup from its caller, so this primitive does not own or refresh a redb transaction.
- Added a focused integration test covering snapshot fallback, updated value visibility, delete visibility, inserted value visibility, and unchanged snapshot data.
- Red/green evidence: Cargo could not resolve `redb` because crates.io is unreachable, both online and offline (`cargo test -p prdb-engine --test overlay --locked` failed to fetch index config; `--offline` reported no cached package). Compiling the actual overlay module with the actual integration test file as a standalone `rustc --test` harness first exposed a borrow-lifetime compile error and then a test quota mistake; after correcting both, all five overlay tests passed.
- Implementation: `Overlay::read` resolves overlay-first and invokes the snapshot closure only for an unstaged row. Its result distinguishes overlay and snapshot borrows so a short-lived snapshot read does not unnecessarily hold the overlay borrow across subsequent mutation. This preserves the existing boundary: the overlay alone does not authorize reads or writes and is not an executor write path.
- Remaining evidence: run Cargo focused test, formatting, Clippy and workspace checks when crates.io or a local redb source becomes available. No redb-pinned snapshot adapter or authorization path exists yet.

## SM-3: retained redb row snapshot — in progress

- Contract: [redb storage](../../specs/redb-storage.md), Invocation Transactions, and [memory storage](../../specs/memory-storage.md), redb Ownership. An invocation snapshot owns a redb read transaction; reads copy one encoded row value out of its guard, and own-write overlay resolution takes precedence. A later commit must not change what the retained snapshot returns.
- Added `RowSnapshot` over the named `rows_v1` byte table and a real-redb integration fixture using the documented 32-byte table-ID/row-ID key shape. The test checks copied snapshot reads, overlay replacement/deletion precedence, and old-versus-current visibility across a subsequent write.
- Verification: the current exact Rust 1.95 compiler and the prebuilt redb 4.4.0 artifact in `target/debug/deps` compiled the engine and the actual `overlay.rs` and `snapshot.rs` integration test files directly. All five overlay tests and the real-redb retained-snapshot test passed. Rustfmt was applied to changed Rust files; `git diff --check` passed.
- Cargo remains unavailable for dependency resolution: crates.io cannot be reached and redb 4.4.0 source/index data are not cached. Direct rustc linkage verifies these focused tests, not Cargo metadata, Clippy, or the workspace suite.
- Remaining: snapshot admission/context, key codec enforcement, authorization, bounded reader admission/age, statement stream rules, row versions/conflict validation, and final atomic commit are not implemented. This is still a foundational test fixture, not an executor-accessible transaction path.

## SM-1: versioned engine table schema — in progress

- Contract: [redb storage](../../specs/redb-storage.md), Database Layout. Added definitions for the initial seven tables (`engine_meta_v1`, `rows_v1`, `indexes_v1`, `catalog_v1`, `outcomes_v1`, `audit_v1`, and `stats_v1`), plus one helper that creates them in the caller's write transaction. The later index-maintenance increment added `row_indexes_v1` as an eighth table.
- A real-redb test commits the table set and metadata fixture, closes and reopens the database, then opens every named table and reads the metadata value.
- Verification: Rust 1.95 direct compilation against the existing redb 4.4.0 artifact succeeded; the schema reopen test passed. The direct method is needed because Cargo cannot reach crates.io or resolve uncached redb source. The named tables use byte-slice key/value definitions only; versioned payload codecs and the index table's final key mapping remain open.
- Remaining: catalog/row/metadata/outcome/audit codecs, schema-aware limits, initialization validation and tests for partially aborted bootstrap remain. This does not complete SM-1.

## SM-1: engine metadata codec — in progress

- Contract: [redb storage](../../specs/redb-storage.md), Database Layout, and [catalog lifecycle](../../specs/catalog-lifecycle.md), Version Domains. ENG1 is a fixed 46-byte record with a nonzero database UUID, independent catalog/expression/built-in-semantics versions, commit sequence, explicit initialization lifecycle, and zero reserved bytes.
- Added canonical encode/decode, exact-length/version/lifecycle/reserved/UUID/version rejection, hand-checked endian fixture, and integration with the `engine_meta_v1` bootstrap table fixture. The redb schema test now round-trips the encoded record through commit and reopen.
- Red/green evidence: direct Rust 1.95 compilation initially found that `MetadataError` could not propagate through test results; it now implements `Display`/`Error`. Two metadata codec tests and the real-redb schema/reopen test pass against the locally available redb 4.4.0 artifact. Rustfmt and whitespace checks are run after the final edit.
- Remaining: this format records versions but does not implement startup compatibility policy or bootstrap recovery. Row/catalog/index/outcome/audit payload codecs and schema/value limits remain open; normal Cargo, Clippy and workspace validation remain blocked by unavailable crates.io package data.

## SM-1 / SM-3: versioned row envelope — in progress

- Contract: [redb storage](../../specs/redb-storage.md), Database Layout, and [concurrency/MVCC](../../specs/concurrency-mvcc.md), Version Visibility. RWR1 stores a nonzero row version and an exact-length schema-aware payload inside a 1 MiB maximum record. Successful commits will assign their commit sequence as the row version; this increment defines the value contract, not the commit path.
- Added bounded encode/decode with magic/version/flags checks, checked length arithmetic, exact payload framing and maximum-size enforcement. Added canonical, malformed and boundary fixtures. The real-redb snapshot test now stores RWR1 rows and confirms old snapshot and newer committed row versions remain distinct while the overlay exposes its staged value.
- Verification: all focused row-record and real-redb snapshot tests passed when compiled directly with Rust 1.95 and the available redb 4.4.0 artifact. Cargo dependency resolution, Clippy and workspace checks remain unverified due unavailable crates.io/source cache.
- Remaining: default materialization, schema migration compatibility, row-local constraints, authorization, index maintenance and required audit are not implemented. Cargo-based verification remains unavailable.

## SM-4: atomic row commit and conflict check — in progress

- Contract: [concurrency/MVCC](../../specs/concurrency-mvcc.md), Commit Protocol, and [physical metadata](../../specs/physical-metadata.md). The storage commit coordinator opens one Immediate/two-phase redb write transaction, requires READY metadata, checks unique invocation and row keys, compares every current row version with the staged expected version, assigns one new engine commit sequence, and publishes row records, metadata, and a COMMITTED outcome atomically.
- Added OUT1 fixed-width outcome encoding for COMMITTED and ABORTED states. Successful row commits publish COMMITTED atomically; proven row conflicts and statement failures after invocation admission publish a durable ABORTED outcome after all private writes are discarded. Invalid identities, sequence/status combinations, retention values and lengths are rejected.
- The real-redb commit test inserts then updates two rows, confirms both receive one sequence and outcome, then submits a stale multirow write and confirms Conflict leaves both latest row payloads and the commit sequence unchanged while persisting ABORTED for the rejected invocation.
- Verification: changed engine and integration tests were compiled with Rust 1.95 against the available redb 4.4.0 artifact; the complete suite at that point passed all 26 tests across 10 integration files. Rustfmt `--check` and `git diff --check` passed. `cargo test --workspace --locked` failed at crates.io index configuration fetch, before compiling project code.
- Remaining: transaction admission through an authenticated identity path, constraints, index maintenance, required audit, cancellation barrier, and commit ambiguity resolution are absent. Principal-bound lookup is implemented at the storage helper level, but authenticated IPC/status endpoint integration is not. Cargo/Clippy/workspace checks still require crates.io source resolution.

## SM-3 / SM-4: invocation snapshot, overlay, and finalization slice — in progress

- Added `InvocationTransaction` to hold one redb snapshot, bounded private overlay, expected base row versions, and invocation outcome context. Reads return the most recent private value/delete before falling back to the snapshot. Staging captures the base version once; statement errors abort, clear earlier staged writes, and record ABORTED; insert-then-delete removes the staged row and finalizes as an outcome-only commit.
- Its finalization calls the serialized row commit path, which validates latest row versions and atomically writes changed rows, metadata sequence, and COMMITTED outcome. The API takes invocation/principal/retention values directly and currently has no trusted admission or authorization caller; do not expose it to executor requests yet.
- Real-redb tests cover own-write visibility, snapshot fallback, another invocation committing a disjoint row while the snapshot is retained, stale same-row abort, staged deletion visibility, and no-op insert/delete outcome.
- Admission now reads metadata and outcomes from the pinned snapshot, rejects non-READY databases, validates invocation/principal/deadline fields, and rejects an already-used invocation ID. The authenticated source of that identity context is still outside this slice.
- A barrier-coordinated two-thread same-row race now verifies both invocations stage from one snapshot before finalization; exactly one commits, the other records ABORTED, and the data sequence advances once.
- Final verification after this increment: Rustfmt `--check` passed for all engine sources/tests and all 31 tests across 11 integration files passed when directly compiled against the cached Rust 1.95/redb 4.4.0 artifacts. `git diff --check` passed.
- Added `lookup_outcome`, which reopens durable COMMITTED/ABORTED records by invocation ID, returns them only to the recorded principal, and treats expired/missing/other-principal records as absent. A real-redb test verifies lookup after close/reopen, wrong-principal filtering, expiry, and missing outcome.
- Remaining: bounded snapshot age/count, identity authentication beyond caller-supplied context, cursor lifecycle, authorization/policy epoch revalidation, constraints, indexes, required audit, cancellation/commit barrier, authenticated IPC status endpoint, and recovery ambiguity.

## SM-1 / SM-4: typed tuple payload and patch merge — in progress

- Contract: [redb storage](../../specs/redb-storage.md), TUP1/Scalar encoding, [schema DDL](../../specs/ddl-schema.md), and [authorization](../../specs/authorization.md), Mutation Permissions. TUP1 binds typed field bytes to a pinned schema version, validates scalar/null/range constraints, and applies a set of field patches atomically while retaining every unchanged value.
- Fixed scalar storage encodings for all initial SQL types, DATE years 0001–9999 and UTC microsecond timestamps over the matching Gregorian range. TUP1 is bounded by the RWR1 payload budget, has exact field framing, and rejects malformed/noncanonical states. Text/byte values are length-checked before copying into the encoded tuple.
- `InvocationTransaction::stage_patch` decodes the pinned row tuple, applies typed patches, and stages the merged tuple while preserving unmodified fields. A real-redb transaction test verifies this across final commit; pure codec tests cover all scalar families, null, malformed headers/states, precision, date range, patch atomicity and exact size limit.
- Verification for this increment: all 37 tests across the 12 integration files passed when directly compiled with Rust 1.95 against the available redb 4.4.0 artifact. Rustfmt `--check` for all engine files and `git diff --check` passed. Full Cargo workspace/Clippy validation remains unavailable because crates.io index/source resolution fails before project compilation.
- Limits: callers must supply the snapshot's trusted column schema and perform per-column read/write authorization before patch requests; those guarantees are not implemented here. SQL text/parameter parsing, default materialization, row-local constraints, indexes, schema migration and authenticated executor wiring remain open.

## SM-2 / SM-4: typed index keys and atomic locator maintenance — in progress

- Contract: [index-key encoding v1](../../specs/index-key-encoding.md), SQL tuple components and Complete Keys and Values, plus [redb storage](../../specs/redb-storage.md), Database Layout and Invocation Transactions. Typed SQL tuples are validated against the same column/null/range rules as TUP1, then encoded with ordered key bytes; complete keys concatenate index ID, tuple, and row UUID.
- Defined ILC1 locator bytes (32 bytes: magic, LE version, reserved zeros, row UUID, nonzero LE row version), exact decoder validation, and a key-aware decoder that rejects a locator whose UUID disagrees with the key suffix. `commit_rows_and_indexes` checks the current locator versions and applies supplied index adds/removes with row writes and outcomes in the same Immediate/two-phase redb transaction. A stale locator conflicts and leaves both row and index state unchanged while recording ABORTED.
- Red evidence: locator integration test initially failed to compile because its typed decode error did not implement `std::error::Error`; added the standard error implementation. The direct real-redb suite then exposed no remaining failures.
- `InvocationTransaction::stage_index_membership` now stages key membership against the pinned locator version and passes those mutations to final commit. Its test inserts and indexes a row, then deletes and unindexes it under consecutive commit sequences.
- `RowSnapshot` now reads registered CAT1 records by canonical key from its retained redb snapshot, copies and validates them, and rejects malformed payload/key mismatches. Index staging has separate bounded key-byte accounting and a cap of 256 distinct key changes per staged row; budget overflow aborts the whole invocation and records ABORTED.
- Verification: direct Rust 1.95 compilation against the available redb 4.4.0 artifact passed all 55 integration tests across 14 files. Direct `rustfmt --check` for all engine sources/tests and `git diff --check` passed. Direct `clippy-driver -D warnings` passed for the engine library and all integration targets after fixing the warnings it found. Cargo test/clippy could not resolve redb because crates.io `config.json` was unreachable; the installed Cargo has no `fmt` subcommand.
- Limits: invocation staging still lacks catalog-derived complete index definitions; uniqueness semantics are caller supplied; PK null rejection, foreign-key/check constraints, catalog secondary indexes and graph/authority validation, and authorization dependency derivation remain unimplemented. Generic catalog read guards now revalidate explicitly read records, but do not establish that every relevant authority/schema record was read. Cancellation/commit ambiguity handling, production outcome status IPC, and gateway/sandbox wiring remain unimplemented. Do not expose the low-level commit API to executor requests.
- Adjacent dependency: a shared schema-driven CAT1 envelope codec now validates typed catalog TLVs, includes field registries for all 16 catalog families, and preserves the existing policy-target fixture. It does not yet provide catalog persistence, semantic definitions, index lookup, or transactional DDL.
- The unified `commit_storage_changes` uses one `CommitRequest` to publish rows, indexes, catalog records, AUD1 audit values, metadata sequence, and outcome in one redb transaction. Catalog changes require a successful administrative audit event for the same principal. Unique index additions are checked against current state plus the final staged additions; NULLS DISTINCT exemptions are caller supplied.

## SM-4 / SM-5: explicit catalog read-set revalidation — in progress

- Contract: [data dictionary](../../specs/data-dictionary.md), Catalog Reads and Commit Validation. `InvocationTransaction::read_catalog_record` copies and validates one CAT1 record from its retained snapshot and captures its exact bytes; missing lookups capture an absence guard. Final write validation compares every captured guard with the current catalog inside the same Immediate/two-phase redb transaction that would publish rows, indexes, metadata, and outcome.
- A changed record or a record created after an absent lookup causes a conflict, persists ABORTED, and publishes no staged row. Both the changed-record and phantom-record cases have real-redb integration coverage.
- Verification: Rust 1.95 directly compiled and ran all 57 tests across 14 integration targets against the available redb 4.4.0 artifact. Direct `clippy-driver -D warnings` passed for the engine library and all 14 targets; rustfmt `--check` and `git diff --check` passed. `cargo test --workspace --locked` was attempted and failed before compilation because `https://index.crates.io/config.json` was unreachable. The configured Cargo binary is present in the Nix store but not on the shell PATH.
- Limits: this provides a generic guard mechanism, not dependency discovery or authorization. Callers must still identify every schema/authority epoch that affects the write. Full catalog/bootstrap integrity, DDL fences, constraint validation, row/cell policy enforcement, and the service integration stages remain outstanding.

## SM-5: record-level startup integrity — in progress

- Contract: [catalog lifecycle](../../specs/catalog-lifecycle.md), Startup and Recovery, [catalog payloads](../../specs/catalog-payloads.md), [data dictionary](../../specs/data-dictionary.md), [redb storage](../../specs/redb-storage.md), Database Layout and Validation, and [index-key encoding](../../specs/index-key-encoding.md), Complete Keys and Values. `validate_persisted_records` validates all required engine tables and the single `format` metadata record within one pinned redb snapshot, requires supported formats and READY lifecycle, bounds each catalog family at 65,536 records, and checks CAT1/EIR1 framing, RWR1 keys/versions, index definitions, bidirectional `indexes_v1`/`row_indexes_v1` membership, exact row/locator version agreement, OUT1 key/frontier consistency, and AUD1 key identity. It validates that exactly one database bootstrap record identifies the engine metadata UUID and repeats its catalog, expression, and built-in semantics versions. The mapping must contain exactly the 30 required sorted codes, with unique nonzero IDs and zero reserved bytes. Codes 1 and 2 resolve to schema records and kind-1 objects; code 3 resolves to the `database_admin` role principal and kind-9 object; codes 101–111 resolve to primitive type records with matching `builtin_kind` and kind-2 objects. Codes 201–216 remain opaque storage-object IDs pending a catalog target/ownership contract. Dependency records resolve their dependent endpoint to a family 2 object; referenced endpoints also resolve there except kind-5 database-scoped system-grant edges, which may reference the family 1 database ID. Kind-1 owned-child edges exactly match parent IDs and the parent graph is acyclic. Kind-2 edges exactly match column default and add-column-default, CHECK, policy-clause, and expression `dependency_ids` references, with matching typed expression/column/role endpoints and no duplicate expression dependencies. Kind-3 edges exactly match FK `referenced_constraint_id` values and connect FK constraints to PRIMARY KEY or UNIQUE constraints. Kind-4 edges exactly match PK/UNIQUE backing-index IDs and index owning-constraint IDs. Kind-5 edges exactly match membership, direct-grant, and policy-recipient authority pairs; grantors resolve to principals, and system grants target the database record. Policy column targets require a cell policy, a matching family 6 column attached to its family 5 table, and the expected object kinds. Constraint tables and source columns resolve to matching typed records/object kinds, source columns belong to the constraint table, and NOT NULL names a non-nullable column. Indexes resolve their table and ordered columns to matching typed records/object kinds, with every column attached to the indexed table. Unknown edge kinds fail closed, and catalog commits apply these rules to the final object, typed-record, constraint, index, and dependency sets. Every object owner resolves to a family 10 user/role; commit batches changing objects or principals validate all owners in the final catalog. Typed records from families 3–9 and 13 must match an objects row's ID and kind, family 10 principals must map user/role to object kinds 8/9, and column/principal normalized names must match their objects rows. Object kinds must be in 1–10. Startup and catalog commits now require a complete one-to-one mapping between family 2 objects and typed rows; commits revalidate the final mapping for every affected ID across families 3–10 and 13, including applicable name agreement. It validates principal authority epochs/login state and membership grantor/member/role endpoints, epochs, edge bounds, and full-graph cycle freedom.
- Ownership: DD-2 owns the bidirectional family-15 dependency-edge lookup index; see [data-dictionary progress](../data-dictionary/PROGRESS.md). This SM-5 entry tracks startup and commit integrity checks for authoritative dependency endpoints, edge semantics, and graph consistency.
- `row_indexes_v1` stores a reverse membership key formed by the 32-byte row key followed by the index key without its duplicate row UUID suffix. This lets commit refresh unchanged index locators in time proportional to the indexes on the changed rows; deleting a row removes all its index memberships atomically. Explicit index additions/removals update both tables in one redb transaction.
- Red evidence: startup tests first failed against the placeholder validator, and dedicated regressions then showed the validator accepted a locator older than its row, row commits left that locator unchanged, startup accepted cyclic/user-target membership graphs, READY startup accepted a missing database bootstrap descriptor, dependency edges could name absent objects, typed index/principal rows could lack or disagree with their generic object record, owned-child edges could be absent, disagree with object parent IDs, or form parent cycles, objects could name missing owner principals, column/principal names could disagree with object names, FK target edges could be omitted or point at a different constraint, PK backing-index edges could be omitted or point at a different index, kind-2 edges could be omitted or point at the wrong expression, and kind-5 edges could be missing or extra for membership, grants, or policy recipients; policy column targets could name missing columns, columns on another table, or columns on row policies; startup accepted constraints with missing tables/source columns, cross-table columns, or nullable NOT NULL columns; and startup/commit accepted indexes with missing or cross-table columns; startup also accepted an FK whose referenced column differed from its target key. Startup and commit accepted syntactically valid credential verifier records while no algorithm profile was defined. Commit regressions also showed dangling dependencies, missing typed object mappings, FK definitions without kind-3 dependency edges, backed PK/UNIQUE definitions without kind-4 edges, and column/expression records without kind-2 edges were accepted before validation, including a schema row with no object. Startup accepted missing mapped schema/type targets, a primitive target with the wrong `builtin_kind`, a mapped role renamed together with its object, and an extra family 2 object without a typed row; commit accepted deletion of a mapped primitive type, a paired role/object rename, and creation of an object without its typed row. The 36 startup tests, catalog dependency, owner-reference, owned-child, typed-object, name-agreement, FK-target, backing-index, expression-reference, authority-edge, index-reference, bootstrap-target commit regressions, and retained-index update/delete test now pass against real redb. Cases cover consistent reopen state, bootstrap identity/version/mapping and target corruption, missing dependency endpoints, same-transaction endpoint creation, rejection of endpoint deletion with a surviving edge, typed-object identity/kind checks across all typed families, non-READY metadata, malformed key/value identity, missing definitions/rows/reverse membership, invalid principal/membership graph shape, zero table ID, and records ahead of the commit frontier.
- Verification after bootstrap-target and two-way object/typed mapping checks: `cargo build --workspace --locked`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo test --workspace --locked` passed in the prescribed Nix shell using Rust 1.95.0 and redb 4.4.0; all 163 integration tests across 17 targets passed. Changed Markdown relative links and `git diff --check` also pass. TinyCC is available in the shell, but SDK and sandbox harness sources do not exist, so C/query acceptance is not yet runnable.
- Limits: startup and catalog commits validate mapped schema, role, and primitive-type targets; codes 201–216 still have no catalog target records whose ownership can be checked. Historical tuple adaptation, supported verifier-profile issuance, exclusive instance locking, and authenticated startup remain open. Index key derivation from changed typed row values and complete catalog-derived index delta validation remain unimplemented. No startup result should enable service admission until the remaining lifecycle and authentication checks are implemented.
- Next: derive typed index keys from row values and validate catalog-derived index deltas. Credential issuance remains gated until an authentication profile assigns supported verifier algorithms and encodings.

## SM-5 dependency: built-in type profile integrity — in progress

- Startup now requires one family-4 record per built-in kind, with the assigned parameter rule, scalar encoding, comparison version, default mask, and cast profile. Catalog commits validate every changed type profile, enforce unique built-in kinds across the final family-4 set, and verify mapped type rows still match their bootstrap codes.
- Red evidence: startup accepted individually altered parameter, encoding, comparison, default-mask, and cast-profile fields; commit accepted a wrong cast profile and two rows claiming the same built-in kind. Regression tests now reject each case.
- The catalog-schema guard fixture now uses canonical profiles and advances the table schema version when changing a column parameter, preserving the catalog mutation and stale-snapshot conflict case.
- Verification: `cargo build --workspace --locked`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo test --workspace --locked` passed in the prescribed Nix shell with Rust 1.95.0 and TinyCC 0.9.27-unstable-2025-01-06; all 166 integration tests across 17 targets passed. `git diff --check` and changed Markdown relative links passed.
- Remaining: typed row payload/schema agreement, verifier-profile issuance, exclusive instance locking, authenticated startup, catalog-derived index deltas, storage-object ownership for bootstrap codes 201–216, and the service admission lifecycle remain open.
- Next: validate typed row payload/schema agreement.

## SM-5 dependency: persisted tuple/schema agreement — in progress

- Startup now resolves persisted family-5 table versions and ordered family-6 columns to their family-4 built-in type profiles. It validates column ordinals, introduction versions, and type parameters, then decodes every RWR1 payload as TUP1 against that schema. Missing table/type definitions and invalid catalog schemas fail closed; malformed tuples, column-count or schema-version mismatches, scalar errors, and invalid NULLs return `InvalidRow`.
- Red evidence: a READY database containing an RWR1 record with arbitrary non-TUP1 bytes was accepted. Regressions now cover malformed tuple bytes, mismatched table schema version, column-count mismatch, NULL in a non-nullable field, and a row whose table has no catalog schema.
- Shared the catalog-to-scalar-type decoder between startup validation and invocation schema loading. Updated the recovery fixture to use a real built-in BYTEA column and TUP1 values.
- Spec decision: persisted TUP1 rows must match the table's current schema version exactly. Historical tuple adaptation is unsupported and remains a prerequisite for schema-changing DDL over existing rows.
- Verification: `cargo build --workspace --locked`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo test --workspace --locked` passed in the prescribed Nix shell with Rust 1.95.0 and TinyCC 0.9.27-unstable-2025-01-06; all 168 integration tests across 17 targets passed. `git diff --check` and changed Markdown relative links passed.
- Remaining: historical tuple adaptation, commit-time schema enforcement for low-level row APIs, typed index-key derivation/delta validation, bootstrap storage-object semantics, verifier-profile issuance, exclusive instance locking, authenticated startup, and service admission remain open.
- Next: derive typed index keys from row values and validate catalog-derived index deltas.

## SM-5 dependency: typed row to catalog index key — in progress

- Contract: [index-key encoding](../../specs/index-key-encoding.md), Complete Keys and Values. The derivation primitive accepts ordered opaque 16-byte schema IDs/types, a schema version and TUP1 payload, and an ordered 16-byte index column-ID list. It rejects zero/duplicate/misaligned schema IDs, empty/zero/duplicate/missing index IDs, and invalid TUP1 data; it projects only the indexed fields in catalog order and returns the complete index-ID/tuple/row-ID key plus an indexed-NULL flag.
- Red evidence: the two new regressions failed against the compiling placeholder with `InvalidDefinition`. They now cover non-leading and reordered projected columns, embedded-zero text, NULL projection, invalid definitions, malformed TUP1 length, and schema-version mismatch.
- Verification: the prescribed Nix shell provided Rust/Cargo 1.95.0, rustfmt 1.9.0, Clippy 0.1.95, and TinyCC 0.9.27-unstable-2025-01-06. Workspace build, formatting check, Clippy with warnings denied, and all 170 integration tests across 17 targets passed.
- Catalog-ID correction: switched the schema/index column-ID interface to opaque 16-byte IDs, matching catalog encoding. The focused `index_key` target (10 tests), formatting, and whitespace checks pass after that API correction.
- Limits: this is a derivation primitive only. Transaction commit still accepts caller-supplied index deltas; it does not yet prove complete catalog-derived membership changes, enforce uniqueness against the final row set, or prevent index-definition changes from publishing inconsistent memberships.
- Next: use the pinned schema/index definitions to derive and enforce complete index deltas for changed rows.

## SM-5 dependency: pinned catalog index definitions — in progress

- Added `load_catalog_indexes` to resolve family-9 indexes against a catalog-loaded table schema. It checks method, uniqueness encoding, NULLS DISTINCT rule, storage ID, VALID state, key version, schema version, and nonempty ordered distinct column IDs belonging to the same table. Each matching index record and the schema/type records are guarded for final commit.
- The real-redb schema loader regression checks a valid unique index, verifies a foreign-table column fails closed, and changes the index record after loading to confirm the stale invocation conflicts.
- Verification: workspace build, format check, Clippy with warnings denied, and all 170 integration tests across 17 targets passed in the prescribed Nix shell. The changed Markdown relative links and `git diff --check` pass.
- Limitation at that step: the lower-level explicit-write API still did not derive complete membership changes; see the following increment for the catalog-loaded invocation gate.

## SM-5 dependency: catalog-derived index delta validation — in progress

- Red evidence: a catalog-loaded typed row insert with three valid indexes and no index writes committed before the change. The regression now expects rejection and confirms no row is published. Additional cases reject a row write after schema loading without index loading, and reject a forged key.
- Invocation finalization now derives the expected old/new key and NULL state for every index loaded from the pinned catalog. It requires the staged mutations to match those keys, row IDs, expected locator versions, uniqueness flags, and NULL-exemption flags. Existing locator versions must match the row version; an already-present key cannot be silently overwritten as a new membership. Catalog schema/index guards are still checked by the serialized commit.
- Real-redb coverage now exercises all three catalog indexes through insertion, indexed-value update, and deletion, and verifies locator versions. A catalog-backed authorized INSERT also advances the index schema versions and publishes its index memberships atomically.
- Verification: prescribed Nix-shell workspace build, formatting check, Clippy with warnings denied, and all 170 integration tests across 17 targets passed. Changed Markdown relative links and `git diff --check` pass.
- Limitation: callers still stage the exact derived `IndexWrite`s after loading schema/index definitions. Index membership is not yet synthesized automatically from the row overlay. Low-level explicit-write commit helpers and projection-only test paths remain trusted scaffolding, not executor-facing paths.
- Next: synthesize index deltas during invocation finalization and make catalog-backed row mutation APIs independent of caller-supplied index writes.

## SM-5 dependency: automatic catalog index maintenance — in progress

- Invocation finalization now synthesizes index additions and removals from the pinned catalog schema/index definitions and the prior/final typed row. Optional caller-staged index writes are accepted only when they exactly match the derived mutations; omitted changes are generated automatically. Unchanged indexed values continue to receive refreshed locators in the serialized storage commit.
- Generated writes are checked against the invocation's index-change and key-byte budgets. A budget failure aborts the invocation without publishing the row.
- Real-redb coverage exercises automatic insert, indexed-value update, and delete maintenance, verifies locators, rejects missing index definitions and forged writes, and checks atomic rejection when synthesized writes exceed the budget.
- Verification: `cargo build --workspace --locked`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and all 170 integration tests across 17 targets passed in the prescribed Nix shell. `git diff --check` passed.
- Remaining: the low-level explicit-write API and projection-only helpers remain trusted scaffolding. Direct low-level row commit helpers do not yet enforce the catalog schema. Index-definition DDL/rebuild behavior, uniqueness validation beyond the current commit path, and service integration remain open.

## SM-5 dependency: schema-loaded tuple commit validation — in progress

- Red evidence: after removing all secondary index definitions from a fixture table, an invocation could load its schema and finalize an arbitrary byte payload because no index derivation decoded the row.
- Finalization now decodes every inserted or updated row against the pinned catalog schema even when the table has zero secondary indexes. It also validates an existing row's tuple before using it as the prior image. Malformed tuples abort with `InvalidRow` and persist ABORTED without publishing the row.
- A real-redb regression removes the table's index definitions, loads its schema, stages a non-TUP1 payload, and verifies the row remains absent with a durable ABORTED outcome.
- Verification: `cargo build --workspace --locked`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and all 171 integration tests across 17 targets passed in the prescribed Nix shell. `git diff --check` passed.
- Remaining: the low-level direct row commit helpers still lack catalog-schema validation and remain trusted-only; historical tuple adaptation and service admission remain open.

## SM-5 dependency: fail-closed index definition changes — in progress

- Red evidence: catalog commit could publish a new index definition for a table that already had rows, without building the corresponding memberships, which could silently produce incomplete indexed query results.
- Catalog commit now rejects adding, changing, or dropping an index definition while its current/final table has persisted rows, the same commit inserts rows for the table, or the index has stored memberships. Changes remain allowed for empty tables until transactional backfill/rebuild exists.
- A real-redb regression commits a row and its existing index membership, attempts to add a second index, and confirms the DDL aborts without changing the catalog, audit, or commit sequence.
- Verification: `cargo build --workspace --locked`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and all 172 integration tests across 17 targets passed in the prescribed Nix shell. `git diff --check` passed.
- Remaining: transactional index backfill/rebuild and online schema migration remain open.

## SM-5 dependency: startup index membership coverage — in progress

- Red evidence: startup accepted a valid typed row with a catalog-defined index but no corresponding `indexes_v1` or `row_indexes_v1` membership; it also accepted a stored key whose tuple component disagreed with the row.
- Startup now derives each persisted row's expected key for every catalog index, checks the locator and reverse membership, and verifies every stored index key equals the derivation. Missing memberships, mismatched keys, and extra entries fail with `InvalidIndex`.
- Real-redb regressions cover missing index coverage and a locator/reverse entry attached to the wrong typed key; a valid persisted row/index image still passes.
- Verification: `cargo build --workspace --locked`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and all 173 integration tests across 17 targets passed in the prescribed Nix shell. `git diff --check` passed.
- Remaining: startup validation still needs uniqueness checking for persisted unique-index duplicates and authenticated service admission.

## SM-5 dependency: persisted unique-index integrity — in progress

- Red evidence: startup accepted two rows with the same non-NULL tuple in a catalog-defined unique index, even though invocation commits enforce uniqueness.
- Startup now validates the catalog uniqueness flag, tracks derived tuple prefixes for unique indexes, and rejects duplicate non-NULL keys. NULL values follow the catalog's NULLS DISTINCT rule and do not collide.
- Real-redb coverage rejects duplicate persisted non-NULL keys and accepts repeated NULL keys under a unique index.
- Verification: `cargo build --workspace --locked`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and all 175 integration tests across 17 targets passed in the prescribed Nix shell. `git diff --check` and changed Markdown link checks passed.
- Remaining: authenticated service admission and foreign-key constraint lifecycle integration remain open.

## SM-5 dependency: PRIMARY KEY and UNIQUE write enforcement — in progress

- Red evidence: catalog-backed invocation loading rejected every PRIMARY KEY/UNIQUE constraint as an unsupported kind, although commit already enforces uniqueness through the corresponding unique index.
- Invocation constraint loading now validates each key constraint's ordered source columns and owned backing index, including table, key shape, owner, uniqueness flag, state, and schema version. PRIMARY KEY columns must also be non-nullable. CHECK expressions load the BOOLEAN profile only when needed.
- A real-redb regression loads a catalog UNIQUE constraint and its owned unique index, stages duplicate tuples, and confirms the whole invocation aborts without publishing either row.
- Verification: `cargo build --workspace --locked`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and all 176 integration tests across 17 targets passed in the prescribed Nix shell. `git diff --check` and changed Markdown link checks passed.
- Remaining: foreign-key runtime enforcement and authenticated service admission remain open.

## SM-5 dependency: primary-key catalog integrity — in progress

- Red evidence: catalog commit and READY startup accepted a PRIMARY KEY on a nullable source column despite its valid owned unique backing index.
- Catalog commit and startup validation now reject nullable PRIMARY KEY columns. Startup also verifies that each PRIMARY KEY/UNIQUE backing index is unique and matches the ordered source columns.
- Real-redb regressions construct coherent catalog object, constraint, dependency, and backing-index records, then reject the invalid primary key at commit and startup.
- Verification: `cargo build --workspace --locked`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and all 178 integration tests across 17 targets passed in the prescribed Nix shell. `git diff --check` and changed Markdown link checks passed.
- Remaining: foreign-key runtime enforcement and authenticated service admission remain open.

## SM-5 dependency: catalog-backed foreign-key enforcement — in progress

- Added invocation loading for valid catalog FOREIGN KEY definitions and their referenced PRIMARY KEY/UNIQUE constraints, backing indexes, source/target schemas, ordered columns, and exact built-in scalar types/parameters. All definitions used by the invocation are guarded for commit.
- The serialized commit checks final source and target rows after applying the staged write set logically, enforcing MATCH SIMPLE, ON UPDATE RESTRICT, and ON DELETE RESTRICT. It checks both child and parent changes, so concurrent child insertion and parent deletion cannot both commit. Combined persisted and staged row visits are capped at 65,536 per commit; exceeding the cap aborts.
- Real-redb regressions cover dangling child rejection, matching child insert, NULL exemption, restricted parent delete, atomic child-plus-parent delete, a barrier-synchronized child-insert/parent-delete race, and exceeding the row-visit budget with an ABORTED outcome and no row publication.
- Verification: with Rust/Cargo 1.95.0, rustfmt 1.9.0, Clippy 0.1.95, and TinyCC 0.9.27-unstable-2025-01-06 in the prescribed Nix shell, workspace build, formatting check, Clippy with warnings denied, and all 183 integration tests across 17 targets passed. Changed Markdown local links and `git diff --check` pass.
- The change includes Rust implementation/tests and updates to [schema DDL](../../specs/ddl-schema.md) and [concurrency/MVCC](../../specs/concurrency-mvcc.md).
- Remaining: authenticated service admission, catalog DDL creation/fencing for foreign keys, and an indexed validation path for larger commit-time scans remain open.

## SM-5 dependency: persisted foreign-key startup validation — in progress

- Startup now validates FK action/match/state fields, the referenced unique backing index, ordered source/target columns, and exact built-in scalar types and parameters. It accepts at most 256 FK constraints.
- After validating index membership, startup checks each non-NULL child tuple against the referenced unique index; NULL values pass under MATCH SIMPLE. Combined child-row visits are bounded at 65,536, and exceeding the bound rejects startup.
- Real-redb startup coverage accepts matching child/parent rows, rejects a dangling child, accepts NULL under MATCH SIMPLE, rejects mismatched source/target types, and rejects a database over the row-visit limit.
- Verification: with Rust/Cargo 1.95.0, rustfmt 1.9.0, and Clippy 0.1.95 in the prescribed Nix shell, workspace build, formatting check, Clippy with warnings denied, and all 183 integration tests across 17 targets passed. Changed Markdown local links and `git diff --check` pass.
- The implementation uses the existing unique backing index to validate references without materializing the full parent key set.
- Remaining: foreign-key DDL creation/fencing and authenticated service admission remain open.

## SM-5 dependency: bounded foreign-key comparison memory — in progress

- The commit-time validator now caps the distinct encoded comparison keys retained for each FK check at 16 MiB, in addition to the 65,536 combined row-visit limit. The transaction aborts with the same generic FK validation limit error before publication when either limit is exceeded.
- A real-redb regression with 128 distinct 70 KiB parent/child keys exceeds the comparison-key budget and confirms the child row version and commit sequence remain unchanged.
- Verification: with Rust/Cargo 1.95.0, rustfmt 1.9.0, and Clippy 0.1.95 in the prescribed Nix shell, workspace build, formatting check, Clippy with warnings denied, and all 184 integration tests across 17 targets passed. Changed Markdown local links and `git diff --check` pass.
- Remaining: FK DDL creation/fencing and authenticated service admission remain open.

## SM-5 dependency: catalog-commit foreign-key type compatibility — in progress

- Final-catalog constraint validation now rejects an FK whose ordered source and referenced columns have different built-in type IDs or parameter bytes. It applies to catalog commits before publication, matching invocation loading and startup checks.
- A real-redb regression submits otherwise coherent FK, PRIMARY KEY, UNIQUE index, and dependency records with mismatched source/target types; catalog commit rejects the change and leaves the commit sequence unchanged.
- Verification: with Rust/Cargo 1.95.0, rustfmt 1.9.0, and Clippy 0.1.95 in the prescribed Nix shell, workspace build, formatting check, Clippy with warnings denied, and all 184 integration tests across 17 targets passed.
- Remaining: validating pre-existing child rows when creating an FK, FK DDL fencing/lifecycle, and authenticated service admission remain open.

## SM-5 dependency: catalog-commit foreign-key creation validation — in progress

- Catalog commits now load the final source and referenced table schemas for changed FK definitions and validate existing non-NULL child rows against the latest target rows plus any row changes in the same serialized commit. The existing 65,536 combined row-visit and 16 MiB per-FK comparison-key limits apply, and invalid data leaves the constraint unpublished with an ABORTED outcome.
- FK definition changes on an existing table require write-epoch advances on both the referencing and referenced tables. Catalog commits also preserve the 256 live-FK limit enforced by invocation loading and startup.
- Real-redb coverage rejects adding an FK over an orphan child, accepts matching pre-existing parent/child rows, and confirms missing the referenced-table epoch advance aborts publication.
- Verification: with Rust/Cargo 1.95.0, rustfmt 1.9.0, Clippy 0.1.95, and TinyCC 0.9.27-unstable-2025-01-06 in the prescribed Nix shell, workspace build, formatting check, Clippy with warnings denied, and all 186 integration tests across 17 targets passed. `git diff --check` and changed Markdown link checks passed.
- Contract updates: [schema DDL](../../specs/ddl-schema.md) defines validation of existing rows on FK creation; [data dictionary](../../specs/data-dictionary.md) requires epoch publication for both related tables.
- Remaining: a separate per-table DDL fence manager, online validation/backfill, and authenticated service admission remain open.

## SM-5 dependency: per-table DDL fence primitive — in progress

- Added a clonable `DdlFenceManager` with shared guards for data writers and exclusive guards for DDL. Multi-table requests are sorted, deduplicated, and acquired all at once; waiting DDL blocks new writers on affected tables. Deadline failures release all waiter state, and RAII guards release acquired fences.
- Red evidence: the first focused run had two failures because the placeholder admitted exclusive DDL alongside active writers; both exclusion regressions now pass.
- Integration tests cover existing-writer exclusion, DDL-writer exclusion, multi-table timeout cleanup, duplicate IDs, shared-writer coexistence, zero-ID rejection, and empty fence sets.
- Verification: with Rust/Cargo 1.95.0 and Clippy 0.1.95 in the prescribed Nix shell, workspace build, formatting check, Clippy with warnings denied, and all 189 integration tests across 18 targets passed. `git diff --check` and changed Markdown link checks passed.
- The implementation refines the locking decision in [schema DDL](../../specs/ddl-schema.md).
- Added `StorageManager` entry points that route invocation commits through shared table fences and catalog commits through exclusive fences derived from changed tables, constraints, indexes, columns, policies, and expression dependents. Fence timeouts durably record an ABORTED outcome. Raw writer helpers remain unfenced trusted primitives.
- Verification after coordinated entry-point integration: with Rust/Cargo 1.95.0, rustfmt 1.9.0, Clippy 0.1.95, and TinyCC 0.9.27-unstable-2025-01-06 in the prescribed Nix shell, workspace build, formatting check, Clippy with warnings denied, and all 191 integration tests across 18 targets passed. `git diff --check` and changed Markdown link checks passed.
- Remaining: production service admission must use `StorageManager` consistently; online validation/backfill and authenticated service admission remain open.

## SM-5 dependency: bounded active invocation snapshots — in progress

- Behavior: reject new runtime admissions once a configured per-manager snapshot cap is reached, including admissions through cloned `StorageManager` handles; release capacity when the transaction drops or ends its snapshot through commit, abort, or expiry. This protects redb from unbounded retained read snapshots.
- Red evidence: the new real-redb regression initially failed because a second snapshot was admitted while the configured limit was one.
- Implementation: `StorageManagerConfig` exposes the DDL-fence timeout and positive active-invocation cap (default 128); atomic admission permits are shared across manager clones and retained by each managed invocation transaction. Failed transaction construction releases its reserved permit through RAII. A zero cap is rejected at configuration time. Low-level transaction constructors remain explicitly outside manager admission.
- Tests: `engine/tests/commit.rs` verifies zero-limit rejection, cross-clone capacity enforcement, permit release immediately after abort while the transaction object remains alive, and subsequent admission.
- Verification: with Rust/Cargo 1.95.0, rustfmt 1.9.0, Clippy 0.1.95, and TinyCC 0.9.27-unstable-2025-01-06 in the prescribed Nix shell, workspace build, formatting check, Clippy with warnings denied, and all 192 integration tests across 18 targets passed. `git diff --check` and changed Markdown local-link checks passed.
- Remaining: bounds for scans, response bytes, and total work still require service lifecycle integration.

## SM-5 dependency: validated StorageManager startup — in progress

- Behavior: do not expose invocation admission from a runtime manager until the recovered image passes the engine's full implemented startup integrity validation and is READY.
- Red evidence: the new real-redb regression inserted a malformed row envelope into an otherwise READY database; initially, constructing a StorageManager still succeeded.
- Implementation: `StorageManager::new` and `with_config` now return a typed initialization error and call `validate_persisted_records` before constructing fence/admission state. The check covers table definitions, bootstrap/catalog, typed rows, indexes, constraints, outcomes, and audit, but it is not client authentication or service identity validation.
- Test: `engine/tests/commit.rs` verifies invalid row persistence rejects manager construction with `StartupError::InvalidRow`; the existing manager routing test verifies a valid recovered image is accepted.
- Verification: with Rust/Cargo 1.95.0, rustfmt 1.9.0, and Clippy 0.1.95 in the prescribed Nix shell, workspace build, formatting check, Clippy with warnings denied, and all 193 integration tests across 18 targets passed. `git diff --check` and changed Markdown local-link checks passed.
- Remaining: authenticated service admission and lifecycle cancellation remain separate service work.

## SM-5 dependency: managed invocation age limits — in progress

- Behavior: bound managed invocation lifetime at snapshot reads, staged-write checks, and commit; allow the supervisor/service to poll idle invocations and cancel expired work.
- Red evidence: with the deadline check temporarily disabled, the real-redb expiry regression failed because `check_deadline()` returned success after the configured maximum age.
- Implementation: configuration defaults to a 15-minute maximum age and rejects zero. A managed invocation checks age when it uses its pinned snapshot, when it stages against a cached base version, and before and after waiting for a shared DDL fence. Expiry durably records ABORTED, drops the retained redb snapshot, and releases the admission slot immediately. `InvocationTransaction::check_deadline` supports service-side polling during idle periods.
- Tests: `engine/tests/commit.rs` checks idle polling expiry, the durable ABORTED outcome, immediate snapshot-slot reuse while the expired transaction object remains alive, and commit-time expiry.
- Verification: with Rust/Cargo 1.95.0, rustfmt 1.9.0, and Clippy 0.1.95 in the prescribed Nix shell, workspace build, formatting check, Clippy with warnings denied, and all 194 integration tests across 18 targets passed. `git diff --check` and changed Markdown local-link checks passed.
- Remaining: no supervisor exists yet to poll idle transactions automatically; scan, result, and total-work budgets also remain service lifecycle work.

## SM-5 dependency: owner-scoped outcome lookup — in progress

- Behavior: provide the trusted service layer a durable outcome query for resolving lost commit replies, exposing only unexpired outcomes belonging to the already authenticated owner.
- Implementation: `StorageManager::lookup_invocation_outcome` delegates to the canonical outcome reader, which maps unknown IDs, owner mismatches, and expired records to absence. The method accepts an authenticated principal ID from its trusted caller; it does not authenticate network clients or grant administrator visibility. The manager no longer exposes a raw redb handle accessor.
- Test: `engine/tests/commit.rs` verifies ABORTED and COMMITTED lookup, owner isolation, expiry, and unknown invocation behavior through `StorageManager`.
- Verification: with Rust/Cargo 1.95.0, rustfmt 1.9.0, and Clippy 0.1.95 in the prescribed Nix shell, workspace build, formatting check, Clippy with warnings denied, and all 195 integration tests across 18 targets passed. `git diff --check` and changed Markdown local-link checks passed.
- Remaining: the supervisor/gateway authentication path is not implemented; administrator outcome visibility remains separate.

## SM-5 dependency: bounded existing-row index creation — in progress

- Coordinated catalog DDL now builds new index memberships from a pinned redb read snapshot while holding the table's exclusive DDL fence, without holding the serialized writer during the scan. It rechecks all scanned row keys and versions in the write transaction, then publishes forward/reverse memberships, catalog definition, audit, sequence, and outcome atomically.
- Builds are limited to 65,536 rows, 65,536 generated memberships, and 64 MiB combined stored-row and generated-key bytes per commit. Unique conflicts, malformed rows, stale snapshots, and limit failures leave no catalog or index publication. Combining a populated-table schema change with index creation remains rejected. Trusted unfenced catalog helpers still fail closed for populated-table index changes.
- Real-redb regressions cover successful backfill, atomic rejection of duplicate values for a new unique index, and failure above the row cap; the low-level rejection regression remains.
- Verification: in the prescribed Nix shell, `cargo build --workspace --locked`, `cargo fmt --all -- --check`, Clippy with warnings denied, and all 198 integration tests across 18 targets passed. `git diff --check` and changed Markdown link checks passed.
- Contract updates: [schema DDL](../../specs/ddl-schema.md) and [redb storage](../../specs/redb-storage.md).
- Remaining: action-specific authorization for other DDL and service admission remain open.

## SM-5 dependency: populated index drop — in progress

- Coordinated DDL now scans a dropped index's physical memberships under the table's exclusive fence, validates each locator against its row version and matching reverse entry, and rechecks the complete index prefix before deletion. Forward/reverse memberships and catalog/object removal publish in one commit; pinned readers keep the pre-drop redb state.
- Removal shares the 65,536-membership and 64 MiB index-key/locator work bounds. Unique/constraint-owned indexes remain subject to final catalog dependency checks. Unfenced low-level catalog commits still reject drops when rows or memberships remain.
- The real-redb lifecycle regression creates an index over an existing row, proves the trusted low-level helper rejects dropping it, then drops it through `StorageManager`; the old snapshot still resolves the old membership and startup accepts the new image.
- Verification: in the prescribed Nix shell, workspace build, format check, Clippy with warnings denied, and all 198 integration tests across 18 targets passed. `git diff --check` and changed Markdown link checks passed.
- Remaining: action-specific authorization for other DDL and service admission remain open.

## SM-5 dependency: populated index rebuild

- Coordinated DDL can update an existing index definition on a populated table under its exclusive table fence. It derives old and new keys from a pinned row snapshot, validates every existing forward locator and matching reverse entry, and rechecks row keys/versions and the complete old index prefix under the serialized writer.
- Changed memberships are removed and inserted in one durable commit with the updated catalog definition, audit event, sequence, and outcome. Unchanged keys retain their row versions. New unique definitions are checked against each other and the current index contents; duplicate values abort without changing the catalog or membership set. Rebuilds share the 65,536-row, 65,536-membership, and 64 MiB DDL bounds. Combining a populated-table schema change with index creation or rebuild remains unsupported.
- Real-redb regressions cover a successful populated-index re-key and nonunique-to-unique change with an old pinned reader, rejection by the unfenced low-level helper, startup validation after publication, and rejection of a unique rebuild with duplicate values. The existing populated-index drop and create regressions remain in place.
- Verification: with Rust/Cargo 1.95.0, rustfmt 1.9.0, Clippy 0.1.95, and TinyCC 0.9.27-unstable-2025-01-06 in the prescribed Nix shell, workspace build, formatting check, Clippy with warnings denied, and all workspace tests passed. `git diff --check` and changed Markdown local-link checks passed.
- Contract updates: [schema DDL](../../specs/ddl-schema.md) and [redb storage](../../specs/redb-storage.md).
- Remaining: action-specific authorization for other DDL and service admission remain open.

## SM-5 dependency: ordinary index DDL authorization — in progress

- `StorageManager::commit_authorized_index_ddl` is a narrow route for an actor already authenticated by a trusted administrative caller. It accepts only index-definition and matching index-object changes, verifies the actor is a live login-enabled user, then checks database-administrator membership, table ownership through the inherited role closure, or an `INDEX` grant on every affected table.
- The route carries the actor, complete membership graph, role records, applicable grants, and table-owner records into the serialized commit as catalog guards. Revoked grants, changed memberships, disabled users, or transferred ownership invalidate a stale authorization. The generic coordinated catalog helper remains trusted-only and has no authorization check.
- A real-redb integration test covers a direct `INDEX` grant, table ownership, inherited membership in the reserved administrator role, denial without authority, rejection of an index-object owner transfer through this route, durable ABORTED outcomes, and absence of catalog/audit publication on denial.
- Verification: in the prescribed Nix shell, workspace build, formatting check, Clippy with warnings denied, and all workspace tests passed. `git diff --check` and changed Markdown local-link checks passed.
- Contract update: [schema DDL](../../specs/ddl-schema.md) and [redb storage](../../specs/redb-storage.md).
- Remaining: the administrative channel must authenticate the actor before calling this route; deterministic authority-change races, authorization rules for constraints/policies/identity/grants, and token-backed gateway/supervisor admission remain open.

## SM-5 dependency: managed user admission — in progress

- `StorageManager::begin_authenticated_invocation` accepts an identity from a trusted caller, validates that the principal is a live `LOGIN` user in the invocation's pinned catalog snapshot, and returns the pinned role closure. Identity and membership records used for admission are guarded through commit. Unknown and disabled users are denied with durable ABORTED outcomes.
- Token verification and expiry/revocation checks are now handled by `begin_token_authenticated_invocation`; the caller-supplied-identity method remains available only after trusted external authentication. The gateway/supervisor authentication boundary remains outside the storage manager, and `begin_invocation` remains a lower-level trusted path.
- A real-redb integration test covers successful admission, disabled and unknown-user denial, durable outcomes, and release of the admission slot after failed attempts.
- Verification: in the prescribed Nix shell, workspace build, formatting check, Clippy with warnings denied, and workspace tests passed. Changed Markdown links and `git diff --check` passed.
- Contract update: [users and roles](../../specs/ddl-identity.md).
- Remaining: token issuance/inventory APIs, gateway/supervisor authentication, and action-specific authorization for other DDL remain open.

## SM-5 dependency: role-membership DDL authorization — in progress

- `StorageManager::commit_authorized_membership_ddl` accepts only membership edges, their exact authority-dependency edges, and affected principals' authority-epoch increments. It authorizes ordinary membership changes through inherited `MANAGE_ROLES` or current administrator membership; changes to the reserved administrator role require a current administrator.
- The route guards the actor identity, complete membership graph, applicable system-grant records, and role records through serialized publication. Added membership edges must name the authenticated actor as grantor, and successful audit events must identify that actor.
- It blocks direct or inherited membership changes that grant or remove database-administrator membership without an administrator, and preserves at least one login-enabled administrator with a current credential.
- A real-redb regression covers authorized membership add/remove, denial without `MANAGE_ROLES`, administrator-only changes to the reserved role, final-administrator preservation, authority epochs, and durable outcomes.
- Verification: in the prescribed Nix shell, workspace build, formatting check, Clippy with warnings denied, and workspace tests passed. Changed Markdown links and `git diff --check` passed.
- Contract update: [users and roles](../../specs/ddl-identity.md).
- Remaining: authorization for identity/grant/policy/constraint DDL, token-backed gateway admission, and deterministic authorization-change race coverage remain open.

## SM-5 dependency: system privilege DDL authorization — in progress

- `StorageManager::commit_authorized_system_privilege_ddl` handles only database-scoped `MANAGE_USERS`, `MANAGE_ROLES`, and `CREATE_SCHEMA` grant/revoke records, their authority dependencies, and grantees' authority-epoch updates. It denies system privilege changes unless the actor is a current database administrator.
- The route guards the actor's login state, complete membership graph, role identities, and successful audit actor through serialized publication. New grant records identify the authenticated administrator as grantor.
- A real-redb regression covers administrator grant/revoke, denial for a non-administrator who holds `MANAGE_ROLES`, atomic dependency removal, epoch advances, and durable outcomes.
- Verification: in the prescribed Nix shell, workspace build, formatting check, Clippy with warnings denied, and workspace tests passed. Changed Markdown links and `git diff --check` passed.
- Contract update: [users and roles](../../specs/ddl-identity.md).
- Remaining: ordinary object grant authorization, user/role lifecycle DDL, token-backed gateway admission, and deterministic authorization-change race coverage remain open.

## SM-5 dependency: object grant DDL authorization — in progress

- `StorageManager::commit_authorized_object_grant_ddl` accepts ordinary family-12 grant/revoke changes, their authority dependencies, and affected grantees' authority-epoch increments. It excludes database-scoped system privileges.
- Each schema/table target is checked against its current owner, inherited `MANAGE_GRANTS` on that same object, or current database-administrator membership. Actor identity, membership closure, owner records, and applicable `MANAGE_GRANTS` records are guarded through publication; new grants identify the actor as grantor.
- Scope validation permits schema-only `USAGE`/`CREATE`, table or column data grants, table-only `DELETE`, and schema/table administration privileges. Column grants resolve ownership through their table.
- A real-redb regression covers owner and `MANAGE_GRANTS` authority, denial without either, grant/revoke publication, authority-epoch updates, and durable outcomes.
- Verification: in the prescribed Nix shell, workspace build, formatting check, Clippy with warnings denied, and workspace tests passed. Changed Markdown links and `git diff --check` passed.
- Contract update: [permission DDL](../../specs/ddl-permissions.md).
- Remaining: identity lifecycle DDL, policy/constraint authorization, token-backed gateway admission, and deterministic authorization-change race coverage remain open.

## SM-5 dependency: shared principal namespace integrity — in progress

- Startup validation rejects persisted duplicate normalized names across users and roles. Catalog publication checks the complete final principal-name set under commit serialization, applying renames and deletions before additions and bounding the scan to the catalog-family limit.
- Real-redb tests cover rejection of a user name colliding with the reserved role, startup rejection of persisted duplicates, and an atomic rename that frees a name for a newly created role.
- Verification: the prescribed Nix-shell workspace build, formatting check, Clippy with warnings denied, and workspace tests passed. Changed Markdown links and `git diff --check` passed.
- Contract update: [users and roles](../../specs/ddl-identity.md).
- Remaining: durable identity tombstones now enforce ID non-reuse; DROP remains unexposed until authorized RESTRICT dependency cleanup is implemented.

## SM-5 dependency: identity lifecycle DDL authorization — in progress

- `StorageManager::commit_authorized_identity_ddl` now supports user creation, rename, and LOGIN/NOLOGIN changes plus role creation and rename. User changes require inherited `MANAGE_USERS`; role changes require inherited `MANAGE_ROLES`; current database administrators may do either.
- The route accepts complete principal/object creation pairs, matched rename pairs, principal-only login-state changes, and one-way token revocations. Disabling a user with active tokens requires their revocation records in the same batch; active tokens cannot remain attached to a NOLOGIN user. It rejects identity deletion and unrelated catalog writes. Actor identity, membership closure, role records, system grants, and the login-enabled administrator set are guarded through commit; successful DDL audit actors must match the authenticated caller.
- A real-redb regression covers MANAGE_USERS admission, MANAGE_ROLES denial, user rename and disable, rejection of disable without token revocation, atomic disable plus revocation, last-login-admin preservation, database-admin role creation, and committed sequences.
- Verification: the prescribed Nix-shell workspace build, formatting check, Clippy with warnings denied, and workspace tests passed. Changed Markdown links and `git diff --check` passed.
- Contract: [users and roles](../../specs/ddl-identity.md).
- Remaining: role/user deletion still needs authorized RESTRICT dependency cleanup, and deterministic authorization-change race coverage remains open.

## SM-5 dependency: credential verifier profile and token-backed invocation admission — in progress

- Family-16 verifiers use SHA-256 algorithm/version 1 over an exactly 32-byte uniformly random bearer secret. The profile validates verifier width, issue/expiry times, revocation metadata, labels, authority epochs, user ownership, and LOGIN state. This profile is not for human-chosen passwords.
- `StorageManager::begin_token_authenticated_invocation` verifies the secret and trusted current time against the invocation's pinned catalog snapshot, then loads the role closure. The verifier, user, and membership records are guarded through commit; a token revocation after admission makes pending writes conflict. Authentication failures do not distinguish unknown, malformed, wrong-secret, expired, revoked, or disabled-user credentials.
- `StorageManager::issue_token` creates random 32-byte secrets through the system CSPRNG, stores only the SHA-256 verifier, authorizes through `MANAGE_USERS`, records a `TokenLifecycle` audit event, and exposes the secret once through a zeroizing result type. `StorageManager::revoke_token` uses the same authorization route; unknown/already-revoked/unauthorized requests share an error. Identity DDL allows only one-way revocation of existing verifiers and requires all active tokens to be revoked atomically with a NOLOGIN change.
- Tests cover supported and malformed profiles, successful admission, wrong secret, expiry, authorized issue/revoke, one-time secret transfer, denied issuance, durable abort outcomes, concurrent revocation conflict, and the LOGIN disable path.
- Verification: Rust/C toolchain versions matched the configured Nix environment; locked workspace build, formatting check, Clippy with warnings denied, and all 216 tests across 18 integration targets passed. Changed Markdown local-link checks and `git diff --check` passed.
- Remaining: bootstrap secret handoff, gateway/supervisor integration, and authentication rate limits.

## SM-5 dependency: policy DDL authorization — in progress

- `StorageManager::commit_authorized_policy_ddl` authorizes row/cell policy and target changes on existing tables. The actor must own every affected table through its role closure or hold inherited `MANAGE_POLICIES` on the table or schema.
- The route accepts paired policy/target/expression/object/dependency writes, requires table write-epoch changes, checks that surviving policies have recipients, and guards the actor, membership graph, owner/schema records, and applicable grants through publication. Requests are bounded by `MAX_AUTHORIZATION_INPUTS`; audit events must identify the authenticated DDL actor.
- A real-redb regression covers table-owner and inherited schema-level `MANAGE_POLICIES` authorization, policy create/update/drop, denial without authority, empty-recipient rejection, epoch publication, and durable outcomes.
- Verification: `cargo build --workspace --locked`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo test --workspace --locked` passed in the prescribed Nix shell; all 210 tests across 18 test targets passed. Changed Markdown local links and `git diff --check` passed.
- Contract: [permission DDL](../../specs/ddl-permissions.md).
- Remaining: non-foreign-key constraint DDL authorization, durable identity tombstones, authenticated control-channel integration, and deterministic authorization-change race coverage remain open.

## SM-5 dependency: foreign-key DDL authorization — in progress

- `StorageManager::commit_authorized_foreign_key_ddl` accepts add/drop batches for foreign keys on existing tables. Source authority is table ownership or inherited `ALTER` on the table/schema. Creation separately requires inherited `REFERENCES` on the target table or every referenced key column; drop requires only source `ALTER`.
- The route requires paired constraint/object/target-dependency records and write-epoch advances for both source and referenced tables. It guards the authenticated actor, full membership graph, source/target table and schema records, referenced key/column records, and applicable authority grants through publication. Request, membership, column, and grant-lookup work is bounded.
- Real-redb regressions cover column-level `REFERENCES` with table `ALTER`, denial when either permission is missing, removal without target `REFERENCES`, durable outcomes, and atomic source/target epoch publication. The same authorized DDL test stages writers against both source and referenced tables before adding the FK, then verifies both old writers conflict and publish no rows after the paired epoch increments. With the FK present, DROP COLUMN attempts on both its referencing and referenced columns return `ColumnHasDependencies`, and DROP TABLE on the referenced table returns `TableHasDependencies`; each records an ABORTED outcome and preserves the FK/catalog records.
- Contracts: [schema DDL](../../specs/ddl-schema.md) and [permission DDL](../../specs/ddl-permissions.md).
- Verification: focused real-redb FK authority, dual-table stale-writer, FK-endpoint DROP COLUMN RESTRICT, and referenced-table DROP RESTRICT regressions passed. In the configured Nix shell, workspace build, format check, warning-denied Clippy, all workspace tests, and `git diff --check` passed for referenced-table rejection. Tool versions were Rust/Cargo 1.95.0, rustfmt 1.9.0, Clippy 0.1.95, and TinyCC 0.9.27-unstable-2025-01-06.
- Remaining: authorization routes for NOT NULL, CHECK, PRIMARY KEY, and UNIQUE DDL; deterministic authorization-change race coverage; and authenticated control-channel integration remain open.

## SM-5 dependency: bounded NOT NULL validation — in progress

- A catalog commit that changes an existing column from nullable to non-nullable scans persisted rows under the table DDL fence, rejects NULL values, and aborts atomically when the 65,536-row or 64 MiB row-record bound is exceeded. Nullability-only changes preserve the tuple schema version and advance the table write epoch.
- A real-redb regression verifies rejection without publication when a stored row contains NULL, then accepts the DDL after the row is updated and confirms startup validation succeeds.
- Verification: prescribed Nix-shell build, formatting, Clippy with warnings denied, and all 212 workspace tests passed.
- Contract updates: [schema DDL](../../specs/ddl-schema.md) and [catalog payloads](../../specs/catalog-payloads.md).
- Remaining: action-specific CHECK, PRIMARY KEY, and UNIQUE authorization routes remain open.

## SM-5 dependency: NOT NULL, CHECK, PRIMARY KEY, and UNIQUE DDL authorization — in progress

- `StorageManager::commit_authorized_constraint_ddl` accepts paired NOT NULL, CHECK, PRIMARY KEY, and UNIQUE constraint changes on existing tables, with required object/dependency records and a table write-epoch advance. Key constraints include their owned unique backing-index definition, object, and kind-4 edge. It permits the table owner or inherited `ALTER` on the table/schema, verifies a login-enabled actor and successful audit attribution, and guards membership, ownership, and grant inputs through fenced publication. PRIMARY KEY columns must already be non-nullable.
- Red evidence: the managed DDL regression failed with `DdlAuthorizationDenied` for a user holding `ALTER` on the table before the authorized route was connected.
- CHECK additions bind a deterministic row-local Boolean expression to the final schema and capture row keys/versions under the table fence. The serialized writer rechecks those rows before publishing. Backing-index creation/removal uses the existing index planner and atomically publishes the index records, memberships, constraint, audit, sequence, and outcome; the planner permits a paired write-epoch-only table update while continuing to reject a schema change on populated tables.
- A real-redb regression covers denied constraint DDL without sequence publication, successful NOT NULL and CHECK add/drop, rejection of a CHECK that fails on a stored row, authorized UNIQUE and PRIMARY KEY add/drop with owned index backfill, and startup validation after each publication.
- Verification: `cargo build --workspace --locked`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo test --workspace --locked` passed in the prescribed Nix shell. Changed Markdown local-link checks and `git diff --check` passed.
- Contract updates: [schema DDL](../../specs/ddl-schema.md) and [permissions](../../specs/ddl-permissions.md).
- Remaining: deterministic authority-change race coverage and authenticated control-channel integration remain open.

## SM-5 dependency: token inventory — in progress

- `StorageManager::list_tokens` returns stable token IDs, labels, issue/expiry times, and revocation metadata from one redb catalog snapshot. The public metadata type has no verifier or secret field.
- Users can list their own tokens. Cross-user inventory requires current inherited `MANAGE_USERS` or database-administrator membership; missing target users and unauthorized cross-user requests share the same result.
- A real-redb regression covers self-inventory, inherited `MANAGE_USERS`, denial for an unrelated user, and visibility of revocation state after the revoke commit.
- Verification: in the prescribed Nix shell, locked workspace build, formatting check, Clippy with warnings denied, and all 216 workspace tests passed. Changed Markdown local links and `git diff --check` passed.
- Contract updates: [users and roles](../../specs/ddl-identity.md) and [storage manager](../../specs/storage-manager.md).
- Remaining: token request rate limits, bootstrap secret handoff, gateway/supervisor integration, authenticated control-channel integration, and user/role DROP RESTRICT remain open.

## SM-5 dependency: credential-backed last-administrator protection

- Red evidence: an identity DDL regression showed that revoking the only unexpired administrator token committed even though the remaining verifier was expired. The test expected `DdlAuthorizationDenied` and observed `Ok(3)` before implementation.
- The serialized writer now checks final membership, principal login state, and family-16 verifier state together. A usable administrator must be in the reserved database-admin closure, have `LOGIN`, and have at least one unrevoked verifier valid at trusted UTC commit time.
- The guard runs for membership changes, login disablement, and active credential revocation, then validates the serialized final catalog. Token issuance remains available to add or restore a credential before old-token revocation.
- Real-redb regressions cover refusal to disable/revoke one administrator when the only other login admin has expired credentials, refusal to change membership when the resulting administrators have only expired credentials, and successful replacement-first token rotation.
- Verification: in the prescribed Nix shell, the locked workspace build, formatting check, Clippy with warnings denied, and all 218 workspace tests passed. Changed Markdown links and `git diff --check` passed.
- Contract: [users and roles](../../specs/ddl-identity.md).
- Remaining: bootstrap secret handoff, authenticated control-channel/gateway integration, token request rate limits, user/role DROP RESTRICT, and deterministic authority-change race coverage remain open.

## SM-5 dependency: durable identity tombstones

- Red evidence: extending the catalog registry test to family 17 failed because the identity-tombstone family had no codec schema.
- Logical catalog schema version 2 adds family 17 for immutable ID-only tombstones and bootstrap storage-object code 217. Startup and serialized commits reject tombstones that overlap live object or principal IDs; identity creation rejects a tombstoned ID and guards the absent tombstone key through publication. Version-1 catalogs fail closed until an offline migration path exists.
- Revoked credential verifiers may remain attached to a tombstoned user ID, while unrevoked verifiers still require a live LOGIN user.
- Real-redb regressions cover accepted tombstones, startup rejection of ID reuse, identity-creation refusal, and catalog-commit refusal to tombstone a live principal.
- Verification: the prescribed Nix-shell locked workspace build, formatting check, Clippy with warnings denied, and all 220 workspace tests passed. Changed Markdown local links and `git diff --check` passed.
- Contract updates: [catalog payloads](../../specs/catalog-payloads.md), [catalog lifecycle](../../specs/catalog-lifecycle.md), [data dictionary](../../specs/data-dictionary.md), and [users and roles](../../specs/ddl-identity.md).
- Remaining: user/role DROP RESTRICT must publish the tombstone and remove blocked references atomically.

## SM-5 dependency: restricted user and role deletion

- Red evidence: the unreferenced-user deletion regression initially failed with `InvalidCatalog` because the authorized identity route rejected deletes and family-17 tombstones.
- The identity DDL route now accepts paired principal/object deletion plus a new tombstone, requires `MANAGE_USERS` or `MANAGE_ROLES`, rejects the reserved `database_admin` role, and performs bounded RESTRICT scans for object ownership/parent references, membership/grantor references, grants, policy targets, and dependency edges. Existing serialized final-catalog validation rejects concurrent references that were absent from the preflight snapshot.
- Deleting a user requires every active family-16 credential to be revoked in the same batch. The writer checks that at least one login-enabled administrator with a current credential remains. The principal/object removal, token revocations, tombstone, audit, outcome, and commit sequence publish atomically.
- Real-redb regressions cover unreferenced user and role drops with tombstone persistence, user-token revocation in the same batch, refusal to omit an active-token revocation, a grant reference that blocks `RESTRICT`, and persisted-catalog validation after deletion.
- Contract updates: [users and roles](../../specs/ddl-identity.md) and [storage manager](../../specs/storage-manager.md).
- Remaining: bootstrap secret handoff, authenticated control-channel/gateway integration, token request rate limits, deterministic authority-change race coverage, and role/user DROP integration through a SQL/control frontend remain open.

## SM-5 dependency: token lifecycle rate limiting

- Red evidence: with the limiter disabled, the real-redb control-path regression expected `RateLimited` on the sixth request across two manager handles but received token metadata.
- `issue_token`, `revoke_token`, and `list_tokens` now use a monotonic rolling 60-second budget per authenticated actor. The default is 30 requests per minute, configured through `StorageManagerConfig`; lower limits take effect across live handles for the same redb `Database` object. The actor-history map is bounded at 4,096 and stale entries are pruned. A full map and poisoned per-actor state fail closed.
- An integration regression verifies the budget across issue, inventory, and revoke calls and across independently constructed manager handles. Zero limits are rejected during configuration validation.
- Contract updates: [users and roles](../../specs/ddl-identity.md) and [storage manager](../../specs/storage-manager.md).
- Verification: prescribed Nix-shell workspace build, formatting check, Clippy with warnings denied, and all 222 workspace tests passed. Changed Markdown local links and `git diff --check` passed.
- Remaining: token-authentication failures still need gateway-level throttling because storage admission lacks peer/source context; bootstrap secret handoff, authenticated control-channel integration, deterministic authority-change race coverage, and SQL/control frontend integration remain open.

## SM-4 / SM-5 dependency: catalog authority-change race validation

- Added a barrier-coordinated real-redb race between an invocation staging a write under a pinned principal authority epoch and a catalog commit advancing that epoch. The row write either commits before the authority change, or the authority change commits first and the invocation records ABORTED; row visibility, outcome, and commit sequence are checked against that order. The existing sequential regression covers the change-first ordering directly.
- Verification: prescribed Nix-shell workspace build, formatting check, Clippy with warnings denied, and all 228 workspace tests passed.
- Contracts: [authorization](../../specs/authorization.md) and [concurrency/MVCC](../../specs/concurrency-mvcc.md).
- Remaining: concurrent schedule coverage across each action-specific grant, membership, policy, identity, and credential route remains open; the supervisor/control-channel integration is not implemented.

## SM-6 dependency: executor relation-resolution dispatch

- Behavior: executor role dispatch for `resolve_relation` decodes a versioned bounded payload, resolves through the connection-scoped session and returns a stable relation ID plus opaque handle. Hidden and absent qualified names share one unavailable response; planner role calls are denied before storage lookup.
- Changed code and fixture: [executor IPC dispatcher](../../engine/src/executor_ipc.rs), [transaction regression](../../engine/tests/transaction.rs), and [IPC protocol](../../specs/ipc-protocol.md).
- Red evidence: the new regression first failed to compile because the executor dispatch module/API did not exist. This established the missing implementation surface; it was not an assertion-level red test.
- Checks: `cargo build --workspace --locked`, `cargo fmt --all -- --check`, warning-denied workspace Clippy, full workspace tests, and `git diff --check` passed in the prescribed Nix shell. Changed Markdown local links passed.
- Remaining: supervisor socket provisioning, scan/cursor/mutation dispatch and reauthorization, SQL integration, and administrative discovery.

## SM-6 dependency: executor relation-resolution stream loop

- Behavior: a bound stream can serve repeated `resolve_relation` frames with one connection-owned handle registry. Duplicate request IDs and more than 256 requests fail closed; clean disconnect and protocol/request/write errors drop the registry and durably abort the bound invocation.
- Changed code and fixture: [executor IPC dispatcher](../../engine/src/executor_ipc.rs), [transaction socket regression](../../engine/tests/transaction.rs), and [IPC protocol](../../specs/ipc-protocol.md).
- Evidence: a real Unix stream pair sends an authorized resolution, half-closes, receives the matching response, and verifies the persisted ABORTED outcome on disconnect. The focused test passes in the prescribed Nix shell.
- Remaining: supervisor socket provisioning, scan/cursor/mutation dispatch and reauthorization, SQL integration, and administrative discovery.
- Next smallest task: add authorized `open_scan` and bounded cursor state to the same session before exposing row batches.

## SM-3 dependency: catalog-derived authorized row scanning

- Behavior: `InvocationTransaction::scan_catalog_authorized_rows` constructs its authorized projection from the pinned table schema, role closure, data grants, and applicable row/cell policies, then scans the pinned snapshot plus private overlay. Callers cannot supply grant or policy vectors to authorize the operation.
- Changed code and fixture: [transaction implementation](../../engine/src/transaction.rs), [catalog scan regression](../../engine/tests/transaction.rs), and [storage-manager contract](../../specs/storage-manager.md).
- Red evidence: the regression initially failed to compile because the catalog-derived scan API did not exist. During fixture completion, row-policy filtering and cell redaction behaved as specified; the valid real-redb fixture was completed with table SELECT and row/cell ALLOW policies.
- Verification: workspace build, `cargo fmt --all -- --check`, warning-denied workspace Clippy, full workspace tests, focused scan regression, and `git diff --check` passed in the prescribed Nix shell.
- Remaining: serialize/bind its continuation to executor IPC, enforce response-byte limits, and test this derived path against policy/grant changes before finalization.
- Next smallest task: add a connection-local scan cursor token that wraps `RowScanCursor` without exposing row IDs or accepting cross-connection tokens.

## SM-6 dependency: connection-local executor scan cursors

- Behavior: `ExecutorMetadataConnection` now issues kind-7 opaque scan cursors, consumes one token per page, revalidates the relation handle and delegates to the pinned-catalog authorized scan path. It rejects replay and cross-connection use, and can close a scan to release the active scan slot.
- Changed code and fixture: [transaction implementation](../../engine/src/transaction.rs), [cursor wire type](../../engine/src/metadata_discovery.rs), and [real-redb pagination regression](../../engine/tests/transaction.rs).
- Evidence: the regression scans two persisted rows across one-row pages, confirms the required final empty page for EOF, rejects replay and replacement-connection use, and rejects the cursor through another kind's decoder. Workspace build, format check, warning-denied Clippy, full workspace tests, and `git diff --check` all pass in the prescribed Nix shell.
- Red evidence: an early assertion expected a full one-row page to prove EOF; the existing scan contract requires an additional page to confirm EOF, so the regression was corrected to consume the continuation and final empty page.
- Remaining: encode/decode `open_scan`, `next_batch`, and `close_cursor` IPC payloads, enforce output-byte bounds, and test the derived path against concurrent policy/grant changes.
- Next smallest task: define the bounded wire payload for opening a scan from a resolved relation handle and validate requested row-count limits.

## SM-6 dependency: executor scan and batch IPC

- Behavior: executor resolve responses now include the authorized declared shape. Version-1 `open_scan`, `next_batch`, and `close_cursor` payloads dispatch through the connection-owned session. The wire path supports ordinal-ordered column subsets, candidate and byte caps, VALUE/NULL/REDACTED cells, and opaque row handles; it never serializes row IDs.
- Changed code and fixtures: [executor IPC codec/dispatcher](../../engine/src/executor_ipc.rs), [row-handle and cursor ownership](../../engine/src/transaction.rs), [real-redb IPC regression](../../engine/tests/transaction.rs), and [IPC protocol contract](../../specs/ipc-protocol.md).
- Evidence: the real-redb regression resolves the catalog shape, opens a selected-column scan, transfers two visible rows through one-use cursor batches under a 48-byte response cap, decodes canonical BYTEA values and opaque row handles, confirms the final empty EOF page, and verifies close/replay behavior.
- Red evidence: the first resolver-response decode rejected ordinal zero because the new decoder assumed one-based ordinals; the catalog stores zero-based ordinals. The decoder now preserves that contract.
- Verification: workspace build, format check, warning-denied Clippy, full workspace tests, all 168 changed-document local links, and `git diff --check` pass in the prescribed Nix shell.
- Remaining: test max-byte failure through the stream loop, exercise NULL/REDACTED wire states and hostile payloads, and connect the provisioned supervisor socket to this loop.
- Next smallest task: add StageInsert/StageUpdate/StageDelete request dispatch that consumes row handles and rechecks catalog authority.

## SM-6 dependency: catalog-authorized executor mutations

- Behavior: executor IPC now decodes full-shape INSERT values and unique ordinal UPDATE patches, and dispatches INSERT/UPDATE/DELETE through connection-local handles. Storage rebuilds write projection, checks, foreign keys, and index definitions from pinned catalog records; policy-referenced fields cannot be updated, and successful UPDATE/DELETE invalidates row handles.
- Changed code and fixture: [executor IPC](../../engine/src/executor_ipc.rs), [catalog-derived write projection and handle staging](../../engine/src/transaction.rs), [mutation codec regressions](../../engine/tests/executor_ipc.rs), [real-redb delete dispatch regression](../../engine/tests/transaction.rs), and [IPC contract](../../specs/ipc-protocol.md).
- Evidence: focused mutation codecs and a real-redb executor INSERT/UPDATE/DELETE regression pass. The fixture seeds indexed rows through authorized executor INSERT, stages a second INSERT, UPDATE, and DELETE through IPC, commits atomically, and verifies persisted tuple values plus row removal from a fresh snapshot. It also verifies catalog-derived grants/policies, staged visibility, and handle invalidation.
- Red evidence: an early mixed-fixture commit returned `InvalidIndex` because its existing rows came from the low-level writer and lacked catalog index locators; reseeding them through authorized INSERT made index maintenance valid. A first seed attempt also hit `IndexOverlayLimitExceeded` with an undersized fixture budget; the fixture now allows its catalog-derived index entries. Workspace build, format, warning-denied Clippy, full tests, local Markdown links, and diff checks pass.
- Remaining: generated identity allocation remains unsupported; broader FK/unique-index mutation cases still need dedicated executor-path fixtures.
- Next smallest task: bind the generic executor request loop to supervisor-provisioned socket lifecycle.

## SM-6 dependency: executor batch overflow abort

- Behavior: a single authorized row that exceeds the caller's `max_bytes` aborts the invocation before any partial row is returned.
- Changed fixture: [real-redb executor batch regression](../../engine/tests/transaction.rs).
- Evidence: the request is rejected at a 46-byte cap for a 47-byte encoded row, and the invocation's durable outcome is ABORTED. The 48-byte pagination case still succeeds.
- Verification: workspace build, format check, warning-denied Clippy, full workspace tests, all 158 changed-document local links, and `git diff --check` pass.
- Remaining: exercise this failure through a live Unix stream and validate NULL/REDACTED and malformed response edge cases.
- Next smallest task: add a stream-level multi-operation executor request regression before socket provisioning work.

## SM-6 dependency: multi-operation executor stream

- Behavior: the generic `serve_executor` entry point keeps connection-owned relation and cursor handles across `resolve_relation`, `open_scan`, and `close_cursor` requests on one Unix stream. Clean EOF ends only the executor session and leaves terminal outcome authority to the supervisor; protocol failures durably abort. The original resolver-named function remains as a compatibility wrapper.
- Changed code and fixture: [executor stream loop](../../engine/src/executor_ipc.rs) and [Unix-stream regression](../../engine/tests/transaction.rs).
- Evidence: the focused stream regression confirms a close acknowledgement, no outcome on clean EOF, and an ABORTED outcome only after the caller explicitly aborts.
- Red evidence: an initial attempt to include `next_batch` in the relation-discovery-only fixture failed with `CorruptCatalog`; that fixture lacks the scan policy records. The richer catalog-authorized regression already exercises `next_batch` through the same dispatcher, while this stream test covers persistent handle state and cursor lifecycle.
- Verification: workspace build, format check, warning-denied Clippy, full tests, all 159 changed-document local links, and `git diff --check` pass.
- The generic stream entry-point rename and compatibility wrapper also pass the full workspace validation; 160 local Markdown links resolve.
- Remaining: run scan and mutation operations through a live stream with the full authorization fixture; implement supervisor observation of worker completion and its trusted finalize/abort channel.
- Next smallest task: bind executor and planner streams to a supervisor-owned invocation lifecycle.

## SM-6 dependency: generated UUID identities

- Behavior: executor INSERT now uses DEFAULT markers independently from NULL, allocates UUID identities only for DEFAULT, rejects explicit generated values and updates to identity columns, and validates collisions against the serialized final table contents.
- Changed code and contract: [transaction and commit validation](../../engine/src/transaction.rs), [executor INSERT codec](../../engine/src/executor_ipc.rs), [IPC protocol](../../specs/ipc-protocol.md), and [schema DDL contract](../../specs/ddl-schema.md).
- Verification: `catalog_backed_executor_generates_uuid_identity_and_rejects_override_and_update` uses a real redb catalog and executor metadata connection to verify DEFAULT materialization and commit, durable aborts for explicit UUID override and generated-column update, and deterministic duplicate UUIDs through the trusted staging path. A duplicate against a committed row is rejected as `UniqueViolation`; two concurrent snapshots staging the same new UUID produce exactly one committed row and one durable ABORTED outcome. The focused test, locked workspace build, format check, warning-denied Clippy, and full serial workspace suite passed; the latter ran in the delegated `systemd --user` cgroup service with the sandbox fixture environment.
- Remaining: broader SQL runtime acceptance remains part of executor/supervisor integration work.

## SM-6: periodic bounded submission expiry — implemented

- Behavior and normative contract: [supervisor staging lifecycle](../../specs/supervisor.md). Expired pre-start reservations must be reaped even while the service receives no client traffic, and each maintenance pass must have a bounded scan cost.
- Implementation: `StagingStore::reap_expired(max_scanned)` scans an ordered reservation map with a rotating cursor, visits at most the caller's budget, and eventually covers all live reservations. Opportunistic staging operations use a 256-entry pass. The gateway service runs the same 256-entry reaper on each accept-loop iteration and stops if the staging state is poisoned.
- Red evidence: disabling the service-loop reaper made `idle_gateway_service_periodically_reaps_expired_staging` fail: closing the expired connection returned its still-live reservation ID. Restoring periodic maintenance made the test pass. The bounded-pass regression creates three expired reservations, confirms one is returned per one-entry pass, and checks all are unavailable afterward.
- Verification: focused reaper and idle-service tests passed. In the prescribed Nix shell, `cargo build --workspace --locked`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked`, and `git diff --check` passed. The workspace had 22 passing library unit tests; the commit integration target had 109 passing and 2 pre-existing cgroup-dependent tests ignored.
- Remaining: production manifest validation and a supervisor executable still depend on the manifest format and process admission contracts left open in the specs; gateway/supervisor process-level execution remains incomplete.

## SM-6: requested-scope decoding and enforcement — implemented

- Behavior and normative contracts: [client SDK](../../specs/client-sdk.md), Requested Capability Scope v1; [client protocol](../../specs/client-protocol.md), PREPARE/SEAL/START; [authorization](../../specs/authorization.md); and [storage manager](../../specs/storage-manager.md). The client's requested scope is a ceiling intersected with current catalog-derived grants at START, never a grant source.
- Implementation: added the bounded canonical `PSC1` decoder in [submission_scope.rs](../../engine/src/submission_scope.rs), validates it during staging SEAL, decodes again at START, and applies the intersection to every transaction-built `AuthorizationContext` in [transaction.rs](../../engine/src/transaction.rs). The supervisor admission path accepts the decoded scope in [storage_manager.rs](../../engine/src/storage_manager.rs), and both legacy and executor launch START paths pass it from [client_protocol.rs](../../engine/src/client_protocol.rs).
- Red evidence: removing the grant-intersection filter made `invocation_scope_intersects_requested_capabilities_with_catalog_grants` fail because DELETE authority outside the request remained present. Removing the SEAL decoder check made `sealing_rejects_invalid_v1_scope_even_when_custom_validator_accepts_it` fail because malformed `scope` sealed successfully. Both passed with their checks restored.
- Verification: canonical decoder, malformed-input, seal, START, and scope-intersection regressions passed. In the prescribed Nix shell, `cargo build --workspace --locked`, `cargo fmt --all -- --check`, warning-denied workspace Clippy, `cargo test --workspace --locked`, and `git diff --check` passed. The suite included 27 passing library unit tests and 109 commit integration tests; 2 cgroup-dependent sandbox tests remain ignored.
- Decisions: `PSC1` uses a big-endian u16 count, strict bytewise record ordering, nonzero IDs, five operation codes, three target kinds, and a 200-capability ceiling (6,806-byte maximum encoding).
- Remaining: manifest v1 encoding/compatibility validation, service executable integration, and sandboxed scope-to-storage acceptance remain open.

## SM-6: manifest v1 compatibility and ELF dependency validation — implemented

- Behavior and normative contracts: [client SDK](../../specs/client-sdk.md), Artifact Manifest v1; [client protocol](../../specs/client-protocol.md), PREPARE/SEAL; [supervisor staging](../../specs/supervisor.md); and [toolchain](../../specs/toolchain.md). Admission accepts only canonical `PMF1` Linux x86_64 manifests for configured CPU/runtime/SDK profiles and dependencies.
- Implementation: added bounded `PMF1` decoding and a configured `VersionedSubmissionMetadataValidator`. SEAL now supplies its immutable artifact bytes to metadata validation; the production validator parses ELF64 program/dynamic tables without loading or executing the library, rejects loader search/audit/filter tags, and requires the actual `DT_NEEDED` set to equal the declared allowlisted dependencies.
- Red/green evidence: the manifest test suite covers unsupported target/ABI/protocol/symbols, malformed/trailing/unsorted/duplicate data, profile and dependency rejection, malformed ELF, prohibited dynamic tags, and mismatch between manifest and artifact dependencies. Focused tests pass with validation enabled.
- Red/green check: temporarily removing the manifest-versus-ELF dependency comparison made `elf_dependency_reader_checks_loaded_names_without_loading_the_artifact` fail because a manifest listing libc and libm accepted an ELF listing only libc. Restoring the comparison made it pass.
- Verification: Rust/Cargo 1.95.0, rustfmt 1.9.0, Clippy 0.1.95, TinyCC 0.9.27-unstable-2025-01-06 were verified in the prescribed Nix shell. Workspace build, format check, warning-denied Clippy, all workspace tests, and `git diff --check` passed. The engine library had 33 passing unit tests; the commit target had 109 passing and 2 pre-existing cgroup/TinyCC fixture tests ignored.
- Decisions: manifest encoding uses the 28-byte `PMF1` header and bounded canonical body specified in the SDK contract. Deployment-specific CPU/runtime profile and dependency allowlist remain configuration inputs, since the repository has not selected its final TinyCC/runtime profile.
- Remaining: supervisor executable integration, production profile configuration, and sandboxed scope-to-storage acceptance remain open.

## SM-6: manifest validation through staging SEAL — verified

- Behavior and normative contracts: [client protocol](../../specs/client-protocol.md), PREPARE/SEAL; [client SDK](../../specs/client-sdk.md), Artifact Manifest v1; and [supervisor staging](../../specs/supervisor.md). SEAL must validate the artifact bytes that were uploaded and pinned in that reservation.
- Regression: [submission_manifest.rs](../../engine/src/submission_manifest.rs) now stages canonical metadata and ELF fixtures through `StagingStore::prepare`, `upload_chunk`, and `seal`. The matching dependency set seals successfully; an ELF missing a declared dependency is rejected as `InvalidMetadata`.
- Red/green evidence: temporarily making SEAL call metadata-only `validate` caused the mismatch reservation to seal successfully and the new test failed at the expected assertion. Restoring `validate_artifact` makes it pass.
- Verification: focused regression passed in the prescribed Nix shell. Workspace build, format check, warning-denied Clippy, all workspace tests, and `git diff --check` passed. The engine library had 34 passing unit tests; the commit target had 109 passing and 2 pre-existing cgroup/TinyCC fixture tests ignored.
- Remaining: supervisor executable integration, production compatibility-profile configuration, and execution inside the sandbox remain open.

## SM-6: verify ABI exports before SEAL — implemented

- Behavior and normative contracts: [client SDK](../../specs/client-sdk.md), Artifact Manifest v1 and its acceptance plan; [supervisor staging](../../specs/supervisor.md). A manifest naming ABI entry points is accepted only when the pinned ELF exports both symbols with the required ELF symbol kinds.
- Implementation: the bounded ELF64 dynamic-table reader now requires and validates `DT_HASH`, `DT_SYMTAB`, and `DT_SYMENT` when checking an artifact. It walks bounded dynamic symbols and accepts the descriptor only as a defined externally visible OBJECT and the entry point only as a defined externally visible FUNC. No loader or constructor is invoked.
- Red/green evidence: before the check, the validator accepted an artifact missing `prdb_query_descriptor_v3`; `elf_dependency_reader_checks_loaded_names_without_loading_the_artifact` failed at the expected assertion. The test also covers an object symbol exported with FUNC type. Both pass with the check enabled.
- Toolchain evidence: the configured TinyCC 0.9.27-unstable-2025-01-06 output for a shared object using `puts` contained `libc.so.6`, `ld-linux-x86-64.so.2`, both SysV/GNU hash tables, and the required ABI-named exports. A temporary Rust validation harness accepted it when the manifest and profile declared those dependencies.
- Verification: manifest-focused tests and full workspace build, formatting, warning-denied Clippy, all workspace tests, and `git diff --check` passed. The engine library had 34 passing unit tests; the commit target had 109 passing and 2 pre-existing cgroup/TinyCC fixture tests ignored.
- Remaining: supervisor executable integration and confined query execution acceptance remain open.

## SM-6: authenticated pre-invocation schema discovery — implemented

- Behavior and normative contract: [metadata discovery](../../specs/metadata-discovery.md). A client can build a requested scope before invocation admission from schemas visible to its authenticated principal.
- Implementation: `StorageManager::discover_token_authorized_schemas` opens a request-scoped transaction snapshot, validates the token against that snapshot, derives catalog schema USAGE visibility, and consumes transaction-local pages internally. It caps full results at 4,096 records and 1 MiB, returns no partial result on overflow, and creates no invocation admission or durable outcome.
- Regression: the credential lifecycle integration test now verifies that a valid token sees the public `information_schema` namespace and that a wrong secret is rejected.
- Verification: the focused `token_admission_checks_expiry_secret_revocation_and_commit_guards` real-redb test passed in the prescribed Nix shell. Initial sandbox Nix access was denied; the authorized `nix-shell` invocation succeeded.
- Later verified by the selected-scope TCP and metadata paging increments below.

## SM-6: expose schema discovery through the client control channel — implemented

- Behavior and normative contract: [client protocol](../../specs/client-protocol.md), ADMIN read envelope and `metadata.schemas.list`; [metadata discovery](../../specs/metadata-discovery.md).
- Implementation: the versioned read decoder accepts only `phase=read`, operation version 1, operation `metadata.schemas.list`, and empty arguments. The connection-owned handler revalidates its token and emits bounded schema IDs/names from the storage snapshot API. Existing gateway administrative-operation dispatch carries this request over the control channel; no invocation is admitted.
- Regression evidence: strict decoder tests reject arguments and mutation phase; the real-redb gateway dispatch test authenticates a non-admin control session and receives the public schema in the response.
- Verification: focused decoder and real-redb gateway-dispatch regressions passed. `cargo build --workspace --locked`, `cargo fmt --all -- --check`, warning-denied workspace Clippy, and `cargo test --workspace --locked` passed. Commit integration had 109 passing and 2 cgroup-dependent tests ignored.
- Later extended with session-bound network pages; see the metadata paging increment below.

## SM-6: authenticated relation discovery over the control channel — implemented

- Behavior and normative contract: [metadata discovery](../../specs/metadata-discovery.md), relation-grant filtering; [client protocol](../../specs/client-protocol.md), `metadata.relations.list`.
- Implementation: `StorageManager::discover_token_authorized_relations` authenticates and filters under one request-scoped catalog snapshot, consumes the transaction-bound cursors internally, and bounds the complete result by record count and encoded ID/name bytes. The client read requires exactly one schema ID. Hidden and absent schemas serialize identically to an authorized empty schema.
- Regression evidence: strict request decoding checks the schema ID and rejects extra arguments. Real-redb gateway dispatch authenticates an ordinary principal, returns only its granted `visible_app` schema, exposes the granted `metrics` relation, and returns its `reading` column shape. A second table in the visible schema without a data grant is omitted; an ungranted schema containing a table and an absent schema both return the indistinguishable empty relation array. The reads do not start invocations.
- Verification: focused decoder and real-redb gateway-dispatch regressions passed. `cargo build --workspace --locked`, `cargo fmt --all -- --check`, warning-denied workspace Clippy, and `cargo test --workspace --locked` passed. Commit integration had 109 passing and 2 cgroup-dependent tests ignored.
- Later extended with session-bound network pages; see the metadata paging increment below.

## SM-6: authenticated column-shape discovery over the control channel — implemented

- Behavior and normative contract: [metadata discovery](../../specs/metadata-discovery.md), declared columns; [client protocol](../../specs/client-protocol.md), `metadata.columns.list`.
- Implementation: `InvocationTransaction::describe_catalog_authorized_columns_page` derives the scope and grant set from its pinned catalog; the storage-manager adapter authenticates the token in a request-scoped snapshot and consumes its pages. It exposes only stable column IDs, names, zero-based ordinals, canonical scalar type names, and nullability. Built-in `information_schema` descriptors share the same stable relation and column IDs as ordinary query metadata. `MetadataClient` negotiates control mode, authenticates, parses all three metadata response shapes into typed records, and provides those records directly to `RequestedScopeBuilder` callers.
- Regression evidence: strict decoder checks require both schema and relation IDs. Real-redb gateway dispatch obtains the public `information_schema.columns` shape and the granted user table's `reading` integer column through an ordinary principal's control connection.
- Verification: the new `client_metadata` TCP test proves typed schema/relation/column decoding, secret zeroization, ordinal-gap preservation, and requested-scope construction. A real-redb transaction regression verifies column discovery through a column-level SELECT grant, and gateway dispatch verifies built-in columns. `cargo build --workspace --locked`, `cargo fmt --all -- --check`, warning-denied workspace Clippy, and `cargo test --workspace --locked` passed; commit integration had 109 passing and 2 cgroup-dependent tests ignored.
- Later extended with session-bound network pages; complete snapshots remain bounded by the storage adapter's 1 MiB cap.

## SM-6: align relation discovery with table catalog membership — fixed

- Behavior and normative contract: [storage manager](../../specs/storage-manager.md), grant-filtered relation discovery; [metadata discovery](../../specs/metadata-discovery.md), pre-invocation discovery.
- Red evidence: the gateway regression initially authenticated with an admin token and could not prove ordinary-principal filtering. After switching to an ordinary principal with explicit schema USAGE and table SELECT grants, the test showed schema discovery worked but relation discovery returned an empty list. The table catalog held the relation and grant, while schema records had no relation ID list.
- Implementation: both relation enumeration and relation-grant loading now scan the pinned table catalog and select records whose stored schema ID matches the requested schema. The regression verifies the granted schema, relation, and column over the real gateway path; an ungranted table within that visible schema and a second schema with no USAGE grant remain hidden.
- Verification: the focused real-redb gateway regression passed. Rust 1.95.0, Cargo 1.95.0, rustfmt 1.9.0, Clippy 0.1.95, and TinyCC 0.9.27-unstable-2025-01-06 were verified in the prescribed Nix shell. Workspace build, format check, warning-denied Clippy, all workspace tests, and `git diff --check` passed; the commit integration target had 109 passing tests and 2 cgroup-dependent tests ignored.
- Later extended with session-bound network pages; complete snapshots remain bounded by the storage adapter's 1 MiB cap.

## SM-6: build requested scope from authenticated TCP discovery — implemented

- Behavior and normative contract: [client SDK](../../specs/client-sdk.md), Requested Capability Scope v1 and `MetadataClient`; [metadata discovery](../../specs/metadata-discovery.md); [storage manager](../../specs/storage-manager.md), token-authenticated pre-invocation reads.
- Regression: the real-redb authorization integration test now runs `MetadataClient` over loopback TCP against a `ClientConnectionSession` backed by the real `StorageManager`. An ordinary user's bearer token returns only the granted schema and relation, then the declared granted column. `RequestedScopeBuilder` produces and decodes PSC1 with schema USAGE and the selected column SELECT capability. The same fixture omits an ungranted relation in the visible schema and a separate schema without USAGE.
- Evidence: the focused `storage_manager_runtime_and_metadata_reads_enforce_authorization` integration test passed. Existing gateway-dispatch checks in that fixture independently verify the versioned gateway ADMIN read route and no-invocation behavior. Workspace build, format check, warning-denied Clippy, and all workspace tests passed; commit integration had 109 passing tests and 2 cgroup-dependent tests ignored.
- Later implemented by the resumable metadata paging increment below. The storage methods still return bounded full lists; pagination is owned by the connection session.

## SM-6: resumable metadata pages across control requests — implemented

- Behavior and normative contract: [client protocol](../../specs/client-protocol.md), metadata `limit`/`cursor`; [metadata discovery](../../specs/metadata-discovery.md); [client SDK](../../specs/client-sdk.md), `MetadataClient` page methods.
- Implementation: paged schema, relation, and column reads capture each bounded complete result from one token-authenticated storage snapshot in the connection-owned `ClientConnectionSession`. The session retains at most one result (bounded by the 1 MiB storage cap), emits at most 128 records per page, returns one-use random 16-byte cursors, and expires continuations after 60 seconds idle. Cursors bind the operation, filter, limit, principal, and role closure. New metadata queries replace an outstanding cursor; role changes, replay, filter/limit substitution, and cross-connection use reject.
- Client behavior: `MetadataClient::list_*_page` exposes cursor pages, while existing `list_*` methods follow pages with a default size of 64 and validate ordering across page boundaries.
- Regression evidence: the real-redb TCP integration test pages all three record kinds, rejects query substitution, cursor replay, and cross-connection cursor use, preserves the old column name across a concurrent rename, and observes the renamed column on a fresh query. Decoder tests cover canonical limits and cursor encodings.
- Verification: Rust 1.95.0, Cargo 1.95.0, rustfmt 1.9.0, Clippy 0.1.95, and TinyCC 0.9.27-unstable-2025-01-06 were verified in the prescribed Nix shell. `cargo build --workspace --locked`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo test --workspace --locked` passed. Commit integration had 109 passing tests and 2 cgroup/TinyCC fixture tests ignored; `git diff --check` passed.
- Remaining: server-side snapshots are connection-local and process-local; complete result snapshots remain bounded at 1 MiB, and control-session memory is retained until completion, replacement, connection close, or an attempted continuation after expiry.

## SM-1 / SM-7: repeated durable writes under a retained reader — verified

- Contract: [redb storage](../../specs/redb-storage.md) and [storage manager validation](05-validation-and-integration.md). A retained read transaction must continue to observe its pinned version while later durable writes make progress.
- Regression: the real-redb [foundation test](../../engine/tests/redb_foundation.rs) pins an initial value, completes 64 Immediate/two-phase commits of 32 KiB replacements while that reader remains open, verifies the pinned reader still sees the original bytes, and confirms a new reader sees the latest committed value.
- Verification: Rust 1.95.0 and redb 4.4.0; `cargo fmt --all -- --check` and `cargo test -p prdb-engine --test redb_foundation --locked` passed.
- Scope: this validates redb snapshot stability and writer progress for this bounded workload. It does not establish the storage manager's admission limits under sustained pressure, physical reclamation/compaction behavior, or power-loss durability.
- Remaining: power-loss durability and production-duration contention remain unverified; bounded retained-reader, concurrent-writer, and reclamation workloads are recorded below.

## SM-4 / SM-7: durable writer progress under a retained invocation snapshot — verified

- Contract: [MVCC](../../specs/concurrency-mvcc.md) and [storage manager validation](05-validation-and-integration.md). A long-lived admitted invocation keeps its original read view while independent writers durably publish later versions.
- Regression: the real-redb [transaction test](../../engine/tests/transaction.rs) holds one invocation snapshot while 32 other invocations each commit a 16 KiB replacement to the same row. The retained reader returns the original row after every commit; a new invocation reads the 32nd committed value.
- Verification: `cargo fmt --all -- --check` and `cargo test -p prdb-engine --test transaction retained_invocation_snapshot_survives_a_durable_write_burst --locked` passed.
- Scope: validates engine transaction behavior for a bounded single-row burst. It does not measure production reader scheduling, sustained write/analytical contention, file reclamation, or power-loss durability.

### Expanded retained invocation pressure — verified

- Regression: expanded `retained_invocation_snapshot_survives_a_durable_write_burst` to hold one invocation snapshot open while 64 other invocations durably replace the same row with 32 KiB values (2 MiB of committed replacement payload). After every commit, the held snapshot still reads its original value; a fresh invocation reads the last replacement.
- Verification: the focused transaction integration test passed with the pinned Nix toolchain.
- Scope: bounded invocation-write pressure only; sustained contention and power-loss behavior remain unmeasured. A separate single-reader compaction case follows.

### Concurrent retained invocation pressure — verified

- Regression: `retained_invocation_snapshot_survives_concurrent_writer_pressure` pins one invocation snapshot while four concurrent writers each publish 64 durable 32 KiB replacements to separate rows (8 MiB total across 256 commits). In each writer round, the reader also rereads all four rows and checks that its original values remain visible; a new invocation reads each writer's final value.
- Verification: formatting and the focused transaction integration test passed with the pinned Nix toolchain; the focused test completed in 1.11 seconds.
- Scope: expanded bounded concurrent pressure; extended production-duration workloads and power-loss behavior remain unmeasured.

### Physical reclamation with a retained reader — verified

- Regression: `retained_reader_blocks_compaction_until_release_then_reclaims_deleted_pages` writes 6.25 MiB of durable values, pins a read snapshot, deletes most rows, and verifies redb rejects compaction while the reader is open. The pinned snapshot still reads all original rows; after release, compaction shrinks the database file and the remaining rows are intact.
- Verification: the focused redb foundation integration test passed on redb 4.4.0.
- Scope: bounded single-reader reclamation evidence; sustained mixed read/write pressure and power-loss behavior remain unmeasured.

### Restricted-principal metadata filtering through gateway and supervisor — verified

- Regression: `authenticated_metadata_reads_cross_gateway_and_supervisor_processes` creates an owner-owned `admin_private` schema and `private_records` table, plus a separate login user/token without grants. Through the actual gateway and supervisor processes, the restricted token still discovers `information_schema` but does not receive the private schema, relations, or columns, including when it supplies the known private schema/table IDs.
- Verification: the focused process test passed in the prescribed Nix toolchain. Workspace build, format, warning-denied Clippy, all workspace tests, and `git diff --check` passed; the commit integration target had 109 passed and 3 delegated-cgroup tests ignored.
### Administrative metadata visibility through the control path — implemented

- Implementation: authenticated control metadata now adds schemas owned by the principal/active owner roles and all schemas for database administrators; relation and column control reads use the same authority check. These checks are isolated from invocation-bound schema grants. Metadata page continuations compare current visible object IDs before serving retained pages, while stable-ID renames preserve the original page snapshot.
- Regression: the process integration authenticates database-admin, schema-owner, and ungranted-reader tokens through the actual gateway and supervisor. The administrator sees the owner-created schema and relation; the owner can inspect declared columns without an explicit USAGE/data grant; the unrelated reader sees none of the private schema, relation, or columns even when supplying known IDs.
- Boundary: the same administrator and schema-owner credentials, when admitted as ordinary invocation transactions, still do not see the schema through grant-filtered `list_catalog_authorized_schemas`.
- Verification: `authenticated_metadata_reads_cross_gateway_and_supervisor_processes` and `storage_manager_runtime_and_metadata_reads_enforce_authorization` passed. Workspace build, format, warning-denied Clippy, all tests, and `git diff --check` passed; the commit target had 109 passed and 3 delegated-cgroup tests ignored.
- Cursor regression: `storage_manager_runtime_and_metadata_reads_enforce_authorization` captures a schema page for an ordinary principal, revokes that principal's schema USAGE grant in a concurrent authorized commit, and verifies the saved continuation is rejected. The same test confirms a stable-ID column rename still serves the original captured page, rejects replay, and rejects cross-connection cursors.
- Verification: the focused metadata authorization integration passed with the pinned Nix toolchain. Workspace build, format, warning-denied Clippy, all tests, and `git diff --check` passed; the commit target had 109 passed and 3 delegated-cgroup tests ignored.
- Distinct-principal SQL acceptance: `sandboxed_v3_sql_dml_sequence_is_visible_and_commits` runs the maintained broad-scope and restricted-scope TinyCC artifacts through the confined worker. The broad principal's SQL/API results include its granted schema, table, and column; the restricted principal receives `empty_roundtrip` while its artifact verifies hidden schema, table, and column predicates return no rows. The artifact uses the same SQL information_schema consumer used by ordinary queries.
- Verification: the acceptance passed again with `/sys/fs/cgroup/user.slice/user-1000.slice/user@1000.service`, whose `cpu`, `memory`, and `pids` controllers are enabled. This NixOS profile used the pinned loader/libc paths and an empty `PRDB_TEST_RUNTIME_READ_ONLY` list because no additional read-only runtime files were required.
- Terminal-loss regression: `sandboxed_v3_sql_dml_sequence_is_visible_and_commits` now completes the real worker and relays every provisional SQL result, closes the gateway before terminal delivery, requires the terminal frame write to fail, and then verifies the durable outcome is COMMITTED. The restricted-principal query still follows the normal terminal path.
- Verification: the terminal-loss acceptance passed under the delegated `user@1000.service` cgroup. The confined metadata submission, caught-query-error abort, disconnected gateway restart, and SQL DML/terminal-loss acceptances all passed with the same NixOS runtime profile. Default workspace build, format, warning-denied Clippy, and tests passed; three delegated-cgroup tests remain ignored by the default suite.
- Remaining: the five redb backend callbacks now have injected failure coverage, and bounded mixed reader/writer pressure is covered above. Power-loss durability, production-duration contention, and other supported platform/filesystem profiles remain unverified.

## SM-1 / SM-7: recover an injected redb storage write error — verified

- Regression: `redb_storage_write_error_quarantines_and_recovers_atomically` injects an error from the redb backend's `write` callback during a managed row commit. It verifies the live manager rejects new admissions after the error, then closes and reopens the database and checks that the row, outcome, and commit sequence are either all present or all absent.
- Verification: the focused test passed. Full workspace build, format, warning-denied Clippy, tests, and `git diff --check` passed; the commit integration target had 109 passing tests and 3 delegated-cgroup tests ignored.
- Scope: adds a backend-write failure point alongside the existing injected sync failure. Power loss, other storage-file operations, and other platform/durability profiles remain untested.

### Backend read failure at database open — verified

- Regression: `redb_storage_read_or_len_error_rejects_database_open` injects one-shot backend read and length failures while redb opens an existing database image and verifies each failure rejects open before a storage manager can be constructed.
- Verification: the focused library test passed.
- Scope: read and length fail-closed evidence at startup; additional file operations and platform/durability profiles remain untested.

### Compaction resize failure — verified

- Regression: `redb_compaction_resize_error_preserves_reopenable_data` injects a backend `set_len` failure during redb compaction. Compaction returns the storage error; after reopen, all live rows remain and deleted rows stay deleted.
- Verification: the focused library test passed on redb 4.4.0.
- Scope: bounded compaction-resize recovery only; other file operations, power loss, and platform/durability profiles remain untested.

### Truncated redb image fails closed at service startup — verified

- Regression: `storage_open_rejects_a_truncated_redb_image_and_releases_the_instance_lock` creates a READY instance, truncates its committed redb file to one byte, and verifies `StorageInstance::open` refuses admission. It restores the original image and verifies a subsequent open and full persisted-record validation succeed, proving the failed attempt releases the instance lock.
- Verification: the focused bootstrap integration test passed with the pinned Nix toolchain.
- Scope: fail-closed behavior for a severely truncated local image; it does not simulate torn writes, arbitrary page corruption, or power loss.

## SM-3 / SM-7: cumulative invocation scan-work budget — implemented

- Contract: [memory storage](../../specs/memory-storage.md) and [storage manager](../../specs/storage-manager.md). Per-page caps alone must not let one long-lived invocation scan unbounded row candidates or catalog metadata across repeated scans.
- Implementation: `StorageManagerConfig::max_invocation_scan_work` defaults to 65,536. Managed transactions receive this cap; trusted low-level transactions use the same default. Physical row identities are charged after merging snapshot and overlay IDs and before authorization filtering. Catalog-family records are charged before type, grant, and ownership filtering. Repeated scans consume the budget again. The catalog-family helper owns the durable abort on budget exhaustion, so callers cannot accidentally leave an invocation active after receiving `ScanBudgetExceeded`. Exceeding the cap discards staged writes and persists ABORTED.
- Red evidence: disabling the cumulative cap made the new regression continue to a third page, so its expected `ScanBudgetExceeded` assertion failed. Restoring the cap makes the test pass.
- Regression: `managed_scan_work_budget_is_cumulative_and_aborts_private_writes` consumes a two-unit budget across two one-row pages, then verifies the third page aborts and persists ABORTED without its staged write. It also verifies that metadata discovery with a one-unit cap aborts and discards a staged write. The storage-manager configuration test rejects a zero budget.
- Verification: the focused scan-work regression and full workspace build, format check, warning-denied Clippy, and test suite passed under Rust 1.95.0 with TinyCC 0.9.27-unstable-2025-01-06 after making budget-exhaustion abort local to the catalog scan helper.
- Scope: bounds physical row candidates and catalog-family records used by managed invocation discovery. It does not account for every CPU or memory operation.

## SM-7: first release-mode managed pressure benchmark — measured

- Contract: [redb storage validation](../../specs/redb-storage.md) requires operational point reads and durable writes to be measured under bounded analytical scans, with compilation and execution reported separately.
- Harness: [`storage_pressure.rs`](../../engine/examples/storage_pressure.rs) creates a temporary READY redb instance, seeds four 32 KiB rows, starts a managed invocation with a retained authorized snapshot, then runs four managed writer threads for 64 durable commits each. During the writes, the reader repeats full authorized scans and point reads and compares every returned value with its original snapshot. The benchmark removes its temporary instance afterward.
- Command: `nix-shell -p rustc cargo clippy rustfmt tinycc --run 'cargo build --release -p prdb-engine --example storage_pressure --locked'`, followed separately by `nix-shell -p rustc cargo clippy rustfmt tinycc --run './target/release/examples/storage_pressure'`.
- Measurement: redb 4.4.0, Rust 1.95.0, x86_64 Linux, ext4 on `/tmp` (`/dev/vda2`). One local run completed 256 durable commits in 0.610 s (419.5 commits/s) while the retained snapshot performed 8,930 full authorized scans and 35,720 point reads (14,632.4 scans/s). Exact values matched the original snapshot through all writer commits.
- Scope: one synthetic four-row workload and one local run; timings are machine-specific and do not establish production-duration contention, database growth/reclamation, power-loss durability, or other filesystem profiles.

## SM-7: operational commit latency with analytical pressure — measured

- Harness: [`storage_pressure.rs`](../../engine/examples/storage_pressure.rs) now measures 256 operational-only commits, restores the four seeded rows, and measures another 256 commits while an authorized reader retains its earlier snapshot and repeats full scans and point reads. Latencies time the durable `commit()` call only. The harness sets the managed reader scan-work budget to 2,000,000 to avoid ending this synthetic sustained scan interval at the default per-invocation budget.
- Command: `nix-shell -p rustc cargo clippy rustfmt tinycc --run 'cargo run --release -p prdb-engine --example storage_pressure --locked --offline'`.
- Measurement: redb 4.4.0, Rust 1.95.0, x86_64 Linux. Operational-only commit p50/p95/p99 was 6.169/11.787/17.016 ms; mixed-load p50/p95/p99 was 9.004/11.184/16.042 ms. The mixed writer phase completed in 0.582 s (440.2 commits/s) as the retained snapshot completed 8,557 full authorized scans and 34,228 point reads, observing only the original row values.
- Scope: one synthetic local run. The percentile shift is not stable evidence of an improvement or regression. This measures the managed storage API with repeated authorized scans, not concurrent SQL analytics; repeatable runs, SQL joins, maintenance, growth/reclamation, and other storage profiles remain unmeasured.

## SM-7: five-run commit-latency repeatability sample — measured

- Method: five sequential release-harness runs, each using a fresh temporary READY redb instance on the same x86_64 Linux host; compilation was outside all runs.
- Summary: median of the five run-level p50/p95/p99 values was 5.910/12.708/18.127 ms operational-only and 8.921/13.383/19.172 ms with the retained authorized snapshot scanning. Mixed throughput ranged 415.3–450.7 commits/s; each run completed 8,297–9,124 scans and preserved the snapshot values.
- Scope: repeated runs on one host strengthen local repeatability only. They do not represent SQL joins/sorts, maintenance, long-duration retention, another filesystem, or production targets. The mixed p99 varied from 16.406 to 34.279 ms.

## SM-1 / SM-7: process crashes at durable commit I/O boundaries — verified

- Contract: [redb storage recovery](../../specs/redb-storage.md) requires atomic rows, outcomes, and commit sequence after process exit; process termination does not prove power-loss durability.
- Regression: `process_crash_during_backend_write_recovers_atomic_commit` exits with code 87 immediately after an underlying backend write; `process_crash_after_durable_sync_recovers_atomic_commit` exits with code 86 immediately after a successful sync callback. Each parent reopens the database and accepts only the fully committed row/outcome/sequence or the original empty state.
- Verification: both focused crash tests passed on redb 4.4.0. The full workspace build, format check, warning-denied Clippy, and tests also passed; the commit integration target had 109 passing tests and 3 delegated-cgroup tests ignored.
- Scope: abrupt process exits after a backend write and successful sync callback; kernel cache and filesystem remain alive. Torn-write, power-loss, production filesystem, and other durability-profile recovery remain open.

## SM-1 / SM-7: single-byte persisted-page corruption rejection — verified

- Contract: [redb storage recovery](../../specs/redb-storage.md) requires invalid persisted state to fail closed before service admission, with the instance lock released after a failed open.
- Regression: `storage_open_rejects_single_byte_page_corruption_and_releases_the_instance_lock` flips one byte at the same offset in successive 4 KiB pages of a READY redb image until open rejects a corrupted page. It restores the valid image and confirms the instance can reopen and persisted records validate.
- Verification: the focused bootstrap regression passed. The workspace build, formatting check, warning-denied Clippy, and full workspace tests passed with Rust 1.95.0; the commit integration target had 109 passing tests and 3 delegated-cgroup tests ignored.
- Scope: detects the tested redb page corruption through the pinned local open/validation path. It does not prove every byte/page corruption is detected, simulate torn writes or power loss, or establish recovery across other platforms and durability profiles.

## SM-5: authorized foreign-key removal through StorageManager and ADMIN — implemented

- Contract: [storage manager](../../specs/storage-manager.md) requires FK removal to delete the constraint, its object and target dependency atomically, and advance both participating table write epochs so pinned invocations cannot commit against the old relationship graph.
- Implementation: `StorageManager::drop_foreign_key` reads and validates the FK and target constraint, constructs expected-value deletes, advances the distinct source/target table epochs, and delegates to the serialized authorized FK DDL commit path. Self-referencing FKs advance their shared table once.
- Exposure: authenticated ADMIN `PREPARE`/`START` accepts `foreign_keys.drop` with a foreign-key ID and returns the durable committed outcome. Ownership/DBA authorization, auditing, cancellation and operation-state behavior follow the existing DDL gateway.
- Regression: `storage_manager_drops_foreign_key_and_advances_both_table_epochs` verifies FK/object/dependency removal, both epoch updates, stale source/target writers conflicting, and startup integrity. `storage_manager_drops_self_referential_foreign_key_once_per_table` verifies a self-reference advances the shared table epoch once. The authenticated control integration test exercises the wire operation; decoder tests reject extra arguments.
- Verification: focused manager, decoder, and authenticated gateway tests passed. `cargo build --workspace --locked`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo test --workspace --locked` passed under Rust 1.95.0. The commit integration target had 114 passed and 3 sandbox/cgroup-dependent tests ignored.
- Remaining: repeated race stress and broader external constraint/policy interactions remain open.

## SM-5: structured StorageManager foreign-key creation — implemented

- Contract: [schema DDL](../../specs/ddl-schema.md) requires FK creation to validate ordered source/target columns, source ALTER and target REFERENCES authority, existing rows, and both table epochs under one serialized fenced commit.
- Implementation: `StorageManager::create_foreign_key` takes a typed request with source table/columns and a target PRIMARY KEY/UNIQUE constraint. It generates the FK ID, constructs the constraint/object/dependency records, increments each distinct table write epoch, and delegates final authority, compatibility, orphan-row, fence and publication checks to the authorized FK DDL path.
- Regression: `storage_manager_creates_foreign_key_and_advances_both_table_epochs` creates an FK using a target REFERENCES grant and source owner authority. It verifies returned server-generated ID, durable COMMITTED outcome, all catalog records and dependency, both table epoch increments, and startup integrity. The existing authority-separation and FK-create/table-drop race regressions also pass.
- Exposure: authenticated ADMIN `foreign_keys.create` uses connection-owned PREPARE/START, accepts ordered source columns plus a target key constraint, and returns the generated FK ID. The strict decoder rejects extra arguments; the real ADMIN control-dispatch integration now creates then drops an FK.
- Verification: focused FK creation, authorization-separation, FK/table-drop race, decoder, and authenticated control-path tests passed. Workspace build, format check, warning-denied Clippy, and full tests passed under Rust 1.95.0; the commit integration target had 115 passed and 3 delegated-cgroup tests ignored.

## SM-5: structured UNIQUE creation through StorageManager and ADMIN — implemented

- Contract: [schema DDL](../../specs/ddl-schema.md) requires UNIQUE creation to validate current rows, publish its owned unique index atomically, preserve NULLS DISTINCT behavior, and invalidate writers pinned before DDL.
- Implementation: `StorageManager::create_unique_constraint` generates constraint/index IDs and creates the family-8 definition, family-9 unique index, objects, dependency and table epoch update. The authorized constraint DDL commit validates and backfills the physical index under the table fence. ADMIN `constraints.create_unique` uses connection-owned PREPARE/START and returns both IDs.
- Regression: `storage_manager_creates_unique_constraint_and_backfills_existing_rows` inserts two unique values before DDL, creates the constraint, checks stale writer conflict and durable ABORTED outcome, and validates startup. Decoder coverage checks ordered ID arrays and exact arguments; the authenticated control integration creates a UNIQUE constraint before creating/dropping an FK.
- Verification: focused manager, decoder, and authenticated ADMIN control-path tests passed. Workspace build, format check, warning-denied Clippy, and full tests passed under Rust 1.95.0; the commit integration target had 117 passed and 3 delegated-cgroup tests ignored.
- Remaining: PRIMARY KEY and CHECK creation/removal need equivalent structured manager and ADMIN operations; SQL `ALTER TABLE` parsing remains outside this control protocol.

## SM-5: structured PRIMARY KEY creation through StorageManager and ADMIN — implemented

- Contract: [schema DDL](../../specs/ddl-schema.md) requires PRIMARY KEY columns to be non-nullable and the owned unique index to be validated/backfilled before publication.
- Implementation: `StorageManager::create_primary_key` shares the fenced key-constraint catalog path with UNIQUE while writing the PRIMARY KEY kind. Authenticated ADMIN `constraints.create_primary_key` exposes the structured request through connection-owned PREPARE/START and returns the generated constraint and index IDs.
- Regression: real-redb coverage creates a PRIMARY KEY on a non-null column, verifies its owned index and startup integrity, and proves a writer pinned before creation conflicts, publishes no row, and records ABORTED. Decoder coverage checks the exact ordered-column request, and the authenticated control integration creates a PRIMARY KEY before creating a UNIQUE and exercising FK lifecycle.
- Verification: focused manager, decoder, authenticated gateway, and stale-writer regressions passed. `cargo build --workspace --locked`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo test --workspace --locked` passed in the prescribed Nix shell; the commit integration target reported 121 passed and 3 cgroup-dependent ignored.
- Remaining: CHECK creation/removal and SQL `ALTER TABLE` parsing remain open.

## SM-5: structured CHECK lifecycle through StorageManager and ADMIN — implemented

- Implementation: typed `create_check_constraint` and `drop_check_constraint` operations generate/remove CHECK, expression, object, and dependency records and advance the table write epoch through the authorized, fenced constraint commit. Authenticated ADMIN exposes create/drop as connection-owned PREPARE/START operations; creation accepts bounded EIR1 bytes and declared column dependencies.
- Regression: real-redb coverage creates a Boolean CHECK, verifies startup integrity, then removes the CHECK/expression. A row-local CHECK that rejects an existing row leaves no publication and records ABORTED; non-row-local identity expressions are rejected. Writers staged before CHECK add and drop each conflict afterward, publish no row, and record ABORTED. Decoder tests cover create/drop requests, and the authenticated ADMIN control integration creates then drops a CHECK.
- Verification: focused lifecycle, decoder, authenticated gateway, and stale-writer regressions passed. `cargo build --workspace --locked`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo test --workspace --locked` passed in the prescribed Nix shell; the commit integration target reported 121 passed and 3 cgroup-dependent ignored.
- Remaining: SQL `ALTER TABLE` parsing remains outside this control protocol.

## SM-5: structured PRIMARY KEY and UNIQUE removal through StorageManager and ADMIN — implemented

- Implementation: typed PK/UNIQUE drop methods remove the constraint, owned index, object rows, and kind-4 dependency, and advance the table write epoch through the authorized constraint commit. That commit removes physical index memberships and checks remaining FK references before publication. ADMIN exposes `constraints.drop_primary_key` and `constraints.drop_unique` with connection-owned PREPARE/START.
- Regression: manager tests create and backfill a UNIQUE over existing rows and create a PRIMARY KEY, then remove each and verify catalog cleanup and startup integrity. Writers staged before UNIQUE and CHECK drops receive Conflict, publish no row, and record ABORTED. Dropping a PRIMARY KEY still referenced by an FK aborts without changing the commit sequence or catalog. The authenticated ADMIN integration creates and removes both key kinds after exercising CHECK and FK lifecycle; decoder tests verify the exact drop request shapes.
- Verification: focused manager, decoder, authenticated gateway, dependency-rejection, and stale-writer regressions passed. `cargo build --workspace --locked`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo test --workspace --locked` passed under Rust 1.95.0; the commit integration target had 120 passed and 3 cgroup-dependent tests ignored.
- Remaining: SQL `ALTER TABLE` parsing remains outside this control protocol.

## SM-5: typed ordinary index creation through StorageManager — implemented

- Contract: [storage manager](../../specs/storage-manager.md) requires ordinary index publication to authorize the actor, validate/backfill current rows, atomically publish the index and object, and advance the affected table write epoch.
- Implementation: `StorageManager::create_index` accepts a normalized name, ordered column IDs, and uniqueness flag; it generates the ID and delegates publication/backfill to `commit_authorized_index_ddl`. The narrow authorization guard now permits only kind-1 ownership-edge writes whose dependent is an index changed in the same batch. Invalid request/audit, storage, ID-generation, and commit errors have a typed `IndexLifecycleError`.
- Regression: `storage_manager_creates_named_unique_index_and_backfills_atomically` builds a named unique index over populated data, verifies its schema parent, ownership edge, physical forward/reverse memberships, and stored uniqueness, and proves a writer pinned before index publication conflicts, publishes no row, and records ABORTED.
- TDD evidence: the first real-redb run rejected the generated index object as `InvalidCatalog`; the accepted catalog shape requires a schema parent plus its matching kind-1 edge and the canonical database-admin owner.
- Verification: the focused real-redb lifecycle regression passed. `cargo build --workspace --locked`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo test --workspace --locked` all passed in the prescribed Nix shell.
- Remaining: typed StorageManager removal/rebuild requests and authenticated ADMIN operation dispatch remain open; the raw authorized index DDL commit path remains available for trusted internal callers.

## SM-5: typed ordinary index removal through StorageManager — implemented

- Contract: [storage manager](../../specs/storage-manager.md) requires index removal to authorize the current actor, remove the definition and physical memberships atomically, and advance the table write epoch.
- Implementation: `StorageManager::drop_index` accepts an index ID and captures exact current bytes for the family-9 definition, family-2 object, and schema-parent kind-1 ownership edge. It removes all three through `commit_authorized_index_ddl`; that path removes forward and reverse memberships and rejects a drop that leaves an invalid dependent constraint.
- Regression: `storage_manager_creates_named_unique_index_and_backfills_atomically` now continues through typed drop, checking durable COMMITTED status and removal of the definition, object, ownership edge, and all forward/reverse memberships.
- Verification: the focused real-redb create/drop lifecycle regression passed. `cargo build --workspace --locked`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo test --workspace --locked` all passed in the prescribed Nix shell; the commit integration target reported 124 passed and 3 cgroup-dependent ignored.
- Remaining: authenticated ADMIN operation dispatch remains open.

## SM-5: typed ordinary index rebuild through StorageManager — implemented

- Contract: [storage manager](../../specs/storage-manager.md) requires rebuild to validate old memberships, derive replacement keys from current rows, publish the updated definition and memberships atomically, advance the table write epoch, and preserve older snapshots.
- Implementation: `StorageManager::rebuild_index` accepts an index ID, ordered column IDs, and uniqueness profile. It captures the exact current index definition and table schema version, then rewrites the typed catalog fields and delegates the replacement build to `commit_authorized_index_ddl`.
- Regression: `storage_manager_rebuilds_populated_indexes_atomically` now exercises the typed API with authenticated database-admin membership. It verifies old snapshot visibility, replacement forward/reverse memberships, and durable conflict of a writer pinned before the rebuild.
- Verification: the focused real-redb rebuild regression passed. `cargo build --workspace --locked`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo test --workspace --locked` all passed in the prescribed Nix shell; the commit integration target reported 124 passed and 3 cgroup-dependent ignored.
- Remaining: none for the typed StorageManager layer; authenticated ADMIN operation dispatch is tracked below.

## SM-5: key constraints after schema evolution — fixed

- Contract: typed unique and primary-key DDL must create a backing index compatible with the current table schema, including after column changes.
- Red evidence: the multi-process ADMIN regression created a column, then unique constraint, and failed with `Commit(InvalidCatalog)`.
- Cause and fix: the generated family-9 index used the table schema version in its format-version field and hardcoded its schema-version field to 1. The builder now writes fixed format version 1 and the current schema version separately.
- Regression: the real gateway/supervisor lifecycle creates and drops a unique constraint after column addition; the scenario passes after the fix.
- Verification: the focused process regression and complete workspace build, format, warning-denied Clippy, and tests passed; the commit integration target reported 124 passed and 3 cgroup-dependent ignored.

## SM-5: typed object grant/revoke lifecycle — implemented

- Contract: object grants use the stable family-12 key and envelope format, validate privilege/object/scope combinations against the live catalog, and publish the grant, first/last authority dependency edge, grantee epoch, audit, and durable outcome atomically.
- Implementation: `StorageManager::set_object_grant` accepts typed schema/table/column targets and the closed object privilege set. It rejects duplicate grants and absent revokes, verifies that a column belongs to its table and that the grantee is a live user or role, and delegates publication to the authorized grant DDL validator.
- Regression: the gateway/supervisor process test uses the typed AdminClient grant path to grant both column-level SELECT and REFERENCES, create/drop a foreign key, revoke REFERENCES while SELECT remains, then revoke SELECT as the final grant. This exercises first/last authority dependency handling. Decoder tests cover table/column request shapes and reject an unknown privilege; CLI parsing accepts only the closed privilege set.
- Verification: focused decoder, process, parser, and CLI help checks passed. The full workspace build, format check, warning-denied Clippy, and rerun of the full test suite passed under Rust 1.95.0; the commit integration target reported 124 passed and 3 cgroup-dependent ignored. The first full test run had one intermittent `InvalidAudit` in the existing CHECK lifecycle test, and that test passed alone.
- Remaining: policy CLI and broader authenticated ADMIN coverage remain open.

## SM-5: typed policy create/drop through StorageManager — implemented

- Contract: row and cell policies use family 13 definitions, family 14 recipient/column targets, purpose-tagged family 7 expressions, exact dependency edges, table write-epoch fencing, audit, and durable outcome. `SELECT`/`DELETE` require USING; `INSERT` requires WITH CHECK; `UPDATE` requires both. Cell policies require column targets and reject DELETE.
- Implementation: `StorageManager::create_policy` validates normalized per-table names, operation/clause shape, unique live user/role recipients, same-table cell columns, expression Boolean type and binding against the current ordered schema, and live role dependencies. It generates policy/expression IDs and publishes all records and edges atomically. `drop_policy` removes the policy object, recipient/column targets, expression objects/records and all dependencies while advancing the table epoch.
- Regression: the existing policy DDL authorization integration now creates a typed row SELECT ALLOW policy for a role, checks its generated expression ID and committed outcome, then drops it and checks the next durable commit. Existing policy DDL tests cover inherited `MANAGE_POLICIES`, denied actors, target validity, and required recipients.
- Verification: focused policy lifecycle passed. Rust 1.95.0 workspace format check, warning-denied Clippy, build, and full tests passed; the commit integration target reported 124 passed and 3 cgroup-dependent ignored.
- Remaining: broader row/cell operation coverage and repeated race stress remain open.

## SM-2: reject zero IDs in derived index keys — verified

- Contract: [index-key encoding v1](../../specs/index-key-encoding.md) requires catalog table/index/row/column identifiers to be nonzero before a persisted SQL index key is derived.
- Red evidence: `catalog_index_key_rejects_zero_index_and_row_ids` failed because `derive_index_key` produced a complete key with a zero index ID.
- Implementation: `derive_index_key` now rejects a zero index ID or row UUID before decoding and projecting the typed tuple; the codec spec states the nonzero-ID rule.
- Verification: all 12 `index_key` integration tests passed. With Rust/Cargo 1.95.0, `cargo build --workspace --locked`, `cargo fmt --all -- --check`, warning-denied workspace Clippy, and `cargo test --workspace --locked` passed; the commit integration target reported 125 passed and 3 delegated-cgroup tests ignored.
- Remaining: no change to storage admission or migration behavior; database-format upgrade evidence and production power-loss/filesystem validation remain open.

## SM-5 test-fixture correction: CHECK DDL race cleanup audit correlation — fixed

- Red evidence: parallel `cargo test --workspace --locked` and default-parallel `cargo test --test commit` runs intermittently failed in `storage_manager_creates_and_drops_check_constraint` with `InvalidAudit`; the test passed alone and serially.
- Cause: when the staged INSERT won the barrier race, cleanup used drop invocation ID `0xd5` with an audit event whose helper-default correlation ID was `0xd7`. The drop API correctly rejected the mismatched event.
- Fix: the race-winning cleanup branch now sets the audit correlation ID to the actual drop invocation ID before calling `drop_check_constraint`.
- Verification: default-parallel `cargo test --locked --test commit` passed (129 passed, 3 cgroup-dependent ignored), followed by `cargo test --workspace --locked`, which passed across all targets. Rust formatting passed; warning-denied workspace Clippy is recorded with this fix's handoff.

## SM-1 / CL-4 redb physical format v2 fixture — verified slice

- Added an archived two-entry physical redb-v2 fixture generated and independently checked by a separate, locked redb 2.6.4 crate. Its source and reproduction steps are in [`redb-file-formats`](../../engine/tests/fixtures/redb-file-formats/README.md).
- Regression confirms pinned redb 4.4.0 read-only open returns `UpgradeRequired(2)` and leaves the fixture bytes unchanged. A disposable copy was upgraded with redb 2.6.4 `Database::upgrade()`, then opened read/write and read-only by redb 4.4.0 with both original table values intact.
- Verification: the focused `bootstrap_binary` regression passed. The redb 2.6.4 generator verified and recreated the fixture byte-for-byte; its `Database::upgrade()` converted a disposable copy, which redb 4.4.0 then recovered read/write and reopened read-only with both values intact. Formatting, locked workspace build, warning-denied all-target Clippy, and the full serial delegated-cgroup workspace suite passed; sandbox and process tests were enabled and none were ignored. `git diff --check` passed.
- Remaining: this is a small physical-format fixture, not a populated PRDB file or a supported operator upgrade command. Add application-data fixtures and an offline workflow before claiming redb-format upgrade support.

## SM-1 / CL-4 redb physical format v2-to-v3 reopen — verified follow-up

- Added an archived v3 physical-format fixture made by copying the v2 image and running redb 2.6.4 `Database::upgrade()`. Its hash and reproduction steps are recorded with the fixtures.
- Regression opens a copy read/write with pinned redb 4.4.0 to perform recovery, checks both original table entries, closes it, then reopens read-only and confirms the entries remain present. Together with the v2 rejection test, this records the supported redb API transition boundary in the repository.
- Verification: the focused `bootstrap_binary` regression passed. Broader workspace validation is not recorded in this follow-up.
- Remaining: the fixture still does not contain a populated PRDB logical catalog, and no operator-facing offline physical-format upgrade workflow exists.

## SM-1 / CL-4 populated PRDB redb-v2 compatibility — verified slice

- Extended the isolated generator to initialize a real READY instance through the current engine, add a schema/table/default expression and one application row, then transcribe all 20 byte-oriented logical engine tables into a redb 2.6.4 physical v2 file. The companion v3 file is that populated image after `Database::upgrade()`.
- Regression confirms pinned redb 4.4 rejects the populated v2 file with `UpgradeRequired(2)` without changing its bytes. After upgrading a copy, redb 4.4 recovery and read-only READY validation accept the complete populated catalog and preserve the application row payload.
- Verification: the generator passed warning-denied all-target Clippy; its create and verify operations reported 20 engine tables and one application row. Both focused `bootstrap_binary` regressions passed. Formatting, targeted test compilation, and `git diff --check` passed.
- Remaining: this establishes populated data compatibility but is not an operator-facing offline upgrade workflow; byte backup, interruption recovery, path-level atomic publication, and complete operator instructions remain open.

## CL-4 / SM-1 offline redb physical-v2 upgrade — implemented

- Red evidence: `maintenance_redb_v2_upgrade_backs_up_and_atomically_publishes_ready_instance` first failed because `prdb-maintain` had no `redb-format-upgrade` operation.
- Implementation: added the explicit offline command with required `--confirm-redb-v2`, exclusive instance locking, an independent mode-0600 raw-byte backup outside the instance, and same-directory private staging. The pinned redb 2.6.4 `Database::upgrade()` operates only on the staging copy. All 20 logical tables are hashed across the old and new APIs; redb 4.4 must pass complete READY validation before atomic filename exchange. The old v2 source remains at `.prdb-redb-upgrade.next`. Retry verifies the backup/source relationship and active logical contents.
- Specification and operator notes: updated [redb storage](../../specs/redb-storage.md), [catalog lifecycle](../../specs/catalog-lifecycle.md), and [physical-format fixture instructions](../../engine/tests/fixtures/redb-file-formats/README.md). The transition is physical-only and does not migrate logical catalog versions.
- Verification: the initial targeted regression was observed failing. The final-source checks passed: formatting, locked offline workspace build, warning-denied all-target Clippy, the focused serial `bootstrap_binary` target (31 passed), TinyCC SDK `make test`, release-profile generation/validation, and the full serial workspace suite inside the delegated `systemd --user` service. The cgroup probe reported writable `cpu memory pids`; sandbox and process tests were enabled and the suite completed with zero ignored tests. `git diff --check` passed.
- Limits: power-loss/filesystem fault injection remains open. Keep the independent backup and retained v2 source until the post-upgrade validation window ends.

## Atomic table-level PRIMARY KEY creation — implemented

- Contract: [schema DDL](../../specs/ddl-schema.md). `StorageManager::create_table_with_primary_key` accepts ordered distinct non-nullable column names and publishes the new table, its columns/default expressions, PRIMARY KEY constraint, owned unique index, dependency edge, audit, and operation outcome atomically. The legacy `create_table` path is unchanged.
- Validation: the commit DDL authority guard permits and validates exactly one optional PK/index pair, verifies source columns are non-nullable members of the created table, checks the owned index metadata/object records and dependency, and keeps the audit target on the table ID.
- Regression: `storage_manager_creates_primary_key_with_owned_unique_index` now creates a composite-key table through the new path and validates the reopened catalog. The focused test and full 173-test commit integration target passed.
- Verification: locked workspace build, format check, warning-denied Clippy, full serial delegated-cgroup workspace suite (sandbox/process tests enabled, zero ignored), and `git diff --check` passed.

## Atomic table-level UNIQUE creation — implemented

- Contract: [schema DDL](../../specs/ddl-schema.md). `StorageManager::create_table_with_constraints` accepts one optional PRIMARY KEY plus multiple ordered UNIQUE column lists. It generates each constraint/index identity and publishes family-8 constraint, family-9 unique index, object records, and kind-4 dependency with the table, columns, audit, and outcome. Nullable UNIQUE columns are accepted and inherit NULLS DISTINCT semantics.
- Validation: the table-create commit guard verifies one-to-one constraint/index ownership, table membership, ordered source/index columns, the unique index shape, object identity/owner/version, dependency edges, and the single-primary-key maximum.
- Regression: the atomic table lifecycle regression creates composite PRIMARY KEY and nullable UNIQUE constraints together and validates startup. The full 173-test commit target passed.
- Verification: locked workspace build, format check, warning-denied Clippy, full serial delegated-cgroup workspace suite (sandbox/process tests enabled, zero ignored), and `git diff --check` passed.
## ID-SQL-2: authenticated principal name lookup

- Added `resolve_token_administrative_identity_name`, which authenticates each request and resolves one indexed principal name in the pinned catalog snapshot after checking `MANAGE_USERS` or `MANAGE_ROLES` (or database-admin membership). The full real-redb/process lifecycle suite passes in the delegated-cgroup workspace run; hidden and absent names share the null result.

## ID-SQL-3: DROP identity SQL dispatch

- SQL calls the existing identity drop lifecycle. RESTRICT reference checks, user credential revocation, role membership validation, and permanent user/role tombstones remain enforced in storage. The delegated gateway/supervisor SQL lifecycle passes and the full serial delegated-cgroup workspace suite passes.

## ID-SQL-4: LOGIN state SQL dispatch

- SQL dispatches to the existing storage lifecycle. Atomic active-token revocation on NOLOGIN and the no-token-restoration behavior on LOGIN remain storage enforced. Delegated process and full workspace suites pass.

## ID-SQL-5: role membership SQL dispatch

- SQL invokes the existing membership lifecycle, which validates endpoint kinds, cycles, and administrator safety under commit serialization. Delegated process and full workspace suites pass.
