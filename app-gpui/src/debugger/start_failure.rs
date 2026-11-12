//! Why a debug session didn't start, as the user reads it. An adapter that listens for the
//! debuggee (php-debug waiting for Xdebug) fails with Node's
//! `listen EADDRINUSE: address already in use :::9003` when another IDE holds the port.

use crate::platform::i18n::{I18n, Param};
use regex::Regex;
use std::sync::LazyLock;

static PORT_IN_USE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?-u:\b)EADDRINUSE(?-u:\b).*:([0-9]+)\s*$").expect("the port pattern is valid"));
static RUBY_DEBUG_MISSING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"^the bundle of (.+) has no `debug` gem\. ",
        r#"Add `gem "debug"` to the Gemfile and run `bundle install`$"#,
    ))
    .expect("the bundle pattern is valid")
});

/// The message of a failed start: `message` is what the backend or the adapter reported.
pub fn start_failure_message(message: &str, i18n: &mut I18n) -> String {
    if let Some(gemfile) = RUBY_DEBUG_MISSING.captures(message).and_then(|found| found.get(1)) {
        return i18n.tx_built(
            &["the bundle of ", " has no `debug` gem. Add `gem \"debug\"` to the Gemfile and run `bundle install`"],
            &[Param::from(gemfile.as_str())],
        );
    }
    match PORT_IN_USE.captures(message).and_then(|found| found.get(1)) {
        Some(port) => i18n.tx_built(
            &[
                "Port ",
                " is already in use: another IDE or debug session may be listening on it. \
                 Stop it or choose another port.",
            ],
            &[Param::from(port.as_str())],
        ),
        None => message.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PORT_IN_USE_TEXT: &str = concat!(
        "Port 9003 is already in use: another IDE or debug session may be listening on it. ",
        "Stop it or choose another port.",
    );

    #[test]
    fn names_the_port_another_listener_holds() {
        let mut i18n = I18n::default();
        for message in [
            "launch failed: listen EADDRINUSE: address already in use :::9003",
            "launch failed: listen EADDRINUSE: address already in use 127.0.0.1:9003",
        ] {
            assert_eq!(start_failure_message(message, &mut i18n), PORT_IN_USE_TEXT, "{message}");
        }
    }

    #[test]
    fn translates_the_missing_ruby_debug_gem_instruction() {
        let message = "the bundle of /app with spaces/Gemfile has no `debug` gem. \
                       Add `gem \"debug\"` to the Gemfile and run `bundle install`";
        let mut i18n = I18n::default();
        assert_eq!(start_failure_message(message, &mut i18n), message);
        i18n.set_locale(Some("zh"), "en");
        assert_eq!(
            start_failure_message(message, &mut i18n),
            "/app with spaces/Gemfile 的 bundle 中没有 `debug` gem。请在 Gemfile 中添加 `gem \"debug\"`，然后运行 `bundle install`",
        );
    }

    #[test]
    fn keeps_other_failures_as_they_are() {
        let mut i18n = I18n::default();
        for message in ["launch failed: Attach requests are not supported", "debug adapter exited"] {
            assert_eq!(start_failure_message(message, &mut i18n), message);
        }
    }
}
