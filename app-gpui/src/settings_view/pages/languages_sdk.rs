//! The toolchains the language pages offer and the questions those pages ask the backend.
//!
//! The installations of each kind are detected once a page that offers them is first shown, not
//! when Settings opens, and kept while the dialog is open ([`ToolchainCatalog`], emptied for each
//! Settings opened), so switching pages does not probe the toolchains again; Detect again asks
//! anew. The SDK combo
//! ([`SdkFieldView`]) lists the empty choice, the detected installations and a value set by hand,
//! with Detect again and Browse…; the path field ([`PathFieldView`]) trims what is typed and
//! offers Browse…. Views own no backend: they ask through [`LanguageEvent`], and [`answer`] asks
//! the backend off the window's thread.
//!
//! Test builds never probe a toolchain, look a tool up on the PATH or run an interpreter: the
//! answers come from the [`TestToolchains`] a test sets, and are empty without one.

use super::{
    Binding, ControlEvent, Then, WIDE_FIELD, claim_field_keys, drop_down, drop_down_row, frame, hint_line, muted,
    new_field, row, row_label, select_box,
};
use crate::platform::input;
use crate::settings_model::language_pages::tildify;
use crate::settings_view::controls::{button, tx};
use crate::ui::text_field::{TextField, TextFieldEvent};
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Entity, EventEmitter, Global, IntoElement, Keystroke, MouseButton, Render,
    SharedString, Subscription, Window, div, prelude::*,
};
use serde::Deserialize;
use serde_json::Value;
use std::path::PathBuf;
use std::rc::Rc;

/// What the backend detects (its `SdkKind`, named as on the wire).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolchainKind {
    Python,
    Node,
    Php,
    PackageManager,
    Rust,
    Go,
    TypeScript,
    Jdk,
    Maven,
    Ruby,
}

impl ToolchainKind {
    /// The backend's name of the kind.
    pub fn wire(self) -> &'static str {
        match self {
            Self::Python => "python",
            Self::Node => "node",
            Self::Php => "php",
            Self::PackageManager => "packageManager",
            Self::Rust => "rust",
            Self::Go => "go",
            Self::TypeScript => "typeScript",
            Self::Jdk => "jdk",
            Self::Maven => "maven",
            Self::Ruby => "ruby",
        }
    }
}

/// One installation the backend found: the shape of its `Sdk` on the wire (the crate that defines
/// it is not a dependency of this one).
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Toolchain {
    /// The executable or directory the setting stores.
    pub path: String,
    /// Display name, e.g. `Python 3.12 (.venv)`.
    pub name: String,
    #[serde(default)]
    pub version: Option<String>,
    /// Where it was found: `venv`, `system`, `workspace`, `path`…
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub home: Option<String>,
    /// Rust standard library sources, Go default GOPATH.
    #[serde(default)]
    pub extra: Option<String>,
}

/// Where the detection of a kind stands.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Detection {
    /// Not asked for yet.
    #[default]
    Idle,
    Loading,
    Ready,
}

#[derive(Debug, Clone)]
struct CatalogEntry {
    kind: ToolchainKind,
    detection: Detection,
    found: Vec<Toolchain>,
}

/// The installations found so far, per kind.
#[derive(Debug, Clone, Default)]
pub struct ToolchainCatalog {
    entries: Vec<CatalogEntry>,
}

impl Global for ToolchainCatalog {}

impl ToolchainCatalog {
    fn entry(&self, kind: ToolchainKind) -> Option<&CatalogEntry> {
        self.entries.iter().find(|entry| entry.kind == kind)
    }

    fn entry_mut(&mut self, kind: ToolchainKind) -> &mut CatalogEntry {
        let at = match self.entries.iter().position(|entry| entry.kind == kind) {
            Some(at) => at,
            None => {
                let entry = CatalogEntry { kind, detection: Detection::Idle, found: Vec::new() };
                self.entries.push(entry);
                self.entries.len() - 1
            }
        };
        &mut self.entries[at]
    }

    pub fn detection(&self, kind: ToolchainKind) -> Detection {
        self.entry(kind).map_or(Detection::Idle, |entry| entry.detection)
    }

    pub fn found(&self, kind: ToolchainKind) -> &[Toolchain] {
        match self.entry(kind) {
            Some(entry) => &entry.found,
            None => &[],
        }
    }
}

/// The installations of `kind` found so far; none before the first detection.
pub fn found(kind: ToolchainKind, cx: &App) -> Vec<Toolchain> {
    cx.try_global::<ToolchainCatalog>().map(|catalog| catalog.found(kind).to_vec()).unwrap_or_default()
}

pub fn detection(kind: ToolchainKind, cx: &App) -> Detection {
    cx.try_global::<ToolchainCatalog>().map_or(Detection::Idle, |catalog| catalog.detection(kind))
}

