use crate::diagnostic::Diagnostic;
use crate::lexer::{Token, TokenKind, unexpected};
use std::collections::{HashMap, HashSet};

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
    /// Names bound by `use` declarations, which may name an imported struct.
    import_names: HashSet<String>,
    enum_names: HashSet<String>,
    module: String,
    suppress_struct_literal: bool,
    type_close_debt: usize,
    type_parameters: HashSet<String>,
    lifetime_parameters: HashSet<String>,
    self_type_allowed: bool,
    diagnostics: Vec<Diagnostic>,
}

impl Parser {
    #[cfg(test)]
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
        let mut enum_names: HashSet<String> = tokens
            .windows(2)
            .filter_map(|pair| match (&pair[0].kind, &pair[1].kind) {
                (TokenKind::Enum, TokenKind::Ident(name)) => Some(name.clone()),
                _ => None,
            })
            .collect();
        // `Result` is a compiler-provided prelude enum and is available in every source file.
        enum_names.insert("Result".to_owned());
        let import_names = collect_import_names(&tokens);
        Self {
            tokens,
            current: 0,
            struct_names,
            import_names,
            enum_names,
            module,
            suppress_struct_literal: false,
            type_close_debt: 0,
            type_parameters: HashSet::new(),
            lifetime_parameters: HashSet::new(),
            self_type_allowed: false,
            diagnostics: Vec::new(),
        }
    }

    pub fn parse_program(mut self) -> (Program, Vec<Diagnostic>) {
        let mut enums = Vec::new();
        let mut structs = Vec::new();
        let mut globals = Vec::new();
        let mut functions = Vec::new();
        let mut extern_functions = Vec::new();
        let mut traits = Vec::new();
        let mut impls = Vec::new();
        let mut modules = Vec::new();
        let mut uses = Vec::new();
        let mut diagnostics = Vec::new();
        while !self.at(&TokenKind::Eof) {
            let parser_diagnostic_count = self.diagnostics.len();
            let declaration_start = self.current;
            let is_public = self.take(&TokenKind::Pub);
            let declaration_result = self.parse_top_level_declaration(
                is_public,
                &mut enums,
                &mut structs,
                &mut globals,
                &mut functions,
                &mut extern_functions,
                &mut traits,
                &mut impls,
                &mut modules,
                &mut uses,
            );
            match declaration_result {
                Ok(()) => {}
                Err(error) => {
                    diagnostics.push(error);
                    self.synchronize_top_level(declaration_start);
                }
            }
            if self.diagnostics.len() > parser_diagnostic_count {
                if let Some(function) = functions.last()
                    && function.span.line == self.tokens[declaration_start].line
                    && function.span.column == self.tokens[declaration_start].column
                {
                    functions.pop();
                }
                self.synchronize_top_level(declaration_start);
            }
            if diagnostics
                .last()
                .is_some_and(|diagnostic| diagnostic.message.contains("found `end of file`"))
                && self.diagnostics.len() > parser_diagnostic_count
            {
                diagnostics.pop();
                if let Some((index, _)) = self
                    .tokens
                    .iter()
                    .enumerate()
                    .skip(declaration_start + 1)
                    .find(|(_, token)| matches!(token.kind, TokenKind::Fn))
                {
                    self.current = index;
                } else {
                    break;
                }
            }
        }
        if functions.is_empty()
            && extern_functions.is_empty()
            && enums.is_empty()
            && structs.is_empty()
            && traits.is_empty()
            && impls.is_empty()
            && globals.is_empty()
            && modules.is_empty()
            && uses.is_empty()
            && diagnostics.is_empty()
            && self.diagnostics.is_empty()
        {
            diagnostics.push("program must contain at least one declaration".into());
        }
        diagnostics.extend(std::mem::take(&mut self.diagnostics));
        (
            Program {
                enums,
                structs,
                globals,
                functions,
                extern_functions,
                traits,
                impls,
                modules,
                uses,
                source_files: Vec::new(),
                source_texts: Vec::new(),
                functions_are_qualified: false,
                lambda_captures: HashMap::new(),
            },
            diagnostics,
        )
    }

    fn parse_top_level_declaration(
        &mut self,
        is_public: bool,
        enums: &mut Vec<EnumDecl>,
        structs: &mut Vec<StructDecl>,
        globals: &mut Vec<GlobalDecl>,
        functions: &mut Vec<Function>,
        extern_functions: &mut Vec<ExternFunction>,
        traits: &mut Vec<TraitDecl>,
        impls: &mut Vec<ImplDecl>,
        modules: &mut Vec<ModuleDecl>,
        uses: &mut Vec<UseDecl>,
    ) -> Result<(), Diagnostic> {
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
        } else if self.at(&TokenKind::Use) {
            uses.push(self.parse_use(is_public)?);
        } else if self.at(&TokenKind::Enum) {
            enums.push(self.parse_enum(is_public)?);
        } else if self.at(&TokenKind::Struct) {
            structs.push(self.parse_struct(is_public)?);
        } else if self.at(&TokenKind::Const) || self.at(&TokenKind::Let) {
            let mutable = self.at(&TokenKind::Let);
            globals.push(self.parse_global(is_public, mutable)?);
        } else if self.at(&TokenKind::Extern) {
            extern_functions.push(self.parse_extern_function(is_public)?);
        } else if self.at(&TokenKind::Fn) {
            functions.push(self.parse_function(is_public)?);
        } else if self.at(&TokenKind::Trait) {
            traits.push(self.parse_trait(is_public)?);
        } else if self.at(&TokenKind::Impl) {
            impls.push(self.parse_impl(is_public)?);
        } else {
            return Err(unexpected(
                self.peek(),
                "`fn`, `extern`, `struct`, `enum`, `trait`, `impl`, `const`, `let`, `mod`, or `use` declaration",
            ));
        }
        Ok(())
    }

    fn synchronize_top_level(&mut self, declaration_start: usize) {
        let mut braces = 0usize;
        let mut parentheses = 0usize;
        let mut brackets = 0usize;

        for index in declaration_start..self.tokens.len() {
            let token = &self.tokens[index];
            let delimiters_are_balanced = braces == 0 && parentheses == 0 && brackets == 0;
            let is_candidate = index >= self.current
                && index > declaration_start
                && is_top_level_start(&token.kind)
                && (delimiters_are_balanced
                    || (token.column == 1
                        && !delimiters_close_after(
                            &self.tokens[index..],
                            braces,
                            parentheses,
                            brackets,
                        )));
            if is_candidate {
                self.current = index;
                self.type_parameters.clear();
                self.lifetime_parameters.clear();
                self.self_type_allowed = false;
                self.type_close_debt = 0;
                return;
            }

            match &token.kind {
                TokenKind::LBrace => braces += 1,
                TokenKind::RBrace => braces = braces.saturating_sub(1),
                TokenKind::LParen => parentheses += 1,
                TokenKind::RParen => parentheses = parentheses.saturating_sub(1),
                TokenKind::LBracket => brackets += 1,
                TokenKind::RBracket => brackets = brackets.saturating_sub(1),
                _ => {}
            }
        }

        self.current = self.tokens.len().saturating_sub(1);
        self.type_parameters.clear();
        self.lifetime_parameters.clear();
        self.self_type_allowed = false;
        self.type_close_debt = 0;
    }

    pub(super) fn report(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic);
    }

    pub(super) fn synchronize_statement(&mut self) {
        let mut braces = 0usize;
        let mut parentheses = 0usize;
        let mut brackets = 0usize;

        while !self.at(&TokenKind::Eof) {
            if braces == 0 && parentheses == 0 && brackets == 0 {
                if self.at(&TokenKind::RBrace) {
                    return;
                }
                if is_statement_boundary(&self.peek().kind) {
                    return;
                }
                if self.at(&TokenKind::Semicolon) {
                    self.advance();
                    return;
                }
            }

            match &self.peek().kind {
                TokenKind::LBrace => braces += 1,
                TokenKind::RBrace if braces > 0 => braces -= 1,
                TokenKind::RBrace if parentheses == 0 && brackets == 0 => return,
                TokenKind::LParen => parentheses += 1,
                TokenKind::RParen => parentheses = parentheses.saturating_sub(1),
                TokenKind::LBracket => brackets += 1,
                TokenKind::RBracket => brackets = brackets.saturating_sub(1),
                _ => {}
            }
            self.advance();
        }
    }

    pub(super) fn synchronize_match_arm(&mut self) {
        let start = self.current;
        let mut braces = 0usize;
        let mut parentheses = 0usize;
        let mut brackets = 0usize;

        while !self.at(&TokenKind::Eof) {
            if braces == 0 && parentheses == 0 && brackets == 0 {
                if self.at(&TokenKind::RBrace) {
                    return;
                }
                if self.at(&TokenKind::Comma) {
                    self.advance();
                    return;
                }
                if self.current > start && can_start_pattern(&self.peek().kind) {
                    return;
                }
            }

            match &self.peek().kind {
                TokenKind::LBrace => braces += 1,
                TokenKind::RBrace if braces > 0 => braces -= 1,
                TokenKind::RBrace if parentheses == 0 && brackets == 0 => return,
                TokenKind::LParen => parentheses += 1,
                TokenKind::RParen => parentheses = parentheses.saturating_sub(1),
                TokenKind::LBracket => brackets += 1,
                TokenKind::RBracket => brackets = brackets.saturating_sub(1),
                _ => {}
            }
            self.advance();
        }
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

/// Names introduced by top-level `use` declarations, scanned like `struct_names`
/// so an imported struct can be recognized as a struct literal.
fn collect_import_names(tokens: &[Token]) -> HashSet<String> {
    let mut names = HashSet::new();
    let mut index = 0;
    while index < tokens.len() {
        if matches!(tokens[index].kind, TokenKind::Use) {
            index += 1;
            let mut last = None;
            let mut alias = None;
            let mut awaiting_alias = false;
            while index < tokens.len() {
                match &tokens[index].kind {
                    TokenKind::Ident(name) => {
                        if awaiting_alias {
                            alias = Some(name.clone());
                            awaiting_alias = false;
                        }
                        last = Some(name.clone());
                    }
                    TokenKind::As => awaiting_alias = true,
                    TokenKind::DoubleColon => {}
                    _ => break,
                }
                index += 1;
            }
            if let Some(name) = alias.or(last) {
                names.insert(name);
            }
        }
        index += 1;
    }
    names
}

fn is_statement_boundary(kind: &TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Let
            | TokenKind::Const
            | TokenKind::If
            | TokenKind::While
            | TokenKind::For
            | TokenKind::Break
            | TokenKind::Continue
            | TokenKind::Return
    )
}

