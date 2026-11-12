//! The action system ("everything is an action"), modelled on IDEA's ActionManager.
//!
//! Menus, keybindings, toolbar buttons, context menus and the Actions tab of Search Everywhere
//! all resolve to the same registry, so a feature registers its behaviour once. Default
//! keybindings live in keymaps, not on the action.
//!
//! An action's static description (title, category, `when` clause, icon…) comes from
//! `assets/gen/actions.json`, extracted from the webview UI's sources: a feature registers the
//! handler for an id and the registry attaches the description. Titles are written in English;
//! `action.<id>`, `action.<id>.description` and `action.category.<category>` catalog keys, else
//! the source-text catalog, translate them. The English title stays searchable.

use super::context::{ContextKeys, compile_when};
use super::i18n::I18n;
use gpui_kit::{App, Global, Window};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::OnceLock;

/// The static description of an action.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionMeta {
    /// Dotted, stable id, e.g. `navigate.searchEverywhere`.
    pub id: String,
    #[serde(default)]
    pub title: String,
    /// Catalog key of the title, for the few actions that are not written in English at the source.
    #[serde(default)]
    pub title_key: Option<String>,
    /// Group shown in Search Everywhere and the keymap editor, e.g. `Navigate`.
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub category_key: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// Icon name from the icon table.
    #[serde(default)]
    pub icon: Option<String>,
    /// Enablement; evaluated against context keys.
    #[serde(default)]
    pub when: Option<String>,
    /// Extra condition for keyboard shortcuts only (menus and Find Action ignore it), e.g. to
    /// yield a shared shortcut to an editor action while typing.
    #[serde(default)]
    pub key_when: Option<String>,
    /// Extra search terms for the Actions tab.
    #[serde(default)]
    pub keywords: Vec<String>,
    /// The title comes from outside the IDE (a plugin) and is shown as written.
    #[serde(default)]
    pub verbatim: bool,
    /// Hidden from Search Everywhere (internal / parameterised actions).
    #[serde(default)]
    pub hidden: bool,
    /// A toggle action (IDEA's ToggleAction): Search Everywhere shows an ON/OFF switch.
    #[serde(default)]
    pub toggle: bool,
}

impl ActionMeta {
    pub fn new(id: &str, title: &str) -> Self {
        Self { id: id.to_owned(), title: title.to_owned(), ..Default::default() }
    }

    pub fn category(mut self, category: &str) -> Self {
        self.category = Some(category.to_owned());
        self
    }

    pub fn when(mut self, when: &str) -> Self {
        self.when = Some(when.to_owned());
        self
    }

    pub fn key_when(mut self, key_when: &str) -> Self {
        self.key_when = Some(key_when.to_owned());
        self
    }

    /// The title, description and category in the active locale.
    pub fn localized(&self, i18n: &I18n) -> LocalizedAction {
        let text = |key: String, source: Option<&str>| -> Option<String> {
            let source = source.filter(|s| !s.is_empty())?;
            let translated = match i18n.lookup(&key) {
                Some(message) => super::i18n::format_message(message, &[], i18n.locale()),
                None => i18n.tx(source),
            };
            (translated != source).then_some(translated)
        };
        let english_title = match &self.title_key {
            Some(key) => i18n.t(key, &[]),
            None => self.title.clone(),
        };
        let english_category = match &self.category_key {
            Some(key) => Some(i18n.t(key, &[])),
            None => self.category.clone(),
        };
        let title = if self.verbatim { None } else { text(format!("action.{}", self.id), Some(&english_title)) };
        let description = text(format!("action.{}.description", self.id), self.description.as_deref());
        let category = english_category.as_deref().and_then(|c| text(format!("action.category.{c}"), Some(c)));
        let mut keywords = self.keywords.clone();
        if title.is_some() {
            keywords.push(english_title.clone());
        }
        LocalizedAction {
            title: title.unwrap_or(english_title),
            description: description.or_else(|| self.description.clone()),
            category: category.or(english_category),
            keywords,
        }
    }
}

/// What the UI shows for an action in the active locale.
#[derive(Debug, Clone, PartialEq)]
pub struct LocalizedAction {
    pub title: String,
    pub description: Option<String>,
    pub category: Option<String>,
    /// The action's keywords, plus the English title when the title is translated.
    pub keywords: Vec<String>,
}

#[derive(Deserialize)]
struct Manifest {
    actions: Vec<ActionMeta>,
}

