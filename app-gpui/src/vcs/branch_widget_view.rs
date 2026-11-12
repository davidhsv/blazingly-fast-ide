//! The title bar's version control widgets.
//!
//! [`BranchWidget`] names the current branch (or the short commit of a detached HEAD, or the
//! operation the repository is stopped in) with markers for incoming and outgoing commits; a click
//! asks for the Branches popup. [`MergeRebaseWidget`] shows Resolve, Continue and Abort while a
//! merge, rebase, cherry-pick or revert is stopped or conflicts are left. Neither reads the
//! repository: the workbench hands them the state and carries out what their events ask.

use crate::platform::i18n::{I18n, Localization, Param};
use crate::theme;
use crate::vcs::branches::{RepoEntry, widget_shows_repo};
use crate::vcs_ops::branch_widget::head_label;
use crate::vcs_ops::branch_widget_extensions::BranchPresentation;
use crate::vcs_ops::operations::{
    Control, InProgress, control_label, in_progress, operation_controls, operation_title,
};
use crate::vcs_ops::store::{RepoHead, VcsState};
use gpui_kit::{
    App, ClickEvent, Context, EventEmitter, IntoElement, Render, SharedString, Window, div, prelude::*, px,
};
use ide_vcs::BranchInfo;

/// What the branch widget shows.
#[derive(Clone, Debug, Default)]
pub struct BranchWidgetState {
    /// The project is a Git repository: the widget is shown.
    pub available: bool,
    /// HEAD of the repository the widget names.
    pub branch: Option<BranchInfo>,
    /// The repository named beside the branch: several repositories that are not on one branch.
    pub repo: Option<String>,
    /// How many repositories the project has.
    pub repositories: usize,
    /// Another feature's presentation of the branch (a pull request on it).
    pub presentation: Option<BranchPresentation>,
    /// The Branches popup is open.
    pub open: bool,
}

impl BranchWidgetState {
    /// The state of the widget over the version control state. `current`: the current repository
    /// ([`crate::vcs::branches::current_repo_for`]); when the repositories are on different
    /// branches and it is not the selected one, the widget names its HEAD from the last scan (it
    /// has no upstream there), and no presentation applies.
    pub fn from_vcs(
        vcs: &VcsState,
        current: Option<&str>,
        presentation: Option<&BranchPresentation>,
        open: bool,
    ) -> Self {
        let entries: Vec<RepoEntry> = vcs
            .roots
            .iter()
            .map(|root| {
                let head = vcs.heads.get(root);
                RepoEntry {
                    root: root.clone(),
                    name: vcs.repo_name(root).to_owned(),
                    head: head.and_then(|head| head.head.clone()),
                    head_commit: head.and_then(|head| head.head_commit.clone()),
                }
            })
            .collect();
        let diverged = widget_shows_repo(&entries);
        let other = current.filter(|current| diverged && Some(*current) != vcs.current_repo_root());
        let branch = match other {
            Some(root) => {
                let head = vcs.heads.get(root).cloned().unwrap_or_default();
                Some(BranchInfo {
                    head: head.head,
                    head_commit: head.head_commit,
                    upstream: None,
                    ahead: 0,
                    behind: 0,
                    operation: vcs.operations.get(root).cloned(),
                })
            }
            None => vcs.branch.clone(),
        };
        Self {
            available: vcs.available,
            branch,
            repo: current.filter(|_| diverged).map(|root| vcs.repo_name(root).to_owned()),
            repositories: vcs.roots.len(),
            presentation: presentation.filter(|_| other.is_none()).cloned(),
            open,
        }
    }
}

/// What the branch widget asks of the workbench.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BranchWidgetEvent {
    /// Open the Branches popup, or close it when it is open.
    TogglePopup,
}

pub struct BranchWidget {
    state: BranchWidgetState,
}

impl EventEmitter<BranchWidgetEvent> for BranchWidget {}

/// The operation a HEAD is stopped in, as the backend names it.
fn operation_of(branch: &BranchInfo) -> Option<&str> {
    branch.operation.as_deref().filter(|op| !op.is_empty())
}

impl BranchWidget {
    pub fn new(state: BranchWidgetState) -> Self {
        Self { state }
    }

    pub fn set_state(&mut self, state: BranchWidgetState, cx: &mut Context<Self>) {
        self.state = state;
        cx.notify();
    }

    pub fn state(&self) -> &BranchWidgetState {
        &self.state
    }

