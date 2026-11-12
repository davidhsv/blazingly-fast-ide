//! Remote development: in a window whose project lives on another machine, the commands that
//! machine's server answers run there instead of here. This is where each command of such a
//! window runs, how its answer is shaped for the client, and when the window stops being a
//! remote one.
//!
//! A command is named as the server names it (`fs_read_dir`, `settings_set`) and carries its
//! arguments as a JSON object. The caller carries a [`RoutePlan`] out and hands the answers back.

use super::client_settings::{is_client_setting, overlay_client_settings};
use serde_json::{Value, json};
use std::collections::HashSet;

/// Where a remote project lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteTarget {
    /// An SSH configuration, by id.
    Ssh { config: String },
    /// A WSL distribution, by name.
    Wsl { distro: String },
    /// The dev container of a local folder; `config` picks one of several (empty: the default).
    DevContainer { folder: String, config: String },
}

/// The connection of a remote window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteSession {
    pub target: RemoteTarget,
    /// The project folder on the remote machine.
    pub root: String,
    /// `user@host:port`, or the distribution's name.
    pub host: String,
    /// `linux`, `macos` or `windows`: the OS the project's commands run on.
    pub os: String,
    /// The commands the server answers.
    pub commands: Vec<String>,
}

/// Where a command of a remote window runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Destination {
    Local,
    Remote,
    /// Here and on the server: a global tooling setting, which the server's language servers
    /// and the local windows both read.
    Both,
}

fn text<'v>(args: &'v Value, name: &str) -> &'v str {
    args.get(name).and_then(Value::as_str).unwrap_or("")
}

/// Where `cmd` runs when the server answers the commands in `served`. Always local: fonts and
/// the global settings snapshot, client settings, opening a project (a local folder; it leaves
/// the remote one) and closing it (that drops the connection, which ends the server's session).
pub fn remote_routing(cmd: &str, args: &Value, served: &HashSet<String>) -> Destination {
    if matches!(cmd, "system_font_families" | "settings_get_global") {
        return Destination::Local;
    }
    if cmd == "settings_set" && is_client_setting(text(args, "key")) {
        return Destination::Local;
    }
    if !served.contains(cmd) || matches!(cmd, "open_project" | "close_project") {
        return Destination::Local;
    }
    if cmd == "settings_set" && text(args, "scope") == "global" {
        return Destination::Both;
    }
    Destination::Remote
}

/// A command with its arguments.
#[derive(Debug, Clone, PartialEq)]
pub struct Call {
    pub cmd: String,
    pub args: Value,
}

impl Call {
    fn new(cmd: &str, args: &Value) -> Self {
        Self { cmd: cmd.to_owned(), args: args.clone() }
    }
}

/// What to do with the server's answer before the caller of the command sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    AsIs,
    /// Effective settings: the client's own settings are laid over them.
    Settings,
    /// A project snapshot: the client's settings are laid over its `settings`.
    Project,
    /// A build model: a server that predates independent Gradle builds answers with one build
    /// where a list of them is expected.
    BuildModel,
}

impl Shape {
    /// Whether [`shape_response`] needs this machine's global settings (`settings_get_global`,
    /// run locally).
    pub fn needs_client_settings(self) -> bool {
        matches!(self, Self::Settings | Self::Project)
    }
}

/// How a command of a remote window is carried out.
#[derive(Debug, Clone, PartialEq)]
pub struct RoutePlan {
    /// Run these here first, in order. Without a remote call the last one's answer is the result.
    pub local: Vec<Call>,
    /// Then run this on the server; its answer, shaped, is the result.
    pub remote: Option<Call>,
    pub shape: Shape,
    /// When the command succeeds the window is no longer a remote one.
    pub ends_session: bool,
}

const BUILD_MODEL_COMMANDS: [&str; 4] = ["build_model", "build_sync", "gradle_link_projects", "gradle_unlink_projects"];

