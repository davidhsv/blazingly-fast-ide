//! An agent's error as shown in the conversation: the IDE's own sign-in guidance is translated, the
//! command in it and whatever the agent adds stay as written.

use crate::platform::i18n::{I18n, Param};
use regex::Regex;
use std::sync::LazyLock;

/// `message` with a leading "<agent> needs you to sign in. <hint>" line translated; any other message
/// (the agent's own errors) unchanged.
pub fn agent_error_text(message: &str, i18n: &mut I18n) -> String {
    static AUTH: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?s)^(.+?) needs you to sign in\. ([^\n]+)(.*)$").expect("the pattern is valid"));
    let Some(m) = AUTH.captures(message) else { return message.to_owned() };
    let (Some(name), Some(hint)) = (m.get(1), m.get(2)) else { return message.to_owned() };
    // The name cannot span lines: `.` of the guidance pattern does not cross them.
    if name.as_str().contains(['\n', '\r']) {
        return message.to_owned();
    }
    let hint = i18n.tx_template(hint.as_str());
    let guidance =
        i18n.tx_built(&["", " needs you to sign in. ", ""], &[Param::from(name.as_str()), Param::from(hint)]);
    format!("{guidance}{}", m.get(3).map_or("", |g| g.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chinese() -> I18n {
        let mut i18n = I18n::default();
        i18n.set_locale(Some("zh"), "en");
        i18n
    }

    #[test]
    fn translates_sign_in_guidance_with_its_command_intact() {
        for (hint, translated) in [
            ("Run `cursor-agent login` in a terminal.", "在终端中运行 `cursor-agent login`。"),
            (
                "Run `'/registry path/cursor-agent' login` in a terminal.",
                "在终端中运行 `'/registry path/cursor-agent' login`。",
            ),
            (
                "Run `& 'C:\\Agent Tools\\cursor-agent.cmd' login` in a terminal.",
                "在终端中运行 `& 'C:\\Agent Tools\\cursor-agent.cmd' login`。",
            ),
            ("Run `'/registry/claude'` in a terminal and sign in.", "在终端中运行 `'/registry/claude'` 并登录。"),
        ] {
            let text = agent_error_text(&format!("Cursor needs you to sign in. {hint}"), &mut chinese());
            assert_eq!(text, format!("Cursor 需要你登录。{translated}"), "{hint}");
        }
    }

    #[test]
    fn preserves_external_agent_errors() {
        let message = "Agent says: authentication failed";
        assert_eq!(agent_error_text(message, &mut chinese()), message);
    }

    #[test]
    fn translates_only_the_guidance_when_the_agent_adds_its_own_explanation() {
        let details = "Authentication failed: token expired.\nRun `account repair` in a terminal.";
        let message = format!(
            "Google Antigravity needs you to sign in. Run `'/registry/antigravity' auth login` in a terminal.\n\n\
             {details}"
        );
        assert_eq!(
            agent_error_text(&message, &mut chinese()),
            format!("Google Antigravity 需要你登录。在终端中运行 `'/registry/antigravity' auth login`。\n\n{details}")
        );
    }
}
