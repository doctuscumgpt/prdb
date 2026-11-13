//! Local database initialization and one-time administrator credential handoff.

use crate::audit::{AuditAction, AuditEvent, AuditResult};
use crate::catalog::{
    CatalogField, CatalogRecord, WireType, decode_registered_envelope, encode_envelope,
    family_schema,
};
use crate::credentials::{
    TOKEN_SECRET_BYTES, decode_verifier_record, encode_new_verifier_record,
    encode_revoked_verifier_record, now_unix_micros,
};
use crate::ipc::{
    DispatchError, FrameError, IpcFrame, Operation, ServiceRole, authorize_operation, read_frame,
    write_frame,
};
use crate::metadata::{EngineMetadata, Lifecycle};
use crate::schema::{AUDIT, CATALOG, ENGINE_META, PRINCIPAL_NAMES, SCHEMA_NAMES, create_tables};
use crate::startup::{
    StartupError, validate_initializing_records, validate_persisted_records,
    validate_persisted_records_read_only,
};
use crate::storage_manager::{StorageManager, StorageManagerConfig, StorageManagerInitError};
use redb::{
    Database, Durability, ReadOnlyDatabase, ReadableDatabase, ReadableTable, ReadableTableMetadata,
};
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use zeroize::Zeroize;

const DATABASE_FILE: &str = "storage.redb";
const MIGRATION_STAGING_FILE: &str = ".prdb-migration.next";
const LOCK_FILE: &str = ".prdb.lock";
const MARKER_FILE: &str = ".prdb-bootstrap";
const SETUP_SOCKET_FILE: &str = ".prdb-setup.sock";
const MARKER: &[u8] = b"PRDB-LOCAL-SETUP-1\n";
const SETUP_IO_TIMEOUT: Duration = Duration::from_secs(60);
pub const SETUP_CREDENTIAL_OPERATION: u16 = Operation::BootstrapDeliverCredential as u16;
pub const SETUP_ACK_OPERATION: u16 = Operation::BootstrapAcknowledgeDelivery as u16;
const ADMIN_ROLE_ID: [u8; 16] = builtin_id(3);
const MAX_BOOTSTRAP_RECORDS: usize = 65_536;
type EncodedCatalogRecord = (Vec<u8>, Vec<u8>);

pub(crate) const fn database_admin_role_id() -> [u8; 16] {
    ADMIN_ROLE_ID
}

struct BootstrapIdentity {
    admin_user_id: [u8; 16],
    token_id: [u8; 16],
    verifier: CatalogRecord,
    verifier_bytes: Vec<u8>,
    name: String,
}

/// Failure while creating, resuming, acknowledging, or opening a local instance.
#[derive(Debug)]
pub enum BootstrapError {
    AlreadyExists,
    NotPrivate,
    InstanceLocked,
    InvalidRequest,
    NotInitializing,
    SecretNotTaken,
    InvalidHandoff,
    SetupChannel,
    UnsupportedPlatform,
    RandomnessUnavailable,
    InvalidCatalog,
    AuditCapacityExceeded,
    Storage,
    Io(std::io::Error),
    Startup(StartupError),
}

impl std::fmt::Display for BootstrapError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "local database setup failed: {self:?}")
    }
}

impl std::error::Error for BootstrapError {}

impl From<std::io::Error> for BootstrapError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

/// A setup session holds the database and exclusive instance lock until READY
/// is published or the session is dropped.
pub struct BootstrapSession {
    directory: PathBuf,
    database: Database,
    _lock: InstanceLock,
    admin_user_id: [u8; 16],
    token_id: [u8; 16],
    secret: OneTimeSecret,
    secret_taken: bool,
}

impl BootstrapSession {
    pub fn admin_user_id(&self) -> [u8; 16] {
        self.admin_user_id
    }

    pub fn token_id(&self) -> [u8; 16] {
        self.token_id
    }

    /// Transfer the bootstrap secret once. An untaken secret is zeroized on drop.
    pub fn take_secret(&mut self) -> Option<[u8; TOKEN_SECRET_BYTES]> {
        let secret = self.secret.take();
        self.secret_taken |= secret.is_some();
        secret
    }

    /// Exposes the initializing database only to trusted local setup code.
    pub fn database(&self) -> &Database {
        &self.database
    }

    pub fn bind_setup_listener(&self) -> Result<BootstrapSetupListener, BootstrapError> {
        if self.secret_taken {
            return Err(BootstrapError::SecretNotTaken);
        }
        let path = self.directory.join(SETUP_SOCKET_FILE);
        let listener = UnixListener::bind(&path)?;
        let metadata = fs::symlink_metadata(&path)?;
        let setup_listener = BootstrapSetupListener {
            listener,
            path,
            directory: self.directory.clone(),
            device: metadata.dev(),
            inode: metadata.ino(),
            cleaned: false,
        };
        fs::set_permissions(&setup_listener.path, fs::Permissions::from_mode(0o600))?;
        validate_private_socket(&setup_listener.path)?;
        sync_directory(&self.directory)?;
        Ok(setup_listener)
    }

    fn send_handoff(&mut self, stream: &mut UnixStream) -> Result<(), BootstrapError> {
        authorize_operation(
            ServiceRole::Bootstrap,
            Operation::BootstrapDeliverCredential,
        )
        .map_err(map_dispatch_error)?;
        let request_id = random_request_id()?;
        let mut secret = self.take_secret().ok_or(BootstrapError::SecretNotTaken)?;
        let mut payload = Vec::with_capacity(16 + TOKEN_SECRET_BYTES);
        payload.extend_from_slice(&self.token_id);
        payload.extend_from_slice(&secret);
        secret.zeroize();
        let mut frame = IpcFrame {
            operation: SETUP_CREDENTIAL_OPERATION,
            request_id,
            payload,
        };
        let write_result = write_frame(stream, &frame).map_err(map_frame_error);
        frame.payload.zeroize();
        write_result?;
        let acknowledgement = read_frame(stream)
            .map_err(map_frame_error)?
            .ok_or(BootstrapError::InvalidHandoff)?;
        let acknowledgement_operation =
            Operation::from_code(acknowledgement.operation).map_err(map_dispatch_error)?;
        authorize_operation(ServiceRole::Bootstrap, acknowledgement_operation)
            .map_err(map_dispatch_error)?;
        if acknowledgement.operation != SETUP_ACK_OPERATION
            || acknowledgement.request_id != request_id
            || !acknowledgement.payload.is_empty()
        {
            return Err(BootstrapError::InvalidHandoff);
        }
        Ok(())
    }

    /// Publish READY after the caller has taken the secret and completed
    /// protected delivery. The returned instance continues to hold the lock.
    pub fn acknowledge(self) -> Result<StorageInstance, BootstrapError> {
        if !self.secret_taken {
            return Err(BootstrapError::SecretNotTaken);
        }
        validate_initializing_records(&self.database).map_err(BootstrapError::Startup)?;
        publish_ready(&self.database, self.admin_user_id, self.token_id)?;
        validate_persisted_records(&self.database).map_err(BootstrapError::Startup)?;
        let database_path = self.directory.join(DATABASE_FILE);
        Ok(StorageInstance {
            database: Box::new(self.database),
            database_path,
            _lock: self._lock,
        })
    }
}

pub struct BootstrapSetupListener {
    listener: UnixListener,
    path: PathBuf,
    directory: PathBuf,
    device: u64,
    inode: u64,
    cleaned: bool,
}

impl BootstrapSetupListener {
    pub fn socket_path(&self) -> &Path {
        &self.path
    }

    pub fn accept_and_acknowledge(
        mut self,
        mut session: BootstrapSession,
    ) -> Result<StorageInstance, BootstrapError> {
        if self.directory != session.directory {
            let _ = self.cleanup();
            return Err(BootstrapError::InvalidRequest);
        }
        let handoff_result = (|| {
            let (mut stream, _) = self.listener.accept()?;
            validate_setup_peer(&stream)?;
            stream.set_read_timeout(Some(SETUP_IO_TIMEOUT))?;
            stream.set_write_timeout(Some(SETUP_IO_TIMEOUT))?;
            session.send_handoff(&mut stream)
        })();
        let cleanup_result = self.cleanup();
        handoff_result?;
        cleanup_result?;
        session.acknowledge()
    }

