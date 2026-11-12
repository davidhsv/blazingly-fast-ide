//! Compare entry points (Compare With…, Compare Directories, Compare Files, Compare with Clipboard,
//! Compare Texts) and the diff viewer's own actions.
//!
//! Each action is a sequence of decisions between the things only the application can do (ask the
//! backend what a path is, show a picker, read the clipboard): the functions here take what is known
//! so far and say what to do next. Notices are English titles for an informational notification of
//! the version control group.

use super::diff_input::{DiffData, DiffSide};
use super::diff_options::Side;
use super::doc::Direction;
use crate::platform::i18n::{I18n, Param};
use crate::vcs::changes::base_name;
use serde_json::Value;

/// The action ids of this module.
pub mod action {
    pub const OPEN_DIFF: &str = "vcs.openDiff";
    pub const NEXT_DIFFERENCE: &str = "diff.nextDifference";
    pub const PREV_DIFFERENCE: &str = "diff.prevDifference";
    pub const SHOW_SETTINGS: &str = "diff.showSettings";
    pub const NEXT_FILE: &str = "diff.nextFile";
    pub const PREV_FILE: &str = "diff.prevFile";
    pub const APPLY_LEFT_SIDE: &str = "diff.applyLeftSide";
    pub const APPLY_RIGHT_SIDE: &str = "diff.applyRightSide";
    pub const JUMP_TO_SOURCE: &str = "diff.jumpToSource";
    pub const COMPARE_WITH: &str = "diff.compareWith";
    pub const COMPARE_DIRECTORIES: &str = "diff.compareDirectories";
    pub const COMPARE_FILES: &str = "diff.compareFiles";
    pub const COMPARE_WITH_CLIPBOARD: &str = "diff.compareWithClipboard";
    pub const COMPARE_TEXTS: &str = "diff.compareTexts";
}

/// Project-relative path for labels.
fn display_path<'a>(path: &'a str, root: Option<&str>) -> &'a str {
    root.filter(|root| !root.is_empty())
        .and_then(|root| path.strip_prefix(root))
        .and_then(|rest| rest.strip_prefix('/'))
        .unwrap_or(path)
}

/// Diff of two files on disk; both sides are live, editable documents. `root` is the project root.
pub fn compare_files_data(left_path: &str, right_path: &str, root: Option<&str>) -> DiffData {
    DiffData::new(
        right_path,
        DiffSide::File { path: left_path.to_owned(), label: Some(display_path(left_path, root).to_owned()) },
        DiffSide::Worktree { label: Some(display_path(right_path, root).to_owned()) },
    )
}

/// The clipboard against the file, which stays editable, or against the editor's selection.
pub fn clipboard_diff_data(
    path: &str,
    clipboard: &str,
    selection: Option<&str>,
    root: Option<&str>,
    i18n: &mut I18n,
) -> DiffData {
    let name = Param::Text(base_name(path).to_owned());
    let selection = selection.filter(|selection| !selection.is_empty());
    let (title, right) = match selection {
        Some(selection) => {
            (i18n.tx_built(&["Clipboard ↔ ", " (selection)"], &[name]), DiffSide::text(selection, "Selection"))
        }
        None => (
            i18n.tx_built(&["Clipboard ↔ ", ""], &[name]),
            DiffSide::Worktree { label: Some(display_path(path, root).to_owned()) },
        ),
    };
    DiffData { title: Some(title), ..DiffData::new(path, DiffSide::text(clipboard, "Clipboard"), right) }
}

/// One side of the argument of Compare Texts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextSide {
    pub text: String,
    pub label: String,
}

/// The argument of Compare Texts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextsArg {
    pub path: String,
    pub title: Option<String>,
    pub left: TextSide,
    pub right: TextSide,
    pub line: Option<u32>,
}

/// A JSON value as text, as a label given in any type is shown.
fn label_text(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(text)) => text.clone(),
        Some(other) => other.to_string(),
    }
}

/// Reads the argument of Compare Texts: `{ path, title?, left: { text, label }, right: { text,
/// label }, line? }`. `None` unless the path and both texts are strings.
pub fn parse_texts_arg(arg: &Value) -> Option<TextsArg> {
    let side = |value: Option<&Value>| {
        let side = value?.as_object()?;
        Some(TextSide { text: side.get("text")?.as_str()?.to_owned(), label: label_text(side.get("label")) })
    };
    let arg = arg.as_object()?;
    let (left, right) = (side(arg.get("left")), side(arg.get("right")));
    Some(TextsArg {
        path: arg.get("path")?.as_str()?.to_owned(),
        title: arg.get("title").and_then(Value::as_str).map(str::to_owned),
        left: left?,
        right: right?,
        line: arg.get("line").and_then(Value::as_u64).and_then(|line| u32::try_from(line).ok()),
    })
}

