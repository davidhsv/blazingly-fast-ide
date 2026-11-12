//! Version Control, Build / Execution / Deployment and Tools settings as features consume them:
//! every number in range, every choice one of its options, every list without blanks. The shape
//! and the defaults are the backend's (`ide_core::settings`); what the types cannot express is
//! put right here, so that a hand-edited settings file cannot hand a feature a value it has no
//! answer for.

use super::remote_settings::{normalized_deployment, normalized_run_targets, normalized_ssh};
use ide_core::settings::{
    ActionsOnSaveSettings, ConsoleSettings, DebuggerSettings, DeploymentSettings, ExternalTool, RunSettings, RunTarget,
    Settings, SshSettings, TerminalSettings, VcsSettings,
};
use std::collections::HashSet;

/// An inclusive range a number setting is kept in.
pub type Limits<T> = (T, T);

pub const HISTORY_LIMIT: Limits<u32> = (10, 100_000);
/// Commit message subject and body width.
pub const MESSAGE_LIMIT: Limits<u32> = (20, 500);
pub const LARGE_FILE_MB: Limits<u32> = (1, 100_000);
pub const BUFFER_KB: Limits<u32> = (16, 1_048_576);
pub const TOOLTIP_DELAY_MS: Limits<u32> = (0, 10_000);
pub const TERMINATION_TIMEOUT_MS: Limits<u32> = (0, 600_000);
pub const TERMINAL_FONT_SIZE: Limits<f32> = (6.0, 72.0);
pub const TERMINAL_LINE_HEIGHT: Limits<f32> = (0.5, 3.0);
pub const TERMINAL_COLUMN_WIDTH: Limits<f32> = (0.5, 2.0);

fn within(value: u32, (min, max): Limits<u32>) -> u32 {
    value.clamp(min, max)
}

fn within_f32(value: f32, default: f32, (min, max): Limits<f32>) -> f32 {
    if value.is_finite() { value.clamp(min, max) } else { default }
}

fn one_of(value: &str, allowed: &[&str], default: &str) -> String {
    let chosen = if allowed.contains(&value) { value } else { default };
    chosen.to_owned()
}

fn non_blank(value: &str, default: &str) -> String {
    let chosen = if value.trim().is_empty() { default } else { value };
    chosen.to_owned()
}

fn without_blanks(values: &[String]) -> Vec<String> {
    values.iter().filter(|value| !value.trim().is_empty()).cloned().collect()
}

const CONFIRMATIONS: [&str; 3] = ["ask", "silently", "never"];

pub fn normalized_vcs(vcs: &VcsSettings) -> VcsSettings {
    let default = VcsSettings::default();
    let mut out = vcs.clone();
    out.history_limit = within(vcs.history_limit, HISTORY_LIMIT);
    out.git.executable = vcs.git.executable.trim().to_owned();
    out.git.update_method = one_of(&vcs.git.update_method, &["merge", "rebase"], &default.git.update_method);
    out.git.incoming_check = one_of(&vcs.git.incoming_check, &["auto", "always", "never"], &default.git.incoming_check);
    out.git.large_file_mb = within(vcs.git.large_file_mb, LARGE_FILE_MB);
    out.commit.subject_limit = within(vcs.commit.subject_limit, MESSAGE_LIMIT);
    out.commit.body_limit = within(vcs.commit.body_limit, MESSAGE_LIMIT);
    out.commit.disabled_plugin_checks = without_blanks(&vcs.commit.disabled_plugin_checks);
    out.confirmation.on_create = one_of(&vcs.confirmation.on_create, &CONFIRMATIONS, &default.confirmation.on_create);
    out.confirmation.on_delete = one_of(&vcs.confirmation.on_delete, &CONFIRMATIONS, &default.confirmation.on_delete);
    out.confirmation.on_patch_created =
        one_of(&vcs.confirmation.on_patch_created, &["ask", "no", "reveal"], &default.confirmation.on_patch_created);
    out.issue_navigation.retain(|link| !link.pattern.trim().is_empty());
    out
}

