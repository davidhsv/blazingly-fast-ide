//! Run with Coverage (IDEA's third executor beside Run and Debug): which configurations a coverage runner
//! is known for, the suites of the project window (the last few runs and imported reports, one shown in
//! editors and the project tree until hidden), the Coverage tool window's directory tree with its
//! percentages, the project tree's badges and the editor's coverage stripes.

use crate::platform::collate;
use crate::platform::i18n::{I18n, Param};
use ide_run::RunConfig;
use regex::Regex;
use serde::Deserialize;
use serde_json::{Value, json};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::LazyLock;

/// Coverage of one source line (the backend's `LineCoverage`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LineCoverage {
    /// 0-based.
    pub line: u32,
    /// Execution count; 0 = not covered.
    pub hits: u32,
    pub branches_total: u32,
    pub branches_covered: u32,
}

/// Coverage of one file (the backend's `FileCoverage`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileCoverage {
    pub path: String,
    /// Sorted by line.
    pub lines: Vec<LineCoverage>,
    pub lines_total: u32,
    pub lines_covered: u32,
    pub branches_total: u32,
    pub branches_covered: u32,
}

/// One coverage run's report (the backend's `CoverageSuite`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoverageSuite {
    /// Tool that produced it, e.g. `cargo-llvm-cov`, `JaCoCo`.
    pub runner: String,
    /// Whether branch counts are meaningful.
    pub has_branches: bool,
    /// Sorted by path.
    pub files: Vec<FileCoverage>,
}

// ---------------------------------------------------------------------------
// Which configurations can run with coverage.

static WINDOWS_EXECUTABLE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\.(exe|cmd|bat)$").expect("the pattern is valid"));
static PYTHON: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(python[\d.]*|py|pytest|py\.test)$").expect("the pattern is valid"));

/// File name of a command without a Windows executable extension.
pub fn program_name(command: &str) -> String {
    let name = command.rsplit(['/', '\\']).next().unwrap_or(command);
    WINDOWS_EXECUTABLE.replace(name, "").into_owned()
}

/// What [`coverage_supports`] reads of a run configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoverageProbe<'a> {
    pub command: &'a str,
    pub args: &'a [String],
    pub kind: &'a str,
    pub is_test: bool,
}

impl<'a> CoverageProbe<'a> {
    pub fn of(config: &'a RunConfig) -> Self {
        Self { command: &config.command, args: &config.args, kind: &config.kind, is_test: config.is_test }
    }
}

fn runs_jvm_tests(args: &[String]) -> bool {
    args.iter().any(|arg| {
        let task = arg.rsplit(':').next().unwrap_or(arg);
        ["test", "verify", "check", "integrationTest"].contains(&task) || arg.starts_with("-Dtest=") || arg == "--tests"
    })
}

/// Whether a coverage runner is known for a configuration (cheap; the backend decides for real): cargo
/// test / run / nextest, go test, Maven / Gradle tests, package.json test scripts, Node.js programs, Python
/// and Ruby (scripts, rspec, test rake tasks).
pub fn coverage_supports(config: CoverageProbe) -> bool {
    let program = program_name(config.command);
    let args = config.args;
    match program.as_str() {
        "cargo" => {
            let sub = args.iter().find(|arg| !arg.starts_with('+')).map(String::as_str);
            matches!(sub, Some("test" | "run" | "nextest"))
        }
        "go" => args.first().is_some_and(|arg| arg == "test"),
        "mvn" | "mvnw" | "gradle" | "gradlew" => runs_jvm_tests(args),
        "npm" | "pnpm" | "yarn" | "bun" | "bundle" | "rake" | "rails" => config.is_test,
        "node" | "npx" | "tsx" | "ts-node" | "ruby" | "rspec" => true,
        _ => {
            config.kind == "python"
                || config.kind == "pytest"
                || PYTHON.is_match(&program)
                || (matches!(program.as_str(), "uv" | "poetry") && args.first().is_some_and(|arg| arg == "run"))
        }
    }
}

