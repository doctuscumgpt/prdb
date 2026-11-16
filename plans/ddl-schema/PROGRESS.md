# Schema DDL Progress

## DS-3 / DD-3: same-name index recreation retains distinct catalog identity — verified slice

- The named unique-index lifecycle regression verifies that dropping and recreating an index under the same schema/name publishes a distinct ID and only the recreated family-2/family-9 records and dependency in the current snapshot. A reader pinned before the drop continues resolving the original index ID and records.
- Verification: focused real-redb regression, format check, locked workspace build, warning-denied all-target Clippy, and the complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests.

Migrated component-owned progress entries from [storage-manager progress](../storage-manager/PROGRESS.md); evidence is preserved below.

## DS-SQL-27: self-referencing FOREIGN KEY in CREATE TABLE — verified

- `CREATE TABLE` now accepts inline or table-level foreign keys that target a PRIMARY KEY or UNIQUE constraint declared in the same statement. Storage resolves the local target after generating the table's column/key IDs and publishes the FK, dependency, key index, and table together. No pre-existing REFERENCES grant is needed for a target key owned by that same CREATE; external FK authority remains unchanged.
- The control payload uses `tables.create` version 2 only when a same-statement target is present; ordinary and external-FK requests retain version 1. Strict decoder and authenticated AdminClient request regressions cover the zero-ID/local-column sentinel and the self-target identity.
- Verification: focused real-redb storage, strict v2 decoder, and authenticated AdminClient SQL control-channel tests pass. The complete serial delegated-cgroup workspace suite passes with sandbox/process tests enabled and zero ignored tests (274 library tests, 174 commit tests, 42 client-admin tests, plus all other targets). Workspace build, format check, warning-denied Clippy, TinyCC SDK tests/fixture, release-profile validation, and `git diff --check` pass. Cargo.lock is unchanged.

### DS-SQL-27 gateway/supervisor process follow-up — verified

- `self_referencing_create_table_sql_crosses_gateway_and_supervisor_processes` starts separate production gateway and supervisor binaries, creates a schema, invokes the actual `prdb-client admin-sql` command for a table-level composite self-reference, then confirms the FK points to the new table and preserves source/target column order through authenticated metadata. It drops that table through SQL RESTRICT, then creates an externally referencing pair, confirms the parent drop is rejected while both relations remain, and drops child then parent through SQL. Inline single-column coverage remains in the separate authenticated AdminClient regression.
- Verification: the original focused test and complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests. After extending the process lifecycle with external-FK RESTRICT assertions, the focused delegated process test passed again. Post-change `cargo fmt --all -- --check`, warning-denied Clippy, and `git diff --check` passed. This isolated acceptance keeps the earlier long metadata lifecycle's index and policy fixtures independent.
- Follow-up: the child FK is now listed through authenticated `constraint-list`; the returned generated ID is passed to `ALTER TABLE ... DROP CONSTRAINT`. The test confirms the formerly restricted parent can then be dropped. The focused delegated process test, workspace build, format check, warning-denied Clippy, and `git diff --check` pass; `Cargo.lock` is unchanged.

## DS-5 / SM-5: empty schema DROP RESTRICT — verified

- Behavior and normative contracts: [schema DDL](../../specs/ddl-schema.md), [client protocol](../../specs/client-protocol.md), and [storage manager](../../specs/storage-manager.md). The owner, a member of an owning role, or a database administrator can remove an empty non-system schema. Child objects and remaining dependency edges reject the request. A second emptiness check runs under the serialized redb writer to linearize against concurrent table creation.
- Red evidence: the new schema lifecycle test first called a compiling `drop_schema` stub and failed with `SchemaLifecycleError::InvalidRequest`.
- Implementation: [authorized schema drop commit path](../../engine/src/commit.rs), [storage entry point](../../engine/src/storage_manager.rs), and connection-owned [`schemas.drop` PREPARE/START](../../engine/src/client_protocol.rs). Schema and object records, DDL audit, and durable outcome publish atomically.
- Regression evidence: the real-redb lifecycle test covers owner success, unauthorized rejection, a populated-schema RESTRICT rejection with durable ABORTED outcome, and record deletion. A barrier-coordinated create/drop race accepts only a complete table-creation win or complete schema-drop win, then reopens through startup validation. The gateway ADMIN lifecycle test creates and drops an empty schema through PREPARE/START; the decoder test checks the strict schema-drop request.
- Verification: in the configured Nix shell, `cargo build --workspace --locked`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo test --workspace --locked` all passed. `git diff --check` passed.
- Remaining: table/column rename, default and nullability changes, schema ownership transfer, full constraint/admin wiring, and separate supervisor/gateway process integration remain incomplete.
- Next smallest behavior: implement one supported table or column alteration with snapshot/fence validation.

## DS-5 / SM-5: table DROP RESTRICT — implemented

- Behavior and normative contracts: [schema DDL](../../specs/ddl-schema.md), [client protocol](../../specs/client-protocol.md), and [storage manager](../../specs/storage-manager.md). The table owner or a database administrator can drop a table if no external dependency remains. The DDL commit removes owned table/column/constraint/index/policy definitions, expressions, grants and dependency edges, table rows, and physical index entries atomically. Cleanup is bounded to 65,536 rows, 65,536 index entries, and 64 MiB combined row tuple payload plus physical index key and locator bytes per commit.
- Implementation: [authorized table-drop commit path](../../engine/src/commit.rs), [storage entry point](../../engine/src/storage_manager.rs), and connection-owned [`tables.drop` PREPARE/START](../../engine/src/client_protocol.rs).
- Regression evidence: real-redb tests verify unauthorized rejection, atomic rejection of 65,537 rows followed by successful table removal at exactly 65,536 rows, and conflict for a writer pinned before table drop. The successful drop also removes an authorized table grant and its authority dependency while advancing the grantee's authority epoch. Table-owned row policy and CHECK constraint fixtures verify removal of each typed definition, expression, object records, policy target, and dependency edges. A barrier-coordinated race between a staged writer commit and table drop accepts either commit order, checks durable outcomes, and confirms no orphan row or table record remains. Byte-boundary tests reject 64 MiB plus one byte and accept exactly 64 MiB for both tuple payload bytes alone and the combined tuple payload, physical index-key, and locator bytes; they check row/catalog deletion and startup validation. The stale writer publishes no row and records a durable ABORTED outcome. The two-index cleanup boundary rejects 65,538 memberships atomically, then removes both indexes at exactly 65,536 entries; the test checks table/column/object/catalog deletion, both physical index directions, and startup validation. The FK authorization lifecycle also confirms that dropping a referenced table is rejected with `TableHasDependencies`, leaves its FK and table records intact, and records a durable ABORTED outcome. The strict protocol decoder accepts the exact `table_id` request, and the real-redb gateway ADMIN lifecycle test exercises PREPARE/START and the committed response.
- Verification: focused table-drop row, byte, index-entry, pinned-writer, and FK RESTRICT regressions passed. In the configured Nix shell, workspace build, format check, warning-denied Clippy, all workspace tests, and `git diff --check` passed for the payload accounting fix and byte-boundary test. Tool versions were Rust/Cargo 1.95.0, rustfmt 1.9.0, Clippy 0.1.95, and TinyCC 0.9.27-unstable-2025-01-06.
- TDD note: the exact byte-boundary test first failed after the planner fix because serialized snapshot revalidation still counted row-record framing; both planning and commit-time revalidation now count decoded tuple payload bytes. The table-grant regression first exposed that DROP TABLE neither advanced a grantee's authority epoch nor allowed the resulting principal update through its restricted catalog-write guard; the cleanup now advances each affected grantee once and the authorization guard accepts only the corresponding one-step epoch changes. The policy cleanup regression first failed with `InvalidCatalog` because the planner decoded policy-target records using the generic envelope parser; DROP TABLE now uses the dedicated policy-target decoder. The row/index-entry tests were added after the original implementation and have no observed failing behavior run to record.
- Remaining: DROP TABLE coverage for remaining constraint combinations and separate supervisor/gateway process integration remain open.

## DS-5 / SM-5: referenced-table epoch on FK owner drop — verified

- Behavior and normative contracts: [storage manager](../../specs/storage-manager.md), [schema DDL](../../specs/ddl-schema.md), and the table DROP RESTRICT lifecycle above. Dropping the table that owns a foreign key removes the FK and its target dependency atomically and advances the referenced table's write epoch once, so a writer pinned to its earlier constraint graph cannot commit afterward.
- Red evidence: the real-redb owner-table drop regression first failed with `InvalidCatalog`; final catalog validation showed the referenced table epoch had not advanced when the owned FK disappeared.
- Implementation: [table-drop write set](../../engine/src/storage_manager.rs) updates each distinct referenced table epoch with the cleanup. [Commit validation and authorization](../../engine/src/commit.rs) identify the actual deleted table record, permit only one-step epoch-only updates for tables referenced by FKs owned by that table, and treat a table record as deleted only when its payload is removed.
- Regression evidence: [storage_manager_drop_referencing_table_removes_owned_foreign_key](../../engine/tests/commit.rs) removes two source-table foreign keys to the same target, verifies both dependency edges are deleted, preserves the target constraint and index, advances the referenced-table epoch exactly once, records a durable COMMITTED outcome, and passes startup validation. A writer staged against the referenced table before the drop conflicts afterward, records ABORTED, and publishes no row.
- Verification: the focused test passed. In the configured Nix shell, `cargo build --workspace --locked`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo test --workspace --locked` passed; all 103 commit integration tests passed. `git diff --check` passed. Tool versions were Rust/Cargo 1.95.0, rustfmt 1.9.0, Clippy 0.1.95, and TinyCC 0.9.27-unstable-2025-01-06.
- Remaining: other constraint variants and separate supervisor/gateway process integration remain open.

## DS-5 / SM-5: owned NOT NULL, PRIMARY KEY, and UNIQUE cleanup on table drop — verified

- Behavior and normative contracts: [schema DDL](../../specs/ddl-schema.md) and [storage manager](../../specs/storage-manager.md). Dropping a table removes its internally generated NOT NULL constraint and the column's nullable metadata, plus owned PRIMARY KEY and UNIQUE definitions, backing index objects, kind-4 dependencies, and forward/reverse physical index memberships atomically with its rows.
- Red evidence: no behavioral failure was observed for this added coverage; the existing table-drop path already implemented index cleanup. The new acceptance test makes owned constraint/index cleanup observable against persisted physical memberships.
- Regression evidence: [storage_manager_drop_table_removes_owned_primary_and_unique_indexes](../../engine/tests/commit.rs) first makes a nullable column NOT NULL through the storage-manager lifecycle, then adds both key constraints. It commits a row through `InvocationTransaction` so both physical index directions contain entries, drops the table, checks all constraint/index/column records and dependencies are gone, verifies the row and memberships are removed, and reopens through startup validation.
- Verification: the focused real-redb test passed. In the configured Nix shell, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo test --workspace --locked` passed; all 104 commit integration tests passed. `git diff --check` passed.
- Remaining: DROP TABLE coverage for remaining constraint combinations and separate supervisor/gateway process integration remain open.

## DS-5 / SM-5: self-referential FK cleanup on table drop — verified

- Behavior and normative contracts: [schema DDL](../../specs/ddl-schema.md) and [storage manager](../../specs/storage-manager.md). A self-referential foreign key is owned by the table it references; DROP TABLE removes that FK, its target dependency, and its owned table/index definitions atomically rather than treating the internal reference as an external RESTRICT dependency.
- Red evidence: no behavioral failure was observed. The first fixture attempt was rejected during catalog setup because its column and object names differed; after correcting the fixture and publishing the primary key before the FK, the lifecycle test passed.
- Regression evidence: [storage_manager_drop_table_removes_self_referential_foreign_key](../../engine/tests/commit.rs) creates a table with a primary key, adds a foreign key from that table back to its own key, commits a row whose FK value resolves to itself, and then drops the table through `StorageManager`. It checks removal of table/column/constraint/index objects, both dependency edges, the row, and both physical index directions, then confirms a durable COMMITTED outcome and validates startup.
- Verification: the focused real-redb test passed. In the configured Nix shell, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo test --workspace --locked` passed; all 105 commit integration tests passed. `git diff --check` passed.
- Remaining: DROP TABLE coverage for remaining constraint combinations and separate supervisor/gateway process integration remain open.
- Next smallest behavior: review remaining storage-manager validation gaps and address the next unfinished acceptance case before process-level integration.

## DS-5 / SM-5: column DROP RESTRICT — implemented

- Behavior and normative contracts: [schema DDL](../../specs/ddl-schema.md), [client protocol](../../specs/client-protocol.md), and [storage manager](../../specs/storage-manager.md). The table owner or a database administrator can drop an unreferenced column from a non-system table. Expressions, constraints, indexes, and policies that reference the column block the operation. Storage removes the column and its default expression/grants/dependency edges, compacts ordinals, advances schema version and write epoch, rewrites tuples, and rebuilds surviving index keys in one bounded fenced commit.
- Implementation: [authorized column-drop commit path](../../engine/src/commit.rs), [storage entry point](../../engine/src/storage_manager.rs), and connection-owned [`tables.drop_column` PREPARE/START](../../engine/src/client_protocol.rs). The rewrite budget counts old and rewritten tuple payload bytes, excluding row-record framing.
- Regression evidence: a real-redb test inserts a two-column row, creates an index on the retained column, verifies RESTRICT rejects dropping that indexed column with a durable aborted outcome, then drops the other column while a writer is pinned to the old schema. The writer conflicts, publishes no row, and records an aborted outcome. The FK authorization lifecycle also verifies RESTRICT blocks dropping either FK endpoint. The test checks ordinal compaction, schema version bump, retained tuple value, committed outcome, and that the surviving index locator advances to the rewritten row version while its logical key and reverse membership remain valid. A separate row-boundary regression rejects 65,537 rows atomically and accepts exactly 65,536, then verifies retained values, schema version, durable outcome, and startup validation. The byte-boundary regression rejects 64 MiB plus one byte atomically and accepts exactly 64 MiB of combined tuple payloads, then verifies the retained value and startup validation. A strict ADMIN decoder test checks the two-ID request.
- TDD note: the behavior test first failed with `InvalidRequest` against the compiling storage-manager stub, then passed after the schema rewrite path was implemented. The exact byte-boundary regression first failed because the commit path included row-record framing in its tuple-payload budget; the implementation now measures the decoded old tuple payload consistently with the contract.
- Verification: focused real-redb rewrite, RESTRICT, index-locator, row-boundary, and byte-boundary regressions passed. In the configured Nix shell, workspace build, format check, warning-denied Clippy, all workspace tests, and `git diff --check` passed for the row and byte boundary coverage. Tool versions were Rust/Cargo 1.95.0, rustfmt 1.9.0, Clippy 0.1.95, and TinyCC 0.9.27-unstable-2025-01-06.
- Remaining: external-FK/constraint/policy dependency regressions, index/constraint DDL race coverage, and separate supervisor/gateway process integration remain open.

## DS-5 / SM-5: table rename — implemented

- Behavior and normative contracts: [schema DDL](../../specs/ddl-schema.md), [client protocol](../../specs/client-protocol.md), and [storage manager](../../specs/storage-manager.md). The table owner or a database administrator can rename a non-system table. The stable table ID and tuple schema version remain fixed; the table write epoch and object version advance. Schema-local name uniqueness is checked inside the serialized final catalog commit.
- Implementation: [authorized table-rename commit path](../../engine/src/commit.rs), [storage entry point](../../engine/src/storage_manager.rs), and connection-owned [`tables.rename` PREPARE/START](../../engine/src/client_protocol.rs).
- Regression evidence: the real-redb lifecycle test verifies name/object update, stable table identity, unchanged schema version, incremented write epoch, duplicate-name rejection with a durable aborted outcome, and subsequent row/index operations through a column drop. The strict decoder test accepts the exact table ID and normalized name shape.
- TDD note: the rename assertion first failed with `InvalidRequest` against the compiling storage stub; the storage path and authorized commit guard now pass it.
- Verification: `cargo build --workspace --locked`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked`, and `git diff --check` all passed in the configured Nix shell; the focused storage and decoder tests passed as well.
- Remaining: column rename gateway dispatch coverage, role-owner authorization regression, and other supported alterations remain open.

## DS-5 / SM-5: column nullability changes — implemented

- Behavior and normative contracts: [schema DDL](../../specs/ddl-schema.md), [client protocol](../../specs/client-protocol.md), and [storage-manager contract](../../specs/storage-manager.md). The table owner or a database administrator can set or drop NOT NULL through `tables.alter_column_nullability`. Storage keeps the tuple schema version, increments the table write epoch, validates persisted rows before setting NOT NULL, and adds/removes the internal constraint atomically under the table DDL fence.
- Implementation: [authorized constraint DDL path](../../engine/src/commit.rs), [storage entry point](../../engine/src/storage_manager.rs), and connection-owned PREPARE/START in [client protocol](../../engine/src/client_protocol.rs).
- Regression evidence: the real-redb test verifies that an existing NULL row rejects SET NOT NULL with a durable aborted outcome and leaves catalog nullability unchanged; after removing that row, a writer staged against the nullable schema conflicts after SET NOT NULL, publishes no row, and records an aborted outcome. SET then DROP NOT NULL updates the column, constraint/object records, and write epoch while preserving schema version. Startup validation succeeds afterward. The decoder accepts the strict operation shape and rejects an invalid nullable value.
- TDD note: the storage behavior assertion initially failed against the stub returning `InvalidRequest`, then passed through the authorized storage manager path.
- Verification: in the configured Nix shell, `cargo build --workspace --locked`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked`, and `git diff --check` all passed. The focused storage and protocol decoder regressions passed.
- Remaining: stale-writer race coverage for other DDL paths, role-owner authorization regression, and separate supervisor/gateway process integration remain open.

## DS-5 / SM-5: column rename — implemented

- Behavior and normative contracts: [schema DDL](../../specs/ddl-schema.md), [client protocol](../../specs/client-protocol.md), and [storage-manager contract](../../specs/storage-manager.md). The table owner or a database administrator can rename a column on a non-system table. Storage preserves column ID, ordinal, tuple schema version, and dependencies; updates the family 6 and family 2 names and object version; and advances the table write epoch. Table-local uniqueness is checked against the serialized final catalog.
- Implementation: [authorized column-rename commit path](../../engine/src/commit.rs), [storage entry point](../../engine/src/storage_manager.rs), and connection-owned PREPARE/START in [client protocol](../../engine/src/client_protocol.rs).
- Regression evidence: the real-redb test verifies duplicate-name rejection with a durable aborted outcome and successful name agreement in both catalog records; a writer staged against the previous column schema conflicts after rename, publishes no row, and records an aborted outcome. The gateway ADMIN lifecycle test exercises `tables.rename_column` PREPARE/START, and the strict decoder test checks the request shape.
- TDD note: the behavior test first failed with `InvalidRequest` against the compiling storage stub. Initial implementation exposed the family 6 name tag and schema-epoch behavior, which were corrected to match the catalog contract.
- Verification: in the configured Nix shell, `cargo build --workspace --locked`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked`, and `git diff --check` all passed. Focused real-redb, protocol decoder, and gateway ADMIN PREPARE/START tests passed.
- Remaining: stale-writer coverage for default changes, DROP COLUMN, and index/constraint DDL, plus separate supervisor/gateway process integration.

## DS-5 / SM-5: current column default changes — implemented

- Behavior and normative contracts: [schema DDL](../../specs/ddl-schema.md), [client protocol](../../specs/client-protocol.md), and [storage-manager contract](../../specs/storage-manager.md). Owners or database administrators can set, replace, or drop a non-generated column's current insert default on non-system tables. Typed literal, NULL, and UUID current-principal defaults use the create-table encoding. Existing rows and schema version remain unchanged; the table write epoch advances, and replaced/dropped expression catalog records and dependency edges are removed atomically. The immutable add-column default is preserved.
- Implementation: [authorized constraint DDL guard](../../engine/src/commit.rs), [storage entry point](../../engine/src/storage_manager.rs), and connection-owned `tables.set_column_default` / `tables.drop_column_default` PREPARE/START in [client protocol](../../engine/src/client_protocol.rs).
- Regression evidence: the real-redb storage regression exercises set, replacement, drop, and rejection of an absent-default drop. A writer staged before a default change conflicts, publishes no row, and records an aborted outcome. The strict decoder checks both command shapes; the gateway ADMIN lifecycle test runs set and drop through PREPARE/START.
- TDD note: the first storage behavior assertion failed against the `InvalidRequest` stub; the catalog expression/object/dependency changes then passed through the serialized authorized DDL path.
- Verification: `cargo build --workspace --locked`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked`, and `git diff --check` all passed in the configured Nix shell. Focused storage, decoder, and gateway ADMIN PREPARE/START regressions passed.
- Remaining: stale-writer coverage for DROP COLUMN and index/constraint DDL, plus separate supervisor/gateway process integration.

## DS-5 / DS-7: table rename versus a pinned writer — verified

