//! System Settings (Date Formats, HTTP Proxy, Updates), Editor › File Types, Build Tools (CMake,
//! Gradle, Maven), Tools (Actions on Save, External Tools) and Version Control (Directory Mappings,
//! Issue Navigation), each opened through File › Settings… on its page: every control the page has
//! is read with its value, one is changed with the mouse or the keyboard, and Apply stores it (in
//! the temporary project's settings file for a page edited in the project). Reset and Cancel
//! discard an edit on a page of each kind: standard controls, controls of their own, a page drawn
//! by a view of its own.
//!
//! Nothing here reaches the network or runs a program: the installations a Build Tools page offers
//! come from a detection the test gives, Check connection… uses a checker the test gives, and
//! Check Now runs an action the test registers.

use super::*;
use crate::backend::Backend;
use crate::platform::actions::ActionRegistry;
use crate::platform::settings::{effective, global};
use crate::settings_view::controls::UNAVAILABLE;
use crate::settings_view::dialog::SettingsDialogView;
use crate::settings_view::pages::Then;
use crate::settings_view::pages::build_tool_page::ToolPage;
use crate::settings_view::pages::languages_sdk::{SdkDetection, SdkFieldView, Toolchain, ToolchainKind};
use crate::settings_view::pages::list_editor::ListEditor;
use crate::settings_view::pages::{
    FormView, build_cmake, build_gradle, build_maven, editor_files, general_date_formats, general_http_proxy,
    general_updates, tools_actions_on_save, tools_external_tools, vcs_issue_navigation, vcs_mappings,
};
use gpui_kit::{AnyView, App, Focusable};
use serde_json::{Value, json};
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;

fn window(cx: &mut TestAppContext) -> (Entity<Workbench>, &mut VisualTestContext) {
    let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
    cx.update(|window, _| window.activate_window());
    (workbench, cx)
}

/// A trusted project with one file, open in a window.
fn project(cx: &mut TestAppContext) -> (tempfile::TempDir, PathBuf, Entity<Workbench>, &mut VisualTestContext) {
    let folder = tempfile::tempdir().expect("a temporary folder");
    let root = folder.path().canonicalize().expect("the folder exists");
    std::fs::write(root.join("a.txt"), "a\n").expect("the file is written");
    cx.update(|cx| cx.global::<Backend>().host.trust.decide(&root, true, false)).expect("the project is trusted");
    let (workbench, cx) = window(cx);
    workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
    (folder, root, workbench, cx)
}

/// The project's own settings file; empty while there is none.
fn project_file(root: &Path) -> String {
    std::fs::read_to_string(root.join(".openidea/settings.toml")).unwrap_or_default()
}

/// File › Settings… on `page`: the dialog shows it and draws every control it has.
fn open_page(workbench: &Entity<Workbench>, page: &str, cx: &mut VisualTestContext) -> Entity<SettingsDialogView> {
    run_action(cx, "file.settings", Some(Value::from(page)));
    let dialog = settings_dialog_of(workbench, cx).expect("Settings opens");
    assert_eq!(dialog.read_with(cx, |view, cx| view.selected_page(cx)).as_deref(), Some(page));
    let lines = dialog.read_with(cx, |view, cx| view.control_lines(cx));
    assert!(lines.iter().all(|line| !line.ends_with(UNAVAILABLE)), "{page} draws every control: {lines:?}");
    dialog
}

/// The view of the control of its own `slot` on `page`.
fn custom<V: 'static>(
    dialog: &Entity<SettingsDialogView>,
    page: &str,
    slot: &str,
    cx: &mut VisualTestContext,
) -> Entity<V> {
    let view: Option<AnyView> = dialog.read_with(cx, |view, _| view.custom_control(page, slot).cloned());
    view.and_then(|view| view.downcast().ok()).unwrap_or_else(|| panic!("{page} has its {slot} control"))
}

/// The view drawn for the whole of `page`.
fn page_view<V: 'static>(dialog: &Entity<SettingsDialogView>, page: &str, cx: &mut VisualTestContext) -> Entity<V> {
    let view: Option<AnyView> = dialog.read_with(cx, |view, _| view.page_view(page).cloned());
    view.and_then(|view| view.downcast().ok()).unwrap_or_else(|| panic!("{page} has its view"))
}

/// The lines of the parts a Build Tools page draws now.
fn tool_page_lines(page: &Entity<ToolPage>, cx: &mut VisualTestContext) -> Vec<String> {
    let parts = page.read_with(cx, |page, cx| page.shown_parts(cx));
    let mut lines = Vec::new();
    for part in parts {
        let part = match part.downcast::<SdkFieldView>() {
            Ok(field) => {
                lines.extend(field.read_with(cx, |field, cx| field.lines(cx)));
                continue;
            }
            Err(part) => part,
        };
        if let Ok(form) = part.downcast::<FormView>() {
            lines.extend(form.read_with(cx, |form, cx| form.lines(cx)));
        }
    }
    lines
}

