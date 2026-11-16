# Metadata Discovery Progress

## MD-5: table metadata cursor retains its catalog snapshot across rename — verified

- Added a real-redb regression that opens the first `information_schema.tables` page, publishes a relation rename, and continues paging with the original invocation. The retained cursor returns the original name; a newly admitted invocation returns the renamed name.
- The fixture publishes the two catalog records directly because it deliberately omits production catalog records required by the DDL commit validator. This isolates snapshot behavior; rename authorization and catalog validation remain covered by DDL tests.
- Verification: focused transaction regression passed. Locked workspace build, format check, warning-denied all-target Clippy, TinyCC SDK tests, release profile/fixture, and full serial delegated-cgroup workspace suite passed (all tests enabled, zero ignored). `git diff --check` passed; Cargo.lock is unchanged.

## Administrative index pages include constraint-owned indexes — verified

- `metadata.indexes.list` now accepts constraint-owned index object records whose optional schema ID is absent. The index catalog record binds each index to the selected table, and the table record binds that table to the authorized schema; a present schema ID must still match.
- The delegated gateway/supervisor regression lists 130 UNIQUE constraints and their constraint-owned indexes across metadata pages. It first failed with `CorruptCatalog` on the valid omitted-schema-ID shape; after the validation fix, the focused process test passed.
- Contracts: [client protocol](../../specs/client-protocol.md), [metadata discovery](../../specs/metadata-discovery.md), and the [client SQL command](../../specs/client-sql-command.md).
- Verification: focused delegated process test passed (1 passed, 0 failed). Workspace build, format check, warning-denied Clippy, and `git diff --check` pass; `Cargo.lock` is unchanged.

## DP-SQL-3 follow-up: policy-scoped identity lookup — verified locally

- Added a point lookup for one named principal that first verifies a real schema/table relationship and then requires database-admin membership, schema/table ownership, or scoped `MANAGE_POLICIES`. Hidden and absent identities both return no result; no listing capability is exposed.
- Protocol decoder coverage and the focused delegated gateway/supervisor policy lifecycle pass. The SQL caller still needs existing relation and column visibility to bind names.
- Verification: locked workspace build, formatting check, warning-denied all-target Clippy, maintained TinyCC SDK tests/fixture, and full serial delegated-cgroup workspace tests passed with zero ignored tests. `git diff --check` passed and Cargo.lock is unchanged. Commit is the remaining handoff step.

## DP-SQL-4: policy-name point resolution — verified

- Added an authenticated, table-scoped policy-name lookup returning only the stable ID needed by DROP SQL. It requires the same ownership or scoped `MANAGE_POLICIES` authority as policy DDL; the mutation independently rechecks authorization.
- The delegated owner lifecycle drops both row and cell policies by name. Focused parser/protocol/process tests and the full serial delegated-cgroup suite pass with zero ignored tests; format, build, Clippy, TinyCC SDK tests/fixture, and `git diff --check` pass. Cargo.lock is unchanged.

## DP-SQL-5: policy-authorized metadata visibility — verified

- Relation discovery combines existing data-grant visibility with policy DDL authority for table owners and scoped `MANAGE_POLICIES` holders. Declared columns are visible within the same schema/table scope, without exposing rows or policy bodies.
- The process test failed before the change because the owner could not resolve the table without SELECT; it now passes for row/cell CREATE and DROP SQL with only schema USAGE. Locked workspace build, format check, warning-denied Clippy, TinyCC SDK tests/fixture, and full delegated-cgroup workspace tests passed with zero ignored; `git diff --check` passed and Cargo.lock is unchanged.

Migrated component-owned progress entries from [storage-manager progress](../storage-manager/PROGRESS.md); evidence is preserved below.

## MD-1 dependency: pinned authorized relation listing — in progress

- Red evidence: `authorized_relation_listing_uses_schema_usage_and_data_grants` failed against the empty listing scaffold because the granted schema returned no page.
- `InvocationTransaction::list_authorized_relations` now reads table IDs and definitions from the invocation's pinned catalog. It requires schema USAGE plus a persisted table SELECT/INSERT/UPDATE/DELETE or column SELECT/INSERT/UPDATE grant, validates the pinned role closure and each supplied grant, and omits inaccessible relations without reading rows or evaluating row policies. Stable relation IDs and names are returned in bounded pages (128 relations, 1 MiB); transaction-local cursors bind the invocation, schema, role/scope, grants and authority dependencies. A forged grant and a cursor from another invocation are rejected.
- Tests cover write-only discovery, column-grant discovery, inaccessible relations, schema USAGE denial, malformed hidden row bytes, catalog grant validation and continuation ownership.
- Checks: the locked workspace build, `cargo fmt --all -- --check`, Clippy with warnings denied, the full workspace test suite, and `git diff --check` passed in the prescribed Nix shell. Local Markdown links in the three changed documents resolved.
- Contracts: [metadata discovery](../../specs/metadata-discovery.md), [storage manager](../../specs/storage-manager.md), and [metadata discovery plan](../metadata-discovery/README.md).
- Remaining: serialized IPC cursors, built-in `information_schema` scans, relation resolution handles, and administrative discovery remain open.

## MD-1 dependency: authorized declared-column descriptions — in progress

- Red evidence: `authorized_column_description_returns_only_declared_shape` failed against the unavailable-result scaffold despite schema USAGE and a persisted table INSERT grant.
- `InvocationTransaction::describe_authorized_columns` validates the pinned role closure, actual grant records, schema usage, relation ownership in the schema, and the catalog table schema before returning stable column IDs, names, ordinals, scalar types, and nullability. It omits default and policy details and returns no partial shape when the 1 MiB response bound is exceeded.
- The regression verifies table write access permits declared-shape discovery and schema usage alone does not; the result type contains no default or policy fields.
- Checks: the locked workspace build, formatting check, warning-denied Clippy, the full workspace test suite, and `git diff --check` passed in the prescribed Nix shell. Focused transaction tests also passed.
- Contracts: [metadata discovery](../../specs/metadata-discovery.md), [storage manager](../../specs/storage-manager.md), and [metadata discovery plan](../metadata-discovery/README.md).
- Remaining: serialized IPC cursors, built-in `information_schema` scans, resolution handles, and administrative discovery remain open.

## MD-1 dependency: schema listing by pinned USAGE grants — in progress

