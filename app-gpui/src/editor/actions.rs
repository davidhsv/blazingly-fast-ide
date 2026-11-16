//! The editor's entries in the action registry: what menus, the keymap and Find Action run on
//! the focused editor. Caret movement and typing are the editor's own key handling, as they are
//! CodeMirror's base keymap in the webview UI.

use super::view::Editor;
use crate::platform::actions::ActionRegistry;
use crate::workbench;
use gpui_kit::{App, Context, Window};

/// Runs `act` on the editor of the window an action was invoked in.
pub(super) fn on_editor(
    window: &mut Window,
    cx: &mut App,
    act: impl FnOnce(&mut Editor, &mut Context<Editor>),
) -> anyhow::Result<()> {
    let editor =
        workbench::active_editor(window, cx).ok_or_else(|| anyhow::anyhow!("no editor is open in this window"))?;
    editor.update(cx, act);
    Ok(())
}

pub fn register(cx: &mut App) {
    let registry = cx.default_global::<ActionRegistry>();
    registry.handle("edit.undo", |call, window, cx| {
        // While the Project view has the keyboard, Undo reverts the view's last delete or move first.
        if call.ctx.is_truthy("projectViewFocus") && workbench::undo_in_project_view(window, cx) {
            return Ok(());
        }
        // With nothing of its own left to undo, the editor undoes the refactoring of its file.
        if workbench::undo_refactoring(window, cx) {
            return Ok(());
        }
        on_editor(window, cx, |editor, cx| {
            editor.undo(cx);
        })
    });
    registry.handle("edit.redo", |_, window, cx| {
        if workbench::redo_refactoring(window, cx) {
            return Ok(());
        }
        on_editor(window, cx, |editor, cx| {
            editor.redo(cx);
        })
    });
    registry.handle("edit.copy", |_, window, cx| {
        on_editor(window, cx, |editor, cx| {
            editor.copy(cx);
        })
    });
    registry.handle("edit.cut", |_, window, cx| {
        on_editor(window, cx, |editor, cx| {
            editor.cut(cx);
        })
    });
    registry.handle("edit.paste", |_, window, cx| {
        on_editor(window, cx, |editor, cx| {
            editor.paste(cx);
        })
    });
    registry.handle("edit.selectAll", |_, window, cx| on_editor(window, cx, |editor, cx| editor.select_all(cx)));
    // Save All: every unsaved document of the window, whichever tab is in front; the writes run on
    // the backend and a failure is told to the user once it is known.
    registry.handle("file.save", |_, window, cx| {
        let workbench =
            workbench::for_window(window, cx).ok_or_else(|| anyhow::anyhow!("this window has no documents"))?;
        workbench.update(cx, |workbench, cx| workbench.save_all_documents(cx));
        Ok(())
    });
    super::command_actions::register(cx);
    super::core_actions::register(cx);
    super::intelligence::register(cx);
}
