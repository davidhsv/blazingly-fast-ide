//! What running plugin code shows in a window: status bar items (a window's own item above the
//! global one), background tasks, context keys for `when` clauses, notifications with buttons that
//! run the plugin's commands, and the window's answers to the host's command requests.
//!
//! [`RuntimeUi`] calls nothing: [`RuntimeUi::handle`] turns a [`PluginEvent`] into
//! [`RuntimeEffect`]s, and fetches come back through their `*_fetched` methods.

use super::contribution::Contribution;
use crate::workbench::notifications::{NotificationAction, NotificationLevel, NotificationRequest};
use ide_host::plugins::{PluginContextKey, PluginEvent, StatusItem};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};

/// The prefix of a notification button's command: the JSON of a [`PluginCommandCall`] follows.
pub const NOTIFICATION_COMMAND_PREFIX: &str = "plugin-command:";

/// A plugin command to run, with its argument.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginCommandCall {
    pub plugin_id: String,
    pub command: String,
    pub argument: Option<String>,
}

impl PluginCommandCall {
    /// The call a notification button's command carries.
    pub fn parse(command: &str) -> Option<Self> {
        serde_json::from_str(command.strip_prefix(NOTIFICATION_COMMAND_PREFIX)?).ok()
    }

    fn encode(&self) -> String {
        format!("{NOTIFICATION_COMMAND_PREFIX}{}", serde_json::to_string(self).unwrap_or_default())
    }
}

/// A plugin's status bar item as the window shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShownStatusItem {
    pub plugin_id: String,
    pub item: StatusItem,
}

/// A plugin's background task.
#[derive(Clone, Debug, PartialEq)]
pub struct PluginTask {
    /// `plugin:<task id>`.
    pub id: String,
    pub plugin_id: String,
    pub title: String,
    /// `done / total` once the total is known.
    pub fraction: Option<f64>,
    pub message: Option<String>,
    pub cancellable: bool,
    pub done: bool,
}

/// What the window does for a plugin event.
#[derive(Clone, Debug, PartialEq)]
pub enum RuntimeEffect {
    /// Status items changed: read [`RuntimeUi::status_items`].
    StatusItemsChanged,
    /// Tasks changed: read [`RuntimeUi::tasks`].
    TasksChanged,
    /// `plugin_context_keys`; answer with [`RuntimeUi::context_keys_fetched`].
    FetchContextKeys,
    /// Set (`Some`) or clear (`None`) a context key.
    SetContext {
        name: String,
        value: Option<Value>,
    },
    Notify(NotificationRequest),
    CopyToClipboard(String),
    /// Open in the system browser.
    OpenUrl(String),
    /// Run an action of the window with `arg`, then answer `request_id` (`plugin_answer_command`).
    ExecuteAction {
        request_id: String,
        id: String,
        arg: Option<String>,
    },
    /// Run a plugin's command for the host's request, then answer `request_id` with its result.
    RunPluginCommand {
        request_id: String,
        plugin_id: String,
        command: String,
        arg: Option<String>,
        chain: Vec<String>,
    },
    /// Answer `request_id` at once with [`CommandAnswer::NotFound`].
    NotFound {
        request_id: String,
    },
}

fn level(level: ide_host::plugins::NotificationLevel) -> NotificationLevel {
    match level {
        ide_host::plugins::NotificationLevel::Info => NotificationLevel::Info,
        ide_host::plugins::NotificationLevel::Warning => NotificationLevel::Warning,
        ide_host::plugins::NotificationLevel::Error => NotificationLevel::Error,
    }
}

/// The plugin runtime's state in one window.
#[derive(Debug, Default)]
pub struct RuntimeUi {
    global: BTreeMap<(String, String), StatusItem>,
    window: BTreeMap<(String, String), StatusItem>,
    tasks: Vec<PluginTask>,
    applied: HashMap<String, HashSet<String>>,
    fetching: bool,
    dirty: bool,
}

impl RuntimeUi {
    /// The items the status bar shows, by `<plugin>/<id>`: a window's own item over the global one.
    pub fn status_items(&self) -> BTreeMap<String, ShownStatusItem> {
        let mut items = BTreeMap::new();
        for ((plugin_id, id), item) in self.global.iter().chain(&self.window) {
            let shown = ShownStatusItem { plugin_id: plugin_id.clone(), item: item.clone() };
            items.insert(format!("{plugin_id}/{id}"), shown);
        }
        items
    }

