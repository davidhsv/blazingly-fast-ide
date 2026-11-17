use super::*;

/// The language plugin code reads with `locale()` follows the settings: with Chinese chosen, the host answers that
/// language's tag.
#[gpui_kit::test]
async fn a_plugin_reads_the_ui_language_the_settings_choose(cx: &mut TestAppContext) {
    use ide_workspace::settings::SettingsScope;
    let home = start(cx);
    let folder = plugin_copy(home.path(), "api14", str::to_owned);
    link(cx, &folder, &[], "events");
    let (_folder, _root, _workbench, cx) = project(cx, &[("a.txt", "a\n")]);
    let host = cx.update(|_, cx| cx.global::<Plugins>().host.clone());
    assert_eq!(host.ui_locale(), "en", "the app starts in English");
    cx.update(|_, cx| {
        crate::platform::settings::set(LABEL, SettingsScope::Global, "appearance.locale", &Value::from("zh"), cx)
    })
    .expect("the language is saved");
    wait(cx, "the plugin host is told the chosen language", |_| host.ui_locale() == "zh-CN");
}
