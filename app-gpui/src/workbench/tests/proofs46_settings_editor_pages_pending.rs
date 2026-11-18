//! Editor pages of File › Settings… on the real backend, under a temporary home and project:
//! Natural Languages, Spelling, Font's preview, Code Style's tabs, Other File Types, File Encodings
//! and Code Folding. Each page is opened as a user opens it, its controls are used with the mouse and
//! the keyboard, Apply or OK writes the settings files, Reset and Cancel discard an edit, and the
//! effect is read where an editor or the disk shows one.

use super::proofs19_settings_misc::bring_into_view;
use super::*;
use crate::editor::view::Editor;
use crate::platform::settings::{effective, global};
use crate::settings_view::dialog::SettingsDialogView;
use crate::settings_view::pages::FormView;
use crate::settings_view::pages::code_style::SchemeView;
use crate::settings_view::pages::editor_code_style_tabs_pending::{CodeStyleTab, CodeStyleTabsView};
use crate::settings_view::pages::editor_font_preview_pending::{FontPreviewView, SAMPLE};
use crate::settings_view::pages::editor_natural_languages_pending::NaturalLanguagesView;
use crate::settings_view::pages::editor_spelling_pending::SpellingView;
use crate::settings_view::pages::font_picker::FontCatalog;
use crate::theme::fonts::{FontStatus, SystemFonts};
use crate::theme::installed_fonts::InstalledFontNames;
use gpui_kit::{AnyView, Focusable};
use serde_json::{Value, json};
use std::path::Path;

fn window(cx: &mut TestAppContext) -> (Entity<Workbench>, &mut VisualTestContext) {
    let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
    cx.update(|window, _| window.activate_window());
    (workbench, cx)
}

/// A trusted project of `files`, open in an active window.
fn project<'a>(
    cx: &'a mut TestAppContext,
    files: &[(&str, &str)],
) -> (tempfile::TempDir, std::path::PathBuf, Entity<Workbench>, &'a mut VisualTestContext) {
    let (folder, root, workbench, cx) = trusted_project(cx, files);
    cx.update(|window, _| window.activate_window());
    (folder, root, workbench, cx)
}

/// The project's own settings file; empty while there is none.
fn project_file(root: &Path) -> String {
    std::fs::read_to_string(root.join(".openidea/settings.toml")).unwrap_or_default()
}

/// File › Settings… on `page`; no control of the page says it is not available.
fn open_page(workbench: &Entity<Workbench>, page: &str, cx: &mut VisualTestContext) -> Entity<SettingsDialogView> {
    run_action(cx, "file.settings", Some(Value::from(page)));
    let dialog = settings_dialog_of(workbench, cx).expect("Settings opens");
    assert_eq!(dialog.read_with(cx, |view, cx| view.selected_page(cx)).as_deref(), Some(page));
    let lines = dialog.read_with(cx, |view, cx| view.control_lines(cx));
    let unavailable = crate::settings_view::controls::UNAVAILABLE;
    assert!(!lines.iter().any(|line| line.ends_with(unavailable)), "{page} draws every control: {lines:?}");
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

fn effective_value(cx: &mut VisualTestContext, key: &str) -> Option<Value> {
    cx.update(|_, cx| setting_of(&effective("w1", cx), key))
}

fn global_value(cx: &mut VisualTestContext, key: &str) -> Option<Value> {
    cx.update(|_, cx| setting_of(&global(cx), key))
}

/// A number setting of the global settings, whether it is stored whole or with a fraction.
fn global_number(cx: &mut VisualTestContext, key: &str) -> Option<f64> {
    global_value(cx, key).and_then(|value| value.as_f64())
}

fn is_modified(dialog: &Entity<SettingsDialogView>, cx: &mut VisualTestContext) -> bool {
    dialog.read_with(cx, |view, cx| view.is_modified(cx))
}

/// Clicks the field drawn as `at` and types `text` over its text.
fn type_into(cx: &mut VisualTestContext, at: &'static str, text: &str) {
    bring_into_view(cx, at);
    click_on(cx, at);
    cx.simulate_keystrokes("cmd-a");
    cx.simulate_input(text);
    cx.run_until_parked();
}

/// Clicks + of the list of `key`, types `text` into the new row and presses Enter.
fn add_row(cx: &mut VisualTestContext, key: &'static str, text: &str) {
    let add: &'static str = Box::leak(format!("settings-list-{key}-add").into_boxed_str());
    bring_into_view(cx, add);
    click_on(cx, add);
    cx.simulate_input(text);
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
}

/// Opens `file` of the project and waits for its editor.
fn open_file(workbench: &Entity<Workbench>, file: &Path, cx: &mut VisualTestContext) -> Entity<Editor> {
    let wanted = file.to_path_buf();
    workbench.update_in(cx, |w, window, cx| w.open_project_file(wanted.clone(), window, cx));
    let wanted = file.to_path_buf();
    wait_long(cx, "the file's editor", |cx| {
        workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).path() == wanted.as_path()))
    });
    workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor")
}

