//! Version Control › Git: the Git executable with its Test, the commit size warning, the Push
//! dialog, protected branches, the update method, background fetching and the saved HTTPS
//! credentials.

use super::{
    Binding, ControlEvent, MEDIUM_FIELD, Part, Then, WIDE_FIELD, claim_field_keys, fixed_choices, frame, hint_line,
    new_field, row, row_label, toggle,
};
use crate::platform::input;
use crate::settings_view::controls::{button, tx};
use crate::theme;
use crate::ui::text_field::{TextField, TextFieldEvent};
use gpui_kit::{
    App, ClickEvent, Context, Entity, EventEmitter, IntoElement, Keystroke, Render, SharedString, Subscription, Window,
    div, prelude::*, px,
};
use serde_json::Value;
use std::rc::Rc;

pub const PAGE: &str = "vcs.git";
pub const EXECUTABLE_KEY: &str = "vcs.git.executable";

/// The forms of the page, by slot label.
pub fn forms() -> Vec<(&'static str, Vec<Part>)> {
    vec![
        (
            "Warn when committing files larger than",
            vec![Part::ToggleNumber {
                toggle: "vcs.git.warnLargeFiles",
                number: "vcs.git.largeFileMb",
                label: "Warn when committing files larger than",
                unit: Some("MB"),
                min: 1.0,
                max: 100_000.0,
                hint: None,
            }],
        ),
        (
            "Show Push dialog for Commit and Push",
            vec![
                toggle("vcs.git.showPushDialog", "Show Push dialog for Commit and Push").into(),
                toggle(
                    "vcs.git.pushDialogOnlyProtected",
                    "Show Push dialog only when committing to protected branches",
                )
                .gate(super::Gate::On("vcs.git.showPushDialog"))
                .indented()
                .into(),
            ],
        ),
        (
            "Protected branches",
            vec![
                super::field("vcs.git.protectedBranches", "Protected branches", super::FieldKind::Text)
                    .width(MEDIUM_FIELD)
                    .hint(
                        "Force push is not allowed to these branches. Separate patterns with ; (wildcards * and ? are \
                         supported).",
                    )
                    .into(),
            ],
        ),
        (
            "Update method",
            vec![super::radio("vcs.git.updateMethod", "Update method", &[("merge", "Merge"), ("rebase", "Rebase")])],
        ),
        (
            "Explicitly check for incoming commits on remotes",
            vec![Part::Select {
                key: "vcs.git.incomingCheck",
                label: "Explicitly check for incoming commits on remotes",
                choices: fixed_choices(&[("auto", "Auto"), ("always", "Always"), ("never", "Never")]),
                hint: Some(
                    "Fetches remotes in the background so incoming commits show in the Branches popup. Auto stops \
                     after an authentication failure."
                        .into(),
                ),
                beside: None,
                indent: false,
            }],
        ),
    ]
}

/// What `git --version` said, as the page shows it: "Git version is 2.44.0 (/usr/bin/git)".
pub fn version_line(version: &str, path: &str) -> String {
    let version = version.trim();
    let shown = match version.strip_prefix("git version") {
        Some(rest) => format!("Git version is{rest}"),
        None => version.to_owned(),
    };
    format!("{shown} ({path})")
}

