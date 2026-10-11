use crate::diagnostic::{Diagnostic, SourceSpan};

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    Ident(String),
    Integer(u64),
    StringLit(String),
    Fn,
    Enum,
    Struct,
    Match,
    Let,
    Const,
    If,
    Else,
    While,
    Loop,
    For,
    In,
    Break,
    Continue,
    Return,
    True,
    False,
    I64,
    Bool,
    Str,
    Unit,
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    DotDot,
    Dot,
    Colon,
    DoubleColon,
    Comma,
    Semicolon,
    Arrow,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Bang,
    Equal,
    EqualEqual,
    BangEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Amp,
    Caret,
    Pipe,
    ShiftLeft,
    ShiftRight,
    AndAnd,
    OrOr,
    Eof,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    pub line: usize,
    pub column: usize,
    pub end_column: usize,
}

impl Token {
    fn describe(&self) -> String {
        match &self.kind {
            TokenKind::Ident(s) => format!("identifier `{s}`"),
            TokenKind::Integer(n) => format!("integer `{n}`"),
            TokenKind::StringLit(s) => format!("string literal {s:?}"),
            kind => format!("`{}`", token_name(kind)),
        }
    }
}

fn token_name(kind: &TokenKind) -> &'static str {
    match kind {
        TokenKind::Fn => "fn",
        TokenKind::Enum => "enum",
        TokenKind::Struct => "struct",
        TokenKind::Match => "match",
        TokenKind::Let => "let",
        TokenKind::Const => "const",
        TokenKind::If => "if",
        TokenKind::Else => "else",
        TokenKind::While => "while",
        TokenKind::Loop => "loop",
        TokenKind::For => "for",
        TokenKind::In => "in",
        TokenKind::Break => "break",
        TokenKind::Continue => "continue",
        TokenKind::Return => "return",
        TokenKind::True => "true",
        TokenKind::False => "false",
        TokenKind::I64 => "i64",
        TokenKind::Bool => "bool",
        TokenKind::Str => "str",
        TokenKind::Unit => "unit",
        TokenKind::LParen => "(",
        TokenKind::RParen => ")",
        TokenKind::LBrace => "{",
        TokenKind::RBrace => "}",
        TokenKind::LBracket => "[",
        TokenKind::RBracket => "]",
        TokenKind::DotDot => "..",
        TokenKind::Dot => ".",
        TokenKind::Colon => ":",
        TokenKind::DoubleColon => "::",
        TokenKind::Comma => ",",
        TokenKind::Semicolon => ";",
        TokenKind::Arrow => "->",
        TokenKind::Plus => "+",
        TokenKind::Minus => "-",
        TokenKind::Star => "*",
        TokenKind::Slash => "/",
        TokenKind::Percent => "%",
        TokenKind::Bang => "!",
        TokenKind::Equal => "=",
        TokenKind::EqualEqual => "==",
        TokenKind::BangEqual => "!=",
        TokenKind::Less => "<",
        TokenKind::LessEqual => "<=",
        TokenKind::Greater => ">",
        TokenKind::GreaterEqual => ">=",
        TokenKind::Amp => "&",
        TokenKind::Caret => "^",
        TokenKind::Pipe => "|",
        TokenKind::ShiftLeft => "<<",
        TokenKind::ShiftRight => ">>",
        TokenKind::AndAnd => "&&",
        TokenKind::OrOr => "||",
        TokenKind::Eof => "end of file",
        TokenKind::Ident(_) => "identifier",
        TokenKind::Integer(_) => "integer",
        TokenKind::StringLit(_) => "string literal",
    }
}