    fn cleanup(&mut self) -> Result<(), BootstrapError> {
        if self.cleaned {
            return Ok(());
        }
        match fs::symlink_metadata(&self.path) {
            Ok(metadata)
                if metadata.file_type().is_socket()
                    && metadata.dev() == self.device
                    && metadata.ino() == self.inode =>
            {
                fs::remove_file(&self.path)?;
                sync_directory(&self.directory)?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            _ => return Err(BootstrapError::NotPrivate),
        }
        self.cleaned = true;
        Ok(())
    }
}

impl Drop for BootstrapSetupListener {
    fn drop(&mut self) {
        if !self.cleaned {
            let matches_bound_socket = fs::symlink_metadata(&self.path).is_ok_and(|metadata| {
                metadata.file_type().is_socket()
                    && metadata.dev() == self.device
                    && metadata.ino() == self.inode
            });
            if matches_bound_socket {
                let _ = fs::remove_file(&self.path);
                let _ = sync_directory(&self.directory);
            }
        }
    }
}

/// A client-side bootstrap handoff. The secret is zeroized if it is not taken.
pub struct BootstrapHandoff {
    token_id: [u8; 16],
    request_id: u64,
    secret: Option<[u8; TOKEN_SECRET_BYTES]>,
}

impl BootstrapHandoff {
    pub fn token_id(&self) -> [u8; 16] {
        self.token_id
    }

    pub fn take_secret(&mut self) -> Option<[u8; TOKEN_SECRET_BYTES]> {
        let secret = *self.secret.as_ref()?;
        if let Some(secret) = &mut self.secret {
            secret.zeroize();
        }
        self.secret = None;
        Some(secret)
    }

    /// Confirm that the caller received the secret, then acknowledge its frame.
    pub fn acknowledge_delivery<W: Write>(self, writer: &mut W) -> Result<(), BootstrapError> {
        if self.secret.is_some() {
            return Err(BootstrapError::SecretNotTaken);
        }
        authorize_operation(
            ServiceRole::Bootstrap,
            Operation::BootstrapAcknowledgeDelivery,
        )
        .map_err(map_dispatch_error)?;
        write_frame(
            writer,
            &IpcFrame {
                operation: SETUP_ACK_OPERATION,
                request_id: self.request_id,
                payload: Vec::new(),
            },
        )
        .map_err(map_frame_error)
    }
}

impl Drop for BootstrapHandoff {
    fn drop(&mut self) {
        if let Some(secret) = &mut self.secret {
            secret.zeroize();
        }
    }
}

/// Receive the one-time token credential from the protected local setup socket.
pub fn receive_bootstrap_handoff<R: Read>(
    reader: &mut R,
) -> Result<BootstrapHandoff, BootstrapError> {
    let mut frame = read_frame(reader)
        .map_err(map_frame_error)?
        .ok_or(BootstrapError::InvalidHandoff)?;
    if frame.operation != SETUP_CREDENTIAL_OPERATION
        || frame.request_id == 0
        || frame.payload.len() != 16 + TOKEN_SECRET_BYTES
    {
        frame.payload.zeroize();
        return Err(BootstrapError::InvalidHandoff);
    }
    let mut token_id = [0; 16];
    token_id.copy_from_slice(&frame.payload[..16]);
    let mut secret = [0; TOKEN_SECRET_BYTES];
    secret.copy_from_slice(&frame.payload[16..]);
    frame.payload.zeroize();
    Ok(BootstrapHandoff {
        token_id,
        request_id: frame.request_id,
        secret: Some(secret),
    })
}

struct OneTimeSecret(Option<[u8; TOKEN_SECRET_BYTES]>);

impl OneTimeSecret {
    fn take(&mut self) -> Option<[u8; TOKEN_SECRET_BYTES]> {
        let output = *self.0.as_ref()?;
        if let Some(secret) = &mut self.0 {
            secret.zeroize();
        }
        self.0 = None;
        Some(output)
    }
}

impl Drop for OneTimeSecret {
    fn drop(&mut self) {
        if let Some(secret) = &mut self.0 {
            secret.zeroize();
        }
    }
}

/// An opened READY instance. Keep this value alive for as long as any
/// `StorageManager` borrows its database handle.
pub struct StorageInstance {
    database: Box<Database>,
    database_path: PathBuf,
    _lock: InstanceLock,
}

/// An integrity-validated READY instance opened without redb writes. The
/// exclusive instance lock prevents a cooperating service from changing the
/// image during inspection.
pub struct ReadOnlyStorageInstance {
    database: ReadOnlyDatabase,
    _lock: InstanceLock,
}

impl ReadOnlyStorageInstance {
    pub fn open(directory: impl AsRef<Path>) -> Result<Self, BootstrapError> {
        let directory = directory.as_ref();
        validate_managed_directory(directory, true)?;
        let lock = InstanceLock::acquire(directory)?;
        if directory.join(SETUP_SOCKET_FILE).try_exists()? {
            return Err(BootstrapError::InvalidRequest);
        }
        validate_managed_directory(directory, true)?;
        let database_path = directory.join(DATABASE_FILE);
        validate_private_file(&database_path)?;
        let database =
            ReadOnlyDatabase::open(&database_path).map_err(|_| BootstrapError::Storage)?;
        validate_persisted_records_read_only(&database).map_err(BootstrapError::Startup)?;
        Ok(Self {
            database,
            _lock: lock,
        })
    }

    pub fn database(&self) -> &ReadOnlyDatabase {
        &self.database
    }
}

#[cfg(target_os = "linux")]
fn exchange_files(first: &Path, second: &Path) -> Result<(), BootstrapError> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let first =
        CString::new(first.as_os_str().as_bytes()).map_err(|_| BootstrapError::InvalidRequest)?;
    let second =
        CString::new(second.as_os_str().as_bytes()).map_err(|_| BootstrapError::InvalidRequest)?;
    // SAFETY: both path strings are NUL-terminated. The caller holds the
    // instance lock; RENAME_EXCHANGE atomically swaps the two file names.
    let result = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            first.as_ptr(),
            libc::AT_FDCWD,
            second.as_ptr(),
            libc::RENAME_EXCHANGE,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(BootstrapError::Io(std::io::Error::last_os_error()))
    }
}

#[cfg(not(target_os = "linux"))]
fn exchange_files(_first: &Path, _second: &Path) -> Result<(), BootstrapError> {
    Err(BootstrapError::UnsupportedPlatform)
}

#[derive(Debug)]
pub enum AuditArchiveError {
    InvalidOutputPath,
    InvalidArchive,
    ConcurrentChange,
    Storage,
    Io(std::io::Error),
}

impl std::fmt::Display for AuditArchiveError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidOutputPath => formatter.write_str("invalid audit archive output path"),
            Self::InvalidArchive => formatter.write_str(
                "existing audit archive is invalid or does not match the current history",
            ),
            Self::ConcurrentChange => {
                formatter.write_str("audit history changed during offline archival")
            }
            Self::Storage => formatter.write_str("audit archive storage operation failed"),
            Self::Io(error) => write!(formatter, "audit archive I/O failed: {error}"),
        }
    }
}

impl std::error::Error for AuditArchiveError {}

impl StorageInstance {
    /// Open and validate a READY instance while holding its exclusive lock.
    pub fn open(directory: impl AsRef<Path>) -> Result<Self, BootstrapError> {
        let directory = directory.as_ref();
        validate_managed_directory(directory, true)?;
        let lock = InstanceLock::acquire(directory)?;
        remove_stale_setup_socket(directory)?;
        validate_managed_directory(directory, true)?;
        let database_path = directory.join(DATABASE_FILE);
        validate_private_file(&database_path)?;
        let database = Database::open(&database_path).map_err(|_| BootstrapError::Storage)?;
        validate_persisted_records(&database).map_err(BootstrapError::Startup)?;
        Ok(Self {
            database: Box::new(database),
            database_path,
            _lock: lock,
        })
    }

    pub fn database(&self) -> &Database {
        &self.database
    }