- Red evidence: `schema_listing_includes_only_usage_grants_and_pages_by_schema_id` failed against the empty-page scaffold although two schemas had effective USAGE grants.
- `InvocationTransaction::list_authorized_schemas` validates the pinned role closure and supplied grant records, filters to schema USAGE within the admitted scope, and returns only live schema objects. Pages are bounded to 128 records and 1 MiB, ordered by stable schema ID, with a transaction-local cursor bound to the caller and authorization inputs.
- The real-redb fixture includes one ungranted schema between two granted schemas; the two visible schemas page in order and the hidden schema name is omitted.
- Checks: the locked workspace build, formatting check, warning-denied Clippy, full workspace tests, and `git diff --check` passed in the prescribed Nix shell. The focused transaction target passed.
- Contracts: [metadata discovery](../../specs/metadata-discovery.md), [storage manager](../../specs/storage-manager.md), and [metadata discovery plan](../metadata-discovery/README.md).
- Remaining: serialized IPC cursors, built-in `information_schema` scans, relation resolution handles, and administrative discovery remain open.

## MD-1 dependency: paginated declared-column descriptions — in progress

- Red evidence: `authorized_column_description_pages_bind_the_snapshot_and_order` failed against the unavailable page scaffold despite a table INSERT grant; it returned no description page.
- `InvocationTransaction::describe_authorized_columns_page` rechecks the pinned schema, role closure, grants, and dependencies on every page, then returns at most 128 declared columns and 1 MiB in ordinal order. Its continuation binds the invocation, relation, and authorization inputs; cross-invocation cursor reuse rejects. A too-large single column returns an error without a partial record.
- The real-redb regression pages a two-column relation, checks ordinals and ordering, and rejects a cursor in another invocation before successfully continuing the original snapshot.
- Checks: the locked workspace build, formatting check, warning-denied Clippy, full workspace tests, and `git diff --check` passed in the prescribed Nix shell. The focused transaction target passed as well.
- Contracts: [metadata discovery](../../specs/metadata-discovery.md), [storage manager](../../specs/storage-manager.md), and [metadata discovery plan](../metadata-discovery/README.md).
- Remaining: serialized IPC cursors, built-in `information_schema` scans, relation resolution handles, and administrative discovery remain open.

## MD-1 dependency: exact authorized relation resolution — in progress

- Red evidence: `authorized_relation_resolution_requires_visibility_and_exact_name` failed against the unavailable-result scaffold for the granted `metrics` relation.
- `InvocationTransaction::resolve_authorized_relation` walks filtered relation pages using an exact name match, then independently rechecks access before returning the stable relation ID, name, and declared columns. Hidden and nonexistent names both return no result. Input names and the combined description are bounded; the result does not authorize row reads.
- The real-redb regression verifies exact case-sensitive lookup, the declared shape, and indistinguishable unavailable results for a hidden relation and a missing name.
- Contracts: [metadata discovery](../../specs/metadata-discovery.md), [storage manager](../../specs/storage-manager.md), and [metadata discovery plan](../metadata-discovery/README.md).
- Remaining: serialized IPC cursors, built-in `information_schema` scans, connection-local executor handles, and administrative discovery remain open.

## MD-1 dependency: exact authorized schema resolution — in progress

- Red evidence: `authorized_schema_resolution_matches_only_visible_exact_names` failed against the unavailable-result scaffold for the USAGE-visible `app` schema.
- `InvocationTransaction::resolve_authorized_schema` walks the invocation's bounded USAGE-filtered schema pages and compares the exact requested name. Hidden and absent names return the same unavailable result; the resolved stable ID carries no authority by itself.
- The real-redb regression checks visible resolution, case sensitivity, and matching unavailable outcomes for a hidden schema and an absent name.
- Contracts: [metadata discovery](../../specs/metadata-discovery.md), [storage manager](../../specs/storage-manager.md), and [metadata discovery plan](../metadata-discovery/README.md).
- Remaining: serialized IPC cursors, built-in `information_schema` scans, connection-local executor handles, and administrative discovery remain open.

## MD-1 dependency: catalog-derived schema USAGE grants — in progress

- Red evidence: `schema_access_loader_uses_pinned_principal_schema_usage_grants` returned no grants against the unavailable-loader scaffold although the pinned catalog contains two schema USAGE records.
- `InvocationTransaction::load_catalog_schema_usage_grants` derives only schema USAGE grants for the admitted principal and its pinned role closure. It validates grantor and schema records and retains catalog guards for grants, schemas, grantors, and role membership so the authority is rechecked at finalization.
- The real-redb regression confirms that only the two granted schema records are returned; table and column grants do not enter the result.
- Contracts: [metadata discovery](../../specs/metadata-discovery.md), [storage manager](../../specs/storage-manager.md), and [metadata discovery plan](../metadata-discovery/README.md).
- Remaining: serialized IPC cursors, built-in `information_schema` scans, connection-local executor handles, and administrative discovery remain open.

## MD-1 dependency: invocation-bound schema listing — in progress

- Red evidence: `catalog_authorized_schema_listing_derives_the_invocation_scope` failed against the unavailable entry-point scaffold instead of returning the two schemas granted in the invocation's pinned catalog.
- `InvocationTransaction::list_catalog_authorized_schemas` derives the admitted principal's role closure and schema USAGE grants from the pinned catalog, constructs the authorization context internally, and delegates to the bounded page implementation. Caller-provided identities and grant scopes are not accepted.
- The real-redb regression pages the visible `app` and `public` schemas without a caller-supplied context or grant set.
- Contracts: [metadata discovery](../../specs/metadata-discovery.md), [storage manager](../../specs/storage-manager.md), and [metadata discovery plan](../metadata-discovery/README.md).
- Remaining: catalog-derived relation grants and reference resolution, serialized IPC cursors, built-in `information_schema` scans, connection-local executor handles, and administrative discovery remain open.

## MD-1 dependency: invocation-bound relation listing — in progress

