//! Best-effort delivery of durable AUD1 records to a private JSONL file.
//!
//! The AUDIT table is the durable outbox. Each line is synced before a
//! delivered marker is committed to STATS, so a crash can cause a duplicate
//! line but cannot silently acknowledge a line that was not made durable.

use crate::audit::{AuditEvent, MAX_AUDIT_EVENTS};
use crate::schema::{AUDIT, STATS};
use redb::{Database, ReadableDatabase, ReadableTable, ReadableTableMetadata};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

const SINK_ID_KEY: &[u8] = b"audit_sink_id_v1";
const DELIVERED_PREFIX: &[u8] = b"audit_sink_delivered_v1:";
const MAX_BATCH: usize = 64;
const MAX_LINE_LENGTH: u64 = 16 * 1024;

static DELIVERY_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Debug)]
pub enum AuditSinkError {
    InvalidBatch,
    InvalidPath,
    InvalidSinkFile,
    Storage,
    Io(std::io::Error),
}

impl std::fmt::Display for AuditSinkError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "audit JSONL delivery failed: {self:?}")
    }
}

impl std::error::Error for AuditSinkError {}

impl From<std::io::Error> for AuditSinkError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

/// Offline diagnostic for the optional durable audit JSONL sink.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuditSinkStatus {
    pub configured: bool,
    pub pending_events: u64,
    pub oldest_pending_age_us: Option<u64>,
}

/// Summarize pending JSONL deliveries from one bounded audit snapshot.
/// Intended for offline maintenance, not a high-frequency runtime endpoint.
pub fn status<D: ReadableDatabase>(
    database: &D,
    now_us: i64,
) -> Result<AuditSinkStatus, AuditSinkError> {
    if now_us <= 0 {
        return Err(AuditSinkError::Storage);
    }
    let read = database.begin_read().map_err(|_| AuditSinkError::Storage)?;
    let audit = read
        .open_table(AUDIT)
        .map_err(|_| AuditSinkError::Storage)?;
    let stats = read
        .open_table(STATS)
        .map_err(|_| AuditSinkError::Storage)?;
    let configured = match stats
        .get(SINK_ID_KEY)
        .map_err(|_| AuditSinkError::Storage)?
    {
        Some(value) if value.value().len() == 60 => true,
        Some(_) => return Err(AuditSinkError::Storage),
        None => false,
    };
    if audit.len().map_err(|_| AuditSinkError::Storage)? > MAX_AUDIT_EVENTS as u64 {
        return Err(AuditSinkError::Storage);
    }
    let mut pending_events = 0u64;
    let mut oldest_pending_at_us = None::<i64>;
    let mut event_ids = HashSet::new();
    for entry in audit.iter().map_err(|_| AuditSinkError::Storage)? {
        let (key, value) = entry.map_err(|_| AuditSinkError::Storage)?;
        let event_id: [u8; 16] = key
            .value()
            .try_into()
            .map_err(|_| AuditSinkError::Storage)?;
        let event = AuditEvent::decode(value.value()).map_err(|_| AuditSinkError::Storage)?;
        if event.event_id != event_id {
            return Err(AuditSinkError::Storage);
        }
        if !event_ids.insert(event_id) {
            return Err(AuditSinkError::Storage);
        }
        let mut marker_key = DELIVERED_PREFIX.to_vec();
        marker_key.extend_from_slice(&event_id);
        let marker = stats
            .get(marker_key.as_slice())
            .map_err(|_| AuditSinkError::Storage)?;
        match marker.as_ref().map(|value| value.value()) {
            None => {
                if configured {
                    pending_events = pending_events.saturating_add(1);
                    oldest_pending_at_us =
                        Some(oldest_pending_at_us.map_or(event.occurred_at_us, |oldest| {
                            oldest.min(event.occurred_at_us)
                        }));
                }
            }
            Some(b"1") => {}
            Some(_) => return Err(AuditSinkError::Storage),
        }
    }
    for entry in stats.iter().map_err(|_| AuditSinkError::Storage)? {
        let (key, value) = entry.map_err(|_| AuditSinkError::Storage)?;
        let key = key.value();
        if !key.starts_with(DELIVERED_PREFIX) {
            continue;
        }
        if key.len() != DELIVERED_PREFIX.len() + 16 || value.value() != b"1" || !configured {
            return Err(AuditSinkError::Storage);
        }
        let event_id: [u8; 16] = key[DELIVERED_PREFIX.len()..]
            .try_into()
            .map_err(|_| AuditSinkError::Storage)?;
        if !event_ids.contains(&event_id) {
            return Err(AuditSinkError::Storage);
        }
    }
    let oldest_pending_age_us = oldest_pending_at_us.map(|timestamp| {
        u64::try_from(now_us.saturating_sub(timestamp).max(0)).unwrap_or(u64::MAX)
    });
    Ok(AuditSinkStatus {
        configured,
        pending_events,
        oldest_pending_age_us,
    })
}

