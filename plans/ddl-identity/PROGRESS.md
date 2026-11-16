# Identity DDL Progress

Migrated component-owned progress entries from [storage-manager progress](../storage-manager/PROGRESS.md); evidence is preserved below.

## SM-5 / DDL-identity dependency: prepared CREATE USER ADMIN operation

- Behavior: `users.create` uses the connection-owned PREPARE/START flow. START creates a random stable user ID, atomically publishes its LOGIN principal and matching named object, records DDL audit/outcome, and returns the user ID. The user receives no data privileges or credential; `tokens.issue` provisions login separately.
- Contract and implementation: [identity DDL protocol](../../specs/client-protocol.md), [storage-manager identity lifecycle](../../engine/src/storage_manager.rs), and [ADMIN session handling](../../engine/src/client_protocol.rs).
- Regression evidence: decoder tests accept canonical names and reject uppercase names. A real-redb `MANAGE_USERS` session creates the user, issues a token, authenticates a new query session with it, and confirms token inventory is attached to the created user.
- Verification: format check, workspace build, warning-denied Clippy, all workspace tests, and `git diff --check` passed in the configured Nix shell.
- Remaining at that increment: role creation is recorded below; other user lifecycle DDL, remaining ADMIN operations, and separate supervisor/worker process integration remain open.

## SM-5 / DDL-identity dependency: prepared CREATE ROLE ADMIN operation

- Behavior: `roles.create` uses connection-owned PREPARE/START. Storage checks `MANAGE_ROLES`/database-administrator authority and atomically writes a generated non-login role principal and matching named object with DDL audit/outcome. Roles start without memberships or privileges.
- Contract and implementation: [identity DDL protocol](../../specs/client-protocol.md), [shared storage principal creation](../../engine/src/storage_manager.rs), and [ADMIN session handling](../../engine/src/client_protocol.rs).
- Regression evidence: decoder accepts normalized role names. A real-redb database-administrator session creates the role, and the test reads its persisted principal record to verify role kind and disabled LOGIN state.
- Verification: format check, workspace build, warning-denied Clippy, all workspace tests, and `git diff --check` passed in the configured Nix shell.
- Remaining at that increment: role membership DDL is recorded below; other user lifecycle DDL, remaining ADMIN operations, and separate supervisor/worker process integration remain open.

## SM-5 / DDL-identity dependency: role membership ADMIN operations

- Behavior: `roles.grant` and `roles.revoke` use the connection-owned PREPARE/START flow. Storage atomically changes the membership and dependency records, increments the member authority epoch, and delegates authorization, endpoint validation, cycle checks, and last-administrator protection to the established membership DDL commit path.
- Contract and implementation: [membership protocol](../../specs/client-protocol.md), [storage membership lifecycle](../../engine/src/storage_manager.rs), and [ADMIN session handling](../../engine/src/client_protocol.rs).
- Regression evidence: decoder tests cover grant and revoke schemas. A real-redb ADMIN lifecycle grants and revokes a newly created role for a new user, then authenticates again to confirm role closure changes are visible.
- Verification: format check, workspace build, warning-denied Clippy, all workspace tests, and `git diff --check` passed in the configured Nix shell.
- Remaining: other user lifecycle DDL, privilege grant/revoke ADMIN operations, remaining ADMIN operations, and separate supervisor/worker process integration.

## SM-5 / DDL-identity dependency: system privilege ADMIN operations

- Behavior: `privileges.grant` and `privileges.revoke` use connection-owned PREPARE/START. Storage atomically changes the database-scoped grant and dependency edge, increments the grantee's authority epoch, and delegates authorization to the current database-administrator check.
- Contract and implementation: [system privilege ADMIN protocol](../../specs/client-protocol.md), [storage-manager contract](../../specs/storage-manager.md), [storage lifecycle](../../engine/src/storage_manager.rs), and [ADMIN dispatch](../../engine/src/client_protocol.rs).
- Regression evidence: decoder coverage checks grant/revoke schemas. The real-redb ADMIN test grants `MANAGE_ROLES` to a newly created user and verifies that the user's token-backed session can then create a role.
- Focused verification: formatting and the client-protocol decoder and real-redb ADMIN lifecycle tests passed in the configured Nix shell. Workspace checks remain to run.
- Remaining: user rename/login-state/drop ADMIN operations, remaining ADMIN operations, and separate supervisor/worker process integration.

