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

## AU-2 / WI-2: catalog-backed principals receive different executor cell views — verified

- Contract: [authorization](../../specs/authorization.md) requires row visibility and cell readability to be checked separately; an authorized row with a denied field carries `REDACTED`, without exposing the stored value.
- Red evidence: the first fixture revision omitted the restricted principal's row SELECT ALLOW policy, and the executor scan returned zero rows instead of the expected visible rows. Adding row ALLOW still left the fixture incomplete until the restricted principal also received an explicit SELECT grant on the projected column.
- Regression: `catalog_authorized_scan_builds_its_projection_from_pinned_records` scans the same committed Bytea rows as two catalog principals through executor IPC. The first principal has row and cell SELECT ALLOW policies and receives the stored bytes. The second has schema USAGE, table and column SELECT, and row SELECT ALLOW but no cell SELECT ALLOW; every returned cell is `REDACTED`.
- Verification: the focused transaction integration test passed; workspace build, formatting check, warning-denied workspace Clippy, and the serial delegated-cgroup workspace test suite all passed in the pinned Rust/Cargo 1.95.0 Nix shell. The suite's syscall-kill fixture emitted its expected child-process core-dump record; the systemd test unit exited successfully.
- Scope: proves different cell views through catalog-derived authorization and executor IPC using trusted identity fixtures. Token-authenticated process admission, hostile direct IPC bypass attempts, and the broader two-user sandbox acceptance remain open.

## AU-2 / WI-2: token-authenticated DDL-created principals receive different cell views — verified

- Contract: [authorization](../../specs/authorization.md) requires a token-admitted identity, catalog grants, row visibility, and independent per-cell readability to compose before executor IPC returns a value.
- Red evidence: removing the restricted principal's row SELECT ALLOW policy made the executor scan return zero rows; the expected contract was a visible row whose denied cell is `REDACTED`.
- Regression: `token_authenticated_catalog_scans_redact_cells_per_principal` bootstraps an administrator, then creates two LOGIN users, issues tokens, creates a schema and table, grants schema/table/column SELECT, and creates separate row ALLOW policies plus a cell ALLOW for only one user through two-phase ADMIN requests served by `serve_gateway_client_session` over framed Unix streams. Both token-authenticated invocations scan the same committed variable-length Bytea row through executor IPC; one receives the bytes and the other receives only `Redacted`.
- Verification: the focused test passed. `cargo fmt --all -- --check`, locked workspace build, warning-denied workspace Clippy, maintained TinyCC SDK tests, sandbox-query fixture build, and the serial delegated-cgroup workspace test suite passed under Rust/Cargo 1.95.0. The commit integration target reported 146 passed and zero ignored.
- Control service regression: `gateway_service_listener_authenticates_control_admin_over_socket` connects to the bound gateway Unix socket, sends control HELLO and token AUTH, and creates a user, schema, table, and cell SELECT policy through ADMIN PREPARE/START. The focused test passed.
- Forgery regression: `executor_resolve_relation_ipc_dispatch_returns_only_authorized_handle` now alters a valid resolved-relation token and attempts OPEN SCAN over executor IPC; storage rejects the forged handle with `InvalidMetadataRequest`. The focused test passed.
- Confined hostile-client regression: `sandboxed_direct_executor_ipc_rejects_forged_relation_handle` loads a TinyCC-built query in the sandbox, calls the executor callback directly instead of the SDK, and submits OPEN SCAN and STAGE INSERT with a valid-shaped relation token that was never issued. The focused delegated-cgroup test verifies both are rejected, no result frame is produced, and storage records ABORTED.
- Forged row-mutation regression: `sandboxed_direct_executor_ipc_rejects_forged_row_handle_mutation` submits STAGE DELETE and STAGE UPDATE directly through the callback with a valid-shaped row token that was never issued; the focused delegated-cgroup test verifies both are rejected and the invocation is durably ABORTED.
- Handle-lifetime regression: the real-redb executor scan test now verifies that closing a relation invalidates its in-flight scan, its inserted-row handle, and the relation handle itself. Existing checks cover connection/invocation scoping and row-handle expiration after successful update/delete. The documented implementation has no cross-invocation authorization-decision cache.
- Verification: focused forged-relation and forged-row sandbox tests passed under delegated cgroup v2. Locked workspace build, rustfmt check, warning-denied Clippy, and TinyCC sandbox fixture build passed. The full serial delegated-cgroup workspace suite passed: 148 commit integration tests and 74 transaction integration tests, with all other workspace targets green.
- Hidden-size comparison: the token-authenticated cell-view test now scans two rows whose authorized secret values differ from 1 to 2048 bytes. The authorized reader sees both lengths while the restricted reader receives the same REDACTED cell representation for each row; no hidden bytes or lengths appear in sanitized cells.
- Verification for the hidden-size comparison: the focused token-authenticated ADMIN/gateway/executor test passed. Locked workspace build, rustfmt check, warning-denied Clippy, and sandbox-query fixture build passed; the full serial delegated-cgroup workspace suite passed with 148 commit integration tests and 74 transaction integration tests.
- Diagnostic comparison: `executor_resolve_relation_ipc_dispatch_returns_only_authorized_handle` now asserts byte-identical executor responses for an existing but hidden relation and an absent relation name in the same visible schema. Optimizer binding separately maps absent and mismatched relations to the same `RelationUnavailable` error.
- Verification for the diagnostic comparison: the focused real-redb executor IPC test passed. Locked workspace build, rustfmt check, warning-denied Clippy, and the full serial delegated-cgroup workspace suite passed; the commit and transaction targets reported 148 and 74 passing tests.
- Scope: covers token verification, control-created two-user grants and policies, catalog-derived authorization, executor IPC dispatch, bound gateway socket ADMIN creation, and confined direct-callback forged relation and row-handle attempts. The cell-view test bootstraps catalog administration and seeds rows directly as fixture setup. Broader hostile IPC sequences, mutation/constraint diagnostic equivalence, and timing inference remain open.

