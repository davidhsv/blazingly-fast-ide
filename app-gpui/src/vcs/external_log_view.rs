//! The editor tab of a Git log of repositories outside the project (Open External Git Log…): their
//! commits, newest first across the repositories, read-only.
//!
//! The view reads nothing itself: [`ExternalLogView::load`] asks its window for the commits
//! ([`ExternalLogEvent::Load`]), which answers with [`ExternalLogView::set_commits`].

use crate::editor::tabs::EditorInput;
use crate::platform::date_format::DateFormatOptions;
use crate::platform::i18n::Localization;
use crate::platform::input;
use crate::platform::local_time::{now_ms, utc_offset_minutes};
use crate::theme;
use crate::vcs::changes::base_name;
use crate::vcs_diff::model::format_time;
use gpui_kit::{
    App, ClickEvent, Context, EventEmitter, FocusHandle, Focusable, IntoElement, Keystroke, Render, SharedString,
    Window, div, prelude::*, px,
};
use ide_vcs::Commit;
use serde_json::{Value, json};

/// The editor kind of Git log tabs.
pub const GIT_LOG_KIND: &str = "git-log";

/// The tab input of the log of the repositories `roots`: one tab per set of repositories, whatever
/// their order, titled with their roots.
pub fn external_log_input(roots: &[String]) -> EditorInput {
    let mut sorted = roots.to_vec();
    sorted.sort();
    EditorInput {
        kind: GIT_LOG_KIND.to_owned(),
        key: format!("git-log:external:{}", sorted.join("\n")),
        path: None,
        data: Some(json!({ "label": roots.join(", "), "repositories": roots })),
    }
}

/// The repositories a log tab shows; `None` for another kind of tab or one without repositories.
pub fn external_log_roots(input: &EditorInput) -> Option<Vec<String>> {
    if input.kind != GIT_LOG_KIND {
        return None;
    }
    let listed = input.data.as_ref()?.get("repositories")?.as_array()?;
    let roots: Vec<String> = listed.iter().filter_map(Value::as_str).map(str::to_owned).collect();
    (!roots.is_empty()).then_some(roots)
}

/// The tab's title: the roots of its repositories.
pub fn external_log_title(input: &EditorInput) -> String {
    let label = input.data.as_ref().and_then(|data| data.get("label")).and_then(Value::as_str);
    label.map_or_else(|| "Git Log".to_owned(), str::to_owned)
}

pub fn external_log_icon(_: &EditorInput) -> String {
    "toolwindow-git".to_owned()
}

/// A commit as the list draws it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogRow {
    pub summary: String,
    pub short_id: String,
    pub author: String,
    pub time: String,
    /// The repository's folder name, when the tab shows several.
    pub repo: Option<String>,
    pub active: bool,
}

/// What the view asks of its window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExternalLogEvent {
    /// Read the commits of these repositories (`vcs_log` of each) and call
    /// [`ExternalLogView::set_commits`].
    Load(Vec<String>),
}

enum Loaded {
    Loading,
    Failed(String),
    Shown(Vec<LogRow>),
}

pub struct ExternalLogView {
    roots: Vec<String>,
    loaded: Loaded,
    active: usize,
    focus: FocusHandle,
    key_claim: u64,
}

impl EventEmitter<ExternalLogEvent> for ExternalLogView {}

impl ExternalLogView {
    /// The log of `roots`; nothing is read until [`Self::load`].
    pub fn new(roots: Vec<String>, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let claimant = cx.weak_entity();
        let key_claim = input::claim_keys(
            focus.clone(),
            move |keystroke, _, cx| claimant.update(cx, |view, cx| view.claim_key(keystroke, cx)).unwrap_or(false),
            cx,
        );
        cx.on_release(|view, cx| input::release_keys(view.key_claim, cx)).detach();
        Self { roots, loaded: Loaded::Loading, active: 0, focus, key_claim }
    }

    /// Asks for the commits (again).
    pub fn load(&mut self, cx: &mut Context<Self>) {
        cx.emit(ExternalLogEvent::Load(self.roots.clone()));
    }

