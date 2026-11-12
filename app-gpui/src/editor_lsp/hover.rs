//! Hover: the problems and quick documentation at the pointer, which yield to the navigation
//! modifier (⌘ on macOS, Ctrl elsewhere) and to inlay hints, and the definition preview's lines.
//!
//! The modifier is followed window-wide, from keys and from pointer events (a key released
//! outside the window never reports its key-up): while it is held alone the ⌘-hover link and its
//! brief info own the pointer, so a plain hover is not shown, an open one closes, and an answer
//! that arrives meanwhile is dropped. A popup the pointer is in stays (to select and copy).

use super::diagnostics::LintDiagnostic;
use super::positions::range_to_offsets;
use crate::editor::buffer;
use ide_core::{Location, Range};
use ide_lsp::types::Hover;
use ropey::Rope;

/// The modifier keys of an event.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub meta: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

/// ⌘ (macOS) or Ctrl alone: not with ⌥ or ⇧ (other gestures), nor with the other platform's key.
pub fn is_navigation_gesture(modifiers: Modifiers, mac: bool) -> bool {
    let (primary, other) = if mac { (modifiers.meta, modifiers.ctrl) } else { (modifiers.ctrl, modifiers.meta) };
    primary && !other && !modifiers.alt && !modifiers.shift
}

/// What the pointer was over at its last move.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PointerTarget {
    #[default]
    None,
    /// A hover popup or another tooltip.
    Tooltip,
    /// An inlay hint, which has its own tooltip.
    Inlay,
    Other,
}

/// The navigation modifier as a window-wide state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NavigationModifier {
    pub mac: bool,
    held: bool,
    pointer: PointerTarget,
}

impl NavigationModifier {
    pub fn new(mac: bool) -> Self {
        Self { mac, ..Self::default() }
    }

    pub fn held(&self) -> bool {
        self.held
    }

    pub fn pointer(&self) -> PointerTarget {
        self.pointer
    }

    fn set(&mut self, held: bool) -> Option<bool> {
        (self.held != held).then(|| {
            self.held = held;
            held
        })
    }

    /// A key went down or up; returns the new state when it changed.
    pub fn key(&mut self, modifiers: Modifiers) -> Option<bool> {
        self.set(is_navigation_gesture(modifiers, self.mac))
    }

    /// The pointer moved or was pressed over `target`; returns the new state when it changed.
    pub fn pointer_event(&mut self, modifiers: Modifiers, target: PointerTarget) -> Option<bool> {
        self.pointer = target;
        self.set(is_navigation_gesture(modifiers, self.mac))
    }

    /// The window lost focus: the key's release will not be seen.
    pub fn blur(&mut self) -> Option<bool> {
        self.set(false)
    }

    /// Whether the plain hover stays out.
    pub fn hover_yields(&self) -> bool {
        self.held || self.pointer == PointerTarget::Inlay
    }

    /// The modifier changed: whether an open hover popup closes (not one the pointer is in).
    pub fn closes_hover(&self, held: bool, popup_open: bool) -> bool {
        held && popup_open && self.pointer != PointerTarget::Tooltip
    }
}

/// A hover popup over `from..to`: the problems there, then the documentation.
#[derive(Debug, Clone)]
pub struct HoverPopup {
    pub from: u32,
    pub to: u32,
    pub diagnostics: Vec<LintDiagnostic>,
    pub hover: Option<Hover>,
}

/// The popup at `pos` once the documentation request (if Quick documentation on hover asked
/// for it) answered; `None` when the hover yields by now or there is nothing to show. The popup
/// spans the documented range, else the first problem, else the word.
pub fn hover_popup(
    modifier: &NavigationModifier,
    doc: &Rope,
    pos: u32,
    diagnostics: Vec<LintDiagnostic>,
    hover: Option<Hover>,
) -> Option<HoverPopup> {
    if modifier.hover_yields() || (diagnostics.is_empty() && hover.is_none()) {
        return None;
    }
    let range = if let Some(range) = hover.as_ref().and_then(|hover| hover.range) {
        range_to_offsets(doc, range)
    } else if let Some(first) = diagnostics.first() {
        first.from..first.to
    } else {
        buffer::word_at(doc, pos)
    };
    Some(HoverPopup { from: range.start, to: range.end, diagnostics, hover })
}

/// Lines shown before the target when the declaration's extent is unknown.
const DEFINITION_CONTEXT_BEFORE: u32 = 1;
/// At most this many lines around a target whose declaration's extent is unknown.
const DEFINITION_MAX_LINES: u32 = 30;
/// A declaration taller than this is cut.
const DECLARATION_MAX_LINES: u32 = 400;