/// Every action the webview UI registers statically, by id.
pub fn manifest() -> &'static BTreeMap<String, ActionMeta> {
    static MANIFEST: OnceLock<BTreeMap<String, ActionMeta>> = OnceLock::new();
    MANIFEST.get_or_init(|| {
        let manifest: Manifest = serde_json::from_str(include_str!("../../assets/gen/actions.json"))
            .expect("assets/gen/actions.json is generated and valid");
        manifest.actions.into_iter().map(|a| (a.id.clone(), a)).collect()
    })
}

/// How the user started an action (for listeners such as plugins' `action-performed`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ActionSource {
    Keyboard,
    Menu,
    Toolbar,
    ContextMenu,
    Search,
    #[default]
    Other,
}

impl ActionSource {
    pub fn name(self) -> &'static str {
        match self {
            Self::Keyboard => "keyboard",
            Self::Menu => "menu",
            Self::Toolbar => "toolbar",
            Self::ContextMenu => "contextMenu",
            Self::Search => "search",
            Self::Other => "other",
        }
    }
}

/// What a handler is given: the context keys it was enabled against and the invoker's argument.
pub struct ActionCall<'a> {
    pub ctx: &'a ContextKeys,
    /// Optional argument from the invoker (e.g. a menu passing a file path).
    pub arg: Option<&'a serde_json::Value>,
}

type RunFn = dyn Fn(&ActionCall, &mut Window, &mut App) -> anyhow::Result<()>;
type ToggledFn = dyn Fn(&Window, &App) -> bool;
type Listener = dyn Fn(&str, Option<&serde_json::Value>, ActionSource, &mut App);

pub struct Action {
    pub meta: ActionMeta,
    run: Rc<RunFn>,
    toggled: Option<Rc<ToggledFn>>,
}

impl Action {
    pub fn new(
        meta: ActionMeta,
        run: impl Fn(&ActionCall, &mut Window, &mut App) -> anyhow::Result<()> + 'static,
    ) -> Self {
        Self { meta, run: Rc::new(run), toggled: None }
    }

    /// Makes this a toggle action whose current state is `toggled`.
    pub fn toggled(mut self, toggled: impl Fn(&Window, &App) -> bool + 'static) -> Self {
        self.meta.toggle = true;
        self.toggled = Some(Rc::new(toggled));
        self
    }

    /// Whether a toggle action is on in `window`; none for an action that is not a toggle.
    pub fn is_toggled(&self, window: &Window, cx: &App) -> Option<bool> {
        self.toggled.as_ref().map(|toggled| toggled(window, cx))
    }
}

/// Why an action could not be run.
#[derive(Debug)]
pub enum ActionError {
    /// No action with this id is registered, or its `when` clause does not hold.
    Unavailable(String),
    /// The handler failed.
    Failed(anyhow::Error),
}

impl std::fmt::Display for ActionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(id) => write!(f, "Action unavailable: {id}"),
            Self::Failed(error) => write!(f, "{error}"),
        }
    }
}
impl std::error::Error for ActionError {}

/// Actions that stay available while a project is opening.
const WHILE_OPENING: [&str; 4] = ["app.quit", "window.next", "window.previous", "window.toggleFullScreen"];

/// Whether `meta` is enabled in `ctx`. A malformed `when` clause disables the action.
pub fn is_action_enabled(meta: &ActionMeta, ctx: &ContextKeys) -> bool {
    if ctx.is_truthy("projectOpening") && !WHILE_OPENING.contains(&meta.id.as_str()) {
        return false;
    }
    when_holds(meta, meta.when.as_deref(), ctx)
}

/// Whether a keyboard shortcut may run the action (`when` and `keyWhen`).
pub fn is_action_key_enabled(meta: &ActionMeta, ctx: &ContextKeys) -> bool {
    is_action_enabled(meta, ctx) && when_holds(meta, meta.key_when.as_deref(), ctx)
}

fn when_holds(meta: &ActionMeta, clause: Option<&str>, ctx: &ContextKeys) -> bool {
    let Some(clause) = clause else { return true };
    match compile_when(clause) {
        Ok(when) => when.eval(ctx),
        Err(error) => {
            tracing::error!(action = %meta.id, %error, "malformed when clause");
            false
        }
    }
}

