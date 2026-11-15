//! Settings pages opened through File › Settings… as a user opens them, each driven as the page's
//! own cases drive it: CMake's argument fields and profiles, the Maven runner's JRE and files, the
//! Gradle page found by the search, File Encodings, the UI fallback font, Main menu, shared JDT LS
//! against the global and project layers and the Java language server's target, and the HTTP
//! Proxy's settings staged key by key. Values are read from what the page shows, from the edits
//! staged and from the settings in effect (and the project's settings file for a project's page).
//!
//! The installations offered are the ones the test gives: no toolchain is probed.

use super::proofs19_settings_misc::bring_into_view;
use super::*;
use crate::platform::command_line::parse_args;
use crate::platform::settings::{SettingsWrite, effective, global, set, set_global};
use crate::settings_model::builtin_pages::settings_registry;
use crate::settings_model::staging::Scope;
use crate::settings_view::dialog::SettingsDialogView;
use crate::settings_view::pages::build_cmake::CMakeProfilesView;
use crate::settings_view::pages::build_tool_page::ToolPage;
use crate::settings_view::pages::font_picker::FontCatalog;
use crate::settings_view::pages::languages_controls::LanguagePageView;
use crate::settings_view::pages::languages_sdk::{SdkDetection, SdkFieldView, Toolchain, ToolchainKind};
use crate::settings_view::pages::{FormView, appearance, build_cmake, build_gradle, build_maven, general_http_proxy};
use crate::theme::fonts::{FontStatus, SystemFonts};
use crate::ui::text_field::TextField;
use gpui_kit::{AnyView, Focusable};
use ide_core::settings::CMakeProfile;
use serde_json::{Value, json};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

const JDTLS_SHARED: &str = "languages.java.jdtlsShared";
const SHARED_TOGGLE: &str = "settings-toggle-languages.java.jdtlsShared";

fn window(cx: &mut TestAppContext) -> (Entity<Workbench>, &mut VisualTestContext) {
    let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
    cx.update(|window, _| window.activate_window());
    (workbench, cx)
}

/// A trusted project of `files`, open in an active window.
fn project<'a>(
    cx: &'a mut TestAppContext,
    files: &[(&str, &str)],
) -> (tempfile::TempDir, PathBuf, Entity<Workbench>, &'a mut VisualTestContext) {
    let (folder, root, workbench, cx) = trusted_project(cx, files);
    cx.update(|window, _| window.activate_window());
    (folder, root, workbench, cx)
}

/// The project's own settings file; empty while there is none.
fn project_file(root: &Path) -> String {
    std::fs::read_to_string(root.join(".openidea/settings.toml")).unwrap_or_default()
}

/// File › Settings… on `page`.
fn open_page(workbench: &Entity<Workbench>, page: &str, cx: &mut VisualTestContext) -> Entity<SettingsDialogView> {
    run_action(cx, "file.settings", Some(Value::from(page)));
    let dialog = settings_dialog_of(workbench, cx).expect("Settings opens");
    assert_eq!(dialog.read_with(cx, |view, cx| view.selected_page(cx)).as_deref(), Some(page));
    dialog
}

/// The view drawn for the whole of `page`.
fn page_view<V: 'static>(dialog: &Entity<SettingsDialogView>, page: &str, cx: &mut VisualTestContext) -> Entity<V> {
    let view: Option<AnyView> = dialog.read_with(cx, |view, _| view.page_view(page).cloned());
    view.and_then(|view| view.downcast().ok()).unwrap_or_else(|| panic!("{page} has its view"))
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

/// The language page `page`'s own view, in Settings opened on it.
fn language_page(workbench: &Entity<Workbench>, page: &str, cx: &mut VisualTestContext) -> Entity<LanguagePageView> {
    let dialog = open_page(workbench, page, cx);
    page_view(&dialog, page, cx)
}

fn language_lines(view: &Entity<LanguagePageView>, cx: &mut VisualTestContext) -> Vec<String> {
    view.read_with(cx, |view, cx| view.lines(cx))
}

fn effective_value(cx: &mut VisualTestContext, key: &str) -> Option<Value> {
    cx.update(|_, cx| setting_of(&effective("w1", cx), key))
}

