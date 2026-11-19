# SDK Library Repository Implementation Plan

## Contract and Dependencies

Implement the [SDK library repository specification](../../specs/sdk-library-repository.md). The repository is durable instance service state containing administrator-installed immutable SQL SDK shared libraries. Clients select a release in a protected persistent config and upload query.so only. The supervisor resolves and stages the exact repository release; only the confined executor loads it.

This work spans the [client protocol](../../specs/client-protocol.md), [gateway](../client-gateway/README.md), [supervisor](../supervisor/README.md), [client SDK](../client-sdk/README.md), [SQL command](../client-sql-command/README.md), [interactive SQL client](../interactive-sql/README.md), and [sandbox](../linux-sandbox/README.md). Keep normative contracts in `specs/`. Existing v1 SQL-library upload and deployment-cache behavior is a legacy protocol/configuration path; repository-enabled clients and services must negotiate the query-only profile and never silently fall back.

## Execution Order and Current Status

Implement the increments below in dependency order, with focused failing tests before each behavior and spec updates in the same increment. Keep each increment independently reviewable and commit it after its applicable checks pass.

```text
SLR-1 repository storage
  ├── SLR-2 authenticated API ── SLR-5 client config and repository commands
  ├── SLR-3 query-only submission profile
  └── SLR-4 supervisor resolve/pin/stage
                 SLR-2 + SLR-3 + SLR-4 + SLR-5
                               └── SLR-6 package and SQL client migration
                                            └── SLR-7 legacy retirement and end-to-end acceptance
```

SLR-1 through SLR-3 are implemented and committed. SLR-4 has a verified supervisor resolve/pin slice; confined executor staging acceptance remains. See [progress and verification evidence](PROGRESS.md). SLR-5 through SLR-7 remain. SLR-2, SLR-3, and SLR-4 were designed to proceed after SLR-1 in parallel, and their current implementations converge on the same release identity and query-only PREPARE contract for SLR-6. SLR-5 depends on the typed API for remote list/select/install/remove, while its local config parser and filesystem behavior can be developed independently. SLR-7 is the final retirement gate and must not remove compatibility paths until the repository-only route has end-to-end evidence.

For every increment, record in `PROGRESS.md`: the spec clauses implemented, the test that first exposed the missing behavior, the source and protocol changes, commands actually run, outcomes, and the next increment. A focused unit test establishes only the local contract; API, upload, and confinement claims require the corresponding real process boundary tests described below.

## Implementation Decisions

- Store repository bytes and canonical metadata in instance-local service state outside transactional user catalog records. Use a single immutable release per SDK release/digest/compatibility identity, synchronize writes, and atomically publish without replacement.
- Reuse the existing bounded ELF/manifest validation where appropriate, but validate administrator-supplied repository bytes independently at install and again before use. The trusted supervisor/storage path must never load either SDK or query code.
- Route list/install/remove through typed authenticated control operations. Listing exposes bounded compatibility metadata to authenticated users; install and remove require current database-admin authority. Never grant repository mutations through query IPC.
- Resolve and pin the repository entry before storage admission. Stage the library beside query.so read/execute-only, retain the pin through every worker cleanup path, and reject removal while pinned.
- Use one versioned client config with a default instance, per-instance IP/port, SDK release/profile/ABI, default credential and named credential profiles. Keep secrets out of artifacts and build-tool environments; apply the specified mode, ownership, no-follow and atomic-update checks.
- Keep compatible SDK binaries locally for compilation/linking. Do not place the SDK DLL in the package or query upload; the manifest binds query.so to the server repository release.

## Increments

