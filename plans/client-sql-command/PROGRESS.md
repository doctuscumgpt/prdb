# `prdb-client run-sql` Progress

## Name-resolved CREATE SCHEMA ownership

- `admin-sql` accepts typed `AUTHORIZATION USER owner_name` and `AUTHORIZATION ROLE owner_name`; it resolves the identity over authenticated metadata and then uses the existing `schemas.create` operation. The explicit UUID owner form remains available.
- Verification: focused parser and authenticated TCP tests passed for both USER and ROLE resolution, as did the gateway/supervisor process lifecycle that creates and drops a schema owned by a renamed user. Locked workspace build, format check, warning-denied all-target Clippy, TinyCC SDK/fixture, delegated cgroup probe, and full serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests. `git diff --check` passed; Cargo.lock is unchanged.

## Default CREATE SCHEMA owner

- `admin-sql` now accepts `CREATE SCHEMA name` and omits the owner from the typed request; the authenticated server assigns the current actor as owner. UUID, USER, and ROLE clauses continue to work.
- Parser, protocol decoder, and authenticated request coverage verifies the owner field is omitted, so the server assigns the actor at START. The gateway/supervisor process lifecycle executes and removes a default-owned schema through `admin-sql`.
- Verification: focused tests, workspace build, format check, warning-denied Clippy, TinyCC SDK/fixture, and full serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests. `Cargo.lock` is unchanged.

## Untyped schema-owner resolution

- `CREATE SCHEMA ... AUTHORIZATION name` and `ALTER SCHEMA ... OWNER TO name` now use the authenticated, non-enumerating identity lookup for either users or roles. The server exposes the identity only if the caller has management authority for its resolved kind.
- Verification: focused parser/protocol/client/process tests passed. Locked workspace build, format check, warning-denied all-target Clippy, TinyCC SDK tests and sandbox fixture, and the complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests (269 library tests, 174 commit tests, 75 transaction tests; all other targets passed). The cgroup probe reported writable `cpu memory pids`; `git diff --check` passed and Cargo.lock is unchanged.

## CREATE TABLE column-level UNIQUE

- The bounded `admin-sql` parser accepts column-level `UNIQUE` and sends the same one-column typed unique-constraint definition used by table-level syntax. Parser coverage rejects duplicate inline/table definitions, and authenticated AdminClient plus CLI coverage exercise the inline form.
- Verification: the red/green parser regression, authenticated AdminClient request test, CLI recognition test, locked workspace build, format check, warning-denied all-target Clippy, TinyCC SDK tests and fixture, and full serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests. The suite reported 270 library tests, 174 commit tests, and 75 transaction tests, with all other targets passing. `git diff --check` passed and Cargo.lock is unchanged.

## CREATE TABLE column-level PRIMARY KEY — verified

- The bounded parser lowers a column-level primary key to the existing `primary_key_columns` request field and enforces NOT NULL. Duplicate inline/table-level declarations reject.
- Red/green parser coverage passes, and authenticated AdminClient and CLI tests use the inline form. Obsolete syntax-rejection assertions were removed while malformed and unsupported syntax checks remain.
- Verification: all 38 DDL parser tests, focused AdminClient/CLI tests, locked workspace build, format check, warning-denied all-target Clippy, TinyCC SDK tests/fixture, and the complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests (271 library, 174 commit, 75 transaction tests; all other targets passed). `git diff --check` passed and Cargo.lock is unchanged.

## CREATE TABLE column-level CHECK — verified

- The bounded parser accepts inline CHECK expressions that reference only their declared column and sends them through the existing typed EIR1 check definitions.
- Red/green parser coverage and authenticated AdminClient/CLI request coverage passed. Locked workspace build, format check, warning-denied all-target Clippy, TinyCC SDK tests/fixture, and complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests (272 library, 174 commit, 75 transaction tests; all other targets passed). The cgroup probe passed with writable `cpu memory pids`; `git diff --check` passed and Cargo.lock is unchanged.

## Implemented

- `CREATE ROW|CELL POLICY` now resolves recipients and `HAS_ROLE` names through a table/schema scoped identity read. Storage returns no identity for absent names or callers without database-admin, ownership, or scoped `MANAGE_POLICIES` authority. Policy DDL authority now exposes the target relation's declared shape through authenticated metadata; the process regression grants schema USAGE but no SELECT to the table owner.
- `DROP ROW|CELL POLICY name ON schema.table RESTRICT` resolves a matching kind/name only within the requested table and dispatches the existing `policies.drop` mutation. The delegated lifecycle creates and drops both policy kinds as a table owner. Locked build, format, warning-denied Clippy, TinyCC SDK tests/fixture, and the full delegated-cgroup workspace suite pass with zero ignored tests.
- Regression: the process lifecycle now proves CREATE and DROP resolve table and cell-column names for the owner without SELECT. Focused process test passed. Locked workspace build, format check, warning-denied Clippy, TinyCC SDK tests/fixture, and full delegated-cgroup workspace suite passed with zero ignored tests.
- Regression coverage: the delegated lifecycle also grants a non-owner scoped `MANAGE_POLICIES` on the table plus schema `USAGE`, without a data grant. The user resolves the relation and successfully creates/drops a row policy through `admin-sql`.
- Red/green: the manager lifecycle first failed with an unavailable metadata response; tracing exposed that the relation grant loader treated valid non-data privileges as catalog corruption. It now filters valid non-data privileges while retaining target-shape validation.
- Latest verification: focused production metadata process test passed. Locked workspace build, fmt check, warning-denied Clippy, TinyCC SDK tests, sandbox fixture and release-profile builds, cgroup capability probe, and full serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests. `git diff --check` passed; Cargo.lock is unchanged.

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

## SQLCLI-5: generated `run-sql` through the production gateway — verified for v1

