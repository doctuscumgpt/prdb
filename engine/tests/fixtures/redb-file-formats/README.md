# redb physical-format fixtures

`redb-2.6.4-v2.redb` is a two-entry database created by the isolated generator in `v2-generator/` with redb 2.6.4. Its SHA-256 is `c6e592c61a417f50ac11a0b640a28553c95f78d7ba5fe0984d0e0894346ac020`. The generator's `verify-v2` operation reopens it with redb 2.6.4 and checks both key/value pairs.

Recreate and verify the fixture from the repository root with the pinned shell:

```sh
nix develop --no-update-lock-file -c cargo run --locked \
  --manifest-path engine/tests/fixtures/redb-file-formats/v2-generator/Cargo.toml \
  -- create-v2 /tmp/redb-2.6.4-v2.redb
nix develop --no-update-lock-file -c cargo run --locked \
  --manifest-path engine/tests/fixtures/redb-file-formats/v2-generator/Cargo.toml \
  -- verify-v2 /tmp/redb-2.6.4-v2.redb
```

The pinned redb 4.4.0 read-only opener rejects this fixture with `UpgradeRequired(2)` and leaves its bytes unchanged; `bootstrap_binary` locks in that compatibility boundary. To exercise the upstream physical-format upgrade API on a disposable copy, run `upgrade-v2-to-v3` with the generator. A redb 4.4.0 read/write open then performs recovery; after it closes, redb 4.4.0 read-only open succeeds and the two values remain unchanged. This fixture covers physical redb compatibility only. It is not a populated PRDB instance or an operator-facing in-place upgrade workflow.

The upstream redb 4.4.0 changelog records removal of file-format v2 and directs users to `Database::upgrade()` in redb 2.6 to migrate to format v3: [redb 4.4.0 changelog](https://github.com/cberner/redb/blob/v4.4.0/CHANGELOG.md).
