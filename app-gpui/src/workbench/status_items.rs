//! The built-in status bar items that are text or an icon with a click: what each says, its
//! tooltip, what a click does and where it sits, from the data the window already has.
//!
//! [`spec`] gives the registration of every built-in item (id, side, priority, title, LightEdit,
//! `when`); the `*_item` functions build the [`StatusText`] the bar draws, `None` while the item
//! has nothing to say. The menus some items open are here too, as rows whose commands the window
//! carries out.

use super::ENCODINGS;
use super::status_bar::{StatusAlign, StatusClick, StatusItemSpec, StatusText};
use super::tasks::BackgroundTask;
use crate::editor::view::EditorStatus;
use crate::features_a::vim::mode::VimModeState;
use crate::features_b::perf::{MemorySummary, indicator_label, indicator_tooltip};
use crate::platform::i18n::{I18n, Param};
use crate::vcs_ops::menu::MenuItem;
use serde_json::json;
use std::path::Path;

pub const BREADCRUMBS: &str = "editor.breadcrumbs";
pub const CARET: &str = "editor.caret";
pub const LINE_ENDING: &str = "editor.lineEnding";
pub const ENCODING: &str = "editor.encoding";
pub const INDENT: &str = "editor.indent";
pub const READ_ONLY: &str = "editor.readOnly";
pub const USAGES: &str = "editor-lsp.usagesStatus";
pub const AGENTS: &str = "agents.status";
pub const LIGHT_EDIT: &str = "app.lightEdit";
pub const LANGUAGE_SERVICES: &str = "code-insight.servers";
pub const MACRO_RECORDING: &str = "macros.recording";
pub const MEMORY: &str = "perf.memory";
pub const REMOTE: &str = "remote.location";
pub const PHP_LEVEL: &str = "php.languageLevel";
pub const PYTHON_INTERPRETER: &str = "python.interpreter";
pub const VIM: &str = "vim.mode";
pub const TASKS: &str = super::status_bar::TASKS_ITEM;
pub const INFO: &str = "workbench.info";

/// The action LightEdit's Open as Project runs.
pub const LIGHT_EDIT_OPEN_PROJECT: &str = "lightEdit.openProject";

/// A built-in item: (id, side, priority, title, hidden by default, shown in LightEdit).
type BuiltIn = (&'static str, StatusAlign, f64, Option<&'static str>, bool, bool);

/// Every built-in item.
const BUILT_INS: [BuiltIn; 18] = [
    (BREADCRUMBS, StatusAlign::Left, 0.0, Some("Navigation Bar"), false, false),
    (CARET, StatusAlign::Right, 20.0, Some("Line:Column Number"), false, true),
    (LINE_ENDING, StatusAlign::Right, 30.0, Some("Line Separator"), false, true),
    (ENCODING, StatusAlign::Right, 40.0, Some("File Encoding"), false, true),
    (INDENT, StatusAlign::Right, 50.0, Some("Indentation"), false, true),
    (READ_ONLY, StatusAlign::Right, 55.0, Some("Read-only Attribute"), false, true),
    (USAGES, StatusAlign::Left, 10.0, None, false, false),
    (AGENTS, StatusAlign::Right, 3.0, Some("Agents"), false, false),
    (LIGHT_EDIT, StatusAlign::Left, 20.0, None, false, true),
    (LANGUAGE_SERVICES, StatusAlign::Right, 5.0, Some("Language Services"), false, false),
    (MACRO_RECORDING, StatusAlign::Left, 4.0, None, false, false),
    (MEMORY, StatusAlign::Right, 100.0, Some("Memory Indicator"), true, false),
    (REMOTE, StatusAlign::Left, 100.0, Some("Remote Host"), false, false),
    (PHP_LEVEL, StatusAlign::Right, 89.0, Some("PHP Language Level"), false, false),
    (PYTHON_INTERPRETER, StatusAlign::Right, 90.0, Some("Python Interpreter"), false, false),
    (VIM, StatusAlign::Right, 45.0, Some("Vim"), false, true),
    (TASKS, StatusAlign::Right, 0.0, None, false, true),
    (INFO, StatusAlign::Left, 5.0, None, false, true),
];

