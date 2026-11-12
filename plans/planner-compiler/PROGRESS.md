# Optimizer Progress

## OP-7: bounded composite join request and authorized binding — in progress

- Contract: [optimizer](../../specs/planner-compiler.md). Decode a two-relation SELECT with stable left/right projections, two to eight equality keys, and an optional limit; bind both relations through the invocation's authorized planner metadata before planning.
- Red/green evidence: the request test initially failed to compile because the statement decoder and typed request records were absent. The maintained tests now cover a composite request, invalid projection side, key-count bounds, trailing bytes, left/right column-ID binding, incompatible key types, cross-schema relation shapes, and unavailable relations.
- Implementation: `decode_join_select_request` accepts statement 4 under the existing request version and size/identifier bounds. `bind_join_select_request_via_metadata` independently resolves each relation and maps projections/keys to stable metadata. It requires a common schema identity/version; equality keys require matching scalar types, with Decimal precision allowed to vary when scale matches. This binding conveys no executor capability.
- Verification: both focused request decoder and binding tests passed under the locked workspace toolchain. ABI constants are generated from `engine/abi/query_abi_constants.json` and include the new statement tag.
- Plan/process implementation: `JoinSelectPlanV1` is a distinct kind-4 encoding, preserving kinds 1–3. Both scans carry unique ordinal-sorted shapes; projections retain left/right side and source order; equality keys retain bound column metadata; and LIMIT is optional. Rust decoding validates common schema identity, relation identity, scan ordering/uniqueness, projection/key membership, compatible key types, bounds, and trailing bytes. `OptimizeJoinSelect` (`0x1207`) resolves both authorized relations through the planner process and returns only a validated plan. Optimizer response framing accepts all four plan kinds.
- Verification: plan codec round-trip/malformed-reference regression and the two-relation optimizer process regression pass. The process test verifies both metadata requests, stable relation names, scan shapes, two keys, and LIMIT. Generated ABI consistency, workspace build, format, warning-denied Clippy, maintained TinyCC tests/fixture builds, and the full serial workspace suite pass. The suite ran in a delegated service; commit integration reported 142 passed, zero ignored, and all cgroup-backed tests executed. `git diff --check` passed.
- SDK boundary: `prdb_sdk_optimizer_decode_join_plan_v1` independently decodes kind 4 and validates both scan shapes, ordered projections, key references/type compatibility, LIMIT, bounds, and canonical framing. `prdb_sdk_optimizer_validate_executor_join_relations_v1` re-resolves both plan references through normal executor storage IPC and matches schema/table identities, epochs, relation names, and scanned column definitions before any execution code receives the plan.
- Request adapter and execution gate: the SDK encodes the parsed alias-aware v19 INNER JOIN AST into request statement 4 and submits it through ABI v5's operation-selected `OptimizeJoinSelect` callback. It preserves projection sides/order, composite keys, and LIMIT; unsupported or malformed spans fail before callback dispatch. `prdb_sdk_optimizer_match_join_plan_v19_v1` checks returned relation names/schema, projection order/sides/types, composite key order/types, and LIMIT against the parsed and bound AST. `prdb_sdk_sql_execute_inner_join_optimizer_v1` submits the plan, binds against authorized planner shapes, matches the AST, re-resolves both relations through executor storage, and only then runs the matching bounded SDK join pipeline.
- Verification: TinyCC compiles and runs the maintained `sdk-optimizer-plan-v1` fixture; it checks a hand-built kind-4 plan, both-side metadata resolution, AST matching and a mismatched LIMIT, invalid projection side, mismatched key types, and trailing bytes. The optimizer fixture checks canonical statement-4 bytes, bounded output behavior, operation selection, and response decoding. `sdk-sql-execute-v19` exercises optimizer submission, plan matching, both executor identity lookups, and result equivalence on a composite join. The confined `sandboxed_v3_sql_dml_sequence_is_visible_and_commits` acceptance now executes the two-key aliased v19 join through ABI v5 optimizer submission, validates the plan against SQL, re-resolves both relations, and commits the relayed result.

## OP-7: confined optimizer join acceptance — verified

- Red evidence: the first delegated acceptance failed with `Planner(DuplicateRequestId)` after earlier direct planner calls. Optimizer metadata lookups had reused optimizer-channel IDs on the shared planner stream; a two-relation lookup also needed distinct IDs for each side.
- Fix: optimizer planner requests now use a disjoint range above the direct planner request cap and allocate a unique ID per relation. The optimizer process regression asserts those IDs for SELECT, DELETE, UPDATE, and both join relations.
- Verification: optimizer process tests passed (7/7); the maintained TinyCC SDK suite and sandbox query fixture build passed. The focused confined DML sequence passed with the optimizer-backed join. Locked workspace build, format check, warning-denied Clippy, and the full serial workspace test suite passed in the delegated `systemd --user` unit; commit integration reported 142 passed, zero ignored, including all cgroup tests, and metadata process integration reported 2 passed. `git diff --check` passed.
- Remaining: broaden grouped optimizer planning beyond COUNT and gather analytical-load and mixed-workload measurements. The optimizer-gated single-key COUNT plus v12 ORDER BY composition is verified below; its sort remains in SDK execution.

## OP-7 dependency: bounded single-key COUNT(*) plan — Rust service implemented

- Contract: the first grouped optimizer plan accepts `SELECT key, COUNT(*) FROM relation GROUP BY key [LIMIT n]`; it carries only authorized stable relation identity and the one scanned key column. It has no WHERE, HAVING, or ORDER BY.
- Implementation: statement 5 (`OptimizeGroupCount`, `0x1208`) is bounded and decoded, binds the key against invocation-authorized metadata, and lowers to validated plan kind 5. The scan shape must equal the group key exactly; the optional LIMIT applies after grouping.
- Verification: request bounds/trailing bytes, authorized key binding, kind-5 round-trip/scan validation, and optimizer process dispatch are covered by four focused Rust regressions; they passed. ABI generation was refreshed and checked.
- TinyCC boundary: `prdb_sdk_optimizer_decode_group_count_plan_v1` independently validates kind 5, the exact one-column scan/group key, stable relation references, optional LIMIT, and canonical framing. The SDK encodes the v4 COUNT(*) AST as statement 5 and submits it through ABI v5's `OptimizeGroupCount` operation.
- SQL validation and execution gate: the SDK matches kind 5 against the parsed v4 COUNT(*) statement, its authorized grouping-key binding, schema/relation names, and optional LIMIT. It re-resolves the relation through executor storage and checks the stable relation identity and exact scanned column definition before running the bounded COUNT grouping scan.
- Confined acceptance: the maintained ABI v5 SQL artifact executes `SELECT id, COUNT(*) FROM records GROUP BY id` through optimizer operation `0x1208`, validates current executor identity, relays the exact SQLR result, and verifies it through durable commit in the existing cgroup-backed SQL sequence.
- Verification: TinyCC regressions cover valid kind-5 decode, scan/key mismatch, AST/LIMIT matching, canonical request bytes, operation selection, response decoding, and filtered-query rejection. `make -f engine/sdk/Makefile test sandbox-query-fixture` and the focused confined acceptance passed. Locked workspace build, format check, warning-denied Clippy, and the full serial workspace suite also passed in the delegated user service with cgroup tests enabled and zero ignored tests. Composite grouping, aggregates beyond COUNT(*), WHERE/HAVING, and ordering remain unsupported by this optimizer request subset.

## OP-7 dependency: bounded composite COUNT(*) request and plan — verified

- Contract: statement 6 extends the existing grouped COUNT optimizer operation to two through eight keys in source order, plus optional post-group LIMIT. It carries no filters, HAVING, or ordering.
- Implementation: the bounded Rust decoder, invocation-authorized binder, kind-6 plan codec, canonical ordinal-sorted scan shape, optimizer service, process dispatch, and response validation are implemented. TinyCC independently encodes the v13 AST, decodes and matches kind 6, and checks executor identity before the existing bounded composite grouping runner executes.
- Confined acceptance: the ABI v5 SQL artifact submits `SELECT left_key, right_key, COUNT(*) FROM grouped_records GROUP BY left_key, right_key LIMIT 2` through statement 6 / operation `0x1208`. The existing exact SQLR assertions pass through result relay and durable commit.
- Verification: focused Rust request bounds/trailing-byte, binding duplicate/order, plan codec/canonical scan, and optimizer process-channel tests passed (4 tests). TinyCC tests cover exact statement-6 request bytes, kind-6 scan/key validation, AST matching and LIMIT mismatch. `make -f engine/sdk/Makefile test sandbox-query-fixture` and the focused confined acceptance passed. Grouped ordering plans and analytical workload measurements remain open.

