//! The editor's gutter in a project window: the change bars against HEAD and their popup, the
//! bookmarks column, the run marks of the producers and of a `package.json`, the caret's row.

use super::*;
use crate::editor::view::Editor;
use crate::everywhere::bookmarks_store::Mark;
use crate::platform::keymap::is_mac_host;
use crate::run_producers::context_run::RunCommand;
use crate::run_producers::npm_gutter::ScriptCommand;
use crate::ui::popup_menu::PopupMenu;
use crate::vcs::hunk_popup::{NEXT, ROLLBACK};
use crate::vcs_diff::gutter::LaneShape;
use gpui_kit::{Bounds, Focusable as _, Modifiers, Pixels, point};
use ide_workspace::settings::SettingsScope;
use std::path::Path;

/// `b.txt` at HEAD.
const NUMBERS: &str = "1\n2\n3\n4\n5\n6\n";
/// `b.txt` in the working tree: line 2 modified, line 4 deleted, a line added at the end.
const CHANGED: &str = "1\nTWO\n3\n5\n6\nseven\n";

/// A repository whose `b.txt` differs from HEAD as [`CHANGED`] does.
fn changed_numbers(root: &Path) {
    init_repo(root);
    std::fs::write(root.join("b.txt"), NUMBERS).expect("the file is written");
    git_in(root, &["add", "."]);
    git_in(root, &["commit", "-q", "-m", "numbers"]);
    std::fs::write(root.join("b.txt"), CHANGED).expect("the file is written");
}

/// Opens `name` of the project in an editor, once it holds the file's text.
fn open(workbench: &Entity<Workbench>, root: &Path, name: &str, cx: &mut VisualTestContext) -> Entity<Editor> {
    let path = root.join(name);
    let opened = path.clone();
    workbench.update_in(cx, move |w, window, cx| w.open_project_file(opened, window, cx));
    wait_long(cx, "the file opens", |cx| {
        workbench.read_with(cx, |w, cx| w.editor().is_some_and(|editor| editor.read(cx).path() == path))
    });
    workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor")
}

/// Puts the caret at the start of `line` and gives the editor the keyboard.
fn caret_to_line(workbench: &Entity<Workbench>, editor: &Entity<Editor>, line: u32, cx: &mut VisualTestContext) {
    workbench.update_in(cx, |_, window, cx| {
        editor.update(cx, |editor, cx| {
            let at = editor.offset_of(line, 0);
            editor.set_selections(vec![(at, at)], 0, cx);
        });
        window.focus(&editor.focus_handle(cx), cx);
    });
    wait_long(cx, "the editor has the keyboard", |cx| context_is_on(cx, "editorTextFocus"));
}

/// The lane marks the last frame drew: line, colour token and shape, top to bottom.
fn drawn_lane(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> Vec<(u32, &'static str, LaneShape)> {
    let rows = editor.read_with(cx, |editor, _| editor.painted_gutter_rows());
    rows.into_iter().filter_map(|(line, _, lane)| lane.map(|(mark, _)| (line, mark.color, mark.shape))).collect()
}

/// Where the last frame drew the lane mark of `line`.
fn lane_bounds(editor: &Entity<Editor>, line: u32, cx: &mut VisualTestContext) -> Bounds<Pixels> {
    let rows = editor.read_with(cx, |editor, _| editor.painted_gutter_rows());
    let found = rows.into_iter().find(|(painted, _, _)| *painted == line);
    found.and_then(|(_, _, lane)| lane).map(|(_, bounds)| bounds).expect("the line's mark is drawn")
}

/// The text's left edge in the window.
fn text_left(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> Pixels {
    editor.read_with(cx, |editor, _| editor.position_of(editor.offset_of(0, 0)).expect("line 1 is painted").x)
}

fn selector(name: String) -> &'static str {
    Box::leak(name.into_boxed_str())
}

fn press(cx: &mut VisualTestContext, at: gpui_kit::Point<Pixels>, modifiers: Modifiers) {
    cx.simulate_click(at, modifiers);
    cx.run_until_parked();
}

fn primary() -> Modifiers {
    Modifiers { platform: is_mac_host(), control: !is_mac_host(), ..Modifiers::default() }
}

/// The rows of the first level of `menu`, without the selection mark.
fn menu_rows<C: Clone + 'static>(menu: &Entity<PopupMenu<C>>, cx: &mut VisualTestContext) -> Vec<String> {
    let lines = menu.read_with(cx, |menu, _| menu.lines());
    let first = lines.into_iter().next().unwrap_or_default();
    first.into_iter().map(|line| line.trim_start_matches("> ").to_owned()).collect()
}

