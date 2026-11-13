//! The "New" popup: a "Search for file type" field over the kinds the project view can create
//! (File, Directory or Package, then the file templates and the other contributed kinds). Typing
//! filters the rows, the arrows move over them past the separators, Enter or a click chooses.
//!
//! A row that runs something else (a template's dialog, another feature's action) is handed to
//! the workbench as it is. A row that creates a file or folder asks for its name in the popup:
//! the field checks the name as it is typed (a nested `a/b/c.ts` creates the folders on the
//! way, a dotted package name nested packages) and shows the problem under it. Enter creates,
//! Escape goes back to the kinds; Escape on the kinds closes the popup.

use crate::platform::actions::ActionMeta;
use crate::platform::i18n::{I18n, Localization};
use crate::platform::input;
use crate::platform::misc::platform_info::Os;
use crate::project_tree_paths::NameCheck;
use crate::project_view::context_menu::{Item, NewKind, ProjectCommand};
use crate::project_view::new_popup_model::{NewPopupRequest, NewPopupRow, PopupActions, PopupRun, new_popup_rows};
use crate::project_view::operations::check_new_entry_name;
use crate::theme;
use crate::ui::matching::index_runs;
use crate::ui::text_field::{TextField, TextFieldEvent};
use gpui_kit::{
    App, ClickEvent, Context, Div, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, Keystroke, MouseButton,
    Render, SharedString, Subscription, Window, div, prelude::*, px,
};
use std::collections::{BTreeMap, BTreeSet};

const WIDTH: f32 = 280.0;
const ROW_HEIGHT: f32 = 24.0;
const LIST_MAX_HEIGHT: f32 = 420.0;

/// What the popup asks of the workbench.
#[derive(Debug, Clone, PartialEq)]
pub enum NewPopupEvent {
    /// Escape on the kinds, or a click beside the popup: close it.
    Dismissed,
    /// A row that runs something else was chosen: close the popup and run it.
    Run(PopupRun),
    /// A row that creates a file or folder was chosen ([`ProjectCommand::New`]): the popup now
    /// asks for the name. Run the command to open the creation in the folder, then hand the
    /// popup the folder's names with [`NewPopupView::set_name_rule`].
    Begin(ProjectCommand),
    /// Enter on a name that passed the checks: create it (the inline editor's commit), then
    /// close the popup.
    Create { name: String },
    /// Escape while naming: the popup is back on the kinds; drop the creation that was begun.
    Back,
}

/// What the popup knows of the registered actions, owned for as long as it is open.
#[derive(Debug, Clone, Default)]
pub struct PopupActionSet {
    pub registered: BTreeMap<String, ActionMeta>,
    /// The ids of the registered actions that are off in the current context.
    pub disabled: BTreeSet<String>,
    /// The shortcut the keymap shows for an action, by action id.
    pub shortcuts: BTreeMap<String, String>,
}

impl PopupActionSet {
    fn view(&self) -> PopupActions<'_> {
        PopupActions { registered: &self.registered, disabled: &self.disabled, shortcuts: &self.shortcuts }
    }
}

/// What a new name is checked against: the names in the folder it is created in, and the OS the
/// project's files are on (case and reserved names).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameRule {
    pub siblings: Vec<String>,
    pub host_os: Os,
}

/// The check of the inline editor for a new item of `kind`: a nested path is allowed, a package
/// name is split at its dots first.
pub fn check_new_name(name: &str, kind: NewKind, rule: &NameRule) -> NameCheck {
    let pkg = match kind {
        NewKind::Package(pkg) => Some(pkg),
        NewKind::File | NewKind::Directory => None,
    };
    check_new_entry_name(name, pkg, &rule.siblings, rule.host_os)
}

/// The name being asked for.
struct Naming {
    kind: NewKind,
    /// The chosen row, in the UI language.
    label: String,
    icon: Option<String>,
    /// Unset until the workbench has opened the creation in the folder.
    rule: Option<NameRule>,
    /// Enter was pressed: an empty name shows its problem too.
    submitted: bool,
}

pub struct NewPopupView {
    items: Vec<Item>,
    actions: PopupActionSet,
    search: Entity<TextField>,
    name: Entity<TextField>,
    rows: Vec<NewPopupRow>,
    /// The row the user put the selection on; the first that can be chosen otherwise.
    active: Option<usize>,
    naming: Option<Naming>,
    key_claims: [u64; 2],
    _field_events: [Subscription; 2],
}