/// The diff Compare Texts opens.
pub fn texts_diff_data(arg: &TextsArg) -> DiffData {
    DiffData {
        title: arg.title.clone(),
        line: arg.line,
        ..DiffData::new(
            &arg.path,
            DiffSide::text(&arg.left.text, &arg.left.label),
            DiffSide::text(&arg.right.text, &arg.right.label),
        )
    }
}

/// The path an action argument names: the argument itself, or its `path`.
pub fn arg_path(arg: Option<&Value>) -> Option<&str> {
    match arg? {
        Value::String(path) => Some(path.as_str()),
        Value::Object(fields) => fields.get("path")?.as_str(),
        _ => None,
    }
}

/// The paths an action argument names: a list, its `paths`, or the one path of [`arg_path`].
pub fn arg_paths(arg: Option<&Value>) -> Vec<String> {
    let listed = match arg {
        Some(Value::Array(items)) => Some(items),
        Some(Value::Object(fields)) => fields.get("paths").and_then(Value::as_array),
        _ => None,
    };
    match listed {
        Some(items) => items.iter().filter_map(Value::as_str).map(str::to_owned).collect(),
        None => arg_path(arg).map(str::to_owned).into_iter().collect(),
    }
}

/// The diff the Open Diff action shows for its argument, a [`DiffData`] in JSON (such as a pull
/// request file); `None` for anything else.
pub fn open_diff_arg(arg: Option<&Value>) -> Option<DiffData> {
    serde_json::from_value(arg?.clone()).ok()
}

/// What the focused diff viewer is asked to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavigatorCommand {
    /// Move to the next or previous difference.
    Difference(Direction),
    /// Open the next or previous file of the diff's chain.
    File(Direction),
    /// Show the viewer's settings as a popup menu.
    ShowSettings,
    /// Copy the change at the caret from this side to the other one.
    ApplyChange(Side),
    JumpToSource,
}

/// The command a diff viewer action sends to the focused viewer.
pub fn navigator_command(action_id: &str) -> Option<NavigatorCommand> {
    Some(match action_id {
        action::NEXT_DIFFERENCE => NavigatorCommand::Difference(Direction::Next),
        action::PREV_DIFFERENCE => NavigatorCommand::Difference(Direction::Previous),
        action::SHOW_SETTINGS => NavigatorCommand::ShowSettings,
        action::NEXT_FILE => NavigatorCommand::File(Direction::Next),
        action::PREV_FILE => NavigatorCommand::File(Direction::Previous),
        action::APPLY_LEFT_SIDE => NavigatorCommand::ApplyChange(Side::A),
        action::APPLY_RIGHT_SIDE => NavigatorCommand::ApplyChange(Side::B),
        action::JUMP_TO_SOURCE => NavigatorCommand::JumpToSource,
        _ => return None,
    })
}

/// Where the application runs, as far as picking a path goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompareEnv {
    /// The window works on a remote project: its paths are not on this machine.
    pub remote: bool,
}

/// How the user picks the other directory of Compare With.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DirectoryPicker {
    /// The system's folder dialog, opened at `default_path`.
    Native { title: &'static str, default_path: String },
    /// A text prompt for the path, in a monospace field, for paths the system's dialog cannot reach.
    Prompt { title: &'static str, label: &'static str, initial: String },
}

/// The system's file dialog for the other file of Compare With.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FilePicker {
    pub title: &'static str,
    /// The folder the dialog opens in; `None` leaves it to the system.
    pub default_path: Option<String>,
}

/// What Compare With does first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompareWithStart {
    /// Nothing to compare: show the notice.
    Notify(&'static str),
    /// Ask the backend whether `path` is a directory and go on with [`compare_with_kind`].
    CheckKind { path: String },
    /// `path` is known to be a directory: show the picker.
    PickDirectory { path: String, picker: DirectoryPicker },
}

/// What Compare With does once it knows what its path is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompareWithPick {
    PickDirectory { path: String, picker: DirectoryPicker },
    PickFile { path: String, picker: FilePicker },
}