- Contract: [SQL input command](../../specs/client-sql-command.md). The CLI must compile and package its generated wrapper locally, submit once through authenticated PREPARE/SEAL/START, emit complete SQLR data, and report success only after durable COMMITTED.
- Regression: `authenticated_metadata_and_admin_lifecycle_cross_gateway_and_supervisor_processes` starts `prdb-client run-sql` with SQL, typed parameter JSON, credential, and PSC1 files against the live gateway. It selects the restricted user's visible `empty_roundtrip` schema row from `information_schema.schemata`, validates the complete one-column SQLR record and committed terminal diagnostic, then checks the durable invocation state through STATUS using the receipt ID. It also submits malformed SQL, requires a nonzero CLI exit with no SQLR output and an ABORTED terminal diagnostic, then reconciles the second receipt through STATUS to the durable ABORTED outcome. A separate principal with explicit USAGE and SELECT grants on `admin_private` submits a query using a PSC1 file that contains only `empty_roundtrip`; the confined invocation aborts and its receipt reports ABORTED, proving request scope is not widened from SQL context or database grants. A one-shot TCP proxy then forwards PREPARE/SEAL and the single START to the production gateway but drops START's accepted response; the CLI exits without SQLR output, preserves its receipt, and the test reconciles that receipt through a separate authenticated STATUS connection to a terminal outcome. The proxy asserts exactly one START was sent.
- Red/green: the first process run failed during packaging because the installed release-profile SDK predated the SQL dispatcher symbol. Rebuilding `release-profile` refreshed the SDK; the same acceptance then passed through the confined executor and durable storage path.
- Verification: the focused delegated production-process test passed (`1 passed, 0 failed, 0 ignored`). Workspace build, format check, warning-denied Clippy, maintained TinyCC SDK tests, release-profile validation (10 engine binaries and one query fixture), and the delegated cgroup probe passed. The full serial delegated-cgroup workspace suite passed with sandbox and process tests enabled; no tests were ignored.
- Scope: covers one authorized information-schema SELECT with one TEXT parameter, malformed-SQL abort, rejection of a database-authorized relation outside the caller's PSC1 scope, and lost START acceptance reconciliation on the pinned Linux x86_64 profile. The separate SQL library acceptance in [`plans/sql-library/PROGRESS.md`](../sql-library/PROGRESS.md) verifies the DML/read sequence through bounded SDK calls in one confined invocation. It does not extend `run-sql` v1, which remains single-statement SELECT-only; future CLI DML or script support requires a versioned follow-up.

## SQLCLI-1 local failures do not connect — verified follow-up

- Contract: invalid SQL input and malformed parameter files fail before credentials are used or a gateway connection is opened.
- Regression: a real `prdb-client run-sql` subprocess is pointed at a live local listener with valid credentials and otherwise complete CLI options. Whitespace-only, NUL-containing, invalid UTF-8, over-limit, missing-file, and symlink SQL inputs, plus parameter files with an unknown key, a noncanonical Integer value (`042`), malformed BYTEA hex (`hex:0G`), and input larger than 1 MiB, fail with their expected diagnostics; the listener confirms none attempted a TCP connection. Separate subprocesses feed valid SQL through `--sql-file -` before checking malformed parameters, and verify empty EOF, NUL, invalid UTF-8, and over-limit stdin failures without contacting the listener. With valid SQL and a built SDK, malformed PSC1 scope fails during package validation as `InvalidScope`; requesting SDK release 2 against the supplied release-1 library fails as `InvalidConfiguration`. Both package rejections open no connection and leave no temporary files. A separately built SDK and valid package inputs with an injected failing TinyCC executable confirm compiler errors also occur before any gateway connection and remove the private build workspace.
- Scalar and boundary fixtures: a hand-checked PSP1 record set covers BOOLEAN, SMALLINT, INTEGER, BIGINT, DECIMAL, TEXT, VARCHAR, BYTEA, UUID, DATE, TIMESTAMPTZ, and typed NULL encoding. Invalid Boolean, integer-range/sign/zero padding, Decimal precision/scale/zero, VARCHAR character limit, BYTEA case, UUID form, and DATE/TIMESTAMPTZ range forms reject. SQL accepts exactly 65,536 bytes and rejects one byte more; parameter JSON rejects input larger than 1 MiB.
- Verification: the focused `client_package` process test passed. Workspace build, format check, warning-denied Clippy, maintained TinyCC SDK tests, release-profile validation (10 engine binaries and one query fixture), sandbox fixture build, delegated cgroup capability probe, and full serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests. The suite used the locked NixOS glibc 2.42 loader/libc and GCC 15.3 `libgcc_s` paths. `git diff --check` passed.
- Scope: covers SQL/parameter size boundaries, file and stdin input errors, no-follow regular-file behavior, canonical parameter forms across all scalar types, malformed-scope and incompatible-SDK package validation, temporary artifact cleanup on packaging errors, and representative input/compiler failures before network connection. Other package-validation cases remain open for command-level coverage.

## SQLCLI-3 adversarial SQL bytes — verified follow-up

- Contract: generated C keeps user SQL in numeric byte data so quoting, backslashes, high-bit UTF-8, and injection-shaped text cannot change the translation unit.
- Regression: `generated_wrapper_preserves_adversarial_utf8_sql_as_exact_numeric_data` checks the complete hand-specified SQL byte array for a query containing quote/backslash bytes, a forged `#include`, and `é`; it also verifies the fixed dispatcher call and that neither the injected directive nor UTF-8 text appears literally in the generated source.
- Verification: the focused generated-wrapper test, locked workspace build, format check, warning-denied Clippy, maintained TinyCC SDK tests, sandbox fixture build, release-profile validation, delegated cgroup probe, and full serial delegated-cgroup workspace suite all passed. The suite included sandbox/process tests with zero ignored tests; `git diff --check` passed.
- Scope: this proves exact source-byte embedding. Parameter-independent `query.so` output and compiler-environment secret isolation remain open in SQLCLI-3.

