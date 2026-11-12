//! The Run/Debug Configurations dialog: the configurations by type (and folder) on the left, with
//! Add (the "Add New Configuration" type list), Remove, Copy, Create New Folder and Move Up / Down;
//! the selected configuration's form on the right, built from its type's fields, then the working
//! directory, environment, the options "Modify options" adds, the before launch tasks and the command
//! line it runs; Run, Debug, OK, Cancel and Apply below.
//!
//! The dialog edits a working copy of the store's configurations and never writes the store: OK,
//! Apply, Run and Debug hand the copy to the workbench as [`EditConfigsEvent::Save`], which answers
//! with [`EditConfigs::saved`]. Paths are picked by the workbench too ([`EditConfigsEvent::Browse`]).

use crate::platform::collate;
use crate::platform::command_line::{format_args, parse_args};
use crate::platform::i18n::Localization;
use crate::platform::input;
use crate::platform::keymap::is_mac_host;
use crate::run::before_launch::{LaunchEnv, task_label};
use crate::run::config_types::{BrowseKind, FieldControl, FieldOption, RunConfigType, RunConfigTypes, TypeEnv};
use crate::run::configs::{
    ConfigErrors, command_line, compound_cycle, config_icon, cwd_of, duplicate_config, format_env, group_configs,
    kind_label, parse_env, remove_config, to_cwd, upsert_config, validate_config,
};
use crate::run::env_dialog::env_from_rows;
use crate::run::java_launch::{is_jvm_application, parse_java_args};
use crate::run::store::RunStore;
use crate::run::types::{
    COMPOUND, blank_template, form_params, missing_params, new_config_of_type, terminal_applies, with_params,
};
use crate::theme;
use crate::ui::matching::fuzzy_match;
use crate::ui::text_field::{TextField, TextFieldEvent};
use gpui_kit::{
    AnyElement, App, ClipboardItem, Context, Div, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, Keystroke,
    MouseButton, Render, SharedString, Stateful, Subscription, Window, deferred, div, prelude::*, px,
};
use ide_run::{BeforeLaunchTask, BuildPolicy, RunConfig};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;

const WIDTH: f32 = 860.0;
const HEIGHT: f32 = 580.0;
const SIDEBAR_WIDTH: f32 = 270.0;
const LABEL_WIDTH: f32 = 150.0;
const ROW: f32 = 24.0;
const MENU_WIDTH: f32 = 300.0;

/// What happens once the working copy is saved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AfterSave {
    /// Apply: the dialog stays open.
    Stay,
    /// OK: select this configuration and close the dialog.
    Close { select: Option<String> },
    /// Run or Debug: close the dialog and launch this configuration (`run.runConfig` /
    /// `run.debugConfig`).
    Launch { id: String, debug: bool },
}

/// The configurations to save.
#[derive(Clone, Debug, PartialEq)]
pub struct SaveChanges {
    /// Every configuration of the dialog (for `RunStore::save_edited_configs`).
    pub draft: Vec<RunConfig>,
    /// Detected configurations removed here, to suppress when the user configurations are saved.
    pub removed_detected_ids: Vec<String>,
}

/// What the dialog asks of its window.
#[derive(Clone, Debug, PartialEq)]
pub enum EditConfigsEvent {
    /// OK, Apply, Run or Debug: save `changes` when there are any, then do `then`. Answer with
    /// [`EditConfigs::saved`].
    Save { changes: Option<SaveChanges>, then: AfterSave },
    /// Cancel or Escape: close the dialog without saving (`RunStore::editor_closed`).
    Cancelled,
    /// Pick a path for the field `target` in the local file dialog, opened at `from`; answer with
    /// [`EditConfigs::browse_picked`].
    Browse { target: String, kind: BrowseKind, multiple: bool, from: Option<String> },
}

/// A row of the configurations tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TreeRow {
    Group {
        kind: String,
        label: String,
        icon: String,
    },
    Folder {
        kind: String,
        name: String,
    },
    /// `depth` 2 inside a folder.
    Config {
        id: String,
        depth: u8,
    },
}

impl TreeRow {
    pub fn key(&self) -> String {
        match self {
            Self::Group { kind, .. } => format!("g:{kind}"),
            Self::Folder { kind, name } => format!("f:{kind}:{name}"),
            Self::Config { id, .. } => format!("c:{id}"),
        }
    }
}

/// The tree: a group per type, its folders first (with their configurations), then its loose
/// configurations. `extra_folders` (kind, name) are folders created here and still empty; collapsed
/// nodes (by key) hide their children.
pub fn tree_rows(
    configs: &[RunConfig],
    extra_folders: &[(String, String)],
    collapsed: &HashSet<String>,
    types: &RunConfigTypes,
) -> Vec<TreeRow> {
    let mut groups = group_configs(configs, types);
    for (kind, _) in extra_folders {
        if !groups.iter().any(|group| group.kind == *kind) {
            groups.push(crate::run::configs::ConfigGroup {
                kind: kind.clone(),
                label: kind_label(kind, types),
                configs: Vec::new(),
            });
        }
    }
    let mut rows = Vec::new();
    for group in groups {
        let row = TreeRow::Group {
            kind: group.kind.clone(),
            label: group.label.clone(),
            icon: config_icon(&group.kind, false, types),
        };
        let collapsed_group = collapsed.contains(&row.key());
        rows.push(row);
        if collapsed_group {
            continue;
        }
        let mut folders: Vec<String> = Vec::new();
        let named = group.configs.iter().filter_map(|config| config.extras.folder.clone());
        let extra = extra_folders.iter().filter(|(kind, _)| *kind == group.kind).map(|(_, name)| name.clone());
        for name in named.chain(extra) {
            if !folders.contains(&name) {
                folders.push(name);
            }
        }
        for name in folders {
            let row = TreeRow::Folder { kind: group.kind.clone(), name: name.clone() };
            let collapsed_folder = collapsed.contains(&row.key());
            rows.push(row);
            if collapsed_folder {
                continue;
            }
            let inside = group.configs.iter().filter(|config| config.extras.folder.as_deref() == Some(name.as_str()));
            rows.extend(inside.map(|config| TreeRow::Config { id: config.id.clone(), depth: 2 }));
        }
        let loose = group.configs.iter().filter(|config| config.extras.folder.is_none());
        rows.extend(loose.map(|config| TreeRow::Config { id: config.id.clone(), depth: 1 }));
    }
    rows
}

/// Moves `id` before (`delta` -1) or after (1) its neighbour among the configurations of the same type
/// and folder; anything else stays.
pub fn move_config(configs: &[RunConfig], id: &str, delta: isize) -> Vec<RunConfig> {
    let mut out = configs.to_vec();
    let Some(current) = configs.iter().position(|config| config.id == id) else { return out };
    let same = |config: &RunConfig| {
        config.kind == configs[current].kind && config.extras.folder == configs[current].extras.folder
    };
    let siblings: Vec<usize> = (0..configs.len()).filter(|&index| same(&configs[index])).collect();
    let Some(at) = siblings.iter().position(|&index| index == current) else { return out };
    if let Some(&other) = at.checked_add_signed(delta).and_then(|next| siblings.get(next)) {
        out.swap(current, other);
    }
    out
}

/// A row of "Add New Configuration".
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypeRow {
    Group {
        label: String,
    },
    /// `indices`: the matched characters of the label; `nested` under a group.
    Type {
        id: String,
        label: String,
        icon: String,
        indices: Option<Vec<usize>>,
        nested: bool,
    },
}

impl TypeRow {
    pub fn key(&self) -> String {
        match self {
            Self::Group { label } => format!("group:{label}"),
            Self::Type { id, .. } => id.clone(),
        }
    }
}

/// The types by label, those of a group (Docker, Servers) under it, narrowed to the ones whose label
/// (or group) matches `query`.
pub fn type_rows(types: &RunConfigTypes, query: &str) -> Vec<TypeRow> {
    let query = query.trim();
    let matched = |label: &str, group: Option<&str>| -> Option<Option<Vec<usize>>> {
        if query.is_empty() {
            return Some(None);
        }
        let in_group = group.is_some_and(|group| fuzzy_match(group, query).is_some());
        fuzzy_match(label, query).or_else(|| in_group.then(Vec::new)).map(Some)
    };
    // Entries by label: a type of its own, or (`None`) a group standing for its members.
    let mut entries: Vec<(String, Option<&RunConfigType>)> = Vec::new();
    for ty in types.iter() {
        match &ty.group {
            None => entries.push((ty.label.clone(), Some(ty))),
            Some(group) if !entries.iter().any(|(label, own)| own.is_none() && label == group) => {
                entries.push((group.clone(), None));
            }
            Some(_) => {}
        }
    }
    entries.sort_by(|a, b| collate::compare(&a.0, &b.0));
    let row = |ty: &RunConfigType, indices: Option<Vec<usize>>, nested: bool| TypeRow::Type {
        id: ty.id.clone(),
        label: ty.label.clone(),
        icon: ty.icon.clone(),
        indices,
        nested,
    };
    let mut rows = Vec::new();
    for (label, ty) in entries {
        if let Some(ty) = ty {
            if let Some(indices) = matched(&ty.label, None) {
                rows.push(row(ty, indices, false));
            }
            continue;
        }
        let members: Vec<TypeRow> = types
            .iter()
            .filter(|ty| ty.group.as_deref() == Some(label.as_str()))
            .filter_map(|ty| Some(row(ty, matched(&ty.label, Some(label.as_str()))?, true)))
            .collect();
        if !members.is_empty() {
            rows.push(TypeRow::Group { label });
            rows.extend(members);
        }
    }
    rows
}

/// The configurations a compound can add: every other one that is not a member yet and would not make
/// it include itself. As (id, name).
pub fn member_rows(config: &RunConfig, all: &[RunConfig]) -> Vec<(String, String)> {
    let members = &config.extras.compound;
    let closes_cycle = |candidate: &RunConfig| {
        let mut probe = config.clone();
        probe.extras.compound.push(candidate.id.clone());
        compound_cycle(&probe, all)
    };
    all.iter()
        .filter(|other| other.id != config.id && !members.contains(&other.id) && !closes_cycle(other))
        .map(|other| (other.id.clone(), other.name.clone()))
        .collect()
}

/// The errors of every user configuration of the dialog, by id: its own validation and its type's
/// required and rejected fields (which replace "Command is required").
pub fn config_errors(draft: &[RunConfig], types: &RunConfigTypes, env: &TypeEnv) -> BTreeMap<String, ConfigErrors> {
    let mut out = BTreeMap::new();
    for config in draft.iter().filter(|config| !config.detected) {
        let mut errors = validate_config(config, draft, types);
        let missing = missing_params(config, types.get(&config.kind), env);
        if !missing.is_empty() {
            errors.remove("command");
        }
        errors.extend(missing);
        if !errors.is_empty() {
            out.insert(config.id.clone(), errors);
        }
    }
    out
}

/// An option "Modify options" adds to the form.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RunOption {
    AllowParallel,
    StdinFile,
    EnvFiles,
    EmulateTerminal,
    OutputFile,
    ShowOnStdout,
    ShowOnStderr,
    IsTest,
    BuildPolicy,
    BeforeLaunch,
    OpenToolWindow,
    FocusToolWindow,
}

impl RunOption {
    fn label(self) -> &'static str {
        match self {
            Self::AllowParallel => "Allow multiple instances",
            Self::StdinFile => "Redirect input",
            Self::EnvFiles => "Paths to \".env\" files",
            Self::EmulateTerminal => "Emulate terminal in output console",
            Self::OutputFile => "Save console output to file",
            Self::ShowOnStdout => "Show console when a message is printed to stdout",
            Self::ShowOnStderr => "Show console when a message is printed to stderr",
            Self::IsTest => "Show results in the test runner",
            Self::BuildPolicy => "Build",
            Self::BeforeLaunch => "Add before launch task",
            Self::OpenToolWindow => "Open run/debug tool window when started",
            Self::FocusToolWindow => "Focus run/debug tool window when started",
        }
    }

    /// An option with a value field: adding it shows the field, removing it clears the value.
    fn has_value(self) -> bool {
        matches!(self, Self::StdinFile | Self::EnvFiles | Self::OutputFile | Self::BeforeLaunch | Self::BuildPolicy)
    }

    /// Whether `config` has a value other than the default for the option.
    fn is_set(self, config: &RunConfig, open_by_default: bool) -> bool {
        let extras = &config.extras;
        match self {
            Self::AllowParallel => config.allow_parallel,
            Self::StdinFile => extras.stdin_file.is_some(),
            Self::EnvFiles => !extras.env_files.is_empty(),
            Self::EmulateTerminal => extras.emulate_terminal,
            Self::OutputFile => extras.output_file.is_some(),
            Self::ShowOnStdout => extras.show_on_stdout,
            Self::ShowOnStderr => extras.show_on_stderr,
            Self::IsTest => config.is_test,
            Self::BuildPolicy => extras.build_policy.unwrap_or_default() != BuildPolicy::Automatic,
            Self::BeforeLaunch => !extras.before_launch.is_empty(),
            Self::OpenToolWindow => extras.open_tool_window.unwrap_or(open_by_default),
            Self::FocusToolWindow => extras.focus_tool_window,
        }
    }

    /// Turns the option on or off in `config`.
    fn apply(self, config: &mut RunConfig, on: bool) {
        let extras = &mut config.extras;
        match self {
            Self::AllowParallel => config.allow_parallel = on,
            Self::StdinFile => extras.stdin_file = None,
            Self::EnvFiles => extras.env_files.clear(),
            Self::EmulateTerminal => extras.emulate_terminal = on,
            Self::OutputFile => extras.output_file = None,
            Self::ShowOnStdout => extras.show_on_stdout = on,
            Self::ShowOnStderr => extras.show_on_stderr = on,
            Self::IsTest => {
                config.is_test = on;
                if on {
                    extras.emulate_terminal = false;
                }
            }
            Self::BuildPolicy => extras.build_policy = None,
            Self::BeforeLaunch => extras.before_launch.clear(),
            Self::OpenToolWindow => extras.open_tool_window = Some(on),
            Self::FocusToolWindow => extras.focus_tool_window = on,
        }
    }
}

/// The groups of "Modify options"; `process` groups apply only to configurations that start a process.
const OPTION_GROUPS: [(&str, &[RunOption], bool); 4] = [
    (
        "Operating System",
        &[RunOption::AllowParallel, RunOption::StdinFile, RunOption::EnvFiles, RunOption::EmulateTerminal],
        true,
    ),
    ("Logs", &[RunOption::OutputFile, RunOption::ShowOnStdout, RunOption::ShowOnStderr], true),
    ("Test", &[RunOption::IsTest], true),
    (
        "Before Launch",
        &[RunOption::BuildPolicy, RunOption::BeforeLaunch, RunOption::OpenToolWindow, RunOption::FocusToolWindow],
        false,
    ),
];

/// The flag options shown as removable chips under the form.
const CHIPS: [RunOption; 7] = [
    RunOption::AllowParallel,
    RunOption::EmulateTerminal,
    RunOption::ShowOnStdout,
    RunOption::ShowOnStderr,
    RunOption::OpenToolWindow,
    RunOption::FocusToolWindow,
    RunOption::IsTest,
];