/// How a comparison ends.
#[derive(Clone, Debug, PartialEq)]
pub enum CompareOutcome {
    /// Nothing happens (the picker was dismissed, or a path was compared with itself).
    Nothing,
    Notify(&'static str),
    /// Ask the backend whether `other` is a directory and go on with [`compare_with_directory`].
    CheckDirectory {
        other: String,
    },
    ShowDirectories {
        left: String,
        right: String,
    },
    /// Open the diff where the diffs the user opens go.
    OpenDiff(Box<DiffData>),
}

const COMPARE_WITH_TITLE: &str = "Compare With";

fn directory_picker(near: &str, env: CompareEnv) -> DirectoryPicker {
    if env.remote {
        DirectoryPicker::Prompt { title: COMPARE_WITH_TITLE, label: "Directory path", initial: near.to_owned() }
    } else {
        DirectoryPicker::Native { title: COMPARE_WITH_TITLE, default_path: near.to_owned() }
    }
}

/// Compare With…: the path is the argument's, else the first one selected in the project view, else
/// the active editor's. An argument with `directory: true` vouches for a directory.
pub fn compare_with_start(
    arg: Option<&Value>,
    selection: &[String],
    active_path: Option<&str>,
    env: CompareEnv,
) -> CompareWithStart {
    let path = arg_path(arg).or(selection.first().map(String::as_str)).or(active_path);
    let Some(path) = path.filter(|path| !path.is_empty()) else {
        return CompareWithStart::Notify("Open or select a file to compare");
    };
    let directory = arg.and_then(|arg| arg.get("directory")).and_then(Value::as_bool) == Some(true);
    if directory {
        return CompareWithStart::PickDirectory { path: path.to_owned(), picker: directory_picker(path, env) };
    }
    CompareWithStart::CheckKind { path: path.to_owned() }
}

/// Compare With… for a path the backend looked at: a directory is compared with a directory, a file
/// with a file. A path it could not look at counts as a file.
pub fn compare_with_kind(path: &str, is_directory: bool, env: CompareEnv) -> CompareWithPick {
    if is_directory {
        return CompareWithPick::PickDirectory { path: path.to_owned(), picker: directory_picker(path, env) };
    }
    let folder = path.rfind('/').map(|slash| &path[..slash]).filter(|folder| !folder.is_empty());
    let picker = FilePicker { title: COMPARE_WITH_TITLE, default_path: folder.map(str::to_owned) };
    CompareWithPick::PickFile { path: path.to_owned(), picker }
}

/// The user picked `other` (or dismissed the picker) as the directory to compare `path` with.
pub fn compare_with_directory_picked(path: &str, other: Option<&str>) -> CompareOutcome {
    match other.filter(|other| !other.is_empty() && *other != path) {
        Some(other) => CompareOutcome::CheckDirectory { other: other.to_owned() },
        None => CompareOutcome::Nothing,
    }
}

/// The backend looked at the picked path `other`.
pub fn compare_with_directory(path: &str, other: &str, is_directory: bool) -> CompareOutcome {
    if !is_directory {
        return CompareOutcome::Notify("Select a directory to compare");
    }
    CompareOutcome::ShowDirectories { left: path.to_owned(), right: other.to_owned() }
}

/// What the user picked to compare a file with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PickedComparison {
    File(String),
    /// The content of a file the application cannot name by path, with its name.
    Text {
        text: String,
        label: String,
    },
}

/// The user picked what to compare the file `path` with (or dismissed the picker). `root` is the
/// project root.
pub fn compare_with_file_picked(path: &str, picked: Option<PickedComparison>, root: Option<&str>) -> CompareOutcome {
    match picked {
        None => CompareOutcome::Nothing,
        Some(PickedComparison::File(other)) if other == path => CompareOutcome::Nothing,
        Some(PickedComparison::File(other)) => {
            CompareOutcome::OpenDiff(Box::new(compare_files_data(path, &other, root)))
        }
        Some(PickedComparison::Text { text, label }) => CompareOutcome::OpenDiff(Box::new(DiffData::new(
            path,
            DiffSide::Text { text, label },
            DiffSide::Worktree { label: Some(display_path(path, root).to_owned()) },
        ))),
    }
}

const TWO_DIRECTORIES: &str = "Select two directories to compare";

/// What Compare Directories does first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompareDirectoriesStart {
    Notify(&'static str),
    /// Ask the backend whether both paths are directories and go on with [`compare_directories`].
    CheckKinds {
        left: String,
        right: String,
    },
}

