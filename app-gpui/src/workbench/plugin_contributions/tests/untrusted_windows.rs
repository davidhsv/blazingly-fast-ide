//! An installed plugin in a window whose project is in Restricted Mode: every call that would run
//! the plugin's code there is refused, as the desktop's command guard refuses it, and what the
//! window shows says so; once the project is trusted, the same entry point runs the plugin.

use super::*;
use std::sync::LazyLock;

/// What the guard answers for a command that needs a trusted project.
const RESTRICTED: &str = "Project is in Restricted Mode. Trust the project to enable execution.";

static DATE: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"\d{4}-\d{2}-\d{2}").expect("the date pattern is valid"));

/// A project of `files` (path, text) in Restricted Mode, open in an active window.
fn restricted_project<'a>(
    cx: &'a mut TestAppContext,
    files: &[(&str, &str)],
) -> (tempfile::TempDir, PathBuf, Entity<Workbench>, &'a mut VisualTestContext) {
    let folder = tempfile::tempdir().expect("a temporary folder");
    let root = folder.path().canonicalize().expect("the folder exists");
    for (path, text) in files {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().expect("a folder")).expect("the folder is made");
        std::fs::write(path, text).expect("the file is written");
    }
    cx.update(|cx| cx.global::<Backend>().host.trust.decide(&root, false, false)).expect("previewed");
    let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new(LABEL, window, cx));
    cx.update(|window, _| window.activate_window());
    workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
    wait_long(cx, "the project opens", |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
    assert!(workbench.read_with(cx, |w, cx| w.project_restricted(cx)), "the project is in Restricted Mode");
    (folder, root, workbench, cx)
}

/// Trust Project, as the banner's button runs it; waits for the project to leave Restricted Mode.
fn trust(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    run_action(cx, "project.trust", None);
    wait_long(cx, "the project is trusted", |cx| !workbench.read_with(cx, |w, cx| w.project_restricted(cx)));
}

/// The level and body of the notification titled `title`, once it shows.
fn notice(
    workbench: &Entity<Workbench>,
    title: &str,
    cx: &mut VisualTestContext,
) -> (super::super::super::notifications::NotificationLevel, Option<String>) {
    wait_long(cx, &format!("the notice {title}"), |cx| notice_titled(workbench, title, cx).is_some());
    let shown = notice_titled(workbench, title, cx).expect("the notice shows");
    (shown.level, shown.body)
}

/// Lets a second of plugin events and answers arrive: what was refused stays refused.
fn let_a_second_pass(cx: &mut VisualTestContext) {
    let until = std::time::Instant::now() + std::time::Duration::from_secs(1);
    wait_long(cx, "a second", |_| std::time::Instant::now() >= until);
}

fn editor_text(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |editor, _| editor.text())
}

/// A plugin command run as an action (the palette, its key) in Restricted Mode is refused: the
/// error notification names the command and carries the guard's reason, the editor keeps its text
/// and the plugin's code is not loaded. Trusted, the same action inserts the date.
#[gpui_kit::test]
async fn a_plugin_command_run_as_an_action_is_refused_in_restricted_mode_and_runs_once_trusted(
    cx: &mut TestAppContext,
) {
    use super::super::super::notifications::NotificationLevel;
    let home = start_with_plugins(cx);
    let folder = plugin_copy(home.path(), "hello", str::to_owned);
    link(cx, &folder, &[], "insert-date");
    let (_folder, root, workbench, cx) = restricted_project(cx, &[("notes.txt", "one\n")]);
    let editor = open_file(&workbench, root.join("notes.txt"), cx);
    run_action(cx, "plugin.hello.insert-date", None);
    let refused = notice(&workbench, "Hello: Insert Date failed", cx);
    assert_eq!(refused, (NotificationLevel::Error, Some(RESTRICTED.to_owned())));
    assert_eq!(editor_text(&editor, cx), "one\n");
    assert_eq!(status_of("hello", cx), Some(PluginStatus::Inactive), "no plugin code was loaded");

    trust(&workbench, cx);
    run_action(cx, "plugin.hello.insert-date", None);
    wait_long(cx, "the plugin inserts the date", |cx| DATE.is_match(&editor_text(&editor, cx)));
    assert_eq!(status_of("hello", cx), Some(PluginStatus::Active));
    cx.update(|_, cx| cx.global::<Backend>().host.close_project(LABEL));
}

