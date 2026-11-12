//! Gutter icons from plugins' `line-markers` providers, and the navigation button both kinds of
//! plugin markers share. Markers are asked once per document version (debounced) and follow their
//! lines in between; a click opens the marker's target. The gutter column exists only while there
//! are markers, so editors without plugin providers keep their width.
//!
//! [`LineMarkerGutter`] is one editor's state: it returns [`GutterEffect`]s (a timer, a fetch with a
//! ticket) and takes the answers; an answer for an older document version or generation is dropped.

use super::marker_icons::{MarkerIcons, Resolved};
use crate::platform::i18n::{I18n, Param};
use crate::theme::icons::icon_path;
use ide_core::Location;
use ide_host::plugins::LineMarkerInfo;

/// Delay between an edit and the next request.
pub const DEBOUNCE_MS: u64 = 400;

/// The icon a marker's `icon` names: a legacy alias, a registered icon name, else the navigate one.
pub fn marker_icon(icon: Option<&str>) -> String {
    let name = icon.unwrap_or("navigate");
    let alias = match name {
        "implemented" => Some("gutter-implemented"),
        "implementing" | "navigate" => Some("gutter-implementing"),
        "overridden" => Some("gutter-overridden"),
        "overriding" => Some("gutter-overriding"),
        "run" => Some("gutter-run"),
        _ => None,
    };
    match alias {
        Some(alias) => alias.to_owned(),
        None if icon_path(name).is_some() => name.to_owned(),
        None => "gutter-implementing".to_owned(),
    }
}

/// What a click on a navigation button does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NavigationClick {
    Navigate(Location),
    /// Several targets: a chooser titled `title` lists them.
    Choose {
        title: String,
        targets: Vec<Location>,
    },
}

/// The button of one or more targets (interactive and indexed markers).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NavigationButton {
    pub targets: Vec<Location>,
    pub icon: String,
    /// The tooltip, then `<file>:<line>` for each target.
    pub title: String,
}

impl NavigationButton {
    pub fn new(targets: Vec<Location>, icon: &str, tooltip: Option<&str>) -> Self {
        let places = targets.iter().map(|t| {
            let path = t.path.to_string_lossy();
            let name = path.rsplit('/').next().unwrap_or_default().to_owned();
            format!("{name}:{}", t.range.start.line + 1)
        });
        let title: Vec<String> = tooltip.map(str::to_owned).into_iter().chain(places).collect();
        let icon = if icon_path(icon).is_some() { icon } else { "arrow-right" };
        Self { targets, icon: icon.to_owned(), title: title.join("\n") }
    }

    /// The accessible label.
    pub fn label(&self, i18n: &mut I18n) -> String {
        i18n.tx_built(&["Go to mapping: ", ""], &[Param::Text(self.title.clone())])
    }

    pub fn click(&self, i18n: &I18n) -> Option<NavigationClick> {
        match self.targets.as_slice() {
            [] => None,
            [only] => Some(NavigationClick::Navigate(only.clone())),
            several => {
                Some(NavigationClick::Choose { title: i18n.tx("Choose Mapping Target"), targets: several.to_vec() })
            }
        }
    }
}

/// A line marker on an existing line, with its icon resolved.
#[derive(Clone, Debug)]
pub struct PlacedMarker {
    pub marker: LineMarkerInfo,
    /// 0-based.
    pub line: usize,
    pub icon: String,
}

impl PlacedMarker {
    /// The marker's button when it has a target; otherwise it is a plain icon with the tooltip.
    pub fn button(&self) -> Option<NavigationButton> {
        self.marker.target.clone().map(|target| NavigationButton::new(vec![target], &self.icon, None))
    }
}

/// The markers on lines that exist in a document of `line_count` lines.
pub fn place_markers(line_count: usize, markers: &[LineMarkerInfo], icons: &MarkerIcons) -> Vec<PlacedMarker> {
    markers
        .iter()
        .filter(|m| (m.line as usize) < line_count)
        .map(|marker| {
            let resolved = match icons.resolve_marker_icon(&marker.plugin_id, marker.icon.as_deref()) {
                Resolved::Ready(icon) => icon,
                Resolved::Pending => marker.icon.clone(),
            };
            PlacedMarker { marker: marker.clone(), line: marker.line as usize, icon: marker_icon(resolved.as_deref()) }
        })
        .collect()
}

