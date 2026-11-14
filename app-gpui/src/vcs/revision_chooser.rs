//! The chooser of a branch, tag or revision for Compare with Branch or Revision…: HEAD, then the
//! branches shared by several repositories of the selection, then each repository's local and remote
//! branches and tags. Typing narrows the list and always offers the typed text as a revision of every
//! repository. ↑ and ↓ move, Enter or a click chooses, Escape cancels.
//!
//! The chooser reads nothing: its window reads the branches and tags of each repository and hands
//! them over with [`RevisionChooser::set_repositories`].

use crate::platform::i18n::{I18n, Localization, Param};
use crate::platform::input;
use crate::theme;
use crate::ui::matching::fuzzy_match;
use crate::ui::text_field::{TextField, TextFieldEvent};
use crate::vcs::branches::{sort_local, sort_remote};
use crate::vcs::changes::base_name;
use crate::vcs_ops::compare_paths::RevisionChoice;
use gpui_kit::{
    App, ClickEvent, Context, Entity, EventEmitter, FocusHandle, Focusable, FontWeight, IntoElement, Keystroke,
    MouseButton, Render, SharedString, Subscription, Window, div, prelude::*, px,
};
use ide_vcs::Branch;
use std::collections::BTreeMap;

const WIDTH: f32 = 460.0;
const LIST_HEIGHT: f32 = 280.0;

/// The branches and tags of one repository (`root`: `None` for the project's only one).
#[derive(Clone, Debug, Default)]
pub struct RepositoryRevisions {
    pub root: Option<String>,
    pub branches: Vec<Branch>,
    pub tags: Vec<String>,
}

/// A row of the chooser: a revision to choose, or the heading of a repository.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RevisionItem {
    pub rev: String,
    /// What the revision is, in the UI language: "local", "remote", "tag", "revision"…
    pub detail: String,
    /// The repositories it is chosen for; `None` applies it to every repository of the selection.
    pub repositories: Option<Vec<String>>,
    /// The byte offsets of `rev` the typed text matched.
    pub matches: Option<Vec<usize>>,
    pub section: bool,
}

impl RevisionItem {
    fn new(rev: &str, detail: String, repositories: Option<Vec<String>>) -> Self {
        Self { rev: rev.to_owned(), detail, repositories, matches: None, section: false }
    }

    pub fn choice(&self) -> RevisionChoice {
        RevisionChoice { rev: self.rev.clone(), repositories: self.repositories.clone() }
    }
}