    pub fn tasks(&self) -> &[PluginTask] {
        &self.tasks
    }

    /// Asks for the context keys again: one fetch at a time, and changes meanwhile cost one more.
    pub fn refresh_context_keys(&mut self) -> Vec<RuntimeEffect> {
        if self.fetching {
            self.dirty = true;
            return Vec::new();
        }
        self.fetching = true;
        self.dirty = false;
        vec![RuntimeEffect::FetchContextKeys]
    }

    /// The window's project changed (any new project, even with the same root: another backend may
    /// serve it): status items are listed again and the keys fetched again.
    pub fn project_changed(&mut self) -> Vec<RuntimeEffect> {
        self.global.clear();
        self.window.clear();
        let mut effects = vec![RuntimeEffect::StatusItemsChanged];
        effects.extend(self.refresh_context_keys());
        effects
    }

    /// The effective keys of every plugin (`Err`: the fetch failed): each is set, keys no longer
    /// reported are cleared. A value that is not JSON clears its key.
    pub fn context_keys_fetched(&mut self, result: Result<Vec<PluginContextKey>, String>) -> Vec<RuntimeEffect> {
        self.fetching = false;
        let mut effects = Vec::new();
        if let Ok(keys) = result {
            let mut fresh: HashMap<String, HashSet<String>> = HashMap::new();
            for key in &keys {
                let value = serde_json::from_str::<Value>(&key.value).ok();
                let name = format!("plugin.{}.{}", key.plugin_id, key.key);
                effects.push(RuntimeEffect::SetContext { name, value });
                fresh.entry(key.plugin_id.clone()).or_default().insert(key.key.clone());
            }
            for (plugin_id, applied) in &self.applied {
                for key in applied {
                    if !fresh.get(plugin_id).is_some_and(|keys| keys.contains(key)) {
                        let name = format!("plugin.{plugin_id}.{key}");
                        effects.push(RuntimeEffect::SetContext { name, value: None });
                    }
                }
            }
            self.applied = fresh;
        }
        if self.dirty {
            effects.extend(self.refresh_context_keys());
        }
        effects
    }

    /// A plugin event for this window. `contributions` are the live ones (plugin commands), and
    /// `has_action` tells whether the window has an action. Returns `None` for events not handled here.
    pub fn handle(
        &mut self,
        event: &PluginEvent,
        contributions: &[Contribution],
        has_action: &dyn Fn(&str) -> bool,
    ) -> Option<Vec<RuntimeEffect>> {
        let effects = match event {
            PluginEvent::StatusItemChanged { plugin_id, id, item } => {
                set_item(&mut self.global, plugin_id, id, item.clone());
                vec![RuntimeEffect::StatusItemsChanged]
            }
            PluginEvent::WindowStatusItemChanged { plugin_id, id, item } => {
                set_item(&mut self.window, plugin_id, id, item.clone());
                vec![RuntimeEffect::StatusItemsChanged]
            }
            PluginEvent::TaskStarted { plugin_id, task_id, title, cancellable } => {
                let id = format!("plugin:{task_id}");
                self.tasks.retain(|t| t.id != id);
                self.tasks.push(PluginTask {
                    id,
                    plugin_id: plugin_id.clone(),
                    title: title.clone(),
                    fraction: None,
                    message: None,
                    cancellable: *cancellable,
                    done: false,
                });
                vec![RuntimeEffect::TasksChanged]
            }
            PluginEvent::TaskProgress { task_id, done, total, message, .. } => {
                let id = format!("plugin:{task_id}");
                let Some(task) = self.tasks.iter_mut().find(|t| t.id == id) else { return Some(Vec::new()) };
                task.fraction = total.filter(|total| *total > 0).map(|total| f64::from(*done) / f64::from(total));
                task.message.clone_from(message);
                vec![RuntimeEffect::TasksChanged]
            }
            PluginEvent::TaskEnded { task_id, .. } => {
                let id = format!("plugin:{task_id}");
                let Some(task) = self.tasks.iter_mut().find(|t| t.id == id) else { return Some(Vec::new()) };
                task.done = true;
                vec![RuntimeEffect::TasksChanged]
            }
            PluginEvent::ContextChanged { .. } => self.refresh_context_keys(),
            PluginEvent::Notification { plugin_id, level: lvl, title, body, actions } => {
                let actions = actions
                    .iter()
                    .map(|action| NotificationAction {
                        label: action.label.clone(),
                        command: PluginCommandCall {
                            plugin_id: plugin_id.clone(),
                            command: action.command.clone(),
                            argument: action.argument.clone(),
                        }
                        .encode(),
                    })
                    .collect();
                vec![RuntimeEffect::Notify(NotificationRequest {
                    body: Some(body.clone()),
                    actions,
                    verbatim: true,
                    group: Some("plugins".to_owned()),
                    ..NotificationRequest::new(level(*lvl), title.clone())
                })]
            }
            PluginEvent::CopyToClipboard { text, .. } => vec![RuntimeEffect::CopyToClipboard(text.clone())],
            PluginEvent::OpenUrl { url, .. } => {
                let lower = url.to_ascii_lowercase();
                if lower.starts_with("https://") || lower.starts_with("http://") {
                    vec![RuntimeEffect::OpenUrl(url.clone())]
                } else {
                    Vec::new()
                }
            }
            PluginEvent::ExecuteCommand { request_id, id, arg, chain } => {
                vec![command_request(request_id, id, arg.clone(), chain, contributions, has_action)]
            }
            _ => return None,
        };
        Some(effects)
    }
}