/// Debug adapters with a launch template, as (id, label).
const ADAPTERS: [(&str, &str); 10] = [
    ("netcoredbg", "netcoredbg (.NET)"),
    ("java-debug", "java-debug (Java)"),
    ("kotlin-debug-adapter", "kotlin-debug-adapter (Kotlin)"),
    ("debugpy", "debugpy (Python)"),
    ("js-debug", "js-debug (Node.js / TypeScript)"),
    ("delve", "Delve (Go)"),
    ("codelldb", "CodeLLDB (Rust, C, C++)"),
    ("lldb-dap", "lldb-dap (C, C++, Swift)"),
    ("php-debug", "php-debug (PHP)"),
    ("rdbg", "rdbg (Ruby)"),
];

const BUILD_POLICIES: [(BuildPolicy, &str); 5] = [
    (BuildPolicy::Automatic, "Automatic (incremental)"),
    (BuildPolicy::Make, "Build (Make)"),
    (BuildPolicy::Rebuild, "Rebuild"),
    (BuildPolicy::NoBuild, "No build"),
    (BuildPolicy::MakeAndContinueOnErrors, "Build, no error check"),
];

/// The build policies a configuration of `kind` can use: Gradle cannot skip building or ignore
/// errors, Maven cannot skip building.
fn build_policies(kind: &str) -> Vec<(BuildPolicy, &'static str)> {
    BUILD_POLICIES
        .into_iter()
        .filter(|(policy, _)| match kind {
            "gradle" => !matches!(policy, BuildPolicy::NoBuild | BuildPolicy::MakeAndContinueOnErrors),
            "maven" => *policy != BuildPolicy::NoBuild,
            _ => true,
        })
        .collect()
}

fn build_policy_applies(config: &RunConfig) -> bool {
    is_jvm_application(&config.kind) || matches!(config.kind.as_str(), "maven" | "gradle")
}

/// What is selected in the tree.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Selection {
    Config(String),
    Folder { kind: String, name: String },
}

/// A path field's picker; `EnvFiles` adds several `.env` files.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Picker {
    Path(BrowseKind),
    EnvFiles,
}

/// One piece of the form, in order.
#[derive(Clone, Debug, PartialEq)]
enum FormItem {
    Notice(String),
    /// A text field edited under `key`; `value` is its text when it is created.
    Text {
        key: String,
        label: String,
        value: String,
        placeholder: Option<String>,
        error: Option<String>,
        hint: Option<String>,
        picker: Option<Picker>,
    },
    Check {
        key: String,
        label: String,
        on: bool,
    },
    Select {
        key: String,
        label: String,
        value: String,
        options: Vec<FieldOption>,
        error: Option<String>,
    },
    /// Read-only text the form computes, with Copy.
    Output {
        key: String,
        label: String,
        text: String,
    },
    /// The type's name and "Modify options".
    Section(String),
    /// A compound's configurations, as (id, name).
    Members {
        members: Vec<(String, String)>,
        error: Option<String>,
        can_add: bool,
    },
    /// The before launch tasks: their labels, and whether each is a shell command edited in place.
    Tasks {
        tasks: Vec<(String, bool)>,
        error: Option<String>,
    },
    Chips(Vec<RunOption>),
    /// The command line it runs.
    Preview(String),
}

/// A menu of the dialog.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Menu {
    /// "Add New Configuration".
    AddType,
    /// The choices of the select field `key`.
    Select(String),
    Options,
    AddTask,
    AddMember,
}

/// What a menu row does.
#[derive(Clone, Debug, PartialEq, Eq)]
enum MenuChoice {
    Value { key: String, value: String },
    Option(RunOption),
    Task(BeforeLaunchTask),
    Member(String),
}

/// A row of a menu: a header, or a choice (`None`: disabled).
#[derive(Clone, Debug, PartialEq, Eq)]
struct MenuRow {
    label: String,
    header: bool,
    checked: bool,
    choice: Option<MenuChoice>,
}

impl MenuRow {
    fn header(label: &str) -> Self {
        Self { label: label.to_owned(), header: true, checked: false, choice: None }
    }

    fn item(label: impl Into<String>, checked: bool, choice: Option<MenuChoice>) -> Self {
        Self { label: label.into(), header: false, checked, choice }
    }
}

fn tx(cx: &App, text: &str) -> String {
    cx.try_global::<Localization>().map_or_else(|| text.to_owned(), |localization| localization.0.tx(text))
}

/// Text built at run time, translated through the catalog's templates.
fn tx_template(cx: &App, text: &str) -> String {
    cx.try_global::<Localization>().map_or_else(|| text.to_owned(), |localization| localization.0.tx_template(text))
}

/// `path` relative to `root` when it lies inside it ("" for the root itself).
fn project_relative(root: Option<&str>, path: &str) -> String {
    let Some(root) = root else { return path.to_owned() };
    if path == root {
        return String::new();
    }
    path.strip_prefix(root).and_then(|rest| rest.strip_prefix('/')).unwrap_or(path).to_owned()
}

/// `New Folder`, or `New Folder (2)`… when the type already has a folder with that name.
fn unique_folder(kind: &str, configs: &[RunConfig], extra: &[(String, String)]) -> String {
    let taken = |name: &str| {
        configs.iter().any(|config| config.kind == kind && config.extras.folder.as_deref() == Some(name))
            || extra.iter().any(|(own, folder)| own == kind && folder == name)
    };
    if !taken("New Folder") {
        return "New Folder".to_owned();
    }
    (2u32..).map(|i| format!("New Folder ({i})")).find(|name| !taken(name)).unwrap_or_default()
}

pub struct EditConfigs {
    types: Rc<RunConfigTypes>,
    env: TypeEnv,
    draft: Vec<RunConfig>,
    /// The temporary configurations when the dialog opened.
    temporary: Vec<String>,
    /// Configuration templates by kind ("Edit configuration templates…"), applied to added ones.
    templates: Vec<RunConfig>,
    project_root: Option<String>,
    selection: Option<Selection>,
    extra_folders: Vec<(String, String)>,
    collapsed: HashSet<String>,
    dirty: bool,
    removed_detected_ids: Vec<String>,
    saving: bool,
    save_error: Option<String>,
    /// Why the environment field's text is not a valid environment.
    env_error: Option<String>,
    /// Value options added through "Modify options" for the selected configuration.
    shown: HashSet<RunOption>,
    menu: Option<Menu>,
    add_field: Entity<TextField>,
    /// The selected row of "Add New Configuration".
    add_active: Option<usize>,
    /// The form's text fields by key, for the selected configuration.
    fields: HashMap<String, (Entity<TextField>, Subscription)>,
    tree_focus: FocusHandle,
    show_on_start: bool,
    debug_available: bool,
    external_tools: Vec<(String, String)>,
    tool_windows: Vec<(String, String)>,
    key_claims: Vec<u64>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<EditConfigsEvent> for EditConfigs {}

impl EditConfigs {
    /// The dialog on the configurations of `store`, with the store's edit target (else its selection)
    /// selected. `templates` are the project's configuration templates.
    pub fn new(
        store: &Entity<RunStore>,
        types: Rc<RunConfigTypes>,
        templates: Vec<RunConfig>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (draft, temporary, target, dirty, project_root) = {
            let store = store.read(cx);
            let target = store.edit_target.clone().or_else(|| store.selected_id.clone());
            (store.configs().to_vec(), store.temporary.clone(), target, store.edit_dirty, store.project_root.clone())
        };
        let selection = target
            .filter(|id| draft.iter().any(|config| config.id == *id))
            .or_else(|| draft.first().map(|config| config.id.clone()))
            .map(Selection::Config);
        let placeholder = tx(cx, "Search types");
        let add_field = cx.new(|cx| TextField::new(cx).with_placeholder(placeholder));
        let subscriptions =
            vec![cx.subscribe_in(&add_field, window, |view, _, event: &TextFieldEvent, window, cx| match event {
                TextFieldEvent::Changed => {
                    view.add_active = view.first_type_row(cx);
                    cx.notify();
                }
                TextFieldEvent::Submitted => {}
                TextFieldEvent::Cancelled => view.close_menu(window, cx),
            })];
        let tree_focus = cx.focus_handle();
        let tree = cx.weak_entity();
        let tree_claim = input::claim_keys(
            tree_focus.clone(),
            move |keystroke, window, cx| {
                tree.update(cx, |view, cx| view.tree_key(keystroke, window, cx)).unwrap_or(false)
            },
            cx,
        );
        let popup = cx.weak_entity();
        let add_claim = input::claim_keys(
            add_field.focus_handle(cx),
            move |keystroke, window, cx| {
                popup.update(cx, |view, cx| view.add_key(keystroke, window, cx)).unwrap_or(false)
            },
            cx,
        );
        cx.on_release(|view, cx| {
            for claim in &view.key_claims {
                input::release_keys(*claim, cx);
            }
        })
        .detach();
        Self {
            types,
            env: TypeEnv { windows: cfg!(windows), gradle_local: false },
            draft,
            temporary,
            templates,
            project_root,
            selection,
            extra_folders: Vec::new(),
            collapsed: HashSet::new(),
            dirty,
            removed_detected_ids: Vec::new(),
            saving: false,
            save_error: None,
            env_error: None,
            shown: HashSet::new(),
            menu: None,
            add_field,
            add_active: None,
            fields: HashMap::new(),
            tree_focus,
            show_on_start: true,
            debug_available: true,
            external_tools: Vec::new(),
            tool_windows: Vec::new(),
            key_claims: vec![tree_claim, add_claim],
            _subscriptions: subscriptions,
        }
    }

    /// What new configurations and command lines read from the settings.
    pub fn set_type_env(&mut self, env: TypeEnv, cx: &mut Context<Self>) {
        self.env = env;
        cx.notify();
    }

    /// Settings › Run › "Show the Run tool window when a process starts", whether Debug is offered,
    /// and what the before launch tasks can name: external tools and tool windows, as (id, name).
    pub fn set_capabilities(
        &mut self,
        show_on_start: bool,
        debug: bool,
        external_tools: Vec<(String, String)>,
        tool_windows: Vec<(String, String)>,
        cx: &mut Context<Self>,
    ) {
        (self.show_on_start, self.debug_available) = (show_on_start, debug);
        (self.external_tools, self.tool_windows) = (external_tools, tool_windows);
        cx.notify();
    }

    // -----------------------------------------------------------------------------------------------
    // Accessors

    /// The working copy.
    pub fn draft(&self) -> &[RunConfig] {
        &self.draft
    }

    /// The tree as text: `[Type]`, `Folder/`, and a configuration's name.
    pub fn tree_lines(&self) -> Vec<String> {
        self.rows().iter().map(|row| self.tree_line(row)).collect()
    }

    /// The selected tree row, as [`Self::tree_lines`] spells it.
    pub fn selected(&self) -> Option<String> {
        let rows = self.rows();
        let index = self.selected_row(&rows)?;
        Some(self.tree_line(&rows[index]))
    }

    /// The toolbar's buttons: their titles and whether they can be used.
    pub fn toolbar(&self) -> Vec<(String, bool)> {
        self.toolbar_buttons().into_iter().map(|(_, _, title, enabled)| (title.to_owned(), enabled)).collect()
    }

    /// The form as text: `Label: value`, `[x] Label`, notices, `! error`, `Command line: …`.
    pub fn form_lines(&self, cx: &App) -> Vec<String> {
        let mut lines = Vec::new();
        for item in self.form_items() {
            match item {
                FormItem::Notice(text) => lines.push(text),
                FormItem::Text { key, label, value, error, .. } => {
                    let text = self.fields.get(&key).map_or(value, |(field, _)| field.read(cx).text().to_owned());
                    lines.push(format!("{label}: {text}"));
                    lines.extend(error.map(|error| format!("! {error}")));
                }
                FormItem::Check { label, on, .. } => lines.push(format!("[{}] {label}", if on { "x" } else { " " })),
                FormItem::Select { label, value, options, error, .. } => {
                    let chosen = options.iter().find(|option| option.value == value);
                    let shown = chosen.map_or(value.clone(), |option| option.label.clone());
                    lines.push(format!("{label}: {shown}"));
                    lines.extend(error.map(|error| format!("! {error}")));
                }
                FormItem::Output { label, text, .. } => lines.push(format!("{label}: {text}")),
                FormItem::Section(title) => lines.push(format!("== {title} ==")),
                FormItem::Members { members, error, .. } => {
                    let names: Vec<String> = members.into_iter().map(|(_, name)| name).collect();
                    lines.push(format!("Configurations: {}", names.join(", ")));
                    lines.extend(error.map(|error| format!("! {error}")));
                }
                FormItem::Tasks { tasks, error } => {
                    let labels: Vec<String> = tasks.into_iter().map(|(label, _)| label).collect();
                    lines.push(format!("Before launch: {}", labels.join("; ")));
                    lines.extend(error.map(|error| format!("! {error}")));
                }
                FormItem::Chips(chips) => {
                    let labels: Vec<&str> = chips.iter().map(|chip| chip.label()).collect();
                    lines.push(format!("Options: {}", labels.join(", ")));
                }
                FormItem::Preview(line) => lines.push(format!("Command line: {line}")),
            }
        }
        lines
    }

    /// The open menu's rows: `== Header ==`, `[x] Label` for a checked row, `Label (disabled)`.
    pub fn menu_lines(&self) -> Vec<String> {
        self.menu_rows()
            .into_iter()
            .map(|row| match (row.header, row.checked, row.choice.is_some()) {
                (true, _, _) => format!("== {} ==", row.label),
                (_, true, _) => format!("[x] {}", row.label),
                (_, false, false) => format!("{} (disabled)", row.label),
                _ => row.label,
            })
            .collect()
    }

