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

## CL-4: independent compatibility gates — partial

- Engine metadata records and enforces the current catalog schema (2), expression IR (1), and built-in semantics (1) versions. Startup and transaction/commit paths reject unsupported values; commit tests verify unsupported versions cannot publish rows or outcomes.
- Family-16 verifier records carry algorithm and profile-version fields. The implemented SHA-256 v1 profile is used for newly issued tokens; startup and commit validation reject unsupported algorithms/versions and malformed verifier records.
- `metadata` codec tests cover unsupported engine-metadata encoding versions and noncanonical fields. Startup tests cover supported verifier records and malformed/disabled-user records.
- `rejects_unsupported_credential_algorithms_and_profile_versions` verifies startup rejects both an unknown verifier algorithm and an unknown profile version for an otherwise valid login identity.
- Verification: the focused startup regression passed; formatting, locked workspace build, and warning-denied Clippy passed. The full serial workspace suite passed inside the delegated `systemd --user` service with sandbox/process tests enabled and zero ignored tests.
- Remaining: populated old/new catalog and expression fixtures, older-binary rejection against a newer directory, explicit redb compatibility fixtures, and transition-specific semantics preservation. No migration path is implemented.

## CL-3 follow-up: reject unsupported metadata before cache repair — verified

- Startup now reads engine metadata and checks supported logical versions and lifecycle before rebuilding disposable catalog lookup indexes. Unsupported or not-ready images stop before those writes.
- `unsupported_catalog_version_is_rejected_before_derived_index_rebuild` removes a disposable index, changes the catalog version, verifies `StorageInstance::open` reports `UnsupportedFormat`, and verifies startup did not recreate the index.
- Verification: the focused regression passed; workspace formatting, locked build, and warning-denied Clippy passed. The full serial workspace suite passed inside the delegated `systemd --user` service with sandbox/process tests enabled and zero ignored tests.
- Contract boundary: redb must open and recover before the engine can read logical version metadata. The startup spec now distinguishes redb-owned physical recovery from engine-authored repairs and requires logical compatibility checks before the latter.

## CL-3 operator diagnostic: read-only READY inspection — implemented

- `prdb-maintain inspect --instance-dir PATH` uses the exclusive instance lock and read-only redb open, validates READY state without rebuilding derived indexes, then prints bounded counts and logical versions without credential material.
- Regression verifies the database file remains byte-identical and inspection is rejected while another instance owner holds the lock.
- Verification: the focused maintenance CLI regression passed. Workspace formatting, locked build, warning-denied Clippy, and the full serial workspace suite passed inside the delegated `systemd --user` service with sandbox/process tests enabled and zero ignored tests.

## CL-5 publication design: same-directory atomic exchange — decided

- redb owns physical file-format conversion and recovery. Logical migration writes a distinct private staging database through the pinned redb API and leaves active `storage.redb` untouched until the target passes full read-only READY validation.
- Publication uses Linux `renameat2(RENAME_EXCHANGE)` within the managed instance directory, followed by directory sync. Before exchange, `storage.redb` remains the source; after exchange, it names the target and `.prdb-migration.next` retains the old source. Unsupported exchange fails before publication; two sequential renames are prohibited.
- `StorageInstance::publish_staged_generation` implements this boundary: it requires a manifest-verified backup matching the active database ID and commit sequence, validates target READY state read-only, checks the target identity/frontier, closes handles, syncs both images, exchanges names, syncs the directory, and reopens the active generation. Tests verify inode exchange, retained source name, rejection of an unrelated backup, and no exchange on target database-ID mismatch.
- Migrations with multiple committed batches persist bounded progress in the target. Startup only opens `storage.redb`; offline maintenance owns resume/discard and ambiguous post-exchange recovery. Automatic rollback after target writes remains prohibited.
- Managed-directory validation now permits the exact `.prdb-migration.next` name only as an owner-owned mode-0600 regular file; startup ignores its contents and continues using `storage.redb`. A regression rejects broader permissions and any similarly named unexpected file.
- Verification: the focused publication regressions, workspace formatting, locked build, warning-denied Clippy, and full serial workspace suite passed inside the delegated `systemd --user` service with sandbox/process tests enabled and zero ignored tests.
- Remaining: implement source/target state validation, v1-to-v2 transformation, recovery commands, power-loss/filesystem evidence, and populated migration fixtures. This design record does not claim migration safety.

## CL-5 recovery diagnostic: inspect active and staged generations — implemented

- `prdb-maintain migration-inspect --instance-dir PATH` takes the exclusive instance lock, validates the active READY generation and any private `.prdb-migration.next` image through read-only redb handles, then reports both generations' logical versions and commit sequence. It does not mutate either image or decide whether to resume, discard, or publish.
- Regression verifies matching active/staged images are both reported, both redb files remain byte-identical, and inspection is rejected while another owner holds the instance lock. The help output includes the operation.
- The command validates only formats supported by this engine. It cannot yet inspect a v1 source after an interrupted v1-to-v2 conversion; migration-aware source validation remains open.
- When both images validate, the report also flags database-ID mismatch and compares commit sequences as same-frontier, active-ahead, or staging-ahead; this is diagnostic only and does not infer which image should be kept.
- Compatibility finding: v1 has no family-17 identity tombstones. Audit history is bounded and can be pruned, so it cannot prove the complete set of deleted user/role IDs. A v1-to-v2 conversion must not synthesize or omit permanent reservations without a trusted source; the transition remains fail-closed pending a safe contract.
- Verification: the pinned-shell formatting check, locked workspace build, and warning-denied all-target Clippy passed. The full serial workspace suite passed inside the delegated `systemd --user` unit, with sandbox integration enabled and zero ignored tests. `git diff --check` passed.
