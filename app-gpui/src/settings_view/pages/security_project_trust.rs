//! Security › Project Trust: the projects trusted one by one, each with Revoke Trust, and the
//! trusted locations, every project inside one trusted without asking, each with Remove, and Add
//! folder… to trust another. The decisions are the backend's trust store's and take effect at
//! once, so the page stages nothing: each button is a request the workbench carries out, and the
//! workbench gives the page the lists again (or why that failed) once it is done.

use super::{Binding, row};
use crate::settings_view::controls::{button, search_label, section, tx};
use crate::theme;
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, EventEmitter, IntoElement, Render, SharedString, Subscription, Window, div,
    prelude::*, px,
};
use std::path::PathBuf;

pub const PAGE: &str = "projectTrust";

const INTRO: &str = "Trust decisions are saved in your user data and take effect immediately.";
const PROJECTS: &str = "Trusted projects";
const LOCATIONS: &str = "Trusted locations";
const ADD: &str = "Add folder…";
const LOCATIONS_HINT: &str = "Every project inside a trusted location is trusted without asking.";
const REVOKE: &str = "Revoke Trust";
const REMOVE: &str = "Remove";

/// What the Settings search finds on the page.
pub const LABELS: [&str; 5] = [PROJECTS, LOCATIONS, ADD, REVOKE, LOCATIONS_HINT];

/// The trust store's decisions as the page lists them: the paths as they are shown.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrustLists {
    pub projects: Vec<String>,
    pub locations: Vec<String>,
}

impl TrustLists {
    /// The projects and locations of `decisions`; the restricted projects are not listed.
    pub fn from_decisions(decisions: &ide_host::trust::TrustDecisions) -> Self {
        fn shown(paths: &[PathBuf]) -> Vec<String> {
            paths.iter().map(|path| path.display().to_string()).collect()
        }
        Self { projects: shown(&decisions.projects), locations: shown(&decisions.locations) }
    }
}

/// What a button of the page asks the workbench to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectTrustEvent {
    /// Revoke Trust: the project at this path is not trusted any more.
    Revoke(String),
    /// Remove: the location at this path is no longer trusted.
    RemoveLocation(String),
    /// Add folder…: a folder chosen with the system's chooser becomes a trusted location.
    AddLocation,
}

pub struct ProjectTrustView {
    binding: Binding,
    /// None until the workbench has read the store.
    lists: Option<TrustLists>,
    /// Why the last read or request failed; it stays until the page is opened again.
    error: String,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ProjectTrustEvent> for ProjectTrustView {}

impl ProjectTrustView {
    pub fn new(binding: Binding, cx: &mut Context<Self>) -> Self {
        // The search marks the page's labels as it is typed: the view follows the dialog's model.
        let observe = cx.observe(&binding.model, |_: &mut Self, _, cx| cx.notify());
        Self { binding, lists: None, error: String::new(), _subscriptions: vec![observe] }
    }

    pub fn set_lists(&mut self, lists: TrustLists, cx: &mut Context<Self>) {
        self.lists = Some(lists);
        cx.notify();
    }

    pub fn set_error(&mut self, error: String, cx: &mut Context<Self>) {
        self.error = error;
        cx.notify();
    }

    pub fn lists(&self) -> Option<&TrustLists> {
        self.lists.as_ref()
    }

    pub fn error(&self) -> &str {
        &self.error
    }

    /// The page as text: the intro, the error, each heading, then a row per path with its button.
    pub fn lines(&self, cx: &App) -> Vec<String> {
        let mut out = vec![tx(cx, INTRO)];
        if !self.error.is_empty() {
            out.push(format!("! {}", self.error));
        }
        out.push(tx(cx, PROJECTS));
        let lists = self.lists.clone().unwrap_or_default();
        out.extend(lists.projects.iter().map(|path| format!("  {path} [{}]", tx(cx, REVOKE))));
        out.push(format!("{} [{}]", tx(cx, LOCATIONS), tx(cx, ADD)));
        out.push(format!("    {}", tx(cx, LOCATIONS_HINT)));
        out.extend(lists.locations.iter().map(|path| format!("  {path} [{}]", tx(cx, REMOVE))));
        out
    }

    /// A listed path with the button that asks `event` of it.
    fn path_row(
        &self,
        selector: String,
        path: &str,
        label: &str,
        event: ProjectTrustEvent,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let text = tx(cx, label);
        let id = selector.clone();
        row(cx, false)
            .justify_between()
            .child(
                // A long path wraps anywhere rather than pushing the button out of the page.
                div().min_w_0().flex_1().child(SharedString::from(path.to_owned())),
            )
            .child(
                div()
                    .id(SharedString::from(id))
                    .debug_selector(move || selector)
                    .child(button(cx, &text, false, true))
                    .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| cx.emit(event.clone()))),
            )
            .into_any_element()
    }
}

