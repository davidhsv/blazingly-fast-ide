//! Update Tag with Emmet (`emmet.updateTag`) in a markup editor: the tag around the caret is
//! renamed, given classes, an id and attributes from an abbreviation the user types
//! (`section.card#main[data-x=1]`). The editor's menu offers it for HTML-like files through the
//! `templates.emmet` group.

use super::Workbench;
use super::notifications::{NotificationLevel, NotificationRequest};
use super::services_window::on_workbench;
use crate::editor::buffer::{Assoc, map_offset};
use crate::editor::view::Editor;
use crate::platform::actions::ActionRegistry;
use crate::platform::i18n::{Localization, Param};
use crate::templates::emmet_model::{enclosing_tag, update_tag_edits};
use crate::templates::update_tag_dialog::{UpdateTagDialog, UpdateTagEvent};
use gpui_kit::{App, AppContext as _, Context, Entity, Focusable as _, Window};

pub(super) const UPDATE_TAG: &str = "emmet.updateTag";
/// One notification at a time for the action: the next replaces it.
const NOTICE_KEY: &str = "emmet-update-tag";

pub(super) fn register_actions(registry: &mut ActionRegistry) {
    registry.handle(UPDATE_TAG, |_, window, cx| {
        on_workbench(window, cx, |workbench, window, cx| workbench.update_tag_with_emmet(window, cx))
    });
}

/// Applies `abbreviation` to the tag around the caret of `editor`, the selections following the
/// edit; false when it describes no single element or there is no tag any more.
fn apply_abbreviation(editor: &Entity<Editor>, abbreviation: &str, cx: &mut App) -> bool {
    editor.update(cx, |editor, cx| {
        let text = editor.text();
        let Some(edits) = enclosing_tag(&text, editor.caret()).and_then(|tag| update_tag_edits(&tag, abbreviation))
        else {
            return false;
        };
        let map = |offset| map_offset(offset, &edits, Assoc::Before);
        let selections: Vec<(u32, u32)> =
            editor.selections().iter().map(|&(anchor, head)| (map(anchor), map(head))).collect();
        let main = editor.main_index();
        editor.apply_edit(&edits, selections, main, cx)
    })
}

impl Workbench {
    /// The prompt of Update Tag with Emmet, while it is open.
    pub fn update_tag_dialog(&self) -> Option<Entity<UpdateTagDialog>> {
        self.vcs_dialog_view::<UpdateTagDialog>()
    }

    /// Update Tag with Emmet: asks for the abbreviation of the tag around the active editor's caret
    /// and applies it.
    fn update_tag_with_emmet(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.editor().cloned() else { return };
        let tag = {
            let editor = editor.read(cx);
            enclosing_tag(&editor.text(), editor.caret())
        };
        let Some(tag) = tag else {
            let mut notice = NotificationRequest::new(NotificationLevel::Info, "No tag at the caret");
            notice.key = Some(NOTICE_KEY.to_owned());
            self.notify(notice, cx);
            return;
        };
        let dialog = cx.new(|cx| UpdateTagDialog::new(&tag.name, cx));
        let target = editor.downgrade();
        let events = cx.subscribe_in(&dialog, window, move |workbench, _, event: &UpdateTagEvent, window, cx| {
            workbench.close_vcs_dialog(window, cx);
            let Some(editor) = target.upgrade() else { return };
            let focus = editor.focus_handle(cx);
            window.focus(&focus, cx);
            let UpdateTagEvent::Apply(abbreviation) = event else { return };
            if !apply_abbreviation(&editor, abbreviation, cx) {
                let body = cx
                    .default_global::<Localization>()
                    .0
                    .tx_built(&["'", "' does not describe a single element."], &[Param::from(abbreviation.as_str())]);
                let mut notice = NotificationRequest::new(NotificationLevel::Warning, "Cannot update the tag");
                notice.body = Some(body);
                notice.key = Some(NOTICE_KEY.to_owned());
                workbench.notify(notice, cx);
            }
        });
        let focus = dialog.focus_handle(cx);
        self.open_vcs_dialog(dialog.into(), focus, events, window, cx);
    }
}
