# Catalog Lifecycle Progress

## CL-3 / DD-3 / DS-3: schema-drop ID reservation — verified slice

- DROP SCHEMA now publishes the empty family-17 reservation in the same catalog transaction that removes its schema and object rows. Commit validation requires that reservation for schema-object deletion and rejects its coexistence with any live family-2 object or principal, preventing ID reuse.
- The regression observed the missing reservation before implementation, then passed after verifying atomic presence and rejected recreation of the retired schema ID. The existing unauthorized-owner assertion also passes through the focused schema lifecycle test.
- Verification: the focused real-redb regression, workspace build, formatting check, warning-denied all-target Clippy, and complete serial delegated-cgroup workspace suite passed. Sandbox and process tests were enabled; zero tests were ignored.
- Remaining lifecycle scope: permanent reservations for retired object kinds other than schemas, users, and roles, and trustworthy history for pre-v2 schema drops.

## CL-3 / DD-3 / DS-3: table-drop ID reservation — verified slice

- DROP TABLE now includes the stable table ID's family-17 reservation in its authorized catalog batch. The table-drop guard validates the empty family-17 payload and serializes against a concurrent reservation; the generic catalog validator rejects table deletion without a reservation and blocks reuse.
- Red/green evidence: the updated real-redb lifecycle regression failed before implementation because the dropped table had no family-17 record, then passed with the same-ID recreation attempt rejected.
- Verification: workspace build, formatting check, warning-denied all-target Clippy, and complete serial delegated-cgroup workspace suite passed. Sandbox and process tests were enabled; zero tests were ignored.
- Remaining lifecycle scope: retirement for owned child definitions and other object kinds, plus historical pre-v2 table-drop identity history.

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
- A separate read-only validator checks the exact catalog-v1 READY source shape, including its 30 bootstrap mappings and lack of family-17 tombstones. `migration-inspect` can report a v1 active source while ordinary `StorageInstance::open` continues to reject it for service admission.
- When both images validate, the report also flags database-ID mismatch and compares commit sequences as same-frontier, active-ahead, or staging-ahead; this is diagnostic only and does not infer which image should be kept.
- Compatibility finding: v1 has no family-17 identity tombstones. Audit history is bounded and can be pruned, so it cannot prove the complete set of deleted user/role IDs. A v1-to-v2 conversion must not synthesize or omit permanent reservations without a trusted source; the transition remains fail-closed pending a safe contract.
- Verification: the pinned-shell formatting check, locked workspace build, and warning-denied all-target Clippy passed. The full serial workspace suite passed inside the delegated `systemd --user` unit, with sandbox integration enabled and zero ignored tests. `git diff --check` passed.

## CL-5 source validation: read-only catalog-v1 validation — implemented

- Added an offline-only source validator for catalog version 1. It uses the full READY integrity walk, expects the legacy 30 bootstrap mappings and families 1–16, and applies the historical rule that every verifier, including a revoked one, must reference a live principal. It opens the redb image read-only and never rebuilds derived indexes.
- `prdb-maintain migration-inspect` can now inspect a v1 active source while normal service startup continues to reject it. A regression converts an initialized fixture to the exact v1 metadata/descriptor shape, verifies the full source walk, checks current startup rejection, and confirms the CLI diagnostic.
- The historical verifier rule is preserved: even revoked v1 verifiers must reference a live principal. A regression inserts a well-formed revoked orphan verifier and verifies the source validator rejects it.
- This proves source integrity but does not resolve v1's missing tombstone history or perform the v1-to-v2 conversion.
- Verification: pinned-shell format check, locked workspace build, warning-denied all-target Clippy, and full serial workspace suite inside the delegated `systemd --user` service passed. Sandbox integration ran with zero ignored tests; `git diff --check` passed.

## CL-5 recovery command: discard an unpublished staged image — implemented slice

- `prdb-maintain migration-discard-stage --instance-dir PATH --backup-dir PATH` removes `.prdb-migration.next` only when the active image is byte-for-byte identical to a verified backup, its database ID/frontier matches the backup, and the staged READY image matches the same ID/frontier. Both images use read-only redb validation under the exclusive instance lock; the directory is synced after removal.
- A transformed sidecar retained after publication cannot pass the active-byte comparison, so this command cannot erase the old source generation. It does not select or restore generations.
- Regression covers successful removal while preserving active bytes, rejection of an unrelated verified backup without removing staging, and rejection while an instance owner holds the lock.
- Focused discard process tests passed. Pinned-shell format check, locked workspace build, warning-denied all-target Clippy, and the full serial workspace suite passed inside the delegated `systemd --user` unit with sandbox tests enabled and zero ignored tests. `git diff --check` passed.

