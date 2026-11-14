//! The Settings dialog: the search field and the page tree on the left, the selected page on the
//! right under its breadcrumb (with Reset, the Global / This project switch and Back / Forward),
//! and OK / Cancel / Apply below, in the platform's order.
//!
//! The state is the [`SettingsDialog`] model, held by entity so that the views given for bespoke
//! pages stage into the same edits. A page declared by a [`PageSpec`] with sections is drawn from
//! it with [`super::controls`]; any other page shows the view the caller gave for its id, or a
//! line saying it is not available. Nothing is written here: Apply and OK hand the writes to the
//! caller ([`SettingsDialogEvent::Apply`]), which reports back how each went through the
//! `apply_*` methods, and the dialog closes after OK once everything was saved.
//!
//! The keyboard: typing searches the pages (and marks the matching options); Up / Down in the
//! search move through the tree; Escape clears the search first, then cancels; Enter is OK.
//! In the tree, Left / Right close and open groups, Home / End go to the ends.

use super::controls::{self, ControlHost, PageControls, UNAVAILABLE, tx};
use crate::platform::i18n::{I18n, Localization, Param};
use crate::platform::input;
use crate::platform::keymap::is_mac_host;
use crate::settings_model::page_spec::{PageSpec, spec_labels};
use crate::settings_model::pages::{
    PageNode, SettingsPage, breadcrumb, build_page_tree, filter_page_tree, flatten, query_terms,
};
use crate::settings_model::staging::{
    ApplyRun, ApplyStart, SSH_SETTINGS_PAGE, Scope, SecretWrite, SettingsDialog, StagedChange, settings_title,
};
use crate::theme;
use crate::ui::text_field::{TextField, TextFieldEvent};
use gpui_kit::{
    AnyElement, AnyView, App, ClickEvent, Context, Div, Entity, EventEmitter, FocusHandle, Focusable, FontWeight,
    IntoElement, Keystroke, MouseButton, Render, SharedString, Subscription, Window, div, prelude::*, px,
};
use serde_json::Value;
use std::collections::{BTreeSet, HashMap};

const DIALOG_WIDTH: f32 = 980.0;
const DIALOG_HEIGHT: f32 = 720.0;
const SIDEBAR_WIDTH: f32 = 240.0;
/// How far each level of the tree is indented.
const INDENT: f32 = 16.0;

/// What a group folder's page says about the group.
const GROUP_DESCRIPTIONS: [(&str, &str); 6] = [
    (
        "Appearance & Behavior",
        "Customize IDE appearance and behavior: change themes and font size, configure system settings and \
         notifications.",
    ),
    ("Editor", "Configure code editing: completion, folding, tabs, fonts, colors, code style and inspections."),
    ("Build, Execution, Deployment", "Configure build tools, run configurations, debugger and deployment."),
    ("Languages & Frameworks", "Configure language servers, interpreters, SDKs and framework support."),
    ("Tools", "Configure external tools, terminal, database and other integrations."),
    ("Version Control", "Configure the settings related to version control used in your project"),
];

const PROJECT_NOTE: &str = "Saved to .openidea/settings.toml in this project; overrides the global value.";
const GLOBAL_NOTE: &str = "Saved to the global settings: used by new projects and by projects without their own value.";

/// The pages the dialog lists.
#[derive(Debug, Clone, Default)]
pub struct SettingsPageSet {
    /// Pages declared by a spec: drawn from its sections when it has any, else by the view the
    /// caller gives for the page.
    pub specs: Vec<PageSpec>,
    /// Pages with a view of their own (Keymap, Plugins, a plugin's page).
    pub pages: Vec<SettingsPage>,
}

/// What the dialog asks of its caller.
#[derive(Debug, Clone, PartialEq)]
pub enum SettingsDialogEvent {
    /// Apply or OK: write `changes` in order, stopping at the first failure, then report with
    /// [`SettingsDialogView::apply_writes_done`], store each [`SettingsDialogView::apply_next_secret`],
    /// write each [`SettingsDialogView::apply_next_external`] and call
    /// [`SettingsDialogView::finish_apply`].
    Apply { changes: Vec<StagedChange> },
    /// Cancel, Escape, or OK once everything was saved: the dialog is closed and its edits gone.
    Closed,
}

/// A row of the page tree as it is drawn.
#[derive(Debug, Clone, PartialEq)]
struct TreeRow {
    key: String,
    title: String,
    depth: usize,
    has_children: bool,
    expanded: bool,
}

pub struct SettingsDialogView {
    model: Entity<SettingsDialog>,
    specs: Vec<PageSpec>,
    /// Every page, those of the specs first.
    pages: Vec<SettingsPage>,
    tree: Vec<PageNode>,
    /// Search texts of a page besides its own (the action names of the Keymap).
    extra_labels: HashMap<String, Vec<String>>,
    page_views: HashMap<String, AnyView>,
    /// Views of a spec's controls of their own, by page id and label.
    custom_views: HashMap<(String, String), AnyView>,
    has_project: bool,
    /// The groups the user opened, while there is no search.
    expanded: BTreeSet<String>,
    search: Entity<TextField>,
    tree_focus: FocusHandle,
    controls: PageControls,
    /// The page and scope the controls were built for.
    built_for: Option<(String, Scope)>,
    /// The pages visited while the dialog is open, and where Back / Forward stand among them.
    history: Vec<String>,
    history_at: usize,
    apply: Option<ApplyRun>,
    /// The Apply under way came from OK: the dialog closes once it saved everything.
    close_after_apply: bool,
    claims: Vec<u64>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<SettingsDialogEvent> for SettingsDialogView {}

/// The nodes above `key`, root first.
fn ancestor_keys(nodes: &[PageNode], key: &str) -> Vec<String> {
    for node in nodes {
        if node.key == key {
            return Vec::new();
        }
        let below = ancestor_keys(&node.children, key);
        if !below.is_empty() || node.children.iter().any(|child| child.key == key) {
            return std::iter::once(node.key.clone()).chain(below).collect();
        }
    }
    Vec::new()
}

/// The page a search lands on: the first that matches, else the first page.
fn first_page(nodes: &[PageNode]) -> Option<String> {
    let listed: Vec<&PageNode> = flatten(nodes).into_iter().filter(|node| node.page.is_some()).collect();
    listed.iter().find(|node| node.matched).or_else(|| listed.first()).map(|node| node.key.clone())
}

fn find_node<'n>(nodes: &'n [PageNode], key: &str) -> Option<&'n PageNode> {
    flatten(nodes).into_iter().find(|node| node.key == key)
}

impl SettingsDialogView {
    /// The dialog over `model`, which the caller has opened ([`SettingsDialog::open`] or the
    /// Settings window's start). `has_project` offers the This project scope on pages that take it.
    pub fn new(model: Entity<SettingsDialog>, set: SettingsPageSet, has_project: bool, cx: &mut Context<Self>) -> Self {
        let query = model.read(cx).query.clone();
        let search = cx.new(|cx| {
            let mut field = TextField::new(cx);
            field.set_text(&query, cx);
            field
        });
        let tree_focus = cx.focus_handle();
        let observe = cx.observe(&model, |view: &mut Self, _, cx| view.refresh(false, cx));
        let search_events = cx.subscribe(&search, |view: &mut Self, _, event: &TextFieldEvent, cx| match event {
            TextFieldEvent::Changed => view.query_changed(cx),
            TextFieldEvent::Submitted => view.ok(cx),
            TextFieldEvent::Cancelled => view.escape_search(cx),
        });
        let claimant = cx.weak_entity();
        let search_claim = input::claim_keys(
            search.focus_handle(cx),
            move |keystroke, _, cx| claimant.update(cx, |view, cx| view.search_key(keystroke, cx)).unwrap_or(false),
            cx,
        );
        let claimant = cx.weak_entity();
        let tree_claim = input::claim_keys(
            tree_focus.clone(),
            move |keystroke, _, cx| claimant.update(cx, |view, cx| view.tree_key(keystroke, cx)).unwrap_or(false),
            cx,
        );
        cx.on_release(|view: &mut Self, cx: &mut App| {
            for claim in view.claims.drain(..) {
                input::release_keys(claim, cx);
            }
            view.controls.release(cx);
        })
        .detach();
        let mut view = Self {
            model,
            specs: Vec::new(),
            pages: Vec::new(),
            tree: Vec::new(),
            extra_labels: HashMap::new(),
            page_views: HashMap::new(),
            custom_views: HashMap::new(),
            has_project,
            expanded: BTreeSet::new(),
            search,
            tree_focus,
            controls: PageControls::empty(),
            built_for: None,
            history: Vec::new(),
            history_at: 0,
            apply: None,
            close_after_apply: false,
            claims: vec![search_claim, tree_claim],
            _subscriptions: vec![observe, search_events],
        };
        view.set_pages(set, cx);
        view
    }