/// Puts the caret at the start of the editor's text, gives it the keyboard and presses Tab.
fn press_tab_at_start(editor: &Entity<Editor>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| editor.select(0, 0, cx));
        window.focus(&editor.focus_handle(cx), cx);
    });
    cx.simulate_keystrokes("tab");
    cx.run_until_parked();
}

// ---------------------------------------------------------------------------------------------
// Natural Languages

// Settings › Editor › Natural Languages shows the bundled English dictionaries used by Proofreading ›
// Typo, and stores nothing
#[gpui_kit::test]
async fn natural_languages_shows_the_bundled_english_dictionaries_used_by_typo(cx: &mut TestAppContext) {
    let home = start(cx);
    let (workbench, cx) = window(cx);
    let dialog = open_page(&workbench, "editor.naturalLanguages", cx);
    let view: Entity<NaturalLanguagesView> = page_view(&dialog, "editor.naturalLanguages", cx);
    assert_eq!(
        view.read_with(cx, |view, cx| view.lines(cx)),
        [
            "— Languages —",
            "English (USA, UK): Used by Proofreading › Typo",
            "    Bundled Hunspell dictionaries (SCOWL en_US and en_GB) and common programming terms.",
        ]
    );
    assert!(cx.debug_bounds("settings-natural-language-en").is_some(), "the language's row is drawn");
    click_on(cx, "settings-natural-language-en");
    assert!(!is_modified(&dialog, cx), "nothing on the page is a setting");
    click_on(cx, "settings-ok");
    assert!(settings_dialog_of(&workbench, cx).is_none());
    assert!(!global_settings_file(&home).contains("naturalLanguages"));
}

// ---------------------------------------------------------------------------------------------
// Spelling

// Settings › Editor › Natural Languages › Spelling: a word added to the project dictionary is stored
// with the project, one added to the application dictionary and a dictionary file in the global
// settings, each trimmed and once
#[gpui_kit::test]
async fn spelling_stores_the_project_and_application_words_and_the_dictionary_files_on_apply(cx: &mut TestAppContext) {
    let home = start(cx);
    let (_folder, root, workbench, cx) = project(cx, &[("notes.txt", "Teh qwzxv word\n")]);
    let dialog = open_page(&workbench, "editor.naturalLanguages.spelling", cx);
    let view: Entity<SpellingView> = page_view(&dialog, "editor.naturalLanguages.spelling", cx);
    assert_eq!(
        view.read_with(cx, |view, cx| view.lines(cx)),
        [
            "— Accepted words —",
            "Project dictionary:",
            "  No words",
            "Application dictionary:",
            "  No words",
            "— Custom dictionaries —",
            "Dictionary files:",
            "  No custom dictionaries",
        ]
    );
    add_row(cx, "spelling.projectWords", "  teh  ");
    add_row(cx, "spelling.ideWords", "qwzxv");
    add_row(cx, "spelling.ideWords", "qwzxv");
    add_row(cx, "spelling.customDictionaries", "words.txt");
    assert!(is_modified(&dialog, cx));
    assert!(!project_file(&root).contains("teh"), "nothing is written before Apply");
    click_on(cx, "settings-apply");
    settle(cx, |_| project_file(&root).contains("teh"));
    let stored = global_settings_file(&home);
    assert!(stored.contains("qwzxv") && stored.contains("words.txt"), "{stored}");
    assert!(!stored.contains("teh"), "the project's words stay with the project: {stored}");
    assert_eq!(effective_value(cx, "spelling.projectWords"), Some(json!(["teh"])));
    assert_eq!(global_value(cx, "spelling.ideWords"), Some(json!(["qwzxv"])));
    assert_eq!(global_value(cx, "spelling.customDictionaries"), Some(json!(["words.txt"])));

    // A word edited to nothing goes; OK writes it and closes the dialog.
    let application = view.read_with(cx, |view, _| view.application().clone());
    click_on(cx, "settings-list-spelling.ideWords-row-0");
    cx.simulate_keystrokes("delete");
    cx.run_until_parked();
    assert_eq!(application.read_with(cx, |list, cx| list.rows(cx)).len(), 0);
    click_on(cx, "settings-ok");
    settle(cx, |cx| settings_dialog_of(&workbench, cx).is_none());
    assert_eq!(global_value(cx, "spelling.ideWords"), Some(json!([])));
}

