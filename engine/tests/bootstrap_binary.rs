use prdb_engine::audit::{AuditAction, AuditEvent, AuditResult};
use prdb_engine::bootstrap::{BootstrapError, StorageInstance};
use prdb_engine::client_credentials::read_client_credentials;
use prdb_engine::metadata::EngineMetadata;
use prdb_engine::schema::{AUDIT, ENGINE_META, POLICIES};
use prdb_engine::startup::StartupError;
use redb::{Database, ReadableDatabase, ReadableTable, ReadableTableMetadata, TableHandle};
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

struct Fixture(std::path::PathBuf);

impl Fixture {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!(
            "prdb-init-cli-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ));
        std::fs::create_dir(&directory).expect("create fixture directory");
        Self(directory)
    }

    fn instance_dir(&self) -> std::path::PathBuf {
        self.0.join("instance")
    }

    fn credential_file(&self) -> std::path::PathBuf {
        self.0.join("admin.credentials")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn initialize_command(fixture: &Fixture, resume: bool) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_prdb-init"));
    command
        .arg("--instance-dir")
        .arg(fixture.instance_dir())
        .arg("--administrator")
        .arg("first_admin")
        .arg("--credential-file")
        .arg(fixture.credential_file())
        .arg("--expires-in-seconds")
        .arg("86400");
    if resume {
        command.arg("--resume");
    }
    command.output().expect("run prdb-init")
}

#[test]
fn init_help_lists_required_bootstrap_inputs_cleanly() {
    let output = Command::new(env!("CARGO_BIN_EXE_prdb-init"))
        .arg("--help")
        .output()
        .expect("run prdb-init help");
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).expect("UTF-8 help");
    for option in [
        "--instance-dir",
        "--administrator",
        "--credential-file",
        "--expires-in-seconds",
        "--resume",
    ] {
        assert!(help.contains(option), "missing {option} from help");
    }
    assert!(!help.contains('+'));
}

#[test]
fn maintenance_help_lists_offline_audit_archival() {
    let output = Command::new(env!("CARGO_BIN_EXE_prdb-maintain"))
        .arg("--help")
        .output()
        .expect("run maintenance help");
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).expect("UTF-8 help");
    assert!(help.contains("audit-archive --instance-dir PATH --output PATH"));
    assert!(help.contains("audit-verify --archive PATH"));
    assert!(help.contains("backup --instance-dir PATH --output-dir PATH"));
    assert!(help.contains("backup-verify --backup-dir PATH"));
}

