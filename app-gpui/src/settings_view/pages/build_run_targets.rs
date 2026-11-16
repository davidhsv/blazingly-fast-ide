//! Build, Execution, Deployment › Run Targets: the SSH hosts and Docker / Compose containers run
//! configurations can run on, and the default target. The list has + / − / copy / move up / move
//! down, with the targets plugins register shown read-only above the user's own; the target
//! selected is edited below it ([`form`]), its project folder on an SSH target chosen in a folder
//! browser over SFTP ([`folder`]).
//!
//! The targets (`runTargets`) and the default target (`run.defaultTarget`) are settings of the
//! scope the page edits. The SSH configurations offered are the global `ssh.configs`, whatever the
//! scope. The Docker connections are the Services tool window's (`docker.json` in the app's data
//! folder), read once; no Docker program or daemon is reached for them.
//!
//! What needs the backend goes through seams a test can replace ([`sources`]): the folder
//! browser's listing, host key trust and password ([`RemoteFolders`]), and the Docker connections
//! ([`DockerSource`]). A test build has neither unless a test sets it: the browser's listing opens
//! an SSH connection, which reads the user's `~/.ssh` files and reaches the network.

mod folder;
mod form;
#[cfg(test)]
mod proofs30_run_targets_view;
mod sources;

pub use folder::FolderState;
pub use sources::{
    DockerChoice, DockerSource, FolderLister, HostTrust, PluginTarget, RemoteFolders, RunTargetDocker,
    RunTargetFolders, SecretSetter, backend_docker, backend_folders, docker_source, folders, load_sources,
};

use super::{Binding, ControlEvent, new_field, strings_of};
use crate::platform_more::remote_settings::{normalized_run_targets, with_unique_ids};
use crate::settings_model::deployment_model::RemoteIds;
use crate::settings_model::run_targets::split_list;
use crate::settings_model::staging::Scope;
use crate::settings_view::controls::tx;
use crate::theme;
use crate::ui::text_field::{TextField, TextFieldEvent};
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Entity, EventEmitter, IntoElement, MouseButton, Render, SharedString,
    Subscription, Window, div, prelude::*, px,
};
use ide_core::settings::{RunTarget, RunTargetKind, SshConfiguration};
use serde_json::Value;

pub const PAGE: &str = "build.runTargets";
/// The slot the editor fills.
pub const SLOT: &str = "Run Targets";
const KEY: &str = "runTargets";
const DEFAULT_KEY: &str = "run.defaultTarget";
const SSH_KEY: &str = "ssh.configs";
const EMPTY: &str = "No run targets. Click + to add one.";
const NOTE_TARGETS: &str = "Choose a target under “Run on” in a run configuration. Interpreters left empty are \
                            looked up on the target's PATH. SSH configurations are managed in Tools › SSH \
                            Configurations.";
const NOTE_DOCKER: &str = "Docker connections come from the Services tool window. Each run gets a new container, \
                           removed when it ends. Python configurations can be debugged there; coverage and \
                           profiling stay on this machine.";

/// The kinds of target: (value, name in the settings, label).
pub const KINDS: [(RunTargetKind, &str, &str); 3] = [
    (RunTargetKind::Ssh, "ssh", "SSH"),
    (RunTargetKind::Docker, "docker", "Docker"),
    (RunTargetKind::Compose, "compose", "Docker Compose"),
];

/// The text fields of a target: (field, label, placeholder). `projectRoot` and `containerRoot` take
/// their placeholder from the project's name ([`RunTargetsView::placeholder_of`]).
const FIELDS: [(&str, &str, Option<&str>); 11] = [
    ("name", "Name", None),
    ("projectRoot", "Project folder on target", None),
    ("composeFiles", "Compose files", Some("compose.yaml")),
    ("service", "Service", None),
    ("image", "Image", Some("python:3.12")),
    ("containerRoot", "Container folder", None),
    ("ports", "Published ports", Some("8000:8000")),
    ("runOptions", "Run options", Some("--network host")),
    ("python", "Python interpreter", Some("python3")),
    ("node", "Node.js interpreter", Some("node")),
    ("java", "Java executable", Some("java")),
];

/// The fields typed as lists of words, stored as lists.
const LIST_FIELDS: [&str; 2] = ["composeFiles", "ports"];

// ---------------------------------------------------------------------------
// The targets

