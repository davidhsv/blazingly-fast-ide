//! Run with Profiler: which configurations a profiler is known for, the snapshots of this project
//! window (the last few stay open as tabs), and what a view of one needs: the flame graph's
//! layout, frame search, weights as text and frame locations. Also the notifications of a run that
//! cannot be profiled.
//!
//! The profile types mirror the backend's `ide_profiler` ones, which this crate does not depend
//! on; they read the same JSON.

use crate::platform::i18n::{I18n, Param};
use crate::workbench::notifications::{NotificationAction, NotificationLevel, NotificationRequest};
use ide_run::{RunConfig, RunExtras};
use regex::Regex;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

/// How many snapshots stay open.
pub const MAX_SNAPSHOTS: usize = 5;
/// Rectangles narrower than this share of the graph are not drawn.
pub const MIN_FLAME_WIDTH: f64 = 0.002;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileFrame {
    pub name: String,
    pub file: Option<String>,
    /// 0-based.
    pub line: Option<u32>,
}

/// A call-tree node; node 0 is the synthetic root (`all`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileNode {
    /// Index into [`Profile::frames`].
    pub frame: u32,
    /// Weight of this node and everything it called.
    pub total: f64,
    /// Weight sampled in this node itself.
    pub self_weight: f64,
    /// Indices into [`Profile::nodes`], heaviest first.
    pub children: Vec<u32>,
}

/// Per-function totals.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MethodStat {
    pub frame: u32,
    pub self_weight: f64,
    pub total: f64,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub profiler: String,
    /// `ms` (time-weighted) or `samples`.
    pub unit: String,
    pub total: f64,
    pub frames: Vec<ProfileFrame>,
    /// Depth-first; `nodes[0]` is the root.
    pub nodes: Vec<ProfileNode>,
    /// Sorted by self weight, heaviest first.
    pub methods: Vec<MethodStat>,
}

/// A command line that installs a missing profiler.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfilerInstall {
    pub command: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
}

static PYTHON_PROGRAM: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:python[0-9.]*|py|pytest|py\.test)$").expect("the pattern is valid"));

/// The program's file name without a Windows launcher extension.
fn program_name(command: &str) -> &str {
    let name = command.rsplit(['/', '\\']).next().unwrap_or(command);
    let lower = name.to_ascii_lowercase();
    if [".exe", ".cmd", ".bat"].iter().any(|extension| lower.ends_with(extension)) {
        &name[..name.len() - 4]
    } else {
        name
    }
}

/// Whether a profiler is known for `config` (cheap; the backend decides for real): Node.js programs
/// and npm scripts, Python, JVM applications, and on Linux native programs and `cargo run`.
pub fn profiler_supports(config: &RunConfig, linux: bool) -> bool {
    let program = program_name(&config.command);
    if ["node", "npx", "tsx", "ts-node", "npm", "pnpm", "yarn", "java"].contains(&program) {
        return true;
    }
    if program == "cargo" {
        return linux && config.args.iter().find(|arg| !arg.starts_with('+')).is_some_and(|arg| arg == "run");
    }
    if config.kind == "native" {
        return linux;
    }
    config.kind == "python"
        || config.kind == "pytest"
        || PYTHON_PROGRAM.is_match(program)
        || (matches!(program, "uv" | "poetry") && config.args.first().is_some_and(|arg| arg == "run"))
}

/// `12.3 ms`, `1.2 s`, `340 samples`.
pub fn format_weight(value: f64, unit: &str) -> String {
    if unit == "ms" {
        if value >= 1000.0 {
            let seconds = value / 1000.0;
            return if value >= 10_000.0 { format!("{seconds:.1} s") } else { format!("{seconds:.2} s") };
        }
        return if value >= 100.0 { format!("{} ms", value.round()) } else { format!("{value:.1} ms") };
    }
    let n = value.round();
    format!("{n} {}", if n == 1.0 { "sample" } else { "samples" })
}

/// `value` as a share of `total`: `60%`, `5.0%`.
pub fn share(value: f64, total: f64) -> String {
    if total <= 0.0 {
        return "0%".to_owned();
    }
    let percent = value / total * 100.0;
    if percent >= 10.0 { format!("{}%", percent.round()) } else { format!("{percent:.1}%") }
}