/// Path to Git executable: the path (empty: the one found on the PATH, named by the placeholder)
/// and Test, which runs it and says which version answered or why it failed.
pub struct GitExecutableView {
    binding: Binding,
    field: Entity<TextField>,
    result: Option<Result<String, String>>,
    testing: bool,
    claim: u64,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ControlEvent> for GitExecutableView {}

impl GitExecutableView {
    /// `detected`: the executable found on the PATH.
    pub fn new(binding: Binding, detected: Option<&str>, cx: &mut Context<Self>) -> Self {
        let placeholder = Self::placeholder(detected, cx);
        let text = binding.text(EXECUTABLE_KEY, cx);
        let field = new_field(&text, Some(placeholder), cx);
        let events = cx.subscribe(&field, |view: &mut Self, field, event: &TextFieldEvent, cx| match event {
            TextFieldEvent::Changed => {
                let text = field.read(cx).text().to_owned();
                view.result = None;
                view.binding.stage(EXECUTABLE_KEY, Value::String(text), cx);
                cx.notify();
            }
            TextFieldEvent::Submitted => cx.emit(ControlEvent::Submit),
            TextFieldEvent::Cancelled => cx.emit(ControlEvent::Cancel),
        });
        let claim = claim_field_keys(&field, |_: &mut Self, keystroke: &Keystroke, cx| enter_escape(keystroke, cx), cx);
        let observe = cx.observe(&binding.model, |view: &mut Self, _, cx| {
            let wanted = view.binding.text(EXECUTABLE_KEY, cx);
            if view.field.read(cx).text() != wanted {
                view.field.update(cx, |field, cx| field.set_text(&wanted, cx));
            }
            cx.notify();
        });
        cx.on_release(|view: &mut Self, cx: &mut App| input::release_keys(view.claim, cx)).detach();
        Self { binding, field, result: None, testing: false, claim, _subscriptions: vec![events, observe] }
    }

    /// What the empty field says: the executable found on the PATH, once it is known.
    fn placeholder(detected: Option<&str>, cx: &mut App) -> String {
        match detected {
            Some(path) => {
                let localization = cx.default_global::<crate::platform::i18n::Localization>();
                localization
                    .0
                    .tx_built(&["Auto-detected: ", ""], &[crate::platform::i18n::Param::Text(path.to_owned())])
            }
            None => tx(cx, "Auto-detected"),
        }
    }

    /// The executable found on the PATH, looked up off the window's thread.
    pub fn set_detected(&mut self, detected: Option<&str>, cx: &mut Context<Self>) {
        let placeholder = Self::placeholder(detected, cx);
        self.field.update(cx, |field, cx| field.set_placeholder(placeholder, cx));
    }

    pub fn field(&self) -> &Entity<TextField> {
        &self.field
    }

    pub fn is_testing(&self) -> bool {
        self.testing
    }

    /// The last Test's answer: the version line, or why it failed.
    pub fn result(&self) -> Option<&Result<String, String>> {
        self.result.as_ref()
    }

    pub fn set_result(&mut self, result: Result<String, String>, cx: &mut Context<Self>) {
        (self.result, self.testing) = (Some(result), false);
        cx.notify();
    }

    /// Test: runs the executable in the field.
    pub fn test(&mut self, cx: &mut Context<Self>) {
        if self.testing {
            return;
        }
        (self.testing, self.result) = (true, None);
        let path = self.field.read(cx).text().to_owned();
        let view = cx.weak_entity();
        let then: Then<Result<String, String>> = Rc::new(move |result, cx: &mut App| {
            view.update(cx, |view, cx| view.set_result(result, cx)).ok();
        });
        cx.emit(ControlEvent::TestGit { path, then });
        cx.notify();
    }
}

/// Enter is the dialog's OK and Escape its Cancel.
fn enter_escape<V: EventEmitter<ControlEvent>>(keystroke: &Keystroke, cx: &mut Context<V>) -> bool {
    if keystroke.modifiers.modified() {
        return false;
    }
    match keystroke.key.as_str() {
        "enter" => cx.emit(ControlEvent::Submit),
        "escape" => cx.emit(ControlEvent::Cancel),
        _ => return false,
    }
    true
}

impl Render for GitExecutableView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        const LABEL: &str = "Path to Git executable";
        let query = self.binding.query(cx);
        let matched = self.binding.matches(LABEL, cx);
        let label = tx(cx, LABEL);
        let test_label = tx(cx, "Test");
        let enabled = !self.testing;
        let test = div()
            .id("settings-git-test")
            .debug_selector(|| "settings-git-test".to_owned())
            .child(button(cx, &test_label, false, enabled))
            .when(enabled, |el| el.on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.test(cx))));
        let result = self.result.as_ref().map(|result| {
            let (color, text, selector) = match result {
                Ok(text) => ("success", text.clone(), "settings-git-ok"),
                Err(text) => ("danger", text.clone(), "settings-git-error"),
            };
            hint_line(cx, &text, super::LABEL_WIDTH)
                .debug_selector(move || selector.to_owned())
                .text_color(theme::color(cx, color))
        });
        div()
            .flex()
            .flex_col()
            .child(
                row(cx, matched)
                    .child(row_label(cx, &label, &query))
                    .child(
                        frame(cx, WIDE_FIELD, false)
                            .id("settings-text-vcs.git.executable")
                            .debug_selector(|| "settings-text-vcs.git.executable".to_owned())
                            .child(self.field.clone()),
                    )
                    .child(test),
            )
            .children(result)
    }
}

