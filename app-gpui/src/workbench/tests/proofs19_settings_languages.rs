//! Languages & Frameworks and Python Interpreter, visited through File › Settings… as a user opens
//! them: every control of the page with its label and first value, one control changed with the
//! mouse or the keyboard, applied, and read back from the settings in effect (and from the
//! project's settings file for the pages a project's values show on first). The installations
//! offered are the ones the test gives: no toolchain is probed, no tool looked up on the PATH.

use super::proofs19_settings_misc::bring_into_view;
use super::*;
use crate::settings_view::pages::languages_controls::LanguagePageView;
use crate::settings_view::pages::languages_sdk::{PluginFormatter, SdkDetection, Toolchain, ToolchainKind};
use gpui_kit::Focusable;
use serde_json::{Value, json};

/// An installation found at `path`.
fn toolchain(path: &std::path::Path, name: &str, version: Option<&str>) -> Toolchain {
    Toolchain {
        path: path.to_string_lossy().into_owned(),
        name: name.to_owned(),
        version: version.map(str::to_owned),
        source: "path".to_owned(),
        home: None,
        extra: None,
    }
}

/// The answers the test build gives in place of the backend's toolchain probes.
fn give_toolchains(cx: &mut VisualTestContext, test: SdkDetection) {
    cx.update(|_, cx| cx.set_global(test));
}

/// Opens Settings on `page` through the registry, as File › Settings… does, and returns the page's
/// own view.
fn open_page(cx: &mut VisualTestContext, workbench: &Entity<Workbench>, page: &str) -> Entity<LanguagePageView> {
    run_action(cx, "file.settings", Some(Value::from(page)));
    let dialog = settings_dialog_of(workbench, cx).expect("Settings opens");
    assert_eq!(dialog.read_with(cx, |view, cx| view.selected_page(cx)).as_deref(), Some(page));
    assert_eq!(dialog.read_with(cx, |view, cx| view.control_lines(cx)), ["<page view>"], "{page} draws its own view");
    let view = dialog.read_with(cx, |view, _| view.page_view(page).cloned());
    let view = view.and_then(|view| view.downcast::<LanguagePageView>().ok()).expect("the page's view");
    cx.run_until_parked();
    view
}

/// The page as text.
fn lines(view: &Entity<LanguagePageView>, cx: &mut VisualTestContext) -> Vec<String> {
    view.read_with(cx, |view, cx| view.lines(cx))
}

/// The page as text without its help lines.
fn labels(view: &Entity<LanguagePageView>, cx: &mut VisualTestContext) -> Vec<String> {
    lines(view, cx).into_iter().filter(|line| !line.starts_with("    ")).collect()
}

fn effective(cx: &mut VisualTestContext, key: &str) -> Option<Value> {
    cx.update(|_, cx| setting_of(&crate::platform::settings::effective("w1", cx), key))
}

fn modified(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    let dialog = settings_dialog_of(workbench, cx).expect("Settings is open");
    dialog.read_with(cx, |view, cx| view.is_modified(cx))
}

/// Clicks the field and types `text` over its text.
fn type_into(cx: &mut VisualTestContext, field: &'static str, text: &str) {
    click_on(cx, field);
    cx.simulate_keystrokes("cmd-a");
    cx.simulate_input(text);
    cx.run_until_parked();
}

/// Opens the drop-down `list` and clicks its choice `choice`.
fn choose(cx: &mut VisualTestContext, list: &'static str, choice: &'static str) {
    click_on(cx, list);
    click_on(cx, choice);
}

/// The project's settings file.
fn project_file(root: &std::path::Path) -> String {
    std::fs::read_to_string(root.join(".openidea/settings.toml")).unwrap_or_default()
}

/// A window without a project.
fn window(cx: &mut TestAppContext) -> (Entity<Workbench>, &mut VisualTestContext) {
    let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
    cx.update(|window, _| window.activate_window());
    (workbench, cx)
}

/// Answers the folder chooser the last Browse… opened with `path`.
fn answer_browse(cx: &mut VisualTestContext, path: &std::path::Path) {
    wait_long(cx, "the folder chooser opens", |cx| cx.did_prompt_for_paths());
    let chosen = path.to_path_buf();
    cx.simulate_path_prompt_response(move |_| Some(vec![chosen]));
    cx.run_until_parked();
}

