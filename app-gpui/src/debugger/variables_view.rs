//! The Variables pane of the Debug tool window: the evaluate field, the last evaluation, the
//! watches and the variables of the selected frame (the locals of its first scope, then its other
//! scopes), as a tree whose children are asked for when a node opens.
//!
//! Each row shows a value's name, its type and the value on one line, coloured by what it looks
//! like; a value that differs from what it had at the previous stop, in the same thread and frame
//! depth, has its name highlighted. Set Value and editing a watch happen in a text field in place
//! of the row.
//!
//! [`VariablesView`] owns what it shows and nothing of the session: what it needs from the debug
//! adapter goes out as a request in a [`VariablesViewEvent`] with an id, and the answer comes back
//! through the method named after it ([`VariablesView::scopes_loaded`], [`VariablesView::children_loaded`],
//! [`VariablesView::evaluated`], [`VariablesView::value_set`]). A new stop or another frame drops
//! what was loaded and the answers still under way for it; the open nodes stay open and load again.

use crate::debugger::actions::Debugger;
use crate::debugger::effects::EvaluateContext;
use crate::debugger::focused_view::{DebugView, DebugViewCommands};
use crate::debugger::state::{DebugState, SessionStatus};
use crate::debugger::value_changes::ValueChanges;
use crate::debugger::values::{ValueKind, classify_value, one_line, sort_variables};
use crate::platform::i18n::Localization;
use crate::platform::input;
use crate::theme;
use crate::ui::text_field::{TextField, TextFieldEvent};
use crate::vcs_ops::menu::MenuItem;
use gpui_kit::{
    AnyElement, App, ClickEvent, ClipboardItem, Context, Entity, EventEmitter, FocusHandle, FocusOutEvent, Focusable,
    Hsla, IntoElement, Keystroke, Modifiers, MouseButton, MouseDownEvent, Pixels, Point, Render, ScrollStrategy,
    SharedString, Subscription, UniformListScrollHandle, Window, div, prelude::*, px, uniform_list,
};
use ide_core::DebugSessionId;
use ide_dap::{Scope, Variable};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

/// What the Variables pane asks of the workbench. Every request carries an id for its answer.
#[derive(Clone, Debug, PartialEq)]
pub enum VariablesViewEvent {
    /// The scopes of `frame` (`ide_host::ops::debug::debug_scopes`), for
    /// [`VariablesView::scopes_loaded`].
    LoadScopes { request: u64, session: DebugSessionId, frame: i64 },
    /// The children of a variables reference (`debug_variables`), for [`VariablesView::children_loaded`].
    LoadChildren { request: u64, session: DebugSessionId, reference: i64 },
    /// Evaluate `expression` in the selected frame: the call `DebugState::evaluate` builds, for
    /// [`VariablesView::evaluated`].
    Evaluate { request: u64, session: DebugSessionId, expression: String, context: EvaluateContext },
    /// Set Value: `DebugState::set_variable_value`, then `DebugState::variable_set` and
    /// [`VariablesView::value_set`] with its answer.
    SetValue {
        request: u64,
        session: DebugSessionId,
        container: i64,
        name: String,
        evaluate_name: Option<String>,
        value: String,
    },
    /// `DebugState::add_watch`.
    AddWatch(String),
    /// `DebugState::remove_watch`.
    RemoveWatch(String),
    /// The watch `old` now reads `new`, in its place.
    EditWatch { old: String, new: String },
    /// Copy Value of a structured value: its text with its contents (`value_text`), walking the
    /// children of `reference`.
    CopyValue { session: DebugSessionId, value: String, reference: i64 },
    /// Add Field Watchpoint: ask the adapter for the data breakpoint of `name` in `container`
    /// (`debug_data_breakpoint_info`), then `SpecialBreakpoints::add_field_watchpoint` under `label`.
    AddFieldWatchpoint { session: DebugSessionId, container: i64, name: String, label: String },
    /// Open the selected frame's source; for a local of the frame, at its declaration.
    JumpToSource { local: Option<String> },
    /// Show [`VariablesView::variable_menu`] here.
    ShowMenu(Point<Pixels>),
    /// The pane took the keyboard focus: `FocusedView::focus` with these commands.
    Focused(DebugViewCommands),
    /// The pane lost the keyboard focus: `FocusedView::blur`.
    Blurred(DebugView),
}

/// A row of the context menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VariableCommand {
    SetValue,
    EditWatch,
    RemoveWatch,
    NewWatch,
    CopyValue,
    CopyName,
    AddToWatches,
    AddFieldWatchpoint,
    JumpToSource,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NodeKind {
    /// The last expression evaluated in the evaluate field.
    Eval,
    Watch,
    Scope,
    Var,
    /// Why a node has no children: none, or the adapter's error.
    Message,
}

#[derive(Clone, Debug)]
struct Node {
    key: String,
    kind: NodeKind,
    name: String,
    value: String,
    type_name: Option<String>,
    /// Above 0 when the node has children.
    reference: i64,
    evaluate_name: Option<String>,
    error: Option<String>,
    /// A root of the tree (a local of the frame, a watch), not a member of another value.
    depth0: bool,
    /// The variables reference of the scope or value holding the node (0 when unknown).
    container: i64,
}

#[derive(Clone, Debug)]
struct Row {
    node: Node,
    depth: usize,
    expanded: bool,
}

/// The frame the variables are of, at one stop.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Target {
    session: DebugSessionId,
    frame: i64,
    stop_seq: u64,
    /// Where values are compared across stops: the session, its thread and the frame's depth.
    scope: String,
}

#[derive(Clone, Debug)]
struct FrameData {
    scopes: Vec<Scope>,
    locals: Vec<Variable>,
}

/// What an answer is for. Those tied to a generation are dropped once the view moved on.
#[derive(Clone, Debug)]
enum Request {
    Scopes { generation: u64 },
    Locals { generation: u64, scopes: Vec<Scope> },
    Children { generation: u64, key: String, reference: i64 },
    Watch { generation: u64, expression: String },
    Eval { expression: String },
    SetValue,
}