fn global_value(cx: &mut VisualTestContext, key: &str) -> Option<Value> {
    cx.update(|_, cx| setting_of(&global(cx), key))
}

/// The value of `key` in `scope` as the dialog's pages show it: the edit staged, else the scope's.
fn staged_value(
    dialog: &Entity<SettingsDialogView>,
    scope: Scope,
    key: &str,
    cx: &mut VisualTestContext,
) -> Option<Value> {
    dialog.read_with(cx, |view, cx| view.model().read(cx).staged_value(scope, key))
}

fn is_modified(dialog: &Entity<SettingsDialogView>, cx: &mut VisualTestContext) -> bool {
    dialog.read_with(cx, |view, cx| view.is_modified(cx))
}

/// Clicks the field drawn as `at` and types `text` over its text.
fn type_into(cx: &mut VisualTestContext, at: &'static str, text: &str) {
    click_on(cx, at);
    cx.simulate_keystrokes("cmd-a");
    cx.simulate_input(text);
    cx.run_until_parked();
}

/// Gives `field` the keyboard and types `text` over its text.
fn type_over(cx: &mut VisualTestContext, field: &Entity<TextField>, text: &str) {
    cx.update(|window, cx| {
        field.update(cx, |field, cx| field.select_all(cx));
        window.focus(&field.focus_handle(cx), cx);
    });
    cx.simulate_input(text);
    cx.run_until_parked();
}

/// Types `text` into the Settings search.
fn search(dialog: &Entity<SettingsDialogView>, text: &str, cx: &mut VisualTestContext) {
    let field = dialog.read_with(cx, |view, _| view.search_field().clone());
    cx.update(|window, cx| window.focus(&field.focus_handle(cx), cx));
    cx.simulate_input(text);
    cx.run_until_parked();
}

/// Stores `value` for `key` in the project's settings.
fn store_in_project(key: &str, value: Value, cx: &mut VisualTestContext) {
    cx.update(|_, cx| set("w1", Scope::Project, key, &value, cx)).expect("the setting is stored");
}

/// The text field of setting `key` in a form of a Build Tools page.
fn tool_page_field(page: &Entity<ToolPage>, key: &str, cx: &mut VisualTestContext) -> Entity<TextField> {
    let parts = page.read_with(cx, |page, cx| page.shown_parts(cx));
    let mut forms = parts.into_iter().filter_map(|part| part.downcast::<FormView>().ok());
    let found = forms.find_map(|form| form.read_with(cx, |form, _| form.field(key).cloned()));
    found.unwrap_or_else(|| panic!("the page has a field for {key}"))
}

/// The first line of the installation combo of setting `key` on a Build Tools page.
fn sdk_line(page: &Entity<ToolPage>, key: &str, cx: &mut VisualTestContext) -> String {
    let parts = page.read_with(cx, |page, cx| page.shown_parts(cx));
    let mut combos = parts.into_iter().filter_map(|part| part.downcast::<SdkFieldView>().ok());
    let combo = combos.find(|combo| combo.read_with(cx, |combo, _| combo.spec().key == key));
    let combo = combo.unwrap_or_else(|| panic!("the page has a combo for {key}"));
    combo.read_with(cx, |combo, cx| combo.lines(cx)).remove(0)
}

/// Detection that finds the one JDK at `path`; logs the kinds it is asked for.
fn give_jdk(path: &str, cx: &mut VisualTestContext) -> Rc<RefCell<Vec<ToolchainKind>>> {
    let jdk = Toolchain {
        path: path.to_owned(),
        name: "JDK 21".to_owned(),
        version: Some("21".to_owned()),
        source: "system".to_owned(),
        ..Toolchain::default()
    };
    let test = SdkDetection { found: vec![(ToolchainKind::Jdk, vec![jdk])], ..SdkDetection::default() };
    let asked = test.asked.clone();
    cx.update(|_, cx| cx.set_global(test));
    asked
}

/// The text of the CMake profile field `name` (`cmakeArgs`, `buildArgs`, …).
fn cmake_text(view: &Entity<CMakeProfilesView>, name: &str, cx: &mut VisualTestContext) -> String {
    let text = view.read_with(cx, |view, cx| view.field(name).map(|field| field.read(cx).text().to_owned()));
    text.unwrap_or_else(|| panic!("the profile has a {name} field"))
}

