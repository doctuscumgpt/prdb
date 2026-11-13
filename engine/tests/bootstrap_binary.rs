use prdb_engine::audit::{AuditAction, AuditEvent, AuditResult};
use prdb_engine::bootstrap::{BootstrapError, StorageInstance};
use prdb_engine::catalog::{decode_registered_envelope, encode_envelope, family_schema};
use prdb_engine::client_credentials::read_client_credentials;
use prdb_engine::metadata::EngineMetadata;
use prdb_engine::schema::{AUDIT, CATALOG, ENGINE_META, POLICIES};
use prdb_engine::startup::StartupError;
use redb::{
    Database, ReadOnlyDatabase, ReadableDatabase, ReadableTable, ReadableTableMetadata, TableHandle,
};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
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
    assert!(help.contains("inspect --instance-dir PATH"));
    assert!(help.contains("migration-inspect --instance-dir PATH"));
    assert!(help.contains("migration-discard-stage --instance-dir PATH --backup-dir PATH"));
}

#[test]
fn maintenance_inspection_is_read_only_bounded_and_requires_offline_lock() {
    let fixture = Fixture::new();
    let mut session =
        prdb_engine::bootstrap::initialize(fixture.instance_dir(), "inspect_admin", i64::MAX)
            .expect("initialize inspect fixture");
    let mut secret = session.take_secret().expect("bootstrap secret");
    use zeroize::Zeroize;
    secret.zeroize();
    drop(session.acknowledge().expect("publish READY"));

    let database_path = fixture.instance_dir().join("storage.redb");
    let before = std::fs::read(&database_path).expect("read database before inspection");
    let inspected = Command::new(env!("CARGO_BIN_EXE_prdb-maintain"))
        .args(["inspect", "--instance-dir"])
        .arg(fixture.instance_dir())
        .output()
        .expect("run read-only inspect");
    assert!(
        inspected.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&inspected.stderr)
    );
    let output = String::from_utf8(inspected.stdout).expect("UTF-8 inspection result");
    for expected in [
        "status=READY",
        "catalog_version=2",
        "expression_version=1",
        "builtin_semantics_version=1",
        "catalog_records=",
        "rows=",
        "index_entries=",
    ] {
        assert!(output.contains(expected), "missing {expected}: {output}");
    }
    assert_eq!(
        std::fs::read(&database_path).expect("read database after inspection"),
        before,
        "inspection must not write to the redb image"
    );

    let live = StorageInstance::open(fixture.instance_dir()).expect("lock instance");
    let rejected = Command::new(env!("CARGO_BIN_EXE_prdb-maintain"))
        .args(["inspect", "--instance-dir"])
        .arg(fixture.instance_dir())
        .output()
        .expect("attempt inspection during service ownership");
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("InstanceLocked"));
    drop(live);
}

