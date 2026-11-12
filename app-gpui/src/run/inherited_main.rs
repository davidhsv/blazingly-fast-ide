//! `main` inherited from a superclass (in the project or a library JAR): the JVM launches
//! `java -cp … Sub` fine when `Base` declares `static main`, but the scanner only sees the file's own
//! text. The language server is asked for the superclass chain once, and the answer is remembered for
//! the file's run targets.
//!
//! [`MainResolution`] does no I/O: it hands out the next [`ResolveStep`] and continues with its answer.

use crate::run::jvm_targets::{
    JvmMainCandidate, JvmTargetKind, jvm_main_candidates, scan_jvm_targets, simple_class_name,
};
use crate::run::targets::{RunTarget, TargetKind};
use ide_core::{Pos, SymbolKind};
use ide_lsp::types::HierarchyItem;
use std::collections::{HashMap, HashSet, VecDeque};

/// How many levels of superclasses are looked at.
const MAX_DEPTH: usize = 6;

fn key_of(class_name: &str, base: &str) -> String {
    format!("{class_name} extends {base}")
}

/// Where a hierarchy item's source is read from: its virtual document, else its file.
fn source_of(item: &HierarchyItem) -> String {
    item.uri.clone().unwrap_or_else(|| ide_core::ipc_path::to_ipc(&item.path))
}

/// What is known about inherited `main` methods.
#[derive(Clone, Debug, Default)]
pub struct InheritedMain {
    /// Whether a type declares a static `main` itself, by source and name.
    declares: HashMap<String, bool>,
    /// File → `class extends base` → inherits a static `main`.
    resolved: HashMap<String, HashMap<String, bool>>,
}

impl InheritedMain {
    /// After the server settles: ask again where it said "no" (it may have been indexing); keep the
    /// "yes" answers.
    pub fn retry_negative(&mut self) {
        self.declares.clear();
        for answers in self.resolved.values_mut() {
            answers.retain(|_, inherits| *inherits);
        }
    }

    pub fn clear(&mut self) {
        self.declares.clear();
        self.resolved.clear();
    }

    pub fn has_inherited_main(&self, path: &str, class_name: &str, base: &str) -> bool {
        self.resolved.get(path).and_then(|answers| answers.get(&key_of(class_name, base))) == Some(&true)
    }

    /// Candidates of `path` not answered yet.
    pub fn unresolved_candidates(&self, path: &str, text: &str) -> Vec<JvmMainCandidate> {
        let done = self.resolved.get(path);
        let answered = |candidate: &JvmMainCandidate| {
            done.is_some_and(|done| done.contains_key(&key_of(&candidate.class_name, &candidate.base)))
        };
        jvm_main_candidates(text, path).into_iter().filter(|candidate| !answered(candidate)).collect()
    }

    /// Records the answer of a [`MainResolution`] for a candidate of `path` (`None`: the server could
    /// not say, the candidate stays pending). True when a new inherited `main` was found.
    pub fn record(&mut self, path: &str, candidate: &JvmMainCandidate, inherits: Option<bool>) -> bool {
        let Some(inherits) = inherits else { return false };
        let key = key_of(&candidate.class_name, &candidate.base);
        self.resolved.entry(path.to_string()).or_default().insert(key, inherits);
        inherits
    }

    /// `main` targets for the file's classes whose superclass declares one (answers recorded earlier).
    pub fn inherited_run_targets(&self, path: &str, text: &str) -> Vec<RunTarget> {
        if self.resolved.get(path).is_none_or(HashMap::is_empty) {
            return Vec::new();
        }
        jvm_main_candidates(text, path)
            .into_iter()
            .filter(|candidate| self.has_inherited_main(path, &candidate.class_name, &candidate.base))
            .map(|candidate| RunTarget {
                class_name: Some(candidate.class_name.clone()),
                spring: candidate.spring,
                ..RunTarget::new(
                    candidate.line,
                    TargetKind::Main,
                    simple_class_name(&candidate.class_name),
                    vec![candidate.class_name.clone()],
                )
            })
            .collect()
    }
}

