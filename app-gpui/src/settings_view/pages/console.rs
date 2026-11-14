//! Build, Execution, Deployment › Console: the cycle buffer size and the patterns of the console
//! lines that are folded.

use super::Part;
use super::list_editor::{ListShape, ListSpec};

pub const PAGE: &str = "build.console";

/// The cycle buffer's range in KB.
pub const BUFFER_KB: (f64, f64) = (16.0, 1_048_576.0);

/// The form controls of the page, by slot label.
pub fn forms() -> Vec<(&'static str, Vec<Part>)> {
    vec![(
        "Override console cycle buffer size",
        vec![Part::ToggleNumber {
            toggle: "console.overrideBuffer",
            number: "console.bufferKb",
            label: "Override console cycle buffer size",
            unit: Some("KB"),
            min: BUFFER_KB.0,
            max: BUFFER_KB.1,
            hint: Some("Older output is discarded"),
        }],
    )]
}

/// The list editors of the page.
pub fn lists() -> Vec<ListSpec> {
    vec![ListSpec {
        label: "Fold console lines that contain",
        key: "console.foldPatterns",
        shape: ListShape::Strings { placeholder: "at org.example." },
        empty: "No patterns",
        hint: Some("Runs of consecutive matching lines collapse into one expandable “<N internal lines>” row."),
    }]
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::super::{Binding, FormView};
    use super::*;
    use crate::settings_model::staging::Scope;
    use gpui_kit::TestAppContext;

    // Not from a spec: the buffer checkbox and its size stage the two console settings.
    #[gpui_kit::test]
    async fn the_buffer_checkbox_and_size_stage_the_console_settings(cx: &mut TestAppContext) {
        // The buffer is overridden by default: start from a project that turned it off.
        let model = cx.update(|cx| dialog_model(PAGE, &[("console.overrideBuffer", serde_json::json!(false))], cx));
        let binding = Binding::fixed(model.clone(), PAGE, Scope::Project);
        let (_, parts) = forms().remove(0);
        let (view, _, cx) = open(cx, move |_, cx| FormView::new(binding, parts, cx));
        click(cx, "settings-toggle-console.overrideBuffer");
        let field = view.read_with(cx, |view, _| view.field("console.bufferKb").cloned()).expect("the size field");
        type_over(&field, "4096", cx);
        let staged = staged(&model, cx);
        assert!(staged.contains(&("project:console.overrideBuffer".to_owned(), "true".to_owned())), "{staged:?}");
        assert!(staged.contains(&("project:console.bufferKb".to_owned(), "4096".to_owned())), "{staged:?}");
        let lines = view.read_with(cx, |view, cx| view.lines(cx));
        assert_eq!(lines[0], "[x] Override console cycle buffer size: 4096 KB");
    }
}
