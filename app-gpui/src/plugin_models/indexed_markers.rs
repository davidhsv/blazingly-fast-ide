//! Gutter markers from plugins' document indexers, resolved by the host from its cached graph: one
//! button per line going to the targets of every marker on it (a chooser for several), with the
//! first declared icon among them. An edit hides the markers until the edited text is indexed.
//!
//! [`IndexedMarkersView`] is one editor's state. Icons whose package files are still being read
//! hold the markers back; they are placed when [`IndexedMarkersView::refresh`] runs again after the
//! icons load, unless the document or the entry changed in between.

use super::line_markers::{NavigationButton, marker_icon};
use super::marker_icons::{IndexedMarkerIcon, MarkerIcons, Resolved};
use ide_core::Location;
use ide_plugin::indexing::FileMarkers;
use std::collections::HashMap;

/// The button of one line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarkerRow {
    /// 0-based.
    pub line: usize,
    pub button: NavigationButton,
}

/// `(plugin, declaration id)` → its icon, for the declarations an entry's markers name.
pub type DeclaredIcons = HashMap<(String, String), IndexedMarkerIcon>;

/// The declared icons of `entry`'s markers; `None` while one of them is still loading.
pub fn resolve_icons(entry: &FileMarkers, icons: &MarkerIcons) -> Option<DeclaredIcons> {
    let mut resolved = DeclaredIcons::new();
    for marker in &entry.markers {
        let Some(id) = &marker.icon else { continue };
        match icons.resolve_indexed_marker_icon(&marker.plugin_id, id) {
            Resolved::Pending => return None,
            Resolved::Ready(Some(icon)) => {
                resolved.insert((marker.plugin_id.clone(), id.clone()), icon);
            }
            Resolved::Ready(None) => {}
        }
    }
    Some(resolved)
}

/// A line, its targets and its icon, while rows are collected.
type Row<'a> = (usize, Vec<Location>, Option<&'a IndexedMarkerIcon>);

/// One row per line of a document of `line_count` lines, with the targets of every marker on it
/// (once each) and the first declared icon among them. Without gutter icons there are none.
pub fn markers_of(
    entry: Option<&FileMarkers>,
    line_count: usize,
    icons: &DeclaredIcons,
    show_gutter_icons: bool,
) -> Vec<MarkerRow> {
    let Some(entry) = entry.filter(|_| show_gutter_icons && line_count > 0) else { return Vec::new() };
    let mut rows: Vec<Row<'_>> = Vec::new();
    for marker in &entry.markers {
        let line = (marker.range.start.line as usize).min(line_count - 1);
        let at = match rows.iter().position(|(l, _, _)| *l == line) {
            Some(at) => at,
            None => {
                rows.push((line, Vec::new(), None));
                rows.len() - 1
            }
        };
        let row = &mut rows[at];
        for target in &marker.targets {
            if !row.1.iter().any(|t| t.path == target.path && t.range == target.range) {
                row.1.push(target.clone());
            }
        }
        if row.2.is_none() {
            row.2 = marker.icon.as_ref().and_then(|id| icons.get(&(marker.plugin_id.clone(), id.clone())));
        }
    }
    rows.sort_by_key(|(line, _, _)| *line);
    rows.into_iter()
        .map(|(line, targets, icon)| {
            let name = marker_icon(icon.and_then(|icon| icon.icon.as_deref()));
            let tooltip = icon.and_then(|icon| icon.tooltip.as_deref());
            MarkerRow { line, button: NavigationButton::new(targets, &name, tooltip) }
        })
        .collect()
}

/// The indexed markers of one editor.
#[derive(Debug, Default)]
pub struct IndexedMarkersView {
    rows: Vec<MarkerRow>,
    /// The entry last placed or waiting for icons, as JSON; `None` after an edit.
    last: Option<String>,
    waiting: bool,
}

