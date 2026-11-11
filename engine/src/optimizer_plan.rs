//! Deterministic single-table SELECT plan construction for optimizer v1.

use crate::optimizer_binding::{BoundColumn, BoundPredicate, BoundSelect};
use crate::tuple::ScalarType;
use std::collections::HashSet;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RelationReference {
    pub schema_name: String,
    pub schema_id: [u8; 16],
    pub schema_version: u64,
    pub relation_name: String,
    pub relation_id: [u8; 16],
    pub table_epoch: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlanNode {
    Scan { columns: Vec<BoundColumn> },
    Filter(BoundPredicate),
    Project(Vec<BoundColumn>),
    Limit(u32),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SingleTablePlanV1 {
    pub relation: RelationReference,
    /// Nodes form one linear pipeline in execution order.
    pub nodes: Vec<PlanNode>,
}

pub const MAX_PLAN_BYTES: usize = 64 * 1024;
pub const MAX_PLAN_SCAN_COLUMNS: usize = crate::optimizer_request::MAX_PROJECTIONS * 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlanError {
    EmptyProjection,
    InvalidRelationReference,
    InvalidColumnReference,
    InvalidScalarType,
    InvalidPredicate,
    InvalidPipeline,
    Truncated,
    TooLarge,
    UnsupportedVersion(u16),
    InvalidEncoding,
    TrailingBytes,
}

impl std::fmt::Display for PlanError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "optimizer plan error: {self:?}")
    }
}

impl std::error::Error for PlanError {}

/// Encode a validated SELECT plan as canonical version-1 bytes suitable for
/// transfer over a bounded optimizer/executor channel.
pub fn encode_plan_v1(plan: &SingleTablePlanV1) -> Result<Vec<u8>, PlanError> {
    validate_plan_v1(plan)?;
    let mut output = Vec::with_capacity(256);
    output.extend_from_slice(&crate::query_abi::encode_optimizer_plan_header_record(
        1, 1, 0,
    ));
    put_name(&mut output, &plan.relation.schema_name)?;
    output.extend_from_slice(
        &crate::query_abi::encode_optimizer_plan_schema_reference_record(
            plan.relation.schema_id,
            plan.relation.schema_version,
        ),
    );
    put_name(&mut output, &plan.relation.relation_name)?;
    output.extend_from_slice(
        &crate::query_abi::encode_optimizer_plan_relation_reference_record(
            plan.relation.relation_id,
            plan.relation.table_epoch,
        ),
    );
    output.push(u8::try_from(plan.nodes.len()).map_err(|_| PlanError::InvalidPipeline)?);
    for node in &plan.nodes {
        match node {
            PlanNode::Scan { columns } => {
                put_column_list_header(&mut output, 1, columns.len())?;
                for column in columns {
                    put_column(&mut output, column);
                }
            }
            PlanNode::Filter(predicate) => {
                output.push(2);
                put_predicate(&mut output, predicate)?;
            }
            PlanNode::Project(columns) => {
                put_column_list_header(&mut output, 3, columns.len())?;
                for column in columns {
                    put_column(&mut output, column);
                }
            }
            PlanNode::Limit(limit) => {
                output.extend_from_slice(
                    &crate::query_abi::encode_optimizer_plan_limit_node_record(4, *limit),
                );
            }
        }
        if output.len() > MAX_PLAN_BYTES {
            return Err(PlanError::TooLarge);
        }
    }
    if output.len() > MAX_PLAN_BYTES {
        return Err(PlanError::TooLarge);
    }
    Ok(output)
}