    /// "Add New Configuration"'s rows while it is open: `[Group]` and type labels.
    pub fn type_lines(&self, cx: &App) -> Vec<String> {
        if self.menu != Some(Menu::AddType) {
            return Vec::new();
        }
        self.type_rows(cx)
            .iter()
            .map(|row| match row {
                TypeRow::Group { label } => format!("[{label}]"),
                TypeRow::Type { label, .. } => label.clone(),
            })
            .collect()
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn is_invalid(&self) -> bool {
        self.env_error.is_some() || !config_errors(&self.draft, &self.types, &self.env).is_empty()
    }

    pub fn can_apply(&self) -> bool {
        self.dirty && !self.saving && !self.is_invalid()
    }

    pub fn can_ok(&self) -> bool {
        !self.saving && !self.is_invalid()
    }

    /// Run and Debug: the selected configuration, applied and launched.
    pub fn can_run(&self) -> bool {
        self.current().is_some() && self.can_ok()
    }

    pub fn save_error(&self) -> Option<&str> {
        self.save_error.as_deref()
    }

    /// The text field of the form under `key` (`name`, `command`, `args`, `cwd`, `env`, `envFiles`,
    /// `stdinFile`, `outputFile`, `param:<field>`, `task:<index>`, `folder`), once it is drawn.
    pub fn field(&self, key: &str) -> Option<Entity<TextField>> {
        self.fields.get(key).map(|(field, _)| field.clone())
    }

    pub fn tree_focus(&self) -> &FocusHandle {
        &self.tree_focus
    }

    // -----------------------------------------------------------------------------------------------
    // The working copy

    fn current(&self) -> Option<&RunConfig> {
        match &self.selection {
            Some(Selection::Config(id)) => self.draft.iter().find(|config| config.id == *id),
            _ => None,
        }
    }

    fn rows(&self) -> Vec<TreeRow> {
        tree_rows(&self.draft, &self.extra_folders, &self.collapsed, &self.types)
    }

    fn tree_line(&self, row: &TreeRow) -> String {
        match row {
            TreeRow::Group { label, .. } => format!("[{label}]"),
            TreeRow::Folder { name, .. } => format!("{name}/"),
            TreeRow::Config { id, .. } => {
                self.draft.iter().find(|config| config.id == *id).map(|config| config.name.clone()).unwrap_or_default()
            }
        }
    }

    fn selected_row(&self, rows: &[TreeRow]) -> Option<usize> {
        rows.iter().position(|row| match (row, &self.selection) {
            (TreeRow::Config { id, .. }, Some(Selection::Config(selected))) => id == selected,
            (TreeRow::Folder { kind, name }, Some(Selection::Folder { kind: own, name: folder })) => {
                kind == own && name == folder
            }
            _ => false,
        })
    }

    /// Selects `selection`; the form is drawn anew for it.
    fn select(&mut self, selection: Option<Selection>, cx: &mut Context<Self>) {
        if self.selection != selection {
            self.selection = selection;
            self.fields.clear();
            self.shown.clear();
            self.env_error = None;
            self.menu = None;
        }
        cx.notify();
    }

    /// Selects the tree row at `index`; a type's row opens or closes it.
    pub fn click_row(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(row) = self.rows().into_iter().nth(index) else { return };
        match row {
            TreeRow::Group { .. } => self.toggle_collapsed(&row.key(), cx),
            TreeRow::Folder { kind, name } => self.select(Some(Selection::Folder { kind, name }), cx),
            TreeRow::Config { id, .. } => self.select(Some(Selection::Config(id)), cx),
        }
    }

    fn toggle_collapsed(&mut self, key: &str, cx: &mut Context<Self>) {
        if !self.collapsed.remove(key) {
            self.collapsed.insert(key.to_owned());
        }
        cx.notify();
    }

    /// Replaces the selected configuration (the edit makes a detected one a user configuration).
    fn replace(&mut self, next: RunConfig, cx: &mut Context<Self>) {
        self.draft = upsert_config(&self.draft, &next);
        self.dirty = true;
        cx.notify();
    }

    fn edit_current(&mut self, edit: impl FnOnce(&mut RunConfig), cx: &mut Context<Self>) {
        let Some(mut config) = self.current().cloned() else { return };
        edit(&mut config);
        self.replace(config, cx);
    }

    /// Sets the type param `key` to `value` (removed when empty), with the command line it produces.
    fn set_param(&mut self, key: &str, value: &str, cx: &mut Context<Self>) {
        let Some(config) = self.current().cloned() else { return };
        let ty = self.types.get(&config.kind);
        let mut params = form_params(&config, ty, &self.env).unwrap_or_default();
        if value.is_empty() {
            params.remove(key);
        } else {
            params.insert(key.to_owned(), value.to_owned());
        }
        let next = with_params(&config, params, ty, &self.env);
        self.replace(next, cx);
    }

    /// Puts `text` into the field `key` when it is drawn, for a value set from outside the field.
    fn sync_field(&mut self, key: &str, text: &str, cx: &mut Context<Self>) {
        if let Some((field, _)) = self.fields.get(key) {
            field.update(cx, |field, cx| field.set_text(text, cx));
        }
    }

    /// The text of the field `key` changed.
    fn field_edited(&mut self, key: &str, text: &str, cx: &mut Context<Self>) {
        let some = |text: &str| Some(text.trim()).filter(|text| !text.is_empty()).map(str::to_owned);
        match key {
            "name" => self.edit_current(|config| config.name = text.to_owned(), cx),
            "command" => self.edit_current(
                |config| {
                    config.command = text.to_owned();
                    config.extras.params.clear();
                },
                cx,
            ),
            "args" => self.edit_current(
                |config| {
                    let args = parse_args(text, false);
                    if parse_java_args(&args).main_class != parse_java_args(&config.args).main_class {
                        config.extras.module = None;
                    }
                    config.args = args;
                    config.extras.params.clear();
                },
                cx,
            ),
            "cwd" => self.edit_current(|config| config.cwd = to_cwd(some(text).as_deref()), cx),
            "env" => {
                let entries = parse_env(text);
                let env = match env_from_rows(&entries) {
                    Ok(env) => {
                        self.env_error = None;
                        env
                    }
                    Err((env, error)) => {
                        self.env_error = Some(error);
                        env
                    }
                };
                self.edit_current(|config| config.env = env, cx);
            }
            "envFiles" => self.edit_current(
                |config| {
                    let files = text.split(';').map(str::trim).filter(|file| !file.is_empty());
                    config.extras.env_files = files.map(str::to_owned).collect();
                },
                cx,
            ),
            "stdinFile" => self.edit_current(|config| config.extras.stdin_file = some(text), cx),
            "outputFile" => self.edit_current(|config| config.extras.output_file = some(text), cx),
            "folder" => {}
            task if task.starts_with("task:") => {
                let Some(index) = task["task:".len()..].parse::<usize>().ok() else { return };
                self.edit_current(
                    |config| {
                        if let Some(task) = config.extras.before_launch.get_mut(index) {
                            task.target = text.to_owned();
                        }
                    },
                    cx,
                );
            }
            param => {
                if let Some(param) = param.strip_prefix("param:") {
                    self.set_param(param, text, cx);
                }
            }
        }
    }

    /// Enter in the field `key`: a folder's name is renamed.
    fn field_submitted(&mut self, key: &str, cx: &mut Context<Self>) {
        if key != "folder" {
            return;
        }
        let Some(Selection::Folder { kind, name }) = self.selection.clone() else { return };
        let Some(to) = self.field("folder").map(|field| field.read(cx).text().trim().to_owned()) else { return };
        if to.is_empty() || to == name {
            return;
        }
        for config in self.draft.iter_mut().filter(|config| config.kind == kind) {
            if config.extras.folder.as_deref() == Some(name.as_str()) {
                config.extras.folder = Some(to.clone());
                config.detected = false;
            }
        }
        for (own, folder) in &mut self.extra_folders {
            if *own == kind && *folder == name {
                folder.clone_from(&to);
            }
        }
        self.dirty = true;
        self.select(Some(Selection::Folder { kind, name: to }), cx);
    }

    /// Escape in a field: closes the open menu, else the dialog.
    fn escape(&mut self, window: Option<&mut Window>, cx: &mut Context<Self>) {
        if self.menu.is_some() {
            self.menu = None;
            if let Some(window) = window {
                window.focus(&self.tree_focus, cx);
            }
            cx.notify();
            return;
        }
        cx.emit(EditConfigsEvent::Cancelled);
    }

    /// The text field `key`, made with `value` the first time it is drawn.
    fn text_field(
        &mut self,
        key: &str,
        value: &str,
        placeholder: Option<&str>,
        cx: &mut Context<Self>,
    ) -> Entity<TextField> {
        if let Some((field, _)) = self.fields.get(key) {
            return field.clone();
        }
        let placeholder = placeholder.map(|placeholder| tx(cx, placeholder)).unwrap_or_default();
        let field = cx.new(|cx| {
            let mut field = TextField::new(cx).with_placeholder(placeholder);
            field.set_text(value, cx);
            field
        });
        let owned = key.to_owned();
        let subscription = cx.subscribe(&field, move |view, field, event: &TextFieldEvent, cx| match event {
            TextFieldEvent::Changed => {
                let text = field.read(cx).text().to_owned();
                view.field_edited(&owned, &text, cx);
            }
            TextFieldEvent::Submitted => view.field_submitted(&owned, cx),
            TextFieldEvent::Cancelled => view.escape(None, cx),
        });
        self.fields.insert(key.to_owned(), (field.clone(), subscription));
        field
    }

    // -----------------------------------------------------------------------------------------------
    // The toolbar

    fn remove_title(&self) -> &'static str {
        match (&self.selection, self.current()) {
            (Some(Selection::Folder { .. }), _) => "Remove Folder",
            (_, Some(config)) if !config.detected && !config.id.starts_with("user:") => {
                "Remove Override (restore detected)"
            }
            _ => "Remove Configuration",
        }
    }

    /// (selector, icon, title, enabled) of each toolbar button.
    fn toolbar_buttons(&self) -> Vec<(&'static str, &'static str, &'static str, bool)> {
        let config = self.current().is_some();
        let folder = matches!(self.selection, Some(Selection::Folder { .. }));
        vec![
            ("run-edit-add", "plus", "Add New Configuration", true),
            ("run-edit-remove", "minus", self.remove_title(), config || folder),
            ("run-edit-copy", "copy", "Copy Configuration", config),
            ("run-edit-folder", "folder", "Create New Folder", config || folder),
            ("run-edit-up", "arrow-up", "Move Up", config),
            ("run-edit-down", "arrow-down", "Move Down", config),
        ]
    }

    fn press_toolbar(&mut self, selector: &str, window: &mut Window, cx: &mut Context<Self>) {
        match selector {
            "run-edit-add" => self.open_add(window, cx),
            "run-edit-remove" => self.remove(cx),
            "run-edit-copy" => self.duplicate(cx),
            "run-edit-folder" => self.new_folder(cx),
            "run-edit-up" => self.move_selected(-1, cx),
            "run-edit-down" => self.move_selected(1, cx),
            _ => {}
        }
    }

    /// Removes the selected configuration (a detected one is suppressed on save), or the selected folder
    /// (its configurations move up a level).
    pub fn remove(&mut self, cx: &mut Context<Self>) {
        match self.selection.clone() {
            Some(Selection::Folder { kind, name }) => {
                for config in self.draft.iter_mut().filter(|config| config.kind == kind) {
                    if config.extras.folder.as_deref() == Some(name.as_str()) {
                        config.extras.folder = None;
                        config.detected = false;
                    }
                }
                self.extra_folders.retain(|(own, folder)| !(*own == kind && *folder == name));
                self.dirty = true;
                self.select(None, cx);
            }
            Some(Selection::Config(id)) => {
                let Some(index) = self.draft.iter().position(|config| config.id == id) else { return };
                if self.draft[index].detected && !self.temporary.contains(&id) {
                    self.removed_detected_ids.push(id.clone());
                }
                self.draft = remove_config(&self.draft, &id);
                let neighbour = self.draft.get(index.min(self.draft.len().saturating_sub(1)));
                let next = neighbour.map(|config| Selection::Config(config.id.clone()));
                self.dirty = true;
                self.select(next, cx);
            }
            None => {}
        }
    }

    fn duplicate(&mut self, cx: &mut Context<Self>) {
        let Some(config) = self.current() else { return };
        let copy = duplicate_config(config, &self.draft);
        let id = copy.id.clone();
        self.draft.push(copy);
        self.dirty = true;
        self.select(Some(Selection::Config(id)), cx);
    }

    /// A new folder in the selected configuration's type, which moves into it.
    fn new_folder(&mut self, cx: &mut Context<Self>) {
        let kind = match (&self.selection, self.current()) {
            (_, Some(config)) => config.kind.clone(),
            (Some(Selection::Folder { kind, .. }), None) => kind.clone(),
            _ => return,
        };
        let name = unique_folder(&kind, &self.draft, &self.extra_folders);
        self.extra_folders.push((kind.clone(), name.clone()));
        if let Some(mut config) = self.current().cloned() {
            config.extras.folder = Some(name.clone());
            self.replace(config, cx);
        }
        self.select(Some(Selection::Folder { kind, name }), cx);
    }

    fn move_selected(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(id) = self.current().map(|config| config.id.clone()) else { return };
        self.draft = move_config(&self.draft, &id, delta);
        self.dirty = true;
        cx.notify();
    }

    /// The keys of the tree while it has the keyboard: the arrows move, Delete removes, ⌘N / Ctrl+N
    /// and Alt+Insert add, Escape cancels.
    fn tree_key(&mut self, keystroke: &Keystroke, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let modifiers = keystroke.modifiers;
        let plain = !modifiers.alt && !modifiers.control && !modifiers.platform && !modifiers.shift;
        let primary = if is_mac_host() { modifiers.platform || modifiers.control } else { modifiers.control };
        match keystroke.key.as_str() {
            "delete" if plain => {
                if self.selection.is_none() {
                    return false;
                }
                self.remove(cx);
            }
            "n" if primary && !modifiers.alt => self.open_add(window, cx),
            "insert" if modifiers.alt => self.open_add(window, cx),
            "up" | "down" if plain => {
                let rows = self.rows();
                let selectable: Vec<usize> =
                    (0..rows.len()).filter(|&index| !matches!(rows[index], TreeRow::Group { .. })).collect();
                let at = self.selected_row(&rows).and_then(|row| selectable.iter().position(|&index| index == row));
                let next = match (at, keystroke.key.as_str()) {
                    (None, _) => selectable.first(),
                    (Some(at), "up") => selectable.get(at.saturating_sub(1)),
                    (Some(at), _) => selectable.get((at + 1).min(selectable.len().saturating_sub(1))),
                };
                if let Some(&index) = next {
                    self.click_row(index, cx);
                }
            }
            "escape" => self.escape(Some(window), cx),
            _ => return false,
        }
        true
    }

    // -----------------------------------------------------------------------------------------------
    // Add New Configuration

    fn type_rows(&self, cx: &App) -> Vec<TypeRow> {
        type_rows(&self.types, self.add_field.read(cx).text())
    }

    fn first_type_row(&self, cx: &App) -> Option<usize> {
        self.type_rows(cx).iter().position(|row| matches!(row, TypeRow::Type { .. }))
    }

    /// Opens "Add New Configuration" with the keyboard in its search.
    pub fn open_add(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.add_field.update(cx, |field, cx| field.set_text("", cx));
        self.menu = Some(Menu::AddType);
        self.add_active = self.first_type_row(cx);
        window.focus(&self.add_field.focus_handle(cx), cx);
        cx.notify();
    }

    fn close_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.menu = None;
        window.focus(&self.tree_focus, cx);
        cx.notify();
    }

    /// The keys of "Add New Configuration"'s search: the arrows move, Enter adds.
    fn add_key(&mut self, keystroke: &Keystroke, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let rows = self.type_rows(cx);
        let types: Vec<usize> = (0..rows.len()).filter(|&index| matches!(rows[index], TypeRow::Type { .. })).collect();
        let at = self.add_active.and_then(|active| types.iter().position(|&index| index == active));
        match keystroke.key.as_str() {
            "enter" => {
                if let Some(TypeRow::Type { id, .. }) = self.add_active.and_then(|index| rows.get(index)) {
                    let id = id.clone();
                    self.add(&id, window, cx);
                }
            }
            "up" | "down" => {
                let Some(last) = types.len().checked_sub(1) else { return true };
                let next = match (at, keystroke.key.as_str()) {
                    (None, _) => 0,
                    (Some(0), "up") => last,
                    (Some(at), "up") => at - 1,
                    (Some(at), _) if at == last => 0,
                    (Some(at), _) => at + 1,
                };
                self.add_active = Some(types[next]);
                cx.notify();
            }
            _ => return false,
        }
        true
    }

