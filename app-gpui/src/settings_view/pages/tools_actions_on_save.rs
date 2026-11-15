//! Tools › Actions on Save: what Save and Save All run on a file before writing it (reformat,
//! optimize imports, code cleanup), and where the trailing spaces and the final line break are
//! set instead.

use super::{Part, toggle};

pub const PAGE: &str = "tools.actionsOnSave";

/// The forms of the page, by slot label.
pub fn forms() -> Vec<(&'static str, Vec<Part>)> {
    vec![
        (
            "Reformat code",
            vec![
                toggle("editor.formatOnSave", "Reformat code")
                    .hint("Formats the whole file with the language server's formatter.")
                    .into(),
                toggle("actionsOnSave.optimizeImports", "Optimize imports")
                    .hint("Runs the language server's Organize Imports action.")
                    .into(),
                toggle("actionsOnSave.cleanup", "Run code cleanup")
                    .hint("Applies the language server's fix-all action (source.fixAll).")
                    .into(),
            ],
        ),
        (
            "Save",
            vec![Part::Link {
                before: Some(
                    "Actions run when a file is saved with Save (⌘S) or Save All. Trailing spaces and the final line \
                     break are configured in",
                ),
                text: "Editor › General",
                page: "editor.general",
                query: None,
            }],
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::super::{Binding, FormView};
    use super::*;
    use crate::settings_model::staging::Scope;
    use gpui_kit::TestAppContext;

    // Not from a spec: the three checkboxes are off by default and stage into the project.
    #[gpui_kit::test]
    async fn the_save_actions_are_off_by_default_and_stage_into_the_project(cx: &mut TestAppContext) {
        let model = cx.update(|cx| dialog_model(PAGE, &[], cx));
        let binding = Binding::fixed(model.clone(), PAGE, Scope::Project);
        let (_, parts) = forms().remove(0);
        let (view, _, cx) = open(cx, move |_, cx| FormView::new(binding, parts, cx));
        let lines = view.read_with(cx, |view, cx| view.lines(cx));
        assert_eq!(
            lines,
            [
                "[ ] Reformat code",
                "    Formats the whole file with the language server's formatter.",
                "[ ] Optimize imports",
                "    Runs the language server's Organize Imports action.",
                "[ ] Run code cleanup",
                "    Applies the language server's fix-all action (source.fixAll).",
            ]
        );
        click(cx, "settings-toggle-actionsOnSave.optimizeImports");
        assert_eq!(staged(&model, cx), [("project:actionsOnSave.optimizeImports".to_owned(), "true".to_owned())]);
    }

    // Not from a spec: the note under the checkboxes opens Editor › General.
    #[gpui_kit::test]
    async fn the_note_opens_editor_general(cx: &mut TestAppContext) {
        let model = cx.update(|cx| dialog_model(PAGE, &[], cx));
        let binding = Binding::fixed(model, PAGE, Scope::Project);
        let (_, parts) = forms().remove(1);
        let (_view, seen, cx) = open(cx, move |_, cx| FormView::new(binding, parts, cx));
        click(cx, "settings-link-own-editor.general");
        assert_eq!(seen.borrow().as_slice(), ["open editor.general"]);
    }
}