/// The plugin's Tools menu entry stays listed and enabled in Restricted Mode; chosen there, it is
/// refused with the guard's reason. Trusted, the entry inserts the date.
#[gpui_kit::test]
async fn a_plugins_menu_entry_is_refused_in_restricted_mode_and_runs_once_trusted(cx: &mut TestAppContext) {
    use super::super::super::menu::{self, MenuCommand};
    use super::super::super::menu_spec::DYNAMIC_PLUGIN_TOOLS;
    use super::super::super::notifications::NotificationLevel;
    let home = start_with_plugins(cx);
    let folder = plugin_copy(home.path(), "hello", hello_menus);
    link(cx, &folder, &[], "insert-date");
    let (_folder, root, workbench, cx) = restricted_project(cx, &[("notes.txt", "one\n")]);
    let editor = open_file(&workbench, root.join("notes.txt"), cx);
    const INSERT_DATE: &str = "plugin.hello.insert-date";
    let state = cx.update(|window, cx| menu::state(window, cx));
    let listed =
        state.dynamic.get(DYNAMIC_PLUGIN_TOOLS).is_some_and(|items| items.iter().any(|i| i.action == INSERT_DATE));
    assert!(listed && !state.disabled.contains(INSERT_DATE), "the entry is listed and enabled");
    cx.dispatch_action(MenuCommand { action: INSERT_DATE.to_owned(), arg: None });
    let refused = notice(&workbench, "Hello: Insert Date failed", cx);
    assert_eq!(refused, (NotificationLevel::Error, Some(RESTRICTED.to_owned())));
    assert_eq!(editor_text(&editor, cx), "one\n");

    trust(&workbench, cx);
    cx.dispatch_action(MenuCommand { action: INSERT_DATE.to_owned(), arg: None });
    wait_long(cx, "the plugin inserts the date", |cx| DATE.is_match(&editor_text(&editor, cx)));
    cx.update(|_, cx| cx.global::<Backend>().host.close_project(LABEL));
}

/// A click on the plugin's declared status item in Restricted Mode is refused with the guard's
/// reason. Trusted, the click runs Word Count, whose notification shows.
#[gpui_kit::test]
async fn a_plugins_status_item_click_is_refused_in_restricted_mode_and_runs_once_trusted(cx: &mut TestAppContext) {
    use super::super::super::notifications::NotificationLevel;
    let home = start_with_plugins(cx);
    let folder = plugin_copy(home.path(), "hello", str::to_owned);
    link(cx, &folder, &[], "word-count");
    let (_folder, root, workbench, cx) = restricted_project(cx, &[("notes.txt", "one two three\n")]);
    open_file(&workbench, root.join("notes.txt"), cx);
    wait_long(cx, "the declared item is drawn", |cx| cx.debug_bounds("status:plugin.hello/hello").is_some());
    click(cx, "status:plugin.hello/hello");
    let refused = notice(&workbench, "Hello: Word Count of Active File failed", cx);
    assert_eq!(refused, (NotificationLevel::Error, Some(RESTRICTED.to_owned())));
    assert_eq!(status_of("hello", cx), Some(PluginStatus::Inactive), "no plugin code was loaded");

    trust(&workbench, cx);
    click(cx, "status:plugin.hello/hello");
    wait_long(cx, "the plugin's notification", |cx| notice_titled(&workbench, "Word count: notes.txt", cx).is_some());
    cx.update(|_, cx| cx.global::<Backend>().host.close_project(LABEL));
}