/// Passwords: the hosts with a saved HTTPS credential, each with Forget.
pub struct GitCredentialsView {
    binding: Binding,
    hosts: Vec<String>,
}

impl EventEmitter<ControlEvent> for GitCredentialsView {}

impl GitCredentialsView {
    pub fn new(binding: Binding, hosts: Vec<String>) -> Self {
        Self { binding, hosts }
    }

    pub fn hosts(&self) -> &[String] {
        &self.hosts
    }

    pub fn set_hosts(&mut self, hosts: Vec<String>, cx: &mut Context<Self>) {
        self.hosts = hosts;
        cx.notify();
    }

    /// Forget: the credential of `key` goes.
    pub fn forget(&mut self, key: &str, cx: &mut Context<Self>) {
        let view = cx.weak_entity();
        let then: Then<Vec<String>> = Rc::new(move |hosts, cx: &mut App| {
            view.update(cx, |view, cx| view.set_hosts(hosts, cx)).ok();
        });
        cx.emit(ControlEvent::ForgetCredential { key: key.to_owned(), then });
    }

    /// The control as text: `Passwords: No saved credentials`, or a line per host.
    pub fn lines(&self, cx: &App) -> Vec<String> {
        if self.hosts.is_empty() {
            return vec![format!("{}: {}", tx(cx, "Passwords"), tx(cx, "No saved credentials"))];
        }
        let forget = tx(cx, "Forget");
        let hosts = self.hosts.iter().map(|host| format!("  {host} [{forget}]"));
        std::iter::once(format!("{}:", tx(cx, "Passwords"))).chain(hosts).collect()
    }
}

