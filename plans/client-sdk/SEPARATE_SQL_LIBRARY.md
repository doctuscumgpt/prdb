# Separate SQL Shared Library and Supervisor Cache Plan

## Objective

Replace the statically linked SQL/runtime SDK archive in submitted query artifacts with a versioned shared library that the supervisor resolves from a verified local cache. Keep query compilation in the bounded client packager, keep native library loading in the confined executor, and preserve storage authorization and transaction boundaries.

Normative contracts are in [Client SDK](../../specs/client-sdk.md), [SQL library](../../specs/sql-library.md), [Toolchain](../../specs/toolchain.md), [Architecture](../../specs/architecture.md), and [Linux sandbox](../../specs/linux-sandbox.md). The implementation status and verification evidence for each increment are recorded in [PROGRESS.md](PROGRESS.md).

## Design Decisions

- Build one immutable SQL/runtime shared library per SDK release, native ABI, target, CPU baseline, and runtime profile. Give it a versioned SONAME.
- Install SDK release artifacts through the trusted deployment/package path, not through query uploads. Each registry entry records the library path, digest, SONAME, SDK release, ABI, target, CPU baseline, and runtime profile.
- Key cache entries by content digest plus compatibility identity. Verify bytes against the registered digest before first use and before recovery from any cache replacement or corruption. Use atomic publication and read-only permissions; never replace an entry in place.
- Validate package metadata and the query ELF as data. Require its actual `DT_NEEDED` set to match the manifest, and require the selected release's registered SQL SONAME. Resolve the SDK release and pin the entry at START before worker launch.
- Stage query.so and the selected cached SDK `.so` beside each other in the invocation's private artifact directory. The worker receives read/execute access to that directory only. Retain the cache entry until the invocation has exited and released staged resources.
- Permit only the exact `$ORIGIN` RUNPATH needed to find the staged library; reject all other RPATH/RUNPATH values and loader override environment variables. Resolve other shared dependencies only through the configured runtime profile.
- The supervisor never loads SDK or query code. Both are loaded by the clean executor after seccomp and the other sandbox restrictions are active. Constructors are untrusted execution. Each invocation has its own process and mutable state; only immutable file contents and kernel page-cache pages are shared.
- Reject static SDK packages by default. The only migration exception is explicit `--allow-legacy-static-sdk-release 1`; SDK release 2 is the cutoff and removes that option after release-1 packages have migrated. Never silently fall back between static and shared SDK modes.

## Incremental Implementation

| Increment | Observable failure first | Minimum implementation | Acceptance evidence |
| --- | --- | --- | --- |
| SQLSO-1: Build release | Fixture cannot link or load against a shared SDK library | Add a maintained target that builds `libprdb_sql.so.<release>` from the existing PIC SDK objects, sets SONAME, records release compatibility metadata and SHA-256, and links with the supported shared linker and `-z noexecstack`. Preserve the static archive only for transition tests. | Inspect ELF SONAME, ABI exports, `DT_NEEDED`, relocations, stack flags, target/profile metadata; TinyCC C tests link and run only the maintained SDK unit executables. |
| SQLSO-2: Package query | Current package path links the SDK archive statically or accepts an incompatible SONAME | Change `prdb-packager` to compile the query object with bounded TinyCC, then invoke a fixed shared-link command against the selected SDK release. Emit the exact `$ORIGIN` RUNPATH, derive PMF metadata from the final ELF, and publish query.so plus existing scope/digest sidecars atomically. | Package tests prove `DT_NEEDED` names the expected SONAME, SDK implementation symbols are undefined imports in query.so, wrong ABI/release/SONAME/RUNPATH and malformed ELF fail closed, and package creation never loads the artifact. |
| SQLSO-3: Registry and resolution | START has no trusted mapping from manifest SDK release to an installed library | Add a supervisor SDK registry for deployment-installed immutable releases. Validate registry metadata and library digests at startup or first use. At START, resolve profile/ABI/release/SONAME, pin the selected cache entry, and fail before storage admission or worker launch on any mismatch or cache miss. | Unit/process tests cover supported release resolution, absent and mismatched release, digest corruption, target/profile mismatch, concurrent lookup, atomic publication, and cleanup after failed admission. |
| SQLSO-4: Private staging and load | Executor's artifact directory contains only query.so | Stage the pinned cached SDK `.so` alongside query.so; update launch ownership and Landlock read/execute rules for that private directory. Keep the existing clean environment and load-after-seccomp ordering. Ensure the executor releases the cache pin after worker exit, including launch failure, cancellation, and timeout. | Confined integration test loads the query and separate SDK library, exercises a SQL call, and confirms durable outcome. Hostile SDK/query constructors, absent dependency, invalid RUNPATH, and load failure remain confined and abort safely. No trusted supervisor/storage process calls `dlopen` on either library. |
| SQLSO-5: Release and migration | Release packaging omits the shared SDK or leaves ambiguous static/shared compatibility | Update SDK release packaging, supervisor configuration, artifact compatibility rules, deployment docs, and fixture variables. Define the static-to-shared cutoff and remove the legacy acceptance path when supported packages have migrated. | Clean release-profile build/install/start test; manifest compatibility matrix; cache reuse and eviction tests; full serial workspace suite in the delegated cgroup with zero ignored tests; SDK C suite and fixture build. |

Run Cargo operations that write the same target directory sequentially. The cgroup-backed integration suite must run in the delegated service and must not be substituted by a fake cgroup or a trusted-process library load.

## Important Failure and Resource Rules

- Unsupported or unavailable SDK releases fail closed before invocation storage admission and before any user code runs.
- A library digest mismatch, malformed ELF, SONAME mismatch, ABI/profile mismatch, unexpected dependency, or disallowed loader path is a package/admission error.
- Cache storage has a configured byte limit and bounded concurrent loads. Eviction considers only entries with no active invocation pins; it removes an entry atomically and cannot change the bytes observed by an active worker.
- If cache I/O or staging fails after START setup begins but before storage admission, release the pin and staged directory and leave the invocation in the existing retryable prepared state where the state machine permits it. Do not consume START or create transaction state on failure.
- Once a worker is launched, query/SDK load errors follow ordinary invocation failure and durable abort rules. Never retry or replay arbitrary query code automatically.
- Cache identity is not authorization or proof that code is safe. All SQL calls still flow through the same scoped host callbacks and trusted storage validation.

## Handoff Record

For each increment, record the changed spec sections, observed failing case, implementation, actual commands and results, unsupported profiles, and the next smallest increment. Do not claim supervisor caching or shared-library confinement based only on ELF inspection or packaging tests; acceptance requires loading and SQL execution through the real confined executor path.