/// The text field of setting `key` in a form of a Build Tools page.
fn tool_page_field(
    page: &Entity<ToolPage>,
    key: &str,
    cx: &mut VisualTestContext,
) -> Entity<crate::ui::text_field::TextField> {
    let parts = page.read_with(cx, |page, cx| page.shown_parts(cx));
    let mut forms = parts.into_iter().filter_map(|part| part.downcast::<FormView>().ok());
    let found = forms.find_map(|form| form.read_with(cx, |form, _| form.field(key).cloned()));
    found.unwrap_or_else(|| panic!("the page has a field for {key}"))
}

fn effective_value(cx: &mut VisualTestContext, key: &str) -> Option<Value> {
    cx.update(|_, cx| setting_of(&effective("w1", cx), key))
}

fn global_value(cx: &mut VisualTestContext, key: &str) -> Option<Value> {
    cx.update(|_, cx| setting_of(&global(cx), key))
}

/// Clicks the field drawn as `at` and types `text` over its text.
fn type_into(cx: &mut VisualTestContext, at: &'static str, text: &str) {
    click_on(cx, at);
    cx.simulate_keystrokes("cmd-a");
    cx.simulate_input(text);
    cx.run_until_parked();
}

/// Gives `field` the keyboard and types `text` over its text.
fn type_over(cx: &mut VisualTestContext, field: &Entity<crate::ui::text_field::TextField>, text: &str) {
    cx.update(|window, cx| {
        field.update(cx, |field, cx| field.select_all(cx));
        window.focus(&field.focus_handle(cx), cx);
    });
    cx.simulate_input(text);
    cx.run_until_parked();
}

/// Scrolls the page shown until the control drawn as `at` is above the dialog's buttons; the
/// wheel turns over the page just above OK, clear of the lists that scroll on their own.
pub(super) fn bring_into_view(cx: &mut VisualTestContext, at: &'static str) {
    use gpui_kit::{Modifiers, ScrollDelta, ScrollEvent, ScrollSource, TouchPhase, point, px};
    for _ in 0..40 {
        cx.run_until_parked();
        let target = cx.debug_bounds(at).unwrap_or_else(|| panic!("{at} is drawn"));
        let ok = cx.debug_bounds("settings-ok").expect("the dialog's OK is drawn");
        if target.bottom() < ok.top() - px(8.0) {
            return;
        }
        cx.simulate_event(ScrollEvent {
            position: point(ok.center().x, ok.top() - px(40.0)),
            delta: ScrollDelta::Pixels(point(px(0.0), px(-60.0))),
            modifiers: Modifiers::default(),
            touch_phase: TouchPhase::Moved,
            source: ScrollSource::Pointer,
        });
    }
    panic!("{at} cannot be scrolled into view");
}

fn is_modified(dialog: &Entity<SettingsDialogView>, cx: &mut VisualTestContext) -> bool {
    dialog.read_with(cx, |view, cx| view.is_modified(cx))
}

/// Detection that finds one JDK and one Maven home inside `folder`, and logs the kinds it is asked for.
fn fake_installations(folder: &Path, cx: &mut VisualTestContext) -> Rc<RefCell<Vec<ToolchainKind>>> {
    let installation = |path: PathBuf, name: &str| Toolchain {
        path: path.display().to_string(),
        name: name.to_owned(),
        ..Toolchain::default()
    };
    let test = SdkDetection {
        found: vec![
            (ToolchainKind::Jdk, vec![installation(folder.join("jdks/temurin-21"), "Temurin 21")]),
            (ToolchainKind::Maven, vec![installation(folder.join("maven-3.9"), "Maven 3.9")]),
        ],
        ..SdkDetection::default()
    };
    let asked = test.asked.clone();
    cx.update(|_, cx| cx.set_global(test));
    asked
}

// Date Formats: the override, the pattern, the clock, the pretty forms and the preview; the
// override ticked and a pattern typed apply to the global settings.
#[gpui_kit::test]
async fn date_formats_override_and_pattern_apply(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (workbench, cx) = window(cx);
    let dialog = open_page(&workbench, general_date_formats::PAGE, cx);
    let view: Entity<general_date_formats::DateFormatsView> =
        custom(&dialog, general_date_formats::PAGE, general_date_formats::SLOT, cx);
    let lines = view.read_with(cx, |view, cx| view.lines(cx));
    assert_eq!(
        lines[..6],
        [
            "[ ] Override system date and time format",
            "  Date format: dd.MM.yyyy (disabled)",
            "    Placeholders: yyyy, yy, MMMM, MMM, MM, M, dd, d",
            "  [x] Use 24-hour time format (disabled)",
            "[x] Use pretty formatting",
            "    Show \"Today\", \"Yesterday\" and \"N min ago\" for recent dates",
        ]
        .map(str::to_owned)
    );
    assert!(lines[6].starts_with("Preview: ") && lines[6].contains(" · "), "{lines:?}");
    click_on(cx, "settings-toggle-general.dateFormat.overrideSystem");
    type_into(cx, "settings-text-general.dateFormat.pattern", "yyyy-MM-dd");
    click_on(cx, "settings-toggle-general.dateFormat.pretty");
    click_on(cx, "settings-apply");
    assert_eq!(global_value(cx, "general.dateFormat.overrideSystem"), Some(json!(true)));
    assert_eq!(global_value(cx, "general.dateFormat.pattern"), Some(json!("yyyy-MM-dd")));
    assert_eq!(global_value(cx, "general.dateFormat.pretty"), Some(json!(false)));
}