// Settings › Spelling: Reset discards a word added on the page and Cancel after the same edit
// writes nothing
#[gpui_kit::test]
async fn spelling_reset_and_cancel_discard_an_added_word(cx: &mut TestAppContext) {
    let home = start(cx);
    let (workbench, cx) = window(cx);
    let dialog = open_page(&workbench, "editor.naturalLanguages.spelling", cx);
    let view: Entity<SpellingView> = page_view(&dialog, "editor.naturalLanguages.spelling", cx);
    assert!(view.read_with(cx, |view, _| view.project().is_none()), "no project: no project dictionary");
    add_row(cx, "spelling.ideWords", "kubectl");
    assert!(is_modified(&dialog, cx));
    click_on(cx, "settings-reset");
    assert!(!is_modified(&dialog, cx));
    let lines = view.read_with(cx, |view, cx| view.lines(cx));
    assert_eq!(lines[1..3], ["Application dictionary:", "  No words"]);
    add_row(cx, "spelling.ideWords", "kubectl");
    click_on(cx, "settings-cancel");
    assert!(settings_dialog_of(&workbench, cx).is_none(), "Cancel closes the dialog");
    assert_eq!(global_value(cx, "spelling.ideWords"), Some(json!([])));
    assert!(!global_settings_file(&home).contains("kubectl"));
}

// ---------------------------------------------------------------------------------------------
// Font

/// The fonts the pickers and the editor know: given here, never read from the machine.
fn given_fonts(cx: &mut VisualTestContext) {
    cx.update(|_, cx| {
        let mut catalog = SystemFonts::default();
        catalog.status = FontStatus::Ready;
        cx.set_global(FontCatalog(catalog));
        cx.set_global(InstalledFontNames(vec!["Test Mono".to_owned(), "Test Fallback".to_owned()]));
    });
}

