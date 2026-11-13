//! Offline, checksummed export format for bounded AUD1 history.

use crate::audit::{AuditEvent, MAX_AUDIT_EVENTS};
use sha2::{Digest, Sha256};

const HEADER_LENGTH: usize = 14;
const DIGEST_LENGTH: usize = 32;
pub const MAX_ARCHIVE_LENGTH: usize =
    HEADER_LENGTH + MAX_AUDIT_EVENTS * (16 + 4 + 4_164) + DIGEST_LENGTH;
pub type AuditArchiveRecord = ([u8; 16], Vec<u8>);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuditArchiveFormatError {
    InvalidLength,
    InvalidMagic,
    UnsupportedVersion,
    InvalidDigest,
    TooManyEvents,
    InvalidAuditEvent,
    UnsortedEvents,
}

impl std::fmt::Display for AuditArchiveFormatError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "invalid audit archive: {self:?}")
    }
}

impl std::error::Error for AuditArchiveFormatError {}

pub fn encode(events: &[AuditArchiveRecord]) -> Result<Vec<u8>, AuditArchiveFormatError> {
    if events.len() > MAX_AUDIT_EVENTS {
        return Err(AuditArchiveFormatError::TooManyEvents);
    }
    let mut output = Vec::with_capacity(
        HEADER_LENGTH
            + events
                .iter()
                .map(|(_, value)| 20usize.saturating_add(value.len()))
                .sum::<usize>()
            + DIGEST_LENGTH,
    );
    output.extend_from_slice(b"PRA1");
    output.extend_from_slice(&1u16.to_le_bytes());
    output.extend_from_slice(&(events.len() as u64).to_le_bytes());
    let mut previous = None;
    for (key, value) in events {
        let decoded =
            AuditEvent::decode(value).map_err(|_| AuditArchiveFormatError::InvalidAuditEvent)?;
        if decoded.event_id != *key {
            return Err(AuditArchiveFormatError::InvalidAuditEvent);
        }
        if previous.is_some_and(|previous| previous >= *key) {
            return Err(AuditArchiveFormatError::UnsortedEvents);
        }
        previous = Some(*key);
        let value_length =
            u32::try_from(value.len()).map_err(|_| AuditArchiveFormatError::InvalidLength)?;
        output.extend_from_slice(key);
        output.extend_from_slice(&value_length.to_le_bytes());
        output.extend_from_slice(value);
    }
    let digest = Sha256::digest(&output);
    output.extend_from_slice(&digest);
    Ok(output)
}

pub fn decode(bytes: &[u8]) -> Result<Vec<AuditArchiveRecord>, AuditArchiveFormatError> {
    if bytes.len() < HEADER_LENGTH + DIGEST_LENGTH || bytes.len() > MAX_ARCHIVE_LENGTH {
        return Err(AuditArchiveFormatError::InvalidLength);
    }
    if &bytes[..4] != b"PRA1" {
        return Err(AuditArchiveFormatError::InvalidMagic);
    }
    if u16::from_le_bytes(bytes[4..6].try_into().unwrap()) != 1 {
        return Err(AuditArchiveFormatError::UnsupportedVersion);
    }
    let count = usize::try_from(u64::from_le_bytes(bytes[6..14].try_into().unwrap()))
        .map_err(|_| AuditArchiveFormatError::TooManyEvents)?;
    if count > MAX_AUDIT_EVENTS {
        return Err(AuditArchiveFormatError::TooManyEvents);
    }
    let payload_end = bytes.len() - DIGEST_LENGTH;
    let digest = Sha256::digest(&bytes[..payload_end]);
    if digest.as_slice() != &bytes[payload_end..] {
        return Err(AuditArchiveFormatError::InvalidDigest);
    }
    let mut offset = HEADER_LENGTH;
    let mut events = Vec::with_capacity(count);
    let mut previous = None;
    for _ in 0..count {
        if payload_end.saturating_sub(offset) < 20 {
            return Err(AuditArchiveFormatError::InvalidLength);
        }
        let key: [u8; 16] = bytes[offset..offset + 16].try_into().unwrap();
        let value_length =
            u32::from_le_bytes(bytes[offset + 16..offset + 20].try_into().unwrap()) as usize;
        offset += 20;
        if value_length > 4_164 || payload_end.saturating_sub(offset) < value_length {
            return Err(AuditArchiveFormatError::InvalidLength);
        }
        if previous.is_some_and(|previous| previous >= key) {
            return Err(AuditArchiveFormatError::UnsortedEvents);
        }
        previous = Some(key);
        let value = bytes[offset..offset + value_length].to_vec();
        let decoded =
            AuditEvent::decode(&value).map_err(|_| AuditArchiveFormatError::InvalidAuditEvent)?;
        if decoded.event_id != key {
            return Err(AuditArchiveFormatError::InvalidAuditEvent);
        }
        events.push((key, value));
        offset += value_length;
    }
    if offset != payload_end {
        return Err(AuditArchiveFormatError::InvalidLength);
    }
    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::{AuditArchiveFormatError, decode, encode};
    use crate::audit::{AuditAction, AuditEvent, AuditResult};

    fn event(event_id: [u8; 16]) -> ([u8; 16], Vec<u8>) {
        let event = AuditEvent {
            event_id,
            actor_id: Some([0x10; 16]),
            correlation_id: [0x20; 16],
            occurred_at_us: 1,
            action: AuditAction::Ddl,
            result: AuditResult::Succeeded,
            target_ids: vec![[0x30; 16]],
        };
        (event_id, event.encode().unwrap())
    }

    #[test]
    fn archive_round_trips_and_rejects_digest_or_record_corruption() {
        let expected = vec![event([1; 16]), event([2; 16])];
        let mut archive = encode(&expected).unwrap();
        assert_eq!(decode(&archive), Ok(expected));
        archive[20] ^= 1;
        assert_eq!(
            decode(&archive),
            Err(AuditArchiveFormatError::InvalidDigest)
        );
    }

    #[test]
    fn archive_requires_sorted_unique_event_ids() {
        assert_eq!(
            encode(&[event([2; 16]), event([1; 16])]),
            Err(AuditArchiveFormatError::UnsortedEvents)
        );
    }
}