    /// The commits read, each with its repository's root, or why they could not be read.
    pub fn set_commits(&mut self, commits: Result<Vec<(String, Commit)>, String>, cx: &mut Context<Self>) {
        self.loaded = match commits {
            Err(error) => Loaded::Failed(error),
            Ok(mut commits) => {
                // Newest first across the repositories; a stable sort keeps each one's own order.
                commits.sort_by(|(_, a), (_, b)| b.time_ms.total_cmp(&a.time_ms));
                let several = self.roots.len() > 1;
                let i18n = &mut cx.default_global::<Localization>().0;
                let now = now_ms();
                let offset = utc_offset_minutes(now);
                let rows = commits
                    .into_iter()
                    .map(|(root, commit)| LogRow {
                        time: format_time(Some(commit.time_ms), now, offset, &DateFormatOptions::default(), i18n),
                        summary: commit.summary,
                        short_id: commit.short_id,
                        author: commit.author,
                        repo: several.then(|| base_name(&root).to_owned()),
                        active: false,
                    })
                    .collect();
                Loaded::Shown(rows)
            }
        };
        self.active = 0;
        cx.notify();
    }

    pub fn roots(&self) -> &[String] {
        &self.roots
    }

    pub fn is_loading(&self) -> bool {
        matches!(self.loaded, Loaded::Loading)
    }

    pub fn error(&self) -> Option<&str> {
        match &self.loaded {
            Loaded::Failed(error) => Some(error),
            Loaded::Loading | Loaded::Shown(_) => None,
        }
    }

    /// The commits as the list draws them; none until they are read.
    pub fn rows(&self) -> Vec<LogRow> {
        match &self.loaded {
            Loaded::Shown(rows) => rows
                .iter()
                .enumerate()
                .map(|(index, row)| LogRow { active: index == self.active, ..row.clone() })
                .collect(),
            Loaded::Loading | Loaded::Failed(_) => Vec::new(),
        }
    }

    fn len(&self) -> usize {
        match &self.loaded {
            Loaded::Shown(rows) => rows.len(),
            Loaded::Loading | Loaded::Failed(_) => 0,
        }
    }

    /// Makes row `index` the selected one.
    pub fn select(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.len() {
            self.active = index;
            cx.notify();
        }
    }

    /// ↑, ↓, Home and End move the selection.
    fn claim_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        if keystroke.modifiers.modified() {
            return false;
        }
        let len = self.len();
        let next = match keystroke.key.as_str() {
            "down" if self.active + 1 < len => self.active + 1,
            "up" if self.active > 0 => self.active - 1,
            "down" | "up" => self.active,
            "home" => 0,
            "end" => len.saturating_sub(1),
            _ => return false,
        };
        self.select(next, cx);
        true
    }

    fn render_row(&self, index: usize, row: &LogRow, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let muted = theme::color(cx, "fg-muted");
        let selector = format!("external-log-row-{index}");
        div()
            .id(SharedString::from(selector.clone()))
            .debug_selector(move || selector.clone())
            .flex_none()
            .h(px(24.0))
            .px(px(10.0))
            .flex()
            .items_center()
            .gap(px(12.0))
            .overflow_hidden()
            .whitespace_nowrap()
            .when(row.active, |el| el.bg(theme::color(cx, "bg-selected")))
            .hover(|el| el.bg(theme::color(cx, "bg-hover")))
            .children(row.repo.clone().map(|repo| div().flex_none().text_color(muted).child(SharedString::from(repo))))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .text_color(theme::color(cx, "fg"))
                    .child(SharedString::from(row.summary.clone())),
            )
            .child(div().flex_none().text_color(muted).child(SharedString::from(row.author.clone())))
            .child(div().flex_none().text_color(muted).child(SharedString::from(row.time.clone())))
            .child(div().flex_none().text_color(muted).child(SharedString::from(row.short_id.clone())))
            .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| {
                window.focus(&view.focus, cx);
                view.select(index, cx);
            }))
    }
}

