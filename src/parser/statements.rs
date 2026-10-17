use super::*;

impl Parser {
    pub(super) fn parse_block(&mut self) -> Result<Block, Diagnostic> {
        self.consume(&TokenKind::LBrace, "`{")?;
        let mut statements = Vec::new();
        let mut tail = None;
        while !self.at(&TokenKind::RBrace) && !self.at(&TokenKind::Eof) {
            let statement_start = self.current;
            let parsed = (|| {
                let statement = if self.at(&TokenKind::Let) || self.at(&TokenKind::Const) {
                    let immutable = self.at(&TokenKind::Const);
                    let start = self.advance().clone();
                    let pattern = self.parse_pattern()?;
                    let ty = if self.take(&TokenKind::Colon) {
                        Some(self.parse_type()?)
                    } else {
                        None
                    };
                    self.consume(&TokenKind::Equal, "`=`")?;
                    let value = self.parse_expression(0)?;
                    self.consume(&TokenKind::Semicolon, "`;`")?;
                    Stmt::Let {
                        pattern,
                        ty,
                        value,
                        immutable,
                        span: Span::from(&start),
                    }
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
                            Stmt::Expr(expression)
                        } else {
                            tail = Some(expression);
                            return Ok(None);
                        }
                    } else {
                        Stmt::If {
                            condition,
                            then_block,
                            else_block,
                        }
                    }
                } else if self.take(&TokenKind::While) {
                    let condition = self.parse_expression(0)?;
                    let body = self.parse_block()?;
                    Stmt::While { condition, body }
                } else if self.take(&TokenKind::For) {
                    let name = self.take_ident("loop variable")?;
                    self.consume(&TokenKind::In, "`in`")?;
                    let iterable = self.parse_expression(0)?;
                    if self.take(&TokenKind::DotDot) {
                        let end = self.parse_expression(0)?;
                        let body = self.parse_block()?;
                        Stmt::For {
                            name,
                            start: iterable,
                            end,
                            body,
                        }
                    } else {
                        let span = span_of_expr_parser(&iterable);
                        let body = self.parse_block()?;
                        Stmt::ForArray {
                            name,
                            array: iterable,
                            body,
                            span,
                        }
                    }
                } else if self.take(&TokenKind::Break) {
                    let span = Span::from(self.previous());
                    let value = if self.at(&TokenKind::Semicolon) {
                        None
                    } else {
                        Some(self.parse_expression(0)?)
                    };
                    self.consume(&TokenKind::Semicolon, "`;`")?;
                    Stmt::Break { value, span }
                } else if self.take(&TokenKind::Continue) {
                    let span = Span::from(self.previous());
                    self.consume(&TokenKind::Semicolon, "`;`")?;
                    Stmt::Continue { span }
                } else if self.take(&TokenKind::Return) {
                    let span = Span::from(self.previous());
                    let value = if self.at(&TokenKind::Semicolon) {
                        None
                    } else {
                        Some(self.parse_expression(0)?)
                    };
                    self.consume(&TokenKind::Semicolon, "`;`")?;
                    Stmt::Return { value, span }
                } else if let (TokenKind::Ident(name), TokenKind::Equal) =
                    (&self.peek().kind, &self.peek_n(1).kind)
                {
                    let name = name.clone();
                    let assignment = self.advance().clone();
                    let span = Span::from(&assignment);
                    self.advance();
                    let value = self.parse_expression(0)?;
                    self.consume(&TokenKind::Semicolon, "`;` after assignment")?;
                    Stmt::Assign { name, value, span }
                } else {
                    let expr = self.parse_expression(0)?;
                    if self.take(&TokenKind::Equal) {
                        let assignment_span = Span::from(self.previous());
                        let value = self.parse_expression(0)?;
                        self.consume(&TokenKind::Semicolon, "`;`")?;
                        Stmt::AssignPlace {
                            target: expr,
                            value,
                            span: assignment_span,
                        }
                    } else if self.take(&TokenKind::Semicolon) {
                        Stmt::Expr(expr)
                    } else if self.at(&TokenKind::RBrace) {
                        tail = Some(expr);
                        return Ok(None);
                    } else {
                        return Err(unexpected(self.peek(), "`;` or `}` after expression"));
                    }
                };
                Ok(Some(statement))
            })();

            match parsed {
                Ok(Some(statement)) => statements.push(statement),
                Ok(None) => break,
                Err(error) => {
                    self.report(error);
                    self.synchronize_statement();
                    // If parsing failed at the statement's first token, discard at least one
                    // token. Otherwise synchronization stopped at the next statement start.
                    if self.current == statement_start && !self.at(&TokenKind::RBrace) {
                        self.advance();
                    }
                }
            }
        }
        self.consume(&TokenKind::RBrace, "`}`")?;
        Ok(Block { statements, tail })
    }
}
