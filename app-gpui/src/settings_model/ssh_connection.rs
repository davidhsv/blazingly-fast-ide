//! Test Connection of Settings › Tools › SSH Configurations: what a failure asks of the user, and
//! the Port field.

use ide_core::settings::SshAuth;
use regex::Regex;
use std::sync::LazyLock;

/// What the user can do about a failed test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SshTestRemedy {
    /// Confirm the host's key, which is not known yet.
    TrustHostKey,
    /// Type the password or the key's passphrase.
    Secret,
}

/// What a failed connection test asks for: an unknown host key to confirm, or a missing or
/// rejected password or key passphrase to type. `kind` is the error's kind as the backend names
/// it. A changed host key is never offered for trust.
pub fn ssh_test_remedy(kind: Option<&str>, message: &str, auth: SshAuth) -> Option<SshTestRemedy> {
    static CHANGED_KEY: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^SSH host key .* has changed").expect("the pattern is valid"));
    if message.starts_with("Unknown SSH host key") {
        return Some(SshTestRemedy::TrustHostKey);
    }
    if kind != Some("permissionDenied") || CHANGED_KEY.is_match(message) {
        return None;
    }
    let asks_for_secret = ["SSH password required", "SSH key passphrase required", "cannot decrypt SSH key"];
    if asks_for_secret.iter().any(|start| message.starts_with(start)) {
        return Some(SshTestRemedy::Secret);
    }
    // With the OpenSSH configuration and agent there is nothing to type unless a key is encrypted.
    let rejected = message.starts_with("SSH authentication failed") && auth != SshAuth::OpenSsh;
    rejected.then_some(SshTestRemedy::Secret)
}

/// The port typed in the Port field; `None` (the default port) for an empty or invalid one.
pub fn parse_ssh_port(text: &str) -> Option<u16> {
    let digits = text.trim();
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits.parse::<u16>().ok().filter(|&port| port > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    // SSH configuration Test Connection › offers to trust an unknown host key, never a changed one
    #[test]
    fn offers_to_trust_an_unknown_host_key_never_a_changed_one() {
        let unknown = "Unknown SSH host key for box:22 (ssh-ed25519 SHA256:abc)";
        assert_eq!(ssh_test_remedy(Some("protocol"), unknown, SshAuth::Password), Some(SshTestRemedy::TrustHostKey));
        let changed = "SSH host key of box:22 has changed (known_hosts line 3); possible attack";
        assert_eq!(ssh_test_remedy(Some("permissionDenied"), changed, SshAuth::Password), None);
    }

    // SSH configuration Test Connection › asks for a password or passphrase when one is missing or rejected
    #[test]
    fn asks_for_a_password_or_passphrase_when_one_is_missing_or_rejected() {
        let denied = |message: &str, auth: SshAuth| ssh_test_remedy(Some("permissionDenied"), message, auth);
        let secret = Some(SshTestRemedy::Secret);
        assert_eq!(denied("SSH password required for me@box:22", SshAuth::Password), secret);
        assert_eq!(denied("SSH key passphrase required for ~/.ssh/id", SshAuth::KeyPair), secret);
        assert_eq!(denied("SSH authentication failed for me@box:22", SshAuth::Password), secret);
        assert_eq!(denied("SSH authentication failed for me@box:22", SshAuth::OpenSsh), None);
        assert_eq!(ssh_test_remedy(Some("io"), "connection refused", SshAuth::Password), None);
    }

    // SSH configuration Test Connection › parses the port field
    #[test]
    fn parses_the_port_field() {
        assert_eq!(parse_ssh_port(""), None);
        assert_eq!(parse_ssh_port(" 2222 "), Some(2222));
        assert_eq!(parse_ssh_port("0"), None);
        assert_eq!(parse_ssh_port("70000"), None);
        assert_eq!(parse_ssh_port("22x"), None);
    }
}