/// One rectangle of the flame graph, in fractions of the zoomed width.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlameRect {
    pub node: usize,
    pub depth: usize,
    /// 0..1 of the visible width.
    pub x: f64,
    pub width: f64,
}

/// The flame graph's rectangles and its number of rows.
#[derive(Debug, Clone, PartialEq)]
pub struct FlameLayout {
    pub rects: Vec<FlameRect>,
    pub depth: usize,
}

/// The parent of every node (`None` for the root).
pub fn parent_index(profile: &Profile) -> Vec<Option<usize>> {
    let mut parents = vec![None; profile.nodes.len()];
    for (index, node) in profile.nodes.iter().enumerate() {
        for &child in &node.children {
            if let Some(parent) = parents.get_mut(child as usize) {
                *parent = Some(index);
            }
        }
    }
    parents
}

/// Flame graph rectangles (root at depth 0, callees in the rows below their callers) for the
/// subtree under `focus`, dropping those narrower than `min_width`. The focused node's ancestors
/// are shown full width above it, as in IDEA.
pub fn flame_layout(profile: &Profile, focus: usize, min_width: f64) -> FlameLayout {
    let nodes = &profile.nodes;
    let parents = parent_index(profile);
    let mut ancestors = Vec::new();
    let mut parent = parents.get(focus).copied().flatten();
    while let Some(at) = parent {
        ancestors.push(at);
        parent = parents.get(at).copied().flatten();
    }
    ancestors.reverse();
    let mut rects: Vec<FlameRect> =
        ancestors.iter().enumerate().map(|(depth, &node)| FlameRect { node, depth, x: 0.0, width: 1.0 }).collect();
    let base = ancestors.len();
    let total = nodes.get(focus).map_or(0.0, |node| node.total);
    if total <= 0.0 {
        return FlameLayout { rects, depth: base };
    }
    let mut max_depth = base;
    let mut stack = vec![(focus, base, 0.0)];
    while let Some((node, depth, x)) = stack.pop() {
        let Some(n) = nodes.get(node) else { continue };
        let width = n.total / total;
        if width < min_width {
            continue;
        }
        rects.push(FlameRect { node, depth, x, width });
        max_depth = max_depth.max(depth);
        let mut cx = x;
        for &child in &n.children {
            let child = child as usize;
            stack.push((child, depth + 1, cx));
            cx += nodes.get(child).map_or(0.0, |c| c.total) / total;
        }
    }
    FlameLayout { rects, depth: max_depth + 1 }
}

/// Frames whose name contains `query` (case-insensitive), or `None` when not searching.
pub fn matching_frames(profile: &Profile, query: &str) -> Option<BTreeSet<usize>> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return None;
    }
    let frames = profile.frames.iter().enumerate();
    Some(frames.filter(|(_, frame)| frame.name.to_lowercase().contains(&query)).map(|(index, _)| index).collect())
}

/// Short file location of a frame: `index.js:12`.
pub fn frame_location(profile: &Profile, frame: usize) -> Option<String> {
    let frame = profile.frames.get(frame)?;
    let file = frame.file.as_deref().filter(|file| !file.is_empty())?;
    let name = file.rsplit(['/', '\\']).next().unwrap_or(file);
    Some(match frame.line {
        Some(line) => format!("{name}:{}", u64::from(line) + 1),
        None => name.to_owned(),
    })
}

/// Colour slot (1–4) of a frame name, stable across renders.
pub fn color_slot(name: &str) -> u32 {
    let hash = name.encode_utf16().fold(0i32, |hash, unit| hash.wrapping_mul(31).wrapping_add(i32::from(unit)));
    hash.unsigned_abs() % 4 + 1
}

/// A profile taken from one run.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub id: String,
    /// Run configuration name.
    pub name: String,
    /// Unix milliseconds.
    pub created_at: f64,
    pub profile: Profile,
}

/// The tabs of a snapshot's view.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ProfilerTab {
    #[default]
    Flame,
    Tree,
    Methods,
}

/// The Profiler tool window's snapshots.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProfilerStore {
    /// Newest first.
    pub snapshots: Vec<Snapshot>,
    pub active_id: Option<String>,
    pub tab: ProfilerTab,
    pub query: String,
    seq: u64,
}

