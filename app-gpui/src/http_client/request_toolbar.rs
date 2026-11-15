//! The toolbar above an `.http` file's editor: Run All Requests in File, Stop, "Run with:" and the
//! environment chooser (No Environment, then the environments the file's `http-client.env.json`
//! files declare), and Reload Environments.
//!
//! The editor is the workbench's ([`RequestToolbar::new`] takes it as a view); the toolbar reports
//! through [`RequestToolbarEvent`] and is fed the environments, the choice and whether a request
//! of the file is being sent. A buffer without a file has no environments to choose from.

use crate::platform::i18n::Localization;
use crate::theme;
use gpui_kit::{
    AnyView, App, ClickEvent, Context, Div, EventEmitter, FocusHandle, Focusable, IntoElement, MouseButton, Render,
    SharedString, Stateful, Window, deferred, div, prelude::*, px,
};

/// What the toolbar asks of the window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RequestToolbarEvent {
    /// Run every request of the file, in order.
    RunAll,
    /// Cancel the request being sent.
    Stop,
    /// The environment chosen for the file (`None`: No Environment).
    Choose(Option<String>),
    /// Load the file's environments again.
    Reload,
}

/// A toolbar control as drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ToolbarButton {
    pub id: &'static str,
    pub enabled: bool,
}

pub const RUN_ALL: &str = "http-run-all";
pub const STOP: &str = "http-stop";
pub const CHOOSER: &str = "http-env-chooser";
pub const RELOAD: &str = "http-reload-envs";

const TOOLBAR_HEIGHT: f32 = 28.0;

fn tx(cx: &mut App, text: &str) -> String {
    cx.default_global::<Localization>().0.tx(text)
}

pub struct RequestToolbar {
    path: Option<String>,
    editor: AnyView,
    environments: Option<Vec<String>>,
    chosen: Option<String>,
    running: bool,
    open: bool,
    focus: FocusHandle,
}

impl EventEmitter<RequestToolbarEvent> for RequestToolbar {}

impl RequestToolbar {
    /// The toolbar of request file `path` (`None` for a buffer without a file) over `editor`.
    pub fn new(path: Option<String>, editor: AnyView, cx: &mut Context<Self>) -> Self {
        Self { path, editor, environments: None, chosen: None, running: false, open: false, focus: cx.focus_handle() }
    }