- Behavior and normative contracts: [schema DDL](../../specs/ddl-schema.md), [MVCC concurrency](../../specs/concurrency-mvcc.md), and [storage manager](../../specs/storage-manager.md). A transaction that has loaded the table schema and indexes and staged a row against that snapshot must not commit after table rename advances the write epoch.
- Regression evidence: the real-redb table lifecycle test stages a valid tuple under the pinned schema, renames the table, then verifies commit returns `CommitError::Conflict`, the row is absent, and the invocation has a durable ABORTED outcome.
- Implementation: no code change was needed. The existing schema/object exact-byte guards and shared DDL fence reject the stale commit.
- Verification: focused lifecycle regression passed. `cargo build --workspace --locked`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked`, and `git diff --check` all passed in the configured Nix shell.
- Remaining: stale-writer coverage for column rename, nullability/default changes, add/drop column, and index/constraint DDL; role-owner authorization regression; separate supervisor/gateway process integration.

## DS-5 / DS-7: table rename and staged writer serialize atomically — verified

- Reconciled the earlier race status against `storage_manager_renames_tables_and_drop_column_rewrites_rows`: a barrier starts the rename and staged writer together, then checks either writer-first publication before the rename or rename-first conflict/ABORTED with no row. The test also confirms the retained name reader keeps its pinned schema and the current name index resolves only the renamed table.
- Verification: included in the complete serial delegated-cgroup workspace suite on 2026-11-16; all 191 commit integration tests passed, including sandbox and process coverage. Locked workspace build, formatting, warning-denied Clippy, TinyCC SDK tests/fixture, and `git diff --check` passed.
- Remaining: stale-writer coverage for column rename, nullability/default changes, add/drop column, and index/constraint DDL; role-owner authorization regression; separate supervisor/gateway process integration.

## DS-5 / SM-5: table ADD COLUMN — implemented

- Behavior and normative contracts: [schema DDL](../../specs/ddl-schema.md), [client protocol](../../specs/client-protocol.md), and [storage manager](../../specs/storage-manager.md). The table owner or a database administrator can append a non-generated column to a non-system table. NOT NULL requires a typed non-null literal; nullable columns without a default backfill NULL. Context-dependent defaults are rejected. Storage advances schema version and write epoch, rewrites existing tuples, and updates index definitions and locators atomically under the DDL fence. Row rewrites are bounded to 65,536 rows and 64 MiB of combined old and rewritten tuple payloads.
- Implementation: [serialized add-column validation and row rewrite](../../engine/src/commit.rs), [storage manager entry point](../../engine/src/storage_manager.rs), and connection-owned `tables.add_column` PREPARE/START in [client protocol](../../engine/src/client_protocol.rs).
- Regression evidence: the real-redb test seeds a row, builds an index, adds a NOT NULL text column with a literal default, then adds a nullable text column without a default. It checks both backfilled values, updated index schema versions, and locators for each rewritten row. A writer staged against the old schema conflicts after ADD COLUMN, publishes no row, and records an aborted outcome. Large real-redb cases reject 65,537 rows atomically then accept exactly 65,536 rows, and reject a rewrite totaling 64 MiB plus one byte then accept exactly 64 MiB; startup validation succeeds after accepted rewrites. Decoder coverage accepts the exact operation and rejects a missing required default and `CURRENT_PRINCIPAL_ID`. The gateway ADMIN lifecycle test exercises PREPARE/START and the committed response.
- TDD note: the storage behavior regression first failed with `InvalidRequest` against the stub and passed after the serialized DDL/rewrite path was added.
- Verification: the focused real-redb backfill/index regression, strict decoder test, and gateway ADMIN lifecycle test passed. In the configured Nix shell, workspace build, format check, warning-denied Clippy, all workspace tests, and `git diff --check` passed. Tool versions were Rust/Cargo 1.95.0, rustfmt 1.9.0, Clippy 0.1.95, and TinyCC 0.9.27-unstable-2025-01-06.
- Remaining: separate supervisor/gateway process integration.

## DS-5 / SM-5 / DS-7: table ownership transfer — implemented

- Behavior and normative contracts: [schema DDL](../../specs/ddl-schema.md), [client protocol](../../specs/client-protocol.md), and [storage manager](../../specs/storage-manager.md). The current owner or a database administrator can transfer a non-system table to an existing user or role. The table object owner/version and table write epoch update atomically; tuple schema, rows, indexes, and grants stay unchanged. The new owner receives table DDL authority without data privileges.
- Implementation: the storage entry point and serialized authority guard update only the table object and table record, validate the target principal, reject system schemas, and guard actor/owner/membership inputs. `tables.set_owner` uses connection-owned ADMIN PREPARE/START.
- Regression evidence: the real-redb lifecycle transfers to a role, checks there are no table data-grant records for that role, confirms an inherited member can rename the table, rejects an unrelated user's transfer with a durable ABORTED outcome, and verifies a writer pinned before the transfer aborts with `Conflict` and publishes no row. The gateway ADMIN lifecycle and strict request decoder cover PREPARE/START and the exact argument shape.
- TDD note: the ownership behavior assertion first failed with `InvalidRequest` against a compiling storage-manager stub; it passes through the authorized DDL path.
- Verification: focused real-redb, decoder, and gateway lifecycle tests passed. Workspace build, format check, warning-denied Clippy, all workspace tests, and `git diff --check` passed in the configured Nix shell. Tool versions were Rust/Cargo 1.95.0, rustfmt 1.9.0, Clippy 0.1.95, and TinyCC 0.9.27-unstable-2025-01-06.
- Remaining: DDL limit boundaries, other stale-writer races, and separate supervisor/gateway process integration.

## DS-5: schema ownership transfer — implemented

- Behavior and normative contracts: [schema DDL](../../specs/ddl-schema.md), [client protocol](../../specs/client-protocol.md), and [storage manager](../../specs/storage-manager.md). A schema owner or database administrator can transfer a non-system schema to a live user or role. The owner object and version update atomically; schema contents stay unchanged. Schema owners can create tables and manage the schema without gaining table data privileges.
- Implementation: the storage entry point updates the schema object; its serialized authorization guard validates actor and target principals, role membership, system-schema mappings, schema record, and audit. The `schemas.set_owner` operation uses connection-owned ADMIN PREPARE/START. Table creation now recognizes current schema ownership as DDL authority.
- Regression evidence: real-redb coverage transfers the schema to a role, checks owner/version, rejects the previous non-admin owner's drop with a durable ABORTED outcome, and verifies an inherited member can create and drop a table, rename the schema, and receives no table data grants. Decoder and gateway ADMIN lifecycle tests cover the exact request and committed result.
- TDD note: the schema transfer assertion first failed with `InvalidRequest` against the compiling storage-manager stub; it passes through the serialized owner guard.
- Verification: focused real-redb, decoder, and gateway lifecycle tests passed. Workspace build, format check, warning-denied Clippy, all workspace tests, and `git diff --check` passed in the configured Nix shell. Tool versions were Rust/Cargo 1.95.0, rustfmt 1.9.0, Clippy 0.1.95, and TinyCC 0.9.27-unstable-2025-01-06.
- Remaining at this milestone: additional stale-writer and limit boundary evidence, and separate supervisor/gateway process integration.

## DS-5: schema rename — implemented

- Behavior and normative contracts: [schema DDL](../../specs/ddl-schema.md), [client protocol](../../specs/client-protocol.md), and [storage-manager contract](../../specs/storage-manager.md). The schema owner or a database administrator can rename a non-system schema. Storage preserves the schema ID, schema record, and child objects while updating the owned object name and version atomically. Final name uniqueness is checked by the serialized catalog writer.
- Implementation: `StorageManager::rename_schema` and its authorized commit guard update only the schema object. The `schemas.rename` operation uses connection-owned ADMIN PREPARE/START.
- Regression evidence: the real-redb lifecycle test verifies duplicate-name rejection, successful rename with a stable schema ID and incremented object version, and denial to the previous non-admin owner. Decoder and gateway lifecycle tests cover the exact request and committed result.
- TDD note: the schema rename assertion first failed with `InvalidRequest` against the compiling storage stub; the rename and uniqueness checks now pass through the serialized authorized DDL path.
- Verification: focused real-redb, decoder, and gateway lifecycle tests passed. Workspace build, format check, warning-denied Clippy, all workspace tests, and `git diff --check` passed in the configured Nix shell. Tool versions were Rust/Cargo 1.95.0, rustfmt 1.9.0, Clippy 0.1.95, and TinyCC 0.9.27-unstable-2025-01-06.
- Remaining: additional stale-writer and limit boundary evidence, and separate supervisor/gateway process integration.

## DS-5: schema ownership transfer by inherited role member — verified

- Contract: the current schema owner, including a member inheriting ownership through a role, may transfer a non-system schema to a live user or role. Ownership changes atomically and immediately determine who may perform later schema DDL.
- Regression: the real-redb schema lifecycle test transfers ownership to a role, then has an inherited member transfer it to a user. The previous role member is denied a subsequent rename, while the new owner successfully drops the schema; the test checks durable outcomes and the final commit sequence.
- Verification: `nix develop --no-update-lock-file -c cargo test -p prdb-engine --test commit storage_manager_creates_schema_atomically_for_privileged_actor --locked --offline -- --exact --nocapture` passed (`1 passed, 0 ignored`). `git diff --check` passed.
- Implementation note: this increment confirms the existing guarded storage path and clarifies the schema-owner definition; no production code change was required.

## DS-5 / DS-7: index publication versus a pinned writer — verified

- Behavior and normative contracts: [schema DDL](../../specs/ddl-schema.md), [MVCC concurrency](../../specs/concurrency-mvcc.md), and [storage manager](../../specs/storage-manager.md). Standalone index create, drop, and rebuild advance each affected table's write epoch without changing tuple schema version, so writers pinned to an older index set cannot publish after DDL.
- Implementation: [`commit.rs`](../../engine/src/commit.rs) appends expected-byte table-record epoch updates to the same serialized DDL commit for affected tables that do not already have a table-record update. Combined schema rewrites retain their existing single epoch increment.
- Regression evidence: real-redb lifecycle tests stage row writes before index creation and rebuild, then confirm both conflict with durable ABORTED outcomes and publish no rows. The create/drop lifecycle also checks exact table write-epoch increments. Rebuild assertions verify the old snapshot retains its index definition and memberships while the current snapshot contains only rebuilt forward and reverse memberships.
- TDD note: the new stale-writer assertion first failed because the pinned write committed successfully at sequence 3 after index publication; it passes after index DDL began incrementing the table write epoch.
- Verification: focused real-redb create/drop and rebuild stale-writer regressions passed. In the configured Nix shell, workspace build, format check, warning-denied Clippy, all workspace tests, and `git diff --check` passed after the create/drop fix. Tool versions were Rust/Cargo 1.95.0, rustfmt 1.9.0, Clippy 0.1.95, and TinyCC 0.9.27-unstable-2025-01-06.
- Remaining: separate supervisor/gateway process integration and broader DDL acceptance evidence.

## DS-5 / DS-7: CHECK constraint publication versus a pinned writer — verified

- Behavior and normative contracts: [schema DDL](../../specs/ddl-schema.md), [MVCC concurrency](../../specs/concurrency-mvcc.md), and [storage manager](../../specs/storage-manager.md). A writer pinned before a CHECK constraint is published must not commit using the older table constraint epoch.
- Regression evidence: the real-redb authorized constraint lifecycle stages a tuple against the existing table schema, publishes a CHECK constraint and table write-epoch increment, then verifies the staged writer conflicts, publishes no row, and records an ABORTED outcome.
- Implementation: no code change was needed; the existing serialized constraint DDL path and explicit table epoch update reject the stale commit.
- TDD note: the focused race regression passed against the current implementation.
- Verification: focused regression passed. In the configured Nix shell, workspace build, format check, warning-denied Clippy, all workspace tests, and `git diff --check` passed for this increment. Tool versions were Rust/Cargo 1.95.0, rustfmt 1.9.0, Clippy 0.1.95, and TinyCC 0.9.27-unstable-2025-01-06.
- Remaining: other constraint variants and separate supervisor/gateway process integration.

## DS-5 / DS-7: CHECK constraint removal preserves reader snapshots — verified

- Contract: [schema DDL](../../specs/ddl-schema.md) requires readers pinned before a constraint drop to retain the old catalog view while later readers see the removed constraint and its private expression records.
- Regression: `storage_manager_creates_and_drops_check_constraint` now holds a redb snapshot across `drop_check_constraint`; the pinned snapshot retains the CHECK, expression, and dependency records while a new snapshot sees all three removed.
- Verification: the focused real-redb lifecycle test passed (`1 passed, 0 ignored`). Workspace build, format check, warning-denied Clippy, maintained TinyCC SDK tests, sandbox-query fixture, and release-profile validation (10 binaries and one query fixture) passed. The full serial delegated-cgroup workspace suite passed with 224 library tests, 164 commit tests, and 75 transaction tests, plus all other integration suites; no tests were ignored. `git diff --check` passed.
- Scope: covers one CHECK constraint and its expression dependency; it does not simulate process crash or power loss.

## DS-5 / DS-7: PRIMARY KEY and UNIQUE publication versus a pinned writer — verified

- Behavior: a writer that loaded a table's schema and indexes before PRIMARY KEY and UNIQUE constraints were published cannot commit a row using the stale constraint graph.
- Regression: the real-redb [constraint and table-drop lifecycle test](../../engine/tests/commit.rs) stages a row before the combined PK/UNIQUE catalog commit, then verifies the stale writer receives `Conflict`, publishes no row, and records an ABORTED outcome.
- Implementation: no production change was needed; the existing serialized constraint DDL path advances the table write epoch and the commit guard rejects the stale writer.
- Verification: the focused regression passed in the configured Nix shell.
- Remaining: separate supervisor/gateway process integration and broader DDL acceptance evidence remain open.

## DS-5 / DS-7: FOREIGN KEY removal versus pinned source and target writers — verified

- Behavior: removing a FOREIGN KEY advances both the source and referenced table write epochs, so writers pinned to the prior relationship graph cannot publish rows afterward.
- Regression: the real-redb [FK authority lifecycle test](../../engine/tests/commit.rs) stages one row on each table before authorized FK removal; both writers receive `Conflict`, publish no row, and record ABORTED outcomes.
- Implementation: no production change was needed; the existing serialized foreign-key DDL path protects both table epochs.
- Verification: the focused real-redb FK authority regression passed in the configured Nix shell.
- Remaining: separate supervisor/gateway process integration and broader DDL acceptance evidence remain open.

## DS-5 / DS-7: FOREIGN KEY removal preserves reader snapshots — verified

- Contract: [schema DDL](../../specs/ddl-schema.md) requires a reader pinned before FK removal to retain the relationship, object, and target-dependency records; readers opened after publication see the complete removal.
- Regression: `storage_manager_drops_foreign_key_and_advances_both_table_epochs` now keeps a real redb snapshot across `drop_foreign_key`. The pinned reader retains the FK in the source table's constraint index and all three catalog records, while a fresh reader sees no constraint or records.
- Verification: the focused real-redb FK lifecycle test passed (`1 passed, 0 ignored`). Workspace build, format check, warning-denied Clippy, maintained TinyCC SDK tests, sandbox-query fixture, and release-profile validation (10 binaries and one query fixture) passed. The full serial delegated-cgroup workspace suite passed with 224 library tests, 164 commit tests, and 75 transaction tests, plus all other integration suites; no tests were ignored. `git diff --check` passed.
- Scope: covers one FK over the existing fixture tables; it does not simulate process crash or power loss.

## DS-5: role-owned table authority follows current membership — verified

- Behavior: a role member can use owner-derived table DDL while membership is active, then loses that authority after an administrator revokes the membership.
- Regression: the real-redb [table rename and ownership lifecycle test](../../engine/tests/commit.rs) transfers ownership to a role, verifies the member can rename the table, revokes membership through a credentialed database administrator, and confirms the former member's next rename is denied with an ABORTED outcome and leaves the table name unchanged.
- Test setup: the initial attempt to revoke membership from the fixture's non-credentialed owner was correctly denied because changing a role's membership requires an authorized administrative caller. The fixture now provisions a credentialed database administrator and performs the revocation through the supported API.
- Verification: the focused real-redb lifecycle test passed in the configured Nix shell.
- Remaining: separate supervisor/gateway process integration and broader DDL acceptance evidence remain open.

## DS-5 / NP-6: schema, table, primary-key, and foreign-key DDL through separate service processes — verified

- Contract: [schema DDL](../../specs/ddl-schema.md) and [client protocol](../../specs/client-protocol.md). Authenticated control operations create and drop schema/table definitions through the deployed TCP gateway and supervisor, with storage validating and committing each operation.
- Red evidence: the first process attempt to create the foreign key was rejected because the fixture had not granted `REFERENCES` on the target key column. Adding that explicit grant made the operation succeed, confirming database authentication alone does not supply this key-level authority.
- Regression: `supervisor_and_gateway_binaries_commit_a_confined_submission` uses the public `AdminClient` against both production binaries to create a schema, UUID identity table and PRIMARY KEY with its backing index; grant `REFERENCES` on the key column; create a referencing table and FOREIGN KEY; then drop the referencing table, target table and empty schema. The same connection first checks active query status while the ordinary client is backpressured on a 16 MiB result stream, then checks COMMITTED and lists token metadata before exercising DDL.
- Verification: the focused test passed in the delegated `systemd --user` service with the pinned runtime paths and writable cgroup parent. `cargo fmt --all -- --check`, `cargo build --workspace --locked`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `make -f engine/sdk/Makefile test`, `make -f engine/sdk/Makefile sandbox-query-fixture`, and the full serial workspace suite all passed. The suite ran with all sandbox fixtures enabled and reported no ignored tests.
- Scope: exercises the schema/table ADMIN lifecycle across process boundaries on a real READY instance. DDL crash recovery, wider operation families, and the complete DS-5 ALTER matrix remain open.

## SM-5 / DS-5: external foreign-key RESTRICT on table drop — verified

- Contract: [schema DDL](../../specs/ddl-schema.md) requires an external dependency to block `DROP TABLE ... RESTRICT`; failed DDL must leave catalog state unchanged and record a durable ABORTED outcome.
- Regression: `storage_manager_drop_table_restricts_external_foreign_key_dependencies` creates a source FK referencing a target table, attempts to drop the target through `StorageManager`, and verifies `TableHasDependencies`, unchanged commit sequence and both tables/FK edge, an ABORTED outcome, and successful persisted-record validation.
- Verification: the focused commit integration test and full workspace build, formatting check, warning-denied Clippy, and test suite passed under Rust 1.95.0. The commit integration target had 110 passed and 3 delegated-cgroup tests ignored.
- Scope: verifies the referenced-table RESTRICT path and durable failure behavior. The source-table path, which owns and removes its FK, has separate coverage; broader external constraint/policy and DDL race coverage remain open.

## SM-5 / DS-5: FK creation racing with referenced-table drop — verified

- Contract: [schema DDL](../../specs/ddl-schema.md) requires FK creation and referenced-table removal to serialize so neither operation can publish a dangling reference or silently remove a committed constraint.
- Regression: `foreign_key_creation_racing_referenced_table_drop_is_atomic` releases an authorized FK add and target-table `DROP ... RESTRICT` together from a barrier. It accepts only one committed DDL operation, checks both durable outcomes and the single commit-sequence advance, validates the resulting catalog shape, and reopens through `validate_persisted_records`.
- Verification: the focused race test passed. Workspace build, formatting check, warning-denied Clippy, and full workspace tests passed with Rust 1.95.0; the commit integration target had 111 passed and 3 delegated-cgroup tests ignored.
- Scope: one barrier-coordinated schedule per run; repeated stress and races with other external constraint/policy changes remain open.

## SM-5 / DS-5: UNIQUE backfill racing with duplicate insertion — verified

- Contract: [schema DDL](../../specs/ddl-schema.md) requires unique constraint validation against the serialized final table state; a concurrent insert and constraint backfill cannot both publish duplicate values.
- Regression: `unique_constraint_backfill_racing_duplicate_insert_is_atomic` stages a duplicate-value insert while adding an owned UNIQUE constraint and its backing index, then releases both commits from a barrier. It accepts only either a committed insert with aborted backfill or committed backfill with the stale insert aborted. It verifies both outcomes, exactly one commit-sequence advance, matching row/index presence, and startup integrity.
- Verification: the focused race test passed. Workspace build, formatting check, warning-denied Clippy, and full workspace tests passed with Rust 1.95.0; the commit integration target had 112 passed and 3 delegated-cgroup tests ignored.
- Scope: one barrier-coordinated schedule per run; repeated race stress and races involving other constraint and policy changes remain open.

## SM-5 / DS-7: FK removal racing with referenced-table drop — verified

- Contract: removing an FK and dropping its referenced table must serialize so no committed table remains with a dangling dependency and no committed table drop leaves the FK behind.
- Regression: `foreign_key_removal_racing_referenced_table_drop_is_atomic` releases authorized FK removal and target-table drop together. It accepts FK removal with either a successful subsequent table drop, a RESTRICT abort, or a stale-catalog conflict; checks durable outcomes and sequence advances; verifies source/FK/target shape; and runs startup integrity validation.
- Evidence: the first run observed a stale-catalog `Conflict` for the table drop after FK removal had serialized first. The test records that as a safe durable abort alongside the explicit dependency RESTRICT result. The focused barrier test passes.
- Verification: workspace build, format check, warning-denied Clippy, and all workspace tests passed under Rust 1.95.0. The commit integration target had 116 passed and 3 delegated-cgroup tests ignored.

## SM-5 / DS-7: FK creation racing with referenced key removal — verified

- Contract: foreign-key creation and PRIMARY KEY removal must serialize so the catalog never commits an FK with a missing referenced key or drops a key while a committed FK still depends on it.
- Regression: `foreign_key_creation_racing_referenced_primary_key_drop_is_atomic` releases the authorized FK catalog commit and typed PK drop from a barrier. It accepts only one committed DDL operation, verifies both durable outcomes, checks the corresponding FK/key/dependency records, and runs startup integrity validation.
- Evidence: the focused barrier-coordinated regression passed against real redb.
- Verification: the focused barrier regression passed. `cargo build --workspace --locked`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo test --workspace --locked` passed under Rust 1.95.0; the commit integration target had 121 passed and 3 cgroup-dependent tests ignored.