## AU-3: hidden and conditional unique keys do not expose probe outcomes — verified

- Contract: when an index key cell is hidden or only conditionally readable, point access must fall back before consulting the query's equality value so hidden key existence cannot select a different managed result.
- Regression: `unique_constraint_uses_its_backing_index_for_invocation_writes` now submits both a matching and a differently sized non-matching value under hidden and conditional cell projections. Each pair returns the same `Unavailable` result; the existing readable projection still returns the sanitized row for a matching key and reconciles staged updates/deletes.
- Verification: the focused real-redb test passed. Locked workspace build, rustfmt check, warning-denied Clippy, and the full serial delegated-cgroup workspace suite passed; the commit and transaction targets reported 148 and 74 passing tests.
- The later OP-5 integration connects this authorized probe to exact-equality optimizer hints and the TinyCC runner; the confined SQL acceptance exercises `SELECT id ... WHERE id = $1 LIMIT 1`. The catalog-index fixture compares point and range results against an independently filtered authorized scan and retains hidden/conditional-key `Unavailable` fallback checks.
- Remaining: broader key types/coercions need SQL contracts and acceptance coverage; complex-predicate redaction-error equivalence and wider workloads remain open.

## AU-2 / AU-7: hidden-row UPDATE and DELETE match absent-row diagnostics — verified