impl Request {
    fn generation(&self) -> Option<u64> {
        match self {
            Self::Scopes { generation }
            | Self::Locals { generation, .. }
            | Self::Children { generation, .. }
            | Self::Watch { generation, .. } => Some(*generation),
            Self::Eval { .. } | Self::SetValue => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum EditKind {
    SetValue,
    NewWatch,
    /// Editing the watch with this expression.
    EditWatch(String),
}

#[derive(Clone, Debug)]
struct Editing {
    /// The row edited in place.
    key: String,
    kind: EditKind,
    /// Why the adapter refused the value.
    error: Option<String>,
    /// The Set Value under way.
    request: Option<u64>,
}

/// The key of the row a new watch is typed in.
const NEW_WATCH_KEY: &str = "\u{0}new-watch";

/// What the pane can do for the global actions while it has the focus.
const COMMANDS: DebugViewCommands =
    DebugViewCommands { view: DebugView::Variables, set_value: true, jump_to_source: true };

pub struct VariablesView {
    debugger: Entity<Debugger>,
    focus: FocusHandle,
    scroll: UniformListScrollHandle,
    eval_field: Entity<TextField>,
    eval_placeholder: String,
    edit_field: Entity<TextField>,
    /// Data Views › Sort values alphabetically.
    sort_values: bool,
    target: Option<Target>,
    /// Counts the targets shown: what was asked for an earlier one is stale.
    generation: u64,
    frame_data: Option<FrameData>,
    /// Loaded children, by the key of their parent.
    children: HashMap<String, Vec<Node>>,
    /// Nodes whose children are being loaded.
    loading: HashSet<String>,
    /// Open nodes; they stay open across stops.
    expanded: HashSet<String>,
    watch_results: HashMap<String, Result<Variable, String>>,
    /// The watch expressions as last read from the debugger.
    shown_watches: Vec<String>,
    /// The active session had finished when last read (the note says so).
    shown_finished: bool,
    /// Watches evaluated (or under way) for the current target.
    watch_requested: HashSet<String>,
    last_eval: Option<(String, Result<Variable, String>)>,
    pending: HashMap<u64, Request>,
    requests: u64,
    selected: Option<String>,
    /// The row the context menu was opened on.
    menu_key: Option<String>,
    editing: Option<Editing>,
    changes: ValueChanges,
    key_claims: Vec<u64>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<VariablesViewEvent> for VariablesView {}

impl VariablesView {
    /// The pane over `debugger`'s active session. Its first requests go out on the next turn of
    /// the event loop, once the caller has subscribed.
    pub fn new(debugger: Entity<Debugger>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let eval_placeholder = cx.default_global::<Localization>().0.tx("Evaluate expression (↩) or add a watch (⇧⌘↩)");
        let placeholder = eval_placeholder.clone();
        let eval_field = cx.new(|cx| TextField::new(cx).with_placeholder(placeholder));
        let edit_field = cx.new(TextField::new);
        let focus = cx.focus_handle();
        let (eval_focus, edit_focus) = (eval_field.read(cx).focus_handle(cx), edit_field.read(cx).focus_handle(cx));
        let tree = cx.weak_entity();
        let tree_claim = input::claim_keys(
            focus.clone(),
            move |keystroke, window, cx| {
                tree.update(cx, |view, cx| view.tree_key(keystroke, window, cx)).unwrap_or(false)
            },
            cx,
        );
        let evaluate = cx.weak_entity();
        let eval_claim = input::claim_keys(
            eval_focus,
            move |keystroke, window, cx| {
                evaluate.update(cx, |view, cx| view.eval_key(keystroke, window, cx)).unwrap_or(false)
            },
            cx,
        );
        let edit = cx.weak_entity();
        let edit_claim = input::claim_keys(
            edit_focus.clone(),
            move |keystroke, window, cx| {
                edit.update(cx, |view, cx| view.edit_key(keystroke, window, cx)).unwrap_or(false)
            },
            cx,
        );
        cx.on_release(|view, cx| {
            for claim in &view.key_claims {
                input::release_keys(*claim, cx);
            }
        })
        .detach();
        let subscriptions = vec![
            cx.observe(&debugger, |view, _, cx| view.refresh(cx)),
            cx.subscribe(&edit_field, |view, _, event: &TextFieldEvent, cx| {
                if *event == TextFieldEvent::Changed
                    && let Some(editing) = view.editing.as_mut()
                {
                    editing.error = None;
                    cx.notify();
                }
            }),
            cx.on_focus_in(&focus, window, |_, _, cx| cx.emit(VariablesViewEvent::Focused(COMMANDS))),
            cx.on_focus_out(&focus, window, |_, _: FocusOutEvent, _, cx| {
                cx.emit(VariablesViewEvent::Blurred(DebugView::Variables));
            }),
            cx.on_focus_out(&edit_focus, window, |view, _: FocusOutEvent, _, cx| view.edit_blurred(cx)),
        ];
        cx.spawn(async move |view, cx| {
            let _ = view.update(cx, |view: &mut VariablesView, cx| view.refresh(cx));
        })
        .detach();
        Self {
            debugger,
            focus,
            scroll: UniformListScrollHandle::new(),
            eval_field,
            eval_placeholder,
            edit_field,
            sort_values: false,
            target: None,
            generation: 0,
            frame_data: None,
            children: HashMap::new(),
            loading: HashSet::new(),
            expanded: HashSet::new(),
            watch_results: HashMap::new(),
            shown_watches: Vec::new(),
            shown_finished: false,
            watch_requested: HashSet::new(),
            last_eval: None,
            pending: HashMap::new(),
            requests: 0,
            selected: None,
            menu_key: None,
            editing: None,
            changes: ValueChanges::default(),
            key_claims: vec![tree_claim, eval_claim, edit_claim],
            _subscriptions: subscriptions,
        }
    }

    /// Data Views › Sort values alphabetically: applies to what loads from now on.
    pub fn set_sort_values(&mut self, sort: bool) {
        self.sort_values = sort;
    }

    /// Brings the pane in line with the debugger: another frame or stop starts afresh (asking for
    /// its scopes), and every watch not evaluated for it yet is asked for. Safe to call any time;
    /// it redraws only when what it shows changed.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        let (target, watches, finished) = {
            let state = &self.debugger.read(cx).state;
            let finished = state.active_session().is_some_and(|session| session.status == SessionStatus::Terminated);
            (target_of(state), state.watches().to_vec(), finished)
        };
        let changed = target != self.target || watches != self.shown_watches || finished != self.shown_finished;
        self.shown_finished = finished;
        if target != self.target {
            self.generation += 1;
            self.target.clone_from(&target);
            self.frame_data = None;
            self.children.clear();
            self.loading.clear();
            self.watch_results.clear();
            self.watch_requested.clear();
            let generation = self.generation;
            self.pending.retain(|_, request| request.generation().is_none_or(|of| of == generation));
            if let Some(target) = &target {
                self.changes.enter(&target.scope, target.stop_seq);
                let request = self.request(Request::Scopes { generation });
                cx.emit(VariablesViewEvent::LoadScopes { request, session: target.session, frame: target.frame });
            }
        }
        if let Some(target) = &target {
            for expression in watches.iter().cloned() {
                if self.watch_requested.insert(expression.clone()) {
                    let watch = Request::Watch { generation: self.generation, expression: expression.clone() };
                    let request = self.request(watch);
                    let context = EvaluateContext::Watch;
                    cx.emit(VariablesViewEvent::Evaluate { request, session: target.session, expression, context });
                }
            }
        }
        self.shown_watches = watches;
        if changed {
            cx.notify();
        }
    }

    fn request(&mut self, request: Request) -> u64 {
        self.requests += 1;
        self.pending.insert(self.requests, request);
        self.requests
    }

    // ---------------------------------------------------------------------------------------------
    // Answers

    /// The scopes of the frame arrived: the locals of the first one that is not expensive are
    /// asked for next. A failure (the frame went away) shows no variables.
    pub fn scopes_loaded(&mut self, request: u64, result: Result<Vec<Scope>, String>, cx: &mut Context<Self>) {
        let Some(Request::Scopes { generation }) = self.pending.remove(&request) else { return };
        let Some(target) = self.target.clone().filter(|_| generation == self.generation) else { return };
        let scopes = result.unwrap_or_else(|error| {
            tracing::debug!("debugger: no scopes for frame {}: {error}", target.frame);
            Vec::new()
        });
        match scopes.iter().find(|scope| !scope.expensive).map(|scope| scope.variables_reference) {
            Some(reference) => {
                let request = self.request(Request::Locals { generation, scopes });
                cx.emit(VariablesViewEvent::LoadChildren { request, session: target.session, reference });
            }
            None => {
                self.frame_data = Some(FrameData { scopes, locals: Vec::new() });
                self.data_changed(cx);
            }
        }
    }

    /// The children of a variables reference arrived: the frame's locals, or a node's members
    /// (none, or the adapter's error, show as a message under the node).
    pub fn children_loaded(&mut self, request: u64, result: Result<Vec<Variable>, String>, cx: &mut Context<Self>) {
        match self.pending.remove(&request) {
            Some(Request::Locals { generation, scopes }) if generation == self.generation => {
                let locals = match result {
                    Ok(variables) => self.sorted(variables),
                    Err(error) => {
                        tracing::debug!("debugger: no locals: {error}");
                        Vec::new()
                    }
                };
                self.frame_data = Some(FrameData { scopes, locals });
            }
            Some(Request::Children { generation, key, reference }) if generation == self.generation => {
                self.loading.remove(&key);
                let nodes = match result {
                    Ok(variables) if variables.is_empty() => vec![message(&key, "empty", tx(cx, "No variables"))],
                    Ok(variables) => child_nodes(&format!("{key}/"), reference, &self.sorted(variables), false),
                    Err(error) => vec![message(&key, "error", error)],
                };
                self.children.insert(key, nodes);
            }
            _ => return,
        }
        self.data_changed(cx);
    }

    /// A watch or the evaluate field's expression was evaluated.
    pub fn evaluated(&mut self, request: u64, result: Result<Variable, String>, cx: &mut Context<Self>) {
        match self.pending.remove(&request) {
            Some(Request::Watch { generation, expression }) if generation == self.generation => {
                self.forget_children(&watch_key(&expression));
                self.watch_results.insert(expression, result);
            }
            Some(Request::Eval { expression }) => {
                let previous = self.last_eval.as_ref().map(|(expression, _)| eval_key(expression));
                if let Some(previous) = previous {
                    self.forget_children(&previous);
                }
                self.forget_children(&eval_key(&expression));
                self.last_eval = Some((expression, result));
            }
            _ => return,
        }
        self.data_changed(cx);
    }

    /// The Set Value under way was answered: the field closes, or says why the value was refused.
    /// The debugger reads the variables again after a success (its stop sequence moves on).
    pub fn value_set(
        &mut self,
        request: u64,
        result: Result<Variable, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !matches!(self.pending.remove(&request), Some(Request::SetValue)) {
            return;
        }
        let Some(editing) = self.editing.as_mut().filter(|editing| editing.request == Some(request)) else { return };
        match result {
            Ok(_) => self.end_edit(window, cx),
            Err(error) => {
                editing.request = None;
                editing.error = Some(error);
                cx.notify();
            }
        }
    }

    fn sorted(&self, variables: Vec<Variable>) -> Vec<Variable> {
        fn name(variable: &Variable) -> &str {
            &variable.name
        }
        if self.sort_values { sort_variables(&variables, name) } else { variables }
    }

    /// Drops the loaded children of `key` and of everything below it.
    fn forget_children(&mut self, key: &str) {
        let below = format!("{key}/");
        self.children.retain(|loaded, _| loaded != key && !loaded.starts_with(&below));
        self.loading.retain(|loaded| loaded != key && !loaded.starts_with(&below));
    }

    fn data_changed(&mut self, cx: &mut Context<Self>) {
        self.record_values(cx);
        self.load_expanded(cx);
        cx.notify();
    }

    /// Remembers the values shown now, for the next stop to compare with.
    fn record_values(&mut self, cx: &App) {
        let Some(scope) = self.target.as_ref().map(|target| target.scope.clone()) else { return };
        for row in self.rows(cx).iter().filter(|row| tracks_changes(&row.node)) {
            self.changes.record(&scope, &row.node.key, &row.node.value);
        }
    }

    /// Asks for the children of every open node that has none loaded.
    fn load_expanded(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.target.as_ref().map(|target| target.session) else { return };
        let wanted: Vec<(String, i64)> = self
            .rows(cx)
            .into_iter()
            .filter(|row| {
                row.expanded && !self.children.contains_key(&row.node.key) && !self.loading.contains(&row.node.key)
            })
            .map(|row| (row.node.key, row.node.reference))
            .collect();
        for (key, reference) in wanted {
            self.loading.insert(key.clone());
            let request = self.request(Request::Children { generation: self.generation, key, reference });
            cx.emit(VariablesViewEvent::LoadChildren { request, session, reference });
        }
    }

    // ---------------------------------------------------------------------------------------------
    // The tree

    fn roots(&self, cx: &App) -> Vec<Node> {
        let mut out = Vec::new();
        if let Some((expression, result)) = &self.last_eval {
            out.push(evaluated_node(NodeKind::Eval, eval_key(expression), expression, Some(result)));
        }
        let suspended = self.target.is_some();
        for watch in self.debugger.read(cx).state.watches() {
            let mut node = evaluated_node(NodeKind::Watch, watch_key(watch), watch, self.watch_results.get(watch));
            if !suspended {
                node.error = Some(tx(cx, "Not suspended"));
            }
            out.push(node);
        }
        if self.editing.as_ref().is_some_and(|editing| editing.kind == EditKind::NewWatch) {
            out.push(evaluated_node(NodeKind::Watch, NEW_WATCH_KEY.to_owned(), "", None));
        }
        if let Some(data) = self.frame_data.as_ref().filter(|_| suspended) {
            let first = data.scopes.iter().position(|scope| !scope.expensive);
            if let Some(scope) = first.and_then(|at| data.scopes.get(at)) {
                out.extend(child_nodes("local:", scope.variables_reference, &data.locals, true));
            }
            for (_, scope) in data.scopes.iter().enumerate().filter(|(at, _)| Some(*at) != first) {
                out.push(Node {
                    key: format!("scope:{}", scope.name),
                    kind: NodeKind::Scope,
                    name: scope.name.clone(),
                    value: String::new(),
                    type_name: None,
                    reference: scope.variables_reference,
                    evaluate_name: None,
                    error: None,
                    depth0: true,
                    container: 0,
                });
            }
        }
        out
    }

    fn rows(&self, cx: &App) -> Vec<Row> {
        let mut out = Vec::new();
        for node in self.roots(cx) {
            self.push_row(node, 0, &mut out);
        }
        out
    }

    fn push_row(&self, node: Node, depth: usize, out: &mut Vec<Row>) {
        let expanded = node.reference > 0 && self.expanded.contains(&node.key);
        let children = if expanded { self.children.get(&node.key) } else { None };
        out.push(Row { node, depth, expanded });
        for child in children.into_iter().flatten() {
            self.push_row(child.clone(), depth + 1, out);
        }
    }

    fn find_node(&self, key: &str, cx: &App) -> Option<Node> {
        self.rows(cx).into_iter().find(|row| row.node.key == key).map(|row| row.node)
    }

    fn selected_node(&self, cx: &App) -> Option<Node> {
        self.selected.as_deref().and_then(|key| self.find_node(key, cx))
    }

    fn is_changed(&self, node: &Node) -> bool {
        tracks_changes(node)
            && self.target.as_ref().is_some_and(|target| self.changes.changed(&target.scope, &node.key, &node.value))
    }

    /// Set Value applies: a value of the suspended frame or a watch, which the adapter can find
    /// by its container or its expression.
    fn editable(&self, node: &Node) -> bool {
        self.target.is_some()
            && matches!(node.kind, NodeKind::Var | NodeKind::Watch)
            && node.key != NEW_WATCH_KEY
            && (node.container > 0 || node.evaluate_name.is_some())
    }

    /// Add Field Watchpoint applies: a member the adapter can put a data breakpoint on.
    fn can_watch(&self, node: &Node, cx: &App) -> bool {
        let session = self.debugger.read(cx).state.active_session();
        let supported = session.is_some_and(|session| session.supports(|caps| caps.data_breakpoints));
        supported && self.target.is_some() && node.kind == NodeKind::Var && node.container > 0
    }

    // ---------------------------------------------------------------------------------------------
    // What is shown

    /// Each row: its name, `= {type} value` on one line (or `= error`), indented two spaces a level.
    pub fn variable_lines(&self, cx: &App) -> Vec<String> {
        self.rows(cx).iter().filter(|row| row.node.key != NEW_WATCH_KEY).map(row_text).collect()
    }

    /// The names of the rows whose value changed since the previous stop.
    pub fn changed_names(&self, cx: &App) -> Vec<String> {
        self.rows(cx).into_iter().filter(|row| self.is_changed(&row.node)).map(|row| row.node.name).collect()
    }

    /// What the pane says instead of a tree: nothing is suspended and there is nothing to watch.
    pub fn note(&self, cx: &App) -> Option<String> {
        let state = &self.debugger.read(cx).state;
        if self.target.is_some() || !state.watches().is_empty() || self.last_eval.is_some() {
            return None;
        }
        let finished = state.active_session().is_some_and(|session| session.status == SessionStatus::Terminated);
        Some(tx(cx, if finished { "The session has finished." } else { "Variables are available while suspended." }))
    }

    pub fn eval_field(&self) -> &Entity<TextField> {
        &self.eval_field
    }

    pub fn eval_placeholder(&self) -> &str {
        &self.eval_placeholder
    }

    /// The field of the row edited in place.
    pub fn edit_field(&self) -> &Entity<TextField> {
        &self.edit_field
    }

    pub fn is_editing(&self) -> bool {
        self.editing.is_some()
    }

    /// Why the adapter refused the value typed in the edited row.
    pub fn edit_error(&self) -> Option<&str> {
        self.editing.as_ref().and_then(|editing| editing.error.as_deref())
    }

    /// The selected row's name.
    pub fn selected_name(&self, cx: &App) -> Option<String> {
        self.selected_node(cx).map(|node| node.name)
    }

    // ---------------------------------------------------------------------------------------------
    // Commands

    /// Opens or closes a node; an opened node without children loaded asks for them.
    fn toggle(&mut self, key: &str, cx: &mut Context<Self>) {
        if !self.expanded.remove(key) {
            self.expanded.insert(key.to_owned());
            self.load_expanded(cx);
        }
        cx.notify();
    }

    /// Set Value (F2) on the selected row.
    pub fn begin_set_value(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(node) = self.selected_node(cx).filter(|node| self.editable(node)) else { return };
        self.begin_edit(node.key, EditKind::SetValue, &node.value, window, cx);
    }

    /// New Watch: an empty row after the watches takes the expression.
    pub fn begin_new_watch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.begin_edit(NEW_WATCH_KEY.to_owned(), EditKind::NewWatch, "", window, cx);
    }

    fn begin_edit(&mut self, key: String, kind: EditKind, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.selected = Some(key.clone());
        self.editing = Some(Editing { key, kind, error: None, request: None });
        self.edit_field.update(cx, |field, cx| {
            field.set_text(text, cx);
            field.select_all(cx);
        });
        let focus = self.edit_field.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
        cx.notify();
    }

    /// Enter in the edited row: Set Value asks the adapter (the row stays until it answers); a new
    /// or edited watch takes the expression, an emptied one goes.
    fn commit_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editing) = self.editing.clone().filter(|editing| editing.request.is_none()) else { return };
        let text = self.edit_field.read(cx).text().to_owned();
        match editing.kind {
            EditKind::SetValue => {
                let node = self.find_node(&editing.key, cx);
                let (Some(node), Some(target)) = (node, self.target.clone()) else {
                    self.end_edit(window, cx);
                    return;
                };
                if text == node.value {
                    self.end_edit(window, cx);
                    return;
                }
                let request = self.request(Request::SetValue);
                if let Some(editing) = self.editing.as_mut() {
                    editing.request = Some(request);
                    editing.error = None;
                }
                cx.emit(VariablesViewEvent::SetValue {
                    request,
                    session: target.session,
                    container: node.container,
                    name: node.name,
                    evaluate_name: node.evaluate_name,
                    value: text,
                });
                cx.notify();
            }
            EditKind::NewWatch => {
                let expression = text.trim();
                if !expression.is_empty() {
                    cx.emit(VariablesViewEvent::AddWatch(expression.to_owned()));
                }
                self.end_edit(window, cx);
            }
            EditKind::EditWatch(old) => {
                let expression = text.trim();
                if expression.is_empty() {
                    cx.emit(VariablesViewEvent::RemoveWatch(old));
                } else if expression != old {
                    cx.emit(VariablesViewEvent::EditWatch { old, new: expression.to_owned() });
                }
                self.end_edit(window, cx);
            }
        }
    }

    /// Closes the edited row; the keyboard goes back to the tree if the row had it.
    fn end_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let had_focus = self.edit_field.read(cx).focus_handle(cx).is_focused(window);
        self.editing = None;
        if had_focus {
            window.focus(&self.focus, cx);
        }
        cx.notify();
    }

    /// The edited row lost the keyboard: it closes, unless a Set Value is under way.
    fn edit_blurred(&mut self, cx: &mut Context<Self>) {
        if self.editing.as_ref().is_some_and(|editing| editing.request.is_none()) {
            self.editing = None;
            cx.notify();
        }
    }

    /// Jump to Source (F4) on the selected row.
    pub fn jump_to_source(&mut self, cx: &mut Context<Self>) {
        let node = self.selected_node(cx);
        self.jump_to(node.as_ref(), cx);
    }

    fn jump_to(&mut self, node: Option<&Node>, cx: &mut Context<Self>) {
        let local = node.filter(|node| node.depth0 && node.kind == NodeKind::Var).map(|node| node.name.clone());
        cx.emit(VariablesViewEvent::JumpToSource { local });
    }

    /// Copy Value: a scalar is copied as shown; a structured value with its contents, which the
    /// workbench walks.
    fn copy_value(&mut self, node: &Node, cx: &mut Context<Self>) {
        match self.target.as_ref().filter(|_| node.reference > 0) {
            Some(target) => cx.emit(VariablesViewEvent::CopyValue {
                session: target.session,
                value: node.value.clone(),
                reference: node.reference,
            }),
            None => cx.write_to_clipboard(ClipboardItem::new_string(node.value.clone())),
        }
    }

    /// The context menu of the row it was opened on.
    pub fn variable_menu(&self, cx: &App) -> Vec<MenuItem<VariableCommand>> {
        let Some(node) = self.menu_key.as_deref().and_then(|key| self.find_node(key, cx)) else { return Vec::new() };
        if node.kind == NodeKind::Message || node.key == NEW_WATCH_KEY {
            return Vec::new();
        }
        let mut items = Vec::new();
        if self.editable(&node) {
            items.push(MenuItem::item("Set Value…", VariableCommand::SetValue).icon("edit").shortcut("F2"));
            items.push(MenuItem::Separator);
        }
        if node.kind == NodeKind::Watch {
            items.push(MenuItem::item("Edit Watch…", VariableCommand::EditWatch));
            items.push(MenuItem::item("Remove Watch", VariableCommand::RemoveWatch).icon("minus"));
        }
        if node.kind != NodeKind::Scope {
            items.push(MenuItem::item("Copy Value", VariableCommand::CopyValue).icon("copy"));
            items.push(MenuItem::item("Copy Name", VariableCommand::CopyName));
            if node.kind == NodeKind::Var && node.evaluate_name.is_some() {
                items.push(MenuItem::item("Add to Watches", VariableCommand::AddToWatches).icon("eye"));
            }
            if self.can_watch(&node, cx) {
                let item = MenuItem::item("Add Field Watchpoint", VariableCommand::AddFieldWatchpoint);
                items.push(item.icon("breakpoint-field"));
            }
            items.push(MenuItem::Separator);
            items.push(MenuItem::item("Jump to Source", VariableCommand::JumpToSource).shortcut("F4"));
        }
        items.push(MenuItem::Separator);
        items.push(MenuItem::item("New Watch…", VariableCommand::NewWatch).icon("plus"));
        items
    }

    pub fn run_variable_command(&mut self, command: VariableCommand, window: &mut Window, cx: &mut Context<Self>) {
        let Some(node) = self.menu_key.clone().and_then(|key| self.find_node(&key, cx)) else { return };
        match command {
            VariableCommand::SetValue => {
                self.selected = Some(node.key);
                self.begin_set_value(window, cx);
            }
            VariableCommand::EditWatch => {
                self.begin_edit(node.key, EditKind::EditWatch(node.name.clone()), &node.name, window, cx);
            }
            VariableCommand::RemoveWatch => cx.emit(VariablesViewEvent::RemoveWatch(node.name)),
            VariableCommand::NewWatch => self.begin_new_watch(window, cx),
            VariableCommand::CopyValue => self.copy_value(&node, cx),
            VariableCommand::CopyName => cx.write_to_clipboard(ClipboardItem::new_string(node.name)),
            VariableCommand::AddToWatches => {
                if let Some(expression) = node.evaluate_name {
                    cx.emit(VariablesViewEvent::AddWatch(expression));
                }
            }
            VariableCommand::AddFieldWatchpoint => {
                if let Some(target) = self.target.as_ref() {
                    cx.emit(VariablesViewEvent::AddFieldWatchpoint {
                        session: target.session,
                        container: node.container,
                        label: node.evaluate_name.unwrap_or_else(|| node.name.clone()),
                        name: node.name,
                    });
                }
            }
            VariableCommand::JumpToSource => self.jump_to(Some(&node), cx),
        }
    }

    /// Enter in the evaluate field evaluates its expression in the selected frame.
    fn evaluate_field(&mut self, cx: &mut Context<Self>) {
        let expression = self.eval_field.read(cx).text().trim().to_owned();
        let Some(session) = self.target.as_ref().map(|target| target.session).filter(|_| !expression.is_empty()) else {
            return;
        };
        let request = self.request(Request::Eval { expression: expression.clone() });
        let context = EvaluateContext::Repl;
        cx.emit(VariablesViewEvent::Evaluate { request, session, expression, context });
    }

    /// ⇧⌘↩ or the Add button: the evaluate field's expression becomes a watch; with an empty field
    /// the button opens a new watch row.
    fn add_watch_from_field(&mut self, window: Option<&mut Window>, cx: &mut Context<Self>) {
        let expression = self.eval_field.read(cx).text().trim().to_owned();
        if expression.is_empty() {
            if let Some(window) = window {
                self.begin_new_watch(window, cx);
            }
            return;
        }
        cx.emit(VariablesViewEvent::AddWatch(expression));
        self.eval_field.update(cx, |field, cx| field.set_text("", cx));
    }

    // ---------------------------------------------------------------------------------------------
    // Keys and pointer

    /// The tree's keys: arrows, Home and End move the selection; Right opens a node, then steps
    /// into it; Left closes it, then goes to its parent; Enter opens or closes; F2 sets the value;
    /// Delete removes a watch.
    fn tree_key(&mut self, keystroke: &Keystroke, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if keystroke.modifiers.modified() {
            return false;
        }
        let rows = self.rows(cx);
        let current = self.selected.as_ref().and_then(|key| rows.iter().position(|row| &row.node.key == key));
        let row = current.and_then(|at| rows.get(at));
        let last = rows.len().saturating_sub(1);
        let key_at = |at: usize| rows.get(at).map(|row| row.node.key.clone());
        match (keystroke.key.as_str(), row) {
            ("down", _) => self.selected = key_at(current.map_or(0, |at| (at + 1).min(last))),
            ("up", _) => self.selected = key_at(current.map_or(0, |at| at.saturating_sub(1))),
            ("home", _) => self.selected = key_at(0),
            ("end", _) => self.selected = key_at(last),
            ("right", Some(row)) if row.node.reference > 0 && !row.expanded => self.toggle(&row.node.key, cx),
            ("right", Some(row)) if row.expanded => self.selected = current.and_then(|at| key_at(at + 1)),
            ("left", Some(row)) if row.expanded => self.toggle(&row.node.key, cx),
            ("left", Some(row)) => {
                let above = current.map_or(&rows[..0], |at| &rows[..at]);
                if let Some(parent) = above.iter().rev().find(|candidate| candidate.depth < row.depth) {
                    self.selected = Some(parent.node.key.clone());
                }
            }
            ("enter", Some(row)) if row.node.reference > 0 => self.toggle(&row.node.key, cx),
            ("f2", Some(_)) => self.begin_set_value(window, cx),
            ("delete" | "backspace", Some(row)) if row.node.kind == NodeKind::Watch => {
                cx.emit(VariablesViewEvent::RemoveWatch(row.node.name.clone()));
            }
            _ => return false,
        }
        let shown = self.rows(cx);
        if let Some(at) = self.selected.as_ref().and_then(|key| shown.iter().position(|row| &row.node.key == key)) {
            self.scroll.scroll_to_item(at, ScrollStrategy::Top);
        }
        cx.notify();
        true
    }

    /// The evaluate field's keys: Enter evaluates, ⇧⌘↩ adds a watch.
    fn eval_key(&mut self, keystroke: &Keystroke, _: &mut Window, cx: &mut Context<Self>) -> bool {
        let modifiers = &keystroke.modifiers;
        if keystroke.key != "enter" || modifiers.alt {
            return false;
        }
        if modifiers.shift && primary(modifiers) {
            self.add_watch_from_field(None, cx);
        } else if !modifiers.modified() {
            self.evaluate_field(cx);
        } else {
            return false;
        }
        true
    }

    /// The edited row's keys: Enter commits, Escape gives up.
    fn edit_key(&mut self, keystroke: &Keystroke, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if keystroke.modifiers.modified() {
            return false;
        }
        match keystroke.key.as_str() {
            "enter" => self.commit_edit(window, cx),
            "escape" => self.end_edit(window, cx),
            _ => return false,
        }
        true
    }

    fn clicked_row(&mut self, key: &str, toggle: bool, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        self.selected = Some(key.to_owned());
        if toggle {
            self.toggle(key, cx);
        }
        cx.notify();
    }

    fn opened_menu(&mut self, key: &str, at: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        self.selected = Some(key.to_owned());
        self.menu_key = Some(key.to_owned());
        cx.emit(VariablesViewEvent::ShowMenu(at));
        cx.notify();
    }

    /// Opens the context menu on the row with `name`, as a right click on it does.
    pub fn open_menu_on(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let key = self.rows(cx).into_iter().find(|row| row.node.name == name).map(|row| row.node.key);
        if let Some(key) = key {
            self.opened_menu(&key, Point::default(), window, cx);
        }
    }
}

