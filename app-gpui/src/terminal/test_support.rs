//! What the tests that start real shells share.

use crate::platform::settings;
use gpui_kit::App;
use ide_workspace::settings::SettingsScope;
use std::path::Path;

/// Gives every shell started from now on a home of its own under `home`, so that a test reads
/// none of the user's shell startup files and writes nothing to the user's shell history. Call
/// once the backend runs and before any terminal starts.
pub fn isolate_shells(home: &Path, cx: &mut App) {
    let shell_home = home.join("shell-home");
    std::fs::create_dir_all(&shell_home).expect("the shell's home is created");
    std::fs::write(shell_home.join(".zshrc"), "").expect("an empty startup file is written");
    let env = serde_json::json!({ "HOME": shell_home, "ZDOTDIR": shell_home, "HISTFILE": "/dev/null" });
    settings::set("", SettingsScope::Global, "terminal.env", &env, cx).expect("the shells' environment is set");
}
