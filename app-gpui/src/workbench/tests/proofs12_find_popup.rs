use super::*;
use crate::find::popup_view::{FindMenu, FindPopup, PREVIEW_DEBOUNCE, SEARCH_DEBOUNCE};
use crate::find::session::FindStatus;
use crate::find::store::{HistoryKind, PINNED_KEY, ScopeTab, find_history};
use crate::platform::storage::UiStorage;
use gpui_kit::Modifiers;
use serde_json::json;
use std::cell::Cell;
use std::rc::Rc;

/// Two files with three matches of `order`: two on lines 2 and 5 of `a.txt`, one on line 1 of `b.txt`.
const FILES: &[(&str, &str)] = &[("a.txt", "first\nval order = 1\nx\nx\norder()\n"), ("b.txt", "// order\n")];
const A2: &str = "a.txt:2 val order = 1";
const A5: &str = "a.txt:5 order()";
const B1: &str = "b.txt:1 // order";
const ROWS: [&str; 3] = ["find-popup-row-0", "find-popup-row-1", "find-popup-row-2"];

fn find_popup(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<FindPopup> {
    workbench.read_with(cx, |w, _| w.find_popup()).expect("Find in Files is open")
}

/// Lets the query rest and the backend answer until the search for what the options ask now has ended;
/// the rows it found.
fn searched(popup: &Entity<FindPopup>, cx: &mut VisualTestContext) -> Vec<String> {
    for _ in 0..300 {
        cx.executor().advance_clock(SEARCH_DEBOUNCE + PREVIEW_DEBOUNCE);
        cx.run_until_parked();
        if popup.read_with(cx, |popup, _| popup.status() == FindStatus::Done && !popup.is_stale()) {
            return popup.read_with(cx, |popup, _| popup.lines());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("the search did not finish within three seconds");
}

/// Find (or Replace) in Files on the window's project, with `query` searched for.
fn open_find(
    workbench: &Entity<Workbench>,
    replace: bool,
    query: &str,
    cx: &mut VisualTestContext,
) -> Entity<FindPopup> {
    let id = if replace { "find.replaceInPath" } else { "find.inPath" };
    run_action(cx, id, Some(json!({ "query": query })));
    let popup = find_popup(workbench, cx);
    searched(&popup, cx);
    popup
}

fn rows(popup: &Entity<FindPopup>, cx: &mut VisualTestContext) -> Vec<String> {
    popup.read_with(cx, |popup, _| popup.lines())
}

fn sorted(mut lines: Vec<String>) -> Vec<String> {
    lines.sort();
    lines
}

fn selected(popup: &Entity<FindPopup>, cx: &mut VisualTestContext) -> Option<String> {
    popup.read_with(cx, |popup, _| popup.selected())
}

fn chosen(popup: &Entity<FindPopup>, cx: &mut VisualTestContext) -> Vec<String> {
    popup.read_with(cx, |popup, _| popup.chosen_lines())
}

/// The matches marked on the selected row's line, as UTF-16 column ranges.
fn marks_of_selected(popup: &Entity<FindPopup>, cx: &mut VisualTestContext) -> Vec<(u32, u32)> {
    popup.read_with(cx, |popup, _| {
        let row = popup.selected().unwrap_or_default();
        let line: u32 = row
            .split_once(':')
            .and_then(|(_, rest)| rest.split_once(' '))
            .map_or(0, |(line, _)| line.parse::<u32>().unwrap_or(1).saturating_sub(1));
        popup.preview_marks(line)
    })
}

fn read(root: &std::path::Path, name: &str) -> String {
    std::fs::read_to_string(root.join(name)).expect("the file is read")
}

fn files_unchanged(root: &std::path::Path) -> bool {
    FILES.iter().all(|(name, text)| read(root, name) == *text)
}

fn click_with(cx: &mut VisualTestContext, selector: &'static str, modifiers: Modifiers) {
    cx.run_until_parked();
    let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
    cx.simulate_click(bounds.center(), modifiers);
    cx.run_until_parked();
}

/// Moves the keyboard down the results until `row` is the selected one.
fn select_row(popup: &Entity<FindPopup>, row: &str, cx: &mut VisualTestContext) {
    for _ in 0..3 {
        if selected(popup, cx).as_deref() == Some(row) {
            return;
        }
        cx.simulate_keystrokes("down");
    }
    assert_eq!(selected(popup, cx).as_deref(), Some(row), "in {:?}", rows(popup, cx));
}

fn stored(cx: &mut VisualTestContext, key: &str) -> Option<String> {
    cx.update(|_, cx| cx.global::<UiStorage>().get(key).map(str::to_owned))
}

fn recent_searches(cx: &mut VisualTestContext) -> Vec<String> {
    stored(cx, HistoryKind::Find.storage_key()).map(|text| find_history(&text)).unwrap_or_default()
}

/// The file the active editor shows.
fn edited(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<PathBuf> {
    workbench.read_with(cx, |w, cx| w.editor().map(|editor| editor.read(cx).path().to_path_buf()))
}

// src/features/search/__tests__/findPopup.test.tsx › Find in Files popup › ⇧↓ extends the selection; ↵ in Replace
// mode replaces the selected rows
/// In Replace in Files, ⇧↓ adds the next row to the selection and the Replace button counts both rows'
/// occurrences; Enter replaces exactly those, on disk, and moves to the row still to replace.
#[gpui_kit::test]
async fn shift_down_extends_the_selection_and_enter_in_replace_mode_replaces_the_selected_rows(
    cx: &mut TestAppContext,
) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, FILES);
    let popup = open_find(&workbench, true, "order", cx);
    popup.update(cx, |popup, cx| popup.set_replacement("item", cx));
    let found = searched(&popup, cx);
    assert_eq!(sorted(found.clone()), [A2, A5, B1]);
    select_row(&popup, A2, cx);
    cx.simulate_keystrokes("shift-down");
    assert_eq!(chosen(&popup, cx), [A2, A5]);
    assert_eq!(popup.update(cx, |popup, cx| popup.replace_label(cx)), "Replace 2 Occurrences");

    cx.simulate_keystrokes("enter");
    wait_long(cx, "the two rows to be replaced", |cx| {
        popup.read_with(cx, |popup, _| !popup.is_replacing() && popup.replaced_lines().len() == 2)
    });
    assert_eq!(read(&root, "a.txt"), "first\nval item = 1\nx\nx\nitem()\n");
    assert_eq!(read(&root, "b.txt"), "// order\n", "the row that was not selected is left");
    // The next row still to replace is selected, when it comes after the replaced ones.
    let b_after = found.iter().position(|row| row == B1) > found.iter().position(|row| row == A5);
    if b_after {
        assert_eq!(selected(&popup, cx).as_deref(), Some(B1));
    }
    cx.simulate_keystrokes("escape");
    close_backend_project(cx);
}

// src/features/search/__tests__/findPopup.test.tsx › Find in Files popup › ⌘-click toggles rows into the selection
/// A click with the primary modifier adds a row to the selection; a plain click selects that row alone.
#[gpui_kit::test]
async fn cmd_click_toggles_rows_into_the_selection(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _root, workbench, cx) = trusted_project(cx, FILES);
    let popup = open_find(&workbench, false, "order", cx);
    assert_eq!(chosen(&popup, cx).len(), 1);
    let primary = cx.update(|_, cx| Modifiers::secondary_key(cx));
    click_with(cx, ROWS[2], primary);
    assert_eq!(chosen(&popup, cx).len(), 2);
    click_with(cx, ROWS[1], Modifiers::default());
    let found = rows(&popup, cx);
    assert_eq!(chosen(&popup, cx), [found[1].clone()]);
    cx.simulate_keystrokes("escape");
    close_backend_project(cx);
}

// src/features/search/__tests__/findPopup.test.tsx › Find in Files popup › the pin is a remembered toggle that keeps
// the popup open when a result is opened
/// The pin is stored as on; with it, Enter opens the selected match in the editor and the popup stays open
/// with its footer.
#[gpui_kit::test]
async fn the_pin_is_a_remembered_toggle_that_keeps_the_popup_open_when_a_result_is_opened(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, FILES);
    let popup = open_find(&workbench, false, "order", cx);
    click_with(cx, "find-popup-pin", Modifiers::default());
    assert!(popup.read_with(cx, |popup, _| popup.pinned()));
    assert_eq!(stored(cx, PINNED_KEY).as_deref(), Some("true"));
    let first = selected(&popup, cx).expect("a row is selected");
    let file = root.join(first.split_once(':').expect("a row names its file").0);

    cx.simulate_keystrokes("enter");
    wait_long(cx, "the match to be opened", |cx| edited(&workbench, cx).as_deref() == Some(file.as_path()));
    assert!(workbench.read_with(cx, |w, _| w.find_popup().is_some()), "the pinned popup stays open");
    assert!(cx.debug_bounds("find-popup-open-in-find-window").is_some());
    cx.simulate_keystrokes("escape");
    close_backend_project(cx);
}

