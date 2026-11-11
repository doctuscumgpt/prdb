//! Protocol loop used by the isolated optimizer executable.

use crate::executor_ipc::ResolveRelationRequest;
use crate::ipc::{DispatchError, IpcFrame, Operation, ServiceRole, authorize_operation};
use crate::optimizer_ipc::{
    MAX_OPTIMIZER_REQUESTS_PER_CONNECTION, OptimizerPayloadError, OptimizerResponse,
    OptimizerStatus, status_for_error,
};
use crate::optimizer_plan::MAX_PLAN_BYTES;
use crate::planner_ipc::{
    DescribePlanRelationResponse, MAX_PLANNER_REQUESTS_PER_CONNECTION, PlannerRequestError,
};
use std::collections::HashSet;
use std::io::{Read, Write};

#[derive(Debug)]
pub enum OptimizerProcessError {
    Frame(crate::ipc::FrameError),
    Dispatch(DispatchError),
    PlannerClosed,
    PlannerResponseMismatch,
    PlannerPayload(PlannerRequestError),
    Response(OptimizerPayloadError),
    RequestLimitExceeded,
    DuplicateRequestId,
}

impl std::fmt::Display for OptimizerProcessError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "optimizer process protocol error: {self:?}")
    }
}

impl std::error::Error for OptimizerProcessError {}

/// Serve structured requests using only two inherited Unix streams: an
/// executor-submission endpoint and an invocation-bound planner endpoint.
/// The optimizer never opens storage files or receives executor handles.
pub fn serve_optimizer_process<S: Read + Write, P: Read + Write>(
    submission: &mut S,
    planner: &mut P,
) -> Result<(), OptimizerProcessError> {
    let mut request_ids = HashSet::new();
    // Planner IPC is shared with direct SDK planner requests. Keep optimizer
    // metadata lookups in a disjoint range and give every relation its own ID.
    let mut next_planner_request_id = MAX_PLANNER_REQUESTS_PER_CONNECTION as u64 + 1;
    loop {
        let request = match crate::ipc::read_frame(submission) {
            Ok(Some(request)) => request,
            Ok(None) => return Ok(()),
            Err(error) => return Err(OptimizerProcessError::Frame(error)),
        };
        if request_ids.len() >= MAX_OPTIMIZER_REQUESTS_PER_CONNECTION {
            return Err(OptimizerProcessError::RequestLimitExceeded);
        }
        if !request_ids.insert(request.request_id) {
            return Err(OptimizerProcessError::DuplicateRequestId);
        }
        let operation =
            Operation::from_code(request.operation).map_err(OptimizerProcessError::Dispatch)?;
        authorize_operation(ServiceRole::OptimizerSubmission, operation)
            .map_err(OptimizerProcessError::Dispatch)?;
        if !matches!(
            operation,
            Operation::OptimizeSelect
                | Operation::OptimizeDelete
                | Operation::OptimizeUpdate
                | Operation::OptimizeJoinSelect
        ) {
            return Err(OptimizerProcessError::Dispatch(
                DispatchError::OperationNotAllowed {
                    role: ServiceRole::OptimizerSubmission,
                    operation,
                },
            ));
        }

        let decoded_names = match operation {
            Operation::OptimizeSelect => {
                crate::optimizer_request::decode_select_request(&request.payload)
                    .map(|decoded| vec![(decoded.schema_name, decoded.relation_name)])
            }
            Operation::OptimizeDelete => {
                crate::optimizer_request::decode_delete_request(&request.payload)
                    .map(|decoded| vec![(decoded.schema_name, decoded.relation_name)])
            }
            Operation::OptimizeUpdate => {
                crate::optimizer_request::decode_update_request(&request.payload)
                    .map(|decoded| vec![(decoded.schema_name, decoded.relation_name)])
            }
            Operation::OptimizeJoinSelect => crate::optimizer_request::decode_join_select_request(
                &request.payload,
            )
            .map(|decoded| {
                vec![
                    (decoded.schema_name.clone(), decoded.left_relation_name),
                    (decoded.schema_name, decoded.right_relation_name),
                ]
            }),
            _ => unreachable!("optimizer operation was checked above"),
        };
        let relation_names = match decoded_names {
            Ok(names) => names,
            Err(_) => {
                write_submission_response(
                    submission,
                    request.operation,
                    request.request_id,
                    OptimizerResponse::failure(OptimizerStatus::InvalidRequest)
                        .map_err(OptimizerProcessError::Response)?,
                )?;
                continue;
            }
        };
        let mut relations = Vec::with_capacity(relation_names.len());
        for (schema_name, relation_name) in relation_names {
            let planner_request_id = next_planner_request_id;
            next_planner_request_id = next_planner_request_id
                .checked_add(1)
                .ok_or(OptimizerProcessError::RequestLimitExceeded)?;
            let planner_request = IpcFrame {
                operation: Operation::DescribePlanRelation as u16,
                request_id: planner_request_id,
                payload: ResolveRelationRequest {
                    schema_name,
                    relation_name,
                }
                .encode()
                .map_err(|error| OptimizerProcessError::PlannerPayload(error.into()))?,
            };
            authorize_operation(ServiceRole::Planner, Operation::DescribePlanRelation)
                .map_err(OptimizerProcessError::Dispatch)?;
            crate::ipc::write_frame(planner, &planner_request)
                .map_err(OptimizerProcessError::Frame)?;
            let planner_response = crate::ipc::read_frame(planner)
                .map_err(OptimizerProcessError::Frame)?
                .ok_or(OptimizerProcessError::PlannerClosed)?;
            if planner_response.operation != planner_request.operation
                || planner_response.request_id != planner_request.request_id
            {
                return Err(OptimizerProcessError::PlannerResponseMismatch);
            }
            relations.push(
                DescribePlanRelationResponse::decode(&planner_response.payload)
                    .map_err(OptimizerProcessError::PlannerPayload)?
                    .relation,
            );
        }
        let optimized = match operation {
            Operation::OptimizeSelect => {
                crate::optimizer_service::optimize_select_v1_with_relation(
                    &request.payload,
                    relations.remove(0),
                )
            }
            Operation::OptimizeDelete => {
                crate::optimizer_service::optimize_delete_v1_with_relation(
                    &request.payload,
                    relations.remove(0),
                )
            }
            Operation::OptimizeUpdate => {
                crate::optimizer_service::optimize_update_v1_with_relation(
                    &request.payload,
                    relations.remove(0),
                )
            }
            Operation::OptimizeJoinSelect => {
                crate::optimizer_service::optimize_join_select_v1_with_relations(
                    &request.payload,
                    relations.remove(0),
                    relations.remove(0),
                )
            }
            _ => unreachable!("optimizer operation was checked above"),
        };
        let response = match optimized {
            Ok(plan) if plan.len() <= MAX_PLAN_BYTES => OptimizerResponse::success(plan),
            Ok(_) => OptimizerResponse::failure(OptimizerStatus::Internal),
            Err(error) => OptimizerResponse::failure(status_for_error(&error)),
        }
        .map_err(OptimizerProcessError::Response)?;
        write_submission_response(submission, request.operation, request.request_id, response)?;
    }
}

