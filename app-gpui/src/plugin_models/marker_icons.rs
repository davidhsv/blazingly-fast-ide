//! Icons of plugins' `markerIcons` declarations: a package SVG for gutter markers, found through the
//! declaration the host validated (never by a path a marker names). A line marker names the
//! declared package path; an indexed marker names the declaration the host matched to its key.
//!
//! [`MarkerIcons`] reads nothing: [`MarkerIcons::project`] returns the asset prefix to read and
//! [`MarkerIcons::assets_answered`] takes the files. A newer declaration of the same icon replaces
//! an older one; removing the older one then keeps the newer.

use super::plugin_icon::{IconMarkup, PluginIcons, Registration};
use serde::Deserialize;
use std::collections::HashMap;

/// A `markerIcons` declaration.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct MarkerIconDecl {
    pub id: String,
    /// The package path line markers name.
    pub icon: String,
    #[serde(default)]
    pub tooltip: Option<String>,
}

/// The icon and tooltip of an indexed marker's declaration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexedMarkerIcon {
    /// `None` when the package icon could not be loaded.
    pub icon: Option<String>,
    pub tooltip: Option<String>,
}

/// What an icon resolves to now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resolved<T> {
    /// Its package icon is still being read: resolve again when [`MarkerIcons::assets_answered`]
    /// reports a change.
    Pending,
    Ready(T),
}

/// A projected declaration, for [`MarkerIcons::remove`] and [`MarkerIcons::assets_answered`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MarkerIconHandle(u64);

/// The assets to read for a projected declaration: `plugin_assets(plugin_id, prefix)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarkerIconAssets {
    pub handle: MarkerIconHandle,
    pub plugin_id: String,
    pub prefix: String,
}

#[derive(Clone, Debug)]
struct Projected {
    plugin_id: String,
    decl: MarkerIconDecl,
    /// `None` while loading; then the icon name, `None` inside when the package icon is unusable.
    icon: Option<Option<String>>,
}

#[derive(Debug, Default)]
pub struct MarkerIcons {
    next: u64,
    projected: HashMap<MarkerIconHandle, Projected>,
    /// (plugin, package path) → the newest declaration naming it.
    by_path: HashMap<(String, String), MarkerIconHandle>,
    /// (plugin, declaration id) → the newest declaration with that id.
    by_id: HashMap<(String, String), MarkerIconHandle>,
}

impl MarkerIcons {
    /// Projects a declaration of plugin `plugin_id`; returns what to read for its icon.
    pub fn project(&mut self, plugin_id: &str, decl: MarkerIconDecl) -> MarkerIconAssets {
        self.next += 1;
        let handle = MarkerIconHandle(self.next);
        let prefix = format!("markerIcon/{}/", decl.id);
        self.by_path.insert((plugin_id.to_owned(), decl.icon.clone()), handle);
        self.by_id.insert((plugin_id.to_owned(), decl.id.clone()), handle);
        self.projected.insert(handle, Projected { plugin_id: plugin_id.to_owned(), decl, icon: None });
        MarkerIconAssets { handle, plugin_id: plugin_id.to_owned(), prefix }
    }

    /// The files read for `handle` (`Err`: the plugin is gone or the read failed): its
    /// `markerIcon/<id>/light` and optional `/dark` SVGs become an icon name. Returns the icons to
    /// register and whether resolved icons changed (markers must be drawn again).
    pub fn assets_answered(
        &mut self,
        handle: MarkerIconHandle,
        assets: Result<Vec<(String, String)>, String>,
        names: &mut PluginIcons,
    ) -> (Vec<Registration>, bool) {
        let Some(projected) = self.projected.get_mut(&handle) else { return (Vec::new(), false) };
        let (icon, registrations) = match assets {
            Ok(assets) => {
                let svg = |variant: &str| {
                    let id = format!("markerIcon/{}/{variant}", projected.decl.id);
                    assets.iter().find(|(asset, _)| *asset == id).map(|(_, svg)| svg.as_str())
                };
                match svg("light") {
                    Some(light) => {
                        let (name, registrations) =
                            names.name(&projected.plugin_id, Some(IconMarkup { light, dark: svg("dark") }));
                        (Some(name), registrations)
                    }
                    None => (None, Vec::new()),
                }
            }
            Err(error) => {
                tracing::warn!("plugin {}: marker icon {}: {error}", projected.plugin_id, projected.decl.id);
                (None, Vec::new())
            }
        };
        projected.icon = Some(icon);
        (registrations, true)
    }

