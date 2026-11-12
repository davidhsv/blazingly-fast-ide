//! What every code-intelligence feature shares: when a request may be sent, what an error shows,
//! the notes beside inexact results, and timers that restart.

use crate::platform::i18n::{I18n, Param};
use ide_core::error::ErrorKind;
use ide_core::{IdeError, RequestId};
use ide_lsp::types::ReferencesIncomplete;
use regex::Regex;
use std::sync::LazyLock;

/// What to do with a request that waits for the editors' pending edits to reach the backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dispatch {
    /// Edits typed just before are still on their way: the server must see them first.
    Wait,
    Send,
    /// The request was cancelled meanwhile: it is not sent (the backend ignores cancelling an id
    /// it has not seen).
    Cancelled,
}

/// Whether a request goes out: once every open editor is `synced`, and only if still `wanted`.
pub fn dispatch(synced: bool, wanted: bool) -> Dispatch {
    if !synced {
        Dispatch::Wait
    } else if wanted {
        Dispatch::Send
    } else {
        Dispatch::Cancelled
    }
}

/// Whether a request has somewhere to go: `connected` is a backend in this process or an installed
/// route to a remote one. Without either it fails as `unavailable`, unsent, so that callers have a
/// single error path.
pub fn route(connected: bool) -> Result<(), IdeError> {
    if connected { Ok(()) } else { Err(IdeError::new(ErrorKind::Unavailable, NO_SERVER)) }
}

/// The one request of a feature that is in flight. A newer request supersedes the older one, whose
/// answer is then dropped.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Inflight {
    current: Option<RequestId>,
}

impl Inflight {
    /// Starts request `id`; returns the request it supersedes, to cancel in the backend.
    pub fn begin(&mut self, id: RequestId) -> Option<RequestId> {
        self.current.replace(id)
    }

    /// Whether `id` is still the request whose answer is wanted.
    pub fn is_current(&self, id: RequestId) -> bool {
        self.current == Some(id)
    }

    /// The answer of `id` arrived (or failed); returns whether it is still wanted.
    pub fn finish(&mut self, id: RequestId) -> bool {
        let current = self.is_current(id);
        if current {
            self.current = None;
        }
        current
    }

    /// Gives up the request in flight; returns it, to cancel in the backend.
    pub fn cancel(&mut self) -> Option<RequestId> {
        self.current.take()
    }
}

/// A timer that restarts: only the run scheduled last fires.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Debounce {
    generation: u64,
    pending: bool,
}

impl Debounce {
    /// Schedules a run, dropping the pending one; the caller starts a timer carrying the ticket.
    pub fn schedule(&mut self) -> u64 {
        self.generation += 1;
        self.pending = true;
        self.generation
    }

    /// Drops the pending run.
    pub fn cancel(&mut self) {
        self.pending = false;
    }

    /// The timer of `ticket` elapsed; returns whether its run is due.
    pub fn fire(&mut self, ticket: u64) -> bool {
        let due = self.pending && ticket == self.generation;
        if due {
            self.pending = false;
        }
        due
    }
}

/// Why a project-wide search may have missed usages of `word`, for a note beside its results.
pub fn incomplete_note(i18n: &mut I18n, reason: ReferencesIncomplete, word: Option<&str>) -> String {
    match (reason, word) {
        (ReferencesIncomplete::Indexing, _) => i18n.tx("May be incomplete: the language server is still indexing"),
        (ReferencesIncomplete::TooManyFiles, Some(word)) => i18n
            .tx_built(&["May be incomplete: too many files mention '", "' to search them all"], &[Param::from(word)]),
        (ReferencesIncomplete::TooManyFiles, None) => {
            i18n.tx("May be incomplete: too many files mention the name to search them all")
        }
        (ReferencesIncomplete::ServerFailed, _) => {
            i18n.tx("May be incomplete: the language server could not check this name")
        }
        (ReferencesIncomplete::FilesSkipped, Some(word)) => i18n.tx_built(
            &["May be incomplete: some files that mention '", "' are too large or unreadable"],
            &[Param::from(word)],
        ),
        (ReferencesIncomplete::FilesSkipped, None) => {
            i18n.tx("May be incomplete: some files that mention the name are too large or unreadable")
        }
    }
}

/// The note beside text-match results: what they are and how to get exact ones. `light_mode` is
/// the IDE running without language servers.
pub fn approximate_note(i18n: &I18n, light_mode: bool) -> String {
    i18n.tx(if light_mode {
        "Text matches: switch to Deep mode for exact results"
    } else {
        "Text matches: no language server answered"
    })
}

/// The note of a references or call hierarchy result, if it may be inexact.
pub fn result_note(
    i18n: &mut I18n,
    incomplete: Option<ReferencesIncomplete>,
    approximate: bool,
    word: Option<&str>,
    light_mode: bool,
) -> Option<String> {
    let mut notes = Vec::new();
    if approximate {
        notes.push(approximate_note(i18n, light_mode));
    }
    if let Some(reason) = incomplete {
        notes.push(incomplete_note(i18n, reason, word));
    }
    notes.retain(|note| !note.is_empty());
    (!notes.is_empty()).then(|| notes.join(" · "))
}