#[test]
fn migration_inspection_validates_active_and_staged_images_without_writing_them() {
    let fixture = Fixture::new();
    let mut session = prdb_engine::bootstrap::initialize(
        fixture.instance_dir(),
        "migration_inspect_admin",
        i64::MAX,
    )
    .expect("initialize migration inspection fixture");
    let mut secret = session.take_secret().expect("bootstrap secret");
    use zeroize::Zeroize;
    secret.zeroize();
    drop(session.acknowledge().expect("publish READY"));

    let active_path = fixture.instance_dir().join("storage.redb");
    let staging_path = fixture.instance_dir().join(".prdb-migration.next");
    let backup_path = fixture.0.join("verified-backup");
    std::fs::copy(&active_path, &staging_path).expect("prepare staged image");
    std::fs::set_permissions(&staging_path, std::fs::Permissions::from_mode(0o600))
        .expect("make staged image private");
    let active_before = std::fs::read(&active_path).expect("read active image");
    let staging_before = std::fs::read(&staging_path).expect("read staged image");

    let inspected = Command::new(env!("CARGO_BIN_EXE_prdb-maintain"))
        .args(["migration-inspect", "--instance-dir"])
        .arg(fixture.instance_dir())
        .output()
        .expect("run migration inspection");
    assert!(
        inspected.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&inspected.stderr)
    );
    let output = String::from_utf8(inspected.stdout).expect("UTF-8 migration inspection result");
    assert!(output.contains("active=READY"), "{output}");
    assert!(
        output.contains("staging=READY relationship=same_frontier"),
        "{output}"
    );
    assert_eq!(
        std::fs::read(&active_path).expect("reread active image"),
        active_before
    );
    assert_eq!(
        std::fs::read(&staging_path).expect("reread staged image"),
        staging_before
    );

    let live = StorageInstance::open(fixture.instance_dir()).expect("hold instance lock");
    let rejected = Command::new(env!("CARGO_BIN_EXE_prdb-maintain"))
        .args(["migration-inspect", "--instance-dir"])
        .arg(fixture.instance_dir())
        .output()
        .expect("attempt migration inspection while instance is owned");
    assert!(!rejected.status.success());
    drop(live);

    prdb_engine::offline_backup::create_verified_backup(fixture.instance_dir(), &backup_path)
        .expect("create source backup after closing writer");
    let active_before_discard =
        std::fs::read(&active_path).expect("read active source before discard");

    let discarded = Command::new(env!("CARGO_BIN_EXE_prdb-maintain"))
        .args(["migration-discard-stage", "--instance-dir"])
        .arg(fixture.instance_dir())
        .arg("--backup-dir")
        .arg(&backup_path)
        .output()
        .expect("discard verified unpublished stage");
    assert!(
        discarded.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&discarded.stderr)
    );
    assert!(
        !staging_path.exists(),
        "verified unpublished staging image should be removed"
    );
    assert_eq!(
        std::fs::read(&active_path).expect("reread active image"),
        active_before_discard,
        "discard must preserve active source bytes"
    );
}

#[test]
fn migration_stage_discard_rejects_an_unrelated_verified_backup() {
    let active_fixture = Fixture::new();
    let other_fixture = Fixture::new();
    for (fixture, name) in [
        (&active_fixture, "discard_source_admin"),
        (&other_fixture, "discard_other_admin"),
    ] {
        let mut session =
            prdb_engine::bootstrap::initialize(fixture.instance_dir(), name, i64::MAX)
                .expect("initialize discard fixture");
        let mut secret = session.take_secret().expect("bootstrap secret");
        use zeroize::Zeroize;
        secret.zeroize();
        drop(session.acknowledge().expect("publish READY"));
    }
    let active_path = active_fixture.instance_dir().join("storage.redb");
    let staging_path = active_fixture.instance_dir().join(".prdb-migration.next");
    std::fs::copy(&active_path, &staging_path).expect("prepare staged image");
    std::fs::set_permissions(&staging_path, std::fs::Permissions::from_mode(0o600))
        .expect("make staged image private");
    let backup_path = other_fixture.0.join("unrelated-backup");
    prdb_engine::offline_backup::create_verified_backup(other_fixture.instance_dir(), &backup_path)
        .expect("create unrelated source backup");

    let live = StorageInstance::open(active_fixture.instance_dir()).expect("hold instance lock");
    let locked = Command::new(env!("CARGO_BIN_EXE_prdb-maintain"))
        .args(["migration-discard-stage", "--instance-dir"])
        .arg(active_fixture.instance_dir())
        .arg("--backup-dir")
        .arg(&backup_path)
        .output()
        .expect("attempt discard while instance is owned");
    assert!(!locked.status.success());
    drop(live);

    let rejected = Command::new(env!("CARGO_BIN_EXE_prdb-maintain"))
        .args(["migration-discard-stage", "--instance-dir"])
        .arg(active_fixture.instance_dir())
        .arg("--backup-dir")
        .arg(&backup_path)
        .output()
        .expect("attempt discard with unrelated backup");
    assert!(!rejected.status.success());
    assert!(staging_path.exists(), "failed discard must retain staging");
    drop(
        StorageInstance::open(active_fixture.instance_dir()).expect("active source remains READY"),
    );
}