fn can_start_pattern(kind: &TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Ident(_)
            | TokenKind::True
            | TokenKind::False
            | TokenKind::Integer(_)
            | TokenKind::Float(_)
            | TokenKind::Float32(_)
            | TokenKind::Minus
            | TokenKind::LParen
            | TokenKind::LBracket
    )
}

fn is_top_level_start(kind: &TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Pub
            | TokenKind::Fn
            | TokenKind::Extern
            | TokenKind::Struct
            | TokenKind::Enum
            | TokenKind::Trait
            | TokenKind::Impl
            | TokenKind::Const
            | TokenKind::Let
            | TokenKind::Mod
            | TokenKind::Use
    )
}

fn delimiters_close_after(
    tokens: &[Token],
    mut braces: usize,
    mut parentheses: usize,
    mut brackets: usize,
) -> bool {
    for token in tokens {
        match &token.kind {
            TokenKind::LBrace => braces += 1,
            TokenKind::RBrace => braces = braces.saturating_sub(1),
            TokenKind::LParen => parentheses += 1,
            TokenKind::RParen => parentheses = parentheses.saturating_sub(1),
            TokenKind::LBracket => brackets += 1,
            TokenKind::RBracket => brackets = brackets.saturating_sub(1),
            _ => {}
        }
        if braces == 0 && parentheses == 0 && brackets == 0 {
            return true;
        }
        if matches!(token.kind, TokenKind::Eof) {
            break;
        }
    }
    false
}