## OP-7 dependency: bounded ordered SELECT plan — implemented

- Contract: statement 7 sends explicit projections, one source-column ORDER BY key, direction, and optional LIMIT through the existing `OptimizeSelect` operation. The order key may be hidden from the output projection.
- Implementation: Rust decodes and binds the request through invocation-authorized planner metadata and emits kind 7 with a canonical union scan shape, ordered projections, order-key metadata, direction, and LIMIT. TinyCC independently validates and matches the plan to the v3 SQL AST, re-resolves the relation through executor storage, then stable-sorts at most 128 authorized rows within 1 MiB before applying LIMIT.
- Regression: Rust request, binding, plan codec, and optimizer process tests cover statement 7. TinyCC covers exact request bytes, hidden-key plan decoding/matching, zero LIMIT without a scan, materialization exhaustion without output, and cancellation after 32 published rows before the next callback. The confined SQL artifact verifies descending hidden-key output through durable result relay.
- Verification: the full maintained SDK suite passed after the executor-bound regressions. The focused Rust optimizer regressions and delegated confined acceptance passed in the preceding ordered SELECT increment.
- Remaining: grouped optimizer ordering and analytical/mixed-workload measurements remain open.

## OP-7 follow-up: optimizer-gated grouped COUNT ORDER BY — verified

- Contract: a one-key `COUNT(*)` query with v12's one- or two-output ORDER BY remains bounded and must validate the optimizer's group scan identity before emitting sorted SQLR results. The request does not carry WHERE or HAVING.
- Implementation: `prdb_sdk_sql_execute_group_count_ordered_optimizer_v1` parses and binds the full v12 query, derives its frozen v4 COUNT/group-key view for statement-5 request encoding, matches the kind-5 plan and LIMIT, re-resolves executor identity, then delegates to the bounded v12 grouped runner for HAVING-free aggregation, NULL ordering, stable ties, and LIMIT-after-sort behavior.
- Confined acceptance: the maintained ABI v5 query artifact now routes `SELECT id, COUNT(*) ... ORDER BY COUNT(*) DESC NULLS LAST, id ASC NULLS LAST LIMIT 1` through this optimizer gate. The end-to-end test checks the exact relayed SQLR result and durable commit.
- Verification: `nix develop --no-update-lock-file -c make -f engine/sdk/Makefile sandbox-query-fixture` and the full maintained SDK suite passed. The focused delegated `sandboxed_v3_sql_dml_sequence_is_visible_and_commits` acceptance passed with the cgroup probe confirming writable `cpu memory pids` delegation; the run verified the exact ordered SQLR frame through durable commit. The full Rust workspace suite was not rerun for this C-only increment.
- Remaining: optimizer plan kinds that encode grouped sort items, composite-group optimizer ordering, and analytical/mixed-workload measurements remain open.

## OP-2 dependency: generated unknown estimate response — implemented

- Contract: when an authorized estimate is unavailable or exceeds its allowance, planner IPC returns the exact versioned unknown response and keeps the invocation usable.
- Implementation: the shared ABI schema now generates the unknown response encoder/decoder, and [`planner_ipc`](../../engine/src/planner_ipc.rs) uses them instead of duplicating version/status bytes. Exact estimate tails remain bounded and schema-driven.
- Regression: `planner_stream_estimate_allowance_falls_back_without_aborting_invocation` verifies the unknown estimate is decoded and the invocation continues.
- Verification: ABI drift/schema tests, focused fallback regression, locked workspace build, format check, warning-denied Clippy, serial workspace tests (129 passed, 3 cgroup-dependent ignored), and maintained TinyCC SDK suite passed in the configured Nix shell.
- Remaining: optimizer binding/planning integration and execution remain open as described below.

## OP-3 ABI v4 SQL callback fixture — implemented; confined acceptance verified

- Behavior: the maintained SQL write/read fixture now exports both v3 and v4. The v4 entry submits a parameterized `id = 42` SELECT through the executor's fixed optimizer callback, validates the optimizer Filter against the AST and parameter, independently resolves the relation through executor storage, and checks the projected row.
- Verification: TinyCC SDK tests and sandbox-fixture compilation pass. The actual fixture packages successfully as ABI v4 without loading; the package test checks exact descriptor/entry symbols. Workspace build, format, warning-denied Clippy, and tests pass (129 commit tests; 3 cgroup tests ignored).
- Remaining: bounded mutation plans remain open.

## OP-1 dependency: bounded structured SELECT request decoder

- Contract: [optimizer specification](../../specs/planner-compiler.md). The first request payload is a versioned, single-relation SELECT with explicit projections, optional bounded predicate tree, and optional LIMIT.
- Implementation: [`optimizer_request`](../../engine/src/optimizer_request.rs) decodes identifiers, record lengths, predicate tags and values into owned Rust data. It caps requests at 64 KiB, predicates at 16 KiB, projections at 128, text literals at 4,096 bytes, predicate nodes at 256, depth at 32, and boolean fan-in at 16. Unknown versions/statements/nodes, invalid booleans/operators/identifiers, truncation, and trailing bytes fail closed.
- Boundary: the decoder performs structural validation only. It has no storage access and does not bind names or types, authorize relations, choose plans, or grant execution authority. The C parser and worker channels do not call it yet.
- Regression: four unit tests cover a compound typed request, malformed outer records/identifiers, excessive collection bounds and unsupported nodes, and predicate byte/node/depth exhaustion.
- Verification: locked workspace build and format passed; warning-denied Clippy passed; serial locked workspace tests passed (129 commit integration tests and all other active suites passed; 3 cgroup-dependent commit tests and 1 disconnected-submission process test were ignored). `git diff --check` passed.
- Remaining: define and validate type semantics against authorized metadata, return only non-disclosing bind failures, produce and validate a logical/physical plan, integrate a bounded isolated optimizer process and its supervisor-bound submission channel, and execute plans through SDK operations.

## OP-2 dependency: bind SELECT against authorized relation shape

- Implementation: [`optimizer_binding`](../../engine/src/optimizer_binding.rs) decodes the v1 request, resolves its exact qualified name through the invocation-bound `PlannerMetadataConnection`, and binds schema ID/version, table ID/write epoch, and projection/predicate column IDs, ordinals, types, and nullability. It validates relation shape and expression type compatibility. Missing descriptions and relation-name mismatches both return `RelationUnavailable`.
- Boundary: the helper receives no principal, grant, raw catalog, or storage handle and creates no executor permission. Its only metadata source is the planner connection opened on the admitted transaction.
- Regression: three unit tests cover stable reference binding, non-disclosing relation absence/name mismatch, and unknown-column/type-incompatible rejection. A real-redb integration regression binds an authorized user table, checks schema ID/version, table ID/epoch, column ID/shape, and confirms a missing relation maps to `RelationUnavailable`.
- Verification: focused binder tests, the focused planner stream/metadata integration regression, locked workspace build, formatting, warning-denied Clippy, serial workspace tests, and `git diff --check` passed (132 commit integration tests passed; 3 cgroup-dependent commit tests and 1 process test ignored).
- Remaining: define additional literal coercions/SQL NULL semantics. Isolated optimizer process, versioned plan output, and supervisor-bound submission channel remain open.

## OP-3 dependency: lower a bound SELECT to a validated plan pipeline

- Implementation: [`optimizer_plan`](../../engine/src/optimizer_plan.rs) creates and canonically encodes a linear scan/filter/project/limit plan with schema ID/version, relation ID/write epoch, and stable column references. The scan reads the unique ordinal-sorted union of projection and predicate columns; output projection order and duplicates are preserved. The plan contains no planner handle, grant, or commit operation.
- Validation: `validate_plan_v1` and the byte decoder check the pipeline shape, relation identity, unique sorted scan columns, projection membership, filter references/types, and predicate depth/node/fan-in/text bounds. Column references in predicates resolve only through the decoded scan shape. The SDK and executor must repeat plan validation after channel transfer.
- Regression: eight unit tests verify pipeline construction/order, scan-column union, malformed order/duplicate limits, invalid filters/references/depth/types, canonical byte round-trip, and malformed version/type/truncated/trailing payload rejection.
- Verification: focused optimizer plan tests passed. Locked workspace build, format, warning-denied Clippy, serial workspace tests, and `git diff --check` passed (129 commit integration tests passed; 3 cgroup-dependent commit tests and 1 process test ignored).
- Remaining: assign/transfer the plan over the supervisor-bound channel, independently re-resolve the relation through executor storage, interpret the plan through the SDK, and implement mutation plans and optimizer process isolation.

