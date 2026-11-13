//! The pages other features contribute: each is a view of its own (Plugins, Language Servers,
//! Agents, TODO, Vim, the template pages, Log, GitHub, Project Trust), so they declare only where
//! they sit and the words the search finds them by.

use super::{EDITOR_GROUP, TOOLS_GROUP};
use crate::settings_model::language_pages::LANGUAGES_GROUP;
use crate::settings_model::page_spec::PageSpec;

/// The order of a page that names none.
const DEFAULT_ORDER: f64 = 100.0;

pub(super) fn pages() -> Vec<PageSpec> {
    vec![
        PageSpec::new("plugins", "Plugins", 250).keywords(&[
            "plugin",
            "extension",
            "wasm",
            "install",
            "theme",
            "marketplace",
        ]),
        PageSpec::new("languages.servers", "Language Servers", 400).group(LANGUAGES_GROUP).project_scope().keywords(&[
            "lsp",
            "language server",
            "debugger",
            "adapter",
            "dap",
            "install",
            "initialization options",
            "rust-analyzer",
            "gopls",
            "pyright",
        ]),
        PageSpec::new("tools.agents", "Agents", 500).group(TOOLS_GROUP).keywords(&[
            "agent",
            "acp",
            "registry",
            "install",
            "update",
            "coding agent",
        ]),
        PageSpec::new("editor.todo", "TODO", 40).group(EDITOR_GROUP).keywords(&["todo", "fixme", "hack", "pattern"]),
        PageSpec::new("editor.vim", "Vim", 45).group(EDITOR_GROUP).keywords(&[
            "vim",
            "ideavim",
            "ideavimrc",
            "emulation",
            "shortcut conflicts",
            "handler",
        ]),
        PageSpec::new("editor.liveTemplates", "Live Templates", 28).group(EDITOR_GROUP).keywords(&[
            "snippet",
            "abbreviation",
            "surround",
            "template",
        ]),
        PageSpec::new("editor.general.postfixCompletion", "Postfix Completion", 7)
            .group(EDITOR_GROUP)
            .keywords(&["postfix", "template", ".if", ".var", ".for"]),
        PageSpec::new("editor.fileTemplates", "File and Code Templates", 26).group(EDITOR_GROUP).keywords(&[
            "new file",
            "class",
            "template",
            "package",
            "includes",
            "file header",
            "code",
            "scheme",
            "reformat",
        ]),
        PageSpec::new("vcs.log", "Log", 6).keywords(&["git log", "references", "tags", "columns", "commit time"]),
        PageSpec::new("vcs.github", "GitHub", 9).keywords(&["pull request", "viewed", "review"]),
        PageSpec::new("projectTrust", "Project Trust", DEFAULT_ORDER).group("Security"),
    ]
}
