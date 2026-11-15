use super::*;
use crate::github::time::timestamp_ms;
use crate::github::ui_model::full_date_time;
use crate::github::window_view::TabCommand;
use crate::platform::date_format::DateFormatOptions;
use crate::platform::local_time::utc_offset_minutes;
use crate::platform::settings;
use ide_workspace::settings::SettingsScope;

fn tab_titles(window: &Entity<PullRequestsWindow>, cx: &mut VisualTestContext) -> Vec<String> {
    window.update(cx, |window, cx| window.tab_titles(cx))
}

fn active_tab(window: &Entity<PullRequestsWindow>, cx: &mut VisualTestContext) -> GhTab {
    window.read_with(cx, |window, _| window.tabs().active)
}

fn copied_url(cx: &mut VisualTestContext) -> Option<String> {
    run_action(cx, "github.pullRequest.copyUrl", None);
    cx.read_from_clipboard().and_then(|item| item.text())
}

fn pull_requests_shown(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    workbench.read_with(cx, |w, _| w.is_tool_window_visible(PULL_REQUESTS_TOOL_WINDOW))
}

/// View Pull Request on a number already open selects its tab instead of adding one; closing the active tab
/// selects the tab before it, and the list tab has no close button.
#[gpui_kit::test]
async fn opens_each_pull_request_once_and_closes_back_to_the_previous_tab(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, _root) = github_project(cx, GITHUB_REMOTE, github_answers);
    let window = show_list(&workbench, 2, cx);
    for number in [5, 7, 5] {
        run_action(cx, "github.pullRequest.show", Some(json!(number)));
    }
    assert_eq!(tab_titles(&window, cx), ["app", "#5", "#7"]);
    assert_eq!(active_tab(&window, cx), GhTab::PullRequest(5));
    assert_eq!(context_text(cx, "githubPrTab").as_deref(), Some("pr"));
    assert!(cx.debug_bounds("gh-tab-close-0").is_none(), "the list tab does not close");

    run_action(cx, "github.pullRequest.show", Some(json!(7)));
    assert_eq!(active_tab(&window, cx), GhTab::PullRequest(7));
    click_on(cx, "gh-tab-close-2");
    assert_eq!(tab_titles(&window, cx), ["app", "#5"]);
    assert_eq!(active_tab(&window, cx), GhTab::PullRequest(5), "the tab before the closed one is selected");
    assert!(workbench.read_with(cx, |w, _| w.pull_request_details(7).is_none()), "#7's panel is dropped");

    click_on(cx, "gh-tab-close-1");
    assert_eq!(tab_titles(&window, cx), ["app"]);
    assert_eq!(active_tab(&window, cx), GhTab::List);
    assert_eq!(context_text(cx, "githubPrTab").as_deref(), Some("list"));
}

/// The pull request actions apply to the active `#N` tab's pull request, else to the one selected in the list:
/// Copy Pull Request URL without a number copies the selected row's page, then the open tab's, then again the
/// row's once that tab is closed.
#[gpui_kit::test]
async fn targets_the_active_pull_request_tab_else_the_list_selection(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, _root) = github_project(cx, GITHUB_REMOTE, github_answers);
    let window = show_list(&workbench, 2, cx);
    click_on(cx, "gh-pr-row-2");
    assert_eq!(window.read_with(cx, |window, _| window.context_pull_request()), Some(2));
    assert_eq!(copied_url(cx).as_deref(), Some("https://github.com/org/app/pull/2"));

    open_first(&workbench, cx);
    assert_eq!(active_tab(&window, cx), GhTab::PullRequest(1));
    assert_eq!(copied_url(cx).as_deref(), Some("https://github.com/org/app/pull/1"));

    click_on(cx, "gh-tab-close-1");
    assert_eq!(active_tab(&window, cx), GhTab::List);
    assert_eq!(window.read_with(cx, |window, _| window.context_pull_request()), Some(2));
    assert_eq!(copied_url(cx).as_deref(), Some("https://github.com/org/app/pull/2"));
}

