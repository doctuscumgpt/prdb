# Workload and Integration Progress

## WI-7 follow-up: sandboxed cancellation beside operational commits — covered

- Regression: the enabled `sandboxed_v3_sql_dml_sequence_is_visible_and_commits` acceptance starts a second confined cancellation fixture alongside four independent operational invocations. Each performs an authorized point read and full scan, then waits at a commit gate; the test releases all four commits as it submits cancellation and requires the SDK cancellation poll to be relayed within two seconds, followed by a durable ABORTED outcome.
- Verification: the focused delegated-cgroup acceptance passed (1 passed, 0 ignored). The final focused run measured the owner cancellation request at 30 µs and the SDK poll relay at 11.619 ms, with all four operational transactions held at the commit gate as cancellation was launched; no commit completed before the query process exited. `cargo build --workspace --locked`, `cargo fmt --all -- --check`, warning-denied workspace Clippy, maintained TinyCC SDK tests, and the full serial delegated-cgroup workspace suite passed; commit integration reported 142 passed/0 ignored and transaction integration 74 passed/0 ignored.
- Scope: this covers the real cgroup/Landlock query path and cancellation competing with operational reads/commits on the same local redb database. The observation is one local sample, not a latency target; it does not yet exercise cancellation during an active analytical query or a saturated multi-client analytics workload.

## WI-7 follow-up: physical page reclamation after snapshot release — verified

- Regression: [`retained_reader_blocks_compaction_until_release_then_reclaims_deleted_pages`](../../engine/tests/redb_foundation.rs) holds a 64-row, 100-KiB-per-row snapshot while deleting 58 rows. It verifies compaction is rejected with `TransactionInProgress`, the old snapshot still reads deleted values, and after release compaction reduces file size and allocated pages while preserving the six current rows.
- Verification: all four `redb_foundation` tests passed. Workspace build, formatting, warning-denied Clippy, and the full serial delegated-cgroup workspace suite passed; commit integration reported 142 passed/0 ignored and transaction integration 74 passed/0 ignored. The fuller storage-manager progress record describes the exact fixture and limitations.
- Scope: this establishes redb-level history release and explicit compaction on the local x86_64 Linux `/tmp` profile. It does not establish managed service scheduling, compaction latency during operational load, or target-filesystem behavior.

## WI-7 follow-up: cancellation releases snapshot admission during writer load — covered

- Regression: [`managed_cancellation_releases_snapshot_admission_during_write_pressure`](../../engine/tests/transaction.rs) fills the configured managed-snapshot cap, verifies new work is rejected, accepts cancellation and observes it through the executor metadata checkpoint, then proves a new admission succeeds while an unrelated staged writer and retained reader remain active. The cancelled overlay is durably ABORTED and unpublished; the retained reader preserves its original value during 32 concurrent durable writes.
- Verification: focused real-redb regression passed (1 passed, 0 ignored). Workspace build, formatting, warning-denied Clippy, and the full serial delegated-cgroup suite passed; commit integration reported 142 passed/0 ignored and transaction integration reported 74 passed/0 ignored.
- Scope: this exercises local storage admission and concurrent commits. It does not measure physical redb history reclamation, cancellation relay through the external gateway, or cancellation during saturated sandbox SQL.

## WI-7 follow-up: bounded outcome maintenance during retained reads and commits — measured

- Harness: [`storage_pressure.rs`](../../engine/examples/storage_pressure.rs) seeds 256 expired terminal outcomes, then runs four operational writers (256 durable commits), a retained authorized reader, and repeated outcome-prune transactions together. Each maintenance transaction scans at most 16 outcomes; a 5 ms pause between batches deliberately yields CPU and writer admission. The production supervisor currently schedules outcome maintenance every 60 seconds, so this is a synthetic saturation profile rather than its normal cadence.
- Repeatability: five sequential fresh-database runs on Linux x86_64, Rust 1.95.0, redb 4.4.0. The median of run-level commit p50/p95/p99 was 5.730/15.595/21.117 ms operational-only and 10.383/14.495/21.385 ms under retained authorized reads plus pruning. Mixed phases completed in 0.650–0.677 s (378.1–393.7 commits/s), 9,693–10,495 full scans, 38,772–41,980 point reads, and removed all 256 expired outcomes in 43–45 bounded transactions.
- Verification: the five release harness runs completed; `cargo build --workspace --locked`, `cargo fmt --all -- --check`, warning-denied workspace Clippy, maintained TinyCC SDK tests, and the full serial delegated-cgroup workspace suite passed. The commit integration target reported 142 passed and 0 ignored.
- Interpretation: a prior unpaced harness run let read and maintenance loops spin, producing up to 43,592 prune transactions and writer p99 values above 10 seconds. Adding cooperative reader yielding and a 5 ms inter-batch pause eliminated that artificial scheduler starvation. The final repeated sample shows higher median commit p50 under this local mixed workload; p95/p99 overlap with operational-only runs. These results are comparative evidence, not service-level targets or a model of the configured 60-second maintenance cadence.
- Remaining: cross-process saturated SQL analytics, output/planning queue pressure, cancellation/reclamation during load, memory/copy accounting, and target-hardware repeats remain open.