## CL-5 prerequisite: verified catalog-v1 migration backup — implemented

- Added `prdb-maintain migration-backup --instance-dir PATH --output-dir PATH` to copy a v1 migration source while holding its exclusive lock, using read-only source validation. The existing backup manifest format applies unchanged; `backup-verify` now accepts both supported v1 migration sources and current v2 READY images.
- Regression converts a fixture to catalog-v1 metadata and descriptor form, creates and verifies its independent backup, and checks the source bytes remain unchanged.
- This makes a v1 source independently preservable; it does not provide a safe tombstone closure or a v1-to-v2 transform.
- Verification: pinned-shell format check, locked workspace build, warning-denied all-target Clippy, and full serial workspace suite inside the delegated `systemd --user` service passed. Sandbox integration ran with zero ignored tests; `git diff --check` passed.

## CL-4 supported logical version matrix — clarified

- The lifecycle specification now lists service admission, read-only source/backup support, and executable transition status for catalog/expression/built-in semantics tuples `1/1/1`, `2/1/1`, and unknown tuples.
- It records that catalog v1 is fully inspectable and backup-capable for offline migration, while v1-to-v2 conversion remains unavailable until prior deleted identity IDs can be preserved safely.
- Verification: documentation consistency and `git diff --check`; no code behavior changed.

## CL-5 recovery diagnostic: compare generations with the verified backup — implemented slice

- `prdb-maintain migration-inspect --instance-dir PATH --backup-dir PATH` now verifies the supplied backup and reports whether the exact active and staged database bytes match its preserved source image. An absent stage reports `staging_matches=false`.
- This identifies which named file still holds the backup source after an interrupted generation exchange. It remains diagnostic only; matching bytes does not select a recovery action or prove the exchange was durably acknowledged.
- Regression verifies the active and staged images both match the source backup, checks both database files remain byte-identical, and retains the lock-exclusion and unpublished-stage discard coverage.
- Verification: focused maintenance CLI test passed. Pinned-shell format check, locked workspace build, warning-denied all-target Clippy, and full serial workspace suite inside the delegated `systemd --user` service passed. Sandbox tests ran with zero ignored; `git diff --check` passed.

## CL-4: reject both legacy and future catalog versions before index repair — verified

- Regression: the startup fixture now mutates a READY v2 image to catalog versions 1 and 3, removes a disposable derived index, and verifies `StorageInstance::open` returns `UnsupportedFormat` without recreating that index in either case.
- Verification: focused `bootstrap_binary` regression, `cargo fmt --all -- --check`, locked workspace build, warning-denied all-target Clippy, and the full serial delegated-cgroup workspace suite passed with zero ignored tests. `git diff --check` passed.
- Scope: covers the current binary's compatibility gate for one known legacy and one unknown future catalog version; it does not substitute for testing an archived older binary against a directory produced by a future release.

## CL-4 historical catalog-v1 runtime probe — partial evidence

- Built the engine from historical commit `295caee7^`, whose `EngineMetadata::supports_current_formats` accepts catalog v1, in a temporary source tree. Its real `StorageManager::new` path rejected a freshly initialized catalog-v2 instance with `Startup(UnsupportedFormat)`.
- A second probe performed only `redb::Database::open` and close on a byte-identical copy; that alone changed the database-file digest. A row-by-row digest over all 20 logical engine tables matched a control copy exactly before and after the historical runtime's rejected `StorageManager::new` attempt. The runtime returned `UnsupportedFormat`; the physical change is in redb-owned file state, not the logical table contents covered by the probe. No archived binary or database fixture was added to the repository.
- Remaining: add repeatable archived runtime/data fixtures and retain the distinction between redb-owned open/recovery metadata changes and engine-authored logical mutations. Do not describe the rejection as byte-preserving.

## CL-4 / CL-5 source fixture: populated v2 and v1 images — verified slice

- Fixture: the bootstrap binary integration test now uses real storage DDL to add a user schema, table, and stored default expression, then persists a schema-encoded row. It validates that populated catalog as current v2 before converting only the engine metadata and database descriptor to the exact v1 shape.
- Compatibility evidence: the full current validator accepts the v2 image and sees the row; the offline v1 migration-source validator accepts the converted image and sees the same row and expanded catalog. Normal `StorageInstance::open` still rejects v1, and the migration backup remains byte-preserving.
- Verification: the focused test passed, then all 21 `bootstrap_binary` integration tests passed. Formatting, workspace build, warning-denied Clippy, maintained TinyCC tests, sandbox fixture, release-profile validation (10 engine binaries and one query fixture), and the full serial delegated-cgroup workspace suite passed; the cgroup probe reported writable `cpu memory pids`, and the suite had zero ignored tests. `git diff --check` passed.
- Remaining: this generated fixture is not an archived prior-release binary/data set and does not provide the missing v1 identity-deletion history. Older-binary rejection, explicit redb compatibility fixtures, a safe v1-to-v2 transition contract, and populated transition semantics remain open.