- Contract: authorization must not reveal whether a row exists when the caller's row policy hides it; denied UPDATE or DELETE against a hidden row and against an absent row return the same non-disclosing diagnostic.
- Regression: `authorized_patch_checks_changed_cells_and_freezes_policy_dependencies` and `authorized_delete_requires_visibility_and_an_explicit_delete_grant` give both attempts the same operation scope, grants, and policy shape. One stored row is hidden by its SELECT policy; the other target has no stored row. Both UPDATE and DELETE staging calls return `TransactionError::NotFound` with identical debug diagnostics.
- Verification: both focused real-redb transaction tests passed. Workspace build, format check, warning-denied all-target Clippy, maintained TinyCC SDK tests and sandbox fixture, and the full serial delegated-cgroup workspace suite passed with zero ignored tests; the suite reported 233 engine unit tests, 172 commit tests, and 75 transaction tests. `git diff --check` passed.
- Scope: verifies the storage staging errors for UPDATE and DELETE under one row policy. It does not claim constant-time behavior, external IPC byte equivalence, or equivalent diagnostics for constraint failures.

## AU-2 / AU-7: constraint failure diagnostics omit row and value details — verified

- Contract: rejected CHECK, unique-index, and foreign-key mutations return generic diagnostics that do not include the target row identity or attempted/conflicting value. Constraint success/failure may still reveal information; these checks cover diagnostic contents only.
- Regression: `authorized_patch_checks_changed_cells_and_freezes_policy_dependencies` compares CHECK-failing updates on distinct rows and values. `unique_constraint_uses_its_backing_index_for_invocation_writes` compares conflicts for distinct unique values and rows. `catalog_foreign_keys_check_child_and_parent_mutations` compares missing parent references for distinct values and rows.
- Remaining: global constraint success/failure may still reveal hidden key existence, as specified; broader hostile IPC sequences and timing inference remain open.

## SM-5 / AU-6: identity drop aborts a role-authorized staged write — verified

- Contract: dropping an identity revokes token admission and invalidates authorization state so an invocation admitted before the drop cannot publish its staged write.
- Regression: `identity_drop_aborts_a_role_authorized_staged_update` creates a user and role, admits the user by token through that role, scans and stages a real row update under role grants and row/cell policies, revokes membership, and drops the user. The stale invocation conflicts and records ABORTED, the original row remains, the drop commits, and later use of the dropped user's token fails without recording an invocation outcome.
- Implementation: no production change was needed; membership and identity guards reject the stale finalization, while identity drop revokes the token atomically.
- Verification: focused regression passed. Locked workspace build, rustfmt check, warning-denied all-target Clippy, and the full serial delegated-cgroup workspace suite passed in the pinned Rust/Cargo 1.95.0 Nix shell. The suite reported 233 engine unit tests, 173 commit integration tests, 75 transaction integration tests, all remaining workspace targets green, and zero ignored tests. The fixture includes a usable login-enabled administrator credential required by final-admin validation.
- Remaining: repeat races for identity and other authority-changing DDL, and broader concurrent schedules.

## 2026-11 delegated acceptance checkpoint

- The complete serial delegated-cgroup workspace suite passed with sandbox and process tests enabled and zero ignored tests. Relevant direct coverage includes 13 authorization unit tests, 177 commit tests, and 75 transaction tests; the full run also exercised the confined forged-handle and two-principal cell-redaction paths.
- This is a validation checkpoint for the existing authorization implementation. AU-3 broader SQL/optimizer equivalence, AU-7 wider hostile IPC sequences, and remaining authority-change race schedules remain open.

## 2026-11 delegated acceptance checkpoint

- The complete serial delegated-cgroup workspace suite passed with sandbox and process tests enabled and zero ignored tests. Relevant direct coverage includes 13 authorization unit tests, 177 commit tests, and 75 transaction tests; the full run also exercised confined forged-handle and two-principal cell-redaction paths.
- AU-3 broader SQL/optimizer equivalence, AU-7 wider hostile IPC sequences, and remaining authority-change race schedules remain open.

## 2026-11-16 integrated acceptance refresh

- The complete serial delegated-cgroup workspace suite passed with sandbox and process tests enabled and zero ignored tests. It includes 291 engine unit tests, 177 commit tests, 75 transaction tests, and the hostile forged-handle executor paths.
- Authorization gaps remain as recorded above: broader AU-3 key/workload/error equivalence, AU-7 hostile IPC sequences and timing inference, and additional authority-change race schedules.

