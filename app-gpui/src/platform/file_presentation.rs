//! Visible-file JVM presentation: the icon a Java or Kotlin file gets from what it declares, and
//! the resolver that asks the backend about the files on screen in bounded batches and caches
//! the answers for tree rows and tab strips alike.

use crate::platform::project_roots::{FolderRole, RootFolder, root_for};
use crate::platform::virtual_docs::{basename, relative_path};
use crate::theme::icons::file_icon_name;
use std::collections::{BTreeMap, HashMap};

/// The most files one request asks about.
pub const MAX_BATCH: usize = 64;
/// The most answers, and the most modification times, kept.
pub const MAX_CACHE: usize = 4096;

/// What a JVM source file declares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JvmTypeKind {
    Class,
    AbstractClass,
    Interface,
    Enum,
    Record,
    Annotation,
    Exception,
    Object,
}

/// The backend's view of a JVM source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JvmPresentation {
    /// `None` for a file without a type declaration.
    pub kind: Option<JvmTypeKind>,
    pub spring_boot: bool,
    pub runnable: bool,
}

/// The answer for one file of a batch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JvmFilePresentation {
    pub path: String,
    pub presentation: JvmPresentation,
}

/// Whether the backend can say what `path` declares.
pub fn is_jvm_file(path: &str) -> bool {
    path.ends_with(".java") || path.ends_with(".kt")
}

/// `application*.yml`, `bootstrap*.properties` and the like, on a single line.
fn is_spring_config_name(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("application").or_else(|| name.strip_prefix("bootstrap")) else { return false };
    [".yml", ".yaml", ".properties"]
        .into_iter()
        .filter_map(|extension| rest.strip_suffix(extension))
        .any(|middle| !middle.contains(['\n', '\r', '\u{2028}', '\u{2029}']))
}

/// `FooTest`, `FooTests` and `FooIT` sources; an all-caps name such as `AUDIT` is no test.
fn is_test_class_name(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".java").or_else(|| name.strip_suffix(".kt")) else { return false };
    let integration_test = |before: &str| before.ends_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit());
    stem.ends_with("Test") || stem.ends_with("Tests") || stem.strip_suffix("IT").is_some_and(integration_test)
}

/// The icon of the file `path`: Spring configuration, the type it declares, a test class, the
/// Spring Boot application, else the icon of its file type. `info` is the backend's answer, if it
/// has one yet; `roots` are the effective roots of the project at `root`; `spring` is whether the
/// build uses Spring.
pub fn presentation_icon(
    path: &str,
    info: Option<&JvmPresentation>,
    roots: &[RootFolder],
    root: Option<&str>,
    spring: bool,
) -> &'static str {
    let name = basename(path);
    let role = root
        .filter(|root| !root.is_empty())
        .and_then(|root| root_for(roots, &relative_path(root, path)))
        .map(|folder| folder.kind);
    let in_resources = matches!(role, Some(FolderRole::Resources | FolderRole::TestResources));
    if is_spring_config_name(&name) && (spring || in_resources) {
        return "spring-config";
    }
    let Some(kind) = info.and_then(|info| info.kind) else { return file_icon_name(&name) };
    if info.is_some_and(|info| info.spring_boot) {
        return "spring-boot";
    }
    if is_test_class_name(&name) || role == Some(FolderRole::Test) {
        return "jvm-test";
    }
    match kind {
        JvmTypeKind::Class => "class",
        JvmTypeKind::AbstractClass => "class-abstract",
        JvmTypeKind::Interface => "interface",
        JvmTypeKind::Enum => "enum",
        JvmTypeKind::Record => "record",
        JvmTypeKind::Annotation => "annotation",
        JvmTypeKind::Exception => "exception",
        JvmTypeKind::Object => "kotlin-object",
    }
}

/// How a file is shown in a tree row or a list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FilePresentation {
    pub icon: &'static str,
    /// The file gets a run marker of its own; Spring Boot applications and tests have theirs in
    /// the icon.
    pub runnable: bool,
}

pub fn file_presentation(
    path: &str,
    info: Option<&JvmPresentation>,
    roots: &[RootFolder],
    root: Option<&str>,
    spring: bool,
) -> FilePresentation {
    let icon = presentation_icon(path, info, roots, root, spring);
    let runnable = info.is_some_and(|info| info.runnable) && icon != "spring-boot" && icon != "jvm-test";
    FilePresentation { icon, runnable }
}

