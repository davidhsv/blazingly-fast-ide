//! Build, Execution, Deployment › Debugger › Data Views: the delay of the value tooltip.

use super::{Part, number};

pub const PAGE: &str = "build.debugger.dataViews";

/// The form controls of the page, by slot label.
pub fn forms() -> Vec<(&'static str, Vec<Part>)> {
    vec![(
        "Value tooltip delay (ms)",
        vec![
            number("debugger.valueTooltipDelay", "Value tooltip delay (ms)", 0.0, 10_000.0, 100.0)
                .indented()
                .note_off("debugger.valueTooltip", "Value tooltips are off")
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

    // Not from a spec: the delay steps by 100 ms with the arrows, and says when tooltips are off.
    #[gpui_kit::test]
    async fn the_delay_steps_by_a_hundred_and_says_when_tooltips_are_off(cx: &mut TestAppContext) {
        let settings = [("debugger.valueTooltip", json!(false)), ("debugger.valueTooltipDelay", json!(700))];
        let model = cx.update(|cx| dialog_model(PAGE, &settings, cx));
        let binding = Binding::fixed(model.clone(), PAGE, Scope::Global);
        let (_, parts) = forms().remove(0);
        let (view, _, cx) = open(cx, move |_, cx| FormView::new(binding, parts, cx));
        let lines = view.read_with(cx, |view, cx| view.lines(cx));
        assert_eq!(lines, ["  Value tooltip delay (ms): 700 — Value tooltips are off"]);
        let field = view.read_with(cx, |view, _| view.field("debugger.valueTooltipDelay").cloned()).expect("a field");
        cx.update(|window, cx| window.focus(&gpui_kit::Focusable::focus_handle(field.read(cx), cx), cx));
        cx.simulate_keystrokes("up");
        cx.run_until_parked();
        assert_eq!(staged(&model, cx), [("global:debugger.valueTooltipDelay".to_owned(), "800".to_owned())]);
    }
}
