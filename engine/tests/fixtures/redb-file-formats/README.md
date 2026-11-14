# redb physical-format fixtures

`redb-2.6.4-v2.redb` is a two-entry database created by the isolated generator in `v2-generator/` with redb 2.6.4. Its SHA-256 is `c6e592c61a417f50ac11a0b640a28553c95f78d7ba5fe0984d0e0894346ac020`. The generator's `verify-v2` operation reopens it with redb 2.6.4 and checks both key/value pairs. `redb-2.6.4-v3-upgraded.redb` is a copy upgraded with that release's `Database::upgrade()` API; its SHA-256 is `854a0247391df5ca3e9959f167f7e7e5ed5bf683301a529e44becf3932b371e5`.

`prdb-redb-2.6.4-v2.redb` is a populated PRDB READY image. The generator initializes it through the current engine, creates a user schema and table with a default expression, writes one application row, then copies all 20 logical engine tables into a physical v2 file. Its SHA-256 is `73555b1e799eb95f31856effae685bb1e40beac01a7e7ea7c8abc867b77ee20f`. `prdb-redb-2.6.4-v3-upgraded.redb` is its physical-format upgrade via `Database::upgrade()`; its SHA-256 is `7ed856f16d7194c8254b686c13687bded8f15355ade2ebc26393ef91a97419d1`. Bootstrap IDs are random, so regeneration creates a semantically equivalent image with different bytes.

Recreate and verify the fixture from the repository root with the pinned shell:

```sh
nix develop --no-update-lock-file -c cargo run --locked \
  --manifest-path engine/tests/fixtures/redb-file-formats/v2-generator/Cargo.toml \
  -- create-v2 /tmp/redb-2.6.4-v2.redb
nix develop --no-update-lock-file -c cargo run --locked \
  --manifest-path engine/tests/fixtures/redb-file-formats/v2-generator/Cargo.toml \
  -- verify-v2 /tmp/redb-2.6.4-v2.redb
nix develop --no-update-lock-file -c cargo run --locked \
  --manifest-path engine/tests/fixtures/redb-file-formats/v2-generator/Cargo.toml \
  -- create-populated-prdb-v2 /tmp/prdb-redb-2.6.4-v2.redb
nix develop --no-update-lock-file -c cargo run --locked \
  --manifest-path engine/tests/fixtures/redb-file-formats/v2-generator/Cargo.toml \
  -- verify-populated-prdb-v2 /tmp/prdb-redb-2.6.4-v2.redb
```

The pinned redb 4.4.0 read-only opener rejects both v2 fixtures with `UpgradeRequired(2)` and leaves their bytes unchanged. To recreate either upgraded image, copy the corresponding v2 file to a disposable path and run `upgrade-v2-to-v3` with the generator. The small fixture test checks both table values. The populated fixture test validates the complete PRDB READY catalog and verifies its application row after redb 4.4.0 recovery. These fixtures establish logical-record preservation for the redb physical upgrade API; they do not yet provide an operator-facing offline in-place upgrade workflow.

The upstream redb 4.4.0 changelog records removal of file-format v2 and directs users to `Database::upgrade()` in redb 2.6 to migrate to format v3: [redb 4.4.0 changelog](https://github.com/cberner/redb/blob/v4.4.0/CHANGELOG.md).