// src/features/search/__tests__/findPopup.test.tsx › Find in Files popup › find-next keys move the selection and ⌥↓
// lists recent searches
/// The popup's Next and Previous Result actions move the selection along the results, and Recent Searches lists
/// the searches remembered before.
#[gpui_kit::test]
async fn find_next_keys_move_the_selection_and_alt_down_lists_recent_searches(cx: &mut TestAppContext) {
    let _home = start(cx);
    cx.update(|cx| cx.global_mut::<UiStorage>().set(HistoryKind::Find.storage_key(), r#"["older"]"#.to_owned()));
    let (_folder, _root, workbench, cx) = trusted_project(cx, FILES);
    let popup = open_find(&workbench, false, "order", cx);
    let found = rows(&popup, cx);
    assert_eq!(found.len(), 3);
    run_action(cx, "find.popup.nextResult", None);
    run_action(cx, "find.popup.nextResult", None);
    assert_eq!(selected(&popup, cx), Some(found[2].clone()));
    run_action(cx, "find.popup.previousResult", None);
    assert_eq!(selected(&popup, cx), Some(found[1].clone()));
    run_action(cx, "find.popup.recentSearches", None);
    let (menu, lines) = popup.read_with(cx, |popup, _| (popup.menu(), popup.menu_lines()));
    assert_eq!(menu, Some(FindMenu::History(HistoryKind::Find)));
    assert!(lines.contains(&"older".to_owned()), "{lines:?}");
    cx.simulate_keystrokes("escape escape");
    close_backend_project(cx);
}

// src/features/search/__tests__/findPopup.test.tsx › Find in Files popup › the Directory scope's browse button picks
// the folder, starting at the current one
/// The Directory tab's browse button asks for a folder, and the folder chosen is the one searched in.
#[gpui_kit::test]
async fn the_directory_scopes_browse_button_picks_the_folder_starting_at_the_current_one(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("src/main/a.txt", "order\n"), ("b.txt", "order\n")]);
    let src = text_of(&root.join("src"));
    run_action(cx, "find.inPath", Some(json!({ "directory": src, "query": "order" })));
    let popup = find_popup(&workbench, cx);
    assert_eq!(popup.read_with(cx, |popup, _| popup.options().scope_tab), ScopeTab::Directory);
    click_with(cx, "find-popup-browse", Modifiers::default());
    assert!(cx.did_prompt_for_paths(), "a folder is asked for");
    let folders_only = Rc::new(Cell::new(false));
    let seen = Rc::clone(&folders_only);
    let main = root.join("src/main");
    cx.simulate_path_prompt_response(move |options| {
        seen.set(options.directories && !options.files && !options.multiple);
        Some(vec![main])
    });
    cx.run_until_parked();
    assert!(folders_only.get(), "one folder is asked for");
    let main = text_of(&root.join("src/main"));
    assert_eq!(popup.read_with(cx, |popup, _| popup.options().directory.clone()), main);
    assert_eq!(searched(&popup, cx), ["src/main/a.txt:1 order"]);
    cx.simulate_keystrokes("escape");
    close_backend_project(cx);
}