## WI-7 follow-up: authorized point reads and commits during confined SQL — measured

- Harness: the confined SQL acceptance compares authorized point reads and `commit()` durations across four operational threads. Baseline uses 64 reads and commits before START; the analytics phase records only operations that finish while the confined executor process is still live. Point-read time covers `read_catalog_authorized_row`, including pinned schema/grant/policy loading and projection; commit time covers `commit()` only.
- Profile and result: five sequential fresh-database runs on Linux x86_64, Rust 1.95.0, redb 4.4.0. Median of run-level p50/p95/p99 was 1.420/2.250/3.241 ms for point reads at baseline and 1.786/3.088/3.994 ms with SQL active. Commit medians were 11.626/15.378/20.622 ms at baseline and 11.544/16.710/20.198 ms with SQL active. Concurrent phases yielded 139–163 active-worker samples.
- Verification: the catalog-derived point-read regression, five focused delegated-cgroup runs, workspace build, formatting, warning-denied Clippy, and full serial delegated workspace tests passed; the commit integration target reported 142 passed and 0 ignored.
- Scope: these short runs use a three-row table and one SQL worker; they are local comparisons, not saturation-capacity evidence or latency targets. Larger scans/joins/sorts, cancellation/reclamation pressure, maintenance, memory/copy accounting, and target-hardware repeats remain open.

## WI-7 follow-up: commit latency beside confined SQL — measured

- Harness: the enabled `sandboxed_v3_sql_dml_sequence_is_visible_and_commits` acceptance measures `commit()` duration for authorized read-only operational invocations. It compares 64 baseline commits across four operational threads with commits completed while the confined SQL executor is still live; each phase uses the same fresh three-row table and authorization path.
- Profile and result: five sequential fresh-database runs on Linux x86_64 with Rust 1.95.0 and redb 4.4.0. The median of the run-level p50/p95/p99 values was 12.507/17.560/18.367 ms for the baseline and 12.033/15.660/17.375 ms alongside SQL. Baseline run p95 ranged 13.885–25.573 ms; concurrent SQL p95 ranged 15.337–16.162 ms. Each concurrent phase produced 152–164 samples whose commit completed while the executor process remained live.
- Verification: all five focused delegated-cgroup acceptance runs passed. `cargo build --workspace --locked`, `cargo fmt --all -- --check`, warning-denied workspace Clippy, and the full serial delegated workspace suite passed; the commit integration target reported 142 passed and 0 ignored.
- Scope: timing covers durable `commit()` only, excluding admission and scan work. This is a small local comparison, not evidence of saturation capacity or a configured latency target; point-read measurements, larger analytics, maintenance, memory/copy accounting, and target-hardware repeats remain open.

## WI-7 follow-up: operational storage work during confined SQL — covered

- Regression: the enabled `sandboxed_v3_sql_dml_sequence_is_visible_and_commits` acceptance now runs four independent operational threads alongside the real sandboxed SQL worker. Each invocation performs an authorized grouped-table scan and durable read-only commit; the bounded loops complete at least 64 invocations in total while worker-channel service/result relay runs, and the test requires a live executor-process sample.
- Verification: the focused acceptance passed through the delegated cgroup launcher. The cgroup probe reported writable `cpu`, `memory`, and `pids` delegation. The full locked workspace suite passed serially with the sandbox tests enabled; the commit integration target reported 142 passed, 0 failed, and 0 ignored. Workspace build, formatting, and warning-denied Clippy passed.
- Scope: this verifies concurrent correctness and admission on one small local dataset. Commit percentiles are recorded in the measurement below; sustained saturation, point-read latency, larger concurrent analytics, maintenance pressure, memory/copy accounting, and target-hardware repeats remain open.

## WI-7 dependency: authorized-estimate baseline cost — measured

