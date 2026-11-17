# Data Dictionary Progress

## DD-3 / CL-3 / DS-3: final catalog validation requires object tombstones — verified slice

- DROP FOREIGN KEY now atomically reserves the deleted FK constraint ID in family 17. Serialized FK DDL validation requires a reservation for every deleted FK; final catalog validation requires reservations for every deleted family-2 schema or object of kinds 3–10, including trusted generic catalog commits.
- `storage_manager_drops_foreign_key_and_advances_both_table_epochs` checks the reservation. `catalog_rejects_foreign_key_deletion_without_identity_tombstone` reproduced and closed the generic-commit bypass. The complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests; build, format, warning-denied all-target Clippy, and `git diff --check` passed.


## DD-3 / CL-3 / DS-3: constraint DDL retires constraint and expression IDs — verified slice

- CHECK, NOT NULL, PRIMARY KEY, and UNIQUE drops now reserve every deleted constraint/expression/index object ID in family 17. The constraint DDL validator requires exact agreement with deleted object records; key-constraint backing-index reservations remain included.
- `storage_manager_authorizes_constraint_ddl_by_table_alter` and all DROP COLUMN regressions passed. The complete serial delegated-cgroup workspace suite passed with sandbox and process integration tests enabled and zero ignored tests; workspace build, formatting, and warning-denied all-target Clippy passed.
- Remaining: type-object retirement has no current drop path; historical pre-v2 drop history is handled separately by the migration contract.


## DD-3 / CL-3 / DP-3: DROP POLICY reserves removed object IDs — verified slice

- Direct policy drops and atomic policy replacement now reserve the deleted policy and expression object IDs in family 17. Policy DDL validation requires an exact match to deleted objects; the policy ID and expression IDs cannot be reused.
- `storage_manager_authorizes_policy_ddl_for_owners_and_inherited_manage_policies` verifies both reservations after DROP POLICY. Focused test passed. Verification: workspace build, formatting check, warning-denied all-target Clippy, and full serial delegated-cgroup workspace suite passed. Sandbox and process tests were enabled; zero tests were ignored.
- Remaining: independent deletion paths for other object kinds and historical pre-v2 drop history.


## DD-3 / CL-3 / DS-3: DROP COLUMN reserves removed column and expression IDs — verified slice

- DROP COLUMN now publishes family-17 reservations for the removed column object and any removed current or immutable-default expression objects. Commit validation derives the exact reservation set from the deleted column/expression objects.
- `storage_manager_renames_tables_and_drop_column_rewrites_rows` verifies the column reservation while retaining old-reader and stale-writer behavior. Focused test passed. Verification: workspace build, formatting check, warning-denied all-target Clippy, and full serial delegated-cgroup workspace suite passed. Sandbox and process tests were enabled; zero tests were ignored.
- Remaining: retirement in other independent object-deletion paths and historical pre-v2 drop history.


## DD-3 / CL-3 / DS-3: DROP TABLE reserves every removed owned object ID — verified slice

- DROP TABLE now reserves IDs for every family-2 object it removes: the table, columns, expressions, constraints, indexes, and policies. The authorization validator derives the exact expected reservation set from the deleted object records and requires a matching empty family-17 record for each ID.
- `storage_manager_drop_table_removes_owned_catalog_and_rows` verifies reservations for a table with a default expression, CHECK expression, policy expression, policy, constraint, and column. Focused real-redb test passed.
- Verification: workspace build, formatting check, warning-denied all-target Clippy, and full serial delegated-cgroup workspace suite passed. Sandbox and process tests were enabled; zero tests were ignored.


## DD-3 / CL-3 / DS-3: key-constraint backing-index IDs are retired — verified slice

- `DROP PRIMARY KEY` and `DROP UNIQUE` now reserve the owned backing-index ID in family 17 in the same authorized catalog transaction that removes its index, constraint, object, and dependency records. The constraint DDL validator requires reservations to match the deleted owned indexes exactly.
- `storage_manager_creates_primary_key_with_owned_unique_index` checks the reservation in the post-drop snapshot while preserving the old-snapshot and stale-writer checks. Focused test passed. Verification: workspace build, formatting check, warning-denied all-target Clippy, and full serial delegated-cgroup workspace suite passed. Sandbox and process tests were enabled; zero tests were ignored.
- Remaining: retirement for other object kinds and historical pre-v2 drop history.


## DD-3 / CL-3 / DS-3: retired schema IDs are permanently reserved — verified slice

- Contract: family 17 now reserves dropped schema IDs as well as deleted user/role IDs. A schema's object and typed records and its reservation must publish atomically; final catalog validation rejects any reservation that coexists with a live object/principal and requires a reservation when a schema object is deleted.
- Red/green evidence: the schema lifecycle regression first failed because DROP SCHEMA left no family-17 record. It now checks the reservation and attempts a later raw catalog recreation of the same schema/object ID, which must fail with `InvalidCatalog`. The focused `storage_manager_creates_schema_atomically_for_privileged_actor` test passed after updating the schema-drop DDL validator to admit and guard the third catalog write.
- Spec updates: [catalog payloads](../../specs/catalog-payloads.md), [catalog lifecycle](../../specs/catalog-lifecycle.md), [data dictionary](../../specs/data-dictionary.md), and [schema DDL](../../specs/ddl-schema.md).
- Verification: the focused real-redb regression, workspace build, formatting check, warning-denied all-target Clippy, and full serial workspace suite passed. The suite ran inside the delegated `systemd --user` service with sandbox and process tests enabled; zero tests were ignored.
- Remaining: this does not persist retirement for dropped table, column, index, constraint, policy, expression, or type IDs. Historical pre-v2 schema drops are not reconstructed by the identity-history migration input.

## DD-3 / CL-3 / DS-3: retired table IDs are permanently reserved — verified slice

- DROP TABLE now atomically publishes a family-17 reservation with deletion of the table's object and typed records. The table-drop authorization validator validates that exact reservation and guards its absence; final catalog validation requires a reservation for table-object deletion and rejects a later recreation attempt.
- Red evidence: without the reservation, `storage_manager_drop_table_removes_owned_catalog_and_rows` failed at the new post-drop assertion. The green regression checks that the reservation is visible in the current snapshot, then attempts to restore the table's pre-drop object and typed records and requires `InvalidCatalog`.
- Verification: focused real-redb table lifecycle regression, workspace build, formatting check, warning-denied all-target Clippy, and full serial delegated-cgroup workspace suite passed. Sandbox and process tests were enabled; zero tests were ignored.
- Remaining: IDs for owned child definitions and other object kinds are not independently tombstoned; historical pre-v2 table drops are not reconstructed.

## DD-3 / CL-3 / DS-3: retired index IDs are permanently reserved — verified slice

- Ordinary `DROP INDEX` now publishes a family-17 reservation with deletion of its family-2/family-9 records and physical memberships. Its authorized DDL validator requires that reservation and rejects a later same-ID catalog recreation. `DROP TABLE` also reserves its table ID and any owned index object IDs it deletes.
- Regression coverage in `storage_manager_creates_named_unique_index_and_backfills_atomically` checks the reservation and attempts to restore the pre-drop records; `storage_manager_drop_table_removes_owned_primary_and_unique_indexes` covers index retirement nested under table deletion. Focused tests pass.
- Verification: focused index/table regressions, workspace build, formatting, warning-denied all-target Clippy, and full serial delegated-cgroup workspace suite passed. Sandbox and process tests were enabled; zero tests were ignored.
- Remaining: IDs for other dropped child and object kinds remain open.

## Policy-target catalog envelope — in progress

- Contract: [catalog payloads v1](../../specs/catalog-payloads.md), family 14. The 71-byte envelope includes a 39-byte canonical logical key and no TLVs; IDs are nonzero and target kind is 1, 2, or 3.
- Both tests failed when the compiling placeholder rejected every record. The implementation now encodes the independently specified fixture and rejects malformed header, key, marker, ID, kind, length and trailing bytes.
- This validates only the record's bytes and local fields. Catalog graph references, policy semantics, storage, bootstrap, and all other catalog families remain unimplemented.

## DD-1: schema-driven CAT1 envelope — in progress

- Contract: [catalog payload encoding v1](../../specs/catalog-payloads.md), Envelope and TLVs. Added a shared schema-driven envelope encoder/decoder with explicit family field registries, required/optional tags, typed wire values, canonical sorted TLVs, exact lengths, zero reserved bytes, size/field/list bounds, and rejection of malformed IDs, invalid UTF-8, and unknown tags/types. IDS fields validate nonzero IDs and can declare sorted-unique set semantics.
- Red/green evidence: generic round-trip, unknown-field, malformed-length, wire-width, boolean, UTF-8, and IDS set fixtures pass; the pre-existing 71-byte policy-target binary fixture remains byte-identical.
- Verification: the direct real-redb-linked engine suite passed all 55 integration tests across 14 files; direct rustfmt check and direct Clippy warnings-as-errors passed. A pinned redb snapshot test reads a catalog payload, while malformed stored catalog bytes fail closed. Catalog/audit/outcome publication is covered by a real-redb atomic-commit test.
- Added concrete field registries for all 16 catalog families and canonical family-key structure checks, including typed UUID/u16 components, bounded enum codes, and the object-scope grant target equality rule. A table-driven fixture now encodes and decodes required fields for every registered family.
- Membership graph integrity is now checked against the latest catalog plus staged membership changes during serialized publication. It rejects missing member/role records, non-role targets, disconnected cycles, and graphs over 4096 edges. Real-redb tests verify rollback and ABORTED outcome publication.
- Family-7 expression records now use bounded EIR1 and are validated on catalog reads/writes. Invocation APIs load purpose-checked default/CHECK expressions from their pinned snapshot and guard their bytes through final commit.
- Remaining: generated all-family fixtures are not independent hand-checked byte fixtures; broader semantic cross-record integrity, bootstrap, DDL validation/authority checks, catalog secondary indexes, and comprehensive snapshot-aware catalog operations remain unimplemented.

## DD-1: independent database-record bytes — verified