- Red evidence: `catalog_authorized_relation_listing_derives_the_invocation_scope` failed against the unavailable entry-point scaffold rather than returning the two relations covered by persisted grants.
- `load_catalog_relation_discovery_grants` derives schema USAGE and table/column data grants for the admitted principal from its pinned role closure and one schema. It validates columns and grantors and captures authority guards. `list_catalog_authorized_relations` builds the context internally and delegates to bounded relation pages.
- The real-redb regression pages `metrics` and `secret` from their actual write-only/column grants and produces the same unavailable result for an ungranted schema and a missing schema ID.
- Contracts: [metadata discovery](../../specs/metadata-discovery.md), [storage manager](../../specs/storage-manager.md), and [metadata discovery plan](../metadata-discovery/README.md).
- Remaining: catalog-derived exact relation resolution, serialized IPC cursors, built-in `information_schema` scans, connection-local executor handles, and administrative discovery remain open.

## MD-1 dependency: invocation-bound qualified relation resolution — in progress

- Red evidence: `catalog_authorized_qualified_relation_resolution_uses_pinned_grants` returned no resolution against the unavailable-result scaffold for visible `app.metrics`.
- `InvocationTransaction::resolve_catalog_authorized_relation` resolves the schema name from pinned USAGE grants, then resolves the relation name with schema-scoped table/column grants. It constructs both authorization contexts internally and rechecks relation authority before returning the declared shape.
- The real-redb regression confirms exact qualified lookup and matching unavailable results for hidden and absent schemas and relations.
- Contracts: [metadata discovery](../../specs/metadata-discovery.md), [storage manager](../../specs/storage-manager.md), and [metadata discovery plan](../metadata-discovery/README.md).
- Remaining: serialized IPC cursors, built-in `information_schema` scans, connection-local executor handles, and administrative discovery remain open.

## MD-3 dependency: opaque schema cursor wire tokens — in progress

- Red evidence: the schema pagination regression failed after serializing and decoding a continuation because the in-process cursor's invocation and generation fields were absent from the wire form.
- `SchemaScanCursor` now serializes as a fixed 16-byte opaque token. The invocation retains its cursor position and authorization binding, consumes each issued token once, and rejects modified, replayed, and cross-invocation tokens. Token generation uses the operating-system randomness source and fails the invocation if secure randomness is unavailable.
- Tests cover byte round-trip through a later page, malformed token width, cross-invocation use, token modification, and replay after page completion.
- Contracts: [metadata discovery](../../specs/metadata-discovery.md), [storage manager](../../specs/storage-manager.md), and [metadata discovery plan](../metadata-discovery/README.md).
- Remaining: relation and column cursor wire forms, cursors across process restart, built-in `information_schema` scans, connection-local executor handles, and administrative discovery remain open.

## MD-3 dependency: relation and column cursor wire tokens — in progress

- The relation and column regressions now serialize continuations to fixed-width bytes and resume the same scan after decoding, so Rust struct fields are not part of the transport contract.
- Relation and column cursors now serialize as fixed 16-byte opaque tokens. Invocation-local state binds each token to its scan, authorization inputs, and next position; token consumption is one-use and uses OS randomness.
- Real-redb regressions round-trip both cursor types across page requests, reject cross-invocation use, and reject replay after a page completes. Schema cursor regressions also cover modified bytes and invalid token width.
- Contracts: [metadata discovery](../../specs/metadata-discovery.md), [storage manager](../../specs/storage-manager.md), and [metadata discovery plan](../metadata-discovery/README.md).
- Remaining: cursor lifetime across process restart, built-in `information_schema` scans, connection-local executor handles, and administrative discovery remain open.

## MD-3 dependency: cursor-kind binding

- Added a cursor-kind discriminator to the 16-byte schema, relation, and column tokens. Decoders reject tokens from the other cursor families; the remaining bytes use the operating-system random source.
- Regression assertions verify that each cursor encoding is accepted only by its matching decoder.
- Contracts: [metadata discovery](../../specs/metadata-discovery.md) and [storage manager](../../specs/storage-manager.md).

## MD-4 dependency: authorized `information_schema.schemata` projection

- Red evidence: `information_schema_schemata_exposes_public_and_authorized_names` failed before the projection was implemented.
- `InvocationTransaction::scan_information_schema_schemata` reuses the catalog-authorized schema scan and maps each visible schema to a row containing only `schema_name`, preserving page bounds and continuation behavior.
- The regression pages the `app` and `public` schemas through one invocation and verifies ordering and completion.
- Contracts: [metadata discovery](../../specs/metadata-discovery.md), [storage manager](../../specs/storage-manager.md), and [metadata discovery plan](../metadata-discovery/README.md).
- Remaining: SQL planner/executor integration for `information_schema`, `tables` and `columns` projections, connection-local executor handles, and administrative discovery.

## MD-4 dependency: schema-scoped `information_schema.tables` projection

- Red evidence: `information_schema_tables_projects_authorized_schema_pages` failed against the empty-result scaffold for visible schema `app`.
- `InvocationTransaction::scan_information_schema_tables_in_schema` resolves an exact schema name through pinned USAGE grants, applies the existing table/column grant filter, and projects each visible relation to `table_schema`, `table_name`, and `table_type = BASE TABLE`. Projected schema/type bytes count toward the existing bounded page size; relation continuations cannot be reused through the raw relation-list API.
- The real-redb regression pages `app` relations, returns an empty page for visible empty `public`, and gives the same unavailable result for hidden `private` and absent `missing` schemas.
- Contracts: [metadata discovery](../../specs/metadata-discovery.md), [storage manager](../../specs/storage-manager.md), and [metadata discovery plan](../metadata-discovery/README.md).
- Remaining: iterate all visible schemas for an unfiltered tables scan, SQL binding/execution, the columns projection, connection-local executor handles, and administrative discovery.

## MD-4 dependency: relation-scoped `information_schema.columns` projection

- Red evidence: `information_schema_columns_projects_declared_shape_in_pages` failed against the empty-result scaffold for visible `app.metrics`.
- `InvocationTransaction::scan_information_schema_columns_for_relation` resolves schema and relation names through pinned grants, pages the relation's declared columns, and projects one-based ordinals, YES/NO nullability, built-in type names, and only applicable type parameters. Projected metadata bytes count toward the page cap, and the continuation is bound to this projection.
- Regressions cover two ordinal-ordered pages, nullable values, BYTEA's inapplicable parameters, hidden/missing object equivalence, and rejection of the projection cursor by the raw description API. Type mapping covers VARCHAR and DECIMAL parameters plus integer numeric precision/scale.
- Contracts: [metadata discovery](../../specs/metadata-discovery.md), [storage manager](../../specs/storage-manager.md), and [metadata discovery plan](../metadata-discovery/README.md).
- Remaining: iterate all visible relations for a complete columns scan, SQL binding/execution, connection-local executor handles, and administrative discovery.

