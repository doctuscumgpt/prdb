# `prdb-client run-sql` Progress

## Implemented

- `run-sql` reads bounded UTF-8 SQL from a regular file or stdin, validates an explicit normalized schema name, and parses the canonical `PDB-SQL-PARAMETERS-1` JSON file into PSP1 invocation bytes.
- A generated query ABI v3 wrapper emits SQL and schema as numeric byte constants. SQL source injection is therefore not possible, and parameter values remain invocation data.
- The maintained SQL SDK now contains `prdb_sdk_sql_run_v1`, a PSP1 decoder, typed parameter metadata for Decimal scale, strict placeholder coverage checks, SELECT v2 execution, SQLR row writing, and statement-failure latching.
- The CLI uses `package_query` in a mode-0700 temporary workspace, finishes all local validation and packaging before reading credentials or connecting, and then uses the existing upload/seal/receipt/START/result lifecycle.
- TinyCC static and shared SDK tests exercise PSP1 decoding and malformed-block rejection. The shared linker explicitly emits both GNU and SysV hash tables because the PRDB manifest validator uses SysV hash metadata to verify query symbols.

## Resolved Decisions

- The dispatcher API is `prdb_sdk_sql_run_v1`; it routes only to SQL ABI v2 single-relation SELECT and preserves the existing parser/binder/executor behavior.
- Generated queries use native query ABI v3 for its statement-failure callback. The package profile remains explicit through `--sdk-library`, `--include-dir`, `--sdk-release`, `--cpu-baseline`, `--runtime-profile`, and `--compiler-identity`.
- Parameters use the PSP1 block over the existing invocation parameter channel. Decimal precision/scale remains separate from the generic value ABI because that record has no Decimal type metadata.
- Temporary package output is removed when the command returns. Retained diagnostic packages and profile auto-discovery remain deferred.

## Validation Evidence

- `nix develop --no-update-lock-file -c cargo test --workspace --locked -- --test-threads=1` passed inside the delegated `systemd --user` service with the documented fixtures/runtime dependencies, including `libgcc_s`.
- `nix develop --no-update-lock-file -c cargo build --workspace --locked` passed.
- `nix develop --no-update-lock-file -c cargo fmt --all -- --check` passed.
- `nix develop --no-update-lock-file -c cargo clippy --workspace --all-targets --locked -- -D warnings` passed.
- `nix develop --no-update-lock-file -c make -f engine/sdk/Makefile test` passed, including `sdk-sql-dispatch-v1` and shared SDK tests.
- `nix develop --no-update-lock-file -c make -f engine/sdk/Makefile release-profile` passed and validated the pinned Linux release profile.
- `nix develop --no-update-lock-file -c make -f engine/sdk/Makefile sandbox-query-fixture` passed.
- A command-level `run-sql` smoke check compiled and packaged `SELECT 1` through the generated wrapper and the maintained SDK; with a deliberately unused `127.0.0.1:1` endpoint, it then stopped at the expected connection-refused error before START.
- `git diff --check` passed.

The confined suite validates the existing SQL SDK, result relay, authorization, and lifecycle paths. A dedicated acceptance that starts `prdb-client run-sql` against a live test instance with a generated SQL wrapper remains a useful follow-up; this validation does not claim a new command-specific live-server test.