/// A target of the list shown, read field by field: a field missing or of the wrong type takes
/// its default (upload before each run: on), an unknown kind is SSH, and a list keeps only its
/// words. A malformed item thus never hides the others, and staging the list keeps every target.
fn read_target(item: &Value) -> RunTarget {
    let text = |key: &str| item.get(key).and_then(Value::as_str).unwrap_or_default().to_owned();
    let flag = |key: &str, default: bool| item.get(key).and_then(Value::as_bool).unwrap_or(default);
    let kind = match item.get("kind").and_then(Value::as_str) {
        Some("docker") => RunTargetKind::Docker,
        Some("compose") => RunTargetKind::Compose,
        _ => RunTargetKind::Ssh,
    };
    RunTarget {
        kind,
        id: text("id"),
        name: text("name"),
        ssh_config: text("sshConfig"),
        project_root: text("projectRoot"),
        sync_before_run: flag("syncBeforeRun", true),
        python: text("python"),
        node: text("node"),
        java: text("java"),
        docker_connection: text("dockerConnection"),
        image: text("image"),
        compose_files: strings_of(item.get("composeFiles")),
        service: text("service"),
        container_root: text("containerRoot"),
        ports: strings_of(item.get("ports")),
        run_options: text("runOptions"),
        image_user: flag("imageUser", false),
    }
}

/// The targets as the page shows them (staged, else stored, in the scope it edits), each under an
/// id of its own; a value that is not a list reads as none.
pub fn targets(binding: &Binding, cx: &App) -> Vec<RunTarget> {
    let value = binding.value(KEY, cx);
    let read: Vec<RunTarget> = value.iter().filter_map(Value::as_array).flatten().map(read_target).collect();
    normalized_run_targets(&read)
}

fn stage_targets(binding: &Binding, targets: &[RunTarget], cx: &mut App) {
    match serde_json::to_value(targets) {
        Ok(value) => binding.stage(KEY, value, cx),
        Err(error) => tracing::warn!("Settings: the run targets could not be staged: {error}"),
    }
}

/// The global SSH configurations, staged else stored, each under an id of its own; an item that is
/// not a configuration is left out.
pub fn ssh_configs(binding: &Binding, cx: &App) -> Vec<SshConfiguration> {
    let value = binding.model.read(cx).staged_value(Scope::Global, SSH_KEY);
    let items = value.iter().filter_map(Value::as_array).flatten();
    let read: Vec<SshConfiguration> =
        items.filter_map(|item| serde_json::from_value::<SshConfiguration>(item.clone()).ok()).collect();
    with_unique_ids(&read, "ssh")
}

/// A new target (`Target <n>`, on the first SSH configuration, uploading before each run), or a copy
/// of `base` named after it; either with the fresh `id`.
pub fn new_target(targets: &[RunTarget], base: Option<&RunTarget>, first_ssh: Option<&str>, id: String) -> RunTarget {
    match base {
        Some(base) => RunTarget { id, name: format!("{} (copy)", base.name), ..base.clone() },
        None => RunTarget {
            id,
            name: format!("Target {}", targets.len() + 1),
            ssh_config: first_ssh.unwrap_or_default().to_owned(),
            sync_before_run: true,
            ..RunTarget::default()
        },
    }
}

pub fn kind_name(kind: RunTargetKind) -> &'static str {
    KINDS.iter().find(|(own, _, _)| *own == kind).map_or("ssh", |(_, name, _)| *name)
}

pub fn kind_label(kind: RunTargetKind) -> &'static str {
    KINDS.iter().find(|(own, _, _)| *own == kind).map_or("SSH", |(_, _, label)| *label)
}

fn kind_of(name: &str) -> RunTargetKind {
    KINDS.iter().find(|(_, own, _)| *own == name).map_or(RunTargetKind::Ssh, |(kind, _, _)| *kind)
}

/// The image or service of a container target, for the list.
pub fn container_summary(target: &RunTarget) -> String {
    match target.kind {
        RunTargetKind::Docker => target.image.clone(),
        RunTargetKind::Compose if target.service.is_empty() => "compose".to_owned(),
        RunTargetKind::Compose => format!("compose:{}", target.service),
        RunTargetKind::Ssh => String::new(),
    }
}