// ---------------------------------------------------------------------------
// Percentages and the tree.

/// Percentage (0–100), or `None` when there is nothing to cover.
pub fn percent(covered: u32, total: u32) -> Option<f64> {
    (total > 0).then(|| f64::from(covered) / f64::from(total) * 100.0)
}

/// Whole percent, `<1` below 1.
fn shown_percent(p: f64) -> String {
    if p > 0.0 && p < 1.0 { "<1".to_owned() } else { format!("{}", p.floor()) }
}

/// IDEA style: `85% (17/20)`; `n/a` when there is nothing to cover.
pub fn format_percent(covered: u32, total: u32) -> String {
    match percent(covered, total) {
        None => "n/a".to_owned(),
        Some(p) => format!("{}% ({covered}/{total})", shown_percent(p)),
    }
}

/// A directory or file of the Coverage tool window; directories sum their files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverageNode {
    /// Absolute path (directories too).
    pub path: String,
    pub name: String,
    pub dir: bool,
    pub children: Vec<CoverageNode>,
    pub lines_total: u32,
    pub lines_covered: u32,
    pub branches_total: u32,
    pub branches_covered: u32,
}

impl CoverageNode {
    fn dir(path: String, name: String) -> Self {
        Self {
            path,
            name,
            dir: true,
            children: Vec::new(),
            lines_total: 0,
            lines_covered: 0,
            branches_total: 0,
            branches_covered: 0,
        }
    }
}

fn join(a: &str, b: &str) -> String {
    if a.is_empty() { b.to_owned() } else { format!("{}/{b}", a.strip_suffix('/').unwrap_or(a)) }
}

fn sum(node: &mut CoverageNode) {
    if !node.dir {
        return;
    }
    node.children.iter_mut().for_each(sum);
    node.lines_total = node.children.iter().map(|c| c.lines_total).sum();
    node.lines_covered = node.children.iter().map(|c| c.lines_covered).sum();
    node.branches_total = node.children.iter().map(|c| c.branches_total).sum();
    node.branches_covered = node.children.iter().map(|c| c.branches_covered).sum();
}

fn compact(node: &CoverageNode) -> CoverageNode {
    let mut current = node.clone();
    while current.dir && current.children.len() == 1 && current.children[0].dir {
        let only = current.children.remove(0);
        current = CoverageNode { name: format!("{}/{}", current.name, only.name), ..only };
    }
    let children = current.children.iter().map(compact).collect();
    CoverageNode { children, ..current }
}

/// The files of `suite` as a directory tree relative to `root` (files elsewhere under their absolute
/// directories); with `compact_dirs` single-child directory chains are one row (`src/main/java`).
pub fn coverage_tree(suite: &CoverageSuite, root: &str, compact_dirs: bool) -> Vec<CoverageNode> {
    let mut top = CoverageNode::dir(root.to_owned(), String::new());
    let prefix = format!("{}/", root.strip_suffix('/').unwrap_or(root));
    for file in &suite.files {
        let inside = file.path.starts_with(&prefix);
        let rel = if inside { &file.path[prefix.len()..] } else { file.path.strip_prefix('/').unwrap_or(&file.path) };
        let parts: Vec<&str> = rel.split('/').collect();
        let (name, dirs) = parts.split_last().map_or(("", &[][..]), |(name, dirs)| (*name, dirs));
        let mut node = &mut top;
        let mut base = if inside { root.to_owned() } else { String::new() };
        for part in dirs {
            base = if inside || !base.is_empty() { join(&base, part) } else { format!("/{part}") };
            let index = node.children.iter().position(|c| c.dir && c.name == *part).unwrap_or_else(|| {
                node.children.push(CoverageNode::dir(base.clone(), (*part).to_owned()));
                node.children.len() - 1
            });
            node = &mut node.children[index];
        }
        node.children.push(CoverageNode {
            path: file.path.clone(),
            name: name.to_owned(),
            dir: false,
            children: Vec::new(),
            lines_total: file.lines_total,
            lines_covered: file.lines_covered,
            branches_total: file.branches_total,
            branches_covered: file.branches_covered,
        });
    }
    sum(&mut top);
    if compact_dirs { top.children.iter().map(compact).collect() } else { top.children }
}