    /// Validate a verified source backup and staged image, then atomically
    /// publish the staged database. On success, the former active image
    /// remains at `.prdb-migration.next`.
    #[cfg(target_os = "linux")]
    pub fn publish_staged_generation(
        self,
        verified_backup_directory: impl AsRef<Path>,
    ) -> Result<Self, BootstrapError> {
        let Self {
            database,
            database_path,
            _lock,
        } = self;
        let database_path = database_path.canonicalize()?;
        let directory = database_path
            .parent()
            .ok_or(BootstrapError::InvalidRequest)?
            .to_path_buf();
        let staging_path = directory.join(MIGRATION_STAGING_FILE);
        validate_private_file(&staging_path)?;

        let source_report =
            validate_persisted_records(&database).map_err(BootstrapError::Startup)?;
        let backup = crate::offline_backup::verify_backup(verified_backup_directory)
            .map_err(|_| BootstrapError::InvalidRequest)?;
        if source_report.metadata.database_id != backup.database_id
            || source_report.metadata.commit_sequence != backup.commit_sequence
        {
            return Err(BootstrapError::InvalidRequest);
        }
        let staged_database =
            ReadOnlyDatabase::open(&staging_path).map_err(|_| BootstrapError::Storage)?;
        let staged_report = validate_persisted_records_read_only(&staged_database)
            .map_err(BootstrapError::Startup)?;
        if source_report.metadata.database_id != staged_report.metadata.database_id
            || source_report.metadata.commit_sequence != staged_report.metadata.commit_sequence
        {
            return Err(BootstrapError::InvalidRequest);
        }
        drop(staged_database);
        drop(database);

        File::open(&staging_path)?.sync_all()?;
        File::open(&database_path)?.sync_all()?;
        exchange_files(&database_path, &staging_path)?;
        sync_directory(&directory)?;

        validate_private_file(&database_path)?;
        validate_private_file(&staging_path)?;
        let database = Database::open(&database_path).map_err(|_| BootstrapError::Storage)?;
        let published_report =
            validate_persisted_records(&database).map_err(BootstrapError::Startup)?;
        if published_report.metadata.database_id != source_report.metadata.database_id
            || published_report.metadata.commit_sequence != source_report.metadata.commit_sequence
        {
            return Err(BootstrapError::InvalidCatalog);
        }
        Ok(Self {
            database: Box::new(database),
            database_path,
            _lock,
        })
    }

    pub(crate) fn close_database_and<T, E>(
        self,
        operation: impl FnOnce(&Path) -> Result<T, E>,
    ) -> Result<T, E> {
        let Self {
            database,
            database_path,
            _lock,
        } = self;
        let directory = database_path.parent().unwrap_or_else(|| Path::new("."));
        drop(database);
        let _lock = _lock;
        operation(directory)
    }

    /// Export the complete bounded audit table to a private checksummed file,
    /// make that file and its directory entry durable, then remove the exact
    /// exported records in one redb transaction. An existing matching archive
    /// permits retry after a crash between export and database pruning.
    pub fn archive_audit_events(
        &mut self,
        output_path: impl AsRef<Path>,
    ) -> Result<usize, AuditArchiveError> {
        let output_path = output_path.as_ref();
        let output_name = output_path
            .file_name()
            .filter(|name| !name.is_empty())
            .ok_or(AuditArchiveError::InvalidOutputPath)?;
        let output_parent = output_path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let canonical_parent = output_parent
            .canonicalize()
            .map_err(AuditArchiveError::Io)?;
        let instance_directory = self
            .database_path
            .parent()
            .ok_or(AuditArchiveError::InvalidOutputPath)?
            .canonicalize()
            .map_err(AuditArchiveError::Io)?;
        if canonical_parent.starts_with(instance_directory) {
            return Err(AuditArchiveError::InvalidOutputPath);
        }
        let output_path = canonical_parent.join(output_name);

        let events = read_audit_archive_records(&self.database)?;
        if events.is_empty() {
            return Ok(0);
        }
        let encoded =
            crate::audit_archive::encode(&events).map_err(|_| AuditArchiveError::Storage)?;
        if output_path.exists() {
            let existing = read_bounded_archive(&output_path)?;
            let archived = crate::audit_archive::decode(&existing)
                .map_err(|_| AuditArchiveError::InvalidArchive)?;
            if archived != events {
                return Err(AuditArchiveError::InvalidArchive);
            }
            sync_archive_directory(&canonical_parent).map_err(AuditArchiveError::Io)?;
        } else {
            write_archive_without_overwrite(&canonical_parent, &output_path, &encoded)?;
            sync_archive_directory(&canonical_parent).map_err(AuditArchiveError::Io)?;
        }

        let write = self
            .database
            .begin_write()
            .map_err(|_| AuditArchiveError::Storage)?;
        {
            let mut table = write
                .open_table(AUDIT)
                .map_err(|_| AuditArchiveError::Storage)?;
            if read_audit_records_from_table(&table)? != events {
                return Err(AuditArchiveError::ConcurrentChange);
            }
            let event_ids = events
                .iter()
                .map(|(event_id, _)| *event_id)
                .collect::<Vec<_>>();
            let mut stats = write
                .open_table(crate::schema::STATS)
                .map_err(|_| AuditArchiveError::Storage)?;
            crate::audit_sink::remove_delivery_markers(&mut stats, &event_ids)
                .map_err(|_| AuditArchiveError::Storage)?;
            for (event_id, _) in &events {
                table
                    .remove(event_id.as_slice())
                    .map_err(|_| AuditArchiveError::Storage)?;
            }
        }
        write.commit().map_err(|_| AuditArchiveError::Storage)?;
        Ok(events.len())
    }

    /// Make one explicit attempt to reclaim redb pages while this handle is
    /// exclusively borrowed. Storage managers and transactions borrowing the
    /// instance must be dropped before maintenance can run.
    pub fn compact(&mut self) -> Result<bool, redb::CompactionError> {
        self.database.compact()
    }

    /// Close and reopen the redb handle while retaining the instance lock,
    /// then validate the recovered READY image before returning it. Callers
    /// must drop every `StorageManager` borrowing this instance first.
    pub fn reopen(self) -> Result<Self, BootstrapError> {
        let closed_database_key = &*self.database as *const Database as usize;
        let Self {
            database,
            database_path,
            _lock,
        } = self;
        let directory = database_path
            .parent()
            .ok_or(BootstrapError::InvalidRequest)?;
        validate_managed_directory(directory, true)?;
        drop(database);
        validate_managed_directory(directory, true)?;
        validate_private_file(&database_path)?;
        let database = Database::open(&database_path).map_err(|_| BootstrapError::Storage)?;
        validate_persisted_records(&database).map_err(BootstrapError::Startup)?;
        crate::storage_manager::clear_uncertain_database_key(closed_database_key);
        Ok(Self {
            database: Box::new(database),
            database_path,
            _lock,
        })
    }

    /// Build a service manager from this already validated, locked instance.
    pub fn storage_manager(
        &self,
        config: StorageManagerConfig,
    ) -> Result<StorageManager<'_>, StorageManagerInitError> {
        StorageManager::from_validated_database(&self.database, config)
    }

    /// Prune expired durable outcomes in a bounded immediate redb transaction.
    /// `now_us` must come from the trusted service clock.
    pub fn prune_expired_outcomes(
        &self,
        now_us: i64,
        max_scanned: usize,
    ) -> Result<crate::outcome::OutcomePruneResult, crate::outcome::OutcomeMaintenanceError> {
        crate::outcome::prune_expired_outcomes(&self.database, now_us, max_scanned)
    }
}

fn read_audit_archive_records(
    database: &Database,
) -> Result<Vec<crate::audit_archive::AuditArchiveRecord>, AuditArchiveError> {
    let read = database
        .begin_read()
        .map_err(|_| AuditArchiveError::Storage)?;
    let table = read
        .open_table(AUDIT)
        .map_err(|_| AuditArchiveError::Storage)?;
    if table.len().map_err(|_| AuditArchiveError::Storage)? > crate::audit::MAX_AUDIT_EVENTS as u64
    {
        return Err(AuditArchiveError::Storage);
    }
    read_audit_records_from_table(&table)
}

fn read_audit_records_from_table<T: redb::ReadableTable<&'static [u8], &'static [u8]>>(
    table: &T,
) -> Result<Vec<crate::audit_archive::AuditArchiveRecord>, AuditArchiveError> {
    let mut events = Vec::new();
    for entry in table.iter().map_err(|_| AuditArchiveError::Storage)? {
        let (key, value) = entry.map_err(|_| AuditArchiveError::Storage)?;
        let key: [u8; 16] = key
            .value()
            .try_into()
            .map_err(|_| AuditArchiveError::Storage)?;
        let value = value.value().to_vec();
        let decoded = AuditEvent::decode(&value).map_err(|_| AuditArchiveError::Storage)?;
        if decoded.event_id != key
            || events
                .last()
                .is_some_and(|(previous, _): &([u8; 16], Vec<u8>)| *previous >= key)
        {
            return Err(AuditArchiveError::Storage);
        }
        events.push((key, value));
        if events.len() > crate::audit::MAX_AUDIT_EVENTS {
            return Err(AuditArchiveError::Storage);
        }
    }
    Ok(events)
}

