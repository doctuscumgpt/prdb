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
pub enum CheckExpressionSql {
    Column(String),
    IsNull(String),
    IsNotNull(String),
    Not(Box<CheckExpressionSql>),
    And(Vec<CheckExpressionSql>),
    Or(Vec<CheckExpressionSql>),
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
    let mut saw_table_constraint = false;
    loop {
        if parser.peek_word("primary") || parser.peek_word("unique") {
            saw_table_constraint = true;
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
    Ok(CreateTableSql {
        schema,
        table,
        columns,
        primary_key_columns,
        unique_constraints,
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

/// Parse `ALTER TABLE schema.table ADD CHECK (expression)` using a bounded
/// boolean grammar over column truth values and `IS [NOT] NULL` predicates.
pub fn parse_add_check_constraint(sql: &str) -> Result<AddCheckConstraintSql, DdlSqlError> {
    let mut parser = statement_parser(sql)?;
    parser.word("alter")?;
    parser.word("table")?;
    let schema = parser.identifier()?;
    parser.take(Token::Dot)?;
    let table = parser.identifier()?;
    parser.word("add")?;
    parser.word("check")?;
    parser.take(Token::LeftParen)?;
    let expression = parser.check_or()?;
    parser.take(Token::RightParen)?;
    finish_statement(&mut parser)?;
    Ok(AddCheckConstraintSql {
        schema,
        table,
        expression,
    })
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
        let column = self.identifier()?;
        if self.peek_word("is") {
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
            Ok(CheckExpressionSql::Column(column))
        }
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
            "ALTER TABLE app.events ADD CHECK (amount >= 0)",
            "ALTER TABLE app.events ADD CHECK (active); ALTER TABLE app.events ADD CHECK (active)",
            "ALTER TABLE events ADD CHECK (active)",
            "ALTER TABLE app.events ADD CONSTRAINT c CHECK (active)",
            "ALTER TABLE app.events ADD CHECK (missing IS NULL OR)",
        ] {
            assert!(parse_add_check_constraint(sql).is_err(), "accepted {sql}");
        }
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
