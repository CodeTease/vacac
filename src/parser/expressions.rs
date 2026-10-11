use super::*;

impl Parser {
    pub(super) fn parse_expression(&mut self, min_precedence: u8) -> Result<Expr, Diagnostic> {
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

    pub(super) fn parse_prefix(&mut self) -> Result<Expr, Diagnostic> {
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
            TokenKind::Ident(first) => {
                let mut path = vec![first];
                while self.at(&TokenKind::DoubleColon)
                    && matches!(&self.peek_n(1).kind, TokenKind::Ident(_))
                    && matches!(&self.peek_n(2).kind, TokenKind::DoubleColon)
                {
                    self.advance();
                    path.push(self.take_ident("name after `::`")?);
                }
                if self.at(&TokenKind::DoubleColon) {
                    self.advance();
                    path.push(self.take_ident("name after `::`")?);
                }
                let name = path.join("::");
                if path.len() >= 2 && self.take(&TokenKind::LParen) {
                    let mut args = Vec::new();
                    if !self.at(&TokenKind::RParen) {
                        loop {
                            args.push(self.parse_expression(0)?);
                            if !self.take(&TokenKind::Comma) || self.at(&TokenKind::RParen) {
                                break;
                            }
                        }
                    }
                    self.consume(&TokenKind::RParen, "`)`")?;
                    return Ok(Expr::Call { name, args, span });
                }
                if path.len() >= 2 && !self.parsing_match_value && self.at(&TokenKind::LBrace) {
                    let type_name = path.last().expect("path has at least two segments");
                    let enum_name = path
                        .get(path.len() - 2)
                        .expect("path has at least two segments");
                    if self.enum_names.contains(enum_name) && !self.struct_names.contains(type_name)
                    {
                        let variant_name = path.pop().expect("path has multiple segments");
                        return Ok(Expr::Variant {
                            enum_name: path.join("::"),
                            variant_name,
                            payload: None,
                            span,
                        });
                    }
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
                    return Ok(Expr::Struct { name, fields, span });
                }
                if path.len() >= 2 {
                    let variant_name = path.pop().expect("path has multiple segments");
                    let enum_name = path.join("::");
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
                } else if self.struct_names.contains(&name) && self.at(&TokenKind::LBrace) {
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
                } else if self.take(&TokenKind::LParen) {
                    let mut args = Vec::new();
                    if !self.at(&TokenKind::RParen) {
                        loop {
                            args.push(self.parse_expression(0)?);
                            if !self.take(&TokenKind::Comma) || self.at(&TokenKind::RParen) {
                                break;
                            }
                        }
                    }
                    self.consume(&TokenKind::RParen, "`)`")?;
                    Ok(Expr::Call { name, args, span })
                } else {
                    Ok(Expr::Variable(name, span))
                }
            }
            TokenKind::LParen => {
                let expr = self.parse_expression(0)?;
                self.consume(&TokenKind::RParen, "`)`")?;
                Ok(expr)
            }
            _ => Err(unexpected(&token, "expression")),
        }
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
}
