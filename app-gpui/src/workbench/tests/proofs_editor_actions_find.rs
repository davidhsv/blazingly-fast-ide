use super::*;
use crate::find::popup_view::{FindMenu, SEARCH_DEBOUNCE};
use crate::find::session::FindStatus;
use crate::find::store::HistoryKind;

/// Waits for the popup's search to finish and returns its rows.
fn searched(popup: &Entity<FindPopup>, cx: &mut VisualTestContext) -> Vec<String> {
    for _ in 0..300 {
        cx.executor().advance_clock(SEARCH_DEBOUNCE);
        cx.run_until_parked();
        if popup.read_with(cx, |popup, _| popup.status() == FindStatus::Done && !popup.is_stale()) {
            return popup.read_with(cx, |popup, _| popup.lines());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("the search did not finish within three seconds");
}

fn selected(popup: &Entity<FindPopup>, cx: &mut VisualTestContext) -> Option<String> {
    popup.read_with(cx, |popup, _| popup.selected())
}

/// The popup's own keys run as actions while it is open: Next and Previous Result move the
/// selected row round the ends of the list, Recent Searches lists the strings searched before,
/// and Open Result opens the selected match with the caret on it and closes the popup.
#[gpui_kit::test]
async fn the_find_in_files_popups_commands_move_the_selection_list_recent_searches_and_open_the_match(
    cx: &mut TestAppContext,
) {
    let _home = start(cx);
    cx.update(|cx| {
        cx.global_mut::<UiStorage>().set(HistoryKind::Find.storage_key(), r#"["old query"]"#.to_owned());
    });
    let (_folder, root, workbench, cx) = search_project(cx);
    run_action(cx, "find.inPath", None);
    let popup = workbench.read_with(cx, |w, _| w.find_popup()).expect("Find in Files is open");
    // "pub struct Widget;\n\npub fn make_widget() -> Widget {\n    Widget\n}\n": the first match is
    // on the first line, the last one on the fourth.
    cx.simulate_input("widget");
    let lines = searched(&popup, cx);
    assert!(lines.len() >= 3 && lines.iter().all(|line| line.contains("src/alpha.rs")), "{lines:?}");
    let (first, second, last) = (lines[0].clone(), lines[1].clone(), lines[lines.len() - 1].clone());
    assert_eq!(selected(&popup, cx), Some(first.clone()));

    run_action(cx, "find.popup.previousResult", None);
    assert_eq!(selected(&popup, cx), Some(last.clone()), "Previous Result goes round to the last row");
    run_action(cx, "find.popup.nextResult", None);
    assert_eq!(selected(&popup, cx), Some(first.clone()), "Next Result goes round to the first row");
    run_action(cx, "find.popup.nextResult", None);
    assert_eq!(selected(&popup, cx), Some(second));

    run_action(cx, "find.popup.recentSearches", None);
    assert_eq!(
        popup.read_with(cx, |popup, _| (popup.menu(), popup.menu_lines())),
        (Some(FindMenu::History(HistoryKind::Find)), vec!["Recent Searches".to_owned(), "old query".to_owned()])
    );
    cx.simulate_keystrokes("escape");
    assert_eq!(popup.read_with(cx, |popup, _| popup.menu()), None);
    assert_eq!(workbench.read_with(cx, |w, _| w.find_popup()), Some(popup.clone()), "Escape closed the list only");

    run_action(cx, "find.popup.previousResult", None);
    run_action(cx, "find.popup.previousResult", None);
    assert_eq!(selected(&popup, cx), Some(last));
    run_action(cx, "find.popup.openResult", None);
    let alpha = root.join("src/alpha.rs");
    settle(cx, |cx| {
        workbench.read_with(cx, |w, cx| {
            w.editor().is_some_and(|e| e.read(cx).path() == alpha && e.read(cx).caret_position() == (3, 4))
        })
    });
    assert!(workbench.read_with(cx, |w, _| w.find_popup().is_none()), "opening a match closes the popup");
    cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
}

/// Scroll to Results During Typing is a toggle of the open find bar: turned off, typing a query
/// counts the occurrences but leaves the selection where it was, and Find Next still goes to the
/// next one; turned on again, typing selects the occurrence nearest the selection.
#[gpui_kit::test]
async fn scroll_to_results_during_typing_turns_the_selection_while_typing_off_and_on(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = search_project(cx);
    let alpha = root.join("src/alpha.rs");
    workbench.update_in(cx, |w, window, cx| w.open_project_file(alpha.clone(), window, cx));
    settle(cx, |cx| {
        cx.update(|window, cx| {
            let editor = workbench.read(cx).editor();
            editor.is_some_and(|e| e.read(cx).path() == alpha && e.focus_handle(cx).is_focused(window))
        })
    });
    let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");
    let toggled = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| {
            let action = cx.global::<ActionRegistry>().get("find.scrollToResults").expect("registered");
            action.is_toggled(window, cx)
        })
    };
    let selection = |cx: &mut VisualTestContext| editor.read_with(cx, |e, _| e.selection());

    run_action(cx, "find.inFile", None);
    let bar = workbench.read_with(cx, |w, _| w.find_bar()).expect("the bar is open");
    assert_eq!(toggled(cx), Some(true));
    run_action(cx, "find.scrollToResults", None);
    assert_eq!(toggled(cx), Some(false));

    // "pub struct Widget;\n\npub fn make_widget() -> Widget {\n    Widget\n}\n"
    cx.simulate_input("widget");
    cx.run_until_parked();
    assert_eq!(bar.read_with(cx, |bar, _| bar.match_count()), 4);
    assert_eq!(selection(cx), 0..0, "typing does not move the selection");
    cx.simulate_keystrokes("enter");
    assert_eq!(selection(cx), 11..17, "Find Next still goes to the next occurrence");

    run_action(cx, "find.scrollToResults", None);
    assert_eq!(toggled(cx), Some(true));
    bar.update(cx, |bar, cx| bar.set_search("make", cx));
    cx.run_until_parked();
    assert_eq!(selection(cx), 27..31, "typing selects the occurrence nearest the selection");
    cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
}
