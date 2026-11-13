# Integrating `workbench/app_shell.rs`

`app-gpui/src/workbench/app_shell.rs` is written and is not compiled yet: nothing declares it. The edits below wire it in. Every edit is anchored on lines that occur once in the file. Apply them in this order: 1 (prompt_dialogs.rs), 2 (mod.rs), 3 (windows.rs), 4 (menu.rs), then 5 (tests).

Nothing was compiled. The riskiest spots are listed in section 7.

---

## 1. `app-gpui/src/vcs/prompt_dialogs.rs`: a message box whose texts are already translated

`PromptDialog::ask` takes an `AskRequest` whose `AskButton.label` is a `&'static str` of English source text. The app models' prompts (`close_guard::Prompt`) arrive translated through the keyed catalog, with `String` labels. Add one constructor. `Spec` and `build` are private, so it has to live in this file.

Anchor: insert directly after the closing `}` of `pub fn ask(request: AskRequest, window: &mut Window, cx: &mut Context<Self>) -> Self { … }`, before `fn build(spec: Spec, …`:

```rust
    /// A message box whose title, message, checkbox and buttons are in the UI language already
    /// (the application shell's close and open questions); Escape answers [`CANCEL`].
    pub fn message(
        title: String,
        body: String,
        warning: bool,
        checkbox: Option<String>,
        buttons: Vec<DialogButton>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let spec = Spec {
            title,
            message: Some(body),
            warning,
            width: WIDTH,
            field: None,
            checkbox: checkbox.map(|label| (label, false)),
            buttons,
        };
        Self::build(spec, window, cx)
    }
```

It takes seven arguments, which is clippy's limit for `too_many_arguments`. The render pass runs `tx` on these texts; text that is not in the source-text catalog comes back unchanged.

---

## 2. `app-gpui/src/workbench/mod.rs`

### 2a. Declare the module

Anchor (line 4):

```rust
mod bookmarks_window;
```

Replace with:

```rust
mod app_shell;
mod bookmarks_window;
```

### 2b. The workbench's fields: the close requests and the dialog

Anchor:

```rust
    /// The unsaved-changes question is on screen.
    closing: bool,
```

Replace with:

```rust
    /// The window's close requests: one is decided at a time.
    close_requests: crate::app_models::close_guard::CloseRequests,
    /// The close or open question shown over the window.
    app_dialog: Option<app_shell::AppDialog>,
```

Anchor (in `Workbench::new`, the struct literal):

```rust
            previous_editor: None,
            closing: false,
```

Replace with:

```rust
            previous_editor: None,
            close_requests: Default::default(),
            app_dialog: None,
```

### 2c. The window's close hook

Anchor:

```rust
        // Closing the window asks about unsaved changes first.
        let guard = cx.weak_entity();
        window.on_window_should_close(cx, move |window, cx| {
            guard.update(cx, |workbench, cx| workbench.confirm_close(window, cx)).unwrap_or(true)
        });
```

Replace with:

```rust
        // Closing the window is decided first: the exit, the running processes, the unsaved documents.
        let guard = cx.weak_entity();
        window.on_window_should_close(cx, move |window, cx| app_shell::window_should_close(&guard, window, cx));
```

The hook must not run inside `guard.update`. `window_should_close` reads every window's workbench, this one included, and reading an entity that is being updated panics (double lease).

### 2d. Delete `confirm_close`

Delete the whole method, from:

```rust
    /// Whether the window may close now. With unsaved changes it may not: the user is asked to
    /// save them, discard them or stay, and the window closes once that is settled.
    pub fn confirm_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
```

through its closing lines:

```rust
            if close {
                let _ = cx.update(|window, _| window.remove_window());
            }
        })
        .detach();
        false
    }
```

Keep the method that follows it, `/// Background tasks of this window, in listing order.` `pub fn tasks(&self) -> &Tasks {`.

These stay as they are, as unguarded building blocks that `app_shell.rs` calls: `save_all`, `unsaved_count`, `auto_save`, `autosave`, `report_save_failure`, `leave`, `close_project`, `open_project`, `open_file`, `is_empty` and `restore_unsaved`. The tab-close question in `tabs_changed` stays on `window.prompt`. `leave` keeps its `autosave(None, cx)`: it does nothing with autosave off, and with autosave on the guard has already saved.

### 2e. Draw the dialog

Anchor (in `impl Render for Workbench`):

```rust
            .children(self.vcs_dialog_layer())
```

Replace with:

```rust
            .children(self.vcs_dialog_layer())
            .children(self.app_dialog_layer())
```

The layer is drawn at priority 2, over a Git dialog (priority 1). A close question can then come up while a Git dialog is open without cancelling the Git operation, which `show_prompt_dialog` would do.

No other line of mod.rs changes. `WelcomeEvent::Open` keeps calling `windows::open_project_with_policy`, which now hands over to `app_shell`.

---

## 3. `app-gpui/src/workbench/windows.rs`

### 3a. Imports

Anchor:

```rust
use crate::platform::i18n::Localization;
use crate::platform::settings;
use gpui_kit::{AnyWindowHandle, App, AppContext as _, PathPromptOptions, PromptLevel, Window};
```

Replace with:

```rust
use crate::platform::i18n::Localization;
use gpui_kit::{AnyWindowHandle, App, AppContext as _, PathPromptOptions, Window};
```

### 3b. `open_project_with_policy`: hand over to the model

Replace the whole function, from:

```rust
/// Opens the project at `path` where "Open project in" says: a window that shows nothing takes
/// it; one with a project asks, or follows the stored answer.
pub fn open_project_with_policy(path: &Path, window: &mut Window, cx: &mut App) -> anyhow::Result<()> {
```

through its closing `}` (just before `/// Opens loose \`files\` in \`window\`…`), with:

```rust
/// Opens the project at `path` where "Open project in" says: a window that shows nothing takes
/// it; one with a project asks (with "Don't ask again"), or follows the stored answer. Opening it
/// in this window first settles leaving the project it shows.
pub fn open_project_with_policy(path: &Path, window: &mut Window, cx: &mut App) -> anyhow::Result<()> {
    super::app_shell::open_project_with_policy(path, Vec::new(), window, cx)
}
```

### 3c. `open_files`: hand over to the model

Replace the whole function, from:

```rust
/// Opens loose `files` in `window`: as tabs of its project, or the last one for editing on its
/// own when the window shows nothing; a window editing a loose file opens them in a new window.
pub fn open_files(files: Vec<PathBuf>, window: &mut Window, cx: &mut App) -> anyhow::Result<()> {
```

through its closing `}` (just before `/// Asks for a folder (or files) and hands what was picked…`), with:

```rust
/// Opens loose `files` in `window`: as tabs of its project; otherwise each file of a project open
/// in another window goes there, and every other file is edited on its own, the first in this
/// window when it shows nothing.
pub fn open_files(files: Vec<PathBuf>, window: &mut Window, cx: &mut App) -> anyhow::Result<()> {
    super::app_shell::open_files(files, window, cx)
}
```

### 3d. `file.openProjectPath` takes `{path, newWindow}` too

Anchor:

```rust
    registry.handle("file.openProjectPath", |call, window, cx| match path_argument(call.arg) {
        Some(path) => open_project_with_policy(&path, window, cx),
        None => Ok(()),
    });
```

Replace with:

```rust
    registry.handle("file.openProjectPath", |call, window, cx| match call.arg {
        // An explicit choice of window: `newWindow` as asked, whatever "Open project in" says.
        Some(serde_json::Value::Object(request)) => match request.get("path").and_then(serde_json::Value::as_str) {
            Some(path) => {
                let new_window = request.get("newWindow").and_then(serde_json::Value::as_bool).unwrap_or(false);
                super::app_shell::open_project_explicit(Path::new(path), new_window, window, cx)
            }
            None => Ok(()),
        },
        arg => match path_argument(arg) {
            Some(path) => open_project_with_policy(&path, window, cx),
            None => Ok(()),
        },
    });
```

### 3e. `file.closeProject` goes through the leave guard

Anchor:

```rust
    registry.handle("file.closeProject", |_, window, cx| {
        if let Some(workbench) = for_window(window, cx) {
            workbench.update(cx, |workbench, cx| workbench.close_project(window, cx));
        }
        Ok(())
    });
```

Replace with:

```rust
    registry.handle("file.closeProject", |_, window, cx| {
        super::app_shell::request_close_project(window, cx);
        Ok(())
    });
```

These stay unchanged: `open_window`, `project_window`, `canonical`, `open_project` (still used by `file.openInNewWindow` and as the fallback for a window without a workbench), `pick`, `path_argument`, `window.next` / `window.previous` / `window.toggleFullScreen`, and the unit tests.

---

## 4. `app-gpui/src/workbench/menu.rs`: quit

Anchor:

```rust
    registry.handle("app.quit", |_, _, cx| {
        // Each window is asked first; one with unsaved changes keeps the app open while it asks.
        let windows = cx.windows();
        let mut all = true;
        for window in windows {
            let closes = window
                .update(cx, |_, window, cx| {
                    super::for_window(window, cx).is_none_or(|workbench| {
                        workbench.update(cx, |workbench, cx| workbench.confirm_close(window, cx))
                    })
                })
                .unwrap_or(true);
            all &= closes;
        }
        if all {
            cx.quit();
        }
        Ok(())
    });
```

Replace with:

```rust
    registry.handle("app.quit", |_, window, cx| {
        // The exit is confirmed once, then every window decides its own close; one that stays calls the quit off.
        super::app_shell::quit(window, cx);
        Ok(())
    });
```

---

## 5. Tests

### 5a. Existing tests that open projects or files, close windows or projects, or answer a close prompt

Two tests must change, both in `workbench/mod.rs`. Every other test in the table passes unchanged.

