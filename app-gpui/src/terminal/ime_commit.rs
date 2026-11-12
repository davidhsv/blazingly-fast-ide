//! Collapses the repeated data emissions of one input-method commit.
//!
//! Some platforms deliver the text an input method commits more than once: the committed text
//! first, then a stale emission that repeats it together with what was committed before (typing
//! `；你好，` sends `，` and then `好，`). One commit must send one chunk, so the gate lets the first
//! emission through and drops the rest until the commit closes. A commit is delimited by the
//! input method's own keystroke outside a composition, or by a composition input when the
//! platform reports no keystroke. Commits that really compose are left alone: a composition
//! stands the gate down for that commit only, never for the session, because the very next
//! punctuation commit still needs deduplication.
//!
//! Every commit ends with a rebase: the input buffer the commit was read from is emptied, so the
//! next commit does not carry old text. The gate owns no clock: it names the timer it waits on
//! ([`ImeCommitGate::timer`]) and is told when that timer fires.

/// How long a commit may stay open when the end of its composition is never reported.
pub const IME_COMMIT_TIMEOUT_MS: u32 = 250;

/// The timer a gate waits on: after `delay_ms`, pass `id` to [`ImeCommitGate::timer_fired`]. A
/// gate waits on one timer at a time; a new one replaces the previous.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImeTimer {
    pub id: u64,
    pub delay_ms: u32,
}

/// Whether the caller empties its input buffer now.
pub type Rebase = bool;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImeCommitGate {
    open: bool,
    sent: bool,
    composing: bool,
    timer: Option<ImeTimer>,
    timers: u64,
}

impl ImeCommitGate {
    pub fn new() -> Self {
        Self::default()
    }

    /// The timer to arm, if the gate waits on one.
    pub fn timer(&self) -> Option<ImeTimer> {
        self.timer
    }

    fn schedule(&mut self, delay_ms: u32) {
        self.timers += 1;
        self.timer = Some(ImeTimer { id: self.timers, delay_ms });
    }

    /// A commit starts: the input method's own keystroke outside a composition.
    ///
    /// Always starts a fresh commit, even when the previous one is still inside its timeout: two
    /// commits can arrive back to back, and the second must not be swallowed.
    pub fn begin(&mut self) {
        if self.composing {
            return;
        }
        self.open = true;
        self.sent = false;
        self.schedule(IME_COMMIT_TIMEOUT_MS);
    }

    /// A composition input arrived. Opens a commit only when no commit is open: resetting `sent`
    /// here would let the emissions already queued for this commit go out again.
    pub fn input(&mut self) {
        if self.composing || self.open {
            return;
        }
        self.open = true;
        self.sent = false;
        // A close that `finish` already queued runs first, and ends this commit with it.
        if self.timer.is_none() {
            self.schedule(IME_COMMIT_TIMEOUT_MS);
        }
    }

    /// A composition begins. Its text is sent once by whoever handles compositions, so the gate
    /// stays out of the way, but only until the composition ends.
    pub fn composition_start(&mut self) {
        self.composing = true;
        self.timer = None;
        self.open = false;
        self.sent = false;
    }

    /// The composition, or a directly committed character, ended. The close is queued rather
    /// than run, so it lands behind the emission of the committed text and cannot cut it off.
    pub fn finish(&mut self) {
        self.composing = false;
        self.schedule(0);
    }

    /// A keystroke the input method did not take (Enter, an arrow, a letter typed outside it): the
    /// commit is over, and what that key sends must reach the shell.
    pub fn keystroke(&mut self) -> Rebase {
        self.open && !self.composing && self.close()
    }

    /// Whether this emission should reach the shell.
    pub fn accept(&mut self) -> bool {
        if !self.open {
            return true;
        }
        if self.sent {
            return false;
        }
        self.sent = true;
        true
    }

    /// The timer `id` fired. A timer that was replaced or dropped since does nothing.
    pub fn timer_fired(&mut self, id: u64) -> Rebase {
        self.timer.is_some_and(|timer| timer.id == id) && self.close()
    }