/// A language the registry knows, with the names of its configured language servers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownLanguage {
    pub id: String,
    pub name: String,
    pub servers: Vec<String>,
}

/// A formatter a plugin contributes, with the languages it formats (`*`: any).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginFormatter {
    pub id: String,
    pub name: String,
    pub languages: Vec<String>,
}

/// What a language page asks the backend.
#[derive(Clone)]
pub enum LanguageAsk {
    /// Detect the installations of `kind`, unless known or under way; `again`: anew.
    Detect { kind: ToolchainKind, again: bool },
    /// The tool `name` found on the PATH.
    LocateTool { name: &'static str, then: Then<Option<String>> },
    /// The Xdebug version the PHP at `php` loads (`Some(None)`: none); `None` when the check failed.
    PhpXdebug { php: String, then: Then<Option<Option<String>>> },
    /// The interpreter the project uses when none is set: its virtualenv.
    PythonEffective { then: Then<Option<String>> },
    /// The languages of the registry, plugin languages included.
    Languages { then: Then<Vec<KnownLanguage>> },
    /// The formatters plugins contribute.
    PluginFormatters { then: Then<Vec<PluginFormatter>> },
    /// Whether the window's project is a workspace (its manifest is in the project folder).
    Workspace { then: Then<bool> },
}

/// What a control of a language page asks of the dialog's caller.
#[derive(Clone)]
pub enum LanguageEvent {
    /// What any settings control asks (OK, Cancel, the file chooser).
    Control(ControlEvent),
    /// A question for the backend.
    Ask(LanguageAsk),
}

/// Answers `ask` for the window `window_label`.
pub fn answer(ask: LanguageAsk, window_label: &str, cx: &mut App) {
    match ask {
        LanguageAsk::Detect { kind, again } => detect(kind, again, window_label, cx),
        LanguageAsk::LocateTool { name, then } => locate_tool(name, window_label, then, cx),
        LanguageAsk::PhpXdebug { php, then } => php_xdebug(php, window_label, then, cx),
        LanguageAsk::PythonEffective { then } => python_effective(window_label, then, cx),
        LanguageAsk::Languages { then } => {
            // The registry is read in memory; no file or program is touched.
            let languages = cx.try_global::<crate::backend::Backend>().map(|backend| {
                let all = backend.host.languages.all();
                let known = all.into_iter().map(|language| KnownLanguage {
                    servers: language.language_servers.iter().map(|server| server.name.clone()).collect(),
                    id: language.id.0,
                    name: language.name,
                });
                known.collect::<Vec<_>>()
            });
            then(languages.unwrap_or_default(), cx);
        }
        LanguageAsk::PluginFormatters { then } => {
            let reading = cx.background_executor().spawn(async {
                let choices = ide_host::ops::lsp::plugin_formatters();
                let formatters = choices.into_iter().map(|choice| PluginFormatter {
                    id: choice.id,
                    name: choice.name,
                    languages: choice.languages,
                });
                formatters.collect::<Vec<_>>()
            });
            cx.spawn(async move |cx| {
                let formatters = reading.await;
                cx.update(|cx| then(formatters, cx));
            })
            .detach();
        }
        LanguageAsk::Workspace { then } => {
            let backend = cx.try_global::<crate::backend::Backend>();
            let root = backend.and_then(|backend| backend.host.project(window_label).ok()).map(|p| p.root.clone());
            let reading = cx.background_executor().spawn(async move {
                root.is_some_and(|root| ide_workspace::workspace::Workspace::load(&root).ok().flatten().is_some())
            });
            cx.spawn(async move |cx| {
                let workspace = reading.await;
                cx.update(|cx| then(workspace, cx));
            })
            .detach();
        }
    }
}

fn detect(kind: ToolchainKind, again: bool, window_label: &str, cx: &mut App) {
    let now = detection(kind, cx);
    if now == Detection::Loading || (now == Detection::Ready && !again) {
        return;
    }
    // `update_global` tells the views observing the catalog; the forms that read it repaint with
    // the windows.
    cx.default_global::<ToolchainCatalog>();
    cx.update_global::<ToolchainCatalog, _>(|catalog, _| catalog.entry_mut(kind).detection = Detection::Loading);
    cx.refresh_windows();
    let finish = move |result: Result<Vec<Toolchain>, String>, cx: &mut App| {
        cx.update_global::<ToolchainCatalog, _>(|catalog, _| {
            let entry = catalog.entry_mut(kind);
            entry.detection = Detection::Ready;
            // A failed detection offers nothing found, as the combo shows only the other choices.
            match result {
                Ok(found) => entry.found = found,
                Err(error) => {
                    tracing::warn!("Settings: detecting the {} installations failed: {error}", kind.wire());
                    entry.found.clear();
                }
            }
        });
        cx.refresh_windows();
    };
    run_detection(kind, window_label, finish, cx);
}

/// The installations of `kind`, as the backend detects them for the window.
#[cfg(not(test))]
async fn detect_on(host: &ide_host::Host, window: &str, kind: ToolchainKind) -> Result<Vec<Toolchain>, String> {
    let kind = serde_json::from_value(Value::from(kind.wire())).map_err(|error| error.to_string())?;
    let found = ide_host::ops::sdk::sdk_detect(host, window, kind).await.map_err(|error| error.message)?;
    serde_json::to_value(found).and_then(serde_json::from_value).map_err(|error| error.to_string())
}

#[cfg(not(test))]
fn run_detection(
    kind: ToolchainKind,
    window_label: &str,
    finish: impl FnOnce(Result<Vec<Toolchain>, String>, &mut App) + 'static,
    cx: &mut App,
) {
    let Some(backend) = cx.try_global::<crate::backend::Backend>() else {
        finish(Err("The backend is not running".to_owned()), cx);
        return;
    };
    let label = window_label.to_owned();
    let work = backend.with_host(move |host| async move { detect_on(&host, &label, kind).await });
    cx.spawn(async move |cx| {
        let result = work.await.unwrap_or_else(|error| Err(error.to_string()));
        cx.update(|cx| finish(result, cx));
    })
    .detach();
}

#[cfg(not(test))]
fn locate_tool(name: &'static str, window_label: &str, then: Then<Option<String>>, cx: &mut App) {
    let Some(backend) = cx.try_global::<crate::backend::Backend>() else { return then(None, cx) };
    let label = window_label.to_owned();
    let work = backend.with_host(move |host| async move {
        let located = ide_host::ops::sdk::sdk_locate_tools(&host, &label, vec![name.to_owned()], None).await;
        located.ok().and_then(|located| located.tools.into_iter().next()).and_then(|tool| tool.path)
    });
    cx.spawn(async move |cx| {
        let path = work.await.ok().flatten().map(|path| path.to_string_lossy().into_owned());
        cx.update(|cx| then(path, cx));
    })
    .detach();
}

#[cfg(not(test))]
fn php_xdebug(php: String, window_label: &str, then: Then<Option<Option<String>>>, cx: &mut App) {
    let Some(backend) = cx.try_global::<crate::backend::Backend>() else { return then(None, cx) };
    let label = window_label.to_owned();
    let work = backend.with_host(move |host| async move {
        ide_host::ops::sdk::sdk_php_xdebug(&host, &label, PathBuf::from(php), None).await
    });
    cx.spawn(async move |cx| {
        let xdebug = match work.await {
            Ok(Ok(found)) => Some(found.xdebug),
            // The row stays hidden when the check fails, as a failed check shows nothing.
            Ok(Err(error)) => {
                tracing::warn!("Settings: checking Xdebug failed: {}", error.message);
                None
            }
            Err(error) => {
                tracing::warn!("Settings: checking Xdebug failed: {error}");
                None
            }
        };
        cx.update(|cx| then(xdebug, cx));
    })
    .detach();
}

#[cfg(not(test))]
fn python_effective(window_label: &str, then: Then<Option<String>>, cx: &mut App) {
    let Some(backend) = cx.try_global::<crate::backend::Backend>() else { return then(None, cx) };
    let label = window_label.to_owned();
    let work = backend.with_host(move |host| async move {
        ide_host::ops::sdk::sdk_python_effective(&host, &label, None)
            .ok()
            .flatten()
            .map(|path| path.to_string_lossy().into_owned())
    });
    cx.spawn(async move |cx| {
        let python = work.await.ok().flatten();
        cx.update(|cx| then(python, cx));
    })
    .detach();
}

/// What a test answers for the toolchains: the installations per kind, the tools on the PATH, the
/// Xdebug of a PHP and the project's virtualenv interpreter. Detections are counted.
#[cfg(test)]
#[derive(Clone, Default)]
pub struct TestToolchains {
    pub found: Vec<(ToolchainKind, Vec<Toolchain>)>,
    pub located: Vec<(&'static str, String)>,
    pub xdebug: Vec<(String, Option<String>)>,
    pub python: Option<String>,
    pub detections: Rc<std::cell::Cell<usize>>,
}

#[cfg(test)]
impl Global for TestToolchains {}

#[cfg(test)]
fn run_detection(
    kind: ToolchainKind,
    _: &str,
    finish: impl FnOnce(Result<Vec<Toolchain>, String>, &mut App) + 'static,
    cx: &mut App,
) {
    let found = cx.try_global::<TestToolchains>().map(|test| {
        test.detections.set(test.detections.get() + 1);
        test.found.iter().find(|(own, _)| *own == kind).map(|(_, found)| found.clone()).unwrap_or_default()
    });
    finish(Ok(found.unwrap_or_default()), cx);
}

#[cfg(test)]
fn locate_tool(name: &'static str, _: &str, then: Then<Option<String>>, cx: &mut App) {
    let path = cx
        .try_global::<TestToolchains>()
        .and_then(|test| test.located.iter().find(|(own, _)| *own == name).map(|(_, path)| path.clone()));
    then(path, cx);
}

#[cfg(test)]
fn php_xdebug(php: String, _: &str, then: Then<Option<Option<String>>>, cx: &mut App) {
    let xdebug = cx
        .try_global::<TestToolchains>()
        .and_then(|test| test.xdebug.iter().find(|(own, _)| *own == php).map(|(_, xdebug)| xdebug.clone()));
    then(xdebug, cx);
}

#[cfg(test)]
fn python_effective(_: &str, then: Then<Option<String>>, cx: &mut App) {
    let python = cx.try_global::<TestToolchains>().and_then(|test| test.python.clone());
    then(python, cx);
}

/// Enter is the dialog's OK and Escape its Cancel.
pub fn enter_escape<V: EventEmitter<LanguageEvent>>(keystroke: &Keystroke, cx: &mut Context<V>) -> bool {
    if keystroke.modifiers.modified() {
        return false;
    }
    match keystroke.key.as_str() {
        "enter" => cx.emit(LanguageEvent::Control(ControlEvent::Submit)),
        "escape" => cx.emit(LanguageEvent::Control(ControlEvent::Cancel)),
        _ => return false,
    }
    true
}

/// A Browse… request that stages the path chosen for `key`.
pub fn browse_into(binding: &Binding, key: &'static str, directories: bool) -> LanguageEvent {
    let binding = binding.clone();
    let then: Then<PathBuf> = Rc::new(move |path: PathBuf, cx: &mut App| {
        binding.stage(key, Value::String(path.to_string_lossy().into_owned()), cx);
    });
    LanguageEvent::Control(ControlEvent::Browse { directories, then })
}

// ---------------------------------------------------------------------------
// The SDK combo

/// What a combo offers besides the detected installations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SdkChoices {
    /// The empty choice of the spec, then the detected installations.
    Detected,
    /// Bundled and the project's `node_modules/typescript`, never the detected list itself.
    TypeScript,
    /// The project's virtualenv as the empty choice, named once the backend says which it is.
    Python,
}

/// An SDK combo of a page: the setting, the row label, what it detects, its empty choice
/// (label, detail), whether Browse… picks a directory, the help line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SdkSpec {
    pub key: &'static str,
    pub label: &'static str,
    pub kind: ToolchainKind,
    pub empty: Option<(&'static str, Option<&'static str>)>,
    pub directory: bool,
    pub hint: Option<&'static str>,
    pub choices: SdkChoices,
}