/// The plugin's view, opened in Restricted Mode, shows that the plugin could not show it, with the
/// guard's reason, and loads no plugin code. Opened in a trusted project's window, it shows the rows
/// the plugin renders.
#[gpui_kit::test]
async fn a_plugins_view_renders_nothing_in_restricted_mode_and_its_rows_in_a_trusted_project(cx: &mut TestAppContext) {
    let home = start_with_plugins(cx);
    let folder = plugin_copy(home.path(), "hello", str::to_owned);
    link(cx, &folder, &[], "word-count");
    let (_folder, _root, workbench, cx) = restricted_project(cx, &[("a.txt", "a\n")]);
    let panel = |cx: &mut VisualTestContext| workbench.read_with(cx, |w, _| w.plugin_view_panel("hello", "greetings"));
    run_action(cx, "toolwindow.plugin.hello.greetings", None);
    wait_long(cx, "the view fails", |cx| panel(cx).is_some_and(|p| p.read_with(cx, |p, _| p.error().is_some())));
    let failed = panel(cx).expect("the view's panel");
    assert_eq!(failed.read_with(cx, |p, _| p.error().map(str::to_owned)).as_deref(), Some(RESTRICTED));
    assert!(failed.read_with(cx, |p, _| p.content().is_none()));
    assert!(cx.debug_bounds("plugin-view-failed").is_some(), "the failure is drawn in place of the view");
    assert_eq!(status_of("hello", cx), Some(PluginStatus::Inactive), "no plugin code was loaded");
    cx.update(|_, cx| cx.global::<Backend>().host.close_project(LABEL));

    let (_trusted, _root, trusted, cx) = trusted_project(cx, "w2", &[("a.txt", "a\n")]);
    run_action(cx, "toolwindow.plugin.hello.greetings", None);
    wait_long(cx, "the view's rows", |cx| {
        let panel = trusted.read_with(cx, |w, _| w.plugin_view_panel("hello", "greetings"));
        panel.is_some_and(|p| p.read_with(cx, |p, _| p.lines()) == ["▸ English", "▸ Deutsch", "▸ 日本語"])
    });
    cx.update(|_, cx| cx.global::<Backend>().host.close_project("w2"));
}

/// A view rendered while the project was trusted keeps its rows once trust is revoked; an event
/// sent from it then (opening a node) is refused, and the reason shows above the view, which
/// stays as it was. Trusted again, the event reaches the plugin and the node opens.
#[gpui_kit::test]
async fn a_plugin_views_event_is_refused_in_restricted_mode_and_reaches_the_plugin_once_trusted(
    cx: &mut TestAppContext,
) {
    const LANGUAGES: [&str; 3] = ["▸ English", "▸ Deutsch", "▸ 日本語"];
    let home = start_with_plugins(cx);
    let folder = plugin_copy(home.path(), "hello", str::to_owned);
    link(cx, &folder, &[], "word-count");
    let (_folder, _root, workbench, cx) = trusted_project(cx, LABEL, &[("a.txt", "a\n")]);
    let panel = |cx: &mut VisualTestContext| workbench.read_with(cx, |w, _| w.plugin_view_panel("hello", "greetings"));
    let lines = |cx: &mut VisualTestContext| panel(cx).map(|p| p.read_with(cx, |p, _| p.lines())).unwrap_or_default();
    run_action(cx, "toolwindow.plugin.hello.greetings", None);
    wait_long(cx, "the view's rows", |cx| lines(cx) == LANGUAGES);
    run_action(cx, "project.revokeTrust", None);
    wait_long(cx, "Restricted Mode", |cx| workbench.read_with(cx, |w, cx| w.project_restricted(cx)));
    let_a_second_pass(cx);
    assert_eq!(lines(cx), LANGUAGES, "the rows stay");
    click_on(cx, "plugin-view-toggle:0");
    wait_long(cx, "the refusal shows", |cx| panel(cx).is_some_and(|p| p.read_with(cx, |p, _| p.error().is_some())));
    let error = panel(cx).and_then(|p| p.read_with(cx, |p, _| p.error().map(str::to_owned)));
    assert_eq!(error.as_deref(), Some(RESTRICTED));
    assert_eq!(lines(cx), LANGUAGES, "the node did not open");

    trust(&workbench, cx);
    click_on(cx, "plugin-view-toggle:0");
    wait_long(cx, "the node opens", |cx| lines(cx).first().map(String::as_str) == Some("▾ English"));
    cx.update(|_, cx| cx.global::<Backend>().host.close_project(LABEL));
}