## OP-3 dependency: compose invocation-bound optimizer service

- Behavior: one library entry point decodes, resolves and binds a structured request through the invocation's planner metadata connection, lowers it, and returns canonical plan bytes without transferring planner handles or authority.
- Implementation: [`optimizer_service`](../../engine/src/optimizer_service.rs) composes the existing binder and plan validator/encoder. The real-redb planner regression now invokes it, decodes the returned plan, and checks schema/table IDs and pinned epochs; unavailable relations map to a non-disclosing bind result.
- Verification: locked workspace build, format, warning-denied Clippy, serial workspace tests, and `git diff --check` passed (129 commit integration tests passed; 3 cgroup-dependent tests ignored).
- Remaining: bounded submission IPC, isolated optimizer process and supervisor channel, independent executor-side relation resolution, SDK interpretation, and mutation plans.

## OP-3 dependency: define optimizer submission payload and role

- Behavior: reserve one role-specific operation for SELECT submissions and accept a successful response only when it contains a complete independently validated plan; errors use a small non-disclosing status set.
- Implementation: operation `0x1203`, generated Rust/C constants, `OptimizerSubmission` role allowlisting, and canonical response payload encoder/decoder are defined in `optimizer_ipc`. Request structure remains the canonical SELECT v1 payload and its 64 KiB decoder. This only defines and validates payloads; no socket handler or executor callback is connected yet.
- Verification: generated ABI files matched the schema. Locked workspace build, format, warning-denied Clippy, serial workspace tests, and `git diff --check` passed (129 commit integration tests passed; 3 cgroup-dependent tests ignored; 81 Rust unit tests passed).
- Remaining: connect the submission socket to `optimize_select_v1`, launch the optimizer in its own process, independently re-resolve plan references through the executor channel, and interpret plans in the SDK.

## OP-3 dependency: serve optimizer submissions over a bounded socket

- Behavior: a provisioned optimizer-submission stream validates request IDs and role, returns canonical plan payloads, and gives the same unavailable status for hidden/missing relations. Malformed framing, duplicate IDs, limit exhaustion, transport failures, and storage transaction errors abort the invocation.
- Implementation: [`prdb-optimizer`](../../engine/src/bin/prdb-optimizer.rs) serves submissions in a separate process using only inherited submission and planner sockets. [`optimizer_process`](../../engine/src/optimizer_process.rs) validates each request, resolves stable authorized relation metadata through the planner socket, builds a plan, and validates the response before writing it. The previous in-process stream handler was removed. A real-redb subprocess integration regression checks successful and unavailable lookups.
- Boundary: the isolated process now has production supervisor launch, OS confinement, aggregate invocation resource limits, and a distinct planner metadata channel. The executor callback/channel is provisioned but not yet exposed through the query ABI.
- Verification: focused real-redb optimizer subprocess and protocol-loop tests passed. Generated ABI consistency, locked workspace build, format, warning-denied Clippy, serial workspace tests, and `git diff --check` passed (132 commit integration tests passed; 3 cgroup-dependent tests and 1 disconnected-submission process test ignored; 82 Rust unit tests passed).
- Remaining: expose optimizer submission through the executor query ABI, independently re-resolve plans in storage, and execute plans in the SDK.

## OP-3 dependency: C SDK optimizer response envelope — implemented

- Behavior: the C SDK can submit an already encoded bounded SELECT request through the ABI v4 optimizer callback and decode the canonical response version/status/length envelope. Successful responses expose borrowed plan bytes; errors expose no plan. Malformed versions, statuses, lengths, trailing bytes, oversized plans, and overlapping output/payload records are rejected.
- Implementation: [`prdb_sdk_optimizer_v1.h`](../../engine/include/prdb_sdk_optimizer_v1.h) and [`optimizer_v1.c`](../../engine/sdk/optimizer_v1.c) provide the status decoder and callback wrapper. The result record borrows plan bytes from the caller's response buffer.
- Regression: [`sdk_optimizer_v1.c`](../../engine/tests/fixtures/sdk_optimizer_v1.c) checks successful plans, unavailable status, callback I/O errors, response capacity behavior, and malformed payload rejection under TinyCC.
- Limitation: this step does not serialize a parsed SQL AST, validate plan structure, resolve relation IDs through executor storage, or execute plan operators.
- Verification: the full maintained TinyCC SDK suite passed, including this fixture. Workspace build, formatting, warning-denied Clippy, serial Rust tests, generated ABI check, and `git diff --check` also passed (129 commit integration tests; three cgroup-dependent tests ignored).
- Remaining: serialize the supported SQL AST into canonical optimizer requests, independently decode/validate plans, re-resolve the relation, and route physical operators through SDK executor calls.

## OP-3 dependency: encode simple SQL ASTs as optimizer requests — implemented

- Behavior: the C SDK serializes an explicit-column, single-relation SQL v2 SELECT AST into canonical optimizer request v1, with a caller-supplied schema and optional LIMIT.
- Boundary: `SELECT *` and every WHERE tree return `UNSUPPORTED`; the encoder does not silently discard either projection or predicate semantics. It checks ABI/version/AST shape, identifier and span bounds, request size, and output overlap; undersized buffers receive only the required length.
- Regression: the TinyCC optimizer fixture parses `SELECT name FROM items LIMIT 7;`, compares the encoded bytes to the canonical request, checks undersized-buffer behavior, and verifies star and parameterized predicates are rejected.
- Verification: the targeted fixture and full maintained TinyCC SDK suite passed. Rust 1.95 locked workspace build, format check, warning-denied Clippy, and serial workspace tests passed (82 library tests and 129 commit tests passed; 3 cgroup-dependent commit tests and 1 cgroup-dependent process test were ignored). Generated ABI consistency and `git diff --check` passed.
- Remaining: independently decode and validate plans in the executor, re-resolve plan relation identities through executor storage, and execute supported plan operators.

## OP-3 dependency: independently decode basic plans in the C SDK — implemented

- Behavior: the C SDK independently decodes canonical optimizer plan v1 for `Scan -> Project -> optional Limit`.
- Boundary: it validates framing, nonzero stable relation/column IDs and epochs, ASCII identifiers, scalar type encodings, scan-column ordering/uniqueness, projection membership, node order, and trailing bytes. Relation names borrow the input payload; column records are copied into the result. Filter nodes return `UNSUPPORTED` until their bounded prefix trees receive independent structural and type validation.
- Regression: the TinyCC fixture checks a hand-constructed plan, borrowed names and copied column metadata, projection-reference rejection, filter rejection, trailing-byte rejection, and undersized output records.
- Verification: targeted fixture and full maintained TinyCC SDK suite passed. Rust 1.95 locked workspace build, format check, warning-denied Clippy, and serial workspace tests passed (82 library tests and 129 commit tests passed; 3 cgroup-dependent commit tests and 1 cgroup-dependent process test were ignored). Generated ABI consistency and `git diff --check` passed.
- Remaining: decode/type-check filter predicates, independently resolve plan IDs through executor storage, and interpret validated nodes through SDK calls.

## OP-3 dependency: validate bounded filter plans in the C SDK — implemented