/// Decode and independently validate a canonical optimizer v1 SELECT plan.
/// The executor must still resolve the logical relation through its own
/// invocation-bound storage connection before using it.
pub fn decode_plan_v1(bytes: &[u8]) -> Result<SingleTablePlanV1, PlanError> {
    if bytes.len() > MAX_PLAN_BYTES {
        return Err(PlanError::TooLarge);
    }
    let mut reader = PlanReader { bytes, offset: 0 };
    let version = reader.u16()?;
    if version != 1 {
        return Err(PlanError::UnsupportedVersion(version));
    }
    let (_, kind, reserved) =
        crate::query_abi::decode_optimizer_plan_header_record(bytes).ok_or(PlanError::Truncated)?;
    reader.offset = crate::query_abi::OPTIMIZER_PLAN_HEADER_RECORD_HEADER_BYTES as usize;
    if kind != 1 || reserved != 0 {
        return Err(PlanError::InvalidEncoding);
    }
    let schema_name = reader.name()?;
    let schema_reference = reader
        .take(crate::query_abi::OPTIMIZER_PLAN_SCHEMA_REFERENCE_RECORD_HEADER_BYTES as usize)?;
    let (schema_id, schema_version) =
        crate::query_abi::decode_optimizer_plan_schema_reference_record(schema_reference)
            .ok_or(PlanError::Truncated)?;
    let relation_name = reader.name()?;
    let relation_reference = reader
        .take(crate::query_abi::OPTIMIZER_PLAN_RELATION_REFERENCE_RECORD_HEADER_BYTES as usize)?;
    let (relation_id, table_epoch) =
        crate::query_abi::decode_optimizer_plan_relation_reference_record(relation_reference)
            .ok_or(PlanError::Truncated)?;
    let relation = RelationReference {
        schema_name,
        schema_id,
        schema_version,
        relation_name,
        relation_id,
        table_epoch,
    };
    let node_count = usize::from(reader.u8()?);
    if !(2..=4).contains(&node_count) {
        return Err(PlanError::InvalidPipeline);
    }
    let mut nodes = Vec::with_capacity(node_count);
    let mut scan_columns = Vec::new();
    for index in 0..node_count {
        let tag_offset = reader.offset;
        let tag = reader.u8()?;
        let node = match tag {
            1 if index == 0 => {
                let count = reader.column_list_count(tag_offset, 1, MAX_PLAN_SCAN_COLUMNS)?;
                if count == 0 {
                    return Err(PlanError::InvalidColumnReference);
                }
                let mut columns = Vec::with_capacity(count);
                for _ in 0..count {
                    columns.push(reader.column()?);
                }
                scan_columns = columns.clone();
                PlanNode::Scan { columns }
            }
            2 => PlanNode::Filter(reader.predicate(&scan_columns)?),
            3 => {
                let count = reader.column_list_count(
                    tag_offset,
                    3,
                    crate::optimizer_request::MAX_PROJECTIONS,
                )?;
                let mut columns = Vec::with_capacity(count);
                for _ in 0..count {
                    columns.push(reader.column()?);
                }
                PlanNode::Project(columns)
            }
            4 => PlanNode::Limit(reader.limit_node_value(tag_offset)?),
            _ => return Err(PlanError::InvalidPipeline),
        };
        nodes.push(node);
    }
    if reader.offset != bytes.len() {
        return Err(PlanError::TrailingBytes);
    }
    let plan = SingleTablePlanV1 { relation, nodes };
    validate_plan_v1(&plan)?;
    Ok(plan)
}

fn put_name(output: &mut Vec<u8>, name: &str) -> Result<(), PlanError> {
    if !valid_identifier(name) {
        return Err(PlanError::InvalidRelationReference);
    }
    output.push(u8::try_from(name.len()).map_err(|_| PlanError::TooLarge)?);
    output.extend_from_slice(name.as_bytes());
    Ok(())
}

fn put_column_list_header(
    output: &mut Vec<u8>,
    node_tag: u8,
    column_count: usize,
) -> Result<(), PlanError> {
    let column_count = u16::try_from(column_count).map_err(|_| PlanError::TooLarge)?;
    output.extend_from_slice(
        &crate::query_abi::encode_optimizer_plan_column_list_header_record(node_tag, column_count),
    );
    Ok(())
}

fn put_column(output: &mut Vec<u8>, column: &BoundColumn) {
    let (type_code, parameters) = encode_scalar_type(column.scalar_type);
    output.extend_from_slice(&crate::query_abi::encode_optimizer_plan_column_record(
        column.column_id,
        column.ordinal,
        type_code,
        parameters,
        u8::from(column.nullable),
    ));
}

fn encode_scalar_type(scalar_type: ScalarType) -> (u8, [u8; 4]) {
    match scalar_type {
        ScalarType::Boolean => (1, [0; 4]),
        ScalarType::SmallInt => (2, [0; 4]),
        ScalarType::Integer => (3, [0; 4]),
        ScalarType::BigInt => (4, [0; 4]),
        ScalarType::Decimal { precision, scale } => (5, [precision, scale, 0, 0]),
        ScalarType::Text => (6, [0; 4]),
        ScalarType::Varchar { max_chars } => (7, max_chars.to_be_bytes()),
        ScalarType::Bytea => (8, [0; 4]),
        ScalarType::Uuid => (9, [0; 4]),
        ScalarType::Date => (10, [0; 4]),
        ScalarType::TimestampTz => (11, [0; 4]),
    }
}

fn put_predicate(output: &mut Vec<u8>, predicate: &BoundPredicate) -> Result<(), PlanError> {
    match predicate {
        BoundPredicate::Null => output.push(0x00),
        BoundPredicate::Boolean(value) => output.extend_from_slice(&[0x01, u8::from(*value)]),
        BoundPredicate::Integer(value) => {
            output.extend_from_slice(
                &crate::query_abi::encode_optimizer_plan_integer_predicate_record(0x02, *value),
            );
        }
        BoundPredicate::Text(value) => {
            let text_length = u16::try_from(value.len()).map_err(|_| PlanError::TooLarge)?;
            output.extend_from_slice(
                &crate::query_abi::encode_optimizer_plan_text_predicate_header_record(
                    0x03,
                    text_length,
                ),
            );
            output.extend_from_slice(value.as_bytes());
        }
        BoundPredicate::Column(column) => {
            output.extend_from_slice(
                &crate::query_abi::encode_optimizer_plan_column_predicate_record(
                    0x10,
                    column.column_id,
                ),
            );
        }
        BoundPredicate::Compare {
            operator,
            left,
            right,
        } => {
            output.push(0x20);
            output.push(match operator {
                crate::optimizer_request::Comparison::Equal => 0,
                crate::optimizer_request::Comparison::NotEqual => 1,
                crate::optimizer_request::Comparison::Less => 2,
                crate::optimizer_request::Comparison::LessOrEqual => 3,
                crate::optimizer_request::Comparison::Greater => 4,
                crate::optimizer_request::Comparison::GreaterOrEqual => 5,
            });
            put_predicate(output, left)?;
            put_predicate(output, right)?;
        }
        BoundPredicate::And(children) | BoundPredicate::Or(children) => {
            let is_and = matches!(predicate, BoundPredicate::And(_));
            output.push(if is_and { 0x30 } else { 0x31 });
            output.push(u8::try_from(children.len()).map_err(|_| PlanError::InvalidPredicate)?);
            for child in children {
                put_predicate(output, child)?;
            }
        }
        BoundPredicate::Not(child) => {
            output.push(0x32);
            put_predicate(output, child)?;
        }
    }
    Ok(())
}

