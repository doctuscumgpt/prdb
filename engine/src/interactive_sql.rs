//! Bounded statement and RUNAS-block parsing for the interactive SQL client.
//!
//! This parser only frames statements and classifies top-level commands. Query
//! grammar, binding, and authorization remain in the SDK and server paths.

pub const MAX_INTERACTIVE_SQL_BYTES: usize = 65_536;
pub const MAX_RUNAS_STATEMENTS: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InteractiveStatement<'a> {
    Begin,
    Commit,
    Rollback,
    RunAs {
        username: &'a str,
        sql: &'a str,
        statement_count: usize,
    },
    Ddl(&'a str),
    Query(&'a str),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InteractiveSqlError {
    Empty,
    TooLarge,
    EmbeddedNul,
    UnterminatedString,
    UnterminatedComment,
    UnbalancedParentheses,
    EmptyRunAs,
    InvalidRunAsUsername,
    MissingEndRunAs,
    InvalidEndRunAs,
    NestedRunAs,
    DdlInsideRunAs,
    TransactionControlInsideRunAs,
    TooManyRunAsStatements,
    TransactionControlArguments,
}

impl std::fmt::Display for InteractiveSqlError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "interactive SQL input error: {self:?}")
    }
}

impl std::error::Error for InteractiveSqlError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TokenKind {
    Word,
    Quoted,
    Semicolon,
    Punctuation,
}

#[derive(Clone, Copy, Debug)]
struct Token {
    kind: TokenKind,
    start: usize,
    end: usize,
    line_start: bool,
}

pub fn parse_interactive_sql(
    sql: &str,
) -> Result<Vec<InteractiveStatement<'_>>, InteractiveSqlError> {
    if sql.len() > MAX_INTERACTIVE_SQL_BYTES {
        return Err(InteractiveSqlError::TooLarge);
    }
    if sql.as_bytes().contains(&0) {
        return Err(InteractiveSqlError::EmbeddedNul);
    }
    let tokens = lex(sql)?;
    let mut statements = Vec::new();
    let mut index = 0;

    while index < tokens.len() {
        while index < tokens.len() && tokens[index].kind == TokenKind::Semicolon {
            index += 1;
        }
        if index == tokens.len() {
            break;
        }

        let first = tokens[index];
        if is_word(sql, first, "RUNAS") {
            let username_token = tokens
                .get(index + 1)
                .copied()
                .filter(|token| token.kind == TokenKind::Word)
                .ok_or(InteractiveSqlError::InvalidRunAsUsername)?;
            let username = &sql[username_token.start..username_token.end];
            if !is_normalized_username(username) {
                return Err(InteractiveSqlError::InvalidRunAsUsername);
            }
            let end_index = (index + 2..tokens.len())
                .find(|candidate| {
                    let token = tokens[*candidate];
                    token.kind == TokenKind::Word
                        && token.line_start
                        && is_word(sql, token, "ENDRUNAS")
                })
                .ok_or(InteractiveSqlError::MissingEndRunAs)?;
            let end_token = tokens[end_index];
            let body = &sql[username_token.end..end_token.start];
            let body_tokens = &tokens[index + 2..end_index];
            let statement_count = validate_runas_body(sql, body_tokens)?;
            statements.push(InteractiveStatement::RunAs {
                username,
                sql: body,
                statement_count,
            });
            index = end_index + 1;
            if tokens
                .get(index)
                .is_some_and(|token| token.kind == TokenKind::Semicolon)
            {
                index += 1;
            } else if index < tokens.len() {
                return Err(InteractiveSqlError::InvalidEndRunAs);
            }
            continue;
        }

        let end_index = (index..tokens.len())
            .find(|candidate| tokens[*candidate].kind == TokenKind::Semicolon)
            .unwrap_or(tokens.len());
        let segment = &tokens[index..end_index];
        if segment.is_empty() {
            index = end_index.saturating_add(1);
            continue;
        }
        let statement = &sql[first.start..segment.last().expect("nonempty segment").end];
        let first_word = (first.kind == TokenKind::Word).then(|| &sql[first.start..first.end]);
        let command = match first_word.map(str::to_ascii_uppercase).as_deref() {
            Some("BEGIN") => {
                if segment.len() != 1 {
                    return Err(InteractiveSqlError::TransactionControlArguments);
                }
                InteractiveStatement::Begin
            }
            Some("COMMIT") => {
                if segment.len() != 1 {
                    return Err(InteractiveSqlError::TransactionControlArguments);
                }
                InteractiveStatement::Commit
            }
            Some("ROLLBACK") => {
                if segment.len() != 1 {
                    return Err(InteractiveSqlError::TransactionControlArguments);
                }
                InteractiveStatement::Rollback
            }
            Some("CREATE" | "ALTER" | "DROP" | "GRANT" | "REVOKE" | "TRUNCATE") => {
                InteractiveStatement::Ddl(statement)
            }
            _ => InteractiveStatement::Query(statement),
        };
        statements.push(command);
        index = if end_index < tokens.len() {
            end_index + 1
        } else {
            end_index
        };
    }

    if statements.is_empty() {
        return Err(InteractiveSqlError::Empty);
    }
    Ok(statements)
}

