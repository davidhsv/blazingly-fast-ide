//! Execute Maven Goal and Execute Gradle Task as the user drives them: opened through their actions, with keys
//! pressed in the drawn popup and clicks on its rows and on the names of the projects it can run in.

use super::proofs21_build_windows::removed;
use super::*;
use gpui_kit::{Modifiers, point};

/// `lines` as the tool's history of the project, newest first, as a run keeps it.
fn keep_history(root: &Path, tool: BuildTool, lines: &[&str], cx: &mut VisualTestContext) {
    let key = history_key(tool, &root.to_string_lossy());
    let stored = serde_json::to_string(lines).expect("the history is JSON");
    cx.update(|_, cx| cx.default_global::<UiStorage>().set(&key, stored));
}

/// Runs `action` and returns the popup it opens, with the keyboard in its command line.
fn open_execute_popup(workbench: &Entity<Workbench>, action: &str, cx: &mut VisualTestContext) -> Entity<ExecutePopup> {
    run_action(cx, action, None);
    let popup = workbench.read_with(cx, |w, _| w.execute_popup()).expect("the popup is open");
    let focused = cx.update(|window, cx| popup.read(cx).field().focus_handle(cx).is_focused(window));
    assert!(focused, "the keyboard is in the command line");
    popup
}

/// What the command line shows.
fn field_text(popup: &Entity<ExecutePopup>, cx: &mut VisualTestContext) -> String {
    popup.read_with(cx, |popup, cx| popup.field().read(cx).text().to_owned())
}

fn drawn_rows(popup: &Entity<ExecutePopup>, cx: &mut VisualTestContext) -> Vec<ExecuteRow> {
    popup.read_with(cx, |popup, _| popup.rows())
}

fn active(popup: &Entity<ExecutePopup>, cx: &mut VisualTestContext) -> Option<usize> {
    popup.read_with(cx, |popup, _| popup.state().active())
}

/// The headings of the completion list, in order.
fn sections(popup: &Entity<ExecutePopup>, cx: &mut VisualTestContext) -> Vec<&'static str> {
    drawn_rows(popup, cx)
        .into_iter()
        .filter_map(|row| match row {
            ExecuteRow::Section { label, .. } => Some(label),
            ExecuteRow::Completion(_) => None,
        })
        .collect()
}

/// The line of the completion at `index`.
fn completion_line(rows: &[ExecuteRow], index: usize) -> String {
    match rows.get(index) {
        Some(ExecuteRow::Completion(completion)) => completion.line.clone(),
        other => panic!("row {index} is a completion: {other:?}"),
    }
}

fn leak(text: String) -> &'static str {
    Box::leak(text.into_boxed_str())
}

/// Escape closes the popup.
fn dismiss(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(workbench.read_with(cx, |w, _| w.execute_popup().is_none()), "Escape closes the popup");
}

/// With one line run before, the popup lists it under Recent above the Maven Goals; Tab puts it into the command
/// line as it is.
#[gpui_kit::test]
async fn separates_recent_commands_from_maven_goals_and_accepts_a_recent_command_with_tab(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root, _tools) = build_project(cx, true, maven_project);
    keep_history(&root, BuildTool::Maven, &["package -o"], cx);
    let popup = open_execute_popup(&workbench, "maven.executeGoal", cx);
    assert_eq!(sections(&popup, cx), ["Recent", "Maven Goals"]);
    assert!(cx.debug_bounds("execute-row-1").is_some(), "the recent line is drawn");
    cx.simulate_keystrokes("tab");
    assert_eq!(field_text(&popup, cx), "package -o");
    dismiss(&workbench, cx);
    close(cx);
}

/// Down twice passes over the Maven Goals heading to the first goal, which Tab completes.
#[gpui_kit::test]
async fn skips_section_headers_during_keyboard_navigation(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root, _tools) = build_project(cx, true, maven_project);
    keep_history(&root, BuildTool::Maven, &["package -o"], cx);
    let popup = open_execute_popup(&workbench, "maven.executeGoal", cx);
    cx.simulate_keystrokes("down down");
    cx.simulate_keystrokes("tab");
    assert_eq!(field_text(&popup, cx), "clean ");
    dismiss(&workbench, cx);
    close(cx);
}

/// With no line run before, the list starts at the Maven Goals heading and has no Recent; Tab completes the first
/// goal.
#[gpui_kit::test]
async fn omits_the_empty_recent_section_when_no_commands_have_been_run(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, _root, _tools) = build_project(cx, true, maven_project);
    let popup = open_execute_popup(&workbench, "maven.executeGoal", cx);
    assert_eq!(drawn_rows(&popup, cx)[0], ExecuteRow::Section { key: "commands", label: "Maven Goals" });
    assert!(!sections(&popup, cx).contains(&"Recent"));
    cx.simulate_keystrokes("tab");
    assert_eq!(field_text(&popup, cx), "clean ");
    dismiss(&workbench, cx);
    close(cx);
}

