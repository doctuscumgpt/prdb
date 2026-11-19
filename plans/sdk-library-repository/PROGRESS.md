# SDK Library Repository Progress

## Status

SLR-1 through SLR-7 are implemented. The server stores immutable SDK releases in the instance-local `sdk-libraries` repository, exposes authenticated list/install/remove operations, resolves the exact manifest release before admission, pins it through worker cleanup, and stages it beside the query in the confined executor directory. Clients persist endpoint, credentials, SDK selection, and local build tools in `PRDB-CLIENT-CONFIG-1`; packages and submissions contain the query artifact and metadata, not the SDK library.

The compatibility retirement is complete: client SDK upload fields and chunks, legacy SDK package layout, static SDK fallback, cache loader/eviction code, and deployment release/cache flags were removed. Existing SDKs must be installed explicitly through the admin API. The validated target is Linux x86-64 with repository-declared CPU baseline, runtime profile, and native ABI.

## Delivered Increments

- **SLR-1 — Repository storage:** strict metadata/ELF/digest/profile validation, quota enforcement, immutable synchronized release files, atomic no-replace publication, restart recovery, stable listing, and pin-aware removal.
- **SLR-2 — Admin API:** typed authenticated list/install/remove and operation-status commands; install streams are bounded, operation outcomes are durable, and mutations recheck current database-admin authority.
- **SLR-3 — Query-only protocol:** capability negotiation is mandatory; PREPARE carries no SDK size/digest, SDK chunks are rejected, and peers cannot downgrade silently.
- **SLR-4 — Resolve and stage:** START resolves the exact SDK release/ABI before storage admission. Executor preparation stages the pinned library under its SONAME and retains the repository pin until worker cleanup.
- **SLR-5 — Client config and repository commands:** strict private atomic config files include default instance, IP/port, credential profiles, SDK selection, and local compiler/linker paths; `instance` and `sdk-library` commands manage selections and releases.
- **SLR-6 — SQL client migration:** package submit, run-sql, and interactive SQL use configured settings and local SDK build inputs while transferring only the query package; active interactive transactions lock SDK selection.
- **SLR-7 — Retirement:** all legacy upload/cache/static fallback paths and server cache flags are removed. Docker/systemd guidance and migration instructions use repository installs.

## Verification

- `make -f engine/sdk/Makefile test` — passed.
- `make -f engine/sdk/Makefile sandbox-query-fixture` — passed; fixtures that depend on the SDK shared object declare its versioned SONAME and `$ORIGIN` runpath.
- `nix develop --no-update-lock-file -c cargo build --workspace --locked` — passed.
- `nix develop --no-update-lock-file -c cargo fmt --all -- --check` — passed.
- `nix develop --no-update-lock-file -c cargo clippy --workspace --all-targets --locked -- -D warnings` — passed.
- The full serial workspace suite passed inside the delegated `systemd --user` cgroup service with the sandbox fixture/runtime environment required by [SANDBOX_TEST.md](../sql-library/SANDBOX_TEST.md): `cargo test --workspace --locked -- --test-threads=1`.
- Confined acceptance includes a repository-installed SDK loading in the worker, a query-only DML package committing successfully, missing dependency failure in the executor, hostile constructor confinement, and pin/removal lifecycle coverage.
- `git diff --check` passed. Nix 2.34.9 rejects the repository's documented `nix develop --locked` spelling; checks used equivalent `--no-update-lock-file` mode.

## Handoff

The user-facing spec is [sdk-library-repository.md](../../specs/sdk-library-repository.md), with protocol and client details in [client-protocol.md](../../specs/client-protocol.md) and [client-sdk.md](../../specs/client-sdk.md). Install a compatible release into each database instance before submitting packages that declare it. The client keeps a local copy for compilation; the database repository supplies the runtime library.
