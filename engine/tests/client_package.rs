use prdb_engine::client_config::{
    CONFIG_FORMAT, ClientConfig, ClientCredential as ConfigCredential, ClientInstance, ClientTools,
    SdkSelection, encode_token_secret,
};
use prdb_engine::client_credentials::write_client_credentials;
use prdb_engine::client_protocol::{
    CLIENT_FRAME_HEADER_BYTES, ClientCredentials, ClientFrame, ClientFrameDecoder,
    decode_client_chunk, decode_client_credentials, decode_client_prepare,
};
use prdb_engine::client_sql::{encode_parameter_file, generate_query_source};
use prdb_engine::client_submit::SubmissionClient;
use prdb_engine::submission_manifest::{
    ArtifactCompatibilityProfile, VersionedSubmissionMetadataValidator, decode_elf_dynamic_info,
    decode_manifest_v1,
};
use prdb_engine::supervisor::{SubmissionMetadata, SubmissionMetadataValidator, WorkloadClass};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use zeroize::Zeroize;

fn unique_temp_dir() -> PathBuf {
    let mut random = [0u8; 12];
    getrandom::fill(&mut random).unwrap();
    let suffix = random
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    std::env::temp_dir().join(format!(
        "prdb-client-package-{}-{suffix}",
        std::process::id()
    ))
}

#[test]
fn generated_client_frame_header_preserves_little_endian_wire_bytes() {
    let frame = ClientFrame {
        kind: 102,
        request_id: 0x0102_0304_0506_0708,
        response_sequence: 0x1112_1314_1516_1718,
        payload: vec![b'X'],
    };
    let encoded = frame.encode().unwrap();
    assert_eq!(CLIENT_FRAME_HEADER_BYTES, 32);
    assert_eq!(
        &encoded[..CLIENT_FRAME_HEADER_BYTES],
        &[
            b'P', b'D', b'B', b'N', 1, 0, 0, 0, 102, 0, 0, 0, 1, 0, 0, 0, 8, 7, 6, 5, 4, 3, 2, 1,
            24, 23, 22, 21, 20, 19, 18, 17,
        ]
    );
    let mut decoder = ClientFrameDecoder::default();
    assert_eq!(decoder.push(&encoded).unwrap(), [frame]);
    decoder.finish().unwrap();
}

#[test]
fn client_frame_decoder_accepts_every_split_of_a_golden_frame() {
    // Independent hand-checked PDBN v1.0 HELLO frame carrying `{}`.
    let golden = [
        b'P', b'D', b'B', b'N', 1, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0, 7, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, b'{', b'}',
    ];
    let expected = ClientFrame {
        kind: 1,
        request_id: 7,
        response_sequence: 0,
        payload: b"{}".to_vec(),
    };

    for split in 0..=golden.len() {
        let mut decoder = ClientFrameDecoder::default();
        let first = decoder.push(&golden[..split]).unwrap();
        let second = decoder.push(&golden[split..]).unwrap();
        assert_eq!(first.len() + second.len(), 1, "split at byte {split}");
        assert_eq!(first.into_iter().chain(second).next().unwrap(), expected);
        decoder.finish().unwrap();
    }
}