// Go: GOROOT offers the Go found, Detect again looks again, and the GOPATH chosen with Browse… is
// written to the project with the GOROOT chosen.
#[gpui_kit::test]
async fn go_goroot_and_gopath_are_written_to_the_project(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("main.go", "package main\n")]);
    let goroot = root.join("sdk/go");
    let gopath = root.join("gopath");
    std::fs::create_dir_all(&gopath).expect("the folder is made");
    let mut go = toolchain(&goroot, "go1.22.5", Some("1.22.5"));
    go.extra = Some(root.join("default-gopath").to_string_lossy().into_owned());
    let test = SdkDetection { found: vec![(ToolchainKind::Go, vec![go])], ..SdkDetection::default() };
    let asked = test.asked.clone();
    give_toolchains(cx, test);
    let view = open_page(cx, &workbench, "languages.go");
    assert_eq!(
        lines(&view, cx),
        [
            "— GOROOT —".to_owned(),
            "GOROOT: Default ▾".to_owned(),
            "    Go SDK used by gopls and the go run / test configurations.".to_owned(),
            "— GOPATH —".to_owned(),
            format!("Project GOPATH: ({})", root.join("default-gopath").display()),
        ]
    );
    assert_eq!(asked.borrow().as_slice(), [ToolchainKind::Go], "showing the page detects the Go installations once");
    click_on(cx, "settings-sdk-languages.go.goroot-refresh");
    assert_eq!(asked.borrow().len(), 2, "Detect again looks again");
    choose(cx, "settings-sdk-languages.go.goroot", "settings-sdk-languages.go.goroot-choice-1");
    click_on(cx, "settings-browse-languages.go.gopath");
    answer_browse(cx, &gopath);
    click_on(cx, "settings-apply");
    assert_eq!(effective(cx, "languages.go.goroot"), Some(json!(goroot.to_string_lossy())));
    assert_eq!(effective(cx, "languages.go.gopath"), Some(json!(gopath.to_string_lossy())));
    let file = project_file(&root);
    assert!(file.contains("goroot") && file.contains(gopath.to_string_lossy().as_ref()), "{file}");
}

// Go › Build Tags and Go Modules: without a project each page says one is needed.
#[gpui_kit::test]
async fn go_build_tags_and_modules_say_a_project_is_needed_without_one(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (workbench, cx) = window(cx);
    let view = open_page(cx, &workbench, "languages.go.buildTags");
    assert_eq!(lines(&view, cx), ["Open a project to configure Go build tags."]);
    assert!(cx.debug_bounds("settings-project-only").is_some(), "the message is drawn");
    let view = open_page(cx, &workbench, "languages.go.modules");
    assert_eq!(lines(&view, cx), ["Open a project to configure Go modules."]);
}

// Go › Build Tags: in a project the OS chosen and the custom tags typed are written to it.
#[gpui_kit::test]
async fn go_build_tags_are_written_to_the_project(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("main.go", "package main\n")]);
    let view = open_page(cx, &workbench, "languages.go.buildTags");
    assert_eq!(
        lines(&view, cx),
        [
            "Operating System: Automatic ▾",
            "Architecture: Automatic ▾",
            "Go Version: ",
            "Compiler: Automatic ▾",
            "CGO: Automatic ▾",
            "Custom Tags: ",
            "Experiments: ",
        ]
    );
    type_into(cx, "settings-text-languages.go.buildTags.experiments", "loopvar");
    click_on(cx, "settings-reset");
    assert!(!modified(&workbench, cx), "Reset discards the experiments");
    assert_eq!(lines(&view, cx)[6], "Experiments: ");
    choose(cx, "settings-select-languages.go.buildTags.os", "settings-choice-languages.go.buildTags.os-linux");
    type_into(cx, "settings-text-languages.go.buildTags.custom", "integration debug");
    click_on(cx, "settings-apply");
    assert_eq!(effective(cx, "languages.go.buildTags.os"), Some(json!("linux")));
    assert_eq!(effective(cx, "languages.go.buildTags.custom"), Some(json!("integration debug")));
    assert!(project_file(&root).contains("integration debug"));
}

// Go › Go Modules: a variable typed is written to the project; Cancel after turning vendoring off
// writes nothing.
#[gpui_kit::test]
async fn go_modules_variables_are_written_to_the_project_and_cancel_discards(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("go.mod", "module example.test\n")]);
    let view = open_page(cx, &workbench, "languages.go.modules");
    let mut expected = vec!["[x] Enable vendoring support automatically".to_owned()];
    expected.extend(
        ["GOPROXY", "GOPRIVATE", "GOFLAGS", "GONOPROXY", "GONOSUMDB", "GOSUMDB", "GOMODCACHE", "GOTOOLCHAIN", "GOWORK"]
            .map(|name| format!("{name}: ")),
    );
    assert_eq!(lines(&view, cx), expected);
    type_into(cx, "settings-text-languages.go.modules.env.GOPROXY", "https://proxy.example.test");
    click_on(cx, "settings-ok");
    assert_eq!(effective(cx, "languages.go.modules.env.GOPROXY"), Some(json!("https://proxy.example.test")));
    assert!(project_file(&root).contains("https://proxy.example.test"));

    open_page(cx, &workbench, "languages.go.modules");
    click_on(cx, "settings-toggle-languages.go.modules.autoVendor");
    assert!(modified(&workbench, cx));
    click_on(cx, "settings-cancel");
    assert!(settings_dialog_of(&workbench, cx).is_none(), "Cancel closes Settings");
    assert_eq!(effective(cx, "languages.go.modules.autoVendor"), Some(json!(true)), "Cancel writes nothing");
}