/// The bookmarks of the window, as (line, mnemonic).
fn stored_bookmarks(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<(u32, Option<char>)> {
    workbench.read_with(cx, |w, _| w.bookmarks().data().items.iter().map(|b| (b.line, b.mnemonic)).collect())
}

/// Added and modified lines get a bar the height of the line, the line after deleted ones a notch,
/// each in its colour, in the lane between the icon columns and the fold chevrons, right of the
/// numbers.
#[gpui_kit::test]
async fn the_change_lane_draws_bars_on_added_and_modified_lines_and_a_notch_where_lines_were_deleted(
    cx: &mut TestAppContext,
) {
    let (workbench, cx, _folders, root) = git_project(cx, changed_numbers);
    let editor = open(&workbench, &root, "b.txt", cx);
    let expected = [
        (1, "gutter-modified", LaneShape::Bar),
        (3, "gutter-deleted", LaneShape::Deleted),
        (5, "gutter-added", LaneShape::Bar),
    ];
    wait_long(cx, "the bars of the file's changes", |cx| drawn_lane(&editor, cx) == expected);

    let left = text_left(&editor, cx);
    let (lane_left, lane_width) = editor.read_with(cx, |editor, cx| editor.change_lane(cx)).expect("a lane");
    let fold = editor.read_with(cx, |editor, cx| editor.fold_column_width(cx));
    let bar = lane_bounds(&editor, 1, cx);
    assert_eq!(bar.left(), left + lane_left, "the bar starts at the lane's left edge");
    assert!(bar.size.width < lane_width, "the bar is narrower than its lane: {bar:?}");
    assert_eq!(lane_left + lane_width, -fold, "the fold chevrons' column follows the lane");
    let number = editor.read_with(cx, |editor, _| editor.painted_line_numbers().into_iter().find(|(l, _)| *l == 1));
    let number = number.expect("line 2's number is drawn").1;
    assert!(number.right() <= bar.left(), "the number {number:?} is left of the bar {bar:?}");
}

/// While the text is edited the bars move with their lines at once; once the edit rests the backend
/// is asked again and its answer adds the new line's bar.
#[gpui_kit::test]
async fn the_bars_follow_an_edit_at_once_and_the_backends_next_answer_adds_the_typed_line(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root) = git_project(cx, changed_numbers);
    let editor = open(&workbench, &root, "b.txt", cx);
    wait_long(cx, "the bars", |cx| drawn_lane(&editor, cx).len() == 3);
    caret_to_line(&workbench, &editor, 0, cx);
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let moved: Vec<u32> = editor.read_with(cx, |editor, _| {
        editor.lane_marks().iter().map(|mark| crate::editor::buffer::line_col(editor.rope(), mark.from).0).collect()
    });
    assert_eq!(moved, [2, 4, 6], "each bar moved one line down with its line");
    wait_long(cx, "the typed line is an added one", |cx| {
        drawn_lane(&editor, cx).first() == Some(&(0, "gutter-added", LaneShape::Bar))
    });
}