// src/features/search/__tests__/findPopup.test.tsx › Find in Files popup › closing remembers the search text
/// Escape closes the popup, and what it searched for heads the recent searches.
#[gpui_kit::test]
async fn closing_remembers_the_search_text(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _root, workbench, cx) = trusted_project(cx, FILES);
    open_find(&workbench, false, "order", cx);
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(workbench.read_with(cx, |w, _| w.find_popup().is_none()));
    assert_eq!(recent_searches(cx).first().map(String::as_str), Some("order"));
    close_backend_project(cx);
}

// src/features/search/__tests__/findPopup.test.tsx › outdated popup results › marks retained rows immediately in
// replace=%s and waits for the first new file
/// A new query marks the rows of the old one as outdated at once: they stay listed without their matches
/// marked, the count gives way to "Updating results…", and Replace replaces nothing; the new search's file
/// then takes their place, counted and marked.
#[gpui_kit::test]
async fn marks_retained_rows_immediately_and_waits_for_the_first_new_file(cx: &mut TestAppContext) {
    let _home = start(cx);
    let mut files = FILES.to_vec();
    files.push(("fresh.txt", "fresh\n"));
    let (_folder, root, workbench, cx) = trusted_project(cx, &files);
    for replace in [false, true] {
        let popup = open_find(&workbench, replace, "order", cx);
        if replace {
            popup.update(cx, |popup, cx| popup.set_replacement("item", cx));
            searched(&popup, cx);
        }
        let old = rows(&popup, cx);
        assert_eq!(old.len(), 3);
        assert!(!marks_of_selected(&popup, cx).is_empty(), "the matches are marked");

        popup.update(cx, |popup, cx| popup.set_pattern("fresh", cx));
        assert!(popup.read_with(cx, |popup, _| popup.is_stale()));
        assert_eq!(popup.update(cx, |popup, cx| popup.status_text(cx)), "Updating results…");
        assert_eq!(rows(&popup, cx), old, "the old rows stay");
        assert!(marks_of_selected(&popup, cx).is_empty(), "with no match marked");
        if replace {
            click_with(cx, "find-popup-replace", Modifiers::default());
            assert!(!popup.read_with(cx, |popup, _| popup.is_replacing()), "Replace waits for the new rows");
        }

        assert_eq!(searched(&popup, cx), ["fresh.txt:1 fresh"]);
        assert_eq!(popup.update(cx, |popup, cx| popup.status_text(cx)), "1 match in 1 file");
        assert_eq!(marks_of_selected(&popup, cx), [(0, 5)]);
        assert_eq!(popup.update(cx, |popup, cx| popup.replace_label(cx)), "Replace");
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
    }
    assert!(files_unchanged(&root), "nothing was replaced");
    assert_eq!(read(&root, "fresh.txt"), "fresh\n");
    close_backend_project(cx);
}