/// Deliver up to `max_events` pending AUD1 records to a mode-0600 JSONL file.
///
/// A path change resets delivery markers and replays the retained audit
/// history to the new sink. Each JSON object carries `event_id` as a stable
/// deduplication key. Callers should treat failures as retryable telemetry
/// failures and must not turn an already committed database operation into an
/// abort.
pub fn deliver_jsonl_batch(
    database: &Database,
    path: &Path,
    max_events: usize,
) -> Result<usize, AuditSinkError> {
    deliver_jsonl_batch_with_after_sync(database, path, max_events, |_| Ok(()))
}

fn deliver_jsonl_batch_with_after_sync<F>(
    database: &Database,
    path: &Path,
    max_events: usize,
    mut after_sync: F,
) -> Result<usize, AuditSinkError>
where
    F: FnMut(&[u8; 16]) -> Result<(), AuditSinkError>,
{
    if max_events == 0 || max_events > MAX_BATCH {
        return Err(AuditSinkError::InvalidBatch);
    }
    let _guard = DELIVERY_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| AuditSinkError::Storage)?;
    let (canonical_path, parent, path_id) = canonical_sink_path(path)?;
    let (mut file, created) = match OpenOptions::new()
        .read(true)
        .append(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(&canonical_path)
    {
        Ok(file) => (file, true),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (
            OpenOptions::new()
                .read(true)
                .append(true)
                .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
                .open(&canonical_path)?,
            false,
        ),
        Err(error) => return Err(error.into()),
    };
    let metadata = file.metadata()?;
    if !metadata.file_type().is_file()
        || metadata.permissions().mode() & 0o7777 != 0o600
        || metadata.nlink() != 1
    {
        return Err(AuditSinkError::InvalidSinkFile);
    }
    // SAFETY: flock only operates on this valid open file descriptor.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err(AuditSinkError::Io(std::io::Error::last_os_error()));
    }
    if created {
        File::open(&parent)?.sync_all()?;
    }
    repair_partial_tail(&mut file)?;
    let birth_time = file_birth_time(&file)?;
    let mut sink_id = [0; 60];
    sink_id[..32].copy_from_slice(&path_id);
    sink_id[32..40].copy_from_slice(&metadata.dev().to_le_bytes());
    sink_id[40..48].copy_from_slice(&metadata.ino().to_le_bytes());
    sink_id[48..56].copy_from_slice(&birth_time.0.to_le_bytes());
    sink_id[56..].copy_from_slice(&birth_time.1.to_le_bytes());
    install_sink_id(database, &sink_id)?;
    let pending = pending_events(database, max_events)?;
    if pending.is_empty() {
        return Ok(0);
    }

    let mut delivered = 0;
    for (event_id, event) in pending {
        let line = encode_json_line(&event);
        file.write_all(&line)?;
        file.sync_all()?;
        after_sync(&event_id)?;
        mark_delivered(database, &event_id)?;
        delivered += 1;
    }
    Ok(delivered)
}

/// Remove delivery bookkeeping for events pruned from AUDIT in the same
/// transaction that removes their outbox records.
pub(crate) fn remove_delivery_markers(
    table: &mut redb::Table<&'static [u8], &'static [u8]>,
    event_ids: &[[u8; 16]],
) -> Result<(), AuditSinkError> {
    for event_id in event_ids {
        let mut key = DELIVERED_PREFIX.to_vec();
        key.extend_from_slice(event_id);
        table
            .remove(key.as_slice())
            .map_err(|_| AuditSinkError::Storage)?;
    }
    Ok(())
}

fn canonical_sink_path(path: &Path) -> Result<(PathBuf, PathBuf, [u8; 32]), AuditSinkError> {
    if !path.is_absolute() {
        return Err(AuditSinkError::InvalidPath);
    }
    let name = path
        .file_name()
        .filter(|name| !name.is_empty())
        .ok_or(AuditSinkError::InvalidPath)?;
    let input_parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent = input_parent.canonicalize()?;
    let canonical = parent.join(name);
    let sink_id: [u8; 32] = Sha256::digest(canonical.as_os_str().as_encoded_bytes()).into();
    Ok((canonical, parent, sink_id))
}