pub fn normalized_console(console: &ConsoleSettings) -> ConsoleSettings {
    ConsoleSettings {
        buffer_kb: within(console.buffer_kb, BUFFER_KB),
        encoding: non_blank(&console.encoding, &ConsoleSettings::default().encoding),
        fold_patterns: without_blanks(&console.fold_patterns),
        ..console.clone()
    }
}

pub fn normalized_debugger(debugger: &DebuggerSettings) -> DebuggerSettings {
    let value_tooltip_delay = within(debugger.value_tooltip_delay, TOOLTIP_DELAY_MS);
    DebuggerSettings { value_tooltip_delay, ..debugger.clone() }
}

pub fn normalized_run(run: &RunSettings) -> RunSettings {
    let termination_timeout_ms = within(run.termination_timeout_ms, TERMINATION_TIMEOUT_MS);
    RunSettings { termination_timeout_ms, ..run.clone() }
}

pub fn normalized_terminal(terminal: &TerminalSettings) -> TerminalSettings {
    let default = TerminalSettings::default();
    let named = terminal.env.iter().filter(|(name, _)| !name.trim().is_empty());
    TerminalSettings {
        start_directory: terminal.start_directory.trim().to_owned(),
        env: named.map(|(name, value)| (name.clone(), value.clone())).collect(),
        shell_path: terminal.shell_path.trim().to_owned(),
        tab_name: non_blank(&terminal.tab_name, &default.tab_name),
        cursor_shape: one_of(&terminal.cursor_shape, &["block", "underline", "bar"], &default.cursor_shape),
        font_family: non_blank(&terminal.font_family, &default.font_family),
        font_size: within_f32(terminal.font_size, default.font_size, TERMINAL_FONT_SIZE),
        line_height: within_f32(terminal.line_height, default.line_height, TERMINAL_LINE_HEIGHT),
        fallback_font_family: terminal.fallback_font_family.trim().to_owned(),
        column_width: within_f32(terminal.column_width, default.column_width, TERMINAL_COLUMN_WIDTH),
        ..terminal.clone()
    }
}

/// The external tools that have a name, each under an id of its own: a missing id becomes
/// `tool<position>`, a repeated one gets `_` appended.
pub fn normalized_external_tools(tools: &[ExternalTool]) -> Vec<ExternalTool> {
    let default = ExternalTool::default();
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for (index, tool) in tools.iter().enumerate() {
        let name = tool.name.trim();
        if name.is_empty() {
            continue;
        }
        let trimmed = tool.id.trim();
        let mut id = if trimmed.is_empty() { format!("tool{}", index + 1) } else { trimmed.to_owned() };
        while seen.contains(&id) {
            id.push('_');
        }
        seen.insert(id.clone());
        let group = non_blank(&tool.group, &default.group);
        out.push(ExternalTool { id, name: name.to_owned(), group, ..tool.clone() });
    }
    out
}

/// The workflow sections of the settings, normalised.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkflowSettings {
    pub vcs: VcsSettings,
    pub console: ConsoleSettings,
    pub debugger: DebuggerSettings,
    pub run: RunSettings,
    pub terminal: TerminalSettings,
    pub actions_on_save: ActionsOnSaveSettings,
    pub external_tools: Vec<ExternalTool>,
    pub ssh: SshSettings,
    pub deployment: DeploymentSettings,
    pub run_targets: Vec<RunTarget>,
}

/// The workflow settings of `settings` (the open project's effective ones, or the defaults).
pub fn workflow_settings(settings: &Settings) -> WorkflowSettings {
    WorkflowSettings {
        vcs: normalized_vcs(&settings.vcs),
        console: normalized_console(&settings.console),
        debugger: normalized_debugger(&settings.debugger),
        run: normalized_run(&settings.run),
        terminal: normalized_terminal(&settings.terminal),
        actions_on_save: settings.actions_on_save.clone(),
        external_tools: normalized_external_tools(&settings.external_tools),
        ssh: normalized_ssh(&settings.ssh),
        deployment: normalized_deployment(&settings.deployment),
        run_targets: normalized_run_targets(&settings.run_targets),
    }
}

