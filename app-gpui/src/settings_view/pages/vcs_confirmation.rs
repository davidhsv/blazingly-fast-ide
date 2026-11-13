//! Version Control › Confirmation: what happens to created and deleted files, and the history
//! limit.

use super::{Part, radio};

pub const PAGE: &str = "vcs.confirmation";

/// The forms of the page, by slot label.
pub fn forms() -> Vec<(&'static str, Vec<Part>)> {
    vec![
        (
            "When files are created",
            vec![radio(
                "vcs.confirmation.onCreate",
                "When files are created",
                &[("ask", "Ask"), ("silently", "Add silently"), ("never", "Do not add")],
            )],
        ),
        (
            "When files are deleted",
            vec![radio(
                "vcs.confirmation.onDelete",
                "When files are deleted",
                &[("ask", "Ask"), ("silently", "Remove silently"), ("never", "Do not remove")],
            )],
        ),
        (
            "Limit history to",
            vec![Part::ToggleNumber {
                toggle: "vcs.limitHistory",
                number: "vcs.historyLimit",
                label: "Limit history to",
                unit: Some("rows"),
                min: 10.0,
                max: 100_000.0,
                hint: None,
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

    // Not from a spec: a radio button stages its choice for created files.
    #[gpui_kit::test]
    async fn a_radio_button_stages_what_happens_to_created_files(cx: &mut TestAppContext) {
        let model = cx.update(|cx| dialog_model(PAGE, &[], cx));
        let binding = Binding::fixed(model.clone(), PAGE, Scope::Project);
        let (_, parts) = forms().remove(0);
        let (view, _, cx) = open(cx, move |_, cx| FormView::new(binding, parts, cx));
        click(cx, "settings-radio-vcs.confirmation.onCreate-silently");
        assert_eq!(staged(&model, cx), [("project:vcs.confirmation.onCreate".to_owned(), "\"silently\"".to_owned())]);
        let lines = view.read_with(cx, |view, cx| view.lines(cx));
        assert_eq!(lines, ["When files are created: ( ) Ask (•) Add silently ( ) Do not add"]);
    }
}
