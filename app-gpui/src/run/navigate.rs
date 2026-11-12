//! Opens file locations printed in run output (resolving relative and bare names).
//!
//! Resolving a location reads directories, the file index and sources. [`TargetResolver`] does no I/O:
//! it hands out the next [`ResolveStep`] and continues with its answer.

use crate::find::utf16;
use crate::platform::i18n::{I18n, Param};
use crate::run::java_launch::FoundSource;
use crate::run::links::{FileTarget, Resolve, join_path};
use crate::workbench::notifications::{NotificationLevel, NotificationRequest};
use regex::Regex;
use std::sync::LazyLock;

/// How many files of the target's name the file index is asked for.
pub const NAME_SEARCH_LIMIT: u32 = 50;
/// How much of a source file is read to find its package declaration, in UTF-16 code units.
const PACKAGE_SCAN: u32 = 20_000;
/// Symbol kinds that declare a type.
const TYPE_KINDS: [&str; 5] = ["class", "interface", "enum", "struct", "trait"];

/// Absolute run directory of a configuration (`cwd` is relative to the project root).
pub fn run_directory(project_root: Option<&str>, cwd: Option<&str>) -> String {
    let root = project_root.unwrap_or("/");
    match cwd.filter(|cwd| !cwd.is_empty()) {
        Some(cwd) => join_path(root, cwd),
        None => root.to_string(),
    }
}

fn package_of(class_name: &str) -> &str {
    class_name.rsplit_once('.').map_or("", |(package, _)| package)
}

/// Whether a JVM source declares `package` (Kotlin files needn't sit in their package's directory).
fn declares_package(text: &str, package: &str) -> bool {
    static DECLARATION: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?m)^\s*package\s+([A-Za-z0-9_.`]+)").expect("the package pattern is valid"));
    let head = utf16::slice(text, 0, PACKAGE_SCAN);
    let declared = DECLARATION.captures(head).and_then(|found| found.get(1)).map_or("", |name| name.as_str());
    declared.replace('`', "") == package
}

/// A workspace symbol as the language server reports it; `kind` is its serialised name (`class`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SymbolHit {
    pub name: String,
    pub kind: String,
    pub container: Option<String>,
    /// Where the symbol is: the URI of a virtual document, else the file path.
    pub location: String,
}

/// What resolving a location needs next.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResolveStep {
    /// Whether directory `dir` has an entry `name` that is not a directory (a failed listing: no);
    /// then [`TargetResolver::exists`].
    FileExists { dir: String, name: String },
    /// Ask the file index for `name`, then [`TargetResolver::files_found`].
    FindFiles { name: String, limit: u32 },
    /// Read the file, then [`TargetResolver::source_read`].
    ReadSource { path: String },
    /// Ask the language server for the workspace symbols matching `query`, then
    /// [`TargetResolver::symbols_found`].
    WorkspaceSymbols { query: String },
    /// The path (or virtual document URI) to open; `None` when nothing was found.
    Done(Option<String>),
}

#[derive(Clone, Debug)]
enum Phase {
    /// Checking the path against the run directory.
    InRunDirectory {
        candidate: String,
    },
    /// Checking the path against the project root.
    InProject {
        candidate: String,
    },
    ByName,
    /// Reading same-named sources until one declares the frame's package.
    ByPackage {
        candidates: Vec<String>,
        next: usize,
    },
    Library,
}

/// Resolves a printed location to a file: against the run directory, then the project root, then by
/// name through the file index, and for a JVM class outside the project through the language server.
#[derive(Clone, Debug)]
pub struct TargetResolver {
    target: FileTarget,
    project_root: Option<String>,
    phase: Phase,
}

fn exists_step(path: &str) -> ResolveStep {
    let (dir, name) = path.rsplit_once('/').unwrap_or(("", path));
    ResolveStep::FileExists { dir: if dir.is_empty() { "/" } else { dir }.to_string(), name: name.to_string() }
}

impl TargetResolver {
    /// Starts resolving `target`, printed by a process running in `cwd` (absolute).
    pub fn start(target: FileTarget, cwd: &str, project_root: Option<&str>) -> (Self, ResolveStep) {
        let mut resolver = Self { target, project_root: project_root.map(str::to_string), phase: Phase::ByName };
        if resolver.target.resolve != Resolve::Path {
            let step = resolver.find_by_name();
            return (resolver, step);
        }
        let candidate = join_path(cwd, &resolver.target.path);
        let step = exists_step(&candidate);
        resolver.phase = Phase::InRunDirectory { candidate };
        (resolver, step)
    }