## SM-5 / SDK-5: role membership CLI across gateway and supervisor processes — verified

- Contract: `roles.grant` and `roles.revoke` are authenticated, audited ADMIN mutations, and a role referenced by membership cannot be dropped until the membership is revoked.
- Regression: `authenticated_metadata_and_admin_lifecycle_cross_gateway_and_supervisor_processes` grants `MANAGE_ROLES` to a role, assigns that role to a token-bearing user through `prdb-client role-grant`, and confirms the user can create a role through the same gateway/supervisor path. The role cannot be dropped while membership references it. After `prdb-client role-revoke`, the user's token still authenticates but `role-create` is rejected; the role's system privilege is then revoked, and the identities are cleaned up.
- Verification: the focused real-process test passed through the separate `prdb-supervisor` and `prdb-gateway` binaries.
- Remaining: other identity/control ADMIN families, wider concurrent membership race coverage, and confined query execution remain open.

## SM-5 / DDL-identity dependency: user and role rename ADMIN operations

- Behavior: `users.rename` and `roles.rename` use connection-owned PREPARE/START. Storage updates the principal name and authority epoch with the matching named object and object version in one authorized DDL commit.
- Contract and implementation: [identity ADMIN protocol](../../specs/client-protocol.md), [storage-manager contract](../../specs/storage-manager.md), [identity lifecycle](../../engine/src/storage_manager.rs), and [ADMIN dispatch](../../engine/src/client_protocol.rs).
- Regression evidence: strict decoder tests cover both operation schemas. The real-redb identity lifecycle test exercises `StorageManager::rename_identity` under `MANAGE_USERS`, alongside the authorization path's existing rename validation.
- Remaining: login-state and DROP ADMIN operations, remaining ADMIN operations, and separate supervisor/worker process integration.

## SM-5 / DDL-identity dependency: user LOGIN state ADMIN operation

- Behavior: `users.login` uses connection-owned PREPARE/START and changes the LOGIN flag and authority epoch. Disabling LOGIN revokes every outstanding token in the same commit; enabling LOGIN does not restore tokens.
- Contract and implementation: [login-state protocol](../../specs/client-protocol.md), [storage-manager contract](../../specs/storage-manager.md), [identity lifecycle](../../engine/src/storage_manager.rs), and [ADMIN dispatch](../../engine/src/client_protocol.rs).
- Regression evidence: decoder coverage checks the strict boolean-string schema. A real-redb identity lifecycle regression verifies that disabling a user revokes its active token atomically, and that a change cannot remove the last login-enabled administrator.
- Remaining: DROP ADMIN operations, remaining ADMIN operations, and separate supervisor/worker process integration.

## SM-5 / DDL-identity dependency: user and role DROP ADMIN operations

- Behavior: `users.drop` and `roles.drop` use connection-owned PREPARE/START and delegate RESTRICT validation to the authorized identity commit path. User drops revoke outstanding credentials, delete the principal/object pair, and add a permanent tombstone atomically.
- Contract and implementation: [DROP ADMIN protocol](../../specs/client-protocol.md), [storage-manager contract](../../specs/storage-manager.md), [identity lifecycle](../../engine/src/storage_manager.rs), and [ADMIN dispatch](../../engine/src/client_protocol.rs).
- Regression evidence: decoder coverage checks both strict operation schemas. Real-redb tests exercise successful user/role drops, user credential revocation and tombstones, plus rejection of a drop while a privilege grant references the user.
- Remaining: remaining ADMIN operations and separate supervisor/worker process integration.

