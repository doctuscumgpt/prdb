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
         Stop gateway and supervisor services before compact or audit-archive."
    );
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn run() -> Result<(), Box<dyn std::error::Error>> {
    use prdb_engine::bootstrap::StorageInstance;
    use std::fs::OpenOptions;
    use std::io::Read;
    use std::os::unix::fs::MetadataExt;
    use std::os::unix::fs::OpenOptionsExt;
    use std::path::PathBuf;
    use std::time::Instant;

    let mut args = std::env::args_os().skip(1);
    let operation = args
        .next()
        .ok_or("operation is required: compact, audit-archive, or audit-verify")?;
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

    if args.next().as_deref() != Some(std::ffi::OsStr::new("--instance-dir")) {
        return Err("--instance-dir PATH is required".into());
    }
    let instance_dir = PathBuf::from(args.next().ok_or("instance directory is required")?);
    let archive_output = if operation == "compact" {
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

    let mut instance = StorageInstance::open(instance_dir)?;
    if let Some(output) = archive_output {
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