    /// Withdraws a declaration; returns whether it was the one in effect for its path or id.
    pub fn remove(&mut self, handle: MarkerIconHandle) -> bool {
        let Some(projected) = self.projected.remove(&handle) else { return false };
        let path = (projected.plugin_id.clone(), projected.decl.icon.clone());
        let id = (projected.plugin_id, projected.decl.id);
        let mut current = false;
        if self.by_path.get(&path) == Some(&handle) {
            self.by_path.remove(&path);
            current = true;
        }
        if self.by_id.get(&id) == Some(&handle) {
            self.by_id.remove(&id);
            current = true;
        }
        current
    }

    /// The icon a line marker of `plugin_id` names: the declared package icon's name, else the name
    /// as given (a built-in icon or an alias); `None` without one.
    pub fn resolve_marker_icon(&self, plugin_id: &str, icon: Option<&str>) -> Resolved<Option<String>> {
        let Some(icon) = icon.filter(|icon| !icon.is_empty()) else { return Resolved::Ready(None) };
        let declared = self.by_path.get(&(plugin_id.to_owned(), icon.to_owned())).and_then(|h| self.projected.get(h));
        match declared.map(|projected| &projected.icon) {
            Some(None) => Resolved::Pending,
            Some(Some(Some(name))) => Resolved::Ready(Some(name.clone())),
            _ => Resolved::Ready(Some(icon.to_owned())),
        }
    }

