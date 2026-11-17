use super::*;
use crate::platform::i18n::Localization;
use gpui_kit::{Bounds, Pixels};

/// The height of a row of the results.
const ROW: f32 = 24.0;

/// A project whose files are `count` copies named `{stem}{i}.js`, for every pair given, opened in an active
/// window once the index lists them.
fn layout_project<'a>(
    cx: &'a mut TestAppContext,
    sets: &[(&str, usize)],
) -> (tempfile::TempDir, Entity<Workbench>, &'a mut VisualTestContext) {
    let mut files: Vec<(String, String)> = Vec::new();
    for (stem, count) in sets {
        files.extend((0..*count).map(|i| (format!("src/{stem}{i}.js"), format!("// {stem} {i}\n"))));
    }
    files.push(("src/main/kotlin/com/acme/navigation/FindAction.kt".to_owned(), "class FindAction\n".to_owned()));
    let listed: Vec<(&str, &str)> = files.iter().map(|(path, text)| (path.as_str(), text.as_str())).collect();
    let (folder, _root, workbench, cx) = trusted_project(cx, &listed);
    cx.update(|window, _| window.activate_window());
    let stem = sets.first().map_or("FindAction", |(stem, _)| *stem).to_owned();
    settle(cx, |cx| {
        cx.update(|_, cx| {
            let backend = cx.global::<Backend>();
            backend.block_on(ide_host::ops::nav::find_files(&backend.host, "w1", stem.clone(), 5))
        })
        .is_ok_and(|found| !found.is_empty())
    });
    (folder, workbench, cx)
}

/// Where `selector` was drawn last.
fn bounds(cx: &mut VisualTestContext, selector: &'static str) -> Bounds<Pixels> {
    cx.run_until_parked();
    cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"))
}

/// A selector built at run time, as `debug_bounds` takes it.
fn leaked(selector: String) -> &'static str {
    Box::leak(selector.into_boxed_str())
}

/// Search Everywhere on Files with `query` typed, once its rows are in.
fn files_tab(workbench: &Entity<Workbench>, query: &str, cx: &mut VisualTestContext) -> Entity<SearchEverywhere> {
    run_action(cx, "navigate.gotoFile", None);
    let everywhere = popup(workbench, cx);
    everywhere.update(cx, |popup, cx| popup.field().update(cx, |field, cx| field.select_all(cx)));
    type_query(cx, query);
    let stem = query.to_owned();
    settle(cx, |cx| {
        everywhere
            .read_with(cx, |popup, _| !popup.is_loading() && popup.lines().iter().any(|line| line.contains(&stem)))
    });
    everywhere
}

/// Without a preview the results take the height of their rows: one, two or five rows of 24 px at least
/// for one, two and 24 results.
#[gpui_kit::test]
async fn without_the_preview_the_results_list_shows_its_rows(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, workbench, cx) = layout_project(cx, &[("main", 24), ("pair", 2), ("solo", 1)]);
    for (stem, count) in [("solo", 1_usize), ("pair", 2), ("main", 24)] {
        let everywhere = files_tab(&workbench, stem, cx);
        let shown =
            everywhere.read_with(cx, |popup, _| popup.lines().iter().filter(|line| line.contains(stem)).count());
        assert_eq!(shown.min(5), count.min(5), "{stem}: the rows listed");
        let list = bounds(cx, "search-everywhere-results");
        let wanted = count.min(5) as f32 * ROW;
        assert!(f32::from(list.size.height) >= wanted, "{stem}: {:?} holds {wanted} px of rows", list.size);
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
    }
    close_backend_project(cx);
}

