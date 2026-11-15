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
