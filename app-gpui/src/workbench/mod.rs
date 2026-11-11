//! The window shell. For now: a frame around one editor (LightEdit), which is the part of the
//! workbench the editor slice needs. Tool windows, the title bar and the status bar follow.

use crate::backend::Backend;
use crate::editor::view::Editor;
use crate::platform::input;
use crate::theme;
use gpui_kit::{
    App, Context, Entity, Focusable, Global, IntoElement, ModifiersChangedEvent, MouseDownEvent, Render, SharedString,
    WeakEntity, Window, WindowId, div, prelude::*,
};
use std::collections::HashMap;
use std::path::PathBuf;

/// What a window shows.
enum Content {
    /// Nothing is open.
    Empty,
    Opening(PathBuf),
    Editor(Entity<Editor>),
    Failed(SharedString),
}

/// One window: its backend label and what it shows.
pub struct Workbench {
    /// The label the backend knows this window by.
    label: String,
    content: Content,
}

/// The workbench of each window, so that an action can find what it acts on.
#[derive(Default)]
struct Workbenches(HashMap<WindowId, WeakEntity<Workbench>>);

impl Global for Workbenches {}

/// The workbench shown in `window`.
pub fn for_window(window: &Window, cx: &App) -> Option<Entity<Workbench>> {
    cx.try_global::<Workbenches>()?.0.get(&window.window_handle().window_id())?.upgrade()
}

/// The editor an action invoked in `window` acts on.
pub fn active_editor(window: &Window, cx: &App) -> Option<Entity<Editor>> {
    for_window(window, cx)?.read(cx).editor().cloned()
}

impl Workbench {
    /// The workbench of `window`, known to the backend as `label`.
    pub fn new(label: &str, window: &Window, cx: &mut Context<Self>) -> Self {
        let id = window.window_handle().window_id();
        let this = cx.weak_entity();
        cx.default_global::<Workbenches>().0.insert(id, this);
        cx.on_release(move |_, cx| {
            cx.default_global::<Workbenches>().0.remove(&id);
            input::window_blurred(id, cx);
            crate::platform::context::drop_window_context(id, cx);
        })
        .detach();
        Self { label: label.to_owned(), content: Content::Empty }
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn editor(&self) -> Option<&Entity<Editor>> {
        match &self.content {
            Content::Editor(editor) => Some(editor),
            _ => None,
        }
    }

    /// The message shown instead of an editor, when opening failed.
    pub fn failure(&self) -> Option<&str> {
        match &self.content {
            Content::Failed(message) => Some(message),
            _ => None,
        }
    }

    /// Opens a loose file for editing (LightEdit: no project, no tool windows).
    pub fn open_file(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.content = Content::Opening(path.clone());
        cx.notify();
        let label = self.label.clone();
        let opened = cx.global::<Backend>().with_host(move |host| async move {
            let (blocking_host, blocking_label, file) = (host.clone(), label.clone(), path.clone());
            tokio::task::spawn_blocking(move || blocking_host.open_light(&blocking_label, &file).map(|_| ()))
                .await
                .map_err(|e| ide_core::IdeError::internal(e.to_string()))??;
            let file = path.canonicalize().unwrap_or(path);
            ide_host::ops::doc::open(&host, &label, file).await
        });
        cx.spawn_in(window, async move |workbench, cx| {
            let result = opened.await;
            let _ = workbench.update_in(cx, |workbench, window, cx| {
                let host = cx.global::<Backend>().host.clone();
                let label = workbench.label.clone();
                // The document's text is read before the view exists, so a failure is a message
                // in the window instead of an editor without a document.
                let text = result
                    .map_err(|error| error.to_string())
                    .and_then(|opened| opened.map_err(|error| error.message))
                    .and_then(|info| {
                        let project = host.project(&label).map_err(|error| error.message)?;
                        let (version, rope) =
                            project.documents.rope_snapshot(info.id).map_err(|error| error.message)?;
                        Ok((info, version, rope))
                    });
                workbench.content = match text {
                    Ok((info, version, rope)) => {
                        let editor = cx.new(|cx| Editor::with_text(host, &label, &info, version, rope, cx));
                        let focus = editor.focus_handle(cx);
                        window.focus(&focus, cx);
                        Content::Editor(editor)
                    }
                    Err(message) => Content::Failed(message.into()),
                };
                cx.notify();
            });
        })
        .detach();
    }
}

impl Render for Workbench {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let message = |text: SharedString| {
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(theme::color(cx, "fg-muted"))
                .child(text)
        };
        let body = match &self.content {
            Content::Empty => message("".into()).into_any_element(),
            Content::Opening(path) => message(format!("Opening {}…", path.display()).into()).into_any_element(),
            Content::Failed(error) => message(error.clone()).into_any_element(),
            Content::Editor(editor) => editor.clone().into_any_element(),
        };
        div()
            .size_full()
            .bg(theme::color(cx, "bg-app"))
            .text_color(theme::color(cx, "fg"))
            .text_size(theme::length(cx, "fs-ui"))
            // Modifier taps and mouse buttons have no application-level hook: forward them.
            .on_modifiers_changed(cx.listener(|_, event: &ModifiersChangedEvent, window, cx| {
                input::modifiers_changed(&event.modifiers, window, cx);
            }))
            .capture_any_mouse_down(cx.listener(|_, event: &MouseDownEvent, window, cx| {
                if input::mouse_down(event.button, &event.modifiers, false, window, cx).consumed() {
                    cx.stop_propagation();
                }
            }))
            .child(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::AppDirs;
    use gpui_kit::{TestAppContext, VisualTestContext};
    use std::time::Duration;

    fn start(cx: &mut TestAppContext) -> tempfile::TempDir {
        cx.executor().allow_parking();
        let home = tempfile::tempdir().expect("a temporary home");
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
            crate::editor::actions::register(cx);
            Backend::init(AppDirs::under(home.path()), cx).expect("the backend starts");
        });
        home
    }