impl ProfilerStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Opens the profile of the run of configuration `name`, taken at `now`, as the active
    /// snapshot; the oldest beyond [`MAX_SNAPSHOTS`] closes.
    pub fn add_snapshot(&mut self, name: &str, profile: Profile, now: f64) -> &Snapshot {
        self.seq += 1;
        let id = format!("profile{}", self.seq);
        self.snapshots.insert(0, Snapshot { id: id.clone(), name: name.to_owned(), created_at: now, profile });
        self.snapshots.truncate(MAX_SNAPSHOTS);
        self.active_id = Some(id);
        &self.snapshots[0]
    }

    pub fn active_snapshot(&self) -> Option<&Snapshot> {
        self.snapshots.iter().find(|snapshot| Some(&snapshot.id) == self.active_id.as_ref())
    }

    /// Closes snapshot `id`; closing the active one activates the newest left.
    pub fn close_snapshot(&mut self, id: &str) {
        self.snapshots.retain(|snapshot| snapshot.id != id);
        if self.active_id.as_deref() == Some(id) {
            self.active_id = self.snapshots.first().map(|snapshot| snapshot.id.clone());
        }
    }

    /// Another project was opened: its snapshots are gone. The chosen tab stays.
    pub fn reset(&mut self) {
        self.snapshots.clear();
        self.active_id = None;
        self.query.clear();
    }
}

/// The action of a missing-profiler notification that runs [`MissingProfiler::install`].
pub const INSTALL_COMMAND: &str = "install";
/// The action of a missing-profiler notification that opens [`MissingProfiler::homepage`].
pub const DOWNLOAD_COMMAND: &str = "download";

/// The notification of a configuration no profiler is known for (`reason` says why).
pub fn unsupported_notification(name: &str, reason: &str) -> NotificationRequest {
    NotificationRequest {
        key: Some("profiler-unsupported".to_owned()),
        group: Some("run".to_owned()),
        body: Some(reason.to_owned()),
        ..NotificationRequest::new(NotificationLevel::Info, format!("Cannot profile '{name}'"))
    }
}

/// What to show when the profiler a run needs is not installed.
#[derive(Debug, Clone, PartialEq)]
pub struct MissingProfiler {
    pub notification: NotificationRequest,
    /// The run configuration of the notification's Install action.
    pub install: Option<RunConfig>,
    /// The page the Download… action opens.
    pub homepage: Option<String>,
}

/// The notification that profiler `tool` is missing: `message` explains it, with the command line
/// that installs it or else its homepage.
pub fn missing_profiler(
    tool: &str,
    message: &str,
    install: Option<&ProfilerInstall>,
    homepage: Option<&str>,
    i18n: &mut I18n,
) -> MissingProfiler {
    let mut actions = Vec::new();
    let config = install.map(|install| {
        actions.push(NotificationAction { label: "Install".to_owned(), command: INSTALL_COMMAND.to_owned() });
        RunConfig {
            id: format!("install:{tool}"),
            name: format!("Install {tool}"),
            kind: "shell".to_owned(),
            command: install.command.clone(),
            args: install.args.clone(),
            cwd: install.cwd.as_deref().map(Into::into),
            env: BTreeMap::new(),
            detected: true,
            is_test: false,
            debug_adapter: None,
            allow_parallel: false,
            extras: RunExtras::default(),
        }
    });
    if homepage.is_some() {
        actions.push(NotificationAction { label: "Download…".to_owned(), command: DOWNLOAD_COMMAND.to_owned() });
    }
    let body = match (install, homepage) {
        (Some(install), _) => {
            let line = std::iter::once(&install.command).chain(&install.args).cloned().collect::<Vec<_>>().join(" ");
            i18n.tx_built(&["", " Install it with: ", ""], &[Param::from(message), Param::from(line)])
        }
        (None, Some(homepage)) => format!("{message} {homepage}"),
        (None, None) => message.to_owned(),
    };
    let notification = NotificationRequest {
        key: Some(format!("profiler-missing-{tool}")),
        group: Some("run".to_owned()),
        body: Some(body),
        sticky: Some(true),
        actions,
        ..NotificationRequest::new(NotificationLevel::Warning, format!("{tool} is not installed"))
    };
    MissingProfiler { notification, install: config, homepage: homepage.map(str::to_owned) }
}

