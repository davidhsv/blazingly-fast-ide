//! File › New Project…: an empty project folder, named and placed by the user, optionally a Git
//! repository. The dialog checks the name as it is typed and says where the folder goes; Create
//! hands the folder to the workbench, which creates it and either closes the dialog or gives it
//! the error to show.

use super::{button, checkbox, field_frame, footer, frame, title_row, tx};
use crate::app_models::project_actions::{join_path, project_name_error};
use crate::platform::i18n::{Localization, Param};
use crate::platform::input;
use crate::theme;
use crate::ui::text_field::{TextField, TextFieldEvent};
use gpui_kit::{
    App, ClickEvent, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, Keystroke, Render,
    SharedString, Subscription, Window, div, prelude::*, px,
};

const WIDTH: f32 = 520.0;
const LABEL_WIDTH: f32 = 72.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NewProjectEvent {
    /// Create the folder `path` (with a Git repository when `git`) and open it.
    Create {
        path: String,
        git: bool,
    },
    /// Browse…: ask for a folder, starting at this one.
    Browse(String),
    Cancelled,
}

pub struct NewProjectDialog {
    name: Entity<TextField>,
    location: Entity<TextField>,
    git: bool,
    /// The folder is being created: Create cannot be pressed again.
    busy: bool,
    /// Why the last Create failed.
    error: Option<String>,
    focus: FocusHandle,
    key_claim: u64,
    _fields: [Subscription; 2],
}

impl EventEmitter<NewProjectEvent> for NewProjectDialog {}

impl NewProjectDialog {
    /// The dialog with the name `untitled` in `location`, and a Git repository.
    pub fn new(location: &str, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let field = |text: &str, cx: &mut Context<Self>| {
            cx.new(|cx| {
                let mut field = TextField::new(cx);
                field.set_text(text, cx);
                field
            })
        };
        let name = field("untitled", cx);
        let location = field(location, cx);
        let listen = |field: &Entity<TextField>, window: &mut Window, cx: &mut Context<Self>| {
            cx.subscribe_in(field, window, |dialog: &mut Self, _, event: &TextFieldEvent, _, cx| match event {
                TextFieldEvent::Changed => cx.notify(),
                TextFieldEvent::Submitted => dialog.create(cx),
                TextFieldEvent::Cancelled => cx.emit(NewProjectEvent::Cancelled),
            })
        };
        let subscriptions = [listen(&name, window, cx), listen(&location, window, cx)];
        let focus = name.focus_handle(cx);
        let claimant = cx.weak_entity();
        let key_claim = input::claim_keys(
            focus.clone(),
            move |keystroke, _, cx| claimant.update(cx, |dialog, cx| dialog.claim_key(keystroke, cx)).unwrap_or(false),
            cx,
        );
        cx.on_release(|dialog, cx| input::release_keys(dialog.key_claim, cx)).detach();
        Self { name, location, git: true, busy: false, error: None, focus, key_claim, _fields: subscriptions }
    }

    pub fn name_field(&self) -> &Entity<TextField> {
        &self.name
    }

    pub fn location_field(&self) -> &Entity<TextField> {
        &self.location
    }