struct PlanReader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl PlanReader<'_> {
    fn take(&mut self, count: usize) -> Result<&[u8], PlanError> {
        let end = self.offset.checked_add(count).ok_or(PlanError::TooLarge)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(PlanError::Truncated)?;
        self.offset = end;
        Ok(value)
    }

    fn u8(&mut self) -> Result<u8, PlanError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, PlanError> {
        Ok(u16::from_be_bytes(
            self.take(2)?.try_into().map_err(|_| PlanError::Truncated)?,
        ))
    }

    fn name(&mut self) -> Result<String, PlanError> {
        let length = usize::from(self.u8()?);
        let bytes = self.take(length)?;
        let name = std::str::from_utf8(bytes).map_err(|_| PlanError::InvalidEncoding)?;
        if !valid_identifier(name) {
            return Err(PlanError::InvalidRelationReference);
        }
        Ok(name.to_owned())
    }

    fn column_list_count(
        &mut self,
        header_offset: usize,
        expected_tag: u8,
        maximum: usize,
    ) -> Result<usize, PlanError> {
        let header_len =
            crate::query_abi::OPTIMIZER_PLAN_COLUMN_LIST_HEADER_RECORD_HEADER_BYTES as usize;
        let end = header_offset
            .checked_add(header_len)
            .ok_or(PlanError::TooLarge)?;
        let header = self
            .bytes
            .get(header_offset..end)
            .ok_or(PlanError::Truncated)?;
        let (node_tag, count) =
            crate::query_abi::decode_optimizer_plan_column_list_header_record(header)
                .ok_or(PlanError::Truncated)?;
        if node_tag != expected_tag {
            return Err(PlanError::InvalidPipeline);
        }
        self.offset = end;
        let count = usize::from(count);
        if count > maximum {
            return Err(PlanError::TooLarge);
        }
        Ok(count)
    }

    fn limit_node_value(&mut self, header_offset: usize) -> Result<u32, PlanError> {
        let header_len = crate::query_abi::OPTIMIZER_PLAN_LIMIT_NODE_RECORD_HEADER_BYTES as usize;
        let end = header_offset
            .checked_add(header_len)
            .ok_or(PlanError::TooLarge)?;
        let header = self
            .bytes
            .get(header_offset..end)
            .ok_or(PlanError::Truncated)?;
        let (node_tag, row_limit) =
            crate::query_abi::decode_optimizer_plan_limit_node_record(header)
                .ok_or(PlanError::Truncated)?;
        if node_tag != 4 {
            return Err(PlanError::InvalidPipeline);
        }
        self.offset = end;
        Ok(row_limit)
    }

    fn text_predicate_length(&mut self, header_offset: usize) -> Result<u16, PlanError> {
        let header_len =
            crate::query_abi::OPTIMIZER_PLAN_TEXT_PREDICATE_HEADER_RECORD_HEADER_BYTES as usize;
        let end = header_offset
            .checked_add(header_len)
            .ok_or(PlanError::TooLarge)?;
        let header = self
            .bytes
            .get(header_offset..end)
            .ok_or(PlanError::Truncated)?;
        let (node_tag, text_length) =
            crate::query_abi::decode_optimizer_plan_text_predicate_header_record(header)
                .ok_or(PlanError::Truncated)?;
        if node_tag != 0x03 {
            return Err(PlanError::InvalidEncoding);
        }
        self.offset = end;
        Ok(text_length)
    }

    fn integer_predicate_value(&mut self, header_offset: usize) -> Result<i64, PlanError> {
        let header_len =
            crate::query_abi::OPTIMIZER_PLAN_INTEGER_PREDICATE_RECORD_HEADER_BYTES as usize;
        let end = header_offset
            .checked_add(header_len)
            .ok_or(PlanError::TooLarge)?;
        let header = self
            .bytes
            .get(header_offset..end)
            .ok_or(PlanError::Truncated)?;
        let (node_tag, integer_value) =
            crate::query_abi::decode_optimizer_plan_integer_predicate_record(header)
                .ok_or(PlanError::Truncated)?;
        if node_tag != 0x02 {
            return Err(PlanError::InvalidEncoding);
        }
        self.offset = end;
        Ok(integer_value)
    }

    fn column_predicate_id(&mut self, header_offset: usize) -> Result<[u8; 16], PlanError> {
        let header_len =
            crate::query_abi::OPTIMIZER_PLAN_COLUMN_PREDICATE_RECORD_HEADER_BYTES as usize;
        let end = header_offset
            .checked_add(header_len)
            .ok_or(PlanError::TooLarge)?;
        let header = self
            .bytes
            .get(header_offset..end)
            .ok_or(PlanError::Truncated)?;
        let (node_tag, column_id) =
            crate::query_abi::decode_optimizer_plan_column_predicate_record(header)
                .ok_or(PlanError::Truncated)?;
        if node_tag != 0x10 {
            return Err(PlanError::InvalidEncoding);
        }
        self.offset = end;
        Ok(column_id)
    }

    fn column(&mut self) -> Result<BoundColumn, PlanError> {
        let bytes =
            self.take(crate::query_abi::OPTIMIZER_PLAN_COLUMN_RECORD_HEADER_BYTES as usize)?;
        let (column_id, ordinal, type_code, parameters, nullable) =
            crate::query_abi::decode_optimizer_plan_column_record(bytes)
                .ok_or(PlanError::Truncated)?;
        let nullable = match nullable {
            0 => false,
            1 => true,
            _ => return Err(PlanError::InvalidEncoding),
        };
        let scalar_type = decode_scalar_type(type_code, parameters)?;
        Ok(BoundColumn {
            column_id,
            ordinal,
            scalar_type,
            nullable,
        })
    }

    fn predicate(&mut self, scan_columns: &[BoundColumn]) -> Result<BoundPredicate, PlanError> {
        let mut nodes = 0;
        self.predicate_at_depth(scan_columns, 1, &mut nodes)
    }

    fn predicate_at_depth(
        &mut self,
        scan_columns: &[BoundColumn],
        depth: usize,
        nodes: &mut usize,
    ) -> Result<BoundPredicate, PlanError> {
        if depth > crate::optimizer_request::MAX_PREDICATE_DEPTH {
            return Err(PlanError::InvalidPredicate);
        }
        *nodes += 1;
        if *nodes > crate::optimizer_request::MAX_PREDICATE_NODES {
            return Err(PlanError::InvalidPredicate);
        }
        let tag_offset = self.offset;
        match self.u8()? {
            0x00 => Ok(BoundPredicate::Null),
            0x01 => match self.u8()? {
                0 => Ok(BoundPredicate::Boolean(false)),
                1 => Ok(BoundPredicate::Boolean(true)),
                _ => Err(PlanError::InvalidEncoding),
            },
            0x02 => Ok(BoundPredicate::Integer(
                self.integer_predicate_value(tag_offset)?,
            )),
            0x03 => {
                let length = usize::from(self.text_predicate_length(tag_offset)?);
                if length > crate::optimizer_request::MAX_TEXT_LITERAL_BYTES {
                    return Err(PlanError::TooLarge);
                }
                let text = std::str::from_utf8(self.take(length)?)
                    .map_err(|_| PlanError::InvalidEncoding)?
                    .to_owned();
                Ok(BoundPredicate::Text(text))
            }
            0x10 => {
                let column_id = self.column_predicate_id(tag_offset)?;
                let column = scan_columns
                    .iter()
                    .find(|column| column.column_id == column_id)
                    .ok_or(PlanError::InvalidColumnReference)?;
                Ok(BoundPredicate::Column(column.clone()))
            }
            0x20 => {
                let operator = match self.u8()? {
                    0 => crate::optimizer_request::Comparison::Equal,
                    1 => crate::optimizer_request::Comparison::NotEqual,
                    2 => crate::optimizer_request::Comparison::Less,
                    3 => crate::optimizer_request::Comparison::LessOrEqual,
                    4 => crate::optimizer_request::Comparison::Greater,
                    5 => crate::optimizer_request::Comparison::GreaterOrEqual,
                    _ => return Err(PlanError::InvalidEncoding),
                };
                let left = Box::new(self.predicate_at_depth(scan_columns, depth + 1, nodes)?);
                let right = Box::new(self.predicate_at_depth(scan_columns, depth + 1, nodes)?);
                Ok(BoundPredicate::Compare {
                    operator,
                    left,
                    right,
                })
            }
            0x30 | 0x31 => {
                let tag = self.bytes[self.offset - 1];
                let count = usize::from(self.u8()?);
                if !(2..=16).contains(&count) {
                    return Err(PlanError::InvalidPredicate);
                }
                let mut children = Vec::with_capacity(count);
                for _ in 0..count {
                    children.push(self.predicate_at_depth(scan_columns, depth + 1, nodes)?);
                }
                if tag == 0x30 {
                    Ok(BoundPredicate::And(children))
                } else {
                    Ok(BoundPredicate::Or(children))
                }
            }
            0x32 => Ok(BoundPredicate::Not(Box::new(self.predicate_at_depth(
                scan_columns,
                depth + 1,
                nodes,
            )?))),
            _ => Err(PlanError::InvalidEncoding),
        }
    }
}

