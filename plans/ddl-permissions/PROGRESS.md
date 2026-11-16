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

## DP-SQL-10: null-safe policy comparisons — implemented slice

- Policy SQL now accepts `column IS DISTINCT FROM literal` and `column IS NOT DISTINCT FROM literal`, including NULL. Lowering composes existing null checks and inequality expressions so policies receive a definite Boolean result for nullable columns.
- Parser/binder regressions cover text and NULL comparisons plus rejected malformed operands. The focused test, locked workspace build, format check, warning-denied all-target Clippy, `git diff --check`, and full serial delegated-cgroup suite pass with sandbox/process coverage and zero ignored tests. Cargo.lock is unchanged.

## DP-SQL-7: repeated absent conditional policy drop — verified

- The authenticated gateway/supervisor metadata lifecycle now executes `DROP ROW POLICY IF EXISTS` for the same absent target twice. Both requests return a no-op without creating a policy mutation, while the neighboring existing-target drops still use the ordinary authorized path.
- Verification: `nix develop --no-update-lock-file --command cargo test -p prdb-engine --test metadata_process authenticated_metadata_and_admin_lifecycle_cross_gateway_and_supervisor_processes -- --exact --nocapture` passed (1 test). The broader workspace checks were not rerun for this test-only increment.

## 2026-11 delegated acceptance checkpoint

- The complete serial delegated-cgroup workspace suite passed with sandbox and process tests enabled and zero ignored tests. The authenticated gateway/supervisor lifecycle covers repeated absent `DROP ROW POLICY IF EXISTS` calls alongside existing-target policy drops; see DP-SQL-7 above.
- Broader permission-change race schedules remain open.

## 2026-11-16 integrated acceptance refresh

- The complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests. It includes object-grant revocation linearization, policy-owner/manager authorization and replacement, policy evaluation, and cross-process grant/policy DDL coverage.
- Broader permission-change race schedules and administrative SQL coverage beyond the implemented forms remain open.

## DP-SQL-11: atomic multi-privilege grant SQL — verified

- `GRANT` and `REVOKE` accept a bounded comma-separated privilege list. The authenticated protocol carries that list as one PREPARE/START operation; storage validates all requested grant states before publishing grant records, provenance dependencies, one grantee epoch change, one audit event, and one durable outcome in a single catalog commit.
- The delegated gateway/supervisor metadata lifecycle grants `ALTER, INDEX, MANAGE_POLICIES, MANAGE_GRANTS` together, verifies every table-scoped grant through metadata discovery, revokes the same set together, and verifies no grant remains. Parser and protocol regressions cover the documented list, duplicates, unknown names, and column-list restrictions.
- Verification: focused parser/protocol/client tests and the cross-process metadata lifecycle passed; locked workspace build, format check, warning-denied all-target Clippy, maintained TinyCC SDK tests and sandbox fixture, and the full serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests. `git diff --check` passed; Cargo.lock is unchanged.

## DP-8 / DP-SQL-11: batched grants race authority revocation — verified slice

- A storage-level race starts an `ALTER, INDEX` grant batch against revocation of the actor's `MANAGE_GRANTS` grant. If the batch commits first, both privileges, their shared dependency, one grantee epoch increment, and a committed outcome are present. If revocation commits first, the batch aborts and none of those batch records are present. The revocation always commits.
- Verification: the focused real-redb race, workspace format check, warning-denied all-target Clippy, and `git diff --check` passed. The complete serial delegated-cgroup workspace suite passed afterward with sandbox/process tests enabled and zero ignored tests; it reported 291 engine unit tests, 178 commit tests, and 75 transaction tests.

## DP-8: deterministic revocation-first batch denial — verified slice

- Extended `object_grant_batch_racing_manage_grants_revocation_is_atomic` beyond its barrier race with a deterministic revoke-before-batch order. After `MANAGE_GRANTS` is revoked, a two-privilege INSERT/UPDATE batch is denied and durably ABORTED; neither grant nor provenance edge appears, the grantee epoch stays unchanged, and the commit sequence does not advance for the rejected batch.
- Verification: focused real-redb race plus deterministic order regression, locked workspace build, format check, warning-denied all-target Clippy, `git diff --check`, and complete serial delegated-cgroup workspace suite passed with sandbox and process tests enabled and zero ignored tests (293 unit, 190 commit, 76 transaction tests, plus all remaining targets).

## DP-8 / AU-6: policy removal races a staged writer — verified slice

- Extended the policy lifecycle regression to race removal of a row policy and its private expression against a writer staged from the policy-bearing snapshot. Writer-first publishes before the removal; drop-first durably aborts the stale write. The regression checks row state, both outcomes, commit ordering, retained old-snapshot visibility, and current catalog removal.
- Verification: the focused real-redb regression, locked workspace build, format check, warning-denied all-target Clippy, and `git diff --check` passed. The complete serial delegated-cgroup suite passed with sandbox/process tests enabled and zero ignored: 291 unit tests, 183 commit tests, 75 transaction tests, and all remaining targets.

