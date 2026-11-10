# Data Dictionary Progress

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