impl Render for GitCredentialsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        const LABEL: &str = "Passwords";
        let query = self.binding.query(cx);
        let matched = self.binding.matches(LABEL, cx);
        let label = tx(cx, LABEL);
        if self.hosts.is_empty() {
            let empty = tx(cx, "No saved credentials");
            let hint = tx(
                cx,
                "Credentials the IDE answers git's HTTPS prompts with. Saved after a successful fetch / pull / push \
                 when \"Remember for this host\" was checked.",
            );
            return div()
                .flex()
                .flex_col()
                .child(
                    row(cx, matched)
                        .debug_selector(|| "settings-git-no-credentials".to_owned())
                        .child(row_label(cx, &label, &query))
                        .child(super::muted(cx, &empty)),
                )
                .child(hint_line(cx, &hint, super::LABEL_WIDTH))
                .into_any_element();
        }
        let forget = tx(cx, "Forget");
        let rows: Vec<_> = self
            .hosts
            .clone()
            .into_iter()
            .enumerate()
            .map(|(index, host)| {
                let key = host.clone();
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.0))
                    .px(px(6.0))
                    .h(px(super::ROW_HEIGHT))
                    .child(div().font_family("monospace").child(SharedString::from(host)))
                    .child(
                        div()
                            .id(SharedString::from(format!("settings-git-forget-{index}")))
                            .debug_selector(move || format!("settings-git-forget-{index}"))
                            .child(button(cx, &forget, false, true))
                            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.forget(&key, cx))),
                    )
                    .into_any_element()
            })
            .collect();
        let hint = tx(
            cx,
            "Credentials the IDE answers git's HTTPS prompts with, stored in the app's own secret file (not git's \
             credential.helper). A rejected credential is forgotten automatically.",
        );
        div()
            .flex()
            .flex_col()
            .child(row(cx, matched).child(row_label(cx, &label, &query)))
            .child(
                div()
                    .ml(px(4.0))
                    .max_w(px(560.0))
                    .rounded(theme::length(cx, "radius-small"))
                    .border_1()
                    .border_color(theme::color(cx, "border"))
                    .children(rows),
            )
            .child(hint_line(cx, &hint, 4.0))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;
    use crate::settings_model::staging::Scope;
    use gpui_kit::TestAppContext;

    // Not from a spec: git's answer reads "Git version is …" with the executable that answered.
    #[test]
    fn the_version_line_names_the_version_and_the_executable() {
        assert_eq!(version_line("git version 2.44.0\n", "/usr/bin/git"), "Git version is 2.44.0 (/usr/bin/git)");
        assert_eq!(version_line("2.44", "git"), "2.44 (git)");
    }

    // Not from a spec: a path typed is staged and clears the last result; Test asks for a run
    // with the path and shows the answer.
    #[gpui_kit::test]
    async fn test_runs_the_path_typed_and_shows_the_answer(cx: &mut TestAppContext) {
        let model = cx.update(|cx| dialog_model(PAGE, &[], cx));
        let binding = Binding::fixed(model.clone(), PAGE, Scope::Global);
        let (view, seen, cx) = open(cx, move |_, cx| GitExecutableView::new(binding, Some("/usr/bin/git"), cx));
        let field = view.read_with(cx, |view, _| view.field().clone());
        type_over(&field, "/opt/git/bin/git", cx);
        assert_eq!(staged(&model, cx), [("global:vcs.git.executable".to_owned(), "\"/opt/git/bin/git\"".to_owned())]);
        click(cx, "settings-git-test");
        assert_eq!(seen.borrow().as_slice(), ["test-git /opt/git/bin/git"]);
        assert!(view.read_with(cx, |view, _| view.is_testing()));
        view.update(cx, |view, cx| view.set_result(Err("No such file".to_owned()), cx));
        assert_eq!(view.read_with(cx, |view, _| view.result().cloned()), Some(Err("No such file".to_owned())));
        type_over(&field, "git", cx);
        assert_eq!(view.read_with(cx, |view, _| view.result().cloned()), None, "an edit clears the result");
    }

    // Not from a spec: Forget asks for the host's credential to go and shows the hosts left.
    #[gpui_kit::test]
    async fn forget_asks_for_the_credential_to_go_and_shows_the_hosts_left(cx: &mut TestAppContext) {
        let model = cx.update(|cx| dialog_model(PAGE, &[], cx));
        let binding = Binding::fixed(model, PAGE, Scope::Global);
        let hosts = vec!["https://a.example".to_owned(), "https://b.example".to_owned()];
        let (view, seen, cx) = open(cx, move |_, _| GitCredentialsView::new(binding, hosts));
        click(cx, "settings-git-forget-0");
        assert_eq!(seen.borrow().as_slice(), ["forget https://a.example"]);
        view.update(cx, |view, cx| view.set_hosts(vec!["https://b.example".to_owned()], cx));
        assert_eq!(view.read_with(cx, |view, cx| view.lines(cx)), ["Passwords:", "  https://b.example [Forget]"]);
        view.update(cx, |view, cx| view.set_hosts(Vec::new(), cx));
        assert_eq!(view.read_with(cx, |view, cx| view.lines(cx)), ["Passwords: No saved credentials"]);
    }
}
