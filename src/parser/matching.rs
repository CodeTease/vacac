use super::*;

impl Parser {
    pub(super) fn parse_pattern(&mut self) -> Result<MatchPattern, Diagnostic> {
        if matches!(&self.peek().kind, TokenKind::Ident(name) if name == "_") {
            self.advance();
            return Ok(MatchPattern::Wildcard);
        }
        if self.take(&TokenKind::True) {
            return Ok(MatchPattern::Bool(true));
        }
        if self.take(&TokenKind::False) {
            return Ok(MatchPattern::Bool(false));
        }
        if self.take(&TokenKind::Minus) {
            return match self.advance().kind {
                TokenKind::Integer(value) if value <= (i64::MAX as u64) + 1 => {
                    if value == (i64::MAX as u64) + 1 {
                        Ok(MatchPattern::I64(i64::MIN))
                    } else {
                        Ok(MatchPattern::I64(-(value as i64)))
                    }
                }
                TokenKind::Float32(value) => Ok(MatchPattern::F32(-value)),
                TokenKind::Float(value) => Ok(MatchPattern::F64(-value)),
                _ => Err(unexpected(self.previous(), "signed numeric pattern")),
            };
        }
        match self.peek().kind.clone() {
            TokenKind::Integer(value) if value <= i64::MAX as u64 => {
                self.advance();
                Ok(MatchPattern::I64(value as i64))
            }
            TokenKind::Integer(_) => Err(unexpected(self.peek(), "i64 pattern")),
            TokenKind::Float32(value) => {
                self.advance();
                Ok(MatchPattern::F32(value))
            }
            TokenKind::Float(value) => {
                self.advance();
                Ok(MatchPattern::F64(value))
            }
            TokenKind::LParen => self.parse_parenthesized_pattern(),
            TokenKind::LBracket => self.parse_array_pattern(),
            TokenKind::Ident(_) => self.parse_named_pattern(),
            _ => Err(unexpected(self.peek(), "pattern")),
        }
    }

    fn parse_parenthesized_pattern(&mut self) -> Result<MatchPattern, Diagnostic> {
        self.advance();
        if self.take(&TokenKind::RParen) {
            return Err(unexpected(
                self.previous(),
                "at least one tuple pattern element; empty tuples are not supported",
            ));
        }
        let first = self.parse_pattern()?;
        if !self.take(&TokenKind::Comma) {
            self.consume(&TokenKind::RParen, "`)` after grouped pattern")?;
            return Ok(first);
        }
        let mut elements = vec![first];
        if !self.at(&TokenKind::RParen) {
            loop {
                elements.push(self.parse_pattern()?);
                if !self.take(&TokenKind::Comma) || self.at(&TokenKind::RParen) {
                    break;
                }
            }
        }
        self.consume(&TokenKind::RParen, "`)` after tuple pattern")?;
        Ok(MatchPattern::Tuple(elements))
    }

    fn parse_array_pattern(&mut self) -> Result<MatchPattern, Diagnostic> {
        self.advance();
        let mut elements = Vec::new();
        if !self.at(&TokenKind::RBracket) {
            loop {
                elements.push(self.parse_pattern()?);
                if !self.take(&TokenKind::Comma) || self.at(&TokenKind::RBracket) {
                    break;
                }
            }
        }
        self.consume(&TokenKind::RBracket, "`]` after array pattern")?;
        Ok(MatchPattern::Array(elements))
    }

    fn parse_named_pattern(&mut self) -> Result<MatchPattern, Diagnostic> {
        let mut path = vec![self.take_ident("pattern name")?];
        while self.take(&TokenKind::DoubleColon) {
            path.push(self.take_ident("name after `::` in pattern")?);
        }
        if self.take(&TokenKind::LParen) {
            if path.len() == 1 {
                let binding = path.pop().expect("path is non-empty");
                if !self.at(&TokenKind::RParen) {
                    return Err(unexpected(self.peek(), "`)` after grouped pattern"));
                }
                self.advance();
                return Ok(MatchPattern::Binding(binding));
            }
            let variant_name = path.pop().expect("non-empty path");
            let enum_name = path.join("::");
            let mut bindings = Vec::new();
            if self.at(&TokenKind::RParen) {
                return Err(unexpected(self.peek(), "at least one enum payload pattern"));
            }
            loop {
                bindings.push(self.parse_pattern()?);
                if !self.take(&TokenKind::Comma) || self.at(&TokenKind::RParen) {
                    break;
                }
            }
            self.consume(&TokenKind::RParen, "`)` after enum payload pattern")?;
            return Ok(MatchPattern::Variant {
                enum_name,
                variant_name,
                bindings: Some(bindings),
            });
        }
        if self.take(&TokenKind::LBrace) {
            let name = path.join("::");
            let mut fields = Vec::new();
            let mut rest = false;
            if !self.at(&TokenKind::RBrace) {
                loop {
                    if self.take(&TokenKind::DotDot) {
                        rest = true;
                        if self.take(&TokenKind::Comma) && !self.at(&TokenKind::RBrace) {
                            return Err(unexpected(self.peek(), "`}` after struct pattern `..`"));
                        }
                        if !self.at(&TokenKind::RBrace) {
                            return Err(unexpected(self.peek(), "`}` after struct pattern `..`"));
                        }
                        break;
                    }
                    let field_token = self.peek().clone();
                    let field_name = self.take_ident("struct field pattern")?;
                    let pattern = if self.take(&TokenKind::Colon) {
                        self.parse_pattern()?
                    } else {
                        MatchPattern::Binding(field_name.clone())
                    };
                    fields.push((field_name, pattern, Span::from(&field_token)));
                    if !self.take(&TokenKind::Comma) {
                        break;
                    }
                    if self.at(&TokenKind::RBrace) {
                        break;
                    }
                }
            }
            self.consume(&TokenKind::RBrace, "`}` after struct pattern")?;
            return Ok(MatchPattern::Struct { name, fields, rest });
        }
        if path.len() > 1 {
            let variant_name = path.pop().expect("path has multiple segments");
            return Ok(MatchPattern::Variant {
                enum_name: path.join("::"),
                variant_name,
                bindings: None,
            });
        }
        Ok(MatchPattern::Binding(
            path.pop().expect("path is non-empty"),
        ))
    }

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
            let pattern = self.parse_pattern()?;
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
