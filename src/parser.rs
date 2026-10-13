use crate::diagnostic::Diagnostic;
use crate::lexer::{Token, TokenKind, unexpected};
use std::collections::HashSet;

pub use crate::ast::*;

mod declarations;
mod expressions;
mod matching;
mod statements;

impl From<&Token> for Span {
    fn from(token: &Token) -> Self {
        Self {
            line: token.line,
            column: token.column,
            end_column: token.end_column,
            file: token.file,
        }
    }
}

pub struct Parser {
    tokens: Vec<Token>,
    current: usize,
    struct_names: HashSet<String>,
    enum_names: HashSet<String>,
    module: String,
    parsing_match_value: bool,
    type_close_debt: usize,
    type_parameters: HashSet<String>,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self::new_in_module(tokens, String::new())
    }

    pub fn new_in_module(tokens: Vec<Token>, module: String) -> Self {
        let struct_names = tokens
            .windows(2)
            .filter_map(|pair| match (&pair[0].kind, &pair[1].kind) {
                (TokenKind::Struct, TokenKind::Ident(name)) => Some(name.clone()),
                _ => None,
            })
            .collect();
        let enum_names = tokens
            .windows(2)
            .filter_map(|pair| match (&pair[0].kind, &pair[1].kind) {
                (TokenKind::Enum, TokenKind::Ident(name)) => Some(name.clone()),
                _ => None,
            })
            .collect();
        Self {
            tokens,
            current: 0,
            struct_names,
            enum_names,
            module,
            parsing_match_value: false,
            type_close_debt: 0,
            type_parameters: HashSet::new(),
        }
    }

    pub fn parse_program(mut self) -> Result<Program, Diagnostic> {
        let mut enums = Vec::new();
        let mut structs = Vec::new();
        let mut globals = Vec::new();
        let mut functions = Vec::new();
        let mut extern_functions = Vec::new();
        let mut modules = Vec::new();
        while !self.at(&TokenKind::Eof) {
            let is_public = self.take(&TokenKind::Pub);
            if self.take(&TokenKind::Mod) {
                if is_public {
                    return Err(unexpected(
                        self.previous(),
                        "a declaration after `pub` (`pub mod` is not supported)",
                    ));
                }
                let token = self.previous().clone();
                let name = self.take_ident("module name")?;
                self.consume(&TokenKind::Semicolon, "`;` after module declaration")?;
                modules.push(ModuleDecl {
                    name,
                    span: Span::from(&token),
                });
                continue;
            }
            if self.at(&TokenKind::Enum) {
                enums.push(self.parse_enum(is_public)?);
            } else if self.at(&TokenKind::Struct) {
                structs.push(self.parse_struct(is_public)?);
            } else if self.at(&TokenKind::Const) || self.at(&TokenKind::Let) {
                let mutable = self.at(&TokenKind::Let);
                globals.push(self.parse_global(is_public, mutable)?);
            } else if self.at(&TokenKind::Extern) {
                extern_functions.push(self.parse_extern_function(is_public)?);
            } else {
                if !self.at(&TokenKind::Fn) {
                    return Err(unexpected(
                        self.peek(),
                        "`fn`, `extern`, `struct`, `enum`, `const`, `let`, or `mod` declaration",
                    ));
                }
                functions.push(self.parse_function(is_public)?);
            }
        }
        if functions.is_empty()
            && extern_functions.is_empty()
            && enums.is_empty()
            && structs.is_empty()
            && globals.is_empty()
            && modules.is_empty()
        {
            return Err("program must contain at least one declaration".into());
        }
        Ok(Program {
            enums,
            structs,
            globals,
            functions,
            extern_functions,
            modules,
            source_files: Vec::new(),
            source_texts: Vec::new(),
            functions_are_qualified: false,
        })
    }

    fn take_ident(&mut self, expected: &str) -> Result<String, Diagnostic> {
        match self.advance().kind.clone() {
            TokenKind::Ident(name) => Ok(name),
            _ => Err(unexpected(self.previous(), expected)),
        }
    }
    fn at(&self, kind: &TokenKind) -> bool {
        std::mem::discriminant(&self.peek().kind) == std::mem::discriminant(kind)
    }
    fn take(&mut self, kind: &TokenKind) -> bool {
        if self.at(kind) {
            self.advance();
            true
        } else {
            false
        }
    }
    fn consume(&mut self, kind: &TokenKind, expected: &str) -> Result<Token, Diagnostic> {
        if self.at(kind) {
            Ok(self.advance().clone())
        } else {
            Err(unexpected(self.peek(), expected))
        }
    }
    fn advance(&mut self) -> &Token {
        if !self.at(&TokenKind::Eof) {
            self.current += 1;
        }
        self.previous()
    }
    fn peek(&self) -> &Token {
        &self.tokens[self.current]
    }
    fn peek_n(&self, n: usize) -> &Token {
        self.tokens
            .get(self.current + n)
            .unwrap_or_else(|| self.tokens.last().unwrap())
    }
    fn previous(&self) -> &Token {
        &self.tokens[self.current.saturating_sub(1)]
    }

    pub(super) fn consume_type_greater(&mut self) -> Result<(), Diagnostic> {
        if self.type_close_debt > 0 {
            self.type_close_debt -= 1;
            return Ok(());
        }
        if self.take(&TokenKind::Greater) {
            return Ok(());
        }
        if self.take(&TokenKind::ShiftRight) {
            self.type_close_debt = 1;
            return Ok(());
        }
        Err(unexpected(self.peek(), "`>` after generic type argument"))
    }
}

fn span_of_expr_parser(expr: &Expr) -> Span {
    match expr {
        Expr::Integer(_, span)
        | Expr::Float32(_, span)
        | Expr::Float(_, span)
        | Expr::Bool(_, span)
        | Expr::String(_, span)
        | Expr::Tuple(_, span)
        | Expr::Array(_, span)
        | Expr::Variable(_, span)
        | Expr::Index { span, .. }
        | Expr::Loop { span, .. }
        | Expr::Field { span, .. }
        | Expr::Struct { span, .. }
        | Expr::Unary { span, .. }
        | Expr::Binary { span, .. }
        | Expr::Call { span, .. }
        | Expr::If { span, .. }
        | Expr::Match { span, .. }
        | Expr::Variant { span, .. } => *span,
    }
}