pub fn lex(input: &str) -> Result<Vec<Token>, Diagnostic> {
    let bytes = input.as_bytes();
    let (mut i, mut line, mut column) = (0usize, 1usize, 1usize);
    let mut tokens = Vec::new();
    while i < bytes.len() {
        let b = bytes[i];
        if b.is_ascii_whitespace() {
            if b == b'\n' {
                line += 1;
                column = 1;
            } else {
                column += 1;
            }
            i += 1;
            continue;
        }
        if b == b'/' && bytes.get(i + 1) == Some(&b'/') {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
                column += 1;
            }
            continue;
        }
        if b == b'/' && bytes.get(i + 1) == Some(&b'*') {
            let start_line = line;
            let start_column = column;
            i += 2;
            column += 2;
            let mut closed = false;
            while i < bytes.len() {
                if bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/') {
                    i += 2;
                    column += 2;
                    closed = true;
                    break;
                }
                if bytes[i] == b'\n' {
                    line += 1;
                    column = 1;
                    i += 1;
                } else {
                    i += 1;
                    column += 1;
                }
            }
            if !closed {
                return Err(Diagnostic::at(
                    SourceSpan {
                        line: start_line,
                        column: start_column,
                        end_column: column,
                    },
                    "unterminated block comment",
                ));
            }
            continue;
        }

        let start_line = line;
        let start_column = column;
        let token = if b.is_ascii_alphabetic() || b == b'_' {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
                column += 1;
            }
            let text = &input[start..i];
            match text {
                "fn" => TokenKind::Fn,
                "enum" => TokenKind::Enum,
                "struct" => TokenKind::Struct,
                "match" => TokenKind::Match,
                "let" => TokenKind::Let,
                "const" => TokenKind::Const,
                "if" => TokenKind::If,
                "else" => TokenKind::Else,
                "while" => TokenKind::While,
                "loop" => TokenKind::Loop,
                "for" => TokenKind::For,
                "in" => TokenKind::In,
                "break" => TokenKind::Break,
                "continue" => TokenKind::Continue,
                "return" => TokenKind::Return,
                "true" => TokenKind::True,
                "false" => TokenKind::False,
                "i64" => TokenKind::I64,
                "bool" => TokenKind::Bool,
                "str" => TokenKind::Str,
                "unit" => TokenKind::Unit,
                _ => TokenKind::Ident(text.to_owned()),
            }
        } else if b.is_ascii_digit() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
                column += 1;
            }
            let text = &input[start..i];
            TokenKind::Integer(text.parse::<u64>().map_err(|_| {
                Diagnostic::at(
                    SourceSpan {
                        line: start_line,
                        column: start_column,
                        end_column: column,
                    },
                    "integer literal is out of range",
                )
            })?)
        } else if b == b'"' {
            i += 1;
            column += 1;
            let mut value = Vec::new();
            loop {
                match bytes.get(i) {
                    None | Some(b'\n') => {
                        return Err(Diagnostic::at(
                            SourceSpan {
                                line: start_line,
                                column: start_column,
                                end_column: column,
                            },
                            "unterminated string literal",
                        ));
                    }
                    Some(b'"') => {
                        i += 1;
                        column += 1;
                        break;
                    }
                    Some(b'\\') => {
                        let escape_column = column;
                        let escaped = match bytes.get(i + 1) {
                            Some(b'n') => b'\n',
                            Some(b't') => b'\t',
                            Some(b'r') => b'\r',
                            Some(b'\\') => b'\\',
                            Some(b'"') => b'"',
                            Some(b'0') => {
                                return Err(Diagnostic::at(
                                    SourceSpan {
                                        line,
                                        column: escape_column,
                                        end_column: escape_column + 2,
                                    },
                                    "NUL characters are not supported in string literals",
                                ));
                            }
                            None | Some(b'\n') => {
                                return Err(Diagnostic::at(
                                    SourceSpan {
                                        line: start_line,
                                        column: start_column,
                                        end_column: column + 1,
                                    },
                                    "unterminated string literal",
                                ));
                            }
                            Some(_) => {
                                let escape = input[i + 1..].chars().next().unwrap_or('?');
                                return Err(Diagnostic::at(
                                    SourceSpan {
                                        line,
                                        column: escape_column,
                                        end_column: escape_column + 2,
                                    },
                                    format!(
                                        "unknown escape sequence `\\{}`",
                                        escape.escape_debug()
                                    ),
                                ));
                            }
                        };
                        value.push(escaped);
                        i += 2;
                        column += 2;
                    }
                    Some(&byte) => {
                        value.push(byte);
                        i += 1;
                        // Count columns in characters, not UTF-8 continuation bytes.
                        if byte & 0xC0 != 0x80 {
                            column += 1;
                        }
                    }
                }
            }
            TokenKind::StringLit(
                String::from_utf8(value).expect("string literal built from valid UTF-8 source"),
            )
        } else {
            let (kind, width) = match b {
                b'(' => (TokenKind::LParen, 1),
                b')' => (TokenKind::RParen, 1),
                b'{' => (TokenKind::LBrace, 1),
                b'}' => (TokenKind::RBrace, 1),
                b'[' => (TokenKind::LBracket, 1),
                b']' => (TokenKind::RBracket, 1),
                b'.' if bytes.get(i + 1) == Some(&b'.') => (TokenKind::DotDot, 2),
                b'.' => (TokenKind::Dot, 1),
                b':' if bytes.get(i + 1) == Some(&b':') => (TokenKind::DoubleColon, 2),
                b':' => (TokenKind::Colon, 1),
                b',' => (TokenKind::Comma, 1),
                b';' => (TokenKind::Semicolon, 1),
                b'+' => (TokenKind::Plus, 1),
                b'*' => (TokenKind::Star, 1),
                b'/' => (TokenKind::Slash, 1),
                b'%' => (TokenKind::Percent, 1),
                b'-' if bytes.get(i + 1) == Some(&b'>') => (TokenKind::Arrow, 2),
                b'-' => (TokenKind::Minus, 1),
                b'!' if bytes.get(i + 1) == Some(&b'=') => (TokenKind::BangEqual, 2),
                b'!' => (TokenKind::Bang, 1),
                b'=' if bytes.get(i + 1) == Some(&b'=') => (TokenKind::EqualEqual, 2),
                b'=' => (TokenKind::Equal, 1),
                b'<' if bytes.get(i + 1) == Some(&b'=') => (TokenKind::LessEqual, 2),
                b'<' if bytes.get(i + 1) == Some(&b'<') => (TokenKind::ShiftLeft, 2),
                b'<' => (TokenKind::Less, 1),
                b'>' if bytes.get(i + 1) == Some(&b'=') => (TokenKind::GreaterEqual, 2),
                b'>' if bytes.get(i + 1) == Some(&b'>') => (TokenKind::ShiftRight, 2),
                b'>' => (TokenKind::Greater, 1),
                b'&' if bytes.get(i + 1) == Some(&b'&') => (TokenKind::AndAnd, 2),
                b'&' => (TokenKind::Amp, 1),
                b'|' if bytes.get(i + 1) == Some(&b'|') => (TokenKind::OrOr, 2),
                b'|' => (TokenKind::Pipe, 1),
                b'^' => (TokenKind::Caret, 1),
                _ => {
                    return Err(Diagnostic::at(
                        SourceSpan {
                            line,
                            column,
                            end_column: column + 1,
                        },
                        format!("unexpected character `{}`", b as char),
                    ));
                }
            };
            i += width;
            column += width;
            kind
        };
        tokens.push(Token {
            kind: token,
            line: start_line,
            column: start_column,
            end_column: column,
        });
    }
    tokens.push(Token {
        kind: TokenKind::Eof,
        line,
        column,
        end_column: column,
    });
    Ok(tokens)
}

pub fn unexpected(token: &Token, expected: &str) -> Diagnostic {
    Diagnostic::at(
        SourceSpan {
            line: token.line,
            column: token.column,
            end_column: token.end_column,
        },
        format!("expected {expected}, found {}", token.describe()),
    )
}