// Settings › Editor › Font: the Preview follows the family, size, line height, ligatures and weight
// being edited before Apply; its text can be typed into and is never stored; Cancel and Reset bring
// the stored font back
#[gpui_kit::test]
async fn font_preview_follows_the_edits_before_apply_and_its_text_is_not_stored(cx: &mut TestAppContext) {
    let home = start(cx);
    let (workbench, cx) = window(cx);
    given_fonts(cx);
    let dialog = open_page(&workbench, "editor.font", cx);
    let preview: Entity<FontPreviewView> = custom(&dialog, "editor.font", "Preview", cx);
    assert_eq!(preview.read_with(cx, |view, cx| view.text(cx)), SAMPLE);
    let font = |cx: &mut VisualTestContext| preview.read_with(cx, |view, cx| view.font(cx)).expect("a font");
    assert_eq!(f32::from(font(cx).size), 13.0);

    type_into(cx, "settings-font-editor", "Test Mono");
    cx.simulate_keystrokes("enter");
    type_into(cx, "settings-number-editor.fontSize", "15");
    type_into(cx, "settings-number-editor.lineHeight", "24");
    bring_into_view(cx, "settings-toggle-editor.ligatures");
    click_on(cx, "settings-toggle-editor.ligatures");
    bring_into_view(cx, "settings-select-editor.fontWeight");
    click_on(cx, "settings-select-editor.fontWeight");
    click_on(cx, "settings-choice-editor.fontWeight-500");
    assert_eq!(global_number(cx, "editor.fontSize"), Some(13.0), "nothing is applied yet");
    let shown = font(cx);
    assert_eq!(shown.font.family.as_ref(), "Test Mono");
    assert_eq!((f32::from(shown.size), f32::from(shown.line_height)), (15.0, 24.0));
    assert_ne!(shown.font.features.is_calt_enabled(), Some(false), "ligatures are on");
    assert_eq!(shown.font.weight, gpui_kit::FontWeight(500.0));

    // The sample takes what is typed, Enter included, and none of it is a setting.
    bring_into_view(cx, "settings-font-preview");
    click_on(cx, "settings-font-preview");
    let area = preview.read_with(cx, |view, _| view.area().clone());
    assert!(cx.update(|window, cx| area.focus_handle(cx).is_focused(window)), "the sample has the keyboard");
    cx.simulate_keystrokes("cmd-a");
    cx.simulate_input("fi -> != ===");
    cx.simulate_keystrokes("enter");
    cx.simulate_input("typed");
    cx.run_until_parked();
    assert!(settings_dialog_of(&workbench, cx).is_some(), "Enter in the sample is not OK");
    assert_eq!(preview.read_with(cx, |view, cx| view.text(cx)), "fi -> != ===\ntyped");
    click_on(cx, "settings-apply");
    settle(cx, |cx| global_number(cx, "editor.fontSize") == Some(15.0));
    let stored = global_settings_file(&home);
    assert!(!stored.contains("typed") && !stored.contains("full-featured"), "{stored}");

    // Reset: the preview shows the stored font again; Cancel after an edit writes nothing.
    type_into(cx, "settings-number-editor.fontSize", "20");
    assert_eq!(f32::from(font(cx).size), 20.0);
    click_on(cx, "settings-reset");
    assert_eq!(f32::from(font(cx).size), 15.0);
    type_into(cx, "settings-number-editor.fontSize", "20");
    click_on(cx, "settings-cancel");
    assert!(settings_dialog_of(&workbench, cx).is_none());
    assert_eq!(global_number(cx, "editor.fontSize"), Some(15.0));
}

// ---------------------------------------------------------------------------------------------
// Code Style

// Settings › Editor › Code Style shows the scheme above the General and Formatter tabs: General's
// line separator, hard wrap and EditorConfig support and Formatter's Do not format pattern and
// markers apply to the global settings; Reset and Cancel discard an edit
#[gpui_kit::test]
async fn code_style_tabs_switch_and_their_edits_apply_reset_and_cancel(cx: &mut TestAppContext) {
    let home = start(cx);
    let (workbench, cx) = window(cx);
    let dialog = open_page(&workbench, "editor.codeStyle", cx);
    let view: Entity<CodeStyleTabsView> = page_view(&dialog, "editor.codeStyle", cx);
    let lines = view.read_with(cx, |view, cx| view.lines(cx));
    assert_eq!(lines[0], "Scheme: Default (IDE) ▾");
    assert_eq!(lines[2], "[General] Formatter");
    assert!(cx.debug_bounds("settings-list-editor.codeStyle.doNotFormat-add").is_none(), "Formatter is hidden");

    click_on(cx, "settings-select-editor.codeStyle.lineSeparator");
    click_on(cx, "settings-choice-editor.codeStyle.lineSeparator-crlf");
    type_into(cx, "settings-number-editor.codeStyle.hardWrapAt", "100");
    bring_into_view(cx, "settings-toggle-editor.codeStyle.editorconfig");
    click_on(cx, "settings-toggle-editor.codeStyle.editorconfig");

    click_on(cx, "settings-code-style-tab-formatter");
    assert_eq!(view.read_with(cx, |view, _| view.tab()), CodeStyleTab::Formatter);
    add_row(cx, "editor.codeStyle.doNotFormat", "generated/**");
    bring_into_view(cx, "settings-toggle-editor.codeStyle.formatterTagsEnabled");
    click_on(cx, "settings-toggle-editor.codeStyle.formatterTagsEnabled");
    type_into(cx, "settings-text-editor.codeStyle.formatterOffTag", "fmt:off");
    click_on(cx, "settings-apply");
    settle(cx, |cx| global_number(cx, "editor.codeStyle.hardWrapAt") == Some(100.0));
    assert_eq!(global_value(cx, "editor.codeStyle.lineSeparator"), Some(json!("crlf")));
    assert_eq!(global_value(cx, "editor.codeStyle.editorconfig"), Some(json!(false)));
    assert_eq!(global_value(cx, "editor.codeStyle.doNotFormat"), Some(json!(["generated/**"])));
    assert_eq!(global_value(cx, "editor.codeStyle.formatterTagsEnabled"), Some(json!(true)));
    assert_eq!(global_value(cx, "editor.codeStyle.formatterOffTag"), Some(json!("fmt:off")));
    let stored = global_settings_file(&home);
    assert!(stored.contains("hardWrapAt = 100") && stored.contains("generated/**"), "{stored}");

    bring_into_view(cx, "settings-toggle-editor.codeStyle.formatterTagsRegexp");
    click_on(cx, "settings-toggle-editor.codeStyle.formatterTagsRegexp");
    assert!(is_modified(&dialog, cx));
    click_on(cx, "settings-reset");
    assert!(!is_modified(&dialog, cx));
    click_on(cx, "settings-toggle-editor.codeStyle.formatterTagsRegexp");
    click_on(cx, "settings-cancel");
    assert!(settings_dialog_of(&workbench, cx).is_none());
    assert_eq!(global_value(cx, "editor.codeStyle.formatterTagsRegexp"), Some(json!(false)));

    // The page opens on General again.
    let dialog = open_page(&workbench, "editor.codeStyle", cx);
    let view: Entity<CodeStyleTabsView> = page_view(&dialog, "editor.codeStyle", cx);
    assert_eq!(view.read_with(cx, |view, _| view.tab()), CodeStyleTab::General);
}

