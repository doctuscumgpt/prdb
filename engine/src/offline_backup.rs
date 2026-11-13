//! Verified, quiescent copies of READY database instances.

use crate::bootstrap::{BootstrapError, ReadOnlyStorageInstance, StorageInstance};
use crate::startup::{
    StartupError, StorageHealthReport, validate_migration_image_read_only,
    validate_persisted_records_read_only,
};
use redb::ReadOnlyDatabase;
use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};

const DATABASE_FILE: &str = "storage.redb";
const LOCK_FILE: &str = ".prdb.lock";
const MARKER_FILE: &str = ".prdb-bootstrap";
const MANIFEST_MAGIC: &str = "PRDB-BACKUP-1";
const MAX_MANIFEST_BYTES: usize = 256;

#[derive(Debug)]
pub enum OfflineBackupError {
    InvalidPath,
    DestinationExists,
    InvalidManifest,
    IntegrityMismatch,
    Bootstrap(BootstrapError),
    Io(io::Error),
}

impl std::fmt::Display for OfflineBackupError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPath => formatter.write_str("invalid offline backup path"),
            Self::DestinationExists => formatter.write_str("backup destination already exists"),
            Self::InvalidManifest => formatter.write_str("invalid offline backup manifest"),
            Self::IntegrityMismatch => {
                formatter.write_str("offline backup file does not match its manifest")
            }
            Self::Bootstrap(error) => write!(formatter, "database validation failed: {error}"),
            Self::Io(error) => write!(formatter, "offline backup I/O failed: {error}"),
        }
    }
}

impl std::error::Error for OfflineBackupError {}

impl From<BootstrapError> for OfflineBackupError {
    fn from(error: BootstrapError) -> Self {
        Self::Bootstrap(error)
    }
}

