use super::*;
use crate::agents::fixtures::{agent, subagent};
use crate::agents::session::conversation_rows;
use crate::agents::store::{AgentsStore, Saved};

/// A conversation view: its scroll offset and heights, applying what [`Follow`] asks for. `clamp` keeps
/// the offset within the content, as a laid-out view does.
struct Pane {
    follow: Follow,
    store: AgentsStore,
    key: &'static str,
    top: f64,
    height: f64,
    viewport: f64,
    clamp: bool,
    now: f64,
    smooth: Option<f64>,
    wake: Option<f64>,
    rerenders: usize,
}

impl Pane {
    fn open(store: AgentsStore, len: usize, height: f64, viewport: f64, clamp: bool) -> Self {
        let follow = Follow::open(store.scroll_position("A"), len);
        Self {
            follow,
            store,
            key: "A",
            top: 0.0,
            height,
            viewport,
            clamp,
            now: 0.0,
            smooth: None,
            wake: None,
            rerenders: 0,
        }
    }

    fn g(&self) -> Geometry {
        Geometry { top: self.top, height: self.height, viewport: self.viewport }
    }

    fn apply(&mut self, effects: impl IntoIterator<Item = ScrollEffect>) {
        for effect in effects {
            match effect {
                ScrollEffect::SetTop(top) => {
                    self.top = if self.clamp { top.clamp(0.0, (self.height - self.viewport).max(0.0)) } else { top };
                }
                ScrollEffect::SmoothTo(top) => self.smooth = Some(top),
                ScrollEffect::Save(position) => self.store.set_scroll_position(self.key, position),
                ScrollEffect::WakeAt(at) => self.wake = Some(at),
                ScrollEffect::Rerender => self.rerenders += 1,
            }
        }
    }

    /// Lays the rows out, again after each batch of earlier rows, until all that is due is painted.
    fn layout(&mut self, entries: &[AgentEntry], visible: bool, first_row_top: Option<f64>) {
        loop {
            let effects = self.follow.laid_out(self.g(), entries, false, visible, first_row_top);
            let again = effects.contains(&ScrollEffect::Rerender);
            self.apply(effects);
            if !again {
                break;
            }
        }
    }

    fn scroll_to(&mut self, top: f64) {
        self.top = top;
        let effects = self.follow.scrolled(self.g(), self.now);
        self.apply(effects);
    }

    fn wheel(&mut self) {
        self.follow.user_input(self.now);
    }

    fn close(self) -> AgentsStore {
        let mut store = self.store;
        if let Some(position) = self.follow.close(self.top) {
            store.set_scroll_position(self.key, position);
        }
        store
    }
}

fn conversation() -> Vec<AgentEntry> {
    vec![agent("Answer A"), subagent("child", "Explore task A")]
}

fn store() -> AgentsStore {
    AgentsStore::new(Saved::default())
}

#[test]
fn hides_jump_to_latest_and_follows_new_replies_after_retracting_the_conversation() {
    let entries = conversation();
    let mut pane = Pane::open(store(), entries.len(), 1000.0, 400.0, false);
    pane.layout(&entries, true, None);
    pane.wheel();
    pane.scroll_to(100.0);
    assert!(pane.follow.shows_jump_to_latest(&entries));
    pane.height = 400.0;
    pane.layout(&[], true, None);
    assert!(!pane.follow.shows_jump_to_latest(&entries));
    pane.height = 1400.0;
    let next = [agent("New reply")];
    pane.layout(&next, true, None);
    assert_eq!(pane.top, 1400.0);
    assert!(!pane.follow.shows_jump_to_latest(&next));
}

#[test]
fn defers_prepending_during_a_pointer_drag_and_preserves_the_first_rows_viewport_position() {
    let history: Vec<AgentEntry> = (0..90).map(|i| agent(&format!("History {i}"))).collect();
    // Opened hidden: the initial batch hasn't restored the bottom offset.
    let mut pane = Pane::open(store(), history.len(), 3000.0, 400.0, false);
    pane.layout(&history, false, None);
    // The conversation gets a new tail while the scrollbar is held.
    pane.follow.pointer_down(pane.now);
    let mut entries = history;
    entries.extend((0..90).map(|i| agent(&format!("New {i}"))));
    pane.layout(&entries, false, None);
    pane.scroll_to(20.0);
    let count = entries.len() - pane.follow.start(&entries, false);
    pane.now += 180.0;
    assert_eq!(pane.follow.due(pane.now, &entries, false, Some(100.0)), None);
    assert_eq!(entries.len() - pane.follow.start(&entries, false), count);
    // The first row's top is measured at release, then shifts down when rows are inserted.
    let wake = pane.follow.pointer_up(pane.g(), pane.now);
    pane.apply(wake);
    let at = pane.wake.expect("a load once the pointer is still");
    assert_eq!(pane.follow.due(at, &entries, false, Some(100.0)), Some(ScrollEffect::Rerender));
    pane.layout(&entries, false, Some(500.0));
    assert!(entries.len() - pane.follow.start(&entries, false) > count);
    assert_eq!(pane.top, 420.0);
}

