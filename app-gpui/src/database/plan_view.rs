//! The Query Plan tab: the operations of an explained statement as a tree rooted in the statement
//! itself ("Select"), each with its parameters, and the selected operation's properties beside it.
//! Explain Again asks for the plan once more.

use super::dialog_parts::tx;
use crate::theme;
use gpui_kit::{
    App, ClickEvent, Context, EventEmitter, FocusHandle, Focusable, IntoElement, Render, SharedString, Window, div,
    prelude::*, px,
};
use ide_db::{PlanNode, PlanProperty};
use std::collections::BTreeSet;

/// What the Query Plan tab asks of its owner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlanEvent {
    /// Explain Again: `DbStore::rerun_result` of the plan's result id.
    Rerun(u64),
}

/// A row of the plan tree as drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanRow {
    /// `None` for the statement the operations are rooted in, else the index of the plan node.
    pub node: Option<usize>,
    pub depth: usize,
    pub operation: String,
    pub params: String,
    pub icon: Option<&'static str>,
    pub expandable: bool,
    pub expanded: bool,
}

pub struct PlanView {
    /// The id of the plan's result tab in the store.
    id: u64,
    /// The data source the statement was explained on, as messages name it.
    source: String,
    statement: String,
    plan: Vec<PlanNode>,
    busy: bool,
    /// `None`: the statement row.
    selected: Option<usize>,
    /// Collapsed rows (`None`: the statement row); everything starts expanded.
    collapsed: BTreeSet<Option<usize>>,
    focus: FocusHandle,
}

impl EventEmitter<PlanEvent> for PlanView {}

/// `Select` for `select …`: the statement's first word, capitalised.
fn statement_operation(statement: &str) -> String {
    let word: String = statement.trim_start().chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
    let word = if word.is_empty() { "statement".to_owned() } else { word.to_lowercase() };
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => word,
    }
}

/// A table for scans of one, a key for index lookups, an arrow for the statement.
fn plan_icon(operation: &str, statement: bool) -> Option<&'static str> {
    let lower = operation.to_lowercase();
    if statement {
        Some("arrow-left")
    } else if lower.contains("index") {
        Some("db-key")
    } else if lower.contains("scan") {
        Some("db-table")
    } else {
        None
    }
}

