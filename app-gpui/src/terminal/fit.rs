//! When a terminal refits its grid to its box, and when the process behind it is told.
//!
//! Both are state machines over a clock that is passed in (milliseconds), so their owner arms one
//! timer for [`FitScheduler::deadline`] / [`SettledResize::deadline`] and they are testable
//! without one.

/// How long requests must stop before a terminal that keeps resizing (a tool window drag)
/// refits. A fit reflows the whole scrollback, so during the drag the pane only clips.
pub const FIT_SETTLE_MS: f64 = 120.0;
/// How long the size must stay put before the process behind the terminal is told.
pub const PROCESS_RESIZE_DELAY_MS: f64 = 150.0;
/// How long an idle terminal waits for a frame before it fits anyway: a window in the background
/// may get no frames, and must not stay unfitted until it is shown.
const FRAME_FALLBACK_MS: f64 = 100.0;
/// The smallest grid a terminal is fitted to.
const MIN_COLS: f32 = 2.0;
const MIN_ROWS: f32 = 1.0;

/// A terminal's size in cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridSize {
    pub cols: u16,
    pub rows: u16,
}

/// The grid that fills `width` × `height` pixels with cells of `cell_width` × `cell_height`:
/// whole cells only, at least 2 columns and 1 row. `None` while the cell is not measured yet (a
/// zero or non-finite size), when there is nothing to fit to.
pub fn grid_size(width: f32, height: f32, cell_width: f32, cell_height: f32) -> Option<GridSize> {
    let measured = |cell: f32| cell.is_finite() && cell > 0.0;
    if !measured(cell_width) || !measured(cell_height) || !width.is_finite() || !height.is_finite() {
        return None;
    }
    let cells = |available: f32, cell: f32, min: f32| (available.max(0.0) / cell).floor().max(min) as u16;
    Some(GridSize { cols: cells(width, cell_width, MIN_COLS), rows: cells(height, cell_height, MIN_ROWS) })
}

/// Coalesces fit requests. An idle terminal fits on the next frame; while requests keep coming
/// (its host is being resized) it fits once, [`FIT_SETTLE_MS`] after the last one.
#[derive(Debug, Clone, PartialEq)]
pub struct FitScheduler {
    /// The pending fit also runs on the next frame.
    on_frame: bool,
    /// When the pending fit runs whatever happens.
    deadline: Option<f64>,
    last_request: f64,
}

impl Default for FitScheduler {
    fn default() -> Self {
        Self { on_frame: false, deadline: None, last_request: f64::NEG_INFINITY }
    }
}

impl FitScheduler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Asks for a fit at time `now`.
    pub fn schedule(&mut self, now: f64) {
        let resizing = now - self.last_request < FIT_SETTLE_MS;
        self.last_request = now;
        if resizing {
            self.cancel();
            self.deadline = Some(now + FIT_SETTLE_MS);
            return;
        }
        if self.deadline.is_some() {
            return;
        }
        self.on_frame = true;
        self.deadline = Some(now + FRAME_FALLBACK_MS);
    }

    /// A frame is being drawn. Returns whether to fit now.
    pub fn frame(&mut self) -> bool {
        let due = self.on_frame;
        if due {
            self.cancel();
        }
        due
    }

    /// Time has passed. Returns whether to fit now.
    pub fn tick(&mut self, now: f64) -> bool {
        let due = self.deadline.is_some_and(|deadline| now >= deadline);
        if due {
            self.cancel();
        }
        due
    }

    /// The caller fits right now: whatever was scheduled is dropped.
    pub fn flush(&mut self) {
        self.cancel();
    }

    pub fn cancel(&mut self) {
        self.on_frame = false;
        self.deadline = None;
    }

    /// The pending fit wants the next frame.
    pub fn wants_frame(&self) -> bool {
        self.on_frame
    }

    /// When [`Self::tick`] is due.
    pub fn deadline(&self) -> Option<f64> {
        self.deadline
    }

    pub fn is_pending(&self) -> bool {
        self.on_frame || self.deadline.is_some()
    }
}

/// Reports a size once it has stayed the same for [`PROCESS_RESIZE_DELAY_MS`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SettledResize {
    pending: Option<(f64, GridSize)>,
}

impl SettledResize {
    pub fn new() -> Self {
        Self::default()
    }

    /// The terminal was given `size` at time `now`.
    pub fn resized(&mut self, size: GridSize, now: f64) {
        self.pending = Some((now + PROCESS_RESIZE_DELAY_MS, size));
    }

    /// Time has passed. Returns the size to report once it has settled.
    pub fn tick(&mut self, now: f64) -> Option<GridSize> {
        let (_, size) = self.pending.filter(|(deadline, _)| now >= *deadline)?;
        self.pending = None;
        Some(size)
    }

    pub fn cancel(&mut self) {
        self.pending = None;
    }