- Behavior: `prdb_sdk_optimizer_decode_plan_with_filter_v1` accepts the full canonical plan v1 pipeline and returns a borrowed raw prefix predicate view alongside the decoded scan/project records.
- Boundary: the decoder validates predicate tags and operators, UTF-8/text limits, column IDs against the scan shape, comparison operand compatibility, Boolean fan-in and NOT operands, depth 32/node 256 bounds, and a Boolean root. The existing basic decoder keeps its stable output record and returns `UNSUPPORTED` for filtered plans; callers use the new explicit filter-view entry point.
- Regression: TinyCC fixtures cover a valid nested AND/NOT comparison, invalid root type, incompatible comparison operands, invalid UTF-8, filter absence, and output capacity errors.
- Verification: targeted fixture and full maintained TinyCC SDK suite passed. Rust 1.95 locked workspace build, format check, warning-denied Clippy, and serial workspace tests passed (82 library tests and 129 commit tests passed; 3 cgroup-dependent commit tests and 1 cgroup-dependent process test were ignored). Generated ABI consistency and `git diff --check` passed.
- Remaining: independently resolve plan IDs through executor storage, interpret validated nodes through SDK calls, and connect the plan decoder to optimizer submission results.

## OP-3 dependency: expose stable authorized plan relation metadata

- Behavior: the optimizer can resolve exact qualified names to a pinned schema/table identity and declared column shape without receiving a storage handle or executor capability.
- Implementation: planner operation `describe_plan_relation` (`0x1204`) calls the invocation-bound metadata resolver directly and returns schema ID/version, table ID/write epoch and authorized columns. It does not allocate a planner handle. The existing planner channel regression checks returned identities against the real-redb catalog; role tests reject the operation on the optimizer submission role.
- Verification: focused planner channel regression passed. Generated ABI consistency, locked workspace build, format, warning-denied Clippy, serial workspace tests, and `git diff --check` passed (129 commit integration tests passed; 3 cgroup-dependent tests ignored; 81 Rust unit tests passed).
- Remaining: production process launch now gives the optimizer separate submission and planner metadata sockets; executor callback integration, executor-side re-resolution, and SDK interpretation remain.

## OP-3 dependency: re-resolve stable plan identity through executor storage

- Behavior: the executor independently checks a decoded plan's qualified relation against the same invocation-pinned authorized catalog and returns stable schema/table/column identities without creating a storage handle.
- Implementation: executor operation `resolve_plan_relation` (`0x1105`) reuses the versioned qualified-name request and the planner `describe_plan_relation` response encoding. Shared identity assembly validates catalog schema metadata for both roles. Executor-role allowlisting permits the operation while planner-role use is rejected; a real-storage IPC regression verifies the pinned identity and that hidden/missing relations are indistinguishable. C helper `prdb_sdk_optimizer_validate_executor_relation_v1` re-decodes the full plan, requests this metadata, and checks all stable identities plus each scanned column's ID, ordinal, type parameters, and nullability.
- Regression: TinyCC fixture checks successful identity matching, stale epoch rejection, scanned column ID mismatch, unavailable relation, and undersized response handling.
- Verification: Rust workspace and TinyCC suite passed for the preceding IPC increment. The current C helper's focused fixture passed; full workspace/TinyCC validation and diff checks are pending for this increment.
- Remaining: resolve the executor scan capability only after identity match, then interpret the validated operators.

## OP-3 dependency: acquire executor relation capability after plan identity validation

- Behavior: a plan receives an ordinary executor relation handle only after its stable identity and scanned shape match metadata resolved from the invocation's authorized snapshot.
- Implementation: `prdb_sdk_optimizer_acquire_plan_relation_v1` invokes the independent metadata validator first, then calls executor `resolve_relation` (`0x1101`) and checks that the returned relation ID/name still match the plan before reporting success. It returns the normal connection-local executor handle and full declared column shapes for subsequent scan setup.
- Regression: TinyCC fixture verifies both IPC calls on success, the returned kind-6 handle and relation shape, and rejection if the capability response returns a different relation ID.
- Verification: focused and full maintained TinyCC SDK suites passed; generated ABI consistency, documentation link checks, and `git diff --check` passed.
- Remaining: open a scan using the validated capability and interpret bounded plan operators.

## OP-3 dependency: open a scan only after plan validation

- Behavior: the SDK validates a physical plan through executor metadata, acquires the ordinary executor handle, and opens its full scan. A zero LIMIT produces an empty result without creating a cursor.
- Implementation: `prdb_sdk_optimizer_open_plan_scan_v1` composes plan decoding, stable-identity comparison, executor relation resolution, and full-scan open. It returns the checked relation metadata and connection-local cursor token in caller-owned buffers.
- Regression: TinyCC fixture verifies the metadata and capability requests precede `open_scan`, the returned cursor has kind 7, and LIMIT 0 performs no cursor request.
- Verification: focused and full maintained TinyCC SDK suites passed; generated ABI consistency, documentation link checks, and `git diff --check` passed.
- Remaining: consume bounded batches, evaluate Filter, project rows, honor nonzero LIMIT, and emit SQL results.

## OP-3 dependency: consume projected rows from bounded scan batches

- Behavior: a bounded C executor runs an unfiltered plan over immutable storage batches, emits projection-ordered values, polls cancellation, stops at LIMIT, and closes early cursors. Filter plans fail as unsupported before scan creation.
- Implementation: `prdb_sdk_optimizer_execute_unfiltered_plan_v1` composes validated metadata/capability/scan setup with a 126-page budget so two IPC calls per page plus setup and an early close stay within the 256-request executor stream limit. The SQL optimizer bridge additionally spends one request checking the returned plan against the submitted AST, so its effective page ceiling is 125. It polls cancellation, applies the stateful projection/LIMIT batch consumer, and closes the cursor on cancellation or early stop. Values borrow the current batch only during the result callback.
- Regression: TinyCC fixture checks source-ordinal projection, callback value/type, limit completion, request order, cursor close after early LIMIT, cancellation abort/cleanup, and rejection of filtered plans.
- Verification: focused and full maintained TinyCC SDK suites passed; generated ABI consistency, documentation link checks, and `git diff --check` passed.
- Remaining at this increment: evaluate Filter predicates, encode and emit SQL results, and add a real executor channel integration covering the plan-driven batch path. Filter evaluation is implemented in the following increment.

## OP-3 dependency: evaluate bounded Filter plans over scanned rows

- Behavior: the SDK evaluates validated plan predicates against each immutable scanned row before projection. SQL three-valued logic is preserved, including short-circuit behavior for AND/OR; only TRUE rows are emitted. Predicate errors such as REDACTED inputs fail plan execution and trigger normal cursor cleanup.
- Implementation: `prdb_sdk_optimizer_consume_plan_batch_v1` decodes the bounded prefix tree, resolves scanned columns by stable ID and ordinal, evaluates scalar comparisons through the shared SQL evaluator, and integrates with the bounded executor scan loop. False and UNKNOWN rows do not count toward LIMIT.
- Regression: TinyCC fixture checks that a matching row emits, a nonmatching row is skipped, and the bounded executor completes a Filter scan with early LIMIT and cursor close.
- Verification: focused and full maintained TinyCC SDK suites passed; generated ABI consistency, documentation link checks, and `git diff --check` passed.
- Remaining: submit WHERE predicates through the SQL optimizer bridge and add a real process/channel execution fixture. SQLR row emission is implemented in the following increment.

## OP-3 dependency: connect optimizer submission to unfiltered SQL execution

- Behavior: a parsed explicit-projection SELECT is encoded and submitted through ABI v4, its successful plan is checked against the submitted relation/projection order/LIMIT and executor-authorized column names, and the unfiltered plan is run through bounded storage pages. Unsupported predicates return to the existing SQL path without latching statement failure; optimizer or execution failures latch failure.
- Implementation: `prdb_sdk_optimizer_execute_select_ast_v1` composes canonical request encoding, the dedicated optimizer callback, response decoding, query-to-plan semantic checks, and the bounded executor plan runner. The extra metadata request is included in the 125-page budget.
- Regression: TinyCC fixture submits parsed SELECTs, checks optimizer/executor callback order and one projected result, accepts case-folded relation/projection spellings, rejects a changed projection before scanning, verifies WHERE fallback makes no optimizer call, and checks that optimizer error status latches statement failure.
- Verification: focused and full maintained TinyCC SDK suites passed; generated ABI consistency, documentation link checks, and `git diff --check` passed.
- Remaining: submit WHERE predicates through the SQL optimizer bridge, encode/emit SQL results, and exercise the path with a real optimizer/executor IPC invocation. Returned unfiltered plans are compared with the submitted SELECT and emitted as SQLR rows; the bridge leaves WHERE queries on the established SQL path.

## OP-3 dependency: launch and confine the optimizer with each invocation