## SQLCLI-3 parameter/compiler isolation — verified follow-up

- Contract: changing invocation parameter values must not change the generated native query artifact. Parameter data and inherited secrets must not enter the TinyCC environment or generated source.
- Regression: `generated_query_artifact_is_independent_of_parameter_values` encodes two distinct PSP1 TEXT values, packages the same generated `SELECT $1` wrapper against the maintained SQL library, and requires byte-identical `query.so` artifacts with neither value present. The compiler-failure subprocess uses a capturing TinyCC shim, an inherited secret sentinel, and a parameter value; it verifies the compiler environment is cleared and the captured source contains no parameter bytes.
- Verification: both focused `client_package` process tests passed. Locked workspace build, format check, warning-denied Clippy, maintained TinyCC SDK tests, sandbox fixture build, release-profile validation (10 engine binaries and one query fixture), delegated cgroup probe, and the full serial delegated-cgroup workspace suite passed. The suite included sandbox/process tests with zero ignored tests; `git diff --check` passed.
- Scope: proves package independence for two canonical parameter values and isolates inherited compiler environment from one sentinel. It does not inspect arbitrary platform-specific environment channels or every parameter type.

## SQLCLI-2 / SQLCLI-5 reject multiple statements — verified follow-up

- Contract: [`client-sql-command.md`](../../specs/client-sql-command.md) requires exactly one SQL statement; the SDK rejects a second statement inside the confined executor, and the invocation aborts through the normal durable outcome path.
- Regression: the maintained v2 TinyCC parser fixture submits `SELECT id FROM t; SELECT id FROM t`, requires `SYNTAX_ERROR`, and checks the output AST remains unchanged. The production gateway/supervisor test invokes `run-sql` with two metadata SELECT statements, requires failure with no SQLR bytes and an `outcome=Aborted` diagnostic, then reads the invocation receipt and verifies durable `ABORTED` through authenticated STATUS.
- Finding: the v2 parser already rejected trailing tokens after its optional semicolon; the gap was missing explicit acceptance coverage, not missing parser behavior.
- Verification: the focused TinyCC parser fixture and focused delegated process test passed. Locked workspace build, format check, warning-denied Clippy, complete maintained TinyCC SDK suite, sandbox fixture build, release-profile validation, delegated cgroup probe, and the full serial delegated-cgroup workspace suite passed. The full suite included sandbox/process tests with zero ignored tests; `git diff --check` passed.
- Scope: verifies a second statement is rejected within the confined v1 command path and cannot yield a committed result. It does not expand the SQL grammar.

## SQLCLI-1 patchelf failure does not connect — verified follow-up

- Contract: local packaging failures must finish before credentials are used or a gateway connection is opened, and temporary package output must be removed.
- Regression: the `run_sql_local_failures_do_not_connect_and_gateway_failures_clean_workspace` process test supplies valid SQL, parameters, scope, and SDK inputs, then injects a failing `patchelf` executable after TinyCC compilation/linking. It requires a `CompilerFailed` diagnostic, confirms the listener received no connection, and checks the private workspace is empty.
- Verification: the focused process test passed. Locked workspace build, format check, warning-denied Clippy, maintained TinyCC SDK tests, sandbox fixture build, release-profile validation (10 engine binaries and one query fixture), delegated cgroup probe, and the full serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests. `git diff --check` passed.
- Scope: covers one post-link package finalization failure and confirms cleanup/no-connect behavior. Other packaging error paths remain open for command-level coverage.

## SQLCLI-3 shared linker environment isolation — verified follow-up

- Contract: client credentials and SQL parameter values stay out of every local build tool's inherited environment; shared-link failure remains local and cleans temporary output.
- Regression: the `run_sql_local_failures_do_not_connect_and_gateway_failures_clean_workspace` process test injects a sentinel into the CLI environment and uses a failing shared-linker shim to record whether it inherits that sentinel. It also requires the expected `CompilerFailed` diagnostic, no gateway connection, and an empty private workspace. Removing `env_clear()` from the linker invocation made the regression fail with the captured value `leaked`; restoring it made the test pass.
- Specification: clarified that TinyCC, the shared linker, and `patchelf` run with inherited environment variables cleared.
- Verification: focused process test passed. Workspace build, format check, warning-denied Clippy, maintained TinyCC SDK tests, sandbox fixture build, release-profile validation (10 engine binaries and one query fixture), and the full serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests. `git diff --check` passed.
- Scope: verifies the shared linker subprocess against one inherited environment sentinel. TinyCC environment isolation is covered in the parameter/compiler isolation entry above; `patchelf` environment isolation is covered in the next entry.

## SQLCLI-3 `patchelf` environment isolation — verified follow-up

- Contract: the ELF rpath adjustment tool receives no inherited credentials or other parent environment variables; failure remains local and temporary output is cleaned.
- Regression: the existing `run_sql_local_failures_do_not_connect_and_gateway_failures_clean_workspace` process test now injects a sentinel and uses a failing `patchelf` shim to record its environment. It requires the sentinel to be absent, the `CompilerFailed` diagnostic, no gateway connection, and no private workspace artifacts. Removing `env_clear()` from the `patchelf` command made the test fail with captured value `leaked`; restoring it made the test pass.
- Verification: focused process test passed. Workspace build, format check, warning-denied Clippy, maintained TinyCC SDK tests, sandbox fixture build, release-profile validation (10 engine binaries and one query fixture), delegated cgroup probe, and the full serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests. `git diff --check` passed.
- Scope: verifies `patchelf` against one inherited environment sentinel. Other environment sources and host-specific tool behavior remain outside this check.

## SQLCLI-1 workspace cleanup after gateway failure — verified follow-up

