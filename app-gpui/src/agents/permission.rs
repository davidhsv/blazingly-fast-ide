//! A permission request in the conversation: its heading and command, the buttons that answer it and
//! how the answer is recorded. The option id is always sent back unchanged.

use crate::platform::i18n::{I18n, Param};
use ide_agent::AgentPermissionOption;
use regex::Regex;
use std::sync::LazyLock;

fn pattern(source: &str) -> Regex {
    Regex::new(source).expect("the pattern is valid")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PermissionDetails {
    /// The command the agent wants to run, as written (escapes are not interpreted).
    pub command: Option<String>,
    pub title: String,
    /// What the code area shows: the command, else the request's own title.
    pub detail: String,
}

/// Splits a request titled `Run <command>` (or `<session>: Run <command>` for a subagent) into a
/// heading naming `provider` and the command.
pub fn permission_details(title: &str, provider: &str, i18n: &mut I18n) -> PermissionDetails {
    static RUN: LazyLock<Regex> = LazyLock::new(|| pattern(r"(?s)^Run (.+)"));
    static SUBAGENT_RUN: LazyLock<Regex> = LazyLock::new(|| pattern(r"(?s)^[^\n]+?: Run (.+)"));
    let command = RUN
        .captures(title)
        .or_else(|| SUBAGENT_RUN.captures(title))
        .and_then(|m| m.get(1))
        .map(|g| g.as_str().to_owned());
    let heading = match command {
        Some(_) => i18n.tx_built(&["", " wants to run a command"], &[Param::from(provider)]),
        None => i18n.tx("Allow tool call?"),
    };
    let detail = command.clone().unwrap_or_else(|| title.to_owned());
    PermissionDetails { command, title: heading, detail }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonVariant {
    Primary,
    Secondary,
    Ghost,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PermissionChoice {
    pub label: String,
    pub variant: ButtonVariant,
}

fn session_scoped(name: &str) -> bool {
    static SESSION: LazyLock<Regex> = LazyLock::new(|| pattern(r"(?i)for (this |the )?session"));
    SESSION.is_match(name)
}

/// The button of an option, with the scope it grants spelled out.
pub fn permission_option(option: &AgentPermissionOption, command: bool, i18n: &I18n) -> PermissionChoice {
    static STOP: LazyLock<Regex> = LazyLock::new(|| pattern(r"(?i)decline and stop"));
    static PLAIN: LazyLock<Regex> = LazyLock::new(|| pattern(r"(?i)^(allow( once)?|accept|decline|reject)$"));
    let stop = option.id == "cancel" || STOP.is_match(&option.name);
    let kind = option.kind.as_str();
    let label = if stop {
        i18n.tx("Decline and Stop")
    } else if kind == "allow_always" {
        i18n.tx(if session_scoped(&option.name) { "Allow for This Session" } else { "Always Allow" })
    } else if kind == "reject_always" {
        i18n.tx("Always Decline")
    } else if (command || PLAIN.is_match(&option.name)) && kind == "allow_once" {
        i18n.tx("Allow Once")
    } else if (command || PLAIN.is_match(&option.name)) && kind == "reject_once" {
        i18n.tx("Decline")
    } else {
        option.name.clone()
    };
    let variant = if kind == "allow_once" {
        ButtonVariant::Primary
    } else if stop || kind == "reject_always" {
        ButtonVariant::Ghost
    } else {
        ButtonVariant::Secondary
    };
    PermissionChoice { label, variant }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PermissionResult {
    pub label: String,
    pub allowed: bool,
}

/// How an answered request is recorded: compactly, by the kind of the chosen option.
pub fn permission_result(options: &[AgentPermissionOption], chosen: &str, i18n: &I18n) -> PermissionResult {
    if chosen == "Cancelled" {
        return PermissionResult { label: i18n.tx("Cancelled"), allowed: false };
    }
    let Some(option) = options.iter().find(|o| o.name == chosen) else {
        return PermissionResult { label: chosen.to_owned(), allowed: false };
    };
    let label = match option.kind.as_str() {
        "allow_once" => i18n.tx("Allowed once"),
        "allow_always" if session_scoped(&option.name) => i18n.tx("Allowed for this session"),
        "allow_always" => i18n.tx("Always allowed"),
        "reject_once" => i18n.tx("Declined"),
        "reject_always" => i18n.tx("Always declined"),
        _ => chosen.to_owned(),
    };
    PermissionResult { label, allowed: option.kind.starts_with("allow") }
}

/// `python3 …`: a resolved command shortened to its program.
pub fn command_prefix(command: &str) -> String {
    format!("{} …", command.split_whitespace().next().unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TITLE: &str =
        "Run python3 -c 'from pathlib import Path\np = Path(\"note.txt\")\np.write_text(\"one\\ntwo\")'";

    fn option(kind: &str, id: &str, name: &str) -> AgentPermissionOption {
        AgentPermissionOption { id: id.to_owned(), name: name.to_owned(), kind: kind.to_owned() }
    }

    #[test]
    fn separates_the_command_from_the_heading_without_interpreting_shell_or_python_escapes() {
        let details = permission_details(TITLE, "Codex", &mut I18n::default());
        assert_eq!(details.title, "Codex wants to run a command");
        let command = details.command.expect("a command");
        assert!(command.contains("Path(\"note.txt\")\np.write_text(\"one\\ntwo\")"));
        assert_eq!(command_prefix(&command), "python3 …");
    }

    #[test]
    fn names_the_requesting_provider() {
        for provider in ["Codex", "Claude Code", "Gemini"] {
            let details = permission_details(TITLE, provider, &mut I18n::default());
            assert_eq!(details.title, format!("{provider} wants to run a command"));
        }
    }

    #[test]
    fn keeps_a_subagent_command_in_the_code_area_without_its_session_label() {
        let mut i18n = I18n::default();
        assert_eq!(
            permission_details(&format!("Explore: {TITLE}"), "Codex", &mut i18n).command,
            permission_details(TITLE, "Codex", &mut i18n).command
        );
    }

    #[test]
    fn presents_kind_without_changing_its_id() {
        let i18n = I18n::default();
        for (kind, id, label, variant) in [
            ("allow_once", "yes", "Allow Once", ButtonVariant::Primary),
            ("allow_always", "policy", "Always Allow", ButtonVariant::Secondary),
            ("reject_once", "no", "Decline", ButtonVariant::Secondary),
            ("reject_always", "never", "Always Decline", ButtonVariant::Ghost),
            ("reject_once", "cancel", "Decline and Stop", ButtonVariant::Ghost),
        ] {
            let option = option(kind, id, "Always Allow `python3 -c a very long script`");
            assert_eq!(permission_option(&option, true, &i18n), PermissionChoice { label: label.into(), variant });
            assert_eq!(option.id, id);
        }
    }

    #[test]
    fn preserves_the_narrower_session_scope_and_custom_question_answers() {
        let i18n = I18n::default();
        let session = option("allow_always", "session", "Allow for This Session");
        assert_eq!(permission_option(&session, true, &i18n).label, "Allow for This Session");
        assert_eq!(permission_option(&option("allow_once", "ship", "Ship it"), false, &i18n).label, "Ship it");
    }

    #[test]
    fn records_kind_compactly() {
        let i18n = I18n::default();
        let name = "Option with a long command";
        for (kind, label, allowed) in [
            ("allow_once", "Allowed once", true),
            ("allow_always", "Always allowed", true),
            ("reject_once", "Declined", false),
            ("reject_always", "Always declined", false),
        ] {
            assert_eq!(
                permission_result(&[option(kind, "opaque", name)], name, &i18n),
                PermissionResult { label: label.into(), allowed }
            );
        }
    }
}
