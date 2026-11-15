# Permission DDL Progress

## DP-SQL-1: object-level grant SQL

- `admin-sql` now accepts one privilege on a named schema or table and resolves the object and grantee through authenticated metadata before using the established typed `objects.grant`/`objects.revoke` operation. Storage remains authoritative for scope validity, current owner/`MANAGE_GRANTS`, audit, and publication.
- Parser tests cover schema/table forms and reject scope mismatches, malformed directions, grant option, and multiple statements. The delegated gateway/supervisor lifecycle covers SQL USAGE grant/revoke.
- Locked workspace build, format check, warning-denied all-target Clippy, maintained TinyCC SDK tests/fixture, and full serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests. `git diff --check` passed; Cargo.lock is unchanged.
- Follow-up: SQL grantee lookup now uses `metadata.grant_identities.resolve`, scoped to the target schema/table and available to its owner, inherited `MANAGE_GRANTS` holder, or database administrator. This removes the unrelated global `MANAGE_USERS`/`MANAGE_ROLES` requirement from object-grant SQL; the typed mutation still revalidates grant authority.
- Follow-up: the endpoint now resolves schema/table names and the grantee together, returning IDs only after scoped grant authority succeeds. Object-grant SQL no longer depends on ordinary schema/relation discovery merely to identify its target.
- Follow-up: declared column metadata is now available to the target table owner or inherited `MANAGE_GRANTS` holder without schema USAGE, allowing explicit-column grant SQL to bind its column names without broadening data access.
- The shared scoped-authority matcher now checks privilege codes in the catalog key's privilege segment and object scope separately. This restores the intended inherited `MANAGE_GRANTS` lookup and is reused for scoped `INDEX` metadata; workspace build and warning-denied Clippy pass.

## DP-SQL-2: single-column grant SQL

- The bounded grammar first added one column target for table SELECT/INSERT/UPDATE/REFERENCES and bound it to an authenticated column ID before using the existing `objects.grant` or `objects.revoke` operation. The follow-up below extends this route to atomic batches.
- Parser rejection and delegated process grant/revoke regressions pass. Locked workspace build, format check, warning-denied all-target Clippy, maintained TinyCC SDK tests/fixture, and full serial delegated-cgroup workspace suite pass with sandbox/process tests enabled and zero ignored tests. `git diff --check` passed; Cargo.lock is unchanged.

## DP-SQL-2: atomic multi-column grant SQL — verified

- The bounded grammar and typed PREPARE/START request carry 2–64 distinct column IDs in one grant/revoke operation. Storage validates the full target set and prepares all grant/dependency writes with one grantee epoch update, audit event, and durable outcome.
- Red/green: the parser regression first rejected `GRANT SELECT (first_name, last_name) ...`; the protocol regression verifies the array-shaped `target_ids` request. The delegated gateway/supervisor SQL lifecycle proves overlapping batch grants fail without granting their absent member, then verifies a successful batch grant and batch revoke.
- Verification: focused protocol and cross-process metadata tests; locked workspace build, format check, warning-denied all-target Clippy, maintained TinyCC SDK tests/fixture, release-profile validation, cgroup capability probe, and full serial delegated-cgroup workspace suite all passed with sandbox/process tests enabled and zero ignored tests. `git diff --check` passed; Cargo.lock is unchanged.

## DP-SQL-3: bounded CREATE ROW/CELL POLICY SQL — implemented

- Added operation-specific bounded policy clauses, row/cell targets, named USER/ROLE recipients, Boolean predicates, and `CURRENT_PRINCIPAL_ID()` / catalog-bound `HAS_ROLE(...)` binding to the typed `policies.create` operation. The delegated process test creates both row and cell policies, then removes them.
- Verification: parser/binder and delegated process tests, locked workspace build, format check, warning-denied all-target Clippy, maintained TinyCC SDK tests/fixture, and the full serial delegated-cgroup workspace suite pass with sandbox/process tests enabled and zero ignored; `git diff --check` passed. Cargo.lock is unchanged.
- Follow-up: `metadata.policy_identities.resolve` resolves one user or role only when the authenticated principal owns the policy schema/table, holds scoped `MANAGE_POLICIES`, or is a database administrator. Scoped table and column metadata is also available to those policy DDL authorities; the delegated owner lifecycle creates and drops row and cell policies with schema USAGE and no SELECT grant. Full suite evidence for this metadata visibility increment is recorded below.
- Remaining: broaden DDL SQL coverage for administrative object families beyond policies.