## SM-5 / DS-7: CHECK creation racing with a violating insert — verified

- Contract: CHECK publication must validate the serialized final row set, and an invocation pinned before constraint publication cannot commit using the older table write epoch.
- Regression: `storage_manager_creates_and_drops_check_constraint` races a non-NULL insert against publishing `CHECK (column IS NULL)` from a three-party barrier. It accepts only either a committed insert with CHECK backfill durably aborted as `CheckViolation`, or committed CHECK DDL with the stale insert rejected as `Conflict`. Both paths verify durable outcomes, exactly one sequence advance, and row presence consistent with the winner; startup validation succeeds after cleanup.
- Verification: the focused real-redb lifecycle/race regression passed. `cargo build --workspace --locked`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo test --workspace --locked` passed in the prescribed Nix shell; the commit integration target reported 121 passed and 3 cgroup-dependent ignored.

## SM-5 / DS-7: FK creation racing with an orphan child insert — verified

- Contract: FK creation validates existing child rows against the referenced key set, while a writer pinned before FK publication must conflict after the source table epoch advances.
- Regression: `foreign_key_creation_racing_orphan_child_insert_is_atomic` releases an orphan child insert and typed FK creation from a barrier. It accepts only a committed insert with durable FK `ForeignKeyViolation`, or committed FK creation with the stale insert rejected as `Conflict`. Both outcomes verify durable statuses, one commit-sequence advance, and row/catalog state consistent with the winner; any published FK is removed before startup validation.
- Verification: the focused real-redb barrier regression passed ten consecutive fresh-database runs, exercising FK cleanup when DDL wins. Full workspace build, format, warning-denied Clippy, and tests passed in the prescribed Nix shell; the commit integration target reported 123 passed and 3 cgroup-dependent ignored.

## SM-5 / DS-7: index DDL racing with table ownership transfer — verified

- Contract: index DDL must recheck current owner/grant authority under serialized commit, so a former table owner cannot publish an index after ownership changes.
- Regression: `index_ddl_racing_table_owner_transfer_rechecks_authority_at_commit` releases an old owner's index creation and administrator's table-owner transfer from a barrier. It permits the index to commit before transfer, or the transfer to commit while the index aborts with `DdlAuthorizationDenied`; a transfer planned before a competing table-epoch change may instead abort with `Conflict`. Durable outcomes, sequence increments, index presence, final owner, and startup integrity are checked.
- Verification: the focused real-redb barrier regression passed. `cargo build --workspace --locked`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo test --workspace --locked` passed in the prescribed Nix shell; the commit integration target reported 123 passed and 3 cgroup-dependent ignored.

## SM-5 / DS-7: same-profile index rebuild — implemented

- Contract: `indexes.rebuild` rebuilds and validates physical memberships even when the requested columns and uniqueness profile match the existing definition; it records audit/outcome and advances the table write epoch.
- Red evidence: the focused real-redb populated-index regression initially failed with `Commit(InvalidCatalog)` when repeating the just-committed definition. The real gateway/supervisor test also showed the same behavior on an empty table.
- Implementation: identical family-9 definition writes are now treated as explicit rebuild intent during table-fence discovery, index authorization, and backfill planning. Rebuilds with no rows still enter the authorized commit path and commit the durable audit/outcome plus table epoch.
- Regression: the populated redb test repeats the same definition and checks the next commit sequence and COMMITTED outcome. The real-process ADMIN test now rebuilds first to a changed uniqueness profile, then repeats that profile through gateway/supervisor, and finally drops the index.
- Verification: both focused regressions passed. `cargo build --workspace --locked`, `cargo fmt --all -- --check`, warning-denied workspace Clippy, and `cargo test --workspace --locked` passed under Rust/Cargo 1.95.0; the commit integration target reported 124 passed and 3 cgroup-dependent ignored.

## SM-5 / DS-7: policy creation conflicts with stale writers — verified

- Contract: a policy DDL commit advances its table write epoch; a writer pinned before that authority change must not commit using stale policy state.
- Regression: the named-index lifecycle test now stages a row write, creates a row policy through typed `StorageManager::create_policy`, and confirms the stale writer records an ABORTED outcome and publishes no row while policy creation records COMMITTED.
- Verification: the focused real-redb regression passed. Rust 1.95.0 workspace build, formatting, warning-denied Clippy, and full tests passed; the commit integration target reported 124 passed and 3 cgroup-dependent ignored.
- Remaining: race coverage against other DDL families and confined artifact execution remain open.

## SM-5 / DS-7: policy DDL racing with a staged writer — verified

- Contract: policy publication and invocation writes must linearize under the table fence. A writer that commits first may succeed; a writer that reaches finalization after policy publication must conflict and persist ABORTED.
- Regression: a barrier-coordinated real-redb race stages a row write, then concurrently commits it against typed row-policy creation. It accepts either order and checks row visibility, both durable outcomes, and the resulting commit sequence. The focused race test passed in ten fresh runs.
- Verification: focused repeated race passed. Rust 1.95.0 workspace build, format check, warning-denied Clippy, and full test suite passed; the commit integration target reported 124 passed and 3 cgroup-dependent ignored.
- Remaining: races against other DDL families, broader stress schedules, and confined artifact execution remain open.

## SM-5 / DS-7: policy DDL racing with table-owner transfer — verified

- Contract: policy creation must revalidate table ownership at its serialized commit boundary when a concurrent owner transfer changes authority.
- Regression: a barrier-coordinated real-redb race concurrently creates a row policy as the current table owner and transfers ownership as a database administrator. It accepts the valid serial orders, checks durable outcomes and commit-sequence advancement, verifies the resulting policy and table owner, and validates the recovered catalog image.
- Verification: focused test passed once and in ten fresh processes. Rust 1.95.0 workspace build, format check, warning-denied Clippy, and full test suite passed; the commit integration target reported 125 passed and 3 cgroup-dependent ignored.
- Remaining: races against other DDL families, broader stress schedules, and confined artifact execution remain open.

## DS-5 / NP-6: column ALTER operations through gateway and supervisor processes — verified

- Contract: [schema DDL](../../specs/ddl-schema.md) and [client protocol](../../specs/client-protocol.md). A non-admin table owner must be able to administer its own columns through the authenticated control path, with storage preserving stable column identity and enforcing the declared changes.
- Regression: the production `prdb-gateway` and `prdb-supervisor` process acceptance creates a schema and table as a regular user with `CREATE_SCHEMA`, then sets a typed text default, renames the column, sets it NOT NULL, and drops the current default through `AdminClient`. A subsequent authenticated metadata read confirms the column ID is unchanged, its name is `body`, its type remains TEXT, and nullability is false. The existing object grant remains bound to the same column ID.
- Verification: the focused confined process test passed inside a delegated `systemd --user` unit. Workspace format, locked build, warning-denied Clippy, `git diff --check`, and the complete serial delegated-cgroup workspace suite passed with zero ignored tests.
- Scope: covers the `AdminClient`/TCP/gateway/supervisor/storage path for these ALTER operations. The full ALTER matrix, DDL crash recovery, and additional process-level operation families remain open.

## DS-5 / DS-7: DROP COLUMN preserves preexisting reader snapshots — verified

- Contract: [schema DDL](../../specs/ddl-schema.md) requires old read snapshots to retain compatible catalog definitions and physical row versions across a schema rewrite; new snapshots must observe the rewritten schema and rows.
- Regression: `storage_manager_renames_tables_and_drop_column_rewrites_rows` now pins an invocation reader before DROP COLUMN. After publication, it decodes the retained row with the old two-column schema and verifies a new invocation sees the surviving one-column schema and rewritten tuple. The existing stale-writer check continues to require durable abort and no partial row publication.
- Implementation: no storage change was needed; the retained redb snapshot and schema cache already preserve old-reader semantics. This increment adds the missing real-redb acceptance evidence.
- Verification: the focused commit integration test passed; `cargo build --workspace --locked`, `cargo fmt --all -- --check`, warning-denied workspace Clippy, TinyCC SDK tests and sandbox fixture build passed. The full serial workspace suite passed inside the delegated `systemd --user` cgroup with 222 library tests, 150 commit tests, 75 transaction tests, all sandbox/process tests enabled, and zero ignored tests. The cgroup probe reported writable v2 delegation with `cpu`, `memory`, and `pids`; `git diff --check` passed.
- Remaining: other DDL families still need old-reader coverage, and DDL interruption/reopen publication tests remain open.

## DS-6 / DS-7: index DDL process-crash recovery — verified

- Contract: [schema DDL](../../specs/ddl-schema.md) requires an index to become usable only after complete atomic publication; catalog definitions, both physical membership directions, audit, outcome, and commit sequence must recover at one committed state.
- Regression: `process_crash_during_authorized_index_changes_recovers_all_or_none` creates, removes, and rebuilds a populated index in child processes, with exits at the backend write boundary and after each DDL call returns. The rebuild changes its indexed column. Reopen validates the old or new catalog/object/schema dependency, table epoch, forward locator, reverse membership, audit, durable outcome, commit sequence, preserved row, and old/new key memberships. Every recovered database passes startup validation.
- Scope: this exercises abrupt process loss at the backend write boundary and after durable API completion for index creation, removal, and rebuild. It does not simulate power loss or claim filesystem-level durability beyond redb's configured sync contract.
- Verification: focused six-case real-redb process-crash test passed. Locked workspace build, format check, warning-denied Clippy, maintained TinyCC SDK tests, release build/profile validation (10 binaries and one query fixture), and delegated cgroup probe passed. The complete serial delegated-cgroup workspace suite passed with 224 library tests, 168 commit integration tests, and 75 transaction tests; all other integration targets passed and no tests were ignored. `git diff --check` passed.
- Remaining: crash/reopen coverage for other multi-record DDL publications and actual power-loss validation remain open.

## DS-5 / DS-7: read-only invocation completes across table rename — verified

- Red evidence: an admitted invocation loaded a table schema before `rename_table`, retained the old redb catalog snapshot, and correctly saw the old definition; its read-only COMMIT then failed with `Conflict` because commit validation still checked data catalog guards even though it published no rows or index changes.
- Implementation: `InvocationTransaction::commit_inner` skips the family-2 display-name object and family-5 table epoch guards when no row or index writes are being published. Read-only invocations record their durable outcome against their pinned table definition after a rename; other catalog guards still revalidate. Staged writes retain the full catalog conflict checks.
- Regression: `storage_manager_renames_tables_and_drop_column_rewrites_rows` verifies old and new catalog snapshots resolve their respective table names, the old invocation retains its loaded schema across rename, both read-only invocations commit, and the stale writer still aborts without publishing its row.
- Verification: focused rename, token-revocation, schema-guard, and index-guard integration tests passed. Workspace build, format, warning-denied Clippy, TinyCC SDK tests and sandbox fixture, release-profile validation, delegated cgroup probe, and the full serial delegated-cgroup workspace suite passed. The full suite included 224 library tests, 154 commit tests, and 75 transaction tests; all targets reported zero ignored tests. `git diff --check` passed.

## DS-6 / DS-7: schema, table, and column rename process-crash recovery — verified

- Contract: [schema DDL](../../specs/ddl-schema.md) requires schema/table/column names, their catalog/object records, affected table epoch, audit event, operation outcome, and commit sequence to recover as one atomic publication.
- Regression: `process_crash_during_table_rename_recovers_all_or_none` creates a real schema/table and exits child processes at the backend write boundary or immediately after `rename_schema`, `rename_table`, or `rename_column` returns. Reopen validates startup and requires old or new name resolution, stable schema/table/column IDs and column ordinal, audit, durable outcome, and commit sequence to agree at both outcomes.
- Verification: focused six-case real-redb process-crash test passed. Workspace build, format check, warning-denied Clippy, TinyCC SDK tests and sandbox fixture, release-profile validation, delegated cgroup probe, and the complete serial delegated-cgroup workspace suite passed with 224 library tests, 168 commit integration tests, and 75 transaction tests; all other integration targets passed and no tests were ignored. `git diff --check` passed.

## DS-6 / DS-7: schema create/drop process-crash recovery — verified

- Contract: [schema DDL](../../specs/ddl-schema.md) requires creation and removal of the schema record and owned object record to publish atomically with the audit event, durable outcome, and commit sequence.
- Regression: `process_crash_during_schema_create_or_drop_recovers_all_or_none` exercises create and empty-schema drop at the backend-write and post-commit boundaries. Reopen validates startup and requires both catalog records, name lookup, audit, outcome, and sequence to reflect either the full pre-DDL or committed state.
- Verification: focused four-case real-redb process-crash test passed. Workspace build, format check, warning-denied Clippy, TinyCC SDK tests, release-profile validation, delegated cgroup probe, and the complete serial delegated-cgroup workspace suite passed; counts are recorded in the current handoff. `git diff --check` passed.
- Scope: process exit tests atomic backend recovery and durable API completion; it does not simulate power loss or extend redb's filesystem durability guarantee.

## DS-6 / DS-7: schema owner-transfer process-crash recovery — verified

- Contract: schema ownership, object version, authority dependencies, audit, durable outcome, and commit sequence recover as one serialized publication.
- Regression: `process_crash_during_schema_owner_transfer_recovers_all_or_none` transfers a populated schema's owner in a child process and exits at the backend write boundary or after the API returns. Reopen through `StorageManager` validates startup and verifies the previous owner/version on the interrupted write, or the new built-in role owner with one version/sequence advance, audit, and committed outcome after completion.
- Verification: formatting, locked offline workspace build, and warning-denied all-target Clippy passed. The focused two-boundary process-crash test passed both directly and inside the delegated user service. An initial broad commit-target run from the desktop scope failed its 12 cgroup-dependent cases because delegation and fixture/runtime variables were not configured. The subsequent full serial workspace suite passed inside the delegated user service; the commit target reported 171 passed, sandbox/process tests ran enabled, and no tests were ignored.
- Scope: this adds recovery evidence for the implemented transfer path; it does not simulate power loss.

## DS-5: schema owner transfer preserves pinned readers — verified

- Regression: `storage_manager_creates_schema_atomically_for_privileged_actor` now retains a redb reader across ownership transfer. That reader continues to report the original owner and object version, while a reader opened after publication reports the successor role and incremented version.
- Test-fixture correction: an initial run retained the original redb value guard beyond the assertion and triggered redb's freed-page reference assertion on a later fixture commit. The test now drops that guard while keeping the read transaction pinned, then passes the old-reader check and the full lifecycle.
- Verification: the focused lifecycle test passed (`1 passed, 0 ignored`), formatting passed, warning-denied Clippy for the commit integration target passed, and the full serial workspace suite passed inside the delegated user service with all sandbox/process tests enabled and zero ignored tests.
- Implementation note: the storage behavior was already implemented; this increment adds explicit old-reader evidence.

## DS-5: table owner transfer preserves pinned readers — verified

- Regression: `storage_manager_renames_tables_and_drop_column_rewrites_rows` now retains a redb catalog snapshot across `set_table_owner`. The pinned view keeps the original owner, object version, and table epoch; a new view sees the successor role with the object version and table epoch each advanced once. The existing stale writer still aborts after transfer without publishing its row.
- Verification: the focused lifecycle test passed (`1 passed, 0 ignored`), formatting passed, warning-denied Clippy for the commit integration target passed, and `git diff --check` passed.
- Implementation note: transfer behavior was already implemented; this increment adds explicit snapshot evidence for both ownership and its write fence.

## DS-6 / DS-7: table owner-transfer process-crash recovery — verified

- Contract: table ownership, object version, table write epoch, audit, durable outcome, and commit sequence recover as one publication.
- Regression: `process_crash_during_table_owner_transfer_recovers_all_or_none` creates a real schema/table and successor role, then exits at the backend-write boundary or after `set_table_owner` returns. Reopen through `StorageManager` validates startup and checks the old owner/version/epoch on the interrupted write or the successor role with version, epoch, and sequence advanced once after completion; audit and outcome agree with the recovered state.
- Verification: the focused two-boundary process-crash test passed inside the delegated user service (`1 passed, 0 ignored`); formatting, warning-denied Clippy for the commit integration target, and `git diff --check` passed.
- Scope: abrupt process exit does not simulate power loss.

## DS-6 / DS-7: DROP COLUMN process-crash recovery — verified

- Contract: [schema DDL](../../specs/ddl-schema.md) requires the new table/column catalog, rewritten rows, audit event, outcome, and commit sequence to publish atomically.
- Regression: `process_crash_during_drop_column_recovers_all_or_none` creates a two-column table with a row, exits a child at the backend write boundary or after `drop_column` returns, then reopens and checks schema version, column presence/ordinal, row tuple shape/value, audit, outcome, and commit sequence.
- Verification: focused two-boundary process-crash test passed. Workspace format check, warning-denied Clippy, and full serial delegated-cgroup suite passed; the suite included 224 library tests, 156 commit tests, and 75 transaction tests, with zero ignored tests across all targets. TinyCC SDK tests and sandbox fixtures, release-profile validation (10 binaries and one query fixture), delegated cgroup probe, and `git diff --check` passed.
- Scope: process exit covers redb atomic recovery for one row rewrite. It does not simulate power loss or validate larger rewrite limits.

## DS-6 / DS-7: ADD COLUMN process-crash recovery — verified

- Contract: [schema DDL](../../specs/ddl-schema.md) requires the appended column definition, backfilled rows, audit event, durable outcome, and commit sequence to publish atomically.
- Regression: `process_crash_during_add_column_recovers_all_or_none` creates a real schema/table with a row, exits a child at the backend write boundary or after `add_table_column` returns, then reopens and checks schema version, appended-column presence, preserved column ordinal and value, rewritten nullable tuple, audit, outcome, and sequence. Startup validation runs after both recovery paths.
- Verification: focused two-boundary process-crash test passed. Workspace build, format check, warning-denied Clippy, maintained TinyCC SDK tests, release-profile validation (10 binaries and one query fixture), and the delegated cgroup probe passed. The complete serial workspace suite passed in the delegated `systemd --user` service: 224 library tests, 157 commit tests, 75 transaction tests, all sandbox/process tests enabled, and zero ignored tests. `git diff --check` passed.
- Scope: process exit covers redb atomic recovery for one nullable-column rewrite. It does not simulate power loss or validate larger rewrite limits.

## DS-6 / DS-7: DROP TABLE process-crash recovery — verified

- Contract: [schema DDL](../../specs/ddl-schema.md) requires removal of the table, columns, index definition and memberships, rows, audit event, durable outcome, and commit sequence as one publication.
- Regression: `process_crash_during_drop_table_recovers_all_or_none` drops a populated table owning a plain index, PRIMARY KEY, self-referential FOREIGN KEY, UNIQUE constraint, CHECK expression, row policy, REFERENCES grant, and SELECT grant. The write-boundary recovery retains all three physical indexes and both membership directions for each, all four constraints and their object records, the CHECK expression, policy and expression records with dependency edges, both grants and authority edges, FK target dependency, row, and old sequence without a drop audit or outcome. Post-commit recovery removes the complete owned graph and row, then recovers the audit, committed outcome, and one sequence advance. Both reopened images pass startup validation.
- Verification: focused two-boundary process-crash test passed. Workspace build, format check, warning-denied Clippy, maintained TinyCC SDK tests, release-profile validation (10 binaries and one query fixture), and delegated cgroup probe passed. The complete serial workspace suite passed in the delegated `systemd --user` service: 224 library tests, 169 commit tests, and 75 transaction tests, including confined SQL and process integration tests.
- Scope: process exit checks atomic recovery for one table with one row, two owned key constraints, one self-referential FK, one CHECK, one additional index, one row policy, and REFERENCES/SELECT grants. It does not simulate power loss, cover incoming external-FK rejection at the crash boundary, or every owned-object combination.

## DS-6: rejected DROP TABLE outcome survives process exit — verified

- Contract: [schema DDL](../../specs/ddl-schema.md) requires an incoming foreign-key dependency to reject `DROP TABLE ... RESTRICT` without removing either table or the FK edge; failed DDL records a durable ABORTED outcome.
- Regression: `process_exit_after_rejected_table_drop_preserves_dependencies` seeds the real catalog with a target table, source table, and FK dependency, then has a child process attempt the restricted drop and exit immediately after the expected error. Reopen verifies both tables, both constraint records, and the FK target edge remain, the outcome is ABORTED, the commit sequence did not advance, and startup validation passes.
- Red/green: lifecycle coverage already demonstrated the rejection and durable abort; this increment adds the missing process-exit and reopen evidence for that failure path.
- Verification: the focused child-process regression passed. Workspace build, format check, warning-denied Clippy, maintained TinyCC SDK tests, release-profile validation (10 engine binaries and one query fixture), and delegated cgroup probe passed. The complete serial workspace suite passed in the delegated `systemd --user` service: 224 library tests, 170 commit tests, and 75 transaction tests, including sandbox and process integration tests; no tests were ignored.
- Scope: the child exits after `drop_table` returns the durable dependency error. It does not inject a process exit during the aborted-outcome write or simulate power loss.

## DS-3 / DS-7: CREATE TABLE process-crash recovery — verified