pub fn is_unavailable(error: &IdeError) -> bool {
    error.kind == ErrorKind::Unavailable
}

pub fn is_cancelled(error: &IdeError) -> bool {
    error.kind == ErrorKind::Cancelled
}

pub const NO_SERVER: &str = "No language server available";

static LACKS_FEATURE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\S.* doesn't support \S.*$").expect("the pattern is valid"));

/// The message of an `unavailable` error when a running server lacks the feature ("jdtls doesn't
/// support rename"); `fallback` when no server is running for the file.
pub fn unavailable_text<'a>(error: &'a IdeError, fallback: &'a str) -> &'a str {
    if is_unavailable(error) && LACKS_FEATURE.is_match(&error.message) { &error.message } else { fallback }
}

/// True when `unavailable` means no server is running for the file, rather than a running server
/// lacking the feature.
pub fn is_no_server(error: &IdeError) -> bool {
    is_unavailable(error) && !LACKS_FEATURE.is_match(&error.message)
}

/// The hint an error shows at the caret, in English (the hint translates it when shown).
/// Background requests fail silently; explicit actions say "No language server available" for a
/// missing server and "jdtls doesn't support X" when the running server lacks the feature.
pub fn error_hint(error: &IdeError, explicit: bool) -> Option<String> {
    if is_cancelled(error) || !explicit {
        return None;
    }
    Some(if is_unavailable(error) { unavailable_text(error, NO_SERVER).to_owned() } else { error.message.clone() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unavailable(message: &str) -> IdeError {
        IdeError::new(ErrorKind::Unavailable, message)
    }

    #[test]
    fn waits_until_open_editors_edits_reached_the_backend() {
        assert_eq!(dispatch(false, true), Dispatch::Wait);
        assert_eq!(dispatch(true, true), Dispatch::Send);
        assert_eq!(dispatch(true, false), Dispatch::Cancelled);
    }

    #[test]
    fn outside_tauri_goes_through_an_installed_command_route_only() {
        let unrouted = route(false).expect_err("nowhere to send it");
        assert!(is_unavailable(&unrouted));
        assert!(is_no_server(&unrouted));
        assert_eq!(error_hint(&unrouted, true).as_deref(), Some(NO_SERVER));
        assert!(route(true).is_ok());
    }

    #[test]
    fn explicit_message_to_shown_hint() {
        for (message, shown) in [
            ("jdtls doesn't support type hierarchy", "jdtls doesn't support type hierarchy"),
            ("pyright, ruff doesn't support code lens", "pyright, ruff doesn't support code lens"),
            ("no running language server supports rename", "No language server available"),
            ("jdtls is not running", "No language server available"),
        ] {
            assert_eq!(error_hint(&unavailable(message), true).as_deref(), Some(shown), "{message}");
        }
    }

    #[test]
    fn stays_silent_for_background_requests() {
        assert_eq!(error_hint(&unavailable("jdtls doesn't support inlay hints"), false), None);
        assert_eq!(error_hint(&IdeError::new(ErrorKind::Cancelled, "cancelled"), true), None);
    }

    #[test]
    fn marks_text_matches_and_points_at_deep_mode_while_the_ide_is_in_blaze_mode() {
        let mut i18n = I18n::default();
        assert_eq!(
            result_note(&mut i18n, None, true, Some("x"), true).as_deref(),
            Some("Text matches: switch to Deep mode for exact results")
        );
        assert_eq!(
            result_note(&mut i18n, None, true, Some("x"), false).as_deref(),
            Some("Text matches: no language server answered")
        );
    }

    #[test]
    fn keeps_the_incompleteness_note_next_to_the_text_match_note() {
        let mut i18n = I18n::default();
        let too_many = incomplete_note(&mut i18n, ReferencesIncomplete::TooManyFiles, Some("x"));
        assert_eq!(too_many, "May be incomplete: too many files mention 'x' to search them all");
        assert_eq!(
            result_note(&mut i18n, Some(ReferencesIncomplete::TooManyFiles), true, Some("x"), true),
            Some(format!("Text matches: switch to Deep mode for exact results · {too_many}"))
        );
        let indexing = incomplete_note(&mut i18n, ReferencesIncomplete::Indexing, Some("x"));
        assert_eq!(
            result_note(&mut i18n, Some(ReferencesIncomplete::Indexing), false, Some("x"), true),
            Some(indexing)
        );
        assert_eq!(result_note(&mut i18n, None, false, Some("x"), true), None);
    }

    // Not from a spec: a newer request supersedes the one in flight, and only the last timer fires.
    #[test]
    fn a_newer_request_supersedes_the_one_in_flight() {
        let mut inflight = Inflight::default();
        assert_eq!(inflight.begin(RequestId(1)), None);
        assert_eq!(inflight.begin(RequestId(2)), Some(RequestId(1)));
        assert!(!inflight.finish(RequestId(1)));
        assert!(inflight.finish(RequestId(2)));
        let mut debounce = Debounce::default();
        let first = debounce.schedule();
        let second = debounce.schedule();
        assert!(!debounce.fire(first));
        assert!(debounce.fire(second));
    }
}