- Regression: `database_record_matches_independent_catalog_v1_fixture` compares the generic family-1 encoder with an independently assembled CAT1 envelope, including the typed UUID logical-key component, empty BYTES field, optional-field omission, TLV order, and exact total/key lengths. The generated decoder is then checked against the same hand-authored bytes.
- Verification: the focused `cargo test --test catalog database_record_matches_independent_catalog_v1_fixture --locked` passed in the pinned Nix shell; `cargo fmt --all -- --check` passed.
- Remaining: add independent byte fixtures for the other catalog families; this first fixture does not complete all-family independent coverage.

## DD-1: independent object-record bytes — verified

- Regression: `object_record_matches_independent_catalog_v1_fixture` checks a hand-assembled family-2 CAT1 record with its typed UUID key, omitted optional parent ID, UTF-8 normalized name, nonzero owner ID, and little-endian epoch/kind fields. The generic encoder must match every byte and the registered decoder must recover the same fields.
- Verification: both independent family-1/family-2 fixtures passed with `cargo test --test catalog record_matches_independent_catalog_v1_fixture --locked` in the pinned Nix shell.
- Remaining: independent byte fixtures for families 3–17 and broader catalog semantics remain open.

## DD-1: independent schema-record bytes — verified

- Regression: `schema_record_matches_independent_catalog_v1_fixture` checks a hand-assembled family-3 CAT1 record and its IDS search-path field, including the little-endian list count, raw ID bytes, field length, and total envelope length.
- Verification: the focused catalog fixture command passed all three independent family-1, family-2, and family-3 fixtures.
- Remaining: independent byte fixtures for families 4–17 and broader catalog semantics remain open.

## DD-1: independent built-in type-record bytes — verified

- Regression: `type_record_matches_independent_catalog_v1_fixture` verifies a family-4 CAT1 record containing two U16 and four U32 fields against a hand-assembled byte sequence, then checks the registered decoder's result.
- Verification: the focused catalog fixture command passed all four independent family-1 through family-4 fixtures.
- Remaining: independent byte fixtures for families 5–17 and broader catalog semantics remain open.

## DD-1: independent table-record bytes — verified

- Regression: `table_record_matches_independent_catalog_v1_fixture` checks family-5 schema/storage IDs, schema/write epochs, row identity mode, and exact CAT1 lengths against a hand-assembled record and the registered decoder.
- Verification: the focused fixture command passed all independent family-1 through family-5 CAT1 tests.
- Remaining: independent byte fixtures for families 6–17 and broader catalog semantics remain open.

## DD-1: independent column-record bytes — verified

- Regression: `column_record_matches_independent_catalog_v1_fixture` verifies a family-6 record with type parameters in BYTES, nullable=false, omitted optional expression IDs, and nonzero schema version against its exact CAT1 bytes and decoded fields.
- Verification: the focused catalog fixture command passed all independent family-1 through family-6 fixtures.
- Remaining: independent byte fixtures for families 7–17 and broader catalog semantics remain open.

## DD-1: independent expression-record bytes — verified

- Regression: `expression_record_matches_independent_catalog_v1_fixture` checks the purpose/version fields, result type and parameters, EIR1 bytes, and a one-entry IDS dependency list against exact family-7 CAT1 bytes.
- Verification: all seven independent family-1 through family-7 catalog fixtures passed in the focused test run.
- Remaining: independent byte fixtures for families 8–17 and broader catalog semantics remain open.

## DD-1: independent constraint-record bytes — verified

- Regression: `constraint_record_matches_independent_catalog_v1_fixture` checks a family-8 NOT NULL-shaped record with one source column, an empty referenced-column list, omitted optional references, and fixed action/state fields against exact bytes.
- Verification: the focused catalog fixture command passed all independent family-1 through family-8 tests.
- Remaining: independent byte fixtures for families 9–17 and broader catalog semantics remain open.

## DD-1: independent index-record bytes — verified

- Regression: `index_record_matches_independent_catalog_v1_fixture` checks a family-9 index with a one-column IDS key, unique flag, omitted owner constraint, storage object, state, key version, and table schema version against exact bytes.
- Verification: the focused catalog fixture command passed all independent family-1 through family-9 tests.
- Remaining: independent byte fixtures for families 10–17 and broader catalog semantics remain open.

## DD-1: independent principal-record bytes — verified

- Regression: `principal_record_matches_independent_catalog_v1_fixture` checks the family-10 role kind, normalized UTF-8 name, false login flag, nonzero authority epoch, typed key, and exact envelope bytes.
- Verification: the focused catalog fixture command passed all independent family-1 through family-10 tests.
- Remaining: independent byte fixtures for families 11–17 and broader catalog semantics remain open.

## DD-1: independent membership-record bytes — verified

- Regression: `membership_record_matches_independent_catalog_v1_fixture` checks the two typed UUID components in the family-11 logical key and the grantor ID/membership epoch TLVs against independently assembled bytes.
- Verification: the focused catalog fixture command passed all independent family-1 through family-11 tests.
- Remaining: independent byte fixtures for families 12–17 and broader catalog semantics remain open.

## DD-1: independent grant-record bytes — verified

- Regression: `grant_record_matches_independent_catalog_v1_fixture` checks the compound family-12 key's grantee, object, versioned privilege, object-scope marker, and repeated target ID, plus grantor/authority epoch fields.
- Verification: the focused catalog fixture command passed all independent family-1 through family-12 tests.
- Remaining: independent byte fixtures for families 13–17 and broader catalog semantics remain open.

## DD-1: independent policy-record bytes — verified

- Regression: `policy_record_matches_independent_catalog_v1_fixture` checks the family-13 table ID, row-policy/SELECT/ALLOW codes, included USING expression ID, omitted CHECK expression ID, and authority epoch against exact bytes.
- Verification: the focused catalog fixture command passed all independent family-1 through family-13 tests.
- Remaining: independent byte fixtures for families 14–17 and broader catalog semantics remain open.

## DD-1: independent dependency-record bytes — verified

- Regression: `dependency_record_matches_independent_catalog_v1_fixture` verifies the family-15 dependent/referenced UUID pair, edge-kind component, zero field count, and exact 71-byte no-TLV envelope.
- Verification: the focused catalog fixture command passed the independent family-15 dependency fixture together with families 1–13. The existing family-14 policy-target fixture independently checks its no-TLV key format.
- Remaining: independent fixtures for family 16 and family 17, then broader catalog semantics, remain open.

## DD-1: independent credential-verifier bytes — verified

- Regression: `verifier_record_matches_independent_catalog_v1_fixture` checks family-16 algorithm/version, 32-byte verifier, signed issue/expiry times, non-revoked state with omitted revocation time, UTF-8 label, and authority epoch against exact bytes.
- Verification: the focused family-16 catalog fixture passed under the pinned Nix Rust toolchain.
- Remaining: independent family-17 tombstone bytes and broader catalog semantics remain open.

## DD-1: independent tombstone bytes and all-family CAT1 fixtures — verified

- Regression: `tombstone_record_matches_independent_catalog_v1_fixture` verifies family 17's permanent-reservation key and zero-TLV envelope. Together, the family 1–17 tests now compare every registered family against hand-assembled wire bytes; family 14 is covered by the original independently specified policy-target fixture.
- Verification: `nix develop --offline --command cargo test --test catalog --locked` passed all 22 catalog integration tests, including all 17 family fixtures, malformed/noncanonical envelope checks, and the complete field-registry check.
- Remaining: broader semantic cross-record integrity, DDL/bootstrap authority validation, catalog secondary indexes, and comprehensive snapshot-aware catalog operations remain open.

## DD-1: database identity cannot be an object — verified

- Contract: [catalog payloads v1](../../specs/catalog-payloads.md) states that the database record has no family-2 object row. Added commit-time validation that rejects staging the database UUID as any object identity, and startup validation that fails closed if a persisted family-2 record uses the database UUID.
- Regression evidence: the commit test first demonstrated that the invalid schema/object pair was published (commit sequence advanced); the startup corruption test first demonstrated that the malformed image was accepted. Both now assert rejection, and the commit test verifies no sequence advancement.
- Verification: focused startup and commit regressions passed under the pinned Nix development shell.
- Remaining: broader semantic cross-record integrity, DDL/bootstrap authority validation, catalog secondary indexes, and comprehensive snapshot-aware catalog operations remain open.

## DD-1: schema search paths resolve to schemas — verified

- Contract: family-3 `search_path` IDs must resolve in the final catalog view to family-3 schema records whose family-2 object has schema kind. Startup fails closed for unresolved entries; serialized catalog commits reject unresolved or removed targets before publication.
- Regression evidence: an independent startup corruption case first showed that a search-path entry for a missing schema was accepted. Matching startup and commit regressions now reject that image/write, with the commit sequence unchanged.
- Verification: full startup integration target passed (49 tests); full commit integration target passed (131 tests, 4 sandbox fixtures ignored because delegated cgroups and TinyCC fixtures are unavailable). Formatting check passed.
- Remaining: broader semantic cross-record integrity, DDL/bootstrap authority validation, catalog secondary indexes, and comprehensive snapshot-aware catalog operations remain open.

## DD-2: principal-name secondary lookup — verified (partial DD-2)

- Contract: exact normalized user/role names map to one principal ID in the `principal_names_v1` redb table. Family-10 writes and this index publish in one transaction; readers use their existing redb snapshot. Startup verifies index/catalog agreement and rebuilds the derived index when opening a database that predates the table.
- Regression evidence: a snapshot test first showed that committed principals could not be resolved through the new lookup. It now covers creation, rename, retained old-snapshot lookup, current-snapshot lookup, and rejection of an index hit that disagrees with its family-10 row. Startup rebuilds a missing, stale, or malformed derived index from authoritative family-10 rows.
- Verification: `nix develop --offline --command cargo test --workspace --locked` passed all workspace tests; 4 sandbox executor fixtures were ignored because their delegated cgroup and TinyCC fixtures are unavailable. Workspace Clippy with warnings denied and `cargo fmt --all -- --check` passed.
- Remaining: complete DD-2 lookup/index coverage for schemas, objects, memberships, grants, policies, and dependencies.

## DD-2: snapshot-aware schema-name index — verified