/// A press on a change bar opens the change's popup with its original and current lines; Rollback
/// puts the original line back and the bar goes once the backend has answered again.
#[gpui_kit::test]
async fn a_press_on_a_bar_opens_its_changes_popup_and_rollback_puts_the_original_line_back(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root) = git_project(cx, changed_numbers);
    let editor = open(&workbench, &root, "b.txt", cx);
    wait_long(cx, "the bars", |cx| drawn_lane(&editor, cx).len() == 3);
    let unmarked = editor.read_with(cx, |editor, cx| {
        let at = editor.position_of(editor.offset_of(0, 0)).expect("line 1 is painted");
        let (lane, width) = editor.change_lane(cx).expect("a lane");
        point(at.x + lane + width / 2.0, at.y + editor.line_height(cx) / 2.0)
    });
    press(cx, unmarked, Modifiers::default());
    assert!(workbench.read_with(cx, |w, _| w.hunk_popup().is_none()), "a line without a change opens nothing");

    let bar = lane_bounds(&editor, 1, cx).center();
    press(cx, bar, Modifiers::default());
    let popup = workbench.read_with(cx, |w, _| w.hunk_popup()).expect("the change's popup");
    assert_eq!(popup.read_with(cx, |popup, _| popup.lines()), ["- 2", "+ TWO"]);
    let title = popup.update(cx, |popup, cx| popup.title(cx));
    assert_eq!(title, ("Modified lines".to_owned(), Some("1 of 3".to_owned())));

    let rollback = cx.debug_bounds(ROLLBACK).expect("Rollback is drawn");
    press(cx, rollback.center(), Modifiers::default());
    assert_eq!(editor.read_with(cx, |editor, _| editor.text()), "1\n2\n3\n5\n6\nseven\n");
    assert!(workbench.read_with(cx, |w, _| w.hunk_popup().is_none()), "the popup closes");
    wait_long(cx, "the rolled back line has no bar", |cx| {
        drawn_lane(&editor, cx) == [(3, "gutter-deleted", LaneShape::Deleted), (5, "gutter-added", LaneShape::Bar)]
    });
}

/// Next Change in the popup moves the caret to the next change and shows that change's popup.
#[gpui_kit::test]
async fn next_change_in_the_popup_moves_the_caret_and_shows_the_next_changes_popup(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root) = git_project(cx, changed_numbers);
    let editor = open(&workbench, &root, "b.txt", cx);
    wait_long(cx, "the bars", |cx| drawn_lane(&editor, cx).len() == 3);
    caret_to_line(&workbench, &editor, 1, cx);
    let bar = lane_bounds(&editor, 1, cx).center();
    press(cx, bar, Modifiers::default());
    assert!(workbench.read_with(cx, |w, _| w.hunk_popup().is_some()));
    let next = cx.debug_bounds(NEXT).expect("Next Change is drawn");
    press(cx, next.center(), Modifiers::default());
    let deleted_line = editor.read_with(cx, |editor, _| editor.offset_of(3, 0));
    wait_long(cx, "the next change's popup", |cx| {
        let popup = workbench.read_with(cx, |w, _| w.hunk_popup());
        popup.is_some_and(|popup| popup.update(cx, |popup, cx| popup.title(cx)).0 == "Deleted lines")
    });
    assert_eq!(editor.read_with(cx, |editor, _| editor.caret()), deleted_line);
    let popup = workbench.read_with(cx, |w, _| w.hunk_popup()).expect("the popup");
    assert_eq!(popup.read_with(cx, |popup, _| popup.lines()), ["- 4"]);
}

/// An untracked file is all added lines.
#[gpui_kit::test]
async fn an_untracked_file_is_all_added_lines(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root) = git_project(cx, |root| {
        init_repo(root);
        std::fs::write(root.join("new.txt"), "a\nb\n").expect("the file is written");
    });
    let editor = open(&workbench, &root, "new.txt", cx);
    let added = [(0, "gutter-added", LaneShape::Bar), (1, "gutter-added", LaneShape::Bar)];
    wait_long(cx, "the untracked file's bars", |cx| drawn_lane(&editor, cx) == added);
}