/// Compare Directories: the two paths of the argument, or without one the project view's selection.
pub fn compare_directories_start(arg: Option<&Value>, selection: &[String]) -> CompareDirectoriesStart {
    let paths = if arg.is_some() { arg_paths(arg) } else { selection.to_vec() };
    match <[String; 2]>::try_from(paths) {
        Ok([left, right]) => CompareDirectoriesStart::CheckKinds { left, right },
        Err(_) => CompareDirectoriesStart::Notify(TWO_DIRECTORIES),
    }
}

/// Compare Directories once the backend looked at both paths.
pub fn compare_directories(left: &str, right: &str, both_directories: bool) -> CompareOutcome {
    if !both_directories {
        return CompareOutcome::Notify(TWO_DIRECTORIES);
    }
    CompareOutcome::ShowDirectories { left: left.to_owned(), right: right.to_owned() }
}

/// Compare Files: the two files of the argument, folders left out. `root` is the project root.
pub fn compare_files(arg: Option<&Value>, root: Option<&str>) -> CompareOutcome {
    let paths: Vec<String> = arg_paths(arg).into_iter().filter(|path| !path.ends_with('/')).collect();
    match paths.as_slice() {
        [left, right] => CompareOutcome::OpenDiff(Box::new(compare_files_data(left, right, root))),
        _ => CompareOutcome::Notify("Select two files to compare"),
    }
}

/// The file Compare with Clipboard compares: the argument's, else the active editor's.
pub fn clipboard_target<'a>(arg: Option<&'a Value>, active_path: Option<&'a str>) -> Result<&'a str, &'static str> {
    arg_path(arg)
        .or(active_path)
        .filter(|path| !path.is_empty())
        .ok_or("Open or select a file to compare with the clipboard")
}

/// Compare with Clipboard once the clipboard was read (`None`: it could not be). `selection` is the
/// selected text of the active editor when that editor shows `path`.
pub fn compare_with_clipboard(
    path: &str,
    clipboard: Option<&str>,
    selection: Option<&str>,
    root: Option<&str>,
    i18n: &mut I18n,
) -> CompareOutcome {
    match clipboard {
        None => CompareOutcome::Notify("The clipboard could not be read"),
        Some(clipboard) => {
            CompareOutcome::OpenDiff(Box::new(clipboard_diff_data(path, clipboard, selection, root, i18n)))
        }
    }
}

