#![cfg(all(unix, target_os = "linux", target_arch = "x86_64"))]

use prdb_engine::audit::{AuditAction, AuditEvent, AuditResult};
use prdb_engine::bootstrap::initialize;
use prdb_engine::client_credentials::{
    read_client_credentials, remove_client_credentials, write_client_credentials,
};
use prdb_engine::client_protocol::{ClientCredentials, ClientFrame, ClientFrameDecoder};
use prdb_engine::client_submit::{
    AdminClient, AdminColumnDefinition, AdminCreatePolicyRequest, AdminObjectGrantTarget,
    AdminObjectPrivilege, AdminPolicyEffect, AdminPolicyKind, AdminPolicyOperation,
    AdminPolicyRecipient, AdminSystemPrivilege, CancellationResult, InvocationStatus,
    MetadataClient, SubmissionClient, TerminalOutcome,
};
use prdb_engine::storage_manager::{
    CreateTableRequest, ObjectGrantPrivilege, ObjectGrantTarget, SetObjectGrantRequest,
    TableColumnDefinition, TokenIssueRequest,
};
use prdb_engine::submission_manifest::{encode_manifest_v1, manifest_v1_from_elf};
use prdb_engine::supervisor::WorkloadClass;
use prdb_engine::tuple::ScalarType;
use sha2::{Digest, Sha256};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

struct ChildGuard(Option<Child>);

fn read_client_frame(stream: &mut std::net::TcpStream) -> std::io::Result<Option<ClientFrame>> {
    let mut decoder = ClientFrameDecoder::default();
    let mut input = [0u8; 8192];
    loop {
        let bytes = stream.read(&mut input)?;
        if bytes == 0 {
            return Ok(None);
        }
        let mut frames = decoder
            .push(&input[..bytes])
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
        if !frames.is_empty() {
            return Ok(Some(frames.remove(0)));
        }
    }
}

fn start_response_dropping_proxy(
    gateway_address: std::net::SocketAddr,
) -> std::io::Result<(
    std::net::SocketAddr,
    std::thread::JoinHandle<std::io::Result<usize>>,
)> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let proxy_address = listener.local_addr()?;
    let handle =
        std::thread::spawn(move || {
            let (mut client, _) = listener.accept()?;
            let mut gateway = std::net::TcpStream::connect(gateway_address)?;
            client.set_read_timeout(Some(Duration::from_secs(10)))?;
            client.set_write_timeout(Some(Duration::from_secs(10)))?;
            gateway.set_read_timeout(Some(Duration::from_secs(10)))?;
            gateway.set_write_timeout(Some(Duration::from_secs(10)))?;
            let mut starts = 0;
            while let Some(request) = read_client_frame(&mut client)? {
                if request.kind == 7 {
                    starts += 1;
                }
                gateway.write_all(&request.encode().map_err(|error| {
                    std::io::Error::new(std::io::ErrorKind::InvalidData, error)
                })?)?;
                let Some(response) = read_client_frame(&mut gateway)? else {
                    break;
                };
                if request.kind == 7 && response.kind == 100 {
                    // The gateway accepted START, but the client loses its reply.
                    return Ok(starts);
                }
                client.write_all(&response.encode().map_err(|error| {
                    std::io::Error::new(std::io::ErrorKind::InvalidData, error)
                })?)?;
            }
            Ok(starts)
        });
    Ok((proxy_address, handle))
}

impl ChildGuard {
    fn child(&mut self) -> &mut Child {
        self.0.as_mut().expect("child process still running")
    }

