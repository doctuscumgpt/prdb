//! Read-only integrity checks for a recovered redb image.

use crate::audit::AuditEvent;
use crate::authorization::{MAX_MEMBERSHIP_EDGES, Membership, validate_membership_graph};
use crate::catalog::{
    CatalogRecord, decode_builtin_scalar_type, decode_registered_envelope,
    validate_builtin_type_profile, validate_owned_child_graph,
};
use crate::credentials::{CredentialVerifier, decode_verifier_record};
use crate::expression::decode_catalog_expression_record;
use crate::index_key::{DerivedIndexKey, derive_index_key, encode_sql_tuple};
use crate::index_locator::IndexLocator;
use crate::metadata::{EngineMetadata, Lifecycle};
use crate::outcome::{InvocationOutcome, OutcomeStatus};
use crate::row_record::RowRecord;
use crate::schema::{
    AUDIT, CATALOG, COLUMN_LOOKUPS, DEPENDENCIES, ENGINE_META, GRANTS, INDEX_NAMES, INDEXES,
    MEMBERSHIPS, OBJECT_OWNERS, OUTCOMES, POLICIES, PRINCIPAL_NAMES, RELATION_NAMES, ROW_INDEXES,
    ROWS, SCHEMA_NAMES, STATS, TABLE_CONSTRAINTS, TABLE_INDEXES,
};
use crate::tuple::{ColumnSchema, decode_tuple};
use redb::{
    Database, ReadOnlyDatabase, ReadTransaction, ReadableDatabase, ReadableTable,
    ReadableTableMetadata, TableHandle,
};
use std::collections::{HashMap, HashSet};

const METADATA_KEY: &[u8] = b"format";
const MAX_CATALOG_RECORDS_PER_FAMILY: usize = 65_536;
const MAX_FOREIGN_KEYS_PER_DATABASE: usize = 256;
const MAX_FOREIGN_KEY_ROW_VISITS_PER_STARTUP: usize = 65_536;
type IndexDefinitions = HashMap<[u8; 16], [u8; 16]>;
type IndexColumns = HashMap<[u8; 16], Vec<[u8; 16]>>;
type UniqueIndexIds = HashSet<[u8; 16]>;

#[derive(Clone, Debug)]
struct PersistedTableSchema {
    schema_version: u64,
    column_ids: Vec<[u8; 16]>,
    columns: Vec<ColumnSchema>,
}

#[derive(Clone, Debug)]
struct PersistedColumnSchema {
    column_id: [u8; 16],
    ordinal: u32,
    type_id: [u8; 16],
    parameters: Vec<u8>,
    nullable: bool,
    introduced_schema_version: u64,
}

type PersistedTableSchemas = HashMap<[u8; 16], PersistedTableSchema>;

#[derive(Clone, Debug)]
struct PersistedForeignKey {
    source_table_id: [u8; 16],
    source_ordinals: Vec<usize>,
    target_index_id: [u8; 16],
    comparison_columns: Vec<ColumnSchema>,
}

struct PersistedCatalog {
    record_count: usize,
    index_definitions: IndexDefinitions,
    index_columns: IndexColumns,
    unique_index_ids: UniqueIndexIds,
    table_schemas: PersistedTableSchemas,
    foreign_keys: Vec<PersistedForeignKey>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StorageHealthReport {
    pub metadata: EngineMetadata,
    pub catalog_records: usize,
    pub rows: usize,
    pub index_entries: usize,
    pub outcomes: usize,
    pub audit_events: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StartupError {
    Storage,
    InvalidMetadata,
    UnsupportedFormat,
    NotReady,
    UnexpectedMetadataKey,
    CatalogLimit,
    InvalidCatalog,
    InvalidRow,
    InvalidForeignKey,
    ForeignKeyValidationLimitExceeded,
    SequenceAhead,
    InvalidIndex,
    InvalidOutcome,
    InvalidAudit,
    AuditCapacityExceeded,
    CountOverflow,
}

impl std::fmt::Display for StartupError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "storage startup validation failed: {self:?}")
    }
}

impl std::error::Error for StartupError {}

/// Validate the recovered image's table set, version state, bootstrap mapping,
/// record envelopes, and implemented references against one pinned redb
/// snapshot. This may rebuild stale derived indexes from authoritative catalog
/// rows. It does not certify authenticated service admission.
pub fn validate_persisted_records(
    database: &Database,
) -> Result<StorageHealthReport, StartupError> {
    validate_persisted_records_for_lifecycle(database, Lifecycle::Ready)
}

/// Validate a READY image without rebuilding derived lookup tables or writing
/// to the redb file. This is used by offline backup verification.
pub fn validate_persisted_records_read_only(
    database: &ReadOnlyDatabase,
) -> Result<StorageHealthReport, StartupError> {
    let read = database.begin_read().map_err(storage_error)?;
    validate_persisted_records_snapshot(&read, Lifecycle::Ready)
}

pub(crate) fn validate_initializing_records(
    database: &Database,
) -> Result<StorageHealthReport, StartupError> {
    validate_persisted_records_for_lifecycle(database, Lifecycle::Initializing)
}

fn validate_persisted_records_for_lifecycle(
    database: &Database,
    required_lifecycle: Lifecycle,
) -> Result<StorageHealthReport, StartupError> {
    // Compatibility and lifecycle gates must run before rebuilding disposable
    // indexes so an unsupported image is rejected without database writes.
    {
        let read = database.begin_read().map_err(storage_error)?;
        let metadata = read_metadata(&read)?;
        if !metadata.supports_current_formats() {
            return Err(StartupError::UnsupportedFormat);
        }
        if metadata.lifecycle != required_lifecycle {
            return Err(StartupError::NotReady);
        }
    }
    ensure_principal_name_index(database)?;
    ensure_schema_name_index(database)?;
    ensure_relation_name_index(database)?;
    ensure_column_lookup_index(database)?;
    ensure_index_name_index(database)?;
    ensure_table_index_index(database)?;
    ensure_table_constraint_index(database)?;
    ensure_object_owner_index(database)?;
    ensure_membership_index(database)?;
    ensure_grant_index(database)?;
    ensure_policy_index(database)?;
    ensure_dependency_index(database)?;
    let read = database.begin_read().map_err(storage_error)?;
    validate_persisted_records_snapshot(&read, required_lifecycle)
}

fn validate_persisted_records_snapshot(
    read: &ReadTransaction,
    required_lifecycle: Lifecycle,
) -> Result<StorageHealthReport, StartupError> {
    validate_tables(read)?;
    validate_name_domains(read)?;
    let metadata = read_metadata(read)?;
    if !metadata.supports_current_formats() {
        return Err(StartupError::UnsupportedFormat);
    }
    if metadata.lifecycle != required_lifecycle {
        return Err(StartupError::NotReady);
    }

    let catalog = validate_catalog(read, &metadata)?;
    let rows = validate_rows(read, metadata.commit_sequence, &catalog.table_schemas)?;
    let index_entries = validate_indexes(
        read,
        metadata.commit_sequence,
        &catalog.index_definitions,
        &catalog.index_columns,
        &catalog.unique_index_ids,
        &catalog.table_schemas,
    )?;
    validate_foreign_key_rows(read, &catalog.foreign_keys, &catalog.table_schemas)?;
    let outcomes = validate_outcomes(read, metadata.commit_sequence)?;
    let audit_events = validate_audit(read)?;

    Ok(StorageHealthReport {
        metadata,
        catalog_records: catalog.record_count,
        rows,
        index_entries,
        outcomes,
        audit_events,
    })
}

fn collect_principal_name_entries<T>(
    catalog: &T,
) -> Result<HashMap<Vec<u8>, [u8; 16]>, StartupError>
where
    T: ReadableTable<&'static [u8], &'static [u8]>,
{
    let prefix = 10u16.to_be_bytes();
    let mut entries = HashMap::new();
    for entry in catalog.range(prefix.as_slice()..).map_err(storage_error)? {
        let (key, value) = entry.map_err(storage_error)?;
        let key = key.value();
        if !key.starts_with(&prefix) {
            break;
        }
        if key.len() != 19 || key[2] != 1 {
            return Err(StartupError::InvalidCatalog);
        }
        let record =
            decode_registered_envelope(value.value()).map_err(|_| StartupError::InvalidCatalog)?;
        if record.family_code != 10 || record.logical_key != key {
            return Err(StartupError::InvalidCatalog);
        }
        let name = record
            .fields
            .iter()
            .find(|field| field.tag == 2)
            .ok_or(StartupError::InvalidCatalog)?;
        let principal_id: [u8; 16] = key[3..19]
            .try_into()
            .map_err(|_| StartupError::InvalidCatalog)?;
        if name.payload.is_empty()
            || name.payload.len() > 4096
            || entries.insert(name.payload.clone(), principal_id).is_some()
        {
            return Err(StartupError::InvalidCatalog);
        }
    }
    Ok(entries)
}

/// Rebuild the derived principal-name lookup if it is missing or stale. The
/// family-10 records remain authoritative; this never changes catalog data.
fn ensure_principal_name_index(database: &Database) -> Result<(), StartupError> {
    let read = database.begin_read().map_err(storage_error)?;
    let has_index = read
        .list_tables()
        .map_err(storage_error)?
        .any(|table| table.name() == PRINCIPAL_NAMES.name());
    if has_index {
        let catalog = read.open_table(CATALOG).map_err(storage_error)?;
        let expected = collect_principal_name_entries(&catalog)?;
        let index = read.open_table(PRINCIPAL_NAMES).map_err(storage_error)?;
        let mut actual = HashMap::new();
        let mut well_formed = true;
        for entry in index.iter().map_err(storage_error)? {
            let (name, id) = entry.map_err(storage_error)?;
            match id.value().try_into() {
                Ok(id) => {
                    actual.insert(name.value().to_vec(), id);
                }
                Err(_) => well_formed = false,
            }
        }
        if well_formed && actual == expected {
            return Ok(());
        }
    }
    drop(read);

    let write = database.begin_write().map_err(storage_error)?;
    let catalog = write.open_table(CATALOG).map_err(storage_error)?;
    let entries = collect_principal_name_entries(&catalog)?;
    let mut name_index = write.open_table(PRINCIPAL_NAMES).map_err(storage_error)?;
    let stale_keys = name_index
        .iter()
        .map_err(storage_error)?
        .map(|entry| {
            entry
                .map(|(name, _)| name.value().to_vec())
                .map_err(storage_error)
        })
        .collect::<Result<Vec<_>, _>>()?;
    for name in stale_keys {
        name_index.remove(name.as_slice()).map_err(storage_error)?;
    }
    for (name, id) in entries {
        name_index
            .insert(name.as_slice(), &id[..])
            .map_err(storage_error)?;
    }
    drop(name_index);
    drop(catalog);
    write.commit().map_err(storage_error)?;
    Ok(())
}

fn collect_schema_name_entries<T>(catalog: &T) -> Result<HashMap<Vec<u8>, [u8; 16]>, StartupError>
where
    T: ReadableTable<&'static [u8], &'static [u8]>,
{
    let prefix = 2u16.to_be_bytes();
    let mut entries = HashMap::new();
    let mut count = 0usize;
    for entry in catalog.range(prefix.as_slice()..).map_err(storage_error)? {
        let (key, value) = entry.map_err(storage_error)?;
        let key = key.value();
        if !key.starts_with(&prefix) {
            break;
        }
        if key.len() != 19 || key[2] != 1 {
            continue;
        }
        let record =
            decode_registered_envelope(value.value()).map_err(|_| StartupError::InvalidCatalog)?;
        if record.family_code != 2 || record.logical_key != key {
            return Err(StartupError::InvalidCatalog);
        }
        if field_u16(&record, 1)? != 1 {
            continue;
        }
        count += 1;
        if count > MAX_CATALOG_RECORDS_PER_FAMILY {
            return Err(StartupError::InvalidCatalog);
        }
        let name = record
            .fields
            .iter()
            .find(|field| field.tag == 3)
            .ok_or(StartupError::InvalidCatalog)?
            .payload
            .clone();
        let schema_id: [u8; 16] = key[3..19]
            .try_into()
            .map_err(|_| StartupError::InvalidCatalog)?;
        if name.is_empty() || name.len() > 4096 || entries.insert(name, schema_id).is_some() {
            return Err(StartupError::InvalidCatalog);
        }
    }
    Ok(entries)
}

/// Rebuild the derived schema-name lookup from authoritative family-2 rows.
fn ensure_schema_name_index(database: &Database) -> Result<(), StartupError> {
    let read = database.begin_read().map_err(storage_error)?;
    let has_index = read
        .list_tables()
        .map_err(storage_error)?
        .any(|table| table.name() == SCHEMA_NAMES.name());
    if has_index {
        let catalog = read.open_table(CATALOG).map_err(storage_error)?;
        let expected = collect_schema_name_entries(&catalog)?;
        let index = read.open_table(SCHEMA_NAMES).map_err(storage_error)?;
        let mut actual = HashMap::new();
        let mut well_formed = true;
        for entry in index.iter().map_err(storage_error)? {
            let (name, id) = entry.map_err(storage_error)?;
            match <[u8; 16]>::try_from(id.value()) {
                Ok(id) => {
                    actual.insert(name.value().to_vec(), id);
                }
                Err(_) => well_formed = false,
            }
        }
        if well_formed && actual == expected {
            return Ok(());
        }
    }
    drop(read);

    let write = database.begin_write().map_err(storage_error)?;
    let catalog = write.open_table(CATALOG).map_err(storage_error)?;
    let entries = collect_schema_name_entries(&catalog)?;
    let mut name_index = write.open_table(SCHEMA_NAMES).map_err(storage_error)?;
    let stale_keys = name_index
        .iter()
        .map_err(storage_error)?
        .map(|entry| {
            entry
                .map(|(name, _)| name.value().to_vec())
                .map_err(storage_error)
        })
        .collect::<Result<Vec<_>, _>>()?;
    for name in stale_keys {
        name_index.remove(name.as_slice()).map_err(storage_error)?;
    }
    for (name, id) in entries {
        name_index
            .insert(name.as_slice(), &id[..])
            .map_err(storage_error)?;
    }
    drop(name_index);
    drop(catalog);
    write.commit().map_err(storage_error)?;
    Ok(())
}

fn collect_relation_name_entries<T>(catalog: &T) -> Result<HashMap<Vec<u8>, [u8; 16]>, StartupError>
where
    T: ReadableTable<&'static [u8], &'static [u8]>,
{
    let prefix = 5u16.to_be_bytes();
    let mut entries = HashMap::new();
    let mut count = 0usize;
    for entry in catalog.range(prefix.as_slice()..).map_err(storage_error)? {
        let (key, value) = entry.map_err(storage_error)?;
        let key = key.value();
        if !key.starts_with(&prefix) {
            break;
        }
        let table_record =
            decode_registered_envelope(value.value()).map_err(|_| StartupError::InvalidCatalog)?;
        if table_record.family_code != 5 || table_record.logical_key != key || key.len() != 19 {
            return Err(StartupError::InvalidCatalog);
        }
        count += 1;
        if count > MAX_CATALOG_RECORDS_PER_FAMILY {
            return Err(StartupError::InvalidCatalog);
        }
        let relation_id: [u8; 16] = key[3..19]
            .try_into()
            .map_err(|_| StartupError::InvalidCatalog)?;
        let schema_id = field_id(&table_record, 1)?;
        let object_key = {
            let mut object_key = 2u16.to_be_bytes().to_vec();
            object_key.push(1);
            object_key.extend_from_slice(&relation_id);
            object_key
        };
        let object_bytes = catalog
            .get(object_key.as_slice())
            .map_err(storage_error)?
            .ok_or(StartupError::InvalidCatalog)?;
        let object = decode_registered_envelope(object_bytes.value())
            .map_err(|_| StartupError::InvalidCatalog)?;
        if object.family_code != 2
            || object.logical_key != object_key
            || field_u16(&object, 1)? != 3
            || object
                .fields
                .iter()
                .find(|field| field.tag == 2)
                .is_some_and(|parent| parent.payload.as_slice() != schema_id)
        {
            return Err(StartupError::InvalidCatalog);
        }
        let name = object
            .fields
            .iter()
            .find(|field| field.tag == 3)
            .ok_or(StartupError::InvalidCatalog)?
            .payload
            .clone();
        if name.is_empty() || name.len() > 4096 {
            return Err(StartupError::InvalidCatalog);
        }
        let mut index_key = Vec::with_capacity(16 + name.len());
        index_key.extend_from_slice(&schema_id);
        index_key.extend_from_slice(&name);
        if entries.insert(index_key, relation_id).is_some() {
            return Err(StartupError::InvalidCatalog);
        }
    }
    Ok(entries)
}

