//! The notice above an editor whose file has unresolved merge conflicts, as IntelliJ shows it ("File
//! has unresolved merge conflicts" with Resolve conflicts…). It follows Git's own record of the
//! conflict (the file's status), so it appears whatever the file's text holds and whether or not a
//! merge, rebase or unstash is still in progress, and goes away once the file is resolved.
//!
//! [`ConflictBanner`] owns no backend: the editor hands it the repository status whenever that
//! changes, and Resolve conflicts… asks the workbench to open the merge editor for the file.

use crate::platform::i18n::Localization;
use crate::theme;
use crate::vcs::changes::{EffectiveKind, FileStatus, effective_kind, strip_slash};
use gpui_kit::{
    App, ClickEvent, Context, EventEmitter, IntoElement, Render, SharedString, Window, div, prelude::*, px,
};

/// English text of the banner.
pub const CONFLICT_TEXT: &str = "File has unresolved merge conflicts";
/// English label of its one action.
pub const RESOLVE_LABEL: &str = "Resolve conflicts…";

/// How a banner is drawn: a warning has the warning colours and icon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BannerLevel {
    Info,
    Warning,
}

/// What the banner shows: its level, its English text and the English labels of its actions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BannerSpec {
    pub level: BannerLevel,
    pub text: &'static str,
    pub actions: Vec<&'static str>,
}

/// The banner above the editor of `path`, given the repository status: a warning with Resolve
/// conflicts… while Git records the file as conflicted, nothing otherwise.
pub fn conflict_banner(path: &str, status: &[FileStatus]) -> Option<BannerSpec> {
    let path = strip_slash(path);
    let conflicted = status
        .iter()
        .find(|file| strip_slash(&file.path) == path)
        .is_some_and(|file| effective_kind(file) == Some(EffectiveKind::Conflicted));
    conflicted.then(|| BannerSpec { level: BannerLevel::Warning, text: CONFLICT_TEXT, actions: vec![RESOLVE_LABEL] })
}

/// What the banner asks of the workbench.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConflictBannerEvent {
    /// Resolve conflicts…: open the merge editor for this file.
    Resolve { path: String },
}

/// The conflict banner of one editor.
pub struct ConflictBanner {
    path: String,
    banner: Option<BannerSpec>,
}

impl EventEmitter<ConflictBannerEvent> for ConflictBanner {}

