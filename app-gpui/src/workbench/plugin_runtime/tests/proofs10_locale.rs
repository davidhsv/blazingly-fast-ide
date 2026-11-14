use super::*;

/// What `command` of api14 answers for the window, run to its end.
fn api14(cx: &mut VisualTestContext, root: &Path, command: &str, arg: Option<&str>) -> IdeResult<Option<String>> {
    let running = run_in_window(cx, root, "api14", command, arg);
    wait(cx, "the command ends", |_| running.is_finished());
    running.join().expect("the command ran")
}

/// The plugin's code reads the UI language: with Chinese chosen in the settings, `locale()` answers that language's
/// tag.
#[gpui_kit::test]
async fn a_plugin_reads_the_ui_language_the_settings_choose(cx: &mut TestAppContext) {
    use ide_workspace::settings::SettingsScope;
    let home = start(cx);
    let folder = plugin_copy(home.path(), "api14", str::to_owned);
    link(cx, &folder, &[], "events");
    let (_folder, root, _workbench, cx) = project(cx, &[("a.txt", "a\n")]);
    cx.update(|_, cx| {
        crate::platform::settings::set(LABEL, SettingsScope::Global, "appearance.locale", &Value::from("zh"), cx)
    })
    .expect("the language is saved");
    wait(cx, "the plugin reads the chosen language", |cx| {
        api14(cx, &root, "locale", None).ok().flatten().as_deref() == Some("zh-CN")
    });
}