fn read_bounded_archive(path: &Path) -> Result<Vec<u8>, AuditArchiveError> {
    let metadata = fs::symlink_metadata(path).map_err(AuditArchiveError::Io)?;
    if !metadata.file_type().is_file()
        || metadata.len() > crate::audit_archive::MAX_ARCHIVE_LENGTH as u64
        || metadata.permissions().mode() & 0o7777 != 0o600
    {
        return Err(AuditArchiveError::InvalidArchive);
    }
    fs::read(path).map_err(AuditArchiveError::Io)
}

fn write_archive_without_overwrite(
    directory: &Path,
    output_path: &Path,
    bytes: &[u8],
) -> Result<(), AuditArchiveError> {
    let mut temp_file = None;
    let mut temp_path = None;
    for _ in 0..16 {
        let mut random = [0u8; 16];
        getrandom::fill(&mut random).map_err(|_| AuditArchiveError::Storage)?;
        let suffix = random
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let mut temp_name = std::ffi::OsString::from(".prdb-audit-");
        temp_name.push(suffix);
        temp_name.push(".tmp");
        let candidate = directory.join(temp_name);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
            .open(&candidate)
        {
            Ok(file) => {
                temp_path = Some(candidate);
                temp_file = Some(file);
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(AuditArchiveError::Io(error)),
        }
    }
    let temp_path = temp_path.ok_or(AuditArchiveError::Storage)?;
    let result = (|| {
        let mut file = temp_file.ok_or(AuditArchiveError::Storage)?;
        file.write_all(bytes).map_err(AuditArchiveError::Io)?;
        file.sync_all().map_err(AuditArchiveError::Io)?;
        drop(file);
        match fs::hard_link(&temp_path, output_path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let existing = read_bounded_archive(output_path)?;
                if existing != bytes {
                    return Err(AuditArchiveError::InvalidArchive);
                }
            }
            Err(error) => return Err(AuditArchiveError::Io(error)),
        }
        fs::remove_file(&temp_path).map_err(AuditArchiveError::Io)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result
}

fn sync_archive_directory(directory: &Path) -> Result<(), std::io::Error> {
    File::open(directory)?.sync_all()
}

/// Create a new local database in a previously absent directory.
///
/// The name must be a normalized lowercase ASCII identifier, and expiry is an
/// absolute future Unix timestamp in microseconds.
pub fn initialize(
    directory: impl AsRef<Path>,
    administrator_name: &str,
    expires_at_us: i64,
) -> Result<BootstrapSession, BootstrapError> {
    validate_administrator_name(administrator_name)?;
    validate_expiry(expires_at_us)?;
    let directory = directory.as_ref();
    let mut directory_builder = fs::DirBuilder::new();
    directory_builder.mode(0o700);
    directory_builder.create(directory).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            BootstrapError::AlreadyExists
        } else {
            BootstrapError::Io(error)
        }
    })?;
    fs::set_permissions(directory, fs::Permissions::from_mode(0o700))?;
    sync_parent_directory(directory)?;
    let lock = InstanceLock::acquire(directory)?;
    write_marker(directory)?;
    sync_directory(directory)?;
    let database_path = directory.join(DATABASE_FILE);
    let database = Database::create(&database_path).map_err(|_| BootstrapError::Storage)?;
    set_private_file_mode(&database_path)?;
    sync_directory(directory)?;
    let (admin_user_id, token_id, secret) =
        seed_database(&database, administrator_name, expires_at_us)?;
    validate_initializing_records(&database).map_err(BootstrapError::Startup)?;
    Ok(BootstrapSession {
        directory: directory.to_path_buf(),
        database,
        _lock: lock,
        admin_user_id,
        token_id,
        secret,
        secret_taken: false,
    })
}

/// Resume a valid INITIALIZING image, or recover a private empty directory left
/// before the setup marker/database was created. The name must match an image
/// that already contains the initial administrator.
pub fn resume(
    directory: impl AsRef<Path>,
    administrator_name: &str,
    expires_at_us: i64,
) -> Result<BootstrapSession, BootstrapError> {
    validate_administrator_name(administrator_name)?;
    validate_expiry(expires_at_us)?;
    let directory = directory.as_ref();
    if directory.join(MARKER_FILE).try_exists()? {
        validate_managed_directory(directory, false)?;
    } else {
        validate_unmarked_setup_directory(directory)?;
    }
    let lock = InstanceLock::acquire(directory)?;
    remove_stale_setup_socket(directory)?;
    if !directory.join(MARKER_FILE).try_exists()? {
        validate_unmarked_setup_directory(directory)?;
        write_marker(directory)?;
        sync_directory(directory)?;
    }
    validate_managed_directory(directory, false)?;
    let database_path = directory.join(DATABASE_FILE);
    let database = if database_path.exists() {
        validate_private_file(&database_path)?;
        Database::open(&database_path).map_err(|_| BootstrapError::Storage)?
    } else {
        Database::create(&database_path).map_err(|_| BootstrapError::Storage)?
    };
    set_private_file_mode(&database_path)?;
    sync_directory(directory)?;

    if database_is_empty(&database)? {
        let (admin_user_id, token_id, secret) =
            seed_database(&database, administrator_name, expires_at_us)?;
        validate_initializing_records(&database).map_err(BootstrapError::Startup)?;
        return Ok(BootstrapSession {
            directory: directory.to_path_buf(),
            database,
            _lock: lock,
            admin_user_id,
            token_id,
            secret,
            secret_taken: false,
        });
    }

    validate_initializing_records(&database).map_err(|error| match error {
        StartupError::NotReady => BootstrapError::NotInitializing,
        other => BootstrapError::Startup(other),
    })?;
    let now_us = now_unix_micros().ok_or(BootstrapError::InvalidRequest)?;
    let identity = load_bootstrap_identity(&database)?;
    if identity.name != administrator_name {
        return Err(BootstrapError::InvalidRequest);
    }
    let admin_user_id = identity.admin_user_id;
    let old_token_id = identity.token_id;
    let mut secret = OneTimeSecret(Some([0; TOKEN_SECRET_BYTES]));
    let (token_id, verifier_payload) = issue_replacement_token(
        &database,
        admin_user_id,
        secret.0.as_mut().ok_or(BootstrapError::Storage)?,
        now_us,
        expires_at_us,
    )?;
    let revoked_payload = encode_revoked_verifier_record(&identity.verifier, now_us)
        .map_err(|_| BootstrapError::InvalidCatalog)?;
    let mut audit_event = new_bootstrap_audit(now_us, vec![admin_user_id, old_token_id, token_id])?;
    audit_event.target_ids.sort_unstable();
    audit_event.target_ids.dedup();
    let metadata = read_metadata(&database)?;
    let next_sequence = metadata
        .commit_sequence
        .checked_add(1)
        .ok_or(BootstrapError::Storage)?;
    let current_metadata = metadata.encode().map_err(|_| BootstrapError::Storage)?;
    let next_metadata = EngineMetadata {
        commit_sequence: next_sequence,
        ..metadata
    }
    .encode()
    .map_err(|_| BootstrapError::Storage)?;
    let old_key = credential_key(old_token_id);
    let new_key = credential_key(token_id);
    let event_key = audit_event.event_id;
    let event_payload = audit_event
        .encode()
        .map_err(|_| BootstrapError::InvalidCatalog)?;
    let mut write = database
        .begin_write()
        .map_err(|_| BootstrapError::Storage)?;
    write
        .set_durability(Durability::Immediate)
        .map_err(|_| BootstrapError::Storage)?;
    write.set_two_phase_commit(true);
    {
        let mut catalog = write
            .open_table(CATALOG)
            .map_err(|_| BootstrapError::Storage)?;
        let current_old = catalog
            .get(old_key.as_slice())
            .map_err(|_| BootstrapError::Storage)?
            .map(|value| value.value().to_vec());
        let new_exists = catalog
            .get(new_key.as_slice())
            .map_err(|_| BootstrapError::Storage)?
            .is_some();
        if current_old.as_deref() != Some(identity.verifier_bytes.as_slice()) || new_exists {
            return Err(BootstrapError::InvalidCatalog);
        }
        catalog
            .insert(old_key.as_slice(), revoked_payload.as_slice())
            .map_err(|_| BootstrapError::Storage)?;
        catalog
            .insert(new_key.as_slice(), verifier_payload.as_slice())
            .map_err(|_| BootstrapError::Storage)?;
    }
    {
        let mut audit = write
            .open_table(AUDIT)
            .map_err(|_| BootstrapError::Storage)?;
        let current_events = usize::try_from(audit.len().map_err(|_| BootstrapError::Storage)?)
            .map_err(|_| BootstrapError::AuditCapacityExceeded)?;
        if !crate::audit::has_audit_capacity(current_events, 1) {
            return Err(BootstrapError::AuditCapacityExceeded);
        }
        if audit
            .get(event_key.as_slice())
            .map_err(|_| BootstrapError::Storage)?
            .is_some()
        {
            return Err(BootstrapError::RandomnessUnavailable);
        }
        audit
            .insert(event_key.as_slice(), event_payload.as_slice())
            .map_err(|_| BootstrapError::Storage)?;
    }
    {
        let mut engine_meta = write
            .open_table(ENGINE_META)
            .map_err(|_| BootstrapError::Storage)?;
        let persisted = engine_meta
            .get(&b"format"[..])
            .map_err(|_| BootstrapError::Storage)?
            .map(|value| value.value().to_vec());
        if persisted.as_deref() != Some(current_metadata.as_slice()) {
            return Err(BootstrapError::NotInitializing);
        }
        engine_meta
            .insert(&b"format"[..], next_metadata.as_slice())
            .map_err(|_| BootstrapError::Storage)?;
    }
    write.commit().map_err(|_| BootstrapError::Storage)?;
    validate_initializing_records(&database).map_err(BootstrapError::Startup)?;
    Ok(BootstrapSession {
        directory: directory.to_path_buf(),
        database,
        _lock: lock,
        admin_user_id,
        token_id,
        secret,
        secret_taken: false,
    })
}