- Behavior: production START launches optimizer and executor as separate confined processes in one invocation cgroup. The optimizer receives only its submission and planner sockets; storage serves its planner socket alongside executor planner and storage channels. Either launch failure cleans up the other process and invocation resources.
- Implementation: `ExecutorLaunchConfig` and `prdb-supervisor --optimizer` select the optimizer executable. The launcher stages independent command policies, starts and reaps both children, and retains the optimizer endpoint in the executor. Storage drains three worker channels concurrently and aborts all peers on handler failure. The optimizer installs `no_new_privs` and the worker seccomp filter before serving.
- Verification: the serial workspace suite passed (82 library unit tests; 129 commit integration tests; three cgroup-dependent commit tests and one cgroup-dependent process test ignored). After the final child-reaping change, workspace build, formatting, warning-denied Clippy, and all four launcher unit tests passed. Generated ABI consistency and `git diff --check` passed. End-to-end confined launch remains unverified because the host has no delegated cgroup test fixture.
- Remaining: route an optimizer request from the executor ABI, validate returned plans again against executor storage, and interpret SELECT plans through SDK operations.


## OP-3 dependency: emit projected optimizer rows as SQLR records

- Behavior: the supported optimizer SELECT path writes each projected row as a provisional SQLR v1 record using executor-authorized column names and declared types. Result callback failures fail the statement so the invocation cannot commit partial output.
- Implementation: `prdb_sdk_sql_write_projected_result_v1` encodes and writes a projected single-relation row in host-sized chunks. `prdb_sdk_optimizer_execute_select_ast_to_result_v1` adapts bounded plan execution to that writer.
- Regression: TinyCC fixture executes a submitted SELECT through ABI v4, captures the SQLR record, checks its projection name/type/state/bytes, and verifies a failed result callback latches statement failure.
- Verification: focused and full maintained TinyCC SDK suites passed; generated ABI consistency, documentation link checks, and `git diff --check` passed.
- Remaining: submit WHERE predicates through the optimizer bridge and add a real process/channel execution fixture.


## OP-3 dependency: encode bounded SQL predicates in optimizer requests

- Behavior: callers can serialize parsed single-relation SELECT predicates and supplied positional values into the existing optimizer request v1 format without changing the ABI wire version. Unsupported forms remain eligible for the existing SQL path.
- Implementation: `prdb_sdk_optimizer_encode_select_with_parameters_v1` serializes comparisons, Boolean AND/OR/NOT trees, and Boolean/INTEGER/TEXT/NULL values into the bounded prefix representation expected by the Rust decoder. The compatibility encoder continues to reject WHERE trees.
- Regression: TinyCC fixture checks an exact comparison request payload and rejects missing parameters; unfiltered request encoding remains byte-for-byte stable.
- Verification: focused and full maintained TinyCC SDK suites passed; generated ABI consistency, documentation link checks, and `git diff --check` passed.
- Remaining: route the validated parameterized bridge through a real optimizer/executor process and channel integration.


## OP-3 dependency: submit and execute validated parameterized WHERE plans

- Behavior: supported parameterized SELECT predicates are submitted to the optimizer, and the SDK rejects a returned Filter unless its operator tree, scanned column names, comparison operators, and values match the submitted AST and parameters. Matching plans execute through bounded filtered scans and can emit SQLR rows.
- Implementation: `prdb_sdk_optimizer_execute_select_ast_with_parameters_v1` composes parameterized request encoding, ABI v4 optimizer submission, executor metadata validation, AST-to-Filter comparison, filtered batch execution, and sticky statement failure. `prdb_sdk_optimizer_execute_select_ast_with_parameters_to_result_v1` emits provisional SQLR records for matching rows.
- Regression: TinyCC fixture checks successful filtered submission/execution/result emission and rejects a plan whose literal differs from the supplied parameter. Unsupported predicates remain on the existing SQL path.
- Verification: focused and full maintained TinyCC SDK suites passed; generated ABI consistency, documentation link checks, and `git diff --check` passed.
- Remaining: exercise the path through the confined optimizer/executor processes and channels; add bounded mutation plans.

## OP-3 dependency: verify filtered optimizer process output against executor storage

- Behavior: a parameterized equality SELECT crosses the real optimizer subprocess submission and planner channels; the returned Filter retains its bound column identity and literal, and executor storage independently resolves the plan's qualified relation and scanned column shape over its own IPC channel. The fixture then obtains a normal relation handle, opens a scan using the plan's scanned column IDs, and consumes a finished bounded batch.
- Regression: `planner_stream_estimate_allowance_falls_back_without_aborting_invocation` submits both an unfiltered request and `value = 7` with LIMIT, checks the decoded Filter node, compares schema/table identity plus every scanned column's ID, ordinal, type, and nullability with executor metadata, and confirms the empty fixture table returns no rows and no continuation. The existing unavailable-relation check remains on the optimizer process stream.
- Verification: focused Rust process/channel integration and format check passed. Workspace build, formatting, and warning-denied Clippy passed; the full workspace test run hit `InvalidAudit` in `storage_manager_creates_and_drops_check_constraint`, which passed alone, and the serial commit integration suite passed (129 passed, 3 cgroup-dependent tests ignored). The new scan assertion is being verified separately.
- Remaining: run the C SDK callback, Filter evaluation, and result path against a real confined optimizer/executor invocation; add bounded mutation plans.

## OP-1 / OP-3 dependency: malformed optimizer requests fail without stopping the process

- Behavior: a structurally malformed request receives `InvalidRequest` without reaching planner metadata, and the same optimizer subprocess remains able to serve a later valid request.
- Regression: `planner_stream_estimate_allowance_falls_back_without_aborting_invocation` sends an empty SELECT payload to the real optimizer child, verifies the status and request ID, then proceeds through valid unfiltered and filtered submissions plus the unavailable-relation case. `optimizer_process::tests` sends a malformed IPC frame, a duplicate request ID after an `InvalidRequest` response, and 256 invalid requests followed by request 257, checking frame rejection, duplicate-ID rejection, and the connection request ceiling.
- Verification: the focused process-loop tests and real-process integration passed. Locked workspace build, formatting, warning-denied Clippy, and full tests passed; 3 cgroup-dependent commit tests and 1 cgroup-dependent process test were ignored.
- Remaining: run the SDK callback, Filter evaluation, and result path in a confined invocation; add bounded mutation plans.

## OP-3 dependency: execute the filtered SQL callback through confined optimizer and executor processes — verified

- Contract: a parameterized `SELECT id FROM records WHERE id = $1 LIMIT 1` must pass through the real optimizer submission process, be independently checked against authorized executor metadata, scan through the executor channel, and return the matching SQL value from the confined artifact.
- Regression: `sandboxed_v3_sql_dml_sequence_is_visible_and_commits` runs the maintained ABI-v4 `sql_v3_write_read.c` fixture. Its v4 entry submits the equality predicate with parameter 42, confirms the returned row after prior insert/update operations in the same invocation, and reports the result through the normal callback path.
- Verification: the ignored acceptance passed under a transient `systemd --user` unit with delegated `cpu memory pids`, using the built TinyCC fixture and configured glibc/libgcc runtime paths. The full acceptance also checked its SQLR result frames and durable COMMITTED outcome after terminal delivery failed. `sandbox-query-fixture` was already current; the test ran exactly as `cargo test --locked --test commit sandboxed_v3_sql_dml_sequence_is_visible_and_commits -- --ignored --exact --nocapture` inside the delegated unit.
- Remaining: bounded mutation plans and broader optimizer planning remain open.

## OP-3 dependency: define and validate the canonical bounded DELETE plan — implemented

- Contract: [optimizer specification](../../specs/planner-compiler.md), mutation-plan format. A DELETE plan carries an authorized logical relation, a Boolean WHERE predicate, the minimal sorted scan shape, and a Delete sink; it never carries an executor handle or write authority.
- Implementation: `DeletePlanV1`, `plan_delete_v1`, `encode_delete_plan_v1`, `decode_delete_plan_v1`, and `validate_delete_plan_v1` implement the canonical v1 `Scan -> Filter -> Delete` layout. Constant predicates encode zero scan columns to request the full declared row shape. Predicate column collection now enforces the existing node/depth bounds.
- Red/green evidence: the round-trip test first failed to compile because the DELETE planner/codec did not exist; it now verifies a two-column sorted scan and canonical round-trip, rejects a corrupted Delete sink, exercises constant Boolean predicates, and rejects non-Boolean or over-depth predicates.
- Verification: all 19 `optimizer_plan::tests` passed in the locked Nix shell. The hand-checked-byte round-trip test was rerun exactly after its expected payload was added. Workspace build, format check, warning-denied Clippy, and full locked tests passed (130 library unit tests; 4 cgroup-dependent tests ignored by default; active integration and doc tests passed). `git diff --check` passed.
- Remaining: bind a versioned mutation request through the invocation metadata connection, serve it through the isolated optimizer process, decode it independently in the SDK, and execute it through the normal authorized mutation channel. UPDATE plans and bounded mutation-planning integration remain open.