/// Rebuild schema-scoped relation-name entries from authoritative table and object rows.
fn ensure_relation_name_index(database: &Database) -> Result<(), StartupError> {
    let read = database.begin_read().map_err(storage_error)?;
    let has_index = read
        .list_tables()
        .map_err(storage_error)?
        .any(|table| table.name() == RELATION_NAMES.name());
    if has_index {
        let catalog = read.open_table(CATALOG).map_err(storage_error)?;
        let expected = collect_relation_name_entries(&catalog)?;
        let index = read.open_table(RELATION_NAMES).map_err(storage_error)?;
        let mut actual = HashMap::new();
        let mut well_formed = true;
        for entry in index.iter().map_err(storage_error)? {
            let (key, id) = entry.map_err(storage_error)?;
            match <[u8; 16]>::try_from(id.value()) {
                Ok(id) => {
                    actual.insert(key.value().to_vec(), id);
                }
                Err(_) => well_formed = false,
            }
        }
        if well_formed && actual == expected {
            return Ok(());
        }
    }
    drop(read);
    let write = database.begin_write().map_err(storage_error)?;
    let catalog = write.open_table(CATALOG).map_err(storage_error)?;
    let entries = collect_relation_name_entries(&catalog)?;
    let mut index = write.open_table(RELATION_NAMES).map_err(storage_error)?;
    let stale_keys = index
        .iter()
        .map_err(storage_error)?
        .map(|entry| {
            entry
                .map(|(key, _)| key.value().to_vec())
                .map_err(storage_error)
        })
        .collect::<Result<Vec<_>, _>>()?;
    for key in stale_keys {
        index.remove(key.as_slice()).map_err(storage_error)?;
    }
    for (key, id) in entries {
        index
            .insert(key.as_slice(), &id[..])
            .map_err(storage_error)?;
    }
    drop(index);
    drop(catalog);
    write.commit().map_err(storage_error)?;
    Ok(())
}

fn collect_column_lookup_entries<T>(catalog: &T) -> Result<HashMap<Vec<u8>, [u8; 16]>, StartupError>
where
    T: ReadableTable<&'static [u8], &'static [u8]>,
{
    let prefix = 6u16.to_be_bytes();
    let mut entries = HashMap::new();
    for entry in catalog.range(prefix.as_slice()..).map_err(storage_error)? {
        let (key, value) = entry.map_err(storage_error)?;
        let key = key.value();
        if !key.starts_with(&prefix) {
            break;
        }
        let record =
            decode_registered_envelope(value.value()).map_err(|_| StartupError::InvalidCatalog)?;
        if record.family_code != 6 || record.logical_key != key || key.len() != 19 || key[2] != 1 {
            return Err(StartupError::InvalidCatalog);
        }
        let column_id: [u8; 16] = key[3..19]
            .try_into()
            .map_err(|_| StartupError::InvalidCatalog)?;
        let table_id = field_id(&record, 1)?;
        let ordinal = field_u32(&record, 2)?;
        let name = field_payload(&record, 3)?;
        if column_id == [0; 16] || table_id == [0; 16] || name.is_empty() || name.len() > 4096 {
            return Err(StartupError::InvalidCatalog);
        }
        let mut by_name = vec![1];
        by_name.extend_from_slice(&table_id);
        by_name.extend_from_slice(name);
        let mut by_ordinal = vec![2];
        by_ordinal.extend_from_slice(&table_id);
        by_ordinal.extend_from_slice(&ordinal.to_be_bytes());
        if entries.insert(by_name, column_id).is_some()
            || entries.insert(by_ordinal, column_id).is_some()
        {
            return Err(StartupError::InvalidCatalog);
        }
    }
    Ok(entries)
}

/// Rebuild both table/name and table/ordinal lookups from family-6 records.
fn ensure_column_lookup_index(database: &Database) -> Result<(), StartupError> {
    let read = database.begin_read().map_err(storage_error)?;
    let has_index = read
        .list_tables()
        .map_err(storage_error)?
        .any(|table| table.name() == COLUMN_LOOKUPS.name());
    if has_index {
        let catalog = read.open_table(CATALOG).map_err(storage_error)?;
        let expected = collect_column_lookup_entries(&catalog)?;
        let index = read.open_table(COLUMN_LOOKUPS).map_err(storage_error)?;
        let mut actual = HashMap::new();
        let mut well_formed = true;
        for entry in index.iter().map_err(storage_error)? {
            let (key, value) = entry.map_err(storage_error)?;
            if value.value().len() != 16 {
                well_formed = false;
                continue;
            }
            let column_id: [u8; 16] = value.value().try_into().unwrap();
            if actual.insert(key.value().to_vec(), column_id).is_some() {
                well_formed = false;
            }
        }
        if well_formed && actual == expected {
            return Ok(());
        }
    }
    drop(read);
    let write = database.begin_write().map_err(storage_error)?;
    let catalog = write.open_table(CATALOG).map_err(storage_error)?;
    let expected = collect_column_lookup_entries(&catalog)?;
    let mut index = write.open_table(COLUMN_LOOKUPS).map_err(storage_error)?;
    let stale_keys = index
        .iter()
        .map_err(storage_error)?
        .map(|entry| {
            entry
                .map(|(key, _)| key.value().to_vec())
                .map_err(storage_error)
        })
        .collect::<Result<Vec<_>, _>>()?;
    for key in stale_keys {
        index.remove(key.as_slice()).map_err(storage_error)?;
    }
    for (key, column_id) in expected {
        index
            .insert(key.as_slice(), &column_id[..])
            .map_err(storage_error)?;
    }
    drop(index);
    drop(catalog);
    write.commit().map_err(storage_error)?;
    Ok(())
}

fn collect_index_name_entries<T>(catalog: &T) -> Result<HashMap<Vec<u8>, [u8; 16]>, StartupError>
where
    T: ReadableTable<&'static [u8], &'static [u8]>,
{
    let prefix = 2u16.to_be_bytes();
    let mut entries = HashMap::new();
    for entry in catalog.range(prefix.as_slice()..).map_err(storage_error)? {
        let (key, value) = entry.map_err(storage_error)?;
        let key = key.value();
        if !key.starts_with(&prefix) {
            break;
        }
        let record =
            decode_registered_envelope(value.value()).map_err(|_| StartupError::InvalidCatalog)?;
        if record.family_code != 2 || record.logical_key != key || key.len() != 19 || key[2] != 1 {
            return Err(StartupError::InvalidCatalog);
        }
        if field_u16(&record, 1)? != 7 {
            continue;
        }
        let index_id: [u8; 16] = key[3..19]
            .try_into()
            .map_err(|_| StartupError::InvalidCatalog)?;
        let Some(schema_id) = field_optional_id(&record, 2)? else {
            continue;
        };
        let name = field_payload(&record, 3)?;
        if index_id == [0; 16] || schema_id == [0; 16] || name.is_empty() || name.len() > 4096 {
            return Err(StartupError::InvalidCatalog);
        }
        let mut name_key = Vec::with_capacity(16 + name.len());
        name_key.extend_from_slice(&schema_id);
        name_key.extend_from_slice(name);
        if entries.insert(name_key, index_id).is_some() {
            return Err(StartupError::InvalidCatalog);
        }
    }
    Ok(entries)
}

/// Rebuild schema-scoped index-name entries from authoritative family-2 objects.
fn ensure_index_name_index(database: &Database) -> Result<(), StartupError> {
    let read = database.begin_read().map_err(storage_error)?;
    let has_index = read
        .list_tables()
        .map_err(storage_error)?
        .any(|table| table.name() == INDEX_NAMES.name());
    if has_index {
        let catalog = read.open_table(CATALOG).map_err(storage_error)?;
        let expected = collect_index_name_entries(&catalog)?;
        let index = read.open_table(INDEX_NAMES).map_err(storage_error)?;
        let mut actual = HashMap::new();
        let mut well_formed = true;
        for entry in index.iter().map_err(storage_error)? {
            let (key, value) = entry.map_err(storage_error)?;
            if value.value().len() != 16 {
                well_formed = false;
                continue;
            }
            let index_id: [u8; 16] = value.value().try_into().unwrap();
            if actual.insert(key.value().to_vec(), index_id).is_some() {
                well_formed = false;
            }
        }
        if well_formed && actual == expected {
            return Ok(());
        }
    }
    drop(read);
    let write = database.begin_write().map_err(storage_error)?;
    let catalog = write.open_table(CATALOG).map_err(storage_error)?;
    let expected = collect_index_name_entries(&catalog)?;
    let mut index = write.open_table(INDEX_NAMES).map_err(storage_error)?;
    let stale_keys = index
        .iter()
        .map_err(storage_error)?
        .map(|entry| {
            entry
                .map(|(key, _)| key.value().to_vec())
                .map_err(storage_error)
        })
        .collect::<Result<Vec<_>, _>>()?;
    for key in stale_keys {
        index.remove(key.as_slice()).map_err(storage_error)?;
    }
    for (key, index_id) in expected {
        index
            .insert(key.as_slice(), &index_id[..])
            .map_err(storage_error)?;
    }
    drop(index);
    drop(catalog);
    write.commit().map_err(storage_error)?;
    Ok(())
}

fn collect_table_index_entries<T>(catalog: &T) -> Result<HashSet<Vec<u8>>, StartupError>
where
    T: ReadableTable<&'static [u8], &'static [u8]>,
{
    let prefix = 9u16.to_be_bytes();
    let mut entries = HashSet::new();
    for entry in catalog.range(prefix.as_slice()..).map_err(storage_error)? {
        let (key, value) = entry.map_err(storage_error)?;
        let key = key.value();
        if !key.starts_with(&prefix) {
            break;
        }
        let record =
            decode_registered_envelope(value.value()).map_err(|_| StartupError::InvalidCatalog)?;
        if record.family_code != 9 || record.logical_key != key || key.len() != 19 || key[2] != 1 {
            return Err(StartupError::InvalidCatalog);
        }
        let table_id = field_id(&record, 1)?;
        let index_id: [u8; 16] = key[3..19]
            .try_into()
            .map_err(|_| StartupError::InvalidCatalog)?;
        if table_id == [0; 16] || index_id == [0; 16] {
            return Err(StartupError::InvalidCatalog);
        }
        let mut index_key = Vec::with_capacity(32);
        index_key.extend_from_slice(&table_id);
        index_key.extend_from_slice(&index_id);
        if !entries.insert(index_key) {
            return Err(StartupError::InvalidCatalog);
        }
    }
    Ok(entries)
}

/// Rebuild table-scoped family-9 definition lookups from authoritative rows.
fn ensure_table_index_index(database: &Database) -> Result<(), StartupError> {
    let read = database.begin_read().map_err(storage_error)?;
    let has_index = read
        .list_tables()
        .map_err(storage_error)?
        .any(|table| table.name() == TABLE_INDEXES.name());
    if has_index {
        let catalog = read.open_table(CATALOG).map_err(storage_error)?;
        let expected = collect_table_index_entries(&catalog)?;
        let index = read.open_table(TABLE_INDEXES).map_err(storage_error)?;
        let mut actual = HashSet::new();
        let mut well_formed = true;
        for entry in index.iter().map_err(storage_error)? {
            let (key, value) = entry.map_err(storage_error)?;
            if key.value().len() != 32 || !value.value().is_empty() {
                well_formed = false;
            }
            actual.insert(key.value().to_vec());
        }
        if well_formed && actual == expected {
            return Ok(());
        }
    }
    drop(read);
    let write = database.begin_write().map_err(storage_error)?;
    let catalog = write.open_table(CATALOG).map_err(storage_error)?;
    let expected = collect_table_index_entries(&catalog)?;
    let mut index = write.open_table(TABLE_INDEXES).map_err(storage_error)?;
    let stale_keys = index
        .iter()
        .map_err(storage_error)?
        .map(|entry| {
            entry
                .map(|(key, _)| key.value().to_vec())
                .map_err(storage_error)
        })
        .collect::<Result<Vec<_>, _>>()?;
    for key in stale_keys {
        index.remove(key.as_slice()).map_err(storage_error)?;
    }
    for key in expected {
        index
            .insert(key.as_slice(), &b""[..])
            .map_err(storage_error)?;
    }
    drop(index);
    drop(catalog);
    write.commit().map_err(storage_error)?;
    Ok(())
}

fn collect_table_constraint_entries<T>(catalog: &T) -> Result<HashSet<Vec<u8>>, StartupError>
where
    T: ReadableTable<&'static [u8], &'static [u8]>,
{
    let prefix = 8u16.to_be_bytes();
    let mut entries = HashSet::new();
    for entry in catalog.range(prefix.as_slice()..).map_err(storage_error)? {
        let (key, value) = entry.map_err(storage_error)?;
        let key = key.value();
        if !key.starts_with(&prefix) {
            break;
        }
        let record =
            decode_registered_envelope(value.value()).map_err(|_| StartupError::InvalidCatalog)?;
        if record.family_code != 8 || record.logical_key != key || key.len() != 19 || key[2] != 1 {
            return Err(StartupError::InvalidCatalog);
        }
        let table_id = field_id(&record, 1)?;
        let constraint_id: [u8; 16] = key[3..19]
            .try_into()
            .map_err(|_| StartupError::InvalidCatalog)?;
        if table_id == [0; 16] || constraint_id == [0; 16] {
            return Err(StartupError::InvalidCatalog);
        }
        let mut index_key = Vec::with_capacity(32);
        index_key.extend_from_slice(&table_id);
        index_key.extend_from_slice(&constraint_id);
        if !entries.insert(index_key) {
            return Err(StartupError::InvalidCatalog);
        }
    }
    Ok(entries)
}

/// Rebuild table-scoped family-8 constraint lookups from authoritative rows.
fn ensure_table_constraint_index(database: &Database) -> Result<(), StartupError> {
    let read = database.begin_read().map_err(storage_error)?;
    let has_index = read
        .list_tables()
        .map_err(storage_error)?
        .any(|table| table.name() == TABLE_CONSTRAINTS.name());
    if has_index {
        let catalog = read.open_table(CATALOG).map_err(storage_error)?;
        let expected = collect_table_constraint_entries(&catalog)?;
        let index = read.open_table(TABLE_CONSTRAINTS).map_err(storage_error)?;
        let mut actual = HashSet::new();
        let mut well_formed = true;
        for entry in index.iter().map_err(storage_error)? {
            let (key, value) = entry.map_err(storage_error)?;
            if key.value().len() != 32 || !value.value().is_empty() {
                well_formed = false;
            }
            actual.insert(key.value().to_vec());
        }
        if well_formed && actual == expected {
            return Ok(());
        }
    }
    drop(read);
    let write = database.begin_write().map_err(storage_error)?;
    let catalog = write.open_table(CATALOG).map_err(storage_error)?;
    let expected = collect_table_constraint_entries(&catalog)?;
    let mut index = write.open_table(TABLE_CONSTRAINTS).map_err(storage_error)?;
    let stale_keys = index
        .iter()
        .map_err(storage_error)?
        .map(|entry| {
            entry
                .map(|(key, _)| key.value().to_vec())
                .map_err(storage_error)
        })
        .collect::<Result<Vec<_>, _>>()?;
    for key in stale_keys {
        index.remove(key.as_slice()).map_err(storage_error)?;
    }
    for key in expected {
        index
            .insert(key.as_slice(), &b""[..])
            .map_err(storage_error)?;
    }
    drop(index);
    drop(catalog);
    write.commit().map_err(storage_error)?;
    Ok(())
}

fn collect_object_owner_entries<T>(catalog: &T) -> Result<HashSet<Vec<u8>>, StartupError>
where
    T: ReadableTable<&'static [u8], &'static [u8]>,
{
    let prefix = 2u16.to_be_bytes();
    let mut entries = HashSet::new();
    for entry in catalog.range(prefix.as_slice()..).map_err(storage_error)? {
        let (key, value) = entry.map_err(storage_error)?;
        let key = key.value();
        if !key.starts_with(&prefix) {
            break;
        }
        let record =
            decode_registered_envelope(value.value()).map_err(|_| StartupError::InvalidCatalog)?;
        if record.family_code != 2 || record.logical_key != key || key.len() != 19 || key[2] != 1 {
            return Err(StartupError::InvalidCatalog);
        }
        let object_id: [u8; 16] = key[3..19]
            .try_into()
            .map_err(|_| StartupError::InvalidCatalog)?;
        let owner_id = field_id(&record, 4)?;
        if object_id == [0; 16] || owner_id == [0; 16] {
            return Err(StartupError::InvalidCatalog);
        }
        let mut index_key = Vec::with_capacity(32);
        index_key.extend_from_slice(&owner_id);
        index_key.extend_from_slice(&object_id);
        if !entries.insert(index_key) {
            return Err(StartupError::InvalidCatalog);
        }
    }
    Ok(entries)
}

/// Rebuild family-2 owner lookups from authoritative object records.
fn ensure_object_owner_index(database: &Database) -> Result<(), StartupError> {
    let read = database.begin_read().map_err(storage_error)?;
    let has_index = read
        .list_tables()
        .map_err(storage_error)?
        .any(|table| table.name() == OBJECT_OWNERS.name());
    if has_index {
        let catalog = read.open_table(CATALOG).map_err(storage_error)?;
        let expected = collect_object_owner_entries(&catalog)?;
        let index = read.open_table(OBJECT_OWNERS).map_err(storage_error)?;
        let mut actual = HashSet::new();
        let mut well_formed = true;
        for entry in index.iter().map_err(storage_error)? {
            let (key, value) = entry.map_err(storage_error)?;
            if key.value().len() != 32 || !value.value().is_empty() {
                well_formed = false;
            }
            actual.insert(key.value().to_vec());
        }
        if well_formed && actual == expected {
            return Ok(());
        }
    }
    drop(read);
    let write = database.begin_write().map_err(storage_error)?;
    let catalog = write.open_table(CATALOG).map_err(storage_error)?;
    let expected = collect_object_owner_entries(&catalog)?;
    let mut index = write.open_table(OBJECT_OWNERS).map_err(storage_error)?;
    let stale_keys = index
        .iter()
        .map_err(storage_error)?
        .map(|entry| {
            entry
                .map(|(key, _)| key.value().to_vec())
                .map_err(storage_error)
        })
        .collect::<Result<Vec<_>, _>>()?;
    for key in stale_keys {
        index.remove(key.as_slice()).map_err(storage_error)?;
    }
    for key in expected {
        index
            .insert(key.as_slice(), &b""[..])
            .map_err(storage_error)?;
    }
    drop(index);
    drop(catalog);
    write.commit().map_err(storage_error)?;
    Ok(())
}

