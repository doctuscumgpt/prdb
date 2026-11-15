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
const MAX_CHECK_IN_VALUES: usize = 64;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateTableSql {
    pub schema: String,
    pub table: String,
    pub if_not_exists: bool,
    pub columns: Vec<AdminColumnDefinition>,
    pub primary_key_columns: Option<Vec<String>>,
    pub primary_key_name: Option<String>,
    pub unique_constraints: Vec<Vec<String>>,
    pub unique_constraint_names: Vec<Option<String>>,
    pub check_constraints: Vec<CheckExpressionSql>,
    pub check_constraint_names: Vec<Option<String>>,
    pub foreign_key_names: Vec<Option<String>>,
    pub foreign_keys: Vec<CreateTableForeignKeySql>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateTableForeignKeySql {
    pub name: Option<String>,
    pub columns: Vec<String>,
    pub referenced_schema: String,
    pub referenced_table: String,
    pub referenced_columns: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateSchemaSql {
    pub schema: String,
    pub owner: CreateSchemaOwnerSql,
    pub if_not_exists: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SqlIdentityKind {
    Either,
    User,
    Role,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CreateSchemaOwnerSql {
    CurrentUser,
    Id([u8; 16]),
    Identity { name: String, kind: SqlIdentityKind },
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DropTableSql {
    pub schema: String,
    pub table: String,
    pub if_exists: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TruncateTableSql {
    pub schema: String,
    pub table: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DropSchemaSql {
    pub schema: String,
    pub if_exists: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnerTransferSql {
    pub schema: String,
    pub table: Option<String>,
    pub owner_name: String,
    pub owner_kind: SqlIdentityKind,
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
    pub name: Option<String>,
    pub columns: Vec<String>,
    pub kind: KeyConstraintKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AddForeignKeySql {
    pub schema: String,
    pub table: String,
    pub name: Option<String>,
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
    pub constraint_name: Option<String>,
    pub if_exists: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AddCheckConstraintSql {
    pub schema: String,
    pub table: String,
    pub name: Option<String>,
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
pub struct WidenVarcharSql {
    pub schema: String,
    pub table: String,
    pub column: String,
    pub max_chars: u32,
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
    pub if_not_exists: bool,
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
    pub if_exists: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateIndexSql {
    pub schema: String,
    pub table: String,
    pub name: String,
    pub columns: Vec<String>,
    pub unique: bool,
    pub if_not_exists: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DropIndexSql {
    pub schema: String,
    pub name: String,
    pub if_exists: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RebuildIndexSql {
    pub schema: String,
    pub name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AlterIndexRebuildSql {
    pub schema: String,
    pub name: String,
    pub columns: Vec<String>,
    pub unique: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenameIndexSql {
    pub schema: String,
    pub name: String,
    pub new_name: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CreateIdentityKind {
    User,
    Role,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateIdentitySql {
    pub kind: CreateIdentityKind,
    pub if_not_exists: bool,
    pub name: String,
    pub login_enabled: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenameIdentitySql {
    pub kind: CreateIdentityKind,
    pub old_name: String,
    pub new_name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DropIdentitySql {
    pub kind: CreateIdentityKind,
    pub name: String,
    pub if_exists: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SetUserLoginSql {
    pub name: String,
    pub enabled: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoleMembershipSql {
    pub grant: bool,
    pub role_name: String,
    pub member_name: String,
    pub member_is_role: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SystemPrivilegeSql {
    pub grant: bool,
    pub privilege: String,
    pub grantee_name: String,
    pub grantee_is_role: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObjectPrivilegeSql {
    pub grant: bool,
    pub privilege: String,
    pub schema_name: String,
    pub table_name: Option<String>,
    pub column_names: Vec<String>,
    pub grantee_name: String,
    pub grantee_is_role: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CheckExpressionSql {
    Column(String),
    Literal(CheckLiteralSql),
    CurrentPrincipalId,
    HasRole(String),
    Add(Box<CheckExpressionSql>, Box<CheckExpressionSql>),
    Subtract(Box<CheckExpressionSql>, Box<CheckExpressionSql>),
    Multiply(Box<CheckExpressionSql>, Box<CheckExpressionSql>),
    Modulo(Box<CheckExpressionSql>, Box<CheckExpressionSql>),
    Divide(Box<CheckExpressionSql>, Box<CheckExpressionSql>),
    Negate(Box<CheckExpressionSql>),
    IsNull(Box<CheckExpressionSql>),
    IsNotNull(Box<CheckExpressionSql>),
    BooleanTest {
        expression: Box<CheckExpressionSql>,
        test: CheckBooleanTest,
        negated: bool,
    },
    Compare {
        operator: CheckComparisonOperator,
        left: Box<CheckExpressionSql>,
        right: Box<CheckExpressionSql>,
    },
    In {
        expression: Box<CheckExpressionSql>,
        values: Vec<Box<CheckExpressionSql>>,
        negated: bool,
    },
    Between {
        expression: Box<CheckExpressionSql>,
        lower: Box<CheckExpressionSql>,
        upper: Box<CheckExpressionSql>,
        negated: bool,
    },
    DistinctFrom {
        expression: Box<CheckExpressionSql>,
        right: Box<CheckExpressionSql>,
        negated: bool,
    },
    Not(Box<CheckExpressionSql>),
    And(Vec<CheckExpressionSql>),
    Or(Vec<CheckExpressionSql>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PolicyRecipientSql {
    pub name: String,
    pub is_role: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreatePolicySql {
    pub replace_existing: bool,
    pub schema: String,
    pub table: String,
    pub name: String,
    pub is_cell: bool,
    pub columns: Vec<String>,
    pub operation: String,
    pub recipients: Vec<PolicyRecipientSql>,
    pub effect: String,
    pub using_expression: Option<CheckExpressionSql>,
    pub with_check_expression: Option<CheckExpressionSql>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DropPolicySql {
    pub schema: String,
    pub table: String,
    pub name: String,
    pub is_cell: bool,
    pub if_exists: bool,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CheckBooleanTest {
    True,
    False,
    Unknown,
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
    Star,
    Percent,
    Slash,
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
    let if_not_exists = if parser.peek_word("if") {
        parser.word("if")?;
        parser.word("not")?;
        parser.word("exists")?;
        true
    } else {
        false
    };
    let schema = parser.identifier()?;
    parser.take(Token::Dot)?;
    let table = parser.identifier()?;
    parser.take(Token::LeftParen)?;
    let mut columns = Vec::new();
    let mut primary_key_columns = None;
    let mut primary_key_name = None;
    let mut unique_constraints = Vec::new();
    let mut unique_constraint_names = Vec::new();
    let mut check_constraints = Vec::new();
    let mut check_constraint_names = Vec::new();
    let mut foreign_keys = Vec::new();
    let mut saw_table_constraint = false;
    loop {
        let constraint_name = if parser.peek_word("constraint") {
            parser.word("constraint")?;
            Some(parser.identifier()?)
        } else {
            None
        };
        if parser.peek_word("primary")
            || parser.peek_word("unique")
            || parser.peek_word("check")
            || parser.peek_word("foreign")
        {
            saw_table_constraint = true;
            if parser.peek_word("check") {
                if constraint_name.is_some() && check_constraints.len() >= crate::tuple::MAX_COLUMNS
                {
                    return Err(DdlSqlError::InvalidStatement);
                }
                let expression = parser.check_constraint_expression()?;
                check_constraints.push(expression);
                check_constraint_names.push(constraint_name);
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
                    name: constraint_name.clone(),
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
                    primary_key_name = constraint_name;
                } else {
                    if unique_constraints.len() >= crate::tuple::MAX_COLUMNS
                        || unique_constraints.contains(&names)
                    {
                        return Err(DdlSqlError::InvalidStatement);
                    }
                    unique_constraints.push(names);
                    unique_constraint_names.push(constraint_name);
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
        let mut unique = false;
        let mut unique_name = None;
        let mut primary_key = false;
        let mut primary_key_column_name = None;
        loop {
            if parser.peek_word("not") && nullable {
                parser.word("not")?;
                parser.word("null")?;
                nullable = false;
            } else if parser.peek_word("default") && default.is_none() {
                parser.word("default")?;
                default = Some(parser.default_value(scalar_type)?);
            } else if parser.peek_word("unique") && !unique {
                parser.word("unique")?;
                unique = true;
            } else if parser.peek_word("constraint") {
                parser.word("constraint")?;
                let constraint_name = parser.identifier()?;
                if parser.peek_word("check") {
                    if check_constraints.len() >= crate::tuple::MAX_COLUMNS {
                        return Err(DdlSqlError::InvalidStatement);
                    }
                    let expression = parser.check_constraint_expression()?;
                    validate_column_check_expression(&name, &expression)?;
                    check_constraints.push(expression);
                    check_constraint_names.push(Some(constraint_name));
                } else if parser.peek_word("primary") && !primary_key {
                    parser.word("primary")?;
                    parser.word("key")?;
                    primary_key = true;
                    primary_key_column_name = Some(constraint_name);
                } else if parser.peek_word("unique") && !unique {
                    parser.word("unique")?;
                    unique = true;
                    unique_name = Some(constraint_name);
                } else if parser.peek_word("references") {
                    if foreign_keys.len() >= crate::tuple::MAX_COLUMNS {
                        return Err(DdlSqlError::InvalidStatement);
                    }
                    parser.word("references")?;
                    let referenced_schema = parser.identifier()?;
                    parser.take(Token::Dot)?;
                    let referenced_table = parser.identifier()?;
                    let referenced_columns = parse_column_name_list(&mut parser)?;
                    if referenced_columns.len() != 1 {
                        return Err(DdlSqlError::InvalidStatement);
                    }
                    foreign_keys.push(CreateTableForeignKeySql {
                        name: Some(constraint_name),
                        columns: vec![name.clone()],
                        referenced_schema,
                        referenced_table,
                        referenced_columns,
                    });
                } else {
                    return Err(DdlSqlError::InvalidStatement);
                }
            } else if parser.peek_word("primary") && !primary_key {
                parser.word("primary")?;
                parser.word("key")?;
                primary_key = true;
            } else if parser.peek_word("check") || parser.peek_word("constraint") {
                let constraint_name = if parser.peek_word("constraint") {
                    parser.word("constraint")?;
                    Some(parser.identifier()?)
                } else {
                    None
                };
                if check_constraints.len() >= crate::tuple::MAX_COLUMNS {
                    return Err(DdlSqlError::InvalidStatement);
                }
                let expression = parser.check_constraint_expression()?;
                validate_column_check_expression(&name, &expression)?;
                check_constraints.push(expression);
                check_constraint_names.push(constraint_name);
            } else if parser.peek_word("references") {
                if foreign_keys.len() >= crate::tuple::MAX_COLUMNS {
                    return Err(DdlSqlError::InvalidStatement);
                }
                parser.word("references")?;
                let referenced_schema = parser.identifier()?;
                parser.take(Token::Dot)?;
                let referenced_table = parser.identifier()?;
                let referenced_columns = parse_column_name_list(&mut parser)?;
                if referenced_columns.len() != 1 {
                    return Err(DdlSqlError::InvalidStatement);
                }
                foreign_keys.push(CreateTableForeignKeySql {
                    name: None,
                    columns: vec![name.clone()],
                    referenced_schema,
                    referenced_table,
                    referenced_columns,
                });
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
        if primary_key {
            nullable = false;
        }
        columns.push(AdminColumnDefinition {
            name: name.clone(),
            scalar_type,
            nullable,
            generated_uuid_identity,
            default,
        });
        if primary_key && primary_key_columns.replace(vec![name.clone()]).is_some() {
            return Err(DdlSqlError::InvalidStatement);
        }
        if primary_key {
            primary_key_name = primary_key_column_name;
        }
        if unique {
            let key_columns = vec![name.clone()];
            if unique_constraints.contains(&key_columns) {
                return Err(DdlSqlError::InvalidStatement);
            }
            unique_constraints.push(key_columns);
            unique_constraint_names.push(unique_name);
        }
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
    if check_constraint_names.len() != check_constraints.len()
        || check_constraint_names
            .iter()
            .flatten()
            .collect::<std::collections::HashSet<_>>()
            .len()
            != check_constraint_names.iter().flatten().count()
    {
        return Err(DdlSqlError::InvalidStatement);
    }
    let foreign_key_names = foreign_keys
        .iter()
        .map(|foreign_key| foreign_key.name.clone())
        .collect::<Vec<_>>();
    let named_names = primary_key_name
        .iter()
        .chain(unique_constraint_names.iter().flatten())
        .chain(check_constraint_names.iter().flatten())
        .chain(foreign_key_names.iter().flatten())
        .collect::<std::collections::HashSet<_>>();
    if named_names.len()
        != usize::from(primary_key_name.is_some())
            + unique_constraint_names.iter().flatten().count()
            + check_constraint_names.iter().flatten().count()
            + foreign_key_names.iter().flatten().count()
    {
        return Err(DdlSqlError::InvalidStatement);
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
        if_not_exists,
        columns,
        primary_key_columns,
        primary_key_name,
        unique_constraints,
        unique_constraint_names,
        check_constraints,
        check_constraint_names,
        foreign_key_names,
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
    let if_not_exists = if parser.peek_word("if") {
        parser.word("if")?;
        parser.word("not")?;
        parser.word("exists")?;
        true
    } else {
        false
    };
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
        if_not_exists,
    })
}

/// Parse a bounded schema-qualified DROP INDEX with explicit RESTRICT.
pub fn parse_drop_index(sql: &str) -> Result<DropIndexSql, DdlSqlError> {
    let mut parser = statement_parser(sql)?;
    parser.word("drop")?;
    parser.word("index")?;
    let if_exists = if parser.peek_word("if") {
        parser.word("if")?;
        parser.word("exists")?;
        true
    } else {
        false
    };
    let schema = parser.identifier()?;
    parser.take(Token::Dot)?;
    let name = parser.identifier()?;
    parser.word("restrict")?;
    finish_statement(&mut parser)?;
    Ok(DropIndexSql {
        schema,
        name,
        if_exists,
    })
}

/// Parse a bounded schema-qualified index rebuild request.
pub fn parse_rebuild_index(sql: &str) -> Result<RebuildIndexSql, DdlSqlError> {
    let mut parser = statement_parser(sql)?;
    parser.word("rebuild")?;
    parser.word("index")?;
    let schema = parser.identifier()?;
    parser.take(Token::Dot)?;
    let name = parser.identifier()?;
    finish_statement(&mut parser)?;
    Ok(RebuildIndexSql { schema, name })
}

/// Parse an atomic index definition rebuild using a new ordered column set.
pub fn parse_alter_index_rebuild(sql: &str) -> Result<AlterIndexRebuildSql, DdlSqlError> {
    let mut parser = statement_parser(sql)?;
    parser.word("alter")?;
    parser.word("index")?;
    let schema = parser.identifier()?;
    parser.take(Token::Dot)?;
    let name = parser.identifier()?;
    parser.word("rebuild")?;
    if parser.peek_word("using") {
        parser.word("using")?;
        parser.word("btree")?;
    }
    let unique = if parser.peek_word("unique") {
        parser.word("unique")?;
        true
    } else if parser.peek_word("nonunique") {
        parser.word("nonunique")?;
        false
    } else {
        return Err(DdlSqlError::InvalidStatement);
    };
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
    Ok(AlterIndexRebuildSql {
        schema,
        name,
        columns,
        unique,
    })
}

/// Parse a bounded schema-qualified index rename.
pub fn parse_rename_index(sql: &str) -> Result<RenameIndexSql, DdlSqlError> {
    let mut parser = statement_parser(sql)?;
    parser.word("alter")?;
    parser.word("index")?;
    let schema = parser.identifier()?;
    parser.take(Token::Dot)?;
    let name = parser.identifier()?;
    parser.word("rename")?;
    parser.word("to")?;
    let new_name = parser.identifier()?;
    finish_statement(&mut parser)?;
    Ok(RenameIndexSql {
        schema,
        name,
        new_name,
    })
}

/// Parse the bounded administrative `CREATE USER` or `CREATE ROLE` form.
pub fn parse_create_identity(sql: &str) -> Result<CreateIdentitySql, DdlSqlError> {
    let mut parser = statement_parser(sql)?;
    parser.word("create")?;
    let kind = if parser.peek_word("user") {
        parser.word("user")?;
        CreateIdentityKind::User
    } else {
        parser.word("role")?;
        CreateIdentityKind::Role
    };
    let if_not_exists = if parser.peek_word("if") {
        parser.word("if")?;
        parser.word("not")?;
        parser.word("exists")?;
        true
    } else {
        false
    };
    let name = parser.identifier()?;
    let login_enabled = if kind == CreateIdentityKind::User {
        if parser.peek_word("login") {
            parser.word("login")?;
            true
        } else {
            parser.word("nologin")?;
            false
        }
    } else {
        false
    };
    finish_statement(&mut parser)?;
    Ok(CreateIdentitySql {
        kind,
        if_not_exists,
        name,
        login_enabled,
    })
}

/// Parse `ALTER USER|ROLE old_name RENAME TO new_name`.
pub fn parse_rename_identity(sql: &str) -> Result<RenameIdentitySql, DdlSqlError> {
    let mut parser = statement_parser(sql)?;
    parser.word("alter")?;
    let kind = if parser.peek_word("user") {
        parser.word("user")?;
        CreateIdentityKind::User
    } else {
        parser.word("role")?;
        CreateIdentityKind::Role
    };
    let old_name = parser.identifier()?;
    parser.word("rename")?;
    parser.word("to")?;
    let new_name = parser.identifier()?;
    finish_statement(&mut parser)?;
    Ok(RenameIdentitySql {
        kind,
        old_name,
        new_name,
    })
}

/// Parse `DROP USER|ROLE [IF EXISTS] name RESTRICT`.
pub fn parse_drop_identity(sql: &str) -> Result<DropIdentitySql, DdlSqlError> {
    let mut parser = statement_parser(sql)?;
    parser.word("drop")?;
    let kind = if parser.peek_word("user") {
        parser.word("user")?;
        CreateIdentityKind::User
    } else {
        parser.word("role")?;
        CreateIdentityKind::Role
    };
    let if_exists = if parser.peek_word("if") {
        parser.word("if")?;
        parser.word("exists")?;
        true
    } else {
        false
    };
    let name = parser.identifier()?;
    parser.word("restrict")?;
    finish_statement(&mut parser)?;
    Ok(DropIdentitySql {
        kind,
        name,
        if_exists,
    })
}

/// Parse `ALTER USER name LOGIN|NOLOGIN`.
pub fn parse_set_user_login(sql: &str) -> Result<SetUserLoginSql, DdlSqlError> {
    let mut parser = statement_parser(sql)?;
    parser.word("alter")?;
    parser.word("user")?;
    let name = parser.identifier()?;
    let enabled = if parser.peek_word("login") {
        parser.word("login")?;
        true
    } else {
        parser.word("nologin")?;
        false
    };
    finish_statement(&mut parser)?;
    Ok(SetUserLoginSql { name, enabled })
}

/// Parse `GRANT role TO USER|ROLE member` and its REVOKE counterpart.
pub fn parse_role_membership(sql: &str) -> Result<RoleMembershipSql, DdlSqlError> {
    let mut parser = statement_parser(sql)?;
    let grant = if parser.peek_word("grant") {
        parser.word("grant")?;
        true
    } else {
        parser.word("revoke")?;
        false
    };
    let role_name = parser.identifier()?;
    if grant {
        parser.word("to")?;
    } else {
        parser.word("from")?;
    }
    let member_is_role = if parser.peek_word("user") {
        parser.word("user")?;
        false
    } else {
        parser.word("role")?;
        true
    };
    let member_name = parser.identifier()?;
    finish_statement(&mut parser)?;
    Ok(RoleMembershipSql {
        grant,
        role_name,
        member_name,
        member_is_role,
    })
}

/// Parse a database-scoped system privilege grant or revoke.
///
/// Both the explicit `SYSTEM PRIVILEGE` form and the concise `ON DATABASE`
/// form are accepted for compatibility with the permission DDL syntax.
pub fn parse_system_privilege(sql: &str) -> Result<SystemPrivilegeSql, DdlSqlError> {
    let mut parser = statement_parser(sql)?;
    let grant = if parser.peek_word("grant") {
        parser.word("grant")?;
        true
    } else {
        parser.word("revoke")?;
        false
    };
    let explicit_system_privilege = parser.peek_word("system");
    let privilege = if explicit_system_privilege {
        parser.word("system")?;
        parser.word("privilege")?;
        parser.identifier()?
    } else {
        parser.identifier()?
    };
    if !matches!(
        privilege.as_str(),
        "manage_users" | "manage_roles" | "create_schema"
    ) {
        return Err(DdlSqlError::InvalidStatement);
    }
    if !explicit_system_privilege {
        parser.word("on")?;
        parser.word("database")?;
    }
    if grant {
        parser.word("to")?;
    } else {
        parser.word("from")?;
    }
    let grantee_is_role = if parser.peek_word("user") {
        parser.word("user")?;
        false
    } else {
        parser.word("role")?;
        true
    };
    let grantee_name = parser.identifier()?;
    finish_statement(&mut parser)?;
    Ok(SystemPrivilegeSql {
        grant,
        privilege,
        grantee_name,
        grantee_is_role,
    })
}

/// Parse one object-scoped privilege grant/revoke on a schema or table.
pub fn parse_object_privilege(sql: &str) -> Result<ObjectPrivilegeSql, DdlSqlError> {
    let mut parser = statement_parser(sql)?;
    let grant = if parser.peek_word("grant") {
        parser.word("grant")?;
        true
    } else {
        parser.word("revoke")?;
        false
    };
    let privilege = parser.identifier()?;
    if !matches!(
        privilege.as_str(),
        "usage"
            | "select"
            | "insert"
            | "update"
            | "delete"
            | "references"
            | "create"
            | "alter"
            | "index"
            | "manage_policies"
            | "manage_grants"
    ) {
        return Err(DdlSqlError::InvalidStatement);
    }
    let mut column_names = Vec::new();
    if parser.peek() == Some(&Token::LeftParen) {
        parser.take(Token::LeftParen)?;
        loop {
            let column = parser.identifier()?;
            if column_names.contains(&column) || column_names.len() == 64 {
                return Err(DdlSqlError::InvalidStatement);
            }
            column_names.push(column);
            if parser.peek() != Some(&Token::Comma) {
                break;
            }
            parser.take(Token::Comma)?;
        }
        parser.take(Token::RightParen)?;
    }
    if !column_names.is_empty()
        && !matches!(
            privilege.as_str(),
            "select" | "insert" | "update" | "references"
        )
    {
        return Err(DdlSqlError::InvalidStatement);
    }
    parser.word("on")?;
    let is_table = if parser.peek_word("schema") {
        parser.word("schema")?;
        false
    } else {
        parser.word("table")?;
        true
    };
    if (is_table && matches!(privilege.as_str(), "usage" | "create"))
        || (!is_table
            && matches!(
                privilege.as_str(),
                "select"
                    | "insert"
                    | "update"
                    | "delete"
                    | "references"
                    | "index"
                    | "manage_policies"
                    | "manage_grants"
            ))
    {
        return Err(DdlSqlError::InvalidStatement);
    }
    if !column_names.is_empty() && !is_table {
        return Err(DdlSqlError::InvalidStatement);
    }
    let schema_name = parser.identifier()?;
    let table_name = if is_table {
        parser.take(Token::Dot)?;
        Some(parser.identifier()?)
    } else {
        None
    };
    if grant {
        parser.word("to")?;
    } else {
        parser.word("from")?;
    }
    let grantee_is_role = if parser.peek_word("user") {
        parser.word("user")?;
        false
    } else {
        parser.word("role")?;
        true
    };
    let grantee_name = parser.identifier()?;
    finish_statement(&mut parser)?;
    Ok(ObjectPrivilegeSql {
        grant,
        privilege,
        schema_name,
        table_name,
        column_names,
        grantee_name,
        grantee_is_role,
    })
}

/// Parse bounded schema creation with an optional UUID or typed owner.
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
    let if_not_exists = if parser.peek_word("if") {
        parser.word("if")?;
        parser.word("not")?;
        parser.word("exists")?;
        true
    } else {
        false
    };
    let schema = parser.identifier()?;
    let owner = if parser.peek_word("authorization") {
        parser.word("authorization")?;
        if let Some(Token::String(value)) = parser.peek() {
            let owner_id = parse_uuid(value)?;
            if owner_id == [0; 16] {
                return Err(DdlSqlError::InvalidStatement);
            }
            parser.position += 1;
            CreateSchemaOwnerSql::Id(owner_id)
        } else {
            let kind = if parser.peek_word("user") {
                parser.word("user")?;
                SqlIdentityKind::User
            } else if parser.peek_word("role") {
                parser.word("role")?;
                SqlIdentityKind::Role
            } else {
                SqlIdentityKind::Either
            };
            CreateSchemaOwnerSql::Identity {
                name: parser.identifier()?,
                kind,
            }
        }
    } else {
        CreateSchemaOwnerSql::CurrentUser
    };
    finish_statement(&mut parser)?;
    Ok(CreateSchemaSql {
        schema,
        owner,
        if_not_exists,
    })
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

pub fn parse_schema_owner_transfer(sql: &str) -> Result<OwnerTransferSql, DdlSqlError> {
    let mut parser = statement_parser(sql)?;
    parser.word("alter")?;
    parser.word("schema")?;
    let schema = parser.identifier()?;
    parser.word("owner")?;
    parser.word("to")?;
    let owner_kind = if parser.peek_word("user") {
        parser.word("user")?;
        SqlIdentityKind::User
    } else if parser.peek_word("role") {
        parser.word("role")?;
        SqlIdentityKind::Role
    } else {
        SqlIdentityKind::Either
    };
    let owner_name = parser.identifier()?;
    finish_statement(&mut parser)?;
    Ok(OwnerTransferSql {
        schema,
        table: None,
        owner_name,
        owner_kind,
    })
}

pub fn parse_drop_schema(sql: &str) -> Result<DropSchemaSql, DdlSqlError> {
    let mut parser = statement_parser(sql)?;
    parser.word("drop")?;
    parser.word("schema")?;
    let if_exists = if parser.peek_word("if") {
        parser.word("if")?;
        parser.word("exists")?;
        true
    } else {
        false
    };
    let schema = parser.identifier()?;
    parser.word("restrict")?;
    finish_statement(&mut parser)?;
    Ok(DropSchemaSql { schema, if_exists })
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

pub fn parse_table_owner_transfer(sql: &str) -> Result<OwnerTransferSql, DdlSqlError> {
    let mut parser = statement_parser(sql)?;
    parser.word("alter")?;
    parser.word("table")?;
    let schema = parser.identifier()?;
    parser.take(Token::Dot)?;
    let table = parser.identifier()?;
    parser.word("owner")?;
    parser.word("to")?;
    let owner_kind = if parser.peek_word("user") {
        parser.word("user")?;
        SqlIdentityKind::User
    } else {
        parser.word("role")?;
        SqlIdentityKind::Role
    };
    let owner_name = parser.identifier()?;
    finish_statement(&mut parser)?;
    Ok(OwnerTransferSql {
        schema,
        table: Some(table),
        owner_name,
        owner_kind,
    })
}

pub fn parse_drop_table(sql: &str) -> Result<DropTableSql, DdlSqlError> {
    let mut parser = statement_parser(sql)?;
    parser.word("drop")?;
    parser.word("table")?;
    let if_exists = if parser.peek_word("if") {
        parser.word("if")?;
        parser.word("exists")?;
        true
    } else {
        false
    };
    let schema = parser.identifier()?;
    parser.take(Token::Dot)?;
    let table = parser.identifier()?;
    parser.word("restrict")?;
    finish_statement(&mut parser)?;
    Ok(DropTableSql {
        schema,
        table,
        if_exists,
    })
}

/// Parse `TRUNCATE TABLE schema.table RESTRICT`.
pub fn parse_truncate_table(sql: &str) -> Result<TruncateTableSql, DdlSqlError> {
    let mut parser = statement_parser(sql)?;
    parser.word("truncate")?;
    parser.word("table")?;
    let schema = parser.identifier()?;
    parser.take(Token::Dot)?;
    let table = parser.identifier()?;
    parser.word("restrict")?;
    finish_statement(&mut parser)?;
    Ok(TruncateTableSql { schema, table })
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
    let name = if parser.peek_word("constraint") {
        parser.word("constraint")?;
        Some(parser.identifier()?)
    } else {
        None
    };
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
        name,
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
    let name = if parser.peek_word("constraint") {
        parser.word("constraint")?;
        Some(parser.identifier()?)
    } else {
        None
    };
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
        name,
        columns,
        referenced_schema,
        referenced_table,
        referenced_columns,
    })
}

/// Parse qualified `ALTER TABLE ... DROP CONSTRAINT name|'generated-id'`.
pub fn parse_drop_constraint(sql: &str) -> Result<DropConstraintSql, DdlSqlError> {
    let mut parser = statement_parser(sql)?;
    parser.word("alter")?;
    parser.word("table")?;
    let schema = parser.identifier()?;
    parser.take(Token::Dot)?;
    let table = parser.identifier()?;
    parser.word("drop")?;
    parser.word("constraint")?;
    let if_exists = if parser.peek_word("if") {
        parser.word("if")?;
        parser.word("exists")?;
        true
    } else {
        false
    };
    let (constraint_id, constraint_name) = match parser.peek() {
        Some(Token::String(value)) => {
            let parsed = parse_uuid(value)?;
            parser.position += 1;
            (parsed, None)
        }
        Some(Token::Word(_)) => ([0; 16], Some(parser.identifier()?)),
        _ => return Err(DdlSqlError::InvalidStatement),
    };
    finish_statement(&mut parser)?;
    if constraint_id == [0; 16] && constraint_name.is_none() {
        return Err(DdlSqlError::InvalidStatement);
    }
    Ok(DropConstraintSql {
        schema,
        table,
        constraint_id,
        constraint_name,
        if_exists,
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

/// Parse one bounded CREATE ROW/CELL POLICY statement.
pub fn parse_create_policy(sql: &str) -> Result<CreatePolicySql, DdlSqlError> {
    let mut parser = statement_parser(sql)?;
    parser.word("create")?;
    let replace_existing = if parser.peek_word("or") {
        parser.word("or")?;
        parser.word("replace")?;
        true
    } else {
        false
    };
    let is_cell = if parser.peek_word("cell") {
        parser.word("cell")?;
        true
    } else {
        parser.word("row")?;
        false
    };
    parser.word("policy")?;
    let name = parser.identifier()?;
    parser.word("on")?;
    let schema = parser.identifier()?;
    parser.take(Token::Dot)?;
    let table = parser.identifier()?;
    let columns = if is_cell && parser.peek() == Some(&Token::LeftParen) {
        parse_column_name_list(&mut parser)?
    } else {
        Vec::new()
    };
    parser.word("for")?;
    let operation = ["select", "insert", "update", "delete"]
        .into_iter()
        .find(|operation| parser.peek_word(operation))
        .ok_or(DdlSqlError::InvalidStatement)?;
    parser.position += 1;
    parser.word("to")?;
    let mut recipients = Vec::new();
    loop {
        let is_role = if parser.peek_word("user") {
            parser.word("user")?;
            false
        } else {
            parser.word("role")?;
            true
        };
        recipients.push(PolicyRecipientSql {
            name: parser.identifier()?,
            is_role,
        });
        if parser.peek() != Some(&Token::Comma) {
            break;
        }
        parser.position += 1;
    }
    parser.word("as")?;
    let effect = if parser.peek_word("allow") {
        parser.word("allow")?;
        "allow"
    } else {
        parser.word("restrict")?;
        "restrict"
    };
    let mut using_expression = None;
    let mut with_check_expression = None;
    while parser.peek_word("using") || parser.peek_word("with") {
        if parser.peek_word("using") {
            if using_expression.is_some() {
                return Err(DdlSqlError::InvalidStatement);
            }
            parser.word("using")?;
            parser.take(Token::LeftParen)?;
            using_expression = Some(parser.check_or()?);
            parser.take(Token::RightParen)?;
        } else {
            if with_check_expression.is_some() {
                return Err(DdlSqlError::InvalidStatement);
            }
            parser.word("with")?;
            parser.word("check")?;
            parser.take(Token::LeftParen)?;
            with_check_expression = Some(parser.check_or()?);
            parser.take(Token::RightParen)?;
        }
    }
    finish_statement(&mut parser)?;
    let required_clauses = match operation {
        "select" | "delete" => using_expression.is_some() && with_check_expression.is_none(),
        "insert" => using_expression.is_none() && with_check_expression.is_some(),
        "update" => using_expression.is_some() && with_check_expression.is_some(),
        _ => false,
    };
    if recipients.is_empty()
        || recipients.len() > crate::authorization::MAX_POLICY_SUBJECTS
        || recipients
            .iter()
            .map(|recipient| (recipient.is_role, &recipient.name))
            .collect::<std::collections::HashSet<_>>()
            .len()
            != recipients.len()
        || (is_cell && (columns.is_empty() || has_duplicates(&columns)))
        || (!is_cell && !columns.is_empty())
        || (is_cell && operation == "delete")
        || !required_clauses
    {
        return Err(DdlSqlError::InvalidStatement);
    }
    Ok(CreatePolicySql {
        replace_existing,
        schema,
        table,
        name,
        is_cell,
        columns,
        operation: operation.to_owned(),
        recipients,
        effect: effect.to_owned(),
        using_expression,
        with_check_expression,
    })
}

pub fn parse_drop_policy(sql: &str) -> Result<DropPolicySql, DdlSqlError> {
    let mut parser = statement_parser(sql)?;
    parser.word("drop")?;
    let is_cell = if parser.peek_word("cell") {
        parser.word("cell")?;
        true
    } else {
        parser.word("row")?;
        false
    };
    parser.word("policy")?;
    let if_exists = if parser.peek_word("if") {
        parser.word("if")?;
        parser.word("exists")?;
        true
    } else {
        false
    };
    let name = parser.identifier()?;
    parser.word("on")?;
    let schema = parser.identifier()?;
    parser.take(Token::Dot)?;
    let table = parser.identifier()?;
    parser.word("restrict")?;
    finish_statement(&mut parser)?;
    Ok(DropPolicySql {
        schema,
        table,
        name,
        is_cell,
        if_exists,
    })
}

/// Parse `ALTER TABLE schema.table ADD [CONSTRAINT name] CHECK (expression)` using a bounded
/// Boolean grammar over columns, typed comparisons, literals, and NULL tests.
pub fn parse_add_check_constraint(sql: &str) -> Result<AddCheckConstraintSql, DdlSqlError> {
    let mut parser = statement_parser(sql)?;
    parser.word("alter")?;
    parser.word("table")?;
    let schema = parser.identifier()?;
    parser.take(Token::Dot)?;
    let table = parser.identifier()?;
    parser.word("add")?;
    let name = if parser.peek_word("constraint") {
        parser.word("constraint")?;
        Some(parser.identifier()?)
    } else {
        None
    };
    let expression = parser.check_constraint_expression()?;
    finish_statement(&mut parser)?;
    Ok(AddCheckConstraintSql {
        schema,
        table,
        name,
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

fn validate_column_check_expression(
    column: &str,
    expression: &CheckExpressionSql,
) -> Result<(), DdlSqlError> {
    fn only_column(column: &str, expression: &CheckExpressionSql) -> bool {
        match expression {
            CheckExpressionSql::Column(name) => name == column,
            CheckExpressionSql::IsNull(expression) | CheckExpressionSql::IsNotNull(expression) => {
                only_column(column, expression)
            }
            CheckExpressionSql::BooleanTest { expression, .. } => only_column(column, expression),
            CheckExpressionSql::Literal(_) => true,
            CheckExpressionSql::CurrentPrincipalId | CheckExpressionSql::HasRole(_) => false,
            CheckExpressionSql::Compare { left, right, .. } => {
                only_column(column, left) && only_column(column, right)
            }
            CheckExpressionSql::Add(left, right)
            | CheckExpressionSql::Subtract(left, right)
            | CheckExpressionSql::Multiply(left, right)
            | CheckExpressionSql::Modulo(left, right)
            | CheckExpressionSql::Divide(left, right) => {
                only_column(column, left) && only_column(column, right)
            }
            CheckExpressionSql::Negate(expression) => only_column(column, expression),
            CheckExpressionSql::In { expression, .. }
            | CheckExpressionSql::Between { expression, .. } => only_column(column, expression),
            CheckExpressionSql::DistinctFrom {
                expression, right, ..
            } => only_column(column, expression) && only_column(column, right),
            CheckExpressionSql::Not(inner) => only_column(column, inner),
            CheckExpressionSql::And(items) | CheckExpressionSql::Or(items) => {
                items.iter().all(|item| only_column(column, item))
            }
        }
    }

    if only_column(column, expression) {
        Ok(())
    } else {
        Err(DdlSqlError::InvalidStatement)
    }
}

/// Bind SQL CHECK names and context-typed literals to a row-local EIR1 tree.
pub fn bind_check_expression(
    expression: &CheckExpressionSql,
    lookup: impl Fn(&str) -> Option<(usize, ScalarType)>,
) -> Result<crate::authorization::Expr, DdlSqlError> {
    fn has_policy_context(expression: &CheckExpressionSql) -> bool {
        match expression {
            CheckExpressionSql::CurrentPrincipalId | CheckExpressionSql::HasRole(_) => true,
            CheckExpressionSql::Compare { left, right, .. } => {
                has_policy_context(left) || has_policy_context(right)
            }
            CheckExpressionSql::Add(left, right)
            | CheckExpressionSql::Subtract(left, right)
            | CheckExpressionSql::Multiply(left, right)
            | CheckExpressionSql::Modulo(left, right)
            | CheckExpressionSql::Divide(left, right) => {
                has_policy_context(left) || has_policy_context(right)
            }
            CheckExpressionSql::Negate(expression) => has_policy_context(expression),
            CheckExpressionSql::In { expression, .. }
            | CheckExpressionSql::Between { expression, .. } => has_policy_context(expression),
            CheckExpressionSql::DistinctFrom {
                expression, right, ..
            } => has_policy_context(expression) || has_policy_context(right),
            CheckExpressionSql::Not(inner) => has_policy_context(inner),
            CheckExpressionSql::And(items) | CheckExpressionSql::Or(items) => {
                items.iter().any(has_policy_context)
            }
            CheckExpressionSql::BooleanTest { expression, .. } => has_policy_context(expression),
            CheckExpressionSql::IsNull(expression) | CheckExpressionSql::IsNotNull(expression) => {
                has_policy_context(expression)
            }
            CheckExpressionSql::Column(_) | CheckExpressionSql::Literal(_) => false,
        }
    }
    if has_policy_context(expression) {
        return Err(DdlSqlError::InvalidStatement);
    }
    let mut no_roles = |_: &str| None;
    let expression = bind_expression(expression, &lookup, &mut no_roles)?;
    crate::expression::encode_expression(&expression).map_err(|_| DdlSqlError::InvalidStatement)?;
    Ok(expression)
}

pub fn bind_policy_expression(
    expression: &CheckExpressionSql,
    lookup: impl Fn(&str) -> Option<(usize, ScalarType)>,
    role_lookup: impl FnMut(&str) -> Option<[u8; 16]>,
) -> Result<crate::authorization::Expr, DdlSqlError> {
    let mut role_lookup = role_lookup;
    let expression = bind_expression(expression, &lookup, &mut role_lookup)?;
    crate::expression::encode_expression(&expression).map_err(|_| DdlSqlError::InvalidStatement)?;
    Ok(expression)
}

fn bind_expression(
    expression: &CheckExpressionSql,
    lookup: &impl Fn(&str) -> Option<(usize, ScalarType)>,
    role_lookup: &mut impl FnMut(&str) -> Option<[u8; 16]>,
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
            CheckExpressionSql::CurrentPrincipalId => Some(ScalarType::Uuid),
            CheckExpressionSql::HasRole(_) => Some(ScalarType::Boolean),
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
            | CheckExpressionSql::BooleanTest { .. }
            | CheckExpressionSql::In { .. }
            | CheckExpressionSql::Between { .. }
            | CheckExpressionSql::DistinctFrom { .. }
            | CheckExpressionSql::Compare { .. }
            | CheckExpressionSql::Not(_)
            | CheckExpressionSql::And(_)
            | CheckExpressionSql::Or(_) => Some(ScalarType::Boolean),
            CheckExpressionSql::Literal(CheckLiteralSql::Null) => None,
            CheckExpressionSql::Add(left, right)
            | CheckExpressionSql::Subtract(left, right)
            | CheckExpressionSql::Multiply(left, right)
            | CheckExpressionSql::Modulo(left, right)
            | CheckExpressionSql::Divide(left, right) => {
                let left_hint = comparison_hint(left, lookup);
                let right_hint = comparison_hint(right, lookup);
                if matches!(left.as_ref(), CheckExpressionSql::Literal(_)) {
                    right_hint.or(left_hint)
                } else {
                    left_hint.or(right_hint)
                }
            }
            CheckExpressionSql::Negate(expression) => comparison_hint(expression, lookup),
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
        role_lookup: &mut impl FnMut(&str) -> Option<[u8; 16]>,
    ) -> Result<(crate::authorization::Expr, Option<ScalarType>), DdlSqlError> {
        use crate::authorization::Expr;
        use CheckExpressionSql as Sql;
        let bound = match expression {
            Sql::Column(name) => {
                let (ordinal, scalar_type) = lookup(name).ok_or(DdlSqlError::InvalidStatement)?;
                (Expr::Field(ordinal), Some(scalar_type))
            }
            Sql::CurrentPrincipalId => (Expr::CurrentPrincipalId, Some(ScalarType::Uuid)),
            Sql::HasRole(name) => (
                Expr::HasRole(role_lookup(name).ok_or(DdlSqlError::InvalidStatement)?),
                Some(ScalarType::Boolean),
            ),
            Sql::Add(left, right)
            | Sql::Subtract(left, right)
            | Sql::Multiply(left, right)
            | Sql::Modulo(left, right)
            | Sql::Divide(left, right) => {
                let left_is_null_literal =
                    matches!(left.as_ref(), Sql::Literal(CheckLiteralSql::Null));
                let right_is_null_literal =
                    matches!(right.as_ref(), Sql::Literal(CheckLiteralSql::Null));
                let (left, right, left_type, right_type) =
                    if matches!(left.as_ref(), Sql::Literal(_)) {
                        let (right, right_type) = bind(right, None, lookup, role_lookup)?;
                        let (left, left_type) = bind(left, right_type, lookup, role_lookup)?;
                        (left, right, left_type, right_type)
                    } else {
                        let (left, left_type) = bind(left, None, lookup, role_lookup)?;
                        let (right, right_type) = bind(right, left_type, lookup, role_lookup)?;
                        (left, right, left_type, right_type)
                    };
                let arithmetic_type = match (left_type, right_type) {
                    (Some(left), Some(right)) if left == right => Some(left),
                    (None, Some(right)) if left_is_null_literal => Some(right),
                    (Some(left), None) if right_is_null_literal => Some(left),
                    _ => None,
                };
                if !matches!(
                    arithmetic_type,
                    Some(ScalarType::SmallInt | ScalarType::Integer | ScalarType::BigInt)
                ) {
                    return Err(DdlSqlError::InvalidStatement);
                }
                let expression = match expression {
                    Sql::Add(_, _) => Expr::Add(Box::new(left), Box::new(right)),
                    Sql::Subtract(_, _) => Expr::Subtract(Box::new(left), Box::new(right)),
                    Sql::Multiply(_, _) => Expr::Multiply(Box::new(left), Box::new(right)),
                    Sql::Modulo(_, _) => Expr::Modulo(Box::new(left), Box::new(right)),
                    Sql::Divide(_, _) => Expr::Divide(Box::new(left), Box::new(right)),
                    _ => unreachable!(),
                };
                (expression, arithmetic_type)
            }
            Sql::Negate(inner) => {
                let (inner, scalar_type) = bind(
                    inner,
                    expected.or_else(|| comparison_hint(inner, lookup)),
                    lookup,
                    role_lookup,
                )?;
                let null_literal = matches!(inner, Expr::Literal(None));
                if !null_literal
                    && !matches!(
                        scalar_type,
                        Some(ScalarType::SmallInt | ScalarType::Integer | ScalarType::BigInt)
                    )
                {
                    return Err(DdlSqlError::InvalidStatement);
                }
                (Expr::Negate(Box::new(inner)), scalar_type)
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
            Sql::IsNull(value) | Sql::IsNotNull(value) => {
                let (value, _) = bind(value, None, lookup, role_lookup)?;
                let is_null = Expr::IsNull(Box::new(value));
                let expression = if matches!(expression, Sql::IsNotNull(_)) {
                    Expr::Not(Box::new(is_null))
                } else {
                    is_null
                };
                (expression, Some(ScalarType::Boolean))
            }
            Sql::BooleanTest {
                expression,
                test,
                negated,
            } => {
                let (value, scalar_type) =
                    bind(expression, Some(ScalarType::Boolean), lookup, role_lookup)?;
                if scalar_type != Some(ScalarType::Boolean) {
                    return Err(DdlSqlError::InvalidStatement);
                }
                let expression = match test {
                    CheckBooleanTest::Unknown => Expr::IsNull(Box::new(value)),
                    CheckBooleanTest::True | CheckBooleanTest::False => {
                        let is_known = Expr::Not(Box::new(Expr::IsNull(Box::new(value.clone()))));
                        let expected = *test == CheckBooleanTest::True;
                        let equals_expected = Expr::Equal(
                            Box::new(value),
                            Box::new(Expr::Literal(Some(ScalarValue::Boolean(expected)))),
                        );
                        Expr::And(vec![is_known, equals_expected])
                    }
                };
                let expression = if *negated {
                    Expr::Not(Box::new(expression))
                } else {
                    expression
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
                let (left, left_type) = bind(left, right_hint, lookup, role_lookup)?;
                let (right, right_type) = bind(
                    right,
                    left_hint.or(right_hint).or(left_type),
                    lookup,
                    role_lookup,
                )?;
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
            Sql::In {
                expression,
                values,
                negated,
            } => {
                let (left, column_type) = bind(expression, None, lookup, role_lookup)?;
                let column_type = column_type.ok_or(DdlSqlError::InvalidStatement)?;
                let mut alternatives = Vec::with_capacity(values.len());
                for value in values {
                    let (value, value_type) = bind(value, Some(column_type), lookup, role_lookup)?;
                    if value_type
                        .is_some_and(|value_type| !scalar_compatible(column_type, value_type))
                    {
                        return Err(DdlSqlError::InvalidStatement);
                    }
                    alternatives.push(Expr::Equal(Box::new(left.clone()), Box::new(value)));
                }
                let expression = Expr::Or(alternatives);
                let expression = if *negated {
                    Expr::Not(Box::new(expression))
                } else {
                    expression
                };
                (expression, Some(ScalarType::Boolean))
            }
            Sql::Between {
                expression,
                lower,
                upper,
                negated,
            } => {
                let (left, column_type) = bind(expression, None, lookup, role_lookup)?;
                let column_type = column_type.ok_or(DdlSqlError::InvalidStatement)?;
                let bind_bound = |bound_expression: &CheckExpressionSql,
                                  role_lookup: &mut _|
                 -> Result<Expr, DdlSqlError> {
                    let (bound, bound_type) =
                        bind(bound_expression, Some(column_type), lookup, role_lookup)?;
                    if bound_type
                        .is_some_and(|bound_type| !scalar_compatible(column_type, bound_type))
                    {
                        return Err(DdlSqlError::InvalidStatement);
                    }
                    Ok(bound)
                };
                let lower = bind_bound(lower, role_lookup)?;
                let upper = bind_bound(upper, role_lookup)?;
                let expression = Expr::And(vec![
                    Expr::GreaterEqual(Box::new(left.clone()), Box::new(lower)),
                    Expr::LessEqual(Box::new(left), Box::new(upper)),
                ]);
                let expression = if *negated {
                    Expr::Not(Box::new(expression))
                } else {
                    expression
                };
                (expression, Some(ScalarType::Boolean))
            }
            Sql::DistinctFrom {
                expression,
                right,
                negated,
            } => {
                let right_is_literal = matches!(right.as_ref(), Sql::Literal(_));
                let (left_hint, right_hint) =
                    if let Some(scalar_type) = common_numeric_literal_type(expression, right) {
                        (Some(scalar_type), Some(scalar_type))
                    } else {
                        (
                            comparison_hint(expression, lookup),
                            comparison_hint(right, lookup),
                        )
                    };
                let (left, left_type) = bind(expression, right_hint, lookup, role_lookup)?;
                let (right, right_type) = bind(
                    right,
                    left_hint.or(right_hint).or(left_type),
                    lookup,
                    role_lookup,
                )?;
                if let (Some(left_type), Some(right_type)) = (left_type, right_type)
                    && !scalar_compatible(left_type, right_type)
                {
                    return Err(DdlSqlError::InvalidStatement);
                }
                let expression = if right_type.is_none() {
                    let is_null = Expr::IsNull(Box::new(left));
                    if *negated {
                        is_null
                    } else {
                        Expr::Not(Box::new(is_null))
                    }
                } else {
                    let distinct = if right_is_literal {
                        Expr::Or(vec![
                            Expr::IsNull(Box::new(left.clone())),
                            Expr::NotEqual(Box::new(left), Box::new(right)),
                        ])
                    } else {
                        let left_is_null = Expr::IsNull(Box::new(left.clone()));
                        let right_is_null = Expr::IsNull(Box::new(right.clone()));
                        let left_is_not_null = Expr::Not(Box::new(left_is_null.clone()));
                        let right_is_not_null = Expr::Not(Box::new(right_is_null.clone()));
                        Expr::Or(vec![
                            Expr::And(vec![left_is_null, right_is_not_null.clone()]),
                            Expr::And(vec![left_is_not_null.clone(), right_is_null]),
                            Expr::And(vec![
                                left_is_not_null,
                                right_is_not_null,
                                Expr::NotEqual(Box::new(left), Box::new(right)),
                            ]),
                        ])
                    };
                    if *negated {
                        Expr::Not(Box::new(distinct))
                    } else {
                        distinct
                    }
                };
                (expression, Some(ScalarType::Boolean))
            }
            Sql::Not(inner) => {
                let (inner, scalar_type) =
                    bind(inner, Some(ScalarType::Boolean), lookup, role_lookup)?;
                if scalar_type.is_some_and(|scalar_type| scalar_type != ScalarType::Boolean) {
                    return Err(DdlSqlError::InvalidStatement);
                }
                (Expr::Not(Box::new(inner)), Some(ScalarType::Boolean))
            }
            Sql::And(items) | Sql::Or(items) => {
                let mut expressions = Vec::with_capacity(items.len());
                for item in items {
                    let (item, scalar_type) =
                        bind(item, Some(ScalarType::Boolean), lookup, role_lookup)?;
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

    let (expression, scalar_type) =
        bind(expression, Some(ScalarType::Boolean), lookup, role_lookup)?;
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

/// Parse the bounded, lossless `VARCHAR` widening form of `ALTER COLUMN TYPE`.
pub fn parse_widen_varchar(sql: &str) -> Result<WidenVarcharSql, DdlSqlError> {
    let mut parser = statement_parser(sql)?;
    parser.word("alter")?;
    parser.word("table")?;
    let schema = parser.identifier()?;
    parser.take(Token::Dot)?;
    let table = parser.identifier()?;
    parser.word("alter")?;
    parser.word("column")?;
    let column = parser.identifier()?;
    parser.word("type")?;
    parser.word("varchar")?;
    parser.take(Token::LeftParen)?;
    let max_chars = parser.number()?;
    parser.take(Token::RightParen)?;
    if max_chars == 0 {
        return Err(DdlSqlError::InvalidStatement);
    }
    finish_statement(&mut parser)?;
    Ok(WidenVarcharSql {
        schema,
        table,
        column,
        max_chars,
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
    let if_not_exists = if parser.peek_word("if") {
        parser.word("if")?;
        parser.word("not")?;
        parser.word("exists")?;
        true
    } else {
        false
    };
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
        if_not_exists,
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
    let if_exists = if parser.peek_word("if") {
        parser.word("if")?;
        parser.word("exists")?;
        true
    } else {
        false
    };
    parser.word("column")?;
    let column = parser.identifier()?;
    parser.word("restrict")?;
    finish_statement(&mut parser)?;
    Ok(DropColumnSql {
        schema,
        table,
        column,
        if_exists,
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
            if self.peek_word("is")
                || self.peek_word("between")
                || self.peek_word("in")
                || matches!(
                    self.peek(),
                    Some(Token::Plus | Token::Minus | Token::Star | Token::Percent | Token::Slash)
                )
                || (self.peek_word("not")
                    && matches!(self.tokens.get(self.position + 1), Some(Token::Word(word)) if word == "between" || word == "in"))
                || matches!(
                    self.peek(),
                    Some(
                        Token::Equal
                            | Token::NotEqual
                            | Token::Less
                            | Token::LessEqual
                            | Token::Greater
                            | Token::GreaterEqual
                    )
                )
            {
                let expression = self.check_value_expression_tail(expression)?;
                return self.check_value_predicate(expression);
            }
            return Ok(expression);
        }
        self.check_predicate()
    }

    fn check_predicate(&mut self) -> Result<CheckExpressionSql, DdlSqlError> {
        let left = self.check_value_expression()?;
        self.check_value_predicate(left)
    }

    fn check_value_expression(&mut self) -> Result<CheckExpressionSql, DdlSqlError> {
        let expression = self.check_value_term()?;
        self.check_value_expression_tail(expression)
    }

    fn check_value_expression_tail(
        &mut self,
        expression: CheckExpressionSql,
    ) -> Result<CheckExpressionSql, DdlSqlError> {
        let mut expression = self.check_value_term_tail(expression)?;
        loop {
            let constructor = match self.peek() {
                Some(Token::Plus) => CheckExpressionSql::Add,
                Some(Token::Minus) => CheckExpressionSql::Subtract,
                _ => break,
            };
            self.position += 1;
            let right = self.check_value_term()?;
            expression = constructor(Box::new(expression), Box::new(right));
        }
        Ok(expression)
    }

    fn check_value_term(&mut self) -> Result<CheckExpressionSql, DdlSqlError> {
        let expression = self.check_value_atom()?;
        self.check_value_term_tail(expression)
    }

    fn check_value_term_tail(
        &mut self,
        mut expression: CheckExpressionSql,
    ) -> Result<CheckExpressionSql, DdlSqlError> {
        while matches!(
            self.peek(),
            Some(Token::Star | Token::Percent | Token::Slash)
        ) {
            let modulo = self.peek() == Some(&Token::Percent);
            let divide = self.peek() == Some(&Token::Slash);
            self.position += 1;
            let right = self.check_value_atom()?;
            expression = if modulo {
                CheckExpressionSql::Modulo(Box::new(expression), Box::new(right))
            } else if divide {
                CheckExpressionSql::Divide(Box::new(expression), Box::new(right))
            } else {
                CheckExpressionSql::Multiply(Box::new(expression), Box::new(right))
            };
        }
        Ok(expression)
    }

    fn check_value_predicate(
        &mut self,
        left: CheckExpressionSql,
    ) -> Result<CheckExpressionSql, DdlSqlError> {
        if self.peek_word("is") {
            self.check_is_predicate(left)
        } else if self.peek_word("between")
            || (self.peek_word("not")
                && matches!(self.tokens.get(self.position + 1), Some(Token::Word(word)) if word == "between"))
        {
            let negated = if self.peek_word("not") {
                self.position += 1;
                true
            } else {
                false
            };
            self.word("between")?;
            let lower = self.check_value_expression()?;
            self.word("and")?;
            let upper = self.check_value_expression()?;
            Ok(CheckExpressionSql::Between {
                expression: Box::new(left),
                lower: Box::new(lower),
                upper: Box::new(upper),
                negated,
            })
        } else if self.peek_word("in")
            || (self.peek_word("not")
                && matches!(self.tokens.get(self.position + 1), Some(Token::Word(word)) if word == "in"))
        {
            let negated = if self.peek_word("not") {
                self.position += 1;
                true
            } else {
                false
            };
            self.word("in")?;
            self.take(Token::LeftParen)?;
            let mut values = Vec::new();
            loop {
                if values.len() == MAX_CHECK_IN_VALUES {
                    return Err(DdlSqlError::InvalidStatement);
                }
                values.push(Box::new(self.check_value_expression()?));
                if self.peek() == Some(&Token::RightParen) {
                    break;
                }
                self.take(Token::Comma)?;
            }
            if values.is_empty() {
                return Err(DdlSqlError::InvalidStatement);
            }
            self.take(Token::RightParen)?;
            Ok(CheckExpressionSql::In {
                expression: Box::new(left),
                values,
                negated,
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
                let right = if self.peek() == Some(&Token::LeftParen) {
                    self.check_unary()?
                } else {
                    self.check_value_expression()?
                };
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

    fn check_is_predicate(
        &mut self,
        left: CheckExpressionSql,
    ) -> Result<CheckExpressionSql, DdlSqlError> {
        self.word("is")?;
        let negated = if self.peek_word("not") {
            self.position += 1;
            true
        } else {
            false
        };
        if self.peek_word("distinct") {
            self.word("distinct")?;
            self.word("from")?;
            let right = self.check_unary()?;
            Ok(CheckExpressionSql::DistinctFrom {
                expression: Box::new(left),
                right: Box::new(right),
                negated,
            })
        } else if self.peek_word("true") || self.peek_word("false") || self.peek_word("unknown") {
            let test = if self.peek_word("true") {
                CheckBooleanTest::True
            } else if self.peek_word("false") {
                CheckBooleanTest::False
            } else {
                CheckBooleanTest::Unknown
            };
            self.position += 1;
            Ok(CheckExpressionSql::BooleanTest {
                expression: Box::new(left),
                test,
                negated,
            })
        } else {
            self.word("null")?;
            Ok(if negated {
                CheckExpressionSql::IsNotNull(Box::new(left))
            } else {
                CheckExpressionSql::IsNull(Box::new(left))
            })
        }
    }

    fn check_value_atom(&mut self) -> Result<CheckExpressionSql, DdlSqlError> {
        if self.peek() == Some(&Token::Minus)
            && !matches!(self.tokens.get(self.position + 1), Some(Token::Number(_)))
        {
            self.position += 1;
            return Ok(CheckExpressionSql::Negate(Box::new(
                self.check_value_atom()?,
            )));
        }
        if self.peek() == Some(&Token::LeftParen) {
            self.position += 1;
            let expression = self.check_value_expression()?;
            self.take(Token::RightParen)?;
            return Ok(expression);
        }
        if self.peek_word("current_principal_id") {
            self.position += 1;
            self.take(Token::LeftParen)?;
            self.take(Token::RightParen)?;
            return Ok(CheckExpressionSql::CurrentPrincipalId);
        }
        if self.peek_word("has_role") {
            self.position += 1;
            self.take(Token::LeftParen)?;
            let role_name = match self.peek() {
                Some(Token::String(name)) => name.clone(),
                _ => return Err(DdlSqlError::InvalidStatement),
            };
            self.position += 1;
            self.take(Token::RightParen)?;
            return Ok(CheckExpressionSql::HasRole(role_name));
        }
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
            b'*' => {
                tokens.push(Token::Star);
                cursor += 1;
            }
            b'%' => {
                tokens.push(Token::Percent);
                cursor += 1;
            }
            b'/' => {
                tokens.push(Token::Slash);
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
            parsed.owner,
            CreateSchemaOwnerSql::Id([
                0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd,
                0xee, 0xff,
            ])
        );
        assert_eq!(
            parse_create_schema("CREATE SCHEMA app;").unwrap().owner,
            CreateSchemaOwnerSql::CurrentUser
        );
        let conditional =
            parse_create_schema("CREATE SCHEMA IF NOT EXISTS app AUTHORIZATION ROLE app_admin;")
                .unwrap();
        assert!(conditional.if_not_exists);
        assert_eq!(conditional.schema, "app");
        assert_eq!(
            conditional.owner,
            CreateSchemaOwnerSql::Identity {
                name: "app_admin".into(),
                kind: SqlIdentityKind::Role,
            }
        );
        assert_eq!(
            parse_create_schema("CREATE SCHEMA app AUTHORIZATION USER app_owner")
                .unwrap()
                .owner,
            CreateSchemaOwnerSql::Identity {
                name: "app_owner".into(),
                kind: SqlIdentityKind::User,
            }
        );
        assert_eq!(
            parse_create_schema("CREATE SCHEMA app AUTHORIZATION ROLE app_admin")
                .unwrap()
                .owner,
            CreateSchemaOwnerSql::Identity {
                name: "app_admin".into(),
                kind: SqlIdentityKind::Role,
            }
        );
        assert_eq!(
            parse_create_schema("CREATE SCHEMA app AUTHORIZATION app_owner")
                .unwrap()
                .owner,
            CreateSchemaOwnerSql::Identity {
                name: "app_owner".into(),
                kind: SqlIdentityKind::Either,
            }
        );
        for sql in [
            "CREATE SCHEMA app AUTHORIZATION GROUP app_owner",
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
            DropSchemaSql {
                schema: "app".into(),
                if_exists: false,
            }
        );
        assert_eq!(
            parse_drop_schema("DROP SCHEMA IF EXISTS app RESTRICT;").unwrap(),
            DropSchemaSql {
                schema: "app".into(),
                if_exists: true,
            }
        );
        for sql in [
            "ALTER SCHEMA app RENAME application",
            "ALTER SCHEMA app RENAME TO \"Mixed\"",
            "DROP SCHEMA app",
            "DROP SCHEMA app CASCADE",
            "DROP SCHEMA IF app RESTRICT",
            "DROP SCHEMA IF NOT EXISTS app RESTRICT",
            "DROP SCHEMA app RESTRICT; DROP SCHEMA b RESTRICT",
        ] {
            assert!(
                parse_rename_schema(sql).is_err() && parse_drop_schema(sql).is_err(),
                "accepted {sql}"
            );
        }
        assert_eq!(
            parse_schema_owner_transfer("ALTER SCHEMA app OWNER TO USER owner_name").unwrap(),
            OwnerTransferSql {
                schema: "app".into(),
                table: None,
                owner_name: "owner_name".into(),
                owner_kind: SqlIdentityKind::User,
            }
        );
        assert_eq!(
            parse_schema_owner_transfer("ALTER SCHEMA app OWNER TO owner_name").unwrap(),
            OwnerTransferSql {
                schema: "app".into(),
                table: None,
                owner_name: "owner_name".into(),
                owner_kind: SqlIdentityKind::Either,
            }
        );
        let invalid = "ALTER SCHEMA app OWNER TO USER owner_name CASCADE";
        assert!(
            parse_schema_owner_transfer(invalid).is_err(),
            "accepted {invalid}"
        );
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
            DropTableSql {
                schema: "app".into(),
                table: "events".into(),
                if_exists: false,
            }
        );
        assert_eq!(
            parse_drop_table("DROP TABLE IF EXISTS app.events RESTRICT;").unwrap(),
            DropTableSql {
                schema: "app".into(),
                table: "events".into(),
                if_exists: true,
            }
        );
        assert_eq!(
            parse_truncate_table("TRUNCATE TABLE app.events RESTRICT;").unwrap(),
            TruncateTableSql {
                schema: "app".into(),
                table: "events".into(),
            }
        );
        for sql in [
            "ALTER TABLE events RENAME TO archived",
            "ALTER TABLE app.events RENAME archived",
            "DROP TABLE app.events",
            "DROP TABLE app.events CASCADE",
            "DROP TABLE IF app.events RESTRICT",
            "DROP TABLE IF NOT EXISTS app.events RESTRICT",
            "DROP TABLE app.events RESTRICT; DROP TABLE app.other RESTRICT",
        ] {
            assert!(
                parse_rename_table(sql).is_err() && parse_drop_table(sql).is_err(),
                "accepted {sql}"
            );
        }
        for sql in [
            "TRUNCATE app.events RESTRICT",
            "TRUNCATE TABLE events RESTRICT",
            "TRUNCATE TABLE app.events",
            "TRUNCATE TABLE app.events CASCADE",
            "TRUNCATE TABLE app.events RESTRICT; TRUNCATE TABLE app.other RESTRICT",
        ] {
            assert!(parse_truncate_table(sql).is_err(), "accepted {sql}");
        }
        assert_eq!(
            parse_table_owner_transfer("ALTER TABLE app.events OWNER TO ROLE owners;").unwrap(),
            OwnerTransferSql {
                schema: "app".into(),
                table: Some("events".into()),
                owner_name: "owners".into(),
                owner_kind: SqlIdentityKind::Role,
            }
        );
        for sql in [
            "ALTER TABLE app.events OWNER TO owners",
            "ALTER TABLE events OWNER TO ROLE owners",
            "ALTER TABLE app.events OWNER TO ROLE owners CASCADE",
        ] {
            assert!(parse_table_owner_transfer(sql).is_err(), "accepted {sql}");
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
                name: None,
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
        assert_eq!(
            parse_add_key_constraint(
                "ALTER TABLE app.events ADD CONSTRAINT events_key UNIQUE (id)"
            )
            .unwrap()
            .name
            .as_deref(),
            Some("events_key")
        );
        for sql in [
            "ALTER TABLE events ADD UNIQUE (id)",
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
                name: None,
                columns: vec!["tenant_id".into(), "event_id".into()],
                referenced_schema: "core".into(),
                referenced_table: "tenants".into(),
                referenced_columns: vec!["tenant_id".into(), "id".into()],
            }
        );
        assert_eq!(
            parse_add_foreign_key(
                "ALTER TABLE app.events ADD CONSTRAINT events_tenant_fk FOREIGN KEY (tenant_id) REFERENCES core.tenants (id)"
            )
            .unwrap()
            .name
            .as_deref(),
            Some("events_tenant_fk")
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
                constraint_name: None,
                if_exists: false,
            }
        );
        assert!(parse_drop_constraint(
            "ALTER TABLE app.orders DROP CONSTRAINT IF EXISTS '01234567-89ab-cdef-0123-456789abcdef'"
        )
        .unwrap()
        .if_exists);
        for sql in [
            "ALTER TABLE orders DROP CONSTRAINT '0123456789abcdef0123456789abcdef'",
            "ALTER TABLE app.orders DROP CONSTRAINT '00000000000000000000000000000000'",
        ] {
            assert!(parse_drop_constraint(sql).is_err(), "accepted {sql}");
        }
        let named = parse_drop_constraint(
            "ALTER TABLE app.orders DROP CONSTRAINT IF EXISTS fk_orders_customer",
        )
        .unwrap();
        assert_eq!(named.constraint_name.as_deref(), Some("fk_orders_customer"));
        assert!(named.if_exists);
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
                    CheckExpressionSql::IsNull(Box::new(CheckExpressionSql::Column(
                        "deleted_at".into(),
                    ))),
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
    fn parses_named_create_table_check_constraints() {
        let parsed = parse_create_table(
            "CREATE TABLE app.events (id INTEGER, label TEXT CONSTRAINT label_nonempty CHECK (label IS NOT NULL), CONSTRAINT id_positive CHECK (id > 0));",
        )
        .unwrap();
        assert_eq!(
            parsed.check_constraint_names,
            vec![Some("label_nonempty".into()), Some("id_positive".into())]
        );
        assert_eq!(parsed.check_constraints.len(), 2);
        for sql in [
            "CREATE TABLE app.events (id INTEGER, CONSTRAINT repeated CHECK (id > 0), CONSTRAINT repeated CHECK (id < 10))",
            "CREATE TABLE app.events (id INTEGER, CONSTRAINT 9bad CHECK (id > 0))",
        ] {
            assert!(parse_create_table(sql).is_err(), "accepted {sql}");
        }
    }

    #[test]
    fn parses_named_create_table_key_and_foreign_key_constraints() {
        let parsed = parse_create_table(
            "CREATE TABLE app.orders (id UUID CONSTRAINT orders_pk PRIMARY KEY, code TEXT CONSTRAINT orders_code_key UNIQUE, customer_id UUID, CONSTRAINT customer_fk FOREIGN KEY (customer_id) REFERENCES crm.customers (id), CONSTRAINT other_key UNIQUE (code, customer_id))",
        )
        .unwrap();
        assert_eq!(parsed.primary_key_name.as_deref(), Some("orders_pk"));
        assert_eq!(
            parsed.unique_constraint_names,
            vec![Some("orders_code_key".into()), Some("other_key".into())]
        );
        assert_eq!(parsed.foreign_key_names, vec![Some("customer_fk".into())]);
        assert_eq!(parsed.foreign_keys[0].name.as_deref(), Some("customer_fk"));

        for sql in [
            "CREATE TABLE app.t (id UUID CONSTRAINT same PRIMARY KEY, value TEXT CONSTRAINT same UNIQUE)",
            "CREATE TABLE app.t (id UUID, CONSTRAINT same UNIQUE (id), CONSTRAINT same FOREIGN KEY (id) REFERENCES app.x (id))",
        ] {
            assert!(parse_create_table(sql).is_err(), "accepted {sql}");
        }
    }

    #[test]
    fn parses_create_table_if_not_exists() {
        let parsed =
            parse_create_table("CREATE TABLE IF NOT EXISTS app.events (id UUID PRIMARY KEY);")
                .unwrap();
        assert!(parsed.if_not_exists);
        assert_eq!(parsed.schema, "app");
        assert_eq!(parsed.table, "events");
        assert_eq!(parsed.primary_key_columns, Some(vec!["id".into()]));
        assert!(
            !parse_create_table("CREATE TABLE app.events (id UUID)")
                .unwrap()
                .if_not_exists
        );
    }

    #[test]
    fn parses_column_level_unique_as_a_single_column_constraint() {
        let parsed = parse_create_table(
            "CREATE TABLE app.accounts (id UUID GENERATED ALWAYS AS IDENTITY, email TEXT UNIQUE NOT NULL, nickname TEXT DEFAULT 'guest', PRIMARY KEY (id));",
        )
        .unwrap();
        assert_eq!(parsed.primary_key_columns, Some(vec!["id".into()]));
        assert_eq!(parsed.unique_constraints, vec![vec![String::from("email")]]);
        assert!(!parsed.columns[1].nullable);
        assert_eq!(
            parsed.columns[2].default,
            Some(AdminColumnDefault::Literal(ScalarValue::Text(
                "guest".into()
            )))
        );

        for sql in [
            "CREATE TABLE app.accounts (email TEXT UNIQUE UNIQUE)",
            "CREATE TABLE app.accounts (email TEXT UNIQUE, UNIQUE (email))",
        ] {
            assert!(parse_create_table(sql).is_err(), "accepted {sql}");
        }
    }

    #[test]
    fn parses_column_level_primary_key_as_non_nullable() {
        let parsed = parse_create_table(
            "CREATE TABLE app.accounts (id UUID PRIMARY KEY, email TEXT UNIQUE);",
        )
        .unwrap();
        assert_eq!(parsed.primary_key_columns, Some(vec![String::from("id")]));
        assert!(!parsed.columns[0].nullable);
        assert_eq!(parsed.unique_constraints, vec![vec![String::from("email")]]);

        for sql in [
            "CREATE TABLE app.accounts (id INTEGER PRIMARY KEY, email INTEGER PRIMARY KEY)",
            "CREATE TABLE app.accounts (id INTEGER PRIMARY KEY, PRIMARY KEY (id))",
            "CREATE TABLE app.accounts (id INTEGER PRIMARY KEY PRIMARY KEY)",
        ] {
            assert!(parse_create_table(sql).is_err(), "accepted {sql}");
        }
    }

    #[test]
    fn parses_column_level_check_against_its_declared_column() {
        let parsed = parse_create_table(
            "CREATE TABLE app.payments (amount DECIMAL(8,2) CHECK (amount >= 0));",
        )
        .unwrap();
        assert_eq!(
            parsed.check_constraints,
            vec![CheckExpressionSql::Compare {
                operator: CheckComparisonOperator::GreaterEqual,
                left: Box::new(CheckExpressionSql::Column("amount".into())),
                right: Box::new(CheckExpressionSql::Literal(CheckLiteralSql::Number(
                    "0".into()
                ))),
            }]
        );
        assert!(parse_create_table(
            "CREATE TABLE app.payments (amount DECIMAL(8,2), other INTEGER, CHECK (amount >= 0));"
        )
        .is_ok());
        assert!(
            parse_create_table(
                "CREATE TABLE app.payments (amount DECIMAL(8,2) CHECK (other >= 0), other INTEGER)"
            )
            .is_err()
        );
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

        let grouped = parse_add_check_constraint(
            "ALTER TABLE app.events ADD CHECK (enabled = (priority >= 1))",
        )
        .unwrap();
        assert_eq!(
            bind_check_expression(&grouped.expression, |name| match name {
                "enabled" => Some((0, ScalarType::Boolean)),
                "priority" => Some((1, ScalarType::Integer)),
                _ => None,
            })
            .unwrap(),
            Expr::Equal(
                Box::new(Expr::Field(0)),
                Box::new(Expr::GreaterEqual(
                    Box::new(Expr::Field(1)),
                    Box::new(Expr::Literal(Some(ScalarValue::Integer(1)))),
                )),
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
    fn parses_and_binds_integer_check_arithmetic_with_precedence() {
        use crate::authorization::Expr;

        let parsed = parse_add_check_constraint(
            "ALTER TABLE app.inventory ADD CHECK (quantity * 2 + reserved <= capacity)",
        )
        .unwrap();
        assert_eq!(
            parsed.expression,
            CheckExpressionSql::Compare {
                operator: CheckComparisonOperator::LessEqual,
                left: Box::new(CheckExpressionSql::Add(
                    Box::new(CheckExpressionSql::Multiply(
                        Box::new(CheckExpressionSql::Column("quantity".into())),
                        Box::new(CheckExpressionSql::Literal(CheckLiteralSql::Number(
                            "2".into(),
                        ))),
                    )),
                    Box::new(CheckExpressionSql::Column("reserved".into())),
                )),
                right: Box::new(CheckExpressionSql::Column("capacity".into())),
            }
        );
        let bound = bind_check_expression(&parsed.expression, |name| match name {
            "quantity" => Some((0, ScalarType::Integer)),
            "reserved" => Some((1, ScalarType::Integer)),
            "capacity" => Some((2, ScalarType::Integer)),
            _ => None,
        })
        .unwrap();
        assert_eq!(
            bound,
            Expr::LessEqual(
                Box::new(Expr::Add(
                    Box::new(Expr::Multiply(
                        Box::new(Expr::Field(0)),
                        Box::new(Expr::Literal(Some(ScalarValue::Integer(2)))),
                    )),
                    Box::new(Expr::Field(1)),
                )),
                Box::new(Expr::Field(2)),
            )
        );
        let modulo =
            parse_add_check_constraint("ALTER TABLE app.inventory ADD CHECK (quantity % 2 = 0)")
                .unwrap();
        assert_eq!(
            bind_check_expression(&modulo.expression, |name| {
                (name == "quantity").then_some((0, ScalarType::Integer))
            })
            .unwrap(),
            Expr::Equal(
                Box::new(Expr::Modulo(
                    Box::new(Expr::Field(0)),
                    Box::new(Expr::Literal(Some(ScalarValue::Integer(2)))),
                )),
                Box::new(Expr::Literal(Some(ScalarValue::Integer(0)))),
            )
        );
        let divide = parse_add_check_constraint(
            "ALTER TABLE app.inventory ADD CHECK (quantity / 2 * reserved = capacity)",
        )
        .unwrap();
        assert_eq!(
            divide.expression,
            CheckExpressionSql::Compare {
                operator: CheckComparisonOperator::Equal,
                left: Box::new(CheckExpressionSql::Multiply(
                    Box::new(CheckExpressionSql::Divide(
                        Box::new(CheckExpressionSql::Column("quantity".into())),
                        Box::new(CheckExpressionSql::Literal(CheckLiteralSql::Number(
                            "2".into(),
                        ))),
                    )),
                    Box::new(CheckExpressionSql::Column("reserved".into())),
                )),
                right: Box::new(CheckExpressionSql::Column("capacity".into())),
            }
        );
        assert!(
            bind_check_expression(&divide.expression, |name| match name {
                "quantity" | "reserved" | "capacity" => Some((0, ScalarType::Integer)),
                _ => None,
            })
            .is_ok()
        );
        let negation =
            parse_add_check_constraint("ALTER TABLE app.inventory ADD CHECK (-quantity <= 0)")
                .unwrap();
        assert_eq!(
            bind_check_expression(&negation.expression, |name| {
                (name == "quantity").then_some((0, ScalarType::Integer))
            })
            .unwrap(),
            Expr::LessEqual(
                Box::new(Expr::Negate(Box::new(Expr::Field(0)))),
                Box::new(Expr::Literal(Some(ScalarValue::Integer(0)))),
            )
        );
        let null_negation =
            parse_add_check_constraint("ALTER TABLE app.inventory ADD CHECK (-NULL IS NULL)")
                .unwrap();
        assert!(bind_check_expression(&null_negation.expression, |_| None).is_ok());
        assert!(
            parse_add_check_constraint(
                "ALTER TABLE app.inventory ADD CHECK (quantity + 'two' <= capacity)"
            )
            .is_ok()
        );
        assert!(
            bind_check_expression(
                &parse_add_check_constraint(
                    "ALTER TABLE app.inventory ADD CHECK (quantity + 'two' <= capacity)"
                )
                .unwrap()
                .expression,
                |name| match name {
                    "quantity" | "capacity" => Some((0, ScalarType::Integer)),
                    _ => None,
                }
            )
            .is_err()
        );
    }

    #[test]
    fn parses_and_binds_check_in_predicates_with_typed_literals() {
        use crate::authorization::Expr;

        let parsed = parse_create_table(
            "CREATE TABLE app.jobs (state VARCHAR(8), CHECK (state IN ('new', 'running', 'done')))",
        )
        .unwrap();
        let expected = CheckExpressionSql::In {
            expression: Box::new(CheckExpressionSql::Column("state".into())),
            values: vec![
                Box::new(CheckExpressionSql::Literal(CheckLiteralSql::String(
                    "new".into(),
                ))),
                Box::new(CheckExpressionSql::Literal(CheckLiteralSql::String(
                    "running".into(),
                ))),
                Box::new(CheckExpressionSql::Literal(CheckLiteralSql::String(
                    "done".into(),
                ))),
            ],
            negated: false,
        };
        assert_eq!(parsed.check_constraints, vec![expected.clone()]);
        assert_eq!(
            bind_check_expression(&expected, |name| {
                (name == "state").then_some((0, ScalarType::Varchar { max_chars: 8 }))
            })
            .unwrap(),
            Expr::Or(vec![
                Expr::Equal(
                    Box::new(Expr::Field(0)),
                    Box::new(Expr::Literal(Some(ScalarValue::Varchar("new".into())))),
                ),
                Expr::Equal(
                    Box::new(Expr::Field(0)),
                    Box::new(Expr::Literal(Some(ScalarValue::Varchar("running".into())))),
                ),
                Expr::Equal(
                    Box::new(Expr::Field(0)),
                    Box::new(Expr::Literal(Some(ScalarValue::Varchar("done".into())))),
                ),
            ])
        );

        let negative = parse_add_check_constraint(
            "ALTER TABLE app.jobs ADD CHECK (priority NOT IN (1, 2, NULL))",
        )
        .unwrap();
        assert!(matches!(
            negative.expression,
            CheckExpressionSql::In {
                ref expression,
                ref values,
                negated: true
            } if expression.as_ref() == &CheckExpressionSql::Column("priority".into()) && values.len() == 3
        ));
        assert!(
            parse_create_table("CREATE TABLE app.jobs (state TEXT, CHECK (state IN ()))").is_err()
        );
        let literal =
            parse_add_check_constraint("ALTER TABLE app.jobs ADD CHECK ('ok' IN ('ok'))").unwrap();
        assert!(bind_check_expression(&literal.expression, |_| None).is_ok());
        let grouped = parse_add_check_constraint(
            "ALTER TABLE app.jobs ADD CHECK ((priority >= 1) IN (TRUE, FALSE))",
        )
        .unwrap();
        assert!(
            bind_check_expression(&grouped.expression, |name| {
                (name == "priority").then_some((0, ScalarType::Integer))
            })
            .is_ok()
        );
        let columns = parse_create_table(
            "CREATE TABLE app.jobs (state TEXT, previous_state TEXT, CHECK (state IN ('new', previous_state)))",
        )
        .unwrap();
        assert!(
            bind_check_expression(&columns.check_constraints[0], |name| match name {
                "state" => Some((0, ScalarType::Text)),
                "previous_state" => Some((1, ScalarType::Text)),
                _ => None,
            })
            .is_ok()
        );
        let repeated_values = vec!["TRUE"; MAX_CHECK_IN_VALUES].join(", ");
        let oversized = parse_add_check_constraint(&format!(
            "ALTER TABLE app.jobs ADD CHECK ((priority >= 1) IN ({repeated_values}))"
        ))
        .unwrap();
        assert!(
            bind_check_expression(&oversized.expression, |name| {
                (name == "priority").then_some((0, ScalarType::Integer))
            })
            .is_err()
        );
        assert!(
            bind_check_expression(&negative.expression, |name| {
                (name == "priority").then_some((0, ScalarType::Integer))
            })
            .is_ok()
        );
        assert!(
            bind_check_expression(&negative.expression, |name| {
                (name == "priority").then_some((0, ScalarType::Boolean))
            })
            .is_err()
        );
        for count in [MAX_CHECK_IN_VALUES, MAX_CHECK_IN_VALUES + 1] {
            let values = (0..count)
                .map(|value| value.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            let sql =
                format!("CREATE TABLE app.jobs (priority INTEGER, CHECK (priority IN ({values})))");
            assert_eq!(
                parse_create_table(&sql).is_ok(),
                count == MAX_CHECK_IN_VALUES
            );
        }
    }

    #[test]
    fn parses_and_binds_check_between_predicates_inclusively() {
        use crate::authorization::Expr;

        let parsed =
            parse_add_check_constraint("ALTER TABLE app.jobs ADD CHECK (priority BETWEEN 1 AND 9)")
                .unwrap();
        assert_eq!(
            parsed.expression,
            CheckExpressionSql::Between {
                expression: Box::new(CheckExpressionSql::Column("priority".into())),
                lower: Box::new(CheckExpressionSql::Literal(CheckLiteralSql::Number(
                    "1".into(),
                ))),
                upper: Box::new(CheckExpressionSql::Literal(CheckLiteralSql::Number(
                    "9".into(),
                ))),
                negated: false,
            }
        );
        assert_eq!(
            bind_check_expression(&parsed.expression, |name| {
                (name == "priority").then_some((0, ScalarType::Integer))
            })
            .unwrap(),
            Expr::And(vec![
                Expr::GreaterEqual(
                    Box::new(Expr::Field(0)),
                    Box::new(Expr::Literal(Some(ScalarValue::Integer(1)))),
                ),
                Expr::LessEqual(
                    Box::new(Expr::Field(0)),
                    Box::new(Expr::Literal(Some(ScalarValue::Integer(9)))),
                ),
            ])
        );

        let negative = parse_create_table(
            "CREATE TABLE app.jobs (state TEXT CHECK (state NOT BETWEEN 'a' AND 'z'))",
        )
        .unwrap();
        assert!(matches!(
            negative.check_constraints.first(),
            Some(CheckExpressionSql::Between { negated: true, .. })
        ));
        assert!(
            bind_check_expression(&negative.check_constraints[0], |name| {
                (name == "state").then_some((0, ScalarType::Text))
            })
            .is_ok()
        );
        assert!(
            parse_add_check_constraint("ALTER TABLE app.jobs ADD CHECK (priority BETWEEN 1)")
                .is_err()
        );
        assert!(
            bind_check_expression(&parsed.expression, |name| {
                (name == "priority").then_some((0, ScalarType::Boolean))
            })
            .is_err()
        );
        let grouped = parse_add_check_constraint(
            "ALTER TABLE app.jobs ADD CHECK ((priority >= 1) BETWEEN FALSE AND TRUE)",
        )
        .unwrap();
        assert!(
            bind_check_expression(&grouped.expression, |name| {
                (name == "priority").then_some((0, ScalarType::Integer))
            })
            .is_ok()
        );
        let bounds = parse_add_check_constraint(
            "ALTER TABLE app.jobs ADD CHECK (priority BETWEEN minimum AND maximum)",
        )
        .unwrap();
        assert_eq!(
            bind_check_expression(&bounds.expression, |name| match name {
                "priority" => Some((0, ScalarType::Integer)),
                "minimum" => Some((1, ScalarType::Integer)),
                "maximum" => Some((2, ScalarType::Integer)),
                _ => None,
            })
            .unwrap(),
            Expr::And(vec![
                Expr::GreaterEqual(Box::new(Expr::Field(0)), Box::new(Expr::Field(1))),
                Expr::LessEqual(Box::new(Expr::Field(0)), Box::new(Expr::Field(2))),
            ])
        );
    }

    #[test]
    fn parses_and_binds_null_safe_distinctness_predicates() {
        use crate::authorization::Expr;

        let distinct = parse_add_check_constraint(
            "ALTER TABLE app.items ADD CHECK (code IS DISTINCT FROM 'retired')",
        )
        .unwrap();
        assert_eq!(
            distinct.expression,
            CheckExpressionSql::DistinctFrom {
                expression: Box::new(CheckExpressionSql::Column("code".into())),
                right: Box::new(CheckExpressionSql::Literal(CheckLiteralSql::String(
                    "retired".into(),
                ))),
                negated: false,
            }
        );
        assert_eq!(
            bind_check_expression(&distinct.expression, |name| {
                (name == "code").then_some((0, ScalarType::Text))
            })
            .unwrap(),
            Expr::Or(vec![
                Expr::IsNull(Box::new(Expr::Field(0))),
                Expr::NotEqual(
                    Box::new(Expr::Field(0)),
                    Box::new(Expr::Literal(Some(ScalarValue::Text("retired".into())))),
                ),
            ])
        );

        let not_distinct = parse_create_table(
            "CREATE TABLE app.items (code UUID CHECK (code IS NOT DISTINCT FROM NULL))",
        )
        .unwrap();
        assert!(matches!(
            not_distinct.check_constraints.first(),
            Some(CheckExpressionSql::DistinctFrom { negated: true, .. })
        ));
        assert_eq!(
            bind_check_expression(&not_distinct.check_constraints[0], |name| {
                (name == "code").then_some((0, ScalarType::Uuid))
            })
            .unwrap(),
            Expr::IsNull(Box::new(Expr::Field(0)))
        );

        let expression = parse_add_check_constraint(
            "ALTER TABLE app.items ADD CHECK ((code IS NULL) IS DISTINCT FROM TRUE)",
        )
        .unwrap();
        assert_eq!(
            bind_check_expression(&expression.expression, |name| {
                (name == "code").then_some((0, ScalarType::Text))
            })
            .unwrap(),
            Expr::Or(vec![
                Expr::IsNull(Box::new(Expr::IsNull(Box::new(Expr::Field(0))))),
                Expr::NotEqual(
                    Box::new(Expr::IsNull(Box::new(Expr::Field(0)))),
                    Box::new(Expr::Literal(Some(ScalarValue::Boolean(true)))),
                ),
            ])
        );

        let columns = parse_add_check_constraint(
            "ALTER TABLE app.items ADD CHECK (code IS DISTINCT FROM other)",
        )
        .unwrap();
        let bound = bind_check_expression(&columns.expression, |name| match name {
            "code" => Some((0, ScalarType::Text)),
            "other" => Some((1, ScalarType::Text)),
            _ => None,
        })
        .unwrap();
        assert!(matches!(bound, Expr::Or(ref branches) if branches.len() == 3));
        assert!(crate::expression::encode_expression(&bound).is_ok());
        assert!(
            bind_check_expression(&columns.expression, |name| match name {
                "code" => Some((0, ScalarType::Text)),
                "other" => Some((1, ScalarType::Integer)),
                _ => None,
            })
            .is_err()
        );

        let literals = parse_add_check_constraint(
            "ALTER TABLE app.items ADD CHECK ('x' IS NOT DISTINCT FROM 'x')",
        )
        .unwrap();
        assert!(bind_check_expression(&literals.expression, |_| None).is_ok());

        for sql in [
            "ALTER TABLE app.items ADD CHECK (code IS DISTINCT)",
            "ALTER TABLE app.items ADD CHECK (code IS DISTINCT FROM)",
        ] {
            assert!(parse_add_check_constraint(sql).is_err(), "accepted {sql}");
        }
    }

    #[test]
    fn parses_and_binds_boolean_truth_tests() {
        use crate::authorization::Expr;

        let is_null =
            parse_add_check_constraint("ALTER TABLE app.jobs ADD CHECK ((active = TRUE) IS NULL)")
                .unwrap();
        let equal_true = Expr::Equal(
            Box::new(Expr::Field(0)),
            Box::new(Expr::Literal(Some(ScalarValue::Boolean(true)))),
        );
        assert_eq!(
            bind_check_expression(&is_null.expression, |name| {
                (name == "active").then_some((0, ScalarType::Boolean))
            })
            .unwrap(),
            Expr::IsNull(Box::new(equal_true.clone()))
        );
        let is_not_null = parse_add_check_constraint(
            "ALTER TABLE app.jobs ADD CHECK ((active = TRUE) IS NOT NULL)",
        )
        .unwrap();
        assert_eq!(
            bind_check_expression(&is_not_null.expression, |name| {
                (name == "active").then_some((0, ScalarType::Boolean))
            })
            .unwrap(),
            Expr::Not(Box::new(Expr::IsNull(Box::new(equal_true))))
        );

        let compound =
            parse_add_check_constraint("ALTER TABLE app.jobs ADD CHECK ((active = TRUE) IS TRUE)")
                .unwrap();
        assert!(matches!(
            &compound.expression,
            CheckExpressionSql::BooleanTest {
                expression,
                test: CheckBooleanTest::True,
                negated: false,
            } if matches!(**expression, CheckExpressionSql::Compare { .. })
        ));
        assert!(
            bind_check_expression(&compound.expression, |name| {
                (name == "active").then_some((0, ScalarType::Boolean))
            })
            .is_ok()
        );
        let role = parse_add_check_constraint(
            "ALTER TABLE app.jobs ADD CHECK (HAS_ROLE('operator') IS TRUE)",
        )
        .unwrap();
        assert_eq!(
            bind_policy_expression(
                &role.expression,
                |_| None,
                |name| { (name == "operator").then_some([9; 16]) }
            )
            .unwrap(),
            Expr::And(vec![
                Expr::Not(Box::new(Expr::IsNull(Box::new(Expr::HasRole([9; 16]))))),
                Expr::Equal(
                    Box::new(Expr::HasRole([9; 16])),
                    Box::new(Expr::Literal(Some(ScalarValue::Boolean(true)))),
                ),
            ])
        );
        assert!(bind_check_expression(&role.expression, |_| None).is_err());

        for (sql, test, negated, expected_value) in [
            ("active IS TRUE", CheckBooleanTest::True, false, Some(true)),
            (
                "active IS NOT TRUE",
                CheckBooleanTest::True,
                true,
                Some(true),
            ),
            (
                "active IS FALSE",
                CheckBooleanTest::False,
                false,
                Some(false),
            ),
            (
                "active IS NOT FALSE",
                CheckBooleanTest::False,
                true,
                Some(false),
            ),
            ("active IS UNKNOWN", CheckBooleanTest::Unknown, false, None),
            (
                "active IS NOT UNKNOWN",
                CheckBooleanTest::Unknown,
                true,
                None,
            ),
        ] {
            let statement =
                parse_add_check_constraint(&format!("ALTER TABLE app.jobs ADD CHECK ({sql})"))
                    .unwrap();
            assert_eq!(
                statement.expression,
                CheckExpressionSql::BooleanTest {
                    expression: Box::new(CheckExpressionSql::Column("active".into())),
                    test,
                    negated,
                }
            );
            let expected = match expected_value {
                None => Expr::IsNull(Box::new(Expr::Field(0))),
                Some(value) => Expr::And(vec![
                    Expr::Not(Box::new(Expr::IsNull(Box::new(Expr::Field(0))))),
                    Expr::Equal(
                        Box::new(Expr::Field(0)),
                        Box::new(Expr::Literal(Some(ScalarValue::Boolean(value)))),
                    ),
                ]),
            };
            let expected = if negated {
                Expr::Not(Box::new(expected))
            } else {
                expected
            };
            assert_eq!(
                bind_check_expression(&statement.expression, |name| {
                    (name == "active").then_some((0, ScalarType::Boolean))
                })
                .unwrap(),
                expected
            );
        }
        let non_boolean =
            parse_add_check_constraint("ALTER TABLE app.jobs ADD CHECK (state IS TRUE)").unwrap();
        assert!(
            bind_check_expression(&non_boolean.expression, |name| {
                (name == "state").then_some((0, ScalarType::Text))
            })
            .is_err()
        );
        for sql in [
            "ALTER TABLE app.jobs ADD CHECK (active IS MAYBE)",
            "ALTER TABLE app.jobs ADD CHECK (active IS TRUEISH)",
        ] {
            assert!(parse_add_check_constraint(sql).is_err(), "accepted {sql}");
        }
    }

    #[test]
    fn binds_policy_context_functions_to_typed_authority_nodes() {
        use crate::authorization::Expr;
        let current = parse_add_check_constraint(
            "ALTER TABLE app.records ADD CHECK (owner = CURRENT_PRINCIPAL_ID())",
        )
        .unwrap();
        let expression = bind_policy_expression(
            &current.expression,
            |name| (name == "owner").then_some((0, ScalarType::Uuid)),
            |_| None,
        )
        .unwrap();
        assert_eq!(
            expression,
            Expr::Equal(Box::new(Expr::Field(0)), Box::new(Expr::CurrentPrincipalId),)
        );
        let role =
            parse_add_check_constraint("ALTER TABLE app.records ADD CHECK (HAS_ROLE('payroll'))")
                .unwrap();
        assert_eq!(
            bind_policy_expression(
                &role.expression,
                |_| None,
                |name| { (name == "payroll").then_some([7; 16]) }
            )
            .unwrap(),
            Expr::HasRole([7; 16])
        );
        assert!(bind_check_expression(&current.expression, |_| None).is_err());
        assert!(bind_policy_expression(&role.expression, |_| None, |_| None).is_err());
    }

    #[test]
    fn parses_bounded_check_alterations_and_rejects_other_expression_syntax() {
        assert_eq!(
            parse_add_check_constraint("ALTER TABLE app.events ADD CHECK (active AND (deleted IS NULL OR deleted IS NOT NULL));").unwrap(),
            AddCheckConstraintSql {
                schema: "app".into(),
                table: "events".into(),
                name: None,
                expression: CheckExpressionSql::And(vec![
                    CheckExpressionSql::Column("active".into()),
                    CheckExpressionSql::Or(vec![
                        CheckExpressionSql::IsNull(Box::new(CheckExpressionSql::Column(
                            "deleted".into(),
                        ))),
                        CheckExpressionSql::IsNotNull(Box::new(CheckExpressionSql::Column(
                            "deleted".into(),
                        ))),
                    ]),
                ]),
            }
        );
        let named = parse_add_check_constraint(
            "ALTER TABLE app.events ADD CONSTRAINT positive_amount CHECK (amount > 0)",
        )
        .unwrap();
        assert_eq!(named.name.as_deref(), Some("positive_amount"));
        let named_drop =
            parse_drop_constraint("ALTER TABLE app.orders DROP CONSTRAINT positive_amount")
                .unwrap();
        assert_eq!(
            named_drop.constraint_name.as_deref(),
            Some("positive_amount")
        );
        assert_eq!(named_drop.constraint_id, [0; 16]);
        for sql in [
            "ALTER TABLE app.events ADD CHECK (active); ALTER TABLE app.events ADD CHECK (active)",
            "ALTER TABLE events ADD CHECK (active)",
            "ALTER TABLE app.events ADD CONSTRAINT CHECK (active)",
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
    fn parses_bounded_varchar_widening_syntax() {
        assert_eq!(
            parse_widen_varchar("ALTER TABLE app.events ALTER COLUMN label TYPE VARCHAR(120);")
                .unwrap(),
            WidenVarcharSql {
                schema: "app".into(),
                table: "events".into(),
                column: "label".into(),
                max_chars: 120,
            }
        );
        for sql in [
            "ALTER TABLE app.events ALTER COLUMN label TYPE VARCHAR(0)",
            "ALTER TABLE events ALTER COLUMN label TYPE VARCHAR(120)",
            "ALTER TABLE app.events ALTER COLUMN label TYPE INTEGER",
            "ALTER TABLE app.events ALTER COLUMN label TYPE VARCHAR(120) USING label",
            "ALTER TABLE app.events ALTER COLUMN label TYPE VARCHAR(120); ALTER TABLE app.events ALTER COLUMN label TYPE VARCHAR(140)",
        ] {
            assert!(parse_widen_varchar(sql).is_err(), "accepted {sql}");
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
                if_not_exists: false,
            }
        );
        let unique = parse_create_index(
            "CREATE UNIQUE INDEX employee_department ON app.employees (department_id, name)",
        )
        .unwrap();
        assert!(unique.unique);
        assert_eq!(unique.columns, ["department_id", "name"]);
        let conditional = parse_create_index(
            "CREATE UNIQUE INDEX IF NOT EXISTS employee_department ON app.employees (department_id, name)",
        )
        .unwrap();
        assert!(conditional.unique);
        assert!(conditional.if_not_exists);
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
    fn parses_bounded_drop_index_sql() {
        assert_eq!(
            parse_drop_index("DROP INDEX app.employee_department RESTRICT;").unwrap(),
            DropIndexSql {
                schema: "app".into(),
                name: "employee_department".into(),
                if_exists: false,
            }
        );
        for sql in [
            "DROP INDEX employee_department RESTRICT",
            "DROP INDEX app.employee_department",
            "DROP INDEX app.employee_department CASCADE",
            "DROP INDEX app.employee_department RESTRICT; DROP TABLE app.users RESTRICT",
        ] {
            assert!(parse_drop_index(sql).is_err(), "accepted {sql}");
        }
    }

    #[test]
    fn parses_bounded_rebuild_index_sql() {
        assert_eq!(
            parse_rebuild_index("REBUILD INDEX app.employee_department;").unwrap(),
            RebuildIndexSql {
                schema: "app".into(),
                name: "employee_department".into(),
            }
        );
        for sql in [
            "REBUILD INDEX employee_department",
            "REBUILD INDEX app.employee_department RESTRICT",
            "REBUILD INDEX app.employee_department; DROP TABLE app.users RESTRICT",
        ] {
            assert!(parse_rebuild_index(sql).is_err(), "accepted {sql}");
        }
    }

    #[test]
    fn parses_bounded_alter_index_rebuild_sql() {
        assert_eq!(
            parse_alter_index_rebuild(
                "ALTER INDEX app.employee_department REBUILD USING BTREE UNIQUE (department_id, employee_id);"
            )
            .unwrap(),
            AlterIndexRebuildSql {
                schema: "app".into(),
                name: "employee_department".into(),
                columns: vec!["department_id".into(), "employee_id".into()],
                unique: true,
            }
        );
        for sql in [
            "ALTER INDEX employee_department REBUILD UNIQUE (department_id)",
            "ALTER INDEX app.employee_department REBUILD (department_id)",
            "ALTER INDEX app.employee_department REBUILD USING HASH UNIQUE (department_id)",
            "ALTER INDEX app.employee_department REBUILD UNIQUE (department_id, department_id)",
            "ALTER INDEX app.employee_department REBUILD NONUNIQUE ()",
            "ALTER INDEX app.employee_department REBUILD UNIQUE (department_id); DROP INDEX app.other RESTRICT",
        ] {
            assert!(parse_alter_index_rebuild(sql).is_err(), "accepted {sql}");
        }
    }

    #[test]
    fn parses_bounded_rename_index_sql() {
        assert_eq!(
            parse_rename_index("ALTER INDEX app.employee_department RENAME TO employee_team;")
                .unwrap(),
            RenameIndexSql {
                schema: "app".into(),
                name: "employee_department".into(),
                new_name: "employee_team".into(),
            }
        );
        for sql in [
            "ALTER INDEX employee_department RENAME TO employee_team",
            "ALTER INDEX app.employee_department RENAME employee_team",
            "ALTER INDEX app.employee_department RENAME TO employee_team; DROP INDEX app.other RESTRICT",
        ] {
            assert!(parse_rename_index(sql).is_err(), "accepted {sql}");
        }
    }

    #[test]
    fn parses_bounded_identity_creation_sql() {
        assert_eq!(
            parse_create_identity("CREATE USER analyst LOGIN;").unwrap(),
            CreateIdentitySql {
                kind: CreateIdentityKind::User,
                if_not_exists: false,
                name: "analyst".into(),
                login_enabled: true,
            }
        );
        assert_eq!(
            parse_create_identity("CREATE ROLE reporting").unwrap(),
            CreateIdentitySql {
                kind: CreateIdentityKind::Role,
                if_not_exists: false,
                name: "reporting".into(),
                login_enabled: false,
            }
        );
        assert!(
            parse_create_identity("CREATE USER IF NOT EXISTS analyst NOLOGIN")
                .unwrap()
                .if_not_exists
        );
        assert!(
            parse_create_identity("CREATE ROLE IF NOT EXISTS reporting")
                .unwrap()
                .if_not_exists
        );
        for sql in [
            "CREATE USER",
            "CREATE USER IF EXISTS analyst LOGIN",
            "CREATE GROUP reporting",
            "CREATE USER analyst",
            "CREATE USER app.analyst",
            "CREATE ROLE reporting LOGIN",
            "CREATE ROLE reporting; CREATE USER analyst",
        ] {
            assert!(parse_create_identity(sql).is_err(), "accepted {sql}");
        }
        assert!(
            !parse_create_identity("CREATE USER service_loader NOLOGIN")
                .unwrap()
                .login_enabled
        );
    }

    #[test]
    fn parses_bounded_identity_rename_sql() {
        assert_eq!(
            parse_rename_identity("ALTER USER analyst RENAME TO analyst_admin;").unwrap(),
            RenameIdentitySql {
                kind: CreateIdentityKind::User,
                old_name: "analyst".into(),
                new_name: "analyst_admin".into(),
            }
        );
        assert_eq!(
            parse_rename_identity("ALTER ROLE reporting RENAME TO report_readers").unwrap(),
            RenameIdentitySql {
                kind: CreateIdentityKind::Role,
                old_name: "reporting".into(),
                new_name: "report_readers".into(),
            }
        );
        for sql in [
            "ALTER USER RENAME TO analyst_admin",
            "ALTER USER analyst RENAME analyst_admin",
            "ALTER USER analyst RENAME TO app.analyst_admin",
            "ALTER USER analyst RENAME TO analyst_admin; DROP USER other RESTRICT",
        ] {
            assert!(parse_rename_identity(sql).is_err(), "accepted {sql}");
        }
    }

    #[test]
    fn parses_bounded_identity_drop_sql() {
        assert_eq!(
            parse_drop_identity("DROP USER service_loader RESTRICT;").unwrap(),
            DropIdentitySql {
                kind: CreateIdentityKind::User,
                name: "service_loader".into(),
                if_exists: false,
            }
        );
        assert_eq!(
            parse_drop_identity("DROP ROLE old_role RESTRICT").unwrap(),
            DropIdentitySql {
                kind: CreateIdentityKind::Role,
                name: "old_role".into(),
                if_exists: false,
            }
        );
        for sql in [
            "DROP USER service_loader",
            "DROP USER service_loader CASCADE",
            "DROP ROLE RESTRICT",
            "DROP ROLE old_role RESTRICT; CREATE ROLE other",
        ] {
            assert!(parse_drop_identity(sql).is_err(), "accepted {sql}");
        }
    }

    #[test]
    fn parses_bounded_user_login_sql() {
        assert_eq!(
            parse_set_user_login("ALTER USER analyst NOLOGIN;").unwrap(),
            SetUserLoginSql {
                name: "analyst".into(),
                enabled: false,
            }
        );
        assert_eq!(
            parse_set_user_login("ALTER USER analyst LOGIN").unwrap(),
            SetUserLoginSql {
                name: "analyst".into(),
                enabled: true,
            }
        );
        for sql in [
            "ALTER ROLE analyst LOGIN",
            "ALTER USER analyst",
            "ALTER USER analyst LOGIN; ALTER USER analyst NOLOGIN",
        ] {
            assert!(parse_set_user_login(sql).is_err(), "accepted {sql}");
        }
    }

    #[test]
    fn parses_bounded_role_membership_sql() {
        assert_eq!(
            parse_role_membership("GRANT managers TO USER alice;").unwrap(),
            RoleMembershipSql {
                grant: true,
                role_name: "managers".into(),
                member_name: "alice".into(),
                member_is_role: false,
            }
        );
        assert_eq!(
            parse_role_membership("REVOKE managers FROM ROLE app_owner").unwrap(),
            RoleMembershipSql {
                grant: false,
                role_name: "managers".into(),
                member_name: "app_owner".into(),
                member_is_role: true,
            }
        );
        for sql in [
            "GRANT managers USER alice",
            "REVOKE managers TO USER alice",
            "GRANT managers TO alice",
            "GRANT managers TO USER alice WITH ADMIN OPTION",
            "GRANT managers TO USER alice; REVOKE managers FROM USER alice",
        ] {
            assert!(parse_role_membership(sql).is_err(), "accepted {sql}");
        }
    }

    #[test]
    fn parses_bounded_system_privilege_sql() {
        assert_eq!(
            parse_system_privilege("GRANT SYSTEM PRIVILEGE MANAGE_ROLES TO ROLE administrators;")
                .unwrap(),
            SystemPrivilegeSql {
                grant: true,
                privilege: "manage_roles".into(),
                grantee_name: "administrators".into(),
                grantee_is_role: true,
            }
        );
        assert_eq!(
            parse_system_privilege("REVOKE SYSTEM PRIVILEGE CREATE_SCHEMA FROM USER deployer")
                .unwrap(),
            SystemPrivilegeSql {
                grant: false,
                privilege: "create_schema".into(),
                grantee_name: "deployer".into(),
                grantee_is_role: false,
            }
        );
        assert_eq!(
            parse_system_privilege("GRANT MANAGE_USERS ON DATABASE TO USER account_admin;")
                .unwrap(),
            SystemPrivilegeSql {
                grant: true,
                privilege: "manage_users".into(),
                grantee_name: "account_admin".into(),
                grantee_is_role: false,
            }
        );
        assert_eq!(
            parse_system_privilege("REVOKE CREATE_SCHEMA ON DATABASE FROM ROLE deployers").unwrap(),
            SystemPrivilegeSql {
                grant: false,
                privilege: "create_schema".into(),
                grantee_name: "deployers".into(),
                grantee_is_role: true,
            }
        );
        for sql in [
            "GRANT SYSTEM PRIVILEGE SELECT TO USER alice",
            "GRANT SYSTEM PRIVILEGE MANAGE_USERS USER alice",
            "REVOKE SYSTEM PRIVILEGE CREATE_SCHEMA TO USER alice",
            "GRANT SYSTEM PRIVILEGE MANAGE_ROLES TO ROLE admins WITH ADMIN OPTION",
            "GRANT SYSTEM PRIVILEGE MANAGE_USERS TO USER alice; REVOKE SYSTEM PRIVILEGE MANAGE_USERS FROM USER alice",
            "GRANT MANAGE_USERS TO USER alice",
            "GRANT SELECT ON DATABASE TO USER alice",
            "GRANT MANAGE_USERS ON SCHEMA app TO USER alice",
            "REVOKE MANAGE_ROLES ON DATABASE TO ROLE admins",
        ] {
            assert!(parse_system_privilege(sql).is_err(), "accepted {sql}");
        }
    }

    #[test]
    fn parses_bounded_object_privilege_sql() {
        assert_eq!(
            parse_object_privilege("GRANT USAGE ON SCHEMA app TO ROLE managers;").unwrap(),
            ObjectPrivilegeSql {
                grant: true,
                privilege: "usage".into(),
                schema_name: "app".into(),
                table_name: None,
                column_names: vec![],
                grantee_name: "managers".into(),
                grantee_is_role: true,
            }
        );
        assert_eq!(
            parse_object_privilege("REVOKE SELECT ON TABLE app.employees FROM USER reader")
                .unwrap(),
            ObjectPrivilegeSql {
                grant: false,
                privilege: "select".into(),
                schema_name: "app".into(),
                table_name: Some("employees".into()),
                column_names: vec![],
                grantee_name: "reader".into(),
                grantee_is_role: false,
            }
        );
        for sql in [
            "GRANT SELECT ON employees TO USER reader",
            "GRANT USAGE ON TABLE app.employees TO USER reader",
            "GRANT SELECT ON SCHEMA app TO USER reader WITH GRANT OPTION",
            "REVOKE USAGE ON SCHEMA app TO ROLE managers",
            "GRANT DELETE ON TABLE app.t TO ROLE r; REVOKE DELETE ON TABLE app.t FROM ROLE r",
        ] {
            assert!(parse_object_privilege(sql).is_err(), "accepted {sql}");
        }
        assert_eq!(
            parse_object_privilege("GRANT SELECT (salary) ON TABLE app.employees TO ROLE readers")
                .unwrap()
                .column_names,
            vec!["salary"]
        );
        for sql in [
            "GRANT DELETE (salary) ON TABLE app.t TO ROLE r",
            "GRANT SELECT (a) ON SCHEMA app TO ROLE r",
            "GRANT SELECT () ON TABLE app.t TO ROLE r",
            "GRANT SELECT (a, a) ON TABLE app.t TO ROLE r",
        ] {
            assert!(parse_object_privilege(sql).is_err(), "accepted {sql}");
        }
    }

    #[test]
    fn parses_multiple_column_object_privilege_targets() {
        assert_eq!(
            parse_object_privilege(
                "GRANT SELECT (first_name, last_name) ON TABLE app.people TO ROLE readers"
            )
            .unwrap()
            .column_names,
            vec!["first_name", "last_name"]
        );
        let too_many_columns = (0..65)
            .map(|index| format!("column_{index}"))
            .collect::<Vec<_>>()
            .join(", ");
        assert!(
            parse_object_privilege(&format!(
                "GRANT SELECT ({too_many_columns}) ON TABLE app.people TO ROLE readers"
            ))
            .is_err()
        );
    }

    #[test]
    fn parses_bounded_row_and_cell_policy_sql() {
        let row = parse_create_policy(
            "CREATE ROW POLICY visible ON app.records FOR SELECT TO ROLE readers, USER auditor AS ALLOW USING (enabled = TRUE AND owner = CURRENT_PRINCIPAL_ID())",
        )
        .unwrap();
        assert_eq!(row.name, "visible");
        assert!(!row.replace_existing);
        assert!(!row.is_cell);
        assert_eq!(row.operation, "select");
        assert_eq!(row.recipients.len(), 2);
        assert!(row.with_check_expression.is_none());
        let cell = parse_create_policy(
            "CREATE CELL POLICY salary_read ON app.records (salary) FOR SELECT TO ROLE managers AS RESTRICT USING (HAS_ROLE('payroll') OR salary IS NULL);",
        )
        .unwrap();
        assert!(cell.is_cell);
        assert_eq!(cell.columns, ["salary"]);
        let replacement = parse_create_policy(
            "CREATE OR REPLACE ROW POLICY visible ON app.records FOR SELECT TO ROLE readers AS ALLOW USING (enabled = TRUE)",
        )
        .unwrap();
        assert!(replacement.replace_existing);
        assert_eq!(replacement.name, "visible");
        for sql in [
            "CREATE ROW POLICY p ON app.t FOR SELECT TO ROLE r AS ALLOW",
            "CREATE ROW POLICY p ON app.t FOR UPDATE TO ROLE r AS ALLOW USING (TRUE)",
            "CREATE ROW POLICY p ON app.t FOR SELECT TO ROLE r AS ALLOW WITH CHECK (TRUE)",
            "CREATE CELL POLICY p ON app.t (x) FOR DELETE TO ROLE r AS ALLOW USING (TRUE)",
            "CREATE CELL POLICY p ON app.t (x, x) FOR SELECT TO ROLE r AS ALLOW USING (TRUE)",
            "CREATE ROW POLICY p ON app.t FOR INSERT TO ROLE r, ROLE r AS ALLOW WITH CHECK (TRUE)",
            "CREATE ROW POLICY p ON app.t FOR SELECT TO ROLE r AS ALLOW USING (random() > 0)",
            "CREATE ROW POLICY p ON app.t FOR SELECT TO ROLE r AS ALLOW USING (TRUE); CREATE ROLE other",
        ] {
            assert!(parse_create_policy(sql).is_err(), "accepted {sql}");
        }
    }

    #[test]
    fn parses_bounded_row_and_cell_policy_drop_sql() {
        let row = parse_drop_policy("DROP ROW POLICY visible ON app.records RESTRICT;").unwrap();
        assert_eq!(row.schema, "app");
        assert_eq!(row.table, "records");
        assert_eq!(row.name, "visible");
        assert!(!row.is_cell);
        let cell =
            parse_drop_policy("DROP CELL POLICY salary_read ON app.records RESTRICT").unwrap();
        assert!(cell.is_cell);
        for sql in [
            "DROP POLICY visible ON app.records RESTRICT",
            "DROP ROW POLICY visible ON app.records",
            "DROP ROW POLICY visible ON app.records CASCADE",
            "DROP CELL POLICY p ON app.t RESTRICT; DROP ROLE r",
        ] {
            assert!(parse_drop_policy(sql).is_err(), "accepted {sql}");
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
        assert!(!parsed.if_not_exists);
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
        let conditional =
            parse_add_column("ALTER TABLE app.events ADD COLUMN IF NOT EXISTS label TEXT").unwrap();
        assert!(conditional.if_not_exists);
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
                column: "obsolete".into(),
                if_exists: false,
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
                    name: None,
                    columns: vec!["customer_id".into()],
                    referenced_schema: "crm".into(),
                    referenced_table: "customers".into(),
                    referenced_columns: vec!["id".into()],
                },
                CreateTableForeignKeySql {
                    name: None,
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
    fn parses_column_level_foreign_key_constraints() {
        let parsed = parse_create_table(
            "CREATE TABLE app.order_lines (customer_id INTEGER REFERENCES crm.customers (id), order_id BIGINT, FOREIGN KEY (order_id) REFERENCES app.orders (id));",
        )
        .unwrap();
        assert_eq!(
            parsed.foreign_keys,
            vec![
                CreateTableForeignKeySql {
                    name: None,
                    columns: vec!["customer_id".into()],
                    referenced_schema: "crm".into(),
                    referenced_table: "customers".into(),
                    referenced_columns: vec!["id".into()],
                },
                CreateTableForeignKeySql {
                    name: None,
                    columns: vec!["order_id".into()],
                    referenced_schema: "app".into(),
                    referenced_table: "orders".into(),
                    referenced_columns: vec!["id".into()],
                },
            ]
        );
        for sql in [
            "CREATE TABLE app.order_lines (customer_id INTEGER REFERENCES customers (id))",
            "CREATE TABLE app.order_lines (customer_id INTEGER REFERENCES crm.customers (id, region_id))",
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
            "CREATE TABLE app.events (id UUID, other INTEGER CHECK (id IS NOT NULL))",
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
