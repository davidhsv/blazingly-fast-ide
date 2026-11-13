use super::*;

struct Entry {
    key: &'static str,
    section: bool,
}

impl Keyed for Entry {
    fn key(&self) -> &str {
        self.key
    }
}

fn entries_selected(rows: &[Entry], selection: &RowSelection) -> Vec<&'static str> {
    selected_rows(rows, selection).into_iter().map(|row| row.key).collect()
}

// src/ui/rowSelection.test.ts › list selection across sections › selects only options across headers in either
// direction
/// A ⇧ range from an option across section headers holds only the options, downwards and upwards from the
/// anchor, and toggling a header changes nothing.
#[test]
fn selects_only_options_across_headers_in_either_direction() {
    let entry = |key, section| Entry { key, section };
    let rows = [
        entry("files", true),
        entry("a", false),
        entry("symbols", true),
        entry("b", false),
        entry("actions", true),
        entry("c", false),
    ];
    let is_section = |row: &Entry| row.section;
    let single = RowSelection { lead: Some(3), anchor: Some(3), keys: None };
    let down = select_row(&rows, &single, 5, SelectMode::Range, Some(&is_section));
    assert_eq!(entries_selected(&rows, &down), ["b", "c"]);
    let up = select_row(&rows, &down, 1, SelectMode::Range, Some(&is_section));
    assert_eq!(entries_selected(&rows, &up), ["a", "b"]);
    assert_eq!(select_row(&rows, &up, 2, SelectMode::Toggle, Some(&is_section)), up);
}
