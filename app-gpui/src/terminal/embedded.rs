//! Terminal widgets embedded in other views: a terminal with the terminal's font, theme and
//! links, whose input and size go to the owner instead of a terminal session (a Run console
//! emulating a terminal feeds its process).

use super::fit::{FitScheduler, GridSize, SettledResize};
use super::keys::{KeyEvent, TerminalKeyAction, terminal_key_action};

/// What an embedded terminal does with a key event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmbeddedKey {
    /// The terminal's own key handling takes it.
    Default,
    /// Type this into the terminal: it reaches the owner as input.
    Input(String),
    /// Copy the selection to the clipboard.
    Copy,
    /// Nothing more: the release of a key whose press was handled here.
    Swallow,
}

/// The keys an embedded terminal handles itself: Shift+Enter as Esc+Return, and copying a
/// selection before the copy shortcut reaches the process as an interrupt. `pressed`: the key
/// went down (rather than up). `mac`: the app runs on macOS.
pub fn embedded_key(e: &KeyEvent, pressed: bool, has_selection: bool, mac: bool) -> EmbeddedKey {
    if e.ime {
        return EmbeddedKey::Default;
    }
    let handled = match terminal_key_action(e, has_selection, mac) {
        TerminalKeyAction::Send(data) if e.key == "Enter" => EmbeddedKey::Input(data),
        TerminalKeyAction::Copy => EmbeddedKey::Copy,
        _ => return EmbeddedKey::Default,
    };
    if pressed { handled } else { EmbeddedKey::Swallow }
}

/// What an embedded terminal's owner does after a fit step.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FitOutcome {
    /// The terminal was fitted: give it this grid, unless it has it already.
    pub grid: Option<GridSize>,
    /// The size settled on one the owner does not know yet: tell it.
    pub report: Option<GridSize>,
}

/// When an embedded terminal refits to its host, and when its owner hears of a new size: a fit
/// on the next frame for a lone resize, one fit once a run of resizes (a drag) settles, and one
/// report of the size the terminal ends on. Its owner learns the initial size from whoever
/// creates the terminal; after that only changes are reported.
///
/// `measure` in the methods below gives the grid that fills the host now
/// ([`super::fit::grid_size`]), or `None` while the host is hidden or its cell is unmeasured.
#[derive(Debug, Clone, PartialEq)]
pub struct EmbeddedFit {
    fitter: FitScheduler,
    process_resize: SettledResize,
    /// The size the owner was last told.
    reported: GridSize,
}

impl EmbeddedFit {
    /// `initial`: the size the terminal is created with.
    pub fn new(initial: GridSize) -> Self {
        Self { fitter: FitScheduler::new(), process_resize: SettledResize::new(), reported: initial }
    }

    /// The host changed size at time `now`.
    pub fn request(&mut self, now: f64) {
        self.process_resize.cancel();
        self.fitter.schedule(now);
    }

    fn fit(&mut self, now: f64, measure: impl FnOnce() -> Option<GridSize>) -> Option<GridSize> {
        let size = measure()?;
        self.process_resize.resized(size, now);
        Some(size)
    }

    /// A frame is being drawn. Returns the grid to give the terminal, if it was fitted.
    pub fn frame(&mut self, now: f64, measure: impl FnOnce() -> Option<GridSize>) -> Option<GridSize> {
        if self.fitter.frame() { self.fit(now, measure) } else { None }
    }

    /// Time has passed.
    pub fn tick(&mut self, now: f64, measure: impl FnOnce() -> Option<GridSize>) -> FitOutcome {
        let grid = if self.fitter.tick(now) { self.fit(now, measure) } else { None };
        let report = self.process_resize.tick(now).filter(|size| *size != self.reported);
        if let Some(size) = report {
            self.reported = size;
        }
        FitOutcome { grid, report }
    }

    /// Fits now, dropping any scheduled fit. Returns the grid to give the terminal.
    pub fn fit_now(&mut self, now: f64, measure: impl FnOnce() -> Option<GridSize>) -> Option<GridSize> {
        self.fitter.flush();
        self.fit(now, measure)
    }

    /// Drops everything pending: the terminal is going away.
    pub fn cancel(&mut self) {
        self.fitter.cancel();
        self.process_resize.cancel();
    }

    /// The pending fit wants the next frame.
    pub fn wants_frame(&self) -> bool {
        self.fitter.wants_frame()
    }

    /// When [`Self::tick`] is due.
    pub fn deadline(&self) -> Option<f64> {
        match (self.fitter.deadline(), self.process_resize.deadline()) {
            (Some(fit), Some(report)) => Some(fit.min(report)),
            (fit, report) => fit.or(report),
        }
    }