## SM-5 / SDK-5: user lifecycle CLI across gateway and supervisor processes — verified

- Contract: user rename, login-state changes, and drop are authenticated ADMIN mutations; disabling login and dropping a user revoke all outstanding tokens atomically.
- Regression: `authenticated_metadata_and_admin_lifecycle_cross_gateway_and_supervisor_processes` renames a token-bearing user, disables login through `prdb-client`, verifies its token fails authentication, re-enables login and verifies the old token remains revoked, issues a replacement token, then drops the user through the CLI and verifies the replacement token fails authentication.
- Verification: the focused real-process test passed with separate gateway and supervisor binaries.
- Remaining: process coverage of further administrative families, control-path stress, and confined query execution remain open.

## ID-3 / NP-6: reject schema DDL from a regular token across service processes — verified

- Contract: [identity DDL](../../specs/ddl-identity.md), [schema DDL](../../specs/ddl-schema.md), and [client protocol](../../specs/client-protocol.md). Authentication establishes a principal but does not grant database-administrator or `CREATE_SCHEMA` authority.
- Regression: the gateway/supervisor process test creates a regular user and issues a token through ADMIN, authenticates a second control connection with that token, and verifies `schemas.create` returns an administrative rejection. The administrator grants `CREATE_SCHEMA`; the user reconnects, creates and drops an owned schema, then the administrator revokes the grant and drops the user, revoking its token. Finally the administrator creates the initially denied schema name, showing the rejected request left no catalog state.
- Verification: the focused separate-process regression passed inside the delegated user service; full workspace, formatting, build, Clippy, and TinyCC acceptance results are recorded with the owning schema DDL increment.
- Scope: checks identity-based authorization and denial side effects for schema creation across production processes. Other DDL privilege boundaries and concurrent revocation races remain open.

## ID-5 / NP-6: revoked system privilege takes effect on an open ADMIN connection — verified

- Contract: [identity DDL](../../specs/ddl-identity.md) and [client protocol](../../specs/client-protocol.md) require control operations to revalidate the authenticated token and current authority; a revocation must not leave the existing connection with stale `CREATE_SCHEMA` authority.
- Regression: the gateway/supervisor process test keeps the regular user's authenticated `AdminClient` open while the administrator revokes `CREATE_SCHEMA`. The same connection's next schema-create request is rejected, and the administrator then creates that schema name to prove the denied request left no catalog state before dropping the user.
- Verification: the focused `supervisor_and_gateway_binaries_commit_a_confined_submission` test passed in the delegated `systemd --user` service with the configured runtime and SDK release paths. `cargo fmt --all -- --check`, workspace build, warning-denied Clippy, `git diff --check`, and the full serial workspace suite passed; all sandbox integration tests were enabled and zero tests were ignored.
- Scope: confirms a privilege revocation affects the next ADMIN request on an already authenticated control connection. Other system/object privilege revocation cases and concurrent revocation races remain open.

## SM-5 / AU-6: identity drop conflicts with a staged role-authorized write — verified

- Contract: [identity DDL](../../specs/ddl-identity.md), [authorization](../../specs/authorization.md), and [concurrency/MVCC](../../specs/concurrency-mvcc.md). A token-authenticated invocation must not publish a staged row mutation after its identity is dropped.
- Regression: `identity_drop_aborts_a_role_authorized_staged_update` creates the login and role through `StorageManager`, admits the user by token, reads and stages an update under role grants and row/cell policies, then revokes role membership and drops the identity. Finalization conflicts with a durable ABORTED outcome, the prior row remains, and subsequent token admission fails without an invocation outcome.
- Verification: focused real-redb test passed. Locked workspace build, rustfmt check, warning-denied all-target Clippy, and the full serial delegated-cgroup workspace suite passed under Rust/Cargo 1.95.0 in the pinned Nix shell. The suite reported 233 engine unit tests, 173 commit integration tests, 75 transaction integration tests, all remaining workspace targets green, and zero ignored tests.
- Scope: verifies stale row-write rejection and token revocation for `StorageManager::drop_identity`; the separate gateway/supervisor CLI path already has identity-drop coverage, while repeated identity-drop race schedules remain open.