- Contract: [schema DDL](../../specs/ddl-schema.md) requires a table definition and its typed columns/name indexes to publish with the audit event, durable outcome, and commit sequence as one transaction.
- Regression: `process_crash_during_create_table_recovers_all_or_none` creates an authorized schema, then exits a child at the backend write boundary or after `create_table` returns. Reopen and startup validation show either no table name, audit, outcome, or sequence advance, or a resolvable table and column with valid typed records, audit, committed outcome, and one sequence advance.
- Verification: focused two-boundary process-crash test passed. Workspace build, format check, warning-denied Clippy, maintained TinyCC SDK tests, release-profile validation (10 binaries and one query fixture), delegated cgroup probe, and the complete serial delegated-cgroup workspace suite passed: 224 library tests, 159 commit tests, and 75 transaction tests, including confined SQL and process integration tests.
- Initial scope: process exit covered one table column without constraints and did not simulate power loss or cover constraint/default combinations. The primary-key follow-up below extends the constraint coverage.

### CREATE TABLE with PRIMARY KEY crash-recovery follow-up — verified

- `process_crash_during_create_table_recovers_all_or_none` now creates the table and PRIMARY KEY/backing index in one operation. At the interrupted-write boundary it requires no table, constraint, audit, outcome, or sequence advance; after commit it checks that the table, column, PRIMARY KEY, owned index, dependency, audit, committed outcome, and single sequence advance are all recovered, then startup validation passes.
- The focused two-boundary process-crash regression passed. Locked workspace formatting, warning-denied all-target Clippy, maintained TinyCC SDK tests and sandbox fixture, and the full serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests (273 library, 174 commit, 75 transaction tests; all other targets passed). `Cargo.lock` is unchanged.
- Scope: the recovery test now covers an atomic PRIMARY KEY and owned index, while separate tests cover CHECK, UNIQUE, and FOREIGN KEY creation recovery. It does not simulate power loss or cover every combination of defaults and multiple constraint kinds in one CREATE TABLE.

## DS-4 / DS-7: UNIQUE constraint process-crash recovery — verified

- Contract: [schema DDL](../../specs/ddl-schema.md) requires the UNIQUE constraint, its owned index definition and memberships, table epoch, audit event, durable outcome, and commit sequence to publish atomically after validating existing rows.
- Regression: `process_crash_during_unique_constraint_creation_recovers_all_or_none` seeds a real table row, then exits a child at the backend write boundary or after `create_unique_constraint` returns. Reopen and startup validation require either no constraint, audit, outcome, or sequence advance, or the complete UNIQUE definition and owned index with forward locator, reverse membership, preserved row, committed outcome, and one sequence advance.
- Verification: focused two-boundary process-crash test passed. Workspace build, format check, warning-denied Clippy, maintained TinyCC SDK tests, release-profile validation (10 binaries and one query fixture), delegated cgroup probe, and the complete serial delegated-cgroup workspace suite passed: 224 library tests, 160 commit tests, and 75 transaction tests, including confined SQL and process integration tests.
- Scope: process exit covers one UNIQUE constraint over one non-null integer column and one row. It does not simulate power loss or cover composite keys, duplicate rejection, or PRIMARY KEY publication.

## DS-4 / DS-7: CHECK constraint process-crash recovery — verified

- Contract: [schema DDL](../../specs/ddl-schema.md) requires a CHECK constraint, its private expression and dependency edge, table write epoch, audit event, durable outcome, and sequence to publish atomically.
- Regression: `process_crash_during_check_constraint_creation_recovers_all_or_none` exits a child at the backend-write boundary or after `create_check_constraint` returns. Reopen and startup validation require either no CHECK and unchanged epoch/sequence/audit/outcome, or the complete CHECK/expression/dependency set with one epoch and sequence advance, preserved row, audit, and committed outcome.
- Verification: the focused two-boundary process-crash test passed (`1 passed, 0 ignored`). Workspace build, format check, warning-denied Clippy, maintained TinyCC SDK tests, sandbox-query fixture, and release-profile validation (10 binaries and one query fixture) passed. The full serial delegated-cgroup workspace suite passed with 224 library tests, 165 commit tests, and 75 transaction tests, plus all other integration suites; no tests were ignored. `git diff --check` passed.
- Scope: process exit covers a true literal CHECK on one existing fixture table; it does not simulate power loss or cover dependent-column expressions.

## DS-4 / DS-7: FOREIGN KEY process-crash recovery — verified

- Contract: [schema DDL](../../specs/ddl-schema.md) requires the FK constraint, target dependency, source and target table epochs, audit event, durable outcome, and commit sequence to publish atomically after validating existing rows and REFERENCES authority.
- Regression: `process_crash_during_foreign_key_creation_recovers_all_or_none` seeds matching rows in two real tables, grants REFERENCES on the target key, then exits a child at the backend write boundary or after `create_foreign_key` returns. Reopen and startup validation require either the complete pre-FK state with unchanged table epochs and no audit/outcome/sequence advance, or the FK catalog/object/dependency records, both incremented epochs, preserved rows, audit, committed outcome, and one sequence advance.
- Verification: focused two-boundary process-crash test passed. Workspace build, format check, warning-denied Clippy, maintained TinyCC SDK tests, sandbox fixture build, release-profile validation (10 binaries and one query fixture), delegated cgroup probe, and the complete serial delegated-cgroup workspace suite passed: 224 library tests, 161 commit tests, and 75 transaction tests, including confined SQL and process integration tests, with no ignored tests.
- Scope: process exit checks redb atomic recovery for one FK over one non-null integer key and matching rows. It does not simulate power loss or cover composite and UNIQUE referenced keys.

## DS-4 / DS-7: PRIMARY KEY process-crash recovery — verified

- Contract: [schema DDL](../../specs/ddl-schema.md) requires the PRIMARY KEY constraint, owned unique index definition and memberships, table write epoch, audit event, durable outcome, and commit sequence to publish atomically after validating existing rows.
- Regression: `process_crash_during_primary_key_creation_recovers_all_or_none` seeds a real non-null integer row, then exits a child at the backend write boundary or after `create_primary_key` returns. Reopen and startup validation require either no constraint, owned index, audit, outcome, or sequence/epoch advance, or the complete PRIMARY KEY and backing index with forward locator, reverse membership, preserved row, incremented table epoch, audit, committed outcome, and one sequence advance.
- Verification: focused two-boundary process-crash test passed. Workspace build, format check, warning-denied Clippy, maintained TinyCC SDK tests, release-profile validation (10 binaries and one query fixture), and the complete serial delegated-cgroup workspace suite passed: 224 library tests, 162 commit tests, and 75 transaction tests, including confined SQL and process integration tests, with no ignored tests. `git diff --check` passed.
- Scope: process exit checks redb atomic recovery for a PRIMARY KEY over one non-null integer column and one row. It does not simulate power loss or cover composite keys and validation failure cases.

## DS-5 / DS-7: PRIMARY KEY drop preserves existing reader snapshots — verified

- Contract: [schema DDL](../../specs/ddl-schema.md) requires a reader pinned before constraint removal to retain its catalog view while readers opened after publication see the complete new state.
- Regression: `storage_manager_creates_primary_key_with_owned_unique_index` now holds a redb reader across `drop_primary_key`. The old snapshot retains the PRIMARY KEY, backing index, their catalog objects, and ownership dependency; a new snapshot contains none of those records. The existing test also verifies a stale writer cannot commit after PRIMARY KEY publication and startup validation passes after removal.
- Verification: the focused real-redb lifecycle test passed. Workspace build, format check, warning-denied Clippy, TinyCC SDK tests, release-profile validation, and the full serial delegated-cgroup workspace suite passed: 224 library tests, 162 commit tests, and 75 transaction tests, with sandbox/process acceptance enabled and no ignored tests. `git diff --check` passed.
- Scope: this verifies retained catalog snapshots for PRIMARY KEY removal. It does not add physical index membership rows to this fixture or simulate power loss.

## DS-5 / DS-7: UNIQUE constraint drop preserves reader snapshots — verified

- Contract: [schema DDL](../../specs/ddl-schema.md) requires a reader pinned before UNIQUE removal to retain the constraint, owned index, catalog objects, and ownership edge; new readers see the removed set.
- Regression: `storage_manager_creates_unique_constraint_and_backfills_existing_rows` now holds a redb snapshot across `drop_unique_constraint`. The old reader retains the UNIQUE/index records and the new reader reports no constraint or surviving records.
- Verification: the focused real-redb lifecycle test passed (`1 passed, 0 ignored`). Workspace build, format check, warning-denied Clippy, maintained TinyCC SDK tests, sandbox-query fixture, and release-profile validation (10 binaries and one query fixture) passed. The full serial delegated-cgroup workspace suite passed with 224 library tests, 164 commit tests, and 75 transaction tests, plus all other integration suites; no tests were ignored. `git diff --check` passed.
- Scope: covers one single-column UNIQUE constraint; it does not simulate process crash or power loss.

## DS-4 / DS-7: key-constraint drop process-crash recovery — verified

- Contract: [schema DDL](../../specs/ddl-schema.md) requires removal of a PRIMARY KEY or UNIQUE constraint, its owned index, both object records, ownership dependency and physical index memberships to publish atomically with the table write epoch, audit, outcome, and commit sequence.
- Regression: `process_crash_during_key_constraint_drop_recovers_all_or_none` creates a real non-null integer key and row, then exercises both constraint kinds at the backend-write and post-commit boundaries. Reopen and startup validation require either all catalog/index records and both physical memberships with unchanged epoch/sequence and no drop audit/outcome, or their complete removal with one epoch and sequence advance, preserved row, audit, and committed outcome.
- Verification: all four real-redb process-crash cases passed. Workspace build, format check, warning-denied Clippy, maintained TinyCC SDK tests, sandbox-query fixture, release-profile validation (10 engine binaries and one query fixture), and the full serial delegated-cgroup suite passed. The suite reported 224 library, 163 commit, and 75 transaction tests passed, with no ignored tests. `git diff --check` passed.
- Scope: process exit covers PRIMARY KEY and UNIQUE removal over one integer row. It does not simulate power loss or cover FOREIGN KEY removal.

## DS-4 / DS-7: FOREIGN KEY drop process-crash recovery — verified

- Contract: [schema DDL](../../specs/ddl-schema.md) requires removal of a FOREIGN KEY and its target dependency to publish atomically with both participating table write epochs, audit, outcome, and commit sequence; the referenced key and its index remain intact.
- Regression: `process_crash_during_foreign_key_drop_recovers_all_or_none` creates matching rows in source and target tables, a referenced PRIMARY KEY, and a FOREIGN KEY, then exits a child at the backend-write and post-commit boundaries. Reopen and startup validation require either all FK records/dependency plus unchanged epochs and no audit/outcome/sequence advance, or complete FK removal with both epochs and sequence advanced once, preserved rows, audit, committed outcome, and the referenced PRIMARY KEY's forward and reverse index memberships.
- Verification: the focused two-boundary process-crash test passed. Workspace build, format check, warning-denied Clippy, maintained TinyCC SDK tests, sandbox-query fixture, and release-profile validation (10 binaries and one query fixture) passed. The full serial delegated-cgroup workspace suite passed with 224 library tests, 164 commit tests, and 75 transaction tests, plus all other integration suites; no tests were ignored. `git diff --check` passed.
- Scope: process exit checks redb atomic recovery for one FK between two one-column integer tables. It does not simulate power loss or cover composite keys.

## DS-4 / DS-7: CHECK constraint drop process-crash recovery — verified

- Contract: [schema DDL](../../specs/ddl-schema.md) requires constraint removal to atomically publish the definition, expression, dependency edges, table write epoch, audit event, durable outcome, and commit sequence.
- Regression: `process_crash_during_check_constraint_drop_recovers_all_or_none` creates a CHECK over a populated fixture table, then exits a child at the backend-write boundary or immediately after `drop_check_constraint` returns. Reopen and startup validation require either the full CHECK/expression/dependency set with unchanged epoch and sequence and no drop audit/outcome, or complete removal with one epoch and sequence advance, preserved row, audit, and committed outcome.
- Verification: the focused two-boundary process-crash test passed. Workspace build, format check, warning-denied Clippy, maintained TinyCC SDK tests, release-profile validation (10 engine binaries and one query fixture), and the full serial delegated-cgroup workspace suite passed with 224 library tests, 166 commit tests, and 75 transaction tests; all remaining integration suites passed and no tests were ignored. `git diff --check` passed.
- Scope: process exit checks redb atomic recovery for a literal-TRUE CHECK on one populated fixture table. It does not simulate power loss or cover CHECK expressions with column dependencies.

## DS-5 / DS-7: column default replacement/removal process-crash recovery — verified

- Contract: [schema DDL](../../specs/ddl-schema.md) requires replacing or removing a column's default expression to update the column reference, remove old expression/object/dependency records, add replacement records when applicable, advance the table write epoch, and publish audit/outcome/sequence atomically.
- Regression: `process_crash_during_column_default_change_recovers_all_or_none` creates a real table with a typed integer default, then exercises replacement and removal at the backend-write and post-commit boundaries. Reopen and startup validation require either the original default reference and records with unchanged epoch/sequence and no DDL audit/outcome, the complete replacement reference and records with the old records removed, or complete removal of the reference and records; committed changes advance epoch/sequence once and recover audit and committed outcome.
- Verification: all four real-redb process-crash cases passed. Workspace build, format check, warning-denied Clippy, maintained TinyCC SDK tests, release build/profile validation (10 engine binaries and one query fixture), and the full serial delegated-cgroup workspace suite passed with 224 library tests, 167 commit tests, and 75 transaction tests; all remaining integration suites passed and no tests were ignored. `git diff --check` passed.
- Scope: process exit checks literal integer default replacement/removal. It does not simulate power loss or cover `CURRENT_PRINCIPAL_ID` defaults.

## DS-5 / DS-7: NOT NULL set/drop process-crash recovery — verified

- Contract: [schema DDL](../../specs/ddl-schema.md) requires SET/DROP NOT NULL to change the column nullability flag, create/remove the internal constraint and object records as applicable, advance the table write epoch, and publish audit/outcome/sequence atomically without changing tuple schema version.
- Regression: `process_crash_during_not_null_change_recovers_all_or_none` exercises SET NOT NULL and DROP NOT NULL at the backend-write and post-commit boundaries. Reopen and startup validation require either the complete prior nullable/constraint state with unchanged epoch/sequence and no DDL audit/outcome, or the complete new state with one epoch and sequence advance, audit, and committed outcome.
- Verification: all four real-redb process-crash cases passed. Workspace build, format check, warning-denied Clippy, maintained TinyCC SDK tests, release build/profile validation (10 engine binaries and one query fixture), and the full serial delegated-cgroup workspace suite passed with 224 library tests, 168 commit tests, and 75 transaction tests; all remaining integration suites passed and no tests were ignored. `git diff --check` passed.
- Scope: uses one nullable fixture column with no NULL rows. It does not simulate power loss or retest NULL rejection, which remains covered by the lifecycle test.

## DS-5 / DS-7: CURRENT_PRINCIPAL_ID default process-crash recovery — verified

- Contract: replacing a column's current default with `CURRENT_PRINCIPAL_ID()` must atomically publish its typed expression, object record, column dependency, table write epoch, audit event, durable outcome, and commit sequence. The context default is valid only for UUID columns.
- Regression: `process_crash_during_column_default_change_recovers_all_or_none` now also replaces a UUID column's literal default with `CURRENT_PRINCIPAL_ID()`. It exits at the backend-write boundary and immediately after the operation returns, then reopens the database. Recovery checks the prior or new default reference as appropriate, decodes the committed expression as a default-purpose `CurrentPrincipalId`, validates its object and dependency records, and checks the table epoch, audit, outcome, sequence, and persisted catalog.
- Verification: all six cases across literal replacement, context-default replacement, and default removal passed at both process-exit boundaries. The focused regression, workspace build, format check, warning-denied all-target Clippy, TinyCC SDK suite and sandbox fixture, locked release build, and full serial delegated-cgroup workspace suite passed with zero ignored tests. The full suite reported 233 engine unit tests, 172 commit tests, and 75 transaction tests; `git diff --check` passed.
- Scope: process exits exercise redb atomic recovery for one UUID column with no row values. They do not simulate power loss or test applying the context default to a newly inserted row; ordinary default evaluation remains covered by the existing catalog and DML tests.

## DS-8: basic CREATE TABLE SQL on the administrative control path — implemented

- Contract: [schema DDL](../../specs/ddl-schema.md), [client SDK](../../specs/client-sdk.md), and [SQL library boundary](../../specs/sql-library.md). One schema-qualified CREATE TABLE statement with scalar columns and optional NOT NULL is translated by the authenticated AdminClient into the existing typed `tables.create` PREPARE/START operation. Query invocations continue to reject DDL; storage remains responsible for authorization and catalog validation.
- Implementation: [`ddl_sql.rs`](../../engine/src/ddl_sql.rs) provides a 65,536-byte bounded parser for unquoted normalized identifiers, initial scalar types, DECIMAL/VARCHAR parameters, and optional final semicolon. `AdminClient::execute_create_table_sql` resolves the schema using paged metadata on the same control connection, then calls `create_table`.
- Regression evidence: parser tests accept qualified scalar definitions and reject unqualified names, unsupported inline constraints, multiple statements, quoted identifiers, and invalid Decimal precision. A mock authenticated TCP control-session regression verifies schema discovery, typed `tables.create` PREPARE/START, generated IDs, and commit sequence.
- Verification: `nix develop -c cargo test -p prdb-engine --lib ddl_sql::tests --locked --offline` passed (2 tests). `nix develop -c cargo test -p prdb-engine --test client_admin admin_client_executes_create_table_sql_on_authenticated_control_channel --locked --offline -- --exact` passed (1 test). The initial locked-shell command form `nix develop --locked -c ...` was rejected by the installed Nix CLI; checks ran in the repository shell without that flag.
- Limits at the initial DS-8 commit: this was a client-side SQL convenience over the existing admin protocol, not a SQL endpoint; its first parser slice omitted defaults, generated identities, inline constraints, quoted identifiers, and other DDL syntax. Later entries extend typed defaults, generated identities, and CREATE SCHEMA. Schema lookup for CREATE TABLE requires the schema to be visible through authenticated control metadata.
- Historical next step at this point: add SQL `CREATE SCHEMA` with explicit owner handling. Implemented in the later follow-up entry.


## DS-8 follow-up: typed SQL column defaults — implemented

- Contract: [schema DDL](../../specs/ddl-schema.md) and [client SDK](../../specs/client-sdk.md). SQL defaults are translated into the existing typed `tables.create` request, so table, column, default-expression catalog records, audit, and durable outcome retain the existing atomic publication path.
- Implementation: CREATE TABLE column modifiers now accept optional `NOT NULL` and `DEFAULT` in either order. Supported literals include BOOLEAN, signed integer, exact DECIMAL coefficient/scale conversion, quoted TEXT/VARCHAR with doubled-quote escaping, `X'hex'` BYTEA, quoted UUID, engine-epoch DATE/TIMESTAMPTZ integers, SQL NULL, and UUID `CURRENT_PRINCIPAL_ID()`. Malformed, type-mismatched, out-of-scale, and over-precision values reject before network mutation.
- Regression evidence: parser fixtures cover text escaping, negative DECIMAL, BOOLEAN, BYTEA, literal and context UUID defaults, SQL NULL, modifier order, scale/precision rejection, type mismatch, malformed hex and VARCHAR length. The authenticated mock control-session test verifies SQL DECIMAL and escaped text defaults serialize through the existing typed table-create operation. The DECIMAL regression first failed because the parser advanced over `NOT NULL` after consuming a signed value; token consumption is now explicit and the behavior passes.
- Verification: `nix develop -c cargo fmt --all -- --check`, `nix develop -c cargo clippy --workspace --all-targets --locked --offline -- -D warnings`, `nix develop -c cargo build --workspace --locked --offline`, four focused parser tests, the authenticated AdminClient TCP integration test, and `git diff --check` passed. The maintained TinyCC test suite, sandbox-query fixture build, release-profile validation, delegated cgroup probe, and complete serial delegated-cgroup workspace suite also passed; the workspace suite ran all sandbox and process tests with zero ignored tests.
- CLI follow-up: `prdb-client admin-sql` now exposes this parser over the authenticated control path; process tests prove committed typed CREATE TABLE and pre-auth rejection of unsupported DDL.
- Remaining at the typed-default increment: user-defined checks/keys/foreign keys are not composable atomically with CREATE TABLE yet; generated identity SQL syntax and quoted identifiers were then unsupported.
- Historical next step at this point: SQL generated UUID identity syntax. Implemented in the later follow-up entry.

## DS-8 follow-up: SQL generated UUID identity columns — implemented

- Contract: [schema DDL](../../specs/ddl-schema.md). `UUID GENERATED ALWAYS AS IDENTITY` maps to the existing typed `generated_uuid_identity` table-create field; storage owns allocation and INSERT override checks.
- Regression: parser cases verify identity is non-null with no default and reject non-UUID identity, a combined DEFAULT, and incomplete clauses. The authenticated AdminClient control-session test asserts the typed identity flag in `tables.create`; the CLI process test exercises the same syntax through HELLO/AUTH, schema lookup, PREPARE/START, and commit.
- Implementation: the bounded parser accepts the exact four-token identity clause following UUID and infers NOT NULL while allowing a redundant explicit NOT NULL.
- Verification: all five parser unit tests, the authenticated AdminClient control-session regression, and both `admin-sql` process tests passed. Workspace formatting, locked offline build, warning-denied all-target Clippy, `git diff --check`, and the complete serial delegated-cgroup workspace suite passed; sandbox and process tests ran with zero ignored tests.
- Historical next step at this point: add atomic inline key syntax or CREATE SCHEMA SQL after reviewing composition with typed DDL operations. CREATE SCHEMA is implemented below.

## DS-8 follow-up: CREATE SCHEMA SQL with explicit owner — implemented

