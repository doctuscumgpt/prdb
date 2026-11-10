//! Snapshot-aware lookups through engine-maintained catalog secondary indexes.

use crate::catalog::decode_registered_envelope;
use crate::schema::{CATALOG, PRINCIPAL_NAMES, SCHEMA_NAMES};
use redb::ReadTransaction;

/// Failure while reading a derived catalog index.
#[derive(Debug, Eq, PartialEq)]
pub enum CatalogIndexError {
    InvalidName,
    InvalidEntry,
    Storage(String),
}

impl std::fmt::Display for CatalogIndexError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "catalog index lookup failed: {self:?}")
    }
}

impl std::error::Error for CatalogIndexError {}

/// Resolve an exact normalized user or role name in the supplied redb snapshot.
///
/// Callers normalize and authorize the name before lookup. The index and the
/// family-10 catalog rows share a redb transaction, so an older read snapshot
/// continues to resolve the name as it existed when that snapshot was opened.
pub fn lookup_principal_id_by_name(
    transaction: &ReadTransaction,
    normalized_name: &str,
) -> Result<Option<[u8; 16]>, CatalogIndexError> {
    if normalized_name.is_empty() || normalized_name.len() > 4096 {
        return Err(CatalogIndexError::InvalidName);
    }
    let table = transaction
        .open_table(PRINCIPAL_NAMES)
        .map_err(|error| CatalogIndexError::Storage(error.to_string()))?;
    let Some(value) = table
        .get(normalized_name.as_bytes())
        .map_err(|error| CatalogIndexError::Storage(error.to_string()))?
    else {
        return Ok(None);
    };
    let principal_id: [u8; 16] = value
        .value()
        .try_into()
        .map_err(|_| CatalogIndexError::InvalidEntry)?;
    if principal_id == [0; 16] {
        return Err(CatalogIndexError::InvalidEntry);
    }
    drop(value);
    drop(table);
    let mut principal_key = 10u16.to_be_bytes().to_vec();
    principal_key.push(1);
    principal_key.extend_from_slice(&principal_id);
    let catalog = transaction
        .open_table(CATALOG)
        .map_err(|error| CatalogIndexError::Storage(error.to_string()))?;
    let Some(record_bytes) = catalog
        .get(principal_key.as_slice())
        .map_err(|error| CatalogIndexError::Storage(error.to_string()))?
    else {
        return Err(CatalogIndexError::InvalidEntry);
    };
    let record = decode_registered_envelope(record_bytes.value())
        .map_err(|_| CatalogIndexError::InvalidEntry)?;
    if record.family_code != 10 || record.logical_key != principal_key {
        return Err(CatalogIndexError::InvalidEntry);
    }
    let catalog_name = record
        .fields
        .iter()
        .find(|field| field.tag == 2)
        .ok_or(CatalogIndexError::InvalidEntry)?;
    if catalog_name.payload != normalized_name.as_bytes() {
        return Err(CatalogIndexError::InvalidEntry);
    }
    Ok(Some(principal_id))
}

/// Resolve an exact normalized schema name in the supplied redb snapshot.
/// The indexed ID is checked against the authoritative family-2 schema object.
pub fn lookup_schema_id_by_name(
    transaction: &ReadTransaction,
    normalized_name: &str,
) -> Result<Option<[u8; 16]>, CatalogIndexError> {
    if normalized_name.is_empty() || normalized_name.len() > 4096 {
        return Err(CatalogIndexError::InvalidName);
    }
    let table = transaction
        .open_table(SCHEMA_NAMES)
        .map_err(|error| CatalogIndexError::Storage(error.to_string()))?;
    let Some(value) = table
        .get(normalized_name.as_bytes())
        .map_err(|error| CatalogIndexError::Storage(error.to_string()))?
    else {
        return Ok(None);
    };
    let schema_id: [u8; 16] = value
        .value()
        .try_into()
        .map_err(|_| CatalogIndexError::InvalidEntry)?;
    if schema_id == [0; 16] {
        return Err(CatalogIndexError::InvalidEntry);
    }
    drop(value);
    drop(table);
    let mut object_key = 2u16.to_be_bytes().to_vec();
    object_key.push(1);
    object_key.extend_from_slice(&schema_id);
    let catalog = transaction
        .open_table(CATALOG)
        .map_err(|error| CatalogIndexError::Storage(error.to_string()))?;
    let Some(record_bytes) = catalog
        .get(object_key.as_slice())
        .map_err(|error| CatalogIndexError::Storage(error.to_string()))?
    else {
        return Err(CatalogIndexError::InvalidEntry);
    };
    let record = decode_registered_envelope(record_bytes.value())
        .map_err(|_| CatalogIndexError::InvalidEntry)?;
    if record.family_code != 2 || record.logical_key != object_key {
        return Err(CatalogIndexError::InvalidEntry);
    }
    let kind = record
        .fields
        .iter()
        .find(|field| field.tag == 1)
        .ok_or(CatalogIndexError::InvalidEntry)?;
    let name = record
        .fields
        .iter()
        .find(|field| field.tag == 3)
        .ok_or(CatalogIndexError::InvalidEntry)?;
    if kind.payload.as_slice() != 1u16.to_le_bytes() || name.payload != normalized_name.as_bytes() {
        return Err(CatalogIndexError::InvalidEntry);
    }
    Ok(Some(schema_id))
}
