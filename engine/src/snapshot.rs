//! Read-only row access through an invocation's retained redb snapshot.
use crate::catalog::{CatalogRecord, InvalidCatalogRecord, decode_registered_envelope};
use crate::expression::decode_catalog_expression_record;
use crate::index_locator::{IndexLocator, LocatorError};
use crate::schema::{CATALOG, ENGINE_META, INDEXES, POLICIES, RELATION_NAMES, ROWS, SCHEMA_NAMES};
use redb::ReadTransaction;

pub const MAX_CATALOG_FAMILY_SCAN: usize = 65_536;
pub(crate) const MAX_ROW_SCAN_PAGE: usize = 1_024;
const MAX_INDEX_SCAN_PAGE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum SnapshotError {
    InvalidTableId,
    InvalidCursor,
    InvalidScanLimit,
    InvalidRowKey,
    InvalidIndexPrefix,
    InvalidIndexCursor,
    InvalidIndexKey,
    InvalidIndexLocator,
    InvalidCatalogRecord,
    RecordLimitExceeded,
    Storage(String),
}

impl std::fmt::Display for SnapshotError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "snapshot scan failed: {self:?}")
    }
}

impl std::error::Error for SnapshotError {}

/// Owns one redb read transaction for the lifetime of the invocation snapshot.
pub struct RowSnapshot {
    transaction: ReadTransaction,
}

/// One validated row/version candidate returned by an ordered index prefix scan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IndexCandidate {
    pub key: Vec<u8>,
    pub locator: IndexLocator,
}

/// A bounded page from one pinned index prefix. A continuation is the last
/// complete key; pass it back to resume strictly after that key.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IndexScanPage {
    pub candidates: Vec<IndexCandidate>,
    pub continuation: Option<Vec<u8>>,
}

impl RowSnapshot {
    pub fn new(transaction: ReadTransaction) -> Self {
        Self { transaction }
    }

    /// Copy engine metadata from the same retained read transaction as catalog
    /// and row access.
    pub fn get_engine_metadata_value(&self) -> Result<Option<Vec<u8>>, Box<dyn std::error::Error>> {
        let table = self.transaction.open_table(ENGINE_META)?;
        Ok(table
            .get(&b"format"[..])?
            .map(|value| value.value().to_vec()))
    }

    /// Copy one row value out of its redb guard. No guard or database borrow escapes.
    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, Box<dyn std::error::Error>> {
        let table = self.transaction.open_table(ROWS)?;
        Ok(table.get(key)?.map(|value| value.value().to_vec()))
    }

    /// Return the roles granted directly to a member through the validated
    /// snapshot index. The lookup checks each derived entry against family 11.
    pub(crate) fn lookup_roles_for_member(
        &self,
        member_id: [u8; 16],
    ) -> Result<Vec<[u8; 16]>, SnapshotError> {
        crate::catalog_index::lookup_roles_for_member(&self.transaction, member_id).map_err(
            |error| match error {
                crate::catalog_index::CatalogIndexError::Storage(error) => {
                    SnapshotError::Storage(error)
                }
                crate::catalog_index::CatalogIndexError::InvalidName
                | crate::catalog_index::CatalogIndexError::InvalidEntry => {
                    SnapshotError::InvalidCatalogRecord
                }
                crate::catalog_index::CatalogIndexError::RecordLimitExceeded => {
                    SnapshotError::InvalidCatalogRecord
                }
            },
        )
    }

    pub(crate) fn scan_relation_ids_for_schema(
        &self,
        schema_id: [u8; 16],
        max_records: usize,
    ) -> Result<Vec<[u8; 16]>, SnapshotError> {
        crate::catalog_index::scan_relation_ids_for_schema(
            &self.transaction,
            schema_id,
            max_records,
        )
        .map_err(|error| match error {
            crate::catalog_index::CatalogIndexError::InvalidName
            | crate::catalog_index::CatalogIndexError::InvalidEntry => {
                SnapshotError::InvalidCatalogRecord
            }
            crate::catalog_index::CatalogIndexError::RecordLimitExceeded => {
                SnapshotError::RecordLimitExceeded
            }
            crate::catalog_index::CatalogIndexError::Storage(error) => {
                SnapshotError::Storage(error)
            }
        })
    }