    pub fn exists(&mut self, exists: bool) -> ResolveStep {
        match std::mem::replace(&mut self.phase, Phase::ByName) {
            Phase::InRunDirectory { candidate } | Phase::InProject { candidate } if exists => {
                ResolveStep::Done(Some(candidate))
            }
            Phase::InRunDirectory { candidate } => {
                // Paths printed relative to the project root (cargo in a workspace member, etc.).
                let from_root = self
                    .project_root
                    .as_deref()
                    .filter(|_| !self.target.path.starts_with('/'))
                    .map(|root| join_path(root, &self.target.path))
                    .filter(|from_root| *from_root != candidate);
                match from_root {
                    Some(from_root) => {
                        let step = exists_step(&from_root);
                        self.phase = Phase::InProject { candidate: from_root };
                        step
                    }
                    None => self.find_by_name(),
                }
            }
            _ => self.find_by_name(),
        }
    }

    /// The index answered (`None`: the search failed).
    pub fn files_found(&mut self, found: Option<&[FoundSource]>) -> ResolveStep {
        let Some(hits) = found else { return self.not_in_project() };
        let name = self.name();
        let tail = self.target.path.strip_prefix('.').unwrap_or(&self.target.path);
        let tail = tail.strip_prefix('/').unwrap_or(self.target.path.as_str());
        let exact = hits
            .iter()
            .find(|hit| hit.rel_path == tail || hit.rel_path.strip_suffix(tail).is_some_and(|dir| dir.ends_with('/')));
        if let Some(exact) = exact {
            return ResolveStep::Done(Some(exact.path.clone()));
        }
        let named: Vec<String> = hits
            .iter()
            .filter(|hit| hit.rel_path.rsplit('/').next() == Some(name))
            .map(|hit| hit.path.clone())
            .collect();
        // A stack frame names its class: a same-named file of another package is not it.
        if self.target.class_name.is_some() {
            self.phase = Phase::ByPackage { candidates: named, next: 0 };
            return self.next_candidate();
        }
        ResolveStep::Done(named.into_iter().next())
    }

    /// The source of the candidate was read (`None`: it could not be).
    pub fn source_read(&mut self, text: Option<&str>) -> ResolveStep {
        let Phase::ByPackage { candidates, next } = &mut self.phase else { return ResolveStep::Done(None) };
        let package = self.target.class_name.as_deref().map_or("", package_of);
        if text.is_some_and(|text| declares_package(text, package)) {
            return ResolveStep::Done(candidates.get(*next).cloned());
        }
        *next += 1;
        self.next_candidate()
    }

    /// The language server answered (`None`: the request failed).
    pub fn symbols_found(&mut self, symbols: Option<&[SymbolHit]>) -> ResolveStep {
        let Some((package, simple)) = self.library_class() else { return ResolveStep::Done(None) };
        let hit = symbols.unwrap_or_default().iter().find(|symbol| {
            symbol.name == simple
                && TYPE_KINDS.contains(&symbol.kind.as_str())
                && symbol.container.as_deref().unwrap_or_default() == package
        });
        ResolveStep::Done(hit.map(|symbol| symbol.location.clone()))
    }

    fn name(&self) -> &str {
        self.target.path.rsplit('/').next().unwrap_or(&self.target.path)
    }

    fn find_by_name(&mut self) -> ResolveStep {
        self.phase = Phase::ByName;
        ResolveStep::FindFiles { name: self.name().to_string(), limit: NAME_SEARCH_LIMIT }
    }

    fn next_candidate(&mut self) -> ResolveStep {
        match &self.phase {
            Phase::ByPackage { candidates, next } if *next < candidates.len() => {
                ResolveStep::ReadSource { path: candidates[*next].clone() }
            }
            _ => self.not_in_project(),
        }
    }

    /// The package and simple name of the frame's outer class.
    fn library_class(&self) -> Option<(&str, &str)> {
        let outer = self.target.class_name.as_deref()?.split('$').next()?;
        Some((package_of(outer), outer.rsplit('.').next().unwrap_or(outer)))
    }

    /// Library / JDK classes: the source the language server has for them (attached sources or
    /// decompiled), opened as a read-only virtual document.
    fn not_in_project(&mut self) -> ResolveStep {
        match self.library_class() {
            Some((_, simple)) => {
                let query = simple.to_string();
                self.phase = Phase::Library;
                ResolveStep::WorkspaceSymbols { query }
            }
            None => ResolveStep::Done(None),
        }
    }
}