#[test]
fn migration_inspection_reports_database_identity_mismatch() {
    let active_fixture = Fixture::new();
    let staged_fixture = Fixture::new();
    for (fixture, name) in [
        (&active_fixture, "active_migration_inspect_admin"),
        (&staged_fixture, "staged_migration_inspect_admin"),
    ] {
        let mut session =
            prdb_engine::bootstrap::initialize(fixture.instance_dir(), name, i64::MAX)
                .expect("initialize migration inspection fixture");
        let mut secret = session.take_secret().expect("bootstrap secret");
        use zeroize::Zeroize;
        secret.zeroize();
        drop(session.acknowledge().expect("publish READY"));
    }
    let staging_path = active_fixture.instance_dir().join(".prdb-migration.next");
    std::fs::copy(
        staged_fixture.instance_dir().join("storage.redb"),
        &staging_path,
    )
    .expect("prepare unrelated staged image");
    std::fs::set_permissions(&staging_path, std::fs::Permissions::from_mode(0o600))
        .expect("make staged image private");

    let inspected = Command::new(env!("CARGO_BIN_EXE_prdb-maintain"))
        .args(["migration-inspect", "--instance-dir"])
        .arg(active_fixture.instance_dir())
        .output()
        .expect("inspect unrelated generations");
    assert!(
        inspected.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&inspected.stderr)
    );
    let output = String::from_utf8(inspected.stdout).expect("UTF-8 migration inspection result");
    assert!(
        output.contains("staging=READY relationship=database_id_mismatch"),
        "{output}"
    );
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
fn v1_source_validator_checks_legacy_ready_images_without_admitting_them() {
    let fixture = Fixture::new();
    let mut session =
        prdb_engine::bootstrap::initialize(fixture.instance_dir(), "v1_source_admin", i64::MAX)
            .expect("initialize source fixture");
    let mut secret = session.take_secret().expect("bootstrap secret");
    use zeroize::Zeroize;
    secret.zeroize();
    drop(session.acknowledge().expect("publish source READY"));

    let database_path = fixture.instance_dir().join("storage.redb");
    let database = Database::open(&database_path).expect("open fixture for legacy conversion");
    let mut metadata = {
        let read = database.begin_read().expect("read fixture metadata");
        let table = read.open_table(ENGINE_META).expect("open metadata table");
        let encoded = table.get(&b"format"[..]).expect("read metadata").unwrap();
        EngineMetadata::decode(encoded.value()).expect("decode current metadata")
    };
    metadata.catalog_version = 1;
    let encoded_metadata = metadata.encode().expect("encode v1 metadata");
    let mut database_key = 1u16.to_be_bytes().to_vec();
    database_key.push(1);
    database_key.extend_from_slice(&metadata.database_id);
    let mut fields = {
        let read = database.begin_read().expect("read fixture catalog");
        let table = read.open_table(CATALOG).expect("open catalog table");
        let encoded = table
            .get(database_key.as_slice())
            .expect("read database record")
            .unwrap();
        decode_registered_envelope(encoded.value())
            .expect("decode database record")
            .fields
    };
    for field in &mut fields {
        if field.tag == 2 {
            field.payload = 1u32.to_le_bytes().to_vec();
        }
        if field.tag == 3 {
            assert_eq!(
                u32::from_le_bytes(field.payload[..4].try_into().unwrap()),
                31
            );
            field.payload[..4].copy_from_slice(&30u32.to_le_bytes());
            field.payload.truncate(4 + 30 * 20);
        }
    }
    let database_record = encode_envelope(
        &family_schema(1).expect("database family schema"),
        &database_key,
        &fields,
    )
    .expect("encode v1 database record");
    {
        let write = database.begin_write().expect("begin fixture downgrade");
        write
            .open_table(CATALOG)
            .expect("open catalog for downgrade")
            .insert(database_key.as_slice(), database_record.as_slice())
            .expect("write v1 database record");
        write
            .open_table(ENGINE_META)
            .expect("open metadata for downgrade")
            .insert(&b"format"[..], &encoded_metadata[..])
            .expect("write v1 metadata");
        write.commit().expect("commit v1 source fixture");
    }
    drop(database);

    let source = ReadOnlyDatabase::open(&database_path).expect("open legacy source read-only");
    let report = prdb_engine::startup::validate_v1_migration_source_read_only(&source)
        .expect("validate v1 source for offline conversion");
    assert_eq!(report.metadata.catalog_version, 1);
    assert_eq!(report.metadata.database_id, metadata.database_id);
    assert!(matches!(
        prdb_engine::startup::validate_persisted_records_read_only(&source),
        Err(StartupError::UnsupportedFormat)
    ));
    drop(source);
    let inspected = Command::new(env!("CARGO_BIN_EXE_prdb-maintain"))
        .args(["migration-inspect", "--instance-dir"])
        .arg(fixture.instance_dir())
        .output()
        .expect("inspect legacy migration source");
    assert!(
        inspected.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&inspected.stderr)
    );
    let output = String::from_utf8(inspected.stdout).expect("UTF-8 migration inspection result");
    assert!(output.contains("active=READY"), "{output}");
    assert!(output.contains("catalog_version=1"), "{output}");
    assert!(output.contains("staging=absent"), "{output}");
    assert!(matches!(
        StorageInstance::open(fixture.instance_dir()),
        Err(BootstrapError::Startup(StartupError::UnsupportedFormat))
    ));
}

