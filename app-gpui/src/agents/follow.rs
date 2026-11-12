//! Scrolling a conversation: it follows the end as output streams in until the user scrolls away, offers
//! one click back to the latest message, keeps its offset per conversation (shared by the editor tab and
//! the docked view), and paints a long conversation from its end, prepending earlier entries in batches
//! without moving the text on screen.
//!
//! [`Follow`] is fed what the view observes: the scroll offset with the content and viewport heights,
//! user input (wheel, touch, keys, pointer), pointer drags, layouts and timers. It answers with
//! [`ScrollEffect`]s. A scroll is the user's when it comes with recent input, during a drag, or without a
//! change of the content height (the scrollbar thumb, a jump); otherwise the layout moved the offset (a
//! row grew, or WebKit moved the offset back while rows laid out) and a following view returns to the end.

use crate::agents::session::{ENTRY_BATCH, rows_start};
use crate::agents::store::ScrollPosition;
use ide_agent::AgentEntry;

/// Closer than this to the end counts as at the end.
pub const AT_END_PX: f64 = 40.0;
/// Scrolled this close to the top of what is painted, the earlier entries are loaded.
pub const NEAR_TOP_PX: f64 = 120.0;
/// Earlier entries are loaded once scrolling has been still this long.
pub const STILL_MS: f64 = 120.0;
/// A scroll this soon after user input is the user's.
pub const USER_SCROLL_MS: f64 = 500.0;

/// The scroll offset, the content height and the viewport height.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geometry {
    pub top: f64,
    pub height: f64,
    pub viewport: f64,
}