struct InstanceLock {
    _file: File,
}

impl InstanceLock {
    fn acquire(directory: &Path) -> Result<Self, BootstrapError> {
        let path = directory.join(LOCK_FILE);
        let file = match OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
            .open(&path)
        {
            Ok(file) => {
                fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
                file.sync_all()?;
                sync_directory(directory)?;
                file
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                validate_private_file(&path)?;
                OpenOptions::new()
                    .read(true)
                    .write(true)
                    .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
                    .open(&path)?
            }
            Err(error) => return Err(BootstrapError::Io(error)),
        };
        validate_private_file(&path)?;
        // SAFETY: the file descriptor remains owned by `file` until the lock
        // wrapper is dropped; flock does not retain a pointer to Rust memory.
        let status = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if status != 0 {
            let error = std::io::Error::last_os_error();
            return if error.kind() == std::io::ErrorKind::WouldBlock {
                Err(BootstrapError::InstanceLocked)
            } else {
                Err(BootstrapError::Io(error))
            };
        }
        Ok(Self { _file: file })
    }
}

fn seed_database(
    database: &Database,
    administrator_name: &str,
    expires_at_us: i64,
) -> Result<([u8; 16], [u8; 16], OneTimeSecret), BootstrapError> {
    let now_us = now_unix_micros().ok_or(BootstrapError::InvalidRequest)?;
    validate_expiry_at(expires_at_us, now_us)?;
    let reserved = (1u16..=3)
        .chain(101u16..=111)
        .chain(201u16..=217)
        .map(builtin_id)
        .collect::<HashSet<_>>();
    let database_id = random_id(&reserved)?;
    let mut excluded = reserved;
    excluded.insert(database_id);
    let admin_user_id = random_id(&excluded)?;
    excluded.insert(admin_user_id);
    let mut material = [0u8; 16 + TOKEN_SECRET_BYTES];
    if getrandom::fill(&mut material).is_err() {
        material.zeroize();
        return Err(BootstrapError::RandomnessUnavailable);
    }
    let mut token_id = [0; 16];
    token_id.copy_from_slice(&material[..16]);
    let mut token_secret = [0; TOKEN_SECRET_BYTES];
    token_secret.copy_from_slice(&material[16..]);
    material.zeroize();
    if excluded.contains(&token_id) || token_id == [0; 16] {
        token_secret.zeroize();
        return Err(BootstrapError::RandomnessUnavailable);
    }
    let secret = OneTimeSecret(Some(token_secret));
    token_secret.zeroize();
    let verifier = encode_new_verifier_record(
        token_id,
        admin_user_id,
        secret.0.as_ref().ok_or(BootstrapError::Storage)?,
        now_us,
        expires_at_us,
        "bootstrap",
    )
    .map_err(|_| BootstrapError::InvalidCatalog)?;
    let database_record = database_record(database_id)?;
    let mut catalog_records =
        bootstrap_catalog_records(admin_user_id, administrator_name, token_id, verifier)?;
    catalog_records.push(database_record);
    let event = new_bootstrap_audit(now_us, vec![admin_user_id, token_id])?;
    let event_key = event.event_id;
    let event_payload = event.encode().map_err(|_| BootstrapError::InvalidCatalog)?;
    let metadata = EngineMetadata {
        database_id,
        catalog_version: 2,
        expression_version: 1,
        builtin_semantics_version: 1,
        commit_sequence: 1,
        lifecycle: Lifecycle::Initializing,
    }
    .encode()
    .map_err(|_| BootstrapError::Storage)?;

    let mut write = database
        .begin_write()
        .map_err(|_| BootstrapError::Storage)?;
    write
        .set_durability(Durability::Immediate)
        .map_err(|_| BootstrapError::Storage)?;
    write.set_two_phase_commit(true);
    create_tables(&write).map_err(|_| BootstrapError::Storage)?;
    {
        let mut principal_names = write
            .open_table(PRINCIPAL_NAMES)
            .map_err(|_| BootstrapError::Storage)?;
        for (key, payload) in &catalog_records {
            if !key.starts_with(&10u16.to_be_bytes()) {
                continue;
            }
            let principal_id = key.get(3..19).ok_or(BootstrapError::InvalidCatalog)?;
            let record =
                decode_registered_envelope(payload).map_err(|_| BootstrapError::InvalidCatalog)?;
            let name = record
                .fields
                .iter()
                .find(|field| field.tag == 2)
                .ok_or(BootstrapError::InvalidCatalog)?;
            if principal_names
                .insert(name.payload.as_slice(), principal_id)
                .map_err(|_| BootstrapError::Storage)?
                .is_some()
            {
                return Err(BootstrapError::InvalidCatalog);
            }
        }
    }
    {
        let mut schema_names = write
            .open_table(SCHEMA_NAMES)
            .map_err(|_| BootstrapError::Storage)?;
        for (key, payload) in &catalog_records {
            if !key.starts_with(&2u16.to_be_bytes()) {
                continue;
            }
            let record =
                decode_registered_envelope(payload).map_err(|_| BootstrapError::InvalidCatalog)?;
            let kind = record
                .fields
                .iter()
                .find(|field| field.tag == 1)
                .ok_or(BootstrapError::InvalidCatalog)?;
            if kind.payload.as_slice() != 1u16.to_le_bytes() {
                continue;
            }
            let name = record
                .fields
                .iter()
                .find(|field| field.tag == 3)
                .ok_or(BootstrapError::InvalidCatalog)?;
            let schema_id = key.get(3..19).ok_or(BootstrapError::InvalidCatalog)?;
            if schema_names
                .insert(name.payload.as_slice(), schema_id)
                .map_err(|_| BootstrapError::Storage)?
                .is_some()
            {
                return Err(BootstrapError::InvalidCatalog);
            }
        }
    }
    {
        let mut catalog = write
            .open_table(CATALOG)
            .map_err(|_| BootstrapError::Storage)?;
        for (key, payload) in catalog_records {
            catalog
                .insert(key.as_slice(), payload.as_slice())
                .map_err(|_| BootstrapError::Storage)?;
        }
    }
    {
        let mut audit = write
            .open_table(AUDIT)
            .map_err(|_| BootstrapError::Storage)?;
        let current_events = usize::try_from(audit.len().map_err(|_| BootstrapError::Storage)?)
            .map_err(|_| BootstrapError::AuditCapacityExceeded)?;
        if !crate::audit::has_audit_capacity(current_events, 1) {
            return Err(BootstrapError::AuditCapacityExceeded);
        }
        audit
            .insert(event_key.as_slice(), event_payload.as_slice())
            .map_err(|_| BootstrapError::Storage)?;
    }
    {
        let mut engine_meta = write
            .open_table(ENGINE_META)
            .map_err(|_| BootstrapError::Storage)?;
        engine_meta
            .insert(&b"format"[..], metadata.as_slice())
            .map_err(|_| BootstrapError::Storage)?;
    }
    write.commit().map_err(|_| BootstrapError::Storage)?;
    Ok((admin_user_id, token_id, secret))
}