/// The plan for `cmd` in a remote window whose server answers `served`.
pub fn route(cmd: &str, args: &Value, served: &HashSet<String>) -> RoutePlan {
    let local_only = |ends_session: bool| RoutePlan {
        local: vec![Call::new(cmd, args)],
        remote: None,
        shape: Shape::AsIs,
        ends_session,
    };
    if matches!(cmd, "open_project" | "light_edit_open") {
        return local_only(true);
    }
    let destination = remote_routing(cmd, args, served);
    if cmd == "settings_set" && is_client_setting(text(args, "key")) {
        // A client preference is this machine's, whichever scope the window wrote it in; the
        // answer is the host's settings again, with the new value laid over them.
        let mut global = args.clone();
        if let Some(fields) = global.as_object_mut() {
            fields.insert("scope".to_owned(), json!("global"));
        }
        return RoutePlan {
            local: vec![Call { cmd: cmd.to_owned(), args: global }],
            remote: Some(Call::new("settings_get", &json!({}))),
            shape: Shape::Settings,
            ends_session: false,
        };
    }
    if destination == Destination::Local {
        return local_only(false);
    }
    let shape = match cmd {
        "settings_set" | "settings_get" => Shape::Settings,
        "project_current" => Shape::Project,
        _ if BUILD_MODEL_COMMANDS.contains(&cmd) => Shape::BuildModel,
        _ => Shape::AsIs,
    };
    RoutePlan {
        local: if destination == Destination::Both { vec![Call::new(cmd, args)] } else { Vec::new() },
        remote: Some(Call::new(cmd, args)),
        shape,
        ends_session: false,
    }
}

/// The result of a command from the server's `answer`. `client` is this machine's global
/// settings, needed when [`Shape::needs_client_settings`].
pub fn shape_response(shape: Shape, answer: Value, client: Option<&Value>) -> Value {
    let overlaid = |settings: &Value| client.map_or_else(|| settings.clone(), |c| overlay_client_settings(settings, c));
    match (shape, answer) {
        (Shape::Settings, settings) => overlaid(&settings),
        (Shape::Project, Value::Object(mut project)) => {
            let settings = overlaid(project.get("settings").unwrap_or(&Value::Null));
            project.insert("settings".to_owned(), settings);
            Value::Object(project)
        }
        (Shape::BuildModel, Value::Object(mut model)) => {
            if let Some(gradle) = model.get_mut("gradle").filter(|gradle| gradle.is_object()) {
                *gradle = Value::Array(vec![gradle.take()]);
            }
            Value::Object(model)
        }
        (_, answer) => answer,
    }
}

/// Whether this window's project is a remote one, and what its server answers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RemoteWindow {
    session: Option<RemoteSession>,
    served: HashSet<String>,
}

impl RemoteWindow {
    pub fn new() -> Self {
        Self::default()
    }

    /// Routes the commands the server answers to it from now on.
    pub fn enable(&mut self, session: RemoteSession) {
        self.served = session.commands.iter().cloned().collect();
        self.session = Some(session);
    }

    pub fn disable(&mut self) {
        self.served.clear();
        self.session = None;
    }

    pub fn session(&self) -> Option<&RemoteSession> {
        self.session.as_ref()
    }

    pub fn is_remote(&self) -> bool {
        self.session.is_some()
    }

    /// The plan for `cmd`; `None` in a window that is not remote: the command runs here as usual.
    pub fn route(&self, cmd: &str, args: &Value) -> Option<RoutePlan> {
        self.session.as_ref().map(|_| route(cmd, args, &self.served))
    }

    /// A plan was carried out. A successful one that leaves the remote project ends the session
    /// before the command's result is published; a failed one leaves the window as it was.
    pub fn finished(&mut self, plan: &RoutePlan, succeeded: bool) {
        if plan.ends_session && succeeded {
            self.disable();
        }
    }

    /// The window's project changed from `previous_root` to `root` (`opening`: a project is
    /// being opened). Leaving the session's root by any way (Close Project, a local project
    /// opened over it) ends the session. A window that never held the session's project (a
    /// comparison or deployment window rides its parent's session and keeps just the settings)
    /// leaves the session alone.
    pub fn project_changed(&mut self, previous_root: Option<&str>, root: Option<&str>, opening: bool) {
        let Some(session) = &self.session else { return };
        if opening || previous_root != Some(session.root.as_str()) || root == Some(session.root.as_str()) {
            return;
        }
        self.disable();
    }