    /// When [`Self::tick`] is due.
    pub fn deadline(&self) -> Option<f64> {
        self.pending.map(|(deadline, _)| deadline)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scheduler on a fake clock that draws a frame every 16 ms, counting the fits.
    struct Fitting {
        scheduler: FitScheduler,
        now: u32,
        frames: bool,
        fits: u32,
    }

    impl Fitting {
        fn new(frames: bool) -> Self {
            Self { scheduler: FitScheduler::new(), now: 0, frames, fits: 0 }
        }

        fn schedule(&mut self) {
            self.scheduler.schedule(f64::from(self.now));
        }

        fn advance(&mut self, ms: u32) {
            for _ in 0..ms {
                self.now += 1;
                if self.frames && self.now.is_multiple_of(16) && self.scheduler.frame() {
                    self.fits += 1;
                }
                if self.scheduler.tick(f64::from(self.now)) {
                    self.fits += 1;
                }
            }
        }

        fn advance_to_next_frame(&mut self) {
            self.advance(16 - self.now % 16);
        }
    }

    // src/features/terminal/terminal.test.ts › fit scheduling › fits an idle terminal on the next frame, and a
    // resizing one once it settles
    #[test]
    fn fits_an_idle_terminal_on_the_next_frame_and_a_resizing_one_once_it_settles() {
        let mut f = Fitting::new(true);
        f.schedule();
        f.advance_to_next_frame();
        assert_eq!(f.fits, 1);
        for _ in 0..30 {
            f.schedule();
            f.advance(16);
        }
        assert_eq!(f.fits, 1);
        f.advance(FIT_SETTLE_MS as u32);
        assert_eq!(f.fits, 2);
        f.advance(FIT_SETTLE_MS as u32);
        f.schedule();
        f.scheduler.cancel();
        f.advance(FIT_SETTLE_MS as u32 * 2);
        assert_eq!(f.fits, 2);
    }

    // src/features/terminal/terminal.test.ts › fit scheduling › fits a lone second request after it settles without
    // needing another request
    #[test]
    fn fits_a_lone_second_request_after_it_settles_without_needing_another_request() {
        let mut f = Fitting::new(true);
        f.schedule();
        f.advance_to_next_frame();
        f.advance(40);
        f.schedule();
        f.advance(FIT_SETTLE_MS as u32 - 1);
        assert_eq!(f.fits, 1);
        f.advance(1);
        assert_eq!(f.fits, 2);
        assert!(!f.scheduler.is_pending());
    }

    // src/features/terminal/terminal.test.ts › fit scheduling › fits without animation frames in a background window
    #[test]
    fn fits_without_animation_frames_in_a_background_window() {
        let mut f = Fitting::new(false);
        f.schedule();
        assert!(f.scheduler.wants_frame());
        f.advance(100);
        assert_eq!(f.fits, 1);
        assert!(!f.scheduler.is_pending());
    }

    // src/features/terminal/terminal.test.ts › fit scheduling › cancels all pending work after repeated requests: %s
    #[test]
    fn cancels_all_pending_work_after_repeated_requests() {
        for repeated in [false, true] {
            let mut f = Fitting::new(true);
            f.schedule();
            if repeated {
                f.schedule();
            }
            f.scheduler.cancel();
            assert!(!f.scheduler.is_pending());
            f.advance(FIT_SETTLE_MS as u32 * 2);
            assert_eq!(f.fits, 0);
        }
    }

    // src/features/terminal/terminal.test.ts › fit scheduling › flushes a deferred fit immediately and leaves no
    // delayed duplicate
    #[test]
    fn flushes_a_deferred_fit_immediately_and_leaves_no_delayed_duplicate() {
        let mut f = Fitting::new(true);
        f.schedule();
        f.schedule();
        assert_eq!(f.scheduler.deadline(), Some(FIT_SETTLE_MS));
        // The fit of the caller that flushes.
        f.scheduler.flush();
        f.fits += 1;
        assert!(!f.scheduler.is_pending());
        f.advance(FIT_SETTLE_MS as u32 * 2);
        assert_eq!(f.fits, 1);
    }

    // src/features/terminal/terminal.test.ts › fit scheduling › reports only the size the terminal settles on
    #[test]
    fn reports_only_the_size_the_terminal_settles_on() {
        let mut r = SettledResize::new();
        let mut reports = Vec::new();
        r.resized(GridSize { cols: 80, rows: 20 }, 0.0);
        let half = PROCESS_RESIZE_DELAY_MS / 2.0;
        reports.extend(r.tick(half));
        r.resized(GridSize { cols: 80, rows: 24 }, half);
        reports.extend(r.tick(PROCESS_RESIZE_DELAY_MS));
        assert_eq!(r.deadline(), Some(half + PROCESS_RESIZE_DELAY_MS));
        reports.extend(r.tick(half + PROCESS_RESIZE_DELAY_MS));
        reports.extend(r.tick(half + PROCESS_RESIZE_DELAY_MS * 2.0));
        assert_eq!(reports, [GridSize { cols: 80, rows: 24 }]);
    }

    // Not from a spec: whole cells only, never fewer than 2 columns and 1 row, nothing before the cell is measured.
    #[test]
    fn maps_a_pixel_box_to_whole_cells() {
        assert_eq!(grid_size(600.0, 400.0, 7.5, 16.0), Some(GridSize { cols: 80, rows: 25 }));
        assert_eq!(grid_size(599.9, 399.9, 7.5, 16.0), Some(GridSize { cols: 79, rows: 24 }));
        assert_eq!(grid_size(3.0, 2.0, 7.5, 16.0), Some(GridSize { cols: 2, rows: 1 }));
        assert_eq!(grid_size(-10.0, 0.0, 7.5, 16.0), Some(GridSize { cols: 2, rows: 1 }));
        assert_eq!(grid_size(600.0, 400.0, 0.0, 16.0), None);
        assert_eq!(grid_size(600.0, 400.0, 7.5, f32::NAN), None);
        assert_eq!(grid_size(f32::INFINITY, 400.0, 7.5, 16.0), None);
    }
}