- Contract: [schema DDL](../../specs/ddl-schema.md), [client SDK](../../specs/client-sdk.md), and [client SQL command](../../specs/client-sql-command.md). `CREATE SCHEMA name AUTHORIZATION 'uuid'` uses the existing typed `schemas.create` protocol and storage authorization path.
- Implementation: bounded parser accepts normalized schema identifiers and nonzero hexadecimal UUID owner text with optional canonical hyphens. `AdminClient::execute_create_schema_sql` maps the parsed statement to `create_schema`; `admin-sql` validates before credentials/network and reports the committed schema identity.
- Regression: parser tests cover valid UUID form, missing/invalid owner, zero owner, and multiple statements. The authenticated AdminClient test checks typed schema creation followed by rename/owner/drop. A process test drives HELLO/AUTH, typed `schemas.create` PREPARE/START, and committed response.
- Verification: six parser unit tests, the authenticated AdminClient schema lifecycle control-session test, and all three `admin-sql` process tests passed. Workspace formatting, locked offline build, warning-denied all-target Clippy, `git diff --check`, and the complete serial delegated-cgroup workspace suite passed; sandbox and process tests ran with zero ignored tests.
- Next smallest behavior: add inline constraints to CREATE TABLE only after defining an atomic typed composite operation; unsupported forms continue to fail before network access.

## DS-8 follow-up: schema rename and restricted drop SQL — implemented

- Contract: [schema DDL](../../specs/ddl-schema.md), [client SDK](../../specs/client-sdk.md), and [client SQL command](../../specs/client-sql-command.md). SQL names resolve through authenticated visible-schema metadata; storage remains authoritative for ownership, RESTRICT dependencies, and publication.
- Implementation: `ALTER SCHEMA name RENAME TO new_name` and `DROP SCHEMA name RESTRICT` parse as single statements and map to existing `schemas.rename` and `schemas.drop` operations. Resolution uses the same control connection and preserves opaque metadata cursors.
- Regression: parser tests reject incomplete clauses, quoted names, CASCADE, and multi-statement input. The authenticated AdminClient TCP integration renames then drops a schema via SQL and verifies the typed operation arguments and committed results.
- Verification: all seven parser tests and the authenticated AdminClient TCP rename/drop regression passed. Workspace formatting, locked offline build, warning-denied all-target Clippy, `git diff --check`, and the complete serial delegated-cgroup workspace suite passed; sandbox and process tests ran with zero ignored tests.
- Next smallest behavior: add SQL `CREATE TABLE ... PRIMARY KEY` only after extending the typed create operation and storage commit to atomically publish the key and backing index with the table.

## DS-8 follow-up: table rename and restricted drop SQL — implemented

- Contract: [schema DDL](../../specs/ddl-schema.md), [client SDK](../../specs/client-sdk.md), and [client SQL command](../../specs/client-sql-command.md). Qualified table names resolve through the authenticated schema and relation metadata pages; storage retains ownership and dependency validation.
- Implementation: `ALTER TABLE schema.table RENAME TO name` and `DROP TABLE schema.table RESTRICT` map to the existing `tables.rename` and `tables.drop` operations on the same authenticated control connection.
- Regression: parser tests reject unqualified names, incomplete syntax, CASCADE, and multiple statements. The mock TCP AdminClient test resolves schema and relation metadata, checks typed operation arguments for rename/drop, and verifies committed outcomes.
- Verification: all eight parser tests and the authenticated AdminClient TCP rename/drop regression passed. Workspace formatting, locked offline build, warning-denied all-target Clippy, `git diff --check`, and the complete serial delegated-cgroup workspace suite passed; sandbox and process tests ran with zero ignored tests.
- Next smallest behavior: add SQL `CREATE TABLE ... PRIMARY KEY` only after extending the typed create operation and storage commit to atomically publish the key and backing index with the table.

## DS-3: atomic PRIMARY KEY in CREATE TABLE SQL — implemented

- Contract: [schema DDL](../../specs/ddl-schema.md) and [client protocol](../../specs/client-protocol.md). The bounded administrative SQL grammar now accepts one final table-level `PRIMARY KEY (columns...)`; its columns become non-nullable and the table, constraint, owned unique index, dependencies, audit, and outcome publish in one authorized commit.
- Implementation: [SQL parser](../../engine/src/ddl_sql.rs), [table creation](../../engine/src/storage_manager.rs), and commit-time DDL validation in [commit.rs](../../engine/src/commit.rs).
- Regression evidence: composite-key parser coverage checks implied NOT NULL; the authenticated storage lifecycle regression creates a composite key and validates startup; protocol decoding accepts valid keys and rejects nullable or duplicate names; AdminClient and `admin-sql` control requests carry the structured column list.
- Verification: focused parser, decoder, AdminClient, CLI, and storage tests passed. Locked workspace build, format check, warning-denied Clippy, and the full serial delegated-cgroup workspace suite passed with sandbox/process coverage enabled and zero ignored tests. `git diff --check` passed.
- Next: extend bounded CREATE TABLE SQL to support atomic foreign-key definitions; named constraints remain deferred.

## DS-3: atomic UNIQUE constraints in CREATE TABLE SQL — implemented

- Contract: [schema DDL](../../specs/ddl-schema.md) and [client protocol](../../specs/client-protocol.md). The bounded administrative SQL form accepts multiple distinct final table-level `UNIQUE (columns...)` clauses, optionally alongside one PRIMARY KEY. UNIQUE columns may be nullable; NULL-containing tuples remain distinct. Each definition and owned unique index publishes with the new table in one authorized commit.
- Implementation: [SQL parser](../../engine/src/ddl_sql.rs), [table creation](../../engine/src/storage_manager.rs), and create-table DDL validation in [commit.rs](../../engine/src/commit.rs).
- Regression evidence: parser coverage handles multiple composite keys and rejects duplicate, unknown, or repeated columns. The authenticated storage regression atomically creates a composite PRIMARY KEY plus nullable UNIQUE constraint and validates persisted catalog startup. Decoder, AdminClient, and CLI tests assert nested typed arrays.
- Verification: focused parser, decoder, AdminClient, CLI, and storage tests passed. Locked workspace build, format check, warning-denied Clippy, and the full serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests. `git diff --check` passed.
- Next: extend bounded CREATE TABLE SQL to support atomic foreign-key definitions; named constraints remain deferred.

## DS-8: ALTER TABLE SQL for PRIMARY KEY and UNIQUE — implemented

- Contract: [schema DDL](../../specs/ddl-schema.md), [client SDK](../../specs/client-sdk.md), and [client SQL command](../../specs/client-sql-command.md). Qualified `ALTER TABLE schema.table ADD PRIMARY KEY (...)` and `ADD UNIQUE (...)` resolve names through authenticated metadata, then invoke the existing separately authorized constraint operation. PRIMARY KEY columns are checked non-nullable before mutation; storage remains the authority.
- Implementation: bounded statement parsing in [ddl_sql.rs](../../engine/src/ddl_sql.rs), metadata resolution and typed dispatch in [client_submit.rs](../../engine/src/client_submit.rs), and `admin-sql` routing in [prdb-client.rs](../../engine/src/bin/prdb-client.rs). Named constraints remain unsupported because the current typed APIs generate catalog identities.
- Regression: parser tests accept composite key forms and reject unqualified, empty, duplicate, named, and multi-statement input. The authenticated TCP test resolves schema, relation, and columns, then verifies the typed `constraints.create_unique` request and returned IDs. The CLI process test confirms recognized SQL reaches credential loading before network access.
- Verification: focused parser, AdminClient TCP, and CLI tests passed. Locked workspace build, format check, warning-denied Clippy, complete serial delegated-cgroup workspace suite with sandbox/process tests enabled and zero ignored tests, and `git diff --check` passed.
- Next: extend bounded CREATE TABLE SQL to support atomic foreign-key definitions; named constraints remain deferred.

## DS-8: bounded ALTER TABLE CHECK SQL — implemented

- Contract: [schema DDL](../../specs/ddl-schema.md), [client SDK](../../specs/client-sdk.md), and [client SQL command](../../specs/client-sql-command.md). `ALTER TABLE schema.table ADD CHECK (...)` accepts Boolean column references, `IS [NOT] NULL`, `NOT`, `AND`, `OR`, parentheses, and typed scalar comparisons. Names bind through authenticated column metadata to EIR1 field ordinals and stable dependency IDs; typed storage validation, authorization, backfill, write fencing, audit, and durable outcome remain authoritative.
- Regression: parser coverage verifies precedence/grouping, typed literal binding and type mismatch rejection, named constraints, unknown forms, unqualified names, and multiple statements. CLI coverage verifies valid CHECK syntax is recognized before credentials/network.
- Limit at this milestone: ALTER CHECK was unnamed and FOREIGN KEY definitions were unsupported; later named ALTER CHECK support is recorded in DS-SQL-49. Typed APIs retain the same validated EIR1 boundary.
- Verification: focused parser, authenticated AdminClient TCP binding, and CLI pre-credential recognition tests passed. Locked workspace build, format check, warning-denied all-target Clippy, and the complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled; `git diff --check` passed.

## DS-8: ALTER COLUMN SET/DROP NOT NULL SQL — implemented

- Contract: [schema DDL](../../specs/ddl-schema.md), [client SDK](../../specs/client-sdk.md), and [client SQL command](../../specs/client-sql-command.md). Qualified `ALTER TABLE schema.table ALTER COLUMN column SET NOT NULL` and `DROP NOT NULL` bind the named column through authenticated metadata and call the existing typed nullability operation. Storage continues to validate existing rows, enforce DDL authority, and advance the table write epoch under its fence.
- Regression: parser tests cover both operations and reject unqualified tables, malformed actions, and multiple statements. Authenticated TCP coverage checks metadata resolution, stable column ID submission, and committed response; the CLI process test checks pre-credential recognition.
- Limit: validation and storage mutation remain the existing typed path; SQL only adds name resolution and dispatch.
- Verification: parser, authenticated AdminClient TCP, and CLI pre-credential recognition regressions passed. Locked workspace build, format check, warning-denied all-target Clippy, complete serial delegated-cgroup workspace suite with sandbox/process tests enabled, and `git diff --check` passed.

## DS-8: ALTER TABLE ADD COLUMN SQL — implemented

- Contract: [schema DDL](../../specs/ddl-schema.md), [client SDK](../../specs/client-sdk.md), and [client SQL command](../../specs/client-sql-command.md). One typed column can be appended using `ALTER TABLE schema.table ADD COLUMN name type [NOT NULL] [DEFAULT literal]`; NOT NULL requires a compatible non-null literal. Context-dependent defaults are rejected for backfill. Storage keeps the existing bounded atomic row/index/locator rewrite behavior.
- Regression: parser coverage checks supported typed defaults and rejects missing non-null defaults, NULL backfills, current-principal defaults, unqualified names, and multiple statements. Authenticated TCP coverage verifies table resolution and `tables.add_column` arguments; CLI coverage confirms pre-credential syntax recognition.
- Verification: parser, authenticated AdminClient TCP, and CLI pre-credential recognition tests passed. Locked workspace build, format check, warning-denied all-target Clippy, full serial delegated-cgroup workspace suite with sandbox/process tests enabled, and `git diff --check` passed.

## DS-8: ALTER TABLE RENAME COLUMN SQL — implemented

- Contract: [schema DDL](../../specs/ddl-schema.md), [client SDK](../../specs/client-sdk.md), and [client SQL command](../../specs/client-sql-command.md). Qualified column rename resolves the table and column through authenticated metadata, then calls `tables.rename_column`; storage preserves the column ID and applies its existing fenced atomic catalog update.
- Regression: parser tests cover the qualified form and reject malformed/multiple statements. Authenticated TCP coverage verifies the stable ID and typed new name; CLI coverage checks local recognition before credentials/network.
- Verification: parser, authenticated AdminClient TCP binding, and CLI pre-credential recognition tests passed. Locked workspace build, format check, warning-denied all-target Clippy, full serial delegated-cgroup workspace suite with sandbox/process tests enabled, and `git diff --check` passed.

## DS-8: ALTER TABLE DROP COLUMN RESTRICT SQL — implemented

- Contract: [schema DDL](../../specs/ddl-schema.md), [client SDK](../../specs/client-sdk.md), and [client SQL command](../../specs/client-sql-command.md). Qualified `ALTER TABLE schema.table DROP COLUMN column RESTRICT` resolves the stable column ID through authenticated metadata and invokes the typed `tables.drop_column` operation. Storage remains authoritative for dependency checks, row/index rewrite bounds, DDL fencing, audit, and durable outcome.
- Regression: parser coverage rejects missing RESTRICT, CASCADE, unqualified names, and multiple statements. AdminClient TCP coverage verifies column ID binding and committed result; CLI coverage checks recognition before credential/network access.
- Verification: parser, authenticated AdminClient TCP binding, and CLI pre-credential recognition tests passed. Locked workspace build, format check, warning-denied all-target Clippy, full serial delegated-cgroup workspace suite with sandbox/process tests enabled, and `git diff --check` passed.
- Production process follow-up: the real gateway/supervisor metadata lifecycle adds a non-null column with a typed literal default, creates an index on it, confirms `DROP COLUMN ... RESTRICT` is rejected while the index depends on it, drops the index through `admin-sql`, then drops the column and verifies authenticated metadata returns the original column only. The focused delegated process regression, workspace build, format check, warning-denied Clippy, and `git diff --check` pass.

## DS-9: Atomic table-level CHECK in CREATE TABLE SQL — implemented

- Contract: [schema DDL](../../specs/ddl-schema.md), [client SDK](../../specs/client-sdk.md), and [client SQL command](../../specs/client-sql-command.md). CREATE TABLE accepts final table-level CHECK clauses with Boolean column references, `IS [NOT] NULL`, `NOT`, `AND`, `OR`, parentheses, typed comparisons, and context-typed scalar literals. The client resolves names against the declared columns, binds stable IDs/ordinals, and sends canonical EIR1. Storage publishes constraints, expression objects, dependencies, columns, keys, and owned indexes atomically and validates catalog shape and expression bindings.
- Regression: parser name binding, client EIR1 decoding, authenticated AdminClient SQL submission, CLI recognition, and storage catalog publication are covered.
- Limit at this milestone: named constraints and FOREIGN KEY definitions remained unsupported in CREATE TABLE SQL. Named ALTER CHECK support is recorded in DS-SQL-49; CHECK expressions remain bounded to deterministic row-local EIR1.
- Validation: parser, control decoder, authenticated AdminClient, CLI, and storage regressions; locked workspace build; format check; warning-denied all-target Clippy; the full serial delegated-cgroup workspace suite including sandbox/process tests (zero ignored); maintained TinyCC SDK tests; sandbox fixture build; and `git diff --check` passed.


## DS-10: Typed comparisons and literals in CHECK SQL — implemented

- Contract: [schema DDL](../../specs/ddl-schema.md), [client SDK](../../specs/client-sdk.md), and [client SQL command](../../specs/client-sql-command.md). CREATE TABLE and ALTER TABLE CHECK accept `=`, `!=`, `<>`, `<`, `<=`, `>`, and `>=`; operands bind compatible columns or typed scalar literals. Integer, exact DECIMAL, boolean, string/VARCHAR/UUID, BYTEA, epoch DATE/TIMESTAMPTZ, and NULL literals are contextually validated. DECIMAL literal coefficients use the compared column scale.
- Regression: parser and binder tests cover DECIMAL coefficient scaling, VARCHAR and Boolean typing, type mismatch/loss rejection, ALTER metadata binding/dependency IDs, CREATE TABLE EIR1 decoding, CLI recognition, expression validation, and atomic storage publication.
- Limit: CHECK remains deterministic and row-local; named constraints and FOREIGN KEY SQL are deferred.
- Validation: focused parser, EIR binding, authenticated AdminClient, CLI, and storage regressions passed. Locked workspace build, format check, warning-denied all-target Clippy, complete serial delegated-cgroup workspace suite (sandbox/process tests enabled; zero ignored), and `git diff --check` passed.


## DS-11: ALTER TABLE FOREIGN KEY SQL lifecycle — implemented

- Contract: [schema DDL](../../specs/ddl-schema.md), [client SDK](../../specs/client-sdk.md), and [client SQL command](../../specs/client-sql-command.md). ADD resolves source/target relations and columns through authenticated metadata and requires the ordered target columns to match a visible PRIMARY KEY or UNIQUE constraint before dispatching typed FK creation. DROP CONSTRAINT accepts the generated foreign-key ID and verifies it belongs to the selected table before typed removal.
- Regression: parser coverage checks qualified composite forms, equal arity, duplicate-name rejection, single-statement syntax, and generated-ID parsing. Authenticated TCP tests verify metadata binding, target constraint selection, create/drop operations, and returned IDs; CLI recognition is covered before credentials/network.
- Follow-up: atomic CREATE TABLE FK definitions are implemented in DS-12 below. User-defined constraint names remain deferred.
- Validation: focused parser, authenticated AdminClient, and CLI regressions passed. Locked workspace build, format check, warning-denied all-target Clippy, complete serial delegated-cgroup workspace suite (sandbox/process tests enabled; zero ignored), and `git diff --check` passed.


## DS-12: Atomic FOREIGN KEY definitions in CREATE TABLE SQL — implemented

- Contract: [schema DDL](../../specs/ddl-schema.md), [client SDK](../../specs/client-sdk.md), and [client SQL command](../../specs/client-sql-command.md). CREATE TABLE accepts table-level FOREIGN KEY clauses referencing an existing schema-qualified relation. The authenticated client binds ordered referenced columns to a visible PRIMARY KEY or UNIQUE constraint; storage independently validates column names, arity, exact scalar types, target key shape, and REFERENCES authority.
- Implementation: the typed create operation carries FK source-column names and target constraint IDs. Storage publishes the new table, FK constraint/object/dependency records, target table write-epoch changes, audit, and durable outcome atomically. Commit validation guards target catalog records and checks authority.
- Regression: parser tests cover multiple FK clauses and invalid arity, duplicate/unknown columns, qualification, and multiple statements. Authenticated TCP coverage checks metadata binding and typed arguments; storage coverage verifies atomic catalog/dependency/epoch publication and startup validation. No stale tests were found; existing negative syntax tests remain relevant.
- Validation: focused parser, authenticated AdminClient, and storage regressions passed. Locked workspace build, formatting, warning-denied all-target Clippy, TinyCC SDK tests, sandbox fixture build, and the full serial delegated-cgroup workspace suite passed (all sandbox/process tests enabled, zero ignored); `git diff --check` passed.
- Next: continue the adjacent identity DDL SQL surface with rename, RESTRICT drop, and role membership forms.


## DS-13: ALTER COLUMN SET/DROP DEFAULT SQL — implemented

- Contract: [schema DDL](../../specs/ddl-schema.md), [client SDK](../../specs/client-sdk.md), and [client SQL command](../../specs/client-sql-command.md). Qualified `SET DEFAULT`/`DROP DEFAULT` resolves the declared column type and nullability through authenticated metadata, binds one supported typed literal, and uses the existing typed operations; existing rows remain unchanged.
- Red evidence: the new parser regression failed to compile because the bounded ALTER DEFAULT parser did not exist.
- Implementation: parser and binder support typed defaults, including exact DECIMAL scaling, and the authenticated client/CLI route to existing storage operations.
- Verification: parser/binder, authenticated AdminClient TCP, and CLI pre-credential tests passed. Locked workspace build, formatting, warning-denied all-target Clippy, TinyCC SDK tests and sandbox fixture, and the complete serial delegated-cgroup workspace suite passed with zero ignored tests; `git diff --check` passed.


## DS-14: DROP CONSTRAINT SQL lifecycle — implemented

- Contract: [schema DDL](../../specs/ddl-schema.md), [client SDK](../../specs/client-sdk.md), and [client SQL command](../../specs/client-sql-command.md). `DROP CONSTRAINT 'generated-id'` must first verify the generated ID belongs to the selected table, then dispatch the corresponding typed drop operation for PRIMARY KEY, UNIQUE, CHECK, or FOREIGN KEY.
- Red evidence: the new authenticated AdminClient regression for dropping a table's PRIMARY KEY failed because SQL supported only FOREIGN KEY kinds.
- Implementation: metadata kind selection now dispatches to the existing typed drop API for each droppable constraint kind and rejects NOT NULL or IDs absent from the selected relation.
- Regression: authenticated TCP coverage verifies the generated ID is matched to the selected table’s PK before `constraints.drop_primary_key` is sent; existing FK-drop coverage remains green, and CLI syntax validation passes before credential/network access.
- Verification: focused client/parser/CLI tests, locked workspace build, formatting, warning-denied all-target Clippy, TinyCC SDK tests/fixture, and full serial delegated-cgroup workspace suite passed; all sandbox/process tests ran with zero ignored. `git diff --check` passed.

## DS-15: CREATE INDEX SQL — implemented

- Contract: [schema DDL](../../specs/ddl-schema.md), [client SDK](../../specs/client-sdk.md), and [client SQL command](../../specs/client-sql-command.md). Bounded `CREATE [UNIQUE] INDEX name ON schema.table [USING BTREE] (column, ...)` binds schema, relation, and ordered column names through authenticated metadata and dispatches the existing typed index create operation.
- Regression: parser tests accept supported forms and reject unsupported methods, duplicate/empty columns, unqualified relations, and multiple statements. Authenticated TCP coverage verifies the name, table ID, ordered column IDs, and uniqueness flag. CLI coverage verifies syntax recognition before credentials/network. No stale tests were identified; existing malformed-input regressions remain applicable.
- Verification: focused parser, authenticated AdminClient, and CLI tests passed. Locked workspace build, format check, warning-denied all-target Clippy, maintained TinyCC SDK tests, sandbox-query fixture build, complete serial delegated-cgroup workspace suite (sandbox/process tests enabled; zero ignored), and `git diff --check` passed.
- Limit: DROP INDEX SQL remains unsupported until index names can be resolved through authenticated metadata.


