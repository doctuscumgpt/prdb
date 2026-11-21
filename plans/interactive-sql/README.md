# Interactive SQL Client Implementation Plan

## Objective and Dependencies

Implement the proposed [interactive SQL sessions and transactions contract](../../specs/interactive-sql.md), including `BEGIN`/`COMMIT`/`ROLLBACK`, connection-independent transaction attachment, `RUNAS username ... ENDRUNAS`, multiple credential profiles, and autocommit DDL through typed `ADMIN` operations. This is a cross-component feature; the client alone cannot keep an MVCC context alive across query packages.

Preserve the current v1 one-package/one-transaction protocol and behavior. The new feature requires a separately versioned client protocol/API, supervisor/storage-owned transaction state, and a SQL SDK batch entry point. Keep SQL text parsing and native execution in the existing trusted client and confined SDK boundaries; no SQL is sent to the gateway and no user artifact receives credentials or transaction-control authority.

Coordinate with [client protocol](../client-protocol/README.md), [client gateway](../client-gateway/README.md), [client SDK](../client-sdk/README.md), [administrative SQL contract](../../specs/admin-sql.md), [SQL library](../sql-library/README.md), [supervisor](../supervisor/README.md), [transactions/MVCC](../concurrency-mvcc/README.md), [storage manager](../storage-manager/README.md), [authorization](../authorization/README.md), [identity/authentication](../ddl-identity/README.md), and [sandbox](../linux-sandbox/README.md). Update normative specs before implementation when a decision below changes the current proposal.

## Decisions Before Implementation

Freeze these contracts in `specs/interactive-sql.md` and all affected protocol/component specs before adding cross-process interfaces:

- New protocol major/version and capability negotiation; v1 must reject or ignore no new transaction semantics and must never be used as a silent fallback.
- Exact v2 authentication payload encoding and auth-proof forwarding; explicit-transaction control and attachment authenticate the controller, while each RUNAS package authenticates its selected actor. A RUNAS package inside another user's explicit transaction proves both identities; autocommit RUNAS needs only the actor credential because that actor owns the implicit package transaction.
- V2's first release is loopback-only. Remote access waits for confidential authenticated transport.
- Runtime limits are set in the proposal to a five-minute idle timeout, the configured 15-minute default transaction lifetime, 128 packages, 65,536 staged rows, 64 MiB staged writes/package artifact, and the existing storage active-invocation quotas; outcome status uses the configured invocation-outcome retention period.
- Exact result-event wire encoding and output buffering/streaming details. Statement ordinals and no result replay are fixed; an uncertain package reply reconciles through PACKAGE_STATUS and is never replayed.
- Which SQL SDK statements are allowed in batch mode. DDL and transaction-control statements remain forbidden inside RUNAS.
- Protocol frame constants, operation JSON schemas, and golden fixtures; semantic v2.0 operations and kinds 13–18 are now specified.

The existing spec chooses that possession of the target user's valid token authenticates RUNAS without a separate impersonation grant, that DDL cannot run inside a transaction or RUNAS block, the controller alone controls commit/rollback, credential files reuse `PDB-CREDENTIALS-1`, and RUNAS overlays are authorization-filtered. Revisit those choices explicitly if security review changes them.

## Client Delivery Sequence

The user-facing command is a composition layer over the parser, credential loader, package builder, query protocol, and typed DDL client. It must not keep a socket open to keep MVCC state alive. Keep the existing `submit` and `admin-sql` commands unchanged; add a distinct `interactive-sql` command and make script mode use the same dispatcher as the REPL.

Implement the client in this order, after the protocol and storage prerequisites in ISQL-1 through ISQL-6 expose stable typed APIs:

1. **Command and session configuration.** Add `prdb-client interactive-sql` options for gateway address, controller credential file/default username, repeated `--credential-profile USER=FILE`, schema/scope, and the existing SDK/compiler/runtime package profile. Reuse `ClientCredentialProfiles` and the established safe SQL input and package-building code. Validate the default profile and all paths before opening a network connection. Keep credential bytes in zeroizing storage and redact them from errors.
2. **Single sequential dispatcher.** Feed interactive lines or bounded script input through `interactive_sql` parsing and dispatch one parsed item at a time. Track only client-visible mode (`Autocommit` or `Explicit { transaction_id, state }`), controller profile, and whether the server has marked the transaction abort-only. Do not pipeline statements. Keep parser classification separate from server authorization.
3. **Transaction lifecycle over short-lived connections.** Map `BEGIN`, each package, `COMMIT`, `ROLLBACK`, and status reconciliation to the typed v2 client API. Each operation may open a fresh TCP connection. Explicit-transaction operations authenticate the controller; a RUNAS package also authenticates its selected actor. An autocommit RUNAS package authenticates the actor as owner of its implicit transaction. Preserve transaction/package IDs and never repeat an uncertain package or commit. Recover by querying status/outcome on a newly authenticated connection.
4. **Package and result path.** Compile an ordinary query statement to its own package; compile the whole RUNAS body to one batch package using `generate_query_batch_source` and the maintained SQL batch SDK. Submit it against the active explicit transaction or through the existing autocommit invocation lifecycle. Decode statement-indexed result events, stop on the first package failure, and mark explicit-transaction output provisional until commit succeeds.
5. **Typed DDL path and cleanup.** Route supported top-level DDL through existing `ddl_sql` parsing and `AdminClient` operations only in autocommit mode. Reject DDL inside BEGIN or RUNAS before dispatch and leave an otherwise active transaction unchanged. Wait for durable ADMIN outcome before advancing. On EOF, interrupt, and ordinary errors, close/rollback an active transaction where possible; if the outcome is uncertain, report the transaction ID and reconciliation action without claiming rollback.

Keep the dispatcher independent of terminal I/O so script tests and an eventual REPL exercise identical state transitions. A script without `BEGIN` remains statement-autocommit: earlier successful statements stay committed after a later failure. Results from an explicit transaction must be labeled provisional and must not be presented as committed data until COMMIT reports a durable committed outcome.

### Client-Specific Acceptance

- CLI option tests prove duplicate/missing/unknown options, missing default profiles, invalid credential mappings, invalid scope/compiler profiles, and malformed SQL fail before network submission.
- Dispatcher tests use a scripted transport to prove exact call ordering for autocommit, BEGIN/package/COMMIT, BEGIN/package/ROLLBACK, RUNAS in both modes, DDL, parser errors, package errors, and EOF. Assert explicit-transaction operations carry controller authentication, explicit-transaction RUNAS carries both proofs, autocommit RUNAS carries the actor proof, and each operation can use a distinct connection.
- Failure-injection tests lose replies after BEGIN, package execution, COMMIT, ROLLBACK, and DDL. Verify the client uses ATTACH/STATUS/PACKAGE_STATUS or the existing ADMIN outcome lookup as appropriate and never replays a non-idempotent operation.
- Package tests assert one ordinary statement per package and one complete RUNAS block per package; an error in any batch statement prevents later statements and yields one failed package outcome.
- Output tests distinguish statement ordinal, package completion, and transaction commitment. Results from an active explicit transaction remain provisional after package completion and are only confirmed after durable COMMIT.
- A focused real-gateway test executes a multi-package transaction across fresh connections, includes a RUNAS package under a second profile, and confirms that DDL is a separate durable ADMIN operation outside the transaction. Full acceptance remains ISQL-8 and uses the confined executor harness.

## Increments