fn collect_membership_index_keys<T>(catalog: &T) -> Result<HashSet<Vec<u8>>, StartupError>
where
    T: ReadableTable<&'static [u8], &'static [u8]>,
{
    let prefix = 11u16.to_be_bytes();
    let mut keys = HashSet::new();
    for entry in catalog.range(prefix.as_slice()..).map_err(storage_error)? {
        let (key, value) = entry.map_err(storage_error)?;
        let key = key.value();
        if !key.starts_with(&prefix) {
            break;
        }
        let record =
            decode_registered_envelope(value.value()).map_err(|_| StartupError::InvalidCatalog)?;
        if record.family_code != 11
            || record.logical_key != key
            || key.len() != 36
            || key[2] != 1
            || key[19] != 1
        {
            return Err(StartupError::InvalidCatalog);
        }
        let member_id: [u8; 16] = key[3..19]
            .try_into()
            .map_err(|_| StartupError::InvalidCatalog)?;
        let role_id: [u8; 16] = key[20..36]
            .try_into()
            .map_err(|_| StartupError::InvalidCatalog)?;
        if member_id == [0; 16] || role_id == [0; 16] {
            return Err(StartupError::InvalidCatalog);
        }
        for (direction, first, second) in [(1u8, member_id, role_id), (2u8, role_id, member_id)] {
            let mut index_key = Vec::with_capacity(33);
            index_key.push(direction);
            index_key.extend_from_slice(&first);
            index_key.extend_from_slice(&second);
            if !keys.insert(index_key) {
                return Err(StartupError::InvalidCatalog);
            }
        }
    }
    Ok(keys)
}

/// Rebuild both direct-membership directions from authoritative family-11 rows.
fn ensure_membership_index(database: &Database) -> Result<(), StartupError> {
    let read = database.begin_read().map_err(storage_error)?;
    let has_index = read
        .list_tables()
        .map_err(storage_error)?
        .any(|table| table.name() == MEMBERSHIPS.name());
    if has_index {
        let catalog = read.open_table(CATALOG).map_err(storage_error)?;
        let expected = collect_membership_index_keys(&catalog)?;
        let index = read.open_table(MEMBERSHIPS).map_err(storage_error)?;
        let mut actual = HashSet::new();
        let mut well_formed = true;
        for entry in index.iter().map_err(storage_error)? {
            let (key, value) = entry.map_err(storage_error)?;
            if !value.value().is_empty() {
                well_formed = false;
            }
            actual.insert(key.value().to_vec());
        }
        if well_formed && actual == expected {
            return Ok(());
        }
    }
    drop(read);
    let write = database.begin_write().map_err(storage_error)?;
    let catalog = write.open_table(CATALOG).map_err(storage_error)?;
    let expected = collect_membership_index_keys(&catalog)?;
    let mut index = write.open_table(MEMBERSHIPS).map_err(storage_error)?;
    let stale_keys = index
        .iter()
        .map_err(storage_error)?
        .map(|entry| {
            entry
                .map(|(key, _)| key.value().to_vec())
                .map_err(storage_error)
        })
        .collect::<Result<Vec<_>, _>>()?;
    for key in stale_keys {
        index.remove(key.as_slice()).map_err(storage_error)?;
    }
    for key in expected {
        index
            .insert(key.as_slice(), &b""[..])
            .map_err(storage_error)?;
    }
    drop(index);
    drop(catalog);
    write.commit().map_err(storage_error)?;
    Ok(())
}

fn collect_grant_index_keys<T>(catalog: &T) -> Result<HashSet<Vec<u8>>, StartupError>
where
    T: ReadableTable<&'static [u8], &'static [u8]>,
{
    let prefix = 12u16.to_be_bytes();
    let mut keys = HashSet::new();
    for entry in catalog.range(prefix.as_slice()..).map_err(storage_error)? {
        let (key, value) = entry.map_err(storage_error)?;
        let key = key.value();
        if !key.starts_with(&prefix) {
            break;
        }
        let record =
            decode_registered_envelope(value.value()).map_err(|_| StartupError::InvalidCatalog)?;
        if record.family_code != 12
            || record.logical_key != key
            || key.len() != 59
            || key[2] != 1
            || key[19] != 1
            || key[36] != 1
            || key[39] != 1
            || key[42] != 1
        {
            return Err(StartupError::InvalidCatalog);
        }
        let grantee: [u8; 16] = key[3..19]
            .try_into()
            .map_err(|_| StartupError::InvalidCatalog)?;
        let object: [u8; 16] = key[20..36]
            .try_into()
            .map_err(|_| StartupError::InvalidCatalog)?;
        if grantee == [0; 16] || object == [0; 16] {
            return Err(StartupError::InvalidCatalog);
        }
        for (direction, indexed_id) in [(1u8, grantee), (2u8, object)] {
            let mut index_key = Vec::with_capacity(76);
            index_key.push(direction);
            index_key.extend_from_slice(&indexed_id);
            index_key.extend_from_slice(key);
            if !keys.insert(index_key) {
                return Err(StartupError::InvalidCatalog);
            }
        }
    }
    Ok(keys)
}

/// Rebuild grant indexes ordered by grantee and protected object.
fn ensure_grant_index(database: &Database) -> Result<(), StartupError> {
    let read = database.begin_read().map_err(storage_error)?;
    let has_index = read
        .list_tables()
        .map_err(storage_error)?
        .any(|table| table.name() == GRANTS.name());
    if has_index {
        let catalog = read.open_table(CATALOG).map_err(storage_error)?;
        let expected = collect_grant_index_keys(&catalog)?;
        let index = read.open_table(GRANTS).map_err(storage_error)?;
        let mut actual = HashSet::new();
        let mut well_formed = true;
        for entry in index.iter().map_err(storage_error)? {
            let (key, value) = entry.map_err(storage_error)?;
            if !value.value().is_empty() {
                well_formed = false;
            }
            actual.insert(key.value().to_vec());
        }
        if well_formed && actual == expected {
            return Ok(());
        }
    }
    drop(read);
    let write = database.begin_write().map_err(storage_error)?;
    let catalog = write.open_table(CATALOG).map_err(storage_error)?;
    let expected = collect_grant_index_keys(&catalog)?;
    let mut index = write.open_table(GRANTS).map_err(storage_error)?;
    let stale_keys = index
        .iter()
        .map_err(storage_error)?
        .map(|entry| {
            entry
                .map(|(key, _)| key.value().to_vec())
                .map_err(storage_error)
        })
        .collect::<Result<Vec<_>, _>>()?;
    for key in stale_keys {
        index.remove(key.as_slice()).map_err(storage_error)?;
    }
    for key in expected {
        index
            .insert(key.as_slice(), &b""[..])
            .map_err(storage_error)?;
    }
    drop(index);
    drop(catalog);
    write.commit().map_err(storage_error)?;
    Ok(())
}

fn collect_policy_index_keys<T>(catalog: &T) -> Result<HashSet<Vec<u8>>, StartupError>
where
    T: ReadableTable<&'static [u8], &'static [u8]>,
{
    let prefix = 13u16.to_be_bytes();
    let mut keys = HashSet::new();
    for entry in catalog.range(prefix.as_slice()..).map_err(storage_error)? {
        let (key, value) = entry.map_err(storage_error)?;
        let key = key.value();
        if !key.starts_with(&prefix) {
            break;
        }
        let record =
            decode_registered_envelope(value.value()).map_err(|_| StartupError::InvalidCatalog)?;
        if record.family_code != 13 || record.logical_key != key || key.len() != 19 || key[2] != 1 {
            return Err(StartupError::InvalidCatalog);
        }
        let table_id = field_id(&record, 1)?;
        let operation = field_u16(&record, 3)?;
        let policy_id: [u8; 16] = key[3..19]
            .try_into()
            .map_err(|_| StartupError::InvalidCatalog)?;
        if table_id == [0; 16] || policy_id == [0; 16] || !matches!(operation, 1..=4) {
            return Err(StartupError::InvalidCatalog);
        }
        let mut index_key = Vec::with_capacity(34);
        index_key.extend_from_slice(&table_id);
        index_key.extend_from_slice(&operation.to_be_bytes());
        index_key.extend_from_slice(&policy_id);
        if !keys.insert(index_key) {
            return Err(StartupError::InvalidCatalog);
        }
    }
    Ok(keys)
}

/// Rebuild the table-and-operation policy index from authoritative family-13 rows.
fn ensure_policy_index(database: &Database) -> Result<(), StartupError> {
    let read = database.begin_read().map_err(storage_error)?;
    let has_index = read
        .list_tables()
        .map_err(storage_error)?
        .any(|table| table.name() == POLICIES.name());
    if has_index {
        let catalog = read.open_table(CATALOG).map_err(storage_error)?;
        let expected = collect_policy_index_keys(&catalog)?;
        let index = read.open_table(POLICIES).map_err(storage_error)?;
        let mut actual = HashSet::new();
        let mut well_formed = true;
        for entry in index.iter().map_err(storage_error)? {
            let (key, value) = entry.map_err(storage_error)?;
            if !value.value().is_empty() {
                well_formed = false;
            }
            actual.insert(key.value().to_vec());
        }
        if well_formed && actual == expected {
            return Ok(());
        }
    }
    drop(read);
    let write = database.begin_write().map_err(storage_error)?;
    let catalog = write.open_table(CATALOG).map_err(storage_error)?;
    let expected = collect_policy_index_keys(&catalog)?;
    let mut index = write.open_table(POLICIES).map_err(storage_error)?;
    let stale_keys = index
        .iter()
        .map_err(storage_error)?
        .map(|entry| {
            entry
                .map(|(key, _)| key.value().to_vec())
                .map_err(storage_error)
        })
        .collect::<Result<Vec<_>, _>>()?;
    for key in stale_keys {
        index.remove(key.as_slice()).map_err(storage_error)?;
    }
    for key in expected {
        index
            .insert(key.as_slice(), &b""[..])
            .map_err(storage_error)?;
    }
    drop(index);
    drop(catalog);
    write.commit().map_err(storage_error)?;
    Ok(())
}

fn collect_dependency_index_keys<T>(catalog: &T) -> Result<HashSet<Vec<u8>>, StartupError>
where
    T: ReadableTable<&'static [u8], &'static [u8]>,
{
    let prefix = 15u16.to_be_bytes();
    let mut keys = HashSet::new();
    for entry in catalog.range(prefix.as_slice()..).map_err(storage_error)? {
        let (key, value) = entry.map_err(storage_error)?;
        let key = key.value();
        if !key.starts_with(&prefix) {
            break;
        }
        let record =
            decode_registered_envelope(value.value()).map_err(|_| StartupError::InvalidCatalog)?;
        if record.family_code != 15
            || record.logical_key != key
            || key.len() != 39
            || key[2] != 1
            || key[19] != 1
            || key[36] != 1
        {
            return Err(StartupError::InvalidCatalog);
        }
        let dependent: [u8; 16] = key[3..19]
            .try_into()
            .map_err(|_| StartupError::InvalidCatalog)?;
        let referenced: [u8; 16] = key[20..36]
            .try_into()
            .map_err(|_| StartupError::InvalidCatalog)?;
        let kind = u16::from_be_bytes([key[37], key[38]]);
        if dependent == [0; 16] || referenced == [0; 16] || !matches!(kind, 1..=5) {
            return Err(StartupError::InvalidCatalog);
        }
        for (direction, first, second) in
            [(1u8, dependent, referenced), (2u8, referenced, dependent)]
        {
            let mut index_key = Vec::with_capacity(35);
            index_key.push(direction);
            index_key.extend_from_slice(&first);
            index_key.extend_from_slice(&second);
            index_key.extend_from_slice(&kind.to_be_bytes());
            if !keys.insert(index_key) {
                return Err(StartupError::InvalidCatalog);
            }
        }
    }
    Ok(keys)
}

/// Rebuild dependency lookups in both directions from authoritative family-15 rows.
fn ensure_dependency_index(database: &Database) -> Result<(), StartupError> {
    let read = database.begin_read().map_err(storage_error)?;
    let has_index = read
        .list_tables()
        .map_err(storage_error)?
        .any(|table| table.name() == DEPENDENCIES.name());
    if has_index {
        let catalog = read.open_table(CATALOG).map_err(storage_error)?;
        let expected = collect_dependency_index_keys(&catalog)?;
        let index = read.open_table(DEPENDENCIES).map_err(storage_error)?;
        let mut actual = HashSet::new();
        let mut well_formed = true;
        for entry in index.iter().map_err(storage_error)? {
            let (key, value) = entry.map_err(storage_error)?;
            if !value.value().is_empty() {
                well_formed = false;
            }
            actual.insert(key.value().to_vec());
        }
        if well_formed && actual == expected {
            return Ok(());
        }
    }
    drop(read);
    let write = database.begin_write().map_err(storage_error)?;
    let catalog = write.open_table(CATALOG).map_err(storage_error)?;
    let expected = collect_dependency_index_keys(&catalog)?;
    let mut index = write.open_table(DEPENDENCIES).map_err(storage_error)?;
    let stale_keys = index
        .iter()
        .map_err(storage_error)?
        .map(|entry| {
            entry
                .map(|(key, _)| key.value().to_vec())
                .map_err(storage_error)
        })
        .collect::<Result<Vec<_>, _>>()?;
    for key in stale_keys {
        index.remove(key.as_slice()).map_err(storage_error)?;
    }
    for key in expected {
        index
            .insert(key.as_slice(), &b""[..])
            .map_err(storage_error)?;
    }
    drop(index);
    drop(catalog);
    write.commit().map_err(storage_error)?;
    Ok(())
}

fn storage_error(_error: impl std::fmt::Display) -> StartupError {
    StartupError::Storage
}

fn validate_name_domains(read: &ReadTransaction) -> Result<(), StartupError> {
    let relation_names = read.open_table(RELATION_NAMES).map_err(storage_error)?;
    let index_names = read.open_table(INDEX_NAMES).map_err(storage_error)?;
    for entry in index_names.iter().map_err(storage_error)? {
        let (key, _) = entry.map_err(storage_error)?;
        if relation_names
            .get(key.value())
            .map_err(storage_error)?
            .is_some()
        {
            return Err(StartupError::InvalidCatalog);
        }
    }
    Ok(())
}

fn insert_expression_reference_edge(
    edges: &mut std::collections::HashSet<([u8; 16], [u8; 16])>,
    dependent_id: [u8; 16],
    referenced_id: [u8; 16],
) -> Result<(), StartupError> {
    edges.insert((dependent_id, referenced_id));
    if edges.len() > MAX_CATALOG_RECORDS_PER_FAMILY {
        return Err(StartupError::InvalidCatalog);
    }
    Ok(())
}

fn insert_authority_edge(
    edges: &mut std::collections::HashSet<([u8; 16], [u8; 16])>,
    dependent_id: [u8; 16],
    referenced_id: [u8; 16],
) -> Result<(), StartupError> {
    edges.insert((dependent_id, referenced_id));
    if edges.len() > MAX_CATALOG_RECORDS_PER_FAMILY {
        return Err(StartupError::InvalidCatalog);
    }
    Ok(())
}

fn validate_tables(read: &ReadTransaction) -> Result<(), StartupError> {
    drop(read.open_table(ENGINE_META).map_err(storage_error)?);
    drop(read.open_table(ROWS).map_err(storage_error)?);
    drop(read.open_table(INDEXES).map_err(storage_error)?);
    drop(read.open_table(ROW_INDEXES).map_err(storage_error)?);
    drop(read.open_table(CATALOG).map_err(storage_error)?);
    drop(read.open_table(PRINCIPAL_NAMES).map_err(storage_error)?);
    drop(read.open_table(SCHEMA_NAMES).map_err(storage_error)?);
    drop(read.open_table(RELATION_NAMES).map_err(storage_error)?);
    drop(read.open_table(COLUMN_LOOKUPS).map_err(storage_error)?);
    drop(read.open_table(INDEX_NAMES).map_err(storage_error)?);
    drop(read.open_table(TABLE_INDEXES).map_err(storage_error)?);
    drop(read.open_table(TABLE_CONSTRAINTS).map_err(storage_error)?);
    drop(read.open_table(OBJECT_OWNERS).map_err(storage_error)?);
    drop(read.open_table(MEMBERSHIPS).map_err(storage_error)?);
    drop(read.open_table(GRANTS).map_err(storage_error)?);
    drop(read.open_table(POLICIES).map_err(storage_error)?);
    drop(read.open_table(DEPENDENCIES).map_err(storage_error)?);
    drop(read.open_table(OUTCOMES).map_err(storage_error)?);
    drop(read.open_table(AUDIT).map_err(storage_error)?);
    drop(read.open_table(STATS).map_err(storage_error)?);
    Ok(())
}

