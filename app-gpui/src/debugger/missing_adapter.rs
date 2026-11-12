//! A debug adapter that is not installed: name it and offer the one-click install followed by the
//! same launch again, instead of a bare error.

use ide_core::IdeError;
use ide_core::error::ErrorKind;

/// Adapters the tools installer can provide: (adapter id, tool name, notification title).
const INSTALLABLE: [(&str, &str, &str); 8] = [
    ("kotlin-debug-adapter", "kotlin-debug-adapter", "The Kotlin debugger is not installed"),
    ("netcoredbg", "netcoredbg", "netcoredbg is not installed"),
    ("debugpy", "debugpy", "debugpy is not installed"),
    ("js-debug", "js-debug", "The JavaScript debugger (vscode-js-debug) is not installed"),
    ("delve", "delve", "Delve is not installed"),
    ("codelldb", "codelldb", "CodeLLDB is not installed"),
    ("php-debug", "php-debug", "PHP Debug (Xdebug) is not installed"),
    ("bash-debug", "bash-debug", "Bash Debug is not installed"),
];

/// The install offer of a missing adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingAdapter {
    pub tool: &'static str,
    pub title: &'static str,
    pub body: String,
}

/// The install offer for a failed start, or `None` when the failure is something else.
pub fn missing_adapter(adapter: &str, error: &IdeError) -> Option<MissingAdapter> {
    let (_, tool, title) = INSTALLABLE.iter().copied().find(|(id, ..)| *id == adapter)?;
    if error.kind != ErrorKind::Unavailable {
        return None;
    }
    let message = error.message.as_str();
    if adapter == "debugpy" && message.starts_with("debugpy is not installed") {
        let body =
            format!("{message}. OpenIDEA can install a private copy that every Python interpreter uses for debugging.");
        return Some(MissingAdapter { tool, title, body });
    }
    message.starts_with(&format!("debug adapter `{adapter}` not found")).then(|| MissingAdapter {
        tool,
        title,
        body: message.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unavailable(message: &str) -> IdeError {
        IdeError::unavailable(message)
    }

    fn not_found(adapter: &str) -> IdeError {
        unavailable(&format!("debug adapter `{adapter}` not found (x)"))
    }

    #[test]
    fn offers_openideas_debugpy_when_the_interpreter_has_none() {
        let e = unavailable("debugpy is not installed for /opt/homebrew/bin/python3.14. Install OpenIDEA's copy");
        let offer = missing_adapter("debugpy", &e).expect("an install offer");
        assert_eq!((offer.tool, offer.title), ("debugpy", "debugpy is not installed"));
    }

    #[test]
    fn offers_the_managed_install_of_a_missing_adapter_binary() {
        let e = unavailable(
            "debug adapter `js-debug` not found (`js-debug-adapter` is neither installed by OpenIDEA nor on PATH)",
        );
        let offer = missing_adapter("js-debug", &e).expect("an install offer");
        assert_eq!(offer.tool, "js-debug");
        assert_eq!(offer.title, "The JavaScript debugger (vscode-js-debug) is not installed");
        for adapter in ["kotlin-debug-adapter", "netcoredbg", "delve"] {
            assert_eq!(missing_adapter(adapter, &not_found(adapter)).map(|offer| offer.tool), Some(adapter));
        }
        assert_eq!(
            missing_adapter("bash-debug", &not_found("bash-debug")).map(|offer| offer.title),
            Some("Bash Debug is not installed"),
        );
    }

    #[test]
    fn offers_the_managed_install_of_the_missing_php_debug_adapter() {
        let e = unavailable(concat!(
            "debug adapter `php-debug` not found (`php-debug-adapter` is neither installed by OpenIDEA nor on PATH). ",
            "Install PHP Debug (Xdebug) in Settings › Languages & Frameworks › Language Servers, and enable Xdebug",
        ));
        assert_eq!(
            missing_adapter("php-debug", &e),
            Some(MissingAdapter {
                tool: "php-debug",
                title: "PHP Debug (Xdebug) is not installed",
                body: e.message.clone()
            }),
        );
    }

    #[test]
    fn leaves_other_failures_and_adapters_without_an_installer_alone() {
        assert_eq!(missing_adapter("js-debug", &unavailable("connection refused")), None);
        // Not a missing tool: any other kind of failure with the same words.
        assert_eq!(missing_adapter("js-debug", &IdeError::internal("debug adapter `js-debug` not found")), None);
        assert_eq!(missing_adapter("lldb-dap", &not_found("lldb-dap")), None);
    }
}
