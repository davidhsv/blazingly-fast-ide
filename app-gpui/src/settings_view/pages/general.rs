//! Appearance & Behavior › System Settings: where projects open, the default project directory
//! and saving when the IDE is idle.

use super::{FieldKind, Part, field, radio};

pub const PAGE: &str = "general";

/// The forms of the page, by slot label.
pub fn forms() -> Vec<(&'static str, Vec<Part>)> {
    vec![
        (
            "Open project in",
            vec![radio(
                "general.openProjectIn",
                "Open project in",
                &[("new", "New window"), ("same", "Current window"), ("ask", "Ask")],
            )],
        ),
        (
            "Default directory",
            vec![
                field("general.defaultProjectDirectory", "Default directory", FieldKind::Text)
                    .placeholder("System default")
                    .hint("This directory is preselected in \"Open…\" and \"New Window\" dialogs.")
                    .browse(true)
                    .into(),
            ],
        ),
        (
            "Save files if the IDE is idle for",
            vec![Part::ToggleNumber {
                toggle: "general.saveOnIdle",
                number: "general.idleSeconds",
                label: "Save files if the IDE is idle for",
                unit: Some("seconds"),
                min: 1.0,
                max: 3600.0,
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

    // Not from a spec: Browse… asks for a directory and the one chosen is staged.
    #[gpui_kit::test]
    async fn browse_asks_for_a_directory_and_stages_the_one_chosen(cx: &mut TestAppContext) {
        let model = cx.update(|cx| dialog_model(PAGE, &[], cx));
        let binding = Binding::fixed(model.clone(), PAGE, Scope::Global);
        let (_, parts) = forms().remove(1);
        let (view, seen, cx) = open(cx, move |_, cx| FormView::new(binding, parts, cx));
        let asked: std::rc::Rc<std::cell::RefCell<Option<super::super::Then<std::path::PathBuf>>>> = Default::default();
        let keep = asked.clone();
        cx.update(|_, cx| {
            cx.subscribe(&view, move |_, event: &super::super::ControlEvent, _| {
                if let super::super::ControlEvent::Browse { directories: true, then } = event {
                    *keep.borrow_mut() = Some(then.clone());
                }
            })
            .detach();
        });
        click(cx, "settings-browse-general.defaultProjectDirectory");
        assert_eq!(seen.borrow().as_slice(), ["browse"]);
        let then = asked.borrow_mut().take().expect("a directory is asked for");
        cx.update(|_, cx| then(std::path::PathBuf::from("/tmp/projects"), cx));
        cx.run_until_parked();
        assert_eq!(
            staged(&model, cx),
            [("global:general.defaultProjectDirectory".to_owned(), "\"/tmp/projects\"".to_owned())]
        );
        let field =
            view.read_with(cx, |view, _| view.field("general.defaultProjectDirectory").cloned()).expect("a field");
        assert_eq!(field.read_with(cx, |field, _| field.text().to_owned()), "/tmp/projects");
    }
}
