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
         Stop the gateway and supervisor services before running maintenance."
    );
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn run() -> Result<(), Box<dyn std::error::Error>> {
    use prdb_engine::bootstrap::StorageInstance;
    use std::path::PathBuf;
    use std::time::Instant;

    let mut args = std::env::args_os().skip(1);
    let operation = args
        .next()
        .ok_or("operation is required: compact or audit-archive")?;
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