#[test]
fn admin_sql_cli_runs_create_table_over_authenticated_control_mode() {
    let temp = unique_temp_dir();
    fs::create_dir(&temp).unwrap();
    let credential_path = temp.join("admin.cred");
    write_client_credentials(
        &credential_path,
        &ClientCredentials {
            token_id: [0x41; 16],
            token_secret: [0x42; 32],
        },
    )
    .unwrap();
    let sql_path = temp.join("create.sql");
    fs::write(
        &sql_path,
        "CREATE TABLE app.events (id UUID GENERATED ALWAYS AS IDENTITY PRIMARY KEY, amount DECIMAL(5,2) DEFAULT 1.20 UNIQUE CHECK (amount >= 0.75), customer_id INTEGER REFERENCES crm.customers (id));",
    )
    .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut decoder = ClientFrameDecoder::default();
        let hello = read_client_frame(&mut stream, &mut decoder);
        assert_eq!(hello.kind, 1);
        write_server_response(&mut stream, &hello, br#"{"transport":"plaintext_tcp","limits":{"chunk_bytes":131072,"control_bytes":32768,"payload_bytes":524288},"mode":"control","minor":0,"major":1}"#);
        let auth = read_client_frame(&mut stream, &mut decoder);
        assert_eq!(auth.kind, 2);
        let decoded = decode_client_credentials(&auth.payload).unwrap();
        assert_eq!(decoded.token_id, [0x41; 16]);
        assert_eq!(decoded.token_secret, [0x42; 32]);
        write_server_response(
            &mut stream,
            &auth,
            br#"{"session_id":"abababababababababababababababab","authenticated":true}"#,
        );

        let schemas = read_client_frame(&mut stream, &mut decoder);
        assert_eq!(schemas.kind, 10);
        assert!(
            String::from_utf8(schemas.payload.clone())
                .unwrap()
                .contains(r#""operation":"metadata.schemas.list""#)
        );
        write_server_response(
            &mut stream,
            &schemas,
            format!(
                r#"{{"schemas":[{{"schema_id":"{}","name":"app"}}],"next_cursor":null}}"#,
                "11".repeat(16)
            )
            .as_bytes(),
        );

        let target_schema = read_client_frame(&mut stream, &mut decoder);
        assert!(
            String::from_utf8_lossy(&target_schema.payload)
                .contains(r#""operation":"metadata.schemas.list""#)
        );
        write_server_response(
            &mut stream,
            &target_schema,
            format!(
                r#"{{"schemas":[{{"schema_id":"{}","name":"crm"}}],"next_cursor":null}}"#,
                "12".repeat(16)
            )
            .as_bytes(),
        );

        for (operation, response) in [
            (
                "metadata.relations.list",
                format!(
                    r#"{{"relations":[{{"relation_id":"{}","name":"customers"}}],"next_cursor":null}}"#,
                    "13".repeat(16)
                ),
            ),
            (
                "metadata.columns.list",
                format!(
                    r#"{{"columns":[{{"column_id":"{}","name":"id","ordinal":0,"scalar_type":"integer","nullable":false}}],"next_cursor":null}}"#,
                    "14".repeat(16)
                ),
            ),
            (
                "metadata.constraints.list",
                format!(
                    r#"{{"constraints":[{{"constraint_id":"{}","name":"primary_key_constraint_15151515151515151515151515151515","kind":"primary_key","column_ids":["{}"],"referenced_table_id":null,"referenced_column_ids":[]}}],"next_cursor":null}}"#,
                    "15".repeat(16),
                    "14".repeat(16)
                ),
            ),
        ] {
            let request = read_client_frame(&mut stream, &mut decoder);
            assert!(
                String::from_utf8_lossy(&request.payload)
                    .contains(&format!(r#""operation":"{operation}""#))
            );
            write_server_response(&mut stream, &request, response.as_bytes());
        }

        let prepare = read_client_frame(&mut stream, &mut decoder);
        assert_eq!(prepare.kind, 10);
        let payload = String::from_utf8(prepare.payload.clone()).unwrap();
        assert!(payload.contains(r#""operation":"tables.create""#));
        assert!(payload.contains(r#""generated_uuid_identity":true"#));
        assert!(payload.contains(r#""primary_key_columns":["id"]"#));
        assert!(payload.contains(r#""unique_constraints":[["amount"]]"#));
        assert!(payload.contains(r#""check_expressions_eir1":[""#));
        assert!(payload.contains(r#""value":"1.20""#));
        assert!(payload.contains(&format!(
            r#""foreign_key_source_columns":[["customer_id"]],"foreign_key_target_constraint_ids":["{}"]"#,
            "15".repeat(16)
        )));
        let operation_id = [0x22; 16];
        write_server_response(
            &mut stream,
            &prepare,
            format!(
                r#"{{"operation_id":"{}","state":"prepared"}}"#,
                hex(&operation_id)
            )
            .as_bytes(),
        );
        let start = read_client_frame(&mut stream, &mut decoder);
        assert_eq!(start.kind, 10);
        write_server_response(
            &mut stream,
            &start,
            format!(r#"{{"operation_id":"{}","outcome":"committed","table_id":"{}","column_ids":["{}","{}","{}"],"commit_sequence":"77"}}"#, hex(&operation_id), "33".repeat(16), "44".repeat(16), "55".repeat(16), "56".repeat(16)).as_bytes(),
        );
    });

    let output = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address.to_string(),
            "--credentials",
            credential_path.to_str().unwrap(),
            "--sql-file",
            sql_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "admin-sql failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains(&"33".repeat(16)));
    assert!(stdout.contains("with 3 columns"));
    assert!(stdout.contains("commit 77"));
    server.join().unwrap();
    fs::remove_dir_all(temp).unwrap();
}

#[test]
fn admin_sql_cli_runs_create_schema_with_explicit_owner() {
    let temp = unique_temp_dir();
    fs::create_dir(&temp).unwrap();
    let credential_path = temp.join("admin.cred");
    write_client_credentials(
        &credential_path,
        &ClientCredentials {
            token_id: [0x41; 16],
            token_secret: [0x42; 32],
        },
    )
    .unwrap();
    let sql_path = temp.join("create.sql");
    fs::write(
        &sql_path,
        "CREATE SCHEMA app AUTHORIZATION '01010101-0101-0101-0101-010101010101';",
    )
    .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut decoder = ClientFrameDecoder::default();
        let hello = read_client_frame(&mut stream, &mut decoder);
        write_server_response(&mut stream, &hello, br#"{"transport":"plaintext_tcp","limits":{"chunk_bytes":131072,"control_bytes":32768,"payload_bytes":524288},"mode":"control","minor":0,"major":1}"#);
        let auth = read_client_frame(&mut stream, &mut decoder);
        let decoded = decode_client_credentials(&auth.payload).unwrap();
        assert_eq!(decoded.token_id, [0x41; 16]);
        assert_eq!(decoded.token_secret, [0x42; 32]);
        write_server_response(
            &mut stream,
            &auth,
            br#"{"session_id":"abababababababababababababababab","authenticated":true}"#,
        );

        let prepare = read_client_frame(&mut stream, &mut decoder);
        let payload = String::from_utf8(prepare.payload.clone()).unwrap();
        assert!(payload.contains(r#""operation":"schemas.create""#));
        assert!(payload.contains(r#""name":"app""#));
        assert!(payload.contains(r#""owner_id":"01010101010101010101010101010101""#));
        let operation_id = [0x23; 16];
        write_server_response(
            &mut stream,
            &prepare,
            format!(
                r#"{{"operation_id":"{}","state":"prepared"}}"#,
                hex(&operation_id)
            )
            .as_bytes(),
        );
        let start = read_client_frame(&mut stream, &mut decoder);
        write_server_response(
            &mut stream,
            &start,
            format!(r#"{{"operation_id":"{}","outcome":"committed","schema_id":"{}","commit_sequence":"78"}}"#, hex(&operation_id), "66".repeat(16)).as_bytes(),
        );
    });

    let output = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address.to_string(),
            "--credentials",
            credential_path.to_str().unwrap(),
            "--sql-file",
            sql_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "admin-sql failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains(&"66".repeat(16)));
    assert!(stdout.contains("commit 78"));
    server.join().unwrap();
    fs::remove_dir_all(temp).unwrap();
}

#[test]
fn admin_sql_cli_rejects_unsupported_ddl_before_credentials_or_network() {
    let temp = unique_temp_dir();
    fs::create_dir(&temp).unwrap();
    let sql_path = temp.join("unsupported.sql");
    fs::write(&sql_path, "DROP TABLE app.events").unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address.to_string(),
            "--credentials",
            temp.join("missing.cred").to_str().unwrap(),
            "--sql-file",
            sql_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("invalid administrative SQL"));
    assert!(matches!(
        listener.accept(),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
    ));
    fs::write(&sql_path, "ALTER TABLE app.events ADD UNIQUE (id)").unwrap();
    let recognized = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address.to_string(),
            "--credentials",
            temp.join("missing.cred").to_str().unwrap(),
            "--sql-file",
            sql_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!recognized.status.success());
    assert!(!String::from_utf8_lossy(&recognized.stderr).contains("invalid administrative SQL"));
    assert!(matches!(
        listener.accept(),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
    ));
    fs::write(&sql_path, "CREATE USER analyst LOGIN").unwrap();
    let recognized_create_user = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address.to_string(),
            "--credentials",
            temp.join("missing.cred").to_str().unwrap(),
            "--sql-file",
            sql_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!recognized_create_user.status.success());
    assert!(
        !String::from_utf8_lossy(&recognized_create_user.stderr)
            .contains("invalid administrative SQL")
    );
    assert!(matches!(
        listener.accept(),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
    ));
    fs::write(&sql_path, "ALTER USER analyst RENAME TO analyst_admin").unwrap();
    let recognized_rename = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address.to_string(),
            "--credentials",
            temp.join("missing.cred").to_str().unwrap(),
            "--sql-file",
            sql_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!recognized_rename.status.success());
    assert!(
        !String::from_utf8_lossy(&recognized_rename.stderr).contains("invalid administrative SQL")
    );
    assert!(matches!(
        listener.accept(),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
    ));
    fs::write(&sql_path, "CREATE USER service_loader NOLOGIN").unwrap();
    let recognized_no_login = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address.to_string(),
            "--credentials",
            temp.join("missing.cred").to_str().unwrap(),
            "--sql-file",
            sql_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!recognized_no_login.status.success());
    assert!(
        !String::from_utf8_lossy(&recognized_no_login.stderr)
            .contains("invalid administrative SQL")
    );
    assert!(matches!(
        listener.accept(),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
    ));
    fs::write(&sql_path, "DROP INDEX app.employee_department RESTRICT").unwrap();
    let recognized_drop_index = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address.to_string(),
            "--credentials",
            temp.join("missing.cred").to_str().unwrap(),
            "--sql-file",
            sql_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!recognized_drop_index.status.success());
    assert!(
        !String::from_utf8_lossy(&recognized_drop_index.stderr)
            .contains("invalid administrative SQL")
    );
    assert!(matches!(
        listener.accept(),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
    ));
    fs::write(
        &sql_path,
        "ALTER TABLE app.events ADD CHECK (enabled AND deleted IS NULL)",
    )
    .unwrap();
    let recognized_check = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address.to_string(),
            "--credentials",
            temp.join("missing.cred").to_str().unwrap(),
            "--sql-file",
            sql_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!recognized_check.status.success());
    assert!(
        !String::from_utf8_lossy(&recognized_check.stderr).contains("invalid administrative SQL")
    );
    assert!(matches!(
        listener.accept(),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
    ));
    fs::write(
        &sql_path,
        "ALTER TABLE app.orders ADD FOREIGN KEY (customer_id) REFERENCES app.customers (id)",
    )
    .unwrap();
    let recognized_foreign_key = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address.to_string(),
            "--credentials",
            temp.join("missing.cred").to_str().unwrap(),
            "--sql-file",
            sql_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!recognized_foreign_key.status.success());
    assert!(
        !String::from_utf8_lossy(&recognized_foreign_key.stderr)
            .contains("invalid administrative SQL")
    );
    assert!(matches!(
        listener.accept(),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
    ));
    fs::write(
        &sql_path,
        "ALTER TABLE app.employees ALTER COLUMN active SET NOT NULL",
    )
    .unwrap();
    let recognized_nullability = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address.to_string(),
            "--credentials",
            temp.join("missing.cred").to_str().unwrap(),
            "--sql-file",
            sql_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!recognized_nullability.status.success());
    assert!(
        !String::from_utf8_lossy(&recognized_nullability.stderr)
            .contains("invalid administrative SQL")
    );
    assert!(matches!(
        listener.accept(),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
    ));
    fs::write(
        &sql_path,
        "ALTER TABLE app.orders DROP CONSTRAINT 'c5c5c5c5-c5c5-c5c5-c5c5-c5c5c5c5c5c5'",
    )
    .unwrap();
    let recognized_drop_constraint = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address.to_string(),
            "--credentials",
            temp.join("missing.cred").to_str().unwrap(),
            "--sql-file",
            sql_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!recognized_drop_constraint.status.success());
    assert!(
        !String::from_utf8_lossy(&recognized_drop_constraint.stderr)
            .contains("invalid administrative SQL")
    );
    assert!(matches!(
        listener.accept(),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
    ));
    fs::write(
        &sql_path,
        "ALTER TABLE app.employees ALTER COLUMN active SET DEFAULT TRUE",
    )
    .unwrap();
    let recognized_default = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address.to_string(),
            "--credentials",
            temp.join("missing.cred").to_str().unwrap(),
            "--sql-file",
            sql_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!recognized_default.status.success());
    assert!(
        !String::from_utf8_lossy(&recognized_default.stderr).contains("invalid administrative SQL")
    );
    assert!(matches!(
        listener.accept(),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
    ));
    fs::write(
        &sql_path,
        "CREATE UNIQUE INDEX employee_department ON app.employees USING BTREE (department_id, name)",
    )
    .unwrap();
    let recognized_create_index = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address.to_string(),
            "--credentials",
            temp.join("missing.cred").to_str().unwrap(),
            "--sql-file",
            sql_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!recognized_create_index.status.success());
    assert!(
        !String::from_utf8_lossy(&recognized_create_index.stderr)
            .contains("invalid administrative SQL")
    );
    assert!(matches!(
        listener.accept(),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
    ));
    fs::write(
        &sql_path,
        "ALTER TABLE app.events ADD COLUMN note TEXT DEFAULT 'new'",
    )
    .unwrap();
    let recognized_add_column = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address.to_string(),
            "--credentials",
            temp.join("missing.cred").to_str().unwrap(),
            "--sql-file",
            sql_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!recognized_add_column.status.success());
    assert!(
        !String::from_utf8_lossy(&recognized_add_column.stderr)
            .contains("invalid administrative SQL")
    );
    assert!(matches!(
        listener.accept(),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
    ));
    fs::write(
        &sql_path,
        "ALTER TABLE app.events RENAME COLUMN old_name TO new_name",
    )
    .unwrap();
    let recognized_rename_column = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address.to_string(),
            "--credentials",
            temp.join("missing.cred").to_str().unwrap(),
            "--sql-file",
            sql_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!recognized_rename_column.status.success());
    assert!(
        !String::from_utf8_lossy(&recognized_rename_column.stderr)
            .contains("invalid administrative SQL")
    );
    assert!(matches!(
        listener.accept(),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
    ));
    fs::write(
        &sql_path,
        "ALTER TABLE app.events DROP COLUMN obsolete RESTRICT",
    )
    .unwrap();
    let recognized_drop_column = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-sql",
            "--address",
            &address.to_string(),
            "--credentials",
            temp.join("missing.cred").to_str().unwrap(),
            "--sql-file",
            sql_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!recognized_drop_column.status.success());
    assert!(
        !String::from_utf8_lossy(&recognized_drop_column.stderr)
            .contains("invalid administrative SQL")
    );
    assert!(matches!(
        listener.accept(),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
    ));
    fs::remove_dir_all(temp).unwrap();
}

#[test]
fn admin_status_cli_validates_invocation_id_before_credentials_or_network() {
    let temp = unique_temp_dir();
    fs::create_dir(&temp).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "admin-status",
            "--address",
            &address.to_string(),
            "--credentials",
            temp.join("missing.cred").to_str().unwrap(),
            "--invocation-id",
            "not-an-id",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("unknown command"), "{stderr}");
    assert!(
        stderr.contains("--invocation-id must be 32 lowercase hexadecimal digits"),
        "{stderr}"
    );
    assert!(matches!(
        listener.accept(),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
    ));
}

#[test]
fn run_sql_local_failures_do_not_connect_and_gateway_failures_clean_workspace() {
    let temp = unique_temp_dir();
    fs::create_dir(&temp).unwrap();
    let credentials = temp.join("client.cred");
    write_client_credentials(
        &credentials,
        &ClientCredentials {
            token_id: [0x41; 16],
            token_secret: [0x42; 32],
        },
    )
    .unwrap();

    let local_failures = [
        (
            "empty-sql",
            b" \n\t".to_vec(),
            None,
            "nonempty UTF-8",
        ),
        (
            "nul-sql",
            b"SELECT\0 1".to_vec(),
            None,
            "nonempty UTF-8",
        ),
        (
            "invalid-utf8-sql",
            vec![0xff],
            None,
            "nonempty UTF-8",
        ),
        (
            "oversized-sql",
            vec![b'x'; 65_537],
            None,
            "exceed their size limit",
        ),
        (
            "missing-sql-file",
            Vec::new(),
            None,
            "could not read SQL input",
        ),
        (
            "symlink-sql-file",
            b"SELECT 1".to_vec(),
            None,
            "could not read SQL input",
        ),
        (
            "malformed-parameters",
            b"SELECT 1".to_vec(),
            Some(br#"{"format":"PDB-SQL-PARAMETERS-1","values":[],"extra":true}"#.to_vec()),
            "invalid SQL parameter file",
        ),
        (
            "noncanonical-integer-parameter",
            b"SELECT $1".to_vec(),
            Some(br#"{"format":"PDB-SQL-PARAMETERS-1","values":[{"type":"integer","value":"042"}]}"#.to_vec()),
            "invalid SQL parameter file",
        ),
        (
            "malformed-bytea-parameter",
            b"SELECT $1".to_vec(),
            Some(br#"{"format":"PDB-SQL-PARAMETERS-1","values":[{"type":"bytea","value":"hex:0G"}]}"#.to_vec()),
            "invalid SQL parameter file",
        ),
        (
            "oversized-parameter-file",
            b"SELECT 1".to_vec(),
            Some(vec![b' '; 1024 * 1024 + 1]),
            "SQL parameter input exceeds 1 MiB",
        ),
    ];

    for (case, sql, parameters, expected_error) in local_failures {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let sql_path = temp.join(format!("{case}.sql"));
        match case {
            "missing-sql-file" => {}
            "symlink-sql-file" => {
                let target_path = temp.join("symlink-target.sql");
                fs::write(&target_path, &sql).unwrap();
                symlink(&target_path, &sql_path).unwrap();
            }
            _ => fs::write(&sql_path, &sql).unwrap(),
        }
        let parameter_path = temp.join(format!("{case}.json"));
        if let Some(parameters) = &parameters {
            fs::write(&parameter_path, parameters).unwrap();
        }

        let mut command = Command::new(env!("CARGO_BIN_EXE_prdb-client"));
        command.args([
            "run-sql",
            "--address",
            &address.to_string(),
            "--credentials",
            credentials.to_str().unwrap(),
            "--sql-file",
            sql_path.to_str().unwrap(),
            "--schema",
            "public",
            "--scope",
            temp.join("missing-scope.psc1").to_str().unwrap(),
            "--receipt",
            temp.join("receipt").to_str().unwrap(),
            "--sdk-library",
            temp.join("missing-sdk.so").to_str().unwrap(),
            "--include-dir",
            temp.join("missing-include").to_str().unwrap(),
            "--sdk-release",
            "1",
            "--cpu-baseline",
            "x86_64-v1",
            "--runtime-profile",
            "test-runtime",
            "--compiler-identity",
            "run-sql local validation test",
        ]);
        if parameters.is_some() {
            command.args(["--parameters", parameter_path.to_str().unwrap()]);
        }
        let output = command.output().unwrap();
        assert!(!output.status.success(), "{case} unexpectedly succeeded");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(expected_error),
            "{case} returned an unexpected error: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock),
            "{case} opened a gateway connection before local validation completed"
        );
    }

    let stdin_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    stdin_listener.set_nonblocking(true).unwrap();
    let stdin_address = stdin_listener.local_addr().unwrap();
    let stdin_parameters = temp.join("stdin-parameters.json");
    fs::write(
        &stdin_parameters,
        br#"{"format":"PDB-SQL-PARAMETERS-1","values":[],"extra":true}"#,
    )
    .unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .args([
            "run-sql",
            "--address",
            &stdin_address.to_string(),
            "--credentials",
            credentials.to_str().unwrap(),
            "--sql-file",
            "-",
            "--schema",
            "public",
            "--scope",
            temp.join("missing-stdin-scope.psc1").to_str().unwrap(),
            "--receipt",
            temp.join("stdin-receipt").to_str().unwrap(),
            "--sdk-library",
            temp.join("missing-stdin-sdk.so").to_str().unwrap(),
            "--include-dir",
            temp.join("missing-stdin-include").to_str().unwrap(),
            "--sdk-release",
            "1",
            "--cpu-baseline",
            "x86_64-v1",
            "--runtime-profile",
            "test-runtime",
            "--compiler-identity",
            "run-sql stdin validation test",
            "--parameters",
            stdin_parameters.to_str().unwrap(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"SELECT 1").unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        !output.status.success(),
        "stdin SQL with malformed parameters unexpectedly succeeded"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("invalid SQL parameter file"),
        "stdin SQL returned an unexpected local error: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        matches!(stdin_listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock),
        "run-sql opened a gateway connection after stdin validation failed"
    );

    let stdin_failures = [
        ("empty", Vec::new(), "nonempty UTF-8"),
        ("nul", b"SELECT\0 1".to_vec(), "nonempty UTF-8"),
        ("invalid-utf8", vec![0xff], "nonempty UTF-8"),
        ("oversized", vec![b'x'; 65_537], "exceed their size limit"),
    ];
    for (case, sql, expected_error) in stdin_failures {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
            .args([
                "run-sql",
                "--address",
                &address.to_string(),
                "--credentials",
                credentials.to_str().unwrap(),
                "--sql-file",
                "-",
                "--schema",
                "public",
                "--scope",
                temp.join("missing-stdin-scope.psc1").to_str().unwrap(),
                "--receipt",
                temp.join("stdin-error-receipt").to_str().unwrap(),
                "--sdk-library",
                temp.join("missing-stdin-sdk.so").to_str().unwrap(),
                "--include-dir",
                temp.join("missing-stdin-include").to_str().unwrap(),
                "--sdk-release",
                "1",
                "--cpu-baseline",
                "x86_64-v1",
                "--runtime-profile",
                "test-runtime",
                "--compiler-identity",
                "run-sql stdin error test",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(&sql).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            !output.status.success(),
            "stdin {case} unexpectedly succeeded"
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(expected_error),
            "stdin {case} returned an unexpected error: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock),
            "run-sql opened a gateway connection after stdin {case} validation failed"
        );
    }

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let sdk_build = temp.join("sql-sdk");
    let build = Command::new("make")
        .current_dir(&root)
        .args([
            "-f",
            "engine/sdk/Makefile",
            &format!("BUILD_DIR={}", sdk_build.display()),
            "SDK_RUNTIME_PROFILE=test-runtime",
            "sdk-shared-release",
        ])
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "SQL SDK build failed: {}",
        String::from_utf8_lossy(&build.stderr)
    );
    let sdk_release =
        sdk_build.join("releases/linux-x86_64/x86_64-v1/test-runtime/sdk-1-abis1,2,3,4,5");
    let sql_workspace_root = temp.join("sql-workspaces");
    fs::create_dir(&sql_workspace_root).unwrap();
    let scope_path = temp.join("valid-scope.psc1");
    let mut scope = b"PSC1\0\x01\x01\x01".to_vec();
    scope.extend_from_slice(&[0x51; 16]);
    fs::write(&scope_path, scope).unwrap();
    let invalid_scope_path = temp.join("invalid-scope.psc1");
    fs::write(&invalid_scope_path, b"not-a-PSC1-scope").unwrap();
    let sql_path = temp.join("valid.sql");
    fs::write(&sql_path, "SELECT 1").unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .env("TMPDIR", &sql_workspace_root)
        .args([
            "run-sql",
            "--address",
            &address.to_string(),
            "--credentials",
            credentials.to_str().unwrap(),
            "--sql-file",
            sql_path.to_str().unwrap(),
            "--schema",
            "public",
            "--scope",
            invalid_scope_path.to_str().unwrap(),
            "--receipt",
            temp.join("invalid-scope.receipt").to_str().unwrap(),
            "--sdk-library",
            sdk_release.join("libprdb_sql.so.1").to_str().unwrap(),
            "--include-dir",
            root.join("engine/include").to_str().unwrap(),
            "--sdk-release",
            "1",
            "--cpu-baseline",
            "x86_64-v1",
            "--runtime-profile",
            "test-runtime",
            "--compiler-identity",
            "run-sql invalid scope test",
        ])
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "invalid-scope package unexpectedly succeeded"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("InvalidScope"),
        "unexpected invalid-scope package error: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock),
        "run-sql opened a gateway connection after scope validation failed"
    );
    assert!(
        fs::read_dir(&sql_workspace_root).unwrap().next().is_none(),
        "run-sql left temporary build artifacts after scope validation failed"
    );

    let output = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .env("TMPDIR", &sql_workspace_root)
        .args([
            "run-sql",
            "--address",
            &address.to_string(),
            "--credentials",
            credentials.to_str().unwrap(),
            "--sql-file",
            sql_path.to_str().unwrap(),
            "--schema",
            "public",
            "--scope",
            scope_path.to_str().unwrap(),
            "--receipt",
            temp.join("invalid-sdk-profile.receipt").to_str().unwrap(),
            "--sdk-library",
            sdk_release.join("libprdb_sql.so.1").to_str().unwrap(),
            "--include-dir",
            root.join("engine/include").to_str().unwrap(),
            "--sdk-release",
            "2",
            "--cpu-baseline",
            "x86_64-v1",
            "--runtime-profile",
            "test-runtime",
            "--compiler-identity",
            "run-sql incompatible SDK test",
        ])
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "incompatible SDK profile unexpectedly succeeded"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("InvalidConfiguration"),
        "unexpected incompatible SDK profile error: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock),
        "run-sql opened a gateway connection after SDK profile validation failed"
    );
    assert!(
        fs::read_dir(&sql_workspace_root).unwrap().next().is_none(),
        "run-sql left temporary build artifacts after SDK profile validation failed"
    );

    fs::write(&sql_path, "SELECT $1").unwrap();
    let parameter_secret = "sqlcli-parameter-secret-4d3a";
    let parameter_path = temp.join("compiler-failure-parameters.json");
    fs::write(
        &parameter_path,
        format!(
            "{{\"format\":\"PDB-SQL-PARAMETERS-1\",\"values\":[{{\"type\":\"text\",\"value\":\"{parameter_secret}\"}}]}}"
        ),
    )
    .unwrap();
    let compiler_source_capture = temp.join("compiler-source.capture");
    let compiler_environment_capture = temp.join("compiler-environment.capture");
    let fake_tcc = temp.join("capture-compiler-env");
    fs::write(
        &fake_tcc,
        format!(
            "#!/bin/sh\nif [ \"${{PRDB_TEST_COMPILER_SECRET_SENTINEL+x}}\" = x ]; then printf leaked > '{}'; else printf clean > '{}'; fi\nwhile IFS= read -r line; do printf '%s\\n' \"$line\"; done < \"$8\" > '{}'\nexit 1\n",
            compiler_environment_capture.display(),
            compiler_environment_capture.display(),
            compiler_source_capture.display(),
        ),
    )
    .unwrap();
    fs::set_permissions(&fake_tcc, fs::Permissions::from_mode(0o700)).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .env("TMPDIR", &sql_workspace_root)
        .env(
            "PRDB_TEST_COMPILER_SECRET_SENTINEL",
            "credential-secret-sentinel",
        )
        .args([
            "run-sql",
            "--address",
            &address.to_string(),
            "--credentials",
            credentials.to_str().unwrap(),
            "--sql-file",
            sql_path.to_str().unwrap(),
            "--schema",
            "public",
            "--scope",
            scope_path.to_str().unwrap(),
            "--receipt",
            temp.join("compiler-failure.receipt").to_str().unwrap(),
            "--sdk-library",
            sdk_release.join("libprdb_sql.so.1").to_str().unwrap(),
            "--include-dir",
            root.join("engine/include").to_str().unwrap(),
            "--sdk-release",
            "1",
            "--cpu-baseline",
            "x86_64-v1",
            "--runtime-profile",
            "test-runtime",
            "--compiler-identity",
            "run-sql compiler failure test",
            "--parameters",
            parameter_path.to_str().unwrap(),
            "--tcc",
            fake_tcc.to_str().unwrap(),
            "--shared-linker",
            "cc",
            "--patchelf",
            "patchelf",
        ])
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "compiler failure unexpectedly succeeded"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("CompilerFailed"),
        "unexpected compiler failure: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock),
        "run-sql opened a gateway connection after compilation failed"
    );
    assert!(
        fs::read_dir(&sql_workspace_root).unwrap().next().is_none(),
        "run-sql left temporary build artifacts after compilation failed"
    );
    assert_eq!(
        fs::read_to_string(&compiler_environment_capture).unwrap(),
        "clean",
        "compiler inherited a parent environment secret"
    );
    let captured_source = fs::read(&compiler_source_capture).unwrap();
    assert!(
        !captured_source
            .windows(parameter_secret.len())
            .any(|window| window == parameter_secret.as_bytes()),
        "parameter data appeared in generated compiler source"
    );

    let patchelf_environment_capture = temp.join("patchelf-environment.capture");
    let fake_patchelf = temp.join("capture-patchelf-env");
    fs::write(
        &fake_patchelf,
        format!(
            "#!/bin/sh\nif [ \"${{PRDB_TEST_PATCHELF_SECRET_SENTINEL+x}}\" = x ]; then printf leaked > '{}'; else printf clean > '{}'; fi\nexit 1\n",
            patchelf_environment_capture.display(),
            patchelf_environment_capture.display(),
        ),
    )
    .unwrap();
    fs::set_permissions(&fake_patchelf, fs::Permissions::from_mode(0o700)).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .env("TMPDIR", &sql_workspace_root)
        .env(
            "PRDB_TEST_PATCHELF_SECRET_SENTINEL",
            "credential-secret-sentinel",
        )
        .args([
            "run-sql",
            "--address",
            &address.to_string(),
            "--credentials",
            credentials.to_str().unwrap(),
            "--sql-file",
            sql_path.to_str().unwrap(),
            "--schema",
            "public",
            "--scope",
            scope_path.to_str().unwrap(),
            "--receipt",
            temp.join("patchelf-failure.receipt").to_str().unwrap(),
            "--sdk-library",
            sdk_release.join("libprdb_sql.so.1").to_str().unwrap(),
            "--include-dir",
            root.join("engine/include").to_str().unwrap(),
            "--sdk-release",
            "1",
            "--cpu-baseline",
            "x86_64-v1",
            "--runtime-profile",
            "test-runtime",
            "--compiler-identity",
            "run-sql patchelf failure test",
            "--parameters",
            parameter_path.to_str().unwrap(),
            "--tcc",
            "tcc",
            "--shared-linker",
            "cc",
            "--patchelf",
            fake_patchelf.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "patchelf failure unexpectedly succeeded"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("CompilerFailed"),
        "unexpected patchelf failure: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(&patchelf_environment_capture).unwrap(),
        "clean",
        "patchelf inherited a parent environment secret"
    );
    assert!(
        matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock),
        "run-sql opened a gateway connection after patchelf failed"
    );
    assert!(
        fs::read_dir(&sql_workspace_root).unwrap().next().is_none(),
        "run-sql left temporary build artifacts after patchelf failed"
    );

    let linker_environment_capture = temp.join("linker-environment.capture");
    let fake_linker = temp.join("capture-linker-env");
    fs::write(
        &fake_linker,
        format!(
            "#!/bin/sh\nif [ \"${{PRDB_TEST_LINKER_SECRET_SENTINEL+x}}\" = x ]; then printf leaked > '{}'; else printf clean > '{}'; fi\nexit 1\n",
            linker_environment_capture.display(),
            linker_environment_capture.display(),
        ),
    )
    .unwrap();
    fs::set_permissions(&fake_linker, fs::Permissions::from_mode(0o700)).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .env("TMPDIR", &sql_workspace_root)
        .env(
            "PRDB_TEST_LINKER_SECRET_SENTINEL",
            "credential-secret-sentinel",
        )
        .args([
            "run-sql",
            "--address",
            &address.to_string(),
            "--credentials",
            credentials.to_str().unwrap(),
            "--sql-file",
            sql_path.to_str().unwrap(),
            "--schema",
            "public",
            "--scope",
            scope_path.to_str().unwrap(),
            "--receipt",
            temp.join("linker-failure.receipt").to_str().unwrap(),
            "--sdk-library",
            sdk_release.join("libprdb_sql.so.1").to_str().unwrap(),
            "--include-dir",
            root.join("engine/include").to_str().unwrap(),
            "--sdk-release",
            "1",
            "--cpu-baseline",
            "x86_64-v1",
            "--runtime-profile",
            "test-runtime",
            "--compiler-identity",
            "run-sql linker failure test",
            "--tcc",
            "tcc",
            "--shared-linker",
            fake_linker.to_str().unwrap(),
            "--patchelf",
            "patchelf",
        ])
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "shared linker failure unexpectedly succeeded"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("CompilerFailed"),
        "unexpected shared linker failure: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(&linker_environment_capture).unwrap(),
        "clean",
        "shared linker inherited a parent environment secret"
    );
    assert!(
        matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock),
        "run-sql opened a gateway connection after linking failed"
    );
    assert!(
        fs::read_dir(&sql_workspace_root).unwrap().next().is_none(),
        "run-sql left temporary build artifacts after linking failed"
    );

    let gateway_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let gateway_address = gateway_listener.local_addr().unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .env("TMPDIR", &sql_workspace_root)
        .args([
            "run-sql",
            "--address",
            &gateway_address.to_string(),
            "--credentials",
            credentials.to_str().unwrap(),
            "--sql-file",
            sql_path.to_str().unwrap(),
            "--schema",
            "public",
            "--scope",
            scope_path.to_str().unwrap(),
            "--receipt",
            temp.join("gateway-failure.receipt").to_str().unwrap(),
            "--sdk-library",
            sdk_release.join("libprdb_sql.so.1").to_str().unwrap(),
            "--include-dir",
            root.join("engine/include").to_str().unwrap(),
            "--sdk-release",
            "1",
            "--cpu-baseline",
            "x86_64-v1",
            "--runtime-profile",
            "test-runtime",
            "--compiler-identity",
            "run-sql gateway failure test",
            "--parameters",
            parameter_path.to_str().unwrap(),
            "--tcc",
            "tcc",
            "--shared-linker",
            "cc",
            "--patchelf",
            "patchelf",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let (gateway_stream, _) = gateway_listener.accept().unwrap();
    drop(gateway_stream);
    let output = child.wait_with_output().unwrap();
    assert!(
        !output.status.success(),
        "run-sql unexpectedly succeeded after the gateway closed the connection"
    );
    assert!(
        fs::read_dir(&sql_workspace_root).unwrap().next().is_none(),
        "run-sql left temporary build artifacts after gateway submission failed"
    );

    fs::remove_dir_all(temp).unwrap();
}

#[test]
fn generated_query_artifact_is_independent_of_parameter_values() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let temp = unique_temp_dir();
    fs::create_dir(&temp).unwrap();
    let sdk_build = temp.join("sdk");
    let sdk_release =
        sdk_build.join("releases/linux-x86_64/x86_64-v1/test-runtime/sdk-1-abis1,2,3,4,5");
    let source_path = temp.join("query.c");
    let scope_path = temp.join("scope.psc1");
    let mut scope = b"PSC1\0\x01\x01\x01".to_vec();
    scope.extend_from_slice(&[0x35; 16]);
    fs::write(&scope_path, scope).unwrap();
    fs::write(
        &source_path,
        generate_query_source("public", b"SELECT $1").unwrap(),
    )
    .unwrap();

    let build = Command::new("make")
        .current_dir(&root)
        .args([
            "-f",
            "engine/sdk/Makefile",
            &format!("BUILD_DIR={}", sdk_build.display()),
            "SDK_RUNTIME_PROFILE=test-runtime",
            "sdk-shared-release",
        ])
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "SQL SDK build failed: {}",
        String::from_utf8_lossy(&build.stderr)
    );

    let parameter_values = [
        "first-parameter-secret-7e91",
        "second-parameter-secret-0b24",
    ];
    let mut artifact_bytes = Vec::new();
    let mut parameter_blocks = Vec::new();
    for (index, value) in parameter_values.iter().enumerate() {
        let parameter_json = format!(
            "{{\"format\":\"PDB-SQL-PARAMETERS-1\",\"values\":[{{\"type\":\"text\",\"value\":\"{value}\"}}]}}"
        );
        parameter_blocks.push(encode_parameter_file(parameter_json.as_bytes()).unwrap());
        let package_path = temp.join(format!("package-{index}"));
        let status = Command::new(env!("CARGO_BIN_EXE_prdb-packager"))
            .arg("--source")
            .arg(&source_path)
            .arg("--sdk-library")
            .arg(sdk_release.join("libprdb_sql.so.1"))
            .arg("--shared-linker")
            .arg("cc")
            .arg("--include-dir")
            .arg(root.join("engine/include"))
            .arg("--scope")
            .arg(&scope_path)
            .arg("--output-dir")
            .arg(&package_path)
            .args([
                "--abi",
                "3",
                "--sdk-release",
                "1",
                "--cpu-baseline",
                "x86_64-v1",
                "--runtime-profile",
                "test-runtime",
                "--compiler-identity",
                "TinyCC SQL parameter separation test",
            ])
            .output()
            .unwrap();
        assert!(
            status.status.success(),
            "package {index} failed: stdout={} stderr={}",
            String::from_utf8_lossy(&status.stdout),
            String::from_utf8_lossy(&status.stderr)
        );
        artifact_bytes.push(fs::read(package_path.join("query.so")).unwrap());
    }

    assert_ne!(
        parameter_blocks[0].as_slice(),
        parameter_blocks[1].as_slice()
    );
    assert_eq!(artifact_bytes[0], artifact_bytes[1]);
    for (index, artifact) in artifact_bytes.iter().enumerate() {
        assert!(
            !artifact
                .windows(parameter_values[index].len())
                .any(|window| window == parameter_values[index].as_bytes()),
            "parameter value {index} appeared in query.so"
        );
    }
    fs::remove_dir_all(temp).unwrap();
}

#[test]
fn packager_compiles_and_atomically_publishes_metadata_without_loading_query() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let temp = unique_temp_dir();
    fs::create_dir(&temp).unwrap();
    let sdk_build = temp.join("sdk");
    let source = temp.join("query.c");
    let scope = temp.join("scope.psc1");
    let package = temp.join("package");
    fs::write(
        &source,
        b"#include \"prdb_query_abi_v4.h\"\n\
          #include <stdlib.h>\n\
          const prdb_query_descriptor_v4_record prdb_query_descriptor_v4 = {\n\
            sizeof(prdb_query_descriptor_v4_record), 4, sizeof(prdb_host_api_v4), sizeof(prdb_invocation_v4)};\n\
          __attribute__((constructor)) static void must_not_load(void) { abort(); }\n\
          int32_t prdb_query_main_v4(const prdb_host_api_v4 *h, const prdb_invocation_v4 *i) { (void)h; (void)i; return 0; }\n",
    )
    .unwrap();
    let mut scope_bytes = b"PSC1\0\x01\x02\x02".to_vec();
    scope_bytes.extend([1; 16]);
    fs::write(&scope, &scope_bytes).unwrap();

    let build = Command::new("make")
        .current_dir(&root)
        .args([
            "-f",
            "engine/sdk/Makefile",
            &format!("BUILD_DIR={}", sdk_build.display()),
            "SDK_RUNTIME_PROFILE=test-runtime",
            "sdk-shared-release",
        ])
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );

    let release_dir =
        sdk_build.join("releases/linux-x86_64/x86_64-v1/test-runtime/sdk-1-abis1,2,3,4,5");
    let library = release_dir.join("libprdb_sql.so.1");
    let status = Command::new(env!("CARGO_BIN_EXE_prdb-packager"))
        .arg("--legacy-package")
        .arg("--source")
        .arg(&source)
        .arg("--sdk-library")
        .arg(&library)
        .arg("--shared-linker")
        .arg("cc")
        .arg("--include-dir")
        .arg(root.join("engine/include"))
        .arg("--scope")
        .arg(&scope)
        .arg("--output-dir")
        .arg(&package)
        .args([
            "--abi",
            "4",
            "--sdk-release",
            "1",
            "--cpu-baseline",
            "x86_64-v1",
            "--runtime-profile",
            "test-runtime",
            "--compiler-identity",
            "TinyCC integration test",
        ])
        .output()
        .unwrap();
    assert!(
        status.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&status.stdout),
        String::from_utf8_lossy(&status.stderr)
    );

    let artifact = fs::read(package.join("query.so")).unwrap();
    let supplied_sql_library = fs::read(package.join("sql.so")).unwrap();
    let manifest_bytes = fs::read(package.join("manifest.pmf1")).unwrap();
    let manifest = decode_manifest_v1(&manifest_bytes).unwrap();
    assert_eq!(manifest.native_abi_version, 4);
    assert_eq!(manifest.sdk_release, 1);
    assert_eq!(manifest.cpu_baseline, "x86_64-v1");
    assert_eq!(manifest.runtime_profile, "test-runtime");
    assert_eq!(manifest.descriptor_symbol, "prdb_query_descriptor_v4");
    assert_eq!(manifest.entry_point_symbol, "prdb_query_main_v4");
    assert_eq!(
        fs::read(package.join("requested_scope.psc1")).unwrap(),
        scope_bytes
    );
    let digest = Sha256::digest(&artifact)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
        + "\n";
    assert_eq!(
        fs::read_to_string(package.join("artifact.sha256")).unwrap(),
        digest
    );
    assert_eq!(
        fs::read_to_string(package.join("sql-library.sha256")).unwrap(),
        Sha256::digest(&supplied_sql_library)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
            + "\n"
    );
    let dynamic = decode_elf_dynamic_info(&artifact).unwrap();
    assert_eq!(dynamic.runpath.as_deref(), Some("$ORIGIN"));
    assert!(dynamic.rpath.is_none());
    assert!(dynamic.needed.iter().any(|name| name == "libprdb_sql.so.1"));
    assert!(!temp.join("constructor-ran").exists());

    let invalid_runpath_artifact = temp.join("query-invalid-runpath.so");
    fs::write(&invalid_runpath_artifact, &artifact).unwrap();
    let invalid_runpath = Command::new("patchelf")
        .arg("--set-rpath")
        .arg("$ORIGIN:/tmp")
        .arg(&invalid_runpath_artifact)
        .status()
        .unwrap();
    assert!(invalid_runpath.success());
    let metadata = SubmissionMetadata {
        manifest_version: 1,
        manifest: manifest_bytes.clone(),
        scope_version: 1,
        requested_scope: scope_bytes.clone(),
        deadline_ms: 30_000,
        workload_class: WorkloadClass::Operational,
    };
    let profile = ArtifactCompatibilityProfile::new(
        manifest.cpu_baseline.clone(),
        manifest.runtime_profile.clone(),
        manifest.sdk_release,
        manifest.dependencies.iter().cloned().collect(),
    )
    .unwrap();
    let validator = VersionedSubmissionMetadataValidator::new(profile);
    assert!(!validator.validate_artifact(&metadata, &fs::read(&invalid_runpath_artifact).unwrap()));
    assert!(validator.validate_sql_library(&metadata, &supplied_sql_library));
    let invalid_library = temp.join("sql-invalid-runpath.so");
    fs::write(&invalid_library, &supplied_sql_library).unwrap();
    assert!(
        Command::new("patchelf")
            .arg("--set-rpath")
            .arg("/tmp")
            .arg(&invalid_library)
            .status()
            .unwrap()
            .success()
    );
    assert!(!validator.validate_sql_library(&metadata, &fs::read(invalid_library).unwrap()));

    let rejected_release = Command::new(env!("CARGO_BIN_EXE_prdb-packager"))
        .arg("--source")
        .arg(&source)
        .arg("--sdk-library")
        .arg(&library)
        .arg("--shared-linker")
        .arg("cc")
        .arg("--include-dir")
        .arg(root.join("engine/include"))
        .arg("--scope")
        .arg(&scope)
        .arg("--output-dir")
        .arg(temp.join("rejected-release-package"))
        .args([
            "--abi",
            "4",
            "--sdk-release",
            "2",
            "--cpu-baseline",
            "x86_64-v1",
            "--runtime-profile",
            "test-runtime",
            "--compiler-identity",
            "TinyCC integration test",
        ])
        .output()
        .unwrap();
    assert!(!rejected_release.status.success());
    assert!(!temp.join("rejected-release-package").exists());

    let expected_artifact = artifact.clone();
    let expected_sql_library = supplied_sql_library.clone();
    let expected_manifest = manifest_bytes.clone();
    let expected_scope = scope_bytes.clone();
    let parameters = vec![0x5a; 300_000];
    let expected_parameters = parameters.clone();
    let receipt_path = temp.join("invocation.receipt");
    let server_receipt_path = receipt_path.clone();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(10)))
            .unwrap();
        let mut decoder = ClientFrameDecoder::default();
        let hello = read_client_frame(&mut stream, &mut decoder);
        assert_eq!(hello.kind, 1);
        write_server_response(&mut stream, &hello, br#"{"transport":"plaintext_tcp","limits":{"chunk_bytes":131072,"control_bytes":32768,"payload_bytes":524288},"mode":"query","minor":0,"major":1}"#);
        let auth = read_client_frame(&mut stream, &mut decoder);
        assert_eq!(auth.kind, 2);
        let decoded_credentials = decode_client_credentials(&auth.payload).unwrap();
        assert_eq!(decoded_credentials.token_id, [7; 16]);
        assert_eq!(decoded_credentials.token_secret, [9; 32]);
        let mut decoded_credentials = decoded_credentials;
        decoded_credentials.token_secret.zeroize();
        write_server_response(
            &mut stream,
            &auth,
            br#"{"session_id":"abababababababababababababababab","authenticated":true}"#,
        );
        let prepare = read_client_frame(&mut stream, &mut decoder);
        assert_eq!(prepare.kind, 3);
        let decoded = decode_client_prepare(&prepare.payload).unwrap();
        assert_eq!(decoded.artifact_bytes as usize, expected_artifact.len());
        assert_eq!(
            decoded.sql_library_bytes as usize,
            expected_sql_library.len()
        );
        assert_eq!(
            decoded.sql_library_sha256,
            Sha256::digest(&expected_sql_library).as_slice()
        );
        assert_eq!(decoded.manifest, expected_manifest);
        assert_eq!(decoded.requested_scope, expected_scope);
        assert_eq!(decoded.parameter_bytes as usize, expected_parameters.len());
        let expected_parameter_hash: [u8; 32] = Sha256::digest(&expected_parameters).into();
        assert_eq!(decoded.parameter_sha256, expected_parameter_hash);
        write_server_response(&mut stream, &prepare, br#"{"invocation_id":"01010101010101010101010101010101","artifact_chunk_bytes":131072,"parameter_chunk_bytes":131072,"sql_library_chunk_bytes":131072}"#);
        let mut offset = 0usize;
        while offset < expected_artifact.len() {
            let frame = read_client_frame(&mut stream, &mut decoder);
            assert_eq!(frame.kind, 4);
            let chunk = decode_client_chunk(&frame.payload).unwrap();
            assert_eq!(chunk.invocation_id, [1; 16]);
            assert_eq!(chunk.offset, offset as u64);
            let next_offset = offset + chunk.data.len();
            assert_eq!(chunk.data, expected_artifact[offset..next_offset]);
            offset = next_offset;
            write_server_response(
                &mut stream,
                &frame,
                format!("{{\"next_offset\":\"{offset}\"}}").as_bytes(),
            );
        }
        let mut parameter_offset = 0usize;
        while parameter_offset < expected_parameters.len() {
            let frame = read_client_frame(&mut stream, &mut decoder);
            assert_eq!(frame.kind, 5);
            let chunk = decode_client_chunk(&frame.payload).unwrap();
            assert_eq!(chunk.invocation_id, [1; 16]);
            assert_eq!(chunk.offset, parameter_offset as u64);
            let next_offset = parameter_offset + chunk.data.len();
            assert_eq!(
                chunk.data,
                expected_parameters[parameter_offset..next_offset]
            );
            parameter_offset = next_offset;
            write_server_response(
                &mut stream,
                &frame,
                format!("{{\"next_offset\":\"{parameter_offset}\"}}").as_bytes(),
            );
        }
        let mut sql_library_offset = 0usize;
        while sql_library_offset < expected_sql_library.len() {
            let frame = read_client_frame(&mut stream, &mut decoder);
            assert_eq!(frame.kind, 12);
            let chunk = decode_client_chunk(&frame.payload).unwrap();
            assert_eq!(chunk.invocation_id, [1; 16]);
            assert_eq!(chunk.offset, sql_library_offset as u64);
            let next_offset = sql_library_offset + chunk.data.len();
            assert_eq!(
                chunk.data,
                expected_sql_library[sql_library_offset..next_offset]
            );
            sql_library_offset = next_offset;
            write_server_response(
                &mut stream,
                &frame,
                format!("{{\"next_offset\":\"{sql_library_offset}\"}}").as_bytes(),
            );
        }
        let seal = read_client_frame(&mut stream, &mut decoder);
        assert_eq!(seal.kind, 6);
        write_server_response(&mut stream, &seal, br#"{"state":"prepared"}"#);
        let start = read_client_frame(&mut stream, &mut decoder);
        assert_eq!(start.kind, 7);
        assert_eq!(
            fs::read_to_string(&server_receipt_path).unwrap(),
            "PDBR1 01010101010101010101010101010101\n"
        );
        let mut stream_bytes = ClientFrame {
            kind: 100,
            request_id: start.request_id,
            response_sequence: 0,
            payload: br#"{"state":"accepted"}"#.to_vec(),
        }
        .encode()
        .unwrap();
        stream_bytes.extend(
            ClientFrame {
                kind: 102,
                request_id: start.request_id,
                response_sequence: 1,
                payload: b"provisional-row".to_vec(),
            }
            .encode()
            .unwrap(),
        );
        stream_bytes.extend(
            ClientFrame {
                kind: 103,
                request_id: start.request_id,
                response_sequence: 2,
                payload: br#"{"invocation_id":"01010101010101010101010101010101","outcome":"committed","reason_code":"none"}"#.to_vec(),
            }
            .encode()
            .unwrap(),
        );
        stream.write_all(&stream_bytes).unwrap();
        let status = read_client_frame(&mut stream, &mut decoder);
        assert_eq!(status.kind, 8);
        write_server_response(&mut stream, &status, br#"{"state":"committed"}"#);
    });
    let mut token_secret = [9; 32];
    let mut client =
        SubmissionClient::connect_authenticated(address, [7; 16], &mut token_secret).unwrap();
    assert_eq!(token_secret, [0; 32]);
    assert_eq!(
        client
            .upload_and_seal(
                &package,
                &parameters,
                30_000,
                prdb_engine::supervisor::WorkloadClass::Operational,
            )
            .unwrap(),
        [1; 16]
    );
    {
        let mut started = client.start([1; 16], &receipt_path).unwrap();
        assert_eq!(
            started.next_event().unwrap(),
            Some(prdb_engine::client_submit::ClientResultEvent::Data(
                b"provisional-row".to_vec()
            ))
        );
        assert!(matches!(
            started.next_event().unwrap(),
            Some(prdb_engine::client_submit::ClientResultEvent::Terminal(terminal))
                if terminal.invocation_id == [1; 16]
                    && terminal.outcome == prdb_engine::client_submit::TerminalOutcome::Committed
                    && terminal.reason_code == "none"
        ));
        assert_eq!(started.next_event().unwrap(), None);
    }
    assert!(client.start([1; 16], &receipt_path).is_err());
    assert_eq!(
        client.status([1; 16]).unwrap(),
        prdb_engine::client_submit::InvocationStatus::Committed
    );
    server.join().unwrap();

    let repository_package = temp.join("repository-package");
    let repository_packager = Command::new(env!("CARGO_BIN_EXE_prdb-packager"))
        .arg("--source")
        .arg(&source)
        .arg("--sdk-library")
        .arg(&library)
        .arg("--shared-linker")
        .arg("cc")
        .arg("--include-dir")
        .arg(root.join("engine/include"))
        .arg("--scope")
        .arg(&scope)
        .arg("--output-dir")
        .arg(&repository_package)
        .args([
            "--abi",
            "4",
            "--sdk-release",
            "1",
            "--cpu-baseline",
            "x86_64-v1",
            "--runtime-profile",
            "test-runtime",
            "--compiler-identity",
            "TinyCC integration test",
        ])
        .output()
        .unwrap();
    assert!(
        repository_packager.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&repository_packager.stdout),
        String::from_utf8_lossy(&repository_packager.stderr)
    );
    assert!(repository_package.join("query.so").is_file());
    assert!(repository_package.join("manifest.pmf1").is_file());
    assert!(!repository_package.join("sql.so").exists());
    assert!(!repository_package.join("sql-library.sha256").exists());
    let repository_artifact = artifact.clone();
    let repository_manifest = manifest_bytes.clone();
    let repository_scope = scope_bytes.clone();
    let repository_parameters = parameters.clone();
    let repository_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let repository_address = repository_listener.local_addr().unwrap();
    let repository_server = std::thread::spawn(move || {
        let (mut stream, _) = repository_listener.accept().unwrap();
        let mut decoder = ClientFrameDecoder::default();
        let hello = read_client_frame(&mut stream, &mut decoder);
        assert_eq!(hello.kind, 1);
        assert!(
            std::str::from_utf8(&hello.payload)
                .unwrap()
                .contains(r#""sdk_library_mode":"repository""#)
        );
        write_server_response(
            &mut stream,
            &hello,
            br#"{"transport":"plaintext_tcp","limits":{"chunk_bytes":131072,"control_bytes":32768,"payload_bytes":524288},"mode":"query","sdk_library_mode":"repository","minor":0,"major":1}"#,
        );
        let auth = read_client_frame(&mut stream, &mut decoder);
        assert_eq!(auth.kind, 2);
        let decoded_credentials = decode_client_credentials(&auth.payload).unwrap();
        assert_eq!(decoded_credentials.token_id, [7; 16]);
        assert_eq!(decoded_credentials.token_secret, [9; 32]);
        let mut decoded_credentials = decoded_credentials;
        decoded_credentials.token_secret.zeroize();
        write_server_response(
            &mut stream,
            &auth,
            br#"{"session_id":"abababababababababababababababab","authenticated":true}"#,
        );
        let prepare = read_client_frame(&mut stream, &mut decoder);
        assert_eq!(prepare.kind, 3);
        let decoded = decode_client_prepare(&prepare.payload).unwrap();
        assert!(decoded.repository_sdk_library);
        assert_eq!(decoded.sql_library_bytes, 0);
        assert_eq!(decoded.manifest, repository_manifest);
        assert_eq!(decoded.requested_scope, repository_scope);
        assert_eq!(decoded.artifact_bytes as usize, repository_artifact.len());
        assert_eq!(
            decoded.parameter_bytes as usize,
            repository_parameters.len()
        );
        write_server_response(
            &mut stream,
            &prepare,
            br#"{"invocation_id":"02020202020202020202020202020202","artifact_chunk_bytes":131072,"parameter_chunk_bytes":131072}"#,
        );
        for (kind, bytes) in [(4, repository_artifact), (5, repository_parameters)] {
            let mut offset = 0usize;
            while offset < bytes.len() {
                let frame = read_client_frame(&mut stream, &mut decoder);
                assert_eq!(frame.kind, kind);
                let chunk = decode_client_chunk(&frame.payload).unwrap();
                assert_eq!(chunk.invocation_id, [2; 16]);
                assert_eq!(chunk.offset, offset as u64);
                let next_offset = offset + chunk.data.len();
                assert_eq!(chunk.data, bytes[offset..next_offset]);
                offset = next_offset;
                write_server_response(
                    &mut stream,
                    &frame,
                    format!(r#"{{"next_offset":"{offset}"}}"#).as_bytes(),
                );
            }
        }
        let seal = read_client_frame(&mut stream, &mut decoder);
        assert_eq!(seal.kind, 6);
        write_server_response(&mut stream, &seal, br#"{"state":"prepared"}"#);
        let start = read_client_frame(&mut stream, &mut decoder);
        assert_eq!(start.kind, 7);
        let mut events = ClientFrame {
            kind: 100,
            request_id: start.request_id,
            response_sequence: 0,
            payload: br#"{"state":"accepted"}"#.to_vec(),
        }
        .encode()
        .unwrap();
        events.extend(
            ClientFrame {
                kind: 103,
                request_id: start.request_id,
                response_sequence: 1,
                payload: br#"{"invocation_id":"02020202020202020202020202020202","outcome":"committed","reason_code":"none"}"#.to_vec(),
            }
            .encode()
            .unwrap(),
        );
        stream.write_all(&events).unwrap();
    });
    let config_dir = temp.join("client-config");
    fs::create_dir(&config_dir).unwrap();
    fs::set_permissions(&config_dir, fs::Permissions::from_mode(0o700)).unwrap();
    let client_config_path = config_dir.join("client.json");
    let local_sdk_root = temp.join("local-sdk");
    let local_include = local_sdk_root.join("include");
    fs::create_dir_all(&local_include).unwrap();
    for entry in fs::read_dir(root.join("engine/include")).unwrap() {
        let entry = entry.unwrap();
        fs::copy(entry.path(), local_include.join(entry.file_name())).unwrap();
    }
    let local_release =
        local_sdk_root.join("releases/linux-x86_64/x86_64-v1/test-runtime/sdk-1-abis1,2,3,4,5");
    fs::create_dir_all(&local_release).unwrap();
    for entry in fs::read_dir(&release_dir).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_file() {
            fs::copy(entry.path(), local_release.join(entry.file_name())).unwrap();
        }
    }
    ClientConfig {
        format: CONFIG_FORMAT.to_owned(),
        default_instance: Some("local".to_owned()),
        client_tools: Some(ClientTools {
            include_dir: local_include.display().to_string(),
            tcc: "tcc".to_owned(),
            shared_linker: "cc".to_owned(),
            patchelf: "patchelf".to_owned(),
        }),
        instances: vec![ClientInstance {
            name: "local".to_owned(),
            address: repository_address.ip().to_string(),
            port: repository_address.port(),
            sdk: SdkSelection {
                release: 1,
                target: "linux-x86_64".to_owned(),
                cpu_baseline: "x86_64-v1".to_owned(),
                runtime_profile: "test-runtime".to_owned(),
                native_abi: 4,
            },
            default_credential: "alice".to_owned(),
            credentials: vec![ConfigCredential {
                name: "alice".to_owned(),
                username: "alice".to_owned(),
                token_id: "07".repeat(16),
                token_secret: encode_token_secret(&[9; 32]),
            }],
        }],
    }
    .save_atomic(&client_config_path)
    .unwrap();
    let repository_parameters_path = temp.join("repository-parameters.bin");
    fs::write(&repository_parameters_path, &parameters).unwrap();
    let repository_receipt = temp.join("repository.receipt");
    let client_output = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .arg("submit")
        .arg("--config")
        .arg(&client_config_path)
        .arg("--instance")
        .arg("local")
        .arg("--package")
        .arg(&repository_package)
        .arg("--receipt")
        .arg(&repository_receipt)
        .arg("--parameters")
        .arg(repository_parameters_path)
        .output()
        .unwrap();
    assert!(
        client_output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&client_output.stdout),
        String::from_utf8_lossy(&client_output.stderr)
    );
    assert_eq!(
        fs::read_to_string(repository_receipt).unwrap(),
        "PDBR1 02020202020202020202020202020202\n"
    );
    repository_server.join().unwrap();

    let mut configured = ClientConfig::load(&client_config_path).unwrap();
    configured.instances[0].sdk.native_abi = 3;
    configured.save_atomic(&client_config_path).unwrap();
    let sql_path = temp.join("configured-run-sql.sql");
    fs::write(&sql_path, "SELECT 1").unwrap();
    let run_sql_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let run_sql_address = run_sql_listener.local_addr().unwrap();
    configured.instances[0].address = run_sql_address.ip().to_string();
    configured.instances[0].port = run_sql_address.port();
    configured.save_atomic(&client_config_path).unwrap();
    let run_sql_scope = scope_bytes.clone();
    let run_sql_server = std::thread::spawn(move || {
        let (mut stream, _) = run_sql_listener.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(10)))
            .unwrap();
        let mut decoder = ClientFrameDecoder::default();
        let hello = read_client_frame(&mut stream, &mut decoder);
        assert_eq!(hello.kind, 1);
        assert!(
            String::from_utf8_lossy(&hello.payload).contains(r#""sdk_library_mode":"repository""#)
        );
        write_server_response(
            &mut stream,
            &hello,
            br#"{"transport":"plaintext_tcp","limits":{"chunk_bytes":131072,"control_bytes":32768,"payload_bytes":524288},"mode":"query","sdk_library_mode":"repository","minor":0,"major":1}"#,
        );
        let auth = read_client_frame(&mut stream, &mut decoder);
        assert_eq!(auth.kind, 2);
        write_server_response(
            &mut stream,
            &auth,
            br#"{"session_id":"abababababababababababababababab","authenticated":true}"#,
        );
        let prepare = read_client_frame(&mut stream, &mut decoder);
        assert_eq!(prepare.kind, 3);
        let decoded = decode_client_prepare(&prepare.payload).unwrap();
        assert!(decoded.repository_sdk_library);
        assert_eq!(decoded.sql_library_bytes, 0);
        assert_eq!(decoded.parameter_bytes, 0);
        assert_eq!(decoded.requested_scope, run_sql_scope);
        let manifest = decode_manifest_v1(&decoded.manifest).unwrap();
        assert_eq!(manifest.native_abi_version, 3);
        assert_eq!(manifest.sdk_release, 1);
        write_server_response(
            &mut stream,
            &prepare,
            br#"{"invocation_id":"03030303030303030303030303030303","artifact_chunk_bytes":131072,"parameter_chunk_bytes":131072}"#,
        );
        let mut offset = 0usize;
        while offset < decoded.artifact_bytes as usize {
            let frame = read_client_frame(&mut stream, &mut decoder);
            assert_eq!(frame.kind, 4);
            let chunk = decode_client_chunk(&frame.payload).unwrap();
            assert_eq!(chunk.offset, offset as u64);
            offset += chunk.data.len();
            write_server_response(
                &mut stream,
                &frame,
                format!(r#"{{"next_offset":"{offset}"}}"#).as_bytes(),
            );
        }
        let seal = read_client_frame(&mut stream, &mut decoder);
        assert_eq!(seal.kind, 6);
        write_server_response(&mut stream, &seal, br#"{"state":"prepared"}"#);
        let start = read_client_frame(&mut stream, &mut decoder);
        assert_eq!(start.kind, 7);
        let mut events = ClientFrame {
            kind: 100,
            request_id: start.request_id,
            response_sequence: 0,
            payload: br#"{"state":"accepted"}"#.to_vec(),
        }
        .encode()
        .unwrap();
        events.extend(
            ClientFrame {
                kind: 103,
                request_id: start.request_id,
                response_sequence: 1,
                payload: br#"{"invocation_id":"03030303030303030303030303030303","outcome":"committed","reason_code":"none"}"#.to_vec(),
            }
            .encode()
            .unwrap(),
        );
        stream.write_all(&events).unwrap();
    });
    let run_sql_receipt = temp.join("configured-run-sql.receipt");
    let run_sql_output = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .arg("run-sql")
        .arg("--config")
        .arg(&client_config_path)
        .arg("--instance")
        .arg("local")
        .arg("--sql-file")
        .arg(&sql_path)
        .arg("--schema")
        .arg("app")
        .arg("--scope")
        .arg(&scope)
        .arg("--receipt")
        .arg(&run_sql_receipt)
        .output()
        .unwrap();
    assert!(
        run_sql_output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&run_sql_output.stdout),
        String::from_utf8_lossy(&run_sql_output.stderr)
    );
    run_sql_server.join().unwrap();

    let interactive_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let interactive_address = interactive_listener.local_addr().unwrap();
    configured.instances[0].address = interactive_address.ip().to_string();
    configured.instances[0].port = interactive_address.port();
    configured.instances[0].sdk.native_abi = 3;
    configured.save_atomic(&client_config_path).unwrap();
    let credentials = config_dir.join("alice.credentials");
    write_client_credentials(
        &credentials,
        &ClientCredentials {
            token_id: [7; 16],
            token_secret: [9; 32],
        },
    )
    .unwrap();
    let interactive_sql_path = temp.join("configured-interactive.sql");
    fs::write(&interactive_sql_path, "BEGIN; COMMIT;").unwrap();
    let (began_tx, began_rx) = mpsc::channel();
    let (continue_tx, continue_rx) = mpsc::channel();
    let interactive_server = std::thread::spawn(move || {
        let accept_transaction_control = |mut stream: TcpStream, expected_kind: u16| {
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(10)))
                .unwrap();
            let mut decoder = ClientFrameDecoder::negotiating();
            let hello = read_client_frame(&mut stream, &mut decoder);
            assert_eq!(hello.kind, 1);
            assert!(
                String::from_utf8_lossy(&hello.payload)
                    .contains(r#""sdk_library_mode":"repository""#)
            );
            write_server_response_v2(
                &mut stream,
                &hello,
                br#"{"transport":"plaintext_tcp","limits":{"chunk_bytes":131072,"control_bytes":32768,"payload_bytes":524288},"mode":"query","sdk_library_mode":"repository","minor":0,"major":2}"#,
            );
            let auth = read_client_frame(&mut stream, &mut decoder);
            assert_eq!(auth.kind, 2);
            write_server_response_v2(
                &mut stream,
                &auth,
                br#"{"session_id":"abababababababababababababababab","authenticated":true}"#,
            );
            let request = read_client_frame(&mut stream, &mut decoder);
            assert_eq!(request.kind, expected_kind);
            (stream, decoder, request)
        };

        let (mut stream, _decoder, begin) = accept_transaction_control(
            interactive_listener.accept().unwrap().0,
            prdb_engine::client_protocol::CLIENT_V2_TX_BEGIN_KIND,
        );
        let begin_payload =
            prdb_engine::client_protocol::ClientV2Request::decode(begin.kind, &begin.payload)
                .unwrap();
        assert!(matches!(
            begin_payload,
            prdb_engine::client_protocol::ClientV2Request::TxBegin { .. }
        ));
        let begun = prdb_engine::client_protocol::ClientV2Response::TransactionBegun {
            transaction_id: [0x44; 16],
            state: prdb_engine::client_protocol::ClientV2TransactionState::Active,
            idle_timeout_ms: 30_000,
            lifetime_ms: 3_600_000,
            max_package_bytes: 1_048_576,
            max_packages: 64,
        };
        write_server_response_v2(&mut stream, &begin, &begun.encode());
        began_tx.send(()).unwrap();
        continue_rx.recv().unwrap();

        let (mut stream, _decoder, commit) = accept_transaction_control(
            interactive_listener.accept().unwrap().0,
            prdb_engine::client_protocol::CLIENT_V2_TX_COMMIT_KIND,
        );
        let committed = prdb_engine::client_protocol::ClientV2Response::Transaction {
            transaction_id: [0x44; 16],
            state: prdb_engine::client_protocol::ClientV2TransactionState::Committed,
        };
        write_server_response_v2(&mut stream, &commit, &committed.encode());
    });

    let interactive_output = Command::new(env!("CARGO_BIN_EXE_prdb-client"))
        .arg("interactive-sql")
        .arg("--config")
        .arg(&client_config_path)
        .arg("--instance")
        .arg("local")
        .arg("--sql-file")
        .arg(&interactive_sql_path)
        .arg("--schema")
        .arg("app")
        .arg("--scope")
        .arg(&scope)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    began_rx
        .recv_timeout(std::time::Duration::from_secs(10))
        .unwrap();

    let instance_set = || {
        Command::new(env!("CARGO_BIN_EXE_prdb-client"))
            .arg("instance")
            .arg("set")
            .arg("--config")
            .arg(&client_config_path)
            .arg("--instance")
            .arg("local")
            .arg("--address")
            .arg(interactive_address.ip().to_string())
            .arg("--port")
            .arg(interactive_address.port().to_string())
            .arg("--release")
            .arg("2")
            .arg("--target")
            .arg("linux-x86_64")
            .arg("--cpu-baseline")
            .arg("x86_64-v1")
            .arg("--runtime-profile")
            .arg("test-runtime")
            .arg("--abi")
            .arg("3")
            .arg("--default-credential")
            .arg("alice")
            .arg("--username")
            .arg("alice")
            .arg("--credentials")
            .arg(&credentials)
            .output()
            .unwrap()
    };
    let blocked_selection = instance_set();
    continue_tx.send(()).unwrap();
    let interactive_output = interactive_output.wait_with_output().unwrap();
    assert!(
        !blocked_selection.status.success(),
        "SDK selection changed during an active interactive transaction: {}",
        String::from_utf8_lossy(&blocked_selection.stderr)
    );
    assert!(
        String::from_utf8_lossy(&blocked_selection.stderr)
            .contains("active interactive transaction"),
        "unexpected selection-change error: {}",
        String::from_utf8_lossy(&blocked_selection.stderr)
    );
    assert!(
        interactive_output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&interactive_output.stdout),
        String::from_utf8_lossy(&interactive_output.stderr)
    );
    interactive_server.join().unwrap();
    assert!(instance_set().status.success());
    assert_eq!(
        ClientConfig::load(&client_config_path).unwrap().instances[0]
            .sdk
            .release,
        2
    );

    let second = Command::new(env!("CARGO_BIN_EXE_prdb-packager"))
        .arg("--source")
        .arg(&source)
        .arg("--sdk-library")
        .arg(&library)
        .arg("--shared-linker")
        .arg("cc")
        .arg("--include-dir")
        .arg(root.join("engine/include"))
        .arg("--scope")
        .arg(&scope)
        .arg("--output-dir")
        .arg(&package)
        .args([
            "--abi",
            "3",
            "--sdk-release",
            "1",
            "--cpu-baseline",
            "x86_64-v1",
            "--runtime-profile",
            "test-runtime",
            "--compiler-identity",
            "TinyCC integration test",
        ])
        .output()
        .unwrap();
    assert!(
        !second.status.success(),
        "existing package path was overwritten"
    );
    fs::remove_dir_all(temp).unwrap();
}