impl PlanView {
    pub fn new(id: u64, source: &str, statement: &str, plan: Vec<PlanNode>, cx: &mut Context<Self>) -> Self {
        Self {
            id,
            source: source.to_owned(),
            statement: statement.to_owned(),
            plan,
            busy: false,
            selected: None,
            collapsed: BTreeSet::new(),
            focus: cx.focus_handle(),
        }
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn statement(&self) -> &str {
        &self.statement
    }

    /// A new plan for the tab (a re-explained statement): everything expanded again.
    pub fn set_plan(&mut self, id: u64, source: &str, statement: &str, plan: Vec<PlanNode>, cx: &mut Context<Self>) {
        self.id = id;
        self.source = source.to_owned();
        self.statement = statement.to_owned();
        self.plan = plan;
        self.busy = false;
        self.selected = None;
        self.collapsed.clear();
        cx.notify();
    }

    pub fn set_busy(&mut self, busy: bool, cx: &mut Context<Self>) {
        self.busy = busy;
        cx.notify();
    }

    pub fn is_busy(&self) -> bool {
        self.busy
    }

    fn children(&self, parent: Option<usize>) -> Vec<usize> {
        let parent = parent.and_then(|index| u32::try_from(index).ok());
        (0..self.plan.len()).filter(|&index| self.plan[index].parent == parent).collect()
    }

    /// The rows shown: the statement, then the operations under it depth first, without those under
    /// a collapsed row.
    pub fn rows(&self) -> Vec<PlanRow> {
        let mut rows = Vec::new();
        if self.plan.is_empty() {
            return rows;
        }
        let expanded = !self.collapsed.contains(&None);
        rows.push(PlanRow {
            node: None,
            depth: 0,
            operation: statement_operation(&self.statement),
            params: String::new(),
            icon: plan_icon("", true),
            expandable: !self.children(None).is_empty(),
            expanded,
        });
        if expanded {
            // Depth first; a malformed plan whose parents loop is cut by the visited set.
            let mut visited = BTreeSet::new();
            let mut stack: Vec<(usize, usize)> = self.children(None).into_iter().rev().map(|i| (i, 1)).collect();
            while let Some((index, depth)) = stack.pop() {
                if !visited.insert(index) {
                    continue;
                }
                let node = &self.plan[index];
                let children = self.children(Some(index));
                let open = !self.collapsed.contains(&Some(index));
                rows.push(PlanRow {
                    node: Some(index),
                    depth,
                    operation: node.operation.clone(),
                    params: node.params.clone(),
                    icon: plan_icon(&node.operation, false),
                    expandable: !children.is_empty(),
                    expanded: open,
                });
                if open {
                    stack.extend(children.into_iter().rev().map(|child| (child, depth + 1)));
                }
            }
        }
        rows
    }

    /// The selected row (`None`: the statement).
    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    pub fn select(&mut self, node: Option<usize>, cx: &mut Context<Self>) {
        self.selected = node.filter(|&index| index < self.plan.len());
        cx.notify();
    }

    pub fn toggle(&mut self, node: Option<usize>, cx: &mut Context<Self>) {
        if !self.collapsed.remove(&node) {
            self.collapsed.insert(node);
        }
        cx.notify();
    }

    /// The selected operation's properties; the statement's is its operation.
    pub fn properties(&self) -> Vec<PlanProperty> {
        match self.selected.and_then(|index| self.plan.get(index)) {
            Some(node) => node.properties.clone(),
            None => {
                let operation = statement_operation(&self.statement).to_lowercase();
                vec![PlanProperty { name: "Operation".to_owned(), value: operation }]
            }
        }
    }

    pub fn rerun(&mut self, cx: &mut Context<Self>) {
        if !self.busy {
            self.busy = true;
            cx.emit(PlanEvent::Rerun(self.id));
            cx.notify();
        }
    }
}

impl Focusable for PlanView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for PlanView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let border = theme::color(cx, "border");
        let muted = theme::color(cx, "fg-muted");
        let selected_bg = theme::color(cx, "bg-tree-selected");
        let explain = tx(cx, "Explain Again");
        let enabled = !self.busy;
        let rerun = div()
            .id("db-plan-rerun")
            .debug_selector(|| "db-plan-rerun".to_owned())
            .size(px(24.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(4.0))
            .when(enabled, |el| el.hover(|el| el.bg(theme::color(cx, "bg-hover"))))
            .tooltip(crate::ui::tooltip::tooltip(explain))
            .child(theme::icon(cx, "refresh", 14.0, theme::color(cx, if enabled { "fg" } else { "fg-faint" })))
            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.rerun(cx)));
        let toolbar = div()
            .flex_none()
            .h(px(30.0))
            .px(px(6.0))
            .flex()
            .items_center()
            .gap(px(4.0))
            .border_b_1()
            .border_color(border)
            .child(rerun)
            .child(div().flex_1())
            .child(div().text_color(muted).child(SharedString::from(self.source.clone())));
        let (operation_label, params_label) = (tx(cx, "Operation"), tx(cx, "Params"));
        let header = div()
            .flex_none()
            .h(px(24.0))
            .px(px(8.0))
            .flex()
            .items_center()
            .text_color(muted)
            .border_b_1()
            .border_color(border)
            .child(div().w(px(260.0)).child(SharedString::from(operation_label)))
            .child(SharedString::from(params_label));
        let rows: Vec<_> = self
            .rows()
            .into_iter()
            .enumerate()
            .map(|(index, row)| {
                let node = row.node;
                let chevron = row.expandable.then(|| {
                    let icon = if row.expanded { "chevron-down" } else { "chevron-right" };
                    div()
                        .id(SharedString::from(format!("db-plan-toggle-{index}")))
                        .debug_selector(move || format!("db-plan-toggle-{index}"))
                        .size(px(16.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(theme::icon(cx, icon, 12.0, theme::color(cx, "fg-subtle")))
                        .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                            view.toggle(node, cx);
                            cx.stop_propagation();
                        }))
                });
                let icon = row.icon.map(|icon| theme::icon(cx, icon, 16.0, theme::color(cx, "fg")));
                div()
                    .id(SharedString::from(format!("db-plan-row-{index}")))
                    .debug_selector(move || format!("db-plan-row-{index}"))
                    .h(theme::length(cx, "h-list-row"))
                    .px(px(8.0))
                    .flex()
                    .items_center()
                    .whitespace_nowrap()
                    .when(self.selected == node, |el| el.bg(selected_bg))
                    .child(
                        div()
                            .w(px(260.0))
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap(px(4.0))
                            .pl(px(16.0 * row.depth as f32))
                            .child(div().w(px(16.0)).children(chevron))
                            .children(icon)
                            .child(SharedString::from(row.operation)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .text_color(muted)
                            .child(SharedString::from(row.params)),
                    )
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.select(node, cx)))
            })
            .collect();
        let empty = rows.is_empty().then(|| {
            let text = tx(cx, "Empty plan");
            div().p(px(12.0)).text_color(muted).child(SharedString::from(text))
        });
        let (property_label, value_label) = (tx(cx, "Property"), tx(cx, "Value"));
        let properties: Vec<_> = self
            .properties()
            .into_iter()
            .map(|property| {
                div()
                    .min_h(px(22.0))
                    .px(px(8.0))
                    .flex()
                    .items_center()
                    .child(div().w(px(140.0)).flex_none().text_color(muted).child(SharedString::from(property.name)))
                    .child(div().flex_1().min_w_0().child(SharedString::from(property.value)))
            })
            .collect();
        div()
            .size_full()
            .flex()
            .flex_col()
            .track_focus(&self.focus)
            .text_size(theme::length(cx, "fs-ui"))
            .text_color(theme::color(cx, "fg"))
            .child(toolbar)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(div().flex_1().min_w_0().flex().flex_col().child(header).child(
                        div().id("db-plan-tree").flex_1().min_h_0().overflow_y_scroll().children(rows).children(empty),
                    ))
                    .child(
                        div()
                            .w(px(320.0))
                            .flex_none()
                            .flex()
                            .flex_col()
                            .border_l_1()
                            .border_color(border)
                            .child(
                                div()
                                    .h(px(24.0))
                                    .px(px(8.0))
                                    .flex()
                                    .items_center()
                                    .text_color(muted)
                                    .border_b_1()
                                    .border_color(border)
                                    .child(div().w(px(140.0)).child(SharedString::from(property_label)))
                                    .child(SharedString::from(value_label)),
                            )
                            .children(properties),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{Entity, Modifiers, TestAppContext, VisualTestContext};
    use std::cell::RefCell;
    use std::rc::Rc;

    fn node(parent: Option<u32>, operation: &str, params: &str) -> PlanNode {
        PlanNode {
            parent,
            operation: operation.into(),
            params: params.into(),
            properties: vec![PlanProperty { name: "Detail".into(), value: format!("{operation} {params}") }],
        }
    }

    fn plan() -> Vec<PlanNode> {
        vec![
            node(None, "Nested Loop", ""),
            node(Some(0), "Full Scan", "table: users;"),
            node(Some(0), "Index Scan", "o"),
        ]
    }

    fn open(cx: &mut TestAppContext) -> (Entity<PlanView>, Rc<RefCell<Vec<PlanEvent>>>, &mut VisualTestContext) {
        cx.update(|cx| cx.set_global(theme::ActiveTheme::default()));
        let (view, cx) = cx.add_window_view(|_, cx| PlanView::new(7, "app", "select * from users", plan(), cx));
        let events: Rc<RefCell<Vec<PlanEvent>>> = Rc::default();
        let log = Rc::clone(&events);
        cx.update(|_, cx| {
            cx.subscribe(&view, move |_, event: &PlanEvent, _| log.borrow_mut().push(event.clone())).detach();
        });
        (view, events, cx)
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        cx.run_until_parked();
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.run_until_parked();
    }

    fn operations(view: &Entity<PlanView>, cx: &mut VisualTestContext) -> Vec<(usize, String)> {
        view.read_with(cx, |v, _| v.rows().into_iter().map(|row| (row.depth, row.operation)).collect())
    }

    #[gpui_kit::test]
    async fn roots_the_operations_in_the_statement_with_everything_expanded(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx);
        let expected = [(0, "Select"), (1, "Nested Loop"), (2, "Full Scan"), (2, "Index Scan")];
        let expected: Vec<(usize, String)> = expected.iter().map(|(d, o)| (*d, (*o).to_owned())).collect();
        assert_eq!(operations(&view, cx), expected);
        let icons: Vec<Option<&str>> = view.read_with(cx, |v, _| v.rows().iter().map(|row| row.icon).collect());
        assert_eq!(icons, [Some("arrow-left"), None, Some("db-table"), Some("db-key")]);
    }

    #[gpui_kit::test]
    async fn selecting_an_operation_shows_its_properties(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx);
        let statement = view.read_with(cx, |v, _| v.properties());
        assert_eq!(statement, [PlanProperty { name: "Operation".into(), value: "select".into() }]);
        click(cx, "db-plan-row-2");
        assert_eq!(view.read_with(cx, |v, _| v.selected()), Some(1));
        let properties = view.read_with(cx, |v, _| v.properties());
        assert_eq!(properties[0].value, "Full Scan table: users;");
    }

    #[gpui_kit::test]
    async fn collapsing_an_operation_hides_what_is_under_it(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx);
        click(cx, "db-plan-toggle-1");
        assert_eq!(operations(&view, cx).len(), 2);
        click(cx, "db-plan-toggle-1");
        assert_eq!(operations(&view, cx).len(), 4);
    }

    #[gpui_kit::test]
    async fn explain_again_asks_once_until_the_new_plan_arrives(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx);
        click(cx, "db-plan-rerun");
        click(cx, "db-plan-rerun");
        assert_eq!(*events.borrow(), [PlanEvent::Rerun(7)]);
        view.update(cx, |v, cx| v.set_plan(7, "app", "select 1", vec![node(None, "Scan", "")], cx));
        assert!(!view.read_with(cx, |v, _| v.is_busy()));
        click(cx, "db-plan-rerun");
        assert_eq!(events.borrow().len(), 2);
    }

    #[gpui_kit::test]
    async fn an_empty_plan_shows_no_rows(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx);
        view.update(cx, |v, cx| v.set_plan(7, "app", "select 1", Vec::new(), cx));
        assert!(view.read_with(cx, |v, _| v.rows().is_empty()));
    }
}