fn decode_scalar_type(code: u8, parameters: [u8; 4]) -> Result<ScalarType, PlanError> {
    match code {
        1 if parameters == [0; 4] => Ok(ScalarType::Boolean),
        2 if parameters == [0; 4] => Ok(ScalarType::SmallInt),
        3 if parameters == [0; 4] => Ok(ScalarType::Integer),
        4 if parameters == [0; 4] => Ok(ScalarType::BigInt),
        5 if (1..=38).contains(&parameters[0])
            && parameters[1] <= parameters[0]
            && parameters[2..] == [0; 2] =>
        {
            Ok(ScalarType::Decimal {
                precision: parameters[0],
                scale: parameters[1],
            })
        }
        6 if parameters == [0; 4] => Ok(ScalarType::Text),
        7 if u32::from_be_bytes(parameters) != 0 => Ok(ScalarType::Varchar {
            max_chars: u32::from_be_bytes(parameters),
        }),
        8 if parameters == [0; 4] => Ok(ScalarType::Bytea),
        9 if parameters == [0; 4] => Ok(ScalarType::Uuid),
        10 if parameters == [0; 4] => Ok(ScalarType::Date),
        11 if parameters == [0; 4] => Ok(ScalarType::TimestampTz),
        _ => Err(PlanError::InvalidEncoding),
    }
}