#[test]
fn maintenance_backup_is_private_validated_and_checksum_verified() {
    let fixture = Fixture::new();
    let mut session =
        prdb_engine::bootstrap::initialize(fixture.instance_dir(), "backup_admin", i64::MAX)
            .expect("initialize backup source");
    let mut secret = session.take_secret().expect("bootstrap secret");
    use zeroize::Zeroize;
    secret.zeroize();
    drop(session.acknowledge().expect("publish source READY"));

    let backup_dir = fixture.0.join("backup");
    let create = Command::new(env!("CARGO_BIN_EXE_prdb-maintain"))
        .args(["backup", "--instance-dir"])
        .arg(fixture.instance_dir())
        .arg("--output-dir")
        .arg(&backup_dir)
        .output()
        .expect("create verified offline backup");
    assert!(
        create.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&create.stderr)
    );
    assert!(String::from_utf8_lossy(&create.stdout).contains("backup verified"));
    assert_eq!(
        std::fs::metadata(&backup_dir)
            .expect("backup directory")
            .permissions()
            .mode()
            & 0o7777,
        0o700
    );
    for name in [".prdb-bootstrap", "storage.redb"] {
        assert_eq!(
            std::fs::metadata(backup_dir.join(name))
                .expect("backup file")
                .permissions()
                .mode()
                & 0o7777,
            0o600
        );
        assert_eq!(
            std::fs::read(fixture.instance_dir().join(name)).unwrap(),
            std::fs::read(backup_dir.join(name)).unwrap()
        );
    }

    let manifest_path = fixture.0.join("backup.prdb-backup");
    let original_manifest = std::fs::read(&manifest_path).expect("backup manifest");
    assert_eq!(
        std::fs::metadata(&manifest_path)
            .expect("manifest metadata")
            .permissions()
            .mode()
            & 0o7777,
        0o600
    );
    let verified = Command::new(env!("CARGO_BIN_EXE_prdb-maintain"))
        .args(["backup-verify", "--backup-dir"])
        .arg(&backup_dir)
        .output()
        .expect("verify offline backup");
    assert!(
        verified.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&verified.stderr)
    );
    let stable_database = std::fs::read(backup_dir.join("storage.redb")).unwrap();
    let verified_again = Command::new(env!("CARGO_BIN_EXE_prdb-maintain"))
        .args(["backup-verify", "--backup-dir"])
        .arg(&backup_dir)
        .output()
        .expect("repeat offline backup verification");
    assert!(
        verified_again.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&verified_again.stderr)
    );
    assert_eq!(
        std::fs::read(backup_dir.join("storage.redb")).unwrap(),
        stable_database,
        "read-only verification must preserve backup bytes"
    );

    let mut corrupted_manifest = original_manifest.clone();
    let last = corrupted_manifest.len() - 2;
    corrupted_manifest[last] = if corrupted_manifest[last] == b'0' {
        b'1'
    } else {
        b'0'
    };
    std::fs::write(&manifest_path, &corrupted_manifest).unwrap();
    let rejected = Command::new(env!("CARGO_BIN_EXE_prdb-maintain"))
        .args(["backup-verify", "--backup-dir"])
        .arg(&backup_dir)
        .output()
        .expect("verify corrupted backup manifest");
    assert!(!rejected.status.success());
    std::fs::write(&manifest_path, original_manifest).unwrap();

    let repeated = Command::new(env!("CARGO_BIN_EXE_prdb-maintain"))
        .args(["backup", "--instance-dir"])
        .arg(fixture.instance_dir())
        .arg("--output-dir")
        .arg(&backup_dir)
        .output()
        .expect("refuse to overwrite an existing backup");
    assert!(!repeated.status.success());

    let live = StorageInstance::open(fixture.instance_dir()).expect("lock source instance");
    let locked = Command::new(env!("CARGO_BIN_EXE_prdb-maintain"))
        .args(["backup", "--instance-dir"])
        .arg(fixture.instance_dir())
        .arg("--output-dir")
        .arg(fixture.0.join("blocked-backup"))
        .output()
        .expect("reject backup while source is live");
    assert!(!locked.status.success());
    assert!(String::from_utf8_lossy(&locked.stderr).contains("InstanceLocked"));
    drop(live);
}

#[test]
fn unsupported_catalog_version_is_rejected_before_derived_index_rebuild() {
    let fixture = Fixture::new();
    let mut session =
        prdb_engine::bootstrap::initialize(fixture.instance_dir(), "version_admin", i64::MAX)
            .expect("initialize version fixture");
    let mut secret = session.take_secret().expect("bootstrap secret");
    use zeroize::Zeroize;
    secret.zeroize();
    drop(session.acknowledge().expect("publish READY"));

    let database_path = fixture.instance_dir().join("storage.redb");
    let database = Database::open(&database_path).expect("open version fixture");
    let mut metadata = {
        let read = database.begin_read().expect("read engine metadata");
        let table = read.open_table(ENGINE_META).expect("open metadata table");
        EngineMetadata::decode(
            table
                .get(&b"format"[..])
                .expect("read metadata")
                .expect("metadata exists")
                .value(),
        )
        .expect("decode metadata")
    };
    metadata.catalog_version = 1;
    let encoded = metadata.encode().expect("encode unsupported metadata");
    {
        let write = database.begin_write().expect("begin metadata update");
        write
            .open_table(ENGINE_META)
            .expect("open metadata table")
            .insert(&b"format"[..], &encoded[..])
            .expect("write unsupported version");
        write.delete_table(POLICIES).expect("remove derived index");
        write.commit().expect("commit unsupported metadata");
    }
    drop(database);

    let open_error = match StorageInstance::open(fixture.instance_dir()) {
        Ok(_) => panic!("unsupported catalog version must be rejected"),
        Err(error) => error,
    };
    assert!(
        matches!(
            &open_error,
            BootstrapError::Startup(StartupError::UnsupportedFormat)
        ),
        "unexpected startup error: {open_error:?}"
    );

    let database = Database::open(&database_path).expect("reopen unsupported image");
    let read = database.begin_read().expect("read unsupported image");
    assert!(
        !read
            .list_tables()
            .expect("list tables")
            .any(|table| table.name() == POLICIES.name()),
        "unsupported databases must not have disposable indexes rebuilt"
    );
}