/// A file of a folder without a repository asks for its hunks and draws no bar.
#[gpui_kit::test]
async fn a_file_outside_any_repository_has_no_bars(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, plain, workbench, cx) =
        crate::workbench::plugins_window::test_support::trusted_project(cx, "w1", &[("a.txt", "one\n")]);
    let editor = open(&workbench, &plain, "a.txt", cx);
    wait_long(cx, "the gutter asked for its hunks", |cx| {
        workbench.read_with(cx, |w, _| !w.editor_gutter_hunks_pending(&editor))
    });
    assert!(editor.read_with(cx, |editor, _| editor.lane_marks().is_empty()));
}

/// Toggle Bookmark draws the bookmark icon on the caret's line in the bookmarks column, right of the
/// numbers; toggled again, the icon goes.
#[gpui_kit::test]
async fn toggle_bookmark_draws_the_bookmark_icon_on_the_carets_line_in_the_bookmarks_column(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root) = git_project(cx, changed_numbers);
    let editor = open(&workbench, &root, "b.txt", cx);
    caret_to_line(&workbench, &editor, 2, cx);
    run_action(cx, "navigate.toggleBookmark", None);
    let mark = selector("bookmark-gutter-2".to_owned());
    wait_long(cx, "the bookmark's icon", |cx| cx.debug_bounds(mark).is_some());
    let drawn = cx.debug_bounds(mark).expect("the bookmark is drawn");
    let (column, width) = editor.read_with(cx, |editor, cx| editor.gutter_column("bookmarks", cx)).expect("a column");
    let left = text_left(&editor, cx);
    assert_eq!((drawn.left(), drawn.size.width), (left + column, width), "the mark fills its column");
    let row_top = editor.read_with(cx, |editor, _| editor.position_of(editor.offset_of(2, 0)).expect("painted").y);
    assert_eq!(drawn.top(), row_top, "the mark is on the caret's line");
    let icon = cx.debug_bounds("icon:bookmark").expect("the bookmark icon");
    assert!(drawn.contains(&icon.center()), "the icon {icon:?} is in its cell {drawn:?}");
    let marks = workbench.read_with(cx, |w, _| w.editor_gutter_bookmarks(&editor));
    assert_eq!(marks, [Mark { line: 2, mnemonic: None }]);

    run_action(cx, "navigate.toggleBookmark", None);
    wait_long(cx, "the icon goes", |cx| cx.debug_bounds(mark).is_none());
    assert!(stored_bookmarks(&workbench, cx).is_empty());
}

/// Toggle Bookmark with Mnemonic draws the chosen digit on the caret's line instead of the icon.
#[gpui_kit::test]
async fn toggle_bookmark_with_mnemonic_draws_the_chosen_digit_on_the_carets_line(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root) = git_project(cx, changed_numbers);
    let editor = open(&workbench, &root, "b.txt", cx);
    caret_to_line(&workbench, &editor, 3, cx);
    run_action(cx, "navigate.toggleBookmarkWithMnemonic", None);
    cx.simulate_keystrokes("5");
    cx.run_until_parked();
    assert_eq!(stored_bookmarks(&workbench, cx), [(3, Some('5'))]);
    let chip = selector("bookmark-mnemonic-3".to_owned());
    wait_long(cx, "the mnemonic is drawn", |cx| cx.debug_bounds(chip).is_some());
    let cell = cx.debug_bounds(selector("bookmark-gutter-3".to_owned())).expect("the bookmark's cell");
    let drawn = cx.debug_bounds(chip).expect("the chip");
    assert!(cell.contains(&drawn.center()), "the chip {drawn:?} is in its cell {cell:?}");
    let marks = workbench.read_with(cx, |w, _| w.editor_gutter_bookmarks(&editor));
    assert_eq!(marks, [Mark { line: 3, mnemonic: Some('5') }]);
}

