# Authorization Progress

Migrated component-owned progress entries from [storage-manager progress](../storage-manager/PROGRESS.md); evidence is preserved below.

## AU-1 dependency: bounded policy composition — in progress

- Contract: [authorization](../../specs/authorization.md), Policy Evaluation. Added trusted typed expressions for field reads, principal/role context, comparisons, Boolean composition, and NULL checks. Work is bounded to 256 nodes and 32 nested levels; runtime errors fail closed. Authorization independently requires an invocation-scope capability and direct/inherited grant. Applicable ALLOWs are alternatives, every RESTRICT must pass, and an UPDATE ALLOW must pass both old/new predicates on that same policy.
- Red evidence: the new integration target first failed to compile because the authorization module did not exist. Implemented the evaluator and added truth-table, missing-grant/scope, role/principal, NULL, type-error, depth, and UPDATE-pairing cases.
- Verification: Rust 1.95 directly compiled and ran all 73 tests across 15 integration targets against the available redb 4.4.0 artifact. Direct `clippy-driver -D warnings` passed for the engine library and all 15 targets; rustfmt `--check` and `git diff --check` passed.
- Limits: callers currently supply trusted pinned roles, grants, policies, targets, and typed rows. Catalog-derived loading, authenticated admission, authorized cell projection/redaction, mutation integration, and policy dependency epoch wiring remain required before the evaluator can authorize production requests.

## AU-2 dependency: snapshot row projection and cell redaction — in progress

- Contract: [authorization](../../specs/authorization.md), Authorized View and Mutation Permissions. `InvocationTransaction::read_authorized` selects the row from the private overlay or pinned redb snapshot before decoding it with the pinned schema and invoking the policy projector. It requires schema USAGE and at least one relevant column grant; row policy denial makes the row absent; cell denial emits a `Redacted` enum variant with no value or length; readable NULL remains distinct. Runtime policy errors abort and persist ABORTED.
- A real-redb invocation fixture shows a principal-dependent variable-length field as VALUE for one user and REDACTED for another while returning the same permitted field; another real-redb test verifies an evaluation error becomes a durable terminal abort. Pure projection tests also cover row denial and NULL/value/redaction distinctions.
- Verification: all 73 tests across 15 integration files passed via direct Rust 1.95 compilation against redb 4.4.0. Direct Clippy with warnings denied passed for the engine library and every integration target; rustfmt and whitespace checks passed. Cargo workspace verification remains blocked by crates.io reachability.
- Limits: this is a trusted engine API fixture, not authenticated user provisioning, catalog-derived policy/grant loading, sanitized IPC serialization, or proof of hostile-client non-disclosure. Updates/inserts/deletes are not yet routed through the evaluator.

## AU-4 dependency: policy-checked field patch — in progress

- Contract: [authorization](../../specs/authorization.md), Mutation Permissions. `InvocationTransaction::stage_patch_authorized` reads the private/snapshot row, decodes the pinned tuple, computes the complete patched tuple, then requires row SELECT visibility, row UPDATE authority, and both SELECT and UPDATE authority on each changed cell. UPDATE policies evaluate old and proposed values in one policy; unchanged hidden fields are preserved. Patches that touch policy-driving columns from the pinned dependency set are denied.
- A real-redb integration test commits an authorized name change while preserving an inaccessible UUID/secret field and proves a policy-driving-field patch is rejected without changing the stored row.
- Verification: all 73 tests across 15 integration targets passed after this addition. Direct Clippy with warnings denied passed for the library and all integration targets; rustfmt and whitespace checks passed. Cargo remains unable to reach crates.io.
- Limits: schemas, grants, policies, and policy-driving column IDs are supplied as trusted inputs rather than loaded from authenticated catalog state. Insert/delete/defaults and policy dependency capture/revalidation remain incomplete.

## AU-5 dependency: insert/delete authorization — in progress

- `stage_insert_authorized` accepts a fully materialized typed row, checks table INSERT and each non-generated field's INSERT grant/policy, then stages it atomically; generated fields skip cell checks but remain subject to table creation policy. `stage_delete_authorized` checks row visibility through the authorized projector and then requires explicit table DELETE grant/policy; it does not demand readability for every field.
- Real-redb coverage denies missing field INSERT and table DELETE grants with durable ABORTED outcomes, then verifies generated-column insertion and authorized deletion. No row write is published on denied attempts.
- `stage_insert_authorized_with_defaults` now distinguishes explicit NULL from DEFAULT and materializes trusted literals or `CURRENT_PRINCIPAL_ID()` before row policies and CHECK constraints. An unsupported expression aborts with no row publication.
- Limits: generated UUID allocation/collision checks, catalog-derived defaults/grants/policies/context, full constraint loading, and DDL enforcement remain unimplemented. These methods are trusted engine APIs, not the user-facing SQL path.