// Java: the project JDK, its language level, shared JDT LS, the server's JDK and its JVM options;
// the level and sharing chosen are written to the project; Reset discards typed JVM options.
#[gpui_kit::test]
async fn java_level_and_shared_jdt_ls_apply_and_reset_discards_jvm_options(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("A.java", "class A {}\n")]);
    let jdk = toolchain(&root.join("jdks/21"), "21 (Temurin)", Some("21.0.4"));
    give_toolchains(cx, SdkDetection { found: vec![(ToolchainKind::Jdk, vec![jdk])], ..SdkDetection::default() });
    let view = open_page(cx, &workbench, "languages.java");
    assert_eq!(
        labels(&view, cx),
        [
            "— Project JDK —",
            "SDK: Auto ▾",
            "Language level: SDK default ▾",
            "— Language Server —",
            "[ ] Share JDT LS across builds",
            "JDK: Auto ▾",
            "JVM options: ",
        ]
    );
    let shown = lines(&view, cx);
    let hint = "    Uses one JDT LS process for linked builds in each local ordinary project window. Other project \
                windows use separate processes.";
    assert!(shown.iter().any(|line| line == hint), "{shown:?}");
    type_into(cx, "settings-text-languages.java.jdtlsVmArgs", "-Xmx4g");
    assert!(modified(&workbench, cx));
    click_on(cx, "settings-reset");
    assert!(!modified(&workbench, cx), "Reset discards the JVM options");
    choose(cx, "settings-select-languages.java.languageLevel", "settings-choice-languages.java.languageLevel-21");
    click_on(cx, "settings-toggle-languages.java.jdtlsShared");
    choose(cx, "settings-sdk-languages.java.jdtlsJdk", "settings-sdk-languages.java.jdtlsJdk-choice-1");
    click_on(cx, "settings-apply");
    assert_eq!(effective(cx, "languages.java.languageLevel"), Some(json!("21")));
    assert_eq!(effective(cx, "languages.java.jdtlsShared"), Some(json!(true)));
    assert_eq!(effective(cx, "languages.java.jdtlsJdk"), Some(json!(root.join("jdks/21").to_string_lossy())));
    assert_eq!(effective(cx, "languages.java.jdtlsVmArgs"), Some(json!("")));
    assert!(project_file(&root).contains("jdtlsShared"));
}

// Markdown: the preview size shows the editor's until one is set; one typed and stepped with Up
// applies globally; Reset and Cancel discard an edit.
#[gpui_kit::test]
async fn markdown_preview_size_follows_the_editor_until_set_and_applies(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (workbench, cx) = window(cx);
    let view = open_page(cx, &workbench, "languages.markdown");
    assert_eq!(
        lines(&view, cx),
        [
            "— Preview —",
            "Preview font size: 13 px",
            "    Also used by Markdown cells in Jupyter notebooks. Defaults to the editor font size, independent of \
             the UI font size.",
        ]
    );
    const FIELD: &str = "settings-number-languages.markdown.previewFontSize";
    type_into(cx, FIELD, "20");
    click_on(cx, "settings-reset");
    assert!(!modified(&workbench, cx), "Reset discards the size");
    assert_eq!(labels(&view, cx)[1], "Preview font size: 13 px");
    type_into(cx, FIELD, "16");
    cx.simulate_keystrokes("up");
    cx.run_until_parked();
    click_on(cx, "settings-apply");
    assert_eq!(effective(cx, "languages.markdown.previewFontSize"), Some(json!(17)));
    type_into(cx, FIELD, "30");
    click_on(cx, "settings-cancel");
    assert_eq!(effective(cx, "languages.markdown.previewFontSize"), Some(json!(17)), "Cancel writes nothing");
}

// Node.js: the interpreter, the run target and the package managers found; the manager chosen is
// written to the project.
#[gpui_kit::test]
async fn node_package_manager_is_written_to_the_project(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("package.json", "{}\n")]);
    let pnpm = toolchain(&root.join("bin/pnpm"), "pnpm", Some("9.1.0"));
    let test = SdkDetection { found: vec![(ToolchainKind::PackageManager, vec![pnpm])], ..SdkDetection::default() };
    give_toolchains(cx, test);
    let view = open_page(cx, &workbench, "languages.node");
    let shown = labels(&view, cx);
    assert_eq!(shown[0], "Node interpreter: Default ▾");
    assert_eq!(shown[1], "Run in target: Default run target ▾");
    assert_eq!(shown[2], "Package manager: Auto ▾");
    assert_eq!(shown.len(), 3, "{shown:?}");
    choose(cx, "settings-select-languages.node.packageManager", "settings-choice-languages.node.packageManager-pnpm");
    assert_eq!(labels(&view, cx)[2], "Package manager: pnpm ▾");
    choose(cx, "settings-select-languages.node.runTarget", "settings-choice-languages.node.runTarget-local");
    click_on(cx, "settings-apply");
    assert_eq!(effective(cx, "languages.node.packageManager"), Some(json!("pnpm")));
    assert_eq!(effective(cx, "languages.node.runTarget"), Some(json!("local")));
    assert!(project_file(&root).contains("pnpm"));
}