fn bootstrap_catalog_records(
    admin_user_id: [u8; 16],
    administrator_name: &str,
    token_id: [u8; 16],
    verifier: Vec<u8>,
) -> Result<Vec<EncodedCatalogRecord>, BootstrapError> {
    let mut records = Vec::new();
    let owner = ADMIN_ROLE_ID;
    records.push((
        object_key(ADMIN_ROLE_ID),
        encode_record(
            2,
            &object_key(ADMIN_ROLE_ID),
            &object_fields(9, "database_admin", owner),
        )?,
    ));
    records.push((
        principal_key(ADMIN_ROLE_ID),
        encode_record(
            10,
            &principal_key(ADMIN_ROLE_ID),
            &principal_fields(2, "database_admin", false),
        )?,
    ));
    for (code, name) in [(1u16, "pg_catalog"), (2, "information_schema")] {
        let id = builtin_id(code);
        let key = object_key(id);
        records.push((
            key.clone(),
            encode_record(2, &key, &object_fields(1, name, owner))?,
        ));
        let key = id_key(3, id);
        let fields = [CatalogField {
            tag: 1,
            wire_type: WireType::Ids,
            payload: 0u32.to_le_bytes().to_vec(),
        }];
        records.push((key.clone(), encode_record(3, &key, &fields)?));
    }
    for kind in 1u16..=11 {
        let id = builtin_id(100 + kind);
        let type_key = id_key(4, id);
        let type_fields = builtin_type_fields(kind);
        records.push((type_key.clone(), encode_record(4, &type_key, &type_fields)?));
        let name = format!("builtin_type_{kind}");
        let key = object_key(id);
        records.push((
            key.clone(),
            encode_record(2, &key, &object_fields(2, &name, owner))?,
        ));
    }
    let user_object_key = object_key(admin_user_id);
    records.push((
        user_object_key.clone(),
        encode_record(
            2,
            &user_object_key,
            &object_fields(8, administrator_name, owner),
        )?,
    ));
    let user_principal_key = principal_key(admin_user_id);
    records.push((
        user_principal_key.clone(),
        encode_record(
            10,
            &user_principal_key,
            &principal_fields(1, administrator_name, true),
        )?,
    ));
    let membership_key = membership_key(admin_user_id, ADMIN_ROLE_ID);
    records.push((
        membership_key.clone(),
        encode_record(
            11,
            &membership_key,
            &[
                CatalogField {
                    tag: 1,
                    wire_type: WireType::Id,
                    payload: ADMIN_ROLE_ID.to_vec(),
                },
                CatalogField {
                    tag: 2,
                    wire_type: WireType::U64,
                    payload: 1u64.to_le_bytes().to_vec(),
                },
            ],
        )?,
    ));
    let dependency_key = dependency_key(admin_user_id, ADMIN_ROLE_ID, 5);
    records.push((
        dependency_key.clone(),
        encode_record(15, &dependency_key, &[])?,
    ));
    let token_key = credential_key(token_id);
    records.push((token_key, verifier));
    Ok(records)
}

fn database_record(database_id: [u8; 16]) -> Result<(Vec<u8>, Vec<u8>), BootstrapError> {
    let key = id_key(1, database_id);
    let mut builtins = 31u32.to_le_bytes().to_vec();
    for code in (1u16..=3).chain(101u16..=111).chain(201u16..=217) {
        builtins.extend_from_slice(&code.to_le_bytes());
        builtins.extend_from_slice(&0u16.to_le_bytes());
        builtins.extend_from_slice(&builtin_id(code));
    }
    let fields = [
        CatalogField {
            tag: 1,
            wire_type: WireType::Text,
            payload: b"database".to_vec(),
        },
        CatalogField {
            tag: 2,
            wire_type: WireType::U32,
            payload: 2u32.to_le_bytes().to_vec(),
        },
        CatalogField {
            tag: 3,
            wire_type: WireType::Bytes,
            payload: builtins,
        },
        CatalogField {
            tag: 5,
            wire_type: WireType::U32,
            payload: 1u32.to_le_bytes().to_vec(),
        },
        CatalogField {
            tag: 6,
            wire_type: WireType::U32,
            payload: 1u32.to_le_bytes().to_vec(),
        },
    ];
    Ok((key.clone(), encode_record(1, &key, &fields)?))
}

fn builtin_type_fields(kind: u16) -> Vec<CatalogField> {
    let parameter_rule = match kind {
        5 => 1u16,
        7 => 2,
        _ => 0,
    };
    let default_mask = if kind == 9 { 7u32 } else { 1 };
    vec![
        u16_field(1, kind),
        u16_field(2, parameter_rule),
        u32_field(3, 1),
        u32_field(4, 1),
        u32_field(5, default_mask),
        u32_field(6, 1),
    ]
}

fn object_fields(kind: u16, name: &str, owner_id: [u8; 16]) -> Vec<CatalogField> {
    vec![
        u16_field(1, kind),
        text_field(3, name),
        id_field(4, owner_id),
        u64_field(5, 1),
        u16_field(6, 1),
    ]
}

fn principal_fields(kind: u16, name: &str, login: bool) -> Vec<CatalogField> {
    vec![
        u16_field(1, kind),
        text_field(2, name),
        CatalogField {
            tag: 3,
            wire_type: WireType::Bool,
            payload: vec![u8::from(login)],
        },
        u64_field(4, 1),
    ]
}

fn load_bootstrap_identity(database: &Database) -> Result<BootstrapIdentity, BootstrapError> {
    let read = database.begin_read().map_err(|_| BootstrapError::Storage)?;
    let table = read
        .open_table(CATALOG)
        .map_err(|_| BootstrapError::Storage)?;
    let mut admin_user = None;
    let mut principal_count = 0usize;
    for entry in table
        .range(10u16.to_be_bytes().as_slice()..)
        .map_err(|_| BootstrapError::Storage)?
    {
        let (key, value) = entry.map_err(|_| BootstrapError::Storage)?;
        if !key.value().starts_with(&10u16.to_be_bytes()) {
            break;
        }
        principal_count += 1;
        if principal_count > MAX_BOOTSTRAP_RECORDS {
            return Err(BootstrapError::InvalidCatalog);
        }
        let record = decode_registered_envelope(value.value())
            .map_err(|_| BootstrapError::InvalidCatalog)?;
        if record.logical_key != key.value() {
            return Err(BootstrapError::InvalidCatalog);
        }
        let kind = field_u16(&record, 1)?;
        if kind == 1 {
            if admin_user.is_some() || !field_bool(&record, 3)? {
                return Err(BootstrapError::InvalidCatalog);
            }
            let id = id_from_key(key.value())?;
            let name = field_text(&record, 2)?.to_owned();
            admin_user = Some((id, name));
        }
    }
    if principal_count != 2 {
        return Err(BootstrapError::InvalidCatalog);
    }
    let (admin_user_id, name) = admin_user.ok_or(BootstrapError::InvalidCatalog)?;
    let expected_membership = membership_key(admin_user_id, ADMIN_ROLE_ID);
    if table
        .get(expected_membership.as_slice())
        .map_err(|_| BootstrapError::Storage)?
        .is_none()
    {
        return Err(BootstrapError::InvalidCatalog);
    }

    let prefix = 16u16.to_be_bytes();
    let mut active = None;
    let mut scanned = 0usize;
    for entry in table
        .range(prefix.as_slice()..)
        .map_err(|_| BootstrapError::Storage)?
    {
        let (key, value) = entry.map_err(|_| BootstrapError::Storage)?;
        if !key.value().starts_with(&prefix) {
            break;
        }
        scanned += 1;
        if scanned > MAX_BOOTSTRAP_RECORDS {
            return Err(BootstrapError::InvalidCatalog);
        }
        let record = decode_registered_envelope(value.value())
            .map_err(|_| BootstrapError::InvalidCatalog)?;
        let verifier =
            decode_verifier_record(&record).map_err(|_| BootstrapError::InvalidCatalog)?;
        if verifier.user_id != admin_user_id {
            return Err(BootstrapError::InvalidCatalog);
        }
        if !verifier.revoked {
            if active.is_some() {
                return Err(BootstrapError::InvalidCatalog);
            }
            active = Some((id_from_key(key.value())?, record, value.value().to_vec()));
        }
    }
    let (token_id, verifier, verifier_bytes) = active.ok_or(BootstrapError::InvalidCatalog)?;
    Ok(BootstrapIdentity {
        admin_user_id,
        token_id,
        verifier,
        verifier_bytes,
        name,
    })
}

