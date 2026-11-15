use super::*;

fn listed_ids(page: &Entity<PluginsPage>, cx: &mut VisualTestContext) -> Vec<String> {
    items(page, cx).into_iter().map(|item| item.id).collect()
}

/// Replaces the text of the page's search field with `text`, typed.
fn search_for(page: &Entity<PluginsPage>, text: &str, cx: &mut VisualTestContext) {
    let field = page.read_with(cx, |page, _| page.search_field().clone());
    cx.update(|window, cx| window.focus(&field.focus_handle(cx), cx));
    field.update(cx, |field, cx| field.select_all(cx));
    cx.simulate_keystrokes("backspace");
    cx.simulate_input(text);
    cx.run_until_parked();
}

/// The search field keeps the plugins whose name, id, author or contribution holds the text, in any case; a
/// blank search keeps them all.
#[gpui_kit::test]
async fn filters_by_name_id_author_and_contribution(cx: &mut TestAppContext) {
    let (page, _events, cx) = open(cx);
    let alpha = plugin(
        "x.alpha",
        json!({ "name": "Alpha", "contributions": [{ "point": "commands", "label": "Count Words" }] }),
    );
    let beta = plugin("y.beta", json!({ "name": "Beta", "authors": ["Zed"] }));
    list(&page, cx, vec![alpha, beta], Vec::new());
    assert_eq!(listed_ids(&page, cx), ["x.alpha", "y.beta"]);

    search_for(&page, "count", cx);
    assert_eq!(listed_ids(&page, cx), ["x.alpha"], "by a contribution");
    assert!(drawn(cx, "plugins-row-x.alpha"));
    search_for(&page, "zed", cx);
    assert_eq!(listed_ids(&page, cx), ["y.beta"], "by an author");
    search_for(&page, "BETA", cx);
    assert_eq!(listed_ids(&page, cx), ["y.beta"], "by the name");
    search_for(&page, "x.alp", cx);
    assert_eq!(listed_ids(&page, cx), ["x.alpha"], "by the id");
    search_for(&page, " ", cx);
    assert_eq!(listed_ids(&page, cx), ["x.alpha", "y.beta"]);
}

const RECT: &str = r#"<svg viewBox="0 0 16 16"><rect width="16" height="16"/></svg>"#;
const CIRCLE: &str = r#"<svg viewBox="0 0 16 16"><circle cx="8" cy="8" r="8"/></svg>"#;

/// What the icon renderer loads for icon `name`: the SVG it is served, empty for none.
fn served(name: &str) -> String {
    use gpui_kit::AssetSource as _;
    let assets = theme::icons::IconAssets { fallback: gpui_kit::assets::Assets };
    let bytes = assets.load(name).ok().flatten().unwrap_or_default();
    String::from_utf8_lossy(&bytes).into_owned()
}

/// The icon the row of the only listed plugin `id` draws once it is listed with `icon`.
fn row_icon(page: &Entity<PluginsPage>, id: &str, icon: Value, cx: &mut VisualTestContext) -> String {
    list(page, cx, vec![plugin(id, json!({ "name": id, "icon": icon }))], Vec::new());
    let row = items(page, cx).into_iter().find(|item| item.id == id).expect("the plugin is listed");
    assert!(drawn(cx, leak(format!("plugins-row-{id}"))));
    row.icon
}

fn leak(text: String) -> &'static str {
    Box::leak(text.into_boxed_str())
}

/// A plugin's row draws its icon under a name of its own, which serves the plugin's markup.
#[gpui_kit::test]
async fn renders_a_plugins_icon_under_a_name_of_its_own(cx: &mut TestAppContext) {
    let (page, _, cx) = open(cx);
    let name = row_icon(&page, "p14.ab", json!({ "light": RECT, "dark": null }), cx);
    assert!(name.starts_with("ext:pluginIcon/p14.ab/"), "{name}");
    assert!(served(&name).contains("<rect"), "{}", served(&name));
}

/// Two copies of one plugin id with different icons (a marketplace and a local copy) draw two icons, each its
/// own markup.
#[gpui_kit::test]
async fn keeps_two_icons_of_the_same_plugin_id_apart_a_marketplace_and_a_local_copy(cx: &mut TestAppContext) {
    let (page, _, cx) = open(cx);
    let published = row_icon(&page, "p14.same", json!({ "light": RECT, "dark": null }), cx);
    let local = row_icon(&page, "p14.same", json!({ "light": CIRCLE, "dark": null }), cx);
    assert_ne!(local, published);
    assert!(served(&published).contains("<rect"));
    assert!(served(&local).contains("<circle"));
}

/// An icon with a dark variant serves that variant under the icon's dark name.
#[gpui_kit::test]
async fn registers_the_dark_variant_with_it(cx: &mut TestAppContext) {
    let (page, _, cx) = open(cx);
    let name = row_icon(&page, "p14.cd", json!({ "light": RECT, "dark": CIRCLE }), cx);
    assert!(served(&format!("{name}_dark")).contains("<circle"));
}

/// The icons of plugins `g` and `g_dark` stay apart: neither is the other's dark variant.
#[gpui_kit::test]
async fn keeps_the_icons_of_plugins_whose_ids_differ_by_dark_apart(cx: &mut TestAppContext) {
    let (page, _, cx) = open(cx);
    let plain = row_icon(&page, "p14g", json!({ "light": RECT, "dark": null }), cx);
    let other = row_icon(&page, "p14g_dark", json!({ "light": CIRCLE, "dark": null }), cx);
    assert!(!served(&plain).contains("<circle"));
    assert!(served(&format!("{plain}_dark")).is_empty(), "the plain icon has no dark variant");
    assert!(served(&other).contains("<circle"));
}

/// An icon changed to one without a dark variant has none, though the icon it replaced had one.
#[gpui_kit::test]
async fn gives_a_changed_icon_without_a_dark_variant_none(cx: &mut TestAppContext) {
    let (page, _, cx) = open(cx);
    row_icon(&page, "p14.ef", json!({ "light": RECT, "dark": CIRCLE }), cx);
    let name = row_icon(&page, "p14.ef", json!({ "light": RECT, "dark": null }), cx);
    assert!(served(&name).contains("<rect"));
    assert!(served(&format!("{name}_dark")).is_empty());
}

/// The served icon keeps the root's fill and stroke, and SVG's default black fill when the root sets none.
#[gpui_kit::test]
async fn keeps_the_roots_fill_and_stroke_and_svgs_default_black_fill(cx: &mut TestAppContext) {
    let (page, _, cx) = open(cx);
    let filled = r##"<svg viewBox="0 0 16 16" fill="#E5A50A" stroke="#FFFFFF"><circle cx="8" cy="8" r="7"/></svg>"##;
    let name = row_icon(&page, "p14.hi", json!({ "light": filled, "dark": null }), cx);
    assert!(served(&name).contains(r##"<g fill="#E5A50A" stroke="#FFFFFF"><circle"##), "{}", served(&name));
    let plain = r#"<svg viewBox="0 0 16 16"><circle cx="8" cy="8" r="7"/></svg>"#;
    let name = row_icon(&page, "p14.jk", json!({ "light": plain, "dark": null }), cx);
    assert!(served(&name).contains(r##"<g fill="#000"><circle"##), "{}", served(&name));
}
