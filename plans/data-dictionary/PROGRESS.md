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