## DS-16: DROP INDEX SQL and name-bearing administrative metadata — implemented

- Contract: [schema DDL](../../specs/ddl-schema.md), [client SDK](../../specs/client-sdk.md), and [client SQL command](../../specs/client-sql-command.md). `DROP INDEX schema.name RESTRICT` resolves the schema and enumerates its visible relations and logical indexes through authenticated metadata, then submits the resolved generated ID to typed index drop.
- Implementation: administrative index metadata now includes the logical name while continuing to omit physical locators and memberships. The client searches all paginated visible relations/indexes, rejects absent or ambiguous matches, and invokes the existing typed operation.
- Regression: parser cases cover qualification, mandatory RESTRICT, and single-statement bounds. Authenticated TCP tests verify name-bearing metadata decoding and name-to-ID drop dispatch; CLI coverage verifies syntax recognition before credentials/network. Existing malformed-syntax and storage authorization tests remain relevant; no stale tests were identified.
- Validation: focused parser, metadata client, AdminClient drop, CLI, and process-level metadata lifecycle tests passed. Locked workspace build, format check, warning-denied all-target Clippy, maintained TinyCC SDK tests, sandbox-query fixture build, and the complete serial delegated-cgroup workspace suite (sandbox/process tests enabled; zero ignored) passed; `git diff --check` passed.

## DS-SQL-17: schema and table owner transfer SQL — implemented

- Added bounded `ALTER SCHEMA ... OWNER TO USER|ROLE ...` and `ALTER TABLE schema.table OWNER TO USER|ROLE ...` parsing and dispatch through authenticated object/identity resolution to existing typed owner-transfer operations. The delegated metadata lifecycle exercises both SQL forms and restores its fixture ownership after each change.
- Verification: focused parser tests and the delegated gateway/supervisor metadata lifecycle passed. Locked workspace build, format check, warning-denied all-target Clippy, maintained TinyCC SDK tests/fixture, and full serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored; `git diff --check` passed. Cargo.lock is unchanged.

## DS-SQL-18: named index DDL SQL — verified

- `CREATE INDEX`, `REBUILD INDEX schema.name`, and `DROP INDEX schema.name RESTRICT` use authenticated metadata to resolve the schema, relation, ordered columns, or one unambiguous logical index. Rebuild sends the existing ordered column IDs and uniqueness profile to `indexes.rebuild`; storage validates and atomically republishes memberships under its DDL authority/fencing rules.
- Regression: the parser rejects unqualified, extra-clause, and multi-statement forms. The gateway/supervisor metadata lifecycle exercises named create, unchanged rebuild, definition-changing rebuild, and drop through `admin-sql`; authenticated metadata confirms the changed ordered columns and uniqueness flag.
- Verification: focused parser and delegated process tests passed, including a focused rerun after the catalog assertions were added. Locked offline workspace build, format check, warning-denied all-target Clippy, and the complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests. `git diff --check` passed; Cargo.lock is unchanged.

## DS-SQL-19: ALTER INDEX definition rebuild SQL — verified

- Added bounded `ALTER INDEX schema.name REBUILD [USING BTREE] {UNIQUE|NONUNIQUE} (columns...)`. The client resolves the existing index and owning table through authenticated metadata, binds the requested ordered columns, and sends one `indexes.rebuild` operation; storage remains responsible for authority, uniqueness, fencing, and atomic membership publication.
- Red/green: the parser regression initially failed to compile because the syntax was absent, then passed after implementation. The delegated gateway/supervisor metadata lifecycle successfully changes an index's column definition through `admin-sql`.
- Verification: focused parser and delegated process tests passed. Locked offline workspace build, format check, warning-denied all-target Clippy, and full serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests (269 library tests, 174 commit tests, and 75 transaction tests). `git diff --check` passed; Cargo.lock is unchanged.

## DS-SQL-20: resolve CREATE SCHEMA owner names — implemented

- `CREATE SCHEMA name AUTHORIZATION USER owner_name` and `... ROLE owner_name` now resolve the explicitly typed identity through authenticated metadata before invoking `schemas.create`. The existing hexadecimal UUID form remains supported.
- Regression evidence: parser coverage checks both identity kinds and rejects an untyped name. Authenticated AdminClient coverage verifies user-name resolution and exact owner ID dispatch. The separate gateway/supervisor process lifecycle runs `admin-sql` create/drop with a renamed user as owner.
- Verification: focused parser, authenticated AdminClient, and production process tests passed. Locked workspace build, format check, warning-denied all-target Clippy, maintained TinyCC SDK tests and sandbox fixture passed. The cgroup probe reported writable `cpu memory pids`; the full serial delegated-cgroup workspace suite passed with sandbox/process coverage enabled and zero ignored tests (269 library, 174 commit, 75 transaction tests; all other targets passed). `git diff --check` passed; Cargo.lock is unchanged.

## DS-SQL-21: default CREATE SCHEMA ownership — verified

- `CREATE SCHEMA name` omits the wire owner field; the authenticated control server resolves it to the current actor when committing. Explicit UUID, USER, and ROLE owners remain supported.
- Parser, protocol decoder, and authenticated client regressions cover omitted owner. The gateway/supervisor process lifecycle executes the default-owner `admin-sql` form and drops the created schema.
- Verification: focused parser, protocol, client, and production process tests passed. Locked workspace build, format check, warning-denied all-target Clippy, TinyCC suite/fixture, and full serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests (269 library tests, 174 commit tests, 75 transaction tests; all other targets passed). `git diff --check` passed; Cargo.lock is unchanged.

## DS-SQL-22: resolve untyped schema owner names — verified

- `CREATE SCHEMA ... AUTHORIZATION name` and `ALTER SCHEMA ... OWNER TO name` now accept the identity syntax shown in the schema DDL examples. A non-enumerating `metadata.identities.resolve` lookup can resolve either kind and reveals it only when the caller has the corresponding identity-management authority. Typed USER/ROLE forms remain available.
- Parser, protocol decoder, authenticated client, and production gateway/supervisor process regressions cover untyped CREATE and ALTER schema owner resolution.
- Verification: focused parser/protocol/client/process tests passed. Locked workspace build, format check, warning-denied all-target Clippy, maintained TinyCC SDK tests and sandbox fixture, plus the complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests (269 library tests, 174 commit tests, 75 transaction tests; all other targets passed). The cgroup probe reported writable `cpu memory pids`; `git diff --check` passed and Cargo.lock is unchanged.

## DS-SQL-23: column-level UNIQUE in CREATE TABLE — verified

- `CREATE TABLE` now accepts a column `UNIQUE` modifier and lowers it to the same one-column unique-constraint list as table-level `UNIQUE (column)`. Duplicate inline/table definitions reject; nullable columns retain the existing NULLS DISTINCT behavior.
- Red/green: the parser regression failed with `InvalidStatement` before the change and passes afterward. Authenticated AdminClient request coverage confirms the inline form emits the same `unique_constraints` payload; CLI coverage accepts the form before credentials/network access.
- Verification: focused parser, authenticated AdminClient, and CLI tests passed. Locked workspace build, format check, warning-denied all-target Clippy, maintained TinyCC SDK tests and sandbox fixture, and the full serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests (270 library tests, 174 commit tests, 75 transaction tests; all other targets passed). The cgroup probe reported writable `cpu memory pids`; `git diff --check` passed and Cargo.lock is unchanged.

## DS-SQL-24: column-level PRIMARY KEY in CREATE TABLE — verified

- `CREATE TABLE` accepts a column-level `PRIMARY KEY`, lowers it to the existing one-column primary-key list, and marks the column NOT NULL. Multiple primary-key declarations reject.
- Red/green: the parser regression failed with `InvalidStatement` before implementation and passes after the parser change. Authenticated AdminClient and CLI coverage exercise inline PRIMARY KEY. The full run identified two obsolete parser rejection assertions for the newly supported form; those stale expectations were removed while malformed cases remain covered.
- Verification: all 38 DDL parser tests, focused authenticated AdminClient and CLI tests, locked workspace build, format check, warning-denied all-target Clippy, TinyCC SDK tests and sandbox fixture, and the full serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests (271 library tests, 174 commit tests, 75 transaction tests; all other targets passed). `git diff --check` passed and Cargo.lock is unchanged.

## DS-SQL-25: column-level CHECK in CREATE TABLE — verified

- `CREATE TABLE` accepts the existing bounded CHECK expression after a column definition when every column reference names that same column. The expression is lowered to the existing check-constraint list, so binding, catalog publication, and row enforcement reuse the table-level path.
- Red/green parser coverage first failed with `InvalidStatement`, then passed after implementation. Parser tests retain rejection for cross-column inline checks; authenticated AdminClient request and CLI recognition coverage use the inline form.
- Verification: all 39 DDL parser tests and focused AdminClient/CLI tests passed. Locked workspace build, format check, warning-denied all-target Clippy, TinyCC SDK tests and sandbox fixture, and the full serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests (272 library tests, 174 commit tests, 75 transaction tests; all other targets passed). The cgroup probe reported writable `cpu memory pids`; `git diff --check` passed and Cargo.lock is unchanged.

## DS-SQL-26: column-level FOREIGN KEY in CREATE TABLE — verified

- Column-level `REFERENCES schema.table (column)` now lowers to the existing one-column foreign-key definition; target key binding, REFERENCES authorization, and atomic table/FK publication remain in the existing client and storage path.
- The parser regression failed with `InvalidStatement` before the change and passes afterward, including malformed and multi-column reference rejection. Authenticated AdminClient and CLI integration coverage passed, including authenticated target-key resolution and typed payload validation.
- Verification: 40 DDL parser tests and focused AdminClient/CLI tests passed. Locked workspace build, format check, warning-denied all-target Clippy, TinyCC SDK tests and sandbox fixture, and the full serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests (273 library, 174 commit, 75 transaction tests; all other targets passed). The cgroup probe reported writable `cpu memory pids`; `git diff --check` passed and Cargo.lock is unchanged.

### DS-SQL-26 production process follow-up — verified

- The gateway/supervisor DDL lifecycle creates one referencing table with column-level `REFERENCES`, then reads back its FK source column, target table, and referenced column through authenticated metadata. It also exercises `ALTER TABLE ... ADD FOREIGN KEY` and generated-ID `DROP CONSTRAINT` through `admin-sql`, with authenticated metadata readback between operations.
- The first run exposed the expected dependency guard when teardown tried to drop the referenced UNIQUE constraint before the new FK table. Teardown now drops the dependent table first.
- Verification: `authenticated_metadata_and_admin_lifecycle_cross_gateway_and_supervisor_processes` passed inside the delegated cgroup service with `cpu memory pids` enabled after both inline and ALTER foreign-key SQL paths were added (1 passed, 0 failed). The initial column-level FK implementation's full validation is recorded above; this follow-up changes process integration coverage and progress evidence.

## DS-8 ALTER TABLE ADD COLUMN SQL production lifecycle — verified

- Contract: a qualified SQL column addition with a typed non-null default must pass through authenticated `admin-sql`, commit via the gateway/supervisor path, and appear in the resulting table metadata.
- Regression: the cross-process administrative lifecycle now adds `label VARCHAR(30) NOT NULL DEFAULT 'unknown'` to a real table using `admin-sql`, then verifies through authenticated metadata that the column is at ordinal 1, has the declared VARCHAR bound, is non-nullable, and is included in the completed column page.
- Verification: the focused delegated-cgroup gateway/supervisor process test passed (1 passed, 0 failed). Locked workspace build, format check, warning-denied all-target Clippy, maintained TinyCC SDK tests and sandbox fixture, release-profile validation (10 binaries and one query fixture), and the full serial delegated-cgroup workspace suite passed with zero ignored tests. `git diff --check` passed; Cargo.lock is unchanged.

## DS-SQL-28: authorized `DROP TABLE IF EXISTS` — verified

- Contract: [schema DDL](../../specs/ddl-schema.md) and [admin SQL command](../../specs/client-sql-command.md) define conditional table drop. Present tables follow table-owner/database-admin authorization and RESTRICT dependency checks; an unresolved target is a no-op only after database-admin authority is confirmed. The no-op creates no DDL outcome.
- Red/green: the parser regression first failed to compile because there was no conditional-drop representation. The updated parser now covers qualified regular and conditional forms and rejects malformed conditionals. Production gateway/supervisor process coverage exercises administrator no-op and non-administrator denial.
- Implementation: [`DROP TABLE IF EXISTS` parsing and dispatch](../../engine/src/ddl_sql.rs), authenticated control flow in [`client_submit.rs`](../../engine/src/client_submit.rs), CLI output in [`prdb-client.rs`](../../engine/src/bin/prdb-client.rs), with process coverage in [`metadata_process.rs`](../../engine/tests/metadata_process.rs).
- Verification: parser regression passed; `self_referencing_create_table_sql_crosses_gateway_and_supervisor_processes` passed inside the delegated user service (1 passed, 0 failed), exercising administrator no-op and non-administrator denial. Workspace build, format check, warning-denied all-target Clippy, TinyCC SDK tests, sandbox fixture, writable `cpu memory pids` cgroup probe, and the complete serial delegated-cgroup workspace suite passed with all tests enabled (278 library, 174 commit, 3 metadata-process, 75 transaction tests; all other targets passed). `git diff --check` passed and Cargo.lock is unchanged.

## DS-SQL-29: authorized `DROP SCHEMA IF EXISTS` — verified

- Contract: [schema DDL](../../specs/ddl-schema.md) and [admin SQL command](../../specs/client-sql-command.md) allow an absent-schema no-op only after checking authority. Present schemas retain normal owner/database-admin authorization and RESTRICT emptiness checks; no-op creates no DDL outcome.
- Red/green: the parser regression first failed to compile because there was no conditional schema-drop representation. Coverage now checks regular and conditional qualified schema forms and rejects malformed conditionals. The process regression covers administrator no-op and non-administrator denial.
- Implementation: parser, authenticated `AdminClient` helper, `admin-sql` dispatch, and gateway/supervisor regression. The parser regression passed; the focused delegated gateway/supervisor process test passed (1 passed, 0 failed), covering administrator no-op and non-administrator denial. Workspace build, format, warning-denied Clippy, and full serial delegated-cgroup workspace suite passed with all tests enabled (278 library, 174 commit, 3 metadata-process, 75 transaction tests; all other targets passed). `git diff --check` passed; Cargo.lock is unchanged.

## DS-SQL-30: conditional drops preserve present-object RESTRICT failures — verified

- Contract: `DROP TABLE IF EXISTS` and `DROP SCHEMA IF EXISTS` suppress only unresolved-target errors after authority is checked. A present but restricted object must still fail and remain intact.
- Regression: the gateway/supervisor lifecycle now successfully drops a present table with `IF EXISTS`, rejects `DROP TABLE IF EXISTS` for a parent referenced by a child FK, and rejects `DROP SCHEMA IF EXISTS` for a non-empty schema. Existing rows/relations remain checked after the failed table drop.
- Verification: the focused delegated process test passed (1 passed, 0 failed). Workspace build, format, warning-denied Clippy, writable `cpu memory pids` cgroup probe, and full serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests (278 library, 174 commit, 3 metadata-process, 75 transaction tests; all other targets passed). `git diff --check` passed; Cargo.lock is unchanged.

## DS-SQL-31: authorized `DROP INDEX IF EXISTS` — implemented slice

- Extended bounded index-drop SQL parsing and `admin-sql` dispatch with `DROP INDEX IF EXISTS schema.name RESTRICT`. Present indexes retain typed authorization and dependency checks; unresolved names are a no-op only after the authenticated server confirms database-administrator authority. Ambiguous index-name resolution remains an error.
- Updated the schema DDL and SQL command contracts. Workspace build, formatting, warning-denied all-target Clippy, and `git diff --check` passed; `Cargo.lock` is unchanged. Focused parser and delegated process regressions remain to be added; no test suite was run for this increment.

## DS-SQL-32: index-authorized SQL target metadata — implemented slice

- Administrative schema/relation/column/index discovery now recognizes table ownership and inherited table-scoped `INDEX` grants. `admin-sql` can bind CREATE/DROP/REBUILD INDEX names and columns without granting row access or requiring schema USAGE. Metadata discovery also returns the relevant schema name for index-authorized tables.
- Corrected the privilege/scope key matching in the scoped object-DDL metadata authority helper. Workspace build, formatting, warning-denied all-target Clippy, and `git diff --check` passed; `Cargo.lock` is unchanged. Focused protocol, authorization, and gateway/supervisor regressions remain to be added; no test suite was run for this increment.

## DS-SQL-33: conditional column drops — implemented slice

- Added `ALTER TABLE schema.table DROP COLUMN IF EXISTS column RESTRICT`. A resolved column follows the existing typed drop path and its dependency, rewrite-budget, and authorization checks. An unresolved schema/table/column is a no-op only after database-administrator authority is confirmed; no DDL outcome is created.
- Updated the schema DDL and client SQL contracts. Workspace build, formatting, warning-denied Clippy, and `git diff --check` pass. No tests were added or run; Cargo.lock is unchanged.

## DS-SQL-34: conditional constraint drops — implemented slice

- Added `ALTER TABLE schema.table DROP CONSTRAINT IF EXISTS 'generated-id'`. Resolved IDs retain generated-ID table binding and dispatch through the typed PRIMARY KEY, UNIQUE, CHECK, or FOREIGN KEY drop path; unresolved IDs require database-administrator authority for a no-op.
- Updated the schema DDL contract. The delegated gateway/supervisor lifecycle now drops a present FK with `IF EXISTS`, repeats the same statement to verify the administrator no-op, and confirms an ALTER-only grantee cannot suppress a missing ID. The focused process regression, formatting, warning-denied all-target Clippy, and `git diff --check` pass; Cargo.lock is unchanged.

## DS-SQL-35: ALTER-authorized constraint binding — implemented slice

- Constraint metadata binding for SQL DDL now recognizes table ownership and inherited table-scoped `ALTER`, matching the typed constraint mutation authority. These principals can resolve the target schema/table and declared columns without data grants or schema USAGE. CHECK bodies remain omitted and hidden FK target details stay redacted.
- Updated metadata-discovery, SDK, and DDL contracts. The focused delegated gateway/supervisor lifecycle verifies table-owner constraint listing and owner/direct-ALTER-grantee `ADD COLUMN` SQL binding without schema USAGE. The focused process regression, workspace build, formatting, warning-denied all-target Clippy, and `git diff --check` pass. Cargo.lock is unchanged.

## DS-SQL-36: schema CREATE-grantee SQL binding — implemented slice

- Schema-scoped `CREATE` grantees can resolve a schema name for `CREATE TABLE` through authenticated metadata, without exposing its existing relations or columns. Table creation still uses the typed operation and storage authorization.
- Updated metadata-discovery and client SQL contracts. The focused delegated gateway/supervisor lifecycle verifies a schema-scoped `CREATE` grantee can run `CREATE TABLE` without schema `USAGE`; typed storage remains authoritative. The focused process regression, formatting, workspace build, warning-denied all-target Clippy, and `git diff --check` pass; Cargo.lock is unchanged.

## DS-SQL-37: schema ALTER grants — verified

- Effective schema-scoped `ALTER` now authorizes schema rename, ownership transfer, and empty-schema drop through the typed commit guard, with applicable grant and membership records guarded through publication. Schema-scoped ALTER principals can bind schema names for SQL DDL without gaining relation or column discovery.
- Updated the schema DDL, permission, and metadata-discovery contracts. The focused gateway/supervisor process lifecycle issues `GRANT ALTER ON SCHEMA` and `REVOKE ALTER ON SCHEMA`, then verifies that the non-owner grantee without schema `USAGE` can rename through `admin-sql`; rename-back and revocation restore the fixture. Build, format check, warning-denied Clippy, and the focused delegated process test pass. Cargo.lock is unchanged.

## DS-SQL-38: conditional index drop process coverage — verified

- The production gateway/supervisor lifecycle now drops a present logical index through `DROP INDEX IF EXISTS ... RESTRICT`, then repeats the statement and verifies the database-administrator no-op reports that no operation committed. The same fixture still confirms RESTRICT blocks dropping a column while its index exists.
- The focused delegated process test and `cargo fmt --all` pass; Cargo.lock is unchanged.

## DS-6: table-scoped ALTER for rename, owner transfer, and drop — verified slice

- Typed table rename, owner transfer, and drop guards now accept effective table- or schema-scoped `ALTER` grants in addition to ownership/database administration. Direct and inherited grant records are retained as commit guards so revocation cannot authorize stale DDL. Column rename uses the same scoped authority.
- Updated table rename/drop/owner-transfer contracts. The production gateway/supervisor test grants table ALTER, renames the target through `admin-sql` without schema USAGE, exercises owner transfer through the typed ADMIN operation, restores ownership, and drops a separate table through `admin-sql` as an ALTER grantee. The focused delegated process test and formatting pass. Cargo.lock is unchanged.

## DS-SQL-39: CREATE SCHEMA IF NOT EXISTS — verified extension

- Extended bounded schema SQL with `CREATE SCHEMA IF NOT EXISTS`. An absent schema follows the ordinary typed `schemas.create` authorization and publication path. If the schema already resolves, only a database administrator receives a no-op; it creates no DDL outcome and does not resolve or apply an optional owner clause.
- Updated schema DDL, client SDK, and client SQL contracts. Parser coverage and the production gateway/supervisor lifecycle verify create, administrator repeat no-op, and non-administrator denial. The targeted parser and delegated process tests, workspace build, format check, warning-denied all-target Clippy, and `git diff --check` pass; Cargo.lock is unchanged.