## DP-SQL-4: bounded DROP ROW/CELL POLICY SQL — verified

- Added a strict `DROP ROW|CELL POLICY name ON schema.table RESTRICT` parser and a scoped policy-name read that returns one ID only to database administrators, schema/table owners, or scoped `MANAGE_POLICIES` holders. The existing `policies.drop` mutation remains authoritative and rechecks DDL permission.
- Regressions: parser and protocol tests pass; the delegated gateway/supervisor lifecycle creates and drops row and cell policies through `admin-sql` as a table owner.
- Verification: focused parser/protocol tests and the delegated gateway/supervisor lifecycle passed. Locked workspace build, format check, warning-denied all-target Clippy, TinyCC SDK tests/fixture, and full serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored. `git diff --check` passed; Cargo.lock is unchanged.

## DP-SQL-5: policy-authorized table and column binding — verified

- Authenticated relation and column metadata now includes a table when the caller owns the table/schema or has scoped `MANAGE_POLICIES`, while still requiring schema USAGE and preserving ordinary grant-based discovery for other relations.
- The delegated gateway/supervisor lifecycle verifies both owner-based binding and non-owner manager binding. The table owner creates/drops row and cell policies with schema USAGE and no SELECT; a separate non-owner with schema USAGE and table-scoped `MANAGE_POLICIES` resolves the relation and creates/drops a row policy without data grants.
- Red/green: the manager regression first failed with unavailable metadata because relation grant discovery treated valid `MANAGE_POLICIES` as catalog corruption. The loader now filters valid non-data grants and preserves malformed target checks.
- Latest verification: the focused production metadata process test, locked workspace build, format check, warning-denied all-target Clippy, TinyCC SDK tests/fixture, release-profile validation, cgroup capability probe, full serial delegated-cgroup workspace suite with sandbox/process tests enabled, and `git diff --check` passed with zero ignored tests. Cargo.lock is unchanged.

## DP-1 / DP-2 / NP-6: column grant authority through separate service processes — verified

- Contract: [permission DDL](../../specs/ddl-permissions.md) and [client protocol](../../specs/client-protocol.md). An authenticated identity cannot grant privileges without current object ownership, inherited `MANAGE_GRANTS`, or database-administrator authority. An owner may grant a specific existing column privilege to a user.
- Regression: the production gateway/supervisor process test creates a table as its regular schema owner. A second authenticated user attempts to grant itself column-level SELECT and receives an administrative rejection. The owner then grants SELECT on that one column through `AdminClient`; dropping the table removes the grant before the recipient is dropped.
- Verification: the focused `supervisor_and_gateway_binaries_commit_a_confined_submission` test passed in the delegated `systemd --user` service. `cargo fmt --all -- --check`, workspace build, warning-denied Clippy, `git diff --check`, and the full serial workspace suite passed; sandbox integration tests were enabled and zero tests were ignored. The gateway fixture uses three control workers for the administrator, owner, and recipient connections that coexist during the check.
- Scope: covers object-grant actor authorization and successful column-grant publication across real service processes. It does not execute a query as the recipient or cover policy requirements and grant-provenance inspection.

## DP-2: table-wide data grants expand to current columns — verified

- Normative contract: unqualified table `SELECT`, `INSERT`, `UPDATE`, and `REFERENCES` grants expand to the columns present at operation time; later columns receive no grant.
- Red evidence: the delegated SQL lifecycle failed because `GRANT SELECT ON TABLE ...` produced an object-scope grant instead of one grant per current column.
- Implementation: unqualified table data privilege SQL uses one compact `all_columns` PREPARE target; storage resolves and atomically publishes per-column grants and dependency changes from its catalog snapshot. Explicit named-column lists retain their 1–64 bound. Large expansions keep the audit target list within AUD1's fixed limit.
- Verification: the delegated gateway/supervisor lifecycle checks every existing column receives a grant and an added column does not. Locked workspace build, format check, warning-denied all-target Clippy, maintained TinyCC SDK tests/fixture, release-profile validation, delegated cgroup probe, and full serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored. `git diff --check` passed; Cargo.lock is unchanged.