impl From<io::Error> for OfflineBackupError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackupVerification {
    pub database_bytes: u64,
    pub database_sha256: [u8; 32],
    pub database_id: [u8; 16],
    pub commit_sequence: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct FileDigest {
    length: u64,
    sha256: [u8; 32],
}

/// Copy a READY instance into a new private directory while holding the source
/// instance lock. A manifest is published only after the copied redb image
/// independently opens and passes the startup integrity validator.
pub fn create_verified_backup(
    source_directory: impl AsRef<Path>,
    backup_directory: impl AsRef<Path>,
) -> Result<BackupVerification, OfflineBackupError> {
    let source_directory = source_directory.as_ref().canonicalize()?;
    let source = StorageInstance::open(&source_directory)?;
    source.close_database_and(|source_directory| {
        create_backup_copy(
            source_directory,
            backup_directory.as_ref(),
            validate_persisted_records_read_only,
        )
    })
}

/// Create a verified backup of either a current READY instance or a catalog-v1
/// migration source while holding its instance lock and without writing source.
pub fn create_verified_migration_backup(
    source_directory: impl AsRef<Path>,
    backup_directory: impl AsRef<Path>,
) -> Result<BackupVerification, OfflineBackupError> {
    let source_directory = source_directory.as_ref().canonicalize()?;
    let source = ReadOnlyStorageInstance::open_for_migration_inspection(&source_directory)?;
    validate_migration_image_read_only(source.database()).map_err(BootstrapError::Startup)?;
    create_backup_copy(
        &source_directory,
        backup_directory.as_ref(),
        validate_migration_image_read_only,
    )
}

fn create_backup_copy(
    source_directory: &Path,
    backup_directory: &Path,
    validate: fn(&ReadOnlyDatabase) -> Result<StorageHealthReport, StartupError>,
) -> Result<BackupVerification, OfflineBackupError> {
    let destination = canonical_destination(backup_directory)?;
    if destination.starts_with(source_directory) {
        return Err(OfflineBackupError::InvalidPath);
    }
    let manifest_path = manifest_path(&destination)?;
    if path_exists(&destination)? || path_exists(&manifest_path)? {
        return Err(OfflineBackupError::DestinationExists);
    }

    fs::create_dir(&destination)?;
    let mut manifest_created = false;
    let result = (|| {
        fs::set_permissions(&destination, fs::Permissions::from_mode(0o700))?;
        sync_directory(
            destination
                .parent()
                .ok_or(OfflineBackupError::InvalidPath)?,
        )?;

        let marker_copy = copy_private_file(
            &source_directory.join(MARKER_FILE),
            &destination.join(MARKER_FILE),
        )?;
        let database_copy = copy_private_file(
            &source_directory.join(DATABASE_FILE),
            &destination.join(DATABASE_FILE),
        )?;
        sync_directory(&destination)?;

        if marker_copy != digest_private_file(&destination.join(MARKER_FILE))?
            || database_copy != digest_private_file(&destination.join(DATABASE_FILE))?
        {
            return Err(OfflineBackupError::IntegrityMismatch);
        }

        let verified = ReadOnlyStorageInstance::open_for_migration_inspection(&destination)?;
        let validation = validate(verified.database()).map_err(BootstrapError::Startup)?;
        let marker = digest_private_file(&destination.join(MARKER_FILE))?;
        let database = digest_private_file(&destination.join(DATABASE_FILE))?;
        if marker_copy != marker || database_copy != database {
            return Err(OfflineBackupError::IntegrityMismatch);
        }
        drop(verified);

        write_manifest(&manifest_path, &marker, &database)?;
        manifest_created = true;
        sync_directory(
            destination
                .parent()
                .ok_or(OfflineBackupError::InvalidPath)?,
        )?;
        Ok(BackupVerification {
            database_bytes: database.length,
            database_sha256: database.sha256,
            database_id: validation.metadata.database_id,
            commit_sequence: validation.metadata.commit_sequence,
        })
    })();

    if result.is_err() {
        if manifest_created {
            let _ = fs::remove_file(&manifest_path);
        }
        let _ = fs::remove_dir_all(&destination);
        if let Some(parent) = destination.parent() {
            let _ = sync_directory(parent);
        }
    }
    result
}

/// Verify the private manifest, both copied files, and the READY database
/// integrity before treating a backup directory as a retained source.
pub fn verify_backup(
    backup_directory: impl AsRef<Path>,
) -> Result<BackupVerification, OfflineBackupError> {
    let backup_directory = backup_directory.as_ref().canonicalize()?;
    let lock = open_private_file(&backup_directory.join(LOCK_FILE))?;
    if lock.metadata()?.len() != 0 {
        return Err(OfflineBackupError::InvalidPath);
    }
    let instance = ReadOnlyStorageInstance::open_for_migration_inspection(&backup_directory)?;
    let validation =
        validate_migration_image_read_only(instance.database()).map_err(BootstrapError::Startup)?;
    let manifest = read_manifest(&manifest_path(&backup_directory)?)?;
    let marker = digest_private_file(&backup_directory.join(MARKER_FILE))?;
    let database = digest_private_file(&backup_directory.join(DATABASE_FILE))?;
    if marker != manifest.0 || database != manifest.1 {
        return Err(OfflineBackupError::IntegrityMismatch);
    }
    Ok(BackupVerification {
        database_bytes: database.length,
        database_sha256: database.sha256,
        database_id: validation.metadata.database_id,
        commit_sequence: validation.metadata.commit_sequence,
    })
}

/// Compare a private database image with the exact database bytes recorded by
/// a previously verified backup manifest.
pub fn database_image_matches_backup(
    image_path: impl AsRef<Path>,
    backup: &BackupVerification,
) -> Result<bool, OfflineBackupError> {
    let digest = digest_private_file(image_path.as_ref())?;
    Ok(digest.length == backup.database_bytes && digest.sha256 == backup.database_sha256)
}

fn canonical_destination(path: &Path) -> Result<PathBuf, OfflineBackupError> {
    let name = path.file_name().ok_or(OfflineBackupError::InvalidPath)?;
    if name.is_empty() || name == "." || name == ".." {
        return Err(OfflineBackupError::InvalidPath);
    }
    if path
        .components()
        .any(|component| component == Component::ParentDir)
    {
        return Err(OfflineBackupError::InvalidPath);
    }
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .canonicalize()?;
    Ok(parent.join(name))
}

fn manifest_path(directory: &Path) -> Result<PathBuf, OfflineBackupError> {
    let name = directory
        .file_name()
        .ok_or(OfflineBackupError::InvalidPath)?;
    let mut manifest_name = OsString::from_vec(name.as_bytes().to_vec());
    manifest_name.push(".prdb-backup");
    Ok(directory
        .parent()
        .ok_or(OfflineBackupError::InvalidPath)?
        .join(manifest_name))
}

fn path_exists(path: &Path) -> Result<bool, OfflineBackupError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(OfflineBackupError::Io(error)),
    }
}

