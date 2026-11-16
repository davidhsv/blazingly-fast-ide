//! Select Folder on Target: the project folder of an SSH target chosen among the folders of its
//! server, listed over SFTP with the target's SSH configuration. The browser opens under the
//! Project folder row on the folder typed (`~`, the login folder, while it is empty); `~` and
//! relative paths turn absolute once a listing tells where they are. A folder is selected with a
//! click and opened with a double click; the path can be typed (Enter lists it), the parent
//! opened and the listing read again. OK takes the folder selected, else the one listed.
//!
//! A listing that fails on an unknown host key offers Trust Host; one that fails for a missing or
//! wrong password or passphrase offers a [`SecretField`] and Connect, which hands what is typed to
//! the backend for this session (it is never staged or saved here) and lists again. The secret
//! leaves the view only as a [`SecretText`] and is cleared once it is handed over.

use super::RunTargetsView;
use crate::database::store::SecretText;
use crate::platform::collate;
use crate::settings_model::run_targets::parent_path;
use crate::settings_view::controls::{button, tx};
use crate::settings_view::pages::{Then, WIDE_FIELD, frame, icon_button, new_field};
use crate::theme;
use crate::ui::secret_field::{BULLET, SecretField, SecretFieldEvent};
use crate::ui::text_field::{TextField, TextFieldEvent};
use gpui_kit::{
    AnyElement, App, AppContext, ClickEvent, Context, Entity, IntoElement, SharedString, Subscription, div, prelude::*,
    px,
};
use ide_core::IdeError;
use ide_ssh::RemoteEntry;
use std::rc::Rc;

const TITLE: &str = "Select Folder on Target";

/// The listing failed on a host key the user has not trusted yet.
pub fn needs_host_trust(error: &str) -> bool {
    error.starts_with("Unknown SSH host key")
}

/// The listing failed for a missing or wrong password or key passphrase.
pub fn needs_secret(error: &str) -> bool {
    error.starts_with("SSH password required")
        || error.starts_with("SSH key passphrase required")
        || error.contains("passphrase required")
        || error.starts_with("cannot decrypt SSH key")
}

/// The open browser.
pub(super) struct FolderBrowser {
    /// The SSH configuration listed with.
    ssh: String,
    /// The folder shown: the one asked for, or where it turned out to be.
    path: String,
    /// The folder last asked for.
    listed: String,
    path_field: Entity<TextField>,
    /// The subfolders of the folder shown, by name; none while they are read.
    entries: Option<Vec<RemoteEntry>>,
    error: Option<String>,
    selected: Option<String>,
    secret: Entity<SecretField>,
    /// Counts the listings asked for: an answer to an earlier one is dropped.
    request: u64,
    _subscriptions: Vec<Subscription>,
}

/// What the browser shows below its path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderState {
    Loading,
    /// The names of the subfolders.
    Listed(Vec<String>),
    Failed(String),
}

impl RunTargetsView {
    pub fn folder_open(&self) -> bool {
        self.folder.is_some()
    }

    /// The folder the browser shows.
    pub fn folder_path(&self) -> Option<&str> {
        self.folder.as_ref().map(|browser| browser.path.as_str())
    }

    pub fn folder_path_field(&self) -> Option<&Entity<TextField>> {
        self.folder.as_ref().map(|browser| &browser.path_field)
    }

    pub fn folder_secret_field(&self) -> Option<&Entity<SecretField>> {
        self.folder.as_ref().map(|browser| &browser.secret)
    }

    pub fn folder_state(&self) -> Option<FolderState> {
        let browser = self.folder.as_ref()?;
        Some(match (&browser.error, &browser.entries) {
            (Some(error), _) => FolderState::Failed(error.clone()),
            (None, None) => FolderState::Loading,
            (None, Some(entries)) => FolderState::Listed(entries.iter().map(|entry| entry.name.clone()).collect()),
        })
    }

