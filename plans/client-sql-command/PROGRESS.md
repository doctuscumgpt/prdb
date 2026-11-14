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

## SQLCLI-5: generated `run-sql` through the production gateway — verified

- Contract: [SQL input command](../../specs/client-sql-command.md). The CLI must compile and package its generated wrapper locally, submit once through authenticated PREPARE/SEAL/START, emit complete SQLR data, and report success only after durable COMMITTED.
- Regression: `authenticated_metadata_and_admin_lifecycle_cross_gateway_and_supervisor_processes` starts `prdb-client run-sql` with SQL, typed parameter JSON, credential, and PSC1 files against the live gateway. It selects the restricted user's visible `empty_roundtrip` schema row from `information_schema.schemata`, validates the complete one-column SQLR record and committed terminal diagnostic, then checks the durable invocation state through STATUS using the receipt ID. It also submits malformed SQL, requires a nonzero CLI exit with no SQLR output and an ABORTED terminal diagnostic, then reconciles the second receipt through STATUS to the durable ABORTED outcome. A separate principal with explicit USAGE and SELECT grants on `admin_private` submits a query using a PSC1 file that contains only `empty_roundtrip`; the confined invocation aborts and its receipt reports ABORTED, proving request scope is not widened from SQL context or database grants. A one-shot TCP proxy then forwards PREPARE/SEAL and the single START to the production gateway but drops START's accepted response; the CLI exits without SQLR output, preserves its receipt, and the test reconciles that receipt through a separate authenticated STATUS connection to a terminal outcome. The proxy asserts exactly one START was sent.
- Red/green: the first process run failed during packaging because the installed release-profile SDK predated the SQL dispatcher symbol. Rebuilding `release-profile` refreshed the SDK; the same acceptance then passed through the confined executor and durable storage path.
- Verification: the focused delegated production-process test passed (`1 passed, 0 failed, 0 ignored`). Workspace build, format check, warning-denied Clippy, maintained TinyCC SDK tests, release-profile validation (10 engine binaries and one query fixture), and the delegated cgroup probe passed. The full serial delegated-cgroup workspace suite passed with sandbox and process tests enabled; no tests were ignored.
- Scope: covers one authorized information-schema SELECT with one TEXT parameter, malformed-SQL abort, rejection of a database-authorized relation outside the caller's PSC1 scope, and lost START acceptance reconciliation on the pinned Linux x86_64 profile. SQL DML remains deferred by the v1 contract; broader command-specific failure cases remain open.

## SQLCLI-1 local failures do not connect — verified follow-up

- Contract: invalid SQL input and malformed parameter files fail before credentials are used or a gateway connection is opened.
- Regression: a real `prdb-client run-sql` subprocess is pointed at a live local listener with valid credentials and otherwise complete CLI options. Whitespace-only, NUL-containing, invalid UTF-8, and over-limit SQL, plus a parameter file with an unknown key, each fail with the expected validation diagnostic; the listener confirms none attempted a TCP connection. A separately built SDK and valid package inputs with an injected failing TinyCC executable confirm compiler errors also occur before any gateway connection.
- Verification: the focused `client_package` process test passed. Workspace build, format check, warning-denied Clippy, maintained TinyCC SDK tests, release-profile validation, sandbox fixture build, delegated cgroup capability probe, and full serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests. The suite used the locked NixOS glibc 2.42 loader/libc and GCC 15.3 `libgcc_s` paths. `git diff --check` passed.
- Scope: covers empty, NUL-containing, invalid UTF-8, and over-limit SQL, strict parameter-object validation, and compiler failure at the CLI/network boundary. Scalar-canonicalization and other package-validation failures remain covered by their lower-level tests or need further command-level coverage.