/// The frame the variables are of: the active session's selected frame while it is suspended.
fn target_of(state: &DebugState) -> Option<Target> {
    let session = state.active_session().filter(|session| session.status == SessionStatus::Paused)?;
    let frame = session.selected_frame?;
    let depth = session.frames.iter().position(|candidate| candidate.id == frame);
    let scope = format!("{}:{:?}:{:?}", session.id.0, session.selected_thread, depth);
    Some(Target { session: session.id, frame, stop_seq: state.stop_seq(), scope })
}

fn watch_key(expression: &str) -> String {
    format!("watch:{expression}")
}

fn eval_key(expression: &str) -> String {
    format!("eval:{expression}")
}

/// A watch or an evaluation: its expression, and its value or error once evaluated.
fn evaluated_node(kind: NodeKind, key: String, name: &str, result: Option<&Result<Variable, String>>) -> Node {
    let value = result.and_then(|result| result.as_ref().ok());
    Node {
        key,
        kind,
        name: name.to_owned(),
        value: value.map(|v| v.value.clone()).unwrap_or_default(),
        type_name: value.and_then(|v| v.type_name.clone()),
        reference: value.map_or(0, |v| v.variables_reference),
        evaluate_name: Some(value.and_then(|v| v.evaluate_name.clone()).unwrap_or_else(|| name.to_owned())),
        error: result.and_then(|result| result.as_ref().err().cloned()),
        depth0: true,
        container: 0,
    }
}