pub fn sdk(key: &'static str, label: &'static str, kind: ToolchainKind) -> SdkSpec {
    SdkSpec { key, label, kind, empty: None, directory: false, hint: None, choices: SdkChoices::Detected }
}

impl SdkSpec {
    pub fn empty(self, label: &'static str, detail: &'static str) -> Self {
        Self { empty: Some((label, Some(detail))), ..self }
    }
    pub fn directory(self) -> Self {
        Self { directory: true, ..self }
    }
    pub fn hint(self, hint: &'static str) -> Self {
        Self { hint: Some(hint), ..self }
    }
    pub fn choices(self, choices: SdkChoices) -> Self {
        Self { choices, ..self }
    }
}

/// One choice of a combo, its label and detail translated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdkChoice {
    pub value: String,
    pub label: String,
    pub detail: Option<String>,
}

fn choice(value: &str, label: String, detail: Option<String>) -> SdkChoice {
    SdkChoice { value: value.to_owned(), label, detail }
}

/// IDEA's SDK combo: the empty choice, the detected installations, the value set by hand, Detect
/// again and Browse….
pub struct SdkFieldView {
    binding: Binding,
    spec: SdkSpec,
    open: bool,
    /// The project's virtualenv for a Python combo: `None` until the backend answers.
    auto: Option<Option<String>>,
    asked: bool,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<LanguageEvent> for SdkFieldView {}

impl SdkFieldView {
    pub fn new(binding: Binding, spec: SdkSpec, cx: &mut Context<Self>) -> Self {
        let observe = cx.observe(&binding.model, |_: &mut Self, _, cx| cx.notify());
        let catalog = cx.observe_global::<ToolchainCatalog>(|_: &mut Self, cx| cx.notify());
        Self { binding, spec, open: false, auto: None, asked: false, _subscriptions: vec![observe, catalog] }
    }