fn copy_private_file(source: &Path, destination: &Path) -> Result<FileDigest, OfflineBackupError> {
    let mut input = open_private_file(source)?;
    let source_length = input.metadata()?.len();
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(destination)?;
    let mut hasher = Sha256::new();
    let mut length = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        output.write_all(&buffer[..count])?;
        hasher.update(&buffer[..count]);
        length = length
            .checked_add(count as u64)
            .ok_or(OfflineBackupError::IntegrityMismatch)?;
    }
    if length != source_length || input.metadata()?.len() != source_length {
        return Err(OfflineBackupError::IntegrityMismatch);
    }
    output.set_permissions(fs::Permissions::from_mode(0o600))?;
    output.sync_all()?;
    Ok(FileDigest {
        length,
        sha256: hasher.finalize().into(),
    })
}

fn digest_private_file(path: &Path) -> Result<FileDigest, OfflineBackupError> {
    let mut file = open_private_file(path)?;
    let source_length = file.metadata()?.len();
    let mut hasher = Sha256::new();
    let mut length = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
        length = length
            .checked_add(count as u64)
            .ok_or(OfflineBackupError::IntegrityMismatch)?;
    }
    if length != source_length || file.metadata()?.len() != source_length {
        return Err(OfflineBackupError::IntegrityMismatch);
    }
    Ok(FileDigest {
        length,
        sha256: hasher.finalize().into(),
    })
}

fn open_private_file(path: &Path) -> Result<File, OfflineBackupError> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.file_type().is_file()
        || metadata.mode() & 0o7777 != 0o600
        || metadata.uid() != unsafe { libc::geteuid() }
    {
        return Err(OfflineBackupError::InvalidPath);
    }
    Ok(file)
}

fn write_manifest(
    path: &Path,
    marker: &FileDigest,
    database: &FileDigest,
) -> Result<(), OfflineBackupError> {
    let content = format!(
        "{MANIFEST_MAGIC}\n{} {}\n{} {}\n",
        marker.length,
        encode_hex(&marker.sha256),
        database.length,
        encode_hex(&database.sha256)
    );
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(path)?;
    let result = file
        .set_permissions(fs::Permissions::from_mode(0o600))
        .and_then(|()| file.write_all(content.as_bytes()))
        .and_then(|()| file.sync_all());
    if let Err(error) = result {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(OfflineBackupError::Io(error));
    }
    Ok(())
}

fn read_manifest(path: &Path) -> Result<(FileDigest, FileDigest), OfflineBackupError> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.file_type().is_file()
        || metadata.mode() & 0o7777 != 0o600
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.len() > MAX_MANIFEST_BYTES as u64
    {
        return Err(OfflineBackupError::InvalidManifest);
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take((MAX_MANIFEST_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Err(OfflineBackupError::InvalidManifest);
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| OfflineBackupError::InvalidManifest)?;
    let lines = text.lines().collect::<Vec<_>>();
    if lines.len() != 3 || lines[0] != MANIFEST_MAGIC || !text.ends_with('\n') {
        return Err(OfflineBackupError::InvalidManifest);
    }
    let marker = parse_digest_line(lines[1])?;
    let database = parse_digest_line(lines[2])?;
    let canonical = format!(
        "{MANIFEST_MAGIC}\n{} {}\n{} {}\n",
        marker.length,
        encode_hex(&marker.sha256),
        database.length,
        encode_hex(&database.sha256)
    );
    if canonical.as_bytes() != bytes {
        return Err(OfflineBackupError::InvalidManifest);
    }
    Ok((marker, database))
}

fn parse_digest_line(line: &str) -> Result<FileDigest, OfflineBackupError> {
    let mut fields = line.split(' ');
    let length = fields
        .next()
        .ok_or(OfflineBackupError::InvalidManifest)?
        .parse::<u64>()
        .map_err(|_| OfflineBackupError::InvalidManifest)?;
    let hash = fields
        .next()
        .filter(|_| fields.next().is_none())
        .ok_or(OfflineBackupError::InvalidManifest)?;
    let decoded = decode_hex(hash)?;
    let sha256 = decoded
        .try_into()
        .map_err(|_| OfflineBackupError::InvalidManifest)?;
    Ok(FileDigest { length, sha256 })
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

fn decode_hex(value: &str) -> Result<Vec<u8>, OfflineBackupError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(OfflineBackupError::InvalidManifest);
    }
    let mut bytes = Vec::with_capacity(32);
    for pair in value.as_bytes().chunks_exact(2) {
        let high = hex_digit(pair[0]).ok_or(OfflineBackupError::InvalidManifest)?;
        let low = hex_digit(pair[1]).ok_or(OfflineBackupError::InvalidManifest)?;
        bytes.push((high << 4) | low);
    }
    Ok(bytes)
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

fn sync_directory(path: &Path) -> Result<(), io::Error> {
    File::open(path)?.sync_all()
}
