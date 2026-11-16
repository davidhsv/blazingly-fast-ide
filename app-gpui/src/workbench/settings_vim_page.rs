//! Settings › Editor › Vim on the window's Settings dialog, and Tools › Vim Settings… that opens it.
//!
//! The page is a view drawn for its whole page whose changes are stored at once in the UI storage
//! under the Vim settings' key, so nothing is staged and Apply, Reset and Cancel leave it alone. The
//! workbench reads the rc file for it: `.openideavimrc`, else `.ideavimrc`, in the home folder, with
//! the files its `source` lines reach, each read through the backend's text reader as any file the
//! IDE reads without opening it. The page reads it the first time a dialog shows it in the app, never
//! while Settings shows another page; later showings show what was read until Reload reads it again.
//!
//! Under a home of its own (tests, `OPENIDEA_GPUI_HOME`) the home folder is that one, so nothing
//! reads the user's own rc file.

use super::Workbench;
use super::services_window::on_workbench;
use crate::backend::{AppDirs, Backend};
use crate::features_a::vim::rc::{RC_FILES, SourcedFiles, parse_vimrc};
use crate::features_a::vim::settings::{VIM_SETTINGS_KEY, vim_settings_json};
use crate::platform::actions::ActionRegistry;
use crate::platform::storage::UiStorage;
use crate::settings_model::staging::Scope;
use crate::settings_view::dialog::SettingsDialogView;
use crate::settings_view::pages::Binding;
use crate::settings_view::pages::editor_vim::{self, VimRcStatus, VimSettingsEvent, VimSettingsView};
use gpui_kit::{AppContext, Context, Entity, Subscription, Window};
use ide_host::Host;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Registers Tools › Vim Settings…, which opens Settings on the Vim page.
pub(super) fn register_actions(registry: &mut ActionRegistry) {
    registry.handle("vim.settings", |_, window, cx| {
        on_workbench(window, cx, |workbench, window, cx| {
            workbench.open_settings(Some(editor_vim::PAGE), false, window, cx);
        })
    });
}

/// The folder the rc files are looked for in: the home of its own of `dirs` when it has one, else
/// the user's home.
pub(super) fn rc_home(dirs: &AppDirs) -> Option<PathBuf> {
    if dirs.isolated {
        return dirs.data.parent().map(Path::to_path_buf);
    }
    dirs::home_dir()
}

/// Reads the rc file in `home` for `window`: the first of [`RC_FILES`] that can be read, then the
/// files its `source` lines reach (`~` is `home`), and parses them. A file that cannot be read is
/// the next name tried, or a sourced file reported as unreadable by the parse.
async fn read_rc(host: Arc<Host>, window: String, home: PathBuf) -> VimRcStatus {
    let mut found = None;
    for name in RC_FILES {
        let path = home.join(name);
        if let Ok(text) = ide_host::ops::fs::read_text(&host, &window, path.clone()).await {
            found = Some((path, text));
            break;
        }
    }
    let Some((path, text)) = found else { return VimRcStatus::read(None, &parse_vimrc("", None)) };
    let mut sourced = SourcedFiles::new(&text, &home.to_string_lossy());
    while let Some(next) = sourced.pending() {
        let read = ide_host::ops::fs::read_text(&host, &window, PathBuf::from(next)).await.ok();
        sourced.answer(read.as_deref());
    }
    let sources = sourced.finish();
    VimRcStatus::read(Some(path.display().to_string()), &parse_vimrc(&text, Some(&sources)))
}

impl Workbench {
    /// Puts the Vim page's view on `dialog`, reads the rc file when the dialog opens on the page and
    /// nothing has read it yet, and returns the subscription that answers the page; the caller keeps
    /// it as long as the dialog is open.
    pub(super) fn mount_vim_settings_page(
        &mut self,
        dialog: &Entity<SettingsDialogView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Subscription> {
        let model = dialog.read(cx).model().clone();
        // The page has no Global / This project switch: its values are the user's everywhere.
        let binding = Binding::fixed(model, editor_vim::PAGE, Scope::Global);
        let page = cx.new(|cx| VimSettingsView::new(binding, cx));
        let labels = editor_vim::LABELS.iter().map(|label| (*label).to_owned()).collect();
        dialog.update(cx, |dialog, cx| {
            dialog.set_page_view(editor_vim::PAGE, page.clone().into(), cx);
            dialog.set_page_labels(editor_vim::PAGE, labels, cx);
        });
        if page.read(cx).is_shown() {
            self.read_vim_rc_once(cx);
        }
        vec![cx.subscribe_in(&page, window, Self::vim_settings_page_event)]
    }

    fn vim_settings_page_event(
        &mut self,
        _: &Entity<VimSettingsView>,
        event: &VimSettingsEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            VimSettingsEvent::Changed(settings) => {
                cx.default_global::<UiStorage>().set(VIM_SETTINGS_KEY, vim_settings_json(settings));
            }
            VimSettingsEvent::ReloadRc => self.reload_vim_rc(cx),
            VimSettingsEvent::Shown => self.read_vim_rc_once(cx),
        }
    }

    /// Reads the rc file unless a read has finished already in this app.
    fn read_vim_rc_once(&mut self, cx: &mut Context<Self>) {
        if !cx.try_global::<VimRcStatus>().is_some_and(|status| status.loaded) {
            self.reload_vim_rc(cx);
        }
    }

    /// Reads the rc file off the main thread and publishes what it found. Without a backend, or a
    /// home folder, there is no file to read.
    fn reload_vim_rc(&mut self, cx: &mut Context<Self>) {
        let none = || VimRcStatus::read(None, &parse_vimrc("", None));
        let Some(backend) = cx.try_global::<Backend>() else {
            cx.set_global(none());
            return;
        };
        let Some(home) = rc_home(&backend.dirs) else {
            cx.set_global(none());
            return;
        };
        let window = self.label.clone();
        let work = backend.with_host(move |host| read_rc(host, window, home));
        cx.spawn(async move |_, cx| {
            let status = match work.await {
                Ok(status) => status,
                Err(error) => {
                    tracing::error!("reading the Vim rc file failed: {error}");
                    none()
                }
            };
            cx.update(|cx| cx.set_global(status));
        })
        .detach();
    }
}