    pub fn spec(&self) -> &SdkSpec {
        &self.spec
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// The project's virtualenv, as the backend tells it.
    pub fn set_auto(&mut self, auto: Option<String>, cx: &mut Context<Self>) {
        self.auto = Some(auto);
        cx.notify();
    }

    /// What the combo offers, in order.
    pub fn choices(&self, cx: &App) -> Vec<SdkChoice> {
        let value = self.binding.text(self.spec.key, cx);
        let mut out = Vec::new();
        match self.spec.choices {
            SdkChoices::TypeScript => {
                let workspace = found(ToolchainKind::TypeScript, cx).into_iter().find(|sdk| sdk.source == "workspace");
                let bundled = tx(cx, "the language server's own TypeScript");
                out.push(choice("bundled", tx(cx, "Bundled"), Some(bundled)));
                out.push(match workspace {
                    Some(workspace) => choice("workspace", workspace.name, Some(tildify(&workspace.path))),
                    None => choice(
                        "workspace",
                        "node_modules/typescript".to_owned(),
                        Some(tx(cx, "not installed in this project")),
                    ),
                });
                if !value.is_empty() && value != "bundled" && value != "workspace" {
                    out.push(choice(&value, tildify(&value), Some(tx(cx, "custom"))));
                }
                return out;
            }
            SdkChoices::Python => {
                let (label, detail) = match &self.auto {
                    Some(Some(auto)) => (tx(cx, "Project virtualenv (auto-detect)"), tildify(auto)),
                    _ => (tx(cx, "<No interpreter>"), tx(cx, "No virtualenv in the project")),
                };
                out.push(choice("", label, Some(detail)));
            }
            SdkChoices::Detected => {
                if let Some((label, detail)) = self.spec.empty {
                    out.push(choice("", tx(cx, label), detail.map(|detail| tx(cx, detail))));
                }
            }
        }
        for sdk in found(self.spec.kind, cx) {
            let detail = tildify(&sdk.path);
            out.push(choice(&sdk.path, sdk.name, Some(detail)));
        }
        if !value.is_empty() && !out.iter().any(|choice| choice.value == value) {
            out.push(choice(&value, tildify(&value), Some(tx(cx, "custom"))));
        }
        out
    }

    /// The label the combo shows: its value's choice, else what the empty box says.
    pub fn current(&self, cx: &App) -> String {
        let value = self.binding.text(self.spec.key, cx);
        let choices = self.choices(cx);
        match choices.into_iter().find(|choice| choice.value == value) {
            Some(choice) => choice.label,
            None if detection(self.spec.kind, cx) == Detection::Loading => tx(cx, "Detecting…"),
            None => tx(cx, "<No interpreter>"),
        }
    }

    /// The interpreter a Python combo stands for: the one set, else the project's virtualenv.
    pub fn python(&self, cx: &App) -> Option<String> {
        let value = self.binding.text(self.spec.key, cx);
        if !value.is_empty() {
            return Some(value);
        }
        self.auto.clone().flatten()
    }

    /// The control as text: `Label: current ▾` and its help line.
    pub fn lines(&self, cx: &App) -> Vec<String> {
        let mut out = vec![format!("{}: {} ▾", tx(cx, self.spec.label), self.current(cx))];
        if self.spec.choices == SdkChoices::Python
            && let Some(line) = self.python_line(cx)
        {
            out.push(format!("  {line}"));
        }
        out.extend(self.spec.hint.map(|hint| format!("    {}", tx(cx, hint))));
        out
    }

    /// `Python 3.12 (.venv) — ~/p/.venv/bin/python` under a Python combo.
    fn python_line(&self, cx: &App) -> Option<String> {
        let python = self.python(cx)?;
        let name = found(ToolchainKind::Python, cx).into_iter().find(|sdk| sdk.path == python).map(|sdk| sdk.name);
        Some(match name {
            Some(name) => format!("{name} — {}", tildify(&python)),
            None => tildify(&python),
        })
    }

    pub fn choose(&mut self, value: String, cx: &mut Context<Self>) {
        self.open = false;
        self.binding.stage(self.spec.key, Value::String(value), cx);
        cx.notify();
    }

    pub fn toggle_open(&mut self, cx: &mut Context<Self>) {
        self.open = !self.open;
        cx.notify();
    }

    fn detect_again(&mut self, cx: &mut Context<Self>) {
        cx.emit(LanguageEvent::Ask(LanguageAsk::Detect { kind: self.spec.kind, again: true }));
    }

    fn browse(&mut self, cx: &mut Context<Self>) {
        cx.emit(browse_into(&self.binding, self.spec.key, self.spec.directory));
    }
}

impl Render for SdkFieldView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The project's virtualenv is asked for the first time the combo is shown.
        if self.spec.choices == SdkChoices::Python && !self.asked {
            self.asked = true;
            let view = cx.weak_entity();
            let then: Then<Option<String>> = Rc::new(move |auto, cx: &mut App| {
                view.update(cx, |view, cx| view.set_auto(auto, cx)).ok();
            });
            cx.emit(LanguageEvent::Ask(LanguageAsk::PythonEffective { then }));
        }
        let key = self.spec.key;
        let query = self.binding.query(cx);
        let matched = self.binding.matches(self.spec.label, cx);
        let label = tx(cx, self.spec.label);
        let value = self.binding.text(key, cx);
        let choices = self.choices(cx);
        let current = self.current(cx);
        let boxed = select_box(cx, &current)
            .id(SharedString::from(format!("settings-sdk-{key}")))
            .debug_selector(move || format!("settings-sdk-{key}"))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.toggle_open(cx)));
        let list = if self.open {
            let mut rows: Vec<AnyElement> = Vec::new();
            for (index, offered) in choices.iter().enumerate() {
                let picked = offered.value.clone();
                let detail = offered.detail.as_ref().map(|detail| muted(cx, detail));
                let drawn =
                    drop_down_row(cx, format!("settings-sdk-{key}-{index}"), &offered.label, offered.value == value)
                        .children(detail)
                        .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.choose(picked.clone(), cx)));
                rows.push(drawn.into_any_element());
            }
            Some(drop_down(cx, rows))
        } else {
            None
        };
        let detect = match self.spec.choices {
            SdkChoices::TypeScript => None,
            SdkChoices::Detected | SdkChoices::Python => Some(if detection(self.spec.kind, cx) == Detection::Loading {
                muted(cx, &tx(cx, "Detecting…")).into_any_element()
            } else {
                let text = tx(cx, "Detect again");
                div()
                    .id(SharedString::from(format!("settings-sdk-{key}-detect")))
                    .debug_selector(move || format!("settings-sdk-{key}-detect"))
                    .child(button(cx, &text, false, true))
                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.detect_again(cx)))
                    .into_any_element()
            }),
        };
        let browse_label = tx(cx, "Browse…");
        let browse = div()
            .id(SharedString::from(format!("settings-browse-{key}")))
            .debug_selector(move || format!("settings-browse-{key}"))
            .child(button(cx, &browse_label, false, true))
            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.browse(cx)));
        let path = (self.spec.choices == SdkChoices::Python)
            .then(|| self.python_line(cx))
            .flatten()
            .map(|line| hint_line(cx, &line, 0.0).debug_selector(move || format!("settings-sdk-{key}-path")));
        let hint = self.spec.hint.map(|hint| hint_line(cx, &tx(cx, hint), 0.0));
        div()
            .flex()
            .flex_col()
            .child(
                row(cx, matched)
                    .child(row_label(cx, &label, &query))
                    .child(div().relative().child(boxed).children(list))
                    .children(detect)
                    .child(browse),
            )
            .children(path)
            .children(hint)
    }
}