fn issue_replacement_token(
    database: &Database,
    user_id: [u8; 16],
    secret: &mut [u8; TOKEN_SECRET_BYTES],
    issued_at_us: i64,
    expires_at_us: i64,
) -> Result<([u8; 16], Vec<u8>), BootstrapError> {
    let read = database.begin_read().map_err(|_| BootstrapError::Storage)?;
    let table = read
        .open_table(CATALOG)
        .map_err(|_| BootstrapError::Storage)?;
    for _ in 0..8 {
        let token_id = random_id(&HashSet::from([user_id, ADMIN_ROLE_ID]))?;
        let key = credential_key(token_id);
        if table
            .get(key.as_slice())
            .map_err(|_| BootstrapError::Storage)?
            .is_none()
        {
            if getrandom::fill(secret).is_err() {
                secret.zeroize();
                return Err(BootstrapError::RandomnessUnavailable);
            }
            let verifier = encode_new_verifier_record(
                token_id,
                user_id,
                secret,
                issued_at_us,
                expires_at_us,
                "bootstrap",
            )
            .map_err(|_| BootstrapError::InvalidCatalog)?;
            return Ok((token_id, verifier));
        }
    }
    Err(BootstrapError::RandomnessUnavailable)
}

fn new_bootstrap_audit(
    now_us: i64,
    mut target_ids: Vec<[u8; 16]>,
) -> Result<AuditEvent, BootstrapError> {
    let event_id = random_id(&HashSet::new())?;
    let correlation_id = random_id(&HashSet::from([event_id]))?;
    target_ids.sort_unstable();
    target_ids.dedup();
    Ok(AuditEvent {
        event_id,
        actor_id: None,
        correlation_id,
        occurred_at_us: now_us,
        action: AuditAction::TokenLifecycle,
        result: AuditResult::Succeeded,
        target_ids,
    })
}

fn publish_ready(
    database: &Database,
    user_id: [u8; 16],
    token_id: [u8; 16],
) -> Result<(), BootstrapError> {
    let metadata = read_metadata(database)?;
    if metadata.lifecycle != Lifecycle::Initializing {
        return Err(BootstrapError::NotInitializing);
    }
    let next_sequence = metadata
        .commit_sequence
        .checked_add(1)
        .ok_or(BootstrapError::Storage)?;
    let current_metadata = metadata.encode().map_err(|_| BootstrapError::Storage)?;
    let encoded = EngineMetadata {
        lifecycle: Lifecycle::Ready,
        commit_sequence: next_sequence,
        ..metadata
    }
    .encode()
    .map_err(|_| BootstrapError::Storage)?;
    let mut write = database
        .begin_write()
        .map_err(|_| BootstrapError::Storage)?;
    write
        .set_durability(Durability::Immediate)
        .map_err(|_| BootstrapError::Storage)?;
    write.set_two_phase_commit(true);
    let now_us = now_unix_micros().ok_or(BootstrapError::InvalidRequest)?;
    {
        let catalog = write
            .open_table(CATALOG)
            .map_err(|_| BootstrapError::Storage)?;
        let key = credential_key(token_id);
        let value = catalog
            .get(key.as_slice())
            .map_err(|_| BootstrapError::Storage)?
            .ok_or(BootstrapError::InvalidCatalog)?;
        let record = decode_registered_envelope(value.value())
            .map_err(|_| BootstrapError::InvalidCatalog)?;
        let verifier =
            decode_verifier_record(&record).map_err(|_| BootstrapError::InvalidCatalog)?;
        if verifier.user_id != user_id || !verifier.is_valid_at(now_us) {
            return Err(BootstrapError::InvalidCatalog);
        }
    }
    {
        let mut engine_meta = write
            .open_table(ENGINE_META)
            .map_err(|_| BootstrapError::Storage)?;
        let persisted = engine_meta
            .get(&b"format"[..])
            .map_err(|_| BootstrapError::Storage)?
            .map(|value| value.value().to_vec());
        if persisted.as_deref() != Some(current_metadata.as_slice()) {
            return Err(BootstrapError::NotInitializing);
        }
        engine_meta
            .insert(&b"format"[..], encoded.as_slice())
            .map_err(|_| BootstrapError::Storage)?;
    }
    write.commit().map_err(|_| BootstrapError::Storage)
}

fn read_metadata(database: &Database) -> Result<EngineMetadata, BootstrapError> {
    let read = database.begin_read().map_err(|_| BootstrapError::Storage)?;
    let table = read
        .open_table(ENGINE_META)
        .map_err(|_| BootstrapError::Storage)?;
    let value = table
        .get(&b"format"[..])
        .map_err(|_| BootstrapError::Storage)?
        .ok_or(BootstrapError::InvalidCatalog)?;
    EngineMetadata::decode(value.value()).map_err(|_| BootstrapError::InvalidCatalog)
}

fn database_is_empty(database: &Database) -> Result<bool, BootstrapError> {
    let read = database.begin_read().map_err(|_| BootstrapError::Storage)?;
    let tables = read.list_tables().map_err(|_| BootstrapError::Storage)?;
    Ok(tables.count() == 0)
}

fn encode_record(
    family_code: u16,
    key: &[u8],
    fields: &[CatalogField],
) -> Result<Vec<u8>, BootstrapError> {
    let schema = family_schema(family_code).ok_or(BootstrapError::InvalidCatalog)?;
    encode_envelope(&schema, key, fields).map_err(|_| BootstrapError::InvalidCatalog)
}

fn id_key(family_code: u16, id: [u8; 16]) -> Vec<u8> {
    let mut key = family_code.to_be_bytes().to_vec();
    key.push(1);
    key.extend_from_slice(&id);
    key
}

fn object_key(id: [u8; 16]) -> Vec<u8> {
    id_key(2, id)
}

fn principal_key(id: [u8; 16]) -> Vec<u8> {
    id_key(10, id)
}

fn credential_key(id: [u8; 16]) -> Vec<u8> {
    let mut key = 16u16.to_be_bytes().to_vec();
    key.push(1);
    key.extend_from_slice(&id);
    key
}

fn membership_key(member_id: [u8; 16], role_id: [u8; 16]) -> Vec<u8> {
    let mut key = 11u16.to_be_bytes().to_vec();
    key.push(1);
    key.extend_from_slice(&member_id);
    key.push(1);
    key.extend_from_slice(&role_id);
    key
}

fn dependency_key(dependent_id: [u8; 16], referenced_id: [u8; 16], kind: u16) -> Vec<u8> {
    let mut key = 15u16.to_be_bytes().to_vec();
    key.push(1);
    key.extend_from_slice(&dependent_id);
    key.push(1);
    key.extend_from_slice(&referenced_id);
    key.push(1);
    key.extend_from_slice(&kind.to_be_bytes());
    key
}

const fn builtin_id(code: u16) -> [u8; 16] {
    let mut id = [0x80; 16];
    id[15] = code as u8;
    id
}

fn id_from_key(key: &[u8]) -> Result<[u8; 16], BootstrapError> {
    key.get(3..19)
        .ok_or(BootstrapError::InvalidCatalog)?
        .try_into()
        .map_err(|_| BootstrapError::InvalidCatalog)
}

fn field(record: &CatalogRecord, tag: u16) -> Result<&[u8], BootstrapError> {
    record
        .fields
        .iter()
        .find(|field| field.tag == tag)
        .map(|field| field.payload.as_slice())
        .ok_or(BootstrapError::InvalidCatalog)
}

fn field_u16(record: &CatalogRecord, tag: u16) -> Result<u16, BootstrapError> {
    field(record, tag)?
        .try_into()
        .map(u16::from_le_bytes)
        .map_err(|_| BootstrapError::InvalidCatalog)
}

fn field_bool(record: &CatalogRecord, tag: u16) -> Result<bool, BootstrapError> {
    match field(record, tag)? {
        [0] => Ok(false),
        [1] => Ok(true),
        _ => Err(BootstrapError::InvalidCatalog),
    }
}