// Python Interpreter: the project's virtualenv is the empty choice, named with its path; the
// interpreter chosen and the virtualenv activation are written to the project.
#[gpui_kit::test]
async fn python_interpreter_and_activation_are_written_to_the_project(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("main.py", "print(1)\n")]);
    let venv = root.join(".venv/bin/python");
    let other = root.join("pythons/3.13/bin/python3");
    let found = vec![toolchain(&venv, "Python 3.12 (.venv)", Some("3.12.4")), toolchain(&other, "Python 3.13", None)];
    let python = Some(venv.to_string_lossy().into_owned());
    give_toolchains(
        cx,
        SdkDetection { found: vec![(ToolchainKind::Python, found)], python, ..SdkDetection::default() },
    );
    let view = open_page(cx, &workbench, "python.interpreter");
    assert_eq!(
        lines(&view, cx),
        [
            "Python Interpreter: Project virtualenv (auto-detect) ▾".to_owned(),
            format!("  Python 3.12 (.venv) — {}", venv.display()),
            "— Terminal —".to_owned(),
            "[x] Activate virtualenv".to_owned(),
            "    New terminal sessions run the interpreter's virtualenv activate script.".to_owned(),
        ]
    );
    choose(cx, "settings-sdk-languages.python.interpreter", "settings-sdk-languages.python.interpreter-choice-2");
    click_on(cx, "settings-toggle-languages.python.activateVirtualenv");
    click_on(cx, "settings-apply");
    assert_eq!(effective(cx, "languages.python.interpreter"), Some(json!(other.to_string_lossy())));
    assert_eq!(effective(cx, "languages.python.activateVirtualenv"), Some(json!(false)));
    assert!(project_file(&root).contains("activateVirtualenv"));
}

// PHP: the language level, include paths, stubs, PHPStan, the CLI interpreter on the PATH with
// its version and Xdebug; an include path added and a level chosen are written to the project.
#[gpui_kit::test]
async fn php_include_path_and_level_are_written_to_the_project(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("index.php", "<?php\n")]);
    let php = root.join("bin/php");
    let php_text = php.to_string_lossy().into_owned();
    give_toolchains(
        cx,
        SdkDetection {
            found: vec![(ToolchainKind::Php, vec![toolchain(&php, "PHP 8.3.1", Some("8.3.1"))])],
            located: vec![("php", php_text.clone())],
            xdebug: vec![(php_text, Some("3.3.0".to_owned()))],
            ..SdkDetection::default()
        },
    );
    let view = open_page(cx, &workbench, "languages.php");
    assert_eq!(
        labels(&view, cx),
        [
            "PHP language level: Auto ▾",
            "Include paths:",
            "  No include paths",
            "Additional stubs:",
            "  No additional stubs",
            "— PHPStan —",
            "Configuration file: (phpstan.neon, phpstan.neon.dist or phpstan.dist.neon)",
            "— CLI Interpreter —",
            "Run in target: Default run target ▾",
            "PHP interpreter: Default ▾",
            "Version: 8.3.1",
            "Xdebug: 3.3.0",
        ]
    );
    click_on(cx, "settings-list-languages.php.includePaths-add");
    cx.simulate_input("../platform");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    choose(cx, "settings-select-languages.php.languageLevel", "settings-choice-languages.php.languageLevel-8.2");
    // The interpreter's Xdebug, last on the page, is reached by scrolling it.
    bring_into_view(cx, "settings-php-xdebug");
    click_on(cx, "settings-apply");
    assert_eq!(effective(cx, "languages.php.includePaths"), Some(json!(["../platform"])));
    assert_eq!(effective(cx, "languages.php.languageLevel"), Some(json!("8.2")));
    assert!(project_file(&root).contains("../platform"));
}

