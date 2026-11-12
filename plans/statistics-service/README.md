# Statistics Service Implementation Plan

## Contract and Dependencies

Implement [statistics](../../specs/statistics-service.md) as a trusted module within [storage](../storage-manager/README.md). Depend on [authorization](../authorization/README.md), [snapshots/overlays](../concurrency-mvcc/README.md), [dictionary validity](../data-dictionary/README.md) and planner-role [IPC](../ipc-protocol/README.md). A separate collector process and global statistics are not initial prerequisites.

## Initial Implemented Slice

The internal `InvocationTransaction::estimate_authorized_scan` API returns exact visible-row and requested-column VALUE/NULL/REDACTED counts from the pinned snapshot plus private overlay, or `Unknown` without partial counts. It scans at most 16,384 candidates per estimate and the planner metadata connection admits at most 16 estimates over its lifetime; exhaustion returns `Unknown` while leaving the invocation active. It returns exact results only for policy shapes with row inclusion independent of row values: literal policies can prove a full population or an empty population without scanning rows. Value-dependent policies and invalid contexts return `Unknown` before scanning. Ordinary unordered single-table SELECT can also opt into the separate bounded population probe, which reads at most five row keys, returns a count only for populations of 0–4, and never decodes row values. An exact empty population can produce the validated no-scan hint; exact populations 1–4 can select a sequential scan over an indexed path using the measured local cutoff. Both planner estimate operations share the same cumulative allowance. `planner_ipc` dispatches relation descriptions and estimates through connection-scoped kind-9 handles. The trusted transaction layer additionally supports exact predicate-match counts for at most four rows when all referenced cells are readable; redacted or unsupported predicates return `Unknown`. The planner-only EstimateScan operation now accepts bounded bound-predicate requests by qualified relation name and returns exact visible/matching counts or `Unknown`. The SDK re-resolves the relation and checks cancellation before skipping an empty scan. The IPC role matrix denies estimate operations to executors and executor scan operations to planners. `SupervisorInvocation::serve_worker_channels` provisions the planner and executor storage streams concurrently and drains both before worker-exit observation. Optimizer request scheduling, use of predicate selectivity to alter plans, broader estimate-driven optimizer choices, and planner task scheduling remain unimplemented.

## Decisions Before Further Implementation

Extend the bounded trusted predicate contract through planner IPC, including request accounting and unsupported responses, before optimizer use. Define cache validity across principal/grant/policy/data/schema/private-write generation. Start without cross-principal reuse. Fix which logical access-path information is public; never release physical counts, global epochs/costs or raw samples indirectly.

## Increments

| ID | Observable test first | Minimum implementation and evidence |
| --- | --- | --- |
| ST-1 | Executor calling estimate/refresh and planner requesting unrestricted data are denied | The operation matrix covers the pure role check. Provisioned-channel dispatch and dedicated planning handles remain |
| ST-2 | Scoped visible/readable/null/redacted populations match a reference authorized scan | The full summary, statically empty population, bounded exact population probe, and bounded trusted predicate-match estimator for 0–4 rows are implemented; planner-facing predicate estimates and broader policy-filtered estimates remain |
| ST-3 | Changing hidden values or adding invisible rows leaves normalized released estimates unchanged | Paired authorized-view comparisons now verify unchanged visible rows and normalized `Unknown` estimates across differing hidden candidates; timing/resource noninterference and broader estimate fields remain |
| ST-4 | Estimate-specific exhaustion returns unknown, but context cancellation/deadline drains and terminates | The transaction estimate returns no partial result on exhaustion and checks cancellation/deadline at row and response checkpoints; asynchronous planner-task draining and heuristic integration remain |
| ST-5 | A private write or changed authorization context prevents stale summary reuse and cross-invocation leakage | Transaction-local write generation and complete validity keys; safe invalidation even when notifications are lost |
| ST-6 | Later private collection is bounded, undisclosed and cannot starve commits | Optional trusted committed-snapshot collectors, protected rebuildable stats store and separately admitted maintenance; no planner-triggered unrestricted refresh |

## Acceptance and Handoff

Use authorized-view reference fixtures with nulls/redactions and repeated chosen-predicate probes. A sample lacking redactions is not permission to execute an aggregate, and readable-only summaries never redefine SQL semantics. Record planning overhead/quality on selective policies and mixed workloads before richer sampling/caches. Timing/cache noninterference is not established by normalized-content tests. First behavior: a role-denied estimate request, then a bounded exact visible-row summary.