/// A Debug profile whose arguments of `name` are `args`.
fn profile_with(name: &str, args: &[&str]) -> CMakeProfile {
    let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
    match name {
        "cmakeArgs" => CMakeProfile { cmake_args: args, ..CMakeProfile::default() },
        _ => CMakeProfile { build_args: args, ..CMakeProfile::default() },
    }
}

fn profiles_json(profiles: &[CMakeProfile]) -> Value {
    serde_json::to_value(profiles).expect("the profiles are JSON")
}

// round-trips spaces, quotes and empty values in %s
#[gpui_kit::test]
async fn round_trips_spaces_quotes_and_empty_values_in_the_cmake_and_build_arguments(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = project(cx, &[("CMakeLists.txt", "project(a)\n")]);
    let args = ["two words", "a \"quote\"", "it's", "", r"C:\some path\file"];
    let typed = r#"-DNAME="two words" "-DQUOTE=it's" """#;
    let expected = ["-DNAME=two words", "-DQUOTE=it's", ""];
    let fields: [(&str, &'static str); 2] =
        [("cmakeArgs", "settings-text-cmake.cmakeArgs"), ("buildArgs", "settings-text-cmake.buildArgs")];
    for (name, at) in fields {
        store_in_project("cmake.profiles", profiles_json(&[profile_with(name, &args)]), cx);
        let dialog = open_page(&workbench, build_cmake::PAGE, cx);
        let view: Entity<CMakeProfilesView> = page_view(&dialog, build_cmake::PAGE, cx);
        assert_eq!(parse_args(&cmake_text(&view, name, cx), false), args, "{name}: the stored words are shown");
        type_into(cx, at, typed);
        assert_eq!(cmake_text(&view, name, cx), typed, "{name}: the text typed stays as typed");
        let staged = staged_value(&dialog, Scope::Project, "cmake.profiles", cx);
        assert_eq!(staged, Some(profiles_json(&[profile_with(name, &expected)])), "{name}: the words are staged");
        click_on(cx, "settings-ok");
        assert!(settings_dialog_of(&workbench, cx).is_none(), "OK closes Settings");
        let dialog = open_page(&workbench, build_cmake::PAGE, cx);
        let view: Entity<CMakeProfilesView> = page_view(&dialog, build_cmake::PAGE, cx);
        assert_eq!(parse_args(&cmake_text(&view, name, cx), false), expected, "{name}: shown again as stored");
        click_on(cx, "settings-cancel");
    }
    let file = project_file(&root);
    assert!(file.contains("-DQUOTE=it's"), "{file}");
}

// displays each profile's arguments when switching profiles
#[gpui_kit::test]
async fn displays_each_profiles_arguments_when_switching_profiles(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _root, workbench, cx) = project(cx, &[("CMakeLists.txt", "project(a)\n")]);
    let release = CMakeProfile {
        name: "Release".to_owned(),
        cmake_args: vec!["-DMESSAGE=release build".to_owned()],
        ..CMakeProfile::default()
    };
    store_in_project("cmake.profiles", profiles_json(&[CMakeProfile::default(), release]), cx);
    let dialog = open_page(&workbench, build_cmake::PAGE, cx);
    let view: Entity<CMakeProfilesView> = page_view(&dialog, build_cmake::PAGE, cx);
    type_into(cx, "settings-text-cmake.cmakeArgs", "\"unfinished ");
    click_on(cx, "settings-cmake-profile-1");
    assert_eq!(view.read_with(cx, |view, cx| view.selected(cx)), 1);
    assert_eq!(parse_args(&cmake_text(&view, "cmakeArgs", cx), false), ["-DMESSAGE=release build"]);
}

// chooses an installed Maven runner JRE in project scope and returns to Project JDK
#[gpui_kit::test]
async fn chooses_an_installed_maven_runner_jre_in_project_scope_and_returns_to_project_jdk(cx: &mut TestAppContext) {
    const KEY: &str = "languages.java.maven.runnerJre";
    let _home = start(cx);
    let spec = settings_registry(true).into_iter().find(|spec| spec.id == build_maven::PAGE).expect("the Maven page");
    assert!(spec.project_scope);
    assert_eq!(spec.default_scope, Some(Scope::Project));
    for word in ["JRE", "runner", "JDK", "JAVA_HOME"] {
        assert!(spec.keywords.contains(&word), "the search finds Maven by {word}");
    }
    let (_folder, root, workbench, cx) = project(cx, &[("pom.xml", "<project/>\n")]);
    let jdk = root.join("jdks/21").display().to_string();
    let asked = give_jdk(&jdk, cx);
    let dialog = open_page(&workbench, build_maven::PAGE, cx);
    assert_eq!(dialog.read_with(cx, |view, cx| view.scope(cx)), Scope::Project);
    let page: Entity<ToolPage> = page_view(&dialog, build_maven::PAGE, cx);
    settle(cx, |_| asked.borrow().contains(&ToolchainKind::Jdk));
    assert_eq!(sdk_line(&page, KEY, cx), "JRE: Use Project JDK ▾");
    click_on(cx, "settings-sdk-languages.java.maven.runnerJre");
    click_on(cx, "settings-sdk-languages.java.maven.runnerJre-choice-1");
    assert_eq!(sdk_line(&page, KEY, cx), "JRE: JDK 21 ▾");
    assert_eq!(staged_value(&dialog, Scope::Project, KEY, cx), Some(json!(jdk)));
    assert_eq!(staged_value(&dialog, Scope::Global, KEY, cx), Some(json!("")));
    click_on(cx, "settings-sdk-languages.java.maven.runnerJre");
    click_on(cx, "settings-sdk-languages.java.maven.runnerJre-choice-0");
    assert_eq!(sdk_line(&page, KEY, cx), "JRE: Use Project JDK ▾");
    assert_eq!(staged_value(&dialog, Scope::Project, KEY, cx), Some(json!("")));
}

// Settings dialog › stages properties conversion through its action and applies the checkbox
#[gpui_kit::test]
async fn stages_properties_conversion_and_applies_the_checkbox(cx: &mut TestAppContext) {
    const KEY: &str = "editor.encodings.nativeToAscii";
    const TOGGLE: &str = "settings-toggle-editor.encodings.nativeToAscii";
    let _home = start(cx);
    let (workbench, cx) = window(cx);
    let dialog = open_page(&workbench, "editor.fileEncodings", cx);
    let lines = dialog.read_with(cx, |view, cx| view.control_lines(cx));
    assert!(lines.contains(&"[ ] Transparent native-to-ascii conversion".to_owned()), "{lines:?}");
    bring_into_view(cx, TOGGLE);
    click_on(cx, TOGGLE);
    let lines = dialog.read_with(cx, |view, cx| view.control_lines(cx));
    assert!(lines.contains(&"[x] Transparent native-to-ascii conversion".to_owned()), "{lines:?}");
    assert_eq!(global_value(cx, KEY), Some(json!(false)), "nothing is written before Apply");
    click_on(cx, "settings-apply");
    assert_eq!(global_value(cx, KEY), Some(json!(true)));
}

// Settings dialog › edits CMake profiles in project settings and persists them on Apply
#[gpui_kit::test]
async fn edits_cmake_profiles_in_project_settings_and_persists_them_on_apply(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = project(cx, &[("CMakeLists.txt", "project(a)\n")]);
    let dialog = open_page(&workbench, build_cmake::PAGE, cx);
    let view: Entity<CMakeProfilesView> = page_view(&dialog, build_cmake::PAGE, cx);
    assert!(view.read_with(cx, |view, cx| view.lines(cx)).contains(&"> Debug".to_owned()));
    let before = effective_value(cx, "cmake.profiles");
    click_on(cx, "settings-cmake-add");
    type_into(cx, "settings-text-cmake.name", "Release");
    type_into(cx, "settings-text-cmake.buildType", "Release");
    type_into(cx, "settings-text-cmake.generator", "Ninja");
    // The generator's suggestions lie over the fields below it: the last field gets the keyboard
    // without a click, which closes them.
    let build_args = view.read_with(cx, |view, _| view.field("buildArgs").cloned()).expect("the field");
    type_over(cx, &build_args, "-j 8");
    assert_eq!(effective_value(cx, "cmake.profiles"), before, "nothing is written before Apply");
    click_on(cx, "settings-apply");
    let profiles = effective_value(cx, "cmake.profiles").unwrap_or(Value::Null);
    assert_eq!((&profiles[0]["name"], &profiles[0]["buildType"]), (&json!("Debug"), &json!("Debug")));
    assert_eq!(profiles[1]["name"], json!("Release"));
    assert_eq!(profiles[1]["buildType"], json!("Release"));
    assert_eq!(profiles[1]["generator"], json!("Ninja"));
    assert_eq!(profiles[1]["buildArgs"], json!(["-j", "8"]));
    let file = project_file(&root);
    assert!(file.contains("Ninja"), "{file}");
}

// Settings dialog › finds Gradle JVM and applies project Gradle settings
#[gpui_kit::test]
async fn finds_gradle_jvm_and_applies_project_gradle_settings(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = project(cx, &[("build.gradle", "\n")]);
    let jdk = root.join("jdks/21").display().to_string();
    let asked = give_jdk(&jdk, cx);
    let dialog = open_page(&workbench, build_gradle::PAGE, cx);
    search(&dialog, "Gradle JVM", cx);
    let pages = dialog.read_with(cx, |view, cx| view.pages(cx));
    assert!(pages.iter().any(|page| page.trim() == "Gradle"), "{pages:?}");
    assert_eq!(dialog.read_with(cx, |view, cx| view.selected_page(cx)).as_deref(), Some(build_gradle::PAGE));
    assert!(cx.debug_bounds("settings-sdk-languages.java.gradle.jvm").is_some(), "Gradle JVM is drawn");
    settle(cx, |_| asked.borrow().contains(&ToolchainKind::Jdk));
    click_on(cx, "settings-sdk-languages.java.gradle.jvm");
    click_on(cx, "settings-sdk-languages.java.gradle.jvm-choice-1");
    click_on(cx, "settings-select-languages.java.gradle.distribution");
    click_on(cx, "settings-choice-languages.java.gradle.distribution-local");
    let page: Entity<ToolPage> = page_view(&dialog, build_gradle::PAGE, cx);
    let home = tool_page_field(&page, "languages.java.gradle.home", cx);
    assert_eq!(home.read_with(cx, |field, _| field.placeholder().to_owned()), "Gradle installation directory");
    let gradle_home = root.join("gradle").display().to_string();
    type_over(cx, &home, &gradle_home);
    let user_home = tool_page_field(&page, "languages.java.gradle.userHome", cx);
    let cache = root.join("gradle-cache").display().to_string();
    type_over(cx, &user_home, &cache);
    click_on(cx, "settings-apply");
    assert_eq!(effective_value(cx, "languages.java.gradle.jvm"), Some(json!(jdk)));
    assert_eq!(effective_value(cx, "languages.java.gradle.distribution"), Some(json!("local")));
    assert_eq!(effective_value(cx, "languages.java.gradle.home"), Some(json!(gradle_home)));
    assert_eq!(effective_value(cx, "languages.java.gradle.userHome"), Some(json!(cache)));
    let file = project_file(&root);
    assert!(file.contains("gradle-cache") && file.contains("local"), "{file}");
}

// Settings dialog › persists Maven toolchains and profiles in project settings
#[gpui_kit::test]
async fn persists_maven_toolchains_and_profiles_in_project_settings(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = project(cx, &[("pom.xml", "<project/>\n")]);
    let dialog = open_page(&workbench, build_maven::PAGE, cx);
    let page: Entity<ToolPage> = page_view(&dialog, build_maven::PAGE, cx);
    let toolchains = tool_page_field(&page, "languages.java.maven.toolchains", cx);
    type_over(cx, &toolchains, "config files/toolchains.xml");
    let profiles = tool_page_field(&page, "languages.java.maven.activeProfiles", cx);
    type_over(cx, &profiles, "a, b,!c");
    click_on(cx, "settings-apply");
    assert_eq!(effective_value(cx, "languages.java.maven.toolchains"), Some(json!("config files/toolchains.xml")));
    assert_eq!(effective_value(cx, "languages.java.maven.activeProfiles"), Some(json!("a, b,!c")));
    let file = project_file(&root);
    assert!(file.contains("config files/toolchains.xml") && file.contains("a, b,!c"), "{file}");
}

// Settings dialog › finds the Maven active profiles setting
#[gpui_kit::test]
async fn finds_the_maven_active_profiles_setting(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _root, workbench, cx) = project(cx, &[("pom.xml", "<project/>\n")]);
    let dialog = open_page(&workbench, build_maven::PAGE, cx);
    search(&dialog, "Active profiles", cx);
    assert_eq!(dialog.read_with(cx, |view, cx| view.selected_page(cx)).as_deref(), Some(build_maven::PAGE));
    assert!(cx.debug_bounds("settings-text-languages.java.maven.activeProfiles").is_some(), "Active profiles");
}