fn file_birth_time(file: &File) -> Result<(i64, u32), AuditSinkError> {
    let empty_path = c"";
    // SAFETY: statx writes to the initialized result structure and the fd is
    // held open for the entire call. AT_EMPTY_PATH addresses that open file.
    let mut result = unsafe { std::mem::zeroed::<libc::statx>() };
    let status = unsafe {
        libc::statx(
            file.as_raw_fd(),
            empty_path.as_ptr(),
            libc::AT_EMPTY_PATH,
            libc::STATX_BTIME,
            &mut result,
        )
    };
    if status != 0 {
        return Err(AuditSinkError::Io(std::io::Error::last_os_error()));
    }
    if result.stx_mask & libc::STATX_BTIME == 0 {
        return Err(AuditSinkError::InvalidSinkFile);
    }
    Ok((result.stx_btime.tv_sec, result.stx_btime.tv_nsec))
}

fn install_sink_id(database: &Database, sink_id: &[u8; 60]) -> Result<(), AuditSinkError> {
    {
        let read = database.begin_read().map_err(|_| AuditSinkError::Storage)?;
        let stats = read
            .open_table(STATS)
            .map_err(|_| AuditSinkError::Storage)?;
        if stats
            .get(SINK_ID_KEY)
            .map_err(|_| AuditSinkError::Storage)?
            .is_some_and(|value| value.value() == sink_id)
        {
            return Ok(());
        }
    }
    let write = database
        .begin_write()
        .map_err(|_| AuditSinkError::Storage)?;
    {
        let mut stats = write
            .open_table(STATS)
            .map_err(|_| AuditSinkError::Storage)?;
        let current = stats
            .get(SINK_ID_KEY)
            .map_err(|_| AuditSinkError::Storage)?
            .map(|value| value.value().to_vec());
        if current.as_deref() != Some(sink_id.as_slice()) {
            let mut keys = Vec::new();
            for entry in stats.iter().map_err(|_| AuditSinkError::Storage)? {
                let (key, _) = entry.map_err(|_| AuditSinkError::Storage)?;
                let key = key.value().to_vec();
                if key.starts_with(DELIVERED_PREFIX) {
                    keys.push(key);
                }
            }
            for key in keys {
                stats
                    .remove(key.as_slice())
                    .map_err(|_| AuditSinkError::Storage)?;
            }
            stats
                .insert(SINK_ID_KEY, sink_id.as_slice())
                .map_err(|_| AuditSinkError::Storage)?;
        }
    }
    write.commit().map_err(|_| {
        crate::storage_manager::mark_database_uncertain(database);
        AuditSinkError::Storage
    })
}

fn pending_events(
    database: &Database,
    max_events: usize,
) -> Result<Vec<([u8; 16], AuditEvent)>, AuditSinkError> {
    let read = database.begin_read().map_err(|_| AuditSinkError::Storage)?;
    let audit = read
        .open_table(AUDIT)
        .map_err(|_| AuditSinkError::Storage)?;
    let stats = read
        .open_table(STATS)
        .map_err(|_| AuditSinkError::Storage)?;
    if audit.len().map_err(|_| AuditSinkError::Storage)? > MAX_AUDIT_EVENTS as u64 {
        return Err(AuditSinkError::Storage);
    }
    let mut events = Vec::with_capacity(max_events);
    for entry in audit.iter().map_err(|_| AuditSinkError::Storage)? {
        if events.len() == max_events {
            break;
        }
        let (key, value) = entry.map_err(|_| AuditSinkError::Storage)?;
        let id: [u8; 16] = key
            .value()
            .try_into()
            .map_err(|_| AuditSinkError::Storage)?;
        let event = AuditEvent::decode(value.value()).map_err(|_| AuditSinkError::Storage)?;
        if event.event_id != id {
            return Err(AuditSinkError::Storage);
        }
        let mut marker = DELIVERED_PREFIX.to_vec();
        marker.extend_from_slice(&id);
        let marker = stats
            .get(marker.as_slice())
            .map_err(|_| AuditSinkError::Storage)?
            .map(|value| value.value().to_vec());
        match marker.as_deref() {
            None => events.push((id, event)),
            Some(b"1") => {}
            Some(_) => return Err(AuditSinkError::Storage),
        }
    }
    Ok(events)
}