pub(crate) fn contains_parameter_placeholder(sql: &str) -> bool {
    let Ok(tokens) = lex(sql) else {
        return true;
    };
    tokens.windows(2).any(|pair| {
        let marker = pair[0];
        let digit = pair[1];
        marker.kind == TokenKind::Punctuation
            && &sql[marker.start..marker.end] == "$"
            && marker.end == digit.start
            && digit.kind == TokenKind::Punctuation
            && sql.as_bytes()[digit.start].is_ascii_digit()
    })
}

fn validate_runas_body(sql: &str, tokens: &[Token]) -> Result<usize, InteractiveSqlError> {
    let mut count = 0usize;
    let mut start = 0usize;
    for end in 0..=tokens.len() {
        if end != tokens.len() && tokens[end].kind != TokenKind::Semicolon {
            continue;
        }
        let segment = &tokens[start..end];
        start = end.saturating_add(1);
        if segment.is_empty() {
            continue;
        }
        let first = segment[0];
        let Some(command) = (first.kind == TokenKind::Word)
            .then(|| &sql[first.start..first.end])
            .map(str::to_ascii_uppercase)
        else {
            count += 1;
            if count > MAX_RUNAS_STATEMENTS {
                return Err(InteractiveSqlError::TooManyRunAsStatements);
            }
            continue;
        };
        match command.as_str() {
            "RUNAS" => return Err(InteractiveSqlError::NestedRunAs),
            "ENDRUNAS" => return Err(InteractiveSqlError::InvalidEndRunAs),
            "BEGIN" | "COMMIT" | "ROLLBACK" => {
                return Err(InteractiveSqlError::TransactionControlInsideRunAs);
            }
            "CREATE" | "ALTER" | "DROP" | "GRANT" | "REVOKE" | "TRUNCATE" => {
                return Err(InteractiveSqlError::DdlInsideRunAs);
            }
            _ => {}
        }
        count += 1;
        if count > MAX_RUNAS_STATEMENTS {
            return Err(InteractiveSqlError::TooManyRunAsStatements);
        }
    }
    if count == 0 {
        return Err(InteractiveSqlError::EmptyRunAs);
    }
    Ok(count)
}