    /// Whether the widget draws anything: the project is a repository whose HEAD is known.
    pub fn is_shown(&self) -> bool {
        self.state.available && self.state.branch.is_some()
    }

    fn head(&self) -> Option<RepoHead> {
        let branch = self.state.branch.as_ref()?;
        Some(RepoHead { head: branch.head.clone(), head_commit: branch.head_commit.clone() })
    }

    /// The widget's text: "Rebasing main" while an operation is stopped, else another feature's
    /// presentation, else the branch or the short commit of a detached HEAD.
    pub fn label(&self, i18n: &mut I18n) -> String {
        let Some(branch) = self.state.branch.as_ref() else { return String::new() };
        if let Some(op) = in_progress(operation_of(branch)) {
            return operation_title(op, branch.head.as_deref(), i18n);
        }
        match &self.state.presentation {
            Some(presentation) => presentation.text.clone(),
            None => head_label(self.head().as_ref(), i18n),
        }
    }

    /// The icon before the label: the presentation's, a branch, or a commit on a detached HEAD.
    pub fn icon(&self) -> &str {
        if let Some(icon) = self.state.presentation.as_ref().and_then(|presentation| presentation.icon.as_deref()) {
            return icon;
        }
        let named = self.state.branch.as_ref().and_then(|branch| branch.head.as_deref()).is_some_and(|h| !h.is_empty());
        if named { "branch" } else { "commit" }
    }

    /// An operation the repository is in that has no controls (a bisect), shown beside the label.
    pub fn operation_tag(&self) -> Option<&str> {
        let op = operation_of(self.state.branch.as_ref()?)?;
        in_progress(Some(op)).is_none().then_some(op)
    }

    /// Commits to pull, and commits to push.
    pub fn counts(&self) -> (u32, u32) {
        self.state.branch.as_ref().map_or((0, 0), |branch| (branch.behind, branch.ahead))
    }

    /// The tooltip's lines: the presentation's, the repository, the branch, what it tracks, the
    /// repositories it stands for and the commits to pull and push.
    pub fn tooltip(&self, i18n: &mut I18n) -> Vec<String> {
        let Some(branch) = self.state.branch.as_ref() else { return Vec::new() };
        let head = head_label(self.head().as_ref(), i18n);
        let head = match operation_of(branch) {
            Some(op) => format!("{head} ({op})"),
            None => head,
        };
        let mut lines = Vec::new();
        if let Some(tooltip) = self.state.presentation.as_ref().and_then(|presentation| presentation.tooltip.clone()) {
            lines.push(tooltip);
        }
        if let Some(repo) = &self.state.repo {
            lines.push(i18n.tx_built(&["Repository: ", ""], &[Param::from(repo.as_str())]));
        }
        lines.push(i18n.tx_built(&["Git branch: ", ""], &[Param::from(head.as_str())]));
        if let Some(upstream) = branch.upstream.as_deref().filter(|upstream| !upstream.is_empty()) {
            lines.push(i18n.tx_built(&["Tracking ", ""], &[Param::from(upstream)]));
        }
        if self.state.repositories > 1 && self.state.repo.is_none() {
            lines.push(i18n.tx_built(&["In all ", " repositories"], &[Param::from(self.state.repositories)]));
        }
        let (behind, ahead) = (branch.behind as usize, branch.ahead as usize);
        let sync = match (behind > 0, ahead > 0) {
            (true, true) => Some(
                i18n.tx_built(&["", " incoming, ", " outgoing commits"], &[Param::from(behind), Param::from(ahead)]),
            ),
            (true, false) => Some(i18n.tx_built(&["", " incoming commits"], &[Param::from(behind)])),
            (false, true) => Some(i18n.tx_built(&["", " outgoing commits"], &[Param::from(ahead)])),
            (false, false) => None,
        };
        lines.extend(sync);
        lines
    }
}