- Contract: after local compilation and packaging succeed, any ordinary submission error still removes the generated package while preserving the caller-owned receipt behavior.
- Regression: `run_sql_local_failures_do_not_connect_and_gateway_failures_clean_workspace` starts `run-sql` with valid SQL, parameters, scope, credentials, and SDK inputs. A local TCP listener accepts and immediately closes the authenticated submission connection, forcing a post-packaging failure; the command exits unsuccessfully and the test requires the private workspace to be empty. The same process test retains the earlier no-connect and cleanup assertions for local input/package failures.
- Verification: focused process test passed. Workspace build, format check, warning-denied Clippy, maintained TinyCC SDK tests, sandbox fixture build, release-profile validation (10 engine binaries and one query fixture), and the full serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests. `git diff --check` passed.
- Scope: covers workspace removal after a connection closes during submission. It does not simulate process termination or signals.

## SQLCLI-3 zeroize PSP1 parameters on local errors — implemented

- Contract: SQL parameter bytes are invocation data, never compiler input; the client clears its PSP1 buffer after submission and when any earlier packaging or workspace error returns.
- Finding: `submit_package_and_stream` cleared the mutable parameter slice, but `run_sql` used `?` for workspace creation and package compilation before entering that function. Those local errors dropped a plain `Vec<u8>` without zeroization.
- Implementation: `read_sql_parameters` now returns `Zeroizing<Vec<u8>>`; its temporary raw JSON input is also zeroizing. The PSP1 buffer is therefore cleared on submission completion and on every earlier return path, while `submit_package_and_stream` retains its explicit clear after network use.
- Specification: documented the PSP1 buffer's all-return-path zeroization behavior.
- Verification: the focused `run-sql` local/package/gateway-failure process test passed. Locked workspace build, format check, warning-denied Clippy, complete maintained TinyCC suite, sandbox fixture build, release-profile validation (10 engine binaries and one query fixture), and the full serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests. `git diff --check` passed.
- Scope: covers the PSP1 byte buffer and raw JSON file buffer owned by `run_sql`. It does not claim to wipe parser-internal temporary string allocations.

## SQLCLI-3 zeroize parameter-encoding intermediates — implemented

- Contract: parameter values remain runtime-only data, including while the client parses and converts canonical JSON into PSP1; temporary encoded copies are cleared if a later value fails validation.
- Red evidence: the new `parsed_parameter_json_tree_zeroizes_nested_strings` regression first failed to compile because `ResponseValue` had no `Zeroize` implementation.
- Implementation: `ResponseValue` recursively zeroizes strings and object keys and clears nested arrays/maps. The SQL encoder owns the parsed tree, per-value payloads, and partial record bytes through `Zeroizing`; it now returns the final PSP1 vector in a zeroizing owner too. `run_sql` retains zeroizing raw-JSON and PSP1 ownership across every local and submission return path.
- Specification: documented zeroization of the parsed tree and temporary payload/record buffers after valid JSON parsing.
- Verification: all 10 `client_sql` unit tests passed; the parameter-independent artifact process test and the local/package/gateway-failure process test passed. Workspace build, format check, warning-denied Clippy, complete maintained TinyCC suite, sandbox fixture build, release-profile validation (10 engine binaries and one query fixture), delegated cgroup probe, and full serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests. `git diff --check` passed.
- Scope: covers owned parameter buffers after successful JSON parsing and the final encoded block. Temporary allocations made internally while malformed JSON is being parsed are not claimed to be wiped.

## SQLCLI-3 zeroize malformed-JSON parser intermediates — implemented

- Contract: parameter JSON is runtime secret input; decoded prefixes must be cleared if a later syntax, Unicode, duplicate-key, or trailing-input error rejects the document.
- Implementation: JSON string decoding now owns its byte buffer through `Zeroizing`, including invalid-UTF-8 conversion failures. The bounded response-value parser holds partially built arrays, maps, keys, values, and the root under zeroizing owners, and transfers them only after the complete JSON value is accepted.
- Regression: extended the strict response decoder test with a nested partial string followed by malformed syntax and an invalid surrogate pair; both continue to fail as malformed input. The zeroization itself has no caller-visible output, so the regression checks the rejection contract while the ownership guards provide the cleanup guarantee.
- Specification: documented clearing of partial decoded strings and nested values on malformed parameter JSON.
- Verification: the focused response-decoder unit test passed. Locked workspace build, format check, warning-denied Clippy, complete maintained TinyCC SDK suite, sandbox query fixture build, and release-profile validation (10 engine binaries and one query fixture) passed. The delegated cgroup probe reported a writable cgroup v2 parent with `cpu memory pids` available and enabled. The full serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests. `git diff --check` passed.
- Scope: this covers allocations owned by the shared bounded JSON parser and `ResponseValue` tree. The original input remains the caller's responsibility; `run-sql` owns its input in a zeroizing buffer.


## Administrative SQL CLI: authenticated CREATE TABLE — verified

- Contract: [client SQL command](../../specs/client-sql-command.md), [client SDK](../../specs/client-sdk.md), and [schema DDL](../../specs/ddl-schema.md). `prdb-client admin-sql` reads a bounded SQL file or stdin, parses supported CREATE TABLE syntax before credentials/network, and routes only the typed table definition over control mode. `run-sql` remains the confined query path.
- Implementation: command dispatch validates via `parse_create_table`, keeps SQL bytes in a zeroizing owner, authenticates with `AdminClient`, then uses `execute_create_table_sql`. Success reports only generated IDs, column count, and commit sequence.
- Regression: the process test drives HELLO/AUTH, paged schema lookup, `tables.create` PREPARE/START, and committed response through a mock TCP control service and verifies the CLI summary.
- Verification: `nix develop -c cargo test -p prdb-engine --test client_package admin_sql_cli --locked --offline` passed both CLI tests. `nix develop -c cargo fmt --all -- --check`, warning-denied workspace Clippy, locked workspace build, and `git diff --check` passed. The complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests.
- Next smallest behavior: expose CREATE SCHEMA SQL with an explicit owner clause and the same pre-network parser validation.