## ID-SQL-1: bounded CREATE USER/ROLE administrative SQL — implemented

- Contract: [identity DDL](../../specs/ddl-identity.md), [client SDK](../../specs/client-sdk.md), and [admin-sql](../../specs/client-sql-command.md). The bounded SQL subset accepts `CREATE USER name LOGIN|NOLOGIN` and `CREATE ROLE name` and dispatches existing authenticated typed operations. User creation issues no credential; token issuance remains separate.
- Regression: parser cases reject missing login mode, role login modifiers, malformed qualification, and multiple statements. A real-redb regression confirms NOLOGIN users cannot receive tokens; the delegated cross-process metadata/admin lifecycle now creates a NOLOGIN user over gateway/supervisor ADMIN and verifies token issuance fails. Authenticated TCP coverage verifies both operation names, identity names, and returned IDs; CLI coverage verifies syntax recognition before credentials/network. Existing typed identity lifecycle regressions remain relevant; no stale tests were identified.
- Validation: focused parser, authenticated AdminClient, and CLI tests passed. Locked workspace build, format check, warning-denied all-target Clippy, maintained TinyCC SDK tests/fixture, full serial delegated-cgroup workspace suite (sandbox/process tests enabled; zero ignored; 257 library tests), and `git diff --check` passed.
- Follow-up: add rename, restricted drop, and role membership SQL in bounded increments with exact identity-name resolution and typed authorization preserved.

## ID-SQL-2: bounded ALTER USER/ROLE RENAME SQL — implemented

- Behavior: `ALTER USER|ROLE old_name RENAME TO new_name` performs a single authenticated indexed name lookup and dispatches the existing typed rename operation by stable identity ID. The resolver requires the matching `MANAGE_USERS`/`MANAGE_ROLES` privilege or database-admin membership and returns the same null result for hidden and absent names; the mutation independently rechecks authorization.
- Contract and implementation: [identity DDL](../../specs/ddl-identity.md), [client SQL command](../../specs/client-sql-command.md), [name lookup transaction](../../engine/src/transaction.rs), [authenticated storage entry point](../../engine/src/storage_manager.rs), and [ADMIN protocol/CLI](../../engine/src/client_protocol.rs).
- Regression evidence: bounded parser cases cover users, roles, and malformed/multi-statement forms; protocol decoder tests enforce exact kind/name arguments; CLI recognition verifies valid rename SQL is accepted before credential/network access. The delegated `authenticated_metadata_and_admin_lifecycle_cross_gateway_and_supervisor_processes` test now performs the SQL rename against the production gateway and supervisor.
- Verification: locked workspace build, format check, warning-denied all-target Clippy, maintained TinyCC SDK tests and sandbox fixture, the focused delegated process test, and full serial delegated-cgroup workspace suite passed. The suite enabled sandbox/process integration tests, reported 258 library tests, and ignored zero tests; `git diff --check` passed. Cargo.lock is unchanged.
- Scope: SQL rename resolves one name without enumeration and delegates the mutation to existing identity DDL. DROP RESTRICT, login-state, and membership SQL remain follow-up work. No stale tests were identified; the regression suite was retained.

## ID-SQL-3: bounded DROP USER/ROLE RESTRICT SQL — implemented