    pub fn model(&self) -> &Entity<SettingsDialog> {
        &self.model
    }

    /// Lists the pages of `set` in place of those listed so far.
    pub fn set_pages(&mut self, set: SettingsPageSet, cx: &mut Context<Self>) {
        let SettingsPageSet { specs, pages } = set;
        let mut all: Vec<SettingsPage> = specs.iter().map(SettingsPage::from).collect();
        all.extend(pages.into_iter().filter(|page| !specs.iter().any(|spec| spec.id == page.id)));
        self.tree = build_page_tree(&all);
        (self.specs, self.pages) = (specs, all);
        // The pages are new: the controls are built for them again.
        self.built_for = None;
        self.refresh(false, cx);
    }

    /// The view drawn for `page`, in place of its spec's sections or of the "not available" line.
    pub fn set_page_view(&mut self, page: &str, view: AnyView, cx: &mut Context<Self>) {
        self.page_views.insert(page.to_owned(), view);
        cx.notify();
    }

    /// The view drawn for the control of its own labelled `label` on `page`.
    pub fn set_custom_control(&mut self, page: &str, label: &str, view: AnyView, cx: &mut Context<Self>) {
        self.custom_views.insert((page.to_owned(), label.to_owned()), view);
        cx.notify();
    }

    /// The view drawn for the control of its own labelled `label` on `page`, when it has one.
    pub fn custom_control(&self, page: &str, label: &str) -> Option<&AnyView> {
        self.custom_views.get(&(page.to_owned(), label.to_owned()))
    }

    /// More texts the search finds `page` by (the action names of the Keymap).
    pub fn set_page_labels(&mut self, page: &str, labels: Vec<String>, cx: &mut Context<Self>) {
        self.extra_labels.insert(page.to_owned(), labels);
        cx.notify();
    }

    /// Whether a project is open: pages that take it then offer the This project scope.
    pub fn set_has_project(&mut self, has_project: bool, cx: &mut Context<Self>) {
        self.has_project = has_project;
        self.refresh(false, cx);
    }

    fn spec(&self, page: &str) -> Option<&PageSpec> {
        self.specs.iter().find(|spec| spec.id == page)
    }

    /// The search texts of `page`: its spec's labels and those given for it.
    fn labels_of(&self, page: &str) -> Vec<String> {
        let own = self.spec(page).map(spec_labels).unwrap_or_default();
        let own = own.into_iter().map(str::to_owned);
        own.chain(self.extra_labels.get(page).into_iter().flatten().cloned()).collect()
    }

    fn filtered(&self, query: &str, i18n: &I18n) -> Vec<PageNode> {
        filter_page_tree(&self.tree, query, &|page: &str| self.labels_of(page), i18n)
    }

    /// The page shown: the model's, else the first page.
    fn current_key(&self, cx: &App) -> Option<String> {
        let page_id = &self.model.read(cx).page_id;
        let all = flatten(&self.tree);
        let found = all.iter().find(|node| node.key == *page_id);
        found.or_else(|| all.iter().find(|node| node.page.is_some())).map(|node| node.key.clone())
    }

    /// Whether `page` offers the This project scope now.
    fn offers_project_scope(&self, page: &str, dialog: &SettingsDialog) -> bool {
        let declared = match self.spec(page) {
            Some(spec) => spec.project_scope,
            None => self.pages.iter().any(|listed| listed.id == page && listed.project_scope),
        };
        declared && self.has_project && !dialog.new_projects
    }

    /// The key the scope of `page` is chosen under: its scope group's, else its own.
    fn scope_key<'p>(&'p self, page: &'p str) -> &'p str {
        self.spec(page).and_then(|spec| spec.scope_group).unwrap_or(page)
    }

    fn page_scope(&self, page: &str, cx: &App) -> Scope {
        let dialog = self.model.read(cx);
        let default = self.spec(page).and_then(|spec| spec.default_scope);
        let chosen = dialog.scopes.get(self.scope_key(page)).copied().or(default);
        if self.offers_project_scope(page, dialog) && chosen == Some(Scope::Project) {
            Scope::Project
        } else {
            Scope::Global
        }
    }

    /// Follows the model: a new current page joins the history and has its groups opened, and
    /// the controls are built for a new page or scope, else shown the values now staged.
    fn refresh(&mut self, force: bool, cx: &mut Context<Self>) {
        let Some(current) = self.current_key(cx) else {
            self.controls.release(cx);
            self.controls = PageControls::empty();
            self.built_for = None;
            cx.notify();
            return;
        };
        if self.history.get(self.history_at) != Some(&current) {
            self.history.truncate(self.history_at + 1);
            self.history.push(current.clone());
            self.history_at = self.history.len() - 1;
            self.expanded.extend(ancestor_keys(&self.tree, &current));
        }
        let scope = self.page_scope(&current, cx);
        let wanted = (current, scope);
        if self.built_for.as_ref() == Some(&wanted) {
            self.controls.sync(&self.model, force, cx);
        } else {
            self.controls.release(cx);
            let spec = self.spec(&wanted.0).cloned();
            self.controls = PageControls::build(&self.model, spec.as_ref(), &wanted.0, scope, cx);
            self.built_for = Some(wanted);
        }
        cx.notify();
    }

    fn select(&mut self, key: &str, cx: &mut Context<Self>) {
        self.model.update(cx, |dialog, cx| {
            dialog.select_page(key);
            cx.notify();
        });
    }

    /// The search's text changed: the tree narrows to it, and when the page shown no longer
    /// matches, the first page that does is shown.
    fn query_changed(&mut self, cx: &mut Context<Self>) {
        let query = self.search.read(cx).text().to_owned();
        let current = self.current_key(cx);
        let landing = if query_terms(&query).is_empty() {
            None
        } else {
            let filtered = with_i18n(cx, |i18n| self.filtered(&query, i18n));
            let still = flatten(&filtered).iter().any(|node| Some(&node.key) == current.as_ref() && node.matched);
            if still { None } else { first_page(&filtered) }
        };
        self.model.update(cx, |dialog, cx| {
            dialog.query = query;
            if let Some(page) = landing {
                dialog.select_page(&page);
            }
            cx.notify();
        });
    }

    /// Escape in the search: clears it, else cancels the dialog.
    fn escape_search(&mut self, cx: &mut Context<Self>) {
        if self.controls.close_select() {
            cx.notify();
        } else if self.search.read(cx).text().is_empty() {
            self.cancel(cx);
        } else {
            self.search.update(cx, |field, cx| field.set_text("", cx));
            self.query_changed(cx);
        }
    }

    /// The rows of the tree as shown: every match with its groups open while searching, else
    /// the groups the user opened.
    fn rows(&self, cx: &App) -> Vec<TreeRow> {
        /// Lists `nodes` at `depth`, and below each open one its children; `open` is `None`
        /// when every group is open.
        fn walk(nodes: &[PageNode], depth: usize, open: Option<&BTreeSet<String>>, out: &mut Vec<TreeRow>) {
            for node in nodes {
                let has_children = !node.children.is_empty();
                let expanded = has_children && open.is_none_or(|open| open.contains(&node.key));
                let (key, title) = (node.key.clone(), node.title.clone());
                out.push(TreeRow { key, title, depth, has_children, expanded });
                if expanded {
                    walk(&node.children, depth + 1, open, out);
                }
            }
        }
        let query = &self.model.read(cx).query;
        let open = query_terms(query).is_empty().then_some(&self.expanded);
        let filtered = with_i18n(cx, |i18n| self.filtered(query, i18n));
        let mut rows = Vec::new();
        walk(&filtered, 0, open, &mut rows);
        rows
    }

