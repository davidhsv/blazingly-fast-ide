//! Tools: Terminal, Actions on Save, Agents › Chat, External Tools and SSH Configurations. The
//! MCP Server page comes from its own module.

use super::{TOOLS_GROUP, custom, hint, text, toggle};
use crate::settings_model::page_spec::{OptionDetail, PageSpec};

fn terminal() -> PageSpec {
    PageSpec::new("tools.terminal", "Terminal", 600)
        .group(TOOLS_GROUP)
        .project_scope()
        .keywords(&["shell", "console", "zsh", "bash", "pty"])
        .section(
            Some("Project Settings"),
            vec![custom("Start directory", &[]), custom("Environment variables", &["env"])],
        )
        .section(
            Some("Application Settings"),
            vec![
                custom("Shell path", &[]),
                text("terminal.tabName", "Default Tab name"),
                custom(
                    "Audible bell",
                    &[
                        "Close session when it ends",
                        "Mouse reporting",
                        "Copy to clipboard on selection",
                        "Paste on middle mouse button click",
                        "Override IDE shortcuts",
                        "Shell integration",
                        "Highlight hyperlinks",
                        "Use Option as Meta key",
                        "Move focus to the Editor with Escape",
                        "Configure terminal keybindings",
                    ],
                ),
                custom("Cursor shape", &["Block", "Underline", "Vertical", "Blinking cursor"]),
            ],
        )
        .section(
            Some("Font"),
            vec![custom(
                "Use editor font",
                &["Font", "Show only monospaced fonts", "Size", "Line height", "Fallback font", "Column width"],
            )],
        )
        .detail("terminal.tabName", OptionDetail { placeholder: Some("Local"), ..OptionDetail::default() })
}

fn actions_on_save() -> PageSpec {
    PageSpec::new("tools.actionsOnSave", "Actions on Save", 601)
        .group(TOOLS_GROUP)
        .project_scope()
        .keywords(&["save", "format on save", "reformat", "organize imports", "cleanup"])
        .section(Some("Activate on Save"), vec![custom("Reformat code", &["Optimize imports", "Run code cleanup"])])
        .section(None, vec![custom("Save", &[])])
}

/// Tools › Agents › Chat: under the Agents page.
fn agent_chat() -> PageSpec {
    PageSpec::new("tools.agents.chat", "Chat", 0)
        .group(TOOLS_GROUP)
        .keywords(&["agent", "claude", "codex", "usage", "quota", "rate limit", "cost", "tokens", "meter"])
        .section(
            Some("Shown in the Task Header"),
            vec![
                toggle("agents.showContextUsage", "Context window usage"),
                toggle("agents.showWorkTime", "Time spent on the task"),
                toggle("agents.showUsageLimits", "Usage limits"),
                toggle("agents.showCost", "Session cost"),
            ],
        )
        .detail(
            "agents.showUsageLimits",
            hint("The account's 5-hour and weekly limits, when the agent reports them (Claude Code)"),
        )
        .detail(
            "agents.showCost",
            hint(
                "What the session has cost so far, as the agent reports it (API pricing, also shown on subscriptions)",
            ),
        )
}

fn external_tools() -> PageSpec {
    PageSpec::new("tools.externalTools", "External Tools", 602)
        .group(TOOLS_GROUP)
        .keywords(&["program", "arguments", "macro", "working directory", "command"])
        .section(
            None,
            vec![custom(
                "External Tools",
                &["Program", "Arguments", "Working directory", "Insert Macro", "Synchronize files after execution"],
            )],
        )
}

/// SSH Configurations; it edits the global settings only.
pub(super) fn ssh_configurations() -> PageSpec {
    PageSpec::new("tools.sshConfigurations", "SSH Configurations", 603)
        .group(TOOLS_GROUP)
        .keywords(&["ssh", "remote", "host", "server", "key pair", "private key", "password", "known_hosts", "agent"])
        .section(
            None,
            vec![custom(
                "SSH Configurations",
                &[
                    "Host",
                    "Port",
                    "User name",
                    "Authentication type",
                    "Private key file",
                    "Password",
                    "Passphrase",
                    "Save password",
                    "Save passphrase",
                    "Test Connection",
                    "OpenSSH config and authentication agent",
                ],
            )],
        )
}

/// The pages in the order they are registered, MCP Server and SSH Configurations excepted.
pub(super) fn pages() -> Vec<PageSpec> {
    vec![terminal(), actions_on_save(), agent_chat(), external_tools()]
}