| ID | Observable failure first | Minimum implementation | Acceptance evidence |
| --- | --- | --- | --- |
| ISQL-1: Contract and protocol fixtures | A v1 peer accepts or ambiguously interprets a transaction/RUNAS request | Resolve the decisions above; define v2 operation schemas for BEGIN, ATTACH, package execution, COMMIT, ROLLBACK, STATUS, and dual controller/actor authentication; add independent golden request/response fixtures and explicit v1 rejection. | Version/capability matrix, malformed/duplicate/out-of-order frames, authentication mismatch, transaction ID non-capability, bounded payloads, and v1 compatibility tests pass. |
| ISQL-2: Client credentials and block parser | Bad RUNAS boundaries or a wrong credential profile reach network submission | Add private profile loading/zeroization and the interactive SQL lexer/parser for transaction commands, DDL routing, and non-nested RUNAS blocks. Classify only supported statements; do not use semicolon splitting or trust the username field as identity. | Unit tests cover strings/comments containing ENDRUNAS, nested/missing/empty blocks, size/count caps, normalized names, profile missing/mismatch/revocation paths, secret redaction, and invalid DDL placement without opening a connection. |
| ISQL-3: One-package SQL batch SDK | A RUNAS body is split into multiple invocations or a later statement error leaves a successful package result | Add a versioned bounded batch entry point (or generated wrapper contract) that runs up to the selected statement cap sequentially, uses typed parameters, emits statement-indexed result boundaries, and latches any statement failure. Keep all statements in the block in one package transaction. | Maintained TinyCC SDK tests cover ordered results, shared writes within the package, malformed/multiple input, output callback failure, statement error after prior writes, result ordinal framing, and package-wide abort. Run code only through the confined harness. |
| ISQL-4: Storage transaction handle across packages | A second package creates a new snapshot or cannot see the transaction's own writes | Refactor transaction ownership from a single invocation into a bounded storage transaction context with Begin/Attach/Execute/Commit/Rollback/Status semantics. Retain one snapshot, private overlay, conflict set, limits, and durable outcome across disposable package executors. It must survive client TCP disconnects and reattach only after controller authentication. | Storage-level tests demonstrate same-snapshot reads, read-your-authorized-writes, isolation from concurrent transactions, all-or-nothing commit/rollback, stale-write conflicts, idle expiry, and no reattachment by ID alone. Process restart aborts undecided transactions and preserves committed outcome lookup. |
| ISQL-5: Per-package effective actor | A RUNAS user inherits controller grants or reads another actor's private writes without authorization | Bind each package to exactly one authenticated actor; add actor-aware authorization projection over the retained snapshot and transaction overlay; record write actors and revalidate their credential/authority epochs at commit. Keep transaction-control APIs controller-only. | Two-user storage tests cover exact target identity, role/policy changes, denied reads/writes despite stronger controller grants, authorized reads of visible prior writes, hidden private writes, and commit rejection after actor credential/authority revocation. |
| ISQL-6: Gateway, supervisor, and protocol v2 | A detached transaction is connection-bound, can be used concurrently, or accepts forged actor/controller identity | Implement v2 gateway auth and transaction registry; authenticate the controller on every reattachment/control operation and actor independently for RUNAS packages; serialize one operation per transaction; bind disposable executor host capabilities to server-selected transaction and actor. Keep ADMIN on its control role. | Real TCP tests reattach from fresh connections, disconnect/reconnect before commit, reject cross-user takeover, duplicate/concurrent requests, actor mismatch, forged transaction/actor fields, and ADMIN from worker IPC. Exercise lost reply, cancellation, lease expiry, and restart. |
| ISQL-7: Interactive dispatcher and autocommit DDL | The client routes DDL through a query artifact or silently commits around DDL in BEGIN | Compose parser, profiles, SDK batch package, transaction v2 operations, and existing typed AdminClient calls. Default profile controls ordinary/autocommit DDL; RUNAS selects a profile per block; DDL is rejected inside explicit transactions and RUNAS without changing an active transaction. | CLI tests cover scripts and interactive sequences across autocommit, BEGIN/COMMIT/ROLLBACK, RUNAS under multiple profiles, independent DDL outcomes, stopping after error, exact statement order, and credential cleanup. Verify uncertain operations are reconciled, never replayed. |
| ISQL-8: Confined end-to-end acceptance | Unit and mock tests pass while the real TinyCC package crosses a trust boundary incorrectly | Exercise the new path with actual client, v2 TCP gateway, supervisor, storage, SDK package, and disposable confined executor. Keep authorization, quotas, cancellation, outcome reconciliation, and durable publication enabled. | Delegated-cgroup integration proves one RUNAS block equals one package, multiple packages share one explicit transaction across fresh connections, commit/rollback outcomes are correct, DDL remains separate, failures abort atomically, and hostile artifacts cannot access credentials, ADMIN, or transaction control. Run applicable locked Rust/C checks from `AGENTS.md`. |

