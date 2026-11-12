//! Plugin language visuals of one editor: embedded-language highlighting ranges and gutter actions
//! (Run, Debug …) that run a command of the plugin's language feature. A line's actions share one
//! icon: a click runs a single action, several open a menu, as IntelliJ's run gutter does. The
//! visuals are asked for after each edit (debounced) and an answer for an older document is dropped.

use super::line_markers::marker_icon;
use super::marker_icons::{MarkerIcons, Resolved};
use crate::theme::icons::icon_path;
use ide_host::ops::lsp::PluginLanguageVisuals;
use ide_lsp::types::Command;
use ide_plugin::language_features::{GutterAction, Injection};
use std::collections::BTreeMap;

/// Delay between an edit and the next request.
pub const DEBOUNCE_MS: u64 = 300;

/// A gutter action of a plugin.
#[derive(Clone, Debug, PartialEq)]
pub struct LineAction {
    pub plugin_id: String,
    pub action: GutterAction,
}

impl LineAction {
    /// The command it runs through the document's language features (`lsp_execute_command`).
    pub fn command(&self) -> Command {
        let argument = serde_json::to_string(&self.action.argument).unwrap_or_else(|_| "null".to_owned());
        Command {
            title: self.action.tooltip.clone(),
            command: format!("openidea.plugin-language:{}:{}", self.plugin_id, self.action.command),
            arguments: vec![serde_json::Value::String(argument)],
        }
    }

    /// The icon of its menu item: a registered icon name only.
    pub fn menu_icon(&self) -> Option<&str> {
        self.action.icon.as_deref().filter(|icon| icon_path(icon).is_some())
    }
}

/// The actions of one line behind one icon.
#[derive(Clone, Debug, PartialEq)]
pub struct ActionMarker {
    /// 0-based.
    pub line: usize,
    /// The icon of the first action.
    pub icon: String,
    /// Every action's tooltip, one per line.
    pub title: String,
    pub actions: Vec<LineAction>,
}

/// What a click on an action marker does.
#[derive(Clone, Debug)]
pub enum ActionClick {
    Execute(Command),
    /// A menu of `(label, icon, command)`.
    Menu(Vec<(String, Option<String>, Command)>),
}

impl ActionMarker {
    pub fn click(&self) -> Option<ActionClick> {
        match self.actions.as_slice() {
            [] => None,
            [only] => Some(ActionClick::Execute(only.command())),
            several => Some(ActionClick::Menu(
                several
                    .iter()
                    .map(|a| (a.action.tooltip.clone(), a.menu_icon().map(str::to_owned), a.command()))
                    .collect(),
            )),
        }
    }
}

/// The actions on lines of a document of `line_count` lines, grouped by line in line order, with
/// declared icons resolved.
pub fn action_markers(line_count: usize, actions: &[(String, GutterAction)], icons: &MarkerIcons) -> Vec<ActionMarker> {
    let mut by_line: BTreeMap<usize, Vec<LineAction>> = BTreeMap::new();
    for (plugin_id, action) in actions.iter().filter(|(_, a)| (a.line as usize) < line_count) {
        let icon = match icons.resolve_marker_icon(plugin_id, action.icon.as_deref()) {
            Resolved::Ready(icon) => icon,
            Resolved::Pending => action.icon.clone(),
        };
        let action = GutterAction { icon, ..action.clone() };
        by_line.entry(action.line as usize).or_default().push(LineAction { plugin_id: plugin_id.clone(), action });
    }
    by_line
        .into_iter()
        .map(|(line, actions)| ActionMarker {
            line,
            icon: marker_icon(actions[0].action.icon.as_deref()),
            title: actions.iter().map(|a| a.action.tooltip.as_str()).collect::<Vec<_>>().join("\n"),
            actions,
        })
        .collect()
}

/// A request for the editor's visuals.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VisualsTicket {
    generation: u64,
    /// The document version asked about.
    pub version: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VisualsEffect {
    /// Call [`LanguageVisuals::run`] after this many milliseconds (replacing a pending call).
    Schedule(u64),
    /// `plugin_language_visuals(doc)`; answer with [`LanguageVisuals::answered`].
    Fetch(VisualsTicket),
}

/// The language visuals of one editor.
#[derive(Debug, Default)]
pub struct LanguageVisuals {
    generation: u64,
    injections: Vec<Injection>,
    markers: Vec<ActionMarker>,
    skipped: bool,
}

impl LanguageVisuals {
    /// Visuals for a new editor; the first request is scheduled.
    pub fn new() -> (Self, Vec<VisualsEffect>) {
        (Self::default(), vec![VisualsEffect::Schedule(DEBOUNCE_MS)])
    }

    /// Ranges to highlight with an embedded language's grammar.
    pub fn injections(&self) -> &[Injection] {
        &self.injections
    }

    pub fn markers(&self) -> &[ActionMarker] {
        &self.markers
    }