/// The rows for `repos` narrowed to `query`: shared branches first, then each repository's revisions.
pub fn revision_items(repos: &[RepositoryRevisions], query: &str, i18n: &mut I18n) -> Vec<RevisionItem> {
    let mut all = vec![RevisionItem::new("HEAD", i18n.tx("Last commit"), None)];
    let multi = repos.len() > 1;
    if multi {
        let mut shared: BTreeMap<&str, Vec<String>> = BTreeMap::new();
        let mut order: Vec<&str> = Vec::new();
        for repo in repos {
            let mut names: Vec<&str> = repo.branches.iter().map(|branch| branch.name.as_str()).collect();
            names.dedup();
            for name in names {
                let roots = shared.entry(name).or_insert_with(|| {
                    order.push(name);
                    Vec::new()
                });
                if let Some(root) = &repo.root {
                    roots.push(root.clone());
                }
            }
        }
        for name in order {
            let roots = shared.remove(name).unwrap_or_default();
            if roots.len() > 1 {
                let detail = i18n.tx_built(&["in ", " repositories"], &[Param::from(roots.len())]);
                all.push(RevisionItem::new(name, detail, Some(roots)));
            }
        }
    }
    for repo in repos {
        if multi {
            let root = repo.root.clone().unwrap_or_default();
            let name = base_name(&root).to_owned();
            all.push(RevisionItem { section: true, ..RevisionItem::new(&name, root, Some(Vec::new())) });
        }
        let repositories = repo.root.clone().map(|root| vec![root]);
        for branch in sort_local(&repo.branches, None) {
            let detail = i18n.tx(if branch.current { "current branch" } else { "local" });
            all.push(RevisionItem::new(&branch.name, detail, repositories.clone()));
        }
        for branch in sort_remote(&repo.branches, None) {
            all.push(RevisionItem::new(&branch.name, i18n.tx("remote"), repositories.clone()));
        }
        for tag in &repo.tags {
            all.push(RevisionItem::new(tag, i18n.tx("tag"), repositories.clone()));
        }
    }
    let query = query.trim();
    if query.is_empty() {
        return all;
    }
    let matched: Vec<RevisionItem> = all
        .into_iter()
        .filter_map(|item| {
            if item.section {
                return Some(item);
            }
            let matches = fuzzy_match(&item.rev, query)?;
            Some(RevisionItem { matches: Some(matches), ..item })
        })
        .collect();
    // A heading stays only above a revision of its own.
    let mut filtered: Vec<RevisionItem> = Vec::with_capacity(matched.len());
    for (index, item) in matched.iter().enumerate() {
        if item.section && !matched.get(index + 1).is_some_and(|next| !next.section) {
            continue;
        }
        filtered.push(item.clone());
    }
    // The typed text applies to the whole selection, even when one repository offers it.
    if !filtered.iter().any(|item| !item.section && item.rev == query && item.repositories.is_none()) {
        filtered.push(RevisionItem::new(query, i18n.tx("revision"), None));
    }
    filtered
}

/// The next row in `direction` from `active` that is not a heading, going round the ends.
fn step(items: &[RevisionItem], active: usize, down: bool) -> usize {
    let len = items.len();
    if len == 0 {
        return 0;
    }
    let mut at = active.min(len - 1);
    for _ in 0..len {
        at = if down { (at + 1) % len } else { (at + len - 1) % len };
        if !items[at].section {
            return at;
        }
    }
    active
}

/// The first row that is not a heading.
fn first_choosable(items: &[RevisionItem]) -> usize {
    items.iter().position(|item| !item.section).unwrap_or(0)
}

/// What the chooser tells its window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RevisionChooserEvent {
    Chosen(RevisionChoice),
    Cancelled,
}

/// A row as drawn, for tests and the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChooserRow {
    pub rev: String,
    pub detail: String,
    pub section: bool,
    pub active: bool,
}

pub struct RevisionChooser {
    title: String,
    repos: Vec<RepositoryRevisions>,
    error: Option<String>,
    field: Entity<TextField>,
    items: Vec<RevisionItem>,
    active: usize,
    key_claim: u64,
    _field_events: Subscription,
}

impl EventEmitter<RevisionChooserEvent> for RevisionChooser {}

impl RevisionChooser {
    /// A chooser titled `title` (in the UI language) with HEAD only, until the repositories are read.
    pub fn new(title: String, cx: &mut Context<Self>) -> Self {
        let placeholder = cx.default_global::<Localization>().0.tx("Branch, tag or commit hash");
        let field = cx.new(|cx| TextField::new(cx).with_placeholder(placeholder));
        let field_events = cx.subscribe(&field, |chooser: &mut Self, _, event: &TextFieldEvent, cx| match event {
            TextFieldEvent::Changed => chooser.query_changed(cx),
            // Enter is the chooser's own key: it is claimed before the field sees it.
            TextFieldEvent::Submitted => {}
            TextFieldEvent::Cancelled => cx.emit(RevisionChooserEvent::Cancelled),
        });
        let focus = field.focus_handle(cx);
        let claimant = cx.weak_entity();
        let key_claim = input::claim_keys(
            focus,
            move |keystroke, _, cx| {
                claimant.update(cx, |chooser, cx| chooser.claim_key(keystroke, cx)).unwrap_or(false)
            },
            cx,
        );
        cx.on_release(|chooser, cx| input::release_keys(chooser.key_claim, cx)).detach();
        let mut chooser = Self {
            title,
            repos: Vec::new(),
            error: None,
            field,
            items: Vec::new(),
            active: 0,
            key_claim,
            _field_events: field_events,
        };
        chooser.rebuild(cx);
        chooser
    }

