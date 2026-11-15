//! Sync with Deployed: the local and the server's files of the selection side by side, each row with
//! the operation Synchronize applies to it, above the content comparison of the selected row.
//!
//! The dialog reads the comparison from the [`DeploymentStore`] the workbench owns and keeps its own
//! filters, selection and content scan in a [`SyncView`]. A row's operation and the identical-files
//! filter are written to the store's comparison. What needs the backend is a [`SyncDialogEvent`]:
//! the steps to synchronize, a comparison to start again, a row whose bytes to compare, a preview
//! to load. The workbench answers with [`SyncDialog::scan_answered`],
//! [`SyncDialog::preview_loaded`] and [`SyncDialog::synchronized`].

use super::sync_preview::{LoadedPreview, SyncPreviewEvent, SyncPreviewView};
use crate::features_b::deployment::model::{SyncChoice, format_size, format_time, sync_choices};
use crate::features_b::deployment::preview::PreviewRequests;
use crate::features_b::deployment::store::{DeploymentStore, SyncDialogState, SyncRunMode};
use crate::features_b::deployment::sync_view::{
    CompareBy, DialogEffect, ScanRequest, SyncView, TableKey, choice_label,
};
use crate::features_b::deployment::types::{SyncStatus, SyncStep, TransferReport};
use crate::platform::i18n::{Localization, Param};
use crate::platform::local_time::{now_ms, utc_offset_minutes};
use crate::theme;
use crate::ui::text_field::{TextField, TextFieldEvent};
use crate::ui::tooltip::WithTooltip;
use crate::vcs_ops::menu::{MenuEntry, MenuItem};
use gpui_kit::{
    App, ClickEvent, Context, Div, Entity, EventEmitter, FocusHandle, Focusable, Hsla, IntoElement, MouseButton,
    Pixels, Point, Render, SharedString, Stateful, Subscription, UniformListScrollHandle, Window, div, prelude::*, px,
    uniform_list,
};

const SIZE_COLUMN: f32 = 80.0;
const TIME_COLUMN: f32 = 148.0;
const OPERATION_COLUMN: f32 = 108.0;

/// A menu the dialog opens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SyncMenu {
    /// Compare by: Size and Timestamp, or Binary Content.
    CompareBy,
    /// The operations of the row of this path.
    Choice(String),
}

/// What a row of one of the dialog's menus does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SyncCommand {
    CompareBy(CompareBy),
    Choose { path: String, choice: SyncChoice },
}

/// What the dialog asks of the workbench.
#[derive(Clone, Debug, PartialEq)]
pub enum SyncDialogEvent {
    /// The comparison fills a window of its own: save the open documents of these steps first,
    /// then answer with [`SyncDialog::prepared`].
    BeforeSynchronize { steps: Vec<SyncStep> },
    /// Synchronize `steps` (`DeploymentStore::run_sync`), then answer with
    /// [`SyncDialog::synchronized`].
    Run { steps: Vec<SyncStep>, mode: SyncRunMode },
    /// Refresh comparison: compare `paths` with `server` again.
    Refresh { server: String, paths: Vec<String> },
    /// Compare the bytes of a row; answer with [`SyncDialog::scan_answered`].
    Scan(ScanRequest),
    /// Load a preview, abort the one it replaces; answer with [`SyncDialog::preview_loaded`].
    Preview(PreviewRequests),
    /// Open `menu` ([`SyncDialog::menu_items`]) at `position`.
    ShowMenu { menu: SyncMenu, position: Point<Pixels> },
    /// Cancel or Escape: close the dialog.
    Close,
}

/// A button of the dialog, and whether it can be pressed and shows pressed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncButton {
    pub id: &'static str,
    pub enabled: bool,
    pub pressed: bool,
}

/// A row of the file table as shown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncRow {
    pub path: String,
    pub status: SyncStatus,
    pub choice: SyncChoice,
    pub selected: bool,
    /// The cells left to right: local name, size, date, operation, date, size, remote name.
    pub cells: [String; 7],
}

pub struct SyncDialog {
    focus: FocusHandle,
    table_focus: FocusHandle,
    store: Entity<DeploymentStore>,
    view: SyncView,
    preview: Entity<SyncPreviewView>,
    filter: Entity<TextField>,
    project_root: String,
    server_root: String,
    /// The comparison's rows a scan was last started for.
    revision: Option<u64>,
    scroll: UniformListScrollHandle,
    key_claims: [u64; 2],
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<SyncDialogEvent> for SyncDialog {}

fn tx(cx: &App, text: &str) -> String {
    cx.try_global::<Localization>().map_or_else(|| text.to_owned(), |localization| localization.0.tx(text))
}

/// `text` with values, in the active language.
fn tx_built(cx: &mut App, strings: &[&str], values: &[Param]) -> String {
    cx.default_global::<Localization>().0.tx_built(strings, values)
}

fn status_color(status: SyncStatus, cx: &App) -> Hsla {
    theme::color(
        cx,
        match status {
            SyncStatus::LocalOnly | SyncStatus::LocalNewer => "vcs-added",
            SyncStatus::RemoteOnly | SyncStatus::RemoteNewer => "vcs-modified",
            SyncStatus::Different => "warning",
            SyncStatus::Same => "fg-subtle",
        },
    )
}

impl SyncDialog {
    /// The dialog over the comparison in `store`; `project_root` and `server_root` are shown above
    /// the table. `standalone`: the comparison fills a window of its own, which saves the local
    /// documents before a run. The owner subscribes, then calls [`SyncDialog::store_changed`] once.
    pub fn new(
        store: Entity<DeploymentStore>,
        project_root: String,
        server_root: String,
        standalone: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let preview = cx.new(SyncPreviewView::new);
        let filter = cx.new(TextField::new);
        let subscriptions = vec![
            cx.observe(&store, |dialog, _, cx| dialog.store_changed(cx)),
            cx.subscribe(&preview, |_, _, event: &SyncPreviewEvent, cx| {
                let SyncPreviewEvent::Requests(requests) = event;
                cx.emit(SyncDialogEvent::Preview(requests.clone()));
            }),
            cx.subscribe_in(&filter, window, |dialog, field, event: &TextFieldEvent, _, cx| match event {
                TextFieldEvent::Changed => {
                    dialog.view.filter = field.read(cx).text().to_owned();
                    dialog.sync_preview(cx);
                }
                // Enter in the filter synchronizes nothing.
                TextFieldEvent::Submitted => {}
                TextFieldEvent::Cancelled => cx.emit(SyncDialogEvent::Close),
            }),
        ];
        let (focus, table_focus) = (cx.focus_handle(), cx.focus_handle());
        // The table's keys and Escape are the dialog's own, ahead of the keymap.
        let table = cx.weak_entity();
        let table_claim = crate::platform::input::claim_keys(
            table_focus.clone(),
            move |keystroke, _, cx| {
                !keystroke.modifiers.modified()
                    && table.update(cx, |dialog, cx| dialog.table_key(&keystroke.key, cx)).unwrap_or(false)
            },
            cx,
        );
        let dialog = cx.weak_entity();
        let escape_claim = crate::platform::input::claim_keys(
            focus.clone(),
            move |keystroke, _, cx| {
                let escape = keystroke.key == "escape" && !keystroke.modifiers.modified();
                escape && dialog.update(cx, |_, cx| cx.emit(SyncDialogEvent::Close)).is_ok()
            },
            cx,
        );
        cx.on_release(|dialog, cx| {
            for claim in dialog.key_claims {
                crate::platform::input::release_keys(claim, cx);
            }
        })
        .detach();
        Self {
            focus,
            table_focus,
            store,
            view: SyncView::new(standalone),
            preview,
            filter,
            project_root,
            server_root,
            revision: None,
            scroll: UniformListScrollHandle::new(),
            key_claims: [table_claim, escape_claim],
            _subscriptions: subscriptions,
        }
    }