impl Geometry {
    fn at_end(self) -> bool {
        self.height - self.top - self.viewport < AT_END_PX
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ScrollEffect {
    /// Set the scroll offset.
    SetTop(f64),
    /// Scroll smoothly to the offset.
    SmoothTo(f64),
    /// Keep the conversation's position in the store ([`crate::agents::store::AgentsStore::set_scroll_position`]).
    Save(ScrollPosition),
    /// At `at_ms`, call [`Follow::due`].
    WakeAt(f64),
    /// More rows are painted: render again, then call [`Follow::laid_out`].
    Rerender,
}

/// The scroll state of one conversation view.
#[derive(Clone, Debug)]
pub struct Follow {
    position: ScrollPosition,
    /// The kept offset was applied once laid out.
    restored: bool,
    previous_len: usize,
    away: bool,
    /// How many of the last entries are painted.
    shown: usize,
    dragging: bool,
    user_input_ms: Option<f64>,
    seen_height: f64,
    load_at: Option<f64>,
    /// The top of the first painted row before earlier rows were prepended.
    anchor: Option<f64>,
}

impl Follow {
    /// A view of a conversation of `len` entries with its kept position, if any. Opened at the end, the
    /// last entries come first; a kept offset needs them all.
    pub fn open(saved: Option<ScrollPosition>, len: usize) -> Self {
        let position = saved.unwrap_or(ScrollPosition { top: 0.0, stick: true });
        let away = !position.stick;
        Self {
            position,
            restored: false,
            previous_len: len,
            away,
            shown: if away { usize::MAX } else { ENTRY_BATCH },
            dragging: false,
            user_input_ms: None,
            seen_height: 0.0,
            load_at: None,
            anchor: None,
        }
    }

    pub fn position(&self) -> ScrollPosition {
        self.position
    }

    /// The first entry painted ([`crate::agents::session::conversation_rows`] from it).
    pub fn start(&self, entries: &[AgentEntry], finding: bool) -> usize {
        rows_start(entries, self.shown, finding)
    }

    /// Whether Jump to Latest is offered: scrolled away from the end of a conversation with entries.
    pub fn shows_jump_to_latest(&self, entries: &[AgentEntry]) -> bool {
        self.away && !entries.is_empty()
    }

    /// Wheel, touch, keys or a pointer press in the conversation.
    pub fn user_input(&mut self, now: f64) {
        self.user_input_ms = Some(now);
    }

    /// A pointer press in the conversation (on the scrollbar, say): earlier entries wait until it is
    /// released, so the scrollbar isn't resized under the pointer.
    pub fn pointer_down(&mut self, now: f64) {
        self.user_input(now);
        self.dragging = true;
        self.load_at = None;
    }

    /// The pointer was released (or the window lost focus).
    pub fn pointer_up(&mut self, g: Geometry, now: f64) -> Option<ScrollEffect> {
        if !self.dragging {
            return None;
        }
        self.dragging = false;
        (self.position.stick || g.top < NEAR_TOP_PX).then(|| self.load_when_still(now))
    }

    fn load_when_still(&mut self, now: f64) -> ScrollEffect {
        let at = now + STILL_MS;
        self.load_at = Some(at);
        ScrollEffect::WakeAt(at)
    }

    fn by_user(&self, laid_out: bool, now: f64) -> bool {
        self.dragging || self.user_input_ms.is_some_and(|t| now - t < USER_SCROLL_MS) || !laid_out
    }

    /// The view scrolled to `g`.
    pub fn scrolled(&mut self, g: Geometry, now: f64) -> Vec<ScrollEffect> {
        let at_end = g.at_end();
        let laid_out = g.height != self.seen_height;
        self.seen_height = g.height;
        let stick = at_end || (self.position.stick && !self.by_user(laid_out, now));
        let mut effects = Vec::new();
        let mut top = g.top;
        if stick && !at_end {
            // Still following: back to the end the offset was moved away from.
            top = g.height;
            effects.push(ScrollEffect::SetTop(top));
        }
        self.position = ScrollPosition { top, stick };
        effects.push(ScrollEffect::Save(self.position));
        self.away = !stick;
        // Reached the top of what is painted: the earlier entries load once the drag has settled.
        self.load_at = None;
        if !stick && top < NEAR_TOP_PX && !self.dragging {
            effects.push(self.load_when_still(now));
        }
        effects
    }

    /// A [`ScrollEffect::WakeAt`] came due. `first_row_top` is the top of the first painted row, which
    /// stays on screen while earlier rows are prepended above it.
    pub fn due(
        &mut self,
        now: f64,
        entries: &[AgentEntry],
        finding: bool,
        first_row_top: Option<f64>,
    ) -> Option<ScrollEffect> {
        if !self.load_at.is_some_and(|at| now >= at) {
            return None;
        }
        self.load_at = None;
        if self.start(entries, finding) == 0 || self.dragging || self.anchor.is_some() {
            return None;
        }
        if !self.position.stick {
            self.anchor = first_row_top;
        }
        self.shown = self.shown.saturating_add(ENTRY_BATCH);
        Some(ScrollEffect::Rerender)
    }

    /// The rows were laid out at `g` (after every render). A view hidden behind another tab
    /// (`visible` false) has no layout to scroll; shown again, it follows the end again. `first_row_top`
    /// as for [`Self::due`], measured now.
    pub fn laid_out(
        &mut self,
        g: Geometry,
        entries: &[AgentEntry],
        finding: bool,
        visible: bool,
        first_row_top: Option<f64>,
    ) -> Vec<ScrollEffect> {
        let mut effects = Vec::new();
        let mut top = g.top;
        if visible {
            let len = entries.len();
            let truncated = len < self.previous_len;
            self.previous_len = len;
            // Rolled back (or emptied) while at its end: follow what comes next.
            if (truncated || len == 0) && g.at_end() {
                self.position = ScrollPosition { top: g.top, stick: true };
                self.away = false;
                effects.push(ScrollEffect::Save(self.position));
            }
            if self.position.stick {
                top = g.height;
                effects.push(ScrollEffect::SetTop(top));
            } else if !self.restored {
                top = self.position.top;
                effects.push(ScrollEffect::SetTop(top));
            }
            self.restored = true;
        }
        // Earlier rows were prepended above the first one: follow it, so the same text stays on screen.
        if let Some(from) = self.anchor.take()
            && !self.position.stick
            && let Some(now_top) = first_row_top
        {
            effects.push(ScrollEffect::SetTop(top + now_top - from));
        }
        // Following the end, the rest is painted in the background; a user who scrolled away gets it
        // only after stopping near the top.
        if self.position.stick && !self.dragging && self.start(entries, finding) > 0 {
            self.shown = self.shown.saturating_add(ENTRY_BATCH);
            effects.push(ScrollEffect::Rerender);
        }
        effects
    }

    /// The content or the viewport changed size (a diagram drew after the last chunk, the pane resized).
    pub fn resized(&self, g: Geometry) -> Option<ScrollEffect> {
        self.position.stick.then_some(ScrollEffect::SetTop(g.height))
    }

    /// Back to the end of the conversation; following resumes from there.
    pub fn jump_to_latest(&mut self, g: Geometry) -> Vec<ScrollEffect> {
        self.position = ScrollPosition { top: g.height, stick: true };
        self.away = false;
        vec![ScrollEffect::SmoothTo(g.height), ScrollEffect::Save(self.position)]
    }

    /// The view closes at offset `top`: the position to keep, once it was laid out.
    pub fn close(&self, top: f64) -> Option<ScrollPosition> {
        self.restored.then_some(ScrollPosition { top, ..self.position })
    }
}

#[cfg(test)]
mod tests;