/// Lower a bound request to a conservative scan/filter/project/limit
/// pipeline. Scan columns are the unique union of projected and predicate
/// columns, ordered by catalog ordinal.
pub fn plan_select_v1(bound: BoundSelect) -> Result<SingleTablePlanV1, PlanError> {
    if bound.projections.is_empty() {
        return Err(PlanError::EmptyProjection);
    }
    let relation = RelationReference {
        schema_name: bound.schema_name,
        schema_id: bound.schema_id,
        schema_version: bound.schema_version,
        relation_name: bound.relation_name,
        relation_id: bound.relation_id,
        table_epoch: bound.table_epoch,
    };
    validate_relation(&relation)?;

    let mut scan_columns = Vec::with_capacity(bound.projections.len());
    for column in &bound.projections {
        add_column(&mut scan_columns, column)?;
    }
    if let Some(predicate) = &bound.predicate {
        collect_predicate_columns(predicate, &mut scan_columns)?;
    }
    scan_columns.sort_by_key(|column| column.ordinal);

    let mut nodes = Vec::with_capacity(4);
    nodes.push(PlanNode::Scan {
        columns: scan_columns,
    });
    if let Some(predicate) = bound.predicate {
        nodes.push(PlanNode::Filter(predicate));
    }
    nodes.push(PlanNode::Project(bound.projections));
    if let Some(limit) = bound.limit {
        nodes.push(PlanNode::Limit(limit));
    }
    let plan = SingleTablePlanV1 { relation, nodes };
    validate_plan_v1(&plan)?;
    Ok(plan)
}

/// Validate the structural invariants an executor must check independently
/// before interpreting a decoded plan.
pub fn validate_plan_v1(plan: &SingleTablePlanV1) -> Result<(), PlanError> {
    validate_relation(&plan.relation)?;
    if !(2..=4).contains(&plan.nodes.len()) {
        return Err(PlanError::InvalidPipeline);
    }
    let PlanNode::Scan { columns } = &plan.nodes[0] else {
        return Err(PlanError::InvalidPipeline);
    };
    validate_columns(columns)?;

    let mut index = 1;
    if matches!(plan.nodes.get(index), Some(PlanNode::Filter(_))) {
        index += 1;
    }
    let Some(PlanNode::Project(projections)) = plan.nodes.get(index) else {
        return Err(PlanError::InvalidPipeline);
    };
    if projections.is_empty() {
        return Err(PlanError::EmptyProjection);
    }
    validate_columns_allow_duplicates(projections)?;
    for projection in projections {
        if !columns.iter().any(|scanned| scanned == projection) {
            return Err(PlanError::InvalidColumnReference);
        }
    }
    index += 1;
    if matches!(plan.nodes.get(index), Some(PlanNode::Limit(_))) {
        index += 1;
    }
    if index != plan.nodes.len() {
        return Err(PlanError::InvalidPipeline);
    }
    if let Some(PlanNode::Filter(predicate)) = plan.nodes.get(1) {
        if predicate_type(predicate, columns)? != Some(ScalarType::Boolean) {
            return Err(PlanError::InvalidPredicate);
        }
        let mut refs = Vec::new();
        collect_predicate_columns(predicate, &mut refs)?;
        for reference in refs {
            if !columns.iter().any(|scanned| scanned == &reference) {
                return Err(PlanError::InvalidColumnReference);
            }
        }
    }
    Ok(())
}

fn validate_relation(relation: &RelationReference) -> Result<(), PlanError> {
    if relation.schema_id == [0; 16]
        || relation.relation_id == [0; 16]
        || relation.schema_version == 0
        || relation.table_epoch == 0
        || !valid_identifier(&relation.schema_name)
        || !valid_identifier(&relation.relation_name)
    {
        return Err(PlanError::InvalidRelationReference);
    }
    Ok(())
}

