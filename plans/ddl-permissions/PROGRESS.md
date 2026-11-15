# Permission DDL Progress

## DP-SQL-1: object-level grant SQL

- `admin-sql` now accepts one privilege on a named schema or table and resolves the object and grantee through authenticated metadata before using the established typed `objects.grant`/`objects.revoke` operation. Storage remains authoritative for scope validity, current owner/`MANAGE_GRANTS`, audit, and publication.
- Parser tests cover schema/table forms and reject scope mismatches, malformed directions, grant option, and multiple statements. The delegated gateway/supervisor lifecycle covers SQL USAGE grant/revoke.
- Locked workspace build, format check, warning-denied all-target Clippy, maintained TinyCC SDK tests/fixture, and full serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests. `git diff --check` passed; Cargo.lock is unchanged.

## DP-SQL-2: single-column grant SQL

- The bounded grammar now accepts one column target for table SELECT/INSERT/UPDATE/REFERENCES and binds it to an authenticated column ID before using the existing `objects.grant` or `objects.revoke` operation. Multiple column targets are still available only through typed APIs.
- Parser rejection and delegated process grant/revoke regressions pass. Locked workspace build, format check, warning-denied all-target Clippy, maintained TinyCC SDK tests/fixture, and full serial delegated-cgroup workspace suite pass with sandbox/process tests enabled and zero ignored tests. `git diff --check` passed; Cargo.lock is unchanged.

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
- The delegated gateway/supervisor lifecycle failed before this change at CREATE POLICY relation resolution, then passed when the table owner's SELECT grant was removed. This process regression verifies owner-based binding; the separate scoped `MANAGE_POLICIES` metadata-name-resolution case is not yet verified and remains follow-up work.
- Latest verification: the focused production metadata process test, locked workspace build, format check, warning-denied all-target Clippy, TinyCC SDK tests/fixture, release-profile validation, cgroup capability probe, full serial delegated-cgroup workspace suite with sandbox/process tests enabled, and `git diff --check` passed with zero ignored tests. Cargo.lock is unchanged.

## DP-1 / DP-2 / NP-6: column grant authority through separate service processes — verified

- Contract: [permission DDL](../../specs/ddl-permissions.md) and [client protocol](../../specs/client-protocol.md). An authenticated identity cannot grant privileges without current object ownership, inherited `MANAGE_GRANTS`, or database-administrator authority. An owner may grant a specific existing column privilege to a user.
- Regression: the production gateway/supervisor process test creates a table as its regular schema owner. A second authenticated user attempts to grant itself column-level SELECT and receives an administrative rejection. The owner then grants SELECT on that one column through `AdminClient`; dropping the table removes the grant before the recipient is dropped.
- Verification: the focused `supervisor_and_gateway_binaries_commit_a_confined_submission` test passed in the delegated `systemd --user` service. `cargo fmt --all -- --check`, workspace build, warning-denied Clippy, `git diff --check`, and the full serial workspace suite passed; sandbox integration tests were enabled and zero tests were ignored. The gateway fixture uses three control workers for the administrator, owner, and recipient connections that coexist during the check.
- Scope: covers object-grant actor authorization and successful column-grant publication across real service processes. It does not execute a query as the recipient or cover policy requirements and grant-provenance inspection.
