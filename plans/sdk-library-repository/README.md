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

SLR-1 through SLR-3 are implemented and committed. SLR-4 has a verified supervisor resolve/pin slice; confined executor staging acceptance remains. SLR-5 has a verified config and CLI slice; full CLI-to-gateway integration remains. SLR-6 has verified configured submit/run-sql gateway tests and an interactive BEGIN/COMMIT selection-lock test; interactive recovery/RUNAS coverage remains. See [progress and verification evidence](PROGRESS.md). SLR-7 remains. SLR-2, SLR-3, and SLR-4 were designed to proceed after SLR-1 in parallel, and their current implementations converge on the same release identity and query-only PREPARE contract for SLR-6. SLR-5 uses the typed API for remote list/select/install/remove, while its local config parser and filesystem behavior are implemented. SLR-7 is the final retirement gate and must not remove compatibility paths until the repository-only route has end-to-end evidence.

### Remaining Execution Sequence

Complete the remaining work in this order. Keep each task as a small red–green–refactor–specify increment and update `PROGRESS.md` with the first failing assertion, implementation boundary, exact commands and remaining gaps.

1. **Close SLR-5 control-command integration.** Drive `sdk-library list`, `select`, `install` and `remove` through the configured client and real gateway/control path. Cover ordinary-user listing, administrator-only mutation, current-authority revocation, operation-status retry after a lost reply, and persistence of selected version and unrelated config entries. Do not treat mocked CLI parsing as gateway acceptance.
2. **Complete SLR-6 one-shot commands.** Verify configured `submit` and `run-sql` end to end against the repository query-only profile. Assert `run-sql` builds with the selected local SDK but package and upload contain only `query.so`, manifest, scope and parameters. Assert mismatched release/profile/ABI and legacy SDK files fail before connecting; endpoint overrides must match config and must not rewrite it.
3. **Complete SLR-6 interactive behavior.** Exercise the config-driven interactive client through repository-profile negotiation, autocommit, explicit BEGIN/COMMIT/ROLLBACK, transaction recovery and RUNAS. Prove SDK selection changes fail while an explicit transaction holds the selection lock, then succeed after transaction completion; prove autocommit statements use the saved selection. Keep RUNAS scoped to identity.
4. **Finish SLR-4 confined acceptance.** In the delegated cgroup harness, run a query linked against a repository release and verify the supervisor stages and pins the exact library until worker cleanup. Cover launch failure, cancellation, timeout and normal exit; removal must remain blocked until cleanup and succeed afterward. Run hostile constructors only in this confined harness.
5. **Retire the compatibility path in SLR-7.** Once tasks 1–4 pass, remove client SDK upload fields/chunks and server cache/release-directory startup flags from the repository-enabled path, remove `prdb-packager --legacy-package`, and provide an explicit administrator import/reinstall migration path with actionable upgrade errors. Add regression checks that capability mismatch never downgrades to uploaded libraries or a cache.
6. **Run the final acceptance gate.** Run maintained TinyCC package tests, locked workspace build, format and Clippy checks, then the full serial workspace test suite inside the delegated `systemd --user` cgroup service. Record exact outcomes, target/profile limits and any unavailable acceptance evidence. Do not mark SLR-7 complete until a query-only package has traversed client, gateway, supervisor and confined executor and commits successfully against the installed SDK release.

Dependencies: task 1 depends on SLR-2 and SLR-5; tasks 2–3 depend on SLR-3 and SLR-5; task 4 depends on SLR-1 and SLR-4; task 5 depends on the successful evidence from tasks 1–4; task 6 is the final gate. Work on tasks 1–4 can proceed in parallel when code boundaries permit, but compatibility removal and final acceptance are sequential gates.

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
