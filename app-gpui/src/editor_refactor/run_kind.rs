//! A refactoring run through the language server's code actions of one kind, with a syntax-based
//! fallback for when the server offers no enabled action or cannot answer (Introduce Variable /
//! Constant without a server).

use crate::editor_lsp::intentions::{KindOutcome, kind_outcome};
use crate::editor_lsp::lsp::{error_hint, is_cancelled, is_unavailable};
use ide_core::IdeError;
use ide_lsp::types::CodeAction;

/// Why the fallback is asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FallbackReason {
    /// No server is ready for the file.
    NotReady,
    /// The server failed.
    Error,
    /// The server offers no enabled action.
    Empty,
}

/// What the caller does with the server's answer.
#[derive(Debug, Clone)]
#[expect(clippy::large_enum_variant, reason = "one per server answer, consumed at once")]
pub enum KindStep {
    /// Nothing (the request was cancelled).
    Nothing,
    /// Apply the action, or offer the choice of several (titled with the refactoring's label).
    Run(KindOutcome),
    /// Show this hint at the caret (English source text).
    Hint(String),
    /// Run the syntax-based fallback; when it declines, show `otherwise` (if any) as the hint.
    Fallback { reason: FallbackReason, otherwise: Option<String> },
}

/// The step for the server's answer to the code actions of `kind` (a prefix; empty for any)
/// that `keep` accepts. `none` is the hint when none applies, `line_error` the message of an
/// error on the selected lines; `has_fallback` whether a fallback exists.
pub fn kind_step(
    answer: Result<Vec<CodeAction>, IdeError>,
    kind: &str,
    keep: &dyn Fn(&CodeAction) -> bool,
    none: &str,
    line_error: Option<&str>,
    has_fallback: bool,
) -> KindStep {
    match answer {
        Err(error) if is_cancelled(&error) => KindStep::Nothing,
        Err(error) => {
            let otherwise = error_hint(&error, true);
            if has_fallback {
                let reason = if is_unavailable(&error) { FallbackReason::NotReady } else { FallbackReason::Error };
                KindStep::Fallback { reason, otherwise }
            } else {
                otherwise.map_or(KindStep::Nothing, KindStep::Hint)
            }
        }
        Ok(actions) => match kind_outcome(&actions, kind, keep, none, line_error) {
            KindOutcome::None(hint) if has_fallback => {
                KindStep::Fallback { reason: FallbackReason::Empty, otherwise: Some(hint) }
            }
            KindOutcome::None(hint) => KindStep::Hint(hint),
            outcome => KindStep::Run(outcome),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_core::ServerId;
    use ide_core::error::ErrorKind;
    use regex::Regex;

    fn action(title: &str, disabled_reason: Option<&str>) -> CodeAction {
        CodeAction {
            title: title.to_owned(),
            kind: Some("refactor.extract.constant".to_owned()),
            is_preferred: false,
            disabled_reason: disabled_reason.map(str::to_owned),
            edit: None,
            command: None,
            data: None,
            server: ServerId(1),
        }
    }

    fn run(answer: Result<Vec<CodeAction>, IdeError>) -> KindStep {
        let title = Regex::new("(?i)constant").expect("the pattern is valid");
        let keep = |action: &CodeAction| title.is_match(&action.title);
        kind_step(answer, "refactor.extract", &keep, "Nothing", None, true)
    }

    #[test]
    fn uses_the_server_s_action_when_there_is_one() {
        let step = run(Ok(vec![action("Extract to constant", None)]));
        let KindStep::Run(KindOutcome::Apply(applied)) = step else { panic!("expected the action to apply") };
        assert_eq!(applied.title, "Extract to constant");
    }

    #[test]
    fn falls_back_when_the_server_is_not_ready_or_fails() {
        let not_ready = run(Err(IdeError::new(ErrorKind::Unavailable, "no server")));
        assert!(matches!(not_ready, KindStep::Fallback { reason: FallbackReason::NotReady, .. }));
        let failed = run(Err(IdeError::new(ErrorKind::Protocol, "boom")));
        assert!(matches!(failed, KindStep::Fallback { reason: FallbackReason::Error, .. }));
    }

    #[test]
    fn asks_the_fallback_on_an_empty_answer_and_keeps_the_server_s_reason_when_it_declines() {
        let step = run(Ok(vec![action("Extract to constant", Some("Cannot extract empty range"))]));
        let KindStep::Fallback { reason, otherwise } = step else { panic!("expected the fallback") };
        assert_eq!(reason, FallbackReason::Empty);
        assert_eq!(otherwise.as_deref(), Some("Cannot extract empty range"));
    }
}