| Test (file)                                                                                                                                                                                                                                                                                                                                                                                                                                                  | Verdict                                                   | Why / what changes                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | --------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `closing_a_window_asks_about_unsaved_changes` (workbench/mod.rs)                                                                                                                                                                                                                                                                                                                                                                                             | **must change**: replace it with `saves_all_on_save` (5b) | It runs with the default `files.autoSave: true` and expects the question; with autosave on the document is now saved without one (as the brief asks; not from a spec, see 7). Its one window has a project, so a close returns it to the Welcome screen instead of closing (`CloseRequests::request` → `CloseRoute::ToWelcome`, from `closeGuard.ts` `onCloseRequested`: "Closing the last project (or LightEdit) window closes its project and shows the welcome screen"). So `assert!(confirm_close(..))`, which no longer exists, and `settle(cx, \|cx\| cx.windows().is_empty())` no longer hold. The question now appears after `run_until_parked` (the decision is a task). |
| `projects_open_in_this_window_a_new_one_or_the_one_that_has_them` (workbench/mod.rs)                                                                                                                                                                                                                                                                                                                                                                         | **must change**: 5c                                       | The where-to-open question has a "Don't ask again" checkbox, so it is a `PromptDialog`, not `window.prompt`: `has_pending_prompt` / `simulate_prompt_answer` become `answer_app_dialog`. A project open in another window is asked about first, then focused (`windows.ts` `openProjectWithPolicy` calls `chooseProjectWindow` before `windowOpenProject`; `windows.test.ts:168` "does nothing locally when another window was focused").                                                                                                                                                                                                                                         |
| `without_autosave_closing_an_unsaved_tab_asks` (workbench/mod.rs)                                                                                                                                                                                                                                                                                                                                                                                            | passes                                                    | Closing a tab (`tabs_changed`) is untouched and stays on `window.prompt`.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| `a_window_with_nothing_open_welcomes_with_the_recent_projects` (workbench/mod.rs)                                                                                                                                                                                                                                                                                                                                                                            | passes                                                    | It calls `Workbench::close_project` / `open_project` directly, and those stay unguarded. Enter or a click on a row opens through `OpenFlow::with_policy` on an empty window: `in_window`, no question, a task that `settle` drives. Its "Remove" prompt belongs to the Welcome view.                                                                                                                                                                                                                                                                                                                                                                                              |
| `the_code_insight_actions_and_events_run_in_a_window_with_a_project`, `a_projects_tabs_and_carets_are_restored`, `the_bookmarks_window_lists_opens_renames_and_deletes_what_the_project_stores`, `tool_window_actions_show_focus_and_hide_the_project_view`, `files_open_in_tabs_that_the_keymap_cycles_closes_and_reopens`, `a_project_shows_its_tree_and_opens_files_in_the_editor`, `a_tab_that_cannot_be_saved_reopens_with_its_text` (workbench/mod.rs) | pass                                                      | Direct calls of the unguarded `close_project` / `open_project`.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| Every test that opens through `open_project`, `trusted_project`, `search_project`, `run_project` or `git_project` (about 60 in workbench/mod.rs; also `editor/intelligence.rs` `open`, `plugin_models/plugins_page.rs`, `workbench/plugins_window.rs`, `workbench/settings_window.rs`)                                                                                                                                                                       | pass                                                      | `Workbench::open_project` is unchanged and never goes through the guard.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| Tests that call `host.close_project("w1")` (backend.rs, project_tree.rs, everywhere/_, vcs/_, editor/*, find/popup_view.rs `close_project`)                                                                                                                                                                                                                                                                                                                  | pass                                                      | Backend calls, not the workbench's.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| `opens_a_loose_file_in_an_editor_and_types_into_it`, `the_keymap_runs_editor_actions_on_the_windows_editor`, `a_single_file_window_opens_the_find_bar_over_its_editor` (workbench/mod.rs)                                                                                                                                                                                                                                                                    | pass                                                      | They call `Workbench::open_file` directly, which is unchanged.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| `run/driver.rs` `asks_before_terminating_a_running_process_and_keeps_the_tab_when_cancelled`, `closes_a_finished_session_without_asking`                                                                                                                                                                                                                                                                                                                     | pass                                                      | The Run tab's own question (`RunDriver::close_tab`) is untouched.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| `welcome.rs` `moves_with_arrows_filters_by_typing_opens_with_enter_and_removes_with_delete`, `a_search_that_matches_nothing_says_so_and_can_be_cleared`, `groups_list_their_projects_and_fold_with_the_keyboard_and_the_pointer`, `the_rows_of_a_menu_open_show_copy_regroup_and_remove`, `offers_open_in_new_window_in_the_row_menu_when_the_action_exists`                                                                                                 | pass                                                      | They test the Welcome view's own prompts and events, with no workbench involved.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| `welcome_model.rs` `a_projects_menu_opens_shows_copies_groups_and_removes_it`, `menu_spec.rs`, `title_bar.rs` (`file.closeProject` items)                                                                                                                                                                                                                                                                                                                    | pass                                                      | Menu tables only; the action ids are unchanged.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| `find/popup_view.rs` `replace_all_asks_first_when_several_files_change`, `replace_all_in_one_file_does_not_ask`                                                                                                                                                                                                                                                                                                                                              | pass                                                      | The Replace All prompt is unrelated.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              |
| `platform_more/project.rs` `shows_the_target_while_opening_…`, `leaves_the_current_project_alone_when_trust_is_cancelled`                                                                                                                                                                                                                                                                                                                                    | pass                                                      | A separate model (`platform_more::project::OpenStep`), not touched.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| `workbench/windows.rs` unit tests (`labels_sort_…`, `next_label_fills_gaps`, `cycling_wraps_…`)                                                                                                                                                                                                                                                                                                                                                              | pass                                                      | Pure functions, unchanged.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| `app_models/windows.rs`, `app_models/close_guard.rs` model tests                                                                                                                                                                                                                                                                                                                                                                                             | pass                                                      | The models are unchanged.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |

Total: 2 must change; the rest (about 120 tests across the files named) pass unchanged.

### 5b. Replace `closing_a_window_asks_about_unsaved_changes`

Delete the whole test, from:

```rust
    // src/features/app/closeGuard.ts › decideClose (the unsaved-changes question)
    #[gpui_kit::test]
    async fn closing_a_window_asks_about_unsaved_changes(cx: &mut TestAppContext) {
```

through its last lines:

```rust
        settle(cx, |_| std::fs::read_to_string(root.join("notes.txt")).is_ok_and(|text| text == "my notes\n"));
        settle(cx, |cx| cx.windows().is_empty());
    }
```

and paste in its place the helpers and new tests (a) to (f) below.

```rust
    /// The title of the close or open question shown over `workbench`'s window.
    fn app_dialog_title(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<String> {
        workbench.read_with(cx, |w, cx| w.app_dialog().map(|dialog| dialog.read(cx).title().to_owned()))
    }

    /// Answers the close or open question shown over the window `handle` with `button`, its "Don't
    /// ask again" checkbox `checked`, as the dialog itself would.
    fn answer_app_dialog(cx: &mut TestAppContext, handle: gpui_kit::AnyWindowHandle, button: &str, checked: bool) {
        use crate::vcs::prompt_dialogs::{DialogAnswer, PromptDialogEvent};
        let dialog = handle
            .update(cx, |_, window, cx| for_window(window, cx).and_then(|w| w.read(cx).app_dialog().cloned()))
            .expect("the window is open")
            .expect("a question is shown");
        let answer = DialogAnswer { button: button.to_owned(), checked, ..DialogAnswer::default() };
        dialog.update(cx, |_, cx| cx.emit(PromptDialogEvent::Closed(answer)));
        cx.run_until_parked();
    }

    /// Whether a close or open question is shown over the window `handle`.
    fn shows_app_dialog(cx: &mut TestAppContext, handle: gpui_kit::AnyWindowHandle) -> bool {
        handle
            .update(cx, |_, window, cx| for_window(window, cx).is_some_and(|w| w.read(cx).app_dialog().is_some()))
            .expect("the window is open")
    }

    // src/features/app/__tests__/closeGuard.test.ts › decideClose › asks before terminating a running process
    /// Closing the last window while a run configuration's process runs asks to terminate it, naming
    /// it: Cancel keeps the project and the process; Terminate stops the process, and the window,
    /// the last one with a project, shows the Welcome screen instead of closing.
    #[gpui_kit::test]
    async fn asks_before_terminating_a_running_process(cx: &mut TestAppContext) {
        use crate::platform::actions::execute_action_or_throw;
        use crate::run::configs::blank_config;
        let _home = start(cx);
        let folder = tempfile::tempdir().expect("a temporary folder");
        let root = folder.path().canonicalize().expect("the folder exists");
        let nap = ide_run::RunConfig {
            command: "/bin/sleep".to_owned(),
            args: vec!["30".to_owned()],
            ..blank_config("nap", "nap", "program")
        };
        ide_run::save_user_configs(&root, &[nap]).expect("the configuration is saved");
        cx.update(|cx| cx.global::<Backend>().host.trust.decide(&root, true, false)).expect("the project is trusted");
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
        settle(cx, |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
        let driver = workbench.read_with(cx, |w, _| w.run_driver().cloned()).expect("the project runs things");
        let store = driver.read_with(cx, |driver, _| driver.store().clone());
        // Processes and their timers: real time passes, and so does the executor's clock.
        let long = |cx: &mut VisualTestContext, what: &str, done: &dyn Fn(&mut VisualTestContext) -> bool| {
            for _ in 0..3000 {
                cx.executor().advance_clock(Duration::from_millis(20));
                cx.run_until_parked();
                if done(cx) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            panic!("{what} did not happen within thirty seconds");
        };
        long(cx, "listing the configurations", &|cx| store.read_with(cx, |store, _| store.loaded));
        store.update(cx, |store, _| store.select_config(Some("nap")));
        cx.update(|window, cx| execute_action_or_throw("run.run", None, window, cx)).expect("it runs");
        let running = |cx: &mut VisualTestContext| {
            store.read_with(cx, |store, _| store.sessions.iter().any(|s| s.is_live() && s.run_id.is_some()))
        };
        long(cx, "the start of sleep", &running);

        // Cancel: the project and the process stay.
        assert!(!cx.simulate_close());
        cx.run_until_parked();
        assert_eq!(app_dialog_title(&workbench, cx).as_deref(), Some("Process 'nap' Is Running"));
        click_on(cx, "prompt-button-cancel");
        assert_eq!(app_dialog_title(&workbench, cx), None);
        assert!(workbench.read_with(cx, |w, _| w.project_tree().is_some()));
        assert!(running(cx));

        // Terminate: the process is stopped and waited for, then the project closes.
        assert!(!cx.simulate_close());
        cx.run_until_parked();
        click_on(cx, "prompt-button-terminate");
        long(cx, "the end of sleep and the Welcome screen", &|cx| {
            !running(cx) && workbench.read_with(cx, |w, _| w.welcome().is_some())
        });
        assert!(!cx.windows().is_empty(), "the last window stays, showing the Welcome screen");
    }

    // src/features/app/__tests__/closeGuard.test.ts › decideClose › saves all on Save
    /// With Files › Autosave off, closing the last window with an unsaved document asks Save All /
    /// Don't Save / Cancel: Cancel keeps the project and the text, Save All writes the file and the
    /// window shows the Welcome screen. With autosave on the document is saved without a question.
    #[gpui_kit::test]
    async fn saves_all_on_save(cx: &mut TestAppContext) {
        use ide_workspace::settings::SettingsScope;
        let _home = start(cx);
        let folder = tempfile::tempdir().expect("a temporary folder");
        let root = folder.path().canonicalize().expect("the folder exists");
        std::fs::write(root.join("notes.txt"), "notes\n").expect("the file is written");
        // A file of its own for the autosave part: the project reopens notes.txt with its caret where it was left.
        std::fs::write(root.join("draft.txt"), "draft\n").expect("the file is written");
        let autosave = |on: bool, cx: &mut App| {
            crate::platform::settings::set("w1", SettingsScope::Global, "files.autoSave", &on.into(), cx)
                .expect("the setting is stored");
        };
        cx.update(|cx| autosave(false, cx));
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        let open = |name: &str, cx: &mut VisualTestContext| {
            workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
            settle(cx, |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
            let file = root.join(name);
            workbench.update_in(cx, |workbench, window, cx| workbench.open_project_file(file.clone(), window, cx));
            settle(cx, |cx| {
                workbench.read_with(cx, |w, cx| w.editor().is_some_and(|editor| editor.read(cx).path() == file))
            });
        };
        let on_disk = |name: &str| std::fs::read_to_string(root.join(name)).expect("readable");
        let welcomes = |cx: &mut VisualTestContext| workbench.read_with(cx, |w, _| w.welcome().is_some());

        // Nothing unsaved: nothing is asked, and the last window shows the Welcome screen.
        workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
        settle(cx, |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
        assert!(!cx.simulate_close());
        cx.run_until_parked();
        assert!(!cx.has_pending_prompt());
        assert!(welcomes(cx));

        open("notes.txt", cx);
        cx.simulate_input("my ");
        // Typed a moment ago: the document counts as unsaved before the backend has said so.
        assert_eq!(workbench.read_with(cx, |w, cx| w.unsaved_count(cx)), 1);
        assert!(!cx.simulate_close());
        cx.run_until_parked();
        let (title, body) = cx.pending_prompt().expect("the question is asked");
        assert_eq!(title, "Unsaved Changes");
        let project = root.file_name().expect("a name").to_string_lossy().into_owned();
        assert_eq!(body, format!("1 file has unsaved changes in {project}. Save before closing?"));
        // Cancel: the project and the text stay, nothing is written.
        cx.simulate_prompt_answer("Cancel");
        cx.run_until_parked();
        assert_eq!(
            workbench.read_with(cx, |w, cx| w.editor().map(|e| e.read(cx).text())).as_deref(),
            Some("my notes\n")
        );
        assert_eq!(on_disk("notes.txt"), "notes\n");
        assert!(workbench.read_with(cx, |w, _| w.project_tree().is_some()));

        // Save All writes the file, then the window shows the Welcome screen.
        assert!(!cx.simulate_close());
        cx.run_until_parked();
        cx.simulate_prompt_answer("Save All");
        settle(cx, |cx| on_disk("notes.txt") == "my notes\n" && welcomes(cx));

        // With autosave on nothing is asked: the document is saved and the window shows the Welcome screen.
        cx.update(|_, cx| autosave(true, cx));
        open("draft.txt", cx);
        cx.simulate_input("our ");
        assert!(!cx.simulate_close());
        settle(cx, |cx| on_disk("draft.txt") == "our draft\n" && welcomes(cx));
        assert!(!cx.has_pending_prompt());
    }

    // src/features/app/__tests__/windows.test.ts › explicit project-window requests › keeps the ordinary
    // unsaved-change guard for current-window opens
    /// Opening another project in this window (File › Open with `newWindow: false`) first settles
    /// leaving the one it shows: an unsaved document is asked about; Cancel keeps the project and the
    /// text, Don't Save opens the other project and leaves the file as it was on disk.
    #[gpui_kit::test]
    async fn keeps_the_ordinary_unsaved_change_guard_for_current_window_opens(cx: &mut TestAppContext) {
        use ide_workspace::settings::SettingsScope;
        let _home = start(cx);
        let folders: Vec<tempfile::TempDir> =
            (0..2).map(|_| tempfile::tempdir().expect("a temporary folder")).collect();
        let roots: Vec<PathBuf> = folders.iter().map(|f| f.path().canonicalize().expect("the folder exists")).collect();
        for root in &roots {
            std::fs::write(root.join("notes.txt"), "notes\n").expect("the file is written");
        }
        cx.update(|cx| {
            crate::platform::settings::set("w1", SettingsScope::Global, "files.autoSave", &false.into(), cx)
                .expect("the setting is stored");
        });
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        workbench.update_in(cx, |workbench, window, cx| workbench.open_project(roots[0].clone(), window, cx));
        settle(cx, |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
        let file = roots[0].join("notes.txt");
        workbench.update_in(cx, |workbench, window, cx| workbench.open_project_file(file.clone(), window, cx));
        settle(cx, |cx| workbench.read_with(cx, |w, _| w.editor().is_some()));
        cx.simulate_input("my ");
        let project = |cx: &mut VisualTestContext| workbench.read_with(cx, |w, cx| w.project_root(cx));
        // An explicit request for this window: "Open project in" is not asked.
        let other = serde_json::json!({ "path": roots[1].to_string_lossy(), "newWindow": false });

        // Cancel: the project and its unsaved text stay.
        run_action(cx, "file.openProjectPath", Some(other.clone()));
        let (title, _) = cx.pending_prompt().expect("leaving the project asks");
        assert_eq!(title, "Unsaved Changes");
        cx.simulate_prompt_answer("Cancel");
        cx.run_until_parked();
        assert_eq!(project(cx).as_deref(), Some(&*roots[0]));
        assert_eq!(
            workbench.read_with(cx, |w, cx| w.editor().map(|e| e.read(cx).text())).as_deref(),
            Some("my notes\n")
        );

        // Don't Save: the other project opens here, and the file is as it was.
        run_action(cx, "file.openProjectPath", Some(other));
        cx.simulate_prompt_answer("Don't Save");
        settle(cx, |cx| project(cx).as_deref() == Some(&*roots[1]));
        assert_eq!(std::fs::read_to_string(&file).expect("readable"), "notes\n");
    }

    // Not from a spec (src/workbench/projectWindow.ts › chooseProjectWindow): "Don't ask again" on the
    // open-project question stores the answer, and the next open follows it without asking.
    #[gpui_kit::test]
    async fn dont_ask_again_on_the_open_project_question_stores_the_answer(cx: &mut TestAppContext) {
        use crate::platform::actions::execute_action_or_throw;
        use windows::Target;
        let _home = start(cx);
        cx.update(gpui_kit::init);
        let folders: Vec<tempfile::TempDir> =
            (0..3).map(|_| tempfile::tempdir().expect("a temporary folder")).collect();
        let roots: Vec<PathBuf> = folders.iter().map(|f| f.path().canonicalize().expect("the folder exists")).collect();
        let run = |cx: &mut TestAppContext, window: gpui_kit::AnyWindowHandle, path: &std::path::Path| {
            let arg = serde_json::Value::String(path.to_string_lossy().into_owned());
            window
                .update(cx, |_, window, cx| execute_action_or_throw("file.openProjectPath", Some(&arg), window, cx))
                .expect("the window is open")
                .expect("the action runs");
            cx.run_until_parked();
        };
        let projects = |cx: &mut TestAppContext| -> Vec<Option<PathBuf>> {
            cx.update(|cx| windows::windows(cx))
                .into_iter()
                .map(|(_, handle)| {
                    handle
                        .update(cx, |_, window, cx| for_window(window, cx).and_then(|w| w.read(cx).project_root(cx)))
                        .expect("the window is open")
                })
                .collect()
        };
        let wait = |cx: &mut TestAppContext, done: &dyn Fn(&mut TestAppContext) -> bool| {
            for _ in 0..300 {
                cx.run_until_parked();
                if done(cx) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            panic!("the backend did not deliver within three seconds");
        };
        let main = cx.update(|cx| windows::open_window(Target::Empty, cx)).expect("a window opens");
        run(cx, main, &roots[0]);
        wait(cx, &|cx| projects(cx) == [Some(roots[0].clone())]);

        // Asked, with Don't ask again: New Window opens it, and the answer is stored.
        run(cx, main, &roots[1]);
        assert!(shows_app_dialog(cx, main));
        answer_app_dialog(cx, main, "new", true);
        wait(cx, &|cx| projects(cx) == [Some(roots[0].clone()), Some(roots[1].clone())]);
        assert_eq!(cx.update(|cx| crate::platform::settings::global(cx).general.open_project_in), "new");

        // The next open follows it without asking.
        run(cx, main, &roots[2]);
        assert!(!shows_app_dialog(cx, main));
        wait(cx, &|cx| projects(cx).len() == 3 && projects(cx)[2].as_deref() == Some(&*roots[2]));
        assert_eq!(projects(cx)[0].as_deref(), Some(&*roots[0]));
    }

    // src/features/app/__tests__/windows.test.ts › loose files (LightEdit) › turns a window without a
    // project into LightEdit when no LightEdit window exists
    /// Three loose files opened into a window that shows nothing all open: the first in that window,
    /// edited on its own, and each of the others in a window of its own, since a window edits one
    /// loose file.
    #[gpui_kit::test]
    async fn turns_a_window_without_a_project_into_light_edit_when_no_light_edit_window_exists(
        cx: &mut TestAppContext,
    ) {
        use crate::platform::actions::execute_action_or_throw;
        use windows::Target;
        let _home = start(cx);
        cx.update(gpui_kit::init);
        let folder = tempfile::tempdir().expect("a temporary folder");
        let root = folder.path().canonicalize().expect("the folder exists");
        let files: Vec<PathBuf> = ["a.txt", "b.md", "c.txt"].iter().map(|name| root.join(name)).collect();
        for file in &files {
            std::fs::write(file, "text\n").expect("the file is written");
        }
        let main = cx.update(|cx| windows::open_window(Target::Empty, cx)).expect("a window opens");
        main.update(cx, |_, window, cx| execute_action_or_throw("file.openFile", None, window, cx))
            .expect("the window is open")
            .expect("the action runs");
        cx.run_until_parked();
        assert!(cx.did_prompt_for_paths());
        let picked = files.clone();
        cx.simulate_path_prompt_response(move |options| {
            assert!(options.files && options.multiple);
            Some(picked)
        });
        let shown = |cx: &mut TestAppContext| -> Vec<(String, Option<PathBuf>)> {
            cx.update(|cx| windows::windows(cx))
                .into_iter()
                .map(|(label, handle)| {
                    let file = handle
                        .update(cx, |_, window, cx| {
                            let editor = for_window(window, cx).and_then(|w| w.read(cx).editor().cloned());
                            editor.map(|editor| editor.read(cx).path().to_path_buf())
                        })
                        .ok()
                        .flatten();
                    (label, file)
                })
                .collect()
        };
        for attempt in 0..=300 {
            cx.run_until_parked();
            let now = shown(cx);
            if now.len() == 3 && now.iter().all(|(_, file)| file.is_some()) {
                break;
            }
            assert!(attempt < 300, "the files did not open within three seconds: {now:?}");
            std::thread::sleep(Duration::from_millis(10));
        }
        let shown = shown(cx);
        assert_eq!(shown[0], ("main".to_owned(), Some(files[0].clone())));
        let mut others: Vec<PathBuf> = shown[1..].iter().filter_map(|(_, file)| file.clone()).collect();
        others.sort();
        assert_eq!(others, files[1..]);
        for (label, _) in &shown {
            cx.update(|cx| cx.global::<Backend>().host.close_project(label));
        }
    }

    // src/features/app/__tests__/closeGuard.test.ts › decideClose › 'don't ask again' turns the setting off
    /// Closing the last window, showing nothing, confirms the exit when general.confirmExit asks
    /// (it does by default): Cancel keeps the window, and a second close while the question is up is
    /// refused; Exit with Don't ask again turns the setting off and closes the window, and the next
    /// window closes without a question.
    #[gpui_kit::test]
    async fn dont_ask_again_turns_the_setting_off(cx: &mut TestAppContext) {
        let _home = start(cx);
        let (workbench, first) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        assert!(!first.simulate_close());
        first.run_until_parked();
        assert_eq!(app_dialog_title(&workbench, first).as_deref(), Some("Confirm Exit"));
        // One close is decided at a time.
        assert!(!first.simulate_close());
        click_on(first, "prompt-button-cancel");
        assert_eq!(app_dialog_title(&workbench, first), None);
        assert!(!first.windows().is_empty());

        assert!(!first.simulate_close());
        first.run_until_parked();
        click_on(first, "prompt-checkbox");
        click_on(first, "prompt-button-exit");
        settle(first, |cx| cx.windows().is_empty());
        assert!(!cx.update(|cx| crate::platform::settings::global(cx).general.confirm_exit));

        let (_workbench, second) = cx.add_window_view(|window, cx| Workbench::new("w2", window, cx));
        assert!(second.simulate_close(), "nothing is asked: the window closes");
    }
```

### 5c. Change `projects_open_in_this_window_a_new_one_or_the_one_that_has_them`

Edit 1. Anchor:

```rust
        // One with a project asks; New Window opens the next label.
        run(cx, main, "file.openProjectPath", Some(&roots[1]));
        assert!(cx.has_pending_prompt());
        cx.simulate_prompt_answer("New Window");
```

Replace with:

```rust
        // One with a project asks; New Window opens the next label.
        run(cx, main, "file.openProjectPath", Some(&roots[1]));
        assert!(shows_app_dialog(cx, main));
        answer_app_dialog(cx, main, "new", false);
```

Edit 2. Anchor:

```rust
        // A project that a window has is not opened again: that window comes to the front.
        let second = window(cx, "project-1");
        run(cx, second, "file.openProjectPath", Some(&roots[0]));
        assert!(!cx.has_pending_prompt());
        assert_eq!(labels(cx).len(), 2);
```

Replace with:

```rust
        // A project that a window has is not opened again: asked where, that window comes to the front.
        let second = window(cx, "project-1");
        run(cx, second, "file.openProjectPath", Some(&roots[0]));
        assert!(shows_app_dialog(cx, second));
        answer_app_dialog(cx, second, "new", false);
        assert_eq!(labels(cx).len(), 2);
```

(windows.test.ts:168 "does nothing locally when another window was focused": the backend focuses the window that has the project, whatever the answer.)

Edit 3. Anchor:

```rust
        run(cx, second, "file.openProjectPath", Some(&roots[2]));
        cx.simulate_prompt_answer("Cancel");
        cx.run_until_parked();
        assert_eq!(project(cx, "project-1").as_deref(), Some(&*roots[1]));
        run(cx, second, "file.openProjectPath", Some(&roots[2]));
        cx.simulate_prompt_answer("This Window");
```

Replace with:

```rust
        run(cx, second, "file.openProjectPath", Some(&roots[2]));
        answer_app_dialog(cx, second, "cancel", false);
        assert_eq!(project(cx, "project-1").as_deref(), Some(&*roots[1]));
        run(cx, second, "file.openProjectPath", Some(&roots[2]));
        answer_app_dialog(cx, second, "same", false);
```

Edit 4. Anchor (the stored-answer part):

```rust
        run(cx, second, "file.openProjectPath", Some(&roots[1]));
        assert!(!cx.has_pending_prompt());
        wait(cx, &|cx| project(cx, "project-1").as_deref() == Some(&roots[1]));
```

Replace with:

```rust
        run(cx, second, "file.openProjectPath", Some(&roots[1]));
        assert!(!shows_app_dialog(cx, second));
        wait(cx, &|cx| project(cx, "project-1").as_deref() == Some(&roots[1]));
```

The rest of the test passes as written. `file.openProject` with "same" stored does not ask, and leaving needs no question. `window.new` / `window.next` / `window.previous` are unchanged. `file.closeProject` now goes through the leave guard, which settles at once with nothing to ask, so `project(cx, "project-1") == None` holds right after `run`. `file.openRecentProject` on the empty window asks nothing.

---

## 6. What is not wired, and why

- **Attach** is not offered: `carry_out_open` drops the `attach` button from the question (`const ATTACH`). The backend has the operation (`ide_host::ops::project::attach_workspace`, `crates/ide-host/src/ops/project.rs:114`). Two things are missing natively:
  - The Project view (`project_tree.rs`) is rooted at the project folder (`reveal_path` returns false outside it), so an attached folder would not be seen.
  - There is no trust question (the webview's `confirmProjectTrust`).

  The steps (`ConfirmTrust` answered Ok, `ReadTrust` from `host.trust.trusted`, `Attach`, `ReadCurrentProject`, `RevealProject`) are carried out all the same. Removing the filter turns Attach on, once the tree shows workspace members and a trust dialog exists.

- **LightEdit tabs**: a native LightEdit window holds one editor (`Content::Editor`). Of the loose files left to an empty window, the first opens there and every other one gets its own window via `windows::open_window(Target::File)`. With "one LightEdit window collects them all" there is no collecting window, so routing sends a file to another window only when that window's project contains it or that window already edits it.
- **Backend window routing** (`window_open_project` / `window_open_files`) does not exist natively. `request_window` and `route_files` answer it from the open workbenches. **WSL and remote**: `ResolveWsl` always answers `None`, and `ConnectRemote` is unreachable (answered as a failure).
- **Running processes**: only the run driver's live sessions (`RunStore.sessions` with `is_live()`) are named and terminated. This matches the webview's single provider (`src/features/run/index.ts:401`, run plus debug sessions):
  - The native debugger is not compiled (`workbench/debug.rs` has no `mod` line).
  - The build and the terminal's shells are left out for parity: the webview does not ask about either. Both could be wired:
    - A running build can be stopped and seen to end: `workbench/build_window.rs` `BuildSlot.running: Option<Task<()>>` is cleared when the build ends, and `stop_build` cancels its request. To include it, add a name for it to `running_process_names`, call `stop_build` in `terminate`, and wait on `observe_self` until `build.running` is `None`.
    - The terminal's sessions are listed by `terminal/store.rs` `sessions()`. I did not read how a session's liveness and its stop are exposed.

  `platform_more::running_processes::RunningProcessesProviders` is not used, because nothing registers with it. There is no "Terminating…" panel with a Kill button. Instead, after `TERMINATE_GRACE` (10 s) a second Stop kills what is still shutting down. After another 10 s the close gives up and reports "Cannot stop process" with the names.

- **Prepared custom documents** (notebooks): there are none natively. `PrepareClose` is answered `prepared(None)` and `DiscardPrepared` `Ok(())`.
- **`documents_closed`** (auxiliary document windows): there are none natively. It is always `true`, so `CloseRoute::Refused` is unreachable but handled.
- **`LaunchQueue`**, the CLI launcher (`cli_command.rs`), `updates.rs`, `workspace_dialog.rs`, `project_actions.rs`, `shell.rs`, `invalidate_caches.rs`, `RemoveFromWorkspace`, `tabs_to_close`, `light_edit_should_close` (LightEdit's auto-close on its last tab), `split_dropped` and `light_edit_file_as_project` are not wired. There is no native launch event, update installer, workspace dialog, window-group tabs or file drop to drive them, and the brief scoped this change to opening and closing. `file.openInNewWindow` keeps `windows::open_project(path, true)`, which is equivalent to `OpenFlow::explicit(path, true)` without the leave guard (nothing is left).

---

## 7. Assumptions I could not confirm by reading, and where compilation is most likely to fail

1. **Autosave on saves without asking.** This follows the brief, not the spec: `closeGuard.ts` `decideClose` asks whenever `dirty > 0`, whatever `files.autoSave` says. In `app_shell::decide_close`, with autosave on and documents unsaved, `save_all` runs first and the model gets the remaining count. A failed save is reported and then asked about. To follow the spec instead, pass `autosave = false` in `request_close` / `request_leave`.
2. **`PromptDialog::message` (section 1) is new.** `app_shell.rs` does not compile without it.
3. **Private items across modules.** `app_shell.rs` reads `Workbench`'s private fields (`label`, `content`, `close_requests`, `app_dialog`, `editor_observer`, `light_find_bar`), the private `Content` enum, `Workbenches` and its `.0`, and calls private methods (`auto_save`, `save_all`, `report_save_failure`, `leave`, `sync_welcome`). This is legal for a child module of `workbench`. It fails only if one of them was made private to a narrower module meanwhile.
4. **`RunStore` notifies when a session ends (read).** `run/driver.rs` `run_exited` updates the store without a notify. The notify comes from `finish`, after a `FINISH_DELAY_MS` timer (`finish_later`). `stopped()` therefore sees the end once that timer has run, which in tests needs `advance_clock`; the `long` loop does that.
5. **Opening is awaited by observing the workbench.** `project_opened()` uses `cx.observe_self`. It relies on `Workbench::open_project`'s completion calling `cx.notify()` (it does, at the end of its `update_in`), and on `Content::Opening` being replaced by `Project` or `Failed`.
6. **gpui signatures.** Each was read in gpui-pre-0.3.12, but its combination here is new:
   - `Window::spawn(&self, cx: &App, AsyncFnOnce(&mut AsyncWindowContext) -> R) -> Task<R>`.
   - `AsyncWindowContext::update(|&mut Window, &mut App|) -> Result<R>`.
   - `AsyncApp::update(|&mut App|) -> R` (no `Result`).
   - `AnyWindowHandle::update(cx: &mut AsyncApp, …) -> Result<R>` from inside `cx.spawn(async move |cx: &mut AsyncApp| …)` in `quit`.
   - `cx.background_executor().timer(..)` on `&mut AsyncWindowContext`, through `Deref` to `AsyncApp`.
   - `futures::future::select` over `std::pin::pin!` of a `oneshot::Receiver` and a gpui `Task<()>`.
7. **Likely compile spots in `app_shell.rs`:**
   - `decide_close`: `this` and `decision` are moved into `window.spawn` closures inside a `loop`, in arms that `return`. NLL accepts this; if not, build `Started` before the loop.
   - `project_opened`: `sender` is moved into the `observe_self` closure in one match arm and `take()`n in the other.
   - `request_close`: `state.clone()` is moved into the `update` closure while `state` is used again in the `ToWelcome` arm.
   - Let-chains (`if let … && let …`) in `stopped`, `project_opened` and `quit`.
   - `cx.update(|cx| windows::windows(cx))` passing `&mut App` to a `fn(&App)`.
8. **Tests.**
   - `click_on` needs the dialog layer drawn. In `add_window_view` windows it is, as in the other click tests.
   - For windows opened with `windows::open_window`, the tests answer by emitting `PromptDialogEvent::Closed` on the dialog (`answer_app_dialog`) rather than clicking, so they do not depend on those windows being drawn.
   - `asks_before_terminating_a_running_process` depends on `/bin/sleep` ending on the first Stop (SIGTERM) and on `long` advancing the clock. A slow machine could reach the 10 s grace and kill it, which still passes.
   - `turns_a_window_without_a_project_into_light_edit_…` assumes `Editor::path()` returns the canonical path that was passed. Check `editor/view.rs:365` and `Editor::with_text`.
   - `dont_ask_again_turns_the_setting_off` assumes `Settings::default().general.confirm_exit == true` (`crates/ide-core/src/settings.rs:337`) and that `remove_window` empties `cx.windows()` after `run_until_parked`, as the old test relied on.
9. **Quit.** `menu.rs` `app.quit` now decides the windows' closes one after another; `request_close` in a quit does not remove a window. Every window stays open until all have agreed and `cx.quit()` runs. A window that refuses calls off the quit (`Quitting(false)`), and the windows that agreed before it stay open. No test covers quit: `cx.quit()` is not exercised in the test platform.
10. **LightEdit back to Welcome (read).** `close_to_welcome` calls `host.close_project_detached(label)` for a LightEdit window, as `close_project` does for a project. `Host::begin_close` (`crates/ide-host/src/lib.rs:251`) removes any entry of `projects`, LightEdit sessions included (it branches on `project.light`).