    /// Ends the commit.
    pub fn close(&mut self) -> Rebase {
        self.timer = None;
        // A new composition started before this close ran; its own finish will rebase.
        if self.composing {
            return false;
        }
        self.open = false;
        self.sent = false;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A gate with the timer it waits on fired by hand, counting the rebases.
    #[derive(Default)]
    struct Harness {
        gate: ImeCommitGate,
        rebases: u32,
        sent: Vec<&'static str>,
    }

    impl Harness {
        /// Fires the pending timer, the way a queued task runs after the current event.
        fn run(&mut self) {
            if let Some(timer) = self.gate.timer() {
                self.rebases += u32::from(self.gate.timer_fired(timer.id));
            }
        }

        fn keystroke(&mut self) {
            self.rebases += u32::from(self.gate.keystroke());
        }

        /// Collects the emissions that reach the shell.
        fn emit(&mut self, data: &'static str) {
            if self.gate.accept() {
                self.sent.push(data);
            }
        }
    }

    // src/features/terminal/imeCommit.test.ts › ImeCommitGate › takes the first emission of a commit and drops the
    // repeated one
    #[test]
    fn takes_the_first_emission_of_a_commit_and_drops_the_repeated_one() {
        let mut gate = ImeCommitGate::new();
        gate.begin();
        assert!(gate.accept());
        assert!(!gate.accept());
    }

    // src/features/terminal/imeCommit.test.ts › ImeCommitGate › keeps the real text of the pi case and drops the
    // stale suffix
    #[test]
    fn keeps_the_real_text_of_the_pi_case_and_drops_the_stale_suffix() {
        let mut h = Harness::default();
        h.gate.begin();
        h.emit("在吗");
        h.gate.finish();
        h.emit("好在吗");
        h.run();
        assert_eq!(h.sent, ["在吗"]);
    }

    // src/features/terminal/imeCommit.test.ts › ImeCommitGate › lets a key the IME did not take through right after
    // a commit
    #[test]
    fn lets_a_key_the_ime_did_not_take_through_right_after_a_commit() {
        // Punctuation committed directly, then Enter within the commit's timeout: Enter must run.
        let mut h = Harness::default();
        h.gate.begin();
        h.emit("，");
        h.keystroke();
        h.emit("\r");
        assert_eq!(h.sent, ["，", "\r"]);
        assert_eq!(h.rebases, 1);
        // Outside a commit, or during a composition, a keystroke changes nothing.
        h.keystroke();
        h.gate.composition_start();
        h.keystroke();
        assert_eq!(h.rebases, 1);
    }

    // src/features/terminal/imeCommit.test.ts › ImeCommitGate › starts a new commit on the next IME keystroke even
    // inside the timeout
    #[test]
    fn starts_a_new_commit_on_the_next_ime_keystroke_even_inside_the_timeout() {
        let mut gate = ImeCommitGate::new();
        gate.begin();
        assert!(gate.accept());
        gate.begin();
        assert!(gate.accept());
        assert!(!gate.accept());
    }

    // src/features/terminal/imeCommit.test.ts › ImeCommitGate › drops the stale suffix of a WebKitGTK punctuation
    // commit without compositionstart
    #[test]
    fn drops_the_stale_suffix_of_a_punctuation_commit_without_a_composition_start() {
        let mut h = Harness::default();
        h.gate.begin();
        h.emit("，");
        h.gate.finish();
        h.emit("好，");
        h.run();
        assert_eq!(h.sent, ["，"]);
        assert_eq!(h.rebases, 1);
    }

    // src/features/terminal/imeCommit.test.ts › ImeCommitGate › stands down for a real composition, then gates the
    // next commit again
    #[test]
    fn stands_down_for_a_real_composition_then_gates_the_next_commit_again() {
        let mut h = Harness::default();
        h.gate.begin();
        h.gate.composition_start();
        assert!(h.gate.accept());
        assert!(h.gate.accept());
        h.gate.finish();
        h.run();
        assert_eq!(h.rebases, 1);

        h.gate.begin();
        assert!(h.gate.accept());
        assert!(!h.gate.accept());
    }

    // src/features/terminal/imeCommit.test.ts › ImeCommitGate › accepts every emission while composing
    #[test]
    fn accepts_every_emission_while_composing() {
        let mut gate = ImeCommitGate::new();
        gate.composition_start();
        assert!(gate.accept());
        assert!(gate.accept());
        assert!(gate.accept());
    }

    // src/features/terminal/imeCommit.test.ts › ImeCommitGate › rebases after a composition that xterm sent by itself
    #[test]
    fn rebases_after_a_composition_sent_by_its_own_handler() {
        let mut h = Harness::default();
        h.gate.composition_start();
        h.gate.finish();
        assert_eq!(h.rebases, 0);
        h.run();
        assert_eq!(h.rebases, 1);
    }

    // src/features/terminal/imeCommit.test.ts › ImeCommitGate › rebases after a commit whose compositionend
    // arrives, not before
    #[test]
    fn rebases_after_a_commit_whose_composition_end_arrives_not_before() {
        let mut h = Harness::default();
        h.gate.begin();
        assert!(h.gate.accept());
        h.gate.finish();
        assert_eq!(h.rebases, 0);
        h.run();
        assert_eq!(h.rebases, 1);
    }

    // src/features/terminal/imeCommit.test.ts › ImeCommitGate › opens a commit from a composition input when no
    // keydown preceded it
    #[test]
    fn opens_a_commit_from_a_composition_input_when_no_keystroke_preceded_it() {
        let mut gate = ImeCommitGate::new();
        gate.input();
        assert!(gate.accept());
        assert!(!gate.accept());
        // A second input event belongs to the same commit and must not reopen it.
        gate.input();
        assert!(!gate.accept());
    }

    // src/features/terminal/imeCommit.test.ts › ImeCommitGate › closes a commit whose compositionend never arrives
    #[test]
    fn closes_a_commit_whose_composition_end_never_arrives() {
        let mut h = Harness::default();
        h.gate.begin();
        assert_eq!(h.gate.timer().map(|timer| timer.delay_ms), Some(IME_COMMIT_TIMEOUT_MS));
        h.run();
        assert_eq!(h.rebases, 1);
        assert!(h.gate.accept());
    }

    // src/features/terminal/imeCommit.test.ts › ImeCommitGate › does not rebase into a composition that started
    // before the close ran
    #[test]
    fn does_not_rebase_into_a_composition_that_started_before_the_close_ran() {
        let mut h = Harness::default();
        h.gate.begin();
        h.gate.finish();
        let queued = h.gate.timer();
        h.gate.composition_start();
        h.run();
        // The close that was queued before the composition started is void by now.
        assert!(!h.gate.timer_fired(queued.map_or(0, |timer| timer.id)));
        assert_eq!(h.rebases, 0);
    }
}