/// Compare Texts: the diff of its argument, or nothing for an argument that does not parse.
pub fn compare_texts(arg: Option<&Value>) -> CompareOutcome {
    match arg.and_then(parse_texts_arg) {
        Some(texts) => CompareOutcome::OpenDiff(Box::new(texts_diff_data(&texts))),
        None => CompareOutcome::Nothing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::actions::manifest;
    use crate::platform::context::{ContextKeys, evaluate_when};
    use crate::vcs_diff::diff_input::{diff_input, diff_title, side_file, side_key};
    use serde_json::json;

    const LOCAL: CompareEnv = CompareEnv { remote: false };

    fn strings(paths: &[&str]) -> Vec<String> {
        paths.iter().map(|path| (*path).to_owned()).collect()
    }

    fn opened(outcome: CompareOutcome) -> DiffData {
        match outcome {
            CompareOutcome::OpenDiff(data) => *data,
            other => panic!("a diff opens, not {other:?}"),
        }
    }

    #[test]
    fn compare_files_makes_both_sides_live_files() {
        let data = compare_files_data("/p/a.ts", "/p/b.ts", None);
        assert_eq!(side_file(&data.left, &data.path), Some("/p/a.ts"));
        assert_eq!(side_file(&data.right, &data.path), Some("/p/b.ts"));
        assert_eq!(diff_title(&data), "a.ts ↔ b.ts");
    }

    #[test]
    fn compare_with_clipboard_the_file_or_the_selection() {
        let mut i18n = I18n::default();
        let whole = clipboard_diff_data("/p/a.ts", "clip", None, None, &mut i18n);
        assert_eq!(whole.left, DiffSide::text("clip", "Clipboard"));
        assert_eq!(side_file(&whole.right, &whole.path), Some("/p/a.ts"));
        let selection = clipboard_diff_data("/p/a.ts", "clip", Some("sel"), None, &mut i18n);
        assert_eq!(side_file(&selection.right, &selection.path), None);
        assert_eq!(diff_title(&selection), "Clipboard ↔ a.ts (selection)");
    }

    #[test]
    fn text_sides_get_distinct_tabs_per_content_and_compare_texts_arguments_are_validated() {
        assert_ne!(side_key(&DiffSide::text("a", "Original")), side_key(&DiffSide::text("b", "Original")));
        let arg = json!({
            "path": "/p/a",
            "left": { "text": "1", "label": "L" },
            "right": { "text": "2", "label": "R" }
        });
        assert_eq!(parse_texts_arg(&arg).map(|texts| texts.path), Some("/p/a".to_owned()));
        assert_eq!(parse_texts_arg(&json!({ "path": "/p/a", "left": { "text": 1 } })), None);
        let input = diff_input(&compare_files_data("/p/a", "/p/b", None)).expect("diff data is plain JSON");
        assert!(input.key.contains("file:/p/a"));
    }

    #[test]
    fn has_a_stable_searchable_action_and_resolves_the_selection_for_find_action() {
        let meta = manifest().get(action::COMPARE_DIRECTORIES).expect("the action is described");
        assert_eq!(meta.title, "Compare Directories");
        assert!(!meta.hidden);
        let mut context = ContextKeys::new();
        context.set("projectOpen", true);
        assert_eq!(evaluate_when(meta.when.as_deref(), &context).ok(), Some(true));
        // Without an argument the project view's selection is compared.
        let selection = strings(&["/p/left", "/p/right"]);
        let CompareDirectoriesStart::CheckKinds { left, right } = compare_directories_start(None, &selection) else {
            panic!("two paths are selected");
        };
        let shown = CompareOutcome::ShowDirectories { left: "/p/left".to_owned(), right: "/p/right".to_owned() };
        assert_eq!(compare_directories(&left, &right, true), shown);
        let arg = json!({ "paths": ["/x/left", "/x/right"] });
        let from_arg = compare_directories_start(Some(&arg), &selection);
        let (left, right) = ("/x/left".to_owned(), "/x/right".to_owned());
        assert_eq!(from_arg, CompareDirectoriesStart::CheckKinds { left, right });
    }

    #[test]
    fn rejects_mixed_selections_and_does_not_start_a_directory_comparison() {
        let arg = json!(["/p/left", "/p/file.txt"]);
        let CompareDirectoriesStart::CheckKinds { left, right } = compare_directories_start(Some(&arg), &[]) else {
            panic!("the argument names two paths");
        };
        // The second path is a file.
        let outcome = compare_directories(&left, &right, false);
        assert_eq!(outcome, CompareOutcome::Notify("Select two directories to compare"));
    }

    #[test]
    fn uses_the_native_folder_picker_for_compare_with_on_a_local_directory() {
        let arg = json!({ "path": "/p/left", "directory": true });
        let start = compare_with_start(Some(&arg), &[], None, LOCAL);
        let picker = DirectoryPicker::Native { title: "Compare With", default_path: "/p/left".to_owned() };
        assert_eq!(start, CompareWithStart::PickDirectory { path: "/p/left".to_owned(), picker });
        let picked = compare_with_directory_picked("/p/left", Some("/p/right"));
        assert_eq!(picked, CompareOutcome::CheckDirectory { other: "/p/right".to_owned() });
        let shown = CompareOutcome::ShowDirectories { left: "/p/left".to_owned(), right: "/p/right".to_owned() };
        assert_eq!(compare_with_directory("/p/left", "/p/right", true), shown);
    }

    #[test]
    fn uses_remote_paths_and_validates_the_selected_directory_via_the_routed_filesystem() {
        let remote = CompareEnv { remote: true };
        let selection = strings(&["/remote/left"]);
        let start = compare_with_start(None, &selection, None, remote);
        assert_eq!(start, CompareWithStart::CheckKind { path: "/remote/left".to_owned() });
        let CompareWithPick::PickDirectory { path, picker } = compare_with_kind("/remote/left", true, remote) else {
            panic!("a directory is compared with a directory");
        };
        let initial = "/remote/left".to_owned();
        assert_eq!(picker, DirectoryPicker::Prompt { title: "Compare With", label: "Directory path", initial });
        let picked = compare_with_directory_picked(&path, Some("/remote/right"));
        assert_eq!(picked, CompareOutcome::CheckDirectory { other: "/remote/right".to_owned() });
        let shown =
            CompareOutcome::ShowDirectories { left: "/remote/left".to_owned(), right: "/remote/right".to_owned() };
        assert_eq!(compare_with_directory(&path, "/remote/right", true), shown);
    }

    #[test]
    fn leaves_file_compare_with_on_the_existing_diff_flow() {
        let arg = json!("/p/a.txt");
        let start = compare_with_start(Some(&arg), &[], None, LOCAL);
        assert_eq!(start, CompareWithStart::CheckKind { path: "/p/a.txt".to_owned() });
        let CompareWithPick::PickFile { path, picker } = compare_with_kind("/p/a.txt", false, LOCAL) else {
            panic!("a file is compared with a file");
        };
        assert_eq!(picker, FilePicker { title: "Compare With", default_path: Some("/p".to_owned()) });
        let picked = Some(PickedComparison::File("/p/b.txt".to_owned()));
        assert_eq!(opened(compare_with_file_picked(&path, picked, None)).path, "/p/b.txt");
    }

    // Not from a spec: what each action does with nothing to compare, and the diffs the others open.
    #[test]
    fn reports_what_is_missing_and_opens_the_other_comparisons() {
        let mut i18n = I18n::default();
        let nothing = compare_with_start(None, &[], None, LOCAL);
        assert_eq!(nothing, CompareWithStart::Notify("Open or select a file to compare"));
        assert_eq!(compare_with_directory_picked("/p/a", Some("/p/a")), CompareOutcome::Nothing);
        assert_eq!(compare_with_directory_picked("/p/a", None), CompareOutcome::Nothing);
        let not_a_directory = compare_with_directory("/p/a", "/p/b.txt", false);
        assert_eq!(not_a_directory, CompareOutcome::Notify("Select a directory to compare"));
        let same = Some(PickedComparison::File("/p/a.txt".to_owned()));
        assert_eq!(compare_with_file_picked("/p/a.txt", same, None), CompareOutcome::Nothing);
        let text = Some(PickedComparison::Text { text: "t".to_owned(), label: "other.txt".to_owned() });
        let against_text = opened(compare_with_file_picked("/p/a.txt", text, Some("/p")));
        assert_eq!(against_text.left, DiffSide::text("t", "other.txt"));
        assert_eq!(against_text.right, DiffSide::Worktree { label: Some("a.txt".to_owned()) });
        let one = compare_directories_start(Some(&json!("/p/left")), &[]);
        assert_eq!(one, CompareDirectoriesStart::Notify("Select two directories to compare"));
        let files = json!(["/p/a.txt", "/p/dir/", "/p/b.txt"]);
        assert_eq!(opened(compare_files(Some(&files), None)).path, "/p/b.txt");
        let one_file = compare_files(Some(&json!(["/p/a.txt"])), None);
        assert_eq!(one_file, CompareOutcome::Notify("Select two files to compare"));
        assert!(clipboard_target(None, None).is_err());
        assert_eq!(clipboard_target(Some(&json!({ "path": "/p/a.ts" })), Some("/p/b.ts")), Ok("/p/a.ts"));
        let unread = compare_with_clipboard("/p/a.ts", None, None, None, &mut i18n);
        assert_eq!(unread, CompareOutcome::Notify("The clipboard could not be read"));
        let clip = opened(compare_with_clipboard("/p/a.ts", Some("clip"), None, None, &mut i18n));
        assert_eq!(diff_title(&clip), "Clipboard ↔ a.ts");
        let texts = json!({
            "path": "/p/a",
            "title": "T",
            "line": 4,
            "left": { "text": "1" },
            "right": { "text": "2" }
        });
        let diff = opened(compare_texts(Some(&texts)));
        assert_eq!((diff.title.as_deref(), diff.line), (Some("T"), Some(4)));
        assert_eq!(compare_texts(Some(&json!({ "path": 1 }))), CompareOutcome::Nothing);
        assert_eq!(open_diff_arg(Some(&serde_json::to_value(&diff).expect("plain JSON"))), Some(diff));
        assert_eq!(open_diff_arg(Some(&json!({ "path": "/p/a" }))), None);
        assert_eq!(navigator_command(action::NEXT_FILE), Some(NavigatorCommand::File(Direction::Next)));
        assert_eq!(navigator_command(action::APPLY_RIGHT_SIDE), Some(NavigatorCommand::ApplyChange(Side::B)));
        assert_eq!(navigator_command(action::COMPARE_WITH), None);
    }
}