fn write_submission_response<S: Write>(
    submission: &mut S,
    operation: u16,
    request_id: u64,
    response: OptimizerResponse,
) -> Result<(), OptimizerProcessError> {
    let frame = IpcFrame {
        operation,
        request_id,
        payload: response.encode().map_err(OptimizerProcessError::Response)?,
    };
    crate::ipc::write_frame(submission, &frame).map_err(OptimizerProcessError::Frame)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata_discovery::{
        AuthorizedPlanRelation, AuthorizedRelationDescription, VisibleColumn,
    };
    use crate::optimizer_ipc::OptimizerStatus;
    use crate::optimizer_plan::{PlanNode, decode_plan_v1};
    use crate::planner_ipc::DescribePlanRelationResponse;
    use crate::tuple::ScalarType;
    use std::net::Shutdown;
    use std::os::unix::net::UnixStream;

    fn select_request() -> Vec<u8> {
        let mut bytes = vec![0, 1, 1, 0];
        for name in ["public", "items"] {
            bytes.push(name.len() as u8);
            bytes.extend_from_slice(name.as_bytes());
        }
        bytes.extend_from_slice(&1u16.to_be_bytes());
        bytes.push(4);
        bytes.extend_from_slice(b"name");
        bytes.extend_from_slice(&0u32.to_be_bytes());
        bytes.push(0);
        bytes
    }

    fn delete_request() -> Vec<u8> {
        let mut bytes = vec![0, 1, 2, 0];
        for name in ["public", "items"] {
            bytes.push(name.len() as u8);
            bytes.extend_from_slice(name.as_bytes());
        }
        bytes.extend_from_slice(&2u32.to_be_bytes());
        bytes.extend_from_slice(&[0x01, 1]);
        bytes
    }

    fn update_request() -> Vec<u8> {
        let mut bytes = vec![0, 1, crate::query_abi::OPTIMIZER_UPDATE_STATEMENT, 0];
        for name in ["public", "items"] {
            bytes.push(name.len() as u8);
            bytes.extend_from_slice(name.as_bytes());
        }
        bytes.extend_from_slice(&1u16.to_be_bytes());
        bytes.push(6);
        bytes.extend_from_slice(b"active");
        bytes.push(0);
        bytes.extend_from_slice(&1u32.to_be_bytes());
        let predicate = [0x20, 0, 0x10, 2, b'i', b'd', 0x02, 0, 0, 0, 0, 0, 0, 0, 7];
        bytes.extend_from_slice(&(predicate.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&predicate);
        bytes
    }

    fn join_select_request() -> Vec<u8> {
        let mut bytes = vec![0, 1, crate::query_abi::OPTIMIZER_JOIN_SELECT_STATEMENT, 0];
        for name in ["public", "lefts", "rights"] {
            bytes.push(name.len() as u8);
            bytes.extend_from_slice(name.as_bytes());
        }
        bytes.extend_from_slice(&2u16.to_be_bytes());
        for (side, name) in [(0, "value"), (1, "value")] {
            bytes.push(side);
            bytes.push(name.len() as u8);
            bytes.extend_from_slice(name.as_bytes());
        }
        bytes.push(2);
        for name in ["id", "id", "active", "active"] {
            bytes.push(name.len() as u8);
            bytes.extend_from_slice(name.as_bytes());
        }
        bytes.extend_from_slice(&[1, 0, 0, 0, 5]);
        bytes
    }

    #[test]
    fn process_protocol_uses_planner_metadata_and_returns_a_validated_plan() {
        let (mut submission_client, mut submission_server) = UnixStream::pair().unwrap();
        let (mut planner_client, mut planner_server) = UnixStream::pair().unwrap();
        std::thread::scope(|scope| {
            let planner_thread = scope.spawn(|| {
                let request = crate::ipc::read_frame(&mut planner_server)
                    .unwrap()
                    .expect("optimizer must resolve plan relation metadata");
                assert_eq!(request.operation, Operation::DescribePlanRelation as u16);
                assert_eq!(request.request_id, 257);
                let relation = AuthorizedPlanRelation {
                    schema_id: [1; 16],
                    schema_version: 3,
                    table_epoch: 7,
                    relation: AuthorizedRelationDescription {
                        relation_id: [2; 16],
                        name: "items".to_owned(),
                        columns: vec![VisibleColumn {
                            column_id: [3; 16],
                            name: "name".to_owned(),
                            ordinal: 0,
                            scalar_type: ScalarType::Text,
                            nullable: false,
                        }],
                    },
                };
                crate::ipc::write_frame(
                    &mut planner_server,
                    &IpcFrame {
                        operation: request.operation,
                        request_id: request.request_id,
                        payload: DescribePlanRelationResponse {
                            relation: Some(relation),
                        }
                        .encode()
                        .unwrap(),
                    },
                )
                .unwrap();
            });
            let optimizer_thread = scope
                .spawn(|| serve_optimizer_process(&mut submission_server, &mut planner_client));

            crate::ipc::write_frame(
                &mut submission_client,
                &IpcFrame {
                    operation: Operation::OptimizeSelect as u16,
                    request_id: 9,
                    payload: select_request(),
                },
            )
            .unwrap();
            let response = crate::ipc::read_frame(&mut submission_client)
                .unwrap()
                .expect("optimizer must return a plan");
            assert_eq!(response.request_id, 9);
            let response = OptimizerResponse::decode(&response.payload).unwrap();
            assert_eq!(response.status, OptimizerStatus::Success);
            let plan = decode_plan_v1(response.plan.as_deref().unwrap()).unwrap();
            assert_eq!(plan.relation.schema_id, [1; 16]);
            assert_eq!(plan.relation.schema_version, 3);
            assert_eq!(plan.relation.table_epoch, 7);
            assert!(
                matches!(plan.nodes.first(), Some(PlanNode::Scan { columns }) if columns[0].column_id == [3; 16])
            );
            submission_client.shutdown(Shutdown::Write).unwrap();
            planner_thread.join().unwrap();
            optimizer_thread.join().unwrap().unwrap();
        });
    }

    #[test]
    fn process_protocol_dispatches_delete_and_returns_a_validated_delete_plan() {
        let (mut submission_client, mut submission_server) = UnixStream::pair().unwrap();
        let (mut planner_client, mut planner_server) = UnixStream::pair().unwrap();
        std::thread::scope(|scope| {
            let planner_thread = scope.spawn(|| {
                let request = crate::ipc::read_frame(&mut planner_server)
                    .unwrap()
                    .expect("optimizer must resolve DELETE relation metadata");
                assert_eq!(request.operation, Operation::DescribePlanRelation as u16);
                assert_eq!(request.request_id, 257);
                let relation = AuthorizedPlanRelation {
                    schema_id: [1; 16],
                    schema_version: 3,
                    table_epoch: 7,
                    relation: AuthorizedRelationDescription {
                        relation_id: [2; 16],
                        name: "items".to_owned(),
                        columns: vec![VisibleColumn {
                            column_id: [3; 16],
                            name: "id".to_owned(),
                            ordinal: 0,
                            scalar_type: ScalarType::Integer,
                            nullable: false,
                        }],
                    },
                };
                crate::ipc::write_frame(
                    &mut planner_server,
                    &IpcFrame {
                        operation: request.operation,
                        request_id: request.request_id,
                        payload: DescribePlanRelationResponse {
                            relation: Some(relation),
                        }
                        .encode()
                        .unwrap(),
                    },
                )
                .unwrap();
            });
            let optimizer_thread = scope
                .spawn(|| serve_optimizer_process(&mut submission_server, &mut planner_client));

            crate::ipc::write_frame(
                &mut submission_client,
                &IpcFrame {
                    operation: Operation::OptimizeDelete as u16,
                    request_id: 10,
                    payload: delete_request(),
                },
            )
            .unwrap();
            let response = crate::ipc::read_frame(&mut submission_client)
                .unwrap()
                .expect("optimizer must return a DELETE plan");
            assert_eq!(response.operation, Operation::OptimizeDelete as u16);
            assert_eq!(response.request_id, 10);
            let response = OptimizerResponse::decode(&response.payload).unwrap();
            assert_eq!(response.status, OptimizerStatus::Success);
            let plan =
                crate::optimizer_plan::decode_delete_plan_v1(response.plan.as_deref().unwrap())
                    .unwrap();
            assert_eq!(plan.relation.schema_id, [1; 16]);
            assert_eq!(plan.relation.table_epoch, 7);
            assert!(plan.scan_columns.is_empty());
            submission_client.shutdown(Shutdown::Write).unwrap();
            planner_thread.join().unwrap();
            optimizer_thread.join().unwrap().unwrap();
        });
    }

    #[test]
    fn process_protocol_dispatches_update_and_returns_a_validated_update_plan() {
        let (mut submission_client, mut submission_server) = UnixStream::pair().unwrap();
        let (mut planner_client, mut planner_server) = UnixStream::pair().unwrap();
        std::thread::scope(|scope| {
            let planner_thread = scope.spawn(|| {
                let request = crate::ipc::read_frame(&mut planner_server)
                    .unwrap()
                    .expect("optimizer must resolve UPDATE relation metadata");
                assert_eq!(request.operation, Operation::DescribePlanRelation as u16);
                assert_eq!(request.request_id, 257);
                let relation = AuthorizedPlanRelation {
                    schema_id: [1; 16],
                    schema_version: 3,
                    table_epoch: 7,
                    relation: AuthorizedRelationDescription {
                        relation_id: [2; 16],
                        name: "items".to_owned(),
                        columns: vec![
                            VisibleColumn {
                                column_id: [3; 16],
                                name: "id".to_owned(),
                                ordinal: 0,
                                scalar_type: ScalarType::Integer,
                                nullable: false,
                            },
                            VisibleColumn {
                                column_id: [4; 16],
                                name: "active".to_owned(),
                                ordinal: 1,
                                scalar_type: ScalarType::Boolean,
                                nullable: true,
                            },
                        ],
                    },
                };
                crate::ipc::write_frame(
                    &mut planner_server,
                    &IpcFrame {
                        operation: request.operation,
                        request_id: request.request_id,
                        payload: DescribePlanRelationResponse {
                            relation: Some(relation),
                        }
                        .encode()
                        .unwrap(),
                    },
                )
                .unwrap();
            });
            let optimizer_thread = scope
                .spawn(|| serve_optimizer_process(&mut submission_server, &mut planner_client));

            crate::ipc::write_frame(
                &mut submission_client,
                &IpcFrame {
                    operation: Operation::OptimizeUpdate as u16,
                    request_id: 11,
                    payload: update_request(),
                },
            )
            .unwrap();
            let response = crate::ipc::read_frame(&mut submission_client)
                .unwrap()
                .expect("optimizer must return an UPDATE plan");
            assert_eq!(response.operation, Operation::OptimizeUpdate as u16);
            assert_eq!(response.request_id, 11);
            let response = OptimizerResponse::decode(&response.payload).unwrap();
            assert_eq!(response.status, OptimizerStatus::Success);
            let plan =
                crate::optimizer_plan::decode_update_plan_v1(response.plan.as_deref().unwrap())
                    .unwrap();
            assert_eq!(plan.relation.schema_id, [1; 16]);
            assert_eq!(plan.relation.table_epoch, 7);
            assert_eq!(plan.scan_columns.len(), 2);
            assert_eq!(plan.assignments[0].column.column_id, [4; 16]);
            assert_eq!(
                plan.assignments[0].value,
                crate::optimizer_binding::BoundUpdateValue::Parameter(1)
            );
            submission_client.shutdown(Shutdown::Write).unwrap();
            planner_thread.join().unwrap();
            optimizer_thread.join().unwrap().unwrap();
        });
    }

    #[test]
    fn process_protocol_resolves_both_join_relations_and_returns_a_validated_join_plan() {
        let (mut submission_client, mut submission_server) = UnixStream::pair().unwrap();
        let (mut planner_client, mut planner_server) = UnixStream::pair().unwrap();
        std::thread::scope(|scope| {
            let planner_thread = scope.spawn(|| {
                for (name, relation_id, base_column_id) in
                    [("lefts", 0x31, 0x41), ("rights", 0x32, 0x51)]
                {
                    let request = crate::ipc::read_frame(&mut planner_server)
                        .unwrap()
                        .expect("optimizer must resolve each join relation");
                    let decoded = ResolveRelationRequest::decode(&request.payload).unwrap();
                    assert_eq!(request.request_id, if name == "lefts" { 257 } else { 258 });
                    assert_eq!(decoded.schema_name, "public");
                    assert_eq!(decoded.relation_name, name);
                    let relation = AuthorizedPlanRelation {
                        schema_id: [1; 16],
                        schema_version: 3,
                        table_epoch: u64::from(relation_id),
                        relation: AuthorizedRelationDescription {
                            relation_id: [relation_id; 16],
                            name: name.to_owned(),
                            columns: vec![
                                VisibleColumn {
                                    column_id: [base_column_id; 16],
                                    name: "id".to_owned(),
                                    ordinal: 0,
                                    scalar_type: ScalarType::Integer,
                                    nullable: false,
                                },
                                VisibleColumn {
                                    column_id: [base_column_id + 1; 16],
                                    name: "active".to_owned(),
                                    ordinal: 1,
                                    scalar_type: ScalarType::Boolean,
                                    nullable: false,
                                },
                                VisibleColumn {
                                    column_id: [base_column_id + 2; 16],
                                    name: "value".to_owned(),
                                    ordinal: 2,
                                    scalar_type: ScalarType::Text,
                                    nullable: true,
                                },
                            ],
                        },
                    };
                    crate::ipc::write_frame(
                        &mut planner_server,
                        &IpcFrame {
                            operation: request.operation,
                            request_id: request.request_id,
                            payload: DescribePlanRelationResponse {
                                relation: Some(relation),
                            }
                            .encode()
                            .unwrap(),
                        },
                    )
                    .unwrap();
                }
            });
            let optimizer_thread = scope
                .spawn(|| serve_optimizer_process(&mut submission_server, &mut planner_client));

            crate::ipc::write_frame(
                &mut submission_client,
                &IpcFrame {
                    operation: Operation::OptimizeJoinSelect as u16,
                    request_id: 12,
                    payload: join_select_request(),
                },
            )
            .unwrap();
            let response = crate::ipc::read_frame(&mut submission_client)
                .unwrap()
                .expect("optimizer must return a join plan");
            assert_eq!(response.operation, Operation::OptimizeJoinSelect as u16);
            assert_eq!(response.request_id, 12);
            let response = OptimizerResponse::decode(&response.payload).unwrap();
            assert_eq!(response.status, OptimizerStatus::Success);
            let plan = crate::optimizer_plan::decode_join_select_plan_v1(
                response.plan.as_deref().unwrap(),
            )
            .unwrap();
            assert_eq!(plan.left_relation.relation_name, "lefts");
            assert_eq!(plan.right_relation.relation_name, "rights");
            assert_eq!(plan.left_scan_columns.len(), 3);
            assert_eq!(plan.right_scan_columns.len(), 3);
            assert_eq!(plan.join_keys.len(), 2);
            assert_eq!(plan.limit, Some(5));
            submission_client.shutdown(Shutdown::Write).unwrap();
            planner_thread.join().unwrap();
            optimizer_thread.join().unwrap().unwrap();
        });
    }

    #[test]
    fn process_protocol_rejects_malformed_frames() {
        let (mut submission_client, mut submission_server) = UnixStream::pair().unwrap();
        let (mut planner_client, _planner_server) = UnixStream::pair().unwrap();
        let optimizer_thread = std::thread::spawn(move || {
            serve_optimizer_process(&mut submission_server, &mut planner_client)
        });

        submission_client.write_all(&[0; 28]).unwrap();
        submission_client.shutdown(Shutdown::Write).unwrap();
        assert!(matches!(
            optimizer_thread.join().unwrap(),
            Err(OptimizerProcessError::Frame(_))
        ));
    }

    #[test]
    fn process_protocol_rejects_duplicate_request_ids_after_invalid_request() {
        let (mut submission_client, mut submission_server) = UnixStream::pair().unwrap();
        let (mut planner_client, _planner_server) = UnixStream::pair().unwrap();
        let optimizer_thread = std::thread::spawn(move || {
            serve_optimizer_process(&mut submission_server, &mut planner_client)
        });
        let invalid_request = IpcFrame {
            operation: Operation::OptimizeSelect as u16,
            request_id: 41,
            payload: Vec::new(),
        };
        crate::ipc::write_frame(&mut submission_client, &invalid_request).unwrap();
        let invalid_response = crate::ipc::read_frame(&mut submission_client)
            .unwrap()
            .expect("invalid request receives a response");
        assert_eq!(invalid_response.request_id, invalid_request.request_id);
        assert_eq!(
            OptimizerResponse::decode(&invalid_response.payload)
                .unwrap()
                .status,
            OptimizerStatus::InvalidRequest
        );

        crate::ipc::write_frame(&mut submission_client, &invalid_request).unwrap();
        submission_client.shutdown(Shutdown::Write).unwrap();
        assert!(matches!(
            optimizer_thread.join().unwrap(),
            Err(OptimizerProcessError::DuplicateRequestId)
        ));
    }

    #[test]
    fn process_protocol_enforces_connection_request_limit() {
        let (mut submission_client, mut submission_server) = UnixStream::pair().unwrap();
        let (mut planner_client, _planner_server) = UnixStream::pair().unwrap();
        let optimizer_thread = std::thread::spawn(move || {
            serve_optimizer_process(&mut submission_server, &mut planner_client)
        });

        for request_id in 1..=MAX_OPTIMIZER_REQUESTS_PER_CONNECTION as u64 {
            crate::ipc::write_frame(
                &mut submission_client,
                &IpcFrame {
                    operation: Operation::OptimizeSelect as u16,
                    request_id,
                    payload: Vec::new(),
                },
            )
            .unwrap();
            let response = crate::ipc::read_frame(&mut submission_client)
                .unwrap()
                .expect("invalid request remains recoverable within the stream budget");
            assert_eq!(response.request_id, request_id);
            assert_eq!(
                OptimizerResponse::decode(&response.payload).unwrap().status,
                OptimizerStatus::InvalidRequest
            );
        }

        crate::ipc::write_frame(
            &mut submission_client,
            &IpcFrame {
                operation: Operation::OptimizeSelect as u16,
                request_id: MAX_OPTIMIZER_REQUESTS_PER_CONNECTION as u64 + 1,
                payload: Vec::new(),
            },
        )
        .unwrap();
        submission_client.shutdown(Shutdown::Write).unwrap();
        assert!(matches!(
            optimizer_thread.join().unwrap(),
            Err(OptimizerProcessError::RequestLimitExceeded)
        ));
    }
}