    /// The icon and tooltip of declaration `id` of `plugin_id`, which the host matched to an indexed
    /// marker; `None` when no such declaration is live.
    pub fn resolve_indexed_marker_icon(&self, plugin_id: &str, id: &str) -> Resolved<Option<IndexedMarkerIcon>> {
        let Some(projected) =
            self.by_id.get(&(plugin_id.to_owned(), id.to_owned())).and_then(|h| self.projected.get(h))
        else {
            return Resolved::Ready(None);
        };
        match &projected.icon {
            None => Resolved::Pending,
            Some(icon) => {
                Resolved::Ready(Some(IndexedMarkerIcon { icon: icon.clone(), tooltip: projected.decl.tooltip.clone() }))
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::plugin_models::line_markers::marker_icon;

    pub(crate) fn decl(id: &str, icon: &str, tooltip: Option<&str>) -> MarkerIconDecl {
        MarkerIconDecl { id: id.to_owned(), icon: icon.to_owned(), tooltip: tooltip.map(str::to_owned) }
    }

    pub(crate) fn svg(shape: &str) -> String {
        format!(r#"<svg viewBox="0 0 16 16">{shape}</svg>"#)
    }

    fn ready<T>(resolved: Resolved<T>) -> T {
        match resolved {
            Resolved::Ready(value) => value,
            Resolved::Pending => panic!("still loading"),
        }
    }

    #[test]
    fn resolves_declared_package_paths_within_their_plugin_and_uses_the_dark_svg() {
        let (mut icons, mut names) = (MarkerIcons::default(), PluginIcons::default());
        let read = icons.project("p", decl("resource", "icons/resource.svg", None));
        assert_eq!((read.plugin_id.as_str(), read.prefix.as_str()), ("p", "markerIcon/resource/"));
        assert_eq!(icons.resolve_marker_icon("p", Some("icons/resource.svg")), Resolved::Pending);
        let assets = vec![
            ("markerIcon/resource/light".to_owned(), svg(r#"<rect width="16" height="16"/>"#)),
            ("markerIcon/resource/dark".to_owned(), svg(r#"<circle cx="8" cy="8" r="7"/>"#)),
        ];
        let (registered, changed) = icons.assets_answered(read.handle, Ok(assets), &mut names);
        assert!(changed);
        let name = ready(icons.resolve_marker_icon("p", Some("icons/resource.svg"))).expect("an icon");
        assert!(registered.iter().any(|(n, svg)| *n == format!("{name}_dark") && svg.contains("<circle")));
        let elsewhere = ready(icons.resolve_marker_icon("other", Some("icons/resource.svg")));
        assert_eq!(elsewhere.as_deref(), Some("icons/resource.svg"));
        crate::theme::icons::register_external_icons(registered.into_iter().map(|(n, svg)| (n, svg.into_bytes())));
        assert_eq!(marker_icon(Some(&name)), name);
        icons.remove(read.handle);
        let removed = ready(icons.resolve_marker_icon("p", Some("icons/resource.svg")));
        assert_eq!(removed.as_deref(), Some("icons/resource.svg"));
    }

    #[test]
    fn retains_built_in_names_legacy_aliases_and_unknown_icon_fallback() {
        let icons = MarkerIcons::default();
        let database = ready(icons.resolve_marker_icon("p", Some("database")));
        assert_eq!(marker_icon(database.as_deref()), "database");
        assert_eq!(marker_icon(Some("overriding")), "gutter-overriding");
        assert_eq!(marker_icon(Some("not-an-icon")), "gutter-implementing");
        assert_eq!(marker_icon(Some("__proto__")), "gutter-implementing");
        assert_eq!(marker_icon(Some("constructor")), "gutter-implementing");
        assert_eq!(marker_icon(None), "gutter-implementing");
    }

    #[test]
    fn keeps_a_replacement_when_an_older_declaration_is_disposed() {
        let (mut icons, mut names) = (MarkerIcons::default(), PluginIcons::default());
        let assets = || Ok(vec![("markerIcon/resource/light".to_owned(), svg(r#"<rect width="16" height="16"/>"#))]);
        let old = icons.project("p", decl("resource", "icons/resource.svg", None));
        let new = icons.project("p", decl("resource", "icons/resource.svg", None));
        icons.assets_answered(old.handle, assets(), &mut names);
        icons.assets_answered(new.handle, assets(), &mut names);
        assert!(!icons.remove(old.handle), "the replacement stays in effect");
        let path = ready(icons.resolve_marker_icon("p", Some("icons/resource.svg"))).expect("an icon");
        assert!(path.starts_with("ext:"), "{path}");
        let indexed = ready(icons.resolve_indexed_marker_icon("p", "resource")).expect("declared");
        assert!(indexed.icon.is_some_and(|icon| icon.starts_with("ext:")));
        assert!(icons.remove(new.handle));
        assert_eq!(icons.resolve_indexed_marker_icon("p", "resource"), Resolved::Ready(None));
    }

    #[test]
    fn preserves_line_marker_lookup_with_projected_indexed_marker_defaults() {
        let (mut icons, mut names) = (MarkerIcons::default(), PluginIcons::default());
        let read = icons.project("p", decl("resource", "icons/resource.svg", None));
        let assets = vec![("markerIcon/resource/light".to_owned(), svg(r#"<rect width="16" height="16"/>"#))];
        icons.assets_answered(read.handle, Ok(assets), &mut names);
        let icon = ready(icons.resolve_marker_icon("p", Some("icons/resource.svg")));
        assert!(icon.as_deref().is_some_and(|icon| icon.starts_with("ext:")));
        let expected = Resolved::Ready(Some(IndexedMarkerIcon { icon, tooltip: None }));
        assert_eq!(icons.resolve_indexed_marker_icon("p", "resource"), expected);
    }

    #[test]
    fn falls_back_when_an_asset_fails_to_load() {
        let (mut icons, mut names) = (MarkerIcons::default(), PluginIcons::default());
        let read = icons.project("p", decl("resource", "icons/resource.svg", None));
        icons.assets_answered(read.handle, Err("disabled".to_owned()), &mut names);
        let icon = ready(icons.resolve_marker_icon("p", Some("icons/resource.svg")));
        assert_eq!(marker_icon(icon.as_deref()), "gutter-implementing");
    }
}