// Settings › Code Style: Hard wrap at and the visual guides set on the page are the right margins of
// an open editor once applied
#[gpui_kit::test]
async fn code_style_hard_wrap_and_guides_set_on_the_page_are_the_editors_right_margins(cx: &mut TestAppContext) {
    let _home = start(cx);
    let long = format!("{}\n", "x".repeat(200));
    let (_folder, root, workbench, cx) = project(cx, &[("a.txt", long.as_str())]);
    let editor = open_file(&workbench, &root.join("a.txt"), cx);
    assert_eq!(editor.read_with(cx, |e, _| e.facts().right_margins.clone()), [120]);
    open_page(&workbench, "editor.codeStyle", cx);
    type_into(cx, "settings-number-editor.codeStyle.hardWrapAt", "100");
    type_into(cx, "settings-text-editor.codeStyle.visualGuides", "80");
    assert_eq!(editor.read_with(cx, |e, _| e.facts().right_margins.clone()), [120], "nothing before Apply");
    click_on(cx, "settings-ok");
    wait_long(cx, "the new margins", |cx| editor.read_with(cx, |e, _| e.facts().right_margins == [80, 100]));
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    assert_eq!(editor.read_with(cx, |e, _| e.painted_right_margins()).len(), 2);
}

// Settings › Editor › Code Style › Other File Types shows the scheme row; Tab size and Use tab
// character applied there are what Tab inserts in a plain-text file
#[gpui_kit::test]
async fn other_file_types_shows_the_scheme_and_its_indentation_is_what_tab_inserts(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = project(cx, &[("a.txt", "x\n"), ("b.txt", "y\n")]);
    let dialog = open_page(&workbench, "editor.codeStyle.other", cx);
    let scheme: Entity<SchemeView> = custom(&dialog, "editor.codeStyle.other", "Scheme", cx);
    let lines = scheme.read_with(cx, |view, cx| view.lines(cx));
    assert_eq!(lines[0], "Scheme: Default (IDE) ▾ [Copy to Project]");
    let lines = dialog.read_with(cx, |view, cx| view.control_lines(cx));
    assert!(lines.contains(&"Scheme: <custom>".to_owned()), "{lines:?}");
    assert!(lines.contains(&"[ ] Use tab character".to_owned()), "{lines:?}");
    type_into(cx, "settings-number-editor.tabSize", "2");
    click_on(cx, "settings-ok");
    settle(cx, |cx| global_number(cx, "editor.tabSize") == Some(2.0));

    let editor = open_file(&workbench, &root.join("a.txt"), cx);
    press_tab_at_start(&editor, cx);
    assert_eq!(editor.read_with(cx, |e, _| e.text()), "  x\n");

    open_page(&workbench, "editor.codeStyle.other", cx);
    click_on(cx, "settings-toggle-editor.insertSpaces");
    click_on(cx, "settings-ok");
    settle(cx, |cx| global_value(cx, "editor.insertSpaces") == Some(json!(false)));
    wait_long(cx, "tabs for indentation", |cx| editor.read_with(cx, |e, _| e.facts().indent_unit == "\t"));
    let other = open_file(&workbench, &root.join("b.txt"), cx);
    assert_eq!(other.read_with(cx, |e, _| e.facts().tab_size), 2);
    press_tab_at_start(&other, cx);
    assert_eq!(other.read_with(cx, |e, _| e.text()), "\ty\n");
}