## CL-5 v1 identity-history input — specified and codec implemented

- Contract: [catalog lifecycle](../../specs/catalog-lifecycle.md) now defines `PRDB-IDENTITY-HISTORY-1`, an operator-certified complete list of v1 deleted user/role IDs bound to the source database ID and commit sequence. The SHA-256 detects accidental corruption; completeness remains an explicit operator trust input and is never inferred from prunable AUD1 history.
- Implementation: [`identity_history`](../../engine/src/identity_history.rs) encodes/decodes the canonical sorted nonzero ID set, enforces count/byte bounds and checksums, and reads only an owner-owned mode-0600 single-link regular file without following symlinks. The read path binds the file to the expected database ID and commit sequence.
- Regression: four unit tests cover canonical round-trip, noncanonical/zero/duplicate/truncated inputs, digest corruption, file privacy, symlink rejection, and mismatched source identity.
- Verification: the initial test compile failed because the codec was absent; after implementation, all four focused tests passed. Workspace formatting, build, warning-denied Clippy, and `git diff --check` passed.
- Remaining: the acknowledgment flag, verified-backup binding at the CLI, conversion of logical tables and tombstones into a bounded v2 staging image, staged-target validation/publication, and interruption recovery are not implemented. Catalog v1 remains rejected for service admission and no migration is yet executable.

## CL-5 identity-history collision guard — implemented slice

- Added `IdentityHistory::validate_tombstones_unoccupied`, which validates the canonical history before checking candidate IDs against a caller-supplied scan of source family 2, 10, and 17 records. It rejects any historical deletion ID that is still occupied by an object, principal, or tombstone.
- Regression covers a colliding source ID and a collision-free set. The migration executor must still supply the validated source-family scan and bind it to a verified backup before conversion.
- Verification: all five focused identity-history tests passed; formatting, the locked workspace build, warning-denied all-target Clippy, and the full serial delegated-cgroup workspace suite passed. Sandbox/process tests ran enabled with zero ignored tests. The delegated probe reported a writable cgroup v2 parent with `cpu memory pids` available and enabled. `git diff --check` passed.

## CL-5 v1 record transformer — implemented slice

- Added a one-transaction v1-to-v2 staging transformer. It requires a fully validated v1 READY target and matching history database ID/frontier, rejects history IDs already present in family 2, 10, or 17, updates the engine metadata and database descriptor (including built-in mapping 217), and inserts every family-17 tombstone atomically.
- The operator history bounds the write set to 65,536 tombstones; a failed validation or write leaves the staging image at v1. The caller remains responsible for proving staging is a byte-for-byte source copy backed by the verified source backup, then running full v2 READY validation and atomic publication.
- Regression converts a populated catalog-v1 copy with a tombstone, validates retained row/frontier semantics and code 217, then verifies an occupied tombstone ID fails without changing the staging catalog version.
- Verification: the focused populated migration-source regression passed; formatting, locked workspace build, warning-denied all-target Clippy, and the full serial delegated-cgroup workspace suite passed. Sandbox and process tests were enabled with zero ignored tests. `git diff --check` passed.

## CL-5: populated migration preserves multiple certified identity tombstones — verified slice

- Expanded `v1_source_validator_checks_legacy_ready_images_without_admitting_them` to migrate three source-bound retired identity IDs in one populated v1-to-v2 transformation. The upgraded READY image is checked for each family-17 record and for an exact increase of three catalog records, alongside the existing row, database ID, commit frontier, and built-in mapping checks.
- Verification: focused migration-source regression, locked workspace build, format check, warning-denied all-target Clippy, and the complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests.
- Remaining: archived prior-release binary/data fixtures, real power-loss/device-cache evidence, and broader filesystem fault campaigns remain open.

## CL-5 offline v1-to-v2 apply command — implemented slice

- Added `prdb-maintain migration-apply`, requiring the exclusive active-source lock, a manifest-verified backup matching the active source bytes/ID/frontier, a private source-bound identity-history file, and `--confirm-complete-identity-history`.
- The command creates or resumes private staging, repairs an invalid stage only while the active source still matches the verified backup, runs the one-transaction transformer, validates the exact history set and full v2 READY image, and compares all other logical tables and source catalog records before publication. It then syncs closed images, atomically exchanges filenames, syncs the directory, and revalidates both the published target and retained v1 source.
- The version matrix and upgrade contract now describe the executable transition. Integration coverage proves acknowledgment enforcement, rejection and repair of a valid but source-modified v2 stage, resumption of an exact converted target, populated v1 CLI migration with row/audit preservation, successful v2 service open, post-publication inspection identifying the active v2 and backup-matching v1 source, retained byte-identical v1 source, and refusal to discard that source after publication.
- Verification: the focused apply-command regression passed. Formatting, locked workspace build, warning-denied all-target Clippy, and the full serial delegated-cgroup workspace suite passed; all sandbox and process tests were enabled with zero ignored tests. The cgroup probe reported writable `cpu memory pids` controllers, `git diff --check` passed.

