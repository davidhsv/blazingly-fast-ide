//! The app's windows: opening one, finding the one that has a project, going from one to the
//! next, and where a project opens ("Open project in": this window, a new one, or ask).
//!
//! Windows are known by label: `main`, then `project-<n>` with the lowest free number.

use super::{Workbench, Workbenches, for_window, window_options};
use crate::platform::actions::ActionRegistry;
use crate::platform::i18n::Localization;
use gpui_kit::{AnyWindowHandle, App, AppContext as _, PathPromptOptions, Window};
use std::path::{Path, PathBuf};

pub const MAIN: &str = "main";
const PREFIX: &str = "project-";

/// Sort key: `main` first, then `project-<n>` by n, then anything else.
fn order_key(label: &str) -> (u8, u64, String) {
    if label == MAIN {
        (0, 0, String::new())
    } else if let Some(n) = label.strip_prefix(PREFIX).and_then(|n| n.parse().ok()) {
        (1, n, String::new())
    } else {
        (2, 0, label.to_owned())
    }
}

pub fn sorted_labels<'a>(labels: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut sorted: Vec<String> = labels.into_iter().map(str::to_owned).collect();
    sorted.sort_by_key(|label| order_key(label));
    sorted
}

/// Lowest free `project-<n>` label.
pub fn next_label<'a>(existing: impl IntoIterator<Item = &'a str>) -> String {
    let used: std::collections::HashSet<&str> = existing.into_iter().collect();
    (1u64..).map(|n| format!("{PREFIX}{n}")).find(|label| !used.contains(label.as_str())).expect("unbounded")
}

/// The window after (or before) `current` in label order, wrapping around.
pub fn cycle_target(sorted: &[String], current: &str, forward: bool) -> Option<String> {
    if sorted.len() < 2 {
        return None;
    }
    let at = sorted.iter().position(|label| label == current).unwrap_or(0);
    let count = sorted.len();
    Some(sorted[if forward { (at + 1) % count } else { (at + count - 1) % count }].clone())
}

/// What a new window shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Empty,
    Project(PathBuf),
    /// A loose file, edited without a project.
    File(PathBuf),
}

/// The open windows with their labels, in label order.
pub fn windows(cx: &App) -> Vec<(String, AnyWindowHandle)> {
    let Some(known) = cx.try_global::<Workbenches>() else { return Vec::new() };
    let mut found: Vec<(String, AnyWindowHandle)> = cx
        .windows()
        .into_iter()
        .filter_map(|handle| {
            let workbench = known.0.get(&handle.window_id())?.upgrade()?;
            Some((workbench.read(cx).label().to_owned(), handle))
        })
        .collect();
    found.sort_by_key(|(label, _)| order_key(label));
    found
}

/// Opens a window showing `target` and brings it to the front.
pub fn open_window(target: Target, cx: &mut App) -> anyhow::Result<AnyWindowHandle> {
    let open = windows(cx);
    let label = if open.iter().any(|(label, _)| label == MAIN) {
        next_label(open.iter().map(|(label, _)| label.as_str()))
    } else {
        MAIN.to_owned()
    };
    let (handle, _) = gpui_kit::open_window(window_options(), cx, move |window, cx| {
        cx.new(|cx| {
            let mut workbench = Workbench::new(&label, window, cx);
            match target {
                Target::Empty => {}
                Target::Project(root) => workbench.open_project_asking(root, window, cx),
                Target::File(file) => workbench.open_file(file, window, cx),
            }
            workbench
        })
    })?;
    let _ = handle.update(cx, |_, window, _| window.activate_window());
    Ok(handle)
}

/// The window that has the project at `root` open.
fn project_window(root: &Path, cx: &App) -> Option<AnyWindowHandle> {
    let known = cx.try_global::<Workbenches>()?;
    cx.windows().into_iter().find(|handle| {
        known
            .0
            .get(&handle.window_id())
            .and_then(|workbench| workbench.upgrade())
            .is_some_and(|workbench| workbench.read(cx).project_root(cx).as_deref() == Some(root))
    })
}

/// A folder as the backend knows it once open: with links resolved.
fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// Opens the project at `path`: the window that already has it comes to the front; otherwise it
/// opens in `window` when that shows nothing and `new_window` is not asked for, else in a new
/// window.
pub fn open_project(path: &Path, new_window: bool, window: &mut Window, cx: &mut App) -> anyhow::Result<()> {
    if let Some(open) = project_window(&canonical(path), cx) {
        if open.window_id() == window.window_handle().window_id() {
            window.activate_window();
        } else {
            let _ = open.update(cx, |_, window, _| window.activate_window());
        }
        return Ok(());
    }
    let here = for_window(window, cx).filter(|workbench| !new_window && workbench.read(cx).is_empty());
    match here {
        Some(workbench) => {
            workbench.update(cx, |workbench, cx| workbench.open_project_asking(path.to_path_buf(), window, cx))
        }
        None => {
            open_window(Target::Project(path.to_path_buf()), cx)?;
        }
    }
    Ok(())
}

