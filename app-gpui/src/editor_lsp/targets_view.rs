//! The chooser of navigation targets, shared by Go to Declaration, Go to Implementation(s), Go
//! to Type Declaration and Show Usages: a [`ListPopup`] of the [`Chooser`]'s targets, each with
//! its name, its line and where it is, and a pin that shows them in the Find tool window. While
//! the server still searches its title says so, and the rows fill in when the search ends.

use super::hover_view::tx;
use super::intentions_view::{ListOptions, ListPopup, ListPopupEvent, ListRow};
use super::navigation::{Chooser, PIN_LABEL, TargetRow, target_rows};
use crate::platform::virtual_docs::basename;
use crate::theme::icons::file_icon_name;
use gpui_kit::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, Render, Subscription, Window, prelude::*,
};
use ide_core::ipc_path::to_ipc;
use ide_core::{Location, SymbolKind};
use std::collections::HashMap;

/// What the chooser asks of its owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetsEvent {
    /// A target was chosen: close the chooser and open it focused, the caret at its start.
    Open(Location),
    /// The pin: close the chooser and run [`super::navigation::SHOW_USAGE_RESULTS_ACTION`] with the
    /// symbol and the targets.
    Pin { symbol: String, targets: Vec<Location> },
    /// The selected target changed: highlight what it would open.
    Active(Location),
    /// Escape or a click beside it: close the chooser, and cancel the search that still runs
    /// ([`super::navigation::GotoFlow::chooser_closed`]).
    Closed,
}

/// The badge of an index symbol's kind.
fn symbol_icon(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::File => "symbol-file",
        SymbolKind::Module => "symbol-module",
        SymbolKind::Namespace => "symbol-namespace",
        SymbolKind::Package => "symbol-package",
        SymbolKind::Class => "symbol-class",
        SymbolKind::Interface => "symbol-interface",
        SymbolKind::Struct => "symbol-struct",
        SymbolKind::Enum => "symbol-enum",
        SymbolKind::Trait => "symbol-trait",
        SymbolKind::Function | SymbolKind::Macro | SymbolKind::Operator => "symbol-function",
        SymbolKind::Method => "symbol-method",
        SymbolKind::Constructor => "symbol-constructor",
        SymbolKind::Field => "symbol-field",
        SymbolKind::Property | SymbolKind::Event | SymbolKind::Key => "symbol-property",
        SymbolKind::Variable => "symbol-variable",
        SymbolKind::Constant | SymbolKind::EnumMember => "symbol-constant",
        SymbolKind::TypeParameter | SymbolKind::TypeAlias => "symbol-type",
        SymbolKind::Unknown => "symbol-text",
    }
}

/// The list rows of chooser rows: an index symbol shows its badge, any other target its file's
/// icon. A row's id is its position.
fn list_rows(rows: &[TargetRow]) -> Vec<ListRow> {
    rows.iter()
        .enumerate()
        .map(|(index, row)| {
            let icon = match row.symbol {
                Some(kind) => symbol_icon(kind).to_owned(),
                None => file_icon_name(&basename(&to_ipc(&row.target.path))).to_owned(),
            };
            ListRow {
                id: index.to_string(),
                label: row.label.clone(),
                detail: Some(row.detail.clone()),
                preview: row.preview.clone(),
                icon: Some(icon),
                ..ListRow::default()
            }
        })
        .collect()
}

/// The targets chooser, shown in the workbench's popup slot: it takes the keyboard. The owner
/// draws it below the caret line at [`crate::editor::view::Editor::position_of`] of the caret,
/// flipping above when there is no room below.
pub struct TargetsChooser {
    list: Entity<ListPopup>,
    rows: Vec<TargetRow>,
    chooser: Chooser,
    _events: Subscription,
}

impl EventEmitter<TargetsEvent> for TargetsChooser {}