/// ⌘-click (Ctrl-click) on a line's number toggles that line's bookmark without moving the caret;
/// ⌘-click on the bookmark takes it away again.
#[gpui_kit::test]
async fn a_primary_click_in_the_gutter_toggles_the_lines_bookmark(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root) = git_project(cx, changed_numbers);
    let editor = open(&workbench, &root, "b.txt", cx);
    caret_to_line(&workbench, &editor, 0, cx);
    let number = editor.read_with(cx, |editor, _| editor.painted_line_numbers().into_iter().find(|(l, _)| *l == 4));
    let number = number.expect("line 5's number is drawn").1;
    press(cx, number.center(), primary());
    assert_eq!(stored_bookmarks(&workbench, cx), [(4, None)]);
    assert_eq!(editor.read_with(cx, |editor, _| editor.caret()), 0, "the caret stays");
    let mark = selector("bookmark-gutter-4".to_owned());
    wait_long(cx, "the bookmark's icon", |cx| cx.debug_bounds(mark).is_some());
    let drawn = cx.debug_bounds(mark).expect("the bookmark is drawn");
    press(cx, drawn.center(), Modifiers::default());
    assert_eq!(stored_bookmarks(&workbench, cx), [(4, None)], "a plain press on the icon keeps the bookmark");
    press(cx, drawn.center(), primary());
    assert!(stored_bookmarks(&workbench, cx).is_empty());
}

/// A line typed above a bookmark moves the bookmark down with its line, in the gutter and in the
/// stored bookmarks.
#[gpui_kit::test]
async fn a_bookmark_follows_its_line_when_a_line_is_inserted_above_it(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root) = git_project(cx, changed_numbers);
    let editor = open(&workbench, &root, "b.txt", cx);
    caret_to_line(&workbench, &editor, 2, cx);
    run_action(cx, "navigate.toggleBookmark", None);
    caret_to_line(&workbench, &editor, 0, cx);
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(stored_bookmarks(&workbench, cx), [(3, None)]);
    let marks = workbench.read_with(cx, |w, _| w.editor_gutter_bookmarks(&editor));
    assert_eq!(marks, [Mark { line: 3, mnemonic: None }]);
    let moved = selector("bookmark-gutter-3".to_owned());
    wait_long(cx, "the icon on the moved line", |cx| cx.debug_bounds(moved).is_some());
}

/// The caret's row is tinted across the whole gutter; under a selection the tint follows the line of
/// its moving end.
#[gpui_kit::test]
async fn the_carets_row_is_tinted_across_the_gutter(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root) = git_project(cx, changed_numbers);
    let editor = open(&workbench, &root, "b.txt", cx);
    caret_to_line(&workbench, &editor, 2, cx);
    let tinted = |cx: &mut VisualTestContext| -> Vec<u32> {
        let rows = editor.read_with(cx, |editor, _| editor.painted_gutter_rows());
        rows.into_iter().filter(|(_, caret, _)| *caret).map(|(line, _, _)| line).collect()
    };
    wait_long(cx, "the caret's row", |cx| tinted(cx) == [2]);
    let (from, to) = editor.read_with(cx, |editor, _| (editor.offset_of(1, 0), editor.offset_of(4, 1)));
    workbench.update_in(cx, |_, _, cx| editor.update(cx, |editor, cx| editor.set_selections(vec![(from, to)], 0, cx)));
    wait_long(cx, "the selection's moving end", |cx| tinted(cx) == [4]);
}