/// A settings page's action in Restricted Mode is refused: the reason shows under its button, no
/// plugin code is loaded and the plugin says nothing. Trusted, the action runs the plugin's
/// command, which reads the saved row and says so.
#[gpui_kit::test]
async fn a_plugin_settings_pages_action_is_refused_in_restricted_mode_and_runs_once_trusted(cx: &mut TestAppContext) {
    let home = start_with_plugins(cx);
    let folder = plugin_copy(home.path(), "settings-pages", str::to_owned);
    link(cx, &folder, &[], "test");
    let (_folder, _root, workbench, cx) = restricted_project(cx, &[("a.txt", "a\n")]);
    run_action(cx, "file.settings", Some(Value::from("plugins.settings-pages")));
    wait_long(cx, "the plugin's page lists its pages", |cx| {
        cx.debug_bounds("plugin-settings-link:plugins.settings-pages.settings:llm").is_some()
    });
    click(cx, "plugin-settings-link:plugins.settings-pages.settings:llm");
    wait_long(cx, "the LLM page shows", |cx| cx.debug_bounds("plugin-setting-add:profiles").is_some());
    click(cx, "plugin-setting-add:profiles");
    click(cx, "plugin-settings-action:0");
    wait_long(cx, "the error shows under the button", |cx| cx.debug_bounds("plugin-settings-action-error:0").is_some());
    let_a_second_pass(cx);
    assert!(notice_titled(&workbench, "Model Profiles", cx).is_none(), "the plugin said nothing");
    assert_eq!(status_of("settings-pages", cx), Some(PluginStatus::Inactive), "no plugin code was loaded");

    trust(&workbench, cx);
    click(cx, "plugin-settings-action:0");
    wait_long(cx, "the plugin read the row", |cx| {
        notice_titled(&workbench, "Model Profiles", cx)
            .is_some_and(|notice| notice.body.as_deref() == Some("Read 1 saved profile(s)"))
    });
    cx.update(|_, cx| cx.global::<Backend>().host.close_project(LABEL));
}

/// The plugin's gutter actions are not asked for in Restricted Mode: no icon shows on the line.
/// Trusted, the next edit asks for them and the icon shows.
#[gpui_kit::test]
async fn a_plugins_gutter_actions_are_not_asked_for_in_restricted_mode_and_show_once_trusted(cx: &mut TestAppContext) {
    let home = start_with_plugins(cx);
    let folder = plugin_copy(home.path(), "language-features", str::to_owned);
    link(cx, &folder, &[], "run");
    let (_folder, root, workbench, cx) = restricted_project(cx, &[("notes.txt", "one\ntwo\n")]);
    let editor = open_file(&workbench, root.join("notes.txt"), cx);
    let markers = |cx: &mut VisualTestContext| workbench.read_with(cx, |w, _| w.plugin_gutter_markers(&editor));
    let_a_second_pass(cx);
    assert!(markers(cx).is_empty(), "no gutter action in Restricted Mode");
    assert!(cx.debug_bounds("plugin-gutter:0").is_none());

    trust(&workbench, cx);
    editor.update(cx, |editor, cx| editor.insert(Some(0..0), "zero\n", cx));
    wait_long(cx, "the gutter action shows", |cx| !markers(cx).is_empty());
    cx.update(|_, cx| cx.global::<Backend>().host.close_project(LABEL));
}