## CL-6 explicit backup restore — implemented slice

- Added `prdb-maintain restore --instance-dir PATH --backup-dir PATH --confirm-data-loss`. It requires a verified current v2 backup and the explicit data-loss acknowledgment, validates a staging copy, and uses atomic exchange so the displaced active file remains under the private staging name.
- Restore can recover an unreadable active redb image without opening it. It copies through `.prdb-restore.copying`, verifies before renaming to staging, discards and rebuilds an interrupted partial copy, resumes only an exact backup copy already in staging, and reports `already-active` on an idempotent retry after publication. Startup tolerates the private copy name while continuing to use `storage.redb`.
- Regression commits a later schema generation through `StorageManager` after backup, proves its commit sequence advanced, corrupts that newer active image, verifies missing acknowledgment leaves it untouched, restores the exact backup bytes, checks the displaced newer image is retained, and verifies an idempotent retry.
- A separate collision regression confirms an unrelated private staging image causes restore to fail without changing either the active image or the staging bytes.
- Startup regression confirms a private partial `.prdb-restore.copying` file is tolerated while `storage.redb` remains active, but rejects that file when permissions are broadened and continues to reject unknown files.
- Verification: the focused restore regression against the newer active generation passed. The preceding implementation revision passed formatting, locked workspace build, warning-denied all-target Clippy, and the full serial delegated-cgroup workspace suite; all sandbox and process tests were enabled with zero ignored tests. The cgroup probe reported writable `cpu memory pids` controllers, and `git diff --check` passed.

## CL-4 populated redb physical-format compatibility — verified slice

- Added archived redb-2.6.4 physical v2 and upgraded v3 images containing a READY PRDB catalog, a user schema/table, a stored default expression, and one application row. The generator starts from a current engine image and copies all 20 byte-oriented logical tables into redb 2.6.4.
- The pinned opener rejects the populated v2 image with `UpgradeRequired(2)` and no byte changes. After the upstream 2.6.4 `Database::upgrade()` and redb 4.4 recovery, current READY validation accepts the full image and the application row remains intact.
- Verification: generator create/verify reported 20 tables and one row; the focused rejection and upgraded-image regressions passed. The separate redb operator workflow, including backup binding and interruption-safe publication, remains open.

## CL-4 offline redb physical-v2 upgrade — implemented

- Red evidence: `maintenance_redb_v2_upgrade_backs_up_and_atomically_publishes_ready_instance` initially failed because the maintenance CLI rejected `redb-format-upgrade`.
- The new `prdb-maintain redb-format-upgrade` requires explicit physical-v2 acknowledgment and the exclusive instance lock. It creates or verifies an independent mode-0600 source backup, rebuilds private same-directory staging from the verified backup, upgrades with pinned redb 2.6.4, compares digests over all 20 engine tables, validates READY with redb 4.4, then atomically exchanges active and staging names while retaining the v2 source. Retry verifies backup/source identity and the active logical digest.
- Regression covers missing acknowledgment, in-instance backup refusal, interrupted staging rebuild, backup byte identity and permissions, retained source bytes, populated READY data, idempotent retry, and refusal under a live instance lock.
- Verification: final-source formatting, locked offline workspace build, warning-denied all-target Clippy, the focused serial `bootstrap_binary` suite (31 passed), TinyCC SDK `make test`, release-profile generation/validation, and the full serial workspace suite inside the delegated `systemd --user` service passed. The cgroup probe reported writable `cpu memory pids`; sandbox and process tests were enabled and the full suite completed with zero ignored tests. `git diff --check` passed.
- Limits: process-crash and ordinary filesystem tests do not establish power-loss durability; add filesystem-level fault injection before making that claim.

## CL-4 physical upgrade process-exit recovery — verified

- Red evidence: `maintenance_redb_physical_upgrade_recovers_after_process_exit_at_exchange` failed because the maintenance process returned success instead of stopping at the selected publication boundary.
- A debug-build-only failpoint exits immediately after `RENAME_EXCHANGE` and before directory sync. The process regression verifies that the new active image opens READY, the old v2 bytes remain in the sidecar, and a fresh maintenance invocation validates both generations and returns `already-current`.
- Verification: the failpoint regression passed after observing the initial failure; formatting and `git diff --check` passed. This is process-exit/restart evidence with the same live kernel and filesystem, not a power-loss test.

