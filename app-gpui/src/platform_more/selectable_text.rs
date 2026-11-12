//! Read-only text the user can select and copy, like the Markdown preview, Quick Documentation
//! and message panes. The workbench chrome is not selectable; content panes are. A right-click
//! on a selection, or anywhere in such a pane, offers Copy.

use crate::platform::dispatcher::Mods;
use crate::platform::keymap::format_binding;

/// The text selected in the window when a context menu is asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection<'a> {
    pub text: &'a str,
    /// The selection reaches the clicked element (it is inside it, or the element is part of
    /// the selected range).
    pub covers_target: bool,
}

/// What a right-click landed on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CopyTarget<'a> {
    /// It is (inside) an editable field, which has a menu of its own.
    pub editable: bool,
    /// It is inside a pane of selectable read-only text.
    pub selectable: bool,
    pub selection: Option<Selection<'a>>,
}

/// The Copy row of a context menu. Running it copies `text`: what was selected when the menu
/// opened, whatever is selected by the time the row is chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopyItem {
    pub id: &'static str,
    /// English source text of the label.
    pub label: &'static str,
    pub icon: &'static str,
    pub shortcut: String,
    pub disabled: bool,
    pub text: String,
}

/// The Copy menu for a right-click on `target`, or `None` when it is not on readable text: an
/// editable field, or chrome that no selection reaches.
pub fn copy_menu_for(target: &CopyTarget<'_>, mac: bool) -> Option<CopyItem> {
    if target.editable {
        return None;
    }
    let text = target.selection.filter(|selection| selection.covers_target).map_or("", |selection| selection.text);
    if text.is_empty() && !target.selectable {
        return None;
    }
    Some(CopyItem {
        id: "copy",
        label: "Copy",
        icon: "copy",
        shortcut: format_binding("Meta+C", mac),
        disabled: text.is_empty(),
        text: text.to_owned(),
    })
}

/// A key press in a pane of output text that is not an editor.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OutputKeyPress<'a> {
    /// The key as the keymap names it (`C`, `Insert`).
    pub key: &'a str,
    pub mods: Mods,
    /// Something handled the key already.
    pub handled: bool,
    /// An input method is composing with it.
    pub composing: bool,
    /// The focus is in an editable child of the pane.
    pub in_editable: bool,
    /// There is a non-empty selection and both of its ends are inside the pane.
    pub selection_in_pane: bool,
}

/// Whether the key copies the pane's selection: ⌘C / Ctrl+C or Ctrl+Insert, on a selection of
/// the pane itself. Editable children and selections that reach into other views keep the key.
pub fn copies_selection(press: &OutputKeyPress<'_>, mac: bool) -> bool {
    let mods = press.mods;
    if press.handled || press.composing || mods.alt || mods.shift {
        return false;
    }
    let primary = if mac { mods.meta } else { mods.ctrl };
    let copy = (press.key.eq_ignore_ascii_case("c") && (mods.ctrl || primary))
        || (press.key == "Insert" && mods.ctrl && !mods.meta);
    copy && !press.in_editable && press.selection_in_pane
}

#[cfg(test)]
mod tests {
    use super::*;

    fn selected(text: &str) -> Option<Selection<'_>> {
        Some(Selection { text, covers_target: true })
    }

    fn pane() -> CopyTarget<'static> {
        CopyTarget { selectable: true, ..CopyTarget::default() }
    }

    // copyMenuFor › offers Copy of the selection in a selectable pane
    #[test]
    fn offers_copy_of_the_selection_in_a_selectable_pane() {
        let item = copy_menu_for(&CopyTarget { selection: selected("Hello world"), ..pane() }, true);
        let item = item.expect("a selectable pane has a Copy row");
        assert_eq!((item.label, item.disabled), ("Copy", false));
        assert_eq!((item.id, item.icon, item.shortcut.as_str()), ("copy", "copy", "⌘C"));
    }

    // copyMenuFor › offers a disabled Copy in a selectable pane with nothing selected
    #[test]
    fn offers_a_disabled_copy_in_a_selectable_pane_with_nothing_selected() {
        let item = copy_menu_for(&pane(), true).expect("a selectable pane has a Copy row");
        assert_eq!((item.label, item.disabled), ("Copy", true));
    }

    // copyMenuFor › offers Copy on a selection in any pane that opted in with its own CSS
    #[test]
    fn offers_copy_on_a_selection_in_any_pane_that_opted_in_with_its_own_css() {
        let console = CopyTarget { selection: selected("line 1"), ..CopyTarget::default() };
        assert_eq!(copy_menu_for(&console, true).map(|item| item.disabled), Some(false));
    }

    // copyMenuFor › stays out of chrome without a selection
    #[test]
    fn stays_out_of_chrome_without_a_selection() {
        assert_eq!(copy_menu_for(&CopyTarget::default(), true), None);
    }

    // copyMenuFor › ignores a selection elsewhere when right-clicking chrome
    #[test]
    fn ignores_a_selection_elsewhere_when_right_clicking_chrome() {
        let elsewhere = Selection { text: "Hello", covers_target: false };
        assert_eq!(copy_menu_for(&CopyTarget { selection: Some(elsewhere), ..CopyTarget::default() }, true), None);
    }

    // copyMenuFor › leaves editable targets alone: %s
    #[test]
    fn leaves_editable_targets_alone() {
        for selection in [None, selected("x")] {
            assert_eq!(copy_menu_for(&CopyTarget { editable: true, selection, ..pane() }, true), None);
        }
    }

    // copyMenuFor › copies the text captured when the menu opened
    #[test]
    fn copies_the_text_captured_when_the_menu_opened() {
        let mut live = String::from("Hello");
        let item = copy_menu_for(&CopyTarget { selection: selected(&live), ..pane() }, true);
        // The selection goes away while the menu is open.
        live.clear();
        assert_eq!(item.map(|item| item.text).as_deref(), Some("Hello"));
    }

    // Not from a spec: which key presses copy the selection of an output pane.
    #[test]
    fn copies_the_panes_own_selection_on_the_copy_keys() {
        let press = |key: &'static str, mods: Mods| OutputKeyPress {
            key,
            mods,
            selection_in_pane: true,
            ..OutputKeyPress::default()
        };
        let meta = Mods { meta: true, ..Mods::NONE };
        let ctrl = Mods { ctrl: true, ..Mods::NONE };
        assert!(copies_selection(&press("c", meta), true));
        assert!(copies_selection(&press("C", ctrl), true));
        assert!(copies_selection(&press("C", ctrl), false));
        assert!(!copies_selection(&press("C", meta), false));
        assert!(copies_selection(&press("Insert", ctrl), false));
        assert!(!copies_selection(&press("Insert", Mods { meta: true, ..ctrl }), true));
        assert!(!copies_selection(&press("C", Mods { shift: true, ..ctrl }), false));
        assert!(!copies_selection(&press("V", ctrl), false));
        for blocked in [
            OutputKeyPress { handled: true, ..press("C", ctrl) },
            OutputKeyPress { composing: true, ..press("C", ctrl) },
            OutputKeyPress { in_editable: true, ..press("C", ctrl) },
            OutputKeyPress { selection_in_pane: false, ..press("C", ctrl) },
        ] {
            assert!(!copies_selection(&blocked, false), "{blocked:?}");
        }
    }
}