// src/features/search/__tests__/findPopup.test.tsx › outdated popup results › blocks %s during debounce and
// validation
/// While a changed query rests, Replace, Replace All, Enter, ⇧⌥↵ and ⌥A replace nothing and ask nothing.
#[gpui_kit::test]
async fn blocks_replace_replace_all_enter_and_the_all_keys_during_debounce(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, FILES);
    let popup = open_find(&workbench, true, "order", cx);
    popup.update(cx, |popup, cx| popup.set_replacement("item", cx));
    assert_eq!(searched(&popup, cx).len(), 3);

    popup.update(cx, |popup, cx| popup.set_pattern("ord", cx));
    assert!(popup.read_with(cx, |popup, _| popup.is_stale()));
    click_with(cx, "find-popup-replace", Modifiers::default());
    click_with(cx, "find-popup-replace-all", Modifiers::default());
    cx.simulate_keystrokes("enter");
    cx.simulate_keystrokes("shift-alt-enter");
    cx.simulate_keystrokes("alt-a");
    cx.run_until_parked();
    assert!(popup.read_with(cx, |popup, _| popup.is_stale()), "the query is still resting");
    assert!(!cx.has_pending_prompt(), "nothing is asked");
    assert!(!popup.read_with(cx, |popup, _| popup.is_replacing()), "nothing is replaced");
    assert!(workbench.read_with(cx, |w, _| w.find_popup().is_some()));

    assert_eq!(searched(&popup, cx).len(), 3);
    assert!(files_unchanged(&root));
    cx.simulate_keystrokes("escape");
    close_backend_project(cx);
}