/// Where the editor puts the caret: 0-based line and UTF-16 column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub line: u32,
    pub col: u32,
}

/// What clicking a location does once it was resolved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpenAction {
    OpenFile { path: String, selection: Option<Selection>, focus: bool },
    Notify(NotificationRequest),
}

fn link_notification(level: NotificationLevel, title: &str, body: String) -> OpenAction {
    let mut request = NotificationRequest::new(level, title);
    request.body = Some(body);
    request.key = Some("run-link".to_string());
    request.group = Some("run".to_string());
    OpenAction::Notify(request)
}

/// Opens the file a location resolved to (`path`), or tells why there is nothing to open.
pub fn open_resolved(target: &FileTarget, path: Option<&str>, focus: bool, i18n: &mut I18n) -> OpenAction {
    let Some(path) = path else {
        // A library or JDK frame without sources: nothing to open, which is no error.
        if let Some(class_name) = &target.class_name {
            let outer = class_name.split('$').next().unwrap_or(class_name);
            let body = i18n.tx_built(
                &["", " is not part of the project, and no library sources are available for it."],
                &[Param::from(outer)],
            );
            return link_notification(NotificationLevel::Info, "No sources for this class", body);
        }
        return link_notification(NotificationLevel::Warning, "File not found", target.path.clone());
    };
    let selection = target
        .line
        .filter(|line| *line > 0)
        .map(|line| Selection { line: line - 1, col: target.col.unwrap_or(1).saturating_sub(1) });
    OpenAction::OpenFile { path: path.to_string(), selection, focus }
}

/// Opens the definition found for a Minitest test (`site`: its file and 1-based line), or tells that
/// the search found none.
pub fn open_ruby_test(target: &FileTarget, site: Option<(&str, u32)>, focus: bool) -> OpenAction {
    match site {
        Some((path, line)) => OpenAction::OpenFile {
            path: path.to_string(),
            selection: Some(Selection { line: line.saturating_sub(1), col: 0 }),
            focus,
        },
        None => {
            let body = format!("{}#{}", target.class_name.as_deref().unwrap_or_default(), target.path);
            link_notification(NotificationLevel::Warning, "File not found", body)
        }
    }
}