/// Opens the project at `path` where "Open project in" says: a window that shows nothing takes
/// it; one with a project asks (with "Don't ask again"), or follows the stored answer. Opening it
/// in this window first settles leaving the project it shows.
pub fn open_project_with_policy(path: &Path, window: &mut Window, cx: &mut App) -> anyhow::Result<()> {
    super::app_shell::open_project_with_policy(path, Vec::new(), window, cx)
}

/// Opens loose `files` in `window`: as tabs of its project; otherwise each file of a project open
/// in another window goes there, and every other file is edited on its own, the first in this
/// window when it shows nothing.
pub fn open_files(files: Vec<PathBuf>, window: &mut Window, cx: &mut App) -> anyhow::Result<()> {
    super::app_shell::open_files(files, window, cx)
}

/// Asks for a folder (or files) and hands what was picked to `then` in the asking window.
fn pick(
    directories: bool,
    title: String,
    window: &mut Window,
    cx: &mut App,
    then: impl FnOnce(Vec<PathBuf>, &mut Window, &mut App) -> anyhow::Result<()> + 'static,
) {
    let picked = cx.prompt_for_paths(PathPromptOptions {
        files: !directories,
        directories,
        multiple: !directories,
        prompt: Some(title.into()),
    });
    window
        .spawn(cx, async move |cx| {
            let Ok(Ok(Some(paths))) = picked.await else { return };
            let _ = cx.update(|window, cx| {
                if let Err(error) = then(paths, window, cx) {
                    tracing::error!("cannot open what was picked: {error:#}");
                }
            });
        })
        .detach();
}

/// The path an action was given as its argument.
fn path_argument(arg: Option<&serde_json::Value>) -> Option<PathBuf> {
    arg.and_then(serde_json::Value::as_str).map(PathBuf::from)
}

pub fn register_actions(cx: &mut App) {
    let registry = cx.default_global::<ActionRegistry>();
    registry.handle("window.new", |_, _, cx| open_window(Target::Empty, cx).map(|_| ()));
    registry.handle("file.openProject", |_, window, cx| {
        let title = cx.default_global::<Localization>().0.tx("Open…");
        pick(true, title, window, cx, |paths, window, cx| match paths.first() {
            Some(path) => open_project_with_policy(path, window, cx),
            None => Ok(()),
        });
        Ok(())
    });
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
    registry.handle("file.openRecentProject", |call, window, cx| match path_argument(call.arg) {
        Some(path) => open_project_with_policy(&path, window, cx),
        None => Ok(()),
    });
    registry.handle("file.openInNewWindow", |call, window, cx| {
        if let Some(path) = path_argument(call.arg) {
            return open_project(&path, true, window, cx);
        }
        let title = cx.default_global::<Localization>().0.t("app.window.pickTitle", &[]);
        pick(true, title, window, cx, |paths, window, cx| match paths.first() {
            Some(path) => open_project(path, true, window, cx),
            None => Ok(()),
        });
        Ok(())
    });
    registry.handle("file.openFile", |_, window, cx| {
        let title = cx.default_global::<Localization>().0.t("app.window.pickFileTitle", &[]);
        pick(false, title, window, cx, open_files);
        Ok(())
    });
    registry.handle("file.closeProject", |_, window, cx| {
        super::app_shell::request_close_project(window, cx);
        Ok(())
    });
    for (id, forward) in [("window.next", true), ("window.previous", false)] {
        registry.handle(id, move |_, window, cx| {
            let open = windows(cx);
            let labels: Vec<String> = open.iter().map(|(label, _)| label.clone()).collect();
            let current = for_window(window, cx).map(|workbench| workbench.read(cx).label().to_owned());
            let target = cycle_target(&labels, current.as_deref().unwrap_or(MAIN), forward);
            if let Some((_, handle)) = target.and_then(|label| open.into_iter().find(|(known, _)| *known == label)) {
                // The asking window is the one being updated: the other is free.
                let _ = handle.update(cx, |_, window, _| window.activate_window());
            }
            Ok(())
        });
    }
    registry.handle("window.toggleFullScreen", |_, window, _| {
        window.toggle_fullscreen();
        Ok(())
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_sort_main_first_then_by_number() {
        assert_eq!(sorted_labels(["project-10", "project-2", "main", "x"]), ["main", "project-2", "project-10", "x"]);
    }

    #[test]
    fn next_label_fills_gaps() {
        assert_eq!(next_label(["main"]), "project-1");
        assert_eq!(next_label(["main", "project-1", "project-3"]), "project-2");
    }

    #[test]
    fn cycling_wraps_around_and_needs_two_windows() {
        let labels = ["main", "project-1", "project-2"].map(str::to_owned);
        assert_eq!(cycle_target(&labels, "main", true).as_deref(), Some("project-1"));
        assert_eq!(cycle_target(&labels, "project-2", true).as_deref(), Some("main"));
        assert_eq!(cycle_target(&labels, "main", false).as_deref(), Some("project-2"));
        assert_eq!(cycle_target(&labels[..1], "main", true), None);
    }
}
