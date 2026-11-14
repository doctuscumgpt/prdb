use prdb_engine::audit::{AuditAction, AuditEvent, AuditResult};
use prdb_engine::bootstrap::initialize;
use prdb_engine::row_record::RowRecord;
use prdb_engine::schema::ROWS;
use prdb_engine::storage_manager::{
    CreateTableRequest, StorageManager, TableColumnDefault, TableColumnDefinition,
};
use prdb_engine::tuple::{encode_tuple, ColumnSchema, ScalarType, ScalarValue};
use redb_v2::{
    Database, ReadableTable as ReadableTableV2, ReadableTableMetadata as ReadableTableMetadataV2,
    TableDefinition,
};
use redb_v4::{ReadableDatabase as ReadableDatabaseV4, ReadableTable as ReadableTableV4};
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use zeroize::Zeroize;

const FIXTURE_TABLE: TableDefinition<&[u8], &[u8]> =
    TableDefinition::new("compatibility_fixture_v1");
const ENGINE_TABLES: [&str; 20] = prdb_engine::schema::ENGINE_TABLE_NAMES;

struct TemporaryDirectory(PathBuf);

impl TemporaryDirectory {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path = std::env::temp_dir().join(format!("prdb-redb-v2-generator-{nonce}"));
        std::fs::DirBuilder::new().mode(0o700).create(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let operation = args
        .next()
        .ok_or("operation is required: create-v2, create-populated-prdb-v2, verify-v2, verify-populated-prdb-v2, or upgrade-v2-to-v3")?;
    let path = args.next().ok_or("database path is required")?;
    if args.next().is_some() {
        return Err("unexpected trailing arguments".into());
    }

    match operation.to_str() {
        Some("create-v2") => create_small_v2(Path::new(&path))?,
        Some("create-populated-prdb-v2") => create_populated_prdb_v2(Path::new(&path))?,
        Some("verify-v2") => verify_small_v2(Path::new(&path))?,
        Some("verify-populated-prdb-v2") => verify_populated_prdb_v2(Path::new(&path))?,
        Some("upgrade-v2-to-v3") => {
            let mut database = Database::open(Path::new(&path))?;
            if !database.upgrade()? {
                return Err("database was already upgraded".into());
            }
        }
        _ => return Err("unknown operation".into()),
    }
    Ok(())
}

fn create_small_v2(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let database = Database::create(path)?;
    let write = database.begin_write()?;
    {
        let mut table = write.open_table(FIXTURE_TABLE)?;
        table.insert(&b"catalog-version"[..], &b"1"[..])?;
        table.insert(&b"payload"[..], &b"redb-file-format-v2-fixture"[..])?;
    }
    write.commit()?;
    Ok(())
}

fn verify_small_v2(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let database = Database::open(path)?;
    let read = database.begin_read()?;
    let table = read.open_table(FIXTURE_TABLE)?;
    if table.len()? != 2 {
        return Err("fixture contains an unexpected key count".into());
    }
    let version = table
        .get(&b"catalog-version"[..])?
        .ok_or("catalog-version key is missing")?;
    let payload = table
        .get(&b"payload"[..])?
        .ok_or("payload key is missing")?;
    if version.value() != b"1" || payload.value() != b"redb-file-format-v2-fixture" {
        return Err("fixture contents do not match the v2 generator".into());
    }
    println!("physical_file_format=v2 keys=2");
    Ok(())
}

fn verify_populated_prdb_v2(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let database = Database::open(path)?;
    let read = database.begin_read()?;
    let mut rows = 0;
    for name in ENGINE_TABLES {
        let table = read.open_table(TableDefinition::<&[u8], &[u8]>::new(name))?;
        if name == "rows_v1" {
            for entry in table.iter()? {
                let (key, value) = entry?;
                if key.value().len() != 32 {
                    return Err("application row key has an invalid length".into());
                }
                let record = RowRecord::decode(value.value())?;
                if !record
                    .payload
                    .windows(b"persisted redb v2 row".len())
                    .any(|window| window == b"persisted redb v2 row")
                {
                    return Err("application row payload does not match the fixture".into());
                }
                rows += 1;
            }
        }
    }
    if rows != 1 {
        return Err("fixture must contain exactly one application row".into());
    }
    println!(
        "physical_file_format=v2 engine_tables={} application_rows={rows}",
        ENGINE_TABLES.len()
    );
    Ok(())
}

fn create_populated_prdb_v2(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    if path.exists() {
        return Err("output file already exists".into());
    }
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    if !parent.is_dir() {
        return Err("output parent directory must already exist".into());
    }

    let temporary = TemporaryDirectory::new()?;
    let instance_dir = temporary.0.join("source-instance");
    let mut setup = initialize(&instance_dir, "fixture_admin", i64::MAX)?;
    let admin_id = setup.admin_user_id();
    if let Some(mut secret) = setup.take_secret() {
        secret.zeroize();
    }
    let instance = setup.acknowledge()?;
    let storage = StorageManager::new(instance.database(), Duration::from_millis(100))?;
    let make_audit = |event_id, correlation_id| AuditEvent {
        event_id,
        actor_id: Some(admin_id),
        correlation_id,
        occurred_at_us: 1,
        action: AuditAction::Ddl,
        result: AuditResult::Succeeded,
        target_ids: Vec::new(),
    };
    let schema = storage.create_schema(
        [0x41; 16],
        admin_id,
        "fixture_data",
        admin_id,
        i64::MAX,
        make_audit([0x42; 16], [0x41; 16]),
    )?;
    let table = storage.create_table(CreateTableRequest {
        invocation_id: [0x43; 16],
        authenticated_actor_id: admin_id,
        schema_id: schema.schema_id,
        name: "records".to_owned(),
        columns: vec![TableColumnDefinition {
            name: "payload".to_owned(),
            scalar_type: ScalarType::Text,
            nullable: false,
            generated_uuid_identity: false,
            default: Some(TableColumnDefault::Literal(Some(ScalarValue::Text(
                "default value".to_owned(),
            )))),
        }],
        retention_deadline_us: i64::MAX,
        audit_event: make_audit([0x44; 16], [0x43; 16]),
    })?;
    let mut row_key = Vec::with_capacity(32);
    row_key.extend_from_slice(&table.table_id);
    row_key.extend_from_slice(&[0x45; 16]);
    let row_payload = encode_tuple(
        &[ColumnSchema {
            scalar_type: ScalarType::Text,
            nullable: false,
        }],
        1,
        &[Some(ScalarValue::Text("persisted redb v2 row".to_owned()))],
    )?;
    let row_record = RowRecord {
        row_version: 1,
        payload: row_payload,
    }
    .encode()?;
    {
        let write = instance.database().begin_write()?;
        write
            .open_table(ROWS)?
            .insert(row_key.as_slice(), row_record.as_slice())?;
        write.commit()?;
    }
    drop(storage);
    drop(instance);

    let source = redb_v4::ReadOnlyDatabase::open(temporary.0.join("source-instance/storage.redb"))?;
    let rows = copy_engine_tables_to_v2(&source, path)?;
    let metadata = std::fs::metadata(path)?;
    if !metadata.is_file() {
        return Err("output did not produce a regular file".into());
    }
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    println!(
        "physical_file_format=v2 engine_tables={} application_rows={rows}",
        ENGINE_TABLES.len()
    );
    Ok(())
}

fn copy_engine_tables_to_v2(
    source: &redb_v4::ReadOnlyDatabase,
    output: &Path,
) -> Result<usize, Box<dyn std::error::Error>> {
    let mut table_records = Vec::with_capacity(ENGINE_TABLES.len());
    let read = source.begin_read()?;
    let mut row_count = 0;
    for name in ENGINE_TABLES {
        let definition = redb_v4::TableDefinition::<&[u8], &[u8]>::new(name);
        let table = read.open_table(definition)?;
        let mut records = Vec::new();
        for entry in table.iter()? {
            let (key, value) = entry?;
            records.push((key.value().to_vec(), value.value().to_vec()));
        }
        if name == "rows_v1" {
            row_count = records.len();
        }
        table_records.push((name, records));
    }
    drop(read);

    let database = Database::create(output)?;
    let write = database.begin_write()?;
    for (name, records) in table_records {
        let mut table = write.open_table(TableDefinition::<&[u8], &[u8]>::new(name))?;
        for (key, value) in records {
            table.insert(key.as_slice(), value.as_slice())?;
        }
    }
    write.commit()?;
    drop(database);
    Ok(row_count)
}
