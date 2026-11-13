use prdb_engine::audit::{AuditAction, AuditEvent, AuditResult};
use prdb_engine::schema::{AUDIT, create_tables};
use redb::{Database, ReadableDatabase, ReadableTableMetadata};
use std::os::unix::fs::PermissionsExt;

struct Fixture(std::path::PathBuf);

impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "prdb-audit-sink-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ));
        std::fs::create_dir(&path).expect("create audit sink fixture");
        Self(path)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn create_database(path: &std::path::Path) -> Result<Database, Box<dyn std::error::Error>> {
    let database = Database::create(path)?;
    let write = database.begin_write()?;
    create_tables(&write)?;
    {
        let mut audit = write.open_table(AUDIT)?;
        let event = AuditEvent {
            event_id: [0x11; 16],
            actor_id: Some([0x12; 16]),
            correlation_id: [0x13; 16],
            occurred_at_us: 42,
            action: AuditAction::Ddl,
            result: AuditResult::Succeeded,
            target_ids: vec![[0x14; 16], [0x15; 16]],
        };
        audit.insert(event.event_id.as_slice(), event.encode()?.as_slice())?;
    }
    write.commit()?;
    Ok(database)
}

#[test]
fn jsonl_sink_delivers_durably_once_and_replays_after_sink_change()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new();
    let database = create_database(&fixture.0.join("database.redb"))?;
    let first_sink = fixture.0.join("first.jsonl");
    assert_eq!(
        prdb_engine::audit_sink::deliver_jsonl_batch(&database, &first_sink, 8)?,
        1
    );
    let first = std::fs::read_to_string(&first_sink)?;
    assert!(first.ends_with('\n'));
    assert!(first.contains("\"event_id\":\"11111111111111111111111111111111\""));
    assert!(first.contains("\"actor_id\":\"12121212121212121212121212121212\""));
    assert!(first.contains("\"occurred_at_us\":42"));
    assert_eq!(
        std::fs::metadata(&first_sink)?.permissions().mode() & 0o7777,
        0o600
    );
    assert_eq!(
        prdb_engine::audit_sink::deliver_jsonl_batch(&database, &first_sink, 8)?,
        0
    );
    assert_eq!(std::fs::read_to_string(&first_sink)?, first);

    std::fs::remove_file(&first_sink)?;
    assert_eq!(
        prdb_engine::audit_sink::deliver_jsonl_batch(&database, &first_sink, 8)?,
        1
    );
    assert_eq!(std::fs::read_to_string(&first_sink)?, first);

    let second_sink = fixture.0.join("second.jsonl");
    assert_eq!(
        prdb_engine::audit_sink::deliver_jsonl_batch(&database, &second_sink, 8)?,
        1
    );
    assert_eq!(std::fs::read_to_string(&second_sink)?, first);
    Ok(())
}

#[test]
fn jsonl_sink_failure_keeps_audit_event_pending() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new();
    let database = create_database(&fixture.0.join("database.redb"))?;
    let invalid_sink = fixture.0.join("missing").join("audit.jsonl");
    assert!(prdb_engine::audit_sink::deliver_jsonl_batch(&database, &invalid_sink, 8).is_err());
    assert_eq!(database.begin_read()?.open_table(AUDIT)?.len()?, 1);
    let valid_sink = fixture.0.join("recovered.jsonl");
    assert_eq!(
        prdb_engine::audit_sink::deliver_jsonl_batch(&database, &valid_sink, 8)?,
        1
    );
    Ok(())
}

#[test]
fn jsonl_sink_reads_only_the_requested_pending_batch() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new();
    let database = create_database(&fixture.0.join("database.redb"))?;
    {
        let write = database.begin_write()?;
        {
            let mut audit = write.open_table(AUDIT)?;
            for value in 1..=64u8 {
                let event = AuditEvent {
                    event_id: [value; 16],
                    actor_id: Some([0x12; 16]),
                    correlation_id: [0x13; 16],
                    occurred_at_us: i64::from(value),
                    action: AuditAction::Ddl,
                    result: AuditResult::Succeeded,
                    target_ids: vec![[0x14; 16]],
                };
                audit.insert(event.event_id.as_slice(), event.encode()?.as_slice())?;
            }
            audit.insert([0xff; 16].as_slice(), b"malformed".as_slice())?;
        }
        write.commit()?;
    }

    let sink = fixture.0.join("bounded.jsonl");
    assert_eq!(
        prdb_engine::audit_sink::deliver_jsonl_batch(&database, &sink, 64)?,
        64
    );
    assert_eq!(std::fs::read_to_string(&sink)?.lines().count(), 64);
    assert!(prdb_engine::audit_sink::deliver_jsonl_batch(&database, &sink, 64).is_err());
    Ok(())
}

#[test]
fn jsonl_sink_recovers_an_incomplete_final_line() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new();
    let database = create_database(&fixture.0.join("database.redb"))?;
    let sink = fixture.0.join("partial.jsonl");
    std::fs::write(&sink, b"{\"incomplete\"")?;
    std::fs::set_permissions(&sink, std::fs::Permissions::from_mode(0o600))?;

    assert_eq!(
        prdb_engine::audit_sink::deliver_jsonl_batch(&database, &sink, 8)?,
        1
    );
    let output = std::fs::read_to_string(sink)?;
    assert_eq!(output.lines().count(), 1);
    assert!(output.starts_with("{\"event_id\":"));
    assert!(output.ends_with('\n'));
    Ok(())
}

#[test]
fn jsonl_sink_recovers_a_partial_tail_after_a_large_file() -> Result<(), Box<dyn std::error::Error>>
{
    let fixture = Fixture::new();
    let database = create_database(&fixture.0.join("database.redb"))?;
    let sink = fixture.0.join("large-partial.jsonl");
    let mut contents = b"{\"prior\":true}\n".repeat(2_000);
    let complete_length = contents.len();
    contents.extend_from_slice(b"{\"incomplete\"");
    std::fs::write(&sink, &contents)?;
    std::fs::set_permissions(&sink, std::fs::Permissions::from_mode(0o600))?;

    assert_eq!(
        prdb_engine::audit_sink::deliver_jsonl_batch(&database, &sink, 8)?,
        1
    );
    let output = std::fs::read(&sink)?;
    assert!(output.len() > complete_length);
    assert_eq!(&output[..complete_length], &contents[..complete_length]);
    assert!(output[complete_length..].starts_with(b"{\"event_id\":"));
    assert!(output.ends_with(b"\n"));
    assert_eq!(output.iter().filter(|byte| **byte == b'\n').count(), 2_001);
    Ok(())
}

#[test]
fn jsonl_sink_rejects_non_private_existing_files() -> Result<(), Box<dyn std::error::Error>> {
    let fixture = Fixture::new();
    let database = create_database(&fixture.0.join("database.redb"))?;
    let sink = fixture.0.join("public.jsonl");
    std::fs::write(&sink, b"")?;
    std::fs::set_permissions(&sink, std::fs::Permissions::from_mode(0o644))?;

    assert!(prdb_engine::audit_sink::deliver_jsonl_batch(&database, &sink, 8).is_err());
    assert!(
        prdb_engine::audit_sink::deliver_jsonl_batch(
            &database,
            std::path::Path::new("audit.jsonl"),
            8
        )
        .is_err()
    );
    Ok(())
}
