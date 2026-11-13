use prdb_engine::audit::{AuditAction, AuditEvent, AuditResult};
use prdb_engine::bootstrap::StorageInstance;
use prdb_engine::client_credentials::read_client_credentials;
use prdb_engine::schema::AUDIT;
use redb::{ReadableDatabase, ReadableTable, ReadableTableMetadata};
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