/// Columns the tree sorts by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    Name,
    Lines,
    Branches,
}

/// Sorted copy: by name (directories first) or by a percentage (rows without data last either way).
pub fn sort_tree(nodes: &[CoverageNode], key: SortKey, ascending: bool) -> Vec<CoverageNode> {
    let value = |n: &CoverageNode| match key {
        SortKey::Lines => percent(n.lines_covered, n.lines_total),
        _ => percent(n.branches_covered, n.branches_total),
    };
    let directed = |order: Ordering| if ascending { order } else { order.reverse() };
    let mut sorted = nodes.to_vec();
    sorted.sort_by(|a, b| {
        if key == SortKey::Name {
            if a.dir != b.dir {
                return if a.dir { Ordering::Less } else { Ordering::Greater };
            }
            return directed(collate::compare(&a.name, &b.name));
        }
        match (value(a), value(b)) {
            (None, None) => collate::compare(&a.name, &b.name),
            (None, Some(_)) => Ordering::Greater,
            (Some(_), None) => Ordering::Less,
            (Some(va), Some(vb)) => directed(va.total_cmp(&vb)).then_with(|| collate::compare(&a.name, &b.name)),
        }
    });
    sorted
        .into_iter()
        .map(|n| if n.dir { CoverageNode { children: sort_tree(&n.children, key, ascending), ..n } } else { n })
        .collect()
}

/// Per path (files and directories): the line coverage badge of the project tree, e.g. `85% lines`.
pub fn coverage_badges(nodes: &[CoverageNode]) -> HashMap<String, String> {
    fn visit(node: &CoverageNode, out: &mut HashMap<String, String>) {
        if let Some(p) = percent(node.lines_covered, node.lines_total) {
            out.insert(node.path.clone(), format!("{}% lines", shown_percent(p)));
        }
        for child in &node.children {
            visit(child, out);
        }
    }
    let mut out = HashMap::new();
    for node in nodes {
        visit(node, &mut out);
    }
    out
}

// ---------------------------------------------------------------------------
// Editor stripes.

/// How a line is covered: green covered, yellow partly covered (branches / blocks), red not covered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineState {
    Covered,
    Partial,
    Uncovered,
}

impl LineState {
    /// The theme color of the stripe.
    pub fn color_token(self) -> &'static str {
        match self {
            Self::Covered => "coverage-covered",
            Self::Partial => "coverage-partial",
            Self::Uncovered => "coverage-uncovered",
        }
    }
}

pub fn line_state(line: &LineCoverage) -> LineState {
    if line.hits == 0 {
        LineState::Uncovered
    } else if line.branches_covered < line.branches_total {
        LineState::Partial
    } else {
        LineState::Covered
    }
}

/// A stripe in the editor's change lane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverageMarker {
    pub line: u32,
    pub state: LineState,
    /// Translated tooltip.
    pub title: String,
}

fn marker_title(line: &LineCoverage, i18n: &mut I18n) -> String {
    let hits = match line_state(line) {
        LineState::Uncovered => i18n.tx("Not covered"),
        _ => i18n.tx_built(&["Hits: ", ""], &[Param::from(f64::from(line.hits))]),
    };
    if line.branches_total == 0 {
        return hits;
    }
    i18n.tx_built(
        &["", ", ", "/", " branches covered"],
        &[
            Param::from(hits),
            Param::from(f64::from(line.branches_covered)),
            Param::from(f64::from(line.branches_total)),
        ],
    )
}