/// Change Repository or Account… while not connected shows the Pull Requests window, the first time and again
/// after it was hidden, and the window says why it cannot connect.
#[gpui_kit::test]
async fn opens_the_tool_window_for_change_repository_or_account_while_not_connected(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root) = github_project(cx, GITHUB_REMOTE, github_answers);
    stand_in(GhCli::not_installed(), cx);
    assert!(!pull_requests_shown(&workbench, cx));
    run_action(cx, "github.changeRepository", None);
    assert!(pull_requests_shown(&workbench, cx));
    wait_long(cx, "gh is checked", |cx| matches!(content(&workbench, cx), Some(ToolWindowContent::Unavailable(_))));

    workbench.update_in(cx, |w, window, cx| w.with_layout(window, cx, |layout| layout.hide(PULL_REQUESTS_TOOL_WINDOW)));
    cx.run_until_parked();
    assert!(!pull_requests_shown(&workbench, cx));
    run_action(cx, "github.changeRepository", None);
    assert!(pull_requests_shown(&workbench, cx));
    wait_long(cx, "gh is checked again", |cx| match content(&workbench, cx) {
        Some(ToolWindowContent::Unavailable(shown)) => shown.title == "GitHub CLI is not installed",
        _ => false,
    });
    assert!(!context_on(cx, "githubConnected"));
    assert!(gh_calls(&root).is_empty(), "no gh ran");
}

/// With pretty formatting off in the Date Formats settings, a row of the list writes its creation as the plain
/// date and time in the user's pattern.
#[gpui_kit::test]
async fn shows_the_plain_date_and_time_with_pretty_formatting_off(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, _root) = github_project(cx, GITHUB_REMOTE, github_answers);
    let window = show_list(&workbench, 2, cx);
    let list = window.read_with(cx, |window, _| window.list().clone());
    let label = workbench.read_with(cx, |w, _| w.label().to_owned());
    let plain = json!({ "overrideSystem": true, "pattern": "dd.MM.yyyy", "use24Hour": true, "pretty": false });
    cx.update(|_, cx| settings::set(&label, SettingsScope::Global, "general.dateFormat", &plain, cx))
        .expect("the date format is set");
    cx.run_until_parked();

    let ms = timestamp_ms("2026-11-01T10:00:00Z").expect("the fixture's creation is a timestamp");
    let options =
        DateFormatOptions { override_system: true, pattern: "dd.MM.yyyy".into(), use_24_hour: true, pretty: false };
    let created = full_date_time(ms, utc_offset_minutes(ms as f64), &options);
    assert!(created.contains(".11.2026") || created.contains(".10.2026"), "{created}");
    let info = list.update(cx, |list, cx| list.rows(cx))[0].row.info.clone();
    assert!(info.starts_with(&format!("#1 · created {created}")), "{info}");
    assert!(!info.contains(" ago") && !info.contains("yesterday"), "{info}");
}

/// Create Pull Request… opens the New Pull Request tab once; closing it selects the tab before it. Tabs that do
/// not fit beside the header's buttons scroll, the active one into view; this one is wider than the narrow tool
/// window leaves room for, so it is closed from its menu.
#[gpui_kit::test]
async fn opens_the_new_pull_request_tab_once_and_closes_back_to_the_previous_tab(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, _root) = github_project(cx, GITHUB_REMOTE, github_answers);
    let window = show_list(&workbench, 2, cx);
    run_action(cx, "github.pullRequest.show", Some(json!(5)));
    run_action(cx, "github.pullRequest.show", Some(json!(7)));
    run_action(cx, "github.createPullRequest", None);
    click_on(cx, "gh-create-pull-request");
    assert_eq!(window.update(cx, |window, cx| window.tab_titles(cx)), ["app", "#5", "#7", "New Pull Request at app"]);
    assert_eq!(context_text(cx, "githubPrTab").as_deref(), Some("create"));
    assert!(cx.debug_bounds("gh-tab-loading").is_none(), "the tab shows the New Pull Request view");
    let tab = cx.debug_bounds("gh-tab-3").expect("the tab is drawn");
    let strip = cx.debug_bounds("gh-tabs").expect("the strip is drawn");
    let create = cx.debug_bounds("gh-create-pull-request").expect("the + button is drawn");
    assert_eq!(tab.left(), strip.left(), "the strip scrolls the active tab into view");
    assert!(create.left() >= strip.right(), "the header's buttons stay beside the strip: {create:?} {strip:?}");
    cx.simulate_mouse_down(tab.center(), gpui_kit::MouseButton::Right, gpui_kit::Modifiers::default());
    cx.simulate_mouse_up(tab.center(), gpui_kit::MouseButton::Right, gpui_kit::Modifiers::default());
    cx.run_until_parked();
    let menu = workbench.read_with(cx, |w, _| w.open_menu::<TabCommand>()).expect("the tab's menu is open");
    let rows = menu.read_with(cx, |menu, _| menu.lines());
    assert_eq!(rows[0].first().map(|row| row.trim_start_matches("> ")), Some("Close Tab"), "{rows:?}");
    click_on(cx, "menu-row-0-0");
    assert_eq!(tab_titles(&window, cx), ["app", "#5", "#7"]);
    assert_eq!(window.read_with(cx, |window, _| window.tabs().active), GhTab::PullRequest(7));
}