## DS-SQL-40: CREATE TABLE IF NOT EXISTS — verified extension

- Added `CREATE TABLE IF NOT EXISTS` to the bounded authenticated SQL parser and CLI. Absent relations use the existing typed create path; an existing relation is a database-administrator-only no-op with no DDL outcome, and its definition is not compared or reconciled.
- Updated schema DDL, client SDK, and client SQL contracts. Parser and gateway/supervisor process coverage verifies table creation, administrator repeat no-op, and rejection of the no-op for a non-administrator even when that actor has table-scoped ALTER. Targeted parser and delegated process tests pass; Cargo.lock is unchanged.

## DS-SQL-41: CREATE INDEX IF NOT EXISTS — verified extension

- Added `CREATE [UNIQUE] INDEX IF NOT EXISTS` to bounded SQL. An absent index uses the regular typed build; a resolved existing logical index no-ops only for a database administrator, with no DDL outcome and no definition reconciliation.
- Updated schema DDL, client SDK, and client SQL contracts. Parser and production gateway/supervisor lifecycle tests cover conditional creation and administrator repeat no-op. Targeted tests pass; Cargo.lock is unchanged.

## DS-SQL-42: conditional column addition — verified

- Added `ALTER TABLE ... ADD COLUMN IF NOT EXISTS`. An absent column uses the existing typed add-column operation; a resolved existing column is a database-administrator-only no-op and does not compare or reconcile the requested definition.
- Updated the schema DDL, client SDK, and client SQL contracts. The parser unit test and delegated production gateway/supervisor process test pass. Workspace build, formatting, warning-denied all-target Clippy, and `git diff --check` pass; Cargo.lock is unchanged.

## DS-SQL-43: ALTER-authorized table owner-name binding — verified

- Added authenticated table-scoped USER/ROLE name resolution for `ALTER TABLE ... OWNER TO`. The lookup grants no identity listing and returns the same absent result for unknown names or actors without table owner/ALTER/database-admin authority; typed mutation rechecks authorization and destination state.
- The delegated gateway/supervisor lifecycle grants table ALTER, performs name-based SQL owner transfer as that grantee, revokes ALTER, then confirms the transferred owner retains catalog DDL visibility. The focused process test, workspace build, format check, warning-denied all-target Clippy, and `git diff --check` pass; Cargo.lock is unchanged.

## DS-SQL-44: ALTER-authorized schema owner-name binding — verified

- Generalized `metadata.alter_identities.resolve` to schema and table object IDs. Schema owners and effective schema ALTER grantees can bind SQL destination owner names without global identity-management authority; typed operations continue to revalidate the change.
- The delegated gateway/supervisor lifecycle grants schema ALTER and transfers schema ownership through `admin-sql` as that grantee. The process test, workspace build, format check, warning-denied all-target Clippy, and `git diff --check` pass; Cargo.lock is unchanged.

## DS-SQL-46: ALTER INDEX RENAME — verified extension

- Added schema-qualified `ALTER INDEX schema.name RENAME TO new_name` across bounded SQL parsing, authenticated metadata binding, the typed `indexes.rename` operation, and atomic family-2 catalog publication. Rename preserves the stable index ID and physical definition; the existing index DDL authorization path checks owner/INDEX/database-admin authority and schema name uniqueness.
- Storage coverage verifies old-name removal, new-name resolution, stable ID, audit/commit sequence, and successful rebuild after rename. Delegated gateway/supervisor SQL coverage verifies metadata readback and collision rejection without losing the renamed index. Workspace build, formatting, warning-denied Clippy, and full serial delegated-cgroup workspace suite pass with sandbox/process tests enabled and zero ignored; `git diff --check` passes and Cargo.lock is unchanged.

## DS-9: lossless VARCHAR widening through SQL — implemented slice verified

- Contract: `ALTER TABLE schema.table ALTER COLUMN column TYPE VARCHAR(n)` accepts only an existing `VARCHAR(m)` where `n > m`. It preserves the stable column ID, tuple schema version, stored row bytes, dependent definitions, and index key bytes; it advances the table write epoch so stale writers abort. Shrinking, same-width, cross-type conversion, and `USING` are rejected.
- Added the bounded parser, authenticated `admin-sql` name binding, `tables.widen_varchar_column` control operation, and storage/catalog publication through the existing authorized constraint DDL commit path. The commit validator permits only a strict VARCHAR length increase with unchanged type ID and all other column metadata preserved.
- Focused evidence: parser unit test passes; the delegated storage regression verifies old-snapshot metadata, unchanged tuple bytes/schema version/index schema version, widened-value decoding, stale-writer conflict, no-op rejection, and unauthorized actor rejection. The delegated gateway/supervisor process lifecycle verifies SQL execution, authenticated metadata readback, stable column ID, and index availability after widening.
- Validation: locked workspace build, formatting check, warning-denied all-target Clippy, focused storage and process tests, and the complete serial delegated-cgroup workspace suite all pass. The full suite reported 281 library tests and 175 commit tests passed with sandbox/process coverage enabled and zero ignored. Cargo.lock is unchanged.
- Remaining: broader lockstep DDL acceptance for concurrent FK/policy interactions remains open.

## DS-SQL-47: SET/DROP NOT NULL SQL across gateway and supervisor — verified

- Added authenticated `admin-sql` integration coverage for `ALTER TABLE ... ALTER COLUMN ... SET NOT NULL` and `DROP NOT NULL` through separate production gateway and supervisor processes. The test reads the column metadata after each operation and confirms the nullable flag changes while preserving the existing table lifecycle.
- Updated the schema DDL contract to state that SQL name binding uses authenticated metadata and the typed operation remains authoritative for authorization and fenced row validation.
- Verification: `authenticated_metadata_and_admin_lifecycle_cross_gateway_and_supervisor_processes` passed inside the delegated `systemd --user` service. The path exercises both transitions over the external TCP control protocol; storage regressions separately cover NULL rejection, stale-writer conflict, and internal constraint cleanup.

## DS-SQL-48: bounded TRUNCATE TABLE RESTRICT — verified slice

- Added `TRUNCATE TABLE schema.table RESTRICT` to the bounded SQL parser and authenticated `admin-sql` path. It resolves the target through authenticated metadata and submits `tables.truncate` over the existing PREPARE/START protocol.
- Storage holds the exclusive table DDL fence across its snapshot scan, rejects any incoming foreign key even when the referencing table is empty, validates physical index entries against reverse mappings, then atomically removes rows and index memberships while preserving definitions. It advances the table write epoch and table object version. The operation is limited to 65,536 rows, 65,536 index entries, and 64 MiB of row-record plus index-key/locator bytes.
- Real-redb coverage verifies authorization denial and durable ABORTED outcome, retained old snapshots, row/index/reverse-index removal, index-definition retention, stale-writer conflict, and persisted-record validation. A separate regression verifies empty-table incoming-FK RESTRICT behavior.
- Verification: both focused storage regressions and the delegated gateway/supervisor process lifecycle pass. Locked workspace build, formatting check, warning-denied all-target Clippy, and the complete serial delegated-cgroup workspace suite pass with sandbox and process tests enabled. The full suite reported 281 library tests and 177 commit tests passed; no tests were ignored. `git diff --check` passes and Cargo.lock is unchanged.

## DS-SQL-48 / DD-5: TRUNCATE state survives database reopen — verified slice

- Extended `storage_manager_truncates_rows_and_index_memberships_atomically` to close the StorageManager and redb handle after a successful truncate, reopen the same database file, and rerun startup validation. The reopened image retains the committed outcome and table index definition while containing no truncated row, forward index entry, or reverse row-index membership.
- Verification: focused real-redb truncate regression, locked workspace build, format check, warning-denied all-target Clippy, `git diff --check`, and the complete serial delegated-cgroup workspace suite passed with sandbox and process tests enabled and zero ignored tests (293 unit, 190 commit, 76 transaction tests, plus all remaining targets). This confirms post-commit reopen consistency; it is not a process-crash or power-loss test.

## DS-SQL-49: named ALTER CHECK constraints — verified

- Added `ALTER TABLE ... ADD CONSTRAINT name CHECK (...)` and `DROP CONSTRAINT name` alongside the existing unnamed and generated-ID forms. Names are normalized and retained on the CHECK catalog object; stable IDs remain the mutation key.
- The authorized commit guard rejects duplicate names within a table only after validating DDL authority, records a durable ABORTED outcome on conflict, and serializes the check with the table's DDL epoch. A paired unauthorized-caller regression confirms duplicate-name status is not revealed and records a durable ABORTED outcome. Authenticated constraint metadata now includes catalog object names, allowing the client to resolve the name without broad catalog access.
- Verification: parser and protocol v1/v2 tests, named storage/duplicate-name regression, all 42 client-admin tests, and the production gateway/supervisor CLI lifecycle pass. Workspace build, format check, warning-denied all-target Clippy, and full serial delegated-cgroup suite pass (281 library and 177 commit tests; no ignored tests). Cargo.lock is unchanged.

## DS-SQL-50: named CHECK constraints in CREATE TABLE — verified

- Extended the bounded grammar with column- and table-level `CONSTRAINT name CHECK` forms. Names remain aligned with EIR1 expressions through the new `tables.create` v3 field and publish as CHECK catalog object names in the same atomic table-creation commit.
- Parser, authenticated AdminClient request, and real-redb catalog regressions pass. Production process coverage now includes a named CREATE TABLE CHECK alongside a self-referencing FK; the full serial delegated-cgroup workspace suite passes with sandbox and process tests enabled (282 library tests and 177 commit tests; no ignored tests).

## DS-SQL-51: named key and foreign-key constraints in CREATE TABLE — implemented

- Extended SQL parsing and the typed CREATE TABLE path with optional names for PRIMARY KEY, UNIQUE, and FOREIGN KEY constraints, including column-level shorthand. Duplicate names are rejected across all constraint kinds in a statement; persisted objects use explicit names and unnamed objects retain generated labels.
- `tables.create` v4 carries the primary-key name plus arrays aligned with UNIQUE and FK definitions; older versions remain valid. Existing constraint metadata and name-based DROP CONSTRAINT routing cover the persisted names.
- Focused parser and authenticated AdminClient serialization regressions pass, including a self-referencing FK and simultaneous PK/UNIQUE/CHECK/FK names. A real-redb regression verifies atomic persistence of named PK, UNIQUE, FK, and CHECK objects. Commit validation accepts only normalized explicit labels while retaining generated labels. The complete serial delegated-cgroup workspace suite passes (283 library tests, 177 commit tests, and all other targets; zero ignored). Workspace build, format check, warning-denied Clippy, and `git diff --check` pass; Cargo.lock is unchanged.


## DS-SQL-52: named ALTER key and foreign-key constraints — implemented

- `ALTER TABLE ... ADD CONSTRAINT name PRIMARY KEY|UNIQUE|FOREIGN KEY` now binds through authenticated metadata, persists names on constraint objects, and sends operation v2; v1 remains unchanged for unnamed requests. Duplicate names are checked in the authorized commit path, including ALTER and REFERENCES checks for FK creation.
- Parser, protocol v2 decoder, authenticated AdminClient request, and real-redb storage regressions pass. A production gateway/supervisor process test creates named PK/UNIQUE/FK constraints, reads their names through authenticated metadata, and drops each by name. The full serial delegated-cgroup workspace suite passes with 283 library tests and 177 commit tests, zero ignored; workspace build, format check, warning-denied Clippy, focused storage/client/protocol tests, and the named ALTER gateway/supervisor process lifecycle pass. `git diff --check` passes and Cargo.lock is unchanged.

## DS-SQL-53: finite-set CHECK expressions — implemented slice

- The bounded CHECK grammar now accepts column `IN` and `NOT IN` against one to 64 typed scalar literals in both CREATE TABLE and ALTER CHECK SQL. Binding resolves the column once, coerces each literal to its declared type, rejects mismatched types, and lowers the predicate to existing EIR1 equality/OR/NOT nodes so SQL UNKNOWN behavior remains defined by the existing evaluator. The 64-item bound keeps the lowered tree within the expression-node budget.
- Regression coverage checks typed VARCHAR and numeric binding, NULL literals, positive and negative forms, unsupported expression/list shapes, type mismatch, and 64/65 item boundaries.
- Verification: the focused parser/binder test passes. `cargo build --workspace --locked`, `cargo fmt --all -- --check`, warning-denied all-target Clippy, `git diff --check`, and the full serial delegated-cgroup workspace suite pass. The suite ran 284 library tests and 177 commit tests plus all other targets, with sandbox/process coverage enabled and zero ignored tests. Cargo.lock is unchanged.

## DS-SQL-54: inclusive range CHECK expressions — implemented slice

- CHECK SQL now accepts column `BETWEEN` and `NOT BETWEEN` with typed scalar literal bounds in CREATE TABLE and ALTER CHECK forms. Binding lowers each form to inclusive `>=` and `<=` comparisons joined by AND, with NOT applied to the conjunction for the negative form. The existing evaluator supplies SQL UNKNOWN behavior for NULL values and bounds.
- Regression coverage checks inclusive comparison binding, text and integer type binding, NOT BETWEEN, type mismatch, and malformed bound expressions. The focused parser/binder test passes. `cargo build --workspace --locked`, format check, warning-denied all-target Clippy, `git diff --check`, and the full serial delegated-cgroup workspace suite pass; the suite ran 285 library tests and 177 commit tests plus all other targets with sandbox/process coverage enabled and zero ignored tests. Cargo.lock is unchanged.

## DS-SQL-55: null-safe CHECK comparisons — implemented slice

- CHECK SQL now accepts `column IS DISTINCT FROM literal` and `column IS NOT DISTINCT FROM literal`, including NULL. Binding lowers NULL comparisons to `IS NULL` forms and non-NULL comparisons to `IS NULL OR <>`; the negative form negates that complete Boolean result, preserving two-valued null-safe semantics using existing EIR1 nodes.
- Regression coverage checks text and NULL binding and rejects missing or non-literal operands. The focused parser/binder test passes. Workspace build, format check, warning-denied all-target Clippy, `git diff --check`, and the full serial delegated-cgroup suite pass with sandbox/process coverage and zero ignored tests (286 library tests and 177 commit tests). Cargo.lock is unchanged.

## DS-SQL-56: Boolean truth tests in CHECK and policy SQL — verified

- Contract: boolean columns accept `IS [NOT] TRUE`, `FALSE`, and `UNKNOWN` in CREATE TABLE CHECK, ALTER CHECK, and policy expression SQL. Binding rejects non-BOOLEAN columns. SQL truth tests always return Boolean: TRUE/FALSE tests return false for NULL, UNKNOWN tests match NULL, and NOT forms negate those results.
- Red/green: the new parser regression failed before implementation on `active IS TRUE`; it now checks all six forms, exact EIR lowering, non-Boolean rejection, and malformed forms. Implementation uses existing EIR1 null checks, equality, AND, and NOT nodes without changing protocol or storage formats.
- Verification: the focused parser/binder regression and complete serial delegated-cgroup workspace suite pass (289 library tests, 177 commit tests, sandbox/process tests enabled, zero ignored). Workspace build, format check, warning-denied all-target Clippy, and `git diff --check` pass. Cargo.lock is unchanged.

## DS-SQL-57: Boolean truth tests on grouped expressions — verified

- Contract: `IS [NOT] TRUE`, `FALSE`, and `UNKNOWN` accepts a Boolean-valued expression, including parenthesized comparisons/conjunctions. Policy-context functions may appear only in policy SQL; non-Boolean operands reject.
- Red/green: added a regression for `(active = TRUE) IS TRUE` and `HAS_ROLE('operator') IS TRUE`; the first failed before implementation. The parser now attaches truth tests to the grouped expression, and binding validates Boolean result type before lowering to EIR1.
- Verification: parser/binder and authenticated AdminClient EIR-serialization regressions pass. The full serial delegated-cgroup workspace suite passes (289 library tests, 177 commit tests, sandbox/process tests enabled, zero ignored). Workspace build, format check, warning-denied all-target Clippy, and `git diff --check` pass. Cargo.lock is unchanged.

## DS-SQL-58: `IS [NOT] NULL` on supported expressions — verified

- The CREATE TABLE CHECK and ALTER CHECK binder now accepts null tests over any supported scalar expression, including parenthesized comparisons. Binding recursively resolves columns and types, then lowers to the existing EIR1 `IS NULL` node and optional `NOT`; no wire or storage format changed. Column-level CHECK still restricts references to its declared column.
- Regression coverage checks parser/binder EIR for `(active = TRUE) IS [NOT] NULL` and authenticated AdminClient EIR serialization for a null test on an integer comparison. The full serial delegated-cgroup workspace suite passes (289 library tests, 177 commit tests, sandbox/process tests enabled, zero ignored); workspace build, format check, warning-denied all-target Clippy, and `git diff --check` pass. Cargo.lock is unchanged.

## DS-SQL-59: null-safe distinctness on supported expressions — verified

- `IS [NOT] DISTINCT FROM` now accepts a supported scalar expression on the left and one context-typed scalar literal on the right. Binding retains two-valued NULL-safe semantics by lowering to the existing EIR1 null checks, comparisons, and Boolean nodes. Column-level CHECK references remain limited to the declared column, and policy context restrictions still apply recursively.
- Regression coverage checks `(code IS NULL) IS DISTINCT FROM TRUE` and existing NULL/non-NULL literal forms. The full serial delegated-cgroup workspace suite passes (289 library tests, 177 commit tests, sandbox/process tests enabled, zero ignored); workspace build, format check, warning-denied all-target Clippy, and `git diff --check` pass. Cargo.lock is unchanged.

## DS-SQL-60: finite-set and range predicates on supported expressions — verified

- CHECK SQL now accepts any supported scalar expression before `IN`/`NOT IN` and `BETWEEN`/`NOT BETWEEN`, with context-typed literal lists/bounds. Binding recursively preserves column dependencies and policy-context restrictions, lowers through existing EIR1 nodes, and validates the final canonical encoding so operand replication cannot exceed node, depth, or byte limits.
- Regression coverage includes grouped Boolean expressions, literal-only IN, the 64-item boundary, and rejection when a complex repeated operand exceeds the EIR1 node budget. The full serial delegated-cgroup workspace suite passes (289 library tests, 177 commit tests, sandbox/process tests enabled, zero ignored); workspace build, format check, warning-denied all-target Clippy, and `git diff --check` pass. Cargo.lock is unchanged.

## DS-SQL-61: null-safe distinctness on both expressions — verified

- `IS [NOT] DISTINCT FROM` now binds supported scalar expressions on both sides with contextual type checking. The lowering explicitly handles null/null, null/value, value/null, and value/value cases using existing EIR1 nodes, producing two-valued results; the completed expression is checked against canonical EIR1 bounds. Column-level CHECK and policy-context validation recurse through both operands.
- Regression coverage includes expression/expression binding, mismatched types, literal compatibility, and encoding. The full serial delegated-cgroup workspace suite passes (289 library tests, 177 commit tests, sandbox/process tests enabled, zero ignored); workspace build, format check, warning-denied all-target Clippy, and `git diff --check` pass. Cargo.lock is unchanged.

## DS-SQL-62: finite-set and range operands accept expressions — verified

- `IN` lists now accept up to 64 supported scalar expressions, and `BETWEEN` accepts supported scalar expressions for both bounds. Binding contextually checks each operand against the tested expression's type, retains SQL UNKNOWN behavior, and validates the complete lowered tree against canonical EIR1 limits. Existing scalar literal forms remain supported.
- Regression coverage checks column references in IN lists and range bounds, grouped Boolean operands, literal lists, type mismatches, the 64/65-item boundary, and a repeated-expression EIR overflow. The full serial delegated-cgroup workspace suite passes (289 library tests, 177 commit tests, sandbox/process tests enabled, zero ignored); workspace build, format check, warning-denied all-target Clippy, and `git diff --check` pass. Cargo.lock is unchanged.

## DS-SQL-63: comparisons accept grouped right operands — verified

- Bounded CHECK comparison parsing now accepts a parenthesized supported scalar expression on the right, matching the existing grouped-expression support on the left. Existing contextual literal binding and same-type validation produce the same EIR1 nodes; protocol and storage formats are unchanged.
- Regression coverage binds `enabled = (priority >= 1)` to typed EIR1 and rejects incompatible types through the existing binder. The full serial delegated-cgroup workspace suite passes (289 library tests, 177 commit tests, sandbox/process tests enabled, zero ignored); workspace build, format check, warning-denied all-target Clippy, and `git diff --check` pass. Cargo.lock is unchanged.

## DS-SQL-64: checked integer arithmetic in CHECK expressions — implemented

- SQL CHECK binding now accepts SMALLINT, INTEGER, and BIGINT `+`, `-`, and `*`, with multiplication precedence, same-type operands, and contextual integer literal binding. NULL propagates. Runtime overflow fails closed as an expression evaluation error.
- EIR1 encoding writes version 2 for the new arithmetic opcodes and continues to decode version 1 expressions. Catalog expression metadata remains version 1. Decimal arithmetic and division remain unsupported.
- Regression coverage checks SQL precedence/type binding, checked evaluation, NULL propagation, overflow failure, EIR v2 round-trip, and EIR v1 decode compatibility. Focused regressions and the full serial delegated-cgroup workspace suite passed (290 library tests, 177 commit tests, sandbox/process coverage enabled, zero ignored). Workspace build, format check, warning-denied all-target Clippy, and `git diff --check` passed; Cargo.lock is unchanged.