fn field_text(record: &CatalogRecord, tag: u16) -> Result<&str, BootstrapError> {
    std::str::from_utf8(field(record, tag)?).map_err(|_| BootstrapError::InvalidCatalog)
}

fn u16_field(tag: u16, value: u16) -> CatalogField {
    CatalogField {
        tag,
        wire_type: WireType::U16,
        payload: value.to_le_bytes().to_vec(),
    }
}

fn u32_field(tag: u16, value: u32) -> CatalogField {
    CatalogField {
        tag,
        wire_type: WireType::U32,
        payload: value.to_le_bytes().to_vec(),
    }
}

fn u64_field(tag: u16, value: u64) -> CatalogField {
    CatalogField {
        tag,
        wire_type: WireType::U64,
        payload: value.to_le_bytes().to_vec(),
    }
}

fn id_field(tag: u16, value: [u8; 16]) -> CatalogField {
    CatalogField {
        tag,
        wire_type: WireType::Id,
        payload: value.to_vec(),
    }
}

fn text_field(tag: u16, value: &str) -> CatalogField {
    CatalogField {
        tag,
        wire_type: WireType::Text,
        payload: value.as_bytes().to_vec(),
    }
}

fn random_id(excluded: &HashSet<[u8; 16]>) -> Result<[u8; 16], BootstrapError> {
    for _ in 0..8 {
        let mut id = [0; 16];
        if getrandom::fill(&mut id).is_err() {
            id.zeroize();
            return Err(BootstrapError::RandomnessUnavailable);
        }
        if id != [0; 16] && !excluded.contains(&id) {
            return Ok(id);
        }
        id.zeroize();
    }
    Err(BootstrapError::RandomnessUnavailable)
}

fn validate_administrator_name(name: &str) -> Result<(), BootstrapError> {
    let bytes = name.as_bytes();
    let first_is_valid = bytes
        .first()
        .is_some_and(|byte| byte.is_ascii_lowercase() || *byte == b'_');
    if bytes.is_empty()
        || bytes.len() > 128
        || !first_is_valid
        || name == "database_admin"
        || !bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
    {
        return Err(BootstrapError::InvalidRequest);
    }
    Ok(())
}

fn validate_expiry(expires_at_us: i64) -> Result<(), BootstrapError> {
    let now_us = now_unix_micros().ok_or(BootstrapError::InvalidRequest)?;
    validate_expiry_at(expires_at_us, now_us)
}

fn validate_expiry_at(expires_at_us: i64, now_us: i64) -> Result<(), BootstrapError> {
    if expires_at_us <= now_us {
        return Err(BootstrapError::InvalidRequest);
    }
    Ok(())
}

fn validate_managed_directory(
    directory: &Path,
    require_database: bool,
) -> Result<(), BootstrapError> {
    let metadata = fs::symlink_metadata(directory)?;
    if !metadata.file_type().is_dir()
        || metadata.uid() != effective_uid()
        || metadata.permissions().mode() & 0o7777 != 0o700
    {
        return Err(BootstrapError::NotPrivate);
    }
    validate_private_file(&directory.join(MARKER_FILE))?;
    let mut saw_database = false;
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        match entry.file_name().to_str() {
            Some(MARKER_FILE | LOCK_FILE) => {}
            Some(DATABASE_FILE) => {
                validate_private_file(&entry.path())?;
                saw_database = true;
            }
            Some(MIGRATION_STAGING_FILE) => validate_private_file(&entry.path())?,
            Some(SETUP_SOCKET_FILE) => validate_private_socket(&entry.path())?,
            _ => return Err(BootstrapError::NotPrivate),
        }
    }
    if require_database && !saw_database {
        return Err(BootstrapError::NotInitializing);
    }
    validate_marker(directory)?;
    Ok(())
}

fn validate_unmarked_setup_directory(directory: &Path) -> Result<(), BootstrapError> {
    let metadata = fs::symlink_metadata(directory)?;
    if !metadata.file_type().is_dir()
        || metadata.uid() != effective_uid()
        || metadata.permissions().mode() & 0o7777 != 0o700
    {
        return Err(BootstrapError::NotPrivate);
    }
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if entry.file_name() != LOCK_FILE {
            return Err(BootstrapError::NotPrivate);
        }
        validate_private_file(&entry.path())?;
    }
    Ok(())
}

fn validate_private_file(path: &Path) -> Result<(), BootstrapError> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_file()
        || metadata.uid() != effective_uid()
        || metadata.permissions().mode() & 0o7777 != 0o600
        || metadata.nlink() != 1
    {
        return Err(BootstrapError::NotPrivate);
    }
    Ok(())
}

fn validate_private_socket(path: &Path) -> Result<(), BootstrapError> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_socket()
        || metadata.uid() != effective_uid()
        || metadata.permissions().mode() & 0o7777 != 0o600
        || metadata.nlink() != 1
    {
        return Err(BootstrapError::NotPrivate);
    }
    Ok(())
}

fn remove_stale_setup_socket(directory: &Path) -> Result<(), BootstrapError> {
    let path = directory.join(SETUP_SOCKET_FILE);
    match fs::symlink_metadata(&path) {
        Ok(_) => {
            validate_private_socket(&path)?;
            fs::remove_file(path)?;
            sync_directory(directory)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(BootstrapError::Io(error)),
    }
    Ok(())
}

fn validate_setup_peer(stream: &UnixStream) -> Result<(), BootstrapError> {
    #[cfg(target_os = "linux")]
    {
        let mut credentials = libc::ucred {
            pid: 0,
            uid: u32::MAX,
            gid: 0,
        };
        let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        // SAFETY: `stream` owns a live socket descriptor, and `credentials` and
        // `length` point to initialized writable storage with the required sizes.
        let status = unsafe {
            libc::getsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                (&mut credentials as *mut libc::ucred).cast(),
                &mut length,
            )
        };
        if status != 0 {
            return Err(BootstrapError::Io(std::io::Error::last_os_error()));
        }
        if length as usize != std::mem::size_of::<libc::ucred>()
            || credentials.pid <= 0
            || credentials.uid != effective_uid()
        {
            return Err(BootstrapError::InvalidHandoff);
        }
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = stream;
        Err(BootstrapError::UnsupportedPlatform)
    }
}

fn random_request_id() -> Result<u64, BootstrapError> {
    for _ in 0..8 {
        let mut bytes = [0; 8];
        if getrandom::fill(&mut bytes).is_err() {
            bytes.zeroize();
            return Err(BootstrapError::RandomnessUnavailable);
        }
        let request_id = u64::from_be_bytes(bytes);
        bytes.zeroize();
        if request_id != 0 {
            return Ok(request_id);
        }
    }
    Err(BootstrapError::RandomnessUnavailable)
}

fn map_frame_error(_error: FrameError) -> BootstrapError {
    BootstrapError::SetupChannel
}

fn map_dispatch_error(_error: DispatchError) -> BootstrapError {
    BootstrapError::SetupChannel
}

fn write_marker(directory: &Path) -> Result<(), BootstrapError> {
    let path = directory.join(MARKER_FILE);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(path)?;
    fs::set_permissions(
        directory.join(MARKER_FILE),
        fs::Permissions::from_mode(0o600),
    )?;
    file.write_all(MARKER)?;
    file.sync_all()?;
    Ok(())
}

fn validate_marker(directory: &Path) -> Result<(), BootstrapError> {
    let path = directory.join(MARKER_FILE);
    validate_private_file(&path)?;
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(path)?;
    let mut contents = Vec::new();
    file.take((MARKER.len() + 1) as u64)
        .read_to_end(&mut contents)?;
    if contents != MARKER {
        return Err(BootstrapError::NotPrivate);
    }
    Ok(())
}

fn set_private_file_mode(path: &Path) -> Result<(), BootstrapError> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_file() || metadata.uid() != effective_uid() || metadata.nlink() != 1
    {
        return Err(BootstrapError::NotPrivate);
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    OpenOptions::new()
        .write(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(path)?
        .sync_all()?;
    validate_private_file(path)
}

fn sync_directory(directory: &Path) -> Result<(), BootstrapError> {
    File::open(directory)?.sync_all()?;
    Ok(())
}

fn sync_parent_directory(directory: &Path) -> Result<(), BootstrapError> {
    let parent = directory
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    File::open(parent)?.sync_all()?;
    Ok(())
}

fn effective_uid() -> u32 {
    // SAFETY: geteuid has no preconditions and does not read or write memory.
    unsafe { libc::geteuid() }
}