## SM-4 / AU-5 dependency: INSERT default materialization — in progress

- Added an explicit `InsertInput::{Value, Default}` request form. Supported default expressions in this slice are typed literals (including NULL) and the admitted principal ID; unsupported expression shapes fail closed. Missing defaults materialize to NULL and tuple schema validation enforces nullability and types.
- Real-redb coverage confirms a current-principal default is visible to both the INSERT policy and CHECK expression before the row is committed. An unsupported field-dependent default yields a durable ABORTED outcome and publishes no row.
- Verification: all 79 tests across 15 integration targets passed with direct Rust 1.95 compilation against cached redb 4.4.0. Direct Clippy with warnings denied, rustfmt `--check`, and `git diff --check` passed.
- Remaining: schema-bound defaults still arrive from a trusted caller and must be tied to captured catalog definitions; generated UUID allocation, omitted/DEFAULT column mapping from SQL, DDL, and process integration remain incomplete.
- Next dependency: validate EIR type binding and declared column/object dependencies, then derive schema, grants, and policies from their pinned catalog records.

## AU-6 dependency: authorized catalog dependency guards — in progress

- `AuthorizedProjection` carries the exact catalog key/value set used for the supplied principal/roles, grants, policies, and schema. Before projection or mutation, `InvocationTransaction` verifies the context principal equals the admitted invocation principal, checks each dependency byte value (including absence) against its pinned redb snapshot, and adds it to the same catalog guard map used by final commit. A mismatch rejects the operation. At final commit, concurrent catalog changes produce conflict plus durable ABORTED, with no data publication.
- The real-redb race test changes a principal authority record after an authorized field patch; the pending writer conflicts and the old row remains intact. Catalog dependency values are validated as registered CAT1 records with matching logical keys.
- Limits: callers still provide dependency sets, so omitted grant/policy/membership/schema records cannot be detected. Role closure and permission resolution are not loaded from the catalog; token revocation, schema DDL fences, and authenticated admission remain incomplete.

## AU-1 dependency: bounded role closure — in progress

- `resolve_role_closure` computes the transitive member-to-role closure from pinned membership edges using an iterative bounded traversal. It rejects zero IDs, duplicate edges, reachable cycles, and more than 4096 edges. This produces the role set consumed by policy recipient matching without recursive stack growth.
- Integration fixtures cover nested inheritance, shared inherited roles, cycle rejection, duplicate rejection, and limit failure. Administrative commit-time validation of the complete membership graph is now implemented; CAT1 membership loading for runtime authorization remains outstanding.

## SM-4 / AU-5 dependency: row-local CHECK enforcement — in progress

- Added bounded evaluation of schema-bound CHECK expressions over complete proposed tuples in the authorized insert and patch paths. SQL TRUE and UNKNOWN pass; FALSE returns a generic constraint violation; malformed/type-invalid expressions fail closed and abort. These expressions are trusted inputs, and their defining catalog records belong in the existing guarded dependency set.
- Tests cover TRUE/FALSE/NULL semantics and real-redb insert and update attempts. Rejected attempts discard staged data and record ABORTED; valid writes still commit.
- A regression test exposed that the shared permission helper checked grants but omitted the invocation scope. Updated it so every grant lookup also requires the narrowed capability; this applies consistently to table and cell authorization.
- Verification: all 74 tests across 15 integration targets passed when directly compiled with Rust 1.95 against cached redb 4.4.0. Direct Clippy with warnings denied passed for the library and every integration target; rustfmt `--check` and `git diff --check` passed. `cargo test --workspace --locked` again stopped before compilation because crates.io `config.json` was unreachable.
- Limits: expressions are not loaded or bound from catalog definitions; low-level trusted mutation methods bypass this authorized check path. NOT NULL, primary/unique and foreign-key validation, DDL fences, defaults/materialization, and commit-time cross-row constraint checks remain outstanding.

## SM-4 / AU-5 / DD-1 dependency: schema-bound catalog expressions — in progress