- Behavior: `DROP USER|ROLE name RESTRICT` resolves one exact authenticated identity name, then submits the existing typed drop operation. Storage retains dependency validation, credential revocation, tombstones, and authorization checks.
- Regression evidence: parser tests reject missing/unsupported drop modes and multiple statements. The cross-process identity lifecycle now renames and drops the user through `admin-sql` by name, then verifies its credential no longer authenticates.
- Verification: the parser regression and full cross-process lifecycle test passed; the latter uses SQL rename and drop through the gateway/supervisor and confirms token revocation after drop. Workspace build, format check, warning-denied all-target Clippy, maintained TinyCC SDK tests and sandbox fixture, full serial delegated-cgroup workspace suite, and `git diff --check` passed. The suite enabled sandbox/process tests, reported 259 library tests, and ignored zero tests. One earlier full run had an unrelated timing failure in a gateway reaper unit test; a focused rerun passed, and the final full run passed with the documented runtime paths. Cargo.lock is unchanged.
- Scope: SQL drop preserves the typed operation's RESTRICT semantics and tombstone behavior. LOGIN state and membership SQL remain follow-up work. No stale tests were identified or deleted; regression coverage remains active.

## ID-SQL-4: bounded ALTER USER LOGIN/NOLOGIN SQL — implemented

- Behavior: `ALTER USER name LOGIN|NOLOGIN` resolves one authenticated user name and dispatches the existing `users.login` operation. Storage atomically revokes tokens when disabling LOGIN, and enabling LOGIN does not restore them.
- Regression evidence: parser cases cover LOGIN, NOLOGIN, wrong identity kind, and multiple statements. The delegated gateway/supervisor identity lifecycle now performs NOLOGIN and LOGIN changes through `admin-sql` and confirms revoked credentials remain invalid.
- Verification: parser and delegated gateway/supervisor lifecycle regressions passed. Workspace build, format check, warning-denied all-target Clippy, maintained TinyCC SDK tests and sandbox fixture, full serial delegated-cgroup workspace suite, and `git diff --check` passed. The suite enabled sandbox/process tests, reported 260 library tests, and ignored zero tests. Cargo.lock is unchanged.
- Scope: SQL LOGIN changes reuse the storage mutation that revokes all outstanding credentials atomically when disabling login; enabling does not revalidate old credentials. Role membership SQL remains the next planned identity increment.

## ID-SQL-5: bounded role membership SQL — implemented

- Behavior: `GRANT role TO USER|ROLE member` and `REVOKE role FROM USER|ROLE member` resolve both names through authenticated metadata and invoke existing typed membership DDL. Storage remains responsible for privilege checks, endpoint kinds, cycle detection, and administrator invariants.
- Regression evidence: parser cases reject missing member kinds, wrong grammar, ADMIN OPTION, and multiple statements. The delegated gateway/supervisor lifecycle now grants and revokes membership through `admin-sql`, verifying inherited `MANAGE_ROLES` access and its removal.
- Verification: parser and delegated gateway/supervisor lifecycle regressions passed. Workspace build, format check, warning-denied all-target Clippy, maintained TinyCC SDK tests and sandbox fixture, full serial delegated-cgroup workspace suite, and `git diff --check` passed. The suite enabled sandbox/process tests, reported 261 library tests, and ignored zero tests. Cargo.lock is unchanged.

## ID-SQL-6: bounded system privilege SQL — implemented

- Behavior: `GRANT|REVOKE MANAGE_USERS|MANAGE_ROLES|CREATE_SCHEMA TO|FROM USER|ROLE name` resolves the grantee through authenticated metadata and dispatches the existing database-scoped typed privilege mutation. Storage retains database-admin authorization.
- Regression evidence: parser cases reject unsupported privileges, malformed direction, ADMIN OPTION, and multiple statements. The delegated process lifecycle grants and revokes `MANAGE_ROLES` using SQL, then verifies inherited role authorization.
- Verification: parser and delegated gateway/supervisor lifecycle regressions passed. Workspace build, format check, warning-denied all-target Clippy, maintained TinyCC SDK tests and sandbox fixture, full serial delegated-cgroup workspace suite, and `git diff --check` passed. The suite enabled sandbox/process tests, reported 262 library tests, and ignored zero tests. One run hit the unrelated intermittent gateway reaper timing test; the focused test passed and a full retry passed. Cargo.lock is unchanged.

