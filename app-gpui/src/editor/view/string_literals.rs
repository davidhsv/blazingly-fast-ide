//! Copy and paste inside string literals: an ordinary paste into a plain string literal is escaped for
//! it, and a copy or cut of text inside one puts the unescaped text on the clipboard. Paste as Plain
//! Text, typing and anything else that edits the text leave it as it is. Smart Keys › Escape text on
//! paste turns both off.

use super::Editor;
use crate::editor_commands::string_clipboard::{escape_paste, unescape_copied};
use crate::editor_commands::{Edit, EditState};

impl Editor {
    /// `paste`, a paste of the clipboard computed on `state`, escaped for the plain string literals its
    /// ranges are in; unchanged where they are not in one.
    pub(super) fn escaped_paste(&self, state: &EditState, paste: &Edit) -> Edit {
        escape_paste(state, &self.literal_tree(), self.language.as_str(), self.facts.escape_on_paste, paste)
    }

    /// `text`, as copied from `state`, unescaped when every range lies inside a plain string literal.
    pub(super) fn unescaped_copy(&self, state: &EditState, text: &str) -> String {
        unescape_copied(state, &self.literal_tree(), self.language.as_str(), self.facts.escape_on_paste, text)
    }
}