## MD-4 dependency: all-visible-schema `information_schema.tables` scan

- Red evidence: `information_schema_tables_scan_walks_visible_schema_pages` failed against the empty scan scaffold.
- `InvocationTransaction::scan_information_schema_tables` walks schema pages in stable schema-ID order and relation pages in stable relation-ID order. Its kind-specific opaque cursor retains the current schema and both child continuations; the scan stays bounded to one schema's relation page per call.
- The real-redb regression decodes and resumes a 16-byte token, verifies that the cursor kind is distinct, omits the hidden `private` schema, advances through empty `public`, and rejects cross-invocation use and replay.
- Contracts: [metadata discovery](../../specs/metadata-discovery.md), [storage manager](../../specs/storage-manager.md), and [metadata discovery plan](../metadata-discovery/README.md).
- Remaining: iterate visible relations for a complete columns scan, fixed public self-description rows, SQL binding/execution, connection-local executor handles, and administrative discovery.

## MD-4 dependency: all-visible-relation `information_schema.columns` scan

- Red evidence: `information_schema_columns_scan_walks_visible_relation_pages` failed against the empty scan scaffold for visible `app.metrics`.
- `InvocationTransaction::scan_information_schema_columns` walks schema, relation, and column pages in stable schema-ID/relation-ID/ordinal order. Its kind-specific token retains each active child continuation, while each call returns at most one relation's bounded column page.
- The real-redb regression pages two columns of `app.metrics`, continues to `app.secret`, advances over empty `public`, verifies cursor-kind isolation, and rejects cross-invocation use and replay.
- Contracts: [metadata discovery](../../specs/metadata-discovery.md), [storage manager](../../specs/storage-manager.md), and [metadata discovery plan](../metadata-discovery/README.md).
- Remaining: fixed public self-description rows, SQL binding/execution, connection-local executor handles, and administrative discovery.

## MD-4 dependency: public `information_schema` self-description

- Red evidence: `information_schema_builtin_relations_have_public_self_descriptions` first returned the granted `app` schema instead of the public reserved namespace.
- Schema discovery now includes `information_schema` as a public exception to user-schema USAGE grants, represented without recursively reading its virtual relations from the catalog. Its fixed logical relations are `columns`, `schemata`, and `tables`; all are reported as `VIEW`.
- The relation resolver and schema-scoped scans return fixed column IDs, names, ordinals, scalar types, and nullability for all three shapes. Global tables and columns scans include those definitions and retain their existing bounded nested cursors.
- Real-redb regressions cover the complete `columns` shape, the other relation definitions, paged `VIEW` rows, public schema listing, and global scans across built-in and granted user relations.
- Contracts: [metadata discovery](../../specs/metadata-discovery.md), [storage manager](../../specs/storage-manager.md), and [metadata discovery plan](../metadata-discovery/README.md).
- Remaining: SQL binding/execution for the virtual relations, connection-local executor handles, and administrative discovery.

## MD-3 dependency: executor connection relation handles

- Red evidence: the connection-handle regression did not compile because the invocation had no executor metadata session or handle API.
- `InvocationTransaction::open_executor_metadata_connection` creates a session-local registry. Resolution returns a random 16-byte kind-6 handle plus the authorized description; each revalidation resolves the stored qualified name through pinned catalog grants and checks its stable relation ID. Handles do not authorize data operations.
- Each session can issue at most 256 handles over its lifetime. Explicit close and session drop invalidate them. The regression covers byte round-trip, cursor-kind/width rejection, revalidation, close, and rejection from a replacement connection and another invocation.
- Contracts: [metadata discovery](../../specs/metadata-discovery.md), [storage manager](../../specs/storage-manager.md), and [IPC protocol](../../specs/ipc-protocol.md).
- Remaining: bind one session to each provisioned executor socket, implement scan/mutation request dispatch and reauthorization, SQL integration, and administrative discovery.

## SM-6 / MD-4: exercise virtual scans through executor IPC and the SQL consumer — verified

- Contract: [storage manager](../../specs/storage-manager.md), [metadata discovery](../../specs/metadata-discovery.md), and [SQL library](../../specs/sql-library.md). SQL supplies the admitted schema separately from its unqualified `FROM` relation; storage remains the authority for filtering metadata rows.
- Regression: `information_schema_virtual_scans_round_trip_through_executor_ipc` scans all three virtual relations through the production Unix-socket executor loop with a real redb invocation and checks ordered rows, typed cells, and NULLs. The maintained TinyCC fixture `sdk_sql_information_schema_v2.c` executes `SELECT schema_name FROM schemata WHERE schema_name = $1 LIMIT 1` with the schema set to `information_schema`, verifies filtering against two supplied sanitized rows, emits the matching row, and closes the live cursor at LIMIT.
- Verification: `make -f engine/sdk/Makefile test` passed. Workspace build, format, warning-denied Clippy, workspace tests, and `git diff --check` passed; the transaction target had 68 passing tests, and commit integration had 109 passing with 2 delegated-cgroup tests ignored.
- End-to-end regression: `sandboxed_v3_sql_dml_sequence_is_visible_and_commits` executes the TinyCC artifact through the confined worker with a writable delegated cgroup. The artifact queries all three virtual relations through SQL with parameterized schema filters, then completes the existing INSERT/SELECT/UPDATE/DELETE sequence; the test observes the provisional rows and committed terminal outcome. `sandboxed_v3_sql_error_latches_failure_and_durably_aborts_after_partial_output` also passed when run explicitly.
- Authorization setup: the test grants schema/table/column capabilities and installs row and cell allow policies for its fixture relation, matching the executor's table, column, and policy checks.
- SDK fix: close-cursor response decoding now matches the existing executor IPC wire shape (u16 version plus u8 status); the live LIMIT queries exercise cleanup.
- API/SQL parity: the supervisor exposes narrow read-only methods that query the owned invocation transaction for bounded metadata pages, including continuations. The confined acceptance test compares returned SQL projection values with the same invocation's metadata API rows for the public relations and a granted user schema/table/column. It also queries an ungranted table and a nonexistent name through `tables` and `columns`; SQL returns zero rows for each and the API pages omit the ungranted relation. An authorized empty schema appears in both API and SQL `schemata` results while its `tables` and `columns` queries return no rows through either path. A separate schema containing a table but lacking USAGE is absent from API `schemata`, `tables`, and `columns`; SQL predicates for that hidden schema and a nonexistent schema both yield zero rows across all three views.
- Verification: the rebuilt TinyCC artifact, maintained SDK suite, workspace build, format check, warning-denied Clippy, and workspace tests passed (commit target: 109 passed, 2 delegated-cgroup tests ignored). The confined metadata/DML acceptance and both delegated-cgroup SQL acceptance tests passed with the writable delegated-cgroup parent and declared runtime allowlists; `git diff --check` passed.
- Distinct principals: the real-redb discovery fixture now includes a second principal with USAGE on only the public `public` schema. `information_schema_scans_respect_each_principals_grants` verifies that the broader principal sees `app` and its granted/DELETE-visible relations, while the restricted principal sees only the public virtual schema; the corresponding table and column scan pages narrow with those grants.
- Distinct-principal SQL coverage is recorded below; the authenticated network path follow-up is recorded in the subsequent process integration.