    /// The document changed: what was shown is stale.
    pub fn doc_changed(&mut self) -> Vec<VisualsEffect> {
        self.generation += 1;
        self.injections.clear();
        self.markers.clear();
        vec![VisualsEffect::Schedule(DEBOUNCE_MS)]
    }

    /// The editor providers changed (a plugin came or went): everything goes at once.
    pub fn providers_changed(&mut self) -> Vec<VisualsEffect> {
        self.doc_changed()
    }

    /// A marker icon was declared or loaded.
    pub fn marker_icons_changed(&mut self) -> Vec<VisualsEffect> {
        self.generation += 1;
        vec![VisualsEffect::Schedule(DEBOUNCE_MS)]
    }

    /// The editor is visible again; an answer skipped while hidden is asked for again.
    pub fn shown_again(&mut self) -> Vec<VisualsEffect> {
        if self.skipped { self.marker_icons_changed() } else { Vec::new() }
    }

    /// The debounce elapsed: asks about document version `version`.
    pub fn run(&mut self, version: u64) -> Vec<VisualsEffect> {
        vec![VisualsEffect::Fetch(VisualsTicket { generation: self.generation, version })]
    }

    /// The visuals for `ticket`, for a document now at `version` with `line_count` lines, shown
    /// (`visible`) or not. A failure clears them. Returns whether they changed.
    pub fn answered(
        &mut self,
        ticket: VisualsTicket,
        version: u64,
        line_count: usize,
        visible: bool,
        result: Result<PluginLanguageVisuals, String>,
        icons: &MarkerIcons,
    ) -> bool {
        if ticket.generation != self.generation || ticket.version != version {
            return false;
        }
        let visuals = match result {
            Ok(visuals) => visuals,
            Err(_) => {
                let changed = !self.injections.is_empty() || !self.markers.is_empty();
                self.injections.clear();
                self.markers.clear();
                return changed;
            }
        };
        self.skipped = !visible;
        if self.skipped {
            return false;
        }
        self.markers = action_markers(line_count, &visuals.gutter_actions, icons);
        self.injections = visuals.injections;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn action(icon: &str, tooltip: &str, command: &str, argument: &str) -> GutterAction {
        GutterAction {
            line: 0,
            icon: Some(icon.to_owned()),
            tooltip: tooltip.to_owned(),
            command: command.to_owned(),
            argument: Some(argument.to_owned()),
        }
    }

    fn visuals(actions: Vec<(String, GutterAction)>) -> PluginLanguageVisuals {
        PluginLanguageVisuals { version: 1, injections: Vec::new(), colors: Vec::new(), gutter_actions: actions }
    }

    fn fetch(effects: Vec<VisualsEffect>) -> VisualsTicket {
        match effects.as_slice() {
            [VisualsEffect::Fetch(ticket)] => *ticket,
            other => panic!("a fetch: {other:?}"),
        }
    }

    #[test]
    fn renders_owned_gutter_commands_and_drops_answers_to_superseded_buffers() {
        let icons = MarkerIcons::default();
        let (mut view, _) = LanguageVisuals::new();
        let run = || visuals(vec![("test".to_owned(), action("run", "Run test", "run", "value"))]);
        let old = fetch(view.run(1));
        view.doc_changed();
        assert!(!view.answered(old, 2, 1, true, Ok(run()), &icons), "the answer is for the old buffer");
        assert!(view.markers().is_empty());
        let current = fetch(view.run(2));
        assert!(view.answered(current, 2, 1, true, Ok(run()), &icons));
        let [marker] = view.markers() else { panic!("one marker") };
        assert_eq!(marker.title, "Run test");
        let Some(ActionClick::Execute(command)) = marker.click() else { panic!("runs the action") };
        assert_eq!(command.command, "openidea.plugin-language:test:run");
        assert_eq!(command.arguments, vec![json!("\"value\"")]);
    }

    #[test]
    fn puts_the_actions_of_one_line_behind_one_icon_whose_click_opens_a_menu() {
        let icons = MarkerIcons::default();
        let (mut view, _) = LanguageVisuals::new();
        let both = visuals(vec![
            ("test".to_owned(), action("run", "Run 'T'", "run", "x")),
            ("test".to_owned(), action("debug", "Debug 'T'", "debug", "x")),
        ]);
        let ticket = fetch(view.run(1));
        assert!(view.answered(ticket, 1, 1, true, Ok(both), &icons));
        let [marker] = view.markers() else { panic!("one marker") };
        let Some(ActionClick::Menu(items)) = marker.click() else { panic!("a menu") };
        let labels: Vec<(&str, Option<&str>)> = items.iter().map(|(l, i, _)| (l.as_str(), i.as_deref())).collect();
        assert_eq!(labels, vec![("Run 'T'", Some("run")), ("Debug 'T'", Some("debug"))]);
        assert_eq!(items[1].2.command, "openidea.plugin-language:test:debug");
    }
}