/// The nodes of `variables`, held by `container`, keyed under `prefix`; a repeated name (a
/// shadowed local) gets its ordinal.
fn child_nodes(prefix: &str, container: i64, variables: &[Variable], depth0: bool) -> Vec<Node> {
    let mut seen: HashMap<&str, usize> = HashMap::new();
    variables
        .iter()
        .map(|variable| {
            let count = seen.entry(variable.name.as_str()).or_default();
            *count += 1;
            let key = if *count > 1 {
                format!("{prefix}{}#{count}", variable.name)
            } else {
                format!("{prefix}{}", variable.name)
            };
            Node {
                key,
                kind: NodeKind::Var,
                name: variable.name.clone(),
                value: variable.value.clone(),
                type_name: variable.type_name.clone(),
                reference: variable.variables_reference,
                evaluate_name: variable.evaluate_name.clone(),
                error: None,
                depth0,
                container,
            }
        })
        .collect()
}

/// A row under `parent` that says why it has no children.
fn message(parent: &str, tag: &str, text: String) -> Node {
    Node {
        key: format!("{parent}/\u{0}{tag}"),
        kind: NodeKind::Message,
        name: text,
        value: String::new(),
        type_name: None,
        reference: 0,
        evaluate_name: None,
        error: None,
        depth0: false,
        container: 0,
    }
}