    fn settle(cx: &mut VisualTestContext, done: impl Fn(&mut VisualTestContext) -> bool) {
        for _ in 0..300 {
            cx.run_until_parked();
            if done(cx) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("the backend did not deliver within three seconds");
    }

    #[gpui_kit::test]
    async fn opens_a_loose_file_in_an_editor_and_types_into_it(cx: &mut TestAppContext) {
        let _home = start(cx);
        let folder = tempfile::tempdir().expect("a temporary folder");
        let file = folder.path().join("todo.txt");
        std::fs::write(&file, "milk\n").expect("the file is written");
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        workbench.update_in(cx, |workbench, window, cx| workbench.open_file(file.clone(), window, cx));
        settle(cx, |cx| workbench.read_with(cx, |w, _| w.editor().is_some() || w.failure().is_some()));
        let editor = workbench
            .read_with(cx, |w, _| w.editor().cloned())
            .unwrap_or_else(|| panic!("{:?}", workbench.read_with(cx, |w, _| w.failure().map(str::to_owned))));
        assert_eq!(editor.read_with(cx, |e, _| e.text()), "milk\n");
        // The editor has focus: typing goes to the document without a click.
        cx.simulate_input("oat ");
        assert_eq!(editor.read_with(cx, |e, _| e.text()), "oat milk\n");
        let host = cx.update(|_, cx| cx.global::<Backend>().host.clone());
        host.close_project("w1");
    }

    /// ⌘S is `file.save` in the default keymap: the keystroke goes through the dispatcher, the
    /// registry and the editor to the backend, which writes the file.
    #[gpui_kit::test]
    async fn the_keymap_runs_editor_actions_on_the_windows_editor(cx: &mut TestAppContext) {
        use crate::platform::actions::{ActionSource, execute_action_from};
        let _home = start(cx);
        let folder = tempfile::tempdir().expect("a temporary folder");
        let file = folder.path().join("todo.txt");
        std::fs::write(&file, "milk\n").expect("the file is written");
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        // Without an editor the action has nothing to act on, and says so instead of failing silently.
        assert!(
            cx.update(|window, cx| crate::platform::actions::execute_action_or_throw("file.save", None, window, cx))
                .is_err()
        );
        workbench.update_in(cx, |workbench, window, cx| workbench.open_file(file.clone(), window, cx));
        settle(cx, |cx| workbench.read_with(cx, |w, _| w.editor().is_some()));
        let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");

        cx.simulate_input("oat ");
        cx.simulate_keystrokes("cmd-s");
        settle(cx, |_| std::fs::read_to_string(&file).is_ok_and(|text| text == "oat milk\n"));

        // The Edit menu's actions act on the same editor.
        assert!(cx.update(|window, cx| execute_action_from(ActionSource::Menu, "edit.undo", None, window, cx)));
        assert_eq!(editor.read_with(cx, |e, _| e.text()), "milk\n");
        assert!(cx.update(|window, cx| execute_action_from(ActionSource::Menu, "edit.redo", None, window, cx)));
        assert!(cx.update(|window, cx| execute_action_from(ActionSource::Menu, "edit.selectAll", None, window, cx)));
        assert!(cx.update(|window, cx| execute_action_from(ActionSource::Menu, "edit.cut", None, window, cx)));
        assert_eq!(editor.read_with(cx, |e, _| e.text()), "");
        assert!(cx.update(|window, cx| execute_action_from(ActionSource::Menu, "edit.paste", None, window, cx)));
        assert!(cx.update(|window, cx| execute_action_from(ActionSource::Menu, "edit.copy", None, window, cx)));
        assert_eq!(editor.read_with(cx, |e, _| e.text()), "oat milk\n");
        let host = cx.update(|_, cx| cx.global::<Backend>().host.clone());
        host.close_project("w1");
    }

    #[gpui_kit::test]
    async fn a_file_that_cannot_open_is_a_message_in_the_window(cx: &mut TestAppContext) {
        let _home = start(cx);
        let folder = tempfile::tempdir().expect("a temporary folder");
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        // A folder is not a file to edit.
        workbench.update_in(cx, |workbench, window, cx| workbench.open_file(folder.path().to_path_buf(), window, cx));
        settle(cx, |cx| workbench.read_with(cx, |w, _| w.editor().is_some() || w.failure().is_some()));
        workbench.read_with(cx, |w, _| {
            assert!(w.editor().is_none());
            assert!(w.failure().is_some_and(|message| message.contains("is a folder")), "{:?}", w.failure());
        });
    }
}