fn set_item(items: &mut BTreeMap<(String, String), StatusItem>, plugin_id: &str, id: &str, item: Option<StatusItem>) {
    let key = (plugin_id.to_owned(), id.to_owned());
    match item {
        Some(item) => {
            items.insert(key, item);
        }
        None => {
            items.remove(&key);
        }
    }
}

/// How the window answers the host's request to run `id`: a live plugin's command, an action of
/// the window, or nothing found.
fn command_request(
    request_id: &str,
    id: &str,
    arg: Option<String>,
    chain: &[String],
    contributions: &[Contribution],
    has_action: &dyn Fn(&str) -> bool,
) -> RuntimeEffect {
    let command = contributions.iter().filter(|c| c.point == "commands").find_map(|c| {
        let command = c.text("id")?;
        let matches = crate::plugins::plugin_action_id(&c.plugin_id, command) == id;
        matches.then(|| (c.plugin_id.clone(), command.to_owned()))
    });
    if let Some((plugin_id, command)) = command {
        return RuntimeEffect::RunPluginCommand {
            request_id: request_id.to_owned(),
            plugin_id,
            command,
            arg,
            chain: chain.to_vec(),
        };
    }
    if has_action(id) {
        return RuntimeEffect::ExecuteAction { request_id: request_id.to_owned(), id: id.to_owned(), arg };
    }
    RuntimeEffect::NotFound { request_id: request_id.to_owned() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn item(text: &str) -> StatusItem {
        StatusItem { text: text.to_owned(), tooltip: None, command: None }
    }

    fn handle(ui: &mut RuntimeUi, event: PluginEvent) -> Vec<RuntimeEffect> {
        ui.handle(&event, &[], &|_| false).expect("handled")
    }

    fn context(effects: &[RuntimeEffect]) -> Vec<(String, Option<Value>)> {
        effects
            .iter()
            .filter_map(|e| match e {
                RuntimeEffect::SetContext { name, value } => Some((name.clone(), value.clone())),
                _ => None,
            })
            .collect()
    }

    fn key(plugin: &str, key: &str, value: &str) -> PluginContextKey {
        PluginContextKey { plugin_id: plugin.to_owned(), key: key.to_owned(), value: value.to_owned() }
    }

    fn changed() -> PluginEvent {
        PluginEvent::ContextChanged { plugin_id: "p".into(), key: "a".into(), value: Some("true".into()) }
    }

    #[test]
    fn shows_status_items_and_tasks_of_running_plugin_code() {
        let mut ui = RuntimeUi::default();
        let branch =
            StatusItem { text: "main ↑1".into(), tooltip: Some("Branch".into()), command: Some("refresh".into()) };
        let set = |item| PluginEvent::StatusItemChanged { plugin_id: "git".into(), id: "branch".into(), item };
        handle(&mut ui, set(Some(branch.clone())));
        let shown = ShownStatusItem { plugin_id: "git".into(), item: branch };
        assert_eq!(ui.status_items().get("git/branch"), Some(&shown));
        handle(&mut ui, set(None));
        assert!(ui.status_items().is_empty());
        let task = |id: &str| PluginEvent::TaskEnded { plugin_id: "git".into(), task_id: id.into() };
        let started = PluginEvent::TaskStarted {
            plugin_id: "git".into(),
            task_id: "7".into(),
            title: "Fetching".into(),
            cancellable: true,
        };
        handle(&mut ui, started);
        handle(
            &mut ui,
            PluginEvent::TaskProgress {
                plugin_id: "git".into(),
                task_id: "7".into(),
                done: 1,
                total: Some(4),
                message: Some("origin".into()),
            },
        );
        let shown = &ui.tasks()[0];
        assert_eq!(
            (shown.id.as_str(), shown.title.as_str(), shown.fraction, shown.message.as_deref(), shown.cancellable),
            ("plugin:7", "Fetching", Some(0.25), Some("origin"), true)
        );
        handle(&mut ui, task("7"));
        assert!(ui.tasks()[0].done);
    }

    #[test]
    fn keeps_a_window_status_override_above_global_updates_and_falls_back_on_removal() {
        let mut ui = RuntimeUi::default();
        let global = |item| PluginEvent::StatusItemChanged { plugin_id: "scope".into(), id: "state".into(), item };
        let window =
            |item| PluginEvent::WindowStatusItemChanged { plugin_id: "scope".into(), id: "state".into(), item };
        handle(&mut ui, global(Some(item("global"))));
        handle(&mut ui, window(Some(item("window"))));
        handle(&mut ui, global(Some(item("updated"))));
        assert_eq!(ui.status_items()["scope/state"].item.text, "window");
        handle(&mut ui, window(None));
        assert_eq!(ui.status_items()["scope/state"].item.text, "updated");
    }

    #[test]
    fn executes_host_command_requests_through_actions_and_returns_plugin_command_results() {
        let mut ui = RuntimeUi::default();
        let has = |id: &str| id == "test.host-command";
        let request = |request: &str, id: &str, arg: Option<&str>| PluginEvent::ExecuteCommand {
            request_id: request.into(),
            id: id.into(),
            arg: arg.map(str::to_owned),
            chain: vec!["caller".into()],
        };
        let contributions = [Contribution::new("other", "commands", "test", json!({ "id": "test", "title": "Test" }))];
        let action = ui.handle(&request("ide", "test.host-command", Some("argument")), &contributions, &has);
        assert_eq!(
            action,
            Some(vec![RuntimeEffect::ExecuteAction {
                request_id: "ide".into(),
                id: "test.host-command".into(),
                arg: Some("argument".into())
            }])
        );
        let plugin = ui.handle(&request("plugin", "plugin.other.test", Some("argument")), &contributions, &has);
        assert_eq!(
            plugin,
            Some(vec![RuntimeEffect::RunPluginCommand {
                request_id: "plugin".into(),
                plugin_id: "other".into(),
                command: "test".into(),
                arg: Some("argument".into()),
                chain: vec!["caller".into()],
            }])
        );
        let missing = ui.handle(&request("missing", "unknown", None), &contributions, &has);
        assert_eq!(missing, Some(vec![RuntimeEffect::NotFound { request_id: "missing".into() }]));
    }

    #[test]
    fn runs_notification_buttons_as_commands_of_the_publishing_plugin_with_their_arguments() {
        let mut ui = RuntimeUi::default();
        let button = |label: &str, command: &str, argument: Option<&str>| ide_plugin::NotificationAction {
            label: label.into(),
            command: command.into(),
            argument: argument.map(str::to_owned),
        };
        let effects = handle(
            &mut ui,
            PluginEvent::Notification {
                plugin_id: "hello".into(),
                level: ide_host::plugins::NotificationLevel::Info,
                title: "Done".into(),
                body: "Pick an action".into(),
                actions: vec![
                    button("Open", "insert-date", Some("first\nsecond")),
                    button("Refresh", "word-count", None),
                    button("Undo", "insert-date", Some("undo")),
                ],
            },
        );
        let [RuntimeEffect::Notify(note)] = effects.as_slice() else { panic!("a notification") };
        let labels: Vec<&str> = note.actions.iter().map(|a| a.label.as_str()).collect();
        assert_eq!(labels, ["Open", "Refresh", "Undo"]);
        let call = PluginCommandCall::parse(&note.actions[0].command).expect("a plugin command");
        let expected = PluginCommandCall {
            plugin_id: "hello".into(),
            command: "insert-date".into(),
            argument: Some("first\nsecond".into()),
        };
        assert_eq!(call, expected);
    }

    #[test]
    fn shows_plugin_notifications_copies_to_clipboard_opens_urls_and_tracks_theme_changes() {
        let mut ui = RuntimeUi::default();
        let effects = handle(
            &mut ui,
            PluginEvent::Notification {
                plugin_id: "p".into(),
                level: ide_host::plugins::NotificationLevel::Info,
                title: "Done".into(),
                body: "3 words".into(),
                actions: Vec::new(),
            },
        );
        let expected = NotificationRequest {
            body: Some("3 words".into()),
            verbatim: true,
            group: Some("plugins".into()),
            ..NotificationRequest::new(NotificationLevel::Info, "Done")
        };
        assert_eq!(effects, vec![RuntimeEffect::Notify(expected)]);
        let copy = PluginEvent::CopyToClipboard { plugin_id: "p".into(), text: "https://github.com/o/r".into() };
        assert_eq!(handle(&mut ui, copy), vec![RuntimeEffect::CopyToClipboard("https://github.com/o/r".into())]);
        let url = "https://github.com/o/r/blob/main/a.rs";
        let open = PluginEvent::OpenUrl { plugin_id: "p".into(), url: url.into() };
        assert_eq!(handle(&mut ui, open), vec![RuntimeEffect::OpenUrl(url.into())]);
        let file = PluginEvent::OpenUrl { plugin_id: "p".into(), url: "file:///etc/passwd".into() };
        assert_eq!(handle(&mut ui, file), Vec::new());
    }

    #[test]
    fn applies_plugin_context_keys_from_the_backend() {
        let mut ui = RuntimeUi::default();
        assert_eq!(handle(&mut ui, changed()), vec![RuntimeEffect::FetchContextKeys]);
        let set = ui.context_keys_fetched(Ok(vec![key("p", "isEntity", "true"), key("p", "kind", "\"pod\"")]));
        assert_eq!(
            context(&set),
            vec![("plugin.p.isEntity".into(), Some(json!(true))), ("plugin.p.kind".into(), Some(json!("pod")))]
        );
        handle(&mut ui, changed());
        let dropped = ui.context_keys_fetched(Ok(vec![key("p", "kind", "\"pod\"")]));
        assert!(context(&dropped).contains(&("plugin.p.isEntity".into(), None)));
        handle(&mut ui, changed());
        assert_eq!(context(&ui.context_keys_fetched(Ok(Vec::new()))), vec![("plugin.p.kind".into(), None)]);
    }

    #[test]
    fn refreshes_the_keys_when_another_backend_publishes_a_project_with_the_same_root() {
        let mut ui = RuntimeUi::default();
        assert!(ui.project_changed().contains(&RuntimeEffect::FetchContextKeys));
        let fetched = ui.context_keys_fetched(Ok(vec![key("p", "backend", "\"b\"")]));
        assert_eq!(context(&fetched), vec![("plugin.p.backend".into(), Some(json!("b")))]);
    }

    #[test]
    fn coalesces_a_burst_of_changes_into_one_follow_up_fetch() {
        let mut ui = RuntimeUi::default();
        let fetches: usize = (0..5).map(|_| handle(&mut ui, changed()).len()).sum();
        assert_eq!(fetches, 1);
        let first = ui.context_keys_fetched(Ok(vec![key("q", "b", "true")]));
        assert!(first.contains(&RuntimeEffect::FetchContextKeys), "one follow-up for the burst");
        let second = ui.context_keys_fetched(Ok(vec![key("p", "a", "true"), key("q", "b", "true")]));
        assert!(!second.contains(&RuntimeEffect::FetchContextKeys));
        assert!(context(&second).contains(&("plugin.p.a".into(), Some(json!(true)))));
        assert_eq!(handle(&mut ui, changed()), vec![RuntimeEffect::FetchContextKeys]);
        let third = ui.context_keys_fetched(Ok(vec![key("q", "b", "true")]));
        assert!(context(&third).contains(&("plugin.p.a".into(), None)));
    }

    #[test]
    fn fetches_again_after_a_failed_fetch_when_changes_arrived_meanwhile() {
        let mut ui = RuntimeUi::default();
        handle(&mut ui, changed());
        handle(&mut ui, changed());
        assert_eq!(ui.context_keys_fetched(Err("x".into())), vec![RuntimeEffect::FetchContextKeys]);
        let fetched = ui.context_keys_fetched(Ok(vec![key("p", "f", "true")]));
        assert_eq!(context(&fetched), vec![("plugin.p.f".into(), Some(json!(true)))]);
    }
}