## CL-4 physical upgrade pre-publication process exit — verified

- Red evidence: `maintenance_redb_physical_upgrade_rebuilds_valid_stage_after_process_exit` failed because the command returned success instead of exiting before filename exchange.
- A second debug-build-only failpoint exits after the staging image has passed logical digest and READY validation but immediately before exchange. The regression proves the active v2 bytes and independent backup remain identical, the completed staged v3 image is valid, and a fresh retry rebuilds staging and publishes successfully while retaining the v2 source.
- Verification: all 33 serial `bootstrap_binary` tests passed, including both sides of the exchange process-exit cases. Formatting, locked offline workspace build, warning-denied all-target Clippy, and `git diff --check` passed.
- Limit: these abrupt process exits run with the same live kernel and filesystem and do not simulate power loss.

## CL-4 physical upgrade recovers after backup-directory sync failure — verified slice

- Added a debug-only failure point after publishing the independent backup but before syncing its parent directory. `maintenance_redb_physical_upgrade_recovers_after_backup_directory_sync_failure` confirms the active redb-v2 image remains byte-identical, the complete backup is reusable, and staging has not begun; a retry syncs the existing backup, completes conversion, retains the source generation, and opens the target as READY.
- Verification: the focused `bootstrap_binary` regression passed. Workspace build, format check, warning-denied all-target Clippy, and `git diff --check` passed. The complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests, including all 35 bootstrap binary tests.
- Limit: the injected syscall-boundary failure and successful retry do not simulate power loss or validate device cache behavior; power-loss evidence remains open.

## 2026-11 delegated acceptance checkpoint

- The complete serial delegated-cgroup workspace suite passed with sandbox and process tests enabled and zero ignored tests. All 33 `bootstrap_binary` tests passed, covering backup validation, v1 migration, physical redb upgrade, restore, publication retry, and process-exit recovery cases.
- Power-loss/filesystem fault evidence and archived prior-release binary/data fixtures remain open as already recorded above.

## CL-2 bootstrap seed commit-sync failure — verified slice

- Added `bootstrap_seed_sync_failure_is_reopenable_and_retryable`, using a redb `StorageBackend` wrapper that injects one `sync_data` error during the immediate-durability initial catalog transaction. The test confirms seeding reports a storage error, closes the failed database, reopens the real file, retries seeding only when the recovered image is empty, and validates the resulting INITIALIZING catalog and administrator identity.
- Verification: focused regression passed; Nix-shell formatting check, locked workspace build, and warning-denied all-target Clippy passed. The cgroup probe reported a writable delegated parent with `cpu memory pids` available and enabled. The full serial workspace suite passed inside the delegated `systemd --user` service with sandbox and process tests enabled and zero ignored tests.
- Limit: this injects a backend sync error with a live kernel and filesystem. It verifies reopen/retry behavior for the observable commit failure, not power-loss recovery or storage-device cache behavior; the broader transaction-boundary and power-loss evidence remains open.

## CL-2 resume token-rotation commit-sync failure — verified slice

- Extracted the resume token replacement into `rotate_initializing_credential`, the same atomic immediate-durability transaction used by `resume`. `bootstrap_token_rotation_sync_failure_reopens_and_retries_safely` injects a redb `sync_data` failure during that transaction, drops the database handle, reopens and validates the INITIALIZING image, then retries rotation from whichever credential is active. The retry publishes one valid replacement and the catalog passes startup validation.
- Verification: the focused regression and all 16 `bootstrap` integration tests passed. Nix-shell formatting check, locked workspace build, warning-denied all-target Clippy, and `git diff --check` passed. The delegated cgroup probe confirmed writable `cpu memory pids`; the complete serial workspace suite passed inside the delegated `systemd --user` service with sandbox and process tests enabled and zero ignored tests.
- Limit: backend sync-error injection verifies retry behavior with a live kernel and filesystem. It does not simulate power loss or device-cache behavior; those durability limits remain open.

## CL-6: reject migration after post-upgrade writes — verified slice

- Extended `migration_apply_requires_complete_history_ack_and_publishes_validated_v2`: after v1-to-v2 publication, the test commits a real schema through `StorageManager`, then retries the old migration command against the original v1 backup/history. The retry fails; the active catalog remains v2 at the newer commit sequence and the committed schema remains resolvable.
- Verification: the focused `bootstrap_binary` migration test and the complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests. This guards the post-upgrade no-rollback boundary; broader migration semantics preservation and power-loss durability remain open.

