//! Editor › Code Style: the scheme (the scope Code Style and its language pages share), the
//! visual guides and the files the formatter leaves alone.

use super::list_editor::{ListShape, ListSpec};
use super::{
    Binding, ControlEvent, FieldKind, Part, drop_down, drop_down_row, field, hint_line, row, row_label, select_box,
};
use crate::settings_model::code_style_scheme::{CODE_STYLE_SCOPE, copy_scheme};
use crate::settings_model::staging::Scope;
use crate::settings_view::controls::{button, tx};
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, EventEmitter, IntoElement, MouseButton, Render, SharedString, Window, div,
    prelude::*,
};

pub const PAGE: &str = "editor.codeStyle";

/// The form controls of the page, by slot label.
pub fn forms() -> Vec<(&'static str, Vec<Part>)> {
    vec![(
        "Visual guides",
        vec![
            field("editor.codeStyle.visualGuides", "Visual guides", FieldKind::NumberList { min: 1.0, max: 999.0 })
                .width(super::MEDIUM_FIELD)
                .placeholder("Optional")
                .unit("columns")
                .hint("Comma-separated column numbers, e.g. 80, 100.")
                .into(),
        ],
    )]
}

/// The list editors of the page.
pub fn lists() -> Vec<ListSpec> {
    vec![ListSpec {
        label: "Do not format",
        key: "editor.codeStyle.doNotFormat",
        shape: ListShape::Strings { placeholder: "*.min.js" },
        empty: "No patterns",
        hint: Some("File name patterns and folders (glob), e.g. *.min.js, generated/**."),
    }]
}

/// Scheme: Default (IDE) or Project, and Copy to Project / Copy to IDE, which stages every Code
/// Style value in the other scheme and switches to it.
pub struct SchemeView {
    binding: Binding,
    /// A project is open and the dialog edits more than new projects' settings.
    can_project: bool,
    open: bool,
}

impl EventEmitter<ControlEvent> for SchemeView {}

impl SchemeView {
    pub fn new(binding: Binding, can_project: bool, cx: &mut Context<Self>) -> Self {
        cx.observe(&binding.model, |_, _, cx| cx.notify()).detach();
        Self { binding, can_project, open: false }
    }

    fn choose(&mut self, scope: Scope, cx: &mut Context<Self>) {
        self.open = false;
        self.binding.model.update(cx, |dialog, cx| {
            dialog.set_page_scope(CODE_STYLE_SCOPE, scope);
            cx.notify();
        });
        cx.notify();
    }

    /// Copy to Project / Copy to IDE.
    pub fn copy(&mut self, cx: &mut Context<Self>) {
        let from = self.binding.scope(cx);
        let to = match from {
            Scope::Global => Scope::Project,
            Scope::Project => Scope::Global,
        };
        let page = self.binding.page;
        self.binding.model.update(cx, |dialog, cx| {
            copy_scheme(dialog, page, from, to);
            dialog.set_page_scope(CODE_STYLE_SCOPE, to);
            cx.notify();
        });
        cx.notify();
    }

    /// `Scheme: Default (IDE) ▾ [Copy to Project]` and the note on where it is stored.
    pub fn lines(&self, cx: &App) -> Vec<String> {
        let (scheme, copy, note) = texts(self.binding.scope(cx));
        let copy = if self.can_project { format!(" [{}]", tx(cx, copy)) } else { String::new() };
        vec![format!("{}: {} ▾{copy}", tx(cx, "Scheme"), tx(cx, scheme)), format!("    {}", tx(cx, note))]
    }
}

/// The scheme's name, its copy button and where it is stored.
fn texts(scope: Scope) -> (&'static str, &'static str, &'static str) {
    match scope {
        Scope::Global => {
            ("Default (IDE)", "Copy to Project", "Stored in the IDE; used by projects without their own scheme.")
        }
        Scope::Project => ("Project", "Copy to IDE", "Stored in the project."),
    }
}

impl Render for SchemeView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.binding.query(cx);
        let matched = self.binding.matches("Scheme", cx);
        let scope = self.binding.scope(cx);
        let (scheme, copy, note) = texts(scope);
        let shown = tx(cx, scheme);
        let boxed = select_box(cx, &shown)
            .id("settings-code-style-scheme")
            .debug_selector(|| "settings-code-style-scheme".to_owned())
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                view.open = !view.open;
                cx.notify();
            }));
        let mut choices = vec![(Scope::Global, "Default (IDE)", "global")];
        if self.can_project {
            choices.push((Scope::Project, "Project", "project"));
        }
        let list = self.open.then(|| {
            let rows: Vec<AnyElement> = choices
                .into_iter()
                .map(|(value, label, name)| {
                    let text = tx(cx, label);
                    drop_down_row(cx, format!("settings-code-style-scheme-{name}"), &text, value == scope)
                        .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.choose(value, cx)))
                        .into_any_element()
                })
                .collect();
            drop_down(cx, rows)
        });
        let copy = self.can_project.then(|| {
            let text = tx(cx, copy);
            div()
                .id(SharedString::from("settings-code-style-copy"))
                .debug_selector(|| "settings-code-style-copy".to_owned())
                .child(button(cx, &text, false, true))
                .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.copy(cx)))
        });
        let label = tx(cx, "Scheme");
        let note = tx(cx, note);
        div()
            .flex()
            .flex_col()
            .child(
                row(cx, matched)
                    .child(row_label(cx, &label, &query))
                    .child(div().relative().child(boxed).children(list))
                    .children(copy),
            )
            .child(hint_line(cx, &note, super::LABEL_WIDTH))
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;
    use gpui_kit::TestAppContext;
    use serde_json::json;
    use std::rc::Rc;

    // code style scheme › copies every Code Style value to the other scheme (through Copy to
    // Project, which then shows the Project scheme)
    #[gpui_kit::test]
    async fn copy_to_project_stages_the_code_style_in_the_project_and_switches_to_it(cx: &mut TestAppContext) {
        let model = cx.update(|cx| dialog_model(PAGE, &[], cx));
        model.update(cx, |dialog, _| dialog.stage(PAGE, Scope::Global, "editor.codeStyle.hardWrapAt", json!(100)));
        // The scope follows the scheme chosen, as the dialog's page scope does.
        let scope_model = model.clone();
        let scope = Rc::new(move |cx: &App| {
            scope_model.read(cx).scopes.get(CODE_STYLE_SCOPE).copied().unwrap_or(Scope::Global)
        });
        let binding = Binding::new(model.clone(), PAGE, scope);
        let (view, _, cx) = open(cx, move |_, cx| SchemeView::new(binding, true, cx));
        click(cx, "settings-code-style-copy");
        let staged = staged(&model, cx);
        assert!(staged.contains(&("project:editor.codeStyle.hardWrapAt".to_owned(), "100".to_owned())), "{staged:?}");
        let lines = view.read_with(cx, |view, cx| view.lines(cx));
        assert_eq!(lines, ["Scheme: Project ▾ [Copy to IDE]", "    Stored in the project."]);
        click(cx, "settings-code-style-scheme");
        click(cx, "settings-code-style-scheme-global");
        assert_eq!(view.read_with(cx, |view, cx| view.lines(cx))[0], "Scheme: Default (IDE) ▾ [Copy to Project]");
    }
}