/// What a tracing subscriber writes, kept to be read back.
#[derive(Clone, Default)]
struct Captured(std::sync::Arc<parking_lot::Mutex<Vec<u8>>>);

impl std::io::Write for Captured {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Captured {
    type Writer = Captured;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// Every file under `dir`.
fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            files.extend(walk(&path));
        } else {
            files.push(path);
        }
    }
    files
}

// HTTP Proxy: the modes, the manual configuration and its credentials, with the problem that
// keeps it from working; a manual proxy with authentication applies, the password is kept by the
// backend outside the settings file, and it is never drawn, listed, logged or in a Debug output.
#[gpui_kit::test]
async fn http_proxy_manual_configuration_applies_and_the_password_stays_secret(cx: &mut TestAppContext) {
    const SECRET: &str = "pr0xyS3cretE2E";
    let log = Captured::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(log.clone())
        .with_max_level(tracing::Level::TRACE)
        .with_ansi(false)
        .finish();
    let _logging = tracing::subscriber::set_default(subscriber);
    let home = start(cx);
    assert!(!ide_ssh::secrets::default_store_installed(), "the system's credential store is never set up");
    // The checker answers without any connection.
    let checked: Rc<RefCell<Vec<String>>> = Rc::default();
    let seen = checked.clone();
    let checker: general_http_proxy::Checker = Rc::new(
        move |proxy: ide_core::settings::ProxySettings,
              url: String,
              then: Then<Result<String, String>>,
              cx: &mut App| {
            seen.borrow_mut().push(format!("{url} via {}:{}", proxy.host, proxy.port));
            then(Ok("200 OK".to_owned()), cx);
        },
    );
    cx.update(|cx| cx.set_global(general_http_proxy::ConnectionCheck(checker)));
    let (workbench, cx) = window(cx);
    let dialog = open_page(&workbench, general_http_proxy::PAGE, cx);
    let view: Entity<general_http_proxy::HttpProxyView> =
        custom(&dialog, general_http_proxy::PAGE, general_http_proxy::SLOT, cx);
    let lines = view.read_with(cx, |view, cx| view.lines(cx));
    assert_eq!(
        lines,
        [
            "( ) No proxy (•) Auto-detect proxy settings ( ) Manual proxy configuration",
            "    Uses the system proxy: the HTTP_PROXY, HTTPS_PROXY and NO_PROXY environment variables.",
            "(•) HTTP ( ) SOCKS (disabled)",
            "Host name:  (disabled)",
            "Port number: 80 (disabled)",
            "No proxy for:  (disabled)",
            "    Comma-separated host names; * matches any characters, e.g. *.example.com, 192.168.*",
            "[ ] Proxy authentication (disabled)",
            "Login:  (disabled)",
            "Password:  (disabled)",
            "[Check connection…]",
        ]
        .map(str::to_owned)
    );
    click_on(cx, "settings-radio-proxy.mode-manual");
    let lines = view.read_with(cx, |view, cx| view.lines(cx));
    assert!(lines.contains(&"! Proxy host is empty".to_owned()), "{lines:?}");
    type_into(cx, "settings-text-proxy.host", "proxy.example.test");
    type_into(cx, "settings-number-proxy.port", "3128");
    click_on(cx, "settings-toggle-proxy.authenticate");
    type_into(cx, "settings-text-proxy.login", "me");
    let password = view.read_with(cx, |view, _| view.password_field().clone());
    cx.update(|window, cx| window.focus(&password.focus_handle(cx), cx));
    cx.simulate_input(SECRET);
    cx.run_until_parked();
    let lines = view.read_with(cx, |view, cx| view.lines(cx));
    assert!(lines.iter().all(|line| !line.contains(SECRET)), "{lines:?}");
    let staged = dialog.read_with(cx, |view, cx| format!("{:?}", view.model().read(cx).staged().collect::<Vec<_>>()));
    assert!(!staged.contains(SECRET), "a staged change's Debug output names the password");
    let form = view.read_with(cx, |view, cx| view.form(cx));
    assert!(!format!("{form:?} {:?}", form.settings()).contains(SECRET), "the proxy form's Debug output names it");
    click_on(cx, "settings-proxy-check");
    let url = view.read_with(cx, |view, _| view.url_field().clone());
    type_over(cx, &url, "http://example.test/");
    click_on(cx, "settings-proxy-check-run");
    assert_eq!(checked.borrow().as_slice(), ["http://example.test/ via proxy.example.test:3128"]);
    click_on(cx, "settings-apply");
    assert_eq!(global_value(cx, "proxy.mode"), Some(json!("manual")));
    assert_eq!(global_value(cx, "proxy.host"), Some(json!("proxy.example.test")));
    assert_eq!(global_value(cx, "proxy.port"), Some(json!(3128)));
    assert_eq!(global_value(cx, "proxy.login"), Some(json!("me")));
    assert_eq!(effective_value(cx, "proxy.password"), Some(json!(SECRET)), "the backend keeps the password");
    let base = dialog.read_with(cx, |view, cx| view.model().read(cx).base.clone());
    assert_eq!(base["proxy"]["password"], json!(SECRET), "the dialog's settings hold the saved password");
    let model = dialog.read_with(cx, |view, cx| format!("{:?}", view.model().read(cx)));
    assert!(model.contains("proxy.example.test") && !model.contains(SECRET), "the dialog's Debug output names it");
    let file = global_settings_file(&home);
    assert!(file.contains("proxy.example.test") && !file.contains(SECRET), "{file}");
    let logged = String::from_utf8_lossy(&log.0.lock()).into_owned();
    assert!(!logged.contains(SECRET), "the log names the password");
    let kept: Vec<PathBuf> = walk(home.path())
        .into_iter()
        .filter(|file| std::fs::read(file).is_ok_and(|bytes| String::from_utf8_lossy(&bytes).contains(SECRET)))
        .collect();
    // Test builds keep saved passwords in a vault file beside the index, in place of the system's store.
    let vault = |file: &PathBuf| file.file_name().is_some_and(|name| name == "proxy-password.json.vault");
    assert!(!kept.is_empty() && kept.iter().all(vault), "{kept:?}");
}