impl TargetsChooser {
    /// The chooser of `chooser`'s targets. `lines` holds the lines of the target files that could
    /// be read, by path; `root` is the project root paths are shown relative to. Call
    /// [`Self::opened`] once subscribed to it.
    pub fn new(chooser: Chooser, lines: &HashMap<String, Vec<String>>, root: &str, cx: &mut Context<Self>) -> Self {
        let rows = target_rows(&chooser.targets, lines, root);
        let options = ListOptions { pin: Some(tx(cx, PIN_LABEL)), ..ListOptions::default() };
        let list_rows = list_rows(&rows);
        let title = chooser.title.clone();
        let list = cx.new(|cx| ListPopup::new(&title, list_rows, options, cx));
        let events = cx.subscribe(&list, |chooser: &mut Self, _, event: &ListPopupEvent, cx| chooser.listed(event, cx));
        Self { list, rows, chooser, _events: events }
    }

    /// The chooser is shown and its owner listens: the selected target is reported.
    pub fn opened(&mut self, cx: &mut Context<Self>) {
        self.list.update(cx, |list, cx| list.opened(cx));
    }

    /// The navigation answered again ([`super::navigation::GotoStep::Chooser`] for the open
    /// chooser): the title and the rows follow, the selected target stays selected.
    pub fn set_chooser(
        &mut self,
        chooser: Chooser,
        lines: &HashMap<String, Vec<String>>,
        root: &str,
        cx: &mut Context<Self>,
    ) {
        self.rows = target_rows(&chooser.targets, lines, root);
        let rows = list_rows(&self.rows);
        let title = chooser.title.clone();
        self.chooser = chooser;
        self.list.update(cx, |list, cx| {
            list.set_title(&title, cx);
            list.set_rows(rows, cx);
        });
        cx.notify();
    }

    pub fn list(&self) -> &Entity<ListPopup> {
        &self.list
    }

    pub fn searching(&self) -> bool {
        self.chooser.searching
    }

    pub fn title(&self) -> &str {
        &self.chooser.title
    }

    /// The chooser rows, in the order listed before speed search.
    pub fn rows(&self) -> &[TargetRow] {
        &self.rows
    }

    fn target(&self, id: &str) -> Option<Location> {
        let index: usize = id.parse().ok()?;
        self.rows.get(index).map(|row| row.target.clone())
    }

    fn listed(&mut self, event: &ListPopupEvent, cx: &mut Context<Self>) {
        let event = match event {
            ListPopupEvent::Activated(id) => self.target(id).map(TargetsEvent::Open),
            ListPopupEvent::Active(id) => self.target(id).map(TargetsEvent::Active),
            ListPopupEvent::Pin => Some(TargetsEvent::Pin {
                symbol: self.chooser.symbol.clone(),
                targets: self.chooser.targets.iter().map(|target| target.location.clone()).collect(),
            }),
            ListPopupEvent::Dismissed => Some(TargetsEvent::Closed),
            ListPopupEvent::Control { .. } | ListPopupEvent::PreviewWanted(_) => None,
        };
        if let Some(event) = event {
            cx.emit(event);
        }
    }
}

impl Focusable for TargetsChooser {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.list.focus_handle(cx)
    }
}

impl Render for TargetsChooser {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.list.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::super::navigation::{Target, TargetSymbol, chooser_title, searching_title};
    use super::*;
    use crate::platform::i18n::I18n;
    use crate::platform::input;
    use crate::theme;
    use gpui_kit::{Modifiers, TestAppContext, VisualTestContext};
    use ide_core::{Pos, Range};
    use ide_lsp::types::GotoKind;
    use std::cell::RefCell;
    use std::rc::Rc;

    type Events = Rc<RefCell<Vec<TargetsEvent>>>;

    fn location(path: &str, line: u32, col: u32) -> Location {
        Location { path: path.into(), range: Range::new(Pos::new(line, col), Pos::new(line, col + 3)), uri: None }
    }

    fn lines() -> HashMap<String, Vec<String>> {
        HashMap::from([
            ("/p/a.ts".to_owned(), vec!["".to_owned(), "function run() {}".to_owned()]),
            ("/p/b.ts".to_owned(), vec!["export interface R {".to_owned(), "  run(): void;".to_owned()]),
        ])
    }

    fn chooser(targets: Vec<Location>, searching: bool) -> Chooser {
        let mut i18n = I18n::default();
        let title = if searching {
            searching_title(&mut i18n, Some("run"), targets.len())
        } else {
            chooser_title(&mut i18n, GotoKind::Implementation, Some("run"), targets.len(), false)
        };
        Chooser { title, targets: targets.into_iter().map(Target::from).collect(), searching, symbol: "run".into() }
    }

