//! Offline conversion of the redb physical file format while retaining the
//! original v2 generation and an independent byte backup.

use super::{
    InstanceLock, REDB_UPGRADE_STAGING_FILE, validate_managed_directory, validate_private_file,
};
use crate::schema::ENGINE_TABLE_NAMES;
use crate::startup::{
    StorageHealthReport, validate_persisted_records, validate_persisted_records_read_only,
};
use redb::{ReadableDatabase as ReadableDatabaseV4, ReadableTable as ReadableTableV4};
use redb_v2::ReadableTable as ReadableTableV2;
use sha2::{Digest, Sha256};
use std::ffi::CString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const DATABASE_FILE: &str = "storage.redb";
const SETUP_SOCKET_FILE: &str = ".prdb-setup.sock";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PhysicalUpgradeReport {
    pub status: &'static str,
    pub database_bytes: u64,
    pub database_sha256: [u8; 32],
    pub health: StorageHealthReport,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FileDigest {
    length: u64,
    sha256: [u8; 32],
}

/// Convert one managed READY instance from redb physical file format v2 to
/// v3. The caller must stop services and explicitly acknowledge the operation.
/// The function takes the normal exclusive instance lock, writes an
/// independent mode-0600 backup outside the instance, validates a same-directory
/// staging copy, then atomically exchanges the active and retained filenames.
pub fn upgrade_redb_v2_instance(
    instance_directory: impl AsRef<Path>,
    backup_file: impl AsRef<Path>,
    confirm_redb_v2: bool,
) -> Result<PhysicalUpgradeReport, Box<dyn std::error::Error>> {
    if !confirm_redb_v2 {
        return Err(invalid("--confirm-redb-v2 is required"));
    }
    let directory = instance_directory.as_ref().canonicalize()?;
    validate_managed_directory(&directory, true)?;
    let _lock = InstanceLock::acquire(&directory)?;
    if directory.join(SETUP_SOCKET_FILE).try_exists()? {
        return Err(invalid("instance has an active setup socket"));
    }
    validate_managed_directory(&directory, true)?;

    let active_path = directory.join(DATABASE_FILE);
    let backup_path = canonical_external_path(backup_file.as_ref(), &directory)?;
    let staging_path = directory.join(REDB_UPGRADE_STAGING_FILE);

    if let Some(health) = open_current_ready(&active_path)? {
        let backup_digest = file_digest(&backup_path)?;
        if !is_redb_v2_file(&backup_path)? {
            return Err(invalid("backup file is not redb physical format v2"));
        }
        if logical_digest_v2(&backup_path)? != logical_digest_v4(&active_path)? {
            return Err(invalid(
                "backup logical records do not match the active image",
            ));
        }
        if path_exists(&staging_path)? {
            validate_private_file(&staging_path)?;
            if file_digest(&staging_path)? != backup_digest {
                return Err(invalid(
                    "retained physical-v2 generation differs from backup",
                ));
            }
        }
        return Ok(PhysicalUpgradeReport {
            status: "already-current",
            database_bytes: backup_digest.length,
            database_sha256: backup_digest.sha256,
            health,
        });
    }

    let source_digest = ensure_independent_backup(&active_path, &backup_path, &directory)?;
    if !is_redb_v2_file(&backup_path)? {
        return Err(invalid("source image is not redb physical format v2"));
    }
    let source_logical_digest = logical_digest_v2(&backup_path)?;

    if path_exists(&staging_path)? {
        validate_private_file(&staging_path)?;
        fs::remove_file(&staging_path)?;
        super::sync_directory(&directory)?;
    }
    copy_private_file(&backup_path, &staging_path)?;

    {
        let mut staged = redb_v2::Database::open(&staging_path)?;
        if !staged.upgrade()? {
            return Err(invalid(
                "staging image was not upgraded from physical format v2",
            ));
        }
    }
    File::open(&staging_path)?.sync_all()?;
    if logical_digest_v2(&staging_path)? != source_logical_digest {
        return Err(invalid(
            "redb physical upgrade changed logical table contents",
        ));
    }

    let staged_health = {
        let staged = redb::Database::open(&staging_path)?;
        let health = validate_persisted_records(&staged)?;
        drop(staged);
        health
    };
    File::open(&staging_path)?.sync_all()?;
    File::open(&active_path)?.sync_all()?;
    let staged_logical_digest = logical_digest_v4(&staging_path)?;
    if staged_logical_digest != source_logical_digest {
        return Err(invalid(
            "redb 4 logical table digest differs from the v2 source",
        ));
    }
    if file_digest(&active_path)? != source_digest || file_digest(&backup_path)? != source_digest {
        return Err(invalid(
            "active source or independent backup changed before publication",
        ));
    }

    if test_exit_before_exchange() {
        std::process::exit(85);
    }
    super::exchange_files(&active_path, &staging_path)?;
    if test_exit_after_exchange() {
        std::process::exit(86);
    }
    if test_fail_directory_sync_after_exchange() {
        return Err(io::Error::other("injected directory sync failure after exchange").into());
    }
    super::sync_directory(&directory)?;

    let health = {
        let active = redb::Database::open(&active_path)?;
        let health = validate_persisted_records(&active)?;
        drop(active);
        health
    };
    File::open(&active_path)?.sync_all()?;
    super::sync_directory(&directory)?;
    if logical_digest_v4(&active_path)? != source_logical_digest {
        return Err(invalid(
            "published image logical table digest differs from source",
        ));
    }
    if file_digest(&staging_path)? != source_digest {
        return Err(invalid(
            "retained source generation differs from independent backup",
        ));
    }

    Ok(PhysicalUpgradeReport {
        status: "upgraded",
        database_bytes: source_digest.length,
        database_sha256: source_digest.sha256,
        health: if health == staged_health {
            health
        } else {
            return Err(invalid(
                "published READY report changed during atomic exchange",
            ));
        },
    })
}

#[cfg(debug_assertions)]
fn test_exit_before_exchange() -> bool {
    std::env::var_os("PRDB_TEST_REDB_UPGRADE_EXIT_BEFORE_EXCHANGE").as_deref()
        == Some(std::ffi::OsStr::new("1"))
}

#[cfg(not(debug_assertions))]
fn test_exit_before_exchange() -> bool {
    false
}

#[cfg(debug_assertions)]
fn test_exit_after_exchange() -> bool {
    std::env::var_os("PRDB_TEST_REDB_UPGRADE_EXIT_AFTER_EXCHANGE").as_deref()
        == Some(std::ffi::OsStr::new("1"))
}

#[cfg(not(debug_assertions))]
fn test_exit_after_exchange() -> bool {
    false
}

#[cfg(debug_assertions)]
fn test_fail_directory_sync_after_exchange() -> bool {
    std::env::var_os("PRDB_TEST_REDB_UPGRADE_FAIL_SYNC_AFTER_EXCHANGE").as_deref()
        == Some(std::ffi::OsStr::new("1"))
}

#[cfg(not(debug_assertions))]
fn test_fail_directory_sync_after_exchange() -> bool {
    false
}

fn open_current_ready(
    path: &Path,
) -> Result<Option<StorageHealthReport>, Box<dyn std::error::Error>> {
    match redb::ReadOnlyDatabase::open(path) {
        Ok(database) => {
            let report = validate_persisted_records_read_only(&database)?;
            Ok(Some(report))
        }
        Err(redb::DatabaseError::UpgradeRequired(2)) => Ok(None),
        Err(error) => Err(Box::new(error)),
    }
}

fn is_redb_v2_file(path: &Path) -> Result<bool, Box<dyn std::error::Error>> {
    Ok(matches!(
        redb::ReadOnlyDatabase::open(path),
        Err(redb::DatabaseError::UpgradeRequired(2))
    ))
}

fn ensure_independent_backup(
    source: &Path,
    backup: &Path,
    instance_directory: &Path,
) -> Result<FileDigest, Box<dyn std::error::Error>> {
    if path_exists(backup)? {
        validate_private_file(backup)?;
        let source_digest = file_digest(source)?;
        if file_digest(backup)? != source_digest {
            return Err(invalid("existing backup does not match the active source"));
        }
        File::open(backup)?.sync_all()?;
        File::open(
            backup
                .parent()
                .ok_or_else(|| invalid("invalid backup path"))?,
        )?
        .sync_all()?;
        return Ok(source_digest);
    }

    let parent = backup
        .parent()
        .ok_or_else(|| invalid("invalid backup path"))?;
    if parent.starts_with(instance_directory) {
        return Err(invalid(
            "independent backup must be outside the instance directory",
        ));
    }
    validate_backup_parent(parent)?;
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let temporary = parent.join(format!(
        ".prdb-redb-upgrade-{}-{nonce}.tmp",
        std::process::id()
    ));
    let mut temporary_created = false;
    let result: Result<FileDigest, Box<dyn std::error::Error>> = (|| {
        copy_private_file(source, &temporary)?;
        temporary_created = true;
        let source_digest = file_digest(source)?;
        if file_digest(&temporary)? != source_digest {
            return Err(invalid("temporary backup differs from active source"));
        }
        publish_without_replace(&temporary, backup)?;
        File::open(parent)?.sync_all()?;
        validate_private_file(backup)?;
        if file_digest(backup)? != source_digest {
            return Err(invalid("published backup differs from active source"));
        }
        Ok(source_digest)
    })();
    if result.is_err() && temporary_created {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn canonical_external_path(
    path: &Path,
    instance_directory: &Path,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let name = path
        .file_name()
        .ok_or_else(|| invalid("invalid backup path"))?;
    if name.is_empty()
        || name == "."
        || name == ".."
        || path.components().any(|part| part == Component::ParentDir)
    {
        return Err(invalid("invalid backup path"));
    }
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .canonicalize()?;
    validate_backup_parent(&parent)?;
    let destination = parent.join(name);
    if destination.starts_with(instance_directory) {
        return Err(invalid(
            "independent backup must be outside the instance directory",
        ));
    }
    Ok(destination)
}

fn copy_private_file(source: &Path, destination: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let mut input = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(source)?;
    validate_private_metadata(&input.metadata()?)?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(destination)?;
    output.set_permissions(fs::Permissions::from_mode(0o600))?;
    validate_private_metadata(&output.metadata()?)?;
    let result: io::Result<()> = (|| {
        io::copy(&mut input, &mut output)?;
        output.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        drop(output);
        let _ = fs::remove_file(destination);
    }
    Ok(result?)
}

fn file_digest(path: &Path) -> Result<FileDigest, Box<dyn std::error::Error>> {
    validate_private_file(path)?;
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(path)?;
    validate_private_metadata(&file.metadata()?)?;
    let mut hasher = Sha256::new();
    let mut length = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        length = length
            .checked_add(count as u64)
            .ok_or_else(|| invalid("database image size overflow"))?;
        hasher.update(&buffer[..count]);
    }
    Ok(FileDigest {
        length,
        sha256: hasher.finalize().into(),
    })
}

fn logical_digest_v2(path: &Path) -> Result<[u8; 32], Box<dyn std::error::Error>> {
    let temporary = TemporaryDatabaseCopy::new(path)?;
    let database = redb_v2::Database::open(&temporary.path)?;
    let read = database.begin_read()?;
    let mut hasher = Sha256::new();
    for name in ENGINE_TABLE_NAMES {
        let table = read.open_table(redb_v2::TableDefinition::<&[u8], &[u8]>::new(name))?;
        hash_table_v2(name, &table, &mut hasher)?;
    }
    Ok(hasher.finalize().into())
}

fn hash_table_v2(
    name: &str,
    table: &redb_v2::ReadOnlyTable<&[u8], &[u8]>,
    hasher: &mut Sha256,
) -> Result<(), Box<dyn std::error::Error>> {
    hash_table_header(name, hasher);
    for entry in table.iter()? {
        let (key, value) = entry?;
        hash_record(key.value(), value.value(), hasher);
    }
    Ok(())
}

fn logical_digest_v4(path: &Path) -> Result<[u8; 32], Box<dyn std::error::Error>> {
    let database = redb::ReadOnlyDatabase::open(path)?;
    let read = database.begin_read()?;
    let mut hasher = Sha256::new();
    for name in ENGINE_TABLE_NAMES {
        let table = read.open_table(redb::TableDefinition::<&[u8], &[u8]>::new(name))?;
        hash_table_header(name, &mut hasher);
        for entry in table.iter()? {
            let (key, value) = entry?;
            hash_record(key.value(), value.value(), &mut hasher);
        }
    }
    Ok(hasher.finalize().into())
}

fn hash_table_header(name: &str, hasher: &mut Sha256) {
    hasher.update((name.len() as u64).to_le_bytes());
    hasher.update(name.as_bytes());
}

fn hash_record(key: &[u8], value: &[u8], hasher: &mut Sha256) {
    hasher.update((key.len() as u64).to_le_bytes());
    hasher.update(key);
    hasher.update((value.len() as u64).to_le_bytes());
    hasher.update(value);
}

fn path_exists(path: &Path) -> Result<bool, io::Error> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn publish_without_replace(source: &Path, destination: &Path) -> Result<(), io::Error> {
    let source = CString::new(source.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid temporary path"))?;
    let destination = CString::new(destination.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid backup path"))?;
    // SAFETY: both C strings are NUL-terminated, and RENAME_NOREPLACE prevents
    // replacement of an operator-provided backup.
    let result = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            source.as_ptr(),
            libc::AT_FDCWD,
            destination.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn invalid(message: &'static str) -> Box<dyn std::error::Error> {
    Box::new(io::Error::new(io::ErrorKind::InvalidInput, message))
}

fn effective_uid() -> u32 {
    // SAFETY: `geteuid` has no preconditions and does not access pointers.
    unsafe { libc::geteuid() }
}

fn validate_backup_parent(parent: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let metadata = fs::metadata(parent)?;
    let sticky = metadata.permissions().mode() & 0o1000 != 0;
    if !metadata.is_dir()
        || (metadata.uid() != effective_uid() && !(sticky && metadata.uid() == 0))
        || (metadata.permissions().mode() & 0o022 != 0 && !sticky)
    {
        return Err(invalid("backup parent directory is not safely owned"));
    }
    Ok(())
}

fn validate_private_metadata(metadata: &fs::Metadata) -> Result<(), Box<dyn std::error::Error>> {
    if !metadata.file_type().is_file()
        || metadata.uid() != effective_uid()
        || metadata.permissions().mode() & 0o7777 != 0o600
        || metadata.nlink() != 1
    {
        return Err(invalid(
            "database image must be an owner-owned private file",
        ));
    }
    Ok(())
}

struct TemporaryDatabaseCopy {
    directory: PathBuf,
    path: PathBuf,
}

impl TemporaryDatabaseCopy {
    fn new(source: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "prdb-redb-v2-digest-{}-{nonce}",
            std::process::id()
        ));
        fs::DirBuilder::new().mode(0o700).create(&directory)?;
        let path = directory.join("database.redb");
        if let Err(error) = copy_private_file(source, &path) {
            let _ = fs::remove_dir_all(&directory);
            return Err(error);
        }
        Ok(Self { directory, path })
    }
}

impl Drop for TemporaryDatabaseCopy {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}