// Updates: listed once Check for Updates is registered; the version, Last checked as the backend
// reports it and Check Now, which runs the action; the automatic check and the source apply.
#[gpui_kit::test]
async fn updates_status_check_now_and_the_automatic_check_apply(cx: &mut TestAppContext) {
    let _home = start(cx);
    let ran = Rc::new(Cell::new(0));
    let count = ran.clone();
    cx.update(|cx| {
        cx.global_mut::<ActionRegistry>().handle(general_updates::CHECK_ACTION, move |_, _, _| {
            count.set(count.get() + 1);
            Ok(())
        });
    });
    let (workbench, cx) = window(cx);
    let dialog = open_page(&workbench, general_updates::PAGE, cx);
    let view: Entity<general_updates::UpdateStatusView> =
        custom(&dialog, general_updates::PAGE, general_updates::SLOT, cx);
    let version = format!("Current version: {}", env!("OPENIDEA_VERSION"));
    let lines = view.read_with(cx, |view, cx| view.lines(cx));
    assert_eq!(lines, [version.clone(), "Last checked: Loading… [Check Now] (disabled)".to_owned()]);
    let controls = dialog.read_with(cx, |view, cx| view.control_lines(cx));
    assert!(controls.contains(&"[ ] Check for updates automatically".to_owned()), "{controls:?}");
    assert!(controls.contains(&"Update source: Automatic ▾".to_owned()), "{controls:?}");
    let status = ide_core::updates::UpdateStatus { last_checked: None, checking: false };
    cx.update(|_, cx| cx.global::<Backend>().emit(None, ide_core::IdeEvent::UpdateStatusChanged { status }));
    settle(cx, |cx| view.read_with(cx, |view, cx| view.lines(cx))[1] == "Last checked: Never [Check Now]");
    click_on(cx, "settings-updates-check");
    assert_eq!(ran.get(), 1, "Check Now runs Check for Updates");
    click_on(cx, "settings-toggle-general.autoCheckUpdates");
    click_on(cx, "settings-select-general.updateSource");
    click_on(cx, "settings-choice-general.updateSource-github");
    click_on(cx, "settings-apply");
    assert_eq!(global_value(cx, "general.autoCheckUpdates"), Some(json!(true)));
    assert_eq!(global_value(cx, "general.updateSource"), Some(json!("github")));
}