| ID | Observable failure first | Minimum implementation | Acceptance evidence |
| --- | --- | --- | --- |
| SLR-1: Repository record and storage | An invalid, incompatible, oversized, or digest-mismatched release can become visible, or a completed release disappears after restart | Add strict bounded metadata decoding, instance-profile/ELF/digest checks, repository quota accounting, immutable file permissions, atomic no-replace install, atomic remove, and startup recovery for incomplete temporary entries | Valid release survives close/reopen; malformed metadata, wrong profile/SONAME/dependencies, digest corruption, quota overflow, and interrupted publication expose no partial release; identical identity/digest is idempotent and identity/different digest conflicts |
| SLR-2: Authenticated repository API | A non-admin can mutate installed libraries, or an admin can acknowledge an install before it is durable | Add typed `sdk_libraries.list`, `install`, and `remove` control operations with bounded metadata and byte streams, current-authority checks, durable operation outcomes, serialized same-identity mutation, and client-side response decoding | Real gateway/control tests prove ordinary authenticated list, admin-only mutations, revoked-admin rejection, idempotent install, conflicting install, bounded pagination, restart persistence, and correct outcomes after lost replies; executor IPC cannot reach these operations |
| SLR-3: Query-only submission profile | A repository-enabled client still uploads SQL library bytes or silently downgrades to a legacy peer | Define and negotiate the repository-enabled PREPARE profile containing query artifact, manifest, scope, parameters, and standard execution settings only; reject old SQL-library fields/chunks and reject unsupported peers without fallback | Golden client/server codecs prove exact fields and lengths; old-profile upload attempts reject; query-only package seals successfully; transport negotiation never falls back after mismatch |
| SLR-4: Supervisor resolve, pin, and stage | A missing release reaches storage admission, a pinned release is removed, or trusted code loads the library | Resolve the exact PMF release/ABI/target/profile before admission, verify repository bytes, retain a lifetime pin, stage under the versioned SONAME beside query.so, and release the pin on launch failure, cancellation, timeout, and worker exit | START fails before admission for missing/mismatched/corrupt release; concurrent resolution shares the immutable entry; removal is blocked while pinned and succeeds after cleanup; delegated confined acceptance loads the staged SDK and hostile constructors remain sandboxed |
| SLR-5: Persistent client configuration and repository commands | Client commands require ad hoc SDK release, endpoint, and credential flags on every run or accept unsafe config | Implement strict `PRDB-CLIENT-CONFIG-1` parsing/encoding, default instance lookup, protected atomic updates, credentials and named profiles, local tool paths, and `instance` / `sdk-library` list/select/install/remove commands | Config unit and filesystem tests cover canonical values, duplicate/unknown keys, permissions/ownership/symlinks, corrupt/truncated files, atomic update, credential zeroization, endpoint scoping, default selection and preserving unrelated entries; release list/select works as a non-admin |
| SLR-6: Package and SQL client migration | `run-sql`, interactive SQL, or package submit uses a release different from the saved selection, or submits sql.so | Make packager derive manifest compatibility from the selected instance setting and use the matching local SDK only as link input; make `run-sql` and interactive SQL use saved/default instance and credentials; make package `submit` compare its manifest to the saved selection; remove SDK DLL and digest sidecars from package/upload layout | Client tests prove generated package contains query.so only, selected SONAME/profile/ABI appear in PMF and ELF, mismatched package fails before upload, explicit override does not mutate saved config, RUNAS changes identity only, and release changes are rejected during an active interactive transaction |
| SLR-7: Retirement and end-to-end migration | Deployments still require cache flags or mixed old/new package modes can silently execute | Remove `--sdk-release-dir`, `--sdk-cache-dir`, cache eviction configuration, optional SDK upload support, and legacy fallback from repository-enabled startup; document explicit import or reinstall workflow and upgrade errors | Clean instance install/start using repository only; full client-to-gateway-to-supervisor-to-confined-executor run submits only query.so and commits; absent release rejects before execution; compatibility tests prove no static/cache/upload fallback |

## Cross-Increment Invariants

- Install/repository mutation is not a database catalog transaction and cannot be performed through SQL query execution.
- Every release is immutable. An SDK build change requires a new version identity; running workers keep their selected bytes for their complete lifetime.
- A package's PMF release, ABI, target and profiles must agree with the client setting and exact server repository entry. No nearest-version selection, default-server version substitution, automatic static fallback, or cache-miss fallback is permitted.
- The client SDK is a local build dependency only. The query package and submission carry query.so, manifest, scope, parameters and normal metadata; they do not carry the SDK shared object.
- SHA-256 provides integrity checking, not code trust. Native validation remains data-only in trusted processes; constructors execute only under the worker sandbox.
- A repository install/removal acknowledgement follows its durable outcome. A failed or uncertain install must be reconciled by operation status, not blindly overwritten with different bytes.

## Final Acceptance and Handoff

Run the maintained TinyCC package tests, locked workspace build/format/lint checks, and the full serial workspace suite inside the delegated cgroup service required by [AGENTS.md](../../AGENTS.md). The SDK/query load acceptance must use the real confined executor harness; ELF inspection or a trusted-process loader is not evidence of safe loading. Test repository restart/recovery in isolated directories and use deterministic concurrency barriers for install/resolve/remove races. Record supported target/profile limits and each command's actual outcome in `PROGRESS.md` as implementation begins. Documentation-only plan checks are local-link and consistency checks; they do not establish runtime acceptance.