## OP-3 dependency: decode and bind bounded DELETE optimizer requests — implemented

- Contract: [optimizer specification](../../specs/planner-compiler.md), structured request v1. DELETE requests must carry one bounded Boolean WHERE tree, resolve only within the invocation's authorized metadata view, and create no executor capability.
- Wire format: generated statement tag `PRDB_OPTIMIZER_DELETE_STATEMENT=2`; the v1 header is followed by schema/relation identifiers and a nonempty length-prefixed predicate. It has no projection or LIMIT fields and reuses the established predicate node, text, depth, and request-size bounds.
- Implementation: `decode_delete_request`, `bind_delete_request`/`bind_delete_request_via_metadata`, and `optimize_delete_v1`/`optimize_delete_v1_with_relation` decode, resolve and type-check the predicate, then return the canonical DELETE plan bytes from the previous increment. The shared generated Rust/C ABI constant is derived from `engine/abi/query_abi_constants.json`.
- Red/green evidence: decoder and binder tests initially failed to compile because their entry points were absent. The regressions now verify a parameter-free integer equality predicate, stable authorized column IDs, missing/mismatched relation non-disclosure, Boolean-root enforcement, mandatory WHERE, and rejection of trailing request bytes. The service regression decodes the output and compares its relation, scan column, and bound predicate.
- Verification: the focused request, binder, and service tests passed; `engine/tools/generate_query_abi.py --check` passed. The full workspace build, format check, warning-denied Clippy, and locked tests also passed; four cgroup-dependent tests were ignored by default. Host Nix 2.34.9 rejects `nix develop --locked`, so checks used `nix develop -c` and the repository's `flake.lock`.
- Remaining: add a dedicated optimizer IPC operation and response validation for kind-2 plans, then add C SDK request/decoder and execution integration. UPDATE request and plan support remains open.

## OP-3 dependency: serve DELETE plans through optimizer process IPC — implemented

- Contract: [optimizer specification](../../specs/planner-compiler.md). DELETE uses a distinct optimizer submission operation and the existing invocation-bound planner metadata channel. The process returns only a canonical validated plan; storage remains the mutation and authorization boundary.
- Wire operation: generated `PRDB_IPC_OP_OPTIMIZE_DELETE=0x1205`, separately allowlisted for the `OptimizerSubmission` role. The process decodes the request before metadata lookup, routes to the DELETE service helper, and keeps operation/request IDs on the response.
- Response validation: the existing bounded optimizer response envelope now independently accepts either a canonical SELECT plan or canonical DELETE plan, rejecting malformed payloads as before.
- Evidence: the process regression covers metadata lookup, DELETE dispatch, response IDs, and kind-2 plan decoding; response round-trip covers both plan kinds; IPC tests assert the operation code and role authorization.
- Verification: focused optimizer-process and IPC tests passed. Full workspace build, format check, warning-denied Clippy, and locked tests passed; four cgroup-dependent cases were ignored by default. ABI generator `--check` and `git diff --check` passed. As recorded above, host Nix 2.34.9 requires `nix develop -c` rather than the unsupported `nix develop --locked` form; the repository flake lock remains in use.
- Remaining: C SDK DELETE execution integration, then UPDATE request/plan support.

## OP-3 dependency: independently decode DELETE plans in the C SDK — implemented

- Contract: [optimizer specification](../../specs/planner-compiler.md). The SDK must independently validate plan kind 2 and the exact `Scan -> Filter -> Delete` pipeline before any execution integration.
- Implementation: `prdb_sdk_optimizer_decode_delete_plan_v1` validates identities, zero-to-256 sorted scan columns, the required bounded Boolean filter tree, Delete sink, node count, and complete payload. Constant predicates correctly allow an empty scan shape. Existing SELECT decoder entry points remain SELECT-specific.
- Evidence: the TinyCC fixture accepts a constant-Boolean DELETE plan and checks output kind/shape and borrowed predicate bytes; it rejects kind mismatches, incorrect terminal nodes, and malformed Boolean literals.
- Verification: `nix develop -c make -f engine/sdk/Makefile test` passed, including ABI generator checks, TinyCC SDK builds, and all maintained SDK executables. Workspace Rust checks passed in the immediately preceding process-IPC increment; this increment changes only C SDK and plan documentation.
- Remaining: integrate DELETE execution with independently resolved executor handles and ordinary authorized scan/mutation IPC; then continue UPDATE request and plan support.

## OP-3 dependency: add operation-aware optimizer callback ABI v5 — implemented

- Contract: [query worker ABI](../../specs/query-worker.md), [client SDK ABI](../../specs/client-sdk.md), and [planner specification](../../specs/planner-compiler.md). ABI v4 remains fixed to SELECT; DELETE submission requires a new ABI record so callers cannot reinterpret the old callback.
- Implementation: ABI v5 preserves the complete v4 host table prefix and adds an operation parameter. The confined executor validates the v5 descriptor before dispatch, supplies both callbacks, and applies the existing `OptimizerSubmission` role allowlist. PMF1 encoders/validators accept v5 with its exact symbol pair.
- Evidence: Rust ABI layout and descriptor validation, operation forwarding/denial, ELF manifest validation, and TinyCC C layout/callback fixtures pass. The v4 fixed-operation fixture still passes.
- Verification: workspace build, formatting, warning-denied Clippy, full locked tests, ABI generator check, and TinyCC SDK tests passed.
- Remaining: execute matching row handles through authorized scan/stage-delete IPC.

## OP-3 dependency: encode and submit bounded C DELETE requests — implemented

- Contract: [optimizer specification](../../specs/planner-compiler.md). Parsed DELETE statements require a bounded Boolean WHERE tree; the SDK callback grants only the `OptimizeDelete` submission operation.
- Implementation: `prdb_sdk_optimizer_encode_delete_v1` writes canonical statement tag 2 requests with schema/relation identifiers and a mandatory prefix predicate. `prdb_sdk_optimizer_submit_delete_v1` uses ABI v5's operation-selected callback and validates the bounded optimizer response envelope.
- Coverage: the maintained TinyCC fixture checks exact wire bytes, missing parameter rejection, unsupported `IS NULL`, no partial output on a short buffer, null output-buffer validation, callback operation selection, and successful response decoding.
- Verification: the pinned Nix shell's maintained SDK suite and ABI generator checks passed. Workspace build, format check, warning-denied Clippy, and full locked tests passed (four cgroup-dependent tests ignored by default). The initial test attempt inherited a root-only Nix temporary directory and failed unrelated permission-sensitive tests; rerunning with `TMPDIR=/tmp` as the repository user passed.
- Remaining: execute matched row handles through authorized scan/stage-delete IPC. UPDATE request and plan support remains open.

## OP-3 dependency: verify DELETE plans against the SQL AST — implemented

- Contract: [optimizer specification](../../specs/planner-compiler.md). A returned plan must match the submitted relation, predicate columns, operators, and bound parameter values before its relation capability is acquired.
- Implementation: `prdb_sdk_optimizer_validate_delete_plan_with_parameters_v1` independently decodes the kind-2 plan, compares its filter tree to the parsed DELETE AST using authorized planner metadata, and checks stable relation identity. The shared relation validator and relation-acquisition path now decode either SELECT or DELETE plans.
- Coverage: the TinyCC fixture accepts a matching integer predicate, rejects a changed parameter and a changed column, and acquires the independently resolved relation handle for the DELETE plan.
- Verification: focused plan fixture, maintained TinyCC SDK suite, and ABI generator checks passed in the pinned Nix shell.
- Remaining: scan authorized rows, evaluate the verified filter, stage matching row handles through executor mutation IPC, and add confined process acceptance. UPDATE request and plan support remains open.

