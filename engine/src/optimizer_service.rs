//! Invocation-bound structured query planning service.

use crate::metadata_discovery::AuthorizedPlanRelation;
use crate::optimizer_binding::{
    MetadataBindError, bind_delete_request, bind_delete_request_via_metadata, bind_select_request,
    bind_select_request_via_metadata,
};
use crate::optimizer_plan::{
    PlanError, RelationReference, encode_delete_plan_v1, encode_plan_v1, plan_delete_v1,
    plan_select_v1,
};
use crate::transaction::PlannerMetadataConnection;
use std::fmt;

/// Failure returned to the caller of the optimizer service. Relation lookup
/// failures intentionally do not expose whether a name exists outside the
/// invocation's authorized metadata view.
#[derive(Debug)]
pub enum OptimizerError {
    Bind(MetadataBindError),
    Plan(PlanError),
}

impl fmt::Display for OptimizerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "optimizer service error: {self:?}")
    }
}

impl std::error::Error for OptimizerError {}

impl From<MetadataBindError> for OptimizerError {
    fn from(error: MetadataBindError) -> Self {
        Self::Bind(error)
    }
}

impl From<PlanError> for OptimizerError {
    fn from(error: PlanError) -> Self {
        Self::Plan(error)
    }
}

/// Resolve, bind, lower, and encode one canonical SELECT request using only
/// the metadata authority already attached to this invocation. The returned
/// bytes are data; the executor must independently resolve and authorize the
/// relation before execution.
pub fn optimize_select_v1(
    request: &[u8],
    connection: &mut PlannerMetadataConnection<'_, '_>,
) -> Result<Vec<u8>, OptimizerError> {
    let bound = bind_select_request_via_metadata(request, connection)?;
    encode_bound_select(bound)
}

/// Optimize a request using an authorized stable relation description
/// obtained through a separate planner IPC connection. The relation record
/// carries no storage capability and is still validated during binding.
pub fn optimize_select_v1_with_relation(
    request: &[u8],
    relation: Option<AuthorizedPlanRelation>,
) -> Result<Vec<u8>, OptimizerError> {
    let bound = bind_select_request(request, relation)
        .map_err(|error| OptimizerError::Bind(MetadataBindError::Bind(error)))?;
    encode_bound_select(bound)
}

/// Resolve, bind, lower, and encode one canonical DELETE request using only
/// the invocation-bound planner metadata connection. Storage still owns
/// authorization and every eventual mutation.
pub fn optimize_delete_v1(
    request: &[u8],
    connection: &mut PlannerMetadataConnection<'_, '_>,
) -> Result<Vec<u8>, OptimizerError> {
    let bound = bind_delete_request_via_metadata(request, connection)?;
    encode_bound_delete(bound)
}

/// Optimize a DELETE request using the authorized stable relation description
/// from the planner channel. This description grants no executor capability.
pub fn optimize_delete_v1_with_relation(
    request: &[u8],
    relation: Option<AuthorizedPlanRelation>,
) -> Result<Vec<u8>, OptimizerError> {
    let bound = bind_delete_request(request, relation)
        .map_err(|error| OptimizerError::Bind(MetadataBindError::Bind(error)))?;
    encode_bound_delete(bound)
}

fn encode_bound_select(
    bound: crate::optimizer_binding::BoundSelect,
) -> Result<Vec<u8>, OptimizerError> {
    let plan = plan_select_v1(bound)?;
    Ok(encode_plan_v1(&plan)?)
}

fn encode_bound_delete(
    bound: crate::optimizer_binding::BoundDelete,
) -> Result<Vec<u8>, OptimizerError> {
    let plan = plan_delete_v1(
        RelationReference {
            schema_name: bound.schema_name,
            schema_id: bound.schema_id,
            schema_version: bound.schema_version,
            relation_name: bound.relation_name,
            relation_id: bound.relation_id,
            table_epoch: bound.table_epoch,
        },
        bound.predicate,
    )?;
    Ok(encode_delete_plan_v1(&plan)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata_discovery::{AuthorizedRelationDescription, VisibleColumn};
    use crate::optimizer_plan::decode_delete_plan_v1;
    use crate::tuple::ScalarType;

    fn delete_request() -> Vec<u8> {
        let mut bytes = vec![0, 1, crate::query_abi::OPTIMIZER_DELETE_STATEMENT, 0];
        for name in ["public", "items"] {
            bytes.push(name.len() as u8);
            bytes.extend_from_slice(name.as_bytes());
        }
        let predicate = [0x20, 0, 0x10, 2, b'i', b'd', 0x02, 0, 0, 0, 0, 0, 0, 0, 7];
        bytes.extend_from_slice(&(predicate.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&predicate);
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
                columns: vec![VisibleColumn {
                    column_id: [0x21; 16],
                    name: "id".to_owned(),
                    ordinal: 0,
                    scalar_type: ScalarType::Integer,
                    nullable: false,
                }],
            },
        }
    }

    #[test]
    fn authorized_delete_request_produces_only_a_bounded_delete_plan() {
        let encoded =
            optimize_delete_v1_with_relation(&delete_request(), Some(relation())).unwrap();
        let plan = decode_delete_plan_v1(&encoded).unwrap();
        assert_eq!(plan.relation.schema_id, [0x10; 16]);
        assert_eq!(plan.relation.relation_id, [0x11; 16]);
        assert_eq!(plan.scan_columns[0].column_id, [0x21; 16]);
        assert_eq!(
            plan.predicate,
            crate::optimizer_binding::BoundPredicate::Compare {
                operator: crate::optimizer_request::Comparison::Equal,
                left: Box::new(crate::optimizer_binding::BoundPredicate::Column(
                    crate::optimizer_binding::BoundColumn {
                        column_id: [0x21; 16],
                        ordinal: 0,
                        scalar_type: ScalarType::Integer,
                        nullable: false,
                    }
                )),
                right: Box::new(crate::optimizer_binding::BoundPredicate::Integer(7)),
            }
        );
    }
}
