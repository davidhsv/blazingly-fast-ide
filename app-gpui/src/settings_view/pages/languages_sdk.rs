//! The toolchains the Settings pages offer (Languages & Frameworks, Build Tools) and the questions
//! those pages ask the backend about them.
//!
//! The installations of each kind are detected once a control that offers them is first shown, not
//! when Settings opens, and kept while that dialog is open ([`Toolchains`]): switching pages does
//! not look for them again, and every combo of a kind lists the same installations; Detect again
//! looks anew. The SDK combo ([`SdkFieldView`]) lists the empty choice, the installations found and
//! a value set by hand, with Detect again and Browse…; the path field ([`PathFieldView`]) trims
//! what is typed and offers Browse….
//!
//! The backend's detection reads the PATH, the system's and the version managers' folders and asks
//! the tools found for their versions, so a test answers in its place with an [`SdkDetection`] of
//! its own; a test build without one finds nothing, looks nothing up and runs nothing. The same
//! holds for the formatters plugins contribute, which the plugin host of the whole process lists.

use super::{
    Binding, ControlEvent, Then, WIDE_FIELD, claim_field_keys, drop_down, drop_down_row, frame, hint_line, icon_button,
    muted, new_field, row, row_label, select_box,
};
use crate::platform::input;
use crate::settings_model::language_pages::tildify;
use crate::settings_model::staging::SettingsDialog;
use crate::settings_view::controls::tx;
use crate::ui::text_field::{TextField, TextFieldEvent};
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Entity, EntityId, EventEmitter, Global, IntoElement, Keystroke, MouseButton,
    Render, SharedString, Subscription, Window, div, prelude::*,
};
use serde_json::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

/// What the backend detects (its `SdkKind`, named as on the wire).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

/// One installation the backend found, as the pages show it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Toolchain {
    /// The executable or directory the setting stores.
    pub path: String,
    /// Display name, e.g. `Python 3.12 (.venv)`.
    pub name: String,
    pub version: Option<String>,
    /// Where it was found: `venv`, `system`, `workspace`, `path`…
    pub source: String,
    pub home: Option<String>,
    /// Rust standard library sources, Go default GOPATH.
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

/// What a test answers in place of the backend for every toolchain question: the installations
/// per kind, the tools on the PATH, the Xdebug of a PHP, the project's virtualenv interpreter and
/// the formatters plugins contribute. The kinds asked for are logged, in order.
#[derive(Clone, Default)]
pub struct SdkDetection {
    pub found: Vec<(ToolchainKind, Vec<Toolchain>)>,
    pub located: Vec<(&'static str, String)>,
    pub xdebug: Vec<(String, Option<String>)>,
    pub python: Option<String>,
    pub formatters: Vec<PluginFormatter>,
    pub asked: Rc<RefCell<Vec<ToolchainKind>>>,
}

impl Global for SdkDetection {}

/// The installations found so far, per Settings dialog and kind; the views that show them observe
/// it.
#[derive(Default)]
pub struct ToolchainCatalog {
    dialogs: HashMap<EntityId, HashMap<ToolchainKind, (Detection, Vec<Toolchain>)>>,
}

impl Global for ToolchainCatalog {}

/// The toolchains of one Settings dialog, asked of the backend for its window.
#[derive(Clone)]
pub struct Toolchains {
    dialog: EntityId,
    window: Rc<str>,
}

impl Toolchains {
    /// The toolchains of the dialog `model` belongs to; what was found goes with the dialog.
    pub fn new(model: &Entity<SettingsDialog>, window: &str, cx: &App) -> Self {
        let dialog = model.entity_id();
        cx.observe_release(model, move |_, cx| {
            if cx.has_global::<ToolchainCatalog>() {
                cx.update_global::<ToolchainCatalog, _>(|catalog, _| catalog.dialogs.remove(&dialog));
            }
        })
        .detach();
        Self { dialog, window: Rc::from(window) }
    }

    pub fn window(&self) -> &str {
        &self.window
    }

    fn entry(&self, kind: ToolchainKind, cx: &App) -> Option<(Detection, Vec<Toolchain>)> {
        let catalog = cx.try_global::<ToolchainCatalog>()?;
        catalog.dialogs.get(&self.dialog)?.get(&kind).cloned()
    }