impl Render for BranchWidget {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.is_shown() {
            return div().into_any_element();
        }
        let label = self.label(&mut cx.default_global::<Localization>().0);
        let (fg, muted, subtle) =
            (theme::color(cx, "fg-strong"), theme::color(cx, "fg-muted"), theme::color(cx, "fg-subtle"));
        let (behind, ahead) = self.counts();
        let hover_bg = theme::color(cx, "bg-hover");
        let open = self.state.open;
        let repo = self.state.repo.clone().map(|repo| {
            div()
                .flex_none()
                .max_w(px(120.0))
                .flex()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_color(muted)
                .child(SharedString::from(repo))
                .child(div().text_color(subtle).child(SharedString::from(" /")))
        });
        let tag = self.operation_tag().map(|op| {
            div()
                .flex_none()
                .text_color(theme::color(cx, "warning"))
                .text_size(theme::length(cx, "fs-tiny"))
                .child(SharedString::from(op.to_uppercase()))
        });
        let incoming = (behind > 0).then(|| theme::icon(cx, "vcs-incoming", 12.0, theme::color(cx, "accent")));
        let outgoing = (ahead > 0).then(|| theme::icon(cx, "vcs-outgoing", 12.0, theme::color(cx, "success")));
        div()
            .id("vcs-branch-widget")
            .debug_selector(|| "vcs-branch-widget".to_owned())
            .h(px(30.0))
            .max_w(px(280.0))
            .px(px(8.0))
            .flex()
            .items_center()
            .gap(px(4.0))
            .rounded(px(6.0))
            .whitespace_nowrap()
            .text_color(fg)
            .text_size(theme::length(cx, "fs-ui"))
            .when(open, |el| el.bg(hover_bg))
            .hover(move |el| el.bg(hover_bg))
            .child(theme::icon(cx, self.icon(), 16.0, muted))
            .children(repo)
            .child(div().min_w_0().overflow_hidden().child(SharedString::from(label)))
            .children(tag)
            .children(incoming)
            .children(outgoing)
            .child(theme::icon(cx, "chevron-down", 14.0, subtle))
            .on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(BranchWidgetEvent::TogglePopup)))
            .into_any_element()
    }
}

/// What the merge and rebase buttons show.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MergeRebaseState {
    /// The operation the repository is stopped in, as the backend names it.
    pub operation: Option<String>,
    /// Files with unresolved conflicts are left.
    pub conflicts: bool,
    /// A network operation runs: the buttons wait for it.
    pub busy: bool,
}

/// What the merge and rebase buttons ask of the workbench.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MergeRebaseEvent {
    /// Open the conflicts dialog (`vcs.resolveConflicts`).
    ResolveConflicts,
    /// Continue or abort the stopped operation.
    Control(InProgress, Control),
}

/// A button of the merge and rebase widget.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MergeButton {
    Resolve,
    Control(InProgress, Control),
}

pub struct MergeRebaseWidget {
    state: MergeRebaseState,
}

impl EventEmitter<MergeRebaseEvent> for MergeRebaseWidget {}

impl MergeRebaseWidget {
    pub fn new(state: MergeRebaseState) -> Self {
        Self { state }
    }

    pub fn set_state(&mut self, state: MergeRebaseState, cx: &mut Context<Self>) {
        self.state = state;
        cx.notify();
    }

    /// Whether the widget belongs in the title bar: an operation is stopped or conflicts are left.
    pub fn is_needed(&self) -> bool {
        self.state.conflicts || in_progress(self.state.operation.as_deref()).is_some()
    }

    /// Resolve while conflicts are left; Continue once they are resolved (a merge is concluded by
    /// committing); Abort while an operation is stopped.
    fn buttons(&self) -> Vec<MergeButton> {
        let op = in_progress(self.state.operation.as_deref());
        let mut out = Vec::new();
        if self.state.conflicts {
            out.push(MergeButton::Resolve);
        }
        if let Some(op) = op {
            let controls = operation_controls(op);
            if !self.state.conflicts && controls.contains(&Control::Continue) {
                out.push(MergeButton::Control(op, Control::Continue));
            }
            if controls.contains(&Control::Abort) {
                out.push(MergeButton::Control(op, Control::Abort));
            }
        }
        out
    }

    fn label(button: MergeButton, i18n: &I18n) -> String {
        match button {
            MergeButton::Resolve => i18n.tx("Resolve Conflicts…"),
            MergeButton::Control(op, control) => i18n.tx(&control_label(op, control)),
        }
    }

    /// The buttons' labels (their tooltips), in the UI language.
    pub fn labels(&self, cx: &App) -> Vec<String> {
        let fallback;
        let i18n = match cx.try_global::<Localization>() {
            Some(localization) => &localization.0,
            None => {
                fallback = I18n::default();
                &fallback
            }
        };
        self.buttons().into_iter().map(|button| Self::label(button, i18n)).collect()
    }

    /// Whether the buttons can be pressed.
    pub fn enabled(&self) -> bool {
        !self.state.busy
    }
}