/// What resolving a candidate needs next.
#[derive(Clone, Debug)]
pub enum ResolveStep {
    /// Ask the language server for the supertypes of the type at `pos` of `path`, then
    /// [`MainResolution::supertypes_found`].
    Supertypes { path: String, pos: Pos },
    /// Read the source of the supertype (file, or decompiled / source-jar document), then
    /// [`MainResolution::text_read`].
    Text(Box<HierarchyItem>),
    /// Whether a superclass declares a static `main`; `None` when the server could not say (not
    /// ready), so the caller retries later.
    Done(Option<bool>),
}

/// Looks for a static `main` in the superclasses of a type, level by level. Interfaces are skipped: a
/// static interface `main` is not inherited.
#[derive(Clone, Debug)]
pub struct MainResolution {
    depth: usize,
    level: Vec<(String, Pos)>,
    next_in_level: usize,
    next_level: Vec<(String, Pos)>,
    /// Supertypes of the type being looked at, still to check.
    supers: VecDeque<HierarchyItem>,
    /// The supertype whose source is being read.
    reading: Option<HierarchyItem>,
    seen: HashSet<String>,
}

impl MainResolution {
    pub fn start(path: &str, pos: Pos) -> (Self, ResolveStep) {
        let resolution = Self {
            depth: 0,
            level: vec![(path.to_string(), pos)],
            next_in_level: 1,
            next_level: Vec::new(),
            supers: VecDeque::new(),
            reading: None,
            seen: HashSet::new(),
        };
        (resolution, ResolveStep::Supertypes { path: path.to_string(), pos })
    }

    /// The server answered (`None`: it could not).
    pub fn supertypes_found(&mut self, supers: Option<Vec<HierarchyItem>>, known: &mut InheritedMain) -> ResolveStep {
        match supers {
            Some(supers) => self.supers = supers.into(),
            None if self.depth == 0 => return ResolveStep::Done(None),
            None => {}
        }
        self.advance(known)
    }

    /// The source of the supertype was read (`None`: it is unreadable, which is not remembered: the
    /// document may become readable later).
    pub fn text_read(&mut self, text: Option<&str>, known: &mut InheritedMain) -> ResolveStep {
        let Some(item) = self.reading.take() else { return self.advance(known) };
        let declares = text.is_some_and(|text| {
            let name = item.name.split('<').next().unwrap_or_default();
            let declares = scan_jvm_targets(text, "", false).iter().any(|target| {
                target.kind == JvmTargetKind::Main && !target.instance && simple_class_name(&target.class_name) == name
            });
            known.declares.insert(format!("{}#{}", source_of(&item), item.name), declares);
            declares
        });
        if declares {
            return ResolveStep::Done(Some(true));
        }
        self.next_level.push((ide_core::ipc_path::to_ipc(&item.path), item.selection_range.start));
        self.advance(known)
    }

    fn advance(&mut self, known: &InheritedMain) -> ResolveStep {
        loop {
            while let Some(item) = self.supers.pop_front() {
                if item.kind != SymbolKind::Class {
                    continue;
                }
                let source = source_of(&item);
                if !self.seen.insert(format!("{source}:{}", item.selection_range.start.line)) {
                    continue;
                }
                match known.declares.get(&format!("{source}#{}", item.name)) {
                    Some(true) => return ResolveStep::Done(Some(true)),
                    Some(false) => {
                        self.next_level.push((ide_core::ipc_path::to_ipc(&item.path), item.selection_range.start));
                    }
                    None => {
                        self.reading = Some(item.clone());
                        return ResolveStep::Text(Box::new(item));
                    }
                }
            }
            if let Some((path, pos)) = self.level.get(self.next_in_level).cloned() {
                self.next_in_level += 1;
                return ResolveStep::Supertypes { path, pos };
            }
            self.depth += 1;
            self.level = std::mem::take(&mut self.next_level);
            self.next_in_level = 0;
            if self.depth >= MAX_DEPTH || self.level.is_empty() {
                return ResolveStep::Done(Some(false));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_core::Range;

    const MAIN: &str = "public static void main(String[] a) {}";
    const AT: Pos = Pos::new(3, 13);

    fn item(name: &str, path: &str, kind: SymbolKind) -> HierarchyItem {
        HierarchyItem {
            name: name.into(),
            kind,
            detail: None,
            path: path.into(),
            uri: None,
            range: Range::new(Pos::new(0, 0), Pos::new(5, 1)),
            selection_range: Range::new(Pos::new(0, 6), Pos::new(0, 6 + name.len() as u32)),
            from_ranges: Vec::new(),
        }
    }

    fn class(name: &str, path: &str) -> HierarchyItem {
        item(name, path, SymbolKind::Class)
    }

    /// What the language server and the file system answer: supertypes by path (`None`: the request
    /// fails) and source texts by path.
    struct Deps<'a> {
        supers: &'a [(&'a str, Option<Vec<HierarchyItem>>)],
        texts: &'a [(&'a str, &'a str)],
        supertype_requests: usize,
        text_reads: usize,
    }

