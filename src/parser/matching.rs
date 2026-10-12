use super::*;

impl Parser {
    pub(super) fn parse_match(&mut self, span: Span) -> Result<Expr, Diagnostic> {
        let was_parsing_match_value = self.parsing_match_value;
        self.parsing_match_value = true;
        let parsed_value = self.parse_expression(7);
        self.parsing_match_value = was_parsing_match_value;
        let value = parsed_value?;
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
                match self.advance().kind {
                    TokenKind::Integer(value) if value <= (i64::MAX as u64) + 1 => {
                        if value == (i64::MAX as u64) + 1 {
                            MatchPattern::I64(i64::MIN)
                        } else {
                            MatchPattern::I64(-(value as i64))
                        }
                    }
                    TokenKind::Float32(value) => MatchPattern::F32(-value),
                    TokenKind::Float(value) => MatchPattern::F64(-value),
                    _ => return Err(unexpected(self.previous(), "signed i64 match pattern")),
                }
            } else if let TokenKind::Integer(value) = &self.peek().kind {
                if *value > i64::MAX as u64 {
                    return Err(unexpected(self.peek(), "i64 match pattern"));
                }
                let value = *value;
                self.advance();
                MatchPattern::I64(value as i64)
            } else if let TokenKind::Float32(value) = &self.peek().kind {
                let value = *value;
                self.advance();
                MatchPattern::F32(value)
            } else if let TokenKind::Float(value) = &self.peek().kind {
                let value = *value;
                self.advance();
                MatchPattern::F64(value)
            } else {
                let mut path = vec![self.take_ident("enum name in match pattern")?];
                while self.take(&TokenKind::DoubleColon) {
                    path.push(self.take_ident("name after `::` in match pattern")?);
                }
                if path.len() < 2 {
                    return Err(unexpected(self.peek(), "`::` in match pattern"));
                }
                let variant_name = path.pop().expect("path has multiple segments");
                let enum_name = path.join("::");
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
}