// Settings dialog › finds the Maven toolchains setting
#[gpui_kit::test]
async fn finds_the_maven_toolchains_setting(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _root, workbench, cx) = project(cx, &[("pom.xml", "<project/>\n")]);
    let dialog = open_page(&workbench, build_maven::PAGE, cx);
    search(&dialog, "Toolchains", cx);
    assert_eq!(dialog.read_with(cx, |view, cx| view.selected_page(cx)).as_deref(), Some(build_maven::PAGE));
    assert!(cx.debug_bounds("settings-text-languages.java.maven.toolchains").is_some(), "Toolchains file");
}

// Settings dialog › enables, saves and clears the custom UI fallback font
#[gpui_kit::test]
async fn enables_saves_and_clears_the_custom_ui_fallback_font(cx: &mut TestAppContext) {
    const KEY: &str = "appearance.uiFontFallback";
    const FALLBACK: &str = "settings-font-ui-fallback";
    const CUSTOM: &str = "settings-toggle-appearance.useCustomFont";
    // The first suggestion of the fallback font's list, drawn while the list is open.
    const SUGGESTION: &str = "settings-font-ui-fallback-0";
    let _home = start(cx);
    // The pickers suggest the bundled families only: no installed font is read.
    cx.update(|cx| {
        let mut fonts = SystemFonts::default();
        fonts.status = FontStatus::Ready;
        cx.set_global(FontCatalog(fonts));
    });
    let (workbench, cx) = window(cx);
    let dialog = open_page(&workbench, appearance::PAGE, cx);
    bring_into_view(cx, CUSTOM);
    bring_into_view(cx, FALLBACK);
    click_on(cx, FALLBACK);
    assert!(cx.debug_bounds(SUGGESTION).is_none(), "the fallback font cannot be chosen while the custom font is off");
    click_on(cx, CUSTOM);
    click_on(cx, FALLBACK);
    assert!(cx.debug_bounds(SUGGESTION).is_some(), "the fallback font can be chosen once it is on");
    cx.simulate_keystrokes("cmd-a");
    cx.simulate_input("Songti SC");
    // Escape closes the suggestions the typing opened.
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(is_modified(&dialog, cx));
    assert_eq!(global_value(cx, KEY), Some(json!("")), "nothing is written before Apply");
    click_on(cx, "settings-apply");
    assert_eq!(global_value(cx, KEY), Some(json!("Songti SC")));
    click_on(cx, FALLBACK);
    cx.simulate_keystrokes("cmd-a backspace escape");
    cx.run_until_parked();
    click_on(cx, "settings-apply");
    assert_eq!(global_value(cx, KEY), Some(json!("")));
    click_on(cx, CUSTOM);
    click_on(cx, FALLBACK);
    assert!(cx.debug_bounds(SUGGESTION).is_none(), "the fallback font cannot be chosen once the custom font is off");
    click_on(cx, "settings-apply");
    assert_eq!(global_value(cx, "appearance.useCustomFont"), Some(json!(false)));
    assert_eq!(global_value(cx, KEY), Some(json!("")));
}