    pub(crate) fn scan_object_ids_for_owner(
        &self,
        owner_id: [u8; 16],
        max_records: usize,
    ) -> Result<Vec<[u8; 16]>, SnapshotError> {
        crate::catalog_index::scan_object_ids_for_owner(&self.transaction, owner_id, max_records)
            .map_err(|error| match error {
                crate::catalog_index::CatalogIndexError::InvalidName
                | crate::catalog_index::CatalogIndexError::InvalidEntry => {
                    SnapshotError::InvalidCatalogRecord
                }
                crate::catalog_index::CatalogIndexError::RecordLimitExceeded => {
                    SnapshotError::RecordLimitExceeded
                }
                crate::catalog_index::CatalogIndexError::Storage(error) => {
                    SnapshotError::Storage(error)
                }
            })
    }

    /// List one table's columns in ordinal order from this pinned snapshot.
    pub(crate) fn scan_column_ids_for_table(
        &self,
        table_id: [u8; 16],
        max_records: usize,
    ) -> Result<Vec<[u8; 16]>, SnapshotError> {
        crate::catalog_index::scan_column_ids_for_table(&self.transaction, table_id, max_records)
            .map_err(|error| match error {
                crate::catalog_index::CatalogIndexError::InvalidName
                | crate::catalog_index::CatalogIndexError::InvalidEntry => {
                    SnapshotError::InvalidCatalogRecord
                }
                crate::catalog_index::CatalogIndexError::RecordLimitExceeded => {
                    SnapshotError::RecordLimitExceeded
                }
                crate::catalog_index::CatalogIndexError::Storage(error) => {
                    SnapshotError::Storage(error)
                }
            })
    }

    /// List one table's index definitions from this pinned snapshot.
    pub(crate) fn scan_index_ids_for_table(
        &self,
        table_id: [u8; 16],
        max_records: usize,
    ) -> Result<Vec<[u8; 16]>, SnapshotError> {
        crate::catalog_index::scan_index_ids_for_table(&self.transaction, table_id, max_records)
            .map_err(|error| match error {
                crate::catalog_index::CatalogIndexError::InvalidName
                | crate::catalog_index::CatalogIndexError::InvalidEntry => {
                    SnapshotError::InvalidCatalogRecord
                }
                crate::catalog_index::CatalogIndexError::RecordLimitExceeded => {
                    SnapshotError::RecordLimitExceeded
                }
                crate::catalog_index::CatalogIndexError::Storage(error) => {
                    SnapshotError::Storage(error)
                }
            })
    }

    /// List one table's constraints from this pinned snapshot.
    pub(crate) fn scan_constraint_ids_for_table(
        &self,
        table_id: [u8; 16],
        max_records: usize,
    ) -> Result<Vec<[u8; 16]>, SnapshotError> {
        crate::catalog_index::scan_constraint_ids_for_table(
            &self.transaction,
            table_id,
            max_records,
        )
        .map_err(|error| match error {
            crate::catalog_index::CatalogIndexError::InvalidName
            | crate::catalog_index::CatalogIndexError::InvalidEntry => {
                SnapshotError::InvalidCatalogRecord
            }
            crate::catalog_index::CatalogIndexError::RecordLimitExceeded => {
                SnapshotError::RecordLimitExceeded
            }
            crate::catalog_index::CatalogIndexError::Storage(error) => {
                SnapshotError::Storage(error)
            }
        })
    }

    pub(crate) fn lookup_dependents_for_referenced(
        &self,
        referenced_id: [u8; 16],
        max_records: usize,
    ) -> Result<Vec<CatalogRecord>, SnapshotError> {
        crate::catalog_index::lookup_dependents_for_referenced_limited(
            &self.transaction,
            referenced_id,
            max_records,
        )
        .map_err(|error| match error {
            crate::catalog_index::CatalogIndexError::InvalidName
            | crate::catalog_index::CatalogIndexError::InvalidEntry => {
                SnapshotError::InvalidCatalogRecord
            }
            crate::catalog_index::CatalogIndexError::RecordLimitExceeded => {
                SnapshotError::RecordLimitExceeded
            }
            crate::catalog_index::CatalogIndexError::Storage(error) => {
                SnapshotError::Storage(error)
            }
        })
    }

    /// Return grants indexed under one grantee from the pinned catalog
    /// snapshot. Each derived entry is checked against family 12.
    pub(crate) fn lookup_grants_for_grantee(
        &self,
        grantee_id: [u8; 16],
    ) -> Result<Vec<CatalogRecord>, SnapshotError> {
        crate::catalog_index::lookup_grants_for_grantee(&self.transaction, grantee_id).map_err(
            |error| match error {
                crate::catalog_index::CatalogIndexError::Storage(error) => {
                    SnapshotError::Storage(error)
                }
                crate::catalog_index::CatalogIndexError::InvalidName
                | crate::catalog_index::CatalogIndexError::InvalidEntry => {
                    SnapshotError::InvalidCatalogRecord
                }
                crate::catalog_index::CatalogIndexError::RecordLimitExceeded => {
                    SnapshotError::InvalidCatalogRecord
                }
            },
        )
    }

