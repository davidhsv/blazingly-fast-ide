//! Where the caret has been, for the commands that go back there:
//!  - [`NavigationHistory`]: Back / Forward (⌘[ / ⌘]);
//!  - [`EditHistory`]: Navigate › Last Edit Location (⇧⌘⌫) / Next Edit Location;
//!  - [`SelectionStack`]: Extend / Shrink Selection (⌥↑ / ⌥↓).
//!
//! Lines are zero-based, columns and offsets count UTF-16 code units.

/// Back entries of one file closer than this many lines are one place.
pub const NAV_MERGE_LINES: u32 = 4;
const NAV_MAX_ENTRIES: usize = 100;

/// An edit at most this many lines from the newest edit place of the file updates that place.
pub const EDIT_MERGE_LINES: u32 = 3;
const EDIT_MAX_ENTRIES: usize = 60;

/// User event of in-file navigation commands (method up / down, code block start / end…).
pub const NAVIGATE_EVENT: &str = "select.navigate";

/// User events of caret moves that count as navigation: mouse clicks, reveals, find, stripe and
/// sticky-line clicks, navigate commands.
const NAVIGATION_EVENTS: [&str; 6] =
    ["select.pointer", "select.reveal", "select.search", "select.overview", "select.sticky", NAVIGATE_EVENT];

/// Whether a caret move tagged `user_event` is a navigation: the event is one of the navigation
/// events or a dotted sub-event of one (`select.pointer.drag`).
pub fn is_navigation(user_event: &str) -> bool {
    NAVIGATION_EVENTS
        .iter()
        .any(|event| user_event.strip_prefix(*event).is_some_and(|rest| rest.is_empty() || rest.starts_with('.')))
}

/// A caret position in a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    pub path: String,
    pub line: u32,
    pub col: u32,
}

/// One entry of the Back / Forward history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavigationLocation {
    pub path: String,
    pub line: u32,
    pub col: u32,
    /// Milliseconds since the Unix epoch when the location was last visited.
    pub time: u64,
}

/// A copy of the Back / Forward history for its readers (Recent Locations).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavigationSnapshot {
    /// Oldest first.
    pub entries: Vec<NavigationLocation>,
    /// Position of the current location in `entries`; `None` when empty.
    pub index: Option<usize>,
}

/// Back / Forward navigation history, like IDEA's: the entry at `index` is the current place,
/// kept up to date while the caret moves. Only a navigation (a file switch, a mouse click, a jump
/// to a declaration / usage / search result / line…) that changes the caret line leaves the
/// previous place behind as a back entry and drops the forward stack; typing, arrows and paging
/// don't. A back entry within [`NAV_MERGE_LINES`] of the one before it replaces it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NavigationHistory {
    entries: Vec<NavigationLocation>,
    index: Option<usize>,
}

impl NavigationHistory {
    /// Oldest first.
    pub fn entries(&self) -> &[NavigationLocation] {
        &self.entries
    }

    /// Position of the current place in the entries; `None` when empty.
    pub fn index(&self) -> Option<usize> {
        self.index
    }

    pub fn current(&self) -> Option<&NavigationLocation> {
        self.index.and_then(|index| self.entries.get(index))
    }

    pub fn can_go_back(&self) -> bool {
        self.index.is_some_and(|index| index > 0)
    }

    pub fn can_go_forward(&self) -> bool {
        self.index.map_or(0, |index| index + 1) < self.entries.len()
    }

    /// Records the caret position at time `now` (milliseconds since the Unix epoch).
    /// `navigation`: the move came from a navigation command; a file switch always counts as one.
    /// Returns whether a place was added, which changes what Back and Forward lead to.
    pub fn update(&mut self, place: Place, navigation: bool, now: u64) -> bool {
        let entry = NavigationLocation { path: place.path, line: place.line, col: place.col, time: now };
        let Some(index) = self.index.filter(|&index| index < self.entries.len()) else {
            self.entries = vec![entry];
            self.index = Some(0);
            return true;
        };
        let current = &self.entries[index];
        if current.path == entry.path && (!navigation || current.line == entry.line) {
            self.entries[index] = entry;
            return false;
        }
        // The place we leave becomes a back entry, merged with a close one before it.
        self.entries.truncate(index + 1);
        if index > 0 && same_place(&self.entries[index - 1], &self.entries[index]) {
            self.entries.remove(index - 1);
        }
        self.entries.push(entry);
        let excess = self.entries.len().saturating_sub(NAV_MAX_ENTRIES);
        self.entries.drain(..excess);
        self.index = Some(self.entries.len() - 1);
        true
    }

