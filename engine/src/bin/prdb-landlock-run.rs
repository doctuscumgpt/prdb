#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
fn main() {
    eprintln!("prdb-landlock-run currently supports Linux x86_64 only");
    std::process::exit(1);
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn main() {
    if let Err(error) = run() {
        eprintln!("prdb-landlock-run: {error}");
        std::process::exit(126);
    }
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn run() -> Result<(), Box<dyn std::error::Error>> {
    use prdb_engine::linux_sandbox::{LandlockPolicy, LandlockRuleset};
    use std::os::unix::process::CommandExt;
    use std::path::PathBuf;

    let mut policy = LandlockPolicy::default();
    let mut args = std::env::args_os().skip(1);
    let mut command = None::<Vec<std::ffi::OsString>>;
    while let Some(argument) = args.next() {
        let flag = argument.to_string_lossy();
        if flag == "--" {
            command = Some(args.collect());
            break;
        }
        let value = args
            .next()
            .ok_or_else(|| format!("missing value for {flag}"))?;
        let value_text = value
            .to_str()
            .ok_or_else(|| format!("non-UTF-8 value for {flag}"))?;
        match flag.as_ref() {
            "--read" => {
                policy.allow_read(PathBuf::from(value))?;
            }
            "--traverse" => {
                policy.allow_path_traversal(PathBuf::from(value))?;
            }
            "--read-execute" => {
                policy.allow_read_execute(PathBuf::from(value))?;
            }
            "--read-write-file" => {
                policy.allow_read_write_file(PathBuf::from(value))?;
            }
            "--writable" => {
                policy.allow_writable(PathBuf::from(value))?;
            }
            "--read-env" => {
                for path in env_paths(value_text)? {
                    policy.allow_read(path)?;
                }
            }
            "--traverse-env" => {
                for path in env_paths(value_text)? {
                    policy.allow_path_traversal(path)?;
                }
            }
            "--read-execute-env" => {
                for path in env_paths(value_text)? {
                    policy.allow_read_execute(path)?;
                }
            }
            "--writable-env" => {
                let path = std::env::var_os(value_text)
                    .ok_or_else(|| format!("environment variable {value_text} is unset"))?;
                policy.allow_writable(PathBuf::from(path))?;
            }
            unknown => return Err(format!("unknown option {unknown}").into()),
        }
    }
    let mut command = command.ok_or("expected -- PROGRAM [ARG ...]")?.into_iter();
    let program = command.next().ok_or("missing program after --")?;

    let ruleset = LandlockRuleset::new(&policy)
        .map_err(|error| format!("prepare Landlock policy: {error}"))?;
    ruleset
        .restrict_current_thread()
        .map_err(|error| format!("enforce Landlock policy: {error}"))?;
    let error = std::process::Command::new(program).args(command).exec();
    Err(format!("exec service: {error}").into())
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn env_paths(name: &str) -> Result<Vec<std::path::PathBuf>, Box<dyn std::error::Error>> {
    let value = std::env::var(name).map_err(|_| format!("environment variable {name} is unset"))?;
    let paths = value
        .split_whitespace()
        .map(std::path::PathBuf::from)
        .collect::<Vec<_>>();
    if paths.is_empty() {
        return Err(format!("environment variable {name} has no paths").into());
    }
    Ok(paths)
}