#[test]
fn packager_validates_abi_v4_exports_on_the_maintained_optimizer_fixture() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let temp = unique_temp_dir();
    fs::create_dir(&temp).unwrap();
    let sdk_build = temp.join("sdk");
    let scope = temp.join("scope.psc1");
    let package = temp.join("package");
    let mut scope_bytes = b"PSC1\0\x01\x02\x02".to_vec();
    scope_bytes.extend([1; 16]);
    fs::write(&scope, &scope_bytes).unwrap();

    let build = Command::new("make")
        .current_dir(&root)
        .args([
            "-f",
            "engine/sdk/Makefile",
            &format!("BUILD_DIR={}", sdk_build.display()),
            "SDK_RUNTIME_PROFILE=test-runtime",
            "sdk-shared-release",
        ])
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );

    let status = Command::new(env!("CARGO_BIN_EXE_prdb-packager"))
        .arg("--source")
        .arg(root.join("engine/tests/fixtures/sql_v3_write_read.c"))
        .arg("--sdk-library")
        .arg(sdk_build.join(
            "releases/linux-x86_64/x86_64-v1/test-runtime/sdk-1-abis1,2,3,4,5/libprdb_sql.so.1",
        ))
        .arg("--shared-linker")
        .arg("cc")
        .arg("--include-dir")
        .arg(root.join("engine/include"))
        .arg("--scope")
        .arg(&scope)
        .arg("--output-dir")
        .arg(&package)
        .args([
            "--abi",
            "4",
            "--sdk-release",
            "1",
            "--cpu-baseline",
            "x86_64-v1",
            "--runtime-profile",
            "test-runtime",
            "--compiler-identity",
            "TinyCC optimizer fixture test",
        ])
        .output()
        .unwrap();
    assert!(
        status.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&status.stdout),
        String::from_utf8_lossy(&status.stderr)
    );

    let manifest = decode_manifest_v1(&fs::read(package.join("manifest.pmf1")).unwrap())
        .expect("decode maintained fixture manifest");
    assert_eq!(manifest.native_abi_version, 4);
    assert_eq!(manifest.descriptor_symbol, "prdb_query_descriptor_v4");
    assert_eq!(manifest.entry_point_symbol, "prdb_query_main_v4");
    let artifact = fs::read(package.join("query.so")).unwrap();
    assert!(artifact.len() < 100_000);
    let dynamic = decode_elf_dynamic_info(&artifact).unwrap();
    assert_eq!(dynamic.runpath.as_deref(), Some("$ORIGIN"));
    assert!(dynamic.rpath.is_none());
    assert!(dynamic.needed.iter().any(|name| name == "libprdb_sql.so.1"));
    let dynamic_symbols = Command::new("readelf")
        .args(["--dyn-syms", "-W"])
        .arg(package.join("query.so"))
        .output()
        .unwrap();
    assert!(dynamic_symbols.status.success());
    let dynamic_symbols = String::from_utf8_lossy(&dynamic_symbols.stdout);
    assert!(dynamic_symbols.lines().any(|line| {
        line.contains(" UND ") && line.contains("prdb_sdk_optimizer_execute_update_ast_v1")
    }));
    assert_eq!(
        fs::read(package.join("requested_scope.psc1")).unwrap(),
        scope_bytes
    );
    fs::remove_dir_all(temp).unwrap();
}

fn read_client_frame(stream: &mut TcpStream, decoder: &mut ClientFrameDecoder) -> ClientFrame {
    let mut bytes = [0; 4096];
    loop {
        let count = stream.read(&mut bytes).unwrap();
        assert_ne!(count, 0, "client closed before expected frame");
        let mut frames = decoder.push(&bytes[..count]).unwrap();
        if !frames.is_empty() {
            assert_eq!(frames.len(), 1, "client pipelined requests");
            return frames.remove(0);
        }
    }
}

fn write_server_response(stream: &mut TcpStream, request: &ClientFrame, payload: &[u8]) {
    let response = ClientFrame {
        kind: 100,
        request_id: request.request_id,
        response_sequence: 0,
        payload: payload.to_vec(),
    };
    stream.write_all(&response.encode().unwrap()).unwrap();
}

fn write_server_response_v2(stream: &mut TcpStream, request: &ClientFrame, payload: &[u8]) {
    let response = ClientFrame {
        kind: 100,
        request_id: request.request_id,
        response_sequence: 0,
        payload: payload.to_vec(),
    };
    stream.write_all(&response.encode_v2().unwrap()).unwrap();
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