// Prettier: the path, the provider, run on save and every language it formats; Prettier chosen
// as the provider and YAML turned off are written to the project.
#[gpui_kit::test]
async fn prettier_provider_and_languages_are_written_to_the_project(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("README.md", "# A\n")]);
    let view = open_page(cx, &workbench, "languages.prettier");
    let mut expected: Vec<String> = [
        "— Prettier —",
        "Prettier path: ",
        "Formatting provider: Automatic (language server first) ▾",
        "[ ] Run on save",
        "— Enabled languages —",
    ]
    .map(str::to_owned)
    .to_vec();
    let names = [
        "Markdown",
        "YAML",
        "CSS",
        "SCSS",
        "Less",
        "HTML",
        "GraphQL",
        "JavaScript",
        "JavaScript JSX",
        "TypeScript",
        "TypeScript JSX",
        "JSON",
        "JSON with Comments",
        "Vue",
    ];
    expected.extend(names.map(|name| format!("[x] {name}")));
    assert_eq!(labels(&view, cx), expected);
    choose(cx, "settings-select-languages.prettier.prefer", "settings-choice-languages.prettier.prefer-1");
    click_on(cx, "settings-toggle-languages.prettier.languages-yaml");
    // The last language is below the dialog's buttons until the page is scrolled.
    bring_into_view(cx, "settings-toggle-languages.prettier.languages-vue");
    click_on(cx, "settings-toggle-languages.prettier.languages-vue");
    click_on(cx, "settings-apply");
    assert_eq!(effective(cx, "languages.prettier.prefer"), Some(json!(true)));
    let enabled = effective(cx, "languages.prettier.languages").and_then(|value| value.as_array().cloned());
    let enabled = enabled.unwrap_or_default();
    assert!(!enabled.contains(&json!("yaml")) && enabled.contains(&json!("markdown")), "{enabled:?}");
    assert!(!enabled.contains(&json!("vue")), "{enabled:?}");
    assert!(project_file(&root).contains("prefer"));
}

// oxfmt: its own settings, apart from Prettier's; Run on save is written to the project.
#[gpui_kit::test]
async fn oxfmt_run_on_save_is_written_to_the_project(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("a.ts", "export {}\n")]);
    let view = open_page(cx, &workbench, "languages.oxfmt");
    let shown = labels(&view, cx);
    assert_eq!(
        shown[..4],
        ["— oxfmt —", "oxfmt path: ", "Formatting provider: Automatic (language server first) ▾", "[ ] Run on save"]
    );
    click_on(cx, "settings-toggle-languages.oxfmt.runOnSave");
    click_on(cx, "settings-apply");
    assert_eq!(effective(cx, "languages.oxfmt.runOnSave"), Some(json!(true)));
    assert_eq!(effective(cx, "languages.prettier.runOnSave"), Some(json!(false)));
    assert!(project_file(&root).contains("oxfmt"));
}

// Ruff: the path, the configuration directory chosen with Browse…, the provider, run on save and
// Python; the directory is written to the project.
#[gpui_kit::test]
async fn ruff_configuration_directory_is_chosen_with_browse_and_written_to_the_project(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("config/ruff/ruff.toml", "line-length = 100\n")]);
    let view = open_page(cx, &workbench, "python.ruff");
    assert_eq!(
        labels(&view, cx),
        [
            "— Ruff —",
            "Ruff path: ",
            "Configuration directory: ",
            "Formatting provider: Automatic (language server first) ▾",
            "[ ] Run on save",
            "— Enabled languages —",
            "[x] Python",
        ]
    );
    click_on(cx, "settings-browse-languages.ruff.configurationDirectory");
    answer_browse(cx, &root.join("config/ruff"));
    click_on(cx, "settings-apply");
    let expected = root.join("config/ruff").to_string_lossy().into_owned();
    assert_eq!(effective(cx, "languages.ruff.configurationDirectory"), Some(json!(expected)));
    assert!(project_file(&root).contains("configurationDirectory"));
}

// Ruby: the interpreters found are offered after Auto; the one chosen is written to the project.
#[gpui_kit::test]
async fn ruby_interpreter_is_written_to_the_project(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("app.rb", "puts 1\n")]);
    let ruby = root.join("rubies/3.3.0/bin/ruby");
    let found = vec![(ToolchainKind::Ruby, vec![toolchain(&ruby, "ruby 3.3.0", Some("3.3.0"))])];
    give_toolchains(cx, SdkDetection { found, ..SdkDetection::default() });
    let view = open_page(cx, &workbench, "languages.ruby");
    assert_eq!(labels(&view, cx), ["Ruby interpreter: Auto ▾"]);
    choose(cx, "settings-sdk-languages.ruby.interpreter", "settings-sdk-languages.ruby.interpreter-choice-1");
    assert_eq!(labels(&view, cx), ["Ruby interpreter: ruby 3.3.0 ▾"]);
    click_on(cx, "settings-apply");
    assert_eq!(effective(cx, "languages.ruby.interpreter"), Some(json!(ruby.to_string_lossy())));
    assert!(project_file(&root).contains("rubies"));
}

