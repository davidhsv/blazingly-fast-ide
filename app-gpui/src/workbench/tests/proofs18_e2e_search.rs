use super::proofs18_e2e_fixtures::double_shift;
use super::*;

const SERVICE: &str = "src/main/kotlin/com/acme/orders/service/OrderService.kt";

type Opened<'a> = (tempfile::TempDir, PathBuf, Entity<Workbench>, &'a mut VisualTestContext);

/// A project with the Kotlin service file, open in an active window once its index lists the file.
fn orders_project(cx: &mut TestAppContext) -> Opened<'_> {
    let (folder, root, workbench, cx) =
        trusted_project(cx, &[(SERVICE, "class OrderService\n"), ("build.gradle.kts", "plugins {}\n")]);
    cx.update(|window, _| window.activate_window());
    settle(cx, |cx| {
        let host = cx.update(|_, cx| cx.global::<Backend>().host.clone());
        ide_host::ops::nav::find_files(&host, "w1", "OrderSer".to_owned(), 5).is_ok_and(|found| !found.is_empty())
    });
    (folder, root, workbench, cx)
}

/// Double Shift opens Search Everywhere on All with the keyboard in its query; Tab goes through Classes,
/// Files, Symbols, Actions and Text, and back to All. Escape closes it.
#[gpui_kit::test]
async fn double_shift_opens_search_everywhere_with_its_tabs_escape_closes_it(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _root, workbench, cx) = orders_project(cx);
    double_shift(cx);
    let everywhere = workbench.read_with(cx, |w, _| w.search_everywhere()).expect("double Shift opens it");
    assert!(cx.update(|window, cx| everywhere.focus_handle(cx).is_focused(window)), "the query has the keyboard");
    assert_eq!(everywhere.read_with(cx, |popup, _| popup.tab()), SeTab::All);
    let mut shown = Vec::new();
    for _ in 0..6 {
        cx.simulate_keystrokes("tab");
        cx.run_until_parked();
        shown.push(everywhere.read_with(cx, |popup, _| popup.tab()));
    }
    assert_eq!(shown, [SeTab::Classes, SeTab::Files, SeTab::Symbols, SeTab::Actions, SeTab::Text, SeTab::All]);
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(workbench.read_with(cx, |w, _| w.search_everywhere().is_none()), "Escape closes it");
    close_backend_project(cx);
}

/// "Language Servers" typed after double Shift lists it; Enter opens Settings on the Language Servers page.
#[gpui_kit::test]
async fn search_everywhere_finds_and_runs_an_action(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _root, workbench, cx) = orders_project(cx);
    double_shift(cx);
    let everywhere = workbench.read_with(cx, |w, _| w.search_everywhere()).expect("double Shift opens it");
    type_query(cx, "Language Servers");
    settle(cx, |cx| {
        everywhere.read_with(cx, |popup, _| popup.selected().is_some_and(|line| line.contains("Language Servers")))
    });
    cx.simulate_keystrokes("enter");
    settle(cx, |cx| workbench.read_with(cx, |w, _| w.settings_dialog().is_some()));
    let dialog = workbench.read_with(cx, |w, _| w.settings_dialog().cloned()).expect("Settings opens");
    assert_eq!(dialog.read_with(cx, |view, cx| view.selected_page(cx)).as_deref(), Some("languages.servers"));
    close_backend_project(cx);
}

/// "OrderService" typed after double Shift lists the service file; Enter opens it in the editor.
#[gpui_kit::test]
async fn search_everywhere_finds_a_file_and_opens_it(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = orders_project(cx);
    double_shift(cx);
    let everywhere = workbench.read_with(cx, |w, _| w.search_everywhere()).expect("double Shift opens it");
    type_query(cx, "OrderService");
    settle(cx, |cx| everywhere.read_with(cx, |popup, _| popup.lines().iter().any(|line| line == SERVICE)));
    cx.simulate_keystrokes("enter");
    let file = root.join(SERVICE);
    settle(cx, |cx| workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).path() == file)));
    let titles = workbench.read_with(cx, |w, _| w.tab_titles());
    assert!(titles.iter().any(|tab| tab.active && tab.name == "OrderService.kt"), "{titles:?}");
    close_backend_project(cx);
}