/// A request for the editor's markers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GutterTicket {
    pub generation: u64,
    /// The document version asked about.
    pub version: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GutterEffect {
    /// Call [`LineMarkerGutter::run`] after this many milliseconds (replacing a pending call).
    Schedule(u64),
    /// `plugin_line_markers(doc)`; answer with [`LineMarkerGutter::answered`].
    Fetch(GutterTicket),
}

/// The plugin line markers of one editor.
#[derive(Debug)]
pub struct LineMarkerGutter {
    generation: u64,
    markers: Vec<PlacedMarker>,
    /// Whether the column is in the gutter.
    shown: bool,
    /// The last answer arrived while the editor was hidden.
    skipped: bool,
}

impl LineMarkerGutter {
    /// A gutter for a new editor; its first request is scheduled.
    pub fn new() -> (Self, Vec<GutterEffect>) {
        let gutter = Self { generation: 0, markers: Vec::new(), shown: false, skipped: false };
        (gutter, vec![GutterEffect::Schedule(DEBOUNCE_MS)])
    }

    pub fn markers(&self) -> &[PlacedMarker] {
        &self.markers
    }

    /// Whether the column is shown.
    pub fn shown(&self) -> bool {
        self.shown
    }

    /// The document changed: markers follow their lines (`map_line` maps an old line to its new
    /// one) and are asked for again.
    pub fn doc_changed(&mut self, map_line: impl Fn(usize) -> usize) -> Vec<GutterEffect> {
        for marker in &mut self.markers {
            marker.line = map_line(marker.line);
        }
        vec![GutterEffect::Schedule(DEBOUNCE_MS)]
    }

    /// The editor is visible again; an answer skipped while hidden is asked for again.
    pub fn shown_again(&mut self) -> Vec<GutterEffect> {
        if self.skipped { vec![GutterEffect::Schedule(DEBOUNCE_MS)] } else { Vec::new() }
    }

    /// The host dropped its document indexes: the markers and the column go at once.
    pub fn document_indexes_changed(&mut self) -> Vec<GutterEffect> {
        self.generation += 1;
        self.shown = false;
        self.markers.clear();
        vec![GutterEffect::Schedule(DEBOUNCE_MS)]
    }

    /// A marker icon was declared, loaded or withdrawn.
    pub fn marker_icons_changed(&mut self) -> Vec<GutterEffect> {
        self.generation += 1;
        vec![GutterEffect::Schedule(DEBOUNCE_MS)]
    }

    /// The debounce elapsed: asks for the markers of document version `version`.
    pub fn run(&mut self, version: u64) -> Vec<GutterEffect> {
        self.generation += 1;
        vec![GutterEffect::Fetch(GutterTicket { generation: self.generation, version })]
    }