fn validate_columns(columns: &[BoundColumn]) -> Result<(), PlanError> {
    if columns.is_empty() || columns.len() > crate::tuple::MAX_COLUMNS {
        return Err(PlanError::InvalidColumnReference);
    }
    let mut ids = HashSet::with_capacity(columns.len());
    let mut ordinals = HashSet::with_capacity(columns.len());
    for column in columns {
        if column.column_id == [0; 16]
            || !ids.insert(column.column_id)
            || !ordinals.insert(column.ordinal)
        {
            return Err(PlanError::InvalidColumnReference);
        }
        if !valid_scalar_type(column.scalar_type) {
            return Err(PlanError::InvalidScalarType);
        }
    }
    if columns
        .windows(2)
        .any(|pair| pair[0].ordinal >= pair[1].ordinal)
    {
        return Err(PlanError::InvalidColumnReference);
    }
    Ok(())
}

fn validate_columns_allow_duplicates(columns: &[BoundColumn]) -> Result<(), PlanError> {
    if columns.is_empty()
        || columns.len() > crate::tuple::MAX_COLUMNS
        || columns.iter().any(|column| column.column_id == [0; 16])
    {
        return Err(PlanError::InvalidColumnReference);
    }
    if columns
        .iter()
        .any(|column| !valid_scalar_type(column.scalar_type))
    {
        return Err(PlanError::InvalidScalarType);
    }
    for (index, column) in columns.iter().enumerate() {
        if columns[index + 1..]
            .iter()
            .any(|later| later.ordinal == column.ordinal && later.column_id != column.column_id)
        {
            return Err(PlanError::InvalidColumnReference);
        }
    }
    Ok(())
}

fn add_column(columns: &mut Vec<BoundColumn>, column: &BoundColumn) -> Result<(), PlanError> {
    if column.column_id == [0; 16] {
        return Err(PlanError::InvalidColumnReference);
    }
    if let Some(existing) = columns
        .iter()
        .find(|existing| existing.ordinal == column.ordinal)
    {
        return if existing == column {
            Ok(())
        } else {
            Err(PlanError::InvalidColumnReference)
        };
    }
    if columns
        .iter()
        .any(|existing| existing.column_id == column.column_id)
    {
        return Err(PlanError::InvalidColumnReference);
    }
    columns.push(column.clone());
    Ok(())
}

fn collect_predicate_columns(
    predicate: &BoundPredicate,
    columns: &mut Vec<BoundColumn>,
) -> Result<(), PlanError> {
    match predicate {
        BoundPredicate::Column(column) => add_column(columns, column),
        BoundPredicate::Compare { left, right, .. } => {
            collect_predicate_columns(left, columns)?;
            collect_predicate_columns(right, columns)
        }
        BoundPredicate::And(children) | BoundPredicate::Or(children) => {
            for child in children {
                collect_predicate_columns(child, columns)?;
            }
            Ok(())
        }
        BoundPredicate::Not(child) => collect_predicate_columns(child, columns),
        BoundPredicate::Null
        | BoundPredicate::Boolean(_)
        | BoundPredicate::Integer(_)
        | BoundPredicate::Text(_) => Ok(()),
    }
}

fn predicate_type(
    predicate: &BoundPredicate,
    scan_columns: &[BoundColumn],
) -> Result<Option<ScalarType>, PlanError> {
    let mut nodes = 0;
    predicate_type_at_depth(predicate, scan_columns, 1, &mut nodes)
}

fn predicate_type_at_depth(
    predicate: &BoundPredicate,
    scan_columns: &[BoundColumn],
    depth: usize,
    nodes: &mut usize,
) -> Result<Option<ScalarType>, PlanError> {
    if depth > crate::optimizer_request::MAX_PREDICATE_DEPTH {
        return Err(PlanError::InvalidPredicate);
    }
    *nodes += 1;
    if *nodes > crate::optimizer_request::MAX_PREDICATE_NODES {
        return Err(PlanError::InvalidPredicate);
    }
    match predicate {
        BoundPredicate::Null => Ok(None),
        BoundPredicate::Boolean(_) => Ok(Some(ScalarType::Boolean)),
        BoundPredicate::Integer(_) => Ok(Some(ScalarType::Integer)),
        BoundPredicate::Text(value)
            if value.len() <= crate::optimizer_request::MAX_TEXT_LITERAL_BYTES =>
        {
            Ok(Some(ScalarType::Text))
        }
        BoundPredicate::Text(_) => Err(PlanError::InvalidPredicate),
        BoundPredicate::Column(column) => {
            if scan_columns.iter().any(|scanned| scanned == column) {
                Ok(Some(column.scalar_type))
            } else {
                Err(PlanError::InvalidColumnReference)
            }
        }
        BoundPredicate::Compare { left, right, .. } => {
            let left = predicate_type_at_depth(left, scan_columns, depth + 1, nodes)?;
            let right = predicate_type_at_depth(right, scan_columns, depth + 1, nodes)?;
            if matches!((left, right), (Some(left), Some(right)) if left != right) {
                return Err(PlanError::InvalidPredicate);
            }
            Ok(Some(ScalarType::Boolean))
        }
        BoundPredicate::And(children) | BoundPredicate::Or(children) => {
            if !(2..=16).contains(&children.len()) {
                return Err(PlanError::InvalidPredicate);
            }
            for child in children {
                if predicate_type_at_depth(child, scan_columns, depth + 1, nodes)?
                    != Some(ScalarType::Boolean)
                {
                    return Err(PlanError::InvalidPredicate);
                }
            }
            Ok(Some(ScalarType::Boolean))
        }
        BoundPredicate::Not(child) => {
            if predicate_type_at_depth(child, scan_columns, depth + 1, nodes)?
                != Some(ScalarType::Boolean)
            {
                return Err(PlanError::InvalidPredicate);
            }
            Ok(Some(ScalarType::Boolean))
        }
    }
}