## CL-6: migrated bootstrap credential retains verifier semantics — verified slice

- Extended `migration_apply_requires_complete_history_ack_and_publishes_validated_v2` to preserve the initial token in zeroizing test memory and authenticate a new query session after v1-to-v2 publication. The authenticated session resolves to the same administrator ID, exercising the migrated verifier through the production storage authentication path.
- Verification: the focused migration CLI regression passed. Locked workspace build, format check, warning-denied all-target Clippy, and `git diff --check` passed. The complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests.

## CL-6: migration preserves grants and row/cell policy behavior — verified slice

- Expanded `migration_apply_requires_complete_history_ack_and_publishes_validated_v2` with a second login user, token, schema USAGE, table SELECT, column SELECT, an ALLOW row predicate, a RESTRICT cell policy, and two stored rows. Before conversion and after conversion, the authenticated user's authorized read hides the nonmatching row and returns REDACTED for the matching row's cell. This exercises token, grant, row-policy, and cell-policy behavior against the same catalog IDs on both sides of conversion.
- Verification: the focused migration CLI regression passed. Locked workspace build, format check, warning-denied all-target Clippy, and `git diff --check` passed. The complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests.

## 2026-11-16 integrated acceptance refresh

- The complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests. All 33 bootstrap binary tests passed, including backup verification, migration, physical redb upgrade, restore, and publication retry/process-exit recovery.
- Abrupt process-exit coverage does not simulate power loss; filesystem fault evidence and archived prior-release fixtures remain open.

## 2026-11-16 delegated integration refresh

- The complete serial delegated-cgroup workspace suite passed with sandbox and process tests enabled and zero ignored tests. All 33 `bootstrap_binary` tests passed alongside 179 real-redb commit integration tests, including schema create/drop process recovery and atomic catalog publication checks.
- Power-loss/filesystem fault evidence and archived prior-release binary/data fixtures remain open.

## CL-4 / CL-5: recover from post-exchange directory-sync failure — verified slice

- Added a debug-build-only injected failure immediately after atomic generation exchange and before the containing-directory sync. `maintenance_redb_physical_upgrade_recovers_after_directory_sync_failure` verifies the command reports the injected error, the former physical-v2 image remains byte-identical in the retained sidecar, the active READY image passes read-only startup validation, and a fresh maintenance invocation safely reports `already-current`.
- Verification: the focused `bootstrap_binary` regression passed; workspace formatting, warning-denied all-target Clippy, and `git diff --check` passed. The complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests, including all 34 bootstrap binary tests and all 179 commit tests.
- Limit: injected syscall-boundary failure and process-exit tests do not simulate power loss or validate storage-device cache behavior. Actual filesystem/power-loss fault evidence remains open.

## CL-4: rebuild a converted stage after pre-sync failure — verified slice

- Added a debug-only failure point after redb physical-v2 conversion completes but before the staging file is synced or validated. `maintenance_redb_physical_upgrade_rebuilds_stage_after_conversion_failure` verifies that the active v2 image and independent backup remain byte-identical, then retries through stage cleanup and publishes a READY upgraded image while retaining the v2 source.
- Verification: focused `bootstrap_binary` regression, locked workspace build, formatting, warning-denied all-target Clippy, and `git diff --check` passed. The complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests; it includes 36 bootstrap binary tests.
- Limit: the injected error verifies recovery ordering with a live kernel and filesystem; power-loss and device-cache durability evidence remain open.


## DS-SQL-51: normalized explicit constraint object names — verified

- The serialized CREATE TABLE validator now accepts canonical explicit PK, UNIQUE, and FK object names while validating the normalized identifier profile; generated labels continue to pass. Real-redb persistence and the full delegated workspace suite pass; details are recorded in [schema DDL progress](../ddl-schema/PROGRESS.md).


## DS-SQL-52: ALTER constraint name uniqueness — implemented

- The authorized constraint commit checks PK/UNIQUE/CHECK names after ALTER authority and checks FK names after source ALTER plus REFERENCES authorization. Real-redb, process metadata, and name-based drop coverage passes. The full serial delegated-cgroup workspace suite passes with zero ignored; see [schema DDL progress](../ddl-schema/PROGRESS.md). Cargo.lock is unchanged.

## 2026-11 delegated acceptance checkpoint

- The complete serial delegated-cgroup workspace suite passed with sandbox and process tests enabled and zero ignored tests. All 33 `bootstrap_binary` tests passed, covering backup validation, v1 migration, physical redb upgrade, restore, publication retry, and process-exit recovery cases.
- Power-loss/filesystem fault evidence and archived prior-release binary/data fixtures remain open as already recorded above.