impl Render for ProjectTrustView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.binding.query(cx);
        let lists = self.lists.clone().unwrap_or_default();
        let projects: Vec<AnyElement> = lists
            .projects
            .iter()
            .enumerate()
            .map(|(index, path)| {
                let event = ProjectTrustEvent::Revoke(path.clone());
                self.path_row(format!("settings-trust-revoke-{index}"), path, REVOKE, event, cx)
            })
            .collect();
        let locations: Vec<AnyElement> = lists
            .locations
            .iter()
            .enumerate()
            .map(|(index, path)| {
                let event = ProjectTrustEvent::RemoveLocation(path.clone());
                self.path_row(format!("settings-trust-remove-{index}"), path, REMOVE, event, cx)
            })
            .collect();
        let error = (!self.error.is_empty()).then(|| {
            div()
                .debug_selector(|| "settings-trust-error".to_owned())
                .py(px(4.0))
                .text_color(theme::color(cx, "danger"))
                .child(SharedString::from(self.error.clone()))
        });
        let add_label = tx(cx, ADD);
        let add = div()
            .id("settings-trust-add")
            .debug_selector(|| "settings-trust-add".to_owned())
            .child(button(cx, &add_label, false, true))
            .on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(ProjectTrustEvent::AddLocation)));
        let locations_title = tx(cx, LOCATIONS);
        let locations_head = row(cx, self.binding.matches(LOCATIONS, cx))
            .justify_between()
            .mt(px(8.0))
            .child(div().text_color(theme::color(cx, "fg-subtle")).child(search_label(cx, &locations_title, &query)))
            .child(add);
        div()
            .flex()
            .flex_col()
            .gap(px(2.0))
            .child(div().py(px(4.0)).child(SharedString::from(tx(cx, INTRO))))
            .children(error)
            .child(section(cx, Some(&tx(cx, PROJECTS)), &query, projects))
            .child(locations_head)
            .child(div().text_color(theme::color(cx, "fg-secondary")).child(SharedString::from(tx(cx, LOCATIONS_HINT))))
            .child(div().flex().flex_col().pl(px(4.0)).children(locations))
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{click, dialog_model, init};
    use super::*;
    use crate::settings_model::staging::Scope;
    use gpui_kit::TestAppContext;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn lists() -> TrustLists {
        TrustLists { projects: vec!["/work/app".to_owned()], locations: vec!["/work/clients".to_owned()] }
    }

    // Not from a spec: before the store is read the page lists nothing; then each project has Revoke
    // Trust and each location Remove, and the buttons ask for what they name.
    #[gpui_kit::test]
    async fn the_buttons_ask_to_revoke_remove_and_add(cx: &mut TestAppContext) {
        init(cx);
        let model = cx.update(|cx| dialog_model(PAGE, &[], cx));
        let binding = Binding::fixed(model, PAGE, Scope::Global);
        let (view, cx) = cx.add_window_view(move |_, cx| ProjectTrustView::new(binding, cx));
        cx.update(|window, _| window.activate_window());
        let seen: Rc<RefCell<Vec<ProjectTrustEvent>>> = Rc::default();
        let log = seen.clone();
        cx.update(|_, cx| {
            cx.subscribe(&view, move |_, event: &ProjectTrustEvent, _| log.borrow_mut().push(event.clone())).detach();
        });
        let empty = view.read_with(cx, |view, cx| view.lines(cx));
        assert_eq!(
            empty,
            [
                INTRO,
                "Trusted projects",
                "Trusted locations [Add folder…]",
                "    Every project inside a trusted location is trusted without asking.",
            ]
        );
        view.update(cx, |view, cx| view.set_lists(lists(), cx));
        let shown = view.read_with(cx, |view, cx| view.lines(cx));
        assert_eq!(shown[2], "  /work/app [Revoke Trust]");
        assert_eq!(shown[5], "  /work/clients [Remove]");
        click(cx, "settings-trust-revoke-0");
        click(cx, "settings-trust-remove-0");
        click(cx, "settings-trust-add");
        assert_eq!(
            *seen.borrow(),
            [
                ProjectTrustEvent::Revoke("/work/app".to_owned()),
                ProjectTrustEvent::RemoveLocation("/work/clients".to_owned()),
                ProjectTrustEvent::AddLocation,
            ]
        );
    }

    // Not from a spec: a failure is shown above the lists, which stay as they were.
    #[gpui_kit::test]
    async fn a_failure_is_shown_above_the_lists(cx: &mut TestAppContext) {
        init(cx);
        let model = cx.update(|cx| dialog_model(PAGE, &[], cx));
        let binding = Binding::fixed(model, PAGE, Scope::Global);
        let (view, cx) = cx.add_window_view(move |_, cx| ProjectTrustView::new(binding, cx));
        view.update(cx, |view, cx| {
            view.set_lists(lists(), cx);
            view.set_error("trusted location must be an existing directory".to_owned(), cx);
        });
        let shown = view.read_with(cx, |view, cx| view.lines(cx));
        assert_eq!(shown[1], "! trusted location must be an existing directory");
        assert_eq!(shown[3], "  /work/app [Revoke Trust]");
        assert!(cx.debug_bounds("settings-trust-error").is_some());
    }
}