impl EventEmitter<NewPopupEvent> for NewPopupView {}

fn tx(cx: &mut App, text: &str) -> String {
    cx.default_global::<Localization>().0.tx(text)
}

impl NewPopupView {
    /// The popup for `request` (the rows of [`crate::project_view::context_menu::new_menu_items`]).
    pub fn new(request: &NewPopupRequest, actions: PopupActionSet, cx: &mut Context<Self>) -> Self {
        let placeholder = tx(cx, "Search for file type");
        let search = cx.new(|cx| TextField::new(cx).with_placeholder(placeholder));
        let name = cx.new(TextField::new);
        let search_events = cx.subscribe(&search, |popup: &mut Self, _, event: &TextFieldEvent, cx| match event {
            TextFieldEvent::Changed => popup.query_changed(cx),
            // Enter and Escape are claimed before the field sees them.
            TextFieldEvent::Submitted | TextFieldEvent::Cancelled => {}
        });
        let name_events = cx.subscribe(&name, |_: &mut Self, _, event: &TextFieldEvent, cx| {
            if *event == TextFieldEvent::Changed {
                cx.notify();
            }
        });
        let claimant = cx.weak_entity();
        let search_claim = input::claim_keys(
            search.focus_handle(cx),
            move |keystroke, window, cx| {
                claimant.update(cx, |popup, cx| popup.claim_list_key(keystroke, window, cx)).unwrap_or(false)
            },
            cx,
        );
        let claimant = cx.weak_entity();
        let name_claim = input::claim_keys(
            name.focus_handle(cx),
            move |keystroke, window, cx| {
                claimant.update(cx, |popup, cx| popup.claim_name_key(keystroke, window, cx)).unwrap_or(false)
            },
            cx,
        );
        cx.on_release(|popup, cx| {
            for claim in popup.key_claims {
                input::release_keys(claim, cx);
            }
        })
        .detach();
        let mut popup = Self {
            items: request.items.clone(),
            actions,
            search,
            name,
            rows: Vec::new(),
            active: None,
            naming: None,
            key_claims: [search_claim, name_claim],
            _field_events: [search_events, name_events],
        };
        popup.rows = popup.compute_rows(cx);
        popup
    }

    fn compute_rows(&self, cx: &mut App) -> Vec<NewPopupRow> {
        let query = self.search.read(cx).text().to_owned();
        let i18n: &I18n = &cx.default_global::<Localization>().0;
        new_popup_rows(&self.items, &query, &self.actions.view(), i18n)
    }

    fn query_changed(&mut self, cx: &mut Context<Self>) {
        self.rows = self.compute_rows(cx);
        self.active = None;
        cx.notify();
    }

    /// The rows as shown: a label, or `-` for a separator.
    pub fn rows(&self) -> Vec<String> {
        self.rows.iter().map(|row| if row.separator { "-".to_owned() } else { row.label.clone() }).collect()
    }

    /// The index of the selected row: the one the user chose, else the first that can be chosen.
    pub fn current(&self) -> Option<usize> {
        let chosen = self.active.filter(|&at| self.rows.get(at).is_some_and(|row| !row.separator));
        chosen.or_else(|| self.rows.iter().position(|row| !row.separator))
    }

    /// The label of the selected row.
    pub fn selected(&self) -> Option<&str> {
        self.current().map(|at| self.rows[at].label.as_str())
    }

    pub fn search_field(&self) -> &Entity<TextField> {
        &self.search
    }

    pub fn name_field(&self) -> &Entity<TextField> {
        &self.name
    }

    /// Whether the popup asks for a name, and for which kind (the chosen row's label).
    pub fn naming(&self) -> Option<&str> {
        self.naming.as_ref().map(|naming| naming.label.as_str())
    }

    /// The names in the folder the new item goes into: the name can now be checked and created.
    pub fn set_name_rule(&mut self, rule: NameRule, cx: &mut Context<Self>) {
        if let Some(naming) = &mut self.naming {
            naming.rule = Some(rule);
            cx.notify();
        }
    }

    /// The check of the typed name; `None` until [`Self::set_name_rule`] was called.
    fn name_check(&self, cx: &App) -> Option<NameCheck> {
        let naming = self.naming.as_ref()?;
        let rule = naming.rule.as_ref()?;
        Some(check_new_name(self.name.read(cx).text(), naming.kind, rule))
    }

