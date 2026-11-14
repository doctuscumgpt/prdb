//! Bounded SQL DDL accepted by the authenticated administrative client.
//!
//! This parser only translates a deliberately small CREATE TABLE grammar to
//! the existing typed control operation. Authorization and catalog validation
//! remain server-side.

use crate::{client_submit::AdminColumnDefinition, tuple::ScalarType};

const MAX_DDL_BYTES: usize = 65_536;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateTableSql {
    pub schema: String,
    pub table: String,
    pub columns: Vec<AdminColumnDefinition>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DdlSqlError {
    InvalidStatement,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Token {
    Word(String),
    Number(String),
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
    loop {
        if columns.len() == 4096 {
            return Err(DdlSqlError::InvalidStatement);
        }
        let name = parser.identifier()?;
        let scalar_type = parser.scalar_type()?;
        let nullable = if parser.peek_word("not") {
            parser.word("not")?;
            parser.word("null")?;
            false
        } else {
            true
        };
        columns.push(AdminColumnDefinition {
            name,
            scalar_type,
            nullable,
            generated_uuid_identity: false,
            default: None,
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
    Ok(CreateTableSql {
        schema,
        table,
        columns,
    })
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

    fn identifier(&mut self) -> Result<String, DdlSqlError> {
        match self.peek() {
            Some(Token::Word(word))
                if !word.is_empty()
                    && word.len() <= 63
                    && word.as_bytes()[0].is_ascii_lowercase()
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
}