fn is_normalized_username(username: &str) -> bool {
    let bytes = username.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 128
        && (bytes[0].is_ascii_lowercase() || bytes[0] == b'_')
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

fn is_word(sql: &str, token: Token, expected: &str) -> bool {
    token.kind == TokenKind::Word && sql[token.start..token.end].eq_ignore_ascii_case(expected)
}

fn lex(sql: &str) -> Result<Vec<Token>, InteractiveSqlError> {
    let bytes = sql.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0usize;
    let mut line_start = true;
    let mut parentheses = 0usize;

    while index < bytes.len() {
        match bytes[index] {
            b' ' | b'\t' | b'\r' => index += 1,
            b'\n' => {
                index += 1;
                line_start = true;
            }
            b'-' if bytes.get(index + 1) == Some(&b'-') => {
                index += 2;
                while index < bytes.len() && bytes[index] != b'\n' {
                    index += 1;
                }
                if index < bytes.len() {
                    index += 1;
                    line_start = true;
                }
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                index += 2;
                let mut depth = 1usize;
                while index < bytes.len() && depth > 0 {
                    if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'*') {
                        depth += 1;
                        index += 2;
                    } else if bytes[index] == b'*' && bytes.get(index + 1) == Some(&b'/') {
                        depth -= 1;
                        index += 2;
                    } else {
                        if bytes[index] == b'\n' {
                            line_start = true;
                        }
                        index += 1;
                    }
                }
                if depth != 0 {
                    return Err(InteractiveSqlError::UnterminatedComment);
                }
            }
            quote @ (b'\'' | b'"') => {
                let start = index;
                let token_line_start = line_start;
                index += 1;
                let mut closed = false;
                while index < bytes.len() {
                    if bytes[index] == quote {
                        if bytes.get(index + 1) == Some(&quote) {
                            index += 2;
                        } else {
                            index += 1;
                            closed = true;
                            break;
                        }
                    } else {
                        index += 1;
                    }
                }
                if !closed {
                    return Err(InteractiveSqlError::UnterminatedString);
                }
                tokens.push(Token {
                    kind: TokenKind::Quoted,
                    start,
                    end: index,
                    line_start: token_line_start,
                });
                line_start = false;
            }
            b';' => {
                if parentheses != 0 {
                    return Err(InteractiveSqlError::UnbalancedParentheses);
                }
                tokens.push(Token {
                    kind: TokenKind::Semicolon,
                    start: index,
                    end: index + 1,
                    line_start,
                });
                index += 1;
                line_start = true;
            }
            byte if is_word_start(byte) => {
                let start = index;
                let token_line_start = line_start;
                index += 1;
                while bytes.get(index).is_some_and(|next| is_word_continue(*next)) {
                    index += 1;
                }
                tokens.push(Token {
                    kind: TokenKind::Word,
                    start,
                    end: index,
                    line_start: token_line_start,
                });
                line_start = false;
            }
            byte => {
                let start = index;
                let token_line_start = line_start;
                if byte == b'(' {
                    parentheses = parentheses.saturating_add(1);
                } else if byte == b')' {
                    if parentheses == 0 {
                        return Err(InteractiveSqlError::UnbalancedParentheses);
                    }
                    parentheses -= 1;
                }
                index += 1;
                tokens.push(Token {
                    kind: TokenKind::Punctuation,
                    start,
                    end: index,
                    line_start: token_line_start,
                });
                line_start = false;
            }
        }
    }
    if parentheses != 0 {
        return Err(InteractiveSqlError::UnbalancedParentheses);
    }
    Ok(tokens)
}

fn is_word_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