## Grant provenance `prdb-client` projection — verified

- The existing authorized `metadata.grants.list` client page now has a `grant-list` CLI projection that follows connection-local pages and returns direct grants with grantor IDs, without principal names or inherited-grant expansion. The production process regression verifies a real column grant. The focused gateway/supervisor CLI regression passed and verifies direct grantor/grantee provenance without names. Workspace build, format, warning-denied Clippy, cgroup probe, full serial delegated-cgroup workspace suite, and `git diff --check` passed; see [client SQL command progress](../client-sql-command/PROGRESS.md). Cargo.lock is unchanged.

## Policy summary `prdb-client` projection — verified

- The existing `metadata.policies.list` page now has a `policy-list` CLI projection that follows connection-local pages and preserves the API's recipient redaction. It never returns expression identifiers or bodies. A production process regression exercises administrator visibility, including row/cell summaries, recipient IDs, and omission of expression bodies; a table-owner attempt receives the server's empty result.
- The focused process regression, workspace build, format, warning-denied Clippy, delegated cgroup probe, and full serial delegated-cgroup suite passed; see [client SQL command progress](../client-sql-command/PROGRESS.md). Cargo.lock is unchanged.

## Scoped grant target resolution type — implemented slice

- Added a named control result type carrying schema, object, and grantee IDs after the authenticated grant-scope lookup succeeds. This keeps the shared discovery contract explicit while returning no IDs for unauthorized or absent targets.
- Workspace build, formatting, warning-denied all-target Clippy, and `git diff --check` passed; `Cargo.lock` is unchanged. Focused decoder and process regressions remain to be added; no test suite was run for this increment.

## MANAGE_GRANTS column metadata for SQL binding — implemented slice

- The authenticated columns read now exposes a target table's declared shape to its owner or inherited scoped `MANAGE_GRANTS` holder without requiring schema USAGE. This lets object-grant SQL bind named columns while keeping the information limited to the object the caller may administer.
- Workspace build, formatting, warning-denied all-target Clippy, and `git diff --check` passed; `Cargo.lock` is unchanged. Focused authorization and process regressions remain to be added; no test suite was run for this increment.

## INDEX-authorized DDL metadata — implemented slice

- Scoped table owners and inherited `INDEX` grantees can resolve the schema/table and inspect only the declared columns and logical indexes needed for index DDL. The schema is discoverable from table ownership or a direct/inherited INDEX grant even without schema USAGE; data discovery remains unchanged.
- Workspace build, format, warning-denied all-target Clippy, and `git diff --check` passed; `Cargo.lock` is unchanged. Focused authorization/process regressions remain to be added; no test suite was run.

## ALTER-authorized constraint metadata — implemented slice

- Constraint ID/kind/source-column discovery now includes schema owners, table owners, database administrators, and inherited table-scoped `ALTER` grantees. ALTER principals can resolve the target schema/table and declared columns without data grants or schema USAGE. CHECK bodies remain omitted, and foreign-key target details still require visibility of the target schema.
- Metadata discovery, SDK, and SQL contracts are updated. The focused delegated gateway/supervisor lifecycle now verifies table-owner constraint listing and owner/direct-ALTER-grantee SQL name binding without schema USAGE. The focused process regression, workspace build, format check, warning-denied all-target Clippy, and `git diff --check` pass. Cargo.lock is unchanged.

## Schema CREATE-authorized SQL binding — implemented slice

- Schema-scoped `CREATE` grantees can resolve only the granted schema name through authenticated metadata, allowing SQL table creation to reach its existing typed authorization check without exposing that schema's relations or columns.
- Metadata and DDL contracts are updated. The delegated gateway/supervisor lifecycle grants schema-scoped `CREATE` without `USAGE` and successfully creates a table through `admin-sql`. The focused process regression, formatting, workspace build, warning-denied all-target Clippy, and `git diff --check` pass; Cargo.lock is unchanged.

## Schema ALTER-authorized SQL binding — verified

- Effective schema-scoped `ALTER` grantees can resolve schema names for `ALTER SCHEMA` without schema `USAGE`; this reveals no relations or columns. Typed schema rename, owner-transfer, and drop commit guards now accept and guard the direct/inherited ALTER grant, including grant revocation races.
- Permission, metadata-discovery, schema DDL, and client SQL contracts are updated. The gateway/supervisor lifecycle regression grants ALTER to a non-owner with no schema USAGE and renames the schema through `admin-sql`, then renames it back and revokes the grant. The focused delegated process test, workspace build, formatting, warning-denied all-target Clippy, and `git diff --check` pass. Cargo.lock is unchanged.

## DS-SQL-43: table ALTER-scoped identity resolution — verified

- Added `metadata.table_alter_identities.resolve` to resolve one destination USER/ROLE ID under target table owner/effective ALTER/database-admin authority. Unknown identities and insufficient target authority are non-disclosing; no identity listing or extra principal fields are returned.
- The real delegated process test exercises the protocol as an ALTER grantee. Workspace build, format check, warning-denied all-target Clippy, and `git diff --check` pass; Cargo.lock is unchanged.

## DS-SQL-44: schema ALTER-scoped identity resolution — verified