/// Page Up from nothing selected lands on the first goal, not on its heading: Enter then completes it instead of
/// running the empty line.
#[gpui_kit::test]
async fn starts_page_up_on_a_completion_rather_than_a_section(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, _root, _tools) = build_project(cx, true, maven_project);
    let popup = open_execute_popup(&workbench, "maven.executeGoal", cx);
    cx.simulate_keystrokes("pageup");
    cx.simulate_keystrokes("enter");
    assert_eq!(field_text(&popup, cx), "clean ");
    assert!(workbench.read_with(cx, |w, _| w.execute_popup().is_some()), "Enter completed and ran nothing");
    dismiss(&workbench, cx);
    close(cx);
}

/// The arrows wrap between the last completion and the first, over both headings, and Page Down then Page Up come
/// back to the first. The Maven project's lifecycle plugins add their goals after the phases, so the last
/// completion is the last of those goals, as the list draws it.
#[gpui_kit::test]
async fn navigates_sections_and_wraps(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root, _tools) = build_project(cx, true, maven_project);
    keep_history(&root, BuildTool::Maven, &["package -o"], cx);
    let popup = open_execute_popup(&workbench, "maven.executeGoal", cx);
    let rows = drawn_rows(&popup, cx);
    let last = format!("{} ", completion_line(&rows, rows.len() - 1));
    dismiss(&workbench, cx);
    let cases: [(&str, &str); 5] = [
        ("up", last.as_str()),
        ("up down", "package -o"),
        ("down up", last.as_str()),
        ("down down up", "package -o"),
        ("pagedown pageup", "package -o"),
    ];
    for (keys, expected) in cases {
        let popup = open_execute_popup(&workbench, "maven.executeGoal", cx);
        cx.simulate_keystrokes(keys);
        cx.simulate_keystrokes("tab");
        assert_eq!(field_text(&popup, cx), expected, "{keys}");
        dismiss(&workbench, cx);
    }
    close(cx);
}

/// Typing changes the list and leaves nothing in it selected; a click on a heading accepts nothing, and Tab then
/// completes the first goal of the typed word.
#[gpui_kit::test]
async fn clears_selection_when_typing_changes_the_rows_and_ignores_section_clicks(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root, _tools) = build_project(cx, true, maven_project);
    keep_history(&root, BuildTool::Maven, &["package -o"], cx);
    let popup = open_execute_popup(&workbench, "maven.executeGoal", cx);
    cx.simulate_keystrokes("up");
    assert!(active(&popup, cx).is_some(), "Up selects the last completion");
    cx.simulate_input("cl");
    assert_eq!(active(&popup, cx), None, "typing clears the selection");
    // Up scrolled the long list to its end; the short one is drawn from its top.
    wait_long(cx, "the shorter list at its top", |cx| popup.read_with(cx, |popup, _| popup.list_scroll() == px(0.0)));
    let rows = drawn_rows(&popup, cx);
    let heading = rows.iter().position(|row| matches!(row, ExecuteRow::Section { label: "Maven Goals", .. }));
    let heading = heading.expect("the Maven Goals heading is listed");
    let below =
        cx.debug_bounds(leak(format!("execute-row-{}", heading + 1))).expect("the heading's first completion is drawn");
    // A heading is the 28 px row right above its first completion.
    cx.simulate_click(point(below.left() + px(40.0), below.top() - px(14.0)), Modifiers::default());
    cx.run_until_parked();
    assert!(workbench.read_with(cx, |w, _| w.execute_popup().is_some()), "a click on a heading keeps the popup");
    assert_eq!(field_text(&popup, cx), "cl", "a heading is not accepted");
    assert_eq!(active(&popup, cx), None);
    cx.simulate_keystrokes("tab");
    assert_eq!(field_text(&popup, cx), "clean ");
    dismiss(&workbench, cx);
    close(cx);
}

/// An option alone has no completions: no key changes the line or selects anything, and the list draws no heading.
/// With a Maven project to run it in, Enter then runs the line as it is: the popup closes and the line is kept as
/// typed.
#[gpui_kit::test]
async fn keeps_an_empty_completion_list_safe_for_all_navigation_and_acceptance_keys(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root, _tools) = build_project(cx, true, maven_project);
    let popup = open_execute_popup(&workbench, "maven.executeGoal", cx);
    cx.simulate_input("-Dunknown");
    for key in ["down", "up", "pagedown", "pageup", "tab"] {
        cx.simulate_keystrokes(key);
        assert_eq!(field_text(&popup, cx), "-Dunknown", "{key}");
        assert_eq!(active(&popup, cx), None, "{key}");
    }
    assert!(drawn_rows(&popup, cx).is_empty(), "no completion and no Maven Goals heading");
    assert!(cx.debug_bounds("execute-row-0").is_none());

    cx.simulate_keystrokes("enter");
    assert!(workbench.read_with(cx, |w, _| w.execute_popup().is_none()), "running closes the popup");
    let key = history_key(BuildTool::Maven, &root.to_string_lossy());
    let stored = cx.update(|_, cx| cx.global::<UiStorage>().get(&key).map(str::to_owned));
    assert_eq!(parse_history(stored.as_deref()).first().map(String::as_str), Some("-Dunknown"));
    let store = workbench
        .read_with(cx, |w, cx| w.run_driver().map(|driver| driver.read(cx).store().clone()))
        .expect("the project runs things");
    wait_long(cx, "the line's run to end", |cx| {
        store.read_with(cx, |store, _| {
            store.sessions.iter().any(|s| s.config.args.iter().any(|arg| arg == "-Dunknown") && !s.is_live())
        })
    });
    close(cx);
}