fn valid_identifier(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 63
        && (bytes[0].is_ascii_alphabetic() || bytes[0] == b'_')
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
}

fn valid_scalar_type(scalar_type: ScalarType) -> bool {
    match scalar_type {
        ScalarType::Decimal { precision, scale } => {
            (1..=38).contains(&precision) && scale <= precision
        }
        ScalarType::Varchar { max_chars } => max_chars != 0,
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::optimizer_request::Comparison;
    use crate::tuple::ScalarType;

    fn column(id: u8, ordinal: u32, scalar_type: ScalarType) -> BoundColumn {
        BoundColumn {
            column_id: [id; 16],
            ordinal,
            scalar_type,
            nullable: false,
        }
    }

    fn bound_select() -> BoundSelect {
        let output = column(1, 0, ScalarType::Integer);
        let filter = column(2, 1, ScalarType::Boolean);
        BoundSelect {
            schema_name: "sales".to_owned(),
            schema_id: [0x10; 16],
            schema_version: 4,
            table_epoch: 6,
            relation_id: [0x20; 16],
            relation_name: "orders".to_owned(),
            projections: vec![output.clone()],
            predicate: Some(BoundPredicate::Compare {
                operator: Comparison::Equal,
                left: Box::new(BoundPredicate::Column(filter)),
                right: Box::new(BoundPredicate::Boolean(true)),
            }),
            limit: Some(5),
        }
    }

    #[test]
    fn lowers_filter_columns_into_scan_and_preserves_operator_order() {
        let plan = plan_select_v1(bound_select()).unwrap();
        assert_eq!(
            plan.relation,
            RelationReference {
                schema_name: "sales".to_owned(),
                schema_id: [0x10; 16],
                schema_version: 4,
                relation_name: "orders".to_owned(),
                relation_id: [0x20; 16],
                table_epoch: 6,
            }
        );
        assert_eq!(
            plan.nodes,
            [
                PlanNode::Scan {
                    columns: vec![
                        column(1, 0, ScalarType::Integer),
                        column(2, 1, ScalarType::Boolean),
                    ],
                },
                PlanNode::Filter(BoundPredicate::Compare {
                    operator: Comparison::Equal,
                    left: Box::new(BoundPredicate::Column(column(2, 1, ScalarType::Boolean))),
                    right: Box::new(BoundPredicate::Boolean(true)),
                }),
                PlanNode::Project(vec![column(1, 0, ScalarType::Integer)]),
                PlanNode::Limit(5),
            ]
        );
        assert_eq!(validate_plan_v1(&plan), Ok(()));
    }

    #[test]
    fn rejects_noncanonical_operator_order_and_duplicate_limits() {
        let mut plan = plan_select_v1(bound_select()).unwrap();
        plan.nodes.swap(1, 2);
        assert_eq!(validate_plan_v1(&plan), Err(PlanError::InvalidPipeline));

        let mut plan = plan_select_v1(bound_select()).unwrap();
        plan.nodes.pop();
        plan.nodes.push(PlanNode::Limit(5));
        plan.nodes.push(PlanNode::Limit(9));
        assert_eq!(validate_plan_v1(&plan), Err(PlanError::InvalidPipeline));
    }

    #[test]
    fn rejects_non_boolean_filters_and_unscanned_references() {
        let mut plan = plan_select_v1(bound_select()).unwrap();
        plan.nodes[1] = PlanNode::Filter(BoundPredicate::Integer(1));
        assert_eq!(validate_plan_v1(&plan), Err(PlanError::InvalidPredicate));

        let mut plan = plan_select_v1(bound_select()).unwrap();
        plan.nodes[1] = PlanNode::Filter(BoundPredicate::Compare {
            operator: Comparison::Equal,
            left: Box::new(BoundPredicate::Column(column(3, 2, ScalarType::Integer))),
            right: Box::new(BoundPredicate::Integer(1)),
        });
        assert_eq!(
            validate_plan_v1(&plan),
            Err(PlanError::InvalidColumnReference)
        );
    }

    #[test]
    fn plan_validator_reapplies_predicate_depth_budget() {
        let mut predicate = BoundPredicate::Boolean(true);
        for _ in 0..=crate::optimizer_request::MAX_PREDICATE_DEPTH {
            predicate = BoundPredicate::Not(Box::new(predicate));
        }
        let mut plan = plan_select_v1(bound_select()).unwrap();
        plan.nodes[1] = PlanNode::Filter(predicate);
        assert_eq!(validate_plan_v1(&plan), Err(PlanError::InvalidPredicate));
    }

    #[test]
    fn scan_contains_unique_sorted_union_of_output_and_filter_columns() {
        let mut select = bound_select();
        select.predicate = Some(BoundPredicate::Compare {
            operator: Comparison::Equal,
            left: Box::new(BoundPredicate::Column(column(1, 0, ScalarType::Integer))),
            right: Box::new(BoundPredicate::Integer(2)),
        });
        let plan = plan_select_v1(select).unwrap();
        let PlanNode::Scan { columns } = &plan.nodes[0] else {
            panic!("first node must scan")
        };
        assert_eq!(columns, &[column(1, 0, ScalarType::Integer)]);
    }

    #[test]
    fn versioned_plan_encoding_round_trips_canonically() {
        let plan = plan_select_v1(bound_select()).unwrap();
        let bytes = encode_plan_v1(&plan).unwrap();
        assert_eq!(&bytes[..4], &[0, 1, 1, 0]);
        let decoded = decode_plan_v1(&bytes).unwrap();
        assert_eq!(decoded, plan);
        assert_eq!(encode_plan_v1(&decoded).unwrap(), bytes);
    }

    #[test]
    fn plan_decoder_rejects_malformed_and_noncanonical_payloads() {
        let plan = plan_select_v1(bound_select()).unwrap();
        let bytes = encode_plan_v1(&plan).unwrap();

        let mut wrong_version = bytes.clone();
        wrong_version[1] = 2;
        assert_eq!(
            decode_plan_v1(&wrong_version),
            Err(PlanError::UnsupportedVersion(2))
        );

        let mut truncated = bytes.clone();
        truncated.pop();
        assert_eq!(decode_plan_v1(&truncated), Err(PlanError::Truncated));

        let mut trailing = bytes.clone();
        trailing.push(0);
        assert_eq!(decode_plan_v1(&trailing), Err(PlanError::TrailingBytes));

        let mut unsupported_type = bytes;
        let scan_first_column_type_code = 89;
        unsupported_type[scan_first_column_type_code] = 0xff;
        assert_eq!(
            decode_plan_v1(&unsupported_type),
            Err(PlanError::InvalidEncoding)
        );

        let mut invalid_decimal = encode_plan_v1(&plan).unwrap();
        invalid_decimal[scan_first_column_type_code] = 5;
        invalid_decimal[scan_first_column_type_code + 1] = 39;
        invalid_decimal[scan_first_column_type_code + 2] = 0;
        invalid_decimal[scan_first_column_type_code + 3] = 0;
        invalid_decimal[scan_first_column_type_code + 4] = 0;
        assert_eq!(
            decode_plan_v1(&invalid_decimal),
            Err(PlanError::InvalidEncoding)
        );
    }

    #[test]
    fn text_predicate_header_matches_the_generated_wire_record() {
        let predicate = BoundPredicate::Text("hi".to_owned());
        let mut encoded = Vec::new();
        put_predicate(&mut encoded, &predicate).unwrap();
        assert_eq!(encoded, [0x03, 0x00, 0x02, b'h', b'i']);

        let mut reader = PlanReader {
            bytes: &encoded,
            offset: 0,
        };
        assert_eq!(reader.predicate(&[]).unwrap(), predicate);
        assert_eq!(reader.offset, encoded.len());
    }

    #[test]
    fn integer_predicate_record_matches_the_generated_wire_record() {
        let predicate = BoundPredicate::Integer(-2);
        let mut encoded = Vec::new();
        put_predicate(&mut encoded, &predicate).unwrap();
        assert_eq!(
            encoded,
            [0x02, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xfe]
        );

        let mut reader = PlanReader {
            bytes: &encoded,
            offset: 0,
        };
        assert_eq!(reader.predicate(&[]).unwrap(), predicate);
        assert_eq!(reader.offset, encoded.len());
    }

    #[test]
    fn column_predicate_record_matches_the_generated_wire_record() {
        let source_column = column(0xab, 0, ScalarType::Integer);
        let predicate = BoundPredicate::Column(source_column.clone());
        let mut encoded = Vec::new();
        put_predicate(&mut encoded, &predicate).unwrap();
        assert_eq!(encoded, [&[0x10][..], &[0xabu8; 16][..]].concat());

        let mut reader = PlanReader {
            bytes: &encoded,
            offset: 0,
        };
        assert_eq!(reader.predicate(&[source_column]).unwrap(), predicate);
        assert_eq!(reader.offset, encoded.len());
    }

    #[test]
    fn plan_encoder_rejects_invalid_scalar_type_parameters() {
        let mut plan = plan_select_v1(bound_select()).unwrap();
        let PlanNode::Scan { columns } = &mut plan.nodes[0] else {
            panic!("first node must scan")
        };
        columns[0].scalar_type = ScalarType::Decimal {
            precision: 0,
            scale: 0,
        };
        assert_eq!(encode_plan_v1(&plan), Err(PlanError::InvalidScalarType));
    }
}
