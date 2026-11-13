//! Version Control › Commit: the message line limits and the commit checks.

use super::{Gate, Part, toggle};

pub const PAGE: &str = "vcs.commit";

const MESSAGE_LIMIT: (f64, f64) = (20.0, 500.0);

/// The forms of the page, by slot label.
pub fn forms() -> Vec<(&'static str, Vec<Part>)> {
    let limit = |toggle: &'static str, number: &'static str, label: &'static str| Part::ToggleNumber {
        toggle,
        number,
        label,
        unit: None,
        min: MESSAGE_LIMIT.0,
        max: MESSAGE_LIMIT.1,
        hint: None,
    };
    vec![
        (
            "Limit subject line",
            vec![
                limit("vcs.commit.limitSubject", "vcs.commit.subjectLimit", "Limit subject line"),
                limit("vcs.commit.limitBody", "vcs.commit.bodyLimit", "Limit body line"),
                toggle("vcs.commit.showRightMargin", "Show right margin")
                    .gate(Gate::AnyOn(&["vcs.commit.limitBody", "vcs.commit.limitSubject"]))
                    .indented()
                    .into(),
                toggle("vcs.commit.wrapAtMargin", "Wrap when typing reaches right margin")
                    .gate(Gate::On("vcs.commit.limitBody"))
                    .indented()
                    .into(),
            ],
        ),
        (
            "Analyze code",
            vec![Part::Columns(
                vec![
                    toggle("vcs.commit.analyzeCode", "Analyze code")
                        .hint("Reports errors and warnings from language servers.")
                        .into(),
                    toggle("vcs.commit.checkTodo", "Check TODO")
                        .hint("Looks for TODO and FIXME in added lines.")
                        .into(),
                    toggle("vcs.commit.cleanup", "Cleanup").hint("Applies fix-all code actions.").into(),
                ],
                vec![
                    toggle("vcs.commit.reformat", "Reformat code").into(),
                    toggle("vcs.commit.optimizeImports", "Optimize imports").into(),
                ],
            )],
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
    use serde_json::json;

    // Not from a spec: Show right margin can be used while a line limit is on, Wrap only while the
    // body limit is on.
    #[gpui_kit::test]
    async fn the_margin_options_follow_the_line_limits(cx: &mut TestAppContext) {
        let settings = [("vcs.commit.limitSubject", json!(false)), ("vcs.commit.limitBody", json!(false))];
        let model = cx.update(|cx| dialog_model(PAGE, &settings, cx));
        let binding = Binding::fixed(model.clone(), PAGE, Scope::Global);
        let (_, parts) = forms().remove(0);
        let (view, _, cx) = open(cx, move |_, cx| FormView::new(binding, parts, cx));
        click(cx, "settings-toggle-vcs.commit.showRightMargin");
        assert!(staged(&model, cx).is_empty(), "a disabled checkbox does not change");
        click(cx, "settings-toggle-vcs.commit.limitSubject");
        let lines = view.read_with(cx, |view, cx| view.lines(cx));
        assert!(lines.iter().any(|line| line.trim_start().starts_with("[")
            && line.contains("Show right margin")
            && !line.ends_with("(disabled)")));
        assert!(
            lines
                .iter()
                .any(|line| line.contains("Wrap when typing reaches right margin") && line.ends_with("(disabled)"))
        );
        click(cx, "settings-toggle-vcs.commit.showRightMargin");
        let staged = staged(&model, cx);
        assert!(staged.iter().any(|(key, _)| key == "global:vcs.commit.showRightMargin"), "{staged:?}");
    }
}
