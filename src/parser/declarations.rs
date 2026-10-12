use super::*;

impl Parser {
    pub(super) fn parse_enum(&mut self, is_public: bool) -> Result<EnumDecl, Diagnostic> {
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
                self.consume(&TokenKind::RParen, "`)`")?;
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
            is_public,
            module: self.module.clone(),
        })
    }

    pub(super) fn parse_struct(&mut self, is_public: bool) -> Result<StructDecl, Diagnostic> {
        let start = self.consume(&TokenKind::Struct, "`struct`")?;
        let name = self.take_ident("struct name")?;
        let generic_params = self.parse_generic_parameters()?;
        let previous_type_parameters = std::mem::replace(
            &mut self.type_parameters,
            generic_params.iter().cloned().collect(),
        );
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
            self.type_parameters = previous_type_parameters;
            return Err(unexpected(self.peek(), "at least one struct field"));
        }
        self.consume(&TokenKind::RBrace, "`}` after struct fields")?;
        self.type_parameters = previous_type_parameters;
        Ok(StructDecl {
            name,
            generic_params,
            fields,
            span: Span::from(&start),
            is_public,
            module: self.module.clone(),
        })
    }

    pub(super) fn parse_function(&mut self, is_public: bool) -> Result<Function, Diagnostic> {
        let start = self.consume(&TokenKind::Fn, "`fn`")?;
        let name = self.take_ident("function name")?;
        let generic_params = self.parse_generic_parameters()?;
        let previous_type_parameters = std::mem::replace(
            &mut self.type_parameters,
            generic_params.iter().cloned().collect(),
        );
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
        self.type_parameters = previous_type_parameters;
        Ok(Function {
            name,
            generic_params,
            params,
            return_type,
            body,
            span: Span::from(&start),
            is_public,
            module: self.module.clone(),
        })
    }

    pub(super) fn parse_type(&mut self) -> Result<Type, Diagnostic> {
        if self.take(&TokenKind::LParen) {
            if self.take(&TokenKind::RParen) {
                return Err(unexpected(
                    self.previous(),
                    "at least one tuple type element; empty tuple types are not supported",
                ));
            }
            let first = self.parse_type()?;
            if !self.take(&TokenKind::Comma) {
                self.consume(&TokenKind::RParen, "`)` after grouped type")?;
                return Ok(first);
            }
            let mut elements = vec![first];
            if !self.at(&TokenKind::RParen) {
                loop {
                    elements.push(self.parse_type()?);
                    if !self.take(&TokenKind::Comma) || self.at(&TokenKind::RParen) {
                        break;
                    }
                }
            }
            self.consume(&TokenKind::RParen, "`)` after tuple type")?;
            Ok(Type::Tuple(elements))
        } else if self.take(&TokenKind::I64) {
            Ok(Type::I64)
        } else if self.take(&TokenKind::F32) {
            Ok(Type::F32)
        } else if self.take(&TokenKind::F64) {
            Ok(Type::F64)
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
        } else if matches!(&self.peek().kind, TokenKind::Ident(_)) {
            let first = self.take_ident("type name")?;
            if self.type_parameters.contains(&first) {
                return Ok(Type::Parameter(first));
            }
            if first == "Vec" {
                self.consume(&TokenKind::Less, "`<` after `Vec`")?;
                let element = self.parse_type()?;
                self.consume_type_greater()?;
                return Ok(Type::Vector(Box::new(element)));
            }
            let mut segments = vec![first];
            while self.take(&TokenKind::DoubleColon) {
                segments.push(self.take_ident("type name after `::`")?);
            }
            let name = segments.join("::");
            if self.take(&TokenKind::Less) {
                let mut arguments = Vec::new();
                if self.at(&TokenKind::Greater) || self.at(&TokenKind::ShiftRight) {
                    return Err(unexpected(
                        self.peek(),
                        "at least one generic type argument",
                    ));
                }
                loop {
                    arguments.push(self.parse_type()?);
                    if !self.take(&TokenKind::Comma) {
                        break;
                    }
                    if self.at(&TokenKind::Greater) || self.at(&TokenKind::ShiftRight) {
                        break;
                    }
                }
                self.consume_type_greater()?;
                Ok(Type::Applied(name, arguments))
            } else {
                Ok(Type::Named(name))
            }
        } else {
            Err(unexpected(
                self.peek(),
                "type `i64`, `f32`, `f64`, `bool`, `str`, `unit`, a named type, tuple type, or `[element; length]`",
            ))
        }
    }

    fn parse_generic_parameters(&mut self) -> Result<Vec<String>, Diagnostic> {
        if !self.take(&TokenKind::Less) {
            return Ok(Vec::new());
        }
        let mut parameters = Vec::new();
        loop {
            let token = self.peek().clone();
            let parameter = self.take_ident("generic parameter")?;
            if parameters.contains(&parameter) {
                return Err(Diagnostic::at(
                    Span::from(&token).into(),
                    format!("generic parameter `{parameter}` is defined more than once"),
                ));
            }
            parameters.push(parameter);
            if !self.take(&TokenKind::Comma) {
                break;
            }
            if self.at(&TokenKind::Greater) {
                return Err(unexpected(self.peek(), "generic parameter after `,`"));
            }
        }
        self.consume_type_greater()?;
        Ok(parameters)
    }
}