// Main menu appearance setting › offers both styles on a frameless %s title bar
#[cfg(any(target_os = "windows", target_os = "linux"))]
#[gpui_kit::test]
async fn offers_both_main_menu_styles_on_a_frameless_title_bar(cx: &mut TestAppContext) {
    const KEY: &str = "appearance.mainMenuStyle";
    let _home = start(cx);
    let (workbench, cx) = window(cx);
    let dialog = open_page(&workbench, appearance::PAGE, cx);
    let form: Entity<FormView> = custom(&dialog, appearance::PAGE, "Main menu", cx);
    assert_eq!(form.read_with(cx, |form, cx| form.lines(cx)), ["Main menu: Hamburger button ▾"]);
    bring_into_view(cx, "settings-select-appearance.mainMenuStyle");
    click_on(cx, "settings-select-appearance.mainMenuStyle");
    click_on(cx, "settings-choice-appearance.mainMenuStyle-merged");
    assert_eq!(form.read_with(cx, |form, cx| form.lines(cx)), ["Main menu: Merged with main toolbar ▾"]);
    assert_eq!(staged_value(&dialog, Scope::Global, KEY, cx), Some(json!("merged")));
}

// Main menu appearance setting › hides the setting without the frameless menu (%s)
#[cfg(not(any(target_os = "windows", target_os = "linux")))]
#[gpui_kit::test]
async fn hides_the_main_menu_setting_without_the_frameless_menu(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (workbench, cx) = window(cx);
    let dialog = open_page(&workbench, appearance::PAGE, cx);
    assert!(!appearance::frameless_title_bar());
    let _empty: Entity<crate::settings_view::pages::StackView> = custom(&dialog, appearance::PAGE, "Main menu", cx);
    assert!(cx.debug_bounds("settings-select-appearance.mainMenuStyle").is_none(), "no Main menu choice is drawn");
    assert!(cx.debug_bounds("settings-select-appearance.zoom").is_some(), "the page's other choices are");
}