    /// Whether "Create Git repository" is checked.
    pub fn git(&self) -> bool {
        self.git
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// The folder Create makes, while the name and the location are valid.
    pub fn path(&self, cx: &App) -> Option<String> {
        let name = self.name.read(cx).text();
        let location = self.location.read(cx).text().trim();
        (project_name_error(name).is_none() && !location.is_empty()).then(|| join_path(location, name))
    }

    /// The line under the fields, in the UI language: the name's problem, else where the folder goes.
    pub fn hint(&self, cx: &mut App) -> Option<String> {
        if let Some(problem) = project_name_error(self.name.read(cx).text()) {
            return Some(tx(cx, problem));
        }
        let path = self.path(cx)?;
        let template = ["Project will be created in: ", ""];
        Some(cx.default_global::<Localization>().0.tx_built(&template, &[Param::Text(path)]))
    }

    /// Whether Create can be pressed.
    pub fn can_create(&self, cx: &App) -> bool {
        !self.busy && self.path(cx).is_some()
    }

    /// The workbench is creating the folder, or gave up with `error`.
    pub fn set_busy(&mut self, busy: bool, error: Option<String>, cx: &mut Context<Self>) {
        self.busy = busy;
        self.error = error;
        cx.notify();
    }

    /// The folder Browse… picked.
    pub fn set_location(&mut self, location: &str, cx: &mut Context<Self>) {
        self.location.update(cx, |field, cx| field.set_text(location, cx));
        cx.notify();
    }

    fn create(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.path(cx).filter(|_| !self.busy) else { return };
        self.busy = true;
        self.error = None;
        cx.emit(NewProjectEvent::Create { path, git: self.git });
        cx.notify();
    }

    fn toggle_git(&mut self, cx: &mut Context<Self>) {
        self.git = !self.git;
        cx.notify();
    }

    fn browse(&mut self, cx: &mut Context<Self>) {
        let current = self.location.read(cx).text().trim().to_owned();
        cx.emit(NewProjectEvent::Browse(current));
    }

    fn claim_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        if keystroke.modifiers.modified() {
            return false;
        }
        match keystroke.key.as_str() {
            "enter" => self.create(cx),
            "escape" => cx.emit(NewProjectEvent::Cancelled),
            _ => return false,
        }
        true
    }

    /// A labelled field, framed in red while `invalid`, with what follows it on the row.
    fn row(
        cx: &mut App,
        label: &str,
        field: &Entity<TextField>,
        invalid: bool,
        trailing: Option<gpui_kit::AnyElement>,
    ) -> gpui_kit::Div {
        let label = tx(cx, label);
        div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(div().w(px(LABEL_WIDTH)).flex_none().child(SharedString::from(label)))
            .child(field_frame(cx, invalid).flex_1().min_w_0().child(div().flex_1().min_w_0().child(field.clone())))
            .children(trailing)
    }
}