impl Render for MergeRebaseWidget {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let enabled = self.enabled();
        let buttons: Vec<_> = self
            .buttons()
            .into_iter()
            .enumerate()
            .map(|(index, button)| {
                let (icon, tone) = match button {
                    MergeButton::Control(_, Control::Abort) => ("vcs-abort", "danger"),
                    _ => ("resolve-continue", "success"),
                };
                let color = theme::color(cx, if enabled { tone } else { "fg-faint" });
                let hover_bg = theme::color(cx, "bg-hover");
                let event = match button {
                    MergeButton::Resolve => MergeRebaseEvent::ResolveConflicts,
                    MergeButton::Control(op, control) => MergeRebaseEvent::Control(op, control),
                };
                div()
                    .id(SharedString::from(format!("merge-rebase-{index}")))
                    .debug_selector(|| format!("merge-rebase-{index}"))
                    .size(px(26.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(6.0))
                    .when(enabled, |el| el.hover(move |el| el.bg(hover_bg)))
                    .child(theme::icon(cx, icon, 16.0, color))
                    .on_click(cx.listener(move |widget, _: &ClickEvent, _, cx| {
                        if widget.enabled() {
                            cx.emit(event);
                        }
                    }))
            })
            .collect();
        div().flex().items_center().gap(px(2.0)).children(buttons)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::input;
    use gpui_kit::{Entity, Modifiers, TestAppContext, VisualTestContext};
    use serde_json::json;
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::rc::Rc;

    fn info(head: Option<&str>, upstream: Option<&str>, ahead: u32, behind: u32, op: Option<&str>) -> BranchInfo {
        let wire = json!({
            "head": head,
            "headCommit": "0123456789abcdef",
            "upstream": upstream,
            "ahead": ahead,
            "behind": behind,
            "operation": op,
        });
        serde_json::from_value(wire).expect("branch info in its wire form")
    }

    fn state(branch: BranchInfo) -> BranchWidgetState {
        BranchWidgetState { available: true, branch: Some(branch), repositories: 1, ..BranchWidgetState::default() }
    }

    fn init(cx: &mut TestAppContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
    }

    fn label(widget: &Entity<BranchWidget>, cx: &mut VisualTestContext) -> String {
        widget.update(cx, |widget, cx| widget.label(&mut cx.default_global::<Localization>().0))
    }

    // Not from a spec: the branch, what it tracks and its commits to pull and push; a click asks
    // for the Branches popup.
    #[gpui_kit::test]
    async fn names_the_branch_with_its_counts_and_opens_the_popup(cx: &mut TestAppContext) {
        init(cx);
        let branch = info(Some("main"), Some("origin/main"), 1, 2, None);
        let (widget, cx) = cx.add_window_view(|_, _| BranchWidget::new(state(branch)));
        let events: Rc<RefCell<Vec<BranchWidgetEvent>>> = Rc::default();
        let log = Rc::clone(&events);
        cx.update(|_, cx| {
            cx.subscribe(&widget, move |_, event: &BranchWidgetEvent, _| log.borrow_mut().push(event.clone())).detach();
        });
        cx.run_until_parked();
        assert_eq!(label(&widget, cx), "main");
        widget.update(cx, |widget, cx| {
            assert_eq!(widget.icon(), "branch");
            assert_eq!(widget.counts(), (2, 1));
            assert_eq!(widget.operation_tag(), None);
            let tooltip = widget.tooltip(&mut cx.default_global::<Localization>().0);
            assert_eq!(tooltip, ["Git branch: main", "Tracking origin/main", "2 incoming, 1 outgoing commits"]);
        });
        let bounds = cx.debug_bounds("vcs-branch-widget").expect("the widget is drawn");
        cx.simulate_click(bounds.center(), Modifiers::default());
        assert_eq!(*events.borrow(), [BranchWidgetEvent::TogglePopup]);
    }

    // Not from a spec: a detached HEAD by its short commit, a stopped rebase by what it does, an
    // operation without controls beside the branch.
    #[gpui_kit::test]
    async fn names_a_detached_head_and_an_operation_in_progress(cx: &mut TestAppContext) {
        init(cx);
        let (widget, cx) = cx.add_window_view(|_, _| BranchWidget::new(state(info(None, None, 0, 0, None))));
        assert_eq!(label(&widget, cx), "01234567");
        assert_eq!(widget.read_with(cx, |widget, _| widget.icon().to_owned()), "commit");

        let rebasing = state(info(Some("main"), None, 0, 0, Some("rebase")));
        widget.update(cx, |widget, cx| widget.set_state(rebasing, cx));
        assert_eq!(label(&widget, cx), "Rebasing main");
        widget.update(cx, |widget, cx| {
            let tooltip = widget.tooltip(&mut cx.default_global::<Localization>().0);
            assert_eq!(tooltip, ["Git branch: main (rebase)"]);
        });
        let merging = state(info(None, None, 0, 0, Some("merge")));
        widget.update(cx, |widget, cx| widget.set_state(merging, cx));
        assert_eq!(label(&widget, cx), "Merging");

        let bisecting = state(info(Some("main"), None, 0, 0, Some("bisect")));
        widget.update(cx, |widget, cx| widget.set_state(bisecting, cx));
        assert_eq!(label(&widget, cx), "main");
        let tag = widget.read_with(cx, |widget, _| widget.operation_tag().map(str::to_owned));
        assert_eq!(tag.as_deref(), Some("bisect"));

        widget.update(cx, |widget, cx| widget.set_state(BranchWidgetState::default(), cx));
        assert!(!widget.read_with(cx, |widget, _| widget.is_shown()), "no repository: nothing is drawn");
    }

    // Not from a spec: another feature's presentation replaces the branch's, and with several
    // repositories on different branches the widget names the current one.
    #[gpui_kit::test]
    async fn follows_presentations_and_names_the_repository(cx: &mut TestAppContext) {
        init(cx);
        let presentation =
            BranchPresentation { icon: Some("github".to_owned()), text: "#12 on feature".to_owned(), tooltip: None };
        let mut vcs = VcsState {
            available: true,
            root: Some("/p".to_owned()),
            roots: vec!["/p/a".to_owned(), "/p/b".to_owned()],
            branch: Some(info(Some("main"), Some("origin/main"), 0, 0, None)),
            ..VcsState::default()
        };
        let head = |name: &str| RepoHead { head: Some(name.to_owned()), head_commit: Some("abc".to_owned()) };
        vcs.heads = BTreeMap::from([("/p/a".to_owned(), head("main")), ("/p/b".to_owned(), head("main"))]);

        // On one branch everywhere: the presentation shows, with no repository named.
        let shared = BranchWidgetState::from_vcs(&vcs, Some("/p/b"), Some(&presentation), false);
        assert_eq!(shared.repo, None);
        let (widget, cx) = cx.add_window_view(|_, _| BranchWidget::new(shared));
        assert_eq!(label(&widget, cx), "#12 on feature");
        widget.update(cx, |widget, cx| {
            assert_eq!(widget.icon(), "github");
            let tooltip = widget.tooltip(&mut cx.default_global::<Localization>().0);
            assert_eq!(tooltip, ["Git branch: main", "Tracking origin/main", "In all 2 repositories"]);
        });

        // b is on another branch and holds the active file: its HEAD and name, no presentation.
        vcs.heads.insert("/p/b".to_owned(), head("dev"));
        vcs.operations.insert("/p/b".to_owned(), "merge".to_owned());
        let diverged = BranchWidgetState::from_vcs(&vcs, Some("/p/b"), Some(&presentation), false);
        assert_eq!(diverged.repo.as_deref(), Some("b"));
        widget.update(cx, |widget, cx| widget.set_state(diverged, cx));
        assert_eq!(label(&widget, cx), "Merging dev");
        widget.update(cx, |widget, cx| {
            let tooltip = widget.tooltip(&mut cx.default_global::<Localization>().0);
            assert_eq!(tooltip, ["Repository: b", "Git branch: dev (merge)"]);
        });
    }

    fn merge_widget(
        cx: &mut TestAppContext,
        state: MergeRebaseState,
    ) -> (Entity<MergeRebaseWidget>, &mut VisualTestContext) {
        init(cx);
        cx.add_window_view(|_, _| MergeRebaseWidget::new(state))
    }

    // src/features/vcs/__tests__/mergeRebaseWidget.test.tsx › MergeRebaseWidget › renders nothing without conflicts or
    // an operation
    #[gpui_kit::test]
    async fn renders_nothing_without_conflicts_or_an_operation(cx: &mut TestAppContext) {
        let (widget, cx) = merge_widget(cx, MergeRebaseState::default());
        widget.read_with(cx, |widget, cx| {
            assert!(!widget.is_needed());
            assert!(widget.labels(cx).is_empty());
        });
        assert!(cx.debug_bounds("merge-rebase-0").is_none());
    }

    // src/features/vcs/__tests__/mergeRebaseWidget.test.tsx › MergeRebaseWidget › offers only Resolve for conflicts
    // left without an operation (stash pop)
    #[gpui_kit::test]
    async fn offers_only_resolve_for_conflicts_left_without_an_operation(cx: &mut TestAppContext) {
        let state = MergeRebaseState { conflicts: true, ..MergeRebaseState::default() };
        let (widget, cx) = merge_widget(cx, state);
        assert_eq!(widget.read_with(cx, |widget, cx| widget.labels(cx)), ["Resolve Conflicts…"]);
        let events: Rc<RefCell<Vec<MergeRebaseEvent>>> = Rc::default();
        let log = Rc::clone(&events);
        cx.update(|_, cx| {
            cx.subscribe(&widget, move |_, event: &MergeRebaseEvent, _| log.borrow_mut().push(*event)).detach();
        });
        let bounds = cx.debug_bounds("merge-rebase-0").expect("the button is drawn");
        cx.simulate_click(bounds.center(), Modifiers::default());
        assert_eq!(*events.borrow(), [MergeRebaseEvent::ResolveConflicts]);
    }

    // src/features/vcs/__tests__/mergeRebaseWidget.test.tsx › MergeRebaseWidget › offers Continue and Abort for a
    // rebase with nothing left to resolve
    #[gpui_kit::test]
    async fn offers_continue_and_abort_for_a_rebase_with_nothing_left_to_resolve(cx: &mut TestAppContext) {
        let state = MergeRebaseState { operation: Some("rebase".to_owned()), ..MergeRebaseState::default() };
        let (widget, cx) = merge_widget(cx, state);
        assert_eq!(widget.read_with(cx, |widget, cx| widget.labels(cx)), ["Continue Rebase", "Abort Rebase"]);
        let events: Rc<RefCell<Vec<MergeRebaseEvent>>> = Rc::default();
        let log = Rc::clone(&events);
        cx.update(|_, cx| {
            cx.subscribe(&widget, move |_, event: &MergeRebaseEvent, _| log.borrow_mut().push(*event)).detach();
        });
        let bounds = cx.debug_bounds("merge-rebase-1").expect("Abort is drawn");
        cx.simulate_click(bounds.center(), Modifiers::default());
        assert_eq!(*events.borrow(), [MergeRebaseEvent::Control(InProgress::Rebase, Control::Abort)]);

        // While a push runs the buttons wait.
        let busy = MergeRebaseState { operation: Some("rebase".to_owned()), busy: true, ..MergeRebaseState::default() };
        widget.update(cx, |widget, cx| widget.set_state(busy, cx));
        cx.simulate_click(bounds.center(), Modifiers::default());
        assert_eq!(events.borrow().len(), 1);
        // A merge is concluded by committing: Abort only.
        let merge = MergeRebaseState { operation: Some("merge".to_owned()), ..MergeRebaseState::default() };
        widget.update(cx, |widget, cx| widget.set_state(merge, cx));
        assert_eq!(widget.read_with(cx, |widget, cx| widget.labels(cx)), ["Abort Merge"]);
    }

    // src/features/vcs/__tests__/mergeRebaseWidget.test.tsx › MergeRebaseWidget › translates operation button labels
    #[gpui_kit::test]
    async fn translates_operation_button_labels(cx: &mut TestAppContext) {
        let state = MergeRebaseState { operation: Some("rebase".to_owned()), ..MergeRebaseState::default() };
        let (widget, cx) = merge_widget(cx, state);
        cx.update(|_, cx| cx.default_global::<Localization>().0.set_locale(Some("zh"), "en"));
        assert_eq!(widget.read_with(cx, |widget, cx| widget.labels(cx)), ["继续变基", "中止变基"]);
    }

    // Not from a spec: every design token and icon the widgets draw with exists.
    #[test]
    fn the_design_tokens_the_widgets_use_exist() {
        let theme = theme::ActiveTheme::default().theme;
        let colors =
            ["fg-strong", "fg-muted", "fg-subtle", "fg-faint", "bg-hover", "warning", "accent", "success", "danger"];
        for name in colors {
            assert!(theme.color(name).is_some(), "--{name} is a colour");
        }
        let icons =
            ["branch", "commit", "chevron-down", "vcs-incoming", "vcs-outgoing", "resolve-continue", "vcs-abort"];
        for name in icons {
            assert!(theme::icons::icon_path(name).is_some(), "the {name} icon exists");
        }
    }
}