/// The text a field of `target` shows; a list as its words separated by spaces.
fn text_of(target: &RunTarget, name: &str) -> String {
    match name {
        "name" => target.name.clone(),
        "projectRoot" => target.project_root.clone(),
        "composeFiles" => target.compose_files.join(" "),
        "service" => target.service.clone(),
        "image" => target.image.clone(),
        "containerRoot" => target.container_root.clone(),
        "ports" => target.ports.join(" "),
        "runOptions" => target.run_options.clone(),
        "python" => target.python.clone(),
        "node" => target.node.clone(),
        "java" => target.java.clone(),
        _ => String::new(),
    }
}

/// Stores `text` typed in field `name` of `target`; a list field stores its words.
fn set_text(target: &mut RunTarget, name: &str, text: &str) {
    let owned = text.to_owned();
    match name {
        "name" => target.name = owned,
        "projectRoot" => target.project_root = owned,
        "composeFiles" => target.compose_files = split_list(text),
        "service" => target.service = owned,
        "image" => target.image = owned,
        "containerRoot" => target.container_root = owned,
        "ports" => target.ports = split_list(text),
        "runOptions" => target.run_options = owned,
        "python" => target.python = owned,
        "node" => target.node = owned,
        "java" => target.java = owned,
        _ => {}
    }
}

/// The drop-downs of the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Default,
    Kind,
    Ssh,
    Docker,
}

impl Choice {
    /// The name its selectors are drawn under.
    pub fn key(self) -> &'static str {
        match self {
            Self::Default => DEFAULT_KEY,
            Self::Kind => "target.kind",
            Self::Ssh => "target.sshConfig",
            Self::Docker => "target.dockerConnection",
        }
    }
}

/// One choice of a drop-down.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pick {
    pub value: String,
    pub label: String,
    /// Muted after the label in the open list (what an SSH configuration connects to).
    pub detail: Option<String>,
    pub enabled: bool,
}

impl Pick {
    fn new(value: impl Into<String>, label: impl Into<String>) -> Self {
        Self { value: value.into(), label: label.into(), detail: None, enabled: true }
    }
}