    /// Adds a configuration of the type `kind`, with its template, in the selected folder of that type.
    pub fn add(&mut self, kind: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ty) = self.types.get(kind) else { return };
        let folder = match &self.selection {
            Some(Selection::Folder { kind: own, name }) if own == kind => Some(name.clone()),
            _ => None,
        };
        let saved = self.templates.iter().find(|template| template.kind == kind).cloned();
        let template = saved.unwrap_or_else(|| blank_template(ty, &self.env));
        let config = new_config_of_type(ty, &self.draft, Some(&template), folder.as_deref(), &self.env);
        let id = config.id.clone();
        self.draft.push(config);
        self.dirty = true;
        self.close_menu(window, cx);
        self.select(Some(Selection::Config(id)), cx);
    }

    // -----------------------------------------------------------------------------------------------
    // The form

    fn is_shown(&self, config: &RunConfig, option: RunOption) -> bool {
        self.shown.contains(&option) || option.is_set(config, self.show_on_start)
    }

    /// The options "Modify options" offers for `config`, by group.
    fn options_of(&self, config: &RunConfig) -> Vec<(&'static str, Vec<RunOption>)> {
        let ty = self.types.get(&config.kind);
        let compound = config.kind == COMPOUND;
        let no_process = compound || ty.is_some_and(|ty| ty.debug_only);
        let terminal = terminal_applies(config, ty);
        OPTION_GROUPS
            .iter()
            .filter(|(_, _, process)| !no_process || !process)
            .map(|(label, options, _)| {
                let offered = options.iter().copied().filter(|option| {
                    (!compound || *option == RunOption::BeforeLaunch)
                        && (*option != RunOption::EmulateTerminal || terminal || config.extras.emulate_terminal)
                        && (*option != RunOption::BuildPolicy || build_policy_applies(config))
                });
                (*label, offered.collect())
            })
            .collect()
    }

    fn form_items(&self) -> Vec<FormItem> {
        match (&self.selection, self.current()) {
            (_, Some(config)) => self.config_items(config),
            (Some(Selection::Folder { name, .. }), None) => vec![FormItem::Text {
                key: "folder".to_owned(),
                label: "Folder name".to_owned(),
                value: name.clone(),
                placeholder: None,
                error: None,
                hint: Some("Press Enter to rename".to_owned()),
                picker: None,
            }],
            _ => Vec::new(),
        }
    }

    fn config_items(&self, config: &RunConfig) -> Vec<FormItem> {
        let ty = self.types.get(&config.kind);
        let compound = config.kind == COMPOUND;
        let no_process = compound || ty.is_some_and(|ty| ty.debug_only);
        let all_errors = config_errors(&self.draft, &self.types, &self.env);
        let errors = all_errors.get(&config.id).cloned().unwrap_or_default();
        let error = |key: &str| errors.get(key).cloned();
        let text =
            |key: &str, label: &str, value: String, placeholder: Option<&str>, picker: Option<Picker>| FormItem::Text {
                key: key.to_owned(),
                label: label.to_owned(),
                value,
                placeholder: placeholder.map(str::to_owned),
                error: error(key),
                hint: None,
                picker,
            };
        let mut items = Vec::new();
        if config.detected {
            items.push(FormItem::Notice(
                "Detected from the project. Changes are saved as a user configuration that overrides it.".to_owned(),
            ));
        }
        items.push(text("name", "Name", config.name.clone(), None, None));
        let shared = !config.extras.local;
        items.push(FormItem::Check { key: "local".to_owned(), label: "Store as project file".to_owned(), on: shared });
        items.push(FormItem::Section(kind_label(&config.kind, &self.types)));
        let params = if compound { None } else { form_params(config, ty, &self.env) };
        if compound {
            let members = config
                .extras
                .compound
                .iter()
                .map(|id| {
                    let member = self.draft.iter().find(|other| other.id == *id);
                    let name = member.map_or(id.clone(), |other| other.name.clone());
                    (id.clone(), name)
                })
                .collect();
            let can_add = !member_rows(config, &self.draft).is_empty();
            items.push(FormItem::Members { members, error: error("compound"), can_add });
        } else if let (Some(ty), Some(params)) = (ty, params.as_ref()) {
            for field in ty.fields.iter().filter(|field| field.is_visible(params)) {
                let key = format!("param:{}", field.key);
                let value = params.get(&field.key).cloned().unwrap_or_default();
                let field_error = errors.get(&field.key).cloned();
                items.push(match field.control {
                    FieldControl::Text | FieldControl::Textarea => FormItem::Text {
                        key,
                        label: field.label.clone(),
                        value,
                        placeholder: field.placeholder.clone(),
                        error: field_error,
                        hint: field.hint.clone(),
                        picker: field.browse.as_ref().map(|browse| Picker::Path(browse.kind(params))),
                    },
                    FieldControl::Checkbox => FormItem::Check { key, label: field.label.clone(), on: value == "true" },
                    FieldControl::Select => {
                        let mut options = field.options.clone();
                        if !value.is_empty() && !options.iter().any(|option| option.value == value) {
                            options.push(FieldOption::new(value.clone(), value.clone()));
                        }
                        let value = if value.is_empty() {
                            options.first().map(|option| option.value.clone()).unwrap_or_default()
                        } else {
                            value
                        };
                        FormItem::Select { key, label: field.label.clone(), value, options, error: field_error }
                    }
                    FieldControl::Output => FormItem::Output {
                        key,
                        label: field.label.clone(),
                        text: field.value.as_ref().map(|value| value(params)).unwrap_or_default(),
                    },
                });
            }
        } else {
            if ty.is_some() {
                items.push(FormItem::Notice(
                    "This command line doesn't fit the type's form; it is edited as a command.".to_owned(),
                ));
            }
            items.push(text("command", "Command", config.command.clone(), Some("e.g. ./gradlew or cargo"), None));
            let placeholder = Some("Space separated; quote with ' or \"");
            items.push(text("args", "Arguments", format_args(&config.args), placeholder, None));
        }
        if !no_process {
            if !ty.is_some_and(|ty| ty.owns_working_directory) {
                let cwd = cwd_of(config).unwrap_or_default().to_owned();
                let picker = Some(Picker::Path(BrowseKind::Directory));
                items.push(text("cwd", "Working directory", cwd, Some("Project root"), picker));
            }
            items.push(FormItem::Text {
                key: "env".to_owned(),
                label: "Environment variables".to_owned(),
                value: format_env(&config.env),
                placeholder: None,
                error: self.env_error.clone().or_else(|| error("env")),
                hint: Some("Separate variables with semicolon: VAR=value; VAR1=value1".to_owned()),
                picker: Some(Picker::EnvFiles),
            });
            if !config.extras.include_system_env {
                items.push(FormItem::Notice("System environment variables are not passed".to_owned()));
            }
            if self.is_shown(config, RunOption::EnvFiles) {
                let files = config.extras.env_files.join(";");
                items.push(text("envFiles", "Paths to \".env\" files", files, Some(".env"), None));
            }
            if self.is_shown(config, RunOption::StdinFile) {
                let file = config.extras.stdin_file.clone().unwrap_or_default();
                let placeholder = Some("File relative to the project root");
                let picker = Some(Picker::Path(BrowseKind::File));
                items.push(text("stdinFile", "Redirect input from", file, placeholder, picker));
            }
            if self.is_shown(config, RunOption::OutputFile) {
                let file = config.extras.output_file.clone().unwrap_or_default();
                let placeholder = Some("File relative to the project root");
                items.push(text("outputFile", "Save console output to file", file, placeholder, None));
            }
            let mut adapters: Vec<FieldOption> = vec![FieldOption::new("", "None")];
            adapters.extend(ADAPTERS.iter().map(|(id, label)| FieldOption::new(*id, *label)));
            if let Some(own) = config.debug_adapter.as_deref().filter(|own| !ADAPTERS.iter().any(|(id, _)| id == own)) {
                adapters.push(FieldOption::new(own, format!("{own} (no launch template)")));
            }
            items.push(FormItem::Select {
                key: "debugAdapter".to_owned(),
                label: "Debug adapter".to_owned(),
                value: config.debug_adapter.clone().unwrap_or_default(),
                options: adapters,
                error: None,
            });
        }
        if self.is_shown(config, RunOption::BuildPolicy) && build_policy_applies(config) {
            let options = build_policies(&config.kind)
                .into_iter()
                .map(|(policy, label)| FieldOption::new(policy.key(), label))
                .collect();
            let value = config.extras.build_policy.unwrap_or_default().key().to_owned();
            let key = "buildPolicy".to_owned();
            items.push(FormItem::Select { key, label: "Build".to_owned(), value, options, error: None });
        }
        if self.is_shown(config, RunOption::BeforeLaunch) {
            let env = self.launch_env();
            let line = |task: &BeforeLaunchTask| (task_label(task, env), task.kind == "command");
            let tasks = config.extras.before_launch.iter().map(line).collect();
            items.push(FormItem::Tasks { tasks, error: error("beforeLaunch") });
        }
        let chips: Vec<RunOption> = CHIPS
            .into_iter()
            .filter(|option| {
                let applies = !no_process || matches!(option, RunOption::OpenToolWindow | RunOption::FocusToolWindow);
                applies && option.is_set(config, self.show_on_start)
            })
            .collect();
        if !chips.is_empty() {
            items.push(FormItem::Chips(chips));
        }
        if !no_process {
            items.push(FormItem::Preview(command_line(&config.command, &config.args)));
        }
        items
    }

    fn launch_env(&self) -> LaunchEnv<'_> {
        LaunchEnv {
            configs: &self.draft,
            project_root: self.project_root.as_deref(),
            modules: &[],
            external_tools: &self.external_tools,
            tool_windows: &self.tool_windows,
        }
    }

    /// A check box of the form was set to `on`.
    fn toggle_check(&mut self, key: &str, on: bool, cx: &mut Context<Self>) {
        match key {
            // "Store as project file": off keeps the configuration in the workspace.
            "local" => self.edit_current(|config| config.extras.local = !on, cx),
            param => {
                if let Some(param) = param.strip_prefix("param:") {
                    self.set_param(param, if on { "true" } else { "" }, cx);
                }
            }
        }
    }

    /// The select field `key` was set to `value`.
    fn choose_value(&mut self, key: &str, value: &str, cx: &mut Context<Self>) {
        match key {
            "debugAdapter" => {
                let adapter = Some(value.to_owned()).filter(|adapter| !adapter.is_empty());
                self.edit_current(|config| config.debug_adapter = adapter, cx);
            }
            "buildPolicy" => {
                let policy = BUILD_POLICIES.iter().map(|(policy, _)| *policy).find(|policy| policy.key() == value);
                let policy = policy.filter(|policy| *policy != BuildPolicy::Automatic);
                self.edit_current(|config| config.extras.build_policy = policy, cx);
            }
            param => {
                if let Some(param) = param.strip_prefix("param:") {
                    self.set_param(param, value, cx);
                }
            }
        }
    }

    /// Adds or removes an option of "Modify options".
    fn toggle_option(&mut self, option: RunOption, cx: &mut Context<Self>) {
        let Some(config) = self.current().cloned() else { return };
        let on = !self.is_shown(&config, option);
        if option.has_value() {
            if on {
                self.shown.insert(option);
                cx.notify();
            } else {
                self.shown.remove(&option);
                self.edit_current(|config| option.apply(config, false), cx);
            }
            return;
        }
        self.edit_current(|config| option.apply(config, on), cx);
    }

    fn move_task(&mut self, index: usize, delta: isize, cx: &mut Context<Self>) {
        self.fields.retain(|key, _| !key.starts_with("task:"));
        self.edit_current(
            |config| {
                let tasks = &mut config.extras.before_launch;
                if let Some(other) = index.checked_add_signed(delta).filter(|other| *other < tasks.len()) {
                    tasks.swap(index, other);
                }
            },
            cx,
        );
    }

    fn remove_task(&mut self, index: usize, cx: &mut Context<Self>) {
        self.fields.retain(|key, _| !key.starts_with("task:"));
        self.edit_current(
            |config| {
                if index < config.extras.before_launch.len() {
                    config.extras.before_launch.remove(index);
                }
            },
            cx,
        );
    }

    fn remove_member(&mut self, id: &str, cx: &mut Context<Self>) {
        self.edit_current(|config| config.extras.compound.retain(|member| member != id), cx);
    }

    fn copy_output(&mut self, key: &str, cx: &mut Context<Self>) {
        let text = self.form_items().into_iter().find_map(|item| match item {
            FormItem::Output { key: own, text, .. } if own == key => Some(text),
            _ => None,
        });
        if let Some(text) = text {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    /// The Browse button of the field `key`.
    fn browse(&mut self, key: &str, picker: Picker, cx: &mut Context<Self>) {
        let root = self.project_root.clone();
        let event = match picker {
            Picker::EnvFiles => EditConfigsEvent::Browse {
                target: "envFiles".to_owned(),
                kind: BrowseKind::File,
                multiple: true,
                from: root,
            },
            Picker::Path(kind) => {
                // A type's path field opens at its own absolute value.
                let current = self.fields.get(key).map(|(field, _)| field.read(cx).text().to_owned());
                let absolute = current.filter(|value| key.starts_with("param:") && is_absolute(value));
                EditConfigsEvent::Browse { target: key.to_owned(), kind, multiple: false, from: absolute.or(root) }
            }
        };
        cx.emit(event);
    }

    /// The file dialog of [`EditConfigsEvent::Browse`] picked `paths` for `target` (none when cancelled).
    /// Paths inside the project are kept relative to it, except in a type's own fields.
    pub fn browse_picked(&mut self, target: &str, paths: &[String], cx: &mut Context<Self>) {
        let root = self.project_root.clone();
        let relative = |path: &str| project_relative(root.as_deref(), path);
        match target {
            "envFiles" => {
                let Some(config) = self.current().cloned() else { return };
                let mut files = config.extras.env_files.clone();
                for file in paths.iter().map(|path| relative(path)) {
                    if !files.contains(&file) {
                        files.push(file);
                    }
                }
                if files != config.extras.env_files {
                    let text = files.join(";");
                    self.shown.insert(RunOption::EnvFiles);
                    self.edit_current(|config| config.extras.env_files = files, cx);
                    self.sync_field("envFiles", &text, cx);
                }
            }
            key => {
                let Some(path) = paths.first() else { return };
                if let Some(param) = key.strip_prefix("param:") {
                    self.set_param(param, path, cx);
                    self.sync_field(key, path, cx);
                    return;
                }
                let value = relative(path);
                self.sync_field(key, &value, cx);
                self.field_edited(key, &value, cx);
            }
        }
    }

    // -----------------------------------------------------------------------------------------------
    // Menus

    fn open_menu(&mut self, menu: Menu, cx: &mut Context<Self>) {
        self.menu = if self.menu.as_ref() == Some(&menu) { None } else { Some(menu) };
        cx.notify();
    }

    fn menu_rows(&self) -> Vec<MenuRow> {
        let Some(config) = self.current() else { return Vec::new() };
        match &self.menu {
            None | Some(Menu::AddType) => Vec::new(),
            Some(Menu::Select(key)) => {
                let options = self.form_items().into_iter().find_map(|item| match item {
                    FormItem::Select { key: own, value, options, .. } if own == *key => Some((value, options)),
                    _ => None,
                });
                let Some((value, options)) = options else { return Vec::new() };
                options
                    .into_iter()
                    .map(|option| {
                        let choice = MenuChoice::Value { key: key.clone(), value: option.value.clone() };
                        MenuRow::item(option.label, option.value == value, Some(choice))
                    })
                    .collect()
            }
            Some(Menu::Options) => {
                let blocked = |option: RunOption| match option {
                    RunOption::EmulateTerminal => self.is_shown(config, RunOption::StdinFile),
                    RunOption::StdinFile => config.extras.emulate_terminal,
                    _ => false,
                };
                let mut rows = Vec::new();
                for (label, options) in self.options_of(config) {
                    rows.push(MenuRow::header(label));
                    for option in options {
                        let checked = self.is_shown(config, option);
                        let choice = (checked || !blocked(option)).then_some(MenuChoice::Option(option));
                        rows.push(MenuRow::item(option.label(), checked, choice));
                    }
                }
                rows
            }
            Some(Menu::AddTask) => self.task_rows(config),
            Some(Menu::AddMember) => member_rows(config, &self.draft)
                .into_iter()
                .map(|(id, name)| MenuRow::item(name, false, Some(MenuChoice::Member(id))))
                .collect(),
        }
    }

    /// "Add task": the kinds of before launch tasks, with the configurations, external tools and tool
    /// windows they can name.
    fn task_rows(&self, config: &RunConfig) -> Vec<MenuRow> {
        let task = |kind: &str, target: &str| {
            Some(MenuChoice::Task(BeforeLaunchTask { kind: kind.to_owned(), target: target.to_owned() }))
        };
        let mut rows = vec![MenuRow::header("Run Another Configuration")];
        let runnable = self.draft.iter().filter(|other| other.id != config.id && other.kind != COMPOUND);
        rows.extend(runnable.map(|other| MenuRow::item(other.name.clone(), false, task("run", &other.id))));
        rows.push(MenuRow::header("Build"));
        rows.push(MenuRow::item("Build Project", false, task("build", "")));
        let module = config.extras.module.as_deref();
        rows.push(MenuRow::item("Build Module", false, module.and_then(|module| task("buildModule", module))));
        rows.push(MenuRow::item("Build Artifacts", false, task("buildArtifacts", "")));
        if config.kind == "dotnet" {
            rows.push(MenuRow::item("Build .NET project", false, task("dotnetBuild", "")));
        }
        if !self.external_tools.is_empty() {
            rows.push(MenuRow::header("Run External Tool"));
            let tools = self.external_tools.iter();
            rows.extend(tools.map(|(id, name)| MenuRow::item(name.clone(), false, task("tool", id))));
        }
        rows.push(MenuRow::header("Other"));
        rows.push(MenuRow::item("Run Shell Command", false, task("command", "")));
        if !self.tool_windows.is_empty() {
            rows.push(MenuRow::header("Activate Tool Window"));
            let mut windows = self.tool_windows.clone();
            windows.sort_by(|a, b| collate::compare(&a.1, &b.1));
            rows.extend(windows.into_iter().map(|(id, title)| MenuRow::item(title, false, task("toolWindow", &id))));
        }
        rows
    }

    /// Chooses the row at `index` of the open menu.
    pub fn choose_menu_item(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(choice) = self.menu_rows().into_iter().nth(index).and_then(|row| row.choice) else { return };
        let keep_open = matches!(choice, MenuChoice::Member(_));
        if !keep_open {
            self.menu = None;
        }
        match choice {
            MenuChoice::Value { key, value } => self.choose_value(&key, &value, cx),
            MenuChoice::Option(option) => self.toggle_option(option, cx),
            MenuChoice::Task(task) => self.edit_current(|config| config.extras.before_launch.push(task), cx),
            // The picker stays open so that several can be added.
            MenuChoice::Member(id) => self.edit_current(|config| config.extras.compound.push(id), cx),
        }
        cx.notify();
    }

    // -----------------------------------------------------------------------------------------------
    // Saving

    /// OK, Apply, Run or Debug. With errors, the first configuration that has them is selected instead.
    pub fn save(&mut self, then: AfterSave, cx: &mut Context<Self>) {
        let errors = config_errors(&self.draft, &self.types, &self.env);
        if let Some(bad) = self.draft.iter().find(|config| errors.contains_key(&config.id)) {
            let id = bad.id.clone();
            self.select(Some(Selection::Config(id)), cx);
            return;
        }
        if self.env_error.is_some() || self.saving {
            return;
        }
        let changes = self.dirty.then(|| SaveChanges {
            draft: self.draft.clone(),
            removed_detected_ids: self.removed_detected_ids.clone(),
        });
        self.saving = changes.is_some();
        self.save_error = None;
        cx.emit(EditConfigsEvent::Save { changes, then });
        cx.notify();
    }

    /// The configurations of [`EditConfigsEvent::Save`] were saved, or why they were not.
    pub fn saved(&mut self, result: Result<(), String>, cx: &mut Context<Self>) {
        self.saving = false;
        match result {
            Ok(()) => {
                self.dirty = false;
                self.removed_detected_ids.clear();
            }
            Err(error) => self.save_error = Some(error),
        }
        cx.notify();
    }

    fn ok(&mut self, cx: &mut Context<Self>) {
        let select = self.current().map(|config| config.id.clone());
        self.save(AfterSave::Close { select }, cx);
    }

    fn launch(&mut self, debug: bool, cx: &mut Context<Self>) {
        let Some(id) = self.current().map(|config| config.id.clone()) else { return };
        self.save(AfterSave::Launch { id, debug }, cx);
    }

    pub fn press_footer(&mut self, selector: &str, cx: &mut Context<Self>) {
        match selector {
            "run-edit-run" => self.launch(false, cx),
            "run-edit-debug" => self.launch(true, cx),
            "run-edit-ok" => self.ok(cx),
            "run-edit-cancel" => cx.emit(EditConfigsEvent::Cancelled),
            "run-edit-apply" => self.save(AfterSave::Stay, cx),
            _ => {}
        }
    }

    // -----------------------------------------------------------------------------------------------
    // Drawing

    fn icon_button(
        &self,
        selector: String,
        icon: &'static str,
        enabled: bool,
        cx: &mut Context<Self>,
        on_click: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> Stateful<Div> {
        let color = theme::color(cx, if enabled { "fg-icon" } else { "fg-faint" });
        let hover = theme::color(cx, "bg-hover");
        let id = selector.clone();
        div()
            .id(SharedString::from(id))
            .debug_selector(move || selector)
            .size(px(22.0))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(theme::length(cx, "radius-small"))
            .child(theme::icon(cx, icon, 14.0, color))
            .when(enabled, |el| {
                el.hover(move |el| el.bg(hover)).on_click(cx.listener(move |view, _, window, cx| {
                    cx.stop_propagation();
                    on_click(view, window, cx);
                }))
            })
    }

    fn text_button(
        &self,
        selector: &'static str,
        label: &str,
        primary: bool,
        enabled: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let (bg, fg, border) = if primary && enabled {
            ("bg-button-primary", "fg-button-primary", "bg-button-primary")
        } else {
            ("bg-button", if enabled { "fg" } else { "fg-faint" }, "border-button")
        };
        div()
            .id(selector)
            .debug_selector(move || selector.to_owned())
            .flex_none()
            .h(theme::length(cx, "h-button"))
            .px(px(12.0))
            .flex()
            .items_center()
            .rounded(theme::length(cx, "radius-small"))
            .border_1()
            .border_color(theme::color(cx, border))
            .bg(theme::color(cx, bg))
            .text_color(theme::color(cx, fg))
            .whitespace_nowrap()
            .child(SharedString::from(tx(cx, label)))
            .when(enabled, |el| el.on_click(cx.listener(move |view, _, _, cx| view.press_footer(selector, cx))))
    }

    fn render_sidebar(&self, cx: &mut Context<Self>) -> Div {
        let buttons: Vec<Stateful<Div>> = self
            .toolbar_buttons()
            .into_iter()
            .map(|(selector, icon, _, enabled)| {
                self.icon_button(selector.to_owned(), icon, enabled, cx, move |view, window, cx| {
                    view.press_toolbar(selector, window, cx);
                })
            })
            .collect();
        let rows = self.rows();
        let selected = self.selected_row(&rows);
        let errors = config_errors(&self.draft, &self.types, &self.env);
        let (fg, muted, faint) = (theme::color(cx, "fg"), theme::color(cx, "fg-subtle"), theme::color(cx, "fg-faint"));
        let selected_bg = theme::color(cx, "bg-tree-selected");
        let items: Vec<AnyElement> = rows
            .iter()
            .enumerate()
            .map(|(index, row)| {
                let line = div()
                    .id(("run-edit-tree-row", index))
                    .debug_selector(move || format!("run-edit-tree-{index}"))
                    .h(px(ROW))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .pr(px(8.0))
                    .rounded(theme::length(cx, "radius-row"))
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .text_color(fg)
                    .when(selected == Some(index), |el| el.bg(selected_bg))
                    .on_click(cx.listener(move |view, _, window, cx| {
                        window.focus(&view.tree_focus, cx);
                        view.click_row(index, cx);
                    }));
                let chevron = |key: String| {
                    let name = if self.collapsed.contains(&key) { "chevron-right" } else { "chevron-down" };
                    theme::icon(cx, name, 12.0, muted)
                };
                match row {
                    TreeRow::Group { label, icon, .. } => line
                        .pl(px(6.0))
                        .child(chevron(row.key()))
                        .child(theme::icon(cx, icon, 13.0, muted))
                        .child(SharedString::from(tx(cx, label))),
                    TreeRow::Folder { name, .. } => {
                        let key = row.key();
                        line.pl(px(22.0))
                            .child(
                                div()
                                    .id(SharedString::from(format!("run-edit-toggle-{key}")))
                                    .child(chevron(key.clone()))
                                    .on_click(cx.listener(move |view, _, _, cx| {
                                        cx.stop_propagation();
                                        view.toggle_collapsed(&key, cx);
                                    })),
                            )
                            .child(theme::icon(cx, "folder", 13.0, muted))
                            .child(SharedString::from(name.clone()))
                    }
                    TreeRow::Config { id, depth } => {
                        let Some(config) = self.draft.iter().find(|config| config.id == *id) else {
                            return line.into_any_element();
                        };
                        let icon = config_icon(&config.kind, config.is_test, &self.types);
                        let temporary = self.temporary.contains(&config.id);
                        let name = if config.name.is_empty() { tx(cx, "Unnamed") } else { config.name.clone() };
                        let mark = (!config.detected).then_some(if config.extras.local { "local" } else { "user" });
                        line.pl(px(if *depth == 2 { 50.0 } else { 34.0 }))
                            .child(theme::icon(cx, &icon, 13.0, if temporary { faint } else { muted }))
                            .child(SharedString::from(name))
                            .when(errors.contains_key(&config.id), |el| {
                                el.child(theme::icon(cx, "error", 12.0, theme::color(cx, "danger")))
                            })
                            .children(mark.map(|mark| {
                                let small = theme::length(cx, "fs-small");
                                div().text_color(faint).text_size(small).child(SharedString::from(mark))
                            }))
                    }
                }
                .into_any_element()
            })
            .collect();
        let shortcut = if is_mac_host() { "⌘N" } else { "Alt+Insert" };
        let empty = self.draft.is_empty().then(|| {
            div()
                .p(px(12.0))
                .flex()
                .flex_col()
                .gap(px(4.0))
                .text_color(muted)
                .child(SharedString::from(tx(cx, "No run configurations added.")))
                .child(
                    div()
                        .text_color(theme::color(cx, "accent"))
                        .child(SharedString::from(tx(cx, "Add new…")))
                        .child(div().text_color(faint).child(SharedString::from(shortcut))),
                )
        });
        div()
            .w(px(SIDEBAR_WIDTH))
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .border_r_1()
            .border_color(theme::color(cx, "border"))
            .child(div().flex_none().flex().items_center().gap(px(2.0)).px(px(6.0)).py(px(4.0)).children(buttons))
            .child(
                div()
                    .id("run-edit-tree")
                    .debug_selector(|| "run-edit-tree".to_owned())
                    .track_focus(&self.tree_focus)
                    .flex_1()
                    .min_h_0()
                    .px(px(4.0))
                    .flex()
                    .flex_col()
                    .overflow_y_scroll()
                    .children(items)
                    .children(empty),
            )
    }

    /// A labelled line of the form.
    fn field_row(&self, label: &str, control: impl IntoElement, below: Option<(String, bool)>, cx: &App) -> Div {
        div()
            .flex()
            .items_start()
            .gap(px(12.0))
            .child(
                div()
                    .w(px(LABEL_WIDTH))
                    .flex_none()
                    .h(theme::length(cx, "h-field"))
                    .flex()
                    .items_center()
                    .text_color(theme::color(cx, "fg"))
                    .child(SharedString::from(tx_template(cx, label))),
            )
            .child(div().flex_1().min_w_0().flex().flex_col().gap(px(2.0)).child(control).children(below.map(
                |(text, error)| {
                    div()
                        .text_size(theme::length(cx, "fs-small"))
                        .text_color(theme::color(cx, if error { "danger" } else { "fg-faint" }))
                        .child(SharedString::from(tx_template(cx, &text)))
                },
            )))
    }

    fn render_item(&mut self, item: FormItem, cx: &mut Context<Self>) -> AnyElement {
        let faint = theme::color(cx, "fg-faint");
        match item {
            FormItem::Notice(text) => div()
                .px(px(8.0))
                .py(px(6.0))
                .rounded(theme::length(cx, "radius-small"))
                .bg(theme::color(cx, "bg-feedback-info"))
                .text_color(theme::color(cx, "fg"))
                .child(SharedString::from(tx(cx, &text)))
                .into_any_element(),
            FormItem::Text { key, label, value, placeholder, error, hint, picker } => {
                let field = self.text_field(&key, &value, placeholder.as_deref(), cx);
                let frame = div()
                    .flex_1()
                    .min_w_0()
                    .h(theme::length(cx, "h-field"))
                    .px(px(6.0))
                    .flex()
                    .items_center()
                    .rounded(theme::length(cx, "radius-control"))
                    .border_1()
                    .border_color(theme::color(cx, if error.is_some() { "danger" } else { "border" }))
                    .bg(theme::color(cx, "bg-field"))
                    .child(div().flex_1().min_w_0().child(field));
                let browse = picker.map(|picker| {
                    let target = key.clone();
                    let (icon, selector) = match picker {
                        Picker::EnvFiles => ("folder", "run-edit-browse-envFiles".to_owned()),
                        Picker::Path(_) => ("folder", format!("run-edit-browse-{key}")),
                    };
                    self.icon_button(selector, icon, true, cx, move |view, _, cx| view.browse(&target, picker, cx))
                });
                let control = div().flex().items_center().gap(px(4.0)).child(frame).children(browse);
                let below = error.map(|error| (error, true)).or(hint.map(|hint| (hint, false)));
                self.field_row(&label, control, below, cx).into_any_element()
            }
            FormItem::Check { key, label, on } => {
                let check = crate::settings_view::controls::check_box(cx, on);
                let selector = format!("run-edit-check-{key}");
                let control = div()
                    .id(SharedString::from(selector.clone()))
                    .debug_selector(move || selector)
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(check)
                    .child(SharedString::from(tx(cx, &label)))
                    .on_click(cx.listener(move |view, _, _, cx| view.toggle_check(&key, !on, cx)));
                div().pl(px(LABEL_WIDTH + 12.0)).child(control).into_any_element()
            }
            FormItem::Select { key, label, value, options, error } => {
                let chosen = options.iter().find(|option| option.value == value);
                let shown = chosen.map_or(value.clone(), |option| option.label.clone());
                let selector = format!("run-edit-select-{key}");
                let menu = Menu::Select(key.clone());
                let control = div()
                    .id(SharedString::from(selector.clone()))
                    .debug_selector(move || selector)
                    .h(theme::length(cx, "h-field"))
                    .px(px(8.0))
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .rounded(theme::length(cx, "radius-control"))
                    .border_1()
                    .border_color(theme::color(cx, if error.is_some() { "danger" } else { "border" }))
                    .bg(theme::color(cx, "bg-control"))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .child(SharedString::from(tx(cx, &shown))),
                    )
                    .child(theme::icon(cx, "chevron-down", 12.0, faint))
                    .on_click(cx.listener(move |view, _, _, cx| view.open_menu(menu.clone(), cx)));
                self.field_row(&label, control, error.map(|error| (error, true)), cx).into_any_element()
            }
            FormItem::Output { key, label, text } => {
                let selector = format!("run-edit-copy-{key}");
                let copy = self.icon_button(selector, "copy", true, cx, move |view, _, cx| view.copy_output(&key, cx));
                let control = div()
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .px(px(6.0))
                            .py(px(4.0))
                            .rounded(theme::length(cx, "radius-control"))
                            .border_1()
                            .border_color(theme::color(cx, "border"))
                            .text_color(theme::color(cx, "fg-muted"))
                            .child(SharedString::from(text)),
                    )
                    .child(copy);
                self.field_row(&label, control, None, cx).into_any_element()
            }
            FormItem::Section(title) => div()
                .mt(px(6.0))
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(div().text_color(theme::color(cx, "fg-strong")).child(SharedString::from(tx(cx, &title))))
                .child(div().flex_1())
                .child(
                    div()
                        .id("run-edit-modify")
                        .debug_selector(|| "run-edit-modify".to_owned())
                        .flex()
                        .items_center()
                        .gap(px(4.0))
                        .text_color(theme::color(cx, "accent"))
                        .child(SharedString::from(tx(cx, "Modify options")))
                        .child(theme::icon(cx, "chevron-down", 12.0, theme::color(cx, "accent")))
                        .on_click(cx.listener(|view, _, _, cx| view.open_menu(Menu::Options, cx))),
                )
                .child(div().text_color(faint).child(SharedString::from("⌥M")))
                .into_any_element(),
            FormItem::Members { members, error, can_add } => {
                let mut list = div().flex().flex_col().gap(px(2.0));
                for (index, (id, name)) in members.into_iter().enumerate() {
                    let selector = format!("run-edit-member-remove-{index}");
                    let remove = self.icon_button(selector, "minus", true, cx, move |view, _, cx| {
                        view.remove_member(&id, cx);
                    });
                    let name = div().flex_1().child(SharedString::from(name));
                    list = list.child(div().flex().items_center().gap(px(6.0)).child(name).child(remove));
                }
                let add = self.icon_button("run-edit-add-member".to_owned(), "plus", can_add, cx, |view, _, cx| {
                    view.open_menu(Menu::AddMember, cx);
                });
                list = list.child(add);
                self.field_row("Configurations", list, error.map(|error| (error, true)), cx).into_any_element()
            }
            FormItem::Tasks { tasks, error } => {
                let count = tasks.len();
                let targets: Vec<String> = self
                    .current()
                    .map(|config| config.extras.before_launch.iter().map(|task| task.target.clone()).collect())
                    .unwrap_or_default();
                let mut list = div().flex().flex_col().gap(px(2.0));
                for (index, (label, command)) in tasks.into_iter().enumerate() {
                    let shown: AnyElement = if command {
                        let target = targets.get(index).cloned().unwrap_or_default();
                        let placeholder = Some("Shell command, e.g. make generate");
                        let field = self.text_field(&format!("task:{index}"), &target, placeholder, cx);
                        div()
                            .flex_1()
                            .min_w_0()
                            .h(theme::length(cx, "h-field"))
                            .px(px(6.0))
                            .flex()
                            .items_center()
                            .rounded(theme::length(cx, "radius-control"))
                            .border_1()
                            .border_color(theme::color(cx, "border"))
                            .child(div().flex_1().min_w_0().child(field))
                            .into_any_element()
                    } else {
                        div().flex_1().min_w_0().child(SharedString::from(tx_template(cx, &label))).into_any_element()
                    };
                    let (up, down, remove) = (
                        format!("run-edit-task-up-{index}"),
                        format!("run-edit-task-down-{index}"),
                        format!("run-edit-task-remove-{index}"),
                    );
                    let up = self.icon_button(up, "arrow-up", index > 0, cx, move |view, _, cx| {
                        view.move_task(index, -1, cx);
                    });
                    let down = self.icon_button(down, "arrow-down", index + 1 < count, cx, move |view, _, cx| {
                        view.move_task(index, 1, cx);
                    });
                    let remove = self.icon_button(remove, "minus", true, cx, move |view, _, cx| {
                        view.remove_task(index, cx);
                    });
                    let line = div().flex().items_center().gap(px(4.0));
                    list = list.child(line.child(shown).child(up).child(down).child(remove));
                }
                let add = self.icon_button("run-edit-add-task".to_owned(), "plus", true, cx, |view, _, cx| {
                    view.open_menu(Menu::AddTask, cx);
                });
                list = list.child(add);
                self.field_row("Before launch", list, error.map(|error| (error, true)), cx).into_any_element()
            }
            FormItem::Chips(chips) => {
                let mut row = div().pl(px(LABEL_WIDTH + 12.0)).flex().flex_wrap().gap(px(6.0));
                for option in chips {
                    let selector = format!("run-edit-chip-{option:?}");
                    let remove = self.icon_button(selector, "close", true, cx, move |view, _, cx| {
                        view.edit_current(|config| option.apply(config, false), cx);
                    });
                    row = row.child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.0))
                            .pl(px(8.0))
                            .rounded(theme::length(cx, "radius-small"))
                            .bg(theme::color(cx, "bg-control"))
                            .child(SharedString::from(tx(cx, option.label())))
                            .child(remove),
                    );
                }
                row.into_any_element()
            }
            FormItem::Preview(line) => div()
                .mt(px(6.0))
                .flex()
                .flex_col()
                .gap(px(2.0))
                .child(
                    div()
                        .text_color(faint)
                        .text_size(theme::length(cx, "fs-small"))
                        .child(SharedString::from(tx(cx, "Command line"))),
                )
                .child(
                    div()
                        .text_size(theme::length(cx, "fs-code"))
                        .text_color(theme::color(cx, "fg"))
                        .child(SharedString::from(if line.is_empty() { "—".to_owned() } else { line })),
                )
                .into_any_element(),
        }
    }

    fn render_form(&mut self, cx: &mut Context<Self>) -> Div {
        let items = self.form_items();
        let body: Vec<AnyElement> = items.into_iter().map(|item| self.render_item(item, cx)).collect();
        let empty = self.selection.is_none().then(|| {
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(4.0))
                .pt(px(40.0))
                .text_color(theme::color(cx, "fg-subtle"))
                .child(SharedString::from(tx(cx, "Add new run configuration…")))
                .child(SharedString::from(tx(cx, "or select run configuration to edit")))
        });
        div().flex_1().min_w_0().h_full().child(
            div()
                .id("run-edit-form")
                .size_full()
                .p(px(12.0))
                .flex()
                .flex_col()
                .gap(px(8.0))
                .overflow_y_scroll()
                .children(body)
                .children(empty),
        )
    }

    fn render_footer(&self, cx: &mut Context<Self>) -> Div {
        let can_run = self.can_run();
        let invalid = self.is_invalid();
        let error = if invalid { Some(tx(cx, "Fix the highlighted fields")) } else { self.save_error.clone() };
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(8.0))
            .px(px(12.0))
            .py(px(8.0))
            .border_t_1()
            .border_color(theme::color(cx, "border"))
            .child(self.text_button("run-edit-run", "Run", false, can_run, cx))
            .when(self.debug_available, |el| el.child(self.text_button("run-edit-debug", "Debug", false, can_run, cx)))
            .children(error.map(|error| {
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .text_color(theme::color(cx, "danger"))
                    .child(theme::icon(cx, "error", 13.0, theme::color(cx, "danger")))
                    .child(SharedString::from(error))
            }))
            .child(div().flex_1())
            .child(self.text_button("run-edit-ok", "OK", true, self.can_ok(), cx))
            .child(self.text_button("run-edit-cancel", "Cancel", false, true, cx))
            .child(self.text_button("run-edit-apply", "Apply", false, self.can_apply(), cx))
    }

    fn render_menu(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let menu = self.menu.as_ref()?;
        let (fg, faint) = (theme::color(cx, "fg-strong"), theme::color(cx, "fg-faint"));
        let subtle = theme::color(cx, "fg-subtle");
        let (selected_bg, hover_bg) = (theme::color(cx, "bg-tree-selected"), theme::color(cx, "bg-hover"));
        let content: Vec<AnyElement> = if *menu == Menu::AddType {
            let rows = self.type_rows(cx);
            let mut items: Vec<AnyElement> = vec![
                div()
                    .px(px(8.0))
                    .py(px(4.0))
                    .text_color(theme::color(cx, "fg-secondary"))
                    .child(SharedString::from(tx(cx, "Add New Configuration")))
                    .into_any_element(),
                div()
                    .debug_selector(|| "run-edit-type-search".to_owned())
                    .mx(px(4.0))
                    .px(px(6.0))
                    .h(theme::length(cx, "h-field"))
                    .flex()
                    .items_center()
                    .border_b_1()
                    .border_color(theme::color(cx, "border-popup"))
                    .child(div().flex_1().child(self.add_field.clone()))
                    .into_any_element(),
            ];
            if rows.is_empty() {
                let nothing = SharedString::from(tx(cx, "Nothing found"));
                items.push(div().px(px(8.0)).text_color(faint).child(nothing).into_any_element());
            }
            for (index, row) in rows.into_iter().enumerate() {
                let line = div()
                    .id(("run-edit-type", index))
                    .debug_selector(move || format!("run-edit-type-{index}"))
                    .h(px(ROW))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .px(px(8.0))
                    .rounded(px(4.0))
                    .whitespace_nowrap();
                items.push(match row {
                    TypeRow::Group { label } => {
                        line.text_color(subtle).child(SharedString::from(tx(cx, &label))).into_any_element()
                    }
                    TypeRow::Type { id, label, icon, nested, .. } => line
                        .when(nested, |el| el.pl(px(20.0)))
                        .text_color(fg)
                        .when(self.add_active == Some(index), |el| el.bg(selected_bg))
                        .hover(move |el| el.bg(hover_bg))
                        .child(theme::icon(cx, &icon, 13.0, subtle))
                        .child(SharedString::from(label))
                        .on_click(cx.listener(move |view, _, window, cx| view.add(&id, window, cx)))
                        .into_any_element(),
                });
            }
            items
        } else {
            self.menu_rows()
                .into_iter()
                .enumerate()
                .map(|(index, row)| {
                    let mark = if row.checked {
                        theme::icon(cx, "check", 12.0, theme::color(cx, "fg-muted"))
                    } else {
                        div().flex_none().size(px(12.0)).into_any_element()
                    };
                    let line = div()
                        .id(("run-edit-menu", index))
                        .debug_selector(move || format!("run-edit-menu-{index}"))
                        .flex_none()
                        .h(theme::length(cx, "h-row"))
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .px(px(8.0))
                        .rounded(px(4.0))
                        .whitespace_nowrap()
                        .overflow_hidden();
                    if row.header {
                        let label = SharedString::from(tx(cx, &row.label));
                        let small = theme::length(cx, "fs-small");
                        return line.text_color(subtle).text_size(small).child(label).into_any_element();
                    }
                    let line = line.child(mark).child(SharedString::from(tx_template(cx, &row.label)));
                    if row.choice.is_none() {
                        return line.text_color(faint).into_any_element();
                    }
                    line.text_color(fg)
                        .hover(move |el| el.bg(hover_bg))
                        .on_click(cx.listener(move |view, _, _, cx| view.choose_menu_item(index, cx)))
                        .into_any_element()
                })
                .collect()
        };
        let list = div()
            .id("run-edit-menu")
            .flex_none()
            .w(px(MENU_WIDTH))
            .max_h(px(HEIGHT - 120.0))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .p(px(4.0))
            .rounded(theme::length(cx, "radius-popup"))
            .border_1()
            .border_color(theme::color(cx, "border-popup"))
            .bg(theme::color(cx, "bg-popup"))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .children(content);
        // The type list opens over the tree, the other menus over the form.
        let left = *menu == Menu::AddType;
        let backdrop = div()
            .id("run-edit-menu-backdrop")
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .pt(px(72.0))
            .px(px(16.0))
            .when(!left, |el| el.justify_end())
            .on_mouse_down(MouseButton::Left, cx.listener(|view, _, window, cx| view.close_menu(window, cx)))
            .child(list);
        Some(deferred(backdrop).with_priority(1).into_any_element())
    }
}