#[test]
fn maintenance_verifies_pra1_archives_and_rejects_corruption()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new();
    let event = AuditEvent {
        event_id: [0x41; 16],
        actor_id: Some([0x42; 16]),
        correlation_id: [0x43; 16],
        occurred_at_us: 1,
        action: AuditAction::Ddl,
        result: AuditResult::Succeeded,
        target_ids: vec![[0x44; 16]],
    };
    let archive = prdb_engine::audit_archive::encode(&[(event.event_id, event.encode()?)])?;
    let archive_path = fixture.0.join("audit.archive");
    std::fs::write(&archive_path, &archive)?;
    std::fs::set_permissions(&archive_path, std::fs::Permissions::from_mode(0o600))?;

    let verified = Command::new(env!("CARGO_BIN_EXE_prdb-maintain"))
        .args(["audit-verify", "--archive"])
        .arg(&archive_path)
        .output()?;
    assert!(
        verified.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&verified.stderr)
    );
    assert!(String::from_utf8_lossy(&verified.stdout).contains("events=1"));

    let corrupted_path = fixture.0.join("corrupted.archive");
    let mut corrupted = archive;
    let last = corrupted.len() - 1;
    corrupted[last] ^= 1;
    std::fs::write(&corrupted_path, &corrupted)?;
    std::fs::set_permissions(&corrupted_path, std::fs::Permissions::from_mode(0o600))?;
    let rejected = Command::new(env!("CARGO_BIN_EXE_prdb-maintain"))
        .args(["audit-verify", "--archive"])
        .arg(&corrupted_path)
        .output()?;
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("InvalidDigest"));
    assert_eq!(std::fs::read(corrupted_path)?, corrupted);
    Ok(())
}

#[test]
fn init_delivers_and_persists_one_time_credential_before_ready() {
    let fixture = Fixture::new();
    let output = initialize_command(&fixture, false);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("database is READY"));
    assert!(!String::from_utf8_lossy(&output.stdout).contains("token_secret"));
    let metadata = std::fs::metadata(fixture.credential_file()).expect("credential metadata");
    assert_eq!(metadata.permissions().mode() & 0o7777, 0o600);
    let credentials = read_client_credentials(fixture.credential_file())
        .expect("read saved bootstrap credential");
    assert_ne!(credentials.token_id, [0; 16]);
    drop(credentials);
    drop(StorageInstance::open(fixture.instance_dir()).expect("open READY instance"));
}

#[test]
fn init_resumes_interrupted_bootstrap_and_rotates_the_credential() {
    let fixture = Fixture::new();
    let session =
        prdb_engine::bootstrap::initialize(fixture.instance_dir(), "first_admin", i64::MAX)
            .expect("create interrupted bootstrap");
    let original_token = session.token_id();
    drop(session);

    let output = initialize_command(&fixture, true);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let credentials = read_client_credentials(fixture.credential_file())
        .expect("read replacement bootstrap credential");
    assert_ne!(credentials.token_id, original_token);
    drop(credentials);
    drop(StorageInstance::open(fixture.instance_dir()).expect("open resumed READY instance"));
}