    fn stop(&mut self) -> std::io::Result<std::process::ExitStatus> {
        let mut child = self.0.take().expect("child process still running");
        // SAFETY: the PID belongs to the child process held by this guard.
        unsafe {
            libc::kill(child.id() as libc::pid_t, libc::SIGTERM);
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = child.try_wait()? {
                return Ok(status);
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                return child.wait();
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn kill(&mut self) -> std::io::Result<std::process::ExitStatus> {
        let mut child = self.0.take().expect("child process still running");
        let _ = child.kill();
        child.wait()
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[test]
fn authenticated_metadata_and_admin_lifecycle_cross_gateway_and_supervisor_processes()
-> Result<(), Box<dyn std::error::Error>> {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let root = std::env::temp_dir().join(format!(
        "prdb-metadata-process-{}-{nonce}",
        std::process::id()
    ));
    std::fs::create_dir(&root)?;
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))?;
    let _cleanup = DirectoryGuard(root.clone());

    let instance_dir = root.join("instance");
    let mut setup = initialize(&instance_dir, "metadata_process_admin", i64::MAX)?;
    let token_id = setup.token_id();
    let admin_user_id = setup.admin_user_id();
    let mut token_secret = setup.take_secret().ok_or("bootstrap secret missing")?;
    let committed_invocation_id = [0xa3; 16];
    let restricted_token;
    let mut restricted_secret;
    let scope_reader_token;
    let scope_reader_secret;
    let owner_token;
    let mut owner_secret;
    let restricted_schema_id;
    let restricted_table_id;
    let restricted_index_id;
    let sql_visible_schema_id;
    {
        let instance = setup.acknowledge()?;
        let storage = instance.storage_manager(Default::default())?;
        let now_us = i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_micros())?;
        let retention_deadline_us = now_us.saturating_add(3_600_000_000);

        let restricted_user = storage.create_user(
            [0xa6; 16],
            admin_user_id,
            "metadata_reader",
            retention_deadline_us,
            AuditEvent {
                event_id: [0xa7; 16],
                actor_id: Some(admin_user_id),
                correlation_id: [0xa6; 16],
                occurred_at_us: now_us,
                action: AuditAction::Ddl,
                result: AuditResult::Succeeded,
                target_ids: Vec::new(),
            },
        )?;
        let owner_user = storage.create_user(
            [0xac; 16],
            admin_user_id,
            "schema_owner",
            retention_deadline_us,
            AuditEvent {
                event_id: [0xad; 16],
                actor_id: Some(admin_user_id),
                correlation_id: [0xac; 16],
                occurred_at_us: now_us,
                action: AuditAction::Ddl,
                result: AuditResult::Succeeded,
                target_ids: Vec::new(),
            },
        )?;
        let scope_reader = storage.create_user(
            [0xc0; 16],
            admin_user_id,
            "sql_scope_reader",
            retention_deadline_us,
            AuditEvent {
                event_id: [0xc1; 16],
                actor_id: Some(admin_user_id),
                correlation_id: [0xc0; 16],
                occurred_at_us: now_us,
                action: AuditAction::Ddl,
                result: AuditResult::Succeeded,
                target_ids: Vec::new(),
            },
        )?;
        let restricted_schema = storage.create_schema(
            [0xa4; 16],
            admin_user_id,
            "admin_private",
            owner_user.user_id,
            retention_deadline_us,
            AuditEvent {
                event_id: [0xa5; 16],
                actor_id: Some(admin_user_id),
                correlation_id: [0xa4; 16],
                occurred_at_us: now_us,
                action: AuditAction::Ddl,
                result: AuditResult::Succeeded,
                target_ids: Vec::new(),
            },
        )?;
        restricted_schema_id = restricted_schema.schema_id;
        let sql_visible_schema = storage.create_schema(
            [0xb4; 16],
            admin_user_id,
            "empty_roundtrip",
            restricted_user.user_id,
            retention_deadline_us,
            AuditEvent {
                event_id: [0xb5; 16],
                actor_id: Some(admin_user_id),
                correlation_id: [0xb4; 16],
                occurred_at_us: now_us,
                action: AuditAction::Ddl,
                result: AuditResult::Succeeded,
                target_ids: Vec::new(),
            },
        )?;
        sql_visible_schema_id = sql_visible_schema.schema_id;
        storage.set_object_grant(SetObjectGrantRequest {
            invocation_id: [0xba; 16],
            authenticated_actor_id: admin_user_id,
            grantee_id: restricted_user.user_id,
            object_id: sql_visible_schema_id,
            privilege: ObjectGrantPrivilege::Usage,
            target: ObjectGrantTarget::Object,
            grant: true,
            retention_deadline_us,
            audit_event: AuditEvent {
                event_id: [0xbb; 16],
                actor_id: Some(admin_user_id),
                correlation_id: [0xba; 16],
                occurred_at_us: now_us,
                action: AuditAction::AuthorityChange,
                result: AuditResult::Succeeded,
                target_ids: vec![restricted_user.user_id, sql_visible_schema_id],
            },
        })?;
        let sql_hidden_schema = storage.create_schema(
            [0xb6; 16],
            admin_user_id,
            "sql_roundtrip",
            admin_user_id,
            retention_deadline_us,
            AuditEvent {
                event_id: [0xb7; 16],
                actor_id: Some(admin_user_id),
                correlation_id: [0xb6; 16],
                occurred_at_us: now_us,
                action: AuditAction::Ddl,
                result: AuditResult::Succeeded,
                target_ids: Vec::new(),
            },
        )?;
        storage.create_table(CreateTableRequest {
            invocation_id: [0xb8; 16],
            authenticated_actor_id: admin_user_id,
            schema_id: sql_hidden_schema.schema_id,
            name: "records".to_owned(),
            columns: vec![TableColumnDefinition {
                name: "id".to_owned(),
                scalar_type: ScalarType::Integer,
                nullable: true,
                generated_uuid_identity: false,
                default: None,
            }],
            retention_deadline_us,
            audit_event: AuditEvent {
                event_id: [0xb9; 16],
                actor_id: Some(admin_user_id),
                correlation_id: [0xb8; 16],
                occurred_at_us: now_us,
                action: AuditAction::Ddl,
                result: AuditResult::Succeeded,
                target_ids: Vec::new(),
            },
        })?;
        let private_table = storage.create_table(CreateTableRequest {
            invocation_id: [0xaa; 16],
            authenticated_actor_id: admin_user_id,
            schema_id: restricted_schema_id,
            name: "private_records".to_owned(),
            columns: vec![TableColumnDefinition {
                name: "payload".to_owned(),
                scalar_type: ScalarType::Text,
                nullable: false,
                generated_uuid_identity: false,
                default: None,
            }],
            retention_deadline_us,
            audit_event: AuditEvent {
                event_id: [0xab; 16],
                actor_id: Some(admin_user_id),
                correlation_id: [0xaa; 16],
                occurred_at_us: now_us,
                action: AuditAction::Ddl,
                result: AuditResult::Succeeded,
                target_ids: Vec::new(),
            },
        })?;
        restricted_table_id = private_table.table_id;
        for (invocation_id, event_id, object_id, privilege) in [
            (
                [0xc2; 16],
                [0xc3; 16],
                restricted_schema_id,
                ObjectGrantPrivilege::Usage,
            ),
            (
                [0xc4; 16],
                [0xc5; 16],
                private_table.table_id,
                ObjectGrantPrivilege::Select,
            ),
        ] {
            storage.set_object_grant(SetObjectGrantRequest {
                invocation_id,
                authenticated_actor_id: admin_user_id,
                grantee_id: scope_reader.user_id,
                object_id,
                privilege,
                target: ObjectGrantTarget::Object,
                grant: true,
                retention_deadline_us,
                audit_event: AuditEvent {
                    event_id,
                    actor_id: Some(admin_user_id),
                    correlation_id: invocation_id,
                    occurred_at_us: now_us,
                    action: AuditAction::AuthorityChange,
                    result: AuditResult::Succeeded,
                    target_ids: vec![scope_reader.user_id, object_id],
                },
            })?;
        }
        let index_invocation = [0x8a; 16];
        let mut index_audit = AuditEvent {
            event_id: [0x8b; 16],
            actor_id: Some(admin_user_id),
            correlation_id: index_invocation,
            occurred_at_us: now_us,
            action: AuditAction::Ddl,
            result: AuditResult::Succeeded,
            target_ids: Vec::new(),
        };
        index_audit.target_ids = vec![private_table.table_id];
        restricted_index_id = storage
            .create_index(prdb_engine::storage_manager::CreateIndexRequest {
                invocation_id: index_invocation,
                authenticated_actor_id: admin_user_id,
                table_id: private_table.table_id,
                name: "private_payload_key".to_owned(),
                column_ids: vec![private_table.column_ids[0]],
                unique: false,
                retention_deadline_us,
                audit_event: index_audit,
            })?
            .index_id;
        let mut issued = storage.issue_token(TokenIssueRequest {
            invocation_id: [0xa8; 16],
            authenticated_actor_id: admin_user_id,
            user_id: restricted_user.user_id,
            label: "metadata reader",
            expires_at_us: now_us.saturating_add(1_800_000_000),
            retention_deadline_us,
            audit_event: AuditEvent {
                event_id: [0xa9; 16],
                actor_id: Some(admin_user_id),
                correlation_id: [0xa8; 16],
                occurred_at_us: now_us,
                action: AuditAction::TokenLifecycle,
                result: AuditResult::Succeeded,
                target_ids: vec![restricted_user.user_id],
            },
        })?;
        restricted_token = issued.token_id;
        restricted_secret = issued
            .take_secret()
            .ok_or("restricted metadata token secret missing")?;
        let mut owner_issued = storage.issue_token(TokenIssueRequest {
            invocation_id: [0xb0; 16],
            authenticated_actor_id: admin_user_id,
            user_id: owner_user.user_id,
            label: "schema owner",
            expires_at_us: now_us.saturating_add(1_800_000_000),
            retention_deadline_us,
            audit_event: AuditEvent {
                event_id: [0xb1; 16],
                actor_id: Some(admin_user_id),
                correlation_id: [0xb0; 16],
                occurred_at_us: now_us,
                action: AuditAction::TokenLifecycle,
                result: AuditResult::Succeeded,
                target_ids: vec![owner_user.user_id],
            },
        })?;
        owner_token = owner_issued.token_id;
        owner_secret = owner_issued
            .take_secret()
            .ok_or("schema owner token secret missing")?;
        let mut scope_reader_issued = storage.issue_token(TokenIssueRequest {
            invocation_id: [0xc6; 16],
            authenticated_actor_id: admin_user_id,
            user_id: scope_reader.user_id,
            label: "SQL scope reader",
            expires_at_us: now_us.saturating_add(1_800_000_000),
            retention_deadline_us,
            audit_event: AuditEvent {
                event_id: [0xc7; 16],
                actor_id: Some(admin_user_id),
                correlation_id: [0xc6; 16],
                occurred_at_us: now_us,
                action: AuditAction::TokenLifecycle,
                result: AuditResult::Succeeded,
                target_ids: vec![scope_reader.user_id],
            },
        })?;
        scope_reader_token = scope_reader_issued.token_id;
        scope_reader_secret = scope_reader_issued
            .take_secret()
            .ok_or("SQL scope reader token secret missing")?;

        let (mut admin_query, _) = storage.begin_token_authenticated_invocation(
            [0xb2; 16],
            token_id,
            &token_secret,
            retention_deadline_us,
            8,
            4096,
        )?;
        assert!(
            admin_query
                .list_catalog_authorized_schemas(128, None)?
                .schemas
                .iter()
                .all(|schema| schema.schema_id != restricted_schema_id)
        );
        admin_query.abort()?;

        let (mut owner_query, _) = storage.begin_token_authenticated_invocation(
            [0xb3; 16],
            owner_token,
            &owner_secret,
            retention_deadline_us,
            8,
            4096,
        )?;
        assert!(
            owner_query
                .list_catalog_authorized_schemas(128, None)?
                .schemas
                .iter()
                .all(|schema| schema.schema_id != restricted_schema_id)
        );
        owner_query.abort()?;

        let mut invocation = storage.admit_supervisor_invocation(
            committed_invocation_id,
            token_id,
            &token_secret,
            retention_deadline_us,
            8,
            4096,
        )?;
        let mut successful_worker = Command::new("true").spawn()?;
        invocation.observe_worker_exit(&mut successful_worker)?;

        let (mut terminal_writer, terminal_reader) = std::os::unix::net::UnixStream::pair()?;
        drop(terminal_reader);
        assert!(
            prdb_engine::supervisor_ipc::finalize_gateway_invocation_and_send_terminal(
                &mut invocation,
                &mut terminal_writer,
                1,
                prdb_engine::supervisor_ipc::GatewayTerminalDecision::Commit,
            )
            .is_err(),
            "closed terminal receiver must lose delivery after storage commits"
        );
        assert!(matches!(
            storage
                .get_invocation_status(
                    committed_invocation_id,
                    invocation.principal_id(),
                    now_us,
                )?
                .map(|snapshot| snapshot.status),
            Some(prdb_engine::storage_manager::InvocationStatus::Terminal(outcome))
                if outcome.status == prdb_engine::outcome::OutcomeStatus::Committed
        ));
    }
    let mut outcome_status_secret = token_secret;

    let network_sql_package = if let Some(artifact_path) =
        std::env::var_os("PRDB_TEST_RESTRICTED_METADATA_QUERY_ARTIFACT")
    {
        let artifact = std::fs::read(artifact_path)?;
        let manifest = manifest_v1_from_elf(
            &artifact,
            3,
            1,
            "x86_64-v1".to_owned(),
            "test-runtime".to_owned(),
            "TinyCC restricted metadata process fixture".to_owned(),
        )
        .map_err(|error| format!("derive network SQL fixture manifest: {error:?}"))?;
        let dependencies = manifest.dependencies.clone();
        let manifest_bytes = encode_manifest_v1(&manifest)
            .map_err(|error| format!("encode network SQL fixture manifest: {error:?}"))?;
        let sdk_release_dir = std::env::var_os("PRDB_TEST_INSTALLED_SDK_RELEASE_DIR")
            .map(std::path::PathBuf::from)
            .ok_or("PRDB_TEST_INSTALLED_SDK_RELEASE_DIR is required for SQL package tests")?;
        let sql_library = std::fs::read(sdk_release_dir.join("libprdb_sql.so.1"))?;
        let mut requested_scope = b"PSC1\0\x01\x01\x01".to_vec();
        requested_scope.extend_from_slice(&sql_visible_schema_id);
        let package = root.join("restricted-metadata-package");
        std::fs::create_dir(&package)?;
        std::fs::set_permissions(&package, std::fs::Permissions::from_mode(0o700))?;
        for (name, bytes) in [
            ("query.so", artifact.as_slice()),
            ("sql.so", sql_library.as_slice()),
            ("manifest.pmf1", manifest_bytes.as_slice()),
            ("requested_scope.psc1", requested_scope.as_slice()),
        ] {
            let path = package.join(name);
            std::fs::write(&path, bytes)?;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        }
        let artifact_hash = Sha256::digest(&artifact)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let hash_path = package.join("artifact.sha256");
        std::fs::write(&hash_path, format!("{artifact_hash}\n"))?;
        std::fs::set_permissions(&hash_path, std::fs::Permissions::from_mode(0o600))?;
        let sql_library_hash = Sha256::digest(&sql_library)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let hash_path = package.join("sql-library.sha256");
        std::fs::write(&hash_path, format!("{sql_library_hash}\n"))?;
        std::fs::set_permissions(&hash_path, std::fs::Permissions::from_mode(0o600))?;
        Some((package, dependencies))
    } else {
        None
    };

    let cgroup_parent = if network_sql_package.is_some() {
        std::env::var_os("PRDB_TEST_CGROUP_PARENT")
            .map(std::path::PathBuf::from)
            .ok_or("PRDB_TEST_CGROUP_PARENT is required for network SQL sandbox testing")?
    } else {
        root.clone()
    };
    let runtime_paths =
        |name: &str| -> Result<Vec<std::path::PathBuf>, Box<dyn std::error::Error>> {
            let value = std::env::var_os(name).ok_or(format!("{name} is required"))?;
            if name == "PRDB_TEST_RUNTIME_READ_ONLY" && value.is_empty() {
                return Ok(Vec::new());
            }
            let paths = std::env::split_paths(&value).collect::<Vec<_>>();
            if paths.is_empty() || paths.iter().any(|path| !path.is_absolute()) {
                return Err(format!("{name} must contain absolute paths").into());
            }
            Ok(paths)
        };
    let runtime_read_execute = if network_sql_package.is_some() {
        runtime_paths("PRDB_TEST_RUNTIME_READ_EXECUTE")?
    } else {
        vec![std::path::PathBuf::from("/usr")]
    };
    let runtime_read_only = if network_sql_package.is_some() {
        runtime_paths("PRDB_TEST_RUNTIME_READ_ONLY")?
    } else {
        vec![std::path::PathBuf::from("/etc/ld.so.cache")]
    };

    let socket_path = root.join("supervisor.sock");
    let uid = unsafe { libc::geteuid() };
    let mut supervisor_command = Command::new(env!("CARGO_BIN_EXE_prdb-supervisor"));
    supervisor_command.args([
        "--instance-dir",
        instance_dir.to_str().ok_or("non-UTF-8 instance path")?,
        "--service-socket",
        socket_path.to_str().ok_or("non-UTF-8 socket path")?,
        "--gateway-uid",
        &uid.to_string(),
        "--cgroup-parent",
        cgroup_parent.to_str().ok_or("non-UTF-8 cgroup path")?,
        "--executor",
        env!("CARGO_BIN_EXE_prdb-executor"),
        "--optimizer",
        env!("CARGO_BIN_EXE_prdb-optimizer"),
        "--scratch-parent",
        root.to_str().ok_or("non-UTF-8 scratch path")?,
        "--cpu-baseline",
        "x86_64-v1",
        "--runtime-profile",
        "test-runtime",
        "--sdk-release",
        "1",
        "--allow-legacy-static-sdk-release",
        "1",
        "--retention-seconds",
        "3600",
        "--max-rows",
        "8",
        "--max-bytes",
        "4096",
        "--cpu-quota-us",
        "100000",
        "--cpu-period-us",
        "100000",
        "--memory-max-bytes",
        "67108864",
        "--max-processes",
        "16",
    ]);
    for path in &runtime_read_execute {
        supervisor_command.arg("--runtime-read-execute").arg(path);
    }
    for path in &runtime_read_only {
        supervisor_command.arg("--runtime-read-only").arg(path);
    }
    if let Some((_, dependencies)) = &network_sql_package {
        for dependency in dependencies {
            supervisor_command
                .arg("--allowed-dependency")
                .arg(dependency);
        }
    }
    let mut supervisor = ChildGuard(Some(
        supervisor_command
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()?,
    ));

    let socket_deadline = Instant::now() + Duration::from_secs(5);
    while !socket_path.exists() {
        if let Some(status) = supervisor.child().try_wait()? {
            return Err(format!("supervisor exited before binding: {status}").into());
        }
        if Instant::now() >= socket_deadline {
            return Err("supervisor did not bind its service socket".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    let mut gateway = ChildGuard(Some(
        Command::new(env!("CARGO_BIN_EXE_prdb-gateway"))
            .args([
                "--bind",
                "127.0.0.1:0",
                "--service-socket",
                socket_path.to_str().ok_or("non-UTF-8 socket path")?,
                "--service-uid",
                &uid.to_string(),
                "--connections-per-ip",
                "256",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()?,
    ));
    let stderr = gateway
        .child()
        .stderr
        .take()
        .ok_or("gateway stderr missing")?;
    let (address_sender, address_receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let result = BufReader::new(stderr).read_line(&mut line).map(|_| line);
        let _ = address_sender.send(result);
    });
    let address_line = address_receiver.recv_timeout(Duration::from_secs(5))??;
    let address: std::net::SocketAddr = address_line
        .trim()
        .strip_prefix("prdb-gateway listening on ")
        .ok_or("gateway listen line had an unexpected prefix")?
        .parse()?;

    if let Some((package, _)) = network_sql_package.as_ref() {
        let mut query_secret = restricted_secret;
        let mut query =
            SubmissionClient::connect_authenticated(address, restricted_token, &mut query_secret)?;
        assert_eq!(query_secret, [0; 32]);
        let invocation_id =
            query.upload_and_seal(package, &[], 30_000, WorkloadClass::Operational)?;
        let receipt_path = root.join("restricted-metadata.receipt");
        let (results, terminal_outcome) = {
            let mut started = query.start(invocation_id, &receipt_path)?;
            let mut results = Vec::new();
            let mut terminal_outcome = None;
            while let Some(event) = started.next_event()? {
                match event {
                    prdb_engine::client_submit::ClientResultEvent::Data(bytes) => {
                        results.push(bytes);
                    }
                    prdb_engine::client_submit::ClientResultEvent::Terminal(terminal) => {
                        terminal_outcome = Some(terminal.outcome);
                        break;
                    }
                }
            }
            (results, terminal_outcome)
        };
        assert_eq!(results, vec![b"empty_roundtrip".to_vec()]);
        assert_eq!(
            terminal_outcome,
            Some(prdb_engine::client_submit::TerminalOutcome::Committed)
        );
        assert_eq!(query.status(invocation_id)?, InvocationStatus::Committed);
        assert_eq!(
            query.cancel(invocation_id)?,
            CancellationResult::Terminal(TerminalOutcome::Committed)
        );

        let credentials_path = root.join("run-sql-restricted.cred");
        write_client_credentials(
            &credentials_path,
            &ClientCredentials {
                token_id: restricted_token,
                token_secret: restricted_secret,
            },
        )?;
        let sql_path = root.join("run-sql.sql");
        std::fs::write(
            &sql_path,
            "SELECT schema_name FROM schemata WHERE schema_name = $1 LIMIT 1",
        )?;
        std::fs::set_permissions(&sql_path, std::fs::Permissions::from_mode(0o600))?;
        let parameters_path = root.join("run-sql.parameters.json");
        std::fs::write(
            &parameters_path,
            r#"{"format":"PDB-SQL-PARAMETERS-1","values":[{"type":"text","value":"empty_roundtrip"}]}"#,
        )?;
        std::fs::set_permissions(&parameters_path, std::fs::Permissions::from_mode(0o600))?;
        let requested_scope_path = root.join("run-sql.scope.psc1");
        let mut requested_scope = b"PSC1\0\x01\x01\x01".to_vec();
        requested_scope.extend_from_slice(&sql_visible_schema_id);
        std::fs::write(&requested_scope_path, requested_scope)?;
        std::fs::set_permissions(
            &requested_scope_path,
            std::fs::Permissions::from_mode(0o600),
        )?;
        let receipt_path = root.join("run-sql.receipt");
        let sql_address = address.to_string();
        let sdk_release_dir = std::env::var_os("PRDB_TEST_INSTALLED_SDK_RELEASE_DIR")
            .map(std::path::PathBuf::from)
            .ok_or("PRDB_TEST_INSTALLED_SDK_RELEASE_DIR is required for run-sql")?;
        let sql_sdk = sdk_release_dir.join("libprdb_sql.so.1");
        let include_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("include");
        let invoke_run_sql = |address: &str,
                              credentials: &std::path::Path,
                              schema: &str,
                              sql_path: &std::path::Path,
                              parameters_path: &std::path::Path,
                              receipt_path: &std::path::Path|
         -> Result<std::process::Output, Box<dyn std::error::Error>> {
            Ok(Command::new(env!("CARGO_BIN_EXE_prdb-client"))
                .args([
                    "run-sql",
                    "--address",
                    address,
                    "--credentials",
                    credentials
                        .to_str()
                        .ok_or("run-sql credentials path is not UTF-8")?,
                    "--sql-file",
                    sql_path.to_str().ok_or("run-sql SQL path is not UTF-8")?,
                    "--schema",
                    schema,
                    "--scope",
                    requested_scope_path
                        .to_str()
                        .ok_or("run-sql scope path is not UTF-8")?,
                    "--receipt",
                    receipt_path
                        .to_str()
                        .ok_or("run-sql receipt path is not UTF-8")?,
                    "--sdk-library",
                    sql_sdk.to_str().ok_or("SQL SDK path is not UTF-8")?,
                    "--include-dir",
                    include_dir
                        .to_str()
                        .ok_or("SQL SDK include path is not UTF-8")?,
                    "--sdk-release",
                    "1",
                    "--cpu-baseline",
                    "x86_64-v1",
                    "--runtime-profile",
                    "test-runtime",
                    "--compiler-identity",
                    "TinyCC process integration fixture",
                    "--parameters",
                    parameters_path
                        .to_str()
                        .ok_or("run-sql parameter path is not UTF-8")?,
                ])
                .output()?)
        };
        let run_sql = invoke_run_sql(
            &sql_address,
            &credentials_path,
            "information_schema",
            &sql_path,
            &parameters_path,
            &receipt_path,
        )?;
        assert!(
            run_sql.status.success(),
            "run-sql failed: {}",
            String::from_utf8_lossy(&run_sql.stderr)
        );
        assert!(String::from_utf8_lossy(&run_sql.stderr).contains("outcome=Committed"));
        let sqlr = &run_sql.stdout;
        assert!(sqlr.len() >= 32);
        assert_eq!(&sqlr[..4], b"SQLR");
        assert_eq!(u16::from_be_bytes([sqlr[4], sqlr[5]]), 1);
        assert_eq!(u16::from_be_bytes([sqlr[6], sqlr[7]]), 1);
        let column_name_len = usize::from(u16::from_be_bytes([sqlr[8], sqlr[9]]));
        assert_eq!(&sqlr[10..10 + column_name_len], b"schema_name");
        let type_offset = 10 + column_name_len;
        assert_eq!(
            u32::from_be_bytes(sqlr[type_offset..type_offset + 4].try_into()?),
            6
        );
        assert_eq!(sqlr[type_offset + 6], 0);
        let value_len_offset = type_offset + 7;
        let value_len =
            u32::from_be_bytes(sqlr[value_len_offset..value_len_offset + 4].try_into()?) as usize;
        let value_start = value_len_offset + 4;
        assert_eq!(value_len, b"empty_roundtrip".len());
        assert_eq!(
            &sqlr[value_start..value_start + value_len],
            b"empty_roundtrip"
        );
        assert_eq!(value_start + value_len, sqlr.len());
        let receipt_text = std::fs::read_to_string(&receipt_path)?;
        let receipt_id = receipt_text
            .strip_prefix("PDBR1 ")
            .and_then(|value| decode_id_hex(value.trim()))
            .ok_or("run-sql wrote a malformed invocation receipt")?;
        let mut status_secret = restricted_secret;
        let mut status_client =
            SubmissionClient::connect_authenticated(address, restricted_token, &mut status_secret)?;
        assert_eq!(status_secret, [0; 32]);
        assert_eq!(
            status_client.status(receipt_id)?,
            InvocationStatus::Committed
        );

        let invalid_sql_path = root.join("run-sql-invalid.sql");
        std::fs::write(&invalid_sql_path, "SELECT FROM schemata")?;
        std::fs::set_permissions(&invalid_sql_path, std::fs::Permissions::from_mode(0o600))?;
        let empty_parameters_path = root.join("run-sql-empty.parameters.json");
        std::fs::write(
            &empty_parameters_path,
            r#"{"format":"PDB-SQL-PARAMETERS-1","values":[]}"#,
        )?;
        std::fs::set_permissions(
            &empty_parameters_path,
            std::fs::Permissions::from_mode(0o600),
        )?;
        let aborted_receipt_path = root.join("run-sql-aborted.receipt");
        let aborted_run_sql = invoke_run_sql(
            &sql_address,
            &credentials_path,
            "information_schema",
            &invalid_sql_path,
            &empty_parameters_path,
            &aborted_receipt_path,
        )?;
        assert!(
            !aborted_run_sql.status.success(),
            "malformed SQL unexpectedly succeeded: {}",
            String::from_utf8_lossy(&aborted_run_sql.stderr)
        );
        assert!(aborted_run_sql.stdout.is_empty());
        assert!(String::from_utf8_lossy(&aborted_run_sql.stderr).contains("outcome=Aborted"));
        let aborted_receipt_text = std::fs::read_to_string(&aborted_receipt_path)?;
        let aborted_receipt_id = aborted_receipt_text
            .strip_prefix("PDBR1 ")
            .and_then(|value| decode_id_hex(value.trim()))
            .ok_or("aborted run-sql wrote a malformed invocation receipt")?;
        assert_eq!(
            status_client.status(aborted_receipt_id)?,
            InvocationStatus::Aborted
        );

        let multiple_statement_sql_path = root.join("run-sql-multiple-statements.sql");
        std::fs::write(
            &multiple_statement_sql_path,
            "SELECT schema_name FROM schemata; SELECT schema_name FROM schemata",
        )?;
        std::fs::set_permissions(
            &multiple_statement_sql_path,
            std::fs::Permissions::from_mode(0o600),
        )?;
        let multiple_statement_receipt_path = root.join("run-sql-multiple-statements.receipt");
        let multiple_statement_run_sql = invoke_run_sql(
            &sql_address,
            &credentials_path,
            "information_schema",
            &multiple_statement_sql_path,
            &empty_parameters_path,
            &multiple_statement_receipt_path,
        )?;
        assert!(
            !multiple_statement_run_sql.status.success(),
            "multiple SQL statements unexpectedly succeeded: {}",
            String::from_utf8_lossy(&multiple_statement_run_sql.stderr)
        );
        assert!(multiple_statement_run_sql.stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&multiple_statement_run_sql.stderr).contains("outcome=Aborted")
        );
        let multiple_statement_receipt_text =
            std::fs::read_to_string(&multiple_statement_receipt_path)?;
        let multiple_statement_receipt_id = multiple_statement_receipt_text
            .strip_prefix("PDBR1 ")
            .and_then(|value| decode_id_hex(value.trim()))
            .ok_or("multiple-statement run-sql wrote a malformed invocation receipt")?;
        assert_eq!(
            status_client.status(multiple_statement_receipt_id)?,
            InvocationStatus::Aborted
        );

        let scope_reader_credentials_path = root.join("run-sql-scope-reader.cred");
        write_client_credentials(
            &scope_reader_credentials_path,
            &ClientCredentials {
                token_id: scope_reader_token,
                token_secret: scope_reader_secret,
            },
        )?;
        let outside_scope_sql_path = root.join("run-sql-outside-scope.sql");
        std::fs::write(
            &outside_scope_sql_path,
            "SELECT payload FROM private_records LIMIT 1",
        )?;
        std::fs::set_permissions(
            &outside_scope_sql_path,
            std::fs::Permissions::from_mode(0o600),
        )?;
        let outside_scope_receipt_path = root.join("run-sql-outside-scope.receipt");
        let outside_scope_run_sql = invoke_run_sql(
            &sql_address,
            &scope_reader_credentials_path,
            "admin_private",
            &outside_scope_sql_path,
            &empty_parameters_path,
            &outside_scope_receipt_path,
        )?;
        assert!(
            !outside_scope_run_sql.status.success(),
            "run-sql widened PSC1 to include a separately authorized schema"
        );
        assert!(outside_scope_run_sql.stdout.is_empty());
        assert!(String::from_utf8_lossy(&outside_scope_run_sql.stderr).contains("outcome=Aborted"));
        let outside_scope_receipt_text = std::fs::read_to_string(&outside_scope_receipt_path)?;
        let outside_scope_receipt_id = outside_scope_receipt_text
            .strip_prefix("PDBR1 ")
            .and_then(|value| decode_id_hex(value.trim()))
            .ok_or("out-of-scope run-sql wrote a malformed invocation receipt")?;
        let mut scope_status_secret = scope_reader_secret;
        let mut scope_status_client = SubmissionClient::connect_authenticated(
            address,
            scope_reader_token,
            &mut scope_status_secret,
        )?;
        assert_eq!(scope_status_secret, [0; 32]);
        assert_eq!(
            scope_status_client.status(outside_scope_receipt_id)?,
            InvocationStatus::Aborted
        );

        let uncertain_receipt_path = root.join("run-sql-uncertain.receipt");
        let (proxy_address, proxy) = start_response_dropping_proxy(address)?;
        let uncertain_run_sql = invoke_run_sql(
            &proxy_address.to_string(),
            &credentials_path,
            "information_schema",
            &sql_path,
            &parameters_path,
            &uncertain_receipt_path,
        )?;
        assert!(
            !uncertain_run_sql.status.success(),
            "run-sql reported success after its START response was dropped"
        );
        assert!(uncertain_run_sql.stdout.is_empty());
        let start_count = proxy
            .join()
            .map_err(|_| "START response proxy panicked")??;
        assert_eq!(start_count, 1, "run-sql must send START exactly once");
        let uncertain_receipt_text = std::fs::read_to_string(&uncertain_receipt_path)?;
        let uncertain_receipt_id = uncertain_receipt_text
            .strip_prefix("PDBR1 ")
            .and_then(|value| decode_id_hex(value.trim()))
            .ok_or("uncertain run-sql did not preserve its START receipt")?;
        let mut uncertain_status_secret = restricted_secret;
        let mut uncertain_status_client = SubmissionClient::connect_authenticated(
            address,
            restricted_token,
            &mut uncertain_status_secret,
        )?;
        assert_eq!(uncertain_status_secret, [0; 32]);
        let status_deadline = Instant::now() + Duration::from_secs(5);
        let uncertain_status = loop {
            let status = uncertain_status_client.status(uncertain_receipt_id)?;
            if matches!(
                status,
                InvocationStatus::Committed | InvocationStatus::Aborted
            ) {
                break status;
            }
            if Instant::now() >= status_deadline {
                return Err(
                    format!("lost START outcome did not become terminal: {status:?}").into(),
                );
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        assert!(matches!(
            uncertain_status,
            InvocationStatus::Committed | InvocationStatus::Aborted
        ));
        remove_client_credentials(&credentials_path)?;

        let cancel_credentials_path = root.join("cancel-owner.cred");
        let cancel_credentials = ClientCredentials {
            token_id: restricted_token,
            token_secret: restricted_secret,
        };
        write_client_credentials(&cancel_credentials_path, &cancel_credentials)?;
        drop(cancel_credentials);
        let invocation_id_text = encode_id_hex(&invocation_id);
        let cancel_address_text = address.to_string();
        let cancel_command = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
            .args([
                "cancel",
                "--address",
                &cancel_address_text,
                "--credentials",
                cancel_credentials_path
                    .to_str()
                    .ok_or("credential path is not UTF-8")?,
                "--invocation-id",
                &invocation_id_text,
            ])
            .output()?;
        assert!(
            cancel_command.status.success(),
            "cancel CLI failed: {}",
            String::from_utf8_lossy(&cancel_command.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&cancel_command.stdout).trim(),
            "committed"
        );
        remove_client_credentials(&cancel_credentials_path)?;
    }

    let mut admin_control_secret = token_secret;
    let mut metadata = MetadataClient::connect_authenticated(address, token_id, &mut token_secret)?;
    assert_eq!(token_secret, [0; 32]);
    let schemas = metadata.list_schemas()?;
    assert!(schemas.iter().any(|schema| {
        schema.schema_id == prdb_engine::metadata_discovery::INFORMATION_SCHEMA_SCHEMA_ID
            && schema.name == "information_schema"
    }));
    assert!(schemas.iter().any(|schema| schema.name == "admin_private"));
    let relations =
        metadata.list_relations(prdb_engine::metadata_discovery::INFORMATION_SCHEMA_SCHEMA_ID)?;
    assert_eq!(relations.len(), 3);
    let columns = metadata.list_columns(
        prdb_engine::metadata_discovery::INFORMATION_SCHEMA_SCHEMA_ID,
        relations
            .iter()
            .find(|relation| relation.name == "schemata")
            .ok_or("information_schema.schemata was missing")?
            .relation_id,
    )?;
    assert!(!columns.is_empty());
    let schemata_relation = relations
        .iter()
        .find(|relation| relation.name == "schemata")
        .ok_or("information_schema.schemata was missing")?;
    let discover_credentials_path = root.join("discover.credentials");
    let discover_credentials = ClientCredentials {
        token_id,
        token_secret: admin_control_secret,
    };
    write_client_credentials(&discover_credentials_path, &discover_credentials)?;
    drop(discover_credentials);
    let address_text = address.to_string();
    let schema_id_text =
        encode_id_hex(&prdb_engine::metadata_discovery::INFORMATION_SCHEMA_SCHEMA_ID);
    let relation_id_text = encode_id_hex(&schemata_relation.relation_id);
    let discover = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "discover",
            "--address",
            &address_text,
            "--credentials",
            discover_credentials_path
                .to_str()
                .ok_or("non-UTF-8 credential path")?,
            "--schema",
            "information_schema",
            "--relation",
            "schemata",
        ])
        .output()?;
    assert!(
        discover.status.success(),
        "discover CLI failed: {}",
        String::from_utf8_lossy(&discover.stderr)
    );
    let discover_output = String::from_utf8(discover.stdout)?;
    assert!(discover_output.contains(&format!("schema\t{schema_id_text}\tinformation_schema")));
    assert!(discover_output.contains(&format!(
        "relation\t{schema_id_text}\t{relation_id_text}\tschemata"
    )));
    assert!(discover_output.contains(&format!("column\t{relation_id_text}\t0\t")));
    remove_client_credentials(&discover_credentials_path)?;
    assert!(
        metadata
            .list_relations(restricted_schema_id)?
            .iter()
            .any(|relation| relation.relation_id == restricted_table_id)
    );
    assert!(
        !metadata
            .list_columns(restricted_schema_id, restricted_table_id)?
            .is_empty()
    );
    drop(metadata);

    let mut owner_metadata =
        AdminClient::connect_authenticated(address, owner_token, &mut owner_secret)?;
    assert!(
        owner_metadata
            .list_schemas_page(128, None)?
            .records
            .iter()
            .any(|schema| schema.name == "admin_private")
    );
    assert!(
        owner_metadata
            .list_relations_page(restricted_schema_id, 128, None)?
            .records
            .iter()
            .any(|relation| relation.relation_id == restricted_table_id)
    );
    assert!(
        !owner_metadata
            .list_columns_page(restricted_schema_id, restricted_table_id, 128, None)?
            .records
            .is_empty()
    );
    let owner_indexes =
        owner_metadata.list_indexes_page(restricted_schema_id, restricted_table_id, 128, None)?;
    let owner_column_id = owner_metadata
        .list_columns_page(restricted_schema_id, restricted_table_id, 128, None)?
        .records
        .first()
        .ok_or("schema owner returned no declared columns")?
        .column_id;
    assert!(owner_indexes.records.iter().any(|index| {
        index.index_id == restricted_index_id
            && index.column_ids == vec![owner_column_id]
            && !index.unique
    }));
    let owner_schema_cursor = owner_metadata
        .list_schemas_page(1, None)?
        .next_cursor
        .ok_or("expected a second schema visible through ownership")?;
    let mut revocation_admin_secret = admin_control_secret;
    let mut reconnect_admin_secret = admin_control_secret;
    let mut administrator =
        AdminClient::connect_authenticated(address, token_id, &mut revocation_admin_secret)?;
    administrator
        .set_schema_owner(restricted_schema_id, admin_user_id)
        .map_err(|error| format!("schema-owner transfer failed: {error:?}"))?;
    assert!(matches!(
        owner_metadata.list_schemas_page(1, Some(&owner_schema_cursor)),
        Err(prdb_engine::client_submit::ClientSubmitError::ServerRejected(_))
    ));
    let administrator_cursor = administrator
        .list_schemas_page(1, None)
        .map_err(|error| format!("administrator schema cursor start failed: {error:?}"))?
        .next_cursor
        .ok_or("expected a second administrator schema page")?;
    drop(owner_metadata);
    drop(administrator);
    let mut reconnected_admin =
        AdminClient::connect_authenticated(address, token_id, &mut reconnect_admin_secret)
            .map_err(|error| format!("administrator reconnect failed: {error:?}"))?;
    let replayed_cursor = reconnected_admin.list_schemas_page(1, Some(&administrator_cursor));
    assert!(
        matches!(
            &replayed_cursor,
            Err(prdb_engine::client_submit::ClientSubmitError::ServerRejected(_))
        ),
        "replayed administrator cursor was not rejected: {replayed_cursor:?}"
    );
    drop(reconnected_admin);

    let mut restricted_admin_secret = restricted_secret;
    let mut restricted_metadata =
        MetadataClient::connect_authenticated(address, restricted_token, &mut restricted_secret)?;
    let restricted_schemas = restricted_metadata.list_schemas()?;
    assert!(restricted_schemas.iter().any(|schema| {
        schema.schema_id == prdb_engine::metadata_discovery::INFORMATION_SCHEMA_SCHEMA_ID
    }));
    assert!(
        !restricted_schemas
            .iter()
            .any(|schema| schema.name == "admin_private")
    );
    assert!(
        restricted_metadata
            .list_relations(prdb_engine::metadata_discovery::INFORMATION_SCHEMA_SCHEMA_ID)?
            .iter()
            .any(|relation| relation.name == "schemata")
    );
    assert!(
        restricted_metadata
            .list_relations(restricted_schema_id)?
            .is_empty()
    );
    assert!(
        restricted_metadata
            .list_columns(restricted_schema_id, restricted_table_id)?
            .is_empty()
    );
    drop(restricted_metadata);
    let mut restricted_admin = AdminClient::connect_authenticated(
        address,
        restricted_token,
        &mut restricted_admin_secret,
    )?;
    assert!(
        restricted_admin
            .list_indexes_page(restricted_schema_id, restricted_table_id, 128, None)?
            .records
            .is_empty()
    );
    assert!(
        restricted_admin
            .list_grants_page(restricted_schema_id, restricted_table_id, 128, None)?
            .records
            .is_empty()
    );
    assert!(
        restricted_admin
            .list_policies_page(restricted_schema_id, restricted_table_id, 128, None)?
            .records
            .is_empty()
    );
    drop(restricted_admin);

    let admin_cancel_credentials_path = root.join("admin-cancel.cred");
    let admin_cancel_credentials = ClientCredentials {
        token_id,
        token_secret: admin_control_secret,
    };
    write_client_credentials(&admin_cancel_credentials_path, &admin_cancel_credentials)?;
    drop(admin_cancel_credentials);
    let mut admin =
        AdminClient::connect_authenticated(address, token_id, &mut admin_control_secret)?;
    assert_eq!(admin_control_secret, [0; 32]);
    let ddl_schema = admin.create_schema("process_ddl", admin_user_id)?;
    let ddl_table = admin.create_table(
        ddl_schema.identity_id,
        "records",
        &[
            AdminColumnDefinition {
                name: "key".to_owned(),
                scalar_type: ScalarType::Integer,
                nullable: true,
                generated_uuid_identity: false,
                default: None,
            },
            AdminColumnDefinition {
                name: "label".to_owned(),
                scalar_type: ScalarType::Text,
                nullable: true,
                generated_uuid_identity: false,
                default: None,
            },
        ],
    )?;
    let ddl_constraint =
        admin.create_unique_constraint(ddl_table.table_id, &[ddl_table.column_ids[0]])?;
    assert!(ddl_schema.commit_sequence < ddl_table.commit_sequence);
    assert!(ddl_table.commit_sequence < ddl_constraint.commit_sequence);
    let mut verify_credentials = read_client_credentials(&admin_cancel_credentials_path)?;
    let mut ddl_metadata = MetadataClient::connect_authenticated(
        address,
        verify_credentials.token_id,
        &mut verify_credentials.token_secret,
    )?;
    assert!(
        ddl_metadata
            .list_schemas()?
            .iter()
            .any(
                |schema| schema.schema_id == ddl_schema.identity_id && schema.name == "process_ddl"
            )
    );
    assert!(
        ddl_metadata
            .list_relations(ddl_schema.identity_id)?
            .iter()
            .any(
                |relation| relation.relation_id == ddl_table.table_id && relation.name == "records"
            )
    );
    assert_eq!(
        ddl_metadata
            .list_columns(ddl_schema.identity_id, ddl_table.table_id)?
            .first()
            .map(|column| column.column_id),
        Some(ddl_table.column_ids[0])
    );
    drop(ddl_metadata);
    assert_eq!(
        admin.cancel_invocation(committed_invocation_id)?,
        CancellationResult::Terminal(TerminalOutcome::Committed)
    );
    let committed_invocation_id_text = encode_id_hex(&committed_invocation_id);
    let admin_cancel_address_text = address.to_string();
    let admin_cancel_command = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-cancel",
            "--address",
            &admin_cancel_address_text,
            "--credentials",
            admin_cancel_credentials_path
                .to_str()
                .ok_or("credential path is not UTF-8")?,
            "--invocation-id",
            &committed_invocation_id_text,
        ])
        .output()?;
    assert!(
        admin_cancel_command.status.success(),
        "admin-cancel CLI failed: {}",
        String::from_utf8_lossy(&admin_cancel_command.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&admin_cancel_command.stdout).trim(),
        "committed"
    );
    let admin_status_address_text = address.to_string();
    let admin_status_command = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-status",
            "--address",
            &admin_status_address_text,
            "--credentials",
            admin_cancel_credentials_path
                .to_str()
                .ok_or("credential path is not UTF-8")?,
            "--invocation-id",
            &committed_invocation_id_text,
        ])
        .output()?;
    assert!(
        admin_status_command.status.success(),
        "admin-status CLI failed: {}",
        String::from_utf8_lossy(&admin_status_command.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&admin_status_command.stdout).trim(),
        "committed"
    );
    let created_role = admin.create_role("process_admin_role")?;
    assert_ne!(created_role.identity_id, [0; 16]);
    let renamed = admin.rename_role(created_role.identity_id, "process_admin_role_v2")?;
    assert!(renamed.commit_sequence > created_role.commit_sequence);
    let granted = admin
        .grant_system_privilege(created_role.identity_id, AdminSystemPrivilege::CreateSchema)?;
    let revoked = admin
        .revoke_system_privilege(created_role.identity_id, AdminSystemPrivilege::CreateSchema)?;
    assert!(revoked.commit_sequence > granted.commit_sequence);
    let privilege_cli_address = address.to_string();
    let privilege_cli_credentials = admin_cancel_credentials_path
        .to_str()
        .ok_or("credential path is not UTF-8")?;
    let system_privilege_sql_path = root.join("system-privilege.sql");
    std::fs::write(
        &system_privilege_sql_path,
        "GRANT MANAGE_ROLES ON DATABASE TO ROLE process_admin_role_v2;",
    )?;
    let system_privilege_sql_path_text = system_privilege_sql_path
        .to_str()
        .ok_or("SQL path is not UTF-8")?;
    let grant_privilege_cli = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &privilege_cli_address,
            "--credentials",
            privilege_cli_credentials,
            "--sql-file",
            system_privilege_sql_path_text,
        ])
        .output()?;
    assert!(
        grant_privilege_cli.status.success(),
        "system-privilege-grant CLI failed: {}",
        String::from_utf8_lossy(&grant_privilege_cli.stderr)
    );
    assert!(
        String::from_utf8_lossy(&grant_privilege_cli.stdout).contains("granted system privilege")
    );
    let member_user = admin
        .create_user("process_member_user")
        .map_err(|error| format!("create delegated member user: {error:?}"))?;
    let member_user_expiration =
        i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_micros())?
            .saturating_add(900_000_000);
    let member_token = admin.issue_token(
        member_user.identity_id,
        "role membership test",
        member_user_expiration,
    )?;
    let member_credentials_path = root.join("role-member.cred");
    let member_credentials = ClientCredentials {
        token_id: member_token.token_id,
        token_secret: *member_token.token_secret(),
    };
    write_client_credentials(&member_credentials_path, &member_credentials)?;
    drop(member_credentials);
    drop(member_token);
    let usage_grant = admin.grant_object_privilege(
        member_user.identity_id,
        ddl_schema.identity_id,
        AdminObjectPrivilege::Usage,
        AdminObjectGrantTarget::Object,
    )?;
    let table_grant = admin.grant_object_privilege(
        member_user.identity_id,
        ddl_table.table_id,
        AdminObjectPrivilege::Select,
        AdminObjectGrantTarget::Object,
    )?;
    let column_grant = admin.grant_object_privilege(
        member_user.identity_id,
        ddl_table.table_id,
        AdminObjectPrivilege::Select,
        AdminObjectGrantTarget::Column(ddl_table.column_ids[0]),
    )?;
    assert!(usage_grant.commit_sequence < table_grant.commit_sequence);
    assert!(table_grant.commit_sequence < column_grant.commit_sequence);
    let mut member_metadata_credentials = read_client_credentials(&member_credentials_path)?;
    let mut member_metadata = MetadataClient::connect_authenticated(
        address,
        member_metadata_credentials.token_id,
        &mut member_metadata_credentials.token_secret,
    )?;
    assert!(
        member_metadata
            .list_schemas()?
            .iter()
            .any(|schema| schema.schema_id == ddl_schema.identity_id)
    );
    assert_eq!(
        member_metadata
            .list_relations(ddl_schema.identity_id)?
            .iter()
            .map(|relation| relation.relation_id)
            .collect::<Vec<_>>(),
        [ddl_table.table_id]
    );
    assert_eq!(
        member_metadata
            .list_columns(ddl_schema.identity_id, ddl_table.table_id)?
            .iter()
            .map(|column| column.column_id)
            .collect::<Vec<_>>(),
        ddl_table.column_ids
    );
    let revoked_column = admin.revoke_object_privilege(
        member_user.identity_id,
        ddl_table.table_id,
        AdminObjectPrivilege::Select,
        AdminObjectGrantTarget::Column(ddl_table.column_ids[0]),
    )?;
    let revoked_table = admin.revoke_object_privilege(
        member_user.identity_id,
        ddl_table.table_id,
        AdminObjectPrivilege::Select,
        AdminObjectGrantTarget::Object,
    )?;
    let revoked_usage = admin.revoke_object_privilege(
        member_user.identity_id,
        ddl_schema.identity_id,
        AdminObjectPrivilege::Usage,
        AdminObjectGrantTarget::Object,
    )?;
    assert!(revoked_column.commit_sequence < revoked_table.commit_sequence);
    assert!(revoked_table.commit_sequence < revoked_usage.commit_sequence);
    assert!(
        !member_metadata
            .list_schemas()?
            .iter()
            .any(|schema| schema.schema_id == ddl_schema.identity_id)
    );
    drop(member_metadata);
    let object_privilege_sql_path = root.join("object-privilege.sql");
    std::fs::write(
        &object_privilege_sql_path,
        "GRANT USAGE ON SCHEMA process_ddl TO USER process_member_user;",
    )?;
    let object_privilege_sql_path_text = object_privilege_sql_path
        .to_str()
        .ok_or("SQL path is not UTF-8")?;
    let grant_object_privilege_sql = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address.to_string(),
            "--credentials",
            admin_cancel_credentials_path
                .to_str()
                .ok_or("credential path is not UTF-8")?,
            "--sql-file",
            object_privilege_sql_path_text,
        ])
        .output()?;
    assert!(
        grant_object_privilege_sql.status.success(),
        "object privilege GRANT SQL failed: {}",
        String::from_utf8_lossy(&grant_object_privilege_sql.stderr)
    );
    assert!(
        String::from_utf8_lossy(&grant_object_privilege_sql.stdout)
            .contains("granted object privilege")
    );
    std::fs::write(
        &object_privilege_sql_path,
        "REVOKE USAGE ON SCHEMA process_ddl FROM USER process_member_user;",
    )?;
    let revoke_object_privilege_sql = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address.to_string(),
            "--credentials",
            admin_cancel_credentials_path
                .to_str()
                .ok_or("credential path is not UTF-8")?,
            "--sql-file",
            object_privilege_sql_path_text,
        ])
        .output()?;
    assert!(
        revoke_object_privilege_sql.status.success(),
        "object privilege REVOKE SQL failed: {}",
        String::from_utf8_lossy(&revoke_object_privilege_sql.stderr)
    );
    assert!(
        String::from_utf8_lossy(&revoke_object_privilege_sql.stdout)
            .contains("revoked object privilege")
    );
    std::fs::write(
        &object_privilege_sql_path,
        "GRANT SELECT (key) ON TABLE process_ddl.records TO USER process_member_user;",
    )?;
    let grant_existing_column = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address.to_string(),
            "--credentials",
            admin_cancel_credentials_path
                .to_str()
                .ok_or("credential path is not UTF-8")?,
            "--sql-file",
            object_privilege_sql_path_text,
        ])
        .output()?;
    assert!(
        grant_existing_column.status.success(),
        "single-column privilege GRANT SQL failed: {}",
        String::from_utf8_lossy(&grant_existing_column.stderr)
    );
    admin.execute_object_privilege_sql(
        "GRANT ALTER, REFERENCES (key) ON TABLE process_ddl.records TO USER process_member_user",
    )?;
    let mixed_scope_grants =
        admin.list_grants_page(ddl_schema.identity_id, ddl_table.table_id, 128, None)?;
    assert!(mixed_scope_grants.records.iter().any(|grant| {
        grant.grantee_id == member_user.identity_id
            && grant.privilege
                == prdb_engine::metadata_discovery::AdministrativeGrantPrivilege::Alter
            && grant.target_scope
                == prdb_engine::metadata_discovery::AdministrativeGrantTargetScope::Object
            && grant.target_id == ddl_table.table_id
    }));
    assert!(mixed_scope_grants.records.iter().any(|grant| {
        grant.grantee_id == member_user.identity_id
            && grant.privilege
                == prdb_engine::metadata_discovery::AdministrativeGrantPrivilege::References
            && grant.target_scope
                == prdb_engine::metadata_discovery::AdministrativeGrantTargetScope::Column
            && grant.target_id == ddl_table.column_ids[0]
    }));
    admin.execute_object_privilege_sql(
        "REVOKE ALTER, REFERENCES (key) ON TABLE process_ddl.records FROM USER process_member_user",
    )?;
    let after_mixed_scope_revoke =
        admin.list_grants_page(ddl_schema.identity_id, ddl_table.table_id, 128, None)?;
    assert!(!after_mixed_scope_revoke.records.iter().any(|grant| {
        grant.grantee_id == member_user.identity_id
            && matches!(
                grant.privilege,
                prdb_engine::metadata_discovery::AdministrativeGrantPrivilege::Alter
                    | prdb_engine::metadata_discovery::AdministrativeGrantPrivilege::References
            )
    }));
    std::fs::write(
        &object_privilege_sql_path,
        "GRANT SELECT (key, label) ON TABLE process_ddl.records TO USER process_member_user;",
    )?;
    let grant_column_privilege_sql = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address.to_string(),
            "--credentials",
            admin_cancel_credentials_path
                .to_str()
                .ok_or("credential path is not UTF-8")?,
            "--sql-file",
            object_privilege_sql_path_text,
        ])
        .output()?;
    assert!(
        !grant_column_privilege_sql.status.success(),
        "overlapping multi-column privilege GRANT unexpectedly succeeded"
    );
    let after_overlapping_grant =
        admin.list_grants_page(ddl_schema.identity_id, ddl_table.table_id, 128, None)?;
    assert!(
        ddl_table
            .column_ids
            .iter()
            .enumerate()
            .all(|(index, column_id)| {
                let has_grant = after_overlapping_grant.records.iter().any(|grant| {
                    grant.grantee_id == member_user.identity_id
                && grant.privilege
                    == prdb_engine::metadata_discovery::AdministrativeGrantPrivilege::Select
                && grant.target_scope
                    == prdb_engine::metadata_discovery::AdministrativeGrantTargetScope::Column
                && grant.target_id == *column_id
                });
                has_grant == (index == 0)
            })
    );
    std::fs::write(
        &object_privilege_sql_path,
        "REVOKE SELECT (key) ON TABLE process_ddl.records FROM USER process_member_user;",
    )?;
    let revoke_existing_column = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address.to_string(),
            "--credentials",
            admin_cancel_credentials_path
                .to_str()
                .ok_or("credential path is not UTF-8")?,
            "--sql-file",
            object_privilege_sql_path_text,
        ])
        .output()?;
    assert!(
        revoke_existing_column.status.success(),
        "single-column privilege REVOKE SQL failed: {}",
        String::from_utf8_lossy(&revoke_existing_column.stderr)
    );
    std::fs::write(
        &object_privilege_sql_path,
        "GRANT SELECT (key, label) ON TABLE process_ddl.records TO USER process_member_user;",
    )?;
    let grant_column_privilege_sql = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address.to_string(),
            "--credentials",
            admin_cancel_credentials_path
                .to_str()
                .ok_or("credential path is not UTF-8")?,
            "--sql-file",
            object_privilege_sql_path_text,
        ])
        .output()?;
    assert!(
        grant_column_privilege_sql.status.success(),
        "multi-column privilege GRANT SQL failed: {}",
        String::from_utf8_lossy(&grant_column_privilege_sql.stderr)
    );
    let multi_column_grants =
        admin.list_grants_page(ddl_schema.identity_id, ddl_table.table_id, 128, None)?;
    assert!(ddl_table.column_ids.iter().all(|column_id| {
        multi_column_grants.records.iter().any(|grant| {
            grant.grantee_id == member_user.identity_id
                && grant.privilege
                    == prdb_engine::metadata_discovery::AdministrativeGrantPrivilege::Select
                && grant.target_scope
                    == prdb_engine::metadata_discovery::AdministrativeGrantTargetScope::Column
                && grant.target_id == *column_id
        })
    }));
    std::fs::write(
        &object_privilege_sql_path,
        "REVOKE SELECT (key, label) ON TABLE process_ddl.records FROM USER process_member_user;",
    )?;
    let revoke_column_privilege_sql = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address.to_string(),
            "--credentials",
            admin_cancel_credentials_path
                .to_str()
                .ok_or("credential path is not UTF-8")?,
            "--sql-file",
            object_privilege_sql_path_text,
        ])
        .output()?;
    assert!(
        revoke_column_privilege_sql.status.success(),
        "column privilege REVOKE SQL failed: {}",
        String::from_utf8_lossy(&revoke_column_privilege_sql.stderr)
    );
    let multi_column_revokes =
        admin.list_grants_page(ddl_schema.identity_id, ddl_table.table_id, 128, None)?;
    assert!(!multi_column_revokes.records.iter().any(|grant| {
        grant.grantee_id == member_user.identity_id
            && grant.privilege
                == prdb_engine::metadata_discovery::AdministrativeGrantPrivilege::Select
            && grant.target_scope
                == prdb_engine::metadata_discovery::AdministrativeGrantTargetScope::Column
    }));
    admin.execute_object_privilege_sql(
        "GRANT ALTER ON TABLE process_ddl.records TO USER process_member_user",
    )?;
    std::fs::write(
        &object_privilege_sql_path,
        "GRANT ALTER, INDEX ON TABLE process_ddl.records TO USER process_member_user;",
    )?;
    let overlapping_privilege_batch = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address.to_string(),
            "--credentials",
            admin_cancel_credentials_path
                .to_str()
                .ok_or("credential path is not UTF-8")?,
            "--sql-file",
            object_privilege_sql_path_text,
        ])
        .output()?;
    assert!(
        !overlapping_privilege_batch.status.success(),
        "multi-privilege GRANT unexpectedly accepted a pre-existing member"
    );
    let after_overlapping_privilege_batch =
        admin.list_grants_page(ddl_schema.identity_id, ddl_table.table_id, 128, None)?;
    assert!(
        after_overlapping_privilege_batch
            .records
            .iter()
            .any(|grant| {
                grant.grantee_id == member_user.identity_id
                    && grant.privilege
                        == prdb_engine::metadata_discovery::AdministrativeGrantPrivilege::Alter
            })
    );
    assert!(
        !after_overlapping_privilege_batch
            .records
            .iter()
            .any(|grant| {
                grant.grantee_id == member_user.identity_id
                    && grant.privilege
                        == prdb_engine::metadata_discovery::AdministrativeGrantPrivilege::Index
            })
    );
    admin.execute_object_privilege_sql(
        "REVOKE ALTER ON TABLE process_ddl.records FROM USER process_member_user",
    )?;
    std::fs::write(
        &object_privilege_sql_path,
        "GRANT ALTER, INDEX, MANAGE_POLICIES, MANAGE_GRANTS ON TABLE process_ddl.records TO USER process_member_user;",
    )?;
    let grant_multiple_privileges = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address.to_string(),
            "--credentials",
            admin_cancel_credentials_path
                .to_str()
                .ok_or("credential path is not UTF-8")?,
            "--sql-file",
            object_privilege_sql_path_text,
        ])
        .output()?;
    assert!(
        grant_multiple_privileges.status.success(),
        "multi-privilege GRANT SQL failed: {}",
        String::from_utf8_lossy(&grant_multiple_privileges.stderr)
    );
    let multiple_privilege_grants =
        admin.list_grants_page(ddl_schema.identity_id, ddl_table.table_id, 128, None)?;
    for privilege in [
        prdb_engine::metadata_discovery::AdministrativeGrantPrivilege::Alter,
        prdb_engine::metadata_discovery::AdministrativeGrantPrivilege::Index,
        prdb_engine::metadata_discovery::AdministrativeGrantPrivilege::ManagePolicies,
        prdb_engine::metadata_discovery::AdministrativeGrantPrivilege::ManageGrants,
    ] {
        assert!(multiple_privilege_grants.records.iter().any(|grant| {
            grant.grantee_id == member_user.identity_id
                && grant.privilege == privilege
                && grant.target_scope
                    == prdb_engine::metadata_discovery::AdministrativeGrantTargetScope::Object
        }));
    }
    assert!(!multiple_privilege_grants.records.iter().any(|grant| {
        grant.grantee_id == member_user.identity_id
            && grant.privilege
                == prdb_engine::metadata_discovery::AdministrativeGrantPrivilege::Delete
    }));
    std::fs::write(
        &object_privilege_sql_path,
        "REVOKE ALTER, INDEX, MANAGE_POLICIES, MANAGE_GRANTS, DELETE ON TABLE process_ddl.records FROM USER process_member_user;",
    )?;
    let partial_revoke_batch = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address.to_string(),
            "--credentials",
            admin_cancel_credentials_path
                .to_str()
                .ok_or("credential path is not UTF-8")?,
            "--sql-file",
            object_privilege_sql_path_text,
        ])
        .output()?;
    assert!(
        !partial_revoke_batch.status.success(),
        "multi-privilege REVOKE unexpectedly accepted an absent DELETE grant"
    );
    let after_partial_revoke_batch =
        admin.list_grants_page(ddl_schema.identity_id, ddl_table.table_id, 128, None)?;
    for privilege in [
        prdb_engine::metadata_discovery::AdministrativeGrantPrivilege::Alter,
        prdb_engine::metadata_discovery::AdministrativeGrantPrivilege::Index,
        prdb_engine::metadata_discovery::AdministrativeGrantPrivilege::ManagePolicies,
        prdb_engine::metadata_discovery::AdministrativeGrantPrivilege::ManageGrants,
    ] {
        assert!(after_partial_revoke_batch.records.iter().any(|grant| {
            grant.grantee_id == member_user.identity_id
                && grant.privilege == privilege
                && grant.target_scope
                    == prdb_engine::metadata_discovery::AdministrativeGrantTargetScope::Object
        }));
    }
    std::fs::write(
        &object_privilege_sql_path,
        "REVOKE ALTER, INDEX, MANAGE_POLICIES, MANAGE_GRANTS ON TABLE process_ddl.records FROM USER process_member_user;",
    )?;
    let revoke_multiple_privileges = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address.to_string(),
            "--credentials",
            admin_cancel_credentials_path
                .to_str()
                .ok_or("credential path is not UTF-8")?,
            "--sql-file",
            object_privilege_sql_path_text,
        ])
        .output()?;
    assert!(
        revoke_multiple_privileges.status.success(),
        "multi-privilege REVOKE SQL failed: {}",
        String::from_utf8_lossy(&revoke_multiple_privileges.stderr)
    );
    let revoked_multiple_privileges =
        admin.list_grants_page(ddl_schema.identity_id, ddl_table.table_id, 128, None)?;
    assert!(!revoked_multiple_privileges.records.iter().any(|grant| {
        grant.grantee_id == member_user.identity_id
            && matches!(
                grant.privilege,
                prdb_engine::metadata_discovery::AdministrativeGrantPrivilege::Alter
                    | prdb_engine::metadata_discovery::AdministrativeGrantPrivilege::Index
                    | prdb_engine::metadata_discovery::AdministrativeGrantPrivilege::ManagePolicies
                    | prdb_engine::metadata_discovery::AdministrativeGrantPrivilege::ManageGrants
            )
    }));
    std::fs::write(
        &object_privilege_sql_path,
        "GRANT SELECT ON TABLE process_ddl.records TO USER process_member_user;",
    )?;
    let grant_all_existing_columns = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address.to_string(),
            "--credentials",
            admin_cancel_credentials_path
                .to_str()
                .ok_or("credential path is not UTF-8")?,
            "--sql-file",
            object_privilege_sql_path_text,
        ])
        .output()?;
    assert!(
        grant_all_existing_columns.status.success(),
        "table-wide SELECT GRANT SQL failed: {}",
        String::from_utf8_lossy(&grant_all_existing_columns.stderr)
    );
    let table_wide_grants =
        admin.list_grants_page(ddl_schema.identity_id, ddl_table.table_id, 128, None)?;
    assert!(ddl_table.column_ids.iter().all(|column_id| {
        table_wide_grants.records.iter().any(|grant| {
            grant.grantee_id == member_user.identity_id
                && grant.privilege
                    == prdb_engine::metadata_discovery::AdministrativeGrantPrivilege::Select
                && grant.target_scope
                    == prdb_engine::metadata_discovery::AdministrativeGrantTargetScope::Column
                && grant.target_id == *column_id
        })
    }));
    assert!(!table_wide_grants.records.iter().any(|grant| {
        grant.grantee_id == member_user.identity_id
            && grant.privilege
                == prdb_engine::metadata_discovery::AdministrativeGrantPrivilege::Select
            && grant.target_scope
                == prdb_engine::metadata_discovery::AdministrativeGrantTargetScope::Object
    }));
    let added_column = admin.add_column(
        ddl_table.table_id,
        &AdminColumnDefinition {
            name: "future_field".to_owned(),
            scalar_type: ScalarType::Integer,
            nullable: true,
            generated_uuid_identity: false,
            default: None,
        },
    )?;
    let after_column_add =
        admin.list_grants_page(ddl_schema.identity_id, ddl_table.table_id, 128, None)?;
    assert!(!after_column_add.records.iter().any(|grant| {
        grant.grantee_id == member_user.identity_id
            && grant.privilege
                == prdb_engine::metadata_discovery::AdministrativeGrantPrivilege::Select
            && grant.target_id == added_column.column_id
    }));
    let dropped_table = admin.drop_table(ddl_table.table_id)?;
    assert!(dropped_table.commit_sequence > ddl_constraint.commit_sequence);
    assert!(
        admin.drop_schema(ddl_schema.identity_id)?.commit_sequence > dropped_table.commit_sequence
    );
    let role_membership_address = address.to_string();
    let role_membership_credentials = admin_cancel_credentials_path
        .to_str()
        .ok_or("credential path is not UTF-8")?;
    let role_membership_sql_path = root.join("role-membership.sql");
    std::fs::write(
        &role_membership_sql_path,
        "GRANT process_admin_role_v2 TO USER process_member_user;",
    )?;
    let role_membership_sql_path_text = role_membership_sql_path
        .to_str()
        .ok_or("SQL path is not UTF-8")?;
    let grant_role_cli = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &role_membership_address,
            "--credentials",
            role_membership_credentials,
            "--sql-file",
            role_membership_sql_path_text,
        ])
        .output()?;
    assert!(
        grant_role_cli.status.success(),
        "role-grant CLI failed: {}",
        String::from_utf8_lossy(&grant_role_cli.stderr)
    );
    assert!(String::from_utf8_lossy(&grant_role_cli.stdout).contains("granted role membership"));
    let member_cli_credentials = member_credentials_path
        .to_str()
        .ok_or("member credential path is not UTF-8")?;
    let delegated_role_create_cli = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "role-create",
            "--address",
            &role_membership_address,
            "--credentials",
            member_cli_credentials,
            "--name",
            "process_delegated_role",
        ])
        .output()?;
    assert!(
        delegated_role_create_cli.status.success(),
        "role-create with inherited MANAGE_ROLES failed: {}",
        String::from_utf8_lossy(&delegated_role_create_cli.stderr)
    );
    let delegated_role_output = String::from_utf8_lossy(&delegated_role_create_cli.stdout);
    assert!(delegated_role_output.contains("created role "));
    let delegated_role_id = delegated_role_output
        .split_whitespace()
        .nth(2)
        .ok_or("delegated role-create output omitted its ID")?;
    let delegated_role_id = decode_id_hex(delegated_role_id)
        .ok_or("delegated role-create output contained an invalid ID")?;
    assert!(
        matches!(
            admin.drop_role(created_role.identity_id),
            Err(
                prdb_engine::client_submit::ClientSubmitError::AdministrativeOperationRejected { .. }
            )
        ),
        "role drop did not return a rejected outcome while membership still referenced the role"
    );
    std::fs::write(
        &role_membership_sql_path,
        "REVOKE process_admin_role_v2 FROM USER process_member_user;",
    )?;
    let revoke_role_cli = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &role_membership_address,
            "--credentials",
            role_membership_credentials,
            "--sql-file",
            role_membership_sql_path_text,
        ])
        .output()?;
    assert!(
        revoke_role_cli.status.success(),
        "role-revoke CLI failed: {}",
        String::from_utf8_lossy(&revoke_role_cli.stderr)
    );
    assert!(String::from_utf8_lossy(&revoke_role_cli.stdout).contains("revoked role membership"));
    let mut member_authentication_probe = read_client_credentials(&member_credentials_path)?;
    let authenticated_member = SubmissionClient::connect_authenticated(
        address,
        member_authentication_probe.token_id,
        &mut member_authentication_probe.token_secret,
    )?;
    drop(authenticated_member);
    let denied_role_create_cli = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "role-create",
            "--address",
            &role_membership_address,
            "--credentials",
            member_cli_credentials,
            "--name",
            "process_denied_role_after_membership_revoke",
        ])
        .output()?;
    assert!(
        !denied_role_create_cli.status.success(),
        "role-create remained authorized after the member's role was revoked"
    );
    std::fs::write(
        &system_privilege_sql_path,
        "REVOKE MANAGE_ROLES ON DATABASE FROM ROLE process_admin_role_v2;",
    )?;
    let revoke_privilege_cli = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &privilege_cli_address,
            "--credentials",
            privilege_cli_credentials,
            "--sql-file",
            system_privilege_sql_path_text,
        ])
        .output()?;
    assert!(
        revoke_privilege_cli.status.success(),
        "system-privilege-revoke CLI failed: {}",
        String::from_utf8_lossy(&revoke_privilege_cli.stderr)
    );
    assert!(
        String::from_utf8_lossy(&revoke_privilege_cli.stdout).contains("revoked system privilege")
    );
    let admin_credentials = read_client_credentials(&admin_cancel_credentials_path)?;
    let mut reauthenticated_secret = admin_credentials.token_secret;
    admin = AdminClient::connect_authenticated(
        address,
        admin_credentials.token_id,
        &mut reauthenticated_secret,
    )?;
    assert_eq!(reauthenticated_secret, [0; 32]);
    let dropped = admin
        .drop_role(created_role.identity_id)
        .map_err(|error| format!("drop membership target role after revoke: {error:?}"))?;
    assert!(dropped.commit_sequence > revoked.commit_sequence);
    admin.drop_role(delegated_role_id)?;
    admin.drop_user(member_user.identity_id)?;
    let mut dropped_member_credentials = read_client_credentials(&member_credentials_path)?;
    assert!(
        SubmissionClient::connect_authenticated(
            address,
            dropped_member_credentials.token_id,
            &mut dropped_member_credentials.token_secret,
        )
        .is_err()
    );
    remove_client_credentials(&member_credentials_path)?;

    let created_schema = admin.create_schema("process_admin_schema", admin_user_id)?;
    let created_table = admin.create_table(
        created_schema.identity_id,
        "process_admin_table",
        &[AdminColumnDefinition {
            name: "payload".to_owned(),
            scalar_type: ScalarType::Text,
            nullable: true,
            generated_uuid_identity: false,
            default: None,
        }],
    )?;
    assert_ne!(created_table.table_id, [0; 16]);
    assert_eq!(created_table.column_ids.len(), 1);
    let add_column_sql_path = root.join("add-column.sql");
    std::fs::write(
        &add_column_sql_path,
        "ALTER TABLE process_admin_schema.process_admin_table ADD COLUMN IF NOT EXISTS label VARCHAR(30) NOT NULL DEFAULT 'unknown';",
    )?;
    let add_column_cli = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address.to_string(),
            "--credentials",
            admin_cancel_credentials_path
                .to_str()
                .ok_or("credential path is not UTF-8")?,
            "--sql-file",
            add_column_sql_path
                .to_str()
                .ok_or("SQL path is not UTF-8")?,
        ])
        .output()?;
    assert!(
        add_column_cli.status.success(),
        "admin-sql ADD COLUMN failed: {}",
        String::from_utf8_lossy(&add_column_cli.stderr)
    );
    assert!(String::from_utf8_lossy(&add_column_cli.stdout).contains("added column"));
    let added_columns =
        admin.list_columns_page(created_schema.identity_id, created_table.table_id, 16, None)?;
    let added_label = added_columns
        .records
        .iter()
        .find(|column| column.name == "label")
        .ok_or("committed ADD COLUMN SQL was absent from authenticated metadata")?;
    assert_eq!(added_label.ordinal, 1);
    assert_eq!(
        added_label.scalar_type,
        ScalarType::Varchar { max_chars: 30 }
    );
    assert!(!added_label.nullable);
    assert!(added_columns.next_cursor.is_none());
    for (suffix, sql, expected_nullable) in [
        (
            "set-not-null",
            "ALTER TABLE process_admin_schema.process_admin_table ALTER COLUMN payload SET NOT NULL;",
            false,
        ),
        (
            "drop-not-null",
            "ALTER TABLE process_admin_schema.process_admin_table ALTER COLUMN payload DROP NOT NULL;",
            true,
        ),
    ] {
        let sql_path = root.join(format!("{suffix}.sql"));
        std::fs::write(&sql_path, sql)?;
        let result = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
            .args([
                "admin-sql",
                "--address",
                &address.to_string(),
                "--credentials",
                admin_cancel_credentials_path
                    .to_str()
                    .ok_or("credential path is not UTF-8")?,
                "--sql-file",
                sql_path.to_str().ok_or("SQL path is not UTF-8")?,
            ])
            .output()?;
        assert!(
            result.status.success(),
            "admin-sql {suffix} failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        let columns = admin.list_columns_page(
            created_schema.identity_id,
            created_table.table_id,
            16,
            None,
        )?;
        let label = columns
            .records
            .iter()
            .find(|column| column.name == "payload")
            .ok_or("column nullability SQL removed its target column")?;
        assert_eq!(label.nullable, expected_nullable, "{suffix} metadata");
    }
    let add_column_if_missing_path = root.join("add-column-if-not-exists.sql");
    std::fs::write(
        &add_column_if_missing_path,
        "ALTER TABLE process_admin_schema.process_admin_table ADD COLUMN IF NOT EXISTS label TEXT;",
    )?;
    let add_column_if_missing = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address.to_string(),
            "--credentials",
            admin_cancel_credentials_path
                .to_str()
                .ok_or("credential path is not UTF-8")?,
            "--sql-file",
            add_column_if_missing_path
                .to_str()
                .ok_or("SQL path is not UTF-8")?,
        ])
        .output()?;
    assert!(
        add_column_if_missing.status.success(),
        "admin-sql conditional ADD COLUMN failed: {}",
        String::from_utf8_lossy(&add_column_if_missing.stderr)
    );
    assert!(
        String::from_utf8_lossy(&add_column_if_missing.stdout)
            .contains("column already exists; no operation committed")
    );
    let dependency_sql_path = root.join("column-dependency.sql");
    std::fs::write(
        &dependency_sql_path,
        "CREATE INDEX IF NOT EXISTS process_label_index ON process_admin_schema.process_admin_table (label);",
    )?;
    let create_dependency_index = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address.to_string(),
            "--credentials",
            admin_cancel_credentials_path
                .to_str()
                .ok_or("credential path is not UTF-8")?,
            "--sql-file",
            dependency_sql_path
                .to_str()
                .ok_or("SQL path is not UTF-8")?,
        ])
        .output()?;
    assert!(
        create_dependency_index.status.success(),
        "admin-sql CREATE INDEX failed: {}",
        String::from_utf8_lossy(&create_dependency_index.stderr)
    );
    let repeated_create_dependency_index = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address.to_string(),
            "--credentials",
            admin_cancel_credentials_path
                .to_str()
                .ok_or("credential path is not UTF-8")?,
            "--sql-file",
            dependency_sql_path
                .to_str()
                .ok_or("SQL path is not UTF-8")?,
        ])
        .output()?;
    assert!(
        repeated_create_dependency_index.status.success(),
        "admin-sql repeated CREATE INDEX IF NOT EXISTS failed: {}",
        String::from_utf8_lossy(&repeated_create_dependency_index.stderr)
    );
    assert!(
        String::from_utf8_lossy(&repeated_create_dependency_index.stdout)
            .contains("index already exists; no operation committed")
    );
    let widen_column_sql_path = root.join("widen-varchar-column.sql");
    std::fs::write(
        &widen_column_sql_path,
        "ALTER TABLE process_admin_schema.process_admin_table ALTER COLUMN label TYPE VARCHAR(60);",
    )?;
    let widen_column_cli = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address.to_string(),
            "--credentials",
            admin_cancel_credentials_path
                .to_str()
                .ok_or("credential path is not UTF-8")?,
            "--sql-file",
            widen_column_sql_path
                .to_str()
                .ok_or("SQL path is not UTF-8")?,
        ])
        .output()?;
    assert!(
        widen_column_cli.status.success(),
        "admin-sql VARCHAR widening failed: {}",
        String::from_utf8_lossy(&widen_column_cli.stderr)
    );
    assert!(
        String::from_utf8_lossy(&widen_column_cli.stdout)
            .contains("widened VARCHAR column label to 60 characters")
    );
    let widened_columns =
        admin.list_columns_page(created_schema.identity_id, created_table.table_id, 16, None)?;
    let widened_label = widened_columns
        .records
        .iter()
        .find(|column| column.name == "label")
        .ok_or("widened column missing from authenticated metadata")?;
    assert_eq!(widened_label.column_id, added_label.column_id);
    assert_eq!(
        widened_label.scalar_type,
        ScalarType::Varchar { max_chars: 60 }
    );
    let table_index_list = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "index-list",
            "--address",
            &address.to_string(),
            "--credentials",
            admin_cancel_credentials_path
                .to_str()
                .ok_or("credential path is not UTF-8")?,
            "--schema-id",
            &encode_id_hex(&created_schema.identity_id),
            "--table-id",
            &encode_id_hex(&created_table.table_id),
        ])
        .output()?;
    assert!(
        table_index_list.status.success(),
        "index-list failed after CREATE INDEX: {}",
        String::from_utf8_lossy(&table_index_list.stderr)
    );
    let table_index_json = String::from_utf8_lossy(&table_index_list.stdout);
    assert!(table_index_json.contains("\"name\":\"process_label_index\""));
    assert!(table_index_json.contains(&encode_id_hex(&added_label.column_id)));
    assert!(table_index_json.contains("\"unique\":false"));
    let truncate_sql_path = root.join("truncate-table.sql");
    std::fs::write(
        &truncate_sql_path,
        "TRUNCATE TABLE process_admin_schema.process_admin_table RESTRICT;",
    )?;
    let truncate_cli = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address.to_string(),
            "--credentials",
            admin_cancel_credentials_path
                .to_str()
                .ok_or("credential path is not UTF-8")?,
            "--sql-file",
            truncate_sql_path.to_str().ok_or("SQL path is not UTF-8")?,
        ])
        .output()?;
    assert!(
        truncate_cli.status.success(),
        "admin-sql TRUNCATE TABLE failed: {}",
        String::from_utf8_lossy(&truncate_cli.stderr)
    );
    assert!(String::from_utf8_lossy(&truncate_cli.stdout).contains("truncated table"));
    let retained_index_list = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "index-list",
            "--address",
            &address.to_string(),
            "--credentials",
            admin_cancel_credentials_path
                .to_str()
                .ok_or("credential path is not UTF-8")?,
            "--schema-id",
            &encode_id_hex(&created_schema.identity_id),
            "--table-id",
            &encode_id_hex(&created_table.table_id),
        ])
        .output()?;
    assert!(
        retained_index_list.status.success(),
        "index-list failed after TRUNCATE TABLE: {}",
        String::from_utf8_lossy(&retained_index_list.stderr)
    );
    assert!(
        String::from_utf8_lossy(&retained_index_list.stdout)
            .contains("\"name\":\"process_label_index\"")
    );
    let named_check_sql_path = root.join("add-named-check-constraint.sql");
    std::fs::write(
        &named_check_sql_path,
        "ALTER TABLE process_admin_schema.process_admin_table ADD CONSTRAINT process_label_present CHECK (label IS NOT NULL);",
    )?;
    let named_check_cli = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address.to_string(),
            "--credentials",
            admin_cancel_credentials_path
                .to_str()
                .ok_or("credential path is not UTF-8")?,
            "--sql-file",
            named_check_sql_path
                .to_str()
                .ok_or("SQL path is not UTF-8")?,
        ])
        .output()?;
    assert!(
        named_check_cli.status.success(),
        "admin-sql named CHECK creation failed: {}",
        String::from_utf8_lossy(&named_check_cli.stderr)
    );
    let listed_named_constraint = admin
        .list_constraints_page(created_schema.identity_id, created_table.table_id, 16, None)?
        .records
        .into_iter()
        .find(|constraint| constraint.name == "process_label_present")
        .ok_or("authenticated constraint metadata omitted the named CHECK")?;
    assert_eq!(
        listed_named_constraint.kind,
        prdb_engine::metadata_discovery::AdministrativeConstraintKind::Check
    );
    let drop_named_check_sql_path = root.join("drop-named-check-constraint.sql");
    std::fs::write(
        &drop_named_check_sql_path,
        "ALTER TABLE process_admin_schema.process_admin_table DROP CONSTRAINT process_label_present;",
    )?;
    let drop_named_check_cli = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address.to_string(),
            "--credentials",
            admin_cancel_credentials_path
                .to_str()
                .ok_or("credential path is not UTF-8")?,
            "--sql-file",
            drop_named_check_sql_path
                .to_str()
                .ok_or("SQL path is not UTF-8")?,
        ])
        .output()?;
    assert!(
        drop_named_check_cli.status.success(),
        "admin-sql named CHECK drop failed: {}",
        String::from_utf8_lossy(&drop_named_check_cli.stderr)
    );
    let drop_column_sql_path = root.join("drop-column.sql");
    std::fs::write(
        &drop_column_sql_path,
        "ALTER TABLE process_admin_schema.process_admin_table DROP COLUMN label RESTRICT;",
    )?;
    let blocked_column_drop = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address.to_string(),
            "--credentials",
            admin_cancel_credentials_path
                .to_str()
                .ok_or("credential path is not UTF-8")?,
            "--sql-file",
            drop_column_sql_path
                .to_str()
                .ok_or("SQL path is not UTF-8")?,
        ])
        .output()?;
    assert!(
        !blocked_column_drop.status.success(),
        "admin-sql DROP COLUMN ignored its index dependency"
    );
    let columns_after_blocked_drop =
        admin.list_columns_page(created_schema.identity_id, created_table.table_id, 16, None)?;
    assert_eq!(columns_after_blocked_drop.records.len(), 2);
    assert!(
        columns_after_blocked_drop
            .records
            .iter()
            .any(|column| column.name == "label")
    );
    std::fs::write(
        &dependency_sql_path,
        "DROP INDEX IF EXISTS process_admin_schema.process_label_index RESTRICT;",
    )?;
    let drop_dependency_index = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address.to_string(),
            "--credentials",
            admin_cancel_credentials_path
                .to_str()
                .ok_or("credential path is not UTF-8")?,
            "--sql-file",
            dependency_sql_path
                .to_str()
                .ok_or("SQL path is not UTF-8")?,
        ])
        .output()?;
    assert!(
        drop_dependency_index.status.success(),
        "admin-sql DROP INDEX failed: {}",
        String::from_utf8_lossy(&drop_dependency_index.stderr)
    );
    std::fs::write(
        &dependency_sql_path,
        "DROP INDEX IF EXISTS process_admin_schema.process_label_index RESTRICT;",
    )?;
    let repeated_drop_dependency_index = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address.to_string(),
            "--credentials",
            admin_cancel_credentials_path
                .to_str()
                .ok_or("credential path is not UTF-8")?,
            "--sql-file",
            dependency_sql_path
                .to_str()
                .ok_or("SQL path is not UTF-8")?,
        ])
        .output()?;
    assert!(
        repeated_drop_dependency_index.status.success(),
        "admin-sql repeated DROP INDEX IF EXISTS failed: {}",
        String::from_utf8_lossy(&repeated_drop_dependency_index.stderr)
    );
    assert!(
        String::from_utf8_lossy(&repeated_drop_dependency_index.stdout)
            .contains("index was absent; no operation committed")
    );
    let drop_column_cli = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address.to_string(),
            "--credentials",
            admin_cancel_credentials_path
                .to_str()
                .ok_or("credential path is not UTF-8")?,
            "--sql-file",
            drop_column_sql_path
                .to_str()
                .ok_or("SQL path is not UTF-8")?,
        ])
        .output()?;
    assert!(
        drop_column_cli.status.success(),
        "admin-sql DROP COLUMN failed: {}",
        String::from_utf8_lossy(&drop_column_cli.stderr)
    );
    assert!(String::from_utf8_lossy(&drop_column_cli.stdout).contains("dropped column"));
    let columns_after_drop =
        admin.list_columns_page(created_schema.identity_id, created_table.table_id, 16, None)?;
    assert_eq!(columns_after_drop.records.len(), 1);
    assert_eq!(columns_after_drop.records[0].name, "payload");
    assert!(columns_after_drop.next_cursor.is_none());
    let unique_constraint = admin
        .create_unique_constraint(created_table.table_id, &created_table.column_ids)
        .map_err(|error| format!("process ADMIN create_unique_constraint failed: {error}"))?;
    let transfer_owner = admin.create_user("process_table_owner")?;
    let token_expiration =
        i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_micros())?
            .saturating_add(900_000_000);
    let transfer_credentials = admin.issue_token(
        transfer_owner.identity_id,
        "table owner transfer",
        token_expiration,
    )?;
    let mut transfer_token_id = transfer_credentials.token_id;
    let transfer_credential_path = root.join("table-owner.cred");
    let transfer_client_credentials = ClientCredentials {
        token_id: transfer_token_id,
        token_secret: *transfer_credentials.token_secret(),
    };
    write_client_credentials(&transfer_credential_path, &transfer_client_credentials)?;
    drop(transfer_client_credentials);
    drop(transfer_credentials);
    let address_text = address.to_string();
    let admin_cli_credentials = admin_cancel_credentials_path
        .to_str()
        .ok_or("credential path is not UTF-8")?;
    let rename_sql_path = root.join("rename-user.sql");
    std::fs::write(
        &rename_sql_path,
        "ALTER USER process_table_owner RENAME TO process_table_owner_renamed;",
    )?;
    let rename_sql_path_text = rename_sql_path.to_str().ok_or("SQL path is not UTF-8")?;
    let rename_user_cli = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address_text,
            "--credentials",
            admin_cli_credentials,
            "--sql-file",
            rename_sql_path_text,
        ])
        .output()?;
    assert!(
        rename_user_cli.status.success(),
        "user-rename CLI failed: {}",
        String::from_utf8_lossy(&rename_user_cli.stderr)
    );
    assert!(String::from_utf8_lossy(&rename_user_cli.stdout).contains("renamed user"));
    let login_sql_path = root.join("login-state.sql");
    std::fs::write(
        &login_sql_path,
        "ALTER USER process_table_owner_renamed NOLOGIN;",
    )?;
    let login_sql_path_text = login_sql_path.to_str().ok_or("SQL path is not UTF-8")?;
    let disable_login_cli = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address_text,
            "--credentials",
            admin_cli_credentials,
            "--sql-file",
            login_sql_path_text,
        ])
        .output()?;
    assert!(
        disable_login_cli.status.success(),
        "user-login disable CLI failed: {}",
        String::from_utf8_lossy(&disable_login_cli.stderr)
    );
    assert!(String::from_utf8_lossy(&disable_login_cli.stdout).contains("set user NOLOGIN"));
    let mut revoked_transfer_credentials = read_client_credentials(&transfer_credential_path)?;
    let revoked_transfer_token_id = revoked_transfer_credentials.token_id;
    assert!(
        SubmissionClient::connect_authenticated(
            address,
            revoked_transfer_token_id,
            &mut revoked_transfer_credentials.token_secret,
        )
        .is_err()
    );
    std::fs::write(
        &login_sql_path,
        "ALTER USER process_table_owner_renamed LOGIN;",
    )?;
    let enable_login_cli = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address_text,
            "--credentials",
            admin_cli_credentials,
            "--sql-file",
            login_sql_path_text,
        ])
        .output()?;
    assert!(
        enable_login_cli.status.success(),
        "user-login enable CLI failed: {}",
        String::from_utf8_lossy(&enable_login_cli.stderr)
    );
    assert!(String::from_utf8_lossy(&enable_login_cli.stdout).contains("set user LOGIN"));
    let mut still_revoked_credentials = read_client_credentials(&transfer_credential_path)?;
    assert!(
        SubmissionClient::connect_authenticated(
            address,
            still_revoked_credentials.token_id,
            &mut still_revoked_credentials.token_secret,
        )
        .is_err()
    );
    let renewed_credentials = admin.issue_token(
        transfer_owner.identity_id,
        "table owner transfer renewed",
        token_expiration,
    )?;
    transfer_token_id = renewed_credentials.token_id;
    remove_client_credentials(&transfer_credential_path)?;
    let transfer_client_credentials = ClientCredentials {
        token_id: transfer_token_id,
        token_secret: *renewed_credentials.token_secret(),
    };
    write_client_credentials(&transfer_credential_path, &transfer_client_credentials)?;
    drop(transfer_client_credentials);
    drop(renewed_credentials);
    let admin_owner_id_text = encode_id_hex(&admin_user_id);
    admin.execute_object_privilege_sql(
        "GRANT ALTER ON TABLE process_admin_schema.process_admin_table TO USER process_table_owner_renamed",
    )?;
    let table_alter_sql_path = root.join("table-alter-grantee.sql");
    for (statement, expected) in [
        (
            "ALTER TABLE process_admin_schema.process_admin_table RENAME TO process_admin_table_altered;",
            "renamed table",
        ),
        (
            "ALTER TABLE process_admin_schema.process_admin_table_altered RENAME TO process_admin_table;",
            "renamed table",
        ),
    ] {
        std::fs::write(&table_alter_sql_path, statement)?;
        let command = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
            .args([
                "admin-sql",
                "--address",
                &address_text,
                "--credentials",
                transfer_credential_path
                    .to_str()
                    .ok_or("credential path is not UTF-8")?,
                "--sql-file",
                table_alter_sql_path
                    .to_str()
                    .ok_or("SQL path is not UTF-8")?,
            ])
            .output()?;
        assert!(
            command.status.success(),
            "table ALTER grantee SQL {statement:?} failed: {}",
            String::from_utf8_lossy(&command.stderr)
        );
        assert!(String::from_utf8_lossy(&command.stdout).contains(expected));
    }
    let table_alter_credentials = read_client_credentials(&transfer_credential_path)?;
    let mut table_alter_secret = table_alter_credentials.token_secret;
    let mut table_alter_client = AdminClient::connect_authenticated(
        address,
        table_alter_credentials.token_id,
        &mut table_alter_secret,
    )?;
    table_alter_client.set_table_owner(created_table.table_id, transfer_owner.identity_id)?;
    drop(table_alter_client);
    admin.set_table_owner(created_table.table_id, admin_user_id)?;
    admin.execute_object_privilege_sql(
        "REVOKE ALTER ON TABLE process_admin_schema.process_admin_table FROM USER process_table_owner_renamed",
    )?;
    let create_alter_drop_sql_path = root.join("create-alter-drop-target.sql");
    std::fs::write(
        &create_alter_drop_sql_path,
        "CREATE TABLE IF NOT EXISTS process_admin_schema.process_admin_drop_target (value INTEGER);",
    )?;
    let create_alter_drop_target = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address_text,
            "--credentials",
            admin_cli_credentials,
            "--sql-file",
            create_alter_drop_sql_path
                .to_str()
                .ok_or("SQL path is not UTF-8")?,
        ])
        .output()?;
    assert!(
        create_alter_drop_target.status.success(),
        "ALTER drop target CREATE TABLE failed: {}",
        String::from_utf8_lossy(&create_alter_drop_target.stderr)
    );
    let repeated_create_alter_drop_target = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address_text,
            "--credentials",
            admin_cli_credentials,
            "--sql-file",
            create_alter_drop_sql_path
                .to_str()
                .ok_or("SQL path is not UTF-8")?,
        ])
        .output()?;
    assert!(
        repeated_create_alter_drop_target.status.success(),
        "administrator CREATE TABLE IF NOT EXISTS repeat failed: {}",
        String::from_utf8_lossy(&repeated_create_alter_drop_target.stderr)
    );
    assert!(
        String::from_utf8_lossy(&repeated_create_alter_drop_target.stdout)
            .contains("table already exists; no operation committed")
    );
    admin.execute_object_privilege_sql(
        "GRANT ALTER ON TABLE process_admin_schema.process_admin_drop_target TO USER process_table_owner_renamed",
    )?;
    let non_admin_create_existing_table = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address_text,
            "--credentials",
            transfer_credential_path
                .to_str()
                .ok_or("credential path is not UTF-8")?,
            "--sql-file",
            create_alter_drop_sql_path
                .to_str()
                .ok_or("SQL path is not UTF-8")?,
        ])
        .output()?;
    assert!(
        !non_admin_create_existing_table.status.success(),
        "non-administrator CREATE TABLE IF NOT EXISTS suppressed an existing target"
    );
    std::fs::write(
        &table_alter_sql_path,
        "DROP TABLE process_admin_schema.process_admin_drop_target RESTRICT;",
    )?;
    let drop_table_alter_grantee = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address_text,
            "--credentials",
            transfer_credential_path
                .to_str()
                .ok_or("credential path is not UTF-8")?,
            "--sql-file",
            table_alter_sql_path
                .to_str()
                .ok_or("SQL path is not UTF-8")?,
        ])
        .output()?;
    assert!(
        drop_table_alter_grantee.status.success(),
        "table ALTER grantee DROP TABLE failed: {}",
        String::from_utf8_lossy(&drop_table_alter_grantee.stderr)
    );
    assert!(String::from_utf8_lossy(&drop_table_alter_grantee.stdout).contains("dropped table"));
    admin.execute_object_privilege_sql(
        "GRANT ALTER ON SCHEMA process_admin_schema TO USER process_table_owner_renamed",
    )?;
    let schema_alter_sql_path = root.join("schema-alter-grantee.sql");
    for (old_name, new_name) in [
        ("process_admin_schema", "process_admin_schema_renamed"),
        ("process_admin_schema_renamed", "process_admin_schema"),
    ] {
        std::fs::write(
            &schema_alter_sql_path,
            format!("ALTER SCHEMA {old_name} RENAME TO {new_name};"),
        )?;
        let command = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
            .args([
                "admin-sql",
                "--address",
                &address_text,
                "--credentials",
                transfer_credential_path
                    .to_str()
                    .ok_or("credential path is not UTF-8")?,
                "--sql-file",
                schema_alter_sql_path
                    .to_str()
                    .ok_or("SQL path is not UTF-8")?,
            ])
            .output()?;
        assert!(
            command.status.success(),
            "schema ALTER grantee SQL failed: {}",
            String::from_utf8_lossy(&command.stderr)
        );
        assert!(String::from_utf8_lossy(&command.stdout).contains("renamed schema"));
    }
    admin.execute_object_privilege_sql(
        "REVOKE ALTER ON SCHEMA process_admin_schema FROM USER process_table_owner_renamed",
    )?;
    admin.set_schema_owner(created_schema.identity_id, transfer_owner.identity_id)?;
    let schema_id_text = encode_id_hex(&created_schema.identity_id);
    let schema_transfer_command = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "schema-owner-set",
            "--address",
            &address_text,
            "--credentials",
            transfer_credential_path
                .to_str()
                .ok_or("credential path is not UTF-8")?,
            "--schema-id",
            &schema_id_text,
            "--owner-id",
            &admin_owner_id_text,
        ])
        .output()?;
    assert!(
        schema_transfer_command.status.success(),
        "schema-owner CLI failed: {}",
        String::from_utf8_lossy(&schema_transfer_command.stderr)
    );
    assert!(
        String::from_utf8_lossy(&schema_transfer_command.stdout).contains("changed schema owner")
    );
    let schema_owner_sql_path = root.join("schema-owner.sql");
    admin.execute_object_privilege_sql(
        "GRANT ALTER ON SCHEMA process_admin_schema TO USER process_table_owner_renamed",
    )?;
    std::fs::write(
        &schema_owner_sql_path,
        "ALTER SCHEMA process_admin_schema OWNER TO process_table_owner_renamed;",
    )?;
    let schema_owner_sql_command = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address_text,
            "--credentials",
            transfer_credential_path
                .to_str()
                .ok_or("credential path is not UTF-8")?,
            "--sql-file",
            schema_owner_sql_path
                .to_str()
                .ok_or("SQL path is not UTF-8")?,
        ])
        .output()?;
    assert!(
        schema_owner_sql_command.status.success(),
        "schema owner SQL failed: {}",
        String::from_utf8_lossy(&schema_owner_sql_command.stderr)
    );
    assert!(
        String::from_utf8_lossy(&schema_owner_sql_command.stdout).contains("changed schema owner")
    );
    admin.execute_object_privilege_sql(
        "REVOKE ALTER ON SCHEMA process_admin_schema FROM USER process_table_owner_renamed",
    )?;
    std::fs::write(
        &schema_owner_sql_path,
        "CREATE SCHEMA IF NOT EXISTS process_sql_owned AUTHORIZATION process_table_owner_renamed;",
    )?;
    let create_owned_schema_sql = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address_text,
            "--credentials",
            admin_cli_credentials,
            "--sql-file",
            schema_owner_sql_path
                .to_str()
                .ok_or("SQL path is not UTF-8")?,
        ])
        .output()?;
    assert!(
        create_owned_schema_sql.status.success(),
        "CREATE SCHEMA owner-name SQL failed: {}",
        String::from_utf8_lossy(&create_owned_schema_sql.stderr)
    );
    assert!(String::from_utf8_lossy(&create_owned_schema_sql.stdout).contains("created schema"));
    let repeated_create_owned_schema_sql = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address_text,
            "--credentials",
            admin_cli_credentials,
            "--sql-file",
            schema_owner_sql_path
                .to_str()
                .ok_or("SQL path is not UTF-8")?,
        ])
        .output()?;
    assert!(
        repeated_create_owned_schema_sql.status.success(),
        "CREATE SCHEMA IF NOT EXISTS repeat failed: {}",
        String::from_utf8_lossy(&repeated_create_owned_schema_sql.stderr)
    );
    assert!(
        String::from_utf8_lossy(&repeated_create_owned_schema_sql.stdout)
            .contains("schema already exists; no operation committed")
    );
    let non_admin_create_existing_schema = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address_text,
            "--credentials",
            transfer_credential_path
                .to_str()
                .ok_or("credential path is not UTF-8")?,
            "--sql-file",
            schema_owner_sql_path
                .to_str()
                .ok_or("SQL path is not UTF-8")?,
        ])
        .output()?;
    assert!(
        !non_admin_create_existing_schema.status.success(),
        "non-administrator CREATE SCHEMA IF NOT EXISTS suppressed an existing target"
    );
    std::fs::write(
        &schema_owner_sql_path,
        "DROP SCHEMA process_sql_owned RESTRICT;",
    )?;
    let drop_owned_schema_sql = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address_text,
            "--credentials",
            admin_cli_credentials,
            "--sql-file",
            schema_owner_sql_path
                .to_str()
                .ok_or("SQL path is not UTF-8")?,
        ])
        .output()?;
    assert!(
        drop_owned_schema_sql.status.success(),
        "DROP SCHEMA SQL cleanup failed: {}",
        String::from_utf8_lossy(&drop_owned_schema_sql.stderr)
    );
    assert!(String::from_utf8_lossy(&drop_owned_schema_sql.stdout).contains("dropped schema"));
    std::fs::write(&schema_owner_sql_path, "CREATE SCHEMA process_sql_default;")?;
    let create_default_owned_schema_sql = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address_text,
            "--credentials",
            admin_cli_credentials,
            "--sql-file",
            schema_owner_sql_path
                .to_str()
                .ok_or("SQL path is not UTF-8")?,
        ])
        .output()?;
    assert!(
        create_default_owned_schema_sql.status.success(),
        "CREATE SCHEMA current-user SQL failed: {}",
        String::from_utf8_lossy(&create_default_owned_schema_sql.stderr)
    );
    std::fs::write(
        &schema_owner_sql_path,
        "DROP SCHEMA process_sql_default RESTRICT;",
    )?;
    let drop_default_owned_schema_sql = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address_text,
            "--credentials",
            admin_cli_credentials,
            "--sql-file",
            schema_owner_sql_path
                .to_str()
                .ok_or("SQL path is not UTF-8")?,
        ])
        .output()?;
    assert!(
        drop_default_owned_schema_sql.status.success(),
        "DROP SCHEMA current-user cleanup failed: {}",
        String::from_utf8_lossy(&drop_default_owned_schema_sql.stderr)
    );
    admin.set_schema_owner(created_schema.identity_id, admin_user_id)?;
    let transferred = admin.set_table_owner(created_table.table_id, transfer_owner.identity_id)?;
    assert!(transferred.commit_sequence > 0);
    let table_id_text = encode_id_hex(&created_table.table_id);
    let credential_path_text = transfer_credential_path
        .to_str()
        .ok_or("credential path is not UTF-8")?;
    let transfer_command = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "table-owner-set",
            "--address",
            &address_text,
            "--credentials",
            credential_path_text,
            "--table-id",
            &table_id_text,
            "--owner-id",
            &admin_owner_id_text,
        ])
        .output()?;
    assert!(
        transfer_command.status.success(),
        "table-owner CLI failed: {}",
        String::from_utf8_lossy(&transfer_command.stderr)
    );
    assert!(String::from_utf8_lossy(&transfer_command.stdout).contains("changed table owner"));
    let table_owner_sql_path = root.join("table-owner.sql");
    admin.execute_object_privilege_sql(
        "GRANT ALTER ON TABLE process_admin_schema.process_admin_table TO USER process_table_owner_renamed",
    )?;
    std::fs::write(
        &table_owner_sql_path,
        "ALTER TABLE process_admin_schema.process_admin_table OWNER TO USER process_table_owner_renamed;",
    )?;
    let table_owner_sql_command = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address_text,
            "--credentials",
            credential_path_text,
            "--sql-file",
            table_owner_sql_path
                .to_str()
                .ok_or("SQL path is not UTF-8")?,
        ])
        .output()?;
    assert!(
        table_owner_sql_command.status.success(),
        "table owner SQL failed: {}",
        String::from_utf8_lossy(&table_owner_sql_command.stderr)
    );
    assert!(
        String::from_utf8_lossy(&table_owner_sql_command.stdout).contains("changed table owner")
    );
    admin.execute_object_privilege_sql(
        "REVOKE ALTER ON TABLE process_admin_schema.process_admin_table FROM USER process_table_owner_renamed",
    )?;
    let mut table_owner_credentials = read_client_credentials(&transfer_credential_path)?;
    let mut table_owner_client = AdminClient::connect_authenticated(
        address,
        table_owner_credentials.token_id,
        &mut table_owner_credentials.token_secret,
    )?;
    let table_owner_constraints = table_owner_client.list_constraints_page(
        created_schema.identity_id,
        created_table.table_id,
        128,
        None,
    )?;
    assert!(table_owner_constraints.records.iter().any(|constraint| {
        constraint.kind == prdb_engine::metadata_discovery::AdministrativeConstraintKind::Unique
    }));
    let owner_alter_sql_path = root.join("table-owner-add-column.sql");
    std::fs::write(
        &owner_alter_sql_path,
        "ALTER TABLE process_admin_schema.process_admin_table ADD COLUMN owner_added INTEGER;",
    )?;
    let owner_alter_sql = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address_text,
            "--credentials",
            credential_path_text,
            "--sql-file",
            owner_alter_sql_path
                .to_str()
                .ok_or("SQL path is not UTF-8")?,
        ])
        .output()?;
    assert!(
        owner_alter_sql.status.success(),
        "table-owner ALTER SQL failed: {}",
        String::from_utf8_lossy(&owner_alter_sql.stderr)
    );
    assert!(String::from_utf8_lossy(&owner_alter_sql.stdout).contains("added column"));
    drop(table_owner_client);
    admin.set_table_owner(created_table.table_id, admin_user_id)?;
    admin.grant_object_privilege(
        transfer_owner.identity_id,
        created_table.table_id,
        AdminObjectPrivilege::Alter,
        AdminObjectGrantTarget::Object,
    )?;
    let grantee_alter_sql_path = root.join("alter-grantee-add-column.sql");
    std::fs::write(
        &grantee_alter_sql_path,
        "ALTER TABLE process_admin_schema.process_admin_table ADD COLUMN grantee_added INTEGER;",
    )?;
    let grantee_alter_sql = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address_text,
            "--credentials",
            credential_path_text,
            "--sql-file",
            grantee_alter_sql_path
                .to_str()
                .ok_or("SQL path is not UTF-8")?,
        ])
        .output()?;
    assert!(
        grantee_alter_sql.status.success(),
        "ALTER-grantee SQL failed: {}",
        String::from_utf8_lossy(&grantee_alter_sql.stderr)
    );
    assert!(String::from_utf8_lossy(&grantee_alter_sql.stdout).contains("added column"));
    admin
        .grant_object_privilege(
            transfer_owner.identity_id,
            created_schema.identity_id,
            AdminObjectPrivilege::Create,
            AdminObjectGrantTarget::Object,
        )
        .map_err(|error| format!("schema CREATE grant failed: {error:?}"))?;
    let schema_create_sql_path = root.join("create-table-with-schema-grant.sql");
    std::fs::write(
        &schema_create_sql_path,
        "CREATE TABLE process_admin_schema.process_created_by_grantee (payload TEXT);",
    )?;
    let schema_create_sql = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address_text,
            "--credentials",
            credential_path_text,
            "--sql-file",
            schema_create_sql_path
                .to_str()
                .ok_or("SQL path is not UTF-8")?,
        ])
        .output()?;
    assert!(
        schema_create_sql.status.success(),
        "CREATE-grantee SQL failed without schema USAGE: {}",
        String::from_utf8_lossy(&schema_create_sql.stderr)
    );
    assert!(String::from_utf8_lossy(&schema_create_sql.stdout).contains("created table"));
    let denied_constraint_noop_sql_path = root.join("alter-grantee-missing-constraint.sql");
    std::fs::write(
        &denied_constraint_noop_sql_path,
        "ALTER TABLE process_admin_schema.process_admin_table DROP CONSTRAINT IF EXISTS '00000000-0000-0000-0000-000000000001';",
    )?;
    let denied_constraint_noop = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address_text,
            "--credentials",
            credential_path_text,
            "--sql-file",
            denied_constraint_noop_sql_path
                .to_str()
                .ok_or("SQL path is not UTF-8")?,
        ])
        .output()?;
    assert!(
        !denied_constraint_noop.status.success(),
        "ALTER grantee received an absent-constraint no-op without database-admin authority"
    );
    let added_column = admin.add_column(
        created_table.table_id,
        &AdminColumnDefinition {
            name: "extra".to_owned(),
            scalar_type: ScalarType::Integer,
            nullable: true,
            generated_uuid_identity: false,
            default: None,
        },
    )?;
    let source_table = admin.create_table(
        created_schema.identity_id,
        "process_admin_source",
        &[AdminColumnDefinition {
            name: "payload".to_owned(),
            scalar_type: ScalarType::Text,
            nullable: true,
            generated_uuid_identity: false,
            default: None,
        }],
    )?;
    let select_grant = admin.grant_object_privilege(
        admin_user_id,
        created_table.table_id,
        AdminObjectPrivilege::Select,
        AdminObjectGrantTarget::Column(created_table.column_ids[0]),
    )?;
    let references_grant = admin.grant_object_privilege(
        admin_user_id,
        created_table.table_id,
        AdminObjectPrivilege::References,
        AdminObjectGrantTarget::Column(created_table.column_ids[0]),
    )?;
    let inline_reference_table = admin.execute_create_table_sql(
        "CREATE TABLE process_admin_schema.process_admin_inline_source (payload TEXT REFERENCES process_admin_schema.process_admin_table (payload))",
    )?;
    assert_ne!(inline_reference_table.table_id, [0; 16]);
    assert_eq!(inline_reference_table.column_ids.len(), 1);
    let inline_reference_constraints = admin.list_constraints_page(
        created_schema.identity_id,
        inline_reference_table.table_id,
        128,
        None,
    )?;
    let inline_reference_fk = inline_reference_constraints
        .records
        .iter()
        .find(|constraint| {
            constraint.kind
                == prdb_engine::metadata_discovery::AdministrativeConstraintKind::ForeignKey
        })
        .ok_or("inline REFERENCES did not publish a foreign key")?;
    assert_eq!(
        inline_reference_fk.column_ids,
        inline_reference_table.column_ids
    );
    assert_eq!(
        inline_reference_fk.referenced_table_id,
        Some(created_table.table_id)
    );
    assert_eq!(
        inline_reference_fk.referenced_column_ids,
        created_table.column_ids
    );
    let administrative_grants = admin.list_grants_page(
        created_schema.identity_id,
        created_table.table_id,
        128,
        None,
    )?;
    assert!(administrative_grants.records.iter().any(|grant| {
        grant.grantee_id == admin_user_id
            && grant.grantor_id == admin_user_id
            && grant.privilege
                == prdb_engine::metadata_discovery::AdministrativeGrantPrivilege::Select
            && grant.target_scope
                == prdb_engine::metadata_discovery::AdministrativeGrantTargetScope::Column
            && grant.target_id == created_table.column_ids[0]
    }));
    assert!(administrative_grants.records.iter().any(|grant| {
        grant.grantee_id == admin_user_id
            && grant.privilege
                == prdb_engine::metadata_discovery::AdministrativeGrantPrivilege::References
    }));
    let foreign_key_cli_address = address.to_string();
    let foreign_key_cli_credentials = admin_cancel_credentials_path
        .to_str()
        .ok_or("credential path is not UTF-8")?;
    let add_foreign_key_sql_path = root.join("add-foreign-key.sql");
    std::fs::write(
        &add_foreign_key_sql_path,
        "ALTER TABLE process_admin_schema.process_admin_source ADD FOREIGN KEY (payload) REFERENCES process_admin_schema.process_admin_table (payload);",
    )?;
    let create_foreign_key_cli = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &foreign_key_cli_address,
            "--credentials",
            foreign_key_cli_credentials,
            "--sql-file",
            add_foreign_key_sql_path
                .to_str()
                .ok_or("SQL path is not UTF-8")?,
        ])
        .output()?;
    assert!(
        create_foreign_key_cli.status.success(),
        "admin-sql ADD FOREIGN KEY failed: {}",
        String::from_utf8_lossy(&create_foreign_key_cli.stderr)
    );
    let foreign_key_cli_output = String::from_utf8_lossy(&create_foreign_key_cli.stdout);
    assert!(foreign_key_cli_output.contains("created FOREIGN KEY "));
    let foreign_key_cli_id = foreign_key_cli_output
        .split_whitespace()
        .nth(3)
        .ok_or("admin-sql ADD FOREIGN KEY output omitted the generated ID")?;
    let foreign_key_id = decode_id_hex(foreign_key_cli_id)
        .ok_or("admin-sql ADD FOREIGN KEY output contained an invalid ID")?;
    let listed_source_constraints = admin.list_constraints_page(
        created_schema.identity_id,
        source_table.table_id,
        128,
        None,
    )?;
    let listed_foreign_key = listed_source_constraints
        .records
        .iter()
        .find(|constraint| constraint.constraint_id == foreign_key_id)
        .ok_or("administrative constraint page omitted the foreign key")?;
    assert_eq!(
        listed_foreign_key.kind,
        prdb_engine::metadata_discovery::AdministrativeConstraintKind::ForeignKey
    );
    assert_eq!(
        listed_foreign_key.column_ids,
        vec![source_table.column_ids[0]]
    );
    assert_eq!(
        listed_foreign_key.referenced_table_id,
        Some(created_table.table_id)
    );
    assert_eq!(
        listed_foreign_key.referenced_column_ids,
        created_table.column_ids
    );
    let hidden_target_schema = admin.create_schema("process_hidden_fk_target", admin_user_id)?;
    let hidden_target_table = admin.create_table(
        hidden_target_schema.identity_id,
        "target",
        &[AdminColumnDefinition {
            name: "payload".to_owned(),
            scalar_type: ScalarType::Text,
            nullable: true,
            generated_uuid_identity: false,
            default: None,
        }],
    )?;
    let hidden_target_unique = admin.create_unique_constraint(
        hidden_target_table.table_id,
        &hidden_target_table.column_ids,
    )?;
    admin.grant_object_privilege(
        admin_user_id,
        hidden_target_table.table_id,
        AdminObjectPrivilege::References,
        AdminObjectGrantTarget::Column(hidden_target_table.column_ids[0]),
    )?;
    let redacted_source_schema =
        admin.create_schema("process_redacted_fk_source", transfer_owner.identity_id)?;
    let redacted_source_table = admin.create_table(
        redacted_source_schema.identity_id,
        "process_admin_redacted_source",
        &[AdminColumnDefinition {
            name: "payload".to_owned(),
            scalar_type: ScalarType::Text,
            nullable: true,
            generated_uuid_identity: false,
            default: None,
        }],
    )?;
    let redacted_foreign_key = admin.create_foreign_key(
        redacted_source_table.table_id,
        &redacted_source_table.column_ids,
        hidden_target_unique.constraint_id,
    )?;
    let dependency_list = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "dependency-list",
            "--address",
            &address.to_string(),
            "--credentials",
            admin_cancel_credentials_path
                .to_str()
                .ok_or("credential path is not UTF-8")?,
            "--object-id",
            &encode_id_hex(&redacted_foreign_key.foreign_key_id),
        ])
        .output()?;
    assert!(
        dependency_list.status.success(),
        "dependency-list CLI failed: {}",
        String::from_utf8_lossy(&dependency_list.stderr)
    );
    assert!(String::from_utf8(dependency_list.stdout)?.contains("foreign_key_target"));
    let administrator_dependencies =
        admin.list_dependencies_page(redacted_foreign_key.foreign_key_id, 128, None)?;
    assert!(
        administrator_dependencies.records.iter().any(|edge| {
            edge.dependent_id == redacted_foreign_key.foreign_key_id
            && edge.referenced_id == hidden_target_unique.constraint_id
            && edge.kind
                == prdb_engine::metadata_discovery::AdministrativeDependencyKind::ForeignKeyTarget
        }),
        "administrator dependency page: {:?}",
        administrator_dependencies.records
    );
    let redacted_owner_credentials = read_client_credentials(&transfer_credential_path)?;
    let mut redacted_owner_secret = redacted_owner_credentials.token_secret;
    let mut redacted_owner = AdminClient::connect_authenticated(
        address,
        redacted_owner_credentials.token_id,
        &mut redacted_owner_secret,
    )?;
    drop(redacted_owner_credentials);
    let redacted_constraints = redacted_owner.list_constraints_page(
        redacted_source_schema.identity_id,
        redacted_source_table.table_id,
        128,
        None,
    )?;
    let redacted_foreign_key = redacted_constraints
        .records
        .iter()
        .find(|constraint| constraint.constraint_id == redacted_foreign_key.foreign_key_id)
        .ok_or("owner constraint page omitted the foreign key")?;
    assert_eq!(redacted_foreign_key.referenced_table_id, None);
    assert!(redacted_foreign_key.referenced_column_ids.is_empty());
    let redacted_dependencies =
        redacted_owner.list_dependencies_page(redacted_foreign_key.constraint_id, 128, None)?;
    assert!(
        redacted_dependencies.records.is_empty(),
        "dependency metadata must omit edges whose target is hidden"
    );
    assert!(
        redacted_dependencies.next_cursor.is_none(),
        "an omitted hidden edge must not affect continuation or counts"
    );
    drop(redacted_owner);
    admin.set_schema_owner(redacted_source_schema.identity_id, admin_user_id)?;
    let constraint_uuid = format!(
        "{}-{}-{}-{}-{}",
        &foreign_key_cli_id[0..8],
        &foreign_key_cli_id[8..12],
        &foreign_key_cli_id[12..16],
        &foreign_key_cli_id[16..20],
        &foreign_key_cli_id[20..32]
    );
    let drop_constraint_sql_path = root.join("drop-foreign-key-constraint.sql");
    std::fs::write(
        &drop_constraint_sql_path,
        format!(
            "ALTER TABLE process_admin_schema.process_admin_source DROP CONSTRAINT IF EXISTS '{constraint_uuid}';"
        ),
    )?;
    let drop_foreign_key_cli = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &foreign_key_cli_address,
            "--credentials",
            foreign_key_cli_credentials,
            "--sql-file",
            drop_constraint_sql_path
                .to_str()
                .ok_or("SQL path is not UTF-8")?,
        ])
        .output()?;
    assert!(
        drop_foreign_key_cli.status.success(),
        "admin-sql DROP CONSTRAINT failed: {}",
        String::from_utf8_lossy(&drop_foreign_key_cli.stderr)
    );
    assert!(String::from_utf8_lossy(&drop_foreign_key_cli.stdout).contains("dropped constraint"));
    let repeated_drop_foreign_key_cli = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &foreign_key_cli_address,
            "--credentials",
            foreign_key_cli_credentials,
            "--sql-file",
            drop_constraint_sql_path
                .to_str()
                .ok_or("SQL path is not UTF-8")?,
        ])
        .output()?;
    assert!(
        repeated_drop_foreign_key_cli.status.success(),
        "admin-sql conditional DROP CONSTRAINT no-op failed: {}",
        String::from_utf8_lossy(&repeated_drop_foreign_key_cli.stderr)
    );
    assert!(
        String::from_utf8_lossy(&repeated_drop_foreign_key_cli.stdout)
            .contains("constraint did not exist")
    );
    let revoked_references = admin.revoke_object_privilege(
        admin_user_id,
        created_table.table_id,
        AdminObjectPrivilege::References,
        AdminObjectGrantTarget::Column(created_table.column_ids[0]),
    )?;
    assert!(revoked_references.commit_sequence > references_grant.commit_sequence);
    let revoked_select = admin.revoke_object_privilege(
        admin_user_id,
        created_table.table_id,
        AdminObjectPrivilege::Select,
        AdminObjectGrantTarget::Column(created_table.column_ids[0]),
    )?;
    assert!(revoked_select.commit_sequence > select_grant.commit_sequence);
    let object_grant_address = address.to_string();
    let object_grant_credentials = admin_cancel_credentials_path
        .to_str()
        .ok_or("credential path is not UTF-8")?;
    let object_grant_grantee = encode_id_hex(&transfer_owner.identity_id);
    let object_grant_table = encode_id_hex(&created_table.table_id);
    let object_grant_column = encode_id_hex(&created_table.column_ids[0]);
    let grant_object_cli = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "object-grant",
            "--address",
            &object_grant_address,
            "--credentials",
            object_grant_credentials,
            "--grantee-id",
            &object_grant_grantee,
            "--object-id",
            &object_grant_table,
            "--privilege",
            "select",
            "--target-scope",
            "column",
            "--target-id",
            &object_grant_column,
        ])
        .output()?;
    assert!(
        grant_object_cli.status.success(),
        "object-grant CLI failed: {}",
        String::from_utf8_lossy(&grant_object_cli.stderr)
    );
    assert!(String::from_utf8_lossy(&grant_object_cli.stdout).contains("granted object privilege"));
    let grant_list_schema = encode_id_hex(&created_schema.identity_id);
    let grant_list_cli = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "grant-list",
            "--address",
            &object_grant_address,
            "--credentials",
            object_grant_credentials,
            "--schema-id",
            &grant_list_schema,
            "--table-id",
            &object_grant_table,
        ])
        .output()?;
    assert!(
        grant_list_cli.status.success(),
        "grant-list CLI failed: {}",
        String::from_utf8_lossy(&grant_list_cli.stderr)
    );
    let grant_list_json = String::from_utf8_lossy(&grant_list_cli.stdout);
    assert!(grant_list_json.contains(&format!("\"grantee_id\":\"{object_grant_grantee}\"")));
    assert!(grant_list_json.contains(&format!("\"grantor_id\":\"{admin_owner_id_text}\"")));
    assert!(grant_list_json.contains("\"privilege\":\"select\""));
    assert!(grant_list_json.contains(&format!("\"target_id\":\"{object_grant_column}\"")));
    let revoke_object_cli = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "object-revoke",
            "--address",
            &object_grant_address,
            "--credentials",
            object_grant_credentials,
            "--grantee-id",
            &object_grant_grantee,
            "--object-id",
            &object_grant_table,
            "--privilege",
            "select",
            "--target-scope",
            "column",
            "--target-id",
            &object_grant_column,
        ])
        .output()?;
    assert!(
        revoke_object_cli.status.success(),
        "object-revoke CLI failed: {}",
        String::from_utf8_lossy(&revoke_object_cli.stderr)
    );
    assert!(
        String::from_utf8_lossy(&revoke_object_cli.stdout).contains("revoked object privilege")
    );
    let dropped_inline_reference_table = admin.drop_table(inline_reference_table.table_id)?;
    let dropped_unique = admin
        .drop_unique_constraint(unique_constraint.constraint_id)
        .map_err(|error| format!("process ADMIN drop_unique_constraint failed: {error}"))?;
    assert!(dropped_unique.commit_sequence > dropped_inline_reference_table.commit_sequence);
    assert!(dropped_unique.commit_sequence > unique_constraint.commit_sequence);
    let constraint_cli_address = address.to_string();
    let constraint_cli_credentials = admin_cancel_credentials_path
        .to_str()
        .ok_or("credential path is not UTF-8")?;
    let constraint_cli_table_id = encode_id_hex(&created_table.table_id);
    let constraint_cli_column_ids = encode_id_hex(&created_table.column_ids[0]);
    let create_unique_cli = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "unique-create",
            "--address",
            &constraint_cli_address,
            "--credentials",
            constraint_cli_credentials,
            "--table-id",
            &constraint_cli_table_id,
            "--columns",
            &constraint_cli_column_ids,
        ])
        .output()?;
    assert!(
        create_unique_cli.status.success(),
        "unique-create CLI failed: {}",
        String::from_utf8_lossy(&create_unique_cli.stderr)
    );
    let unique_cli_output = String::from_utf8_lossy(&create_unique_cli.stdout);
    assert!(unique_cli_output.contains("created unique constraint "));
    let unique_cli_id = unique_cli_output
        .split_whitespace()
        .nth(3)
        .ok_or("unique-create output omitted the generated ID")?;
    let drop_unique_cli = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "unique-drop",
            "--address",
            &constraint_cli_address,
            "--credentials",
            constraint_cli_credentials,
            "--constraint-id",
            unique_cli_id,
        ])
        .output()?;
    assert!(
        drop_unique_cli.status.success(),
        "unique-drop CLI failed: {}",
        String::from_utf8_lossy(&drop_unique_cli.stderr)
    );
    assert!(String::from_utf8_lossy(&drop_unique_cli.stdout).contains("dropped key constraint"));

    admin.alter_column_nullability(created_table.table_id, created_table.column_ids[0], false)?;
    let create_primary_key_cli = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "primary-key-create",
            "--address",
            &constraint_cli_address,
            "--credentials",
            constraint_cli_credentials,
            "--table-id",
            &constraint_cli_table_id,
            "--columns",
            &constraint_cli_column_ids,
        ])
        .output()?;
    assert!(
        create_primary_key_cli.status.success(),
        "primary-key-create CLI failed: {}",
        String::from_utf8_lossy(&create_primary_key_cli.stderr)
    );
    let primary_key_cli_output = String::from_utf8_lossy(&create_primary_key_cli.stdout);
    assert!(primary_key_cli_output.contains("created primary-key constraint "));
    let primary_key_cli_id = primary_key_cli_output
        .split_whitespace()
        .nth(3)
        .ok_or("primary-key-create output omitted the generated ID")?;
    let drop_primary_key_cli = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "primary-key-drop",
            "--address",
            &constraint_cli_address,
            "--credentials",
            constraint_cli_credentials,
            "--constraint-id",
            primary_key_cli_id,
        ])
        .output()?;
    assert!(
        drop_primary_key_cli.status.success(),
        "primary-key-drop CLI failed: {}",
        String::from_utf8_lossy(&drop_primary_key_cli.stderr)
    );
    assert!(
        String::from_utf8_lossy(&drop_primary_key_cli.stdout).contains("dropped key constraint")
    );
    admin.alter_column_nullability(created_table.table_id, created_table.column_ids[0], true)?;
    let check_expression = prdb_engine::authorization::Expr::Literal(Some(
        prdb_engine::tuple::ScalarValue::Boolean(true),
    ));
    let check_expression_hex = prdb_engine::expression::encode_expression(&check_expression)?
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let create_check_cli = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "check-create",
            "--address",
            &constraint_cli_address,
            "--credentials",
            constraint_cli_credentials,
            "--table-id",
            &constraint_cli_table_id,
            "--expression-eir1",
            &check_expression_hex,
            "--columns",
            "",
        ])
        .output()?;
    assert!(
        create_check_cli.status.success(),
        "check-create CLI failed: {}",
        String::from_utf8_lossy(&create_check_cli.stderr)
    );
    let check_cli_output = String::from_utf8_lossy(&create_check_cli.stdout);
    assert!(check_cli_output.contains("created CHECK constraint "));
    let check_cli_id = check_cli_output
        .split_whitespace()
        .nth(3)
        .ok_or("check-create output omitted the generated ID")?;
    let drop_check_cli = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "check-drop",
            "--address",
            &constraint_cli_address,
            "--credentials",
            constraint_cli_credentials,
            "--constraint-id",
            check_cli_id,
        ])
        .output()?;
    assert!(
        drop_check_cli.status.success(),
        "check-drop CLI failed: {}",
        String::from_utf8_lossy(&drop_check_cli.stderr)
    );
    assert!(String::from_utf8_lossy(&drop_check_cli.stdout).contains("dropped CHECK constraint"));
    let policy = admin.create_policy(AdminCreatePolicyRequest {
        table_id: created_table.table_id,
        name: "process_visible_rows",
        kind: AdminPolicyKind::Row,
        operation: AdminPolicyOperation::Select,
        effect: AdminPolicyEffect::Allow,
        recipients: &[AdminPolicyRecipient::User(admin_user_id)],
        target_column_ids: &[],
        using_expression: Some(&prdb_engine::authorization::Expr::Literal(Some(
            prdb_engine::tuple::ScalarValue::Boolean(true),
        ))),
        with_check_expression: None,
    })?;
    let listed_policies = admin.list_policies_page(
        created_schema.identity_id,
        created_table.table_id,
        128,
        None,
    )?;
    let listed_policy = listed_policies
        .records
        .iter()
        .find(|listed| listed.policy_id == policy.policy_id)
        .ok_or("administrative policy page omitted the created policy")?;
    assert_eq!(
        listed_policy.kind,
        prdb_engine::metadata_discovery::AdministrativePolicyKind::Row
    );
    assert_eq!(
        listed_policy.operation,
        prdb_engine::metadata_discovery::AdministrativePolicyOperation::Select
    );
    assert_eq!(
        listed_policy.effect,
        prdb_engine::metadata_discovery::AdministrativePolicyEffect::Allow
    );
    assert!(listed_policy.column_ids.is_empty());
    assert_eq!(
        listed_policy
            .recipients
            .as_ref()
            .and_then(|recipients| recipients.first())
            .map(|recipient| recipient.principal_id),
        Some(admin_user_id)
    );

    let schema_owner_policy = admin.create_policy(AdminCreatePolicyRequest {
        table_id: redacted_source_table.table_id,
        name: "schema_owner_policy_summary",
        kind: AdminPolicyKind::Row,
        operation: AdminPolicyOperation::Select,
        effect: AdminPolicyEffect::Allow,
        recipients: &[AdminPolicyRecipient::User(admin_user_id)],
        target_column_ids: &[],
        using_expression: Some(&prdb_engine::authorization::Expr::Literal(Some(
            prdb_engine::tuple::ScalarValue::Boolean(true),
        ))),
        with_check_expression: None,
    })?;
    admin.set_schema_owner(
        redacted_source_schema.identity_id,
        transfer_owner.identity_id,
    )?;
    let owner_policy_credentials = read_client_credentials(&transfer_credential_path)?;
    let mut owner_policy_secret = owner_policy_credentials.token_secret;
    let mut owner_policy_client = AdminClient::connect_authenticated(
        address,
        owner_policy_credentials.token_id,
        &mut owner_policy_secret,
    )?;
    drop(owner_policy_credentials);
    let owner_policies = owner_policy_client.list_policies_page(
        redacted_source_schema.identity_id,
        redacted_source_table.table_id,
        128,
        None,
    )?;
    let owner_policy = owner_policies
        .records
        .iter()
        .find(|listed| listed.policy_id == schema_owner_policy.policy_id)
        .ok_or("schema-owner policy page omitted the created policy")?;
    assert!(owner_policy.recipients.is_none());
    let owner_dependencies = owner_policy_client.list_dependencies_page(
        redacted_foreign_key.constraint_id,
        128,
        None,
    )?;
    assert!(owner_dependencies.records.is_empty());
    drop(owner_policy_client);
    admin.set_schema_owner(redacted_source_schema.identity_id, admin_user_id)?;
    admin.drop_policy(schema_owner_policy.policy_id)?;
    let dropped_policy = admin.drop_policy(policy.policy_id)?;
    assert!(dropped_policy.commit_sequence > policy.commit_sequence);
    let cli_policy_true = prdb_engine::authorization::Expr::Literal(Some(
        prdb_engine::tuple::ScalarValue::Boolean(true),
    ));
    let cli_policy_expression = prdb_engine::expression::encode_expression(&cli_policy_true)?
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let policy_cli_address = address.to_string();
    let policy_cli_credentials = admin_cancel_credentials_path
        .to_str()
        .ok_or("credential path is not UTF-8")?;
    let policy_cli_table_id = encode_id_hex(&created_table.table_id);
    let policy_cli_recipient = format!("user:{}", encode_id_hex(&admin_user_id));
    let create_policy_cli = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "policy-create",
            "--address",
            &policy_cli_address,
            "--credentials",
            policy_cli_credentials,
            "--table-id",
            &policy_cli_table_id,
            "--name",
            "process_cli_policy",
            "--kind",
            "row",
            "--operation",
            "select",
            "--effect",
            "allow",
            "--recipients",
            &policy_cli_recipient,
            "--using-expression-eir1",
            &cli_policy_expression,
        ])
        .output()?;
    assert!(
        create_policy_cli.status.success(),
        "policy-create CLI failed: {}",
        String::from_utf8_lossy(&create_policy_cli.stderr)
    );
    let create_policy_output = String::from_utf8_lossy(&create_policy_cli.stdout);
    assert!(create_policy_output.contains("created policy "));
    let cli_policy_id = create_policy_output
        .split_whitespace()
        .nth(2)
        .ok_or("policy-create output omitted the generated ID")?;
    let drop_policy_cli = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "policy-drop",
            "--address",
            &policy_cli_address,
            "--credentials",
            policy_cli_credentials,
            "--policy-id",
            cli_policy_id,
        ])
        .output()?;
    assert!(
        drop_policy_cli.status.success(),
        "policy-drop CLI failed: {}",
        String::from_utf8_lossy(&drop_policy_cli.stderr)
    );
    assert!(String::from_utf8_lossy(&drop_policy_cli.stdout).contains("dropped policy"));
    let cell_policy = admin.create_policy(AdminCreatePolicyRequest {
        table_id: created_table.table_id,
        name: "process_visible_cells",
        kind: AdminPolicyKind::Cell,
        operation: AdminPolicyOperation::Select,
        effect: AdminPolicyEffect::Allow,
        recipients: &[AdminPolicyRecipient::User(admin_user_id)],
        target_column_ids: &[added_column.column_id],
        using_expression: Some(&prdb_engine::authorization::Expr::Literal(Some(
            prdb_engine::tuple::ScalarValue::Boolean(true),
        ))),
        with_check_expression: None,
    })?;
    let dropped_cell_policy = admin.drop_policy(cell_policy.policy_id)?;
    assert!(dropped_cell_policy.commit_sequence > cell_policy.commit_sequence);
    let policy_true = prdb_engine::authorization::Expr::Literal(Some(
        prdb_engine::tuple::ScalarValue::Boolean(true),
    ));
    let cell_insert_policy = admin.create_policy(AdminCreatePolicyRequest {
        table_id: created_table.table_id,
        name: "process_insertable_cells",
        kind: AdminPolicyKind::Cell,
        operation: AdminPolicyOperation::Insert,
        effect: AdminPolicyEffect::Allow,
        recipients: &[AdminPolicyRecipient::User(admin_user_id)],
        target_column_ids: &[added_column.column_id],
        using_expression: None,
        with_check_expression: Some(&policy_true),
    })?;
    let dropped_cell_insert_policy = admin.drop_policy(cell_insert_policy.policy_id)?;
    assert!(dropped_cell_insert_policy.commit_sequence > cell_insert_policy.commit_sequence);
    let cell_update_policy = admin.create_policy(AdminCreatePolicyRequest {
        table_id: created_table.table_id,
        name: "process_editable_cells",
        kind: AdminPolicyKind::Cell,
        operation: AdminPolicyOperation::Update,
        effect: AdminPolicyEffect::Allow,
        recipients: &[AdminPolicyRecipient::User(admin_user_id)],
        target_column_ids: &[added_column.column_id],
        using_expression: Some(&policy_true),
        with_check_expression: Some(&policy_true),
    })?;
    let dropped_cell_update_policy = admin.drop_policy(cell_update_policy.policy_id)?;
    assert!(dropped_cell_update_policy.commit_sequence > cell_update_policy.commit_sequence);
    for (name, operation, using_expression, with_check_expression) in [
        (
            "process_insertable_rows",
            AdminPolicyOperation::Insert,
            None,
            Some(&policy_true),
        ),
        (
            "process_updatable_rows",
            AdminPolicyOperation::Update,
            Some(&policy_true),
            Some(&policy_true),
        ),
        (
            "process_deletable_rows",
            AdminPolicyOperation::Delete,
            Some(&policy_true),
            None,
        ),
    ] {
        let row_policy = admin.create_policy(AdminCreatePolicyRequest {
            table_id: created_table.table_id,
            name,
            kind: AdminPolicyKind::Row,
            operation,
            effect: AdminPolicyEffect::Allow,
            recipients: &[AdminPolicyRecipient::User(admin_user_id)],
            target_column_ids: &[],
            using_expression,
            with_check_expression,
        })?;
        let dropped_row_policy = admin.drop_policy(row_policy.policy_id)?;
        assert!(dropped_row_policy.commit_sequence > row_policy.commit_sequence);
    }
    let renamed_column =
        admin.rename_column(created_table.table_id, added_column.column_id, "extra_v2")?;
    let dropped_column = admin.drop_column(created_table.table_id, added_column.column_id)?;
    assert!(dropped_column.commit_sequence > renamed_column.commit_sequence);
    let column_cli_address = address.to_string();
    let column_cli_credentials = admin_cancel_credentials_path
        .to_str()
        .ok_or("credential path is not UTF-8")?;
    let column_cli_table_id = encode_id_hex(&created_table.table_id);
    let column_cli_column_id = encode_id_hex(&created_table.column_ids[0]);
    for nullable in ["false", "true"] {
        let nullability_cli = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
            .args([
                "column-nullability",
                "--address",
                &column_cli_address,
                "--credentials",
                column_cli_credentials,
                "--table-id",
                &column_cli_table_id,
                "--column-id",
                &column_cli_column_id,
                "--nullable",
                nullable,
            ])
            .output()?;
        assert!(
            nullability_cli.status.success(),
            "column-nullability CLI ({nullable}) failed: {}",
            String::from_utf8_lossy(&nullability_cli.stderr)
        );
        assert!(
            String::from_utf8_lossy(&nullability_cli.stdout).contains("changed column nullability")
        );
    }
    let set_default_cli = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "column-default-set",
            "--address",
            &column_cli_address,
            "--credentials",
            column_cli_credentials,
            "--table-id",
            &column_cli_table_id,
            "--column-id",
            &column_cli_column_id,
            "--type",
            "text",
            "--value",
            "seed",
        ])
        .output()?;
    assert!(
        set_default_cli.status.success(),
        "column-default-set CLI failed: {}",
        String::from_utf8_lossy(&set_default_cli.stderr)
    );
    assert!(String::from_utf8_lossy(&set_default_cli.stdout).contains("set column default"));
    let drop_default_cli = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "column-default-drop",
            "--address",
            &column_cli_address,
            "--credentials",
            column_cli_credentials,
            "--table-id",
            &column_cli_table_id,
            "--column-id",
            &column_cli_column_id,
        ])
        .output()?;
    assert!(
        drop_default_cli.status.success(),
        "column-default-drop CLI failed: {}",
        String::from_utf8_lossy(&drop_default_cli.stderr)
    );
    assert!(String::from_utf8_lossy(&drop_default_cli.stdout).contains("dropped column default"));
    let created_index = admin
        .create_index(
            created_table.table_id,
            "process_admin_index",
            &created_table.column_ids,
            false,
        )
        .map_err(|error| format!("process ADMIN create_index failed: {error}"))?;
    let create_index_sql_path = root.join("create-index.sql");
    std::fs::write(
        &create_index_sql_path,
        "CREATE INDEX process_admin_sql_index ON process_admin_schema.process_admin_table (payload);",
    )?;
    let create_index_sql = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address_text,
            "--credentials",
            admin_cancel_credentials_path
                .to_str()
                .ok_or("credential path is not UTF-8")?,
            "--sql-file",
            create_index_sql_path
                .to_str()
                .ok_or("SQL path is not UTF-8")?,
        ])
        .output()?;
    assert!(
        create_index_sql.status.success(),
        "CREATE INDEX SQL failed: {}",
        String::from_utf8_lossy(&create_index_sql.stderr)
    );
    assert!(String::from_utf8_lossy(&create_index_sql.stdout).contains("created index "));
    let original_sql_index = admin
        .list_indexes_page(created_schema.identity_id, created_table.table_id, 64, None)?
        .records
        .into_iter()
        .find(|index| index.name == "process_admin_sql_index")
        .ok_or("created index is missing from metadata")?;
    let rename_index_sql_path = root.join("rename-index.sql");
    std::fs::write(
        &rename_index_sql_path,
        "ALTER INDEX process_admin_schema.process_admin_sql_index RENAME TO process_admin_renamed_sql_index;",
    )?;
    let rename_index_sql = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address_text,
            "--credentials",
            admin_cancel_credentials_path
                .to_str()
                .ok_or("credential path is not UTF-8")?,
            "--sql-file",
            rename_index_sql_path
                .to_str()
                .ok_or("SQL path is not UTF-8")?,
        ])
        .output()?;
    assert!(
        rename_index_sql.status.success(),
        "ALTER INDEX RENAME SQL failed: {}",
        String::from_utf8_lossy(&rename_index_sql.stderr)
    );
    assert!(String::from_utf8_lossy(&rename_index_sql.stdout).contains("renamed index"));
    let renamed_index = admin
        .list_indexes_page(created_schema.identity_id, created_table.table_id, 64, None)?
        .records
        .into_iter()
        .find(|index| index.name == "process_admin_renamed_sql_index")
        .ok_or("renamed index is missing from metadata")?;
    assert_eq!(renamed_index.name, "process_admin_renamed_sql_index");
    assert_eq!(renamed_index.index_id, original_sql_index.index_id);
    let duplicate_rename_index_sql_path = root.join("rename-index-collision.sql");
    std::fs::write(
        &duplicate_rename_index_sql_path,
        "ALTER INDEX process_admin_schema.process_admin_renamed_sql_index RENAME TO process_admin_index;",
    )?;
    let duplicate_rename_index_sql = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address_text,
            "--credentials",
            admin_cancel_credentials_path
                .to_str()
                .ok_or("credential path is not UTF-8")?,
            "--sql-file",
            duplicate_rename_index_sql_path
                .to_str()
                .ok_or("SQL path is not UTF-8")?,
        ])
        .output()?;
    assert!(!duplicate_rename_index_sql.status.success());
    assert!(
        admin
            .list_indexes_page(created_schema.identity_id, created_table.table_id, 64, None)?
            .records
            .iter()
            .any(|index| index.index_id == original_sql_index.index_id
                && index.name == "process_admin_renamed_sql_index")
    );
    let alter_index_sql_path = root.join("alter-index.sql");
    std::fs::write(
        &alter_index_sql_path,
        "ALTER INDEX process_admin_schema.process_admin_index REBUILD USING BTREE NONUNIQUE (payload);",
    )?;
    let alter_index_sql = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address_text,
            "--credentials",
            admin_cancel_credentials_path
                .to_str()
                .ok_or("credential path is not UTF-8")?,
            "--sql-file",
            alter_index_sql_path
                .to_str()
                .ok_or("SQL path is not UTF-8")?,
        ])
        .output()?;
    assert!(
        alter_index_sql.status.success(),
        "ALTER INDEX REBUILD SQL failed: {}",
        String::from_utf8_lossy(&alter_index_sql.stderr)
    );
    assert!(String::from_utf8_lossy(&alter_index_sql.stdout).contains("rebuilt index definition"));
    let payload_column_id = admin
        .list_columns_page(created_schema.identity_id, created_table.table_id, 64, None)?
        .records
        .into_iter()
        .find(|column| column.name == "payload")
        .ok_or("ALTER INDEX SQL fixture column is missing")?
        .column_id;
    let altered_index = admin
        .list_indexes_page(created_schema.identity_id, created_table.table_id, 64, None)?
        .records
        .into_iter()
        .find(|index| index.index_id == created_index.index_id)
        .ok_or("ALTER INDEX SQL fixture index is missing")?;
    assert_eq!(altered_index.column_ids, [payload_column_id]);
    assert!(!altered_index.unique);
    let rebuild_index_sql_path = root.join("rebuild-index.sql");
    std::fs::write(
        &rebuild_index_sql_path,
        "REBUILD INDEX process_admin_schema.process_admin_index;",
    )?;
    let rebuild_index_sql = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address_text,
            "--credentials",
            admin_cancel_credentials_path
                .to_str()
                .ok_or("credential path is not UTF-8")?,
            "--sql-file",
            rebuild_index_sql_path
                .to_str()
                .ok_or("SQL path is not UTF-8")?,
        ])
        .output()?;
    assert!(
        rebuild_index_sql.status.success(),
        "REBUILD INDEX SQL failed: {}",
        String::from_utf8_lossy(&rebuild_index_sql.stderr)
    );
    assert!(String::from_utf8_lossy(&rebuild_index_sql.stdout).contains("rebuilt index"));
    let drop_index_sql_path = root.join("drop-index.sql");
    std::fs::write(
        &drop_index_sql_path,
        "DROP INDEX process_admin_schema.process_admin_renamed_sql_index RESTRICT;",
    )?;
    let drop_index_sql = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address_text,
            "--credentials",
            admin_cancel_credentials_path
                .to_str()
                .ok_or("credential path is not UTF-8")?,
            "--sql-file",
            drop_index_sql_path
                .to_str()
                .ok_or("SQL path is not UTF-8")?,
        ])
        .output()?;
    assert!(
        drop_index_sql.status.success(),
        "DROP INDEX SQL failed: {}",
        String::from_utf8_lossy(&drop_index_sql.stderr)
    );
    assert!(String::from_utf8_lossy(&drop_index_sql.stdout).contains("dropped index"));
    let rebuilt_index = admin
        .rebuild_index(created_index.index_id, &created_table.column_ids, true)
        .map_err(|error| format!("process ADMIN rebuild_index failed: {error}"))?;
    let second_index = admin
        .create_index(
            created_table.table_id,
            "process_admin_index_second",
            &created_table.column_ids,
            false,
        )
        .map_err(|error| format!("create second metadata index: {error:?}"))?;
    let cursor_credentials = read_client_credentials(&admin_cancel_credentials_path)?;
    let mut cursor_secret = cursor_credentials.token_secret;
    let mut cursor_admin = AdminClient::connect_authenticated(
        address,
        cursor_credentials.token_id,
        &mut cursor_secret,
    )?;
    drop(cursor_credentials);
    let first_index_page = cursor_admin
        .list_indexes_page(created_schema.identity_id, created_table.table_id, 1, None)
        .map_err(|error| format!("first index page: {error:?}"))?;
    let stale_index_cursor = first_index_page
        .next_cursor
        .ok_or("expected another index page before dropping an index")?;
    admin
        .drop_index(second_index.index_id)
        .map_err(|error| format!("drop second metadata index: {error:?}"))?;
    let stale_page = cursor_admin.list_indexes_page(
        created_schema.identity_id,
        created_table.table_id,
        1,
        Some(&stale_index_cursor),
    );
    assert!(matches!(
        stale_page,
        Err(prdb_engine::client_submit::ClientSubmitError::ServerRejected(_))
    ));
    drop(cursor_admin);
    let listed_indexes = admin
        .list_indexes_page(
            created_schema.identity_id,
            created_table.table_id,
            128,
            None,
        )
        .map_err(|error| format!("complete index page after stale cursor: {error:?}"))?;
    let listed_index = listed_indexes
        .records
        .iter()
        .find(|index| index.index_id == created_index.index_id)
        .ok_or_else(|| {
            format!(
                "administrative index page omitted {} from {:?}",
                encode_id_hex(&created_index.index_id),
                listed_indexes.records
            )
        })?;
    assert_eq!(listed_index.column_ids, created_table.column_ids);
    assert!(listed_index.unique);
    let repeated_rebuild = admin
        .rebuild_index(created_index.index_id, &created_table.column_ids, true)
        .map_err(|error| format!("process ADMIN repeated rebuild failed: {error}"))?;
    let dropped_index = admin
        .drop_index(created_index.index_id)
        .map_err(|error| format!("process ADMIN drop_index failed: {error}"))?;
    assert!(repeated_rebuild.commit_sequence > rebuilt_index.commit_sequence);
    assert!(dropped_index.commit_sequence > repeated_rebuild.commit_sequence);
    let index_cli_address = address.to_string();
    let index_cli_credentials = admin_cancel_credentials_path
        .to_str()
        .ok_or("credential path is not UTF-8")?;
    let index_cli_table_id = encode_id_hex(&created_table.table_id);
    let index_cli_column_ids = encode_id_hex(&created_table.column_ids[0]);
    let create_index_cli = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "index-create",
            "--address",
            &index_cli_address,
            "--credentials",
            index_cli_credentials,
            "--table-id",
            &index_cli_table_id,
            "--name",
            "process_cli_index",
            "--columns",
            &index_cli_column_ids,
            "--unique",
            "true",
        ])
        .output()?;
    assert!(
        create_index_cli.status.success(),
        "index-create CLI failed: {}",
        String::from_utf8_lossy(&create_index_cli.stderr)
    );
    let create_index_output = String::from_utf8_lossy(&create_index_cli.stdout);
    assert!(create_index_output.contains("created index "));
    let cli_index_id = create_index_output
        .split_whitespace()
        .nth(2)
        .ok_or("index-create output omitted the generated ID")?;
    assert_eq!(cli_index_id.len(), 32);
    let rebuild_index_cli = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "index-rebuild",
            "--address",
            &index_cli_address,
            "--credentials",
            index_cli_credentials,
            "--index-id",
            cli_index_id,
            "--columns",
            &index_cli_column_ids,
            "--unique",
            "false",
        ])
        .output()?;
    assert!(
        rebuild_index_cli.status.success(),
        "index-rebuild CLI failed: {}",
        String::from_utf8_lossy(&rebuild_index_cli.stderr)
    );
    assert!(String::from_utf8_lossy(&rebuild_index_cli.stdout).contains("rebuilt index"));
    let drop_index_cli = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "index-drop",
            "--address",
            &index_cli_address,
            "--credentials",
            index_cli_credentials,
            "--index-id",
            cli_index_id,
        ])
        .output()?;
    assert!(
        drop_index_cli.status.success(),
        "index-drop CLI failed: {}",
        String::from_utf8_lossy(&drop_index_cli.stderr)
    );
    assert!(String::from_utf8_lossy(&drop_index_cli.stdout).contains("dropped index"));
    let policy_role = admin.create_role("process_policy_role")?;
    admin.grant_object_privilege(
        transfer_owner.identity_id,
        created_schema.identity_id,
        AdminObjectPrivilege::Usage,
        AdminObjectGrantTarget::Object,
    )?;
    admin.set_table_owner(created_table.table_id, transfer_owner.identity_id)?;
    let pre_policy_ids = admin
        .list_policies_page(created_schema.identity_id, created_table.table_id, 64, None)?
        .records
        .into_iter()
        .map(|policy| policy.policy_id)
        .collect::<std::collections::HashSet<_>>();
    let create_policy_sql_path = root.join("create-row-policy.sql");
    std::fs::write(
        &create_policy_sql_path,
        "CREATE ROW POLICY process_visible ON process_admin_schema.process_admin_table FOR SELECT TO USER metadata_process_admin AS ALLOW USING (payload = 'visible' AND HAS_ROLE('process_policy_role'));",
    )?;
    let create_policy_sql = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address_text,
            "--credentials",
            transfer_credential_path
                .to_str()
                .ok_or("credential path is not UTF-8")?,
            "--sql-file",
            create_policy_sql_path
                .to_str()
                .ok_or("SQL path is not UTF-8")?,
        ])
        .output()?;
    assert!(
        create_policy_sql.status.success(),
        "CREATE POLICY SQL failed: {}",
        String::from_utf8_lossy(&create_policy_sql.stderr)
    );
    assert!(String::from_utf8_lossy(&create_policy_sql.stdout).contains("created policy"));
    let create_cell_policy_sql_path = root.join("create-cell-policy.sql");
    std::fs::write(
        &create_cell_policy_sql_path,
        "CREATE CELL POLICY process_payload ON process_admin_schema.process_admin_table (payload) FOR SELECT TO ROLE process_policy_role AS RESTRICT USING (HAS_ROLE('process_policy_role'));",
    )?;
    let create_cell_policy_sql = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address_text,
            "--credentials",
            transfer_credential_path
                .to_str()
                .ok_or("credential path is not UTF-8")?,
            "--sql-file",
            create_cell_policy_sql_path
                .to_str()
                .ok_or("SQL path is not UTF-8")?,
        ])
        .output()?;
    assert!(
        create_cell_policy_sql.status.success(),
        "CREATE CELL POLICY SQL failed: {}",
        String::from_utf8_lossy(&create_cell_policy_sql.stderr)
    );
    assert!(String::from_utf8_lossy(&create_cell_policy_sql.stdout).contains("created policy"));
    let policy_list_schema = encode_id_hex(&created_schema.identity_id);
    let policy_list_table = encode_id_hex(&created_table.table_id);
    let policy_list_cli = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "policy-list",
            "--address",
            &address_text,
            "--credentials",
            admin_cancel_credentials_path
                .to_str()
                .ok_or("policy-list credentials path is not UTF-8")?,
            "--schema-id",
            &policy_list_schema,
            "--table-id",
            &policy_list_table,
        ])
        .output()?;
    assert!(
        policy_list_cli.status.success(),
        "policy-list CLI failed: {}",
        String::from_utf8_lossy(&policy_list_cli.stderr)
    );
    let policy_list_json = String::from_utf8_lossy(&policy_list_cli.stdout);
    assert!(
        policy_list_json.contains("\"kind\":\"row\""),
        "policy-list omitted row policy summary: {policy_list_json}"
    );
    assert!(policy_list_json.contains("\"kind\":\"cell\""));
    assert!(policy_list_json.contains(&encode_id_hex(&admin_user_id)));
    assert!(policy_list_json.contains(&encode_id_hex(&policy_role.identity_id)));
    assert!(!policy_list_json.contains("HAS_ROLE"));
    let original_policy_ids = admin
        .list_policies_page(created_schema.identity_id, created_table.table_id, 64, None)?
        .records
        .into_iter()
        .map(|policy| policy.policy_id)
        .collect::<std::collections::HashSet<_>>();
    let replace_policy_sql_path = root.join("replace-row-policy.sql");
    std::fs::write(
        &replace_policy_sql_path,
        "CREATE OR REPLACE CELL POLICY process_visible ON process_admin_schema.process_admin_table (payload) FOR SELECT TO USER metadata_process_admin AS ALLOW USING (HAS_ROLE('process_policy_role'));",
    )?;
    let replace_policy_sql = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address_text,
            "--credentials",
            transfer_credential_path
                .to_str()
                .ok_or("credential path is not UTF-8")?,
            "--sql-file",
            replace_policy_sql_path
                .to_str()
                .ok_or("SQL path is not UTF-8")?,
        ])
        .output()?;
    assert!(
        replace_policy_sql.status.success(),
        "CREATE OR REPLACE POLICY SQL failed: {}",
        String::from_utf8_lossy(&replace_policy_sql.stderr)
    );
    assert!(String::from_utf8_lossy(&replace_policy_sql.stdout).contains("created policy"));
    let replaced_policy_ids = admin
        .list_policies_page(created_schema.identity_id, created_table.table_id, 64, None)?
        .records
        .into_iter()
        .map(|policy| policy.policy_id)
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(replaced_policy_ids.len(), original_policy_ids.len());
    assert_eq!(
        replaced_policy_ids
            .intersection(&original_policy_ids)
            .count(),
        original_policy_ids.len() - 1,
        "replacement must remove exactly the old definition and publish its successor"
    );
    let created_policy_ids = admin
        .list_policies_page(created_schema.identity_id, created_table.table_id, 64, None)?
        .records
        .into_iter()
        .map(|policy| policy.policy_id)
        .filter(|policy_id| !pre_policy_ids.contains(policy_id))
        .collect::<Vec<_>>();
    assert_eq!(created_policy_ids.len(), 2);
    for (filename, sql) in [
        (
            "drop-row-policy.sql",
            "DROP CELL POLICY process_visible ON process_admin_schema.process_admin_table RESTRICT;",
        ),
        (
            "drop-cell-policy.sql",
            "DROP CELL POLICY process_payload ON process_admin_schema.process_admin_table RESTRICT;",
        ),
    ] {
        let drop_policy_sql_path = root.join(filename);
        std::fs::write(&drop_policy_sql_path, sql)?;
        let drop_policy_sql = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
            .args([
                "admin-sql",
                "--address",
                &address_text,
                "--credentials",
                transfer_credential_path
                    .to_str()
                    .ok_or("credential path is not UTF-8")?,
                "--sql-file",
                drop_policy_sql_path
                    .to_str()
                    .ok_or("SQL path is not UTF-8")?,
            ])
            .output()?;
        assert!(
            drop_policy_sql.status.success(),
            "DROP POLICY SQL failed: {}",
            String::from_utf8_lossy(&drop_policy_sql.stderr)
        );
        assert!(String::from_utf8_lossy(&drop_policy_sql.stdout).contains("dropped "));
    }
    assert!(admin
        .execute_drop_policy_if_exists_sql(
            "DROP ROW POLICY IF EXISTS process_missing ON process_admin_schema.process_admin_table RESTRICT"
        )
        .map_err(|error| format!("conditional missing policy drop: {error:?}"))?
        .is_none());
    assert!(admin
        .execute_drop_policy_if_exists_sql(
            "DROP ROW POLICY IF EXISTS process_missing ON process_admin_schema.process_admin_table RESTRICT"
        )
        .map_err(|error| format!("repeated conditional missing policy drop: {error:?}"))?
        .is_none());
    admin.set_table_owner(created_table.table_id, admin_user_id)?;
    admin.grant_object_privilege(
        transfer_owner.identity_id,
        created_table.table_id,
        AdminObjectPrivilege::ManagePolicies,
        AdminObjectGrantTarget::Object,
    )?;
    let mut manager_metadata_credentials = read_client_credentials(&transfer_credential_path)?;
    let mut manager_metadata = MetadataClient::connect_authenticated(
        address,
        manager_metadata_credentials.token_id,
        &mut manager_metadata_credentials.token_secret,
    )?;
    assert!(
        manager_metadata
            .list_relations(created_schema.identity_id)?
            .iter()
            .any(|relation| relation.relation_id == created_table.table_id)
    );
    drop(manager_metadata);
    drop(manager_metadata_credentials);
    let managed_policy_sql_path = root.join("create-managed-policy.sql");
    std::fs::write(
        &managed_policy_sql_path,
        "CREATE ROW POLICY process_managed ON process_admin_schema.process_admin_table FOR SELECT TO ROLE process_policy_role AS ALLOW USING (HAS_ROLE('process_policy_role'));",
    )?;
    let create_managed_policy_sql = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address_text,
            "--credentials",
            transfer_credential_path
                .to_str()
                .ok_or("credential path is not UTF-8")?,
            "--sql-file",
            managed_policy_sql_path
                .to_str()
                .ok_or("SQL path is not UTF-8")?,
        ])
        .output()?;
    assert!(
        create_managed_policy_sql.status.success(),
        "CREATE POLICY as MANAGE_POLICIES holder failed: {}",
        String::from_utf8_lossy(&create_managed_policy_sql.stderr)
    );
    let drop_managed_policy_sql_path = root.join("drop-managed-policy.sql");
    std::fs::write(
        &drop_managed_policy_sql_path,
        "DROP ROW POLICY process_managed ON process_admin_schema.process_admin_table RESTRICT;",
    )?;
    let drop_managed_policy_sql = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address_text,
            "--credentials",
            transfer_credential_path
                .to_str()
                .ok_or("credential path is not UTF-8")?,
            "--sql-file",
            drop_managed_policy_sql_path
                .to_str()
                .ok_or("SQL path is not UTF-8")?,
        ])
        .output()?;
    assert!(
        drop_managed_policy_sql.status.success(),
        "DROP POLICY as MANAGE_POLICIES holder failed: {}",
        String::from_utf8_lossy(&drop_managed_policy_sql.stderr)
    );
    let cursor_policy_one = admin.execute_create_policy_sql(
        "CREATE ROW POLICY process_cursor_policy_one ON process_admin_schema.process_admin_table FOR SELECT TO ROLE process_policy_role AS ALLOW USING (HAS_ROLE('process_policy_role'))",
    )?;
    let cursor_policy_two = admin.execute_create_policy_sql(
        "CREATE ROW POLICY process_cursor_policy_two ON process_admin_schema.process_admin_table FOR SELECT TO ROLE process_policy_role AS ALLOW USING (HAS_ROLE('process_policy_role'))",
    )?;
    for privilege in [
        prdb_engine::client_submit::AdminObjectPrivilege::Select,
        prdb_engine::client_submit::AdminObjectPrivilege::Update,
    ] {
        admin.grant_object_privilege(
            policy_role.identity_id,
            created_table.table_id,
            privilege,
            prdb_engine::client_submit::AdminObjectGrantTarget::Object,
        )?;
    }
    let revoked_index_one = admin.create_index(
        created_table.table_id,
        "process_revoked_cursor_index_one",
        &created_table.column_ids,
        false,
    )?;
    let revoked_index_two = admin.create_index(
        created_table.table_id,
        "process_revoked_cursor_index_two",
        &created_table.column_ids,
        false,
    )?;
    admin.set_schema_owner(created_schema.identity_id, transfer_owner.identity_id)?;
    let mut cursor_owner_credentials = read_client_credentials(&transfer_credential_path)?;
    let mut cursor_owner = AdminClient::connect_authenticated(
        address,
        cursor_owner_credentials.token_id,
        &mut cursor_owner_credentials.token_secret,
    )?;
    let first_policy_page = cursor_owner.list_policies_page(
        created_schema.identity_id,
        created_table.table_id,
        1,
        None,
    )?;
    assert_eq!(first_policy_page.records.len(), 1);
    let revoked_policy_cursor = first_policy_page
        .next_cursor
        .ok_or("expected a policy cursor before schema ownership is transferred")?;
    let mut index_cursor_owner_credentials = read_client_credentials(&transfer_credential_path)?;
    let mut index_cursor_owner = AdminClient::connect_authenticated(
        address,
        index_cursor_owner_credentials.token_id,
        &mut index_cursor_owner_credentials.token_secret,
    )?;
    let first_index_page = index_cursor_owner.list_indexes_page(
        created_schema.identity_id,
        created_table.table_id,
        1,
        None,
    )?;
    assert_eq!(first_index_page.records.len(), 1);
    let revoked_index_cursor = first_index_page
        .next_cursor
        .ok_or("expected an index cursor before schema ownership is transferred")?;
    let mut dependency_cursor_owner_credentials =
        read_client_credentials(&transfer_credential_path)?;
    let mut dependency_cursor_owner = AdminClient::connect_authenticated(
        address,
        dependency_cursor_owner_credentials.token_id,
        &mut dependency_cursor_owner_credentials.token_secret,
    )?;
    let first_dependency_page =
        dependency_cursor_owner.list_dependencies_page(created_table.table_id, 1, None)?;
    assert_eq!(first_dependency_page.records.len(), 1);
    let revoked_dependency_cursor = first_dependency_page
        .next_cursor
        .ok_or("expected a dependency cursor before schema ownership is transferred")?;
    let mut grant_cursor_owner_credentials = read_client_credentials(&transfer_credential_path)?;
    let mut grant_cursor_owner = AdminClient::connect_authenticated(
        address,
        grant_cursor_owner_credentials.token_id,
        &mut grant_cursor_owner_credentials.token_secret,
    )?;
    let first_grant_page = grant_cursor_owner.list_grants_page(
        created_schema.identity_id,
        created_table.table_id,
        1,
        None,
    )?;
    assert_eq!(first_grant_page.records.len(), 1);
    let revoked_grant_cursor = first_grant_page
        .next_cursor
        .ok_or("expected a grant cursor before schema ownership is transferred")?;
    admin.set_schema_owner(created_schema.identity_id, admin_user_id)?;
    let invalidated_grant_page = grant_cursor_owner.list_grants_page(
        created_schema.identity_id,
        created_table.table_id,
        1,
        Some(&revoked_grant_cursor),
    );
    assert!(
        matches!(
            &invalidated_grant_page,
            Err(prdb_engine::client_submit::ClientSubmitError::ServerRejected(_))
        ),
        "revoked grant cursor result: {invalidated_grant_page:?}"
    );
    let invalidated_index_page = index_cursor_owner.list_indexes_page(
        created_schema.identity_id,
        created_table.table_id,
        1,
        Some(&revoked_index_cursor),
    );
    assert!(
        matches!(
            &invalidated_index_page,
            Err(prdb_engine::client_submit::ClientSubmitError::ServerRejected(_))
        ),
        "revoked index cursor result: {invalidated_index_page:?}"
    );
    assert!(matches!(
        cursor_owner.list_policies_page(
            created_schema.identity_id,
            created_table.table_id,
            1,
            Some(&revoked_policy_cursor),
        ),
        Err(prdb_engine::client_submit::ClientSubmitError::ServerRejected(_))
    ));
    let invalidated_dependency_page = dependency_cursor_owner.list_dependencies_page(
        created_table.table_id,
        1,
        Some(&revoked_dependency_cursor),
    );
    assert!(matches!(
        invalidated_dependency_page,
        Err(prdb_engine::client_submit::ClientSubmitError::ServerRejected(_))
    ));
    drop(grant_cursor_owner);
    drop(grant_cursor_owner_credentials);
    drop(index_cursor_owner);
    drop(index_cursor_owner_credentials);
    drop(dependency_cursor_owner);
    drop(dependency_cursor_owner_credentials);
    drop(cursor_owner);
    drop(cursor_owner_credentials);

    admin.drop_index(revoked_index_one.index_id)?;
    admin.drop_index(revoked_index_two.index_id)?;

    // MANAGE_POLICIES authorizes DDL but does not disclose administrative
    // policy metadata after schema ownership is returned.
    let mut manager_cursor_credentials = read_client_credentials(&transfer_credential_path)?;
    let mut manager_cursor = AdminClient::connect_authenticated(
        address,
        manager_cursor_credentials.token_id,
        &mut manager_cursor_credentials.token_secret,
    )?;
    let manager_policy_page = manager_cursor.list_policies_page(
        created_schema.identity_id,
        created_table.table_id,
        1,
        None,
    )?;
    assert!(manager_policy_page.records.is_empty());
    assert!(manager_policy_page.next_cursor.is_none());
    drop(manager_cursor);
    drop(manager_cursor_credentials);
    for privilege in [
        prdb_engine::client_submit::AdminObjectPrivilege::Select,
        prdb_engine::client_submit::AdminObjectPrivilege::Update,
    ] {
        admin.revoke_object_privilege(
            policy_role.identity_id,
            created_table.table_id,
            privilege,
            prdb_engine::client_submit::AdminObjectGrantTarget::Object,
        )?;
    }
    admin.drop_policy(cursor_policy_one.policy_id)?;
    admin.drop_policy(cursor_policy_two.policy_id)?;
    assert!(
        admin
            .list_policies_page(created_schema.identity_id, created_table.table_id, 64, None)?
            .records
            .into_iter()
            .all(|policy| pre_policy_ids.contains(&policy.policy_id))
    );
    admin.drop_role(policy_role.identity_id)?;
    admin.revoke_object_privilege(
        transfer_owner.identity_id,
        created_schema.identity_id,
        AdminObjectPrivilege::Usage,
        AdminObjectGrantTarget::Object,
    )?;
    let renamed_table = admin.rename_table(created_table.table_id, "process_admin_table_v2")?;
    let dropped_source_table = admin.drop_table(source_table.table_id)?;
    let dropped_table = admin.drop_table(created_table.table_id)?;
    let created_by_grantee_id = admin
        .list_relations_page(created_schema.identity_id, 128, None)?
        .records
        .into_iter()
        .find(|relation| relation.name == "process_created_by_grantee")
        .map(|relation| relation.relation_id)
        .ok_or("CREATE grantee table was not visible during cleanup")?;
    let dropped_by_grantee_table = admin.drop_table(created_by_grantee_id)?;
    admin.revoke_object_privilege(
        transfer_owner.identity_id,
        created_schema.identity_id,
        AdminObjectPrivilege::Create,
        AdminObjectGrantTarget::Object,
    )?;
    assert!(dropped_source_table.commit_sequence > renamed_table.commit_sequence);
    assert!(dropped_table.commit_sequence > dropped_source_table.commit_sequence);
    assert!(dropped_by_grantee_table.commit_sequence > dropped_table.commit_sequence);
    let dropped_schema = admin.drop_schema(created_schema.identity_id)?;
    assert!(dropped_schema.commit_sequence > dropped_by_grantee_table.commit_sequence);
    let drop_sql_path = root.join("drop-user.sql");
    std::fs::write(
        &drop_sql_path,
        "DROP USER process_table_owner_renamed RESTRICT;",
    )?;
    let drop_sql_path_text = drop_sql_path.to_str().ok_or("SQL path is not UTF-8")?;
    let drop_user_cli = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address_text,
            "--credentials",
            admin_cli_credentials,
            "--sql-file",
            drop_sql_path_text,
        ])
        .output()?;
    assert!(
        drop_user_cli.status.success(),
        "user-drop CLI failed: {}",
        String::from_utf8_lossy(&drop_user_cli.stderr)
    );
    assert!(String::from_utf8_lossy(&drop_user_cli.stdout).contains("dropped user"));
    let mut dropped_user_credentials = read_client_credentials(&transfer_credential_path)?;
    assert!(
        SubmissionClient::connect_authenticated(
            address,
            dropped_user_credentials.token_id,
            &mut dropped_user_credentials.token_secret,
        )
        .is_err()
    );
    remove_client_credentials(&transfer_credential_path)?;
    let created_user = admin.create_user("process_token_user")?;
    let credential_path = root.join("issued-token.cred");
    let issued_user_id_text = encode_id_hex(&created_user.identity_id);
    let credential_path_text = credential_path
        .to_str()
        .ok_or("credential path is not UTF-8")?;
    let issue_token_cli = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "token-issue",
            "--address",
            &address_text,
            "--credentials",
            admin_cli_credentials,
            "--user-id",
            &issued_user_id_text,
            "--label",
            "multi-process token",
            "--ttl-seconds",
            "60",
            "--output-credentials",
            credential_path_text,
        ])
        .output()?;
    assert!(
        issue_token_cli.status.success(),
        "token-issue CLI failed: {}",
        String::from_utf8_lossy(&issue_token_cli.stderr)
    );
    let issued_cli_output = String::from_utf8_lossy(&issue_token_cli.stdout);
    assert!(issued_cli_output.contains("saved token"));
    assert!(!issued_cli_output.contains("token_secret"));
    let issued_token_id = read_client_credentials(&credential_path)?.token_id;

    let list_tokens_cli = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "token-list",
            "--address",
            &address_text,
            "--credentials",
            admin_cli_credentials,
            "--user-id",
            &issued_user_id_text,
        ])
        .output()?;
    assert!(
        list_tokens_cli.status.success(),
        "token-list CLI failed: {}",
        String::from_utf8_lossy(&list_tokens_cli.stderr)
    );
    let token_inventory = String::from_utf8_lossy(&list_tokens_cli.stdout);
    assert!(token_inventory.contains(&encode_id_hex(&issued_token_id)));
    assert!(token_inventory.contains("multi-process token"));
    assert!(!token_inventory.contains("token_secret"));

    let saved_credentials = read_client_credentials(&credential_path)?;
    let encode_secret_material = |bytes: &[u8]| {
        bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    let raw_secret_hex = encode_secret_material(&saved_credentials.token_secret);
    let verifier_hex = encode_secret_material(&prdb_engine::credentials::hash_token_secret(
        &saved_credentials.token_secret,
    ));
    assert!(!token_inventory.contains(&raw_secret_hex));
    assert!(!token_inventory.contains(&verifier_hex));
    let mut issued_secret = saved_credentials.token_secret;
    let mut issued_metadata = MetadataClient::connect_authenticated(
        address,
        saved_credentials.token_id,
        &mut issued_secret,
    )?;
    assert_eq!(issued_secret, [0; 32]);
    assert!(
        issued_metadata
            .list_schemas()?
            .iter()
            .any(|schema| schema.name == "information_schema")
    );
    drop(issued_metadata);
    drop(saved_credentials);

    let old_credentials = read_client_credentials(&credential_path)?;
    let mut old_secret_after_rotation = old_credentials.token_secret;
    let rotation_expiry = i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_micros())?
        .saturating_add(60_000_000);
    let rotated = admin.rotate_token(issued_token_id, "rotated process token", rotation_expiry)?;
    assert_ne!(rotated.token_id, issued_token_id);
    assert!(
        MetadataClient::connect_authenticated(
            address,
            issued_token_id,
            &mut old_secret_after_rotation
        )
        .is_err()
    );
    assert_eq!(old_secret_after_rotation, [0; 32]);
    remove_client_credentials(&credential_path)?;
    write_client_credentials(
        &credential_path,
        &ClientCredentials {
            token_id: rotated.token_id,
            token_secret: *rotated.token_secret(),
        },
    )?;
    let mut rotated_secret = *rotated.token_secret();
    let mut rotated_metadata =
        MetadataClient::connect_authenticated(address, rotated.token_id, &mut rotated_secret)?;
    assert_eq!(rotated_secret, [0; 32]);
    assert!(
        rotated_metadata
            .list_schemas()?
            .iter()
            .any(|schema| schema.name == "information_schema")
    );
    drop(rotated_metadata);

    let revoke_token_cli = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "token-revoke",
            "--address",
            &address_text,
            "--credentials",
            admin_cli_credentials,
            "--token-id",
            &encode_id_hex(&rotated.token_id),
        ])
        .output()?;
    assert!(
        revoke_token_cli.status.success(),
        "token-revoke CLI failed: {}",
        String::from_utf8_lossy(&revoke_token_cli.stderr)
    );
    let revoked_inventory_cli = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "token-list",
            "--address",
            &address_text,
            "--credentials",
            admin_cli_credentials,
            "--user-id",
            &issued_user_id_text,
        ])
        .output()?;
    assert!(
        revoked_inventory_cli.status.success(),
        "token-list after revoke failed: {}",
        String::from_utf8_lossy(&revoked_inventory_cli.stderr)
    );
    let revoked_inventory = String::from_utf8_lossy(&revoked_inventory_cli.stdout);
    assert!(revoked_inventory.contains("\"revoked\":true"));
    let revoked_credentials = read_client_credentials(&credential_path)?;
    let mut revoked_secret = revoked_credentials.token_secret;
    assert!(
        MetadataClient::connect_authenticated(
            address,
            revoked_credentials.token_id,
            &mut revoked_secret,
        )
        .is_err()
    );
    assert_eq!(revoked_secret, [0; 32]);
    let credential_path_text = credential_path
        .to_str()
        .ok_or("credential path is not UTF-8")?;
    let remove_credential_command = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args(["credential-remove", "--credentials", credential_path_text])
        .output()?;
    assert!(
        remove_credential_command.status.success(),
        "credential-remove CLI failed: {}",
        String::from_utf8_lossy(&remove_credential_command.stderr)
    );
    assert!(
        String::from_utf8_lossy(&remove_credential_command.stdout)
            .contains("removed credential file")
    );
    assert!(!credential_path.exists());
    let mut no_login_admin_credentials = read_client_credentials(&admin_cancel_credentials_path)?;
    let mut no_login_admin = AdminClient::connect_authenticated(
        address,
        no_login_admin_credentials.token_id,
        &mut no_login_admin_credentials.token_secret,
    )?;
    let no_login_user = no_login_admin
        .execute_create_identity_sql("CREATE USER sql_service NOLOGIN")
        .map_err(|error| format!("CREATE USER NOLOGIN failed: {error:?}"))?;
    assert!(
        no_login_admin
            .execute_create_identity_if_not_exists_sql(
                "CREATE USER IF NOT EXISTS sql_service LOGIN"
            )?
            .is_none()
    );
    let conditional_role_sql_path = root.join("create-role-if-not-exists.sql");
    std::fs::write(
        &conditional_role_sql_path,
        "CREATE ROLE IF NOT EXISTS sql_conditional_role;",
    )?;
    let create_conditional_role = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address.to_string(),
            "--credentials",
            admin_cancel_credentials_path
                .to_str()
                .ok_or("credential path is not UTF-8")?,
            "--sql-file",
            conditional_role_sql_path
                .to_str()
                .ok_or("SQL path is not UTF-8")?,
        ])
        .output()?;
    assert!(
        create_conditional_role.status.success(),
        "conditional CREATE ROLE failed: {}",
        String::from_utf8_lossy(&create_conditional_role.stderr)
    );
    assert!(String::from_utf8_lossy(&create_conditional_role.stdout).contains("created role"));
    let repeat_conditional_role = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address.to_string(),
            "--credentials",
            admin_cancel_credentials_path
                .to_str()
                .ok_or("credential path is not UTF-8")?,
            "--sql-file",
            conditional_role_sql_path
                .to_str()
                .ok_or("SQL path is not UTF-8")?,
        ])
        .output()?;
    assert!(
        repeat_conditional_role.status.success(),
        "repeated conditional CREATE ROLE failed: {}",
        String::from_utf8_lossy(&repeat_conditional_role.stderr)
    );
    assert!(
        String::from_utf8_lossy(&repeat_conditional_role.stdout)
            .contains("role already exists; no operation committed")
    );
    let token_expiry = i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_micros())?
        .saturating_add(60_000_000);
    assert!(
        no_login_admin
            .issue_token(
                no_login_user.identity_id,
                "must stay disabled",
                token_expiry
            )
            .is_err()
    );
    let mut conditional_drop_credentials = read_client_credentials(&admin_cancel_credentials_path)?;
    let mut conditional_drop_admin = AdminClient::connect_authenticated(
        address,
        conditional_drop_credentials.token_id,
        &mut conditional_drop_credentials.token_secret,
    )?;
    assert!(
        conditional_drop_admin
            .execute_drop_identity_if_exists_sql(
                "DROP ROLE IF EXISTS missing_conditional_role RESTRICT"
            )?
            .is_none()
    );
    assert!(
        conditional_drop_admin
            .execute_drop_identity_if_exists_sql("DROP USER IF EXISTS sql_service RESTRICT")?
            .is_some()
    );
    assert!(
        conditional_drop_admin
            .execute_drop_identity_if_exists_sql("DROP USER IF EXISTS sql_service RESTRICT")?
            .is_none()
    );
    assert!(
        conditional_drop_admin
            .execute_drop_identity_if_exists_sql(
                "DROP ROLE IF EXISTS sql_conditional_role RESTRICT"
            )?
            .is_some()
    );
    assert!(
        conditional_drop_admin
            .execute_drop_identity_if_exists_sql(
                "DROP ROLE IF EXISTS sql_conditional_role RESTRICT"
            )?
            .is_none()
    );
    drop(conditional_drop_admin);
    drop(conditional_drop_credentials);
    drop(no_login_admin);
    drop(no_login_admin_credentials);
    remove_client_credentials(&admin_cancel_credentials_path)?;
    drop(admin);

    let mut outcome_status_client =
        SubmissionClient::connect_authenticated(address, token_id, &mut outcome_status_secret)?;
    assert_eq!(
        outcome_status_client.status(committed_invocation_id)?,
        InvocationStatus::Committed,
        "the supervisor must recover a committed outcome after storage and terminal delivery end"
    );
    drop(outcome_status_client);

    assert!(gateway.stop()?.success());
    assert!(supervisor.stop()?.success());
    Ok(())
}