- Added `schema_names_v1`, keyed by exact normalized schema-name bytes and pointing to the family-2 schema object ID. Schema object create, rename, and deletion update the derived entry in the same redb commit; bootstrap seeds the built-in schema names.
- `resolve_authorized_schema` now uses the pinned index after validating role closure, authority dependencies, grants, and schema USAGE visibility. A matching indexed ID is still validated against the authoritative family-2 and family-3 records.
- Startup reconstructs missing, stale, or malformed name indexes from family-2 schema objects. Snapshot tests cover schema creation/rename while retaining an older read snapshot; startup tests cover missing and stale entries.
- Verification: focused commit/startup regressions passed; all workspace tests passed (4 sandbox executor fixtures ignored because their delegated cgroup and TinyCC fixtures are unavailable); workspace Clippy with warnings denied and `cargo fmt --all -- --check` passed.
- Remaining: complete DD-2 lookup/index coverage for objects, memberships, grants, policies, and dependencies.

## DD-2: bidirectional membership index — verified

- Added `memberships_v1` with separate member-to-role and role-to-member key orders and snapshot lookup APIs. Each returned edge is checked against its authoritative family-11 record.
- Family-11 catalog writes maintain both entries in the same redb transaction; startup rebuilds both directions from authoritative records. Runtime role-closure resolution walks the pinned member-to-role index, validates every result against family 11, and guards each reachable edge and principal record through commit.
- Regression evidence: startup unit tests rebuild missing and stale directions; commit tests verify member-to-role and role-to-member results in both the original and a retained snapshot after another grant is published.
- Verification: membership reconstruction and snapshot regressions passed; `cargo test --workspace --locked` passed (4 sandbox executor fixtures ignored because delegated cgroups and TinyCC fixtures are unavailable); workspace Clippy with warnings denied passed.
- Regression evidence for indexed closure: an authenticated user with no memberships succeeds under a one-record invocation scan budget even when the pinned catalog contains an unrelated role chain; the test failed before the indexed traversal with `ScanBudgetExceeded`. Existing transitive-closure tests verify reachable roles and stale-edge guards.
- Verification after indexed traversal: `cargo build --workspace --locked`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo test --workspace --locked` passed (4 sandbox executor fixtures ignored because delegated cgroups and TinyCC fixtures are unavailable).
- Remaining: complete DD-2 lookup/index coverage for objects, grants, policies, and dependencies.

## DD-2: grantee/object grant indexes — verified

- Added `grants_v1` with grantee-ordered and object-ordered keys carrying the exact family-12 catalog key. Snapshot lookup APIs validate each indexed key and its authoritative record.
- Family-12 writes update both orders in the same redb transaction; startup reconstructs both orders from authoritative family-12 records.
- Invocation grant loaders now enumerate only the admitted principal's and reachable roles' candidates through the pinned grantee index, charge those candidates to the invocation scan-work budget, and retain exact family-12 commit guards.
- Regression tests cover reconstruction from missing/stale tables, reading a committed grant through both key orders, and an authenticated principal with no grants succeeding under a one-record budget despite unrelated grants. The budget regression failed before this change with `ScanBudgetExceeded`.
- Verification: focused grant rebuild/lookup tests passed; `cargo test --workspace --locked` passed (4 sandbox executor fixtures ignored because delegated cgroups and TinyCC fixtures are unavailable); workspace Clippy with warnings denied passed.
- Verification after indexed grant loading: `cargo build --workspace --locked`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo test --workspace --locked` passed (4 sandbox executor fixtures ignored because delegated cgroups and TinyCC fixtures are unavailable).
- Remaining: complete DD-2 lookup/index coverage for objects, policies, and dependencies.

## DD-2: table/operation policy index — verified

- Added `policies_v1`, keyed by table ID, operation code, and policy ID. Snapshot lookup checks each key against the family-13 policy record.
- Policy records are maintained atomically at commit and reconstructed at startup. Policy loading for a table now reads candidate IDs from the pinned index, then guards and validates each authoritative policy row; pre-startup snapshots retain the scan fallback.
- Regression evidence: startup unit tests rebuild missing/stale indexes; policy DDL integration verifies a committed policy through the indexed lookup, and normal transaction policy loading uses that pinned index.
- Verification: targeted policy reconstruction and DDL tests passed; `cargo test --workspace --locked` passed (4 sandbox executor fixtures ignored because delegated cgroups and TinyCC fixtures are unavailable); workspace Clippy with warnings denied passed.
- Remaining: complete DD-2 lookup/index coverage for objects.

## DD-2: bounded policy-target prefix reads — verified

- Runtime policy loading reads family-14 rows by the current policy ID prefix on the pinned catalog snapshot. Only target rows for policies on the requested table count against invocation scan work; each policy remains limited to 4,096 targets.
- Matching target records retain the same canonical CAT1/key checks and exact-byte commit guards. Startup and serialized catalog publication still validate all policy targets globally.
- Regression evidence: `policy_target_prefix_scan_ignores_other_policies_for_its_limit` proves a one-record policy prefix succeeds while a one-record whole-family scan fails when other policy targets are present. Transaction policy loading uses this bounded prefix path.
- Verification: focused snapshot tests passed; `cargo build --workspace --locked`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo test --workspace --locked` passed (4 sandbox executor fixtures ignored because delegated cgroups and TinyCC fixtures are unavailable).
- Remaining: complete DD-2 lookup/index coverage for objects.

## DD-2: bounded policy-index candidate scan — verified

- The pinned table-policy index scan now accepts a record limit and fails when it would exceed that limit. Runtime policy loading caps candidates at the authorization-input limit and charges them to the same cumulative invocation scan-work budget as matching targets.
- Regression evidence: the snapshot test verifies exact-limit candidate retrieval and rejects a table-policy prefix with more entries than its supplied cap; policy loading continues to verify each indexed ID against its authoritative family-13 record.
- Verification: focused snapshot and catalog-loader tests passed; `cargo build --workspace --locked`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo test --workspace --locked` passed (4 sandbox executor fixtures ignored because delegated cgroups and TinyCC fixtures are unavailable).
- Remaining: complete DD-2 lookup/index coverage for objects.

## DD-2: table-scoped bounded schema-column lookup — verified

- Table-schema loading now enumerates family-6 column IDs through the pinned table/ordinal index instead of scanning unrelated tables' columns. Each hit is checked against its authoritative family-6 record and charged to invocation scan work; the list is capped at `MAX_COLUMNS`.
- A bounded family-6 fallback remains when the requested table has no ordinal-index entries, preserving low-level snapshots created before derived-index maintenance. Startup validates/rebuilds the complete column index.
- Regression evidence: `table_column_scan_is_bounded_to_its_ordinal_prefix` proves unrelated ordinal prefixes do not consume a one-record scan limit. Schema loader coverage verifies typed columns and preserves the no-secondary-index compatibility path.
- Verification: all 16 catalog-index unit tests and all 73 transaction tests passed; `cargo build --workspace --locked`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo test --workspace --locked` passed (4 sandbox executor fixtures ignored because delegated cgroups and TinyCC fixtures are unavailable).
- Remaining: complete DD-2 lookup/index coverage for objects.

## DD-2: table-scoped column name and ordinal lookups — verified

- Added `column_lookups_v1` with table/name and table/zero-based-ordinal key orders. Snapshot lookup verifies each result against its authoritative family-6 column row.
- Family-6 writes replace both lookup keys atomically; startup reconstructs missing or stale entries from the catalog.
- Regression evidence: startup tests rebuild missing and stale lookup tables; column rename integration checks indexed lookups in an old snapshot and the current snapshot, including the stable ordinal.
- Verification: focused reconstruction and rename/snapshot tests passed; `cargo build --workspace --locked`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo test --workspace --locked` passed (4 sandbox executor fixtures ignored because delegated cgroups and TinyCC fixtures are unavailable).
- Remaining: complete DD-2 lookup/index coverage for named index objects.

## DD-2: schema-scoped named index lookup — verified

- Added `index_names_v1` for schema-parented family-2 index objects, keyed by parent schema ID and exact normalized name. Snapshot lookup validates the authoritative object kind, parent, and name.
- Family-2 index-object writes maintain the name entry atomically; startup verifies and reconstructs the index from authoritative objects.
- Regression evidence: startup tests reconstruct missing and stale name indexes; the named-index DDL integration covers create/drop lookup while a retained snapshot preserves the pre-drop result.
- Verification: focused startup and index lifecycle tests passed; `cargo build --workspace --locked`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo test --workspace --locked` passed (4 sandbox executor fixtures ignored because delegated cgroups and TinyCC fixtures are unavailable).
- Remaining: DD-2 cross-record naming-domain integrity and broader atomic catalog semantics continue under DD-3 and later increments.

## DD-3: shared schema-local relation/index name domain — verified

- Catalog commit checks the final relation-name and index-name lookup tables together, rejecting a table/index name collision in the same schema before publication.
- Startup performs the same cross-index check after rebuilding each authoritative name index, so persisted collisions fail closed.
- Regression evidence: DDL attempting to create an index with an existing table name leaves the commit sequence and index lookup unchanged; a deliberately conflicting persisted index object is rejected at startup validation. The named-index lifecycle test now drops and recreates the same name and verifies the new ID differs while the live lookup resolves it.
- Verification: targeted startup and DDL regressions passed; `cargo build --workspace --locked`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo test --workspace --locked` passed (4 sandbox executor fixtures ignored because delegated cgroups and TinyCC fixtures are unavailable).
- Remaining: implement and validate remaining DD-3 referential-integrity, drop/recreate ID non-reuse, and complete catalog publication checks.

## DD-4: bounded reverse dependency validation — verified