## ID-SQL-7: conditional identity drops — implemented slice

- Added `DROP USER|ROLE IF EXISTS name RESTRICT`. Present identities use the existing typed drop mutation and preserve management authorization and RESTRICT dependencies. An unresolved identity is suppressed only after the authenticated runtime-list read confirms database-administrator authority; the no-op creates no DDL outcome.
- Updated identity and client SQL contracts. Workspace build, formatting, warning-denied Clippy, and `git diff --check` pass. No tests were added or run; Cargo.lock is unchanged.

## ID-SQL-8: conditional identity creation SQL — verified

- Added `CREATE USER|ROLE IF NOT EXISTS`. Existing requested-kind identities are a no-op only after database-administrator authority is confirmed; an existing user's LOGIN state is not reconciled. Absent identities use existing typed creation operations.
- Parser and delegated gateway/supervisor process regressions pass, including repeated role creation and preserving a NOLOGIN user. Locked workspace build, format check, warning-denied all-target Clippy, and the complete serial delegated-cgroup workspace suite pass (sandbox/process tests enabled; zero ignored). Component SQL contracts are updated; Cargo.lock is unchanged.

## ID-SQL-7: conditional identity drop lifecycle — verified

- The authenticated gateway/supervisor lifecycle now covers `DROP USER|ROLE IF EXISTS` for absent and existing identities. Missing targets are a no-op, existing targets use the typed RESTRICT drop, and a repeated drop remains a no-op. The check reconnects after a deliberately failed token issuance so it exercises a fresh ADMIN session.
- Verification: `nix develop --no-update-lock-file --command cargo test -p prdb-engine --test metadata_process authenticated_metadata_and_admin_lifecycle_cross_gateway_and_supervisor_processes -- --exact --nocapture` passed (1 test). Workspace-wide checks were not rerun for this test-only increment.

## 2026-11 delegated acceptance checkpoint

- The complete serial delegated-cgroup workspace suite passed with sandbox and process tests enabled and zero ignored tests. The authenticated gateway/supervisor lifecycle now also covers repeated `DROP USER|ROLE IF EXISTS` for absent and present targets; see ID-SQL-7 above.
- Broader identity-authority race schedules remain open.

## 2026-11-16 delegated integration refresh

- The complete serial delegated-cgroup workspace suite passed with sandbox and process tests enabled and zero ignored tests: 291 engine unit tests, 179 commit integration tests, 75 transaction tests, and all three metadata process tests. Identity coverage includes last-login-admin protection, token lifecycle, stale-write invalidation, and membership revocation races.
- Broader identity-authority race schedules remain open.

## ID-3 / ID-5: identity drop racing a role-authorized staged write — verified slice

- `identity_drop_aborts_a_role_authorized_staged_update` now races the stale writer's final commit against membership revocation followed by identity drop. The regression accepts either serialized order: a writer that publishes first has a committed outcome before revocation; a writer that loses has a conflict/ABORTED outcome and its row remains unchanged. Membership revocation and identity drop both commit in order, and the dropped token cannot start a new invocation.
- Verification: focused real-redb race passed; formatting, warning-denied all-target Clippy, and `git diff --check` passed. The complete serial delegated-cgroup workspace suite passed afterward with zero ignored tests: 291 engine unit tests, 179 commit tests, 75 transaction tests, and all three metadata process tests.
- Remaining: exercise additional interleavings across rename, role-management revocation, token admission, and pending writes.

## 2026-11-16 integrated acceptance refresh

- The complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests. Identity coverage includes last credentialed-admin protection, identity-drop write invalidation, token lifecycle, role-membership races, and the authenticated gateway/supervisor lifecycle.
- Broader identity-authority race schedules remain open.

## ID-3 / ID-5: identity rename racing a token-authenticated staged write — verified slice