    fn open(cx: &mut TestAppContext, chooser: Chooser) -> (Entity<TargetsChooser>, Events, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let (view, cx) = cx.add_window_view(move |_, cx| TargetsChooser::new(chooser, &lines(), "/p", cx));
        let events: Events = Rc::default();
        let seen = Rc::clone(&events);
        cx.update(|window, cx| {
            cx.subscribe(&view, move |_, event: &TargetsEvent, _| seen.borrow_mut().push(event.clone())).detach();
            window.activate_window();
            window.focus(&view.focus_handle(cx), cx);
        });
        view.update(cx, |view, cx| view.opened(cx));
        cx.run_until_parked();
        (view, events, cx)
    }

    #[gpui_kit::test]
    async fn opens_the_chosen_target_and_reports_the_selected_one(cx: &mut TestAppContext) {
        let targets = vec![location("/p/a.ts", 1, 9), location("/p/b.ts", 1, 2)];
        let (view, events, cx) = open(cx, chooser(targets.clone(), false));
        let labels: Vec<String> =
            view.read_with(cx, |view, _| view.rows().iter().map(|row| row.label.clone()).collect());
        assert_eq!(labels, ["run", "run"]);
        assert_eq!(*events.borrow(), [TargetsEvent::Active(targets[0].clone())]);
        cx.simulate_keystrokes("down enter");
        assert_eq!(events.borrow().last(), Some(&TargetsEvent::Open(targets[1].clone())));
        cx.simulate_keystrokes("escape");
        assert_eq!(events.borrow().last(), Some(&TargetsEvent::Closed));
    }

    #[gpui_kit::test]
    async fn says_it_is_searching_and_fills_in_when_the_search_ends(cx: &mut TestAppContext) {
        let first = vec![location("/p/a.ts", 1, 9)];
        let (view, events, cx) = open(cx, chooser(first.clone(), true));
        assert!(view.read_with(cx, |view, _| view.searching()));
        assert!(view.read_with(cx, |view, _| view.title().contains("searching")));
        let all = vec![location("/p/a.ts", 1, 9), location("/p/b.ts", 1, 2)];
        view.update(cx, |view, cx| view.set_chooser(chooser(all.clone(), false), &lines(), "/p", cx));
        cx.run_until_parked();
        assert!(!view.read_with(cx, |view, _| view.searching()));
        let list = view.read_with(cx, |view, _| view.list().clone());
        let title = view.read_with(cx, |view, _| view.title().to_owned());
        assert_eq!(list.read_with(cx, |list, _| list.title().to_owned()), title);
        assert_eq!(list.read_with(cx, |list, _| list.lines()).len(), 2);
        // The first target stays selected: nothing new is reported for it.
        assert_eq!(*events.borrow(), [TargetsEvent::Active(first[0].clone())]);
        let bounds = cx.debug_bounds("list-pin").expect("the pin is drawn");
        cx.simulate_click(bounds.center(), Modifiers::default());
        assert_eq!(events.borrow().last(), Some(&TargetsEvent::Pin { symbol: "run".into(), targets: all }));
    }

    #[gpui_kit::test]
    async fn an_index_symbol_shows_its_name_and_container(cx: &mut TestAppContext) {
        let symbol = TargetSymbol { name: "Order".into(), kind: SymbolKind::Class, container: Some("shop".into()) };
        let target = Target { location: location("/p/Order.java", 3, 0), symbol: Some(symbol) };
        let title = "Choose Declaration".to_owned();
        let chooser = Chooser { title, targets: vec![target], searching: false, symbol: "Order".into() };
        let (view, _, cx) = open(cx, chooser);
        let row = view.read_with(cx, |view, _| view.rows()[0].clone());
        assert_eq!((row.label.as_str(), row.detail.as_str()), ("Order", "shop · Order.java:4"));
        assert_eq!(list_rows(std::slice::from_ref(&row))[0].icon.as_deref(), Some("symbol-class"));
    }
}