- RESTRICT-drop validation walks `dependencies_v1` in deterministic referenced-ID/edge order, validates each candidate against family 15, and applies a 65,536-candidate bound. The serialized final catalog validator still rechecks the complete graph against the latest state before publication.
- Regression evidence: the boundary case seeds exactly 65,536 family-15 records, attempts one additional dependency, and checks the edge stays absent, the commit sequence is unchanged, and the outcome is ABORTED. Existing FK-create/table-drop and FK-create/primary-key-drop races confirm only a valid serial order commits.
- Verification: focused bound and race regressions passed; `cargo build --workspace --locked`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo test --workspace --locked` passed (4 sandbox executor fixtures ignored because delegated cgroups and TinyCC fixtures are unavailable).
- Remaining: DD-5 snapshot retention and lifecycle acceptance continue in later increments.

## DD-5: pinned schema and row layout survive ADD COLUMN — verified slice

- Regression: `storage_manager_adds_column_and_backfills_existing_rows` now pins a real redb read snapshot before ADD COLUMN, commits the DDL, and verifies the old snapshot still has no lookup for the new column and decodes the old row payload with its prior schema. The current view still verifies the backfilled new shape.
- Verification: the focused regression passed; `cargo build --workspace --locked`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo test --workspace --locked` passed (4 sandbox executor fixtures ignored because delegated cgroups and TinyCC fixtures are unavailable).
- Remaining: cover row/default decoding under reclamation pressure; the existing rename regressions plus the ADD/DROP retention slices above do not claim the complete DD-5 lifecycle gate.

## DD-5: pinned table and row survive DROP TABLE — verified slice

- Regression: `storage_manager_drop_table_removes_owned_catalog_and_rows` holds a real redb snapshot across an atomic table drop, then checks the pre-drop table definition and row remain readable and decodable while the current view confirms their removal. This includes the existing 65,536-row drop path.
- Verification: the focused regression passed; `cargo build --workspace --locked`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo test --workspace --locked` passed (4 sandbox executor fixtures ignored because delegated cgroups and TinyCC fixtures are unavailable).
- Remaining: long-reader reclamation pressure and add-column-default decoding under retained versions remain open.

## DD-2: bidirectional dependency-edge index — verified

- Added `dependencies_v1`, keyed by direction, the dependent/referenced UUID pair, and edge kind. Snapshot lookups validate each index entry against its authoritative family-15 record.
- Family-15 writes maintain both directions in the commit transaction; startup verifies and reconstructs the index from family-15 catalog rows.
- Regression evidence: startup unit tests rebuild missing and stale dependency indexes; commit integration coverage confirms both directions and verifies a retained snapshot remains stable after another dependency is committed.
- Verification: targeted dependency reconstruction and snapshot tests passed; `cargo build --workspace --locked`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo test --workspace --locked` passed (4 sandbox executor fixtures ignored because delegated cgroups and TinyCC fixtures are unavailable).
- Remaining: complete DD-2 lookup/index coverage for objects.

## DD-2: snapshot-aware relation-name index — verified

- Added a schema-scoped `relation_names_v1` lookup key composed of schema ID and exact normalized relation-name bytes. The index points to family-5 table IDs and is checked against the family-5 schema reference and family-2 table object on reads.
- Authorized qualified relation resolution uses the pinned index for hits and independently revalidates relation visibility and column shape; index misses retain the existing authorized scan path for snapshots created before startup repair.
- Regression evidence: real DDL integration checks table creation, old-snapshot lookup after rename, and the new name in a current snapshot. Startup unit fixtures rebuild both stale and missing indexes and verify exact family-5/family-2 references.
- Verification: relation-index reconstruction unit tests and real table create/rename snapshot regression passed. `cargo test --workspace --locked` passed (4 sandbox executor fixtures ignored because delegated cgroups and TinyCC fixtures are unavailable); workspace Clippy with warnings denied passed.
- Remaining: complete DD-2 lookup/index coverage for objects, memberships, grants, policies, and dependencies.

## SM-4 / SM-5 / DD-1 dependency: catalog expression IR and pinned loading — in progress

- Red evidence: the expression-codec integration target initially failed because no `expression` module existed. Added EIR1 with a hand-checked prefix fixture, complete operator/scalar round trips, strict framing, canonical scalar validation, and limits of 256 KiB, 256 nodes, 32 levels, and field ordinals 0–4095.
- Family-7 decoding now validates the expression purpose, IR version, result metadata widths, dependency ID set, and EIR body. Catalog writes reject malformed/unsupported expression records; snapshot and invocation reads validate them. Invocation reads capture exact catalog bytes as commit guards.
- Authorized INSERT can load DEFAULT and CHECK expressions by ID from the pinned catalog, enforce purpose, materialize defaults, and evaluate CHECK and row policies against the resulting tuple. A real-redb race changes the stored default expression after it is read; the invocation durably aborts and publishes no row. The insert integration fixture also loads its CHECK expression from the catalog.
- Verification: all 85 tests across 16 integration targets passed with direct Rust 1.95 compilation against cached redb 4.4.0. Direct Clippy with warnings denied, rustfmt `--check`, and `git diff --check` passed. Cargo workspace commands remain unable to resolve the crates.io index in this environment.
- Limits: EIR decoding checks structure, not schema-aware operand/result types or whether declared dependencies match field references. Callers still provide column schemas, grants, policies, and expression IDs; generated UUID allocation, DDL authorization/fences, complete catalog loaders, and service integration remain outstanding.

## SM-5 / DD-1 / AU-1 dependency: catalog membership graph integrity — in progress

- Membership catalog commits now read the latest family-11 records and overlay the proposed catalog changes inside the serialized redb write transaction. The final graph is checked for live member/role endpoints, role target kinds, zero IDs, cycles in any component, and the 4096-edge bound before publication. Malformed or cyclic final graphs publish no catalog/audit changes and record ABORTED.
- Red evidence: role-closure coverage showed an unreachable cycle was accepted; full-graph validation now rejects it. Real-redb tests also reject a user as a role target and a missing member, with no sequence/catalog/audit publication and durable ABORTED outcomes.
- A barrier-coordinated race submits opposite edges from the same starting catalog; exactly one commits, while the second validates against the newly committed edge and durably aborts.
- Verification: all 77 tests across 15 integration targets passed with direct Rust 1.95 compilation against the cached redb 4.4.0 artifact. Direct Clippy with warnings denied, rustfmt `--check`, and `git diff --check` passed.
- Broader DDL authority, dependency-graph, bootstrap, and catalog-index validation remain open; principal-object correspondence outside the principal records is not yet checked.

## SM-4 / DD-1 dependency: pinned catalog table schema loading — in progress

- Contract: [data dictionary](../../specs/data-dictionary.md), snapshot lookups and write-time dependency guards. Added a bounded catalog-family scan (65,536 records) and `load_catalog_table_schema`, which reads the table definition, discovers its column records, sorts/checks contiguous ordinals, resolves built-in type records, validates type profiles and parameters, and returns stable column/type IDs, nullability, default masks, default expression IDs, generation modes, schema version, and write epoch.
- Every table, matching column, and referenced type record read by the loader is added to the invocation's exact-byte commit guards. The real-redb fixtures validate unsorted record insertion, typed UUID/VARCHAR decoding, type parameters and defaults metadata; one then changes a referenced type and confirms the stale schema reader commits ABORTED with a conflict. Catalog-loaded INSERT now takes the bound schema value, uses its default-expression IDs/type IDs/default masks, and derives generated columns from generation modes. Its end-to-end fixture loads both schema and CHECK/default expressions from the pinned catalog before writing.
- Red evidence: the first real-redb schema fixture failed with `CorruptCatalog` because a non-null VARCHAR column was probed using NULL while validating type shape. The type-shape check now uses a nullable probe schema while retaining the catalog nullability in the returned schema; the focused regression passes.
- Spec decision: column discovery is a bounded family scan until catalog secondary indexes are implemented. DDL must increment table schema version atomically for column changes; DDL enforcement itself is still outstanding.
- Catalog commit now rejects any existing table column add/change/drop unless the table schema version advances atomically; it also rejects moving a column between tables. The regression fixture proves a new column cannot publish without a version update.
- Remaining: bounded family scan may fail closed for larger catalogs. Catalog loader does not yet derive grants/policies/role closure or bind policy expressions (later increments below address those parts). Full DDL, control-channel, gateway/sandbox IPC, long-reader lifecycle, and recovery acceptance remain open.
- Verification after this increment: all 88 tests across 16 integration targets pass via direct Rust 1.95 compilation against cached redb 4.4.0. Direct Clippy with warnings denied, rustfmt `--check`, and `git diff --check` pass. `cargo test --workspace --locked` still fails before project compilation because crates.io `config.json` is unreachable through the configured proxy.

## DD-1 / SM-5 dependency: constraint-definition commit validation — in progress

- Contract: [catalog payloads](../../specs/catalog-payloads.md) constraint kind invariants and [data dictionary](../../specs/data-dictionary.md) atomic catalog validation. A malformed FOREIGN KEY record with a nonempty source list and empty referenced list was accepted before the change (regression assertion returned `Ok(1)`).
- Catalog commit now validates kind-specific shape for changed constraints: NOT NULL, PRIMARY KEY, UNIQUE, CHECK, and FOREIGN KEY each require/forbid the corresponding expression, reference, backing index, column lists, action/match values, and VALID state. The catalog table write epoch fence remains required for existing tables.
- Regression: `engine/tests/commit.rs::catalog_commit_rejects_malformed_constraint_semantics` now proves malformed FK publication is rejected without advancing the commit sequence. Existing real-redb CHECK loader fixture covers a valid CHECK record.
- Limits: this increment does not yet resolve column ownership, referenced key/index definitions, CHECK dependency validity, or implement PK/UNIQUE/FK enforcement and delete/update cascades. Those remain required for DDL acceptance.
- Follow-up reference validation now resolves changed constraints against the transaction's final catalog view. It requires the owning table and local columns, checks NOT NULL against column nullability, validates CHECK expression purpose, verifies PK/UNIQUE backing-index table/columns/uniqueness/ownership/state, and confirms FK target constraint kind and ordered referenced columns. A structurally valid CHECK against a missing table also failed before implementation (`Ok(1)`) and is now rejected atomically.
- Invocation constraint loading now admits valid NOT NULL records when they name exactly one existing non-nullable table column; the real-redb fixture contains this case alongside CHECK and proves the combined catalog loads.
- Verification after both regression cases: 91 tests across 16 real-redb integration targets passed with direct Rust 1.95 compilation against cached redb 4.4.0. Direct Clippy with `-D warnings`, rustfmt `--check`, and `git diff --check` pass.
- Limits: CHECK dependency binding still occurs on invocation load, not all DDL publication paths. Constraint updates/deletions must preserve dependent constraints and indexes; PK/UNIQUE/FK runtime maintenance and FK delete/update behavior are not yet complete. DDL authorization remains unimplemented.
- Environment: direct Rust 1.95 compilation against cached redb 4.4.0 is available. `cargo test --workspace --locked` fails before compilation because crates.io `config.json` cannot be fetched through the configured proxy. TinyCC is not installed/on PATH, so C SDK and sandbox artifact acceptance cannot yet be run. These are validation/toolchain blockers, not design questions.
- Verification after constraint references and NOT NULL loading: 91 tests across 16 real-redb integration targets passed via direct Rust 1.95 compilation against cached redb 4.4.0. Direct Clippy with `-D warnings`, rustfmt `--check`, and `git diff --check` pass.
- Next: startup/bootstrap integrity validation, then an authenticated DDL control path.