// toolchain settings with a project open › keeps the global shared JDT LS preference separate from
// the project's override
#[gpui_kit::test]
async fn keeps_the_global_shared_jdt_ls_preference_separate_from_the_projects_override(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = project(cx, &[("A.java", "class A {}\n")]);
    cx.update(|_, cx| set_global("w1", &[SettingsWrite { key: JDTLS_SHARED, value: json!(true) }], cx))
        .expect("the global layer is written");
    store_in_project(JDTLS_SHARED, json!(false), cx);
    let file = project_file(&root);
    let view = language_page(&workbench, "languages.java", cx);
    let shown = language_lines(&view, cx);
    assert!(shown.contains(&"[ ] Share JDT LS across builds".to_owned()), "the project's value: {shown:?}");
    click_on(cx, "settings-scope-global");
    let shown = language_lines(&view, cx);
    assert!(shown.contains(&"[x] Share JDT LS across builds".to_owned()), "the global value: {shown:?}");
    click_on(cx, SHARED_TOGGLE);
    click_on(cx, "settings-apply");
    assert_eq!(global_value(cx, JDTLS_SHARED), Some(json!(false)));
    assert_eq!(effective_value(cx, JDTLS_SHARED), Some(json!(false)));
    assert_eq!(project_file(&root), file, "the project's layer is not written");
}

