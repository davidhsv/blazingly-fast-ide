//! Settings › Tools › External Tools: a tool's program, arguments and working directory expand IDEA
//! macros (`$FilePath$`, `$ProjectFileDir$`…) into the configuration that runs in the Run tool window.

use crate::platform::command_line::split_command_line;
use crate::run::configs::{blank_config, to_cwd};
use ide_run::RunConfig;

/// What the macros of a tool read: the project and the active editor.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MacroContext {
    pub project_root: String,
    pub project_name: String,
    pub file_path: Option<String>,
    /// 1-based caret line and column.
    pub line: Option<u32>,
    pub column: Option<u32>,
    pub selected_text: String,
}

/// What a tool runs, as its settings say.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExternalTool {
    pub id: String,
    pub name: String,
    pub program: String,
    pub arguments: String,
    pub working_directory: String,
}

pattern!(MACRO, r"\$([A-Za-z]+)\$");

/// The extension of a file name (none for a dot file).
fn extension_of(name: &str) -> &str {
    match name.rfind('.') {
        Some(dot) if dot > 0 => &name[dot + 1..],
        _ => "",
    }
}

/// `path` relative to the project at `root`.
fn relative<'p>(path: &'p str, root: &str) -> &'p str {
    match path.strip_prefix(root) {
        Some("") => "",
        Some(rest) => rest.strip_prefix('/').unwrap_or(path),
        None => path,
    }
}

/// Value of one macro, or `None` for unknown macros (left as typed).
pub fn macro_value(name: &str, context: &MacroContext) -> Option<String> {
    let file = context.file_path.as_deref();
    let dir = file.map_or("", |file| match file.rsplit_once('/') {
        Some((dir, _)) if !dir.is_empty() => dir,
        _ => "/",
    });
    let base = file.map_or("", |file| file.rsplit('/').next().unwrap_or(file));
    let root = context.project_root.as_str();
    let number = |value: Option<u32>| value.map(|value| value.to_string()).unwrap_or_default();
    let value = match name {
        "FilePath" => file.unwrap_or_default().to_string(),
        "FileDir" => dir.to_string(),
        "FileName" => base.to_string(),
        "FileNameWithoutExtension" => match extension_of(base) {
            "" => base.to_string(),
            extension => base[..base.len() - extension.len() - 1].to_string(),
        },
        "FileExt" => extension_of(base).to_string(),
        "FileRelativePath" => file.map(|file| relative(file, root)).unwrap_or_default().to_string(),
        "FileDirRelativeToProjectRoot" => file.map(|_| relative(dir, root)).unwrap_or_default().to_string(),
        "ProjectFileDir" => root.to_string(),
        "ProjectName" => context.project_name.clone(),
        "LineNumber" => number(context.line),
        "ColumnNumber" => number(context.column),
        "SelectedText" => context.selected_text.clone(),
        _ => return None,
    };
    Some(value)
}

/// Expands the macros in `text`; `$Prompt$` takes `prompt`.
pub fn expand_macros(text: &str, context: &MacroContext, prompt: &str) -> String {
    let expand = |found: &regex::Captures| match &found[1] {
        "Prompt" => prompt.to_string(),
        name => macro_value(name, context).unwrap_or_else(|| found[0].to_string()),
    };
    MACRO.replace_all(text, expand).into_owned()
}

/// The run configuration a tool expands to: each argument is expanded on its own, so a path with
/// spaces stays one argument.
pub fn tool_run_config(tool: &ExternalTool, context: &MacroContext, prompt: &str) -> RunConfig {
    let args = split_command_line(&tool.arguments);
    let cwd = expand_macros(tool.working_directory.trim(), context, prompt);
    RunConfig {
        command: expand_macros(tool.program.trim(), context, prompt),
        args: args.iter().map(|arg| expand_macros(arg, context, prompt)).collect(),
        cwd: to_cwd(Some(if cwd.is_empty() { context.project_root.as_str() } else { cwd.as_str() })),
        ..blank_config(format!("external:{}", tool.id), tool.name.as_str(), "external")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::configs::cwd_of;

    fn context(file_path: &str) -> MacroContext {
        MacroContext {
            project_root: "/p".to_string(),
            project_name: "demo".to_string(),
            file_path: Some(file_path.to_string()),
            line: Some(3),
            column: Some(5),
            selected_text: "sel".to_string(),
        }
    }

    #[test]
    fn expands_idea_macros() {
        let context = context("/p/src/main.rs");
        let file = "$FileName$ $FileNameWithoutExtension$ $FileExt$ $FileRelativePath$";
        assert_eq!(expand_macros(file, &context, ""), "main.rs main rs src/main.rs");
        let position = "$ProjectFileDir$:$LineNumber$:$ColumnNumber$ $Unknown$ $Prompt$";
        assert_eq!(expand_macros(position, &context, "x"), "/p:3:5 $Unknown$ x");
    }

    #[test]
    fn builds_a_run_configuration_with_per_argument_expansion() {
        let tool = ExternalTool {
            id: "t".to_string(),
            name: "Fmt".to_string(),
            program: "rustfmt".to_string(),
            arguments: "--edition 2021 \"$FilePath$\"".to_string(),
            working_directory: "$ProjectFileDir$".to_string(),
        };
        let config = tool_run_config(&tool, &context("/p/my dir/a.rs"), "");
        assert_eq!((config.id.as_str(), config.command.as_str()), ("external:t", "rustfmt"));
        assert_eq!(cwd_of(&config), Some("/p"));
        assert_eq!(config.args, ["--edition", "2021", "/p/my dir/a.rs"]);
    }
}