## DD-2: table-scoped index-definition lookup — verified

- Replaced the invocation loader's global family-9 scan with a prefix lookup keyed by table ID and index ID. The lookup charges only matching entries against the invocation scan-work budget, caps candidates at the catalog-family limit, and verifies each derived entry against its authoritative family-9 record. For an empty prefix, the low-level loader keeps a bounded family-9 fallback for snapshots whose rows predate derived-index maintenance. Startup scans family 9 under the same bound and checks or rebuilds the derived index; catalog commits maintain it atomically with family-9 writes.
- Regression evidence: startup fixtures rebuild a missing table-index map and verify that a prefix lookup returns only the requested table's index. The real-redb named-index DDL integration test checks that creating an index publishes its table-index entry.
- Spec: [data dictionary](../../specs/data-dictionary.md) now describes the pinned lookup and startup consistency check.
- Verification: the focused startup rebuild and real-redb index DDL tests passed. `cargo build --workspace --locked`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo test --workspace --locked` passed. The workspace suite reports 4 sandbox fixtures ignored because they require delegated cgroups and built TinyCC fixtures.

## DD-2: table-scoped constraint lookup — verified

- Added `table_constraints_v1`, keyed by owning table and constraint ID. Family-8 catalog writes maintain it atomically; startup verifies or rebuilds it from authoritative records. CHECK/PRIMARY KEY/UNIQUE/NOT NULL loading now reads only the owning table's prefix, with a bounded family-8 fallback for pre-index low-level snapshots.
- Foreign-key validation gathers outgoing constraints from the same table prefix and incoming references through the existing reverse dependency index. Both candidate sets are charged against the invocation scan-work budget, and dependency reads enforce a family bound.
- Regression evidence: startup reconstructs a missing index and checks table-prefix isolation; real CHECK creation/drop verifies index maintenance; CHECK and foreign-key DDL plus all 73 transaction integration tests pass.
- Spec: [data dictionary](../../specs/data-dictionary.md) describes the table-scoped constraint and incoming-FK lookup paths.
- Verification: the startup prefix/rebuild test, CHECK create/drop integration, FK DDL integration, and all 73 transaction tests passed. `cargo build --workspace --locked`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo test --workspace --locked` passed. The suite reports 4 sandbox fixtures ignored because they require delegated cgroups and built TinyCC fixtures.

## DD-2: schema-scoped relation enumeration — verified

- Reused the maintained `relation_names_v1` schema prefix to enumerate relation IDs, validating each result against both its family-5 table row and parented family-2 relation object.
- Replaced global family-5 scans in authorized relation listing, administrative relation listing, and schema-usage grant loading. Matching candidates are charged to the shared invocation scan-work budget; empty-prefix low-level snapshots retain the bounded compatibility scan.
- Regression evidence: the startup stale-index rebuild fixture now enumerates the expected relation ID; authorized relation listing and administrative metadata authorization tests pass.
- Verification: focused startup, transaction, and commit tests passed. `cargo build --workspace --locked`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo test --workspace --locked` passed. The workspace test run reports 4 sandbox fixtures ignored because they require delegated cgroups and built TinyCC fixtures.

## DD-2: owner-scoped object lookup — verified

- Added `object_owners_v1`, keyed by owner principal and family-2 object ID. Family-2 catalog writes maintain the index atomically; startup checks or reconstructs it from authoritative objects.
- Non-administrator schema discovery now loads candidate objects through the admitted user's and roles' owner prefixes, verifies indexed IDs against family-2 owner fields, and charges the candidates to invocation scan work. Database administrators retain the bounded complete object scan to enumerate schemas across all owners.
- Regression evidence: startup reconstructs a missing index and checks owner-prefix limits and scope; real index DDL confirms the new index object's owner lookup. Existing administrative metadata authorization tests exercise schema discovery through the owner prefix.
- Verification: focused owner-index rebuild and DDL tests passed. `cargo build --workspace --locked`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo test --workspace --locked` passed. The workspace suite reports 4 sandbox fixtures ignored because they require delegated cgroups and built TinyCC fixtures.

## DD-1 / OP-1 hardening: coverage-guided catalog and DDL-codec fuzzing — verified slice

- Harness: [`catalog_ddl_codecs`](../../fuzz/fuzz_targets/catalog_ddl_codecs.rs) exercises the registered CAT1 family decoders, policy-target and stored-expression codecs, all bounded optimizer request decoders for SELECT/grouping/DELETE/UPDATE, and the corresponding single-table, DELETE, and UPDATE plan decoders. Each input is capped at `MAX_CATALOG_ENVELOPE_BYTES` (1 MiB).
- Corpus: independent valid database-record and default-expression CAT1 fixtures, standalone EIR1 current-principal expression bytes, and malformed CAT1 framing. No crash artifacts were produced; generated non-crash discoveries were not retained, leaving only the curated seeds checked in.
- Verification: helper-driven nightly fuzz build passed; the seeded campaign completed 1,662,361 executions in 31 seconds, reached 2,475 coverage counters / 4,145 features, and reported no crashes or sanitizer findings. Fuzz-target rustfmt, `cargo fmt --all -- --check`, locked workspace build, warning-denied Clippy, `git diff --check`, and the full serial delegated-cgroup workspace suite passed; the suite reported zero ignored tests.
- Remaining: longer continuous campaigns and minimizing any future crash into a deterministic regression remain required hardening work.

## 2026-11 delegated acceptance checkpoint

- The complete serial delegated-cgroup workspace suite passed with zero ignored tests. Catalog wire-fixture coverage passed (22 tests), startup catalog/index integrity coverage passed (54 tests), and transaction catalog loading and retained-snapshot coverage passed (75 tests).
- DD-5 long-reader reclamation pressure and continued fuzz campaigns remain open; this checkpoint does not change those plan statuses.

## DD-5: retained row/schema across version pressure — verified slice

- Extended `storage_manager_adds_column_and_backfills_existing_rows` to hold both a pre-DDL redb snapshot and a post-ADD snapshot of the backfilled default while committing 64 successive row versions. The old snapshot decodes its original schema/row, the post-DDL snapshot still decodes the stored `ready` default, and the current view decodes the newest row; a later nullable-column backfill preserves that value.
- Verification: the focused commit integration test and the complete serial delegated-cgroup workspace suite passed with zero ignored tests. `retained_reader_blocks_compaction_until_release_then_reclaims_deleted_pages` separately verifies redb compaction is blocked while a reader is held and reclaims pages after release. Longer-duration catalog DDL/read pressure remains open.

## 2026-11-16 integrated acceptance refresh

- The complete serial delegated-cgroup workspace suite passed with zero ignored tests, including 22 catalog wire-fixture tests, 54 startup catalog/index integrity tests, and 75 transaction tests. The updated DD-5 test passed with 64 successive row versions behind a pinned pre-DDL reader.
- Long-duration mixed catalog-DDL/read pressure and continued fuzz campaigns remain open.

## 2026-11 delegated acceptance checkpoint

- The complete serial delegated-cgroup workspace suite passed with zero ignored tests. Catalog wire-fixture coverage passed (22 tests), startup catalog/index integrity coverage passed (54 tests), and transaction catalog loading and retained-snapshot coverage passed (75 tests).
- DD-5 acceptance remains open at this historical checkpoint; see the later DD-5 closure evidence below. Continued fuzz campaigns remain open.

## DD-5: snapshot retention acceptance — verified

- Old snapshots retain definitions and rows across rename and drop (`schema_name_index_tracks_renames_per_snapshot`, `storage_manager_drop_table_removes_owned_catalog_and_rows`). The ADD COLUMN regression now pins both the pre-DDL shape and post-backfill default while 64 newer row versions commit; each snapshot decodes its own row layout.
- `retained_reader_blocks_compaction_until_release_then_reclaims_deleted_pages` verifies a pinned redb reader blocks compaction, preserves its view, and allows page reclamation after release.
- Verification: the complete serial delegated-cgroup workspace suite passed with zero ignored tests, including the updated commit regression and redb foundation retention/compaction tests. Longer-duration mixed catalog-DDL/read pressure remains additional hardening, not an unverified DD-5 contract item.

## DD-1 / OP-1: extended catalog and DDL codec fuzz campaign — verified slice

- The pinned `catalog_ddl_codecs` target completed a 300-second campaign with 19,914,102 executions and no crash or sanitizer finding. The run reached 2,937 coverage counters and 6,397 features; the accumulated 1,538-input corpus was minimized to 1,304 coverage-preserving inputs, while retaining the four named hand-authored seeds.
- The isolated nightly 1.101 compiler emitted a deprecation warning for `AtomicU64::fetch_update` in `gateway.rs`; the repository-pinned stable format, Clippy, build, and workspace test checks had passed independently. No production or Cargo dependency change was made for the fuzz run.
- Longer continuous campaigns and deterministic regressions for any future crash remain hardening work.

## DD-5: schema-name index under concurrent DDL pressure — verified slice

- `schema_create_drop_pressure_preserves_pinned_name_index_snapshots` keeps a pre-DDL reader open while 64 schemas are created and dropped. For every cycle, a fresh snapshot resolves the schema after creation and observes its removal after drop; the retained snapshot continues to observe all names as absent.
- A concurrent reader opens 128 snapshots and resolves all 64 names while the writer publishes the 128 DDL changes, exercising mixed secondary-index reads and catalog publication. The test also checks the create/drop sequence leaves every name absent in the current catalog.
- Verification: the focused real-redb integration test passed in the pinned Nix shell; `cargo fmt --all -- --check`, workspace Clippy with warnings denied, and `git diff --check` passed. The complete serial delegated-cgroup workspace suite passed with zero ignored tests, including all 179 commit integration tests, in 2m56s.
- Remaining: longer-duration mixed catalog-DDL/read campaigns and continued codec fuzzing remain hardening work.