    /// The OS this window's commands run on: the server's, else this desktop's.
    pub fn host_os<'a>(&'a self, desktop_os: &'a str) -> &'a str {
        self.session.as_ref().map_or(desktop_os, |session| session.os.as_str())
    }

    /// The location of this window's project: remote when `root` is the session's, else the
    /// local folder itself.
    pub fn project_location(&self, root: Option<&str>) -> Option<String> {
        let root = root.filter(|root| !root.is_empty())?;
        Some(match &self.session {
            Some(session) if session.root == root => remote_location(&session.target, root),
            _ => root.to_owned(),
        })
    }
}

// Remote project locations: `ssh://<configuration id>/<absolute path>`,
// `wsl://<distribution>/<absolute path>` or `devcontainer://<local folder>[:<config>]/<path>`,
// what the session and the recent projects remember for a remote window.

const SSH_SCHEME: &str = "ssh://";
const WSL_SCHEME: &str = "wsl://";
const DEV_CONTAINER_SCHEME: &str = "devcontainer://";

/// The SSH configuration id, the WSL distribution name or the dev container's local folder.
pub fn target_name(target: &RemoteTarget) -> &str {
    match target {
        RemoteTarget::Ssh { config } => config,
        RemoteTarget::Wsl { distro } => distro,
        RemoteTarget::DevContainer { folder, .. } => folder,
    }
}

/// Whether two targets are the same machine (for a dev container: the same folder and config).
pub fn same_target(a: &RemoteTarget, b: &RemoteTarget) -> bool {
    a == b
}