## DS-SQL-65: integer modulo in CHECK expressions — implemented

- SQL CHECK binding accepts `%` for same-type SMALLINT, INTEGER, and BIGINT operands at the same multiplicative precedence as `*`. NULL propagates; division by zero and signed minimum modulo `-1` fail closed as evaluation errors.
- Authorization integration now also verifies a well-typed SMALLINT modulo-by-zero policy predicate takes the generic failing reference-scan path while an optimized unique-index probe returns `Unavailable`; the invocation is durably aborted.
- EIR1 now writes version 3 and decodes versions 1 and 2; modulo opcode 29 is version 3 only. Catalog expression metadata remains version 1. Focused SQL binding, evaluator, and codec regressions passed. The complete serial delegated-cgroup workspace suite passed (290 library tests, 177 commit tests, sandbox/process tests enabled, zero ignored); workspace build, format check, warning-denied all-target Clippy, and `git diff --check` passed. Cargo.lock is unchanged.

## DS-SQL-66: integer division in CHECK expressions — implemented

- SQL CHECK binding accepts `/` for same-type SMALLINT, INTEGER, and BIGINT operands at multiplicative precedence with `*` and `%`. Division truncates toward zero; NULL propagates; zero divisors and signed minimum divided by `-1` fail closed. DECIMAL division remains unsupported.
- EIR1 now writes version 4 and decodes versions 1–3; division opcode 30 is version 4 only. Catalog expression metadata remains version 1. Focused SQL binding, evaluator, and codec regressions passed. The complete serial delegated-cgroup workspace suite passed (290 library tests, 177 commit tests, sandbox/process tests enabled, zero ignored); workspace build, format check, warning-denied all-target Clippy, and `git diff --check` passed. Cargo.lock is unchanged.

## DS-SQL-67: unary integer negation in CHECK expressions — implemented

- SQL CHECK binding accepts unary `-` on SMALLINT, INTEGER, and BIGINT expressions. NULL propagates, and negating the signed minimum fails closed. Unary plus and DECIMAL arithmetic remain unsupported.
- EIR1 writes version 5, reads versions 1–4, and gates unary negate opcode 31 to v5; catalog expression metadata remains v1. Focused parser, evaluator, and codec regressions passed. The complete serial delegated-cgroup workspace suite passed (290 library tests, 177 commit tests, sandbox/process coverage enabled, zero ignored); workspace build, format check, warning-denied all-target Clippy, and `git diff --check` passed. Cargo.lock is unchanged.

## 2026-11 delegated acceptance checkpoint

- The full serial delegated-cgroup workspace suite passed with sandbox and process tests enabled and zero ignored tests. This includes the real-redb DDL commit matrix, schema/index/constraint crash-recovery checks, and cross-process SQL DDL lifecycles.
- Broader cross-component DDL race acceptance remains open; this pass covers the current suite, not every planned schedule.

## 2026-11-16 integrated acceptance refresh

- The complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests. It exercised the real-redb DDL commit matrix, schema/index/constraint crash-recovery tests, and gateway/supervisor SQL DDL lifecycles.
- Broader cross-component DDL race schedules and the complete ALTER matrix remain open.

## 2026-11-16 delegated integration refresh

- The complete serial delegated-cgroup workspace suite passed with sandbox and process tests enabled and zero ignored tests, including all 179 real-redb commit integration tests and the authenticated metadata process tests. The run includes the DD-5 concurrent schema create/drop and name-index reader regression.
- Broader cross-component DDL race schedules and the complete ALTER matrix remain open.

## DS-5 / DS-7: column-default ALTER racing a staged writer — verified slice

- Updated `storage_manager_sets_replaces_and_drops_column_defaults` to race the default change against a staged row insert. Either the row commits before the ALTER and has a committed outcome, or the ALTER publishes first and the stale writer conflicts with an ABORTED outcome; the row's persisted bytes and commit sequence are checked against the winning order.
- Verification: the focused real-redb DDL race passed; formatting, warning-denied all-target Clippy, and `git diff --check` passed. The complete serial delegated-cgroup workspace suite passed afterward with sandbox/process tests enabled and zero ignored tests, including 34 bootstrap binary tests, 179 commit tests, and 75 transaction tests.
- Remaining: stale-writer races for index/constraint DDL and additional ALTER combinations remain open.

## DS-5 / DS-7: DROP DEFAULT racing a staged writer — verified slice

- Extended `storage_manager_sets_replaces_and_drops_column_defaults` to race removal of an existing INTEGER default against a writer staged from the default-bearing snapshot. Writer-first commits before the drop; DROP-first durably aborts the stale writer. The regression checks row state, commit order, both durable outcomes, and the default-drop outcome.
- Verification: the focused real-redb DDL race passed. Workspace build, format check, warning-denied all-target Clippy, `git diff --check`, and the complete serial delegated-cgroup suite passed with sandbox and process tests enabled and zero ignored tests.

## DS-5 / DS-7: DROP COLUMN racing a staged writer — verified slice

- Extended `storage_manager_renames_tables_and_drop_column_rewrites_rows` to race the staged writer against the exclusive DROP COLUMN publication. If the row commits first, DROP COLUMN rewrites it to the surviving one-column schema before publication; if DDL wins, the stale write conflicts and no row is left behind. Both orders are checked against the durable outcome, while the pinned reader retains its original two-column view.
- Verification: the focused real-redb regression passed; formatting, warning-denied all-target Clippy, and `git diff --check` passed. The complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests, including 34 bootstrap binary tests, 179 commit tests, and 75 transaction tests.
- Remaining: stale-writer races for index/constraint DDL and additional ALTER combinations remain open.

## DS-6 / DS-7: index publication racing a staged writer — verified slice

- Extended `storage_manager_builds_and_drops_populated_indexes_atomically` to race publication of a catalog index against a staged row insert. A writer that commits first is included in the same index publication with matching forward and reverse locator records; if index publication wins, the stale writer conflicts and leaves no row. The test checks durable writer outcomes, table epoch advancement, and index backfill before exercising index drop.
- Verification: the focused real-redb regression passed; formatting, warning-denied all-target Clippy, and `git diff --check` passed. The complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests, including 34 bootstrap binary tests, 179 commit tests, and 75 transaction tests.
- Remaining: constraint DDL races and additional ALTER combinations remain open.

## DS-6 / DS-7: CHECK constraint creation racing a staged writer — verified slice

- `storage_manager_creates_and_drops_check_constraint` now races CHECK creation against a staged row insert using a barrier. If the row commits first, the constraint publication follows it and retains the row; if CHECK creation wins, the stale writer conflicts and leaves no row. The regression verifies commit ordering, durable outcome, row presence, and sequence-dependent later constraint drop.
- Verification: focused real-redb regression passed. Workspace build, format check, warning-denied all-target Clippy, and `git diff --check` passed. The complete serial delegated-cgroup workspace suite passed afterward with sandbox and process tests enabled and zero ignored tests: 291 engine unit tests, 179 commit tests, 75 transaction tests, and all other workspace targets.
- Remaining: additional constraint DDL races and ALTER combinations remain open.

## DS-4 / DS-7: foreign-key creation racing a staged child write — verified slice

- `foreign_key_creation_racing_a_staged_child_write_is_atomic` seeds a parent key, stages a non-NULL child row that references it from a snapshot without the FK, then races its commit against FK creation on that table. The only accepted outcomes are writer-first with both commits in sequence, or FK-first with the stale writer durably ABORTED and no row. The test verifies the FK record and dependency edge, writer outcome, row state, commit sequence, and persisted catalog integrity.
- Verification: the focused real-redb race passed. Locked workspace build, format check, warning-denied all-target Clippy, and the complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests; the suite reported 291 engine unit tests, 180 commit tests, 75 transaction tests, and all remaining workspace targets.
- Remaining: further constraint DDL schedules and the complete supported ALTER race matrix remain open.

## DS-3: reject CHECK expressions that do not match the table schema — verified slice

- Final catalog publication now binds CHECK expressions against the table's final column schema, validates BOOLEAN result type and exact stable-column dependencies, and rejects non-row-local expressions for every CHECK record.
- A real-redb catalog commit regression submits a CHECK that reads a BYTEA field while its record declares BOOLEAN. Publication fails atomically with `InvalidCatalog` and a durable ABORTED outcome.
- Verification: focused regression, locked workspace build, format check, warning-denied all-target Clippy, and the complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored.

## DS-4 / DS-7: foreign-key drop racing a staged child write — verified slice

- `foreign_key_drop_racing_a_staged_child_write_is_atomic` stages a valid child insert against a pinned catalog that still contains its FK, then races the writer commit with FK removal. Writer-first preserves the valid child row before the drop; drop-first makes the stale writer durably ABORTED without advancing the commit sequence. Both orders leave the FK, name index, and dependency removed, with startup catalog validation succeeding.
- Verification: focused real-redb race passed; locked workspace build, format check, warning-denied all-target Clippy, and `git diff --check` passed. The complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests; its 182 commit integration tests include this race.
- Remaining: stale-writer races for index/constraint DDL and additional ALTER combinations remain open.

## DS-7: SET NOT NULL racing a staged NULL insert — verified slice

- `set_not_null_racing_a_staged_null_insert_is_atomic` starts a staged NULL row insert and `SET NOT NULL` from the same nullable snapshot. If the row publishes first, the ALTER records an aborted `NotNullViolation`; if the ALTER publishes first, the stale insert records ABORTED. The test verifies exactly one successful commit, matching row/nullability state, both durable outcomes, and startup catalog validity.
- Verification: focused real-redb race, locked workspace build, format check, warning-denied all-target Clippy, and `git diff --check` passed. The full serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests: 291 unit tests, 183 commit tests, 75 transaction tests, and all remaining targets.
- Remaining: stale-writer races for index/constraint DDL and additional ALTER combinations remain open.

## DS-7: DROP NOT NULL racing a staged writer — verified slice

- Added `drop_not_null_racing_a_staged_writer_is_atomic`, pairing the existing SET NOT NULL race. A staged writer with a valid non-NULL row races the nullability relaxation; outcomes and row publication follow commit order, the column becomes nullable, the table schema version remains stable, and the write epoch advances once. Startup validates the final catalog graph.
- Verification: focused real-redb race passed. Workspace build, format check, warning-denied all-target Clippy, and the complete serial delegated-cgroup suite passed with sandbox/process tests enabled and zero ignored tests (293 unit tests, 187 commit tests, 76 transaction tests, plus all remaining targets).

## DS-7: CHECK removal racing a staged writer — verified slice

- Extended `storage_manager_creates_and_drops_check_constraint` to race removal of the CHECK, its expression, and its dependency edge against a writer staged from the old schema snapshot. Writer-first commits before removal; drop-first makes the stale writer durably ABORTED. The test checks row presence, current catalog cleanup, the pinned pre-drop snapshot, commit order, and runtime conflict accounting.
- Verification: focused real-redb regression, locked workspace build, format check, warning-denied all-target Clippy, and `git diff --check` passed. The complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests: 291 unit tests, 183 commit tests, 75 transaction tests, and remaining targets.
- Remaining: stale-writer races for index/key-constraint DDL and additional ALTER combinations remain open.

## DS-7: index removal racing a staged writer — verified slice

- Extended `storage_manager_builds_and_drops_populated_indexes_atomically` to race index removal against a staged insert using the pinned index definition. Writer-first commits the row before index removal; drop-first makes the stale writer durably ABORTED. In both orders the index definition is removed and neither forward nor reverse index membership remains, including for a row committed immediately before the drop.
- Verification: focused real-redb regression, locked workspace build, format check, warning-denied all-target Clippy, and `git diff --check` passed. The complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests: 291 unit tests, 183 commit tests, 75 transaction tests, and remaining targets.
- Remaining: stale-writer races for key-constraint DDL and additional ALTER combinations remain open.

## DS-7: key-constraint removal racing staged writers — verified slice

- Extended the UNIQUE and PRIMARY KEY lifecycle regressions to race constraint removal with a staged row insert using the pinned constraint/index snapshot. Writer-first commits the row before removal; drop-first records a durable conflict for the stale writer. Both tests verify row presence, DDL and writer outcomes, commit ordering, old-snapshot visibility, current catalog cleanup, and startup catalog validity.
- Verification: both focused real-redb regressions, locked workspace build, format check, warning-denied all-target Clippy, and `git diff --check` passed. The complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests: 291 unit tests, 183 commit tests, 75 transaction tests, and all remaining targets.
- Remaining: additional ALTER combinations remain open.

## DS-7: column rename racing a staged writer — verified slice

- Upgraded the column rename regression to race a writer staged against the old column name. Writer-first commits before the rename; rename-first makes the stale write durably ABORTED. Both paths check old/new name snapshots, row presence, durable outcome, and commit sequence.
- Verification: focused real-redb regression, locked workspace build, format check, warning-denied all-target Clippy, and `git diff --check` passed. The complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests: 291 unit tests, 183 commit tests, 75 transaction tests, and all remaining targets.
- Remaining: additional ALTER combinations remain open.

## DS-7 / DS-9: VARCHAR widening racing a staged writer — verified slice

- Upgraded the metadata-only widening regression to race the ALTER against a staged write using the old VARCHAR width. Writer-first is ordered before widening; widen-first durably aborts the stale write. The test checks durable outcomes, row presence, retained old column parameters, stable tuple/index schema versions and keys, and the final table epoch.
- Verification: focused real-redb regression, locked workspace build, format check, warning-denied all-target Clippy, and `git diff --check` passed. The complete serial delegated-cgroup suite passed with sandbox/process tests enabled and zero ignored tests: 291 unit tests, 183 commit tests, 75 transaction tests, and all remaining targets.
- Remaining: additional ALTER combinations remain open.

## DS-5 / DS-7: ADD COLUMN racing a staged writer — verified slice

- Upgraded the ADD COLUMN backfill regression to race publication against an insert staged from the old schema. If the insert commits first, ADD COLUMN backfills its new field from the immutable default; if DDL wins, the stale insert durably aborts. The test also verifies pinned pre-DDL readers, retained post-DDL rows, index membership, and durable outcomes.
- Verification: focused real-redb regression, locked workspace build, format check, warning-denied all-target Clippy, and `git diff --check` passed. The complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests: 291 unit tests, 183 commit tests, 75 transaction tests, and all remaining targets.
- Remaining: additional ALTER combinations remain open.

## DS-SQL-48 / DS-7: TRUNCATE races an indexed staged insert — verified slice

- Extended `storage_manager_truncates_rows_and_index_memberships_atomically` to race TRUNCATE against a staged insert carrying its catalog-derived index membership. If the insert commits first, TRUNCATE counts and removes it; if TRUNCATE wins, the writer durably conflicts. Both orders leave no row or index membership in the current snapshot, preserve the retained pre-TRUNCATE snapshot, and agree on the durable outcome and commit sequence.
- Verification: focused real-redb regression, locked workspace build, format check, warning-denied all-target Clippy, and the complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests: 291 unit tests, 183 commit tests, 75 transaction tests, and all remaining targets.

## DS-5 / DS-7: table rename races a staged row writer — verified slice

- Upgraded `storage_manager_renames_tables_and_drop_column_rewrites_rows` from a one-sided stale-writer check to a barrier race between table rename and an insert staged against the old name snapshot. Writer-first publishes before rename; rename-first durably aborts the stale writer. The test checks commit order, row bytes, durable outcome, current and retained name-index snapshots, and continued schema reads through the stable table ID.
- Verification: focused real-redb race passed. Formatting, locked workspace build, warning-denied all-target Clippy, and `git diff --check` passed. The complete serial delegated-cgroup workspace suite subsequently passed with sandbox and process integration tests enabled and zero ignored tests.

## DS-3 / DS-7: schema drop races table creation — verified slice

- Strengthened `schema_drop_racing_table_creation_never_leaves_an_orphan` to check both durable operation outcomes and the final catalog graph. If table creation wins, both schema records and both table records remain while DROP SCHEMA aborts as nonempty. If schema drop wins, the schema and typed records are absent, no object retains the dropped schema as parent, and table creation has a durable aborted outcome. Startup integrity validation runs after either order.
- Verification: the focused real-redb barrier race passed. Release-profile validation accepted 10 engine binaries and one query fixture; workspace build, format check, warning-denied all-target Clippy, and the complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests. `git diff --check` passed.
- Remaining: the broader cross-component DDL race matrix remains open.

## DS-3 / DS-5 / DS-7: mixed schema/table lifecycle under name-index readers — verified slice

- Expanded `schema_create_drop_pressure_preserves_pinned_name_index_snapshots` so each of 64 schema create/drop cycles also creates and drops a table. A concurrent reader repeatedly scans schema-name lookups; per-cycle fresh snapshots validate relation-name creation/removal, and a pinned snapshot retains the dropped table name.
- Verification: focused real-redb pressure regression, locked workspace build, format check, warning-denied all-target Clippy, and the complete serial delegated-cgroup suite passed with sandbox/process tests enabled and zero ignored tests (293 unit, 188 commit, 76 transaction tests, plus all remaining targets).

## DS-3 / DS-5: DROP SCHEMA and same-name CREATE preserve snapshot identity — verified slice

- Extended `storage_manager_creates_schema_atomically_for_privileged_actor` to retain a pre-drop schema-name snapshot, verify removal in a fresh snapshot, and create a new schema under the same name. The new DDL publishes a distinct stable ID; the old snapshot continues resolving the dropped ID.
- Verification: focused real-redb lifecycle regression, locked workspace build, format check, warning-denied all-target Clippy, and the complete serial delegated-cgroup suite passed with sandbox/process tests enabled and zero ignored tests (293 unit, 188 commit, 76 transaction tests, plus all remaining targets).

## DS-2: maximum DECIMAL, DATE, and TIMESTAMPTZ boundaries — verified slice

- Added a tuple-codec round trip for both signs of the 38-digit DECIMAL limit, the minimum and maximum supported DATE day values, and the minimum and maximum UTC TIMESTAMPTZ microseconds. The regression rejects a 39-digit DECIMAL and timestamp values one microsecond outside either endpoint; existing range coverage rejects DATE overflow.
- Verification: focused tuple regression, locked workspace build, format check, warning-denied all-target Clippy, and the complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests (293 unit, 188 commit, 76 transaction tests, plus all remaining targets). `git diff --check` passed.

## DS-5 / AU-5: catalog DEFAULT insert racing default replacement — verified slice

- Extended `storage_manager_sets_replaces_and_drops_column_defaults` to stage an authorized INSERT using the stored catalog DEFAULT, then race its commit against replacing that default. If the insert wins, it stores the old value before DDL; if replacement wins, the stale insert has a durable ABORTED outcome and no row is published.
- Verification: focused real-redb race, locked workspace build, format check, warning-denied all-target Clippy, `git diff --check`, and the complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests (293 unit, 189 commit, 76 transaction tests, plus all remaining targets).

## DS-5 / AU-5 / SQL-4: confined omitted-column default and generated identity — verified slice

- Extended the real confined SQL DML acceptance table with a catalog-backed required TEXT default beside its generated UUID identity. An explicit INSERT column list supplies only the UUID's `DEFAULT`; the executor sends DEFAULT for the omitted label, storage materializes it, and the committed row is decoded and checked for both values.
- The fixture grants INSERT on the non-generated column and adds its cell INSERT policy, verifying default materialization still passes through the normal grant and policy checks.
- Verification: `make -f engine/sdk/Makefile sandbox-query-fixture` passed, followed by locked workspace build, formatting, warning-denied all-target Clippy, and the complete serial delegated-cgroup workspace suite with sandbox/process tests enabled and zero ignored tests (294 unit, 190 commit, 76 transaction tests, plus all remaining targets). Broader default-expression and constraint matrices remain open.

## DS-7: table rename racing a column-default ALTER — verified slice

- Extended `storage_manager_sets_replaces_and_drops_column_defaults` with a barrier race between `RENAME TABLE` and `ALTER COLUMN SET DEFAULT` on the same table. Either operation may win; a losing stale catalog write records a durable conflict and is retried. The regression checks both first-attempt outcome records, the final name-index mapping, and persistence of the default expression.
- The new race first exposed that constraint authority validation could inspect stale payload bytes and return `InvalidCatalog` before the generic catalog expected-value check. The commit path now checks catalog write expectations under the affected-table DDL fence for table rename and constraint DDL, so stale DDL reports `Conflict` consistently while system privilege authorization errors retain their established ordering.
- Verification: focused real-redb race and focused system-privilege authorization regression passed. Locked workspace build, format check, warning-denied all-target Clippy, `git diff --check`, and the complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests (294 unit, 192 commit, 77 transaction tests, plus all remaining targets).

## DS-7 / AU-6: column-default ALTER races table-owner transfer — verified slice

- Added a barrier-coordinated real-redb race between an owner's `ALTER COLUMN SET DEFAULT` and an administrator's table-owner transfer. The operation results, durable outcomes, table epoch commit sequence, final owner, and any published default expression must agree with serialized commit order; a stale transfer conflict is retried and its replacement outcome is checked.
- After transfer, the former owner attempts another default alteration. It must be rejected as unauthorized, record an ABORTED outcome, and leave the commit sequence unchanged. The test finishes with persisted-catalog startup validation.
- Verification: the focused race passed 10 consecutive runs and again in the full suite. Locked workspace build, format check, warning-denied all-target Clippy, and `git diff --check` passed. The complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests (294 unit, 194 commit, 77 transaction tests, plus all other targets).