## DP-8 / ID-6: MANAGE_ROLES revocation linearizes with membership grant — verified slice

- The membership DDL race now covers revocation of `MANAGE_ROLES` while the same actor submits a role membership grant. The privilege owner revocation commits in both orders; the membership edge and its durable outcome match the commit order.
- Verification: focused real-redb regression, locked workspace build, format check, warning-denied all-target Clippy, and `git diff --check` passed. The complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests: 291 unit tests, 183 commit tests, 75 transaction tests, and all remaining targets.

## DP-3: duplicate policy names reject before publication — verified slice

- Extended `storage_manager_authorizes_policy_ddl_for_owners_and_inherited_manage_policies` to create a policy, then attempt a second policy with the same table-local name. The duplicate returns `InvalidRequest`, and the catalog commit sequence remains unchanged.
- Verification: focused real-redb policy DDL regression, locked workspace build, format check, warning-denied all-target Clippy, and the complete serial delegated-cgroup suite passed with sandbox/process tests enabled and zero ignored tests (293 unit, 188 commit, 76 transaction tests, plus all remaining targets).

## 2026-11-16 delegated integration refresh

- The complete serial delegated-cgroup workspace suite passed with sandbox and process tests enabled and zero ignored tests: 291 engine unit tests, 179 commit integration tests, and 75 transaction tests. It includes atomic multi-privilege grants racing `MANAGE_GRANTS` revocation plus the authenticated metadata grant/policy lifecycle.
- Broader permission-change race schedules and administrative SQL forms outside the implemented subset remain open.

## DP-1 / ID-5: shared system-privilege dependency lifetime — verified slice

- Fixed system-privilege DDL so the shared authority dependency is inserted with the first database privilege granted to a principal and removed only when the last privilege is revoked. Adding or removing another privilege leaves the shared edge intact; grantee epochs still serialize concurrent changes.
- Regression coverage races CREATE_SCHEMA and MANAGE_USERS grants to the same principal, retries a stale conflict, then verifies both privileges and the shared dependency coexist. It also verifies revoking either one preserves the other and the edge, while revoking the last privilege removes the edge. The separate gateway/supervisor process regression verifies MANAGE_USERS grant and revocation on an already open ADMIN connection.
- Verification: the focused real-redb race regression passed. Workspace build, format check, warning-denied all-target Clippy, `git diff --check`, and the complete serial delegated-cgroup workspace suite passed with sandbox and process tests enabled and zero ignored tests (293 unit tests, 185 commit tests, 76 transaction tests, plus all remaining targets).

## DP-7 / AU-5: catalog policy references freeze executor updates — verified slice

- Extended `catalog_authorized_scan_builds_its_projection_from_pinned_records` with a catalog-bound UPDATE policy for a different principal whose USING and WITH CHECK expressions read the table's BYTEA column. The writer has an unconditional applicable UPDATE policy, so its `STAGE UPDATE` is denied specifically because the referenced field is globally policy-driving; the committed row bytes remain unchanged.
- Executor write projections now gather expression field references from all table policies, while authorization evaluation remains recipient-filtered. Those definitions and expressions are retained as pinned commit guards.
- Verification: focused real-redb transaction regression passed. Locked workspace build, format check, warning-denied all-target Clippy, and the complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests (293 unit, 188 commit, 76 transaction tests, plus all remaining targets). `git diff --check` passed; Cargo.lock is unchanged.

## DP-SQL-11: failed multi-privilege REVOKE is atomic across service processes — verified slice

- Extended the authenticated gateway/supervisor administrative lifecycle to issue a five-privilege REVOKE containing four existing object grants and an absent DELETE grant. The SQL operation fails, and metadata reads confirm all four existing grants remain; the following valid four-privilege REVOKE succeeds.
- Verification: focused production gateway/supervisor process regression, locked workspace build, format check, warning-denied all-target Clippy, `git diff --check`, and the full serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests.
- Remaining: broaden multi-privilege batch validation to provenance/epoch and additional mixed target scopes; broader permission-change race schedules and administrative SQL forms remain open.

## DP-SQL-11 / DP-8: column-target privilege batches update provenance and epoch once — verified slice

- Extended `object_grant_batch_racing_manage_grants_revocation_is_atomic` with a successful `SELECT` plus `UPDATE` batch targeting an explicit two-column subset on a table created through the supported schema/table DDL paths. The regression checks both family-12 grants and each grantee-to-column dependency, verifies no table-wide grant was created, and confirms exactly one grantee epoch increment, one sequence advance, and a committed outcome.
- `ObjectGrantTarget::Columns` now has real-redb acceptance alongside the existing `Object` and `AllColumns` variants. Cross-scope targets in one batch remain outside the request's common-target contract; broader SQL forms and repeated permission-change schedules remain open.
- Verification: the focused real-redb regression passed. Locked workspace build, format check, warning-denied all-target Clippy, and `git diff --check` passed. The complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests (294 unit, 192 commit, 77 transaction tests, plus all remaining targets).