/// Percent-encodes everything but letters, digits and `-_.!~*'()`.
fn encode_component(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&byte) {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// The text a percent-encoded component stands for; `None` when it is malformed.
fn decode_component(text: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(text.len());
    let mut rest = text.as_bytes();
    while let Some((&byte, tail)) = rest.split_first() {
        if byte == b'%' {
            let hex = tail.get(..2).filter(|hex| hex.iter().all(u8::is_ascii_hexdigit))?;
            bytes.push(u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()?);
            rest = &tail[2..];
        } else {
            bytes.push(byte);
            rest = tail;
        }
    }
    String::from_utf8(bytes).ok()
}

/// The location of `path` on `target`.
pub fn remote_location(target: &RemoteTarget, path: &str) -> String {
    let (scheme, config) = match target {
        RemoteTarget::Ssh { .. } => (SSH_SCHEME, String::new()),
        RemoteTarget::Wsl { .. } => (WSL_SCHEME, String::new()),
        RemoteTarget::DevContainer { config, .. } if config.is_empty() => (DEV_CONTAINER_SCHEME, String::new()),
        RemoteTarget::DevContainer { config, .. } => (DEV_CONTAINER_SCHEME, format!(":{}", encode_component(config))),
    };
    let slash = if path.starts_with('/') { "" } else { "/" };
    format!("{scheme}{}{config}{slash}{path}", encode_component(target_name(target)))
}

/// The target and path a location names; `None` for anything else (a local path).
pub fn parse_remote_location(location: &str) -> Option<(RemoteTarget, String)> {
    let (scheme, rest) = [SSH_SCHEME, WSL_SCHEME, DEV_CONTAINER_SCHEME]
        .into_iter()
        .find_map(|scheme| location.strip_prefix(scheme).map(|rest| (scheme, rest)))?;
    let slash = rest.find('/').filter(|slash| *slash > 0)?;
    let (authority, path) = rest.split_at(slash);
    let target = match scheme {
        SSH_SCHEME => RemoteTarget::Ssh { config: decode_component(authority)? },
        WSL_SCHEME => RemoteTarget::Wsl { distro: decode_component(authority)? },
        _ => {
            let (folder, config) = authority.split_once(':').unwrap_or((authority, ""));
            RemoteTarget::DevContainer { folder: decode_component(folder)?, config: decode_component(config)? }
        }
    };
    Some((target, path.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::super::client_settings::CLIENT_SETTINGS_KEYS;
    use super::*;

    fn served(commands: &[&str]) -> HashSet<String> {
        commands.iter().map(|&command| command.to_owned()).collect()
    }

    fn session(root: &str, commands: &[&str]) -> RemoteSession {
        RemoteSession {
            target: RemoteTarget::Ssh { config: "test".to_owned() },
            root: root.to_owned(),
            host: "test".to_owned(),
            os: "linux".to_owned(),
            commands: commands.iter().map(|&command| command.to_owned()).collect(),
        }
    }

    const SETTINGS_COMMANDS: [&str; 3] = ["settings_get", "settings_set", "project_current"];

    fn remote_window() -> RemoteWindow {
        let mut window = RemoteWindow::new();
        window.enable(session("/remote", &SETTINGS_COMMANDS));
        window
    }

    /// This machine's global settings.
    fn local() -> Value {
        json!({
            "keymap": "my-map",
            "keymapOverrides": { "editor.copy": ["Meta+J"] },
            "customKeymaps": {
                "my-map": { "name": "Mine", "parent": "vscode-macos", "overrides": { "editor.paste": ["Meta+K"] } },
            },
            "appearance": { "theme": "dark" },
            "editor": { "fontSize": 19, "tabSize": 8 },
            "terminal": { "fontSize": 18, "shellPath": "/local" },
        })
    }

    /// The settings the remote host answers with.
    fn remote() -> Value {
        json!({
            "keymap": "intellij-linux",
            "keymapOverrides": {},
            "customKeymaps": {},
            "appearance": { "theme": "light" },
            "editor": { "fontSize": 13, "tabSize": 2 },
            "terminal": { "fontSize": 13, "shellPath": "/remote" },
        })
    }

    fn remote_call(cmd: &str, args: Value) -> Option<Call> {
        Some(Call { cmd: cmd.to_owned(), args })
    }

    /// The editor font size and tab size of shaped settings: the client's 19 and the host's 2
    /// once this machine's settings lie over the host's.
    fn font_and_tab_size(settings: &Value) -> (Option<i64>, Option<i64>) {
        (settings["editor"]["fontSize"].as_i64(), settings["editor"]["tabSize"].as_i64())
    }

    // remote invoke › routes served commands, keeps opening / closing local and sets global settings on both sides
    #[test]
    fn routes_served_commands_keeps_opening_and_closing_local_and_sets_global_settings_on_both_sides() {
        let served = served(&[
            "vcs_add_to_ignore",
            "fs_read_dir",
            "open_project",
            "close_project",
            "settings_set",
            "system_font_families",
        ]);
        let routing = |cmd: &str, args: Value| remote_routing(cmd, &args, &served);
        let ignore = json!({ "paths": ["/remote/file"], "destination": "localExclude" });
        assert_eq!(routing("vcs_add_to_ignore", ignore), Destination::Remote);
        assert_eq!(routing("system_font_families", json!({ "refresh": true })), Destination::Local);
        assert_eq!(routing("fs_read_dir", json!({ "path": "/a" })), Destination::Remote);
        assert_eq!(routing("window_open_settings", json!({})), Destination::Local);
        assert_eq!(routing("open_project", json!({ "path": "/a" })), Destination::Local);
        assert_eq!(routing("close_project", json!({})), Destination::Local);
        let set = |scope: &str| json!({ "scope": scope, "key": "k", "value": "1" });
        assert_eq!(routing("settings_set", set("project")), Destination::Remote);
        assert_eq!(routing("settings_set", set("global")), Destination::Both);
    }

    // remote session lifetime › keeps routing when a window with no project of its own updates the settings only
    #[test]
    fn keeps_routing_when_a_window_with_no_project_of_its_own_updates_the_settings_only() {
        let mut window = RemoteWindow::new();
        window.enable(session("/home/me/app", &["vcs_roots"]));
        window.project_changed(None, None, false);
        assert!(window.is_remote());
    }

    // remote session lifetime › ends routing when the window's project leaves the session's root
    #[test]
    fn ends_routing_when_the_windows_project_leaves_the_sessions_root() {
        let mut window = RemoteWindow::new();
        window.enable(session("/home/me/app", &["vcs_roots"]));
        window.project_changed(None, Some("/home/me/app"), false);
        assert!(window.is_remote());
        // Close Project (or a local project opened over the remote one): the session goes.
        window.project_changed(Some("/home/me/app"), None, false);
        assert!(!window.is_remote());
    }

    // remote session lifetime › keeps routing while the project still sits at the session's root
    #[test]
    fn keeps_routing_while_the_project_still_sits_at_the_sessions_root() {
        let mut window = RemoteWindow::new();
        window.enable(session("/home/me/app", &["vcs_roots"]));
        window.project_changed(None, Some("/home/me/app"), false);
        window.project_changed(Some("/home/me/app"), Some("/home/me/app"), false);
        assert!(window.is_remote());
        // While another project is opening nothing is decided yet.
        window.project_changed(Some("/home/me/app"), None, true);
        assert!(window.is_remote());
    }

    // remote client settings › routes window reports through the commands advertised by the remote host
    #[test]
    fn routes_window_reports_through_the_commands_advertised_by_the_remote_host() {
        let mut window = RemoteWindow::new();
        let reports = ["mcp_set_open_editors", "mcp_set_focused_editor", "mcp_report_problems"];
        window.enable(session("/remote", &reports));
        let files = json!([{ "path": "/remote/a.txt", "items": [] }]);
        for (cmd, args) in [
            ("mcp_set_open_editors", json!({ "paths": ["/remote/a.txt"] })),
            ("mcp_set_focused_editor", json!({ "editor": null, "focused": true })),
            ("mcp_report_problems", json!({ "reset": true, "files": files })),
            ("mcp_report_problems", json!({ "reset": false, "files": files })),
        ] {
            let plan = window.route(cmd, &args).expect("the window is remote");
            assert!(plan.local.is_empty(), "{cmd}");
            assert_eq!(plan.remote, remote_call(cmd, args));
            assert_eq!(plan.shape, Shape::AsIs);
        }
    }

    // remote client settings › accepts the released and current remote Gradle build models: $gradle
    #[test]
    fn accepts_the_released_and_current_remote_gradle_build_models() {
        let two = json!([{ "dir": "one", "projects": [] }, { "dir": "two", "projects": [] }]);
        let cases = [
            (json!({ "dir": "app", "projects": [] }), json!([{ "dir": "app", "projects": [] }])),
            (two.clone(), two),
            (Value::Null, Value::Null),
        ];
        for command in BUILD_MODEL_COMMANDS {
            let plan = route(command, &json!({}), &served(&[command]));
            assert_eq!(plan.shape, Shape::BuildModel, "{command}");
            for (gradle, expected) in &cases {
                let answer = json!({ "gradle": gradle, "maven": null });
                assert_eq!(shape_response(plan.shape, answer, None), json!({ "gradle": expected, "maven": null }));
            }
        }
    }

    // remote client settings › ends remote routing before a successful local open is published, even at the same
    // path
    #[test]
    fn ends_remote_routing_before_a_successful_local_open_is_published_even_at_the_same_path() {
        let mut window = remote_window();
        let open = json!({ "path": "/remote" });
        let plan = window.route("open_project", &open).expect("the window is remote");
        assert_eq!((plan.local.as_slice(), &plan.remote), ([Call::new("open_project", &open)].as_slice(), &None));
        window.finished(&plan, true);
        assert!(!window.is_remote());
        // The settings are read here from now on.
        assert_eq!(window.route("settings_get", &json!({})), None);
    }

    // remote client settings › preserves remote routing when opening a local project fails
    #[test]
    fn preserves_remote_routing_when_opening_a_local_project_fails() {
        let mut window = remote_window();
        let plan = window.route("open_project", &json!({ "path": "/missing" })).expect("the window is remote");
        window.finished(&plan, false);
        assert!(window.is_remote());
        let reload = window.route("settings_get", &json!({})).expect("the window is remote");
        assert_eq!(shape_response(reload.shape, remote(), Some(&local()))["editor"]["tabSize"], 2);
    }

    // remote client settings › keeps %s writes local in either scope
    #[test]
    fn keeps_client_settings_writes_local_in_either_scope() {
        let served = served(&SETTINGS_COMMANDS);
        let nested = ["customKeymaps.mine", "keymapOverrides.editor.copy", "appearance.theme"];
        for key in CLIENT_SETTINGS_KEYS.iter().chain(&nested) {
            for scope in ["global", "project"] {
                let args = json!({ "scope": scope, "key": key, "value": "null" });
                assert_eq!(remote_routing("settings_set", &args, &served), Destination::Local, "{key}");
            }
        }
    }

    // remote client settings › preserves routing for host setting %s
    #[test]
    fn preserves_routing_for_host_settings() {
        let served = served(&SETTINGS_COMMANDS);
        for key in [
            "editor.tabSize",
            "editor.formatOnSave",
            "editor.codeStyle.languages",
            "terminal.shellPath",
            "terminal.env",
            "tools.node",
            "languages.python.interpreter",
            "keymapExtra",
        ] {
            let routing = |scope: &str| remote_routing("settings_set", &json!({ "scope": scope, "key": key }), &served);
            assert_eq!((routing("project"), routing("global")), (Destination::Remote, Destination::Both), "{key}");
        }
    }

    // remote client settings › merges settings before publishing a newly connected project
    #[test]
    fn merges_settings_before_publishing_a_newly_connected_project() {
        let connected = json!({ "root": "/remote", "name": "remote", "settings": remote() });
        let project = shape_response(Shape::Project, connected, Some(&local()));
        assert_eq!((&project["root"], &project["settings"]["keymap"]), (&json!("/remote"), &json!("my-map")));
        assert_eq!(font_and_tab_size(&project["settings"]), (Some(19), Some(2)));
    }

    // remote client settings › overlays the project snapshot used by the settings window
    #[test]
    fn overlays_the_project_snapshot_used_by_the_settings_window() {
        let plan = remote_window().route("project_current", &json!({})).expect("the window is remote");
        assert_eq!((plan.remote.is_some(), plan.shape), (true, Shape::Project));
        let snapshot = json!({ "root": "/remote", "settings": remote() });
        let shaped = shape_response(plan.shape, snapshot, Some(&local()));
        assert_eq!(shaped["settings"]["keymap"], "my-map");
        assert_eq!(font_and_tab_size(&shaped["settings"]), (Some(19), Some(2)));
        // A window without a project has no snapshot to shape.
        assert_eq!(shape_response(Shape::Project, Value::Null, Some(&local())), Value::Null);
    }

    // remote client settings › keeps client preferences after a remote tooling write
    #[test]
    fn keeps_client_preferences_after_a_remote_tooling_write() {
        let write = json!({ "scope": "project", "key": "editor.tabSize", "value": "2" });
        let plan = remote_window().route("settings_set", &write).expect("the window is remote");
        // Nothing is written here.
        assert!(plan.local.is_empty());
        assert_eq!(plan.remote, remote_call("settings_set", write));
        let settings = shape_response(plan.shape, remote(), Some(&local()));
        assert_eq!(settings["keymap"], "my-map");
        assert_eq!(font_and_tab_size(&settings), (Some(19), Some(2)));
    }

    // remote client settings › keeps local overrides through a settingsChanged runtime reload
    #[test]
    fn keeps_local_overrides_through_a_settings_changed_runtime_reload() {
        let reload = remote_window().route("settings_get", &json!({})).expect("the window is remote");
        assert!(reload.shape.needs_client_settings());
        let settings = shape_response(reload.shape, remote(), Some(&local()));
        assert_eq!(settings["keymapOverrides"], local()["keymapOverrides"]);
        assert_eq!(settings["keymap"], "my-map");
    }

    // remote client settings › reloads local preferences instead of retaining stale or remote overrides
    #[test]
    fn reloads_local_preferences_instead_of_retaining_stale_or_remote_overrides() {
        let mut cleared = local();
        cleared["keymapOverrides"] = json!({});
        let settings = shape_response(Shape::Settings, remote(), Some(&cleared));
        assert_eq!(settings["keymapOverrides"], json!({}));
    }

    // remote client settings › does not inherit remote UI values when a local field is absent
    #[test]
    fn does_not_inherit_remote_ui_values_when_a_local_field_is_absent() {
        let settings = shape_response(Shape::Settings, remote(), Some(&json!({})));
        assert_eq!(settings.get("keymap"), None);
        assert_eq!(settings["editor"]["tabSize"], 2);
    }

    // remote client settings › leaves local windows unchanged
    #[test]
    fn leaves_local_windows_unchanged() {
        let mut window = remote_window();
        window.disable();
        assert_eq!(window.route("settings_get", &json!({})), None);
        assert_eq!(window.host_os("macos"), "macos");
    }

    // remote client settings › preserves client keymaps and UI settings when loading remote settings
    #[test]
    fn preserves_client_keymaps_and_ui_settings_when_loading_remote_settings() {
        let settings = shape_response(Shape::Settings, remote(), Some(&local()));
        for client_owned in ["keymap", "keymapOverrides", "customKeymaps", "appearance"] {
            assert_eq!(settings[client_owned], local()[client_owned], "{client_owned}");
        }
        assert_eq!(font_and_tab_size(&settings), (Some(19), Some(2)));
        assert_eq!(settings["terminal"], json!({ "fontSize": 18, "shellPath": "/remote" }));
    }

    // remote client settings › shows client preferences in the global settings dialog snapshot
    #[test]
    fn shows_client_preferences_in_the_global_settings_dialog_snapshot() {
        let plan = remote_window().route("settings_get_global", &json!({})).expect("the window is remote");
        // The snapshot is this machine's own: read here, shown as it is.
        assert_eq!(plan.local, [Call::new("settings_get_global", &json!({}))]);
        assert_eq!((plan.remote, plan.shape), (None, Shape::AsIs));
    }

    // remote client settings › writes a client preference only to local global settings and returns merged settings
    #[test]
    fn writes_a_client_preference_only_to_local_global_settings_and_returns_merged_settings() {
        let write = json!({ "scope": "project", "key": "editor.fontSize", "value": "19" });
        let plan = remote_window().route("settings_set", &write).expect("the window is remote");
        let global = json!({ "scope": "global", "key": "editor.fontSize", "value": "19" });
        assert_eq!(plan.local, [Call::new("settings_set", &global)]);
        // The host is only asked for its settings again.
        assert_eq!(plan.remote, remote_call("settings_get", json!({})));
        let settings = shape_response(plan.shape, remote(), Some(&local()));
        assert_eq!(settings["keymap"], "my-map");
        assert_eq!(font_and_tab_size(&settings), (Some(19), Some(2)));
    }

    // Not from a spec: locations name a target and a path and read back, and a window knows its
    // project's location and the OS its commands run on.
    #[test]
    fn writes_and_reads_remote_locations() {
        let ssh = RemoteTarget::Ssh { config: "build box".to_owned() };
        assert_eq!(remote_location(&ssh, "/srv/app"), "ssh://build%20box/srv/app");
        assert_eq!(parse_remote_location("ssh://build%20box/srv/app"), Some((ssh.clone(), "/srv/app".to_owned())));
        let wsl = RemoteTarget::Wsl { distro: "Ubuntu".to_owned() };
        assert_eq!(remote_location(&wsl, "home/me"), "wsl://Ubuntu/home/me");
        let container = RemoteTarget::DevContainer { folder: "/work/app".to_owned(), config: "api:dev".to_owned() };
        let location = remote_location(&container, "/workspaces/app");
        assert_eq!(location, "devcontainer://%2Fwork%2Fapp:api%3Adev/workspaces/app");
        assert_eq!(parse_remote_location(&location), Some((container.clone(), "/workspaces/app".to_owned())));
        let default_config = RemoteTarget::DevContainer { folder: "/work/app".to_owned(), config: String::new() };
        assert_eq!(remote_location(&default_config, "/w"), "devcontainer://%2Fwork%2Fapp/w");
        assert!(!same_target(&container, &default_config) && same_target(&ssh, &ssh.clone()));
        assert_eq!(target_name(&container), "/work/app");
        for local in ["/srv/app", "ssh:///srv", "ssh://host", "ssh://bad%zz/srv"] {
            assert_eq!(parse_remote_location(local), None, "{local}");
        }

        let window = remote_window();
        assert_eq!(window.project_location(Some("/remote")).as_deref(), Some("ssh://test/remote"));
        assert_eq!(window.project_location(Some("/elsewhere")).as_deref(), Some("/elsewhere"));
        assert_eq!(window.project_location(None), None);
        assert_eq!(window.host_os("macos"), "linux");
        assert_eq!(window.session().map(|session| session.host.as_str()), Some("test"));
    }
}