fn mark_delivered(database: &Database, event_id: &[u8; 16]) -> Result<(), AuditSinkError> {
    let write = database
        .begin_write()
        .map_err(|_| AuditSinkError::Storage)?;
    {
        let mut stats = write
            .open_table(STATS)
            .map_err(|_| AuditSinkError::Storage)?;
        let mut key = DELIVERED_PREFIX.to_vec();
        key.extend_from_slice(event_id);
        stats
            .insert(key.as_slice(), b"1".as_slice())
            .map_err(|_| AuditSinkError::Storage)?;
    }
    write.commit().map_err(|_| {
        crate::storage_manager::mark_database_uncertain(database);
        AuditSinkError::Storage
    })
}

fn repair_partial_tail(file: &mut File) -> Result<(), AuditSinkError> {
    let length = file.metadata()?.len();
    if length == 0 {
        return Ok(());
    }
    file.seek(SeekFrom::End(-1))?;
    let mut last = [0];
    file.read_exact(&mut last)?;
    if last[0] == b'\n' {
        file.seek(SeekFrom::End(0))?;
        return Ok(());
    }
    let suffix_length = length.min(MAX_LINE_LENGTH);
    let suffix_start = length - suffix_length;
    let mut suffix = vec![0; suffix_length as usize];
    file.seek(SeekFrom::Start(suffix_start))?;
    file.read_exact(&mut suffix)?;
    let truncate_to = match suffix.iter().rposition(|byte| *byte == b'\n') {
        Some(index) => suffix_start + index as u64 + 1,
        None if suffix_start == 0 => 0,
        None => return Err(AuditSinkError::InvalidSinkFile),
    };
    file.set_len(truncate_to as u64)?;
    file.sync_all()?;
    file.seek(SeekFrom::End(0))?;
    Ok(())
}

fn encode_json_line(event: &AuditEvent) -> Vec<u8> {
    let targets = event
        .target_ids
        .iter()
        .map(|id| format!("\"{}\"", lower_hex(id)))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"event_id\":\"{}\",\"actor_id\":{},\"correlation_id\":\"{}\",\"occurred_at_us\":{},\"action\":{},\"result\":{},\"target_ids\":[{}]}}\n",
        lower_hex(&event.event_id),
        event.actor_id.map_or_else(|| "null".to_owned(), |id| format!("\"{}\"", lower_hex(&id))),
        lower_hex(&event.correlation_id),
        event.occurred_at_us,
        event.action as u16,
        event.result as u16,
        targets,
    )
    .into_bytes()
}

fn lower_hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::{AuditAction, AuditResult};
    use crate::schema::{AUDIT, create_tables};
    use std::os::unix::fs::PermissionsExt;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn synced_line_before_marker_commit_is_replayed_after_database_reopen() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "prdb-audit-sink-crash-window-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir(&directory).expect("create fixture directory");
        let database_path = directory.join("database.redb");
        let sink_path = directory.join("audit.jsonl");
        let database = Database::create(&database_path).expect("create database");
        let write = database.begin_write().expect("begin table creation");
        create_tables(&write).expect("create database tables");
        {
            let mut audit = write.open_table(AUDIT).expect("open audit table");
            let event = AuditEvent {
                event_id: [0x31; 16],
                actor_id: Some([0x32; 16]),
                correlation_id: [0x33; 16],
                occurred_at_us: 44,
                action: AuditAction::Ddl,
                result: AuditResult::Succeeded,
                target_ids: vec![[0x34; 16]],
            };
            audit
                .insert(
                    event.event_id.as_slice(),
                    event.encode().unwrap().as_slice(),
                )
                .expect("insert audit event");
        }
        write.commit().expect("commit audit event");

        let interrupted = deliver_jsonl_batch_with_after_sync(&database, &sink_path, 1, |_| {
            Err(AuditSinkError::Storage)
        });
        assert!(interrupted.is_err());
        let synced_line = std::fs::read_to_string(&sink_path).expect("read synced line");
        assert!(synced_line.ends_with('\n'));
        assert!(synced_line.contains("31313131313131313131313131313131"));

        drop(database);
        let database = Database::open(&database_path).expect("reopen database after crash");
        assert_eq!(
            deliver_jsonl_batch(&database, &sink_path, 1).expect("retry pending event"),
            1
        );
        let replayed = std::fs::read_to_string(&sink_path).expect("read replayed lines");
        let lines: Vec<_> = replayed.lines().collect();
        assert_eq!(lines, [synced_line.trim_end(), synced_line.trim_end()]);
        assert_eq!(
            std::fs::metadata(&sink_path)
                .expect("sink metadata")
                .permissions()
                .mode()
                & 0o7777,
            0o600
        );

        drop(database);
        std::fs::remove_dir_all(directory).expect("remove fixture directory");
    }
}
