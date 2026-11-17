use super::*;
use crate::platform::i18n::Localization;
use crate::platform::ui_language::{self, SystemLanguage};
use menu::{MenuBar, MenuNode};

/// The titles of the menus of the bar, once it follows the window.
fn menu_titles(cx: &mut VisualTestContext) -> Vec<String> {
    cx.update(|window, cx| {
        menu::sync(window, cx);
        let bar = cx.global::<MenuBar>().shown().unwrap_or_default();
        bar.iter()
            .filter_map(|node| match node {
                MenuNode::Submenu { label, .. } => Some(label.clone()),
                _ => None,
            })
            .collect()
    })
}

/// Find Action with "Toggle Light/Dark Theme" typed: waits until the row it selects is titled `title`,
/// then closes the popup.
fn find_action_selects(workbench: &Entity<Workbench>, title: &str, cx: &mut VisualTestContext) {
    run_action(cx, "navigate.findAction", None);
    let everywhere = workbench.read_with(cx, |w, _| w.search_everywhere()).expect("Find Action opens");
    type_query(cx, "Toggle Light/Dark Theme");
    settle(cx, |cx| everywhere.read_with(cx, |popup, _| popup.selected().as_deref() == Some(title)));
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(workbench.read_with(cx, |w, _| w.search_everywhere().is_none()), "Escape closes Find Action");
}

/// File › Settings…, the Language and Region page found by its words, `choice` chosen in Language and
/// applied; returns the dialog's option lines after the Apply, then closes the dialog with OK.
fn apply_language(workbench: &Entity<Workbench>, choice: &'static str, cx: &mut VisualTestContext) -> Vec<String> {
    run_action(cx, "file.settings", None);
    let open = settings_dialog_of(workbench, cx).expect("Settings opens");
    cx.simulate_input("translation");
    cx.run_until_parked();
    assert_eq!(open.read_with(cx, |view, cx| view.selected_page(cx)).as_deref(), Some("general.language"));
    click_on(cx, "settings-select-appearance.locale");
    let before = cx.update(|_, cx| cx.default_global::<Localization>().0.locale());
    click_on(cx, choice);
    assert!(open.read_with(cx, |view, cx| view.can_apply(cx)), "the choice waits for Apply");
    assert_eq!(
        cx.update(|_, cx| cx.default_global::<Localization>().0.locale()),
        before,
        "nothing changes before Apply"
    );
    click_on(cx, "settings-apply");
    let lines = open.read_with(cx, |view, cx| view.control_lines(cx));
    click_on(cx, "settings-ok");
    settle(cx, |cx| settings_dialog_of(workbench, cx).is_none());
    lines
}

/// Settings › Language and Region: Chinese applied in a project window turns the menu bar's menus, the
/// Project tool window's title, Find Action's titles and the open Settings dialog to Chinese at once;
/// English applied again turns them back, without a restart.
#[gpui_kit::test]
async fn the_language_applied_in_settings_reaches_the_menus_a_tool_window_and_find_action(cx: &mut TestAppContext) {
    let home = start(cx);
    cx.update(ui_language::init);
    let (_folder, _root, workbench, cx) = trusted_project(cx, &[("notes.txt", "one\n")]);
    cx.update(|window, _| window.activate_window());
    settle(cx, |cx| cx.debug_bounds("tool-window-title:project:Project").is_some());
    assert!(menu_titles(cx).contains(&"File".to_owned()), "{:?}", menu_titles(cx));
    find_action_selects(&workbench, "Toggle Light/Dark Theme", cx);

    let lines = apply_language(&workbench, "settings-choice-appearance.locale-zh", cx);
    assert!(global_settings_file(&home).contains("locale = \"zh\""), "{}", global_settings_file(&home));
    assert!(lines.iter().any(|line| line.starts_with("语言")), "the dialog itself is in Chinese: {lines:?}");
    settle(cx, |cx| cx.debug_bounds("tool-window-title:project:项目").is_some());
    assert!(cx.debug_bounds("tool-window-title:project:Project").is_none());
    let titles = menu_titles(cx);
    assert!(titles.contains(&"文件".to_owned()) && !titles.contains(&"File".to_owned()), "{titles:?}");
    find_action_selects(&workbench, "切换浅色/深色主题", cx);

    let lines = apply_language(&workbench, "settings-choice-appearance.locale-en", cx);
    assert!(lines.iter().any(|line| line.starts_with("Language")), "{lines:?}");
    settle(cx, |cx| cx.debug_bounds("tool-window-title:project:Project").is_some());
    let titles = menu_titles(cx);
    assert!(titles.contains(&"File".to_owned()) && !titles.contains(&"文件".to_owned()), "{titles:?}");
    find_action_selects(&workbench, "Toggle Light/Dark Theme", cx);
}

/// A home whose stored settings choose Chinese opens its first window in Chinese: the menu bar and the
/// Project tool window's title.
#[gpui_kit::test]
async fn a_home_whose_settings_choose_chinese_opens_its_first_window_in_chinese(cx: &mut TestAppContext) {
    let home = tempfile::tempdir().expect("a temporary home");
    std::fs::create_dir_all(home.path().join("config")).expect("the config folder is made");
    std::fs::write(home.path().join("config/settings.toml"), "[appearance]\nlocale = \"zh\"\n")
        .expect("the settings are written");
    let _home = start_in(home, cx);
    cx.update(ui_language::init);
    let (_folder, _root, _workbench, cx) = trusted_project(cx, &[("notes.txt", "one\n")]);
    settle(cx, |cx| cx.debug_bounds("tool-window-title:project:项目").is_some());
    let titles = menu_titles(cx);
    assert!(titles.contains(&"文件".to_owned()), "{titles:?}");
}

/// With the system in Chinese, a home without a Language setting runs in English; `system`, `auto` and
/// an empty value stand for the system's language, a regional tag for its language, and a language the
/// app does not have for English.
#[gpui_kit::test]
async fn an_unset_language_is_english_system_is_the_systems_and_an_unknown_one_is_english(cx: &mut TestAppContext) {
    use ide_workspace::settings::SettingsScope;
    fn locale(cx: &mut TestAppContext) -> &'static str {
        cx.update(|cx| cx.default_global::<Localization>().0.locale())
    }
    fn write(value: &str, cx: &mut TestAppContext) -> &'static str {
        cx.update(|cx| {
            crate::platform::settings::set(
                "w1",
                SettingsScope::Global,
                "appearance.locale",
                &serde_json::Value::from(value),
                cx,
            )
        })
        .expect("the language is saved");
        cx.run_until_parked();
        locale(cx)
    }
    let home = start(cx);
    assert!(!global_settings_file(&home).contains("locale"), "{}", global_settings_file(&home));
    cx.update(|cx| {
        cx.set_global(SystemLanguage("zh-CN".to_owned()));
        ui_language::init(cx);
    });
    assert_eq!(locale(cx), "en", "unset: English, not the system's Chinese");
    for (value, expected) in
        [("system", "zh"), ("xx", "en"), ("zh-CN", "zh"), ("de", "en"), ("auto", "zh"), ("en", "en"), ("", "zh")]
    {
        assert_eq!(write(value, cx), expected, "{value:?}");
    }
}