fn tracks_changes(node: &Node) -> bool {
    matches!(node.kind, NodeKind::Var | NodeKind::Watch) && node.error.is_none() && node.key != NEW_WATCH_KEY
}

fn row_text(row: &Row) -> String {
    let indent = "  ".repeat(row.depth);
    let node = &row.node;
    match node.kind {
        NodeKind::Scope | NodeKind::Message => format!("{indent}{}", node.name),
        _ => match &node.error {
            Some(error) => format!("{indent}{} = {}", node.name, one_line(error)),
            None => {
                let type_name = node.type_name.as_ref().map(|t| format!("{{{t}}} ")).unwrap_or_default();
                format!("{indent}{} = {type_name}{}", node.name, one_line(&node.value)).trim_end().to_owned()
            }
        },
    }
}

fn node_icon(node: &Node) -> &'static str {
    match node.kind {
        NodeKind::Watch => "eye",
        NodeKind::Eval => "evaluate-expression",
        NodeKind::Scope => "folder",
        NodeKind::Var | NodeKind::Message if node.depth0 => "variable",
        NodeKind::Var | NodeKind::Message => "field",
    }
}

/// The primary modifier: ⌘ on macOS, Ctrl elsewhere.
fn primary(modifiers: &Modifiers) -> bool {
    if cfg!(target_os = "macos") { modifiers.platform } else { modifiers.control }
}

/// UI text in the active language.
fn tx(cx: &App, text: &str) -> String {
    cx.try_global::<Localization>().map_or_else(|| text.to_owned(), |localization| localization.0.tx(text))
}

fn mono_family(cx: &App) -> SharedString {
    let family = cx
        .try_global::<theme::ActiveTheme>()
        .and_then(|active| active.theme.font_families("font-mono").into_iter().next())
        .unwrap_or_else(|| theme::fonts::BUNDLED_MONO_FAMILY.to_owned());
    SharedString::from(family)
}