## CL-3 / DD-7: orphaned principal blocks READY — verified slice

- A recovery regression removes a user's typed object record while retaining its family-10 principal record. Both persisted-catalog validation and `StorageManager` admission reject the image with `InvalidCatalog`; startup does not reconstruct identity authority from the surviving record.
- Verification: focused real-redb recovery regression passed. Workspace build, format check, warning-denied all-target Clippy, and the complete serial delegated-cgroup suite passed with sandbox/process tests enabled and zero ignored tests (293 unit tests, 186 commit tests, 76 transaction tests, plus all remaining targets).

## CL-3: missing bootstrap descriptor blocks READY — verified slice

- Added `startup_rejects_missing_bootstrap_descriptor_before_admission`. The regression removes the required family-1 database descriptor directly from a valid image; both persisted-record validation and `StorageManager::new` reject it as `InvalidCatalog`.
- Verification: focused real-redb startup regression passed. Workspace build, format check, warning-denied all-target Clippy, and the complete serial delegated-cgroup suite passed with sandbox/process tests enabled and zero ignored tests.

## Plan handoff status — reconciled with implemented upgrade paths

- Updated the plan's acceptance section to reflect the implemented CL-1 through CL-6 behavior documented above, including the supported atomic filename exchange and explicit source retention. Removed stale language that said the publication design still needed pre-implementation reconciliation.
- Preserved the actual open evidence: power-loss/device-cache durability, archived prior-release binaries/data directories, and broader filesystem fault campaigns. No claim of power-loss acceptance is made.
- Verification: checked the changed progress link and the statements against the CL-4/CL-5/CL-6 evidence entries; `git diff --check` passed.

## CL-6 restore recovers after post-exchange directory-sync failure — verified slice

- Added a debug-build-only injected failure after restore atomically exchanges the verified backup into `storage.redb` and before syncing the instance directory. The restore subprocess regression verifies the command reports the injected failure, the active file is the byte-identical verified backup, and the displaced active image remains under the private staging name. Startup validates the restored READY generation, and a retry safely reports `already-active` while retaining the displaced image.
- Verification: focused restore subprocess regression, locked workspace build, format check, warning-denied all-target Clippy, and `git diff --check` passed. The full serial delegated-cgroup workspace suite passed with sandbox and process tests enabled and zero ignored tests (293 unit, 190 commit, 76 transaction, 36 maintenance binary tests, plus all remaining targets).
- Limit: this injected syscall-boundary failure verifies recovery with a live kernel and filesystem; it does not simulate power loss or device-cache behavior. Existing filesystem and power-loss evidence remains open.

## CL-5 migration recovers across process exit at publication — verified slice

- Added debug-build-only subprocess exit points immediately before and after the atomic generation exchange. The pre-exchange regression confirms the v1 source stays byte-identical and the fully validated v2 staging image can be resumed and published. The post-exchange regression confirms a fresh `StorageInstance` opens the published v2 generation with the original database ID and commit frontier while the retained migration source remains byte-identical and valid as v1.
- Verification: all focused `bootstrap_binary` migration tests passed; formatting, locked workspace build, warning-denied all-target Clippy, and `git diff --check` passed. The complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests.
- Limit: abrupt process exit with the live kernel and filesystem does not simulate power loss or device-cache behavior. Filesystem fault campaigns and archived prior-release fixtures remain open.

## CL-3 / DD-3: read-only recovery validation checks typed table namespaces — verified slice

- Persisted-catalog validation now checks that an explicit family-2 table parent agrees with the family-5 typed schema ID. The shared validator is used by read-only backup/recovery validation as well as writable startup, so malformed namespace ownership cannot pass the offline READY check.
- Verification: focused real-file read-only and writable startup regressions passed; the complete serial delegated-cgroup workspace suite passed with sandbox/process tests enabled and zero ignored tests (294 library tests, 196 commit tests, 78 transaction tests, plus all remaining targets). Locked workspace build, format check, warning-denied all-target Clippy, and `git diff --check` passed. Cargo.lock is unchanged.

## 2026-11-16 full delegated-cgroup workspace verification

- The complete serial workspace test suite passed inside the delegated `systemd --user` service with sandbox and process integration tests enabled: 955 passed, 0 failed, 0 ignored. This includes the current permission, authorization/schema race, and metadata discovery regressions. The run used the locked release profile and documented TinyCC/runtime artifacts.

## CL-3 / DD-3: startup validation after repeated policy, constraint, and table reclamation — verified slice

