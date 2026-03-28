use std::fmt;

use ariadne::{Color, Label, Report, ReportKind, Source};

use crate::ast::Span;

/// A compiler diagnostic with span and message.
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub span: Span,
    pub message: String,
    pub kind: DiagnosticKind,
}

#[derive(Debug, Clone, Copy)]
pub enum DiagnosticKind {
    Error,
    Warning,
}

impl Diagnostic {
    pub fn error(span: Span, message: impl Into<String>) -> Self {
        Self {
            span,
            message: message.into(),
            kind: DiagnosticKind::Error,
        }
    }

    pub fn warning(span: Span, message: impl Into<String>) -> Self {
        Self {
            span,
            message: message.into(),
            kind: DiagnosticKind::Warning,
        }
    }
}

/// One or more diagnostics were emitted during compilation.
#[derive(Debug, Clone)]
pub struct Errors {
    pub diagnostics: Vec<Diagnostic>,
}

impl fmt::Display for Errors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} error(s)", self.diagnostics.len())
    }
}

impl std::error::Error for Errors {}

/// Render diagnostics to stderr using ariadne.
pub fn report_errors(src: &str, filename: &str, diagnostics: &[Diagnostic]) {
    for diag in diagnostics {
        let kind = match diag.kind {
            DiagnosticKind::Error => ReportKind::Error,
            DiagnosticKind::Warning => ReportKind::Warning,
        };
        let color = match diag.kind {
            DiagnosticKind::Error => Color::Red,
            DiagnosticKind::Warning => Color::Yellow,
        };

        Report::build(kind, (filename, diag.span.clone()))
            .with_message(&diag.message)
            .with_label(
                Label::new((filename, diag.span.clone()))
                    .with_message(&diag.message)
                    .with_color(color),
            )
            .finish()
            .eprint((filename, Source::from(src)))
            .unwrap();
    }
}
