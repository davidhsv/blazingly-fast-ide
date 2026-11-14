//! Fills the Settings dialog's slots for controls of their own ([`SettingsDialogView::set_custom_control`])
//! with the views of [`super::pages`], and answers what those views ask: OK and Cancel from their
//! fields, the system's file chooser, a Git test run, forgetting a saved Git credential, reading
//! the installed fonts again and showing another page.
//!
//! Every control stages into the dialog's model under its page, in the scope the dialog shows
//! for that page, so Apply, OK, Cancel and Reset treat it as they treat the standard controls.
//! Backend work runs on the backend's runtime or the background executor and answers the view
//! that asked through the callback the request carries.

use super::dialog::SettingsDialogView;
use super::pages::font_picker::{FontCatalog, FontPicker, PickerSpec};
use super::pages::list_editor::{ListEditor, ListSpec};
use super::pages::{self, Binding, ControlEvent, FormView, Part, ScopeOf, StackView};
use crate::backend::Backend;
use crate::settings_model::staging::Scope;
use crate::theme::fonts::{FontFamily, FontStatus, SystemFonts, merge_font_families};
use gpui_kit::{
    AnyView, App, AppContext, Entity, EventEmitter, PathPromptOptions, Render, SharedString, Subscription, Window,
    font, px,
};
use std::rc::Rc;

/// The forms of a page, by slot label.
type Forms = Vec<(&'static str, Vec<Part>)>;

/// What the slots of one dialog need from the window.
pub struct Install<'a> {
    /// The backend's label of the window (Git runs are trusted per window).
    pub window_label: &'a str,
    /// A project is open in the window.
    pub has_project: bool,
}

