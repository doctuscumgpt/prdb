//! Bounded version-1 structured query requests for the isolated optimizer.
//!
//! This decoder accepts bounded single-relation SELECT/DELETE/UPDATE/group
//! requests and two-relation composite equijoin SELECT requests. It does not resolve
//! names or grant authority; those checks belong to the invocation-bound
//! planner service and executor/storage path.

use crate::query_abi::{
    OPTIMIZER_COMPOSITE_GROUP_COUNT_STATEMENT, OPTIMIZER_DELETE_STATEMENT,
    OPTIMIZER_GROUP_COUNT_STATEMENT, OPTIMIZER_JOIN_SELECT_STATEMENT, OPTIMIZER_REQUEST_VERSION,
    OPTIMIZER_SELECT_REQUEST_HEADER_BYTES, OPTIMIZER_SELECT_REQUEST_VERSION_OFFSET,
    OPTIMIZER_SELECT_STATEMENT, OPTIMIZER_UPDATE_STATEMENT, decode_optimizer_select_request,
};

pub const MAX_REQUEST_BYTES: usize = 64 * 1024;
pub const MAX_IDENTIFIER_BYTES: usize = 63;
pub const MAX_PROJECTIONS: usize = 128;
pub const MAX_PREDICATE_BYTES: usize = 16 * 1024;
pub const MAX_PREDICATE_NODES: usize = 256;
pub const MAX_PREDICATE_DEPTH: usize = 32;
pub const MAX_TEXT_LITERAL_BYTES: usize = 4096;
pub const MAX_UPDATE_ASSIGNMENTS: usize = 64;
pub const MIN_JOIN_KEYS: usize = 2;
pub const MAX_JOIN_KEYS: usize = 8;
pub const MIN_GROUP_KEYS: usize = 2;
pub const MAX_GROUP_KEYS: usize = 8;
pub const MAX_PARAMETER_INDEX: u32 = 256;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SelectRequest {
    pub schema_name: String,
    pub relation_name: String,
    pub projections: Vec<String>,
    pub predicate: Option<Predicate>,
    pub limit: Option<u32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JoinRelationSide {
    Left,
    Right,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JoinProjection {
    pub relation_side: JoinRelationSide,
    pub column_name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JoinKey {
    pub left_column_name: String,
    pub right_column_name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JoinSelectRequest {
    pub schema_name: String,
    pub left_relation_name: String,
    pub right_relation_name: String,
    pub projections: Vec<JoinProjection>,
    pub join_keys: Vec<JoinKey>,
    pub limit: Option<u32>,
}

/// Bounded first grouped-query shape: one key, COUNT(*), no predicate or
/// ordering, and an optional post-group LIMIT.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GroupCountRequest {
    pub schema_name: String,
    pub relation_name: String,
    pub group_key_name: String,
    pub limit: Option<u32>,
}

/// Bounded two-to-eight-key COUNT(*) grouping request. Keys retain projection
/// order; filters, HAVING, and ORDER BY are outside this request shape.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompositeGroupCountRequest {
    pub schema_name: String,
    pub relation_name: String,
    pub group_key_names: Vec<String>,
    pub limit: Option<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeleteRequest {
    pub schema_name: String,
    pub relation_name: String,
    pub predicate: Predicate,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpdateRequest {
    pub schema_name: String,
    pub relation_name: String,
    pub assignments: Vec<UpdateAssignment>,
    pub predicate: Predicate,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpdateAssignment {
    pub column_name: String,
    pub value: UpdateValue,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UpdateValue {
    Parameter(u32),
    Null,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Predicate {
    Null,
    Boolean(bool),
    Integer(i64),
    Text(String),
    Column(String),
    Compare {
        operator: Comparison,
        left: Box<Predicate>,
        right: Box<Predicate>,
    },
    And(Vec<Predicate>),
    Or(Vec<Predicate>),
    Not(Box<Predicate>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Comparison {
    Equal,
    NotEqual,
    Less,
    LessOrEqual,
    Greater,
    GreaterOrEqual,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RequestError {
    TooLarge,
    Truncated,
    UnsupportedVersion(u16),
    UnsupportedStatement(u8),
    InvalidValue,
    InvalidIdentifier,
    InvalidPredicate,
    PredicateTooLarge,
    TooManyProjections,
    TooManyAssignments,
    InvalidJoinKeyCount,
    InvalidAssignment,
    TooManyPredicateNodes,
    PredicateTooDeep,
    TrailingBytes,
}

impl std::fmt::Display for RequestError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "optimizer request error: {self:?}")
    }
}

impl std::error::Error for RequestError {}

/// Decode the canonical v1 request. All lengths and collection bounds are
/// checked before allocating their corresponding owned values.
pub fn decode_select_request(bytes: &[u8]) -> Result<SelectRequest, RequestError> {
    if bytes.len() > MAX_REQUEST_BYTES {
        return Err(RequestError::TooLarge);
    }
    let header_bytes = OPTIMIZER_SELECT_REQUEST_HEADER_BYTES as usize;
    if bytes.len() < header_bytes {
        return Err(RequestError::Truncated);
    }
    let version_offset = OPTIMIZER_SELECT_REQUEST_VERSION_OFFSET as usize;
    let version = u16::from_be_bytes([bytes[version_offset], bytes[version_offset + 1]]);
    if version != OPTIMIZER_REQUEST_VERSION {
        return Err(RequestError::UnsupportedVersion(version));
    }
    let (statement, flags) =
        decode_optimizer_select_request(bytes).ok_or(RequestError::Truncated)?;
    if statement != OPTIMIZER_SELECT_STATEMENT {
        return Err(RequestError::UnsupportedStatement(statement));
    }
    if flags != 0 {
        return Err(RequestError::InvalidValue);
    }
    let mut cursor = Cursor {
        bytes,
        offset: header_bytes,
    };
    let schema_name = cursor.identifier()?;
    let relation_name = cursor.identifier()?;
    let projection_count = usize::from(cursor.u16()?);
    if projection_count == 0 || projection_count > MAX_PROJECTIONS {
        return Err(RequestError::TooManyProjections);
    }
    let mut projections = Vec::with_capacity(projection_count);
    for _ in 0..projection_count {
        projections.push(cursor.identifier()?);
    }

    let predicate_len = usize::try_from(cursor.u32()?).map_err(|_| RequestError::TooLarge)?;
    if predicate_len > MAX_PREDICATE_BYTES {
        return Err(RequestError::PredicateTooLarge);
    }
    let predicate_bytes = cursor.take(predicate_len)?;
    let predicate = if predicate_bytes.is_empty() {
        None
    } else {
        let mut parser = PredicateParser::new(predicate_bytes);
        let predicate = parser.node(1)?;
        if !parser.cursor.is_empty() {
            return Err(RequestError::TrailingBytes);
        }
        Some(predicate)
    };

    let limit = match cursor.u8()? {
        0 => None,
        1 => Some(cursor.u32()?),
        _ => return Err(RequestError::InvalidValue),
    };
    if !cursor.is_empty() {
        return Err(RequestError::TrailingBytes);
    }
    Ok(SelectRequest {
        schema_name,
        relation_name,
        projections,
        predicate,
        limit,
    })
}

/// Decode one bounded, composite INNER JOIN SELECT request. The request names
/// logical relations and columns only; it contains no handle or authority.
pub fn decode_join_select_request(bytes: &[u8]) -> Result<JoinSelectRequest, RequestError> {
    if bytes.len() > MAX_REQUEST_BYTES {
        return Err(RequestError::TooLarge);
    }
    let header_bytes = OPTIMIZER_SELECT_REQUEST_HEADER_BYTES as usize;
    if bytes.len() < header_bytes {
        return Err(RequestError::Truncated);
    }
    let version_offset = OPTIMIZER_SELECT_REQUEST_VERSION_OFFSET as usize;
    let version = u16::from_be_bytes([bytes[version_offset], bytes[version_offset + 1]]);
    if version != OPTIMIZER_REQUEST_VERSION {
        return Err(RequestError::UnsupportedVersion(version));
    }
    let (statement, flags) =
        decode_optimizer_select_request(bytes).ok_or(RequestError::Truncated)?;
    if statement != OPTIMIZER_JOIN_SELECT_STATEMENT {
        return Err(RequestError::UnsupportedStatement(statement));
    }
    if flags != 0 {
        return Err(RequestError::InvalidValue);
    }
    let mut cursor = Cursor {
        bytes,
        offset: header_bytes,
    };
    let schema_name = cursor.identifier()?;
    let left_relation_name = cursor.identifier()?;
    let right_relation_name = cursor.identifier()?;
    let projection_count = usize::from(cursor.u16()?);
    if projection_count == 0 || projection_count > MAX_PROJECTIONS {
        return Err(RequestError::TooManyProjections);
    }
    let mut projections = Vec::with_capacity(projection_count);
    for _ in 0..projection_count {
        let relation_side = match cursor.u8()? {
            0 => JoinRelationSide::Left,
            1 => JoinRelationSide::Right,
            _ => return Err(RequestError::InvalidValue),
        };
        projections.push(JoinProjection {
            relation_side,
            column_name: cursor.identifier()?,
        });
    }
    let join_key_count = usize::from(cursor.u8()?);
    if !(MIN_JOIN_KEYS..=MAX_JOIN_KEYS).contains(&join_key_count) {
        return Err(RequestError::InvalidJoinKeyCount);
    }
    let mut join_keys = Vec::with_capacity(join_key_count);
    for _ in 0..join_key_count {
        join_keys.push(JoinKey {
            left_column_name: cursor.identifier()?,
            right_column_name: cursor.identifier()?,
        });
    }
    let limit = match cursor.u8()? {
        0 => None,
        1 => Some(cursor.u32()?),
        _ => return Err(RequestError::InvalidValue),
    };
    if !cursor.is_empty() {
        return Err(RequestError::TrailingBytes);
    }
    Ok(JoinSelectRequest {
        schema_name,
        left_relation_name,
        right_relation_name,
        projections,
        join_keys,
        limit,
    })
}

/// Decode the canonical single-key COUNT(*) request. Filters, HAVING, and
/// ORDER BY are intentionally absent from this first grouped-plan version.
pub fn decode_group_count_request(bytes: &[u8]) -> Result<GroupCountRequest, RequestError> {
    if bytes.len() > MAX_REQUEST_BYTES {
        return Err(RequestError::TooLarge);
    }
    let header_bytes = OPTIMIZER_SELECT_REQUEST_HEADER_BYTES as usize;
    if bytes.len() < header_bytes {
        return Err(RequestError::Truncated);
    }
    let version_offset = OPTIMIZER_SELECT_REQUEST_VERSION_OFFSET as usize;
    let version = u16::from_be_bytes([bytes[version_offset], bytes[version_offset + 1]]);
    if version != OPTIMIZER_REQUEST_VERSION {
        return Err(RequestError::UnsupportedVersion(version));
    }
    let (statement, flags) =
        decode_optimizer_select_request(bytes).ok_or(RequestError::Truncated)?;
    if statement != OPTIMIZER_GROUP_COUNT_STATEMENT {
        return Err(RequestError::UnsupportedStatement(statement));
    }
    if flags != 0 {
        return Err(RequestError::InvalidValue);
    }
    let mut cursor = Cursor {
        bytes,
        offset: header_bytes,
    };
    let schema_name = cursor.identifier()?;
    let relation_name = cursor.identifier()?;
    let group_key_name = cursor.identifier()?;
    let limit = match cursor.u8()? {
        0 => None,
        1 => Some(cursor.u32()?),
        _ => return Err(RequestError::InvalidValue),
    };
    if !cursor.is_empty() {
        return Err(RequestError::TrailingBytes);
    }
    Ok(GroupCountRequest {
        schema_name,
        relation_name,
        group_key_name,
        limit,
    })
}

/// Decode a canonical composite COUNT(*) request with an optional post-group
/// LIMIT. Column names remain untrusted until bound against authorized metadata.
pub fn decode_composite_group_count_request(
    bytes: &[u8],
) -> Result<CompositeGroupCountRequest, RequestError> {
    if bytes.len() > MAX_REQUEST_BYTES {
        return Err(RequestError::TooLarge);
    }
    let header_bytes = OPTIMIZER_SELECT_REQUEST_HEADER_BYTES as usize;
    if bytes.len() < header_bytes {
        return Err(RequestError::Truncated);
    }
    let version_offset = OPTIMIZER_SELECT_REQUEST_VERSION_OFFSET as usize;
    let version = u16::from_be_bytes([bytes[version_offset], bytes[version_offset + 1]]);
    if version != OPTIMIZER_REQUEST_VERSION {
        return Err(RequestError::UnsupportedVersion(version));
    }
    let (statement, flags) =
        decode_optimizer_select_request(bytes).ok_or(RequestError::Truncated)?;
    if statement != OPTIMIZER_COMPOSITE_GROUP_COUNT_STATEMENT {
        return Err(RequestError::UnsupportedStatement(statement));
    }
    if flags != 0 {
        return Err(RequestError::InvalidValue);
    }
    let mut cursor = Cursor {
        bytes,
        offset: header_bytes,
    };
    let schema_name = cursor.identifier()?;
    let relation_name = cursor.identifier()?;
    let key_count = usize::from(cursor.u8()?);
    if !(MIN_GROUP_KEYS..=MAX_GROUP_KEYS).contains(&key_count) {
        return Err(RequestError::InvalidValue);
    }
    let mut group_key_names = Vec::with_capacity(key_count);
    for _ in 0..key_count {
        group_key_names.push(cursor.identifier()?);
    }
    let limit = match cursor.u8()? {
        0 => None,
        1 => Some(cursor.u32()?),
        _ => return Err(RequestError::InvalidValue),
    };
    if !cursor.is_empty() {
        return Err(RequestError::TrailingBytes);
    }
    Ok(CompositeGroupCountRequest {
        schema_name,
        relation_name,
        group_key_names,
        limit,
    })
}

/// Decode a canonical DELETE request. DELETE requires a bounded WHERE tree;
/// the encoded relation and predicate confer no storage authority.
pub fn decode_delete_request(bytes: &[u8]) -> Result<DeleteRequest, RequestError> {
    if bytes.len() > MAX_REQUEST_BYTES {
        return Err(RequestError::TooLarge);
    }
    let header_bytes = OPTIMIZER_SELECT_REQUEST_HEADER_BYTES as usize;
    if bytes.len() < header_bytes {
        return Err(RequestError::Truncated);
    }
    let version_offset = OPTIMIZER_SELECT_REQUEST_VERSION_OFFSET as usize;
    let version = u16::from_be_bytes([bytes[version_offset], bytes[version_offset + 1]]);
    if version != OPTIMIZER_REQUEST_VERSION {
        return Err(RequestError::UnsupportedVersion(version));
    }
    let (statement, flags) =
        decode_optimizer_select_request(bytes).ok_or(RequestError::Truncated)?;
    if statement != OPTIMIZER_DELETE_STATEMENT {
        return Err(RequestError::UnsupportedStatement(statement));
    }
    if flags != 0 {
        return Err(RequestError::InvalidValue);
    }
    let mut cursor = Cursor {
        bytes,
        offset: header_bytes,
    };
    let schema_name = cursor.identifier()?;
    let relation_name = cursor.identifier()?;
    let predicate_len = usize::try_from(cursor.u32()?).map_err(|_| RequestError::TooLarge)?;
    if predicate_len == 0 {
        return Err(RequestError::InvalidPredicate);
    }
    if predicate_len > MAX_PREDICATE_BYTES {
        return Err(RequestError::PredicateTooLarge);
    }
    let predicate_bytes = cursor.take(predicate_len)?;
    let mut parser = PredicateParser::new(predicate_bytes);
    let predicate = parser.node(1)?;
    if !parser.cursor.is_empty() || !cursor.is_empty() {
        return Err(RequestError::TrailingBytes);
    }
    Ok(DeleteRequest {
        schema_name,
        relation_name,
        predicate,
    })
}

/// Decode a canonical UPDATE request. Assignments are bounded parameter or
/// NULL values and a Boolean WHERE tree is mandatory; the request grants no
/// executor or mutation capability.
pub fn decode_update_request(bytes: &[u8]) -> Result<UpdateRequest, RequestError> {
    if bytes.len() > MAX_REQUEST_BYTES {
        return Err(RequestError::TooLarge);
    }
    let header_bytes = OPTIMIZER_SELECT_REQUEST_HEADER_BYTES as usize;
    if bytes.len() < header_bytes {
        return Err(RequestError::Truncated);
    }
    let version_offset = OPTIMIZER_SELECT_REQUEST_VERSION_OFFSET as usize;
    let version = u16::from_be_bytes([bytes[version_offset], bytes[version_offset + 1]]);
    if version != OPTIMIZER_REQUEST_VERSION {
        return Err(RequestError::UnsupportedVersion(version));
    }
    let (statement, flags) =
        decode_optimizer_select_request(bytes).ok_or(RequestError::Truncated)?;
    if statement != OPTIMIZER_UPDATE_STATEMENT {
        return Err(RequestError::UnsupportedStatement(statement));
    }
    if flags != 0 {
        return Err(RequestError::InvalidValue);
    }
    let mut cursor = Cursor {
        bytes,
        offset: header_bytes,
    };
    let schema_name = cursor.identifier()?;
    let relation_name = cursor.identifier()?;
    let assignment_count = usize::from(cursor.u16()?);
    if assignment_count == 0 || assignment_count > MAX_UPDATE_ASSIGNMENTS {
        return Err(RequestError::TooManyAssignments);
    }
    let mut assignments = Vec::with_capacity(assignment_count);
    for _ in 0..assignment_count {
        let column_name = cursor.identifier()?;
        let value = match cursor.u8()? {
            0 => {
                let index = cursor.u32()?;
                if index == 0 || index > MAX_PARAMETER_INDEX {
                    return Err(RequestError::InvalidAssignment);
                }
                UpdateValue::Parameter(index)
            }
            1 => UpdateValue::Null,
            _ => return Err(RequestError::InvalidAssignment),
        };
        assignments.push(UpdateAssignment { column_name, value });
    }
    let predicate_len = usize::try_from(cursor.u32()?).map_err(|_| RequestError::TooLarge)?;
    if predicate_len == 0 {
        return Err(RequestError::InvalidPredicate);
    }
    if predicate_len > MAX_PREDICATE_BYTES {
        return Err(RequestError::PredicateTooLarge);
    }
    let predicate_bytes = cursor.take(predicate_len)?;
    let mut parser = PredicateParser::new(predicate_bytes);
    let predicate = parser.node(1)?;
    if !parser.cursor.is_empty() || !cursor.is_empty() {
        return Err(RequestError::TrailingBytes);
    }
    Ok(UpdateRequest {
        schema_name,
        relation_name,
        assignments,
        predicate,
    })
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], RequestError> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or(RequestError::TooLarge)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(RequestError::Truncated)?;
        self.offset = end;
        Ok(value)
    }

    fn u8(&mut self) -> Result<u8, RequestError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, RequestError> {
        Ok(u16::from_be_bytes(
            self.take(2)?
                .try_into()
                .map_err(|_| RequestError::Truncated)?,
        ))
    }

    fn u32(&mut self) -> Result<u32, RequestError> {
        Ok(u32::from_be_bytes(
            self.take(4)?
                .try_into()
                .map_err(|_| RequestError::Truncated)?,
        ))
    }

    fn identifier(&mut self) -> Result<String, RequestError> {
        let length = usize::from(self.u8()?);
        if length == 0 || length > MAX_IDENTIFIER_BYTES {
            return Err(RequestError::InvalidIdentifier);
        }
        let bytes = self.take(length)?;
        if !bytes[0].is_ascii_alphabetic() && bytes[0] != b'_' {
            return Err(RequestError::InvalidIdentifier);
        }
        if !bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
        {
            return Err(RequestError::InvalidIdentifier);
        }
        String::from_utf8(bytes.to_vec()).map_err(|_| RequestError::InvalidIdentifier)
    }

    fn is_empty(&self) -> bool {
        self.offset == self.bytes.len()
    }
}

struct PredicateParser<'a> {
    cursor: Cursor<'a>,
    nodes: usize,
}

impl<'a> PredicateParser<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            cursor: Cursor::new(bytes),
            nodes: 0,
        }
    }

    fn node(&mut self, depth: usize) -> Result<Predicate, RequestError> {
        if depth > MAX_PREDICATE_DEPTH {
            return Err(RequestError::PredicateTooDeep);
        }
        self.nodes += 1;
        if self.nodes > MAX_PREDICATE_NODES {
            return Err(RequestError::TooManyPredicateNodes);
        }
        let tag = self.cursor.u8()?;
        match tag {
            0x00 => Ok(Predicate::Null),
            0x01 => match self.cursor.u8()? {
                0 => Ok(Predicate::Boolean(false)),
                1 => Ok(Predicate::Boolean(true)),
                _ => Err(RequestError::InvalidValue),
            },
            0x02 => Ok(Predicate::Integer(i64::from_be_bytes(
                self.cursor
                    .take(8)?
                    .try_into()
                    .map_err(|_| RequestError::Truncated)?,
            ))),
            0x03 => {
                let length = usize::from(self.cursor.u16()?);
                if length > MAX_TEXT_LITERAL_BYTES {
                    return Err(RequestError::InvalidValue);
                }
                let text = std::str::from_utf8(self.cursor.take(length)?)
                    .map_err(|_| RequestError::InvalidValue)?
                    .to_owned();
                Ok(Predicate::Text(text))
            }
            0x10 => Ok(Predicate::Column(self.cursor.identifier()?)),
            0x20 => {
                let operator = match self.cursor.u8()? {
                    0 => Comparison::Equal,
                    1 => Comparison::NotEqual,
                    2 => Comparison::Less,
                    3 => Comparison::LessOrEqual,
                    4 => Comparison::Greater,
                    5 => Comparison::GreaterOrEqual,
                    _ => return Err(RequestError::InvalidPredicate),
                };
                let left = Box::new(self.node(depth + 1)?);
                let right = Box::new(self.node(depth + 1)?);
                Ok(Predicate::Compare {
                    operator,
                    left,
                    right,
                })
            }
            0x30 | 0x31 => {
                let count = usize::from(self.cursor.u8()?);
                if !(2..=16).contains(&count) {
                    return Err(RequestError::InvalidPredicate);
                }
                let mut children = Vec::with_capacity(count);
                for _ in 0..count {
                    children.push(self.node(depth + 1)?);
                }
                if tag == 0x30 {
                    Ok(Predicate::And(children))
                } else {
                    Ok(Predicate::Or(children))
                }
            }
            0x32 => Ok(Predicate::Not(Box::new(self.node(depth + 1)?))),
            _ => Err(RequestError::InvalidPredicate),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u16_bytes(value: usize, output: &mut Vec<u8>) {
        output.extend_from_slice(&(value as u16).to_be_bytes());
    }

    fn u32_bytes(value: usize, output: &mut Vec<u8>) {
        output.extend_from_slice(&(value as u32).to_be_bytes());
    }

    fn name(value: &str, output: &mut Vec<u8>) {
        output.push(value.len() as u8);
        output.extend_from_slice(value.as_bytes());
    }

    fn select_request(predicate: &[u8]) -> Vec<u8> {
        let mut bytes = vec![0, OPTIMIZER_REQUEST_VERSION as u8];
        bytes.push(OPTIMIZER_SELECT_STATEMENT);
        bytes.push(0);
        name("public", &mut bytes);
        name("items", &mut bytes);
        u16_bytes(2, &mut bytes);
        name("id", &mut bytes);
        name("label", &mut bytes);
        u32_bytes(predicate.len(), &mut bytes);
        bytes.extend_from_slice(predicate);
        bytes.extend_from_slice(&[1, 0, 0, 0, 10]);
        bytes
    }

    fn delete_request(predicate: &[u8]) -> Vec<u8> {
        let mut bytes = vec![0, OPTIMIZER_REQUEST_VERSION as u8, 2, 0];
        name("public", &mut bytes);
        name("items", &mut bytes);
        u32_bytes(predicate.len(), &mut bytes);
        bytes.extend_from_slice(predicate);
        bytes
    }

    fn update_request(predicate: &[u8]) -> Vec<u8> {
        let mut bytes = vec![0, OPTIMIZER_REQUEST_VERSION as u8, 3, 0];
        name("public", &mut bytes);
        name("items", &mut bytes);
        u16_bytes(2, &mut bytes);
        name("label", &mut bytes);
        bytes.push(0);
        bytes.extend_from_slice(&1u32.to_be_bytes());
        name("active", &mut bytes);
        bytes.push(1);
        u32_bytes(predicate.len(), &mut bytes);
        bytes.extend_from_slice(predicate);
        bytes
    }

    fn join_select_request() -> Vec<u8> {
        let mut bytes = vec![0, OPTIMIZER_REQUEST_VERSION as u8, 4, 0];
        name("public", &mut bytes);
        name("lefts", &mut bytes);
        name("rights", &mut bytes);
        u16_bytes(2, &mut bytes);
        bytes.extend_from_slice(&[0]);
        name("label", &mut bytes);
        bytes.extend_from_slice(&[1]);
        name("value", &mut bytes);
        bytes.push(2);
        name("id", &mut bytes);
        name("id", &mut bytes);
        name("region", &mut bytes);
        name("region", &mut bytes);
        bytes.extend_from_slice(&[1, 0, 0, 0, 5]);
        bytes
    }

    fn group_count_request() -> Vec<u8> {
        let mut bytes = vec![0, OPTIMIZER_REQUEST_VERSION as u8];
        bytes.push(OPTIMIZER_GROUP_COUNT_STATEMENT);
        bytes.push(0);
        name("public", &mut bytes);
        name("items", &mut bytes);
        name("category", &mut bytes);
        bytes.extend_from_slice(&[1, 0, 0, 0, 5]);
        bytes
    }

    fn composite_group_count_request(keys: &[&str]) -> Vec<u8> {
        let mut bytes = vec![0, OPTIMIZER_REQUEST_VERSION as u8];
        bytes.push(OPTIMIZER_COMPOSITE_GROUP_COUNT_STATEMENT);
        bytes.push(0);
        name("public", &mut bytes);
        name("items", &mut bytes);
        bytes.push(keys.len() as u8);
        for key in keys {
            name(key, &mut bytes);
        }
        bytes.extend_from_slice(&[1, 0, 0, 0, 5]);
        bytes
    }

    #[test]
    fn decodes_bounded_single_key_group_count_request() {
        let request = decode_group_count_request(&group_count_request()).unwrap();
        assert_eq!(request.schema_name, "public");
        assert_eq!(request.relation_name, "items");
        assert_eq!(request.group_key_name, "category");
        assert_eq!(request.limit, Some(5));

        let mut trailing = group_count_request();
        trailing.push(0);
        assert_eq!(
            decode_group_count_request(&trailing),
            Err(RequestError::TrailingBytes)
        );
    }

    #[test]
    fn decodes_bounded_composite_group_count_request() {
        let request = decode_composite_group_count_request(&composite_group_count_request(&[
            "category", "active",
        ]))
        .unwrap();
        assert_eq!(request.schema_name, "public");
        assert_eq!(request.relation_name, "items");
        assert_eq!(request.group_key_names, ["category", "active"]);
        assert_eq!(request.limit, Some(5));

        for count in [0, 1, 9] {
            let invalid = composite_group_count_request(&vec!["id"; count]);
            assert_eq!(
                decode_composite_group_count_request(&invalid),
                Err(RequestError::InvalidValue)
            );
        }
        let mut trailing = composite_group_count_request(&["id", "active"]);
        trailing.push(0);
        assert_eq!(
            decode_composite_group_count_request(&trailing),
            Err(RequestError::TrailingBytes)
        );
    }

    #[test]
    fn decodes_bounded_composite_join_select_request() {
        let request = decode_join_select_request(&join_select_request()).unwrap();
        assert_eq!(request.schema_name, "public");
        assert_eq!(request.left_relation_name, "lefts");
        assert_eq!(request.right_relation_name, "rights");
        assert_eq!(
            request.projections,
            vec![
                JoinProjection {
                    relation_side: JoinRelationSide::Left,
                    column_name: "label".to_owned(),
                },
                JoinProjection {
                    relation_side: JoinRelationSide::Right,
                    column_name: "value".to_owned(),
                },
            ]
        );
        assert_eq!(
            request.join_keys,
            vec![
                JoinKey {
                    left_column_name: "id".to_owned(),
                    right_column_name: "id".to_owned(),
                },
                JoinKey {
                    left_column_name: "region".to_owned(),
                    right_column_name: "region".to_owned(),
                },
            ]
        );
        assert_eq!(request.limit, Some(5));
    }

    #[test]
    fn join_select_request_rejects_bad_sides_key_counts_and_trailing_bytes() {
        let mut bad_side = join_select_request();
        bad_side[4 + 1 + "public".len() + 1 + "lefts".len() + 1 + "rights".len() + 2] = 2;
        assert_eq!(
            decode_join_select_request(&bad_side),
            Err(RequestError::InvalidValue)
        );
        let mut too_few_keys = join_select_request();
        let key_count_offset = 4
            + 1
            + "public".len()
            + 1
            + "lefts".len()
            + 1
            + "rights".len()
            + 2
            + 1
            + 1
            + "label".len()
            + 1
            + 1
            + "value".len();
        too_few_keys[key_count_offset] = 1;
        assert_eq!(
            decode_join_select_request(&too_few_keys),
            Err(RequestError::InvalidJoinKeyCount)
        );
        let mut trailing = join_select_request();
        trailing.push(0);
        assert_eq!(
            decode_join_select_request(&trailing),
            Err(RequestError::TrailingBytes)
        );
    }

    #[test]
    fn decodes_bounded_delete_request_without_projection_or_limit() {
        let predicate = [0x20, 0, 0x10, 2, b'i', b'd', 0x02, 0, 0, 0, 0, 0, 0, 0, 7];
        let request = decode_delete_request(&delete_request(&predicate)).unwrap();
        assert_eq!(request.schema_name, "public");
        assert_eq!(request.relation_name, "items");
        assert_eq!(
            request.predicate,
            Predicate::Compare {
                operator: Comparison::Equal,
                left: Box::new(Predicate::Column("id".to_owned())),
                right: Box::new(Predicate::Integer(7)),
            }
        );
    }

    #[test]
    fn delete_request_requires_where_and_rejects_trailing_data() {
        assert_eq!(
            decode_delete_request(&delete_request(&[])),
            Err(RequestError::InvalidPredicate)
        );
        let mut trailing = delete_request(&[0x01, 1]);
        trailing.push(0);
        assert_eq!(
            decode_delete_request(&trailing),
            Err(RequestError::TrailingBytes)
        );
        assert_eq!(
            decode_delete_request(&select_request(&[])),
            Err(RequestError::UnsupportedStatement(
                OPTIMIZER_SELECT_STATEMENT
            ))
        );
    }

    #[test]
    fn decodes_bounded_update_assignments_and_required_predicate() {
        let predicate = [0x20, 0, 0x10, 2, b'i', b'd', 0x02, 0, 0, 0, 0, 0, 0, 0, 7];
        let request = decode_update_request(&update_request(&predicate)).unwrap();
        assert_eq!(request.schema_name, "public");
        assert_eq!(request.relation_name, "items");
        assert_eq!(
            request.assignments,
            [
                UpdateAssignment {
                    column_name: "label".to_owned(),
                    value: UpdateValue::Parameter(1),
                },
                UpdateAssignment {
                    column_name: "active".to_owned(),
                    value: UpdateValue::Null,
                },
            ]
        );
        assert_eq!(
            request.predicate,
            Predicate::Compare {
                operator: Comparison::Equal,
                left: Box::new(Predicate::Column("id".to_owned())),
                right: Box::new(Predicate::Integer(7)),
            }
        );
    }

    #[test]
    fn update_request_rejects_invalid_assignment_and_missing_where() {
        assert_eq!(
            decode_update_request(&update_request(&[])),
            Err(RequestError::InvalidPredicate)
        );
        let mut invalid_kind = update_request(&[0x01, 1]);
        let first_assignment_value =
            4 + 1 + "public".len() + 1 + "items".len() + 2 + 1 + "label".len();
        invalid_kind[first_assignment_value] = 9;
        assert_eq!(
            decode_update_request(&invalid_kind),
            Err(RequestError::InvalidAssignment)
        );
        let mut zero_parameter = update_request(&[0x01, 1]);
        zero_parameter[first_assignment_value + 1..first_assignment_value + 5]
            .copy_from_slice(&0u32.to_be_bytes());
        assert_eq!(
            decode_update_request(&zero_parameter),
            Err(RequestError::InvalidAssignment)
        );
        let mut trailing = update_request(&[0x01, 1]);
        trailing.push(0);
        assert_eq!(
            decode_update_request(&trailing),
            Err(RequestError::TrailingBytes)
        );
    }

    #[test]
    fn decodes_bounded_single_relation_request_and_predicate() {
        let mut expression = vec![0x30, 2, 0x20, 0, 0x10];
        name("id", &mut expression);
        expression.push(0x02);
        expression.extend_from_slice(&7i64.to_be_bytes());
        expression.extend_from_slice(&[0x20, 4, 0x10]);
        name("label", &mut expression);
        expression.push(0x03);
        u16_bytes(3, &mut expression);
        expression.extend_from_slice(b"new");

        let request = decode_select_request(&select_request(&expression)).unwrap();
        assert_eq!(request.schema_name, "public");
        assert_eq!(request.relation_name, "items");
        assert_eq!(request.projections, ["id", "label"]);
        assert_eq!(request.limit, Some(10));
        assert_eq!(
            request.predicate,
            Some(Predicate::And(vec![
                Predicate::Compare {
                    operator: Comparison::Equal,
                    left: Box::new(Predicate::Column("id".to_owned())),
                    right: Box::new(Predicate::Integer(7)),
                },
                Predicate::Compare {
                    operator: Comparison::Greater,
                    left: Box::new(Predicate::Column("label".to_owned())),
                    right: Box::new(Predicate::Text("new".to_owned())),
                },
            ]))
        );
    }

    #[test]
    fn rejects_malformed_outer_records_and_identifiers() {
        assert_eq!(decode_select_request(&[]), Err(RequestError::Truncated));
        let mut wrong_version = select_request(&[]);
        wrong_version[1] = 2;
        assert_eq!(
            decode_select_request(&wrong_version),
            Err(RequestError::UnsupportedVersion(2))
        );
        let mut unknown_statement = select_request(&[]);
        unknown_statement[2] = 9;
        assert_eq!(
            decode_select_request(&unknown_statement),
            Err(RequestError::UnsupportedStatement(9))
        );
        let mut invalid_name = select_request(&[]);
        invalid_name[4] = 1;
        invalid_name[5] = b'1';
        assert_eq!(
            decode_select_request(&invalid_name),
            Err(RequestError::InvalidIdentifier)
        );
        let mut trailing = select_request(&[]);
        trailing.push(0);
        assert_eq!(
            decode_select_request(&trailing),
            Err(RequestError::TrailingBytes)
        );
        assert_eq!(
            decode_select_request(&vec![0; MAX_REQUEST_BYTES + 1]),
            Err(RequestError::TooLarge)
        );
    }

    #[test]
    fn rejects_excessive_collections_and_unsupported_nodes() {
        let mut too_many_projection = select_request(&[]);
        let projection_count_offset = 4 + 1 + "public".len() + 1 + "items".len();
        too_many_projection[projection_count_offset..projection_count_offset + 2]
            .copy_from_slice(&((MAX_PROJECTIONS + 1) as u16).to_be_bytes());
        assert_eq!(
            decode_select_request(&too_many_projection),
            Err(RequestError::TooManyProjections)
        );
        assert_eq!(
            decode_select_request(&select_request(&[0x7f])),
            Err(RequestError::InvalidPredicate)
        );
        assert_eq!(
            decode_select_request(&select_request(&[0x01, 2])),
            Err(RequestError::InvalidValue)
        );
    }

    #[test]
    fn predicate_depth_and_node_budgets_fail_closed() {
        let mut deep = vec![0x32; MAX_PREDICATE_DEPTH + 1];
        deep.push(0x00);
        assert_eq!(
            decode_select_request(&select_request(&deep)),
            Err(RequestError::PredicateTooDeep)
        );

        let mut wide = vec![0x30, 16];
        for _ in 0..16 {
            wide.push(0x30);
            wide.push(16);
            wide.extend(std::iter::repeat_n(0x00, 16));
        }
        assert_eq!(
            decode_select_request(&select_request(&wide)),
            Err(RequestError::TooManyPredicateNodes)
        );
        assert_eq!(
            decode_select_request(&select_request(&vec![0x00; MAX_PREDICATE_BYTES + 1])),
            Err(RequestError::PredicateTooLarge)
        );
    }
}
