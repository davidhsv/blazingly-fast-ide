//! Inlay hints for the visible lines, shown as subtle inline labels. A label part naming a
//! declaration navigates there on ⌘-click (Ctrl elsewhere); a plain type hint after an identifier
//! opens that identifier's type; right-click offers to turn the hint's kind off.

use super::completion_items::is_word_char;
use super::hover::{Modifiers, is_navigation_gesture};
use super::positions::pos_to_offset;
use crate::editor::buffer;
use crate::platform::i18n::{I18n, Param};
use ide_core::Location;
use ide_lsp::types::{InlayHint, InlayKind};
use ropey::Rope;

/// One part of a hint as shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlayPartView {
    pub text: String,
    pub tooltip: Option<String>,
    /// The declaration the part names: ⌘-click goes there.
    pub target: Option<Location>,
}

/// A hint as shown at a document offset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlayView {
    pub offset: u32,
    /// Parameter hints stick to the text after them, the others to the text before.
    pub after: bool,
    pub kind: InlayKind,
    pub padding_left: bool,
    pub padding_right: bool,
    /// The label as one piece when no part links or explains anything.
    pub label: String,
    /// The parts, when one of them links or has a tooltip.
    pub parts: Vec<InlayPartView>,
    /// A plain type hint after an identifier: ⌘-click opens the type of the identifier starting
    /// at this offset.
    pub type_link: Option<u32>,
    pub tooltip: Option<String>,
}

/// Where a type hint at `at` can resolve the inferred type from: the identifier it follows.
fn type_hint_offset(doc: &Rope, at: u32) -> Option<u32> {
    if at == 0 || at > buffer::len_utf16(doc) {
        return None;
    }
    let word = buffer::word_at(doc, at - 1);
    let identifier = buffer::slice(doc, word.start, word.end).chars().all(is_word_char) && word.start < word.end;
    (identifier && word.end == at).then_some(word.start)
}

/// The hints as shown in `doc`.
pub fn inlay_views(doc: &Rope, hints: &[InlayHint]) -> Vec<InlayView> {
    hints
        .iter()
        .map(|hint| {
            let offset = pos_to_offset(doc, hint.position);
            let explained = hint.parts.iter().any(|part| part.location.is_some() || part.tooltip.is_some());
            let parts = if explained {
                hint.parts
                    .iter()
                    .map(|part| InlayPartView {
                        text: part.value.clone(),
                        tooltip: part.tooltip.clone(),
                        target: part.location.clone(),
                    })
                    .collect()
            } else {
                Vec::new()
            };
            let linked = hint.parts.iter().any(|part| part.location.is_some());
            let type_link = (hint.kind == InlayKind::Type && !linked).then(|| type_hint_offset(doc, offset)).flatten();
            InlayView {
                offset,
                after: hint.kind == InlayKind::Parameter,
                kind: hint.kind,
                padding_left: hint.padding_left,
                padding_right: hint.padding_right,
                label: hint.label.clone(),
                parts,
                type_link,
                tooltip: hint.tooltip.clone(),
            }
        })
        .collect()
}

/// What a click on a hint does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InlayNavigation {
    /// Open the declaration a part names.
    Open(Location),
    /// Go to Type Declaration of the identifier at this offset.
    TypeDefinition(u32),
}

/// A primary-button press on part `part` of `view` (or on the hint itself), with `modifiers`.
pub fn inlay_click(view: &InlayView, part: Option<usize>, modifiers: Modifiers, mac: bool) -> Option<InlayNavigation> {
    if !is_navigation_gesture(modifiers, mac) {
        return None;
    }
    if let Some(target) = part.and_then(|part| view.parts.get(part)).and_then(|part| part.target.clone()) {
        return Some(InlayNavigation::Open(target));
    }
    view.type_link.map(InlayNavigation::TypeDefinition)
}

fn kind_label(kind: InlayKind) -> &'static str {
    match kind {
        InlayKind::Type => "Type hints",
        InlayKind::Parameter => "Parameter name hints",
        InlayKind::Other => "Other hints",
    }
}

/// A row of the hint's context menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InlayMenuItem {
    /// Turn hints of the kind off (`editor.inlay.<kind>` = false).
    Disable {
        label: String,
        kind: InlayKind,
    },
    Separator,
    /// Open Settings › Editor › Inlay Hints.
    Settings {
        label: String,
    },
}

pub fn inlay_menu_items(kind: InlayKind, i18n: &mut I18n) -> Vec<InlayMenuItem> {
    vec![
        InlayMenuItem::Disable { label: i18n.tx_built(&["Disable ", ""], &[Param::from(kind_label(kind))]), kind },
        InlayMenuItem::Separator,
        InlayMenuItem::Settings { label: i18n.tx("Inlay Hints Settings…") },
    ]
}

/// The Inlay Hints settings of the editor.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InlayPrefs {
    /// Settings › Editor › Show inlay hints.
    pub enabled: bool,
    pub types: bool,
    pub parameters: bool,
    pub other: bool,
    pub disabled_languages: Vec<String>,
}