- Contract: [catalog payloads](../../specs/catalog-payloads.md), EIR1 and expression dependency binding. Added schema-aware checks for result type IDs and parameters, operator operand types, field ordinals, role IDs, and exact stable dependency sets. Defaults are limited to root literals and the admitted principal ID; CHECK and policy results must be Boolean.
- Catalog-loaded CHECK expressions and INSERT defaults now pass through the binder before evaluation/materialization. The loader requires ordered column IDs, schema, and expected type IDs as trusted catalog-derived inputs; malformed or mismatched expressions abort and publish no row. Updated the real-redb INSERT fixture to bind its CHECK and DEFAULT definitions.
- Red evidence: the binding tests first failed to compile because the validator API did not exist. After implementation, focused expression tests and the full direct real-redb suite passed: 87 tests across 16 integration targets using Rust 1.95 and cached redb 4.4.0. Direct Clippy with warnings denied found one cloned-reference lint, which was fixed; final lint/format/whitespace checks are being rerun.
- Spec decision: EIR1 DECIMAL literals are i128 coefficients at scale zero. The spec now requires DDL and invocation binding before execution, exact declared-vs-discovered dependencies, SQL type compatibility rules, and the bounded default expression forms.
- Remaining: schema/type/column IDs still come from trusted callers rather than a catalog-derived table-schema loader. Policies are not loaded from catalog or bound through this invocation API. Full DDL, constraints, bootstrap, gateway/sandbox IPC, and bounded snapshot-reader lifecycle remain incomplete. Cargo dependency resolution is currently unavailable because crates.io cannot be reached.
- Next: complete pinned-snapshot catalog-derived authority and policy loading, DDL epoch enforcement, and the missing service boundary stages.

## AU-1 / SM-5 dependency: pinned catalog role closure — in progress

- `load_catalog_role_closure` now scans the bounded pinned membership family, validates membership payloads/epochs and user/role principal endpoints, and computes the transitive closure with the existing cycle/edge limits. It records exact guards for the membership and principal records it uses. Catalog-loaded authorization no longer needs a caller-supplied membership list for this behavior.
- A real-redb test loads a user-to-role-to-role chain, then updates one membership record; finalization of the old invocation detects the changed guard and durably conflicts. Existing pure tests continue to cover duplicate/cycle/limit behavior, while catalog commit tests enforce endpoint kinds and full-graph cycle integrity.
- Catalog commit now rejects a membership add/change/removal for an existing member unless the same transaction increases that member's principal authority epoch. This closes the unseen-edge case for snapshot role closure; a regression assertion rejects membership publication without the epoch advance.
- Remaining: grants and policy definitions/targets still need catalog loaders and bound expression execution. Catalog DDL is not yet a complete authenticated control channel.
- Verification for the combined schema/role loader and membership-epoch increment: 89 tests across 16 integration targets pass by direct Rust 1.95 compilation against redb 4.4.0; Clippy warnings-as-errors, rustfmt check, and whitespace check pass. Cargo workspace verification remains blocked before compilation by unreachable crates.io index configuration.

## AU-6 dependency: catalog-derived data grants — in progress

- `load_catalog_grants` now derives the admitted principal's role closure and resolves matching schema USAGE, table, and column data grants from pinned family-12 records. It validates scope/object/column relationships, grantor identity and authority epochs, and retains exact guards for matching grants plus referenced principal/column records. Non-query administrative privileges do not expand query authority.
- Extended the real-redb catalog fixture with direct schema USAGE, table SELECT, and column UPDATE grants and verifies all three resulting authorization entries. Catalog commit rejects a grant change for an existing grantee unless it advances that grantee's principal authority epoch in the same transaction.
- Remaining: catalog loaders still need complete privilege/target semantic validation and DDL authority checks. The authenticated administrative DDL path, control IPC, gateway/sandbox integration, and operational hardening remain open.

## AU-1 / AU-6 dependency: catalog-derived row and cell policies — in progress

