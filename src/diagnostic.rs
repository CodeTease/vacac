use std::fmt;

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

    pub fn with_source(self, file: impl Into<String>, source: &str) -> Self {
        self.with_source_id(0, file, source)
    }

    pub fn with_source_id(
        mut self,
        source_id: usize,
        file: impl Into<String>,
        source: &str,
    ) -> Self {
        self.file = Some(file.into());
        self.source_line = self
            .span
            .filter(|span| span.file == Some(source_id))
            .and_then(|span| source.lines().nth(span.line.saturating_sub(1)))
            .map(str::to_owned);
        self
    }

    pub fn from_legacy(message: impl Into<String>, file: &str, source: &str) -> Self {
        let message = message.into();
        let Some(location) = message.strip_prefix("line ") else {
            return Self::new(message).with_file(file);
        };
        let Some((line, rest)) = location.split_once(':') else {
            return Self::new(message).with_file(file);
        };
        let Some((column, tail)) = rest.split_once(':') else {
            return Self::new(message).with_file(file);
        };
        let (Ok(line), Ok(column)) = (line.parse::<usize>(), column.parse::<usize>()) else {
            return Self::new(message).with_file(file);
        };
        let (end_column, text) = if let Some((end_column, text)) = tail.split_once(": ") {
            match end_column.parse::<usize>() {
                Ok(end_column) => (end_column, text),
                Err(_) => return Self::new(message).with_file(file),
            }
        } else if let Some(text) = tail.strip_prefix(' ') {
            (column.saturating_add(1), text)
        } else {
            return Self::new(message).with_file(file);
        };
        Self::at(
            SourceSpan {
                line,
                column,
                end_column,
                file: None,
            },
            text,
        )
        .with_source(file, source)
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
            let marker = format!(
                "{}{}",
                " ".repeat(span.column.saturating_sub(1)),
                "^".repeat(span.end_column.saturating_sub(span.column).max(1))
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

impl fmt::Display for Diagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(span) = self.span {
            if let Some(file) = &self.file {
                write!(
                    formatter,
                    "{file}:{}:{}: {}",
                    span.line, span.column, self.message
                )
            } else {
                write!(
                    formatter,
                    "line {}:{}: {}",
                    span.line, span.column, self.message
                )
            }
        } else if let Some(file) = &self.file {
            write!(formatter, "{file}: {}", self.message)
        } else {
            formatter.write_str(&self.message)
        }
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