    /// Browse…: the browser on the project folder typed, listed with the target's SSH configuration.
    pub(super) fn open_folder(&mut self, cx: &mut Context<Self>) {
        let Some(target) = self.current(cx).filter(|_| self.can_browse(cx)) else { return };
        let initial = if target.project_root.is_empty() { "~".to_owned() } else { target.project_root };
        let path_field = new_field(&initial, None, cx);
        let placeholder = tx(cx, "Password or passphrase");
        let secret = cx.new(|cx| SecretField::new(cx).with_placeholder(placeholder));
        let path_events = cx.subscribe(&path_field, |view: &mut Self, field, event: &TextFieldEvent, cx| match event {
            TextFieldEvent::Changed => cx.notify(),
            TextFieldEvent::Submitted => {
                let typed = field.read(cx).text().trim().to_owned();
                if !typed.is_empty() {
                    view.folder_go(typed, cx);
                }
            }
            TextFieldEvent::Cancelled => view.close_folder(cx),
        });
        let secret_events = cx.subscribe(&secret, |view: &mut Self, _, event: &SecretFieldEvent, cx| match event {
            SecretFieldEvent::Changed => cx.notify(),
            SecretFieldEvent::Submitted => view.folder_send_secret(cx),
            SecretFieldEvent::Cancelled => view.close_folder(cx),
        });
        self.open = None;
        self.folder = Some(FolderBrowser {
            ssh: target.ssh_config,
            path: initial.clone(),
            listed: initial.clone(),
            path_field,
            entries: None,
            error: None,
            selected: None,
            secret,
            request: 0,
            _subscriptions: vec![path_events, secret_events],
        });
        self.folder_go(initial, cx);
    }

    /// Lists folder `next`, forgetting the listing, the error and the selection shown.
    pub fn folder_go(&mut self, next: String, cx: &mut Context<Self>) {
        let Some(folders) = self.folders.clone() else { return };
        let Some(browser) = self.folder.as_mut() else { return };
        browser.request += 1;
        let request = browser.request;
        browser.entries = None;
        browser.error = None;
        browser.selected = None;
        browser.path = next.clone();
        browser.listed = next.clone();
        let (field, ssh) = (browser.path_field.clone(), browser.ssh.clone());
        field.update(cx, |field, cx| field.set_text(&next, cx));
        let view = cx.weak_entity();
        let then: Then<Result<Vec<RemoteEntry>, IdeError>> =
            Rc::new(move |result: Result<Vec<RemoteEntry>, IdeError>, cx: &mut App| {
                view.update(cx, |view, cx| view.folder_listed(request, result, cx)).ok();
            });
        // After this turn: a listing that answers at once finds the view free to update.
        cx.defer(move |cx| (folders.list)(ssh, next, then, cx));
        cx.notify();
    }

    /// Lists the folder shown again (Refresh, and after a trust or a password).
    pub fn folder_retry(&mut self, cx: &mut Context<Self>) {
        if let Some(path) = self.folder.as_ref().map(|browser| browser.path.clone()) {
            self.folder_go(path, cx);
        }
    }

    /// The answer of listing `request`: the subfolders sorted by name; the folder shown becomes
    /// absolute once an entry tells where it is, without listing it again.
    fn folder_listed(&mut self, request: u64, result: Result<Vec<RemoteEntry>, IdeError>, cx: &mut Context<Self>) {
        let Some(browser) = self.folder.as_mut().filter(|browser| browser.request == request) else { return };
        match result {
            Ok(list) => {
                let absolute = list.first().and_then(|entry| parent_path(&entry.path)).map(str::to_owned);
                let mut folders: Vec<RemoteEntry> = list.into_iter().filter(|entry| entry.is_dir).collect();
                folders.sort_by(|a, b| collate::compare(&a.name, &b.name));
                browser.entries = Some(folders);
                if let Some(absolute) = absolute.filter(|absolute| *absolute != browser.listed) {
                    browser.path = absolute.clone();
                    let field = browser.path_field.clone();
                    field.update(cx, |field, cx| field.set_text(&absolute, cx));
                }
            }
            Err(error) => browser.error = Some(error.message),
        }
        cx.notify();
    }

    /// Trust Host: trusts the key that failed the listing, then lists again; a failure is shown.
    pub fn folder_trust(&mut self, cx: &mut Context<Self>) {
        let (Some(folders), Some(browser)) = (self.folders.clone(), self.folder.as_ref()) else { return };
        let (ssh, request) = (browser.ssh.clone(), browser.request);
        let view = cx.weak_entity();
        let then: Then<Result<(), IdeError>> = Rc::new(move |result: Result<(), IdeError>, cx: &mut App| {
            view.update(cx, |view, cx| {
                let Some(browser) = view.folder.as_mut().filter(|browser| browser.request == request) else { return };
                match result {
                    Ok(()) => view.folder_retry(cx),
                    Err(error) => {
                        browser.error = Some(error.message);
                        cx.notify();
                    }
                }
            })
            .ok();
        });
        cx.defer(move |cx| (folders.trust_host)(ssh, then, cx));
    }