// ---------------------------------------------------------------------------------------------
// File Encodings

// Settings › Editor › File Encodings says that a charset in .editorconfig takes precedence and that
// one file's encoding changes from the status bar
#[gpui_kit::test]
async fn file_encodings_says_editorconfig_takes_precedence(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (workbench, cx) = window(cx);
    let dialog = open_page(&workbench, "editor.fileEncodings", cx);
    let lines = dialog.read_with(cx, |view, cx| view.control_lines(cx));
    let at = lines.iter().position(|line| line.starts_with("Create UTF-8 files:")).expect("the BOM choice");
    assert_eq!(
        lines[at + 1],
        "    A charset in .editorconfig takes precedence. Change one file's encoding from the status bar."
    );
}

// Settings › File Encodings: a Project Encoding chosen on the page decodes a project file that is not
// valid UTF-8
#[gpui_kit::test]
async fn a_project_encoding_chosen_on_the_page_decodes_a_project_file(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = project(cx, &[("a.txt", "a\n")]);
    // "中文" in GBK: not valid UTF-8.
    std::fs::write(root.join("gbk.txt"), [0xD6, 0xD0, 0xCE, 0xC4, b'\n']).expect("the file is written");
    open_page(&workbench, "editor.fileEncodings", cx);
    click_on(cx, "settings-select-editor.encodings.project");
    click_on(cx, "settings-choice-editor.encodings.project-gbk");
    click_on(cx, "settings-ok");
    settle(cx, |cx| effective_value(cx, "editor.encodings.project") == Some(json!("gbk")));
    let editor = open_file(&workbench, &root.join("gbk.txt"), cx);
    assert_eq!(editor.read_with(cx, |e, _| e.text()), "中文\n");
}

// Settings › File Encodings: Create UTF-8 files with BOM chosen on the page writes the BOM into a file
// made with New File
#[gpui_kit::test]
async fn create_utf8_files_with_bom_chosen_on_the_page_writes_the_bom_to_a_new_file(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = project(cx, &[("src/main.rs", "fn main() {}\n")]);
    open_page(&workbench, "editor.fileEncodings", cx);
    bring_into_view(cx, "settings-select-editor.encodings.utf8Bom");
    click_on(cx, "settings-select-editor.encodings.utf8Bom");
    click_on(cx, "settings-choice-editor.encodings.utf8Bom-always");
    click_on(cx, "settings-ok");
    settle(cx, |cx| global_value(cx, "editor.encodings.utf8Bom") == Some(json!("always")));
    select_in_tree(&workbench, &root.join("src"), cx);
    run_action(cx, "project.newFile", None);
    let popup = new_popup_of(&workbench, cx);
    cx.simulate_keystrokes("enter");
    enter_new_name(&popup, "notes.txt", cx);
    let notes = root.join("src/notes.txt");
    settle(cx, |_| notes.is_file());
    assert_eq!(std::fs::read(&notes).expect("the file is read"), [0xEF, 0xBB, 0xBF]);
    close_backend_project(cx);
}

// ---------------------------------------------------------------------------------------------
// Code Folding

/// A file header comment, two imports, a documentation comment and a function.
const HEADED: &str =
    "// Header one\n// Header two\nuse std::fmt;\nuse std::io;\n\n/**\n * Doc\n */\nfn a() {\n    b();\n}\n";