/// The lines (first, last; 0-based) to show for a definition: its whole declaration when the
/// syntax tree knows it, else a window around the name.
pub fn definition_span(location: &Location, line_count: u32, declaration: Option<Range>) -> (u32, u32) {
    let start = location.range.start.line;
    let last_line = line_count.saturating_sub(1);
    if let Some(declaration) = declaration.filter(|d| d.start.line <= start && start <= d.end.line) {
        let first = declaration.start.line;
        return (first, last_line.min(declaration.end.line).min(first + DECLARATION_MAX_LINES - 1));
    }
    let first = start.saturating_sub(DEFINITION_CONTEXT_BEFORE);
    let span_end = location.range.end.line.max(start + 12);
    (first, last_line.min(span_end).min(first + DEFINITION_MAX_LINES - 1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_core::Pos;
    use ide_lsp::types::Markup;

    /// The editor of the spec, "query.FlightPlan", and the documentation of `query`.
    fn hovered(modifier: &NavigationModifier) -> Option<HoverPopup> {
        let doc = Rope::from_str("query.FlightPlan");
        // The hover is asked only when it does not yield.
        let hover = (!modifier.hover_yields()).then(|| Hover {
            contents: Markup { kind: "markdown".into(), value: "package query".into() },
            range: None,
        });
        hover_popup(modifier, &doc, 2, Vec::new(), hover)
    }

    /// Both platforms: (mac, its modifier, the other platform's).
    fn platforms() -> [(bool, Modifiers, Modifiers); 2] {
        let ctrl = Modifiers { ctrl: true, ..Modifiers::default() };
        let meta = Modifiers { meta: true, ..Modifiers::default() };
        [(false, ctrl, meta), (true, meta, ctrl)]
    }

    #[test]
    fn tracks_the_platforms_modifier_alone_pressed_and_released() {
        for (mac, own, other) in platforms() {
            let mut modifier = NavigationModifier::new(mac);
            assert!(!modifier.held());
            assert_eq!(modifier.key(own), Some(true));
            assert_eq!(modifier.key(Modifiers::default()), Some(false));
            // With ⇧ or ⌥ it is another gesture; the other platform's key is not it.
            modifier.key(Modifiers { shift: true, ..own });
            assert!(!modifier.held());
            modifier.key(other);
            assert!(!modifier.held());
            let both = Modifiers { meta: true, ctrl: true, ..Modifiers::default() };
            assert!(!is_navigation_gesture(both, mac));
            assert!(is_navigation_gesture(own, mac));
        }
    }

    #[test]
    fn learns_the_state_from_mouse_moves_and_forgets_it_when_the_window_loses_focus() {
        for (mac, own, _) in platforms() {
            let mut modifier = NavigationModifier::new(mac);
            modifier.pointer_event(own, PointerTarget::Other);
            assert!(modifier.held());
            modifier.blur();
            assert!(!modifier.held());
        }
    }

    #[test]
    fn modifier_held_before_hovering_no_documentation_popup() {
        for (mac, own, _) in platforms() {
            let mut modifier = NavigationModifier::new(mac);
            assert!(hovered(&modifier).is_some());
            modifier.key(own);
            assert!(hovered(&modifier).is_none());
        }
    }

    #[test]
    fn modifier_pressed_while_the_documentation_popup_is_open_it_closes() {
        for (mac, own, _) in platforms() {
            let mut modifier = NavigationModifier::new(mac);
            let open = hovered(&modifier).is_some();
            let held = modifier.key(own).expect("the state changed");
            assert!(modifier.closes_hover(held, open));
        }
    }

    #[test]
    fn modifier_pressed_while_the_request_is_in_flight_the_late_answer_is_dropped() {
        for (mac, own, _) in platforms() {
            let mut modifier = NavigationModifier::new(mac);
            assert!(!modifier.hover_yields());
            // Asked; the key goes down before the answer.
            modifier.key(own);
            let answer = Hover { contents: Markup { kind: "markdown".into(), value: "late".into() }, range: None };
            assert!(hover_popup(&modifier, &Rope::from_str("query"), 2, Vec::new(), Some(answer)).is_none());
        }
    }

    #[test]
    fn modifier_released_nothing_stale_is_left_and_the_next_hover_shows_documentation_again() {
        for (mac, own, _) in platforms() {
            let mut modifier = NavigationModifier::new(mac);
            modifier.key(own);
            assert!(hovered(&modifier).is_none());
            let held = modifier.key(Modifiers::default()).expect("the state changed");
            assert!(!modifier.closes_hover(held, false));
            assert!(hovered(&modifier).is_some());
        }
    }

    #[test]
    fn keeps_a_popup_the_pointer_is_in() {
        for (mac, own, _) in platforms() {
            let mut modifier = NavigationModifier::new(mac);
            let open = hovered(&modifier).is_some();
            modifier.pointer_event(Modifiers::default(), PointerTarget::Tooltip);
            assert_eq!(modifier.pointer(), PointerTarget::Tooltip);
            let held = modifier.key(own).expect("the state changed");
            assert!(!modifier.closes_hover(held, open));
        }
    }

    #[test]
    fn no_documentation_popup_over_an_inlay_hint() {
        for (mac, _, _) in platforms() {
            let mut modifier = NavigationModifier::new(mac);
            modifier.pointer_event(Modifiers::default(), PointerTarget::Inlay);
            assert!(hovered(&modifier).is_none());
        }
    }

    fn location() -> Location {
        Location { path: "/a.ts".into(), range: Range::new(Pos::new(40, 9), Pos::new(40, 12)), uri: None }
    }

    fn range(a: u32, b: u32) -> Range {
        Range::new(Pos::new(a, 0), Pos::new(b, 1))
    }

    #[test]
    fn shows_the_whole_enclosing_declaration_when_the_syntax_tree_knows_it() {
        assert_eq!(definition_span(&location(), 500, Some(range(38, 120))), (38, 120));
    }

    #[test]
    fn falls_back_to_a_window_around_the_name_otherwise() {
        assert_eq!(definition_span(&location(), 500, None), (39, 52));
        assert_eq!(definition_span(&location(), 500, Some(range(0, 10))), (39, 52));
    }
}