## DD-2 / DD-5: object-owner index across schema ownership transfer — verified slice

- Extended `storage_manager_creates_schema_atomically_for_privileged_actor` to inspect `object_owners_v1` through both the retained pre-transfer snapshot and a fresh post-transfer snapshot. The old snapshot continues to list the schema under its original owner and excludes the successor; the current snapshot removes the former owner entry and lists the schema under the successor role.
- Verification: the focused real-redb regression passed. Workspace build, format check, warning-denied all-target Clippy, and `git diff --check` passed. The complete serial delegated-cgroup workspace suite passed with sandbox and process tests enabled and zero ignored tests: 291 engine unit tests, 179 commit tests, 75 transaction tests, and all other workspace targets.
- Remaining: longer-duration mixed catalog-DDL/read campaigns and continued codec fuzzing remain hardening work.

## DD-1 / DS-3: CHECK expressions bind to the final catalog schema — verified slice

- Shared final-catalog constraint validation now resolves every CHECK expression's table schema and declared result type, requires a row-local BOOLEAN expression, and runs `validate_expression_binding` against stable column IDs before publication. This applies to catalog writes outside the specialized CREATE TABLE validator as well.
- `catalog_commit_rejects_schema_mismatched_check_expression` builds an otherwise structurally valid CHECK whose EIR reads a BYTEA column while declaring BOOLEAN. The commit rejects it with `InvalidCatalog`, leaves the sequence unchanged, and records ABORTED.
- Verification: focused real-redb regression, locked workspace build, format check, warning-denied all-target Clippy, and the full serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored. Schema-aware CHECK validation at generic publication paths is no longer limited to invocation-time loading.

## DD-7: populated physical-format fixture covers principals, grants, and policies — verified slice

- Expanded the PRDB redb-v2 fixture generator to add a second NOLOGIN user, schema USAGE and table SELECT grants, and a reader-scoped row policy before copying all 20 logical engine tables. The fixture verifier now checks catalog families 10, 12, and 13 alongside the populated application row; the upgraded-image integration test checks those records survive redb v2-to-v3 conversion and redb 4.4 recovery.
- Regenerated the v2 and upgraded v3 fixture images and recorded their SHA-256 digests in the fixture README. The standalone fixture lockfile now includes the engine's direct redb 2.6.4 dependency, allowing regeneration with `--locked`.
- Verification: locked fixture verifier reported 3 principals, 2 grants, 1 policy, and 1 application row; the focused upgraded-image regression passed. Workspace build, formatting checks (including the standalone generator), warning-denied all-target Clippy, `git diff --check`, and the complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored.
- Remaining: longer-duration mixed catalog-DDL/read campaigns and continued codec fuzzing remain additional hardening; this generated logical fixture does not claim to be an archived prior-release engine binary/data directory.

## DD-7 / ID-3: startup rejects a principal without its typed object — verified slice

- Added `startup_rejects_principal_without_matching_identity_object`: after publishing a valid user principal/object pair, the fixture removes only its family-2 user object directly from redb. Persisted-record validation and `StorageManager::new` both reject the recovered catalog as `InvalidCatalog`.
- Verification: focused real-redb recovery regression passed. Workspace build, format check, warning-denied all-target Clippy, and the complete serial delegated-cgroup suite passed with sandbox/process tests enabled and zero ignored tests (293 unit tests, 186 commit tests, 76 transaction tests, plus all remaining targets).

## DD-6: table-schema cache remains bound to its invocation snapshot — verified slice

- Extended `storage_manager_adds_column_and_backfills_existing_rows` to pin a table definition in one invocation before ADD COLUMN, then load the new definition from a separate invocation with a different principal after DDL. The new invocation sees schema version 2 and the added stable column ID; reloading through the older invocation still returns its pinned schema version 1 and original column IDs.
- Verification: focused real-redb regression, locked workspace build, format check, warning-denied all-target Clippy, and the complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests: 291 unit tests, 183 commit tests, 75 transaction tests, and all remaining targets.

## DD-5: ADD COLUMN snapshots survive row-version churn and reclamation — verified slice

- Extended `storage_manager_adds_column_and_backfills_existing_rows` to hold pre- and post-ADD COLUMN redb snapshots while the backfilled row is overwritten 64 times. The retained snapshots still decode their respective old and new layouts, including the immutable default. After releasing all invocation and read handles, explicit redb compaction succeeds; startup validates the compacted catalog and the current row retains the final version and default.
- Verification: focused real-redb regression passed. Formatting, locked workspace build, warning-denied all-target Clippy, and `git diff --check` passed. The complete serial delegated-cgroup workspace suite passed with sandbox and process integration tests enabled and zero ignored tests.
- Remaining: longer-duration mixed catalog-DDL/read campaigns and continued codec fuzzing remain additional hardening work.

## DD-3 / DD-5: dropped relation name receives a new ID — verified slice

- Extended `storage_manager_drop_table_removes_owned_catalog_and_rows` to recreate the dropped table under the same schema/name. The recreated relation has a different ID; a snapshot pinned before DROP still resolves the original ID, while a fresh snapshot resolves the replacement. The drop and recreation outcomes are committed, and the original table's row is absent after recreation.
- Verification: the focused real-redb lifecycle regression passed. Workspace build, format check, warning-denied all-target Clippy, and the complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests. `git diff --check` passed.
- Remaining: other object families and wider mixed catalog-DDL/read pressure remain to be exercised; this slice does not claim complete DD-3 referential integrity coverage.

## DD-3 / DD-5: dropped index snapshots retain definitions and memberships — verified slice

- Extended `storage_manager_creates_named_unique_index_and_backfills_atomically` to retain a catalog snapshot across index DROP and same-name recreation. The retained snapshot still resolves the original index and contains its typed/object records and row-index memberships; the current snapshot resolves the replacement under a distinct ID.
- Verification: the focused real-redb lifecycle regression passed. The pinned release profile validated 10 engine binaries and one query fixture; workspace build, format check, warning-denied all-target Clippy, `git diff --check`, and the complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests.

## DD-2 / DD-3 / DD-5: mixed schema/table name-index pressure — verified slice

- Extended `schema_create_drop_pressure_preserves_pinned_name_index_snapshots`: while a reader repeatedly scans schema names, 64 iterations now create and drop both a schema and a table. Each iteration checks fresh schema/relation-name lookups before and after publication, while a retained redb snapshot continues to resolve the dropped table name.
- This adds mixed schema/table secondary-index writes and verifies relation-name snapshot retention alongside the existing schema-name checks. Longer-duration campaigns and continued codec fuzzing remain additional hardening work.
- Verification: focused real-redb pressure regression, locked workspace build, format check, warning-denied all-target Clippy, and the complete serial delegated-cgroup suite passed with sandbox/process tests enabled and zero ignored tests (293 unit, 188 commit, 76 transaction tests, plus all remaining targets).

## DD-2: grant indexes match an independent catalog scan — verified slice

- Strengthened `grant_indexes_track_grantee_and_object_per_snapshot` to scan the snapshot's family-12 catalog keys, filter them independently by grantee and object IDs, and compare the exact sorted logical keys with both secondary-index lookup results.
- Verification: focused real-redb regression, locked workspace build, format check, warning-denied all-target Clippy, `git diff --check`, and the complete serial delegated-cgroup workspace suite passed with sandbox and process tests enabled and zero ignored tests (293 unit, 189 commit, 76 transaction tests, plus all remaining targets).

## DD-2: membership indexes match an independent catalog scan — verified slice

- Strengthened `membership_indexes_track_both_directions_per_snapshot` to derive member-to-role and role-to-member ID lists from family-11 catalog keys and compare both index directions against that independent scan for retained and current snapshots after a concurrent membership addition.
- Verification: focused real-redb regression, locked workspace build, format check, warning-denied all-target Clippy, `git diff --check`, and the complete serial delegated-cgroup workspace suite passed with sandbox and process tests enabled and zero ignored tests (293 unit, 189 commit, 76 transaction tests, plus all remaining targets).

## DD-3 / DD-5: schema drop and same-name recreation retain distinct identities — verified slice

- Extended `storage_manager_creates_schema_atomically_for_privileged_actor` to pin the schema-name index before DROP, verify a fresh post-drop snapshot no longer resolves the retired name, recreate that name through the storage manager, and prove the new schema has a different stable ID while the pinned snapshot still resolves the original ID.
- Verification: focused real-redb schema lifecycle regression, locked workspace build, format check, warning-denied all-target Clippy, `git diff --check`, and the full serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests (293 unit, 188 commit, 76 transaction tests, plus all remaining targets).

## DD-2: policy and dependency indexes match independent catalog scans — verified slice

- Added a startup/index regression that compares policy lookup by table and operation with a direct scan of family 13, and compares outgoing and incoming dependency lookups with a direct family-15 scan in the same redb snapshot. The fixture includes different operations, tables, dependents, and referenced objects so nonmatching records are excluded from each result.
- Verification: focused index parity regression, locked workspace build, format check, warning-denied all-target Clippy, and `git diff --check` passed. The full serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests (294 unit tests, 190 commit tests, 76 transaction tests, 36 maintenance binary tests, plus all remaining targets).
- Remaining: DD-2 independent index parity for other families and broader DD-3 referential-integrity/pressure coverage remain open as recorded above.

## DD-2: relation-name indexes match independent catalog scans — verified slice

- Strengthened `stale_relation_name_index_is_rebuilt_from_catalog_records` to derive the expected schema/name lookup from family-2 object records and the relation enumeration from family-5 table records, independently of the rebuilt secondary indexes. Both the exact-name lookup and schema relation scan now compare against those catalog-derived results after stale entries are repaired.
- Verification: focused real-redb startup regression passed. Locked workspace build, format check, warning-denied all-target Clippy, `git diff --check`, and the full serial delegated-cgroup workspace suite passed with sandbox and process tests enabled and zero ignored tests (294 unit, 190 commit, 76 transaction tests, plus all remaining targets).
- Remaining: parity for additional secondary-index families and broader DD-3 referential-integrity/pressure coverage remain open.

