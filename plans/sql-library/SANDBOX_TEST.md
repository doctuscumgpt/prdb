# Sandboxed SQL v3 integration tests

The sandbox acceptance tests in `engine/tests/commit.rs` are enabled in the normal workspace suite and must run under the delegated service described below. The `sandboxed_v3_sql_error_latches_failure_and_durably_aborts_after_partial_output` test runs the maintained TinyCC artifact through authenticated staging, the configured cgroup/Landlock executor launch path, result relay, and storage terminal decision. The artifact writes provisional bytes, catches an unsupported SQL statement error, and returns success. The test requires the v3 host latch to turn that caught error into a worker failure, then checks that storage confirms ABORTED before the gateway terminal frame follows the provisional result frame.

The `sandboxed_v3_sql_dml_sequence_is_visible_and_commits` acceptance also starts a second confined cancellation invocation while the primary analytical SQL worker and four operational workers are active. Both sandbox observers start behind a barrier. Before requesting cancellation, the cancellation observer verifies that the SQL executor is still live and an authorized operational transaction is in flight; it then checks that the SDK cancellation poll is relayed within two seconds and storage records ABORTED. This covers cancellation during a real analytics invocation with concurrent operational reads and commits through the cgroup/Landlock path; it is not a configured latency target or a saturated multi-client analytics profile.

The `sandboxed_v3_sql_dml_sequence_is_visible_and_commits` test uses `engine/tests/fixtures/sql_v3_write_read.c` to run a sequence of SDK calls over one sandboxed executor connection. The fixture exports v3 through v5 entry points; ABI v5 adds optimizer-backed UPDATE while retaining the v4 optimizer SELECT path. It creates the nullable `records` table, a two-column `grouped_records` table, and a distinct `linked_records` table. It seeds `(left_key,right_key)` rows `(1,7)`, `(1,7)`, and `(2,9)`, grants the query principal SELECT on the required tables and columns, and includes them in the invocation's requested scope. The artifact inserts NULL into `records`, verifies the row in a following SELECT/UPDATE scan, updates it to 42, verifies that value in a following SELECT, then groups by `id` with `MIN(id)`, `SUM(id)`, and `AVG(id)`. It routes the AVG query through optimizer request statement 10 and plan kind 10, validates the v7 AST and authorized binding, re-resolves executor identity, and invokes the bounded v7 grouping runner before checking the established DECIMAL(38,6) SQLR output. It also routes a v12 grouped AVG query through request statement 11 and plan kind 11, matching two sort expressions (AVG descending NULLS LAST, then group key ascending NULLS FIRST), LIMIT, and the authorized binding before re-resolving executor identity and invoking the bounded v12 sorter; the acceptance checks both ordered DECIMAL(38,6) SQLR frames byte-for-byte. It routes v12 grouped `COUNT(*) ORDER BY COUNT(*) DESC NULLS LAST, id ASC NULLS LAST` through the optimizer's kind-5 grouping-plan validation, then the SDK's bounded grouped-sort runner, and writes its result as SQLR. The optimizer request validates the group key, relation identity, executor shape, and LIMIT; ORDER BY stays in the SDK and this composition accepts no WHERE or HAVING. It runs v13 grouped `COUNT(*)` on the two authorized key columns, then runs v15 with parameterized composite HAVING, v17 with literal composite HAVING, and v18 with `(COUNT(*) >= 2 OR COUNT(*) = 3) AND NOT COUNT(*) < 2`; all composite queries use `ORDER BY right_key DESC NULLS LAST, COUNT(*) DESC, left_key ASC NULLS LAST`. The test checks all seven composite-group result frames byte-for-byte after they cross the result relay. It next runs a v8 `INNER JOIN` across the linked relations through independent planner descriptions and sequential authorized executor scans, followed by the two-key v19 aliased join across `grouped_records AS g` and `linked_records AS l` through ABI v5 optimizer submission, plan-to-AST validation, executor-side stable-identity re-resolution, and bounded join execution. It then runs a v19 self-join of `linked_records AS a` and `linked_records AS b`, followed by a parameterized v20 qualified-WHERE join that filters before LIMIT and writes the selected columns as SQLR. The test checks that final v20 frame byte-for-byte as `(1, 42)`. The artifact then submits `SELECT right_key FROM grouped_records ORDER BY left_key DESC LIMIT 2` through the optimizer-backed kind-7 path and the confined executor; the gateway relay must receive exact one-column SQLR frames containing `9` then `7`, proving that the order key may be hidden from projection and LIMIT follows sorting. The v4 entry point parses and submits `SELECT id FROM records WHERE id = $1 LIMIT 1` through the optimizer process, independently validates the filtered plan and authorized relation shape, and checks the returned integer row is 42. After the multi-row INSERT, the v5 entry submits `UPDATE records SET id = $2 WHERE id = $1` through the isolated optimizer, validates the assignments against the SQL AST and executor metadata, stages the patches through executor IPC, then confirms both rows read back as 43. It next submits `DELETE FROM records WHERE id = $1` through the optimizer and confirms a following SELECT returns no rows. The grouped callbacks are also checked for exact MIN, widened BIGINT SUM, DECIMAL(38,6) AVG, ordered COUNT, composite-key COUNT, and v18 HAVING-tree metadata. The artifact then deletes the `records` row and confirms a final SELECT returns no row. After receiving all provisional SQL results, the test closes the gateway before terminal delivery, requires the terminal frame write to fail, and checks that storage retained COMMITTED. Its restricted-principal query follows the normal terminal path. This exercises SQL parsing/binding, optimizer and executor IPC, private-write visibility across SELECT/UPDATE/DELETE, filtered plan execution, grouped aggregation and composite multi-key ordering, optimizer-backed two-relation authorized JOIN execution with aliases and composite keys, self-join execution, ordered SELECT and grouped aggregate plan matching and execution, SQLR encoding in the confined artifact, result relay, and storage commit through the real invocation path, including lost-terminal recovery.