// File Types: the recognized types with the patterns of the one selected, the ignored names and
// the exclude patterns; in This project, a pattern added to Python and an ignored name added are
// written to the project's settings file.
#[gpui_kit::test]
async fn file_types_patterns_and_ignored_names_are_written_to_the_project(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = project(cx);
    let dialog = open_page(&workbench, editor_files::PAGE, cx);
    click_on(cx, "settings-scope-project");
    let types: Entity<editor_files::RecognizedFileTypesView> =
        custom(&dialog, editor_files::PAGE, "File name patterns", cx);
    let lines = types.read_with(cx, |view, cx| view.lines(cx));
    assert!(lines.contains(&"  Python".to_owned()), "{lines:?}");
    // The list scrolls: the filter brings Python up before it is clicked.
    type_into(cx, "settings-file-types-filter", "Python");
    click_on(cx, "settings-file-type-python");
    let lines = types.read_with(cx, |view, cx| view.lines(cx));
    assert!(lines.contains(&"> Python".to_owned()), "{lines:?}");
    assert!(lines.contains(&"  *.py".to_owned()), "{lines:?}");
    let ignored: Entity<ListEditor> = custom(&dialog, editor_files::PAGE, "Ignore files and folders", cx);
    let defaults = effective_value(cx, "files.ignoredNames");
    let rows = ignored.read_with(cx, |view, cx| view.rows(cx));
    let listed: Vec<String> = rows.into_iter().flatten().collect();
    assert_eq!(Some(Value::from(listed)), defaults, "the ignored names in effect are listed");
    let exclude: Entity<ListEditor> = custom(&dialog, editor_files::PAGE, "Exclude patterns", cx);
    let excluded = effective_value(cx, "files.exclude");
    let rows: Vec<String> = exclude.read_with(cx, |view, cx| view.rows(cx)).into_iter().flatten().collect();
    assert_eq!(Some(Value::from(rows)), excluded, "the exclude patterns in effect are listed");

    click_on(cx, "settings-table-file-type-patterns-add");
    cx.simulate_input("*.snake");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    bring_into_view(cx, "settings-list-files.ignoredNames-add");
    click_on(cx, "settings-list-files.ignoredNames-add");
    cx.simulate_input("*.tmp");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    click_on(cx, "settings-apply");
    let python = effective_value(cx, "files.fileTypes.python.patterns");
    assert_eq!(python, Some(json!(["*.snake"])));
    let names =
        effective_value(cx, "files.ignoredNames").and_then(|value| value.as_array().cloned()).unwrap_or_default();
    assert!(names.contains(&json!("*.tmp")), "{names:?}");
    let file = project_file(&root);
    assert!(file.contains("*.snake") && file.contains("*.tmp"), "{file}");
}

// CMake: without stored profiles the Debug profile with its fields and environment; the build
// directory typed and a variable added are written to the project.
#[gpui_kit::test]
async fn cmake_profile_settings_are_written_to_the_project(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = project(cx);
    let dialog = open_page(&workbench, build_cmake::PAGE, cx);
    let view: Entity<build_cmake::CMakeProfilesView> = page_view(&dialog, build_cmake::PAGE, cx);
    let lines = view.read_with(cx, |view, cx| view.lines(cx));
    assert_eq!(
        lines,
        [
            "— CMake profiles —",
            "> Debug",
            "— Profile settings —",
            "Name: Debug",
            "Build type: Debug",
            "Generator: ",
            "CMake arguments: ",
            "Build directory: ",
            "Build arguments: ",
            "Environment variables:",
            "  No variables",
        ]
        .map(str::to_owned)
    );
    let build_dir = view.read_with(cx, |view, _| view.field("buildDir").cloned()).expect("the field");
    assert_eq!(build_dir.read_with(cx, |field, _| field.placeholder().to_owned()), "cmake-build-debug");
    type_into(cx, "settings-text-cmake.buildDir", "out/debug");
    click_on(cx, "settings-table-cmake-env-add");
    cx.simulate_input("CC");
    cx.simulate_keystrokes("tab");
    cx.simulate_input("clang");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    click_on(cx, "settings-apply");
    let profiles = effective_value(cx, "cmake.profiles").unwrap_or(Value::Null);
    assert_eq!(profiles[0]["buildDir"], json!("out/debug"));
    assert_eq!(profiles[0]["env"], json!({ "CC": "clang" }));
    let file = project_file(&root);
    assert!(file.contains("out/debug") && file.contains("clang"), "{file}");
}

