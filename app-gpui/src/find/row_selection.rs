//! Multi-selection of list rows: a click or ↑↓ selects one row, ⇧ extends from the anchor, ⌘/Ctrl toggles a
//! row.

use std::collections::BTreeSet;

/// A row with a key that identifies it while the list changes.
pub trait Keyed {
    fn key(&self) -> &str;
}

/// Which rows of a list are selected.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RowSelection {
    /// Active row (keyboard focus, preview); `None` before any row is active.
    pub lead: Option<usize>,
    /// Start of ⇧ ranges.
    pub anchor: Option<usize>,
    /// Keys of the selected rows; `None` when only the lead is selected.
    pub keys: Option<BTreeSet<String>>,
}

/// The first row alone.
pub const SINGLE_FIRST: RowSelection = RowSelection { lead: Some(0), anchor: Some(0), keys: None };

/// How a click or key press changes the selection.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SelectMode {
    #[default]
    Single,
    Range,
    Toggle,
}

/// The mode of an event: ⇧ extends, a click with the platform's primary modifier toggles.
pub fn selection_mode(shift: bool, primary_click: bool) -> SelectMode {
    if shift {
        SelectMode::Range
    } else if primary_click {
        SelectMode::Toggle
    } else {
        SelectMode::Single
    }
}

/// The selection after choosing row `index`. Section rows (`is_section`) are never selected, and an index
/// outside the list changes nothing.
pub fn select_row<T: Keyed>(
    rows: &[T],
    selection: &RowSelection,
    index: usize,
    mode: SelectMode,
    is_section: Option<&dyn Fn(&T) -> bool>,
) -> RowSelection {
    let section = |row: &T| is_section.is_some_and(|is_section| is_section(row));
    let Some(row) = rows.get(index).filter(|row| !section(row)) else { return selection.clone() };
    let single = RowSelection { lead: Some(index), anchor: Some(index), keys: None };
    match mode {
        SelectMode::Single => single,
        SelectMode::Range => {
            let anchor = selection.anchor.unwrap_or(0).min(rows.len() - 1);
            let (from, to) = (anchor.min(index), anchor.max(index));
            let keys = rows[from..=to].iter().filter(|row| !section(row)).map(|row| row.key().to_owned()).collect();
            RowSelection { lead: Some(index), anchor: Some(anchor), keys: Some(keys) }
        }
        SelectMode::Toggle => {
            let mut keys = selection.keys.clone().unwrap_or_else(|| {
                selection.lead.and_then(|lead| rows.get(lead)).map(|lead| lead.key().to_owned()).into_iter().collect()
            });
            if keys.insert(row.key().to_owned()) {
                return RowSelection { keys: Some(keys), ..single };
            }
            keys.remove(row.key());
            match rows.iter().rposition(|row| keys.contains(row.key())) {
                Some(rest) => RowSelection { lead: Some(rest), anchor: Some(rest), keys: Some(keys) },
                None => single,
            }
        }
    }
}

/// The selected rows in list order (the lead when nothing else is selected).
pub fn selected_rows<'a, T: Keyed>(rows: &'a [T], selection: &RowSelection) -> Vec<&'a T> {
    let Some(lead) = selection.lead.and_then(|lead| rows.get(lead.min(rows.len().saturating_sub(1)))) else {
        return Vec::new();
    };
    let selected: Vec<&T> = match &selection.keys {
        Some(keys) => rows.iter().filter(|row| keys.contains(row.key())).collect(),
        None => Vec::new(),
    };
    if selected.is_empty() { vec![lead] } else { selected }
}

/// Whether the row with `key` is part of a multi-selection.
pub fn is_row_selected(selection: &RowSelection, key: &str) -> bool {
    selection.keys.as_ref().is_some_and(|keys| keys.contains(key))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Row(&'static str);

    impl Keyed for Row {
        fn key(&self) -> &str {
            self.0
        }
    }

    fn keys(rows: &[Row], selection: &RowSelection) -> Vec<&'static str> {
        selected_rows(rows, selection).into_iter().map(|row| row.0).collect()
    }

    // src/features/search/__tests__/findScopes.test.ts › results selection › ⇧ extends from the anchor, ⌘ toggles,
    // a plain click selects one
    #[test]
    fn shift_extends_from_the_anchor_cmd_toggles_a_plain_click_selects_one() {
        let rows = [Row("a"), Row("b"), Row("c"), Row("d")];
        let mut selection = select_row(&rows, &SINGLE_FIRST, 1, SelectMode::Single, None);
        selection = select_row(&rows, &selection, 3, SelectMode::Range, None);
        assert_eq!(keys(&rows, &selection), ["b", "c", "d"]);
        selection = select_row(&rows, &selection, 0, SelectMode::Range, None);
        assert_eq!(keys(&rows, &selection), ["a", "b"]);
        selection = select_row(&rows, &selection, 3, SelectMode::Toggle, None);
        assert_eq!(keys(&rows, &selection), ["a", "b", "d"]);
        selection = select_row(&rows, &selection, 3, SelectMode::Toggle, None);
        assert_eq!(keys(&rows, &selection), ["a", "b"]);
        assert_eq!(selection.lead, Some(1));
        selection = select_row(&rows, &selection, 2, SelectMode::Single, None);
        assert_eq!(keys(&rows, &selection), ["c"]);
    }

    // Not from a spec: section rows are skipped, an index outside the list changes nothing, and toggling the
    // only selected row off leaves it as the lead.
    #[test]
    fn sections_and_missing_rows_are_never_selected() {
        let rows = [Row("#s"), Row("a"), Row("b")];
        let is_section = |row: &Row| row.0.starts_with('#');
        let start = RowSelection { lead: Some(1), anchor: Some(1), keys: None };
        assert_eq!(select_row(&rows, &start, 0, SelectMode::Single, Some(&is_section)), start);
        assert_eq!(select_row(&rows, &start, 9, SelectMode::Single, None), start);
        let range = select_row(&rows, &RowSelection::default(), 2, SelectMode::Range, Some(&is_section));
        assert_eq!(keys(&rows, &range), ["a", "b"]);
        assert!(is_row_selected(&range, "a") && !is_row_selected(&range, "#s"));
        let off = select_row(&rows, &start, 1, SelectMode::Toggle, None);
        assert_eq!(off, start);
        assert!(selected_rows(&rows, &RowSelection::default()).is_empty());
        assert_eq!(selection_mode(true, true), SelectMode::Range);
        assert_eq!(selection_mode(false, true), SelectMode::Toggle);
        assert_eq!(selection_mode(false, false), SelectMode::Single);
    }
}