- Generalized the non-enumerating owner-name resolver to schema and table object scopes. Effective schema ALTER authority now permits the same target-scoped USER/ROLE resolution as table ALTER authority; untyped schema owners use the global unique-name index under that scope.
- The delegated process test exercises schema owner-name resolution as a schema ALTER grantee. Workspace build, format check, warning-denied all-target Clippy, and `git diff --check` pass; Cargo.lock is unchanged.

## DS-SQL-49 constraint object names — verified

- Administrative constraint pages now include the catalog object's current name in stable-ID order, including generated names for unnamed constraints. Existing CHECK-expression omission and foreign-key target redaction remain in effect.
- The client decoder enforces the expanded bounded record shape, and the authenticated gateway/supervisor SQL lifecycle reads back a named CHECK before dropping it by name. Full serial delegated-cgroup workspace verification passes; Cargo.lock is unchanged.

## 2026-11 delegated acceptance checkpoint

- The complete serial delegated-cgroup workspace suite passed with zero ignored tests. This includes 75 transaction tests for authorized schema/relation/column discovery, cursor scoping, `information_schema` projection, plus all three authenticated metadata process tests.
- Remaining administrative SQL binding and process-bound discovery handle work stays as recorded in the component plan.

## MD-6: schema-owner authority revocation between pages — verified

- The authenticated gateway/supervisor lifecycle now creates two policies, grants schema ownership to a non-administrator, opens a one-record administrative policy page, then transfers schema ownership back before continuing. The old owner’s continuation is rejected after current authority is rechecked.
- This exercises authority revocation while the catalog result itself remains unchanged; policy DDL permission is not treated as administrative discovery authority.
- Verification: `authenticated_metadata_and_admin_lifecycle_cross_gateway_and_supervisor_processes` passed through the production gateway and supervisor. The full delegated-cgroup workspace suite was last run at the prior acceptance checkpoint; it was not rerun for this focused process regression.

## MD-6: grant metadata cursor rechecks current authority — verified slice

- Extended the authenticated gateway/supervisor lifecycle to page direct grant metadata as a schema owner, transfer schema ownership back to the database administrator, and verify the former owner's grant continuation is rejected. Policy and grant cursor checks use separate authenticated sessions because a rejected control request closes its connection.
- This covers revocation of administrative grant visibility while the cursor holds a previously captured grant result; it complements the existing policy cursor check and follows the current-authority invalidation contract in [metadata discovery](../../specs/metadata-discovery.md).
- Verification: the focused production gateway/supervisor process test passed (1 passed, 0 failed). `cargo fmt --all -- --check`, locked workspace build, warning-denied all-target Clippy, maintained TinyCC SDK tests, and `sandbox-query-fixture` passed. The complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests (294 library tests, 196 commit tests, 78 transaction tests, 3 metadata process tests, and all other targets).

## 2026-11-16 integrated acceptance refresh

- The complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests. All three authenticated metadata process tests passed, including the MD-6 schema-owner transfer between administrative policy pages.
- Confined SQL SELECT over all three public projections, including predicates/limits and API parity, is covered by the maintained fixture acceptance recorded in SM-6 / MD-4 above. Cursor state is intentionally invocation/session-local; reconnect or process restart discards it and invalidates outstanding tokens under the normative contract.

## MD-3 / MD-6: administrative cursor session binding — verified slice

- The authenticated gateway/supervisor metadata lifecycle obtains a valid administrator schema continuation, closes that authenticated connection, reconnects with the same credential, and verifies the old continuation is rejected. The neighboring MD-6 case also verifies ownership transfer invalidates an existing owner's continuation.
- Verification: `cargo test --locked --test metadata_process authenticated_metadata_and_admin_lifecycle_cross_gateway_and_supervisor_processes -- --exact --nocapture` passed in the delegated user service. The complete serial delegated-cgroup workspace suite passed after this addition. Continuation after reconnect or process restart is out of contract; the cursor is session-bound and its server-side state is discarded.

## MD-3: schema continuation rejects page-size substitution — verified slice

- Extended `storage_manager_runtime_and_metadata_reads_enforce_authorization` to reuse a valid schema cursor with a different page limit. The substituted request is rejected, and the same cursor remains valid for the original page size, preserving the continuation's query binding.
- Verification: focused real-redb-backed authenticated metadata regression passed. Workspace build, format check, warning-denied all-target Clippy, and the complete serial delegated-cgroup suite passed with sandbox/process tests enabled and zero ignored tests (293 unit tests, 186 commit tests, 76 transaction tests, plus all remaining targets).

## MD-3: relation and column continuations reject page-size substitution — verified slice

- Extended the authenticated `MetadataClient` regression to substitute a different page size for relation and declared-column cursors. Both requests are rejected, and the original cursor remains usable with its bound page size.
- Verification: focused real-redb-backed authenticated metadata regression passed. Workspace build, format check, warning-denied all-target Clippy, and the complete serial delegated-cgroup suite passed with sandbox/process tests enabled and zero ignored tests.

## MD-2: hidden row data does not affect relation discovery — verified slice

- Extended `authorized_relation_listing_uses_schema_usage_and_data_grants` to capture an authorized metadata page while the inaccessible relation is empty, then insert malformed row bytes under that hidden relation and list again from a fresh invocation. Both pages are identical; hidden row storage is not decoded or reflected in ordinary metadata discovery.
- Verification: focused real-redb transaction regression, locked workspace build, format check, warning-denied all-target Clippy, and the complete serial delegated-cgroup suite passed with sandbox/process tests enabled and zero ignored tests (293 unit, 188 commit, 76 transaction tests, plus all remaining targets).

## 2026-11-16 delegated integration refresh

- The complete serial delegated-cgroup workspace suite passed with sandbox and process tests enabled and zero ignored tests. All three authenticated metadata process tests passed, covering gateway/supervisor discovery, session-bound cursor rejection after reconnect, and authority revalidation between pages.
- Cursor continuation after reconnect or process restart is out of contract: server-side session/invocation state is discarded and the old token is invalid.

## MD-5 / AU-7: executor IPC rejects a closed relation handle — verified slice