/// The registered actions. One per process: an action's handler is told which window it runs in.
#[derive(Default)]
pub struct ActionRegistry {
    actions: BTreeMap<String, Rc<Action>>,
    listeners: Vec<(u64, Rc<Listener>)>,
    next_listener: u64,
    /// Set for the duration of the synchronous dispatch.
    source: ActionSource,
    /// Actions running synchronously inside another action's handler.
    nesting: u32,
}

impl Global for ActionRegistry {}

impl ActionRegistry {
    /// Registers an action; re-registering an id replaces it.
    pub fn register(&mut self, action: Action) {
        self.actions.insert(action.meta.id.clone(), Rc::new(action));
    }

    /// Registers the handler of an action the manifest describes.
    ///
    /// Panics when the manifest has no such id: every statically known action is described
    /// there, so an unknown id is a typo.
    pub fn handle(
        &mut self,
        id: &str,
        run: impl Fn(&ActionCall, &mut Window, &mut App) -> anyhow::Result<()> + 'static,
    ) {
        let meta =
            manifest().get(id).unwrap_or_else(|| panic!("action {id} is not in assets/gen/actions.json")).clone();
        self.register(Action::new(meta, run));
    }

    /// Like [`ActionRegistry::handle`], for a toggle action.
    pub fn handle_toggle(
        &mut self,
        id: &str,
        toggled: impl Fn(&Window, &App) -> bool + 'static,
        run: impl Fn(&ActionCall, &mut Window, &mut App) -> anyhow::Result<()> + 'static,
    ) {
        let meta =
            manifest().get(id).unwrap_or_else(|| panic!("action {id} is not in assets/gen/actions.json")).clone();
        self.register(Action::new(meta, run).toggled(toggled));
    }

    pub fn unregister(&mut self, id: &str) {
        self.actions.remove(id);
    }

    pub fn get(&self, id: &str) -> Option<Rc<Action>> {
        self.actions.get(id).cloned()
    }

    pub fn contains(&self, id: &str) -> bool {
        self.actions.contains_key(id)
    }

    pub fn all(&self) -> impl Iterator<Item = &Rc<Action>> {
        self.actions.values()
    }

    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.actions.keys().map(String::as_str)
    }

    /// Called before each action the user runs (keyboard, menus, Find Action), not for the
    /// actions an action runs itself. Macro recording listens here. Returns the listener's id.
    pub fn on_action_executed(
        &mut self,
        listener: impl Fn(&str, Option<&serde_json::Value>, ActionSource, &mut App) + 'static,
    ) -> u64 {
        self.next_listener += 1;
        self.listeners.push((self.next_listener, Rc::new(listener)));
        self.next_listener
    }

    pub fn remove_listener(&mut self, id: u64) {
        self.listeners.retain(|(listener, _)| *listener != id);
    }
}

/// Current state of a toggle action, or `None` for plain actions.
pub fn action_toggled(action: &Action, window: &Window, cx: &App) -> Option<bool> {
    action.toggled.as_ref().map(|toggled| toggled(window, cx))
}

fn run(id: &str, arg: Option<&serde_json::Value>, window: &mut Window, cx: &mut App) -> Result<(), ActionError> {
    let Some(action) = cx.try_global::<ActionRegistry>().and_then(|r| r.get(id)) else {
        tracing::warn!("unknown action: {id}");
        return Err(ActionError::Unavailable(id.to_owned()));
    };
    let ctx = super::context::window_context(window, cx);
    if !is_action_enabled(&action.meta, &ctx) {
        return Err(ActionError::Unavailable(id.to_owned()));
    }
    let (top_level, source, listeners) = {
        let registry = cx.global::<ActionRegistry>();
        (registry.nesting == 0, registry.source, registry.listeners.iter().map(|(_, l)| l.clone()).collect::<Vec<_>>())
    };
    if top_level {
        for listener in listeners {
            listener(id, arg, source, cx);
        }
    }
    cx.global_mut::<ActionRegistry>().nesting += 1;
    let result = (action.run)(&ActionCall { ctx: &ctx, arg }, window, cx);
    cx.global_mut::<ActionRegistry>().nesting -= 1;
    result.map_err(ActionError::Failed)
}

/// Runs an action if registered and enabled. Returns whether it ran; a failing handler is
/// logged, as for any action the user starts.
pub fn execute_action(id: &str, arg: Option<&serde_json::Value>, window: &mut Window, cx: &mut App) -> bool {
    match run(id, arg, window, cx) {
        Ok(()) => true,
        Err(ActionError::Unavailable(_)) => false,
        Err(ActionError::Failed(error)) => {
            tracing::error!("action {id} failed: {error:#}");
            true
        }
    }
}

