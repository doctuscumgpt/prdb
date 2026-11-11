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
- Regression: `authenticated_metadata_and_admin_lifecycle_cross_gateway_and_supervisor_processes` creates two roles, grants one membership through `prdb-client role-grant`, verifies the referenced role drop is rejected, revokes through `prdb-client role-revoke`, reconnects after the rejected START, and then successfully drops both roles.
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