#[test]
fn startup_uses_active_image_and_only_tolerates_private_migration_staging_file() {
    let fixture = Fixture::new();
    let mut session =
        prdb_engine::bootstrap::initialize(fixture.instance_dir(), "migration_admin", i64::MAX)
            .expect("initialize migration fixture");
    let mut secret = session.take_secret().expect("bootstrap secret");
    use zeroize::Zeroize;
    secret.zeroize();
    drop(session.acknowledge().expect("publish READY"));

    let staging = fixture.instance_dir().join(".prdb-migration.next");
    std::fs::write(&staging, b"interrupted target generation")
        .expect("write interrupted migration target");
    std::fs::set_permissions(&staging, std::fs::Permissions::from_mode(0o600))
        .expect("make migration target private");
    drop(StorageInstance::open(fixture.instance_dir()).expect("start from active source image"));

    std::fs::set_permissions(&staging, std::fs::Permissions::from_mode(0o640))
        .expect("broaden staging file permissions");
    assert!(matches!(
        StorageInstance::open(fixture.instance_dir()),
        Err(BootstrapError::NotPrivate)
    ));
    std::fs::set_permissions(&staging, std::fs::Permissions::from_mode(0o600))
        .expect("restore staging permissions");

    let unexpected = fixture.instance_dir().join(".prdb-migration.extra");
    std::fs::write(&unexpected, b"unknown file").expect("write unexpected file");
    std::fs::set_permissions(&unexpected, std::fs::Permissions::from_mode(0o600))
        .expect("make unexpected file private");
    assert!(matches!(
        StorageInstance::open(fixture.instance_dir()),
        Err(BootstrapError::NotPrivate)
    ));
}

#[test]
fn staged_generation_publication_atomically_exchanges_names_and_retains_source() {
    let fixture = Fixture::new();
    let mut session =
        prdb_engine::bootstrap::initialize(fixture.instance_dir(), "publish_admin", i64::MAX)
            .expect("initialize publication fixture");
    let mut secret = session.take_secret().expect("bootstrap secret");
    use zeroize::Zeroize;
    secret.zeroize();
    drop(session.acknowledge().expect("publish READY"));

    let active_path = fixture.instance_dir().join("storage.redb");
    let staging_path = fixture.instance_dir().join(".prdb-migration.next");
    let backup_path = fixture.0.join("verified-backup");
    prdb_engine::offline_backup::create_verified_backup(fixture.instance_dir(), &backup_path)
        .expect("create independently verified source backup");
    std::fs::copy(&active_path, &staging_path).expect("prepare staged database image");
    std::fs::set_permissions(&staging_path, std::fs::Permissions::from_mode(0o600))
        .expect("make staged database private");
    let active_inode = std::fs::metadata(&active_path).unwrap().ino();
    let staged_inode = std::fs::metadata(&staging_path).unwrap().ino();

    let active = StorageInstance::open(fixture.instance_dir()).expect("open active generation");
    let published = active
        .publish_staged_generation(&backup_path)
        .expect("atomically publish staged generation");
    assert_eq!(std::fs::metadata(&active_path).unwrap().ino(), staged_inode);
    assert_eq!(
        std::fs::metadata(&staging_path).unwrap().ino(),
        active_inode
    );
    drop(published);
    drop(StorageInstance::open(fixture.instance_dir()).expect("reopen published generation"));
}