## Administrative SQL CLI: generated UUID identity — implemented

- Contract: [client SQL command](../../specs/client-sql-command.md), [client SDK](../../specs/client-sdk.md), and [schema DDL](../../specs/ddl-schema.md). Identity remains a typed control operation and does not alter the confined query path.
- Regression: the CLI process test submits `UUID GENERATED ALWAYS AS IDENTITY`, asserts the typed request flag, and receives a committed four-column table result.
- Verification: both `admin-sql` process tests passed, including authenticated typed identity submission and pre-auth rejection of unsupported SQL. Workspace formatting, locked offline build, warning-denied all-target Clippy, and the complete serial delegated-cgroup workspace suite passed; sandbox and process tests ran with zero ignored tests. Parser and AdminClient coverage is recorded in [schema DDL progress](../ddl-schema/PROGRESS.md).

## Administrative SQL CLI: explicit-owner CREATE SCHEMA — implemented

- Contract: [client SQL command](../../specs/client-sql-command.md), [client SDK](../../specs/client-sdk.md), and [schema DDL](../../specs/ddl-schema.md). `admin-sql` accepts one bounded `CREATE SCHEMA name AUTHORIZATION 'uuid'` statement and forwards only `schemas.create` over authenticated control mode.
- Regression: process coverage authenticates, verifies the typed schema name and owner ID in PREPARE, completes START, and checks successful CLI output. Unsupported SQL remains rejected before credential access/network.
- Verification: all three `admin-sql` process tests passed, including explicit-owner schema creation and rejection before network. Workspace formatting, locked offline build, warning-denied all-target Clippy, and the complete serial delegated-cgroup workspace suite passed; sandbox and process tests ran with zero ignored tests. Parser and AdminClient evidence is in [schema DDL progress](../ddl-schema/PROGRESS.md).

## Administrative SQL CLI: schema rename and restricted drop — implemented

- Contract: [client SQL command](../../specs/client-sql-command.md), [client SDK](../../specs/client-sdk.md), and [schema DDL](../../specs/ddl-schema.md). The command accepts schema rename and `DROP ... RESTRICT` statements and sends only typed authenticated control operations.
- Regression: parser cases cover both statements and fail-closed handling of CASCADE and multiple statements. AdminClient process-boundary protocol tests cover metadata resolution, PREPARE/START, and committed results.
- Verification: all seven parser tests and the authenticated AdminClient TCP rename/drop regression passed. Workspace formatting, locked offline build, warning-denied all-target Clippy, and the complete serial delegated-cgroup workspace suite passed; sandbox and process tests ran with zero ignored tests. See [schema DDL progress](../ddl-schema/PROGRESS.md).

## Administrative SQL CLI: table rename and restricted drop — implemented

- Contract: [client SQL command](../../specs/client-sql-command.md), [client SDK](../../specs/client-sdk.md), and [schema DDL](../../specs/ddl-schema.md). Qualified names resolve through authenticated metadata and invoke the existing table lifecycle operations.
- Regression: parser tests verify single-statement qualified rename and RESTRICT drop. AdminClient TCP coverage verifies schema/relation metadata resolution, typed PREPARE requests, and committed responses for both operations.
- Verification: all eight parser tests and the authenticated AdminClient TCP rename/drop regression passed after resolving paged metadata. Workspace formatting, locked offline build, warning-denied all-target Clippy, and the complete serial delegated-cgroup workspace suite passed; sandbox and process tests ran with zero ignored tests. See [schema DDL progress](../ddl-schema/PROGRESS.md).

## Administrative SQL CLI: table-level PRIMARY KEY — implemented

- Contract: [client SQL command](../../specs/client-sql-command.md), [client SDK](../../specs/client-sdk.md), and [schema DDL](../../specs/ddl-schema.md). `admin-sql` accepts a final table-level `PRIMARY KEY (columns...)` in CREATE TABLE, validates it before credentials/network, and sends the structured column names through authenticated control mode.
- Regression: the process-level CLI test verifies the PK field in PREPARE; parser regressions cover composite keys and rejection of unknown/duplicate columns.
- Verification: focused CLI test, locked workspace build, format check, warning-denied Clippy, full serial delegated-cgroup workspace suite with sandbox/process tests enabled and zero ignored tests, and `git diff --check` passed.

## Administrative SQL CLI: table-level UNIQUE — implemented

- Contract: [client SQL command](../../specs/client-sql-command.md), [client SDK](../../specs/client-sdk.md), and [schema DDL](../../specs/ddl-schema.md). `admin-sql` accepts multiple final table-level UNIQUE clauses, rejects unknown or repeated columns before credentials/network, and sends nested typed name arrays over authenticated control mode.
- Regression: the CLI process test checks the nested UNIQUE request alongside its PRIMARY KEY; parser coverage verifies nullable/composite UNIQUE and rejects malformed definitions.
- Verification: focused CLI test, locked workspace build, format check, warning-denied Clippy, full serial delegated-cgroup workspace suite with sandbox/process tests enabled and zero ignored tests, and `git diff --check` passed.

## Administrative SQL CLI: ALTER TABLE PRIMARY KEY and UNIQUE — implemented

- Contract: [client SQL command](../../specs/client-sql-command.md), [client SDK](../../specs/client-sdk.md), and [schema DDL](../../specs/ddl-schema.md). `admin-sql` accepts qualified `ADD PRIMARY KEY (...)` and `ADD UNIQUE (...)`, resolves columns through authenticated metadata, and submits typed constraint DDL.
- Regression: parser tests reject unsupported `ADD CONSTRAINT name`, unqualified names, duplicate columns, and multiple statements. The CLI process boundary confirms this supported form passes syntax validation before credential loading/network; AdminClient TCP coverage verifies the typed operation and commit response.
- Verification: focused parser, TCP, and CLI tests, locked workspace build, format check, warning-denied Clippy, full serial delegated-cgroup workspace suite with sandbox/process tests enabled and zero ignored tests, and `git diff --check` passed.