## OP-3 dependency: execute verified DELETE plans through SDK mutation IPC — implemented

- Contract: [optimizer specification](../../specs/planner-compiler.md). Only after plan-to-AST and stable relation checks may the executor scan authorized rows; matching opaque handles are staged through its ordinary mutation operation.
- Implementation: `prdb_sdk_optimizer_execute_delete_ast_v1` submits with ABI v5, validates the returned plan, opens the independently resolved full scan, binds the DELETE predicate to executor relation metadata, polls cancellation between bounded pages, collects matching row handles, closes incomplete scans, and stages deletes. It enforces the executor request budget before staging.
- Coverage: the maintained TinyCC fixture drives a parsed parameterized DELETE through operation-selected optimizer submission, relation validation and acquisition, one executor batch, predicate evaluation, and one successful stage-delete acknowledgement.
- Verification: focused optimizer plan fixture and maintained TinyCC SDK suite passed in the pinned Nix shell; ABI generator checks passed.
- Remaining: exercise DELETE through the confined optimizer/executor process acceptance harness and continue UPDATE request/plan support.

## OP-3 dependency: decode bounded UPDATE optimizer requests — implemented

- Contract: [optimizer specification](../../specs/planner-compiler.md). UPDATE requests identify a bounded assignment set and require a Boolean WHERE tree; names and values remain untrusted until invocation-bound binding.
- Wire format: generated statement tag `OPTIMIZER_UPDATE_STATEMENT=3`; after schema/relation identifiers come 1–64 assignments, each containing a column identifier and either a nonzero parameter index or NULL, followed by a nonempty bounded predicate.
- Implementation: `decode_update_request` validates the request envelope, identifiers, assignment count/kinds, parameter indices, predicate size/tree, and trailing bytes. It does not resolve names, infer assignment types, or grant mutation authority.
- Coverage: Rust tests decode mixed parameter/NULL assignments and reject missing WHERE, invalid assignment kinds, zero parameter indices, and trailing bytes. ABI generation and format checks passed.

## OP-3 dependency: bind bounded UPDATE requests against invocation metadata — implemented

- Contract: [optimizer specification](../../specs/planner-compiler.md). Binding resolves the target relation and assignment/predicate columns only through authorized planner metadata and produces no executor handle or write grant.
- Implementation: `bind_update_request` and `bind_update_request_via_metadata` bind parameter/NULL assignments to stable column identities, preserve assignment order, reject unknown and duplicate targets, reject NULL for non-nullable columns, and require a Boolean predicate root.
- Parameter values remain untyped in this optimizer request and must be checked against the parsed SQL AST and the storage mutation contract before staging.
- Coverage: tests verify stable identity binding, source-order assignments, predicate binding, duplicate case-insensitive targets, hidden columns, nullability, missing relation, and non-Boolean WHERE.
- Remaining: lower UPDATE values through the SDK and validate their runtime types before staging.

## OP-3 dependency: lower bound UPDATEs to canonical plans — implemented

- Contract: [optimizer specification](../../specs/planner-compiler.md). UPDATE plans carry stable relation/column identities and the bounded predicate/assignment data but no handles or write authority.
- Wire format: generated kind-3 plan with `Scan -> Filter -> Update` nodes. The scan includes the ordinal-sorted union of assignment targets and predicate references. The generated Update node header carries 1–64 source-order assignments; every assignment contains a full bound column record and a fixed value record for either a nonzero parameter index or NULL with a zero index.
- Implementation: `plan_update_v1`, `encode_update_plan_v1`, `decode_update_plan_v1`, and `validate_update_plan_v1` check relation identity, complete scan shape, Boolean predicate, target uniqueness, scanned target metadata, parameter bounds, NULLability, and trailing bytes.
- Coverage: focused tests round-trip a parameterized plan and reject NULL to a non-nullable target, zero parameter index, duplicate targets, missing scan references, invalid assignment encoding, and trailing bytes. The ABI generator check passed.
- Remaining: independently decode and validate the kind-3 plan in the C SDK.

## OP-3 dependency: compose authorized UPDATE planning service — implemented

- Contract: [optimizer specification](../../specs/planner-compiler.md). The service composes request decoding, invocation-authorized relation binding, canonical lowering, and plan encoding; it does not create an executor capability.
- Implementation: `optimize_update_v1` uses invocation-bound planner metadata, and `optimize_update_v1_with_relation` composes the authorized description path for a relation resolved over planner IPC.
- Coverage: service tests decode the produced kind-3 plan, verify stable relation identity and scan shape, preserve the parameter assignment, and check non-disclosing missing-relation behavior.
- Remaining: route the request through the isolated optimizer process and independently verify it in C.

## OP-3 dependency: serve UPDATE plans through optimizer process IPC — implemented

- Contract: [optimizer specification](../../specs/planner-compiler.md). UPDATE uses a distinct optimizer submission operation and the same authorized planner metadata channel as SELECT/DELETE.
- Implementation: generated IPC op `OptimizeUpdate=0x1206`, role allowlist, request name decoding, relation metadata resolution, service dispatch, and response plan validation now include kind-3 UPDATE plans.
- Coverage: IPC code/role tests, optimizer response round-trip test, and a two-stream process protocol test verify relation lookup and validated UPDATE plan output.
- Remaining: submit UPDATE ASTs, verify plan-to-AST values, and execute through authorized scan/stage-update IPC.

## OP-3 dependency: independently validate UPDATE plans in the C SDK — implemented

- Contract: [optimizer specification](../../specs/planner-compiler.md). The C SDK independently checks optimizer output before trusting target columns or obtaining an executor relation capability.
- Implementation: `prdb_sdk_optimizer_decode_update_plan_v1` validates kind 3 and the `Scan -> Filter -> Update` pipeline, resolves predicate references against scanned IDs, enforces the exact predicate/assignment scan union, and validates unique targets, parameter indices, fixed NULL records, and target nullability. Generic relation validation now accepts UPDATE plans only through this independent decoder.
- Coverage: TinyCC fixture tests a valid parameterized UPDATE plan, malformed/truncated assignment values, target nullability, output/payload overlap, and executor relation re-resolution.
- Verification: maintained TinyCC SDK suite, all eight ABI generator tests, and ABI generated-file check passed in the pinned Nix shell.
- Remaining: encode and submit SQL UPDATE ASTs, match decoded assignments against AST and parameter types, then execute via authorized scan/stage-update IPC.

## OP-3 dependency: encode and submit UPDATE ASTs from the C SDK — implemented

- Contract: [optimizer specification](../../specs/planner-compiler.md). Serialize a parsed UPDATE with required WHERE, ordered assignments, bounded parameters, schema and relation names; submit only on ABI v5's `OptimizeUpdate` callback.
- Implementation: `prdb_sdk_optimizer_encode_update_v1` checks the UPDATE AST shape and assignment spans, rejects duplicate targets and invalid parameter references/values, then writes the canonical statement-3 request with assignment records and prefix predicate. `prdb_sdk_optimizer_submit_update_v1` uses the operation-selected callback and the shared bounded response decoder.
- Regression: the maintained TinyCC fixture parses a mixed parameter/NULL UPDATE, compares the complete request against hand-checked bytes, verifies undersized-buffer and invalid-parameter behavior, and checks callback operation and successful response decoding.
- Observed failure: the first TinyCC run found the in-progress validation change had accidentally put an UPDATE-only field check in the DELETE encoder and left the old projection shape in UPDATE. The checks were moved to the correct branches; the full maintained SDK suite then passed.
- Verification: pinned Nix shell reported Rust/Cargo/Clippy 1.95.0, rustfmt 1.9.0 and TinyCC 0.9.27-unstable-2025-01-06. `make -f engine/sdk/Makefile test`, locked workspace build, format check, warning-denied Clippy, serial locked workspace tests, and `git diff --check` passed. Rust tests: 138 passed, 4 ignored; the ignored tests include cgroup-dependent acceptance paths.
- Boundary: this is request encoding/submission evidence only; it does not yet validate the returned UPDATE plan against the AST or stage writes.
- Remaining: match decoded assignments against the parsed AST and parameter types, then execute via authorized scan/stage-update IPC and confined-process acceptance.

