# Catalog Lifecycle Progress

Migrated component-owned progress entries from [storage-manager progress](../storage-manager/PROGRESS.md); evidence is preserved below.

## CL-1 / CL-2 dependency: local bootstrap and credential handoff — verified core

- Red evidence: the focused bootstrap regression failed when the initial scaffold returned `NotImplemented` for initialization.
- `bootstrap::initialize` creates an owner-only directory, instance lock, setup marker, redb file, complete built-in catalog, reserved administrator role, first LOGIN user, membership, first token verifier, audit event, and `INITIALIZING` metadata in one immediate-durability transaction.
- `BootstrapSession::take_secret` transfers the random secret once and zeroizes its retained buffer. Acknowledgement requires prior transfer, validates the initializing image and active token, then durably publishes `READY`. `StorageInstance::open` holds the nonblocking exclusive lock and accepts only a validated READY image; its manager constructor reuses that validation.
- Resume rejects READY or corrupt images, recovers owner-only empty pre-marker/pre-commit directories, rotates an unclaimed token atomically, and refuses directories containing unknown files. Real-redb tests cover duplicate initialization, lock exclusion, permissions, verifier-only persistence, lost handoff, acknowledgement ordering, pre-marker recovery, and empty-file recovery.
- Verification: the prescribed Nix-shell workspace build, formatting check, Clippy with warnings denied, and all 227 workspace tests passed. `git diff --check` passed.
- Contract updates: [catalog lifecycle](../../specs/catalog-lifecycle.md), [storage manager](../../specs/storage-manager.md), and [credential lifecycle](../../specs/ddl-identity.md).
- Remaining: a protected setup socket/supervisor to deliver the secret and acknowledge delivery, power-loss fault injection at transaction boundaries, and normal service integration remain open. Direct `StorageManager` constructors are trusted low-level entry points and do not acquire the filesystem lock; service startup must use `StorageInstance::open`.

## CL-2 dependency: protected local bootstrap handoff

- Red evidence: both focused setup-socket regressions failed against the compiling `NotImplemented` listener scaffold.
- `BootstrapSession::bind_setup_listener` creates a mode-0600 Unix socket in the owner-only data directory. The one-shot listener verifies Linux peer credentials against the directory owner, sends token ID and secret in the bounded IPC v1 frame, requires the matching empty acknowledgement, removes the socket durably, then publishes READY. The client helper transfers the secret once and zeroizes retained buffers; disconnect, malformed ACK, or stream timeout leaves INITIALIZING for token rotation on resume. Resume and locked READY open remove a stale validated setup socket.
- Real-file tests cover successful delivery and acknowledgement, unacknowledged disconnect, mismatched request ID, stale-socket cleanup, and READY publication only after delivery.
- Contract updates: [catalog lifecycle](../../specs/catalog-lifecycle.md), [IPC protocol](../../specs/ipc-protocol.md), and [catalog lifecycle plan](../catalog-lifecycle/README.md).
- Verification: prescribed Nix-shell workspace build, formatting check, Clippy with warnings denied, and all 237 workspace integration tests passed.
- Remaining at this checkpoint: production service integration, OS-account deployment policy, and process/power-loss fault injection. Same-UID access to this local socket is not itself a database identity; possession of the issued token authenticates database use. The operator CLI is recorded in the later CL-2 entry below.

## CL-2 operator setup client — verified

- Added `prdb-init` to initialize a new instance or resume an interrupted bootstrap. It connects to the owner-only setup socket, validates and receives the one-time secret, atomically stores the canonical client credential file with mode 0600, acknowledges delivery, then waits for the bootstrap server to publish READY.
- The CLI rejects existing credential paths before changing the instance and never prints the token. `--resume` rotates the unacknowledged token; operators remove any credential left by an unconfirmed prior attempt before resuming.
- Verification: binary integration tests cover first initialization, READY reopen, interrupted setup/token rotation, private credential readability, and no-overwrite behavior. The locked workspace build, formatting, warning-denied Clippy, workspace tests, and systemd unit verification passed. The release profile now includes and validates `prdb-init`.
- Remaining: live service unit activation still depends on operator-supplied runtime allowlists/profile and real service account provisioning; backup/offline upgrade and recovery workflows remain open.