// ---------------------------------------------------------------------------
// The path field

/// What an empty path field says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathPlaceholder {
    Fixed(&'static str),
    /// The default GOPATH of the detected Go, else `go env GOPATH`.
    GoPath,
    /// The standard library of the toolchain set, else that it is detected from the toolchain.
    RustStdlib,
}

/// A path field of a page: the setting, the row label, the empty text, whether Browse… picks a
/// directory, the help line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PathSpec {
    pub key: &'static str,
    pub label: &'static str,
    pub placeholder: PathPlaceholder,
    pub directory: bool,
    pub hint: Option<&'static str>,
}

/// A path typed (trimmed) or chosen with Browse….
pub struct PathFieldView {
    binding: Binding,
    spec: PathSpec,
    field: Entity<TextField>,
    claim: u64,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<LanguageEvent> for PathFieldView {}

impl PathFieldView {
    pub fn new(binding: Binding, spec: PathSpec, cx: &mut Context<Self>) -> Self {
        let text = binding.text(spec.key, cx);
        let placeholder = placeholder_of(spec.placeholder, &binding, cx);
        let field = new_field(&text, Some(placeholder), cx);
        let key = spec.key;
        let events = cx.subscribe(&field, move |view: &mut Self, field, event: &TextFieldEvent, cx| match event {
            TextFieldEvent::Changed => {
                let text = field.read(cx).text().trim().to_owned();
                view.binding.stage(key, Value::String(text), cx);
                cx.notify();
            }
            TextFieldEvent::Submitted => cx.emit(LanguageEvent::Control(ControlEvent::Submit)),
            TextFieldEvent::Cancelled => cx.emit(LanguageEvent::Control(ControlEvent::Cancel)),
        });
        let claim = claim_field_keys(&field, |_: &mut Self, keystroke: &Keystroke, cx| enter_escape(keystroke, cx), cx);
        let observe = cx.observe(&binding.model, |view: &mut Self, _, cx| {
            view.sync(cx);
            cx.notify();
        });
        let catalog = cx.observe_global::<ToolchainCatalog>(|view: &mut Self, cx| {
            view.sync(cx);
            cx.notify();
        });
        cx.on_release(|view: &mut Self, cx: &mut App| input::release_keys(view.claim, cx)).detach();
        Self { binding, spec, field, claim, _subscriptions: vec![events, observe, catalog] }
    }

