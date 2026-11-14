//! Trusted, bounded logical format transitions for offline maintenance.

use crate::catalog::{
    CatalogField, WireType, decode_registered_envelope, encode_envelope, family_schema,
};
use crate::identity_history::{IdentityHistory, IdentityHistoryError};
use crate::metadata::EngineMetadata;
use crate::schema::{CATALOG, ENGINE_META};
use redb::{Database, ReadableTable};

const MAX_V1_TOMBSTONES: usize = crate::identity_history::MAX_IDENTITY_HISTORY_IDS;

#[derive(Debug)]
pub enum MigrationError {
    Storage,
    InvalidSource,
    InvalidHistory(IdentityHistoryError),
    InvalidCatalog,
}

impl std::fmt::Display for MigrationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Storage => formatter.write_str("migration storage operation failed"),
            Self::InvalidSource => {
                formatter.write_str("migration target is not a catalog-v1 READY image")
            }
            Self::InvalidHistory(error) => write!(formatter, "invalid identity history: {error:?}"),
            Self::InvalidCatalog => formatter.write_str("migration catalog conversion failed"),
        }
    }
}

impl std::error::Error for MigrationError {}

/// Apply the atomic catalog-v1 to catalog-v2 record transition to a private
/// staging database that was copied byte-for-byte from its locked source.
///
/// This changes only engine metadata, the database descriptor, and family-17
/// identity tombstones. It uses one redb transaction, bounded by the identity
/// history format's 65,536-ID limit. The caller must validate the source and
/// backup binding before calling, then run full read-only READY validation on
/// this staging image before publication.
pub fn upgrade_v1_staging_database(
    staging: &Database,
    history: &IdentityHistory,
) -> Result<(), MigrationError> {
    crate::startup::validate_v1_migration_database(staging)
        .map_err(|_| MigrationError::InvalidSource)?;

    let write = staging.begin_write().map_err(|_| MigrationError::Storage)?;
    let mut metadata_table = write
        .open_table(ENGINE_META)
        .map_err(|_| MigrationError::Storage)?;
    let current_metadata = metadata_table
        .get(&b"format"[..])
        .map_err(|_| MigrationError::Storage)?
        .ok_or(MigrationError::InvalidSource)?;
    let mut metadata = EngineMetadata::decode(current_metadata.value())
        .map_err(|_| MigrationError::InvalidSource)?;
    drop(current_metadata);
    if metadata.catalog_version != 1
        || history.database_id != metadata.database_id
        || history.commit_sequence != metadata.commit_sequence
    {
        return Err(MigrationError::InvalidSource);
    }

    let mut catalog = write
        .open_table(CATALOG)
        .map_err(|_| MigrationError::Storage)?;
    let database_key = id_key(1, metadata.database_id);
    let current_database_record = catalog
        .get(database_key.as_slice())
        .map_err(|_| MigrationError::Storage)?
        .ok_or(MigrationError::InvalidCatalog)?;
    let mut database_record = decode_registered_envelope(current_database_record.value())
        .map_err(|_| MigrationError::InvalidCatalog)?;
    drop(current_database_record);
    if database_record.logical_key != database_key {
        return Err(MigrationError::InvalidCatalog);
    }

    let mut occupied_ids = Vec::new();
    for entry in catalog.iter().map_err(|_| MigrationError::Storage)? {
        let (key, _) = entry.map_err(|_| MigrationError::Storage)?;
        if key.value().len() != 19 {
            continue;
        }
        let family = u16::from_be_bytes([key.value()[0], key.value()[1]]);
        if matches!(family, 2 | 10 | 17) {
            let mut id = [0; 16];
            id.copy_from_slice(&key.value()[3..19]);
            occupied_ids.push(id);
        }
    }
    if occupied_ids.len() > MAX_V1_TOMBSTONES * 3 {
        return Err(MigrationError::InvalidCatalog);
    }
    history
        .validate_tombstones_unoccupied(occupied_ids)
        .map_err(MigrationError::InvalidHistory)?;

    update_database_descriptor(&mut database_record.fields)?;
    let encoded_database_record = encode_envelope(
        &family_schema(1).ok_or(MigrationError::InvalidCatalog)?,
        &database_key,
        &database_record.fields,
    )
    .map_err(|_| MigrationError::InvalidCatalog)?;
    metadata.catalog_version = 2;
    let encoded_metadata = metadata
        .encode()
        .map_err(|_| MigrationError::InvalidSource)?;

    for id in &history.tombstone_ids {
        let key = id_key(17, *id);
        let tombstone = encode_envelope(
            &family_schema(17).ok_or(MigrationError::InvalidCatalog)?,
            &key,
            &[],
        )
        .map_err(|_| MigrationError::InvalidCatalog)?;
        catalog
            .insert(key.as_slice(), tombstone.as_slice())
            .map_err(|_| MigrationError::Storage)?;
    }
    catalog
        .insert(database_key.as_slice(), encoded_database_record.as_slice())
        .map_err(|_| MigrationError::Storage)?;
    metadata_table
        .insert(&b"format"[..], &encoded_metadata[..])
        .map_err(|_| MigrationError::Storage)?;
    drop(catalog);
    drop(metadata_table);
    write.commit().map_err(|_| MigrationError::Storage)
}

fn update_database_descriptor(fields: &mut [CatalogField]) -> Result<(), MigrationError> {
    let catalog_version = fields
        .iter_mut()
        .find(|field| field.tag == 2)
        .ok_or(MigrationError::InvalidCatalog)?;
    if catalog_version.wire_type != WireType::U32 || catalog_version.payload != 1u32.to_le_bytes() {
        return Err(MigrationError::InvalidCatalog);
    }
    catalog_version.payload = 2u32.to_le_bytes().to_vec();

    let builtins = fields
        .iter_mut()
        .find(|field| field.tag == 3)
        .ok_or(MigrationError::InvalidCatalog)?;
    if builtins.wire_type != WireType::Bytes || builtins.payload.len() != 4 + 30 * 20 {
        return Err(MigrationError::InvalidCatalog);
    }
    let count = u32::from_le_bytes(
        builtins.payload[..4]
            .try_into()
            .map_err(|_| MigrationError::InvalidCatalog)?,
    );
    if count != 30 {
        return Err(MigrationError::InvalidCatalog);
    }
    let last = &builtins.payload[builtins.payload.len() - 20..];
    if u16::from_le_bytes([last[0], last[1]]) != 216 || last[2..4] != [0, 0] {
        return Err(MigrationError::InvalidCatalog);
    }
    let next_count = count + 1;
    builtins.payload[..4].copy_from_slice(&next_count.to_le_bytes());
    builtins.payload.extend_from_slice(&217u16.to_le_bytes());
    builtins.payload.extend_from_slice(&[0, 0]);
    let mut id = [0x80; 16];
    id[15] = 217;
    builtins.payload.extend_from_slice(&id);
    Ok(())
}

fn id_key(family: u16, id: [u8; 16]) -> Vec<u8> {
    let mut key = Vec::with_capacity(19);
    key.extend_from_slice(&family.to_be_bytes());
    key.push(1);
    key.extend_from_slice(&id);
    key
}