- The relation-resolution IPC regression now closes a valid connection-local handle, confirms `OPEN SCAN` with that token fails, then resolves the relation again and verifies the replacement token is distinct. The stale token remains unusable after the fresh handle is issued.
- Verification: focused real-redb IPC regression passed. Workspace build, format check, warning-denied all-target Clippy, and `git diff --check` passed. The complete serial delegated-cgroup workspace suite passed with sandbox/process coverage enabled and zero ignored tests: 291 engine unit tests, 179 commit tests, 75 transaction tests, and all other workspace targets.
- Remaining: broader hostile IPC sequence coverage remains open. Cursor continuation after reconnect or process restart is out of contract because its server-side session/invocation state is discarded.

## MD-5 / AU-7: executor IPC rejects cross-connection handle replay — verified slice

- `executor_resolve_relation_ipc_dispatch_returns_only_authorized_handle` now resolves a relation on one metadata connection and replays that handle through OPEN SCAN on a separate connection in the same invocation. The dispatch layer rejects the otherwise well-formed stale capability with `InvalidMetadataRequest`.
- Verification: the focused real-redb executor IPC regression, locked workspace build, format check, and warning-denied all-target Clippy passed. The full serial delegated-cgroup workspace suite passed with sandbox/process tests enabled: 291 engine unit tests, 179 commit tests, 75 transaction tests, and all other workspace targets.

## MD-5 / AU-7: executor IPC rejects cross-principal handle replay — verified slice

- Extended the dispatch regression to replay a valid resolved-relation handle in OPEN SCAN from a second principal's invocation. The wire-shaped token is rejected with `InvalidMetadataRequest`; no relation scan is opened.
- Verification: the focused real-redb IPC regression, locked workspace build, format check, warning-denied all-target Clippy, and full serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored.

## MD-5 / AU-7: closed relation handle rejected by scan and insert IPC — verified slice

- The relation-resolution IPC regression closes an issued capability, then replays it through both `OPEN SCAN` and `STAGE INSERT`. Both operations reject the closed token as `InvalidMetadataRequest`; a subsequent resolution returns a fresh usable handle.
- Verification: the focused executor IPC regression and the full serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests. The same run passed workspace build, formatting, and warning-denied all-target Clippy checks.

## MD-5 / AU-7: executor IPC rejects cross-principal row-handle replay — verified slice

- The catalog-authorized scan IPC regression now submits a real row handle from one principal's invocation to `STAGE DELETE` in another principal's invocation. The request fails as `InvalidMetadataRequest`, demonstrating that row capabilities remain scoped across principals at the IPC boundary.
- Verification: focused real-redb regression and the full serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests. Workspace build, format check, and warning-denied all-target Clippy passed.

## MD-5 / AU-7: row handles reject same-principal cross-invocation replay — verified slice

- The executor IPC regression now replays a valid row handle from one invocation through both `STAGE DELETE` and `STAGE UPDATE` in a second invocation for the same principal. Both requests fail with `InvalidMetadataRequest`, and the seeded row bytes are unchanged.
- Verification: focused real-redb IPC regression, formatting, locked workspace build, warning-denied all-target Clippy, `git diff --check`, and the complete serial delegated-cgroup workspace suite passed with sandbox and process tests enabled and zero ignored tests.

## MD-5 / DP-1: policy DDL authority does not disclose policy metadata — verified slice

- The authenticated gateway/supervisor lifecycle now checks policy discovery after schema ownership is returned to the database administrator while the former owner retains table-scoped `MANAGE_POLICIES`. Its administrative policy page is empty and has no continuation, so DDL authority alone cannot enumerate protected policy metadata. The adjacent MD-6 regression still verifies that a schema owner's already-issued policy cursor is rejected after ownership transfer.
- Verification: the focused cross-process metadata lifecycle passed through the production gateway and supervisor. Workspace build, format check, warning-denied all-target Clippy, `git diff --check`, and the full serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests.

## MD-5: virtual information_schema relations reject every row mutation — verified slice

- Extended `executor_scan_reads_filtered_information_schema_schemata` to obtain live row handles from `schemata`, `tables`, and `columns`, then verify UPDATE and DELETE rejection for each. The regression also attempts correctly shaped INSERTs into all three relations and verifies each is rejected.
- Verification: focused real-redb transaction regression, locked workspace build, format check, warning-denied all-target Clippy, `git diff --check`, and the complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests (293 unit, 189 commit, 76 transaction tests, plus all remaining targets).

## MD-6: dependency pages omit edges to hidden endpoints — verified slice

- Extended the authenticated gateway/supervisor metadata lifecycle with a source-schema owner whose foreign key points to a target outside that owner's visible scope. Constraint metadata still redacts the target table and columns; the direct dependency page now also returns no records and no continuation, so the hidden endpoint does not affect visible dependency counts.
- Verification: focused production gateway/supervisor process regression, locked workspace build, format check, warning-denied all-target Clippy, `git diff --check`, and the complete serial delegated-cgroup workspace suite passed with sandbox and process tests enabled and zero ignored tests (293 unit, 190 commit, 76 transaction tests, plus all remaining targets).

## MD-6 / ID-4: administrative token inventory excludes secret and verifier bytes — verified slice

- Strengthened the authenticated gateway/supervisor metadata lifecycle's `token-list` CLI check. In addition to requiring nonsecret token ID, label, and lifecycle metadata and rejecting a `token_secret` field, it computes the issued credential's raw secret and verifier digest and proves neither byte sequence appears in the inventory output.
- This exercises the administrative control response over the production gateway/supervisor process boundary; the lifecycle APIs still permit one-time delivery only from token issuance.
- Verification: the focused authenticated metadata process regression passed inside a delegated user service. Locked workspace build, format check, warning-denied all-target Clippy, `git diff --check`, and the full serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests (294 unit, 190 commit, 76 transaction tests, plus all remaining targets).

## MD-3 / MD-4: substituted information_schema continuation tokens reject — verified slice

- Extended the all-visible `information_schema.tables` and `information_schema.columns` cursor regressions with same-kind, fixed-width tokens whose opaque bytes have been modified. Both scans reject the substituted token as `InvalidMetadataRequest`; each token comes from an isolated invocation so rejection's normal invocation-abort behavior does not interfere with valid pagination checks.
- This complements the existing schema-token tampering check and the tables/columns cross-invocation and replay checks. The metadata spec already requires substituted tokens to fail closed, so no contract change was needed.
- Verification: focused table and column scan regressions passed. Locked workspace build, format check, warning-denied all-target Clippy, and `git diff --check` passed. The complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests (294 unit, 192 commit, 77 transaction tests, plus all remaining targets).