- Harness: [`statistics_estimate.rs`](../../engine/examples/statistics_estimate.rs) builds a temporary READY redb database with 8,192 deterministic INTEGER rows, verifies an authorized exact summary, and reports repeated storage-estimate latency separately from setup.
- Profile and result: Rust 1.95.0, redb 4.4.0, x86_64 Linux release build; 11 estimates yielded 8.655 ms median and 12.073 ms p95. The result is a local baseline, not a service-level latency claim or configured target.
- Verification: release example completed; locked workspace build, format check, warning-denied Clippy, serial workspace tests (129 passed, 3 cgroup-dependent ignored), and maintained TinyCC SDK suite passed.
- Remaining: cross-process analytical saturation, operational latency under concurrent SQL analytics, maintenance pressure, memory/copy accounting, and repeatable target-hardware measurements remain open.

## WI-7 dependency: commit latency under retained-snapshot pressure — measured

- Harness: [`storage_pressure.rs`](../../engine/examples/storage_pressure.rs) records 256 operational-only durable commits, resets the four rows, then repeats 256 commits while a retained authorized snapshot performs full scans and point reads. Commit latency covers `commit()` only; row creation, encoding, and staging are excluded. The benchmark raises the managed reader scan-work limit to 2,000,000 so the single retained invocation can sustain this synthetic pressure interval.
- Profile and result: Rust 1.95.0, redb 4.4.0, x86_64 Linux release build. Operational-only p50/p95/p99 was 6.169/11.787/17.016 ms. During mixed load it was 9.004/11.184/16.042 ms; the retained snapshot completed 8,557 full authorized scans and 34,228 point reads while preserving its original values. The full mixed writer phase took 0.582 s (440.2 commits/s).
- Scope: one local run with four 32 KiB rows and four writer threads. The p95 difference is within the noise possible from a single run; this does not establish production SQL contention, service-level latency, or a target. Repeatable runs, SQL joins, maintenance pressure, and target-hardware profiles remain open.

## WI-7 dependency: repeatability sample for storage-pressure latency — measured

- Method: ran the already-built release harness five times sequentially. Each run created and removed a fresh temporary READY database and measured 256 commits in each phase. These are independent database runs on one host, not independent host or filesystem samples.
- Summary: the median of the five run-level p50/p95/p99 values was 5.910/12.708/18.127 ms operational-only and 8.921/13.383/19.172 ms under retained-snapshot scans. Across runs, the operational-only ranges were 5.644–5.960/11.646–15.286/15.699–21.524 ms; mixed-load ranges were 8.697–9.162/11.213–16.845/16.406–34.279 ms. Mixed throughput ranged from 415.3 to 450.7 commits/s, while the snapshot completed 8,297–9,124 scans per run.
- Interpretation: the repeated sample supports a higher median commit latency under this synthetic scan loop on this host, with a wide mixed p99 range. Keep it as local comparative evidence only; workload diversity, longer runs, host variation, SQL joins/sorts, maintenance, and target-hardware profiles remain open.

## WI-7 dependency: estimate latency across authorized row counts — measured

- Harness: [`statistics_estimate.rs`](../../engine/examples/statistics_estimate.rs) measures 11 exact authorized scan estimates at 1,024, 8,192, and 16,383 INTEGER rows, timing the estimate call separately from database setup, seeding, compilation, and planner IPC.
- Profile and result: Rust/Cargo 1.95.0, redb 4.4.0, Linux x86_64. Median/p95 was 0.901/0.975 ms, 7.847/11.258 ms, and 14.202/16.845 ms for the three sizes respectively. The largest dataset is one candidate below the estimate ceiling.
- Scope: a single local run on one host. This provides a population-size baseline for the storage API and does not measure concurrent SQL analytics, planner work, maintenance, target hardware, or service latency.
- Verification: the release harness completed successfully with the locked dependency set.

## WI-7 follow-up: repeated authorized-estimate latency sample — measured

- Method: ran the release estimate harness three additional times sequentially, with fresh temporary databases and 11 measurements at each dataset size.
- Run-level median latency across repetitions: 0.921 ms for 1,024 rows (range 0.899–0.930), 7.460 ms for 8,192 rows (7.124–7.587), and 15.943 ms for 16,383 rows (14.979–23.795). Median run p95 values were 1.406, 9.170, and 16.738 ms respectively.
- Scope: one host with uncontrolled cache and competing load; the 16,383-row p95 ranged from 15.835 to 27.299 ms. This measures the storage estimate call, excluding setup and planner IPC, and is not service-level or target-hardware evidence.
- Verification: each of the three release harness executions completed successfully.