The current write/read fixture also calls the bounded DML v2 runner to insert two rows, then selects both within the same invocation. This covers multi-row staging and private-write visibility in the confined artifact. Durable multi-row commit and a later-row failure followed by durable abort are checked by the enabled acceptance on a host with delegated cgroups; compiling the artifact alone does not establish those results.

The `authenticated_metadata_and_admin_lifecycle_cross_gateway_and_supervisor_processes` process test also runs in the normal suite. Set `PRDB_TEST_RESTRICTED_METADATA_QUERY_ARTIFACT` to the built `sql-v3-restricted-metadata.so` to exercise its restricted metadata branch; it packages the artifact for the test principal, uploads and starts it through `SubmissionClient`, and verifies filtered SQL results and committed STATUS through the external gateway and supervisor processes.

First build the artifact and SDK archive:

```sh
nix-shell -p rustc cargo clippy rustfmt tinycc --run 'make -f engine/sdk/Makefile sandbox-query-fixture'
```

This builds `sql-v3-caught-error.so`, `sql-v3-network-constructor.so`, `sql-v3-write-read.so`, and `sql-v3-restricted-metadata.so`. These are untrusted query artifacts and must only be loaded through the sandboxed executor tests.

Run the full workspace test suite inside a delegated `systemd --user` unit. A direct `cargo test` from a desktop application scope is not a useful preflight: that scope may omit the `cpu` controller even when the host mount is writable. The repository launcher moves its process into a child cgroup, enables the delegated controllers on the unit cgroup, and exports the absolute parent path for the test.

Configure these environment variables in the shell command run by the unit:

- `PRDB_TEST_CGROUP_PARENT`: an existing cgroup v2 parent delegated with enabled `cpu`, `memory`, and `pids` controllers.
- `PRDB_TEST_QUERY_ARTIFACT`: absolute path to `target/sdk-v1/sql-v3-caught-error.so`.
- `PRDB_TEST_WRITE_READ_QUERY_ARTIFACT`: absolute path to `target/sdk-v1/sql-v3-write-read.so`.
- `PRDB_TEST_RESTRICTED_METADATA_QUERY_ARTIFACT`: absolute path to `target/sdk-v1/sql-v3-restricted-metadata.so`, used by the restricted-principal metadata acceptance.
- `PRDB_TEST_NETWORK_CONSTRUCTOR_ARTIFACT`: absolute path to `target/sdk-v1/sql-v3-network-constructor.so`.
- `PRDB_TEST_RUNTIME_READ_EXECUTE`: platform-separated absolute paths for the pinned dynamic loader, libc, and other executor runtime dependencies.
- `PRDB_TEST_RUNTIME_READ_ONLY`: platform-separated absolute paths for runtime configuration files the dynamic loader needs, such as its loader cache when applicable. Set it to an empty string when the selected runtime has no read-only files; do not use absent paths as placeholders.

Run the complete workspace test suite, including all sandbox and process tests, with the configured artifact/runtime paths:

```sh
systemd-run --user --wait --pipe --collect \
  --unit="prdb-sandbox-test-$$.service" --slice=prdb-tests.slice \
  --property='Delegate=cpu memory pids' --property="WorkingDirectory=$PWD" \
  "$PWD/engine/tools/with_delegated_cgroup.sh" sh -c '
    export PRDB_TEST_QUERY_ARTIFACT="$PWD/target/sdk-v1/sql-v3-caught-error.so"
    export PRDB_TEST_NETWORK_CONSTRUCTOR_ARTIFACT="$PWD/target/sdk-v1/sql-v3-network-constructor.so"
    export PRDB_TEST_WRITE_READ_QUERY_ARTIFACT="$PWD/target/sdk-v1/sql-v3-write-read.so"
    export PRDB_TEST_RESTRICTED_METADATA_QUERY_ARTIFACT="$PWD/target/sdk-v1/sql-v3-restricted-metadata.so"
    export PRDB_TEST_RUNTIME_READ_EXECUTE="/absolute/path/ld-linux-x86-64.so.2:/absolute/path/libc.so.6:/absolute/path/libgcc_s.so.1"
    export PRDB_TEST_RUNTIME_READ_ONLY=
    exec nix develop --no-update-lock-file -c cargo test --workspace --locked -- --test-threads=1
  '
```

The launcher supplies `PRDB_TEST_CGROUP_PARENT` and `PRDB_SYSTEMD_CGROUP_PARENT`; the command runs all tests serially so each cgroup-dependent test can provision its child beneath that parent. Do not run query fixtures with `tcc -run`, from a trusted-process loader, or directly outside the confined executor.

For focused process-test debugging, run the test directly inside the same delegated service with its artifact/runtime environment:

```sh
cargo test -p prdb-engine --locked --test metadata_process disconnected_submission_is_reconciled_after_gateway_restart -- --exact --nocapture
```

Do not execute or load the artifact outside this executor path. The complete workspace command reports these tests as passed rather than ignored; the test must run on a host with the configured cgroup delegation and runtime profile.