## DP-SQL-6: database-scoped system privilege syntax — implemented

- Added the permission spec's `GRANT|REVOKE privilege ON DATABASE TO|FROM USER|ROLE name` form for `MANAGE_USERS`, `MANAGE_ROLES`, and `CREATE_SCHEMA`. The preexisting explicit `SYSTEM PRIVILEGE` form remains supported; both route through the same authenticated name resolution and database-admin-only typed mutation.
- Parser regressions cover grant/revoke, user/role recipients, and rejection of unsupported privileges or scopes. The delegated metadata process lifecycle now grants and revokes `MANAGE_ROLES ON DATABASE` through `admin-sql`.
- Stale-test review: no current Rust `#[ignore]` tests were found; historical references to ignored tests remain only in progress evidence, so no test files were removed.
- Verification: parser regression passed (`1 passed, 0 ignored`); the delegated gateway/supervisor lifecycle passed with both GRANT and REVOKE via the `ON DATABASE` syntax. Locked offline workspace build, format check, warning-denied all-target Clippy, maintained TinyCC suite and sandbox fixture, release-profile validation (10 binaries and one query fixture), delegated cgroup probe, and the full serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests. `git diff --check` passed; Cargo.lock is unchanged.

## DP-5: atomic policy replacement — verified

- Red evidence: `CREATE OR REPLACE ROW POLICY ...` was rejected by the bounded parser.
- The SQL route now resolves a same-name row or cell policy and sends one `policies.replace` PREPARE/START operation. Storage composes the old policy's definitions, targets, and dependency removals with the new records and commits them under the existing authorization recheck; one table epoch is advanced.
- The delegated metadata lifecycle replaces an existing row policy with a cell policy and checks that the old ID disappears while its successor is published. A concurrent `MANAGE_POLICIES` revoke regression verifies the replacement either commits before revocation or aborts after it, with a table-epoch advance only on success.
- Verification: focused parser, protocol decoder, storage race, and delegated process tests passed. Locked workspace build, format check, warning-denied all-target Clippy, maintained TinyCC tests/fixture, release-profile validation (10 binaries and one query fixture), delegated cgroup probe, and full serial delegated-cgroup workspace suite passed with all sandbox/process tests enabled and zero ignored. `git diff --check` passed; Cargo.lock is unchanged.

## DP-SQL-7: conditional policy drops — implemented slice

- Added `DROP ROW|CELL POLICY IF EXISTS name ON schema.table RESTRICT`. Present targets dispatch the existing typed `policies.drop` mutation and retain policy authorization. Unresolved schemas, tables, or policies become a no-op only after the authenticated runtime-list read confirms database-administrator authority; no DDL outcome is created for the no-op.
- Updated permission and client SQL contracts and the `admin-sql` CLI dispatch. Workspace build, formatting, warning-denied all-target Clippy, and `git diff --check` pass. No tests were added or run; Cargo.lock is unchanged.

## DP-SQL-8: finite-set policy predicates — implemented slice

- Shared policy SQL parsing and binding now accepts column `IN` and `NOT IN` over 1–64 typed literals, including NULL. The expressions lower to existing EIR1 nodes and retain fail-closed policy evaluation semantics.
- Parser/binder regression coverage is shared with CHECK SQL. The focused test, locked workspace build, format check, warning-denied all-target Clippy, `git diff --check`, and full serial delegated-cgroup workspace suite pass with sandbox/process coverage and zero ignored tests. Cargo.lock is unchanged.

## DP-SQL-9: inclusive policy range predicates — implemented slice

- The shared policy expression grammar now accepts column `BETWEEN` and `NOT BETWEEN` with two typed literal bounds. Existing inclusive comparison nodes and SQL UNKNOWN evaluation preserve policy behavior without changing EIR1.
- Parser/binder regression coverage includes numeric/text bounds, negation, malformed forms, and type mismatch. The focused test, locked workspace build, format check, warning-denied all-target Clippy, `git diff --check`, and full serial delegated-cgroup suite pass with sandbox/process coverage and zero ignored tests. Cargo.lock is unchanged.