    pub fn field(&self) -> &Entity<TextField> {
        &self.field
    }

    /// Shows the value the page shows now (Reset, a new scope, Browse…) and the empty text that
    /// follows the detected toolchains; a draft that only differs by spaces is kept.
    fn sync(&mut self, cx: &mut Context<Self>) {
        let wanted = self.binding.text(self.spec.key, cx);
        if self.field.read(cx).text().trim() != wanted {
            self.field.update(cx, |field, cx| field.set_text(&wanted, cx));
        }
        let placeholder = placeholder_of(self.spec.placeholder, &self.binding, cx);
        if self.field.read(cx).placeholder() != placeholder {
            self.field.update(cx, |field, cx| field.set_placeholder(placeholder, cx));
        }
    }

    /// The control as text: `Label: value`, or `Label: (empty text)`, and its help line.
    pub fn lines(&self, cx: &App) -> Vec<String> {
        let field = self.field.read(cx);
        let shown =
            if field.text().is_empty() { format!("({})", field.placeholder()) } else { field.text().to_owned() };
        let mut out = vec![format!("{}: {shown}", tx(cx, self.spec.label))];
        out.extend(self.spec.hint.map(|hint| format!("    {}", tx(cx, hint))));
        out
    }
}

/// The empty text of a path field now.
fn placeholder_of(placeholder: PathPlaceholder, binding: &Binding, cx: &App) -> String {
    match placeholder {
        PathPlaceholder::Fixed(text) => tx(cx, text),
        PathPlaceholder::GoPath => {
            let gopath = found(ToolchainKind::Go, cx).into_iter().find_map(|sdk| sdk.extra);
            gopath.map_or_else(|| tx(cx, "go env GOPATH"), |gopath| tildify(&gopath))
        }
        PathPlaceholder::RustStdlib => {
            let toolchain = binding.text("languages.rust.toolchain", cx);
            let selected = found(ToolchainKind::Rust, cx).into_iter().find(|sdk| sdk.path == toolchain);
            let stdlib = selected.and_then(|sdk| sdk.extra);
            stdlib.map_or_else(|| tx(cx, "Detected from the toolchain"), |stdlib| tildify(&stdlib))
        }
    }
}

impl Render for PathFieldView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let key = self.spec.key;
        let query = self.binding.query(cx);
        let matched = self.binding.matches(self.spec.label, cx);
        let label = tx(cx, self.spec.label);
        let browse_label = tx(cx, "Browse…");
        let directories = self.spec.directory;
        let browse = div()
            .id(SharedString::from(format!("settings-browse-{key}")))
            .debug_selector(move || format!("settings-browse-{key}"))
            .child(button(cx, &browse_label, false, true))
            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                cx.emit(browse_into(&view.binding, key, directories));
            }));
        let hint = self.spec.hint.map(|hint| hint_line(cx, &tx(cx, hint), 0.0));
        div()
            .flex()
            .flex_col()
            .child(
                row(cx, matched)
                    .child(row_label(cx, &label, &query))
                    .child(
                        frame(cx, WIDE_FIELD, false)
                            .id(SharedString::from(format!("settings-text-{key}")))
                            .debug_selector(move || format!("settings-text-{key}"))
                            .child(self.field.clone()),
                    )
                    .child(browse),
            )
            .children(hint)
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{click, dialog_model, init, staged, type_over};
    use super::*;
    use crate::settings_model::staging::Scope;
    use gpui_kit::TestAppContext;
    use std::cell::RefCell;

    const PAGE: &str = "languages.go";

    fn go(path: &str, gopath: &str) -> Toolchain {
        Toolchain {
            path: path.to_owned(),
            name: "go1.22.5".to_owned(),
            version: Some("1.22.5".to_owned()),
            source: "path".to_owned(),
            home: None,
            extra: Some(gopath.to_owned()),
        }
    }

    type Events = Rc<RefCell<Vec<LanguageEvent>>>;

    fn open_view<V, F>(cx: &mut TestAppContext, build: F) -> (Entity<V>, Events, &mut gpui_kit::VisualTestContext)
    where
        V: Render + EventEmitter<LanguageEvent>,
        F: FnOnce(&mut Window, &mut Context<V>) -> V + 'static,
    {
        init(cx);
        let (view, cx) = cx.add_window_view(build);
        cx.update(|window, _| window.activate_window());
        let events: Events = Rc::default();
        let log = events.clone();
        cx.update(|_, cx| {
            cx.subscribe(&view, move |_, event: &LanguageEvent, _| log.borrow_mut().push(event.clone())).detach();
        });
        cx.run_until_parked();
        (view, events, cx)
    }

    // Not from a spec: the combo offers the empty choice, the detected installations and the value
    // set by hand, and a choice is staged.
    #[gpui_kit::test]
    async fn the_combo_offers_the_detected_installations_and_stages_a_choice(cx: &mut TestAppContext) {
        let stored = [("languages.go.goroot", Value::from("/opt/custom-go"))];
        let model = cx.update(|cx| dialog_model(PAGE, &stored, cx));
        cx.update(|cx| {
            let mut catalog = ToolchainCatalog::default();
            let entry = catalog.entry_mut(ToolchainKind::Go);
            entry.detection = Detection::Ready;
            entry.found = vec![go("/usr/local/go", "/home/ann/go")];
            cx.set_global(catalog);
        });
        let binding = Binding::fixed(model.clone(), PAGE, Scope::Project);
        let spec = sdk("languages.go.goroot", "GOROOT", ToolchainKind::Go).empty("Default", "go on PATH").directory();
        let (view, _, cx) = open_view(cx, move |_, cx| SdkFieldView::new(binding, spec, cx));
        let choices = view.read_with(cx, |view, cx| view.choices(cx));
        let shown: Vec<(&str, &str, Option<&str>)> =
            choices.iter().map(|c| (c.value.as_str(), c.label.as_str(), c.detail.as_deref())).collect();
        assert_eq!(
            shown,
            [
                ("", "Default", Some("go on PATH")),
                ("/usr/local/go", "go1.22.5", Some("/usr/local/go")),
                ("/opt/custom-go", "/opt/custom-go", Some("custom")),
            ]
        );
        assert_eq!(view.read_with(cx, |view, cx| view.lines(cx)), ["GOROOT: /opt/custom-go ▾"]);
        click(cx, "settings-sdk-languages.go.goroot");
        click(cx, "settings-sdk-languages.go.goroot-1");
        assert_eq!(staged(&model, cx), [("project:languages.go.goroot".to_owned(), "\"/usr/local/go\"".to_owned())]);
        assert!(!view.read_with(cx, |view, _| view.is_open()), "a choice closes the list");
    }

    // Not from a spec: Detect again asks the backend anew for the kind; Browse… asks for a
    // directory and stages the one chosen.
    #[gpui_kit::test]
    async fn detect_again_asks_anew_and_browse_stages_the_directory_chosen(cx: &mut TestAppContext) {
        let model = cx.update(|cx| dialog_model(PAGE, &[], cx));
        let binding = Binding::fixed(model.clone(), PAGE, Scope::Global);
        let spec = sdk("languages.go.goroot", "GOROOT", ToolchainKind::Go).empty("Default", "go on PATH").directory();
        let (_, events, cx) = open_view(cx, move |_, cx| SdkFieldView::new(binding, spec, cx));
        click(cx, "settings-sdk-languages.go.goroot-detect");
        click(cx, "settings-browse-languages.go.goroot");
        let events = events.borrow().clone();
        assert!(matches!(
            events.first(),
            Some(LanguageEvent::Ask(LanguageAsk::Detect { kind: ToolchainKind::Go, again: true }))
        ));
        let Some(LanguageEvent::Control(ControlEvent::Browse { directories: true, then })) = events.get(1) else {
            panic!("a directory is asked for");
        };
        cx.update(|_, cx| then(PathBuf::from("/opt/go"), cx));
        cx.run_until_parked();
        assert_eq!(staged(&model, cx), [("global:languages.go.goroot".to_owned(), "\"/opt/go\"".to_owned())]);
    }

    // Not from a spec: a path is staged trimmed, and the empty field names the default GOPATH of
    // the detected Go once it is found.
    #[gpui_kit::test]
    async fn a_path_is_staged_trimmed_and_the_placeholder_follows_the_detected_go(cx: &mut TestAppContext) {
        let model = cx.update(|cx| dialog_model(PAGE, &[], cx));
        let binding = Binding::fixed(model.clone(), PAGE, Scope::Global);
        let spec = PathSpec {
            key: "languages.go.gopath",
            label: "Project GOPATH",
            placeholder: PathPlaceholder::GoPath,
            directory: true,
            hint: None,
        };
        let (view, _, cx) = open_view(cx, move |_, cx| PathFieldView::new(binding, spec, cx));
        assert_eq!(view.read_with(cx, |view, cx| view.lines(cx)), ["Project GOPATH: (go env GOPATH)"]);
        cx.update(|_, cx| {
            cx.default_global::<ToolchainCatalog>();
            cx.update_global::<ToolchainCatalog, _>(|catalog, _| {
                catalog.entry_mut(ToolchainKind::Go).found = vec![go("/g", "/Users/ann/go")];
            });
        });
        cx.run_until_parked();
        assert_eq!(view.read_with(cx, |view, cx| view.lines(cx)), ["Project GOPATH: (~/go)"]);
        let field = view.read_with(cx, |view, _| view.field().clone());
        type_over(&field, "  /work/go ", cx);
        assert_eq!(staged(&model, cx), [("global:languages.go.gopath".to_owned(), "\"/work/go\"".to_owned())]);
    }
}