// Rust: the toolchain chosen names its standard library in the empty field; Clippy and the rustfmt
// arguments are written to the project; Reset and Cancel discard a checkbox edit.
#[gpui_kit::test]
async fn rust_linter_and_rustfmt_arguments_apply_and_reset_and_cancel_discard(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("src/main.rs", "fn main() {}\n")]);
    let bin = root.join("toolchains/stable/bin");
    let mut stable = toolchain(&bin, "stable-aarch64-apple-darwin", Some("1.85.0"));
    stable.extra = Some(root.join("toolchains/stable/lib/rustlib/src/rust/library").to_string_lossy().into_owned());
    give_toolchains(cx, SdkDetection { found: vec![(ToolchainKind::Rust, vec![stable])], ..SdkDetection::default() });
    let view = open_page(cx, &workbench, "languages.rust");
    assert_eq!(
        labels(&view, cx),
        [
            "— Toolchain —",
            "Toolchain location: Default ▾",
            "Standard library: (Detected from the toolchain)",
            "— External Linters —",
            "External tool: Cargo Check ▾",
            "[x] Run external linter on save",
            "— Rustfmt —",
            "Additional arguments: ",
        ]
    );
    click_on(cx, "settings-toggle-languages.rust.checkOnSave");
    click_on(cx, "settings-reset");
    assert!(!modified(&workbench, cx), "Reset discards the checkbox");
    choose(cx, "settings-sdk-languages.rust.toolchain", "settings-sdk-languages.rust.toolchain-choice-1");
    let library = root.join("toolchains/stable/lib/rustlib/src/rust/library");
    assert_eq!(labels(&view, cx)[2], format!("Standard library: ({})", library.display()));
    choose(cx, "settings-select-languages.rust.checkCommand", "settings-choice-languages.rust.checkCommand-clippy");
    type_into(cx, "settings-text-languages.rust.rustfmtArgs", "--edition 2024");
    click_on(cx, "settings-apply");
    assert_eq!(effective(cx, "languages.rust.checkCommand"), Some(json!("clippy")));
    assert_eq!(effective(cx, "languages.rust.rustfmtArgs"), Some(json!("--edition 2024")));
    assert_eq!(effective(cx, "languages.rust.toolchain"), Some(json!(bin.to_string_lossy())));
    let file = project_file(&root);
    assert!(file.contains("clippy"), "{file}");

    click_on(cx, "settings-toggle-languages.rust.checkOnSave");
    click_on(cx, "settings-cancel");
    assert!(settings_dialog_of(&workbench, cx).is_none());
    assert_eq!(effective(cx, "languages.rust.checkOnSave"), Some(json!(true)), "Cancel writes nothing");
    assert_eq!(project_file(&root), file, "Cancel leaves the project's file as it was");
}

// TypeScript: Bundled, the project's node_modules TypeScript, and a lib folder chosen with Browse…,
// which is written to the project and offered as a custom choice.
#[gpui_kit::test]
async fn typescript_version_is_chosen_with_browse_and_written_to_the_project(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("a.ts", "export {}\n")]);
    let lib = root.join("node_modules/typescript/lib");
    let mut workspace = toolchain(&lib, "TypeScript 5.4.5", Some("5.4.5"));
    workspace.source = "workspace".to_owned();
    let custom = root.join("tsdk/lib");
    std::fs::create_dir_all(&custom).expect("the folder is made");
    give_toolchains(
        cx,
        SdkDetection { found: vec![(ToolchainKind::TypeScript, vec![workspace])], ..SdkDetection::default() },
    );
    let view = open_page(cx, &workbench, "languages.typescript");
    assert_eq!(labels(&view, cx), ["TypeScript: Bundled ▾"]);
    choose(cx, "settings-sdk-languages.typescript.version", "settings-sdk-languages.typescript.version-choice-1");
    assert_eq!(labels(&view, cx), ["TypeScript: TypeScript 5.4.5 ▾"]);
    click_on(cx, "settings-sdk-languages.typescript.version-browse");
    answer_browse(cx, &custom);
    assert_eq!(labels(&view, cx), [format!("TypeScript: {} ▾", custom.display())]);
    click_on(cx, "settings-apply");
    assert_eq!(effective(cx, "languages.typescript.version"), Some(json!(custom.to_string_lossy())));
    assert!(project_file(&root).contains("tsdk"));
}