## Administrative SQL: bounded ALTER TABLE CHECK — implemented

- `admin-sql` recognizes one qualified `ALTER TABLE schema.table ADD CHECK (...)` statement before credential access and routes it to the authenticated typed CHECK control operation.
- The bounded parser accepts Boolean column references, `IS [NOT] NULL`, `NOT`, `AND`, `OR`, and parentheses, rejecting comparison/literal syntax and multiple statements.
- Parser regressions cover precedence and fail-closed syntax; CLI process coverage confirms supported syntax passes local recognition without making a network request when credentials are absent.
- Verification: focused parser, AdminClient TCP, and CLI process tests passed. Locked workspace build, format check, warning-denied all-target Clippy, full serial delegated-cgroup workspace suite (sandbox/process tests enabled), and `git diff --check` passed.

## Administrative SQL: ALTER COLUMN SET/DROP NOT NULL — implemented

- `admin-sql` accepts qualified `ALTER TABLE ... ALTER COLUMN ... SET NOT NULL` and `DROP NOT NULL`, resolving the column before typed dispatch.
- Parser, authenticated AdminClient TCP, and CLI pre-credential recognition coverage added; malformed and multi-statement input fails closed.
- Verification: focused parser, AdminClient TCP, and CLI process tests passed. Locked workspace build, format check, warning-denied all-target Clippy, complete serial delegated-cgroup workspace suite with sandbox/process tests enabled, and `git diff --check` passed.

## Administrative SQL: ALTER TABLE ADD COLUMN — implemented

- `admin-sql` accepts one qualified `ADD COLUMN` with supported scalar type, optional NOT NULL, and optional typed literal default.
- Parser checks nullability/default rules and fail-closed statement boundaries. Authenticated client coverage verifies table lookup and typed `tables.add_column` dispatch; CLI process coverage verifies local recognition.
- Verification: focused parser, AdminClient TCP, and CLI process tests passed. Locked workspace build, format check, warning-denied all-target Clippy, full serial delegated-cgroup workspace suite with sandbox/process tests enabled, and `git diff --check` passed.

## Administrative SQL: RENAME COLUMN — implemented

- `admin-sql` accepts qualified `ALTER TABLE schema.table RENAME COLUMN old_name TO new_name` and dispatches the existing typed operation after metadata resolution.
- Parser tests reject malformed and multiple statements; AdminClient TCP coverage checks ID binding; CLI process coverage verifies syntax recognition before credentials/network.
- Verification: focused parser, AdminClient TCP, and CLI process tests passed. Locked workspace build, format check, warning-denied all-target Clippy, full serial delegated-cgroup workspace suite with sandbox/process tests enabled, and `git diff --check` passed.

## Administrative SQL: DROP COLUMN RESTRICT — implemented

- `admin-sql` accepts qualified `ALTER TABLE schema.table DROP COLUMN column RESTRICT`; CASCADE and implicit drop modes remain rejected.
- Parser, AdminClient TCP binding, and CLI pre-credential recognition coverage added.
- Verification: focused parser, AdminClient TCP, and CLI process tests passed. Locked workspace build, format check, warning-denied all-target Clippy, full serial delegated-cgroup workspace suite with sandbox/process tests enabled, and `git diff --check` passed.

## Administrative SQL: CREATE TABLE CHECK — implemented

- `admin-sql` accepts table-level CHECK clauses using Boolean column references, `IS [NOT] NULL`, Boolean operators, parentheses, all six comparison operators, and typed scalar literals. It rejects type mismatches and lossy DECIMAL rescaling.
- SQL names bind to declared columns and the typed create request carries canonical EIR1 so table and CHECK catalog records publish atomically.
- Parser, control decoder, AdminClient, CLI, and storage regressions passed; locked workspace build, format check, warning-denied all-target Clippy, full serial delegated-cgroup workspace tests (including sandbox/process tests with zero ignored), maintained TinyCC SDK tests, sandbox fixture build, and `git diff --check` passed.


## Administrative SQL: typed CHECK comparisons and literals — implemented

- `admin-sql` accepts typed comparisons and literals in CREATE TABLE and ALTER TABLE CHECK clauses and rejects incompatible types before PREPARE.
- Literals are contextual to compared columns; DECIMAL coefficients are rescaled exactly to the column scale.
- Focused parser, authenticated client, CLI, expression, and storage regressions passed. Locked workspace build, format check, warning-denied all-target Clippy, complete serial delegated-cgroup workspace suite (sandbox/process tests enabled; zero ignored), and `git diff --check` passed.


## Administrative SQL: foreign-key create/drop — implemented

- `admin-sql` recognizes qualified ADD FOREIGN KEY and DROP CONSTRAINT by generated ID. Metadata binding requires a visible ordered PRIMARY KEY or UNIQUE target and confirms drop ownership by source table.
- Parser, authenticated AdminClient create/drop, and CLI pre-credential regressions passed. Locked workspace build, format check, warning-denied all-target Clippy, complete serial delegated-cgroup workspace suite (sandbox/process tests enabled; zero ignored), and `git diff --check` passed.


## admin-sql: atomic CREATE TABLE FOREIGN KEY definitions — implemented

- The existing `CREATE TABLE` command accepts multiple table-level `FOREIGN KEY (...) REFERENCES schema.table (...)` clauses and routes them through `AdminClient` metadata binding and one typed create operation.
- Regression coverage verifies parser constraints, authenticated metadata/typed request binding, and atomic storage publication. No obsolete tests were identified; negative parser tests still cover unsupported/malformed input.
- Focused parser, authenticated client, and storage checks passed. Locked workspace build, formatting, warning-denied all-target Clippy, TinyCC SDK tests and sandbox fixture build, and the complete serial delegated-cgroup workspace suite passed with zero ignored tests; `git diff --check` passed.