    /// Moves the selection by `by` rows of the tree.
    fn step(&mut self, by: isize, cx: &mut Context<Self>) {
        let rows = self.rows(cx);
        let Some(last) = rows.len().checked_sub(1) else { return };
        let current = self.current_key(cx);
        let next = match rows.iter().position(|row| Some(&row.key) == current.as_ref()) {
            Some(at) => at.saturating_add_signed(by).min(last),
            None => 0,
        };
        let key = rows[next].key.clone();
        self.select(&key, cx);
    }

    fn toggle(&mut self, key: &str, cx: &mut Context<Self>) {
        if !self.expanded.remove(key) {
            self.expanded.insert(key.to_owned());
        }
        cx.notify();
    }

    /// The keys the search field handles before the keymap.
    fn search_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        if keystroke.modifiers.modified() {
            return false;
        }
        match keystroke.key.as_str() {
            "up" => self.step(-1, cx),
            "down" => self.step(1, cx),
            "enter" => self.ok(cx),
            "escape" => self.escape_search(cx),
            _ => return false,
        }
        true
    }

    /// The keys the tree handles before the keymap: Up / Down / Home / End move, Right opens a
    /// closed group and then steps into it, Left closes an open group and else goes to the
    /// parent, Enter opens or closes a group, Escape cancels.
    fn tree_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        if keystroke.modifiers.modified() {
            return false;
        }
        let rows = self.rows(cx);
        let current = self.current_key(cx);
        let at = rows.iter().position(|row| Some(&row.key) == current.as_ref());
        let row = at.and_then(|at| rows.get(at)).cloned();
        match (keystroke.key.as_str(), row) {
            ("up", _) => self.step(-1, cx),
            ("down", _) => self.step(1, cx),
            ("home", _) => self.step(isize::MIN, cx),
            ("end", _) => self.step(isize::MAX, cx),
            ("right", Some(row)) if row.has_children && !row.expanded => self.toggle(&row.key, cx),
            ("right", Some(row)) if row.has_children => self.step(1, cx),
            ("left", Some(row)) if row.expanded => self.toggle(&row.key, cx),
            ("left", Some(row)) => {
                let index = at.unwrap_or_default();
                let parent = rows[..index].iter().rev().find(|candidate| candidate.depth < row.depth);
                if let Some(parent) = parent.map(|parent| parent.key.clone()) {
                    self.select(&parent, cx);
                }
            }
            ("enter", Some(row)) if row.has_children => self.toggle(&row.key, cx),
            ("enter", _) => self.ok(cx),
            ("escape", _) => self.cancel(cx),
            _ => return false,
        }
        true
    }

    // -----------------------------------------------------------------------
    // Apply, OK, Cancel

    /// Whether Apply has anything to write.
    pub fn can_apply(&self, cx: &App) -> bool {
        let mut probe = self.model.read(cx).clone();
        matches!(probe.begin_apply(), ApplyStart::Run(_))
    }

    /// Whether OK can be pressed: not while an Apply runs.
    pub fn can_ok(&self, cx: &App) -> bool {
        !self.model.read(cx).saving
    }

    pub fn is_applying(&self) -> bool {
        self.apply.is_some()
    }

    /// Apply: hands the writes to the caller.
    pub fn apply(&mut self, cx: &mut Context<Self>) {
        self.start_apply(false, cx);
    }

    /// OK: applies, and closes once everything was saved (at once when nothing is staged).
    pub fn ok(&mut self, cx: &mut Context<Self>) {
        self.start_apply(true, cx);
    }

    fn start_apply(&mut self, close: bool, cx: &mut Context<Self>) {
        let start = self.model.update(cx, |dialog, cx| {
            let start = dialog.begin_apply();
            cx.notify();
            start
        });
        match start {
            ApplyStart::Busy => {}
            ApplyStart::Clean if close => self.close(cx),
            ApplyStart::Clean => {}
            ApplyStart::Run(run) => {
                let changes = run.changes().to_vec();
                (self.apply, self.close_after_apply) = (Some(run), close);
                cx.emit(SettingsDialogEvent::Apply { changes });
            }
        }
    }

    /// The first `written` changes were written; `failure` is why the next one was not. `base`
    /// is the effective settings now; `has_project` whether a project is open (the caller then
    /// loads the global layer into [`SettingsDialog::set_global_base`]).
    pub fn apply_writes_done(
        &mut self,
        written: usize,
        failure: Option<&str>,
        base: Value,
        has_project: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(run) = self.apply.as_mut() else { return };
        self.model.update(cx, |dialog, cx| {
            run.writes_done(dialog, written, failure, base, has_project);
            cx.notify();
        });
    }

    /// The next SSH credential to store; `backend` is whether there is a keychain.
    pub fn apply_next_secret(&mut self, backend: bool, cx: &mut Context<Self>) -> Option<SecretWrite> {
        let run = self.apply.as_mut()?;
        self.model.update(cx, |dialog, _| run.next_secret(dialog, backend))
    }

    pub fn apply_secret_done(&mut self, result: Result<(), String>, cx: &mut Context<Self>) {
        let Some(run) = self.apply.as_mut() else { return };
        self.model.update(cx, |dialog, cx| {
            run.secret_done(dialog, result);
            cx.notify();
        });
    }

    /// The next value kept outside the settings to write: its id and the value.
    pub fn apply_next_external(&mut self, cx: &mut Context<Self>) -> Option<(String, Value)> {
        let run = self.apply.as_mut()?;
        run.next_external(self.model.read(cx))
    }

    pub fn apply_external_done(&mut self, result: Result<Value, String>, cx: &mut Context<Self>) {
        let Some(run) = self.apply.as_mut() else { return };
        self.model.update(cx, |dialog, cx| {
            run.external_done(dialog, result);
            cx.notify();
        });
    }

    /// Ends the Apply; whether everything was saved. After OK, a full save closes the dialog.
    pub fn finish_apply(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(run) = self.apply.take() else { return false };
        let saved = self.model.update(cx, |dialog, cx| {
            let saved = run.finish(dialog);
            cx.notify();
            saved
        });
        let close = std::mem::take(&mut self.close_after_apply);
        if saved && close {
            self.close(cx);
        }
        saved
    }

    /// Cancel: closes an open choice list, else the dialog, discarding what is staged.
    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        if self.controls.close_select() {
            cx.notify();
        } else {
            self.close(cx);
        }
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        (self.apply, self.close_after_apply) = (None, false);
        self.model.update(cx, |dialog, cx| {
            dialog.close();
            cx.notify();
        });
        cx.emit(SettingsDialogEvent::Closed);
    }

    /// Reset: discards the staged edits of the page shown.
    pub fn reset(&mut self, cx: &mut Context<Self>) {
        let Some(current) = self.current_key(cx) else { return };
        self.model.update(cx, |dialog, cx| {
            dialog.reset_page(&current);
            cx.notify();
        });
        self.controls.sync(&self.model, true, cx);
    }

    fn set_scope(&mut self, scope: Scope, cx: &mut Context<Self>) {
        let Some(current) = self.current_key(cx) else { return };
        let key = self.scope_key(&current).to_owned();
        self.model.update(cx, |dialog, cx| {
            dialog.set_page_scope(&key, scope);
            cx.notify();
        });
    }

    fn go(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(key) = self.history.get(index).cloned() else { return };
        self.history_at = index;
        self.select(&key, cx);
    }

    pub fn back(&mut self, cx: &mut Context<Self>) {
        if let Some(index) = self.history_at.checked_sub(1) {
            self.go(index, cx);
        }
    }

    pub fn forward(&mut self, cx: &mut Context<Self>) {
        if self.history_at + 1 < self.history.len() {
            self.go(self.history_at + 1, cx);
        }
    }

    // -----------------------------------------------------------------------
    // What the dialog shows

    /// The rows of the tree as text, indented two spaces per level.
    pub fn pages(&self, cx: &App) -> Vec<String> {
        self.rows(cx).iter().map(|row| format!("{}{}", "  ".repeat(row.depth), row.title)).collect()
    }

    /// The key of the page shown (a group's is `group:<title>`).
    pub fn selected_page(&self, cx: &App) -> Option<String> {
        self.current_key(cx)
    }

    pub fn breadcrumbs(&self, cx: &App) -> Vec<String> {
        self.current_key(cx).map(|key| breadcrumb(&self.tree, &key)).unwrap_or_default()
    }

    /// The pages with staged edits.
    pub fn modified_pages(&self, cx: &App) -> BTreeSet<String> {
        let dialog = self.model.read(cx);
        let mut pages: BTreeSet<String> = dialog.staged().map(|change| change.page.clone()).collect();
        if dialog.has_staged_secrets() {
            pages.insert(SSH_SETTINGS_PAGE.to_owned());
        }
        pages
    }

    /// Whether the page shown has staged edits.
    pub fn is_modified(&self, cx: &App) -> bool {
        self.current_key(cx).is_some_and(|key| self.modified_pages(cx).contains(&key))
    }

    /// Whether the Reset button shows: the page has edits and no Apply runs.
    pub fn reset_visible(&self, cx: &App) -> bool {
        self.is_modified(cx) && !self.model.read(cx).saving
    }

    /// The scope the page shown edits.
    pub fn scope(&self, cx: &App) -> Scope {
        self.current_key(cx).map_or(Scope::Global, |key| self.page_scope(&key, cx))
    }

    /// Whether the page shown offers the Global / This project switch.
    pub fn has_scope_switch(&self, cx: &App) -> bool {
        self.current_key(cx).is_some_and(|key| {
            let own = self.spec(&key).is_some_and(|spec| spec.own_scope_switch);
            self.offers_project_scope(&key, self.model.read(cx)) && !own
        })
    }

    /// The notes above the page: where its values are saved.
    pub fn notes(&self, cx: &App) -> Vec<&'static str> {
        let Some(key) = self.current_key(cx) else { return Vec::new() };
        let dialog = self.model.read(cx);
        let scope = self.page_scope(&key, cx);
        let project_first = self.spec(&key).and_then(|spec| spec.default_scope) == Some(Scope::Project);
        let global_note =
            dialog.new_projects || (self.offers_project_scope(&key, dialog) && scope == Scope::Global && project_first);
        let mut notes = Vec::new();
        if scope == Scope::Project {
            notes.push(PROJECT_NOTE);
        }
        if global_note {
            notes.push(GLOBAL_NOTE);
        }
        notes
    }

    /// Why the last Apply failed, as the footer says it.
    pub fn status(&self, cx: &mut App) -> Option<String> {
        let error = self.model.read(cx).error.clone()?;
        let localization = cx.default_global::<Localization>();
        Some(localization.0.tx_built(&["Could not save: ", ""], &[Param::Text(error)]))
    }

    /// The page shown as lines of text: its controls ([`controls::page_lines`]); a group's
    /// description and `→ Child` links; `<page view>` or the "not available" line for a page
    /// drawn by a view of its own.
    pub fn control_lines(&self, cx: &App) -> Vec<String> {
        let Some(key) = self.current_key(cx) else { return Vec::new() };
        let Some(node) = find_node(&self.tree, &key) else { return Vec::new() };
        if node.page.is_none() {
            let description = GROUP_DESCRIPTIONS.iter().find(|(group, _)| *group == node.title);
            let description = description.map(|(_, text)| tx(cx, text));
            let links = node.children.iter().map(|child| format!("→ {}", child.title));
            return description.into_iter().chain(links).collect();
        }
        if self.page_views.contains_key(&key) {
            return vec!["<page view>".to_owned()];
        }
        match self.spec(&key).filter(|spec| !spec.sections.is_empty()) {
            Some(spec) => controls::page_lines(self, spec, &|label: &str| self.custom_view(&key, label), cx),
            None => vec![tx(cx, UNAVAILABLE)],
        }
    }

    /// The labels of the page's options that the search matches.
    pub fn highlighted(&self, cx: &App) -> Vec<String> {
        let spec = self.current_key(cx).and_then(|key| self.spec(&key));
        spec.map(|spec| controls::matched_labels(self, spec, cx)).unwrap_or_default()
    }

    /// The text field of setting `key` on the page shown.
    pub fn field(&self, key: &str) -> Option<&Entity<TextField>> {
        self.controls.field(key)
    }

    pub fn search_field(&self) -> &Entity<TextField> {
        &self.search
    }

    pub fn tree_focus(&self) -> &FocusHandle {
        &self.tree_focus
    }

    fn custom_view(&self, page: &str, label: &str) -> Option<AnyView> {
        self.custom_views.get(&(page.to_owned(), label.to_owned())).cloned()
    }

    // -----------------------------------------------------------------------
    // Drawing

    fn render_tree(&self, cx: &mut Context<Self>) -> AnyElement {
        let rows = self.rows(cx);
        if rows.is_empty() {
            let text = tx(cx, "No matches");
            return div()
                .px(px(12.0))
                .py(px(8.0))
                .text_color(theme::color(cx, "fg-subtle"))
                .child(SharedString::from(text))
                .into_any_element();
        }
        let query = self.model.read(cx).query.clone();
        let current = self.current_key(cx);
        let modified = self.modified_pages(cx);
        let roots: BTreeSet<&str> = self.tree.iter().map(|node| node.key.as_str()).collect();
        let (fg, strong, accent, icon) = (
            theme::color(cx, "fg"),
            theme::color(cx, "fg-strong"),
            theme::color(cx, "accent"),
            theme::color(cx, "fg-icon"),
        );
        let selected_bg = theme::color(cx, "bg-tree-selected");
        let drawn: Vec<AnyElement> = rows
            .iter()
            .map(|row| {
                let title = tx(cx, &row.title);
                let color = if modified.contains(&row.key) {
                    accent
                } else if roots.contains(row.key.as_str()) {
                    strong
                } else {
                    fg
                };
                let chevron = if row.has_children {
                    let name = if row.expanded { "chevron-down" } else { "chevron-right" };
                    let toggled = row.key.clone();
                    div()
                        .id(SharedString::from(format!("settings-toggle-page-{}", row.key)))
                        .size(px(16.0))
                        .flex_none()
                        .child(theme::icon(cx, name, 16.0, icon))
                        .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                            view.toggle(&toggled, cx);
                            cx.stop_propagation();
                        }))
                        .into_any_element()
                } else {
                    div().size(px(16.0)).flex_none().into_any_element()
                };
                let (clicked, selector) = (row.key.clone(), format!("settings-page-{}", row.key));
                div()
                    .id(SharedString::from(selector.clone()))
                    .debug_selector(move || selector)
                    .h(theme::length(cx, "h-row"))
                    .w_full()
                    .flex()
                    .items_center()
                    .gap(px(2.0))
                    .pl(px(6.0 + INDENT * row.depth as f32))
                    .pr(px(8.0))
                    .text_color(color)
                    .when(roots.contains(row.key.as_str()), |el| el.font_weight(FontWeight::SEMIBOLD))
                    .when(current.as_ref() == Some(&row.key), |el| el.bg(selected_bg))
                    .child(chevron)
                    .child(controls::search_label(cx, &title, &query))
                    .on_click(cx.listener(move |view, event: &ClickEvent, window, cx| {
                        window.focus(&view.tree_focus, cx);
                        view.select(&clicked, cx);
                        if event.click_count() >= 2 {
                            view.toggle(&clicked, cx);
                        }
                    }))
                    .into_any_element()
            })
            .collect();
        div().flex().flex_col().children(drawn).into_any_element()
    }

    fn render_header(&self, key: &str, cx: &mut Context<Self>) -> Div {
        let muted = theme::color(cx, "fg-subtle");
        let crumbs = breadcrumb(&self.tree, key);
        let mut trail: Vec<AnyElement> = Vec::new();
        for (index, crumb) in crumbs.iter().enumerate() {
            if index > 0 {
                trail.push(theme::icon(cx, "chevron-right", 12.0, muted));
            }
            let text = tx(cx, crumb);
            trail.push(div().whitespace_nowrap().child(SharedString::from(text)).into_any_element());
        }
        let reset = self.reset_visible(cx).then(|| {
            let label = tx(cx, "Reset");
            div()
                .id("settings-reset")
                .debug_selector(|| "settings-reset".to_owned())
                .text_color(theme::color(cx, "link"))
                .hover(|el| el.text_color(theme::color(cx, "link-hover")))
                .child(SharedString::from(label))
                .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.reset(cx)))
        });
        let switch = self.has_scope_switch(cx).then(|| {
            let scope = self.scope(cx);
            let choice = |cx: &mut Context<Self>, value: Scope, label: &str, selector: &'static str| {
                let on = scope == value;
                let text = tx(cx, label);
                div()
                    .id(selector)
                    .debug_selector(move || selector.to_owned())
                    .h(theme::length(cx, "h-control"))
                    .px(px(10.0))
                    .flex()
                    .items_center()
                    .rounded(theme::length(cx, "radius-small"))
                    .text_color(theme::color(cx, if on { "fg-strong" } else { "fg-muted" }))
                    .when(on, |el| el.bg(theme::color(cx, "bg-selected")))
                    .hover(|el| el.bg(theme::color(cx, "bg-hover")))
                    .child(SharedString::from(text))
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.set_scope(value, cx)))
            };
            let global = choice(cx, Scope::Global, "Global", "settings-scope-global");
            let project = choice(cx, Scope::Project, "This project", "settings-scope-project");
            div()
                .flex()
                .items_center()
                .gap(px(2.0))
                .p(px(2.0))
                .rounded(theme::length(cx, "radius-small"))
                .border_1()
                .border_color(theme::color(cx, "border"))
                .child(global)
                .child(project)
        });
        let (back, forward) = (self.history_at > 0, self.history_at + 1 < self.history.len());
        let arrow = |cx: &mut Context<Self>, icon: &str, enabled: bool, selector: &'static str| {
            let color = theme::color(cx, if enabled { "fg-icon" } else { "fg-faint" });
            div()
                .id(selector)
                .debug_selector(move || selector.to_owned())
                .size(px(22.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(theme::length(cx, "radius-small"))
                .when(enabled, |el| el.hover(|el| el.bg(theme::color(cx, "bg-toolbar-hover"))))
                .child(theme::icon(cx, icon, 16.0, color))
        };
        let back = arrow(cx, "arrow-left", back, "settings-back")
            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.back(cx)));
        let forward = arrow(cx, "arrow-right", forward, "settings-forward")
            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.forward(cx)));
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(12.0))
            .px(px(16.0))
            .py(px(10.0))
            .child(div().flex_1().min_w_0().flex().items_center().gap(px(4.0)).children(trail))
            .children(reset)
            .children(switch)
            .child(div().flex().items_center().gap(px(2.0)).child(back).child(forward))
    }

    fn render_page(&self, key: &str, cx: &mut Context<Self>) -> AnyElement {
        let muted = theme::color(cx, "fg-subtle");
        let Some(node) = find_node(&self.tree, key).cloned() else {
            let text = tx(cx, "Nothing found");
            return div().text_color(muted).child(SharedString::from(text)).into_any_element();
        };
        if node.page.is_none() {
            let description = GROUP_DESCRIPTIONS.iter().find(|(group, _)| *group == node.title).map(|(_, text)| *text);
            let description = description.map(|text| {
                let text = tx(cx, text);
                div().text_color(muted).mb(px(8.0)).child(SharedString::from(text))
            });
            let query = self.model.read(cx).query.clone();
            let links: Vec<AnyElement> = node
                .children
                .iter()
                .map(|child| {
                    let (target, selector) = (child.key.clone(), format!("settings-link-{}", child.key));
                    let title = tx(cx, &child.title);
                    div()
                        .id(SharedString::from(selector.clone()))
                        .debug_selector(move || selector)
                        .text_color(theme::color(cx, "link"))
                        .hover(|el| el.text_color(theme::color(cx, "link-hover")))
                        .child(controls::search_label(cx, &title, &query))
                        .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.select(&target, cx)))
                        .into_any_element()
                })
                .collect();
            return div().flex().flex_col().gap(px(6.0)).children(description).children(links).into_any_element();
        }
        if let Some(view) = self.page_views.get(key) {
            return div().size_full().child(view.clone()).into_any_element();
        }
        match self.spec(key).filter(|spec| !spec.sections.is_empty()) {
            Some(spec) => {
                let page = key.to_owned();
                controls::page_body(self, spec, &|label: &str| self.custom_view(&page, label), cx).into_any_element()
            }
            None => {
                let text = tx(cx, UNAVAILABLE);
                div().text_color(muted).child(SharedString::from(text)).into_any_element()
            }
        }
    }

    fn render_footer(&self, cx: &mut Context<Self>) -> Div {
        let status = self.status(cx).map(|text| {
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .text_color(theme::color(cx, "danger"))
                .child(theme::icon(cx, "error", 14.0, theme::color(cx, "danger")))
                .child(SharedString::from(text))
        });
        let (ok, apply) = (self.can_ok(cx), self.can_apply(cx));
        let ok_label = tx(cx, "OK");
        let ok = controls::button(cx, &ok_label, true, ok)
            .id("settings-ok")
            .debug_selector(|| "settings-ok".to_owned())
            .when(ok, |el| el.on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.ok(cx))));
        let cancel_label = tx(cx, "Cancel");
        let cancel = controls::button(cx, &cancel_label, false, true)
            .id("settings-cancel")
            .debug_selector(|| "settings-cancel".to_owned())
            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.cancel(cx)));
        let apply_label = tx(cx, "Apply");
        let apply = controls::button(cx, &apply_label, false, apply)
            .id("settings-apply")
            .debug_selector(|| "settings-apply".to_owned())
            .when(apply, |el| el.on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.apply(cx))));
        // IntelliJ's order: macOS puts Cancel first and the default action last.
        let buttons: [AnyElement; 3] = if is_mac_host() {
            [cancel.into_any_element(), apply.into_any_element(), ok.into_any_element()]
        } else {
            [ok.into_any_element(), cancel.into_any_element(), apply.into_any_element()]
        };
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(8.0))
            .px(px(16.0))
            .py(px(10.0))
            .border_t_1()
            .border_color(theme::color(cx, "border"))
            .child(div().flex_1().min_w_0().children(status))
            .children(buttons)
    }
}

