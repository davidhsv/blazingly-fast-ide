//! Version control actions: which paths a selection-based action targets.

use super::ops::vcs_notice;
use crate::workbench::notifications::{NotificationLevel, NotificationRequest};
use serde_json::Value;

/// Id of Compare with Branch or Revision….
pub const COMPARE_WITH_BRANCH: &str = "vcs.compareWithBranch";

/// The path an action argument names: a string, or an object with a `path`.
pub fn arg_path(arg: Option<&Value>) -> Option<&str> {
    let arg = arg?;
    arg.as_str().or_else(|| arg.as_object()?.get("path")?.as_str())
}

/// The paths an action argument names: an array of them, or the one of [`arg_path`].
pub fn arg_paths(arg: Option<&Value>) -> Option<Vec<String>> {
    if let Some(Value::Array(items)) = arg {
        return Some(items.iter().filter_map(Value::as_str).map(str::to_owned).collect());
    }
    arg_path(arg).filter(|path| !path.is_empty()).map(|path| vec![path.to_owned()])
}

/// The change tree is focused: the Commit window, or the Git window's Local Changes tab.
pub fn commit_window_focused(active_tool_window: Option<&str>, git_tab: &str) -> bool {
    match active_tool_window {
        Some("commit") => true,
        Some("git") => git_tab == "changes",
        _ => false,
    }
}

/// What a selection-based action can take its paths from.
#[derive(Clone, Copy, Debug, Default)]
pub struct PathContext<'a> {
    /// The paths selected in the changes tree.
    pub commit_selection: &'a [String],
    /// The changes tree has focus ([`commit_window_focused`]).
    pub commit_window_focused: bool,
    /// The Project view's selection; it keeps it while another view has focus.
    pub project_selection: &'a [String],
    /// The file of the active editor.
    pub active_path: Option<&'a str>,
}

/// The action's own paths, or the changes tree's selection while the tree has focus.
pub fn changed_paths(arg: Option<&Value>, context: PathContext) -> Vec<String> {
    match arg_paths(arg).filter(|paths| !paths.is_empty()) {
        Some(explicit) => explicit,
        None if context.commit_window_focused => context.commit_selection.to_vec(),
        None => Vec::new(),
    }
}

/// Files the current context refers to: argument, commit view selection, or the active file.
pub fn context_paths(arg: Option<&Value>, context: PathContext) -> Vec<String> {
    let changed = changed_paths(arg, context);
    if !changed.is_empty() {
        return changed;
    }
    context.active_path.filter(|path| !path.is_empty()).map(|path| vec![path.to_owned()]).unwrap_or_default()
}

/// Paths a selection-based action targets: the argument, else the changes tree's selection, else
/// the Project view's, else the active file.
pub fn selected_paths(arg: Option<&Value>, context: PathContext) -> Vec<String> {
    let changed = changed_paths(arg, context);
    if !changed.is_empty() {
        changed
    } else if !context.project_selection.is_empty() {
        context.project_selection.to_vec()
    } else {
        context_paths(arg, context)
    }
}

/// What Compare with Branch or Revision… starts from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompareSelection {
    /// The paths to compare ([`super::compare_paths::compare_paths`] takes it from there).
    Paths(Vec<String>),
    /// Nothing is selected: report it.
    Nothing(NotificationRequest),
}

/// Compare with Branch or Revision…: the paths to compare, or what to report when nothing is
/// selected.
pub fn compare_with_branch(arg: Option<&Value>, context: PathContext) -> CompareSelection {
    let paths = selected_paths(arg, context);
    if paths.is_empty() {
        let text = "Open or select a file or folder to compare";
        return CompareSelection::Nothing(vcs_notice(NotificationLevel::Info, text));
    }
    CompareSelection::Paths(paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::actions::{is_action_enabled, manifest};
    use crate::platform::context::ContextKeys;
    use serde_json::json;

    // src/features/vcs/__tests__/compareActions.test.ts › compare action context › is enabled for Git folder selections and uses project selection in menu/Search Everywhere
    #[test]
    fn is_enabled_for_git_folder_selections_and_uses_project_selection_in_menu_and_search_everywhere() {
        let action = manifest().get(COMPARE_WITH_BRANCH).expect("the compare action is registered");
        let mut keys = ContextKeys::new();
        keys.set("vcsAvailable", true);
        keys.set("projectViewFocus", true);
        assert!(is_action_enabled(action, &keys));

        let project = ["/workspace/client".to_owned(), "/workspace/server".to_owned()];
        let context = PathContext { project_selection: &project, ..PathContext::default() };
        assert_eq!(compare_with_branch(None, context), CompareSelection::Paths(project.to_vec()));
        let arg = json!({ "path": "/workspace/client/a.ts" });
        let explicit = CompareSelection::Paths(vec!["/workspace/client/a.ts".to_owned()]);
        assert_eq!(compare_with_branch(Some(&arg), context), explicit);
    }

    // Not from a spec: where the paths come from when the action has no argument.
    #[test]
    fn falls_back_from_the_argument_to_the_selections_and_the_active_file() {
        let changes = ["/p/a.ts".to_owned()];
        let project = ["/p/src".to_owned()];
        let context = PathContext {
            commit_selection: &changes,
            commit_window_focused: true,
            project_selection: &project,
            active_path: Some("/p/open.ts"),
        };
        assert_eq!(selected_paths(None, context), ["/p/a.ts"]);
        assert_eq!(selected_paths(Some(&json!(["/p/x.ts", 1])), context), ["/p/x.ts"]);
        let unfocused = PathContext { commit_window_focused: false, ..context };
        assert_eq!(selected_paths(None, unfocused), ["/p/src"]);
        assert_eq!(context_paths(None, unfocused), ["/p/open.ts"]);
        assert_eq!(arg_path(Some(&json!("/p/y.ts"))), Some("/p/y.ts"));
        assert!(commit_window_focused(Some("git"), "changes") && !commit_window_focused(Some("git"), "log"));
        let CompareSelection::Nothing(notice) = compare_with_branch(None, PathContext::default()) else {
            panic!("expected a notification");
        };
        assert_eq!(notice.title, "Open or select a file or folder to compare");
    }
}
