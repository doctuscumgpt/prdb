//! Bounded framing primitives for client network protocol version 1.0.

use crate::credentials::TOKEN_SECRET_BYTES;
use crate::storage_manager::{AuthenticatedQuerySession, StorageManager};
use crate::supervisor::StagingOwner;
use crate::transaction::TransactionError;
use std::collections::{HashMap, HashSet};
use zeroize::{Zeroize, Zeroizing};

pub const CLIENT_FRAME_HEADER_BYTES: usize =
    crate::query_abi::CLIENT_FRAME_HEADER_RECORD_HEADER_BYTES as usize;
pub const CLIENT_MAX_PAYLOAD_BYTES: usize = 1024 * 1024;
pub const CLIENT_MAX_CONTROL_BYTES: usize = 64 * 1024;
pub const CLIENT_MAX_CHUNK_BYTES: usize = 256 * 1024;
pub const CLIENT_ERROR_KIND: u16 = 101;
pub const CLIENT_V2_TX_BEGIN_KIND: u16 = 13;
pub const CLIENT_V2_TX_ATTACH_KIND: u16 = 14;
pub const CLIENT_V2_TX_COMMIT_KIND: u16 = 15;
pub const CLIENT_V2_TX_ROLLBACK_KIND: u16 = 16;
pub const CLIENT_V2_TX_STATUS_KIND: u16 = 17;
pub const CLIENT_V2_PACKAGE_STATUS_KIND: u16 = 18;
pub const CLIENT_SDK_LIBRARY_CHUNK_KIND: u16 = 19;
const CLIENT_MAGIC: [u8; 4] = *b"PDBN";
const CLIENT_METADATA_CURSOR_KIND: u8 = 0xd3;
pub const CLIENT_METADATA_MAX_PAGE: usize = 128;

/// Transaction-control requests carried by protocol v2.0 frames.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClientV2Request {
    TxBegin {
        begin_request_id: [u8; 16],
    },
    TxAttach {
        transaction_id: [u8; 16],
    },
    TxCommit {
        transaction_id: [u8; 16],
    },
    TxRollback {
        transaction_id: [u8; 16],
    },
    TxStatus {
        transaction_id: [u8; 16],
    },
    PackageStatus {
        transaction_id: [u8; 16],
        package_id: [u8; 16],
    },
}

impl ClientV2Request {
    /// Decode the strict JSON payload for one v2 transaction-control frame.
    pub fn decode(kind: u16, payload: &[u8]) -> Result<Self, ClientControlError> {
        if payload.len() > CLIENT_MAX_CONTROL_BYTES {
            return Err(ClientControlError::InvalidJson);
        }
        let mut fields = parse_string_object(payload)?;
        let request = match kind {
            CLIENT_V2_TX_BEGIN_KIND => Self::TxBegin {
                begin_request_id: decode_lower_hex_id(&take_required(
                    &mut fields,
                    "begin_request_id",
                )?)?,
            },
            CLIENT_V2_TX_ATTACH_KIND => Self::TxAttach {
                transaction_id: decode_lower_hex_id(&take_required(
                    &mut fields,
                    "transaction_id",
                )?)?,
            },
            CLIENT_V2_TX_COMMIT_KIND => Self::TxCommit {
                transaction_id: decode_lower_hex_id(&take_required(
                    &mut fields,
                    "transaction_id",
                )?)?,
            },
            CLIENT_V2_TX_ROLLBACK_KIND => Self::TxRollback {
                transaction_id: decode_lower_hex_id(&take_required(
                    &mut fields,
                    "transaction_id",
                )?)?,
            },
            CLIENT_V2_TX_STATUS_KIND => Self::TxStatus {
                transaction_id: decode_lower_hex_id(&take_required(
                    &mut fields,
                    "transaction_id",
                )?)?,
            },
            CLIENT_V2_PACKAGE_STATUS_KIND => Self::PackageStatus {
                transaction_id: decode_lower_hex_id(&take_required(
                    &mut fields,
                    "transaction_id",
                )?)?,
                package_id: decode_lower_hex_id(&take_required(&mut fields, "package_id")?)?,
            },
            _ => return Err(ClientControlError::UnsupportedV2Operation),
        };
        reject_unknown_fields(fields)?;
        Ok(request)
    }

    /// Encode this request using the canonical v2 JSON field order and hex IDs.
    pub fn encode(self) -> Result<(u16, Vec<u8>), ClientControlError> {
        let (kind, payload) = match self {
            Self::TxBegin { begin_request_id } => (
                CLIENT_V2_TX_BEGIN_KIND,
                format!(
                    "{{\"begin_request_id\":\"{}\"}}",
                    encode_lower_hex_id(begin_request_id)
                ),
            ),
            Self::TxAttach { transaction_id } => (
                CLIENT_V2_TX_ATTACH_KIND,
                format!(
                    "{{\"transaction_id\":\"{}\"}}",
                    encode_lower_hex_id(transaction_id)
                ),
            ),
            Self::TxCommit { transaction_id } => (
                CLIENT_V2_TX_COMMIT_KIND,
                format!(
                    "{{\"transaction_id\":\"{}\"}}",
                    encode_lower_hex_id(transaction_id)
                ),
            ),
            Self::TxRollback { transaction_id } => (
                CLIENT_V2_TX_ROLLBACK_KIND,
                format!(
                    "{{\"transaction_id\":\"{}\"}}",
                    encode_lower_hex_id(transaction_id)
                ),
            ),
            Self::TxStatus { transaction_id } => (
                CLIENT_V2_TX_STATUS_KIND,
                format!(
                    "{{\"transaction_id\":\"{}\"}}",
                    encode_lower_hex_id(transaction_id)
                ),
            ),
            Self::PackageStatus {
                transaction_id,
                package_id,
            } => (
                CLIENT_V2_PACKAGE_STATUS_KIND,
                format!(
                    "{{\"transaction_id\":\"{}\",\"package_id\":\"{}\"}}",
                    encode_lower_hex_id(transaction_id),
                    encode_lower_hex_id(package_id)
                ),
            ),
        };
        Ok((kind, payload.into_bytes()))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClientV2TransactionState {
    Active,
    AbortOnly,
    Committing,
    Committed,
    Aborted,
    Expired,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClientV2PackageState {
    Running,
    Completed,
    Failed,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClientV2Response {
    TransactionBegun {
        transaction_id: [u8; 16],
        state: ClientV2TransactionState,
        idle_timeout_ms: u64,
        lifetime_ms: u64,
        max_package_bytes: u64,
        max_packages: u64,
    },
    Transaction {
        transaction_id: [u8; 16],
        state: ClientV2TransactionState,
    },
    Package {
        transaction_id: [u8; 16],
        package_id: [u8; 16],
        state: ClientV2PackageState,
        result_lost: bool,
    },
}

impl ClientV2Response {
    /// Decode the strict response schema associated with a v2 request kind.
    pub fn decode(request_kind: u16, payload: &[u8]) -> Result<Self, ClientControlError> {
        let mut fields = parse_scalar_object(payload)?;
        let response = match request_kind {
            CLIENT_V2_TX_BEGIN_KIND => {
                let transaction_id =
                    decode_lower_hex_id(&take_string(&mut fields, "transaction_id")?)?;
                let state = decode_tx_state(&take_string(&mut fields, "state")?)?;
                let idle_timeout_ms = take_number(&mut fields, "idle_timeout_ms")?;
                let lifetime_ms = take_number(&mut fields, "lifetime_ms")?;
                let max_package_bytes = take_number(&mut fields, "max_package_bytes")?;
                let max_packages = take_number(&mut fields, "max_packages")?;
                if idle_timeout_ms == 0
                    || lifetime_ms == 0
                    || max_package_bytes == 0
                    || max_packages == 0
                {
                    return Err(ClientControlError::InvalidPrepare);
                }
                Self::TransactionBegun {
                    transaction_id,
                    state,
                    idle_timeout_ms,
                    lifetime_ms,
                    max_package_bytes,
                    max_packages,
                }
            }
            CLIENT_V2_TX_ATTACH_KIND
            | CLIENT_V2_TX_COMMIT_KIND
            | CLIENT_V2_TX_ROLLBACK_KIND
            | CLIENT_V2_TX_STATUS_KIND => {
                let transaction_id =
                    decode_lower_hex_id(&take_string(&mut fields, "transaction_id")?)?;
                let state = decode_tx_state(&take_string(&mut fields, "state")?)?;
                if request_kind == CLIENT_V2_TX_ATTACH_KIND
                    && !matches!(
                        state,
                        ClientV2TransactionState::Active | ClientV2TransactionState::AbortOnly
                    )
                    || request_kind == CLIENT_V2_TX_COMMIT_KIND
                        && !matches!(
                            state,
                            ClientV2TransactionState::Committed
                                | ClientV2TransactionState::Aborted
                                | ClientV2TransactionState::Expired
                        )
                    || request_kind == CLIENT_V2_TX_ROLLBACK_KIND
                        && !matches!(
                            state,
                            ClientV2TransactionState::Aborted | ClientV2TransactionState::Expired
                        )
                {
                    return Err(ClientControlError::InvalidPrepare);
                }
                Self::Transaction {
                    transaction_id,
                    state,
                }
            }
            CLIENT_V2_PACKAGE_STATUS_KIND => {
                let transaction_id =
                    decode_lower_hex_id(&take_string(&mut fields, "transaction_id")?)?;
                let package_id = decode_lower_hex_id(&take_string(&mut fields, "package_id")?)?;
                let state = decode_package_state(&take_string(&mut fields, "state")?)?;
                let result_lost = take_bool(&mut fields, "result_lost")?;
                if result_lost && state != ClientV2PackageState::Completed {
                    return Err(ClientControlError::InvalidPrepare);
                }
                Self::Package {
                    transaction_id,
                    package_id,
                    state,
                    result_lost,
                }
            }
            _ => return Err(ClientControlError::UnsupportedV2Operation),
        };
        reject_unknown_scalars(fields)?;
        Ok(response)
    }

    /// Encode this response as canonical JSON payload for a kind-100 frame.
    pub fn encode(&self) -> Vec<u8> {
        let payload = match self {
            Self::TransactionBegun {
                transaction_id,
                state,
                idle_timeout_ms,
                lifetime_ms,
                max_package_bytes,
                max_packages,
            } => format!(
                "{{\"transaction_id\":\"{}\",\"state\":\"{}\",\"idle_timeout_ms\":{idle_timeout_ms},\"lifetime_ms\":{lifetime_ms},\"max_package_bytes\":{max_package_bytes},\"max_packages\":{max_packages}}}",
                encode_lower_hex_id(*transaction_id),
                encode_tx_state(*state)
            ),
            Self::Transaction {
                transaction_id,
                state,
            } => format!(
                "{{\"transaction_id\":\"{}\",\"state\":\"{}\"}}",
                encode_lower_hex_id(*transaction_id),
                encode_tx_state(*state)
            ),
            Self::Package {
                transaction_id,
                package_id,
                state,
                result_lost,
            } => format!(
                "{{\"transaction_id\":\"{}\",\"package_id\":\"{}\",\"state\":\"{}\",\"result_lost\":{result_lost}}}",
                encode_lower_hex_id(*transaction_id),
                encode_lower_hex_id(*package_id),
                encode_package_state(*state)
            ),
        };
        payload.into_bytes()
    }
}

fn decode_tx_state(text: &str) -> Result<ClientV2TransactionState, ClientControlError> {
    match text {
        "active" => Ok(ClientV2TransactionState::Active),
        "abort_only" => Ok(ClientV2TransactionState::AbortOnly),
        "committing" => Ok(ClientV2TransactionState::Committing),
        "committed" => Ok(ClientV2TransactionState::Committed),
        "aborted" => Ok(ClientV2TransactionState::Aborted),
        "expired" => Ok(ClientV2TransactionState::Expired),
        _ => Err(ClientControlError::InvalidPrepare),
    }
}

fn encode_tx_state(state: ClientV2TransactionState) -> &'static str {
    match state {
        ClientV2TransactionState::Active => "active",
        ClientV2TransactionState::AbortOnly => "abort_only",
        ClientV2TransactionState::Committing => "committing",
        ClientV2TransactionState::Committed => "committed",
        ClientV2TransactionState::Aborted => "aborted",
        ClientV2TransactionState::Expired => "expired",
    }
}

fn client_v2_transaction_state(
    state: crate::storage_manager::InteractiveTransactionState,
) -> ClientV2TransactionState {
    match state {
        crate::storage_manager::InteractiveTransactionState::Active => {
            ClientV2TransactionState::Active
        }
        crate::storage_manager::InteractiveTransactionState::AbortOnly => {
            ClientV2TransactionState::AbortOnly
        }
        crate::storage_manager::InteractiveTransactionState::Committing => {
            ClientV2TransactionState::Committing
        }
        crate::storage_manager::InteractiveTransactionState::Committed => {
            ClientV2TransactionState::Committed
        }
        crate::storage_manager::InteractiveTransactionState::Aborted => {
            ClientV2TransactionState::Aborted
        }
        crate::storage_manager::InteractiveTransactionState::Expired => {
            ClientV2TransactionState::Expired
        }
    }
}

fn decode_package_state(text: &str) -> Result<ClientV2PackageState, ClientControlError> {
    match text {
        "running" => Ok(ClientV2PackageState::Running),
        "completed" => Ok(ClientV2PackageState::Completed),
        "failed" => Ok(ClientV2PackageState::Failed),
        "unknown" => Ok(ClientV2PackageState::Unknown),
        _ => Err(ClientControlError::InvalidPrepare),
    }
}

fn encode_package_state(state: ClientV2PackageState) -> &'static str {
    match state {
        ClientV2PackageState::Running => "running",
        ClientV2PackageState::Completed => "completed",
        ClientV2PackageState::Failed => "failed",
        ClientV2PackageState::Unknown => "unknown",
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClientFrame {
    pub kind: u16,
    pub request_id: u64,
    pub response_sequence: u64,
    pub payload: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Header {
    kind: u16,
    payload_bytes: usize,
    request_id: u64,
    response_sequence: u64,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ClientFrameError {
    InvalidMagic,
    UnsupportedVersion { major: u16, minor: u16 },
    InvalidKind(u16),
    NonzeroFlags,
    PayloadTooLarge,
    InvalidChunkLength,
    InvalidSequence,
    ZeroRequestId,
    AllocationFailed,
    IncompleteFrame,
    RequestIdsNotIncreasing,
    UnexpectedResponseSequence,
    PipelinedRequests,
}

impl std::fmt::Display for ClientFrameError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "client protocol frame error: {self:?}")
    }
}

impl std::error::Error for ClientFrameError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClientMode {
    Query,
    Control,
}

#[derive(Debug)]
pub enum ClientSessionError {
    InvalidHello,
    HelloAlreadyAccepted,
    AuthenticationOutOfOrder,
    AlreadyAuthenticated,
    AuthenticationRequired,
    WrongMode,
    UnsupportedRequest,
    ResourceLimit,
    InvalidControlPayload(ClientControlError),
    InteractiveTransaction(crate::storage_manager::InteractiveTransactionError),
    Staging(crate::supervisor::StagingError),
    Status(crate::storage_manager::InvocationStatusError),
    AdministrativeStatus(crate::storage_manager::AdministrativeInvocationListError),
    AdministrativeRuntimeList(crate::storage_manager::AdministrativeInvocationListError),
    AdministrativeAuditList(crate::storage_manager::AdministrativeAuditListError),
    AdministrativeCancellation(crate::storage_manager::AdministrativeCancellationError),
    SdkLibraryRepositoryUnauthorized,
    SdkLibraryRepositoryUnavailable,
    SdkLibraryRepository(crate::sdk_registry::SdkRegistryError),
    TokenInventory(crate::storage_manager::TokenLifecycleError),
    IdentityLifecycle(crate::storage_manager::IdentityLifecycleError),
    MembershipLifecycle(crate::storage_manager::MembershipLifecycleError),
    SystemPrivilegeLifecycle(crate::storage_manager::SystemPrivilegeLifecycleError),
    ObjectGrantLifecycle(crate::commit::CommitError),
    SchemaLifecycle(crate::storage_manager::SchemaLifecycleError),
    TableLifecycle(crate::storage_manager::TableLifecycleError),
    ForeignKeyLifecycle(crate::commit::CommitError),
    ForeignKeyCreateLifecycle(crate::storage_manager::TableLifecycleError),
    ConstraintLifecycle(crate::storage_manager::TableLifecycleError),
    PolicyLifecycle(crate::storage_manager::TableLifecycleError),
    IndexLifecycle(crate::storage_manager::IndexLifecycleError),
    Storage(TransactionError),
}

#[derive(Debug)]
pub enum ClientStartError {
    Session(ClientSessionError),
    Start(crate::supervisor::StartError<TransactionError>),
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    ExecutorStart(
        crate::supervisor::StartError<
            crate::executor_launcher::ExecutorStartError<TransactionError>,
        >,
    ),
}

impl std::fmt::Display for ClientStartError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "client START failed: {self:?}")
    }
}

impl std::error::Error for ClientStartError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClientControlError {
    InvalidJson,
    DuplicateKey,
    UnknownKey,
    MissingKey,
    InvalidMode,
    InvalidClientName,
    InvalidTokenId,
    InvalidTokenSecret,
    InvalidChunk,
    InvalidPrepare,
    InvalidInvocationId,
    UnsupportedAdminOperation,
    UnsupportedV2Operation,
    InvalidMetadataCursor,
    InvalidMetadataPageLimit,
}

impl std::fmt::Display for ClientControlError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "invalid client control payload: {self:?}")
    }
}

impl std::error::Error for ClientControlError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClientHello {
    pub mode: ClientMode,
    pub client_name: String,
    pub repository_sdk_library: bool,
}

#[derive(Eq, PartialEq)]
pub struct ClientCredentials {
    pub token_id: [u8; 16],
    pub token_secret: [u8; TOKEN_SECRET_BYTES],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClientChunk {
    pub invocation_id: [u8; 16],
    pub offset: u64,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClientPrepare {
    pub manifest_version: u32,
    pub manifest: Vec<u8>,
    pub artifact_bytes: u64,
    pub artifact_sha256: [u8; 32],
    pub repository_sdk_library: bool,
    pub parameter_bytes: u64,
    pub parameter_sha256: [u8; 32],
    pub scope_version: u32,
    pub requested_scope: Vec<u8>,
    pub deadline_ms: u64,
    pub workload_class: crate::supervisor::WorkloadClass,
    /// Present together only for protocol v2 packages executing inside an
    /// explicit transaction.
    pub transaction_id: Option<[u8; 16]>,
    pub package_id: Option<[u8; 16]>,
}

enum PendingAdminAction {
    CreateUser {
        name: String,
    },
    CreateUserNoLogin {
        name: String,
    },
    CreateRole {
        name: String,
    },
    CreateSchema {
        name: String,
        owner_id: Option<[u8; 16]>,
    },
    DropSchema {
        schema_id: [u8; 16],
    },
    SetSchemaOwner {
        schema_id: [u8; 16],
        owner_id: [u8; 16],
    },
    RenameSchema {
        schema_id: [u8; 16],
        name: String,
    },
    DropTable {
        table_id: [u8; 16],
    },
    TruncateTable {
        table_id: [u8; 16],
    },
    DropForeignKey {
        foreign_key_id: [u8; 16],
    },
    CreateForeignKey {
        source_table_id: [u8; 16],
        source_column_ids: Vec<[u8; 16]>,
        target_constraint_id: [u8; 16],
        name: Option<String>,
    },
    CreateUniqueConstraint {
        table_id: [u8; 16],
        column_ids: Vec<[u8; 16]>,
        name: Option<String>,
    },
    CreatePrimaryKey {
        table_id: [u8; 16],
        column_ids: Vec<[u8; 16]>,
        name: Option<String>,
    },
    CreateCheckConstraint {
        table_id: [u8; 16],
        name: Option<String>,
        expression: crate::authorization::Expr,
        dependency_column_ids: Vec<[u8; 16]>,
    },
    CreatePolicy {
        replaced_policy_id: Option<[u8; 16]>,
        table_id: [u8; 16],
        name: String,
        kind: crate::storage_manager::PolicyKind,
        operation: crate::authorization::Operation,
        effect: crate::authorization::Effect,
        recipients: Vec<crate::authorization::Subject>,
        target_column_ids: Vec<[u8; 16]>,
        using_expression: Option<crate::authorization::Expr>,
        with_check_expression: Option<crate::authorization::Expr>,
    },
    DropPolicy {
        policy_id: [u8; 16],
    },
    DropCheckConstraint {
        constraint_id: [u8; 16],
    },
    DropPrimaryKey {
        constraint_id: [u8; 16],
    },
    DropUniqueConstraint {
        constraint_id: [u8; 16],
    },
    CreateIndex {
        table_id: [u8; 16],
        name: String,
        column_ids: Vec<[u8; 16]>,
        unique: bool,
    },
    DropIndex {
        index_id: [u8; 16],
    },
    RebuildIndex {
        index_id: [u8; 16],
        column_ids: Vec<[u8; 16]>,
        unique: bool,
    },
    RenameIndex {
        index_id: [u8; 16],
        name: String,
    },
    RenameTable {
        table_id: [u8; 16],
        name: String,
    },
    SetTableOwner {
        table_id: [u8; 16],
        owner_id: [u8; 16],
    },
    RenameColumn {
        table_id: [u8; 16],
        column_id: [u8; 16],
        name: String,
    },
    SetColumnNullable {
        table_id: [u8; 16],
        column_id: [u8; 16],
        nullable: bool,
    },
    WidenVarcharColumn {
        table_id: [u8; 16],
        column_id: [u8; 16],
        max_chars: u32,
    },
    AlterColumnDefault {
        table_id: [u8; 16],
        column_id: [u8; 16],
        default: Option<crate::storage_manager::TableColumnDefault>,
    },
    DropColumn {
        table_id: [u8; 16],
        column_id: [u8; 16],
    },
    AddColumn {
        table_id: [u8; 16],
        column: crate::storage_manager::TableColumnDefinition,
    },
    CreateTable {
        schema_id: [u8; 16],
        name: String,
        columns: Vec<crate::storage_manager::TableColumnDefinition>,
        primary_key_columns: Option<Vec<String>>,
        primary_key_name: Option<String>,
        unique_constraints: Vec<Vec<String>>,
        unique_constraint_names: Vec<Option<String>>,
        check_expressions: Vec<crate::authorization::Expr>,
        check_constraint_names: Vec<Option<String>>,
        foreign_keys: Vec<crate::storage_manager::CreateTableForeignKeyDefinition>,
        foreign_key_names: Vec<Option<String>>,
    },
    RenameIdentity {
        identity_id: [u8; 16],
        name: String,
    },
    DropIdentity {
        identity_id: [u8; 16],
    },
    SetUserLogin {
        user_id: [u8; 16],
        enabled: bool,
    },
    ChangeRoleMembership {
        member_id: [u8; 16],
        role_id: [u8; 16],
        grant: bool,
    },
    ChangeSystemPrivilege {
        grantee_id: [u8; 16],
        privilege: crate::storage_manager::AdministrativeSystemPrivilege,
        grant: bool,
    },
    ChangeObjectGrant {
        grantee_id: [u8; 16],
        object_id: [u8; 16],
        privileges: Vec<crate::storage_manager::ObjectGrantPrivilege>,
        target: crate::storage_manager::ObjectGrantTarget,
        entries: Option<Vec<crate::storage_manager::ObjectGrantEntry>>,
        grant: bool,
    },
    Revoke {
        token_id: [u8; 16],
    },
    Issue {
        user_id: [u8; 16],
        label: String,
        expires_at_us: i64,
    },
    Rotate {
        old_token_id: [u8; 16],
        label: String,
        expires_at_us: i64,
    },
}

struct PendingAdminOperation {
    operation_id: [u8; 16],
    action: PendingAdminAction,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum MetadataPageQuery {
    Schemas,
    Relations([u8; 16]),
    Columns {
        schema_id: [u8; 16],
        relation_id: [u8; 16],
    },
    Indexes {
        schema_id: [u8; 16],
        relation_id: [u8; 16],
    },
    Constraints {
        schema_id: [u8; 16],
        relation_id: [u8; 16],
    },
    Grants {
        schema_id: [u8; 16],
        relation_id: [u8; 16],
    },
    Policies {
        schema_id: [u8; 16],
        relation_id: [u8; 16],
    },
    Dependencies([u8; 16]),
}

#[derive(Clone, Debug)]
enum MetadataPageRows {
    Schemas(Vec<crate::metadata_discovery::VisibleSchema>),
    Relations(Vec<crate::metadata_discovery::VisibleRelation>),
    Columns(Vec<crate::metadata_discovery::VisibleColumn>),
    Indexes(Vec<crate::metadata_discovery::AdministrativeIndex>),
    Constraints(Vec<crate::metadata_discovery::AdministrativeConstraint>),
    Grants(Vec<crate::metadata_discovery::AdministrativeGrant>),
    Policies(Vec<crate::metadata_discovery::AdministrativePolicy>),
    Dependencies(Vec<crate::metadata_discovery::AdministrativeDependency>),
}

impl MetadataPageRows {
    fn len(&self) -> usize {
        match self {
            Self::Schemas(rows) => rows.len(),
            Self::Relations(rows) => rows.len(),
            Self::Columns(rows) => rows.len(),
            Self::Indexes(rows) => rows.len(),
            Self::Constraints(rows) => rows.len(),
            Self::Grants(rows) => rows.len(),
            Self::Policies(rows) => rows.len(),
            Self::Dependencies(rows) => rows.len(),
        }
    }

    fn has_same_object_ids(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Schemas(left), Self::Schemas(right)) => left
                .iter()
                .map(|schema| schema.schema_id)
                .eq(right.iter().map(|schema| schema.schema_id)),
            (Self::Relations(left), Self::Relations(right)) => left
                .iter()
                .map(|relation| relation.relation_id)
                .eq(right.iter().map(|relation| relation.relation_id)),
            (Self::Columns(left), Self::Columns(right)) => left
                .iter()
                .map(|column| column.column_id)
                .eq(right.iter().map(|column| column.column_id)),
            (Self::Indexes(left), Self::Indexes(right)) => left
                .iter()
                .map(|index| index.index_id)
                .eq(right.iter().map(|index| index.index_id)),
            (Self::Constraints(left), Self::Constraints(right)) => left == right,
            (Self::Grants(left), Self::Grants(right)) => left == right,
            (Self::Policies(left), Self::Policies(right)) => left == right,
            (Self::Dependencies(left), Self::Dependencies(right)) => left == right,
            _ => false,
        }
    }
}

struct ActiveMetadataPageSnapshot {
    query: MetadataPageQuery,
    rows: MetadataPageRows,
    principal_id: [u8; 16],
    role_ids: std::collections::HashSet<[u8; 16]>,
    page_limit: usize,
    next_index: usize,
    current_cursor: Option<[u8; 16]>,
    expires_at: std::time::Instant,
}

struct ActiveSdkLibraryPageSnapshot {
    releases: Vec<crate::sdk_registry::SdkReleaseInfo>,
    principal_id: [u8; 16],
    role_ids: std::collections::HashSet<[u8; 16]>,
    page_limit: usize,
    next_index: usize,
    current_cursor: Option<[u8; 16]>,
    expires_at: std::time::Instant,
}

struct MetadataPagination {
    page_limit: usize,
    cursor: Option<[u8; 16]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClientAdminRead {
    ResolveIdentityName {
        name: String,
        kind: crate::metadata_discovery::AdministrativeIdentityKind,
        policy_scope: Option<([u8; 16], [u8; 16])>,
    },
    ResolveGrantIdentityName {
        schema_name: String,
        table_name: Option<String>,
        name: String,
        kind: crate::metadata_discovery::AdministrativeIdentityKind,
    },
    ResolveAlterIdentityName {
        object_id: [u8; 16],
        name: String,
        kind: crate::metadata_discovery::AdministrativeIdentityKind,
    },
    ResolvePolicyName {
        schema_id: [u8; 16],
        table_id: [u8; 16],
        name: String,
        is_cell: Option<bool>,
    },
    ListSdkLibraries,
    ListSdkLibrariesPage {
        page_limit: usize,
        cursor: Option<[u8; 16]>,
    },
    ListSchemas,
    ListSchemasPage {
        page_limit: usize,
        cursor: Option<[u8; 16]>,
    },
    ListRelations {
        schema_id: [u8; 16],
    },
    ListRelationsPage {
        schema_id: [u8; 16],
        page_limit: usize,
        cursor: Option<[u8; 16]>,
    },
    ListColumns {
        schema_id: [u8; 16],
        relation_id: [u8; 16],
    },
    ListColumnsPage {
        schema_id: [u8; 16],
        relation_id: [u8; 16],
        page_limit: usize,
        cursor: Option<[u8; 16]>,
    },
    ListIndexesPage {
        schema_id: [u8; 16],
        relation_id: [u8; 16],
        page_limit: usize,
        cursor: Option<[u8; 16]>,
    },
    ListConstraintsPage {
        schema_id: [u8; 16],
        relation_id: [u8; 16],
        page_limit: usize,
        cursor: Option<[u8; 16]>,
    },
    ListGrantsPage {
        schema_id: [u8; 16],
        relation_id: [u8; 16],
        page_limit: usize,
        cursor: Option<[u8; 16]>,
    },
    ListPoliciesPage {
        schema_id: [u8; 16],
        relation_id: [u8; 16],
        page_limit: usize,
        cursor: Option<[u8; 16]>,
    },
    ListDependenciesPage {
        object_id: [u8; 16],
        page_limit: usize,
        cursor: Option<[u8; 16]>,
    },
    ListTokens {
        user_id: [u8; 16],
        after_token_id: Option<[u8; 16]>,
    },
    ListAuditEvents {
        page_limit: usize,
        cursor: Option<crate::storage_manager::AdministrativeAuditCursor>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClientAdminCommand {
    ResolveIdentityName {
        name: String,
        kind: crate::metadata_discovery::AdministrativeIdentityKind,
        policy_scope: Option<([u8; 16], [u8; 16])>,
    },
    ResolveGrantIdentityName {
        schema_name: String,
        table_name: Option<String>,
        name: String,
        kind: crate::metadata_discovery::AdministrativeIdentityKind,
    },
    ResolveAlterIdentityName {
        object_id: [u8; 16],
        name: String,
        kind: crate::metadata_discovery::AdministrativeIdentityKind,
    },
    ResolvePolicyName {
        schema_id: [u8; 16],
        table_id: [u8; 16],
        name: String,
        is_cell: Option<bool>,
    },
    ListSdkLibraries,
    ListSdkLibrariesPage {
        page_limit: usize,
        cursor: Option<[u8; 16]>,
    },
    ListSchemas,
    ListSchemasPage {
        page_limit: usize,
        cursor: Option<[u8; 16]>,
    },
    ListRelations {
        schema_id: [u8; 16],
    },
    ListRelationsPage {
        schema_id: [u8; 16],
        page_limit: usize,
        cursor: Option<[u8; 16]>,
    },
    ListColumns {
        schema_id: [u8; 16],
        relation_id: [u8; 16],
    },
    ListColumnsPage {
        schema_id: [u8; 16],
        relation_id: [u8; 16],
        page_limit: usize,
        cursor: Option<[u8; 16]>,
    },
    ListIndexesPage {
        schema_id: [u8; 16],
        relation_id: [u8; 16],
        page_limit: usize,
        cursor: Option<[u8; 16]>,
    },
    ListConstraintsPage {
        schema_id: [u8; 16],
        relation_id: [u8; 16],
        page_limit: usize,
        cursor: Option<[u8; 16]>,
    },
    ListGrantsPage {
        schema_id: [u8; 16],
        relation_id: [u8; 16],
        page_limit: usize,
        cursor: Option<[u8; 16]>,
    },
    ListPoliciesPage {
        schema_id: [u8; 16],
        relation_id: [u8; 16],
        page_limit: usize,
        cursor: Option<[u8; 16]>,
    },
    ListDependenciesPage {
        object_id: [u8; 16],
        page_limit: usize,
        cursor: Option<[u8; 16]>,
    },
    ListTokens {
        user_id: [u8; 16],
        after_token_id: Option<[u8; 16]>,
    },
    ListAuditEvents {
        page_limit: usize,
        cursor: Option<crate::storage_manager::AdministrativeAuditCursor>,
    },
    ListRuntimeInvocations {
        response_version: u64,
        page_limit: usize,
        cursor: Option<crate::storage_manager::AdministrativeInvocationCursor>,
    },
    ListOwnInvocations {
        response_version: u64,
        page_limit: usize,
        cursor: Option<crate::storage_manager::InvocationListCursor>,
    },
    RemoveSdkLibrary {
        operation_id: [u8; 16],
        sdk_release: u32,
    },
    BeginSdkLibraryInstall {
        operation_id: [u8; 16],
        metadata_bytes: u64,
        library_bytes: u64,
        library_sha256: [u8; 32],
    },
    FinishSdkLibraryInstall {
        operation_id: [u8; 16],
    },
    SdkLibraryOperationStatus {
        operation_id: [u8; 16],
    },
    PrepareCreateUser {
        name: String,
    },
    PrepareCreateUserNoLogin {
        name: String,
    },
    PrepareCreateRole {
        name: String,
    },
    PrepareCreateSchema {
        name: String,
        owner_id: Option<[u8; 16]>,
    },
    PrepareDropSchema {
        schema_id: [u8; 16],
    },
    PrepareSetSchemaOwner {
        schema_id: [u8; 16],
        owner_id: [u8; 16],
    },
    PrepareRenameSchema {
        schema_id: [u8; 16],
        name: String,
    },
    PrepareDropTable {
        table_id: [u8; 16],
    },
    PrepareTruncateTable {
        table_id: [u8; 16],
    },
    PrepareDropForeignKey {
        foreign_key_id: [u8; 16],
    },
    PrepareCreateForeignKey {
        source_table_id: [u8; 16],
        source_column_ids: Vec<[u8; 16]>,
        target_constraint_id: [u8; 16],
        name: Option<String>,
    },
    PrepareCreateUniqueConstraint {
        table_id: [u8; 16],
        column_ids: Vec<[u8; 16]>,
        name: Option<String>,
    },
    PrepareCreatePrimaryKey {
        table_id: [u8; 16],
        column_ids: Vec<[u8; 16]>,
        name: Option<String>,
    },
    PrepareCreateCheckConstraint {
        table_id: [u8; 16],
        name: Option<String>,
        expression: crate::authorization::Expr,
        dependency_column_ids: Vec<[u8; 16]>,
    },
    PrepareCreatePolicy {
        replaced_policy_id: Option<[u8; 16]>,
        table_id: [u8; 16],
        name: String,
        kind: crate::storage_manager::PolicyKind,
        operation: crate::authorization::Operation,
        effect: crate::authorization::Effect,
        recipients: Vec<crate::authorization::Subject>,
        target_column_ids: Vec<[u8; 16]>,
        using_expression: Option<crate::authorization::Expr>,
        with_check_expression: Option<crate::authorization::Expr>,
    },
    PrepareDropPolicy {
        policy_id: [u8; 16],
    },
    PrepareDropCheckConstraint {
        constraint_id: [u8; 16],
    },
    PrepareDropPrimaryKey {
        constraint_id: [u8; 16],
    },
    PrepareDropUniqueConstraint {
        constraint_id: [u8; 16],
    },
    PrepareCreateIndex {
        table_id: [u8; 16],
        name: String,
        column_ids: Vec<[u8; 16]>,
        unique: bool,
    },
    PrepareDropIndex {
        index_id: [u8; 16],
    },
    PrepareRebuildIndex {
        index_id: [u8; 16],
        column_ids: Vec<[u8; 16]>,
        unique: bool,
    },
    PrepareRenameIndex {
        index_id: [u8; 16],
        name: String,
    },
    PrepareRenameTable {
        table_id: [u8; 16],
        name: String,
    },
    PrepareSetTableOwner {
        table_id: [u8; 16],
        owner_id: [u8; 16],
    },
    PrepareRenameColumn {
        table_id: [u8; 16],
        column_id: [u8; 16],
        name: String,
    },
    PrepareSetColumnNullable {
        table_id: [u8; 16],
        column_id: [u8; 16],
        nullable: bool,
    },
    PrepareWidenVarcharColumn {
        table_id: [u8; 16],
        column_id: [u8; 16],
        max_chars: u32,
    },
    PrepareAlterColumnDefault {
        table_id: [u8; 16],
        column_id: [u8; 16],
        default: Option<crate::storage_manager::TableColumnDefault>,
    },
    PrepareDropColumn {
        table_id: [u8; 16],
        column_id: [u8; 16],
    },
    PrepareAddColumn {
        table_id: [u8; 16],
        column: crate::storage_manager::TableColumnDefinition,
    },
    PrepareCreateTable {
        schema_id: [u8; 16],
        name: String,
        columns: Vec<crate::storage_manager::TableColumnDefinition>,
        primary_key_columns: Option<Vec<String>>,
        primary_key_name: Option<String>,
        unique_constraints: Vec<Vec<String>>,
        unique_constraint_names: Vec<Option<String>>,
        check_expressions: Vec<crate::authorization::Expr>,
        check_constraint_names: Vec<Option<String>>,
        foreign_keys: Vec<crate::storage_manager::CreateTableForeignKeyDefinition>,
        foreign_key_names: Vec<Option<String>>,
    },
    PrepareRenameIdentity {
        identity_id: [u8; 16],
        name: String,
    },
    PrepareDropIdentity {
        identity_id: [u8; 16],
    },
    PrepareSetUserLogin {
        user_id: [u8; 16],
        enabled: bool,
    },
    PrepareGrantRole {
        member_id: [u8; 16],
        role_id: [u8; 16],
    },
    PrepareRevokeRole {
        member_id: [u8; 16],
        role_id: [u8; 16],
    },
    PrepareSystemPrivilege {
        grantee_id: [u8; 16],
        privilege: crate::storage_manager::AdministrativeSystemPrivilege,
        grant: bool,
    },
    PrepareObjectGrant {
        grantee_id: [u8; 16],
        object_id: [u8; 16],
        privileges: Vec<crate::storage_manager::ObjectGrantPrivilege>,
        target: crate::storage_manager::ObjectGrantTarget,
        entries: Option<Vec<crate::storage_manager::ObjectGrantEntry>>,
        grant: bool,
    },
    PrepareRevoke {
        token_id: [u8; 16],
    },
    PrepareIssue {
        user_id: [u8; 16],
        label: String,
        expires_at_us: i64,
    },
    PrepareRotateToken {
        old_token_id: [u8; 16],
        label: String,
        expires_at_us: i64,
    },
    Start {
        operation_id: [u8; 16],
    },
}

/// Decode the versioned, read-only ADMIN envelope and its first supported operation.
pub fn decode_client_admin_read(payload: &[u8]) -> Result<ClientAdminRead, ClientControlError> {
    match decode_client_admin_command(payload)? {
        ClientAdminCommand::ResolveIdentityName {
            name,
            kind,
            policy_scope,
        } => Ok(ClientAdminRead::ResolveIdentityName {
            name,
            kind,
            policy_scope,
        }),
        ClientAdminCommand::ResolveGrantIdentityName {
            schema_name,
            table_name,
            name,
            kind,
        } => Ok(ClientAdminRead::ResolveGrantIdentityName {
            schema_name,
            table_name,
            name,
            kind,
        }),
        ClientAdminCommand::ResolveAlterIdentityName {
            object_id,
            name,
            kind,
        } => Ok(ClientAdminRead::ResolveAlterIdentityName {
            object_id,
            name,
            kind,
        }),
        ClientAdminCommand::ResolvePolicyName {
            schema_id,
            table_id,
            name,
            is_cell,
        } => Ok(ClientAdminRead::ResolvePolicyName {
            schema_id,
            table_id,
            name,
            is_cell,
        }),
        ClientAdminCommand::ListSdkLibraries => Ok(ClientAdminRead::ListSdkLibraries),
        ClientAdminCommand::ListSdkLibrariesPage { page_limit, cursor } => {
            Ok(ClientAdminRead::ListSdkLibrariesPage { page_limit, cursor })
        }
        ClientAdminCommand::ListSchemas => Ok(ClientAdminRead::ListSchemas),
        ClientAdminCommand::ListSchemasPage { page_limit, cursor } => {
            Ok(ClientAdminRead::ListSchemasPage { page_limit, cursor })
        }
        ClientAdminCommand::ListRelations { schema_id } => {
            Ok(ClientAdminRead::ListRelations { schema_id })
        }
        ClientAdminCommand::ListRelationsPage {
            schema_id,
            page_limit,
            cursor,
        } => Ok(ClientAdminRead::ListRelationsPage {
            schema_id,
            page_limit,
            cursor,
        }),
        ClientAdminCommand::ListColumns {
            schema_id,
            relation_id,
        } => Ok(ClientAdminRead::ListColumns {
            schema_id,
            relation_id,
        }),
        ClientAdminCommand::ListColumnsPage {
            schema_id,
            relation_id,
            page_limit,
            cursor,
        } => Ok(ClientAdminRead::ListColumnsPage {
            schema_id,
            relation_id,
            page_limit,
            cursor,
        }),
        ClientAdminCommand::ListIndexesPage {
            schema_id,
            relation_id,
            page_limit,
            cursor,
        } => Ok(ClientAdminRead::ListIndexesPage {
            schema_id,
            relation_id,
            page_limit,
            cursor,
        }),
        ClientAdminCommand::ListConstraintsPage {
            schema_id,
            relation_id,
            page_limit,
            cursor,
        } => Ok(ClientAdminRead::ListConstraintsPage {
            schema_id,
            relation_id,
            page_limit,
            cursor,
        }),
        ClientAdminCommand::ListGrantsPage {
            schema_id,
            relation_id,
            page_limit,
            cursor,
        } => Ok(ClientAdminRead::ListGrantsPage {
            schema_id,
            relation_id,
            page_limit,
            cursor,
        }),
        ClientAdminCommand::ListPoliciesPage {
            schema_id,
            relation_id,
            page_limit,
            cursor,
        } => Ok(ClientAdminRead::ListPoliciesPage {
            schema_id,
            relation_id,
            page_limit,
            cursor,
        }),
        ClientAdminCommand::ListDependenciesPage {
            object_id,
            page_limit,
            cursor,
        } => Ok(ClientAdminRead::ListDependenciesPage {
            object_id,
            page_limit,
            cursor,
        }),
        ClientAdminCommand::ListTokens {
            user_id,
            after_token_id,
        } => Ok(ClientAdminRead::ListTokens {
            user_id,
            after_token_id,
        }),
        ClientAdminCommand::ListAuditEvents { page_limit, cursor } => {
            Ok(ClientAdminRead::ListAuditEvents { page_limit, cursor })
        }
        _ => Err(ClientControlError::UnsupportedAdminOperation),
    }
}

/// Decode the currently supported ADMIN read and credential lifecycle commands.
pub fn decode_client_admin_command(
    payload: &[u8],
) -> Result<ClientAdminCommand, ClientControlError> {
    if payload.len() > CLIENT_MAX_CONTROL_BYTES {
        return Err(ClientControlError::InvalidJson);
    }
    let text = std::str::from_utf8(payload).map_err(|_| ClientControlError::InvalidJson)?;
    let mut parser = JsonStringObjectParser {
        text: text.as_bytes(),
        offset: 0,
    };
    parser.skip_whitespace();
    parser.expect(b'{')?;
    let mut phase = None;
    let mut version = None;
    let mut operation = None;
    let mut arguments = None;
    let mut table_columns = None;
    let mut primary_key_columns = None;
    let mut unique_constraints = None;
    let mut unique_constraint_names = None;
    let mut check_expressions = None;
    let mut check_constraint_names = None;
    let mut table_foreign_key_sources = None;
    let mut table_foreign_key_targets = None;
    let mut table_foreign_key_target_columns = None;
    let mut foreign_key_names = None;
    let mut foreign_key_source_columns = None;
    let mut constraint_column_ids = None;
    let mut index_unique = None;
    let mut check_dependency_column_ids = None;
    let mut policy_recipients = None;
    let mut policy_target_column_ids = None;
    let mut table_column = None;
    let mut column_default = None;
    let mut operation_id = None;
    let mut seen = std::collections::HashSet::new();
    loop {
        parser.skip_whitespace();
        if parser.consume(b'}') {
            break;
        }
        let key = parser.parse_string()?;
        if !seen.insert(key.clone()) {
            return Err(ClientControlError::DuplicateKey);
        }
        parser.skip_whitespace();
        parser.expect(b':')?;
        parser.skip_whitespace();
        match key.as_str() {
            "phase" => phase = Some(parser.parse_string()?),
            "operation_version" => version = Some(parser.parse_u64_number()?),
            "operation" => operation = Some(parser.parse_string()?),
            "operation_id" => operation_id = Some(parser.parse_string()?),
            "arguments" => {
                parser.expect(b'{')?;
                let mut args = HashMap::new();
                loop {
                    parser.skip_whitespace();
                    if parser.consume(b'}') {
                        break;
                    }
                    let arg = parser.parse_string()?;
                    if args.contains_key(&arg) {
                        return Err(ClientControlError::DuplicateKey);
                    }
                    parser.skip_whitespace();
                    parser.expect(b':')?;
                    parser.skip_whitespace();
                    if arg == "columns" && parser.text.get(parser.offset) == Some(&b'[') {
                        table_columns = Some(parser.parse_table_column_array()?);
                        args.insert(arg, "<structured-columns>".to_owned());
                    } else if arg == "primary_key_columns"
                        && parser.text.get(parser.offset) == Some(&b'[')
                    {
                        primary_key_columns = Some(parser.parse_string_array()?);
                        args.insert(arg, "<structured-names>".to_owned());
                    } else if arg == "primary_key_name" {
                        args.insert(arg, parser.parse_string()?);
                    } else if arg == "unique_constraints"
                        && parser.text.get(parser.offset) == Some(&b'[')
                    {
                        unique_constraints = Some(parser.parse_string_array_array()?);
                        args.insert(arg, "<structured-name-arrays>".to_owned());
                    } else if arg == "unique_constraint_names"
                        && parser.text.get(parser.offset) == Some(&b'[')
                    {
                        unique_constraint_names = Some(parser.parse_string_array()?);
                        args.insert(arg, "<structured-unique-names>".to_owned());
                    } else if arg == "check_expressions_eir1"
                        && parser.text.get(parser.offset) == Some(&b'[')
                    {
                        let encoded = parser.parse_string_array()?;
                        let expressions = encoded
                            .into_iter()
                            .map(|value| {
                                let bytes = decode_lower_hex_bytes(
                                    &value,
                                    crate::expression::MAX_EXPRESSION_BYTES,
                                )?;
                                let expression = crate::expression::decode_expression(&bytes)
                                    .map_err(|_| ClientControlError::InvalidPrepare)?;
                                if !crate::expression::is_row_local_expression(&expression) {
                                    return Err(ClientControlError::InvalidPrepare);
                                }
                                Ok(expression)
                            })
                            .collect::<Result<Vec<_>, ClientControlError>>()?;
                        check_expressions = Some(expressions);
                        args.insert(arg, "<structured-check-expressions>".to_owned());
                    } else if arg == "check_constraint_names"
                        && parser.text.get(parser.offset) == Some(&b'[')
                    {
                        check_constraint_names = Some(parser.parse_string_array()?);
                        args.insert(arg, "<structured-check-names>".to_owned());
                    } else if arg == "foreign_key_source_columns"
                        && parser.text.get(parser.offset) == Some(&b'[')
                    {
                        table_foreign_key_sources = Some(parser.parse_string_array_array()?);
                        args.insert(arg, "<structured-foreign-key-sources>".to_owned());
                    } else if arg == "foreign_key_target_constraint_ids"
                        && parser.text.get(parser.offset) == Some(&b'[')
                    {
                        table_foreign_key_targets = Some(parser.parse_id_array_allow_zero()?);
                        args.insert(arg, "<structured-foreign-key-targets>".to_owned());
                    } else if arg == "foreign_key_target_columns"
                        && parser.text.get(parser.offset) == Some(&b'[')
                    {
                        table_foreign_key_target_columns = Some(parser.parse_string_array_array()?);
                        args.insert(arg, "<structured-foreign-key-target-columns>".to_owned());
                    } else if arg == "foreign_key_names"
                        && parser.text.get(parser.offset) == Some(&b'[')
                    {
                        foreign_key_names = Some(parser.parse_string_array()?);
                        args.insert(arg, "<structured-foreign-key-names>".to_owned());
                    } else if arg == "source_column_ids"
                        && parser.text.get(parser.offset) == Some(&b'[')
                    {
                        foreign_key_source_columns = Some(parser.parse_id_array()?);
                        args.insert(arg, "<structured-ids>".to_owned());
                    } else if arg == "column_ids" && parser.text.get(parser.offset) == Some(&b'[') {
                        constraint_column_ids = Some(parser.parse_id_array()?);
                        args.insert(arg, "<structured-ids>".to_owned());
                    } else if arg == "dependency_column_ids"
                        && parser.text.get(parser.offset) == Some(&b'[')
                    {
                        check_dependency_column_ids = Some(parser.parse_id_array()?);
                        args.insert(arg, "<structured-ids>".to_owned());
                    } else if arg == "target_column_ids"
                        && parser.text.get(parser.offset) == Some(&b'[')
                    {
                        policy_target_column_ids = Some(parser.parse_id_array()?);
                        args.insert(arg, "<structured-ids>".to_owned());
                    } else if arg == "target_ids" && parser.text.get(parser.offset) == Some(&b'[') {
                        let target_ids = parser.parse_string_array()?;
                        if target_ids.len() > 64 {
                            return Err(ClientControlError::InvalidPrepare);
                        }
                        args.insert(arg, target_ids.join(","));
                    } else if arg == "privileges" && parser.text.get(parser.offset) == Some(&b'[') {
                        let privileges = parser.parse_string_array()?;
                        if privileges.is_empty() || privileges.len() > 11 {
                            return Err(ClientControlError::InvalidPrepare);
                        }
                        args.insert(arg, privileges.join(","));
                    } else if arg == "recipients" && parser.text.get(parser.offset) == Some(&b'[') {
                        policy_recipients = Some(parser.parse_policy_recipient_array()?);
                        args.insert(arg, "<structured-recipients>".to_owned());
                    } else if arg == "unique" {
                        index_unique = Some(parser.parse_boolean()?);
                        args.insert(arg, "<boolean>".to_owned());
                    } else if arg == "column" && parser.text.get(parser.offset) == Some(&b'{') {
                        table_column = Some(parser.parse_table_column_definition()?);
                        args.insert(arg, "<structured-column>".to_owned());
                    } else if arg == "default" && parser.text.get(parser.offset) == Some(&b'{') {
                        column_default = Some(parser.parse_table_column_default()?);
                        args.insert(arg, "<structured-default>".to_owned());
                    } else if matches!(
                        arg.as_str(),
                        "using_expression_eir1" | "with_check_expression_eir1"
                    ) && parser.text.get(parser.offset..parser.offset + 4)
                        == Some(b"null")
                    {
                        parser.offset += 4;
                        args.insert(arg, "<null>".to_owned());
                    } else {
                        args.insert(arg, parser.parse_string()?);
                    }
                    parser.skip_whitespace();
                    if !parser.consume(b',') {
                        parser.expect(b'}')?;
                        break;
                    }
                    parser.skip_whitespace();
                    if parser.text.get(parser.offset) == Some(&b'}') {
                        return Err(ClientControlError::InvalidJson);
                    }
                }
                arguments = Some(args);
            }
            _ => return Err(ClientControlError::UnknownKey),
        }
        parser.skip_whitespace();
        if !parser.consume(b',') {
            parser.expect(b'}')?;
            break;
        }
        parser.skip_whitespace();
        if parser.text.get(parser.offset) == Some(&b'}') {
            return Err(ClientControlError::InvalidJson);
        }
    }
    parser.skip_whitespace();
    if parser.offset != parser.text.len() {
        return Err(ClientControlError::InvalidJson);
    }
    match phase.as_deref() {
        Some("read")
            if version == Some(1)
                && operation.as_deref() == Some("metadata.identities.resolve") =>
        {
            if operation_id.is_some() || seen.len() != 4 {
                return Err(ClientControlError::UnknownKey);
            }
            let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
            let name = args.remove("name").ok_or(ClientControlError::MissingKey)?;
            let kind = match args.remove("kind").as_deref() {
                Some("user") => crate::metadata_discovery::AdministrativeIdentityKind::User,
                Some("role") => crate::metadata_discovery::AdministrativeIdentityKind::Role,
                Some("either") => crate::metadata_discovery::AdministrativeIdentityKind::Either,
                _ => return Err(ClientControlError::InvalidPrepare),
            };
            if !args.is_empty() {
                return Err(ClientControlError::UnknownKey);
            }
            Ok(ClientAdminCommand::ResolveIdentityName {
                name,
                kind,
                policy_scope: None,
            })
        }
        Some("read")
            if version == Some(1)
                && operation.as_deref() == Some("metadata.policy_identities.resolve") =>
        {
            if operation_id.is_some() || seen.len() != 4 {
                return Err(ClientControlError::UnknownKey);
            }
            let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
            if args.len() != 4 {
                return Err(ClientControlError::UnknownKey);
            }
            let schema_id = decode_lower_hex_id(
                &args
                    .remove("schema_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            let table_id = decode_lower_hex_id(
                &args
                    .remove("table_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            let name = args.remove("name").ok_or(ClientControlError::MissingKey)?;
            let kind = match args.remove("kind").as_deref() {
                Some("user") => crate::metadata_discovery::AdministrativeIdentityKind::User,
                Some("role") => crate::metadata_discovery::AdministrativeIdentityKind::Role,
                _ => return Err(ClientControlError::InvalidPrepare),
            };
            if !args.is_empty() || schema_id == [0; 16] || table_id == [0; 16] {
                return Err(ClientControlError::InvalidPrepare);
            }
            Ok(ClientAdminCommand::ResolveIdentityName {
                name,
                kind,
                policy_scope: Some((schema_id, table_id)),
            })
        }
        Some("read")
            if version == Some(1)
                && operation.as_deref() == Some("metadata.alter_identities.resolve") =>
        {
            if operation_id.is_some() || seen.len() != 4 {
                return Err(ClientControlError::UnknownKey);
            }
            let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
            if args.len() != 3 {
                return Err(ClientControlError::UnknownKey);
            }
            let object_id = decode_lower_hex_id(
                &args
                    .remove("object_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            let name = args.remove("name").ok_or(ClientControlError::MissingKey)?;
            let kind = match args.remove("kind").as_deref() {
                Some("user") => crate::metadata_discovery::AdministrativeIdentityKind::User,
                Some("role") => crate::metadata_discovery::AdministrativeIdentityKind::Role,
                Some("either") => crate::metadata_discovery::AdministrativeIdentityKind::Either,
                _ => return Err(ClientControlError::InvalidPrepare),
            };
            if !args.is_empty() || object_id == [0; 16] {
                return Err(ClientControlError::InvalidPrepare);
            }
            Ok(ClientAdminCommand::ResolveAlterIdentityName {
                object_id,
                name,
                kind,
            })
        }
        Some("read")
            if version == Some(1)
                && operation.as_deref() == Some("metadata.grant_identities.resolve") =>
        {
            if operation_id.is_some() || seen.len() != 4 {
                return Err(ClientControlError::UnknownKey);
            }
            let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
            if !matches!(args.len(), 3 | 4) {
                return Err(ClientControlError::UnknownKey);
            }
            let schema_name = args
                .remove("schema_name")
                .ok_or(ClientControlError::MissingKey)?;
            let table_name = args.remove("table_name");
            if args.len() != 2 {
                return Err(ClientControlError::UnknownKey);
            }
            let name = args.remove("name").ok_or(ClientControlError::MissingKey)?;
            let kind = match args.remove("kind").as_deref() {
                Some("user") => crate::metadata_discovery::AdministrativeIdentityKind::User,
                Some("role") => crate::metadata_discovery::AdministrativeIdentityKind::Role,
                _ => return Err(ClientControlError::InvalidPrepare),
            };
            if !args.is_empty() {
                return Err(ClientControlError::InvalidPrepare);
            }
            Ok(ClientAdminCommand::ResolveGrantIdentityName {
                schema_name,
                table_name,
                name,
                kind,
            })
        }
        Some("read")
            if version == Some(1) && operation.as_deref() == Some("metadata.policies.resolve") =>
        {
            if operation_id.is_some() || seen.len() != 4 {
                return Err(ClientControlError::UnknownKey);
            }
            let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
            if args.len() != 4 {
                return Err(ClientControlError::UnknownKey);
            }
            let schema_id = decode_lower_hex_id(
                &args
                    .remove("schema_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            let table_id = decode_lower_hex_id(
                &args
                    .remove("table_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            let name = args.remove("name").ok_or(ClientControlError::MissingKey)?;
            let is_cell = match args.remove("kind").as_deref() {
                Some("row") => Some(false),
                Some("cell") => Some(true),
                Some("any") => None,
                _ => return Err(ClientControlError::InvalidPrepare),
            };
            if !args.is_empty() || schema_id == [0; 16] || table_id == [0; 16] {
                return Err(ClientControlError::InvalidPrepare);
            }
            Ok(ClientAdminCommand::ResolvePolicyName {
                schema_id,
                table_id,
                name,
                is_cell,
            })
        }
        Some("read")
            if version == Some(1) && operation.as_deref() == Some("sdk_libraries.list") =>
        {
            if operation_id.is_some() || seen.len() != 4 {
                return Err(ClientControlError::UnknownKey);
            }
            let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
            let pagination =
                decode_metadata_pagination(&mut args)?.ok_or(ClientControlError::MissingKey)?;
            if !args.is_empty() {
                return Err(ClientControlError::UnknownKey);
            }
            Ok(ClientAdminCommand::ListSdkLibrariesPage {
                page_limit: pagination.page_limit,
                cursor: pagination.cursor,
            })
        }
        Some("read")
            if version == Some(1) && operation.as_deref() == Some("metadata.schemas.list") =>
        {
            if operation_id.is_some() || seen.len() != 4 {
                return Err(ClientControlError::UnknownKey);
            }
            let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
            let pagination = decode_metadata_pagination(&mut args)?;
            if !args.is_empty() {
                return Err(ClientControlError::UnknownKey);
            }
            Ok(match pagination {
                Some(pagination) => ClientAdminCommand::ListSchemasPage {
                    page_limit: pagination.page_limit,
                    cursor: pagination.cursor,
                },
                None => ClientAdminCommand::ListSchemas,
            })
        }
        Some("read")
            if version == Some(1) && operation.as_deref() == Some("metadata.relations.list") =>
        {
            if operation_id.is_some() || seen.len() != 4 {
                return Err(ClientControlError::UnknownKey);
            }
            let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
            let schema_id = decode_lower_hex_id(
                &args
                    .remove("schema_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            let pagination = decode_metadata_pagination(&mut args)?;
            if !args.is_empty() {
                return Err(ClientControlError::UnknownKey);
            }
            Ok(match pagination {
                Some(pagination) => ClientAdminCommand::ListRelationsPage {
                    schema_id,
                    page_limit: pagination.page_limit,
                    cursor: pagination.cursor,
                },
                None => ClientAdminCommand::ListRelations { schema_id },
            })
        }
        Some("read")
            if version == Some(1) && operation.as_deref() == Some("metadata.columns.list") =>
        {
            if operation_id.is_some() || seen.len() != 4 {
                return Err(ClientControlError::UnknownKey);
            }
            let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
            let schema_id = decode_lower_hex_id(
                &args
                    .remove("schema_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            let relation_id = decode_lower_hex_id(
                &args
                    .remove("relation_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            let pagination = decode_metadata_pagination(&mut args)?;
            if !args.is_empty() {
                return Err(ClientControlError::UnknownKey);
            }
            Ok(match pagination {
                Some(pagination) => ClientAdminCommand::ListColumnsPage {
                    schema_id,
                    relation_id,
                    page_limit: pagination.page_limit,
                    cursor: pagination.cursor,
                },
                None => ClientAdminCommand::ListColumns {
                    schema_id,
                    relation_id,
                },
            })
        }
        Some("read")
            if version == Some(1) && operation.as_deref() == Some("metadata.indexes.list") =>
        {
            if operation_id.is_some() || seen.len() != 4 {
                return Err(ClientControlError::UnknownKey);
            }
            let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
            let schema_id = decode_lower_hex_id(
                &args
                    .remove("schema_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            let relation_id = decode_lower_hex_id(
                &args
                    .remove("table_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            let pagination =
                decode_metadata_pagination(&mut args)?.ok_or(ClientControlError::MissingKey)?;
            if !args.is_empty() {
                return Err(ClientControlError::UnknownKey);
            }
            Ok(ClientAdminCommand::ListIndexesPage {
                schema_id,
                relation_id,
                page_limit: pagination.page_limit,
                cursor: pagination.cursor,
            })
        }
        Some("read")
            if version == Some(1) && operation.as_deref() == Some("metadata.constraints.list") =>
        {
            if operation_id.is_some() || seen.len() != 4 {
                return Err(ClientControlError::UnknownKey);
            }
            let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
            let schema_id = decode_lower_hex_id(
                &args
                    .remove("schema_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            let relation_id = decode_lower_hex_id(
                &args
                    .remove("table_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            let pagination =
                decode_metadata_pagination(&mut args)?.ok_or(ClientControlError::MissingKey)?;
            if !args.is_empty() {
                return Err(ClientControlError::UnknownKey);
            }
            Ok(ClientAdminCommand::ListConstraintsPage {
                schema_id,
                relation_id,
                page_limit: pagination.page_limit,
                cursor: pagination.cursor,
            })
        }
        Some("read")
            if version == Some(1) && operation.as_deref() == Some("metadata.grants.list") =>
        {
            if operation_id.is_some() || seen.len() != 4 {
                return Err(ClientControlError::UnknownKey);
            }
            let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
            let schema_id = decode_lower_hex_id(
                &args
                    .remove("schema_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            let relation_id = decode_lower_hex_id(
                &args
                    .remove("table_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            let pagination =
                decode_metadata_pagination(&mut args)?.ok_or(ClientControlError::MissingKey)?;
            if !args.is_empty() {
                return Err(ClientControlError::UnknownKey);
            }
            Ok(ClientAdminCommand::ListGrantsPage {
                schema_id,
                relation_id,
                page_limit: pagination.page_limit,
                cursor: pagination.cursor,
            })
        }
        Some("read")
            if version == Some(1) && operation.as_deref() == Some("metadata.policies.list") =>
        {
            if operation_id.is_some() || seen.len() != 4 {
                return Err(ClientControlError::UnknownKey);
            }
            let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
            let schema_id = decode_lower_hex_id(
                &args
                    .remove("schema_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            let relation_id = decode_lower_hex_id(
                &args
                    .remove("table_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            let pagination =
                decode_metadata_pagination(&mut args)?.ok_or(ClientControlError::MissingKey)?;
            if !args.is_empty() {
                return Err(ClientControlError::UnknownKey);
            }
            Ok(ClientAdminCommand::ListPoliciesPage {
                schema_id,
                relation_id,
                page_limit: pagination.page_limit,
                cursor: pagination.cursor,
            })
        }
        Some("read")
            if version == Some(1) && operation.as_deref() == Some("metadata.dependencies.list") =>
        {
            if operation_id.is_some() || seen.len() != 4 {
                return Err(ClientControlError::UnknownKey);
            }
            let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
            let object_id = decode_lower_hex_id(
                &args
                    .remove("object_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            let pagination =
                decode_metadata_pagination(&mut args)?.ok_or(ClientControlError::MissingKey)?;
            if !args.is_empty() {
                return Err(ClientControlError::UnknownKey);
            }
            Ok(ClientAdminCommand::ListDependenciesPage {
                object_id,
                page_limit: pagination.page_limit,
                cursor: pagination.cursor,
            })
        }
        Some("read") if version == Some(1) && operation.as_deref() == Some("tokens.list") => {
            if operation_id.is_some() || seen.len() != 4 {
                return Err(ClientControlError::UnknownKey);
            }
            let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
            let user_id = decode_lower_hex_id(
                &args
                    .remove("user_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            let after_token_id = args
                .remove("after_token_id")
                .map(|value| decode_lower_hex_id(&value))
                .transpose()?;
            if !args.is_empty() {
                return Err(ClientControlError::UnknownKey);
            }
            Ok(ClientAdminCommand::ListTokens {
                user_id,
                after_token_id,
            })
        }
        Some("read") if version == Some(1) && operation.as_deref() == Some("audit.events.list") => {
            if operation_id.is_some() || seen.len() != 4 {
                return Err(ClientControlError::UnknownKey);
            }
            let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
            let page_limit = args.remove("limit").ok_or(ClientControlError::MissingKey)?;
            let page_limit = parse_canonical_positive_i64(&page_limit)
                .and_then(|value| usize::try_from(value).ok())
                .filter(|value| *value <= crate::storage_manager::MAX_ADMIN_AUDIT_PAGE)
                .ok_or(ClientControlError::InvalidPrepare)?;
            let cursor = args
                .remove("cursor")
                .map(|value| {
                    crate::storage_manager::AdministrativeAuditCursor::from_wire(&value)
                        .ok_or(ClientControlError::InvalidPrepare)
                })
                .transpose()?;
            if !args.is_empty() {
                return Err(ClientControlError::UnknownKey);
            }
            Ok(ClientAdminCommand::ListAuditEvents { page_limit, cursor })
        }
        Some("read")
            if matches!(version, Some(1..=8))
                && operation.as_deref() == Some("runtime.invocations.list") =>
        {
            if operation_id.is_some() || seen.len() != 4 {
                return Err(ClientControlError::UnknownKey);
            }
            let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
            let page_limit = args.remove("limit").ok_or(ClientControlError::MissingKey)?;
            let page_limit = parse_canonical_positive_i64(&page_limit)
                .and_then(|value| usize::try_from(value).ok())
                .filter(|value| *value <= crate::storage_manager::MAX_ADMIN_INVOCATION_PAGE)
                .ok_or(ClientControlError::InvalidPrepare)?;
            let cursor = args
                .remove("cursor")
                .map(|value| {
                    crate::storage_manager::AdministrativeInvocationCursor::from_wire(&value)
                        .ok_or(ClientControlError::InvalidPrepare)
                })
                .transpose()?;
            if !args.is_empty() {
                return Err(ClientControlError::UnknownKey);
            }
            Ok(ClientAdminCommand::ListRuntimeInvocations {
                response_version: version.ok_or(ClientControlError::MissingKey)?,
                page_limit,
                cursor,
            })
        }
        Some("read")
            if matches!(version, Some(1 | 2))
                && operation.as_deref() == Some("runtime.invocations.own.list") =>
        {
            if operation_id.is_some() || seen.len() != 4 {
                return Err(ClientControlError::UnknownKey);
            }
            let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
            let page_limit = args.remove("limit").ok_or(ClientControlError::MissingKey)?;
            let page_limit = parse_canonical_positive_i64(&page_limit)
                .and_then(|value| usize::try_from(value).ok())
                .filter(|value| *value <= crate::storage_manager::MAX_OWN_INVOCATION_PAGE)
                .ok_or(ClientControlError::InvalidPrepare)?;
            let cursor = args
                .remove("cursor")
                .map(|value| {
                    crate::storage_manager::InvocationListCursor::from_wire(&value)
                        .ok_or(ClientControlError::InvalidPrepare)
                })
                .transpose()?;
            if !args.is_empty() {
                return Err(ClientControlError::UnknownKey);
            }
            Ok(ClientAdminCommand::ListOwnInvocations {
                response_version: version.ok_or(ClientControlError::MissingKey)?,
                page_limit,
                cursor,
            })
        }
        Some("prepare")
            if version == Some(1) && operation.as_deref() == Some("sdk_libraries.install") =>
        {
            if operation_id.is_some() || seen.len() != 4 {
                return Err(ClientControlError::UnknownKey);
            }
            let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
            let operation_id = decode_lower_hex_id(
                &args
                    .remove("operation_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            if operation_id == [0; 16] {
                return Err(ClientControlError::InvalidPrepare);
            }
            let metadata_bytes = args
                .remove("metadata_bytes")
                .and_then(|value| parse_canonical_positive_i64(&value))
                .and_then(|value| u64::try_from(value).ok())
                .filter(|bytes| *bytes <= 64 * 1024)
                .ok_or(ClientControlError::InvalidPrepare)?;
            let library_bytes = args
                .remove("library_bytes")
                .and_then(|value| parse_canonical_positive_i64(&value))
                .and_then(|value| u64::try_from(value).ok())
                .filter(|bytes| *bytes <= 128 * 1024 * 1024)
                .ok_or(ClientControlError::InvalidPrepare)?;
            let library_sha256 = decode_lower_hex_hash(
                &args
                    .remove("library_sha256")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            if !args.is_empty() {
                return Err(ClientControlError::UnknownKey);
            }
            Ok(ClientAdminCommand::BeginSdkLibraryInstall {
                operation_id,
                metadata_bytes,
                library_bytes,
                library_sha256,
            })
        }
        Some("write")
            if version == Some(1)
                && operation.as_deref() == Some("sdk_libraries.install.finish") =>
        {
            if operation_id.is_some() || seen.len() != 4 {
                return Err(ClientControlError::UnknownKey);
            }
            let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
            let operation_id = decode_lower_hex_id(
                &args
                    .remove("operation_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            if operation_id == [0; 16] || !args.is_empty() {
                return Err(ClientControlError::InvalidPrepare);
            }
            Ok(ClientAdminCommand::FinishSdkLibraryInstall { operation_id })
        }
        Some("write")
            if version == Some(1) && operation.as_deref() == Some("sdk_libraries.remove") =>
        {
            if operation_id.is_some() || seen.len() != 4 {
                return Err(ClientControlError::UnknownKey);
            }
            let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
            let operation_id = decode_lower_hex_id(
                &args
                    .remove("operation_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            let release = args
                .remove("release")
                .ok_or(ClientControlError::MissingKey)?;
            let sdk_release = parse_canonical_positive_i64(&release)
                .and_then(|value| u32::try_from(value).ok())
                .ok_or(ClientControlError::InvalidPrepare)?;
            if !args.is_empty() {
                return Err(ClientControlError::UnknownKey);
            }
            if operation_id == [0; 16] {
                return Err(ClientControlError::InvalidPrepare);
            }
            Ok(ClientAdminCommand::RemoveSdkLibrary {
                operation_id,
                sdk_release,
            })
        }
        Some("read")
            if version == Some(1)
                && operation.as_deref() == Some("sdk_libraries.operation_status") =>
        {
            if operation_id.is_some() || seen.len() != 4 {
                return Err(ClientControlError::UnknownKey);
            }
            let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
            let operation_id = decode_lower_hex_id(
                &args
                    .remove("operation_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            if operation_id == [0; 16] || !args.is_empty() {
                return Err(ClientControlError::InvalidPrepare);
            }
            Ok(ClientAdminCommand::SdkLibraryOperationStatus { operation_id })
        }
        Some("prepare") if version == Some(1) && operation.as_deref() == Some("users.create") => {
            if operation_id.is_some() || seen.len() != 4 {
                return Err(ClientControlError::UnknownKey);
            }
            let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
            let name = args.remove("name").ok_or(ClientControlError::MissingKey)?;
            if !is_canonical_admin_principal_name(&name) || !args.is_empty() {
                return Err(ClientControlError::InvalidPrepare);
            }
            Ok(ClientAdminCommand::PrepareCreateUser { name })
        }
        Some("prepare")
            if version == Some(1) && operation.as_deref() == Some("users.create_no_login") =>
        {
            if operation_id.is_some() || seen.len() != 4 {
                return Err(ClientControlError::UnknownKey);
            }
            let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
            let name = args.remove("name").ok_or(ClientControlError::MissingKey)?;
            if !is_canonical_admin_principal_name(&name) || !args.is_empty() {
                return Err(ClientControlError::InvalidPrepare);
            }
            Ok(ClientAdminCommand::PrepareCreateUserNoLogin { name })
        }
        Some("prepare") if version == Some(1) && operation.as_deref() == Some("roles.create") => {
            if operation_id.is_some() || seen.len() != 4 {
                return Err(ClientControlError::UnknownKey);
            }
            let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
            let name = args.remove("name").ok_or(ClientControlError::MissingKey)?;
            if !is_canonical_admin_principal_name(&name) || !args.is_empty() {
                return Err(ClientControlError::InvalidPrepare);
            }
            Ok(ClientAdminCommand::PrepareCreateRole { name })
        }
        Some("prepare") if version == Some(1) && operation.as_deref() == Some("schemas.create") => {
            if operation_id.is_some() || seen.len() != 4 {
                return Err(ClientControlError::UnknownKey);
            }
            let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
            let name = args.remove("name").ok_or(ClientControlError::MissingKey)?;
            let owner_id = args
                .remove("owner_id")
                .map(|value| decode_lower_hex_id(&value))
                .transpose()?;
            if !is_canonical_admin_principal_name(&name) || !args.is_empty() {
                return Err(ClientControlError::InvalidPrepare);
            }
            Ok(ClientAdminCommand::PrepareCreateSchema { name, owner_id })
        }
        Some("prepare") if version == Some(1) && operation.as_deref() == Some("schemas.drop") => {
            if operation_id.is_some() || seen.len() != 4 {
                return Err(ClientControlError::UnknownKey);
            }
            let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
            let schema_id = decode_lower_hex_id(
                &args
                    .remove("schema_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            if !args.is_empty() {
                return Err(ClientControlError::UnknownKey);
            }
            Ok(ClientAdminCommand::PrepareDropSchema { schema_id })
        }
        Some("prepare")
            if version == Some(1) && operation.as_deref() == Some("schemas.set_owner") =>
        {
            if operation_id.is_some() || seen.len() != 4 {
                return Err(ClientControlError::UnknownKey);
            }
            let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
            let schema_id = decode_lower_hex_id(
                &args
                    .remove("schema_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            let owner_id = decode_lower_hex_id(
                &args
                    .remove("owner_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            if !args.is_empty() {
                return Err(ClientControlError::UnknownKey);
            }
            Ok(ClientAdminCommand::PrepareSetSchemaOwner {
                schema_id,
                owner_id,
            })
        }
        Some("prepare") if version == Some(1) && operation.as_deref() == Some("schemas.rename") => {
            if operation_id.is_some() || seen.len() != 4 {
                return Err(ClientControlError::UnknownKey);
            }
            let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
            let schema_id = decode_lower_hex_id(
                &args
                    .remove("schema_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            let name = args.remove("name").ok_or(ClientControlError::MissingKey)?;
            if !args.is_empty() || !is_canonical_admin_principal_name(&name) {
                return Err(ClientControlError::InvalidPrepare);
            }
            Ok(ClientAdminCommand::PrepareRenameSchema { schema_id, name })
        }
        Some("prepare") if version == Some(1) && operation.as_deref() == Some("tables.drop") => {
            if operation_id.is_some() || seen.len() != 4 {
                return Err(ClientControlError::UnknownKey);
            }
            let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
            let table_id = decode_lower_hex_id(
                &args
                    .remove("table_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            if !args.is_empty() {
                return Err(ClientControlError::UnknownKey);
            }
            Ok(ClientAdminCommand::PrepareDropTable { table_id })
        }
        Some("prepare")
            if version == Some(1) && operation.as_deref() == Some("tables.truncate") =>
        {
            if operation_id.is_some() || seen.len() != 4 {
                return Err(ClientControlError::UnknownKey);
            }
            let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
            let table_id = decode_lower_hex_id(
                &args
                    .remove("table_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            if !args.is_empty() {
                return Err(ClientControlError::UnknownKey);
            }
            Ok(ClientAdminCommand::PrepareTruncateTable { table_id })
        }
        Some("prepare")
            if version == Some(1) && operation.as_deref() == Some("foreign_keys.drop") =>
        {
            if operation_id.is_some() || seen.len() != 4 {
                return Err(ClientControlError::UnknownKey);
            }
            let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
            let foreign_key_id = decode_lower_hex_id(
                &args
                    .remove("foreign_key_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            if !args.is_empty() {
                return Err(ClientControlError::UnknownKey);
            }
            Ok(ClientAdminCommand::PrepareDropForeignKey { foreign_key_id })
        }
        Some("prepare")
            if matches!(version, Some(1 | 2))
                && operation.as_deref() == Some("foreign_keys.create") =>
        {
            if operation_id.is_some() || seen.len() != 4 {
                return Err(ClientControlError::UnknownKey);
            }
            let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
            let source_table_id = decode_lower_hex_id(
                &args
                    .remove("source_table_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            let target_constraint_id = decode_lower_hex_id(
                &args
                    .remove("target_constraint_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            let name = if version == Some(2) {
                let name = args.remove("name").ok_or(ClientControlError::MissingKey)?;
                if !is_canonical_admin_principal_name(&name) {
                    return Err(ClientControlError::InvalidPrepare);
                }
                Some(name)
            } else {
                None
            };
            args.remove("source_column_ids")
                .ok_or(ClientControlError::MissingKey)?;
            let source_column_ids = foreign_key_source_columns
                .take()
                .ok_or(ClientControlError::MissingKey)?;
            if source_column_ids.is_empty() || !args.is_empty() {
                return Err(ClientControlError::InvalidPrepare);
            }
            Ok(ClientAdminCommand::PrepareCreateForeignKey {
                source_table_id,
                source_column_ids,
                target_constraint_id,
                name,
            })
        }
        Some("prepare")
            if matches!(version, Some(1 | 2))
                && operation.as_deref() == Some("constraints.create_unique") =>
        {
            if operation_id.is_some() || seen.len() != 4 {
                return Err(ClientControlError::UnknownKey);
            }
            let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
            let table_id = decode_lower_hex_id(
                &args
                    .remove("table_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            let name = if version == Some(2) {
                let name = args.remove("name").ok_or(ClientControlError::MissingKey)?;
                if !is_canonical_admin_principal_name(&name) {
                    return Err(ClientControlError::InvalidPrepare);
                }
                Some(name)
            } else {
                None
            };
            args.remove("column_ids")
                .ok_or(ClientControlError::MissingKey)?;
            let column_ids = constraint_column_ids
                .take()
                .ok_or(ClientControlError::MissingKey)?;
            if column_ids.is_empty() || !args.is_empty() {
                return Err(ClientControlError::InvalidPrepare);
            }
            Ok(ClientAdminCommand::PrepareCreateUniqueConstraint {
                table_id,
                column_ids,
                name,
            })
        }
        Some("prepare") if version == Some(1) && operation.as_deref() == Some("indexes.create") => {
            if operation_id.is_some() || seen.len() != 4 {
                return Err(ClientControlError::UnknownKey);
            }
            let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
            let table_id = decode_lower_hex_id(
                &args
                    .remove("table_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            let name = args.remove("name").ok_or(ClientControlError::MissingKey)?;
            if args.remove("column_ids").as_deref() != Some("<structured-ids>") {
                return Err(ClientControlError::MissingKey);
            }
            let column_ids = constraint_column_ids
                .take()
                .ok_or(ClientControlError::MissingKey)?;
            if args.remove("unique").as_deref() != Some("<boolean>") || !args.is_empty() {
                return Err(ClientControlError::UnknownKey);
            }
            let unique = index_unique.ok_or(ClientControlError::MissingKey)?;
            if !is_canonical_admin_principal_name(&name)
                || column_ids.is_empty()
                || column_ids
                    .iter()
                    .copied()
                    .collect::<std::collections::HashSet<_>>()
                    .len()
                    != column_ids.len()
            {
                return Err(ClientControlError::InvalidPrepare);
            }
            Ok(ClientAdminCommand::PrepareCreateIndex {
                table_id,
                name,
                column_ids,
                unique,
            })
        }
        Some("prepare") if version == Some(1) && operation.as_deref() == Some("indexes.drop") => {
            if operation_id.is_some() || seen.len() != 4 {
                return Err(ClientControlError::UnknownKey);
            }
            let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
            let index_id = decode_lower_hex_id(
                &args
                    .remove("index_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            if !args.is_empty() {
                return Err(ClientControlError::UnknownKey);
            }
            Ok(ClientAdminCommand::PrepareDropIndex { index_id })
        }
        Some("prepare") if version == Some(1) && operation.as_deref() == Some("indexes.rename") => {
            if operation_id.is_some() || seen.len() != 4 {
                return Err(ClientControlError::UnknownKey);
            }
            let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
            let index_id = decode_lower_hex_id(
                &args
                    .remove("index_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            let name = args.remove("name").ok_or(ClientControlError::MissingKey)?;
            if !args.is_empty() || !is_canonical_admin_principal_name(&name) {
                return Err(ClientControlError::InvalidPrepare);
            }
            Ok(ClientAdminCommand::PrepareRenameIndex { index_id, name })
        }
        Some("prepare")
            if version == Some(1) && operation.as_deref() == Some("indexes.rebuild") =>
        {
            if operation_id.is_some() || seen.len() != 4 {
                return Err(ClientControlError::UnknownKey);
            }
            let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
            let index_id = decode_lower_hex_id(
                &args
                    .remove("index_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            if args.remove("column_ids").as_deref() != Some("<structured-ids>") {
                return Err(ClientControlError::MissingKey);
            }
            let column_ids = constraint_column_ids
                .take()
                .ok_or(ClientControlError::MissingKey)?;
            if args.remove("unique").as_deref() != Some("<boolean>") || !args.is_empty() {
                return Err(ClientControlError::UnknownKey);
            }
            let unique = index_unique.ok_or(ClientControlError::MissingKey)?;
            if column_ids.is_empty()
                || column_ids
                    .iter()
                    .copied()
                    .collect::<std::collections::HashSet<_>>()
                    .len()
                    != column_ids.len()
            {
                return Err(ClientControlError::InvalidPrepare);
            }
            Ok(ClientAdminCommand::PrepareRebuildIndex {
                index_id,
                column_ids,
                unique,
            })
        }
        Some("prepare")
            if matches!(version, Some(1 | 2))
                && operation.as_deref() == Some("constraints.create_primary_key") =>
        {
            if operation_id.is_some() || seen.len() != 4 {
                return Err(ClientControlError::UnknownKey);
            }
            let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
            let table_id = decode_lower_hex_id(
                &args
                    .remove("table_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            let name = if version == Some(2) {
                let name = args.remove("name").ok_or(ClientControlError::MissingKey)?;
                if !is_canonical_admin_principal_name(&name) {
                    return Err(ClientControlError::InvalidPrepare);
                }
                Some(name)
            } else {
                None
            };
            args.remove("column_ids")
                .ok_or(ClientControlError::MissingKey)?;
            let column_ids = constraint_column_ids
                .take()
                .ok_or(ClientControlError::MissingKey)?;
            if column_ids.is_empty() || !args.is_empty() {
                return Err(ClientControlError::InvalidPrepare);
            }
            Ok(ClientAdminCommand::PrepareCreatePrimaryKey {
                table_id,
                column_ids,
                name,
            })
        }
        Some("prepare")
            if matches!(version, Some(1 | 2))
                && operation.as_deref() == Some("constraints.create_check") =>
        {
            let has_name = version == Some(2);
            if operation_id.is_some() || seen.len() != 4 {
                return Err(ClientControlError::UnknownKey);
            }
            let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
            let table_id = decode_lower_hex_id(
                &args
                    .remove("table_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            let name = if has_name {
                Some(args.remove("name").ok_or(ClientControlError::MissingKey)?)
            } else {
                None
            };
            let expression_bytes = decode_lower_hex_bytes(
                &args
                    .remove("expression_eir1")
                    .ok_or(ClientControlError::MissingKey)?,
                crate::expression::MAX_EXPRESSION_BYTES,
            )?;
            let expression = crate::expression::decode_expression(&expression_bytes)
                .map_err(|_| ClientControlError::InvalidPrepare)?;
            args.remove("dependency_column_ids")
                .ok_or(ClientControlError::MissingKey)?;
            let dependency_column_ids = check_dependency_column_ids
                .take()
                .ok_or(ClientControlError::MissingKey)?;
            if !args.is_empty() || !crate::expression::is_row_local_expression(&expression) {
                return Err(ClientControlError::InvalidPrepare);
            }
            Ok(ClientAdminCommand::PrepareCreateCheckConstraint {
                table_id,
                name,
                expression,
                dependency_column_ids,
            })
        }
        Some("prepare")
            if version == Some(1)
                && matches!(
                    operation.as_deref(),
                    Some("policies.create" | "policies.replace")
                ) =>
        {
            if operation_id.is_some() || seen.len() != 4 {
                return Err(ClientControlError::UnknownKey);
            }
            let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
            let table_id = decode_lower_hex_id(
                &args
                    .remove("table_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            let name = args.remove("name").ok_or(ClientControlError::MissingKey)?;
            let kind = match args
                .remove("kind")
                .ok_or(ClientControlError::MissingKey)?
                .as_str()
            {
                "row" => crate::storage_manager::PolicyKind::Row,
                "cell" => crate::storage_manager::PolicyKind::Cell,
                _ => return Err(ClientControlError::InvalidPrepare),
            };
            let policy_operation = match args
                .remove("policy_operation")
                .ok_or(ClientControlError::MissingKey)?
                .as_str()
            {
                "select" => crate::authorization::Operation::Select,
                "insert" => crate::authorization::Operation::Insert,
                "update" => crate::authorization::Operation::Update,
                "delete" => crate::authorization::Operation::Delete,
                _ => return Err(ClientControlError::InvalidPrepare),
            };
            let effect = match args
                .remove("effect")
                .ok_or(ClientControlError::MissingKey)?
                .as_str()
            {
                "allow" => crate::authorization::Effect::Allow,
                "restrict" => crate::authorization::Effect::Restrict,
                _ => return Err(ClientControlError::InvalidPrepare),
            };
            if args.remove("recipients").as_deref() != Some("<structured-recipients>") {
                return Err(ClientControlError::MissingKey);
            }
            let recipients = policy_recipients
                .take()
                .ok_or(ClientControlError::MissingKey)?;
            if args.remove("target_column_ids").as_deref() != Some("<structured-ids>") {
                return Err(ClientControlError::MissingKey);
            }
            let target_column_ids = policy_target_column_ids
                .take()
                .ok_or(ClientControlError::MissingKey)?;
            let using_expression = decode_optional_policy_expression(
                args.remove("using_expression_eir1")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            let with_check_expression = decode_optional_policy_expression(
                args.remove("with_check_expression_eir1")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            let replaced_policy_id = if operation.as_deref() == Some("policies.replace") {
                Some(decode_lower_hex_id(
                    &args
                        .remove("replaced_policy_id")
                        .ok_or(ClientControlError::MissingKey)?,
                )?)
            } else {
                None
            };
            if !args.is_empty()
                || replaced_policy_id == Some([0; 16])
                || !is_canonical_admin_principal_name(&name)
                || recipients.is_empty()
                || recipients
                    .iter()
                    .copied()
                    .collect::<std::collections::HashSet<_>>()
                    .len()
                    != recipients.len()
                || target_column_ids
                    .iter()
                    .copied()
                    .collect::<std::collections::HashSet<_>>()
                    .len()
                    != target_column_ids.len()
                || (kind == crate::storage_manager::PolicyKind::Row
                    && !target_column_ids.is_empty())
                || (kind == crate::storage_manager::PolicyKind::Cell
                    && (target_column_ids.is_empty()
                        || policy_operation == crate::authorization::Operation::Delete))
                || !match policy_operation {
                    crate::authorization::Operation::Select
                    | crate::authorization::Operation::Delete => {
                        using_expression.is_some() && with_check_expression.is_none()
                    }
                    crate::authorization::Operation::Insert => {
                        using_expression.is_none() && with_check_expression.is_some()
                    }
                    crate::authorization::Operation::Update => {
                        using_expression.is_some() && with_check_expression.is_some()
                    }
                    crate::authorization::Operation::Usage => false,
                }
            {
                return Err(ClientControlError::InvalidPrepare);
            }
            Ok(ClientAdminCommand::PrepareCreatePolicy {
                replaced_policy_id,
                table_id,
                name,
                kind,
                operation: policy_operation,
                effect,
                recipients,
                target_column_ids,
                using_expression,
                with_check_expression,
            })
        }
        Some("prepare") if version == Some(1) && operation.as_deref() == Some("policies.drop") => {
            if operation_id.is_some() || seen.len() != 4 {
                return Err(ClientControlError::UnknownKey);
            }
            let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
            let policy_id = decode_lower_hex_id(
                &args
                    .remove("policy_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            if !args.is_empty() {
                return Err(ClientControlError::UnknownKey);
            }
            Ok(ClientAdminCommand::PrepareDropPolicy { policy_id })
        }
        Some("prepare")
            if version == Some(1) && operation.as_deref() == Some("constraints.drop_check") =>
        {
            if operation_id.is_some() || seen.len() != 4 {
                return Err(ClientControlError::UnknownKey);
            }
            let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
            let constraint_id = decode_lower_hex_id(
                &args
                    .remove("constraint_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            if !args.is_empty() {
                return Err(ClientControlError::UnknownKey);
            }
            Ok(ClientAdminCommand::PrepareDropCheckConstraint { constraint_id })
        }
        Some("prepare")
            if version == Some(1)
                && matches!(
                    operation.as_deref(),
                    Some("constraints.drop_primary_key" | "constraints.drop_unique")
                ) =>
        {
            if operation_id.is_some() || seen.len() != 4 {
                return Err(ClientControlError::UnknownKey);
            }
            let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
            let constraint_id = decode_lower_hex_id(
                &args
                    .remove("constraint_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            if !args.is_empty() {
                return Err(ClientControlError::UnknownKey);
            }
            if operation.as_deref() == Some("constraints.drop_primary_key") {
                Ok(ClientAdminCommand::PrepareDropPrimaryKey { constraint_id })
            } else {
                Ok(ClientAdminCommand::PrepareDropUniqueConstraint { constraint_id })
            }
        }
        Some("prepare") if version == Some(1) && operation.as_deref() == Some("tables.rename") => {
            if operation_id.is_some() || seen.len() != 4 {
                return Err(ClientControlError::UnknownKey);
            }
            let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
            let table_id = decode_lower_hex_id(
                &args
                    .remove("table_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            let name = args.remove("name").ok_or(ClientControlError::MissingKey)?;
            if !args.is_empty() || !is_canonical_admin_principal_name(&name) {
                return Err(ClientControlError::InvalidPrepare);
            }
            Ok(ClientAdminCommand::PrepareRenameTable { table_id, name })
        }
        Some("prepare")
            if version == Some(1) && operation.as_deref() == Some("tables.set_owner") =>
        {
            if operation_id.is_some() || seen.len() != 4 {
                return Err(ClientControlError::UnknownKey);
            }
            let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
            let table_id = decode_lower_hex_id(
                &args
                    .remove("table_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            let owner_id = decode_lower_hex_id(
                &args
                    .remove("owner_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            if !args.is_empty() {
                return Err(ClientControlError::UnknownKey);
            }
            Ok(ClientAdminCommand::PrepareSetTableOwner { table_id, owner_id })
        }
        Some("prepare")
            if version == Some(1) && operation.as_deref() == Some("tables.rename_column") =>
        {
            if operation_id.is_some() || seen.len() != 4 {
                return Err(ClientControlError::UnknownKey);
            }
            let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
            let table_id = decode_lower_hex_id(
                &args
                    .remove("table_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            let column_id = decode_lower_hex_id(
                &args
                    .remove("column_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            let name = args.remove("name").ok_or(ClientControlError::MissingKey)?;
            if !args.is_empty() || !is_canonical_admin_principal_name(&name) {
                return Err(ClientControlError::InvalidPrepare);
            }
            Ok(ClientAdminCommand::PrepareRenameColumn {
                table_id,
                column_id,
                name,
            })
        }
        Some("prepare")
            if version == Some(1)
                && operation.as_deref() == Some("tables.alter_column_nullability") =>
        {
            if operation_id.is_some() || seen.len() != 4 {
                return Err(ClientControlError::UnknownKey);
            }
            let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
            let table_id = decode_lower_hex_id(
                &args
                    .remove("table_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            let column_id = decode_lower_hex_id(
                &args
                    .remove("column_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            let nullable = match args.remove("nullable").as_deref() {
                Some("true") => true,
                Some("false") => false,
                _ => return Err(ClientControlError::InvalidPrepare),
            };
            if !args.is_empty() {
                return Err(ClientControlError::UnknownKey);
            }
            Ok(ClientAdminCommand::PrepareSetColumnNullable {
                table_id,
                column_id,
                nullable,
            })
        }
        Some("prepare")
            if version == Some(1)
                && operation.as_deref() == Some("tables.widen_varchar_column") =>
        {
            if operation_id.is_some() || seen.len() != 4 {
                return Err(ClientControlError::UnknownKey);
            }
            let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
            let table_id = decode_lower_hex_id(
                &args
                    .remove("table_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            let column_id = decode_lower_hex_id(
                &args
                    .remove("column_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            let max_chars_text = args
                .remove("max_chars")
                .ok_or(ClientControlError::MissingKey)?;
            let max_chars = max_chars_text
                .parse::<u32>()
                .ok()
                .filter(|value| *value > 0 && value.to_string() == max_chars_text)
                .ok_or(ClientControlError::InvalidPrepare)?;
            if !args.is_empty() {
                return Err(ClientControlError::UnknownKey);
            }
            Ok(ClientAdminCommand::PrepareWidenVarcharColumn {
                table_id,
                column_id,
                max_chars,
            })
        }
        Some("prepare")
            if version == Some(1) && operation.as_deref() == Some("tables.set_column_default") =>
        {
            if operation_id.is_some() || seen.len() != 4 {
                return Err(ClientControlError::UnknownKey);
            }
            let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
            let table_id = decode_lower_hex_id(
                &args
                    .remove("table_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            let column_id = decode_lower_hex_id(
                &args
                    .remove("column_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            let scalar_type = decode_table_scalar_type(
                &args.remove("type").ok_or(ClientControlError::MissingKey)?,
            )?;
            if args.remove("default").as_deref() != Some("<structured-default>") || !args.is_empty()
            {
                return Err(ClientControlError::InvalidPrepare);
            }
            let (kind, value) = column_default.ok_or(ClientControlError::MissingKey)?;
            let default = decode_table_column_default(&kind, value, scalar_type)?;
            Ok(ClientAdminCommand::PrepareAlterColumnDefault {
                table_id,
                column_id,
                default: Some(default),
            })
        }
        Some("prepare")
            if version == Some(1) && operation.as_deref() == Some("tables.drop_column_default") =>
        {
            if operation_id.is_some() || seen.len() != 4 {
                return Err(ClientControlError::UnknownKey);
            }
            let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
            let table_id = decode_lower_hex_id(
                &args
                    .remove("table_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            let column_id = decode_lower_hex_id(
                &args
                    .remove("column_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            if !args.is_empty() {
                return Err(ClientControlError::UnknownKey);
            }
            Ok(ClientAdminCommand::PrepareAlterColumnDefault {
                table_id,
                column_id,
                default: None,
            })
        }
        Some("prepare")
            if version == Some(1) && operation.as_deref() == Some("tables.add_column") =>
        {
            if operation_id.is_some() || seen.len() != 4 {
                return Err(ClientControlError::UnknownKey);
            }
            let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
            let table_id = decode_lower_hex_id(
                &args
                    .remove("table_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            if args.remove("column").as_deref() != Some("<structured-column>") || !args.is_empty() {
                return Err(ClientControlError::InvalidPrepare);
            }
            let column = table_column.ok_or(ClientControlError::MissingKey)?;
            if column.generated_uuid_identity
                || (!column.nullable && column.default.is_none())
                || matches!(
                    column.default,
                    Some(crate::storage_manager::TableColumnDefault::CurrentPrincipalId)
                )
            {
                return Err(ClientControlError::InvalidPrepare);
            }
            Ok(ClientAdminCommand::PrepareAddColumn { table_id, column })
        }
        Some("prepare")
            if version == Some(1) && operation.as_deref() == Some("tables.drop_column") =>
        {
            if operation_id.is_some() || seen.len() != 4 {
                return Err(ClientControlError::UnknownKey);
            }
            let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
            let table_id = decode_lower_hex_id(
                &args
                    .remove("table_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            let column_id = decode_lower_hex_id(
                &args
                    .remove("column_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            if !args.is_empty() {
                return Err(ClientControlError::UnknownKey);
            }
            Ok(ClientAdminCommand::PrepareDropColumn {
                table_id,
                column_id,
            })
        }
        Some("prepare")
            if matches!(version, Some(1..=4)) && operation.as_deref() == Some("tables.create") =>
        {
            if operation_id.is_some() || !(4..=14).contains(&seen.len()) {
                return Err(ClientControlError::UnknownKey);
            }
            let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
            let schema_id = decode_lower_hex_id(
                &args
                    .remove("schema_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            let name = args.remove("name").ok_or(ClientControlError::MissingKey)?;
            let columns_value = args
                .remove("columns")
                .ok_or(ClientControlError::MissingKey)?;
            let columns = table_columns.ok_or(ClientControlError::MissingKey)?;
            let primary_key_marker = args.remove("primary_key_columns");
            if primary_key_marker.as_deref()
                != primary_key_columns.as_ref().map(|_| "<structured-names>")
            {
                return Err(ClientControlError::InvalidPrepare);
            }
            let unique_marker = args.remove("unique_constraints");
            if unique_marker.as_deref()
                != unique_constraints
                    .as_ref()
                    .map(|_| "<structured-name-arrays>")
            {
                return Err(ClientControlError::InvalidPrepare);
            }
            let unique_constraints = unique_constraints.unwrap_or_default();
            let check_marker = args.remove("check_expressions_eir1");
            if check_marker.as_deref()
                != check_expressions
                    .as_ref()
                    .map(|_| "<structured-check-expressions>")
            {
                return Err(ClientControlError::InvalidPrepare);
            }
            let check_expressions = check_expressions.unwrap_or_default();
            let names_marker = args.remove("check_constraint_names");
            if names_marker.as_deref()
                != check_constraint_names
                    .as_ref()
                    .map(|_| "<structured-check-names>")
                || (version.is_some_and(|version| version < 3) && check_constraint_names.is_some())
                || (version == Some(3) && check_constraint_names.is_none())
            {
                return Err(ClientControlError::InvalidPrepare);
            }
            let check_constraint_names = check_constraint_names
                .unwrap_or_default()
                .into_iter()
                .map(|name| if name.is_empty() { None } else { Some(name) })
                .collect::<Vec<_>>();
            if (!check_constraint_names.is_empty()
                && check_constraint_names.len() != check_expressions.len())
                || check_constraint_names
                    .iter()
                    .flatten()
                    .any(|name| !is_canonical_admin_principal_name(name))
                || check_constraint_names
                    .iter()
                    .flatten()
                    .collect::<std::collections::HashSet<_>>()
                    .len()
                    != check_constraint_names.iter().flatten().count()
            {
                return Err(ClientControlError::InvalidPrepare);
            }
            let source_marker = args.remove("foreign_key_source_columns");
            if source_marker.as_deref()
                != table_foreign_key_sources
                    .as_ref()
                    .map(|_| "<structured-foreign-key-sources>")
            {
                return Err(ClientControlError::InvalidPrepare);
            }
            let target_marker = args.remove("foreign_key_target_constraint_ids");
            if target_marker.as_deref()
                != table_foreign_key_targets
                    .as_ref()
                    .map(|_| "<structured-foreign-key-targets>")
            {
                return Err(ClientControlError::InvalidPrepare);
            }
            let source_columns = table_foreign_key_sources.unwrap_or_default();
            let target_constraint_ids = table_foreign_key_targets.unwrap_or_default();
            let target_columns_marker = args.remove("foreign_key_target_columns");
            if target_columns_marker.as_deref()
                != table_foreign_key_target_columns
                    .as_ref()
                    .map(|_| "<structured-foreign-key-target-columns>")
                || (version == Some(1) && table_foreign_key_target_columns.is_some())
            {
                return Err(ClientControlError::InvalidPrepare);
            }
            let target_columns = table_foreign_key_target_columns
                .unwrap_or_else(|| vec![Vec::new(); target_constraint_ids.len()]);
            if source_columns.len() != target_constraint_ids.len()
                || source_columns.len() != target_columns.len()
            {
                return Err(ClientControlError::InvalidPrepare);
            }
            let primary_key_name_field = args.remove("primary_key_name");
            let unique_names_marker = args.remove("unique_constraint_names");
            let foreign_names_marker = args.remove("foreign_key_names");
            if version == Some(4) {
                if primary_key_name_field.is_none()
                    || unique_names_marker.as_deref() != Some("<structured-unique-names>")
                    || foreign_names_marker.as_deref() != Some("<structured-foreign-key-names>")
                    || unique_constraint_names.is_none()
                    || foreign_key_names.is_none()
                {
                    return Err(ClientControlError::InvalidPrepare);
                }
            } else if primary_key_name_field.is_some()
                || unique_names_marker.is_some()
                || foreign_names_marker.is_some()
                || unique_constraint_names.is_some()
                || foreign_key_names.is_some()
            {
                return Err(ClientControlError::InvalidPrepare);
            }
            let primary_key_name = primary_key_name_field.filter(|name| !name.is_empty());
            let unique_constraint_names = unique_constraint_names
                .unwrap_or_default()
                .into_iter()
                .map(|name| if name.is_empty() { None } else { Some(name) })
                .collect::<Vec<_>>();
            let foreign_key_names = foreign_key_names
                .unwrap_or_default()
                .into_iter()
                .map(|name| if name.is_empty() { None } else { Some(name) })
                .collect::<Vec<_>>();
            let named_constraint_names = primary_key_name
                .iter()
                .chain(unique_constraint_names.iter().flatten())
                .chain(check_constraint_names.iter().flatten())
                .chain(foreign_key_names.iter().flatten())
                .collect::<std::collections::HashSet<_>>();
            if primary_key_name
                .as_ref()
                .is_some_and(|_| primary_key_columns.is_none())
                || (!unique_constraint_names.is_empty()
                    && unique_constraint_names.len() != unique_constraints.len())
                || (!foreign_key_names.is_empty()
                    && foreign_key_names.len() != target_columns.len())
                || unique_constraint_names
                    .iter()
                    .flatten()
                    .chain(foreign_key_names.iter().flatten())
                    .chain(check_constraint_names.iter().flatten())
                    .chain(primary_key_name.iter())
                    .any(|name| !is_canonical_admin_principal_name(name))
                || named_constraint_names.len()
                    != usize::from(primary_key_name.is_some())
                        + unique_constraint_names.iter().flatten().count()
                        + check_constraint_names.iter().flatten().count()
                        + foreign_key_names.iter().flatten().count()
            {
                return Err(ClientControlError::InvalidPrepare);
            }
            let foreign_keys = source_columns
                .into_iter()
                .zip(target_constraint_ids.into_iter().zip(target_columns))
                .map(|(source_columns, (target_constraint_id, target_columns))| {
                    let target_columns = if target_columns.is_empty() {
                        None
                    } else {
                        Some(target_columns)
                    };
                    if source_columns.is_empty()
                        || source_columns.len() > columns.len()
                        || (target_constraint_id == [0; 16]) != target_columns.is_some()
                        || target_columns.as_ref().is_some_and(|names| {
                            names.len() > columns.len()
                                || names.iter().collect::<std::collections::HashSet<_>>().len()
                                    != names.len()
                                || names
                                    .iter()
                                    .any(|name| !columns.iter().any(|column| column.name == *name))
                        })
                        || source_columns
                            .iter()
                            .collect::<std::collections::HashSet<_>>()
                            .len()
                            != source_columns.len()
                        || source_columns
                            .iter()
                            .any(|name| !columns.iter().any(|column| column.name == *name))
                    {
                        return Err(ClientControlError::InvalidPrepare);
                    }
                    Ok(crate::storage_manager::CreateTableForeignKeyDefinition {
                        source_columns,
                        target_constraint_id,
                        target_columns,
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            if columns_value != "<structured-columns>"
                || !is_canonical_admin_principal_name(&name)
                || columns.is_empty()
                || columns.len() > crate::tuple::MAX_COLUMNS
                || (primary_key_columns.is_some()
                    && (primary_key_columns.as_ref().is_some_and(Vec::is_empty)
                        || primary_key_columns.as_ref().is_some_and(|names| {
                            names.len() > columns.len()
                                || names.iter().collect::<std::collections::HashSet<_>>().len()
                                    != names.len()
                                || names.iter().any(|name| {
                                    !columns
                                        .iter()
                                        .any(|column| column.name == *name && !column.nullable)
                                })
                        })))
                || !args.is_empty()
                || unique_constraints.len() > crate::tuple::MAX_COLUMNS
                || unique_constraints.iter().any(|names| {
                    names.is_empty()
                        || names.len() > columns.len()
                        || names.iter().collect::<std::collections::HashSet<_>>().len()
                            != names.len()
                        || names
                            .iter()
                            .any(|name| !columns.iter().any(|column| column.name == *name))
                })
                || unique_constraints
                    .iter()
                    .enumerate()
                    .any(|(index, names)| unique_constraints[..index].contains(names))
                || check_expressions.len() > crate::tuple::MAX_COLUMNS
                || foreign_keys.len() > crate::tuple::MAX_COLUMNS
            {
                return Err(ClientControlError::InvalidPrepare);
            }
            Ok(ClientAdminCommand::PrepareCreateTable {
                schema_id,
                name,
                columns,
                primary_key_columns,
                primary_key_name,
                unique_constraints,
                unique_constraint_names,
                check_expressions,
                check_constraint_names,
                foreign_keys,
                foreign_key_names,
            })
        }
        Some("prepare") if version == Some(1) && operation.as_deref() == Some("users.rename") => {
            decode_identity_rename_prepare(arguments, operation_id, seen.len(), "user_id")
        }
        Some("prepare") if version == Some(1) && operation.as_deref() == Some("roles.rename") => {
            decode_identity_rename_prepare(arguments, operation_id, seen.len(), "role_id")
        }
        Some("prepare") if version == Some(1) && operation.as_deref() == Some("users.drop") => {
            decode_identity_drop_prepare(arguments, operation_id, seen.len(), "user_id")
        }
        Some("prepare") if version == Some(1) && operation.as_deref() == Some("roles.drop") => {
            decode_identity_drop_prepare(arguments, operation_id, seen.len(), "role_id")
        }
        Some("prepare") if version == Some(1) && operation.as_deref() == Some("users.login") => {
            decode_user_login_prepare(arguments, operation_id, seen.len())
        }
        Some("prepare") if version == Some(1) && operation.as_deref() == Some("roles.grant") => {
            decode_role_membership_prepare(arguments, operation_id, seen.len(), true)
        }
        Some("prepare") if version == Some(1) && operation.as_deref() == Some("roles.revoke") => {
            decode_role_membership_prepare(arguments, operation_id, seen.len(), false)
        }
        Some("prepare")
            if version == Some(1) && operation.as_deref() == Some("privileges.grant") =>
        {
            decode_system_privilege_prepare(arguments, operation_id, seen.len(), true)
        }
        Some("prepare")
            if version == Some(1) && operation.as_deref() == Some("privileges.revoke") =>
        {
            decode_system_privilege_prepare(arguments, operation_id, seen.len(), false)
        }
        Some("prepare") if version == Some(1) && operation.as_deref() == Some("objects.grant") => {
            decode_object_grant_prepare(arguments, operation_id, seen.len(), true)
        }
        Some("prepare") if version == Some(1) && operation.as_deref() == Some("objects.revoke") => {
            decode_object_grant_prepare(arguments, operation_id, seen.len(), false)
        }
        Some("prepare")
            if version == Some(1) && operation.as_deref() == Some("objects.grant_batch") =>
        {
            decode_object_grant_prepare(arguments, operation_id, seen.len(), true)
        }
        Some("prepare")
            if version == Some(1) && operation.as_deref() == Some("objects.revoke_batch") =>
        {
            decode_object_grant_prepare(arguments, operation_id, seen.len(), false)
        }
        Some("prepare")
            if version == Some(1) && operation.as_deref() == Some("objects.grant_entries") =>
        {
            decode_object_grant_prepare(arguments, operation_id, seen.len(), true)
        }
        Some("prepare")
            if version == Some(1) && operation.as_deref() == Some("objects.revoke_entries") =>
        {
            decode_object_grant_prepare(arguments, operation_id, seen.len(), false)
        }
        Some("prepare") if version == Some(1) && operation.as_deref() == Some("tokens.revoke") => {
            if operation_id.is_some() || seen.len() != 4 {
                return Err(ClientControlError::UnknownKey);
            }
            let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
            let token_id = decode_lower_hex_id(
                &args
                    .remove("token_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            if !args.is_empty() {
                return Err(ClientControlError::UnknownKey);
            }
            Ok(ClientAdminCommand::PrepareRevoke { token_id })
        }
        Some("prepare") if version == Some(1) && operation.as_deref() == Some("tokens.issue") => {
            if operation_id.is_some() || seen.len() != 4 {
                return Err(ClientControlError::UnknownKey);
            }
            let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
            let user_id = decode_lower_hex_id(
                &args
                    .remove("user_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            let label = args.remove("label").ok_or(ClientControlError::MissingKey)?;
            let expires_at_us = args
                .remove("expires_at_us")
                .ok_or(ClientControlError::MissingKey)?;
            if !args.is_empty() {
                return Err(ClientControlError::UnknownKey);
            }
            if label.is_empty() || label.len() > 128 {
                return Err(ClientControlError::InvalidPrepare);
            }
            let expires_at_us = parse_canonical_positive_i64(&expires_at_us)
                .ok_or(ClientControlError::InvalidPrepare)?;
            Ok(ClientAdminCommand::PrepareIssue {
                user_id,
                label,
                expires_at_us,
            })
        }
        Some("prepare") if version == Some(1) && operation.as_deref() == Some("tokens.rotate") => {
            if operation_id.is_some() || seen.len() != 4 {
                return Err(ClientControlError::UnknownKey);
            }
            let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
            let old_token_id = decode_lower_hex_id(
                &args
                    .remove("old_token_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            let label = args.remove("label").ok_or(ClientControlError::MissingKey)?;
            let expires_at_us = parse_canonical_positive_i64(
                &args
                    .remove("expires_at_us")
                    .ok_or(ClientControlError::MissingKey)?,
            )
            .ok_or(ClientControlError::InvalidPrepare)?;
            if !args.is_empty() || label.is_empty() || label.len() > 128 {
                return Err(ClientControlError::InvalidPrepare);
            }
            Ok(ClientAdminCommand::PrepareRotateToken {
                old_token_id,
                label,
                expires_at_us,
            })
        }
        Some("start") if version.is_none() && operation.is_none() => {
            if arguments.is_some() || seen.len() != 2 {
                return Err(ClientControlError::UnknownKey);
            }
            let operation_id =
                decode_lower_hex_id(&operation_id.ok_or(ClientControlError::MissingKey)?)?;
            Ok(ClientAdminCommand::Start { operation_id })
        }
        _ => Err(ClientControlError::UnsupportedAdminOperation),
    }
}

fn decode_role_membership_prepare(
    arguments: Option<HashMap<String, String>>,
    operation_id: Option<String>,
    seen_len: usize,
    grant: bool,
) -> Result<ClientAdminCommand, ClientControlError> {
    if operation_id.is_some() || seen_len != 4 {
        return Err(ClientControlError::UnknownKey);
    }
    let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
    let member_id = decode_lower_hex_id(
        &args
            .remove("member_id")
            .ok_or(ClientControlError::MissingKey)?,
    )?;
    let role_id = decode_lower_hex_id(
        &args
            .remove("role_id")
            .ok_or(ClientControlError::MissingKey)?,
    )?;
    if !args.is_empty() || member_id == role_id {
        return Err(ClientControlError::InvalidPrepare);
    }
    Ok(if grant {
        ClientAdminCommand::PrepareGrantRole { member_id, role_id }
    } else {
        ClientAdminCommand::PrepareRevokeRole { member_id, role_id }
    })
}

fn decode_system_privilege_prepare(
    arguments: Option<HashMap<String, String>>,
    operation_id: Option<String>,
    seen_len: usize,
    grant: bool,
) -> Result<ClientAdminCommand, ClientControlError> {
    if operation_id.is_some() || seen_len != 4 {
        return Err(ClientControlError::UnknownKey);
    }
    let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
    let grantee_id = decode_lower_hex_id(
        &args
            .remove("grantee_id")
            .ok_or(ClientControlError::MissingKey)?,
    )?;
    let privilege = match args
        .remove("privilege")
        .ok_or(ClientControlError::MissingKey)?
        .as_str()
    {
        "manage_users" => crate::storage_manager::AdministrativeSystemPrivilege::ManageUsers,
        "manage_roles" => crate::storage_manager::AdministrativeSystemPrivilege::ManageRoles,
        "create_schema" => crate::storage_manager::AdministrativeSystemPrivilege::CreateSchema,
        _ => return Err(ClientControlError::InvalidPrepare),
    };
    if !args.is_empty() {
        return Err(ClientControlError::UnknownKey);
    }
    Ok(ClientAdminCommand::PrepareSystemPrivilege {
        grantee_id,
        privilege,
        grant,
    })
}

fn decode_object_grant_prepare(
    arguments: Option<HashMap<String, String>>,
    operation_id: Option<String>,
    seen_len: usize,
    grant: bool,
) -> Result<ClientAdminCommand, ClientControlError> {
    if operation_id.is_some() || seen_len != 4 {
        return Err(ClientControlError::UnknownKey);
    }
    let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
    let grantee_id = decode_lower_hex_id(
        &args
            .remove("grantee_id")
            .ok_or(ClientControlError::MissingKey)?,
    )?;
    let object_id = decode_lower_hex_id(
        &args
            .remove("object_id")
            .ok_or(ClientControlError::MissingKey)?,
    )?;
    if let Some(encoded_entries) = args.remove("entries") {
        if !args.is_empty() || encoded_entries.is_empty() {
            return Err(ClientControlError::InvalidPrepare);
        }
        let mut entries = Vec::new();
        for encoded in encoded_entries.split(';') {
            let (encoded_privilege, encoded_target) = encoded
                .split_once('@')
                .ok_or(ClientControlError::InvalidPrepare)?;
            let privilege = decode_object_privilege(encoded_privilege)?;
            let (scope, ids) = encoded_target
                .split_once(':')
                .ok_or(ClientControlError::InvalidPrepare)?;
            let target = match scope {
                "object" if decode_lower_hex_id(ids)? == object_id => {
                    crate::storage_manager::ObjectGrantTarget::Object
                }
                "column" => {
                    crate::storage_manager::ObjectGrantTarget::Column(decode_lower_hex_id(ids)?)
                }
                "columns" => {
                    let mut values = Vec::new();
                    for value in ids.split('+') {
                        values.push(decode_lower_hex_id(value)?);
                    }
                    if !(2..=64).contains(&values.len()) {
                        return Err(ClientControlError::InvalidPrepare);
                    }
                    let mut seen = HashSet::new();
                    if values.iter().any(|value| !seen.insert(*value)) {
                        return Err(ClientControlError::InvalidPrepare);
                    }
                    crate::storage_manager::ObjectGrantTarget::Columns(values)
                }
                "all_columns" if ids.is_empty() => {
                    crate::storage_manager::ObjectGrantTarget::AllColumns
                }
                _ => return Err(ClientControlError::InvalidPrepare),
            };
            if entries
                .iter()
                .any(|entry: &crate::storage_manager::ObjectGrantEntry| {
                    entry.privilege == privilege && entry.target == target
                })
            {
                return Err(ClientControlError::InvalidPrepare);
            }
            entries.push(crate::storage_manager::ObjectGrantEntry { privilege, target });
            if entries.len() > 64 {
                return Err(ClientControlError::InvalidPrepare);
            }
        }
        return Ok(ClientAdminCommand::PrepareObjectGrant {
            grantee_id,
            object_id,
            privileges: Vec::new(),
            target: crate::storage_manager::ObjectGrantTarget::Object,
            entries: Some(entries),
            grant,
        });
    }
    let encoded_privileges = match (args.remove("privilege"), args.remove("privileges")) {
        (Some(single), None) => single,
        (None, Some(multiple)) => multiple,
        _ => return Err(ClientControlError::InvalidPrepare),
    };
    let mut privileges = Vec::new();
    for encoded in encoded_privileges.split(',') {
        let privilege = decode_object_privilege(encoded)?;
        if privileges.contains(&privilege) {
            return Err(ClientControlError::InvalidPrepare);
        }
        privileges.push(privilege);
    }
    if privileges.is_empty() || privileges.len() > 11 {
        return Err(ClientControlError::InvalidPrepare);
    }
    let target_scope = args
        .remove("target_scope")
        .ok_or(ClientControlError::MissingKey)?;
    let target = match target_scope.as_str() {
        "object" => {
            let target_id = decode_lower_hex_id(
                &args
                    .remove("target_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            if target_id != object_id {
                return Err(ClientControlError::InvalidPrepare);
            }
            crate::storage_manager::ObjectGrantTarget::Object
        }
        "column" => {
            let target_id = decode_lower_hex_id(
                &args
                    .remove("target_id")
                    .ok_or(ClientControlError::MissingKey)?,
            )?;
            crate::storage_manager::ObjectGrantTarget::Column(target_id)
        }
        "columns" => {
            let encoded_ids = args
                .remove("target_ids")
                .ok_or(ClientControlError::MissingKey)?;
            let encoded_ids = encoded_ids.split(',').collect::<Vec<_>>();
            if !(2..=64).contains(&encoded_ids.len()) {
                return Err(ClientControlError::InvalidPrepare);
            }
            let mut target_ids = Vec::with_capacity(encoded_ids.len());
            let mut seen_ids = HashSet::with_capacity(encoded_ids.len());
            for encoded_id in encoded_ids {
                let target_id = decode_lower_hex_id(encoded_id)?;
                if !seen_ids.insert(target_id) {
                    return Err(ClientControlError::InvalidPrepare);
                }
                target_ids.push(target_id);
            }
            crate::storage_manager::ObjectGrantTarget::Columns(target_ids)
        }
        "all_columns" => crate::storage_manager::ObjectGrantTarget::AllColumns,
        _ => return Err(ClientControlError::InvalidPrepare),
    };
    if !args.is_empty() {
        return Err(ClientControlError::UnknownKey);
    }
    Ok(ClientAdminCommand::PrepareObjectGrant {
        grantee_id,
        object_id,
        privileges,
        target,
        entries: None,
        grant,
    })
}

fn decode_object_privilege(
    encoded: &str,
) -> Result<crate::storage_manager::ObjectGrantPrivilege, ClientControlError> {
    match encoded {
        "usage" => Ok(crate::storage_manager::ObjectGrantPrivilege::Usage),
        "select" => Ok(crate::storage_manager::ObjectGrantPrivilege::Select),
        "insert" => Ok(crate::storage_manager::ObjectGrantPrivilege::Insert),
        "update" => Ok(crate::storage_manager::ObjectGrantPrivilege::Update),
        "delete" => Ok(crate::storage_manager::ObjectGrantPrivilege::Delete),
        "references" => Ok(crate::storage_manager::ObjectGrantPrivilege::References),
        "create" => Ok(crate::storage_manager::ObjectGrantPrivilege::Create),
        "alter" => Ok(crate::storage_manager::ObjectGrantPrivilege::Alter),
        "index" => Ok(crate::storage_manager::ObjectGrantPrivilege::Index),
        "manage_policies" => Ok(crate::storage_manager::ObjectGrantPrivilege::ManagePolicies),
        "manage_grants" => Ok(crate::storage_manager::ObjectGrantPrivilege::ManageGrants),
        _ => Err(ClientControlError::InvalidPrepare),
    }
}

fn decode_optional_policy_expression(
    value: String,
) -> Result<Option<crate::authorization::Expr>, ClientControlError> {
    if value == "<null>" {
        return Ok(None);
    }
    let bytes = decode_lower_hex_bytes(&value, crate::expression::MAX_EXPRESSION_BYTES)?;
    crate::expression::decode_expression(&bytes)
        .map(Some)
        .map_err(|_| ClientControlError::InvalidPrepare)
}

fn decode_identity_rename_prepare(
    arguments: Option<HashMap<String, String>>,
    operation_id: Option<String>,
    seen_len: usize,
    id_field: &str,
) -> Result<ClientAdminCommand, ClientControlError> {
    if operation_id.is_some() || seen_len != 4 {
        return Err(ClientControlError::UnknownKey);
    }
    let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
    let identity_id = decode_lower_hex_id(
        &args
            .remove(id_field)
            .ok_or(ClientControlError::MissingKey)?,
    )?;
    let name = args.remove("name").ok_or(ClientControlError::MissingKey)?;
    if !is_canonical_admin_principal_name(&name) || !args.is_empty() {
        return Err(ClientControlError::InvalidPrepare);
    }
    Ok(ClientAdminCommand::PrepareRenameIdentity { identity_id, name })
}

fn decode_user_login_prepare(
    arguments: Option<HashMap<String, String>>,
    operation_id: Option<String>,
    seen_len: usize,
) -> Result<ClientAdminCommand, ClientControlError> {
    if operation_id.is_some() || seen_len != 4 {
        return Err(ClientControlError::UnknownKey);
    }
    let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
    let user_id = decode_lower_hex_id(
        &args
            .remove("user_id")
            .ok_or(ClientControlError::MissingKey)?,
    )?;
    let enabled = match args
        .remove("enabled")
        .ok_or(ClientControlError::MissingKey)?
        .as_str()
    {
        "true" => true,
        "false" => false,
        _ => return Err(ClientControlError::InvalidPrepare),
    };
    if !args.is_empty() {
        return Err(ClientControlError::UnknownKey);
    }
    Ok(ClientAdminCommand::PrepareSetUserLogin { user_id, enabled })
}

fn decode_identity_drop_prepare(
    arguments: Option<HashMap<String, String>>,
    operation_id: Option<String>,
    seen_len: usize,
    id_field: &str,
) -> Result<ClientAdminCommand, ClientControlError> {
    if operation_id.is_some() || seen_len != 4 {
        return Err(ClientControlError::UnknownKey);
    }
    let mut args = arguments.ok_or(ClientControlError::MissingKey)?;
    let identity_id = decode_lower_hex_id(
        &args
            .remove(id_field)
            .ok_or(ClientControlError::MissingKey)?,
    )?;
    if !args.is_empty() {
        return Err(ClientControlError::UnknownKey);
    }
    Ok(ClientAdminCommand::PrepareDropIdentity { identity_id })
}

fn is_canonical_admin_principal_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 128
        && (bytes[0].is_ascii_lowercase() || bytes[0] == b'_')
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

fn parse_canonical_positive_i64(value: &str) -> Option<i64> {
    if value.is_empty()
        || (value.len() > 1 && value.starts_with('0'))
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    value.parse::<i64>().ok().filter(|value| *value > 0)
}

impl Drop for ClientCredentials {
    fn drop(&mut self) {
        self.token_secret.zeroize();
    }
}

/// Decode the exact HELLO v1 JSON object. Keys and values are strictly typed
/// strings; duplicate and unknown keys are rejected.
pub fn decode_client_hello(payload: &[u8]) -> Result<ClientHello, ClientControlError> {
    let mut fields = parse_string_object(payload)?;
    let mode = match take_required(&mut fields, "mode")?.as_str() {
        "query" => ClientMode::Query,
        "control" => ClientMode::Control,
        _ => return Err(ClientControlError::InvalidMode),
    };
    let client_name = take_required(&mut fields, "client_name")?;
    if client_name.len() > 128 {
        return Err(ClientControlError::InvalidClientName);
    }
    let repository_sdk_library = match fields.remove("sdk_library_mode") {
        None => false,
        Some(mode) if mode == "repository" => true,
        Some(_) => return Err(ClientControlError::InvalidClientName),
    };
    if repository_sdk_library && mode != ClientMode::Query {
        return Err(ClientControlError::InvalidMode);
    }
    reject_unknown_fields(fields)?;
    Ok(ClientHello {
        mode,
        client_name,
        repository_sdk_library,
    })
}

/// Decode the exact AUTH v1 JSON object, requiring a lowercase opaque token
/// ID and canonical padded base64 for the fixed-size bearer secret.
pub fn decode_client_credentials(payload: &[u8]) -> Result<ClientCredentials, ClientControlError> {
    let mut fields = parse_string_object(payload)?;
    let token_id = decode_lower_hex_id(&take_required(&mut fields, "token_id")?)?;
    let secret_bytes = decode_canonical_base64(&take_required(&mut fields, "token_secret")?)?;
    let token_secret = secret_bytes
        .as_slice()
        .try_into()
        .map_err(|_| ClientControlError::InvalidTokenSecret)?;
    reject_unknown_fields(fields)?;
    Ok(ClientCredentials {
        token_id,
        token_secret,
    })
}

/// Decode ARTIFACT_CHUNK/PARAMETER_CHUNK as invocation ID, little-endian
/// contiguous offset, and 1–256 KiB of opaque data.
pub fn decode_client_chunk(payload: &[u8]) -> Result<ClientChunk, ClientControlError> {
    if !(25..=24 + CLIENT_MAX_CHUNK_BYTES).contains(&payload.len()) {
        return Err(ClientControlError::InvalidChunk);
    }
    let invocation_id: [u8; 16] = payload[..16]
        .try_into()
        .map_err(|_| ClientControlError::InvalidChunk)?;
    if invocation_id == [0; 16] {
        return Err(ClientControlError::InvalidChunk);
    }
    let offset = u64::from_le_bytes(
        payload[16..24]
            .try_into()
            .map_err(|_| ClientControlError::InvalidChunk)?,
    );
    let mut data = Vec::new();
    data.try_reserve_exact(payload.len() - 24)
        .map_err(|_| ClientControlError::InvalidChunk)?;
    data.extend_from_slice(&payload[24..]);
    Ok(ClientChunk {
        invocation_id,
        offset,
        data,
    })
}

/// Decode the exact PREPARE JSON schema, validating declared sizes and all
/// canonical representations before allocating decoded metadata buffers.
pub fn decode_client_prepare(payload: &[u8]) -> Result<ClientPrepare, ClientControlError> {
    decode_client_prepare_for_protocol(payload, 1)
}

/// Decode PREPARE using the negotiated protocol major. Protocol v2 permits a
/// package to bind its immutable staged bytes to a retained transaction; v1
/// rejects those fields and keeps its original one-shot semantics.
pub fn decode_client_prepare_for_protocol(
    payload: &[u8],
    protocol_major: u16,
) -> Result<ClientPrepare, ClientControlError> {
    if !matches!(protocol_major, 1 | 2) {
        return Err(ClientControlError::InvalidPrepare);
    }
    let mut fields = parse_scalar_object(payload)?;
    let manifest_version = take_number(&mut fields, "manifest_version")?
        .try_into()
        .map_err(|_| ClientControlError::InvalidPrepare)?;
    let manifest_text = take_string(&mut fields, "manifest")?;
    let artifact_bytes = take_decimal_string(&mut fields, "artifact_bytes")?;
    let artifact_sha256 = decode_lower_hex_hash(&take_string(&mut fields, "artifact_sha256")?)?;
    let repository_sdk_library = match fields.remove("sdk_library_mode") {
        None => false,
        Some(JsonScalar::String(value)) if value == "repository" => true,
        Some(_) => return Err(ClientControlError::InvalidPrepare),
    };
    let has_sql_library =
        fields.contains_key("sql_library_bytes") || fields.contains_key("sql_library_sha256");
    let parameter_bytes = take_decimal_string(&mut fields, "parameter_bytes")?;
    let parameter_sha256 = decode_lower_hex_hash(&take_string(&mut fields, "parameter_sha256")?)?;
    let scope_version = take_number(&mut fields, "scope_version")?
        .try_into()
        .map_err(|_| ClientControlError::InvalidPrepare)?;
    let scope_text = take_string(&mut fields, "requested_scope")?;
    let deadline_ms = take_decimal_string(&mut fields, "deadline_ms")?;
    let workload_class = match take_string(&mut fields, "workload_class")?.as_str() {
        "operational" => crate::supervisor::WorkloadClass::Operational,
        "analytical" => crate::supervisor::WorkloadClass::Analytical,
        _ => return Err(ClientControlError::InvalidPrepare),
    };
    let transaction_id = fields
        .contains_key("transaction_id")
        .then(|| take_string(&mut fields, "transaction_id"))
        .transpose()?
        .map(|value| decode_lower_hex_id(&value).map_err(|_| ClientControlError::InvalidPrepare))
        .transpose()?;
    let package_id = fields
        .contains_key("package_id")
        .then(|| take_string(&mut fields, "package_id"))
        .transpose()?
        .map(|value| decode_lower_hex_id(&value).map_err(|_| ClientControlError::InvalidPrepare))
        .transpose()?;
    if transaction_id.is_some() != package_id.is_some()
        || protocol_major == 1 && transaction_id.is_some()
        || !repository_sdk_library
    {
        return Err(ClientControlError::InvalidPrepare);
    }
    reject_unknown_scalars(fields)?;
    if artifact_bytes == 0
        || artifact_bytes > crate::supervisor::MAX_ARTIFACT_BYTES
        || has_sql_library
        || parameter_bytes > crate::supervisor::MAX_PARAMETER_BYTES
        || deadline_ms == 0
        || manifest_text.len() > crate::supervisor::MAX_MANIFEST_BYTES.div_ceil(3) * 4
        || scope_text.len() > crate::supervisor::MAX_SCOPE_BYTES.div_ceil(3) * 4
    {
        return Err(ClientControlError::InvalidPrepare);
    }
    let manifest = decode_base64_bytes(&manifest_text)
        .filter(|bytes| !bytes.is_empty() && bytes.len() <= crate::supervisor::MAX_MANIFEST_BYTES)
        .ok_or(ClientControlError::InvalidPrepare)?;
    let requested_scope = decode_base64_bytes(&scope_text)
        .filter(|bytes| !bytes.is_empty() && bytes.len() <= crate::supervisor::MAX_SCOPE_BYTES)
        .ok_or(ClientControlError::InvalidPrepare)?;
    Ok(ClientPrepare {
        manifest_version,
        manifest,
        artifact_bytes,
        artifact_sha256,
        repository_sdk_library,
        parameter_bytes,
        parameter_sha256,
        scope_version,
        requested_scope,
        deadline_ms,
        workload_class,
        transaction_id,
        package_id,
    })
}

pub fn decode_client_seal(payload: &[u8]) -> Result<[u8; 16], ClientControlError> {
    let mut fields = parse_string_object(payload)?;
    let invocation_id = decode_lower_hex_id(&take_required(&mut fields, "invocation_id")?)
        .map_err(|_| ClientControlError::InvalidInvocationId)?;
    reject_unknown_fields(fields)?;
    Ok(invocation_id)
}

pub struct ClientStartPayload {
    pub invocation_id: [u8; 16],
    pub transaction_id: Option<[u8; 16]>,
    pub package_id: Option<[u8; 16]>,
    pub actor: Option<ClientStartActorCredentials>,
}

pub struct ClientStartActorCredentials {
    pub expected_name: String,
    pub token_id: [u8; 16],
    pub token_secret: Zeroizing<[u8; TOKEN_SECRET_BYTES]>,
}

pub(crate) struct ValidatedInteractiveStartCredentials {
    pub controller_token_id: [u8; 16],
    pub controller_token_secret: Zeroizing<[u8; TOKEN_SECRET_BYTES]>,
    pub actor: Option<ClientStartActorCredentials>,
}

/// Decode START identity fields under the negotiated protocol. Explicit v2
/// packages must echo the PREPARE transaction/package binding; v1 cannot
/// carry these fields.
pub fn decode_client_start_for_protocol(
    payload: &[u8],
    protocol_major: u16,
) -> Result<ClientStartPayload, ClientControlError> {
    if !matches!(protocol_major, 1 | 2) || payload.len() > CLIENT_MAX_CONTROL_BYTES {
        return Err(ClientControlError::InvalidInvocationId);
    }
    let mut fields = parse_string_object(payload)?;
    let invocation_id = decode_lower_hex_id(&take_required(&mut fields, "invocation_id")?)
        .map_err(|_| ClientControlError::InvalidInvocationId)?;
    let transaction_id = fields
        .remove("transaction_id")
        .map(|value| decode_lower_hex_id(&value).map_err(|_| ClientControlError::InvalidPrepare))
        .transpose()?;
    let package_id = fields
        .remove("package_id")
        .map(|value| decode_lower_hex_id(&value).map_err(|_| ClientControlError::InvalidPrepare))
        .transpose()?;
    let has_actor_name = fields.contains_key("expected_actor_name");
    let has_actor_token_id = fields.contains_key("actor_token_id");
    let has_actor_token_secret = fields.contains_key("actor_token_secret");
    let actor = match (has_actor_name, has_actor_token_id, has_actor_token_secret) {
        (false, false, false) => None,
        (true, true, true) if protocol_major == 2 => {
            let expected_name = take_required(&mut fields, "expected_actor_name")?;
            if !is_canonical_admin_principal_name(&expected_name) {
                return Err(ClientControlError::InvalidPrepare);
            }
            let token_id = decode_lower_hex_id(&take_required(&mut fields, "actor_token_id")?)
                .map_err(|_| ClientControlError::InvalidPrepare)?;
            let secret_text = Zeroizing::new(take_required(&mut fields, "actor_token_secret")?);
            let secret_bytes = decode_canonical_base64(&secret_text)?;
            let token_secret = <[u8; TOKEN_SECRET_BYTES]>::try_from(secret_bytes.as_slice())
                .map(Zeroizing::new)
                .map_err(|_| ClientControlError::InvalidTokenSecret)?;
            Some(ClientStartActorCredentials {
                expected_name,
                token_id,
                token_secret,
            })
        }
        _ => return Err(ClientControlError::InvalidPrepare),
    };
    if transaction_id.is_some() != package_id.is_some()
        || protocol_major == 1 && transaction_id.is_some()
    {
        return Err(ClientControlError::InvalidPrepare);
    }
    reject_unknown_fields(fields)?;
    Ok(ClientStartPayload {
        invocation_id,
        transaction_id,
        package_id,
        actor,
    })
}

#[derive(Debug)]
enum JsonScalar {
    String(String),
    Number(u64),
    Boolean(bool),
}

fn parse_scalar_object(payload: &[u8]) -> Result<HashMap<String, JsonScalar>, ClientControlError> {
    if payload.len() > CLIENT_MAX_CONTROL_BYTES {
        return Err(ClientControlError::InvalidJson);
    }
    let text = std::str::from_utf8(payload).map_err(|_| ClientControlError::InvalidJson)?;
    let mut parser = JsonStringObjectParser {
        text: text.as_bytes(),
        offset: 0,
    };
    parser.skip_whitespace();
    parser.expect(b'{')?;
    let mut fields = HashMap::new();
    parser.skip_whitespace();
    if !parser.consume(b'}') {
        loop {
            parser.skip_whitespace();
            let key = parser.parse_string()?;
            parser.skip_whitespace();
            parser.expect(b':')?;
            parser.skip_whitespace();
            let value = if parser.text.get(parser.offset) == Some(&b'"') {
                JsonScalar::String(parser.parse_string()?)
            } else if parser
                .text
                .get(parser.offset)
                .is_some_and(u8::is_ascii_digit)
            {
                JsonScalar::Number(parser.parse_u64_number()?)
            } else {
                JsonScalar::Boolean(parser.parse_boolean()?)
            };
            if fields.insert(key, value).is_some() {
                return Err(ClientControlError::DuplicateKey);
            }
            parser.skip_whitespace();
            if parser.consume(b'}') {
                break;
            }
            parser.expect(b',')?;
        }
    }
    parser.skip_whitespace();
    if parser.offset != parser.text.len() {
        return Err(ClientControlError::InvalidJson);
    }
    Ok(fields)
}

fn take_string(
    fields: &mut HashMap<String, JsonScalar>,
    key: &str,
) -> Result<String, ClientControlError> {
    match fields.remove(key).ok_or(ClientControlError::MissingKey)? {
        JsonScalar::String(value) => Ok(value),
        JsonScalar::Number(_) | JsonScalar::Boolean(_) => Err(ClientControlError::InvalidPrepare),
    }
}

fn take_number(
    fields: &mut HashMap<String, JsonScalar>,
    key: &str,
) -> Result<u64, ClientControlError> {
    match fields.remove(key).ok_or(ClientControlError::MissingKey)? {
        JsonScalar::Number(value) => Ok(value),
        JsonScalar::String(_) | JsonScalar::Boolean(_) => Err(ClientControlError::InvalidPrepare),
    }
}

fn take_bool(
    fields: &mut HashMap<String, JsonScalar>,
    key: &str,
) -> Result<bool, ClientControlError> {
    match fields.remove(key).ok_or(ClientControlError::MissingKey)? {
        JsonScalar::Boolean(value) => Ok(value),
        JsonScalar::String(_) | JsonScalar::Number(_) => Err(ClientControlError::InvalidPrepare),
    }
}

fn take_decimal_string(
    fields: &mut HashMap<String, JsonScalar>,
    key: &str,
) -> Result<u64, ClientControlError> {
    let value = take_string(fields, key)?;
    if value.is_empty()
        || (value.len() > 1 && value.starts_with('0'))
        || !value.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(ClientControlError::InvalidPrepare);
    }
    value
        .parse()
        .map_err(|_| ClientControlError::InvalidPrepare)
}

fn decode_lower_hex_hash(text: &str) -> Result<[u8; 32], ClientControlError> {
    if text.len() != 64 {
        return Err(ClientControlError::InvalidPrepare);
    }
    let mut hash = [0; 32];
    for (index, pair) in text.as_bytes().chunks_exact(2).enumerate() {
        hash[index] = (lower_hex_nibble(pair[0]).ok_or(ClientControlError::InvalidPrepare)? << 4)
            | lower_hex_nibble(pair[1]).ok_or(ClientControlError::InvalidPrepare)?;
    }
    Ok(hash)
}

fn decode_base64_bytes(text: &str) -> Option<Vec<u8>> {
    let input = text.as_bytes();
    if input.is_empty() || !input.len().is_multiple_of(4) {
        return None;
    }
    let mut output = Vec::new();
    output.try_reserve_exact(input.len() / 4 * 3).ok()?;
    for (index, chunk) in input.chunks_exact(4).enumerate() {
        let last = index + 1 == input.len() / 4;
        let a = base64_value(chunk[0])?;
        let b = base64_value(chunk[1])?;
        output.push((a << 2) | (b >> 4));
        match (chunk[2], chunk[3]) {
            (b'=', b'=') if last && b & 0x0f == 0 => (),
            (c, b'=') if last => {
                let c = base64_value(c)?;
                if c & 0x03 != 0 {
                    return None;
                }
                output.push((b << 4) | (c >> 2));
            }
            (c, d) if c != b'=' && d != b'=' => {
                let c = base64_value(c)?;
                let d = base64_value(d)?;
                output.push((b << 4) | (c >> 2));
                output.push((c << 6) | d);
            }
            _ => return None,
        }
    }
    Some(output)
}

fn parse_string_object(payload: &[u8]) -> Result<HashMap<String, String>, ClientControlError> {
    if payload.len() > CLIENT_MAX_CONTROL_BYTES {
        return Err(ClientControlError::InvalidJson);
    }
    let text = std::str::from_utf8(payload).map_err(|_| ClientControlError::InvalidJson)?;
    let mut parser = JsonStringObjectParser {
        text: text.as_bytes(),
        offset: 0,
    };
    parser.skip_whitespace();
    parser.expect(b'{')?;
    parser.skip_whitespace();
    let mut fields = HashMap::new();
    if parser.consume(b'}') {
        parser.skip_whitespace();
        return if parser.offset == parser.text.len() {
            Ok(fields)
        } else {
            Err(ClientControlError::InvalidJson)
        };
    }
    loop {
        parser.skip_whitespace();
        let key = parser.parse_string()?;
        parser.skip_whitespace();
        parser.expect(b':')?;
        parser.skip_whitespace();
        let value = parser.parse_string()?;
        if fields.insert(key, value).is_some() {
            return Err(ClientControlError::DuplicateKey);
        }
        parser.skip_whitespace();
        if parser.consume(b'}') {
            parser.skip_whitespace();
            return if parser.offset == parser.text.len() {
                Ok(fields)
            } else {
                Err(ClientControlError::InvalidJson)
            };
        }
        parser.expect(b',')?;
    }
}

pub(crate) struct JsonStringObjectParser<'a> {
    pub(crate) text: &'a [u8],
    pub(crate) offset: usize,
}

impl JsonStringObjectParser<'_> {
    pub(crate) fn skip_whitespace(&mut self) {
        while self
            .text
            .get(self.offset)
            .is_some_and(|byte| matches!(byte, b' ' | b'\n' | b'\r' | b'\t'))
        {
            self.offset += 1;
        }
    }

    pub(crate) fn consume(&mut self, expected: u8) -> bool {
        if self.text.get(self.offset) == Some(&expected) {
            self.offset += 1;
            true
        } else {
            false
        }
    }

    pub(crate) fn expect(&mut self, expected: u8) -> Result<(), ClientControlError> {
        self.consume(expected)
            .then_some(())
            .ok_or(ClientControlError::InvalidJson)
    }

    pub(crate) fn parse_u64_number(&mut self) -> Result<u64, ClientControlError> {
        let start = self.offset;
        while self.text.get(self.offset).is_some_and(u8::is_ascii_digit) {
            self.offset += 1;
        }
        let digits = self
            .text
            .get(start..self.offset)
            .ok_or(ClientControlError::InvalidJson)?;
        if digits.is_empty() || (digits.len() > 1 && digits[0] == b'0') {
            return Err(ClientControlError::InvalidJson);
        }
        std::str::from_utf8(digits)
            .map_err(|_| ClientControlError::InvalidJson)?
            .parse()
            .map_err(|_| ClientControlError::InvalidJson)
    }

    pub(crate) fn parse_string(&mut self) -> Result<String, ClientControlError> {
        self.expect(b'"')?;
        let mut output = Zeroizing::new(Vec::new());
        loop {
            let byte = *self
                .text
                .get(self.offset)
                .ok_or(ClientControlError::InvalidJson)?;
            self.offset += 1;
            match byte {
                b'"' => {
                    let bytes = std::mem::take(&mut *output);
                    return String::from_utf8(bytes).map_err(|error| {
                        let mut bytes = error.into_bytes();
                        bytes.zeroize();
                        ClientControlError::InvalidJson
                    });
                }
                b'\\' => {
                    let escaped = *self
                        .text
                        .get(self.offset)
                        .ok_or(ClientControlError::InvalidJson)?;
                    self.offset += 1;
                    match escaped {
                        b'"' | b'\\' | b'/' => output.push(escaped),
                        b'b' => output.push(0x08),
                        b'f' => output.push(0x0c),
                        b'n' => output.push(b'\n'),
                        b'r' => output.push(b'\r'),
                        b't' => output.push(b'\t'),
                        b'u' => {
                            let first = self.parse_hex_quad()?;
                            let scalar = if (0xd800..=0xdbff).contains(&first) {
                                self.expect(b'\\')?;
                                self.expect(b'u')?;
                                let second = self.parse_hex_quad()?;
                                if !(0xdc00..=0xdfff).contains(&second) {
                                    return Err(ClientControlError::InvalidJson);
                                }
                                0x1_0000
                                    + (((first as u32 - 0xd800) << 10) | (second as u32 - 0xdc00))
                            } else if (0xdc00..=0xdfff).contains(&first) {
                                return Err(ClientControlError::InvalidJson);
                            } else {
                                first as u32
                            };
                            let character =
                                char::from_u32(scalar).ok_or(ClientControlError::InvalidJson)?;
                            let mut encoded = [0; 4];
                            output
                                .extend_from_slice(character.encode_utf8(&mut encoded).as_bytes());
                        }
                        _ => return Err(ClientControlError::InvalidJson),
                    }
                }
                0x00..=0x1f => return Err(ClientControlError::InvalidJson),
                other => output.push(other),
            }
        }
    }

    pub(crate) fn parse_boolean(&mut self) -> Result<bool, ClientControlError> {
        if self.text[self.offset..].starts_with(b"true") {
            self.offset += 4;
            Ok(true)
        } else if self.text[self.offset..].starts_with(b"false") {
            self.offset += 5;
            Ok(false)
        } else {
            Err(ClientControlError::InvalidJson)
        }
    }

    fn parse_table_column_array(
        &mut self,
    ) -> Result<Vec<crate::storage_manager::TableColumnDefinition>, ClientControlError> {
        self.expect(b'[')?;
        self.skip_whitespace();
        let mut columns = Vec::new();
        if self.consume(b']') {
            return Ok(columns);
        }
        loop {
            if columns.len() >= crate::tuple::MAX_COLUMNS {
                return Err(ClientControlError::InvalidPrepare);
            }
            columns.push(self.parse_table_column_definition()?);
            self.skip_whitespace();
            if self.consume(b']') {
                break;
            }
            self.expect(b',')?;
        }
        Ok(columns)
    }

    fn parse_string_array(&mut self) -> Result<Vec<String>, ClientControlError> {
        self.expect(b'[')?;
        self.skip_whitespace();
        let mut values = Vec::new();
        if self.consume(b']') {
            return Ok(values);
        }
        loop {
            if values.len() >= crate::tuple::MAX_COLUMNS {
                return Err(ClientControlError::InvalidPrepare);
            }
            values.push(self.parse_string()?);
            self.skip_whitespace();
            if self.consume(b']') {
                break;
            }
            self.expect(b',')?;
        }
        Ok(values)
    }

    fn parse_string_array_array(&mut self) -> Result<Vec<Vec<String>>, ClientControlError> {
        self.expect(b'[')?;
        self.skip_whitespace();
        let mut values = Vec::new();
        if self.consume(b']') {
            return Ok(values);
        }
        loop {
            if values.len() >= crate::tuple::MAX_COLUMNS {
                return Err(ClientControlError::InvalidPrepare);
            }
            values.push(self.parse_string_array()?);
            self.skip_whitespace();
            if self.consume(b']') {
                break;
            }
            self.expect(b',')?;
        }
        Ok(values)
    }

    fn parse_id_array(&mut self) -> Result<Vec<[u8; 16]>, ClientControlError> {
        self.expect(b'[')?;
        self.skip_whitespace();
        let mut ids = Vec::new();
        if self.consume(b']') {
            return Ok(ids);
        }
        loop {
            if ids.len() >= crate::tuple::MAX_COLUMNS {
                return Err(ClientControlError::InvalidPrepare);
            }
            ids.push(decode_lower_hex_id(&self.parse_string()?)?);
            self.skip_whitespace();
            if self.consume(b']') {
                break;
            }
            self.expect(b',')?;
        }
        Ok(ids)
    }

    fn parse_id_array_allow_zero(&mut self) -> Result<Vec<[u8; 16]>, ClientControlError> {
        self.expect(b'[')?;
        self.skip_whitespace();
        let mut ids = Vec::new();
        if self.consume(b']') {
            return Ok(ids);
        }
        loop {
            if ids.len() >= crate::tuple::MAX_COLUMNS {
                return Err(ClientControlError::InvalidPrepare);
            }
            ids.push(decode_lower_hex_id_allow_zero(&self.parse_string()?)?);
            self.skip_whitespace();
            if self.consume(b']') {
                break;
            }
            self.expect(b',')?;
        }
        Ok(ids)
    }

    fn parse_policy_recipient_array(
        &mut self,
    ) -> Result<Vec<crate::authorization::Subject>, ClientControlError> {
        self.expect(b'[')?;
        self.skip_whitespace();
        let mut recipients = Vec::new();
        if self.consume(b']') {
            return Ok(recipients);
        }
        loop {
            if recipients.len() >= crate::authorization::MAX_POLICY_SUBJECTS {
                return Err(ClientControlError::InvalidPrepare);
            }
            let value = self.parse_string()?;
            let (kind, id) = value
                .split_once(':')
                .ok_or(ClientControlError::InvalidPrepare)?;
            let id = decode_lower_hex_id(id)?;
            recipients.push(match kind {
                "user" => crate::authorization::Subject::Principal(id),
                "role" => crate::authorization::Subject::Role(id),
                _ => return Err(ClientControlError::InvalidPrepare),
            });
            self.skip_whitespace();
            if self.consume(b']') {
                break;
            }
            self.expect(b',')?;
        }
        Ok(recipients)
    }

    fn parse_table_column_definition(
        &mut self,
    ) -> Result<crate::storage_manager::TableColumnDefinition, ClientControlError> {
        self.expect(b'{')?;
        let mut fields = std::collections::HashSet::new();
        let mut name = None;
        let mut scalar_type = None;
        let mut nullable = None;
        let mut generated_uuid_identity = None;
        let mut default = None;
        self.skip_whitespace();
        if self.consume(b'}') {
            return Err(ClientControlError::InvalidPrepare);
        }
        loop {
            self.skip_whitespace();
            let key = self.parse_string()?;
            if !fields.insert(key.clone()) {
                return Err(ClientControlError::DuplicateKey);
            }
            self.skip_whitespace();
            self.expect(b':')?;
            self.skip_whitespace();
            match key.as_str() {
                "name" => name = Some(self.parse_string()?),
                "type" => scalar_type = Some(decode_table_scalar_type(&self.parse_string()?)?),
                "nullable" => nullable = Some(self.parse_boolean()?),
                "generated_uuid_identity" => generated_uuid_identity = Some(self.parse_boolean()?),
                "default" => default = Some(self.parse_table_column_default()?),
                _ => return Err(ClientControlError::UnknownKey),
            }
            self.skip_whitespace();
            if self.consume(b'}') {
                break;
            }
            self.expect(b',')?;
        }
        let name = name.ok_or(ClientControlError::MissingKey)?;
        let scalar_type = scalar_type.ok_or(ClientControlError::MissingKey)?;
        let generated_uuid_identity =
            generated_uuid_identity.ok_or(ClientControlError::MissingKey)?;
        if generated_uuid_identity && default.is_some() {
            return Err(ClientControlError::InvalidPrepare);
        }
        Ok(crate::storage_manager::TableColumnDefinition {
            name,
            scalar_type,
            nullable: nullable.ok_or(ClientControlError::MissingKey)?,
            generated_uuid_identity,
            default: default
                .map(|(kind, value)| decode_table_column_default(&kind, value, scalar_type))
                .transpose()?,
        })
    }

    fn parse_table_column_default(
        &mut self,
    ) -> Result<(String, Option<String>), ClientControlError> {
        self.expect(b'{')?;
        let mut fields = std::collections::HashSet::new();
        let mut kind = None;
        let mut value = None;
        self.skip_whitespace();
        if self.consume(b'}') {
            return Err(ClientControlError::InvalidPrepare);
        }
        loop {
            self.skip_whitespace();
            let key = self.parse_string()?;
            if !fields.insert(key.clone()) {
                return Err(ClientControlError::DuplicateKey);
            }
            self.skip_whitespace();
            self.expect(b':')?;
            self.skip_whitespace();
            match key.as_str() {
                "kind" => kind = Some(self.parse_string()?),
                "value" => value = Some(self.parse_string()?),
                _ => return Err(ClientControlError::UnknownKey),
            }
            self.skip_whitespace();
            if self.consume(b'}') {
                break;
            }
            self.expect(b',')?;
        }
        let kind = kind.ok_or(ClientControlError::MissingKey)?;
        if (kind == "literal" && (fields.len() != 2 || value.is_none()))
            || (kind != "literal" && (fields.len() != 1 || value.is_some()))
        {
            return Err(ClientControlError::InvalidPrepare);
        }
        Ok((kind, value))
    }

    fn parse_hex_quad(&mut self) -> Result<u16, ClientControlError> {
        let end = self
            .offset
            .checked_add(4)
            .ok_or(ClientControlError::InvalidJson)?;
        let digits = self
            .text
            .get(self.offset..end)
            .ok_or(ClientControlError::InvalidJson)?;
        let mut value = 0u16;
        for digit in digits {
            value = value
                .checked_mul(16)
                .and_then(|value| value.checked_add(hex_nibble(*digit)? as u16))
                .ok_or(ClientControlError::InvalidJson)?;
        }
        self.offset = end;
        Ok(value)
    }
}

pub(crate) fn decode_table_scalar_type(
    text: &str,
) -> Result<crate::tuple::ScalarType, ClientControlError> {
    use crate::tuple::ScalarType;
    let invalid = || ClientControlError::InvalidPrepare;
    let parameter = |value: &str| -> Result<u32, ClientControlError> {
        if value.is_empty()
            || (value.len() > 1 && value.starts_with('0'))
            || !value.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(invalid());
        }
        value.parse().map_err(|_| invalid())
    };
    let scalar_type = match text {
        "boolean" => ScalarType::Boolean,
        "smallint" => ScalarType::SmallInt,
        "integer" => ScalarType::Integer,
        "bigint" => ScalarType::BigInt,
        "text" => ScalarType::Text,
        "bytea" => ScalarType::Bytea,
        "uuid" => ScalarType::Uuid,
        "date" => ScalarType::Date,
        "timestamptz" => ScalarType::TimestampTz,
        _ if text.starts_with("decimal(") && text.ends_with(')') => {
            let params = &text[8..text.len() - 1];
            let (precision, scale) = params.split_once(',').ok_or_else(invalid)?;
            if scale.contains(',') {
                return Err(invalid());
            }
            let precision = parameter(precision)?;
            let scale = parameter(scale)?;
            if !(1..=38).contains(&precision) || scale > precision {
                return Err(invalid());
            }
            ScalarType::Decimal {
                precision: precision as u8,
                scale: scale as u8,
            }
        }
        _ if text.starts_with("varchar(") && text.ends_with(')') => {
            let max_chars = parameter(&text[8..text.len() - 1])?;
            if max_chars == 0 {
                return Err(invalid());
            }
            ScalarType::Varchar { max_chars }
        }
        _ => return Err(invalid()),
    };
    Ok(scalar_type)
}

fn decode_table_column_default(
    kind: &str,
    value: Option<String>,
    scalar_type: crate::tuple::ScalarType,
) -> Result<crate::storage_manager::TableColumnDefault, ClientControlError> {
    use crate::storage_manager::TableColumnDefault;
    use crate::tuple::{ScalarType, ScalarValue};
    let invalid = || ClientControlError::InvalidPrepare;
    match (kind, value) {
        ("null", None) => Ok(TableColumnDefault::Literal(None)),
        ("current_principal_id", None) if scalar_type == ScalarType::Uuid => {
            Ok(TableColumnDefault::CurrentPrincipalId)
        }
        ("literal", Some(text)) => {
            let value = match scalar_type {
                ScalarType::Boolean => ScalarValue::Boolean(match text.as_str() {
                    "true" => true,
                    "false" => false,
                    _ => return Err(invalid()),
                }),
                ScalarType::SmallInt => ScalarValue::SmallInt(parse_default_integer(&text)?),
                ScalarType::Integer => ScalarValue::Integer(parse_default_integer(&text)?),
                ScalarType::BigInt => ScalarValue::BigInt(parse_default_integer(&text)?),
                ScalarType::Decimal { scale, .. } => {
                    ScalarValue::Decimal(parse_default_decimal(&text, scale)?)
                }
                ScalarType::Text => ScalarValue::Text(text),
                ScalarType::Varchar { .. } => ScalarValue::Varchar(text),
                ScalarType::Bytea => ScalarValue::Bytea(decode_default_hex(&text)?),
                ScalarType::Uuid => {
                    ScalarValue::Uuid(decode_lower_hex_id(&text).map_err(|_| invalid())?)
                }
                ScalarType::Date => ScalarValue::Date(parse_default_integer(&text)?),
                ScalarType::TimestampTz => ScalarValue::TimestampTz(parse_default_integer(&text)?),
            };
            Ok(TableColumnDefault::Literal(Some(value)))
        }
        _ => Err(invalid()),
    }
}

fn parse_default_integer<T>(text: &str) -> Result<T, ClientControlError>
where
    T: std::str::FromStr,
{
    let digits = text.strip_prefix('-').unwrap_or(text);
    if digits.is_empty()
        || (digits.len() > 1 && digits.starts_with('0'))
        || text == "-0"
        || !digits.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(ClientControlError::InvalidPrepare);
    }
    text.parse().map_err(|_| ClientControlError::InvalidPrepare)
}

fn parse_default_decimal(text: &str, scale: u8) -> Result<i128, ClientControlError> {
    let negative = text.starts_with('-');
    let unsigned = text.strip_prefix('-').unwrap_or(text);
    let (whole, fraction) = match unsigned.split_once('.') {
        Some(parts) => parts,
        None if scale == 0 => (unsigned, ""),
        None => return Err(ClientControlError::InvalidPrepare),
    };
    if whole.is_empty()
        || (whole.len() > 1 && whole.starts_with('0'))
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
        || fraction.len() != scale as usize
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
        || text == "-0"
        || (negative && whole == "0" && fraction.bytes().all(|byte| byte == b'0'))
    {
        return Err(ClientControlError::InvalidPrepare);
    }
    let digits = format!("{whole}{fraction}");
    let magnitude = digits
        .parse::<i128>()
        .map_err(|_| ClientControlError::InvalidPrepare)?;
    if negative {
        magnitude
            .checked_neg()
            .ok_or(ClientControlError::InvalidPrepare)
    } else {
        Ok(magnitude)
    }
}

fn decode_default_hex(text: &str) -> Result<Vec<u8>, ClientControlError> {
    if !text.len().is_multiple_of(2)
        || !text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(ClientControlError::InvalidPrepare);
    }
    text.as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = hex_nibble(pair[0]).ok_or(ClientControlError::InvalidPrepare)?;
            let low = hex_nibble(pair[1]).ok_or(ClientControlError::InvalidPrepare)?;
            Ok((high << 4) | low)
        })
        .collect()
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn take_required(
    fields: &mut HashMap<String, String>,
    key: &str,
) -> Result<String, ClientControlError> {
    fields.remove(key).ok_or(ClientControlError::MissingKey)
}

fn reject_unknown_fields(fields: HashMap<String, String>) -> Result<(), ClientControlError> {
    if fields.is_empty() {
        Ok(())
    } else {
        Err(ClientControlError::UnknownKey)
    }
}

fn reject_unknown_scalars(fields: HashMap<String, JsonScalar>) -> Result<(), ClientControlError> {
    if fields.is_empty() {
        Ok(())
    } else {
        Err(ClientControlError::UnknownKey)
    }
}

fn decode_lower_hex_id(text: &str) -> Result<[u8; 16], ClientControlError> {
    let id = decode_lower_hex_id_allow_zero(text)?;
    if id == [0; 16] {
        return Err(ClientControlError::InvalidTokenId);
    }
    Ok(id)
}

fn decode_lower_hex_id_allow_zero(text: &str) -> Result<[u8; 16], ClientControlError> {
    if text.len() != 32 {
        return Err(ClientControlError::InvalidTokenId);
    }
    let mut id = [0; 16];
    for (index, pair) in text.as_bytes().chunks_exact(2).enumerate() {
        let high = lower_hex_nibble(pair[0]).ok_or(ClientControlError::InvalidTokenId)?;
        let low = lower_hex_nibble(pair[1]).ok_or(ClientControlError::InvalidTokenId)?;
        id[index] = (high << 4) | low;
    }
    Ok(id)
}

fn decode_lower_hex_bytes(text: &str, max_bytes: usize) -> Result<Vec<u8>, ClientControlError> {
    if text.is_empty() || !text.len().is_multiple_of(2) || text.len() / 2 > max_bytes {
        return Err(ClientControlError::InvalidPrepare);
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(text.len() / 2)
        .map_err(|_| ClientControlError::InvalidPrepare)?;
    for pair in text.as_bytes().chunks_exact(2) {
        let high = lower_hex_nibble(pair[0]).ok_or(ClientControlError::InvalidPrepare)?;
        let low = lower_hex_nibble(pair[1]).ok_or(ClientControlError::InvalidPrepare)?;
        bytes.push((high << 4) | low);
    }
    Ok(bytes)
}

fn decode_metadata_pagination(
    arguments: &mut HashMap<String, String>,
) -> Result<Option<MetadataPagination>, ClientControlError> {
    let Some(limit) = arguments.remove("limit") else {
        return if arguments.contains_key("cursor") {
            Err(ClientControlError::InvalidMetadataCursor)
        } else {
            Ok(None)
        };
    };
    let page_limit = limit
        .parse::<usize>()
        .ok()
        .filter(|value| {
            (1..=CLIENT_METADATA_MAX_PAGE).contains(value) && value.to_string() == limit
        })
        .ok_or(ClientControlError::InvalidMetadataPageLimit)?;
    let cursor = arguments
        .remove("cursor")
        .map(|encoded| {
            let cursor = decode_lower_hex_id(&encoded)
                .map_err(|_| ClientControlError::InvalidMetadataCursor)?;
            if cursor[0] != CLIENT_METADATA_CURSOR_KIND {
                return Err(ClientControlError::InvalidMetadataCursor);
            }
            Ok(cursor)
        })
        .transpose()?;
    Ok(Some(MetadataPagination { page_limit, cursor }))
}

fn lower_hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

fn decode_canonical_base64(text: &str) -> Result<Zeroizing<Vec<u8>>, ClientControlError> {
    let input = text.as_bytes();
    if input.is_empty() || !input.len().is_multiple_of(4) {
        return Err(ClientControlError::InvalidTokenSecret);
    }
    let mut output = Zeroizing::new(Vec::new());
    output
        .try_reserve_exact(input.len() / 4 * 3)
        .map_err(|_| ClientControlError::InvalidTokenSecret)?;
    for (index, chunk) in input.chunks_exact(4).enumerate() {
        let last = index + 1 == input.len() / 4;
        let a = base64_value(chunk[0]).ok_or(ClientControlError::InvalidTokenSecret)?;
        let b = base64_value(chunk[1]).ok_or(ClientControlError::InvalidTokenSecret)?;
        output.push((a << 2) | (b >> 4));
        match (chunk[2], chunk[3]) {
            (b'=', b'=') if last && b & 0x0f == 0 => (),
            (c, b'=') if last => {
                let c = base64_value(c).ok_or(ClientControlError::InvalidTokenSecret)?;
                if c & 0x03 != 0 {
                    return Err(ClientControlError::InvalidTokenSecret);
                }
                output.push((b << 4) | (c >> 2));
            }
            (c, d) if c != b'=' && d != b'=' => {
                let c = base64_value(c).ok_or(ClientControlError::InvalidTokenSecret)?;
                let d = base64_value(d).ok_or(ClientControlError::InvalidTokenSecret)?;
                output.push((b << 4) | (c >> 2));
                output.push((c << 6) | d);
            }
            _ => return Err(ClientControlError::InvalidTokenSecret),
        }
    }
    if output.len() != TOKEN_SECRET_BYTES {
        return Err(ClientControlError::InvalidTokenSecret);
    }
    Ok(output)
}

fn base64_value(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

fn encode_base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let a = chunk[0];
        let b = *chunk.get(1).unwrap_or(&0);
        let c = *chunk.get(2).unwrap_or(&0);
        output.push(ALPHABET[(a >> 2) as usize] as char);
        output.push(ALPHABET[(((a & 0x03) << 4) | (b >> 4)) as usize] as char);
        if chunk.len() > 1 {
            output.push(ALPHABET[(((b & 0x0f) << 2) | (c >> 6)) as usize] as char);
        } else {
            output.push('=');
        }
        if chunk.len() > 2 {
            output.push(ALPHABET[(c & 0x3f) as usize] as char);
        } else {
            output.push('=');
        }
    }
    output
}

impl std::fmt::Display for ClientSessionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "client session error: {self:?}")
    }
}

impl std::error::Error for ClientSessionError {}

/// Per-connection authentication and mode state used by the gateway dispatcher.
/// It keeps the bearer secret only for this connection lifetime, zeroizes it
/// on drop, and gives staging authority only after storage AUTH.
pub struct ClientConnectionSession {
    protocol_major: u16,
    repository_sdk_library: bool,
    hello: Option<ClientMode>,
    authenticated: Option<AuthenticatedQuerySession>,
    token_secret: Option<[u8; TOKEN_SECRET_BYTES]>,
    request_ids: ClientRequestIds,
    pending_admin_operation: Option<PendingAdminOperation>,
    metadata_page_snapshot: Option<ActiveMetadataPageSnapshot>,
    sdk_library_page_snapshot: Option<ActiveSdkLibraryPageSnapshot>,
}

impl Default for ClientConnectionSession {
    fn default() -> Self {
        Self {
            protocol_major: 1,
            repository_sdk_library: false,
            hello: None,
            authenticated: None,
            token_secret: None,
            request_ids: ClientRequestIds::default(),
            pending_admin_operation: None,
            metadata_page_snapshot: None,
            sdk_library_page_snapshot: None,
        }
    }
}

impl ClientConnectionSession {
    /// Select the negotiated client framing version before processing HELLO.
    pub fn set_protocol_major(&mut self, major: u16) -> Result<(), ClientSessionError> {
        if !matches!(major, 1 | 2) || self.hello.is_some() {
            return Err(ClientSessionError::UnsupportedRequest);
        }
        self.protocol_major = major;
        Ok(())
    }

    pub(crate) fn protocol_major(&self) -> u16 {
        self.protocol_major
    }

    /// Validate a transaction-bound START request and return zeroizing
    /// controller credentials for trusted service orchestration.
    pub(crate) fn validate_interactive_start_request(
        &mut self,
        storage: &StorageManager<'_>,
        request: &ClientFrame,
        expected_binding: crate::supervisor::InteractivePackageBinding,
    ) -> Result<ValidatedInteractiveStartCredentials, ClientSessionError> {
        if self.protocol_major != 2 || request.kind != 7 || request.response_sequence != 0 {
            return Err(ClientSessionError::UnsupportedRequest);
        }
        self.request_ids
            .accept(request)
            .map_err(|_| ClientSessionError::UnsupportedRequest)?;
        if !self.permits_request_kind(request.kind) {
            return Err(if self.authenticated.is_none() {
                ClientSessionError::AuthenticationRequired
            } else {
                ClientSessionError::WrongMode
            });
        }
        self.revalidate_storage(storage)?;
        let mut start = decode_client_start_for_protocol(&request.payload, self.protocol_major)
            .map_err(ClientSessionError::InvalidControlPayload)?;
        if start.transaction_id != Some(expected_binding.transaction_id)
            || start.package_id != Some(expected_binding.package_id)
        {
            return Err(ClientSessionError::InvalidControlPayload(
                ClientControlError::InvalidPrepare,
            ));
        }
        let token_id = self
            .authenticated
            .as_ref()
            .ok_or(ClientSessionError::AuthenticationRequired)?
            .token_id();
        let token_secret = self
            .token_secret
            .as_ref()
            .ok_or(ClientSessionError::AuthenticationRequired)?;
        Ok(ValidatedInteractiveStartCredentials {
            controller_token_id: token_id,
            controller_token_secret: Zeroizing::new(*token_secret),
            actor: start.actor.take(),
        })
    }

    /// Dispatch one authenticated v2 transaction-control request. The caller
    /// must have negotiated v2 framing before invoking this method.
    pub fn handle_v2_transaction_request(
        &mut self,
        storage: &StorageManager<'_>,
        request: ClientFrame,
        outcome_retention_deadline_us: i64,
    ) -> Result<ClientFrame, ClientSessionError> {
        if request.response_sequence != 0
            || !(CLIENT_V2_TX_BEGIN_KIND..=CLIENT_V2_PACKAGE_STATUS_KIND).contains(&request.kind)
        {
            return Err(ClientSessionError::UnsupportedRequest);
        }
        self.request_ids
            .accept_v2(&request)
            .map_err(|_| ClientSessionError::UnsupportedRequest)?;
        if self.hello != Some(ClientMode::Query) {
            return Err(if self.authenticated.is_none() {
                ClientSessionError::AuthenticationRequired
            } else {
                ClientSessionError::WrongMode
            });
        }
        self.revalidate_storage(storage)?;
        let controller = self
            .authenticated
            .as_ref()
            .ok_or(ClientSessionError::AuthenticationRequired)?;
        let token_id = controller.token_id();
        let token_secret = self
            .token_secret
            .as_ref()
            .ok_or(ClientSessionError::AuthenticationRequired)?;
        let decoded = ClientV2Request::decode(request.kind, &request.payload)
            .map_err(ClientSessionError::InvalidControlPayload)?;
        let response = match decoded {
            ClientV2Request::TxBegin { begin_request_id } => {
                let begun = storage
                    .begin_interactive_transaction_with_request_id(
                        token_id,
                        token_secret,
                        outcome_retention_deadline_us,
                        begin_request_id,
                    )
                    .map_err(ClientSessionError::InteractiveTransaction)?;
                ClientV2Response::TransactionBegun {
                    transaction_id: begun.transaction_id,
                    state: client_v2_transaction_state(begun.state),
                    idle_timeout_ms: begun.idle_timeout_ms,
                    lifetime_ms: begun.lifetime_ms,
                    max_package_bytes: begun.max_package_bytes,
                    max_packages: begun.max_packages,
                }
            }
            ClientV2Request::TxAttach { transaction_id } => {
                let state = storage
                    .attach_interactive_transaction(token_id, token_secret, transaction_id)
                    .map_err(ClientSessionError::InteractiveTransaction)?;
                ClientV2Response::Transaction {
                    transaction_id,
                    state: client_v2_transaction_state(state),
                }
            }
            ClientV2Request::TxCommit { transaction_id } => {
                let result = storage
                    .commit_interactive_transaction(token_id, token_secret, transaction_id)
                    .map_err(ClientSessionError::InteractiveTransaction)?;
                let _commit_sequence = result;
                ClientV2Response::Transaction {
                    transaction_id,
                    state: crate::client_protocol::ClientV2TransactionState::Committed,
                }
            }
            ClientV2Request::TxRollback { transaction_id } => {
                storage
                    .rollback_interactive_transaction(token_id, token_secret, transaction_id)
                    .map_err(ClientSessionError::InteractiveTransaction)?;
                ClientV2Response::Transaction {
                    transaction_id,
                    state: crate::client_protocol::ClientV2TransactionState::Aborted,
                }
            }
            ClientV2Request::TxStatus { transaction_id } => {
                let state = storage
                    .interactive_transaction_status(token_id, token_secret, transaction_id)
                    .map_err(ClientSessionError::InteractiveTransaction)?;
                ClientV2Response::Transaction {
                    transaction_id,
                    state: client_v2_transaction_state(state),
                }
            }
            ClientV2Request::PackageStatus {
                transaction_id,
                package_id,
            } => {
                let status = storage
                    .interactive_package_status(token_id, token_secret, transaction_id, package_id)
                    .map_err(ClientSessionError::InteractiveTransaction)?;
                ClientV2Response::Package {
                    transaction_id,
                    package_id,
                    state: match status.state {
                        crate::storage_manager::InteractivePackageState::Running => {
                            ClientV2PackageState::Running
                        }
                        crate::storage_manager::InteractivePackageState::Completed => {
                            ClientV2PackageState::Completed
                        }
                        crate::storage_manager::InteractivePackageState::Failed => {
                            ClientV2PackageState::Failed
                        }
                        crate::storage_manager::InteractivePackageState::Unknown => {
                            ClientV2PackageState::Unknown
                        }
                    },
                    result_lost: status.result_lost,
                }
            }
        };
        self.json_response(request.request_id, response.encode())
    }

    /// Process the protocol handshake and PING requests. The TCP adapter must
    /// decode bounded frames first and close the connection on framing errors.
    pub fn handle_handshake_request(
        &mut self,
        storage: &StorageManager<'_>,
        request: ClientFrame,
    ) -> Result<ClientFrame, ClientSessionError> {
        if request.response_sequence != 0 {
            return Err(ClientSessionError::UnsupportedRequest);
        }
        self.request_ids
            .accept(&request)
            .map_err(|_| ClientSessionError::UnsupportedRequest)?;
        if !self.permits_request_kind(request.kind) {
            return Err(if self.hello.is_none() || request.kind == 1 {
                ClientSessionError::UnsupportedRequest
            } else if self.authenticated.is_none() {
                ClientSessionError::AuthenticationRequired
            } else {
                ClientSessionError::WrongMode
            });
        }
        let payload = match request.kind {
            1 => {
                let hello = decode_client_hello(&request.payload)
                    .map_err(ClientSessionError::InvalidControlPayload)?;
                let mode_text = match hello.mode {
                    ClientMode::Query => "query",
                    ClientMode::Control => "control",
                };
                self.accept_hello_with_sdk_library_mode(
                    hello.mode,
                    &hello.client_name,
                    hello.repository_sdk_library,
                )?;
                if hello.repository_sdk_library {
                    format!(
                        "{{\"major\":{},\"minor\":0,\"mode\":\"{mode_text}\",\"sdk_library_mode\":\"repository\",\"limits\":{{\"payload_bytes\":{CLIENT_MAX_PAYLOAD_BYTES},\"control_bytes\":{CLIENT_MAX_CONTROL_BYTES},\"chunk_bytes\":{CLIENT_MAX_CHUNK_BYTES}}},\"transport\":\"plaintext_tcp\"}}",
                        self.protocol_major
                    )
                    .into_bytes()
                } else {
                    format!(
                        "{{\"major\":{},\"minor\":0,\"mode\":\"{mode_text}\",\"limits\":{{\"payload_bytes\":{CLIENT_MAX_PAYLOAD_BYTES},\"control_bytes\":{CLIENT_MAX_CONTROL_BYTES},\"chunk_bytes\":{CLIENT_MAX_CHUNK_BYTES}}},\"transport\":\"plaintext_tcp\"}}",
                        self.protocol_major
                    )
                    .into_bytes()
                }
            }
            2 => {
                let credentials = decode_client_credentials(&request.payload)
                    .map_err(ClientSessionError::InvalidControlPayload)?;
                let session =
                    self.authenticate(storage, credentials.token_id, &credentials.token_secret)?;
                let session_id = encode_lower_hex_id(session.connection_id());
                format!("{{\"session_id\":\"{session_id}\",\"authenticated\":true}}").into_bytes()
            }
            11 if request.payload == b"{}" => b"{\"alive\":true}".to_vec(),
            _ => return Err(ClientSessionError::UnsupportedRequest),
        };
        Ok(ClientFrame {
            kind: 100,
            request_id: request.request_id,
            response_sequence: 0,
            payload,
        })
    }

    /// Route one artifact/parameter/SQL-library chunk using
    /// this connection's storage-authenticated owner, and acknowledge only
    /// the contiguous next offset.
    pub fn handle_upload_request(
        &mut self,
        storage: &StorageManager<'_>,
        staging: &crate::supervisor::StagingStore,
        request: ClientFrame,
    ) -> Result<ClientFrame, ClientSessionError> {
        if request.response_sequence != 0
            || !matches!(request.kind, 4 | 5 | 12 | CLIENT_SDK_LIBRARY_CHUNK_KIND)
        {
            return Err(ClientSessionError::UnsupportedRequest);
        }
        self.request_ids
            .accept(&request)
            .map_err(|_| ClientSessionError::UnsupportedRequest)?;
        if !self.permits_request_kind(request.kind) {
            return Err(if self.authenticated.is_none() {
                ClientSessionError::AuthenticationRequired
            } else {
                ClientSessionError::WrongMode
            });
        }
        self.revalidate_storage(storage)?;
        let chunk = decode_client_chunk(&request.payload)
            .map_err(ClientSessionError::InvalidControlPayload)?;
        if request.kind == CLIENT_SDK_LIBRARY_CHUNK_KIND {
            let actor_id = self
                .authenticated
                .as_ref()
                .ok_or(ClientSessionError::AuthenticationRequired)?
                .principal_id();
            if !storage
                .is_database_admin(actor_id)
                .map_err(ClientSessionError::TokenInventory)?
            {
                return Err(ClientSessionError::SdkLibraryRepositoryUnauthorized);
            }
            let repository = staging
                .sdk_repository()
                .ok_or(ClientSessionError::SdkLibraryRepositoryUnavailable)?;
            let next_offset = repository
                .upload_install_chunk(chunk.invocation_id, actor_id, chunk.offset, &chunk.data)
                .map_err(ClientSessionError::SdkLibraryRepository)?;
            return Ok(ClientFrame {
                kind: 100,
                request_id: request.request_id,
                response_sequence: 0,
                payload: format!("{{\"next_offset\":\"{next_offset}\"}}").into_bytes(),
            });
        }
        let kind = match request.kind {
            4 => crate::supervisor::ChunkKind::Artifact,
            5 => crate::supervisor::ChunkKind::Parameters,
            _ => unreachable!("chunk kind was checked"),
        };
        let next_offset = staging
            .upload_chunk(
                self.staging_owner()?,
                chunk.invocation_id,
                kind,
                chunk.offset,
                &chunk.data,
            )
            .map_err(ClientSessionError::Staging)?;
        Ok(ClientFrame {
            kind: 100,
            request_id: request.request_id,
            response_sequence: 0,
            payload: format!("{{\"next_offset\":\"{next_offset}\"}}").into_bytes(),
        })
    }

    /// Reserve bounded supervisor staging from a fully validated PREPARE
    /// request. No storage snapshot or executor is created by this operation.
    pub fn handle_prepare_request(
        &mut self,
        storage: &StorageManager<'_>,
        staging: &crate::supervisor::StagingStore,
        request: ClientFrame,
    ) -> Result<ClientFrame, ClientSessionError> {
        if request.response_sequence != 0 || request.kind != 3 {
            return Err(ClientSessionError::UnsupportedRequest);
        }
        self.request_ids
            .accept(&request)
            .map_err(|_| ClientSessionError::UnsupportedRequest)?;
        if !self.permits_request_kind(request.kind) {
            return Err(if self.authenticated.is_none() {
                ClientSessionError::AuthenticationRequired
            } else {
                ClientSessionError::WrongMode
            });
        }
        self.revalidate_storage(storage)?;
        let prepare = decode_client_prepare_for_protocol(&request.payload, self.protocol_major)
            .map_err(ClientSessionError::InvalidControlPayload)?;
        if prepare.repository_sdk_library != self.repository_sdk_library {
            return Err(ClientSessionError::InvalidControlPayload(
                ClientControlError::InvalidPrepare,
            ));
        }
        let reservation = staging
            .prepare(
                self.staging_owner()?,
                crate::supervisor::PrepareSubmission {
                    manifest_version: prepare.manifest_version,
                    manifest: prepare.manifest,
                    artifact_bytes: prepare.artifact_bytes,
                    artifact_sha256: prepare.artifact_sha256,
                    scope_version: prepare.scope_version,
                    requested_scope: prepare.requested_scope,
                    parameter_bytes: prepare.parameter_bytes,
                    parameter_sha256: prepare.parameter_sha256,
                    deadline_ms: prepare.deadline_ms,
                    workload_class: prepare.workload_class,
                },
            )
            .map_err(ClientSessionError::Staging)?;
        if let (Some(transaction_id), Some(package_id)) =
            (prepare.transaction_id, prepare.package_id)
        {
            staging
                .bind_interactive_package(
                    self.staging_owner()?,
                    reservation.invocation_id,
                    transaction_id,
                    package_id,
                )
                .map_err(ClientSessionError::Staging)?;
        }
        let invocation_id = encode_lower_hex_id(reservation.invocation_id);
        let payload = format!(
            "{{\"invocation_id\":\"{invocation_id}\",\"artifact_chunk_bytes\":{CLIENT_MAX_CHUNK_BYTES},\"parameter_chunk_bytes\":{CLIENT_MAX_CHUNK_BYTES}}}"
        );
        Ok(ClientFrame {
            kind: 100,
            request_id: request.request_id,
            response_sequence: 0,
            payload: payload.into_bytes(),
        })
    }

    /// Verify uploaded lengths, hashes, and versioned admission metadata,
    /// then make the staged submission immutable.
    pub fn handle_seal_request(
        &mut self,
        storage: &StorageManager<'_>,
        staging: &crate::supervisor::StagingStore,
        request: ClientFrame,
    ) -> Result<ClientFrame, ClientSessionError> {
        if request.response_sequence != 0 || request.kind != 6 {
            return Err(ClientSessionError::UnsupportedRequest);
        }
        self.request_ids
            .accept(&request)
            .map_err(|_| ClientSessionError::UnsupportedRequest)?;
        if !self.permits_request_kind(request.kind) {
            return Err(if self.authenticated.is_none() {
                ClientSessionError::AuthenticationRequired
            } else {
                ClientSessionError::WrongMode
            });
        }
        self.revalidate_storage(storage)?;
        let invocation_id = decode_client_seal(&request.payload)
            .map_err(ClientSessionError::InvalidControlPayload)?;
        staging
            .seal(self.staging_owner()?, invocation_id)
            .map_err(ClientSessionError::Staging)?;
        Ok(ClientFrame {
            kind: 100,
            request_id: request.request_id,
            response_sequence: 0,
            payload: b"{\"state\":\"prepared\"}".to_vec(),
        })
    }

    /// Accept one sealed invocation and return both the sequence-zero wire
    /// acknowledgement and the storage admission that the supervisor owns.
    pub fn handle_start_request<'db>(
        &mut self,
        storage: &StorageManager<'db>,
        staging: &crate::supervisor::StagingStore,
        request: ClientFrame,
        retention_deadline_us: i64,
        max_rows: usize,
        max_bytes: usize,
    ) -> Result<
        (
            ClientFrame,
            crate::storage_manager::SupervisorInvocation<'db>,
        ),
        ClientStartError,
    > {
        if request.response_sequence != 0 || request.kind != 7 {
            return Err(ClientStartError::Session(
                ClientSessionError::UnsupportedRequest,
            ));
        }
        self.request_ids
            .accept(&request)
            .map_err(|_| ClientStartError::Session(ClientSessionError::UnsupportedRequest))?;
        if !self.permits_request_kind(request.kind) {
            return Err(ClientStartError::Session(if self.authenticated.is_none() {
                ClientSessionError::AuthenticationRequired
            } else {
                ClientSessionError::WrongMode
            }));
        }
        let invocation_id = decode_client_start_for_protocol(&request.payload, self.protocol_major)
            .map(|start| start.invocation_id)
            .map_err(|error| {
                ClientStartError::Session(ClientSessionError::InvalidControlPayload(error))
            })?;
        let invocation = self.start_submission(
            storage,
            staging,
            invocation_id,
            retention_deadline_us,
            max_rows,
            max_bytes,
        )?;
        let response = ClientFrame {
            kind: 100,
            request_id: request.request_id,
            response_sequence: 0,
            payload: b"{\"state\":\"accepted\"}".to_vec(),
        };
        Ok((response, invocation))
    }

    /// Accept START through the configured executor launch path. The host
    /// profile and cgroup are established before storage admission; the
    /// executor is spawned only after its artifact and confinement command
    /// have been prepared.
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    pub fn handle_start_request_with_executor<'db>(
        &mut self,
        storage: &StorageManager<'db>,
        staging: &crate::supervisor::StagingStore,
        request: ClientFrame,
        admission: crate::supervisor_ipc::GatewayAdmissionConfig,
        executor: &crate::supervisor_ipc::GatewayExecutorConfig,
    ) -> Result<
        (
            ClientFrame,
            crate::storage_manager::SupervisorInvocation<'db>,
            crate::executor_launcher::RunningExecutor,
        ),
        ClientStartError,
    > {
        if request.response_sequence != 0 || request.kind != 7 {
            return Err(ClientStartError::Session(
                ClientSessionError::UnsupportedRequest,
            ));
        }
        self.request_ids
            .accept(&request)
            .map_err(|_| ClientStartError::Session(ClientSessionError::UnsupportedRequest))?;
        if !self.permits_request_kind(request.kind) {
            return Err(ClientStartError::Session(if self.authenticated.is_none() {
                ClientSessionError::AuthenticationRequired
            } else {
                ClientSessionError::WrongMode
            }));
        }
        let invocation_id = decode_client_start_for_protocol(&request.payload, self.protocol_major)
            .map(|start| start.invocation_id)
            .map_err(|error| {
                ClientStartError::Session(ClientSessionError::InvalidControlPayload(error))
            })?;
        let (invocation, worker) = self.start_submission_with_executor(
            storage,
            staging,
            invocation_id,
            admission,
            executor,
        )?;
        let response = ClientFrame {
            kind: 100,
            request_id: request.request_id,
            response_sequence: 0,
            payload: b"{\"state\":\"accepted\"}".to_vec(),
        };
        Ok((response, invocation, worker))
    }

    /// Report the caller-owned staging state or current storage outcome after
    /// revalidating the connection's credential and role closure.
    pub fn handle_status_request(
        &mut self,
        storage: &StorageManager<'_>,
        staging: &crate::supervisor::StagingStore,
        request: ClientFrame,
    ) -> Result<ClientFrame, ClientSessionError> {
        let invocation_id = self.validate_control_request(storage, &request, 8, false)?;
        let session = self
            .authenticated
            .as_ref()
            .ok_or(ClientSessionError::AuthenticationRequired)?;
        let now_us = current_unix_micros().ok_or(ClientSessionError::UnsupportedRequest)?;
        if self.hello == Some(ClientMode::Control) {
            let snapshot = storage
                .get_invocation_status(invocation_id, session.principal_id(), now_us)
                .map_err(ClientSessionError::Status)?;
            let snapshot = match snapshot {
                Some(snapshot) => Some(snapshot),
                None => storage
                    .get_administrative_invocation_status(
                        session.principal_id(),
                        invocation_id,
                        now_us,
                    )
                    .map_err(ClientSessionError::AdministrativeStatus)?,
            };
            let payload = match snapshot.map(|snapshot| snapshot.status) {
                Some(crate::storage_manager::InvocationStatus::Active { .. }) => {
                    b"{\"state\":\"active\"}".to_vec()
                }
                Some(crate::storage_manager::InvocationStatus::Committing { .. }) => {
                    b"{\"state\":\"committing\"}".to_vec()
                }
                Some(crate::storage_manager::InvocationStatus::Terminal(outcome)) => {
                    match outcome.status {
                        crate::outcome::OutcomeStatus::Committed => {
                            b"{\"state\":\"committed\"}".to_vec()
                        }
                        crate::outcome::OutcomeStatus::Aborted => {
                            b"{\"state\":\"aborted\"}".to_vec()
                        }
                    }
                }
                None => b"{\"state\":\"unknown\"}".to_vec(),
            };
            return self.json_response(request.request_id, payload);
        }
        match staging.status(session.staging_owner(), invocation_id) {
            Ok(
                crate::supervisor::SubmissionState::Uploading
                | crate::supervisor::SubmissionState::Sealing
                | crate::supervisor::SubmissionState::Prepared
                | crate::supervisor::SubmissionState::Starting,
            ) => self.json_response(request.request_id, b"{\"state\":\"prepared\"}".to_vec()),
            Ok(crate::supervisor::SubmissionState::Started)
            | Err(crate::supervisor::StagingError::Unavailable) => {
                let snapshot = storage
                    .get_invocation_status(invocation_id, session.principal_id(), now_us)
                    .map_err(ClientSessionError::Status)?;
                let payload = match snapshot.map(|snapshot| snapshot.status) {
                    Some(crate::storage_manager::InvocationStatus::Active { .. }) => {
                        b"{\"state\":\"active\"}".to_vec()
                    }
                    Some(crate::storage_manager::InvocationStatus::Committing { .. }) => {
                        b"{\"state\":\"committing\"}".to_vec()
                    }
                    Some(crate::storage_manager::InvocationStatus::Terminal(outcome)) => {
                        match outcome.status {
                            crate::outcome::OutcomeStatus::Committed => {
                                b"{\"state\":\"committed\"}".to_vec()
                            }
                            crate::outcome::OutcomeStatus::Aborted => {
                                b"{\"state\":\"aborted\"}".to_vec()
                            }
                        }
                    }
                    None => b"{\"state\":\"unknown\"}".to_vec(),
                };
                self.json_response(request.request_id, payload)
            }
            Err(error) => Err(ClientSessionError::Staging(error)),
        }
    }

    /// Execute supported ADMIN reads and connection-owned prepare/start operations.
    pub fn handle_admin_request(
        &mut self,
        storage: &StorageManager<'_>,
        request: ClientFrame,
        retention_deadline_us: i64,
    ) -> Result<ClientFrame, ClientSessionError> {
        self.handle_admin_request_with_staging(storage, None, request, retention_deadline_us)
    }

    /// Execute ADMIN requests with the supervisor-local queue projection
    /// available to version-three runtime-list responses.
    pub fn handle_admin_request_with_staging(
        &mut self,
        storage: &StorageManager<'_>,
        staging: Option<&crate::supervisor::StagingStore>,
        request: ClientFrame,
        retention_deadline_us: i64,
    ) -> Result<ClientFrame, ClientSessionError> {
        if request.response_sequence != 0 || request.kind != 10 {
            return Err(ClientSessionError::UnsupportedRequest);
        }
        self.request_ids
            .accept(&request)
            .map_err(|_| ClientSessionError::UnsupportedRequest)?;
        if !self.permits_request_kind(10) {
            return Err(if self.authenticated.is_none() {
                ClientSessionError::AuthenticationRequired
            } else {
                ClientSessionError::WrongMode
            });
        }
        self.revalidate_storage(storage)?;
        let command = decode_client_admin_command(&request.payload)
            .map_err(ClientSessionError::InvalidControlPayload)?;
        let actor_id = self
            .authenticated
            .as_ref()
            .ok_or(ClientSessionError::AuthenticationRequired)?
            .principal_id();
        if matches!(
            &command,
            ClientAdminCommand::ListSchemas
                | ClientAdminCommand::ListRelations { .. }
                | ClientAdminCommand::ListColumns { .. }
        ) {
            self.metadata_page_snapshot = None;
        }
        match command {
            ClientAdminCommand::ResolveIdentityName {
                name,
                kind,
                policy_scope,
            } => {
                let token_id = self
                    .authenticated
                    .as_ref()
                    .ok_or(ClientSessionError::AuthenticationRequired)?
                    .token_id();
                let token_secret = self
                    .token_secret
                    .as_ref()
                    .ok_or(ClientSessionError::AuthenticationRequired)?;
                let identity_id = match policy_scope {
                    Some((schema_id, table_id)) => storage.resolve_token_policy_identity_name(
                        token_id,
                        token_secret,
                        schema_id,
                        table_id,
                        &name,
                        kind,
                    ),
                    None => storage.resolve_token_administrative_identity_name(
                        token_id,
                        token_secret,
                        &name,
                        kind,
                    ),
                }
                .map_err(ClientSessionError::Storage)?;
                let payload = match identity_id {
                    Some(identity_id) => format!(
                        "{{\"identity_id\":\"{}\"}}",
                        encode_lower_hex_id(identity_id)
                    ),
                    None => "{\"identity_id\":null}".to_owned(),
                };
                self.json_response(request.request_id, payload.into_bytes())
            }
            ClientAdminCommand::ResolveGrantIdentityName {
                schema_name,
                table_name,
                name,
                kind,
            } => {
                let token_id = self
                    .authenticated
                    .as_ref()
                    .ok_or(ClientSessionError::AuthenticationRequired)?
                    .token_id();
                let token_secret = self
                    .token_secret
                    .as_ref()
                    .ok_or(ClientSessionError::AuthenticationRequired)?;
                let resolved = storage
                    .resolve_token_grant_target_identity_name(
                        token_id,
                        token_secret,
                        &schema_name,
                        table_name.as_deref(),
                        &name,
                        kind,
                    )
                    .map_err(ClientSessionError::Storage)?;
                let payload = match resolved {
                    Some(resolution) => format!(
                        "{{\"schema_id\":\"{}\",\"object_id\":\"{}\",\"identity_id\":\"{}\"}}",
                        encode_lower_hex_id(resolution.schema_id),
                        encode_lower_hex_id(resolution.object_id),
                        encode_lower_hex_id(resolution.identity_id)
                    ),
                    None => {
                        "{\"schema_id\":null,\"object_id\":null,\"identity_id\":null}".to_owned()
                    }
                };
                self.json_response(request.request_id, payload.into_bytes())
            }
            ClientAdminCommand::ResolveAlterIdentityName {
                object_id,
                name,
                kind,
            } => {
                let token_id = self
                    .authenticated
                    .as_ref()
                    .ok_or(ClientSessionError::AuthenticationRequired)?
                    .token_id();
                let token_secret = self
                    .token_secret
                    .as_ref()
                    .ok_or(ClientSessionError::AuthenticationRequired)?;
                let identity_id = storage
                    .resolve_token_alter_identity_name(
                        token_id,
                        token_secret,
                        object_id,
                        &name,
                        kind,
                    )
                    .map_err(ClientSessionError::Storage)?;
                let payload = match identity_id {
                    Some(identity_id) => format!(
                        "{{\"identity_id\":\"{}\"}}",
                        encode_lower_hex_id(identity_id)
                    ),
                    None => "{\"identity_id\":null}".to_owned(),
                };
                self.json_response(request.request_id, payload.into_bytes())
            }
            ClientAdminCommand::ResolvePolicyName {
                schema_id,
                table_id,
                name,
                is_cell,
            } => {
                let token_id = self
                    .authenticated
                    .as_ref()
                    .ok_or(ClientSessionError::AuthenticationRequired)?
                    .token_id();
                let token_secret = self
                    .token_secret
                    .as_ref()
                    .ok_or(ClientSessionError::AuthenticationRequired)?;
                let policy_id = storage
                    .resolve_token_policy_name(
                        token_id,
                        token_secret,
                        schema_id,
                        table_id,
                        &name,
                        is_cell,
                    )
                    .map_err(ClientSessionError::Storage)?;
                let payload = match policy_id {
                    Some(policy_id) => {
                        format!("{{\"policy_id\":\"{}\"}}", encode_lower_hex_id(policy_id))
                    }
                    None => "{\"policy_id\":null}".to_owned(),
                };
                self.json_response(request.request_id, payload.into_bytes())
            }
            ClientAdminCommand::ListSdkLibrariesPage { page_limit, cursor } => {
                self.handle_sdk_library_page(staging, request.request_id, page_limit, cursor)
            }
            ClientAdminCommand::RemoveSdkLibrary {
                operation_id,
                sdk_release,
            } => {
                if !storage
                    .is_database_admin(actor_id)
                    .map_err(ClientSessionError::TokenInventory)?
                {
                    return Err(ClientSessionError::SdkLibraryRepositoryUnauthorized);
                }
                let repository = staging
                    .and_then(crate::supervisor::StagingStore::sdk_repository)
                    .ok_or(ClientSessionError::SdkLibraryRepositoryUnavailable)?;
                let removed_bytes = repository
                    .remove_release_with_operation(operation_id, actor_id, sdk_release)
                    .map_err(ClientSessionError::SdkLibraryRepository)?;
                let payload = format!(
                    "{{\"operation_id\":\"{}\",\"release\":{sdk_release},\"removed_bytes\":{removed_bytes}}}",
                    encode_lower_hex_id(operation_id)
                );
                self.json_response(request.request_id, payload.into_bytes())
            }
            ClientAdminCommand::BeginSdkLibraryInstall {
                operation_id,
                metadata_bytes,
                library_bytes,
                library_sha256,
            } => {
                if !storage
                    .is_database_admin(actor_id)
                    .map_err(ClientSessionError::TokenInventory)?
                {
                    return Err(ClientSessionError::SdkLibraryRepositoryUnauthorized);
                }
                let repository = staging
                    .and_then(crate::supervisor::StagingStore::sdk_repository)
                    .ok_or(ClientSessionError::SdkLibraryRepositoryUnavailable)?;
                let next_offset = repository
                    .begin_install_upload(
                        operation_id,
                        actor_id,
                        metadata_bytes,
                        library_bytes,
                        library_sha256,
                    )
                    .map_err(ClientSessionError::SdkLibraryRepository)?;
                let payload = format!(
                    "{{\"operation_id\":\"{}\",\"next_offset\":\"{next_offset}\"}}",
                    encode_lower_hex_id(operation_id)
                );
                self.json_response(request.request_id, payload.into_bytes())
            }
            ClientAdminCommand::SdkLibraryOperationStatus { operation_id } => {
                let repository = staging
                    .and_then(crate::supervisor::StagingStore::sdk_repository)
                    .ok_or(ClientSessionError::SdkLibraryRepositoryUnavailable)?;
                let payload = match repository
                    .operation_outcome(operation_id)
                    .map_err(ClientSessionError::SdkLibraryRepository)?
                {
                    None => format!(
                        "{{\"operation_id\":\"{}\",\"state\":\"unknown\"}}",
                        encode_lower_hex_id(operation_id)
                    ),
                    Some(crate::sdk_registry::SdkLibraryOperationOutcome::Installed {
                        owner_id,
                        sdk_release,
                        sha256,
                        library_bytes,
                    }) if owner_id == actor_id => format!(
                        "{{\"operation_id\":\"{}\",\"state\":\"succeeded\",\"operation\":\"install\",\"release\":{sdk_release},\"sha256\":\"{}\",\"library_bytes\":{library_bytes}}}",
                        encode_lower_hex_id(operation_id),
                        encode_lower_hex_bytes(&sha256)
                    ),
                    Some(crate::sdk_registry::SdkLibraryOperationOutcome::Removed {
                        owner_id,
                        sdk_release,
                        removed_bytes,
                    }) if owner_id == actor_id => format!(
                        "{{\"operation_id\":\"{}\",\"state\":\"succeeded\",\"operation\":\"remove\",\"release\":{sdk_release},\"removed_bytes\":{removed_bytes}}}",
                        encode_lower_hex_id(operation_id)
                    ),
                    Some(_) => {
                        return Err(ClientSessionError::SdkLibraryRepositoryUnauthorized);
                    }
                };
                self.json_response(request.request_id, payload.into_bytes())
            }
            ClientAdminCommand::FinishSdkLibraryInstall { operation_id } => {
                if !storage
                    .is_database_admin(actor_id)
                    .map_err(ClientSessionError::TokenInventory)?
                {
                    return Err(ClientSessionError::SdkLibraryRepositoryUnauthorized);
                }
                let repository = staging
                    .and_then(crate::supervisor::StagingStore::sdk_repository)
                    .ok_or(ClientSessionError::SdkLibraryRepositoryUnavailable)?;
                let release = repository
                    .finish_install_upload(operation_id, actor_id)
                    .map_err(ClientSessionError::SdkLibraryRepository)?;
                let payload = format!(
                    "{{\"operation_id\":\"{}\",\"release\":{},\"sha256\":\"{}\",\"library_bytes\":{}}}",
                    encode_lower_hex_id(operation_id),
                    release.sdk_release,
                    encode_lower_hex_bytes(&release.sha256),
                    release.library_bytes
                );
                self.json_response(request.request_id, payload.into_bytes())
            }
            ClientAdminCommand::ListSdkLibraries => {
                let repository = staging
                    .and_then(crate::supervisor::StagingStore::sdk_repository)
                    .ok_or(ClientSessionError::SdkLibraryRepositoryUnavailable)?;
                let releases = repository
                    .list()
                    .map_err(ClientSessionError::SdkLibraryRepository)?;
                let mut payload = String::from("{\"sdk_libraries\":[");
                for (index, release) in releases.iter().enumerate() {
                    if index != 0 {
                        payload.push(',');
                    }
                    payload.push_str("{\"release\":");
                    payload.push_str(&release.sdk_release.to_string());
                    payload.push_str(",\"native_abis\":[");
                    for (abi_index, abi) in release.native_abis.iter().enumerate() {
                        if abi_index != 0 {
                            payload.push(',');
                        }
                        payload.push_str(&abi.to_string());
                    }
                    payload.push_str("],\"target\":\"");
                    push_json_string(&mut payload, &release.target);
                    payload.push_str("\",\"cpu_baseline\":\"");
                    push_json_string(&mut payload, &release.cpu_baseline);
                    payload.push_str("\",\"runtime_profile\":\"");
                    push_json_string(&mut payload, &release.runtime_profile);
                    payload.push_str("\",\"soname\":\"");
                    push_json_string(&mut payload, &release.soname);
                    payload.push_str("\",\"sha256\":\"");
                    payload.push_str(&encode_lower_hex_bytes(&release.sha256));
                    payload.push_str("\",\"needed\":[");
                    for (dependency_index, dependency) in release.needed.iter().enumerate() {
                        if dependency_index != 0 {
                            payload.push(',');
                        }
                        payload.push('"');
                        push_json_string(&mut payload, dependency);
                        payload.push('"');
                    }
                    payload.push_str("],\"library_bytes\":");
                    payload.push_str(&release.library_bytes.to_string());
                    payload.push('}');
                    if payload.len() > CLIENT_MAX_CONTROL_BYTES {
                        return Err(ClientSessionError::ResourceLimit);
                    }
                }
                payload.push_str("]}");
                if payload.len() > CLIENT_MAX_CONTROL_BYTES {
                    return Err(ClientSessionError::ResourceLimit);
                }
                self.json_response(request.request_id, payload.into_bytes())
            }
            ClientAdminCommand::ListSchemas => {
                let token_id = self
                    .authenticated
                    .as_ref()
                    .ok_or(ClientSessionError::AuthenticationRequired)?
                    .token_id();
                let token_secret = self
                    .token_secret
                    .as_ref()
                    .ok_or(ClientSessionError::AuthenticationRequired)?;
                let schemas = storage
                    .discover_token_authorized_schemas(token_id, token_secret)
                    .map_err(ClientSessionError::Storage)?;
                let mut payload = String::from("{\"schemas\":[");
                for (index, schema) in schemas.iter().enumerate() {
                    if index != 0 {
                        payload.push(',');
                    }
                    payload.push_str("{\"schema_id\":\"");
                    payload.push_str(&encode_lower_hex_id(schema.schema_id));
                    payload.push_str("\",\"name\":\"");
                    push_json_string(&mut payload, &schema.name);
                    payload.push_str("\"}");
                }
                payload.push_str("]}");
                if payload.len() > CLIENT_MAX_CONTROL_BYTES {
                    return Err(ClientSessionError::ResourceLimit);
                }
                self.json_response(request.request_id, payload.into_bytes())
            }
            ClientAdminCommand::ListSchemasPage { page_limit, cursor } => self
                .handle_metadata_discovery_page(
                    storage,
                    request.request_id,
                    MetadataPageQuery::Schemas,
                    page_limit,
                    cursor,
                ),
            ClientAdminCommand::ListRelations { schema_id } => {
                let token_id = self
                    .authenticated
                    .as_ref()
                    .ok_or(ClientSessionError::AuthenticationRequired)?
                    .token_id();
                let token_secret = self
                    .token_secret
                    .as_ref()
                    .ok_or(ClientSessionError::AuthenticationRequired)?;
                let relations = storage
                    .discover_token_authorized_relations(token_id, token_secret, schema_id)
                    .map_err(ClientSessionError::Storage)?;
                let mut payload = String::from("{\"relations\":[");
                if let Some(relations) = relations {
                    for (index, relation) in relations.iter().enumerate() {
                        if index != 0 {
                            payload.push(',');
                        }
                        payload.push_str("{\"relation_id\":\"");
                        payload.push_str(&encode_lower_hex_id(relation.relation_id));
                        payload.push_str("\",\"name\":\"");
                        push_json_string(&mut payload, &relation.name);
                        payload.push_str("\"}");
                    }
                }
                payload.push_str("]}");
                if payload.len() > CLIENT_MAX_CONTROL_BYTES {
                    return Err(ClientSessionError::ResourceLimit);
                }
                self.json_response(request.request_id, payload.into_bytes())
            }
            ClientAdminCommand::ListRelationsPage {
                schema_id,
                page_limit,
                cursor,
            } => self.handle_metadata_discovery_page(
                storage,
                request.request_id,
                MetadataPageQuery::Relations(schema_id),
                page_limit,
                cursor,
            ),
            ClientAdminCommand::ListColumns {
                schema_id,
                relation_id,
            } => {
                let token_id = self
                    .authenticated
                    .as_ref()
                    .ok_or(ClientSessionError::AuthenticationRequired)?
                    .token_id();
                let token_secret = self
                    .token_secret
                    .as_ref()
                    .ok_or(ClientSessionError::AuthenticationRequired)?;
                let columns = storage
                    .discover_token_authorized_columns(
                        token_id,
                        token_secret,
                        schema_id,
                        relation_id,
                    )
                    .map_err(ClientSessionError::Storage)?;
                let mut payload = String::from("{\"columns\":[");
                if let Some(columns) = columns {
                    for (index, column) in columns.iter().enumerate() {
                        if index != 0 {
                            payload.push(',');
                        }
                        payload.push_str("{\"column_id\":\"");
                        payload.push_str(&encode_lower_hex_id(column.column_id));
                        payload.push_str("\",\"name\":\"");
                        push_json_string(&mut payload, &column.name);
                        payload.push_str("\",\"ordinal\":");
                        payload.push_str(&column.ordinal.to_string());
                        payload.push_str(" ,\"scalar_type\":\"");
                        push_json_string(
                            &mut payload,
                            &crate::metadata_discovery::scalar_type_name(column.scalar_type),
                        );
                        payload.push_str("\",\"nullable\":");
                        payload.push_str(if column.nullable { "true" } else { "false" });
                        payload.push('}');
                    }
                }
                payload.push_str("]}");
                if payload.len() > CLIENT_MAX_CONTROL_BYTES {
                    return Err(ClientSessionError::ResourceLimit);
                }
                self.json_response(request.request_id, payload.into_bytes())
            }
            ClientAdminCommand::ListColumnsPage {
                schema_id,
                relation_id,
                page_limit,
                cursor,
            } => self.handle_metadata_discovery_page(
                storage,
                request.request_id,
                MetadataPageQuery::Columns {
                    schema_id,
                    relation_id,
                },
                page_limit,
                cursor,
            ),
            ClientAdminCommand::ListIndexesPage {
                schema_id,
                relation_id,
                page_limit,
                cursor,
            } => self.handle_metadata_discovery_page(
                storage,
                request.request_id,
                MetadataPageQuery::Indexes {
                    schema_id,
                    relation_id,
                },
                page_limit,
                cursor,
            ),
            ClientAdminCommand::ListConstraintsPage {
                schema_id,
                relation_id,
                page_limit,
                cursor,
            } => self.handle_metadata_discovery_page(
                storage,
                request.request_id,
                MetadataPageQuery::Constraints {
                    schema_id,
                    relation_id,
                },
                page_limit,
                cursor,
            ),
            ClientAdminCommand::ListGrantsPage {
                schema_id,
                relation_id,
                page_limit,
                cursor,
            } => self.handle_metadata_discovery_page(
                storage,
                request.request_id,
                MetadataPageQuery::Grants {
                    schema_id,
                    relation_id,
                },
                page_limit,
                cursor,
            ),
            ClientAdminCommand::ListPoliciesPage {
                schema_id,
                relation_id,
                page_limit,
                cursor,
            } => self.handle_metadata_discovery_page(
                storage,
                request.request_id,
                MetadataPageQuery::Policies {
                    schema_id,
                    relation_id,
                },
                page_limit,
                cursor,
            ),
            ClientAdminCommand::ListDependenciesPage {
                object_id,
                page_limit,
                cursor,
            } => self.handle_metadata_discovery_page(
                storage,
                request.request_id,
                MetadataPageQuery::Dependencies(object_id),
                page_limit,
                cursor,
            ),
            ClientAdminCommand::ListTokens {
                user_id,
                after_token_id,
            } => {
                let page = storage
                    .list_tokens_page(actor_id, user_id, after_token_id)
                    .map_err(ClientSessionError::TokenInventory)?;
                let mut payload = String::from("{\"tokens\":[");
                for (index, token) in page.tokens.iter().enumerate() {
                    if index != 0 {
                        payload.push(',');
                    }
                    payload.push_str("{\"token_id\":\"");
                    payload.push_str(&encode_lower_hex_id(token.token_id));
                    payload.push_str("\",\"label\":\"");
                    push_json_string(&mut payload, &token.label);
                    payload.push_str("\",\"issued_at_us\":");
                    payload.push_str(&token.issued_at_us.to_string());
                    payload.push_str(" ,\"expires_at_us\":");
                    payload.push_str(&token.expires_at_us.to_string());
                    payload.push_str(" ,\"revoked\":");
                    payload.push_str(if token.revoked { "true" } else { "false" });
                    payload.push_str(" ,\"revoked_at_us\":");
                    if let Some(revoked_at) = token.revoked_at_us {
                        payload.push_str(&revoked_at.to_string());
                    } else {
                        payload.push_str("null");
                    }
                    payload.push('}');
                }
                payload.push_str("],\"next_after_token_id\":");
                if let Some(cursor) = page.next_after_token_id {
                    payload.push('"');
                    payload.push_str(&encode_lower_hex_id(cursor));
                    payload.push('"');
                } else {
                    payload.push_str("null");
                }
                payload.push('}');
                if payload.len() > CLIENT_MAX_CONTROL_BYTES {
                    return Err(ClientSessionError::ResourceLimit);
                }
                self.json_response(request.request_id, payload.into_bytes())
            }
            ClientAdminCommand::ListAuditEvents { page_limit, cursor } => {
                let page = storage
                    .list_audit_events(actor_id, page_limit, cursor.as_ref())
                    .map_err(ClientSessionError::AdministrativeAuditList)?;
                let mut payload = String::from("{\"events\":[");
                for (index, event) in page.events.iter().enumerate() {
                    if index != 0 {
                        payload.push(',');
                    }
                    payload.push_str("{\"event_id\":\"");
                    payload.push_str(&encode_lower_hex_id(event.event_id));
                    payload.push_str("\",\"actor_id\":");
                    if let Some(actor_id) = event.actor_id {
                        payload.push('"');
                        payload.push_str(&encode_lower_hex_id(actor_id));
                        payload.push('"');
                    } else {
                        payload.push_str("null");
                    }
                    payload.push_str(" ,\"correlation_id\":\"");
                    payload.push_str(&encode_lower_hex_id(event.correlation_id));
                    payload.push_str("\",\"occurred_at_us\":");
                    payload.push_str(&event.occurred_at_us.to_string());
                    payload.push_str(" ,\"action\":");
                    payload.push_str(&(event.action as u16).to_string());
                    payload.push_str(" ,\"result\":");
                    payload.push_str(&(event.result as u16).to_string());
                    payload.push_str(" ,\"target_ids\":[");
                    for (target_index, target_id) in event.target_ids.iter().enumerate() {
                        if target_index != 0 {
                            payload.push(',');
                        }
                        payload.push('"');
                        payload.push_str(&encode_lower_hex_id(*target_id));
                        payload.push('"');
                    }
                    payload.push_str("]}");
                }
                payload.push_str("],\"next_cursor\":");
                if let Some(cursor) = page.continuation {
                    payload.push('"');
                    payload.push_str(&cursor.to_wire());
                    payload.push('"');
                } else {
                    payload.push_str("null");
                }
                payload.push('}');
                if payload.len() > CLIENT_MAX_CONTROL_BYTES {
                    return Err(ClientSessionError::ResourceLimit);
                }
                self.json_response(request.request_id, payload.into_bytes())
            }
            ClientAdminCommand::ListOwnInvocations {
                response_version,
                page_limit,
                cursor,
            } => {
                let now_us = current_unix_micros().ok_or(ClientSessionError::UnsupportedRequest)?;
                let page = storage
                    .list_own_invocations(actor_id, now_us, page_limit, cursor.as_ref())
                    .map_err(ClientSessionError::Status)?;
                let mut payload = format!(
                    "{{\"observed_at_us\":{},\"consistency\":\"best_effort\",\"invocations\":[",
                    page.observed_at_us
                );
                for (index, invocation) in page.invocations.iter().enumerate() {
                    if index != 0 {
                        payload.push(',');
                    }
                    let (state, started_at_us, deadline_at_us, cancellation_requested) =
                        match invocation.status {
                            crate::storage_manager::InvocationStatus::Active {
                                started_at_us,
                                deadline_at_us,
                                cancellation_requested,
                            } => (
                                "active",
                                started_at_us,
                                deadline_at_us,
                                cancellation_requested,
                            ),
                            crate::storage_manager::InvocationStatus::Committing {
                                started_at_us,
                                deadline_at_us,
                                cancellation_requested,
                            } => (
                                "committing",
                                started_at_us,
                                deadline_at_us,
                                cancellation_requested,
                            ),
                            _ => return Err(ClientSessionError::UnsupportedRequest),
                        };
                    payload.push_str("{\"invocation_id\":\"");
                    payload.push_str(&encode_lower_hex_id(invocation.invocation_id));
                    payload.push_str("\",\"state\":\"");
                    payload.push_str(state);
                    payload.push_str("\",\"started_at_us\":");
                    payload.push_str(&started_at_us.to_string());
                    payload.push_str(",\"deadline_at_us\":");
                    push_json_optional_i64(&mut payload, deadline_at_us);
                    payload.push_str(",\"cancellation_requested\":");
                    payload.push_str(if cancellation_requested {
                        "true"
                    } else {
                        "false"
                    });
                    if response_version >= 2 {
                        payload.push_str(",\"staged_rows\":");
                        payload.push_str(&invocation.staged_rows.to_string());
                        payload.push_str(",\"staged_value_bytes\":");
                        payload.push_str(&invocation.staged_value_bytes.to_string());
                    }
                    payload.push('}');
                }
                payload.push_str("],\"next_cursor\":");
                if let Some(cursor) = page.continuation {
                    payload.push('"');
                    payload.push_str(&cursor.to_wire());
                    payload.push('"');
                } else {
                    payload.push_str("null");
                }
                payload.push('}');
                if payload.len() > CLIENT_MAX_CONTROL_BYTES {
                    return Err(ClientSessionError::ResourceLimit);
                }
                self.json_response(request.request_id, payload.into_bytes())
            }
            ClientAdminCommand::ListRuntimeInvocations {
                response_version,
                page_limit,
                cursor,
            } => {
                let now_us = current_unix_micros().ok_or(ClientSessionError::UnsupportedRequest)?;
                let page = storage
                    .list_runtime_invocations(actor_id, now_us, page_limit, cursor.as_ref())
                    .map_err(ClientSessionError::AdministrativeRuntimeList)?;
                let mut payload = format!(
                    "{{\"observed_at_us\":{},\"consistency\":\"best_effort\",\"invocations\":[",
                    page.observed_at_us
                );
                for (index, invocation) in page.invocations.iter().enumerate() {
                    if index != 0 {
                        payload.push(',');
                    }
                    let (state, started_at_us, deadline_at_us, cancellation_requested) =
                        match &invocation.status {
                            crate::storage_manager::InvocationStatus::Active {
                                started_at_us,
                                deadline_at_us,
                                cancellation_requested,
                            } => (
                                "active",
                                Some(*started_at_us),
                                *deadline_at_us,
                                *cancellation_requested,
                            ),
                            crate::storage_manager::InvocationStatus::Committing {
                                started_at_us,
                                deadline_at_us,
                                cancellation_requested,
                            } => (
                                "committing",
                                Some(*started_at_us),
                                *deadline_at_us,
                                *cancellation_requested,
                            ),
                            crate::storage_manager::InvocationStatus::Terminal(outcome) => (
                                match outcome.status {
                                    crate::outcome::OutcomeStatus::Committed => "committed",
                                    crate::outcome::OutcomeStatus::Aborted => "aborted",
                                },
                                None,
                                None,
                                false,
                            ),
                        };
                    payload.push_str("{\"invocation_id\":\"");
                    payload.push_str(&encode_lower_hex_id(invocation.invocation_id));
                    payload.push_str("\",\"transaction_id\":\"");
                    payload.push_str(&encode_lower_hex_id(invocation.transaction_id));
                    payload.push_str("\",\"principal_id\":\"");
                    payload.push_str(&encode_lower_hex_id(invocation.principal_id));
                    payload.push_str("\",\"workload_class\":\"");
                    payload.push_str(match invocation.workload_class {
                        crate::supervisor::WorkloadClass::Operational => "operational",
                        crate::supervisor::WorkloadClass::Analytical => "analytical",
                    });
                    payload.push_str("\",\"state\":\"");
                    payload.push_str(state);
                    payload.push_str("\",\"started_at_us\":");
                    push_json_optional_i64(&mut payload, started_at_us);
                    payload.push_str(" ,\"age_us\":");
                    payload.push_str(&invocation.age_us.to_string());
                    payload.push_str(" ,\"deadline_at_us\":");
                    push_json_optional_i64(&mut payload, deadline_at_us);
                    payload.push_str(" ,\"staged_rows\":");
                    payload.push_str(&invocation.staged_rows.to_string());
                    payload.push_str(" ,\"staged_value_bytes\":");
                    payload.push_str(&invocation.staged_value_bytes.to_string());
                    payload.push_str(" ,\"snapshot_pin_count\":");
                    payload.push_str(&invocation.snapshot_pin_count.to_string());
                    payload.push_str(" ,\"cancellation_requested\":");
                    payload.push_str(if cancellation_requested {
                        "true"
                    } else {
                        "false"
                    });
                    payload.push_str(" ,\"cancellation_reason_code\":");
                    if let Some(reason) = invocation.cancellation_reason_code {
                        payload.push_str(&reason.to_string());
                    } else {
                        payload.push_str("null");
                    }
                    payload.push('}');
                }
                encode_fence_waits(
                    &mut payload,
                    RuntimeInvocationSummary {
                        pinned_snapshot_count: page.pinned_snapshot_count,
                        oldest_snapshot_pin_age_us: page.oldest_snapshot_pin_age_us,
                        invocations_admitted_total: page.invocations_admitted_total,
                        invocation_conflicts_total: page.invocation_conflicts_total,
                        admin_cancellations_accepted_total: page.admin_cancellations_accepted_total,
                        admin_cancellations_denied_total: page.admin_cancellations_denied_total,
                        invocation_policy_denials_total: page.invocation_policy_denials_total,
                        invocation_staged_value_bytes_total: page
                            .invocation_staged_value_bytes_total,
                        invocation_commit_latency_buckets: page.invocation_commit_latency_buckets,
                        invocation_redb_write_acquisition_buckets: page
                            .invocation_redb_write_acquisition_buckets,
                        include_write_lock_wait: response_version >= 7,
                        include_outcome_maintenance: response_version >= 2,
                        outcome_maintenance_passes_total: page.outcome_maintenance_passes_total,
                        outcome_maintenance_failures_total: page.outcome_maintenance_failures_total,
                        outcomes_pruned_total: page.outcomes_pruned_total,
                        waits: &page.fence_waits,
                        truncated: page.fence_waits_truncated,
                    },
                );
                if response_version >= 5 {
                    payload.push_str(",\"gateway_result_bytes_relayed_total\":");
                    payload.push_str(
                        &crate::supervisor_ipc::gateway_result_bytes_relayed_total().to_string(),
                    );
                }
                if response_version >= 3 {
                    let usage = staging
                        .map(crate::supervisor::StagingStore::runtime_usage_summary)
                        .transpose()
                        .map_err(ClientSessionError::Staging)?
                        .unwrap_or_default();
                    payload.push_str(",\"supervisor_queue\":{\"pending_submissions\":");
                    payload.push_str(&usage.pending_submissions.to_string());
                    payload.push_str(",\"starting_submissions\":");
                    payload.push_str(&usage.starting_submissions.to_string());
                    payload.push_str(",\"started_submissions\":");
                    payload.push_str(&usage.started_submissions.to_string());
                    payload.push_str(",\"queued_operational_starts\":");
                    payload.push_str(&usage.queued_operational_starts.to_string());
                    payload.push_str(",\"queued_analytical_starts\":");
                    payload.push_str(&usage.queued_analytical_starts.to_string());
                    if response_version >= 4 {
                        payload.push_str(",\"oldest_queued_start_wait_us\":");
                        payload.push_str(&usage.oldest_queued_start_wait_us.to_string());
                    }
                    if response_version >= 8 {
                        payload.push_str(",\"operational_start_wait_buckets\":[");
                        for (index, bucket) in
                            usage.operational_start_wait_buckets.iter().enumerate()
                        {
                            if index != 0 {
                                payload.push(',');
                            }
                            payload.push_str(&bucket.to_string());
                        }
                        payload.push_str("],\"analytical_start_wait_buckets\":[");
                        for (index, bucket) in
                            usage.analytical_start_wait_buckets.iter().enumerate()
                        {
                            if index != 0 {
                                payload.push(',');
                            }
                            payload.push_str(&bucket.to_string());
                        }
                        payload.push(']');
                    }
                    payload.push_str(",\"reserved_submission_bytes\":");
                    payload.push_str(&usage.reserved_submission_bytes.to_string());
                    payload.push('}');
                }
                payload.push_str(" ,\"next_cursor\":");
                if let Some(cursor) = page.continuation {
                    payload.push('"');
                    payload.push_str(&cursor.to_wire());
                    payload.push('"');
                } else {
                    payload.push_str("null");
                }
                payload.push('}');
                if payload.len() > CLIENT_MAX_CONTROL_BYTES {
                    return Err(ClientSessionError::ResourceLimit);
                }
                self.json_response(request.request_id, payload.into_bytes())
            }
            ClientAdminCommand::PrepareCreateUser { name } => {
                if self.pending_admin_operation.is_some() {
                    return Err(ClientSessionError::WrongMode);
                }
                let mut operation_id = [0; 16];
                getrandom::fill(&mut operation_id).map_err(|_| {
                    ClientSessionError::Storage(TransactionError::RandomnessUnavailable)
                })?;
                if operation_id == [0; 16] {
                    return Err(ClientSessionError::Storage(
                        TransactionError::RandomnessUnavailable,
                    ));
                }
                self.pending_admin_operation = Some(PendingAdminOperation {
                    operation_id,
                    action: PendingAdminAction::CreateUser { name },
                });
                self.json_response(
                    request.request_id,
                    format!(
                        "{{\"operation_id\":\"{}\",\"state\":\"prepared\"}}",
                        encode_lower_hex_id(operation_id)
                    )
                    .into_bytes(),
                )
            }
            ClientAdminCommand::PrepareCreateUserNoLogin { name } => {
                if self.pending_admin_operation.is_some() {
                    return Err(ClientSessionError::WrongMode);
                }
                let mut operation_id = [0; 16];
                getrandom::fill(&mut operation_id).map_err(|_| {
                    ClientSessionError::Storage(TransactionError::RandomnessUnavailable)
                })?;
                if operation_id == [0; 16] {
                    return Err(ClientSessionError::Storage(
                        TransactionError::RandomnessUnavailable,
                    ));
                }
                self.pending_admin_operation = Some(PendingAdminOperation {
                    operation_id,
                    action: PendingAdminAction::CreateUserNoLogin { name },
                });
                self.json_response(
                    request.request_id,
                    format!(
                        "{{\"operation_id\":\"{}\",\"state\":\"prepared\"}}",
                        encode_lower_hex_id(operation_id)
                    )
                    .into_bytes(),
                )
            }
            ClientAdminCommand::PrepareCreateRole { name } => {
                if self.pending_admin_operation.is_some() {
                    return Err(ClientSessionError::WrongMode);
                }
                let mut operation_id = [0; 16];
                getrandom::fill(&mut operation_id).map_err(|_| {
                    ClientSessionError::Storage(TransactionError::RandomnessUnavailable)
                })?;
                if operation_id == [0; 16] {
                    return Err(ClientSessionError::Storage(
                        TransactionError::RandomnessUnavailable,
                    ));
                }
                self.pending_admin_operation = Some(PendingAdminOperation {
                    operation_id,
                    action: PendingAdminAction::CreateRole { name },
                });
                self.json_response(
                    request.request_id,
                    format!(
                        "{{\"operation_id\":\"{}\",\"state\":\"prepared\"}}",
                        encode_lower_hex_id(operation_id)
                    )
                    .into_bytes(),
                )
            }
            ClientAdminCommand::PrepareCreateSchema { name, owner_id } => {
                if self.pending_admin_operation.is_some() {
                    return Err(ClientSessionError::WrongMode);
                }
                let mut operation_id = [0; 16];
                getrandom::fill(&mut operation_id).map_err(|_| {
                    ClientSessionError::Storage(TransactionError::RandomnessUnavailable)
                })?;
                if operation_id == [0; 16] {
                    return Err(ClientSessionError::Storage(
                        TransactionError::RandomnessUnavailable,
                    ));
                }
                self.pending_admin_operation = Some(PendingAdminOperation {
                    operation_id,
                    action: PendingAdminAction::CreateSchema { name, owner_id },
                });
                self.json_response(
                    request.request_id,
                    format!(
                        "{{\"operation_id\":\"{}\",\"state\":\"prepared\"}}",
                        encode_lower_hex_id(operation_id)
                    )
                    .into_bytes(),
                )
            }
            ClientAdminCommand::PrepareDropSchema { schema_id } => {
                if self.pending_admin_operation.is_some() {
                    return Err(ClientSessionError::WrongMode);
                }
                let mut operation_id = [0; 16];
                getrandom::fill(&mut operation_id).map_err(|_| {
                    ClientSessionError::Storage(TransactionError::RandomnessUnavailable)
                })?;
                if operation_id == [0; 16] {
                    return Err(ClientSessionError::Storage(
                        TransactionError::RandomnessUnavailable,
                    ));
                }
                self.pending_admin_operation = Some(PendingAdminOperation {
                    operation_id,
                    action: PendingAdminAction::DropSchema { schema_id },
                });
                self.json_response(
                    request.request_id,
                    format!(
                        "{{\"operation_id\":\"{}\",\"state\":\"prepared\"}}",
                        encode_lower_hex_id(operation_id)
                    )
                    .into_bytes(),
                )
            }
            ClientAdminCommand::PrepareSetSchemaOwner {
                schema_id,
                owner_id,
            } => {
                if self.pending_admin_operation.is_some() {
                    return Err(ClientSessionError::WrongMode);
                }
                let mut operation_id = [0; 16];
                getrandom::fill(&mut operation_id).map_err(|_| {
                    ClientSessionError::Storage(TransactionError::RandomnessUnavailable)
                })?;
                if operation_id == [0; 16] {
                    return Err(ClientSessionError::Storage(
                        TransactionError::RandomnessUnavailable,
                    ));
                }
                self.pending_admin_operation = Some(PendingAdminOperation {
                    operation_id,
                    action: PendingAdminAction::SetSchemaOwner {
                        schema_id,
                        owner_id,
                    },
                });
                self.json_response(
                    request.request_id,
                    format!(
                        "{{\"operation_id\":\"{}\",\"state\":\"prepared\"}}",
                        encode_lower_hex_id(operation_id)
                    )
                    .into_bytes(),
                )
            }
            ClientAdminCommand::PrepareRenameSchema { schema_id, name } => {
                if self.pending_admin_operation.is_some() {
                    return Err(ClientSessionError::WrongMode);
                }
                let mut operation_id = [0; 16];
                getrandom::fill(&mut operation_id).map_err(|_| {
                    ClientSessionError::Storage(TransactionError::RandomnessUnavailable)
                })?;
                if operation_id == [0; 16] {
                    return Err(ClientSessionError::Storage(
                        TransactionError::RandomnessUnavailable,
                    ));
                }
                self.pending_admin_operation = Some(PendingAdminOperation {
                    operation_id,
                    action: PendingAdminAction::RenameSchema { schema_id, name },
                });
                self.json_response(
                    request.request_id,
                    format!(
                        "{{\"operation_id\":\"{}\",\"state\":\"prepared\"}}",
                        encode_lower_hex_id(operation_id)
                    )
                    .into_bytes(),
                )
            }
            ClientAdminCommand::PrepareDropTable { table_id } => {
                if self.pending_admin_operation.is_some() {
                    return Err(ClientSessionError::WrongMode);
                }
                let mut operation_id = [0; 16];
                getrandom::fill(&mut operation_id).map_err(|_| {
                    ClientSessionError::Storage(TransactionError::RandomnessUnavailable)
                })?;
                if operation_id == [0; 16] {
                    return Err(ClientSessionError::Storage(
                        TransactionError::RandomnessUnavailable,
                    ));
                }
                self.pending_admin_operation = Some(PendingAdminOperation {
                    operation_id,
                    action: PendingAdminAction::DropTable { table_id },
                });
                self.json_response(
                    request.request_id,
                    format!(
                        "{{\"operation_id\":\"{}\",\"state\":\"prepared\"}}",
                        encode_lower_hex_id(operation_id)
                    )
                    .into_bytes(),
                )
            }
            ClientAdminCommand::PrepareTruncateTable { table_id } => {
                if self.pending_admin_operation.is_some() {
                    return Err(ClientSessionError::WrongMode);
                }
                let mut operation_id = [0; 16];
                getrandom::fill(&mut operation_id).map_err(|_| {
                    ClientSessionError::Storage(TransactionError::RandomnessUnavailable)
                })?;
                if operation_id == [0; 16] {
                    return Err(ClientSessionError::Storage(
                        TransactionError::RandomnessUnavailable,
                    ));
                }
                self.pending_admin_operation = Some(PendingAdminOperation {
                    operation_id,
                    action: PendingAdminAction::TruncateTable { table_id },
                });
                self.json_response(
                    request.request_id,
                    format!(
                        "{{\"operation_id\":\"{}\",\"state\":\"prepared\"}}",
                        encode_lower_hex_id(operation_id)
                    )
                    .into_bytes(),
                )
            }
            ClientAdminCommand::PrepareDropForeignKey { foreign_key_id } => {
                if self.pending_admin_operation.is_some() {
                    return Err(ClientSessionError::WrongMode);
                }
                let mut operation_id = [0; 16];
                getrandom::fill(&mut operation_id).map_err(|_| {
                    ClientSessionError::Storage(TransactionError::RandomnessUnavailable)
                })?;
                if operation_id == [0; 16] {
                    return Err(ClientSessionError::Storage(
                        TransactionError::RandomnessUnavailable,
                    ));
                }
                self.pending_admin_operation = Some(PendingAdminOperation {
                    operation_id,
                    action: PendingAdminAction::DropForeignKey { foreign_key_id },
                });
                self.json_response(
                    request.request_id,
                    format!(
                        "{{\"operation_id\":\"{}\",\"state\":\"prepared\"}}",
                        encode_lower_hex_id(operation_id)
                    )
                    .into_bytes(),
                )
            }
            ClientAdminCommand::PrepareDropCheckConstraint { constraint_id } => {
                if self.pending_admin_operation.is_some() {
                    return Err(ClientSessionError::WrongMode);
                }
                let mut operation_id = [0; 16];
                getrandom::fill(&mut operation_id).map_err(|_| {
                    ClientSessionError::Storage(TransactionError::RandomnessUnavailable)
                })?;
                if operation_id == [0; 16] {
                    return Err(ClientSessionError::Storage(
                        TransactionError::RandomnessUnavailable,
                    ));
                }
                self.pending_admin_operation = Some(PendingAdminOperation {
                    operation_id,
                    action: PendingAdminAction::DropCheckConstraint { constraint_id },
                });
                self.json_response(
                    request.request_id,
                    format!(
                        "{{\"operation_id\":\"{}\",\"state\":\"prepared\"}}",
                        encode_lower_hex_id(operation_id)
                    )
                    .into_bytes(),
                )
            }
            ClientAdminCommand::PrepareDropPrimaryKey { constraint_id } => {
                if self.pending_admin_operation.is_some() {
                    return Err(ClientSessionError::WrongMode);
                }
                let mut operation_id = [0; 16];
                getrandom::fill(&mut operation_id).map_err(|_| {
                    ClientSessionError::Storage(TransactionError::RandomnessUnavailable)
                })?;
                if operation_id == [0; 16] {
                    return Err(ClientSessionError::Storage(
                        TransactionError::RandomnessUnavailable,
                    ));
                }
                self.pending_admin_operation = Some(PendingAdminOperation {
                    operation_id,
                    action: PendingAdminAction::DropPrimaryKey { constraint_id },
                });
                self.json_response(
                    request.request_id,
                    format!(
                        "{{\"operation_id\":\"{}\",\"state\":\"prepared\"}}",
                        encode_lower_hex_id(operation_id)
                    )
                    .into_bytes(),
                )
            }
            ClientAdminCommand::PrepareDropUniqueConstraint { constraint_id } => {
                if self.pending_admin_operation.is_some() {
                    return Err(ClientSessionError::WrongMode);
                }
                let mut operation_id = [0; 16];
                getrandom::fill(&mut operation_id).map_err(|_| {
                    ClientSessionError::Storage(TransactionError::RandomnessUnavailable)
                })?;
                if operation_id == [0; 16] {
                    return Err(ClientSessionError::Storage(
                        TransactionError::RandomnessUnavailable,
                    ));
                }
                self.pending_admin_operation = Some(PendingAdminOperation {
                    operation_id,
                    action: PendingAdminAction::DropUniqueConstraint { constraint_id },
                });
                self.json_response(
                    request.request_id,
                    format!(
                        "{{\"operation_id\":\"{}\",\"state\":\"prepared\"}}",
                        encode_lower_hex_id(operation_id)
                    )
                    .into_bytes(),
                )
            }
            ClientAdminCommand::PrepareCreateForeignKey {
                source_table_id,
                source_column_ids,
                target_constraint_id,
                name,
            } => {
                if self.pending_admin_operation.is_some() {
                    return Err(ClientSessionError::WrongMode);
                }
                let mut operation_id = [0; 16];
                getrandom::fill(&mut operation_id).map_err(|_| {
                    ClientSessionError::Storage(TransactionError::RandomnessUnavailable)
                })?;
                if operation_id == [0; 16] {
                    return Err(ClientSessionError::Storage(
                        TransactionError::RandomnessUnavailable,
                    ));
                }
                self.pending_admin_operation = Some(PendingAdminOperation {
                    operation_id,
                    action: PendingAdminAction::CreateForeignKey {
                        source_table_id,
                        source_column_ids,
                        target_constraint_id,
                        name,
                    },
                });
                self.json_response(
                    request.request_id,
                    format!(
                        "{{\"operation_id\":\"{}\",\"state\":\"prepared\"}}",
                        encode_lower_hex_id(operation_id)
                    )
                    .into_bytes(),
                )
            }
            ClientAdminCommand::PrepareCreateUniqueConstraint {
                table_id,
                column_ids,
                name,
            } => {
                if self.pending_admin_operation.is_some() {
                    return Err(ClientSessionError::WrongMode);
                }
                let mut operation_id = [0; 16];
                getrandom::fill(&mut operation_id).map_err(|_| {
                    ClientSessionError::Storage(TransactionError::RandomnessUnavailable)
                })?;
                if operation_id == [0; 16] {
                    return Err(ClientSessionError::Storage(
                        TransactionError::RandomnessUnavailable,
                    ));
                }
                self.pending_admin_operation = Some(PendingAdminOperation {
                    operation_id,
                    action: PendingAdminAction::CreateUniqueConstraint {
                        table_id,
                        column_ids,
                        name,
                    },
                });
                self.json_response(
                    request.request_id,
                    format!(
                        "{{\"operation_id\":\"{}\",\"state\":\"prepared\"}}",
                        encode_lower_hex_id(operation_id)
                    )
                    .into_bytes(),
                )
            }
            ClientAdminCommand::PrepareCreateIndex {
                table_id,
                name,
                column_ids,
                unique,
            } => {
                if self.pending_admin_operation.is_some() {
                    return Err(ClientSessionError::WrongMode);
                }
                let mut operation_id = [0; 16];
                getrandom::fill(&mut operation_id).map_err(|_| {
                    ClientSessionError::Storage(TransactionError::RandomnessUnavailable)
                })?;
                if operation_id == [0; 16] {
                    return Err(ClientSessionError::Storage(
                        TransactionError::RandomnessUnavailable,
                    ));
                }
                self.pending_admin_operation = Some(PendingAdminOperation {
                    operation_id,
                    action: PendingAdminAction::CreateIndex {
                        table_id,
                        name,
                        column_ids,
                        unique,
                    },
                });
                self.json_response(
                    request.request_id,
                    format!(
                        "{{\"operation_id\":\"{}\",\"state\":\"prepared\"}}",
                        encode_lower_hex_id(operation_id)
                    )
                    .into_bytes(),
                )
            }
            ClientAdminCommand::PrepareDropIndex { index_id } => {
                if self.pending_admin_operation.is_some() {
                    return Err(ClientSessionError::WrongMode);
                }
                let mut operation_id = [0; 16];
                getrandom::fill(&mut operation_id).map_err(|_| {
                    ClientSessionError::Storage(TransactionError::RandomnessUnavailable)
                })?;
                if operation_id == [0; 16] {
                    return Err(ClientSessionError::Storage(
                        TransactionError::RandomnessUnavailable,
                    ));
                }
                self.pending_admin_operation = Some(PendingAdminOperation {
                    operation_id,
                    action: PendingAdminAction::DropIndex { index_id },
                });
                self.json_response(
                    request.request_id,
                    format!(
                        "{{\"operation_id\":\"{}\",\"state\":\"prepared\"}}",
                        encode_lower_hex_id(operation_id)
                    )
                    .into_bytes(),
                )
            }
            ClientAdminCommand::PrepareRebuildIndex {
                index_id,
                column_ids,
                unique,
            } => {
                if self.pending_admin_operation.is_some() {
                    return Err(ClientSessionError::WrongMode);
                }
                let mut operation_id = [0; 16];
                getrandom::fill(&mut operation_id).map_err(|_| {
                    ClientSessionError::Storage(TransactionError::RandomnessUnavailable)
                })?;
                if operation_id == [0; 16] {
                    return Err(ClientSessionError::Storage(
                        TransactionError::RandomnessUnavailable,
                    ));
                }
                self.pending_admin_operation = Some(PendingAdminOperation {
                    operation_id,
                    action: PendingAdminAction::RebuildIndex {
                        index_id,
                        column_ids,
                        unique,
                    },
                });
                self.json_response(
                    request.request_id,
                    format!(
                        "{{\"operation_id\":\"{}\",\"state\":\"prepared\"}}",
                        encode_lower_hex_id(operation_id)
                    )
                    .into_bytes(),
                )
            }
            ClientAdminCommand::PrepareRenameIndex { index_id, name } => {
                if self.pending_admin_operation.is_some() {
                    return Err(ClientSessionError::WrongMode);
                }
                let mut operation_id = [0; 16];
                getrandom::fill(&mut operation_id).map_err(|_| {
                    ClientSessionError::Storage(TransactionError::RandomnessUnavailable)
                })?;
                if operation_id == [0; 16] {
                    return Err(ClientSessionError::Storage(
                        TransactionError::RandomnessUnavailable,
                    ));
                }
                self.pending_admin_operation = Some(PendingAdminOperation {
                    operation_id,
                    action: PendingAdminAction::RenameIndex { index_id, name },
                });
                self.json_response(
                    request.request_id,
                    format!(
                        "{{\"operation_id\":\"{}\",\"state\":\"prepared\"}}",
                        encode_lower_hex_id(operation_id)
                    )
                    .into_bytes(),
                )
            }
            ClientAdminCommand::PrepareCreatePrimaryKey {
                table_id,
                column_ids,
                name,
            } => {
                if self.pending_admin_operation.is_some() {
                    return Err(ClientSessionError::WrongMode);
                }
                let mut operation_id = [0; 16];
                getrandom::fill(&mut operation_id).map_err(|_| {
                    ClientSessionError::Storage(TransactionError::RandomnessUnavailable)
                })?;
                if operation_id == [0; 16] {
                    return Err(ClientSessionError::Storage(
                        TransactionError::RandomnessUnavailable,
                    ));
                }
                self.pending_admin_operation = Some(PendingAdminOperation {
                    operation_id,
                    action: PendingAdminAction::CreatePrimaryKey {
                        table_id,
                        column_ids,
                        name,
                    },
                });
                self.json_response(
                    request.request_id,
                    format!(
                        "{{\"operation_id\":\"{}\",\"state\":\"prepared\"}}",
                        encode_lower_hex_id(operation_id)
                    )
                    .into_bytes(),
                )
            }
            ClientAdminCommand::PrepareCreateCheckConstraint {
                table_id,
                name,
                expression,
                dependency_column_ids,
            } => {
                if self.pending_admin_operation.is_some() {
                    return Err(ClientSessionError::WrongMode);
                }
                let mut operation_id = [0; 16];
                getrandom::fill(&mut operation_id).map_err(|_| {
                    ClientSessionError::Storage(TransactionError::RandomnessUnavailable)
                })?;
                if operation_id == [0; 16] {
                    return Err(ClientSessionError::Storage(
                        TransactionError::RandomnessUnavailable,
                    ));
                }
                self.pending_admin_operation = Some(PendingAdminOperation {
                    operation_id,
                    action: PendingAdminAction::CreateCheckConstraint {
                        table_id,
                        name,
                        expression,
                        dependency_column_ids,
                    },
                });
                self.json_response(
                    request.request_id,
                    format!(
                        "{{\"operation_id\":\"{}\",\"state\":\"prepared\"}}",
                        encode_lower_hex_id(operation_id)
                    )
                    .into_bytes(),
                )
            }
            ClientAdminCommand::PrepareCreatePolicy {
                replaced_policy_id,
                table_id,
                name,
                kind,
                operation,
                effect,
                recipients,
                target_column_ids,
                using_expression,
                with_check_expression,
            } => {
                if self.pending_admin_operation.is_some() {
                    return Err(ClientSessionError::WrongMode);
                }
                let mut operation_id = [0; 16];
                getrandom::fill(&mut operation_id).map_err(|_| {
                    ClientSessionError::Storage(TransactionError::RandomnessUnavailable)
                })?;
                if operation_id == [0; 16] {
                    return Err(ClientSessionError::Storage(
                        TransactionError::RandomnessUnavailable,
                    ));
                }
                self.pending_admin_operation = Some(PendingAdminOperation {
                    operation_id,
                    action: PendingAdminAction::CreatePolicy {
                        replaced_policy_id,
                        table_id,
                        name,
                        kind,
                        operation,
                        effect,
                        recipients,
                        target_column_ids,
                        using_expression,
                        with_check_expression,
                    },
                });
                self.json_response(
                    request.request_id,
                    format!(
                        "{{\"operation_id\":\"{}\",\"state\":\"prepared\"}}",
                        encode_lower_hex_id(operation_id)
                    )
                    .into_bytes(),
                )
            }
            ClientAdminCommand::PrepareDropPolicy { policy_id } => {
                if self.pending_admin_operation.is_some() {
                    return Err(ClientSessionError::WrongMode);
                }
                let mut operation_id = [0; 16];
                getrandom::fill(&mut operation_id).map_err(|_| {
                    ClientSessionError::Storage(TransactionError::RandomnessUnavailable)
                })?;
                if operation_id == [0; 16] {
                    return Err(ClientSessionError::Storage(
                        TransactionError::RandomnessUnavailable,
                    ));
                }
                self.pending_admin_operation = Some(PendingAdminOperation {
                    operation_id,
                    action: PendingAdminAction::DropPolicy { policy_id },
                });
                self.json_response(
                    request.request_id,
                    format!(
                        "{{\"operation_id\":\"{}\",\"state\":\"prepared\"}}",
                        encode_lower_hex_id(operation_id)
                    )
                    .into_bytes(),
                )
            }
            ClientAdminCommand::PrepareRenameTable { table_id, name } => {
                if self.pending_admin_operation.is_some() {
                    return Err(ClientSessionError::WrongMode);
                }
                let mut operation_id = [0; 16];
                getrandom::fill(&mut operation_id).map_err(|_| {
                    ClientSessionError::Storage(TransactionError::RandomnessUnavailable)
                })?;
                if operation_id == [0; 16] {
                    return Err(ClientSessionError::Storage(
                        TransactionError::RandomnessUnavailable,
                    ));
                }
                self.pending_admin_operation = Some(PendingAdminOperation {
                    operation_id,
                    action: PendingAdminAction::RenameTable { table_id, name },
                });
                self.json_response(
                    request.request_id,
                    format!(
                        "{{\"operation_id\":\"{}\",\"state\":\"prepared\"}}",
                        encode_lower_hex_id(operation_id)
                    )
                    .into_bytes(),
                )
            }
            ClientAdminCommand::PrepareSetTableOwner { table_id, owner_id } => {
                if self.pending_admin_operation.is_some() {
                    return Err(ClientSessionError::WrongMode);
                }
                let mut operation_id = [0; 16];
                getrandom::fill(&mut operation_id).map_err(|_| {
                    ClientSessionError::Storage(TransactionError::RandomnessUnavailable)
                })?;
                if operation_id == [0; 16] {
                    return Err(ClientSessionError::Storage(
                        TransactionError::RandomnessUnavailable,
                    ));
                }
                self.pending_admin_operation = Some(PendingAdminOperation {
                    operation_id,
                    action: PendingAdminAction::SetTableOwner { table_id, owner_id },
                });
                self.json_response(
                    request.request_id,
                    format!(
                        "{{\"operation_id\":\"{}\",\"state\":\"prepared\"}}",
                        encode_lower_hex_id(operation_id)
                    )
                    .into_bytes(),
                )
            }
            ClientAdminCommand::PrepareRenameColumn {
                table_id,
                column_id,
                name,
            } => {
                if self.pending_admin_operation.is_some() {
                    return Err(ClientSessionError::WrongMode);
                }
                let mut operation_id = [0; 16];
                getrandom::fill(&mut operation_id).map_err(|_| {
                    ClientSessionError::Storage(TransactionError::RandomnessUnavailable)
                })?;
                if operation_id == [0; 16] {
                    return Err(ClientSessionError::Storage(
                        TransactionError::RandomnessUnavailable,
                    ));
                }
                self.pending_admin_operation = Some(PendingAdminOperation {
                    operation_id,
                    action: PendingAdminAction::RenameColumn {
                        table_id,
                        column_id,
                        name,
                    },
                });
                self.json_response(
                    request.request_id,
                    format!(
                        "{{\"operation_id\":\"{}\",\"state\":\"prepared\"}}",
                        encode_lower_hex_id(operation_id)
                    )
                    .into_bytes(),
                )
            }
            ClientAdminCommand::PrepareSetColumnNullable {
                table_id,
                column_id,
                nullable,
            } => {
                if self.pending_admin_operation.is_some() {
                    return Err(ClientSessionError::WrongMode);
                }
                let mut operation_id = [0; 16];
                getrandom::fill(&mut operation_id).map_err(|_| {
                    ClientSessionError::Storage(TransactionError::RandomnessUnavailable)
                })?;
                if operation_id == [0; 16] {
                    return Err(ClientSessionError::Storage(
                        TransactionError::RandomnessUnavailable,
                    ));
                }
                self.pending_admin_operation = Some(PendingAdminOperation {
                    operation_id,
                    action: PendingAdminAction::SetColumnNullable {
                        table_id,
                        column_id,
                        nullable,
                    },
                });
                self.json_response(
                    request.request_id,
                    format!(
                        "{{\"operation_id\":\"{}\",\"state\":\"prepared\"}}",
                        encode_lower_hex_id(operation_id)
                    )
                    .into_bytes(),
                )
            }
            ClientAdminCommand::PrepareWidenVarcharColumn {
                table_id,
                column_id,
                max_chars,
            } => {
                if self.pending_admin_operation.is_some() {
                    return Err(ClientSessionError::WrongMode);
                }
                let mut operation_id = [0; 16];
                getrandom::fill(&mut operation_id).map_err(|_| {
                    ClientSessionError::Storage(TransactionError::RandomnessUnavailable)
                })?;
                if operation_id == [0; 16] {
                    return Err(ClientSessionError::Storage(
                        TransactionError::RandomnessUnavailable,
                    ));
                }
                self.pending_admin_operation = Some(PendingAdminOperation {
                    operation_id,
                    action: PendingAdminAction::WidenVarcharColumn {
                        table_id,
                        column_id,
                        max_chars,
                    },
                });
                self.json_response(
                    request.request_id,
                    format!(
                        "{{\"operation_id\":\"{}\",\"state\":\"prepared\"}}",
                        encode_lower_hex_id(operation_id)
                    )
                    .into_bytes(),
                )
            }
            ClientAdminCommand::PrepareAlterColumnDefault {
                table_id,
                column_id,
                default,
            } => {
                if self.pending_admin_operation.is_some() {
                    return Err(ClientSessionError::WrongMode);
                }
                let mut operation_id = [0; 16];
                getrandom::fill(&mut operation_id).map_err(|_| {
                    ClientSessionError::Storage(TransactionError::RandomnessUnavailable)
                })?;
                if operation_id == [0; 16] {
                    return Err(ClientSessionError::Storage(
                        TransactionError::RandomnessUnavailable,
                    ));
                }
                self.pending_admin_operation = Some(PendingAdminOperation {
                    operation_id,
                    action: PendingAdminAction::AlterColumnDefault {
                        table_id,
                        column_id,
                        default,
                    },
                });
                self.json_response(
                    request.request_id,
                    format!(
                        "{{\"operation_id\":\"{}\",\"state\":\"prepared\"}}",
                        encode_lower_hex_id(operation_id)
                    )
                    .into_bytes(),
                )
            }
            ClientAdminCommand::PrepareAddColumn { table_id, column } => {
                if self.pending_admin_operation.is_some() {
                    return Err(ClientSessionError::WrongMode);
                }
                let mut operation_id = [0; 16];
                getrandom::fill(&mut operation_id).map_err(|_| {
                    ClientSessionError::Storage(TransactionError::RandomnessUnavailable)
                })?;
                if operation_id == [0; 16] {
                    return Err(ClientSessionError::Storage(
                        TransactionError::RandomnessUnavailable,
                    ));
                }
                self.pending_admin_operation = Some(PendingAdminOperation {
                    operation_id,
                    action: PendingAdminAction::AddColumn { table_id, column },
                });
                self.json_response(
                    request.request_id,
                    format!(
                        "{{\"operation_id\":\"{}\",\"state\":\"prepared\"}}",
                        encode_lower_hex_id(operation_id)
                    )
                    .into_bytes(),
                )
            }
            ClientAdminCommand::PrepareDropColumn {
                table_id,
                column_id,
            } => {
                if self.pending_admin_operation.is_some() {
                    return Err(ClientSessionError::WrongMode);
                }
                let mut operation_id = [0; 16];
                getrandom::fill(&mut operation_id).map_err(|_| {
                    ClientSessionError::Storage(TransactionError::RandomnessUnavailable)
                })?;
                if operation_id == [0; 16] {
                    return Err(ClientSessionError::Storage(
                        TransactionError::RandomnessUnavailable,
                    ));
                }
                self.pending_admin_operation = Some(PendingAdminOperation {
                    operation_id,
                    action: PendingAdminAction::DropColumn {
                        table_id,
                        column_id,
                    },
                });
                self.json_response(
                    request.request_id,
                    format!(
                        "{{\"operation_id\":\"{}\",\"state\":\"prepared\"}}",
                        encode_lower_hex_id(operation_id)
                    )
                    .into_bytes(),
                )
            }
            ClientAdminCommand::PrepareCreateTable {
                schema_id,
                name,
                columns,
                primary_key_columns,
                primary_key_name,
                unique_constraints,
                unique_constraint_names,
                check_expressions,
                check_constraint_names,
                foreign_keys,
                foreign_key_names,
            } => {
                if self.pending_admin_operation.is_some() {
                    return Err(ClientSessionError::WrongMode);
                }
                let mut operation_id = [0; 16];
                getrandom::fill(&mut operation_id).map_err(|_| {
                    ClientSessionError::Storage(TransactionError::RandomnessUnavailable)
                })?;
                if operation_id == [0; 16] {
                    return Err(ClientSessionError::Storage(
                        TransactionError::RandomnessUnavailable,
                    ));
                }
                self.pending_admin_operation = Some(PendingAdminOperation {
                    operation_id,
                    action: PendingAdminAction::CreateTable {
                        schema_id,
                        name,
                        columns,
                        primary_key_columns,
                        primary_key_name,
                        unique_constraints,
                        unique_constraint_names,
                        check_expressions,
                        check_constraint_names,
                        foreign_keys,
                        foreign_key_names,
                    },
                });
                self.json_response(
                    request.request_id,
                    format!(
                        "{{\"operation_id\":\"{}\",\"state\":\"prepared\"}}",
                        encode_lower_hex_id(operation_id)
                    )
                    .into_bytes(),
                )
            }
            ClientAdminCommand::PrepareRenameIdentity { identity_id, name } => {
                self.prepare_identity_rename(request.request_id, identity_id, name)
            }
            ClientAdminCommand::PrepareDropIdentity { identity_id } => {
                self.prepare_identity_drop(request.request_id, identity_id)
            }
            ClientAdminCommand::PrepareSetUserLogin { user_id, enabled } => {
                self.prepare_user_login(request.request_id, user_id, enabled)
            }
            ClientAdminCommand::PrepareGrantRole { member_id, role_id } => {
                self.prepare_role_membership(request.request_id, member_id, role_id, true)
            }
            ClientAdminCommand::PrepareRevokeRole { member_id, role_id } => {
                self.prepare_role_membership(request.request_id, member_id, role_id, false)
            }
            ClientAdminCommand::PrepareSystemPrivilege {
                grantee_id,
                privilege,
                grant,
            } => self.prepare_system_privilege(request.request_id, grantee_id, privilege, grant),
            ClientAdminCommand::PrepareObjectGrant {
                grantee_id,
                object_id,
                privileges,
                target,
                entries,
                grant,
            } => self.prepare_object_grant(
                request.request_id,
                grantee_id,
                object_id,
                privileges,
                target,
                entries,
                grant,
            ),
            ClientAdminCommand::PrepareRevoke { token_id } => {
                if self.pending_admin_operation.is_some() {
                    return Err(ClientSessionError::WrongMode);
                }
                let mut operation_id = [0; 16];
                getrandom::fill(&mut operation_id).map_err(|_| {
                    ClientSessionError::Storage(TransactionError::RandomnessUnavailable)
                })?;
                if operation_id == [0; 16] {
                    return Err(ClientSessionError::Storage(
                        TransactionError::RandomnessUnavailable,
                    ));
                }
                self.pending_admin_operation = Some(PendingAdminOperation {
                    operation_id,
                    action: PendingAdminAction::Revoke { token_id },
                });
                self.json_response(
                    request.request_id,
                    format!(
                        "{{\"operation_id\":\"{}\",\"state\":\"prepared\"}}",
                        encode_lower_hex_id(operation_id)
                    )
                    .into_bytes(),
                )
            }
            ClientAdminCommand::PrepareIssue {
                user_id,
                label,
                expires_at_us,
            } => {
                if self.pending_admin_operation.is_some() {
                    return Err(ClientSessionError::WrongMode);
                }
                let now_us = current_unix_micros().ok_or(ClientSessionError::UnsupportedRequest)?;
                if expires_at_us <= now_us {
                    return Err(ClientSessionError::InvalidControlPayload(
                        ClientControlError::InvalidPrepare,
                    ));
                }
                let mut operation_id = [0; 16];
                getrandom::fill(&mut operation_id).map_err(|_| {
                    ClientSessionError::Storage(TransactionError::RandomnessUnavailable)
                })?;
                if operation_id == [0; 16] {
                    return Err(ClientSessionError::Storage(
                        TransactionError::RandomnessUnavailable,
                    ));
                }
                self.pending_admin_operation = Some(PendingAdminOperation {
                    operation_id,
                    action: PendingAdminAction::Issue {
                        user_id,
                        label,
                        expires_at_us,
                    },
                });
                self.json_response(
                    request.request_id,
                    format!(
                        "{{\"operation_id\":\"{}\",\"state\":\"prepared\"}}",
                        encode_lower_hex_id(operation_id)
                    )
                    .into_bytes(),
                )
            }
            ClientAdminCommand::PrepareRotateToken {
                old_token_id,
                label,
                expires_at_us,
            } => {
                if self.pending_admin_operation.is_some() {
                    return Err(ClientSessionError::WrongMode);
                }
                let now_us = current_unix_micros().ok_or(ClientSessionError::UnsupportedRequest)?;
                if expires_at_us <= now_us {
                    return Err(ClientSessionError::InvalidControlPayload(
                        ClientControlError::InvalidPrepare,
                    ));
                }
                let mut operation_id = [0; 16];
                getrandom::fill(&mut operation_id).map_err(|_| {
                    ClientSessionError::Storage(TransactionError::RandomnessUnavailable)
                })?;
                if operation_id == [0; 16] {
                    return Err(ClientSessionError::Storage(
                        TransactionError::RandomnessUnavailable,
                    ));
                }
                self.pending_admin_operation = Some(PendingAdminOperation {
                    operation_id,
                    action: PendingAdminAction::Rotate {
                        old_token_id,
                        label,
                        expires_at_us,
                    },
                });
                self.json_response(
                    request.request_id,
                    format!(
                        "{{\"operation_id\":\"{}\",\"state\":\"prepared\"}}",
                        encode_lower_hex_id(operation_id)
                    )
                    .into_bytes(),
                )
            }
            ClientAdminCommand::Start { operation_id } => {
                let Some(pending) = self.pending_admin_operation.as_ref() else {
                    return Err(ClientSessionError::WrongMode);
                };
                if pending.operation_id != operation_id {
                    return Err(ClientSessionError::WrongMode);
                }
                let pending = self
                    .pending_admin_operation
                    .take()
                    .ok_or(ClientSessionError::WrongMode)?;
                let now_us = current_unix_micros().ok_or(ClientSessionError::UnsupportedRequest)?;
                let mut event_id = [0; 16];
                getrandom::fill(&mut event_id).map_err(|_| {
                    ClientSessionError::Storage(TransactionError::RandomnessUnavailable)
                })?;
                if event_id == [0; 16] {
                    return Err(ClientSessionError::Storage(
                        TransactionError::RandomnessUnavailable,
                    ));
                }
                match pending.action {
                    PendingAdminAction::ChangeRoleMembership {
                        member_id,
                        role_id,
                        grant,
                    } => {
                        let sequence = storage
                            .set_role_membership(
                                operation_id,
                                actor_id,
                                crate::storage_manager::RoleMembershipChange {
                                    member_id,
                                    role_id,
                                    grant,
                                },
                                retention_deadline_us,
                                crate::audit::AuditEvent {
                                    event_id,
                                    actor_id: Some(actor_id),
                                    correlation_id: operation_id,
                                    occurred_at_us: now_us,
                                    action: crate::audit::AuditAction::AuthorityChange,
                                    result: crate::audit::AuditResult::Succeeded,
                                    target_ids: Vec::new(),
                                },
                            )
                            .map_err(ClientSessionError::MembershipLifecycle)?;
                        self.json_response(
                            request.request_id,
                            format!(
                                "{{\"operation_id\":\"{}\",\"outcome\":\"committed\",\"commit_sequence\":\"{sequence}\"}}",
                                encode_lower_hex_id(operation_id)
                            )
                            .into_bytes(),
                        )
                    }
                    PendingAdminAction::RenameIdentity { identity_id, name } => {
                        let sequence = storage
                            .rename_identity(
                                operation_id,
                                actor_id,
                                identity_id,
                                &name,
                                retention_deadline_us,
                                crate::audit::AuditEvent {
                                    event_id,
                                    actor_id: Some(actor_id),
                                    correlation_id: operation_id,
                                    occurred_at_us: now_us,
                                    action: crate::audit::AuditAction::Ddl,
                                    result: crate::audit::AuditResult::Succeeded,
                                    target_ids: Vec::new(),
                                },
                            )
                            .map_err(ClientSessionError::IdentityLifecycle)?;
                        self.json_response(
                            request.request_id,
                            format!(
                                "{{\"operation_id\":\"{}\",\"outcome\":\"committed\",\"commit_sequence\":\"{sequence}\"}}",
                                encode_lower_hex_id(operation_id)
                            )
                            .into_bytes(),
                        )
                    }
                    PendingAdminAction::DropIdentity { identity_id } => {
                        let sequence = storage
                            .drop_identity(
                                operation_id,
                                actor_id,
                                identity_id,
                                retention_deadline_us,
                                crate::audit::AuditEvent {
                                    event_id,
                                    actor_id: Some(actor_id),
                                    correlation_id: operation_id,
                                    occurred_at_us: now_us,
                                    action: crate::audit::AuditAction::Ddl,
                                    result: crate::audit::AuditResult::Succeeded,
                                    target_ids: Vec::new(),
                                },
                            )
                            .map_err(ClientSessionError::IdentityLifecycle)?;
                        self.json_response(
                            request.request_id,
                            format!(
                                "{{\"operation_id\":\"{}\",\"outcome\":\"committed\",\"commit_sequence\":\"{sequence}\"}}",
                                encode_lower_hex_id(operation_id)
                            )
                            .into_bytes(),
                        )
                    }
                    PendingAdminAction::SetUserLogin { user_id, enabled } => {
                        let sequence = storage
                            .set_user_login_enabled(
                                operation_id,
                                actor_id,
                                user_id,
                                enabled,
                                retention_deadline_us,
                                crate::audit::AuditEvent {
                                    event_id,
                                    actor_id: Some(actor_id),
                                    correlation_id: operation_id,
                                    occurred_at_us: now_us,
                                    action: crate::audit::AuditAction::Ddl,
                                    result: crate::audit::AuditResult::Succeeded,
                                    target_ids: Vec::new(),
                                },
                            )
                            .map_err(ClientSessionError::IdentityLifecycle)?;
                        self.json_response(
                            request.request_id,
                            format!(
                                "{{\"operation_id\":\"{}\",\"outcome\":\"committed\",\"commit_sequence\":\"{sequence}\"}}",
                                encode_lower_hex_id(operation_id)
                            )
                            .into_bytes(),
                        )
                    }
                    PendingAdminAction::ChangeSystemPrivilege {
                        grantee_id,
                        privilege,
                        grant,
                    } => {
                        let sequence = storage
                            .set_system_privilege(
                                operation_id,
                                actor_id,
                                crate::storage_manager::SystemPrivilegeChange {
                                    grantee_id,
                                    privilege,
                                    grant,
                                },
                                retention_deadline_us,
                                crate::audit::AuditEvent {
                                    event_id,
                                    actor_id: Some(actor_id),
                                    correlation_id: operation_id,
                                    occurred_at_us: now_us,
                                    action: crate::audit::AuditAction::AuthorityChange,
                                    result: crate::audit::AuditResult::Succeeded,
                                    target_ids: Vec::new(),
                                },
                            )
                            .map_err(ClientSessionError::SystemPrivilegeLifecycle)?;
                        self.json_response(
                            request.request_id,
                            format!(
                                "{{\"operation_id\":\"{}\",\"outcome\":\"committed\",\"commit_sequence\":\"{sequence}\"}}",
                                encode_lower_hex_id(operation_id)
                            )
                            .into_bytes(),
                        )
                    }
                    PendingAdminAction::ChangeObjectGrant {
                        grantee_id,
                        object_id,
                        privileges,
                        target,
                        entries,
                        grant,
                    } => {
                        let request_audit_event = crate::audit::AuditEvent {
                            event_id,
                            actor_id: Some(actor_id),
                            correlation_id: operation_id,
                            occurred_at_us: now_us,
                            action: crate::audit::AuditAction::AuthorityChange,
                            result: crate::audit::AuditResult::Succeeded,
                            target_ids: Vec::new(),
                        };
                        let sequence = if let Some(entries) = entries {
                            storage.set_object_grant_entries_batch(
                                crate::storage_manager::SetObjectGrantEntriesBatchRequest {
                                    invocation_id: operation_id,
                                    authenticated_actor_id: actor_id,
                                    grantee_id,
                                    object_id,
                                    entries,
                                    grant,
                                    retention_deadline_us,
                                    audit_event: request_audit_event,
                                },
                            )
                        } else {
                            storage.set_object_grants_batch(
                                crate::storage_manager::SetObjectGrantBatchRequest {
                                    invocation_id: operation_id,
                                    authenticated_actor_id: actor_id,
                                    grantee_id,
                                    object_id,
                                    privileges,
                                    target,
                                    grant,
                                    retention_deadline_us,
                                    audit_event: request_audit_event,
                                },
                            )
                        }
                        .map_err(ClientSessionError::ObjectGrantLifecycle)?;
                        self.json_response(
                            request.request_id,
                            format!(
                                "{{\"operation_id\":\"{}\",\"outcome\":\"committed\",\"commit_sequence\":\"{sequence}\"}}",
                                encode_lower_hex_id(operation_id)
                            )
                            .into_bytes(),
                        )
                    }
                    PendingAdminAction::CreateSchema { name, owner_id } => {
                        let owner_id = owner_id.unwrap_or(actor_id);
                        let created = storage
                            .create_schema(
                                operation_id,
                                actor_id,
                                &name,
                                owner_id,
                                retention_deadline_us,
                                crate::audit::AuditEvent {
                                    event_id,
                                    actor_id: Some(actor_id),
                                    correlation_id: operation_id,
                                    occurred_at_us: now_us,
                                    action: crate::audit::AuditAction::Ddl,
                                    result: crate::audit::AuditResult::Succeeded,
                                    target_ids: Vec::new(),
                                },
                            )
                            .map_err(ClientSessionError::SchemaLifecycle)?;
                        self.json_response(
                            request.request_id,
                            format!(
                                "{{\"operation_id\":\"{}\",\"outcome\":\"committed\",\"schema_id\":\"{}\",\"commit_sequence\":\"{}\"}}",
                                encode_lower_hex_id(operation_id),
                                encode_lower_hex_id(created.schema_id),
                                created.commit_sequence,
                            )
                            .into_bytes(),
                        )
                    }
                    PendingAdminAction::DropSchema { schema_id } => {
                        let dropped = storage
                            .drop_schema(
                                operation_id,
                                actor_id,
                                schema_id,
                                retention_deadline_us,
                                crate::audit::AuditEvent {
                                    event_id,
                                    actor_id: Some(actor_id),
                                    correlation_id: operation_id,
                                    occurred_at_us: now_us,
                                    action: crate::audit::AuditAction::Ddl,
                                    result: crate::audit::AuditResult::Succeeded,
                                    target_ids: Vec::new(),
                                },
                            )
                            .map_err(ClientSessionError::SchemaLifecycle)?;
                        self.json_response(
                            request.request_id,
                            format!(
                                "{{\"operation_id\":\"{}\",\"outcome\":\"committed\",\"schema_id\":\"{}\",\"commit_sequence\":\"{}\"}}",
                                encode_lower_hex_id(operation_id),
                                encode_lower_hex_id(dropped.schema_id),
                                dropped.commit_sequence,
                            )
                            .into_bytes(),
                        )
                    }
                    PendingAdminAction::SetSchemaOwner {
                        schema_id,
                        owner_id,
                    } => {
                        let commit_sequence = storage
                            .set_schema_owner(
                                operation_id,
                                actor_id,
                                schema_id,
                                owner_id,
                                retention_deadline_us,
                                crate::audit::AuditEvent {
                                    event_id,
                                    actor_id: Some(actor_id),
                                    correlation_id: operation_id,
                                    occurred_at_us: now_us,
                                    action: crate::audit::AuditAction::Ddl,
                                    result: crate::audit::AuditResult::Succeeded,
                                    target_ids: Vec::new(),
                                },
                            )
                            .map_err(ClientSessionError::SchemaLifecycle)?;
                        self.json_response(
                            request.request_id,
                            format!(
                                "{{\"operation_id\":\"{}\",\"outcome\":\"committed\",\"schema_id\":\"{}\",\"owner_id\":\"{}\",\"commit_sequence\":\"{}\"}}",
                                encode_lower_hex_id(operation_id),
                                encode_lower_hex_id(schema_id),
                                encode_lower_hex_id(owner_id),
                                commit_sequence,
                            )
                            .into_bytes(),
                        )
                    }
                    PendingAdminAction::RenameSchema { schema_id, name } => {
                        let commit_sequence = storage
                            .rename_schema(
                                operation_id,
                                actor_id,
                                schema_id,
                                &name,
                                retention_deadline_us,
                                crate::audit::AuditEvent {
                                    event_id,
                                    actor_id: Some(actor_id),
                                    correlation_id: operation_id,
                                    occurred_at_us: now_us,
                                    action: crate::audit::AuditAction::Ddl,
                                    result: crate::audit::AuditResult::Succeeded,
                                    target_ids: Vec::new(),
                                },
                            )
                            .map_err(ClientSessionError::SchemaLifecycle)?;
                        self.json_response(
                            request.request_id,
                            format!(
                                "{{\"operation_id\":\"{}\",\"outcome\":\"committed\",\"schema_id\":\"{}\",\"name\":\"{}\",\"commit_sequence\":\"{}\"}}",
                                encode_lower_hex_id(operation_id),
                                encode_lower_hex_id(schema_id),
                                name,
                                commit_sequence,
                            )
                            .into_bytes(),
                        )
                    }
                    PendingAdminAction::DropTable { table_id } => {
                        let dropped = storage
                            .drop_table(
                                operation_id,
                                actor_id,
                                table_id,
                                retention_deadline_us,
                                crate::audit::AuditEvent {
                                    event_id,
                                    actor_id: Some(actor_id),
                                    correlation_id: operation_id,
                                    occurred_at_us: now_us,
                                    action: crate::audit::AuditAction::Ddl,
                                    result: crate::audit::AuditResult::Succeeded,
                                    target_ids: Vec::new(),
                                },
                            )
                            .map_err(ClientSessionError::TableLifecycle)?;
                        self.json_response(
                            request.request_id,
                            format!(
                                "{{\"operation_id\":\"{}\",\"outcome\":\"committed\",\"table_id\":\"{}\",\"commit_sequence\":\"{}\"}}",
                                encode_lower_hex_id(operation_id),
                                encode_lower_hex_id(dropped.table_id),
                                dropped.commit_sequence,
                            )
                            .into_bytes(),
                        )
                    }
                    PendingAdminAction::TruncateTable { table_id } => {
                        let truncated = storage
                            .truncate_table(
                                operation_id,
                                actor_id,
                                table_id,
                                retention_deadline_us,
                                crate::audit::AuditEvent {
                                    event_id,
                                    actor_id: Some(actor_id),
                                    correlation_id: operation_id,
                                    occurred_at_us: now_us,
                                    action: crate::audit::AuditAction::Ddl,
                                    result: crate::audit::AuditResult::Succeeded,
                                    target_ids: Vec::new(),
                                },
                            )
                            .map_err(ClientSessionError::TableLifecycle)?;
                        self.json_response(
                            request.request_id,
                            format!(
                                "{{\"operation_id\":\"{}\",\"outcome\":\"committed\",\"table_id\":\"{}\",\"commit_sequence\":\"{}\"}}",
                                encode_lower_hex_id(operation_id),
                                encode_lower_hex_id(truncated.table_id),
                                truncated.commit_sequence,
                            )
                            .into_bytes(),
                        )
                    }
                    PendingAdminAction::DropForeignKey { foreign_key_id } => {
                        let commit_sequence = storage
                            .drop_foreign_key(
                                operation_id,
                                actor_id,
                                foreign_key_id,
                                retention_deadline_us,
                                crate::audit::AuditEvent {
                                    event_id,
                                    actor_id: Some(actor_id),
                                    correlation_id: operation_id,
                                    occurred_at_us: now_us,
                                    action: crate::audit::AuditAction::Ddl,
                                    result: crate::audit::AuditResult::Succeeded,
                                    target_ids: Vec::new(),
                                },
                            )
                            .map_err(ClientSessionError::ForeignKeyLifecycle)?;
                        self.json_response(
                            request.request_id,
                            format!(
                                "{{\"operation_id\":\"{}\",\"outcome\":\"committed\",\"foreign_key_id\":\"{}\",\"commit_sequence\":\"{}\"}}",
                                encode_lower_hex_id(operation_id),
                                encode_lower_hex_id(foreign_key_id),
                                commit_sequence,
                            )
                            .into_bytes(),
                        )
                    }
                    PendingAdminAction::DropCheckConstraint { constraint_id } => {
                        let commit_sequence = storage
                            .drop_check_constraint(
                                operation_id,
                                actor_id,
                                constraint_id,
                                retention_deadline_us,
                                crate::audit::AuditEvent {
                                    event_id,
                                    actor_id: Some(actor_id),
                                    correlation_id: operation_id,
                                    occurred_at_us: now_us,
                                    action: crate::audit::AuditAction::Ddl,
                                    result: crate::audit::AuditResult::Succeeded,
                                    target_ids: Vec::new(),
                                },
                            )
                            .map_err(ClientSessionError::ConstraintLifecycle)?;
                        self.json_response(
                            request.request_id,
                            format!(
                                "{{\"operation_id\":\"{}\",\"outcome\":\"committed\",\"constraint_id\":\"{}\",\"commit_sequence\":\"{}\"}}",
                                encode_lower_hex_id(operation_id),
                                encode_lower_hex_id(constraint_id),
                                commit_sequence,
                            )
                            .into_bytes(),
                        )
                    }
                    PendingAdminAction::DropPrimaryKey { constraint_id } => {
                        let commit_sequence = storage
                            .drop_primary_key(
                                operation_id,
                                actor_id,
                                constraint_id,
                                retention_deadline_us,
                                crate::audit::AuditEvent {
                                    event_id,
                                    actor_id: Some(actor_id),
                                    correlation_id: operation_id,
                                    occurred_at_us: now_us,
                                    action: crate::audit::AuditAction::Ddl,
                                    result: crate::audit::AuditResult::Succeeded,
                                    target_ids: Vec::new(),
                                },
                            )
                            .map_err(ClientSessionError::ConstraintLifecycle)?;
                        self.json_response(
                            request.request_id,
                            format!(
                                "{{\"operation_id\":\"{}\",\"outcome\":\"committed\",\"constraint_id\":\"{}\",\"commit_sequence\":\"{}\"}}",
                                encode_lower_hex_id(operation_id),
                                encode_lower_hex_id(constraint_id),
                                commit_sequence,
                            )
                            .into_bytes(),
                        )
                    }
                    PendingAdminAction::DropUniqueConstraint { constraint_id } => {
                        let commit_sequence = storage
                            .drop_unique_constraint(
                                operation_id,
                                actor_id,
                                constraint_id,
                                retention_deadline_us,
                                crate::audit::AuditEvent {
                                    event_id,
                                    actor_id: Some(actor_id),
                                    correlation_id: operation_id,
                                    occurred_at_us: now_us,
                                    action: crate::audit::AuditAction::Ddl,
                                    result: crate::audit::AuditResult::Succeeded,
                                    target_ids: Vec::new(),
                                },
                            )
                            .map_err(ClientSessionError::ConstraintLifecycle)?;
                        self.json_response(
                            request.request_id,
                            format!(
                                "{{\"operation_id\":\"{}\",\"outcome\":\"committed\",\"constraint_id\":\"{}\",\"commit_sequence\":\"{}\"}}",
                                encode_lower_hex_id(operation_id),
                                encode_lower_hex_id(constraint_id),
                                commit_sequence,
                            )
                            .into_bytes(),
                        )
                    }
                    PendingAdminAction::CreateForeignKey {
                        source_table_id,
                        source_column_ids,
                        target_constraint_id,
                        name,
                    } => {
                        let create_request = crate::storage_manager::CreateForeignKeyRequest {
                            invocation_id: operation_id,
                            authenticated_actor_id: actor_id,
                            source_table_id,
                            source_column_ids,
                            target_constraint_id,
                            retention_deadline_us,
                            audit_event: crate::audit::AuditEvent {
                                event_id,
                                actor_id: Some(actor_id),
                                correlation_id: operation_id,
                                occurred_at_us: now_us,
                                action: crate::audit::AuditAction::Ddl,
                                result: crate::audit::AuditResult::Succeeded,
                                target_ids: Vec::new(),
                            },
                        };
                        let created = match name {
                            Some(name) => storage.create_named_foreign_key(create_request, name),
                            None => storage.create_foreign_key(create_request),
                        }
                        .map_err(ClientSessionError::ForeignKeyCreateLifecycle)?;
                        self.json_response(
                            request.request_id,
                            format!(
                                "{{\"operation_id\":\"{}\",\"outcome\":\"committed\",\"foreign_key_id\":\"{}\",\"commit_sequence\":\"{}\"}}",
                                encode_lower_hex_id(operation_id),
                                encode_lower_hex_id(created.foreign_key_id),
                                created.commit_sequence,
                            )
                            .into_bytes(),
                        )
                    }
                    PendingAdminAction::CreateUniqueConstraint {
                        table_id,
                        column_ids,
                        name,
                    } => {
                        let create_request =
                            crate::storage_manager::CreateUniqueConstraintRequest {
                                invocation_id: operation_id,
                                authenticated_actor_id: actor_id,
                                table_id,
                                column_ids,
                                retention_deadline_us,
                                audit_event: crate::audit::AuditEvent {
                                    event_id,
                                    actor_id: Some(actor_id),
                                    correlation_id: operation_id,
                                    occurred_at_us: now_us,
                                    action: crate::audit::AuditAction::Ddl,
                                    result: crate::audit::AuditResult::Succeeded,
                                    target_ids: Vec::new(),
                                },
                            };
                        let created = match name {
                            Some(name) => {
                                storage.create_named_unique_constraint(create_request, name)
                            }
                            None => storage.create_unique_constraint(create_request),
                        }
                        .map_err(ClientSessionError::ConstraintLifecycle)?;
                        self.json_response(
                            request.request_id,
                            format!(
                                "{{\"operation_id\":\"{}\",\"outcome\":\"committed\",\"constraint_id\":\"{}\",\"index_id\":\"{}\",\"commit_sequence\":\"{}\"}}",
                                encode_lower_hex_id(operation_id),
                                encode_lower_hex_id(created.constraint_id),
                                encode_lower_hex_id(created.index_id),
                                created.commit_sequence,
                            )
                            .into_bytes(),
                        )
                    }
                    PendingAdminAction::CreateIndex {
                        table_id,
                        name,
                        column_ids,
                        unique,
                    } => {
                        let created = storage
                            .create_index(crate::storage_manager::CreateIndexRequest {
                                invocation_id: operation_id,
                                authenticated_actor_id: actor_id,
                                table_id,
                                name,
                                column_ids,
                                unique,
                                retention_deadline_us,
                                audit_event: crate::audit::AuditEvent {
                                    event_id,
                                    actor_id: Some(actor_id),
                                    correlation_id: operation_id,
                                    occurred_at_us: now_us,
                                    action: crate::audit::AuditAction::Ddl,
                                    result: crate::audit::AuditResult::Succeeded,
                                    target_ids: Vec::new(),
                                },
                            })
                            .map_err(ClientSessionError::IndexLifecycle)?;
                        self.json_response(
                            request.request_id,
                            format!(
                                "{{\"operation_id\":\"{}\",\"outcome\":\"committed\",\"index_id\":\"{}\",\"commit_sequence\":\"{}\"}}",
                                encode_lower_hex_id(operation_id),
                                encode_lower_hex_id(created.index_id),
                                created.commit_sequence,
                            )
                            .into_bytes(),
                        )
                    }
                    PendingAdminAction::DropIndex { index_id } => {
                        let dropped = storage
                            .drop_index(crate::storage_manager::DropIndexRequest {
                                invocation_id: operation_id,
                                authenticated_actor_id: actor_id,
                                index_id,
                                retention_deadline_us,
                                audit_event: crate::audit::AuditEvent {
                                    event_id,
                                    actor_id: Some(actor_id),
                                    correlation_id: operation_id,
                                    occurred_at_us: now_us,
                                    action: crate::audit::AuditAction::Ddl,
                                    result: crate::audit::AuditResult::Succeeded,
                                    target_ids: Vec::new(),
                                },
                            })
                            .map_err(ClientSessionError::IndexLifecycle)?;
                        self.json_response(
                            request.request_id,
                            format!(
                                "{{\"operation_id\":\"{}\",\"outcome\":\"committed\",\"index_id\":\"{}\",\"commit_sequence\":\"{}\"}}",
                                encode_lower_hex_id(operation_id),
                                encode_lower_hex_id(dropped.index_id),
                                dropped.commit_sequence,
                            )
                            .into_bytes(),
                        )
                    }
                    PendingAdminAction::RebuildIndex {
                        index_id,
                        column_ids,
                        unique,
                    } => {
                        let rebuilt = storage
                            .rebuild_index(crate::storage_manager::RebuildIndexRequest {
                                invocation_id: operation_id,
                                authenticated_actor_id: actor_id,
                                index_id,
                                column_ids,
                                unique,
                                retention_deadline_us,
                                audit_event: crate::audit::AuditEvent {
                                    event_id,
                                    actor_id: Some(actor_id),
                                    correlation_id: operation_id,
                                    occurred_at_us: now_us,
                                    action: crate::audit::AuditAction::Ddl,
                                    result: crate::audit::AuditResult::Succeeded,
                                    target_ids: Vec::new(),
                                },
                            })
                            .map_err(ClientSessionError::IndexLifecycle)?;
                        self.json_response(
                            request.request_id,
                            format!(
                                "{{\"operation_id\":\"{}\",\"outcome\":\"committed\",\"index_id\":\"{}\",\"commit_sequence\":\"{}\"}}",
                                encode_lower_hex_id(operation_id),
                                encode_lower_hex_id(rebuilt.index_id),
                                rebuilt.commit_sequence,
                            )
                            .into_bytes(),
                        )
                    }
                    PendingAdminAction::RenameIndex { index_id, name } => {
                        let renamed = storage
                            .rename_index(crate::storage_manager::RenameIndexRequest {
                                invocation_id: operation_id,
                                authenticated_actor_id: actor_id,
                                index_id,
                                name,
                                retention_deadline_us,
                                audit_event: crate::audit::AuditEvent {
                                    event_id,
                                    actor_id: Some(actor_id),
                                    correlation_id: operation_id,
                                    occurred_at_us: now_us,
                                    action: crate::audit::AuditAction::Ddl,
                                    result: crate::audit::AuditResult::Succeeded,
                                    target_ids: Vec::new(),
                                },
                            })
                            .map_err(ClientSessionError::IndexLifecycle)?;
                        self.json_response(
                            request.request_id,
                            format!(
                                "{{\"operation_id\":\"{}\",\"outcome\":\"committed\",\"index_id\":\"{}\",\"commit_sequence\":\"{}\"}}",
                                encode_lower_hex_id(operation_id),
                                encode_lower_hex_id(renamed.index_id),
                                renamed.commit_sequence,
                            )
                            .into_bytes(),
                        )
                    }
                    PendingAdminAction::CreatePrimaryKey {
                        table_id,
                        column_ids,
                        name,
                    } => {
                        let create_request = crate::storage_manager::CreatePrimaryKeyRequest {
                            invocation_id: operation_id,
                            authenticated_actor_id: actor_id,
                            table_id,
                            column_ids,
                            retention_deadline_us,
                            audit_event: crate::audit::AuditEvent {
                                event_id,
                                actor_id: Some(actor_id),
                                correlation_id: operation_id,
                                occurred_at_us: now_us,
                                action: crate::audit::AuditAction::Ddl,
                                result: crate::audit::AuditResult::Succeeded,
                                target_ids: Vec::new(),
                            },
                        };
                        let created = match name {
                            Some(name) => storage.create_named_primary_key(create_request, name),
                            None => storage.create_primary_key(create_request),
                        }
                        .map_err(ClientSessionError::ConstraintLifecycle)?;
                        self.json_response(
                            request.request_id,
                            format!(
                                "{{\"operation_id\":\"{}\",\"outcome\":\"committed\",\"constraint_id\":\"{}\",\"index_id\":\"{}\",\"commit_sequence\":\"{}\"}}",
                                encode_lower_hex_id(operation_id),
                                encode_lower_hex_id(created.constraint_id),
                                encode_lower_hex_id(created.index_id),
                                created.commit_sequence,
                            )
                            .into_bytes(),
                        )
                    }
                    PendingAdminAction::CreateCheckConstraint {
                        table_id,
                        name,
                        expression,
                        dependency_column_ids,
                    } => {
                        let created = storage
                            .create_check_constraint(
                                crate::storage_manager::CreateCheckConstraintRequest {
                                    invocation_id: operation_id,
                                    authenticated_actor_id: actor_id,
                                    table_id,
                                    name,
                                    expression,
                                    dependency_column_ids,
                                    retention_deadline_us,
                                    audit_event: crate::audit::AuditEvent {
                                        event_id,
                                        actor_id: Some(actor_id),
                                        correlation_id: operation_id,
                                        occurred_at_us: now_us,
                                        action: crate::audit::AuditAction::Ddl,
                                        result: crate::audit::AuditResult::Succeeded,
                                        target_ids: Vec::new(),
                                    },
                                },
                            )
                            .map_err(ClientSessionError::ConstraintLifecycle)?;
                        self.json_response(
                            request.request_id,
                            format!(
                                "{{\"operation_id\":\"{}\",\"outcome\":\"committed\",\"constraint_id\":\"{}\",\"expression_id\":\"{}\",\"commit_sequence\":\"{}\"}}",
                                encode_lower_hex_id(operation_id),
                                encode_lower_hex_id(created.constraint_id),
                                encode_lower_hex_id(created.expression_id),
                                created.commit_sequence,
                            )
                            .into_bytes(),
                        )
                    }
                    PendingAdminAction::CreatePolicy {
                        replaced_policy_id,
                        table_id,
                        name,
                        kind,
                        operation,
                        effect,
                        recipients,
                        target_column_ids,
                        using_expression,
                        with_check_expression,
                    } => {
                        let policy_request = crate::storage_manager::CreatePolicyRequest {
                            invocation_id: operation_id,
                            authenticated_actor_id: actor_id,
                            table_id,
                            name,
                            kind,
                            operation,
                            effect,
                            recipients,
                            target_column_ids,
                            using_expression,
                            with_check_expression,
                            retention_deadline_us,
                            audit_event: crate::audit::AuditEvent {
                                event_id,
                                actor_id: Some(actor_id),
                                correlation_id: operation_id,
                                occurred_at_us: now_us,
                                action: crate::audit::AuditAction::Ddl,
                                result: crate::audit::AuditResult::Succeeded,
                                target_ids: Vec::new(),
                            },
                        };
                        let created = match replaced_policy_id {
                            Some(policy_id) => storage.replace_policy(policy_request, policy_id),
                            None => storage.create_policy(policy_request),
                        }
                        .map_err(ClientSessionError::PolicyLifecycle)?;
                        self.json_response(
                            request.request_id,
                            format!(
                                "{{\"operation_id\":\"{}\",\"outcome\":\"committed\",\"policy_id\":\"{}\",\"commit_sequence\":\"{}\"}}",
                                encode_lower_hex_id(operation_id),
                                encode_lower_hex_id(created.policy_id),
                                created.commit_sequence,
                            )
                            .into_bytes(),
                        )
                    }
                    PendingAdminAction::DropPolicy { policy_id } => {
                        let commit_sequence = storage
                            .drop_policy(crate::storage_manager::DropPolicyRequest {
                                invocation_id: operation_id,
                                authenticated_actor_id: actor_id,
                                policy_id,
                                retention_deadline_us,
                                audit_event: crate::audit::AuditEvent {
                                    event_id,
                                    actor_id: Some(actor_id),
                                    correlation_id: operation_id,
                                    occurred_at_us: now_us,
                                    action: crate::audit::AuditAction::Ddl,
                                    result: crate::audit::AuditResult::Succeeded,
                                    target_ids: Vec::new(),
                                },
                            })
                            .map_err(ClientSessionError::PolicyLifecycle)?;
                        self.json_response(
                            request.request_id,
                            format!(
                                "{{\"operation_id\":\"{}\",\"outcome\":\"committed\",\"policy_id\":\"{}\",\"commit_sequence\":\"{}\"}}",
                                encode_lower_hex_id(operation_id),
                                encode_lower_hex_id(policy_id),
                                commit_sequence,
                            )
                            .into_bytes(),
                        )
                    }
                    PendingAdminAction::RenameTable { table_id, name } => {
                        let commit_sequence = storage
                            .rename_table(
                                operation_id,
                                actor_id,
                                table_id,
                                &name,
                                retention_deadline_us,
                                crate::audit::AuditEvent {
                                    event_id,
                                    actor_id: Some(actor_id),
                                    correlation_id: operation_id,
                                    occurred_at_us: now_us,
                                    action: crate::audit::AuditAction::Ddl,
                                    result: crate::audit::AuditResult::Succeeded,
                                    target_ids: Vec::new(),
                                },
                            )
                            .map_err(ClientSessionError::TableLifecycle)?;
                        self.json_response(
                            request.request_id,
                            format!(
                                "{{\"operation_id\":\"{}\",\"outcome\":\"committed\",\"table_id\":\"{}\",\"commit_sequence\":\"{}\"}}",
                                encode_lower_hex_id(operation_id),
                                encode_lower_hex_id(table_id),
                                commit_sequence,
                            )
                            .into_bytes(),
                        )
                    }
                    PendingAdminAction::SetTableOwner { table_id, owner_id } => {
                        let commit_sequence = storage
                            .set_table_owner(
                                operation_id,
                                actor_id,
                                table_id,
                                owner_id,
                                retention_deadline_us,
                                crate::audit::AuditEvent {
                                    event_id,
                                    actor_id: Some(actor_id),
                                    correlation_id: operation_id,
                                    occurred_at_us: now_us,
                                    action: crate::audit::AuditAction::Ddl,
                                    result: crate::audit::AuditResult::Succeeded,
                                    target_ids: Vec::new(),
                                },
                            )
                            .map_err(ClientSessionError::TableLifecycle)?;
                        self.json_response(
                            request.request_id,
                            format!(
                                "{{\"operation_id\":\"{}\",\"outcome\":\"committed\",\"table_id\":\"{}\",\"owner_id\":\"{}\",\"commit_sequence\":\"{}\"}}",
                                encode_lower_hex_id(operation_id),
                                encode_lower_hex_id(table_id),
                                encode_lower_hex_id(owner_id),
                                commit_sequence,
                            )
                            .into_bytes(),
                        )
                    }
                    PendingAdminAction::RenameColumn {
                        table_id,
                        column_id,
                        name,
                    } => {
                        let commit_sequence = storage
                            .rename_column(
                                operation_id,
                                actor_id,
                                table_id,
                                column_id,
                                &name,
                                retention_deadline_us,
                                crate::audit::AuditEvent {
                                    event_id,
                                    actor_id: Some(actor_id),
                                    correlation_id: operation_id,
                                    occurred_at_us: now_us,
                                    action: crate::audit::AuditAction::Ddl,
                                    result: crate::audit::AuditResult::Succeeded,
                                    target_ids: Vec::new(),
                                },
                            )
                            .map_err(ClientSessionError::TableLifecycle)?;
                        self.json_response(
                            request.request_id,
                            format!(
                                "{{\"operation_id\":\"{}\",\"outcome\":\"committed\",\"table_id\":\"{}\",\"column_id\":\"{}\",\"name\":\"{}\",\"commit_sequence\":\"{}\"}}",
                                encode_lower_hex_id(operation_id),
                                encode_lower_hex_id(table_id),
                                encode_lower_hex_id(column_id),
                                name,
                                commit_sequence,
                            )
                            .into_bytes(),
                        )
                    }
                    PendingAdminAction::SetColumnNullable {
                        table_id,
                        column_id,
                        nullable,
                    } => {
                        let commit_sequence = storage
                            .set_column_nullable(
                                operation_id,
                                actor_id,
                                table_id,
                                column_id,
                                nullable,
                                retention_deadline_us,
                                crate::audit::AuditEvent {
                                    event_id,
                                    actor_id: Some(actor_id),
                                    correlation_id: operation_id,
                                    occurred_at_us: now_us,
                                    action: crate::audit::AuditAction::Ddl,
                                    result: crate::audit::AuditResult::Succeeded,
                                    target_ids: Vec::new(),
                                },
                            )
                            .map_err(ClientSessionError::TableLifecycle)?;
                        self.json_response(
                            request.request_id,
                            format!(
                                "{{\"operation_id\":\"{}\",\"outcome\":\"committed\",\"table_id\":\"{}\",\"column_id\":\"{}\",\"nullable\":{},\"commit_sequence\":\"{}\"}}",
                                encode_lower_hex_id(operation_id),
                                encode_lower_hex_id(table_id),
                                encode_lower_hex_id(column_id),
                                nullable,
                                commit_sequence,
                            )
                            .into_bytes(),
                        )
                    }
                    PendingAdminAction::WidenVarcharColumn {
                        table_id,
                        column_id,
                        max_chars,
                    } => {
                        let commit_sequence = storage
                            .widen_varchar_column(
                                operation_id,
                                actor_id,
                                table_id,
                                column_id,
                                max_chars,
                                retention_deadline_us,
                                crate::audit::AuditEvent {
                                    event_id,
                                    actor_id: Some(actor_id),
                                    correlation_id: operation_id,
                                    occurred_at_us: now_us,
                                    action: crate::audit::AuditAction::Ddl,
                                    result: crate::audit::AuditResult::Succeeded,
                                    target_ids: Vec::new(),
                                },
                            )
                            .map_err(ClientSessionError::TableLifecycle)?;
                        self.json_response(
                            request.request_id,
                            format!(
                                "{{\"operation_id\":\"{}\",\"outcome\":\"committed\",\"table_id\":\"{}\",\"column_id\":\"{}\",\"max_chars\":\"{}\",\"commit_sequence\":\"{}\"}}",
                                encode_lower_hex_id(operation_id),
                                encode_lower_hex_id(table_id),
                                encode_lower_hex_id(column_id),
                                max_chars,
                                commit_sequence,
                            )
                            .into_bytes(),
                        )
                    }
                    PendingAdminAction::AlterColumnDefault {
                        table_id,
                        column_id,
                        default,
                    } => {
                        let commit_sequence = storage
                            .alter_column_default(
                                crate::storage_manager::AlterColumnDefaultRequest {
                                    invocation_id: operation_id,
                                    authenticated_actor_id: actor_id,
                                    table_id,
                                    column_id,
                                    default,
                                    retention_deadline_us,
                                    audit_event: crate::audit::AuditEvent {
                                        event_id,
                                        actor_id: Some(actor_id),
                                        correlation_id: operation_id,
                                        occurred_at_us: now_us,
                                        action: crate::audit::AuditAction::Ddl,
                                        result: crate::audit::AuditResult::Succeeded,
                                        target_ids: Vec::new(),
                                    },
                                },
                            )
                            .map_err(ClientSessionError::TableLifecycle)?;
                        self.json_response(
                            request.request_id,
                            format!(
                                "{{\"operation_id\":\"{}\",\"outcome\":\"committed\",\"table_id\":\"{}\",\"column_id\":\"{}\",\"commit_sequence\":\"{}\"}}",
                                encode_lower_hex_id(operation_id),
                                encode_lower_hex_id(table_id),
                                encode_lower_hex_id(column_id),
                                commit_sequence,
                            )
                            .into_bytes(),
                        )
                    }
                    PendingAdminAction::AddColumn { table_id, column } => {
                        let added = storage
                            .add_table_column(crate::storage_manager::AddTableColumnRequest {
                                invocation_id: operation_id,
                                authenticated_actor_id: actor_id,
                                table_id,
                                column,
                                retention_deadline_us,
                                audit_event: crate::audit::AuditEvent {
                                    event_id,
                                    actor_id: Some(actor_id),
                                    correlation_id: operation_id,
                                    occurred_at_us: now_us,
                                    action: crate::audit::AuditAction::Ddl,
                                    result: crate::audit::AuditResult::Succeeded,
                                    target_ids: Vec::new(),
                                },
                            })
                            .map_err(ClientSessionError::TableLifecycle)?;
                        self.json_response(
                            request.request_id,
                            format!(
                                "{{\"operation_id\":\"{}\",\"outcome\":\"committed\",\"table_id\":\"{}\",\"column_id\":\"{}\",\"schema_version\":\"{}\",\"commit_sequence\":\"{}\"}}",
                                encode_lower_hex_id(operation_id),
                                encode_lower_hex_id(added.table_id),
                                encode_lower_hex_id(added.column_id),
                                added.schema_version,
                                added.commit_sequence,
                            )
                            .into_bytes(),
                        )
                    }
                    PendingAdminAction::DropColumn {
                        table_id,
                        column_id,
                    } => {
                        let dropped = storage
                            .drop_column(
                                operation_id,
                                actor_id,
                                table_id,
                                column_id,
                                retention_deadline_us,
                                crate::audit::AuditEvent {
                                    event_id,
                                    actor_id: Some(actor_id),
                                    correlation_id: operation_id,
                                    occurred_at_us: now_us,
                                    action: crate::audit::AuditAction::Ddl,
                                    result: crate::audit::AuditResult::Succeeded,
                                    target_ids: Vec::new(),
                                },
                            )
                            .map_err(ClientSessionError::TableLifecycle)?;
                        self.json_response(
                            request.request_id,
                            format!(
                                "{{\"operation_id\":\"{}\",\"outcome\":\"committed\",\"table_id\":\"{}\",\"column_id\":\"{}\",\"commit_sequence\":\"{}\"}}",
                                encode_lower_hex_id(operation_id),
                                encode_lower_hex_id(dropped.table_id),
                                encode_lower_hex_id(dropped.column_id),
                                dropped.commit_sequence,
                            )
                            .into_bytes(),
                        )
                    }
                    PendingAdminAction::CreateTable {
                        schema_id,
                        name,
                        columns,
                        primary_key_columns,
                        primary_key_name,
                        unique_constraints,
                        unique_constraint_names,
                        check_expressions,
                        check_constraint_names,
                        foreign_keys,
                        foreign_key_names,
                    } => {
                        let create_request = crate::storage_manager::CreateTableRequest {
                            invocation_id: operation_id,
                            authenticated_actor_id: actor_id,
                            schema_id,
                            name,
                            columns,
                            retention_deadline_us,
                            audit_event: crate::audit::AuditEvent {
                                event_id,
                                actor_id: Some(actor_id),
                                correlation_id: operation_id,
                                occurred_at_us: now_us,
                                action: crate::audit::AuditAction::Ddl,
                                result: crate::audit::AuditResult::Succeeded,
                                target_ids: Vec::new(),
                            },
                        };
                        let created = storage
                            .create_table_with_named_constraints_and_foreign_keys(
                                create_request,
                                primary_key_columns.as_deref(),
                                primary_key_name,
                                &unique_constraints,
                                &unique_constraint_names,
                                &check_expressions,
                                &check_constraint_names,
                                &foreign_keys,
                                &foreign_key_names,
                            )
                            .map_err(ClientSessionError::TableLifecycle)?;
                        let mut payload = format!(
                            "{{\"operation_id\":\"{}\",\"outcome\":\"committed\",\"table_id\":\"{}\",\"column_ids\":[",
                            encode_lower_hex_id(operation_id),
                            encode_lower_hex_id(created.table_id),
                        );
                        for (index, column_id) in created.column_ids.iter().enumerate() {
                            if index != 0 {
                                payload.push(',');
                            }
                            payload.push('"');
                            payload.push_str(&encode_lower_hex_id(*column_id));
                            payload.push('"');
                        }
                        payload.push_str(&format!(
                            "],\"commit_sequence\":\"{}\"}}",
                            created.commit_sequence
                        ));
                        if payload.len() > CLIENT_MAX_CONTROL_BYTES {
                            return Err(ClientSessionError::ResourceLimit);
                        }
                        self.json_response(request.request_id, payload.into_bytes())
                    }
                    PendingAdminAction::CreateRole { name } => {
                        let created = storage
                            .create_role(
                                operation_id,
                                actor_id,
                                &name,
                                retention_deadline_us,
                                crate::audit::AuditEvent {
                                    event_id,
                                    actor_id: Some(actor_id),
                                    correlation_id: operation_id,
                                    occurred_at_us: now_us,
                                    action: crate::audit::AuditAction::Ddl,
                                    result: crate::audit::AuditResult::Succeeded,
                                    target_ids: Vec::new(),
                                },
                            )
                            .map_err(ClientSessionError::IdentityLifecycle)?;
                        self.json_response(
                            request.request_id,
                            format!(
                                "{{\"operation_id\":\"{}\",\"outcome\":\"committed\",\"role_id\":\"{}\",\"commit_sequence\":\"{}\"}}",
                                encode_lower_hex_id(operation_id),
                                encode_lower_hex_id(created.role_id),
                                created.commit_sequence,
                            )
                            .into_bytes(),
                        )
                    }
                    PendingAdminAction::CreateUser { name } => {
                        let created = storage
                            .create_user(
                                operation_id,
                                actor_id,
                                &name,
                                retention_deadline_us,
                                crate::audit::AuditEvent {
                                    event_id,
                                    actor_id: Some(actor_id),
                                    correlation_id: operation_id,
                                    occurred_at_us: now_us,
                                    action: crate::audit::AuditAction::Ddl,
                                    result: crate::audit::AuditResult::Succeeded,
                                    target_ids: Vec::new(),
                                },
                            )
                            .map_err(ClientSessionError::IdentityLifecycle)?;
                        self.json_response(
                            request.request_id,
                            format!(
                                "{{\"operation_id\":\"{}\",\"outcome\":\"committed\",\"user_id\":\"{}\",\"commit_sequence\":\"{}\"}}",
                                encode_lower_hex_id(operation_id),
                                encode_lower_hex_id(created.user_id),
                                created.commit_sequence,
                            )
                            .into_bytes(),
                        )
                    }
                    PendingAdminAction::CreateUserNoLogin { name } => {
                        let created = storage
                            .create_user_no_login(
                                operation_id,
                                actor_id,
                                &name,
                                retention_deadline_us,
                                crate::audit::AuditEvent {
                                    event_id,
                                    actor_id: Some(actor_id),
                                    correlation_id: operation_id,
                                    occurred_at_us: now_us,
                                    action: crate::audit::AuditAction::Ddl,
                                    result: crate::audit::AuditResult::Succeeded,
                                    target_ids: Vec::new(),
                                },
                            )
                            .map_err(ClientSessionError::IdentityLifecycle)?;
                        self.json_response(
                            request.request_id,
                            format!(
                                "{{\"operation_id\":\"{}\",\"outcome\":\"committed\",\"user_id\":\"{}\",\"commit_sequence\":\"{}\"}}",
                                encode_lower_hex_id(operation_id),
                                encode_lower_hex_id(created.user_id),
                                created.commit_sequence,
                            )
                            .into_bytes(),
                        )
                    }
                    PendingAdminAction::Revoke { token_id } => {
                        let sequence = storage
                            .revoke_token(
                                operation_id,
                                actor_id,
                                token_id,
                                retention_deadline_us,
                                crate::audit::AuditEvent {
                                    event_id,
                                    actor_id: Some(actor_id),
                                    correlation_id: operation_id,
                                    occurred_at_us: now_us,
                                    action: crate::audit::AuditAction::TokenLifecycle,
                                    result: crate::audit::AuditResult::Succeeded,
                                    target_ids: vec![token_id],
                                },
                            )
                            .map_err(ClientSessionError::TokenInventory)?;
                        self.json_response(
                            request.request_id,
                            format!(
                                "{{\"operation_id\":\"{}\",\"outcome\":\"committed\",\"commit_sequence\":\"{sequence}\"}}",
                                encode_lower_hex_id(operation_id)
                            )
                            .into_bytes(),
                        )
                    }
                    PendingAdminAction::Issue {
                        user_id,
                        label,
                        expires_at_us,
                    } => {
                        let mut issued = storage
                            .issue_token(crate::storage_manager::TokenIssueRequest {
                                invocation_id: operation_id,
                                authenticated_actor_id: actor_id,
                                user_id,
                                label: &label,
                                expires_at_us,
                                retention_deadline_us,
                                audit_event: crate::audit::AuditEvent {
                                    event_id,
                                    actor_id: Some(actor_id),
                                    correlation_id: operation_id,
                                    occurred_at_us: now_us,
                                    action: crate::audit::AuditAction::TokenLifecycle,
                                    result: crate::audit::AuditResult::Succeeded,
                                    target_ids: vec![user_id],
                                },
                            })
                            .map_err(ClientSessionError::TokenInventory)?;
                        let secret = issued
                            .take_secret()
                            .ok_or(ClientSessionError::UnsupportedRequest)?;
                        let mut secret = Zeroizing::new(secret);
                        let secret_base64 = Zeroizing::new(encode_base64(secret.as_ref()));
                        secret.zeroize();
                        let mut payload = Zeroizing::new(format!(
                            "{{\"operation_id\":\"{}\",\"outcome\":\"committed\",\"token_id\":\"{}\",\"token_secret\":\"{}\",\"commit_sequence\":\"{}\"}}",
                            encode_lower_hex_id(operation_id),
                            encode_lower_hex_id(issued.token_id),
                            secret_base64.as_str(),
                            issued.commit_sequence,
                        ));
                        let response_payload = payload.as_bytes().to_vec();
                        payload.zeroize();
                        self.json_response(request.request_id, response_payload)
                    }
                    PendingAdminAction::Rotate {
                        old_token_id,
                        label,
                        expires_at_us,
                    } => {
                        let mut issued = storage
                            .rotate_token(crate::storage_manager::TokenRotationRequest {
                                invocation_id: operation_id,
                                authenticated_actor_id: actor_id,
                                old_token_id,
                                label: &label,
                                expires_at_us,
                                retention_deadline_us,
                                audit_event: crate::audit::AuditEvent {
                                    event_id,
                                    actor_id: Some(actor_id),
                                    correlation_id: operation_id,
                                    occurred_at_us: now_us,
                                    action: crate::audit::AuditAction::TokenLifecycle,
                                    result: crate::audit::AuditResult::Succeeded,
                                    target_ids: vec![old_token_id],
                                },
                            })
                            .map_err(ClientSessionError::TokenInventory)?;
                        let secret = issued
                            .take_secret()
                            .ok_or(ClientSessionError::UnsupportedRequest)?;
                        let mut secret = Zeroizing::new(secret);
                        let secret_base64 = Zeroizing::new(encode_base64(secret.as_ref()));
                        secret.zeroize();
                        let mut payload = Zeroizing::new(format!(
                            "{{\"operation_id\":\"{}\",\"outcome\":\"committed\",\"token_id\":\"{}\",\"token_secret\":\"{}\",\"commit_sequence\":\"{}\"}}",
                            encode_lower_hex_id(operation_id),
                            encode_lower_hex_id(issued.token_id),
                            secret_base64.as_str(),
                            issued.commit_sequence
                        ));
                        let response_payload = payload.as_bytes().to_vec();
                        payload.zeroize();
                        self.json_response(request.request_id, response_payload)
                    }
                }
            }
        }
    }

    fn prepare_role_membership(
        &mut self,
        request_id: u64,
        member_id: [u8; 16],
        role_id: [u8; 16],
        grant: bool,
    ) -> Result<ClientFrame, ClientSessionError> {
        if self.pending_admin_operation.is_some() {
            return Err(ClientSessionError::WrongMode);
        }
        let mut operation_id = [0; 16];
        getrandom::fill(&mut operation_id)
            .map_err(|_| ClientSessionError::Storage(TransactionError::RandomnessUnavailable))?;
        if operation_id == [0; 16] {
            return Err(ClientSessionError::Storage(
                TransactionError::RandomnessUnavailable,
            ));
        }
        let action = PendingAdminAction::ChangeRoleMembership {
            member_id,
            role_id,
            grant,
        };
        self.pending_admin_operation = Some(PendingAdminOperation {
            operation_id,
            action,
        });
        self.json_response(
            request_id,
            format!(
                "{{\"operation_id\":\"{}\",\"state\":\"prepared\"}}",
                encode_lower_hex_id(operation_id)
            )
            .into_bytes(),
        )
    }

    fn prepare_system_privilege(
        &mut self,
        request_id: u64,
        grantee_id: [u8; 16],
        privilege: crate::storage_manager::AdministrativeSystemPrivilege,
        grant: bool,
    ) -> Result<ClientFrame, ClientSessionError> {
        if self.pending_admin_operation.is_some() {
            return Err(ClientSessionError::WrongMode);
        }
        let mut operation_id = [0; 16];
        getrandom::fill(&mut operation_id)
            .map_err(|_| ClientSessionError::Storage(TransactionError::RandomnessUnavailable))?;
        if operation_id == [0; 16] {
            return Err(ClientSessionError::Storage(
                TransactionError::RandomnessUnavailable,
            ));
        }
        self.pending_admin_operation = Some(PendingAdminOperation {
            operation_id,
            action: PendingAdminAction::ChangeSystemPrivilege {
                grantee_id,
                privilege,
                grant,
            },
        });
        self.json_response(
            request_id,
            format!(
                "{{\"operation_id\":\"{}\",\"state\":\"prepared\"}}",
                encode_lower_hex_id(operation_id)
            )
            .into_bytes(),
        )
    }

    #[allow(clippy::too_many_arguments)] // Preserve the decoded ADMIN request fields without an intermediate allocation.
    fn prepare_object_grant(
        &mut self,
        request_id: u64,
        grantee_id: [u8; 16],
        object_id: [u8; 16],
        privileges: Vec<crate::storage_manager::ObjectGrantPrivilege>,
        target: crate::storage_manager::ObjectGrantTarget,
        entries: Option<Vec<crate::storage_manager::ObjectGrantEntry>>,
        grant: bool,
    ) -> Result<ClientFrame, ClientSessionError> {
        if self.pending_admin_operation.is_some() {
            return Err(ClientSessionError::WrongMode);
        }
        let mut operation_id = [0; 16];
        getrandom::fill(&mut operation_id)
            .map_err(|_| ClientSessionError::Storage(TransactionError::RandomnessUnavailable))?;
        if operation_id == [0; 16] {
            return Err(ClientSessionError::Storage(
                TransactionError::RandomnessUnavailable,
            ));
        }
        self.pending_admin_operation = Some(PendingAdminOperation {
            operation_id,
            action: PendingAdminAction::ChangeObjectGrant {
                grantee_id,
                object_id,
                privileges,
                target,
                entries,
                grant,
            },
        });
        self.json_response(
            request_id,
            format!(
                "{{\"operation_id\":\"{}\",\"state\":\"prepared\"}}",
                encode_lower_hex_id(operation_id)
            )
            .into_bytes(),
        )
    }

    fn prepare_identity_rename(
        &mut self,
        request_id: u64,
        identity_id: [u8; 16],
        name: String,
    ) -> Result<ClientFrame, ClientSessionError> {
        if self.pending_admin_operation.is_some() {
            return Err(ClientSessionError::WrongMode);
        }
        let mut operation_id = [0; 16];
        getrandom::fill(&mut operation_id)
            .map_err(|_| ClientSessionError::Storage(TransactionError::RandomnessUnavailable))?;
        if operation_id == [0; 16] {
            return Err(ClientSessionError::Storage(
                TransactionError::RandomnessUnavailable,
            ));
        }
        self.pending_admin_operation = Some(PendingAdminOperation {
            operation_id,
            action: PendingAdminAction::RenameIdentity { identity_id, name },
        });
        self.json_response(
            request_id,
            format!(
                "{{\"operation_id\":\"{}\",\"state\":\"prepared\"}}",
                encode_lower_hex_id(operation_id)
            )
            .into_bytes(),
        )
    }

    fn prepare_user_login(
        &mut self,
        request_id: u64,
        user_id: [u8; 16],
        enabled: bool,
    ) -> Result<ClientFrame, ClientSessionError> {
        if self.pending_admin_operation.is_some() {
            return Err(ClientSessionError::WrongMode);
        }
        let mut operation_id = [0; 16];
        getrandom::fill(&mut operation_id)
            .map_err(|_| ClientSessionError::Storage(TransactionError::RandomnessUnavailable))?;
        if operation_id == [0; 16] {
            return Err(ClientSessionError::Storage(
                TransactionError::RandomnessUnavailable,
            ));
        }
        self.pending_admin_operation = Some(PendingAdminOperation {
            operation_id,
            action: PendingAdminAction::SetUserLogin { user_id, enabled },
        });
        self.json_response(
            request_id,
            format!(
                "{{\"operation_id\":\"{}\",\"state\":\"prepared\"}}",
                encode_lower_hex_id(operation_id)
            )
            .into_bytes(),
        )
    }

    fn prepare_identity_drop(
        &mut self,
        request_id: u64,
        identity_id: [u8; 16],
    ) -> Result<ClientFrame, ClientSessionError> {
        if self.pending_admin_operation.is_some() {
            return Err(ClientSessionError::WrongMode);
        }
        let mut operation_id = [0; 16];
        getrandom::fill(&mut operation_id)
            .map_err(|_| ClientSessionError::Storage(TransactionError::RandomnessUnavailable))?;
        if operation_id == [0; 16] {
            return Err(ClientSessionError::Storage(
                TransactionError::RandomnessUnavailable,
            ));
        }
        self.pending_admin_operation = Some(PendingAdminOperation {
            operation_id,
            action: PendingAdminAction::DropIdentity { identity_id },
        });
        self.json_response(
            request_id,
            format!(
                "{{\"operation_id\":\"{}\",\"state\":\"prepared\"}}",
                encode_lower_hex_id(operation_id)
            )
            .into_bytes(),
        )
    }

    /// Cancel an unstarted reservation without creating a storage outcome.
    /// Started invocations require the supervisor's cancellation service.
    fn handle_sdk_library_page(
        &mut self,
        staging: Option<&crate::supervisor::StagingStore>,
        request_id: u64,
        page_limit: usize,
        cursor: Option<[u8; 16]>,
    ) -> Result<ClientFrame, ClientSessionError> {
        if !(1..=CLIENT_METADATA_MAX_PAGE).contains(&page_limit) {
            return Err(ClientSessionError::InvalidControlPayload(
                ClientControlError::InvalidMetadataPageLimit,
            ));
        }
        let repository = staging
            .and_then(crate::supervisor::StagingStore::sdk_repository)
            .ok_or(ClientSessionError::SdkLibraryRepositoryUnavailable)?;
        let session = self
            .authenticated
            .as_ref()
            .ok_or(ClientSessionError::AuthenticationRequired)?;
        let principal_id = session.principal_id();
        let role_ids = session.role_ids().clone();

        let mut snapshot = if let Some(cursor) = cursor {
            let current = self.sdk_library_page_snapshot.as_ref();
            let invalid = current.is_none_or(|snapshot| {
                snapshot.expires_at <= std::time::Instant::now()
                    || snapshot.principal_id != principal_id
                    || snapshot.role_ids != role_ids
                    || snapshot.current_cursor != Some(cursor)
                    || snapshot.page_limit != page_limit
            });
            if invalid {
                self.sdk_library_page_snapshot = None;
                return Err(ClientSessionError::InvalidControlPayload(
                    ClientControlError::InvalidMetadataCursor,
                ));
            }
            let current_releases = repository
                .list()
                .map_err(ClientSessionError::SdkLibraryRepository)?;
            if !current.is_some_and(|snapshot| snapshot.releases == current_releases) {
                self.sdk_library_page_snapshot = None;
                return Err(ClientSessionError::InvalidControlPayload(
                    ClientControlError::InvalidMetadataCursor,
                ));
            }
            self.sdk_library_page_snapshot.take().ok_or(
                ClientSessionError::InvalidControlPayload(
                    ClientControlError::InvalidMetadataCursor,
                ),
            )?
        } else {
            self.sdk_library_page_snapshot = None;
            ActiveSdkLibraryPageSnapshot {
                releases: repository
                    .list()
                    .map_err(ClientSessionError::SdkLibraryRepository)?,
                principal_id,
                role_ids,
                page_limit,
                next_index: 0,
                current_cursor: None,
                expires_at: std::time::Instant::now() + std::time::Duration::from_secs(60),
            }
        };

        let start = snapshot.next_index;
        let end = start
            .saturating_add(snapshot.page_limit)
            .min(snapshot.releases.len());
        let next_cursor = if end < snapshot.releases.len() {
            let mut token = [0; 16];
            loop {
                getrandom::fill(&mut token).map_err(|_| {
                    ClientSessionError::Storage(TransactionError::RandomnessUnavailable)
                })?;
                token[0] = CLIENT_METADATA_CURSOR_KIND;
                if Some(token) != cursor {
                    break;
                }
            }
            Some(token)
        } else {
            None
        };
        let payload = encode_sdk_library_page(&snapshot.releases[start..end], next_cursor);
        if payload.len() > CLIENT_MAX_CONTROL_BYTES {
            return Err(ClientSessionError::ResourceLimit);
        }
        if let Some(next_cursor) = next_cursor {
            snapshot.next_index = end;
            snapshot.current_cursor = Some(next_cursor);
            snapshot.expires_at = std::time::Instant::now() + std::time::Duration::from_secs(60);
            self.sdk_library_page_snapshot = Some(snapshot);
        } else {
            self.sdk_library_page_snapshot = None;
        }
        self.json_response(request_id, payload.into_bytes())
    }

    fn handle_metadata_discovery_page(
        &mut self,
        storage: &StorageManager<'_>,
        request_id: u64,
        query: MetadataPageQuery,
        page_limit: usize,
        cursor: Option<[u8; 16]>,
    ) -> Result<ClientFrame, ClientSessionError> {
        if !(1..=CLIENT_METADATA_MAX_PAGE).contains(&page_limit) {
            return Err(ClientSessionError::InvalidControlPayload(
                ClientControlError::InvalidMetadataPageLimit,
            ));
        }
        let session = self
            .authenticated
            .as_ref()
            .ok_or(ClientSessionError::AuthenticationRequired)?;
        let token_id = session.token_id();
        let principal_id = session.principal_id();
        let role_ids = session.role_ids().clone();
        let secret = Zeroizing::new(
            *self
                .token_secret
                .as_ref()
                .ok_or(ClientSessionError::AuthenticationRequired)?,
        );

        let mut snapshot = if let Some(cursor) = cursor {
            let current_snapshot = self.metadata_page_snapshot.as_ref();
            let expired = current_snapshot
                .is_some_and(|snapshot| snapshot.expires_at <= std::time::Instant::now());
            let identity_changed = current_snapshot.is_some_and(|snapshot| {
                snapshot.principal_id != principal_id || snapshot.role_ids != role_ids
            });
            if expired || identity_changed {
                self.metadata_page_snapshot = None;
                return Err(ClientSessionError::InvalidControlPayload(
                    ClientControlError::InvalidMetadataCursor,
                ));
            }
            let valid_cursor = current_snapshot.is_some_and(|snapshot| {
                snapshot.current_cursor == Some(cursor)
                    && snapshot.query == query
                    && snapshot.page_limit == page_limit
            });
            if !valid_cursor {
                return Err(ClientSessionError::InvalidControlPayload(
                    ClientControlError::InvalidMetadataCursor,
                ));
            }
            let current_rows = match &query {
                MetadataPageQuery::Schemas => MetadataPageRows::Schemas(
                    storage
                        .discover_token_authorized_schemas(token_id, &secret)
                        .map_err(ClientSessionError::Storage)?,
                ),
                MetadataPageQuery::Relations(schema_id) => MetadataPageRows::Relations(
                    storage
                        .discover_token_authorized_relations(token_id, &secret, *schema_id)
                        .map_err(ClientSessionError::Storage)?
                        .unwrap_or_default(),
                ),
                MetadataPageQuery::Columns {
                    schema_id,
                    relation_id,
                } => MetadataPageRows::Columns(
                    storage
                        .discover_token_authorized_columns(
                            token_id,
                            &secret,
                            *schema_id,
                            *relation_id,
                        )
                        .map_err(ClientSessionError::Storage)?
                        .unwrap_or_default(),
                ),
                MetadataPageQuery::Indexes {
                    schema_id,
                    relation_id,
                } => MetadataPageRows::Indexes(
                    storage
                        .discover_token_administrative_indexes(
                            token_id,
                            &secret,
                            *schema_id,
                            *relation_id,
                        )
                        .map_err(ClientSessionError::Storage)?
                        .unwrap_or_default(),
                ),
                MetadataPageQuery::Constraints {
                    schema_id,
                    relation_id,
                } => MetadataPageRows::Constraints(
                    storage
                        .discover_token_administrative_constraints(
                            token_id,
                            &secret,
                            *schema_id,
                            *relation_id,
                        )
                        .map_err(ClientSessionError::Storage)?
                        .unwrap_or_default(),
                ),
                MetadataPageQuery::Grants {
                    schema_id,
                    relation_id,
                } => MetadataPageRows::Grants(
                    storage
                        .discover_token_administrative_grants(
                            token_id,
                            &secret,
                            *schema_id,
                            *relation_id,
                        )
                        .map_err(ClientSessionError::Storage)?
                        .unwrap_or_default(),
                ),
                MetadataPageQuery::Policies {
                    schema_id,
                    relation_id,
                } => MetadataPageRows::Policies(
                    storage
                        .discover_token_administrative_policies(
                            token_id,
                            &secret,
                            *schema_id,
                            *relation_id,
                        )
                        .map_err(ClientSessionError::Storage)?
                        .unwrap_or_default(),
                ),
                MetadataPageQuery::Dependencies(object_id) => MetadataPageRows::Dependencies(
                    storage
                        .discover_token_administrative_dependencies(token_id, &secret, *object_id)
                        .map_err(ClientSessionError::Storage)?
                        .unwrap_or_default(),
                ),
            };
            if !current_snapshot
                .is_some_and(|snapshot| snapshot.rows.has_same_object_ids(&current_rows))
            {
                self.metadata_page_snapshot = None;
                return Err(ClientSessionError::InvalidControlPayload(
                    ClientControlError::InvalidMetadataCursor,
                ));
            }
            self.metadata_page_snapshot
                .take()
                .ok_or(ClientSessionError::InvalidControlPayload(
                    ClientControlError::InvalidMetadataCursor,
                ))?
        } else {
            self.metadata_page_snapshot = None;
            let rows = match &query {
                MetadataPageQuery::Schemas => MetadataPageRows::Schemas(
                    storage
                        .discover_token_authorized_schemas(token_id, &secret)
                        .map_err(ClientSessionError::Storage)?,
                ),
                MetadataPageQuery::Relations(schema_id) => MetadataPageRows::Relations(
                    storage
                        .discover_token_authorized_relations(token_id, &secret, *schema_id)
                        .map_err(ClientSessionError::Storage)?
                        .unwrap_or_default(),
                ),
                MetadataPageQuery::Columns {
                    schema_id,
                    relation_id,
                } => MetadataPageRows::Columns(
                    storage
                        .discover_token_authorized_columns(
                            token_id,
                            &secret,
                            *schema_id,
                            *relation_id,
                        )
                        .map_err(ClientSessionError::Storage)?
                        .unwrap_or_default(),
                ),
                MetadataPageQuery::Indexes {
                    schema_id,
                    relation_id,
                } => MetadataPageRows::Indexes(
                    storage
                        .discover_token_administrative_indexes(
                            token_id,
                            &secret,
                            *schema_id,
                            *relation_id,
                        )
                        .map_err(ClientSessionError::Storage)?
                        .unwrap_or_default(),
                ),
                MetadataPageQuery::Constraints {
                    schema_id,
                    relation_id,
                } => MetadataPageRows::Constraints(
                    storage
                        .discover_token_administrative_constraints(
                            token_id,
                            &secret,
                            *schema_id,
                            *relation_id,
                        )
                        .map_err(ClientSessionError::Storage)?
                        .unwrap_or_default(),
                ),
                MetadataPageQuery::Grants {
                    schema_id,
                    relation_id,
                } => MetadataPageRows::Grants(
                    storage
                        .discover_token_administrative_grants(
                            token_id,
                            &secret,
                            *schema_id,
                            *relation_id,
                        )
                        .map_err(ClientSessionError::Storage)?
                        .unwrap_or_default(),
                ),
                MetadataPageQuery::Policies {
                    schema_id,
                    relation_id,
                } => MetadataPageRows::Policies(
                    storage
                        .discover_token_administrative_policies(
                            token_id,
                            &secret,
                            *schema_id,
                            *relation_id,
                        )
                        .map_err(ClientSessionError::Storage)?
                        .unwrap_or_default(),
                ),
                MetadataPageQuery::Dependencies(object_id) => MetadataPageRows::Dependencies(
                    storage
                        .discover_token_administrative_dependencies(token_id, &secret, *object_id)
                        .map_err(ClientSessionError::Storage)?
                        .unwrap_or_default(),
                ),
            };
            ActiveMetadataPageSnapshot {
                query,
                rows,
                principal_id,
                role_ids,
                page_limit,
                next_index: 0,
                current_cursor: None,
                expires_at: std::time::Instant::now() + std::time::Duration::from_secs(60),
            }
        };

        let start = snapshot.next_index;
        let end = start
            .saturating_add(snapshot.page_limit)
            .min(snapshot.rows.len());
        let next_cursor = if end < snapshot.rows.len() {
            let mut token = [0; 16];
            loop {
                getrandom::fill(&mut token).map_err(|_| {
                    ClientSessionError::Storage(TransactionError::RandomnessUnavailable)
                })?;
                token[0] = CLIENT_METADATA_CURSOR_KIND;
                if Some(token) != cursor {
                    break;
                }
            }
            Some(token)
        } else {
            None
        };
        let payload = encode_metadata_page(&snapshot, start, end, next_cursor)?;
        if payload.len() > CLIENT_MAX_CONTROL_BYTES {
            return Err(ClientSessionError::ResourceLimit);
        }

        if let Some(next_cursor) = next_cursor {
            snapshot.next_index = end;
            snapshot.current_cursor = Some(next_cursor);
            snapshot.expires_at = std::time::Instant::now() + std::time::Duration::from_secs(60);
            self.metadata_page_snapshot = Some(snapshot);
        } else {
            self.metadata_page_snapshot = None;
        }
        self.json_response(request_id, payload)
    }

    pub fn handle_cancel_prepared_request(
        &mut self,
        storage: &StorageManager<'_>,
        staging: &crate::supervisor::StagingStore,
        request: ClientFrame,
    ) -> Result<ClientFrame, ClientSessionError> {
        let invocation_id = self.validate_control_request(storage, &request, 9, false)?;
        let session = self
            .authenticated
            .as_ref()
            .ok_or(ClientSessionError::AuthenticationRequired)?;
        if self.hello == Some(ClientMode::Control) {
            let now_us = current_unix_micros().ok_or(ClientSessionError::UnsupportedRequest)?;
            let mut event_id = [0; 16];
            getrandom::fill(&mut event_id).map_err(|_| {
                ClientSessionError::Storage(TransactionError::RandomnessUnavailable)
            })?;
            if event_id == [0; 16] {
                return Err(ClientSessionError::Storage(
                    TransactionError::RandomnessUnavailable,
                ));
            }
            let result = storage
                .cancel_invocation(
                    session.principal_id(),
                    invocation_id,
                    1,
                    now_us,
                    crate::audit::AuditEvent {
                        event_id,
                        actor_id: Some(session.principal_id()),
                        correlation_id: invocation_id,
                        occurred_at_us: now_us,
                        action: crate::audit::AuditAction::AdministrativeCancellation,
                        result: crate::audit::AuditResult::Accepted,
                        target_ids: vec![invocation_id],
                    },
                )
                .map_err(ClientSessionError::AdministrativeCancellation)?;
            let payload = match result {
                crate::storage_manager::AdministrativeCancellationResult::Accepted => {
                    b"{\"state\":\"accepted\"}".to_vec()
                }
                crate::storage_manager::AdministrativeCancellationResult::TooLate => {
                    b"{\"state\":\"too_late\"}".to_vec()
                }
                crate::storage_manager::AdministrativeCancellationResult::Terminal(outcome) => {
                    match outcome.status {
                        crate::outcome::OutcomeStatus::Committed => {
                            b"{\"state\":\"committed\"}".to_vec()
                        }
                        crate::outcome::OutcomeStatus::Aborted => {
                            b"{\"state\":\"aborted\"}".to_vec()
                        }
                    }
                }
                crate::storage_manager::AdministrativeCancellationResult::Unknown => {
                    b"{\"state\":\"unknown\"}".to_vec()
                }
            };
            return self.json_response(request.request_id, payload);
        }
        match staging.cancel_prepared(session.staging_owner(), invocation_id) {
            Ok(()) => self.json_response(request.request_id, b"{\"state\":\"accepted\"}".to_vec()),
            Err(
                crate::supervisor::StagingError::InvalidState
                | crate::supervisor::StagingError::Unavailable,
            ) => {
                let now_us = current_unix_micros().ok_or(ClientSessionError::UnsupportedRequest)?;
                let result = storage
                    .cancel_owned_invocation(session.principal_id(), invocation_id, now_us)
                    .map_err(ClientSessionError::Status)?;
                let payload = match result {
                    crate::storage_manager::OwnerCancellationResult::Accepted => {
                        b"{\"state\":\"accepted\"}".to_vec()
                    }
                    crate::storage_manager::OwnerCancellationResult::TooLate => {
                        b"{\"state\":\"too_late\"}".to_vec()
                    }
                    crate::storage_manager::OwnerCancellationResult::Terminal(outcome) => {
                        match outcome.status {
                            crate::outcome::OutcomeStatus::Committed => {
                                b"{\"state\":\"committed\"}".to_vec()
                            }
                            crate::outcome::OutcomeStatus::Aborted => {
                                b"{\"state\":\"aborted\"}".to_vec()
                            }
                        }
                    }
                    crate::storage_manager::OwnerCancellationResult::Unknown => {
                        b"{\"state\":\"unknown\"}".to_vec()
                    }
                };
                self.json_response(request.request_id, payload)
            }
            Err(error) => Err(ClientSessionError::Staging(error)),
        }
    }

    fn validate_control_request(
        &mut self,
        storage: &StorageManager<'_>,
        request: &ClientFrame,
        kind: u16,
        query_mode_required: bool,
    ) -> Result<[u8; 16], ClientSessionError> {
        if request.response_sequence != 0 || request.kind != kind {
            return Err(ClientSessionError::UnsupportedRequest);
        }
        self.request_ids
            .accept(request)
            .map_err(|_| ClientSessionError::UnsupportedRequest)?;
        if !self.permits_request_kind(kind) {
            return Err(if self.authenticated.is_none() {
                ClientSessionError::AuthenticationRequired
            } else {
                ClientSessionError::WrongMode
            });
        }
        let invocation_id = decode_client_seal(&request.payload)
            .map_err(ClientSessionError::InvalidControlPayload)?;
        if query_mode_required && self.hello != Some(ClientMode::Query) {
            return Err(ClientSessionError::WrongMode);
        }
        self.revalidate_storage(storage)?;
        Ok(invocation_id)
    }

    fn revalidate_storage(
        &mut self,
        storage: &StorageManager<'_>,
    ) -> Result<(), ClientSessionError> {
        let session = self
            .authenticated
            .as_mut()
            .ok_or(ClientSessionError::AuthenticationRequired)?;
        let mut secret = *self
            .token_secret
            .as_ref()
            .ok_or(ClientSessionError::AuthenticationRequired)?;
        let result = storage.revalidate_query_session(session, &secret);
        secret.zeroize();
        result.map_err(ClientSessionError::Storage)
    }

    fn json_response(
        &self,
        request_id: u64,
        payload: Vec<u8>,
    ) -> Result<ClientFrame, ClientSessionError> {
        Ok(ClientFrame {
            kind: 100,
            request_id,
            response_sequence: 0,
            payload,
        })
    }

    /// Accept the one HELLO for this connection before any AUTH request.
    pub fn accept_hello(
        &mut self,
        mode: ClientMode,
        client_name: &str,
    ) -> Result<(), ClientSessionError> {
        if self.hello.is_some() {
            return Err(ClientSessionError::HelloAlreadyAccepted);
        }
        if client_name.len() > 128 || self.authenticated.is_some() {
            return Err(ClientSessionError::InvalidHello);
        }
        self.hello = Some(mode);
        Ok(())
    }

    pub fn accept_hello_with_sdk_library_mode(
        &mut self,
        mode: ClientMode,
        client_name: &str,
        repository_sdk_library: bool,
    ) -> Result<(), ClientSessionError> {
        if mode == ClientMode::Query && !repository_sdk_library
            || repository_sdk_library && mode != ClientMode::Query
        {
            return Err(ClientSessionError::InvalidHello);
        }
        self.accept_hello(mode, client_name)?;
        self.repository_sdk_library = repository_sdk_library;
        Ok(())
    }

    pub fn accept_hello_payload(&mut self, payload: &[u8]) -> Result<(), ClientSessionError> {
        let hello =
            decode_client_hello(payload).map_err(ClientSessionError::InvalidControlPayload)?;
        self.accept_hello_with_sdk_library_mode(
            hello.mode,
            &hello.client_name,
            hello.repository_sdk_library,
        )
    }

    /// Authenticate using storage's current token and login/role snapshot.
    /// Callers should map all storage authentication failures to one public
    /// unauthenticated response category.
    pub fn authenticate(
        &mut self,
        storage: &StorageManager<'_>,
        token_id: [u8; 16],
        token_secret: &[u8; TOKEN_SECRET_BYTES],
    ) -> Result<&AuthenticatedQuerySession, ClientSessionError> {
        if self.hello.is_none() {
            return Err(ClientSessionError::AuthenticationOutOfOrder);
        }
        if self.authenticated.is_some() {
            return Err(ClientSessionError::AlreadyAuthenticated);
        }
        let session = storage
            .authenticate_query_session(token_id, token_secret)
            .map_err(ClientSessionError::Storage)?;
        self.authenticated = Some(session);
        self.token_secret = Some(*token_secret);
        Ok(self
            .authenticated
            .as_ref()
            .expect("session was just stored"))
    }

    pub fn authenticate_payload(
        &mut self,
        storage: &StorageManager<'_>,
        payload: &[u8],
    ) -> Result<&AuthenticatedQuerySession, ClientSessionError> {
        let credentials = decode_client_credentials(payload)
            .map_err(ClientSessionError::InvalidControlPayload)?;
        self.authenticate(storage, credentials.token_id, &credentials.token_secret)
    }

    /// Authorize a request kind and refresh token/login/role authority before
    /// each execution, status, cancellation, or administrative operation.
    pub fn authorize_request(
        &mut self,
        storage: &StorageManager<'_>,
        kind: u16,
    ) -> Result<(), ClientSessionError> {
        if !self.permits_request_kind(kind) {
            return if self.authenticated.is_none() {
                Err(ClientSessionError::AuthenticationRequired)
            } else {
                Err(ClientSessionError::WrongMode)
            };
        }
        if (3..=10).contains(&kind) {
            let session = self
                .authenticated
                .as_mut()
                .ok_or(ClientSessionError::AuthenticationRequired)?;
            let secret = self
                .token_secret
                .as_ref()
                .ok_or(ClientSessionError::AuthenticationRequired)?;
            storage
                .revalidate_query_session(session, secret)
                .map_err(ClientSessionError::Storage)?;
        }
        Ok(())
    }

    pub fn session_id(&self) -> Result<[u8; 16], ClientSessionError> {
        self.authenticated
            .as_ref()
            .map(AuthenticatedQuerySession::connection_id)
            .ok_or(ClientSessionError::AuthenticationRequired)
    }

    /// Return the authenticated owner required for query staging.
    pub fn staging_owner(&self) -> Result<StagingOwner, ClientSessionError> {
        if self.hello != Some(ClientMode::Query) {
            return Err(ClientSessionError::WrongMode);
        }
        self.authenticated
            .as_ref()
            .map(AuthenticatedQuerySession::staging_owner)
            .ok_or(ClientSessionError::AuthenticationRequired)
    }

    /// Revalidate authority and open the storage invocation only after a
    /// sealed reservation wins the one-shot START transition.
    pub fn start_submission<'db>(
        &mut self,
        storage: &StorageManager<'db>,
        staging: &crate::supervisor::StagingStore,
        invocation_id: [u8; 16],
        retention_deadline_us: i64,
        max_rows: usize,
        max_bytes: usize,
    ) -> Result<crate::storage_manager::SupervisorInvocation<'db>, ClientStartError> {
        let owner = self.staging_owner().map_err(ClientStartError::Session)?;
        let authenticated = self
            .authenticated
            .as_ref()
            .ok_or(ClientStartError::Session(
                ClientSessionError::AuthenticationRequired,
            ))?;
        let token_id = authenticated.token_id();
        let mut token_secret = *self.token_secret.as_ref().ok_or(ClientStartError::Session(
            ClientSessionError::AuthenticationRequired,
        ))?;
        let result = staging.start_with(owner, invocation_id, |submission| {
            let duration = std::time::Duration::from_millis(submission.metadata().deadline_ms);
            let deadline = std::time::Instant::now()
                .checked_add(duration)
                .ok_or(TransactionError::InvalidContext)?;
            let requested_scope = crate::submission_scope::decode_requested_scope_v1(
                &submission.metadata().requested_scope,
            )
            .map_err(|_| TransactionError::InvalidContext)?;
            storage.admit_supervisor_invocation_with_deadline_and_scope(
                crate::storage_manager::SupervisorAdmissionRequest {
                    invocation_id: submission.invocation_id(),
                    token_id,
                    retention_deadline_us,
                    max_rows,
                    max_bytes,
                    deadline: Some(deadline),
                    workload_class: submission.metadata().workload_class,
                },
                requested_scope,
                &token_secret,
            )
        });
        token_secret.zeroize();
        result.map_err(ClientStartError::Start)
    }

    /// Revalidate token authority and run the one-shot storage plus executor
    /// launch sequence for a sealed query.
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    pub fn start_submission_with_executor<'db>(
        &mut self,
        storage: &StorageManager<'db>,
        staging: &crate::supervisor::StagingStore,
        invocation_id: [u8; 16],
        admission: crate::supervisor_ipc::GatewayAdmissionConfig,
        executor: &crate::supervisor_ipc::GatewayExecutorConfig,
    ) -> Result<
        (
            crate::storage_manager::SupervisorInvocation<'db>,
            crate::executor_launcher::RunningExecutor,
        ),
        ClientStartError,
    > {
        let owner = self.staging_owner().map_err(ClientStartError::Session)?;
        let authenticated = self
            .authenticated
            .as_ref()
            .ok_or(ClientStartError::Session(
                ClientSessionError::AuthenticationRequired,
            ))?;
        let token_id = authenticated.token_id();
        let mut token_secret = *self.token_secret.as_ref().ok_or(ClientStartError::Session(
            ClientSessionError::AuthenticationRequired,
        ))?;
        let result = crate::executor_launcher::start_and_launch_executor_selected(
            staging,
            owner,
            invocation_id,
            &executor.cgroup_parent,
            |metadata| executor.resource_limits_for(metadata.workload_class),
            &executor.launch,
            |submission| {
                let duration = std::time::Duration::from_millis(submission.metadata().deadline_ms);
                let deadline = std::time::Instant::now()
                    .checked_add(duration)
                    .ok_or(TransactionError::InvalidContext)?;
                let requested_scope = crate::submission_scope::decode_requested_scope_v1(
                    &submission.metadata().requested_scope,
                )
                .map_err(|_| TransactionError::InvalidContext)?;
                storage.admit_supervisor_invocation_with_deadline_and_scope(
                    crate::storage_manager::SupervisorAdmissionRequest {
                        invocation_id: submission.invocation_id(),
                        token_id,
                        retention_deadline_us: admission
                            .retention_deadline_us()
                            .map_err(|_| TransactionError::InvalidContext)?,
                        max_rows: admission.max_rows,
                        max_bytes: admission.max_bytes,
                        deadline: Some(deadline),
                        workload_class: submission.metadata().workload_class,
                    },
                    requested_scope,
                    &token_secret,
                )
            },
        );
        token_secret.zeroize();
        result.map_err(ClientStartError::ExecutorStart)
    }

    /// Enforce the v1 mode and pre-authentication operation allowlists.
    pub fn permits_request_kind(&self, kind: u16) -> bool {
        let Some(mode) = self.hello else {
            return kind == 1;
        };
        let Some(_) = self.authenticated else {
            return kind == 2 || kind == 11;
        };
        match mode {
            ClientMode::Query => (3..=9).contains(&kind) || kind == 11,
            ClientMode::Control => {
                (8..=10).contains(&kind) || matches!(kind, 11 | CLIENT_SDK_LIBRARY_CHUNK_KIND)
            }
        }
    }
}

fn encode_lower_hex_id(id: [u8; 16]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(32);
    for byte in id {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

fn encode_lower_hex_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

fn push_json_string(output: &mut String, value: &str) {
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\u{08}' => output.push_str("\\b"),
            '\u{0c}' => output.push_str("\\f"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            c if c <= '\u{1f}' => {
                use std::fmt::Write as _;
                let _ = write!(output, "\\u{:04x}", c as u32);
            }
            c => output.push(c),
        }
    }
}

fn push_json_optional_i64(output: &mut String, value: Option<i64>) {
    if let Some(value) = value {
        output.push_str(&value.to_string());
    } else {
        output.push_str("null");
    }
}

struct RuntimeInvocationSummary<'a> {
    pinned_snapshot_count: usize,
    oldest_snapshot_pin_age_us: u64,
    invocations_admitted_total: u64,
    invocation_conflicts_total: u64,
    admin_cancellations_accepted_total: u64,
    admin_cancellations_denied_total: u64,
    invocation_policy_denials_total: u64,
    invocation_staged_value_bytes_total: u64,
    invocation_commit_latency_buckets: [u64; 6],
    invocation_redb_write_acquisition_buckets: [u64; 6],
    include_write_lock_wait: bool,
    include_outcome_maintenance: bool,
    outcome_maintenance_passes_total: u64,
    outcome_maintenance_failures_total: u64,
    outcomes_pruned_total: u64,
    waits: &'a [crate::ddl_fence::DdlFenceWaitStatus],
    truncated: bool,
}

fn encode_fence_waits(output: &mut String, summary: RuntimeInvocationSummary<'_>) {
    output.push_str("],\"pinned_snapshot_count\":");
    output.push_str(&summary.pinned_snapshot_count.to_string());
    output.push_str(",\"oldest_snapshot_pin_age_us\":");
    output.push_str(&summary.oldest_snapshot_pin_age_us.to_string());
    output.push_str(",\"invocations_admitted_total\":");
    output.push_str(&summary.invocations_admitted_total.to_string());
    output.push_str(",\"invocation_conflicts_total\":");
    output.push_str(&summary.invocation_conflicts_total.to_string());
    output.push_str(",\"admin_cancellations_accepted_total\":");
    output.push_str(&summary.admin_cancellations_accepted_total.to_string());
    output.push_str(",\"admin_cancellations_denied_total\":");
    output.push_str(&summary.admin_cancellations_denied_total.to_string());
    output.push_str(",\"invocation_policy_denials_total\":");
    output.push_str(&summary.invocation_policy_denials_total.to_string());
    output.push_str(",\"invocation_staged_value_bytes_total\":");
    output.push_str(&summary.invocation_staged_value_bytes_total.to_string());
    output.push_str(",\"invocation_commit_latency_buckets\":[");
    for (index, bucket) in summary.invocation_commit_latency_buckets.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        output.push_str(&bucket.to_string());
    }
    output.push(']');
    if summary.include_write_lock_wait {
        output.push_str(",\"invocation_redb_write_acquisition_buckets\":[");
        for (index, bucket) in summary
            .invocation_redb_write_acquisition_buckets
            .iter()
            .enumerate()
        {
            if index != 0 {
                output.push(',');
            }
            output.push_str(&bucket.to_string());
        }
        output.push(']');
    }
    if summary.include_outcome_maintenance {
        output.push_str(",\"outcome_maintenance_passes_total\":");
        output.push_str(&summary.outcome_maintenance_passes_total.to_string());
        output.push_str(",\"outcome_maintenance_failures_total\":");
        output.push_str(&summary.outcome_maintenance_failures_total.to_string());
        output.push_str(",\"outcomes_pruned_total\":");
        output.push_str(&summary.outcomes_pruned_total.to_string());
    }
    output.push_str(",\"fence_waits\":[");
    for (index, wait) in summary.waits.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        output.push_str("{\"wait_id\":\"");
        output.push_str(&encode_lower_hex_id(wait.wait_id));
        output.push_str("\",\"category\":\"");
        output.push_str(match wait.category {
            crate::ddl_fence::DdlFenceWaitCategory::Writer => "writer",
            crate::ddl_fence::DdlFenceWaitCategory::Ddl => "ddl",
        });
        output.push_str("\",\"waited_us\":");
        output.push_str(&wait.waited_us.to_string());
        output.push_str(" ,\"resource_ids\":[");
        for (resource_index, resource_id) in wait.resource_ids.iter().enumerate() {
            if resource_index != 0 {
                output.push(',');
            }
            output.push('\"');
            output.push_str(&encode_lower_hex_id(*resource_id));
            output.push('\"');
        }
        output.push_str("],\"resources_truncated\":");
        output.push_str(if wait.resources_truncated {
            "true"
        } else {
            "false"
        });
        output.push_str(" ,\"blocker_ids\":[");
        for (blocker_index, blocker_id) in wait.blocker_ids.iter().enumerate() {
            if blocker_index != 0 {
                output.push(',');
            }
            output.push('\"');
            output.push_str(&encode_lower_hex_id(*blocker_id));
            output.push('\"');
        }
        output.push_str("],\"blockers_truncated\":");
        output.push_str(if wait.blockers_truncated {
            "true"
        } else {
            "false"
        });
        output.push('}');
    }
    output.push_str("],\"fence_waits_truncated\":");
    output.push_str(if summary.truncated { "true" } else { "false" });
}

fn encode_sdk_library_page(
    releases: &[crate::sdk_registry::SdkReleaseInfo],
    next_cursor: Option<[u8; 16]>,
) -> String {
    let mut payload = String::from("{\"sdk_libraries\":[");
    for (index, release) in releases.iter().enumerate() {
        if index != 0 {
            payload.push(',');
        }
        payload.push_str("{\"release\":");
        payload.push_str(&release.sdk_release.to_string());
        payload.push_str(",\"native_abis\":[");
        for (abi_index, abi) in release.native_abis.iter().enumerate() {
            if abi_index != 0 {
                payload.push(',');
            }
            payload.push_str(&abi.to_string());
        }
        payload.push_str("],\"target\":\"");
        push_json_string(&mut payload, &release.target);
        payload.push_str("\",\"cpu_baseline\":\"");
        push_json_string(&mut payload, &release.cpu_baseline);
        payload.push_str("\",\"runtime_profile\":\"");
        push_json_string(&mut payload, &release.runtime_profile);
        payload.push_str("\",\"soname\":\"");
        push_json_string(&mut payload, &release.soname);
        payload.push_str("\",\"sha256\":\"");
        payload.push_str(&encode_lower_hex_bytes(&release.sha256));
        payload.push_str("\",\"needed\":[");
        for (dependency_index, dependency) in release.needed.iter().enumerate() {
            if dependency_index != 0 {
                payload.push(',');
            }
            payload.push('"');
            push_json_string(&mut payload, dependency);
            payload.push('"');
        }
        payload.push_str("],\"library_bytes\":");
        payload.push_str(&release.library_bytes.to_string());
        payload.push('}');
    }
    payload.push_str("],\"next_cursor\":");
    match next_cursor {
        Some(cursor) => {
            payload.push('"');
            payload.push_str(&encode_lower_hex_id(cursor));
            payload.push('"');
        }
        None => payload.push_str("null"),
    }
    payload.push('}');
    payload
}

fn encode_metadata_page(
    snapshot: &ActiveMetadataPageSnapshot,
    start: usize,
    end: usize,
    next_cursor: Option<[u8; 16]>,
) -> Result<Vec<u8>, ClientSessionError> {
    let mut output = match &snapshot.query {
        MetadataPageQuery::Schemas => String::from("{\"schemas\":["),
        MetadataPageQuery::Relations(_) => String::from("{\"relations\":["),
        MetadataPageQuery::Columns { .. } => String::from("{\"columns\":["),
        MetadataPageQuery::Indexes { .. } => String::from("{\"indexes\":["),
        MetadataPageQuery::Constraints { .. } => String::from("{\"constraints\":["),
        MetadataPageQuery::Grants { .. } => String::from("{\"grants\":["),
        MetadataPageQuery::Policies { .. } => String::from("{\"policies\":["),
        MetadataPageQuery::Dependencies(_) => String::from("{\"dependencies\":["),
    };
    for index in start..end {
        if index != start {
            output.push(',');
        }
        match (&snapshot.query, &snapshot.rows) {
            (MetadataPageQuery::Schemas, MetadataPageRows::Schemas(rows)) => {
                let row = rows.get(index).ok_or(ClientSessionError::ResourceLimit)?;
                output.push_str("{\"schema_id\":\"");
                output.push_str(&encode_lower_hex_id(row.schema_id));
                output.push_str("\",\"name\":\"");
                push_json_string(&mut output, &row.name);
                output.push_str("\"}");
            }
            (MetadataPageQuery::Relations(_), MetadataPageRows::Relations(rows)) => {
                let row = rows.get(index).ok_or(ClientSessionError::ResourceLimit)?;
                output.push_str("{\"relation_id\":\"");
                output.push_str(&encode_lower_hex_id(row.relation_id));
                output.push_str("\",\"name\":\"");
                push_json_string(&mut output, &row.name);
                output.push_str("\"}");
            }
            (MetadataPageQuery::Columns { .. }, MetadataPageRows::Columns(rows)) => {
                let row = rows.get(index).ok_or(ClientSessionError::ResourceLimit)?;
                output.push_str("{\"column_id\":\"");
                output.push_str(&encode_lower_hex_id(row.column_id));
                output.push_str("\",\"name\":\"");
                push_json_string(&mut output, &row.name);
                output.push_str("\",\"ordinal\":");
                output.push_str(&row.ordinal.to_string());
                output.push_str(" ,\"scalar_type\":\"");
                push_json_string(
                    &mut output,
                    &crate::metadata_discovery::scalar_type_name(row.scalar_type),
                );
                output.push_str("\",\"nullable\":");
                output.push_str(if row.nullable { "true" } else { "false" });
                output.push('}');
            }
            (MetadataPageQuery::Indexes { .. }, MetadataPageRows::Indexes(rows)) => {
                let row = rows.get(index).ok_or(ClientSessionError::ResourceLimit)?;
                output.push_str("{\"index_id\":\"");
                output.push_str(&encode_lower_hex_id(row.index_id));
                output.push_str("\",\"name\":\"");
                push_json_string(&mut output, &row.name);
                output.push_str("\",\"column_ids\":[");
                for (column_index, column_id) in row.column_ids.iter().enumerate() {
                    if column_index != 0 {
                        output.push(',');
                    }
                    output.push('"');
                    output.push_str(&encode_lower_hex_id(*column_id));
                    output.push('"');
                }
                output.push_str("],\"unique\":");
                output.push_str(if row.unique { "true" } else { "false" });
                output.push('}');
            }
            (MetadataPageQuery::Constraints { .. }, MetadataPageRows::Constraints(rows)) => {
                let row = rows.get(index).ok_or(ClientSessionError::ResourceLimit)?;
                output.push_str("{\"constraint_id\":\"");
                output.push_str(&encode_lower_hex_id(row.constraint_id));
                output.push_str("\",\"name\":");
                output.push('"');
                push_json_string(&mut output, &row.name);
                output.push('"');
                output.push_str(",\"kind\":\"");
                output.push_str(row.kind.as_wire_name());
                output.push_str("\",\"column_ids\":[");
                for (column_index, column_id) in row.column_ids.iter().enumerate() {
                    if column_index != 0 {
                        output.push(',');
                    }
                    output.push('"');
                    output.push_str(&encode_lower_hex_id(*column_id));
                    output.push('"');
                }
                output.push_str("],\"referenced_table_id\":");
                if let Some(table_id) = row.referenced_table_id {
                    output.push('"');
                    output.push_str(&encode_lower_hex_id(table_id));
                    output.push('"');
                } else {
                    output.push_str("null");
                }
                output.push_str(" ,\"referenced_column_ids\":[");
                for (column_index, column_id) in row.referenced_column_ids.iter().enumerate() {
                    if column_index != 0 {
                        output.push(',');
                    }
                    output.push('"');
                    output.push_str(&encode_lower_hex_id(*column_id));
                    output.push('"');
                }
                output.push_str("]}");
            }
            (MetadataPageQuery::Grants { .. }, MetadataPageRows::Grants(rows)) => {
                let row = rows.get(index).ok_or(ClientSessionError::ResourceLimit)?;
                output.push_str("{\"grantee_id\":\"");
                output.push_str(&encode_lower_hex_id(row.grantee_id));
                output.push_str("\",\"grantor_id\":\"");
                output.push_str(&encode_lower_hex_id(row.grantor_id));
                output.push_str("\",\"privilege\":\"");
                output.push_str(row.privilege.as_wire_name());
                output.push_str("\",\"target_scope\":\"");
                output.push_str(row.target_scope.as_wire_name());
                output.push_str("\",\"target_id\":\"");
                output.push_str(&encode_lower_hex_id(row.target_id));
                output.push_str("\"}");
            }
            (MetadataPageQuery::Policies { .. }, MetadataPageRows::Policies(rows)) => {
                let row = rows.get(index).ok_or(ClientSessionError::ResourceLimit)?;
                output.push_str("{\"policy_id\":\"");
                output.push_str(&encode_lower_hex_id(row.policy_id));
                output.push_str("\",\"kind\":\"");
                output.push_str(match row.kind {
                    crate::metadata_discovery::AdministrativePolicyKind::Row => "row",
                    crate::metadata_discovery::AdministrativePolicyKind::Cell => "cell",
                });
                output.push_str("\",\"operation\":\"");
                output.push_str(match row.operation {
                    crate::metadata_discovery::AdministrativePolicyOperation::Select => "select",
                    crate::metadata_discovery::AdministrativePolicyOperation::Insert => "insert",
                    crate::metadata_discovery::AdministrativePolicyOperation::Update => "update",
                    crate::metadata_discovery::AdministrativePolicyOperation::Delete => "delete",
                });
                output.push_str("\",\"effect\":\"");
                output.push_str(match row.effect {
                    crate::metadata_discovery::AdministrativePolicyEffect::Allow => "allow",
                    crate::metadata_discovery::AdministrativePolicyEffect::Restrict => "restrict",
                });
                output.push_str("\",\"column_ids\":[");
                for (column_index, column_id) in row.column_ids.iter().enumerate() {
                    if column_index != 0 {
                        output.push(',');
                    }
                    output.push('"');
                    output.push_str(&encode_lower_hex_id(*column_id));
                    output.push('"');
                }
                output.push_str("],\"recipients\":");
                if let Some(recipients) = &row.recipients {
                    output.push('[');
                    for (recipient_index, recipient) in recipients.iter().enumerate() {
                        if recipient_index != 0 {
                            output.push(',');
                        }
                        output.push_str("{\"kind\":\"");
                        output.push_str(match recipient.kind {
                            crate::metadata_discovery::AdministrativePolicyRecipientKind::User => {
                                "user"
                            }
                            crate::metadata_discovery::AdministrativePolicyRecipientKind::Role => {
                                "role"
                            }
                        });
                        output.push_str("\",\"principal_id\":\"");
                        output.push_str(&encode_lower_hex_id(recipient.principal_id));
                        output.push_str("\"}");
                    }
                    output.push(']');
                } else {
                    output.push_str("null");
                }
                output.push('}');
            }
            (MetadataPageQuery::Dependencies(_), MetadataPageRows::Dependencies(rows)) => {
                let row = rows.get(index).ok_or(ClientSessionError::ResourceLimit)?;
                output.push_str("{\"dependent_id\":\"");
                output.push_str(&encode_lower_hex_id(row.dependent_id));
                output.push_str("\",\"referenced_id\":\"");
                output.push_str(&encode_lower_hex_id(row.referenced_id));
                output.push_str("\",\"kind\":\"");
                output.push_str(row.kind.as_wire_name());
                output.push_str("\"}");
            }
            _ => return Err(ClientSessionError::ResourceLimit),
        }
    }
    output.push_str("],\"next_cursor\":");
    match next_cursor {
        Some(cursor) => {
            output.push('"');
            output.push_str(&encode_lower_hex_id(cursor));
            output.push('"');
        }
        None => output.push_str("null"),
    }
    output.push('}');
    Ok(output.into_bytes())
}

fn current_unix_micros() -> Option<i64> {
    let duration = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?;
    i64::try_from(duration.as_micros()).ok()
}

impl Drop for ClientConnectionSession {
    fn drop(&mut self) {
        if let Some(secret) = &mut self.token_secret {
            secret.zeroize();
        }
    }
}

pub struct ClientFrameDecoder {
    header: [u8; CLIENT_FRAME_HEADER_BYTES],
    header_bytes: usize,
    pending_header: Option<Header>,
    pending_payload: Vec<u8>,
    protocol_major: u16,
    protocol_minor: u16,
}

impl Default for ClientFrameDecoder {
    fn default() -> Self {
        Self {
            header: [0; CLIENT_FRAME_HEADER_BYTES],
            header_bytes: 0,
            pending_header: None,
            pending_payload: Vec::new(),
            protocol_major: 1,
            protocol_minor: 0,
        }
    }
}

impl ClientFrameDecoder {
    /// Select protocol 1.0 or 2.0 from the first complete frame header, then
    /// require every later frame on this connection to use that version.
    pub fn negotiating() -> Self {
        Self {
            header: [0; CLIENT_FRAME_HEADER_BYTES],
            header_bytes: 0,
            pending_header: None,
            pending_payload: Vec::new(),
            protocol_major: 0,
            protocol_minor: 0,
        }
    }

    /// Decode the connection-independent interactive SQL protocol v2.0.
    pub fn v2() -> Self {
        Self {
            header: [0; CLIENT_FRAME_HEADER_BYTES],
            header_bytes: 0,
            pending_header: None,
            pending_payload: Vec::new(),
            protocol_major: 2,
            protocol_minor: 0,
        }
    }

    pub fn protocol_version(&self) -> Option<(u16, u16)> {
        (self.protocol_major != 0).then_some((self.protocol_major, self.protocol_minor))
    }

    /// Add any part of the TCP byte stream and return all complete frames.
    /// Header limits are checked before payload storage is reserved.
    pub fn push(&mut self, mut input: &[u8]) -> Result<Vec<ClientFrame>, ClientFrameError> {
        let mut frames = Vec::new();
        while !input.is_empty() {
            if self.pending_header.is_none() {
                let needed = CLIENT_FRAME_HEADER_BYTES - self.header_bytes;
                let copied = needed.min(input.len());
                self.header[self.header_bytes..self.header_bytes + copied]
                    .copy_from_slice(&input[..copied]);
                self.header_bytes += copied;
                input = &input[copied..];
                if self.header_bytes < CLIENT_FRAME_HEADER_BYTES {
                    continue;
                }
                let (header, major, minor) =
                    decode_header(&self.header, self.protocol_major, self.protocol_minor)?;
                if self.protocol_major == 0 {
                    self.protocol_major = major;
                    self.protocol_minor = minor;
                }
                self.pending_payload = Vec::new();
                self.pending_payload
                    .try_reserve_exact(header.payload_bytes)
                    .map_err(|_| ClientFrameError::AllocationFailed)?;
                self.pending_header = Some(header);
                if header.payload_bytes == 0 {
                    frames.push(self.take_pending_frame());
                    continue;
                }
            }

            let header = self.pending_header.expect("pending frame header");
            let needed = header.payload_bytes - self.pending_payload.len();
            let copied = needed.min(input.len());
            self.pending_payload.extend_from_slice(&input[..copied]);
            input = &input[copied..];
            if self.pending_payload.len() == header.payload_bytes {
                frames.push(self.take_pending_frame());
            }
        }
        Ok(frames)
    }

    /// Reject an EOF that leaves any partial header or payload buffered.
    pub fn finish(&self) -> Result<(), ClientFrameError> {
        if self.header_bytes != 0 || self.pending_header.is_some() {
            Err(ClientFrameError::IncompleteFrame)
        } else {
            Ok(())
        }
    }

    fn take_pending_frame(&mut self) -> ClientFrame {
        let header = self.pending_header.take().expect("pending frame header");
        let payload = std::mem::take(&mut self.pending_payload);
        self.header_bytes = 0;
        ClientFrame {
            kind: header.kind,
            request_id: header.request_id,
            response_sequence: header.response_sequence,
            payload,
        }
    }
}

impl Drop for ClientFrameDecoder {
    fn drop(&mut self) {
        self.pending_payload.zeroize();
    }
}

impl ClientFrame {
    pub fn encode(&self) -> Result<Vec<u8>, ClientFrameError> {
        self.encode_for_version(1, 0)
    }

    pub fn encode_v2(&self) -> Result<Vec<u8>, ClientFrameError> {
        self.encode_for_version(2, 0)
    }

    fn encode_for_version(
        &self,
        protocol_major: u16,
        protocol_minor: u16,
    ) -> Result<Vec<u8>, ClientFrameError> {
        let header = Header {
            kind: self.kind,
            payload_bytes: self.payload.len(),
            request_id: self.request_id,
            response_sequence: self.response_sequence,
        };
        validate_header_fields(header, protocol_major)?;
        let payload_bytes =
            u32::try_from(self.payload.len()).map_err(|_| ClientFrameError::PayloadTooLarge)?;
        let total_bytes = CLIENT_FRAME_HEADER_BYTES
            .checked_add(self.payload.len())
            .ok_or(ClientFrameError::PayloadTooLarge)?;
        let mut output = Vec::new();
        output
            .try_reserve_exact(total_bytes)
            .map_err(|_| ClientFrameError::AllocationFailed)?;
        let header = crate::query_abi::encode_client_frame_header_record(
            CLIENT_MAGIC,
            protocol_major,
            protocol_minor,
            self.kind,
            0,
            payload_bytes,
            self.request_id,
            self.response_sequence,
        );
        output.extend_from_slice(&header);
        output.extend_from_slice(&self.payload);
        Ok(output)
    }
}

fn decode_header(
    bytes: &[u8; CLIENT_FRAME_HEADER_BYTES],
    expected_major: u16,
    expected_minor: u16,
) -> Result<(Header, u16, u16), ClientFrameError> {
    let Some((magic, major, minor, kind, flags, payload_length, request_id, response_sequence)) =
        crate::query_abi::decode_client_frame_header_record(bytes)
    else {
        return Err(ClientFrameError::InvalidMagic);
    };
    if magic != CLIENT_MAGIC {
        return Err(ClientFrameError::InvalidMagic);
    }
    let supported_negotiation = expected_major == 0 && matches!((major, minor), (1, 0) | (2, 0));
    if !supported_negotiation && (major != expected_major || minor != expected_minor) {
        return Err(ClientFrameError::UnsupportedVersion { major, minor });
    }
    if flags != 0 {
        return Err(ClientFrameError::NonzeroFlags);
    }
    let payload_bytes =
        usize::try_from(payload_length).map_err(|_| ClientFrameError::PayloadTooLarge)?;
    let header = Header {
        kind,
        payload_bytes,
        request_id,
        response_sequence,
    };
    validate_header_fields(header, major)?;
    Ok((header, major, minor))
}

fn validate_header_fields(header: Header, protocol_major: u16) -> Result<(), ClientFrameError> {
    let v2 = protocol_major == 2;
    let maximum = match header.kind {
        1..=3 | 6..=11 | 100 | 101 | 103 => CLIENT_MAX_CONTROL_BYTES,
        13..=18 if v2 => CLIENT_MAX_CONTROL_BYTES,
        4 | 5 | 12 | CLIENT_SDK_LIBRARY_CHUNK_KIND => CLIENT_MAX_CHUNK_BYTES + 24,
        102 => CLIENT_MAX_PAYLOAD_BYTES,
        kind => return Err(ClientFrameError::InvalidKind(kind)),
    };
    if header.payload_bytes > maximum || header.payload_bytes > CLIENT_MAX_PAYLOAD_BYTES {
        return Err(ClientFrameError::PayloadTooLarge);
    }
    if header.request_id == 0 {
        return Err(ClientFrameError::ZeroRequestId);
    }
    match header.kind {
        4 | 5 | 12 | CLIENT_SDK_LIBRARY_CHUNK_KIND if header.payload_bytes <= 24 => {
            Err(ClientFrameError::InvalidChunkLength)
        }
        1..=19 if header.response_sequence != 0 => Err(ClientFrameError::InvalidSequence),
        100 | 101 if header.response_sequence != 0 => Err(ClientFrameError::InvalidSequence),
        102 | 103 if header.response_sequence == 0 => Err(ClientFrameError::InvalidSequence),
        _ => Ok(()),
    }
}

#[derive(Default)]
pub struct ClientRequestIds {
    last_request_id: u64,
}

impl ClientRequestIds {
    /// Enforce strictly increasing nonzero IDs on a single client connection.
    pub fn accept(&mut self, frame: &ClientFrame) -> Result<(), ClientFrameError> {
        if !((1..=12).contains(&frame.kind) || frame.kind == CLIENT_SDK_LIBRARY_CHUNK_KIND)
            || frame.request_id == 0
            || frame.request_id <= self.last_request_id
        {
            return Err(ClientFrameError::RequestIdsNotIncreasing);
        }
        self.last_request_id = frame.request_id;
        Ok(())
    }

    /// Enforce v2 request kinds and strictly increasing nonzero IDs.
    pub fn accept_v2(&mut self, frame: &ClientFrame) -> Result<(), ClientFrameError> {
        self.accept_through(frame, 18)
    }

    fn accept_through(
        &mut self,
        frame: &ClientFrame,
        max_kind: u16,
    ) -> Result<(), ClientFrameError> {
        if !(1..=max_kind).contains(&frame.kind)
            || frame.request_id == 0
            || frame.request_id <= self.last_request_id
        {
            return Err(ClientFrameError::RequestIdsNotIncreasing);
        }
        self.last_request_id = frame.request_id;
        Ok(())
    }
}

#[derive(Default)]
pub struct ClientResponseSequences {
    next_by_request: HashMap<u64, u64>,
}

impl ClientResponseSequences {
    /// Begin streamed responses after the sequence-zero START acceptance.
    pub fn begin_stream(&mut self, request_id: u64) -> Result<(), ClientFrameError> {
        if request_id == 0
            || !self.next_by_request.is_empty()
            || self.next_by_request.contains_key(&request_id)
        {
            return Err(ClientFrameError::UnexpectedResponseSequence);
        }
        self.next_by_request.insert(request_id, 1);
        Ok(())
    }

    /// Validate consecutive RESULT_DATA frames and the terminal sequence.
    pub fn accept_stream_frame(&mut self, frame: &ClientFrame) -> Result<bool, ClientFrameError> {
        let expected = self
            .next_by_request
            .get(&frame.request_id)
            .copied()
            .ok_or(ClientFrameError::UnexpectedResponseSequence)?;
        if !matches!(frame.kind, 102 | 103) || frame.response_sequence != expected {
            return Err(ClientFrameError::UnexpectedResponseSequence);
        }
        if frame.kind == 103 {
            self.next_by_request.remove(&frame.request_id);
            Ok(true)
        } else {
            self.next_by_request.insert(
                frame.request_id,
                expected
                    .checked_add(1)
                    .ok_or(ClientFrameError::UnexpectedResponseSequence)?,
            );
            Ok(false)
        }
    }
}

#[cfg(test)]
mod runtime_wait_tests {
    use super::{ClientAdminCommand, decode_client_admin_command, encode_fence_waits};
    use crate::ddl_fence::{DdlFenceWaitCategory, DdlFenceWaitStatus};

    #[test]
    fn runtime_list_versions_one_through_eight_decode_and_later_versions_reject() {
        for version in 1..=8 {
            let request = format!(
                "{{\"phase\":\"read\",\"operation_version\":{version},\"operation\":\"runtime.invocations.list\",\"arguments\":{{\"limit\":\"1\"}}}}"
            );
            assert!(matches!(
                decode_client_admin_command(request.as_bytes()).unwrap(),
                ClientAdminCommand::ListRuntimeInvocations {
                    response_version,
                    page_limit: 1,
                    cursor: None,
                } if response_version == version
            ));
        }
        let unsupported = br#"{"phase":"read","operation_version":9,"operation":"runtime.invocations.list","arguments":{"limit":"1"}}"#;
        assert!(decode_client_admin_command(unsupported).is_err());
    }

    #[test]
    fn sdk_library_list_is_a_strict_authenticated_read_command() {
        let request = br#"{"phase":"read","operation_version":1,"operation":"sdk_libraries.list","arguments":{"limit":"4"}}"#;
        assert_eq!(
            decode_client_admin_command(request).unwrap(),
            ClientAdminCommand::ListSdkLibrariesPage {
                page_limit: 4,
                cursor: None,
            }
        );
        let unexpected_argument = br#"{"phase":"read","operation_version":1,"operation":"sdk_libraries.list","arguments":{"limit":"4","path":"/tmp"}}"#;
        assert!(decode_client_admin_command(unexpected_argument).is_err());
        let unsupported_version = br#"{"phase":"read","operation_version":2,"operation":"sdk_libraries.list","arguments":{"limit":"4"}}"#;
        assert!(decode_client_admin_command(unsupported_version).is_err());
    }

    #[test]
    fn sdk_library_remove_is_a_typed_write_command() {
        let request = br#"{"phase":"write","operation_version":1,"operation":"sdk_libraries.remove","arguments":{"operation_id":"11111111111111111111111111111111","release":"7"}}"#;
        assert_eq!(
            decode_client_admin_command(request).unwrap(),
            ClientAdminCommand::RemoveSdkLibrary {
                operation_id: [0x11; 16],
                sdk_release: 7,
            }
        );
        let zero = br#"{"phase":"write","operation_version":1,"operation":"sdk_libraries.remove","arguments":{"operation_id":"11111111111111111111111111111111","release":"0"}}"#;
        assert!(decode_client_admin_command(zero).is_err());
        let unknown = br#"{"phase":"write","operation_version":1,"operation":"sdk_libraries.remove","arguments":{"operation_id":"11111111111111111111111111111111","release":"7","abi":"5"}}"#;
        assert!(decode_client_admin_command(unknown).is_err());
    }

    #[test]
    fn sdk_library_install_prepare_and_finish_decode_strictly() {
        let begin = br#"{"phase":"prepare","operation_version":1,"operation":"sdk_libraries.install","arguments":{"operation_id":"11111111111111111111111111111111","metadata_bytes":"128","library_bytes":"4096","library_sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}}"#;
        assert_eq!(
            decode_client_admin_command(begin).unwrap(),
            ClientAdminCommand::BeginSdkLibraryInstall {
                operation_id: [0x11; 16],
                metadata_bytes: 128,
                library_bytes: 4096,
                library_sha256: [0xaa; 32],
            }
        );
        let finish = br#"{"phase":"write","operation_version":1,"operation":"sdk_libraries.install.finish","arguments":{"operation_id":"11111111111111111111111111111111"}}"#;
        assert_eq!(
            decode_client_admin_command(finish).unwrap(),
            ClientAdminCommand::FinishSdkLibraryInstall {
                operation_id: [0x11; 16],
            }
        );
        let excessive = br#"{"phase":"prepare","operation_version":1,"operation":"sdk_libraries.install","arguments":{"operation_id":"11111111111111111111111111111111","metadata_bytes":"128","library_bytes":"134217729","library_sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}}"#;
        assert!(decode_client_admin_command(excessive).is_err());
        let status = br#"{"phase":"read","operation_version":1,"operation":"sdk_libraries.operation_status","arguments":{"operation_id":"11111111111111111111111111111111"}}"#;
        assert_eq!(
            decode_client_admin_command(status).unwrap(),
            ClientAdminCommand::SdkLibraryOperationStatus {
                operation_id: [0x11; 16],
            }
        );
    }

    #[test]
    fn runtime_wait_json_uses_opaque_ids_and_explicit_truncation_flags() {
        let wait = DdlFenceWaitStatus {
            wait_id: [1; 16],
            category: DdlFenceWaitCategory::Ddl,
            waited_us: 23,
            resource_ids: vec![[2; 16]],
            resources_truncated: true,
            blocker_ids: vec![[3; 16]],
            blockers_truncated: false,
        };
        let mut output = String::from("{\"invocations\":[");
        encode_fence_waits(
            &mut output,
            super::RuntimeInvocationSummary {
                pinned_snapshot_count: 2,
                oldest_snapshot_pin_age_us: 23,
                invocations_admitted_total: 7,
                invocation_conflicts_total: 1,
                admin_cancellations_accepted_total: 1,
                admin_cancellations_denied_total: 2,
                invocation_policy_denials_total: 3,
                invocation_staged_value_bytes_total: 8,
                invocation_commit_latency_buckets: [1, 2, 3, 4, 5, 6],
                invocation_redb_write_acquisition_buckets: [6, 5, 4, 3, 2, 1],
                include_write_lock_wait: true,
                include_outcome_maintenance: true,
                outcome_maintenance_passes_total: 7,
                outcome_maintenance_failures_total: 8,
                outcomes_pruned_total: 9,
                waits: &[wait],
                truncated: true,
            },
        );
        assert_eq!(
            output,
            format!(
                "{{\"invocations\":[],\"pinned_snapshot_count\":2,\"oldest_snapshot_pin_age_us\":23,\"invocations_admitted_total\":7,\"invocation_conflicts_total\":1,\"admin_cancellations_accepted_total\":1,\"admin_cancellations_denied_total\":2,\"invocation_policy_denials_total\":3,\"invocation_staged_value_bytes_total\":8,\"invocation_commit_latency_buckets\":[1,2,3,4,5,6],\"invocation_redb_write_acquisition_buckets\":[6,5,4,3,2,1],\"outcome_maintenance_passes_total\":7,\"outcome_maintenance_failures_total\":8,\"outcomes_pruned_total\":9,\"fence_waits\":[{{\"wait_id\":\"{}\",\"category\":\"ddl\",\"waited_us\":23 ,\"resource_ids\":[\"{}\"],\"resources_truncated\":true ,\"blocker_ids\":[\"{}\"],\"blockers_truncated\":false}}],\"fence_waits_truncated\":true",
                "01".repeat(16),
                "02".repeat(16),
                "03".repeat(16)
            )
        );
        let mut legacy_output = String::from("{\"invocations\":[");
        encode_fence_waits(
            &mut legacy_output,
            super::RuntimeInvocationSummary {
                pinned_snapshot_count: 0,
                oldest_snapshot_pin_age_us: 0,
                invocations_admitted_total: 0,
                invocation_conflicts_total: 0,
                admin_cancellations_accepted_total: 0,
                admin_cancellations_denied_total: 0,
                invocation_policy_denials_total: 0,
                invocation_staged_value_bytes_total: 0,
                invocation_commit_latency_buckets: [0; 6],
                invocation_redb_write_acquisition_buckets: [0; 6],
                include_write_lock_wait: false,
                include_outcome_maintenance: false,
                outcome_maintenance_passes_total: 9,
                outcome_maintenance_failures_total: 2,
                outcomes_pruned_total: 7,
                waits: &[],
                truncated: false,
            },
        );
        assert!(!legacy_output.contains("outcome_maintenance_passes_total"));
        assert!(legacy_output.contains("\"fence_waits\":[]"));
    }
}

#[cfg(test)]
mod client_v2_frame_tests {
    use super::{
        CLIENT_V2_PACKAGE_STATUS_KIND, CLIENT_V2_TX_ATTACH_KIND, CLIENT_V2_TX_BEGIN_KIND,
        CLIENT_V2_TX_COMMIT_KIND, CLIENT_V2_TX_ROLLBACK_KIND, CLIENT_V2_TX_STATUS_KIND,
        ClientControlError, ClientFrame, ClientFrameDecoder, ClientFrameError,
        ClientV2PackageState, ClientV2Request, ClientV2Response, ClientV2TransactionState,
        TOKEN_SECRET_BYTES, decode_client_prepare_for_protocol, decode_client_start_for_protocol,
        encode_base64,
    };

    #[test]
    fn v2_transaction_frames_roundtrip_fragmented_and_v1_rejects_them() {
        let frame = ClientFrame {
            kind: CLIENT_V2_TX_BEGIN_KIND,
            request_id: 1,
            response_sequence: 0,
            payload: b"{}".to_vec(),
        };
        let bytes = frame.encode_v2().unwrap();
        assert_eq!(
            &bytes[4..10],
            &[2, 0, 0, 0, CLIENT_V2_TX_BEGIN_KIND as u8, 0]
        );

        let mut decoder = ClientFrameDecoder::v2();
        let mut decoded = Vec::new();
        for part in bytes.chunks(3) {
            decoded.extend(decoder.push(part).unwrap());
        }
        decoder.finish().unwrap();
        assert_eq!(decoded, vec![frame.clone()]);

        let mut v1_decoder = ClientFrameDecoder::default();
        assert_eq!(
            v1_decoder.push(&bytes),
            Err(ClientFrameError::UnsupportedVersion { major: 2, minor: 0 })
        );

        let mut negotiating = ClientFrameDecoder::negotiating();
        assert_eq!(negotiating.protocol_version(), None);
        assert_eq!(negotiating.push(&bytes).unwrap(), vec![frame]);
        assert_eq!(negotiating.protocol_version(), Some((2, 0)));
        let v1_frame = ClientFrame {
            kind: 11,
            request_id: 2,
            response_sequence: 0,
            payload: b"{}".to_vec(),
        }
        .encode()
        .unwrap();
        assert_eq!(
            negotiating.push(&v1_frame),
            Err(ClientFrameError::UnsupportedVersion { major: 1, minor: 0 })
        );
    }

    #[test]
    fn negotiating_decoder_selects_v1_for_legacy_connections() {
        let bytes = ClientFrame {
            kind: 11,
            request_id: 1,
            response_sequence: 0,
            payload: b"{}".to_vec(),
        }
        .encode()
        .unwrap();
        let mut decoder = ClientFrameDecoder::negotiating();
        assert_eq!(decoder.push(&bytes).unwrap().len(), 1);
        assert_eq!(decoder.protocol_version(), Some((1, 0)));
    }

    #[test]
    fn v1_encoder_cannot_emit_v2_only_transaction_kinds() {
        let frame = ClientFrame {
            kind: CLIENT_V2_TX_BEGIN_KIND,
            request_id: 1,
            response_sequence: 0,
            payload: b"{}".to_vec(),
        };
        assert_eq!(frame.encode(), Err(ClientFrameError::InvalidKind(13)));
    }

    #[test]
    fn prepare_transaction_binding_is_v2_only_and_requires_both_ids() {
        let base = r#"{"manifest_version":1,"manifest":"AQ==","artifact_bytes":"1","artifact_sha256":"0000000000000000000000000000000000000000000000000000000000000000","parameter_bytes":"0","parameter_sha256":"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855","scope_version":1,"requested_scope":"AQ==","deadline_ms":"1","workload_class":"operational","sdk_library_mode":"repository""#;
        let v2 = format!(
            "{base},\"transaction_id\":\"11111111111111111111111111111111\",\"package_id\":\"22222222222222222222222222222222\"}}"
        );
        let decoded = decode_client_prepare_for_protocol(v2.as_bytes(), 2).unwrap();
        assert_eq!(decoded.transaction_id, Some([0x11; 16]));
        assert_eq!(decoded.package_id, Some([0x22; 16]));
        assert_eq!(
            decode_client_prepare_for_protocol(v2.as_bytes(), 1),
            Err(ClientControlError::InvalidPrepare)
        );

        let only_transaction =
            format!("{base},\"transaction_id\":\"11111111111111111111111111111111\"}}");
        assert_eq!(
            decode_client_prepare_for_protocol(only_transaction.as_bytes(), 2),
            Err(ClientControlError::InvalidPrepare)
        );
        let zero_package = format!(
            "{base},\"transaction_id\":\"11111111111111111111111111111111\",\"package_id\":\"00000000000000000000000000000000\"}}"
        );
        assert_eq!(
            decode_client_prepare_for_protocol(zero_package.as_bytes(), 2),
            Err(ClientControlError::InvalidPrepare)
        );
    }

    #[test]
    fn start_must_echo_sealed_v2_package_binding() {
        let start = br#"{"invocation_id":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","transaction_id":"11111111111111111111111111111111","package_id":"22222222222222222222222222222222"}"#;
        let decoded = decode_client_start_for_protocol(start, 2).unwrap();
        assert_eq!(decoded.invocation_id, [0xaa; 16]);
        assert_eq!(decoded.transaction_id, Some([0x11; 16]));
        assert_eq!(decoded.package_id, Some([0x22; 16]));
        assert!(decoded.actor.is_none());
        assert_eq!(
            decode_client_start_for_protocol(start, 1).err(),
            Some(ClientControlError::InvalidPrepare)
        );
        let missing_package = br#"{"invocation_id":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","transaction_id":"11111111111111111111111111111111"}"#;
        assert_eq!(
            decode_client_start_for_protocol(missing_package, 2).err(),
            Some(ClientControlError::InvalidPrepare)
        );
    }

    #[test]
    fn start_actor_proof_requires_a_complete_v2_credential_set() {
        let secret = encode_base64(&[0x5a; TOKEN_SECRET_BYTES]);
        let start = format!(
            "{{\"invocation_id\":\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\",\"expected_actor_name\":\"target_user\",\"actor_token_id\":\"11111111111111111111111111111111\",\"actor_token_secret\":\"{secret}\"}}"
        );
        let decoded = decode_client_start_for_protocol(start.as_bytes(), 2).unwrap();
        let actor = decoded.actor.expect("actor proof");
        assert_eq!(actor.expected_name, "target_user");
        assert_eq!(actor.token_id, [0x11; 16]);
        assert_eq!(*actor.token_secret, [0x5a; TOKEN_SECRET_BYTES]);
        assert_eq!(
            decode_client_start_for_protocol(start.as_bytes(), 1).err(),
            Some(ClientControlError::InvalidPrepare)
        );
        let incomplete = br#"{"invocation_id":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","expected_actor_name":"target_user","actor_token_id":"11111111111111111111111111111111"}"#;
        assert_eq!(
            decode_client_start_for_protocol(incomplete, 2).err(),
            Some(ClientControlError::InvalidPrepare)
        );
    }

    #[test]
    fn transaction_request_payloads_have_canonical_golden_encodings() {
        let transaction_id = [0xabu8; 16];
        let package_id = [0xcdu8; 16];
        let cases = [
            (
                ClientV2Request::TxBegin {
                    begin_request_id: [0xabu8; 16],
                },
                CLIENT_V2_TX_BEGIN_KIND,
                "{\"begin_request_id\":\"abababababababababababababababab\"}",
            ),
            (
                ClientV2Request::TxAttach { transaction_id },
                CLIENT_V2_TX_ATTACH_KIND,
                "{\"transaction_id\":\"abababababababababababababababab\"}",
            ),
            (
                ClientV2Request::TxCommit { transaction_id },
                CLIENT_V2_TX_COMMIT_KIND,
                "{\"transaction_id\":\"abababababababababababababababab\"}",
            ),
            (
                ClientV2Request::TxRollback { transaction_id },
                CLIENT_V2_TX_ROLLBACK_KIND,
                "{\"transaction_id\":\"abababababababababababababababab\"}",
            ),
            (
                ClientV2Request::TxStatus { transaction_id },
                CLIENT_V2_TX_STATUS_KIND,
                "{\"transaction_id\":\"abababababababababababababababab\"}",
            ),
            (
                ClientV2Request::PackageStatus {
                    transaction_id,
                    package_id,
                },
                CLIENT_V2_PACKAGE_STATUS_KIND,
                "{\"transaction_id\":\"abababababababababababababababab\",\"package_id\":\"cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd\"}",
            ),
        ];
        for (request, expected_kind, expected_payload) in cases {
            let (kind, payload) = request.encode().unwrap();
            assert_eq!(kind, expected_kind);
            assert_eq!(payload, expected_payload.as_bytes());
            assert_eq!(ClientV2Request::decode(kind, &payload), Ok(request));
        }
    }

    #[test]
    fn transaction_request_decoder_rejects_ambiguous_or_extra_fields() {
        assert!(ClientV2Request::decode(CLIENT_V2_TX_BEGIN_KIND, b"{\"extra\":\"x\"}").is_err());
        assert!(
            ClientV2Request::decode(
                CLIENT_V2_TX_ATTACH_KIND,
                b"{\"transaction_id\":\"abababababababababababababababab\",\"extra\":\"x\"}"
            )
            .is_err()
        );
        assert!(
            ClientV2Request::decode(
                CLIENT_V2_TX_STATUS_KIND,
                b"{\"transaction_id\":\"ABABABABABABABABABABABABABABABAB\"}"
            )
            .is_err()
        );
    }

    #[test]
    fn transaction_responses_match_canonical_schemas() {
        let transaction_id = [0xabu8; 16];
        let begin = ClientV2Response::TransactionBegun {
            transaction_id,
            state: ClientV2TransactionState::Active,
            idle_timeout_ms: 1_000,
            lifetime_ms: 60_000,
            max_package_bytes: 65_536,
            max_packages: 128,
        };
        let begin_bytes = begin.encode();
        assert_eq!(
            begin_bytes,
            b"{\"transaction_id\":\"abababababababababababababababab\",\"state\":\"active\",\"idle_timeout_ms\":1000,\"lifetime_ms\":60000,\"max_package_bytes\":65536,\"max_packages\":128}"
        );
        assert_eq!(
            ClientV2Response::decode(CLIENT_V2_TX_BEGIN_KIND, &begin_bytes),
            Ok(begin)
        );
        let expired_begin = ClientV2Response::TransactionBegun {
            transaction_id,
            state: ClientV2TransactionState::Expired,
            idle_timeout_ms: 1_000,
            lifetime_ms: 60_000,
            max_package_bytes: 65_536,
            max_packages: 128,
        };
        assert_eq!(
            ClientV2Response::decode(CLIENT_V2_TX_BEGIN_KIND, &expired_begin.encode()),
            Ok(expired_begin)
        );

        let committed = ClientV2Response::Transaction {
            transaction_id,
            state: ClientV2TransactionState::Committed,
        };
        let committed_bytes = committed.encode();
        assert_eq!(
            committed_bytes,
            b"{\"transaction_id\":\"abababababababababababababababab\",\"state\":\"committed\"}"
        );
        assert_eq!(
            ClientV2Response::decode(CLIENT_V2_TX_COMMIT_KIND, &committed_bytes),
            Ok(committed)
        );

        let attached = ClientV2Response::Transaction {
            transaction_id,
            state: ClientV2TransactionState::AbortOnly,
        };
        assert_eq!(
            ClientV2Response::decode(CLIENT_V2_TX_ATTACH_KIND, &attached.encode()),
            Ok(attached)
        );
        assert!(ClientV2Response::decode(CLIENT_V2_TX_ROLLBACK_KIND, &committed_bytes).is_err());

        let package = ClientV2Response::Package {
            transaction_id,
            package_id: [0xcdu8; 16],
            state: ClientV2PackageState::Completed,
            result_lost: true,
        };
        let package_bytes = package.encode();
        assert_eq!(
            package_bytes,
            b"{\"transaction_id\":\"abababababababababababababababab\",\"package_id\":\"cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd\",\"state\":\"completed\",\"result_lost\":true}"
        );
        assert_eq!(
            ClientV2Response::decode(CLIENT_V2_PACKAGE_STATUS_KIND, &package_bytes),
            Ok(package)
        );
    }

    #[test]
    fn transaction_response_decoder_rejects_invalid_limits_and_lost_state() {
        assert!(ClientV2Response::decode(
            CLIENT_V2_TX_BEGIN_KIND,
            b"{\"transaction_id\":\"abababababababababababababababab\",\"state\":\"active\",\"idle_timeout_ms\":0,\"lifetime_ms\":1,\"max_package_bytes\":1,\"max_packages\":1}"
        )
        .is_err());
        assert!(ClientV2Response::decode(
            CLIENT_V2_PACKAGE_STATUS_KIND,
            b"{\"transaction_id\":\"abababababababababababababababab\",\"package_id\":\"cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd\",\"state\":\"running\",\"result_lost\":true}"
        )
        .is_err());
    }
}