    /// The branches and tags of the repositories arrived.
    pub fn set_repositories(&mut self, repos: Vec<RepositoryRevisions>, cx: &mut Context<Self>) {
        self.repos = repos;
        self.rebuild(cx);
    }

    /// A repository could not be read: said above the list.
    pub fn set_error(&mut self, error: String, cx: &mut Context<Self>) {
        self.error = Some(error);
        cx.notify();
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn field(&self) -> &Entity<TextField> {
        &self.field
    }

    pub fn rows(&self) -> Vec<ChooserRow> {
        self.items
            .iter()
            .enumerate()
            .map(|(index, item)| ChooserRow {
                rev: item.rev.clone(),
                detail: item.detail.clone(),
                section: item.section,
                active: index == self.active,
            })
            .collect()
    }

    fn rebuild(&mut self, cx: &mut Context<Self>) {
        let query = self.field.read(cx).text().to_owned();
        let i18n = &mut cx.default_global::<Localization>().0;
        self.items = revision_items(&self.repos, &query, i18n);
        self.active = first_choosable(&self.items);
        cx.notify();
    }

    fn query_changed(&mut self, cx: &mut Context<Self>) {
        self.rebuild(cx);
    }

    /// Chooses row `index`; a heading is not a choice.
    pub fn choose(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(item) = self.items.get(index).filter(|item| !item.section) else { return };
        cx.emit(RevisionChooserEvent::Chosen(item.choice()));
    }

    fn claim_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        if keystroke.modifiers.modified() {
            return false;
        }
        match keystroke.key.as_str() {
            "enter" => self.choose(self.active, cx),
            "escape" => cx.emit(RevisionChooserEvent::Cancelled),
            "down" | "up" => {
                self.active = step(&self.items, self.active, keystroke.key == "down");
                cx.notify();
            }
            _ => return false,
        }
        true
    }
}

impl Focusable for RevisionChooser {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.field.focus_handle(cx)
    }
}