    /// The installations of `kind` found so far; none before the first detection.
    pub fn found(&self, kind: ToolchainKind, cx: &App) -> Vec<Toolchain> {
        self.entry(kind, cx).map(|(_, found)| found).unwrap_or_default()
    }

    pub fn detection(&self, kind: ToolchainKind, cx: &App) -> Detection {
        self.entry(kind, cx).map_or(Detection::Idle, |(detection, _)| detection)
    }

    /// Records where the detection of `kind` stands and what it found; the views observing the
    /// catalog repaint, and so do the forms that read it.
    fn set(&self, kind: ToolchainKind, detection: Detection, found: Option<Vec<Toolchain>>, cx: &mut App) {
        cx.default_global::<ToolchainCatalog>();
        cx.update_global::<ToolchainCatalog, _>(|catalog, _| {
            let entry = catalog.dialogs.entry(self.dialog).or_default().entry(kind).or_default();
            entry.0 = detection;
            if let Some(found) = found {
                entry.1 = found;
            }
        });
        cx.refresh_windows();
    }

    /// Detects the installations of `kind`, unless they are known or being looked for; `again`:
    /// anew.
    pub fn detect(&self, kind: ToolchainKind, again: bool, cx: &mut App) {
        let now = self.detection(kind, cx);
        if now == Detection::Loading || (now == Detection::Ready && !again) {
            return;
        }
        self.set(kind, Detection::Loading, None, cx);
        let toolchains = self.clone();
        let finish = move |found: Vec<Toolchain>, cx: &mut App| toolchains.set(kind, Detection::Ready, Some(found), cx);
        let given = cx.try_global::<SdkDetection>().map(|test| {
            test.asked.borrow_mut().push(kind);
            test.found.iter().find(|(own, _)| *own == kind).map(|(_, found)| found.clone()).unwrap_or_default()
        });
        match given {
            Some(found) => finish(found, cx),
            None if cfg!(test) => finish(Vec::new(), cx),
            None => backend_detection(kind, &self.window, finish, cx),
        }
    }
}

/// The backend's detection of `kind` for `window`, on the backend's runtime. A failure is logged
/// and lists nothing, as the combo then offers only its other choices.
fn backend_detection(
    kind: ToolchainKind,
    window: &str,
    finish: impl FnOnce(Vec<Toolchain>, &mut App) + 'static,
    cx: &mut App,
) {
    let Some(backend) = cx.try_global::<crate::backend::Backend>() else {
        finish(Vec::new(), cx);
        return;
    };
    let label = window.to_owned();
    let work = backend.with_host(move |host| async move {
        let wire = serde_json::from_value(Value::from(kind.wire())).map_err(|error| error.to_string())?;
        let found = ide_host::ops::sdk::sdk_detect(&host, &label, wire).await.map_err(|error| error.message)?;
        let text = |path: &std::path::Path| path.to_string_lossy().into_owned();
        let toolchains = found.into_iter().map(|sdk| Toolchain {
            path: text(&sdk.path),
            name: sdk.name,
            version: sdk.version,
            source: sdk.source,
            home: sdk.home.as_deref().map(text),
            extra: sdk.extra.as_deref().map(text),
        });
        Ok::<_, String>(toolchains.collect::<Vec<_>>())
    });
    cx.spawn(async move |cx| {
        let found = match work.await {
            Ok(Ok(found)) => found,
            Ok(Err(error)) => {
                tracing::warn!("Settings: detecting the {} installations failed: {error}", kind.wire());
                Vec::new()
            }
            Err(error) => {
                tracing::warn!("Settings: detecting the {} installations failed: {error}", kind.wire());
                Vec::new()
            }
        };
        cx.update(|cx| finish(found, cx));
    })
    .detach();
}

/// The tool `name` found on the PATH of `window`'s project.
pub fn locate_tool(name: &'static str, window: &str, then: Then<Option<String>>, cx: &mut App) {
    let given = cx
        .try_global::<SdkDetection>()
        .map(|test| test.located.iter().find(|(own, _)| *own == name).map(|(_, path)| path.clone()));
    match given {
        Some(path) => return then(path, cx),
        None if cfg!(test) => return then(None, cx),
        None => {}
    }
    let Some(backend) = cx.try_global::<crate::backend::Backend>() else { return then(None, cx) };
    let label = window.to_owned();
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

/// The Xdebug version the PHP at `php` loads (`Some(None)`: none); `None` when the check failed.
pub fn php_xdebug(php: String, window: &str, then: Then<Option<Option<String>>>, cx: &mut App) {
    let given = cx
        .try_global::<SdkDetection>()
        .map(|test| test.xdebug.iter().find(|(own, _)| *own == php).map(|(_, xdebug)| xdebug.clone()));
    match given {
        Some(xdebug) => return then(xdebug, cx),
        None if cfg!(test) => return then(None, cx),
        None => {}
    }
    let Some(backend) = cx.try_global::<crate::backend::Backend>() else { return then(None, cx) };
    let label = window.to_owned();
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

/// The interpreter `window`'s project uses when none is set: its virtualenv.
pub fn python_effective(window: &str, then: Then<Option<String>>, cx: &mut App) {
    let given = cx.try_global::<SdkDetection>().map(|test| test.python.clone());
    match given {
        Some(python) => return then(python, cx),
        None if cfg!(test) => return then(None, cx),
        None => {}
    }
    let Some(backend) = cx.try_global::<crate::backend::Backend>() else { return then(None, cx) };
    let label = window.to_owned();
    let work = backend.with_host(move |host| async move {
        let python = ide_host::ops::sdk::sdk_python_effective(&host, &label, None).ok().flatten();
        python.map(|path| path.to_string_lossy().into_owned())
    });
    cx.spawn(async move |cx| {
        let python = work.await.ok().flatten();
        cx.update(|cx| then(python, cx));
    })
    .detach();
}

/// The formatters the plugins loaded in this process contribute. The plugin host is the whole
/// process's, so a test build answers with its [`SdkDetection`]'s, or none.
pub fn plugin_formatters(then: Then<Vec<PluginFormatter>>, cx: &mut App) {
    let given = cx.try_global::<SdkDetection>().map(|test| test.formatters.clone());
    match given {
        Some(formatters) => return then(formatters, cx),
        None if cfg!(test) => return then(Vec::new(), cx),
        None => {}
    }
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
    /// The tool `name` found on the PATH.
    LocateTool { name: &'static str, then: Then<Option<String>> },
    /// The Xdebug version the PHP at `php` loads (`Some(None)`: none); `None` when the check failed.
    PhpXdebug { php: String, then: Then<Option<Option<String>>> },
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
        LanguageAsk::LocateTool { name, then } => locate_tool(name, window_label, then, cx),
        LanguageAsk::PhpXdebug { php, then } => php_xdebug(php, window_label, then, cx),
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
        LanguageAsk::PluginFormatters { then } => plugin_formatters(then, cx),
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
pub fn browse_into(binding: &Binding, key: &'static str, directories: bool) -> ControlEvent {
    let binding = binding.clone();
    let then: Then<PathBuf> = Rc::new(move |path: PathBuf, cx: &mut App| {
        binding.stage(key, Value::String(path.to_string_lossy().into_owned()), cx);
    });
    ControlEvent::Browse { directories, then }
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
    /// An empty choice without a detail.
    pub fn empty_label(self, label: &'static str) -> Self {
        Self { empty: Some((label, None)), ..self }
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
    toolchains: Toolchains,
    open: bool,
    /// The project's virtualenv for a Python combo: `None` until the backend answers.
    auto: Option<Option<String>>,
    asked: bool,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ControlEvent> for SdkFieldView {}

impl SdkFieldView {
    pub fn new(binding: Binding, spec: SdkSpec, toolchains: Toolchains, cx: &mut Context<Self>) -> Self {
        let observe = cx.observe(&binding.model, |_: &mut Self, _, cx| cx.notify());
        let catalog = cx.observe_global::<ToolchainCatalog>(|_: &mut Self, cx| cx.notify());
        Self {
            binding,
            spec,
            toolchains,
            open: false,
            auto: None,
            asked: false,
            _subscriptions: vec![observe, catalog],
        }
    }

    pub fn spec(&self) -> &SdkSpec {
        &self.spec
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn detection(&self, cx: &App) -> Detection {
        self.toolchains.detection(self.spec.kind, cx)
    }

    /// The project's virtualenv, as the backend tells it.
    pub fn set_auto(&mut self, auto: Option<String>, cx: &mut Context<Self>) {
        self.auto = Some(auto);
        cx.notify();
    }

    /// What the combo offers, in order.
    pub fn choices(&self, cx: &App) -> Vec<SdkChoice> {
        let value = self.binding.text(self.spec.key, cx);
        let found = self.toolchains.found(self.spec.kind, cx);
        let mut out = Vec::new();
        match self.spec.choices {
            SdkChoices::TypeScript => {
                let workspace = found.into_iter().find(|sdk| sdk.source == "workspace");
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
        for sdk in found {
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
            None if self.detection(cx) == Detection::Loading => tx(cx, "Detecting…"),
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

    /// The control as text: `Label: current ▾`, the choices while the list is open, a Python
    /// combo's interpreter, and the help line.
    pub fn lines(&self, cx: &App) -> Vec<String> {
        let mut out = vec![format!("{}: {} ▾", tx(cx, self.spec.label), self.current(cx))];
        if self.open {
            for offered in self.choices(cx) {
                out.push(match offered.detail {
                    Some(detail) => format!("  {} — {detail}", offered.label),
                    None => format!("  {}", offered.label),
                });
            }
        }
        if let Some(line) = self.python_line(cx) {
            out.push(format!("  {line}"));
        }
        out.extend(self.spec.hint.map(|hint| format!("    {}", tx(cx, hint))));
        out
    }

    /// `Python 3.12 (.venv) — ~/p/.venv/bin/python` under a Python combo.
    fn python_line(&self, cx: &App) -> Option<String> {
        if self.spec.choices != SdkChoices::Python {
            return None;
        }
        let python = self.python(cx)?;
        let found = self.toolchains.found(ToolchainKind::Python, cx);
        let name = found.into_iter().find(|sdk| sdk.path == python).map(|sdk| sdk.name);
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

    /// Looks for the installations again, after this turn: a detection that answers at once then
    /// finds the views free to update.
    fn detect_again(&mut self, cx: &mut Context<Self>) {
        let (toolchains, kind) = (self.toolchains.clone(), self.spec.kind);
        cx.defer(move |cx| toolchains.detect(kind, true, cx));
    }

    /// The installations, and a Python combo's virtualenv, are asked for once the combo is first
    /// drawn, after the frame.
    fn ask_once(&mut self, cx: &mut Context<Self>) {
        if self.asked {
            return;
        }
        self.asked = true;
        let (toolchains, kind) = (self.toolchains.clone(), self.spec.kind);
        let python = self.spec.choices == SdkChoices::Python;
        let view = cx.weak_entity();
        cx.defer(move |cx| {
            toolchains.detect(kind, false, cx);
            if python {
                let then: Then<Option<String>> = Rc::new(move |auto, cx: &mut App| {
                    view.update(cx, |view, cx| view.set_auto(auto, cx)).ok();
                });
                python_effective(toolchains.window(), then, cx);
            }
        });
    }
}

impl Render for SdkFieldView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.ask_once(cx);
        let key = self.spec.key;
        let query = self.binding.query(cx);
        let matched = self.binding.matches(self.spec.label, cx);
        let label = tx(cx, self.spec.label);
        let value = self.binding.text(key, cx);
        let current = self.current(cx);
        let boxed = select_box(cx, &current)
            .id(SharedString::from(format!("settings-sdk-{key}")))
            .debug_selector(move || format!("settings-sdk-{key}"))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.toggle_open(cx)));
        let list = self.open.then(|| {
            let rows: Vec<AnyElement> = self
                .choices(cx)
                .into_iter()
                .enumerate()
                .map(|(index, offered)| {
                    let chosen = offered.value == value;
                    let detail = offered.detail.as_ref().map(|detail| muted(cx, detail));
                    let picked = offered.value;
                    drop_down_row(cx, format!("settings-sdk-{key}-choice-{index}"), &offered.label, chosen)
                        .children(detail)
                        .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.choose(picked.clone(), cx)))
                        .into_any_element()
                })
                .collect();
            drop_down(cx, rows)
        });
        let detect = match self.spec.choices {
            SdkChoices::TypeScript => None,
            SdkChoices::Detected | SdkChoices::Python => Some(if self.detection(cx) == Detection::Loading {
                muted(cx, &tx(cx, "Detecting installations")).into_any_element()
            } else {
                icon_button(cx, format!("settings-sdk-{key}-refresh"), "refresh", "Detect again")
                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.detect_again(cx)))
                    .into_any_element()
            }),
        };
        let directories = self.spec.directory;
        let browse = icon_button(cx, format!("settings-sdk-{key}-browse"), "folder-open", "Browse…").on_click(
            cx.listener(move |view, _: &ClickEvent, _, cx| cx.emit(browse_into(&view.binding, key, directories))),
        );
        let path = self
            .python_line(cx)
            .map(|line| hint_line(cx, &line, 4.0).debug_selector(move || format!("settings-sdk-{key}-path")));
        let hint = self.spec.hint.map(|hint| hint_line(cx, &tx(cx, hint), 4.0));
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
    toolchains: Toolchains,
    field: Entity<TextField>,
    claim: u64,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<LanguageEvent> for PathFieldView {}

impl PathFieldView {
    pub fn new(binding: Binding, spec: PathSpec, toolchains: Toolchains, cx: &mut Context<Self>) -> Self {
        let text = binding.text(spec.key, cx);
        let placeholder = placeholder_of(spec.placeholder, &binding, &toolchains, cx);
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
        Self { binding, spec, toolchains, field, claim, _subscriptions: vec![events, observe, catalog] }
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
        let placeholder = placeholder_of(self.spec.placeholder, &self.binding, &self.toolchains, cx);
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
fn placeholder_of(placeholder: PathPlaceholder, binding: &Binding, toolchains: &Toolchains, cx: &App) -> String {
    match placeholder {
        PathPlaceholder::Fixed(text) => tx(cx, text),
        PathPlaceholder::GoPath => {
            let gopath = toolchains.found(ToolchainKind::Go, cx).into_iter().find_map(|sdk| sdk.extra);
            gopath.map_or_else(|| tx(cx, "go env GOPATH"), |gopath| tildify(&gopath))
        }
        PathPlaceholder::RustStdlib => {
            let toolchain = binding.text("languages.rust.toolchain", cx);
            let selected = toolchains.found(ToolchainKind::Rust, cx).into_iter().find(|sdk| sdk.path == toolchain);
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
        let directories = self.spec.directory;
        let browse = icon_button(cx, format!("settings-browse-{key}"), "folder-open", "Browse…").on_click(cx.listener(
            move |view, _: &ClickEvent, _, cx| {
                cx.emit(LanguageEvent::Control(browse_into(&view.binding, key, directories)));
            },
        ));
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
    use super::super::test_support::{click, dialog_model, init, open, staged, type_over};
    use super::*;
    use crate::settings_model::staging::Scope;
    use gpui_kit::TestAppContext;

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

    fn goroot() -> SdkSpec {
        sdk("languages.go.goroot", "GOROOT", ToolchainKind::Go).empty("Default", "go on PATH").directory()
    }

    // Not from a spec: the installations are looked for once the combo is drawn; it offers the
    // empty choice, those found and the value set by hand, and a choice is staged.
    #[gpui_kit::test]
    async fn the_combo_offers_the_detected_installations_and_stages_a_choice(cx: &mut TestAppContext) {
        let stored = [("languages.go.goroot", Value::from("/opt/custom-go"))];
        let model = cx.update(|cx| dialog_model(PAGE, &stored, cx));
        let test = SdkDetection {
            found: vec![(ToolchainKind::Go, vec![go("/usr/local/go", "/home/ann/go")])],
            ..SdkDetection::default()
        };
        let asked = test.asked.clone();
        cx.update(|cx| cx.set_global(test));
        let binding = Binding::fixed(model.clone(), PAGE, Scope::Project);
        let toolchains = cx.update(|cx| Toolchains::new(&model, "w1", cx));
        let (view, _, cx) = open(cx, move |_, cx| SdkFieldView::new(binding, goroot(), toolchains, cx));
        assert_eq!(asked.borrow().as_slice(), [ToolchainKind::Go], "drawing the combo looks once");
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
        let lines = view.read_with(cx, |view, cx| view.lines(cx));
        assert_eq!(lines[1..], ["  Default — go on PATH", "  go1.22.5 — /usr/local/go", "  /opt/custom-go — custom"]);
        click(cx, "settings-sdk-languages.go.goroot-choice-1");
        assert_eq!(staged(&model, cx), [("project:languages.go.goroot".to_owned(), "\"/usr/local/go\"".to_owned())]);
        assert!(!view.read_with(cx, |view, _| view.is_open()), "a choice closes the list");
        click(cx, "settings-sdk-languages.go.goroot-refresh");
        assert_eq!(asked.borrow().len(), 2, "Detect again looks again");
    }

    // Not from a spec: two combos of one kind in a dialog share what one detection found.
    #[gpui_kit::test]
    async fn the_combos_of_a_kind_share_one_detection(cx: &mut TestAppContext) {
        let model = cx.update(|cx| dialog_model(PAGE, &[], cx));
        let test = SdkDetection {
            found: vec![(ToolchainKind::Go, vec![go("/usr/local/go", "/g")])],
            ..SdkDetection::default()
        };
        let asked = test.asked.clone();
        cx.update(|cx| cx.set_global(test));
        let toolchains = cx.update(|cx| Toolchains::new(&model, "w1", cx));
        let (first_binding, second_binding) =
            (Binding::fixed(model.clone(), PAGE, Scope::Project), Binding::fixed(model.clone(), PAGE, Scope::Global));
        let other = toolchains.clone();
        let (_, _, cx) = open(cx, move |_, cx| SdkFieldView::new(first_binding, goroot(), toolchains, cx));
        // Not drawn: it only reads what the first one's detection found.
        let second = cx.update(|_, cx| cx.new(|cx| SdkFieldView::new(second_binding, goroot(), other, cx)));
        cx.run_until_parked();
        assert_eq!(second.read_with(cx, |view, cx| view.choices(cx)).len(), 2, "the Go found is offered");
        assert_eq!(asked.borrow().as_slice(), [ToolchainKind::Go]);
    }

    // Not from a spec: without a detection of its own a test build finds nothing and runs
    // nothing: no installation, no tool on the PATH, no Xdebug, no virtualenv, no plugin formatter.
    #[gpui_kit::test]
    async fn a_test_build_without_a_detection_of_its_own_finds_nothing(cx: &mut TestAppContext) {
        type Answers = Rc<RefCell<Vec<String>>>;
        let answers: Answers = Rc::default();
        let model = cx.update(|cx| dialog_model(PAGE, &[], cx));
        cx.update(|cx| {
            assert!(!cx.has_global::<SdkDetection>());
            let toolchains = Toolchains::new(&model, "w1", cx);
            toolchains.detect(ToolchainKind::Jdk, false, cx);
            assert_eq!(toolchains.detection(ToolchainKind::Jdk, cx), Detection::Ready, "answered at once");
            assert_eq!(toolchains.found(ToolchainKind::Jdk, cx), []);
            let log = answers.clone();
            locate_tool("php", "w1", Rc::new(move |path, _| log.borrow_mut().push(format!("php {path:?}"))), cx);
            let log = answers.clone();
            let xdebug: Then<Option<Option<String>>> =
                Rc::new(move |xdebug, _| log.borrow_mut().push(format!("xdebug {xdebug:?}")));
            php_xdebug("/usr/bin/php".to_owned(), "w1", xdebug, cx);
            let log = answers.clone();
            python_effective("w1", Rc::new(move |python, _| log.borrow_mut().push(format!("python {python:?}"))), cx);
            let log = answers.clone();
            plugin_formatters(Rc::new(move |found, _| log.borrow_mut().push(format!("formatters {found:?}"))), cx);
        });
        assert_eq!(
            answers.borrow().as_slice(),
            ["php None", "xdebug None", "python None", "formatters []"],
            "answered at once"
        );
    }

    // Not from a spec: Browse… asks for a directory and the one chosen is staged and shown.
    #[gpui_kit::test]
    async fn browse_asks_for_a_folder_and_stages_it(cx: &mut TestAppContext) {
        let model = cx.update(|cx| dialog_model(PAGE, &[], cx));
        let binding = Binding::fixed(model.clone(), PAGE, Scope::Global);
        let toolchains = cx.update(|cx| Toolchains::new(&model, "w1", cx));
        let (view, seen, cx) = open(cx, move |_, cx| SdkFieldView::new(binding, goroot(), toolchains, cx));
        let asked: Rc<RefCell<Option<Then<PathBuf>>>> = Rc::default();
        let keep = asked.clone();
        cx.update(|_, cx| {
            cx.subscribe(&view, move |_, event: &ControlEvent, _| {
                if let ControlEvent::Browse { directories: true, then } = event {
                    *keep.borrow_mut() = Some(then.clone());
                }
            })
            .detach();
        });
        click(cx, "settings-sdk-languages.go.goroot-browse");
        assert_eq!(seen.borrow().as_slice(), ["browse"]);
        let then = asked.borrow_mut().take().expect("a folder is asked for");
        cx.update(|_, cx| then(PathBuf::from("/opt/go"), cx));
        cx.run_until_parked();
        assert_eq!(staged(&model, cx), [("global:languages.go.goroot".to_owned(), "\"/opt/go\"".to_owned())]);
        assert_eq!(view.read_with(cx, |view, cx| view.current(cx)), "/opt/go");
    }

    // Not from a spec: a path is staged trimmed, and the empty field names the default GOPATH of
    // the detected Go once it is found.
    #[gpui_kit::test]
    async fn a_path_is_staged_trimmed_and_the_placeholder_follows_the_detected_go(cx: &mut TestAppContext) {
        let model = cx.update(|cx| dialog_model(PAGE, &[], cx));
        let binding = Binding::fixed(model.clone(), PAGE, Scope::Global);
        let toolchains = cx.update(|cx| Toolchains::new(&model, "w1", cx));
        let spec = PathSpec {
            key: "languages.go.gopath",
            label: "Project GOPATH",
            placeholder: PathPlaceholder::GoPath,
            directory: true,
            hint: None,
        };
        init(cx);
        let detecting = toolchains.clone();
        let (view, cx) = cx.add_window_view(move |_, cx| PathFieldView::new(binding, spec, toolchains, cx));
        cx.run_until_parked();
        assert_eq!(view.read_with(cx, |view, cx| view.lines(cx)), ["Project GOPATH: (go env GOPATH)"]);
        let test = SdkDetection {
            found: vec![(ToolchainKind::Go, vec![go("/g", "/Users/ann/go")])],
            ..SdkDetection::default()
        };
        cx.update(|_, cx| {
            cx.set_global(test);
            detecting.detect(ToolchainKind::Go, false, cx);
        });
        cx.run_until_parked();
        assert_eq!(view.read_with(cx, |view, cx| view.lines(cx)), ["Project GOPATH: (~/go)"]);
        let field = view.read_with(cx, |view, _| view.field().clone());
        type_over(&field, "  /work/go ", cx);
        assert_eq!(staged(&model, cx), [("global:languages.go.gopath".to_owned(), "\"/work/go\"".to_owned())]);
    }

    // Not from a spec: what a dialog found goes with it; another dialog looks again.
    #[gpui_kit::test]
    async fn a_dialog_closed_takes_what_it_found_with_it(cx: &mut TestAppContext) {
        let test = SdkDetection { found: vec![(ToolchainKind::Go, vec![go("/g", "/p")])], ..SdkDetection::default() };
        cx.update(|cx| cx.set_global(test));
        let first = cx.update(|cx| dialog_model(PAGE, &[], cx));
        let second = cx.update(|cx| dialog_model(PAGE, &[], cx));
        let (one, two) = cx.update(|cx| (Toolchains::new(&first, "w1", cx), Toolchains::new(&second, "w2", cx)));
        cx.update(|cx| one.detect(ToolchainKind::Go, false, cx));
        cx.read(|cx| {
            assert_eq!(one.found(ToolchainKind::Go, cx).len(), 1);
            assert_eq!(two.detection(ToolchainKind::Go, cx), Detection::Idle, "the other dialog has not looked");
        });
        drop(first);
        // Entities dropped are released when the app next flushes its effects.
        cx.update(|_| {});
        cx.read(|cx| assert_eq!(one.detection(ToolchainKind::Go, cx), Detection::Idle, "released with its dialog"));
    }
}