## MD-3 / AU-2: information_schema cursor retains grants from its pinned snapshot — verified slice

- Added `information_schema_tables_cursor_retains_grant_snapshot_across_revocation`. It begins a paged tables scan, revokes schema USAGE plus table and column SELECT grants and both derived-index directions, then verifies the retained cursor still returns the previously visible relation. A new invocation sees only the three public virtual relations, and replaying the old cursor in that invocation is rejected.
- The fixture changes grants in one atomic redb write so the check isolates invocation-snapshot semantics; production grant authorization and mutation paths are exercised in the separate permission and process lifecycle regressions.
- Verification: the focused real-redb snapshot regression passed. Locked workspace build, format check, warning-denied all-target Clippy, and `git diff --check` passed. The complete serial delegated-cgroup workspace suite passed with sandbox/process coverage enabled and zero ignored tests (294 unit, 194 commit, 78 transaction tests, plus all other targets).

## MD-6: index metadata cursor rechecks schema-owner authority — verified slice

- Extended the authenticated gateway/supervisor lifecycle with two indexes and a one-record index metadata page for a non-administrator schema owner. After schema ownership transfers back to the database administrator, continuing the former owner's cursor is rejected; the cursor uses its own authenticated session because rejected control requests close their connection.
- This verifies current authority is rechecked for index metadata continuations as well as grant and policy pages, while the captured index catalog rows remain unchanged.
- Verification: the focused production gateway/supervisor process test passed (1 passed, 0 failed, 0 ignored), and warning-denied all-target Clippy passed. The full delegated-cgroup workspace suite was not rerun for this process-test extension.

## MD-6: dependency metadata cursor rechecks schema-owner authority — verified slice

- Extended the authenticated gateway/supervisor metadata lifecycle to page direct dependencies for a table while the caller owns its schema, transfer the schema back to the database administrator, then continue the dependency cursor on the same authenticated session. The continuation is rejected after the caller loses visibility; each edge is still filtered so hidden endpoints do not appear.
- Verification: focused production gateway/supervisor process test passed inside the delegated `systemd --user` service; formatting, warning-denied all-target engine Clippy, and `git diff --check` passed.

## 2026-11-16 full delegated-cgroup workspace verification

- The complete serial workspace test suite passed inside the delegated `systemd --user` service with sandbox and process integration tests enabled: 955 passed, 0 failed, 0 ignored. This includes the current permission, authorization/schema race, and metadata discovery regressions. The run used the locked release profile and documented TinyCC/runtime artifacts.

## 2026-11-16 complete delegated workspace verification after token rotation

- Ran `cargo test --workspace --locked -- --test-threads=1` inside a delegated `systemd --user` unit with the documented TinyCC fixtures and runtime profile. The full suite exited successfully with sandbox and process integration tests enabled and zero ignored tests. The authenticated metadata/admin process test exercised `prdb-client token-rotate`, rejection of the old credential, authentication by the saved replacement, and subsequent revocation.

## 2026-11-16 complete delegated workspace verification after rotation race and CLI preflight

- Re-ran `cargo test --workspace --locked -- --test-threads=1` inside the delegated `systemd --user` unit with the documented TinyCC fixtures and runtime profile. The full workspace exited successfully with sandbox/process integration tests enabled and zero ignored tests. The suite includes the token-rotation/staged-write race and the authenticated process test for rejecting occupied credential-output paths before mutation.

## 2026-11-16 final delegated workspace verification for current tree

- Ran `cargo test --workspace --locked -- --test-threads=1` in the delegated `systemd --user` unit with the documented artifacts and runtime allowlist. The command exited successfully; sandbox and cross-process integration tests were enabled, including `token_rotation_racing_a_staged_write_linearizes` and the token-rotation CLI occupied-output preflight. Zero tests were ignored.


## 2026-11-16 full delegated workspace verification after identity rotation probes

- `cargo test --workspace --locked -- --test-threads=1` passed inside the delegated `systemd --user` service with the documented TinyCC fixtures and runtime profile. Sandbox and process integration tests were enabled; zero tests were ignored. The run includes storage and authenticated ADMIN checks that unauthorized rotations of existing and unknown token IDs return the same generic response.


## 2026-11-16 full delegated workspace verification after identity issuance hardening

- `cargo test --workspace --locked -- --test-threads=1` passed inside the delegated `systemd --user` service with the documented TinyCC fixtures and runtime profile. Sandbox and process integration tests were enabled; zero tests were ignored. This run includes the unauthorized token issue/rotation enumeration regressions and the 128-cycle foreign-key catalog pressure regression.

## 2026-11-16 MD-1 / DS-8: metadata confirms stable column identity after process DDL rename — verified slice

- The production gateway/supervisor ADMIN lifecycle now runs column rename SQL and reads the result through authenticated metadata. The renamed definition retains its original column ID and ordinal, confirming the discovery projection follows the catalog's stable identity through DDL.
- Verification: focused delegated `metadata_process` regression passed with sandbox/runtime configuration; formatting, warning-denied workspace all-target Clippy, and `git diff --check` passed. The complete serial workspace suite passed inside the delegated `systemd --user` service with sandbox and process tests enabled and zero ignored tests.

## 2026-11-16 MD-5 / AU-7: UPDATE and DELETE invalidate duplicate row handles — verified slice

- Extended `catalog_authorized_scan_builds_its_projection_from_pinned_records` to open a second scan and issue distinct handles for the same two rows. After updating one row through its first handle, the alias from the second scan is rejected; deleting the other row likewise invalidates its alias. The regression exercises the executor IPC dispatch and revalidation path.
- Verification: the focused real-redb/IPC regression passed. Workspace formatting, warning-denied all-target Clippy, and `git diff --check` passed. The complete serial delegated-cgroup workspace suite passed with sandbox and process tests enabled and zero ignored tests.

## 2026-11-16 delegated workspace verification after VARCHAR probe type check

- The complete serial `cargo test --workspace --locked -- --test-threads=1` suite passed inside the delegated `systemd --user` service. Sandbox and cross-process coverage were enabled; zero tests were ignored. This run revalidated metadata cursor authority, information-schema scans, executor handles, and authenticated gateway/supervisor discovery regressions on the current tree.
