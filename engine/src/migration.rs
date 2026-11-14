//! Trusted, bounded logical format transitions for offline maintenance.

use crate::catalog::{
    CatalogField, WireType, decode_registered_envelope, encode_envelope, family_schema,
};
use crate::identity_history::{IdentityHistory, IdentityHistoryError};
use crate::metadata::EngineMetadata;
use crate::schema::{CATALOG, ENGINE_META};
use redb::{Database, ReadOnlyDatabase, ReadableDatabase, ReadableTable};

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

/// Verify that a resumed v2 target contains precisely the certified tombstone
/// set. The v1 source validator guarantees that it had no family-17 records.
pub fn validate_v2_staging_history(
    source: &ReadOnlyDatabase,
    staging: &ReadOnlyDatabase,
    history: &IdentityHistory,
) -> Result<crate::startup::StorageHealthReport, MigrationError> {
    let source_report = crate::startup::validate_v1_migration_source_read_only(source)
        .map_err(|_| MigrationError::InvalidSource)?;
    let report = crate::startup::validate_persisted_records_read_only(staging)
        .map_err(|_| MigrationError::InvalidSource)?;
    if report.metadata.catalog_version != 2
        || report.metadata.database_id != history.database_id
        || report.metadata.commit_sequence != history.commit_sequence
        || source_report.metadata.database_id != report.metadata.database_id
        || source_report.metadata.commit_sequence != report.metadata.commit_sequence
        || source_report.metadata.expression_version != report.metadata.expression_version
        || source_report.metadata.builtin_semantics_version
            != report.metadata.builtin_semantics_version
        || source_report.metadata.lifecycle != report.metadata.lifecycle
        || report.catalog_records != source_report.catalog_records + history.tombstone_ids.len()
    {
        return Err(MigrationError::InvalidSource);
    }
    let source_read = source.begin_read().map_err(|_| MigrationError::Storage)?;
    let staging_read = staging.begin_read().map_err(|_| MigrationError::Storage)?;

    macro_rules! require_table_equal {
        ($definition:expr) => {{
            let source_table = source_read
                .open_table($definition)
                .map_err(|_| MigrationError::Storage)?;
            let staging_table = staging_read
                .open_table($definition)
                .map_err(|_| MigrationError::Storage)?;
            let mut source_records = source_table.iter().map_err(|_| MigrationError::Storage)?;
            let mut staging_records = staging_table.iter().map_err(|_| MigrationError::Storage)?;
            loop {
                match (source_records.next(), staging_records.next()) {
                    (None, None) => break,
                    (Some(source_record), Some(staging_record)) => {
                        let (source_key, source_value) =
                            source_record.map_err(|_| MigrationError::Storage)?;
                        let (staging_key, staging_value) =
                            staging_record.map_err(|_| MigrationError::Storage)?;
                        if source_key.value() != staging_key.value()
                            || source_value.value() != staging_value.value()
                        {
                            return Err(MigrationError::InvalidSource);
                        }
                    }
                    _ => return Err(MigrationError::InvalidSource),
                }
            }
        }};
    }

    require_table_equal!(crate::schema::ROWS);
    require_table_equal!(crate::schema::INDEXES);
    require_table_equal!(crate::schema::ROW_INDEXES);
    require_table_equal!(crate::schema::PRINCIPAL_NAMES);
    require_table_equal!(crate::schema::SCHEMA_NAMES);
    require_table_equal!(crate::schema::RELATION_NAMES);
    require_table_equal!(crate::schema::COLUMN_LOOKUPS);
    require_table_equal!(crate::schema::INDEX_NAMES);
    require_table_equal!(crate::schema::TABLE_INDEXES);
    require_table_equal!(crate::schema::TABLE_CONSTRAINTS);
    require_table_equal!(crate::schema::OBJECT_OWNERS);
    require_table_equal!(crate::schema::MEMBERSHIPS);
    require_table_equal!(crate::schema::GRANTS);
    require_table_equal!(crate::schema::POLICIES);
    require_table_equal!(crate::schema::DEPENDENCIES);
    require_table_equal!(crate::schema::OUTCOMES);
    require_table_equal!(crate::schema::AUDIT);
    require_table_equal!(crate::schema::STATS);

    let source_catalog = source_read
        .open_table(CATALOG)
        .map_err(|_| MigrationError::Storage)?;
    let staging_catalog = staging_read
        .open_table(CATALOG)
        .map_err(|_| MigrationError::Storage)?;
    let database_key = id_key(1, history.database_id);
    let source_descriptor = source_catalog
        .get(database_key.as_slice())
        .map_err(|_| MigrationError::Storage)?
        .ok_or(MigrationError::InvalidCatalog)?;
    let mut descriptor = decode_registered_envelope(source_descriptor.value())
        .map_err(|_| MigrationError::InvalidCatalog)?;
    if descriptor.logical_key != database_key {
        return Err(MigrationError::InvalidCatalog);
    }
    update_database_descriptor(&mut descriptor.fields)?;
    let expected_descriptor = encode_envelope(
        &family_schema(1).ok_or(MigrationError::InvalidCatalog)?,
        &database_key,
        &descriptor.fields,
    )
    .map_err(|_| MigrationError::InvalidCatalog)?;
    for entry in source_catalog.iter().map_err(|_| MigrationError::Storage)? {
        let (key, value) = entry.map_err(|_| MigrationError::Storage)?;
        let expected = if key.value() == database_key {
            expected_descriptor.as_slice()
        } else {
            value.value()
        };
        let actual = staging_catalog
            .get(key.value())
            .map_err(|_| MigrationError::Storage)?
            .ok_or(MigrationError::InvalidSource)?;
        if actual.value() != expected {
            return Err(MigrationError::InvalidSource);
        }
    }
    let mut actual_tombstones = Vec::with_capacity(history.tombstone_ids.len());
    for entry in staging_catalog
        .iter()
        .map_err(|_| MigrationError::Storage)?
    {
        let (key, _) = entry.map_err(|_| MigrationError::Storage)?;
        if key.value().starts_with(&17u16.to_be_bytes()) {
            if key.value().len() != 19 {
                return Err(MigrationError::InvalidCatalog);
            }
            let mut id = [0; 16];
            id.copy_from_slice(&key.value()[3..19]);
            actual_tombstones.push(id);
        }
    }
    if actual_tombstones != history.tombstone_ids {
        return Err(MigrationError::InvalidHistory(
            IdentityHistoryError::InvalidHistory,
        ));
    }
    Ok(report)
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