impl ConflictBanner {
    /// The banner of the editor of `path`, hidden until a status says the file is conflicted.
    pub fn new(path: &str, status: &[FileStatus]) -> Self {
        Self { path: path.to_owned(), banner: conflict_banner(path, status) }
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    /// The editor now shows another file.
    pub fn set_path(&mut self, path: &str, status: &[FileStatus], cx: &mut Context<Self>) {
        self.path = path.to_owned();
        self.set_status(status, cx);
    }

    /// The repository status changed.
    pub fn set_status(&mut self, status: &[FileStatus], cx: &mut Context<Self>) {
        let banner = conflict_banner(&self.path, status);
        if banner != self.banner {
            self.banner = banner;
            cx.notify();
        }
    }

    /// What the banner shows; `None` while it is hidden.
    pub fn banner(&self) -> Option<&BannerSpec> {
        self.banner.as_ref()
    }

    /// Runs the action at `index` of the banner.
    pub fn press(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.banner.as_ref().is_some_and(|banner| index < banner.actions.len()) {
            cx.emit(ConflictBannerEvent::Resolve { path: self.path.clone() });
        }
    }
}

fn tx(cx: &mut App, text: &str) -> String {
    cx.default_global::<Localization>().0.tx(text)
}

impl Render for ConflictBanner {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(banner) = self.banner.clone() else {
            return div().debug_selector(|| "conflict-banner-hidden".to_owned());
        };
        let warning = banner.level == BannerLevel::Warning;
        let (background, border) = if warning {
            ("bg-feedback-warning", "border-feedback-warning")
        } else {
            ("bg-editor-banner", "border-editor-tabs")
        };
        let icon = theme::icon(cx, if warning { "warning" } else { "info" }, 14.0, theme::color(cx, "fg-muted"));
        let text = tx(cx, banner.text);
        let actions: Vec<_> = banner
            .actions
            .iter()
            .enumerate()
            .map(|(index, label)| {
                let label = tx(cx, label);
                let selector = format!("conflict-banner-action-{index}");
                div()
                    .id(SharedString::from(selector.clone()))
                    .debug_selector(move || selector.clone())
                    .flex_none()
                    .cursor_pointer()
                    .text_color(theme::color(cx, "link"))
                    .child(SharedString::from(label))
                    .on_click(cx.listener(move |banner, _: &ClickEvent, _, cx| banner.press(index, cx)))
            })
            .collect();
        div()
            .debug_selector(|| "conflict-banner".to_owned())
            .flex_none()
            .w_full()
            .min_h(px(36.0))
            .px(px(12.0))
            .flex()
            .items_center()
            .gap(px(8.0))
            .bg(theme::color(cx, background))
            .border_b_1()
            .border_color(theme::color(cx, border))
            .text_color(theme::color(cx, "fg-strong"))
            .text_size(theme::length(cx, "fs-ui"))
            .child(icon)
            .child(div().flex_none().whitespace_nowrap().child(SharedString::from(text)))
            .child(div().flex_1())
            .children(actions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vcs::changes::ChangeKind;
    use gpui_kit::{Entity, Modifiers, TestAppContext, VisualTestContext};
    use std::cell::RefCell;
    use std::rc::Rc;

    fn status(path: &str, unstaged: ChangeKind) -> Vec<FileStatus> {
        let rel_path = path.trim_start_matches("/p/").to_owned();
        vec![FileStatus { path: path.to_owned(), rel_path, staged: None, unstaged: Some(unstaged), renamed_from: None }]
    }

    // src/features/vcs/__tests__/conflictBanner.test.tsx › the conflict banner › warns above a file Git records as
    // conflicted, and offers to resolve it
    #[test]
    fn warns_above_a_file_git_records_as_conflicted_and_offers_to_resolve_it() {
        let banner = conflict_banner("/p/settings.gradle", &status("/p/settings.gradle", ChangeKind::Conflicted));
        let expected = BannerSpec {
            level: BannerLevel::Warning,
            text: "File has unresolved merge conflicts",
            actions: vec!["Resolve conflicts…"],
        };
        assert_eq!(banner, Some(expected));
    }

    // src/features/vcs/__tests__/conflictBanner.test.tsx › the conflict banner › shows nothing above other files, and
    // goes away when the conflict is resolved
    #[test]
    fn shows_nothing_above_other_files_and_goes_away_when_the_conflict_is_resolved() {
        let conflicted = status("/p/settings.gradle", ChangeKind::Conflicted);
        assert_eq!(conflict_banner("/p/other.gradle", &conflicted), None);
        assert!(conflict_banner("/p/settings.gradle", &conflicted).is_some());
        assert_eq!(conflict_banner("/p/settings.gradle", &status("/p/settings.gradle", ChangeKind::Modified)), None);
    }

    type Events = Rc<RefCell<Vec<ConflictBannerEvent>>>;

    fn open<'a>(
        cx: &'a mut TestAppContext,
        path: &str,
        files: Vec<FileStatus>,
    ) -> (Entity<ConflictBanner>, Events, &'a mut VisualTestContext) {
        cx.update(|cx| cx.set_global(theme::ActiveTheme::default()));
        let path = path.to_owned();
        let (banner, cx) = cx.add_window_view(move |_, _| ConflictBanner::new(&path, &files));
        let events: Events = Rc::default();
        let seen = Rc::clone(&events);
        cx.update(|_, cx| {
            cx.subscribe(&banner, move |_, event: &ConflictBannerEvent, _| seen.borrow_mut().push(event.clone()))
                .detach();
        });
        cx.run_until_parked();
        (banner, events, cx)
    }

    // The banner shows its text and link above a conflicted file, asks for the merge editor when the
    // link is clicked, and hides once the status says the file is resolved.
    #[gpui_kit::test]
    async fn opens_the_merge_editor_from_its_link_and_hides_once_resolved(cx: &mut TestAppContext) {
        let path = "/p/settings.gradle";
        let (banner, events, cx) = open(cx, path, status(path, ChangeKind::Conflicted));
        assert!(cx.debug_bounds("conflict-banner").is_some());
        let link = cx.debug_bounds("conflict-banner-action-0").expect("the link is drawn").center();
        cx.simulate_click(link, Modifiers::default());
        cx.run_until_parked();
        assert_eq!(*events.borrow(), [ConflictBannerEvent::Resolve { path: "/p/settings.gradle".to_owned() }]);

        banner.update(cx, |banner, cx| banner.set_status(&status("/p/settings.gradle", ChangeKind::Modified), cx));
        cx.run_until_parked();
        assert!(banner.read_with(cx, |banner, _| banner.banner().is_none()));
        assert!(cx.debug_bounds("conflict-banner").is_none());
    }
}
