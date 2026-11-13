//! Build, Execution, Deployment › Deployment › Options: the names excluded from every transfer.

use super::{FieldKind, Part, field};

pub const PAGE: &str = "build.deployment.options";

/// The form controls of the page, by slot label.
pub fn forms() -> Vec<(&'static str, Vec<Part>)> {
    vec![(
        "Exclude items by name",
        vec![
            field("deployment.exclude", "Exclude items by name", FieldKind::Excludes)
                .hint("Names or wildcards separated by “;”, e.g. .git;*.pyc. Matching files and folders are never transferred.")
                .into(),
        ],
    )]
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::super::{Binding, FormView};
    use super::*;
    use crate::settings_model::staging::Scope;
    use gpui_kit::TestAppContext;
    use serde_json::json;

    // Not from a spec: the names typed, separated by ";", are staged as the list without blanks
    // or repeats.
    #[gpui_kit::test]
    async fn the_names_typed_are_staged_as_a_list(cx: &mut TestAppContext) {
        let model = cx.update(|cx| dialog_model(PAGE, &[("deployment.exclude", json!([".git"]))], cx));
        let binding = Binding::fixed(model.clone(), PAGE, Scope::Project);
        let (_, parts) = forms().remove(0);
        let (view, _, cx) = open(cx, move |_, cx| FormView::new(binding, parts, cx));
        let field = view.read_with(cx, |view, _| view.field("deployment.exclude").cloned()).expect("a field");
        assert_eq!(field.read_with(cx, |field, _| field.text().to_owned()), ".git");
        type_over(&field, ".git; *.pyc;;.git", cx);
        assert_eq!(staged(&model, cx), [("project:deployment.exclude".to_owned(), "[\".git\",\"*.pyc\"]".to_owned())]);
        assert_eq!(field.read_with(cx, |field, _| field.text().to_owned()), ".git; *.pyc;;.git", "the draft is kept");
    }
}