/// The `when` clause of a built-in item, for those whose showing hangs on a context key rather
/// than on whether there is data to show.
fn built_in_when(id: &str) -> Option<&'static str> {
    match id {
        LIGHT_EDIT => Some("lightEdit"),
        MACRO_RECORDING => Some("macroRecording"),
        _ => None,
    }
}

/// The registration of built-in item `id`.
pub fn spec(id: &str) -> Option<StatusItemSpec> {
    let (id, align, priority, title, hidden, light_edit) = BUILT_INS.iter().find(|entry| entry.0 == id)?;
    let mut spec = StatusItemSpec::new(id, *align, *priority);
    if let Some(title) = title {
        spec = spec.title(title);
    }
    if *hidden {
        spec = spec.hidden_by_default();
    }
    if *light_edit {
        spec = spec.light_edit();
    }
    if let Some(when) = built_in_when(id) {
        spec = spec.when(when);
    }
    Some(spec)
}

/// The ids of every built-in item.
pub fn built_in_ids() -> impl Iterator<Item = &'static str> {
    BUILT_INS.iter().map(|entry| entry.0)
}

/// Line:Column: `12:5`, the selection's size when there is one, or how many carets there are.
/// A click opens Go to Line:Column.
pub fn caret_item(status: &EditorStatus, carets: usize) -> StatusText {
    let text = if carets > 1 { format!("{carets} carets") } else { status.caret_text() };
    StatusText::new(text)
        .tooltip("Go to Line:Column")
        .shortcut_of("navigate.gotoLine")
        .on_click(StatusClick::action("navigate.gotoLine"))
}

/// Line Separator: `LF`, `CRLF` or `CR`; a click opens the separators menu, except for a read-only
/// file, whose separator cannot be changed.
pub fn line_ending_item(status: &EditorStatus) -> StatusText {
    StatusText::new(status.line_ending_text())
        .tooltip("Line Separator")
        .on_click(StatusClick::Open)
        .enabled(!status.read_only)
}

/// The name an encoding is shown under: its entry in [`ENCODINGS`], or the id as the file has it.
pub fn encoding_name(encoding: &str) -> &str {
    let id = encoding.to_lowercase();
    ENCODINGS.iter().find(|(known, _)| *known == id).map_or(encoding, |(_, name)| name)
}

/// File Encoding: the encoding's name; a click opens Reload with / Convert to Encoding.
pub fn encoding_item(status: &EditorStatus) -> StatusText {
    StatusText::new(encoding_name(&status.encoding)).tooltip("File Encoding").on_click(StatusClick::Open)
}

/// The indentation of a file: how wide a tab is and whether indenting inserts spaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Indent {
    pub tab_size: u32,
    pub insert_spaces: bool,
}

impl Indent {
    /// From the editor's indentation unit (`edit_state().indent_unit`) and tab size.
    pub fn of_unit(unit: &str, tab_size: u32) -> Self {
        Self { tab_size, insert_spaces: !unit.starts_with('\t') }
    }

    fn label(self) -> String {
        if self.insert_spaces { format!("{} spaces", self.tab_size) } else { format!("Tab ({})", self.tab_size) }
    }
}

/// Indentation: `4 spaces` or `Tab (4)`; a click opens [`indent_menu`].
pub fn indent_item(indent: Indent) -> StatusText {
    StatusText::new(indent.label()).tooltip("Indentation").on_click(StatusClick::Open)
}

/// The Indentation item's menu: 2, 4 or 8 spaces or tabs for this file, the current one marked,
/// then Configure Indents… (Settings › Code Style).
pub fn indent_menu(current: Indent) -> Vec<MenuItem<Indent>> {
    let choice = |indent: Indent| {
        let row = MenuItem::item(indent.label(), indent);
        if indent == current { row.icon("check") } else { row }
    };
    vec![
        choice(Indent { tab_size: 2, insert_spaces: true }),
        choice(Indent { tab_size: 4, insert_spaces: true }),
        choice(Indent { tab_size: 8, insert_spaces: true }),
        choice(Indent { tab_size: current.tab_size, insert_spaces: false }),
        MenuItem::Separator,
        MenuItem::action_with("file.settings", json!("editor.codeStyle")).labelled("Configure Indents…"),
    ]
}