/// In windows 1000, 720 and 520 px wide, in English and Chinese, the popup and its header fit the window,
/// the query field stays wider than 160 px, every tab Tab selects is shown whole, and the footer is in view.
#[gpui_kit::test]
async fn tabs_and_query_fit_narrow_windows(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, workbench, cx) = layout_project(cx, &[("OrderService", 24)]);
    for locale in ["en", "zh"] {
        cx.update(|_, cx| cx.default_global::<Localization>().0.set_locale(Some(locale), "en"));
        for (width, height) in [(1000.0, 760.0), (720.0, 608.0), (520.0, 608.0)] {
            cx.simulate_resize(gpui_kit::size(gpui_kit::px(width), gpui_kit::px(height)));
            let everywhere = files_tab(&workbench, "OrderService", cx);
            let dialog = bounds(cx, "search-everywhere");
            assert!(
                f32::from(dialog.left()) >= 0.0 && f32::from(dialog.right()) <= width,
                "{locale} {width}: {dialog:?}"
            );
            let header = bounds(cx, "search-everywhere-header");
            assert!(f32::from(header.right()) <= f32::from(dialog.right()) + 1.0, "{locale} {width}: {header:?}");
            let field = bounds(cx, "search-everywhere-query");
            assert!(f32::from(field.size.width) > 160.0, "{locale} {width}: the query is {:?} wide", field.size);
            for _ in 0..6 {
                cx.simulate_keystrokes("tab");
                cx.run_until_parked();
                assert!(
                    cx.update(|window, cx| everywhere.focus_handle(cx).is_focused(window)),
                    "the query keeps the keys"
                );
                let tab = everywhere.read_with(cx, |popup, _| popup.tab());
                let shown = bounds(cx, leaked(format!("search-everywhere-tab-{}", tab.id())));
                assert!(
                    shown.left() >= header.left() && shown.right() <= header.right() + gpui_kit::px(1.0),
                    "{locale} {width}: the {} tab {shown:?} is in the header {header:?}",
                    tab.id()
                );
            }
            settle(cx, |cx| cx.debug_bounds("search-everywhere-footer").is_some());
            let footer = bounds(cx, "search-everywhere-footer");
            assert!(f32::from(footer.bottom()) <= height, "{locale} {width}: the footer {footer:?} is in view");
            cx.simulate_keystrokes("escape");
            cx.run_until_parked();
        }
    }
    close_backend_project(cx);
}

/// A file row and an action row start their names at the same x, and the action's shortcut is drawn after
/// its name, inside its row.
#[gpui_kit::test]
async fn mixed_file_and_action_rows_align_icons_and_names_and_keep_shortcuts_readable(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, workbench, cx) = layout_project(cx, &[]);
    run_action(cx, "navigate.searchEverywhere", None);
    let everywhere = popup(&workbench, cx);
    type_query(cx, "Find Action");
    let rows = |cx: &mut VisualTestContext| everywhere.read_with(cx, |popup, _| popup.lines());
    settle(cx, |cx| {
        let lines = rows(cx);
        lines.iter().any(|line| line.ends_with("FindAction.kt")) && lines.contains(&"Find Action".to_owned())
    });
    let lines = rows(cx);
    let file = lines.iter().position(|line| line.ends_with("FindAction.kt")).expect("the file is listed");
    let action = lines.iter().position(|line| line == "Find Action").expect("the action is listed");
    let file_name = bounds(cx, leaked(format!("search-everywhere-name-{file}")));
    let action_name = bounds(cx, leaked(format!("search-everywhere-name-{action}")));
    assert!((f32::from(file_name.left()) - f32::from(action_name.left())).abs() < 1.0, "{file_name:?} {action_name:?}");
    let keys = bounds(cx, leaked(format!("search-everywhere-shortcut-{action}")));
    assert!(keys.left() >= action_name.right(), "the shortcut {keys:?} follows the name {action_name:?}");
    let row = bounds(cx, leaked(format!("search-everywhere-row-{action}")));
    assert!(keys.right() <= row.right(), "the shortcut {keys:?} is inside its row {row:?}");
    cx.simulate_keystrokes("escape");
    close_backend_project(cx);
}
