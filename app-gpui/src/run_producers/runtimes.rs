//! Runtime lookup for the run-context producers: which tools to locate near a file (project-local
//! `node_modules/.bin`, then the search path, with versions; remembered for a minute per directory),
//! which ones a run target supplies instead, and the runtime markers of the package holding the file.
//!
//! Nothing here asks the backend: [`tool_query`] says what to ask, [`MarkerSearch`] which directory to
//! list next, and [`ToolQuery::env`] puts the answers together.

use crate::run::configs::blank_config;
use crate::run::store::{RunSettings, with_run_target};
use crate::run_producers::producers::{DEFAULT_COMPILE_DIR, LocatedTool, ToolEnv};
use ide_run::RunConfig;
use std::collections::{BTreeMap, HashMap};

/// Tools by name: `None` for one that is not installed.
pub type Located = BTreeMap<String, Option<LocatedTool>>;

/// How long a located tool is remembered, in milliseconds.
pub const TOOL_TTL_MS: f64 = 60_000.0;

/// Tools a Node.js or PHP run target supplies.
const TARGET_TOOLS: [&str; 9] = ["php", "node", "npm", "npx", "pnpm", "yarn", "bun", "tsx", "ts-node"];

fn dir_of(path: &str) -> &str {
    match path.rsplit_once('/') {
        Some((dir, _)) if !dir.is_empty() => dir,
        _ => "/",
    }
}

/// The JavaScript runtimes the package holding a file is set up for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RuntimeMarkers {
    /// `deno.json(c)`.
    pub deno: bool,
    /// `bun.lock(b)`.
    pub bun: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MarkerStep {
    /// List this directory, then [`MarkerSearch::listed`].
    List(String),
    Done(RuntimeMarkers),
}

/// Looks for runtime markers from a file's directory up to the project root: a nested runtime wins,
/// and a package boundary (`package.json`) blocks a parent's runtime.
#[derive(Clone, Debug)]
pub struct MarkerSearch {
    root: String,
    dir: String,
}

impl MarkerSearch {
    pub fn start(root: Option<&str>, file: Option<&str>) -> (Self, MarkerStep) {
        let root = root.unwrap_or_default();
        let inside = |file: &&str| file.strip_prefix(root).is_some_and(|rest| rest.starts_with('/'));
        let dir = file.filter(inside).map_or(root, dir_of);
        let search = Self { root: root.to_string(), dir: dir.to_string() };
        let step = match root {
            "" => MarkerStep::Done(RuntimeMarkers::default()),
            _ => MarkerStep::List(search.dir.clone()),
        };
        (search, step)
    }

    /// The names in the directory; `None` when it cannot be read (it contributes no markers).
    pub fn listed<S: AsRef<str>>(&mut self, names: Option<&[S]>) -> MarkerStep {
        let has = |name: &str| names.is_some_and(|names| names.iter().any(|own| own.as_ref() == name));
        let (deno, bun) = (has("deno.json") || has("deno.jsonc"), has("bun.lockb") || has("bun.lock"));
        let markers = RuntimeMarkers { deno, bun };
        if markers.deno || markers.bun || has("package.json") {
            return MarkerStep::Done(markers);
        }
        if self.dir == self.root {
            return MarkerStep::Done(RuntimeMarkers::default());
        }
        self.dir = dir_of(&self.dir).to_string();
        let below = self.dir.strip_prefix(&self.root).is_some_and(|rest| rest.is_empty() || rest.starts_with('/'));
        if below { MarkerStep::List(self.dir.clone()) } else { MarkerStep::Done(RuntimeMarkers::default()) }
    }
}

/// Where the PHP interpreter of a run comes from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PhpSource {
    /// The production does not ask for one.
    #[default]
    None,
    /// The run target has it: `php` is run there.
    RunTarget,
    /// The interpreter chosen in settings, if any.
    Settings,
}

/// What to ask before a file runs with any of some tools.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ToolQuery {
    /// Tools to locate near `near`. Tools a run target supplies are left out: they resolve there.
    pub locate: Vec<String>,
    /// The directory of the file.
    pub near: Option<String>,
    /// Ask for the project's Python interpreter, as seen from `near`.
    pub python: bool,
    pub php: PhpSource,
    /// Look for the runtime markers of the file's package ([`MarkerSearch`]).
    pub markers: bool,
}