#[cfg(test)]
mod recovery_tests {
    use super::Parser;
    use crate::lexer;

    #[test]
    fn recovery_does_not_treat_a_line_start_keyword_inside_a_body_as_top_level() {
        let source = "fn broken() -> i64 {\n    let value = ;\nfn decoy() -> i64 { 7 }\n}\nfn main() -> i64 { 0 }\n";
        let (program, diagnostics) = Parser::new(lexer::lex(source).unwrap()).parse_program();

        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(
            program
                .functions
                .iter()
                .map(|function| function.name.as_str())
                .collect::<Vec<_>>(),
            ["main"]
        );
    }

    #[test]
    fn recovery_recognizes_a_declaration_after_an_unclosed_body() {
        let source = "fn broken() -> i64 { let value = ;\nfn main() -> i64 { 0 }\n";
        let (program, diagnostics) = Parser::new(lexer::lex(source).unwrap()).parse_program();

        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(
            program
                .functions
                .iter()
                .map(|function| function.name.as_str())
                .collect::<Vec<_>>(),
            ["main"]
        );
    }

    #[test]
    fn recovery_retains_later_declarations_with_blank_lines_and_indentation() {
        let source = "fn broken() -> i64 { let value = ; 0 }\n\n    pub fn recovered() -> i64 { 1 }\n\nfn main() -> i64 { 0 }\n";
        let (program, diagnostics) = Parser::new(lexer::lex(source).unwrap()).parse_program();

        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(
            program
                .functions
                .iter()
                .map(|function| function.name.as_str())
                .collect::<Vec<_>>(),
            ["recovered", "main"]
        );
        assert!(program.functions[0].is_public);
    }