/// The stripes of `file` in a document of `line_count` lines (lines past its end are left out).
pub fn coverage_markers(file: Option<&FileCoverage>, line_count: u32, i18n: &mut I18n) -> Vec<CoverageMarker> {
    let Some(file) = file else { return Vec::new() };
    file.lines
        .iter()
        .filter(|line| line.line < line_count)
        .map(|line| CoverageMarker { line: line.line, state: line_state(line), title: marker_title(line, i18n) })
        .collect()
}

// ---------------------------------------------------------------------------
// Suites.

/// Suites kept (IDEA keeps a list of recent suites to choose from).
pub const MAX_SUITES: usize = 5;

/// A coverage run or an imported report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suite {
    pub id: String,
    /// Run configuration name, or the imported report's file name.
    pub name: String,
    /// Milliseconds since the epoch.
    pub created_at: u64,
    /// The report file of an imported suite.
    pub file: Option<String>,
    pub data: CoverageSuite,
    by_path: HashMap<String, usize>,
}

impl Suite {
    /// The coverage of `path` in this suite.
    pub fn file_coverage(&self, path: &str) -> Option<&FileCoverage> {
        self.by_path.get(path).and_then(|i| self.data.files.get(*i))
    }
}

/// The coverage suites of a project window, newest first; one is shown until hidden.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CoverageStore {
    pub suites: Vec<Suite>,
    pub active_id: Option<String>,
    /// Whether the active suite is shown in editors and the project tree.
    pub visible: bool,
    sequence: u64,
}

impl CoverageStore {
    pub fn active_suite(&self) -> Option<&Suite> {
        self.suites.iter().find(|suite| Some(&suite.id) == self.active_id.as_ref())
    }

    /// The shown coverage of `path`, if any.
    pub fn shown_file_coverage(&self, path: &str) -> Option<&FileCoverage> {
        if !self.visible {
            return None;
        }
        self.active_suite()?.file_coverage(path)
    }

    /// Adds a suite and shows it; a re-imported report `file` replaces its earlier suite. Returns its id.
    pub fn add_suite(&mut self, name: &str, data: CoverageSuite, file: Option<&str>, now: u64) -> String {
        self.sequence += 1;
        let id = format!("suite{}", self.sequence);
        let by_path = data.files.iter().enumerate().map(|(i, f)| (f.path.clone(), i)).collect();
        let file_path = file.map(str::to_owned);
        let suite = Suite { id: id.clone(), name: name.to_owned(), created_at: now, file: file_path, data, by_path };
        if let Some(file) = file {
            self.suites.retain(|known| known.file.as_deref() != Some(file));
        }
        self.suites.insert(0, suite);
        self.suites.truncate(MAX_SUITES);
        self.active_id = Some(id.clone());
        self.visible = true;
        id
    }

    pub fn select_suite(&mut self, id: &str) {
        self.active_id = Some(id.to_owned());
        self.visible = true;
    }

    /// Removes a suite; the newest one left becomes active when it was.
    pub fn close_suite(&mut self, id: &str) {
        self.suites.retain(|suite| suite.id != id);
        if self.active_id.as_deref() == Some(id) {
            self.active_id = self.suites.first().map(|suite| suite.id.clone());
        }
        self.visible = self.visible && self.active_id.is_some();
    }