    impl<'a> Deps<'a> {
        fn new(supers: &'a [(&'a str, Option<Vec<HierarchyItem>>)], texts: &'a [(&'a str, &'a str)]) -> Self {
            Self { supers, texts, supertype_requests: 0, text_reads: 0 }
        }

        fn resolve(&mut self, path: &str, known: &mut InheritedMain) -> Option<bool> {
            let (mut resolution, mut step) = MainResolution::start(path, AT);
            loop {
                step = match step {
                    ResolveStep::Supertypes { path, .. } => {
                        self.supertype_requests += 1;
                        let answer = self.supers.iter().find(|(of, _)| *of == path).map(|(_, supers)| supers.clone());
                        resolution.supertypes_found(answer.unwrap_or(Some(Vec::new())), known)
                    }
                    ResolveStep::Text(item) => {
                        self.text_reads += 1;
                        let source = source_of(&item);
                        let text = self.texts.iter().find(|(of, _)| *of == source).map(|(_, text)| *text);
                        resolution.text_read(text, known)
                    }
                    ResolveStep::Done(answer) => return answer,
                };
            }
        }
    }

    #[test]
    fn finds_main_in_a_direct_superclass_inside_a_jar() {
        let jar = "jdt://contents/lib.jar/pkg/AtomStarter.class";
        let supers = [("/p/App.java", Some(vec![class("AtomStarter", jar)]))];
        let source = format!("package pkg; class AtomStarter {{ {MAIN} }}");
        let texts = [(jar, source.as_str())];
        assert_eq!(Deps::new(&supers, &texts).resolve("/p/App.java", &mut InheritedMain::default()), Some(true));
    }

    #[test]
    fn walks_two_levels() {
        let supers = [
            ("/p/App.java", Some(vec![class("Mid", "/p/Mid.java")])),
            ("/p/Mid.java", Some(vec![class("Base", "/p/Base.java")])),
        ];
        let base = format!("class Base {{ {MAIN} }}");
        let texts = [("/p/Mid.java", "class Mid extends Base {}"), ("/p/Base.java", base.as_str())];
        assert_eq!(Deps::new(&supers, &texts).resolve("/p/App.java", &mut InheritedMain::default()), Some(true));
    }

    #[test]
    fn ignores_interfaces_and_instance_main() {
        let interface = item("Iface", "/p/Iface.java", SymbolKind::Interface);
        let supers = [("/p/App.java", Some(vec![interface, class("Inst", "/p/Inst.java")]))];
        let with_main = format!("interface Iface {{ {MAIN} }}");
        let texts = [("/p/Iface.java", with_main.as_str()), ("/p/Inst.java", "class Inst { void main() {} }")];
        assert_eq!(Deps::new(&supers, &texts).resolve("/p/App.java", &mut InheritedMain::default()), Some(false));
    }

    #[test]
    fn stops_on_cycles() {
        let supers =
            [("/p/A.java", Some(vec![class("B", "/p/B.java")])), ("/p/B.java", Some(vec![class("B", "/p/B.java")]))];
        let texts = [("/p/B.java", "class B {}")];
        let mut deps = Deps::new(&supers, &texts);
        assert_eq!(deps.resolve("/p/A.java", &mut InheritedMain::default()), Some(false));
        assert_eq!(deps.supertype_requests, 2);
    }

    #[test]
    fn is_unknown_when_the_server_cannot_answer_false_when_a_deeper_level_fails() {
        let failing = [("/p/A.java", None)];
        assert_eq!(Deps::new(&failing, &[]).resolve("/p/A.java", &mut InheritedMain::default()), None);
        let supers = [("/p/A.java", Some(vec![class("B", "/p/B.java")])), ("/p/B.java", None)];
        let texts = [("/p/B.java", "class B {}")];
        assert_eq!(Deps::new(&supers, &texts).resolve("/p/A.java", &mut InheritedMain::default()), Some(false));
    }

    #[test]
    fn treats_an_unreadable_supertype_as_no_main() {
        let supers = [("/p/A.java", Some(vec![class("B", "/p/B.java")]))];
        assert_eq!(Deps::new(&supers, &[]).resolve("/p/A.java", &mut InheritedMain::default()), Some(false));
    }

    #[test]
    fn reads_each_supertypes_text_once() {
        let supers =
            [("/p/A.java", Some(vec![class("B", "/p/B.java")])), ("/p/C.java", Some(vec![class("B", "/p/B.java")]))];
        let texts = [("/p/B.java", "class B {}")];
        let (mut deps, mut known) = (Deps::new(&supers, &texts), InheritedMain::default());
        deps.resolve("/p/A.java", &mut known);
        deps.resolve("/p/C.java", &mut known);
        assert_eq!(deps.text_reads, 1);
    }

    const SRC: &str = "package demo;\n\npublic class App extends Base {}\n";

    /// Resolves the pending candidates of `/p/App.java`; true when a new inherited `main` was found.
    fn resolve_candidates(deps: &mut Deps, known: &mut InheritedMain, text: &str) -> bool {
        let mut found = false;
        for candidate in known.unresolved_candidates("/p/App.java", text) {
            let answer = deps.resolve("/p/App.java", known);
            found |= known.record("/p/App.java", &candidate, answer);
        }
        found
    }

    #[test]
    fn records_answers_for_the_files_targets_and_skips_them_next_time() {
        let supers = [("/p/App.java", Some(vec![class("Base", "/p/Base.java")]))];
        let base = format!("class Base {{ {MAIN} }}");
        let texts = [("/p/Base.java", base.as_str())];
        let (mut deps, mut known) = (Deps::new(&supers, &texts), InheritedMain::default());
        assert_eq!(known.unresolved_candidates("/p/App.java", SRC).len(), 1);
        assert!(resolve_candidates(&mut deps, &mut known, SRC));
        assert!(known.has_inherited_main("/p/App.java", "demo.App", "Base"));
        assert!(known.unresolved_candidates("/p/App.java", SRC).is_empty());
        let targets = known.inherited_run_targets("/p/App.java", SRC);
        assert_eq!(targets.iter().map(|t| (t.line, t.name.as_str())).collect::<Vec<_>>(), [(2, "App")]);
    }

    #[test]
    fn asks_again_when_the_extends_clause_changes() {
        let supers = [("/p/App.java", Some(vec![class("Base", "/p/Base.java")]))];
        let base = format!("class Base {{ {MAIN} }}");
        let texts = [("/p/Base.java", base.as_str())];
        let (mut deps, mut known) = (Deps::new(&supers, &texts), InheritedMain::default());
        resolve_candidates(&mut deps, &mut known, SRC);
        let edited = SRC.replace("extends Base", "extends Other");
        assert!(!known.has_inherited_main("/p/App.java", "demo.App", "Other"));
        assert_eq!(known.unresolved_candidates("/p/App.java", &edited).len(), 1);
    }

    #[test]
    fn leaves_a_candidate_pending_while_the_server_is_unavailable() {
        let supers = [("/p/App.java", None)];
        let (mut deps, mut known) = (Deps::new(&supers, &[]), InheritedMain::default());
        assert!(!resolve_candidates(&mut deps, &mut known, SRC));
        assert_eq!(known.unresolved_candidates("/p/App.java", SRC).len(), 1);
    }

    // Not from a spec: after the server settles, a "no" is asked again while a "yes" is kept.
    #[test]
    fn retrying_forgets_the_negative_answers_only() {
        let mut known = InheritedMain::default();
        let candidates = jvm_main_candidates("class A extends X {}\nclass B extends Y {}", "/p/A.java");
        assert!(known.record("/p/A.java", &candidates[0], Some(true)));
        assert!(!known.record("/p/A.java", &candidates[1], Some(false)));
        known.retry_negative();
        let pending = known.unresolved_candidates("/p/A.java", "class A extends X {}\nclass B extends Y {}");
        assert_eq!(pending.iter().map(|candidate| candidate.class_name.as_str()).collect::<Vec<_>>(), ["B"]);
        known.clear();
        assert!(!known.has_inherited_main("/p/A.java", "A", "X"));
    }
}