    /// Moves back; returns the location to navigate to.
    pub fn back(&mut self) -> Option<&NavigationLocation> {
        let index = self.index?.checked_sub(1)?;
        self.index = Some(index);
        self.entries.get(index)
    }

    /// Moves forward; returns the location to navigate to.
    pub fn forward(&mut self) -> Option<&NavigationLocation> {
        if !self.can_go_forward() {
            return None;
        }
        let index = self.index.map_or(0, |index| index + 1);
        self.index = Some(index);
        self.entries.get(index)
    }

    /// Drops entries of a deleted file. Returns whether there were any.
    pub fn remove_path(&mut self, path: &str) -> bool {
        let before = self.entries.len();
        let current = self.index.filter(|&index| self.entries.get(index).is_some_and(|entry| entry.path != path));
        let kept_before = current.map(|index| self.entries[..index].iter().filter(|entry| entry.path != path).count());
        self.entries.retain(|entry| entry.path != path);
        if self.entries.len() == before {
            return false;
        }
        self.index = kept_before.or_else(|| self.entries.len().checked_sub(1));
        true
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.index = None;
    }

    pub fn snapshot(&self) -> NavigationSnapshot {
        NavigationSnapshot { entries: self.entries.clone(), index: self.index }
    }
}

fn same_place(a: &NavigationLocation, b: &NavigationLocation) -> bool {
    a.path == b.path && a.line.abs_diff(b.line) < NAV_MERGE_LINES
}

/// Edit locations for Last Edit Location / Next Edit Location, like IDEA's: every edit records
/// the caret; edits within [`EDIT_MERGE_LINES`] lines of the newest place update it instead of
/// adding one. Repeated Last Edit Location walks back through older places; an edit resets the
/// walk.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EditHistory {
    entries: Vec<Place>,
    /// Position of the last Last / Next Edit Location target; `entries.len()` while not walking.
    index: usize,
}

impl EditHistory {
    /// Oldest first.
    pub fn entries(&self) -> &[Place] {
        &self.entries
    }

    /// Records an edit at `place`.
    pub fn record(&mut self, place: Place) {
        if self.entries.last().is_some_and(|last| near_edit(last, &place)) {
            self.entries.pop();
        } else {
            self.entries.retain(|entry| !near_edit(entry, &place));
        }
        self.entries.push(place);
        let excess = self.entries.len().saturating_sub(EDIT_MAX_ENTRIES);
        self.entries.drain(..excess);
        self.index = self.entries.len();
    }

    /// The place to go back to from `caret`: the newest edit, or the one before it when the caret
    /// already is there (or the walk already started).
    pub fn back(&mut self, caret: Option<&Place>) -> Option<&Place> {
        let walking = self.index != self.entries.len();
        let mut target = self.index.min(self.entries.len()).checked_sub(1)?;
        let newest = &self.entries[target];
        if !walking && caret.is_some_and(|caret| caret.path == newest.path && caret.line == newest.line) {
            target = target.checked_sub(1)?;
        }
        self.index = target;
        self.entries.get(target)
    }

    /// The place after the last walk target, towards the newest edit.
    pub fn forward(&mut self) -> Option<&Place> {
        if self.index + 1 >= self.entries.len() {
            return None;
        }
        self.index += 1;
        self.entries.get(self.index)
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.index = 0;
    }

    /// Drops the places of a deleted file.
    pub fn remove_path(&mut self, path: &str) {
        let before = self.entries.len();
        self.entries.retain(|entry| entry.path != path);
        if self.entries.len() != before {
            self.index = self.entries.len();
        }
    }
}

fn near_edit(a: &Place, b: &Place) -> bool {
    a.path == b.path && a.line.abs_diff(b.line) <= EDIT_MERGE_LINES
}

/// A text range in offsets, `from <= to`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SimpleRange {
    pub from: u32,
    pub to: u32,
}

/// Extend / Shrink Selection for one selection range. Extending pushes the current range and
/// picks the innermost candidate strictly containing it; shrinking pops back. The stack is only
/// valid while the selection is the one the last extend (or shrink) produced.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SelectionStack {
    /// Ranges to restore on shrink, innermost last.
    stack: Vec<SimpleRange>,
    /// The selection the last extend or shrink produced.
    top: Option<SimpleRange>,
}