/// An absolute path on this machine (`/…` or `C:\…`).
fn is_absolute(path: &str) -> bool {
    let bytes = path.as_bytes();
    let drive = bytes.len() > 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
    path.starts_with('/') || (drive && matches!(bytes[2], b'\\' | b'/'))
}

impl Focusable for EditConfigs {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.tree_focus.clone()
    }
}

impl Render for EditConfigs {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let sidebar = self.render_sidebar(cx);
        let form = self.render_form(cx);
        let footer = self.render_footer(cx);
        let menu = self.render_menu(cx);
        div()
            .debug_selector(|| "run-edit-configs".to_owned())
            .relative()
            .w(px(WIDTH))
            .h(px(HEIGHT))
            .flex()
            .flex_col()
            .bg(theme::color(cx, "bg-dialog"))
            .border_1()
            .border_color(theme::color(cx, "border"))
            .rounded(theme::length(cx, "radius-dialog"))
            .overflow_hidden()
            .text_size(theme::length(cx, "fs-ui"))
            .text_color(theme::color(cx, "fg"))
            .child(
                div()
                    .flex_none()
                    .px(px(12.0))
                    .py(px(8.0))
                    .border_b_1()
                    .border_color(theme::color(cx, "border"))
                    .text_color(theme::color(cx, "fg-strong"))
                    .child(SharedString::from(tx(cx, "Run/Debug Configurations"))),
            )
            .child(div().flex_1().min_h_0().flex().child(sidebar).child(form))
            .child(footer)
            .children(menu)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::config_types::RunConfigTypeField;
    use crate::run::configs::blank_config;
    use crate::run::types::register_builtin_types;
    use gpui_kit::{Modifiers, TestAppContext, VisualTestContext};
    use std::cell::RefCell;