## CL-2 follow-up: resume after abrupt process exit at the initial commit boundary — verified

- Contract: a crash after durable creation of the INITIALIZING catalog but before secret handoff must leave the instance unavailable to ordinary startup; resume rotates the unclaimed token and only acknowledgement publishes READY. See [bootstrap interruption behavior](../../specs/catalog-lifecycle.md).
- Regression: `abrupt_process_exit_after_bootstrap_commit_recovers_before_ready` starts a child test process, initializes the database, syncs a private token-ID marker, then calls `_exit` without running Rust destructors. The parent resumes the same directory, verifies the original verifier is revoked, acknowledges the replacement credential, and reopens the validated READY image.
- Verification: the focused subprocess regression passed under the pinned Nix toolchain. This is abrupt process-exit evidence with the kernel and filesystem alive; it does not establish power-loss behavior.
- Remaining: transaction-boundary fault injection for actual durability failures, additional handoff interruption points, and offline upgrade/recovery workflows remain open.

## CL-2 follow-up: retry after abrupt exit at token-rotation commit — verified

- Contract: if resume commits a replacement verifier but the replacement secret is not delivered, another resume must revoke that unclaimed credential and issue a fresh token while keeping the database INITIALIZING.
- Regression: `abrupt_exit_after_token_rotation_revokes_unclaimed_credential_on_retry` starts from an INITIALIZING database, has a child process rotate the token and `_exit` before handoff, then resumes again. It verifies both the original and unclaimed replacement verifiers are revoked, the second replacement differs, and READY validation succeeds only after acknowledgement.
- Verification: the focused subprocess regression passed under the pinned Nix toolchain. Abrupt process exit leaves the kernel and filesystem alive; this is not power-loss evidence.
- Remaining: fault injection during redb commit/sync, further handoff interruption points, and offline upgrade/recovery workflows remain open.

## CL-5 prerequisite: checksummed offline READY backup — implemented

- Contract: a destructive offline migration must retain an independently verified source. The copy is created under an exclusive instance lock, preserves the marker and redb bytes, validates READY state, and cannot be mistaken for an authenticated/restore operation. See [backup and upgrade baseline](../../specs/catalog-lifecycle.md).
- Red evidence: the first integration attempt copied redb while its writer handle was still open; read-only redb open correctly returned `RepairAborted` because the clean-shutdown allocator state had not been persisted. The implementation now closes the recovered writer handle while retaining the filesystem lock before copying.
- Implementation: `prdb-maintain backup` writes a fresh private directory, syncs both files, opens the copy through read-only READY validation, then publishes a canonical `PRDB-BACKUP-1` sidecar manifest with file sizes and SHA-256 digests. `backup-verify` requires the lock and manifest, checks file digests, and validates the database without writing to it. Destinations and manifests are never overwritten.
- Regression: the binary test verifies restrictive modes, byte-for-byte source copy, repeated read-only verification without file changes, manifest corruption rejection, overwrite refusal, and rejection while the source lock is held.
- Verification: the focused maintenance CLI integration passed. `nix develop --no-update-lock-file -c cargo fmt --all -- --check`, `cargo build --workspace --locked`, and `cargo clippy --workspace --all-targets --locked -- -D warnings` passed. The full `cargo test --workspace --locked -- --test-threads=1` suite passed inside the delegated `systemd --user` service with all sandbox tests enabled and zero ignored tests. Local Markdown links and `git diff --check` passed.
- Limits: the manifest is an unsigned integrity check, backups duplicate identity/credentials and must not be run beside the source, and this feature does not restore or migrate databases. Power-loss durability and the migration generation switch remain open.
