use super::*;

const ICON: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="M2 2h12v12H2z"/></svg>"#;

/// A view whose declared icon is an SVG file of the plugin has its tool window drawn with that SVG, read from the
/// plugin's assets, instead of the generic plugin icon.
#[gpui_kit::test]
async fn a_views_svg_icon_is_read_from_the_plugin_and_draws_its_tool_window(cx: &mut TestAppContext) {
    let home = start_with_plugins(cx);
    let folder = plugin_copy(home.path(), "hello", |manifest| {
        let edited = manifest.replacen("icon = \"plugin\"", "icon = \"icons/greetings.svg\"", 1);
        assert_ne!(edited, manifest, "the Greetings view declares its icon by name");
        edited
    });
    std::fs::create_dir_all(folder.join("icons")).expect("the icons folder is made");
    std::fs::write(folder.join("icons/greetings.svg"), ICON).expect("the icon is written");
    link(cx, &folder, &[], "word-count");
    let (_project, _root, workbench, cx) = trusted_project(cx, LABEL, &[]);

    wait_long(cx, "the view's own icon", |cx| {
        registered(&workbench, GREETINGS, cx).is_some_and(|window| window.icon == "ext:hello/view-greetings")
    });
    assert!(cx.debug_bounds("stripe:plugin.hello.greetings").is_some(), "the window is on its stripe");
    assert_eq!(status_of("hello", cx), Some(PluginStatus::Inactive), "reading the icon runs no plugin code");
}
