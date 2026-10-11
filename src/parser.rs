use crate::diagnostic::Diagnostic;
use crate::lexer::{Token, TokenKind, unexpected};
use std::collections::HashSet;

pub use crate::ast::*;

impl From<&Token> for Span {
    fn from(token: &Token) -> Self {
        Self {
            line: token.line,
            column: token.column,
            end_column: token.end_column,
        }
    }
}

pub struct Parser {
    tokens: Vec<Token>,
    current: usize,
    struct_names: HashSet<String>,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        let struct_names = tokens
            .windows(2)
            .filter_map(|pair| match (&pair[0].kind, &pair[1].kind) {
                (TokenKind::Struct, TokenKind::Ident(name)) => Some(name.clone()),
                _ => None,
            })
            .collect();
        Self {
            tokens,
            current: 0,
            struct_names,
        }
    }

    pub fn parse_program(mut self) -> Result<Program, Diagnostic> {
        let mut enums = Vec::new();
        let mut structs = Vec::new();
        let mut functions = Vec::new();
        while !self.at(&TokenKind::Eof) {
            if self.at(&TokenKind::Enum) {
                enums.push(self.parse_enum()?);
            } else if self.at(&TokenKind::Struct) {
                structs.push(self.parse_struct()?);
            } else {
                functions.push(self.parse_function()?);
            }
        }
        if functions.is_empty() && enums.is_empty() && structs.is_empty() {
            return Err("program must contain at least one declaration".into());
        }
        Ok(Program {
            enums,
            structs,
            functions,
        })
    }

    fn parse_enum(&mut self) -> Result<EnumDecl, Diagnostic> {
        let start = self.consume(&TokenKind::Enum, "`enum`")?;
        let name = self.take_ident("enum name")?;
        self.consume(&TokenKind::LBrace, "`{` after enum name")?;
        let mut variants = Vec::new();
        while !self.at(&TokenKind::RBrace) && !self.at(&TokenKind::Eof) {
            let variant_token = self.peek().clone();
            let variant_name = self.take_ident("enum variant name")?;
            let payload = if self.take(&TokenKind::LParen) {
                let mut payload = Vec::new();
                if self.at(&TokenKind::RParen) {
                    return Err(unexpected(self.peek(), "at least one enum payload type"));
                }
                loop {
                    payload.push(self.parse_type()?);
                    if !self.take(&TokenKind::Comma) || self.at(&TokenKind::RParen) {
                        break;
                    }
                }
                self.consume(&TokenKind::RParen, "`)` after enum payload type")?;
                payload
            } else {
                Vec::new()
            };
            variants.push(EnumVariant {
                name: variant_name,
                payload,
                span: Span::from(&variant_token),
            });
            if !self.take(&TokenKind::Comma) && !self.at(&TokenKind::RBrace) {
                return Err(unexpected(self.peek(), "`,` or `}` after enum variant"));
            }
        }
        if variants.is_empty() {
            return Err(unexpected(self.peek(), "at least one enum variant"));
        }
        self.consume(&TokenKind::RBrace, "`}` after enum variants")?;
        Ok(EnumDecl {
            name,
            variants,
            span: Span::from(&start),
        })
    }

    fn parse_struct(&mut self) -> Result<StructDecl, Diagnostic> {
        let start = self.consume(&TokenKind::Struct, "`struct`")?;
        let name = self.take_ident("struct name")?;
        self.consume(&TokenKind::LBrace, "`{` after struct name")?;
        let mut fields = Vec::new();
        while !self.at(&TokenKind::RBrace) && !self.at(&TokenKind::Eof) {
            let field_token = self.peek().clone();
            let field_name = self.take_ident("struct field name")?;
            self.consume(&TokenKind::Colon, "`:` after struct field name")?;
            let ty = self.parse_type()?;
            fields.push(StructField {
                name: field_name,
                ty,
                span: Span::from(&field_token),
            });
            if !self.take(&TokenKind::Comma) && !self.at(&TokenKind::RBrace) {
                return Err(unexpected(self.peek(), "`,` or `}` after struct field"));
            }
        }
        if fields.is_empty() {
            return Err(unexpected(self.peek(), "at least one struct field"));
        }
        self.consume(&TokenKind::RBrace, "`}` after struct fields")?;
        Ok(StructDecl {
            name,
            fields,
            span: Span::from(&start),
        })
    }

    fn parse_function(&mut self) -> Result<Function, Diagnostic> {
        let start = self.consume(&TokenKind::Fn, "`fn`")?;
        let name = self.take_ident("function name")?;
        self.consume(&TokenKind::LParen, "`(`")?;
        let mut params = Vec::new();
        if !self.at(&TokenKind::RParen) {
            loop {
                let param_name = self.take_ident("parameter name")?;
                let param_span = Span::from(self.previous());
                self.consume(&TokenKind::Colon, "`:`")?;
                let ty = self.parse_type()?;
                params.push(Param {
                    name: param_name,
                    ty,
                    span: param_span,
                });
                if !self.take(&TokenKind::Comma) {
                    break;
                }
                if self.at(&TokenKind::RParen) {
                    break;
                }
            }
        }
        self.consume(&TokenKind::RParen, "`)`")?;
        self.consume(&TokenKind::Arrow, "`->`")?;
        let return_type = self.parse_type()?;
        let body = self.parse_block()?;
        Ok(Function {
            name,
            params,
            return_type,
            body,
            span: Span::from(&start),
        })
    }

    fn parse_type(&mut self) -> Result<Type, Diagnostic> {
        if self.take(&TokenKind::I64) {
            Ok(Type::I64)
        } else if self.take(&TokenKind::Bool) {
            Ok(Type::Bool)
        } else if self.take(&TokenKind::Str) {
            Ok(Type::Str)
        } else if self.take(&TokenKind::Unit) {
            Ok(Type::Unit)
        } else if self.take(&TokenKind::LBracket) {
            let element = self.parse_type()?;
            self.consume(&TokenKind::Semicolon, "`;` in array type")?;
            let length = match self.advance().kind {
                TokenKind::Integer(length) if length <= u32::MAX as u64 => length as u32,
                TokenKind::Integer(_) => {
                    return Err(unexpected(
                        self.previous(),
                        "array length no greater than 4294967295",
                    ));
                }
                _ => return Err(unexpected(self.previous(), "array length")),
            };
            self.consume(&TokenKind::RBracket, "`]`")?;
            Ok(Type::Array(Box::new(element), length))
        } else if let TokenKind::Ident(name) = &self.peek().kind {
            let name = name.clone();
            self.advance();
            // Named types are resolved after all declarations are collected.
            Ok(Type::Named(name))
        } else {
            Err(unexpected(
                self.peek(),
                "type `i64`, `bool`, `str`, `unit`, a named type, or `[element; length]`",
            ))
        }
    }

    fn parse_block(&mut self) -> Result<Block, Diagnostic> {
        self.consume(&TokenKind::LBrace, "`{`")?;
        let mut statements = Vec::new();
        let mut tail = None;
        while !self.at(&TokenKind::RBrace) && !self.at(&TokenKind::Eof) {
            if self.at(&TokenKind::Let) || self.at(&TokenKind::Const) {
                let immutable = self.at(&TokenKind::Const);
                let start = self.advance().clone();
                let name = self.take_ident("binding name")?;
                let ty = if self.take(&TokenKind::Colon) {
                    Some(self.parse_type()?)
                } else {
                    None
                };
                self.consume(&TokenKind::Equal, "`=`")?;
                let value = self.parse_expression(0)?;
                self.consume(&TokenKind::Semicolon, "`;`")?;
                statements.push(Stmt::Let {
                    name,
                    ty,
                    value,
                    immutable,
                    span: Span::from(&start),
                });
            } else if self.at(&TokenKind::If) {
                let if_token = self.advance().clone();
                let condition = self.parse_expression(0)?;
                let then_block = self.parse_block()?;
                let else_block = if self.take(&TokenKind::Else) {
                    Some(self.parse_block()?)
                } else {
                    None
                };
                if else_block
                    .as_ref()
                    .is_some_and(|block| then_block.tail.is_some() && block.tail.is_some())
                    && (self.at(&TokenKind::RBrace) || self.at(&TokenKind::Semicolon))
                {
                    let else_block = else_block.expect("checked above");
                    let expression = Expr::If {
                        condition: Box::new(condition),
                        then_block: Box::new(then_block),
                        else_block: Box::new(else_block),
                        span: Span::from(&if_token),
                    };
                    if self.take(&TokenKind::Semicolon) {
                        statements.push(Stmt::Expr(expression));
                    } else {
                        tail = Some(expression);
                        break;
                    }
                } else {
                    statements.push(Stmt::If {
                        condition,
                        then_block,
                        else_block,
                    });
                }
            } else if self.take(&TokenKind::While) {
                let condition = self.parse_expression(0)?;
                let body = self.parse_block()?;
                statements.push(Stmt::While { condition, body });
            } else if self.take(&TokenKind::For) {
                let name = self.take_ident("loop variable")?;
                self.consume(&TokenKind::In, "`in`")?;
                let iterable = self.parse_expression(0)?;
                if self.take(&TokenKind::DotDot) {
                    let end = self.parse_expression(0)?;
                    let body = self.parse_block()?;
                    statements.push(Stmt::For {
                        name,
                        start: iterable,
                        end,
                        body,
                    });
                } else {
                    let span = span_of_expr_parser(&iterable);
                    let body = self.parse_block()?;
                    statements.push(Stmt::ForArray {
                        name,
                        array: iterable,
                        body,
                        span,
                    });
                }
            } else if self.take(&TokenKind::Break) {
                let span = Span::from(self.previous());
                let value = if self.at(&TokenKind::Semicolon) {
                    None
                } else {
                    Some(self.parse_expression(0)?)
                };
                self.consume(&TokenKind::Semicolon, "`;`")?;
                statements.push(Stmt::Break { value, span });
            } else if self.take(&TokenKind::Continue) {
                let span = Span::from(self.previous());
                self.consume(&TokenKind::Semicolon, "`;`")?;
                statements.push(Stmt::Continue { span });
            } else if self.take(&TokenKind::Return) {
                let span = Span::from(self.previous());
                let value = if self.at(&TokenKind::Semicolon) {
                    None
                } else {
                    Some(self.parse_expression(0)?)
                };
                self.consume(&TokenKind::Semicolon, "`;`")?;
                statements.push(Stmt::Return { value, span });
            } else if let (TokenKind::Ident(name), TokenKind::Equal) =
                (&self.peek().kind, &self.peek_n(1).kind)
            {
                let name = name.clone();
                let assignment = self.advance().clone();
                let span = Span::from(&assignment);
                self.advance();
                let value = self.parse_expression(0)?;
                self.consume(&TokenKind::Semicolon, "`;`")?;
                statements.push(Stmt::Assign { name, value, span });
            } else {
                let expr = self.parse_expression(0)?;
                if self.take(&TokenKind::Equal) {
                    let assignment_span = Span::from(self.previous());
                    let value = self.parse_expression(0)?;
                    self.consume(&TokenKind::Semicolon, "`;`")?;
                    statements.push(Stmt::AssignPlace {
                        target: expr,
                        value,
                        span: assignment_span,
                    });
                } else if self.take(&TokenKind::Semicolon) {
                    statements.push(Stmt::Expr(expr));
                } else if self.at(&TokenKind::RBrace) {
                    tail = Some(expr);
                    break;
                } else {
                    return Err(unexpected(self.peek(), "`;` or `}` after expression"));
                }
            }
        }
        self.consume(&TokenKind::RBrace, "`}`")?;
        Ok(Block { statements, tail })
    }

    fn parse_expression(&mut self, min_precedence: u8) -> Result<Expr, Diagnostic> {
        let mut left = self.parse_prefix()?;
        loop {
            if self.at(&TokenKind::LBracket) {
                let span = Span::from(self.advance());
                let index = self.parse_expression(0)?;
                self.consume(&TokenKind::RBracket, "`]` after array index")?;
                left = Expr::Index {
                    array: Box::new(left),
                    index: Box::new(index),
                    span,
                };
                continue;
            }
            if self.at(&TokenKind::Dot) {
                let span = Span::from(self.advance());
                let name = self.take_ident("field name after `.`")?;
                left = Expr::Field {
                    value: Box::new(left),
                    name,
                    span,
                };
                continue;
            }
            let Some((op, precedence)) = self.binary_operator() else {
                break;
            };
            if precedence < min_precedence {
                break;
            }
            let operator = self.advance().clone();
            let right = self.parse_expression(precedence + 1)?;
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
                span: Span::from(&operator),
            };
        }
        Ok(left)
    }

    fn parse_prefix(&mut self) -> Result<Expr, Diagnostic> {
        if self.at(&TokenKind::Minus) {
            let operator = self.advance().clone();
            return Ok(Expr::Unary {
                op: UnaryOp::Negate,
                value: Box::new(self.parse_expression(11)?),
                span: Span::from(&operator),
            });
        }
        if self.at(&TokenKind::Bang) {
            let operator = self.advance().clone();
            return Ok(Expr::Unary {
                op: UnaryOp::Not,
                value: Box::new(self.parse_expression(11)?),
                span: Span::from(&operator),
            });
        }
        let token = self.advance().clone();
        let span = Span::from(&token);
        match token.kind {
            TokenKind::Integer(n) => Ok(Expr::Integer(n, span)),
            TokenKind::True => Ok(Expr::Bool(true, span)),
            TokenKind::False => Ok(Expr::Bool(false, span)),
            TokenKind::StringLit(value) => Ok(Expr::String(value, span)),
            TokenKind::LBracket => {
                let mut elements = Vec::new();
                if !self.at(&TokenKind::RBracket) {
                    loop {
                        elements.push(self.parse_expression(0)?);
                        if !self.take(&TokenKind::Comma) {
                            break;
                        }
                        if self.at(&TokenKind::RBracket) {
                            break;
                        }
                    }
                }
                self.consume(&TokenKind::RBracket, "`]` after array literal")?;
                Ok(Expr::Array(elements, span))
            }
            TokenKind::If => {
                let condition = self.parse_expression(0)?;
                let then_block = self.parse_block()?;
                self.consume(&TokenKind::Else, "`else` in an `if` expression")?;
                let else_block = self.parse_block()?;
                Ok(Expr::If {
                    condition: Box::new(condition),
                    then_block: Box::new(then_block),
                    else_block: Box::new(else_block),
                    span,
                })
            }
            TokenKind::Match => self.parse_match(span),
            TokenKind::Loop => Ok(Expr::Loop {
                body: Box::new(self.parse_block()?),
                span,
            }),
            TokenKind::Ident(enum_name) if self.at(&TokenKind::DoubleColon) => {
                self.advance();
                let variant_name = self.take_ident("enum variant after `::`")?;
                let payload = if self.take(&TokenKind::LParen) {
                    let mut values = Vec::new();
                    if !self.at(&TokenKind::RParen) {
                        loop {
                            values.push(self.parse_expression(0)?);
                            if !self.take(&TokenKind::Comma) || self.at(&TokenKind::RParen) {
                                break;
                            }
                        }
                    } else {
                        return Err(unexpected(self.peek(), "at least one enum payload value"));
                    }
                    self.consume(&TokenKind::RParen, "`)` after enum payload")?;
                    Some(values)
                } else {
                    None
                };
                Ok(Expr::Variant {
                    enum_name,
                    variant_name,
                    payload,
                    span,
                })
            }
            TokenKind::Ident(name)
                if self.struct_names.contains(&name) && self.at(&TokenKind::LBrace) =>
            {
                self.advance();
                let mut fields = Vec::new();
                if !self.at(&TokenKind::RBrace) {
                    loop {
                        let field_token = self.peek().clone();
                        let field_name = self.take_ident("field name")?;
                        self.consume(&TokenKind::Colon, "`:` after field name")?;
                        let value = self.parse_expression(0)?;
                        fields.push((field_name, value, Span::from(&field_token)));
                        if !self.take(&TokenKind::Comma) {
                            break;
                        }
                        if self.at(&TokenKind::RBrace) {
                            break;
                        }
                    }
                }
                self.consume(&TokenKind::RBrace, "`}` after struct initializer")?;
                Ok(Expr::Struct { name, fields, span })
            }
            TokenKind::Ident(name) if self.take(&TokenKind::LParen) => {
                let mut args = Vec::new();
                if !self.at(&TokenKind::RParen) {
                    loop {
                        args.push(self.parse_expression(0)?);
                        if !self.take(&TokenKind::Comma) {
                            break;
                        }
                    }
                }
                self.consume(&TokenKind::RParen, "`)`")?;
                Ok(Expr::Call { name, args, span })
            }
            TokenKind::Ident(name) => Ok(Expr::Variable(name, span)),
            TokenKind::LParen => {
                let expr = self.parse_expression(0)?;
                self.consume(&TokenKind::RParen, "`)`")?;
                Ok(expr)
            }
            _ => Err(unexpected(&token, "expression")),
        }
    }

    fn parse_match(&mut self, span: Span) -> Result<Expr, Diagnostic> {
        let value = self.parse_expression(7)?;
        self.consume(&TokenKind::LBrace, "`{` before match arms")?;
        let mut arms = Vec::new();
        while !self.at(&TokenKind::RBrace) && !self.at(&TokenKind::Eof) {
            let start = self.peek().clone();
            let pattern = if matches!(&self.peek().kind, TokenKind::Ident(name) if name == "_") {
                self.advance();
                MatchPattern::Wildcard
            } else if self.take(&TokenKind::True) {
                MatchPattern::Bool(true)
            } else if self.take(&TokenKind::False) {
                MatchPattern::Bool(false)
            } else if self.take(&TokenKind::Minus) {
                let value = match self.advance().kind {
                    TokenKind::Integer(value) if value <= (i64::MAX as u64) + 1 => {
                        if value == (i64::MAX as u64) + 1 {
                            i64::MIN
                        } else {
                            -(value as i64)
                        }
                    }
                    _ => return Err(unexpected(self.previous(), "signed i64 match pattern")),
                };
                MatchPattern::I64(value)
            } else if let TokenKind::Integer(value) = &self.peek().kind {
                if *value > i64::MAX as u64 {
                    return Err(unexpected(self.peek(), "i64 match pattern"));
                }
                let value = *value;
                self.advance();
                MatchPattern::I64(value as i64)
            } else {
                let enum_name = self.take_ident("enum name in match pattern")?;
                self.consume(&TokenKind::DoubleColon, "`::` in match pattern")?;
                let variant_name = self.take_ident("enum variant in match pattern")?;
                let bindings = if self.take(&TokenKind::LParen) {
                    let mut bindings = Vec::new();
                    if !self.at(&TokenKind::RParen) {
                        loop {
                            if matches!(&self.peek().kind, TokenKind::Ident(name) if name == "_") {
                                self.advance();
                                bindings.push(None);
                            } else {
                                bindings.push(Some(self.take_ident("payload binding or `_`")?));
                            }
                            if !self.take(&TokenKind::Comma) || self.at(&TokenKind::RParen) {
                                break;
                            }
                        }
                    } else {
                        return Err(unexpected(self.peek(), "at least one enum payload binding"));
                    }
                    self.consume(&TokenKind::RParen, "`)` after payload pattern")?;
                    Some(bindings)
                } else {
                    None
                };
                MatchPattern::Variant {
                    enum_name,
                    variant_name,
                    bindings,
                }
            };
            self.consume(&TokenKind::Equal, "`=` in match arm")?;
            self.consume(&TokenKind::Greater, "`>` in match arm (`=>`)")?;
            let body = self.parse_block()?;
            arms.push(MatchArm {
                pattern,
                span: Span::from(&start),
                body,
            });
            if !self.take(&TokenKind::Comma) && !self.at(&TokenKind::RBrace) {
                return Err(unexpected(self.peek(), "`,` or `}` after match arm"));
            }
        }
        if arms.is_empty() {
            return Err(unexpected(self.peek(), "at least one match arm"));
        }
        self.consume(&TokenKind::RBrace, "`}` after match arms")?;
        Ok(Expr::Match {
            value: Box::new(value),
            arms,
            span,
        })
    }

    fn binary_operator(&self) -> Option<(BinaryOp, u8)> {
        Some(match self.peek().kind {
            TokenKind::OrOr => (BinaryOp::Or, 1),
            TokenKind::AndAnd => (BinaryOp::And, 2),
            TokenKind::Pipe => (BinaryOp::BitOr, 3),
            TokenKind::Caret => (BinaryOp::BitXor, 4),
            TokenKind::Amp => (BinaryOp::BitAnd, 5),
            TokenKind::EqualEqual => (BinaryOp::Equal, 6),
            TokenKind::BangEqual => (BinaryOp::NotEqual, 6),
            TokenKind::Less => (BinaryOp::Less, 7),
            TokenKind::LessEqual => (BinaryOp::LessEqual, 7),
            TokenKind::Greater => (BinaryOp::Greater, 7),
            TokenKind::GreaterEqual => (BinaryOp::GreaterEqual, 7),
            TokenKind::ShiftLeft => (BinaryOp::ShiftLeft, 8),
            TokenKind::ShiftRight => (BinaryOp::ShiftRight, 8),
            TokenKind::Plus => (BinaryOp::Add, 9),
            TokenKind::Minus => (BinaryOp::Subtract, 9),
            TokenKind::Star => (BinaryOp::Multiply, 10),
            TokenKind::Slash => (BinaryOp::Divide, 10),
            TokenKind::Percent => (BinaryOp::Remainder, 10),
            _ => return None,
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
}

fn span_of_expr_parser(expr: &Expr) -> Span {
    match expr {
        Expr::Integer(_, span)
        | Expr::Bool(_, span)
        | Expr::String(_, span)
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