// Gradle: who builds and runs, the Gradle JVM among the installations found, the distribution,
// Gradle home with a local installation, the user home; the JVM chosen, a local installation and
// its home are written to the project.
#[gpui_kit::test]
async fn gradle_jvm_distribution_and_home_are_written_to_the_project(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (folder, root, workbench, cx) = project(cx);
    let runs = fake_installations(folder.path(), cx);
    let dialog = open_page(&workbench, build_gradle::PAGE, cx);
    assert_eq!(dialog.read_with(cx, |view, cx| view.scope(cx)), crate::settings_model::staging::Scope::Project);
    let page: Entity<ToolPage> = page_view(&dialog, build_gradle::PAGE, cx);
    settle(cx, |_| !runs.borrow().is_empty());
    let lines = tool_page_lines(&page, cx);
    let labels: Vec<&str> = lines.iter().filter(|line| !line.starts_with("    ")).map(String::as_str).collect();
    assert_eq!(
        labels,
        [
            "Build and run using: Gradle ▾",
            "Gradle JVM: Project JDK ▾",
            "Distribution: Gradle wrapper ▾",
            "Gradle user home: "
        ]
    );
    click_on(cx, "settings-sdk-languages.java.gradle.jvm");
    click_on(cx, "settings-sdk-languages.java.gradle.jvm-choice-1");
    click_on(cx, "settings-select-languages.java.gradle.distribution");
    click_on(cx, "settings-choice-languages.java.gradle.distribution-local");
    let gradle_home = folder.path().join("gradle-8").display().to_string();
    type_into(cx, "settings-text-languages.java.gradle.home", &gradle_home);
    click_on(cx, "settings-apply");
    let jdk = folder.path().join("jdks/temurin-21").display().to_string();
    assert_eq!(effective_value(cx, "languages.java.gradle.jvm"), Some(json!(jdk)));
    assert_eq!(effective_value(cx, "languages.java.gradle.distribution"), Some(json!("local")));
    assert_eq!(effective_value(cx, "languages.java.gradle.home"), Some(json!(gradle_home)));
    let file = project_file(&root);
    assert!(file.contains("gradle-8") && file.contains("temurin-21"), "{file}");
}

// Maven: the JRE and Maven home among the installations found, the files, the repository, the
// profiles, the thread count and the daemon; the Maven home chosen and a thread count typed are
// written to the project.
#[gpui_kit::test]
async fn maven_home_and_thread_count_are_written_to_the_project(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (folder, root, workbench, cx) = project(cx);
    let runs = fake_installations(folder.path(), cx);
    let dialog = open_page(&workbench, build_maven::PAGE, cx);
    let page: Entity<ToolPage> = page_view(&dialog, build_maven::PAGE, cx);
    settle(cx, |_| runs.borrow().len() >= 2);
    let lines = tool_page_lines(&page, cx);
    let labels: Vec<&str> = lines.iter().filter(|line| !line.starts_with("    ")).map(String::as_str).collect();
    assert_eq!(
        labels,
        [
            "JRE: Use Project JDK ▾",
            "Maven home path: Auto ▾",
            "User settings file: ",
            "Toolchains file: ",
            "Local repository: ",
            "Active profiles: ",
            "Thread count: ",
            "[ ] Use Maven Daemon (mvnd)",
            "Maven Daemon home path: ",
        ]
    );
    click_on(cx, "settings-sdk-languages.java.maven.home");
    click_on(cx, "settings-sdk-languages.java.maven.home-choice-1");
    // The page is taller than the dialog: it scrolls to its last control.
    bring_into_view(cx, "settings-text-languages.java.maven.mvndHome");
    let threads = tool_page_field(&page, "languages.java.maven.threads", cx);
    type_over(cx, &threads, "2C");
    click_on(cx, "settings-apply");
    let maven = folder.path().join("maven-3.9").display().to_string();
    assert_eq!(effective_value(cx, "languages.java.maven.home"), Some(json!(maven)));
    assert_eq!(effective_value(cx, "languages.java.maven.threads"), Some(json!("2C")));
    let file = project_file(&root);
    assert!(file.contains("maven-3.9") && file.contains("2C"), "{file}");
}

// Actions on Save: the three actions and the note; in This project, Optimize imports ticked is
// written to the project.
#[gpui_kit::test]
async fn actions_on_save_are_written_to_the_project(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = project(cx);
    let dialog = open_page(&workbench, tools_actions_on_save::PAGE, cx);
    click_on(cx, "settings-scope-project");
    let form: Entity<FormView> = custom(&dialog, tools_actions_on_save::PAGE, "Reformat code", cx);
    let lines = form.read_with(cx, |form, cx| form.lines(cx));
    let labels: Vec<&str> = lines.iter().filter(|line| !line.starts_with("    ")).map(String::as_str).collect();
    assert_eq!(labels, ["[ ] Reformat code", "[ ] Optimize imports", "[ ] Run code cleanup"]);
    let note: Entity<FormView> = custom(&dialog, tools_actions_on_save::PAGE, "Save", cx);
    let note = note.read_with(cx, |form, cx| form.lines(cx));
    assert!(note[0].ends_with("Editor › General →."), "{note:?}");
    click_on(cx, "settings-toggle-actionsOnSave.optimizeImports");
    click_on(cx, "settings-apply");
    assert_eq!(effective_value(cx, "actionsOnSave.optimizeImports"), Some(json!(true)));
    assert!(project_file(&root).contains("optimizeImports"), "{}", project_file(&root));
}