/// A saved file in Restricted Mode is not forwarded to the plugin that watches its saves. Trusted,
/// the next save reaches it.
#[gpui_kit::test]
async fn a_saved_file_is_not_forwarded_to_plugins_in_restricted_mode_and_reaches_them_once_trusted(
    cx: &mut TestAppContext,
) {
    let home = start_with_plugins(cx);
    let folder = plugin_copy(home.path(), "path-events", str::to_owned);
    link(cx, &folder, &["events", "project.read"], "count");
    let (_folder, root, workbench, cx) = restricted_project(cx, &[("config/app.yaml", "a: 1\n")]);
    // The probe runs on the plugin host directly, as the test's own call: it is not the window's.
    let count = |cx: &mut VisualTestContext| run_in_window(cx, &root, "test.path-events", "count", None).ok().flatten();
    let editor = open_file(&workbench, root.join("config/app.yaml"), cx);
    let save = |text: &str, cx: &mut VisualTestContext| {
        editor.update(cx, |editor, cx| editor.insert(Some(0..0), text, cx));
        let saving = editor.read_with(cx, |editor, cx| editor.save(cx));
        cx.executor().spawn(saving).detach();
        wait_long(cx, "the file is saved", |cx| !editor.read_with(cx, |editor, _| editor.is_dirty()));
    };
    save("b: 2\n", cx);
    let_a_second_pass(cx);
    assert_eq!(count(cx), None, "the plugin heard nothing");

    trust(&workbench, cx);
    let_a_second_pass(cx);
    save("c: 3\n", cx);
    wait_long(cx, "the plugin heard the save", |cx| count(cx).as_deref() == Some("1"));
    cx.update(|_, cx| cx.global::<Backend>().host.close_project(LABEL));
}

/// Starts `command` of `plugin` on the plugin host for the window, off the main thread, as a command
/// that runs for it would; the handle answers once the command ends.
#[cfg(unix)]
fn start_for_window(
    cx: &mut VisualTestContext,
    root: &Path,
    plugin: &str,
    command: &str,
    arg: Option<&str>,
) -> std::thread::JoinHandle<ide_core::IdeResult<Option<String>>> {
    let host = cx.update(|_, cx| cx.global::<Plugins>().host.clone());
    let invocation = Invocation {
        command_chain: Vec::new(),
        client: Some(LABEL.to_owned()),
        workspace_root: Some(root.to_path_buf()),
        editor: None,
        selected_paths: Vec::new(),
        global_settings: false,
    };
    let (plugin, command, arg) = (plugin.to_owned(), command.to_owned(), arg.map(str::to_owned));
    std::thread::spawn(move || host.run_command(&plugin, &command, arg.as_deref(), invocation))
}

/// A plugin's question shown in a window in Restricted Mode closes when answered, but the answer
/// does not reach the plugin: its command still waits, until the plugin is turned off. Trusted, the
/// answer typed and confirmed is what the command gets back.
#[cfg(unix)]
#[gpui_kit::test]
async fn an_answer_to_a_plugins_question_does_not_reach_it_in_restricted_mode_and_does_once_trusted(
    cx: &mut TestAppContext,
) {
    let home = start_with_plugins(cx);
    let bin = home.path().join("bin");
    program(&bin, "jobs", "#!/bin/sh\nexit 0\n");
    programs_in(&bin, cx);
    let folder = plugin_copy(home.path(), "jobs", str::to_owned);
    link(cx, &folder, &[], "follow");
    let (_folder, root, workbench, cx) = restricted_project(cx, &[("a.txt", "a\n")]);
    let asked = |cx: &mut VisualTestContext| workbench.read_with(cx, |w, _| w.plugin_prompt().is_some());
    let running = start_for_window(cx, &root, "jobs", "input", Some("Main"));
    wait_long(cx, "the question shows", |cx| asked(cx));
    cx.simulate_keystrokes("enter");
    wait_long(cx, "the question closes", |cx| !asked(cx));
    let_a_second_pass(cx);
    assert!(!running.is_finished(), "the command still waits for its answer");
    cx.update(|_, cx| cx.global::<Plugins>().host.set_enabled("jobs", false)).expect("the plugin turns off");
    wait_long(cx, "the command ends", |_| running.is_finished());
    assert!(running.join().expect("the command ran").is_err(), "the command got no answer");

    cx.update(|_, cx| cx.global::<Plugins>().host.set_enabled("jobs", true)).expect("the plugin turns on");
    trust(&workbench, cx);
    let running = start_for_window(cx, &root, "jobs", "input", Some("Main"));
    wait_long(cx, "the question shows again", |cx| asked(cx));
    cx.simulate_keystrokes("enter");
    wait_long(cx, "the command ends", |_| running.is_finished());
    assert_eq!(running.join().expect("the command ran").expect("it answers").as_deref(), Some("Main"));
    cx.update(|_, cx| cx.global::<Backend>().host.close_project(LABEL));
}
