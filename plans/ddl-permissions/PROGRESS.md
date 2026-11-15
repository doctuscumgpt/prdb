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
- Follow-up: `metadata.policy_identities.resolve` now resolves one user or role only when the authenticated principal owns the policy schema/table, holds scoped `MANAGE_POLICIES`, or is a database administrator. The focused delegated gateway/supervisor lifecycle and full serial delegated-cgroup workspace suite pass with policy SQL issued as a table owner. Locked workspace build, formatting, warning-denied Clippy, maintained TinyCC SDK tests/fixture, and `git diff --check` pass; no tests were ignored and Cargo.lock is unchanged. The owner fixture currently needs schema USAGE and SELECT visibility to resolve the qualified table and its columns through the existing metadata APIs.
- Remaining: scoped relation/column discovery that works without SELECT, and DROP ROW/CELL POLICY SQL.

## DP-1 / DP-2 / NP-6: column grant authority through separate service processes — verified

- Contract: [permission DDL](../../specs/ddl-permissions.md) and [client protocol](../../specs/client-protocol.md). An authenticated identity cannot grant privileges without current object ownership, inherited `MANAGE_GRANTS`, or database-administrator authority. An owner may grant a specific existing column privilege to a user.
- Regression: the production gateway/supervisor process test creates a table as its regular schema owner. A second authenticated user attempts to grant itself column-level SELECT and receives an administrative rejection. The owner then grants SELECT on that one column through `AdminClient`; dropping the table removes the grant before the recipient is dropped.
- Verification: the focused `supervisor_and_gateway_binaries_commit_a_confined_submission` test passed in the delegated `systemd --user` service. `cargo fmt --all -- --check`, workspace build, warning-denied Clippy, `git diff --check`, and the full serial workspace suite passed; sandbox integration tests were enabled and zero tests were ignored. The gateway fixture uses three control workers for the administrator, owner, and recipient connections that coexist during the check.
- Scope: covers object-grant actor authorization and successful column-grant publication across real service processes. It does not execute a query as the recipient or cover policy requirements and grant-provenance inspection.