impl Render for RevisionChooser {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (fg, muted) = (theme::color(cx, "fg"), theme::color(cx, "fg-muted"));
        let revision = cx.default_global::<Localization>().0.tx("revision");
        let tag = cx.default_global::<Localization>().0.tx("tag");
        let rows: Vec<_> = self
            .items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let selector = format!("revision-row-{index}");
                if item.section {
                    return div()
                        .id(SharedString::from(selector.clone()))
                        .debug_selector(move || selector.clone())
                        .h(px(24.0))
                        .flex_none()
                        .px(px(10.0))
                        .flex()
                        .items_center()
                        .text_color(muted)
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(SharedString::from(item.rev.clone()));
                }
                let icon = if item.detail == revision {
                    "commit"
                } else if item.detail == tag {
                    "tag"
                } else {
                    "branch"
                };
                div()
                    .id(SharedString::from(selector.clone()))
                    .debug_selector(move || selector.clone())
                    .h(px(24.0))
                    .flex_none()
                    .px(px(10.0))
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .when(index == self.active, |el| el.bg(theme::color(cx, "bg-selected")))
                    .child(theme::icon(cx, icon, 14.0, muted))
                    .child(div().text_color(fg).child(SharedString::from(item.rev.clone())))
                    .child(div().text_color(muted).child(SharedString::from(item.detail.clone())))
                    .on_click(cx.listener(move |chooser, _: &ClickEvent, _, cx| chooser.choose(index, cx)))
            })
            .collect();
        let error = self.error.clone().map(|error| {
            div().px(px(10.0)).py(px(4.0)).text_color(theme::color(cx, "danger")).child(SharedString::from(error))
        });
        let title = self.title.clone();
        div()
            .debug_selector(|| "revision-chooser".to_owned())
            .w(px(WIDTH))
            .flex()
            .flex_col()
            .bg(theme::color(cx, "bg-popup"))
            .border_1()
            .border_color(theme::color(cx, "border-popup"))
            .rounded(theme::length(cx, "radius-popup"))
            .text_size(theme::length(cx, "fs-ui"))
            .text_color(fg)
            // A click on the chooser is not a click beside it.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .px(px(12.0))
                    .pt(px(10.0))
                    .pb(px(6.0))
                    .text_color(theme::color(cx, "fg-strong"))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(SharedString::from(title)),
            )
            .child(
                div()
                    .px(px(10.0))
                    .py(px(8.0))
                    .border_b_1()
                    .border_color(theme::color(cx, "border-popup"))
                    .child(self.field.clone()),
            )
            .children(error)
            .child(
                div()
                    .id("revision-list")
                    .h(px(LIST_HEIGHT))
                    .py(px(4.0))
                    .flex()
                    .flex_col()
                    .overflow_y_scroll()
                    .children(rows),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{TestAppContext, VisualTestContext};
    use std::cell::RefCell;
    use std::rc::Rc;

    fn branch(name: &str, remote: bool, current: bool) -> Branch {
        Branch {
            name: name.to_owned(),
            remote,
            current,
            upstream: None,
            ahead: 0,
            behind: 0,
            last_commit_ms: 0.0,
            commit: String::new(),
        }
    }

    fn repo(root: &str, branches: &[&str]) -> RepositoryRevisions {
        RepositoryRevisions {
            root: Some(root.to_owned()),
            branches: branches.iter().map(|name| branch(name, false, false)).collect(),
            tags: Vec::new(),
        }
    }

    fn revs(items: &[RevisionItem]) -> Vec<&str> {
        items.iter().map(|item| item.rev.as_str()).collect()
    }

    // src/features/vcs/__tests__/folderCompare.test.tsx › directory comparison › offers shared branches plus each
    // repository's local, remote and tag revisions
    #[test]
    fn offers_shared_branches_plus_each_repositorys_local_remote_and_tag_revisions() {
        let mut i18n = I18n::default();
        let mut client = repo("/workspace/client", &["main", "client-only"]);
        client.branches.push(branch("origin/main", true, false));
        client.tags = vec!["v1".to_owned()];
        let repos = [client, repo("/workspace/server", &["main", "server-only"]), repo("/workspace/admin", &["main"])];
        let items = revision_items(&repos, "", &mut i18n);
        let in_all = |item: &&RevisionItem| item.repositories.as_ref().is_some_and(|roots| roots.len() == 3);
        let shared = items.iter().filter(in_all).find(|item| item.rev == "main");
        assert_eq!(shared.map(|item| item.detail.as_str()), Some("in 3 repositories"));
        let sections: Vec<&str> = items.iter().filter(|item| item.section).map(|item| item.rev.as_str()).collect();
        assert_eq!(sections, ["client", "server", "admin"]);
        let client_only = Some(vec!["/workspace/client".to_owned()]);
        let roots_of = |rev: &str| items.iter().find(|item| item.rev == rev).map(|item| item.repositories.clone());
        assert_eq!(roots_of("origin/main"), Some(client_only.clone()));
        assert_eq!(roots_of("v1"), Some(client_only));
        let typed = revision_items(&repos, "client-only", &mut i18n);
        assert!(typed.iter().any(|item| item.rev == "client-only" && item.repositories.is_none()));
    }

    // Not from a spec: one repository lists HEAD, its local branches, remote branches and tags.
    #[test]
    fn lists_head_then_the_repositorys_branches_and_tags() {
        let mut i18n = I18n::default();
        let one = RepositoryRevisions {
            root: Some("/r".to_owned()),
            branches: vec![branch("main", false, true), branch("origin/main", true, false)],
            tags: vec!["v1".to_owned()],
        };
        let items = revision_items(&[one], "", &mut i18n);
        assert_eq!(revs(&items), ["HEAD", "main", "origin/main", "v1"]);
        let details: Vec<&str> = items.iter().map(|item| item.detail.as_str()).collect();
        assert_eq!(details, ["Last commit", "current branch", "remote", "tag"]);
        assert_eq!(items[1].repositories, Some(vec!["/r".to_owned()]));
    }

    // Not from a spec: a branch of several repositories is offered for all of them, before each one's list.
    #[test]
    fn offers_a_shared_branch_first_and_the_typed_text_as_a_revision_of_every_repository() {
        let mut i18n = I18n::default();
        let repos = [repo("/w/client", &["main", "ui"]), repo("/w/server", &["main"])];
        let items = revision_items(&repos, "", &mut i18n);
        assert_eq!(revs(&items), ["HEAD", "main", "client", "main", "ui", "server", "main"]);
        assert_eq!(items[1].detail, "in 2 repositories");
        assert_eq!(items[1].repositories, Some(vec!["/w/client".to_owned(), "/w/server".to_owned()]));
        assert!(items[2].section);

        let narrowed = revision_items(&repos, "ui", &mut i18n);
        assert_eq!(revs(&narrowed), ["client", "ui", "ui"], "the server heading goes with its rows");
        let typed = narrowed.last().expect("the typed text is offered");
        assert_eq!((typed.detail.as_str(), typed.repositories.clone()), ("revision", None));
    }

    type Events = Rc<RefCell<Vec<RevisionChooserEvent>>>;

    fn open(cx: &mut TestAppContext) -> (Entity<RevisionChooser>, Events, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let title = "Compare with Branch or Revision".to_owned();
        let (chooser, cx) = cx.add_window_view(move |_, cx| RevisionChooser::new(title, cx));
        cx.update(|window, cx| {
            window.activate_window();
            let focus = chooser.focus_handle(cx);
            window.focus(&focus, cx);
        });
        let events: Events = Rc::default();
        let seen = Rc::clone(&events);
        cx.update(|_, cx| {
            cx.subscribe(&chooser, move |_, event: &RevisionChooserEvent, _| seen.borrow_mut().push(event.clone()))
                .detach();
        });
        let repos = vec![RepositoryRevisions {
            root: None,
            branches: vec![branch("main", false, true), branch("feature", false, false)],
            tags: Vec::new(),
        }];
        chooser.update(cx, |chooser, cx| chooser.set_repositories(repos, cx));
        cx.run_until_parked();
        (chooser, events, cx)
    }

    // Not from a spec: typing narrows the list, ↓ skips to the next row, Enter chooses it, Escape cancels.
    #[gpui_kit::test]
    async fn typing_narrows_the_list_and_enter_chooses_the_active_revision(cx: &mut TestAppContext) {
        let (chooser, events, cx) = open(cx);
        let shown = |cx: &mut VisualTestContext| -> Vec<String> {
            chooser.read_with(cx, |chooser, _| chooser.rows().into_iter().map(|row| row.rev).collect())
        };
        assert_eq!(shown(cx), ["HEAD", "main", "feature"]);
        cx.simulate_input("feat");
        cx.run_until_parked();
        assert_eq!(shown(cx), ["feature", "feat"]);
        cx.simulate_keystrokes("down");
        assert!(chooser.read_with(cx, |chooser, _| chooser.rows()[1].active));
        cx.simulate_keystrokes("up enter");
        let chosen = RevisionChoice { rev: "feature".to_owned(), repositories: None };
        assert_eq!(events.borrow().last(), Some(&RevisionChooserEvent::Chosen(chosen)));
        cx.simulate_keystrokes("escape");
        assert_eq!(events.borrow().last(), Some(&RevisionChooserEvent::Cancelled));
    }
}