// toolchain settings with a project open › disables sharing for a Java language server target
// only in project scope
#[gpui_kit::test]
async fn disables_sharing_for_a_java_language_server_target_only_in_project_scope(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = project(cx, &[("A.java", "class A {}\n")]);
    store_in_project("languageServerTargets", json!({ "java": { "target": "docker:java", "server": "jdtls" } }), cx);
    let file = project_file(&root);
    let view = language_page(&workbench, "languages.java", cx);
    let shown = language_lines(&view, cx);
    assert!(shown.contains(&"[ ] Share JDT LS across builds (disabled)".to_owned()), "{shown:?}");
    assert!(shown.iter().any(|line| line.contains("Select Local machine for the Java language server")), "{shown:?}");
    click_on(cx, SHARED_TOGGLE);
    assert!(!modified_of(&workbench, cx), "the checkbox takes no click in the project");
    click_on(cx, "settings-scope-global");
    let shown = language_lines(&view, cx);
    assert!(shown.contains(&"[ ] Share JDT LS across builds".to_owned()), "{shown:?}");
    click_on(cx, SHARED_TOGGLE);
    click_on(cx, "settings-apply");
    assert_eq!(global_value(cx, JDTLS_SHARED), Some(json!(true)));
    assert_eq!(project_file(&root), file, "the project's layer is not written");
}

fn modified_of(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    let dialog = settings_dialog_of(workbench, cx).expect("Settings is open");
    is_modified(&dialog, cx)
}