## admin-sql: ALTER COLUMN SET/DROP DEFAULT — implemented

- Added bounded single-statement parsing for `ALTER TABLE schema.table ALTER COLUMN column SET DEFAULT literal` and `DROP DEFAULT`, including fail-closed malformed and multiple-statement cases.
- The command validates syntax before credentials/network and routes accepted statements through the authenticated AdminClient. Parser/binder, TCP binding, CLI process recognition, locked workspace build, formatting, warning-denied all-target Clippy, TinyCC SDK tests/fixture, and complete serial delegated-cgroup workspace suite passed with zero ignored tests; `git diff --check` passed.


## admin-sql: generated-ID DROP CONSTRAINT lifecycle — implemented

- The bounded DROP CONSTRAINT SQL route now recognizes table-scoped generated IDs for PRIMARY KEY, UNIQUE, CHECK, and FOREIGN KEY constraints and dispatches by authenticated metadata kind.
- The CLI process regression verifies generated-ID DROP syntax is recognized before credentials or network. Parser and AdminClient TCP coverage passed; locked build/fmt/Clippy, TinyCC SDK tests/fixture, and the complete serial delegated-cgroup workspace suite passed with zero ignored tests; `git diff --check` passed.

## admin-sql: CREATE INDEX — implemented

- Added bounded `CREATE [UNIQUE] INDEX ... ON schema.table [USING BTREE] (column, ...)` parsing and routing through authenticated metadata binding to typed index creation.
- Parser, authenticated TCP, and pre-credential CLI recognition regressions passed. Locked workspace build, format check, warning-denied all-target Clippy, maintained TinyCC SDK tests/fixture, complete serial delegated-cgroup workspace suite (sandbox/process tests enabled; zero ignored), and `git diff --check` passed; see the DDL component progress entry. DROP INDEX SQL remains unsupported pending name-bearing index metadata.


## admin-sql: DROP INDEX — implemented

- Authenticated administrative index metadata now exposes logical names. The SQL route resolves `DROP INDEX schema.name RESTRICT` across visible relations and pages, then dispatches the existing typed drop operation by stable generated ID.
- Parser, metadata decoding, authenticated TCP dispatch, and CLI recognition regressions passed. Focused parser, metadata decoding, authenticated TCP dispatch, CLI recognition, and process-level metadata lifecycle tests passed. Locked workspace build, format check, warning-denied all-target Clippy, maintained TinyCC SDK tests/fixture, full serial delegated-cgroup workspace suite (sandbox/process tests enabled; zero ignored), and `git diff --check` passed; see the DDL component progress entry.

## admin-sql: CREATE USER/ROLE — implemented

- Added bounded identity creation syntax and routes to typed authenticated user/role operations; token issuance remains separate.
- Parser, authenticated TCP, and CLI recognition regressions passed. Locked workspace build, format check, warning-denied all-target Clippy, maintained TinyCC SDK tests/fixture, full serial delegated-cgroup workspace suite (sandbox/process tests enabled; zero ignored), and `git diff --check` passed; see the identity DDL progress entry.
## ID-SQL-2: identity rename SQL

- `ALTER USER|ROLE old_name RENAME TO new_name` resolves the current name through the authenticated identity metadata operation, then calls the existing typed rename API. Parser and CLI syntax regressions pass, and the full delegated gateway/supervisor lifecycle test executes SQL rename successfully. Full delegated-cgroup workspace suite, build, formatting, Clippy, and TinyCC checks pass.

## ID-SQL-3: identity drop SQL

- `DROP USER|ROLE name RESTRICT` resolves one name through the same authenticated metadata operation and invokes the existing typed RESTRICT drop. Parser and delegated process regressions pass; build, format, Clippy, TinyCC, and full serial delegated-cgroup workspace checks pass.

## ID-SQL-4: user LOGIN state SQL

- `ALTER USER name LOGIN|NOLOGIN` resolves one name through the authenticated metadata operation and invokes the existing `users.login` mutation. Parser and delegated process regressions pass; build, format, Clippy, TinyCC, and full serial delegated-cgroup workspace checks pass.

## ID-SQL-5: role membership SQL

- `GRANT role TO USER|ROLE member` and matching `REVOKE` resolve both names independently and invoke existing membership mutations. Parser and delegated process regressions pass; build, format, Clippy, TinyCC, and full serial delegated-cgroup workspace checks pass.

## ID-SQL-6: system privilege SQL

- Bounded `GRANT|REVOKE SYSTEM PRIVILEGE` syntax resolves a grantee name and calls the existing typed database privilege operation. Parser and delegated process regressions pass; build, format, Clippy, TinyCC, and full serial delegated-cgroup workspace checks pass.

- Compatibility follow-up: `GRANT|REVOKE MANAGE_USERS|MANAGE_ROLES|CREATE_SCHEMA ON DATABASE TO|FROM USER|ROLE name` now dispatches through the same authenticated resolver and typed mutation. Parser tests cover both supported forms; the delegated metadata lifecycle uses the normative `ON DATABASE` spelling.

## DP-SQL-1: object-level grant SQL

- Added bounded schema/table object privilege GRANT/REVOKE parsing and routing through authenticated object and identity resolution to the existing typed grant operation. Column-targeted grants remain typed-only. Parser and delegated process tests pass; locked workspace build, format, warning-denied Clippy, TinyCC SDK tests/fixture, and full serial delegated-cgroup workspace suite pass with sandbox/process tests enabled and zero ignored.

## DP-SQL-2: single-column grant SQL

- The initial table grant syntax selected one named column for SELECT, INSERT, UPDATE, or REFERENCES, resolved it through authenticated metadata, and submitted the typed column grant. The follow-up below extends this route to atomic batches.