- `token_admission_checks_expiry_secret_revocation_and_commit_guards` now starts the identity rename and staged writer commit behind a shared barrier. Either serialization is accepted: a writer that commits first publishes before the rename, while a writer that loses records a conflict and leaves the row and index unchanged. The test checks the durable outcome status, commit ordering, row payload, and active index membership for both orders; subsequent LOGIN-disable checks derive their starting row/index state from the race result.
- Verification: focused real-redb regression passed. Workspace build, format check, warning-denied all-target Clippy, and `git diff --check` passed. The complete serial delegated-cgroup workspace suite passed afterward with sandbox and process tests enabled and zero ignored tests: 291 engine unit tests, 179 commit tests, and all remaining workspace targets.
- Remaining: exercise additional interleavings across role-management revocation, token admission, and pending writes.

## ID-2 / ID-5: NOLOGIN racing a token-authenticated staged write — verified slice

- `token_admission_checks_expiry_secret_revocation_and_commit_guards` now starts a staged token-authenticated update and `set_user_login_enabled(false)` behind a barrier. If the write commits first, its sequence precedes the atomic LOGIN disable; if LOGIN disable wins, the stale write conflicts and records ABORTED. The test verifies the resulting row, index membership, durable outcome, commit frontier, token revocation, and failed post-disable admission.
- Verification: focused real-redb regression passed. Workspace build, format check, warning-denied all-target Clippy, and `git diff --check` passed. The complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests: 291 engine unit tests, 179 commit tests, 75 transaction tests, and all remaining targets.
- Remaining: exercise further role-membership, token-admission, rename, and pending-write interleavings.

## ID-3 / ID-5 / AU-6: role revocation races an authorized staged writer — verified slice

- Strengthened `role_membership_revocation_aborts_a_staged_role_authorized_writer` to race the staged writer's final commit against membership revocation. Writer-first commits before revocation; revoke-first makes the stale write conflict and durably abort. The test checks final row contents, both durable outcomes, and the order-dependent commit sequence.
- Verification: focused real-redb regression, locked workspace build, format check, and warning-denied all-target Clippy passed. The full serial delegated-cgroup workspace suite passed with sandbox/process tests enabled: 291 engine unit tests, 180 commit tests, 75 transaction tests, and all remaining workspace targets.

## ID-3 / ID-6 / DP-8: membership grant races MANAGE_ROLES revocation — verified slice

- Extended the membership authorization regression to race a role membership grant with an administrator revoking the actor's `MANAGE_ROLES` privilege. Revocation always commits. If membership commits first, its sequence precedes revocation and the edge/dependency remain; if revocation wins, membership records ABORTED and no edge is published.
- Verification: focused real-redb regression, locked workspace build, format check, warning-denied all-target Clippy, and `git diff --check` passed. The complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests: 291 unit tests, 183 commit tests, 75 transaction tests, and all remaining targets.

## ID-4 / ID-5: token issue races LOGIN disable — verified slice

- Extended the token lifecycle regression to race credential issuance with `NOLOGIN` for the same user. It checks both durable outcomes and commit order, and verifies that any committed LOGIN disable leaves no active token, including a token issued during the race. A failed disable leaves the user LOGIN-enabled; a failed issue leaves no issued token.
- Verification: focused real-redb regression, locked workspace build, format check, and warning-denied all-target Clippy passed. The complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests: 291 unit tests, 183 commit tests, 75 transaction tests, and all remaining targets.

## ID-5 / ID-7: token revocation races a staged authorized write — verified slice

- `token_admission_checks_expiry_secret_revocation_and_commit_guards` now starts token revocation and the admitted token's staged authorized write behind a barrier. If the write commits first, its sequence precedes revocation; if revocation wins, the writer conflicts. The durable invocation outcome matches the result, the row remains consistent, and the revoked credential cannot admit another invocation.
- Verification: focused real-redb regression, locked workspace build, format check, warning-denied all-target Clippy, and the complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests: 291 unit tests, 183 commit tests, 75 transaction tests, and all remaining targets.
