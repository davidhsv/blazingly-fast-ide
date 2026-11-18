//! The UTF-8 byte order mark in the File Encoding item's menu: a UTF-8 file (other than a
//! `.properties` file shown through native-to-ASCII conversion) offers Add UTF-8 BOM or Remove UTF-8
//! BOM. The change makes the file unsaved and the next save writes it; with autosave on that save
//! comes at once.

use super::{StatusCommand, Workbench};
use crate::editor::view::{Editor, EditorStatus};
use crate::platform::i18n::Localization;
use crate::vcs_ops::menu::MenuItem;
use gpui_kit::{App, Context, Entity};

impl Workbench {
    /// The byte order mark rows of the encoding menu of `editor`: a separator and the toggle, for a
    /// UTF-8 document only; disabled while the file is read-only.
    pub(super) fn bom_rows(&self, editor: &Editor, status: &EditorStatus, cx: &App) -> Vec<MenuItem<StatusCommand>> {
        let native_to_ascii = crate::platform::settings::effective(&self.label, cx).editor.encodings.native_to_ascii;
        let properties = editor.path().to_string_lossy().ends_with(".properties");
        if status.encoding.to_lowercase() != "utf-8" || (native_to_ascii && properties) {
            return Vec::new();
        }
        let label = if status.has_bom { "Remove UTF-8 BOM" } else { "Add UTF-8 BOM" };
        vec![MenuItem::Separator, MenuItem::item(label, StatusCommand::Bom(!status.has_bom)).disabled(status.read_only)]
    }

    /// Adds or removes the byte order mark of the file `editor` edits. A refusal is told as "Cannot
    /// change the UTF-8 BOM" with the reason; with autosave on the file is written at once.
    pub(super) fn change_bom(&mut self, editor: &Entity<Editor>, has_bom: bool, cx: &mut Context<Self>) {
        let changing = editor.update(cx, |editor, cx| editor.set_bom(has_bom, cx));
        let editor = editor.clone();
        cx.spawn(async move |this, cx| {
            let changed = changing.await;
            let _ = this.update(cx, |workbench, cx| match changed {
                Ok(()) => workbench.autosave(Some(&editor), cx),
                Err(error) => {
                    let title = cx.default_global::<Localization>().0.tx("Cannot change the UTF-8 BOM");
                    workbench.notify_refusal(&title, error, cx);
                }
            });
        })
        .detach();
        cx.notify();
    }
}