// src/features/search/__tests__/findPopup.test.tsx › outdated popup results › invalidates when search options
// change: %j
/// Match Case, Words, Regex, a file mask, another directory and a search context each make the listed rows
/// outdated at once: their matches are no longer marked and Replace replaces nothing.
#[gpui_kit::test]
async fn invalidates_when_search_options_change(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, FILES);
    let popup = open_find(&workbench, true, "order", cx);
    popup.update(cx, |popup, cx| popup.set_replacement("item", cx));
    searched(&popup, cx);
    let outdated = |popup: &Entity<FindPopup>, what: &str, cx: &mut VisualTestContext| {
        assert!(popup.read_with(cx, |popup, _| popup.is_stale()), "{what} makes the rows outdated");
        assert!(marks_of_selected(popup, cx).is_empty(), "{what}: no match is marked");
        click_with(cx, "find-popup-replace", Modifiers::default());
        assert!(!popup.read_with(cx, |popup, _| popup.is_replacing()), "{what}: Replace waits");
    };
    for key in ["alt-c", "alt-w", "alt-x"] {
        cx.simulate_keystrokes(key);
        outdated(&popup, key, cx);
        cx.simulate_keystrokes(key);
        searched(&popup, cx);
    }
    popup.update(cx, |popup, cx| popup.set_mask("*.ts", cx));
    outdated(&popup, "a file mask", cx);
    popup.update(cx, |popup, cx| popup.set_mask_enabled(false, cx));
    searched(&popup, cx);
    cx.simulate_keystrokes("alt-d");
    popup.update(cx, |popup, cx| popup.set_directory("/other", cx));
    outdated(&popup, "another directory", cx);
    cx.simulate_keystrokes("alt-p");
    searched(&popup, cx);
    click_with(cx, "find-popup-context", Modifiers::default());
    cx.simulate_keystrokes("down enter");
    outdated(&popup, "a search context", cx);
    assert!(files_unchanged(&root));
    cx.simulate_keystrokes("escape");
    close_backend_project(cx);
}

// src/features/search/__tests__/findPopup.test.tsx › outdated popup results › clears retained rows and the count when
// the new search finds nothing
/// The old rows stay while a query that finds nothing is searched for; once it has ended they are gone, the
/// header shows no count, the results say "Nothing found" and Replace replaces nothing.
#[gpui_kit::test]
async fn clears_retained_rows_and_the_count_when_the_new_search_finds_nothing(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, FILES);
    let popup = open_find(&workbench, true, "order", cx);
    popup.update(cx, |popup, cx| popup.set_replacement("item", cx));
    searched(&popup, cx);
    popup.update(cx, |popup, cx| popup.set_pattern("absent", cx));
    assert_eq!(rows(&popup, cx).len(), 3);
    assert!(popup.read_with(cx, |popup, _| popup.is_stale()));

    assert!(searched(&popup, cx).is_empty());
    assert_eq!(popup.update(cx, |popup, cx| popup.status_text(cx)), "");
    assert_eq!(popup.update(cx, |popup, cx| popup.empty_text(cx)), ["Nothing found"]);
    click_with(cx, "find-popup-replace", Modifiers::default());
    assert!(!popup.read_with(cx, |popup, _| popup.is_replacing()));
    assert!(files_unchanged(&root));
    cx.simulate_keystrokes("escape");
    close_backend_project(cx);
}

