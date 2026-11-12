//! Syntax errors of the built-in parser as diagnostics, for files no language server checks: none
//! is configured for the language, or it is not installed, stopped or crashed. A running server
//! reports syntax errors itself, so none are added then.

use crate::platform::i18n::{I18n, Param};
use ide_core::event::ServerStatus;
use ide_core::{Diagnostic, Severity};
use ide_syntax::{SyntaxError, SyntaxErrorKind};

/// The inspection producer of these diagnostics.
pub const SYNTAX_ERRORS_PRODUCER: &str = "syntax";
/// The pause after an edit before the file is checked again.
pub const INSPECT_DELAY_MS: u64 = 500;
/// Languages whose dialects the grammar does not follow (the database support checks SQL).
const SKIPPED: [&str; 2] = ["sql", "plaintext"];

/// Whether the parser's errors are reported for a document of `language`.
pub fn checks_language(language: &str, virtual_doc: bool) -> bool {
    !virtual_doc && !SKIPPED.contains(&language)
}

/// Whether a server with this status checks its languages.
pub fn server_checks(status: ServerStatus) -> bool {
    matches!(status, ServerStatus::Starting | ServerStatus::Ready | ServerStatus::Busy)
}

/// The languages running servers check, from each server's status and languages.
pub fn checked_languages<'a>(servers: &[(ServerStatus, &'a [String])]) -> Vec<&'a str> {
    servers
        .iter()
        .filter(|(status, _)| server_checks(*status))
        .flat_map(|(_, languages)| languages.iter().map(String::as_str))
        .collect()
}

pub fn syntax_error_diagnostic(error: &SyntaxError, i18n: &mut I18n) -> Diagnostic {
    let message = match (&error.kind, &error.expected) {
        (SyntaxErrorKind::Missing, Some(expected)) => {
            i18n.tx_built(&["'", "' expected"], &[Param::from(expected.as_str())])
        }
        _ => i18n.tx("Unexpected token"),
    };
    Diagnostic {
        range: error.range,
        severity: Severity::Error,
        message,
        source: Some(i18n.tx("Syntax")),
        code: None,
        unnecessary: false,
        deprecated: false,
        server: None,
    }
}

/// The diagnostics to publish for a document of `language`: none while a running server checks
/// the language, else the parser's errors.
pub fn syntax_diagnostics(
    language: &str,
    checked: &[&str],
    errors: &[SyntaxError],
    i18n: &mut I18n,
) -> Vec<Diagnostic> {
    if checked.contains(&language) {
        return Vec::new();
    }
    errors.iter().map(|error| syntax_error_diagnostic(error, i18n)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_core::{Pos, Range};

    fn errors() -> Vec<SyntaxError> {
        vec![
            SyntaxError {
                kind: SyntaxErrorKind::Unexpected,
                range: Range::new(Pos::new(1, 4), Pos::new(1, 10)),
                expected: None,
                message: "unexpected `iqTask`".into(),
            },
            SyntaxError {
                kind: SyntaxErrorKind::Missing,
                range: Range::new(Pos::new(2, 3), Pos::new(2, 3)),
                expected: Some(";".into()),
                message: "missing `;`".into(),
            },
        ]
    }

    #[test]
    fn reports_the_parsers_errors_when_no_server_checks_the_language() {
        let php = ["php".to_owned()];
        let missing = [(ServerStatus::Missing, &php[..])];
        let shown = syntax_diagnostics("php", &checked_languages(&missing), &errors(), &mut I18n::default());
        let messages: Vec<(&str, Severity, Option<&str>)> =
            shown.iter().map(|d| (d.message.as_str(), d.severity, d.source.as_deref())).collect();
        assert_eq!(
            messages,
            [("Unexpected token", Severity::Error, Some("Syntax")), ("';' expected", Severity::Error, Some("Syntax"))]
        );
    }

    #[test]
    fn leaves_them_to_a_running_server_and_checks_again_when_the_server_stops() {
        let php = ["php".to_owned()];
        let mut i18n = I18n::default();
        let ready = [(ServerStatus::Ready, &php[..])];
        assert!(syntax_diagnostics("php", &checked_languages(&ready), &errors(), &mut i18n).is_empty());
        let crashed = [(ServerStatus::Crashed, &php[..])];
        assert_eq!(syntax_diagnostics("php", &checked_languages(&crashed), &errors(), &mut i18n).len(), 2);
        assert!(checks_language("php", false) && !checks_language("sql", false));
    }
}