- `load_catalog_policies` resolves applicable family-13 definitions and family-14 user/role/column targets from the pinned snapshot. It enforces row/cell target shape, operation clause requirements, recipient kinds, and schema column membership; it schema-binds USING/WITH CHECK expressions to the unique catalog BOOLEAN type and exact field/role dependency IDs. Policy, target, recipient, type, column, and expression records are guarded before the context is returned.
- Catalog commit now requires an existing policy's authority epoch to advance when its definition or targets change, and requires the owning existing table's write epoch to advance for any policy change. It rejects deleting a policy while target rows remain. A real-redb case checks a loaded row SELECT policy and rejects adding a target without the required epochs.
- A low-level catalog/audit test previously used an orphan policy-target row. It now uses a dependency record so the fixture remains valid under policy-target referential checks.
- The same real-redb fixture now also loads transitive role membership, a role-granted column INSERT privilege, and role policy recipients alongside direct user grants/targets.
- The catalog-loaded projection fixture combines a row ALLOW, cell ALLOW, and cell RESTRICT policy; `project_row` confirms the restricted cell stays redacted while the row remains visible. It also loads a transitive role, role-granted INSERT, and role policy recipient.
- Remaining: startup integrity validation, authenticated DDL/control IPC, gateway/sandbox integration, and SM-6/SM-7 acceptance remain open.
- Verification after the policy loader/epoch changes: all 89 tests across 16 integration targets pass via direct Rust 1.95 compilation against cached redb 4.4.0. Direct Clippy with warnings denied, rustfmt `--check`, and `git diff --check` pass. `cargo test --workspace --locked` is still blocked before compilation by the unreachable crates.io index configuration.

## SM-5 / AU-6: object-grant revocation conflicts with a staged writer — verified

- Contract: [authorization](../../specs/authorization.md), [concurrency/MVCC](../../specs/concurrency-mvcc.md), and [storage manager](../../specs/storage-manager.md). Finalization must revalidate the exact grant record used by an authorized write; revocation cannot leave a previously staged write committing under stale authority.
- Regression: `storage_manager_authorizes_object_grants_and_linearizes_revocation_races` stages an authorized row update with the persisted table UPDATE grant and races its commit against `commit_authorized_object_grant_ddl` revocation. It accepts only commit-before-revoke or revoke-before-abort, checks row bytes, durable outcomes, commit sequence, and final grant/dependency state. Ten fresh-process runs all observed commit-before-revoke; token and role-membership regressions separately verify revoke-before-stale-write aborts.
- Verification: the full Rust commit integration target passed (127 passed, 3 delegated-cgroup tests ignored); workspace formatting and warning-denied Clippy passed under Rust/Cargo 1.95.0.
- Remaining: repeat the grant race under schedules that exercise revoke-before-write, and cover other authority-changing DDL races.

## SM-5 / AU-6: role-membership revocation conflicts with a staged writer — verified

- Contract: a committed role-membership change advances the member's authority epoch and invalidates writes admitted through the prior role closure.
- Regression: `role_membership_revocation_aborts_a_staged_role_authorized_writer` admits a user with an inherited role grant, stages an authorized update, revokes the role membership through `StorageManager::set_role_membership`, and verifies the stale writer is durably ABORTED, the row remains unchanged, the revocation is COMMITTED, and the commit sequence advances only for the two successful commits.
- Implementation: no production change was needed; the existing pinned membership and authority-epoch guards reject the stale write.
- Verification: the focused regression passed. The Rust commit integration target passed with 126 tests and 3 delegated-cgroup tests ignored; `cargo fmt --all -- --check`, warning-denied workspace Clippy, and `cargo test --workspace --locked` passed in the prescribed Rust/Cargo 1.95.0 Nix shell.
- Remaining: equivalent stale-writer coverage for identity, credential, and other authority-changing DDL routes, plus repeated concurrent schedules, remains open.

## SM-5 / AU-6: token revocation discards a pending row write — verified

- Contract: a token-authenticated invocation must revalidate its exact credential record at finalization; revoking that token makes a pending invocation conflict before any staged row can publish.
- Regression: `token_admission_checks_expiry_secret_revocation_and_commit_guards` admits an ordinary user, stages a policy-authorized row update and its old/new index memberships, then revokes the token through `StorageManager::revoke_token` as a separate database administrator. It verifies the stale invocation is durably ABORTED, the original row and index entry remain, and the replacement index entry is absent. A second live admin credential keeps final-admin validation satisfied.
- Implementation: no production change was needed; token admission's pinned verifier guard rejects the stale finalization.
- Verification: the focused regression passed. `cargo fmt --all -- --check` and warning-denied workspace Clippy passed. The full Rust commit integration target passed serially with 126 tests and 3 delegated-cgroup tests ignored under the prescribed Rust/Cargo 1.95.0 Nix shell. Two earlier parallel full-target runs hit the existing intermittent `InvalidAudit` in `storage_manager_creates_and_drops_check_constraint`; that test passed alone and in the serial full-target run.
- Remaining: add stale-writer coverage for identity rename/drop and broader authority-changing DDL routes.