impl Focusable for NewProjectDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for NewProjectDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = tx(cx, "New Project");
        let name_invalid = project_name_error(self.name.read(cx).text()).is_some();
        let hint = self.hint(cx);
        let can_create = self.can_create(cx);
        let browse_label = tx(cx, "Browse…");
        let browse = button(cx, browse_label, false, true)
            .id("new-project-browse")
            .debug_selector(|| "new-project-browse".to_owned())
            .on_click(cx.listener(|dialog, _: &ClickEvent, _, cx| dialog.browse(cx)))
            .into_any_element();
        let name_row = Self::row(cx, "Name:", &self.name, name_invalid, None);
        let location_row = Self::row(cx, "Location:", &self.location, false, Some(browse));
        let git_label = tx(cx, "Create Git repository");
        let git = checkbox(cx, git_label, self.git)
            .id("new-project-git")
            .debug_selector(|| "new-project-git".to_owned())
            .on_click(cx.listener(|dialog, _: &ClickEvent, _, cx| dialog.toggle_git(cx)));
        let hint_color = theme::color(cx, if name_invalid { "danger" } else { "fg-muted" });
        let hint = hint.map(|hint| div().text_color(hint_color).child(SharedString::from(hint)));
        let error = self.error.clone().map(|error| {
            let error = tx(cx, &error);
            div().text_color(theme::color(cx, "danger")).child(SharedString::from(error))
        });
        let create_label = tx(cx, "Create");
        let cancel_label = tx(cx, "Cancel");
        let buttons = vec![
            button(cx, create_label, true, can_create)
                .id("new-project-create")
                .debug_selector(|| "new-project-create".to_owned())
                .on_click(cx.listener(|dialog, _: &ClickEvent, _, cx| dialog.create(cx))),
            button(cx, cancel_label, false, true)
                .id("new-project-cancel")
                .debug_selector(|| "new-project-cancel".to_owned())
                .on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(NewProjectEvent::Cancelled))),
        ];
        frame(cx, WIDTH)
            .child(title_row(cx, title))
            .child(
                div()
                    .px(px(16.0))
                    .py(px(4.0))
                    .flex()
                    .flex_col()
                    .gap(px(10.0))
                    .child(name_row)
                    .child(location_row)
                    .children(hint)
                    .child(git)
                    .children(error),
            )
            .child(footer(None, buttons))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{Modifiers, TestAppContext, VisualTestContext};
    use std::cell::RefCell;
    use std::rc::Rc;

    type Events = Rc<RefCell<Vec<NewProjectEvent>>>;

    fn open<'a>(
        cx: &'a mut TestAppContext,
        location: &str,
    ) -> (Entity<NewProjectDialog>, Events, &'a mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let location = location.to_owned();
        let (dialog, cx) = cx.add_window_view(move |window, cx| NewProjectDialog::new(&location, window, cx));
        let events: Events = Rc::default();
        let log = Rc::clone(&events);
        cx.update(|window, cx| {
            cx.subscribe(&dialog, move |_, event: &NewProjectEvent, _| log.borrow_mut().push(event.clone())).detach();
            let focus = dialog.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        (dialog, events, cx)
    }

    fn set(dialog: &Entity<NewProjectDialog>, name: &str, cx: &mut VisualTestContext) {
        let field = dialog.read_with(cx, |dialog, _| dialog.name_field().clone());
        field.update(cx, |field, cx| field.set_text(name, cx));
        cx.run_until_parked();
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.run_until_parked();
    }

    // src/features/app/__tests__/newProject.test.ts › validates the folder name
    #[gpui_kit::test]
    async fn validates_the_folder_name(cx: &mut TestAppContext) {
        let (dialog, events, cx) = open(cx, "/work");
        let hint = |cx: &mut VisualTestContext| dialog.update(cx, |dialog, cx| dialog.hint(cx));
        assert_eq!(hint(cx).as_deref(), Some("Project will be created in: /work/untitled"));
        for bad in ["", "  ", "a/b", "a:b", "..", "."] {
            set(&dialog, bad, cx);
            assert!(!dialog.read_with(cx, |dialog, cx| dialog.can_create(cx)), "{bad:?}");
            click(cx, "new-project-create");
        }
        set(&dialog, "", cx);
        assert_eq!(hint(cx).as_deref(), Some("Enter a project name"));
        set(&dialog, "a*b", cx);
        assert_eq!(hint(cx).as_deref(), Some("The name contains characters a folder name cannot have"));
        assert!(events.borrow().is_empty(), "an invalid name creates nothing");
    }

    #[gpui_kit::test]
    async fn create_reports_the_folder_and_whether_git_is_wanted(cx: &mut TestAppContext) {
        let (dialog, events, cx) = open(cx, "/work/");
        set(&dialog, "demo", cx);
        click(cx, "new-project-git");
        assert!(!dialog.read_with(cx, |dialog, _| dialog.git()));
        click(cx, "new-project-create");
        // Busy until the workbench answers: a second press does nothing.
        click(cx, "new-project-create");
        assert_eq!(*events.borrow(), [NewProjectEvent::Create { path: "/work/demo".to_owned(), git: false }]);
        dialog.update(cx, |dialog, cx| dialog.set_busy(false, Some("/work/demo already exists".to_owned()), cx));
        let error = dialog.read_with(cx, |dialog, _| dialog.error().map(str::to_owned));
        assert_eq!(error.as_deref(), Some("/work/demo already exists"));
        click(cx, "new-project-browse");
        click(cx, "new-project-cancel");
        let tail = events.borrow()[1..].to_vec();
        assert_eq!(tail, [NewProjectEvent::Browse("/work/".to_owned()), NewProjectEvent::Cancelled]);
    }
}
