//! Transient hints at the caret: "No language server available", "Cannot find declaration to go
//! to", the error of a failed action. A hint goes away with the next edit or caret move, with
//! Escape, or after a timeout.
//!
//! Timers are the caller's: showing a hint returns the ticket and the delay of its timeout, and
//! the caller reports back when a timer elapsed.

use crate::platform::i18n::I18n;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Tone {
    #[default]
    Info,
    Error,
    Warning,
}

/// A hint anchored at a document offset. `text` is English source text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hint {
    pub pos: u32,
    pub text: String,
    pub tone: Tone,
}

impl Hint {
    /// The text in the active locale.
    pub fn display(&self, i18n: &I18n) -> String {
        i18n.tx_template(&self.text)
    }
}

/// How long a hint stays without being dismissed.
pub const HINT_TIMEOUT_MS: u64 = 4000;
/// A request faster than this shows no progress hint.
pub const PENDING_DELAY_MS: u64 = 250;
/// How long a progress hint stays at most.
pub const PENDING_TIMEOUT_MS: u64 = 30_000;

/// A timer to start: report `ticket` back once `delay_ms` elapsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HintTimer {
    pub ticket: u64,
    pub delay_ms: u64,
}

/// A progress hint that waits to see whether its request is slow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingHint {
    text: String,
    pos: u32,
    /// The editor's state when the request started: an edit or a caret move since makes the
    /// progress stale.
    epoch: u64,
    /// The hint showing when the request started (its id), which the progress may replace.
    previous: Option<u64>,
    /// The id of the progress hint once shown.
    shown: Option<u64>,
    /// The request finished: a delay timer reported afterwards shows nothing.
    done: bool,
}

/// The hint of one editor.
#[derive(Debug, Clone, Default)]
pub struct HintState {
    current: Option<(u64, Hint)>,
    next_id: u64,
    /// Bumped by every edit and caret move.
    epoch: u64,
    /// The ticket of the timeout that may still hide the current hint.
    timeout: u64,
    closed: bool,
}

impl HintState {
    pub fn hint(&self) -> Option<&Hint> {
        self.current.as_ref().map(|(_, hint)| hint)
    }

    /// Shows a hint, replacing the current one; the caller starts the returned timeout.
    pub fn show(&mut self, text: &str, pos: u32, tone: Tone, timeout_ms: Option<u64>) -> HintTimer {
        self.next_id += 1;
        self.current = Some((self.next_id, Hint { pos, text: text.to_owned(), tone }));
        self.timeout = self.next_id;
        HintTimer { ticket: self.timeout, delay_ms: timeout_ms.unwrap_or(HINT_TIMEOUT_MS) }
    }

    /// A timeout elapsed: hides the hint unless a newer one restarted the timer.
    pub fn timed_out(&mut self, ticket: u64) {
        if ticket == self.timeout && !self.closed {
            self.current = None;
        }
    }

    /// The document or the selection changed: the hint goes away.
    pub fn changed(&mut self) {
        self.epoch += 1;
        self.current = None;
    }

    /// Escape; returns whether there was a hint to hide.
    pub fn hide(&mut self) -> bool {
        self.current.take().is_some()
    }

    /// The editor went away: nothing shows any more.
    pub fn close(&mut self) {
        self.closed = true;
        self.current = None;
    }

    /// A request that may be slow started; the caller starts a timer of [`PENDING_DELAY_MS`] and
    /// then calls [`HintState::pending_due`], and calls [`HintState::pending_done`] when the
    /// request finished.
    pub fn show_pending(&self, text: &str, pos: u32) -> PendingHint {
        PendingHint {
            text: text.to_owned(),
            pos,
            epoch: self.epoch,
            previous: self.current.as_ref().map(|(id, _)| *id),
            shown: None,
            done: false,
        }
    }

    /// The delay passed with the request still running: shows the progress unless the editor
    /// changed or another hint appeared meanwhile. Returns the timeout to start.
    pub fn pending_due(&mut self, pending: &mut PendingHint) -> Option<HintTimer> {
        let current = self.current.as_ref().map(|(id, _)| *id);
        if pending.done || self.closed || self.epoch != pending.epoch || current != pending.previous {
            return None;
        }
        let timer = self.show(&pending.text, pending.pos, Tone::Info, Some(PENDING_TIMEOUT_MS));
        pending.shown = Some(self.next_id);
        Some(timer)
    }

    /// The request finished: removes its own progress hint, and no other.
    pub fn pending_done(&mut self, pending: &mut PendingHint) {
        pending.done = true;
        if pending.shown.is_some() && self.current.as_ref().map(|(id, _)| *id) == pending.shown && !self.closed {
            self.current = None;
            // The progress hint's timeout has nothing left to hide.
            self.timeout = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(state: &HintState) -> Option<&str> {
        state.hint().map(|hint| hint.text.as_str())
    }

    #[test]
    fn does_not_flash_for_fast_requests() {
        let mut state = HintState::default();
        let mut pending = state.show_pending("Waiting…", 0);
        // The request finishes after 100 ms; the delay timer reported later shows nothing.
        state.pending_done(&mut pending);
        assert_eq!(state.pending_due(&mut pending), None);
        assert_eq!(text(&state), None);
    }

    #[test]
    fn shows_a_slow_request_and_removes_its_hint_on_completion() {
        let mut state = HintState::default();
        let mut pending = state.show_pending("Waiting…", 0);
        let timer = state.pending_due(&mut pending);
        assert_eq!(timer.map(|timer| timer.delay_ms), Some(PENDING_TIMEOUT_MS));
        assert_eq!(text(&state), Some("Waiting…"));
        state.pending_done(&mut pending);
        assert_eq!(text(&state), None);
    }

    #[test]
    fn does_not_dismiss_a_newer_hint_when_an_old_request_finishes() {
        let mut state = HintState::default();
        let mut pending = state.show_pending("Waiting…", 0);
        state.pending_due(&mut pending);
        state.show("New error", 0, Tone::Error, None);
        state.pending_done(&mut pending);
        assert_eq!(text(&state), Some("New error"));
    }

    #[test]
    fn does_not_overwrite_a_newer_hint_while_waiting_to_show_progress() {
        let mut state = HintState::default();
        let mut pending = state.show_pending("Waiting…", 0);
        state.show("New error", 0, Tone::Error, None);
        assert_eq!(state.pending_due(&mut pending), None);
        assert_eq!(text(&state), Some("New error"));
        state.pending_done(&mut pending);
        assert_eq!(text(&state), Some("New error"));
    }

    #[test]
    fn does_not_show_stale_progress_after_edit_move_or_destroy() {
        for change in ["edit", "move", "destroy"] {
            let mut state = HintState::default();
            let mut pending = state.show_pending("Waiting…", 0);
            match change {
                "destroy" => state.close(),
                _ => state.changed(),
            }
            assert_eq!(state.pending_due(&mut pending), None, "{change}");
            state.pending_done(&mut pending);
            assert_eq!(text(&state), None, "{change}");
        }
    }
}
