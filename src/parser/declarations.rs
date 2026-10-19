use super::*;

impl Parser {
    pub(super) fn parse_global(
        &mut self,
        is_public: bool,
        mutable: bool,
    ) -> Result<GlobalDecl, Diagnostic> {
        let start = self.advance().clone();
        let name = self.take_ident("global name")?;
        let ty = if self.take(&TokenKind::Colon) {
            Some(self.parse_type()?)
        } else {
            None
        };
        self.consume(&TokenKind::Equal, "`=` in global initializer")?;
        let value = self.parse_expression(0)?;
        self.consume(&TokenKind::Semicolon, "`;` after global initializer")?;
        Ok(GlobalDecl {
            name,
            ty,
            value,
            mutable,
            span: Span::from(&start),
            is_public,
            module: self.module.clone(),
        })
    }

    pub(super) fn parse_enum(&mut self, is_public: bool) -> Result<EnumDecl, Diagnostic> {
        let start = self.consume(&TokenKind::Enum, "`enum`")?;
        let name = self.take_ident("enum name")?;
        let generic_params = self.parse_generic_parameters()?;
        let previous_type_parameters = std::mem::replace(
            &mut self.type_parameters,
            generic_params.iter().cloned().collect(),
        );
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
            self.type_parameters = previous_type_parameters;
            return Err(unexpected(self.peek(), "at least one enum variant"));
        }
        self.consume(&TokenKind::RBrace, "`}` after enum variants")?;
        self.type_parameters = previous_type_parameters;
        Ok(EnumDecl {
            name,
            generic_params,
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
        let (generic_params, lifetime_params) = self.parse_function_generics()?;
        let type_parameter_names: HashSet<String> = generic_params.iter().cloned().collect();
        if let Some(duplicate) = lifetime_params
            .iter()
            .find(|name| type_parameter_names.contains(*name))
        {
            return Err(Diagnostic::at(
                Span::from(&start).into(),
                format!("`{duplicate}` cannot be both a type and lifetime parameter"),
            ));
        }
        let mut all_type_parameters = self.type_parameters.clone();
        all_type_parameters.extend(type_parameter_names);
        let previous_type_parameters =
            std::mem::replace(&mut self.type_parameters, all_type_parameters);
        let declared_lifetimes: HashSet<String> = lifetime_params.iter().cloned().collect();
        let previous_lifetimes =
            std::mem::replace(&mut self.lifetime_parameters, declared_lifetimes.clone());
        self.consume(&TokenKind::LParen, "`(`")?;
        let mut params = Vec::new();
        if !self.at(&TokenKind::RParen) {
            loop {
                let param_name = self.take_ident("parameter name")?;
                let param_span = Span::from(self.previous());
                self.consume(&TokenKind::Colon, "`:`")?;
                let ty = self.parse_type()?;
                self.validate_lifetime_use(&ty, &declared_lifetimes, param_span)?;
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
        self.validate_lifetime_use(&return_type, &declared_lifetimes, Span::from(&start))?;
        let body = self.parse_block()?;
        self.type_parameters = previous_type_parameters;
        self.lifetime_parameters = previous_lifetimes;
        Ok(Function {
            name,
            generic_params,
            lifetime_params,
            params,
            return_type,
            body,
            span: Span::from(&start),
            is_public,
            module: self.module.clone(),
        })
    }

    pub(super) fn parse_extern_function(
        &mut self,
        is_public: bool,
    ) -> Result<ExternFunction, Diagnostic> {
        let start = self.consume(&TokenKind::Extern, "`extern`")?;
        match self.advance().kind.clone() {
            TokenKind::StringLit(abi) if abi == "C" => {}
            _ => return Err(unexpected(self.previous(), "the ABI string `\"C\"`")),
        }
        self.consume(&TokenKind::Fn, "`fn` after the ABI string")?;
        let name = self.take_ident("external function name")?;
        let link_name = name.clone();
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
                if !self.take(&TokenKind::Comma) || self.at(&TokenKind::RParen) {
                    break;
                }
            }
        }
        self.consume(&TokenKind::RParen, "`)`")?;
        self.consume(&TokenKind::Arrow, "`->`")?;
        let return_type = self.parse_type()?;
        self.consume(
            &TokenKind::Semicolon,
            "`;` after external function declaration",
        )?;
        Ok(ExternFunction {
            name,
            link_name,
            params,
            return_type,
            span: Span::from(&start),
            is_public,
            module: self.module.clone(),
        })
    }

    pub(super) fn parse_type(&mut self) -> Result<Type, Diagnostic> {
        if self.take(&TokenKind::Amp) {
            let lifetime = if let TokenKind::Lifetime(name) = &self.peek().kind {
                if !self.lifetime_parameters.contains(name) {
                    return Err(Diagnostic::at(
                        Span::from(self.peek()).into(),
                        format!("undeclared lifetime `'{name}`"),
                    ));
                }
                let name = name.clone();
                self.advance();
                Some(name)
            } else {
                None
            };
            if matches!(&self.peek().kind, TokenKind::Ident(name) if name == "mut") {
                self.advance();
                Ok(Type::MutableReference(
                    Box::new(self.parse_type()?),
                    lifetime,
                ))
            } else {
                Ok(Type::Reference(Box::new(self.parse_type()?), lifetime))
            }
        } else if self.take(&TokenKind::Star) {
            let mutable = if matches!(&self.peek().kind, TokenKind::Ident(name) if name == "mut") {
                self.advance();
                true
            } else if self.take(&TokenKind::Const) {
                false
            } else {
                return Err(unexpected(
                    self.peek(),
                    "`mut` or `const` after `*` in raw pointer type",
                ));
            };
            Ok(Type::RawPointer(Box::new(self.parse_type()?), mutable))
        } else if self.take(&TokenKind::Fn) {
            self.consume(&TokenKind::LParen, "`(` after `fn` in function type")?;
            let mut params = Vec::new();
            if !self.at(&TokenKind::RParen) {
                loop {
                    params.push(self.parse_type()?);
                    if !self.take(&TokenKind::Comma) || self.at(&TokenKind::RParen) {
                        break;
                    }
                }
            }
            self.consume(&TokenKind::RParen, "`)` after function type parameters")?;
            self.consume(&TokenKind::Arrow, "`->` in function type")?;
            Ok(Type::Function(params, Box::new(self.parse_type()?)))
        } else if self.take(&TokenKind::LParen) {
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
                "type `i64`, `f32`, `f64`, `bool`, `str`, `unit`, a named type, reference type, tuple type, function type, or `[element; length]`",
            ))
        }
    }

    fn validate_lifetime_use(
        &self,
        ty: &Type,
        declared: &HashSet<String>,
        span: Span,
    ) -> Result<(), Diagnostic> {
        fn visit(ty: &Type, declared: &HashSet<String>, span: Span) -> Result<(), Diagnostic> {
            match ty {
                Type::Reference(element, lifetime) | Type::MutableReference(element, lifetime) => {
                    if let Some(lifetime) = lifetime {
                        if !declared.contains(lifetime) {
                            return Err(Diagnostic::at(
                                span.into(),
                                format!("undeclared lifetime `'{lifetime}`"),
                            ));
                        }
                    } else if !declared.is_empty() {
                        return Err(Diagnostic::at(
                            span.into(),
                            "references in lifetime-generic function signatures must name a lifetime",
                        ));
                    }
                    visit(element, declared, span)
                }
                Type::Array(element, _) | Type::Vector(element) | Type::RawPointer(element, _) => {
                    visit(element, declared, span)
                }
                Type::Tuple(elements) | Type::Applied(_, elements) => {
                    for element in elements {
                        visit(element, declared, span)?;
                    }
                    Ok(())
                }
                Type::Function(params, result) => {
                    for param in params {
                        visit(param, declared, span)?;
                    }
                    visit(result, declared, span)
                }
                _ => Ok(()),
            }
        }
        visit(ty, declared, span)
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

    fn parse_function_generics(&mut self) -> Result<(Vec<String>, Vec<String>), Diagnostic> {
        if !self.take(&TokenKind::Less) {
            return Ok((Vec::new(), Vec::new()));
        }
        let mut type_parameters = Vec::new();
        let mut lifetime_parameters = Vec::new();
        loop {
            match self.peek().kind.clone() {
                TokenKind::Lifetime(parameter) => {
                    let token = self.advance().clone();
                    if lifetime_parameters.contains(&parameter) {
                        return Err(Diagnostic::at(
                            Span::from(&token).into(),
                            format!("lifetime parameter `'{parameter}` is defined more than once"),
                        ));
                    }
                    lifetime_parameters.push(parameter);
                }
                TokenKind::Ident(parameter) => {
                    let token = self.advance().clone();
                    if type_parameters.contains(&parameter) {
                        return Err(Diagnostic::at(
                            Span::from(&token).into(),
                            format!("generic parameter `{parameter}` is defined more than once"),
                        ));
                    }
                    if lifetime_parameters.contains(&parameter) {
                        return Err(Diagnostic::at(
                            Span::from(&token).into(),
                            format!("`{parameter}` cannot be both a type and lifetime parameter"),
                        ));
                    }
                    type_parameters.push(parameter);
                }
                TokenKind::Less => {
                    return Err(unexpected(
                        self.peek(),
                        "generic type parameter or named lifetime parameter such as `'a`",
                    ));
                }
                _ => {
                    return Err(unexpected(
                        self.peek(),
                        "generic type parameter or named lifetime parameter such as `'a`",
                    ));
                }
            }
            if !self.take(&TokenKind::Comma) {
                break;
            }
            if self.at(&TokenKind::Greater) {
                return Err(unexpected(
                    self.peek(),
                    "generic or lifetime parameter after `,`",
                ));
            }
        }
        self.consume_type_greater()?;
        Ok((type_parameters, lifetime_parameters))
    }
}