// Settings › Editor › General › Code Folding: with Show code folding outline, File header and Imports
// unticked and Documentation comments ticked on the page, a file opened next has no fold column and
// opens with its header and imports open and its documentation comment folded
#[gpui_kit::test]
async fn code_folding_options_changed_on_the_page_decide_the_fold_column_and_the_folds_a_file_opens_with(
    cx: &mut TestAppContext,
) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = project(cx, &[("src/a.rs", HEADED), ("src/b.rs", HEADED)]);
    let editor = open_file(&workbench, &root.join("src/a.rs"), cx);
    wait_long(cx, "the folds", |cx| editor.read_with(cx, |e, _| e.folds_are_current()));
    wait_long(cx, "the default folds", |cx| {
        editor.read_with(cx, |e, _| e.shown_lines() == [0, 2, 4, 5, 6, 7, 8, 9, 10, 11])
    });
    assert!(editor.read_with(cx, |e, cx| e.fold_column_width(cx)) > gpui_kit::px(0.0));

    let dialog = open_page(&workbench, "editor.general.codeFolding", cx);
    for toggle in [
        "settings-toggle-editor.folding.showOutline",
        "settings-toggle-editor.folding.fileHeader",
        "settings-toggle-editor.folding.imports",
        "settings-toggle-editor.folding.docComments",
    ] {
        bring_into_view(cx, toggle);
        click_on(cx, toggle);
    }
    let lines = dialog.read_with(cx, |view, cx| view.control_lines(cx));
    for line in ["[ ] Show code folding outline", "[ ] File header", "[ ] Imports", "[x] Documentation comments"] {
        assert!(lines.contains(&line.to_owned()), "{line}: {lines:?}");
    }
    click_on(cx, "settings-ok");
    settle(cx, |cx| global_value(cx, "editor.folding.docComments") == Some(json!(true)));
    wait_long(cx, "no fold column", |cx| editor.read_with(cx, |e, cx| e.fold_column_width(cx)) == gpui_kit::px(0.0));

    let editor = open_file(&workbench, &root.join("src/b.rs"), cx);
    wait_long(cx, "the folds", |cx| editor.read_with(cx, |e, _| e.folds_are_current()));
    wait_long(cx, "the page's folds", |cx| {
        editor.read_with(cx, |e, _| e.shown_lines() == [0, 1, 2, 3, 4, 5, 8, 9, 10, 11])
    });
    assert_eq!(editor.read_with(cx, |e, cx| e.fold_column_width(cx)), gpui_kit::px(0.0));
    close_backend_project(cx);
}

// Settings › Code Folding: Reset discards an option ticked on the page and Cancel after the same edit
// writes nothing
#[gpui_kit::test]
async fn code_folding_reset_and_cancel_discard_an_edit(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (workbench, cx) = window(cx);
    let dialog = open_page(&workbench, "editor.general.codeFolding", cx);
    click_on(cx, "settings-toggle-editor.folding.docComments");
    assert!(is_modified(&dialog, cx));
    click_on(cx, "settings-reset");
    assert!(!is_modified(&dialog, cx));
    let lines = dialog.read_with(cx, |view, cx| view.control_lines(cx));
    assert!(lines.contains(&"[ ] Documentation comments".to_owned()), "{lines:?}");
    click_on(cx, "settings-toggle-editor.folding.docComments");
    click_on(cx, "settings-cancel");
    assert!(settings_dialog_of(&workbench, cx).is_none());
    assert_eq!(global_value(cx, "editor.folding.docComments"), Some(json!(false)));
}

/// The Code Style page's own form of the visual guides is the slot's view, so search and Reset reach
/// it as they reach every control of the page.
#[gpui_kit::test]
async fn code_style_keeps_its_slot_views_under_the_tabs(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (workbench, cx) = window(cx);
    let dialog = open_page(&workbench, "editor.codeStyle", cx);
    let guides: Entity<FormView> = custom(&dialog, "editor.codeStyle", "Visual guides", cx);
    type_into(cx, "settings-text-editor.codeStyle.visualGuides", "90");
    assert!(is_modified(&dialog, cx));
    click_on(cx, "settings-reset");
    let lines = guides.read_with(cx, |view, cx| view.lines(cx));
    assert_eq!(lines[0], "Visual guides:  columns");
}