    /// Return a bounded, ordered page of row IDs from this pinned snapshot.
    /// A full page is reported as incomplete; callers can resume after its last
    /// row ID, and an empty follow-up page confirms completion.
    pub(crate) fn scan_row_ids_after(
        &self,
        table_id: [u8; 16],
        after_row_id: Option<[u8; 16]>,
        limit: usize,
    ) -> Result<(Vec<[u8; 16]>, bool), SnapshotError> {
        if table_id == [0; 16] {
            return Err(SnapshotError::InvalidTableId);
        }
        if after_row_id.is_some_and(|row_id| row_id == [0; 16]) {
            return Err(SnapshotError::InvalidCursor);
        }
        if limit == 0 || limit > MAX_ROW_SCAN_PAGE {
            return Err(SnapshotError::InvalidScanLimit);
        }

        let mut start_key = Vec::with_capacity(32);
        start_key.extend_from_slice(&table_id);
        if let Some(row_id) = after_row_id {
            start_key.extend_from_slice(&row_id);
        }
        let table = self
            .transaction
            .open_table(ROWS)
            .map_err(|error| SnapshotError::Storage(error.to_string()))?;
        let mut row_ids = Vec::with_capacity(limit);
        for entry in table
            .range::<&[u8]>(start_key.as_slice()..)
            .map_err(|error| SnapshotError::Storage(error.to_string()))?
        {
            let (key, _) = entry.map_err(|error| SnapshotError::Storage(error.to_string()))?;
            let key = key.value();
            if !key.starts_with(&table_id) {
                break;
            }
            if key.len() != 32 {
                return Err(SnapshotError::InvalidRowKey);
            }
            let row_id: [u8; 16] = key[16..]
                .try_into()
                .map_err(|_| SnapshotError::InvalidRowKey)?;
            if row_id == [0; 16] {
                return Err(SnapshotError::InvalidRowKey);
            }
            if after_row_id.is_some_and(|after| row_id <= after) {
                continue;
            }
            row_ids.push(row_id);
            if row_ids.len() == limit {
                break;
            }
        }
        let complete = row_ids.len() < limit;
        Ok((row_ids, complete))
    }

    /// Read one logical index locator from this invocation's pinned snapshot.
    pub fn get_index_version(&self, key: &[u8]) -> Result<Option<u64>, Box<dyn std::error::Error>> {
        let table = self.transaction.open_table(INDEXES)?;
        let value = table.get(key)?.map(|value| value.value().to_vec());
        value
            .map(|bytes| {
                IndexLocator::decode_for_key(key, &bytes)
                    .map(|locator| locator.row_version)
                    .map_err(Into::into)
            })
            .transpose()
    }

    /// Return a bounded page of validated locators whose complete keys share
    /// `prefix`. Prefixes are search bounds, not stored keys. The returned
    /// candidates are untrusted until the transaction layer resolves them
    /// against this same snapshot/overlay and applies authorization.
    pub fn scan_index_prefix(
        &self,
        prefix: &[u8],
        after_key: Option<&[u8]>,
        limit: usize,
    ) -> Result<IndexScanPage, Box<dyn std::error::Error>> {
        if prefix.len() < 16
            || prefix.len().saturating_add(16) > crate::index_key::MAX_INDEX_KEY_BYTES
            || prefix[..16] == [0; 16]
        {
            return Err(Box::new(SnapshotError::InvalidIndexPrefix));
        }
        if limit == 0 || limit > MAX_ROW_SCAN_PAGE {
            return Err(Box::new(SnapshotError::InvalidScanLimit));
        }
        if after_key.is_some_and(|key| {
            !key.starts_with(prefix)
                || key.len() < prefix.len() + 16
                || key.len() > crate::index_key::MAX_INDEX_KEY_BYTES
        }) {
            return Err(Box::new(SnapshotError::InvalidIndexCursor));
        }

        let start_key = after_key.unwrap_or(prefix);
        let table = self.transaction.open_table(INDEXES)?;
        let mut candidates = Vec::with_capacity(limit);
        let mut page_bytes = 0usize;
        let mut has_more = false;
        for entry in table.range::<&[u8]>(start_key..)? {
            let (key, value) = entry?;
            let key = key.value();
            if !key.starts_with(prefix) {
                break;
            }
            if after_key.is_some_and(|after| key <= after) {
                continue;
            }
            if candidates.len() == limit
                || page_bytes.saturating_add(key.len()) > MAX_INDEX_SCAN_PAGE_BYTES
            {
                has_more = true;
                break;
            }
            if key.len() < prefix.len() + 16 || key.len() > crate::index_key::MAX_INDEX_KEY_BYTES {
                return Err(Box::new(SnapshotError::InvalidIndexKey));
            }
            let locator = IndexLocator::decode_for_key(key, value.value())
                .map_err(|_: LocatorError| SnapshotError::InvalidIndexLocator)?;
            if locator.row_id == [0; 16] {
                return Err(Box::new(SnapshotError::InvalidIndexLocator));
            }
            candidates.push(IndexCandidate {
                key: key.to_vec(),
                locator,
            });
            page_bytes += key.len();
        }
        let continuation = has_more
            .then(|| candidates.last().map(|candidate| candidate.key.clone()))
            .flatten();
        Ok(IndexScanPage {
            candidates,
            continuation,
        })
    }