    /// Connect: hands the password or passphrase typed to the backend for this session, empties the
    /// field and lists again.
    pub fn folder_send_secret(&mut self, cx: &mut Context<Self>) {
        let (Some(folders), Some(browser)) = (self.folders.clone(), self.folder.as_ref()) else { return };
        let secret = SecretText::from(browser.secret.read(cx).text(cx));
        let (ssh, request) = (browser.ssh.clone(), browser.request);
        let view = cx.weak_entity();
        let then: Then<Result<(), IdeError>> = Rc::new(move |result: Result<(), IdeError>, cx: &mut App| {
            view.update(cx, |view, cx| {
                let Some(browser) = view.folder.as_mut().filter(|browser| browser.request == request) else { return };
                let field = browser.secret.clone();
                field.update(cx, |field, cx| field.clear(cx));
                match result {
                    Ok(()) => view.folder_retry(cx),
                    Err(error) => {
                        if let Some(browser) = view.folder.as_mut() {
                            browser.error = Some(error.message);
                        }
                        cx.notify();
                    }
                }
            })
            .ok();
        });
        cx.defer(move |cx| (folders.set_secret)(ssh, secret, then, cx));
    }

    pub fn folder_select(&mut self, path: String, cx: &mut Context<Self>) {
        if let Some(browser) = self.folder.as_mut() {
            browser.selected = Some(path);
            cx.notify();
        }
    }

    /// OK: the folder selected, else the one shown, becomes the target's project folder. Nothing
    /// while the listing failed.
    pub fn folder_choose(&mut self, cx: &mut Context<Self>) {
        let Some(browser) = self.folder.as_ref().filter(|browser| browser.error.is_none()) else { return };
        let chosen = browser.selected.clone().unwrap_or_else(|| browser.path.clone());
        self.folder = None;
        self.update_current(cx, |target| target.project_root = chosen);
        cx.notify();
    }

    pub fn close_folder(&mut self, cx: &mut Context<Self>) {
        if self.folder.take().is_some() {
            cx.notify();
        }
    }

    /// The browser as text: its title, `[Parent Folder] <path> [Refresh]`, the subfolders (`>` the
    /// selected one), `Loading…`, `No subfolders` or `! <error>` with `[Trust Host]` or the masked
    /// field (as bullets) and `[Connect]`, then `[OK] [Cancel]`, with `(disabled)` flags.
    pub(super) fn folder_lines(&self, cx: &App) -> Vec<String> {
        let Some(browser) = self.folder.as_ref() else { return Vec::new() };
        let flag = |open: bool| if open { "" } else { " (disabled)" };
        let path = browser.path_field.read(cx).text().to_owned();
        let parent = parent_path(&browser.path).is_some();
        let mut out = vec![
            format!("— {} —", tx(cx, TITLE)),
            format!("[{}]{} {path} [{}]", tx(cx, "Parent Folder"), flag(parent), tx(cx, "Refresh")),
        ];
        match (&browser.error, &browser.entries) {
            (Some(error), _) => {
                out.push(format!("! {error}"));
                if needs_host_trust(error) {
                    out.push(format!("[{}]", tx(cx, "Trust Host")));
                }
                if needs_secret(error) {
                    let bullets: String = browser.secret.read(cx).text(cx).chars().map(|_| BULLET).collect();
                    out.push(format!("{}: {bullets} [{}]", tx(cx, "Password or passphrase"), tx(cx, "Connect")));
                }
            }
            (None, None) => out.push(tx(cx, "Loading…")),
            (None, Some(entries)) if entries.is_empty() => out.push(tx(cx, "No subfolders")),
            (None, Some(entries)) => out.extend(entries.iter().map(|entry| {
                let mark = if browser.selected.as_deref() == Some(entry.path.as_str()) { ">" } else { " " };
                format!("{mark} {}", entry.name)
            })),
        }
        out.push(format!("[{}]{} [{}]", tx(cx, "OK"), flag(browser.error.is_none()), tx(cx, "Cancel")));
        out
    }

