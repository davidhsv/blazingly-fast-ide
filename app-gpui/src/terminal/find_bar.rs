//! ⌘F in the terminal: search the scrollback with Match Case / Words / Regex.

use super::keys::KeyEvent;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TerminalSearchOptions {
    pub case_sensitive: bool,
    pub regex: bool,
    pub whole_word: bool,
}

/// One of the [`TerminalSearchOptions`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchOption {
    CaseSensitive,
    WholeWord,
    Regex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalSearchResult {
    /// The current match, 0-based; `None` when it is unknown (more matches than are highlighted).
    pub index: Option<usize>,
    pub count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindDirection {
    Next,
    Previous,
}

/// "3/12", "999+" style counter text.
pub fn match_counter(result: Option<TerminalSearchResult>, query: &str) -> String {
    let Some(result) = result.filter(|_| !query.is_empty()) else { return String::new() };
    match result {
        TerminalSearchResult { count: 0, .. } => "0 results".to_owned(),
        TerminalSearchResult { index: Some(index), count } => format!("{}/{count}", index + 1),
        TerminalSearchResult { index: None, count } => format!("{count}+"),
    }
}

/// The find bar of the terminal tool window.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TerminalFind {
    pub open: bool,
    /// Grows with every open request: the bar takes the focus once per request, not whenever
    /// it is rebuilt (another tab group becoming active rebuilds it).
    pub seq: u64,
    pub query: String,
    pub options: TerminalSearchOptions,
}

impl TerminalFind {
    pub fn new() -> Self {
        Self::default()
    }

    /// Opens (or refocuses) the find bar, seeding it with the terminal's `selection` when that is
    /// on one line.
    pub fn open(&mut self, selection: Option<&str>) {
        // While open, the terminal selection is the current match: keep the query.
        let seed = selection.filter(|_| !self.open).map(str::trim);
        if let Some(seed) = seed.filter(|seed| !seed.is_empty() && !seed.contains('\n')) {
            seed.clone_into(&mut self.query);
        }
        self.open = true;
        self.seq += 1;
    }

    /// Closes the find bar. Returns whether it was open: then the caller clears the search and
    /// gives the focus back to the terminal.
    pub fn close(&mut self) -> bool {
        std::mem::replace(&mut self.open, false)
    }

    pub fn toggle(&mut self, option: SearchOption) {
        let flag = match option {
            SearchOption::CaseSensitive => &mut self.options.case_sensitive,
            SearchOption::WholeWord => &mut self.options.whole_word,
            SearchOption::Regex => &mut self.options.regex,
        };
        *flag = !*flag;
    }
}

/// What a key pressed in the find bar's field does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindBarKey {
    Close,
    Find(FindDirection),
    Toggle(SearchOption),
}

/// The find bar's own keys: Escape closes it, Enter / F3 / ⌘G go to the next occurrence (the
/// previous one with Shift), Alt+C / W / X toggle an option. `None` for every other key. `mac`:
/// the app runs on macOS, where the primary modifier is ⌘ rather than Ctrl.
pub fn find_bar_key(e: &KeyEvent, mac: bool) -> Option<FindBarKey> {
    let mods = e.mods;
    let direction = if mods.shift { FindDirection::Previous } else { FindDirection::Next };
    let primary = if mac { mods.meta } else { mods.ctrl };
    if e.key == "Escape" {
        Some(FindBarKey::Close)
    } else if e.key == "Enter" || e.key == "F3" || (primary && e.key.eq_ignore_ascii_case("g")) {
        Some(FindBarKey::Find(direction))
    } else if mods.alt && !mods.meta && !mods.ctrl {
        let option = match e.code {
            "KeyC" => SearchOption::CaseSensitive,
            "KeyW" => SearchOption::WholeWord,
            "KeyX" => SearchOption::Regex,
            _ => return None,
        };
        Some(FindBarKey::Toggle(option))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::dispatcher::Mods;

    // src/features/terminal/terminal.test.ts › terminal find › formats the match counter
    #[test]
    fn formats_the_match_counter() {
        let found = |index, count| Some(TerminalSearchResult { index, count });
        assert_eq!(match_counter(None, "x"), "");
        assert_eq!(match_counter(found(Some(2), 12), "x"), "3/12");
        assert_eq!(match_counter(found(None, 2000), "x"), "2000+");
        assert_eq!(match_counter(found(None, 0), "x"), "0 results");
        assert_eq!(match_counter(found(Some(0), 3), ""), "");
    }

    // Not from a spec: opening seeds the query from a one-line selection, and keeps it while open.
    #[test]
    fn opens_with_the_selection_and_closes_once() {
        let mut find = TerminalFind::new();
        find.open(Some("  needle \n"));
        assert_eq!((find.open, find.seq, find.query.as_str()), (true, 1, "needle"));
        find.open(Some("the current match"));
        assert_eq!((find.seq, find.query.as_str()), (2, "needle"));
        assert!(find.close());
        assert!(!find.close());
        find.open(Some("two\nlines"));
        assert_eq!(find.query, "needle");
        find.toggle(SearchOption::Regex);
        assert_eq!(find.options, TerminalSearchOptions { regex: true, ..TerminalSearchOptions::default() });
    }

    // Not from a spec: the keys of the find bar's field.
    #[test]
    fn reads_the_find_bar_keys() {
        let shift = Mods { shift: true, ..Mods::NONE };
        let alt = Mods { alt: true, ..Mods::NONE };
        let meta = Mods { meta: true, ..Mods::NONE };
        let key = |key, code, mods| find_bar_key(&KeyEvent::new(key, code, mods), true);
        assert_eq!(key("Escape", "Escape", Mods::NONE), Some(FindBarKey::Close));
        assert_eq!(key("Enter", "Enter", Mods::NONE), Some(FindBarKey::Find(FindDirection::Next)));
        assert_eq!(key("Enter", "Enter", shift), Some(FindBarKey::Find(FindDirection::Previous)));
        assert_eq!(key("F3", "F3", shift), Some(FindBarKey::Find(FindDirection::Previous)));
        assert_eq!(key("g", "KeyG", meta), Some(FindBarKey::Find(FindDirection::Next)));
        assert_eq!(key("ç", "KeyC", alt), Some(FindBarKey::Toggle(SearchOption::CaseSensitive)));
        assert_eq!(key("x", "KeyX", Mods::NONE), None);
        let on_windows = find_bar_key(&KeyEvent::new("g", "KeyG", meta), false);
        assert_eq!(on_windows, None);
    }
}
