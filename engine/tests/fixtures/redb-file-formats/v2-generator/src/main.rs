use redb::{Database, ReadableTableMetadata, TableDefinition};
use std::path::Path;

const TABLE: TableDefinition<&[u8], &[u8]> = TableDefinition::new("compatibility_fixture_v1");

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let operation = args
        .next()
        .ok_or("operation is required: create-v2, verify-v2, or upgrade-v2-to-v3")?;
    let path = args.next().ok_or("database path is required")?;
    if args.next().is_some() {
        return Err("unexpected trailing arguments".into());
    }

    match operation.to_str() {
        Some("create-v2") => {
            let database = Database::create(Path::new(&path))?;
            let write = database.begin_write()?;
            {
                let mut table = write.open_table(TABLE)?;
                table.insert(&b"catalog-version"[..], &b"1"[..])?;
                table.insert(&b"payload"[..], &b"redb-file-format-v2-fixture"[..])?;
            }
            write.commit()?;
        }
        Some("verify-v2") => {
            let database = Database::open(Path::new(&path))?;
            let read = database.begin_read()?;
            let table = read.open_table(TABLE)?;
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
        }
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