/// Puts a view in every slot this module has a view for, on the 13 pages it covers, and
/// returns the subscriptions that answer their requests; the caller keeps them as long as the
/// dialog is open.
pub fn install(
    dialog: &Entity<SettingsDialogView>,
    install: Install<'_>,
    _window: &mut Window,
    cx: &mut App,
) -> Vec<Subscription> {
    // The pickers read the installed fonts once one of them is shown.
    if !cx.has_global::<FontCatalog>() {
        cx.set_global(FontCatalog::default());
    }
    let model = dialog.read(cx).model().clone();
    let weak = dialog.downgrade();
    let scope: ScopeOf =
        Rc::new(move |cx: &App| weak.upgrade().map_or(Scope::Global, |dialog| dialog.read(cx).scope(cx)));
    let label: Rc<str> = Rc::from(install.window_label);
    let mut slots = Slots { dialog: dialog.clone(), label, subscriptions: Vec::new() };
    let binding = |page: &'static str| Binding::new(model.clone(), page, scope.clone());

    // Pages made of forms and lists.
    let simple: [(&'static str, Forms, Vec<ListSpec>); 6] = [
        (pages::console::PAGE, pages::console::forms(), pages::console::lists()),
        (pages::data_views::PAGE, pages::data_views::forms(), Vec::new()),
        (pages::deployment_options::PAGE, pages::deployment_options::forms(), Vec::new()),
        (pages::vcs_confirmation::PAGE, pages::vcs_confirmation::forms(), Vec::new()),
        (pages::vcs_commit::PAGE, pages::vcs_commit::forms(), Vec::new()),
        (pages::general::PAGE, pages::general::forms(), Vec::new()),
    ];
    for (page, forms, lists) in simple {
        slots.forms(binding(page), forms, cx);
        slots.lists(binding(page), lists, cx);
    }

    // Version Control › Git.
    let git = binding(pages::vcs_git::PAGE);
    slots.forms(git.clone(), pages::vcs_git::forms(), cx);
    let executable = cx.new(|cx| pages::vcs_git::GitExecutableView::new(git.clone(), None, cx));
    slots.put(pages::vcs_git::PAGE, "Path to Git executable", &executable, cx);
    // Finding git on the PATH reads the disk: it runs off the window's thread.
    if let Some(host) = cx.try_global::<Backend>().map(|backend| backend.host.clone()) {
        let weak_executable = executable.downgrade();
        cx.spawn(async move |cx| {
            let detected =
                cx.background_executor().spawn(async move { ide_host::ops::vcs::vcs_detected_git(&host, "") }).await;
            weak_executable.update(cx, |view, cx| view.set_detected(detected.as_deref(), cx)).ok();
        })
        .detach();
    }
    let credentials = cx.new(|_| pages::vcs_git::GitCredentialsView::new(git, Vec::new()));
    slots.put(pages::vcs_git::PAGE, "Passwords", &credentials, cx);
    let weak_credentials = credentials.downgrade();
    let store = git_store(cx);
    cx.spawn(async move |cx| {
        let hosts = cx.background_executor().spawn(async move { saved_git_hosts(store.as_ref()) }).await;
        weak_credentials.update(cx, |view, cx| view.set_hosts(hosts, cx)).ok();
    })
    .detach();

    // Tools › Terminal.
    let terminal = binding(pages::terminal::PAGE);
    slots.forms(terminal.clone(), pages::terminal::forms(), cx);
    slots.lists(terminal.clone(), pages::terminal::lists(), cx);
    let shell = cx.new(|cx| pages::terminal::ShellPathView::new(terminal.clone(), None, Vec::new(), cx));
    slots.put(pages::terminal::PAGE, "Shell path", &shell, cx);
    // The login shell and the installed shells are read off the window's thread.
    let weak_shell = shell.downgrade();
    cx.spawn(async move |cx| {
        let (default_shell, shells) = cx
            .background_executor()
            .spawn(async { (ide_terminal::default_shell_path(), ide_terminal::available_shells()) })
            .await;
        weak_shell
            .update(cx, |view, cx| {
                view.set_default_shell(&default_shell, cx);
                view.set_shells(shells, cx);
            })
            .ok();
    })
    .detach();
    let pieces = pages::terminal::font_slot().into_iter().map(|piece| match piece {
        pages::terminal::FontPiece::Form(parts) => Piece::Form(parts),
        pages::terminal::FontPiece::Picker(spec) => Piece::Picker(spec),
    });
    slots.stack(terminal, pages::terminal::PAGE, "Use editor font", pieces.collect(), cx);

    // Appearance & Behavior › Notifications.
    let notifications = binding(pages::notifications::PAGE);
    let groups = cx.new(|cx| pages::notifications::NotificationGroupsView::new(notifications, cx));
    slots.put(pages::notifications::PAGE, "Popup type", &groups, cx);

    // Appearance & Behavior › Appearance.
    let appearance = binding(pages::appearance::PAGE);
    let zoom_hint = SharedString::from(pages::appearance::zoom_hint(cx));
    // No plugin host lists file icon themes to this dialog: only a stored one is offered.
    let forms = pages::appearance::forms(zoom_hint, Vec::new());
    let icon_theme_stored = !appearance.text("appearance.fileIconTheme", cx).is_empty();
    let (icon, forms): (Vec<_>, Vec<_>) = forms.into_iter().partition(|(label, _)| *label == "File icon theme");
    slots.forms(appearance.clone(), forms, cx);
    if icon_theme_stored {
        slots.forms(appearance.clone(), icon, cx);
    } else {
        // As the page has it: the row shows once a theme is installed or still selected.
        slots.empty(pages::appearance::PAGE, "File icon theme", cx);
    }
    if !pages::appearance::frameless_title_bar() {
        slots.empty(pages::appearance::PAGE, "Main menu", cx);
    }
    let accent = cx.new(|cx| pages::appearance::AccentView::new(appearance.clone(), cx));
    slots.put(pages::appearance::PAGE, "Accent colour", &accent, cx);
    let pieces = pages::appearance::custom_font_slot().into_iter().map(|piece| match piece {
        pages::appearance::FontPiece::Form(parts) => Piece::Form(parts),
        pages::appearance::FontPiece::Picker(spec) => Piece::Picker(spec),
    });
    slots.stack(appearance, pages::appearance::PAGE, "Use custom font", pieces.collect(), cx);

    // Editor › Font.
    let editor_font = binding(pages::editor_font::PAGE);
    slots.forms(editor_font.clone(), pages::editor_font::forms(), cx);
    let family = cx.new(|cx| FontPicker::new(editor_font.clone(), pages::editor_font::font_picker(), cx));
    slots.put(pages::editor_font::PAGE, "Font", &family, cx);
    let fallback = cx.new(|cx| FontPicker::new(editor_font.clone(), pages::editor_font::fallback_picker(), cx));
    slots.put(pages::editor_font::PAGE, "Fallback font", &fallback, cx);
    let weights: Vec<AnyView> = pages::editor_font::WEIGHTS
        .into_iter()
        .map(|weight| {
            let view = cx.new(|cx| pages::editor_font::WeightView::new(editor_font.clone(), weight, cx));
            slots.listen(&view, cx);
            view.into()
        })
        .collect();
    let stack = cx.new(|_| StackView::new(weights));
    slots.fill(pages::editor_font::PAGE, "Main font weight", stack.into(), cx);

    // Editor › Code Style.
    let code_style = binding(pages::code_style::PAGE);
    slots.forms(code_style.clone(), pages::code_style::forms(), cx);
    slots.lists(code_style.clone(), pages::code_style::lists(), cx);
    let can_project = install.has_project && !model.read(cx).new_projects;
    let scheme = cx.new(|cx| pages::code_style::SchemeView::new(code_style, can_project, cx));
    slots.put(pages::code_style::PAGE, "Scheme", &scheme, cx);

    slots.subscriptions
}

/// One piece of a slot that mixes form controls and font pickers.
enum Piece {
    Form(Vec<Part>),
    Picker(PickerSpec),
}

/// The dialog whose slots are filled and the subscriptions to their views.
struct Slots {
    dialog: Entity<SettingsDialogView>,
    label: Rc<str>,
    subscriptions: Vec<Subscription>,
}

impl Slots {
    /// Answers the requests of `view`.
    fn listen<V: Render + EventEmitter<ControlEvent>>(&mut self, view: &Entity<V>, cx: &mut App) {
        let dialog = self.dialog.downgrade();
        let label = self.label.clone();
        self.subscriptions.push(cx.subscribe(view, move |_, event: &ControlEvent, cx| {
            if let Some(dialog) = dialog.upgrade() {
                answer(&dialog, &label, event.clone(), cx);
            }
        }));
    }

    fn fill(&mut self, page: &'static str, slot: &'static str, view: AnyView, cx: &mut App) {
        self.dialog.update(cx, |dialog, cx| dialog.set_custom_control(page, slot, view, cx));
    }

    fn put<V: Render + EventEmitter<ControlEvent>>(
        &mut self,
        page: &'static str,
        slot: &'static str,
        view: &Entity<V>,
        cx: &mut App,
    ) {
        self.listen(view, cx);
        self.fill(page, slot, view.clone().into(), cx);
    }

    /// A slot the page draws nothing in here (a row the platform or the plugins leave out).
    fn empty(&mut self, page: &'static str, slot: &'static str, cx: &mut App) {
        let view = cx.new(|_| StackView::new(Vec::new()));
        self.fill(page, slot, view.into(), cx);
    }

    fn forms(&mut self, binding: Binding, forms: Vec<(&'static str, Vec<Part>)>, cx: &mut App) {
        for (slot, parts) in forms {
            let binding = binding.clone();
            let view = cx.new(|cx| FormView::new(binding, parts, cx));
            self.put(view.read(cx).binding().page, slot, &view, cx);
        }
    }

    fn lists(&mut self, binding: Binding, lists: Vec<ListSpec>, cx: &mut App) {
        for spec in lists {
            let binding = binding.clone();
            let page = binding.page;
            let view = cx.new(|cx| ListEditor::from_spec(binding, spec, cx));
            self.put(page, spec.label, &view, cx);
        }
    }

    fn stack(&mut self, binding: Binding, page: &'static str, slot: &'static str, pieces: Vec<Piece>, cx: &mut App) {
        let mut views: Vec<AnyView> = Vec::new();
        for piece in pieces {
            let binding = binding.clone();
            match piece {
                Piece::Form(parts) => {
                    let view = cx.new(|cx| FormView::new(binding, parts, cx));
                    self.listen(&view, cx);
                    views.push(view.into());
                }
                Piece::Picker(spec) => {
                    let view = cx.new(|cx| FontPicker::new(binding, spec, cx));
                    self.listen(&view, cx);
                    views.push(view.into());
                }
            }
        }
        let stack = cx.new(|_| StackView::new(views));
        self.fill(page, slot, stack.into(), cx);
    }
}

/// Carries out what a control asked.
fn answer(dialog: &Entity<SettingsDialogView>, window_label: &str, event: ControlEvent, cx: &mut App) {
    match event {
        ControlEvent::Submit => dialog.update(cx, |dialog, cx| dialog.ok(cx)),
        ControlEvent::Cancel => dialog.update(cx, |dialog, cx| dialog.cancel(cx)),
        ControlEvent::Browse { directories, then } => {
            let options = PathPromptOptions { files: !directories, directories, multiple: false, prompt: None };
            let picked = cx.prompt_for_paths(options);
            cx.spawn(async move |cx| match picked.await {
                Ok(Ok(Some(paths))) => {
                    if let Some(path) = paths.into_iter().next() {
                        cx.update(|cx| then(path, cx));
                    }
                }
                Ok(Ok(None)) | Err(_) => {}
                Ok(Err(error)) => tracing::warn!("Settings: the file chooser failed: {error}"),
            })
            .detach();
        }
        ControlEvent::TestGit { path, then } => {
            let Some(backend) = cx.try_global::<Backend>() else {
                then(Err("The backend is not running".to_owned()), cx);
                return;
            };
            let label = window_label.to_owned();
            let work = backend
                .with_host(move |host| async move { ide_host::ops::vcs::vcs_git_version(&host, &label, path).await });
            cx.spawn(async move |cx| {
                let shown = match work.await {
                    Ok(Ok(info)) => Ok(pages::vcs_git::version_line(&info.version, &info.path)),
                    Ok(Err(error)) => Err(error.message),
                    Err(error) => Err(error.to_string()),
                };
                cx.update(|cx| then(shown, cx));
            })
            .detach();
        }
        ControlEvent::ForgetCredential { key, then } => {
            let store = git_store(cx);
            let forgetting = cx.background_executor().spawn(async move {
                if let Some(store) = &store {
                    store.forget(&key);
                }
                saved_git_hosts(store.as_ref())
            });
            cx.spawn(async move |cx| {
                let hosts = forgetting.await;
                cx.update(|cx| then(hosts, cx));
            })
            .detach();
        }
        ControlEvent::RefreshFonts => load_font_catalog(cx),
        ControlEvent::OpenPage { page, query } => {
            let model = dialog.read(cx).model().clone();
            if let Some(query) = query {
                let search = dialog.read(cx).search_field().clone();
                search.update(cx, |field, cx| field.set_text(query, cx));
            }
            model.update(cx, |model, cx| {
                if let Some(query) = query {
                    model.query = query.to_owned();
                }
                model.select_page(page);
                cx.notify();
            });
        }
    }
}

/// The backend's store of saved Git HTTPS credentials.
fn git_store(cx: &App) -> Option<ide_ssh::git_creds::GitCredentials> {
    cx.try_global::<Backend>().and_then(|backend| backend.git_credentials().cloned())
}

/// The hosts with a saved Git HTTPS credential.
fn saved_git_hosts(store: Option<&ide_ssh::git_creds::GitCredentials>) -> Vec<String> {
    store.map(ide_ssh::git_creds::GitCredentials::list).unwrap_or_default()
}

/// Reads the installed fonts off the window's thread (hundreds of families are resolved and
/// measured); the pickers say they are loading meanwhile, and follow the list once it is read.
fn load_font_catalog(cx: &mut App) {
    // Two pickers on a page ask together: the fonts are read once.
    if cx.try_global::<FontCatalog>().is_some_and(|catalog| catalog.0.status == FontStatus::Loading) {
        return;
    }
    let mut loading = cx.try_global::<FontCatalog>().map(|catalog| catalog.0.clone()).unwrap_or_default();
    loading.status = FontStatus::Loading;
    cx.set_global(FontCatalog(loading));
    let text_system = cx.text_system().clone();
    let read = cx.background_executor().spawn(async move { system_font_catalog(&text_system) });
    cx.spawn(async move |cx| {
        let catalog = read.await;
        cx.update(|cx| {
            cx.set_global(catalog);
            cx.refresh_windows();
        });
    })
    .detach();
}

/// The names of the installed font families. Test builds read none: no test depends on the
/// fonts of the machine it runs on.
fn installed_font_names(text_system: &gpui_kit::TextSystem) -> Vec<String> {
    #[cfg(not(test))]
    return text_system.all_font_names();
    #[cfg(test)]
    {
        let _ = text_system;
        Vec::new()
    }
}

/// The installed families, each marked monospaced when its "i" is as wide as its "W".
fn system_font_catalog(text_system: &gpui_kit::TextSystem) -> FontCatalog {
    let fonts: Vec<FontFamily> = installed_font_names(text_system)
        .into_iter()
        .map(|name| {
            let id = text_system.resolve_font(&font(SharedString::from(name.clone())));
            let narrow = text_system.advance(id, px(16.0), 'i');
            let wide = text_system.advance(id, px(16.0), 'W');
            let monospaced = match (narrow, wide) {
                (Ok(narrow), Ok(wide)) => (f32::from(narrow.width) - f32::from(wide.width)).abs() < 0.5,
                _ => false,
            };
            FontFamily { name, monospaced }
        })
        .collect();
    let status = if fonts.is_empty() { FontStatus::Empty } else { FontStatus::Ready };
    let mut catalog = SystemFonts::default();
    catalog.fonts = merge_font_families(&fonts);
    (catalog.status, catalog.source) = (status, crate::theme::fonts::FontSource::Native);
    FontCatalog(catalog)
}

#[cfg(test)]
mod tests {
    //! Each page through File › Settings… in a window with the backend, its own controls changed
    //! with the mouse and the keyboard, applied, and read back from the settings in effect.

    use super::*;
    use crate::backend::AppDirs;
    use crate::platform::input;
    use crate::platform::settings::{effective, global};
    use crate::platform::storage::UiStorage;
    use crate::settings_model::schema::{get_at, settings_value};
    use crate::workbench::Workbench;
    use gpui_kit::{Focusable, Modifiers, TestAppContext, VisualTestContext};
    use serde_json::{Value, json};
    use std::path::PathBuf;
    use std::time::Duration;

    /// The backend under a temporary home, the actions, and shells that read nothing of the user's.
    fn start(cx: &mut TestAppContext) -> tempfile::TempDir {
        cx.executor().allow_parking();
        let home = tempfile::tempdir().expect("a temporary home");
        cx.update(|cx| {
            cx.set_global(crate::theme::ActiveTheme::default());
            input::init(cx);
            crate::editor::actions::register(cx);
            crate::workbench::register_actions(cx);
            cx.set_global(UiStorage::default());
            Backend::init(AppDirs::under(home.path()), cx).expect("the backend starts");
            crate::workbench::menu::init(cx);
            crate::terminal::test_support::isolate_shells(home.path(), cx);
            // The pickers suggest the bundled families only, as if the system had no others.
            let mut catalog = SystemFonts::default();
            catalog.status = FontStatus::Ready;
            cx.set_global(FontCatalog(catalog));
        });
        home
    }

    fn settle(cx: &mut VisualTestContext, done: impl Fn(&mut VisualTestContext) -> bool) {
        for _ in 0..3000 {
            cx.run_until_parked();
            if done(cx) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("not done within thirty seconds");
    }

    fn click(cx: &mut VisualTestContext, selector: &str) {
        cx.run_until_parked();
        let selector: &'static str = Box::leak(selector.to_owned().into_boxed_str());
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.run_until_parked();
    }

    /// Clicks the field and types `text` over its text.
    fn type_into(cx: &mut VisualTestContext, selector: &str, text: &str) {
        click(cx, selector);
        cx.simulate_keystrokes("cmd-a");
        cx.simulate_input(text);
        cx.run_until_parked();
    }

    fn open_page(cx: &mut VisualTestContext, workbench: &Entity<Workbench>, page: &str) -> Entity<SettingsDialogView> {
        cx.update(|window, cx| {
            crate::platform::actions::execute_action_or_throw("file.settings", Some(&Value::from(page)), window, cx)
        })
        .expect("Settings opens");
        cx.run_until_parked();
        let dialog = workbench.read_with(cx, |w, _| w.settings_dialog().cloned()).expect("Settings is open");
        assert_eq!(dialog.read_with(cx, |view, cx| view.selected_page(cx)).as_deref(), Some(page));
        let lines = dialog.read_with(cx, |view, cx| view.control_lines(cx));
        let missing: Vec<&String> =
            lines.iter().filter(|line| line.ends_with(super::super::controls::UNAVAILABLE)).collect();
        assert!(missing.is_empty(), "{page} draws every control: {missing:?}");
        dialog
    }

    fn effective_value(cx: &mut VisualTestContext, key: &str) -> Option<Value> {
        cx.update(|_, cx| get_at(&settings_value(&effective("w1", cx)), key).cloned())
    }

    fn global_value(cx: &mut VisualTestContext, key: &str) -> Option<Value> {
        cx.update(|_, cx| get_at(&settings_value(&global(cx)), key).cloned())
    }

    fn window(cx: &mut TestAppContext) -> (Entity<Workbench>, &mut VisualTestContext) {
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        cx.update(|window, _| window.activate_window());
        (workbench, cx)
    }

    /// A trusted project with one file, open in a window.
    fn project(cx: &mut TestAppContext) -> (tempfile::TempDir, PathBuf, Entity<Workbench>, &mut VisualTestContext) {
        let folder = tempfile::tempdir().expect("a temporary folder");
        let root = folder.path().canonicalize().expect("the folder exists");
        std::fs::write(root.join("a.txt"), "a\n").expect("the file is written");
        cx.update(|cx| cx.global::<Backend>().host.trust.decide(&root, true, false)).expect("the project is trusted");
        let (workbench, cx) = window(cx);
        workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
        settle(cx, |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
        (folder, root, workbench, cx)
    }

    fn is_open(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
        workbench.read_with(cx, |w, _| w.settings_dialog().is_some())
    }

    // Build › Console: the buffer checkbox and its size apply; a fold pattern added and then
    // cancelled is not stored.
    #[gpui_kit::test]
    async fn console_buffer_and_fold_patterns_apply_and_cancel_discards(cx: &mut TestAppContext) {
        let _home = start(cx);
        let (workbench, cx) = window(cx);
        let dialog = open_page(cx, &workbench, pages::console::PAGE);
        // The buffer is overridden by default: the first click turns the override off.
        assert_eq!(effective_value(cx, "console.overrideBuffer"), Some(json!(true)));
        click(cx, "settings-toggle-console.overrideBuffer");
        click(cx, "settings-apply");
        assert_eq!(effective_value(cx, "console.overrideBuffer"), Some(json!(false)));
        click(cx, "settings-toggle-console.overrideBuffer");
        type_into(cx, "settings-number-console.bufferKb", "4096");
        assert!(dialog.read_with(cx, |view, cx| view.is_modified(cx)), "the page is marked modified");
        click(cx, "settings-apply");
        assert_eq!(effective_value(cx, "console.overrideBuffer"), Some(json!(true)));
        assert_eq!(effective_value(cx, "console.bufferKb"), Some(json!(4096)));

        let before = effective_value(cx, "console.foldPatterns");
        click(cx, "settings-list-console.foldPatterns-add");
        cx.simulate_input("at org.example.");
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        assert!(dialog.read_with(cx, |view, cx| view.can_apply(cx)));
        click(cx, "settings-cancel");
        assert!(!is_open(&workbench, cx));
        assert_eq!(effective_value(cx, "console.foldPatterns"), before, "Cancel discards the pattern");
    }

    // Build › Debugger › Data Views: Up in the delay steps it by 100 ms; Reset restores it.
    #[gpui_kit::test]
    async fn data_views_delay_steps_with_the_arrows_and_reset_restores_it(cx: &mut TestAppContext) {
        let _home = start(cx);
        let (workbench, cx) = window(cx);
        let dialog = open_page(cx, &workbench, pages::data_views::PAGE);
        let before = effective_value(cx, "debugger.valueTooltipDelay").and_then(|value| value.as_u64()).unwrap_or(0);
        click(cx, "settings-number-debugger.valueTooltipDelay");
        cx.simulate_keystrokes("up");
        cx.run_until_parked();
        click(cx, "settings-reset");
        assert!(!dialog.read_with(cx, |view, cx| view.is_modified(cx)), "Reset discards the edit");
        click(cx, "settings-number-debugger.valueTooltipDelay");
        cx.simulate_keystrokes("up");
        click(cx, "settings-ok");
        assert!(!is_open(&workbench, cx), "OK closes the dialog");
        assert_eq!(effective_value(cx, "debugger.valueTooltipDelay"), Some(json!(before + 100)));
    }

    // Build › Deployment › Options: the names typed are written to the project, whose values the
    // page shows first.
    #[gpui_kit::test]
    async fn deployment_excludes_are_written_to_the_project(cx: &mut TestAppContext) {
        let _home = start(cx);
        let (_folder, root, workbench, cx) = project(cx);
        let dialog = open_page(cx, &workbench, pages::deployment_options::PAGE);
        assert_eq!(dialog.read_with(cx, |view, cx| view.scope(cx)), Scope::Project);
        type_into(cx, "settings-text-deployment.exclude", ".git;*.pyc");
        click(cx, "settings-apply");
        assert_eq!(effective_value(cx, "deployment.exclude"), Some(json!([".git", "*.pyc"])));
        let file = std::fs::read_to_string(root.join(".openidea/settings.toml")).unwrap_or_default();
        assert!(file.contains("*.pyc"), "{file}");
    }

    // Version Control › Confirmation: a radio button applies; Reset restores the other; the
    // history limit applies.
    #[gpui_kit::test]
    async fn vcs_confirmation_radio_buttons_and_history_limit_apply(cx: &mut TestAppContext) {
        let _home = start(cx);
        let (workbench, cx) = window(cx);
        let dialog = open_page(cx, &workbench, pages::vcs_confirmation::PAGE);
        click(cx, "settings-radio-vcs.confirmation.onDelete-never");
        click(cx, "settings-reset");
        assert!(!dialog.read_with(cx, |view, cx| view.is_modified(cx)));
        click(cx, "settings-radio-vcs.confirmation.onCreate-silently");
        type_into(cx, "settings-number-vcs.historyLimit", "500");
        click(cx, "settings-apply");
        assert_eq!(effective_value(cx, "vcs.confirmation.onCreate"), Some(json!("silently")));
        assert_ne!(effective_value(cx, "vcs.confirmation.onDelete"), Some(json!("never")));
        assert_eq!(effective_value(cx, "vcs.historyLimit"), Some(json!(500)));
    }

    // Version Control › Commit: the subject limit and a commit check apply.
    #[gpui_kit::test]
    async fn vcs_commit_limits_and_checks_apply(cx: &mut TestAppContext) {
        let _home = start(cx);
        let (workbench, cx) = window(cx);
        open_page(cx, &workbench, pages::vcs_commit::PAGE);
        let limit = effective_value(cx, "vcs.commit.limitSubject").and_then(|value| value.as_bool()).unwrap_or(false);
        let todo = effective_value(cx, "vcs.commit.checkTodo").and_then(|value| value.as_bool()).unwrap_or(false);
        click(cx, "settings-toggle-vcs.commit.limitSubject");
        type_into(cx, "settings-number-vcs.commit.subjectLimit", "60");
        click(cx, "settings-toggle-vcs.commit.checkTodo");
        click(cx, "settings-apply");
        assert_eq!(effective_value(cx, "vcs.commit.limitSubject"), Some(json!(!limit)));
        assert_eq!(effective_value(cx, "vcs.commit.subjectLimit"), Some(json!(60)));
        assert_eq!(effective_value(cx, "vcs.commit.checkTodo"), Some(json!(!todo)));
    }

    // System Settings: where projects open, the default directory and the idle save apply.
    #[gpui_kit::test]
    async fn system_settings_open_in_directory_and_idle_save_apply(cx: &mut TestAppContext) {
        let _home = start(cx);
        let (workbench, cx) = window(cx);
        open_page(cx, &workbench, pages::general::PAGE);
        click(cx, "settings-radio-general.openProjectIn-same");
        type_into(cx, "settings-text-general.defaultProjectDirectory", "/tmp/projects");
        click(cx, "settings-toggle-general.saveOnIdle");
        type_into(cx, "settings-number-general.idleSeconds", "30");
        click(cx, "settings-apply");
        assert_eq!(global_value(cx, "general.openProjectIn"), Some(json!("same")));
        assert_eq!(global_value(cx, "general.defaultProjectDirectory"), Some(json!("/tmp/projects")));
        assert_eq!(global_value(cx, "general.idleSeconds"), Some(json!(30)));
    }

    // Version Control › Git: Test runs git and says its version; protected branches and the
    // update method apply; without saved credentials Passwords says so.
    #[gpui_kit::test]
    async fn vcs_git_test_protected_branches_and_update_method(cx: &mut TestAppContext) {
        let _home = start(cx);
        let (_folder, _root, workbench, cx) = project(cx);
        open_page(cx, &workbench, pages::vcs_git::PAGE);
        click(cx, "settings-git-test");
        // The answer line is drawn under the field once git, run on the backend, answers.
        settle(cx, |cx| {
            cx.debug_bounds("settings-git-ok").is_some() || cx.debug_bounds("settings-git-error").is_some()
        });
        assert!(cx.debug_bounds("settings-git-ok").is_some(), "git answers with its version");
        let passwords = cx.debug_bounds("settings-git-no-credentials");
        assert!(passwords.is_some(), "Passwords says there are no saved credentials");
        type_into(cx, "settings-text-vcs.git.protectedBranches", "main;release/*");
        click(cx, "settings-radio-vcs.git.updateMethod-rebase");
        click(cx, "settings-apply");
        assert_eq!(effective_value(cx, "vcs.git.protectedBranches"), Some(json!("main;release/*")));
        assert_eq!(effective_value(cx, "vcs.git.updateMethod"), Some(json!("rebase")));
    }

    // Tools › Terminal: a variable added to the environment, the cursor shape and the shell path
    // apply; the variables the session already had stay.
    #[gpui_kit::test]
    async fn terminal_environment_cursor_and_shell_apply(cx: &mut TestAppContext) {
        let _home = start(cx);
        let (workbench, cx) = window(cx);
        open_page(cx, &workbench, pages::terminal::PAGE);
        let before =
            effective_value(cx, "terminal.env").and_then(|value| value.as_object().cloned()).unwrap_or_default();
        click(cx, "settings-list-terminal.env-add");
        cx.simulate_input("OPENIDEA_TEST");
        cx.simulate_keystrokes("tab");
        cx.simulate_input("on");
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        let bell = effective_value(cx, "terminal.audibleBell").and_then(|value| value.as_bool()).unwrap_or(false);
        click(cx, "settings-toggle-terminal.audibleBell");
        type_into(cx, "settings-text-terminal.shellPath", "/bin/sh");
        click(cx, "settings-apply");
        let env = effective_value(cx, "terminal.env").and_then(|value| value.as_object().cloned()).unwrap_or_default();
        assert_eq!(env.get("OPENIDEA_TEST"), Some(&json!("on")), "{env:?}");
        assert!(before.keys().all(|key| env.contains_key(key)), "{env:?}");
        assert_eq!(effective_value(cx, "terminal.audibleBell"), Some(json!(!bell)));
        assert_eq!(effective_value(cx, "terminal.shellPath"), Some(json!("/bin/sh")));
    }

    // Notifications: a group set to No popup applies, and its next notification is listed but
    // shows no balloon.
    #[gpui_kit::test]
    async fn notifications_no_popup_applies_and_the_groups_balloon_is_not_shown(cx: &mut TestAppContext) {
        use crate::workbench::notifications::{NotificationLevel, NotificationRequest};
        let _home = start(cx);
        let (workbench, cx) = window(cx);
        open_page(cx, &workbench, pages::notifications::PAGE);
        click(cx, "settings-notification-vcs-display");
        click(cx, "settings-notification-vcs-display-none");
        click(cx, "settings-ok");
        assert_eq!(global_value(cx, "notifications.groups.vcs.display"), Some(json!("none")));
        workbench.update(cx, |workbench, cx| {
            let request = NotificationRequest {
                group: Some("vcs".to_owned()),
                ..NotificationRequest::new(NotificationLevel::Info, "Pushed 1 commit")
            };
            workbench.notify(request, cx);
        });
        cx.run_until_parked();
        let (balloon, listed) = workbench.read_with(cx, |w, _| {
            let notifications = w.notifications();
            let balloon = notifications.balloons(false).iter().any(|n| n.title == "Pushed 1 commit");
            let listed = notifications.logged().any(|n| n.title == "Pushed 1 commit");
            (balloon, listed)
        });
        assert!(!balloon && listed, "balloon {balloon}, listed {listed}");
    }

    // Editor › Font: a family typed and a custom weight apply; a family edit cancelled is not.
    #[gpui_kit::test]
    async fn editor_font_family_and_weight_apply_and_cancel_discards(cx: &mut TestAppContext) {
        let _home = start(cx);
        cx.update(|cx| {
            let fonts = vec![FontFamily { name: "Maple Mono NF".to_owned(), monospaced: true }];
            let mut catalog = SystemFonts::default();
            (catalog.fonts, catalog.status) = (merge_font_families(&fonts), FontStatus::Ready);
            cx.set_global(FontCatalog(catalog));
        });
        let (workbench, cx) = window(cx);
        open_page(cx, &workbench, pages::editor_font::PAGE);
        type_into(cx, "settings-font-editor", "Maple Mono NF");
        cx.simulate_keystrokes("enter");
        click(cx, "settings-select-editor.boldFontWeight");
        click(cx, "settings-choice-editor.boldFontWeight-600");
        click(cx, "settings-apply");
        assert_eq!(global_value(cx, "editor.fontFamily"), Some(json!("Maple Mono NF")));
        assert_eq!(global_value(cx, "editor.boldFontWeight"), Some(json!(600)));
        type_into(cx, "settings-font-editor", "Unsaved Font");
        click(cx, "settings-cancel");
        assert_eq!(global_value(cx, "editor.fontFamily"), Some(json!("Maple Mono NF")));
    }

    // Editor › Code Style: visual guides and a Do not format pattern apply.
    #[gpui_kit::test]
    async fn code_style_visual_guides_and_do_not_format_apply(cx: &mut TestAppContext) {
        let _home = start(cx);
        let (workbench, cx) = window(cx);
        open_page(cx, &workbench, pages::code_style::PAGE);
        type_into(cx, "settings-text-editor.codeStyle.visualGuides", "80, 100");
        click(cx, "settings-list-editor.codeStyle.doNotFormat-add");
        cx.simulate_input("*.min.js");
        cx.simulate_keystrokes("enter");
        click(cx, "settings-apply");
        assert_eq!(global_value(cx, "editor.codeStyle.visualGuides"), Some(json!([80, 100])));
        assert_eq!(global_value(cx, "editor.codeStyle.doNotFormat"), Some(json!(["*.min.js"])));
    }

    // Appearance: an accent swatch applies and the windows draw with it; the zoom and a UI option
    // apply.
    #[gpui_kit::test]
    async fn appearance_accent_zoom_and_ui_options_apply(cx: &mut TestAppContext) {
        use crate::theme::themes::ACCENT_PRESETS;
        let _home = start(cx);
        cx.update(crate::theme::appearance::init);
        let (workbench, cx) = window(cx);
        open_page(cx, &workbench, pages::appearance::PAGE);
        let background =
            global_value(cx, "appearance.differentToolWindowBackground").and_then(|v| v.as_bool()).unwrap_or(false);
        click(cx, "settings-accent-2");
        click(cx, "settings-select-appearance.zoom");
        click(cx, "settings-choice-appearance.zoom-125");
        click(cx, "settings-toggle-appearance.differentToolWindowBackground");
        click(cx, "settings-apply");
        assert_eq!(global_value(cx, "appearance.accent"), Some(json!(ACCENT_PRESETS[2].value)));
        assert_eq!(global_value(cx, "appearance.zoom"), Some(json!(125)));
        assert_eq!(global_value(cx, "appearance.differentToolWindowBackground"), Some(json!(!background)));
        let accent = |cx: &mut VisualTestContext| {
            cx.update(|_, cx| cx.global::<crate::theme::ActiveTheme>().theme.color("accent").map(|c| c.to_hex()))
        };
        settle(cx, |cx| accent(cx).is_some_and(|hex| hex.eq_ignore_ascii_case(ACCENT_PRESETS[2].value)));
    }

    /// What a test does to a control: a click, or text typed over a field.
    enum Act {
        Click(&'static str),
        Type(&'static str, &'static str),
    }

    // On every page, Reset discards an edit of an own control and Cancel after the same edit
    // writes nothing.
    #[gpui_kit::test]
    async fn on_every_page_reset_and_cancel_discard_an_edit_of_an_own_control(cx: &mut TestAppContext) {
        let _home = start(cx);
        let (workbench, cx) = window(cx);
        let cases: [(&str, Act, &str); 12] = [
            (pages::appearance::PAGE, Act::Click("settings-accent-3"), "appearance.accent"),
            (pages::general::PAGE, Act::Click("settings-toggle-general.saveOnIdle"), "general.saveOnIdle"),
            (pages::notifications::PAGE, Act::Click("settings-notification-run-log"), "notifications.groups"),
            (pages::console::PAGE, Act::Click("settings-toggle-console.overrideBuffer"), "console.overrideBuffer"),
            (
                pages::data_views::PAGE,
                Act::Type("settings-number-debugger.valueTooltipDelay", "1000"),
                "debugger.valueTooltipDelay",
            ),
            (
                pages::deployment_options::PAGE,
                Act::Type("settings-text-deployment.exclude", ".idea"),
                "deployment.exclude",
            ),
            (pages::terminal::PAGE, Act::Click("settings-toggle-terminal.audibleBell"), "terminal.audibleBell"),
            (pages::vcs_commit::PAGE, Act::Click("settings-toggle-vcs.commit.limitSubject"), "vcs.commit.limitSubject"),
            (
                pages::vcs_confirmation::PAGE,
                Act::Click("settings-radio-vcs.confirmation.onDelete-never"),
                "vcs.confirmation.onDelete",
            ),
            (pages::vcs_git::PAGE, Act::Click("settings-toggle-vcs.git.warnLargeFiles"), "vcs.git.warnLargeFiles"),
            (pages::editor_font::PAGE, Act::Type("settings-font-editor", "Unsaved Font"), "editor.fontFamily"),
            (
                pages::code_style::PAGE,
                Act::Type("settings-text-editor.codeStyle.visualGuides", "90"),
                "editor.codeStyle.visualGuides",
            ),
        ];
        for (page, act, key) in cases {
            let before = effective_value(cx, key);
            let dialog = open_page(cx, &workbench, page);
            let run = |cx: &mut VisualTestContext| match &act {
                Act::Click(selector) => click(cx, selector),
                Act::Type(selector, text) => type_into(cx, selector, text),
            };
            run(cx);
            assert!(dialog.read_with(cx, |view, cx| view.is_modified(cx)), "{page}: the edit is staged");
            click(cx, "settings-reset");
            assert!(!dialog.read_with(cx, |view, cx| view.is_modified(cx)), "{page}: Reset discards it");
            run(cx);
            click(cx, "settings-cancel");
            assert!(!is_open(&workbench, cx), "{page}: Cancel closes the dialog");
            assert_eq!(effective_value(cx, key), before, "{page}: Cancel writes nothing");
        }
    }

    /// A project opened without a trust decision, in Restricted Mode.
    fn restricted_project(
        cx: &mut TestAppContext,
    ) -> (tempfile::TempDir, PathBuf, Entity<Workbench>, &mut VisualTestContext) {
        let folder = tempfile::tempdir().expect("a temporary folder");
        let root = folder.path().canonicalize().expect("the folder exists");
        std::fs::write(root.join("a.txt"), "a\n").expect("the file is written");
        let (workbench, cx) = window(cx);
        workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
        settle(cx, |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
        assert!(workbench.read_with(cx, |w, cx| w.project_restricted(cx)));
        (folder, root, workbench, cx)
    }

    fn git_executable(
        dialog: &Entity<SettingsDialogView>,
        cx: &mut VisualTestContext,
    ) -> Entity<pages::vcs_git::GitExecutableView> {
        let view = dialog
            .read_with(cx, |view, _| view.custom_control(pages::vcs_git::PAGE, "Path to Git executable").cloned());
        view.and_then(|view| view.downcast().ok()).expect("the Git executable control")
    }

    // Version Control › Git: Test runs nothing in Restricted Mode and says why, as the backend does.
    #[gpui_kit::test]
    async fn git_test_is_refused_in_restricted_mode(cx: &mut TestAppContext) {
        let _home = start(cx);
        let (_folder, _root, workbench, cx) = restricted_project(cx);
        let dialog = open_page(cx, &workbench, pages::vcs_git::PAGE);
        click(cx, "settings-git-test");
        settle(cx, |cx| cx.debug_bounds("settings-git-error").is_some());
        let view = git_executable(&dialog, cx);
        let result = view.read_with(cx, |view, _| view.result().cloned());
        assert_eq!(
            result,
            Some(Err("Project is in Restricted Mode. Trust the project to enable execution.".to_owned()))
        );
        assert!(cx.debug_bounds("settings-git-ok").is_none());
    }

    // Version Control › Git: the field's placeholder names the git found on the PATH, looked up
    // after the dialog is shown; Test without a path runs that one.
    #[gpui_kit::test]
    async fn git_is_detected_after_the_dialog_opens_and_test_runs_it(cx: &mut TestAppContext) {
        let _home = start(cx);
        let (_folder, _root, workbench, cx) = project(cx);
        let dialog = open_page(cx, &workbench, pages::vcs_git::PAGE);
        let view = git_executable(&dialog, cx);
        let placeholder =
            |cx: &mut VisualTestContext| view.read_with(cx, |view, cx| view.field().read(cx).placeholder().to_owned());
        settle(cx, |cx| placeholder(cx).starts_with("Auto-detected: "));
        let detected = placeholder(cx).trim_start_matches("Auto-detected: ").to_owned();
        click(cx, "settings-git-test");
        settle(cx, |cx| view.read_with(cx, |view, _| view.result().is_some()));
        let result = view.read_with(cx, |view, _| view.result().cloned()).expect("an answer");
        let line = result.expect("git answers");
        assert!(line.contains(&detected), "{line} names {detected}");
    }

    // Tools › Terminal: the shell field's placeholder becomes the login shell once it is read.
    #[gpui_kit::test]
    async fn the_login_shell_is_read_after_the_dialog_opens(cx: &mut TestAppContext) {
        let _home = start(cx);
        let (workbench, cx) = window(cx);
        let dialog = open_page(cx, &workbench, pages::terminal::PAGE);
        let view = dialog.read_with(cx, |view, _| view.custom_control(pages::terminal::PAGE, "Shell path").cloned());
        let view: Entity<pages::terminal::ShellPathView> =
            view.and_then(|view| view.downcast().ok()).expect("the Shell path control");
        let expected = ide_terminal::default_shell_path();
        settle(cx, |cx| view.read_with(cx, |view, cx| view.field().read(cx).placeholder() == expected));
    }

    /// What a tracing subscriber writes, kept to be read back.
    #[derive(Clone, Default)]
    struct Captured(std::sync::Arc<parking_lot::Mutex<Vec<u8>>>);

    impl std::io::Write for Captured {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Captured {
        type Writer = Captured;

        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    // Version Control › Git › Passwords: a credential git saved after a successful command is
    // listed by its host; Forget removes it from the store under the temporary home. The secret is
    // never drawn or logged.
    #[gpui_kit::test]
    async fn passwords_lists_a_saved_credential_and_forget_removes_it(cx: &mut TestAppContext) {
        const SECRET: &str = "pw5f3a9ctest";
        let log = Captured::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(log.clone())
            .with_max_level(tracing::Level::TRACE)
            .with_ansi(false)
            .finish();
        let _logging = tracing::subscriber::set_default(subscriber);
        let home = start(cx);
        assert!(ide_ssh::secrets::secrets_in_files(), "a temporary home keeps its secrets in files");
        let store = cx.update(|cx| cx.global::<Backend>().git_credentials().cloned()).expect("the store");
        store.answered(7, "Username for 'https://git.example.test': ", "me", true);
        store.answered(7, "Password for 'https://me@git.example.test': ", SECRET, true);
        store.command_finished(7, true, "");
        assert_eq!(store.list(), ["https://git.example.test"]);
        assert!(!ide_ssh::secrets::default_store_installed(), "the system's credential store is never set up");
        let kept = |home: &std::path::Path| {
            walk(home)
                .into_iter()
                .filter_map(|file| std::fs::read(&file).ok())
                .any(|bytes| String::from_utf8_lossy(&bytes).contains(SECRET))
        };
        assert!(kept(home.path()), "the credential is kept in a file under the temporary home");
        let (workbench, cx) = window(cx);
        let dialog = open_page(cx, &workbench, pages::vcs_git::PAGE);
        let view = dialog.read_with(cx, |view, _| view.custom_control(pages::vcs_git::PAGE, "Passwords").cloned());
        let view: Entity<pages::vcs_git::GitCredentialsView> =
            view.and_then(|view| view.downcast().ok()).expect("the Passwords control");
        settle(cx, |cx| !view.read_with(cx, |view, _| view.hosts().is_empty()));
        let lines = view.read_with(cx, |view, cx| view.lines(cx));
        assert_eq!(lines, ["Passwords:", "  https://git.example.test [Forget]"]);
        // Passwords is the last row of the page: the page is scrolled to its end first.
        scroll_page(cx, "settings-git-test", 2000.0);
        click(cx, "settings-git-forget-0");
        settle(cx, |cx| view.read_with(cx, |view, _| view.hosts().is_empty()));
        assert!(store.credential("https://git.example.test").is_none(), "the store forgot it");
        assert!(cx.debug_bounds("settings-git-no-credentials").is_some());
        let saved: Vec<String> = walk(home.path())
            .into_iter()
            .filter_map(|file| std::fs::read(&file).ok())
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .collect();
        assert!(!saved.iter().any(|text| text.contains(SECRET)), "no file under the home keeps the secret");
        let logged = String::from_utf8_lossy(&log.0.lock()).into_owned();
        assert!(!logged.contains(SECRET), "the log names the secret");
    }

    /// Scrolls the page under the control `at` by `distance` pixels towards its end.
    fn scroll_page(cx: &mut VisualTestContext, at: &'static str, distance: f32) {
        use gpui_kit::{ScrollDelta, ScrollEvent, TouchPhase, point, px};
        cx.run_until_parked();
        let position = cx.debug_bounds(at).unwrap_or_else(|| panic!("{at} is drawn")).center();
        let delta = ScrollDelta::Pixels(point(px(0.0), px(-distance)));
        cx.simulate_event(ScrollEvent {
            position,
            delta,
            modifiers: Modifiers::default(),
            touch_phase: TouchPhase::Moved,
            source: gpui_kit::ScrollSource::Pointer,
        });
        cx.run_until_parked();
    }

    /// Every file under `dir`.
    fn walk(dir: &std::path::Path) -> Vec<PathBuf> {
        let mut files = Vec::new();
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                files.extend(walk(&path));
            } else {
                files.push(path);
            }
        }
        files
    }

    // Editor › Font: the family, size, line height, ligatures and main weight applied are what the
    // open editor draws its text with; a family that is not installed gives way to the fallback font.
    #[gpui_kit::test]
    async fn the_editor_draws_with_the_font_applied_on_the_font_page(cx: &mut TestAppContext) {
        use crate::editor::view::InstalledFontNames;
        let _home = start(cx);
        // The families the machine has, as far as the editor knows: given here, never read.
        cx.update(|cx| cx.set_global(InstalledFontNames(vec!["Test Mono".to_owned(), "Test Fallback".to_owned()])));
        let (_folder, root, workbench, cx) = project(cx);
        workbench.update_in(cx, |w, window, cx| w.open_project_file(root.join("a.txt"), window, cx));
        settle(cx, |cx| workbench.read_with(cx, |w, _| w.editor().is_some()));
        let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");
        let (font, size, line_height) = editor.read_with(cx, |editor, cx| editor.text_style(cx));
        assert_eq!((f32::from(size), f32::from(line_height)), (13.0, 22.0), "the defaults");
        assert_eq!(font.features.is_calt_enabled(), Some(false), "no ligatures by default");
        assert_eq!(font.weight, gpui_kit::FontWeight(400.0));
        assert_eq!(font.family.as_ref(), "Menlo", "no family of the theme is installed");
        let other = "Test Mono";

        open_page(cx, &workbench, pages::editor_font::PAGE);
        type_into(cx, "settings-font-editor", other);
        cx.simulate_keystrokes("enter");
        type_into(cx, "settings-number-editor.fontSize", "15");
        type_into(cx, "settings-number-editor.lineHeight", "24");
        click(cx, "settings-toggle-editor.ligatures");
        click(cx, "settings-select-editor.fontWeight");
        click(cx, "settings-choice-editor.fontWeight-500");
        click(cx, "settings-apply");
        assert_eq!(effective_value(cx, "editor.fontFamily"), Some(json!(other)));
        settle(cx, |cx| editor.read_with(cx, |editor, cx| f32::from(editor.text_style(cx).1) == 15.0));
        let (font, size, line_height) = editor.read_with(cx, |editor, cx| editor.text_style(cx));
        assert_eq!(font.family.as_ref(), other);
        assert_eq!((f32::from(size), f32::from(line_height)), (15.0, 24.0));
        assert_ne!(font.features.is_calt_enabled(), Some(false), "ligatures are on");
        assert_eq!(font.weight, gpui_kit::FontWeight(500.0));

        // A family that is not installed: the fallback font draws the text.
        type_into(cx, "settings-font-editor", "No Such Family Anywhere");
        cx.simulate_keystrokes("enter");
        scroll_page(cx, "settings-font-editor", 2000.0);
        type_into(cx, "settings-font-editor-fallback", "Test Fallback");
        cx.simulate_keystrokes("enter");
        click(cx, "settings-apply");
        assert_eq!(effective_value(cx, "editor.fallbackFont"), Some(json!("Test Fallback")));
        settle(cx, |cx| {
            editor.read_with(cx, |editor, cx| editor.text_style(cx).0.family.as_ref() == "Test Fallback")
                && effective_value(cx, "editor.fontFamily") == Some(json!("No Such Family Anywhere"))
        });
    }

    // System Settings: a stored "attach" (an older choice) for Open project in shows Ask, and
    // showing it stages nothing.
    #[gpui_kit::test]
    async fn a_stored_attach_for_open_project_in_shows_ask(cx: &mut TestAppContext) {
        use crate::platform::settings::{SettingsWrite, set_global};
        let _home = start(cx);
        let (workbench, cx) = window(cx);
        cx.update(|_, cx| {
            let write = SettingsWrite { key: "general.openProjectIn", value: json!("attach") };
            set_global("w1", &[write], cx).expect("the value is stored");
        });
        assert_eq!(global_value(cx, "general.openProjectIn"), Some(json!("attach")));
        let dialog = open_page(cx, &workbench, pages::general::PAGE);
        let form =
            dialog.read_with(cx, |view, _| view.custom_control(pages::general::PAGE, "Open project in").cloned());
        let form: Entity<FormView> = form.and_then(|view| view.downcast().ok()).expect("the form");
        let lines = form.read_with(cx, |form, cx| form.lines(cx));
        assert_eq!(lines, ["Open project in: ( ) New window ( ) Current window (•) Ask"]);
        assert!(!dialog.read_with(cx, |view, cx| view.is_modified(cx)));
    }

    // The installed fonts are not read when Settings opens: the first page with a font picker
    // reads them, off the window's thread, and its picker says what came back (test builds read
    // none).
    #[gpui_kit::test]
    async fn the_installed_fonts_are_read_once_a_font_picker_is_shown(cx: &mut TestAppContext) {
        let _home = start(cx);
        cx.update(|cx| cx.remove_global::<FontCatalog>());
        let (workbench, cx) = window(cx);
        open_page(cx, &workbench, pages::general::PAGE);
        let status = |cx: &mut VisualTestContext| cx.update(|_, cx| cx.try_global::<FontCatalog>().map(|c| c.0.status));
        assert_eq!(status(cx), Some(FontStatus::Idle), "opening Settings reads no fonts");
        let dialog = open_page(cx, &workbench, pages::editor_font::PAGE);
        settle(cx, |cx| status(cx) == Some(FontStatus::Empty));
        let picker = dialog.read_with(cx, |view, _| view.custom_control(pages::editor_font::PAGE, "Font").cloned());
        let picker: Entity<FontPicker> = picker.and_then(|view| view.downcast().ok()).expect("the font picker");
        let said = picker.read_with(cx, |picker, cx| picker.status(cx));
        assert_eq!(said, "No system fonts were returned. Bundled fonts and manual input are available.");
        let fallback =
            dialog.read_with(cx, |view, _| view.custom_control(pages::editor_font::PAGE, "Fallback font").cloned());
        let fallback: Entity<FontPicker> = fallback.and_then(|view| view.downcast().ok()).expect("the fallback picker");
        let lines = fallback.read_with(cx, |picker, cx| picker.lines(cx));
        assert!(lines.contains(&"    For symbols not supported by the main font".to_owned()), "{lines:?}");
    }

    // A whole number typed into a setting stored as a fraction (the font size, 13.0) that equals
    // it leaves nothing staged and the page unmodified.
    #[gpui_kit::test]
    async fn typing_the_stored_size_as_a_whole_number_leaves_the_page_unmodified(cx: &mut TestAppContext) {
        let _home = start(cx);
        let (workbench, cx) = window(cx);
        let stored = effective_value(cx, "editor.fontSize").expect("a size");
        assert!(stored.is_f64(), "the backend stores the size as a fraction: {stored}");
        let dialog = open_page(cx, &workbench, pages::editor_font::PAGE);
        type_into(cx, "settings-number-editor.fontSize", "14");
        assert!(dialog.read_with(cx, |view, cx| view.is_modified(cx)));
        type_into(cx, "settings-number-editor.fontSize", "13");
        assert!(!dialog.read_with(cx, |view, cx| view.is_modified(cx)), "13 is the stored 13.0");
        assert!(!dialog.read_with(cx, |view, cx| view.can_apply(cx)));
    }

    // The search finds a page by a label of its own controls and marks it there, and the tree
    // marks a page whose own control was edited as modified.
    #[gpui_kit::test]
    async fn the_search_and_the_modified_marker_treat_own_controls_like_standard_ones(cx: &mut TestAppContext) {
        let _home = start(cx);
        let (workbench, cx) = window(cx);
        let dialog = open_page(cx, &workbench, pages::vcs_commit::PAGE);
        click(cx, "settings-toggle-vcs.commit.limitSubject");
        let modified = dialog.read_with(cx, |view, cx| view.modified_pages(cx));
        assert!(modified.contains(pages::vcs_commit::PAGE), "{modified:?}");
        let search = dialog.read_with(cx, |view, _| view.search_field().clone());
        cx.update(|window, cx| window.focus(&search.focus_handle(cx), cx));
        cx.simulate_input("Wrap when typing");
        cx.run_until_parked();
        let pages = dialog.read_with(cx, |view, cx| view.pages(cx));
        assert!(pages.iter().any(|page| page.trim() == "Commit"), "{pages:?}");
        let form =
            dialog.read_with(cx, |view, _| view.custom_control(pages::vcs_commit::PAGE, "Limit subject line").cloned());
        let form: Entity<FormView> = form.and_then(|view| view.downcast().ok()).expect("the form");
        assert!(form.read_with(cx, |form, cx| form.binding().matches("Wrap when typing reaches right margin", cx)));
        assert!(!form.read_with(cx, |form, cx| form.binding().matches("Show right margin", cx)));
    }
}
