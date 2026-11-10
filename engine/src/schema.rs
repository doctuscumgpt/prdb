//! Versioned redb table names and byte-oriented engine-private key/value types.
use redb::{TableDefinition, WriteTransaction};

pub const ENGINE_META: TableDefinition<&[u8], &[u8]> = TableDefinition::new("engine_meta_v1");
pub const ROWS: TableDefinition<&[u8], &[u8]> = TableDefinition::new("rows_v1");
pub const INDEXES: TableDefinition<&[u8], &[u8]> = TableDefinition::new("indexes_v1");
/// Reverse memberships make all index entries for one changed row addressable by row prefix.
pub const ROW_INDEXES: TableDefinition<&[u8], &[u8]> = TableDefinition::new("row_indexes_v1");
pub const CATALOG: TableDefinition<&[u8], &[u8]> = TableDefinition::new("catalog_v1");
/// Exact normalized user/role name to principal ID lookup, atomically mirrored from family 10.
pub const PRINCIPAL_NAMES: TableDefinition<&[u8], &[u8]> =
    TableDefinition::new("principal_names_v1");
/// Exact normalized schema name to schema object ID, mirrored from family 2.
pub const SCHEMA_NAMES: TableDefinition<&[u8], &[u8]> = TableDefinition::new("schema_names_v1");
/// Schema-scoped exact relation name to table object ID, mirrored from families 2 and 5.
pub const RELATION_NAMES: TableDefinition<&[u8], &[u8]> = TableDefinition::new("relation_names_v1");
/// Family-6 columns ordered by table/name and table/ordinal.
pub const COLUMN_LOOKUPS: TableDefinition<&[u8], &[u8]> = TableDefinition::new("column_lookups_v1");
/// Family-2 index objects ordered by parent schema and normalized name.
pub const INDEX_NAMES: TableDefinition<&[u8], &[u8]> = TableDefinition::new("index_names_v1");
/// Family-9 index definitions ordered by owning table and index ID.
pub const TABLE_INDEXES: TableDefinition<&[u8], &[u8]> = TableDefinition::new("table_indexes_v1");
/// Family-8 constraints ordered by owning table and constraint ID.
pub const TABLE_CONSTRAINTS: TableDefinition<&[u8], &[u8]> =
    TableDefinition::new("table_constraints_v1");
/// Direct membership edges stored in member-to-role and role-to-member orders.
pub const MEMBERSHIPS: TableDefinition<&[u8], &[u8]> = TableDefinition::new("memberships_v1");
/// Grant catalog keys ordered by grantee and by protected object.
pub const GRANTS: TableDefinition<&[u8], &[u8]> = TableDefinition::new("grants_v1");
/// Family-13 policy IDs ordered by table and operation.
pub const POLICIES: TableDefinition<&[u8], &[u8]> = TableDefinition::new("policies_v1");
/// Family-15 dependency edges ordered by dependent and by referenced object.
pub const DEPENDENCIES: TableDefinition<&[u8], &[u8]> = TableDefinition::new("dependencies_v1");
pub const OUTCOMES: TableDefinition<&[u8], &[u8]> = TableDefinition::new("outcomes_v1");
pub const AUDIT: TableDefinition<&[u8], &[u8]> = TableDefinition::new("audit_v1");
pub const STATS: TableDefinition<&[u8], &[u8]> = TableDefinition::new("stats_v1");

/// Create every logical engine table in the caller's redb write transaction.
/// The caller must commit or abort this transaction as one atomic unit.
pub fn create_tables(write: &WriteTransaction) -> Result<(), Box<dyn std::error::Error>> {
    drop(write.open_table(ENGINE_META)?);
    drop(write.open_table(ROWS)?);
    drop(write.open_table(INDEXES)?);
    drop(write.open_table(ROW_INDEXES)?);
    drop(write.open_table(CATALOG)?);
    drop(write.open_table(PRINCIPAL_NAMES)?);
    drop(write.open_table(SCHEMA_NAMES)?);
    drop(write.open_table(RELATION_NAMES)?);
    drop(write.open_table(COLUMN_LOOKUPS)?);
    drop(write.open_table(INDEX_NAMES)?);
    drop(write.open_table(TABLE_INDEXES)?);
    drop(write.open_table(TABLE_CONSTRAINTS)?);
    drop(write.open_table(MEMBERSHIPS)?);
    drop(write.open_table(GRANTS)?);
    drop(write.open_table(POLICIES)?);
    drop(write.open_table(DEPENDENCIES)?);
    drop(write.open_table(OUTCOMES)?);
    drop(write.open_table(AUDIT)?);
    drop(write.open_table(STATS)?);
    Ok(())
}