    /// Copy and validate one logical catalog record from this pinned snapshot.
    pub fn get_catalog_record(
        &self,
        key: &[u8],
    ) -> Result<Option<CatalogRecord>, Box<dyn std::error::Error>> {
        self.get_catalog_value(key)?
            .map(|bytes| {
                let record = decode_registered_envelope(&bytes)?;
                if record.logical_key != key {
                    return Err(InvalidCatalogRecord.into());
                }
                if record.family_code == 7 {
                    decode_catalog_expression_record(&record)?;
                }
                Ok(record)
            })
            .transpose()
    }

    /// Copy one raw catalog envelope from this pinned snapshot.
    pub fn get_catalog_value(
        &self,
        key: &[u8],
    ) -> Result<Option<Vec<u8>>, Box<dyn std::error::Error>> {
        let table = self.transaction.open_table(CATALOG)?;
        Ok(table.get(key)?.map(|value| value.value().to_vec()))
    }

    /// Copy an exact schema-name index entry from this pinned snapshot.
    pub fn get_schema_name_index_value(
        &self,
        name: &[u8],
    ) -> Result<Option<Vec<u8>>, Box<dyn std::error::Error>> {
        let table = self.transaction.open_table(SCHEMA_NAMES)?;
        Ok(table.get(name)?.map(|value| value.value().to_vec()))
    }

    /// Copy one schema-scoped relation-name index entry from this snapshot.
    pub fn get_relation_name_index_value(
        &self,
        schema_id: [u8; 16],
        name: &[u8],
    ) -> Result<Option<Vec<u8>>, Box<dyn std::error::Error>> {
        let mut key = Vec::with_capacity(schema_id.len() + name.len());
        key.extend_from_slice(&schema_id);
        key.extend_from_slice(name);
        let table = self.transaction.open_table(RELATION_NAMES)?;
        Ok(table
            .get(key.as_slice())?
            .map(|value| value.value().to_vec()))
    }

    /// List policy IDs for one table in operation/policy order from this snapshot.
    pub fn scan_policy_ids_for_table(
        &self,
        table_id: [u8; 16],
        max_records: usize,
    ) -> Result<Vec<[u8; 16]>, Box<dyn std::error::Error>> {
        self.scan_policy_ids_for_table_limited(table_id, max_records)
            .map_err(|error| Box::new(error) as Box<dyn std::error::Error>)
    }

    pub(crate) fn scan_policy_ids_for_table_limited(
        &self,
        table_id: [u8; 16],
        max_records: usize,
    ) -> Result<Vec<[u8; 16]>, SnapshotError> {
        if table_id == [0; 16] || max_records == 0 || max_records > MAX_CATALOG_FAMILY_SCAN {
            return Err(SnapshotError::InvalidCatalogRecord);
        }
        let table = self
            .transaction
            .open_table(POLICIES)
            .map_err(|error| SnapshotError::Storage(error.to_string()))?;
        let mut ids = Vec::new();
        for entry in table
            .range::<&[u8]>(table_id.as_slice()..)
            .map_err(|error| SnapshotError::Storage(error.to_string()))?
        {
            let (key, value) = entry.map_err(|error| SnapshotError::Storage(error.to_string()))?;
            let key = key.value();
            if !key.starts_with(&table_id) {
                break;
            }
            if !value.value().is_empty()
                || key.len() != 34
                || !matches!(u16::from_be_bytes([key[16], key[17]]), 1..=4)
            {
                return Err(SnapshotError::InvalidCatalogRecord);
            }
            let policy_id: [u8; 16] = key[18..]
                .try_into()
                .map_err(|_| SnapshotError::InvalidCatalogRecord)?;
            if policy_id == [0; 16] {
                return Err(SnapshotError::InvalidCatalogRecord);
            }
            if ids.len() == max_records {
                return Err(SnapshotError::RecordLimitExceeded);
            }
            ids.push(policy_id);
        }
        Ok(ids)
    }