/// A map that lists its keys in the order they were first set.
#[derive(Debug)]
struct Ordered<V> {
    slots: HashMap<String, (u64, V)>,
    order: BTreeMap<u64, String>,
    next: u64,
}

impl<V> Default for Ordered<V> {
    fn default() -> Self {
        Self { slots: HashMap::new(), order: BTreeMap::new(), next: 0 }
    }
}

impl<V> Ordered<V> {
    fn get(&self, key: &str) -> Option<&V> {
        self.slots.get(key).map(|(_, value)| value)
    }

    fn contains(&self, key: &str) -> bool {
        self.slots.contains_key(key)
    }

    fn len(&self) -> usize {
        self.slots.len()
    }

    fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// Sets the value of `key`. A key already present keeps its place.
    fn set(&mut self, key: &str, value: V) {
        if let Some(slot) = self.slots.get_mut(key) {
            slot.1 = value;
            return;
        }
        self.order.insert(self.next, key.to_owned());
        self.slots.insert(key.to_owned(), (self.next, value));
        self.next += 1;
    }

    fn remove(&mut self, key: &str) {
        if let Some((sequence, _)) = self.slots.remove(key) {
            self.order.remove(&sequence);
        }
    }

    fn remove_oldest(&mut self) {
        if let Some((_, key)) = self.order.pop_first() {
            self.slots.remove(&key);
        }
    }

    fn clear(&mut self) {
        self.slots.clear();
        self.order.clear();
    }

    fn keys(&self) -> impl Iterator<Item = &str> {
        self.order.values().map(String::as_str)
    }
}

/// The files of one request to the backend. It goes back to
/// [`JvmPresentations::finish_flush`] with the answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Batch {
    epoch: u64,
    /// Each path with the version it was asked at.
    files: Vec<(String, u64)>,
}

impl Batch {
    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.files.iter().map(|(path, _)| path.as_str())
    }
}