fn read_metadata(read: &ReadTransaction) -> Result<EngineMetadata, StartupError> {
    let table = read.open_table(ENGINE_META).map_err(storage_error)?;
    let mut metadata_bytes = None;
    for entry in table.iter().map_err(storage_error)? {
        let (key, value) = entry.map_err(storage_error)?;
        if key.value() != METADATA_KEY {
            return Err(StartupError::UnexpectedMetadataKey);
        }
        if metadata_bytes.replace(value.value().to_vec()).is_some() {
            return Err(StartupError::InvalidMetadata);
        }
    }
    let bytes = metadata_bytes.ok_or(StartupError::InvalidMetadata)?;
    EngineMetadata::decode(&bytes).map_err(|_| StartupError::InvalidMetadata)
}

fn validate_catalog(
    read: &ReadTransaction,
    metadata: &EngineMetadata,
) -> Result<PersistedCatalog, StartupError> {
    let table = read.open_table(CATALOG).map_err(storage_error)?;
    let mut family_counts = [0usize; 18];
    let mut count = 0usize;
    let mut index_definitions = HashMap::new();
    let mut builtin_mappings = None;
    let mut principal_kinds = HashMap::<[u8; 16], u16>::new();
    let mut principal_login_enabled = HashMap::<[u8; 16], bool>::new();
    let mut credential_verifiers = Vec::<CredentialVerifier>::new();
    let mut principal_names = HashMap::<[u8; 16], Vec<u8>>::new();
    let mut principal_name_ids = HashMap::<Vec<u8>, [u8; 16]>::new();
    let mut object_kinds = HashMap::<[u8; 16], u16>::new();
    let mut identity_tombstones = HashSet::<[u8; 16]>::new();
    let mut object_owners = Vec::<[u8; 16]>::new();
    let mut schema_search_paths = Vec::<Vec<[u8; 16]>>::new();
    let mut typed_object_kinds = HashMap::<[u8; 16], u16>::new();
    let mut builtin_type_kinds = HashMap::<[u8; 16], u16>::new();
    let mut builtin_type_ids = HashMap::<u16, [u8; 16]>::new();
    let mut type_records = HashMap::<[u8; 16], CatalogRecord>::new();
    let mut table_schema_versions = HashMap::<[u8; 16], u64>::new();
    let mut columns_by_table = HashMap::<[u8; 16], Vec<PersistedColumnSchema>>::new();
    let mut column_tables = HashMap::<[u8; 16], [u8; 16]>::new();
    let mut column_nullable = HashMap::<[u8; 16], bool>::new();
    let mut index_columns = HashMap::<[u8; 16], Vec<[u8; 16]>>::new();
    let mut unique_index_ids = HashSet::new();
    let mut policy_tables = HashMap::<[u8; 16], [u8; 16]>::new();
    let mut policy_kinds = HashMap::<[u8; 16], u16>::new();
    let mut expression_reference_edges = std::collections::HashSet::<([u8; 16], [u8; 16])>::new();
    let mut typed_name_records = Vec::<(u16, [u8; 16])>::new();
    let mut constraint_kinds = HashMap::<[u8; 16], u16>::new();
    let mut constraint_tables = HashMap::<[u8; 16], [u8; 16]>::new();
    let mut constraint_source_columns = HashMap::<[u8; 16], Vec<[u8; 16]>>::new();
    let mut constraint_referenced_columns = HashMap::<[u8; 16], Vec<[u8; 16]>>::new();
    let mut foreign_key_targets = HashMap::<[u8; 16], [u8; 16]>::new();
    let mut backing_index_targets = HashMap::<[u8; 16], [u8; 16]>::new();
    let mut index_owners = HashMap::<[u8; 16], [u8; 16]>::new();
    let mut memberships = Vec::<Membership>::new();
    let mut membership_grantors = Vec::<[u8; 16]>::new();
    let mut object_parents = HashMap::<[u8; 16], Option<[u8; 16]>>::new();
    let mut dependencies = Vec::<([u8; 16], [u8; 16], u16)>::new();
    let mut authority_edges = std::collections::HashSet::<([u8; 16], [u8; 16])>::new();
    let mut grantors = Vec::<[u8; 16]>::new();
    let mut database_records = 0usize;
    let mut database_id = None;
    for entry in table.iter().map_err(storage_error)? {
        let (key, value) = entry.map_err(storage_error)?;
        let key = key.value();
        if key.len() < 2 {
            return Err(StartupError::InvalidCatalog);
        }
        let family_code = u16::from_be_bytes([key[0], key[1]]) as usize;
        if !(1..=17).contains(&family_code) {
            return Err(StartupError::InvalidCatalog);
        }
        family_counts[family_code] = family_counts[family_code]
            .checked_add(1)
            .ok_or(StartupError::CountOverflow)?;
        if family_counts[family_code] > MAX_CATALOG_RECORDS_PER_FAMILY {
            return Err(StartupError::CatalogLimit);
        }
        count = count.checked_add(1).ok_or(StartupError::CountOverflow)?;

        let record =
            decode_registered_envelope(value.value()).map_err(|_| StartupError::InvalidCatalog)?;
        if record.logical_key != key || record.family_code as usize != family_code {
            return Err(StartupError::InvalidCatalog);
        }
        if record.family_code == 16 {
            credential_verifiers
                .push(decode_verifier_record(&record).map_err(|_| StartupError::InvalidCatalog)?);
        }
        if record.family_code == 17 {
            let tombstone_id = key
                .get(3..19)
                .and_then(|bytes| bytes.try_into().ok())
                .ok_or(StartupError::InvalidCatalog)?;
            if key.len() != 19 || key[2] != 1 || !identity_tombstones.insert(tombstone_id) {
                return Err(StartupError::InvalidCatalog);
            }
        }
        if record.family_code == 1 {
            if key.len() != 19
                || database_id
                    .replace(key[3..19].try_into().unwrap())
                    .is_some()
            {
                return Err(StartupError::InvalidCatalog);
            }
            database_records += 1;
            builtin_mappings = Some(validate_database_record(&record, metadata)?);
        }
        if record.family_code == 2 {
            if key.len() != 19 {
                return Err(StartupError::InvalidCatalog);
            }
            let object_id: [u8; 16] = key
                .get(3..19)
                .ok_or(StartupError::InvalidCatalog)?
                .try_into()
                .map_err(|_| StartupError::InvalidCatalog)?;
            let kind = field_u16(&record, 1)?;
            let parent_id = record
                .fields
                .iter()
                .find(|field| field.tag == 2)
                .map(|field| {
                    field
                        .payload
                        .as_slice()
                        .try_into()
                        .map_err(|_| StartupError::InvalidCatalog)
                })
                .transpose()?;
            if !matches!(kind, 1..=10)
                || object_parents.insert(object_id, parent_id).is_some()
                || object_kinds.insert(object_id, kind).is_some()
            {
                return Err(StartupError::InvalidCatalog);
            }
            object_owners.push(field_id(&record, 4)?);
        }
        if record.family_code == 3 {
            schema_search_paths.push(field_ids(&record, 1)?);
        }
        if let Some(object_kind) = typed_family_object_kind(&record)? {
            let object_id: [u8; 16] = key
                .get(3..19)
                .ok_or(StartupError::InvalidCatalog)?
                .try_into()
                .map_err(|_| StartupError::InvalidCatalog)?;
            if typed_object_kinds.insert(object_id, object_kind).is_some() {
                return Err(StartupError::InvalidCatalog);
            }
            if matches!(record.family_code, 6 | 10) {
                typed_name_records.push((record.family_code, object_id));
            }
        }
        if record.family_code == 4 {
            let type_id: [u8; 16] = key[3..19]
                .try_into()
                .map_err(|_| StartupError::InvalidCatalog)?;
            let builtin_kind =
                validate_builtin_type_profile(&record).map_err(|_| StartupError::InvalidCatalog)?;
            if !(1..=11).contains(&builtin_kind)
                || builtin_type_kinds.insert(type_id, builtin_kind).is_some()
                || builtin_type_ids.insert(builtin_kind, type_id).is_some()
            {
                return Err(StartupError::InvalidCatalog);
            }
            type_records.insert(type_id, record.clone());
        }
        if record.family_code == 5 {
            let table_id: [u8; 16] = key
                .get(3..19)
                .ok_or(StartupError::InvalidCatalog)?
                .try_into()
                .map_err(|_| StartupError::InvalidCatalog)?;
            let schema_version = field_u64(&record, 2)?;
            if key.len() != 19
                || schema_version == 0
                || field_u64(&record, 3)? == 0
                || field_u16(&record, 5)? != 1
                || table_schema_versions
                    .insert(table_id, schema_version)
                    .is_some()
            {
                return Err(StartupError::InvalidCatalog);
            }
        }
        if record.family_code == 6 {
            let column_id: [u8; 16] = key[3..19]
                .try_into()
                .map_err(|_| StartupError::InvalidCatalog)?;
            let table_id = field_id(&record, 1)?;
            if column_tables.insert(column_id, table_id).is_some() {
                return Err(StartupError::InvalidCatalog);
            }
            if column_nullable
                .insert(column_id, field_bool(&record, 6)?)
                .is_some()
            {
                return Err(StartupError::InvalidCatalog);
            }
            columns_by_table
                .entry(table_id)
                .or_default()
                .push(PersistedColumnSchema {
                    column_id,
                    ordinal: field_u32(&record, 2)?,
                    type_id: field_id(&record, 4)?,
                    parameters: field_payload(&record, 5)?.to_vec(),
                    nullable: field_bool(&record, 6)?,
                    introduced_schema_version: field_u64(&record, 9)?,
                });
        }
        if record.family_code == 7 {
            let expression = decode_catalog_expression_record(&record)
                .map_err(|_| StartupError::InvalidCatalog)?;
            let mut unique_dependencies = std::collections::HashSet::new();
            for dependency_id in expression.dependencies {
                if dependency_id == [0; 16] || !unique_dependencies.insert(dependency_id) {
                    return Err(StartupError::InvalidCatalog);
                }
                insert_expression_reference_edge(
                    &mut expression_reference_edges,
                    expression.expression_id,
                    dependency_id,
                )?;
            }
        }
        if record.family_code == 6 {
            let column_id: [u8; 16] = key
                .get(3..19)
                .ok_or(StartupError::InvalidCatalog)?
                .try_into()
                .map_err(|_| StartupError::InvalidCatalog)?;
            for tag in [7, 10] {
                if let Some(field) = record.fields.iter().find(|field| field.tag == tag) {
                    let expression_id: [u8; 16] = field
                        .payload
                        .as_slice()
                        .try_into()
                        .map_err(|_| StartupError::InvalidCatalog)?;
                    insert_expression_reference_edge(
                        &mut expression_reference_edges,
                        column_id,
                        expression_id,
                    )?;
                }
            }
        }
        if record.family_code == 9 {
            let index_id: [u8; 16] = key
                .get(3..19)
                .ok_or(StartupError::InvalidCatalog)?
                .try_into()
                .map_err(|_| StartupError::InvalidCatalog)?;
            let table_id = field_id(&record, 1)?;
            let columns = field_ids(&record, 2)?;
            let unique = field_bool(&record, 4)?;
            let schema_version = table_schema_versions
                .get(&table_id)
                .copied()
                .ok_or(StartupError::InvalidCatalog)?;
            let mut unique_columns = std::collections::HashSet::new();
            if index_id == [0; 16]
                || table_id == [0; 16]
                || field_u16(&record, 3)? != 1
                || field_u16(&record, 5)? != 1
                || field_optional_id(&record, 6)?.is_some_and(|id| id == [0; 16])
                || field_id(&record, 7)? == [0; 16]
                || field_u16(&record, 8)? != 1
                || field_u32(&record, 9)? != 1
                || field_u64(&record, 10)? != schema_version
                || index_definitions.insert(index_id, table_id).is_some()
            {
                return Err(StartupError::InvalidCatalog);
            }
            if unique {
                unique_index_ids.insert(index_id);
            }
            if columns.is_empty()
                || columns
                    .iter()
                    .any(|column_id| !unique_columns.insert(*column_id))
                || index_columns.insert(index_id, columns).is_some()
            {
                return Err(StartupError::InvalidCatalog);
            }
            if let Some(field) = record.fields.iter().find(|field| field.tag == 6) {
                let owner_id = field
                    .payload
                    .as_slice()
                    .try_into()
                    .map_err(|_| StartupError::InvalidCatalog)?;
                if index_owners.insert(index_id, owner_id).is_some() {
                    return Err(StartupError::InvalidCatalog);
                }
            }
        }
        if record.family_code == 8 {
            if key.len() != 19 {
                return Err(StartupError::InvalidCatalog);
            }
            let constraint_id: [u8; 16] = key[3..19]
                .try_into()
                .map_err(|_| StartupError::InvalidCatalog)?;
            let kind = field_u16(&record, 2)?;
            let table_id = field_id(&record, 1)?;
            let source_columns = field_ids(&record, 3)?;
            if kind == 5 {
                let referenced_columns = field_ids(&record, 6)?;
                if source_columns.is_empty()
                    || referenced_columns.is_empty()
                    || referenced_columns.len() != source_columns.len()
                    || field_optional_id(&record, 4)?.is_some()
                    || field_optional_id(&record, 11)?.is_some()
                    || field_u16(&record, 7)? != 1
                    || field_u16(&record, 8)? != 1
                    || field_u16(&record, 9)? != 1
                    || field_u16(&record, 10)? != 1
                {
                    return Err(StartupError::InvalidCatalog);
                }
            }
            if !matches!(kind, 1..=5) || constraint_kinds.insert(constraint_id, kind).is_some() {
                return Err(StartupError::InvalidCatalog);
            }
            if constraint_tables.insert(constraint_id, table_id).is_some()
                || constraint_source_columns
                    .insert(constraint_id, source_columns)
                    .is_some()
            {
                return Err(StartupError::InvalidCatalog);
            }
            if kind == 5
                && (foreign_key_targets
                    .insert(constraint_id, field_id(&record, 5)?)
                    .is_some()
                    || constraint_referenced_columns
                        .insert(constraint_id, field_ids(&record, 6)?)
                        .is_some())
            {
                return Err(StartupError::InvalidCatalog);
            }
            if matches!(kind, 2 | 3)
                && backing_index_targets
                    .insert(constraint_id, field_id(&record, 11)?)
                    .is_some()
            {
                return Err(StartupError::InvalidCatalog);
            }
            if kind == 4 {
                insert_expression_reference_edge(
                    &mut expression_reference_edges,
                    constraint_id,
                    field_id(&record, 4)?,
                )?;
            } else if record.fields.iter().any(|field| field.tag == 4) {
                return Err(StartupError::InvalidCatalog);
            }
        }
        if record.family_code == 13 {
            let policy_id: [u8; 16] = key
                .get(3..19)
                .ok_or(StartupError::InvalidCatalog)?
                .try_into()
                .map_err(|_| StartupError::InvalidCatalog)?;
            let policy_kind = field_u16(&record, 2)?;
            if !matches!(policy_kind, 1 | 2) {
                return Err(StartupError::InvalidCatalog);
            }
            if policy_tables
                .insert(policy_id, field_id(&record, 1)?)
                .is_some()
                || policy_kinds.insert(policy_id, policy_kind).is_some()
            {
                return Err(StartupError::InvalidCatalog);
            }
            for tag in [5, 6] {
                if let Some(field) = record.fields.iter().find(|field| field.tag == tag) {
                    let expression_id: [u8; 16] = field
                        .payload
                        .as_slice()
                        .try_into()
                        .map_err(|_| StartupError::InvalidCatalog)?;
                    insert_expression_reference_edge(
                        &mut expression_reference_edges,
                        policy_id,
                        expression_id,
                    )?;
                }
            }
        }
        if record.family_code == 10 {
            let principal_id: [u8; 16] = key
                .get(3..19)
                .ok_or(StartupError::InvalidCatalog)?
                .try_into()
                .map_err(|_| StartupError::InvalidCatalog)?;
            let kind = field_u16(&record, 1)?;
            let login_enabled = field_bool(&record, 3)?;
            let name = record
                .fields
                .iter()
                .find(|field| field.tag == 2)
                .ok_or(StartupError::InvalidCatalog)?
                .payload
                .clone();
            if !matches!(kind, 1 | 2)
                || (kind == 2 && login_enabled)
                || field_u64(&record, 4)? == 0
                || principal_kinds.insert(principal_id, kind).is_some()
                || principal_login_enabled
                    .insert(principal_id, login_enabled)
                    .is_some()
                || principal_names.insert(principal_id, name.clone()).is_some()
                || principal_name_ids.insert(name, principal_id).is_some()
            {
                return Err(StartupError::InvalidCatalog);
            }
        }
        if record.family_code == 11 {
            if key.len() != 36 || memberships.len() == MAX_MEMBERSHIP_EDGES {
                return Err(StartupError::InvalidCatalog);
            }
            let member_id: [u8; 16] = key[3..19]
                .try_into()
                .map_err(|_| StartupError::InvalidCatalog)?;
            let role_id: [u8; 16] = key[20..36]
                .try_into()
                .map_err(|_| StartupError::InvalidCatalog)?;
            let grantor_id = field_id(&record, 1)?;
            if field_u64(&record, 2)? == 0 {
                return Err(StartupError::InvalidCatalog);
            }
            memberships.push(Membership { member_id, role_id });
            membership_grantors.push(grantor_id);
            insert_authority_edge(&mut authority_edges, member_id, role_id)?;
        }
        if record.family_code == 12 {
            if key.len() != 59
                || key[2] != 1
                || key[19] != 1
                || key[36] != 1
                || key[39] != 1
                || key[42] != 1
                || field_u64(&record, 2)? == 0
            {
                return Err(StartupError::InvalidCatalog);
            }
            let grantee_id: [u8; 16] = key[3..19]
                .try_into()
                .map_err(|_| StartupError::InvalidCatalog)?;
            let object_id: [u8; 16] = key[20..36]
                .try_into()
                .map_err(|_| StartupError::InvalidCatalog)?;
            let privilege = u16::from_be_bytes([key[37], key[38]]);
            let scope = u16::from_be_bytes([key[40], key[41]]);
            let target_id: [u8; 16] = key[43..59]
                .try_into()
                .map_err(|_| StartupError::InvalidCatalog)?;
            let grantor_id = field_id(&record, 1)?;
            grantors.push(grantor_id);
            let target_kind = object_kinds.get(&target_id).copied();
            let valid_scope = match (privilege, scope) {
                (1 | 7, 1) => object_id == target_id && target_kind == Some(1),
                (2..=4 | 6, 1) => object_id == target_id && target_kind == Some(3),
                (5, 1) => object_id == target_id && target_kind == Some(3),
                (8..=11, 1) => object_id == target_id && matches!(target_kind, Some(1 | 3)),
                (2..=4 | 6, 2) => {
                    object_kinds.get(&object_id) == Some(&3)
                        && target_kind == Some(4)
                        && column_tables.get(&target_id) == Some(&object_id)
                }
                (12..=14, 1) => object_id == target_id && Some(target_id) == database_id,
                _ => false,
            };
            if !matches!(principal_kinds.get(&grantee_id), Some(1 | 2)) || !valid_scope {
                return Err(StartupError::InvalidCatalog);
            }
            insert_authority_edge(&mut authority_edges, grantee_id, target_id)?;
        }
        if record.family_code == 14 {
            if key.len() != 39 || key[2] != 1 || key[19] != 1 || key[22] != 1 {
                return Err(StartupError::InvalidCatalog);
            }
            let policy_id: [u8; 16] = key[3..19]
                .try_into()
                .map_err(|_| StartupError::InvalidCatalog)?;
            let target_kind = u16::from_be_bytes([key[20], key[21]]);
            let target_id: [u8; 16] = key[23..39]
                .try_into()
                .map_err(|_| StartupError::InvalidCatalog)?;
            if typed_object_kinds.get(&policy_id) != Some(&10) {
                return Err(StartupError::InvalidCatalog);
            }
            match target_kind {
                1 if principal_kinds.get(&target_id) == Some(&1) => {
                    insert_authority_edge(&mut authority_edges, policy_id, target_id)?;
                }
                2 if principal_kinds.get(&target_id) == Some(&2) => {
                    insert_authority_edge(&mut authority_edges, policy_id, target_id)?;
                }
                3 if policy_kinds.get(&policy_id) == Some(&2)
                    && typed_object_kinds.get(&target_id) == Some(&4)
                    && column_tables.get(&target_id) == policy_tables.get(&policy_id)
                    && policy_tables.get(&policy_id).is_some_and(|table_id| {
                        object_kinds.get(table_id) == Some(&3)
                            && typed_object_kinds.get(table_id) == Some(&3)
                    }) => {}
                _ => return Err(StartupError::InvalidCatalog),
            }
        }
        if record.family_code == 15 {
            if key.len() != 39 {
                return Err(StartupError::InvalidCatalog);
            }
            let dependent_id: [u8; 16] = key[3..19]
                .try_into()
                .map_err(|_| StartupError::InvalidCatalog)?;
            let referenced_id: [u8; 16] = key[20..36]
                .try_into()
                .map_err(|_| StartupError::InvalidCatalog)?;
            let kind = u16::from_be_bytes(
                key[37..39]
                    .try_into()
                    .map_err(|_| StartupError::InvalidCatalog)?,
            );
            if !matches!(kind, 1..=5) {
                return Err(StartupError::InvalidCatalog);
            }
            dependencies.push((dependent_id, referenced_id, kind));
        }
    }
    if database_records != 1 {
        return Err(StartupError::InvalidCatalog);
    }
    if identity_tombstones
        .iter()
        .any(|id| object_kinds.contains_key(id) || principal_kinds.contains_key(id))
    {
        return Err(StartupError::InvalidCatalog);
    }
    if credential_verifiers.iter().any(|credential| {
        match principal_kinds.get(&credential.user_id) {
            Some(1) => {
                !credential.revoked
                    && principal_login_enabled.get(&credential.user_id) != Some(&true)
            }
            Some(_) => true,
            None => !credential.revoked || !identity_tombstones.contains(&credential.user_id),
        }
    }) {
        return Err(StartupError::InvalidCatalog);
    }
    if database_id != Some(metadata.database_id)
        || object_kinds.contains_key(&metadata.database_id)
        || dependencies
            .iter()
            .any(|(dependent_id, referenced_id, kind)| {
                !object_parents.contains_key(dependent_id)
                    || !(object_parents.contains_key(referenced_id)
                        || *kind == 5 && Some(*referenced_id) == database_id)
            })
    {
        return Err(StartupError::InvalidCatalog);
    }
    let principal_name_index = read.open_table(PRINCIPAL_NAMES).map_err(storage_error)?;
    let mut indexed_principals = 0usize;
    for entry in principal_name_index.iter().map_err(storage_error)? {
        let (name, id) = entry.map_err(storage_error)?;
        let name = name.value();
        let id: [u8; 16] = id
            .value()
            .try_into()
            .map_err(|_| StartupError::InvalidCatalog)?;
        if name.is_empty()
            || name.len() > 4096
            || principal_names.get(&id).map(Vec::as_slice) != Some(name)
        {
            return Err(StartupError::InvalidCatalog);
        }
        indexed_principals += 1;
    }
    if indexed_principals != principal_names.len() {
        return Err(StartupError::InvalidCatalog);
    }

    let builtin_mappings = builtin_mappings.ok_or(StartupError::InvalidCatalog)?;
    for schema_code in [1u16, 2] {
        let schema_id = builtin_mappings
            .get(&schema_code)
            .ok_or(StartupError::InvalidCatalog)?;
        if object_kinds.get(schema_id) != Some(&1) || typed_object_kinds.get(schema_id) != Some(&1)
        {
            return Err(StartupError::InvalidCatalog);
        }
    }
    let admin_role_id = builtin_mappings
        .get(&3)
        .ok_or(StartupError::InvalidCatalog)?;
    if principal_kinds.get(admin_role_id) != Some(&2)
        || object_kinds.get(admin_role_id) != Some(&9)
        || typed_object_kinds.get(admin_role_id) != Some(&9)
        || principal_names
            .get(admin_role_id)
            .is_none_or(|name| name.as_slice() != b"database_admin")
    {
        return Err(StartupError::InvalidCatalog);
    }
    for code in 101u16..=111 {
        let type_id = builtin_mappings
            .get(&code)
            .ok_or(StartupError::InvalidCatalog)?;
        if builtin_type_kinds.get(type_id) != Some(&(code - 100))
            || object_kinds.get(type_id) != Some(&2)
            || typed_object_kinds.get(type_id) != Some(&2)
        {
            return Err(StartupError::InvalidCatalog);
        }
    }
    let actual_expression_reference_edges = dependencies
        .iter()
        .filter(|(_, _, kind)| *kind == 2)
        .map(|(dependent_id, referenced_id, _)| (*dependent_id, *referenced_id))
        .collect::<std::collections::HashSet<_>>();
    if actual_expression_reference_edges != expression_reference_edges
        || expression_reference_edges
            .iter()
            .any(|(dependent_id, referenced_id)| {
                let endpoint_kinds = (
                    object_kinds.get(dependent_id),
                    object_kinds.get(referenced_id),
                    typed_object_kinds.get(dependent_id),
                    typed_object_kinds.get(referenced_id),
                );
                !matches!(
                    endpoint_kinds,
                    (Some(4), Some(5), Some(4), Some(5))
                        | (Some(6), Some(5), Some(6), Some(5))
                        | (Some(10), Some(5), Some(10), Some(5))
                        | (Some(5), Some(4), Some(5), Some(4))
                        | (Some(5), Some(9), Some(5), Some(9))
                )
            })
    {
        return Err(StartupError::InvalidCatalog);
    }
    let actual_authority_edges = dependencies
        .iter()
        .filter(|(_, _, kind)| *kind == 5)
        .map(|(dependent_id, referenced_id, _)| (*dependent_id, *referenced_id))
        .collect::<std::collections::HashSet<_>>();
    if actual_authority_edges != authority_edges
        || authority_edges.iter().any(|(dependent_id, referenced_id)| {
            (!principal_kinds.contains_key(dependent_id)
                && object_kinds.get(dependent_id) != Some(&10))
                || (!principal_kinds.contains_key(referenced_id)
                    && !object_parents.contains_key(referenced_id)
                    && Some(*referenced_id) != database_id)
        })
    {
        return Err(StartupError::InvalidCatalog);
    }
    let foreign_key_edges = dependencies
        .iter()
        .filter(|(_, _, kind)| *kind == 3)
        .map(|(dependent_id, referenced_id, _)| (*dependent_id, *referenced_id))
        .collect::<std::collections::HashSet<_>>();
    if dependencies.iter().filter(|(_, _, kind)| *kind == 3).any(
        |(dependent_id, referenced_id, _)| {
            object_kinds.get(dependent_id) != Some(&6)
                || object_kinds.get(referenced_id) != Some(&6)
                || constraint_kinds.get(dependent_id) != Some(&5)
                || !matches!(constraint_kinds.get(referenced_id), Some(2 | 3))
                || foreign_key_targets.get(dependent_id) != Some(referenced_id)
        },
    ) || foreign_key_targets
        .iter()
        .any(|(dependent_id, referenced_id)| {
            !foreign_key_edges.contains(&(*dependent_id, *referenced_id))
        })
    {
        return Err(StartupError::InvalidCatalog);
    }
    let backing_index_edges = dependencies
        .iter()
        .filter(|(_, _, kind)| *kind == 4)
        .map(|(dependent_id, referenced_id, _)| (*dependent_id, *referenced_id))
        .collect::<std::collections::HashSet<_>>();
    if dependencies.iter().filter(|(_, _, kind)| *kind == 4).any(
        |(dependent_id, referenced_id, _)| {
            object_kinds.get(dependent_id) != Some(&6)
                || object_kinds.get(referenced_id) != Some(&7)
                || !matches!(constraint_kinds.get(dependent_id), Some(2 | 3))
                || backing_index_targets.get(dependent_id) != Some(referenced_id)
                || index_owners.get(referenced_id) != Some(dependent_id)
        },
    ) || backing_index_targets
        .iter()
        .any(|(dependent_id, referenced_id)| {
            !backing_index_edges.contains(&(*dependent_id, *referenced_id))
        })
        || index_owners.iter().any(|(referenced_id, dependent_id)| {
            !backing_index_edges.contains(&(*dependent_id, *referenced_id))
        })
    {
        return Err(StartupError::InvalidCatalog);
    }
    if !validate_owned_child_graph(&object_parents, &dependencies) {
        return Err(StartupError::InvalidCatalog);
    }
    for (constraint_id, table_id) in &constraint_tables {
        let source_columns = constraint_source_columns
            .get(constraint_id)
            .ok_or(StartupError::InvalidCatalog)?;
        if object_kinds.get(table_id) != Some(&3) || typed_object_kinds.get(table_id) != Some(&3) {
            return Err(StartupError::InvalidCatalog);
        }
        let mut unique_source_columns = std::collections::HashSet::new();
        for column_id in source_columns {
            if !unique_source_columns.insert(*column_id)
                || object_kinds.get(column_id) != Some(&4)
                || typed_object_kinds.get(column_id) != Some(&4)
                || column_tables.get(column_id) != Some(table_id)
            {
                return Err(StartupError::InvalidCatalog);
            }
        }
        if constraint_kinds.get(constraint_id) == Some(&1)
            && (source_columns.len() != 1
                || column_nullable.get(&source_columns[0]) != Some(&false))
        {
            return Err(StartupError::InvalidCatalog);
        }
        if matches!(constraint_kinds.get(constraint_id), Some(2 | 3)) {
            let index_id = backing_index_targets
                .get(constraint_id)
                .ok_or(StartupError::InvalidCatalog)?;
            if source_columns.is_empty()
                || index_columns.get(index_id) != Some(source_columns)
                || index_owners.get(index_id) != Some(constraint_id)
                || !unique_index_ids.contains(index_id)
                || (constraint_kinds.get(constraint_id) == Some(&2)
                    && source_columns
                        .iter()
                        .any(|column_id| column_nullable.get(column_id) != Some(&false)))
            {
                return Err(StartupError::InvalidCatalog);
            }
        }
    }
    for (constraint_id, target_id) in &foreign_key_targets {
        let source_columns = constraint_source_columns
            .get(constraint_id)
            .ok_or(StartupError::InvalidCatalog)?;
        let referenced_columns = constraint_referenced_columns
            .get(constraint_id)
            .ok_or(StartupError::InvalidCatalog)?;
        let target_columns = constraint_source_columns
            .get(target_id)
            .ok_or(StartupError::InvalidCatalog)?;
        if !matches!(constraint_kinds.get(target_id), Some(2 | 3))
            || source_columns.is_empty()
            || referenced_columns.len() != source_columns.len()
            || referenced_columns != target_columns
        {
            return Err(StartupError::InvalidCatalog);
        }
    }
    for (index_id, table_id) in &index_definitions {
        if object_kinds.get(table_id) != Some(&3) || typed_object_kinds.get(table_id) != Some(&3) {
            return Err(StartupError::InvalidCatalog);
        }
        let columns = index_columns
            .get(index_id)
            .ok_or(StartupError::InvalidCatalog)?;
        for column_id in columns {
            if object_kinds.get(column_id) != Some(&4)
                || typed_object_kinds.get(column_id) != Some(&4)
                || column_tables.get(column_id) != Some(table_id)
            {
                return Err(StartupError::InvalidCatalog);
            }
        }
    }
    if typed_object_kinds
        .iter()
        .any(|(object_id, expected_kind)| object_kinds.get(object_id) != Some(expected_kind))
        || object_kinds.iter().any(|(object_id, expected_kind)| {
            typed_object_kinds.get(object_id) != Some(expected_kind)
        })
    {
        return Err(StartupError::InvalidCatalog);
    }
    if schema_search_paths.iter().flatten().any(|schema_id| {
        object_kinds.get(schema_id) != Some(&1) || typed_object_kinds.get(schema_id) != Some(&1)
    }) {
        return Err(StartupError::InvalidCatalog);
    }
    for (family, object_id) in typed_name_records {
        let typed_key = catalog_id_key(family, object_id);
        let typed_value = table
            .get(typed_key.as_slice())
            .map_err(storage_error)?
            .ok_or(StartupError::InvalidCatalog)?;
        let typed_record = decode_registered_envelope(typed_value.value())
            .map_err(|_| StartupError::InvalidCatalog)?;
        let name_tag = if family == 6 { 3 } else { 2 };
        let object_key = catalog_id_key(2, object_id);
        let object_value = table
            .get(object_key.as_slice())
            .map_err(storage_error)?
            .ok_or(StartupError::InvalidCatalog)?;
        let object_record = decode_registered_envelope(object_value.value())
            .map_err(|_| StartupError::InvalidCatalog)?;
        if field_payload(&typed_record, name_tag)? != field_payload(&object_record, 3)? {
            return Err(StartupError::InvalidCatalog);
        }
    }
    if object_owners
        .iter()
        .any(|owner_id| !principal_kinds.contains_key(owner_id))
    {
        return Err(StartupError::InvalidCatalog);
    }
    if membership_grantors
        .iter()
        .any(|grantor_id| !principal_kinds.contains_key(grantor_id))
        || grantors
            .iter()
            .any(|grantor_id| !principal_kinds.contains_key(grantor_id))
    {
        return Err(StartupError::InvalidCatalog);
    }
    for membership in &memberships {
        if !principal_kinds.contains_key(&membership.member_id)
            || principal_kinds.get(&membership.role_id) != Some(&2)
        {
            return Err(StartupError::InvalidCatalog);
        }
    }
    validate_membership_graph(&memberships).map_err(|_| StartupError::InvalidCatalog)?;
    let mut table_schemas = HashMap::with_capacity(columns_by_table.len());
    for (table_id, mut columns_by_ordinal) in columns_by_table {
        let schema_version = *table_schema_versions
            .get(&table_id)
            .ok_or(StartupError::InvalidCatalog)?;
        if columns_by_ordinal.len() > crate::tuple::MAX_COLUMNS {
            return Err(StartupError::InvalidCatalog);
        }
        columns_by_ordinal.sort_by_key(|column| column.ordinal);
        let mut columns = Vec::with_capacity(columns_by_ordinal.len());
        let mut column_ids = Vec::with_capacity(columns_by_ordinal.len());
        for (expected_ordinal, column) in columns_by_ordinal.into_iter().enumerate() {
            if column.ordinal as usize != expected_ordinal
                || column.introduced_schema_version == 0
                || column.introduced_schema_version > schema_version
            {
                return Err(StartupError::InvalidCatalog);
            }
            let type_record = type_records
                .get(&column.type_id)
                .ok_or(StartupError::InvalidCatalog)?;
            let scalar_type = decode_builtin_scalar_type(type_record, &column.parameters)
                .map_err(|_| StartupError::InvalidCatalog)?;
            column_ids.push(column.column_id);
            columns.push(ColumnSchema {
                scalar_type,
                nullable: column.nullable,
            });
        }
        if table_schemas
            .insert(
                table_id,
                PersistedTableSchema {
                    schema_version,
                    column_ids,
                    columns,
                },
            )
            .is_some()
        {
            return Err(StartupError::InvalidCatalog);
        }
    }
    if foreign_key_targets.len() > MAX_FOREIGN_KEYS_PER_DATABASE {
        return Err(StartupError::InvalidCatalog);
    }
    let mut foreign_keys = Vec::with_capacity(foreign_key_targets.len());
    for (foreign_key_id, target_constraint_id) in &foreign_key_targets {
        let source_table_id = *constraint_tables
            .get(foreign_key_id)
            .ok_or(StartupError::InvalidCatalog)?;
        let target_table_id = *constraint_tables
            .get(target_constraint_id)
            .ok_or(StartupError::InvalidCatalog)?;
        let source_column_ids = constraint_source_columns
            .get(foreign_key_id)
            .ok_or(StartupError::InvalidCatalog)?;
        let target_column_ids = constraint_referenced_columns
            .get(foreign_key_id)
            .ok_or(StartupError::InvalidCatalog)?;
        let source_schema = table_schemas
            .get(&source_table_id)
            .ok_or(StartupError::InvalidCatalog)?;
        let target_schema = table_schemas
            .get(&target_table_id)
            .ok_or(StartupError::InvalidCatalog)?;
        let source_ordinals = source_column_ids
            .iter()
            .map(|column_id| {
                source_schema
                    .column_ids
                    .iter()
                    .position(|candidate| candidate == column_id)
                    .ok_or(StartupError::InvalidCatalog)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let target_ordinals = target_column_ids
            .iter()
            .map(|column_id| {
                target_schema
                    .column_ids
                    .iter()
                    .position(|candidate| candidate == column_id)
                    .ok_or(StartupError::InvalidCatalog)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut comparison_columns = Vec::with_capacity(target_ordinals.len());
        for (source_ordinal, target_ordinal) in source_ordinals.iter().zip(&target_ordinals) {
            let source_column = source_schema
                .columns
                .get(*source_ordinal)
                .ok_or(StartupError::InvalidCatalog)?;
            let target_column = target_schema
                .columns
                .get(*target_ordinal)
                .ok_or(StartupError::InvalidCatalog)?;
            if source_column.scalar_type != target_column.scalar_type {
                return Err(StartupError::InvalidCatalog);
            }
            comparison_columns.push(ColumnSchema {
                scalar_type: target_column.scalar_type,
                nullable: true,
            });
        }
        let target_index_id = *backing_index_targets
            .get(target_constraint_id)
            .ok_or(StartupError::InvalidCatalog)?;
        foreign_keys.push(PersistedForeignKey {
            source_table_id,
            source_ordinals,
            target_index_id,
            comparison_columns,
        });
    }
    Ok(PersistedCatalog {
        record_count: count,
        index_definitions,
        index_columns,
        unique_index_ids,
        table_schemas,
        foreign_keys,
    })
}

fn typed_family_object_kind(record: &CatalogRecord) -> Result<Option<u16>, StartupError> {
    let kind = match record.family_code {
        3 => Some(1),
        4 => Some(2),
        5 => Some(3),
        6 => Some(4),
        7 => Some(5),
        8 => Some(6),
        9 => Some(7),
        10 => match field_u16(record, 1)? {
            1 => Some(8),
            2 => Some(9),
            _ => return Err(StartupError::InvalidCatalog),
        },
        13 => Some(10),
        _ => None,
    };
    Ok(kind)
}

fn validate_database_record(
    record: &CatalogRecord,
    metadata: &EngineMetadata,
) -> Result<HashMap<u16, [u8; 16]>, StartupError> {
    let key = &record.logical_key;
    if key.len() != 19
        || key[..3] != [0, 1, 1]
        || key[3..] != metadata.database_id
        || field_u32(record, 2)? != metadata.catalog_version
        || field_u32(record, 5)? != metadata.expression_version
        || field_u32(record, 6)? != metadata.builtin_semantics_version
    {
        return Err(StartupError::InvalidCatalog);
    }

    let encoded = record
        .fields
        .iter()
        .find(|field| field.tag == 3)
        .ok_or(StartupError::InvalidCatalog)?
        .payload
        .as_slice();
    let count_bytes: [u8; 4] = encoded
        .get(..4)
        .ok_or(StartupError::InvalidCatalog)?
        .try_into()
        .map_err(|_| StartupError::InvalidCatalog)?;
    let count = u32::from_le_bytes(count_bytes) as usize;
    if count != 31 {
        return Err(StartupError::InvalidCatalog);
    }
    let expected_len = count
        .checked_mul(20)
        .and_then(|entries| entries.checked_add(4))
        .ok_or(StartupError::InvalidCatalog)?;
    if encoded.len() != expected_len {
        return Err(StartupError::InvalidCatalog);
    }

    let mut mappings = HashMap::<u16, [u8; 16]>::with_capacity(count);
    let mut ids = HashMap::<[u8; 16], ()>::with_capacity(count);
    let mut previous_code = None;
    for entry in encoded[4..].chunks_exact(20) {
        let code = u16::from_le_bytes([entry[0], entry[1]]);
        let reserved = u16::from_le_bytes([entry[2], entry[3]]);
        let id: [u8; 16] = entry[4..]
            .try_into()
            .map_err(|_| StartupError::InvalidCatalog)?;
        if previous_code.is_some_and(|previous| code <= previous)
            || !is_required_bootstrap_code(code)
            || reserved != 0
            || id == [0; 16]
            || mappings.insert(code, id).is_some()
            || ids.insert(id, ()).is_some()
        {
            return Err(StartupError::InvalidCatalog);
        }
        previous_code = Some(code);
    }
    if required_bootstrap_codes().any(|code| !mappings.contains_key(&code)) {
        return Err(StartupError::InvalidCatalog);
    }
    Ok(mappings)
}

fn required_bootstrap_codes() -> impl Iterator<Item = u16> {
    [1, 2, 3].into_iter().chain(101..=111).chain(201..=217)
}

fn is_required_bootstrap_code(code: u16) -> bool {
    required_bootstrap_codes().any(|required| required == code)
}

fn field_id(record: &CatalogRecord, tag: u16) -> Result<[u8; 16], StartupError> {
    record
        .fields
        .iter()
        .find(|field| field.tag == tag)
        .ok_or(StartupError::InvalidCatalog)?
        .payload
        .as_slice()
        .try_into()
        .map_err(|_| StartupError::InvalidCatalog)
}

fn field_optional_id(record: &CatalogRecord, tag: u16) -> Result<Option<[u8; 16]>, StartupError> {
    record
        .fields
        .iter()
        .find(|field| field.tag == tag)
        .map(|field| {
            field
                .payload
                .as_slice()
                .try_into()
                .map_err(|_| StartupError::InvalidCatalog)
        })
        .transpose()
}

fn field_ids(record: &CatalogRecord, tag: u16) -> Result<Vec<[u8; 16]>, StartupError> {
    let payload = field_payload(record, tag)?;
    if payload.len() < 4 {
        return Err(StartupError::InvalidCatalog);
    }
    let count = u32::from_le_bytes(
        payload[..4]
            .try_into()
            .map_err(|_| StartupError::InvalidCatalog)?,
    ) as usize;
    if count > 256 || payload.len() != 4 + count * 16 {
        return Err(StartupError::InvalidCatalog);
    }
    payload[4..]
        .chunks_exact(16)
        .map(|bytes| bytes.try_into().map_err(|_| StartupError::InvalidCatalog))
        .collect()
}

fn field_payload(record: &CatalogRecord, tag: u16) -> Result<&[u8], StartupError> {
    record
        .fields
        .iter()
        .find(|field| field.tag == tag)
        .map(|field| field.payload.as_slice())
        .ok_or(StartupError::InvalidCatalog)
}

fn catalog_id_key(family: u16, id: [u8; 16]) -> Vec<u8> {
    let mut key = family.to_be_bytes().to_vec();
    key.push(1);
    key.extend_from_slice(&id);
    key
}

fn field_u16(record: &CatalogRecord, tag: u16) -> Result<u16, StartupError> {
    record
        .fields
        .iter()
        .find(|field| field.tag == tag)
        .ok_or(StartupError::InvalidCatalog)?
        .payload
        .as_slice()
        .try_into()
        .map(u16::from_le_bytes)
        .map_err(|_| StartupError::InvalidCatalog)
}

fn field_u32(record: &CatalogRecord, tag: u16) -> Result<u32, StartupError> {
    record
        .fields
        .iter()
        .find(|field| field.tag == tag)
        .ok_or(StartupError::InvalidCatalog)?
        .payload
        .as_slice()
        .try_into()
        .map(u32::from_le_bytes)
        .map_err(|_| StartupError::InvalidCatalog)
}

fn field_u64(record: &CatalogRecord, tag: u16) -> Result<u64, StartupError> {
    record
        .fields
        .iter()
        .find(|field| field.tag == tag)
        .ok_or(StartupError::InvalidCatalog)?
        .payload
        .as_slice()
        .try_into()
        .map(u64::from_le_bytes)
        .map_err(|_| StartupError::InvalidCatalog)
}

fn field_bool(record: &CatalogRecord, tag: u16) -> Result<bool, StartupError> {
    match record
        .fields
        .iter()
        .find(|field| field.tag == tag)
        .ok_or(StartupError::InvalidCatalog)?
        .payload
        .as_slice()
    {
        [0] => Ok(false),
        [1] => Ok(true),
        _ => Err(StartupError::InvalidCatalog),
    }
}

fn validate_rows(
    read: &ReadTransaction,
    commit_sequence: u64,
    table_schemas: &PersistedTableSchemas,
) -> Result<usize, StartupError> {
    let table = read.open_table(ROWS).map_err(storage_error)?;
    let mut count = 0usize;
    for entry in table.iter().map_err(storage_error)? {
        let (key, value) = entry.map_err(storage_error)?;
        let key = key.value();
        if key.len() != 32 || key[..16] == [0; 16] {
            return Err(StartupError::InvalidRow);
        }
        let record = RowRecord::decode(value.value()).map_err(|_| StartupError::InvalidRow)?;
        if record.row_version > commit_sequence {
            return Err(StartupError::SequenceAhead);
        }
        let table_id: [u8; 16] = key[..16].try_into().map_err(|_| StartupError::InvalidRow)?;
        let schema = table_schemas
            .get(&table_id)
            .ok_or(StartupError::InvalidRow)?;
        decode_tuple(&schema.columns, schema.schema_version, &record.payload)
            .map_err(|_| StartupError::InvalidRow)?;
        count = count.checked_add(1).ok_or(StartupError::CountOverflow)?;
    }
    Ok(count)
}

fn validate_foreign_key_rows(
    read: &ReadTransaction,
    foreign_keys: &[PersistedForeignKey],
    table_schemas: &PersistedTableSchemas,
) -> Result<(), StartupError> {
    let rows = read.open_table(ROWS).map_err(storage_error)?;
    let indexes = read.open_table(INDEXES).map_err(storage_error)?;
    let mut row_visits = 0usize;
    for foreign_key in foreign_keys {
        let source_schema = table_schemas
            .get(&foreign_key.source_table_id)
            .ok_or(StartupError::InvalidCatalog)?;
        let mut start = [0; 32];
        start[..16].copy_from_slice(&foreign_key.source_table_id);
        let mut end = [u8::MAX; 32];
        end[..16].copy_from_slice(&foreign_key.source_table_id);
        for entry in rows
            .range(start.as_slice()..=end.as_slice())
            .map_err(storage_error)?
        {
            let (key, value) = entry.map_err(storage_error)?;
            if key.value().len() != 32 || key.value()[..16] != foreign_key.source_table_id {
                return Err(StartupError::InvalidRow);
            }
            row_visits = row_visits
                .checked_add(1)
                .ok_or(StartupError::ForeignKeyValidationLimitExceeded)?;
            if row_visits > MAX_FOREIGN_KEY_ROW_VISITS_PER_STARTUP {
                return Err(StartupError::ForeignKeyValidationLimitExceeded);
            }
            let row = RowRecord::decode(value.value()).map_err(|_| StartupError::InvalidRow)?;
            let tuple = decode_tuple(
                &source_schema.columns,
                source_schema.schema_version,
                &row.payload,
            )
            .map_err(|_| StartupError::InvalidRow)?;
            let projected = foreign_key
                .source_ordinals
                .iter()
                .map(|ordinal| {
                    tuple
                        .values
                        .get(*ordinal)
                        .cloned()
                        .ok_or(StartupError::InvalidCatalog)
                })
                .collect::<Result<Vec<_>, _>>()?;
            if projected.iter().any(Option::is_none) {
                continue;
            }
            let encoded_tuple = encode_sql_tuple(&foreign_key.comparison_columns, &projected)
                .map_err(|_| StartupError::InvalidRow)?;
            let mut index_prefix = Vec::with_capacity(16 + encoded_tuple.len());
            index_prefix.extend_from_slice(&foreign_key.target_index_id);
            index_prefix.extend_from_slice(&encoded_tuple);
            let mut matching = indexes
                .range(index_prefix.as_slice()..)
                .map_err(storage_error)?;
            let Some(entry) = matching.next() else {
                return Err(StartupError::InvalidForeignKey);
            };
            let (candidate, _) = entry.map_err(storage_error)?;
            if !candidate.value().starts_with(&index_prefix) {
                return Err(StartupError::InvalidForeignKey);
            }
        }
    }
    Ok(())
}

fn validate_indexes(
    read: &ReadTransaction,
    commit_sequence: u64,
    index_definitions: &IndexDefinitions,
    index_columns: &IndexColumns,
    unique_index_ids: &UniqueIndexIds,
    table_schemas: &PersistedTableSchemas,
) -> Result<usize, StartupError> {
    let indexes = read.open_table(INDEXES).map_err(storage_error)?;
    let rows = read.open_table(ROWS).map_err(storage_error)?;
    let reverse = read.open_table(ROW_INDEXES).map_err(storage_error)?;
    let mut indexes_by_table = HashMap::<[u8; 16], Vec<[u8; 16]>>::new();
    for (index_id, table_id) in index_definitions {
        indexes_by_table
            .entry(*table_id)
            .or_default()
            .push(*index_id);
    }
    let mut count = 0usize;
    for entry in indexes.iter().map_err(storage_error)? {
        let (key, value) = entry.map_err(storage_error)?;
        let key = key.value();
        if key.len() < 33 {
            return Err(StartupError::InvalidIndex);
        }
        let index_id: [u8; 16] = key[..16]
            .try_into()
            .map_err(|_| StartupError::InvalidIndex)?;
        let table_id = index_definitions
            .get(&index_id)
            .ok_or(StartupError::InvalidIndex)?;
        let locator = IndexLocator::decode_for_key(key, value.value())
            .map_err(|_| StartupError::InvalidIndex)?;
        if locator.row_version > commit_sequence {
            return Err(StartupError::SequenceAhead);
        }
        let mut row_key = [0; 32];
        row_key[..16].copy_from_slice(table_id);
        row_key[16..].copy_from_slice(&locator.row_id);
        let row = rows
            .get(&row_key[..])
            .map_err(storage_error)?
            .ok_or(StartupError::InvalidIndex)?;
        let row = RowRecord::decode(row.value()).map_err(|_| StartupError::InvalidIndex)?;
        let expected_key = derive_persisted_index_key(
            index_id,
            locator.row_id,
            *table_id,
            &row.payload,
            index_columns,
            table_schemas,
        )?;
        if locator.row_version != row.row_version || expected_key.key.as_slice() != key {
            return Err(StartupError::InvalidIndex);
        }
        let mut reverse_key = row_key.to_vec();
        reverse_key.extend_from_slice(&key[..key.len() - 16]);
        let reverse_value = reverse.get(&reverse_key[..]).map_err(storage_error)?;
        if reverse_value.is_none_or(|value| !value.value().is_empty()) {
            return Err(StartupError::InvalidIndex);
        }
        count = count.checked_add(1).ok_or(StartupError::CountOverflow)?;
    }
    let mut expected_count = 0usize;
    let mut unique_keys = HashSet::<Vec<u8>>::new();
    for entry in rows.iter().map_err(storage_error)? {
        let (key, value) = entry.map_err(storage_error)?;
        let row_key: [u8; 32] = key
            .value()
            .try_into()
            .map_err(|_| StartupError::InvalidRow)?;
        let table_id: [u8; 16] = row_key[..16]
            .try_into()
            .map_err(|_| StartupError::InvalidRow)?;
        let row = RowRecord::decode(value.value()).map_err(|_| StartupError::InvalidRow)?;
        if let Some(index_ids) = indexes_by_table.get(&table_id) {
            for index_id in index_ids {
                let derived = derive_persisted_index_key(
                    *index_id,
                    row_key[16..]
                        .try_into()
                        .map_err(|_| StartupError::InvalidRow)?,
                    table_id,
                    &row.payload,
                    index_columns,
                    table_schemas,
                )?;
                if unique_index_ids.contains(index_id) && !derived.has_null {
                    let unique_key = derived.key[..derived.key.len() - 16].to_vec();
                    if !unique_keys.insert(unique_key) {
                        return Err(StartupError::InvalidIndex);
                    }
                }
                let index_key = derived.key;
                let locator = indexes
                    .get(index_key.as_slice())
                    .map_err(storage_error)?
                    .ok_or(StartupError::InvalidIndex)?;
                let locator = IndexLocator::decode_for_key(&index_key, locator.value())
                    .map_err(|_| StartupError::InvalidIndex)?;
                if locator.row_version != row.row_version || locator.row_id != row_key[16..] {
                    return Err(StartupError::InvalidIndex);
                }
                let mut reverse_key = row_key.to_vec();
                reverse_key.extend_from_slice(&index_key[..index_key.len() - 16]);
                let reverse_value = reverse.get(&reverse_key[..]).map_err(storage_error)?;
                if reverse_value.is_none_or(|value| !value.value().is_empty()) {
                    return Err(StartupError::InvalidIndex);
                }
                expected_count = expected_count
                    .checked_add(1)
                    .ok_or(StartupError::CountOverflow)?;
            }
        }
    }
    let mut reverse_count = 0usize;
    for entry in reverse.iter().map_err(storage_error)? {
        let (key, value) = entry.map_err(storage_error)?;
        let key = key.value();
        if key.len() < 49 || !value.value().is_empty() {
            return Err(StartupError::InvalidIndex);
        }
        let row_key: [u8; 32] = key[..32]
            .try_into()
            .map_err(|_| StartupError::InvalidIndex)?;
        let index_id: [u8; 16] = key[32..48]
            .try_into()
            .map_err(|_| StartupError::InvalidIndex)?;
        if row_key[..16] == [0; 16] {
            return Err(StartupError::InvalidIndex);
        }
        let table_id = index_definitions
            .get(&index_id)
            .ok_or(StartupError::InvalidIndex)?;
        if table_id.as_slice() != &row_key[..16] {
            return Err(StartupError::InvalidIndex);
        }
        let mut index_key = key[32..].to_vec();
        index_key.extend_from_slice(&row_key[16..]);
        let locator = indexes
            .get(&index_key[..])
            .map_err(storage_error)?
            .ok_or(StartupError::InvalidIndex)?;
        let locator = IndexLocator::decode_for_key(&index_key, locator.value())
            .map_err(|_| StartupError::InvalidIndex)?;
        let row = rows
            .get(&row_key[..])
            .map_err(storage_error)?
            .ok_or(StartupError::InvalidIndex)?;
        let row = RowRecord::decode(row.value()).map_err(|_| StartupError::InvalidIndex)?;
        if locator.row_version != row.row_version {
            return Err(StartupError::InvalidIndex);
        }
        reverse_count = reverse_count
            .checked_add(1)
            .ok_or(StartupError::CountOverflow)?;
    }
    if reverse_count != count || expected_count != count {
        return Err(StartupError::InvalidIndex);
    }
    Ok(count)
}

fn derive_persisted_index_key(
    index_id: [u8; 16],
    row_id: [u8; 16],
    table_id: [u8; 16],
    payload: &[u8],
    index_columns: &IndexColumns,
    table_schemas: &PersistedTableSchemas,
) -> Result<DerivedIndexKey, StartupError> {
    let schema = table_schemas
        .get(&table_id)
        .ok_or(StartupError::InvalidCatalog)?;
    let index_column_ids = index_columns
        .get(&index_id)
        .ok_or(StartupError::InvalidCatalog)?;
    derive_index_key(
        index_id,
        row_id,
        schema.schema_version,
        &schema.column_ids,
        &schema.columns,
        payload,
        index_column_ids,
    )
    .map_err(|_| StartupError::InvalidIndex)
}

fn validate_outcomes(read: &ReadTransaction, commit_sequence: u64) -> Result<usize, StartupError> {
    let table = read.open_table(OUTCOMES).map_err(storage_error)?;
    let mut count = 0usize;
    for entry in table.iter().map_err(storage_error)? {
        let (key, value) = entry.map_err(storage_error)?;
        let key: [u8; 16] = key
            .value()
            .try_into()
            .map_err(|_| StartupError::InvalidOutcome)?;
        let outcome =
            InvocationOutcome::decode(value.value()).map_err(|_| StartupError::InvalidOutcome)?;
        if outcome.invocation_id != key
            || (outcome.status == OutcomeStatus::Committed
                && outcome.commit_sequence > commit_sequence)
        {
            return Err(StartupError::InvalidOutcome);
        }
        count = count.checked_add(1).ok_or(StartupError::CountOverflow)?;
    }
    Ok(count)
}

fn validate_audit(read: &ReadTransaction) -> Result<usize, StartupError> {
    let table = read.open_table(AUDIT).map_err(storage_error)?;
    if table.len().map_err(storage_error)? > crate::audit::MAX_AUDIT_EVENTS as u64 {
        return Err(StartupError::AuditCapacityExceeded);
    }
    let mut count = 0usize;
    for entry in table.iter().map_err(storage_error)? {
        let (key, value) = entry.map_err(storage_error)?;
        let key: [u8; 16] = key
            .value()
            .try_into()
            .map_err(|_| StartupError::InvalidAudit)?;
        let event = AuditEvent::decode(value.value()).map_err(|_| StartupError::InvalidAudit)?;
        if event.event_id != key {
            return Err(StartupError::InvalidAudit);
        }
        count = count.checked_add(1).ok_or(StartupError::CountOverflow)?;
    }
    Ok(count)
}

#[cfg(test)]
mod catalog_index_tests {
    use super::{
        ensure_column_lookup_index, ensure_dependency_index, ensure_grant_index,
        ensure_index_name_index, ensure_membership_index, ensure_object_owner_index,
        ensure_policy_index, ensure_relation_name_index, ensure_table_constraint_index,
        ensure_table_index_index, validate_name_domains,
    };
    use crate::catalog::{CatalogField, WireType, encode_envelope, family_schema};
    use crate::catalog_index::{
        lookup_column_id_by_name, lookup_column_id_by_ordinal, lookup_dependencies_for_dependent,
        lookup_dependents_for_referenced, lookup_grants_for_grantee, lookup_grants_for_object,
        lookup_index_id_by_name, lookup_members_for_role, lookup_policies_for_table_operation,
        lookup_relation_id_by_name, lookup_roles_for_member, scan_column_ids_for_table,
        scan_constraint_ids_for_table, scan_index_ids_for_table, scan_object_ids_for_owner,
        scan_relation_ids_for_schema,
    };
    use crate::schema::{
        CATALOG, COLUMN_LOOKUPS, DEPENDENCIES, GRANTS, INDEX_NAMES, MEMBERSHIPS, OBJECT_OWNERS,
        POLICIES, RELATION_NAMES, TABLE_CONSTRAINTS, TABLE_INDEXES, create_tables,
    };
    use redb::{Database, ReadableDatabase, ReadableTable};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    fn database() -> Database {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "prdb-relation-index-{}-{id}.redb",
            std::process::id()
        ));
        let database = Database::create(path).unwrap();
        let write = database.begin_write().unwrap();
        create_tables(&write).unwrap();

        let schema_id = [0x31; 16];
        let relation_id = [0x32; 16];
        let mut table_key = 5u16.to_be_bytes().to_vec();
        table_key.push(1);
        table_key.extend_from_slice(&relation_id);
        let table_fields = [
            CatalogField {
                tag: 1,
                wire_type: WireType::Id,
                payload: schema_id.to_vec(),
            },
            CatalogField {
                tag: 2,
                wire_type: WireType::U64,
                payload: 1u64.to_le_bytes().to_vec(),
            },
            CatalogField {
                tag: 3,
                wire_type: WireType::U64,
                payload: 1u64.to_le_bytes().to_vec(),
            },
            CatalogField {
                tag: 4,
                wire_type: WireType::Id,
                payload: [0x33; 16].to_vec(),
            },
            CatalogField {
                tag: 5,
                wire_type: WireType::U16,
                payload: 1u16.to_le_bytes().to_vec(),
            },
        ];
        let table_value =
            encode_envelope(&family_schema(5).unwrap(), &table_key, &table_fields).unwrap();
        let mut object_key = 2u16.to_be_bytes().to_vec();
        object_key.push(1);
        object_key.extend_from_slice(&relation_id);
        let object_fields = [
            CatalogField {
                tag: 1,
                wire_type: WireType::U16,
                payload: 3u16.to_le_bytes().to_vec(),
            },
            CatalogField {
                tag: 2,
                wire_type: WireType::Id,
                payload: schema_id.to_vec(),
            },
            CatalogField {
                tag: 3,
                wire_type: WireType::Text,
                payload: b"orders".to_vec(),
            },
            CatalogField {
                tag: 4,
                wire_type: WireType::Id,
                payload: [0x34; 16].to_vec(),
            },
            CatalogField {
                tag: 5,
                wire_type: WireType::U64,
                payload: 1u64.to_le_bytes().to_vec(),
            },
            CatalogField {
                tag: 6,
                wire_type: WireType::U16,
                payload: 1u16.to_le_bytes().to_vec(),
            },
        ];
        let object_value =
            encode_envelope(&family_schema(2).unwrap(), &object_key, &object_fields).unwrap();
        let index_id = [0x69; 16];
        let mut index_object_key = 2u16.to_be_bytes().to_vec();
        index_object_key.push(1);
        index_object_key.extend_from_slice(&index_id);
        let index_object_fields = [
            CatalogField {
                tag: 1,
                wire_type: WireType::U16,
                payload: 7u16.to_le_bytes().to_vec(),
            },
            CatalogField {
                tag: 2,
                wire_type: WireType::Id,
                payload: schema_id.to_vec(),
            },
            CatalogField {
                tag: 3,
                wire_type: WireType::Text,
                payload: b"orders_idx".to_vec(),
            },
            CatalogField {
                tag: 4,
                wire_type: WireType::Id,
                payload: [0x34; 16].to_vec(),
            },
            CatalogField {
                tag: 5,
                wire_type: WireType::U64,
                payload: 1u64.to_le_bytes().to_vec(),
            },
            CatalogField {
                tag: 6,
                wire_type: WireType::U16,
                payload: 1u16.to_le_bytes().to_vec(),
            },
        ];
        let index_object_value = encode_envelope(
            &family_schema(2).unwrap(),
            &index_object_key,
            &index_object_fields,
        )
        .unwrap();
        let mut membership_key = 11u16.to_be_bytes().to_vec();
        membership_key.push(1);
        membership_key.extend_from_slice(&[0x41; 16]);
        membership_key.push(1);
        membership_key.extend_from_slice(&[0x42; 16]);
        let membership_fields = [
            CatalogField {
                tag: 1,
                wire_type: WireType::Id,
                payload: [0x43; 16].to_vec(),
            },
            CatalogField {
                tag: 2,
                wire_type: WireType::U64,
                payload: 1u64.to_le_bytes().to_vec(),
            },
        ];
        let membership_value = encode_envelope(
            &family_schema(11).unwrap(),
            &membership_key,
            &membership_fields,
        )
        .unwrap();
        let grantee_id = [0x51; 16];
        let object_id = [0x52; 16];
        let target_id = object_id;
        let mut grant_key = 12u16.to_be_bytes().to_vec();
        grant_key.push(1);
        grant_key.extend_from_slice(&grantee_id);
        grant_key.push(1);
        grant_key.extend_from_slice(&object_id);
        grant_key.push(1);
        grant_key.extend_from_slice(&1u16.to_be_bytes());
        grant_key.push(1);
        grant_key.extend_from_slice(&1u16.to_be_bytes());
        grant_key.push(1);
        grant_key.extend_from_slice(&target_id);
        let grant_fields = [
            CatalogField {
                tag: 1,
                wire_type: WireType::Id,
                payload: [0x54; 16].to_vec(),
            },
            CatalogField {
                tag: 2,
                wire_type: WireType::U64,
                payload: 1u64.to_le_bytes().to_vec(),
            },
        ];
        let grant_value =
            encode_envelope(&family_schema(12).unwrap(), &grant_key, &grant_fields).unwrap();
        let policy_id = [0x62; 16];
        let mut policy_key = 13u16.to_be_bytes().to_vec();
        policy_key.push(1);
        policy_key.extend_from_slice(&policy_id);
        let policy_fields = [
            CatalogField {
                tag: 1,
                wire_type: WireType::Id,
                payload: relation_id.to_vec(),
            },
            CatalogField {
                tag: 2,
                wire_type: WireType::U16,
                payload: 1u16.to_le_bytes().to_vec(),
            },
            CatalogField {
                tag: 3,
                wire_type: WireType::U16,
                payload: 1u16.to_le_bytes().to_vec(),
            },
            CatalogField {
                tag: 4,
                wire_type: WireType::U16,
                payload: 1u16.to_le_bytes().to_vec(),
            },
            CatalogField {
                tag: 5,
                wire_type: WireType::Id,
                payload: [0x63; 16].to_vec(),
            },
            CatalogField {
                tag: 7,
                wire_type: WireType::U64,
                payload: 1u64.to_le_bytes().to_vec(),
            },
        ];
        let policy_value =
            encode_envelope(&family_schema(13).unwrap(), &policy_key, &policy_fields).unwrap();
        let dependent_id = [0x65; 16];
        let referenced_id = [0x66; 16];
        let mut dependency_key = 15u16.to_be_bytes().to_vec();
        dependency_key.push(1);
        dependency_key.extend_from_slice(&dependent_id);
        dependency_key.push(1);
        dependency_key.extend_from_slice(&referenced_id);
        dependency_key.push(1);
        dependency_key.extend_from_slice(&5u16.to_be_bytes());
        let dependency_value =
            encode_envelope(&family_schema(15).unwrap(), &dependency_key, &[]).unwrap();
        let column_id = [0x67; 16];
        let mut column_key = 6u16.to_be_bytes().to_vec();
        column_key.push(1);
        column_key.extend_from_slice(&column_id);
        let column_fields = [
            CatalogField {
                tag: 1,
                wire_type: WireType::Id,
                payload: relation_id.to_vec(),
            },
            CatalogField {
                tag: 2,
                wire_type: WireType::U32,
                payload: 0u32.to_le_bytes().to_vec(),
            },
            CatalogField {
                tag: 3,
                wire_type: WireType::Text,
                payload: b"order_id".to_vec(),
            },
            CatalogField {
                tag: 4,
                wire_type: WireType::Id,
                payload: [0x68; 16].to_vec(),
            },
            CatalogField {
                tag: 5,
                wire_type: WireType::Bytes,
                payload: [0; 8].to_vec(),
            },
            CatalogField {
                tag: 6,
                wire_type: WireType::Bool,
                payload: vec![1],
            },
            CatalogField {
                tag: 8,
                wire_type: WireType::U16,
                payload: 1u16.to_le_bytes().to_vec(),
            },
            CatalogField {
                tag: 9,
                wire_type: WireType::U64,
                payload: 1u64.to_le_bytes().to_vec(),
            },
        ];
        let column_value =
            encode_envelope(&family_schema(6).unwrap(), &column_key, &column_fields).unwrap();
        let mut catalog = write.open_table(CATALOG).unwrap();
        catalog
            .insert(table_key.as_slice(), table_value.as_slice())
            .unwrap();
        catalog
            .insert(object_key.as_slice(), object_value.as_slice())
            .unwrap();
        catalog
            .insert(index_object_key.as_slice(), index_object_value.as_slice())
            .unwrap();
        catalog
            .insert(membership_key.as_slice(), membership_value.as_slice())
            .unwrap();
        catalog
            .insert(grant_key.as_slice(), grant_value.as_slice())
            .unwrap();
        catalog
            .insert(policy_key.as_slice(), policy_value.as_slice())
            .unwrap();
        catalog
            .insert(dependency_key.as_slice(), dependency_value.as_slice())
            .unwrap();
        catalog
            .insert(column_key.as_slice(), column_value.as_slice())
            .unwrap();
        drop(catalog);
        write.commit().unwrap();
        database
    }

    #[test]
    fn stale_relation_name_index_is_rebuilt_from_catalog_records() {
        let database = database();
        let mut key = vec![0x31; 16];
        key.extend_from_slice(b"orders");
        let write = database.begin_write().unwrap();
        let mut index = write.open_table(RELATION_NAMES).unwrap();
        index.insert(key.as_slice(), &[0x55; 16][..]).unwrap();
        index.insert(&b"stale"[..], &[0x44; 3][..]).unwrap();
        drop(index);
        write.commit().unwrap();
        ensure_relation_name_index(&database).unwrap();
        let read = database.begin_read().unwrap();
        assert_eq!(
            lookup_relation_id_by_name(&read, [0x31; 16], "orders").unwrap(),
            Some([0x32; 16])
        );
        assert_eq!(
            scan_relation_ids_for_schema(&read, [0x31; 16], 1).unwrap(),
            vec![[0x32; 16]]
        );
        assert!(
            read.open_table(RELATION_NAMES)
                .unwrap()
                .get(&b"stale"[..])
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn missing_relation_name_index_is_rebuilt_from_catalog_records() {
        let database = database();
        let write = database.begin_write().unwrap();
        assert!(write.delete_table(RELATION_NAMES).unwrap());
        write.commit().unwrap();
        ensure_relation_name_index(&database).unwrap();
        let read = database.begin_read().unwrap();
        assert_eq!(
            lookup_relation_id_by_name(&read, [0x31; 16], "orders").unwrap(),
            Some([0x32; 16])
        );
    }

    #[test]
    fn object_owner_scan_rebuilds_and_uses_the_owner_prefix() {
        let database = database();
        let write = database.begin_write().unwrap();
        assert!(write.delete_table(OBJECT_OWNERS).unwrap());
        write.commit().unwrap();

        ensure_object_owner_index(&database).unwrap();
        let read = database.begin_read().unwrap();
        assert_eq!(
            scan_object_ids_for_owner(&read, [0x34; 16], 2).unwrap(),
            vec![[0x32; 16], [0x69; 16]]
        );
        assert_eq!(
            scan_object_ids_for_owner(&read, [0x34; 16], 1),
            Err(crate::catalog_index::CatalogIndexError::RecordLimitExceeded)
        );
        assert_eq!(
            read.open_table(OBJECT_OWNERS)
                .unwrap()
                .iter()
                .unwrap()
                .count(),
            2
        );
    }

    #[test]
    fn stale_membership_directions_are_rebuilt_from_catalog_records() {
        let database = database();
        let write = database.begin_write().unwrap();
        write
            .open_table(MEMBERSHIPS)
            .unwrap()
            .insert(&[1, 0x41][..], &b"stale"[..])
            .unwrap();
        write.commit().unwrap();
        ensure_membership_index(&database).unwrap();
        let read = database.begin_read().unwrap();
        assert_eq!(
            lookup_roles_for_member(&read, [0x41; 16]).unwrap(),
            vec![[0x42; 16]]
        );
        assert_eq!(
            lookup_members_for_role(&read, [0x42; 16]).unwrap(),
            vec![[0x41; 16]]
        );
    }

    #[test]
    fn missing_membership_index_is_rebuilt_from_catalog_records() {
        let database = database();
        let write = database.begin_write().unwrap();
        assert!(write.delete_table(MEMBERSHIPS).unwrap());
        write.commit().unwrap();
        ensure_membership_index(&database).unwrap();
        let read = database.begin_read().unwrap();
        assert_eq!(
            lookup_roles_for_member(&read, [0x41; 16]).unwrap(),
            vec![[0x42; 16]]
        );
        assert_eq!(
            lookup_members_for_role(&read, [0x42; 16]).unwrap(),
            vec![[0x41; 16]]
        );
    }

    #[test]
    fn stale_grant_indexes_are_rebuilt_from_catalog_records() {
        let database = database();
        let mut grantee_key = vec![1, 0x51];
        grantee_key.resize(17, 0x51);
        grantee_key.extend_from_slice(&[0x61; 59]);
        let write = database.begin_write().unwrap();
        write
            .open_table(GRANTS)
            .unwrap()
            .insert(grantee_key.as_slice(), &b"stale"[..])
            .unwrap();
        write
            .open_table(GRANTS)
            .unwrap()
            .insert(&b"stale"[..], &b"stale"[..])
            .unwrap();
        write.commit().unwrap();
        ensure_grant_index(&database).unwrap();
        let read = database.begin_read().unwrap();
        assert_eq!(
            lookup_grants_for_grantee(&read, [0x51; 16]).unwrap().len(),
            1
        );
        assert_eq!(
            lookup_grants_for_object(&read, [0x52; 16]).unwrap().len(),
            1
        );
        assert!(
            read.open_table(GRANTS)
                .unwrap()
                .get(&b"stale"[..])
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn missing_grant_indexes_are_rebuilt_from_catalog_records() {
        let database = database();
        let write = database.begin_write().unwrap();
        assert!(write.delete_table(GRANTS).unwrap());
        write.commit().unwrap();
        ensure_grant_index(&database).unwrap();
        let read = database.begin_read().unwrap();
        assert_eq!(
            lookup_grants_for_grantee(&read, [0x51; 16]).unwrap().len(),
            1
        );
        assert_eq!(
            lookup_grants_for_object(&read, [0x52; 16]).unwrap().len(),
            1
        );
    }

    #[test]
    fn stale_dependency_directions_are_rebuilt_from_catalog_records() {
        let database = database();
        let write = database.begin_write().unwrap();
        write
            .open_table(DEPENDENCIES)
            .unwrap()
            .insert(&[1, 0x65][..], &b"stale"[..])
            .unwrap();
        write.commit().unwrap();
        ensure_dependency_index(&database).unwrap();
        let read = database.begin_read().unwrap();
        assert_eq!(
            lookup_dependencies_for_dependent(&read, [0x65; 16])
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            lookup_dependents_for_referenced(&read, [0x66; 16])
                .unwrap()
                .len(),
            1
        );
        assert!(
            read.open_table(DEPENDENCIES)
                .unwrap()
                .get(&[1, 0x65][..])
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn missing_dependency_index_is_rebuilt_from_catalog_records() {
        let database = database();
        let write = database.begin_write().unwrap();
        assert!(write.delete_table(DEPENDENCIES).unwrap());
        write.commit().unwrap();
        ensure_dependency_index(&database).unwrap();
        let read = database.begin_read().unwrap();
        assert_eq!(
            lookup_dependencies_for_dependent(&read, [0x65; 16])
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            lookup_dependents_for_referenced(&read, [0x66; 16])
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn missing_column_lookups_are_rebuilt_from_catalog_records() {
        let database = database();
        let write = database.begin_write().unwrap();
        assert!(write.delete_table(COLUMN_LOOKUPS).unwrap());
        write.commit().unwrap();
        ensure_column_lookup_index(&database).unwrap();
        let read = database.begin_read().unwrap();
        assert_eq!(
            lookup_column_id_by_name(&read, [0x32; 16], "order_id").unwrap(),
            Some([0x67; 16])
        );
        assert_eq!(
            lookup_column_id_by_ordinal(&read, [0x32; 16], 0).unwrap(),
            Some([0x67; 16])
        );
    }

    #[test]
    fn table_column_scan_is_bounded_to_its_ordinal_prefix() {
        let database = database();
        ensure_column_lookup_index(&database).unwrap();
        let write = database.begin_write().unwrap();
        let mut index = write.open_table(COLUMN_LOOKUPS).unwrap();
        for ordinal in 0u32..4 {
            let mut key = vec![2];
            key.extend_from_slice(&[0x99; 16]);
            key.extend_from_slice(&ordinal.to_be_bytes());
            let indexed_id = [0xf0u8 + ordinal as u8; 16];
            index.insert(key.as_slice(), &indexed_id[..]).unwrap();
        }
        drop(index);
        write.commit().unwrap();

        let read = database.begin_read().unwrap();
        assert_eq!(
            scan_column_ids_for_table(&read, [0x32; 16], 1).unwrap(),
            vec![[0x67; 16]]
        );
        assert_eq!(
            scan_column_ids_for_table(&read, [0x32; 16], 0),
            Err(crate::catalog_index::CatalogIndexError::InvalidName)
        );
    }

    #[test]
    fn table_index_scan_rebuilds_and_uses_the_table_prefix() {
        let database = database();
        let write = database.begin_write().unwrap();
        let mut catalog = write.open_table(CATALOG).unwrap();
        for (index_id, table_id) in [([0x69; 16], [0x32; 16]), ([0x70; 16], [0x88; 16])] {
            let mut key = 9u16.to_be_bytes().to_vec();
            key.push(1);
            key.extend_from_slice(&index_id);
            let mut columns = 1u32.to_le_bytes().to_vec();
            columns.extend_from_slice(&[0x67; 16]);
            let fields = [
                CatalogField {
                    tag: 1,
                    wire_type: WireType::Id,
                    payload: table_id.to_vec(),
                },
                CatalogField {
                    tag: 2,
                    wire_type: WireType::Ids,
                    payload: columns,
                },
                CatalogField {
                    tag: 3,
                    wire_type: WireType::U16,
                    payload: 1u16.to_le_bytes().to_vec(),
                },
                CatalogField {
                    tag: 4,
                    wire_type: WireType::Bool,
                    payload: vec![0],
                },
                CatalogField {
                    tag: 5,
                    wire_type: WireType::U16,
                    payload: 1u16.to_le_bytes().to_vec(),
                },
                CatalogField {
                    tag: 7,
                    wire_type: WireType::Id,
                    payload: [0x77; 16].to_vec(),
                },
                CatalogField {
                    tag: 8,
                    wire_type: WireType::U16,
                    payload: 1u16.to_le_bytes().to_vec(),
                },
                CatalogField {
                    tag: 9,
                    wire_type: WireType::U32,
                    payload: 1u32.to_le_bytes().to_vec(),
                },
                CatalogField {
                    tag: 10,
                    wire_type: WireType::U64,
                    payload: 1u64.to_le_bytes().to_vec(),
                },
            ];
            let value = encode_envelope(&family_schema(9).unwrap(), &key, &fields).unwrap();
            catalog.insert(key.as_slice(), value.as_slice()).unwrap();
        }
        drop(catalog);
        drop(write.open_table(TABLE_INDEXES).unwrap());
        write.delete_table(TABLE_INDEXES).unwrap();
        write.commit().unwrap();

        ensure_table_index_index(&database).unwrap();
        let read = database.begin_read().unwrap();
        assert_eq!(
            scan_index_ids_for_table(&read, [0x32; 16], 1).unwrap(),
            vec![[0x69; 16]]
        );
        assert_eq!(
            scan_index_ids_for_table(&read, [0x32; 16], 0),
            Err(crate::catalog_index::CatalogIndexError::InvalidName)
        );
        assert_eq!(
            read.open_table(TABLE_INDEXES)
                .unwrap()
                .iter()
                .unwrap()
                .count(),
            2
        );
    }

    #[test]
    fn table_constraint_scan_rebuilds_and_uses_the_table_prefix() {
        let database = database();
        let write = database.begin_write().unwrap();
        let mut catalog = write.open_table(CATALOG).unwrap();
        for (constraint_id, table_id) in [([0x69; 16], [0x32; 16]), ([0x70; 16], [0x88; 16])] {
            let mut key = 8u16.to_be_bytes().to_vec();
            key.push(1);
            key.extend_from_slice(&constraint_id);
            let mut columns = 1u32.to_le_bytes().to_vec();
            columns.extend_from_slice(&[0x67; 16]);
            let empty_ids = 0u32.to_le_bytes().to_vec();
            let fields = [
                CatalogField {
                    tag: 1,
                    wire_type: WireType::Id,
                    payload: table_id.to_vec(),
                },
                CatalogField {
                    tag: 2,
                    wire_type: WireType::U16,
                    payload: 1u16.to_le_bytes().to_vec(),
                },
                CatalogField {
                    tag: 3,
                    wire_type: WireType::Ids,
                    payload: columns,
                },
                CatalogField {
                    tag: 6,
                    wire_type: WireType::Ids,
                    payload: empty_ids,
                },
                CatalogField {
                    tag: 7,
                    wire_type: WireType::U16,
                    payload: 0u16.to_le_bytes().to_vec(),
                },
                CatalogField {
                    tag: 8,
                    wire_type: WireType::U16,
                    payload: 0u16.to_le_bytes().to_vec(),
                },
                CatalogField {
                    tag: 9,
                    wire_type: WireType::U16,
                    payload: 0u16.to_le_bytes().to_vec(),
                },
                CatalogField {
                    tag: 10,
                    wire_type: WireType::U16,
                    payload: 1u16.to_le_bytes().to_vec(),
                },
            ];
            let value = encode_envelope(&family_schema(8).unwrap(), &key, &fields).unwrap();
            catalog.insert(key.as_slice(), value.as_slice()).unwrap();
        }
        drop(catalog);
        drop(write.open_table(TABLE_CONSTRAINTS).unwrap());
        write.delete_table(TABLE_CONSTRAINTS).unwrap();
        write.commit().unwrap();

        ensure_table_constraint_index(&database).unwrap();
        let read = database.begin_read().unwrap();
        assert_eq!(
            scan_constraint_ids_for_table(&read, [0x32; 16], 1).unwrap(),
            vec![[0x69; 16]]
        );
        assert_eq!(
            scan_constraint_ids_for_table(&read, [0x32; 16], 0),
            Err(crate::catalog_index::CatalogIndexError::InvalidName)
        );
        assert_eq!(
            read.open_table(TABLE_CONSTRAINTS)
                .unwrap()
                .iter()
                .unwrap()
                .count(),
            2
        );
    }

    #[test]
    fn stale_column_lookups_are_rebuilt_from_catalog_records() {
        let database = database();
        let write = database.begin_write().unwrap();
        write
            .open_table(COLUMN_LOOKUPS)
            .unwrap()
            .insert(&[1, 0x32][..], &b"stale"[..])
            .unwrap();
        write.commit().unwrap();
        ensure_column_lookup_index(&database).unwrap();
        let read = database.begin_read().unwrap();
        assert_eq!(
            lookup_column_id_by_name(&read, [0x32; 16], "order_id").unwrap(),
            Some([0x67; 16])
        );
        assert_eq!(
            lookup_column_id_by_ordinal(&read, [0x32; 16], 0).unwrap(),
            Some([0x67; 16])
        );
        assert!(
            read.open_table(COLUMN_LOOKUPS)
                .unwrap()
                .get(&[1, 0x32][..])
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn missing_index_name_lookup_is_rebuilt_from_catalog_records() {
        let database = database();
        let write = database.begin_write().unwrap();
        assert!(write.delete_table(INDEX_NAMES).unwrap());
        write.commit().unwrap();
        ensure_index_name_index(&database).unwrap();
        let read = database.begin_read().unwrap();
        assert_eq!(
            lookup_index_id_by_name(&read, [0x31; 16], "orders_idx").unwrap(),
            Some([0x69; 16])
        );
    }

    #[test]
    fn stale_index_name_lookup_is_rebuilt_from_catalog_records() {
        let database = database();
        let write = database.begin_write().unwrap();
        write
            .open_table(INDEX_NAMES)
            .unwrap()
            .insert(&[0x31, 0x73][..], &[0x77; 16][..])
            .unwrap();
        write.commit().unwrap();
        ensure_index_name_index(&database).unwrap();
        let read = database.begin_read().unwrap();
        assert_eq!(
            lookup_index_id_by_name(&read, [0x31; 16], "orders_idx").unwrap(),
            Some([0x69; 16])
        );
        assert!(
            read.open_table(INDEX_NAMES)
                .unwrap()
                .get(&[0x31, 0x73][..])
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn startup_name_domains_reject_relation_index_collisions() {
        let database = database();
        let index_id = [0x69; 16];
        let index_key = {
            let mut key = 2u16.to_be_bytes().to_vec();
            key.push(1);
            key.extend_from_slice(&index_id);
            key
        };
        let conflict_fields = [
            CatalogField {
                tag: 1,
                wire_type: WireType::U16,
                payload: 7u16.to_le_bytes().to_vec(),
            },
            CatalogField {
                tag: 2,
                wire_type: WireType::Id,
                payload: [0x31; 16].to_vec(),
            },
            CatalogField {
                tag: 3,
                wire_type: WireType::Text,
                payload: b"orders".to_vec(),
            },
            CatalogField {
                tag: 4,
                wire_type: WireType::Id,
                payload: [0x34; 16].to_vec(),
            },
            CatalogField {
                tag: 5,
                wire_type: WireType::U64,
                payload: 1u64.to_le_bytes().to_vec(),
            },
            CatalogField {
                tag: 6,
                wire_type: WireType::U16,
                payload: 1u16.to_le_bytes().to_vec(),
            },
        ];
        let conflict_value =
            encode_envelope(&family_schema(2).unwrap(), &index_key, &conflict_fields).unwrap();
        let write = database.begin_write().unwrap();
        write
            .open_table(CATALOG)
            .unwrap()
            .insert(index_key.as_slice(), conflict_value.as_slice())
            .unwrap();
        write.commit().unwrap();
        ensure_relation_name_index(&database).unwrap();
        ensure_index_name_index(&database).unwrap();
        let read = database.begin_read().unwrap();
        assert!(validate_name_domains(&read).is_err());
    }

    #[test]
    fn stale_policy_index_is_rebuilt_from_catalog_records() {
        let database = database();
        let mut stale_key = vec![0x32; 16];
        stale_key.extend_from_slice(&1u16.to_be_bytes());
        stale_key.extend_from_slice(&[0x64; 16]);
        let write = database.begin_write().unwrap();
        write
            .open_table(POLICIES)
            .unwrap()
            .insert(stale_key.as_slice(), &b"stale"[..])
            .unwrap();
        write.commit().unwrap();
        ensure_policy_index(&database).unwrap();
        let read = database.begin_read().unwrap();
        let policies = lookup_policies_for_table_operation(&read, [0x32; 16], 1).unwrap();
        assert_eq!(policies.len(), 1);
        assert_eq!(policies[0].logical_key[3..19], [0x62; 16]);
    }

    #[test]
    fn missing_policy_index_is_rebuilt_from_catalog_records() {
        let database = database();
        let write = database.begin_write().unwrap();
        assert!(write.delete_table(POLICIES).unwrap());
        write.commit().unwrap();
        ensure_policy_index(&database).unwrap();
        let read = database.begin_read().unwrap();
        assert_eq!(
            lookup_policies_for_table_operation(&read, [0x32; 16], 1)
                .unwrap()
                .len(),
            1
        );
    }
}