    pub fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }

    /// The file's environments (`None` until loaded), the one shown as chosen, and whether one of
    /// its requests is being sent.
    pub fn set_state(
        &mut self,
        environments: Option<Vec<String>>,
        chosen: Option<String>,
        running: bool,
        cx: &mut Context<Self>,
    ) {
        self.environments = environments;
        self.chosen = chosen;
        self.running = running;
        cx.notify();
    }

    /// The controls and whether each can be used.
    pub fn toolbar(&self) -> Vec<ToolbarButton> {
        let has_file = self.path.is_some();
        vec![
            ToolbarButton { id: RUN_ALL, enabled: true },
            ToolbarButton { id: STOP, enabled: self.running },
            ToolbarButton { id: CHOOSER, enabled: has_file },
            ToolbarButton { id: RELOAD, enabled: has_file },
        ]
    }

    fn enabled(&self, id: &str) -> bool {
        self.toolbar().iter().any(|button| button.id == id && button.enabled)
    }

    /// The chooser's options, value and English label: No Environment (the empty value), then the
    /// file's environments.
    pub fn options(&self) -> Vec<(String, String)> {
        let mut options = vec![(String::new(), "No Environment".to_owned())];
        options.extend(self.environments.iter().flatten().map(|name| (name.clone(), name.clone())));
        options
    }

    /// The chooser's value: the chosen environment, else No Environment.
    pub fn chosen(&self) -> Option<&str> {
        self.chosen.as_deref()
    }

    pub fn is_chooser_open(&self) -> bool {
        self.open
    }

    pub fn press(&mut self, id: &str, cx: &mut Context<Self>) {
        if !self.enabled(id) {
            return;
        }
        match id {
            RUN_ALL => cx.emit(RequestToolbarEvent::RunAll),
            STOP => cx.emit(RequestToolbarEvent::Stop),
            RELOAD => cx.emit(RequestToolbarEvent::Reload),
            CHOOSER => {
                self.open = !self.open;
                cx.notify();
            }
            _ => {}
        }
    }

    /// Picks `value` in the chooser (the empty value is No Environment).
    pub fn choose(&mut self, value: &str, cx: &mut Context<Self>) {
        self.open = false;
        let env = (!value.is_empty()).then(|| value.to_owned());
        self.chosen.clone_from(&env);
        cx.emit(RequestToolbarEvent::Choose(env));
        cx.notify();
    }

    // ---- rendering ----------------------------------------------------------------------------

    fn icon_button(&self, id: &'static str, icon: &str, title: &str, cx: &mut Context<Self>) -> Stateful<Div> {
        let enabled = self.enabled(id);
        let title = tx(cx, title);
        let color = theme::color(cx, if enabled { "fg" } else { "fg-faint" });
        div()
            .id(id)
            .debug_selector(move || id.to_owned())
            .flex_none()
            .size(px(22.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(theme::length(cx, "radius-small"))
            .when(enabled, |el| el.hover(|el| el.bg(theme::color(cx, "bg-hover"))))
            .tooltip(crate::ui::tooltip::tooltip(title))
            .child(theme::icon(cx, icon, 14.0, color))
            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.press(id, cx)))
    }

    fn chooser(&self, cx: &mut Context<Self>) -> Div {
        let enabled = self.enabled(CHOOSER);
        let chosen = self.chosen.clone().unwrap_or_default();
        let options = self.options();
        let label =
            options.iter().find(|(value, _)| *value == chosen).map_or_else(|| chosen.clone(), |(_, l)| l.clone());
        let label = tx(cx, &label);
        let list = self.open.then(|| {
            let rows: Vec<Stateful<Div>> = options
                .into_iter()
                .enumerate()
                .map(|(i, (value, text))| {
                    let text = tx(cx, &text);
                    let selected = value == chosen;
                    div()
                        .id(SharedString::from(format!("http-env-option-{i}")))
                        .debug_selector(move || format!("http-env-option-{i}"))
                        .h(theme::length(cx, "h-list-row"))
                        .px(px(8.0))
                        .flex()
                        .items_center()
                        .rounded(px(4.0))
                        .whitespace_nowrap()
                        .text_color(theme::color(cx, "fg-strong"))
                        .when(selected, |el| el.bg(theme::color(cx, "bg-tree-selected")))
                        .hover(|el| el.bg(theme::color(cx, "bg-hover")))
                        .child(SharedString::from(text))
                        .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.choose(&value, cx)))
                })
                .collect();
            deferred(
                div()
                    .id("http-env-options")
                    .absolute()
                    .top(px(24.0))
                    .left(px(0.0))
                    .min_w(px(160.0))
                    .max_h(px(360.0))
                    .overflow_y_scroll()
                    .p(px(4.0))
                    .flex()
                    .flex_col()
                    .bg(theme::color(cx, "bg-popup"))
                    .border_1()
                    .border_color(theme::color(cx, "border-popup"))
                    .rounded(theme::length(cx, "radius-popup"))
                    .occlude()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .children(rows),
            )
            .with_priority(1)
        });
        let environment = tx(cx, "Environment");
        div().relative().flex_none().child(
            div()
                .id(CHOOSER)
                .debug_selector(|| CHOOSER.to_owned())
                .h(px(22.0))
                .px(px(8.0))
                .flex()
                .items_center()
                .gap(px(4.0))
                .rounded(theme::length(cx, "radius-small"))
                .whitespace_nowrap()
                .text_color(theme::color(cx, "fg"))
                .when(!enabled, |el| el.opacity(0.5))
                .when(enabled, |el| el.hover(|el| el.bg(theme::color(cx, "bg-hover"))))
                .tooltip(crate::ui::tooltip::tooltip(environment))
                .child(SharedString::from(label))
                .child(theme::icon(cx, "chevron-down", 12.0, theme::color(cx, "fg-subtle")))
                .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.press(CHOOSER, cx)))
                .children(list),
        )
    }
}