/// The colours and metrics of the rows, read once per frame.
#[derive(Clone, Copy)]
struct RowStyle {
    fg: Hsla,
    subtle: Hsla,
    faint: Hsla,
    name: Hsla,
    changed: Hsla,
    danger: Hsla,
    number: Hsla,
    string: Hsla,
    keyword: Hsla,
    icon: Hsla,
    hover: Hsla,
    selected: Hsla,
    accent: Hsla,
    height: Pixels,
    radius: Pixels,
}

impl RowStyle {
    fn value(&self, kind: ValueKind) -> Hsla {
        match kind {
            ValueKind::Number => self.number,
            ValueKind::String => self.string,
            ValueKind::Keyword => self.keyword,
            ValueKind::Object | ValueKind::Plain => self.fg,
        }
    }
}

/// What every row of one frame is drawn with.
struct RowContext {
    style: RowStyle,
    selected: Option<String>,
    /// The edited row's key, why its value was refused, and whether it is a Set Value (the name
    /// stays in front of the field).
    editing: Option<(String, Option<String>, bool)>,
    field: Entity<TextField>,
    changed: HashSet<String>,
}

fn render_row(row: &Row, index: usize, context: &RowContext, cx: &mut Context<VariablesView>) -> AnyElement {
    let style = context.style;
    let node = &row.node;
    let selector = format!("debug-variable-{index}");
    let is_selected = context.selected.as_deref() == Some(node.key.as_str());
    let chevron = if node.reference > 0 {
        let toggled = node.key.clone();
        div()
            .id(SharedString::from(format!("debug-variable-toggle-{index}")))
            .size(px(16.0))
            .flex_none()
            .child(theme::icon(cx, if row.expanded { "chevron-down" } else { "chevron-right" }, 16.0, style.icon))
            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                cx.stop_propagation();
                view.toggle(&toggled, cx);
            }))
            .into_any_element()
    } else {
        div().size(px(16.0)).flex_none().into_any_element()
    };
    let (clicked, menu) = (node.key.clone(), node.key.clone());
    let expandable = node.reference > 0;
    let base = div()
        .id(SharedString::from(selector.clone()))
        .debug_selector(move || selector.clone())
        .h(style.height)
        .w_full()
        .flex()
        .items_center()
        .gap(px(4.0))
        .pl(px(4.0 + 16.0 * row.depth as f32))
        .pr(px(8.0))
        .whitespace_nowrap()
        .overflow_hidden()
        .when(is_selected, |el| el.bg(style.selected))
        .when(!is_selected, |el| el.hover(|el| el.bg(style.hover)))
        .child(chevron)
        .on_click(cx.listener(move |view, event: &ClickEvent, window, cx| {
            view.clicked_row(&clicked, expandable && event.click_count() >= 2, window, cx);
        }))
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |view, event: &MouseDownEvent, window, cx| {
                view.opened_menu(&menu, event.position, window, cx);
            }),
        );
    let icon = theme::icon(cx, node_icon(node), 16.0, style.icon);
    if let Some((_, error, set_value)) = context.editing.as_ref().filter(|(key, _, _)| *key == node.key) {
        let frame = div()
            .flex_1()
            .min_w_0()
            .px(px(4.0))
            .rounded(style.radius)
            .border_1()
            .border_color(if error.is_some() { style.danger } else { style.accent })
            .child(context.field.clone());
        return base
            .child(icon)
            .when(*set_value, |el| {
                el.child(div().flex_none().text_color(style.fg).child(SharedString::from(node.name.clone())))
                    .child(div().flex_none().text_color(style.subtle).child("="))
            })
            .child(frame)
            .children(
                error.clone().map(|error| {
                    div().flex_none().text_color(style.danger).child(SharedString::from(one_line(&error)))
                }),
            )
            .into_any_element();
    }
    match node.kind {
        NodeKind::Message => {
            base.child(div().text_color(style.faint).child(SharedString::from(node.name.clone()))).into_any_element()
        }
        NodeKind::Scope => base
            .child(icon)
            .child(div().text_color(style.fg).child(SharedString::from(node.name.clone())))
            .into_any_element(),
        NodeKind::Eval | NodeKind::Watch | NodeKind::Var => {
            let name_color = if context.changed.contains(&node.key) {
                style.changed
            } else if node.kind == NodeKind::Var {
                style.name
            } else {
                style.fg
            };
            let value = match &node.error {
                Some(error) => div().text_color(style.danger).child(SharedString::from(one_line(error))),
                None => {
                    let kind = classify_value(&node.value, node.type_name.as_deref());
                    div()
                        .flex()
                        .gap(px(4.0))
                        .min_w_0()
                        .overflow_hidden()
                        .children(node.type_name.as_ref().map(|type_name| {
                            let shown = SharedString::from(format!("{{{type_name}}}"));
                            div().flex_none().text_color(style.subtle).child(shown)
                        }))
                        .child(div().text_color(style.value(kind)).child(SharedString::from(one_line(&node.value))))
                }
            };
            base.child(icon)
                .child(div().flex_none().text_color(name_color).child(SharedString::from(node.name.clone())))
                .child(div().flex_none().text_color(style.subtle).child("="))
                .child(value)
                .into_any_element()
        }
    }
}

