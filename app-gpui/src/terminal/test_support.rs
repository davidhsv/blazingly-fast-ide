//! What the tests that start real shells share.

use crate::backend::Backend;
use crate::platform::command_line::quote_path;
use crate::platform::settings;
use gpui_kit::App;
use ide_workspace::settings::SettingsScope;
use std::path::{Path, PathBuf};

/// A plain shell for the tests: `/bin/sh` reading a script, so neither interactive nor a login
/// shell, with no startup file and no history; it runs each line typed into it. Ctrl+C ends the
/// command that runs and not the shell, as at a prompt: the trap is reset in the commands it
/// starts, and a read the interrupt cuts short (status above 128) is read again.
pub const PLAIN_SHELL: &str = "#!/bin/sh\ntrap : INT\nwhile :; do\n  IFS= read -r line\n  status=$?\n  \
                               if [ \"$status\" -gt 128 ]; then continue; fi\n  \
                               if [ \"$status\" -ne 0 ]; then break; fi\n  eval \"$line\"\ndone\n";

/// Writes [`PLAIN_SHELL`] at `<home>/bin/plainsh` and returns its path.
pub fn plain_shell(home: &Path) -> PathBuf {
    let bin = home.join("bin");
    std::fs::create_dir_all(&bin).expect("the folder is made");
    let path = bin.join("plainsh");
    if !path.exists() {
        std::fs::write(&path, PLAIN_SHELL).expect("the shell is written");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("the shell runs");
        }
    }
    path
}

/// Gives every shell started from now on a home of its own under `home`, so that a test reads
/// none of the user's shell startup files and writes nothing to the user's shell history, and
/// makes [`plain_shell`] the shell of Settings › Terminal, without shell integration. Call once
/// the backend runs and before any terminal starts.
pub fn isolate_shells(home: &Path, cx: &mut App) {
    let shell_home = home.join("shell-home");
    std::fs::create_dir_all(&shell_home).expect("the shell's home is created");
    std::fs::write(shell_home.join(".zshrc"), "").expect("an empty startup file is written");
    let env = serde_json::json!({ "HOME": shell_home, "ZDOTDIR": shell_home, "HISTFILE": "/dev/null" });
    settings::set("", SettingsScope::Global, "terminal.env", &env, cx).expect("the shells' environment is set");
    let line = quote_path(&plain_shell(home).to_string_lossy());
    settings::set("", SettingsScope::Global, "terminal.shellPath", &serde_json::json!(line), cx)
        .expect("the shell is set");
    settings::set("", SettingsScope::Global, "terminal.shellIntegration", &serde_json::json!(false), cx)
        .expect("shell integration is off");
}

/// Opens `root`, trusted, as the project of `window` in the backend: only a trusted project's
/// terminals start the shell they are asked for (anywhere else the backend starts the user's
/// login shell).
pub fn trusted_folder(window: &str, root: &Path, cx: &mut App) {
    let backend = cx.global::<Backend>();
    let host = backend.host.clone();
    host.trust.decide(root, true, false).expect("the folder is trusted");
    backend.block_on(async { host.open_project(window, root) }).expect("the project opens");
}

/// Program names that read their startup files and prompt when they run with no script.
const SHELLS: [&str; 8] = ["zsh", "bash", "sh", "fish", "ksh", "dash", "tcsh", "csh"];

/// Panics when a terminal is about to start a login or an interactive shell (a known shell with no
/// arguments, as the backend then adds `-l`, or with `-l`, `--login` or `-i`), or the machine's gh.
/// `trusted` is whether the window's project is trusted: in any other window the backend starts the
/// user's login shell whatever was asked for.
pub fn assert_plain_shell(trusted: bool, shell: Option<&str>, args: &[String]) {
    let (shell, args) = if trusted { (shell, args) } else { (None, &[][..]) };
    let Some(shell) = shell.map(str::trim).filter(|shell| !shell.is_empty()) else {
        panic!("a test terminal starts the user's login shell; use the plain shell of `isolate_shells`")
    };
    let name = Path::new(shell).file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    // gh would reach GitHub with the account signed in on this machine (`gh auth login` asks for one).
    assert_ne!(name, "gh", "a test terminal starts the machine's gh {args:?}");
    let interactive = args.is_empty() || args.iter().any(|arg| ["-l", "--login", "-i"].contains(&arg.as_str()));
    assert!(
        !(SHELLS.contains(&name.as_str()) && interactive),
        "a test terminal starts `{shell}` {args:?}, an interactive or login shell; use the plain shell"
    );
}