    /// The markers for `ticket`, for a document now at `version` with `line_count` lines and shown
    /// (`visible`) or not. Returns whether the markers changed.
    pub fn answered(
        &mut self,
        ticket: GutterTicket,
        version: u64,
        line_count: usize,
        visible: bool,
        markers: Result<Vec<LineMarkerInfo>, String>,
        icons: &MarkerIcons,
    ) -> bool {
        if ticket.generation != self.generation || ticket.version != version {
            return false;
        }
        let markers = match markers {
            Ok(markers) => markers,
            Err(error) => {
                tracing::debug!("plugin line markers: {error}");
                return false;
            }
        };
        self.skipped = !visible;
        if self.skipped {
            return false;
        }
        let placed = place_markers(line_count, &markers, icons);
        if placed.is_empty() && self.markers.is_empty() {
            return false;
        }
        self.shown = !placed.is_empty();
        self.markers = placed;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin_models::marker_icons::tests::{decl, svg};
    use crate::plugin_models::plugin_icon::PluginIcons;

    fn marker(plugin: &str, line: u32, icon: &str, tooltip: &str) -> LineMarkerInfo {
        LineMarkerInfo {
            plugin_id: plugin.to_owned(),
            line,
            icon: Some(icon.to_owned()),
            tooltip: tooltip.to_owned(),
            target: None,
        }
    }

    fn fetch(effects: Vec<GutterEffect>) -> GutterTicket {
        match effects.as_slice() {
            [GutterEffect::Fetch(ticket)] => *ticket,
            other => panic!("a fetch: {other:?}"),
        }
    }

    /// Runs the gutter once for version 1 of a document of `lines` lines and answers with `markers`.
    fn shown(gutter: &mut LineMarkerGutter, lines: usize, markers: Vec<LineMarkerInfo>, icons: &MarkerIcons) -> bool {
        let ticket = fetch(gutter.run(1));
        gutter.answered(ticket, 1, lines, true, Ok(markers), icons)
    }

    #[test]
    fn withdraws_old_interactive_markers_on_reload_and_does_not_refresh_on_idle_status_changes() {
        let icons = MarkerIcons::default();
        let (mut gutter, first) = LineMarkerGutter::new();
        assert_eq!(first, vec![GutterEffect::Schedule(DEBOUNCE_MS)]);
        assert!(shown(&mut gutter, 1, vec![marker("test", 0, "navigate", "old")], &icons));
        assert!(gutter.shown());
        assert_eq!(gutter.document_indexes_changed(), vec![GutterEffect::Schedule(DEBOUNCE_MS)]);
        assert!(!gutter.shown());
        assert!(gutter.markers().is_empty());
        assert!(!shown(&mut gutter, 1, Vec::new(), &icons));
        assert!(!gutter.shown());
    }

    #[test]
    fn renders_ordinary_and_custom_registered_icons_alongside_legacy_marker_aliases() {
        let mut names = PluginIcons::default();
        let circle = svg(r#"<circle cx="8" cy="8" r="6"/>"#);
        let markup = crate::plugin_models::plugin_icon::IconMarkup { light: &circle, dark: None };
        let (custom, registered) = names.name("markers", Some(markup));
        crate::theme::icons::register_external_icons(registered.into_iter().map(|(n, svg)| (n, svg.into_bytes())));
        let icons = MarkerIcons::default();
        let (mut gutter, _) = LineMarkerGutter::new();
        let markers = ["run", "database", custom.as_str()]
            .iter()
            .enumerate()
            .map(|(line, icon)| marker("markers", line as u32, icon, &format!("marker {line}")))
            .collect();
        assert!(shown(&mut gutter, 3, markers, &icons));
        let placed: Vec<(&str, &str)> =
            gutter.markers().iter().map(|m| (m.marker.tooltip.as_str(), m.icon.as_str())).collect();
        assert_eq!(placed, vec![("marker 0", "gutter-run"), ("marker 1", "database"), ("marker 2", custom.as_str())]);
    }

    #[test]
    fn refreshes_a_fallback_when_the_package_icon_declaration_arrives_after_its_markers() {
        let (mut icons, mut names) = (MarkerIcons::default(), PluginIcons::default());
        let (mut gutter, _) = LineMarkerGutter::new();
        let late = || vec![marker("late", 0, "icons/late.svg", "late icon")];
        assert!(shown(&mut gutter, 1, late(), &icons));
        assert_eq!(gutter.markers()[0].icon, "gutter-implementing");
        let read = icons.project("late", decl("late", "icons/late.svg", None));
        let assets = vec![("markerIcon/late/light".to_owned(), svg(r#"<circle cx="8" cy="8" r="6"/>"#))];
        let (registered, changed) = icons.assets_answered(read.handle, Ok(assets), &mut names);
        crate::theme::icons::register_external_icons(registered.into_iter().map(|(n, svg)| (n, svg.into_bytes())));
        assert!(changed);
        assert_eq!(gutter.marker_icons_changed(), vec![GutterEffect::Schedule(DEBOUNCE_MS)]);
        assert!(shown(&mut gutter, 1, late(), &icons));
        assert!(gutter.markers()[0].icon.starts_with("ext:pluginIcon/late/"), "{}", gutter.markers()[0].icon);
    }

    #[test]
    fn places_markers_on_existing_lines_and_drops_an_answer_for_an_older_version() {
        let icons = MarkerIcons::default();
        let (mut gutter, _) = LineMarkerGutter::new();
        let ticket = fetch(gutter.run(1));
        let markers = vec![marker("p", 0, "run", "a"), marker("p", 5, "run", "b")];
        assert!(!gutter.answered(ticket, 2, 2, true, Ok(markers.clone()), &icons), "the document moved on");
        let ticket = fetch(gutter.run(2));
        assert!(gutter.answered(ticket, 2, 2, true, Ok(markers), &icons));
        assert_eq!(gutter.markers().len(), 1);
        gutter.doc_changed(|line| line + 1);
        assert_eq!(gutter.markers()[0].line, 1);
    }
}
