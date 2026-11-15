//! The mouse gestures that select a block of columns: ⌥-drag with the left button, a plain drag
//! while Column Selection Mode is on, and a drag with the middle button. The middle button selects
//! a block everywhere but on Linux, where its press pastes the primary selection.

use super::Editor;
use gpui_kit::{Context, Modifiers, MouseDownEvent, MouseUpEvent, Window};

/// Whether a left-button press with `modifiers` starts a block selection: ⌥ alone, or no ⌥, ⌘ or
/// ⌃ (⇧ allowed) while Column Selection Mode is on.
pub(super) fn left_press_selects_block(modifiers: &Modifiers, column_mode: bool) -> bool {
    let bare = !modifiers.platform && !modifiers.control;
    let alt_drag = modifiers.alt && !modifiers.shift && bare;
    alt_drag || (column_mode && !modifiers.alt && bare)
}

/// Whether a middle-button press with `modifiers` starts a block selection: on every system but
/// Linux, without a modifier.
pub(super) fn middle_press_selects_block(modifiers: &Modifiers) -> bool {
    !cfg!(target_os = "linux") && !modifiers.alt && !modifiers.shift && !modifiers.platform && !modifiers.control
}

impl Editor {
    /// Whether this left-button press starts a block selection.
    pub(super) fn press_selects_block(&self, event: &MouseDownEvent) -> bool {
        event.click_count <= 1 && left_press_selects_block(&event.modifiers, self.memory.column_on)
    }

    /// A middle-button press over the text puts the caret there and starts a block selection that
    /// the drag grows; over the gutter, beside the editor, with a modifier or on Linux it does nothing.
    pub(super) fn middle_mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !middle_press_selects_block(&event.modifiers) || self.gutter_line_at(event.position).is_some() {
            return;
        }
        let Some(offset) = self.offset_at(event.position) else { return };
        window.focus(&self.focus, cx);
        (self.drag_anchor, self.adding) = (None, None);
        self.select(offset, offset, cx);
        self.column_drag = self.block_point_at(event.position);
    }

    /// The middle button is released: the block selection stays as dragged.
    pub(super) fn middle_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.column_drag = None;
    }

    /// Whether a move with `pressed` held goes on with a press of this editor: the left button's
    /// gestures, or the middle button's block selection.
    pub(super) fn drags_with(&self, pressed: Option<gpui_kit::MouseButton>) -> bool {
        use gpui_kit::MouseButton;
        match pressed {
            Some(MouseButton::Left) => true,
            Some(MouseButton::Middle) => self.column_drag.is_some(),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn modifiers(alt: bool, shift: bool, platform: bool, control: bool) -> Modifiers {
        Modifiers { alt, shift, platform, control, ..Modifiers::default() }
    }

    #[test]
    fn alt_alone_or_column_mode_without_alt_starts_a_left_button_block() {
        assert!(left_press_selects_block(&modifiers(true, false, false, false), false));
        assert!(!left_press_selects_block(&modifiers(true, true, false, false), false));
        assert!(!left_press_selects_block(&modifiers(false, false, false, false), false));
        assert!(left_press_selects_block(&modifiers(false, false, false, false), true));
        assert!(left_press_selects_block(&modifiers(false, true, false, false), true));
        assert!(!left_press_selects_block(&modifiers(false, false, true, false), true));
        assert!(!left_press_selects_block(&modifiers(false, false, false, true), true));
    }

    #[test]
    fn the_middle_button_starts_a_block_without_modifiers_except_on_linux() {
        assert_eq!(middle_press_selects_block(&Modifiers::default()), !cfg!(target_os = "linux"));
        for held in [
            modifiers(true, false, false, false),
            modifiers(false, true, false, false),
            modifiers(false, false, true, false),
            modifiers(false, false, false, true),
        ] {
            assert!(!middle_press_selects_block(&held), "{held:?}");
        }
    }
}