## DD-2: column-name and ordinal indexes match independent catalog scans — verified slice

- Strengthened `stale_column_lookups_are_rebuilt_from_catalog_records` to scan family-6 column definitions and independently derive the expected ID by relation/name and relation/ordinal. Both rebuilt lookup directions are compared to those exact catalog-derived results.
- Verification: focused real-redb startup regression passed. Locked workspace build, format check, warning-denied all-target Clippy, `git diff --check`, and the full serial delegated-cgroup workspace suite passed with sandbox and process tests enabled and zero ignored tests (294 unit, 190 commit, 76 transaction tests, plus all remaining targets).
- Remaining: parity for additional secondary-index families and broader DD-3 referential-integrity/pressure coverage remain open.

## DD-2: index-name lookup matches an independent catalog scan — verified slice

- Strengthened `stale_index_name_lookup_is_rebuilt_from_catalog_records` to scan family-2 catalog objects, filter index objects by schema and name, and derive the expected index ID from each matching logical key. The repaired name lookup is compared against that independently derived result.
- Verification: focused real-redb regression, locked workspace build, format check, warning-denied all-target Clippy, and the full serial delegated-cgroup workspace suite passed with sandbox and process tests enabled and zero ignored tests (294 unit tests, 190 commit tests, 76 transaction tests, plus all remaining targets).
- Remaining: parity for additional secondary-index families and broader DD-3 referential-integrity/pressure coverage remain open.

## DD-2: object-owner scan matches an independent catalog scan — verified slice

- Strengthened `object_owner_scan_rebuilds_and_uses_the_owner_prefix` to scan family-2 objects and derive owner-matching IDs from their owner fields and logical keys, then compare the rebuilt owner scan with that independent result. The record-limit and rebuilt-index checks remain covered.
- Verification: focused real-redb regression, locked workspace build, format check, warning-denied all-target Clippy, and the full serial delegated-cgroup workspace suite passed with sandbox and process tests enabled and zero ignored tests (294 unit tests, 190 commit tests, 76 transaction tests, plus all remaining targets).
- Remaining: additional catalog scan families, DD-3 referential integrity, and broader catalog pressure remain open.

## DD-2: per-table index scan matches an independent catalog scan — verified slice

- Strengthened `table_index_scan_rebuilds_and_uses_the_table_prefix` to scan family-9 index definitions and derive the expected table-to-index ID list from each definition's table reference and logical key. The rebuilt per-table index scan is compared with this independent result; table-prefix isolation and record limits remain covered.
- Verification: focused real-redb regression, locked workspace build, format check, warning-denied all-target Clippy, and the full serial delegated-cgroup workspace suite passed with sandbox and process tests enabled and zero ignored tests (294 unit tests, 190 commit tests, 76 transaction tests, plus all remaining targets).
- Remaining: other catalog scan families, DD-3 referential integrity, and broader catalog pressure remain open.

## DD-2: per-table constraint scan matches an independent catalog scan — verified slice

- Strengthened `table_constraint_scan_rebuilds_and_uses_the_table_prefix` to scan family-8 constraint definitions and derive the expected table-to-constraint ID list from each record's table reference and logical key. The rebuilt constraint scan is compared against that independent result; table-prefix isolation and record limits remain covered.
- Verification: focused real-redb regression, locked workspace build, format check, warning-denied all-target Clippy, and the full serial delegated-cgroup workspace suite passed with sandbox and process tests enabled and zero ignored tests (294 unit tests, 190 commit tests, 76 transaction tests, plus all remaining targets).
- Remaining: other catalog scan families, DD-3 referential integrity, and broader catalog pressure remain open.

## DD-2: per-table column scan matches an independent catalog scan — verified slice

- Strengthened `table_column_scan_is_bounded_to_its_ordinal_prefix` to scan family-6 column definitions and derive the expected table-to-column ID list from each record's table reference and logical key. The per-table column scan is compared with that independent catalog result while unrelated secondary-index entries verify prefix isolation.
- Verification: focused real-redb regression, locked workspace build, format check, warning-denied all-target Clippy, `git diff --check`, and the full serial delegated-cgroup workspace suite passed with sandbox and process tests enabled and zero ignored tests (294 unit tests, 190 commit tests, 76 transaction tests, plus all remaining targets).
- Remaining: reconcile DD-2 status across all indexed families, then continue DD-3 referential integrity and broader catalog pressure coverage.

## DD-2: principal and schema name indexes match independent catalog scans — verified slice

- Strengthened the stale-name-index startup regressions to derive the expected `database_admin` principal ID from family-10 records and the `pg_catalog` schema ID from family-2 schema objects. After index repair, both exact-name lookup results are compared with those independent catalog scans.
- Verification: both focused startup regressions, locked workspace build, format check, warning-denied all-target Clippy, `git diff --check`, and the complete serial delegated-cgroup workspace suite passed with sandbox/process coverage enabled and zero ignored tests (294 unit, 190 commit, 76 transaction tests, plus all remaining targets).
- Remaining: audit the complete DD-2 matrix for any lookup still lacking independent catalog parity, then continue DD-3 referential integrity and broader catalog pressure coverage.

## DD-3 / DS-3: same-name index recreation preserves distinct catalog identity — verified slice

- Strengthened `storage_manager_creates_named_unique_index_and_backfills_atomically`: after DROP INDEX and same-name recreation, the current snapshot must contain only the new family-2 object and family-9 definition, the recreated backing-schema dependency, and the new table-index listing. The retained pre-drop snapshot continues resolving the original ID and definition.
- Verification: the focused real-redb lifecycle regression passed. Workspace formatting, locked build, warning-denied all-target Clippy, and the complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests.
- Scope: this verifies distinct live identity for named index drop/recreate and old-reader retention; it does not claim tombstone-based permanent reservation of dropped index IDs.

## DD-3 / DP-3: policy and expression records remain snapshot-distinct across name reuse — verified slice

- The policy DDL integration regression now drops a table-local row policy and recreates the same name. It verifies fresh policy/expression IDs and the replacement's dependency in the current catalog, confirms the table-operation policy index resolves the new ID only, and confirms a retained snapshot still resolves the old policy and expression records.
- Verification: the focused real-redb regression and complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests; format, locked build, warning-denied all-target Clippy, and `git diff --check` passed.

## DD-2: secondary catalog index parity audit — verified

- Audited the maintained catalog lookup indexes against the independent scans recorded above: principal/schema/relation names, column name and ordinal, index name, table-index, table-constraint, table-column, object-owner, membership in both directions, grants by grantee/object, policies by table/operation, and dependencies in both directions. Each has a real-redb parity or rebuild regression deriving expected IDs/keys from its owning catalog family.
- Verification: the full serial delegated-cgroup workspace suite passed on 2026-11-16 with sandbox/process tests enabled and zero ignored; the startup/index unit regressions and transaction/commit parity tests were included. Workspace build, format check, warning-denied all-target Clippy, TinyCC SDK tests/fixture, and `git diff --check` passed.
- Remaining: DD-3 referential-integrity breadth and longer mixed catalog-DDL/read pressure campaigns remain open; this audit does not claim complete integrity coverage.

## DD-2 / DD-3 / DD-5: doubled mixed catalog pressure campaign — verified slice

- Expanded `schema_create_drop_pressure_preserves_pinned_name_index_snapshots` from 64 to 128 schema/table create-drop cycles while a concurrent reader performs 256 rounds of schema-name lookups. Each cycle checks live schema and relation indexes, removes the table and schema, and verifies pinned snapshots still resolve their pre-drop names.
- Replaced the fixture's repeated-byte one-byte IDs with namespace-prefixed 64-bit counters after the larger campaign exposed an `OutcomeAlreadyExists` collision at the old ID boundary.
- Verification: the expanded focused real-redb test passed (3.38 s). Locked workspace build, format check, warning-denied all-target Clippy, and the full serial delegated-cgroup suite passed with sandbox/process coverage enabled and zero ignored tests (294 library tests, 191 commit tests, 77 transaction tests, plus all other targets). `git diff --check` passed; Cargo.lock is unchanged.
- Remaining: more catalog families and integrity relationships still need pressure coverage; the larger run does not close the complete DD-3 matrix.

## DD-3: typed table parents agree with explicit object ownership — verified slice

- A red startup regression showed that `validate_persisted_records` accepted a column whose typed record named table A while its object-parent record and kind-1 dependency named table B. Added a startup integrity check requiring any explicit parent on a typed column to agree with that typed record's table ID; absent optional parent records remain allowed. Index objects can be schema-parented, so their parent is not treated as the typed table ID.
- Added a commit-boundary regression for the same malformed column graph. Catalog publication now rejects it as `InvalidCatalog`, records an ABORTED outcome, and leaves every proposed record absent; startup independently rejects the recovered malformed graph.
- Verification: both focused real-redb regressions passed. The first full suite run exposed that the initial broader check incorrectly treated schema-parented indexes, constraints, and policies as table-parented; after narrowing the check to columns, the complete serial delegated-cgroup workspace suite passed with sandbox/process coverage enabled and zero ignored tests (294 unit, 193 commit, 77 transaction tests, plus all other targets). Locked workspace build, format check, warning-denied all-target Clippy, and `git diff --check` passed.

## DD-3: index object parent matches the typed table schema — verified slice

- Startup and final-catalog commit validation now reject a family-9 index whose family-2 object names an explicit parent schema different from the schema on the typed table record. Parentless indexes remain valid for records without an explicit schema parent. The data-dictionary specification records this relationship.
- Added commit-boundary and persisted-startup regressions for the mismatched-parent graph.
- Verification: focused regressions passed; all 56 startup tests and the complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests (294 library tests, 195 commit tests, 78 transaction tests, plus all remaining targets). Locked workspace build, format check, warning-denied all-target Clippy, and `git diff --check` passed. Cargo.lock is unchanged.

## DD-3 / CL-3: table object parent matches the typed table schema — verified slice