- The 128-cycle real-redb catalog pressure regression now ends by validating the recovered catalog after repeatedly creating/dropping schemas, tables, indexes, row policies, and UNIQUE constraints with backing indexes. This checks that lifecycle cleanup leaves no orphan typed objects, expressions, targets, or dependency edges.
- Verification: focused real-redb pressure test passed (8.08 s); warning-denied all-target engine Clippy, formatting, and `git diff --check` passed.

## 2026-11-16 complete delegated workspace verification after token rotation

- Ran `cargo test --workspace --locked -- --test-threads=1` inside a delegated `systemd --user` unit with the documented TinyCC fixtures and runtime profile. The full suite exited successfully with sandbox and process integration tests enabled and zero ignored tests. The authenticated metadata/admin process test exercised `prdb-client token-rotate`, rejection of the old credential, authentication by the saved replacement, and subsequent revocation.

## 2026-11-16 complete delegated workspace verification after rotation race and CLI preflight

- Re-ran `cargo test --workspace --locked -- --test-threads=1` inside the delegated `systemd --user` unit with the documented TinyCC fixtures and runtime profile. The full workspace exited successfully with sandbox/process integration tests enabled and zero ignored tests. The suite includes the token-rotation/staged-write race and the authenticated process test for rejecting occupied credential-output paths before mutation.

## 2026-11-16 final delegated workspace verification for current tree

- Ran `cargo test --workspace --locked -- --test-threads=1` in the delegated `systemd --user` unit with the documented artifacts and runtime allowlist. The command exited successfully; sandbox and cross-process integration tests were enabled, including `token_rotation_racing_a_staged_write_linearizes` and the token-rotation CLI occupied-output preflight. Zero tests were ignored.


## 2026-11-16 full delegated workspace verification after identity rotation probes

- `cargo test --workspace --locked -- --test-threads=1` passed inside the delegated `systemd --user` service with the documented TinyCC fixtures and runtime profile. Sandbox and process integration tests were enabled; zero tests were ignored. The run includes storage and authenticated ADMIN checks that unauthorized rotations of existing and unknown token IDs return the same generic response.

## CL-3 / DD-3: foreign-key dependency cleanup under repeated DDL pressure — verified slice

- The 128-cycle real-redb lifecycle regression now creates and drops a foreign key for every temporary table, while a stable referenced UNIQUE constraint remains. It checks snapshot visibility of the FK and incoming dependency edge across FK removal, then validates the recovered catalog after all temporary schema/table/constraint records are removed.
- Verification: focused `schema_create_drop_pressure_preserves_pinned_name_index_snapshots` passed (12.47 s); format check, warning-denied all-target workspace Clippy, and `git diff --check` passed. The full workspace suite was not rerun for this pressure-test extension.


## 2026-11-16 full delegated workspace verification after identity issuance hardening

- `cargo test --workspace --locked -- --test-threads=1` passed inside the delegated `systemd --user` service with the documented TinyCC fixtures and runtime profile. Sandbox and process integration tests were enabled; zero tests were ignored. This run includes the unauthorized token issue/rotation enumeration regressions and the 128-cycle foreign-key catalog pressure regression.

## CL-2 / CL-3 catalog schema publication sync-failure recovery — verified slice

- Added a subprocess regression that injects a one-shot backend sync failure during `CREATE SCHEMA`, confirms the in-process storage manager stays quarantined, then reopens the database and checks that the schema/object records, DDL audit, operation outcome, and commit sequence are all in either the pre-publication state or the committed state. Startup validation must also accept the recovered catalog.
- Verification: focused real-redb subprocess regression passed. Workspace formatting, warning-denied all-target Clippy, and the complete serial delegated-cgroup workspace suite passed; sandbox and process integration tests were enabled and zero tests were ignored.
- Limit: injected sync failure with a live kernel and filesystem does not simulate power loss or device-cache behavior.

## 2026-11-16 delegated workspace verification after retained ADD COLUMN compaction check

- The complete serial `cargo test --workspace --locked -- --test-threads=1` suite passed inside the delegated `systemd --user` service. Sandbox and cross-process coverage were enabled; zero tests were ignored. Bootstrap, recovery, migration, and maintenance process regressions ran on the current tree. Power-loss and archived prior-release evidence remain open.

## CL-2 / ID-3: user deletion sync failure recovers as one catalog transition — verified slice

- The identity DDL subprocess regression now injects a redb `sync_data` failure while deleting a user. Reopen validation requires either the untouched identity/credential with no audit or outcome, or the committed deletion with tombstone, revoked credential, audit, durable outcome, and advanced frontier. The storage manager stays quarantined after the ambiguous error until reopen.
- Verification: focused regression, workspace build, formatting, warning-denied all-target Clippy, and the complete serial delegated-cgroup workspace suite passed with sandbox and process tests enabled and zero ignored tests. This does not simulate power loss or device-cache failure.