/// Like [`execute_action`], for callers that need the handler's error (host requests
/// acknowledge an operation only once it has started).
pub fn execute_action_or_throw(
    id: &str,
    arg: Option<&serde_json::Value>,
    window: &mut Window,
    cx: &mut App,
) -> Result<(), ActionError> {
    run(id, arg, window, cx)
}

/// [`execute_action`], telling listeners how the user started it.
pub fn execute_action_from(
    source: ActionSource,
    id: &str,
    arg: Option<&serde_json::Value>,
    window: &mut Window,
    cx: &mut App,
) -> bool {
    let previous = std::mem::replace(&mut cx.default_global::<ActionRegistry>().source, source);
    let ran = execute_action(id, arg, window, cx);
    cx.global_mut::<ActionRegistry>().source = previous;
    ran
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_describes_the_webview_actions() {
        let manifest = manifest();
        assert!(manifest.len() > 700, "{} actions", manifest.len());
        let search = &manifest["navigate.searchEverywhere"];
        assert_eq!(search.title, "Search Everywhere");
        assert_eq!(search.category.as_deref(), Some("Navigate"));
        assert_eq!(search.icon.as_deref(), Some("search"));
        assert_eq!(search.keywords, ["find", "goto", "everything"]);
        // Every `when` / `keyWhen` clause in the manifest compiles.
        for action in manifest.values() {
            for clause in [&action.when, &action.key_when].into_iter().flatten() {
                assert!(compile_when(clause).is_ok(), "{}: {clause}", action.id);
            }
        }
    }

    #[test]
    fn enablement_follows_when_and_key_when() {
        let meta = ActionMeta::new("t.gated", "Gated").when("editorFocus").key_when("!typing");
        let mut ctx = ContextKeys::new();
        assert!(!is_action_enabled(&meta, &ctx));
        ctx.set("editorFocus", true);
        assert!(is_action_enabled(&meta, &ctx) && is_action_key_enabled(&meta, &ctx));
        ctx.set("typing", true);
        assert!(is_action_enabled(&meta, &ctx) && !is_action_key_enabled(&meta, &ctx));
        // A malformed clause disables the action instead of enabling it.
        assert!(!is_action_enabled(&ActionMeta::new("t.bad", "Bad").when("a &&"), &ctx));
    }

    #[test]
    fn only_window_and_quit_actions_run_while_a_project_opens() {
        let mut ctx = ContextKeys::new();
        ctx.set("projectOpening", true);
        assert!(!is_action_enabled(&ActionMeta::new("file.save", "Save"), &ctx));
        for id in WHILE_OPENING {
            assert!(is_action_enabled(&ActionMeta::new(id, id), &ctx), "{id}");
        }
    }

    #[test]
    fn localizes_titles_through_catalog_keys_then_source_text() {
        let mut i18n = I18n::default();
        let meta =
            ActionMeta { keywords: vec!["find".into()], ..ActionMeta::new("t.sync", "Sync with OS").category("View") };
        let english = meta.localized(&i18n);
        assert_eq!(
            (english.title.as_str(), english.category.as_deref(), english.keywords.len()),
            ("Sync with OS", Some("View"), 1)
        );

        i18n.set_locale(Some("zh"), "en");
        let chinese = meta.localized(&i18n);
        assert_eq!(chinese.title, "与操作系统同步");
        // The English title stays searchable.
        assert_eq!(chinese.keywords, ["find", "Sync with OS"]);
        // A catalog key wins over the source-text catalog.
        i18n.register_messages("zh", &[("action.t.sync", "同步"), ("action.category.View", "视图菜单")]);
        let keyed = meta.localized(&i18n);
        assert_eq!((keyed.title.as_str(), keyed.category.as_deref()), ("同步", Some("视图菜单")));
        // A plugin's title is shown as written.
        let verbatim = ActionMeta { verbatim: true, ..meta.clone() }.localized(&i18n);
        assert_eq!(verbatim.title, "Sync with OS");
        // Titles written as catalog keys resolve through the catalog.
        let quit = manifest()["app.about"].localized(&I18n::default());
        assert_eq!(quit.title, I18n::default().t("app.action.about", &[]));
    }
}
