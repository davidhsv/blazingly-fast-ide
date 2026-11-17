//! A postfix template that stands for an editor action, in the window's editors: Java's `.var`
//! runs Introduce Variable on the expression. When no such action is registered the template's
//! text goes in as for any template; when the action is registered but does not run (its condition
//! does not hold), the `;` the statement got goes again and the template's text replaces the
//! expression.

use super::super::Workbench;
use crate::editor::view::{EditOrigin, Editor};
use crate::platform::actions::{ActionRegistry, execute_action};
use crate::templates::editor::TemplateExpansion;
use crate::templates::postfix_action::{action_edit, without_action};
use gpui_kit::{Context, Entity, Window};

impl Workbench {
    /// Puts `expansion` into `editor`, or runs the action its postfix template stands for.
    pub(super) fn expand_or_run_action(
        &mut self,
        editor: &Entity<Editor>,
        expansion: TemplateExpansion,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(run) = expansion.postfix_action else {
            self.expand_template(editor, &expansion, cx);
            return;
        };
        if !cx.try_global::<ActionRegistry>().is_some_and(|registry| registry.contains(run.action)) {
            self.expand_template(editor, &TemplateExpansion { postfix_action: None, ..expansion }, cx);
            return;
        }
        let edit = action_edit(editor.read(cx).rope(), expansion.from, &run, expansion.to);
        let applied = editor.update(cx, |editor, cx| {
            editor.apply_edit_from(EditOrigin::Other, &edit.changes, vec![edit.selection], 0, cx)
        });
        if !applied {
            return;
        }
        // The action updates the window itself, so it runs once this update is over.
        let (workbench, target) = (cx.weak_entity(), editor.downgrade());
        window.defer(cx, move |window, cx| {
            if execute_action(run.action, None, window, cx) {
                return;
            }
            let Some(editor) = target.upgrade() else { return };
            if let Some(terminator) = &edit.terminator {
                editor.update(cx, |editor, cx| {
                    editor.apply_edit_from(
                        EditOrigin::Other,
                        std::slice::from_ref(terminator),
                        vec![edit.selection],
                        0,
                        cx,
                    );
                });
            }
            let fallback = without_action(&expansion, run.dot);
            let expanded = workbench.update(cx, |workbench, cx| workbench.expand_template(&editor, &fallback, cx));
            if let Err(error) = expanded {
                tracing::debug!("the postfix template ended with its window: {error:#}");
            }
        });
    }
}
