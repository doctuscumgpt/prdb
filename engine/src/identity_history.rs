//! Operator-certified, complete identity-deletion history for offline v1 migration.

use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::Read;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;

pub const MAX_IDENTITY_HISTORY_IDS: usize = 65_536;
const MAGIC: &str = "PRDB-IDENTITY-HISTORY-1";
const CHECKSUM_LINE_BYTES: usize = b"sha256=".len() + 64 + 1;
pub const MAX_IDENTITY_HISTORY_BYTES: usize = MAX_IDENTITY_HISTORY_IDS * 33 + 256;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IdentityHistory {
    pub database_id: [u8; 16],
    pub commit_sequence: u64,
    /// Complete set of v1 user and role IDs deleted before family-17 existed.
    /// The order is canonical byte order and duplicates are invalid.
    pub tombstone_ids: Vec<[u8; 16]>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IdentityHistoryError {
    InvalidEncoding,
    InvalidHistory,
    IntegrityMismatch,
    TooLarge,
    TombstoneIdAlreadyUsed,
}

#[derive(Debug)]
pub enum IdentityHistoryFileError {
    Io(std::io::Error),
    InvalidFile,
    InvalidHistory(IdentityHistoryError),
    WrongSource,
}

impl std::fmt::Display for IdentityHistoryFileError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "identity history file I/O failed: {error}"),
            Self::InvalidFile => formatter.write_str("identity history file is not private"),
            Self::InvalidHistory(error) => write!(formatter, "invalid identity history: {error:?}"),
            Self::WrongSource => formatter.write_str("identity history names a different source"),
        }
    }
}

impl std::error::Error for IdentityHistoryFileError {}

impl IdentityHistory {
    /// Reject history IDs that are still reserved by a live catalog identity
    /// or by an existing tombstone in the source image.
    ///
    /// The caller supplies IDs from family 2, 10, and 17 after validating the
    /// source image. Keeping this check separate from the file codec lets the
    /// migration executor scan those bounded catalog families without making
    /// the history format depend on redb.
    pub fn validate_tombstones_unoccupied(
        &self,
        occupied_ids: impl IntoIterator<Item = [u8; 16]>,
    ) -> Result<(), IdentityHistoryError> {
        self.validate()?;
        if occupied_ids
            .into_iter()
            .any(|id| self.tombstone_ids.binary_search(&id).is_ok())
        {
            return Err(IdentityHistoryError::TombstoneIdAlreadyUsed);
        }
        Ok(())
    }

