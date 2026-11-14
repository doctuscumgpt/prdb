# redb physical-format fixtures

`redb-2.6.4-v2.redb` is a two-entry database created by the isolated generator in `v2-generator/` with redb 2.6.4. Its SHA-256 is `c6e592c61a417f50ac11a0b640a28553c95f78d7ba5fe0984d0e0894346ac020`. The generator's `verify-v2` operation reopens it with redb 2.6.4 and checks both key/value pairs. `redb-2.6.4-v3-upgraded.redb` is a copy upgraded with that release's `Database::upgrade()` API; its SHA-256 is `854a0247391df5ca3e9959f167f7e7e5ed5bf683301a529e44becf3932b371e5`.

Recreate and verify the fixture from the repository root with the pinned shell:

```sh
nix develop --no-update-lock-file -c cargo run --locked \
  --manifest-path engine/tests/fixtures/redb-file-formats/v2-generator/Cargo.toml \
  -- create-v2 /tmp/redb-2.6.4-v2.redb
nix develop --no-update-lock-file -c cargo run --locked \
  --manifest-path engine/tests/fixtures/redb-file-formats/v2-generator/Cargo.toml \
  -- verify-v2 /tmp/redb-2.6.4-v2.redb
```

The pinned redb 4.4.0 read-only opener rejects the v2 fixture with `UpgradeRequired(2)` and leaves its bytes unchanged; `bootstrap_binary` locks in that compatibility boundary. To recreate the upgraded fixture, copy the v2 file to a disposable path and run `upgrade-v2-to-v3` with the generator. The integration test opens the archived result read/write and read-only with redb 4.4.0 and verifies both values. This fixture covers physical redb compatibility only. It is not a populated PRDB instance or an operator-facing in-place upgrade workflow.

The upstream redb 4.4.0 changelog records removal of file-format v2 and directs users to `Database::upgrade()` in redb 2.6 to migrate to format v3: [redb 4.4.0 changelog](https://github.com/cberner/redb/blob/v4.4.0/CHANGELOG.md).
