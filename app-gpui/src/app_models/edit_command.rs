//! Edit › Undo, Redo, Cut, Copy, Paste and Select All on Windows and Linux (macOS uses the
//! native menu roles): they act on what has the focus. Undo and Redo in the editor area go to the
//! document's own history first (notebooks and other custom documents keep theirs in the
//! backend), then to the focused editor's, which also holds programmatic edits (templates,
//! completion, refactorings).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditCommand {
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
}

/// Where the focus is when the command runs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EditFocus {
    /// Inside the editor area or one of its panes.
    pub in_editor_area: bool,
    /// A text editor has the focus.
    pub editor: bool,
}

/// Who carries the command out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditRoute {
    /// The active document's own history took it.
    DocumentHistory,
    /// The focused editor's history.
    EditorHistory,
    /// The focused view's own undo handler, else the platform's command.
    Focused,
}

/// Routes `command`; `document_history` asks the active document to undo or redo and says whether
/// it did.
pub fn route_edit_command(
    command: EditCommand,
    focus: EditFocus,
    document_history: impl FnOnce(EditCommand) -> bool,
) -> EditRoute {
    if !matches!(command, EditCommand::Undo | EditCommand::Redo) {
        return EditRoute::Focused;
    }
    if focus.in_editor_area && document_history(command) {
        return EditRoute::DocumentHistory;
    }
    if focus.editor { EditRoute::EditorHistory } else { EditRoute::Focused }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Edit menu Undo / Redo › routes custom document history before the editor's independent local history
    #[test]
    fn routes_custom_document_history_before_the_editors_independent_local_history() {
        let focus = EditFocus { in_editor_area: true, editor: true };
        let mut asked = Vec::new();
        for command in [EditCommand::Undo, EditCommand::Redo] {
            let route = route_edit_command(command, focus, |command| {
                asked.push(command);
                true
            });
            assert_eq!(route, EditRoute::DocumentHistory);
        }
        assert_eq!(asked, [EditCommand::Undo, EditCommand::Redo]);
        // A document without a history of its own leaves it to the editor.
        assert_eq!(route_edit_command(EditCommand::Undo, focus, |_| false), EditRoute::EditorHistory);
        let outside = EditFocus { in_editor_area: false, editor: true };
        assert_eq!(route_edit_command(EditCommand::Undo, outside, |_| panic!("not asked")), EditRoute::EditorHistory);
        assert_eq!(route_edit_command(EditCommand::Paste, focus, |_| panic!("not asked")), EditRoute::Focused);
    }
}