impl SelectionStack {
    /// The range to select next from `current`: the smallest of `candidates` strictly containing
    /// it (the first one among equals); `None` when nothing does.
    pub fn extend(&mut self, current: SimpleRange, candidates: &[SimpleRange]) -> Option<SimpleRange> {
        if self.top != Some(current) {
            *self = Self::default();
        }
        let next = candidates
            .iter()
            .copied()
            .filter(|r| r.from <= current.from && r.to >= current.to && (r.from < current.from || r.to > current.to))
            .min_by_key(|r| r.to.saturating_sub(r.from))?;
        self.stack.push(current);
        self.top = Some(next);
        Some(next)
    }

    /// The range `current` was extended from; `None` (and a reset) when the selection changed
    /// since or there is nothing to shrink to.
    pub fn shrink(&mut self, current: SimpleRange) -> Option<SimpleRange> {
        if self.top != Some(current) {
            *self = Self::default();
            return None;
        }
        let next = self.stack.pop();
        self.top = next;
        next
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(path: &str, line: u32, col: u32) -> Place {
        Place { path: path.to_owned(), line, col }
    }

    fn spot(location: Option<&NavigationLocation>) -> Option<(&str, u32)> {
        location.map(|location| (location.path.as_str(), location.line))
    }

    fn lines(history: &NavigationHistory) -> Vec<u32> {
        history.entries().iter().map(|entry| entry.line).collect()
    }

    // src/editor/core/model/model.test.ts › navigation history › records file switches and navigations, not plain caret moves
    #[test]
    fn records_file_switches_and_navigations_not_plain_caret_moves() {
        let mut h = NavigationHistory::default();
        h.update(at("a", 1, 0), false, 1);
        h.update(at("a", 5, 2), false, 2);
        // Arrows / paging / typing move the current place, however far.
        h.update(at("a", 300, 0), false, 3);
        assert_eq!(h.entries().len(), 1);
        h.update(at("a", 100, 0), true, 4);
        h.update(at("b", 0, 0), false, 5);
        let places: Vec<String> = h.entries().iter().map(|entry| format!("{}:{}", entry.path, entry.line)).collect();
        assert_eq!(places, ["a:300", "a:100", "b:0"]);
        assert_eq!(spot(h.back()), Some(("a", 100)));
        assert_eq!(spot(h.back()), Some(("a", 300)));
        assert_eq!(h.back(), None);
        assert_eq!(spot(h.forward()), Some(("a", 100)));
        // Navigating somewhere new from the middle drops the forward stack.
        h.update(at("c", 3, 0), false, 6);
        let paths: Vec<&str> = h.entries().iter().map(|entry| entry.path.as_str()).collect();
        assert_eq!(paths, ["a", "a", "c"]);
        assert!(!h.can_go_forward());
        h.remove_path("a");
        let snapshot = h.snapshot();
        assert_eq!(snapshot.index, Some(0));
        assert_eq!(snapshot.entries.iter().map(|entry| entry.path.as_str()).collect::<Vec<_>>(), ["c"]);
    }

    // src/editor/core/model/model.test.ts › navigation history › a navigation on the same line records nothing
    #[test]
    fn a_navigation_on_the_same_line_records_nothing() {
        let mut h = NavigationHistory::default();
        h.update(at("a", 7, 0), false, 0);
        h.update(at("a", 7, 9), true, 0);
        assert_eq!(h.entries().len(), 1);
    }

    // src/editor/core/model/model.test.ts › navigation history › merges back places less than four lines apart
    #[test]
    fn merges_back_places_less_than_four_lines_apart() {
        let mut h = NavigationHistory::default();
        h.update(at("a", 10, 0), false, 0);
        h.update(at("a", 50, 0), true, 0);
        h.update(at("a", 12, 0), false, 0);
        h.update(at("a", 80, 0), true, 0);
        // a:10 and a:12 are one place: the newer one stays.
        assert_eq!(lines(&h), [12, 80]);
        h.update(at("a", 20, 0), true, 0);
        assert_eq!(lines(&h), [12, 80, 20]);
    }

    // src/editor/core/model/model.test.ts › navigation history › arriving at a back target updates it in place
    #[test]
    fn arriving_at_a_back_target_updates_it_in_place() {
        let mut h = NavigationHistory::default();
        h.update(at("a", 0, 0), false, 0);
        h.update(at("b", 0, 0), false, 0);
        let target = h.back().cloned().unwrap();
        h.update(at(&target.path, target.line, 4), false, 0);
        assert_eq!(h.entries().len(), 2);
        assert_eq!(h.index(), Some(0));
    }

    #[test]
    fn navigation_events_match_exactly_or_as_dotted_sub_events() {
        assert!(is_navigation("select.pointer"));
        assert!(is_navigation("select.navigate.methodDown"));
        assert!(!is_navigation("select.pointerdown"));
        assert!(!is_navigation("select"));
        assert!(!is_navigation("input.type"));
    }

    // src/editor/core/model/editHistory.test.ts › EditHistory (Last Edit Location) › goes to the newest edit, then walks back through older ones
    #[test]
    fn goes_to_the_newest_edit_then_walks_back_through_older_ones() {
        let mut h = EditHistory::default();
        h.record(at("/a.ts", 10, 0));
        h.record(at("/b.ts", 3, 0));
        h.record(at("/a.ts", 40, 2));
        assert_eq!(h.back(Some(&at("/c.ts", 0, 0))), Some(&at("/a.ts", 40, 2)));
        assert_eq!(h.back(Some(&at("/a.ts", 40, 0))), Some(&at("/b.ts", 3, 0)));
        assert_eq!(h.back(Some(&at("/b.ts", 3, 0))), Some(&at("/a.ts", 10, 0)));
        assert_eq!(h.back(Some(&at("/a.ts", 10, 0))), None);
        assert_eq!(h.forward(), Some(&at("/b.ts", 3, 0)));
    }

    // src/editor/core/model/editHistory.test.ts › EditHistory (Last Edit Location) › skips the newest place when the caret is already there
    #[test]
    fn skips_the_newest_place_when_the_caret_is_already_there() {
        let mut h = EditHistory::default();
        h.record(at("/a.ts", 1, 0));
        h.record(at("/a.ts", 20, 0));
        assert_eq!(h.back(Some(&at("/a.ts", 20, 0))), Some(&at("/a.ts", 1, 0)));
    }

    // src/editor/core/model/editHistory.test.ts › EditHistory (Last Edit Location) › merges nearby edits and resets the walk on a new edit
    #[test]
    fn merges_nearby_edits_and_resets_the_walk_on_a_new_edit() {
        let mut h = EditHistory::default();
        h.record(at("/a.ts", 10, 0));
        h.record(at("/a.ts", 12, 4));
        assert_eq!(h.entries(), [at("/a.ts", 12, 4)]);
        h.record(at("/b.ts", 0, 0));
        assert_eq!(h.back(None), Some(&at("/b.ts", 0, 0)));
        h.record(at("/a.ts", 11, 0));
        let paths: Vec<&str> = h.entries().iter().map(|entry| entry.path.as_str()).collect();
        assert_eq!(paths, ["/b.ts", "/a.ts"]);
        assert_eq!(h.back(None), Some(&at("/a.ts", 11, 0)));
    }

    // src/editor/core/model/editHistory.test.ts › EditHistory (Last Edit Location) › drops places of deleted files
    #[test]
    fn drops_places_of_deleted_files() {
        let mut h = EditHistory::default();
        h.record(at("/a.ts", 1, 0));
        h.record(at("/b.ts", 1, 0));
        h.remove_path("/b.ts");
        assert_eq!(h.back(None), Some(&at("/a.ts", 1, 0)));
    }

    const CANDIDATES: [SimpleRange; 3] =
        [SimpleRange { from: 4, to: 7 }, SimpleRange { from: 0, to: 20 }, SimpleRange { from: 3, to: 9 }];

    fn range(from: u32, to: u32) -> SimpleRange {
        SimpleRange { from, to }
    }

    // src/editor/core/model/model.test.ts › selection stack › extends to the innermost containing range and shrinks back
    #[test]
    fn extends_to_the_innermost_containing_range_and_shrinks_back() {
        let mut stack = SelectionStack::default();
        assert_eq!(stack.extend(range(5, 5), &CANDIDATES), Some(range(4, 7)));
        assert_eq!(stack.extend(range(4, 7), &CANDIDATES), Some(range(3, 9)));
        assert_eq!(stack.shrink(range(3, 9)), Some(range(4, 7)));
        assert_eq!(stack.shrink(range(4, 7)), Some(range(5, 5)));
        assert_eq!(stack.shrink(range(5, 5)), None);
    }

    // src/editor/core/model/model.test.ts › selection stack › resets when the selection changed in between
    #[test]
    fn resets_when_the_selection_changed_in_between() {
        let mut stack = SelectionStack::default();
        assert_eq!(stack.extend(range(5, 5), &CANDIDATES), Some(range(4, 7)));
        assert_eq!(stack.shrink(range(0, 1)), None);
        assert_eq!(stack, SelectionStack::default());
    }
}