/// Runs `with` on the active locale's texts, or the English ones.
fn with_i18n<R>(cx: &App, with: impl FnOnce(&I18n) -> R) -> R {
    match cx.try_global::<Localization>() {
        Some(localization) => with(&localization.0),
        None => with(&I18n::default()),
    }
}

impl ControlHost for SettingsDialogView {
    fn model(&self) -> &Entity<SettingsDialog> {
        &self.model
    }

    fn controls(&self) -> &PageControls {
        &self.controls
    }

    fn controls_mut(&mut self) -> &mut PageControls {
        &mut self.controls
    }

    fn submit(&mut self, cx: &mut Context<Self>) {
        self.ok(cx);
    }

    fn cancel(&mut self, cx: &mut Context<Self>) {
        SettingsDialogView::cancel(self, cx);
    }
}

impl Focusable for SettingsDialogView {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.search.focus_handle(cx)
    }
}

impl Render for SettingsDialogView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (loading, new_projects) = {
            let dialog = self.model.read(cx);
            (dialog.pages_loading, dialog.new_projects)
        };
        let title = tx(cx, settings_title(new_projects));
        let border = theme::color(cx, "border");
        let body = if loading {
            let text = tx(cx, "Loading…");
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .items_center()
                .justify_center()
                .text_size(theme::length(cx, "fs-small"))
                .text_color(theme::color(cx, "fg-subtle"))
                .child(SharedString::from(text))
        } else {
            let search_focused = self.search.focus_handle(cx).is_focused(window);
            let sidebar = div()
                .flex_none()
                .w(px(SIDEBAR_WIDTH))
                .flex()
                .flex_col()
                .min_h_0()
                .border_r_1()
                .border_color(border)
                .child(
                    div()
                        .flex_none()
                        .mx(px(10.0))
                        .my(px(10.0))
                        .h(theme::length(cx, "h-control"))
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .px(px(8.0))
                        .rounded(theme::length(cx, "radius-small"))
                        .border_1()
                        .border_color(theme::color(cx, if search_focused { "focus-ring" } else { "border-strong" }))
                        .bg(theme::color(cx, "bg-input"))
                        .child(theme::icon(cx, "search", 14.0, theme::color(cx, "fg-muted")))
                        .child(div().flex_1().min_w_0().child(self.search.clone())),
                )
                .child(
                    div()
                        .id("settings-tree")
                        .track_focus(&self.tree_focus)
                        .flex_1()
                        .min_h_0()
                        .pb(px(6.0))
                        .overflow_y_scroll()
                        .child(self.render_tree(cx)),
                );
            let main = match self.current_key(cx) {
                Some(key) => {
                    let notes: Vec<Div> =
                        self.notes(cx).into_iter().map(|text| controls::note(cx, &tx(cx, text))).collect();
                    div()
                        .flex_1()
                        .min_w_0()
                        .min_h_0()
                        .flex()
                        .flex_col()
                        .child(self.render_header(&key, cx))
                        .child(div().flex_none().px(px(16.0)).children(notes))
                        .child(
                            div()
                                .id("settings-page")
                                .flex_1()
                                .min_h_0()
                                .px(px(16.0))
                                .pb(px(16.0))
                                .overflow_y_scroll()
                                .child(self.render_page(&key, cx)),
                        )
                }
                None => {
                    let text = tx(cx, "Nothing found");
                    div()
                        .flex_1()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(theme::color(cx, "fg-subtle"))
                        .child(SharedString::from(text))
                }
            };
            div().flex_1().min_h_0().flex().child(sidebar).child(main)
        };
        div()
            .w(px(DIALOG_WIDTH))
            .h(px(DIALOG_HEIGHT))
            .flex()
            .flex_col()
            .overflow_hidden()
            .bg(theme::color(cx, "bg-dialog"))
            .border_1()
            .border_color(theme::color(cx, "border-window"))
            .rounded(theme::length(cx, "radius-popup"))
            .text_size(theme::length(cx, "fs-ui"))
            .text_color(theme::color(cx, "fg"))
            // A press anywhere but on a choice closes its open list.
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, _, _, cx| {
                    if view.controls.close_select() {
                        cx.notify();
                    }
                }),
            )
            .child(
                div()
                    .flex_none()
                    .h(theme::length(cx, "h-header"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .border_b_1()
                    .border_color(border)
                    .text_color(theme::color(cx, "fg-strong"))
                    .child(SharedString::from(title)),
            )
            .child(body)
            .child(self.render_footer(cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings_model::mcp_server::mcp_server_page;
    use crate::settings_model::page_spec::OptionSpec;
    use crate::settings_model::schema::{default_settings, get_at, set_at};
    use crate::settings_model::staging::stage_key;
    use gpui_kit::{Modifiers, TestAppContext, VisualTestContext};
    use serde_json::json;
    use std::cell::RefCell;
    use std::rc::Rc;

    type Events = Rc<RefCell<Vec<SettingsDialogEvent>>>;

    const JVM_OPTIONS: &str = "languages.java.jdtlsVmArgs";

    fn specs() -> Vec<PageSpec> {
        vec![
            PageSpec::new("appearance", "Appearance", 0).group("Appearance & Behavior").section(
                None,
                vec![
                    OptionSpec::Select {
                        key: "appearance.theme",
                        label: "Theme",
                        options: vec![("light", "Islands Light"), ("dark", "Islands Dark")],
                    },
                    OptionSpec::Number {
                        key: "appearance.uiFontSize",
                        label: "UI font size",
                        min: 8.0,
                        max: 32.0,
                        integer: false,
                        unit: None,
                    },
                ],
            ),
            PageSpec::new("editor.general", "General", 20)
                .group("Editor")
                .section(
                    Some("Soft Wraps"),
                    vec![OptionSpec::Toggle { key: "editor.softWrap", label: "Soft-wrap files" }],
                )
                .section(
                    Some("On Save"),
                    vec![OptionSpec::Toggle { key: "editor.formatOnSave", label: "Reformat code on save" }],
                ),
            PageSpec::new("editor.font", "Font", 21)
                .group("Editor")
                .project_scope()
                .section(None, vec![OptionSpec::Text { key: "editor.fontFamily", label: "Font" }]),
            PageSpec::new("editor.codeStyle", "Code Style", 22).group("Editor").section(
                None,
                vec![OptionSpec::Number {
                    key: "editor.tabSize",
                    label: "Tab size",
                    min: 1.0,
                    max: 16.0,
                    integer: true,
                    unit: None,
                }],
            ),
            PageSpec::new("languages.java", "Java", 395)
                .group("Languages & Frameworks")
                .project_scope()
                .project_first()
                .section(None, vec![OptionSpec::Text { key: JVM_OPTIONS, label: "JVM options" }]),
            mcp_server_page(),
        ]
    }

    fn page_set() -> SettingsPageSet {
        SettingsPageSet {
            specs: specs(),
            pages: vec![
                SettingsPage::new("keymap", "Keymap").order(10).keywords(&["shortcut"]),
                SettingsPage::new(SSH_SETTINGS_PAGE, "SSH Configurations").group("Tools").order(600),
            ],
        }
    }

    fn init(cx: &mut TestAppContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
    }

    /// How the dialog is opened: on which page, over which settings.
    struct Opening {
        page: &'static str,
        settings: Value,
        global: Option<Value>,
        new_projects: bool,
        has_project: bool,
    }

    impl Opening {
        fn on(page: &'static str) -> Self {
            Self { page, settings: default_settings(), global: None, new_projects: false, has_project: false }
        }
    }

    /// The dialog in a window of its own with the keyboard in its search, and what it asked so far.
    type Opened<'a> = (Entity<SettingsDialogView>, Events, &'a mut VisualTestContext);

    fn open(cx: &mut TestAppContext, opening: Opening) -> Opened<'_> {
        init(cx);
        let Opening { page, settings, global, new_projects, has_project } = opening;
        let (view, cx) = cx.add_window_view(move |_, cx| {
            let model = cx.new(|_| {
                let mut dialog = SettingsDialog::default();
                dialog.open(Some(page), new_projects, settings);
                if let Some(global) = global {
                    dialog.set_global_base(global);
                }
                dialog
            });
            SettingsDialogView::new(model, page_set(), has_project, cx)
        });
        let events: Events = Rc::default();
        let log = events.clone();
        cx.update(|window, cx| {
            cx.subscribe(&view, move |_, event: &SettingsDialogEvent, _| log.borrow_mut().push(event.clone())).detach();
            let focus = view.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        (view, events, cx)
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        cx.run_until_parked();
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.run_until_parked();
    }

    fn focus_search(view: &Entity<SettingsDialogView>, cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            let focus = view.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
    }

    /// Types `text` over the text of setting `key`'s field.
    fn type_into(view: &Entity<SettingsDialogView>, key: &str, text: &str, cx: &mut VisualTestContext) {
        let field = view.read_with(cx, |view, _| view.field(key).cloned()).expect("the page has the field");
        cx.update(|window, cx| {
            field.update(cx, |field, cx| field.select_all(cx));
            window.focus(&field.focus_handle(cx), cx);
        });
        cx.run_until_parked();
        cx.simulate_input(text);
        cx.run_until_parked();
    }

    fn lines(view: &Entity<SettingsDialogView>, cx: &mut VisualTestContext) -> Vec<String> {
        view.read_with(cx, |view, cx| view.control_lines(cx))
    }

    fn selected(view: &Entity<SettingsDialogView>, cx: &mut VisualTestContext) -> Option<String> {
        view.read_with(cx, |view, cx| view.selected_page(cx))
    }

    fn can_apply(view: &Entity<SettingsDialogView>, cx: &mut VisualTestContext) -> bool {
        view.read_with(cx, |view, cx| view.can_apply(cx))
    }

    fn staged_ids(view: &Entity<SettingsDialogView>, cx: &mut VisualTestContext) -> Vec<String> {
        view.read_with(cx, |view, cx| {
            view.model().read(cx).staged().map(|change| stage_key(change.scope, &change.key)).collect()
        })
    }

    fn is_open(view: &Entity<SettingsDialogView>, cx: &mut VisualTestContext) -> bool {
        view.read_with(cx, |view, cx| view.model().read(cx).open)
    }

    /// The writes the last Apply asked for, as (scope, key, JSON).
    fn last_writes(events: &Events) -> Vec<(Scope, String, String)> {
        let events = events.borrow();
        let changes = events.iter().rev().find_map(|event| match event {
            SettingsDialogEvent::Apply { changes } => Some(changes.clone()),
            SettingsDialogEvent::Closed => None,
        });
        let changes = changes.unwrap_or_default();
        changes.into_iter().map(|change| (change.scope, change.key.clone(), change.json())).collect()
    }

    /// Carries out the Apply under way against settings held in memory, as the workbench does
    /// against the backend: `fail` is a key whose write fails. Returns whether all was saved.
    fn carry_out(
        view: &Entity<SettingsDialogView>,
        settings: &mut Value,
        fail: Option<&str>,
        cx: &mut VisualTestContext,
    ) -> bool {
        let changes = last_writes_of(view, cx);
        let mut written = 0;
        let mut failure = None;
        for change in &changes {
            if Some(change.key.as_str()) == fail {
                failure = Some("invalid value".to_owned());
                break;
            }
            *settings = set_at(settings, &change.key, change.value.clone());
            written += 1;
        }
        let base = settings.clone();
        view.update(cx, |view, cx| {
            view.apply_writes_done(written, failure.as_deref(), base, false, cx);
            while view.apply_next_secret(true, cx).is_some() {
                view.apply_secret_done(Ok(()), cx);
            }
            while let Some((_, value)) = view.apply_next_external(cx) {
                view.apply_external_done(Ok(value), cx);
            }
            view.finish_apply(cx)
        })
    }

    fn last_writes_of(view: &Entity<SettingsDialogView>, cx: &mut VisualTestContext) -> Vec<StagedChange> {
        view.read_with(cx, |view, _| view.apply.as_ref().map(|run| run.changes().to_vec()).unwrap_or_default())
    }

    fn default_bool(key: &str) -> bool {
        get_at(&default_settings(), key).and_then(Value::as_bool).unwrap_or(false)
    }

    // Settings dialog › searches pages and highlights matching option labels
    #[gpui_kit::test]
    async fn searches_pages_and_highlights_matching_option_labels(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx, Opening::on("appearance"));
        cx.simulate_input("soft wrap");
        cx.run_until_parked();
        let pages = view.read_with(cx, |view, cx| view.pages(cx));
        assert_eq!(pages, ["Editor", "  General"]);
        assert_eq!(selected(&view, cx).as_deref(), Some("editor.general"));
        assert_eq!(view.read_with(cx, |view, cx| view.highlighted(cx)), ["Soft-wrap files"]);
        assert_eq!(view.read_with(cx, |view, cx| view.breadcrumbs(cx)), ["Editor", "General"]);
    }

    // Settings dialog › Esc in the search field clears the filter first; ↓ selects the next page keeping the focus
    #[gpui_kit::test]
    async fn esc_in_the_search_field_clears_the_filter_first_and_down_selects_the_next_page(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, Opening::on("appearance"));
        cx.simulate_input("soft wrap");
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        assert_eq!(view.read_with(cx, |view, cx| view.search_field().read(cx).text().to_owned()), "");
        assert_eq!(view.read_with(cx, |view, cx| view.model().read(cx).query.clone()), "");
        assert!(events.borrow().is_empty(), "the dialog stays open");
        let before = selected(&view, cx);
        cx.simulate_keystrokes("down");
        cx.run_until_parked();
        assert_ne!(selected(&view, cx), before);
        let focused = cx.update(|window, cx| view.read(cx).search_field().focus_handle(cx).is_focused(window));
        assert!(focused, "the keyboard stays in the search");
        // A second Escape, with the search empty, cancels.
        cx.simulate_keystrokes("escape");
        assert_eq!(*events.borrow(), [SettingsDialogEvent::Closed]);
        assert!(!is_open(&view, cx));
    }

    // Settings dialog › stages edits and writes them on Apply
    #[gpui_kit::test]
    async fn stages_edits_and_writes_them_on_apply(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, Opening::on("editor.general"));
        assert!(!can_apply(&view, cx));
        click(cx, "settings-toggle-editor.softWrap");
        click(cx, "settings-toggle-editor.formatOnSave");
        assert!(events.borrow().is_empty(), "nothing is written before Apply");
        assert!(view.read_with(cx, |view, cx| view.modified_pages(cx).contains("editor.general")));
        click(cx, "settings-apply");
        let (soft_wrap, format) = (default_bool("editor.softWrap"), default_bool("editor.formatOnSave"));
        assert_eq!(
            last_writes(&events),
            [
                (Scope::Global, "editor.formatOnSave".to_owned(), (!format).to_string()),
                (Scope::Global, "editor.softWrap".to_owned(), (!soft_wrap).to_string()),
            ]
        );
        assert!(!view.read_with(cx, |view, cx| view.can_ok(cx)), "OK waits for the Apply under way");
        let mut settings = default_settings();
        assert!(carry_out(&view, &mut settings, None, cx));
        assert!(!can_apply(&view, cx));
        assert!(is_open(&view, cx), "Apply keeps the dialog open");
        assert!(!view.read_with(cx, |view, cx| view.is_modified(cx)));
        assert_eq!(get_at(&settings, "editor.softWrap"), Some(&json!(!soft_wrap)));
    }

    // Settings dialog › Cancel discards staged edits
    #[gpui_kit::test]
    async fn cancel_discards_staged_edits(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, Opening::on("editor.codeStyle"));
        type_into(&view, "editor.tabSize", "2", cx);
        assert_eq!(staged_ids(&view, cx), ["global|editor.tabSize"]);
        click(cx, "settings-cancel");
        assert_eq!(*events.borrow(), [SettingsDialogEvent::Closed]);
        assert!(!is_open(&view, cx));
        assert!(staged_ids(&view, cx).is_empty());
    }

    // Settings dialog › keeps global and project font edits separate when switching scope
    #[gpui_kit::test]
    async fn keeps_global_and_project_font_edits_separate_when_switching_scope(cx: &mut TestAppContext) {
        let global = set_at(&default_settings(), "editor.fontFamily", json!("Global Font"));
        let settings = set_at(&global, "editor.fontFamily", json!("Project Font"));
        let opening = Opening { settings, global: Some(global), has_project: true, ..Opening::on("editor.font") };
        let (view, events, cx) = open(cx, opening);
        assert!(view.read_with(cx, |view, cx| view.has_scope_switch(cx)));
        click(cx, "settings-scope-project");
        assert_eq!(lines(&view, cx), ["Font: Project Font"]);
        type_into(&view, "editor.fontFamily", "Project Maple", cx);
        click(cx, "settings-scope-global");
        assert_eq!(lines(&view, cx), ["Font: Global Font"]);
        type_into(&view, "editor.fontFamily", "Global Maple", cx);
        click(cx, "settings-scope-project");
        assert_eq!(lines(&view, cx), ["Font: Project Maple"]);
        click(cx, "settings-apply");
        assert_eq!(
            last_writes(&events),
            [
                (Scope::Global, "editor.fontFamily".to_owned(), r#""Global Maple""#.to_owned()),
                (Scope::Project, "editor.fontFamily".to_owned(), r#""Project Maple""#.to_owned()),
            ]
        );
    }

    fn java_opening() -> Opening {
        let global = set_at(&default_settings(), JVM_OPTIONS, json!("-Xmx1g"));
        let settings = set_at(&global, JVM_OPTIONS, json!("-Xmx4g"));
        Opening { settings, global: Some(global), has_project: true, ..Opening::on("languages.java") }
    }

    // toolchain settings with a project open › edit the project first; Global shows and writes the global layer
    #[gpui_kit::test]
    async fn edit_the_project_first_global_shows_and_writes_the_global_layer(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, java_opening());
        assert_eq!(view.read_with(cx, |view, cx| view.scope(cx)), Scope::Project);
        assert_eq!(lines(&view, cx), ["JVM options: -Xmx4g"]);
        assert_eq!(view.read_with(cx, |view, cx| view.notes(cx)), [PROJECT_NOTE]);
        click(cx, "settings-scope-global");
        assert_eq!(lines(&view, cx), ["JVM options: -Xmx1g"]);
        assert_eq!(view.read_with(cx, |view, cx| view.notes(cx)), [GLOBAL_NOTE]);
        type_into(&view, JVM_OPTIONS, "-Xmx2g", cx);
        click(cx, "settings-apply");
        assert_eq!(last_writes(&events), [(Scope::Global, JVM_OPTIONS.to_owned(), r#""-Xmx2g""#.to_owned())]);
    }

    // toolchain settings with a project open › Settings for New Projects edits the global layer on every page
    #[gpui_kit::test]
    async fn settings_for_new_projects_edits_the_global_layer_on_every_page(cx: &mut TestAppContext) {
        let opening = java_opening();
        let global = opening.global.clone().expect("a global layer");
        let (view, _, cx) = open(cx, Opening { settings: global, global: None, new_projects: true, ..opening });
        assert!(!view.read_with(cx, |view, cx| view.has_scope_switch(cx)));
        assert_eq!(view.read_with(cx, |view, cx| view.scope(cx)), Scope::Global);
        assert_eq!(lines(&view, cx), ["JVM options: -Xmx1g"]);
        assert_eq!(view.read_with(cx, |view, cx| view.notes(cx)), [GLOBAL_NOTE]);
    }

    // Settings dialog › enables Apply and Reset for a secret-only edit, and Reset discards it
    #[gpui_kit::test]
    async fn enables_apply_and_reset_for_a_secret_only_edit_and_reset_discards_it(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx, Opening::on(SSH_SETTINGS_PAGE));
        assert!(!can_apply(&view, cx));
        assert!(!view.read_with(cx, |view, cx| view.reset_visible(cx)));
        view.update(cx, |view, cx| {
            view.model().update(cx, |dialog, cx| {
                dialog.stage_ssh_secret("ssh-dialog", "not a real password");
                cx.notify();
            });
        });
        cx.run_until_parked();
        assert!(can_apply(&view, cx));
        assert!(view.read_with(cx, |view, cx| view.reset_visible(cx)));
        click(cx, "settings-reset");
        assert!(!can_apply(&view, cx));
        assert!(!view.read_with(cx, |view, cx| view.is_modified(cx)));
    }

    // Settings dialog › survives invalid settings values
    #[gpui_kit::test]
    async fn survives_invalid_settings_values(cx: &mut TestAppContext) {
        let settings = set_at(&default_settings(), "appearance.theme", json!(42));
        let settings = set_at(&settings, "appearance.uiFontSize", json!("x"));
        let (view, _, cx) = open(cx, Opening { settings, ..Opening::on("appearance") });
        assert_eq!(lines(&view, cx), ["Theme: Islands Light ▾", "UI font size: 13"]);
    }

    /// Enter is OK: it applies and, once everything was saved, closes; a failed write keeps the
    /// dialog open with the edit staged and says why.
    #[gpui_kit::test]
    async fn enter_applies_and_closes_once_saved_and_a_failure_keeps_the_dialog_open(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, Opening::on("editor.general"));
        click(cx, "settings-toggle-editor.softWrap");
        focus_search(&view, cx);
        cx.simulate_keystrokes("enter");
        assert_eq!(last_writes(&events).len(), 1);
        let mut settings = default_settings();
        assert!(!carry_out(&view, &mut settings, Some("editor.softWrap"), cx));
        assert!(is_open(&view, cx));
        let status = view.update(cx, |view, cx| view.status(cx));
        assert_eq!(status.as_deref(), Some("Could not save: editor.softWrap: invalid value"));
        assert_eq!(staged_ids(&view, cx), ["global|editor.softWrap"]);
        click(cx, "settings-ok");
        assert!(carry_out(&view, &mut settings, None, cx));
        assert_eq!(events.borrow().last(), Some(&SettingsDialogEvent::Closed));
        assert!(!is_open(&view, cx));
    }

    /// OK with nothing staged closes at once; Reset is hidden while an Apply runs.
    #[gpui_kit::test]
    async fn ok_with_nothing_staged_closes_and_reset_hides_while_saving(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, Opening::on("editor.general"));
        click(cx, "settings-toggle-editor.softWrap");
        assert!(view.read_with(cx, |view, cx| view.reset_visible(cx)));
        click(cx, "settings-apply");
        assert!(view.read_with(cx, |view, cx| view.is_modified(cx) && !view.reset_visible(cx)));
        let mut settings = default_settings();
        assert!(carry_out(&view, &mut settings, None, cx));
        click(cx, "settings-ok");
        assert_eq!(events.borrow().last(), Some(&SettingsDialogEvent::Closed));
    }

    /// The tree from the keyboard: Down moves, Right opens a group and then steps into it, Left
    /// goes to the group and then closes it. A group's page links to its pages; Back and Forward
    /// go through the pages visited.
    #[gpui_kit::test]
    async fn walks_the_tree_from_the_keyboard_and_through_the_history(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx, Opening::on("appearance"));
        let pages = |view: &Entity<SettingsDialogView>, cx: &mut VisualTestContext| {
            view.read_with(cx, |view, cx| view.pages(cx))
        };
        assert_eq!(
            pages(&view, cx),
            ["Appearance & Behavior", "  Appearance", "Keymap", "Editor", "Languages & Frameworks", "Tools"]
        );
        cx.update(|window, cx| {
            let focus = view.read(cx).tree_focus().clone();
            window.focus(&focus, cx);
        });
        cx.simulate_keystrokes("down down");
        assert_eq!(selected(&view, cx).as_deref(), Some("group:Editor"));
        assert_eq!(lines(&view, cx)[1..], ["→ General".to_owned(), "→ Font".to_owned(), "→ Code Style".to_owned()]);
        cx.simulate_keystrokes("right right");
        assert_eq!(selected(&view, cx).as_deref(), Some("editor.general"));
        cx.simulate_keystrokes("left left");
        assert_eq!(selected(&view, cx).as_deref(), Some("group:Editor"));
        assert_eq!(pages(&view, cx).len(), 6);
        click(cx, "settings-link-editor.font");
        assert_eq!(selected(&view, cx).as_deref(), Some("editor.font"));
        click(cx, "settings-back");
        assert_eq!(selected(&view, cx).as_deref(), Some("group:Editor"));
        click(cx, "settings-forward");
        assert_eq!(selected(&view, cx).as_deref(), Some("editor.font"));
    }

    /// A probe standing for a bespoke page's view.
    struct Probe;

    impl Render for Probe {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().debug_selector(|| "probe".to_owned()).size(px(20.0))
        }
    }

    /// A page without sections shows the view given for it, else says it is not available; a
    /// spec's control of its own does the same with the view given for its label.
    #[gpui_kit::test]
    async fn draws_the_views_given_for_bespoke_pages_and_controls(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx, Opening::on("keymap"));
        assert_eq!(lines(&view, cx), [UNAVAILABLE]);
        let probe = cx.update(|_, cx| AnyView::from(cx.new(|_| Probe)));
        view.update(cx, |view, cx| view.set_page_view("keymap", probe, cx));
        cx.run_until_parked();
        assert_eq!(lines(&view, cx), ["<page view>"]);
        assert!(cx.debug_bounds("probe").is_some());

        view.update(cx, |view, cx| view.select("tools.mcpServer", cx));
        cx.run_until_parked();
        // The checkbox's hint is a line of its own under it.
        let hint = "    Agents can read, edit, and run projects open in OpenIDEA through its built-in tools. Run tools \
                    ask for confirmation. The server listens on 127.0.0.1 only and requires a token.";
        let shown = lines(&view, cx);
        assert_eq!(shown, ["[ ] Enable MCP server".to_owned(), hint.to_owned(), format!("Endpoint: {UNAVAILABLE}")]);
        let endpoint = cx.update(|_, cx| AnyView::from(cx.new(|_| Probe)));
        view.update(cx, |view, cx| view.set_custom_control("tools.mcpServer", "Endpoint", endpoint, cx));
        assert_eq!(lines(&view, cx), ["[ ] Enable MCP server", hint, "Endpoint: <custom>"]);
    }
}