// External Tools: an empty list says how to add one; + adds a tool whose program is a script in
// the temporary folder (never run), with a macro inserted into its arguments; it applies.
#[gpui_kit::test]
async fn external_tools_a_tool_added_applies(cx: &mut TestAppContext) {
    let _home = start(cx);
    let folder = tempfile::tempdir().expect("a temporary folder");
    let script = folder.path().join("lint.sh");
    std::fs::write(&script, "#!/bin/sh\nexit 0\n").expect("the script is written");
    let (workbench, cx) = window(cx);
    let dialog = open_page(&workbench, tools_external_tools::PAGE, cx);
    let view: Entity<tools_external_tools::ExternalToolsView> =
        custom(&dialog, tools_external_tools::PAGE, tools_external_tools::SLOT, cx);
    let lines = view.read_with(cx, |view, cx| view.lines(cx));
    assert_eq!(lines[0], "  No external tools. Click + to add one.");
    click_on(cx, "settings-tools-add");
    let lines = view.read_with(cx, |view, cx| view.lines(cx));
    assert_eq!(
        lines[..10],
        [
            "> [x] Tool 1 | External Tools",
            "— Tool Settings —",
            "Name: Tool 1",
            "Group: External Tools",
            "Description: ",
            "Program:  [Insert Macro…]",
            "Arguments:  [Insert Macro…]",
            "Working directory: $ProjectFileDir$ [Insert Macro…]",
            "[x] Synchronize files after execution",
            "[x] Open console for tool output",
        ]
        .map(str::to_owned)
    );
    type_into(cx, "settings-text-tool.program", &script.display().to_string());
    click_on(cx, "settings-tool-macro-arguments");
    click_on(cx, "settings-tool-macro-arguments-0");
    click_on(cx, "settings-apply");
    let tools = global_value(cx, "externalTools").unwrap_or(Value::Null);
    assert_eq!(tools[0]["name"], json!("Tool 1"));
    assert_eq!(tools[0]["program"], json!(script.display().to_string()));
    assert_eq!(tools[0]["arguments"], json!("$FilePath$"));
}

// Directory Mappings: without a project the table says so; with one it maps the project's root
// (here outside any repository) and nothing is there to apply.
#[gpui_kit::test]
async fn directory_mappings_show_the_project_root(cx: &mut TestAppContext) {
    let _home = start(cx);
    {
        let (workbench, cx) = window(cx);
        let dialog = open_page(&workbench, vcs_mappings::PAGE, cx);
        let view: Entity<vcs_mappings::MappingsView> = custom(&dialog, vcs_mappings::PAGE, vcs_mappings::SLOT, cx);
        assert_eq!(view.read_with(cx, |view, cx| view.lines(cx)), ["Directory | VCS", "No project is open"]);
        click_on(cx, "settings-cancel");
    }
    let (_folder, root, workbench, cx) = project(cx);
    let dialog = open_page(&workbench, vcs_mappings::PAGE, cx);
    let view: Entity<vcs_mappings::MappingsView> = custom(&dialog, vcs_mappings::PAGE, vcs_mappings::SLOT, cx);
    let lines = view.read_with(cx, |view, cx| view.lines(cx));
    assert_eq!(lines[1], format!("<Project> {} | <none>", root.display()));
    assert!(cx.debug_bounds("settings-vcs-mapping-0").is_some());
    assert!(!dialog.read_with(cx, |view, cx| view.can_apply(cx)));
}

// Directory Mappings: a project whose root is a Git repository is mapped to Git.
#[gpui_kit::test]
async fn directory_mappings_map_a_repository_root_to_git(cx: &mut TestAppContext) {
    let _home = start(cx);
    let folder = tempfile::tempdir().expect("a temporary folder");
    let root = folder.path().canonicalize().expect("the folder exists");
    init_repo(&root);
    std::fs::write(root.join("a.txt"), "a\n").expect("the file is written");
    cx.update(|cx| cx.global::<Backend>().host.trust.decide(&root, true, false)).expect("the project is trusted");
    let (workbench, cx) = window(cx);
    workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
    // The repositories of a project are found after it opens; the table is read when Settings opens.
    let found = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| cx.global::<Backend>().host.project("w1").is_ok_and(|p| !p.repos.read().is_empty()))
    };
    settle(cx, found);
    let dialog = open_page(&workbench, vcs_mappings::PAGE, cx);
    let view: Entity<vcs_mappings::MappingsView> = custom(&dialog, vcs_mappings::PAGE, vcs_mappings::SLOT, cx);
    assert_eq!(view.read_with(cx, |view, cx| view.lines(cx))[1], format!("<Project> {} | Git", root.display()));
}