impl Focusable for VariablesView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for VariablesView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = self.rows(cx);
        let changed: HashSet<String> =
            rows.iter().filter(|row| self.is_changed(&row.node)).map(|row| row.node.key.clone()).collect();
        let rows = Rc::new(rows);
        let note = self.note(cx);
        let focused = self.focus.is_focused(window);
        let style = RowStyle {
            fg: theme::color(cx, "fg"),
            subtle: theme::color(cx, "fg-subtle"),
            faint: theme::color(cx, "fg-faint"),
            name: theme::color(cx, "syn-property"),
            changed: theme::color(cx, "link"),
            danger: theme::color(cx, "danger"),
            number: theme::color(cx, "syn-number"),
            string: theme::color(cx, "syn-string"),
            keyword: theme::color(cx, "syn-keyword"),
            icon: theme::color(cx, "fg-icon"),
            hover: theme::color(cx, "bg-tree-hover"),
            selected: theme::color(cx, if focused { "bg-tree-selected" } else { "bg-tree-selected-inactive" }),
            accent: theme::color(cx, "accent"),
            height: theme::length(cx, "h-row"),
            radius: theme::length(cx, "radius-small"),
        };
        let editing = self
            .editing
            .as_ref()
            .map(|editing| (editing.key.clone(), editing.error.clone(), editing.kind == EditKind::SetValue));
        let field = self.edit_field.clone();
        let context = Rc::new(RowContext { style, selected: self.selected.clone(), editing, field, changed });
        let count = rows.len();
        let list = uniform_list(
            "debug-variables",
            count,
            cx.processor(move |_, range: std::ops::Range<usize>, _, cx| {
                let start = range.start;
                rows[range]
                    .iter()
                    .enumerate()
                    .map(|(offset, row)| render_row(row, start + offset, &context, cx))
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.scroll)
        .size_full();
        let mono = mono_family(cx);
        let add = div()
            .id("debug-add-watch")
            .debug_selector(|| "debug-add-watch".to_owned())
            .size(px(24.0))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(style.radius)
            .hover(|el| el.bg(theme::color(cx, "bg-hover")))
            .child(theme::icon(cx, "plus", 16.0, theme::color(cx, "fg-muted")))
            .on_click(cx.listener(|view, _: &ClickEvent, window, cx| view.add_watch_from_field(Some(window), cx)));
        div()
            .debug_selector(|| "debug-variables".to_owned())
            .size_full()
            .flex()
            .flex_col()
            .track_focus(&self.focus)
            .text_size(theme::length(cx, "fs-ui"))
            .child(
                div()
                    .flex_none()
                    .h(px(30.0))
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .px(px(8.0))
                    .border_b_1()
                    .border_color(theme::color(cx, "border"))
                    .child(
                        div()
                            .debug_selector(|| "debug-evaluate-field".to_owned())
                            .flex_1()
                            .min_w_0()
                            .font_family(mono.clone())
                            .child(self.eval_field.clone()),
                    )
                    .child(add),
            )
            .child(match note {
                Some(note) => div()
                    .debug_selector(|| "debug-variables-note".to_owned())
                    .flex_1()
                    .px(px(8.0))
                    .py(px(4.0))
                    .text_color(style.subtle)
                    .child(SharedString::from(note)),
                None => div().flex_1().min_h_0().font_family(mono).child(list),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::debugger::effects::Effect;
    use crate::debugger::state::DebugSession;
    use crate::debugger::state::testing::frame;
    use gpui_kit::{TestAppContext, VisualTestContext};
    use ide_dap::ThreadInfo;
    use std::cell::RefCell;

    type Events = Rc<RefCell<Vec<VariablesViewEvent>>>;

    fn variable(name: &str, value: &str, type_name: Option<&str>, reference: i64) -> Variable {
        Variable {
            name: name.to_owned(),
            value: value.to_owned(),
            type_name: type_name.map(str::to_owned),
            variables_reference: reference,
            evaluate_name: Some(name.to_owned()),
        }
    }

    fn locals_scope() -> Scope {
        Scope { name: "Locals".to_owned(), variables_reference: 1, expensive: false }
    }

    fn suspended(session: &mut DebugSession) {
        session.name = "main".to_owned();
        session.status = SessionStatus::Paused;
        session.stopped_thread = Some(1);
        session.threads = vec![ThreadInfo { id: 1, name: "MainThread".into() }];
        session.selected_thread = Some(1);
        session.frames = vec![frame(7, "main", "/p/app/main.py", 11)];
        session.selected_frame = Some(7);
    }

    fn debugger(init: impl FnOnce(&mut DebugSession)) -> Debugger {
        let mut debugger = Debugger::default();
        debugger.state.adopt_session(DebugSessionId(1), 0.0, init);
        debugger.state.take_effects();
        debugger
    }

    fn open(
        cx: &mut TestAppContext,
        debugger: Debugger,
    ) -> (Entity<VariablesView>, Entity<Debugger>, Events, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        // The pane starts over an empty debugger and is handed the sessions once the events are
        // recorded: opening a test window runs the event loop, so requests made on the view's first
        // turn would otherwise go out before anyone listens.
        let (debugger, sessions) = (cx.update(|cx| cx.new(|_| Debugger::default())), debugger);
        let shared = debugger.clone();
        let (view, cx) = cx.add_window_view(|window, cx| VariablesView::new(shared, window, cx));
        let events: Events = Rc::default();
        let seen = Rc::clone(&events);
        cx.update(|_, cx| {
            cx.subscribe(&view, move |_, event: &VariablesViewEvent, _| seen.borrow_mut().push(event.clone())).detach();
        });
        debugger.update(cx, |debugger, cx| {
            *debugger = sessions;
            cx.notify();
        });
        cx.run_until_parked();
        (view, debugger, events, cx)
    }

    /// The id of the last request `pick` recognises.
    fn last(events: &Events, pick: impl Fn(&VariablesViewEvent) -> Option<u64>) -> u64 {
        events.borrow().iter().rev().find_map(pick).expect("the request was made")
    }

    fn scopes_request(event: &VariablesViewEvent) -> Option<u64> {
        if let VariablesViewEvent::LoadScopes { request, .. } = event { Some(*request) } else { None }
    }

    /// The last children request, for `reference`.
    fn children_request(reference: i64) -> impl Fn(&VariablesViewEvent) -> Option<u64> {
        move |event: &VariablesViewEvent| match event {
            VariablesViewEvent::LoadChildren { request, reference: of, .. } if *of == reference => Some(*request),
            _ => None,
        }
    }

    /// Answers the frame's scopes (Locals, reference 1) and its locals.
    fn answer_frame(view: &Entity<VariablesView>, events: &Events, locals: Vec<Variable>, cx: &mut VisualTestContext) {
        let request = last(events, scopes_request);
        view.update(cx, |view, cx| view.scopes_loaded(request, Ok(vec![locals_scope()]), cx));
        let request = last(events, children_request(1));
        view.update(cx, |view, cx| view.children_loaded(request, Ok(locals), cx));
        cx.run_until_parked();
    }

    fn lines(view: &Entity<VariablesView>, cx: &mut VisualTestContext) -> Vec<String> {
        view.read_with(cx, |view, cx| view.variable_lines(cx))
    }

    fn focus_tree(view: &Entity<VariablesView>, cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            let focus = view.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
    }

    /// Moves the debugger on to its next stop, as a value set through it does.
    fn next_stop(debugger: &Entity<Debugger>, cx: &mut VisualTestContext) {
        debugger.update(cx, |debugger, cx| {
            let state = &mut debugger.state;
            state.set_variable_value(DebugSessionId(1), 1, "count", None, "4").expect("the session is suspended");
            let ticket = state.take_effects().into_iter().find_map(|effect| match effect {
                Effect::Ask { ticket, .. } => Some(ticket),
                _ => None,
            });
            let ticket = ticket.expect("Set Value asks the adapter");
            assert!(state.variable_set(ticket, Ok(variable("count", "4", None, 0))).is_ok());
            state.take_effects();
            cx.notify();
        });
        cx.run_until_parked();
    }

    // Not from a spec: the locals of the first scope are roots, the other scopes follow; a node's
    // children are asked for when it opens and stay open, loading again, at the next stop.
    #[gpui_kit::test]
    async fn children_load_when_a_node_opens_and_again_after_a_stop(cx: &mut TestAppContext) {
        let (view, debugger, events, cx) = open(cx, debugger(suspended));
        let request = last(&events, scopes_request);
        let globals = Scope { name: "Globals".to_owned(), variables_reference: 2, expensive: true };
        view.update(cx, |view, cx| view.scopes_loaded(request, Ok(vec![locals_scope(), globals]), cx));
        let request = last(&events, children_request(1));
        let order = variable("order", "Order@41", Some("Order"), 5);
        view.update(cx, |view, cx| view.children_loaded(request, Ok(vec![order, variable("n", "3", None, 0)]), cx));
        assert_eq!(lines(&view, cx), ["order = {Order} Order@41", "n = 3", "Globals"]);
        focus_tree(&view, cx);
        cx.simulate_keystrokes("down right");
        let request = last(&events, children_request(5));
        let members = vec![variable("id", "7", Some("int"), 0), variable("items", "[]", None, 0)];
        view.update(cx, |view, cx| view.children_loaded(request, Ok(members), cx));
        let opened = ["order = {Order} Order@41", "  id = {int} 7", "  items = []", "n = 3", "Globals"];
        assert_eq!(lines(&view, cx), opened);
        // Closing and opening again asks nothing more.
        cx.simulate_keystrokes("left right");
        assert_eq!(events.borrow().iter().filter_map(children_request(5)).count(), 1);
        // A node with nothing in it says so.
        cx.simulate_keystrokes("end enter");
        let request = last(&events, children_request(2));
        view.update(cx, |view, cx| view.children_loaded(request, Ok(Vec::new()), cx));
        assert_eq!(lines(&view, cx).last().map(String::as_str), Some("  No variables"));

        next_stop(&debugger, cx);
        assert_eq!(lines(&view, cx), Vec::<String>::new(), "nothing of the earlier stop is shown");
        answer_frame(&view, &events, vec![variable("order", "Order@41", Some("Order"), 9)], cx);
        assert_eq!(lines(&view, cx), ["order = {Order} Order@41"]);
        let request = last(&events, children_request(9));
        view.update(cx, |view, cx| view.children_loaded(request, Ok(vec![variable("id", "8", Some("int"), 0)]), cx));
        assert_eq!(lines(&view, cx), ["order = {Order} Order@41", "  id = {int} 8"]);
    }

    // Not from a spec: a value that differs from the previous stop has its name highlighted; one
    // that stayed, or is new, has not.
    #[gpui_kit::test]
    async fn values_changed_since_the_previous_stop_are_highlighted(cx: &mut TestAppContext) {
        let (view, debugger, events, cx) = open(cx, debugger(suspended));
        let locals = vec![variable("count", "3", Some("int"), 0), variable("label", "'hi'", None, 0)];
        answer_frame(&view, &events, locals, cx);
        assert!(view.read_with(cx, |view, cx| view.changed_names(cx)).is_empty());
        next_stop(&debugger, cx);
        let locals = vec![
            variable("count", "4", Some("int"), 0),
            variable("label", "'hi'", None, 0),
            variable("x", "1", None, 0),
        ];
        answer_frame(&view, &events, locals, cx);
        assert_eq!(view.read_with(cx, |view, cx| view.changed_names(cx)), ["count"]);
    }

    // Not from a spec: ⇧⌘↩ in the evaluate field adds a watch, which is evaluated for the frame and
    // listed before the locals; Delete removes it; Enter evaluates the field's expression.
    #[gpui_kit::test]
    async fn watches_are_added_evaluated_and_removed(cx: &mut TestAppContext) {
        let (view, debugger, events, cx) = open(cx, debugger(suspended));
        answer_frame(&view, &events, vec![variable("count", "3", Some("int"), 0)], cx);
        let field = view.read_with(cx, |view, _| view.eval_field().clone());
        cx.update(|window, cx| window.focus(&field.focus_handle(cx), cx));
        cx.simulate_input("count * 2");
        cx.simulate_keystrokes("cmd-shift-enter");
        assert_eq!(events.borrow().last(), Some(&VariablesViewEvent::AddWatch("count * 2".into())));
        assert_eq!(field.read_with(cx, |field, _| field.text().to_owned()), "");
        debugger.update(cx, |debugger, cx| {
            debugger.state.add_watch("count * 2");
            debugger.state.take_effects();
            cx.notify();
        });
        cx.run_until_parked();
        assert_eq!(lines(&view, cx), ["count * 2 =", "count = {int} 3"]);
        let request = last(&events, |event| match event {
            VariablesViewEvent::Evaluate { request, expression, context: EvaluateContext::Watch, .. }
                if expression == "count * 2" =>
            {
                Some(*request)
            }
            _ => None,
        });
        view.update(cx, |view, cx| view.evaluated(request, Ok(variable("count * 2", "6", Some("int"), 0)), cx));
        assert_eq!(lines(&view, cx), ["count * 2 = {int} 6", "count = {int} 3"]);

        cx.simulate_input("count + 1");
        cx.simulate_keystrokes("enter");
        let request = last(&events, |event| match event {
            VariablesViewEvent::Evaluate { request, context: EvaluateContext::Repl, .. } => Some(*request),
            _ => None,
        });
        view.update(cx, |view, cx| view.evaluated(request, Err("name 'cnt' is not defined".into()), cx));
        assert_eq!(lines(&view, cx)[0], "count + 1 = name 'cnt' is not defined");

        focus_tree(&view, cx);
        cx.simulate_keystrokes("down down delete");
        assert_eq!(events.borrow().last(), Some(&VariablesViewEvent::RemoveWatch("count * 2".into())));
    }

    // Not from a spec: F2 edits the selected value in place; the adapter's refusal shows in the
    // row, its success closes it; Escape gives up.
    #[gpui_kit::test]
    async fn set_value_edits_the_row_in_place(cx: &mut TestAppContext) {
        let (view, _, events, cx) = open(cx, debugger(suspended));
        answer_frame(&view, &events, vec![variable("count", "3", Some("int"), 0)], cx);
        focus_tree(&view, cx);
        cx.simulate_keystrokes("down f2");
        assert!(view.read_with(cx, |view, _| view.is_editing()));
        let field = view.read_with(cx, |view, _| view.edit_field().clone());
        assert_eq!(field.read_with(cx, |field, _| field.text().to_owned()), "3");
        cx.simulate_input("x");
        cx.simulate_keystrokes("enter");
        let request = last(&events, |event| match event {
            VariablesViewEvent::SetValue { request, container: 1, name, value, .. }
                if name == "count" && value == "x" =>
            {
                Some(*request)
            }
            _ => None,
        });
        cx.update(|window, cx| {
            view.update(cx, |view, cx| view.value_set(request, Err("bad value".into()), window, cx));
        });
        assert_eq!(view.read_with(cx, |view, _| view.edit_error().map(str::to_owned)).as_deref(), Some("bad value"));
        cx.simulate_input("4");
        assert_eq!(view.read_with(cx, |view, _| view.edit_error().map(str::to_owned)), None);
        cx.simulate_keystrokes("enter");
        let request = last(&events, |event| match event {
            VariablesViewEvent::SetValue { request, value, .. } if value == "x4" => Some(*request),
            _ => None,
        });
        cx.update(|window, cx| {
            view.update(cx, |view, cx| view.value_set(request, Ok(variable("count", "4", None, 0)), window, cx));
        });
        assert!(!view.read_with(cx, |view, _| view.is_editing()));
        cx.simulate_keystrokes("f2");
        assert!(view.read_with(cx, |view, _| view.is_editing()));
        cx.simulate_keystrokes("escape");
        assert!(!view.read_with(cx, |view, _| view.is_editing()));
    }

    // Not from a spec: the context menu of a value and of a watch; Copy Name and Copy Value of a
    // scalar go to the clipboard, a structured value is walked by the workbench; a watch is edited
    // in place.
    #[gpui_kit::test]
    async fn the_context_menu_offers_what_the_row_allows(cx: &mut TestAppContext) {
        let mut world = debugger(suspended);
        world.state.add_watch("total");
        world.state.take_effects();
        let (view, _, events, cx) = open(cx, world);
        answer_frame(&view, &events, vec![variable("count", "3", None, 0), variable("order", "Order@1", None, 5)], cx);
        let labels = |view: &Entity<VariablesView>, cx: &mut VisualTestContext| -> Vec<String> {
            view.read_with(cx, |view, cx| {
                view.variable_menu(cx).iter().map(|item| item.label().unwrap_or("-").to_owned()).collect()
            })
        };
        let open_menu = |name: &'static str, cx: &mut VisualTestContext| {
            cx.update(|window, cx| view.update(cx, |view, cx| view.open_menu_on(name, window, cx)));
        };
        open_menu("count", cx);
        let offered = ["Set Value…", "-", "Copy Value", "Copy Name", "Add to Watches", "-", "Jump to Source", "-"];
        assert_eq!(labels(&view, cx)[..8], offered);
        assert_eq!(labels(&view, cx).last().map(String::as_str), Some("New Watch…"));
        let run = |command: VariableCommand, cx: &mut VisualTestContext| {
            cx.update(|window, cx| view.update(cx, |view, cx| view.run_variable_command(command, window, cx)));
        };
        run(VariableCommand::CopyValue, cx);
        assert_eq!(cx.read_from_clipboard().and_then(|item| item.text()).as_deref(), Some("3"));
        run(VariableCommand::JumpToSource, cx);
        assert_eq!(events.borrow().last(), Some(&VariablesViewEvent::JumpToSource { local: Some("count".into()) }));
        open_menu("order", cx);
        run(VariableCommand::CopyName, cx);
        assert_eq!(cx.read_from_clipboard().and_then(|item| item.text()).as_deref(), Some("order"));
        run(VariableCommand::CopyValue, cx);
        let copy = VariablesViewEvent::CopyValue { session: DebugSessionId(1), value: "Order@1".into(), reference: 5 };
        assert_eq!(events.borrow().last(), Some(&copy));
        open_menu("total", cx);
        assert_eq!(labels(&view, cx)[..4], ["Set Value…", "-", "Edit Watch…", "Remove Watch"]);
        run(VariableCommand::EditWatch, cx);
        cx.simulate_input("grand total");
        cx.simulate_keystrokes("enter");
        let edit = VariablesViewEvent::EditWatch { old: "total".into(), new: "grand total".into() };
        assert_eq!(events.borrow().last(), Some(&edit));
        run(VariableCommand::NewWatch, cx);
        cx.simulate_input("a.b");
        cx.simulate_keystrokes("enter");
        assert_eq!(events.borrow().last(), Some(&VariablesViewEvent::AddWatch("a.b".into())));
    }

    // Not from a spec: without a suspended frame the pane says when variables are there; a watch
    // says it is not suspended.
    #[gpui_kit::test]
    async fn without_a_suspended_frame_the_pane_says_so(cx: &mut TestAppContext) {
        let running = |session: &mut DebugSession| session.status = SessionStatus::Running;
        let (view, debugger, events, cx) = open(cx, debugger(running));
        assert!(events.borrow().is_empty(), "nothing is asked while running");
        let note = view.read_with(cx, |view, cx| view.note(cx));
        assert_eq!(note.as_deref(), Some("Variables are available while suspended."));
        assert!(cx.debug_bounds("debug-variables-note").is_some());
        debugger.update(cx, |debugger, cx| {
            debugger.state.add_watch("total");
            cx.notify();
        });
        cx.run_until_parked();
        assert_eq!(lines(&view, cx), ["total = Not suspended"]);
        debugger.update(cx, |debugger, cx| {
            debugger.state.remove_watch("total");
            if let Some(session) = debugger.state.session_mut(DebugSessionId(1)) {
                session.status = SessionStatus::Terminated;
            }
            cx.notify();
        });
        cx.run_until_parked();
        assert_eq!(view.read_with(cx, |view, cx| view.note(cx)).as_deref(), Some("The session has finished."));
    }
}