#[test]
fn keeps_following_the_end_when_the_offset_moves_back_without_the_user_scrolling() {
    let entries = conversation();
    let mut pane = Pane::open(store(), entries.len(), 1000.0, 400.0, false);
    pane.layout(&entries, true, None);
    pane.scroll_to(600.0);
    // The conversation grew after it was scrolled to the end, or WebKit moved the offset back while rows
    // laid out: both change the content height.
    pane.height = 1300.0;
    pane.scroll_to(600.0);
    assert_eq!(pane.top, 1300.0);
    assert!(!pane.follow.shows_jump_to_latest(&entries));
    // Scrolling up with the wheel leaves the end.
    pane.wheel();
    pane.scroll_to(600.0);
    assert_eq!(pane.top, 600.0);
    assert!(pane.follow.shows_jump_to_latest(&entries));
}

#[test]
fn leaves_the_end_for_a_scroll_that_no_layout_caused_as_dragging_the_scrollbar_thumb() {
    let entries = conversation();
    let mut pane = Pane::open(store(), entries.len(), 1000.0, 400.0, false);
    pane.layout(&entries, true, None);
    pane.scroll_to(600.0);
    pane.scroll_to(0.0);
    assert_eq!(pane.top, 0.0);
    assert!(pane.follow.shows_jump_to_latest(&entries));
}

#[test]
fn offers_one_click_back_to_the_latest_message_after_scrolling_up() {
    let entries = conversation();
    let mut pane = Pane::open(store(), entries.len(), 1000.0, 400.0, false);
    pane.layout(&entries, true, None);
    pane.wheel();
    pane.scroll_to(100.0);
    assert!(pane.follow.shows_jump_to_latest(&entries));
    let jump = pane.follow.jump_to_latest(pane.g());
    pane.apply(jump);
    assert_eq!(pane.smooth, Some(1000.0));
    assert!(!pane.follow.shows_jump_to_latest(&entries));
}

#[test]
fn opens_a_long_conversation_at_its_end_and_renders_the_earlier_entries_after_it() {
    let entries: Vec<AgentEntry> = (0..150).map(|i| agent(&format!("Reply {i}"))).collect();
    let mut pane = Pane::open(store(), entries.len(), 3000.0, 400.0, false);
    let first = conversation_rows(&entries, pane.follow.start(&entries, false));
    assert_eq!(first.last().map(|row| row.index()), Some(149));
    assert!(first.len() < entries.len());
    pane.layout(&entries, true, None);
    assert!(pane.rerenders > 0);
    assert_eq!(conversation_rows(&entries, pane.follow.start(&entries, false)).len(), 150);
}

#[test]
fn retains_scrolling_and_follow_output_when_moving_in_both_directions() {
    for stick in [false, true] {
        let mut entries = conversation();
        let mut pane = Pane::open(store(), entries.len(), 2000.0, 400.0, true);
        pane.layout(&entries, true, None);
        pane.wheel();
        pane.scroll_to(if stick { 1600.0 } else { 230.0 });
        let kept = ScrollPosition { top: if stick { 1600.0 } else { 230.0 }, stick };
        assert_eq!(pane.store.scroll_position("A"), Some(kept));
        // Docked in the tool window, with a shorter viewport.
        let mut dock = Pane::open(pane.close(), entries.len(), 2000.0, 150.0, true);
        dock.layout(&entries, true, None);
        assert_eq!(dock.top, if stick { 1850.0 } else { 230.0 });
        dock.height = 2400.0;
        entries.push(agent("More output"));
        dock.layout(&entries, true, None);
        assert_eq!(dock.top, if stick { 2250.0 } else { 230.0 });
        // Back in the editor.
        let mut editor = Pane::open(dock.close(), entries.len(), 2400.0, 400.0, true);
        editor.layout(&entries, true, None);
        assert_eq!(editor.top, if stick { 2000.0 } else { 230.0 }, "stick: {stick}");
    }
}