/// The notification of a profiled run whose profile could not be read.
pub fn collect_failed_notification(name: &str, error: &str) -> NotificationRequest {
    NotificationRequest {
        key: Some("profiler-done".to_owned()),
        group: Some("run".to_owned()),
        body: Some(error.to_owned()),
        ..NotificationRequest::new(NotificationLevel::Error, format!("No profile for '{name}'"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(name: &str, file: Option<&str>, line: Option<u32>) -> ProfileFrame {
        ProfileFrame { name: name.to_owned(), file: file.map(str::to_owned), line }
    }

    fn node(frame: u32, total: f64, self_weight: f64, children: &[u32]) -> ProfileNode {
        ProfileNode { frame, total, self_weight, children: children.to_vec() }
    }

    /// all(10) → main(10, self 2) → [work(6), io(2)]
    fn profile() -> Profile {
        Profile {
            profiler: "t".to_owned(),
            unit: "samples".to_owned(),
            total: 10.0,
            frames: vec![
                frame("all", None, None),
                frame("main", Some("/p/main.py"), Some(4)),
                frame("work", None, None),
                frame("io", None, None),
            ],
            nodes: vec![
                node(0, 10.0, 0.0, &[1]),
                node(1, 10.0, 2.0, &[2, 3]),
                node(2, 6.0, 6.0, &[]),
                node(3, 2.0, 2.0, &[]),
            ],
            methods: Vec::new(),
        }
    }

    fn config(command: &str, args: &[&str], kind: &str) -> RunConfig {
        RunConfig {
            id: "c".to_owned(),
            name: "c".to_owned(),
            kind: kind.to_owned(),
            command: command.to_owned(),
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
            cwd: None,
            env: BTreeMap::new(),
            detected: true,
            is_test: false,
            debug_adapter: None,
            allow_parallel: false,
            extras: RunExtras::default(),
        }
    }

    #[test]
    fn knows_which_configurations_can_be_profiled() {
        assert!(profiler_supports(&config("node", &["a.js"], "shell"), false));
        assert!(profiler_supports(&config("java", &["-cp", "x", "A"], "application"), false));
        assert!(profiler_supports(&config(".venv/bin/python", &["main.py"], "shell"), false));
        assert!(!profiler_supports(&config("cargo", &["run"], "shell"), false));
        assert!(profiler_supports(&config("cargo", &["run"], "shell"), true));
        assert!(profiler_supports(&config("./app", &[], "native"), true));
        assert!(!profiler_supports(&config("make", &[], "shell"), true));
    }

    #[test]
    fn lays_out_the_flame_graph_and_zooms_into_a_node() {
        let profile = profile();
        let FlameLayout { rects, depth } = flame_layout(&profile, 0, MIN_FLAME_WIDTH);
        assert_eq!(depth, 3);
        let at = |node: usize| rects.iter().find(|rect| rect.node == node).copied();
        assert_eq!(at(0), Some(FlameRect { node: 0, depth: 0, x: 0.0, width: 1.0 }));
        assert_eq!(at(2), Some(FlameRect { node: 2, depth: 2, x: 0.0, width: 0.6 }));
        assert_eq!(at(3), Some(FlameRect { node: 3, depth: 2, x: 0.6, width: 0.2 }));

        let zoomed = flame_layout(&profile, 2, MIN_FLAME_WIDTH);
        let rows: Vec<(usize, usize, f64)> =
            zoomed.rects.iter().map(|rect| (rect.node, rect.depth, rect.width)).collect();
        assert_eq!(rows, [(0, 0, 1.0), (1, 1, 1.0), (2, 2, 1.0)]);
        // Too narrow frames are dropped.
        assert!(!flame_layout(&profile, 0, 0.3).rects.iter().any(|rect| rect.node == 3));
    }

    #[test]
    fn searches_formats_and_locates_frames() {
        let profile = profile();
        assert_eq!(matching_frames(&profile, "  "), None);
        assert_eq!(matching_frames(&profile, "WO"), Some(BTreeSet::from([2])));
        assert_eq!(format_weight(1.0, "samples"), "1 sample");
        assert_eq!(format_weight(12.34, "ms"), "12.3 ms");
        assert_eq!(format_weight(12_340.0, "ms"), "12.3 s");
        assert_eq!(share(6.0, 10.0), "60%");
        assert_eq!(share(0.5, 10.0), "5.0%");
        assert_eq!(frame_location(&profile, 1).as_deref(), Some("main.py:5"));
        assert_eq!(frame_location(&profile, 2), None);
        assert_eq!(color_slot("main"), color_slot("main"));
        assert!((1..=4).contains(&color_slot("main")));
    }
}