pub struct RunTargetsView {
    binding: Binding,
    folders: Option<RemoteFolders>,
    /// The project's name, for the folder placeholders.
    project: String,
    docker: Vec<DockerChoice>,
    plugins: Vec<PluginTarget>,
    ids: RemoteIds,
    selected: Option<String>,
    /// The target the fields show.
    shown: Option<String>,
    fields: Vec<(&'static str, Entity<TextField>)>,
    open: Option<Choice>,
    folder: Option<folder::FolderBrowser>,
    /// Whether the dialog shows this page.
    page_shown: bool,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ControlEvent> for RunTargetsView {}

impl RunTargetsView {
    pub fn new(binding: Binding, folders: Option<RemoteFolders>, cx: &mut Context<Self>) -> Self {
        let mut subscriptions = Vec::new();
        let mut fields = Vec::new();
        for (name, _, placeholder) in FIELDS {
            let field = new_field("", placeholder.map(|placeholder| tx(cx, placeholder)), cx);
            subscriptions.push(cx.subscribe(&field, move |view: &mut Self, field, event: &TextFieldEvent, cx| {
                match event {
                    TextFieldEvent::Changed => {
                        let text = field.read(cx).text().to_owned();
                        view.update_current(cx, |target| set_text(target, name, &text));
                    }
                    TextFieldEvent::Submitted => cx.emit(ControlEvent::Submit),
                    TextFieldEvent::Cancelled => view.escape(cx),
                }
            }));
            fields.push((name, field));
        }
        subscriptions.push(cx.observe(&binding.model, |view: &mut Self, _, cx| view.sync(cx)));
        let selected = targets(&binding, cx).into_iter().next().map(|target| target.id);
        let mut view = Self {
            binding,
            folders,
            project: "project".to_owned(),
            docker: Vec::new(),
            plugins: Vec::new(),
            ids: RemoteIds::default(),
            selected,
            shown: None,
            fields,
            open: None,
            folder: None,
            page_shown: false,
            _subscriptions: subscriptions,
        };
        view.sync(cx);
        view
    }

    pub fn targets(&self, cx: &App) -> Vec<RunTarget> {
        targets(&self.binding, cx)
    }

    /// The target selected; none when the selection is gone from the list.
    pub fn current(&self, cx: &App) -> Option<RunTarget> {
        let id = self.selected.as_deref()?;
        self.targets(cx).into_iter().find(|target| target.id == id)
    }

    fn index(&self, cx: &App) -> Option<usize> {
        let id = self.selected.as_deref()?;
        self.targets(cx).iter().position(|target| target.id == id)
    }

    /// The text field of `name` (`name`, `projectRoot`, `composeFiles`, `service`, `image`,
    /// `containerRoot`, `ports`, `runOptions`, `python`, `node`, `java`).
    pub fn field(&self, name: &str) -> Option<&Entity<TextField>> {
        self.fields.iter().find(|(own, _)| *own == name).map(|(_, field)| field)
    }

    /// The placeholder field `name` shows while it is empty.
    pub fn placeholder(&self, name: &str, cx: &App) -> String {
        self.field(name).map(|field| field.read(cx).placeholder().to_owned()).unwrap_or_default()
    }

    /// The drop-down whose list is open.
    pub fn open_choice(&self) -> Option<Choice> {
        self.open
    }

    pub fn set_project_name(&mut self, name: String, cx: &mut Context<Self>) {
        if !name.is_empty() {
            self.project = name;
        }
        self.sync(cx);
    }

    pub fn set_plugin_targets(&mut self, targets: Vec<PluginTarget>, cx: &mut Context<Self>) {
        self.plugins = targets;
        cx.notify();
    }

    pub fn set_docker_connections(&mut self, connections: Vec<DockerChoice>, cx: &mut Context<Self>) {
        self.docker = connections;
        cx.notify();
    }

    /// What the placeholders of the folder fields say for the kind of target shown.
    fn placeholder_of(&self, name: &str, kind: RunTargetKind) -> Option<String> {
        match name {
            "projectRoot" => Some(format!("~/{}", self.project)),
            "containerRoot" if kind == RunTargetKind::Compose => Some(String::new()),
            "containerRoot" => Some(format!("/workspaces/{}", self.project)),
            _ => None,
        }
    }

    /// Changes the target selected.
    fn update_current(&mut self, cx: &mut Context<Self>, change: impl FnOnce(&mut RunTarget)) {
        let Some(id) = self.selected.clone() else { return };
        let mut all = self.targets(cx);
        let Some(target) = all.iter_mut().find(|target| target.id == id) else { return };
        change(target);
        stage_targets(&self.binding, &all, cx);
    }

    pub fn select(&mut self, id: Option<String>, cx: &mut Context<Self>) {
        self.selected = id;
        self.open = None;
        self.sync(cx);
    }

    /// Add (+): a new target, or a copy of the selected one, added at the end and selected.
    pub fn add(&mut self, copy: bool, cx: &mut Context<Self>) {
        let mut all = self.targets(cx);
        let base = if copy { self.current(cx) } else { None };
        let first_ssh = ssh_configs(&self.binding, cx).into_iter().next().map(|config| config.id);
        let id = self.ids.next("target", crate::platform::local_time::now_ms() as u64);
        let target = new_target(&all, base.as_ref(), first_ssh.as_deref(), id);
        let id = target.id.clone();
        all.push(target);
        stage_targets(&self.binding, &all, cx);
        self.select(Some(id), cx);
    }

    /// Remove (−): the selected target goes; the one now in its place (else the last) is selected.
    pub fn remove(&mut self, cx: &mut Context<Self>) {
        let Some(at) = self.index(cx) else { return };
        let mut all = self.targets(cx);
        all.remove(at);
        stage_targets(&self.binding, &all, cx);
        let next = all.get(at.min(all.len().saturating_sub(1))).map(|target| target.id.clone());
        self.select(next, cx);
    }

    /// Moves the selected target up (`-1`) or down (`1`).
    pub fn move_by(&mut self, by: isize, cx: &mut Context<Self>) {
        let Some(at) = self.index(cx) else { return };
        let mut all = self.targets(cx);
        let Some(to) = at.checked_add_signed(by).filter(|to| *to < all.len()) else { return };
        all.swap(at, to);
        stage_targets(&self.binding, &all, cx);
    }

    /// A choice made in drop-down `which`.
    pub fn choose(&mut self, which: Choice, value: String, cx: &mut Context<Self>) {
        self.open = None;
        match which {
            Choice::Default => self.binding.stage(DEFAULT_KEY, Value::String(value), cx),
            Choice::Kind => {
                let kind = kind_of(&value);
                self.update_current(cx, |target| target.kind = kind);
            }
            Choice::Ssh => self.update_current(cx, |target| target.ssh_config = value),
            Choice::Docker => self.update_current(cx, |target| target.docker_connection = value),
        }
        cx.notify();
    }

    /// Flips checkbox `name` (`syncBeforeRun`, `imageUser`) of the target selected.
    pub fn flip(&mut self, name: &'static str, cx: &mut Context<Self>) {
        self.update_current(cx, |target| match name {
            "syncBeforeRun" => target.sync_before_run = !target.sync_before_run,
            "imageUser" => target.image_user = !target.image_user,
            _ => {}
        });
    }

    /// Escape in a field closes the open list first, else is the dialog's Cancel.
    fn escape(&mut self, cx: &mut Context<Self>) {
        if self.open.take().is_some() {
            cx.notify();
        } else {
            cx.emit(ControlEvent::Cancel);
        }
    }

    /// The fields show the target selected: a field already showing its value is left alone, and a
    /// list field keeps what is typed while it spells the list staged. Another target closes the
    /// folder browser.
    fn sync(&mut self, cx: &mut Context<Self>) {
        // The page opens each time the dialog shows it, as a page of its own: the first target
        // selected, no list or folder browser open; leaving it closes the browser and drops the
        // password typed there.
        let shown = self.binding.model.read(cx).page_id == self.binding.page;
        if shown != self.page_shown {
            self.page_shown = shown;
            self.folder = None;
            self.open = None;
            if shown {
                self.selected = self.targets(cx).into_iter().next().map(|target| target.id);
            }
        }
        let current = self.current(cx);
        let id = current.as_ref().map(|target| target.id.clone());
        if id != self.shown {
            self.shown = id;
            self.folder = None;
            self.open = None;
        }
        let kind = current.as_ref().map_or(RunTargetKind::Ssh, |target| target.kind);
        let mut updates: Vec<(Entity<TextField>, Option<String>, Option<String>)> = Vec::new();
        for (name, field) in &self.fields {
            let read = field.read(cx);
            let shown = read.text().to_owned();
            let wanted = current.as_ref().map(|target| text_of(target, name)).unwrap_or_default();
            let spells = LIST_FIELDS.contains(name)
                && current.as_ref().is_some_and(|target| split_list(&shown) == split_list(&text_of(target, name)));
            let text = (shown != wanted && !spells).then_some(wanted);
            let placeholder = self.placeholder_of(name, kind).filter(|wanted| wanted != read.placeholder());
            if text.is_some() || placeholder.is_some() {
                updates.push((field.clone(), text, placeholder));
            }
        }
        for (field, text, placeholder) in updates {
            field.update(cx, |field, cx| {
                if let Some(text) = text {
                    field.set_text(&text, cx);
                }
                if let Some(placeholder) = placeholder {
                    field.set_placeholder(placeholder, cx);
                }
            });
        }
        cx.notify();
    }

    /// What the list shows beside a target: its SSH configuration's name, else its image or service.
    fn summary(&self, target: &RunTarget, configs: &[SshConfiguration]) -> String {
        match target.kind {
            RunTargetKind::Ssh if target.ssh_config.is_empty() => String::new(),
            RunTargetKind::Ssh => {
                let found = configs.iter().find(|config| config.id == target.ssh_config);
                found.map_or_else(
                    || target.ssh_config.clone(),
                    |config| if config.name.is_empty() { config.host.clone() } else { config.name.clone() },
                )
            }
            _ => container_summary(target),
        }
    }

    /// The choices of drop-down `which`, with the value it holds now.
    pub fn picks(&self, which: Choice, cx: &App) -> (Vec<Pick>, String) {
        let current = self.current(cx);
        match which {
            Choice::Default => {
                let value = self.binding.text(DEFAULT_KEY, cx);
                let mut picks = vec![Pick::new("", tx(cx, "Local machine"))];
                picks.extend(self.targets(cx).into_iter().map(|target| Pick::new(target.id, target.name)));
                picks
                    .extend(self.plugins.iter().map(|t| Pick::new(t.id.clone(), format!("{} ({})", t.name, t.plugin))));
                if !value.is_empty() && !picks.iter().any(|pick| pick.value == value) {
                    picks.push(Pick::new(value.clone(), tx(cx, &format!("{value} (missing)"))));
                }
                (picks, value)
            }
            Choice::Kind => {
                let value = current.map_or("ssh", |target| kind_name(target.kind)).to_owned();
                (KINDS.iter().map(|(_, name, label)| Pick::new(*name, tx(cx, label))).collect(), value)
            }
            Choice::Ssh => {
                let value = current.map(|target| target.ssh_config).unwrap_or_default();
                let configs = ssh_configs(&self.binding, cx);
                let mut picks: Vec<Pick> = configs
                    .iter()
                    .map(|config| {
                        let name = config.name.trim();
                        let label = if name.is_empty() { config.label() } else { name.to_owned() };
                        Pick { detail: Some(config.label()), ..Pick::new(config.id.clone(), label) }
                    })
                    .collect();
                if !value.is_empty() && !configs.iter().any(|config| config.id == value) {
                    picks.push(Pick::new(value.clone(), tx(cx, &format!("{value} (missing)"))));
                }
                if picks.is_empty() {
                    picks.push(Pick { enabled: false, ..Pick::new("", tx(cx, "No SSH configurations")) });
                }
                (picks, value)
            }
            Choice::Docker => {
                let value = current.map(|target| target.docker_connection).unwrap_or_default();
                let mut picks = vec![Pick::new("", tx(cx, "Docker CLI default"))];
                picks.extend(self.docker.iter().map(|(id, name)| Pick::new(id.clone(), name.clone())));
                let known = self.docker.iter().any(|(id, _)| *id == value);
                if !value.is_empty() && !self.docker.is_empty() && !known {
                    picks.push(Pick::new(value.clone(), tx(cx, &format!("{value} (missing)"))));
                }
                (picks, value)
            }
        }
    }

    /// The label drop-down `which` shows: the choice of the value it holds, else nothing.
    fn shown_choice(&self, which: Choice, cx: &App) -> String {
        let (picks, value) = self.picks(which, cx);
        picks.into_iter().find(|pick| pick.value == value).map(|pick| pick.label).unwrap_or_default()
    }

    /// Browse… can be used: the target has an SSH configuration and the browser a backend.
    pub fn can_browse(&self, cx: &App) -> bool {
        self.folders.is_some() && self.current(cx).is_some_and(|target| !target.ssh_config.is_empty())
    }

    /// The editor as text: the plugins' targets (`{name} | {plugin} · Read-only`), the user's
    /// (`>` the selected one: name | SSH configuration, image or service), the default target, the
    /// fields of the target selected ([`Self::form_lines`]), the folder browser when it is open, and
    /// the note.
    pub fn lines(&self, cx: &App) -> Vec<String> {
        let all = self.targets(cx);
        let configs = ssh_configs(&self.binding, cx);
        let read_only = tx(cx, "Read-only");
        let mut out: Vec<String> =
            self.plugins.iter().map(|target| format!("  {} | {} · {read_only}", target.name, target.plugin)).collect();
        out.extend(all.iter().map(|target| {
            let mark = if self.selected.as_deref() == Some(target.id.as_str()) { ">" } else { " " };
            let name = if target.name.is_empty() { &target.id } else { &target.name };
            format!("{mark} {name} | {}", self.summary(target, &configs))
        }));
        if all.is_empty() && self.plugins.is_empty() {
            out.push(format!("  {}", tx(cx, EMPTY)));
        }
        out.push(format!("{}: {} ▾", tx(cx, "Default run target"), self.shown_choice(Choice::Default, cx)));
        if let Some(target) = self.current(cx) {
            out.extend(self.form_lines(&target, cx));
        }
        out.push(format!("{} {}", tx(cx, NOTE_TARGETS), tx(cx, NOTE_DOCKER)));
        out
    }

    fn toolbar(&self, cx: &mut Context<Self>) -> gpui_kit::Div {
        let len = self.targets(cx).len();
        let at = self.index(cx);
        let has = at.is_some();
        let up = at.is_some_and(|at| at > 0);
        let down = at.is_some_and(|at| at + 1 < len);
        let add = tool(cx, "add", "plus", true, "Add")
            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.add(false, cx)));
        let remove = tool(cx, "remove", "minus", has, "Remove")
            .when(has, |el| el.on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.remove(cx))));
        let copy = tool(cx, "copy", "copy", has, "Copy")
            .when(has, |el| el.on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.add(true, cx))));
        let move_up = tool(cx, "up", "arrow-up", up, "Move Up")
            .when(up, |el| el.on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.move_by(-1, cx))));
        let move_down = tool(cx, "down", "arrow-down", down, "Move Down")
            .when(down, |el| el.on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.move_by(1, cx))));
        div()
            .flex()
            .gap(px(2.0))
            .p(px(2.0))
            .border_b_1()
            .border_color(theme::color(cx, "border"))
            .child(add)
            .child(remove)
            .child(copy)
            .child(move_up)
            .child(move_down)
    }

    fn target_list(&self, cx: &mut Context<Self>) -> gpui_kit::Div {
        let all = self.targets(cx);
        let configs = ssh_configs(&self.binding, cx);
        let selected_bg = theme::color(cx, "bg-selected");
        let read_only = tx(cx, "Read-only");
        let muted = |cx: &App, text: String| super::muted(cx, &text).whitespace_nowrap();
        let plugin_rows: Vec<AnyElement> = self
            .plugins
            .iter()
            .map(|target| {
                list_row()
                    .child(name_cell(&target.name))
                    .child(muted(cx, format!("{} · {read_only}", target.plugin)))
                    .into_any_element()
            })
            .collect();
        let rows: Vec<AnyElement> = all
            .iter()
            .enumerate()
            .map(|(index, target)| {
                let id = target.id.clone();
                let selector = format!("settings-target-{index}");
                let selected = self.selected.as_deref() == Some(target.id.as_str());
                let name = if target.name.is_empty() { target.id.clone() } else { target.name.clone() };
                list_row()
                    .id(SharedString::from(selector.clone()))
                    .debug_selector(move || selector)
                    .when(selected, |el| el.bg(selected_bg))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |view, _, _, cx| view.select(Some(id.clone()), cx)),
                    )
                    .child(name_cell(&name))
                    .child(muted(cx, self.summary(target, &configs)))
                    .into_any_element()
            })
            .collect();
        let empty = (all.is_empty() && self.plugins.is_empty()).then(|| {
            let text = tx(cx, EMPTY);
            div().px(px(6.0)).py(px(4.0)).text_color(theme::color(cx, "fg-subtle")).child(SharedString::from(text))
        });
        div()
            .max_w(px(560.0))
            .flex()
            .flex_col()
            .rounded(theme::length(cx, "radius-small"))
            .border_1()
            .border_color(theme::color(cx, "border"))
            .child(self.toolbar(cx))
            .child(div().min_h(px(96.0)).flex().flex_col().children(plugin_rows).children(rows).children(empty))
    }
}

