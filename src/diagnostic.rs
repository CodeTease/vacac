use std::fmt;

const TAB_WIDTH: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceSpan {
    pub line: usize,
    pub column: usize,
    pub end_column: usize,
    pub file: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub message: String,
    pub file: Option<String>,
    pub span: Option<SourceSpan>,
    pub source_id: Option<usize>,
    pub source_line: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticSet {
    pub diagnostics: Vec<Diagnostic>,
}

impl DiagnosticSet {
    pub fn new(diagnostics: Vec<Diagnostic>) -> Self {
        Self { diagnostics }
    }
}

impl From<Diagnostic> for DiagnosticSet {
    fn from(diagnostic: Diagnostic) -> Self {
        Self::new(vec![diagnostic])
    }
}

impl From<String> for DiagnosticSet {
    fn from(message: String) -> Self {
        Diagnostic::new(message).into()
    }
}

impl From<&str> for DiagnosticSet {
    fn from(message: &str) -> Self {
        Diagnostic::new(message).into()
    }
}

impl Diagnostic {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            file: None,
            span: None,
            source_id: None,
            source_line: None,
        }
    }

    pub fn at(span: SourceSpan, message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            file: None,
            span: Some(span),
            source_id: span.file,
            source_line: None,
        }
    }

    pub fn with_file(mut self, file: impl Into<String>) -> Self {
        self.file = Some(file.into());
        self
    }

    pub fn with_source_id(
        mut self,
        source_id: usize,
        file: impl Into<String>,
        source: &str,
    ) -> Self {
        self.source_id = Some(source_id);
        self.file = Some(file.into());
        if let Some(span) = &mut self.span
            && span.file.is_none()
        {
            span.file = Some(source_id);
        }
        self.source_line = self
            .span
            .filter(|span| span.file == Some(source_id))
            .and_then(|span| source.lines().nth(span.line.saturating_sub(1)))
            .map(str::to_owned);
        self
    }

    pub fn render_human(&self) -> String {
        let mut rendered = match (&self.file, self.span) {
            (Some(file), Some(span)) => {
                format!("{file}:{}:{}: {}", span.line, span.column, self.message)
            }
            (Some(file), None) => format!("{file}: {}", self.message),
            (None, Some(span)) => {
                format!("line {}:{}: {}", span.line, span.column, self.message)
            }
            (None, None) => self.message.clone(),
        };
        if let (Some(source_line), Some(span)) = (&self.source_line, self.span) {
            let line = span.line;
            let gutter_width = span.line.to_string().len();
            let (source_line, columns) = expand_tabs(source_line);
            let start = columns
                .get(span.column.saturating_sub(1))
                .copied()
                .unwrap_or_else(|| source_line.chars().count());
            let end = columns
                .get(span.end_column.saturating_sub(1))
                .copied()
                .unwrap_or_else(|| source_line.chars().count());
            let marker = format!(
                "{}{}",
                " ".repeat(start),
                "^".repeat(end.saturating_sub(start).max(1))
            );
            rendered.push_str(&format!(
                "\n {line:>gutter_width$} | {source_line}\n {:>gutter_width$} | {marker}",
                ""
            ));
        }
        rendered
    }

    pub fn render_json(&self) -> String {
        format!(
            "{{\"severity\":\"error\",\"file\":{},\"line\":{},\"column\":{},\"end_column\":{},\"message\":{}}}",
            self.file
                .as_deref()
                .map_or_else(|| "null".to_owned(), json_string),
            self.span
                .map_or("null".to_owned(), |span| span.line.to_string()),
            self.span
                .map_or("null".to_owned(), |span| span.column.to_string()),
            self.span
                .map_or("null".to_owned(), |span| span.end_column.to_string()),
            json_string(&self.message)
        )
    }
}

impl From<crate::ast::Span> for SourceSpan {
    fn from(span: crate::ast::Span) -> Self {
        Self {
            line: span.line,
            column: span.column,
            end_column: span.end_column,
            file: span.file,
        }
    }
}

impl From<&crate::ast::Span> for SourceSpan {
    fn from(span: &crate::ast::Span) -> Self {
        (*span).into()
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.render_human())
    }
}

impl From<String> for Diagnostic {
    fn from(message: String) -> Self {
        Self::new(message)
    }
}

impl From<&str> for Diagnostic {
    fn from(message: &str) -> Self {
        Self::new(message)
    }
}

fn json_string(input: &str) -> String {
    let mut output = String::with_capacity(input.len() + 2);
    output.push('"');
    for character in input.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            character if character.is_control() => {
                output.push_str(&format!("\\u{:04x}", character as u32));
            }
            character => output.push(character),
        }
    }
    output.push('"');
    output
}

/// Expand tabs at four-column stops and return the display boundary for every
/// Unicode scalar-value column used by the lexer/parser spans.
fn expand_tabs(input: &str) -> (String, Vec<usize>) {
    let mut output = String::with_capacity(input.len());
    let mut columns = Vec::with_capacity(input.chars().count() + 1);
    let mut display_column = 0;
    columns.push(display_column);
    for character in input.chars() {
        if character == '\t' {
            let width = TAB_WIDTH - display_column % TAB_WIDTH;
            output.push_str(&" ".repeat(width));
            display_column += width;
        } else {
            output.push(character);
            display_column += 1;
        }
        columns.push(display_column);
    }
    (output, columns)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_contract_has_stable_fields_and_nulls_for_missing_location() {
        let message = format!(
            "quote: {} slash: {} newline:{} tab:{} control:{}",
            '"', '\\', '\n', '\t', '\u{0001}'
        );
        let diagnostic = Diagnostic::new(message);
        assert_eq!(
            diagnostic.render_json(),
            r#"{"severity":"error","file":null,"line":null,"column":null,"end_column":null,"message":"quote: \" slash: \\ newline:\n tab:\t control:\u0001"}"#
        );
    }

    #[test]
    fn human_underlines_expand_tabs_and_count_unicode_scalars() {
        let diagnostic = Diagnostic::at(
            SourceSpan {
                line: 3,
                column: 2,
                end_column: 3,
                file: Some(0),
            },
            "unexpected token",
        )
        .with_source_id(0, "test.vca", "\n\n\tλx\t;");
        let rendered = diagnostic.render_human();
        assert!(rendered.contains(" 3 |     λx  ;"), "{rendered}");
        assert!(rendered.contains("   |     ^"), "{rendered}");
    }

    #[test]
    fn human_underlines_expand_tabs_inside_the_span() {
        let diagnostic = Diagnostic::at(
            SourceSpan {
                line: 1,
                column: 2,
                end_column: 3,
                file: Some(0),
            },
            "bad token",
        )
        .with_source_id(0, "test.vca", "a\tb");
        let rendered = diagnostic.render_human();
        assert!(rendered.contains(" 1 | a   b"), "{rendered}");
        assert!(rendered.contains("   |  ^^^"), "{rendered}");
    }
}