    /// The problem shown under the name field, in the UI language: the typed name's, and an empty
    /// name's once Enter was pressed.
    pub fn name_error(&self, cx: &mut App) -> Option<String> {
        let submitted = self.naming.as_ref().is_some_and(|naming| naming.submitted);
        if self.name.read(cx).text().is_empty() && !submitted {
            return None;
        }
        let error = self.name_check(cx)?.error?;
        Some(tx(cx, &error))
    }

    /// A hint about the typed name that does not stop it (leading or trailing spaces).
    pub fn name_warning(&self, cx: &mut App) -> Option<String> {
        let warning = self.name_check(cx)?.warning?;
        Some(tx(cx, &warning))
    }

    /// Moves the selection by one row over the separators, going round at either end.
    fn step(&mut self, forward: bool, cx: &mut Context<Self>) {
        let selectable: Vec<usize> = (0..self.rows.len()).filter(|&at| !self.rows[at].separator).collect();
        let Some(last) = selectable.len().checked_sub(1) else { return };
        let at = self.current().and_then(|current| selectable.iter().position(|&index| index == current));
        let next = match at {
            None => 0,
            Some(at) if forward => (at + 1) % (last + 1),
            Some(0) => last,
            Some(at) => at - 1,
        };
        self.active = Some(selectable[next]);
        cx.notify();
    }

    /// Chooses the row at `index`: creation asks for a name, anything else is run.
    fn pick(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(row) = self.rows.get(index).filter(|row| !row.separator) else { return };
        let (Some(run), label, icon) = (row.run.clone(), row.label.clone(), row.icon.clone()) else { return };
        match run {
            PopupRun::Command(command @ ProjectCommand::New { kind, .. }) => {
                self.naming = Some(Naming { kind, label, icon, rule: None, submitted: false });
                self.name.update(cx, |field, cx| field.set_text("", cx));
                window.focus(&self.name.focus_handle(cx), cx);
                cx.emit(NewPopupEvent::Begin(command));
                cx.notify();
            }
            run => cx.emit(NewPopupEvent::Run(run)),
        }
    }

    /// Chooses the row that creates a `kind`, as Enter on it does: the popup asks for its name.
    pub fn choose(&mut self, kind: NewKind, window: &mut Window, cx: &mut Context<Self>) {
        let creates = |row: &NewPopupRow| match &row.run {
            Some(PopupRun::Command(ProjectCommand::New { kind: of, .. })) => *of == kind,
            _ => false,
        };
        if let Some(at) = self.rows.iter().position(creates) {
            self.pick(at, window, cx);
        }
    }

    /// Enter on the name: creates it once it has no problem.
    fn create(&mut self, cx: &mut Context<Self>) {
        let Some(naming) = &mut self.naming else { return };
        naming.submitted = true;
        cx.notify();
        let Some(check) = self.name_check(cx) else { return };
        if check.error.is_none() {
            let name = self.name.read(cx).text().to_owned();
            cx.emit(NewPopupEvent::Create { name });
        }
    }