/// A row of the list.
fn list_row() -> gpui_kit::Div {
    div().h(px(super::ROW_HEIGHT - 4.0)).px(px(6.0)).flex().items_center().gap(px(8.0))
}

fn name_cell(name: &str) -> gpui_kit::Div {
    div().flex_1().min_w_0().whitespace_nowrap().child(SharedString::from(name.to_owned()))
}

/// An icon button of the list's toolbar, drawn as `settings-targets-<name>`, faint while it cannot
/// be used.
fn tool(cx: &App, name: &str, icon: &str, enabled: bool, title: &'static str) -> gpui_kit::Stateful<gpui_kit::Div> {
    let selector = format!("settings-targets-{name}");
    let color = theme::color(cx, if enabled { "fg-icon" } else { "fg-faint" });
    div()
        .id(SharedString::from(selector.clone()))
        .debug_selector(move || selector)
        .size(px(22.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(theme::length(cx, "radius-small"))
        .when(enabled, |el| el.hover(|el| el.bg(theme::color(cx, "bg-toolbar-hover"))))
        .tooltip(crate::ui::tooltip::tooltip(title))
        .child(theme::icon(cx, icon, 16.0, color))
}

impl Render for RunTargetsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.binding.query(cx);
        let list = self.target_list(cx);
        let default = self.default_section(&query, cx);
        let form = self.current(cx).map(|target| self.form(&target, &query, cx));
        let note = format!("{} {}", tx(cx, NOTE_TARGETS), tx(cx, NOTE_DOCKER));
        div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            // A press anywhere else in the editor closes the open list.
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, _, _, cx| {
                    if view.open.take().is_some() {
                        cx.notify();
                    }
                }),
            )
            .child(list)
            .child(default)
            .children(form)
            .child(div().pt(px(6.0)).text_color(theme::color(cx, "fg-subtle")).child(SharedString::from(note)))
    }
}
