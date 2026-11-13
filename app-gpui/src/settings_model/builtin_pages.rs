//! Every built-in page of the Settings, as [`PageSpec`]s in the order the Settings tree shows them.
//!
//! A page made of plain controls lists them; a page with controls of its own lists each as an
//! [`OptionSpec::Custom`] slot, so the search still finds it by its words; a page that is a view
//! of its own (Keymap, Plugins, CMake, Gradle, Maven, the language pages…) has no sections and
//! is shown through the dialog's page view.

mod behavior;
mod build;
mod editor;
mod features;
mod tools;
mod vcs;

pub use editor::{CODE_STYLE_SCOPE, COLOR_SCHEME_LANGUAGES, COLOR_SCHEME_SCOPES, code_style_language_pages};

use super::inspections_model::{inspections_page, natural_languages_page, spelling_page};
use super::language_pages::language_pages;
use super::mcp_server::mcp_server_page;
use super::page_spec::{OptionSpec, PageSpec};
use super::pages::{SettingsPage, build_page_tree, flatten};
use super::run_targets::run_targets_page;
use std::collections::HashMap;

/// The group of the Appearance & Behavior pages.
pub const APPEARANCE_GROUP: &str = "Appearance & Behavior";
/// The group of the Editor pages.
pub const EDITOR_GROUP: &str = "Editor";
/// The group of the Tools pages.
pub const TOOLS_GROUP: &str = "Tools";

fn toggle(key: &'static str, label: &'static str) -> OptionSpec {
    OptionSpec::Toggle { key, label }
}

/// A whole number within `min..=max`.
fn number(key: &'static str, label: &'static str, min: f64, max: f64, unit: Option<&'static str>) -> OptionSpec {
    OptionSpec::Number { key, label, min, max, integer: true, unit }
}

fn select(key: &'static str, label: &'static str, options: &[(&'static str, &'static str)]) -> OptionSpec {
    select_owned(key, label, options.to_vec())
}

fn select_owned(key: &'static str, label: &'static str, options: Vec<(&'static str, &'static str)>) -> OptionSpec {
    OptionSpec::Select { key, label, options }
}

fn text(key: &'static str, label: &'static str) -> OptionSpec {
    OptionSpec::Text { key, label }
}

fn custom(label: &'static str, keywords: &[&'static str]) -> OptionSpec {
    custom_owned(label, keywords.to_vec())
}

fn custom_owned(label: &'static str, keywords: Vec<&'static str>) -> OptionSpec {
    OptionSpec::Custom { label, keywords }
}

/// `specs` sorted as the Settings tree lists them, depth-first.
fn in_display_order(mut specs: Vec<PageSpec>) -> Vec<PageSpec> {
    let pages: Vec<SettingsPage> = specs.iter().map(SettingsPage::from).collect();
    let tree = build_page_tree(&pages);
    let listed = flatten(&tree).into_iter().filter(|node| node.page.is_some());
    let rank: HashMap<&str, usize> = listed.enumerate().map(|(at, node)| (node.key.as_str(), at)).collect();
    specs.sort_by_key(|spec| rank.get(spec.id).copied().unwrap_or(usize::MAX));
    specs
}

