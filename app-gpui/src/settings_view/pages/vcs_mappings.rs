//! Version Control › Directory Mappings: the project's root and the version control it is under.

use super::{Binding, ControlEvent, ROW_HEIGHT};
use crate::backend::Backend;
use crate::settings_view::controls::tx;
use crate::theme;
use gpui_kit::{App, Context, EventEmitter, IntoElement, Render, SharedString, Window, div, prelude::*, px};

pub const PAGE: &str = "vcs.mappings";
/// The slot the table fills.
pub const SLOT: &str = "Directory";

/// The project's root and whether a Git repository holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mapping {
    pub root: String,
    pub git: bool,
}

/// The mapping of the project open in `window`; none without a project.
pub fn project_mapping(window: &str, cx: &App) -> Option<Mapping> {
    let project = cx.try_global::<Backend>()?.host.project(window).ok()?;
    let git = !project.repos.read().is_empty();
    Some(Mapping { root: project.root.to_string_lossy().into_owned(), git })
}

pub struct MappingsView {
    binding: Binding,
    mapping: Option<Mapping>,
}

impl EventEmitter<ControlEvent> for MappingsView {}

impl MappingsView {
    pub fn new(binding: Binding, mapping: Option<Mapping>) -> Self {
        Self { binding, mapping }
    }

    /// The table as text: its header, then `<Project> root | Git` or the empty text.
    pub fn lines(&self, cx: &App) -> Vec<String> {
        let header = format!("{} | {}", tx(cx, "Directory"), tx(cx, "VCS"));
        let body = match &self.mapping {
            None => tx(cx, "No project is open"),
            Some(mapping) => format!("{} {} | {}", tx(cx, "<Project>"), mapping.root, vcs_name(mapping, cx)),
        };
        vec![header, body]
    }
}

fn vcs_name(mapping: &Mapping, cx: &App) -> String {
    if mapping.git { "Git".to_owned() } else { tx(cx, "<none>") }
}

impl Render for MappingsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let matched = self.binding.matches(SLOT, cx);
        let border = theme::color(cx, "border");
        let cell = |text: String| div().overflow_hidden().whitespace_nowrap().child(SharedString::from(text));
        let header = div()
            .flex()
            .gap(px(12.0))
            .px(px(6.0))
            .h(px(ROW_HEIGHT - 4.0))
            .items_center()
            .border_b_1()
            .border_color(border)
            .text_color(theme::color(cx, "fg-subtle"))
            .when(matched, |el| el.bg(theme::color(cx, "warning").opacity(0.08)))
            .child(cell(tx(cx, "Directory")).flex_1())
            .child(cell(tx(cx, "VCS")).w(px(120.0)));
        let body = match &self.mapping {
            None => div()
                .px(px(6.0))
                .py(px(4.0))
                .text_color(theme::color(cx, "fg-subtle"))
                .child(SharedString::from(tx(cx, "No project is open"))),
            Some(mapping) => div()
                .debug_selector(|| "settings-vcs-mapping-0".to_owned())
                .flex()
                .gap(px(12.0))
                .px(px(6.0))
                .h(px(ROW_HEIGHT - 4.0))
                .items_center()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .child(theme::icon(cx, "folder", 16.0, theme::color(cx, "fg-icon")))
                        .child(cell(format!("{} {}", tx(cx, "<Project>"), mapping.root))),
                )
                .child(cell(vcs_name(mapping, cx)).w(px(120.0))),
        };
        div()
            .ml(px(4.0))
            .max_w(px(560.0))
            .flex()
            .flex_col()
            .rounded(theme::length(cx, "radius-small"))
            .border_1()
            .border_color(border)
            .child(header)
            .child(body)
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;
    use crate::settings_model::staging::Scope;
    use gpui_kit::TestAppContext;

    // Not from a spec: without a project the table says so; a project in a Git repository is
    // mapped to Git, one outside any to none.
    #[gpui_kit::test]
    async fn the_project_root_is_mapped_to_its_version_control(cx: &mut TestAppContext) {
        let model = cx.update(|cx| dialog_model(PAGE, &[], cx));
        let binding = Binding::fixed(model, PAGE, Scope::Global);
        let (view, _, cx) = open(cx, move |_, _| MappingsView::new(binding, None));
        assert_eq!(view.read_with(cx, |view, cx| view.lines(cx)), ["Directory | VCS", "No project is open"]);
        view.update(cx, |view, cx| {
            view.mapping = Some(Mapping { root: "/work/app".to_owned(), git: true });
            cx.notify();
        });
        assert_eq!(view.read_with(cx, |view, cx| view.lines(cx))[1], "<Project> /work/app | Git");
        view.update(cx, |view, _| view.mapping = Some(Mapping { root: "/work/app".to_owned(), git: false }));
        assert_eq!(view.read_with(cx, |view, cx| view.lines(cx))[1], "<Project> /work/app | <none>");
    }
}