    /// Read an operator-certified history from an owner-owned mode-0600 file
    /// without following symlinks, then bind it to a verified source frontier.
    pub fn read_file(
        path: impl AsRef<Path>,
        expected_database_id: [u8; 16],
        expected_commit_sequence: u64,
    ) -> Result<Self, IdentityHistoryFileError> {
        let path = path.as_ref();
        if !path.is_absolute() {
            return Err(IdentityHistoryFileError::InvalidFile);
        }
        let path_metadata = fs::symlink_metadata(path).map_err(IdentityHistoryFileError::Io)?;
        if !path_metadata.file_type().is_file()
            || path_metadata.nlink() != 1
            || path_metadata.mode() & 0o7777 != 0o600
            || path_metadata.uid() != effective_uid()
            || path_metadata.len() > MAX_IDENTITY_HISTORY_BYTES as u64
        {
            return Err(IdentityHistoryFileError::InvalidFile);
        }
        let mut file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
            .open(path)
            .map_err(|error| {
                if error.raw_os_error() == Some(libc::ELOOP) {
                    IdentityHistoryFileError::InvalidFile
                } else {
                    IdentityHistoryFileError::Io(error)
                }
            })?;
        let before = file.metadata().map_err(IdentityHistoryFileError::Io)?;
        if !same_private_file(&path_metadata, &before) {
            return Err(IdentityHistoryFileError::InvalidFile);
        }
        let mut bytes = Vec::with_capacity(before.len() as usize);
        file.by_ref()
            .take(MAX_IDENTITY_HISTORY_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(IdentityHistoryFileError::Io)?;
        let after = file.metadata().map_err(IdentityHistoryFileError::Io)?;
        if bytes.len() > MAX_IDENTITY_HISTORY_BYTES
            || bytes.len() as u64 != before.len()
            || !same_private_file(&before, &after)
        {
            return Err(IdentityHistoryFileError::InvalidFile);
        }
        let history = Self::decode(&bytes).map_err(IdentityHistoryFileError::InvalidHistory)?;
        if history.database_id != expected_database_id
            || history.commit_sequence != expected_commit_sequence
        {
            return Err(IdentityHistoryFileError::WrongSource);
        }
        Ok(history)
    }

    pub fn encode(&self) -> Result<Vec<u8>, IdentityHistoryError> {
        self.validate()?;
        let mut bytes =
            Vec::with_capacity(160 + self.tombstone_ids.len() * 33 + CHECKSUM_LINE_BYTES);
        bytes.extend_from_slice(MAGIC.as_bytes());
        bytes.push(b'\n');
        bytes.extend_from_slice(b"database_id=");
        append_hex(&mut bytes, &self.database_id);
        bytes.extend_from_slice(b"\ncommit_sequence=");
        bytes.extend_from_slice(self.commit_sequence.to_string().as_bytes());
        bytes.extend_from_slice(b"\ncomplete=true\ntombstone_count=");
        bytes.extend_from_slice(self.tombstone_ids.len().to_string().as_bytes());
        bytes.push(b'\n');
        for id in &self.tombstone_ids {
            append_hex(&mut bytes, id);
            bytes.push(b'\n');
        }
        if bytes.len() + CHECKSUM_LINE_BYTES > MAX_IDENTITY_HISTORY_BYTES {
            return Err(IdentityHistoryError::TooLarge);
        }
        let digest = Sha256::digest(&bytes);
        bytes.extend_from_slice(b"sha256=");
        append_hex(&mut bytes, &digest);
        bytes.push(b'\n');
        Ok(bytes)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, IdentityHistoryError> {
        if bytes.len() > MAX_IDENTITY_HISTORY_BYTES {
            return Err(IdentityHistoryError::TooLarge);
        }
        if bytes.len() < CHECKSUM_LINE_BYTES || !bytes.ends_with(b"\n") {
            return Err(IdentityHistoryError::InvalidEncoding);
        }
        let body_length = bytes.len() - CHECKSUM_LINE_BYTES;
        let checksum_line = &bytes[body_length..bytes.len() - 1];
        if !checksum_line.starts_with(b"sha256=") || checksum_line.len() != 71 {
            return Err(IdentityHistoryError::InvalidEncoding);
        }
        let expected_digest = decode_lower_hex::<32>(&checksum_line[7..])
            .ok_or(IdentityHistoryError::InvalidEncoding)?;
        if Sha256::digest(&bytes[..body_length]).as_slice() != expected_digest {
            return Err(IdentityHistoryError::IntegrityMismatch);
        }
        let body = std::str::from_utf8(&bytes[..body_length])
            .map_err(|_| IdentityHistoryError::InvalidEncoding)?;
        let mut lines = body.split_terminator('\n');
        if lines.next() != Some(MAGIC) {
            return Err(IdentityHistoryError::InvalidEncoding);
        }
        let database_id = parse_prefixed_hex::<16>(lines.next(), "database_id=")
            .ok_or(IdentityHistoryError::InvalidEncoding)?;
        let commit_sequence = parse_prefixed_decimal(lines.next(), "commit_sequence=")
            .ok_or(IdentityHistoryError::InvalidEncoding)?;
        if lines.next() != Some("complete=true") {
            return Err(IdentityHistoryError::InvalidEncoding);
        }
        let tombstone_count = parse_prefixed_decimal(lines.next(), "tombstone_count=")
            .ok_or(IdentityHistoryError::InvalidEncoding)?;
        let tombstone_count =
            usize::try_from(tombstone_count).map_err(|_| IdentityHistoryError::TooLarge)?;
        if tombstone_count > MAX_IDENTITY_HISTORY_IDS {
            return Err(IdentityHistoryError::TooLarge);
        }
        let mut tombstone_ids = Vec::with_capacity(tombstone_count);
        for _ in 0..tombstone_count {
            tombstone_ids.push(
                decode_lower_hex::<16>(
                    lines
                        .next()
                        .ok_or(IdentityHistoryError::InvalidEncoding)?
                        .as_bytes(),
                )
                .ok_or(IdentityHistoryError::InvalidEncoding)?,
            );
        }
        if lines.next().is_some() {
            return Err(IdentityHistoryError::InvalidEncoding);
        }
        let history = Self {
            database_id,
            commit_sequence,
            tombstone_ids,
        };
        history.validate()?;
        Ok(history)
    }

    fn validate(&self) -> Result<(), IdentityHistoryError> {
        if self.database_id == [0; 16]
            || self.commit_sequence == 0
            || self.tombstone_ids.len() > MAX_IDENTITY_HISTORY_IDS
            || self.tombstone_ids.contains(&[0; 16])
            || self.tombstone_ids.windows(2).any(|pair| pair[0] >= pair[1])
        {
            return Err(IdentityHistoryError::InvalidHistory);
        }
        Ok(())
    }
}

fn same_private_file(before: &std::fs::Metadata, after: &std::fs::Metadata) -> bool {
    before.file_type().is_file()
        && after.file_type().is_file()
        && after.nlink() == 1
        && after.mode() & 0o7777 == 0o600
        && after.uid() == effective_uid()
        && before.dev() == after.dev()
        && before.ino() == after.ino()
        && before.len() == after.len()
        && before.ctime() == after.ctime()
        && before.ctime_nsec() == after.ctime_nsec()
}

fn effective_uid() -> u32 {
    // SAFETY: geteuid has no preconditions and does not read or retain pointers.
    unsafe { libc::geteuid() }
}

fn append_hex(bytes: &mut Vec<u8>, value: &[u8]) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in value {
        bytes.push(HEX[(byte >> 4) as usize]);
        bytes.push(HEX[(byte & 0x0f) as usize]);
    }
}

fn decode_lower_hex<const N: usize>(value: &[u8]) -> Option<[u8; N]> {
    if value.len() != N.checked_mul(2)? {
        return None;
    }
    let mut output = [0; N];
    for (index, byte) in output.iter_mut().enumerate() {
        let high = decode_lower_hex_digit(value[index * 2])?;
        let low = decode_lower_hex_digit(value[index * 2 + 1])?;
        *byte = high << 4 | low;
    }
    Some(output)
}

fn decode_lower_hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

fn parse_prefixed_hex<const N: usize>(line: Option<&str>, prefix: &str) -> Option<[u8; N]> {
    decode_lower_hex(line?.strip_prefix(prefix)?.as_bytes())
}

fn parse_prefixed_decimal(line: Option<&str>, prefix: &str) -> Option<u64> {
    let value = line?.strip_prefix(prefix)?;
    if value.is_empty()
        || value.len() > 1 && value.starts_with('0')
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    value.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::{IdentityHistory, IdentityHistoryError, IdentityHistoryFileError};
    use std::os::unix::fs::PermissionsExt;

    fn history() -> IdentityHistory {
        IdentityHistory {
            database_id: [0x11; 16],
            commit_sequence: 42,
            tombstone_ids: vec![[0x21; 16], [0x22; 16]],
        }
    }

    #[test]
    fn identity_history_round_trips_canonical_sorted_ids() {
        let expected = history();
        let encoded = expected.encode().expect("encode identity history");
        assert!(encoded.starts_with(b"PRDB-IDENTITY-HISTORY-1\n"));
        assert_eq!(IdentityHistory::decode(&encoded).unwrap(), expected);
    }

    #[test]
    fn identity_history_rejects_noncanonical_or_incomplete_inputs() {
        let mut unsorted = history();
        unsorted.tombstone_ids.reverse();
        assert_eq!(unsorted.encode(), Err(IdentityHistoryError::InvalidHistory));

        let mut zero = history();
        zero.tombstone_ids[0] = [0; 16];
        assert_eq!(zero.encode(), Err(IdentityHistoryError::InvalidHistory));

        let mut duplicate = history();
        duplicate.tombstone_ids[1] = duplicate.tombstone_ids[0];
        assert_eq!(
            duplicate.encode(),
            Err(IdentityHistoryError::InvalidHistory)
        );

        let mut truncated = history().encode().unwrap();
        truncated.pop();
        assert_eq!(
            IdentityHistory::decode(&truncated),
            Err(IdentityHistoryError::InvalidEncoding)
        );
    }

    #[test]
    fn identity_history_detects_digest_changes() {
        let mut encoded = history().encode().unwrap();
        let index = encoded
            .windows(b"complete=true".len())
            .position(|window| window == b"complete=true")
            .unwrap();
        encoded[index] = b'X';
        assert_eq!(
            IdentityHistory::decode(&encoded),
            Err(IdentityHistoryError::IntegrityMismatch)
        );
    }

    #[test]
    fn identity_history_rejects_tombstones_that_are_still_occupied() {
        let history = history();
        assert_eq!(
            history.validate_tombstones_unoccupied([[0x21; 16]]),
            Err(IdentityHistoryError::TombstoneIdAlreadyUsed)
        );
        assert_eq!(
            history.validate_tombstones_unoccupied([[0x20; 16], [0x23; 16]]),
            Ok(())
        );
    }

    #[test]
    fn identity_history_file_is_private_and_bound_to_source_frontier() {
        let directory = std::env::temp_dir().join(format!(
            "prdb-identity-history-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&directory).unwrap();
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = directory.join("history.txt");
        std::fs::write(&path, history().encode().unwrap()).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();

        let decoded = IdentityHistory::read_file(&path, [0x11; 16], 42).unwrap();
        assert_eq!(decoded, history());
        assert!(matches!(
            IdentityHistory::read_file(&path, [0x12; 16], 42),
            Err(IdentityHistoryFileError::WrongSource)
        ));

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
        assert!(matches!(
            IdentityHistory::read_file(&path, [0x11; 16], 42),
            Err(IdentityHistoryFileError::InvalidFile)
        ));
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();

        let link = directory.join("history-link.txt");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(matches!(
            IdentityHistory::read_file(&link, [0x11; 16], 42),
            Err(IdentityHistoryFileError::InvalidFile)
        ));
        std::fs::remove_dir_all(directory).unwrap();
    }
}