## AU-3 / OP-5: authorized point and range access through optimizer — verified slice

- The optimizer emits a unique point hint only for an exact supported equality on a statically readable unique key; the TinyCC SDK checks the hint against the parsed predicate, re-resolves relation identity, and calls the executor probe. The confined SQL DML acceptance exercises the point path and the two-sided non-unique range path.
- `unique_constraint_uses_its_backing_index_for_invocation_writes` obtains the authorized reference scan and independently filters its rows, then compares row identity and sanitized cells with unique point and bounded range results. Hidden/conditional keys still yield `Unavailable` before key-value matching.
- Verification: the full serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests. Remaining: broaden key types/coercions and complex-predicate error-equivalence coverage.

## AU-3: hidden-key type-error equivalence — verified slice

- The catalog unique-index probe now has a regression where the index key is hidden and the equality input has the wrong scalar type. It returns the same `Unavailable` fallback as matching and nonmatching correctly typed inputs, confirming that key readability is checked before value encoding or comparison.
- Verification: `cargo test --locked --test transaction unique_constraint_uses_its_backing_index_for_invocation_writes -- --exact --nocapture`, formatting check, and `git diff --check` passed. The complete delegated workspace suite passed immediately before this test-only addition; broader key-type/coercion and complex-predicate error-equivalence coverage remains open.

## 2026-11-16 delegated integration refresh

- The complete serial delegated-cgroup workspace suite passed with sandbox and process tests enabled and zero ignored tests: 291 engine unit tests, 179 commit integration tests, and 75 transaction tests. Authorization coverage includes the unique-key fallback regression, hidden-row diagnostic equivalence, authority-change write invalidation, and the confined forged-handle paths.
- Broader key-type/coercion and complex-predicate error equivalence, hostile IPC sequences, and timing inference remain open.

## AU-3: hidden BYTEA key mismatch matrix — verified slice

- Expanded `unique_constraint_uses_its_backing_index_for_invocation_writes` to try every other supported scalar type against a hidden BYTEA unique key. Boolean, integer widths, decimal, text/varchar, UUID, date, and timestamp inputs all return the same `Unavailable` result as matching and nonmatching BYTEA inputs.
- Verification: the focused real-redb transaction regression passed; workspace format check, warning-denied all-target Clippy, and `git diff --check` passed. The complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests: 291 engine unit tests, 179 commit tests, 75 transaction tests, and three metadata process tests.
- Remaining: complex-predicate and coercion equivalence beyond the supported unique-key scalar types, plus hostile IPC and timing-inference coverage.

## AU-7: closed relation capability rejected through executor IPC — verified slice

- `executor_resolve_relation_ipc_dispatch_returns_only_authorized_handle` now sends `OPEN SCAN` with a previously valid relation handle after closing it. Dispatch rejects the stale capability; resolving the same relation again yields a fresh handle, and replaying the closed handle remains rejected.
- Verification: focused real-redb IPC regression passed. Workspace build, format check, warning-denied all-target Clippy, and `git diff --check` passed. The complete serial delegated-cgroup workspace suite passed with sandbox/process coverage enabled and zero ignored tests: 291 engine unit tests, 179 commit tests, 75 transaction tests, and all other workspace targets.
- Remaining: wider hostile IPC sequences, cross-principal handle attacks, and timing inference remain open.

## AU-3: failing policy predicates use the authorized fallback — verified slice