/// What the backend answered to a [`ToolQuery`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ToolAnswers {
    /// The located tools.
    pub tools: Located,
    /// Where single source files are compiled to.
    pub compile_dir: Option<String>,
    pub python: Option<String>,
    /// The PHP interpreter chosen in settings.
    pub php: Option<String>,
    pub markers: RuntimeMarkers,
}

/// The query for running `file` with any of `names`. `python` / `php`: the production may use the
/// project's interpreter.
pub fn tool_query(names: &[String], file: Option<&str>, python: bool, php: bool, settings: &RunSettings) -> ToolQuery {
    let on_target = |command: &str| {
        let config = RunConfig { command: command.to_string(), ..blank_config("", "", command) };
        with_run_target(&config, settings).extras.target.is_some_and(|target| !target.is_empty())
    };
    let supplied = |name: &&String| {
        let runtime = if matches!(name.as_str(), "tsx" | "ts-node") { "node" } else { name.as_str() };
        TARGET_TOOLS.contains(&name.as_str()) && on_target(runtime)
    };
    let source = if !php {
        PhpSource::None
    } else if on_target("php") {
        PhpSource::RunTarget
    } else {
        PhpSource::Settings
    };
    ToolQuery {
        locate: names.iter().filter(|name| !supplied(name)).cloned().collect(),
        near: file.map(|file| dir_of(file).to_string()),
        python,
        php: source,
        markers: names.iter().any(|name| name == "deno" || name == "bun"),
    }
}

impl ToolQuery {
    /// The tool environment the answers make. `windows`: the host that runs the commands is Windows.
    pub fn env(&self, answers: ToolAnswers, windows: bool) -> ToolEnv {
        ToolEnv {
            tools: answers.tools,
            compile_dir: answers.compile_dir.unwrap_or_else(|| DEFAULT_COMPILE_DIR.to_string()),
            windows,
            python: answers.python.filter(|_| self.python),
            php: match self.php {
                PhpSource::None => None,
                PhpSource::RunTarget => Some("php".to_string()),
                PhpSource::Settings => answers.php,
            },
            deno: answers.markers.deno,
            bun: answers.markers.bun,
        }
    }
}

/// When a tool was looked up, and what was found (`None`: the backend could not say).
type Cached = (f64, Option<Option<LocatedTool>>);

/// The tools located lately, per directory.
#[derive(Clone, Debug, Default)]
pub struct ToolCache {
    /// By directory and name.
    tools: HashMap<(String, String), Cached>,
}

impl ToolCache {
    pub fn clear(&mut self) {
        self.tools.clear();
    }

    /// What is remembered of `names` near `near`, and the names still to locate.
    pub fn lookup(&self, names: &[String], near: &str, now_ms: f64) -> (Located, Vec<String>) {
        let (mut known, mut todo) = (Located::new(), Vec::new());
        for name in names {
            let cached = self.tools.get(&(near.to_string(), name.clone()));
            match cached.filter(|(at, _)| now_ms - at < TOOL_TTL_MS) {
                Some((_, Some(tool))) => {
                    known.insert(name.clone(), tool.clone());
                }
                Some((_, None)) => {}
                None => todo.push(name.clone()),
            }
        }
        (known, todo)
    }