    /// Runs `f` on the comparison open in the store.
    fn with_sync<R>(
        &mut self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut SyncView, &mut SyncDialogState) -> R,
    ) -> Option<R> {
        let store = self.store.clone();
        let view = &mut self.view;
        store.update(cx, |store, _| store.sync.as_mut().map(|sync| f(view, sync)))
    }

    /// The comparison open in the store, as it is now.
    pub fn sync<'a>(&self, cx: &'a App) -> Option<&'a SyncDialogState> {
        self.store.read(cx).sync.as_ref()
    }

    /// The store's comparison changed: rows that replaced the old ones get their operations
    /// suggested again (and a content scan, in Binary Content mode), and the preview follows the
    /// selected row.
    pub fn store_changed(&mut self, cx: &mut Context<Self>) {
        let revision = self.revision;
        let restarted = self.with_sync(cx, |view, sync| {
            if sync.entries.is_none() || revision == Some(sync.revision) {
                return (None, None);
            }
            (Some(sync.revision), view.restart(sync))
        });
        if let Some((Some(revision), scan)) = restarted {
            self.revision = Some(revision);
            if let Some(scan) = scan {
                cx.emit(SyncDialogEvent::Scan(scan));
            }
        }
        self.sync_preview(cx);
    }

    /// Shows the selected row's comparison in the preview.
    fn sync_preview(&mut self, cx: &mut Context<Self>) {
        let entry = self.sync(cx).and_then(|sync| self.view.preview_entry(sync));
        self.preview.update(cx, |preview, cx| preview.show(entry, cx));
        cx.notify();
    }

    pub fn preview(&self) -> &Entity<SyncPreviewView> {
        &self.preview
    }

    pub fn filter_field(&self) -> &Entity<TextField> {
        &self.filter
    }

    pub fn table_focus(&self) -> &FocusHandle {
        &self.table_focus
    }

    /// The dialog's title.
    pub fn title(&self, cx: &mut App) -> String {
        let name = self.sync(cx).map(|sync| sync.server_name.clone()).unwrap_or_default();
        tx_built(cx, &["Diff Between 'local files' and 'files on ", "'"], &[Param::from(name.as_str())])
    }

    /// The rows shown, top to bottom.
    pub fn rows(&self, cx: &App) -> Vec<SyncRow> {
        let Some(sync) = self.sync(cx) else { return Vec::new() };
        let selected = self.view.selected(sync).map(|entry| entry.path);
        let offset = utc_offset_minutes(now_ms());
        self.view
            .shown(sync)
            .into_iter()
            .map(|entry| {
                let choice = sync.choices.get(&entry.path).unwrap_or(SyncChoice::Skip);
                let (arrow, name) = choice_label(choice);
                let local_name = if entry.local.is_some() { entry.path.clone() } else { String::new() };
                let remote_name = if entry.remote.is_some() { entry.path.clone() } else { String::new() };
                let size = |stamp: Option<crate::features_b::deployment::types::FileStamp>| {
                    stamp.map(|stamp| format_size(stamp.size)).unwrap_or_default()
                };
                let time = |stamp: Option<crate::features_b::deployment::types::FileStamp>| {
                    format_time(stamp.and_then(|stamp| stamp.mtime), offset)
                };
                SyncRow {
                    selected: selected.as_ref() == Some(&entry.path),
                    status: entry.status,
                    choice,
                    cells: [
                        local_name,
                        size(entry.local),
                        time(entry.local),
                        format!("{arrow} {}", tx(cx, name)),
                        time(entry.remote),
                        size(entry.remote),
                        remote_name,
                    ],
                    path: entry.path,
                }
            })
            .collect()
    }

    /// The column headers of the file table.
    pub fn columns(&self, cx: &App) -> [String; 7] {
        ["Local name", "Size", "Date", "↔", "Date", "Size", "Remote name"].map(|text| tx(cx, text))
    }

    /// The selected row's path.
    pub fn selected(&self, cx: &App) -> Option<String> {
        self.sync(cx).and_then(|sync| self.view.selected(sync)).map(|entry| entry.path)
    }

    /// The toolbar's and the footer's buttons.
    pub fn buttons(&self, cx: &App) -> Vec<SyncButton> {
        let Some(sync) = self.sync(cx) else { return Vec::new() };
        let busy = self.view.busy(sync);
        let button = |id, enabled, pressed| SyncButton { id, enabled, pressed };
        let mut out = vec![
            button("sync-refresh", !busy, false),
            button("sync-show-local", true, self.view.show_local),
            button("sync-show-different", true, self.view.show_different),
            button("sync-show-identical", true, !sync.hide_same),
            button("sync-show-remote", true, self.view.show_remote),
            button("sync-compare-by", !sync.running, false),
            button("sync-selected", self.view.can_synchronize_selected(sync), false),
        ];
        if self.view.progress(sync).is_some() {
            out.push(button("sync-stop-scan", true, false));
        }
        out.push(button("sync-cancel", true, false));
        out.push(button("sync-all", self.view.can_synchronize_all(sync), false));
        out
    }

    fn button_enabled(&self, id: &str, cx: &App) -> bool {
        self.buttons(cx).iter().any(|button| button.id == id && button.enabled)
    }

    /// The error of the comparison and the content check error of the selected row.
    pub fn errors(&self, cx: &App) -> Vec<String> {
        let Some(sync) = self.sync(cx) else { return Vec::new() };
        sync.error.iter().cloned().chain(self.view.selected_check_error(sync)).collect()
    }

    /// "3 of 12 files will change", once the rows are listed.
    pub fn footer(&self, cx: &mut App) -> Option<String> {
        let sync = self.store.read(cx).sync.clone()?;
        self.view.footer(&sync, &mut cx.default_global::<Localization>().0)
    }

    /// "Comparing contents: 3 of 12" while a content scan runs.
    pub fn progress_text(&self, cx: &mut App) -> Option<String> {
        let (progress, total) = {
            let sync = self.sync(cx)?;
            (self.view.progress(sync)?, sync.entries.as_ref().map_or(0, Vec::len))
        };
        Some(tx_built(cx, &["Comparing contents: ", " of ", ""], &[Param::from(progress), Param::from(total)]))
    }

    /// The rows of `menu`.
    pub fn menu_items(&self, menu: &SyncMenu, cx: &App) -> Vec<MenuItem<SyncCommand>> {
        let row = |label: String, run: SyncCommand, checked: bool| {
            MenuItem::Item(MenuEntry {
                label,
                icon: checked.then_some("check"),
                shortcut: None,
                disabled: false,
                danger: false,
                run: Some(run),
                submenu: Vec::new(),
            })
        };
        match menu {
            SyncMenu::CompareBy => {
                [(CompareBy::Metadata, "Size and Timestamp"), (CompareBy::Content, "Binary Content")]
                    .into_iter()
                    .map(|(mode, label)| {
                        row(tx(cx, label), SyncCommand::CompareBy(mode), self.view.compare_by() == mode)
                    })
                    .collect()
            }
            SyncMenu::Choice(path) => {
                let Some(sync) = self.sync(cx) else { return Vec::new() };
                let Some(entry) = sync.entries.iter().flatten().find(|entry| entry.path == *path) else {
                    return Vec::new();
                };
                let current = sync.choices.get(path).unwrap_or(SyncChoice::Skip);
                sync_choices(entry)
                    .into_iter()
                    .map(|choice| {
                        let (arrow, name) = choice_label(choice);
                        let command = SyncCommand::Choose { path: path.clone(), choice };
                        row(format!("{arrow} {}", tx(cx, name)), command, choice == current)
                    })
                    .collect()
            }
        }
    }

    /// Carries out a row of one of the dialog's menus.
    pub fn run_command(&mut self, command: SyncCommand, cx: &mut Context<Self>) {
        match command {
            SyncCommand::CompareBy(mode) => self.set_compare_by(mode, cx),
            SyncCommand::Choose { path, choice } => self.choose(&path, choice, cx),
        }
    }

    /// Sets a row's operation by hand (its menu, Space).
    pub fn choose(&mut self, path: &str, choice: SyncChoice, cx: &mut Context<Self>) {
        let busy = self.sync(cx).is_none_or(|sync| self.view.busy(sync));
        if busy {
            return;
        }
        self.with_sync(cx, |view, sync| view.choose(path, choice, sync));
        self.sync_preview(cx);
    }

    /// Selects the row of `path`.
    pub fn select(&mut self, path: &str, cx: &mut Context<Self>) {
        self.view.select(path);
        self.sync_preview(cx);
    }

    fn set_compare_by(&mut self, mode: CompareBy, cx: &mut Context<Self>) {
        if self.sync(cx).is_none_or(|sync| sync.running) {
            return;
        }
        if let Some(Some(scan)) = self.with_sync(cx, |view, sync| view.set_compare_by(mode, sync)) {
            cx.emit(SyncDialogEvent::Scan(scan));
        }
        self.sync_preview(cx);
    }

    /// The answer of a [`SyncDialogEvent::Scan`]: whether the row's bytes are equal, or why they
    /// could not be compared. The next row is asked for.
    pub fn scan_answered(&mut self, ticket: u64, result: Result<bool, String>, cx: &mut Context<Self>) {
        if let Some(Some(scan)) = self.with_sync(cx, |view, sync| view.equal_result(ticket, result, sync)) {
            cx.emit(SyncDialogEvent::Scan(scan));
        }
        self.sync_preview(cx);
    }

    /// The answer of a [`SyncDialogEvent::Preview`] load.
    pub fn preview_loaded(&mut self, ticket: u64, result: Result<LoadedPreview, String>, cx: &mut Context<Self>) {
        self.preview.update(cx, |preview, cx| preview.loaded(ticket, result, cx));
    }

    /// A run asked for with [`SyncDialogEvent::Run`] ended (`report`: what it did).
    pub fn synchronized(&mut self, report: Option<&TransferReport>, cx: &mut Context<Self>) {
        let sync = self.store.read(cx).sync.clone();
        self.view.synchronized(report, sync.as_ref());
        self.sync_preview(cx);
    }

    fn emit_effect(&mut self, effect: Option<DialogEffect>, cx: &mut Context<Self>) {
        match effect {
            Some(DialogEffect::RunSync { steps, mode }) => cx.emit(SyncDialogEvent::Run { steps, mode }),
            Some(DialogEffect::Refresh { server, paths }) => cx.emit(SyncDialogEvent::Refresh { server, paths }),
            Some(DialogEffect::BeforeSynchronize { steps }) => cx.emit(SyncDialogEvent::BeforeSynchronize { steps }),
            None => {}
        }
        self.sync_preview(cx);
    }

    /// Synchronize All, or Synchronize Selected.
    pub fn synchronize(&mut self, mode: SyncRunMode, cx: &mut Context<Self>) {
        let effect = self.with_sync(cx, |view, sync| view.synchronize(mode, sync)).flatten();
        self.emit_effect(effect, cx);
    }

    /// The answer of [`SyncDialogEvent::BeforeSynchronize`]: the run goes on once the documents
    /// are saved; a failed save stops it with its message.
    pub fn prepared(&mut self, result: Result<(), String>, cx: &mut Context<Self>) {
        let store = self.store.clone();
        let view = &mut self.view;
        let effect = store.update(cx, |store, cx| {
            let effect = view.prepared(result, store.sync.as_mut());
            cx.notify();
            effect
        });
        self.emit_effect(effect, cx);
    }

    /// Refresh comparison.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        let Some(sync) = self.store.read(cx).sync.clone() else { return };
        let effect = self.view.refresh(&sync, true);
        if effect.is_some() {
            self.revision = None;
        }
        self.emit_effect(effect, cx);
    }

    fn toggle_filter(&mut self, id: &str, cx: &mut Context<Self>) {
        match id {
            "sync-show-local" => self.view.show_local = !self.view.show_local,
            "sync-show-different" => self.view.show_different = !self.view.show_different,
            "sync-show-remote" => self.view.show_remote = !self.view.show_remote,
            "sync-show-identical" => {
                self.with_sync(cx, |view, sync| view.toggle_identical(sync));
            }
            _ => return,
        }
        self.sync_preview(cx);
    }

    /// A key on the file table: arrows move the selection, Enter synchronizes the selected row,
    /// Space cycles its operation, Escape closes. Whether the key was the table's.
    pub fn table_key(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        let key = match key {
            "up" => TableKey::Up,
            "down" => TableKey::Down,
            "enter" => TableKey::Enter,
            "space" => TableKey::Space,
            "escape" => {
                cx.emit(SyncDialogEvent::Close);
                return true;
            }
            _ => return false,
        };
        let effect = self.with_sync(cx, |view, sync| view.on_table_key(key, sync)).flatten();
        self.emit_effect(effect, cx);
        if let Some(index) = self.selected(cx).and_then(|path| self.rows(cx).iter().position(|row| row.path == path)) {
            self.scroll.scroll_to_item(index, gpui_kit::ScrollStrategy::Top);
        }
        true
    }

    // ---------------------------------------------------------------------------------------------
    // Rendering

    fn icon_button(
        &self,
        id: &'static str,
        icon: &str,
        tooltip: &str,
        cx: &mut Context<Self>,
        on_click: impl Fn(&mut Self, &ClickEvent, &mut Context<Self>) + 'static,
    ) -> Stateful<Div> {
        let enabled = self.button_enabled(id, cx);
        let color = theme::color(cx, if enabled { "fg-muted" } else { "fg-faint" });
        div()
            .id(id)
            .debug_selector(move || id.to_owned())
            .size(px(24.0))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(theme::length(cx, "radius-small"))
            .when(enabled, |el| {
                el.hover(|el| el.bg(theme::color(cx, "bg-hover")))
                    .on_click(cx.listener(move |dialog, event: &ClickEvent, _, cx| on_click(dialog, event, cx)))
            })
            .child(theme::icon(cx, icon, 16.0, color))
            .with_tooltip(tx(cx, tooltip))
    }

    fn filter_button(
        &self,
        id: &'static str,
        text: &'static str,
        tooltip: &str,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let pressed = self.buttons(cx).iter().any(|button| button.id == id && button.pressed);
        div()
            .id(id)
            .debug_selector(move || id.to_owned())
            .w(px(28.0))
            .h(px(26.0))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(theme::length(cx, "radius-small"))
            .when(pressed, |el| {
                el.border_1().border_color(theme::color(cx, "border")).bg(theme::color(cx, "bg-tree-hover"))
            })
            .text_color(theme::color(cx, if pressed { "accent" } else { "fg-muted" }))
            .child(SharedString::from(text))
            .on_click(cx.listener(move |dialog, _: &ClickEvent, _, cx| dialog.toggle_filter(id, cx)))
            .with_tooltip(tx(cx, tooltip))
    }

    fn separator(cx: &App) -> Div {
        div().w(px(1.0)).h(px(16.0)).mx(px(4.0)).flex_none().bg(theme::color(cx, "border"))
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> Div {
        let compare_label = tx(
            cx,
            match self.view.compare_by() {
                CompareBy::Metadata => "Size and Timestamp",
                CompareBy::Content => "Binary Content",
            },
        );
        let compare_enabled = self.button_enabled("sync-compare-by", cx);
        let compare = div()
            .id("sync-compare-by")
            .debug_selector(|| "sync-compare-by".to_owned())
            .h(px(24.0))
            .flex()
            .items_center()
            .gap(px(4.0))
            .px(px(6.0))
            .rounded(theme::length(cx, "radius-small"))
            .border_1()
            .border_color(theme::color(cx, "border"))
            .bg(theme::color(cx, "bg-input"))
            .when(!compare_enabled, |el| el.opacity(0.5))
            .child(SharedString::from(compare_label))
            .child(theme::icon(cx, "chevron-down", 14.0, theme::color(cx, "fg-icon")))
            .when(compare_enabled, |el| {
                el.on_click(cx.listener(|_, event: &ClickEvent, _, cx| {
                    cx.emit(SyncDialogEvent::ShowMenu { menu: SyncMenu::CompareBy, position: event.position() });
                }))
            });
        let progress = self.progress_text(cx).map(|text| {
            div()
                .flex()
                .items_center()
                .gap(px(4.0))
                .child(
                    div()
                        .debug_selector(|| "sync-progress".to_owned())
                        .text_color(theme::color(cx, "fg-subtle"))
                        .text_size(theme::length(cx, "fs-small"))
                        .child(SharedString::from(text)),
                )
                .child(self.icon_button("sync-stop-scan", "stop", "Stop content comparison", cx, |dialog, _, cx| {
                    dialog.set_compare_by(CompareBy::Metadata, cx)
                }))
        });
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(4.0))
            .child(
                self.icon_button("sync-refresh", "refresh", "Refresh comparison", cx, |dialog, _, cx| {
                    dialog.refresh(cx)
                }),
            )
            .child(Self::separator(cx))
            .child(self.filter_button("sync-show-local", "→", "Show local-only files", cx))
            .child(self.filter_button("sync-show-different", "≠", "Show different files", cx))
            .child(self.filter_button("sync-show-identical", "=", "Show identical files", cx))
            .child(self.filter_button("sync-show-remote", "←", "Show remote-only files", cx))
            .child(Self::separator(cx))
            .child(div().text_color(theme::color(cx, "fg-muted")).child(SharedString::from(tx(cx, "Compare by:"))))
            .child(compare)
            .child(Self::separator(cx))
            .child(self.icon_button("sync-selected", "run", "Synchronize Selected", cx, |dialog, _, cx| {
                dialog.synchronize(SyncRunMode::Selected, cx)
            }))
            .children(progress)
            .child(div().flex_1())
            .child(div().text_color(theme::color(cx, "fg-muted")).child(SharedString::from(tx(cx, "Filter:"))))
            .child(
                div()
                    .debug_selector(|| "sync-filter".to_owned())
                    .w(px(180.0))
                    .h(px(24.0))
                    .flex()
                    .items_center()
                    .px(px(6.0))
                    .rounded(theme::length(cx, "radius-small"))
                    .border_1()
                    .border_color(theme::color(cx, "border"))
                    .bg(theme::color(cx, "bg-input"))
                    .child(div().flex_1().min_w_0().child(self.filter.clone())),
            )
    }

    fn render_roots(&self, sync: &SyncDialogState, cx: &mut Context<Self>) -> Div {
        let muted = theme::color(cx, "fg-muted");
        let local = if self.project_root.is_empty() { tx(cx, "Local files") } else { self.project_root.clone() };
        let remote_root = if self.server_root.is_empty() { "/".to_owned() } else { self.server_root.clone() };
        let side = |icon: &str, text: String, cx: &mut Context<Self>| {
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .gap(px(6.0))
                .overflow_hidden()
                .whitespace_nowrap()
                .child(theme::icon(cx, icon, 14.0, muted))
                .child(SharedString::from(text))
        };
        let local = side("folder", local, cx);
        let remote = side("web", format!("{}: {remote_root}", sync.server_name), cx);
        div().flex_none().flex().gap(px(12.0)).child(local).child(remote)
    }

    fn render_table(&self, window: &Window, cx: &mut Context<Self>) -> Div {
        let rows = self.rows(cx);
        let busy = self.sync(cx).is_none_or(|sync| self.view.busy(sync));
        let focused = self.table_focus.is_focused(window);
        let row_height = theme::length(cx, "h-list-row");
        let selected_bg = theme::color(cx, if focused { "bg-tree-selected" } else { "bg-tree-selected-inactive" });
        let (border, subtle, muted) =
            (theme::color(cx, "border"), theme::color(cx, "fg-subtle"), theme::color(cx, "fg-muted"));
        let colors: Vec<Hsla> = rows.iter().map(|row| status_color(row.status, cx)).collect();
        let header_cells = self.columns(cx);
        let cell = |text: SharedString, width: Option<f32>, right: bool| {
            let cell = div().h_full().flex().items_center().px(px(6.0)).overflow_hidden().whitespace_nowrap();
            let cell = match width {
                Some(width) => cell.w(px(width)).flex_none(),
                None => cell.flex_1().min_w_0(),
            };
            if right { cell.justify_end().child(text) } else { cell.child(text) }
        };
        let widths = [
            None,
            Some(SIZE_COLUMN),
            Some(TIME_COLUMN),
            Some(OPERATION_COLUMN),
            Some(TIME_COLUMN),
            Some(SIZE_COLUMN),
            None,
        ];
        let header =
            div().flex_none().h(row_height).flex().border_b_1().border_color(border).text_color(muted).children(
                header_cells.into_iter().zip(widths).map(|(text, width)| cell(SharedString::from(text), width, false)),
            );
        let empty = rows.is_empty().then(|| {
            div()
                .debug_selector(|| "sync-no-differences".to_owned())
                .h(row_height)
                .flex()
                .items_center()
                .justify_center()
                .text_color(subtle)
                .child(SharedString::from(tx(cx, "No matching differences")))
        });
        let list = uniform_list(
            "sync-rows",
            rows.len(),
            cx.processor(move |_, range: std::ops::Range<usize>, _, cx| {
                range
                    .map(|index| {
                        let row = &rows[index];
                        let color = colors[index];
                        let skipped = row.choice == SyncChoice::Skip;
                        let selector = format!("sync-row:{}", row.path);
                        let operation = format!("sync-operation:{}", row.path);
                        let (clicked, menu_path) = (row.path.clone(), row.path.clone());
                        let cells = row.cells.clone();
                        let text_color = |column: usize| if skipped && column > 0 { subtle } else { color };
                        let mut line = div()
                            .id(SharedString::from(selector.clone()))
                            .debug_selector(move || selector.clone())
                            .h(row_height)
                            .w_full()
                            .flex()
                            .when(row.selected, |el| el.bg(selected_bg))
                            .on_click(cx.listener(move |dialog, _: &ClickEvent, window, cx| {
                                window.focus(&dialog.table_focus, cx);
                                dialog.select(&clicked, cx);
                            }));
                        for (column, (text, width)) in cells.into_iter().zip(widths).enumerate() {
                            let right = column == 1 || column == 5;
                            let content = cell(SharedString::from(text), width, right).text_color(text_color(column));
                            line = if column == 3 {
                                let menu_path = menu_path.clone();
                                let operation = operation.clone();
                                line.child(
                                    div()
                                        .id(SharedString::from(operation.clone()))
                                        .debug_selector(move || operation.clone())
                                        .h_full()
                                        .flex_none()
                                        .border_l_1()
                                        .border_r_1()
                                        .border_color(border)
                                        .child(content)
                                        .when(!busy, |el| {
                                            el.on_click(cx.listener(move |dialog, event: &ClickEvent, window, cx| {
                                                window.focus(&dialog.table_focus, cx);
                                                dialog.select(&menu_path, cx);
                                                let menu = SyncMenu::Choice(menu_path.clone());
                                                cx.emit(SyncDialogEvent::ShowMenu { menu, position: event.position() });
                                                cx.stop_propagation();
                                            }))
                                        }),
                                )
                            } else {
                                line.child(content)
                            };
                        }
                        line
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.scroll)
        .size_full();
        div()
            .debug_selector(|| "sync-table".to_owned())
            .flex_1()
            .min_h(px(100.0))
            .flex()
            .flex_col()
            .border_1()
            .border_color(border)
            .rounded(theme::length(cx, "radius-small"))
            .track_focus(&self.table_focus)
            .child(header)
            .children(empty)
            .child(div().flex_1().min_h_0().child(list))
    }

    fn render_footer(&self, cx: &mut Context<Self>) -> Div {
        let footer = self.footer(cx).unwrap_or_default();
        let all_enabled = self.button_enabled("sync-all", cx);
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(8.0))
            .px(px(16.0))
            .py(px(12.0))
            .child(
                div()
                    .debug_selector(|| "sync-footer".to_owned())
                    .flex_1()
                    .text_color(theme::color(cx, "fg-subtle"))
                    .text_size(theme::length(cx, "fs-small"))
                    .child(SharedString::from(footer)),
            )
            .child(
                crate::vcs::prompt_dialogs::button(cx, tx(cx, "Cancel"), false, false, true)
                    .id("sync-cancel")
                    .debug_selector(|| "sync-cancel".to_owned())
                    .on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(SyncDialogEvent::Close))),
            )
            .child(
                crate::vcs::prompt_dialogs::button(cx, tx(cx, "Synchronize All"), true, false, all_enabled)
                    .id("sync-all")
                    .debug_selector(|| "sync-all".to_owned())
                    .when(all_enabled, |el| {
                        el.on_click(
                            cx.listener(|dialog, _: &ClickEvent, _, cx| dialog.synchronize(SyncRunMode::All, cx)),
                        )
                    }),
            )
    }
}

