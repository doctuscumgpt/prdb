//! Bind structured optimizer requests against invocation-authorized metadata.

use crate::metadata_discovery::{
    AuthorizedPlanRelation, AuthorizedRelationDescription, VisibleColumn,
};
use crate::optimizer_request::{
    Comparison, CompositeGroupCountOrderItem, CompositeGroupCountRequest, GroupCountOrderItem,
    GroupCountRequest, JoinRelationSide, OrderedGroupCountRequest, OrderedSelectRequest, Predicate,
    RequestError, UpdateValue, decode_composite_group_count_request, decode_delete_request,
    decode_group_count_request, decode_join_select_request,
    decode_ordered_composite_group_count_request, decode_ordered_group_count_request,
    decode_ordered_select_request, decode_select_request, decode_update_request,
};
use crate::transaction::{PlannerMetadataConnection, TransactionError};
use crate::tuple::ScalarType;
use std::collections::HashSet;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundSelect {
    pub schema_name: String,
    pub schema_id: [u8; 16],
    pub schema_version: u64,
    pub table_epoch: u64,
    pub relation_id: [u8; 16],
    pub relation_name: String,
    pub projections: Vec<BoundColumn>,
    pub predicate: Option<BoundPredicate>,
    pub limit: Option<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundOrderedSelect {
    pub schema_name: String,
    pub schema_id: [u8; 16],
    pub schema_version: u64,
    pub table_epoch: u64,
    pub relation_id: [u8; 16],
    pub relation_name: String,
    pub projections: Vec<BoundColumn>,
    pub order_key: BoundColumn,
    pub descending: bool,
    pub limit: Option<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundDelete {
    pub schema_name: String,
    pub schema_id: [u8; 16],
    pub schema_version: u64,
    pub table_epoch: u64,
    pub relation_id: [u8; 16],
    pub relation_name: String,
    pub predicate: BoundPredicate,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundGroupCount {
    pub schema_name: String,
    pub schema_id: [u8; 16],
    pub schema_version: u64,
    pub table_epoch: u64,
    pub relation_id: [u8; 16],
    pub relation_name: String,
    pub group_key: BoundColumn,
    pub limit: Option<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundOrderedGroupCount {
    pub schema_name: String,
    pub schema_id: [u8; 16],
    pub schema_version: u64,
    pub table_epoch: u64,
    pub relation_id: [u8; 16],
    pub relation_name: String,
    pub group_key: BoundColumn,
    pub order_items: Vec<GroupCountOrderItem>,
    pub limit: Option<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundCompositeGroupCount {
    pub schema_name: String,
    pub schema_id: [u8; 16],
    pub schema_version: u64,
    pub table_epoch: u64,
    pub relation_id: [u8; 16],
    pub relation_name: String,
    pub group_keys: Vec<BoundColumn>,
    pub limit: Option<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundOrderedCompositeGroupCount {
    pub schema_name: String,
    pub schema_id: [u8; 16],
    pub schema_version: u64,
    pub table_epoch: u64,
    pub relation_id: [u8; 16],
    pub relation_name: String,
    pub group_keys: Vec<BoundColumn>,
    pub order_items: Vec<CompositeGroupCountOrderItem>,
    pub limit: Option<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundUpdate {
    pub schema_name: String,
    pub schema_id: [u8; 16],
    pub schema_version: u64,
    pub table_epoch: u64,
    pub relation_id: [u8; 16],
    pub relation_name: String,
    pub assignments: Vec<BoundUpdateAssignment>,
    pub predicate: BoundPredicate,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundJoinRelation {
    pub relation_id: [u8; 16],
    pub relation_name: String,
    pub table_epoch: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundJoinProjection {
    pub relation_side: JoinRelationSide,
    pub column: BoundColumn,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundJoinKey {
    pub left_column: BoundColumn,
    pub right_column: BoundColumn,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundJoinSelect {
    pub schema_name: String,
    pub schema_id: [u8; 16],
    pub schema_version: u64,
    pub left_relation: BoundJoinRelation,
    pub right_relation: BoundJoinRelation,
    pub projections: Vec<BoundJoinProjection>,
    pub join_keys: Vec<BoundJoinKey>,
    pub limit: Option<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundUpdateAssignment {
    pub column: BoundColumn,
    pub value: BoundUpdateValue,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BoundUpdateValue {
    Parameter(u32),
    Null,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundColumn {
    pub column_id: [u8; 16],
    pub ordinal: u32,
    pub scalar_type: ScalarType,
    pub nullable: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BoundPredicate {
    Null,
    Boolean(bool),
    Integer(i64),
    Text(String),
    Column(BoundColumn),
    Compare {
        operator: Comparison,
        left: Box<BoundPredicate>,
        right: Box<BoundPredicate>,
    },
    And(Vec<BoundPredicate>),
    Or(Vec<BoundPredicate>),
    Not(Box<BoundPredicate>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BindError {
    Request(RequestError),
    RelationUnavailable,
    InvalidMetadata,
    InvalidQuery,
}

#[derive(Debug)]
pub enum MetadataBindError {
    Bind(BindError),
    Transaction(TransactionError),
}

impl std::fmt::Display for MetadataBindError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "optimizer metadata bind failed: {self:?}")
    }
}

impl std::error::Error for MetadataBindError {}

impl From<BindError> for MetadataBindError {
    fn from(error: BindError) -> Self {
        Self::Bind(error)
    }
}

impl std::fmt::Display for BindError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "optimizer bind error: {self:?}")
    }
}

impl std::error::Error for BindError {}

impl From<RequestError> for BindError {
    fn from(error: RequestError) -> Self {
        Self::Request(error)
    }
}

/// Bind one canonical request against a relation description returned by the
/// invocation's authorized planner metadata connection. Missing and
/// undiscoverable relations share one result and never produce a partial plan.
pub fn bind_select_request(
    request_bytes: &[u8],
    relation: Option<AuthorizedPlanRelation>,
) -> Result<BoundSelect, BindError> {
    let request = decode_select_request(request_bytes)?;
    bind_decoded_select(request, relation)
}

/// Resolve the request's qualified relation through a planner connection
/// opened on the invocation transaction, then bind it. The request cannot
/// supply or override the invocation identity, grants, or connection role.
pub fn bind_select_request_via_metadata(
    request_bytes: &[u8],
    connection: &mut PlannerMetadataConnection<'_, '_>,
) -> Result<BoundSelect, MetadataBindError> {
    let request = decode_select_request(request_bytes).map_err(BindError::from)?;
    let relation = connection
        .resolve_relation_for_plan(&request.schema_name, &request.relation_name)
        .map_err(MetadataBindError::Transaction)?
        .map(|(_, description)| description);
    bind_decoded_select(request, relation).map_err(MetadataBindError::Bind)
}

pub fn bind_ordered_select_request(
    request_bytes: &[u8],
    relation: Option<AuthorizedPlanRelation>,
) -> Result<BoundOrderedSelect, BindError> {
    let request = decode_ordered_select_request(request_bytes)?;
    bind_decoded_ordered_select(request, relation)
}

pub fn bind_ordered_select_request_via_metadata(
    request_bytes: &[u8],
    connection: &mut PlannerMetadataConnection<'_, '_>,
) -> Result<BoundOrderedSelect, MetadataBindError> {
    let request = decode_ordered_select_request(request_bytes).map_err(BindError::from)?;
    let relation = connection
        .resolve_relation_for_plan(&request.schema_name, &request.relation_name)
        .map_err(MetadataBindError::Transaction)?
        .map(|(_, relation)| relation);
    bind_decoded_ordered_select(request, relation).map_err(MetadataBindError::Bind)
}

pub fn bind_join_select_request(
    request_bytes: &[u8],
    left_relation: Option<AuthorizedPlanRelation>,
    right_relation: Option<AuthorizedPlanRelation>,
) -> Result<BoundJoinSelect, BindError> {
    let request = decode_join_select_request(request_bytes)?;
    bind_decoded_join_select(request, left_relation, right_relation)
}

pub fn bind_join_select_request_via_metadata(
    request_bytes: &[u8],
    connection: &mut PlannerMetadataConnection<'_, '_>,
) -> Result<BoundJoinSelect, MetadataBindError> {
    let request = decode_join_select_request(request_bytes).map_err(BindError::from)?;
    let left_relation = connection
        .resolve_relation_for_plan(&request.schema_name, &request.left_relation_name)
        .map_err(MetadataBindError::Transaction)?
        .map(|(_, relation)| relation);
    let right_relation = connection
        .resolve_relation_for_plan(&request.schema_name, &request.right_relation_name)
        .map_err(MetadataBindError::Transaction)?
        .map(|(_, relation)| relation);
    bind_decoded_join_select(request, left_relation, right_relation)
        .map_err(MetadataBindError::Bind)
}

pub fn bind_group_count_request(
    request_bytes: &[u8],
    relation: Option<AuthorizedPlanRelation>,
) -> Result<BoundGroupCount, BindError> {
    let request = decode_group_count_request(request_bytes)?;
    bind_decoded_group_count(request, relation)
}

pub fn bind_group_count_request_via_metadata(
    request_bytes: &[u8],
    connection: &mut PlannerMetadataConnection<'_, '_>,
) -> Result<BoundGroupCount, MetadataBindError> {
    let request = decode_group_count_request(request_bytes).map_err(BindError::from)?;
    let relation = connection
        .resolve_relation_for_plan(&request.schema_name, &request.relation_name)
        .map_err(MetadataBindError::Transaction)?
        .map(|(_, relation)| relation);
    bind_decoded_group_count(request, relation).map_err(MetadataBindError::Bind)
}

pub fn bind_ordered_group_count_request(
    request_bytes: &[u8],
    relation: Option<AuthorizedPlanRelation>,
) -> Result<BoundOrderedGroupCount, BindError> {
    let request = decode_ordered_group_count_request(request_bytes)?;
    bind_decoded_ordered_group_count(request, relation)
}

pub fn bind_ordered_group_count_request_via_metadata(
    request_bytes: &[u8],
    connection: &mut PlannerMetadataConnection<'_, '_>,
) -> Result<BoundOrderedGroupCount, MetadataBindError> {
    let request = decode_ordered_group_count_request(request_bytes).map_err(BindError::from)?;
    let relation = connection
        .resolve_relation_for_plan(&request.schema_name, &request.relation_name)
        .map_err(MetadataBindError::Transaction)?
        .map(|(_, relation)| relation);
    bind_decoded_ordered_group_count(request, relation).map_err(MetadataBindError::Bind)
}

pub fn bind_composite_group_count_request(
    request_bytes: &[u8],
    relation: Option<AuthorizedPlanRelation>,
) -> Result<BoundCompositeGroupCount, BindError> {
    let request = decode_composite_group_count_request(request_bytes)?;
    bind_decoded_composite_group_count(request, relation)
}

pub fn bind_composite_group_count_request_via_metadata(
    request_bytes: &[u8],
    connection: &mut PlannerMetadataConnection<'_, '_>,
) -> Result<BoundCompositeGroupCount, MetadataBindError> {
    let request = decode_composite_group_count_request(request_bytes).map_err(BindError::from)?;
    let relation = connection
        .resolve_relation_for_plan(&request.schema_name, &request.relation_name)
        .map_err(MetadataBindError::Transaction)?
        .map(|(_, relation)| relation);
    bind_decoded_composite_group_count(request, relation).map_err(MetadataBindError::Bind)
}

pub fn bind_ordered_composite_group_count_request(
    request_bytes: &[u8],
    relation: Option<AuthorizedPlanRelation>,
) -> Result<BoundOrderedCompositeGroupCount, BindError> {
    let request = decode_ordered_composite_group_count_request(request_bytes)?;
    bind_ordered_composite_group_count_decoded(request, relation)
}

pub fn bind_ordered_composite_group_count_request_via_metadata(
    request_bytes: &[u8],
    connection: &mut PlannerMetadataConnection<'_, '_>,
) -> Result<BoundOrderedCompositeGroupCount, MetadataBindError> {
    let request =
        decode_ordered_composite_group_count_request(request_bytes).map_err(BindError::from)?;
    let relation = connection
        .resolve_relation_for_plan(&request.schema_name, &request.relation_name)
        .map_err(MetadataBindError::Transaction)?
        .map(|(_, relation)| relation);
    bind_ordered_composite_group_count_decoded(request, relation).map_err(MetadataBindError::Bind)
}

fn bind_ordered_composite_group_count_decoded(
    request: crate::optimizer_request::OrderedCompositeGroupCountRequest,
    relation: Option<AuthorizedPlanRelation>,
) -> Result<BoundOrderedCompositeGroupCount, BindError> {
    let relation = authorized_relation(&request.relation_name, relation)?;
    let mut group_keys = Vec::with_capacity(request.group_key_names.len());
    let mut seen = HashSet::with_capacity(request.group_key_names.len());
    for name in &request.group_key_names {
        let column = resolve_column(&relation.relation.columns, name)?;
        if !seen.insert(column.ordinal) {
            return Err(BindError::InvalidQuery);
        }
        group_keys.push(column.into());
    }
    Ok(BoundOrderedCompositeGroupCount {
        schema_name: request.schema_name,
        schema_id: relation.schema_id,
        schema_version: relation.schema_version,
        table_epoch: relation.table_epoch,
        relation_id: relation.relation.relation_id,
        relation_name: relation.relation.name,
        group_keys,
        order_items: request.order_items,
        limit: request.limit,
    })
}

pub fn bind_delete_request(
    request_bytes: &[u8],
    relation: Option<AuthorizedPlanRelation>,
) -> Result<BoundDelete, BindError> {
    let request = decode_delete_request(request_bytes)?;
    bind_decoded_delete(request, relation)
}

pub fn bind_delete_request_via_metadata(
    request_bytes: &[u8],
    connection: &mut PlannerMetadataConnection<'_, '_>,
) -> Result<BoundDelete, MetadataBindError> {
    let request = decode_delete_request(request_bytes).map_err(BindError::from)?;
    let relation = connection
        .resolve_relation_for_plan(&request.schema_name, &request.relation_name)
        .map_err(MetadataBindError::Transaction)?
        .map(|(_, description)| description);
    bind_decoded_delete(request, relation).map_err(MetadataBindError::Bind)
}

pub fn bind_update_request(
    request_bytes: &[u8],
    relation: Option<AuthorizedPlanRelation>,
) -> Result<BoundUpdate, BindError> {
    let request = decode_update_request(request_bytes)?;
    bind_decoded_update(request, relation)
}

pub fn bind_update_request_via_metadata(
    request_bytes: &[u8],
    connection: &mut PlannerMetadataConnection<'_, '_>,
) -> Result<BoundUpdate, MetadataBindError> {
    let request = decode_update_request(request_bytes).map_err(BindError::from)?;
    let relation = connection
        .resolve_relation_for_plan(&request.schema_name, &request.relation_name)
        .map_err(MetadataBindError::Transaction)?
        .map(|(_, description)| description);
    bind_decoded_update(request, relation).map_err(MetadataBindError::Bind)
}

fn bind_decoded_select(
    request: crate::optimizer_request::SelectRequest,
    relation: Option<AuthorizedPlanRelation>,
) -> Result<BoundSelect, BindError> {
    let relation = authorized_relation(&request.relation_name, relation)?;
    let mut projections = Vec::with_capacity(request.projections.len());
    for name in &request.projections {
        projections.push(resolve_column(&relation.relation.columns, name)?.into());
    }
    let predicate = request
        .predicate
        .as_ref()
        .map(|predicate| {
            let (bound, scalar_type) = bind_predicate(predicate, &relation.relation.columns)?;
            if scalar_type != Some(ScalarType::Boolean) {
                return Err(BindError::InvalidQuery);
            }
            Ok(bound)
        })
        .transpose()?;
    Ok(BoundSelect {
        schema_name: request.schema_name,
        schema_id: relation.schema_id,
        schema_version: relation.schema_version,
        table_epoch: relation.table_epoch,
        relation_id: relation.relation.relation_id,
        relation_name: relation.relation.name,
        projections,
        predicate,
        limit: request.limit,
    })
}

fn bind_decoded_ordered_select(
    request: OrderedSelectRequest,
    relation: Option<AuthorizedPlanRelation>,
) -> Result<BoundOrderedSelect, BindError> {
    let relation = authorized_relation(&request.relation_name, relation)?;
    let mut projections = Vec::with_capacity(request.projections.len());
    let mut seen = HashSet::with_capacity(request.projections.len());
    for name in &request.projections {
        let column = resolve_column(&relation.relation.columns, name)?;
        if !seen.insert(column.ordinal) {
            return Err(BindError::InvalidQuery);
        }
        projections.push(column.into());
    }
    let order_key = resolve_column(&relation.relation.columns, &request.order_key_name)?;
    Ok(BoundOrderedSelect {
        schema_name: request.schema_name,
        schema_id: relation.schema_id,
        schema_version: relation.schema_version,
        table_epoch: relation.table_epoch,
        relation_id: relation.relation.relation_id,
        relation_name: relation.relation.name,
        projections,
        order_key: order_key.into(),
        descending: request.descending,
        limit: request.limit,
    })
}

fn bind_decoded_join_select(
    request: crate::optimizer_request::JoinSelectRequest,
    left_relation: Option<AuthorizedPlanRelation>,
    right_relation: Option<AuthorizedPlanRelation>,
) -> Result<BoundJoinSelect, BindError> {
    let left_relation = authorized_relation(&request.left_relation_name, left_relation)?;
    let right_relation = authorized_relation(&request.right_relation_name, right_relation)?;
    if left_relation.schema_id != right_relation.schema_id
        || left_relation.schema_version != right_relation.schema_version
    {
        return Err(BindError::InvalidQuery);
    }
    let projections = request
        .projections
        .iter()
        .map(|projection| {
            let columns = match projection.relation_side {
                JoinRelationSide::Left => &left_relation.relation.columns,
                JoinRelationSide::Right => &right_relation.relation.columns,
            };
            Ok(BoundJoinProjection {
                relation_side: projection.relation_side,
                column: resolve_column(columns, &projection.column_name)?.into(),
            })
        })
        .collect::<Result<Vec<_>, BindError>>()?;
    let join_keys = request
        .join_keys
        .iter()
        .map(|key| {
            let left_column =
                resolve_column(&left_relation.relation.columns, &key.left_column_name)?;
            let right_column =
                resolve_column(&right_relation.relation.columns, &key.right_column_name)?;
            if !join_types_compatible(left_column.scalar_type, right_column.scalar_type) {
                return Err(BindError::InvalidQuery);
            }
            Ok(BoundJoinKey {
                left_column: left_column.into(),
                right_column: right_column.into(),
            })
        })
        .collect::<Result<Vec<_>, BindError>>()?;
    Ok(BoundJoinSelect {
        schema_name: request.schema_name,
        schema_id: left_relation.schema_id,
        schema_version: left_relation.schema_version,
        left_relation: BoundJoinRelation {
            relation_id: left_relation.relation.relation_id,
            relation_name: left_relation.relation.name,
            table_epoch: left_relation.table_epoch,
        },
        right_relation: BoundJoinRelation {
            relation_id: right_relation.relation.relation_id,
            relation_name: right_relation.relation.name,
            table_epoch: right_relation.table_epoch,
        },
        projections,
        join_keys,
        limit: request.limit,
    })
}

fn bind_decoded_group_count(
    request: GroupCountRequest,
    relation: Option<AuthorizedPlanRelation>,
) -> Result<BoundGroupCount, BindError> {
    let relation = authorized_relation(&request.relation_name, relation)?;
    let group_key = resolve_column(&relation.relation.columns, &request.group_key_name)?;
    Ok(BoundGroupCount {
        schema_name: request.schema_name,
        schema_id: relation.schema_id,
        schema_version: relation.schema_version,
        table_epoch: relation.table_epoch,
        relation_id: relation.relation.relation_id,
        relation_name: relation.relation.name,
        group_key: group_key.into(),
        limit: request.limit,
    })
}

fn bind_decoded_ordered_group_count(
    request: OrderedGroupCountRequest,
    relation: Option<AuthorizedPlanRelation>,
) -> Result<BoundOrderedGroupCount, BindError> {
    let relation = authorized_relation(&request.relation_name, relation)?;
    let group_key = resolve_column(&relation.relation.columns, &request.group_key_name)?;
    Ok(BoundOrderedGroupCount {
        schema_name: request.schema_name,
        schema_id: relation.schema_id,
        schema_version: relation.schema_version,
        table_epoch: relation.table_epoch,
        relation_id: relation.relation.relation_id,
        relation_name: relation.relation.name,
        group_key: group_key.into(),
        order_items: request.order_items,
        limit: request.limit,
    })
}

fn bind_decoded_composite_group_count(
    request: CompositeGroupCountRequest,
    relation: Option<AuthorizedPlanRelation>,
) -> Result<BoundCompositeGroupCount, BindError> {
    let relation = authorized_relation(&request.relation_name, relation)?;
    let mut group_keys = Vec::with_capacity(request.group_key_names.len());
    let mut seen = HashSet::with_capacity(request.group_key_names.len());
    for name in &request.group_key_names {
        let column = resolve_column(&relation.relation.columns, name)?;
        if !seen.insert(column.ordinal) {
            return Err(BindError::InvalidQuery);
        }
        group_keys.push(column.into());
    }
    Ok(BoundCompositeGroupCount {
        schema_name: request.schema_name,
        schema_id: relation.schema_id,
        schema_version: relation.schema_version,
        table_epoch: relation.table_epoch,
        relation_id: relation.relation.relation_id,
        relation_name: relation.relation.name,
        group_keys,
        limit: request.limit,
    })
}

fn join_types_compatible(left: ScalarType, right: ScalarType) -> bool {
    match (left, right) {
        (ScalarType::Decimal { scale: left, .. }, ScalarType::Decimal { scale: right, .. }) => {
            left == right
        }
        (ScalarType::Varchar { .. }, ScalarType::Varchar { .. }) => true,
        (left, right) => left == right,
    }
}

fn bind_decoded_delete(
    request: crate::optimizer_request::DeleteRequest,
    relation: Option<AuthorizedPlanRelation>,
) -> Result<BoundDelete, BindError> {
    let relation = authorized_relation(&request.relation_name, relation)?;
    let (predicate, scalar_type) = bind_predicate(&request.predicate, &relation.relation.columns)?;
    if scalar_type != Some(ScalarType::Boolean) {
        return Err(BindError::InvalidQuery);
    }
    Ok(BoundDelete {
        schema_name: request.schema_name,
        schema_id: relation.schema_id,
        schema_version: relation.schema_version,
        table_epoch: relation.table_epoch,
        relation_id: relation.relation.relation_id,
        relation_name: relation.relation.name,
        predicate,
    })
}

fn bind_decoded_update(
    request: crate::optimizer_request::UpdateRequest,
    relation: Option<AuthorizedPlanRelation>,
) -> Result<BoundUpdate, BindError> {
    let relation = authorized_relation(&request.relation_name, relation)?;
    let mut assignments = Vec::with_capacity(request.assignments.len());
    let mut assigned_ordinals = HashSet::with_capacity(request.assignments.len());
    for assignment in request.assignments {
        let column = resolve_column(&relation.relation.columns, &assignment.column_name)?;
        if !assigned_ordinals.insert(column.ordinal) {
            return Err(BindError::InvalidQuery);
        }
        let value = match assignment.value {
            UpdateValue::Parameter(index) => BoundUpdateValue::Parameter(index),
            UpdateValue::Null if column.nullable => BoundUpdateValue::Null,
            UpdateValue::Null => return Err(BindError::InvalidQuery),
        };
        assignments.push(BoundUpdateAssignment {
            column: column.into(),
            value,
        });
    }
    let (predicate, scalar_type) = bind_predicate(&request.predicate, &relation.relation.columns)?;
    if scalar_type != Some(ScalarType::Boolean) {
        return Err(BindError::InvalidQuery);
    }
    Ok(BoundUpdate {
        schema_name: request.schema_name,
        schema_id: relation.schema_id,
        schema_version: relation.schema_version,
        table_epoch: relation.table_epoch,
        relation_id: relation.relation.relation_id,
        relation_name: relation.relation.name,
        assignments,
        predicate,
    })
}

fn authorized_relation(
    requested_name: &str,
    relation: Option<AuthorizedPlanRelation>,
) -> Result<AuthorizedPlanRelation, BindError> {
    let relation = relation.ok_or(BindError::RelationUnavailable)?;
    if !relation.relation.name.eq_ignore_ascii_case(requested_name) {
        return Err(BindError::RelationUnavailable);
    }
    validate_relation_shape(&relation.relation)?;
    Ok(relation)
}

fn validate_relation_shape(relation: &AuthorizedRelationDescription) -> Result<(), BindError> {
    if relation.relation_id == [0; 16] || relation.columns.is_empty() {
        return Err(BindError::InvalidMetadata);
    }
    let mut ids = HashSet::with_capacity(relation.columns.len());
    let mut ordinals = HashSet::with_capacity(relation.columns.len());
    for column in &relation.columns {
        if column.column_id == [0; 16]
            || column.name.is_empty()
            || column.name.len() > 63
            || !column.name.is_ascii()
            || !ids.insert(column.column_id)
            || !ordinals.insert(column.ordinal)
        {
            return Err(BindError::InvalidMetadata);
        }
    }
    if ordinals.len() != relation.columns.len()
        || (0..relation.columns.len()).any(|ordinal| !ordinals.contains(&(ordinal as u32)))
    {
        return Err(BindError::InvalidMetadata);
    }
    Ok(())
}

fn resolve_column<'a>(
    columns: &'a [VisibleColumn],
    name: &str,
) -> Result<&'a VisibleColumn, BindError> {
    let mut matching = columns
        .iter()
        .filter(|column| column.name.eq_ignore_ascii_case(name));
    let column = matching.next().ok_or(BindError::InvalidQuery)?;
    if matching.next().is_some() {
        return Err(BindError::InvalidMetadata);
    }
    Ok(column)
}

impl From<&VisibleColumn> for BoundColumn {
    fn from(column: &VisibleColumn) -> Self {
        Self {
            column_id: column.column_id,
            ordinal: column.ordinal,
            scalar_type: column.scalar_type,
            nullable: column.nullable,
        }
    }
}

fn bind_predicate(
    predicate: &Predicate,
    columns: &[VisibleColumn],
) -> Result<(BoundPredicate, Option<ScalarType>), BindError> {
    match predicate {
        Predicate::Null => Ok((BoundPredicate::Null, None)),
        Predicate::Boolean(value) => {
            Ok((BoundPredicate::Boolean(*value), Some(ScalarType::Boolean)))
        }
        Predicate::Integer(value) => {
            Ok((BoundPredicate::Integer(*value), Some(ScalarType::Integer)))
        }
        Predicate::Text(value) => Ok((BoundPredicate::Text(value.clone()), Some(ScalarType::Text))),
        Predicate::Column(name) => {
            let column = resolve_column(columns, name)?;
            Ok((
                BoundPredicate::Column(column.into()),
                Some(column.scalar_type),
            ))
        }
        Predicate::Compare {
            operator,
            left,
            right,
        } => {
            let (left, left_type) = bind_predicate(left, columns)?;
            let (right, right_type) = bind_predicate(right, columns)?;
            if matches!((left_type, right_type), (Some(left), Some(right)) if left != right) {
                return Err(BindError::InvalidQuery);
            }
            Ok((
                BoundPredicate::Compare {
                    operator: *operator,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                Some(ScalarType::Boolean),
            ))
        }
        Predicate::And(children) | Predicate::Or(children) => {
            if !(2..=16).contains(&children.len()) {
                return Err(BindError::InvalidQuery);
            }
            let mut bound = Vec::with_capacity(children.len());
            for child in children {
                let (child, scalar_type) = bind_predicate(child, columns)?;
                if scalar_type != Some(ScalarType::Boolean) {
                    return Err(BindError::InvalidQuery);
                }
                bound.push(child);
            }
            let node = match predicate {
                Predicate::And(_) => BoundPredicate::And(bound),
                Predicate::Or(_) => BoundPredicate::Or(bound),
                _ => return Err(BindError::InvalidQuery),
            };
            Ok((node, Some(ScalarType::Boolean)))
        }
        Predicate::Not(child) => {
            let (child, scalar_type) = bind_predicate(child, columns)?;
            if scalar_type != Some(ScalarType::Boolean) {
                return Err(BindError::InvalidQuery);
            }
            Ok((
                BoundPredicate::Not(Box::new(child)),
                Some(ScalarType::Boolean),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put_name(bytes: &mut Vec<u8>, name: &str) {
        bytes.push(name.len() as u8);
        bytes.extend_from_slice(name.as_bytes());
    }

    fn request(predicate: &[u8], projection: &str) -> Vec<u8> {
        let mut bytes = vec![0, 1, 1, 0];
        put_name(&mut bytes, "public");
        put_name(&mut bytes, "items");
        bytes.extend_from_slice(&1u16.to_be_bytes());
        put_name(&mut bytes, projection);
        bytes.extend_from_slice(&(predicate.len() as u32).to_be_bytes());
        bytes.extend_from_slice(predicate);
        bytes.push(0);
        bytes
    }

    fn ordered_select_request(projections: &[&str], order_key: &str) -> Vec<u8> {
        let mut bytes = vec![
            0,
            crate::query_abi::OPTIMIZER_REQUEST_VERSION as u8,
            crate::query_abi::OPTIMIZER_ORDERED_SELECT_STATEMENT,
            0,
        ];
        put_name(&mut bytes, "public");
        put_name(&mut bytes, "items");
        bytes.extend_from_slice(&(projections.len() as u16).to_be_bytes());
        for projection in projections {
            put_name(&mut bytes, projection);
        }
        put_name(&mut bytes, order_key);
        bytes.extend_from_slice(&[1, 1]);
        bytes.extend_from_slice(&9u32.to_be_bytes());
        bytes
    }

    fn delete_request(predicate: &[u8]) -> Vec<u8> {
        let mut bytes = vec![0, 1, 2, 0];
        put_name(&mut bytes, "public");
        put_name(&mut bytes, "items");
        bytes.extend_from_slice(&(predicate.len() as u32).to_be_bytes());
        bytes.extend_from_slice(predicate);
        bytes
    }

    fn update_request(assignments: &[(&str, u8, Option<u32>)], predicate: &[u8]) -> Vec<u8> {
        let mut bytes = vec![0, 1, crate::query_abi::OPTIMIZER_UPDATE_STATEMENT, 0];
        put_name(&mut bytes, "public");
        put_name(&mut bytes, "items");
        bytes.extend_from_slice(&(assignments.len() as u16).to_be_bytes());
        for (name, kind, parameter) in assignments {
            put_name(&mut bytes, name);
            bytes.push(*kind);
            if let Some(parameter) = parameter {
                bytes.extend_from_slice(&parameter.to_be_bytes());
            }
        }
        bytes.extend_from_slice(&(predicate.len() as u32).to_be_bytes());
        bytes.extend_from_slice(predicate);
        bytes
    }

    fn join_select_request() -> Vec<u8> {
        let mut bytes = vec![0, 1, crate::query_abi::OPTIMIZER_JOIN_SELECT_STATEMENT, 0];
        put_name(&mut bytes, "public");
        put_name(&mut bytes, "lefts");
        put_name(&mut bytes, "rights");
        bytes.extend_from_slice(&2u16.to_be_bytes());
        bytes.push(0);
        put_name(&mut bytes, "id");
        bytes.push(1);
        put_name(&mut bytes, "value");
        bytes.push(2);
        put_name(&mut bytes, "id");
        put_name(&mut bytes, "id");
        put_name(&mut bytes, "active");
        put_name(&mut bytes, "active");
        bytes.extend_from_slice(&[1, 0, 0, 0, 5]);
        bytes
    }

    fn group_count_request() -> Vec<u8> {
        let mut bytes = vec![0, 1, crate::query_abi::OPTIMIZER_GROUP_COUNT_STATEMENT, 0];
        put_name(&mut bytes, "public");
        put_name(&mut bytes, "items");
        put_name(&mut bytes, "active");
        bytes.extend_from_slice(&[1, 0, 0, 0, 4]);
        bytes
    }

    fn composite_group_count_request(keys: &[&str]) -> Vec<u8> {
        let mut bytes = vec![0, crate::query_abi::OPTIMIZER_REQUEST_VERSION as u8];
        bytes.push(crate::query_abi::OPTIMIZER_COMPOSITE_GROUP_COUNT_STATEMENT);
        bytes.push(0);
        put_name(&mut bytes, "public");
        put_name(&mut bytes, "items");
        bytes.push(keys.len() as u8);
        for key in keys {
            put_name(&mut bytes, key);
        }
        bytes.extend_from_slice(&[1, 0, 0, 0, 4]);
        bytes
    }

    fn relation() -> AuthorizedPlanRelation {
        AuthorizedPlanRelation {
            schema_id: [0x10; 16],
            schema_version: 3,
            table_epoch: 4,
            relation: AuthorizedRelationDescription {
                relation_id: [0x11; 16],
                name: "items".to_owned(),
                columns: vec![
                    VisibleColumn {
                        column_id: [0x21; 16],
                        name: "id".to_owned(),
                        ordinal: 0,
                        scalar_type: ScalarType::Integer,
                        nullable: false,
                    },
                    VisibleColumn {
                        column_id: [0x22; 16],
                        name: "active".to_owned(),
                        ordinal: 1,
                        scalar_type: ScalarType::Boolean,
                        nullable: true,
                    },
                ],
            },
        }
    }

    #[test]
    fn binds_ordered_select_outputs_and_hidden_order_key_from_authorized_shape() {
        let bound = bind_ordered_select_request(
            &ordered_select_request(&["id"], "active"),
            Some(relation()),
        )
        .unwrap();
        assert_eq!(bound.schema_id, [0x10; 16]);
        assert_eq!(bound.relation_id, [0x11; 16]);
        assert_eq!(bound.projections[0].column_id, [0x21; 16]);
        assert_eq!(bound.order_key.column_id, [0x22; 16]);
        assert!(bound.descending);
        assert_eq!(bound.limit, Some(9));
        assert_eq!(
            bind_ordered_select_request(
                &ordered_select_request(&["id", "id"], "active"),
                Some(relation()),
            ),
            Err(BindError::InvalidQuery)
        );
    }

    fn join_relation(name: &str, relation_id: u8) -> AuthorizedPlanRelation {
        AuthorizedPlanRelation {
            schema_id: [0x10; 16],
            schema_version: 3,
            table_epoch: u64::from(relation_id),
            relation: AuthorizedRelationDescription {
                relation_id: [relation_id; 16],
                name: name.to_owned(),
                columns: vec![
                    VisibleColumn {
                        column_id: [relation_id + 0x10; 16],
                        name: "id".to_owned(),
                        ordinal: 0,
                        scalar_type: ScalarType::Integer,
                        nullable: false,
                    },
                    VisibleColumn {
                        column_id: [relation_id + 0x11; 16],
                        name: "active".to_owned(),
                        ordinal: 1,
                        scalar_type: ScalarType::Boolean,
                        nullable: false,
                    },
                    VisibleColumn {
                        column_id: [relation_id + 0x12; 16],
                        name: "value".to_owned(),
                        ordinal: 2,
                        scalar_type: ScalarType::Text,
                        nullable: true,
                    },
                ],
            },
        }
    }

    #[test]
    fn binds_composite_join_to_both_authorized_shapes() {
        let bound = bind_join_select_request(
            &join_select_request(),
            Some(join_relation("lefts", 0x31)),
            Some(join_relation("rights", 0x41)),
        )
        .unwrap();
        assert_eq!(bound.schema_name, "public");
        assert_eq!(bound.schema_id, [0x10; 16]);
        assert_eq!(bound.schema_version, 3);
        assert_eq!(bound.left_relation.relation_id, [0x31; 16]);
        assert_eq!(bound.right_relation.relation_id, [0x41; 16]);
        assert_eq!(
            bound.projections,
            [
                BoundJoinProjection {
                    relation_side: JoinRelationSide::Left,
                    column: BoundColumn {
                        column_id: [0x41; 16],
                        ordinal: 0,
                        scalar_type: ScalarType::Integer,
                        nullable: false,
                    },
                },
                BoundJoinProjection {
                    relation_side: JoinRelationSide::Right,
                    column: BoundColumn {
                        column_id: [0x53; 16],
                        ordinal: 2,
                        scalar_type: ScalarType::Text,
                        nullable: true,
                    },
                },
            ]
        );
        assert_eq!(bound.join_keys.len(), 2);
        assert_eq!(bound.join_keys[0].left_column.ordinal, 0);
        assert_eq!(bound.join_keys[0].right_column.ordinal, 0);
        assert_eq!(bound.join_keys[1].left_column.ordinal, 1);
        assert_eq!(bound.join_keys[1].right_column.ordinal, 1);
        assert_eq!(bound.limit, Some(5));
    }

    #[test]
    fn binds_group_count_key_to_authorized_column_shape() {
        let bound = bind_group_count_request(&group_count_request(), Some(relation())).unwrap();
        assert_eq!(bound.schema_name, "public");
        assert_eq!(bound.schema_id, [0x10; 16]);
        assert_eq!(bound.schema_version, 3);
        assert_eq!(bound.table_epoch, 4);
        assert_eq!(bound.relation_id, [0x11; 16]);
        assert_eq!(bound.relation_name, "items");
        assert_eq!(
            bound.group_key,
            BoundColumn {
                column_id: [0x22; 16],
                ordinal: 1,
                scalar_type: ScalarType::Boolean,
                nullable: true,
            }
        );
        assert_eq!(bound.limit, Some(4));
    }

    #[test]
    fn binds_composite_group_count_keys_to_authorized_shape_in_sql_order() {
        let bound = bind_composite_group_count_request(
            &composite_group_count_request(&["active", "id"]),
            Some(relation()),
        )
        .unwrap();
        assert_eq!(bound.schema_id, [0x10; 16]);
        assert_eq!(bound.relation_id, [0x11; 16]);
        assert_eq!(
            bound
                .group_keys
                .iter()
                .map(|key| key.ordinal)
                .collect::<Vec<_>>(),
            [1, 0]
        );
        assert_eq!(bound.limit, Some(4));
        assert_eq!(
            bind_composite_group_count_request(
                &composite_group_count_request(&["active", "ACTIVE"]),
                Some(relation()),
            ),
            Err(BindError::InvalidQuery)
        );
    }

    #[test]
    fn join_binding_rejects_incompatible_keys_and_cross_schema_shapes() {
        let request = join_select_request();
        let mut right = join_relation("rights", 0x41);
        right.relation.columns[1].scalar_type = ScalarType::Integer;
        assert_eq!(
            bind_join_select_request(&request, Some(join_relation("lefts", 0x31)), Some(right),),
            Err(BindError::InvalidQuery)
        );
        let mut right = join_relation("rights", 0x41);
        right.schema_id = [0x77; 16];
        assert_eq!(
            bind_join_select_request(&request, Some(join_relation("lefts", 0x31)), Some(right),),
            Err(BindError::InvalidQuery)
        );
        assert_eq!(
            bind_join_select_request(&request, None, Some(join_relation("rights", 0x41))),
            Err(BindError::RelationUnavailable)
        );
    }

    #[test]
    fn binds_only_authorized_relation_shape_and_stable_column_references() {
        let predicate = [0x20, 0, 0x10, 2, b'i', b'd', 0x02, 0, 0, 0, 0, 0, 0, 0, 7];
        let plan = bind_select_request(&request(&predicate, "ID"), Some(relation())).unwrap();
        assert_eq!(plan.schema_id, [0x10; 16]);
        assert_eq!(plan.schema_version, 3);
        assert_eq!(plan.table_epoch, 4);
        assert_eq!(plan.relation_id, [0x11; 16]);
        assert_eq!(plan.relation_name, "items");
        assert_eq!(
            plan.projections,
            [BoundColumn {
                column_id: [0x21; 16],
                ordinal: 0,
                scalar_type: ScalarType::Integer,
                nullable: false,
            }]
        );
        assert_eq!(
            plan.predicate,
            Some(BoundPredicate::Compare {
                operator: Comparison::Equal,
                left: Box::new(BoundPredicate::Column(BoundColumn {
                    column_id: [0x21; 16],
                    ordinal: 0,
                    scalar_type: ScalarType::Integer,
                    nullable: false,
                })),
                right: Box::new(BoundPredicate::Integer(7)),
            })
        );
    }

    #[test]
    fn binds_delete_predicate_to_the_authorized_relation_without_a_handle() {
        let predicate = [0x20, 0, 0x10, 2, b'i', b'd', 0x02, 0, 0, 0, 0, 0, 0, 0, 7];
        let bound = bind_delete_request(&delete_request(&predicate), Some(relation())).unwrap();
        assert_eq!(bound.schema_id, [0x10; 16]);
        assert_eq!(bound.schema_version, 3);
        assert_eq!(bound.table_epoch, 4);
        assert_eq!(bound.relation_id, [0x11; 16]);
        assert_eq!(bound.relation_name, "items");
        assert_eq!(
            bound.predicate,
            BoundPredicate::Compare {
                operator: Comparison::Equal,
                left: Box::new(BoundPredicate::Column(BoundColumn {
                    column_id: [0x21; 16],
                    ordinal: 0,
                    scalar_type: ScalarType::Integer,
                    nullable: false,
                })),
                right: Box::new(BoundPredicate::Integer(7)),
            }
        );
    }

    #[test]
    fn binds_update_targets_values_and_predicate_to_authorized_metadata() {
        let predicate = [0x20, 0, 0x10, 2, b'i', b'd', 0x02, 0, 0, 0, 0, 0, 0, 0, 7];
        let request = update_request(&[("active", 0, Some(2)), ("id", 0, Some(1))], &predicate);
        let bound = bind_update_request(&request, Some(relation())).unwrap();
        assert_eq!(bound.schema_name, "public");
        assert_eq!(bound.schema_id, [0x10; 16]);
        assert_eq!(bound.schema_version, 3);
        assert_eq!(bound.table_epoch, 4);
        assert_eq!(bound.relation_id, [0x11; 16]);
        assert_eq!(bound.relation_name, "items");
        assert_eq!(
            bound.assignments,
            [
                BoundUpdateAssignment {
                    column: BoundColumn {
                        column_id: [0x22; 16],
                        ordinal: 1,
                        scalar_type: ScalarType::Boolean,
                        nullable: true,
                    },
                    value: BoundUpdateValue::Parameter(2),
                },
                BoundUpdateAssignment {
                    column: BoundColumn {
                        column_id: [0x21; 16],
                        ordinal: 0,
                        scalar_type: ScalarType::Integer,
                        nullable: false,
                    },
                    value: BoundUpdateValue::Parameter(1),
                },
            ]
        );
        assert_eq!(
            bound.predicate,
            BoundPredicate::Compare {
                operator: Comparison::Equal,
                left: Box::new(BoundPredicate::Column(BoundColumn {
                    column_id: [0x21; 16],
                    ordinal: 0,
                    scalar_type: ScalarType::Integer,
                    nullable: false,
                })),
                right: Box::new(BoundPredicate::Integer(7)),
            }
        );
    }

    #[test]
    fn update_binding_rejects_duplicates_unknown_columns_and_nullability_violations() {
        let predicate = [0x01, 1];
        let duplicate = update_request(&[("active", 0, Some(1)), ("ACTIVE", 1, None)], &predicate);
        assert_eq!(
            bind_update_request(&duplicate, Some(relation())),
            Err(BindError::InvalidQuery)
        );
        let unknown = update_request(&[("hidden", 0, Some(1))], &predicate);
        assert_eq!(
            bind_update_request(&unknown, Some(relation())),
            Err(BindError::InvalidQuery)
        );
        let null_to_required = update_request(&[("id", 1, None)], &predicate);
        assert_eq!(
            bind_update_request(&null_to_required, Some(relation())),
            Err(BindError::InvalidQuery)
        );
        let nullable_null = update_request(&[("active", 1, None)], &predicate);
        assert_eq!(
            bind_update_request(&nullable_null, Some(relation()))
                .unwrap()
                .assignments[0]
                .value,
            BoundUpdateValue::Null
        );
        let no_relation = update_request(&[("active", 1, None)], &predicate);
        assert_eq!(
            bind_update_request(&no_relation, None),
            Err(BindError::RelationUnavailable)
        );
        let non_boolean_where =
            update_request(&[("active", 1, None)], &[0x02, 0, 0, 0, 0, 0, 0, 0, 7]);
        assert_eq!(
            bind_update_request(&non_boolean_where, Some(relation())),
            Err(BindError::InvalidQuery)
        );
    }

    #[test]
    fn delete_binding_is_non_disclosing_and_requires_a_boolean_predicate() {
        let constant_true = delete_request(&[0x01, 1]);
        assert_eq!(
            bind_delete_request(&constant_true, None),
            Err(BindError::RelationUnavailable)
        );
        let mut mismatched = relation();
        mismatched.relation.name = "secret".to_owned();
        assert_eq!(
            bind_delete_request(&constant_true, Some(mismatched)),
            Err(BindError::RelationUnavailable)
        );

        let mut integer = vec![0x02];
        integer.extend_from_slice(&7i64.to_be_bytes());
        assert_eq!(
            bind_delete_request(&delete_request(&integer), Some(relation())),
            Err(BindError::InvalidQuery)
        );
        let unknown_column = [
            0x20, 0, 0x10, 8, b's', b'e', b'c', b'r', b'e', b't', b'_', b'x', 0x02, 0, 0, 0, 0, 0,
            0, 0, 7,
        ];
        assert_eq!(
            bind_delete_request(&delete_request(&unknown_column), Some(relation())),
            Err(BindError::InvalidQuery)
        );
    }

    #[test]
    fn relation_absence_and_name_mismatch_are_non_disclosing() {
        let bytes = request(&[], "id");
        assert_eq!(
            bind_select_request(&bytes, None),
            Err(BindError::RelationUnavailable)
        );
        let mut wrong_relation = relation();
        wrong_relation.relation.name = "secret".to_owned();
        assert_eq!(
            bind_select_request(&bytes, Some(wrong_relation)),
            Err(BindError::RelationUnavailable)
        );
    }

    #[test]
    fn rejects_unknown_columns_and_type_incompatible_predicates() {
        let unknown_column = request(&[], "secret_column");
        assert_eq!(
            bind_select_request(&unknown_column, Some(relation())),
            Err(BindError::InvalidQuery)
        );
        let incompatible = [0x20, 0, 0x10, 2, b'i', b'd', 0x01, 1];
        assert_eq!(
            bind_select_request(&request(&incompatible, "id"), Some(relation())),
            Err(BindError::InvalidQuery)
        );
        let non_boolean_root = [0x10, 2, b'i', b'd'];
        assert_eq!(
            bind_select_request(&request(&non_boolean_root, "id"), Some(relation())),
            Err(BindError::InvalidQuery)
        );
    }
}