impl Focusable for ExternalLogView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ExternalLogView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = theme::color(cx, "fg-muted");
        let message = |text: String| div().p(px(12.0)).text_color(muted).child(SharedString::from(text));
        let body = match &self.loaded {
            Loaded::Failed(error) => message(error.clone()).into_any_element(),
            Loaded::Loading => message(cx.default_global::<Localization>().0.tx("Loading…")).into_any_element(),
            Loaded::Shown(rows) if rows.is_empty() => {
                message(cx.default_global::<Localization>().0.tx("No commits")).into_any_element()
            }
            Loaded::Shown(_) => {
                let rows = self.rows();
                let drawn: Vec<_> =
                    rows.iter().enumerate().map(|(index, row)| self.render_row(index, row, cx)).collect();
                div()
                    .id("external-log-list")
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .overflow_y_scroll()
                    .children(drawn)
                    .into_any_element()
            }
        };
        div()
            .debug_selector(|| "external-log".to_owned())
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            .bg(theme::color(cx, "bg-app"))
            .text_size(theme::length(cx, "fs-ui"))
            .child(
                div()
                    .h(theme::length(cx, "h-header"))
                    .flex_none()
                    .flex()
                    .items_center()
                    .px(px(10.0))
                    .border_b_1()
                    .border_color(theme::color(cx, "border"))
                    .text_color(theme::color(cx, "fg-strong"))
                    .child(SharedString::from(self.roots.join(", "))),
            )
            .child(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{Entity, Modifiers, TestAppContext, VisualTestContext};
    use std::cell::RefCell;
    use std::rc::Rc;

    type Events = Rc<RefCell<Vec<ExternalLogEvent>>>;

    fn commit(id: &str, summary: &str, time_ms: f64) -> Commit {
        let value = json!({
            "id": id.repeat(8), "shortId": id.repeat(7), "parents": [], "author": "Ada", "email": "ada@example.test",
            "timeMs": time_ms, "summary": summary, "message": summary, "refs": [],
        });
        serde_json::from_value(value).expect("a commit")
    }

    fn open(cx: &mut TestAppContext, roots: Vec<String>) -> (Entity<ExternalLogView>, Events, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let (view, cx) = cx.add_window_view(move |_, cx| ExternalLogView::new(roots, cx));
        let events: Events = Rc::default();
        let seen = Rc::clone(&events);
        cx.update(|_, cx| {
            cx.subscribe(&view, move |_, event: &ExternalLogEvent, _| seen.borrow_mut().push(event.clone())).detach();
        });
        view.update(cx, |view, cx| view.load(cx));
        cx.run_until_parked();
        (view, events, cx)
    }

    #[gpui_kit::test]
    async fn lists_the_commits_of_every_repository_newest_first_with_the_repository_named(cx: &mut TestAppContext) {
        let roots = vec!["/x/one".to_owned(), "/x/two".to_owned()];
        let (view, events, cx) = open(cx, roots.clone());
        assert_eq!(events.borrow().as_slice(), [ExternalLogEvent::Load(roots)]);
        let commits = vec![
            ("/x/one".to_owned(), commit("a", "older in one", 1_000.0)),
            ("/x/two".to_owned(), commit("b", "newest in two", 3_000.0)),
            ("/x/one".to_owned(), commit("c", "newer in one", 2_000.0)),
        ];
        view.update(cx, |view, cx| view.set_commits(Ok(commits), cx));
        let rows = view.read_with(cx, |view, _| view.rows());
        let shown: Vec<(&str, Option<&str>)> =
            rows.iter().map(|row| (row.summary.as_str(), row.repo.as_deref())).collect();
        assert_eq!(
            shown,
            [("newest in two", Some("two")), ("newer in one", Some("one")), ("older in one", Some("one"))]
        );
        assert_eq!(rows[0].short_id, "bbbbbbb");
        assert!(rows[0].active);

        let centre = cx.debug_bounds("external-log-row-2").expect("the row is drawn").center();
        cx.simulate_click(centre, Modifiers::default());
        cx.simulate_keystrokes("up");
        let active: Vec<bool> = view.read_with(cx, |view, _| view.rows().into_iter().map(|row| row.active).collect());
        assert_eq!(active, [false, true, false]);
    }

    #[gpui_kit::test]
    async fn a_log_that_cannot_be_read_says_why(cx: &mut TestAppContext) {
        let (view, _events, cx) = open(cx, vec!["/x/one".to_owned()]);
        assert!(view.read_with(cx, |view, _| view.is_loading()));
        view.update(cx, |view, cx| view.set_commits(Err("fatal: not a git repository".to_owned()), cx));
        let error = view.read_with(cx, |view, _| view.error().map(str::to_owned));
        assert_eq!(error.as_deref(), Some("fatal: not a git repository"));
        assert!(view.read_with(cx, |view, _| view.rows().is_empty()));
    }

    #[test]
    fn one_tab_shows_one_set_of_repositories_whatever_their_order() {
        let one = external_log_input(&["/x/b".to_owned(), "/x/a".to_owned()]);
        let other = external_log_input(&["/x/a".to_owned(), "/x/b".to_owned()]);
        assert_eq!(one.key, other.key);
        assert_eq!(external_log_title(&one), "/x/b, /x/a");
        assert_eq!(external_log_roots(&one), Some(vec!["/x/b".to_owned(), "/x/a".to_owned()]));
        assert_eq!(external_log_roots(&EditorInput { kind: "file".to_owned(), ..one }), None);
    }
}
