//! Bounded SQL DDL accepted by the authenticated administrative client.
//!
//! This parser only translates a deliberately small CREATE TABLE grammar to
//! the existing typed control operation. Authorization and catalog validation
//! remain server-side.

use crate::{
    client_submit::{AdminColumnDefault, AdminColumnDefinition},
    tuple::{ScalarType, ScalarValue},
};

const MAX_DDL_BYTES: usize = 65_536;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateTableSql {
    pub schema: String,
    pub table: String,
    pub columns: Vec<AdminColumnDefinition>,
    pub primary_key_columns: Option<Vec<String>>,
    pub unique_constraints: Vec<Vec<String>>,
    pub check_constraints: Vec<CheckExpressionSql>,
    pub foreign_keys: Vec<CreateTableForeignKeySql>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateTableForeignKeySql {
    pub columns: Vec<String>,
    pub referenced_schema: String,
    pub referenced_table: String,
    pub referenced_columns: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateSchemaSql {
    pub schema: String,
    pub owner_id: [u8; 16],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenameSchemaSql {
    pub schema: String,
    pub new_name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenameTableSql {
    pub schema: String,
    pub table: String,
    pub new_name: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyConstraintKind {
    PrimaryKey,
    Unique,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AddKeyConstraintSql {
    pub schema: String,
    pub table: String,
    pub columns: Vec<String>,
    pub kind: KeyConstraintKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AddForeignKeySql {
    pub schema: String,
    pub table: String,
    pub columns: Vec<String>,
    pub referenced_schema: String,
    pub referenced_table: String,
    pub referenced_columns: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DropConstraintSql {
    pub schema: String,
    pub table: String,
    pub constraint_id: [u8; 16],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AddCheckConstraintSql {
    pub schema: String,
    pub table: String,
    pub expression: CheckExpressionSql,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AlterColumnNullabilitySql {
    pub schema: String,
    pub table: String,
    pub column: String,
    pub nullable: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AlterColumnDefaultSql {
    pub schema: String,
    pub table: String,
    pub column: String,
    pub default: Option<DefaultLiteralSql>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DefaultLiteralSql {
    Null,
    CurrentPrincipalId,
    Boolean(bool),
    Number(String),
    String(String),
    Bytea(Vec<u8>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AddColumnSql {
    pub schema: String,
    pub table: String,
    pub column: AdminColumnDefinition,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenameColumnSql {
    pub schema: String,
    pub table: String,
    pub column: String,
    pub new_name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DropColumnSql {
    pub schema: String,
    pub table: String,
    pub column: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateIndexSql {
    pub schema: String,
    pub table: String,
    pub name: String,
    pub columns: Vec<String>,
    pub unique: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CheckExpressionSql {
    Column(String),
    Literal(CheckLiteralSql),
    IsNull(String),
    IsNotNull(String),
    Compare {
        operator: CheckComparisonOperator,
        left: Box<CheckExpressionSql>,
        right: Box<CheckExpressionSql>,
    },
    Not(Box<CheckExpressionSql>),
    And(Vec<CheckExpressionSql>),
    Or(Vec<CheckExpressionSql>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CheckComparisonOperator {
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CheckLiteralSql {
    Null,
    Boolean(bool),
    Number(String),
    String(String),
    Bytea(String),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DdlSqlError {
    InvalidStatement,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Token {
    Word(String),
    Number(String),
    String(String),
    Minus,
    Plus,
    Dot,
    Comma,
    LeftParen,
    RightParen,
    Semicolon,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
}

pub fn parse_create_table(sql: &str) -> Result<CreateTableSql, DdlSqlError> {
    if sql.is_empty() || sql.len() > MAX_DDL_BYTES || sql.as_bytes().contains(&0) {
        return Err(DdlSqlError::InvalidStatement);
    }
    let tokens = tokenize(sql)?;
    let mut parser = Parser {
        tokens,
        position: 0,
    };
    parser.word("create")?;
    parser.word("table")?;
    let schema = parser.identifier()?;
    parser.take(Token::Dot)?;
    let table = parser.identifier()?;
    parser.take(Token::LeftParen)?;
    let mut columns = Vec::new();
    let mut primary_key_columns = None;
    let mut unique_constraints = Vec::new();
    let mut check_constraints = Vec::new();
    let mut foreign_keys = Vec::new();
    let mut saw_table_constraint = false;
    loop {
        if parser.peek_word("primary")
            || parser.peek_word("unique")
            || parser.peek_word("check")
            || parser.peek_word("foreign")
        {
            saw_table_constraint = true;
            if parser.peek_word("check") {
                let expression = parser.check_constraint_expression()?;
                check_constraints.push(expression);
            } else if parser.peek_word("foreign") {
                if foreign_keys.len() >= crate::tuple::MAX_COLUMNS {
                    return Err(DdlSqlError::InvalidStatement);
                }
                parser.word("foreign")?;
                parser.word("key")?;
                let columns = parse_column_name_list(&mut parser)?;
                parser.word("references")?;
                let referenced_schema = parser.identifier()?;
                parser.take(Token::Dot)?;
                let referenced_table = parser.identifier()?;
                let referenced_columns = parse_column_name_list(&mut parser)?;
                if columns.len() > crate::tuple::MAX_COLUMNS
                    || columns.len() != referenced_columns.len()
                    || has_duplicates(&columns)
                    || has_duplicates(&referenced_columns)
                {
                    return Err(DdlSqlError::InvalidStatement);
                }
                foreign_keys.push(CreateTableForeignKeySql {
                    columns,
                    referenced_schema,
                    referenced_table,
                    referenced_columns,
                });
            } else {
                let kind = if parser.peek_word("primary") {
                    parser.word("primary")?;
                    parser.word("key")?;
                    2
                } else {
                    parser.word("unique")?;
                    3
                };
                parser.take(Token::LeftParen)?;
                let mut names = vec![parser.identifier()?];
                while parser.peek() == Some(&Token::Comma) {
                    parser.position += 1;
                    names.push(parser.identifier()?);
                }
                parser.take(Token::RightParen)?;
                if names.len() > crate::tuple::MAX_COLUMNS
                    || names.iter().collect::<std::collections::HashSet<_>>().len() != names.len()
                {
                    return Err(DdlSqlError::InvalidStatement);
                }
                if kind == 2 {
                    if primary_key_columns.replace(names).is_some() {
                        return Err(DdlSqlError::InvalidStatement);
                    }
                } else {
                    if unique_constraints.len() >= crate::tuple::MAX_COLUMNS
                        || unique_constraints.contains(&names)
                    {
                        return Err(DdlSqlError::InvalidStatement);
                    }
                    unique_constraints.push(names);
                }
            }
            if parser.peek() == Some(&Token::Comma) {
                parser.position += 1;
                continue;
            }
            break;
        }
        if saw_table_constraint {
            return Err(DdlSqlError::InvalidStatement);
        }
        if columns.len() == 4096 {
            return Err(DdlSqlError::InvalidStatement);
        }
        let name = parser.identifier()?;
        let scalar_type = parser.scalar_type()?;
        let generated_uuid_identity = if parser.peek_word("generated") {
            parser.word("generated")?;
            parser.word("always")?;
            parser.word("as")?;
            parser.word("identity")?;
            if scalar_type != ScalarType::Uuid {
                return Err(DdlSqlError::InvalidStatement);
            }
            true
        } else {
            false
        };
        let mut nullable = true;
        let mut default = None;
        loop {
            if parser.peek_word("not") && nullable {
                parser.word("not")?;
                parser.word("null")?;
                nullable = false;
            } else if parser.peek_word("default") && default.is_none() {
                parser.word("default")?;
                default = Some(parser.default_value(scalar_type)?);
            } else {
                break;
            }
        }
        if generated_uuid_identity {
            if default.is_some() {
                return Err(DdlSqlError::InvalidStatement);
            }
            nullable = false;
        }
        columns.push(AdminColumnDefinition {
            name,
            scalar_type,
            nullable,
            generated_uuid_identity,
            default,
        });
        if parser.peek() == Some(&Token::Comma) {
            parser.position += 1;
            continue;
        }
        break;
    }
    parser.take(Token::RightParen)?;
    if parser.peek() == Some(&Token::Semicolon) {
        parser.position += 1;
    }
    if parser.peek().is_some() {
        return Err(DdlSqlError::InvalidStatement);
    }
    if let Some(key_columns) = &primary_key_columns {
        for column_name in key_columns {
            let column = columns
                .iter_mut()
                .find(|column| &column.name == column_name)
                .ok_or(DdlSqlError::InvalidStatement)?;
            column.nullable = false;
        }
    }
    if unique_constraints.iter().any(|key_columns| {
        key_columns
            .iter()
            .any(|name| !columns.iter().any(|column| &column.name == name))
    }) {
        return Err(DdlSqlError::InvalidStatement);
    }
    for expression in &check_constraints {
        validate_check_columns(expression, &columns)?;
    }
    if foreign_keys.iter().any(|foreign_key| {
        foreign_key
            .columns
            .iter()
            .any(|name| !columns.iter().any(|column| &column.name == name))
    }) {
        return Err(DdlSqlError::InvalidStatement);
    }
    Ok(CreateTableSql {
        schema,
        table,
        columns,
        primary_key_columns,
        unique_constraints,
        check_constraints,
        foreign_keys,
    })
}

/// Parse a bounded ordinary or unique B-tree index definition.
pub fn parse_create_index(sql: &str) -> Result<CreateIndexSql, DdlSqlError> {
    let mut parser = statement_parser(sql)?;
    parser.word("create")?;
    let unique = if parser.peek_word("unique") {
        parser.word("unique")?;
        true
    } else {
        false
    };
    parser.word("index")?;
    let name = parser.identifier()?;
    parser.word("on")?;
    let schema = parser.identifier()?;
    parser.take(Token::Dot)?;
    let table = parser.identifier()?;
    if parser.peek_word("using") {
        parser.word("using")?;
        parser.word("btree")?;
    }
    parser.take(Token::LeftParen)?;
    let mut columns = vec![parser.identifier()?];
    while parser.peek() == Some(&Token::Comma) {
        parser.position += 1;
        if columns.len() >= crate::tuple::MAX_COLUMNS {
            return Err(DdlSqlError::InvalidStatement);
        }
        columns.push(parser.identifier()?);
    }
    parser.take(Token::RightParen)?;
    finish_statement(&mut parser)?;
    let mut seen = std::collections::HashSet::new();
    if columns.iter().any(|column| !seen.insert(column)) {
        return Err(DdlSqlError::InvalidStatement);
    }
    Ok(CreateIndexSql {
        schema,
        table,
        name,
        columns,
        unique,
    })
}

/// Parse the bounded SQL form `CREATE SCHEMA name AUTHORIZATION 'uuid'`.
pub fn parse_create_schema(sql: &str) -> Result<CreateSchemaSql, DdlSqlError> {
    if sql.is_empty() || sql.len() > MAX_DDL_BYTES || sql.as_bytes().contains(&0) {
        return Err(DdlSqlError::InvalidStatement);
    }
    let mut parser = Parser {
        tokens: tokenize(sql)?,
        position: 0,
    };
    parser.word("create")?;
    parser.word("schema")?;
    let schema = parser.identifier()?;
    parser.word("authorization")?;
    let owner = match parser.peek() {
        Some(Token::String(value)) => value.clone(),
        _ => return Err(DdlSqlError::InvalidStatement),
    };
    parser.position += 1;
    let owner_id = parse_uuid(&owner)?;
    if owner_id == [0; 16] {
        return Err(DdlSqlError::InvalidStatement);
    }
    if parser.peek() == Some(&Token::Semicolon) {
        parser.position += 1;
    }
    if parser.peek().is_some() {
        return Err(DdlSqlError::InvalidStatement);
    }
    Ok(CreateSchemaSql { schema, owner_id })
}

pub fn parse_rename_schema(sql: &str) -> Result<RenameSchemaSql, DdlSqlError> {
    let mut parser = statement_parser(sql)?;
    parser.word("alter")?;
    parser.word("schema")?;
    let schema = parser.identifier()?;
    parser.word("rename")?;
    parser.word("to")?;
    let new_name = parser.identifier()?;
    finish_statement(&mut parser)?;
    Ok(RenameSchemaSql { schema, new_name })
}

pub fn parse_drop_schema(sql: &str) -> Result<String, DdlSqlError> {
    let mut parser = statement_parser(sql)?;
    parser.word("drop")?;
    parser.word("schema")?;
    let schema = parser.identifier()?;
    parser.word("restrict")?;
    finish_statement(&mut parser)?;
    Ok(schema)
}

pub fn parse_rename_table(sql: &str) -> Result<RenameTableSql, DdlSqlError> {
    let mut parser = statement_parser(sql)?;
    parser.word("alter")?;
    parser.word("table")?;
    let schema = parser.identifier()?;
    parser.take(Token::Dot)?;
    let table = parser.identifier()?;
    parser.word("rename")?;
    parser.word("to")?;
    let new_name = parser.identifier()?;
    finish_statement(&mut parser)?;
    Ok(RenameTableSql {
        schema,
        table,
        new_name,
    })
}

pub fn parse_drop_table(sql: &str) -> Result<(String, String), DdlSqlError> {
    let mut parser = statement_parser(sql)?;
    parser.word("drop")?;
    parser.word("table")?;
    let schema = parser.identifier()?;
    parser.take(Token::Dot)?;
    let table = parser.identifier()?;
    parser.word("restrict")?;
    finish_statement(&mut parser)?;
    Ok((schema, table))
}

/// Parse `ALTER TABLE schema.table ADD {PRIMARY KEY|UNIQUE} (columns...)`.
pub fn parse_add_key_constraint(sql: &str) -> Result<AddKeyConstraintSql, DdlSqlError> {
    let mut parser = statement_parser(sql)?;
    parser.word("alter")?;
    parser.word("table")?;
    let schema = parser.identifier()?;
    parser.take(Token::Dot)?;
    let table = parser.identifier()?;
    parser.word("add")?;
    let kind = if parser.peek_word("primary") {
        parser.word("primary")?;
        parser.word("key")?;
        KeyConstraintKind::PrimaryKey
    } else {
        parser.word("unique")?;
        KeyConstraintKind::Unique
    };
    parser.take(Token::LeftParen)?;
    let mut columns = vec![parser.identifier()?];
    while parser.peek() == Some(&Token::Comma) {
        parser.position += 1;
        columns.push(parser.identifier()?);
    }
    parser.take(Token::RightParen)?;
    finish_statement(&mut parser)?;
    if columns.len() > crate::tuple::MAX_COLUMNS
        || columns
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len()
            != columns.len()
    {
        return Err(DdlSqlError::InvalidStatement);
    }
    Ok(AddKeyConstraintSql {
        schema,
        table,
        columns,
        kind,
    })
}

/// Parse qualified `ALTER TABLE ... ADD FOREIGN KEY (...) REFERENCES ...`.
pub fn parse_add_foreign_key(sql: &str) -> Result<AddForeignKeySql, DdlSqlError> {
    let mut parser = statement_parser(sql)?;
    parser.word("alter")?;
    parser.word("table")?;
    let schema = parser.identifier()?;
    parser.take(Token::Dot)?;
    let table = parser.identifier()?;
    parser.word("add")?;
    parser.word("foreign")?;
    parser.word("key")?;
    let columns = parse_column_name_list(&mut parser)?;
    parser.word("references")?;
    let referenced_schema = parser.identifier()?;
    parser.take(Token::Dot)?;
    let referenced_table = parser.identifier()?;
    let referenced_columns = parse_column_name_list(&mut parser)?;
    finish_statement(&mut parser)?;
    if columns.len() > crate::tuple::MAX_COLUMNS
        || columns.len() != referenced_columns.len()
        || has_duplicates(&columns)
        || has_duplicates(&referenced_columns)
    {
        return Err(DdlSqlError::InvalidStatement);
    }
    Ok(AddForeignKeySql {
        schema,
        table,
        columns,
        referenced_schema,
        referenced_table,
        referenced_columns,
    })
}

/// Parse qualified `ALTER TABLE ... DROP CONSTRAINT 'generated-id'`.
/// The current catalog has generated constraint IDs and no user constraint
/// names, so SQL identifies the object using the ID returned by its creation.
pub fn parse_drop_constraint(sql: &str) -> Result<DropConstraintSql, DdlSqlError> {
    let mut parser = statement_parser(sql)?;
    parser.word("alter")?;
    parser.word("table")?;
    let schema = parser.identifier()?;
    parser.take(Token::Dot)?;
    let table = parser.identifier()?;
    parser.word("drop")?;
    parser.word("constraint")?;
    let constraint_id = match parser.peek() {
        Some(Token::String(value)) => {
            let parsed = parse_uuid(value)?;
            parser.position += 1;
            parsed
        }
        _ => return Err(DdlSqlError::InvalidStatement),
    };
    finish_statement(&mut parser)?;
    if constraint_id == [0; 16] {
        return Err(DdlSqlError::InvalidStatement);
    }
    Ok(DropConstraintSql {
        schema,
        table,
        constraint_id,
    })
}

fn parse_column_name_list(parser: &mut Parser) -> Result<Vec<String>, DdlSqlError> {
    parser.take(Token::LeftParen)?;
    let mut names = vec![parser.identifier()?];
    while parser.peek() == Some(&Token::Comma) {
        parser.position += 1;
        names.push(parser.identifier()?);
    }
    parser.take(Token::RightParen)?;
    Ok(names)
}

fn has_duplicates(names: &[String]) -> bool {
    names.iter().collect::<std::collections::HashSet<_>>().len() != names.len()
}

/// Parse `ALTER TABLE schema.table ADD CHECK (expression)` using a bounded
/// Boolean grammar over columns, typed comparisons, literals, and NULL tests.
pub fn parse_add_check_constraint(sql: &str) -> Result<AddCheckConstraintSql, DdlSqlError> {
    let mut parser = statement_parser(sql)?;
    parser.word("alter")?;
    parser.word("table")?;
    let schema = parser.identifier()?;
    parser.take(Token::Dot)?;
    let table = parser.identifier()?;
    parser.word("add")?;
    let expression = parser.check_constraint_expression()?;
    finish_statement(&mut parser)?;
    Ok(AddCheckConstraintSql {
        schema,
        table,
        expression,
    })
}

fn validate_check_columns(
    expression: &CheckExpressionSql,
    columns: &[AdminColumnDefinition],
) -> Result<(), DdlSqlError> {
    bind_check_expression(expression, |name| {
        columns
            .iter()
            .position(|column| column.name == name)
            .map(|ordinal| (ordinal, columns[ordinal].scalar_type))
    })?;
    Ok(())
}

/// Bind SQL CHECK names and context-typed literals to a row-local EIR1 tree.
pub fn bind_check_expression(
    expression: &CheckExpressionSql,
    lookup: impl Fn(&str) -> Option<(usize, ScalarType)>,
) -> Result<crate::authorization::Expr, DdlSqlError> {
    fn scalar_compatible(left: ScalarType, right: ScalarType) -> bool {
        match (left, right) {
            (ScalarType::Decimal { scale: a, .. }, ScalarType::Decimal { scale: b, .. }) => a == b,
            (ScalarType::Varchar { .. }, ScalarType::Varchar { .. }) => true,
            _ => left == right,
        }
    }

    fn comparison_hint(
        expression: &CheckExpressionSql,
        lookup: &impl Fn(&str) -> Option<(usize, ScalarType)>,
    ) -> Option<ScalarType> {
        match expression {
            CheckExpressionSql::Column(name) => lookup(name).map(|(_, scalar_type)| scalar_type),
            CheckExpressionSql::Literal(CheckLiteralSql::Boolean(_)) => Some(ScalarType::Boolean),
            CheckExpressionSql::Literal(CheckLiteralSql::String(_)) => Some(ScalarType::Text),
            CheckExpressionSql::Literal(CheckLiteralSql::Bytea(_)) => Some(ScalarType::Bytea),
            CheckExpressionSql::Literal(CheckLiteralSql::Number(number)) => {
                if number.contains('.') {
                    let scale = number.split_once('.')?.1.len();
                    (scale <= 38).then_some(ScalarType::Decimal {
                        precision: 38,
                        scale: scale as u8,
                    })
                } else {
                    Some(ScalarType::BigInt)
                }
            }
            CheckExpressionSql::IsNull(_)
            | CheckExpressionSql::IsNotNull(_)
            | CheckExpressionSql::Compare { .. }
            | CheckExpressionSql::Not(_)
            | CheckExpressionSql::And(_)
            | CheckExpressionSql::Or(_) => Some(ScalarType::Boolean),
            CheckExpressionSql::Literal(CheckLiteralSql::Null) => None,
        }
    }

    fn common_numeric_literal_type(
        left: &CheckExpressionSql,
        right: &CheckExpressionSql,
    ) -> Option<ScalarType> {
        let (
            CheckExpressionSql::Literal(CheckLiteralSql::Number(left)),
            CheckExpressionSql::Literal(CheckLiteralSql::Number(right)),
        ) = (left, right)
        else {
            return None;
        };
        let left_scale = left.split_once('.').map_or(0, |(_, scale)| scale.len());
        let right_scale = right.split_once('.').map_or(0, |(_, scale)| scale.len());
        let scale = left_scale.max(right_scale);
        if scale == 0 {
            Some(ScalarType::BigInt)
        } else if scale <= 38 {
            Some(ScalarType::Decimal {
                precision: 38,
                scale: scale as u8,
            })
        } else {
            None
        }
    }

    fn number_value(literal: &str, scalar_type: ScalarType) -> Result<ScalarValue, DdlSqlError> {
        match scalar_type {
            ScalarType::SmallInt if !literal.contains('.') => {
                Ok(ScalarValue::SmallInt(literal.parse().map_err(invalid)?))
            }
            ScalarType::Integer if !literal.contains('.') => {
                Ok(ScalarValue::Integer(literal.parse().map_err(invalid)?))
            }
            ScalarType::BigInt if !literal.contains('.') => {
                Ok(ScalarValue::BigInt(literal.parse().map_err(invalid)?))
            }
            ScalarType::Decimal { precision, scale } => Ok(ScalarValue::Decimal(
                parse_decimal_coefficient(literal, precision, scale)?,
            )),
            ScalarType::Date if !literal.contains('.') => {
                Ok(ScalarValue::Date(literal.parse().map_err(invalid)?))
            }
            ScalarType::TimestampTz if !literal.contains('.') => {
                Ok(ScalarValue::TimestampTz(literal.parse().map_err(invalid)?))
            }
            _ => Err(DdlSqlError::InvalidStatement),
        }
    }

    fn bind(
        expression: &CheckExpressionSql,
        expected: Option<ScalarType>,
        lookup: &impl Fn(&str) -> Option<(usize, ScalarType)>,
    ) -> Result<(crate::authorization::Expr, Option<ScalarType>), DdlSqlError> {
        use crate::authorization::Expr;
        use CheckExpressionSql as Sql;
        let bound = match expression {
            Sql::Column(name) => {
                let (ordinal, scalar_type) = lookup(name).ok_or(DdlSqlError::InvalidStatement)?;
                (Expr::Field(ordinal), Some(scalar_type))
            }
            Sql::Literal(literal) => {
                let (value, scalar_type) = match literal {
                    CheckLiteralSql::Null => (None, None),
                    CheckLiteralSql::Boolean(value) => (
                        Some(ScalarValue::Boolean(*value)),
                        Some(ScalarType::Boolean),
                    ),
                    CheckLiteralSql::Number(number) => {
                        let scalar_type = expected
                            .or_else(|| comparison_hint(expression, lookup))
                            .unwrap_or_else(|| {
                                if number.contains('.') {
                                    let scale = number
                                        .split_once('.')
                                        .map_or(0, |(_, fraction)| fraction.len());
                                    ScalarType::Decimal {
                                        precision: 38,
                                        scale: scale as u8,
                                    }
                                } else {
                                    ScalarType::BigInt
                                }
                            });
                        (Some(number_value(number, scalar_type)?), Some(scalar_type))
                    }
                    CheckLiteralSql::String(value) => {
                        let scalar_type = expected.unwrap_or(ScalarType::Text);
                        let value = match scalar_type {
                            ScalarType::Text => ScalarValue::Text(value.clone()),
                            ScalarType::Varchar { max_chars }
                                if value.chars().count() <= max_chars as usize =>
                            {
                                ScalarValue::Varchar(value.clone())
                            }
                            ScalarType::Uuid => ScalarValue::Uuid(parse_uuid(value)?),
                            _ => return Err(DdlSqlError::InvalidStatement),
                        };
                        (Some(value), Some(scalar_type))
                    }
                    CheckLiteralSql::Bytea(value) => {
                        if expected.is_some_and(|scalar_type| scalar_type != ScalarType::Bytea) {
                            return Err(DdlSqlError::InvalidStatement);
                        }
                        (
                            Some(ScalarValue::Bytea(decode_hex(value)?)),
                            Some(ScalarType::Bytea),
                        )
                    }
                };
                (Expr::Literal(value), scalar_type)
            }
            Sql::IsNull(name) | Sql::IsNotNull(name) => {
                let (ordinal, _) = lookup(name).ok_or(DdlSqlError::InvalidStatement)?;
                let is_null = Expr::IsNull(Box::new(Expr::Field(ordinal)));
                let expression = if matches!(expression, Sql::IsNotNull(_)) {
                    Expr::Not(Box::new(is_null))
                } else {
                    is_null
                };
                (expression, Some(ScalarType::Boolean))
            }
            Sql::Compare {
                operator,
                left,
                right,
            } => {
                let (left_hint, right_hint) =
                    if let Some(scalar_type) = common_numeric_literal_type(left, right) {
                        (Some(scalar_type), Some(scalar_type))
                    } else {
                        (
                            comparison_hint(left, lookup),
                            comparison_hint(right, lookup),
                        )
                    };
                let (left, left_type) = bind(left, right_hint, lookup)?;
                let (right, right_type) =
                    bind(right, left_hint.or(right_hint).or(left_type), lookup)?;
                if let (Some(left_type), Some(right_type)) = (left_type, right_type)
                    && !scalar_compatible(left_type, right_type)
                {
                    return Err(DdlSqlError::InvalidStatement);
                }
                let expression = match operator {
                    CheckComparisonOperator::Equal => Expr::Equal(Box::new(left), Box::new(right)),
                    CheckComparisonOperator::NotEqual => {
                        Expr::NotEqual(Box::new(left), Box::new(right))
                    }
                    CheckComparisonOperator::Less => Expr::Less(Box::new(left), Box::new(right)),
                    CheckComparisonOperator::LessEqual => {
                        Expr::LessEqual(Box::new(left), Box::new(right))
                    }
                    CheckComparisonOperator::Greater => {
                        Expr::Greater(Box::new(left), Box::new(right))
                    }
                    CheckComparisonOperator::GreaterEqual => {
                        Expr::GreaterEqual(Box::new(left), Box::new(right))
                    }
                };
                (expression, Some(ScalarType::Boolean))
            }
            Sql::Not(inner) => {
                let (inner, scalar_type) = bind(inner, Some(ScalarType::Boolean), lookup)?;
                if scalar_type.is_some_and(|scalar_type| scalar_type != ScalarType::Boolean) {
                    return Err(DdlSqlError::InvalidStatement);
                }
                (Expr::Not(Box::new(inner)), Some(ScalarType::Boolean))
            }
            Sql::And(items) | Sql::Or(items) => {
                let mut expressions = Vec::with_capacity(items.len());
                for item in items {
                    let (item, scalar_type) = bind(item, Some(ScalarType::Boolean), lookup)?;
                    if scalar_type.is_some_and(|scalar_type| scalar_type != ScalarType::Boolean) {
                        return Err(DdlSqlError::InvalidStatement);
                    }
                    expressions.push(item);
                }
                let expression = if matches!(expression, Sql::And(_)) {
                    Expr::And(expressions)
                } else {
                    Expr::Or(expressions)
                };
                (expression, Some(ScalarType::Boolean))
            }
        };
        Ok(bound)
    }

    let (expression, scalar_type) = bind(expression, Some(ScalarType::Boolean), &lookup)?;
    if scalar_type.is_some_and(|scalar_type| scalar_type != ScalarType::Boolean) {
        return Err(DdlSqlError::InvalidStatement);
    }
    Ok(expression)
}

/// Parse `ALTER TABLE schema.table ALTER COLUMN name {SET|DROP} NOT NULL`.
pub fn parse_alter_column_nullability(sql: &str) -> Result<AlterColumnNullabilitySql, DdlSqlError> {
    let mut parser = statement_parser(sql)?;
    parser.word("alter")?;
    parser.word("table")?;
    let schema = parser.identifier()?;
    parser.take(Token::Dot)?;
    let table = parser.identifier()?;
    parser.word("alter")?;
    parser.word("column")?;
    let column = parser.identifier()?;
    let nullable = if parser.peek_word("set") {
        parser.word("set")?;
        parser.word("not")?;
        parser.word("null")?;
        false
    } else {
        parser.word("drop")?;
        parser.word("not")?;
        parser.word("null")?;
        true
    };
    finish_statement(&mut parser)?;
    Ok(AlterColumnNullabilitySql {
        schema,
        table,
        column,
        nullable,
    })
}

/// Parse `ALTER TABLE ... ALTER COLUMN ... SET|DROP DEFAULT` with a
/// context-independent literal. The authenticated client binds it to the
/// declared column type after metadata resolution.
pub fn parse_alter_column_default(sql: &str) -> Result<AlterColumnDefaultSql, DdlSqlError> {
    let mut parser = statement_parser(sql)?;
    parser.word("alter")?;
    parser.word("table")?;
    let schema = parser.identifier()?;
    parser.take(Token::Dot)?;
    let table = parser.identifier()?;
    parser.word("alter")?;
    parser.word("column")?;
    let column = parser.identifier()?;
    let default = if parser.peek_word("set") {
        parser.word("set")?;
        parser.word("default")?;
        Some(parser.default_literal()?)
    } else {
        parser.word("drop")?;
        parser.word("default")?;
        None
    };
    finish_statement(&mut parser)?;
    Ok(AlterColumnDefaultSql {
        schema,
        table,
        column,
        default,
    })
}

pub fn bind_default_literal(
    literal: &DefaultLiteralSql,
    scalar_type: ScalarType,
) -> Result<AdminColumnDefault, DdlSqlError> {
    let value = match literal {
        DefaultLiteralSql::Null => return Ok(AdminColumnDefault::Null),
        DefaultLiteralSql::CurrentPrincipalId if scalar_type == ScalarType::Uuid => {
            return Ok(AdminColumnDefault::CurrentPrincipalId);
        }
        DefaultLiteralSql::Boolean(value) if scalar_type == ScalarType::Boolean => {
            ScalarValue::Boolean(*value)
        }
        DefaultLiteralSql::Number(value) => match scalar_type {
            ScalarType::SmallInt => ScalarValue::SmallInt(value.parse().map_err(invalid)?),
            ScalarType::Integer => ScalarValue::Integer(value.parse().map_err(invalid)?),
            ScalarType::BigInt => ScalarValue::BigInt(value.parse().map_err(invalid)?),
            ScalarType::Decimal { precision, scale } => {
                ScalarValue::Decimal(parse_decimal_coefficient(value, precision, scale)?)
            }
            ScalarType::Date => ScalarValue::Date(value.parse().map_err(invalid)?),
            ScalarType::TimestampTz => ScalarValue::TimestampTz(value.parse().map_err(invalid)?),
            _ => return Err(DdlSqlError::InvalidStatement),
        },
        DefaultLiteralSql::String(value) => match scalar_type {
            ScalarType::Text => ScalarValue::Text(value.clone()),
            ScalarType::Varchar { max_chars } if value.chars().count() <= max_chars as usize => {
                ScalarValue::Varchar(value.clone())
            }
            ScalarType::Uuid => ScalarValue::Uuid(parse_uuid(value)?),
            _ => return Err(DdlSqlError::InvalidStatement),
        },
        DefaultLiteralSql::Bytea(value) if scalar_type == ScalarType::Bytea => {
            ScalarValue::Bytea(value.clone())
        }
        _ => return Err(DdlSqlError::InvalidStatement),
    };
    Ok(AdminColumnDefault::Literal(value))
}

/// Parse one typed `ALTER TABLE schema.table ADD COLUMN` definition.
pub fn parse_add_column(sql: &str) -> Result<AddColumnSql, DdlSqlError> {
    let mut parser = statement_parser(sql)?;
    parser.word("alter")?;
    parser.word("table")?;
    let schema = parser.identifier()?;
    parser.take(Token::Dot)?;
    let table = parser.identifier()?;
    parser.word("add")?;
    parser.word("column")?;
    let name = parser.identifier()?;
    let scalar_type = parser.scalar_type()?;
    let mut nullable = true;
    let mut default = None;
    loop {
        if parser.peek_word("not") && nullable {
            parser.word("not")?;
            parser.word("null")?;
            nullable = false;
        } else if parser.peek_word("default") && default.is_none() {
            parser.word("default")?;
            default = Some(parser.default_value(scalar_type)?);
        } else {
            break;
        }
    }
    if matches!(default, Some(AdminColumnDefault::CurrentPrincipalId))
        || matches!(default, Some(AdminColumnDefault::Null)) && !nullable
        || !nullable && default.is_none()
    {
        return Err(DdlSqlError::InvalidStatement);
    }
    finish_statement(&mut parser)?;
    Ok(AddColumnSql {
        schema,
        table,
        column: AdminColumnDefinition {
            name,
            scalar_type,
            nullable,
            generated_uuid_identity: false,
            default,
        },
    })
}

/// Parse `ALTER TABLE schema.table RENAME COLUMN name TO new_name`.
pub fn parse_rename_column(sql: &str) -> Result<RenameColumnSql, DdlSqlError> {
    let mut parser = statement_parser(sql)?;
    parser.word("alter")?;
    parser.word("table")?;
    let schema = parser.identifier()?;
    parser.take(Token::Dot)?;
    let table = parser.identifier()?;
    parser.word("rename")?;
    parser.word("column")?;
    let column = parser.identifier()?;
    parser.word("to")?;
    let new_name = parser.identifier()?;
    finish_statement(&mut parser)?;
    Ok(RenameColumnSql {
        schema,
        table,
        column,
        new_name,
    })
}

/// Parse `ALTER TABLE schema.table DROP COLUMN name RESTRICT`.
pub fn parse_drop_column(sql: &str) -> Result<DropColumnSql, DdlSqlError> {
    let mut parser = statement_parser(sql)?;
    parser.word("alter")?;
    parser.word("table")?;
    let schema = parser.identifier()?;
    parser.take(Token::Dot)?;
    let table = parser.identifier()?;
    parser.word("drop")?;
    parser.word("column")?;
    let column = parser.identifier()?;
    parser.word("restrict")?;
    finish_statement(&mut parser)?;
    Ok(DropColumnSql {
        schema,
        table,
        column,
    })
}

fn statement_parser(sql: &str) -> Result<Parser, DdlSqlError> {
    if sql.is_empty() || sql.len() > MAX_DDL_BYTES || sql.as_bytes().contains(&0) {
        return Err(DdlSqlError::InvalidStatement);
    }
    Ok(Parser {
        tokens: tokenize(sql)?,
        position: 0,
    })
}

fn finish_statement(parser: &mut Parser) -> Result<(), DdlSqlError> {
    if parser.peek() == Some(&Token::Semicolon) {
        parser.position += 1;
    }
    if parser.peek().is_some() {
        return Err(DdlSqlError::InvalidStatement);
    }
    Ok(())
}

struct Parser {
    tokens: Vec<Token>,
    position: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.position)
    }

    fn take(&mut self, expected: Token) -> Result<(), DdlSqlError> {
        if self.peek() == Some(&expected) {
            self.position += 1;
            Ok(())
        } else {
            Err(DdlSqlError::InvalidStatement)
        }
    }

    fn word(&mut self, expected: &str) -> Result<(), DdlSqlError> {
        match self.peek() {
            Some(Token::Word(word)) if word == expected => {
                self.position += 1;
                Ok(())
            }
            _ => Err(DdlSqlError::InvalidStatement),
        }
    }

    fn peek_word(&self, expected: &str) -> bool {
        matches!(self.peek(), Some(Token::Word(word)) if word == expected)
    }

    fn check_or(&mut self) -> Result<CheckExpressionSql, DdlSqlError> {
        let mut expressions = vec![self.check_and()?];
        while self.peek_word("or") {
            self.position += 1;
            expressions.push(self.check_and()?);
        }
        Ok(if expressions.len() == 1 {
            expressions.pop().unwrap()
        } else {
            CheckExpressionSql::Or(expressions)
        })
    }

    fn check_and(&mut self) -> Result<CheckExpressionSql, DdlSqlError> {
        let mut expressions = vec![self.check_unary()?];
        while self.peek_word("and") {
            self.position += 1;
            expressions.push(self.check_unary()?);
        }
        Ok(if expressions.len() == 1 {
            expressions.pop().unwrap()
        } else {
            CheckExpressionSql::And(expressions)
        })
    }

    fn check_unary(&mut self) -> Result<CheckExpressionSql, DdlSqlError> {
        if self.peek_word("not") {
            self.position += 1;
            return Ok(CheckExpressionSql::Not(Box::new(self.check_unary()?)));
        }
        if self.peek() == Some(&Token::LeftParen) {
            self.position += 1;
            let expression = self.check_or()?;
            self.take(Token::RightParen)?;
            return Ok(expression);
        }
        self.check_predicate()
    }

    fn check_predicate(&mut self) -> Result<CheckExpressionSql, DdlSqlError> {
        let left = self.check_value_atom()?;
        if self.peek_word("is") {
            let CheckExpressionSql::Column(column) = left else {
                return Err(DdlSqlError::InvalidStatement);
            };
            self.position += 1;
            let is_not = if self.peek_word("not") {
                self.position += 1;
                true
            } else {
                false
            };
            self.word("null")?;
            Ok(if is_not {
                CheckExpressionSql::IsNotNull(column)
            } else {
                CheckExpressionSql::IsNull(column)
            })
        } else {
            let operator = match self.peek() {
                Some(Token::Equal) => Some(CheckComparisonOperator::Equal),
                Some(Token::NotEqual) => Some(CheckComparisonOperator::NotEqual),
                Some(Token::Less) => Some(CheckComparisonOperator::Less),
                Some(Token::LessEqual) => Some(CheckComparisonOperator::LessEqual),
                Some(Token::Greater) => Some(CheckComparisonOperator::Greater),
                Some(Token::GreaterEqual) => Some(CheckComparisonOperator::GreaterEqual),
                _ => None,
            };
            if let Some(operator) = operator {
                self.position += 1;
                let right = self.check_value_atom()?;
                Ok(CheckExpressionSql::Compare {
                    operator,
                    left: Box::new(left),
                    right: Box::new(right),
                })
            } else {
                Ok(left)
            }
        }
    }

    fn check_value_atom(&mut self) -> Result<CheckExpressionSql, DdlSqlError> {
        if self.peek_word("null") {
            self.position += 1;
            return Ok(CheckExpressionSql::Literal(CheckLiteralSql::Null));
        }
        if self.peek_word("true") || self.peek_word("false") {
            let value = self.peek_word("true");
            self.position += 1;
            return Ok(CheckExpressionSql::Literal(CheckLiteralSql::Boolean(value)));
        }
        if self.peek_word("x")
            && matches!(self.tokens.get(self.position + 1), Some(Token::String(_)))
        {
            self.position += 1;
            let Some(Token::String(value)) = self.peek() else {
                return Err(DdlSqlError::InvalidStatement);
            };
            let value = value.clone();
            decode_hex(&value)?;
            self.position += 1;
            return Ok(CheckExpressionSql::Literal(CheckLiteralSql::Bytea(value)));
        }
        if matches!(
            self.peek(),
            Some(Token::Number(_) | Token::Minus | Token::Plus)
        ) {
            let value = self.signed_number()?;
            if value
                .split_once('.')
                .is_some_and(|(_, fraction)| fraction.len() > 38)
            {
                return Err(DdlSqlError::InvalidStatement);
            }
            if self
                .peek()
                .is_some_and(|token| matches!(token, Token::Minus | Token::Plus))
            {
                self.position += 2;
            } else {
                self.position += 1;
            }
            return Ok(CheckExpressionSql::Literal(CheckLiteralSql::Number(value)));
        }
        if let Some(Token::String(value)) = self.peek() {
            let value = value.clone();
            self.position += 1;
            return Ok(CheckExpressionSql::Literal(CheckLiteralSql::String(value)));
        }
        Ok(CheckExpressionSql::Column(self.identifier()?))
    }

    fn check_constraint_expression(&mut self) -> Result<CheckExpressionSql, DdlSqlError> {
        self.word("check")?;
        self.take(Token::LeftParen)?;
        let expression = self.check_or()?;
        self.take(Token::RightParen)?;
        Ok(expression)
    }

    fn identifier(&mut self) -> Result<String, DdlSqlError> {
        match self.peek() {
            Some(Token::Word(word))
                if !word.is_empty()
                    && word.len() <= 128
                    && (word.as_bytes()[0].is_ascii_lowercase() || word.as_bytes()[0] == b'_')
                    && word.bytes().all(|byte| {
                        byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_'
                    }) =>
            {
                let word = word.clone();
                self.position += 1;
                Ok(word)
            }
            _ => Err(DdlSqlError::InvalidStatement),
        }
    }

    fn scalar_type(&mut self) -> Result<ScalarType, DdlSqlError> {
        let name = match self.peek() {
            Some(Token::Word(word)) => word.clone(),
            _ => return Err(DdlSqlError::InvalidStatement),
        };
        self.position += 1;
        let scalar_type = match name.as_str() {
            "boolean" => ScalarType::Boolean,
            "smallint" => ScalarType::SmallInt,
            "integer" | "int" => ScalarType::Integer,
            "bigint" => ScalarType::BigInt,
            "text" => ScalarType::Text,
            "bytea" => ScalarType::Bytea,
            "uuid" => ScalarType::Uuid,
            "date" => ScalarType::Date,
            "timestamptz" => ScalarType::TimestampTz,
            "decimal" => {
                self.take(Token::LeftParen)?;
                let precision = self.number()?;
                self.take(Token::Comma)?;
                let scale = self.number()?;
                self.take(Token::RightParen)?;
                if !(1..=38).contains(&precision) || scale > precision {
                    return Err(DdlSqlError::InvalidStatement);
                }
                ScalarType::Decimal {
                    precision: precision as u8,
                    scale: scale as u8,
                }
            }
            "varchar" => {
                self.take(Token::LeftParen)?;
                let length = self.number()?;
                self.take(Token::RightParen)?;
                if length == 0 {
                    return Err(DdlSqlError::InvalidStatement);
                }
                ScalarType::Varchar { max_chars: length }
            }
            _ => return Err(DdlSqlError::InvalidStatement),
        };
        Ok(scalar_type)
    }

    fn number(&mut self) -> Result<u32, DdlSqlError> {
        match self.peek() {
            Some(Token::Number(value)) => {
                let parsed = value.parse().map_err(|_| DdlSqlError::InvalidStatement)?;
                self.position += 1;
                Ok(parsed)
            }
            _ => Err(DdlSqlError::InvalidStatement),
        }
    }

    fn default_value(
        &mut self,
        scalar_type: ScalarType,
    ) -> Result<AdminColumnDefault, DdlSqlError> {
        if self.peek_word("null") {
            self.position += 1;
            return Ok(AdminColumnDefault::Null);
        }
        if self.peek_word("current_principal_id") {
            if scalar_type != ScalarType::Uuid {
                return Err(DdlSqlError::InvalidStatement);
            }
            self.position += 1;
            self.take(Token::LeftParen)?;
            self.take(Token::RightParen)?;
            return Ok(AdminColumnDefault::CurrentPrincipalId);
        }
        let mut consumed_tokens = if matches!(self.peek(), Some(Token::Minus | Token::Plus)) {
            2
        } else {
            1
        };
        let value = match scalar_type {
            ScalarType::Boolean => match self.peek() {
                Some(Token::Word(word)) if word == "true" => ScalarValue::Boolean(true),
                Some(Token::Word(word)) if word == "false" => ScalarValue::Boolean(false),
                _ => return Err(DdlSqlError::InvalidStatement),
            },
            ScalarType::SmallInt => {
                ScalarValue::SmallInt(self.signed_number()?.parse().map_err(invalid)?)
            }
            ScalarType::Integer => {
                ScalarValue::Integer(self.signed_number()?.parse().map_err(invalid)?)
            }
            ScalarType::BigInt => {
                ScalarValue::BigInt(self.signed_number()?.parse().map_err(invalid)?)
            }
            ScalarType::Decimal { precision, scale } => ScalarValue::Decimal(
                parse_decimal_coefficient(&self.signed_number()?, precision, scale)?,
            ),
            ScalarType::Text => match self.peek() {
                Some(Token::String(value)) => ScalarValue::Text(value.clone()),
                _ => return Err(DdlSqlError::InvalidStatement),
            },
            ScalarType::Varchar { max_chars } => match self.peek() {
                Some(Token::String(value)) if value.chars().count() <= max_chars as usize => {
                    ScalarValue::Varchar(value.clone())
                }
                _ => return Err(DdlSqlError::InvalidStatement),
            },
            ScalarType::Bytea => {
                if !self.peek_word("x") {
                    return Err(DdlSqlError::InvalidStatement);
                }
                consumed_tokens = 2;
                let Some(Token::String(hex)) = self.tokens.get(self.position + 1) else {
                    return Err(DdlSqlError::InvalidStatement);
                };
                ScalarValue::Bytea(decode_hex(hex)?)
            }
            ScalarType::Uuid => {
                let Some(Token::String(text)) = self.peek() else {
                    return Err(DdlSqlError::InvalidStatement);
                };
                ScalarValue::Uuid(parse_uuid(text)?)
            }
            ScalarType::Date => ScalarValue::Date(self.signed_number()?.parse().map_err(invalid)?),
            ScalarType::TimestampTz => {
                ScalarValue::TimestampTz(self.signed_number()?.parse().map_err(invalid)?)
            }
        };
        self.position += consumed_tokens;
        Ok(AdminColumnDefault::Literal(value))
    }

    fn default_literal(&mut self) -> Result<DefaultLiteralSql, DdlSqlError> {
        if self.peek_word("null") {
            self.position += 1;
            return Ok(DefaultLiteralSql::Null);
        }
        if self.peek_word("current_principal_id") {
            self.position += 1;
            self.take(Token::LeftParen)?;
            self.take(Token::RightParen)?;
            return Ok(DefaultLiteralSql::CurrentPrincipalId);
        }
        if self.peek_word("true") || self.peek_word("false") {
            let value = self.peek_word("true");
            self.position += 1;
            return Ok(DefaultLiteralSql::Boolean(value));
        }
        if self.peek_word("x") {
            self.position += 1;
            let Some(Token::String(hex)) = self.peek() else {
                return Err(DdlSqlError::InvalidStatement);
            };
            let value = decode_hex(hex)?;
            self.position += 1;
            return Ok(DefaultLiteralSql::Bytea(value));
        }
        if let Some(Token::String(value)) = self.peek() {
            let value = value.clone();
            self.position += 1;
            return Ok(DefaultLiteralSql::String(value));
        }
        if matches!(
            self.peek(),
            Some(Token::Number(_) | Token::Minus | Token::Plus)
        ) {
            let value = self.signed_number()?;
            if value
                .split_once('.')
                .is_some_and(|(_, fraction)| fraction.len() > 38)
            {
                return Err(DdlSqlError::InvalidStatement);
            }
            if self
                .peek()
                .is_some_and(|token| matches!(token, Token::Minus | Token::Plus))
            {
                self.position += 2;
            } else {
                self.position += 1;
            }
            return Ok(DefaultLiteralSql::Number(value));
        }
        Err(DdlSqlError::InvalidStatement)
    }

    fn signed_number(&mut self) -> Result<String, DdlSqlError> {
        let (negative, number_position) = match self.peek() {
            Some(Token::Minus) => (true, self.position + 1),
            Some(Token::Plus) => (false, self.position + 1),
            _ => (false, self.position),
        };
        let Some(Token::Number(number)) = self.tokens.get(number_position) else {
            return Err(DdlSqlError::InvalidStatement);
        };
        if number.contains('.') && !matches!(number.as_bytes().last(), Some(b'0'..=b'9')) {
            return Err(DdlSqlError::InvalidStatement);
        }
        let mut number = number.clone();
        if negative {
            number.insert(0, '-');
        }
        Ok(number)
    }
}

fn invalid<T>(_: T) -> DdlSqlError {
    DdlSqlError::InvalidStatement
}

fn parse_decimal_coefficient(literal: &str, precision: u8, scale: u8) -> Result<i128, DdlSqlError> {
    let negative = literal.starts_with('-');
    let unsigned = literal.strip_prefix('-').unwrap_or(literal);
    let (whole, fraction) = unsigned.split_once('.').unwrap_or((unsigned, ""));
    if whole.is_empty()
        || !whole.bytes().all(|b| b.is_ascii_digit())
        || !fraction.bytes().all(|b| b.is_ascii_digit())
        || fraction.len() > scale as usize
    {
        return Err(DdlSqlError::InvalidStatement);
    }
    let mut digits = whole.to_owned();
    digits.push_str(fraction);
    digits.extend(std::iter::repeat_n('0', scale as usize - fraction.len()));
    let significant = digits.trim_start_matches('0');
    if significant.len() > precision as usize {
        return Err(DdlSqlError::InvalidStatement);
    }
    let magnitude: i128 = if significant.is_empty() {
        0
    } else {
        significant.parse().map_err(invalid)?
    };
    Ok(if negative { -magnitude } else { magnitude })
}

fn decode_hex(text: &str) -> Result<Vec<u8>, DdlSqlError> {
    if !text.len().is_multiple_of(2) {
        return Err(DdlSqlError::InvalidStatement);
    }
    text.as_bytes()
        .chunks_exact(2)
        .map(|pair| Ok((hex_digit(pair[0])? << 4) | hex_digit(pair[1])?))
        .collect()
}

fn parse_uuid(text: &str) -> Result<[u8; 16], DdlSqlError> {
    if text.len() == 36 {
        for position in [8, 13, 18, 23] {
            if text.as_bytes().get(position) != Some(&b'-') {
                return Err(DdlSqlError::InvalidStatement);
            }
        }
    } else if text.len() != 32 {
        return Err(DdlSqlError::InvalidStatement);
    }
    let compact: String = text.chars().filter(|&ch| ch != '-').collect();
    let bytes = decode_hex(&compact)?;
    bytes.try_into().map_err(invalid)
}

fn hex_digit(byte: u8) -> Result<u8, DdlSqlError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(DdlSqlError::InvalidStatement),
    }
}

fn tokenize(sql: &str) -> Result<Vec<Token>, DdlSqlError> {
    let bytes = sql.as_bytes();
    let mut tokens = Vec::new();
    let mut cursor = 0;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b' ' | b'\t' | b'\r' | b'\n' => cursor += 1,
            b'.' => {
                tokens.push(Token::Dot);
                cursor += 1;
            }
            b',' => {
                tokens.push(Token::Comma);
                cursor += 1;
            }
            b'(' => {
                tokens.push(Token::LeftParen);
                cursor += 1;
            }
            b')' => {
                tokens.push(Token::RightParen);
                cursor += 1;
            }
            b';' => {
                tokens.push(Token::Semicolon);
                cursor += 1;
            }
            b'=' => {
                tokens.push(Token::Equal);
                cursor += 1;
            }
            b'!' if bytes.get(cursor + 1) == Some(&b'=') => {
                tokens.push(Token::NotEqual);
                cursor += 2;
            }
            b'<' if bytes.get(cursor + 1) == Some(&b'=') => {
                tokens.push(Token::LessEqual);
                cursor += 2;
            }
            b'<' if bytes.get(cursor + 1) == Some(&b'>') => {
                tokens.push(Token::NotEqual);
                cursor += 2;
            }
            b'<' => {
                tokens.push(Token::Less);
                cursor += 1;
            }
            b'>' if bytes.get(cursor + 1) == Some(&b'=') => {
                tokens.push(Token::GreaterEqual);
                cursor += 2;
            }
            b'>' => {
                tokens.push(Token::Greater);
                cursor += 1;
            }
            b'-' => {
                tokens.push(Token::Minus);
                cursor += 1;
            }
            b'+' => {
                tokens.push(Token::Plus);
                cursor += 1;
            }
            b'\'' => {
                cursor += 1;
                let mut value = Vec::new();
                let mut closed = false;
                while cursor < bytes.len() {
                    if bytes[cursor] == b'\'' {
                        if bytes.get(cursor + 1) == Some(&b'\'') {
                            value.push(b'\'');
                            cursor += 2;
                        } else {
                            cursor += 1;
                            closed = true;
                            break;
                        }
                    } else {
                        value.push(bytes[cursor]);
                        cursor += 1;
                    }
                }
                if !closed {
                    return Err(DdlSqlError::InvalidStatement);
                }
                tokens.push(Token::String(String::from_utf8(value).map_err(invalid)?));
            }
            byte if byte.is_ascii_alphabetic() || byte == b'_' => {
                let start = cursor;
                cursor += 1;
                while cursor < bytes.len()
                    && (bytes[cursor].is_ascii_alphanumeric() || bytes[cursor] == b'_')
                {
                    cursor += 1;
                }
                let word = std::str::from_utf8(&bytes[start..cursor])
                    .map_err(|_| DdlSqlError::InvalidStatement)?;
                tokens.push(Token::Word(word.to_ascii_lowercase()));
            }
            byte if byte.is_ascii_digit() => {
                let start = cursor;
                cursor += 1;
                while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
                    cursor += 1;
                }
                if bytes.get(cursor) == Some(&b'.')
                    && bytes.get(cursor + 1).is_some_and(u8::is_ascii_digit)
                {
                    cursor += 1;
                    while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
                        cursor += 1;
                    }
                }
                let number = std::str::from_utf8(&bytes[start..cursor])
                    .map_err(|_| DdlSqlError::InvalidStatement)?;
                tokens.push(Token::Number(number.to_owned()));
            }
            _ => return Err(DdlSqlError::InvalidStatement),
        }
    }
    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_qualified_create_table_with_supported_types() {
        let parsed = parse_create_table(
            "CREATE TABLE app.events (id UUID NOT NULL, amount DECIMAL(8, 2), title VARCHAR(80));",
        )
        .unwrap();
        assert_eq!(parsed.schema, "app");
        assert_eq!(parsed.table, "events");
        assert_eq!(parsed.columns.len(), 3);
        assert!(!parsed.columns[0].nullable);
        assert_eq!(
            parsed.columns[1].scalar_type,
            ScalarType::Decimal {
                precision: 8,
                scale: 2
            }
        );
        assert_eq!(
            parsed.columns[2].scalar_type,
            ScalarType::Varchar { max_chars: 80 }
        );
    }

    #[test]
    fn parses_create_schema_with_explicit_uuid_owner() {
        let parsed = parse_create_schema(
            "CREATE SCHEMA app AUTHORIZATION '00112233-4455-6677-8899-aabbccddeeff';",
        )
        .unwrap();
        assert_eq!(parsed.schema, "app");
        assert_eq!(
            parsed.owner_id,
            [
                0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd,
                0xee, 0xff,
            ]
        );
        for sql in [
            "CREATE SCHEMA app",
            "CREATE SCHEMA app AUTHORIZATION app_owner",
            "CREATE SCHEMA app AUTHORIZATION '00000000-0000-0000-0000-000000000000'",
            "CREATE SCHEMA app AUTHORIZATION '00112233445566778899aabbccddeeff'; CREATE SCHEMA b AUTHORIZATION '00112233445566778899aabbccddeeff'",
        ] {
            assert!(parse_create_schema(sql).is_err(), "accepted {sql}");
        }
    }

    #[test]
    fn parses_schema_rename_and_restrict_drop_only() {
        assert_eq!(
            parse_rename_schema("ALTER SCHEMA app RENAME TO application;").unwrap(),
            RenameSchemaSql {
                schema: "app".into(),
                new_name: "application".into(),
            }
        );
        assert_eq!(
            parse_drop_schema("DROP SCHEMA app RESTRICT").unwrap(),
            "app"
        );
        for sql in [
            "ALTER SCHEMA app RENAME application",
            "ALTER SCHEMA app RENAME TO \"Mixed\"",
            "DROP SCHEMA app",
            "DROP SCHEMA app CASCADE",
            "DROP SCHEMA app RESTRICT; DROP SCHEMA b RESTRICT",
        ] {
            assert!(
                parse_rename_schema(sql).is_err() && parse_drop_schema(sql).is_err(),
                "accepted {sql}"
            );
        }
    }

    #[test]
    fn parses_qualified_table_rename_and_restrict_drop() {
        assert_eq!(
            parse_rename_table("ALTER TABLE app.events RENAME TO archived;").unwrap(),
            RenameTableSql {
                schema: "app".into(),
                table: "events".into(),
                new_name: "archived".into(),
            }
        );
        assert_eq!(
            parse_drop_table("DROP TABLE app.events RESTRICT").unwrap(),
            ("app".into(), "events".into())
        );
        for sql in [
            "ALTER TABLE events RENAME TO archived",
            "ALTER TABLE app.events RENAME archived",
            "DROP TABLE app.events",
            "DROP TABLE app.events CASCADE",
            "DROP TABLE app.events RESTRICT; DROP TABLE app.other RESTRICT",
        ] {
            assert!(
                parse_rename_table(sql).is_err() && parse_drop_table(sql).is_err(),
                "accepted {sql}"
            );
        }
    }

    #[test]
    fn parses_table_level_key_constraint_alterations() {
        assert_eq!(
            parse_add_key_constraint("ALTER TABLE app.events ADD UNIQUE (tenant, event_id);")
                .unwrap(),
            AddKeyConstraintSql {
                schema: "app".into(),
                table: "events".into(),
                columns: vec!["tenant".into(), "event_id".into()],
                kind: KeyConstraintKind::Unique,
            }
        );
        assert_eq!(
            parse_add_key_constraint("ALTER TABLE app.events ADD PRIMARY KEY (id)")
                .unwrap()
                .kind,
            KeyConstraintKind::PrimaryKey
        );
        for sql in [
            "ALTER TABLE events ADD UNIQUE (id)",
            "ALTER TABLE app.events ADD CONSTRAINT key UNIQUE (id)",
            "ALTER TABLE app.events ADD UNIQUE ()",
            "ALTER TABLE app.events ADD UNIQUE (id, id)",
            "ALTER TABLE app.events ADD UNIQUE (id); ALTER TABLE app.events ADD UNIQUE (id)",
        ] {
            assert!(parse_add_key_constraint(sql).is_err(), "accepted {sql}");
        }
    }

    #[test]
    fn parses_qualified_foreign_key_alteration() {
        assert_eq!(
            parse_add_foreign_key(
                "ALTER TABLE app.events ADD FOREIGN KEY (tenant_id, event_id) REFERENCES core.tenants (tenant_id, id);"
            )
            .unwrap(),
            AddForeignKeySql {
                schema: "app".into(),
                table: "events".into(),
                columns: vec!["tenant_id".into(), "event_id".into()],
                referenced_schema: "core".into(),
                referenced_table: "tenants".into(),
                referenced_columns: vec!["tenant_id".into(), "id".into()],
            }
        );
        for sql in [
            "ALTER TABLE app.events ADD FOREIGN KEY (tenant_id) REFERENCES core.tenants (tenant_id, id)",
            "ALTER TABLE app.events ADD FOREIGN KEY (tenant_id, tenant_id) REFERENCES core.tenants (tenant_id, id)",
            "ALTER TABLE app.events ADD FOREIGN KEY (tenant_id) REFERENCES tenants (tenant_id)",
            "ALTER TABLE app.events ADD FOREIGN KEY (tenant_id) REFERENCES core.tenants (tenant_id); ALTER TABLE app.events ADD FOREIGN KEY (x) REFERENCES core.tenants (x)",
        ] {
            assert!(parse_add_foreign_key(sql).is_err(), "accepted {sql}");
        }
        assert_eq!(
            parse_drop_constraint(
                "ALTER TABLE app.orders DROP CONSTRAINT '01234567-89ab-cdef-0123-456789abcdef'"
            )
            .unwrap(),
            DropConstraintSql {
                schema: "app".into(),
                table: "orders".into(),
                constraint_id: [
                    0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89,
                    0xab, 0xcd, 0xef,
                ],
            }
        );
        for sql in [
            "ALTER TABLE app.orders DROP CONSTRAINT fk_orders_customer",
            "ALTER TABLE orders DROP CONSTRAINT '0123456789abcdef0123456789abcdef'",
            "ALTER TABLE app.orders DROP CONSTRAINT '00000000000000000000000000000000'",
        ] {
            assert!(parse_drop_constraint(sql).is_err(), "accepted {sql}");
        }
    }

    #[test]
    fn parses_table_level_check_constraints_and_binds_column_names() {
        let parsed = parse_create_table(
            "CREATE TABLE app.events (id UUID NOT NULL, active BOOLEAN, deleted_at TIMESTAMPTZ, amount DECIMAL(8,2), PRIMARY KEY (id), CHECK (active OR deleted_at IS NULL), CHECK (amount >= -1.25));",
        )
        .unwrap();
        assert_eq!(
            parsed.check_constraints,
            vec![
                CheckExpressionSql::Or(vec![
                    CheckExpressionSql::Column("active".into()),
                    CheckExpressionSql::IsNull("deleted_at".into()),
                ]),
                CheckExpressionSql::Compare {
                    operator: CheckComparisonOperator::GreaterEqual,
                    left: Box::new(CheckExpressionSql::Column("amount".into())),
                    right: Box::new(CheckExpressionSql::Literal(CheckLiteralSql::Number(
                        "-1.25".into(),
                    ))),
                },
            ]
        );
        for sql in [
            "CREATE TABLE app.events (id INTEGER, CHECK (missing IS NULL))",
            "CREATE TABLE app.events (id INTEGER, CHECK (id))",
            "CREATE TABLE app.events (amount DECIMAL(8,2), CHECK (amount >= 0.001))",
            "CREATE TABLE app.events (amount TEXT, CHECK (amount >= 0))",
        ] {
            assert!(parse_create_table(sql).is_err(), "accepted {sql}");
        }
    }

    #[test]
    fn binds_check_comparison_literals_to_column_types() {
        use crate::authorization::Expr;

        let expression = CheckExpressionSql::Compare {
            operator: CheckComparisonOperator::GreaterEqual,
            left: Box::new(CheckExpressionSql::Column("amount".into())),
            right: Box::new(CheckExpressionSql::Literal(CheckLiteralSql::Number(
                "-1.25".into(),
            ))),
        };
        let bound = bind_check_expression(&expression, |name| {
            (name == "amount").then_some((
                0,
                ScalarType::Decimal {
                    precision: 8,
                    scale: 2,
                },
            ))
        })
        .unwrap();
        assert_eq!(
            bound,
            Expr::GreaterEqual(
                Box::new(Expr::Field(0)),
                Box::new(Expr::Literal(Some(ScalarValue::Decimal(-125)))),
            )
        );

        let string_check = parse_create_table(
            "CREATE TABLE app.events (label VARCHAR(8), CHECK (label = 'ready'))",
        )
        .unwrap();
        assert_eq!(string_check.check_constraints.len(), 1);
        assert!(
            parse_create_table(
                "CREATE TABLE app.events (active BOOLEAN, CHECK (active = 'ready'))"
            )
            .is_err()
        );
        assert!(
            parse_create_table("CREATE TABLE app.events (amount INTEGER, CHECK (amount >= '0'))")
                .is_err()
        );

        for (operator, expected) in [
            ("=", CheckComparisonOperator::Equal),
            ("!=", CheckComparisonOperator::NotEqual),
            ("<>", CheckComparisonOperator::NotEqual),
            ("<", CheckComparisonOperator::Less),
            ("<=", CheckComparisonOperator::LessEqual),
            (">", CheckComparisonOperator::Greater),
            (">=", CheckComparisonOperator::GreaterEqual),
        ] {
            let statement = format!("ALTER TABLE app.events ADD CHECK (amount {operator} 0)");
            assert!(matches!(
                parse_add_check_constraint(&statement).unwrap().expression,
                CheckExpressionSql::Compare { operator, .. } if operator == expected
            ));
        }
    }

    #[test]
    fn parses_bounded_check_alterations_and_rejects_other_expression_syntax() {
        assert_eq!(
            parse_add_check_constraint("ALTER TABLE app.events ADD CHECK (active AND (deleted IS NULL OR deleted IS NOT NULL));").unwrap(),
            AddCheckConstraintSql {
                schema: "app".into(),
                table: "events".into(),
                expression: CheckExpressionSql::And(vec![
                    CheckExpressionSql::Column("active".into()),
                    CheckExpressionSql::Or(vec![
                        CheckExpressionSql::IsNull("deleted".into()),
                        CheckExpressionSql::IsNotNull("deleted".into()),
                    ]),
                ]),
            }
        );
        for sql in [
            "ALTER TABLE app.events ADD CHECK (active); ALTER TABLE app.events ADD CHECK (active)",
            "ALTER TABLE events ADD CHECK (active)",
            "ALTER TABLE app.events ADD CONSTRAINT c CHECK (active)",
            "ALTER TABLE app.events ADD CHECK (missing IS NULL OR)",
        ] {
            assert!(parse_add_check_constraint(sql).is_err(), "accepted {sql}");
        }
        assert!(
            parse_add_check_constraint("ALTER TABLE app.events ADD CHECK (amount >= 0)").is_ok()
        );
    }

    #[test]
    fn parses_column_nullability_alterations_only() {
        assert_eq!(
            parse_alter_column_nullability(
                "ALTER TABLE app.employees ALTER COLUMN active SET NOT NULL;"
            )
            .unwrap(),
            AlterColumnNullabilitySql {
                schema: "app".into(),
                table: "employees".into(),
                column: "active".into(),
                nullable: false
            }
        );
        assert!(
            parse_alter_column_nullability(
                "ALTER TABLE app.employees ALTER COLUMN active DROP NOT NULL"
            )
            .unwrap()
            .nullable
        );
        for sql in [
            "ALTER TABLE employees ALTER COLUMN active SET NOT NULL",
            "ALTER TABLE app.employees ALTER active SET NOT NULL",
            "ALTER TABLE app.employees ALTER COLUMN active SET NULL",
            "ALTER TABLE app.employees ALTER COLUMN active SET NOT NULL; ALTER TABLE app.employees ALTER COLUMN active DROP NOT NULL",
        ] {
            assert!(
                parse_alter_column_nullability(sql).is_err(),
                "accepted {sql}"
            );
        }
    }

    #[test]
    fn parses_typed_column_default_alterations() {
        assert_eq!(
            parse_alter_column_default(
                "ALTER TABLE app.events ALTER COLUMN amount SET DEFAULT -12.30"
            )
            .unwrap(),
            AlterColumnDefaultSql {
                schema: "app".into(),
                table: "events".into(),
                column: "amount".into(),
                default: Some(DefaultLiteralSql::Number("-12.30".into())),
            }
        );
        assert_eq!(
            parse_alter_column_default("ALTER TABLE app.events ALTER COLUMN amount DROP DEFAULT;")
                .unwrap()
                .default,
            None
        );
        for (sql, expected) in [
            (
                "ALTER TABLE app.events ALTER COLUMN label SET DEFAULT 'ready'",
                DefaultLiteralSql::String("ready".into()),
            ),
            (
                "ALTER TABLE app.events ALTER COLUMN enabled SET DEFAULT TRUE",
                DefaultLiteralSql::Boolean(true),
            ),
            (
                "ALTER TABLE app.events ALTER COLUMN payload SET DEFAULT X'00ff'",
                DefaultLiteralSql::Bytea(vec![0, 255]),
            ),
            (
                "ALTER TABLE app.events ALTER COLUMN owner SET DEFAULT CURRENT_PRINCIPAL_ID()",
                DefaultLiteralSql::CurrentPrincipalId,
            ),
            (
                "ALTER TABLE app.events ALTER COLUMN label SET DEFAULT NULL",
                DefaultLiteralSql::Null,
            ),
        ] {
            assert_eq!(
                parse_alter_column_default(sql).unwrap().default,
                Some(expected)
            );
        }
        let decimal = DefaultLiteralSql::Number("-12.30".into());
        assert_eq!(
            bind_default_literal(
                &decimal,
                ScalarType::Decimal {
                    precision: 5,
                    scale: 2,
                },
            )
            .unwrap(),
            AdminColumnDefault::Literal(ScalarValue::Decimal(-1230))
        );
        assert!(bind_default_literal(&decimal, ScalarType::Integer).is_err());
        assert!(
            bind_default_literal(&DefaultLiteralSql::CurrentPrincipalId, ScalarType::Integer)
                .is_err()
        );
        for sql in [
            "ALTER TABLE app.events ALTER COLUMN label SET DEFAULT",
            "ALTER TABLE app.events ALTER COLUMN label DROP NOT NULL",
            "ALTER TABLE events ALTER COLUMN label DROP DEFAULT",
            "ALTER TABLE app.events ALTER COLUMN label DROP DEFAULT; ALTER TABLE app.events ALTER COLUMN label DROP DEFAULT",
        ] {
            assert!(parse_alter_column_default(sql).is_err(), "accepted {sql}");
        }
    }

    #[test]
    fn parses_bounded_create_index_sql() {
        assert_eq!(
            parse_create_index(
                "CREATE INDEX employee_department ON app.employees USING BTREE (department_id, name);"
            )
            .unwrap(),
            CreateIndexSql {
                schema: "app".into(),
                table: "employees".into(),
                name: "employee_department".into(),
                columns: vec!["department_id".into(), "name".into()],
                unique: false,
            }
        );
        let unique = parse_create_index(
            "CREATE UNIQUE INDEX employee_department ON app.employees (department_id, name)",
        )
        .unwrap();
        assert!(unique.unique);
        assert_eq!(unique.columns, ["department_id", "name"]);
        for sql in [
            "CREATE INDEX employee_department ON employees (department_id)",
            "CREATE INDEX employee_department ON app.employees USING HASH (department_id)",
            "CREATE INDEX employee_department ON app.employees (department_id, department_id)",
            "CREATE INDEX employee_department ON app.employees ()",
            "CREATE INDEX employee_department ON app.employees (department_id); DROP INDEX app.employee_department RESTRICT",
        ] {
            assert!(parse_create_index(sql).is_err(), "accepted {sql}");
        }
    }

    #[test]
    fn parses_typed_add_column_with_bounded_defaults() {
        let parsed = parse_add_column(
            "ALTER TABLE app.events ADD COLUMN label VARCHAR(30) NOT NULL DEFAULT 'unknown';",
        )
        .unwrap();
        assert_eq!(parsed.schema, "app");
        assert_eq!(parsed.table, "events");
        assert_eq!(parsed.column.name, "label");
        assert_eq!(
            parsed.column.scalar_type,
            ScalarType::Varchar { max_chars: 30 }
        );
        assert!(!parsed.column.nullable);
        assert_eq!(
            parsed.column.default,
            Some(AdminColumnDefault::Literal(ScalarValue::Varchar(
                "unknown".into()
            )))
        );
        for sql in [
            "ALTER TABLE app.events ADD COLUMN label TEXT NOT NULL",
            "ALTER TABLE app.events ADD COLUMN label TEXT DEFAULT CURRENT_PRINCIPAL_ID()",
            "ALTER TABLE app.events ADD COLUMN label TEXT NOT NULL DEFAULT NULL",
            "ALTER TABLE events ADD COLUMN label TEXT",
            "ALTER TABLE app.events ADD COLUMN label TEXT; ALTER TABLE app.events ADD COLUMN extra TEXT",
        ] {
            assert!(parse_add_column(sql).is_err(), "accepted {sql}");
        }
    }

    #[test]
    fn parses_qualified_column_rename_only() {
        assert_eq!(
            parse_rename_column("ALTER TABLE app.events RENAME COLUMN old_name TO new_name;")
                .unwrap(),
            RenameColumnSql {
                schema: "app".into(),
                table: "events".into(),
                column: "old_name".into(),
                new_name: "new_name".into()
            }
        );
        for sql in [
            "ALTER TABLE events RENAME COLUMN old_name TO new_name",
            "ALTER TABLE app.events RENAME old_name TO new_name",
            "ALTER TABLE app.events RENAME COLUMN old_name new_name",
            "ALTER TABLE app.events RENAME COLUMN old_name TO new_name; ALTER TABLE app.events RENAME COLUMN x TO y",
        ] {
            assert!(parse_rename_column(sql).is_err(), "accepted {sql}");
        }
    }

    #[test]
    fn parses_restricted_column_drop_only() {
        assert_eq!(
            parse_drop_column("ALTER TABLE app.events DROP COLUMN obsolete RESTRICT;").unwrap(),
            DropColumnSql {
                schema: "app".into(),
                table: "events".into(),
                column: "obsolete".into()
            }
        );
        for sql in [
            "ALTER TABLE events DROP COLUMN obsolete RESTRICT",
            "ALTER TABLE app.events DROP obsolete RESTRICT",
            "ALTER TABLE app.events DROP COLUMN obsolete CASCADE",
            "ALTER TABLE app.events DROP COLUMN obsolete RESTRICT; ALTER TABLE app.events DROP COLUMN other RESTRICT",
        ] {
            assert!(parse_drop_column(sql).is_err(), "accepted {sql}");
        }
    }

    #[test]
    fn parses_composite_primary_key_and_implies_not_null() {
        let parsed = parse_create_table(
            "CREATE TABLE app.events (tenant UUID, event_id BIGINT, label TEXT, PRIMARY KEY (tenant, event_id))",
        )
        .unwrap();
        assert_eq!(
            parsed.primary_key_columns,
            Some(vec!["tenant".into(), "event_id".into()])
        );
        assert!(!parsed.columns[0].nullable);
        assert!(!parsed.columns[1].nullable);
        assert!(parsed.columns[2].nullable);
        for sql in [
            "CREATE TABLE app.t (id UUID, PRIMARY KEY (missing))",
            "CREATE TABLE app.t (id UUID, PRIMARY KEY (id, id))",
            "CREATE TABLE app.t (id UUID, PRIMARY KEY (id), extra INTEGER)",
            "CREATE TABLE app.t (id UUID PRIMARY KEY)",
        ] {
            assert!(parse_create_table(sql).is_err(), "accepted {sql}");
        }
    }

    #[test]
    fn parses_table_level_foreign_key_constraints() {
        let parsed = parse_create_table(
            "CREATE TABLE app.order_lines (customer_id INTEGER, order_id BIGINT, FOREIGN KEY (customer_id) REFERENCES crm.customers (id), FOREIGN KEY (order_id) REFERENCES app.orders (id));",
        )
        .unwrap();
        assert_eq!(
            parsed.foreign_keys,
            vec![
                CreateTableForeignKeySql {
                    columns: vec!["customer_id".into()],
                    referenced_schema: "crm".into(),
                    referenced_table: "customers".into(),
                    referenced_columns: vec!["id".into()],
                },
                CreateTableForeignKeySql {
                    columns: vec!["order_id".into()],
                    referenced_schema: "app".into(),
                    referenced_table: "orders".into(),
                    referenced_columns: vec!["id".into()],
                },
            ]
        );
        for sql in [
            "CREATE TABLE app.order_lines (customer_id INTEGER, FOREIGN KEY (missing) REFERENCES crm.customers (id))",
            "CREATE TABLE app.order_lines (customer_id INTEGER, FOREIGN KEY (customer_id, customer_id) REFERENCES crm.customers (id, other))",
            "CREATE TABLE app.order_lines (customer_id INTEGER, FOREIGN KEY (customer_id) REFERENCES customers (id))",
            "CREATE TABLE app.order_lines (customer_id INTEGER, FOREIGN KEY (customer_id) REFERENCES crm.customers (id)); CREATE TABLE app.x (id INTEGER)",
        ] {
            assert!(parse_create_table(sql).is_err(), "accepted {sql}");
        }
    }

    #[test]
    fn parses_multiple_table_unique_constraints() {
        let parsed = parse_create_table(
            "CREATE TABLE app.events (id UUID NOT NULL, email TEXT, tenant UUID, PRIMARY KEY (id), UNIQUE (email), UNIQUE (tenant, email));",
        )
        .unwrap();
        assert_eq!(parsed.primary_key_columns, Some(vec!["id".into()]));
        assert_eq!(
            parsed.unique_constraints,
            vec![
                vec!["email".to_owned()],
                vec!["tenant".to_owned(), "email".to_owned()],
            ]
        );
        assert!(!parsed.columns[0].nullable);
        assert!(parsed.columns[1].nullable);
        for sql in [
            "CREATE TABLE app.events (id UUID, UNIQUE (missing))",
            "CREATE TABLE app.events (id UUID, UNIQUE (id, id))",
            "CREATE TABLE app.events (id UUID, UNIQUE (id), UNIQUE (id))",
        ] {
            assert!(parse_create_table(sql).is_err(), "accepted {sql}");
        }
    }

    #[test]
    fn rejects_unsupported_or_ambiguous_ddl() {
        for sql in [
            "CREATE TABLE events (id UUID)",
            "CREATE TABLE app.events (id UUID PRIMARY KEY)",
            "CREATE TABLE app.events (id UUID); CREATE TABLE app.other (id UUID)",
            "CREATE TABLE \"app\".events (id UUID)",
            "CREATE TABLE app.events (id DECIMAL(39, 0))",
        ] {
            assert!(parse_create_table(sql).is_err(), "accepted {sql}");
        }
    }

    #[test]
    fn parses_exact_typed_defaults_and_modifier_orders() {
        for column in [
            "label TEXT DEFAULT 'O''Reilly'",
            "amount DECIMAL(6,2) DEFAULT -12.30 NOT NULL",
            "active BOOLEAN NOT NULL DEFAULT TRUE",
            "raw BYTEA DEFAULT X'00ff'",
            "owner UUID DEFAULT CURRENT_PRINCIPAL_ID()",
        ] {
            parse_create_table(&format!("CREATE TABLE app.values ({column})"))
                .unwrap_or_else(|error| panic!("{column}: {error:?}"));
        }
        let parsed = parse_create_table(
            "CREATE TABLE app.values (label TEXT DEFAULT 'O''Reilly', amount DECIMAL(6,2) DEFAULT -12.30 NOT NULL, active BOOLEAN NOT NULL DEFAULT TRUE, raw BYTEA DEFAULT X'00FF', owner UUID DEFAULT CURRENT_PRINCIPAL_ID(), fixed_id UUID DEFAULT '00112233-4455-6677-8899-aabbccddeeff', optional TEXT DEFAULT NULL);",
        )
        .unwrap();
        assert_eq!(
            parsed.columns[0].default,
            Some(AdminColumnDefault::Literal(ScalarValue::Text(
                "O'Reilly".into()
            )))
        );
        assert_eq!(
            parsed.columns[1].default,
            Some(AdminColumnDefault::Literal(ScalarValue::Decimal(-1230)))
        );
        assert!(!parsed.columns[1].nullable);
        assert!(!parsed.columns[2].nullable);
        assert_eq!(
            parsed.columns[2].default,
            Some(AdminColumnDefault::Literal(ScalarValue::Boolean(true)))
        );
        assert_eq!(
            parsed.columns[3].default,
            Some(AdminColumnDefault::Literal(ScalarValue::Bytea(vec![
                0, 255
            ])))
        );
        assert_eq!(
            parsed.columns[4].default,
            Some(AdminColumnDefault::CurrentPrincipalId)
        );
        assert_eq!(
            parsed.columns[5].default,
            Some(AdminColumnDefault::Literal(ScalarValue::Uuid([
                0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd,
                0xee, 0xff,
            ])))
        );
        assert_eq!(parsed.columns[6].default, Some(AdminColumnDefault::Null));
    }

    #[test]
    fn parses_storage_generated_uuid_identity_and_rejects_conflicts() {
        let parsed = parse_create_table(
            "CREATE TABLE app.events (id UUID GENERATED ALWAYS AS IDENTITY, other UUID GENERATED ALWAYS AS IDENTITY NOT NULL)",
        )
        .unwrap();
        assert!(parsed.columns.iter().all(|column| {
            column.generated_uuid_identity && !column.nullable && column.default.is_none()
        }));

        for sql in [
            "CREATE TABLE app.t (id INTEGER GENERATED ALWAYS AS IDENTITY)",
            "CREATE TABLE app.t (id UUID GENERATED ALWAYS AS IDENTITY DEFAULT NULL)",
            "CREATE TABLE app.t (id UUID GENERATED AS IDENTITY)",
        ] {
            assert!(parse_create_table(sql).is_err(), "accepted {sql}");
        }
    }

    #[test]
    fn rejects_defaults_with_loss_or_type_mismatch() {
        for sql in [
            "CREATE TABLE app.t (n DECIMAL(4,2) DEFAULT 1.234)",
            "CREATE TABLE app.t (n DECIMAL(2,1) DEFAULT 123.0)",
            "CREATE TABLE app.t (n INTEGER DEFAULT '1')",
            "CREATE TABLE app.t (n BYTEA DEFAULT X'0')",
            "CREATE TABLE app.t (n INTEGER DEFAULT CURRENT_PRINCIPAL_ID())",
            "CREATE TABLE app.t (n VARCHAR(2) DEFAULT 'abc')",
        ] {
            assert!(parse_create_table(sql).is_err(), "accepted {sql}");
        }
    }
}