## DP-3 / DD-3: same-name policy recreation preserves distinct catalog identity — verified slice

- Extended the real-redb policy lifecycle regression to drop a row policy and recreate the same table-local name. The new policy receives a distinct ID and expression ID; the current policy lookup contains the replacement and excludes the dropped policy, while the pinned pre-drop snapshot still resolves the original policy and expression. The replacement's expression dependency is checked in the current catalog.
- Verification: focused lifecycle regression, formatting, locked workspace build, warning-denied all-target Clippy, `git diff --check`, and the complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests.

## DP-SQL-11 / DP-8: partial column revocation preserves sibling grants — verified slice

- Extended `object_grant_batch_racing_manage_grants_revocation_is_atomic` to revoke SELECT from one column after a two-privilege, two-column grant batch. It verifies that the selected grant is removed, sibling UPDATE/SELECT grants and their original grantor provenance remain, the column authority dependency remains while UPDATE still exists, the grantee epoch advances once, and the operation has one committed outcome.
- Verification: the focused real-redb permission regression passed. Locked workspace build, format check, warning-denied all-target Clippy, and `git diff --check` passed. The complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests (294 library tests, 195 commit tests, 78 transaction tests, plus all other targets). Cargo.lock is unchanged.

## DP-SQL-11 / DP-8: mixed-scope object grant batches — verified slice

- Added storage acceptance for grant/revoke batches whose entries each carry their own target scope. Existing common-target batches delegate to the new path, retaining atomic authorization, provenance, epoch, dependency, and audit handling.
- Added authenticated ADMIN `objects.grant_entries` and `objects.revoke_entries` operations and a public client entry point for per-privilege targets. The protocol regression decodes an object-scoped ALTER plus column-scoped REFERENCES request; the real-redb race regression exercises grant and revoke publication, provenance, dependency cleanup, and one epoch update.
- Verification: focused protocol decoder regression and focused real-redb grant/revocation regression passed; locked engine check, warning-denied all-target engine Clippy, formatting, and `git diff --check` passed.
- Remaining: connect the new entries operation to SQL name binding/syntax; broaden administrative SQL forms and repeated permission-change race schedules.

## DP-SQL-11 / DP-8: mixed-scope grant SQL binding — verified slice

- Connected the mixed-scope ADMIN operation to SQL name binding. A table statement with one column-capable privilege and additional object privileges (for example, `GRANT ALTER, REFERENCES (employee_id) ON TABLE ...`) resolves the column by its catalog name and emits per-privilege targets. The data privilege uses the listed column(s), or all columns when no list is supplied; other privileges stay table-scoped.
- The bounded SQL parser now accepts this mixed form while continuing to reject multi-data-privilege column lists and non-data column targets.
- Verification: focused SQL parser test passed; warning-denied all-target engine Clippy, formatting, and `git diff --check` passed.

## DP-SQL-11 / DP-8: mixed-scope GRANT and REVOKE cross service processes — verified slice

- Extended `authenticated_metadata_and_admin_lifecycle_cross_gateway_and_supervisor_processes` to issue `GRANT ALTER, REFERENCES (key)` and its REVOKE through the authenticated ADMIN client. Permission discovery confirms ALTER is object-scoped and REFERENCES targets the resolved `key` column after grant, then confirms both entries are gone after revoke.
- Verification: the focused gateway/supervisor process test passed inside the delegated `systemd --user` service; formatting, warning-denied all-target engine Clippy, and `git diff --check` passed.

## DP-SQL-11: specify mixed-scope grant-list semantics — verified slice

- Updated the permission contract with the implemented grammar: one column-capable table privilege may carry the parenthesized column list while other compatible table privileges stay object-scoped in the same atomic operation. It also records all-column expansion without a list and the current single-data-privilege limit when a column list is present.
- Verification: checked the examples against the SQL parser and the authenticated cross-process GRANT/REVOKE regression; `git diff --check` passed.

## 2026-11-16 full delegated-cgroup workspace verification

- The complete serial workspace test suite passed inside the delegated `systemd --user` service with sandbox and process integration tests enabled: 955 passed, 0 failed, 0 ignored. This includes the current permission, authorization/schema race, and metadata discovery regressions. The run used the locked release profile and documented TinyCC/runtime artifacts.

## DP-3 / DD-5: row-policy lifecycle and dependency cleanup under catalog pressure — verified slice

- Repeated the authorized row-policy create/drop path across the 128-schema pressure campaign. The policy operation index retains each removed definition in the reader's pinned snapshot and omits it for a fresh snapshot; final persisted-catalog validation checks target and expression dependency cleanup.
- Verification: focused real-redb pressure test passed (8.08 s); warning-denied all-target engine Clippy, formatting, and `git diff --check` passed.
