//! Icon names for a plugin's own SVG icons (its package icon, marker icons). A name follows the
//! markup, so two copies of a plugin with different icons (marketplace, local) keep their own; plugin
//! ids hold no `/`, so no id yields another plugin's name or its `_dark` variant.
//!
//! [`PluginIcons::name`] returns the icons to register (`crate::theme::icons::register_external_icons`)
//! the first time a name is handed out; the caller registers them.

use regex::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;

/// The generic icon of a plugin without one.
pub const GENERIC_ICON: &str = "plugin";

/// A plugin's SVG icon: the light markup and an optional dark one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IconMarkup<'a> {
    pub light: &'a str,
    pub dark: Option<&'a str>,
}

/// An icon to register: its name and its SVG.
pub type Registration = (String, String);

/// FNV-1a over UTF-16 code units: a short key for an icon's markup.
fn hash(text: &str) -> String {
    let mut h: u32 = 0x811c_9dc5;
    for unit in text.encode_utf16() {
        h = (h ^ u32::from(unit)).wrapping_mul(0x0100_0193);
    }
    format!("{h:x}")
}

const ROOT_ONLY: [&str; 5] = ["viewBox", "width", "height", "id", "version"];

static FILL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"\sfill=""#).expect("the fill pattern is valid"));
static ATTRIBUTE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\s([A-Za-z0-9_:-]+)="[^"]*""#).expect("the attribute pattern is valid"));

/// The icon renderer keeps only the root's `viewBox` and draws on `fill="none"`: the root's other
/// attributes move onto a group, with SVG's default black fill unless the root sets one.
pub fn standalone(svg: &str) -> String {
    let Some(open) = svg.find("<svg") else { return svg.to_owned() };
    let Some(start) = svg[open..].find('>').map(|at| open + at + 1) else { return svg.to_owned() };
    let Some(end) = svg.rfind("</svg>").filter(|end| *end >= start) else { return svg.to_owned() };
    let tag = &svg[open..start];
    let inherited: String = ATTRIBUTE
        .captures_iter(tag)
        .filter(|found| {
            let name = &found[1];
            !ROOT_ONLY.contains(&name) && !name.starts_with("xmlns")
        })
        .map(|found| found[0].to_owned())
        .collect();
    let fill = if FILL.is_match(&inherited) { "" } else { " fill=\"#000\"" };
    format!("{tag}<g{fill}{inherited}>{}</g></svg>", &svg[start..end])
}

/// The icon names handed out this session; an icon is registered once.
#[derive(Debug, Default)]
pub struct PluginIcons {
    registered: HashSet<String>,
}

