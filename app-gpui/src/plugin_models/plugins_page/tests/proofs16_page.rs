use super::*;

/// The local catalog's `id` as the Available tab shows it: its tags and its Install / Update button.
fn catalog_row(page: &Entity<PluginsPage>, id: &str, cx: &mut VisualTestContext) -> (Vec<String>, Option<String>) {
    let item = items(page, cx)
        .into_iter()
        .find(|item| item.kind == ItemKind::Catalog && item.id == id)
        .unwrap_or_else(|| panic!("{id} is listed"));
    (item.tags.into_iter().map(|tag| tag.label).collect(), item.install.map(|(label, _)| label))
}

/// The Available tab offers a package not installed to Install, an older installed version to Update, and
/// tags the installed version and a development copy instead of offering a button; the search field keeps
/// the packages whose name holds the text.
#[gpui_kit::test]
async fn derives_install_update_installed_and_dev_states(cx: &mut TestAppContext) {
    let (page, _events, cx) = open(cx);
    page.update(cx, |page, cx| {
        page.set_catalog(Ok(sample_catalog()), cx);
        page.select_tab(PluginsTab::Marketplace, cx);
    });
    list(&page, cx, Vec::new(), Vec::new());
    assert_eq!(catalog_row(&page, "hello", cx), (Vec::new(), Some("Install".to_owned())));

    list(&page, cx, vec![plugin("hello", json!({ "version": "0.0.9" }))], Vec::new());
    assert_eq!(catalog_row(&page, "hello", cx), (Vec::new(), Some("Update".to_owned())));
    list(&page, cx, vec![plugin("hello", json!({ "version": "0.1.0" }))], Vec::new());
    assert_eq!(catalog_row(&page, "hello", cx), (vec!["Installed".to_owned()], None));
    list(&page, cx, vec![plugin("hello", json!({ "version": "0.1.0", "source": "dev" }))], Vec::new());
    assert_eq!(catalog_row(&page, "hello", cx), (vec!["Dev plugin loaded".to_owned()], None));

    let field = page.read_with(cx, |page, _| page.search_field().clone());
    cx.update(|window, cx| window.focus(&field.focus_handle(cx), cx));
    cx.simulate_input("harbor");
    cx.run_until_parked();
    let shown: Vec<String> =
        items(&page, cx).into_iter().filter(|item| item.kind == ItemKind::Catalog).map(|item| item.id).collect();
    assert_eq!(shown, ["theme-harbor-night"]);
}

fn icon_theme(plugin: &str, id: &str) -> Contribution {
    Contribution::new(plugin, "iconThemes", id, json!({ "id": id, "label": id.to_uppercase(), "icons": 1 }))
}

fn adopted(events: &Events) -> Vec<Option<String>> {
    taken(events)
        .into_iter()
        .filter_map(|event| match event {
            PluginsPageEvent::SelectIconTheme(key) => Some(key),
            _ => None,
        })
        .collect()
}

/// Turns plugin `id`, listed off, on with its row's checkbox; the host's listing that follows has it on
/// with `ui` live.
fn enable(
    page: &Entity<PluginsPage>,
    id: &str,
    others: &[PluginInfo],
    ui: Vec<Contribution>,
    cx: &mut VisualTestContext,
) {
    let off = plugin(id, json!({ "enabled": false, "status": "disabled" }));
    list(page, cx, [others, std::slice::from_ref(&off)].concat(), Vec::new());
    click(cx, leak(format!("plugins-check-{id}")));
    page.update(cx, |page, cx| page.finished(id, Ok(()), cx));
    // The listing names its contribution points, whose sections the details show.
    let on = plugin(id, json!({ "contributions": [{ "point": "iconThemes", "label": "Icons" }] }));
    list(page, cx, [others, std::slice::from_ref(&on)].concat(), ui);
}

fn leak(text: String) -> &'static str {
    Box::leak(text.into_boxed_str())
}

/// A plugin turned on while no file icon theme is selected has its first icon theme selected; another one
/// turned on afterwards leaves that choice alone, and so does a plugin without icon themes.
#[gpui_kit::test]
async fn adopts_the_first_icon_theme_of_a_plugin_when_none_is_selected(cx: &mut TestAppContext) {
    let (page, events, cx) = open(cx);
    page.update(cx, |page, cx| page.set_appearance(Some("dark".to_owned()), None, cx));
    let p_themes = vec![icon_theme("p", "first"), icon_theme("p", "second")];

    enable(&page, "p", &[], p_themes.clone(), cx);
    assert_eq!(adopted(&events), [Some("p/first".to_owned())]);
    page.update(cx, |page, cx| page.select(ItemKind::Installed, "p", cx));
    let section = section(&details(&page, cx), "File Icon Themes").clone();
    let actions: Vec<Option<String>> =
        section.rows.iter().map(|row| row.action.as_ref().map(|action| action.label.clone())).collect();
    assert_eq!(actions, [Some("Use Default Icons".to_owned()), Some("Apply".to_owned())]);

    let p = plugin("p", json!({ "contributions": [{ "point": "iconThemes", "label": "Icons" }] }));
    enable(&page, "q", std::slice::from_ref(&p), [p_themes.clone(), vec![icon_theme("q", "other")]].concat(), cx);
    assert!(adopted(&events).is_empty(), "one is selected already");

    page.update(cx, |page, cx| page.set_appearance(Some("dark".to_owned()), None, cx));
    enable(&page, "r", std::slice::from_ref(&p), p_themes, cx);
    assert!(adopted(&events).is_empty(), "r has no icon theme");
}