/// The notification of a URL that could not be opened.
pub fn open_url_failed(error: &str) -> NotificationRequest {
    let mut request = NotificationRequest::new(NotificationLevel::Error, "Cannot open link");
    request.body = Some(error.to_string());
    request.group = Some("run".to_string());
    request
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::links::jvm_frame_target;

    const FILES: [(&str, &str); 3] = [
        ("/p/src/main/java/com/acme/Util.java", "package com.acme;\nclass Util {}"),
        ("/p/other/Util.java", "package org.other;\nclass Util {}"),
        ("/p/src/main/kotlin/Orders.kt", "package com.acme.orders\nfun main() {}"),
    ];

    /// Resolves `target` in the project `/p` holding [`FILES`], with `symbols` known to the language server.
    fn resolve(target: FileTarget, symbols: &[SymbolHit]) -> Option<String> {
        let (mut resolver, mut step) = TargetResolver::start(target, "/p", Some("/p"));
        loop {
            step = match step {
                ResolveStep::FileExists { dir, name } => {
                    resolver.exists(FILES.iter().any(|(path, _)| *path == format!("{dir}/{name}")))
                }
                ResolveStep::FindFiles { name, .. } => {
                    let found: Vec<FoundSource> = FILES
                        .iter()
                        .filter(|(path, _)| path.ends_with(&format!("/{name}")))
                        .map(|(path, _)| FoundSource { path: path.to_string(), rel_path: path[3..].to_string() })
                        .collect();
                    resolver.files_found(Some(&found))
                }
                ResolveStep::ReadSource { path } => {
                    resolver.source_read(FILES.iter().find(|(file, _)| *file == path).map(|(_, text)| *text))
                }
                ResolveStep::WorkspaceSymbols { .. } => resolver.symbols_found(Some(symbols)),
                ResolveStep::Done(path) => return path,
            };
        }
    }

    #[test]
    fn picks_the_file_of_the_frames_package_not_any_file_of_that_name() {
        let acme = jvm_frame_target(Some("com.acme.Util"), "Util.java", Some(3));
        assert_eq!(resolve(acme, &[]).as_deref(), Some("/p/src/main/java/com/acme/Util.java"));
        let other = jvm_frame_target(Some("org.other.Util"), "Util.java", Some(3));
        assert_eq!(resolve(other, &[]).as_deref(), Some("/p/other/Util.java"));
    }

    #[test]
    fn finds_kotlin_files_outside_their_package_directory_by_their_package_declaration() {
        let target = jvm_frame_target(Some("com.acme.orders.OrdersKt"), "Orders.kt", Some(1));
        assert_eq!(resolve(target, &[]).as_deref(), Some("/p/src/main/kotlin/Orders.kt"));
    }

    #[test]
    fn library_classes_open_the_language_servers_source_else_nothing() {
        let array_list = || jvm_frame_target(Some("java.util.ArrayList"), "ArrayList.java", Some(427));
        assert_eq!(resolve(array_list(), &[]), None);
        let symbols = [SymbolHit {
            name: "ArrayList".into(),
            kind: "class".into(),
            container: Some("java.util".into()),
            location: "jdt://contents/ArrayList.class".into(),
        }];
        assert_eq!(resolve(array_list(), &symbols).as_deref(), Some("jdt://contents/ArrayList.class"));
        // Same simple name in another package isn't it.
        assert_eq!(resolve(jvm_frame_target(Some("java.awt.List$X"), "List.java", Some(1)), &symbols), None);
    }

    // Not from a spec: a printed path is tried in the run directory, then in the project, then by name.
    #[test]
    fn printed_paths_resolve_against_the_run_directory_then_the_project() {
        let target = FileTarget {
            path: "src/main/kotlin/Orders.kt".into(),
            line: Some(2),
            col: Some(5),
            resolve: Resolve::Path,
            class_name: None,
        };
        let (mut resolver, step) = TargetResolver::start(target.clone(), "/p/app", Some("/p"));
        let exists = |dir: &str| ResolveStep::FileExists { dir: dir.into(), name: "Orders.kt".into() };
        assert_eq!(step, exists("/p/app/src/main/kotlin"));
        assert_eq!(resolver.exists(false), exists("/p/src/main/kotlin"));
        assert_eq!(resolver.exists(true), ResolveStep::Done(Some("/p/src/main/kotlin/Orders.kt".into())));
        let (mut resolver, _) = TargetResolver::start(target.clone(), "/p", Some("/p"));
        assert_eq!(resolver.exists(false), ResolveStep::FindFiles { name: "Orders.kt".into(), limit: 50 });
        assert_eq!(resolver.files_found(None), ResolveStep::Done(None));
        let mut i18n = I18n::default();
        let opened = open_resolved(&target, Some("/p/a.kt"), true, &mut i18n);
        let selection = Some(Selection { line: 1, col: 4 });
        assert_eq!(opened, OpenAction::OpenFile { path: "/p/a.kt".into(), selection, focus: true });
        assert_eq!(run_directory(Some("/p"), Some("app")), "/p/app");
        assert_eq!(run_directory(None, None), "/");
    }

    // Not from a spec: a missing file warns, a library class without sources only informs.
    #[test]
    fn unresolved_locations_notify() {
        let mut i18n = I18n::default();
        let frame = jvm_frame_target(Some("java.util.Map$Entry"), "Map.java", Some(1));
        let OpenAction::Notify(info) = open_resolved(&frame, None, true, &mut i18n) else { panic!("a notification") };
        assert_eq!((info.level, info.title.as_str()), (NotificationLevel::Info, "No sources for this class"));
        assert!(info.body.is_some_and(|body| body.starts_with("java.util.Map is not part of the project")));
        let ruby = FileTarget {
            path: "test_total".into(),
            line: None,
            col: None,
            resolve: Resolve::RubyTest,
            class_name: Some("CartTest".into()),
        };
        let OpenAction::Notify(warning) = open_ruby_test(&ruby, None, true) else { panic!("a notification") };
        assert_eq!(warning.body.as_deref(), Some("CartTest#test_total"));
        assert_eq!(warning.key.as_deref(), Some("run-link"));
        let opened = open_ruby_test(&ruby, Some(("/p/test/cart_test.rb", 12)), false);
        let selection = Some(Selection { line: 11, col: 0 });
        assert_eq!(opened, OpenAction::OpenFile { path: "/p/test/cart_test.rb".into(), selection, focus: false });
    }
}