impl InlayPrefs {
    /// Whether hints are asked for in a file of `language`.
    pub fn enabled_for(&self, language: &str) -> bool {
        self.enabled && !self.disabled_languages.iter().any(|disabled| disabled == language)
    }

    /// Whether hints of `kind` show.
    pub fn shows(&self, kind: InlayKind) -> bool {
        match kind {
            InlayKind::Type => self.types,
            InlayKind::Parameter => self.parameters,
            InlayKind::Other => self.other,
        }
    }
}

/// How long the hints wait after an edit or a scroll before they are asked for again.
pub const REFRESH_MS: u64 = 400;

/// The hints shown once an answer for the visible lines arrived, or `None` when the document
/// changed meanwhile (ask again).
pub fn inlay_answer(doc: &Rope, unchanged: bool, hints: &[InlayHint], prefs: &InlayPrefs) -> Option<Vec<InlayView>> {
    if !unchanged {
        return None;
    }
    let shown: Vec<InlayHint> = hints.iter().filter(|hint| prefs.shows(hint.kind)).cloned().collect();
    Some(inlay_views(doc, &shown))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor_lsp::hover::NavigationModifier;
    use ide_core::{Pos, Range};
    use ide_lsp::types::InlayLabelPart;

    fn foo() -> Location {
        Location { path: "/p/foo.rs".into(), range: Range::new(Pos::new(4, 7), Pos::new(4, 10)), uri: None }
    }

    fn hint() -> InlayHint {
        InlayHint {
            position: Pos::new(0, 5),
            label: ": Foo".into(),
            kind: InlayKind::Type,
            padding_left: false,
            padding_right: false,
            tooltip: None,
            parts: vec![
                InlayLabelPart { value: ": ".into(), tooltip: None, location: None },
                InlayLabelPart { value: "Foo".into(), tooltip: Some("struct Foo".into()), location: Some(foo()) },
            ],
        }
    }

    fn doc() -> Rope {
        Rope::from_str("let x = foo();")
    }

    fn with(modifiers: impl FnOnce(&mut Modifiers)) -> Modifiers {
        let mut out = Modifiers::default();
        modifiers(&mut out);
        out
    }

    #[test]
    fn renders_label_parts_cmd_click_on_a_part_with_a_location_navigates_there() {
        let views = inlay_views(&doc(), &[hint()]);
        let part = &views[0].parts[1];
        assert_eq!((part.text.as_str(), part.tooltip.as_deref()), ("Foo", Some("struct Foo")));
        assert_eq!(views[0].parts.iter().map(|p| p.text.as_str()).collect::<String>(), ": Foo");
        assert_eq!(inlay_click(&views[0], Some(1), Modifiers::default(), false), None);
        // ⌘ is not the navigation modifier off macOS.
        assert_eq!(inlay_click(&views[0], Some(1), with(|m| m.meta = true), false), None);
        assert_eq!(inlay_click(&views[0], Some(1), with(|m| m.ctrl = true), false), Some(InlayNavigation::Open(foo())));
    }

    #[test]
    fn offers_to_disable_hints_of_the_clicked_kind() {
        let mut i18n = I18n::default();
        let labels = |items: Vec<InlayMenuItem>| -> Vec<String> {
            items
                .into_iter()
                .map(|item| match item {
                    InlayMenuItem::Disable { label, .. } | InlayMenuItem::Settings { label } => label,
                    InlayMenuItem::Separator => "separator".into(),
                })
                .collect()
        };
        assert_eq!(
            labels(inlay_menu_items(InlayKind::Type, &mut i18n)),
            ["Disable Type hints", "separator", "Inlay Hints Settings…"]
        );
        assert_eq!(labels(inlay_menu_items(InlayKind::Parameter, &mut i18n))[0], "Disable Parameter name hints");
    }

    #[test]
    fn uses_type_declaration_navigation_for_a_plain_type_hint_after_an_identifier() {
        let views = inlay_views(&doc(), &[InlayHint { parts: Vec::new(), ..hint() }]);
        assert_eq!(inlay_click(&views[0], None, Modifiers::default(), true), None);
        assert_eq!(
            inlay_click(&views[0], None, with(|m| m.meta = true), true),
            Some(InlayNavigation::TypeDefinition(4))
        );
    }

    #[test]
    fn does_not_treat_parameter_hints_or_type_hints_away_from_an_identifier_as_type_links() {
        let views = inlay_views(
            &doc(),
            &[
                InlayHint { kind: InlayKind::Parameter, parts: Vec::new(), ..hint() },
                InlayHint { position: Pos::new(0, 6), parts: Vec::new(), ..hint() },
            ],
        );
        assert!(views.iter().all(|view| view.type_link.is_none() && view.parts.is_empty()));
    }

    #[test]
    fn tracks_the_navigation_modifier_for_inlay_cursors() {
        for (mac, modifiers) in [(true, with(|m| m.meta = true)), (false, with(|m| m.ctrl = true))] {
            let mut modifier = NavigationModifier::new(mac);
            assert!(!modifier.held());
            modifier.key(modifiers);
            assert!(modifier.held());
            modifier.key(Modifiers::default());
            assert!(!modifier.held());
        }
    }
}