## DP-SQL-2: atomic multi-column grant SQL — verified

- GRANT/REVOKE accepts 1–64 distinct named columns for table SELECT, INSERT, UPDATE, and REFERENCES, resolving every column through authenticated metadata and sending one atomic typed operation.
- The delegated process lifecycle verifies an overlapping grant is rejected without a partial write, then checks successful multi-column grant discovery and complete batch revoke.
- Verification: focused protocol and cross-process metadata tests; locked workspace build, format check, warning-denied all-target Clippy, TinyCC SDK tests/fixture, release-profile validation, cgroup probe, and complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored. `git diff --check` passed; Cargo.lock is unchanged.

## DP-SQL-2: table-wide data grant expansion — verified

- Unqualified table `SELECT`, `INSERT`, `UPDATE`, and `REFERENCES` bind to a compact `all_columns` operation so storage expands the privilege at its catalog snapshot. A service-process regression checks grants on existing columns and confirms a later-added column receives none.
- Red/green: the regression first failed because the SQL path emitted a table object grant; it now passes through the separate gateway and supervisor processes.
- Verification: locked workspace build, format check, warning-denied all-target Clippy, TinyCC SDK tests/fixture, release-profile validation, cgroup capability probe, and complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored. `git diff --check` passed; Cargo.lock is unchanged.

## DS-SQL-17: schema and table OWNER SQL — implemented

- `admin-sql` recognizes bounded schema/table OWNER statements with explicit USER or ROLE targets, resolves names through authenticated metadata, and dispatches typed owner changes. Parser and delegated gateway/supervisor regressions pass; workspace build, format, warning-denied Clippy, TinyCC SDK tests/fixture, and the full serial delegated-cgroup workspace suite pass with sandbox/process tests enabled and zero ignored.

## DP-SQL-3: bounded CREATE ROW/CELL POLICY SQL — implemented

- `admin-sql` recognizes row and cell policy creation, binds identifiers/context functions, and prints the generated policy ID after dispatching the typed operation. Parser/binder and delegated process tests, workspace build, format, warning-denied Clippy, TinyCC SDK tests/fixture, and full serial delegated-cgroup suite pass with zero ignored. Policy-scoped identity resolution and DROP SQL are covered by DP-SQL-4/5 below.

## DP-SQL-4: bounded DROP ROW/CELL POLICY SQL — verified

- `DROP ROW|CELL POLICY name ON schema.table RESTRICT` resolves the named policy only in its table through authenticated metadata, then dispatches `policies.drop`; storage independently rechecks authority.
- Parser/protocol regressions and the delegated gateway/supervisor lifecycle cover both policy kinds. Full serial delegated-cgroup workspace validation passed with sandbox/process tests enabled and zero ignored tests; details are in [permission DDL progress](../ddl-permissions/PROGRESS.md).

## DP-SQL-5: policy-authorized table and column binding — verified

- Relation and column metadata for policy DDL is available to schema/table owners and scoped `MANAGE_POLICIES` holders with schema USAGE, even without data SELECT. Policy recipient and `HAS_ROLE` identity resolution remains scoped to the authenticated policy authority.
- The delegated lifecycle covers owner and non-owner manager paths. Full locked checks and the serial delegated-cgroup workspace suite passed; details are in [permission DDL progress](../ddl-permissions/PROGRESS.md).

## DP-SQL-6: database-scoped system privilege syntax — verified

- `GRANT|REVOKE MANAGE_USERS|MANAGE_ROLES|CREATE_SCHEMA ON DATABASE TO|FROM USER|ROLE name` resolves one grantee through authenticated metadata and calls the database-admin-only typed mutation.
- Parser and gateway/supervisor lifecycle regressions passed. Full locked checks and the serial delegated-cgroup workspace suite passed with zero ignored tests; details are in [permission DDL progress](../ddl-permissions/PROGRESS.md).

## DP-SQL-7: CREATE OR REPLACE ROW/CELL POLICY — verified

- `CREATE OR REPLACE` resolves a same-name policy across row and cell kinds and dispatches the dedicated atomic `policies.replace` protocol operation. The production process lifecycle replaces a row policy with a cell policy; the full serial delegated-cgroup workspace suite, warning-denied Clippy, build, format, TinyCC, and release-profile checks passed with zero ignored tests.

## DP-SQL-8: named index DDL SQL — verified

- `CREATE INDEX`, `REBUILD INDEX schema.name`, and `DROP INDEX schema.name RESTRICT` resolve relation, column, or logical-index metadata through authenticated control reads and dispatch the existing typed operations. Rebuild uses the current ordered columns and uniqueness profile; missing or ambiguous named indexes reject before mutation.
- Parser coverage checks bounded syntax. The delegated gateway/supervisor lifecycle now exercises named CREATE, REBUILD, and DROP via `admin-sql`. Focused tests and full serial delegated-cgroup validation passed with sandbox/process tests enabled and zero ignored tests; `Cargo.lock` is unchanged.

## DP-SQL-9: ALTER INDEX definition rebuild — verified

- `ALTER INDEX schema.name REBUILD [USING BTREE] {UNIQUE|NONUNIQUE} (columns...)` resolves the existing logical index and binds its replacement key columns on the owning table through authenticated metadata, then dispatches the established atomic `indexes.rebuild` operation.
- Parser rejection cases cover unqualified names, omitted uniqueness profile, unsupported methods, duplicate/empty columns, and multiple statements. The gateway/supervisor lifecycle exercises a named index definition change through `admin-sql` and confirms the new ordered column IDs and nonunique profile through authenticated metadata.
- Focused parser and delegated process tests passed, including a focused rerun after the metadata assertions were added. Locked offline workspace build, format check, warning-denied all-target Clippy, and the full serial delegated-cgroup suite passed with sandbox/process tests enabled and zero ignored tests. `Cargo.lock` is unchanged.