    /// Escape on the name: back to the kinds, with the query as it was.
    fn back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.naming.take().is_some() {
            window.focus(&self.search.focus_handle(cx), cx);
            cx.emit(NewPopupEvent::Back);
            cx.notify();
        }
    }

    fn claim_list_key(&mut self, keystroke: &Keystroke, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if keystroke.modifiers.modified() {
            return false;
        }
        match keystroke.key.as_str() {
            "up" | "down" => self.step(keystroke.key == "down", cx),
            "enter" => {
                if let Some(at) = self.current() {
                    self.pick(at, window, cx);
                }
            }
            "escape" => cx.emit(NewPopupEvent::Dismissed),
            _ => return false,
        }
        true
    }

    fn claim_name_key(&mut self, keystroke: &Keystroke, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if keystroke.modifiers.modified() {
            return false;
        }
        match keystroke.key.as_str() {
            "enter" => self.create(cx),
            "escape" => self.back(window, cx),
            _ => return false,
        }
        true
    }

    /// A label with the characters the query matched in the accent colour.
    fn label(cx: &App, row: &NewPopupRow, active: bool) -> Div {
        let color = theme::color(cx, if active { "fg-strong" } else { "fg-emphasis" });
        let accent = theme::color(cx, "accent");
        let mut parts: Vec<Div> = Vec::new();
        let mut at = 0;
        for run in index_runs(&row.label, row.matches.as_deref().unwrap_or_default()) {
            if run.start > at {
                parts.push(div().child(SharedString::from(row.label[at..run.start].to_owned())));
            }
            parts.push(div().text_color(accent).child(SharedString::from(row.label[run.clone()].to_owned())));
            at = run.end;
        }
        if at < row.label.len() {
            parts.push(div().child(SharedString::from(row.label[at..].to_owned())));
        }
        div().flex_1().min_w_0().flex().overflow_hidden().whitespace_nowrap().text_color(color).children(parts)
    }

    fn render_list(&self, cx: &mut Context<Self>) -> Div {
        let current = self.current();
        let rows: Vec<_> = self
            .rows
            .iter()
            .enumerate()
            .map(|(index, row)| {
                if row.separator {
                    return div()
                        .id(index)
                        .h(px(9.0))
                        .flex()
                        .items_center()
                        .px(px(10.0))
                        .child(div().flex_1().h(px(1.0)).bg(theme::color(cx, "border-popup")));
                }
                let active = current == Some(index);
                let muted = theme::color(cx, "fg-muted");
                let icon = row.icon.as_deref().map(|icon| theme::icon(cx, icon, 16.0, theme::color(cx, "fg")));
                let shortcut = row.shortcut.clone().map(|shortcut| {
                    div().flex_none().ml(px(16.0)).text_color(muted).child(SharedString::from(shortcut))
                });
                div()
                    .id(index)
                    .debug_selector(move || format!("new-popup-row-{index}"))
                    .h(px(ROW_HEIGHT))
                    .mx(px(4.0))
                    .px(px(6.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .rounded(theme::length(cx, "radius-small"))
                    .when(active, |el| el.bg(theme::color(cx, "bg-tree-selected")))
                    .child(div().w(px(16.0)).flex_none().flex().justify_center().children(icon))
                    .child(Self::label(cx, row, active))
                    .children(shortcut)
                    .on_hover(cx.listener(move |popup: &mut Self, hovered: &bool, _, cx| {
                        if *hovered && popup.active != Some(index) {
                            popup.active = Some(index);
                            cx.notify();
                        }
                    }))
                    .on_click(cx.listener(move |popup, _: &ClickEvent, window, cx| popup.pick(index, window, cx)))
            })
            .collect();
        let empty = self.rows.is_empty().then(|| {
            let text = tx(cx, "Nothing found");
            div()
                .px(px(10.0))
                .py(px(6.0))
                .text_color(theme::color(cx, "fg-faint"))
                .text_size(theme::length(cx, "fs-small"))
                .child(SharedString::from(text))
        });
        let search = div()
            .flex_none()
            .h(theme::length(cx, "h-row"))
            .flex()
            .items_center()
            .gap(px(6.0))
            .px(px(10.0))
            .mb(px(4.0))
            .border_b_1()
            .border_color(theme::color(cx, "border-popup"))
            .child(theme::icon(cx, "search", 14.0, theme::color(cx, "fg-muted")))
            .child(div().flex_1().min_w_0().child(self.search.clone()));
        div()
            .flex()
            .flex_col()
            .child(search)
            .child(div().id("new-popup-list").max_h(px(LIST_MAX_HEIGHT)).overflow_y_scroll().children(rows))
            .children(empty)
    }

    fn render_name(&self, cx: &mut Context<Self>) -> Div {
        let Some(naming) = &self.naming else { return div() };
        let error = self.name_error(cx);
        let warning = self.name_warning(cx).filter(|_| error.is_none());
        let fg = theme::color(cx, "fg");
        let header = div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .px(px(10.0))
            .h(theme::length(cx, "h-row"))
            .text_color(theme::color(cx, "fg-strong"))
            .children(naming.icon.as_deref().map(|icon| theme::icon(cx, icon, 16.0, fg)))
            .child(SharedString::from(naming.label.clone()));
        let frame = div()
            .mx(px(10.0))
            .h(px(28.0))
            .flex()
            .items_center()
            .px(px(8.0))
            .rounded(theme::length(cx, "radius-small"))
            .border_1()
            .border_color(theme::color(cx, if error.is_some() { "danger" } else { "border" }))
            .bg(theme::color(cx, "bg-input"))
            .child(div().flex_1().min_w_0().child(self.name.clone()));
        let small = theme::length(cx, "fs-small");
        let problem = error.map(|error| {
            div().px(px(10.0)).text_size(small).text_color(theme::color(cx, "danger")).child(SharedString::from(error))
        });
        let hint = warning.map(|warning| {
            let color = theme::color(cx, "warning");
            div().px(px(10.0)).text_size(small).text_color(color).child(SharedString::from(warning))
        });
        div().flex().flex_col().gap(px(6.0)).pb(px(6.0)).child(header).child(frame).children(problem).children(hint)
    }
}

impl Focusable for NewPopupView {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        if self.naming.is_some() { self.name.focus_handle(cx) } else { self.search.focus_handle(cx) }
    }
}