    type Events = Rc<RefCell<Vec<EditConfigsEvent>>>;

    fn types_with(extra: Vec<RunConfigType>) -> Rc<RunConfigTypes> {
        let mut types = RunConfigTypes::new();
        register_builtin_types(&mut types);
        for ty in extra {
            types.register(ty);
        }
        Rc::new(types)
    }

    fn types() -> Rc<RunConfigTypes> {
        types_with(Vec::new())
    }

    fn cfg(id: &str, kind: &str) -> RunConfig {
        RunConfig { command: "x".into(), ..blank_config(id, id, kind) }
    }

    /// The detected configuration of the removal cases.
    fn detected(kind: &str) -> RunConfig {
        RunConfig { command: "run".into(), detected: true, ..blank_config(format!("{kind}:app"), "Detected app", kind) }
    }

    fn store_of(configs: Vec<RunConfig>, selected: Option<&str>) -> RunStore {
        let mut store = RunStore::new();
        store.set_configs(configs);
        store.loaded = true;
        store.selected_id = selected.map(str::to_owned);
        store
    }

    /// The dialog on `store` in a window of its own, with the keyboard in its tree, and what it asked of
    /// the window so far.
    fn open(
        cx: &mut TestAppContext,
        store: RunStore,
        types: Rc<RunConfigTypes>,
    ) -> (Entity<EditConfigs>, Entity<RunStore>, Events, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let store = cx.update(|cx| cx.new(|_| store));
        let shared = store.clone();
        let (view, cx) = cx.add_window_view(|window, cx| EditConfigs::new(&shared, types, Vec::new(), window, cx));
        let events: Events = Rc::default();
        let seen = Rc::clone(&events);
        cx.update(|window, cx| {
            cx.subscribe(&view, move |_, event: &EditConfigsEvent, _| seen.borrow_mut().push(event.clone())).detach();
            let focus = view.read(cx).tree_focus().clone();
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        (view, store, events, cx)
    }

    fn click(cx: &mut VisualTestContext, selector: &str) {
        cx.run_until_parked();
        let selector: &'static str = selector.to_owned().leak();
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.run_until_parked();
    }

    fn lines(view: &Entity<EditConfigs>, cx: &mut VisualTestContext) -> Vec<String> {
        view.read_with(cx, |view, cx| view.form_lines(cx))
    }

    fn tree(view: &Entity<EditConfigs>, cx: &mut VisualTestContext) -> Vec<String> {
        view.read_with(cx, |view, _| view.tree_lines())
    }

    /// Replaces the text of the form field `key` by typing `text` into it.
    fn type_into(view: &Entity<EditConfigs>, key: &str, text: &str, cx: &mut VisualTestContext) {
        cx.run_until_parked();
        let field = view.read_with(cx, |view, _| view.field(key)).unwrap_or_else(|| panic!("{key} is drawn"));
        cx.update(|window, cx| {
            let focus = field.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.simulate_keystrokes("cmd-a backspace");
        if !text.is_empty() {
            cx.simulate_input(text);
        }
    }

    /// Clicks the row `label` of the open menu.
    fn choose(view: &Entity<EditConfigs>, label: &str, cx: &mut VisualTestContext) {
        let rows = view.read_with(cx, |view, _| view.menu_lines());
        let index = rows
            .iter()
            .position(|row| row == label || row.strip_prefix("[x] ") == Some(label))
            .unwrap_or_else(|| panic!("{label} is not in {rows:?}"));
        click(cx, &format!("run-edit-menu-{index}"));
    }

    fn saved(events: &Events) -> Option<(Option<SaveChanges>, AfterSave)> {
        events.borrow().iter().rev().find_map(|event| match event {
            EditConfigsEvent::Save { changes, then } => Some((changes.clone(), then.clone())),
            _ => None,
        })
    }

    fn changes(draft: Vec<RunConfig>, removed: &[&str]) -> Option<SaveChanges> {
        Some(SaveChanges { draft, removed_detected_ids: removed.iter().map(|id| id.to_string()).collect() })
    }

    // src/features/run/runConfigTypes.test.tsx › Run/Debug Configurations tree › groups by type with folders first
    #[test]
    fn groups_by_type_with_folders_first() {
        let in_folder = |id: &str, kind: &str, folder: Option<&str>| {
            let mut config = cfg(id, kind);
            config.extras.folder = folder.map(str::to_owned);
            config
        };
        let web = Some("Web");
        let configs = [in_folder("a", "npm", web), cfg("b", "npm"), in_folder("c", "npm", web), cfg("d", "shell")];
        let types = types();
        let keys = |rows: Vec<TreeRow>| rows.iter().map(TreeRow::key).collect::<Vec<_>>();
        let extra = [("shell".to_owned(), "Empty".to_owned())];
        assert_eq!(
            keys(tree_rows(&configs, &extra, &HashSet::new(), &types)),
            ["g:npm", "f:npm:Web", "c:a", "c:c", "c:b", "g:shell", "f:shell:Empty", "c:d"]
        );
        let collapsed = HashSet::from(["f:npm:Web".to_owned()]);
        assert_eq!(keys(tree_rows(&configs, &[], &collapsed, &types)), ["g:npm", "f:npm:Web", "c:b", "g:shell", "c:d"]);
    }

    // src/features/run/runConfigTypes.test.tsx › Run/Debug Configurations tree › moves within the same type and
    // folder
    #[test]
    fn moves_within_the_same_type_and_folder() {
        let mut configs = vec![cfg("a", "npm"), cfg("b", "npm"), cfg("c", "npm"), cfg("d", "shell")];
        for index in [0, 2] {
            configs[index].extras.folder = Some("Web".to_owned());
        }
        let ids = |configs: Vec<RunConfig>| configs.into_iter().map(|config| config.id).collect::<Vec<_>>();
        assert_eq!(ids(move_config(&configs, "c", -1)), ["c", "b", "a", "d"]);
        assert_eq!(ids(move_config(&configs, "b", -1)), ["a", "b", "c", "d"]);
    }

    // src/features/run/runConfigTypes.test.tsx › configuration types › Add New Configuration lists, groups and
    // filters types
    #[test]
    fn add_new_configuration_lists_groups_and_filters_types() {
        let docker = RunConfigType::new("docker-image", "Docker Image", "docker", |_, _| None).group("Docker");
        let types = types_with(vec![docker]);
        let rows = type_rows(&types, "");
        assert!(rows.contains(&TypeRow::Group { label: "Docker".to_owned() }));
        assert!(rows.iter().any(|row| matches!(row, TypeRow::Type { label, .. } if label == "Compound")));
        let keys = |query: &str| type_rows(&types, query).iter().map(TypeRow::key).collect::<Vec<_>>();
        assert_eq!(keys("grad"), ["gradle"]);
        assert_eq!(keys("dock"), ["group:Docker", "docker-image"]);
    }

    // src/features/run/removal.test.tsx › enables minus for detections and stays removed after Apply and backend
    // reload
    #[gpui_kit::test]
    async fn enables_minus_for_detections_and_stays_removed_after_apply_and_backend_reload(cx: &mut TestAppContext) {
        let (view, store, events, cx) = open(cx, store_of(vec![detected("npm")], Some("npm:app")), types());
        let toolbar = view.read_with(cx, |view, _| view.toolbar());
        assert_eq!(toolbar[1], ("Remove Configuration".to_owned(), true));
        click(cx, "run-edit-remove");
        assert!(events.borrow().is_empty());
        click(cx, "run-edit-apply");
        let (saving, then) = saved(&events).expect("Apply saves");
        assert_eq!((saving.clone(), then), (changes(Vec::new(), &["npm:app"]), AfterSave::Stay));
        // The workbench saves what the store makes of the working copy, and the backend lists none.
        let user = store.update(cx, |store, _| store.save_edited_configs(&saving.expect("changes").draft).1);
        assert!(user.is_empty());
        view.update(cx, |view, cx| view.saved(Ok(()), cx));
        assert!(!view.read_with(cx, |view, _| view.is_dirty()));
        store.update(cx, |store, _| store.set_configs(Vec::new()));
        let reopened = cx.update(|window, cx| cx.new(|cx| EditConfigs::new(&store, types(), Vec::new(), window, cx)));
        assert!(reopened.read_with(cx, |view, _| view.tree_lines()).is_empty());
    }

    // src/features/run/removal.test.tsx › handles Delete only inside the tree
    #[gpui_kit::test]
    async fn handles_delete_only_inside_the_tree(cx: &mut TestAppContext) {
        let (view, _, events, cx) = open(cx, store_of(vec![detected("npm")], Some("npm:app")), types());
        let name = view.read_with(cx, |view, _| view.field("name")).expect("the name field is drawn");
        cx.update(|window, cx| {
            let focus = name.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.simulate_keystrokes("delete");
        assert!(tree(&view, cx).contains(&"Detected app".to_owned()));
        cx.update(|window, cx| {
            let focus = view.read(cx).tree_focus().clone();
            window.focus(&focus, cx);
        });
        cx.simulate_keystrokes("delete");
        assert!(!tree(&view, cx).contains(&"Detected app".to_owned()));
        click(cx, "run-edit-ok");
        assert_eq!(saved(&events), Some((changes(Vec::new(), &["npm:app"]), AfterSave::Close { select: None })));
    }

    // src/features/run/removal.test.tsx › keeps the override label and removes overrides without suppression
    #[gpui_kit::test]
    async fn keeps_the_override_label_and_removes_overrides_without_suppression(cx: &mut TestAppContext) {
        let user = RunConfig { detected: false, ..detected("npm") };
        let (view, _, events, cx) = open(cx, store_of(vec![user], Some("npm:app")), types());
        let toolbar = view.read_with(cx, |view, _| view.toolbar());
        assert_eq!(toolbar[1].0, "Remove Override (restore detected)");
        click(cx, "run-edit-remove");
        click(cx, "run-edit-ok");
        assert_eq!(saved(&events), Some((changes(Vec::new(), &[]), AfterSave::Close { select: None })));
    }

    // src/features/run/removal.test.tsx › Cancel discards a pending detected removal
    #[gpui_kit::test]
    async fn cancel_discards_a_pending_detected_removal(cx: &mut TestAppContext) {
        let (_, store, events, cx) = open(cx, store_of(vec![detected("npm")], Some("npm:app")), types());
        click(cx, "run-edit-remove");
        click(cx, "run-edit-cancel");
        assert_eq!(*events.borrow(), [EditConfigsEvent::Cancelled]);
        assert_eq!(store.read_with(cx, |store, _| store.configs().to_vec()), [detected("npm")]);
    }

    // src/features/run/removal.test.tsx › removes a temporary configuration from Edit Configurations for good
    #[gpui_kit::test]
    async fn removes_a_temporary_configuration_from_edit_configurations_for_good(cx: &mut TestAppContext) {
        let mut store = store_of(vec![detected("npm")], Some("npm:app"));
        let main_go =
            RunConfig { id: "go:file:/test-removal/app/main.go".into(), name: "main.go".into(), ..detected("go") };
        store.adopt_temporary(main_go.clone(), &[]);
        store.edit_target = Some(main_go.id.clone());
        let (view, store, events, cx) = open(cx, store, types());
        let row = tree(&view, cx).iter().position(|line| line == "main.go").expect("main.go is listed");
        click(cx, &format!("run-edit-tree-{row}"));
        assert_eq!(view.read_with(cx, |view, _| view.selected()).as_deref(), Some("main.go"));
        click(cx, "run-edit-remove");
        click(cx, "run-edit-ok");
        // Not a suppressed detection: it was never detected by the backend.
        let (saving, _) = saved(&events).expect("OK saves");
        let saving = saving.expect("changes");
        assert!(saving.removed_detected_ids.is_empty());
        let user = store.update(cx, |store, _| store.save_edited_configs(&saving.draft).1);
        assert!(user.is_empty());
        assert!(store.read_with(cx, |store, _| store.temporary.is_empty()));
    }

    // src/features/run/removal.test.tsx › keeps unedited temporary configurations temporary and saves edited ones
    #[gpui_kit::test]
    async fn keeps_unedited_temporary_configurations_temporary_and_saves_edited_ones(cx: &mut TestAppContext) {
        let mut store = store_of(vec![detected("npm")], Some("npm:app"));
        let maven =
            |id: &str, name: &str| RunConfig { id: id.into(), name: name.into(), detected: false, ..detected("maven") };
        let goal = maven("maven:goal:pom.xml:test", "app [test]");
        let other = maven("maven:goal:pom.xml:verify", "app [verify]");
        store.adopt_temporary(goal.clone(), &[]);
        store.adopt_temporary(other.clone(), &[]);
        store.edit_target = Some(other.id.clone());
        let (view, store, events, cx) = open(cx, store, types());
        type_into(&view, "name", "verify app", cx);
        click(cx, "run-edit-ok");
        let (saving, _) = saved(&events).expect("OK saves");
        let user = store.update(cx, |store, _| store.save_edited_configs(&saving.expect("changes").draft).1);
        assert_eq!(user, [RunConfig { name: "verify app".into(), ..other }]);
        assert_eq!(store.read_with(cx, |store, _| store.temporary.clone()), [goal.id]);
    }

    // src/features/run/runConfigTypes.test.tsx › Run/Debug Configurations tree › adds a configuration of the
    // picked type and edits it
    #[gpui_kit::test]
    async fn adds_a_configuration_of_the_picked_type_and_edits_it(cx: &mut TestAppContext) {
        let (view, _, _, cx) = open(cx, store_of(Vec::new(), None), types());
        assert!(tree(&view, cx).is_empty());
        click(cx, "run-edit-add");
        assert!(view.read_with(cx, |view, cx| view.type_lines(cx)).contains(&"Program".to_owned()));
        cx.simulate_input("prog");
        cx.simulate_keystrokes("enter");
        assert!(lines(&view, cx).contains(&"Name: Unnamed".to_owned()));
        type_into(&view, "param:program", "./server", cx);
        type_into(&view, "param:arguments", "--port 1", cx);
        assert!(lines(&view, cx).contains(&"Command line: ./server --port 1".to_owned()));
        // Modify options adds "Redirect input".
        click(cx, "run-edit-modify");
        choose(&view, "Redirect input", cx);
        assert!(lines(&view, cx).contains(&"Redirect input from: ".to_owned()));
    }

    // src/features/run/runConfigTypes.test.tsx › Run/Debug Configurations tree › opens Add New Configuration on
    // Ctrl+N by typed letter
    #[gpui_kit::test]
    async fn opens_add_new_configuration_on_ctrl_n_by_typed_letter(cx: &mut TestAppContext) {
        let (view, _, _, cx) = open(cx, store_of(Vec::new(), None), types());
        cx.simulate_keystrokes("ctrl-b");
        assert!(view.read_with(cx, |view, cx| view.type_lines(cx)).is_empty());
        cx.simulate_keystrokes("ctrl-n");
        assert!(!view.read_with(cx, |view, cx| view.type_lines(cx)).is_empty());
        // Escape in its search closes it.
        cx.simulate_keystrokes("escape");
        assert!(view.read_with(cx, |view, cx| view.type_lines(cx)).is_empty());
    }

    fn remote_type() -> RunConfigType {
        let number = |value: &str, _: &crate::run::config_types::Params| {
            (!value.chars().all(|c| c.is_ascii_digit())).then_some("Port must be a number".to_owned())
        };
        let port = RunConfigTypeField::new("port", "Port").required().validate(number);
        let agent = RunConfigTypeField::new("agent", "Agent")
            .output(|params| format!("address=*:{}", params.get("port").map_or("", String::as_str)));
        RunConfigType::new("test-remote", "Test Remote", "debug", |_, _| None)
            .defaults(&[("port", "5005")])
            .debug_only()
            .debug_adapter("java-debug")
            .fields(vec![port, agent])
    }

    // src/features/run/runConfigTypes.test.tsx › debug-only types (Remote JVM Debug) › edit only their fields;
    // output fields follow the form
    #[gpui_kit::test]
    async fn edit_only_their_fields_output_fields_follow_the_form(cx: &mut TestAppContext) {
        let (view, _, _, cx) = open(cx, store_of(Vec::new(), None), types_with(vec![remote_type()]));
        click(cx, "run-edit-add");
        cx.simulate_input("remote");
        cx.simulate_keystrokes("enter");
        assert!(lines(&view, cx).contains(&"Agent: address=*:5005".to_owned()));
        type_into(&view, "param:port", "6006", cx);
        let shown = lines(&view, cx);
        assert!(shown.contains(&"Agent: address=*:6006".to_owned()));
        // No process: no working directory, environment, debug adapter or command line.
        let starts = |prefix: &str| shown.iter().any(|line| line.starts_with(prefix));
        assert!(!starts("Working directory") && !starts("Environment") && !starts("Debug adapter"));
        assert!(!starts("Command line"));
        type_into(&view, "param:port", "x", cx);
        assert!(lines(&view, cx).contains(&"! Port must be a number".to_owned()));
        // Emptying the only field keeps the form: there is no command line to edit instead.
        type_into(&view, "param:port", "", cx);
        let shown = lines(&view, cx);
        assert!(shown.contains(&"! Port is required".to_owned()));
        assert!(!shown.iter().any(|line| line.starts_with("Command:")));
    }

    /// A dialog on `config` alone, drawn in no window: for the menus driven through the view's methods.
    fn dialog_of(config: RunConfig, cx: &mut VisualTestContext) -> Entity<EditConfigs> {
        let store = cx.update(|_, cx| cx.new(|_| store_of(vec![config.clone()], Some(&config.id))));
        cx.update(|window, cx| cx.new(|cx| EditConfigs::new(&store, types(), Vec::new(), window, cx)))
    }

    // src/features/run/runConfigTypes.test.tsx › before launch tasks › offers Build Module only with a selected
    // module (%s)
    #[gpui_kit::test]
    async fn offers_build_module_only_with_a_selected_module(cx: &mut TestAppContext) {
        let (_, _, _, cx) = open(cx, store_of(Vec::new(), None), types());
        for module in [None, Some(""), Some("core")] {
            let mut config = cfg("app", "shell");
            config.extras.module = module.map(str::to_owned);
            let artifacts = BeforeLaunchTask { kind: "buildArtifacts".into(), target: String::new() };
            config.extras.before_launch = vec![artifacts];
            let view = dialog_of(config.clone(), cx);
            view.update(cx, |view, cx| view.open_menu(Menu::AddTask, cx));
            let rows = view.read_with(cx, |view, _| view.menu_lines());
            let Some(module) = module else {
                assert!(rows.contains(&"Build Module (disabled)".to_owned()));
                continue;
            };
            let index = rows.iter().position(|row| row == "Build Module").expect("Build Module is offered");
            view.update(cx, |view, cx| view.choose_menu_item(index, cx));
            let tasks = view.read_with(cx, |view, _| view.draft()[0].extras.before_launch.clone());
            let added = BeforeLaunchTask { kind: "buildModule".into(), target: module.to_owned() };
            assert_eq!(tasks, [config.extras.before_launch[0].clone(), added]);
        }
    }

    // src/features/run/runConfigTypes.test.tsx › configuration types › sets the before launch build policy of a
    // Java configuration
    #[gpui_kit::test]
    async fn sets_the_before_launch_build_policy_of_a_java_configuration(cx: &mut TestAppContext) {
        let types = types();
        let ty = types.get("application").expect("Application is registered");
        let env = TypeEnv::default();
        let fresh = new_config_of_type(ty, &[], None, None, &env);
        let mut params = fresh.extras.params.clone();
        params.insert("mainClass".into(), "a.Main".into());
        let config = with_params(&fresh, params, Some(ty), &env);
        let id = config.id.clone();
        let (view, _, _, cx) = open(cx, store_of(vec![config], Some(&id)), Rc::clone(&types));
        click(cx, "run-edit-modify");
        choose(&view, "Build", cx);
        assert!(lines(&view, cx).contains(&"Build: Automatic (incremental)".to_owned()));
        click(cx, "run-edit-select-buildPolicy");
        choose(&view, "Rebuild", cx);
        let policy = view.read_with(cx, |view, _| view.draft()[0].extras.build_policy);
        assert_eq!(policy, Some(BuildPolicy::Rebuild));
    }

    // src/features/run/runConfigTypes.test.tsx › offers only supported build policies for %s
    #[gpui_kit::test]
    async fn offers_only_supported_build_policies(cx: &mut TestAppContext) {
        let (_, _, _, cx) = open(cx, store_of(Vec::new(), None), types());
        for kind in ["maven", "gradle"] {
            let mut config = cfg("main", kind);
            config.extras.build_policy = Some(BuildPolicy::Make);
            let view = dialog_of(config, cx);
            view.update(cx, |view, cx| view.open_menu(Menu::Select("buildPolicy".to_owned()), cx));
            let rows = view.read_with(cx, |view, _| view.menu_lines());
            assert!(!rows.contains(&"No build".to_owned()));
            assert_eq!(rows.contains(&"Build, no error check".to_owned()), kind == "maven");
            assert!(rows.contains(&"Rebuild".to_owned()) && rows.contains(&"[x] Build (Make)".to_owned()));
        }
    }

    // src/features/run/runConfigTypes.test.tsx › before launch tasks › shows the build policy for a Maven
    // configuration
    #[gpui_kit::test]
    async fn shows_the_build_policy_for_a_maven_configuration(cx: &mut TestAppContext) {
        let (_, _, _, cx) = open(cx, store_of(Vec::new(), None), types());
        let view = dialog_of(cfg("mvn", "maven"), cx);
        view.update(cx, |view, cx| view.open_menu(Menu::Options, cx));
        let rows = view.read_with(cx, |view, _| view.menu_lines());
        let index = rows.iter().position(|row| row == "Build").expect("Build is offered");
        view.update(cx, |view, cx| view.choose_menu_item(index, cx));
        let shown = view.read_with(cx, |view, cx| view.form_lines(cx));
        assert!(shown.contains(&"Build: Automatic (incremental)".to_owned()));
    }

    // src/features/run/runConfigTypes.test.tsx › compound configurations › member picker lists candidates and
    // filters them by name
    #[test]
    fn member_picker_lists_candidates() {
        let compound = |id: &str, members: &[&str]| {
            let mut config = RunConfig { command: String::new(), ..blank_config(id, id, COMPOUND) };
            config.extras.compound = members.iter().map(|member| member.to_string()).collect();
            config
        };
        let (a, b) = (cfg("a", "shell"), cfg("b", "shell"));
        let (inner, outer) = (compound("inner", &["b", "a"]), compound("outer", &["a", "inner"]));
        let all = [a, b, inner.clone(), outer];
        let ids = |rows: Vec<(String, String)>| rows.into_iter().map(|(id, _)| id).collect::<Vec<_>>();
        // `outer` contains `inner`, so adding it to `inner` would close a cycle.
        assert_eq!(ids(member_rows(&compound("inner", &[]), &all)), ["a", "b"]);
        // Members already added and the compound itself are left out.
        assert_eq!(ids(member_rows(&compound("inner", &["a"]), &all)), ["b"]);
        assert!(member_rows(&inner, &all).is_empty());
    }

    fn with_root(config: RunConfig) -> RunStore {
        let id = config.id.clone();
        let mut store = store_of(vec![config], Some(&id));
        store.project_root = Some("/work/app".to_owned());
        store
    }

    // src/features/run/browsePaths.test.tsx › Environment variables › adds the .env files chosen in the file
    // dialog, relative to the project root
    #[gpui_kit::test]
    async fn adds_the_env_files_chosen_in_the_file_dialog_relative_to_the_project_root(cx: &mut TestAppContext) {
        let mut config = RunConfig { command: "python3".into(), ..blank_config("app", "app", "shell") };
        config.extras.env_files = vec![".env".into()];
        let (view, _, events, cx) = open(cx, with_root(config), types());
        click(cx, "run-edit-browse-envFiles");
        let asked = EditConfigsEvent::Browse {
            target: "envFiles".into(),
            kind: BrowseKind::File,
            multiple: true,
            from: Some("/work/app".into()),
        };
        assert_eq!(*events.borrow(), [asked]);
        let picked = ["/work/app/.env", "/work/app/config/dev.env", "/elsewhere/shared.env"].map(str::to_owned);
        view.update(cx, |view, cx| view.browse_picked("envFiles", &picked, cx));
        let files = view.read_with(cx, |view, _| view.draft()[0].extras.env_files.clone());
        assert_eq!(files, [".env", "config/dev.env", "/elsewhere/shared.env"]);
    }

    // src/features/run/browsePaths.test.tsx › path fields › pick the working directory relative to the project
    // root
    #[gpui_kit::test]
    async fn pick_the_working_directory_relative_to_the_project_root(cx: &mut TestAppContext) {
        let config = RunConfig { command: "python3".into(), ..blank_config("app", "app", "shell") };
        let (view, _, events, cx) = open(cx, with_root(config), types());
        click(cx, "run-edit-browse-cwd");
        let asked = events.borrow().last().cloned();
        let Some(EditConfigsEvent::Browse { target, kind, multiple, .. }) = asked else { panic!("a browse") };
        assert_eq!((target.as_str(), kind, multiple), ("cwd", BrowseKind::Directory, false));
        view.update(cx, |view, cx| view.browse_picked("cwd", &["/work/app/server".to_owned()], cx));
        let cwd = view.read_with(cx, |view, _| cwd_of(&view.draft()[0]).map(str::to_owned));
        assert_eq!(cwd.as_deref(), Some("server"));
        assert!(lines(&view, cx).contains(&"Working directory: server".to_owned()));
    }

    // src/features/run/browsePaths.test.tsx › path fields › pick a file or a directory for a configuration type's
    // path fields
    #[gpui_kit::test]
    async fn pick_a_file_or_a_directory_for_a_configuration_types_path_fields(cx: &mut TestAppContext) {
        let fields = vec![
            RunConfigTypeField::new("tomcatHome", "Tomcat installation directory").browse(BrowseKind::Directory),
            RunConfigTypeField::new("deploymentPath", "Deployment path").browse_by(|params| {
                let war = params.get("deployment").is_some_and(|mode| mode == "war");
                if war { BrowseKind::File } else { BrowseKind::Directory }
            }),
        ];
        let server = RunConfigType::new("fake-server", "Fake Server", "server", |_, _| {
            Some(crate::run::config_types::CommandLine::new("catalina", Vec::new()))
        })
        .fields(fields);
        let mut config = RunConfig { command: "catalina".into(), ..blank_config("srv", "srv", "fake-server") };
        config.extras.params.insert("deployment".into(), "war".into());
        let (view, _, events, cx) = open(cx, with_root(config), types_with(vec![server]));
        click(cx, "run-edit-browse-param:tomcatHome");
        assert!(matches!(events.borrow().last(), Some(EditConfigsEvent::Browse { kind: BrowseKind::Directory, .. })));
        view.update(cx, |view, cx| view.browse_picked("param:tomcatHome", &["/opt/tomcat".to_owned()], cx));
        let home = view.read_with(cx, |view, _| view.draft()[0].extras.params.get("tomcatHome").cloned());
        assert_eq!(home.as_deref(), Some("/opt/tomcat"));
        click(cx, "run-edit-browse-param:deploymentPath");
        assert!(matches!(events.borrow().last(), Some(EditConfigsEvent::Browse { kind: BrowseKind::File, .. })));
    }

    // Not from a spec: a configuration with errors keeps OK and Apply off and says why; OK selects it; Apply
    // stays on until the save is reported.
    #[gpui_kit::test]
    async fn errors_block_saving_and_apply_follows_the_changes(cx: &mut TestAppContext) {
        let (view, _, events, cx) = open(cx, store_of(vec![cfg("a", "shell"), cfg("b", "shell")], Some("a")), types());
        let state = |view: &Entity<EditConfigs>, cx: &mut VisualTestContext| {
            view.read_with(cx, |view, _| (view.is_dirty(), view.can_apply(), view.can_ok()))
        };
        assert_eq!(state(&view, cx), (false, false, true));
        type_into(&view, "name", "b", cx);
        assert!(lines(&view, cx).contains(&"! Another configuration has this name".to_owned()));
        assert_eq!(state(&view, cx), (true, false, false));
        view.update(cx, |view, cx| view.press_footer("run-edit-ok", cx));
        assert!(events.borrow().is_empty());
        type_into(&view, "name", "c", cx);
        assert_eq!(state(&view, cx), (true, true, true));
        click(cx, "run-edit-apply");
        assert!(matches!(saved(&events), Some((Some(_), AfterSave::Stay))));
        view.update(cx, |view, cx| view.saved(Err("disk full".into()), cx));
        assert_eq!(view.read_with(cx, |view, _| view.save_error().map(str::to_owned)).as_deref(), Some("disk full"));
        assert_eq!(state(&view, cx), (true, true, true));
        view.update(cx, |view, cx| view.saved(Ok(()), cx));
        assert_eq!(state(&view, cx), (false, false, true));
    }

    // Not from a spec: Copy duplicates the selection, Move Down moves it after its neighbour, and a new folder
    // takes the selection into it.
    #[gpui_kit::test]
    async fn copies_moves_and_files_configurations(cx: &mut TestAppContext) {
        let (view, _, _, cx) = open(cx, store_of(vec![cfg("a", "shell"), cfg("b", "shell")], Some("a")), types());
        click(cx, "run-edit-down");
        assert_eq!(tree(&view, cx), ["[Shell Script]", "b", "a"]);
        click(cx, "run-edit-copy");
        assert_eq!(view.read_with(cx, |view, _| view.selected()).as_deref(), Some("a (copy)"));
        click(cx, "run-edit-folder");
        assert_eq!(tree(&view, cx), ["[Shell Script]", "New Folder/", "a (copy)", "b", "a"]);
        assert_eq!(view.read_with(cx, |view, _| view.selected()).as_deref(), Some("New Folder/"));
        // Removing the folder keeps its configuration.
        click(cx, "run-edit-remove");
        assert_eq!(tree(&view, cx), ["[Shell Script]", "b", "a", "a (copy)"]);
    }
}