impl Focusable for SyncDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.table_focus.clone()
    }
}

impl Render for SyncDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(sync) = self.store.read(cx).sync.clone() else { return div().into_any_element() };
        let viewport = window.viewport_size();
        let title = self.title(cx);
        let (danger, subtle) = (theme::color(cx, "danger"), theme::color(cx, "fg-subtle"));
        let errors = self.errors(cx).into_iter().map(|error| {
            div().debug_selector(|| "sync-error".to_owned()).text_color(danger).child(SharedString::from(error))
        });
        let skipped = (!sync.skipped.is_empty()).then(|| {
            let count = tx_built(cx, &["Skipped: ", ""], &[Param::from(sync.skipped.len())]);
            let paths: Vec<&str> = sync.skipped.iter().map(|item| item.path.as_str()).collect();
            div()
                .debug_selector(|| "sync-skipped".to_owned())
                .text_color(subtle)
                .text_size(theme::length(cx, "fs-small"))
                .child(SharedString::from(format!("{count}: {}", paths.join(", "))))
        });
        let status = |text: &str, selector: &'static str, cx: &mut Context<Self>| {
            div()
                .debug_selector(move || selector.to_owned())
                .flex()
                .items_center()
                .justify_center()
                .py(px(12.0))
                .text_color(theme::color(cx, "fg-muted"))
                .child(SharedString::from(tx(cx, text)))
        };
        let comparing = (sync.entries.is_none() && sync.error.is_none())
            .then(|| status("Comparing local and remote files…", "sync-comparing", cx));
        let running = sync.running.then(|| status("Synchronizing…", "sync-running", cx));
        let body = sync.entries.is_some().then(|| {
            let hint = tx(
                cx,
                "Space changes the selected operation. Enter synchronizes the selected file. Synchronize All applies all chosen operations.",
            );
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .gap(px(6.0))
                .child(
                    div()
                        .h(gpui_kit::relative(0.32))
                        .min_h(px(100.0))
                        .flex()
                        .flex_col()
                        .child(self.render_table(window, cx))
                        .child(
                            div()
                                .flex_none()
                                .flex()
                                .justify_end()
                                .px(px(6.0))
                                .py(px(3.0))
                                .text_color(subtle)
                                .text_size(theme::length(cx, "fs-small"))
                                .child(SharedString::from(hint)),
                        ),
                )
                .child(div().flex_1().min_h(px(160.0)).child(self.preview.clone()))
        });
        div()
            .id("sync-dialog")
            .debug_selector(|| "sync-dialog".to_owned())
            .w(viewport.width - px(48.0))
            .h(viewport.height - px(48.0))
            .flex()
            .flex_col()
            .bg(theme::color(cx, "bg-popup"))
            .border_1()
            .border_color(theme::color(cx, "border-popup"))
            .rounded(theme::length(cx, "radius-popup"))
            .text_size(theme::length(cx, "fs-ui"))
            .text_color(theme::color(cx, "fg"))
            .track_focus(&self.focus)
            // A click on the dialog is not a click beside it.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .px(px(16.0))
                    .pt(px(14.0))
                    .pb(px(8.0))
                    .text_color(theme::color(cx, "fg-strong"))
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .child(SharedString::from(title)),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .px(px(16.0))
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(self.render_toolbar(cx))
                    .child(self.render_roots(&sync, cx))
                    .children(errors)
                    .children(skipped)
                    .children(comparing)
                    .children(running)
                    .children(body),
            )
            .child(self.render_footer(cx))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features_b::deployment::model::initial_sync_choices;
    use crate::features_b::deployment::types::{FileStamp, SyncAction, SyncEntry};
    use gpui_kit::{Modifiers, TestAppContext, VisualTestContext};
    use std::cell::RefCell;
    use std::rc::Rc;

    type Events = Rc<RefCell<Vec<SyncDialogEvent>>>;

    fn entry(path: &str, status: SyncStatus) -> SyncEntry {
        SyncEntry {
            path: path.to_owned(),
            remote_path: format!("/srv/app/{path}"),
            local: (status != SyncStatus::RemoteOnly).then_some(FileStamp { size: 2048, mtime: Some(1_700_000_000) }),
            remote: (status != SyncStatus::LocalOnly).then_some(FileStamp { size: 2048, mtime: Some(1_700_000_100) }),
            status,
        }
    }

    fn comparison(entries: Vec<SyncEntry>) -> SyncDialogState {
        SyncDialogState {
            server: "s1".to_owned(),
            server_name: "Staging".to_owned(),
            paths: vec!["/work/app".to_owned()],
            skipped: Vec::new(),
            request: None,
            choices: initial_sync_choices(&entries),
            entries: Some(entries),
            error: None,
            hide_same: true,
            running: false,
            revision: 1,
        }
    }

    struct Opened<'a> {
        dialog: Entity<SyncDialog>,
        store: Entity<DeploymentStore>,
        events: Events,
        cx: &'a mut VisualTestContext,
    }

    fn open(cx: &mut TestAppContext, entries: Vec<SyncEntry>) -> Opened<'_> {
        open_in(cx, entries, false)
    }

    /// The dialog over `entries`; `standalone`: it fills a window of its own.
    fn open_in(cx: &mut TestAppContext, entries: Vec<SyncEntry>, standalone: bool) -> Opened<'_> {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            cx.set_global(Localization::default());
            crate::platform::input::init(cx);
        });
        let store = cx.new(|_| {
            let mut store = DeploymentStore::default();
            store.sync = Some(comparison(entries));
            store
        });
        let held = store.clone();
        let (dialog, cx) = cx.add_window_view(move |window, cx| {
            SyncDialog::new(held, "/work/app".to_owned(), "/srv/app".to_owned(), standalone, window, cx)
        });
        let events: Events = Rc::default();
        let seen = Rc::clone(&events);
        cx.update(|_, cx| {
            cx.subscribe(&dialog, move |_, event: &SyncDialogEvent, _| seen.borrow_mut().push(event.clone())).detach();
        });
        dialog.update(cx, |dialog, cx| dialog.store_changed(cx));
        cx.update(|window, _| window.activate_window());
        cx.run_until_parked();
        Opened { dialog, store, events, cx }
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.run_until_parked();
    }

    fn shown(o: &mut Opened<'_>) -> Vec<String> {
        o.dialog.read_with(o.cx, |dialog, cx| dialog.rows(cx).into_iter().map(|row| row.path).collect())
    }

    fn choice(o: &mut Opened<'_>, path: &str) -> Option<SyncChoice> {
        o.store.read_with(o.cx, |store, _| store.sync.as_ref().and_then(|sync| sync.choices.get(path)))
    }

    fn enabled(o: &mut Opened<'_>, id: &str) -> bool {
        o.dialog.read_with(o.cx, |dialog, cx| dialog.buttons(cx).iter().any(|button| button.id == id && button.enabled))
    }

    fn runs(o: &Opened<'_>) -> Vec<(Vec<SyncStep>, SyncRunMode)> {
        o.events
            .borrow()
            .iter()
            .filter_map(|event| match event {
                SyncDialogEvent::Run { steps, mode } => Some((steps.clone(), *mode)),
                _ => None,
            })
            .collect()
    }

    fn scans(o: &Opened<'_>) -> Vec<ScanRequest> {
        o.events
            .borrow()
            .iter()
            .filter_map(|event| match event {
                SyncDialogEvent::Scan(scan) => Some(scan.clone()),
                _ => None,
            })
            .collect()
    }

    /// The remote path of the row the preview last loaded.
    fn previewed(o: &Opened<'_>) -> Option<String> {
        o.events.borrow().iter().rev().find_map(|event| match event {
            SyncDialogEvent::Preview(PreviewRequests { load: Some(load), .. }) => Some(load.entry.remote_path.clone()),
            _ => None,
        })
    }

    fn step(path: &str, action: SyncAction) -> SyncStep {
        SyncStep { path: path.to_owned(), action }
    }

    fn command(o: &mut Opened<'_>, command: SyncCommand) {
        o.dialog.update(o.cx, |dialog, cx| dialog.run_command(command, cx));
        o.cx.run_until_parked();
    }

    // deployment directory and content comparison › fills a secondary window without creating an
    // in-page modal and saves local edits before synchronization
    #[gpui_kit::test]
    async fn fills_a_secondary_window_without_creating_an_in_page_modal_and_saves_local_edits_before_synchronization(
        cx: &mut TestAppContext,
    ) {
        let mut o = open_in(cx, vec![entry("a.php", SyncStatus::LocalOnly)], true);
        let title = o.dialog.update(o.cx, |dialog, cx| dialog.title(cx));
        assert_eq!(title, "Diff Between 'local files' and 'files on Staging'");
        assert!(o.cx.debug_bounds("sync-table").is_some(), "the window holds the file table");
        click(o.cx, "sync-all");
        let saving = SyncDialogEvent::BeforeSynchronize { steps: vec![step("a.php", SyncAction::Upload)] };
        assert_eq!(o.events.borrow().iter().filter(|event| **event == saving).count(), 1);
        assert!(!enabled(&mut o, "sync-all"), "Synchronize All waits for the save");
        assert!(runs(&o).is_empty(), "nothing is sent before the documents are saved");
        o.dialog.update(o.cx, |dialog, cx| dialog.prepared(Ok(()), cx));
        o.cx.run_until_parked();
        assert_eq!(runs(&o), [(vec![step("a.php", SyncAction::Upload)], SyncRunMode::All)]);
    }

    // deployment directory and content comparison › shows mirrored metadata and previews the
    // selected file, with identical rows filtered
    #[gpui_kit::test]
    async fn shows_mirrored_metadata_and_previews_the_selected_file_with_identical_rows_filtered(
        cx: &mut TestAppContext,
    ) {
        let mut o = open(
            cx,
            vec![
                entry("a.php", SyncStatus::LocalOnly),
                entry("b.php", SyncStatus::RemoteNewer),
                entry("c.php", SyncStatus::Same),
            ],
        );
        let columns = o.dialog.read_with(o.cx, |dialog, cx| dialog.columns(cx));
        assert_eq!((columns[0].as_str(), columns[6].as_str()), ("Local name", "Remote name"));
        assert_eq!(shown(&mut o), ["a.php", "b.php"]);
        let rows = o.dialog.read_with(o.cx, |dialog, cx| dialog.rows(cx));
        assert_eq!(rows[0].cells[0], "a.php");
        assert_eq!(rows[0].cells[1], "2.0 KB");
        assert_eq!(rows[0].cells[6], "", "a.php is not on the server");
        assert_eq!(rows[1].cells[6], "b.php");
        assert_eq!(previewed(&o).as_deref(), Some("/srv/app/a.php"));
        click(o.cx, "sync-row:b.php");
        assert_eq!(o.dialog.read_with(o.cx, |dialog, cx| dialog.selected(cx)).as_deref(), Some("b.php"));
        assert_eq!(previewed(&o).as_deref(), Some("/srv/app/b.php"));
        click(o.cx, "sync-show-identical");
        assert!(shown(&mut o).contains(&"c.php".to_owned()));
        assert_eq!(choice(&mut o, "c.php"), Some(SyncChoice::Skip));
    }

    // deployment directory and content comparison › filters by name and category, navigates with
    // arrows and cycles operations with Space
    #[gpui_kit::test]
    async fn filters_by_name_and_category_navigates_with_arrows_and_cycles_operations_with_space(
        cx: &mut TestAppContext,
    ) {
        let mut o = open(cx, vec![entry("a.php", SyncStatus::LocalOnly), entry("b.php", SyncStatus::RemoteNewer)]);
        click(o.cx, "sync-row:a.php");
        o.cx.simulate_keystrokes("down");
        assert_eq!(o.dialog.read_with(o.cx, |dialog, cx| dialog.selected(cx)).as_deref(), Some("b.php"));
        o.cx.simulate_keystrokes("space");
        assert_eq!(choice(&mut o, "b.php"), Some(SyncChoice::DeleteRemote));
        let field = o.dialog.read_with(o.cx, |dialog, _| dialog.filter_field().clone());
        o.cx.update(|window, cx| window.focus(&field.focus_handle(cx), cx));
        o.cx.simulate_keystrokes("enter");
        assert!(runs(&o).is_empty(), "Enter in the filter synchronizes nothing");
        o.cx.simulate_input("b.php");
        o.cx.run_until_parked();
        assert_eq!(shown(&mut o), ["b.php"]);
        click(o.cx, "sync-show-different");
        assert!(shown(&mut o).is_empty());
        assert!(o.cx.debug_bounds("sync-no-differences").is_some(), "No matching differences");
    }

    // deployment directory and content comparison › updates a row's operation and only applies
    // explicit synchronization
    #[gpui_kit::test]
    async fn updates_a_rows_operation_and_only_applies_explicit_synchronization(cx: &mut TestAppContext) {
        let mut o = open(cx, vec![entry("a.php", SyncStatus::LocalOnly)]);
        command(&mut o, SyncCommand::Choose { path: "a.php".to_owned(), choice: SyncChoice::Skip });
        assert!(!enabled(&mut o, "sync-all"));
        command(&mut o, SyncCommand::Choose { path: "a.php".to_owned(), choice: SyncChoice::Upload });
        assert!(runs(&o).is_empty(), "choosing an operation runs nothing");
        click(o.cx, "sync-all");
        assert_eq!(runs(&o), [(vec![step("a.php", SyncAction::Upload)], SyncRunMode::All)]);
    }

    // deployment directory and content comparison › finds content hidden by identical metadata and
    // skips byte-identical files
    #[gpui_kit::test]
    async fn finds_content_hidden_by_identical_metadata_and_skips_byte_identical_files(cx: &mut TestAppContext) {
        let mut o = open(cx, vec![entry("a.php", SyncStatus::LocalNewer), entry("b.php", SyncStatus::Same)]);
        command(&mut o, SyncCommand::CompareBy(CompareBy::Content));
        let mut answered = 0;
        while let Some(scan) = scans(&o).get(answered).cloned() {
            answered += 1;
            let equal = scan.entry.path == "a.php";
            o.dialog.update(o.cx, |dialog, cx| dialog.scan_answered(scan.ticket, Ok(equal), cx));
            o.cx.run_until_parked();
        }
        assert_eq!(answered, 2, "both rows are compared");
        assert_eq!(shown(&mut o), ["b.php"]);
        assert_eq!(choice(&mut o, "b.php"), Some(SyncChoice::Upload));
        assert_eq!(choice(&mut o, "a.php"), Some(SyncChoice::Skip));
        command(&mut o, SyncCommand::CompareBy(CompareBy::Metadata));
        assert!(shown(&mut o).contains(&"a.php".to_owned()));
        assert_eq!(choice(&mut o, "a.php"), Some(SyncChoice::Upload));
    }

    // deployment directory and content comparison › synchronizes only the current row from the
    // toolbar or Enter, and disables skipped selections
    #[gpui_kit::test]
    async fn synchronizes_only_the_current_row_from_the_toolbar_or_enter_and_disables_skipped_selections(
        cx: &mut TestAppContext,
    ) {
        let mut o = open(cx, vec![entry("a.php", SyncStatus::LocalOnly), entry("b.php", SyncStatus::RemoteNewer)]);
        click(o.cx, "sync-row:b.php");
        click(o.cx, "sync-selected");
        assert_eq!(runs(&o), [(vec![step("b.php", SyncAction::Download)], SyncRunMode::Selected)]);
        o.dialog.update(o.cx, |dialog, cx| dialog.synchronized(None, cx));
        click(o.cx, "sync-row:a.php");
        o.cx.simulate_keystrokes("enter");
        assert_eq!(runs(&o).last(), Some(&(vec![step("a.php", SyncAction::Upload)], SyncRunMode::Selected)));
        o.dialog.update(o.cx, |dialog, cx| dialog.synchronized(None, cx));
        command(&mut o, SyncCommand::Choose { path: "a.php".to_owned(), choice: SyncChoice::Skip });
        assert!(!enabled(&mut o, "sync-selected"));
        assert!(enabled(&mut o, "sync-all"));
        let before = runs(&o).len();
        click(o.cx, "sync-row:a.php");
        o.cx.simulate_keystrokes("enter");
        assert_eq!(runs(&o).len(), before, "a skipped row is not synchronized");
    }

    // deployment directory and content comparison › preserves manual choices, reports failures, and
    // ignores a cancelled scan
    #[gpui_kit::test]
    async fn preserves_manual_choices_reports_failures_and_ignores_a_cancelled_scan(cx: &mut TestAppContext) {
        let mut o = open(cx, vec![entry("a.php", SyncStatus::LocalNewer)]);
        command(&mut o, SyncCommand::Choose { path: "a.php".to_owned(), choice: SyncChoice::Skip });
        command(&mut o, SyncCommand::CompareBy(CompareBy::Content));
        let first = scans(&o).last().cloned().expect("a.php is compared");
        click(o.cx, "sync-stop-scan");
        o.dialog.update(o.cx, |dialog, cx| dialog.scan_answered(first.ticket, Ok(true), cx));
        o.cx.run_until_parked();
        assert_eq!(choice(&mut o, "a.php"), Some(SyncChoice::Skip));
        command(&mut o, SyncCommand::CompareBy(CompareBy::Content));
        let second = scans(&o).last().cloned().expect("a.php is compared again");
        assert_ne!(second.ticket, first.ticket);
        o.dialog
            .update(o.cx, |dialog, cx| dialog.scan_answered(second.ticket, Err("Permission denied".to_owned()), cx));
        o.cx.run_until_parked();
        let errors = o.dialog.read_with(o.cx, |dialog, cx| dialog.errors(cx));
        assert_eq!(errors, ["Permission denied"]);
        assert!(o.cx.debug_bounds("sync-error").is_some());
        assert_eq!(choice(&mut o, "a.php"), Some(SyncChoice::Skip));
    }

    /// The title names the server, the footer counts the changes, Refresh compares the selection
    /// again and Cancel closes.
    #[gpui_kit::test]
    async fn the_title_footer_refresh_and_cancel(cx: &mut TestAppContext) {
        let mut o = open(cx, vec![entry("a.php", SyncStatus::LocalOnly), entry("c.php", SyncStatus::Same)]);
        let title = o.dialog.update(o.cx, |dialog, cx| dialog.title(cx));
        assert_eq!(title, "Diff Between 'local files' and 'files on Staging'");
        let footer = o.dialog.update(o.cx, |dialog, cx| dialog.footer(cx));
        assert_eq!(footer.as_deref(), Some("1 of 2 files will change"));
        click(o.cx, "sync-refresh");
        let refresh = SyncDialogEvent::Refresh { server: "s1".to_owned(), paths: vec!["/work/app".to_owned()] };
        assert!(o.events.borrow().contains(&refresh));
        click(o.cx, "sync-cancel");
        assert_eq!(o.events.borrow().last(), Some(&SyncDialogEvent::Close));
    }
}