/// Read-only Attribute: a lock (read-only) or an open lock; a click flips the file's writable
/// bit. `None` for what is no file on disk (virtual documents, binary or too large files).
pub fn read_only_item(read_only: bool, on_disk: bool) -> Option<StatusText> {
    on_disk.then(|| {
        let label = if read_only { "Make file writable" } else { "Make file read-only" };
        StatusText::new("")
            .icon(if read_only { "lock" } else { "unlock" })
            .tooltip(label)
            .on_click(StatusClick::action("file.toggleReadOnly"))
    })
}

/// Background tasks: the task that is working (the newest one not waiting in a queue), with its
/// progress and how many more run; a click lists them all. `None` without tasks.
pub fn task_item(tasks: &[BackgroundTask], i18n: &I18n) -> Option<StatusText> {
    let running: Vec<&BackgroundTask> = tasks.iter().filter(|task| !task.done).collect();
    let current =
        running.iter().rev().find(|task| !task.queued).or(running.last()).copied().or_else(|| tasks.last())?;
    let tooltip = current.message.clone().unwrap_or_else(|| current.title.clone());
    let item = StatusText::new(i18n.tx(&current.title))
        .tooltip(tooltip)
        .progress(if current.done { Some(1.0) } else { current.fraction })
        .on_click(StatusClick::action("view.backgroundTasks"));
    let more = running.len().saturating_sub(1);
    Some(if more > 0 { item.suffix(format!("+{more}")) } else { item })
}

/// The memory indicator: `OpenIDEA 560M`, the breakdown in its tooltip; a click opens the Memory &
/// Diagnostics popup. `None` until metrics were fetched.
pub fn memory_item(summary: Option<&MemorySummary>) -> Option<StatusText> {
    let summary = summary?;
    Some(StatusText::new(indicator_label(summary)).tooltip(indicator_tooltip(summary)).on_click(StatusClick::Open))
}

/// Macro recording: shown while a macro records; a click stops the recording.
pub fn macro_item(recording: bool, i18n: &I18n) -> Option<StatusText> {
    recording.then(|| {
        StatusText::new(i18n.tx("Recording macro…"))
            .icon("record")
            .tooltip("Stop Macro Recording")
            .on_click(StatusClick::action("macro.stopRecording"))
    })
}

/// The Vim item: the mode (`NORMAL`, `INSERT`, `V-LINE`…) while Vim is on, a faint `Vim` while it
/// is off; a click opens [`vim_menu`].
pub fn vim_item(enabled: bool, mode: &VimModeState) -> StatusText {
    let text = if enabled { mode.label() } else { "Vim".to_owned() };
    StatusText::new(text)
        .mono()
        .dimmed(!enabled)
        .tooltip(if enabled { "Vim mode" } else { "Vim (disabled)" })
        .on_click(StatusClick::Open)
}

/// The Vim item's menu: Enabled (checked while on) flips Vim; then reload `~/.ideavimrc` and the
/// Vim settings. The row's command is the new enabled state.
pub fn vim_menu(enabled: bool) -> Vec<MenuItem<bool>> {
    let toggle = MenuItem::item("Enabled", !enabled);
    vec![
        if enabled { toggle.icon("check") } else { toggle },
        MenuItem::Separator,
        MenuItem::action("vim.reloadRc").labelled("Reload .ideavimrc"),
        MenuItem::action("vim.settings").labelled("Settings…"),
    ]
}

