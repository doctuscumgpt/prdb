# Interactive SQL Sessions and Transactions

## Status and Scope

This document specifies a proposed higher-level SQL client and the server support it requires. It is not implemented behavior. It extends the current one-package/one-transaction submission model so a client can issue `BEGIN`, execute multiple query packages in one MVCC transaction, and finish with `COMMIT` or `ROLLBACK`. The client may also route supported DDL statements through the authenticated administrative control API.

The executor remains untrusted. It does not receive administrative credentials, does not own transaction state, and cannot commit or roll back storage directly. The trusted supervisor and storage manager own the transaction across package executions. Administrative DDL remains a separate control operation.

This extension does not promise transactional DDL. DDL is permitted only when the SQL client has no active explicit transaction; each DDL operation is an independent durable administrative operation. DDL inside `BEGIN` is rejected before dispatch and leaves the transaction active. The current v1 protocol and `prdb-client run-sql` behavior remain unchanged; this feature requires a separately versioned protocol/API that cannot silently fall back to v1.

## Client Statement Routing

The higher-level client accepts one statement at a time interactively or a bounded SQL script. A script is parsed into statements before execution; statement boundaries are found by a lexer/parser that understands strings and comments, never by splitting on every semicolon. The client executes statements sequentially and does not pipeline them.

The client classifies each statement and routes it as follows:

| Statement | Client action |
| --- | --- |
| `BEGIN` | Request a new explicit transaction from the query service. |
| Supported DML/query | Compile/package and execute it against the current transaction, or use one implicit transaction in autocommit mode. |
| `COMMIT` | Ask the query service to durably commit the active explicit transaction. |
| `ROLLBACK` | Ask the query service to abort and release the active explicit transaction. |
| Supported DDL | Only when no explicit transaction is active, translate SQL locally to a typed `ADMIN` control operation and wait for its durable outcome. |
| Other transaction/session commands | Reject as unsupported. |

DDL text is never sent to the query executor or gateway as executable SQL. DDL parsing and name resolution follow the existing authenticated `admin-sql` contract. SQL query parsing/binding/execution remains the versioned SDK behavior in the confined executor. The client may split and classify statements, but classification is not an authorization decision; storage independently validates every query and administrative operation.

When reading a script, the client may detect statement boundaries and obvious unsupported transaction syntax before sending anything. It cannot promise that all query statements are semantically valid before execution unless a compatible trusted parse-only SDK is available. In autocommit mode, statements completed before a later parse or execution error remain committed. In an explicit transaction, a query error aborts the whole transaction as specified below.

## Transaction Semantics

### Autocommit

Autocommit is the default. Each query/DML statement runs as one transaction and is committed or aborted using the current invocation lifecycle. A successful statement becomes durable before the client advances to the next statement. Thus a script without `BEGIN` has statement-level transaction boundaries, not script-wide atomicity.

Each DDL statement also runs independently through its existing authenticated administrative operation and durable outcome protocol. A successful DDL statement is durable before the client advances. A later script failure does not undo earlier query or DDL statements.

### Explicit transaction

`BEGIN` creates a transaction owned by the authenticated principal and bound to the query session. Storage opens one MVCC snapshot at that point and retains the transaction's private write set and validation state across multiple package executions. Each query package executes sequentially against this same context. Later packages see earlier writes from that transaction; other transactions do not see them before commit.

There is one transaction snapshot, one commit decision, and one durable transaction outcome for the entire explicit transaction. Individual package completion is provisional and does not publish writes. `COMMIT` asks the supervisor/storage path to validate and atomically publish the accumulated write set. Existing snapshot-isolation conflict rules, authority revalidation, and durable-commit requirements apply. A conflict or failed validation aborts the full transaction. `ROLLBACK` aborts it without publishing any writes.

`BEGIN` takes no arguments in the initial version. Isolation levels, read-only modes, nested transactions, savepoints, and transaction-local configuration are unsupported. `BEGIN` while a transaction is active, or `COMMIT`/`ROLLBACK` with no active transaction, returns a bounded `invalid_state` error and performs no state change.

### Failure behavior

Any query package error, SDK statement failure, worker crash, cancellation, deadline expiry, or fatal protocol error marks the explicit transaction abort-only and discards its private writes. The session may issue `ROLLBACK` to release the retained transaction state. Further query execution and `COMMIT` are rejected while abort-only; `COMMIT` reports the aborted outcome and closes the transaction. No partial package writes are retained for a later package. Savepoints and recovery within the transaction are not provided.

An unsupported DDL statement inside an explicit transaction is rejected by the client before sending it and does not make the transaction abort-only. The client may continue with query statements or issue `ROLLBACK`. The client must not transparently commit, rollback, or split the transaction to run DDL.

## Results, Cancellation, and Outcomes

Query results produced inside an explicit transaction are provisional until `COMMIT` returns a storage-confirmed committed outcome. The client must label or otherwise keep them distinguishable from committed results. `ROLLBACK`, abort, or an unknown transaction outcome means those results are not committed database output. The client cannot retract bytes already shown to an interactive user.

Cancellation of a running package aborts the enclosing explicit transaction. Cancellation after commit submission follows the existing commit barrier: the result may be committed, aborted, or unknown and must be reconciled, never inferred from a client acknowledgement or socket close.

Every explicit transaction has an opaque server-generated transaction ID. If the `COMMIT` reply is lost, the client reconnects and authenticates, then queries the transaction outcome by ID; it never sends `COMMIT` again blindly. A response of `unknown` requires reconciliation according to the transaction outcome contract and does not authorize retrying the work as a new transaction. Package execution requests are not replayed after an uncertain reply.