// src/features/search/__tests__/findPopup.test.tsx › outdated popup results › keeps opening an outdated result
// available in replace=%s
/// While a changed query rests, the selected outdated row still opens: by Enter in Find, by the Open Result
/// action in Replace. The popup closes, the editor shows the match's file, and nothing is replaced.
#[gpui_kit::test]
async fn keeps_opening_an_outdated_result_available(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, FILES);
    for replace in [false, true] {
        let popup = open_find(&workbench, replace, "order", cx);
        let first = selected(&popup, cx).expect("a row is selected");
        let file = root.join(first.split_once(':').expect("a row names its file").0);
        popup.update(cx, |popup, cx| popup.set_pattern("absent", cx));
        assert!(popup.read_with(cx, |popup, _| popup.is_stale()));
        if replace {
            run_action(cx, "find.popup.openResult", None);
        } else {
            cx.simulate_keystrokes("enter");
        }
        cx.run_until_parked();
        assert!(workbench.read_with(cx, |w, _| w.find_popup().is_none()), "the popup closes (replace: {replace})");
        wait_long(cx, "the match's file to be shown", |cx| edited(&workbench, cx).as_deref() == Some(file.as_path()));
    }
    assert!(files_unchanged(&root));
    close_backend_project(cx);
}

// src/features/search/__tests__/findPopup.test.tsx › outdated popup results › rejects Replace All if the query
// changes while confirmation is open
/// Replace All in two files asks first; a query changed while it asks makes the answer void: nothing is replaced.
#[gpui_kit::test]
async fn rejects_replace_all_if_the_query_changes_while_confirmation_is_open(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, FILES);
    let popup = open_find(&workbench, true, "order", cx);
    popup.update(cx, |popup, cx| popup.set_replacement("item", cx));
    searched(&popup, cx);
    click_with(cx, "find-popup-replace-all", Modifiers::default());
    assert!(cx.has_pending_prompt(), "Replace All in two files asks first");
    popup.update(cx, |popup, cx| popup.set_pattern("ord", cx));
    searched(&popup, cx);
    cx.simulate_prompt_answer("Replace");
    cx.run_until_parked();
    assert!(!popup.read_with(cx, |popup, _| popup.is_replacing()));
    assert!(files_unchanged(&root), "the answer was given for another query");
    assert!(workbench.read_with(cx, |w, _| w.find_popup().is_some()));
    cx.simulate_keystrokes("escape");
    close_backend_project(cx);
}

// src/features/search/__tests__/findPopup.test.tsx › footer, as IDEA › keeps OpenIDEA's Replace preview: a diff in
// place of the code preview
/// Find in Files has no Preview check box; Replace in Files has one, and it puts the replacement's diff in
/// place of the code of the file.
#[gpui_kit::test]
async fn keeps_the_replace_preview_a_diff_in_place_of_the_code_preview(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _root, workbench, cx) = trusted_project(cx, FILES);
    open_find(&workbench, false, "order", cx);
    assert!(cx.debug_bounds("find-popup-preview-mode").is_none(), "Find has no Preview check box");
    let popup = open_find(&workbench, true, "order", cx);
    popup.update(cx, |popup, cx| popup.set_replacement("item", cx));
    searched(&popup, cx);
    wait_long(cx, "the code preview", |cx| popup.read_with(cx, |popup, cx| popup.preview_line(cx).is_some()));
    assert!(popup.read_with(cx, |popup, _| popup.diff_lines().is_empty()));
    click_with(cx, "find-popup-preview-mode", Modifiers::default());
    wait_long(cx, "the diff preview", |cx| popup.read_with(cx, |popup, _| !popup.diff_lines().is_empty()));
    let diff = popup.read_with(cx, |popup, _| popup.diff_lines());
    assert!(diff.iter().any(|line| line.starts_with('+') && line.contains("item")), "{diff:?}");
    cx.simulate_keystrokes("escape");
    close_backend_project(cx);
}

