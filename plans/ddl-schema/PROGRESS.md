# Schema DDL Progress

Migrated component-owned progress entries from [storage-manager progress](../storage-manager/PROGRESS.md); evidence is preserved below.

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
- Remaining: column/table rename gateway dispatch coverage, role-owner authorization regression, DDL stale-writer race, and other supported alterations remain open.

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
- Regression: `process_crash_during_authorized_index_ddl_recovers_all_or_none` builds a real schema, table, and row, then invokes authorized `StorageManager::create_index` in a child process. One child exits immediately after the backend accepts its first write during DDL publication; reopen must show no index, audit, outcome, or sequence advance. A second child exits immediately after `create_index` returns; reopen must validate the complete index definition, forward locator, reverse membership, audit, committed outcome, and exactly one sequence advance. Both reopened databases pass startup validation.
- Scope: this exercises abrupt process loss at the backend write boundary and after durable API completion. It does not simulate power loss or claim filesystem-level durability beyond redb's configured sync contract.
- Verification: the focused two-boundary process test passed. `cargo build --workspace --locked`, `cargo fmt --all -- --check`, warning-denied workspace Clippy, TinyCC SDK tests and sandbox fixture build passed. The full serial workspace suite returned success inside the delegated `systemd --user` service: 222 library tests, 151 commit tests, 75 transaction tests, all targets reported zero ignored tests, and sandbox/process tests were enabled. The cgroup probe reported writable v2 delegation with `cpu`, `memory`, and `pids`; `git diff --check` passed.
- Remaining: crash/reopen coverage for other multi-record DDL publications and actual power-loss validation remain open.

## DS-5 / DS-7: read-only invocation completes across table rename — verified

- Red evidence: an admitted invocation loaded a table schema before `rename_table`, retained the old redb catalog snapshot, and correctly saw the old definition; its read-only COMMIT then failed with `Conflict` because commit validation still checked data catalog guards even though it published no rows or index changes.
- Implementation: `InvocationTransaction::commit_inner` skips the family-2 display-name object and family-5 table epoch guards when no row or index writes are being published. Read-only invocations record their durable outcome against their pinned table definition after a rename; other catalog guards still revalidate. Staged writes retain the full catalog conflict checks.
- Regression: `storage_manager_renames_tables_and_drop_column_rewrites_rows` verifies old and new catalog snapshots resolve their respective table names, the old invocation retains its loaded schema across rename, both read-only invocations commit, and the stale writer still aborts without publishing its row.
- Verification: focused rename, token-revocation, schema-guard, and index-guard integration tests passed. Workspace build, format, warning-denied Clippy, TinyCC SDK tests and sandbox fixture, release-profile validation, delegated cgroup probe, and the full serial delegated-cgroup workspace suite passed. The full suite included 224 library tests, 154 commit tests, and 75 transaction tests; all targets reported zero ignored tests. `git diff --check` passed.

## DS-6 / DS-7: table rename process-crash recovery — verified

- Contract: [schema DDL](../../specs/ddl-schema.md) requires the table name, table epoch, audit event, operation outcome, and commit sequence to recover as one atomic publication.
- Regression: `process_crash_during_table_rename_recovers_all_or_none` creates a real schema/table, then exits a child process at the backend write boundary or immediately after `rename_table` returns. Reopen validates the database and checks old/new name resolution, audit, durable outcome, and commit sequence for either the complete old state or complete new state.
- Verification: focused two-boundary process-crash test passed. Workspace build, format check, warning-denied Clippy, TinyCC SDK tests and sandbox fixture, release-profile validation, and the delegated cgroup probe passed. The full serial delegated-cgroup workspace suite passed with 224 library tests, 155 commit tests, 75 transaction tests, and zero ignored tests across all targets. `git diff --check` passed.
- Scope: process exit tests atomic backend recovery and durable API completion; it does not simulate power loss or extend redb's filesystem durability guarantee.

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
- Regression: `process_crash_during_drop_table_recovers_all_or_none` creates an indexed table with a persisted row, exits a child at the backend write boundary or after `drop_table` returns, then reopens and validates startup. The interrupted path retains the complete table, row, forward index locator, reverse index membership, and old sequence without an audit or outcome; the completed path removes them and recovers the audit, committed outcome, and one sequence advance.
- Verification: focused two-boundary process-crash test passed. Workspace build, format check, warning-denied Clippy, maintained TinyCC SDK tests, release-profile validation (10 binaries and one query fixture), and the delegated cgroup probe passed. The complete serial workspace suite passed in the delegated `systemd --user` service: 224 library tests, 158 commit tests, and 75 transaction tests, including confined SQL and process integration tests.
- Scope: process exit checks atomic recovery for one indexed table with one row. It does not simulate power loss or validate every combination of owned constraints, policies, grants, and dependency records.

## DS-3 / DS-7: CREATE TABLE process-crash recovery — verified

- Contract: [schema DDL](../../specs/ddl-schema.md) requires a table definition and its typed columns/name indexes to publish with the audit event, durable outcome, and commit sequence as one transaction.
- Regression: `process_crash_during_create_table_recovers_all_or_none` creates an authorized schema, then exits a child at the backend write boundary or after `create_table` returns. Reopen and startup validation show either no table name, audit, outcome, or sequence advance, or a resolvable table and column with valid typed records, audit, committed outcome, and one sequence advance.
- Verification: focused two-boundary process-crash test passed. Workspace build, format check, warning-denied Clippy, maintained TinyCC SDK tests, release-profile validation (10 binaries and one query fixture), delegated cgroup probe, and the complete serial delegated-cgroup workspace suite passed: 224 library tests, 159 commit tests, and 75 transaction tests, including confined SQL and process integration tests.
- Scope: process exit covers creation of one table with one column and no constraints. It does not simulate power loss or cover constraint/default creation combinations.

## DS-4 / DS-7: UNIQUE constraint process-crash recovery — verified

- Contract: [schema DDL](../../specs/ddl-schema.md) requires the UNIQUE constraint, its owned index definition and memberships, table epoch, audit event, durable outcome, and commit sequence to publish atomically after validating existing rows.
- Regression: `process_crash_during_unique_constraint_creation_recovers_all_or_none` seeds a real table row, then exits a child at the backend write boundary or after `create_unique_constraint` returns. Reopen and startup validation require either no constraint, audit, outcome, or sequence advance, or the complete UNIQUE definition and owned index with forward locator, reverse membership, preserved row, committed outcome, and one sequence advance.
- Verification: focused two-boundary process-crash test passed. Workspace build, format check, warning-denied Clippy, maintained TinyCC SDK tests, release-profile validation (10 binaries and one query fixture), delegated cgroup probe, and the complete serial delegated-cgroup workspace suite passed: 224 library tests, 160 commit tests, and 75 transaction tests, including confined SQL and process integration tests.
- Scope: process exit covers one UNIQUE constraint over one non-null integer column and one row. It does not simulate power loss or cover composite keys, duplicate rejection, or PRIMARY KEY publication.

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