#[test]
fn self_referencing_create_table_sql_crosses_gateway_and_supervisor_processes()
-> Result<(), Box<dyn std::error::Error>> {
    fn runtime_paths(name: &str) -> Result<Vec<std::path::PathBuf>, Box<dyn std::error::Error>> {
        let value = std::env::var_os(name).ok_or(format!("{name} is required"))?;
        if name == "PRDB_TEST_RUNTIME_READ_ONLY" && value.is_empty() {
            return Ok(Vec::new());
        }
        let paths = std::env::split_paths(&value).collect::<Vec<_>>();
        if paths.is_empty() || paths.iter().any(|path| !path.is_absolute()) {
            return Err(format!("{name} must contain absolute paths").into());
        }
        Ok(paths)
    }

    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let root = std::env::temp_dir().join(format!(
        "prdb-self-reference-process-{}-{nonce}",
        std::process::id()
    ));
    std::fs::create_dir(&root)?;
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))?;
    let _cleanup = DirectoryGuard(root.clone());

    let instance_dir = root.join("instance");
    let mut setup = initialize(&instance_dir, "selfref_process_admin", i64::MAX)?;
    let token_id = setup.token_id();
    let admin_user_id = setup.admin_user_id();
    let token_secret = setup.take_secret().ok_or("bootstrap secret missing")?;
    drop(setup.acknowledge()?);

    let cgroup_parent = std::env::var_os("PRDB_TEST_CGROUP_PARENT")
        .map(std::path::PathBuf::from)
        .ok_or("PRDB_TEST_CGROUP_PARENT must name a delegated cgroup v2 parent")?;
    if !cgroup_parent.is_absolute() {
        return Err("PRDB_TEST_CGROUP_PARENT must be absolute".into());
    }
    let runtime_read_execute = runtime_paths("PRDB_TEST_RUNTIME_READ_EXECUTE")?;
    let runtime_read_only = runtime_paths("PRDB_TEST_RUNTIME_READ_ONLY")?;
    let service_socket = root.join("supervisor.sock");
    let uid = unsafe { libc::geteuid() };
    let uid_string = uid.to_string();
    let mut supervisor_command = Command::new(env!("CARGO_BIN_EXE_prdb-supervisor"));
    supervisor_command.args([
        "--instance-dir",
        instance_dir.to_str().ok_or("non-UTF-8 instance path")?,
        "--service-socket",
        service_socket.to_str().ok_or("non-UTF-8 socket path")?,
        "--gateway-uid",
        &uid_string,
        "--cgroup-parent",
        cgroup_parent.to_str().ok_or("non-UTF-8 cgroup path")?,
        "--executor",
        env!("CARGO_BIN_EXE_prdb-executor"),
        "--optimizer",
        env!("CARGO_BIN_EXE_prdb-optimizer"),
        "--scratch-parent",
        root.to_str().ok_or("non-UTF-8 scratch path")?,
        "--cpu-baseline",
        "x86_64-v1",
        "--runtime-profile",
        "test-runtime",
        "--sdk-release",
        "1",
        "--allow-legacy-static-sdk-release",
        "1",
        "--retention-seconds",
        "3600",
        "--max-rows",
        "8",
        "--max-bytes",
        "4096",
        "--cpu-quota-us",
        "100000",
        "--cpu-period-us",
        "100000",
        "--memory-max-bytes",
        "67108864",
        "--max-processes",
        "16",
    ]);
    for path in &runtime_read_execute {
        supervisor_command.arg("--runtime-read-execute").arg(path);
    }
    for path in &runtime_read_only {
        supervisor_command.arg("--runtime-read-only").arg(path);
    }
    let mut supervisor = ChildGuard(Some(
        supervisor_command
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()?,
    ));
    let socket_deadline = Instant::now() + Duration::from_secs(5);
    while !service_socket.exists() {
        if let Some(status) = supervisor.child().try_wait()? {
            return Err(format!("supervisor exited before binding: {status}").into());
        }
        if Instant::now() >= socket_deadline {
            return Err("supervisor did not bind its service socket".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    let mut gateway = ChildGuard(Some(
        Command::new(env!("CARGO_BIN_EXE_prdb-gateway"))
            .args([
                "--bind",
                "127.0.0.1:0",
                "--service-socket",
                service_socket.to_str().ok_or("non-UTF-8 socket path")?,
                "--service-uid",
                &uid_string,
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()?,
    ));
    let gateway_stderr = gateway
        .child()
        .stderr
        .take()
        .ok_or("gateway stderr missing")?;
    let (address_sender, address_receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let result = BufReader::new(gateway_stderr)
            .read_line(&mut line)
            .map(|_| line);
        let _ = address_sender.send(result);
    });
    let address_line = address_receiver.recv_timeout(Duration::from_secs(5))??;
    let address: std::net::SocketAddr = address_line
        .trim()
        .strip_prefix("prdb-gateway listening on ")
        .ok_or("gateway listen line had an unexpected prefix")?
        .parse()?;

    let mut authenticated_secret = token_secret;
    let mut admin =
        AdminClient::connect_authenticated(address, token_id, &mut authenticated_secret)?;
    assert_eq!(authenticated_secret, [0; 32]);
    let created_schema = admin.execute_create_schema_sql("CREATE SCHEMA selfref_app")?;
    let credentials_path = root.join("selfref-admin.cred");
    write_client_credentials(
        &credentials_path,
        &ClientCredentials {
            token_id,
            token_secret,
        },
    )?;
    let sql_path = root.join("selfref-create-table.sql");
    let address_text = address.to_string();
    let execute_admin_sql = |sql: &str| -> std::io::Result<std::process::Output> {
        std::fs::write(&sql_path, sql)?;
        Command::new(env!("CARGO_BIN_EXE_prdb-client"))
            .args([
                "admin-sql",
                "--address",
                &address_text,
                "--credentials",
                credentials_path
                    .to_str()
                    .ok_or_else(|| std::io::Error::other("non-UTF-8 credentials path"))?,
                "--sql-file",
                sql_path
                    .to_str()
                    .ok_or_else(|| std::io::Error::other("non-UTF-8 SQL path"))?,
            ])
            .output()
    };
    let absent_drop = execute_admin_sql("DROP TABLE IF EXISTS selfref_app.not_present RESTRICT;")?;
    assert!(
        absent_drop.status.success(),
        "administrator DROP TABLE IF EXISTS failed: {}",
        String::from_utf8_lossy(&absent_drop.stderr)
    );
    assert!(String::from_utf8_lossy(&absent_drop.stdout).contains("table did not exist"));
    let absent_schema_drop = execute_admin_sql("DROP SCHEMA IF EXISTS absent_schema RESTRICT;")?;
    assert!(
        absent_schema_drop.status.success(),
        "administrator DROP SCHEMA IF EXISTS failed: {}",
        String::from_utf8_lossy(&absent_schema_drop.stderr)
    );
    assert!(String::from_utf8_lossy(&absent_schema_drop.stdout).contains("schema did not exist"));
    let drop_probe_user = admin.create_user("drop_probe_user")?;
    let token_expiry_us = i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_micros())?
        .saturating_add(3_600_000_000);
    let drop_probe_token = admin.issue_token(
        drop_probe_user.identity_id,
        "drop-if-exists authorization test",
        token_expiry_us,
    )?;
    let mut drop_probe_secret = *drop_probe_token.token_secret();
    let mut drop_probe_admin = AdminClient::connect_authenticated(
        address,
        drop_probe_token.token_id,
        &mut drop_probe_secret,
    )?;
    assert!(
        drop_probe_admin
            .execute_drop_table_if_exists_sql(
                "DROP TABLE IF EXISTS selfref_app.not_present RESTRICT",
            )
            .is_err(),
        "non-administrator must not suppress an absent-table result"
    );
    assert!(
        drop_probe_admin
            .execute_drop_schema_if_exists_sql("DROP SCHEMA IF EXISTS absent_schema RESTRICT")
            .is_err(),
        "non-administrator must not suppress an absent-schema result"
    );
    let cli_output = execute_admin_sql(
        "CREATE TABLE selfref_app.nodes (tenant_id INTEGER, id INTEGER, parent_tenant_id INTEGER, parent_id INTEGER, PRIMARY KEY (tenant_id, id), CONSTRAINT node_tenant_nonnegative CHECK (tenant_id >= 0), FOREIGN KEY (parent_tenant_id, parent_id) REFERENCES selfref_app.nodes (tenant_id, id));",
    )?;
    assert!(
        cli_output.status.success(),
        "admin-sql self-referencing CREATE TABLE failed: {}",
        String::from_utf8_lossy(&cli_output.stderr)
    );
    let restricted_schema_drop = execute_admin_sql("DROP SCHEMA IF EXISTS selfref_app RESTRICT;")?;
    assert!(
        !restricted_schema_drop.status.success(),
        "DROP SCHEMA IF EXISTS must preserve RESTRICT failures for a present schema"
    );
    let cli_stdout = String::from_utf8_lossy(&cli_output.stdout);
    let table_id_text = cli_stdout
        .split_whitespace()
        .nth(2)
        .ok_or("admin-sql output omitted the created table ID")?;
    let table_id = decode_id_hex(table_id_text).ok_or("admin-sql returned an invalid table ID")?;
    let columns = admin.list_columns_page(created_schema.identity_id, table_id, 16, None)?;
    assert_eq!(columns.records.len(), 4);
    let constraints =
        admin.list_constraints_page(created_schema.identity_id, table_id, 16, None)?;
    let foreign_key = constraints
        .records
        .iter()
        .find(|constraint| {
            constraint.kind
                == prdb_engine::metadata_discovery::AdministrativeConstraintKind::ForeignKey
        })
        .ok_or("self-referencing FK was absent from process metadata")?;
    let named_check = constraints
        .records
        .iter()
        .find(|constraint| {
            constraint.kind == prdb_engine::metadata_discovery::AdministrativeConstraintKind::Check
                && constraint.name == "node_tenant_nonnegative"
        })
        .ok_or("named CREATE TABLE CHECK was absent from process metadata")?;
    assert_ne!(named_check.constraint_id, [0; 16]);
    assert_eq!(foreign_key.referenced_table_id, Some(table_id));
    assert_eq!(
        foreign_key.column_ids,
        vec![columns.records[2].column_id, columns.records[3].column_id]
    );
    assert_eq!(
        foreign_key.referenced_column_ids,
        vec![columns.records[0].column_id, columns.records[1].column_id]
    );
    assert!(constraints.next_cursor.is_none());

    let customers = admin.execute_create_table_sql(
        "CREATE TABLE selfref_app.customers (id INTEGER NOT NULL, email TEXT)",
    )?;
    for sql in [
        "ALTER TABLE selfref_app.customers ADD CONSTRAINT customers_pk PRIMARY KEY (id)",
        "ALTER TABLE selfref_app.customers ADD CONSTRAINT customers_email_key UNIQUE (email)",
    ] {
        let output = execute_admin_sql(sql)?;
        assert!(
            output.status.success(),
            "named key ALTER failed for {sql}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let orders = admin.execute_create_table_sql(
        "CREATE TABLE selfref_app.orders (id INTEGER NOT NULL, customer_id INTEGER)",
    )?;
    admin.grant_object_privilege(
        admin_user_id,
        customers.table_id,
        prdb_engine::client_submit::AdminObjectPrivilege::References,
        prdb_engine::client_submit::AdminObjectGrantTarget::Object,
    )?;
    for sql in [
        "ALTER TABLE selfref_app.orders ADD CONSTRAINT orders_pk PRIMARY KEY (id)",
        "ALTER TABLE selfref_app.orders ADD CONSTRAINT orders_customer_fk FOREIGN KEY (customer_id) REFERENCES selfref_app.customers (id)",
    ] {
        let output = execute_admin_sql(sql)?;
        assert!(
            output.status.success(),
            "named key/FK ALTER failed for {sql}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let customer_constraints =
        admin.list_constraints_page(created_schema.identity_id, customers.table_id, 16, None)?;
    for name in ["customers_pk", "customers_email_key"] {
        assert!(
            customer_constraints
                .records
                .iter()
                .any(|constraint| constraint.name == name)
        );
    }
    let order_constraints =
        admin.list_constraints_page(created_schema.identity_id, orders.table_id, 16, None)?;
    for name in ["orders_pk", "orders_customer_fk"] {
        assert!(
            order_constraints
                .records
                .iter()
                .any(|constraint| constraint.name == name)
        );
    }
    for sql in [
        "ALTER TABLE selfref_app.orders DROP CONSTRAINT orders_customer_fk",
        "ALTER TABLE selfref_app.orders DROP CONSTRAINT orders_pk",
        "ALTER TABLE selfref_app.customers DROP CONSTRAINT customers_email_key",
        "ALTER TABLE selfref_app.customers DROP CONSTRAINT customers_pk",
    ] {
        let output = execute_admin_sql(sql)?;
        assert!(
            output.status.success(),
            "name-based constraint drop failed for {sql}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let schema_id_text = encode_id_hex(&created_schema.identity_id);
    let table_id_text = encode_id_hex(&table_id);
    let expected_foreign_key = format!(
        "\"constraint_id\":\"{}\",\"kind\":\"foreign_key\",\"column_ids\":[\"{}\",\"{}\"],\"referenced_table_id\":\"{}\",\"referenced_column_ids\":[\"{}\",\"{}\"]",
        encode_id_hex(&foreign_key.constraint_id),
        encode_id_hex(&columns.records[2].column_id),
        encode_id_hex(&columns.records[3].column_id),
        encode_id_hex(&table_id),
        encode_id_hex(&columns.records[0].column_id),
        encode_id_hex(&columns.records[1].column_id)
    );
    let constraint_list = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "constraint-list",
            "--address",
            &address_text,
            "--credentials",
            credentials_path
                .to_str()
                .ok_or("non-UTF-8 credentials path")?,
            "--schema-id",
            &schema_id_text,
            "--table-id",
            &table_id_text,
        ])
        .output()?;
    assert!(
        constraint_list.status.success(),
        "constraint-list failed: {}",
        String::from_utf8_lossy(&constraint_list.stderr)
    );
    let constraint_json = String::from_utf8_lossy(&constraint_list.stdout);
    assert!(
        constraint_json.contains(&expected_foreign_key),
        "constraint-list omitted ordered self-referencing FK metadata: {constraint_json}"
    );
    assert!(constraint_json.ends_with("]}\n"));

    let many_constraint_columns = (0..130)
        .map(|ordinal| format!("column_{ordinal} INTEGER"))
        .collect::<Vec<_>>()
        .join(", ");
    let many_unique_constraints = (0..130)
        .map(|ordinal| format!("UNIQUE (column_{ordinal})"))
        .collect::<Vec<_>>()
        .join(", ");
    let many_constraint_table = admin.execute_create_table_sql(&format!(
        "CREATE TABLE selfref_app.many_constraints ({many_constraint_columns}, {many_unique_constraints});"
    ))?;
    let many_table_id_text = encode_id_hex(&many_constraint_table.table_id);
    let many_constraint_list = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "constraint-list",
            "--address",
            &address_text,
            "--credentials",
            credentials_path
                .to_str()
                .ok_or("non-UTF-8 credentials path")?,
            "--schema-id",
            &schema_id_text,
            "--table-id",
            &many_table_id_text,
        ])
        .output()?;
    assert!(
        many_constraint_list.status.success(),
        "constraint-list failed to continue across pages: {}",
        String::from_utf8_lossy(&many_constraint_list.stderr)
    );
    let many_constraint_json = String::from_utf8_lossy(&many_constraint_list.stdout);
    assert_eq!(
        many_constraint_json.matches("\"kind\":\"unique\"").count(),
        130,
        "constraint-list omitted UNIQUE constraints across pages: {many_constraint_json}"
    );
    let many_index_list = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "index-list",
            "--address",
            &address_text,
            "--credentials",
            credentials_path
                .to_str()
                .ok_or("non-UTF-8 credentials path")?,
            "--schema-id",
            &schema_id_text,
            "--table-id",
            &many_table_id_text,
        ])
        .output()?;
    assert!(
        many_index_list.status.success(),
        "index-list failed to continue across pages: {}",
        String::from_utf8_lossy(&many_index_list.stderr)
    );
    let many_index_json = String::from_utf8_lossy(&many_index_list.stdout);
    assert_eq!(
        many_index_json.matches("\"unique\":true").count(),
        130,
        "index-list omitted constraint-owned indexes across pages: {many_index_json}"
    );
    let drop_output = execute_admin_sql("DROP TABLE IF EXISTS selfref_app.nodes RESTRICT;")?;
    assert!(
        drop_output.status.success(),
        "admin-sql DROP TABLE failed: {}",
        String::from_utf8_lossy(&drop_output.stderr)
    );
    assert!(String::from_utf8_lossy(&drop_output.stdout).contains("dropped table"));
    let relations = admin.list_relations_page(created_schema.identity_id, 16, None)?;
    assert!(
        relations
            .records
            .iter()
            .all(|relation| relation.name != "nodes")
    );
    let parent_create =
        execute_admin_sql("CREATE TABLE selfref_app.parents (id INTEGER PRIMARY KEY);")?;
    assert!(
        parent_create.status.success(),
        "admin-sql CREATE TABLE parent failed: {}",
        String::from_utf8_lossy(&parent_create.stderr)
    );
    let parent_table_id = decode_id_hex(
        String::from_utf8_lossy(&parent_create.stdout)
            .split_whitespace()
            .nth(2)
            .ok_or("admin-sql parent output omitted the table ID")?,
    )
    .ok_or("admin-sql returned an invalid parent table ID")?;
    admin.grant_object_privilege(
        admin_user_id,
        parent_table_id,
        AdminObjectPrivilege::References,
        AdminObjectGrantTarget::AllColumns,
    )?;
    let child_create = execute_admin_sql(
        "CREATE TABLE selfref_app.children (parent_id INTEGER REFERENCES selfref_app.parents (id));",
    )?;
    assert!(
        child_create.status.success(),
        "admin-sql CREATE TABLE child failed: {}",
        String::from_utf8_lossy(&child_create.stderr)
    );
    let child_table_id = decode_id_hex(
        String::from_utf8_lossy(&child_create.stdout)
            .split_whitespace()
            .nth(2)
            .ok_or("admin-sql child output omitted the table ID")?,
    )
    .ok_or("admin-sql returned an invalid child table ID")?;
    let restricted_drop = execute_admin_sql("DROP TABLE IF EXISTS selfref_app.parents RESTRICT;")?;
    assert!(!restricted_drop.status.success());
    let relations = admin.list_relations_page(created_schema.identity_id, 16, None)?;
    assert!(
        relations
            .records
            .iter()
            .any(|relation| relation.name == "parents")
    );
    assert!(
        relations
            .records
            .iter()
            .any(|relation| relation.name == "children")
    );
    let child_constraints =
        admin.list_constraints_page(created_schema.identity_id, child_table_id, 128, None)?;
    let child_foreign_key = child_constraints
        .records
        .iter()
        .find(|constraint| {
            constraint.kind
                == prdb_engine::metadata_discovery::AdministrativeConstraintKind::ForeignKey
        })
        .ok_or("external FK was absent from child metadata")?;
    let child_table_id_text = encode_id_hex(&child_table_id);
    let child_constraint_list = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "constraint-list",
            "--address",
            &address_text,
            "--credentials",
            credentials_path
                .to_str()
                .ok_or("non-UTF-8 credentials path")?,
            "--schema-id",
            &schema_id_text,
            "--table-id",
            &child_table_id_text,
        ])
        .output()?;
    assert!(
        child_constraint_list.status.success(),
        "constraint-list failed for external FK: {}",
        String::from_utf8_lossy(&child_constraint_list.stderr)
    );
    let child_constraint_json = String::from_utf8_lossy(&child_constraint_list.stdout);
    let foreign_key_kind_offset = child_constraint_json
        .find("\"kind\":\"foreign_key\"")
        .ok_or("constraint-list omitted external FK kind")?;
    let constraint_id_start = child_constraint_json[..foreign_key_kind_offset]
        .rfind("\"constraint_id\":\"")
        .ok_or("constraint-list omitted external FK ID")?
        + "\"constraint_id\":\"".len();
    let constraint_id_end = constraint_id_start + 32;
    let child_constraint_id = &child_constraint_json[constraint_id_start..constraint_id_end];
    assert_eq!(
        child_constraint_id,
        encode_id_hex(&child_foreign_key.constraint_id)
    );
    let drop_constraint = execute_admin_sql(&format!(
        "ALTER TABLE selfref_app.children DROP CONSTRAINT '{child_constraint_id}';"
    ))?;
    assert!(
        drop_constraint.status.success(),
        "admin-sql DROP CONSTRAINT failed: {}",
        String::from_utf8_lossy(&drop_constraint.stderr)
    );
    let parent_drop = execute_admin_sql("DROP TABLE selfref_app.parents RESTRICT;")?;
    assert!(
        parent_drop.status.success(),
        "admin-sql parent DROP TABLE remained restricted after FK drop: {}",
        String::from_utf8_lossy(&parent_drop.stderr)
    );
    let child_drop = execute_admin_sql("DROP TABLE selfref_app.children RESTRICT;")?;
    assert!(
        child_drop.status.success(),
        "admin-sql child DROP TABLE failed: {}",
        String::from_utf8_lossy(&child_drop.stderr)
    );
    drop(admin);

    assert!(gateway.stop()?.success());
    assert!(supervisor.stop()?.success());
    Ok(())
}