    #[test]
    fn recovery_from_a_malformed_declaration_at_eof_terminates_with_one_diagnostic() {
        let source = "fn malformed( -> i64 { 0";
        let (_program, diagnostics) = Parser::new(lexer::lex(source).unwrap()).parse_program();

        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    }

    #[test]
    fn failed_lifetime_generic_function_does_not_leak_lifetimes_during_recovery() {
        let source = "fn broken<'a>(value: &'a i64) -> { 0 }\nfn main(value: &'a i64) -> i64 { 0 }";
        let (program, diagnostics) = Parser::new(lexer::lex(source).unwrap()).parse_program();

        assert_eq!(program.functions.len(), 0, "{:?}", program.functions);
        assert_eq!(diagnostics.len(), 2, "{diagnostics:?}");
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("undeclared lifetime `'a`")),
            "{diagnostics:?}"
        );
    }
}

#[cfg(test)]
mod trait_body_tests {
    use super::Parser;
    use crate::lexer;

    #[test]
    fn trait_functions_parse_required_and_default_signatures() {
        let source = "trait Mixed {\n    fn required(value: Self) -> i64;\n    fn defaulted(value: Self) -> i64 { value + 1 }\n}\nfn main() -> i64 { 0 }\n";
        let (program, diagnostics) = Parser::new(lexer::lex(source).unwrap()).parse_program();

        assert_eq!(diagnostics.len(), 0, "{diagnostics:?}");
        let functions = &program.traits[0].functions;
        assert_eq!(functions.len(), 2);
        assert!(functions[0].body.is_none());
        let body = functions[1].body.as_ref().expect("default body");
        assert!(
            matches!(body.tail, Some(crate::ast::Expr::Binary { .. })),
            "{body:?}"
        );
    }

    #[test]
    fn trait_function_without_terminator_is_rejected() {
        let source = "trait Broken { fn value(value: Self) -> i64 }\nfn main() -> i64 { 0 }\n";
        let (_program, diagnostics) = Parser::new(lexer::lex(source).unwrap()).parse_program();

        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(
            diagnostics[0].message.contains("`;` or a `{` default body"),
            "{diagnostics:?}"
        );
    }
}

#[cfg(test)]
mod use_declaration_tests {
    use super::Parser;
    use crate::lexer;

    #[test]
    fn use_declarations_parse_paths_and_aliases() {
        let source = "use geometry::point::Point;\nuse math as numbers;\nfn main() -> i64 { 0 }\n";
        let (program, diagnostics) = Parser::new(lexer::lex(source).unwrap()).parse_program();

        assert_eq!(diagnostics.len(), 0, "{diagnostics:?}");
        assert_eq!(program.uses.len(), 2);
        assert_eq!(program.uses[0].path, ["geometry", "point", "Point"]);
        assert_eq!(program.uses[0].alias, None);
        assert_eq!(program.uses[1].path, ["math"]);
        assert_eq!(program.uses[1].alias.as_deref(), Some("numbers"));
    }

    #[test]
    fn pub_use_is_rejected() {
        let source = "pub use math::value;\nfn main() -> i64 { 0 }\n";
        let (_program, diagnostics) = Parser::new(lexer::lex(source).unwrap()).parse_program();

        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(
            diagnostics[0]
                .message
                .contains("`pub use` is not supported"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn use_without_semicolon_recovers_to_later_declarations() {
        let source = "use math::value\nfn main() -> i64 { 0 }\n";
        let (program, diagnostics) = Parser::new(lexer::lex(source).unwrap()).parse_program();

        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(
            diagnostics[0].message.contains("expected `;` after import"),
            "{diagnostics:?}"
        );
        assert_eq!(program.functions.len(), 1);
    }

    #[test]
    fn use_declaration_requires_a_path() {
        let source = "use ;\nfn main() -> i64 { 0 }\n";
        let (_program, diagnostics) = Parser::new(lexer::lex(source).unwrap()).parse_program();

        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert!(
            diagnostics[0].message.contains("expected imported name"),
            "{diagnostics:?}"
        );
    }
}

fn span_of_expr_parser(expr: &Expr) -> Span {
    match expr {
        Expr::Integer(_, span)
        | Expr::Float32(_, span)
        | Expr::Float(_, span)
        | Expr::Bool(_, span)
        | Expr::String(_, span)
        | Expr::Unit(span)
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
        | Expr::TraitCall { span, .. }
        | Expr::CallValue { span, .. }
        | Expr::Lambda { span, .. }
        | Expr::If { span, .. }
        | Expr::Match { span, .. }
        | Expr::Try { span, .. }
        | Expr::Variant { span, .. } => *span,
    }
}
