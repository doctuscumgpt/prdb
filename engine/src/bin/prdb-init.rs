#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
fn main() {
    eprintln!("prdb-init currently supports Linux x86_64 only");
    std::process::exit(1);
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn main() {
    if std::env::args().any(|argument| argument == "--help" || argument == "-h") {
        print_help();
        return;
    }
    if let Err(error) = run() {
        eprintln!("prdb-init: {error}");
        std::process::exit(1);
    }
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn print_help() {
    println!(
        "prdb-init --instance-dir PATH --administrator NAME\n\
         --credential-file PATH --expires-in-seconds N [--resume]\n\
         Creates a private database instance and completes the local one-time credential handoff.\n\
         The credential file must not already exist. --resume rotates an unacknowledged token."
    );
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn run() -> Result<(), Box<dyn std::error::Error>> {
    use prdb_engine::bootstrap::{StorageInstance, initialize, receive_bootstrap_handoff, resume};
    use prdb_engine::client_credentials::write_client_credentials;
    use prdb_engine::client_protocol::ClientCredentials;
    use std::io;
    use std::os::unix::net::UnixStream;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};
    use zeroize::Zeroize;

    #[derive(Default)]
    struct Options {
        instance_dir: Option<PathBuf>,
        administrator: Option<String>,
        credential_file: Option<PathBuf>,
        expires_in_seconds: Option<u64>,
        resume: bool,
    }

    let mut options = Options::default();
    let mut args = std::env::args_os().skip(1);
    while let Some(flag) = args.next() {
        let flag_text = flag.to_string_lossy();
        if flag_text == "--resume" {
            if options.resume {
                return Err("--resume may be specified only once".into());
            }
            options.resume = true;
            continue;
        }
        let value = args
            .next()
            .ok_or_else(|| format!("missing value for {flag_text}"))?;
        match flag_text.as_ref() {
            "--instance-dir" => {
                if options.instance_dir.replace(PathBuf::from(value)).is_some() {
                    return Err("--instance-dir may be specified only once".into());
                }
            }
            "--administrator" => {
                let name = value
                    .into_string()
                    .map_err(|_| "--administrator must be UTF-8")?;
                if options.administrator.replace(name).is_some() {
                    return Err("--administrator may be specified only once".into());
                }
            }
            "--credential-file" => {
                if options
                    .credential_file
                    .replace(PathBuf::from(value))
                    .is_some()
                {
                    return Err("--credential-file may be specified only once".into());
                }
            }
            "--expires-in-seconds" => {
                let seconds = value
                    .into_string()
                    .map_err(|_| "--expires-in-seconds must be UTF-8")?
                    .parse::<u64>()?;
                if options.expires_in_seconds.replace(seconds).is_some() {
                    return Err("--expires-in-seconds may be specified only once".into());
                }
            }
            _ => return Err(format!("unknown option {flag_text}").into()),
        }
    }

    let instance_dir = options.instance_dir.ok_or("--instance-dir is required")?;
    let administrator = options.administrator.ok_or("--administrator is required")?;
    let credential_file = options
        .credential_file
        .ok_or("--credential-file is required")?;
    let expires_in_seconds = options
        .expires_in_seconds
        .filter(|seconds| *seconds > 0)
        .ok_or("--expires-in-seconds must be positive")?;
    let credential_parent = credential_file
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(std::path::Path::new("."));
    if !credential_parent.is_dir() {
        return Err("credential-file parent must be an existing directory".into());
    }
    match std::fs::symlink_metadata(&credential_file) {
        Ok(_) => return Err("credential file already exists; refusing to overwrite".into()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let now_us = SystemTime::now().duration_since(UNIX_EPOCH)?.as_micros();
    let expires_at_us = now_us
        .checked_add(u128::from(expires_in_seconds) * 1_000_000)
        .and_then(|value| i64::try_from(value).ok())
        .ok_or("credential expiry is out of range")?;

    let session = if options.resume {
        resume(&instance_dir, &administrator, expires_at_us)?
    } else {
        initialize(&instance_dir, &administrator, expires_at_us)?
    };
    let listener = session.bind_setup_listener()?;
    let socket_path = listener.socket_path().to_path_buf();
    let mut stream = UnixStream::connect(&socket_path)?;
    let server = std::thread::Builder::new()
        .name("prdb-bootstrap-listener".into())
        .spawn(move || listener.accept_and_acknowledge(session))?;

    let handoff_result = (|| -> Result<(), Box<dyn std::error::Error>> {
        let mut handoff = receive_bootstrap_handoff(&mut stream)?;
        let mut secret = handoff
            .take_secret()
            .ok_or("bootstrap handoff did not contain a credential")?;
        let credentials = ClientCredentials {
            token_id: handoff.token_id(),
            token_secret: secret,
        };
        secret.zeroize();
        write_client_credentials(&credential_file, &credentials)?;
        drop(credentials);
        handoff.acknowledge_delivery(&mut stream)?;
        Ok(())
    })();

    let server_result: Result<StorageInstance, _> = server
        .join()
        .map_err(|_| "bootstrap listener thread panicked")?;
    let instance = match server_result {
        Ok(instance) => instance,
        Err(error) => {
            handoff_result?;
            return Err(error.into());
        }
    };
    drop(instance);
    println!("database is READY; administrator credential saved to the mode-0600 file");
    Ok(())
}