#[test]
fn disconnected_submission_is_reconciled_after_gateway_restart()
-> Result<(), Box<dyn std::error::Error>> {
    fn required_paths(name: &str) -> Result<Vec<std::path::PathBuf>, Box<dyn std::error::Error>> {
        let value = std::env::var_os(name).ok_or(format!("{name} is required"))?;
        if name == "PRDB_TEST_RUNTIME_READ_ONLY" && value.is_empty() {
            return Ok(Vec::new());
        }
        let paths = std::env::split_paths(&value).collect::<Vec<_>>();
        if paths.is_empty() || paths.iter().any(|path| !path.is_absolute()) {
            return Err(format!("{name} must contain absolute paths").into());
        }
        Ok(paths)
    }

    fn start_gateway(
        service_socket: &std::path::Path,
        uid: u32,
    ) -> Result<(ChildGuard, std::net::SocketAddr), Box<dyn std::error::Error>> {
        let mut gateway = ChildGuard(Some(
            Command::new(env!("CARGO_BIN_EXE_prdb-gateway"))
                .args([
                    "--bind",
                    "127.0.0.1:0",
                    "--service-socket",
                    service_socket.to_str().ok_or("non-UTF-8 socket path")?,
                    "--service-uid",
                    &uid.to_string(),
                ])
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()?,
        ));
        let stderr = gateway
            .child()
            .stderr
            .take()
            .ok_or("gateway stderr missing")?;
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut line = String::new();
            let result = BufReader::new(stderr).read_line(&mut line).map(|_| line);
            let _ = sender.send(result);
        });
        let line = receiver.recv_timeout(Duration::from_secs(5))??;
        let address = line
            .trim()
            .strip_prefix("prdb-gateway listening on ")
            .ok_or("gateway listen line had an unexpected prefix")?
            .parse()?;
        Ok((gateway, address))
    }

    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let root = std::env::temp_dir().join(format!(
        "prdb-outcome-process-{}-{nonce}",
        std::process::id()
    ));
    std::fs::create_dir(&root)?;
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))?;
    let _cleanup = DirectoryGuard(root.clone());

    let cgroup_parent = std::env::var_os("PRDB_TEST_CGROUP_PARENT")
        .map(std::path::PathBuf::from)
        .ok_or("PRDB_TEST_CGROUP_PARENT must name a delegated cgroup v2 parent")?;
    if !cgroup_parent.is_absolute() {
        return Err("PRDB_TEST_CGROUP_PARENT must be absolute".into());
    }
    let runtime_read_execute = required_paths("PRDB_TEST_RUNTIME_READ_EXECUTE")?;
    let runtime_read_only = required_paths("PRDB_TEST_RUNTIME_READ_ONLY")?;

    let source = root.join("query.c");
    let object_path = root.join("query.o");
    let artifact_path = root.join("query.so");
    std::fs::write(
        &source,
        "#include <stddef.h>\n\
         #include \"prdb_query_abi_v3.h\"\n\
         const prdb_query_descriptor_v3_record prdb_query_descriptor_v3 = {\n\
             sizeof(prdb_query_descriptor_v3_record), 3,\n\
             sizeof(prdb_host_api_v3), sizeof(prdb_invocation_v3)\n\
         };\n\
         int32_t prdb_query_main_v3(const prdb_host_api_v3 *host,\n\
             const prdb_invocation_v3 *invocation) {\n\
             static const uint8_t ready[] = \"QUERY_READY\";\n\
             (void)invocation;\n\
             if (host == NULL || host->write_result == NULL ||\n\
                 host->write_result(host->context, ready, sizeof(ready) - 1) != 0) return 1;\n\
             for (;;) {}\n\
         }\n",
    )?;
    let include_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("include");
    let compile = Command::new("tcc")
        .arg("-fPIC")
        .arg("-c")
        .arg("-I")
        .arg(include_dir)
        .arg(&source)
        .arg("-o")
        .arg(&object_path)
        .output()?;
    if !compile.status.success() {
        return Err(format!(
            "TinyCC failed: {}",
            String::from_utf8_lossy(&compile.stderr)
        )
        .into());
    }
    let link = Command::new("cc")
        .arg("-shared")
        .arg("-Wl,-z,noexecstack")
        .arg(&object_path)
        .arg("-o")
        .arg(&artifact_path)
        .env_remove("NIX_LDFLAGS")
        .env("NIX_DONT_SET_RPATH", "1")
        .output()?;
    if !link.status.success() {
        return Err(format!(
            "shared linker failed: {}",
            String::from_utf8_lossy(&link.stderr)
        )
        .into());
    }
    let artifact = std::fs::read(&artifact_path)?;
    let manifest_value = manifest_v1_from_elf(
        &artifact,
        3,
        1,
        "x86_64-v1".to_owned(),
        "test-runtime".to_owned(),
        "TinyCC process integration fixture".to_owned(),
    )
    .map_err(|error| format!("derive TinyCC fixture manifest: {error:?}"))?;
    let manifest = encode_manifest_v1(&manifest_value)
        .map_err(|error| format!("encode TinyCC fixture manifest: {error:?}"))?;
    let dependencies = manifest_value.dependencies.clone();
    let package = root.join("package");
    std::fs::create_dir(&package)?;
    std::fs::set_permissions(&package, std::fs::Permissions::from_mode(0o700))?;
    let sdk_release_dir = std::env::var_os("PRDB_TEST_INSTALLED_SDK_RELEASE_DIR")
        .map(std::path::PathBuf::from)
        .ok_or("PRDB_TEST_INSTALLED_SDK_RELEASE_DIR is required for SQL package tests")?;
    let sql_library = std::fs::read(sdk_release_dir.join("libprdb_sql.so.1"))?;
    let mut requested_scope = b"PSC1\0\x01\x01\x01".to_vec();
    requested_scope.extend_from_slice(&[1; 16]);
    for (name, bytes) in [
        ("query.so", artifact.as_slice()),
        ("sql.so", sql_library.as_slice()),
        ("manifest.pmf1", manifest.as_slice()),
        // One canonical schema-USAGE capability; the query performs no data access.
        ("requested_scope.psc1", requested_scope.as_slice()),
    ] {
        std::fs::write(package.join(name), bytes)?;
        std::fs::set_permissions(package.join(name), std::fs::Permissions::from_mode(0o600))?;
    }
    let artifact_hash = Sha256::digest(&artifact);
    let hash_sidecar = format!(
        "{}\n",
        artifact_hash
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    );
    std::fs::write(package.join("artifact.sha256"), hash_sidecar)?;
    std::fs::set_permissions(
        package.join("artifact.sha256"),
        std::fs::Permissions::from_mode(0o600),
    )?;
    let sql_library_hash = Sha256::digest(&sql_library);
    let sql_library_sidecar = format!(
        "{}\n",
        sql_library_hash
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    );
    std::fs::write(package.join("sql-library.sha256"), sql_library_sidecar)?;
    std::fs::set_permissions(
        package.join("sql-library.sha256"),
        std::fs::Permissions::from_mode(0o600),
    )?;

    let instance_dir = root.join("instance");
    let mut setup = initialize(&instance_dir, "outcome_process_admin", i64::MAX)?;
    let token_id = setup.token_id();
    let token_secret = setup.take_secret().ok_or("bootstrap secret missing")?;
    drop(setup.acknowledge()?);

    let service_socket = root.join("supervisor.sock");
    let uid = unsafe { libc::geteuid() };
    let uid_string = uid.to_string();
    let cgroup_string = cgroup_parent.to_str().ok_or("non-UTF-8 cgroup path")?;
    let audit_sink_dir = root.join("supervisor-audit");
    let audit_jsonl_path = audit_sink_dir.join("events.jsonl");
    let mut supervisor_command = Command::new(env!("CARGO_BIN_EXE_prdb-supervisor"));
    supervisor_command.env("PRDB_AUDIT_JSONL_PATH", &audit_jsonl_path);
    supervisor_command.args([
        "--instance-dir",
        instance_dir.to_str().ok_or("non-UTF-8 instance path")?,
        "--service-socket",
        service_socket.to_str().ok_or("non-UTF-8 socket path")?,
        "--gateway-uid",
        &uid_string,
        "--cgroup-parent",
        cgroup_string,
        "--executor",
        env!("CARGO_BIN_EXE_prdb-executor"),
        "--optimizer",
        env!("CARGO_BIN_EXE_prdb-optimizer"),
        "--scratch-parent",
        root.to_str().ok_or("non-UTF-8 scratch path")?,
        "--cpu-baseline",
        "x86_64-v1",
        "--runtime-profile",
        "test-runtime",
        "--sdk-release",
        "1",
        "--allow-legacy-static-sdk-release",
        "1",
        "--retention-seconds",
        "3600",
        "--max-rows",
        "8",
        "--max-bytes",
        "4096",
        "--cpu-quota-us",
        "100000",
        "--cpu-period-us",
        "100000",
        "--memory-max-bytes",
        "67108864",
        "--max-processes",
        "16",
    ]);
    for path in runtime_read_execute {
        supervisor_command.arg("--runtime-read-execute").arg(path);
    }
    for path in runtime_read_only {
        supervisor_command.arg("--runtime-read-only").arg(path);
    }
    for dependency in dependencies {
        supervisor_command
            .arg("--allowed-dependency")
            .arg(dependency);
    }
    let mut supervisor = ChildGuard(Some(
        supervisor_command
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()?,
    ));
    let deadline = Instant::now() + Duration::from_secs(5);
    while !service_socket.exists() {
        if let Some(status) = supervisor.child().try_wait()? {
            return Err(format!("supervisor exited before binding: {status}").into());
        }
        if Instant::now() >= deadline {
            return Err("supervisor did not bind its service socket".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    let (mut gateway, address) = start_gateway(&service_socket, uid)?;
    let mut admin_secret = token_secret;
    let mut admin = AdminClient::connect_authenticated(address, token_id, &mut admin_secret)?;
    let mut invalid_admin_secret = [0; 32];
    assert!(
        SubmissionClient::connect_authenticated(address, token_id, &mut invalid_admin_secret)
            .is_err()
    );
    let authentication_audit = admin.list_audit_events_page(128, None)?;
    assert!(authentication_audit.events.iter().any(|event| {
        event.action == 4
            && event.result == 2
            && event.actor_id.is_none()
            && event.target_ids.as_slice() == [token_id]
    }));
    assert!(
        !audit_jsonl_path.exists(),
        "a failed sink must leave its audit event pending without changing authentication"
    );
    std::fs::create_dir(&audit_sink_dir)?;
    let sink_deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(lines) = std::fs::read_to_string(&audit_jsonl_path)
            && lines.contains("\"action\":4")
            && lines.contains("\"result\":2")
        {
            assert!(lines.contains("\"actor_id\":null"));
            assert!(lines.contains(&format!(
                "\"target_ids\":[\"{}\"]",
                encode_id_hex(&token_id)
            )));
            break;
        }
        if Instant::now() >= sink_deadline {
            return Err("supervisor did not deliver the authentication audit event".into());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let runtime_list_credentials_path = root.join("runtime-list-admin.cred");
    write_client_credentials(
        &runtime_list_credentials_path,
        &ClientCredentials {
            token_id,
            token_secret,
        },
    )?;
    let query_user = admin
        .create_user("process_query_owner")
        .map_err(|error| format!("create query user failed: {error:?}"))?;
    let query_schema = admin
        .create_schema("process_query_schema", query_user.identity_id)
        .map_err(|error| format!("create query schema failed: {error:?}"))?;
    admin
        .grant_object_privilege(
            query_user.identity_id,
            query_schema.identity_id,
            AdminObjectPrivilege::Usage,
            AdminObjectGrantTarget::Object,
        )
        .map_err(|error| format!("grant query schema usage failed: {error:?}"))?;
    let mut requested_scope = b"PSC1\0\x01\x01\x01".to_vec();
    requested_scope.extend_from_slice(&query_schema.identity_id);
    std::fs::write(package.join("requested_scope.psc1"), requested_scope)?;
    std::fs::set_permissions(
        package.join("requested_scope.psc1"),
        std::fs::Permissions::from_mode(0o600),
    )?;
    let query_token_expiry =
        i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_micros())? + 3_600_000_000;
    let query_token = admin
        .issue_token(query_user.identity_id, "process query", query_token_expiry)
        .map_err(|error| format!("issue query token failed: {error:?}"))?;
    let query_token_id = query_token.token_id;
    let mut query_secret = *query_token.token_secret();
    let recovery_token = admin
        .issue_token(
            query_user.identity_id,
            "process query recovery",
            query_token_expiry,
        )
        .map_err(|error| format!("issue recovery token failed: {error:?}"))?;
    let recovery_token_id = recovery_token.token_id;
    let mut restart_recovery_secret = *recovery_token.token_secret();
    let mut durable_recovery_secret = *recovery_token.token_secret();
    let invocation_list_credentials_path = root.join("invocation-list-owner.cred");
    write_client_credentials(
        &invocation_list_credentials_path,
        &ClientCredentials {
            token_id: query_token_id,
            token_secret: query_secret,
        },
    )?;
    let mut restart_query_secret = query_secret;
    let mut client =
        SubmissionClient::connect_authenticated(address, query_token_id, &mut query_secret)?;
    let invocation_id = client
        .upload_and_seal(&package, &[], 30_000, WorkloadClass::Operational)
        .map_err(|error| format!("upload/seal failed: {error:?}"))?;
    let receipt = root.join("start.receipt");
    let cancellation_requested_at;
    {
        let mut started = client
            .start(invocation_id, &receipt)
            .map_err(|error| format!("query START failed: {error:?}"))?;
        let ready_event = started.next_event()?;
        assert!(
            matches!(ready_event, Some(prdb_engine::client_submit::ClientResultEvent::Data(ref bytes)) if bytes == b"QUERY_READY"),
            "unexpected query startup event: {ready_event:?}"
        );
        let invocation_list = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
            .args([
                "invocation-list",
                "--address",
                &address.to_string(),
                "--credentials",
                invocation_list_credentials_path
                    .to_str()
                    .ok_or("credential path is not UTF-8")?,
            ])
            .output()?;
        assert!(
            invocation_list.status.success(),
            "invocation-list CLI failed: {}",
            String::from_utf8_lossy(&invocation_list.stderr)
        );
        let invocation_list_output = String::from_utf8(invocation_list.stdout)?;
        assert!(invocation_list_output.contains("\"consistency\":\"best_effort\""));
        assert!(invocation_list_output.contains(&format!(
            "\"invocation_id\":\"{}\"",
            encode_id_hex(&invocation_id)
        )));
        assert!(invocation_list_output.contains("\"state\":\"active\""));
        assert!(invocation_list_output.contains("\"staged_rows\":0"));
        assert!(invocation_list_output.contains("\"staged_value_bytes\":0"));
        assert!(!invocation_list_output.contains("\"principal_id\":"));
        let other_owner_list = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
            .args([
                "invocation-list",
                "--address",
                &address.to_string(),
                "--credentials",
                runtime_list_credentials_path
                    .to_str()
                    .ok_or("credential path is not UTF-8")?,
            ])
            .output()?;
        assert!(
            other_owner_list.status.success(),
            "other-owner invocation-list CLI failed: {}",
            String::from_utf8_lossy(&other_owner_list.stderr)
        );
        let other_owner_output = String::from_utf8(other_owner_list.stdout)?;
        assert!(other_owner_output.contains("\"invocations\":[]"));
        assert!(!other_owner_output.contains(&encode_id_hex(&invocation_id)));
        let mut unrelated_owner_secret = token_secret;
        let mut unrelated_owner = SubmissionClient::connect_authenticated(
            address,
            token_id,
            &mut unrelated_owner_secret,
        )?;
        assert_eq!(
            unrelated_owner.status(invocation_id)?,
            InvocationStatus::Unknown,
            "cross-owner status must not reveal the invocation"
        );
        let runtime_list = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
            .args([
                "runtime-list",
                "--address",
                &address.to_string(),
                "--credentials",
                runtime_list_credentials_path
                    .to_str()
                    .ok_or("credential path is not UTF-8")?,
            ])
            .output()?;
        assert!(
            runtime_list.status.success(),
            "runtime-list CLI failed: {}",
            String::from_utf8_lossy(&runtime_list.stderr)
        );
        let runtime_output = String::from_utf8(runtime_list.stdout)?;
        assert!(runtime_output.contains("\"observed_at_us\":"));
        assert!(runtime_output.contains("\"consistency\":\"best_effort\""));
        assert!(runtime_output.contains("\"invocations\":["));
        assert!(runtime_output.contains("\"workload_class\":\"operational\""));
        assert!(runtime_output.contains("\"staged_rows\":0"));
        assert!(runtime_output.contains("\"staged_value_bytes\":0"));
        assert!(runtime_output.contains("\"snapshot_pin_count\":1"));
        assert!(runtime_output.contains("\"transaction_id\":\""));
        assert!(runtime_output.contains("\"age_us\":"));
        assert!(runtime_output.contains("\"fence_waits\":[]"));
        assert!(runtime_output.contains("\"fence_waits_truncated\":false"));
        assert!(runtime_output.contains("\"gateway_result_bytes_relayed_total\":"));
        let gateway_bytes = runtime_output
            .split_once("\"gateway_result_bytes_sent_to_clients_total\":")
            .and_then(|(_, suffix)| {
                suffix
                    .split(|character: char| !character.is_ascii_digit())
                    .next()
            })
            .and_then(|value| value.parse::<u64>().ok())
            .ok_or("runtime-list omitted the gateway client-write byte counter")?;
        assert!(
            gateway_bytes >= b"QUERY_READY".len() as u64,
            "gateway byte counter did not include the successfully delivered ready event"
        );
        assert!(runtime_output.contains("\"pending_submissions\":0"));
        assert!(runtime_output.contains("\"starting_submissions\":0"));
        assert!(runtime_output.contains("\"started_submissions\":1"));
        assert!(runtime_output.contains("\"queued_operational_starts\":0"));
        assert!(runtime_output.contains("\"queued_analytical_starts\":0"));
        assert!(runtime_output.contains("\"oldest_queued_start_wait_us\":0"));
        assert!(runtime_output.contains("\"operational_start_wait_buckets\":[0, 0, 0, 0, 0, 0]"));
        assert!(runtime_output.contains("\"analytical_start_wait_buckets\":[0, 0, 0, 0, 0, 0]"));
        assert!(runtime_output.contains("\"reserved_submission_bytes\":"));
        assert!(runtime_output.contains("\"pinned_snapshot_count\":1"));
        assert!(runtime_output.contains("\"oldest_snapshot_pin_age_us\":"));
        assert!(runtime_output.contains("\"invocation_policy_denials_total\":0"));
        assert!(runtime_output.contains("\"invocation_staged_value_bytes_total\":0"));
        assert!(runtime_output.contains("\"invocation_commit_latency_buckets\":[0,0,0,0,0,0]"));
        assert!(runtime_output.contains("\"invocation_redb_write_acquisition_buckets\":["));
        let runtime_counter = |name: &str| {
            let field = format!("\"{name}\":");
            runtime_output
                .split_once(&field)
                .and_then(|(_, value)| {
                    value
                        .split(|character: char| !character.is_ascii_digit())
                        .next()
                })
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or_else(|| panic!("missing numeric runtime field {name}: {runtime_output}"))
        };
        assert!(runtime_counter("outcome_maintenance_passes_total") > 0);
        assert_eq!(runtime_counter("outcome_maintenance_failures_total"), 0);
        let _ = runtime_counter("outcomes_pruned_total");
        let audit_list = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
            .args([
                "audit-list",
                "--address",
                &address.to_string(),
                "--credentials",
                runtime_list_credentials_path
                    .to_str()
                    .ok_or("credential path is not UTF-8")?,
                "--limit",
                "1",
            ])
            .output()?;
        assert!(
            audit_list.status.success(),
            "audit-list CLI failed: {}",
            String::from_utf8_lossy(&audit_list.stderr)
        );
        let audit_output = String::from_utf8(audit_list.stdout)?;
        assert!(audit_output.contains("\"events\":[{"));
        assert!(audit_output.contains("\"event_id\":\""));
        assert!(audit_output.contains("\"occurred_at_us\":"));
        assert!(audit_output.contains("\"action\":"));
        assert!(audit_output.contains("\"result\":"));
        assert!(audit_output.contains("\"target_ids\":["));
        assert!(audit_output.contains("\"next_cursor\":\""));
        let active_status = admin
            .status(invocation_id)
            .map_err(|error| format!("active STATUS failed: {error:?}"))?;
        assert_eq!(active_status, InvocationStatus::Active);
        let cancellation = admin
            .cancel_invocation(invocation_id)
            .map_err(|error| format!("active CANCEL failed: {error:?}"))?;
        assert_eq!(cancellation, CancellationResult::Accepted);
        let revoked = admin
            .revoke_token(query_token_id)
            .map_err(|error| format!("revoke original query token failed: {error:?}"))?;
        assert!(revoked.commit_sequence > 0);
        cancellation_requested_at = Instant::now();
        let _gateway_crash_status = gateway.kill()?;
    }
    drop(client);

    let (mut replacement_gateway, replacement_address) = start_gateway(&service_socket, uid)?;
    assert!(
        SubmissionClient::connect_authenticated(
            replacement_address,
            query_token_id,
            &mut restart_query_secret,
        )
        .is_err(),
        "a revoked token must not reconnect for status reconciliation"
    );
    let mut status_client = SubmissionClient::connect_authenticated(
        replacement_address,
        recovery_token_id,
        &mut restart_recovery_secret,
    )?;
    let status_deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        let status = status_client.status(invocation_id)?;
        if matches!(
            status,
            InvocationStatus::Committed | InvocationStatus::Aborted
        ) {
            break status;
        }
        if Instant::now() >= status_deadline {
            return Err(
                format!("invocation did not reach a durable terminal status: {status:?}").into(),
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(status, InvocationStatus::Aborted);
    assert!(
        cancellation_requested_at.elapsed() < Duration::from_secs(2),
        "non-cooperative worker did not reach durable ABORTED within two seconds"
    );
    drop(status_client);
    drop(admin);
    assert!(replacement_gateway.stop()?.success());
    assert!(supervisor.stop()?.success());
    assert!(!service_socket.exists(), "supervisor left a stale socket");

    let mut restarted_supervisor_command = Command::new(env!("CARGO_BIN_EXE_prdb-supervisor"));
    restarted_supervisor_command.args([
        "--instance-dir",
        instance_dir.to_str().ok_or("non-UTF-8 instance path")?,
        "--service-socket",
        service_socket.to_str().ok_or("non-UTF-8 socket path")?,
        "--gateway-uid",
        &uid_string,
        "--cgroup-parent",
        cgroup_string,
        "--executor",
        env!("CARGO_BIN_EXE_prdb-executor"),
        "--optimizer",
        env!("CARGO_BIN_EXE_prdb-optimizer"),
        "--scratch-parent",
        root.to_str().ok_or("non-UTF-8 scratch path")?,
        "--cpu-baseline",
        "x86_64-v1",
        "--runtime-profile",
        "test-runtime",
        "--sdk-release",
        "1",
        "--allow-legacy-static-sdk-release",
        "1",
        "--retention-seconds",
        "3600",
        "--max-rows",
        "8",
        "--max-bytes",
        "4096",
        "--cpu-quota-us",
        "100000",
        "--cpu-period-us",
        "100000",
        "--memory-max-bytes",
        "67108864",
        "--max-processes",
        "16",
    ]);
    for path in required_paths("PRDB_TEST_RUNTIME_READ_EXECUTE")? {
        restarted_supervisor_command
            .arg("--runtime-read-execute")
            .arg(path);
    }
    for path in required_paths("PRDB_TEST_RUNTIME_READ_ONLY")? {
        restarted_supervisor_command
            .arg("--runtime-read-only")
            .arg(path);
    }
    for dependency in &manifest_value.dependencies {
        restarted_supervisor_command
            .arg("--allowed-dependency")
            .arg(dependency);
    }
    let mut restarted_supervisor = ChildGuard(Some(
        restarted_supervisor_command
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()?,
    ));
    let restart_deadline = Instant::now() + Duration::from_secs(5);
    while !service_socket.exists() {
        if let Some(status) = restarted_supervisor.child().try_wait()? {
            return Err(format!("restarted supervisor exited before binding: {status}").into());
        }
        if Instant::now() >= restart_deadline {
            return Err("restarted supervisor did not bind its service socket".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let (mut recovered_gateway, recovered_address) = start_gateway(&service_socket, uid)?;
    let mut recovered_status_client = SubmissionClient::connect_authenticated(
        recovered_address,
        recovery_token_id,
        &mut durable_recovery_secret,
    )?;
    assert_eq!(
        recovered_status_client.status(invocation_id)?,
        InvocationStatus::Aborted,
        "ABORTED must survive closing and reopening the storage process"
    );
    drop(recovered_status_client);
    assert!(recovered_gateway.stop()?.success());
    assert!(restarted_supervisor.stop()?.success());
    Ok(())
}

struct DirectoryGuard(std::path::PathBuf);

fn encode_id_hex(id: &[u8; 16]) -> String {
    let mut encoded = String::with_capacity(32);
    for byte in id {
        use std::fmt::Write as _;
        write!(&mut encoded, "{byte:02x}").expect("writing to String cannot fail");
    }
    encoded
}

fn decode_id_hex(encoded: &str) -> Option<[u8; 16]> {
    if encoded.len() != 32 || !encoded.is_ascii() {
        return None;
    }
    let mut id = [0; 16];
    for (index, byte) in id.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&encoded[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(id)
}

impl Drop for DirectoryGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