/// LightEdit's Open as Project: shown for a file on disk in a LightEdit window; a click opens its
/// folder as a project.
pub fn light_edit_item(path: Option<&Path>, i18n: &I18n) -> Option<StatusText> {
    let folder = path?.parent()?.to_string_lossy().into_owned();
    let tooltip = i18n.t("app.lightEdit.openProjectTooltip", &[("folder", Param::from(folder.as_str()))]);
    Some(
        StatusText::new(i18n.t("app.lightEdit.openProject", &[]))
            .icon("folder-open")
            .emphasis()
            .tooltip(tooltip)
            .on_click(StatusClick::action(LIGHT_EDIT_OPEN_PROJECT)),
    )
}

/// The transient status message (`showStatusInfo`), its text as tooltip too.
pub fn info_item(text: Option<&str>) -> Option<StatusText> {
    text.filter(|text| !text.is_empty()).map(|text| StatusText::new(text).tooltip(text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workbench::status_bar::StatusBarModel;
    use crate::workbench::tasks::{TaskEvent, Tasks};
    use ide_text::LineEnding;
    use serde_json::Value;

    fn status(line: u32, col: u32) -> EditorStatus {
        EditorStatus {
            line,
            col,
            selected_chars: 0,
            selected_line_breaks: 0,
            line_ending: LineEnding::Lf,
            encoding: "utf-8".to_owned(),
            read_only: false,
        }
    }

    #[test]
    fn every_built_in_is_registered_as_the_contributions_list_says() {
        let listed: Value =
            serde_json::from_str(include_str!("../../assets/gen/contributions.json")).expect("the list is JSON");
        let items = listed["registerStatusItem"].as_array().expect("status items are listed");
        let mut seen = 0;
        for item in items {
            // The plugin runtime registers its items under ids computed at runtime.
            let Some(id) = item["id"].as_str() else { continue };
            let spec = spec(id).unwrap_or_else(|| panic!("{id} is a built-in"));
            let align = if item["align"] == "left" { StatusAlign::Left } else { StatusAlign::Right };
            assert_eq!(spec.align, align, "{id}");
            assert_eq!(Some(spec.priority), item["priority"].as_f64(), "{id}");
            assert_eq!(spec.title.as_deref(), item["title"].as_str(), "{id}");
            assert_eq!(spec.light_edit, item["lightEdit"] == true, "{id}");
            assert_eq!(spec.default_visible, item["defaultVisible"].as_bool(), "{id}");
            seen += 1;
        }
        assert_eq!(seen, built_in_ids().count());
    }

    #[test]
    fn the_editor_items_sit_in_the_order_the_bar_draws_them() {
        let mut model = StatusBarModel::default();
        for id in built_in_ids() {
            model.register(spec(id).expect("a built-in"));
        }
        let (_, right) = model.order().ids();
        let at = |id: &str| right.iter().position(|shown| shown == id).expect("on the right");
        assert!(at(TASKS) < at(CARET) && at(CARET) < at(LINE_ENDING) && at(LINE_ENDING) < at(ENCODING));
        assert!(at(ENCODING) < at(VIM) && at(VIM) < at(INDENT) && at(INDENT) < at(READ_ONLY));
        assert_eq!(right.last().map(String::as_str), Some(MEMORY));
    }

    #[test]
    fn the_caret_item_shows_the_position_the_selection_or_the_carets() {
        let item = caret_item(&status(11, 4), 1);
        assert_eq!(item.text, "12:5");
        assert_eq!(item.tooltip.as_deref(), Some("Go to Line:Column"));
        assert_eq!(item.shortcut_action, Some("navigate.gotoLine"));
        assert_eq!(item.click, Some(StatusClick::action("navigate.gotoLine")));
        let selected = EditorStatus { selected_chars: 40, selected_line_breaks: 2, ..status(11, 4) };
        assert_eq!(caret_item(&selected, 1).text, "12:5 (40 chars, 2 line breaks)");
        assert_eq!(caret_item(&selected, 3).text, "3 carets");
    }

    #[test]
    fn the_line_separator_of_a_read_only_file_cannot_be_changed() {
        let item = line_ending_item(&EditorStatus { line_ending: LineEnding::CrLf, ..status(0, 0) });
        assert_eq!((item.text.as_str(), item.enabled, item.click), ("CRLF", true, Some(StatusClick::Open)));
        let read_only = line_ending_item(&EditorStatus { read_only: true, ..status(0, 0) });
        assert!(!read_only.enabled);
    }

    #[test]
    fn the_encoding_item_shows_the_encodings_name() {
        assert_eq!(encoding_item(&status(0, 0)).text, "UTF-8");
        let other = EditorStatus { encoding: "Shift_JIS".to_owned(), ..status(0, 0) };
        assert_eq!(encoding_item(&other).text, "Shift_JIS");
        let unknown = EditorStatus { encoding: "x-mac-roman".to_owned(), ..status(0, 0) };
        assert_eq!(encoding_item(&unknown).text, "x-mac-roman");
        assert_eq!(encoding_item(&status(0, 0)).tooltip.as_deref(), Some("File Encoding"));
    }

    #[test]
    fn the_indentation_item_names_spaces_or_tabs_and_marks_the_current_choice() {
        let four = Indent::of_unit("    ", 4);
        assert_eq!(indent_item(four).text, "4 spaces");
        let tabs = Indent::of_unit("\t", 8);
        assert_eq!(indent_item(tabs).text, "Tab (8)");
        let menu = indent_menu(four);
        let labels: Vec<Option<&str>> = menu.iter().map(MenuItem::label).collect();
        assert_eq!(
            labels,
            [Some("2 spaces"), Some("4 spaces"), Some("8 spaces"), Some("Tab (4)"), None, Some("Configure Indents…")]
        );
        let marked: Vec<bool> =
            menu.iter().map(|row| matches!(row, MenuItem::Item(entry) if entry.icon == Some("check"))).collect();
        assert_eq!(marked, [false, true, false, false, false, false]);
        assert_eq!(menu[3].command(), Some(&Indent { tab_size: 4, insert_spaces: false }));
        assert_eq!((menu[5].action_id(), menu[5].arg()), (Some("file.settings"), Some(&json!("editor.codeStyle"))));
    }

    #[test]
    fn the_lock_flips_the_writable_bit_of_files_on_disk_only() {
        let locked = read_only_item(true, true).expect("a file on disk");
        assert_eq!((locked.icon, locked.tooltip.as_deref()), (Some("lock"), Some("Make file writable")));
        assert_eq!(locked.click, Some(StatusClick::action("file.toggleReadOnly")));
        let open = read_only_item(false, true).expect("a file on disk");
        assert_eq!((open.icon, open.tooltip.as_deref()), (Some("unlock"), Some("Make file read-only")));
        assert_eq!(read_only_item(true, false), None);
    }

    fn event(id: u64, title: &str) -> TaskEvent {
        TaskEvent { id, title: title.to_owned(), message: None, fraction: None, done: false, cancellable: false }
    }

    // src/workbench/Workbench.test.tsx › background tasks › tracks task events and shows the current task
    #[test]
    fn tracks_task_events_and_shows_the_current_task() {
        let i18n = I18n::default();
        let mut tasks = Tasks::default();
        tasks.on_task_event(event(6, "Publish Diagnostics"), 0.0);
        tasks.on_task_event(TaskEvent { fraction: Some(0.5), ..event(7, "Building index") }, 0.0);
        assert_eq!(task_item(tasks.tasks(), &i18n), None, "nothing is listed before the show delay");
        tasks.on_task_event(TaskEvent { fraction: Some(1.0), done: true, ..event(6, "Publish Diagnostics") }, 0.0);
        tasks.expire(600.0);
        let item = task_item(tasks.tasks(), &i18n).expect("a task runs");
        assert_eq!(item.text, "Building index");
        assert_eq!(item.progress.and_then(|progress| progress.fraction), Some(0.5));
        assert_eq!(item.suffix, None);
        assert_eq!(item.click, Some(StatusClick::action("view.backgroundTasks")));
    }

    #[test]
    fn a_working_task_wins_over_the_queued_ones_and_the_others_are_counted() {
        let i18n = I18n::default();
        let mut tasks = Tasks::default();
        tasks.on_task_event(TaskEvent { message: Some("12 of 40".to_owned()), ..event(1, "Indexing") }, 0.0);
        tasks.on_task_event(event(2, "Fetching"), 0.0);
        tasks.expire(600.0);
        let item = task_item(tasks.tasks(), &i18n).expect("tasks run");
        assert_eq!((item.text.as_str(), item.suffix.as_deref()), ("Fetching", Some("+1")));
        assert_eq!(item.progress.map(|progress| progress.fraction), Some(None), "indeterminate");
        let listed = tasks.tasks().to_vec();
        let queued: Vec<BackgroundTask> =
            listed.iter().cloned().map(|task| BackgroundTask { queued: task.title == "Fetching", ..task }).collect();
        let item = task_item(&queued, &i18n).expect("tasks run");
        assert_eq!((item.text.as_str(), item.tooltip.as_deref()), ("Indexing", Some("12 of 40")));
        assert_eq!(task_item(&[], &i18n), None);
    }

    #[test]
    fn the_memory_indicator_shows_the_apps_footprint_once_measured() {
        assert_eq!(memory_item(None), None);
        let app = Some(560 * 1024 * 1024);
        let summary = MemorySummary { app, language_servers: None, processes: None, total: app };
        let item = memory_item(Some(&summary)).expect("measured");
        assert_eq!(item.text, indicator_label(&summary));
        assert_eq!(item.tooltip, Some(indicator_tooltip(&summary)));
        assert_eq!(item.click, Some(StatusClick::Open));
    }

    #[test]
    fn the_macro_item_shows_while_recording_and_stops_it() {
        let i18n = I18n::default();
        assert_eq!(macro_item(false, &i18n), None);
        let item = macro_item(true, &i18n).expect("recording");
        assert_eq!(item.text, "Recording macro…");
        assert_eq!(item.click, Some(StatusClick::action("macro.stopRecording")));
        assert_eq!(spec(MACRO_RECORDING).and_then(|spec| spec.when).as_deref(), Some("macroRecording"));
    }

    #[test]
    fn the_vim_item_shows_the_mode_while_on_and_a_faint_name_while_off() {
        let visual = VimModeState::changed("visual", Some("linewise"));
        let on = vim_item(true, &visual);
        assert_eq!((on.text.as_str(), on.dimmed, on.mono), ("V-LINE", false, true));
        assert_eq!(on.tooltip.as_deref(), Some("Vim mode"));
        let off = vim_item(false, &visual);
        assert_eq!((off.text.as_str(), off.dimmed), ("Vim", true));
        let menu = vim_menu(true);
        assert_eq!(menu[0].command(), Some(&false));
        assert!(matches!(&menu[0], MenuItem::Item(entry) if entry.icon == Some("check")));
        assert_eq!(menu[2].action_id(), Some("vim.reloadRc"));
        assert_eq!(vim_menu(false)[0].command(), Some(&true));
    }

    #[test]
    fn light_edit_offers_to_open_the_files_folder_as_a_project() {
        let i18n = I18n::default();
        let item = light_edit_item(Some(Path::new("/notes/today.md")), &i18n).expect("a file on disk");
        assert_eq!(item.text, "Open as Project");
        assert_eq!(item.tooltip.as_deref(), Some("Open /notes as a project"));
        assert_eq!(item.click, Some(StatusClick::action(LIGHT_EDIT_OPEN_PROJECT)));
        assert!(item.emphasis);
        assert_eq!(light_edit_item(None, &i18n), None);
        assert_eq!(spec(LIGHT_EDIT).and_then(|spec| spec.when).as_deref(), Some("lightEdit"));
    }

    #[test]
    fn the_status_message_is_shown_while_there_is_one() {
        assert_eq!(info_item(None), None);
        assert_eq!(info_item(Some("")), None);
        let item = info_item(Some("Copied path")).expect("a message");
        assert_eq!((item.text.as_str(), item.tooltip.as_deref()), ("Copied path", Some("Copied path")));
        assert_eq!(item.click, None);
    }
}