impl IndexedMarkersView {
    pub fn rows(&self) -> &[MarkerRow] {
        &self.rows
    }

    /// The document changed: the markers go until the text is indexed again.
    pub fn doc_changed(&mut self) {
        self.rows.clear();
        self.last = None;
        self.waiting = false;
    }

    /// Marker icons were declared, loaded or withdrawn, or the gutter settings changed.
    pub fn icons_changed(&mut self) {
        self.last = None;
    }

    /// Places the markers of `entry` (the store's entry for this file) when it is new or icons
    /// changed; holds them back while a declared icon loads. Returns whether the rows changed.
    pub fn refresh(
        &mut self,
        entry: Option<&FileMarkers>,
        line_count: usize,
        icons: &MarkerIcons,
        show_gutter_icons: bool,
    ) -> bool {
        let seen = entry.map(|entry| serde_json::to_string(entry).unwrap_or_default());
        if seen.is_some() && self.last == seen && !self.waiting {
            return false;
        }
        self.last = seen;
        let Some(entry) = entry else {
            self.waiting = false;
            let changed = !self.rows.is_empty();
            self.rows.clear();
            return changed;
        };
        let Some(declared) = resolve_icons(entry, icons) else {
            self.waiting = true;
            return false;
        };
        self.waiting = false;
        let rows = markers_of(Some(entry), line_count, &declared, show_gutter_icons);
        let changed = rows != self.rows;
        self.rows = rows;
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::i18n::I18n;
    use crate::plugin_models::line_markers::NavigationClick;
    use crate::plugin_models::marker_icons::tests::{decl, svg};
    use crate::plugin_models::marker_icons::{MarkerIconAssets, MarkerIconDecl};
    use crate::plugin_models::plugin_icon::PluginIcons;
    use ide_core::{Pos, Range};
    use ide_plugin::indexing::IndexedMarker;

    const LINES: usize = 3;

    fn target(path: &str) -> Location {
        Location::file(path.into(), Range::new(Pos::new(0, 0), Pos::new(0, 4)))
    }

    /// A marker on `find` in line 1 of `/p/Query.xml`.
    fn marker(plugin: &str, icon: Option<&str>, targets: Vec<Location>) -> IndexedMarker {
        IndexedMarker {
            range: Range::new(Pos::new(1, 12), Pos::new(1, 16)),
            targets,
            plugin_id: plugin.to_owned(),
            icon: icon.map(str::to_owned),
        }
    }

    fn entry(version: u32, markers: Vec<IndexedMarker>) -> FileMarkers {
        FileMarkers { path: "/p/Query.xml".into(), version: Some(version), markers }
    }

    fn plain() -> FileMarkers {
        entry(1, vec![marker("p", None, vec![target("/p/Query.java")])])
    }

    struct Icons {
        icons: MarkerIcons,
        names: PluginIcons,
    }

    impl Icons {
        fn new() -> Self {
            Self { icons: MarkerIcons::default(), names: PluginIcons::default() }
        }

        fn declare(&mut self, plugin: &str, decl: MarkerIconDecl) -> MarkerIconAssets {
            self.icons.project(plugin, decl)
        }

        /// Loads the declaration's light SVG with `shape`.
        fn load(&mut self, read: &MarkerIconAssets, id: &str, shape: &str) {
            let assets = vec![(format!("markerIcon/{id}/light"), svg(shape))];
            let (registered, _) = self.icons.assets_answered(read.handle, Ok(assets), &mut self.names);
            crate::theme::icons::register_external_icons(registered.into_iter().map(|(n, s)| (n, s.into_bytes())));
        }
    }

    fn view_of(entry: &FileMarkers, icons: &MarkerIcons) -> IndexedMarkersView {
        let mut view = IndexedMarkersView::default();
        view.refresh(Some(entry), LINES, icons, true);
        view
    }

    #[test]
    fn puts_a_button_on_the_markers_line_that_goes_to_its_target() {
        let icons = Icons::new();
        let view = view_of(&plain(), &icons.icons);
        let [row] = view.rows() else { panic!("one row") };
        assert_eq!(row.line, 1);
        assert!(row.button.title.contains("Query.java:1"));
        assert!(row.button.label(&mut I18n::default()).contains("Query.java:1"));
        assert_eq!(row.button.click(&I18n::default()), Some(NavigationClick::Navigate(target("/p/Query.java"))));
    }

    #[test]
    fn hides_markers_on_edit_and_when_the_entry_goes_away() {
        let icons = Icons::new();
        let mut view = view_of(&plain(), &icons.icons);
        view.doc_changed();
        assert!(view.rows().is_empty());
        let mut view = view_of(&plain(), &icons.icons);
        assert!(view.refresh(None, LINES, &icons.icons, true));
        assert!(view.rows().is_empty());
    }

    #[test]
    fn merges_a_lines_targets_and_offers_a_chooser_for_several() {
        let icons = Icons::new();
        let mut merged = plain();
        merged.markers.push(marker("p", None, vec![target("/p/Query.java"), target("/p/Other.java")]));
        let view = view_of(&merged, &icons.icons);
        let [row] = view.rows() else { panic!("one row") };
        assert!(row.button.title.contains("Other.java"));
        assert_eq!(row.button.targets.len(), 2);
        assert!(matches!(row.button.click(&I18n::default()), Some(NavigationClick::Choose { .. })));
    }

    #[test]
    fn draws_the_declared_icon_and_tooltip_of_the_markers_key_prefix() {
        let mut icons = Icons::new();
        let read = icons.declare("p", decl("usages", "icons/usages.svg", Some("Usages of this class")));
        icons.load(&read, "usages", r#"<circle cx="8" cy="8" r="7"/>"#);
        let mut view = view_of(&plain(), &icons.icons);
        assert_eq!(view.rows()[0].button.title, "Query.java:1");
        let targets = || vec![target("/p/Query.java")];
        let markers = vec![
            marker("p", None, targets()),
            marker("p", Some("unknown"), targets()),
            marker("p", Some("usages"), targets()),
        ];
        let three = entry(2, markers);
        assert!(view.refresh(Some(&three), LINES, &icons.icons, true));
        let button = &view.rows()[0].button;
        assert!(button.icon.starts_with("ext:pluginIcon/p/"), "{}", button.icon);
        assert_eq!(button.title, "Usages of this class\nQuery.java:1");
        assert!(button.label(&mut I18n::default()).contains("Usages of this class"));
        let unknown = entry(3, vec![marker("p", Some("unknown"), targets())]);
        view.refresh(Some(&unknown), LINES, &icons.icons, true);
        assert_eq!(view.rows()[0].button.title, "Query.java:1");
        assert!(!view.rows()[0].button.icon.starts_with("ext:"));
    }

    #[test]
    fn redraws_when_a_marker_icon_declaration_goes_away() {
        let mut icons = Icons::new();
        let read = icons.declare("p", decl("usages", "icons/usages.svg", Some("Usages")));
        icons.load(&read, "usages", r#"<circle cx="8" cy="8" r="7"/>"#);
        let usages = entry(1, vec![marker("p", Some("usages"), vec![target("/p/Query.java")])]);
        let mut view = view_of(&usages, &icons.icons);
        assert!(view.rows()[0].button.icon.starts_with("ext:"));
        assert!(icons.icons.remove(read.handle));
        view.icons_changed();
        assert!(view.refresh(Some(&usages), LINES, &icons.icons, true));
        assert!(!view.rows()[0].button.icon.starts_with("ext:"));
        assert_eq!(view.rows()[0].button.title, "Query.java:1");
    }

    #[test]
    fn drops_pending_icons_after_a_change() {
        for change in ["edit", "remove", "replace", "dispose"] {
            let mut icons = Icons::new();
            let read = icons.declare("p", decl("slow", "icons/slow.svg", Some("Old marker")));
            let slow = entry(2, vec![marker("p", Some("slow"), vec![target("/p/Query.java")])]);
            let mut view = view_of(&plain(), &icons.icons);
            assert!(!view.refresh(Some(&slow), LINES, &icons.icons, true), "held back while the icon loads");
            let current = match change {
                "edit" => {
                    view.doc_changed();
                    None
                }
                "remove" => {
                    view.refresh(None, LINES, &icons.icons, true);
                    None
                }
                "replace" => {
                    view.refresh(Some(&plain()), LINES, &icons.icons, true);
                    Some(plain())
                }
                _ => {
                    view = IndexedMarkersView::default();
                    None
                }
            };
            icons.load(&read, "slow", r#"<circle cx="8" cy="8" r="7"/>"#);
            if change != "dispose" {
                view.refresh(current.as_ref(), LINES, &icons.icons, true);
            }
            assert!(view.rows().iter().all(|row| !row.button.icon.starts_with("ext:")), "{change}");
            match change {
                "replace" => assert_eq!(view.rows()[0].button.title, "Query.java:1"),
                _ => assert!(view.rows().is_empty(), "{change}"),
            }
        }
    }

    #[test]
    fn positions_a_new_indexed_result_against_the_edited_document_while_icons_load() {
        let mut icons = Icons::new();
        let read = icons.declare("p", decl("slow", "icons/slow.svg", None));
        let first = entry(1, vec![marker("p", Some("slow"), vec![target("/p/Query.java")])]);
        let mut view = view_of(&first, &icons.icons);
        view.doc_changed();
        let mut moved = marker("p", Some("slow"), vec![target("/p/Query.java")]);
        moved.range = Range::new(Pos::new(2, 12), Pos::new(2, 16));
        let second = entry(2, vec![moved]);
        assert!(!view.refresh(Some(&second), LINES + 1, &icons.icons, true));
        icons.load(&read, "slow", r#"<circle cx="8" cy="8" r="7"/>"#);
        assert!(view.refresh(Some(&second), LINES + 1, &icons.icons, true));
        assert!(view.rows()[0].button.icon.starts_with("ext:"));
        assert_eq!(view.rows()[0].line, 2);
    }

    #[test]
    fn merges_two_plugins_on_one_line_and_uses_the_first_declared_icon_with_a_plain_text_tooltip() {
        let mut icons = Icons::new();
        let tooltip = r#"<img src="invalid" onerror="alert(1)"> & usages"#;
        let first = icons.declare("first", decl("shared", "icons/shared.svg", Some(tooltip)));
        let second = icons.declare("second", decl("shared", "icons/shared.svg", Some("Second")));
        icons.load(&first, "shared", r#"<circle cx="8" cy="8" r="7"/>"#);
        icons.load(&second, "shared", r#"<rect width="16" height="16"/>"#);
        let both = entry(
            1,
            vec![
                marker("first", Some("shared"), vec![target("/p/Query.java")]),
                marker("second", Some("shared"), vec![target("/p/Query.java"), target("/p/Other.java")]),
            ],
        );
        let mut view = view_of(&both, &icons.icons);
        let [row] = view.rows() else { panic!("one row") };
        assert!(row.button.icon.starts_with("ext:pluginIcon/first/"), "{}", row.button.icon);
        assert_eq!(row.button.title, format!("{tooltip}\nQuery.java:1\nOther.java:1"));
        assert!(matches!(row.button.click(&I18n::default()), Some(NavigationClick::Choose { .. })));
        icons.icons.remove(first.handle);
        view.icons_changed();
        view.refresh(Some(&both), LINES, &icons.icons, true);
        assert!(view.rows()[0].button.icon.starts_with("ext:pluginIcon/second/"));
        assert_eq!(view.rows()[0].button.title, "Second\nQuery.java:1\nOther.java:1");
    }
}