/// The built-in pages with a fixed id, in display order: Appearance & Behavior, Keymap, Editor,
/// Plugins, Version Control, Project Structure, Build, Execution, Deployment, Languages &
/// Frameworks, Tools, and the pages other features contribute. `updates` adds System Settings ›
/// Updates, which only builds that update themselves have. The Code Style page of each language
/// comes from [`code_style_language_pages`].
pub fn settings_registry(updates: bool) -> Vec<PageSpec> {
    let mut specs = behavior::pages();
    if updates {
        specs.push(behavior::updates());
    }
    specs.extend([editor::keymap(), editor::file_types()]);
    specs.extend(editor::pages());
    specs.extend([inspections_page(), natural_languages_page(), spelling_page()]);
    specs.extend(vcs::pages());
    specs.extend(build::pages());
    specs.push(run_targets_page());
    specs.extend(tools::pages());
    specs.extend([mcp_server_page(), tools::ssh_configurations()]);
    specs.extend(language_pages());
    specs.extend(features::pages());
    in_display_order(specs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::i18n::I18n;
    use crate::settings_model::page_spec::spec_labels;
    use crate::settings_model::pages::{PageNode, breadcrumb};
    use crate::settings_model::schema::{default_settings, get_at, parse_settings, set_at};
    use crate::theme::color_scheme::attributes;
    use serde_json::{Value, json};
    use std::collections::HashSet;

    fn spec(id: &str) -> PageSpec {
        settings_registry(true).into_iter().find(|spec| spec.id == id).unwrap_or_else(|| panic!("no page {id}"))
    }

    fn options(spec: &PageSpec) -> impl Iterator<Item = &OptionSpec> {
        spec.sections.iter().flat_map(|section| &section.options)
    }

    fn rows(nodes: &[PageNode], depth: usize, out: &mut Vec<(usize, String)>) {
        for node in nodes {
            out.push((depth, node.key.clone()));
            rows(&node.children, depth + 1, out);
        }
    }

    fn tree_rows(specs: &[PageSpec]) -> Vec<(usize, String)> {
        let pages: Vec<SettingsPage> = specs.iter().map(SettingsPage::from).collect();
        let mut out = Vec::new();
        rows(&build_page_tree(&pages), 0, &mut out);
        out
    }

    /// The values a control writes: both booleans, the range's ends, every choice, some text.
    fn samples(option: &OptionSpec) -> Vec<Value> {
        match option {
            OptionSpec::Toggle { .. } => vec![json!(true), json!(false)],
            OptionSpec::Number { min, max, integer: true, .. } => vec![json!(*min as i64), json!(*max as i64)],
            OptionSpec::Number { min, max, .. } => vec![json!(min), json!(max)],
            OptionSpec::Select { options, .. } => options.iter().map(|(value, _)| json!(value)).collect(),
            OptionSpec::Text { .. } => vec![json!("text")],
            OptionSpec::Custom { .. } => Vec::new(),
        }
    }

    // Not from a spec: the tree lists every built-in page where IntelliJ IDEA has it, each level by
    // order and then title; a page nests under the page its dotted id extends.
    #[test]
    fn lists_every_page_in_the_settings_tree_order() {
        let expected: &[(usize, &str)] = &[
            (0, "group:Appearance & Behavior"),
            (1, "appearance"),
            (1, "general"),
            (2, "general.language"),
            (2, "general.dateFormats"),
            (2, "general.httpProxy"),
            (2, "general.mode"),
            (2, "general.updates"),
            (1, "notifications"),
            (0, "keymap"),
            (0, "group:Editor"),
            (1, "editor.general"),
            (2, "editor.general.autoImport"),
            (2, "editor.general.appearance"),
            (2, "editor.general.breadcrumbs"),
            (2, "editor.general.codeCompletion"),
            (2, "editor.general.codeFolding"),
            (2, "editor.general.editorTabs"),
            (2, "editor.general.gutterIcons"),
            (2, "editor.general.inlineCompletion"),
            (2, "editor.general.postfixCompletion"),
            (2, "editor.general.richTextCopy"),
            (2, "editor.general.smartKeys"),
            (2, "editor.general.stickyLines"),
            (1, "editor.codeEditing"),
            (1, "editor.font"),
            (1, "editor.colorScheme"),
            (2, "editor.colorScheme.general"),
            (2, "editor.colorScheme.language-defaults"),
            (2, "editor.colorScheme.debugger"),
            (2, "editor.colorScheme.python"),
            (2, "editor.colorScheme.markdown"),
            (2, "editor.colorScheme.regexp"),
            (2, "editor.colorScheme.go"),
            (2, "editor.colorScheme.java"),
            (2, "editor.colorScheme.kotlin"),
            (2, "editor.colorScheme.php"),
            (1, "editor.codeStyle"),
            (2, "editor.codeStyle.other"),
            (1, "editor.inspections"),
            (1, "editor.fileTemplates"),
            (1, "editor.naturalLanguages"),
            (2, "editor.naturalLanguages.spelling"),
            (1, "editor.fileEncodings"),
            (1, "editor.liveTemplates"),
            (1, "editor.files"),
            (1, "editor.inlayHints"),
            (1, "editor.todo"),
            (1, "editor.vim"),
            (0, "group:Security"),
            (1, "projectTrust"),
            (0, "plugins"),
            (0, "vcs"),
            (1, "vcs.commit"),
            (1, "vcs.confirmation"),
            (1, "vcs.mappings"),
            (1, "vcs.issueNavigation"),
            (1, "vcs.log"),
            (1, "vcs.shelf"),
            (1, "vcs.git"),
            (1, "vcs.github"),
            (0, "projectStructure"),
            (0, "group:Build, Execution, Deployment"),
            (1, "build.cmake"),
            (1, "build.gradle"),
            (1, "build.maven"),
            (1, "build.console"),
            (1, "build.debugger"),
            (2, "build.debugger.dataViews"),
            (2, "build.debugger.stepping"),
            (1, "build.pythonDebugger"),
            (1, "build.run"),
            (1, "build.runTargets"),
            (1, "build.deployment"),
            (2, "build.deployment.options"),
            (0, "group:Languages & Frameworks"),
            (1, "languages.go"),
            (2, "languages.go.buildTags"),
            (2, "languages.go.modules"),
            (1, "languages.java"),
            (1, "languages.servers"),
            (1, "languages.markdown"),
            (1, "languages.node"),
            (1, "python.interpreter"),
            (1, "languages.php"),
            (1, "languages.prettier"),
            (1, "languages.oxfmt"),
            (1, "languages.ruff"),
            (1, "languages.ruby"),
            (1, "languages.rust"),
            (1, "languages.typescript"),
            (1, "languages.pluginFormatters"),
            (1, "languages.serverTargets"),
            (0, "group:Tools"),
            (1, "tools.agents"),
            (2, "tools.agents.chat"),
            (1, "tools.terminal"),
            (1, "tools.actionsOnSave"),
            (1, "tools.externalTools"),
            (1, "tools.sshConfigurations"),
            (1, "tools.mcpServer"),
        ];
        let registry = settings_registry(true);
        let actual = tree_rows(&registry);
        let actual: Vec<(usize, &str)> = actual.iter().map(|(depth, key)| (*depth, key.as_str())).collect();
        assert_eq!(actual, expected);
        let ids: Vec<&str> = registry.iter().map(|spec| spec.id).collect();
        let pages: Vec<&str> = expected.iter().map(|(_, key)| *key).filter(|key| !key.starts_with("group:")).collect();
        assert_eq!(ids, pages, "the registry is in display order");
    }

    // Not from a spec: Updates is listed only for builds that update themselves.
    #[test]
    fn lists_updates_only_for_builds_that_update_themselves() {
        let without = settings_registry(false);
        assert!(without.iter().all(|spec| spec.id != "general.updates"));
        assert_eq!(without.len() + 1, settings_registry(true).len());
    }

    // Not from a spec: page ids are unique, and so are the setting keys and the slot labels of a
    // page, which the dialog keys its fields and slot views by.
    #[test]
    fn has_no_duplicate_ids_keys_or_slots() {
        let registry = settings_registry(true);
        let mut ids = HashSet::new();
        for spec in &registry {
            assert!(ids.insert(spec.id), "page {} is registered twice", spec.id);
            let mut keys = HashSet::new();
            let mut slots = HashSet::new();
            for option in options(spec) {
                match option.key() {
                    Some(key) => assert!(keys.insert(key), "{}: {key} twice", spec.id),
                    None => assert!(slots.insert(option.label()), "{}: slot {} twice", spec.id, option.label()),
                }
            }
        }
    }

    // Not from a spec: every control writes a setting of the backend's settings, with values the
    // backend takes: both booleans, the ends of a range, every choice, any text.
    #[test]
    fn every_option_writes_a_setting_the_backend_takes() {
        let defaults = default_settings();
        for spec in settings_registry(true) {
            for option in options(&spec) {
                let Some(key) = option.key() else { continue };
                assert!(get_at(&defaults, key).is_some(), "{}: {key} is not a setting", spec.id);
                for value in samples(option) {
                    let written = set_at(&defaults, key, value.clone());
                    assert!(parse_settings(&written).is_ok(), "{}: {key} = {value} is refused", spec.id);
                }
            }
        }
    }

    // Not from a spec: the Color Scheme language pages are the languages of the attribute tree, in
    // the order it first names them.
    #[test]
    fn has_a_color_scheme_page_per_language_of_the_attribute_tree() {
        let mut roots: Vec<&str> = Vec::new();
        for attribute in attributes() {
            let Some(root) = attribute.path.first().map(String::as_str) else { continue };
            if !roots.contains(&root) && !COLOR_SCHEME_SCOPES.contains(&root) {
                roots.push(root);
            }
        }
        let titles: Vec<&str> = COLOR_SCHEME_LANGUAGES.iter().map(|(_, title)| *title).collect();
        assert_eq!(roots, titles);
        for (id, title) in COLOR_SCHEME_LANGUAGES {
            assert_eq!(id, format!("editor.colorScheme.{}", title.to_lowercase()));
            let names: Vec<&str> = attributes()
                .iter()
                .filter(|attribute| attribute.path.first().is_some_and(|root| root == title))
                .map(|attribute| attribute.name.as_str())
                .collect();
            assert_eq!(spec(id).keywords, names, "{id}");
        }
    }

    // Not from a spec: Code Style and its pages share the scheme scope, picked on the page; a
    // language page sorts before Other File Types.
    #[test]
    fn shares_the_code_style_scheme_scope() {
        for id in ["editor.codeStyle", "editor.codeStyle.other"] {
            let page = spec(id);
            let scope = (page.scope_group, page.own_scope_switch, page.project_scope);
            assert_eq!(scope, (Some(CODE_STYLE_SCOPE), true, true), "{id}");
        }
        let languages = code_style_language_pages(&[("rust", "Rust"), ("plaintext", "Plain text"), ("diff", "Diff")]);
        assert_eq!(languages.len(), 1);
        let rust = &languages[0];
        let place = (rust.id.as_str(), rust.title.as_str(), rust.group.as_deref());
        assert_eq!(place, ("editor.codeStyle.rust", "Rust", Some(EDITOR_GROUP)));
        assert!(rust.project_scope && rust.order < Some(spec("editor.codeStyle.other").order));
    }

    // page tree › indexes built-in option labels for search
    #[test]
    fn indexes_built_in_option_labels_for_search() {
        let general = spec_labels(&spec("editor.general"));
        for label in ["Soft-wrap files", "Reformat code on save", "Save Files"] {
            assert!(general.contains(&label), "{label}");
        }
        let completion = spec_labels(&spec("editor.general.codeCompletion"));
        for label in ["Show suggestions as you type", "First letter only"] {
            assert!(completion.contains(&label), "{label}");
        }
        let appearance = spec_labels(&spec("appearance"));
        for label in ["Accent colour", "High Contrast", "UI font size", "Zoom", "Compact mode", "Antialiasing"] {
            assert!(appearance.contains(&label), "{label}");
        }
    }

    // indexes toolbar and status bar visibility in Appearance settings
    #[test]
    fn indexes_toolbar_and_status_bar_visibility_in_appearance_settings() {
        let labels = spec_labels(&spec("appearance"));
        assert!(labels.contains(&"Show main toolbar"));
        assert!(labels.contains(&"Show status bar"));
    }

    // inspections tree › registers as Editor › Inspections
    #[test]
    fn registers_as_editor_inspections() {
        let page = spec("editor.inspections");
        assert_eq!(page.group, Some(EDITOR_GROUP));
        assert!(page.keywords.contains(&"Weak Warning"));
    }

    // inspection profile management › registers Natural Languages › Spelling under Editor
    #[test]
    fn registers_natural_languages_spelling_under_editor() {
        let registry = settings_registry(true);
        let pages: Vec<SettingsPage> = registry.iter().map(SettingsPage::from).collect();
        let tree = build_page_tree(&pages);
        assert_eq!(breadcrumb(&tree, "editor.naturalLanguages.spelling"), ["Editor", "Natural Languages", "Spelling"]);
        assert!(spec("editor.naturalLanguages.spelling").keywords.contains(&"accepted words"));
    }

    // Settings dialog › offers only new window, current window and ask, including old attach settings
    #[test]
    fn offers_only_new_window_current_window_and_ask_including_old_attach_settings() {
        let general = spec("general");
        let open_in = options(&general).find(|option| option.label() == "Open project in");
        let Some(OptionSpec::Custom { keywords, .. }) = open_in else { panic!("no Open project in slot") };
        assert_eq!(keywords, &["New window", "Current window", "Ask"]);
        let attach = parse_settings(&json!({ "general": { "openProjectIn": "attach" } }));
        assert_eq!(attach.map(|settings| settings.general.open_project_in), Ok("attach".to_owned()));
        let i18n = I18n::default();
        let labels: Vec<String> = spec_labels(&general).iter().map(|&label| label.to_owned()).collect();
        assert!(!crate::settings_model::pages::matches_terms(&labels, &["attach".to_owned()], &i18n));
    }

    // Java formatting profile › keeps the profile controls specific to Java
    #[test]
    fn keeps_the_profile_controls_specific_to_java() {
        let pages = code_style_language_pages(&[("java", "Java"), ("rust", "Rust")]);
        let has_profile = |id: &str| {
            let page = pages.iter().find(|page| page.id == id);
            page.is_some_and(|page| page.keywords.iter().any(|keyword| keyword == "Formatting profile"))
        };
        assert!(has_profile("editor.codeStyle.java"));
        assert!(!has_profile("editor.codeStyle.rust"));
    }
}