- Regression: `unique_constraint_uses_its_backing_index_for_invocation_writes` uses a real-redb table with an index-key policy whose predicate divides by zero. The unique-key probe returns `Unavailable` without evaluating the predicate; the authorized reference scan returns the existing generic `EvaluationFailed` error and persists the invocation as `ABORTED`.
- Verification: the focused real-redb transaction test, locked workspace build, format check, warning-denied all-target Clippy, maintained TinyCC SDK tests, and sandbox-query fixture build passed under the locked Rust 1.95.0 development shell. The full serial delegated-cgroup workspace suite passed with sandbox and process coverage enabled: 291 engine unit tests, 179 commit tests, 75 transaction tests, and all other workspace targets.
- Remaining: broader coercion and workload coverage, other erroring predicates, and wider hostile IPC and timing analysis remain open.

## AU-7 / MD-5: executor IPC rejects cross-connection handle replay — verified slice

- `executor_resolve_relation_ipc_dispatch_returns_only_authorized_handle` now resolves a relation on one metadata connection, closes that connection, then replays its valid handle in an OPEN SCAN request on a new connection in the same invocation. Dispatch rejects the replay with `InvalidMetadataRequest`.
- Verification: the focused real-redb executor IPC regression, locked workspace build, format check, and warning-denied all-target Clippy passed. The full serial delegated-cgroup workspace suite passed with sandbox/process tests enabled: 291 engine unit tests, 179 commit tests, 75 transaction tests, and all other workspace targets.
- Remaining: broader hostile IPC sequences, cross-principal handle attacks, and timing inference remain open.

## AU-6 / ID-5: role revocation races an authorized staged writer — verified slice

- `role_membership_revocation_aborts_a_staged_role_authorized_writer` now races final write commit against role-membership revocation. Either the write commits first and precedes the revocation, or the revocation wins and the stale write records ABORTED without changing the row; the durable outcomes and commit sequence are checked for both orders.
- Verification: focused real-redb regression, locked workspace build, format check, and warning-denied all-target Clippy passed. The full serial delegated-cgroup workspace suite passed with sandbox/process tests enabled: 291 engine unit tests, 180 commit tests, 75 transaction tests, and all remaining workspace targets.

## AU-7 / MD-5: executor IPC rejects cross-principal handle replay — verified slice

- `executor_resolve_relation_ipc_dispatch_returns_only_authorized_handle` now replays a valid relation handle over OPEN SCAN from both another metadata connection and a different principal's invocation. Both requests fail with `InvalidMetadataRequest` before a scan is opened.
- Verification: focused real-redb IPC regression, locked workspace build, format check, warning-denied all-target Clippy, and full serial delegated-cgroup workspace suite passed. The suite ran sandbox/process tests with zero ignored: 291 engine unit tests, 180 commit tests, 75 transaction tests, and all remaining targets.

## AU-3: compound policy failures force the reference path — verified slice

- Extended `unique_constraint_uses_its_backing_index_for_invocation_writes` so the SELECT policy combines a failing field expression with a principal predicate. The optimized unique-index probe remains `Unavailable` without evaluating the hidden-key policy, while the authorized reference scan returns the generic `EvaluationFailed` error and durably aborts the invocation.
- Verification: the focused real-redb transaction regression, workspace format check, warning-denied all-target Clippy, and `git diff --check` passed. The complete serial delegated-cgroup suite passed on the immediately preceding schema DDL commit; this test-only extension was verified with the focused regression and static checks.
- Remaining: broader coercion/workload equivalence, additional failing predicates, hostile IPC sequences, and timing inference remain open.

## AU-6 / DP-8: policy removal races a staged writer — verified slice

- The policy lifecycle regression now races removal of an active row policy and its private expression against a staged row write. It checks serialization order, stale-writer abort, durable outcomes, row presence, pinned old-catalog visibility, and cleanup in the current catalog.
- Verification: the focused real-redb regression, locked workspace build, format check, warning-denied all-target Clippy, and `git diff --check` passed. The complete serial delegated-cgroup suite passed with sandbox/process tests enabled and zero ignored: 291 unit tests, 183 commit tests, 75 transaction tests, and all remaining targets.