    /// Copy the bounded target records for one policy from this pinned
    /// snapshot. The family-14 key begins with the policy ID, so unrelated
    /// policies do not contribute to the scan limit.
    pub fn scan_policy_targets_for_policy(
        &self,
        policy_id: [u8; 16],
        max_records: usize,
    ) -> Result<Vec<CatalogRecord>, Box<dyn std::error::Error>> {
        self.scan_policy_targets_for_policy_limited(policy_id, max_records)
            .map_err(|error| Box::new(error) as Box<dyn std::error::Error>)
    }

    pub(crate) fn scan_policy_targets_for_policy_limited(
        &self,
        policy_id: [u8; 16],
        max_records: usize,
    ) -> Result<Vec<CatalogRecord>, SnapshotError> {
        if policy_id == [0; 16] || max_records == 0 || max_records > MAX_CATALOG_FAMILY_SCAN {
            return Err(SnapshotError::InvalidCatalogRecord);
        }
        let mut prefix = 14u16.to_be_bytes().to_vec();
        prefix.push(1);
        prefix.extend_from_slice(&policy_id);
        let table = self
            .transaction
            .open_table(CATALOG)
            .map_err(|error| SnapshotError::Storage(error.to_string()))?;
        let mut records = Vec::new();
        for entry in table
            .range(prefix.as_slice()..)
            .map_err(|error| SnapshotError::Storage(error.to_string()))?
        {
            let (key, value) = entry.map_err(|error| SnapshotError::Storage(error.to_string()))?;
            let key = key.value().to_vec();
            if !key.starts_with(&prefix) {
                break;
            }
            if records.len() == max_records {
                return Err(SnapshotError::RecordLimitExceeded);
            }
            let record = decode_registered_envelope(value.value())
                .map_err(|_| SnapshotError::InvalidCatalogRecord)?;
            if record.family_code != 14
                || record.logical_key != key
                || key.len() != 39
                || key[19] != 1
                || !matches!(u16::from_be_bytes([key[20], key[21]]), 1..=3)
                || key[22] != 1
                || key[23..39] == [0; 16]
            {
                return Err(SnapshotError::InvalidCatalogRecord);
            }
            records.push(record);
        }
        Ok(records)
    }

    /// Copy a bounded family prefix from the pinned catalog snapshot. Callers
    /// that use returned records for authorization or writes must also record
    /// exact-key guards and guard the owning definition's epoch.
    pub fn scan_catalog_family(
        &self,
        family_code: u16,
        max_records: usize,
    ) -> Result<Vec<CatalogRecord>, Box<dyn std::error::Error>> {
        self.scan_catalog_family_limited(family_code, max_records)
            .map_err(|error| Box::new(error) as Box<dyn std::error::Error>)
    }

    pub(crate) fn scan_catalog_family_limited(
        &self,
        family_code: u16,
        max_records: usize,
    ) -> Result<Vec<CatalogRecord>, SnapshotError> {
        if !(1..=16).contains(&family_code)
            || max_records == 0
            || max_records > MAX_CATALOG_FAMILY_SCAN
        {
            return Err(SnapshotError::InvalidCatalogRecord);
        }
        let prefix = family_code.to_be_bytes();
        let table = self
            .transaction
            .open_table(CATALOG)
            .map_err(|error| SnapshotError::Storage(error.to_string()))?;
        let mut records = Vec::new();
        for entry in table
            .range(prefix.as_slice()..)
            .map_err(|error| SnapshotError::Storage(error.to_string()))?
        {
            let (key, value) = entry.map_err(|error| SnapshotError::Storage(error.to_string()))?;
            let key = key.value().to_vec();
            if !key.starts_with(&prefix) {
                break;
            }
            if records.len() == max_records {
                return Err(SnapshotError::RecordLimitExceeded);
            }
            let bytes = value.value().to_vec();
            let record = decode_registered_envelope(&bytes)
                .map_err(|_| SnapshotError::InvalidCatalogRecord)?;
            if record.logical_key != key || record.family_code != family_code {
                return Err(SnapshotError::InvalidCatalogRecord);
            }
            if family_code == 7 {
                decode_catalog_expression_record(&record)
                    .map_err(|_| SnapshotError::InvalidCatalogRecord)?;
            }
            records.push(record);
        }
        Ok(records)
    }
}
