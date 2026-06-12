//! Structured diagnostics for validation and error reporting.
//!
//! Validation functions across the crate return [`RunweaverDiagnostic`]
//! values instead of panicking or formatting strings ad hoc. Each diagnostic
//! carries a stable `SCREAMING_SNAKE_CASE` code, a [`RunweaverDiagnosticSeverity`],
//! a message, and optionally a path and a JSON cause. Callers decide what to
//! do with them — typically aborting when [`has_error_diagnostics`] is true
//! and rendering with [`format_diagnostics`].
//!
//! [`RunweaverDiagnosticsError`] wraps a batch of diagnostics as a returnable
//! error for boundaries (such as the CLI) that must fail with context.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Whether a diagnostic blocks ([`Error`](Self::Error)) or merely informs
/// ([`Warning`](Self::Warning)).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunweaverDiagnosticSeverity {
    Error,
    Warning,
}

/// One validation finding: a stable code, severity, message, and optional
/// path/cause context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunweaverDiagnostic {
    pub code: String,
    pub severity: RunweaverDiagnosticSeverity,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cause: Option<Value>,
}

impl RunweaverDiagnostic {
    pub fn new(
        code: impl Into<String>,
        severity: RunweaverDiagnosticSeverity,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            severity,
            message: message.into(),
            path: None,
            cause: None,
        }
    }

    pub fn with_path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    pub fn with_cause(mut self, cause: Value) -> Self {
        self.cause = Some(cause);
        self
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct RunweaverDiagnosticsError {
    pub message: String,
    pub diagnostics: Vec<RunweaverDiagnostic>,
}

impl RunweaverDiagnosticsError {
    pub fn new(message: impl Into<String>, diagnostics: Vec<RunweaverDiagnostic>) -> Self {
        Self {
            message: message.into(),
            diagnostics,
        }
    }
}

pub fn diagnostic(
    code: impl Into<String>,
    severity: RunweaverDiagnosticSeverity,
    message: impl Into<String>,
) -> RunweaverDiagnostic {
    RunweaverDiagnostic::new(code, severity, message)
}

pub fn error_diagnostic(
    code: impl Into<String>,
    message: impl Into<String>,
) -> RunweaverDiagnostic {
    diagnostic(code, RunweaverDiagnosticSeverity::Error, message)
}

pub fn warning_diagnostic(
    code: impl Into<String>,
    message: impl Into<String>,
) -> RunweaverDiagnostic {
    diagnostic(code, RunweaverDiagnosticSeverity::Warning, message)
}

pub fn has_error_diagnostics(diagnostics: &[RunweaverDiagnostic]) -> bool {
    diagnostics
        .iter()
        .any(|item| item.severity == RunweaverDiagnosticSeverity::Error)
}

pub fn format_diagnostic(diagnostic: &RunweaverDiagnostic) -> String {
    let severity = match diagnostic.severity {
        RunweaverDiagnosticSeverity::Error => "ERROR",
        RunweaverDiagnosticSeverity::Warning => "WARNING",
    };
    let location = diagnostic
        .path
        .as_ref()
        .map(|path| format!(" {path}"))
        .unwrap_or_default();
    format!(
        "{} {}{}: {}",
        severity, diagnostic.code, location, diagnostic.message
    )
}

pub fn format_diagnostics(diagnostics: &[RunweaverDiagnostic]) -> String {
    diagnostics
        .iter()
        .map(format_diagnostic)
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_diagnostic_includes_optional_path_when_present() {
        let diagnostic = error_diagnostic(
            "RUNWEAVER_TOOLCHAIN_PACKAGE_MISSING",
            ".runweaver/package.json is missing.",
        )
        .with_path(".runweaver/package.json");

        assert_eq!(
            format_diagnostic(&diagnostic),
            "ERROR RUNWEAVER_TOOLCHAIN_PACKAGE_MISSING .runweaver/package.json: .runweaver/package.json is missing."
        );
    }

    #[test]
    fn has_error_diagnostics_returns_false_for_warnings_only() {
        let diagnostics = vec![warning_diagnostic("RUNWEAVER_WARNING", "non-blocking")];

        assert!(!has_error_diagnostics(&diagnostics));
    }
}