// Not from a spec: the plugin formatter choices are part of Editor › Code Style.
/// Editor › Code Style lists, under its formatter settings, a choice per language a plugin formatter covers:
/// Automatic and the formatters covering it, in the registry's order; the formatter picked for one language is
/// stored under it, and Reset discards a second pick. Languages & Frameworks has no Plugin Formatters page.
#[gpui_kit::test]
async fn code_style_offers_a_plugin_formatter_per_covered_language_and_stores_the_pick(cx: &mut TestAppContext) {
    use crate::settings_view::pages::languages_controls::PluginFormattersView;
    let _home = start(cx);
    let (_folder, _root, workbench, cx) = trusted_project(cx, &[("query.sql", "select 1;\n")]);
    let formatter = |id: &str, name: &str, languages: &[&str]| PluginFormatter {
        id: id.to_owned(),
        name: name.to_owned(),
        languages: languages.iter().map(|language| (*language).to_owned()).collect(),
    };
    let formatters =
        vec![formatter("p.stylua", "StyLua", &["lua"]), formatter("p.taplo", "Taplo and SQL", &["toml", "sql"])];
    give_toolchains(cx, SdkDetection { formatters, ..SdkDetection::default() });
    run_action(cx, "file.settings", Some(Value::from("languages.pluginFormatters")));
    let dialog = settings_dialog_of(&workbench, cx).expect("Settings opens");
    assert_ne!(dialog.read_with(cx, |view, cx| view.selected_page(cx)).as_deref(), Some("languages.pluginFormatters"));
    click_on(cx, "settings-cancel");
    run_action(cx, "file.settings", Some(Value::from("editor.codeStyle")));
    let dialog = settings_dialog_of(&workbench, cx).expect("Settings opens");
    assert_eq!(dialog.read_with(cx, |view, cx| view.selected_page(cx)).as_deref(), Some("editor.codeStyle"));
    let view = dialog.read_with(cx, |view, _| view.custom_control("editor.codeStyle", "Plugin Formatters").cloned());
    let view = view.and_then(|view| view.downcast::<PluginFormattersView>().ok()).expect("the formatters' view");
    cx.run_until_parked();
    let lines = |cx: &mut VisualTestContext| view.read_with(cx, |view, cx| view.lines(cx));
    assert_eq!(
        lines(cx),
        [
            "lua: Automatic (language server first) ▾",
            "sql: Automatic (language server first) ▾",
            "toml: Automatic (language server first) ▾",
        ]
    );
    bring_into_view(cx, "settings-formatter-2");
    choose(cx, "settings-formatter-1", "settings-formatter-1-1");
    assert_eq!(lines(cx)[1], "sql: Taplo and SQL ▾");
    click_on(cx, "settings-apply");
    assert_eq!(effective(cx, "languages.pluginFormatters"), Some(json!({ "sql": "p.taplo" })));

    choose(cx, "settings-formatter-0", "settings-formatter-0-1");
    assert_eq!(lines(cx)[0], "lua: StyLua ▾");
    assert!(modified(&workbench, cx));
    click_on(cx, "settings-reset");
    assert!(!modified(&workbench, cx), "Reset discards the second pick");
    assert_eq!(
        lines(cx),
        [
            "lua: Automatic (language server first) ▾",
            "sql: Taplo and SQL ▾",
            "toml: Automatic (language server first) ▾"
        ]
    );
    assert_eq!(effective(cx, "languages.pluginFormatters"), Some(json!({ "sql": "p.taplo" })));
}

// The search finds a language page by a label of its own controls.
#[gpui_kit::test]
async fn the_search_finds_a_language_page_by_a_label_of_its_controls(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (workbench, cx) = window(cx);
    open_page(cx, &workbench, "languages.markdown");
    let dialog = settings_dialog_of(&workbench, cx).expect("Settings is open");
    let search = dialog.read_with(cx, |view, _| view.search_field().clone());
    cx.update(|window, cx| window.focus(&search.focus_handle(cx), cx));
    cx.simulate_input("Run external linter");
    cx.run_until_parked();
    let pages = dialog.read_with(cx, |view, cx| view.pages(cx));
    assert!(pages.iter().any(|page| page.trim() == "Rust"), "{pages:?}");
}

/// How a page's control is changed.
enum Act {
    /// Clicks each selector in turn (a drop-down and its choice).
    Click(&'static [&'static str]),
    /// Types the text over the field drawn as the selector.
    Type(&'static str, &'static str),
}