    pub fn is_pending(&self) -> bool {
        self.wants_frame() || self.deadline().is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::dispatcher::{Mods, stroke};
    use crate::terminal::fit::{FIT_SETTLE_MS, PROCESS_RESIZE_DELAY_MS};
    use crate::terminal::keys::claims_copy_stroke;

    const CTRL: Mods = Mods { ctrl: true, alt: false, shift: false, meta: false };
    const SHIFT: Mods = Mods { ctrl: false, alt: false, shift: true, meta: false };

    // src/features/terminal/embedded.test.ts › copies embedded Run terminal selections before xterm can send Ctrl+C
    #[test]
    fn copies_embedded_terminal_selections_before_the_process_gets_ctrl_c() {
        let copy_strokes = [CTRL, Mods { shift: true, ..CTRL }, Mods { meta: true, ..Mods::NONE }];
        for mods in copy_strokes {
            let e = KeyEvent::new("c", "KeyC", mods);
            assert_eq!(embedded_key(&e, true, true, false), EmbeddedKey::Copy);
            // The IDE's own Copy does not run for these strokes.
            assert!(claims_copy_stroke(&stroke("C", mods)));
        }
        let ctrl_c = KeyEvent::new("c", "KeyC", CTRL);
        assert_eq!(embedded_key(&ctrl_c, false, true, false), EmbeddedKey::Swallow);
        // Without a selection the process gets its interrupt.
        assert_eq!(embedded_key(&ctrl_c, true, false, false), EmbeddedKey::Default);
    }

    // src/features/terminal/embedded.test.ts › sends Shift+Enter in embedded Run terminals as Esc+Return
    #[test]
    fn sends_shift_enter_in_embedded_terminals_as_esc_return() {
        let shift_enter = KeyEvent::new("Enter", "Enter", SHIFT);
        assert_eq!(embedded_key(&shift_enter, true, false, false), EmbeddedKey::Input("\x1b\r".to_owned()));
        assert_eq!(embedded_key(&shift_enter, false, false, false), EmbeddedKey::Swallow);
        for mods in [Mods::NONE, Mods { alt: true, ..Mods::NONE }, Mods { shift: true, ..CTRL }] {
            let e = KeyEvent::new("Enter", "Enter", mods);
            assert_eq!(embedded_key(&e, true, false, false), EmbeddedKey::Default);
        }
        let composing = KeyEvent { ime: true, ..shift_enter };
        assert_eq!(embedded_key(&composing, true, false, false), EmbeddedKey::Default);
    }

    /// An embedded terminal in a host whose box fits `layout`, on a fake clock that draws a frame
    /// every 16 ms, collecting the sizes its owner is told.
    struct Embedded {
        fit: EmbeddedFit,
        layout: GridSize,
        now: u32,
        reports: Vec<GridSize>,
    }

    impl Embedded {
        /// A terminal that has settled on its initial 80 × 24.
        fn new() -> Self {
            let layout = GridSize { cols: 80, rows: 24 };
            let mut embedded = Self { fit: EmbeddedFit::new(layout), layout, now: 0, reports: Vec::new() };
            embedded.resize();
            embedded.advance(SETTLE);
            assert!(embedded.reports.is_empty());
            embedded
        }

        /// The host's box changed.
        fn resize(&mut self) {
            self.fit.request(f64::from(self.now));
        }

        fn advance(&mut self, ms: u32) {
            for _ in 0..ms {
                self.now += 1;
                let (now, layout) = (f64::from(self.now), self.layout);
                if self.now.is_multiple_of(16) {
                    self.fit.frame(now, || Some(layout));
                }
                self.reports.extend(self.fit.tick(now, || Some(layout)).report);
            }
        }

        fn advance_to_next_frame(&mut self) {
            self.advance(16 - self.now % 16);
        }
    }

    const SETTLE: u32 = (FIT_SETTLE_MS + PROCESS_RESIZE_DELAY_MS) as u32;

    // src/features/terminal/embeddedFit.test.ts › reports only the final size after a drag, including unchanged
    // final cells: %s
    #[test]
    fn reports_only_the_final_size_after_a_drag_including_unchanged_final_cells() {
        for same_cells in [false, true] {
            let mut embedded = Embedded::new();
            embedded.layout.cols = 79;
            embedded.resize();
            embedded.advance_to_next_frame();
            for _ in 0..40 {
                if !same_cells {
                    embedded.layout.cols -= 1;
                }
                embedded.resize();
                embedded.advance(16);
            }
            assert!(embedded.reports.is_empty());
            embedded.advance(SETTLE);
            assert_eq!(embedded.reports, [embedded.layout]);
            embedded.resize();
            embedded.advance(SETTLE);
            assert_eq!(embedded.reports.len(), 1);
        }
    }

    // src/features/terminal/embeddedFit.test.ts › disposes with no pending fit or process resize timer: %s
    #[test]
    fn disposes_with_no_pending_fit_or_process_resize_timer() {
        for deferred in [false, true] {
            let mut embedded = Embedded::new();
            embedded.layout.cols = 79;
            embedded.resize();
            embedded.advance_to_next_frame();
            if deferred {
                embedded.resize();
            }
            assert!(embedded.fit.is_pending());
            embedded.fit.cancel();
            assert!(!embedded.fit.is_pending());
            embedded.advance(SETTLE);
            assert!(embedded.reports.is_empty());
        }
    }

    // Not from a spec: an explicit fit runs at once, and a hidden host is not fitted.
    #[test]
    fn fits_now_and_skips_a_hidden_host() {
        let mut fit = EmbeddedFit::new(GridSize { cols: 80, rows: 24 });
        fit.request(0.0);
        assert!(fit.wants_frame());
        assert_eq!(fit.fit_now(1.0, || None), None);
        assert!(!fit.is_pending());
        let narrow = GridSize { cols: 60, rows: 24 };
        assert_eq!(fit.fit_now(2.0, || Some(narrow)), Some(narrow));
        assert_eq!(fit.deadline(), Some(2.0 + PROCESS_RESIZE_DELAY_MS));
        assert_eq!(fit.tick(2.0 + PROCESS_RESIZE_DELAY_MS, || None), FitOutcome { grid: None, report: Some(narrow) });
    }
}