#[test]
fn maintenance_compaction_requires_exclusive_instance_lock() {
    let fixture = Fixture::new();
    let mut session =
        prdb_engine::bootstrap::initialize(fixture.instance_dir(), "maint_admin", i64::MAX)
            .expect("initialize maintenance fixture");
    let mut secret = session.take_secret().expect("bootstrap secret");
    use zeroize::Zeroize;
    secret.zeroize();
    let ready = session.acknowledge().expect("publish READY instance");

    let blocked = Command::new(env!("CARGO_BIN_EXE_prdb-maintain"))
        .args(["compact", "--instance-dir"])
        .arg(fixture.instance_dir())
        .output()
        .expect("run maintenance while instance is held");
    assert!(!blocked.status.success());
    assert!(String::from_utf8_lossy(&blocked.stderr).contains("InstanceLocked"));
    drop(ready);

    let compacted = Command::new(env!("CARGO_BIN_EXE_prdb-maintain"))
        .args(["compact", "--instance-dir"])
        .arg(fixture.instance_dir())
        .output()
        .expect("run offline maintenance");
    assert!(
        compacted.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&compacted.stderr)
    );
    assert!(String::from_utf8_lossy(&compacted.stdout).contains("compaction performed="));
    drop(StorageInstance::open(fixture.instance_dir()).expect("reopen READY instance"));
}

#[test]
fn audit_archive_is_durable_before_events_are_removed_and_refuses_overwrite()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new();
    let mut session =
        prdb_engine::bootstrap::initialize(fixture.instance_dir(), "archive_admin", i64::MAX)?;
    let mut secret = session.take_secret().expect("bootstrap secret");
    use zeroize::Zeroize;
    secret.zeroize();
    drop(session.acknowledge()?);

    let instance = StorageInstance::open(fixture.instance_dir())?;
    let write = instance.database().begin_write()?;
    {
        let mut audit = write.open_table(AUDIT)?;
        for byte in [0x21u8, 0x42] {
            let event = AuditEvent {
                event_id: [byte; 16],
                actor_id: Some([0x10; 16]),
                correlation_id: [byte.wrapping_add(1); 16],
                occurred_at_us: i64::from(byte),
                action: AuditAction::Ddl,
                result: AuditResult::Succeeded,
                target_ids: vec![[0x30; 16]],
            };
            audit.insert(event.event_id.as_slice(), event.encode()?.as_slice())?;
        }
    }
    write.commit()?;
    let sink_path = fixture.0.join("audit.jsonl");
    assert_eq!(
        prdb_engine::audit_sink::deliver_jsonl_batch(instance.database(), &sink_path, 8)?,
        3
    );
    drop(instance);

    let archive_path = fixture.0.join("audit.archive");
    let archived = Command::new(env!("CARGO_BIN_EXE_prdb-maintain"))
        .args(["audit-archive", "--instance-dir"])
        .arg(fixture.instance_dir())
        .arg("--output")
        .arg(&archive_path)
        .output()?;
    assert!(
        archived.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&archived.stderr)
    );
    assert!(String::from_utf8_lossy(&archived.stdout).contains("events=3"));
    assert_eq!(
        std::fs::metadata(&archive_path)?.permissions().mode() & 0o7777,
        0o600
    );
    let archive = std::fs::read(&archive_path)?;
    assert!(archive.starts_with(b"PRA1"));
    let mut reopened = StorageInstance::open(fixture.instance_dir())?;
    assert_eq!(
        reopened.database().begin_read()?.open_table(AUDIT)?.len()?,
        0
    );
    let read = reopened.database().begin_read()?;
    let stats = read.open_table(prdb_engine::schema::STATS)?;
    assert!(!stats.iter()?.any(|entry| {
        entry
            .expect("valid stats row")
            .0
            .value()
            .starts_with(b"audit_sink_delivered_v1:")
    }));
    drop(stats);
    drop(read);
    assert!(matches!(
        reopened.archive_audit_events(fixture.instance_dir().join("inside.archive")),
        Err(prdb_engine::bootstrap::AuditArchiveError::InvalidOutputPath)
    ));
    drop(reopened);

    let repeated = Command::new(env!("CARGO_BIN_EXE_prdb-maintain"))
        .args(["audit-archive", "--instance-dir"])
        .arg(fixture.instance_dir())
        .arg("--output")
        .arg(&archive_path)
        .output()?;
    assert!(repeated.status.success());
    assert!(String::from_utf8_lossy(&repeated.stdout).contains("events=0"));
    assert_eq!(std::fs::read(&archive_path)?, archive);
    Ok(())
}