    /// Remembers what looking up `names` near `near` found.
    pub fn store(&mut self, names: &[String], near: &str, found: &Located, now_ms: f64) {
        for name in names {
            self.tools.insert((near.to_string(), name.clone()), (now_ms, found.get(name).cloned()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run_producers::producers::ToolLookup;

    fn names(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    /// The markers found for `file` over the listed directories (others are empty).
    fn root_markers(entries: &[(&str, &[&str])], root: &str, file: &str) -> RuntimeMarkers {
        let (mut search, mut step) = MarkerSearch::start(Some(root), Some(file));
        loop {
            match step {
                MarkerStep::Done(markers) => return markers,
                MarkerStep::List(dir) => {
                    let listed = entries.iter().find(|(listed, _)| *listed == dir).map(|(_, names)| *names);
                    step = search.listed(Some(listed.unwrap_or_default()));
                }
            }
        }
    }

    #[test]
    fn a_nested_runtime_wins_and_a_package_boundary_blocks_a_parents_runtime() {
        let entries: [(&str, &[&str]); 3] = [
            ("/repo", &["deno.json"]),
            ("/repo/apps/node", &["package.json"]),
            ("/repo/apps/bun", &["bun.lock", "package.json"]),
        ];
        let markers = |file: &str| root_markers(&entries, "/repo", file);
        assert_eq!(markers("/repo/apps/node/src/main.ts"), RuntimeMarkers { deno: false, bun: false });
        assert_eq!(markers("/repo/apps/bun/src/main.ts"), RuntimeMarkers { deno: false, bun: true });
        assert_eq!(markers("/repo/apps/deno/src/main.ts"), RuntimeMarkers { deno: true, bun: false });
    }

    #[test]
    fn a_nested_deno_project_is_discovered_under_a_node_root() {
        let entries: [(&str, &[&str]); 2] = [("/repo", &["package.json"]), ("/repo/apps/deno", &["deno.jsonc"])];
        let markers = root_markers(&entries, "/repo", "/repo/apps/deno/main.ts");
        assert_eq!(markers, RuntimeMarkers { deno: true, bun: false });
    }

    #[test]
    fn the_production_python_runtime_query_includes_the_source_directory() {
        let file = Some("/repo/apps/python/src/main.py");
        let query = tool_query(&names(&["python3"]), file, true, false, &RunSettings::default());
        assert_eq!((query.python, query.near.as_deref()), (true, Some("/repo/apps/python/src")));
        let answers = ToolAnswers { python: Some("/env/python".to_string()), ..ToolAnswers::default() };
        assert_eq!(query.env(answers, false).python.as_deref(), Some("/env/python"));
    }

    #[test]
    fn target_php_and_node_runs_do_not_require_locally_installed_interpreters() {
        let settings = RunSettings {
            php_target: "plugin-target:php".to_string(),
            node_target: "plugin-target:node".to_string(),
            ..RunSettings::default()
        };
        let query = tool_query(&names(&["php", "node", "npx", "tsx"]), Some("/repo/main.php"), false, true, &settings);
        assert!(query.locate.is_empty());
        let env = query.env(ToolAnswers::default(), false);
        assert_eq!(env.php.as_deref(), Some("php"));
        for tool in ["node", "npx", "tsx"] {
            assert_eq!(env.tool(tool), ToolLookup::Unknown, "{tool}");
        }
    }

    // Not from a spec: a located tool is remembered per directory for a minute, an unreadable
    // directory contributes no markers, and local tools are located when no run target supplies them.
    #[test]
    fn remembers_located_tools_per_directory() {
        let node = LocatedTool { path: "/bin/node".to_string(), version: Some("24.1.0".to_string()) };
        let found = BTreeMap::from([("node".to_string(), Some(node.clone())), ("tsx".to_string(), None)]);
        let asked = names(&["node", "tsx", "deno"]);
        let mut cache = ToolCache::default();
        assert_eq!(cache.lookup(&asked, "/repo/src", 0.0), (Located::new(), asked.clone()));
        cache.store(&asked, "/repo/src", &found, 0.0);
        // `deno` stays unknown: it is neither listed nor asked for again.
        assert_eq!(cache.lookup(&asked, "/repo/src", 59_999.0), (found, Vec::new()));
        assert_eq!(cache.lookup(&asked, "/repo/src", 60_000.0).1, asked);
        assert_eq!(cache.lookup(&asked, "/repo/other", 1.0).1, asked);
        let query = tool_query(&asked, Some("/repo/src/a.ts"), false, false, &RunSettings::default());
        assert_eq!((query.locate, query.php, query.markers), (asked, PhpSource::None, true));
        let (mut search, _) = MarkerSearch::start(Some("/repo"), Some("/repo/a.ts"));
        assert_eq!(search.listed::<&str>(None), MarkerStep::Done(RuntimeMarkers::default()));
        let (_, step) = MarkerSearch::start(None, Some("/repo/a.ts"));
        assert_eq!(step, MarkerStep::Done(RuntimeMarkers::default()));
    }
}
