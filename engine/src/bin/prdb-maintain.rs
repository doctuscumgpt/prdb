#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
fn main() {
    eprintln!("prdb-maintain currently supports Linux x86_64 only");
    std::process::exit(1);
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn main() {
    if std::env::args().any(|argument| argument == "--help" || argument == "-h") {
        print_help();
        return;
    }
    if let Err(error) = run() {
        eprintln!("prdb-maintain: {error}");
        std::process::exit(1);
    }
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn print_help() {
    println!(
        "prdb-maintain compact --instance-dir PATH\n\
         Runs one offline redb compaction attempt while holding the instance lock.\n\
         prdb-maintain audit-archive --instance-dir PATH --output PATH\n\
         Writes a durable checksummed audit archive before pruning its events.\n\
         prdb-maintain audit-verify --archive PATH\n\
         Verifies one private PRA1 archive without opening a database.\n\
         prdb-maintain backup --instance-dir PATH --output-dir PATH\n\
         Copies and validates a READY instance while holding its exclusive lock.\n\
         prdb-maintain backup-verify --backup-dir PATH\n\
         Verifies the backup manifest, file digests, and READY catalog.\n\
         prdb-maintain inspect --instance-dir PATH\n\
         Read-only READY validation and bounded engine metadata diagnostics.\n\
         prdb-maintain migration-inspect --instance-dir PATH\n\
         Validates the active READY image and any private staged image.\n\
         prdb-maintain migration-discard-stage --instance-dir PATH --backup-dir PATH\n\
         Discards staging only when active bytes match the verified backup.\n\
         Stop gateway and supervisor services before compact, audit-archive, backup, or migration-discard-stage."
    );
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn run() -> Result<(), Box<dyn std::error::Error>> {
    use prdb_engine::bootstrap::{ReadOnlyStorageInstance, StorageInstance};
    use std::fs::OpenOptions;
    use std::io::Read;
    use std::os::unix::fs::MetadataExt;
    use std::os::unix::fs::OpenOptionsExt;
    use std::path::PathBuf;
    use std::time::Instant;

    let mut args = std::env::args_os().skip(1);
    let operation = args.next().ok_or(
        "operation is required: compact, audit-archive, audit-verify, backup, backup-verify, inspect, migration-inspect, or migration-discard-stage",
    )?;
    if operation == "audit-verify" {
        if args.next().as_deref() != Some(std::ffi::OsStr::new("--archive")) {
            return Err("--archive PATH is required".into());
        }
        let archive_path = PathBuf::from(args.next().ok_or("archive path is required")?);
        if args.next().is_some() {
            return Err("unexpected trailing arguments".into());
        }
        let mut archive = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW)
            .open(archive_path)?;
        let metadata = archive.metadata()?;
        if !metadata.file_type().is_file()
            || metadata.mode() & 0o7777 != 0o600
            || metadata.len() > prdb_engine::audit_archive::MAX_ARCHIVE_LENGTH as u64
        {
            return Err("archive must be a bounded regular file with mode 0600".into());
        }
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        archive
            .by_ref()
            .take(prdb_engine::audit_archive::MAX_ARCHIVE_LENGTH as u64 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > prdb_engine::audit_archive::MAX_ARCHIVE_LENGTH {
            return Err("audit archive exceeds the format size limit".into());
        }
        let records = prdb_engine::audit_archive::decode(&bytes)?;
        println!("verified events={}", records.len());
        return Ok(());
    }
    if operation == "backup-verify" {
        if args.next().as_deref() != Some(std::ffi::OsStr::new("--backup-dir")) {
            return Err("--backup-dir PATH is required".into());
        }
        let backup_dir = PathBuf::from(args.next().ok_or("backup directory is required")?);
        if args.next().is_some() {
            return Err("unexpected trailing arguments".into());
        }
        let verified = prdb_engine::offline_backup::verify_backup(&backup_dir)?;
        println!(
            "verified database_bytes={} sha256={}",
            verified.database_bytes,
            verified
                .database_sha256
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        );
        return Ok(());
    }
    if operation == "inspect" {
        if args.next().as_deref() != Some(std::ffi::OsStr::new("--instance-dir")) {
            return Err("--instance-dir PATH is required".into());
        }
        let instance_dir = PathBuf::from(args.next().ok_or("instance directory is required")?);
        if args.next().is_some() {
            return Err("unexpected trailing arguments".into());
        }
        let instance = ReadOnlyStorageInstance::open(&instance_dir)?;
        let report =
            prdb_engine::startup::validate_persisted_records_read_only(instance.database())?;
        let metadata = report.metadata;
        println!(
            "status=READY database_id={} catalog_version={} expression_version={} builtin_semantics_version={} commit_sequence={} catalog_records={} rows={} index_entries={} outcomes={} audit_events={}",
            metadata
                .database_id
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>(),
            metadata.catalog_version,
            metadata.expression_version,
            metadata.builtin_semantics_version,
            metadata.commit_sequence,
            report.catalog_records,
            report.rows,
            report.index_entries,
            report.outcomes,
            report.audit_events,
        );
        return Ok(());
    }

    if operation == "migration-inspect" {
        use std::os::unix::fs::MetadataExt;

        if args.next().as_deref() != Some(std::ffi::OsStr::new("--instance-dir")) {
            return Err("--instance-dir PATH is required".into());
        }
        let instance_dir = PathBuf::from(args.next().ok_or("instance directory is required")?);
        if args.next().is_some() {
            return Err("unexpected trailing arguments".into());
        }

        let active = ReadOnlyStorageInstance::open_for_migration_inspection(&instance_dir)?;
        let active_report =
            prdb_engine::startup::validate_migration_image_read_only(active.database())?;
        let active_meta = active_report.metadata;
        println!(
            "active=READY database_id={} catalog_version={} expression_version={} builtin_semantics_version={} commit_sequence={}",
            active_meta
                .database_id
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>(),
            active_meta.catalog_version,
            active_meta.expression_version,
            active_meta.builtin_semantics_version,
            active_meta.commit_sequence,
        );

        let staging_path = instance_dir.join(".prdb-migration.next");
        if !staging_path.try_exists()? {
            println!("staging=absent");
            return Ok(());
        }
        let staging_metadata = std::fs::symlink_metadata(&staging_path)?;
        if !staging_metadata.file_type().is_file()
            || staging_metadata.nlink() != 1
            || staging_metadata.mode() & 0o7777 != 0o600
            || staging_metadata.uid() != unsafe { libc::geteuid() }
        {
            return Err("staging image must be an owner-owned mode-0600 regular file".into());
        }
        let staged_database = redb::ReadOnlyDatabase::open(&staging_path)?;
        let staged_report =
            prdb_engine::startup::validate_migration_image_read_only(&staged_database)?;
        let staged_meta = staged_report.metadata;
        let relationship = if active_meta.database_id != staged_meta.database_id {
            "database_id_mismatch"
        } else if active_meta.commit_sequence == staged_meta.commit_sequence {
            "same_frontier"
        } else if active_meta.commit_sequence > staged_meta.commit_sequence {
            "active_ahead"
        } else {
            "staging_ahead"
        };
        println!(
            "staging=READY relationship={} database_id={} catalog_version={} expression_version={} builtin_semantics_version={} commit_sequence={}",
            relationship,
            staged_meta
                .database_id
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>(),
            staged_meta.catalog_version,
            staged_meta.expression_version,
            staged_meta.builtin_semantics_version,
            staged_meta.commit_sequence,
        );
        return Ok(());
    }

    if operation == "migration-discard-stage" {
        if args.next().as_deref() != Some(std::ffi::OsStr::new("--instance-dir")) {
            return Err("--instance-dir PATH is required".into());
        }
        let instance_dir = PathBuf::from(args.next().ok_or("instance directory is required")?);
        if args.next().as_deref() != Some(std::ffi::OsStr::new("--backup-dir")) {
            return Err("--backup-dir PATH is required".into());
        }
        let backup_dir = PathBuf::from(args.next().ok_or("backup directory is required")?);
        if args.next().is_some() {
            return Err("unexpected trailing arguments".into());
        }
        StorageInstance::discard_unpublished_staging(&instance_dir, &backup_dir)?;
        println!("staging=discarded active=verified_source");
        return Ok(());
    }

    if args.next().as_deref() != Some(std::ffi::OsStr::new("--instance-dir")) {
        return Err("--instance-dir PATH is required".into());
    }
    let instance_dir = PathBuf::from(args.next().ok_or("instance directory is required")?);
    let operation_argument = if operation == "backup" {
        if args.next().as_deref() != Some(std::ffi::OsStr::new("--output-dir")) {
            return Err("--output-dir PATH is required".into());
        }
        let output = PathBuf::from(args.next().ok_or("backup output directory is required")?);
        if args.next().is_some() {
            return Err("unexpected trailing arguments".into());
        }
        Some(output)
    } else if operation == "compact" {
        if args.next().is_some() {
            return Err("unexpected trailing arguments".into());
        }
        None
    } else if operation == "audit-archive" {
        if args.next().as_deref() != Some(std::ffi::OsStr::new("--output")) {
            return Err("--output PATH is required".into());
        }
        let output = PathBuf::from(args.next().ok_or("archive output path is required")?);
        if args.next().is_some() {
            return Err("unexpected trailing arguments".into());
        }
        Some(output)
    } else {
        return Err(format!("unsupported operation: {}", operation.to_string_lossy()).into());
    };

    if operation == "backup" {
        let output = operation_argument.expect("backup requires an output directory");
        let verified = prdb_engine::offline_backup::create_verified_backup(&instance_dir, &output)?;
        println!(
            "backup verified database_bytes={} sha256={}",
            verified.database_bytes,
            verified
                .database_sha256
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        );
        return Ok(());
    }

    let mut instance = StorageInstance::open(instance_dir)?;
    if operation == "audit-archive" {
        let output = operation_argument.expect("audit archive requires an output path");
        let events = instance.archive_audit_events(output)?;
        println!("archived events={events}");
    } else {
        let started = Instant::now();
        let performed = instance.compact()?;
        println!(
            "compaction performed={performed} elapsed_ms={:.3}",
            started.elapsed().as_secs_f64() * 1_000.0
        );
    }
    Ok(())
}