## SM-5 / AU-6: LOGIN disable conflicts with an authorized staged write — verified

- Contract: disabling a LOGIN user advances the principal authority epoch and revokes all active credentials atomically; an invocation admitted through a prior token and authority snapshot must not publish its pending mutation.
- Regression: `token_admission_checks_expiry_secret_revocation_and_commit_guards` admits a second token-authenticated writer, stages an authorized row patch and corresponding index replacement, disables the user through `StorageManager::set_user_login_enabled`, and verifies the writer is durably ABORTED, the row and old index membership remain, the replacement index membership is absent, new token admission fails, and the issued token is listed as revoked.
- Implementation: no production change was needed; the identity epoch and verifier guards reject the stale write after the serialized LOGIN transition.
- Verification: the focused regression passed. Rust formatting and warning-denied workspace Clippy passed; the full commit integration target passed serially with 126 tests and 3 delegated-cgroup tests ignored in the prescribed Rust/Cargo 1.95.0 Nix shell.
- Remaining: stale-writer coverage for identity rename/drop and other authority-changing DDL routes remains open.

## SM-5 / AU-6: identity rename conflicts with an authorized staged write — verified

- Contract: a token-authenticated invocation admitted under a pinned principal authority snapshot cannot publish after the corresponding identity is renamed through the authorized StorageManager path.
- Regression: `token_admission_checks_expiry_secret_revocation_and_commit_guards` stages an authorized row patch and index replacement, renames that user through `StorageManager::rename_identity` as a separate database administrator, and then verifies the stale writer conflicts and records an ABORTED outcome. The original row and index membership remain, and the replacement index membership is absent. LOGIN disable coverage in the same fixture now follows the rename and checks the resulting commit sequence.
- Implementation: no production change was needed; the existing identity epoch guard rejects the stale finalization.
- Verification: the focused regression passed. Under Rust/Cargo 1.95.0, the workspace build, formatting check, warning-denied Clippy, full workspace tests, and serial commit integration target passed. The commit target reported 126 passed and 3 delegated-cgroup tests ignored.
- Remaining: stale-writer coverage for identity drop and other authority-changing DDL routes remains open.

## SM-5 / AU-6: identity drop revokes token admission — verified

- Contract: dropping a user atomically removes its principal and object, records the permanent identity tombstone, and revokes its credentials; a still-unexpired token for the dropped user must fail new invocation admission without creating an invocation outcome.
- Regression: `identity_ddl_drops_unreferenced_user_and_revokes_tokens_atomically` now tries the target user's original token after `StorageManager::drop_identity` commits and verifies authentication fails and no outcome is recorded for the rejected invocation. The existing test also checks principal/object removal, tombstones, and persisted verifier presence.
- Implementation: no production change was needed; identity drop already commits token revocation atomically, and admission honors the verifier's revoked state.
- Verification: the focused real-redb test passed; `cargo fmt --all -- --check`, warning-denied workspace Clippy, and the serial commit integration target passed under Rust/Cargo 1.95.0. The commit target reported 126 passed and 3 delegated-cgroup tests ignored.
- Remaining: stale-writer races for other authority-changing DDL routes and broader repeated concurrency schedules remain open.

## SM-5 / AU-6: token revocation racing a staged write — verified

- Contract: token-authenticated row publication and token revocation serialize so a write committed first remains durable, while a writer finalized after revocation conflicts and records ABORTED; revocation always records its own committed outcome.
- Regression: `token_revocation_racing_a_staged_write_linearizes` starts a catalog-backed staged row write and `StorageManager::revoke_token` from a barrier, then validates the winning serialization, row state, durable outcomes, commit sequence, index/catalog integrity, and rejection of later admission. Twenty fresh-process repetitions all observed write-before-revoke; the existing `token_admission_checks_expiry_secret_revocation_and_commit_guards` regression covers revoke-before-stale-write and durable abort.
- Implementation: no production change was needed; token verifier guards and the serialized redb commit boundary preserve the allowed outcomes.
- Verification: the focused race passed in twenty fresh processes; `cargo fmt --all -- --check`, warning-denied workspace Clippy, and the serial commit integration target passed under Rust/Cargo 1.95.0. The commit target reported 127 passed and 3 delegated-cgroup tests ignored.
- Remaining: repeated race coverage for other authority-changing DDL routes and broader concurrent schedules remains open.