fn is_word_continue(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

#[cfg(test)]
mod tests {
    use super::{
        InteractiveSqlError as Error, InteractiveStatement as Statement, MAX_INTERACTIVE_SQL_BYTES,
        MAX_RUNAS_STATEMENTS, contains_parameter_placeholder, parse_interactive_sql,
    };

    #[test]
    fn parses_transaction_commands_and_top_level_queries_in_order() {
        assert_eq!(
            parse_interactive_sql("BEGIN; SELECT 1; COMMIT; ROLLBACK").unwrap(),
            vec![
                Statement::Begin,
                Statement::Query("SELECT 1"),
                Statement::Commit,
                Statement::Rollback,
            ]
        );
    }

    #[test]
    fn runas_is_one_block_and_ignores_terminator_inside_literals_and_comments() {
        let sql = "RUNAS app_user\nSELECT 'ENDRUNAS;'; -- ENDRUNAS\nSELECT 2\nENDRUNAS;";
        let parsed = parse_interactive_sql(sql).unwrap();
        assert_eq!(
            parsed,
            vec![Statement::RunAs {
                username: "app_user",
                sql: "\nSELECT 'ENDRUNAS;'; -- ENDRUNAS\nSELECT 2\n",
                statement_count: 2,
            }]
        );
    }

    #[test]
    fn recognizes_ddl_for_control_routing() {
        assert_eq!(
            parse_interactive_sql("-- header\nCREATE TABLE app.t (id INTEGER)").unwrap(),
            vec![Statement::Ddl("CREATE TABLE app.t (id INTEGER)")]
        );
    }

    #[test]
    fn detects_parameters_only_outside_strings_and_comments() {
        assert!(contains_parameter_placeholder(
            "SELECT id FROM t WHERE id = $12"
        ));
        assert!(!contains_parameter_placeholder(
            "SELECT '$1', \"$2\" FROM t -- $3\n"
        ));
        assert!(!contains_parameter_placeholder(
            "SELECT 1 /* $4 /* $5 */ */"
        ));
    }

    #[test]
    fn rejects_invalid_runas_forms_and_nested_transaction_commands() {
        for (sql, expected) in [
            ("RUNAS app_user\nENDRUNAS", Error::EmptyRunAs),
            (
                "RUNAS BadName\nSELECT 1\nENDRUNAS",
                Error::InvalidRunAsUsername,
            ),
            ("RUNAS app_user\nSELECT 1", Error::MissingEndRunAs),
            (
                "RUNAS app_user\nBEGIN\nENDRUNAS",
                Error::TransactionControlInsideRunAs,
            ),
            (
                "RUNAS app_user\nCREATE TABLE t (id INTEGER)\nENDRUNAS",
                Error::DdlInsideRunAs,
            ),
            (
                "RUNAS app_user\nRUNAS other\nSELECT 1\nENDRUNAS\nENDRUNAS",
                Error::NestedRunAs,
            ),
            (
                "RUNAS app_user\nSELECT 1\nENDRUNAS SELECT 2",
                Error::InvalidEndRunAs,
            ),
        ] {
            assert_eq!(parse_interactive_sql(sql), Err(expected), "{sql}");
        }
        assert_eq!(
            parse_interactive_sql("BEGIN WORK"),
            Err(Error::TransactionControlArguments)
        );
    }

    #[test]
    fn rejects_unterminated_lexical_constructs_and_embedded_nul() {
        assert_eq!(
            parse_interactive_sql("SELECT 'unfinished"),
            Err(Error::UnterminatedString)
        );
        assert_eq!(
            parse_interactive_sql("SELECT 1 /* unfinished"),
            Err(Error::UnterminatedComment)
        );
        assert_eq!(
            parse_interactive_sql("SELECT (1; SELECT 2"),
            Err(Error::UnbalancedParentheses)
        );
        assert_eq!(parse_interactive_sql("SELECT 1\0"), Err(Error::EmbeddedNul));
    }

    #[test]
    fn enforces_input_and_runas_statement_bounds() {
        assert_eq!(
            parse_interactive_sql(&" ".repeat(MAX_INTERACTIVE_SQL_BYTES + 1)),
            Err(Error::TooLarge)
        );
        let body = std::iter::repeat_n("SELECT 1", MAX_RUNAS_STATEMENTS + 1)
            .collect::<Vec<_>>()
            .join(";\n");
        let sql = format!("RUNAS app_user\n{body}\nENDRUNAS");
        assert_eq!(
            parse_interactive_sql(&sql),
            Err(Error::TooManyRunAsStatements)
        );
    }

    #[test]
    fn endrunas_must_be_a_standalone_line_terminator() {
        assert_eq!(
            parse_interactive_sql("RUNAS app_user\nSELECT 1\nENDRUNAS + 2"),
            Err(Error::InvalidEndRunAs)
        );
    }
}
