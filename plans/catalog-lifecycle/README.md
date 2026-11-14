# Catalog Lifecycle Implementation Plan

## Contract and Dependencies

Implement [bootstrap/startup/offline upgrades](../../specs/catalog-lifecycle.md) using the pinned [redb backend](../storage-manager/README.md), [dictionary](../data-dictionary/README.md) and [credential profile](../ddl-identity/README.md). Initialization never requires SQL, SDK, optimizer or user code. Supervisor admission waits for validated READY.

## Decisions Before Implementation

The local directory profile, one-time secret handoff over a permission-restricted Unix socket, interrupted-handoff token rotation, `prdb-init` setup client, checksummed offline READY backup/verification, and read-only READY inspection are implemented. Compatibility gates reject unsupported catalog, expression, and built-in semantics versions; family-16 credential rows carry and validate an algorithm and profile version. The CL-5 path-level publication design now uses a same-directory atomic exchange of the active and staging redb filenames, leaving redb page ownership with redb. A version matrix now distinguishes service support, legacy source validation/backup, and executable transitions. Migration inspection can compare active/staged bytes to a verified backup, and unpublished staging can be discarded only when the backup proves the active source is unchanged. Catalog-v1 migration now has a bounded one-transaction record transformer and an offline `migration-apply` command that requires a verified source backup plus an explicitly acknowledged complete identity-history file. The command validates and atomically exchanges the v2 target while retaining the source generation. An explicit `restore` command can recover an unreadable active image from a verified v2 backup only with a data-loss acknowledgment; it retains the displaced file. Old-binary rejection, archived redb compatibility fixtures, broader power-loss evidence, and remaining release acceptance are still open. Distinguish redb format, engine/catalog, policy IR, verifier and built-in semantics versions from network/ABI/plan versions.

## Increments

| ID | Observable test first | Minimum implementation and evidence |
| --- | --- | --- |
| CL-1 | Bootstrap descriptor resolves reserved records without SQL; duplicate initialization cannot replace committed identities | Minimal trusted descriptor, database UUID, tables/indexes and recoverable initialization transaction |
| CL-2 | Failure at every handoff/state boundary resumes safely without a default credential or plaintext persisted secret | Protected local first-admin/token delivery, explicit lost-delivery reconciliation and one-time verifier persistence |
| CL-3 | Unknown version, required record/index corruption or redb recovery error blocks READY | Open with pinned supported redb, validate logical tables/catalog/dependencies/authority and publish readiness only after success |
| CL-4 | An older binary/unsupported verifier or IR cannot silently read-write a newer directory | Explicit independent compatibility gates and populated cross-version fixtures; ordinary startup performs no automatic migration |
| CL-5 prerequisite | A READY source can be preserved and independently re-opened before conversion | Exclusive offline lock, private byte copy, durable checksummed manifest, read-only READY validation, and repeatable verification |
| CL-5 | Offline migration interruption before publication preserves source; after publication startup uses completed target | Stop admission/drain, exclusive access, verified backup/source generation, trusted bounded conversion and redb-supported atomic generation publication |
| CL-6 | Upgrade preserves IDs, data, grants/policies and verifier semantics; post-upgrade writes prevent unsafe automatic rollback | Validate target before switch, retain source for documented window, test reverse migration/explicit restore boundaries and rebuild disposable derived caches |

## Acceptance and Handoff

The CL-1 bootstrap core and CL-2 local socket handoff are implemented with real isolated-file tests for duplicate initialization, lock exclusion, successful delivery/acknowledgement, disconnect recovery, stale socket cleanup and pre-commit recovery. The `prdb-init` CLI now initializes or resumes an instance, persists credentials only through the atomic mode-0600 writer, and publishes READY only after local handoff acknowledgement. Continue with interruption-point coverage, multiple identities/policies and populated data. Use supported redb open/upgrade procedures; never edit private headers, roots or checkpoint metadata. The generation publication design must be reconciled with redb ownership in specs before implementation. Do not claim upgrade safety before a supported transition graph and executable fixtures exist.