/// Follows one section of the settings and tells when its content changes, whatever else in
/// the settings changed with it.
#[derive(Debug, Clone, PartialEq)]
pub struct SectionWatch<T> {
    last: T,
}

impl<T: PartialEq> SectionWatch<T> {
    /// Starts from the section as it is now; that first value is not a change.
    pub fn new(current: T) -> Self {
        Self { last: current }
    }

    /// The settings changed and the section is now `next`. Returns the value it replaces when
    /// the two differ.
    pub fn update(&mut self, next: T) -> Option<T> {
        (next != self.last).then(|| std::mem::replace(&mut self.last, next))
    }

    pub fn current(&self) -> &T {
        &self.last
    }
}

/// The macros usable in the arguments and the working directory of an external tool, with the
/// English source text of what each stands for.
pub const EXTERNAL_TOOL_MACROS: [(&str, &str); 13] = [
    ("$FilePath$", "Absolute path of the current file"),
    ("$FileDir$", "Directory of the current file"),
    ("$FileName$", "Name of the current file"),
    ("$FileNameWithoutExtension$", "File name without extension"),
    ("$FileExt$", "Extension of the current file"),
    ("$FileRelativePath$", "Path of the current file relative to the project"),
    ("$FileDirRelativeToProjectRoot$", "Directory of the current file relative to the project"),
    ("$ProjectFileDir$", "Project root directory"),
    ("$ProjectName$", "Project name"),
    ("$LineNumber$", "Caret line number (1-based)"),
    ("$ColumnNumber$", "Caret column number (1-based)"),
    ("$SelectedText$", "Text selected in the editor"),
    ("$Prompt$", "Asks for a value when the tool runs"),
];

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn settings(raw: Value) -> Settings {
        serde_json::from_value(raw).expect("the backend takes the settings")
    }

    // validateWorkflow › defaults inline blame off and accepts only boolean values
    #[test]
    fn defaults_inline_blame_off_and_accepts_only_boolean_values() {
        assert!(!workflow_settings(&Settings::default()).vcs.git.inline_blame);
        let on = settings(json!({ "vcs": { "git": { "inlineBlame": true } } }));
        assert!(workflow_settings(&on).vcs.git.inline_blame);
        // Anything but a boolean is refused where the settings are stored.
        let text = json!({ "vcs": { "git": { "inlineBlame": "true" } } });
        assert!(serde_json::from_value::<Settings>(text).is_err());
    }

    // validateWorkflow › falls back to defaults for missing or malformed values
    #[test]
    fn falls_back_to_defaults_for_missing_or_malformed_values() {
        let defaults = Settings::default();
        let unchanged = workflow_settings(&defaults);
        assert_eq!(
            (&unchanged.vcs, &unchanged.console, &unchanged.debugger, &unchanged.run, &unchanged.terminal),
            (&defaults.vcs, &defaults.console, &defaults.debugger, &defaults.run, &defaults.terminal)
        );
        assert!(unchanged.external_tools.is_empty() && unchanged.run_targets.is_empty());

        let v = workflow_settings(&settings(json!({
            "vcs": {
                "historyLimit": 3,
                "git": { "updateMethod": "squash", "incomingCheck": "never", "largeFileMb": 0 },
                "confirmation": { "onPatchCreated": "always" },
                "issueNavigation": [{ "pattern": "" }, { "pattern": "#(\\d+)", "link": "x/$1" }],
            },
            "console": { "bufferKb": u32::MAX, "foldPatterns": ["a", " ", ""] },
            "terminal": { "cursorShape": "bar", "env": { "A": "1", " ": "2" }, "tabName": "  " },
            "externalTools": [{ "name": "Lint", "id": "x" }, { "name": "" }, { "name": "Again", "id": "x" }],
        })));
        assert_eq!(v.vcs.history_limit, 10);
        assert_eq!(v.vcs.git.update_method, "merge");
        assert_eq!(v.vcs.git.incoming_check, "never");
        assert_eq!(v.vcs.git.large_file_mb, 1);
        let links: Vec<(&str, &str)> =
            v.vcs.issue_navigation.iter().map(|link| (link.pattern.as_str(), link.link.as_str())).collect();
        assert_eq!(links, [("#(\\d+)", "x/$1")]);
        assert!(v.vcs.commit.non_modal);
        assert_eq!(v.vcs.confirmation.on_patch_created, "ask");
        assert!(!v.vcs.shelf.remove_applied);
        assert_eq!(v.console.buffer_kb, 1_048_576);
        assert_eq!(v.console.fold_patterns, ["a"]);
        assert_eq!((v.terminal.cursor_shape.as_str(), v.terminal.tab_name.as_str()), ("bar", "Local"));
        assert_eq!(v.terminal.env.iter().collect::<Vec<_>>(), [(&"A".to_owned(), &"1".to_owned())]);
        let tools: Vec<(&str, &str)> = v.external_tools.iter().map(|t| (t.id.as_str(), t.name.as_str())).collect();
        assert_eq!(tools, [("x", "Lint"), ("x_", "Again")]);

        // Numbers out of a field's own range and values of another type do not reach here.
        for refused in [
            json!({ "vcs": { "historyLimit": -5 } }),
            json!({ "console": { "bufferKb": 1e12 } }),
            json!({ "console": { "foldPatterns": ["a", 3] } }),
            json!({ "terminal": { "env": { "B": 2 } } }),
        ] {
            assert!(serde_json::from_value::<Settings>(refused.clone()).is_err(), "{refused}");
        }
    }

    // Not from a spec: the remaining clamps and fallbacks, and how a section is followed.
    #[test]
    fn keeps_the_other_numbers_and_choices_in_range_and_follows_a_section() {
        let v = workflow_settings(&settings(json!({
            "vcs": {
                "git": { "executable": " /usr/bin/git " },
                "commit": { "subjectLimit": 5, "bodyLimit": 900, "disabledPluginChecks": ["p/c", " "] },
                "confirmation": { "onCreate": "silently", "onDelete": "maybe" },
            },
            "console": { "encoding": " " },
            "debugger": { "valueTooltipDelay": 99999 },
            "run": { "terminationTimeoutMs": 700000 },
            "terminal": {
                "cursorShape": "beam",
                "fontFamily": "",
                "fontSize": 200,
                "lineHeight": 0.1,
                "columnWidth": 9,
            },
            "externalTools": [{ "name": " Fmt ", "group": " " }],
        })));
        assert_eq!(v.vcs.git.executable, "/usr/bin/git");
        assert_eq!((v.vcs.commit.subject_limit, v.vcs.commit.body_limit), (20, 500));
        assert_eq!(v.vcs.commit.disabled_plugin_checks, ["p/c"]);
        let confirmation = &v.vcs.confirmation;
        assert_eq!((confirmation.on_create.as_str(), confirmation.on_delete.as_str()), ("silently", "ask"));
        assert_eq!(v.console.encoding, "UTF-8");
        assert_eq!((v.debugger.value_tooltip_delay, v.run.termination_timeout_ms), (10_000, 600_000));
        assert_eq!((v.terminal.cursor_shape.as_str(), v.terminal.font_family.as_str()), ("block", "JetBrains Mono"));
        assert_eq!((v.terminal.font_size, v.terminal.line_height, v.terminal.column_width), (72.0, 0.5, 2.0));
        assert_eq!(within_f32(f32::NAN, 13.0, TERMINAL_FONT_SIZE), 13.0);
        let tool = &v.external_tools[0];
        assert_eq!((tool.id.as_str(), tool.name.as_str(), tool.group.as_str()), ("tool1", "Fmt", "External Tools"));
        assert_eq!(EXTERNAL_TOOL_MACROS[7].0, ExternalTool::default().working_directory);

        let mut watch = SectionWatch::new(v.terminal.clone());
        assert_eq!(watch.update(v.terminal.clone()), None);
        let renamed = TerminalSettings { tab_name: "Shell".to_owned(), ..v.terminal.clone() };
        assert_eq!(watch.update(renamed.clone()), Some(v.terminal));
        assert_eq!(watch.current(), &renamed);
    }
}