impl Render for NewPopupView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = if self.naming.is_some() { self.render_name(cx) } else { self.render_list(cx) };
        div()
            .w(px(WIDTH))
            .flex()
            .flex_col()
            .pb(px(4.0))
            .overflow_hidden()
            .bg(theme::color(cx, "bg-popup"))
            .border_1()
            .border_color(theme::color(cx, "border-popup"))
            .rounded(theme::length(cx, "radius-popup"))
            .text_size(theme::length(cx, "fs-ui"))
            // A click in the popup is not a click beside it.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project_tree_paths::PackageKind;
    use crate::project_view::context_menu::Item;
    use crate::vcs_ops::menu::MenuItem;
    use gpui_kit::{Modifiers, TestAppContext, VisualTestContext};
    use serde_json::Value;
    use std::cell::RefCell;
    use std::rc::Rc;

    type Events = Rc<RefCell<Vec<NewPopupEvent>>>;

    fn run(action: &'static str) -> ProjectCommand {
        ProjectCommand::Run { action, arg: Value::Null }
    }

    fn new_file(parent: &str) -> ProjectCommand {
        ProjectCommand::New { kind: NewKind::File, parent: Some(parent.to_owned()), show_view: false }
    }

    fn open(cx: &mut TestAppContext, items: Vec<Item>) -> (Entity<NewPopupView>, Events, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let request = NewPopupRequest { at: (10.0, 10.0), items, seq: 1 };
        let (popup, cx) = cx.add_window_view(move |_, cx| NewPopupView::new(&request, PopupActionSet::default(), cx));
        let events: Events = Rc::default();
        let log = Rc::clone(&events);
        cx.update(|window, cx| {
            cx.subscribe(&popup, move |_, event: &NewPopupEvent, _| log.borrow_mut().push(event.clone())).detach();
            let focus = popup.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        (popup, events, cx)
    }

    fn rows(popup: &Entity<NewPopupView>, cx: &mut VisualTestContext) -> Vec<String> {
        popup.read_with(cx, |popup, _| popup.rows())
    }

    // src/features/project-tree/NewPopup.test.tsx › New popup › filters file kinds as you type and runs the
    // selection with Enter
    #[gpui_kit::test]
    async fn filters_file_kinds_as_you_type_and_runs_the_selection_with_enter(cx: &mut TestAppContext) {
        let items = vec![
            MenuItem::item("File", run("file")).icon("file"),
            MenuItem::Separator,
            MenuItem::item("Python File", run("py")).icon("file-python"),
            MenuItem::item("Stylesheet", run("css")).icon("file-css"),
        ];
        let (popup, events, cx) = open(cx, items);
        assert_eq!(rows(&popup, cx), ["File", "-", "Python File", "Stylesheet"]);
        cx.simulate_input("sty");
        assert_eq!(rows(&popup, cx), ["Stylesheet"]);
        cx.simulate_keystrokes("enter");
        assert_eq!(*events.borrow(), [NewPopupEvent::Run(PopupRun::Command(run("css")))]);
    }

    // src/features/project-tree/NewPopup.test.tsx › New popup › moves the selection with the arrow keys, skipping
    // separators
    #[gpui_kit::test]
    async fn moves_the_selection_with_the_arrow_keys_skipping_separators(cx: &mut TestAppContext) {
        let items =
            vec![MenuItem::item("File", run("file")), MenuItem::Separator, MenuItem::item("Python File", run("py"))];
        let (popup, events, cx) = open(cx, items);
        assert_eq!(popup.read_with(cx, |popup, _| popup.selected().map(str::to_owned)).as_deref(), Some("File"));
        cx.simulate_keystrokes("down");
        assert_eq!(popup.read_with(cx, |popup, _| popup.current()), Some(2));
        cx.simulate_keystrokes("enter");
        assert_eq!(*events.borrow(), [NewPopupEvent::Run(PopupRun::Command(run("py")))]);
        // Past the last row the selection goes round to the first.
        cx.simulate_keystrokes("down");
        assert_eq!(popup.read_with(cx, |popup, _| popup.current()), Some(0));
    }

    // Not from a spec: File asks for the name in the popup; the name is checked against the folder
    // as it is typed, a nested path is fine, Enter creates and Escape goes back to the kinds.
    #[gpui_kit::test]
    async fn a_new_file_is_named_in_the_popup(cx: &mut TestAppContext) {
        let items = vec![MenuItem::item("File", new_file("/p/src")).icon("file")];
        let (popup, events, cx) = open(cx, items);
        cx.simulate_keystrokes("enter");
        assert_eq!(*events.borrow(), [NewPopupEvent::Begin(new_file("/p/src"))]);
        assert_eq!(popup.read_with(cx, |popup, _| popup.naming().map(str::to_owned)).as_deref(), Some("File"));
        // Until the folder is known nothing is created.
        cx.simulate_input("main.ts");
        cx.simulate_keystrokes("enter");
        assert_eq!(events.borrow().len(), 1);
        popup.update(cx, |popup, cx| {
            let rule = NameRule { siblings: vec!["main.ts".to_owned()], host_os: Os::Linux };
            popup.set_name_rule(rule, cx);
        });
        assert_eq!(popup.update(cx, |popup, cx| popup.name_error(cx)).as_deref(), Some("'main.ts' already exists"));
        cx.simulate_keystrokes("enter");
        assert_eq!(events.borrow().len(), 1, "a taken name is not created");
        let field = popup.read_with(cx, |popup, _| popup.name_field().clone());
        field.update(cx, |field, cx| field.set_text("", cx));
        cx.simulate_input("a/b/c.ts");
        assert_eq!(popup.update(cx, |popup, cx| popup.name_error(cx)), None);
        cx.simulate_keystrokes("enter");
        assert_eq!(events.borrow().last(), Some(&NewPopupEvent::Create { name: "a/b/c.ts".to_owned() }));
        cx.simulate_keystrokes("escape");
        assert_eq!(events.borrow().last(), Some(&NewPopupEvent::Back));
        assert!(popup.read_with(cx, |popup, _| popup.naming().is_none()));
        cx.simulate_keystrokes("escape");
        assert_eq!(events.borrow().last(), Some(&NewPopupEvent::Dismissed));
    }

    // Not from a spec: an empty name shows its problem once Enter is pressed; a package name is
    // checked segment by segment.
    #[gpui_kit::test]
    async fn an_empty_or_invalid_package_name_is_flagged(cx: &mut TestAppContext) {
        let kind = NewKind::Package(PackageKind::Jvm);
        let package = ProjectCommand::New { kind, parent: Some("/p".to_owned()), show_view: false };
        let (popup, events, cx) = open(cx, vec![MenuItem::item("Package", package)]);
        cx.run_until_parked();
        let bounds = cx.debug_bounds("new-popup-row-0").expect("the row is drawn");
        cx.simulate_click(bounds.center(), Modifiers::default());
        assert!(matches!(events.borrow().last(), Some(NewPopupEvent::Begin(_))));
        popup.update(cx, |popup, cx| popup.set_name_rule(NameRule { siblings: Vec::new(), host_os: Os::Linux }, cx));
        assert_eq!(popup.update(cx, |popup, cx| popup.name_error(cx)), None, "nothing is flagged before typing");
        cx.simulate_keystrokes("enter");
        assert!(popup.update(cx, |popup, cx| popup.name_error(cx)).is_some());
        cx.simulate_input("com.acme");
        assert_eq!(popup.update(cx, |popup, cx| popup.name_error(cx)), None);
        let rule = NameRule { siblings: Vec::new(), host_os: Os::Linux };
        assert!(check_new_name("com.class", kind, &rule).error.is_some(), "a keyword is no package name");
    }
}
