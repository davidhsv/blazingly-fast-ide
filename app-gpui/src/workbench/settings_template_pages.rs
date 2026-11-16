//! The Settings pages of the templates: Editor › General › Postfix Completion, Editor › Live
//! Templates and Editor › File and Code Templates. Each is a view drawn for its whole page that keeps
//! its values in the UI storage the moment they change, so nothing is staged and Apply, Reset and
//! Cancel leave them alone; the workbench only puts the views on their pages, tells the File and
//! Code Templates page which project its Project scheme is for, and answers OK and Cancel asked
//! from their fields.

use super::Workbench;
use super::settings_feature_pages::mount_page;
use crate::platform::paths::ipc_path;
use crate::settings_model::staging::Scope;
use crate::settings_view::dialog::SettingsDialogView;
use crate::settings_view::pages::editor_file_templates::{self, FileTemplatesPageView};
use crate::settings_view::pages::editor_live_templates::{self, LiveTemplatesPageView};
use crate::settings_view::pages::editor_postfix_completion::{self, PostfixPageView};
use crate::settings_view::pages::{Binding, ControlEvent};
use gpui_kit::{AppContext, Context, Entity, EventEmitter, Render, Subscription};

/// Draws `view` for the whole of `page`, its labels found by the Settings search, and answers OK
/// and Cancel asked from its fields.
fn mount<V: Render + EventEmitter<ControlEvent>>(
    dialog: &Entity<SettingsDialogView>,
    page: &'static str,
    view: &Entity<V>,
    labels: &[&str],
    cx: &mut Context<Workbench>,
) -> Subscription {
    mount_page(dialog, page, view.clone().into(), labels, cx);
    let weak = dialog.downgrade();
    cx.subscribe(view, move |_, _, event: &ControlEvent, cx| {
        let Some(dialog) = weak.upgrade() else { return };
        match event {
            ControlEvent::Submit => dialog.update(cx, |dialog, cx| dialog.ok(cx)),
            ControlEvent::Cancel => dialog.update(cx, |dialog, cx| dialog.cancel(cx)),
            // The template pages ask nothing else of the dialog.
            _ => {}
        }
    })
}

impl Workbench {
    /// Puts the template pages' views on `dialog`, and returns the subscriptions that answer them;
    /// the caller keeps them as long as the dialog is open.
    pub(super) fn mount_template_settings_pages(
        &mut self,
        dialog: &Entity<SettingsDialogView>,
        cx: &mut Context<Self>,
    ) -> Vec<Subscription> {
        let model = dialog.read(cx).model().clone();
        // The pages have no Global / This project switch: their values are the user's everywhere
        // (the File and Code Templates page has its own Default / Project scheme instead).
        let binding = |page: &'static str| Binding::fixed(model.clone(), page, Scope::Global);

        let postfix_binding = binding(editor_postfix_completion::PAGE);
        let postfix = cx.new(|cx| PostfixPageView::new(postfix_binding, cx));
        let live_binding = binding(editor_live_templates::PAGE);
        let live = cx.new(|cx| LiveTemplatesPageView::new(live_binding, cx));
        // The project as the backend names it, the name the Project scheme's keys carry.
        let root = self.project_root(cx).map(|root| ipc_path(&root.to_string_lossy()));
        let files_binding = binding(editor_file_templates::PAGE);
        let files = cx.new(|cx| FileTemplatesPageView::new(files_binding, root, cx));
        vec![
            mount(dialog, editor_postfix_completion::PAGE, &postfix, &editor_postfix_completion::LABELS, cx),
            mount(dialog, editor_live_templates::PAGE, &live, &editor_live_templates::LABELS, cx),
            mount(dialog, editor_file_templates::PAGE, &files, &editor_file_templates::LABELS, cx),
        ]
    }
}