// Issue Navigation: the table, its hint and the preset buttons; in This project, the JIRA pattern
// added with its button and a link typed into the table are written to the project.
#[gpui_kit::test]
async fn issue_navigation_links_are_written_to_the_project(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = project(cx);
    let dialog = open_page(&workbench, vcs_issue_navigation::PAGE, cx);
    click_on(cx, "settings-scope-project");
    let table: Entity<vcs_issue_navigation::IssueNavigationView> =
        custom(&dialog, vcs_issue_navigation::PAGE, vcs_issue_navigation::TABLE_SLOT, cx);
    let lines = table.read_with(cx, |view, cx| view.lines(cx));
    assert_eq!(lines[..2], ["Issue navigation:", "  No issue navigation links"].map(str::to_owned));
    let presets: Entity<vcs_issue_navigation::IssuePresetsView> =
        custom(&dialog, vcs_issue_navigation::PAGE, vcs_issue_navigation::PRESETS_SLOT, cx);
    let buttons = presets.read_with(cx, |view, cx| view.lines(cx));
    assert_eq!(buttons, ["Add pattern: [Add JIRA Pattern] [Add YouTrack Pattern] [Add GitHub Pattern]"]);
    click_on(cx, "settings-issue-preset-jira");
    click_on(cx, "settings-table-issues-add");
    cx.simulate_input("OPS-\\d+");
    cx.simulate_keystrokes("tab");
    cx.simulate_input("https://ops.example.test/$0");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    click_on(cx, "settings-apply");
    let links = effective_value(cx, "vcs.issueNavigation").unwrap_or(Value::Null);
    assert_eq!(links[0]["link"], json!("https://jira.example.com/browse/$0"));
    assert_eq!(links[1], json!({ "pattern": "OPS-\\d+", "link": "https://ops.example.test/$0" }));
    let file = project_file(&root);
    assert!(file.contains("jira.example.com") && file.contains("ops.example.test"), "{file}");
}

/// What a test does to a control: clicks, text typed over a field, or a row added to a table.
enum Act {
    Click(&'static [&'static str]),
    Type(&'static str, &'static str),
    Add(&'static str, &'static str),
}

// Reset discards an edit, and Cancel after the same edit writes nothing, on every page that has a
// control to change: standard controls (Updates), controls of their own (Date Formats, HTTP Proxy,
// File Types, Actions on Save, External Tools, Issue Navigation) and pages drawn by a view of their
// own (CMake, Gradle, Maven).
#[gpui_kit::test]
async fn reset_and_cancel_discard_an_edit_on_each_kind_of_page(cx: &mut TestAppContext) {
    let _home = start(cx);
    cx.update(|cx| cx.global_mut::<ActionRegistry>().handle(general_updates::CHECK_ACTION, |_, _, _| Ok(())));
    let (_folder, _root, workbench, cx) = project(cx);
    let cases: [(&str, Act, &str); 10] = [
        (general_updates::PAGE, Act::Click(&["settings-toggle-general.autoCheckUpdates"]), "general.autoCheckUpdates"),
        (
            general_date_formats::PAGE,
            Act::Click(&["settings-toggle-general.dateFormat.pretty"]),
            "general.dateFormat.pretty",
        ),
        (general_http_proxy::PAGE, Act::Click(&["settings-radio-proxy.mode-none"]), "proxy.mode"),
        (editor_files::PAGE, Act::Add("settings-table-file-type-patterns-add", "*.smoke"), "files.fileTypes"),
        (tools_actions_on_save::PAGE, Act::Click(&["settings-toggle-actionsOnSave.cleanup"]), "actionsOnSave.cleanup"),
        (tools_external_tools::PAGE, Act::Click(&["settings-tools-add"]), "externalTools"),
        (vcs_issue_navigation::PAGE, Act::Click(&["settings-issue-preset-github"]), "vcs.issueNavigation"),
        (build_cmake::PAGE, Act::Type("settings-text-cmake.generator", "Ninja"), "cmake.profiles"),
        (
            build_gradle::PAGE,
            Act::Click(&[
                "settings-select-languages.java.gradle.distribution",
                "settings-choice-languages.java.gradle.distribution-local",
            ]),
            "languages.java.gradle.distribution",
        ),
        (
            build_maven::PAGE,
            Act::Click(&["settings-toggle-languages.java.maven.daemon"]),
            "languages.java.maven.daemon",
        ),
    ];
    for (page, act, key) in cases {
        let before = effective_value(cx, key);
        let dialog = open_page(&workbench, page, cx);
        let run = |cx: &mut VisualTestContext| match &act {
            Act::Click(at) => at.iter().for_each(|at| click_on(cx, at)),
            Act::Type(at, text) => type_into(cx, at, text),
            Act::Add(at, text) => {
                click_on(cx, at);
                cx.simulate_input(text);
                cx.simulate_keystrokes("enter");
                cx.run_until_parked();
            }
        };
        run(cx);
        assert!(is_modified(&dialog, cx), "{page}: the edit is staged");
        click_on(cx, "settings-reset");
        assert!(!is_modified(&dialog, cx), "{page}: Reset discards it");
        run(cx);
        click_on(cx, "settings-cancel");
        assert!(settings_dialog_of(&workbench, cx).is_none(), "{page}: Cancel closes the dialog");
        assert_eq!(effective_value(cx, key), before, "{page}: Cancel writes nothing");
    }
}