fn at_or_below(path: &str, folder: &str) -> bool {
    path.strip_prefix(folder).is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

/// What the JVM files on screen declare, as far as the backend has answered.
///
/// Consumers [`mount`](Self::mount) the files they show. While
/// [`flush_scheduled`](Self::flush_scheduled) is set, the owner calls
/// [`start_flush`](Self::start_flush) on its next turn, sends the batch to the backend and hands
/// the answer to [`finish_flush`](Self::finish_flush). File-system changes go to
/// [`invalidate`](Self::invalidate), another project to
/// [`project_changed`](Self::project_changed).
#[derive(Debug, Default)]
pub struct JvmPresentations {
    /// Answers by path. `None`: the backend had no answer, so the file keeps its plain icon.
    entries: Ordered<Option<JvmPresentation>>,
    /// Paths to ask about, with their version when they were queued.
    pending: Ordered<u64>,
    /// How many consumers show each path.
    visible: Ordered<usize>,
    /// Bumped when a path's answer goes stale, so that an answer already under way is dropped.
    versions: HashMap<String, u64>,
    mtimes: Ordered<f64>,
    flush_scheduled: bool,
    running: bool,
    /// Bumped when every answer under way became irrelevant.
    epoch: u64,
    users: usize,
}

impl JvmPresentations {
    fn version(&self, path: &str) -> u64 {
        self.versions.get(path).copied().unwrap_or(0)
    }

    fn enqueue(&mut self, path: &str) {
        if !is_jvm_file(path) || self.entries.contains(path) {
            return;
        }
        let version = self.version(path);
        self.pending.set(path, version);
        if !self.running {
            self.flush_scheduled = true;
        }
    }

    /// A consumer shows `path`. The first one starts from an empty cache.
    pub fn mount(&mut self, path: &str) {
        if !is_jvm_file(path) {
            return;
        }
        if self.users == 0 {
            self.entries.clear();
            self.mtimes.clear();
        }
        self.users += 1;
        let count = self.visible.get(path).copied().unwrap_or(0) + 1;
        self.visible.set(path, count);
        self.enqueue(path);
    }

    /// A consumer no longer shows `path`. A file nobody shows is not asked about.
    pub fn unmount(&mut self, path: &str) {
        if !is_jvm_file(path) {
            return;
        }
        match self.visible.get(path).copied().unwrap_or(1).saturating_sub(1) {
            0 => {
                self.visible.remove(path);
                self.pending.remove(path);
            }
            count => self.visible.set(path, count),
        }
        self.users = self.users.saturating_sub(1);
        if self.users == 0 {
            self.epoch += 1;
            self.pending.clear();
        }
    }

    /// What the backend said about `path`; `None` while it has not answered, or had no answer.
    pub fn presentation(&self, path: &str) -> Option<&JvmPresentation> {
        self.entries.get(path).and_then(Option::as_ref)
    }

    /// Whether [`start_flush`](Self::start_flush) is due.
    pub fn flush_scheduled(&self) -> bool {
        self.flush_scheduled
    }

    /// The next files to ask the backend about: the first [`MAX_BATCH`] queued paths still on
    /// screen. `None` while a request is under way, or when nothing on screen is queued.
    pub fn start_flush(&mut self) -> Option<Batch> {
        self.flush_scheduled = false;
        if self.running {
            return None;
        }
        let files: Vec<(String, u64)> = self
            .pending
            .keys()
            .filter(|path| self.visible.contains(path))
            .take(MAX_BATCH)
            .map(|path| (path.to_owned(), self.pending.get(path).copied().unwrap_or(0)))
            .collect();
        self.pending.clear();
        if files.is_empty() {
            return None;
        }
        self.running = true;
        Some(Batch { epoch: self.epoch, files })
    }

    /// Takes the backend's answer to `batch`; `None` when the request failed. A failure is
    /// cached like an answer, so that a batch on screen is not retried forever. Answers for
    /// files invalidated meanwhile are dropped, and those files asked about again.
    pub fn finish_flush(&mut self, batch: &Batch, results: Option<&[JvmFilePresentation]>) {
        if batch.epoch == self.epoch {
            for (path, version) in &batch.files {
                if self.version(path) != *version {
                    continue;
                }
                if let Some(results) = results {
                    // An entry answered again becomes the newest one.
                    self.entries.remove(path);
                    let answer = results.iter().find(|result| result.path == *path).map(|result| result.presentation);
                    self.entries.set(path, answer);
                } else {
                    self.entries.set(path, None);
                }
            }
            if results.is_some() {
                while self.entries.len() > MAX_CACHE {
                    self.entries.remove_oldest();
                }
            }
        }
        self.running = false;
        let visible = &self.visible;
        self.versions.retain(|path, _| visible.contains(path));
        // Requests that arrived during the request stay bounded to the mounted consumers.
        let unasked: Vec<String> =
            self.visible.keys().filter(|path| !batch.paths().any(|asked| asked == *path)).map(str::to_owned).collect();
        for path in &unasked {
            self.enqueue(path);
        }
        if !self.pending.is_empty() {
            self.flush_scheduled = true;
        }
    }

    /// Forgets the answers for `paths` and everything below them, and asks again about the ones
    /// on screen.
    pub fn invalidate<P: AsRef<str>>(&mut self, paths: &[P]) {
        let stale: Vec<String> = self
            .entries
            .keys()
            .chain(self.visible.keys().filter(|key| !self.entries.contains(key)))
            .filter(|key| paths.iter().any(|path| at_or_below(key, path.as_ref())))
            .map(str::to_owned)
            .collect();
        for key in stale {
            self.entries.remove(&key);
            *self.versions.entry(key).or_default() += 1;
        }
        let visible: Vec<String> = self.visible.keys().map(str::to_owned).collect();
        for path in &visible {
            self.enqueue(path);
        }
    }

    /// Another project was opened: nothing known or under way applies to it.
    pub fn project_changed(&mut self) {
        self.epoch += 1;
        self.versions.clear();
        self.pending.clear();
        self.entries.clear();
    }

    /// Records the modification time a directory listing reports for `path`. A changed time
    /// invalidates the answer even if the watcher event was missed.
    pub fn note_mtime(&mut self, path: &str, mtime: f64) {
        if !is_jvm_file(path) {
            return;
        }
        let previous = self.mtimes.get(path).copied();
        self.mtimes.set(path, mtime);
        if previous.is_some_and(|previous| previous != mtime) {
            self.invalidate(&[path]);
        }
        if self.mtimes.len() > MAX_CACHE {
            self.mtimes.remove_oldest();
        }
    }
}

/// The icons of `paths`, for a tab strip: its file tabs resolve through the same batches as tree
/// rows.
pub fn presentation_icons<'p>(
    paths: &[&'p str],
    resolved: &JvmPresentations,
    roots: &[RootFolder],
    root: Option<&str>,
    spring: bool,
) -> HashMap<&'p str, &'static str> {
    paths
        .iter()
        .map(|path| (*path, presentation_icon(path, resolved.presentation(path), roots, root, spring)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roots() -> Vec<RootFolder> {
        [
            ("src/main/java", FolderRole::Source),
            ("src/test/java", FolderRole::Test),
            ("src/main/resources", FolderRole::Resources),
            ("src/test/resources", FolderRole::TestResources),
        ]
        .map(|(path, kind)| RootFolder { path: path.to_owned(), kind, detected_from: Some("pom.xml".to_owned()) })
        .into()
    }

    fn of_kind(kind: Option<JvmTypeKind>) -> JvmPresentation {
        JvmPresentation { kind, spring_boot: false, runnable: false }
    }

    fn class() -> Option<JvmPresentation> {
        Some(of_kind(Some(JvmTypeKind::Class)))
    }

    fn icon(path: &str, info: Option<JvmPresentation>, spring: bool) -> &'static str {
        presentation_icon(&format!("/p/{path}"), info.as_ref(), &roots(), Some("/p"), spring)
    }

    // src/platform/filePresentation.test.ts › maps %s to %s
    #[test]
    fn maps_each_kind_to_its_icon() {
        for (kind, expected) in [
            (JvmTypeKind::Class, "class"),
            (JvmTypeKind::AbstractClass, "class-abstract"),
            (JvmTypeKind::Interface, "interface"),
            (JvmTypeKind::Enum, "enum"),
            (JvmTypeKind::Record, "record"),
            (JvmTypeKind::Annotation, "annotation"),
            (JvmTypeKind::Exception, "exception"),
            (JvmTypeKind::Object, "kotlin-object"),
        ] {
            assert_eq!(icon("Foo.java", Some(of_kind(Some(kind))), false), expected);
        }
    }

    // src/platform/filePresentation.test.ts › keeps the type icon of %s
    #[test]
    fn keeps_the_type_icon_of_names_that_only_look_like_tests() {
        for path in ["LIMIT.java", "AUDIT.java", "Contest.java"] {
            assert_eq!(icon(path, class(), false), "class", "{path}");
        }
    }

    // src/platform/filePresentation.test.ts › marks test classes: %s
    #[test]
    fn marks_test_classes() {
        for path in ["FooTest.java", "FooTests.java", "FooIT.java", "FooTest.kt", "src/test/java/Anything.java"] {
            assert_eq!(icon(path, class(), false), "jvm-test", "{path}");
        }
    }

    // src/platform/filePresentation.test.ts › keeps files without declarations and binaries as file types
    #[test]
    fn keeps_files_without_declarations_and_binaries_as_file_types() {
        assert_eq!(icon("Foo.java", Some(of_kind(None)), false), "file-java");
        assert_eq!(icon("FooTest.java", Some(of_kind(None)), false), "file-java");
        assert_eq!(icon("Foo.kt", None, false), "file-kotlin");
        assert_eq!(icon("Foo.class", None, false), "file-binary");
        assert_eq!(icon("Contest.java", class(), false), "class");
    }

    // src/platform/filePresentation.test.ts › requires Spring or a resource root: %s
    #[test]
    fn requires_spring_or_a_resource_root() {
        for name in ["application.yml", "application-dev.yaml", "bootstrap.properties", "bootstrap-local.yml"] {
            assert_eq!(icon(&format!("src/main/resources/{name}"), None, false), "spring-config", "{name}");
            assert_eq!(icon(&format!("src/test/resources/{name}"), None, false), "spring-config", "{name}");
            assert_eq!(icon(name, None, true), "spring-config", "{name}");
            assert_ne!(icon(name, None, false), "spring-config", "{name}");
        }
    }

    // src/platform/filePresentation.test.ts › uses the entire Boot icon on application classes
    #[test]
    fn uses_the_entire_boot_icon_on_application_classes() {
        let app = JvmPresentation { kind: Some(JvmTypeKind::Class), spring_boot: true, runnable: true };
        assert_eq!(icon("App.java", Some(app), false), "spring-boot");
    }

    fn answers(batch: &Batch, kind: JvmTypeKind) -> Vec<JvmFilePresentation> {
        let answer = |path: &str| JvmFilePresentation { path: path.to_owned(), presentation: of_kind(Some(kind)) };
        batch.paths().map(answer).collect()
    }

    /// Runs every flush that comes due, answering each file as an interface; the batch sizes.
    fn drain(resolver: &mut JvmPresentations) -> Vec<usize> {
        let mut sizes = Vec::new();
        while resolver.flush_scheduled() {
            let Some(batch) = resolver.start_flush() else { continue };
            sizes.push(batch.paths().count());
            resolver.finish_flush(&batch, Some(answers(&batch, JvmTypeKind::Interface).as_slice()));
        }
        sizes
    }

    fn kind_of(resolver: &JvmPresentations, path: &str) -> Option<JvmTypeKind> {
        resolver.presentation(path).and_then(|presentation| presentation.kind)
    }

    /// Not from a spec: only JVM files on screen are asked about, 64 at a time.
    #[test]
    fn asks_about_the_files_on_screen_in_bounded_batches() {
        let mut resolver = JvmPresentations::default();
        resolver.mount("/p/Invisible.java");
        resolver.unmount("/p/Invisible.java");
        resolver.mount("/p/config.yml");
        let files: Vec<String> = (0..130_u32).map(|i| format!("/p/F{i}.java")).collect();
        for file in &files {
            resolver.mount(file);
        }
        let tabs: Vec<&str> = files.iter().map(String::as_str).collect();
        assert_eq!(presentation_icons(&tabs, &resolver, &[], Some("/p"), false)["/p/F0.java"], "file-java");
        assert_eq!(drain(&mut resolver), [64, 64, 2]);
        assert_eq!(kind_of(&resolver, "/p/F129.java"), Some(JvmTypeKind::Interface));
        assert_eq!(presentation_icons(&tabs, &resolver, &[], Some("/p"), false)["/p/F0.java"], "interface");
        assert_eq!(resolver.presentation("/p/Invisible.java"), None);
        assert_eq!(resolver.presentation("/p/config.yml"), None);
    }

    /// Not from a spec: a change during a request drops its answer; a changed mtime asks again.
    #[test]
    fn drops_stale_answers_and_asks_again() {
        let mut resolver = JvmPresentations::default();
        resolver.mount("/p/Foo.java");
        let stale = resolver.start_flush().expect("a batch");
        assert_eq!(resolver.start_flush(), None);
        resolver.invalidate(&["/p"]);
        resolver.finish_flush(&stale, Some(answers(&stale, JvmTypeKind::Class).as_slice()));
        assert_eq!(resolver.presentation("/p/Foo.java"), None);
        assert_eq!(drain(&mut resolver), [1]);
        assert_eq!(kind_of(&resolver, "/p/Foo.java"), Some(JvmTypeKind::Interface));

        resolver.note_mtime("/p/Foo.java", 1.0);
        resolver.note_mtime("/p/Foo.java", 1.0);
        assert!(!resolver.flush_scheduled());
        resolver.note_mtime("/p/Foo.java", 2.0);
        assert_eq!(resolver.presentation("/p/Foo.java"), None);
        assert_eq!(drain(&mut resolver), [1]);

        assert_eq!(resolver.start_flush(), None);
        resolver.invalidate(&["/p/Foo.java"]);
        let request = resolver.start_flush().expect("a batch");
        resolver.project_changed();
        resolver.finish_flush(&request, Some(answers(&request, JvmTypeKind::Class).as_slice()));
        assert_eq!(resolver.presentation("/p/Foo.java"), None);
    }

    /// Not from a spec: a failed request is cached, not retried while the files stay on screen.
    #[test]
    fn caches_failures() {
        let mut resolver = JvmPresentations::default();
        let files: Vec<String> = (0..65_u32).map(|i| format!("/p/F{i}.java")).collect();
        for file in &files {
            resolver.mount(file);
        }
        let mut requests = 0_usize;
        while resolver.flush_scheduled() {
            if let Some(batch) = resolver.start_flush() {
                requests += 1;
                resolver.finish_flush(&batch, None);
            }
        }
        assert_eq!(requests, 2);
        assert_eq!(resolver.presentation("/p/F0.java"), None);
    }

    /// Not from a spec: the run marker goes to runnable files whose icon does not already say so.
    #[test]
    fn marks_runnable_files() {
        let main = JvmPresentation { kind: Some(JvmTypeKind::Class), spring_boot: false, runnable: true };
        let roots = roots();
        let shown = |path: &str, info: JvmPresentation| file_presentation(path, Some(&info), &roots, Some("/p"), false);
        assert_eq!(shown("/p/Main.java", main), FilePresentation { icon: "class", runnable: true });
        assert_eq!(shown("/p/MainTest.java", main), FilePresentation { icon: "jvm-test", runnable: false });
        let app = JvmPresentation { spring_boot: true, ..main };
        assert_eq!(shown("/p/App.java", app), FilePresentation { icon: "spring-boot", runnable: false });
        assert!(!file_presentation("/p/Main.java", None, &roots, Some("/p"), false).runnable);
    }
}