impl Focusable for RequestToolbar {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for RequestToolbar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let run_with = tx(cx, "Run with:");
        let bar = div()
            .flex_none()
            .h(px(TOOLBAR_HEIGHT))
            .px(px(8.0))
            .flex()
            .items_center()
            .gap(px(4.0))
            .bg(theme::color(cx, "bg-editor"))
            .border_b_1()
            .border_color(theme::color(cx, "border"))
            .font_family(theme::installed_fonts::ui_family(cx))
            .text_size(theme::length(cx, "fs-ui"))
            .text_color(theme::color(cx, "fg-secondary"))
            .child(self.icon_button(RUN_ALL, "gutter-run-all", "Run All Requests in File", cx))
            .child(self.icon_button(STOP, "stop", "Stop", cx))
            .child(div().ml(px(8.0)).child(SharedString::from(run_with)))
            .child(self.chooser(cx))
            .child(self.icon_button(RELOAD, "refresh", "Reload Environments", cx));
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(theme::color(cx, "bg-editor"))
            .track_focus(&self.focus)
            .child(bar)
            .child(div().flex_1().min_h_0().child(self.editor.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::input;
    use gpui_kit::{Entity, Modifiers, TestAppContext, VisualTestContext};
    use std::cell::RefCell;
    use std::rc::Rc;

    type Events = Rc<RefCell<Vec<RequestToolbarEvent>>>;

    struct Editor;

    impl Render for Editor {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full()
        }
    }

    fn open<'a>(
        cx: &'a mut TestAppContext,
        path: Option<&str>,
    ) -> (Entity<RequestToolbar>, Events, &'a mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let path = path.map(str::to_owned);
        let (view, cx) = cx.add_window_view(|_, cx| {
            let editor = cx.new(|_| Editor);
            RequestToolbar::new(path, editor.into(), cx)
        });
        let events: Events = Rc::default();
        let log = Rc::clone(&events);
        cx.update(|window, cx| {
            cx.subscribe(&view, move |_, event: &RequestToolbarEvent, _| log.borrow_mut().push(event.clone())).detach();
            window.activate_window();
        });
        (view, events, cx)
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        cx.run_until_parked();
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.run_until_parked();
    }

    fn enabled(view: &Entity<RequestToolbar>, id: &str, cx: &mut VisualTestContext) -> Option<bool> {
        view.read_with(cx, |v, _| v.toolbar().into_iter().find(|b| b.id == id).map(|b| b.enabled))
    }

    /// Run All always runs the file; Stop is offered only while one of its requests is sent.
    #[gpui_kit::test]
    async fn runs_all_requests_and_stops_only_while_one_is_sent(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, Some("/p/a.http"));
        assert_eq!(enabled(&view, STOP, cx), Some(false));
        click(cx, STOP);
        click(cx, RUN_ALL);
        view.update(cx, |v, cx| v.set_state(None, None, true, cx));
        click(cx, STOP);
        assert_eq!(*events.borrow(), [RequestToolbarEvent::RunAll, RequestToolbarEvent::Stop]);
    }

    /// The chooser offers No Environment and the file's environments; picking one, then No
    /// Environment, reports each choice; Reload Environments asks for them again.
    #[gpui_kit::test]
    async fn chooses_an_environment_of_the_file(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, Some("/p/a.http"));
        view.update(cx, |v, cx| v.set_state(Some(vec!["dev".into(), "prod".into()]), None, false, cx));
        let labels: Vec<String> = view.read_with(cx, |v, _| v.options().into_iter().map(|(_, l)| l).collect());
        assert_eq!(labels, ["No Environment", "dev", "prod"]);
        click(cx, CHOOSER);
        assert!(view.read_with(cx, |v, _| v.is_chooser_open()));
        click(cx, "http-env-option-2");
        assert_eq!(view.read_with(cx, |v, _| v.chosen().map(str::to_owned)).as_deref(), Some("prod"));
        click(cx, CHOOSER);
        click(cx, "http-env-option-0");
        click(cx, RELOAD);
        assert_eq!(
            *events.borrow(),
            [
                RequestToolbarEvent::Choose(Some("prod".into())),
                RequestToolbarEvent::Choose(None),
                RequestToolbarEvent::Reload
            ]
        );
    }

    /// A buffer without a file runs its requests but has no environments to choose or reload.
    #[gpui_kit::test]
    async fn a_buffer_without_a_file_has_no_environments(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, None);
        assert_eq!(enabled(&view, CHOOSER, cx), Some(false));
        assert_eq!(enabled(&view, RELOAD, cx), Some(false));
        click(cx, CHOOSER);
        assert!(!view.read_with(cx, |v, _| v.is_chooser_open()));
        click(cx, RUN_ALL);
        assert_eq!(*events.borrow(), [RequestToolbarEvent::RunAll]);
    }
}