#[test]
fn staged_generation_publication_rejects_different_database_identity() {
    let source = Fixture::new();
    let other = Fixture::new();
    for (fixture, name) in [(&source, "source_admin"), (&other, "other_admin")] {
        let mut session =
            prdb_engine::bootstrap::initialize(fixture.instance_dir(), name, i64::MAX)
                .expect("initialize publication fixture");
        let mut secret = session.take_secret().expect("bootstrap secret");
        use zeroize::Zeroize;
        secret.zeroize();
        drop(session.acknowledge().expect("publish READY"));
    }

    let active_path = source.instance_dir().join("storage.redb");
    let staging_path = source.instance_dir().join(".prdb-migration.next");
    let backup_path = source.0.join("verified-backup");
    prdb_engine::offline_backup::create_verified_backup(source.instance_dir(), &backup_path)
        .expect("create independently verified source backup");
    std::fs::copy(other.instance_dir().join("storage.redb"), &staging_path)
        .expect("copy a different READY database");
    std::fs::set_permissions(&staging_path, std::fs::Permissions::from_mode(0o600))
        .expect("make staged database private");
    let active_inode = std::fs::metadata(&active_path).unwrap().ino();
    let staged_inode = std::fs::metadata(&staging_path).unwrap().ino();

    let error = match StorageInstance::open(source.instance_dir())
        .expect("open source generation")
        .publish_staged_generation(&backup_path)
    {
        Ok(_) => panic!("reject different database identity"),
        Err(error) => error,
    };
    assert!(matches!(error, BootstrapError::InvalidRequest));
    assert_eq!(std::fs::metadata(&active_path).unwrap().ino(), active_inode);
    assert_eq!(
        std::fs::metadata(&staging_path).unwrap().ino(),
        staged_inode
    );
    drop(StorageInstance::open(source.instance_dir()).expect("source generation remains active"));
}

#[test]
fn staged_generation_publication_requires_a_matching_verified_backup() {
    let source = Fixture::new();
    let other = Fixture::new();
    for (fixture, name) in [(&source, "source_backup_admin"), (&other, "backup_admin")] {
        let mut session =
            prdb_engine::bootstrap::initialize(fixture.instance_dir(), name, i64::MAX)
                .expect("initialize publication fixture");
        let mut secret = session.take_secret().expect("bootstrap secret");
        use zeroize::Zeroize;
        secret.zeroize();
        drop(session.acknowledge().expect("publish READY"));
    }

    let backup_path = other.0.join("verified-backup");
    prdb_engine::offline_backup::create_verified_backup(other.instance_dir(), &backup_path)
        .expect("create an unrelated verified backup");
    let active_path = source.instance_dir().join("storage.redb");
    let staging_path = source.instance_dir().join(".prdb-migration.next");
    std::fs::copy(&active_path, &staging_path).expect("prepare staged source image");
    std::fs::set_permissions(&staging_path, std::fs::Permissions::from_mode(0o600))
        .expect("make staged database private");
    let active_inode = std::fs::metadata(&active_path).unwrap().ino();
    let staged_inode = std::fs::metadata(&staging_path).unwrap().ino();

    let error = match StorageInstance::open(source.instance_dir())
        .expect("open source generation")
        .publish_staged_generation(&backup_path)
    {
        Ok(_) => panic!("reject unrelated verified backup"),
        Err(error) => error,
    };
    assert!(matches!(error, BootstrapError::InvalidRequest));
    assert_eq!(std::fs::metadata(&active_path).unwrap().ino(), active_inode);
    assert_eq!(
        std::fs::metadata(&staging_path).unwrap().ino(),
        staged_inode
    );
    drop(StorageInstance::open(source.instance_dir()).expect("source generation remains active"));
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