- Added final-catalog and persisted-catalog checks requiring an explicit table-object parent schema to agree with family-5's schema ID. This moves the relationship check into the common validator so read-only backup/recovery validation catches the same corruption as writable startup's derived-index rebuild.
- Added a commit-boundary test for atomic rejection and a regression that runs both read-only and writable startup validation against the malformed parent graph.
- Verification: focused real-redb regressions passed; all 57 startup tests and the complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests (294 library tests, 196 commit tests, 78 transaction tests, plus all remaining targets). Locked workspace build, format check, warning-denied all-target Clippy, and `git diff --check` passed. Cargo.lock is unchanged.

## DD-2 / DD-3 / DD-5: index-name and table-index snapshots under catalog pressure — verified slice

- Extended `schema_create_drop_pressure_preserves_pinned_name_index_snapshots` so each of its 128 schema/table cycles also creates and drops a named index. The test checks the index-name lookup and table-index scan before and after the drop in a retained snapshot, then checks both lookups are empty in a fresh snapshot.
- This adds repeated index-family catalog publication/removal and verifies that pinned dictionary readers retain the old index definition while current readers observe its removal.
- Verification: the focused real-redb pressure test passed (1 passed, 0 failed); `cargo fmt --all -- --check` and warning-denied all-target Clippy passed. The full delegated-cgroup workspace suite was not rerun for this test-only extension.

## DD-3 / DS-7: table removal during column-default alteration — verified slice

- Added a barrier race between a default-expression catalog update and table removal. The final-image checks require the table's typed column and any default expression to be absent when DROP TABLE commits, or to match the successful ALTER when the table survives; startup's persisted-catalog validator accepts the resulting graph and durable outcomes reflect each operation.
- Verification: focused real-redb race passed; formatting, warning-denied all-target engine Clippy, and `git diff --check` passed.

## 2026-11-16 full delegated-cgroup workspace verification

- The complete serial workspace test suite passed inside the delegated `systemd --user` service with sandbox and process integration tests enabled: 955 passed, 0 failed, 0 ignored. This includes the current permission, authorization/schema race, and metadata discovery regressions. The run used the locked release profile and documented TinyCC/runtime artifacts.

## DD-2 / DD-3 / DD-5: policy and key-constraint indexes under catalog pressure — verified slice

- Extended the 128-cycle schema/table/index pressure test to create and drop a row policy and a UNIQUE constraint with its owned backing index on each table. Pinned snapshots continue to resolve the removed policy and constraint/index lookup entries; fresh snapshots omit them. Final persisted-catalog validation checks that no removed expressions, targets, dependencies, or typed/object records remain orphaned.
- Verification: focused real-redb pressure test passed (8.08 s); warning-denied all-target engine Clippy, formatting, and `git diff --check` passed.

## 2026-11-16 complete delegated workspace verification after token rotation

- Ran `cargo test --workspace --locked -- --test-threads=1` inside a delegated `systemd --user` unit with the documented TinyCC fixtures and runtime profile. The full suite exited successfully with sandbox and process integration tests enabled and zero ignored tests. The authenticated metadata/admin process test exercised `prdb-client token-rotate`, rejection of the old credential, authentication by the saved replacement, and subsequent revocation.

## 2026-11-16 complete delegated workspace verification after rotation race and CLI preflight

- Re-ran `cargo test --workspace --locked -- --test-threads=1` inside the delegated `systemd --user` unit with the documented TinyCC fixtures and runtime profile. The full workspace exited successfully with sandbox/process integration tests enabled and zero ignored tests. The suite includes the token-rotation/staged-write race and the authenticated process test for rejecting occupied credential-output paths before mutation.

## 2026-11-16 final delegated workspace verification for current tree

- Ran `cargo test --workspace --locked -- --test-threads=1` in the delegated `systemd --user` unit with the documented artifacts and runtime allowlist. The command exited successfully; sandbox and cross-process integration tests were enabled, including `token_rotation_racing_a_staged_write_linearizes` and the token-rotation CLI occupied-output preflight. Zero tests were ignored.


## 2026-11-16 full delegated workspace verification after identity rotation probes

- `cargo test --workspace --locked -- --test-threads=1` passed inside the delegated `systemd --user` service with the documented TinyCC fixtures and runtime profile. Sandbox and process integration tests were enabled; zero tests were ignored. The run includes storage and authenticated ADMIN checks that unauthorized rotations of existing and unknown token IDs return the same generic response.

## DD-2 / DD-3 / DD-5: foreign-key lookup snapshots under catalog pressure — verified slice

- Extended the 128-cycle schema/table/index/policy/UNIQUE-constraint pressure campaign with a stable referenced key and a create/drop FOREIGN KEY on each temporary table. The retained reader continues to see the FK constraint and incoming dependency edge after drop; a fresh snapshot omits both while retaining the referenced target constraint, then omits all temporary constraints after their drops. Persisted-catalog validation checks the final graph.
- Verification: focused `schema_create_drop_pressure_preserves_pinned_name_index_snapshots` passed (12.47 s); format check, warning-denied all-target workspace Clippy, and `git diff --check` passed. The full workspace suite was not rerun for this pressure-test extension.


## 2026-11-16 full delegated workspace verification after identity issuance hardening

- `cargo test --workspace --locked -- --test-threads=1` passed inside the delegated `systemd --user` service with the documented TinyCC fixtures and runtime profile. Sandbox and process integration tests were enabled; zero tests were ignored. This run includes the unauthorized token issue/rotation enumeration regressions and the 128-cycle foreign-key catalog pressure regression.

## DD-2 / DS-9: composite index key parity across VARCHAR widening — verified slice

- Extended the widening acceptance from a single-column index to a composite `(VARCHAR, INTEGER)` index. The test verifies the pre-DDL physical key exists and is byte-identical when derived from the widened column definition, and confirms the existing index entry remains addressable without rebuilding it.
- Verification: focused `storage_manager_widens_varchar_atomically_and_keeps_index_current` passed; formatting, warning-denied all-target workspace Clippy, and `git diff --check` passed. The full workspace suite was not rerun for this test-only extension.

## 2026-11-16 DD-3 / DS-5: concurrent column ALTERs preserve a valid typed catalog — verified slice

- Added a real-redb race between storing a literal column default and creating the NOT NULL constraint for that column. The final nullable/default fields are checked against durable operation outcomes, and `validate_persisted_records` confirms the resulting typed catalog graph.
- Verification: the focused regression passed eight consecutive repeats; formatting, warning-denied workspace all-target Clippy, and `git diff --check` passed. The complete serial workspace suite passed in the delegated `systemd --user` service with sandbox and process tests enabled and zero ignored tests.

## 2026-11-16 DD-2 / DD-5: object-grant index under repeated catalog pressure — verified slice

- Extended `schema_create_drop_pressure_preserves_pinned_name_index_snapshots` with a table-scoped SELECT grant and revoke in each of its 128 lifecycle cycles. The grant lookup resolves the published record, a retained snapshot still returns it after revoke, and a fresh snapshot returns no grants for that table.
- Verification: the focused 128-cycle real-redb regression passed; format check, warning-denied all-target workspace Clippy, `git diff --check`, and the complete serial delegated-cgroup workspace suite passed with sandbox and process tests enabled and zero ignored tests.

## 2026-11-16 DD-2 / DD-5 / ID-3: membership indexes under repeated catalog pressure — verified slice

- Extended the same 128-cycle real-redb campaign with role membership and revocation for each temporary table cycle. The test updates the authoritative family-11 edge, dependency edge, and member epoch together; both member-to-role and role-to-member lookups resolve the edge, a retained snapshot preserves it across revoke, and a fresh snapshot omits it in both directions.
- The pressure fixture uses trusted catalog writes to isolate derived-index and snapshot behavior; authorized membership DDL remains covered by its separate storage and process regressions.
- Verification: the focused pressure regression passed; formatting, warning-denied all-target workspace Clippy, `git diff --check`, and the complete serial delegated-cgroup workspace suite passed with sandbox and process tests enabled and zero ignored tests.

## DD-3 / CL-2: catalog schema publication survives ambiguous sync failure — verified slice

- The Catalog lifecycle regression added in commit `41c1b6ac` reopens after an injected redb sync error during `CREATE SCHEMA`. It validates either the complete pre-publication image or the complete schema/object/audit/outcome publication and commit frontier; partial catalog/index state is rejected, and startup validation must accept the recovered image.
- Verification: the focused subprocess regression and complete serial delegated-cgroup workspace suite passed with sandbox and process tests enabled and zero ignored tests; workspace formatting and warning-denied all-target Clippy passed.
- Limit: this covers an injected sync error with a live kernel and filesystem, not power loss or device-cache behavior.

## DD-2 / DD-3: schema-name and relation indexes remain coherent during concurrent DDL — verified slice

- Added `schema_rename_racing_table_creation_preserves_catalog_identity`, which races a schema rename with table creation addressed by the schema's stable ID. It checks the schema-name index points only to the renamed schema, relation lookup agrees with the created table when creation commits, and every surviving table object retains the same schema parent.
- Verification: focused real-redb race passed; workspace formatting and warning-denied all-target Clippy passed; the complete serial delegated-cgroup workspace suite passed with sandbox and process tests enabled and zero ignored tests.

## 2026-11-16 delegated workspace verification after retained ADD COLUMN compaction check

- The complete serial `cargo test --workspace --locked -- --test-threads=1` suite passed inside the delegated `systemd --user` service. Sandbox and cross-process coverage were enabled; zero tests were ignored. This run revalidated catalog startup integrity, derived-index parity, retained-snapshot, and catalog-pressure regressions on the current tree.

## DD-5: retained ADD COLUMN defaults prevent early reclamation — verified slice

- Strengthened `storage_manager_adds_column_and_backfills_existing_rows`: the pre-ADD and post-ADD snapshots remain open through 64 row rewrites; the older snapshot decodes the original one-column tuple, while the post-ADD snapshot decodes the literal backfill default. Compaction must return `TransactionInProgress` while either snapshot is retained. After releasing both readers, compaction succeeds and the current three-column row and catalog validate.
- Verification: the focused real-redb regression, workspace build, formatting, warning-denied all-target Clippy, and complete serial delegated-cgroup suite passed. Sandbox and process tests were enabled; zero tests were ignored.