impl PluginIcons {
    /// The icon name of plugin `plugin_id`'s icon (the generic one without), and what to register
    /// for it when the name is new.
    pub fn name(&mut self, plugin_id: &str, icon: Option<IconMarkup<'_>>) -> (String, Vec<Registration>) {
        let Some(icon) = icon else { return (GENERIC_ICON.to_owned(), Vec::new()) };
        let key = hash(&format!("{}\n{}", icon.light, icon.dark.unwrap_or("")));
        let name = format!("ext:pluginIcon/{plugin_id}/{key}");
        if !self.registered.insert(name.clone()) {
            return (name, Vec::new());
        }
        let mut registrations = vec![(name.clone(), standalone(icon.light))];
        if let Some(dark) = icon.dark {
            registrations.push((format!("{name}_dark"), standalone(dark)));
        }
        (name, registrations)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn svg(shape: &str, root: &str) -> String {
        format!(r#"<svg viewBox="0 0 16 16"{root}>{shape}</svg>"#)
    }

    fn light(markup: &str) -> Option<IconMarkup<'_>> {
        Some(IconMarkup { light: markup, dark: None })
    }

    fn markup<'a>(registrations: &'a [Registration], name: &str) -> Option<&'a str> {
        registrations.iter().find(|(n, _)| n == name).map(|(_, svg)| svg.as_str())
    }

    const RECT: &str = r#"<rect width="16" height="16"/>"#;
    const CIRCLE: &str = r#"<circle cx="8" cy="8" r="8"/>"#;

    #[test]
    fn gives_plugins_without_an_icon_the_generic_one() {
        let mut icons = PluginIcons::default();
        assert_eq!(icons.name("a.b", None), (GENERIC_ICON.to_owned(), Vec::new()));
    }

    #[test]
    fn renders_a_plugins_icon_under_a_name_of_its_own() {
        let mut icons = PluginIcons::default();
        let rect = svg(RECT, "");
        let (name, registered) = icons.name("a.b", light(&rect));
        assert!(name.starts_with("ext:pluginIcon/a.b/"), "{name}");
        assert!(markup(&registered, &name).is_some_and(|svg| svg.contains("<rect")));
        assert_eq!(icons.name("a.b", light(&rect)), (name, Vec::new()), "registered once");
    }

    #[test]
    fn keeps_two_icons_of_the_same_plugin_id_apart_a_marketplace_and_a_local_copy() {
        let mut icons = PluginIcons::default();
        let (rect, circle) = (svg(RECT, ""), svg(CIRCLE, ""));
        let (published, first) = icons.name("a.b", light(&rect));
        let (local, second) = icons.name("a.b", light(&circle));
        assert_ne!(local, published);
        assert!(markup(&first, &published).is_some_and(|svg| svg.contains("<rect")));
        assert!(markup(&second, &local).is_some_and(|svg| svg.contains("<circle")));
    }

    #[test]
    fn registers_the_dark_variant_with_it() {
        let mut icons = PluginIcons::default();
        let (rect, circle) = (svg(RECT, ""), svg(CIRCLE, ""));
        let (name, registered) = icons.name("c.d", Some(IconMarkup { light: &rect, dark: Some(&circle) }));
        assert!(markup(&registered, &format!("{name}_dark")).is_some_and(|svg| svg.contains("<circle")));
    }

    #[test]
    fn keeps_the_icons_of_plugins_whose_ids_differ_by_dark_apart() {
        let mut icons = PluginIcons::default();
        let (rect, circle) = (svg(RECT, ""), svg(CIRCLE, ""));
        let (plain, first) = icons.name("g", light(&rect));
        let (other, second) = icons.name("g_dark", light(&circle));
        assert!(!first.iter().any(|(_, svg)| svg.contains("<circle")));
        assert!(!second.iter().any(|(name, _)| name.starts_with(&plain)));
        assert!(markup(&second, &other).is_some_and(|svg| svg.contains("<circle")));
    }

    #[test]
    fn gives_a_changed_icon_without_a_dark_variant_none() {
        let mut icons = PluginIcons::default();
        let (rect, circle) = (svg(RECT, ""), svg(CIRCLE, ""));
        icons.name("e.f", Some(IconMarkup { light: &rect, dark: Some(&circle) }));
        let (name, registered) = icons.name("e.f", light(&rect));
        assert_eq!(registered.len(), 1);
        assert!(markup(&registered, &name).is_some_and(|svg| !svg.contains("<circle")));
    }

    #[test]
    fn keeps_the_roots_fill_and_stroke_and_svgs_default_black_fill() {
        let filled = standalone(&svg(r#"<circle cx="8" cy="8" r="7"/>"#, r##" fill="#E5A50A" stroke="#FFFFFF""##));
        assert!(filled.contains(r##"<g fill="#E5A50A" stroke="#FFFFFF"><circle"##), "{filled}");
        let plain = standalone(&svg(r#"<circle cx="8" cy="8" r="7"/>"#, ""));
        assert!(plain.contains(r##"<g fill="#000"><circle"##), "{plain}");
        assert!(plain.starts_with(r#"<svg viewBox="0 0 16 16">"#));
    }
}