## Dependency Order and Review Gates

The increment IDs describe ownership and acceptance; the practical implementation order is:

```text
ISQL-1 contract and wire fixtures
   ├── ISQL-2 parser and credentials ──┐
   ├── ISQL-3 batch SDK ───────────────┤
   └── ISQL-4 transaction registry ────┴── ISQL-5 actor authorization
                                                │
                                      ISQL-6 gateway/supervisor v2
                                                │
                                      ISQL-7 CLI dispatcher and DDL
                                                │
                                      ISQL-8 confined acceptance
```

ISQL-2, ISQL-3, and the storage core of ISQL-4 can proceed as independent slices once ISQL-1 freezes their shared IDs, limits, and error states. ISQL-5 depends on the retained transaction overlay from ISQL-4. ISQL-6 integrates the stable package and actor APIs from ISQL-3 through ISQL-5. ISQL-7 uses only the typed v2 and DDL APIs established by ISQL-6 and the existing `AdminClient`. Do not start end-to-end acceptance by substituting a mock for any of those boundaries.

At every increment, preserve these review gates:

1. **Identity and privilege:** authenticate the controller for transaction lifecycle operations and the selected actor for every RUNAS package. Never pass credentials to the executor. Prove actor permissions do not inherit controller grants, including for staged-row visibility and commit-time revalidation.
2. **Transaction lifetime:** retain the MVCC snapshot and write set in trusted storage state, independent of TCP connections. A fresh authenticated connection must attach; the transaction ID alone must confer no authority. Serialize package/control operations and release state on terminal outcome or expiry.
3. **No replay after uncertainty:** assign stable transaction/package/administrative operation IDs before dispatch. Resolve lost replies through authenticated status APIs; never rerun a package, commit, rollback, or DDL operation whose outcome may already be durable.
4. **Statement routing:** compile each ordinary query as one package and each complete RUNAS body as one batch package. Runas works both with an implicit autocommit transaction and within the current explicit transaction. Keep supported DDL on the typed ADMIN path and reject DDL inside BEGIN or RUNAS before dispatch.
5. **Compatibility and confinement:** retain existing v1 behavior without silent fallback, keep DDL and transaction control unavailable to query artifacts, and run native artifacts only in the confined executor. Maintain focused unit evidence, then real gateway/process evidence, then the delegated-cgroup suite.

Each gate is an invariant across all later increments, not a final-stage hardening task. If a design change affects one, add or update its failure-injection and boundary test in the same increment.

## Cross-Component Acceptance

Do not mark this feature complete from a local SQL parser, storage unit test, protocol mock, or client-only workflow. Acceptance requires the real network path and confined executor. Specifically test:

- A block executed in autocommit uses one implicit transaction; the same block inside BEGIN uses the already-open transaction and does not commit it.
- SQL in a RUNAS block is one query package even when it contains several statements; all statements use exactly the selected user's authorization, and any statement failure invalidates the package.
- The transaction controller can reconnect on a new TCP connection, but the transaction ID is never sufficient to attach, query status, commit, or roll back.
- A transaction owner can attach a separate RUNAS actor only with both authenticated proofs; the actor cannot commit the transaction and the controller's grants do not augment actor permissions.
- DDL uses the separate typed control API, commits independently in autocommit mode, and is rejected without implicit boundaries inside BEGIN/RUNAS.
- Lost replies, gateway/supervisor restart, client crashes, revoked tokens, policy changes, output backpressure, and transaction resource expiry have bounded outcomes and cleanup.

Use explicit barriers and deadlines for races. Keep v1 fixtures and maintained SDK fixtures passing unchanged. Hostile query/SDK libraries run only in the delegated-cgroup sandbox harness described in `AGENTS.md`; no privileged process loads submitted native code.

## Handoff

Record each increment in a future `PROGRESS.md` with: normative sections, first observed failing assertion, changed interfaces, actor/transaction trust assumptions, exact commands/results, delegated-cgroup status, unresolved limits, and the next smallest increment. No implementation or runtime behavior is claimed by this plan.