#[test]
fn audit_archive_requires_the_exclusive_instance_lock_and_preserves_sentinel()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new();
    let mut session =
        prdb_engine::bootstrap::initialize(fixture.instance_dir(), "archive_admin", i64::MAX)?;
    let mut secret = session.take_secret().expect("bootstrap secret");
    use zeroize::Zeroize;
    secret.zeroize();
    let ready = session.acknowledge()?;
    let archive_path = fixture.0.join("audit.archive");
    std::fs::write(&archive_path, b"sentinel")?;
    std::fs::set_permissions(&archive_path, std::fs::Permissions::from_mode(0o600))?;

    let blocked = Command::new(env!("CARGO_BIN_EXE_prdb-maintain"))
        .args(["audit-archive", "--instance-dir"])
        .arg(fixture.instance_dir())
        .arg("--output")
        .arg(&archive_path)
        .output()?;
    assert!(!blocked.status.success());
    assert!(String::from_utf8_lossy(&blocked.stderr).contains("InstanceLocked"));
    drop(ready);

    let refused = Command::new(env!("CARGO_BIN_EXE_prdb-maintain"))
        .args(["audit-archive", "--instance-dir"])
        .arg(fixture.instance_dir())
        .arg("--output")
        .arg(&archive_path)
        .output()?;
    assert!(!refused.status.success());
    assert_eq!(std::fs::read(&archive_path)?, b"sentinel");
    let reopened = StorageInstance::open(fixture.instance_dir())?;
    assert_eq!(
        reopened.database().begin_read()?.open_table(AUDIT)?.len()?,
        1
    );
    Ok(())
}

#[test]
fn audit_archive_completes_pruning_from_a_matching_preexisting_archive()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new();
    let mut session =
        prdb_engine::bootstrap::initialize(fixture.instance_dir(), "archive_admin", i64::MAX)?;
    let mut secret = session.take_secret().expect("bootstrap secret");
    use zeroize::Zeroize;
    secret.zeroize();
    drop(session.acknowledge()?);

    let instance = StorageInstance::open(fixture.instance_dir())?;
    let read = instance.database().begin_read()?;
    let table = read.open_table(AUDIT)?;
    let events = table
        .iter()?
        .map(|entry| {
            let (key, value) = entry?;
            Ok::<_, redb::StorageError>((
                <[u8; 16]>::try_from(key.value()).expect("audit key width"),
                value.value().to_vec(),
            ))
        })
        .collect::<Result<Vec<_>, _>>()?;
    assert!(!events.is_empty());
    drop(table);
    drop(read);
    drop(instance);

    let archive_path = fixture.0.join("recovery.archive");
    std::fs::write(&archive_path, prdb_engine::audit_archive::encode(&events)?)?;
    std::fs::set_permissions(&archive_path, std::fs::Permissions::from_mode(0o600))?;
    std::fs::File::open(&archive_path)?.sync_all()?;
    std::fs::File::open(&fixture.0)?.sync_all()?;

    let archived = Command::new(env!("CARGO_BIN_EXE_prdb-maintain"))
        .args(["audit-archive", "--instance-dir"])
        .arg(fixture.instance_dir())
        .arg("--output")
        .arg(&archive_path)
        .output()?;
    assert!(
        archived.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&archived.stderr)
    );
    assert!(
        String::from_utf8_lossy(&archived.stdout).contains(&format!("events={}", events.len()))
    );
    let reopened = StorageInstance::open(fixture.instance_dir())?;
    assert_eq!(
        reopened.database().begin_read()?.open_table(AUDIT)?.len()?,
        0
    );
    Ok(())
}

#[test]
fn init_refuses_to_overwrite_credential_files_before_creating_database() {
    let fixture = Fixture::new();
    std::fs::write(fixture.credential_file(), b"existing").expect("write sentinel");
    let output = initialize_command(&fixture, false);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("refusing to overwrite"));
    assert!(!fixture.instance_dir().exists());
    assert_eq!(
        std::fs::read(fixture.credential_file()).expect("read sentinel"),
        b"existing"
    );
}