// src/features/search/__tests__/findPopup.test.tsx › mnemonics, as IDEA › ⌥P / ⌥M / ⌥D / ⌥S pick the scope tab, whose
// letter is underlined off macOS
/// ⌥D, ⌥S, ⌥M and ⌥P pick the Directory, Scope, Module and In Project tabs.
#[gpui_kit::test]
async fn alt_letters_pick_the_scope_tab(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _root, workbench, cx) = trusted_project(cx, FILES);
    let popup = open_find(&workbench, false, "order", cx);
    for (key, tab) in [
        ("alt-d", ScopeTab::Directory),
        ("alt-s", ScopeTab::Scope),
        ("alt-m", ScopeTab::Module),
        ("alt-p", ScopeTab::Project),
    ] {
        cx.simulate_keystrokes(key);
        assert_eq!(popup.read_with(cx, |popup, _| popup.options().scope_tab), tab, "{key}");
    }
    cx.simulate_keystrokes("escape");
    close_backend_project(cx);
}

// src/features/search/__tests__/findPopup.test.tsx › layout, as IDEA › shows the preview only once something matches
/// With nothing searched for the popup shows no preview; once a typed query matches, the preview shows the
/// selected match's file.
#[gpui_kit::test]
async fn shows_the_preview_only_once_something_matches(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, FILES);
    run_action(cx, "find.inPath", None);
    let popup = find_popup(&workbench, cx);
    assert!(rows(&popup, cx).is_empty());
    assert!(popup.read_with(cx, |popup, cx| popup.preview_path(cx).is_none() && popup.preview_lines(cx).is_empty()));
    cx.simulate_input("order");
    searched(&popup, cx);
    let first = selected(&popup, cx).expect("a row is selected");
    let file = text_of(&root.join(first.split_once(':').expect("a row names its file").0));
    wait_long(cx, "the preview", |cx| {
        popup.read_with(cx, |popup, cx| popup.preview_path(cx).as_deref() == Some(file.as_str()))
    });
    assert!(!popup.read_with(cx, |popup, cx| popup.preview_lines(cx)).is_empty());
    cx.simulate_keystrokes("escape");
    close_backend_project(cx);
}

// src/features/search/__tests__/findPopup.test.tsx › empty results, as IDEA › names the one option in use and clears
// it
/// With only a file mask in use and nothing to find, the popup names that option; its Clear option link puts the
/// mask out of use and the popup asks for a query again.
#[gpui_kit::test]
async fn names_the_one_option_in_use_and_clears_it(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _root, workbench, cx) = trusted_project(cx, FILES);
    run_action(cx, "find.inPath", None);
    let popup = find_popup(&workbench, cx);
    popup.update(cx, |popup, cx| popup.set_mask("*.rs", cx));
    cx.run_until_parked();
    let empty = |cx: &mut VisualTestContext| popup.update(cx, |popup, cx| popup.empty_text(cx));
    assert_eq!(empty(cx), ["No search text entered.", "Search option used: File mask"]);
    click_with(cx, "find-popup-empty-link", Modifiers::default());
    assert!(!popup.read_with(cx, |popup, _| popup.options().mask_enabled));
    assert_eq!(empty(cx), ["Type search query to find in files"]);
    cx.simulate_keystrokes("escape");
    close_backend_project(cx);
}
