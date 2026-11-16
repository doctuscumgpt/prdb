# redb physical-format fixtures

`redb-2.6.4-v2.redb` is a two-entry database created by the isolated generator in `v2-generator/` with redb 2.6.4. Its SHA-256 is `c6e592c61a417f50ac11a0b640a28553c95f78d7ba5fe0984d0e0894346ac020`. The generator's `verify-v2` operation reopens it with redb 2.6.4 and checks both key/value pairs. `redb-2.6.4-v3-upgraded.redb` is a copy upgraded with that release's `Database::upgrade()` API; its SHA-256 is `854a0247391df5ca3e9959f167f7e7e5ed5bf683301a529e44becf3932b371e5`.

`prdb-redb-2.6.4-v2.redb` is a populated PRDB READY image. The generator initializes it through the current engine, adds a NOLOGIN reader, creates a user schema and table with a default expression, grants the reader schema USAGE and table SELECT, creates a reader-scoped row policy, writes an application row, then copies all 20 logical engine tables into a physical v2 file. Its SHA-256 is `faf14b0856cba052229afd6f20b18f2be1db3ef1599a8eeb45bce45b2ebda2d0`. `prdb-redb-2.6.4-v3-upgraded.redb` is its physical-format upgrade via `Database::upgrade()`; its SHA-256 is `de06c8e832d13369f525ecc852e685d58bb6fb390619baca9f3908ee38e8d6d0`. Bootstrap IDs are random, so regeneration creates a semantically equivalent image with different bytes.

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

The pinned redb 4.4.0 read-only opener rejects both v2 fixtures with `UpgradeRequired(2)` and leaves their bytes unchanged. To recreate either upgraded image, copy the corresponding v2 file to a disposable path and run `upgrade-v2-to-v3` with the generator. The small fixture test checks both table values. The populated fixture verifier requires at least two principals, two grants, and one policy, and the upgraded-image integration test validates those catalog families and the application row after redb 4.4.0 recovery. The operator command `prdb-maintain redb-format-upgrade --instance-dir PATH --backup-file PATH --confirm-redb-v2` applies this transition to a managed READY instance. It creates or verifies an independent byte-for-byte backup, upgrades a same-directory private copy, compares all 20 logical tables, validates READY state, and atomically publishes the new physical format while retaining the v2 source at `.prdb-redb-upgrade.next`. See [catalog lifecycle](../../../../specs/catalog-lifecycle.md) for the full contract and retry behavior.

The upstream redb 4.4.0 changelog records removal of file-format v2 and directs users to `Database::upgrade()` in redb 2.6 to migrate to format v3: [redb 4.4.0 changelog](https://github.com/cberner/redb/blob/v4.4.0/CHANGELOG.md).