// Reset discards an edit, and Cancel after the same edit writes nothing, on every language page.
#[gpui_kit::test]
async fn reset_and_cancel_discard_an_edit_on_each_language_page(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("main.go", "package main\n")]);
    let ruby = toolchain(&root.join("rubies/3.3.0/bin/ruby"), "ruby 3.3.0", Some("3.3.0"));
    give_toolchains(cx, SdkDetection { found: vec![(ToolchainKind::Ruby, vec![ruby])], ..SdkDetection::default() });
    let cases: [(&str, Act, &str); 14] = [
        ("languages.go", Act::Type("settings-text-languages.go.gopath", "/work/go"), "languages.go.gopath"),
        (
            "languages.go.buildTags",
            Act::Type("settings-text-languages.go.buildTags.custom", "integration"),
            "languages.go.buildTags.custom",
        ),
        (
            "languages.go.modules",
            Act::Click(&["settings-toggle-languages.go.modules.autoVendor"]),
            "languages.go.modules.autoVendor",
        ),
        ("languages.java", Act::Click(&["settings-toggle-languages.java.jdtlsShared"]), "languages.java.jdtlsShared"),
        (
            "languages.markdown",
            Act::Type("settings-number-languages.markdown.previewFontSize", "20"),
            "languages.markdown.previewFontSize",
        ),
        (
            "languages.node",
            Act::Click(&["settings-select-languages.node.runTarget", "settings-choice-languages.node.runTarget-local"]),
            "languages.node.runTarget",
        ),
        (
            "python.interpreter",
            Act::Click(&["settings-toggle-languages.python.activateVirtualenv"]),
            "languages.python.activateVirtualenv",
        ),
        (
            "languages.php",
            Act::Click(&[
                "settings-select-languages.php.languageLevel",
                "settings-choice-languages.php.languageLevel-8.2",
            ]),
            "languages.php.languageLevel",
        ),
        (
            "languages.prettier",
            Act::Click(&["settings-toggle-languages.prettier.runOnSave"]),
            "languages.prettier.runOnSave",
        ),
        ("languages.oxfmt", Act::Click(&["settings-toggle-languages.oxfmt.runOnSave"]), "languages.oxfmt.runOnSave"),
        ("python.ruff", Act::Click(&["settings-toggle-languages.ruff.runOnSave"]), "languages.ruff.runOnSave"),
        (
            "languages.ruby",
            Act::Click(&[
                "settings-sdk-languages.ruby.interpreter",
                "settings-sdk-languages.ruby.interpreter-choice-1",
            ]),
            "languages.ruby.interpreter",
        ),
        ("languages.rust", Act::Click(&["settings-toggle-languages.rust.checkOnSave"]), "languages.rust.checkOnSave"),
        (
            "languages.typescript",
            Act::Click(&[
                "settings-sdk-languages.typescript.version",
                "settings-sdk-languages.typescript.version-choice-1",
            ]),
            "languages.typescript.version",
        ),
    ];
    for (page, act, key) in cases {
        let before = effective(cx, key);
        open_page(cx, &workbench, page);
        let run = |cx: &mut VisualTestContext| match &act {
            Act::Click(at) => at.iter().for_each(|at| click_on(cx, at)),
            Act::Type(at, text) => type_into(cx, at, text),
        };
        run(cx);
        assert!(modified(&workbench, cx), "{page}: the edit is staged");
        click_on(cx, "settings-reset");
        assert!(!modified(&workbench, cx), "{page}: Reset discards it");
        run(cx);
        click_on(cx, "settings-cancel");
        assert!(settings_dialog_of(&workbench, cx).is_none(), "{page}: Cancel closes the dialog");
        assert_eq!(effective(cx, key), before, "{page}: Cancel writes nothing");
    }
}

// src/features/settings/__tests__/languages.test.ts › languages settings › puts the Python pages in a Python
// folder, in PyCharm's order
/// File › Settings… opened on the Python folder shows no page body of its own but its description and links to
/// the pages below it (those built here: Dependencies, then Ruff); a link opens its page. The Agents folder sits
/// after Languages & Frameworks and links to Coding Agents, Chat and IDE MCP Server.
#[gpui_kit::test]
async fn puts_the_python_pages_in_a_python_folder_in_pycharms_order(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _root, workbench, cx) = trusted_project(cx, &[("main.py", "print(1)\n")]);
    run_action(cx, "file.settings", Some(Value::from("python")));
    let dialog = settings_dialog_of(&workbench, cx).expect("Settings opens");
    assert_eq!(dialog.read_with(cx, |view, cx| view.selected_page(cx)).as_deref(), Some("python"));
    assert_eq!(
        dialog.read_with(cx, |view, cx| view.control_lines(cx)),
        [
            "Configure the project interpreter and its packages, every interpreter the IDE knows, and Ruff.",
            "→ Dependencies",
            "→ Ruff",
        ]
    );
    let pages = dialog.read_with(cx, |view, cx| view.pages(cx));
    let at = |title: &str| pages.iter().position(|page| page == title).unwrap_or_else(|| panic!("{title}: {pages:?}"));
    assert!(at("Languages & Frameworks") < at("Agents") && at("Agents") < at("Tools"), "{pages:?}");
    click_on(cx, "settings-link-python.ruff");
    assert_eq!(dialog.read_with(cx, |view, cx| view.selected_page(cx)).as_deref(), Some("python.ruff"));

    click_on(cx, "settings-cancel");
    run_action(cx, "file.settings", Some(Value::from("group:Agents")));
    let dialog = settings_dialog_of(&workbench, cx).expect("Settings opens again");
    assert_eq!(dialog.read_with(cx, |view, cx| view.selected_page(cx)).as_deref(), Some("group:Agents"));
    assert_eq!(
        dialog.read_with(cx, |view, cx| view.control_lines(cx)),
        [
            "Install coding agents and configure how they launch, chat and connect to the IDE.",
            "→ Coding Agents",
            "→ Chat",
            "→ IDE MCP Server",
        ]
    );
    click_on(cx, "settings-cancel");
}