## OP-3 dependency: validate UPDATE plans against the SQL AST — implemented

- Contract: [optimizer specification](../../specs/planner-compiler.md). Before obtaining an executor capability, independently verify that the optimizer's predicate and source-order assignments match the parsed SQL and its typed parameters, then re-resolve the relation through executor storage.
- Implementation: `prdb_sdk_optimizer_validate_update_plan_with_parameters_v1` checks UPDATE AST shape, plan schema/relation and assignment count, predicate structure/operator/columns/parameter values, assignment names/order/kinds/indices, exact parameter scalar types and nullability. It calls `resolve_plan_relation` and validates stable identities and scanned column definitions before returning success.
- Regression: the maintained TinyCC optimizer-plan fixture accepts a matching plan and rejects assignment-index and assignment-type mismatches; the matching path verifies executor-side relation resolution.
- Verification: targeted optimizer-plan fixture and full maintained TinyCC SDK suite passed in the pinned Nix shell; ABI generator's eight tests and generated-file check passed.
- Limitation: this validates plans but does not scan or stage UPDATE writes. Parameter assignment compatibility currently requires an exact scalar type code; coercions are unspecified and remain unsupported by this path.
- Remaining: execute matched UPDATE plans over an authorized full scan, stage typed patches through executor mutation IPC, and verify the path through confined optimizer/executor processes.

## OP-3 dependency: execute matched UPDATE plans through executor IPC — implemented; confined acceptance passed

- Contract: [optimizer specification](../../specs/planner-compiler.md). Run the SQL predicate only over the independently resolved relation's authorized full-shape batches, collect matching opaque row handles, close the scan, then stage the exact verified patch set through ordinary executor UPDATE IPC.
- Implementation: `prdb_sdk_optimizer_execute_update_ast_v1` submits and validates the plan, acquires a normal relation handle, binds/evaluates the SQL AST predicate over sanitized rows, polls cancellation, bounds pages and target handles, closes the cursor before mutation, and stages each patch through `stage_update`. Any execution/staging failure latches statement failure; the output count is published only after all stages succeed.
- Regression: maintained TinyCC optimizer-plan fixture executes a matching parameterized UPDATE through mocked optimizer/executor callbacks, checks the operation and typed patch bytes, and confirms one row was staged with no failure latch.
- Verification: targeted optimizer-plan fixture and full maintained TinyCC SDK suite passed in the pinned Nix shell; ABI generation check and all eight generator tests passed.
- Observed acceptance correction: the first confined run staged the UPDATE successfully but the fixture expected three rows; an earlier DELETE had removed the original row, leaving the two later inserts. The fixture now expects two updated rows, verifies their value, then confirms optimizer-backed DELETE removes them. The delegated-cgroup acceptance passes.
- Confined acceptance: the maintained `sql_v3_write_read.c` artifact now exports ABI v5, runs `UPDATE records SET id = $2 WHERE id = $1` through optimizer submission, selects both updated rows, then runs optimizer-backed `DELETE FROM records WHERE id = $1` and confirms no rows remain. The ignored `sandboxed_v3_sql_dml_sequence_is_visible_and_commits` acceptance passed through authenticated staging, confined executor and optimizer processes, result relay, and storage commit in a delegated `systemd --user` cgroup.
- Multi-column acceptance: the same confined fixture now updates both `left_key` and `right_key` on `grouped_records`, then runs a second optimizer UPDATE whose predicate matches both assigned values. Two affected rows confirm both assignments became visible through ordinary executor storage and mutation IPC. The acceptance was run in a delegated `systemd --user` cgroup and passed.
- Verification: `nix develop -c make -f engine/sdk/Makefile sandbox-query-fixture` rebuilt the fixture with pinned TinyCC; the confined `sandboxed_v3_sql_dml_sequence_is_visible_and_commits` test passed with the optimizer and executor processes, authenticated staging, durable commit, and result relay. The fixture setup grants only the grouped table/columns needed for this UPDATE and installs UPDATE table/cell policies.
- NULL assignment acceptance: the confined fixture now runs `UPDATE records SET id = NULL WHERE id = $1` through optimizer submission, verifies both rows as SQL NULL through the regular SELECT path, then restores their values through the authorized SQL UPDATE path so optimizer DELETE acceptance still runs.
- Verification: the rebuilt TinyCC sandbox fixture and delegated-cgroup `sandboxed_v3_sql_dml_sequence_is_visible_and_commits` test passed, including the NULL assignment, its readback, optimizer-backed DELETE, and durable commit.
- Remaining: cover cancellation and stage-update failure in the confined process harness, target exhaustion there, and transaction conflict/policy behavior.

## OP-3 dependency: cancel an in-progress optimizer UPDATE — verified

- Behavior: if executor cancellation arrives between opening an authorized scan and reading its next page, the SDK closes the cursor, stages no UPDATE, latches statement failure, and leaves the result count at its failure value.
- Regression: the maintained `sdk_optimizer_plan_v1.c` callback fixture now returns cancellation at that checkpoint and checks the cancellation status, cursor-close operation, zero stage-update calls, one failure latch, and zero published updated rows. The successful mock UPDATE remains covered.
- Verification: `nix develop -c make -f engine/sdk/Makefile test` passed, including all eight ABI generator tests and the full maintained TinyCC SDK suite.
- Remaining: cover cancellation in the confined process harness, optimizer response loss, target exhaustion, stage-update failure, and transaction conflict/policy behavior.

## OP-3 dependency: fail UPDATE when executor staging fails — verified

- Behavior: if the ordinary executor rejects StageUpdate after a completed scan, the SDK returns callback failure, latches statement failure, and leaves the updated-row count at zero.
- Regression: the maintained `sdk_optimizer_plan_v1.c` mock injects an executor callback error for StageUpdate. It verifies the row batch completed before staging, no success count is published, and exactly one failure is latched. The neighboring cancellation regression still verifies cursor closure before staging begins.
- Verification: `nix develop -c make -f engine/sdk/Makefile test` passed, including the full TinyCC SDK suite and all eight ABI generator tests.
- Remaining: exercise stage failure and cancellation through confined processes, test target exhaustion, and expand transaction conflict/policy cases.

## OP-3 dependency: optimizer submission callback failure aborts UPDATE — verified

- Behavior: an optimizer callback error before a plan response stops execution before executor relation resolution or mutation and latches statement failure.
- Regression: the TinyCC `sdk_optimizer_plan_v1.c` mock injects an optimizer callback error and checks the returned callback status, one failure latch, zero executor requests, zero staged updates, and zero published row count.
- Verification: `nix develop -c make -f engine/sdk/Makefile test` passed, including all ABI generator tests and maintained TinyCC SDK executables.
- Remaining: exercise target exhaustion and mutation cancellation/staging failures through confined processes, then expand transaction conflict/policy cases.

## OP-3 dependency: stop UPDATE when target capacity is exhausted — verified

- Behavior: when a bounded scan finds more matching rows than the caller's handle capacity, execution fails before staging any mutation and publishes no updated-row count.
- Regression: the TinyCC optimizer-plan fixture supplies a two-row matching batch with capacity for one target. It checks `REQUEST_LIMIT`, one sticky failure, zero StageUpdate calls, and zero updated count.
- Verification: `nix develop -c make -f engine/sdk/Makefile test` passed, including all ABI generator tests and maintained TinyCC SDK executables.
- Remaining: exercise cancellation, target exhaustion, and staging failure through confined processes, then expand transaction conflict/policy cases.

## OP-3 dependency: reject an optimizer UPDATE without storage write authority — verified

- Behavior: a readable relation remains available to optimizer planning, but its absent UPDATE grant must reject staging and abort the invocation without committing.
- Regression: the confined SQL artifact submits a parameterized UPDATE against `linked_records`, for which the admitted user has SELECT access but no UPDATE grant. It emits a provisional ready event, then the executor/storage path rejects the mutation; the gateway reports `QueryError` and storage records an `Aborted` outcome.
- Verification: the fixture compiled with pinned TinyCC using `make -f engine/sdk/Makefile sandbox-query-fixture`; `sandboxed_v3_sql_dml_sequence_is_visible_and_commits` passed inside a delegated `systemd --user` cgroup with real optimizer and executor processes.
- Remaining: confined cancellation and explicit staging-failure injection, plus broader transaction conflict/policy acceptance.