/// With nine lines run before, Down selects the newest and Page Down lands on the first goal past the Maven Goals
/// heading, which Enter completes.
#[gpui_kit::test]
async fn page_down_selects_a_completion_rather_than_a_section_at_its_landing_position(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root, _tools) = build_project(cx, true, maven_project);
    let history: Vec<String> = (0..9).rev().map(|i| format!("command-{i}")).collect();
    let lines: Vec<&str> = history.iter().map(String::as_str).collect();
    keep_history(&root, BuildTool::Maven, &lines, cx);
    let popup = open_execute_popup(&workbench, "maven.executeGoal", cx);
    cx.simulate_keystrokes("down");
    cx.simulate_keystrokes("pagedown");
    cx.simulate_keystrokes("enter");
    assert_eq!(field_text(&popup, cx), "clean ");
    assert!(workbench.read_with(cx, |w, _| w.execute_popup().is_some()), "Enter completed and ran nothing");
    dismiss(&workbench, cx);
    close(cx);
}

/// Execute Gradle Task over two linked builds: choosing the other build by its name in the footer drops the
/// selection and its own tasks replace the list, whose first Tab completes; once the builds are gone, Tab leaves
/// the line alone.
#[gpui_kit::test]
async fn resets_selection_when_changing_the_gradle_build_used_for_completion(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root, _tools) = build_project(cx, true, |root, tools| {
        gradle_project(root, tools);
        write(&root.join("libs/other/settings.gradle"), "rootProject.name = 'other'\n");
        // A Java build: its tasks are not the first build's, whose root applies no plugin.
        write(&root.join("libs/other/build.gradle"), "plugins { id 'java' }\n");
        executable(&root.join("libs/other/gradlew"), FAKE_GRADLEW);
    });
    run_action(cx, "gradle.linkProjects", None);
    wait_long(cx, "the build file is asked for", |cx| cx.did_prompt_for_paths());
    let picked = root.join("libs/other/settings.gradle");
    cx.simulate_path_prompt_response(move |_| Some(vec![picked.clone()]));
    let view = workbench.read_with(cx, |w, _| w.build_tool_view(BuildTool::Gradle)).expect("the Gradle window");
    // Linking opens the new build in the tree; that is done before the popup is opened over it.
    wait_long(cx, "the linked build in the tree", |cx| labels(&view, cx).contains(&"other".to_owned()));

    let popup = open_execute_popup(&workbench, "gradle.executeTask", cx);
    let projects = popup.read_with(cx, |popup, _| popup.projects());
    let value_of = |label: &str| {
        let found = projects.iter().find(|choice| choice.label == label).map(|choice| choice.value.clone());
        found.unwrap_or_else(|| panic!("{label} is offered: {projects:?}"))
    };
    let (first, second) = (value_of("demo"), value_of("other"));
    if popup.read_with(cx, |popup, _| popup.state().project() != Some(first.as_str())) {
        click_on(cx, leak(format!("execute-project-{first}")));
    }
    let rows = drawn_rows(&popup, cx);
    assert_eq!(rows[0], ExecuteRow::Section { key: "commands", label: "Gradle Tasks" });
    let first_line = completion_line(&rows, 1);
    cx.simulate_keystrokes("down");
    assert_eq!(active(&popup, cx), Some(1));

    click_on(cx, leak(format!("execute-project-{second}")));
    assert_eq!(popup.read_with(cx, |popup, _| popup.state().project().map(str::to_owned)), Some(second.clone()));
    assert_eq!(active(&popup, cx), None, "no completion stays selected");
    let second_line = completion_line(&drawn_rows(&popup, cx), 1);
    assert_ne!(second_line, first_line, "the other build's tasks are listed");
    cx.simulate_keystrokes("tab");
    assert_eq!(field_text(&popup, cx), format!("{second_line} "));

    // Both builds go: the popup has nothing left to complete with.
    let scripts = [
        root.join("settings.gradle"),
        root.join("build.gradle"),
        root.join("app/build.gradle"),
        root.join("libs/other/settings.gradle"),
        root.join("libs/other/build.gradle"),
    ];
    for script in &scripts {
        std::fs::remove_file(script).expect("the script is removed");
    }
    removed(scripts.to_vec(), cx);
    wait_long(cx, "the builds to go", |cx| {
        popup.read_with(cx, |popup, _| popup.projects().is_empty() && popup.rows().is_empty())
    });
    cx.simulate_keystrokes("tab");
    assert_eq!(field_text(&popup, cx), format!("{second_line} "));
    dismiss(&workbench, cx);
    close(cx);
}