// toolchain settings with a project open › keeps sharing available when Java runs locally: %j
#[gpui_kit::test]
async fn keeps_sharing_available_when_java_runs_locally(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _root, workbench, cx) = project(cx, &[("A.java", "class A {}\n")]);
    let stored = [
        json!({ "java": { "target": "", "server": "jdtls" } }),
        json!({ "python": { "target": "docker:python", "server": "" } }),
    ];
    for targets in stored {
        store_in_project("languageServerTargets", targets.clone(), cx);
        let view = language_page(&workbench, "languages.java", cx);
        let shown = language_lines(&view, cx);
        assert!(shown.contains(&"[ ] Share JDT LS across builds".to_owned()), "{targets}: {shown:?}");
        click_on(cx, "settings-cancel");
    }
}

// toolchain settings with a project open › uses a staged project Java language server target
// before Apply
#[gpui_kit::test]
async fn uses_a_staged_project_java_language_server_target_before_apply(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = project(cx, &[("A.java", "class A {}\n")]);
    let view = language_page(&workbench, "languages.java", cx);
    let shown = language_lines(&view, cx);
    assert!(shown.contains(&"[ ] Share JDT LS across builds".to_owned()), "{shown:?}");
    let dialog = settings_dialog_of(&workbench, cx).expect("Settings is open");
    let model = dialog.read_with(cx, |view, _| view.model().clone());
    model.update(cx, |dialog, cx| {
        let key = "languageServerTargets.java.target";
        dialog.stage("languages.serverTargets", Scope::Project, key, json!("docker:java"));
        cx.notify();
    });
    cx.run_until_parked();
    let shown = language_lines(&view, cx);
    assert!(shown.contains(&"[ ] Share JDT LS across builds (disabled)".to_owned()), "{shown:?}");
    assert!(!project_file(&root).contains("docker:java"), "nothing is written before Apply");
}

// Settings dialog › keeps object settings stable when child keys are staged (HTTP Proxy)
#[gpui_kit::test]
async fn keeps_object_settings_stable_when_child_keys_are_staged(cx: &mut TestAppContext) {
    const NONE: &str = "(•) No proxy ( ) Auto-detect proxy settings ( ) Manual proxy configuration";
    const AUTO: &str = "( ) No proxy (•) Auto-detect proxy settings ( ) Manual proxy configuration";
    const MANUAL: &str = "( ) No proxy ( ) Auto-detect proxy settings (•) Manual proxy configuration";
    let _home = start(cx);
    let (workbench, cx) = window(cx);
    let dialog = open_page(&workbench, general_http_proxy::PAGE, cx);
    let view: Entity<general_http_proxy::HttpProxyView> =
        custom(&dialog, general_http_proxy::PAGE, general_http_proxy::SLOT, cx);
    let first = |cx: &mut VisualTestContext| view.read_with(cx, |view, cx| view.lines(cx)).remove(0);
    assert_eq!(first(cx), AUTO);
    let model = dialog.read_with(cx, |view, _| view.model().clone());
    model.update(cx, |dialog, cx| {
        dialog.stage(general_http_proxy::PAGE, Scope::Global, "proxy.mode", json!("manual"));
        dialog.stage(general_http_proxy::PAGE, Scope::Global, "proxy.host", json!("127.0.0.1"));
        cx.notify();
    });
    cx.run_until_parked();
    let proxy = staged_value(&dialog, Scope::Global, "proxy", cx).unwrap_or(Value::Null);
    assert_eq!((&proxy["mode"], &proxy["host"]), (&json!("manual"), &json!("127.0.0.1")), "{proxy}");
    assert_eq!(first(cx), MANUAL);
    click_on(cx, "settings-reset");
    assert_eq!(first(cx), AUTO);

    click_on(cx, "settings-radio-proxy.mode-manual");
    assert_eq!(first(cx), MANUAL);
    type_into(cx, "settings-text-proxy.host", "127.0.0.1");
    let lines = view.read_with(cx, |view, cx| view.lines(cx));
    assert!(lines.contains(&"Host name: 127.0.0.1".to_owned()), "{lines:?}");
    click_on(cx, "settings-radio-proxy.mode-none");
    assert_eq!(first(cx), NONE);
    let proxy = staged_value(&dialog, Scope::Global, "proxy", cx).unwrap_or(Value::Null);
    assert_eq!((&proxy["mode"], &proxy["host"]), (&json!("none"), &json!("127.0.0.1")), "{proxy}");
}