    /// Show / Hide Coverage; nothing shows without an active suite.
    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible && self.active_id.is_some();
    }

    /// The window switched projects.
    pub fn reset(&mut self) {
        self.suites.clear();
        self.active_id = None;
        self.visible = false;
    }

    /// The `coverageVisible` and `coverageSuites` context keys.
    pub fn context_keys(&self) -> [(&'static str, bool); 2] {
        [("coverageVisible", self.visible), ("coverageSuites", !self.suites.is_empty())]
    }

    /// The project tree's badges of the shown suite.
    pub fn badges(&self, root: Option<&str>) -> HashMap<String, String> {
        match (self.visible.then(|| self.active_suite()).flatten(), root) {
            (Some(suite), Some(root)) => coverage_badges(&coverage_tree(&suite.data, root, false)),
            _ => HashMap::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Imported reports, remembered per project so "Manage Coverage Reports" lists them after a restart.

/// A report imported from a file.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportedReport {
    pub path: String,
    pub name: String,
    /// Milliseconds since the epoch.
    pub imported_at: f64,
}

/// Reports remembered per project.
pub const MAX_IMPORTED: usize = 20;

/// The storage key of the reports imported into the project at `root`.
pub fn imported_key(root: &str) -> String {
    format!("openidea.coverage.imported.v1:{root}")
}

/// The remembered reports from their stored JSON; anything malformed reads as none.
pub fn parse_imported(stored: Option<&str>) -> Vec<ImportedReport> {
    let Some(Ok(Value::Array(items))) = stored.map(serde_json::from_str::<Value>) else { return Vec::new() };
    items
        .iter()
        .filter_map(|item| {
            Some(ImportedReport {
                path: item.get("path")?.as_str()?.to_owned(),
                name: item.get("name")?.as_str()?.to_owned(),
                imported_at: item.get("importedAt")?.as_f64()?,
            })
        })
        .collect()
}

/// The reports as stored (at most [`MAX_IMPORTED`]).
pub fn serialize_imported(reports: &[ImportedReport]) -> String {
    let items: Vec<Value> = reports
        .iter()
        .take(MAX_IMPORTED)
        .map(|r| json!({ "path": r.path, "name": r.name, "importedAt": r.imported_at }))
        .collect();
    Value::Array(items).to_string()
}

/// `report` first, replacing an earlier import of its file.
pub fn remember_import(reports: &[ImportedReport], report: ImportedReport) -> Vec<ImportedReport> {
    let rest: Vec<ImportedReport> = reports.iter().filter(|r| r.path != report.path).cloned().collect();
    std::iter::once(report).chain(rest).take(MAX_IMPORTED).collect()
}

pub fn forget_import(reports: &[ImportedReport], path: &str) -> Vec<ImportedReport> {
    reports.iter().filter(|r| r.path != path).cloned().collect()
}

/// A row of "Manage Coverage Reports": a loaded suite, or an imported report not loaded now.
#[derive(Debug, Clone, PartialEq)]
pub struct ReportEntry {
    pub key: String,
    pub name: String,
    /// Tool or file shown beside the name.
    pub detail: String,
    pub time: f64,
    pub suite_id: Option<String>,
    pub file: Option<String>,
}

/// The loaded suites, then the remembered reports no suite shows.
pub fn report_entries(suites: &[Suite], imported: &[ImportedReport]) -> Vec<ReportEntry> {
    let loaded = suites.iter().map(|suite| ReportEntry {
        key: suite.id.clone(),
        name: suite.name.clone(),
        detail: suite.file.clone().unwrap_or_else(|| suite.data.runner.clone()),
        time: suite.created_at as f64,
        suite_id: Some(suite.id.clone()),
        file: suite.file.clone(),
    });
    let remembered =
        imported.iter().filter(|r| !suites.iter().any(|s| s.file.as_ref() == Some(&r.path))).map(|r| ReportEntry {
            key: format!("file:{}", r.path),
            name: r.name.clone(),
            detail: r.path.clone(),
            time: r.imported_at,
            suite_id: None,
            file: Some(r.path.clone()),
        });
    loaded.chain(remembered).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn supports(command: &str, args: &[&str], is_test: bool) -> bool {
        let args: Vec<String> = args.iter().map(|a| (*a).to_owned()).collect();
        coverage_supports(CoverageProbe { command, args: &args, kind: "shell", is_test })
    }

    #[test]
    fn knows_the_coverage_runners() {
        assert!(supports("cargo", &["test"], false));
        assert!(supports("cargo", &["+nightly", "run"], false));
        assert!(!supports("cargo", &["build"], false));
        assert!(supports("go", &["test", "./..."], false));
        assert!(!supports("go", &["run", "."], false));
        assert!(supports("./mvnw", &["test", "-Dtest=A"], false));
        assert!(supports("gradlew.bat", &[":app:test"], false));
        assert!(!supports("mvn", &["package"], false));
        assert!(supports("pnpm", &["run", "test"], true));
        assert!(!supports("pnpm", &["run", "dev"], false));
        assert!(supports("/v/bin/python3", &["-m", "pytest"], false));
        assert!(supports("uv", &["run", "pytest"], false));
        assert!(supports("/v/bin/ruby", &["-Ilib", "test/a_test.rb"], false));
        assert!(supports("bundle", &["exec", "rspec"], true));
        assert!(!supports("bundle", &["exec", "rake", "db:migrate"], false));
        assert!(!supports("make", &["test"], false));
    }

    fn file(path: &str, covered: u32, total: u32, branches: (u32, u32)) -> FileCoverage {
        FileCoverage {
            path: path.to_owned(),
            lines: Vec::new(),
            lines_covered: covered,
            lines_total: total,
            branches_covered: branches.0,
            branches_total: branches.1,
        }
    }

    fn suite(files: Vec<FileCoverage>) -> CoverageSuite {
        CoverageSuite { runner: "t".to_owned(), has_branches: true, files }
    }

    fn data() -> CoverageSuite {
        suite(vec![
            file("/p/src/a/x.rs", 3, 4, (1, 2)),
            file("/p/src/a/y.rs", 0, 2, (0, 0)),
            file("/p/src/b.rs", 5, 5, (0, 0)),
            file("/other/lib.rs", 1, 1, (0, 0)),
        ])
    }

    fn names(nodes: &[CoverageNode]) -> Vec<&str> {
        nodes.iter().map(|n| n.name.as_str()).collect()
    }

    #[test]
    fn groups_by_directory_sums_and_compacts_single_child_chains() {
        let tree = coverage_tree(&data(), "/p", true);
        assert_eq!(names(&tree), ["src", "other"]);
        let src = &tree[0];
        assert_eq!((src.lines_covered, src.lines_total, src.branches_covered, src.branches_total), (8, 11, 1, 2));
        assert_eq!(tree[1].path, "/other");
        assert_eq!(tree[1].children[0].path, "/other/lib.rs");
        let deep = coverage_tree(&suite(vec![file("/p/src/main/java/A.java", 1, 2, (0, 0))]), "/p", true);
        assert_eq!(deep[0].name, "src/main/java");
        assert_eq!(deep[0].path, "/p/src/main/java");
    }

    #[test]
    fn sorts_by_name_directories_first_or_percentage() {
        let tree = coverage_tree(&data(), "/p", true);
        let by_name = sort_tree(&tree, SortKey::Name, true);
        assert_eq!(names(&by_name[1].children), ["a", "b.rs"]);
        assert_eq!(names(&sort_tree(&tree[0].children, SortKey::Lines, false)), ["b.rs", "a"]);
        assert_eq!(names(&sort_tree(&tree[0].children, SortKey::Branches, true)), ["a", "b.rs"]);
    }

    #[test]
    fn badges_every_directory_and_file_of_the_project_tree() {
        let badges = coverage_badges(&coverage_tree(&data(), "/p", false));
        assert_eq!(badges.get("/p/src").map(String::as_str), Some("72% lines"));
        assert_eq!(badges.get("/p/src/a").map(String::as_str), Some("50% lines"));
        assert_eq!(badges.get("/p/src/a/y.rs").map(String::as_str), Some("0% lines"));
    }

    #[test]
    fn formats_percentages_like_idea() {
        assert_eq!(format_percent(17, 20), "85% (17/20)");
        assert_eq!(format_percent(1, 1000), "<1% (1/1000)");
        assert_eq!(format_percent(0, 0), "n/a");
    }

    #[test]
    fn marks_covered_partial_and_uncovered_lines() {
        let line = |line, hits, branches_total, branches_covered| LineCoverage {
            line,
            hits,
            branches_total,
            branches_covered,
        };
        let f = FileCoverage {
            lines: vec![line(0, 2, 0, 0), line(1, 1, 2, 1), line(2, 0, 0, 0), line(99, 1, 0, 0)],
            ..file("/p/a.rs", 2, 3, (0, 0))
        };
        let states: Vec<LineState> = f.lines.iter().map(line_state).collect();
        assert_eq!(states, [LineState::Covered, LineState::Partial, LineState::Uncovered, LineState::Covered]);
        let mut i18n = I18n::default();
        // "a\nb\nc\n" has four lines, the last one empty.
        let markers = coverage_markers(Some(&f), 4, &mut i18n);
        let shown: Vec<(LineState, &str)> = markers.iter().map(|m| (m.state, m.title.as_str())).collect();
        assert_eq!(
            shown,
            [
                (LineState::Covered, "Hits: 2"),
                (LineState::Partial, "Hits: 1, 1/2 branches covered"),
                (LineState::Uncovered, "Not covered"),
            ]
        );
    }

    #[test]
    fn keeps_the_last_suites_and_hides_coverage_on_demand() {
        let mut store = CoverageStore::default();
        for i in 0..MAX_SUITES + 2 {
            store.add_suite(&format!("run {i}"), suite(vec![file(&format!("/p/{i}.rs"), 1, 1, (0, 0))]), None, 0);
        }
        assert_eq!(store.suites.len(), MAX_SUITES);
        assert_eq!(store.suites[0].name, format!("run {}", MAX_SUITES + 1));
        let newest = format!("/p/{}.rs", MAX_SUITES + 1);
        assert!(store.shown_file_coverage(&newest).is_some());
        store.set_visible(false);
        assert!(store.shown_file_coverage(&newest).is_none());
        let (first, second) = (store.suites[0].id.clone(), store.suites[1].id.clone());
        store.close_suite(&first);
        assert_eq!(store.active_id, Some(second));
    }

    #[test]
    fn replaces_a_re_imported_report_and_lists_remembered_imports_in_manage_coverage_reports() {
        let mut store = CoverageStore::default();
        store.add_suite("tests", suite(vec![file("/p/a.rs", 1, 2, (0, 0))]), None, 1);
        let first = store.add_suite("lcov.info", suite(vec![file("/p/a.rs", 1, 2, (0, 0))]), Some("/ci/lcov.info"), 2);
        let again = store.add_suite("lcov.info", suite(vec![file("/p/a.rs", 2, 2, (0, 0))]), Some("/ci/lcov.info"), 3);
        let ids: Vec<&str> = store.suites.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, [again.as_str(), store.suites[1].id.as_str()]);
        assert!(!store.suites.iter().any(|s| s.id == first));
        assert_eq!(store.shown_file_coverage("/p/a.rs").map(|f| f.lines_covered), Some(2));
        let imported = parse_imported(Some(
            r#"[{"path":"/ci/lcov.info","name":"lcov.info","importedAt":1},
                {"path":"/ci/jacoco.xml","name":"jacoco.xml","importedAt":2}, {"path":3}]"#,
        ));
        let entries = report_entries(&store.suites, &imported);
        let shape: Vec<(&str, &str, bool)> =
            entries.iter().map(|e| (e.name.as_str(), e.detail.as_str(), e.suite_id.is_some())).collect();
        assert_eq!(
            shape,
            [("lcov.info", "/ci/lcov.info", true), ("tests", "t", true), ("jacoco.xml", "/ci/jacoco.xml", false)]
        );
    }
}