The first version does not resume an active transaction on a different connection. Losing the owning query connection, client process, gateway session, or supervisor session aborts any transaction that has not crossed the durable commit barrier. A lost commit reply is the exception: the transaction may already have reached a durable decision, so its outcome must be looked up. Committed outcomes are retained for the configured outcome-retention period.

## DDL Boundaries

The SQL client maintains separate authenticated query and control connections, or equivalent separately authorized logical channels. It uses the query service for `BEGIN`, query package execution, `COMMIT`, `ROLLBACK`, and transaction status. It uses the control service for DDL. Both are authenticated as the same database principal; neither connection's mode itself grants administrative authority.

DDL is allowed only when the SQL client is in autocommit state, with no active explicit transaction. The client waits for the DDL operation's durable outcome before sending the next statement. A successful DDL operation commits independently; a failed or uncertain DDL operation follows the existing ADMIN outcome lookup and no-replay rules. A subsequent query starts a fresh snapshot and observes DDL only if it is authorized and committed.

The initial client does not implement implicit transaction boundaries around DDL. It does not implicitly commit before DDL, nor automatically begin a new transaction after DDL. If a script reaches DDL while an explicit transaction is active, the DDL is rejected and the transaction remains active. Users must commit or roll back explicitly, then issue DDL.

## Server and Protocol Requirements

The existing client protocol v1 binds a query connection to one staged invocation and one invocation transaction. It cannot preserve a transaction across multiple START requests. This feature therefore requires a new protocol/API version with explicit transaction operations; sending new message kinds or `transaction_id` fields to a v1 peer is forbidden. A client requires the advertised transaction capability and fails closed if it is unavailable.

The new query service must provide semantic operations equivalent to:

- `BEGIN`: authenticate/revalidate the caller, reserve transaction resources, open the snapshot, and return a transaction ID and negotiated limits.
- `EXECUTE_PACKAGE`: bind one sealed query package to the active transaction, create a disposable executor, and run it sequentially. The artifact cannot choose or change the transaction ID. Package execution does not finalize the transaction.
- `COMMIT`: stop new package executions, validate authorization epochs and write conflicts, durably publish atomically, and return the transaction outcome.
- `ROLLBACK`: stop new work, discard private writes, release the snapshot and resources, and return an aborted outcome.
- `STATUS`: report an authorized transaction state/outcome, including reconciliation after a lost commit reply.

The supervisor/storage own the transaction ID, snapshot, write set, cumulative accounting, cancellation state, and durable outcome. Each executor remains a disposable, separately confined process and receives only invocation-scoped host capabilities bound by trusted server state to that transaction. The executor cannot access transaction control operations or name another transaction. A package's requested scope remains a per-package ceiling; transaction reuse does not widen any package's authority. Credentials are revalidated for each package and at commit as required by the authorization contract.

The server enforces bounded transaction lifetime, idle time, retained snapshot/write-set resources, active transactions per principal and globally, and concurrent package execution. Only one package may run in a transaction at a time. Limits are negotiated or reported at `BEGIN`, may be lowered by server policy, and cannot be raised by the client. Expiration aborts the transaction and releases its retained resources. Exact default values are an implementation-profile decision and must be specified before release acceptance.

## Compatibility and Failure Cases

- Existing v1 clients retain current behavior: one query package owns one transaction; DDL uses the control API.
- New clients require the transaction protocol version and reject unsupported versions/capabilities without silent fallback.
- Reusing a transaction ID under another principal, connection, or session is rejected.
- Duplicate, out-of-order, or concurrent package execution requests are rejected; there is no implicit multiplexing.
- A transaction ID is not a bearer capability. Every status/control request authenticates and checks ownership or current administrator authority.
- Gateway/supervisor restart aborts active transactions that have no durable decision. Durable commit outcomes remain recoverable through STATUS.
- DDL authorization failure, name-resolution failure, or uncertain outcome never changes the state of a query transaction, because DDL is not sent while one is active.

## Acceptance Evidence

The implementation must demonstrate, across the real client, gateway, supervisor, executor, and storage boundaries:

1. Two sequential packages in one explicit transaction read each other's writes, while a concurrent transaction cannot see those writes before commit.
2. COMMIT publishes the combined write set atomically; ROLLBACK publishes none of it.
3. A package failure, cancellation, crash, timeout, authorization-epoch change, or write conflict prevents the whole explicit transaction from committing.
4. DDL outside a transaction commits independently and is visible to a later transaction; DDL attempted inside an explicit transaction is rejected without silently committing or aborting that transaction.
5. A lost COMMIT reply is resolved by authenticated STATUS without replay; active transactions are released after owner disconnect and after every configured limit expires.
6. A hostile query artifact cannot read/write outside its per-package authorization, forge another transaction ID, invoke ADMIN, or commit/rollback its transaction.
7. Existing v1 submission, ADMIN, and sandbox integration behavior remains unchanged.

## Open Profile Decisions

- Concrete defaults for transaction idle/lifetime limits, private write-set and snapshot budgets, and active transaction quotas.
- Whether interactive result rows are streamed as explicitly provisional or buffered per package until package completion; neither choice may imply transaction commit.
- Exact v2 frame kinds, transaction status schema, and how client disconnect detection interacts with an in-flight durable commit.
- Which currently supported SDK query/DML statement forms are admitted in the first transaction-capable release.

These choices must be settled in the versioned protocol and runtime profiles before implementation acceptance; they do not change the rules that transactions span packages, DDL remains outside them, and storage alone decides durable outcomes.