    /// The browser, drawn under the Project folder row while it is open.
    pub(super) fn folder_panel(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let browser = self.folder.as_ref()?;
        let parent = parent_path(&browser.path).map(str::to_owned);
        let up = icon_button(cx, "settings-target-folder-up".to_owned(), "arrow-up", "Parent Folder")
            .when(parent.is_none(), |el| el.opacity(0.5))
            .when_some(parent, |el, parent| {
                el.on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.folder_go(parent.clone(), cx)))
            });
        let refresh = icon_button(cx, "settings-target-folder-refresh".to_owned(), "refresh", "Refresh")
            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.folder_retry(cx)));
        let path = frame(cx, WIDE_FIELD, false)
            .id("settings-target-folder-path")
            .debug_selector(|| "settings-target-folder-path".to_owned())
            .font_family("monospace")
            .child(browser.path_field.clone());
        let subtle = theme::color(cx, "fg-subtle");
        let status = |text: String| {
            div().px(px(6.0)).py(px(4.0)).text_color(subtle).child(SharedString::from(text)).into_any_element()
        };
        let body: Vec<AnyElement> = match (&browser.error, &browser.entries) {
            (Some(error), _) => vec![self.folder_error(error, cx)],
            (None, None) => vec![status(tx(cx, "Loading…"))],
            (None, Some(entries)) if entries.is_empty() => vec![status(tx(cx, "No subfolders"))],
            (None, Some(entries)) => {
                let selected_bg = theme::color(cx, "bg-selected");
                entries
                    .iter()
                    .enumerate()
                    .map(|(index, entry)| {
                        let selector = format!("settings-target-folder-row-{index}");
                        let chosen = browser.selected.as_deref() == Some(entry.path.as_str());
                        let path = entry.path.clone();
                        div()
                            .id(SharedString::from(selector.clone()))
                            .debug_selector(move || selector)
                            .h(px(22.0))
                            .px(px(6.0))
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .when(chosen, |el| el.bg(selected_bg))
                            .child(theme::icon(cx, "folder", 14.0, theme::color(cx, "fg-icon")))
                            .child(SharedString::from(entry.name.clone()))
                            .on_click(cx.listener(move |view, event: &ClickEvent, _, cx| {
                                if event.click_count() >= 2 {
                                    view.folder_go(path.clone(), cx);
                                } else {
                                    view.folder_select(path.clone(), cx);
                                }
                            }))
                            .into_any_element()
                    })
                    .collect()
            }
        };
        let can_choose = browser.error.is_none();
        let (ok_label, cancel_label) = (tx(cx, "OK"), tx(cx, "Cancel"));
        let ok = div()
            .id("settings-target-folder-ok")
            .debug_selector(|| "settings-target-folder-ok".to_owned())
            .child(button(cx, &ok_label, true, can_choose))
            .when(can_choose, |el| el.on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.folder_choose(cx))));
        let cancel = div()
            .id("settings-target-folder-cancel")
            .debug_selector(|| "settings-target-folder-cancel".to_owned())
            .child(button(cx, &cancel_label, false, true))
            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.close_folder(cx)));
        let title = tx(cx, TITLE);
        let panel = div()
            .debug_selector(|| "settings-target-folder".to_owned())
            .ml(px(4.0))
            .mt(px(4.0))
            .p(px(8.0))
            .max_w(px(520.0))
            .flex()
            .flex_col()
            .gap(px(6.0))
            .rounded(theme::length(cx, "radius-small"))
            .border_1()
            .border_color(theme::color(cx, "border-popup"))
            .bg(theme::color(cx, "bg-popup"))
            .child(div().text_color(theme::color(cx, "fg-strong")).child(SharedString::from(title)))
            .child(div().flex().items_center().gap(px(4.0)).child(up).child(path).child(refresh))
            .child(
                div()
                    .min_h(px(120.0))
                    .max_h(px(240.0))
                    .flex()
                    .flex_col()
                    .rounded(theme::length(cx, "radius-small"))
                    .border_1()
                    .border_color(theme::color(cx, "border"))
                    .children(body),
            )
            .child(div().flex().justify_end().gap(px(6.0)).child(ok).child(cancel));
        Some(panel.into_any_element())
    }

    /// A failed listing: the error, with Trust Host or the password field and Connect when one of
    /// them can help.
    fn folder_error(&self, error: &str, cx: &mut Context<Self>) -> AnyElement {
        let Some(browser) = self.folder.as_ref() else { return div().into_any_element() };
        let trust = needs_host_trust(error).then(|| {
            let label = tx(cx, "Trust Host");
            div()
                .id("settings-target-folder-trust")
                .debug_selector(|| "settings-target-folder-trust".to_owned())
                .child(button(cx, &label, false, true))
                .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.folder_trust(cx)))
        });
        let secret = needs_secret(error).then(|| {
            let label = tx(cx, "Connect");
            let connect = div()
                .id("settings-target-folder-connect")
                .debug_selector(|| "settings-target-folder-connect".to_owned())
                .child(button(cx, &label, false, true))
                .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.folder_send_secret(cx)));
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .child(
                    frame(cx, 200.0, false)
                        .id("settings-secret-target.folder")
                        .debug_selector(|| "settings-secret-target.folder".to_owned())
                        .child(browser.secret.clone()),
                )
                .child(connect)
        });
        div()
            .debug_selector(|| "settings-target-folder-error".to_owned())
            .p(px(6.0))
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(div().text_color(theme::color(cx, "danger")).child(SharedString::from(error.to_owned())))
            .children(trust)
            .children(secret)
            .into_any_element()
    }
}