/// A Rust `main` outside a Cargo project gets a run mark in the run column, right of the bookmarks;
/// pressing it opens Run and Modify Run Configuration… for it, and nothing runs.
#[gpui_kit::test]
async fn a_main_gets_a_run_mark_whose_menu_offers_to_run_and_to_modify_its_configuration(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root) = git_project(cx, |root| {
        init_repo(root);
        std::fs::write(root.join("hello.rs"), "fn main() {\n    println!(\"hi\");\n}\n").expect("written");
    });
    let editor = open(&workbench, &root, "hello.rs", cx);
    wait_long(cx, "the run mark", |cx| {
        workbench.read_with(cx, |w, _| w.editor_gutter_run_marks(&editor)) == [(0, "gutter-run")]
    });
    let (bookmarks, run) = editor.read_with(cx, |editor, cx| {
        (editor.gutter_column("bookmarks", cx).expect("bookmarks"), editor.gutter_column("run", cx).expect("run"))
    });
    let lane = editor.read_with(cx, |editor, cx| editor.change_lane(cx)).expect("a lane");
    assert!(bookmarks.0 + bookmarks.1 <= run.0, "the bookmarks {bookmarks:?} are left of the run marks {run:?}");
    assert!(run.0 + run.1 <= lane.0, "the run marks {run:?} are left of the lane {lane:?}");
    let mark = selector("run-gutter-0".to_owned());
    wait_long(cx, "the mark is drawn", |cx| cx.debug_bounds(mark).is_some());
    let drawn = cx.debug_bounds(mark).expect("the mark");
    let left = text_left(&editor, cx);
    assert!(drawn.left() >= left + run.0 && drawn.right() <= left + run.0 + run.1, "{drawn:?} in {run:?}");
    press(cx, drawn.center(), Modifiers::default());
    let menu = workbench.read_with(cx, |w, _| w.open_menu::<RunCommand>()).expect("the mark's menu");
    let rows = menu_rows(&menu, cx);
    assert!(rows.first().is_some_and(|row| row.starts_with("Run '")), "{rows:?}");
    assert_eq!(rows.last().map(String::as_str), Some("Modify Run Configuration…"), "{rows:?}");
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert_eq!(editor.read_with(cx, |editor, _| editor.caret()), 0, "the press on the mark leaves the caret");
}

/// A `package.json` gets a run mark on each script's line, after Unicode text and CRLF line breaks;
/// pressing it opens Run and Debug with the package manager its lockfile names.
#[gpui_kit::test]
async fn a_package_json_script_gets_a_run_mark_whose_menu_runs_it_with_the_lockfiles_manager(cx: &mut TestAppContext) {
    let manifest = "{\r\n  \"description\": \"中文 🚀\",\r\n  \"scripts\": {\r\n    \"dev\": \"echo ok\"\r\n  }\r\n}";
    let (workbench, cx, _folders, root) = git_project(cx, |root| {
        init_repo(root);
        std::fs::write(root.join("package.json"), manifest).expect("written");
        std::fs::write(root.join("pnpm-lock.yaml"), "lockfileVersion: '9.0'\n").expect("written");
    });
    let editor = open(&workbench, &root, "package.json", cx);
    wait_long(cx, "the script's mark", |cx| {
        workbench.read_with(cx, |w, _| w.editor_gutter_run_marks(&editor)) == [(3, "gutter-run")]
    });
    let mark = selector("run-gutter-3".to_owned());
    wait_long(cx, "the mark is drawn", |cx| cx.debug_bounds(mark).is_some());
    let drawn = cx.debug_bounds(mark).expect("the mark");
    press(cx, drawn.center(), Modifiers::default());
    wait_long(cx, "the script's menu", |cx| workbench.read_with(cx, |w, _| w.open_menu::<ScriptCommand>().is_some()));
    let menu = workbench.read_with(cx, |w, _| w.open_menu::<ScriptCommand>()).expect("the menu");
    assert_eq!(menu_rows(&menu, cx), ["Run 'pnpm run dev'", "Debug 'pnpm run dev'"]);
    cx.simulate_keystrokes("escape");
}

/// With Show gutter icons off, the bookmarks and the run marks take no room.
#[gpui_kit::test]
async fn with_gutter_icons_off_the_bookmarks_and_run_columns_take_no_room(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root) = git_project(cx, |root| {
        init_repo(root);
        std::fs::write(root.join("hello.rs"), "fn main() {}\n").expect("written");
    });
    let editor = open(&workbench, &root, "hello.rs", cx);
    let columns = |cx: &mut VisualTestContext| {
        editor.read_with(cx, |editor, cx| (editor.gutter_column("bookmarks", cx), editor.gutter_column("run", cx)))
    };
    assert!(matches!(columns(cx), (Some(_), Some(_))));
    cx.update(|_, cx| {
        crate::platform::settings::set("w1", SettingsScope::Global, "editor.showGutterIcons", &false.into(), cx)
            .expect("the setting is stored");
    });
    wait_long(cx, "the columns go", |cx| columns(cx) == (None, None));
}
