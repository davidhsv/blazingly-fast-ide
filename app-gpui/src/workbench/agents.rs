//! The Agents feature in a project window: the store of the project's agent tasks, its subscription to
//! the backend's agent events, the Agents tool window with the conversation (or the New Task page)
//! docked in it, and the effects the feature's models return carried out against `ide_host::ops::agent`,
//! the dialogs, the timers, the UI storage and the notifications.
//!
//! The editor area holds file tabs only, so a conversation always shows docked in the tool window: an
//! "open in the editor" ([`AgentEffect::OpenSession`], [`AgentEffect::OpenHome`]) docks it as well, and
//! closing an editor tab of the Agents feature ([`AgentEffect::CloseTab`]) only takes that placement away.
//!
//! Answers of backend calls go to every model holding tickets (the store, the task batches, the docked
//! conversation and its composer); tickets are unique, so each drops the ones it does not know.

use super::notifications::{NotificationLevel, NotificationRequest};
use super::services_window::on_workbench;
use super::{Panel, Workbench};
use crate::agents::composer::{LineRange, can_send, mention_of};
use crate::agents::composer_view::{ComposerEvent, ComposerView};
use crate::agents::markdown::LinkAction;
use crate::agents::model::{HOME_KEY, REGISTRY_PAGE, TOOL_WINDOW};
use crate::agents::reply_actions::{ReplyActions, ReplyTarget};
use crate::agents::session_view::{MessageAction, SessionEvent, SessionView};
use crate::agents::store::{
    AgentCall, AgentEffect, AgentTab, AgentsStore, CallAnswer, ConfirmRequest, DRAFT_KEY, Editors, HISTORY_KEY, HOME,
    OPEN_TASK_COMMAND, Outcome, Reference, SESSION_LOCATION_KEY, Saved, SessionLocation, Ticket,
};
use crate::agents::task_actions::{BatchEffect, ConfirmChecked, TaskBatches, TaskCommand, can_fork};
use crate::agents::task_list_view::{TaskListEvent, TaskListView};
use crate::backend::Backend;
use crate::platform::actions::{ActionRegistry, ActionSource, execute_action_from};
use crate::platform::context::set_context;
use crate::platform::i18n::{I18n, Localization};
use crate::platform::local_time::now_ms;
use crate::platform::storage::UiStorage;
use crate::theme;
use crate::ui::text_field::{TextField, TextFieldEvent};
use crate::vcs::prompt_dialogs::{CANCEL, DialogAnswer, FieldRule, OK, PromptDialog};
use crate::vcs_ops::dialogs::{AskButton, AskRequest, PromptRequest};
use crate::vcs_ops::menu::MenuItem;
use futures::StreamExt as _;
use futures::channel::mpsc::unbounded;
use gpui_kit::{
    AnyView, AnyWindowHandle, App, BorrowAppContext, ClipboardItem, Context, Entity, EntityId, EventEmitter,
    FocusHandle, Focusable, IntoElement, MouseButton, MouseDownEvent, Pixels, Point, Render, SharedString,
    Subscription, Task, Window, div, prelude::*, px,
};
use ide_agent::{AgentEvent, AgentPrompt, AgentTask};
use serde_json::Value;
use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// The context key that says the docked conversation has the keyboard.
const SESSION_FOCUS: &str = "agentSessionFocus";

/// What one of the window's own tickets stands for once its request ends ([`AgentEffect::Finished`]).
enum Purpose {
    /// An edited message sent again from the conversation of this task: the editor learns how it went.
    EditSend(String),
    /// A failure is reported under this title.
    Failure(&'static str),
    /// The New Task page's Start: a failure shows on the page.
    Create,
}

/// The models of the feature that run their own requests beside the store.
#[derive(Default)]
struct AgentModels {
    replies: ReplyActions,
    batches: TaskBatches,
}

/// The conversation docked in the tool window, with its composer.
struct DockedSession {
    task: String,
    session: Entity<SessionView>,
    composer: Entity<ComposerView>,
    _events: [Subscription; 2],
}

/// The Agents feature of the window's project.
pub(super) struct AgentsSlot {
    store: Entity<AgentsStore>,
    models: AgentModels,
    /// The window the workbench is shown in: dialogs, menus and the layout are reached through it from
    /// answers that arrive later.
    window: AnyWindowHandle,
    /// Whether the project was trusted when the subscription was last made.
    trusted: bool,
    /// The tool window's view, once it was shown.
    list: Option<Entity<TaskListView>>,
    docked: Option<DockedSession>,
    home: Option<(Entity<NewTaskPage>, Subscription)>,
    /// The view last handed to the tool window's docked slot.
    shown_view: Option<EntityId>,
    /// The last composer focus request of the store that was carried out.
    focus_served: u64,
    purposes: HashMap<Ticket, Purpose>,
    /// The subscription to the project's agent events; dropping it ends the stream's reader.
    connection: Option<Task<()>>,
    _observer: Subscription,
}

fn turn_number(turn: usize) -> u32 {
    u32::try_from(turn).unwrap_or(u32::MAX)
}

/// UI text in the active language.
fn tx(cx: &App, text: &str) -> String {
    cx.try_global::<Localization>().map_or_else(|| text.to_owned(), |localization| localization.0.tx(text))
}

/// The string argument of an action.
fn text_arg(arg: Option<&Value>) -> Option<String> {
    arg.and_then(Value::as_str).map(str::to_owned)
}

/// The task ids an action is given, as a list of strings.
fn ids_arg(arg: Option<&Value>) -> Vec<String> {
    arg.and_then(Value::as_array)
        .map(|ids| ids.iter().filter_map(Value::as_str).map(str::to_owned).collect())
        .unwrap_or_default()
}

/// The reply an action is for: `{ task, turn }`.
fn reply_target(arg: Option<&Value>) -> Option<ReplyTarget> {
    let arg = arg?;
    let task = arg.get("task")?.as_str()?.to_owned();
    let turn = usize::try_from(arg.get("turn")?.as_u64()?).ok()?;
    Some(ReplyTarget { task, turn })
}

/// `path` relative to the project folder `root` (both IPC paths), when it is inside it.
fn relative_to(root: &str, path: &str) -> Option<String> {
    path.strip_prefix(root)?.strip_prefix('/').filter(|rel| !rel.is_empty()).map(str::to_owned)
}

/// The title a failed call without a ticket is reported under; `None`: it is only logged.
fn failure_title(call: &AgentCall) -> Option<&'static str> {
    match call {
        AgentCall::Prompt { .. } => Some("Couldn't send the message"),
        AgentCall::SetConfig { .. } => Some("Couldn't change the setting"),
        AgentCall::StopBackground { .. } => Some("Couldn't stop the background task"),
        AgentCall::Unqueue { .. } | AgentCall::SendQueuedNow { .. } => Some("Couldn't update the queue"),
        AgentCall::Rename { .. } => Some("Couldn't rename the task"),
        _ => None,
    }
}

/// Runs `call` against the project of window `label`.
async fn run_call(host: &ide_host::Host, label: &str, call: AgentCall) -> Result<CallAnswer, String> {
    use ide_host::ops::agent;
    fn done(result: ide_core::IdeResult<()>) -> ide_core::IdeResult<CallAnswer> {
        result.map(|()| CallAnswer::Done)
    }
    let answer = match call {
        AgentCall::Prompt { task, prompt, now } => done(agent::prompt(host, label, task, prompt, now).await),
        AgentCall::Cancel { task, subagent } => done(agent::cancel(host, label, task, subagent)),
        AgentCall::Rollback { task, turns } => {
            agent::rollback(host, label, task, turns).await.map(CallAnswer::RolledBack)
        }
        AgentCall::Fork { task, turns } => {
            agent::fork(host, label, task, turns).await.map(|copy| CallAnswer::Forked(Box::new(copy)))
        }
        AgentCall::Unqueue { task, queued } => agent::unqueue(host, label, task, queued).map(CallAnswer::Unqueued),
        AgentCall::SendQueuedNow { task, queued } => done(agent::send_queued_now(host, label, task, queued)),
        AgentCall::SetConfig { task, option, value } => done(agent::set_config(host, label, task, option, value).await),
        AgentCall::StopBackground { task, background } => {
            done(agent::stop_background(host, label, task, background).await)
        }
        AgentCall::Respond { task, permission, option } => done(agent::respond(host, label, task, permission, option)),
        AgentCall::Archive { task } => done(agent::archive(host, label, task)),
        AgentCall::Unarchive { task } => done(agent::unarchive(host, label, task)),
        AgentCall::Rename { task, title } => done(agent::rename(host, label, task, title)),
        AgentCall::Delete { task, remove_worktree } => done(agent::delete(host, label, task, remove_worktree).await),
    };
    answer.map_err(|error| error.message)
}

/// Registers the Agents actions.
pub(super) fn register_actions(registry: &mut ActionRegistry) {
    registry.handle("agents.showToolWindow", |_, window, cx| {
        on_workbench(window, cx, |workbench, window, cx| {
            workbench.with_layout(window, cx, |layout| layout.show(TOOL_WINDOW, true));
        })
    });
    registry.handle("agents.newTask", |call, window, cx| {
        let prompt = text_arg(call.arg);
        on_workbench(window, cx, move |workbench, _, cx| {
            workbench.agents_effects(cx, |store| store.open_home(prompt.as_deref(), None));
        })
    });
    for (id, location) in
        [("agents.useToolWindow", SessionLocation::ToolWindow), ("agents.useEditor", SessionLocation::Editor)]
    {
        registry.handle(id, move |_, window, cx| {
            on_workbench(window, cx, move |workbench, _, cx| {
                let editors = workbench.agent_editors();
                workbench.agents_effects(cx, |store| store.set_session_location(location, &editors));
            })
        });
    }
    registry.handle("agents.openTask", |call, window, cx| {
        let Some(task) = text_arg(call.arg) else { return Ok(()) };
        on_workbench(window, cx, move |workbench, _, cx| workbench.open_agent_task(&task, cx))
    });
    registry.handle("agents.dockTask", |call, window, cx| {
        let Some(task) = text_arg(call.arg) else { return Ok(()) };
        on_workbench(window, cx, move |workbench, _, cx| {
            let editors = workbench.agent_editors();
            workbench.agents_effects(cx, |store| store.dock_task(&task, &editors));
        })
    });
    registry.handle("agents.undockTask", |_, window, cx| {
        on_workbench(window, cx, |workbench, _, cx| {
            let editors = workbench.agent_editors();
            workbench.agents_effects(cx, |store| store.undock_task(&editors));
        })
    });
    registry.handle("agents.openTaskInEditor", |call, window, cx| {
        let Some(task) = text_arg(call.arg) else { return Ok(()) };
        on_workbench(window, cx, move |workbench, _, cx| {
            let editors = workbench.agent_editors();
            workbench.agents_effects(cx, |store| store.open_task_in_editor(&task, &editors));
        })
    });
    registry.handle("agents.renameTask", |call, window, cx| {
        let Some(task) = text_arg(call.arg) else { return Ok(()) };
        on_workbench(window, cx, move |workbench, _, cx| workbench.rename_agent_task(task, cx))
    });
    registry.handle("agents.forkTask", |call, window, cx| {
        let Some(task) = text_arg(call.arg) else { return Ok(()) };
        on_workbench(window, cx, move |workbench, _, cx| workbench.fork_agent_task(&task, cx))
    });
    type Batch = fn(Vec<String>) -> TaskCommand;
    let batches: [(&str, Batch); 3] = [
        ("agents.archiveTasks", TaskCommand::Archive),
        ("agents.unarchiveTasks", TaskCommand::Unarchive),
        ("agents.deleteTasks", TaskCommand::Delete),
    ];
    for (id, command) in batches {
        registry.handle(id, move |call, window, cx| {
            let ids = ids_arg(call.arg);
            on_workbench(window, cx, move |workbench, window, cx| {
                workbench.run_task_command(command(ids), window, cx);
            })
        });
    }
    registry.handle("agents.jumpToLatest", |_, window, cx| {
        on_workbench(window, cx, |workbench, _, cx| {
            if let Some(session) = workbench.docked_session() {
                session.update(cx, |session, cx| session.jump_to_latest(cx));
            }
        })
    });
    registry.handle("agents.addAgent", |_, window, cx| {
        let page = Value::from(REGISTRY_PAGE);
        window.defer(cx, move |window, cx| {
            if !execute_action_from(ActionSource::Other, "file.settings", Some(&page), window, cx) {
                tracing::info!("file.settings is not available in this window");
            }
        });
        Ok(())
    });
    registry.handle("agents.sendToAgent", |call, window, cx| {
        // Project view files come as a list of paths; without one the active editor's selection goes.
        let paths = call
            .arg
            .and_then(Value::as_array)
            .map(|paths| paths.iter().filter_map(Value::as_str).map(str::to_owned).collect::<Vec<String>>());
        on_workbench(window, cx, move |workbench, _, cx| workbench.send_to_agent(paths, cx))
    });
    registry.handle("agents.copyReplyMarkdown", |call, _, cx| {
        let text = call.arg.and_then(|arg| arg.get("text")).and_then(Value::as_str).filter(|t| !t.trim().is_empty());
        if let Some(text) = text {
            cx.write_to_clipboard(ClipboardItem::new_string(text.to_owned()));
        }
        Ok(())
    });
    registry.handle("agents.quoteReply", |call, window, cx| {
        let quote = call
            .arg
            .and_then(|arg| Some((arg.get("task")?.as_str()?.to_owned(), arg.get("text")?.as_str()?.to_owned())));
        let Some((task, text)) = quote else { return Ok(()) };
        on_workbench(window, cx, move |workbench, _, cx| {
            workbench.agents_message_action(MessageAction::Quote { task, text }, cx);
        })
    });
    type Reply = fn(ReplyTarget) -> MessageAction;
    let replies: [(&str, Reply); 3] = [
        ("agents.regenerateReply", MessageAction::Regenerate),
        ("agents.forkReply", MessageAction::Branch),
        ("agents.retractMessage", MessageAction::Retract),
    ];
    for (id, action) in replies {
        registry.handle(id, move |call, window, cx| {
            let Some(target) = reply_target(call.arg) else { return Ok(()) };
            on_workbench(window, cx, move |workbench, _, cx| workbench.agents_message_action(action(target), cx))
        });
    }
}

impl Workbench {
    // ---------------------------------------------------------------------------------------------
    // The slot

    /// The project just opened in `window` gets its agents: the store starts from what UI storage
    /// kept (the New Task settings, the message history, where conversations show) and subscribes.
    pub(super) fn mount_agents(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.drop_agents(cx);
        if self.project_root(cx).is_none() {
            return;
        }
        let stored = |key: &str| cx.try_global::<UiStorage>().and_then(|storage| storage.get(key)).map(str::to_owned);
        let (draft, history, location) = (stored(DRAFT_KEY), stored(HISTORY_KEY), stored(SESSION_LOCATION_KEY));
        let saved =
            Saved { draft: draft.as_deref(), history: history.as_deref(), session_location: location.as_deref() };
        let store = cx.new(|_| AgentsStore::new(saved));
        let observer = cx.observe(&store, |workbench, _, cx| workbench.sync_agents_window(cx));
        let trusted = self.agents_project_trusted(cx);
        self.agents = Some(AgentsSlot {
            store: store.clone(),
            models: AgentModels::default(),
            window: window.window_handle(),
            trusted,
            list: None,
            docked: None,
            home: None,
            shown_view: None,
            focus_served: 0,
            purposes: HashMap::new(),
            connection: None,
            _observer: observer,
        });
        let effects = store.update(cx, |store, _| store.connect(trusted));
        self.carry_out_agents(effects, cx);
    }

    /// The project goes: its agents' events and answers are void from now on, and its views close.
    pub(super) fn drop_agents(&mut self, cx: &mut Context<Self>) {
        let Some(slot) = self.agents.take() else { return };
        if let Some(docked) = &slot.docked {
            docked.session.update(cx, |session, cx| session.set_shown(false, cx));
        }
        slot.store.update(cx, |store, cx| {
            store.disconnect();
            cx.notify();
        });
    }

    /// Whether the window's project is trusted: an untrusted one lists no agents.
    fn agents_project_trusted(&self, cx: &App) -> bool {
        cx.try_global::<Backend>()
            .and_then(|backend| backend.host.project(&self.label).ok())
            .is_some_and(|project| project.is_trusted())
    }

    /// The project's trust changed: the agents are subscribed to again, now with (or without) them.
    pub(super) fn agents_trust_changed(&mut self, trusted: bool, cx: &mut Context<Self>) {
        let Some(slot) = self.agents.as_mut().filter(|slot| slot.trusted != trusted) else { return };
        slot.trusted = trusted;
        let store = slot.store.clone();
        let effects = store.update(cx, |store, cx| {
            cx.notify();
            store.connect(trusted)
        });
        self.carry_out_agents(effects, cx);
    }

    /// The window was focused or shown, or lost focus or was hidden: conversations on screen count as
    /// read only while it is active.
    pub(super) fn agents_window_active(&mut self, active: bool, cx: &mut Context<Self>) {
        let Some(store) = self.agents.as_ref().map(|slot| slot.store.clone()) else { return };
        store.update(cx, |store, cx| {
            store.set_window_active(active);
            cx.notify();
        });
    }

    /// The editor area as the Agents feature sees it: file tabs only.
    fn agent_editors(&self) -> Editors {
        Editors { active: self.editor().map(|_| AgentTab::Other), open: Vec::new() }
    }

    /// Runs `f` on the store, the feature's other models and the UI text, and tells the store's
    /// observers; `None` without a project.
    fn agents_step<R>(
        &mut self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut AgentsStore, &mut AgentModels, &mut I18n) -> R,
    ) -> Option<R> {
        let slot = self.agents.as_mut()?;
        let store = slot.store.clone();
        let models = &mut slot.models;
        Some(cx.update_default_global::<Localization, _>(|localization, cx| {
            store.update(cx, |store, cx| {
                cx.notify();
                f(store, models, &mut localization.0)
            })
        }))
    }

    /// Changes the store with `f` and carries out what it returns.
    fn agents_effects(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut AgentsStore) -> Vec<AgentEffect>) {
        let effects = self.agents_step(cx, |store, _, _| f(store)).unwrap_or_default();
        self.carry_out_agents(effects, cx);
    }

    /// Runs `f` in the window once this update is over.
    fn in_agents_window(
        &self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let Some(handle) = self.agents.as_ref().map(|slot| slot.window) else { return };
        cx.spawn(async move |this, cx| {
            // The window is gone: there is nothing left to do in it.
            let _ = handle.update(cx, |_, window, cx| {
                let _ = this.update(cx, |workbench, cx| f(workbench, window, cx));
            });
        })
        .detach();
    }

    /// Runs `work` on the backend's runtime with the host and the window's label, then gives its answer
    /// to `answer`, unless the project changed meanwhile.
    fn agents_backend<T, F>(
        &mut self,
        cx: &mut Context<Self>,
        work: impl FnOnce(Arc<ide_host::Host>, String) -> F + Send + 'static,
        answer: impl FnOnce(&mut Self, Result<T, String>, &mut Context<Self>) + 'static,
    ) where
        T: Send + 'static,
        F: Future<Output = Result<T, String>> + Send + 'static,
    {
        let Some(store) = self.agents.as_ref().map(|slot| slot.store.entity_id()) else { return };
        let label = self.label.clone();
        let call = cx.try_global::<Backend>().map(|backend| backend.with_host(move |host| work(host, label)));
        cx.spawn(async move |this, cx| {
            let result = match call {
                Some(call) => call.await.unwrap_or_else(|error| Err(error.to_string())),
                None => Err("the backend is not running".to_owned()),
            };
            let answered = this.update(cx, |workbench, cx| {
                if workbench.agents.as_ref().is_some_and(|slot| slot.store.entity_id() == store) {
                    answer(workbench, result, cx);
                }
            });
            if answered.is_err() {
                tracing::debug!("the window closed before an agent request answered");
            }
        })
        .detach();
    }

    /// Calls `f` after `delay_ms`, unless the project changed meanwhile.
    fn agents_after(
        &mut self,
        delay_ms: f64,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut Self, &mut Context<Self>) + 'static,
    ) {
        let Some(store) = self.agents.as_ref().map(|slot| slot.store.entity_id()) else { return };
        let wait = Duration::from_secs_f64((delay_ms / 1000.0).max(0.0));
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(wait).await;
            let _ = this.update(cx, |workbench, cx| {
                if workbench.agents.as_ref().is_some_and(|slot| slot.store.entity_id() == store) {
                    f(workbench, cx);
                }
            });
        })
        .detach();
    }

    // ---------------------------------------------------------------------------------------------
    // The subscription

    /// Subscribes to the project's agent events (and lists the agents when `providers`): the tasks and
    /// the agents go to the store as the answer of subscription `generation`, then every event as it
    /// arrives, in order. A later subscription replaces this one.
    fn agents_subscribe(&mut self, generation: u64, providers: bool, cx: &mut Context<Self>) {
        let label = self.label.clone();
        let Some(slot) = self.agents.as_mut() else { return };
        let Some(backend) = cx.try_global::<Backend>() else { return };
        let store = slot.store.clone();
        let (sender, mut events) = unbounded::<AgentEvent>();
        let emit: ide_host::ops::Emit<AgentEvent> = Arc::new(move |event| {
            // The receiver is gone only when the subscription was replaced or the window closed.
            let _ = sender.unbounded_send(event);
        });
        let subscribed = backend.with_host(move |host| async move {
            let tasks = ide_host::ops::agent::subscribe(&host, &label, emit).map_err(|error| error.message)?;
            let providers = match providers {
                true => ide_host::ops::agent::providers(&host, &label).map_err(|error| error.message)?,
                false => Vec::new(),
            };
            Ok::<_, String>((tasks, providers))
        });
        slot.connection = Some(cx.spawn(async move |this, cx| {
            let answer = subscribed.await.unwrap_or_else(|error| Err(error.to_string()));
            let connected = this.update(cx, |workbench, cx| {
                let editors = workbench.agent_editors();
                let effects = workbench.agents_step(cx, |store, _, _| store.connected(generation, answer, &editors));
                workbench.carry_out_agents(effects.unwrap_or_default(), cx);
            });
            if connected.is_err() {
                return;
            }
            while let Some(first) = events.next().await {
                // What has arrived together is applied together.
                let mut batch = vec![first];
                while let Ok(more) = events.try_recv() {
                    batch.push(more);
                }
                let applied = this.update(cx, |workbench, cx| workbench.agents_events(&store, generation, batch, cx));
                if applied.is_err() {
                    return;
                }
            }
        }));
    }

    /// Events of subscription `generation` of `store`.
    fn agents_events(
        &mut self,
        store: &Entity<AgentsStore>,
        generation: u64,
        events: Vec<AgentEvent>,
        cx: &mut Context<Self>,
    ) {
        if self.agents.as_ref().is_none_or(|slot| slot.store != *store) {
            return;
        }
        let effects = self.agents_step(cx, |store, _, i18n| {
            let mut effects = Vec::new();
            for event in events {
                effects.extend(store.on_event(generation, event, i18n));
            }
            effects
        });
        self.carry_out_agents(effects.unwrap_or_default(), cx);
    }

    // ---------------------------------------------------------------------------------------------
    // The effects

    /// Carries out what the store and the reply actions ask for.
    fn carry_out_agents(&mut self, effects: Vec<AgentEffect>, cx: &mut Context<Self>) {
        for effect in effects {
            match effect {
                AgentEffect::Subscribe { generation, providers } => self.agents_subscribe(generation, providers, cx),
                // No editor tab shows an agent task, so there is no tab title to follow.
                AgentEffect::TabTitlesChanged => {}
                AgentEffect::CloseTab { key } => self.close_agent_placement(&key, cx),
                AgentEffect::OpenSession { task, .. } => self.dock_agent_view(Some(task), cx),
                AgentEffect::OpenHome => self.dock_agent_view(None, cx),
                AgentEffect::ShowToolWindow { id } => self.in_agents_window(cx, move |workbench, window, cx| {
                    workbench.with_layout(window, cx, |layout| layout.show(id, true));
                }),
                AgentEffect::Notify(request) => self.agents_notify(*request, cx),
                AgentEffect::LoadEntries { task, subagent } => self.load_agent_entries(task, subagent, cx),
                AgentEffect::RefreshChanges { task } => {
                    let key = task.clone();
                    self.agents_backend(
                        cx,
                        move |host, label| async move {
                            ide_host::ops::agent::changes(&host, &label, task).await.map_err(|error| error.message)
                        },
                        move |workbench, answer, cx| {
                            workbench.agents_step(cx, |store, _, _| store.changes_loaded(&key, answer));
                        },
                    );
                }
                AgentEffect::ScheduleChanges { task, delay_ms } => {
                    self.agents_after(delay_ms, cx, move |workbench, cx| {
                        let effect = workbench.agents_step(cx, |store, _, _| store.changes_due(&task)).flatten();
                        workbench.carry_out_agents(effect.into_iter().collect(), cx);
                    });
                }
                AgentEffect::RefreshProviders => self.agents_backend(
                    cx,
                    |host, label| async move {
                        ide_host::ops::agent::providers(&host, &label).map_err(|error| error.message)
                    },
                    |workbench, answer, cx| {
                        workbench.agents_step(cx, |store, _, _| store.providers_loaded(answer.ok()));
                    },
                ),
                AgentEffect::Save { key, value } => cx.default_global::<UiStorage>().set(key, value),
                AgentEffect::Create { ticket, request } => self.create_agent_task(ticket, *request, cx),
                AgentEffect::Call { ticket, call } => self.run_agent_call(ticket, call, cx),
                AgentEffect::Confirm { ticket, request } => self.confirm_agents(ticket, request, cx),
                AgentEffect::WakeAt { at_ms } => self.agents_after(at_ms - now_ms(), cx, |workbench, cx| {
                    let now = now_ms();
                    let effects = workbench.agents_step(cx, |store, _, i18n| store.tick(now, i18n));
                    workbench.carry_out_agents(effects.unwrap_or_default(), cx);
                }),
                AgentEffect::Finished { ticket, outcome } => self.agent_request_finished(ticket, outcome, cx),
            }
        }
    }

    /// Shows the conversation of `task` (the New Task page for `None`) docked in the tool window: the
    /// store places it there whatever the placement preference says, since the editor area holds files
    /// only.
    fn dock_agent_view(&mut self, task: Option<String>, cx: &mut Context<Self>) {
        let Some(store) = self.agents.as_ref().map(|slot| slot.store.clone()) else { return };
        store.update(cx, |store, cx| {
            store.home_open = task.is_none();
            store.docked = task;
            cx.notify();
        });
        self.in_agents_window(cx, |workbench, window, cx| {
            workbench.with_layout(window, cx, |layout| layout.show(TOOL_WINDOW, true));
        });
    }

    /// An editor tab of the feature closes. Placed in the tool window by preference, nothing is in a
    /// tab; placed "in the editor" (docked here), the conversation or New Task page it names leaves.
    fn close_agent_placement(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(store) = self.agents.as_ref().map(|slot| slot.store.clone()) else { return };
        store.update(cx, |store, cx| {
            if store.session_location != SessionLocation::Editor {
                return;
            }
            if key == HOME_KEY {
                store.home_open = false;
            } else if key.strip_prefix("agent:").is_some_and(|task| store.docked.as_deref() == Some(task)) {
                store.docked = None;
            }
            cx.notify();
        });
    }

    /// Raises a notification of the feature; its Open and Review buttons open the task it is about (the
    /// task is the notification's key, `agent:<id>`).
    fn agents_notify(&mut self, mut request: NotificationRequest, cx: &mut Context<Self>) {
        let task = request.key.as_deref().and_then(|key| key.strip_prefix("agent:")).map(str::to_owned);
        if let Some(task) = task {
            let command = format!("{OPEN_TASK_COMMAND}:{task}");
            for action in request.actions.iter_mut().filter(|action| action.command == OPEN_TASK_COMMAND) {
                action.command.clone_from(&command);
            }
        }
        self.notify(request, cx);
    }

    /// Reports a failure the user should know about.
    fn agents_error(&mut self, title: &str, body: String, cx: &mut Context<Self>) {
        let mut request = NotificationRequest::new(NotificationLevel::Error, title);
        request.body = Some(body);
        self.notify(request, cx);
    }

    fn load_agent_entries(&mut self, task: String, subagent: Option<String>, cx: &mut Context<Self>) {
        let (key, sub) = (task.clone(), subagent.clone());
        self.agents_backend(
            cx,
            move |host, label| async move {
                ide_host::ops::agent::entries(&host, &label, task, subagent).map_err(|error| error.message)
            },
            move |workbench, answer, cx| match answer {
                Ok(entries) => {
                    workbench.agents_step(cx, |store, _, _| store.entries_loaded(&key, sub.as_deref(), entries));
                }
                Err(error) => tracing::warn!(task = %key, "cannot read the agent task's conversation: {error}"),
            },
        );
    }

    /// Creates a task. Its entries may arrive before the answer and find no conversation to land in, so
    /// the conversation is read once the task exists.
    fn create_agent_task(&mut self, ticket: Ticket, request: ide_agent::NewAgentTask, cx: &mut Context<Self>) {
        if let Some(slot) = self.agents.as_mut() {
            slot.purposes.insert(ticket, Purpose::Create);
        }
        self.agents_backend(
            cx,
            move |host, label| async move {
                ide_host::ops::agent::create(&host, &label, request).await.map_err(|error| error.message)
            },
            move |workbench, answer: Result<AgentTask, String>, cx| {
                let created = answer.as_ref().ok().map(|task| task.id.clone());
                let effects = workbench.agents_step(cx, |store, _, _| store.task_created(ticket, answer));
                workbench.carry_out_agents(effects.unwrap_or_default(), cx);
                if let Some(task) = created {
                    workbench.carry_out_agents(vec![AgentEffect::LoadEntries { task, subagent: None }], cx);
                }
            },
        );
    }

    /// Runs a backend call off the main thread. A call with a ticket is answered to every holder of
    /// tickets; a failed one without is reported (or logged, when nothing is to be said).
    fn run_agent_call(&mut self, ticket: Option<Ticket>, call: AgentCall, cx: &mut Context<Self>) {
        let failure = failure_title(&call);
        self.agents_backend(
            cx,
            move |host, label| async move { run_call(&host, &label, call).await },
            move |workbench, answer, cx| match (ticket, answer) {
                (Some(ticket), answer) => workbench.agent_call_answered(ticket, answer, cx),
                (None, Ok(_)) => {}
                (None, Err(error)) => match failure {
                    Some(title) => workbench.agents_error(title, error, cx),
                    None => tracing::warn!("an agent request failed: {error}"),
                },
            },
        );
    }

    /// The answer of a call with a ticket, for the store, the task batches, the docked conversation and
    /// its composer.
    fn agent_call_answered(&mut self, ticket: Ticket, answer: Result<CallAnswer, String>, cx: &mut Context<Self>) {
        let result: Result<(), String> = answer.as_ref().map(|_| ()).map_err(String::clone);
        let answered = self.agents_step(cx, |store, models, _| {
            let effects = store.call_answered(ticket, answer);
            let batch = models.batches.answered(store, ticket, result.clone());
            (effects, batch)
        });
        let (effects, batch) = answered.unwrap_or_default();
        if let Some(docked) = self.agents.as_ref().and_then(|slot| slot.docked.as_ref()) {
            let (session, composer) = (docked.session.clone(), docked.composer.clone());
            session.update(cx, |session, cx| session.answered(ticket, result, cx));
            composer.update(cx, |composer, cx| composer.call_answered(ticket, cx));
        }
        self.carry_out_agents(effects, cx);
        self.carry_out_batch(batch, cx);
    }

    /// A request started with `ticket` ended: the reply actions free their task (and report a failure
    /// of theirs); the window's own requests say how it went.
    fn agent_request_finished(&mut self, ticket: Ticket, outcome: Outcome, cx: &mut Context<Self>) {
        let reported = self.agents_step(cx, |_, models, i18n| models.replies.finished(ticket, &outcome, i18n));
        if let Some(effect) = reported.flatten() {
            self.carry_out_agents(vec![effect], cx);
        }
        let Some(slot) = self.agents.as_mut() else { return };
        match slot.purposes.remove(&ticket) {
            Some(Purpose::EditSend(task)) => {
                let session = slot.docked.as_ref().filter(|docked| docked.task == task).map(|d| d.session.clone());
                let sent = match &outcome {
                    Ok(true) => Some(Ok(())),
                    // Declined: the edit stays open as it was.
                    Ok(false) => None,
                    Err(error) => Some(Err(error.clone())),
                };
                if let (Some(session), Some(sent)) = (session, sent) {
                    session.update(cx, |session, cx| session.edit_sent(sent, cx));
                }
                if let Err(error) = outcome {
                    self.agents_error("Couldn't send the message", error, cx);
                }
            }
            Some(Purpose::Failure(title)) => {
                if let Err(error) = outcome {
                    self.agents_error(title, error, cx);
                }
            }
            Some(Purpose::Create) => {
                if let Some((page, _)) = &slot.home {
                    let page = page.clone();
                    page.update(cx, |page, cx| page.finished(outcome.err(), cx));
                } else if let Err(error) = outcome {
                    self.agents_error("Couldn't start the task", error, cx);
                }
            }
            None => {}
        }
    }

    /// Asks to go ahead with a step of an edit or a reply action; closing the dialog declines.
    fn confirm_agents(&mut self, ticket: Ticket, request: ConfirmRequest, cx: &mut Context<Self>) {
        self.in_agents_window(cx, move |workbench, window, cx| {
            let confirm = crate::vcs_ops::dialogs::ConfirmRequest {
                title: request.title,
                message: request.message,
                ok_label: request.ok,
                danger: true,
            };
            let dialog =
                move |window: &mut Window, cx: &mut Context<PromptDialog>| PromptDialog::confirm(confirm, window, cx);
            let answer = workbench.show_prompt_dialog(dialog, window, cx);
            cx.spawn(async move |this, cx| {
                // A dialog replaced or closed with the window declines.
                let ok = answer.await.is_ok_and(|answer| answer.confirmed());
                let _ = this.update(cx, |workbench, cx| workbench.agents_confirmed(ticket, ok, cx));
            })
            .detach();
        });
    }

    fn agents_confirmed(&mut self, ticket: Ticket, ok: bool, cx: &mut Context<Self>) {
        let now = now_ms();
        let effects = self.agents_step(cx, |store, models, _| {
            models.replies.confirmed(store, ticket, ok, now).unwrap_or_else(|| store.confirmed(ticket, ok, now))
        });
        self.carry_out_agents(effects.unwrap_or_default(), cx);
    }

    // ---------------------------------------------------------------------------------------------
    // The task batches

    fn agents_batch(
        &mut self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut AgentsStore, &mut TaskBatches, &mut I18n) -> Option<BatchEffect>,
    ) {
        let effect = self.agents_step(cx, |store, models, i18n| f(store, &mut models.batches, i18n)).flatten();
        self.carry_out_batch(effect, cx);
    }

    fn carry_out_batch(&mut self, effect: Option<BatchEffect>, cx: &mut Context<Self>) {
        match effect {
            None => {}
            Some(BatchEffect::Call { ticket, call }) => self.run_agent_call(Some(ticket), call, cx),
            Some(BatchEffect::Confirm { ticket, request }) => self.confirm_batch(ticket, request, cx),
            Some(BatchEffect::ShowError { title, message }) => self.show_agents_error(title, message, cx),
        }
    }

    /// Asks once to delete the tasks of a batch, with the checkbox that removes their worktree folders.
    fn confirm_batch(&mut self, ticket: Ticket, request: ConfirmChecked, cx: &mut Context<Self>) {
        self.in_agents_window(cx, move |workbench, window, cx| {
            let ask = AskRequest {
                title: request.title,
                body: request.message,
                warning: true,
                width: None,
                checkbox: request.checkbox,
                buttons: vec![
                    AskButton { id: OK, label: "Delete", primary: true },
                    AskButton { id: CANCEL, label: "Cancel", primary: false },
                ],
            };
            let answer = workbench.show_prompt_dialog(move |window, cx| PromptDialog::ask(ask, window, cx), window, cx);
            cx.spawn(async move |this, cx| {
                let checked = answer.await.ok().filter(DialogAnswer::confirmed).map(|answer| answer.checked);
                let _ = this.update(cx, |workbench, cx| {
                    workbench.agents_batch(cx, |store, batches, i18n| batches.confirmed(store, ticket, checked, i18n));
                });
            })
            .detach();
        });
    }

    /// The failures of a batch, in a message box.
    fn show_agents_error(&mut self, title: String, message: String, cx: &mut Context<Self>) {
        self.in_agents_window(cx, move |workbench, window, cx| {
            let ask = AskRequest {
                title,
                body: message,
                warning: true,
                width: None,
                checkbox: None,
                buttons: vec![AskButton { id: OK, label: "OK", primary: true }],
            };
            // Nothing waits for the answer.
            drop(workbench.show_prompt_dialog(move |window, cx| PromptDialog::ask(ask, window, cx), window, cx));
        });
    }

    /// Carries out a row of a task's menu.
    fn run_task_command(&mut self, command: TaskCommand, window: &mut Window, cx: &mut Context<Self>) {
        match command {
            TaskCommand::Open(task) => self.open_agent_task(&task, cx),
            TaskCommand::Rename(task) => self.rename_agent_task(task, cx),
            TaskCommand::OpenInTerminal(cwd) => {
                let arg = Value::String(cwd);
                window.defer(cx, move |window, cx| {
                    if !execute_action_from(ActionSource::ContextMenu, "terminal.openAt", Some(&arg), window, cx) {
                        tracing::info!("terminal.openAt is not available in this window");
                    }
                });
            }
            TaskCommand::Fork(task) => self.fork_agent_task(&task, cx),
            TaskCommand::Archive(ids) => {
                self.agents_batch(cx, |store, batches, i18n| batches.archive(store, &ids, i18n));
            }
            TaskCommand::Unarchive(ids) => {
                self.agents_batch(cx, |store, batches, i18n| batches.unarchive(store, &ids, i18n));
            }
            TaskCommand::Delete(ids) => {
                self.agents_batch(cx, |store, batches, i18n| batches.delete(store, &ids, i18n));
            }
        }
    }

    /// Opens a task where conversations show: Open of its menu, of a notification, of the action.
    pub(super) fn open_agent_task(&mut self, task: &str, cx: &mut Context<Self>) {
        self.agents_effects(cx, |store| store.open_task(task));
    }

    /// Copy to a New Task: the whole conversation, for an idle task whose agent can copy sessions.
    fn fork_agent_task(&mut self, task: &str, cx: &mut Context<Self>) {
        let forked = self.agents_step(cx, |store, _, _| {
            let can = store.task(task).is_some_and(can_fork);
            can.then(|| store.fork_task(task, None))
        });
        let Some((ticket, effect)) = forked.flatten() else { return };
        if let Some(slot) = self.agents.as_mut() {
            slot.purposes.insert(ticket, Purpose::Failure("Couldn't copy the task"));
        }
        self.carry_out_agents(vec![effect], cx);
    }

    /// Rename…: asks for the task's new name, then renames it.
    fn rename_agent_task(&mut self, task: String, cx: &mut Context<Self>) {
        let title = self.agents.as_ref().and_then(|slot| slot.store.read(cx).task(&task).map(|t| t.title.clone()));
        let Some(title) = title else { return };
        self.in_agents_window(cx, move |workbench, window, cx| {
            let request = PromptRequest {
                title: "Rename Agent Task".to_owned(),
                label: Some("Name".to_owned()),
                initial: title,
                ok_label: "Rename".to_owned(),
                ..PromptRequest::default()
            };
            let rule = FieldRule::NonEmpty("Enter a name".to_owned());
            let dialog = move |window: &mut Window, cx: &mut Context<PromptDialog>| {
                PromptDialog::prompt(request, rule, window, cx)
            };
            let answer = workbench.show_prompt_dialog(dialog, window, cx);
            cx.spawn(async move |this, cx| {
                let Some(renamed) = answer.await.ok().and_then(|answer| answer.prompt_result()) else { return };
                let _ = this.update(cx, |workbench, cx| {
                    workbench.run_agent_call(None, AgentCall::Rename { task, title: renamed.value }, cx);
                });
            })
            .detach();
        });
    }

    // ---------------------------------------------------------------------------------------------
    // Send to Agent and the reply actions

    /// Send to Agent: the files of `paths` (Project view), or the active editor's file with its selected
    /// lines, as `@` references in the composer on show, which shows.
    fn send_to_agent(&mut self, paths: Option<Vec<String>>, cx: &mut Context<Self>) {
        let Some(root) = self.project_root(cx).map(|root| ide_core::ipc_path::to_ipc(&root)) else { return };
        let refs: Vec<Reference> = match paths {
            Some(paths) => paths
                .into_iter()
                .filter_map(|path| {
                    let rel = relative_to(&root, &path)?;
                    Some(Reference { mention: mention_of(&rel, None), path, rel })
                })
                .collect(),
            None => {
                let Some(editor) = self.editor().cloned() else { return };
                let editor = editor.read(cx);
                let path = ide_core::ipc_path::to_ipc(editor.path());
                let Some(rel) = relative_to(&root, &path) else { return };
                let (selection, range) = (editor.selection(), editor.selection_range());
                let lines = (selection.start != selection.end).then(|| {
                    // A selection ending at a line start does not include that line.
                    let last = if range.end.col == 0 && range.end.line > range.start.line {
                        range.end.line - 1
                    } else {
                        range.end.line
                    };
                    LineRange { from: range.start.line + 1, to: last + 1 }
                });
                vec![Reference { mention: mention_of(&rel, lines), path, rel }]
            }
        };
        if refs.is_empty() {
            return;
        }
        let editors = self.agent_editors();
        self.agents_effects(cx, |store| store.add_references(&refs, &editors));
    }

    /// What a message of a conversation (or a reply action) asks for.
    fn agents_message_action(&mut self, action: MessageAction, cx: &mut Context<Self>) {
        let now = now_ms();
        let stepped = self.agents_step(cx, |store, models, i18n| match action {
            MessageAction::Quote { task, text } => {
                models.replies.quote(store, &task, &text);
                (Vec::new(), None)
            }
            MessageAction::Regenerate(target) => (models.replies.regenerate(store, &target, now, i18n), None),
            MessageAction::Branch(target) => (models.replies.fork(store, &target), None),
            MessageAction::Retract(target) => (models.replies.retract(store, &target, now, i18n), None),
            MessageAction::RunAgain { target, text, images } => {
                let (ticket, effects) =
                    store.rerun_from(&target.task, turn_number(target.turn), &text, &images, now, i18n);
                (effects, Some((ticket, Purpose::Failure("Couldn't send the message"))))
            }
            MessageAction::SendAgain { task, text } => {
                let prompt = AgentPrompt { text, files: Vec::new(), images: Vec::new() };
                (vec![AgentEffect::Call { ticket: None, call: AgentCall::Prompt { task, prompt, now: false } }], None)
            }
            MessageAction::Fork(target) => {
                let (ticket, effect) = store.fork_task(&target.task, Some(turn_number(target.turn + 1)));
                (vec![effect], Some((ticket, Purpose::Failure("Couldn't copy the task"))))
            }
            MessageAction::EditSend { target, text, images } => {
                let (ticket, effects) =
                    store.rerun_from(&target.task, turn_number(target.turn), &text, &images, now, i18n);
                (effects, Some((ticket, Purpose::EditSend(target.task))))
            }
        });
        let Some((effects, purpose)) = stepped else { return };
        if let (Some((ticket, purpose)), Some(slot)) = (purpose, self.agents.as_mut()) {
            slot.purposes.insert(ticket, purpose);
        }
        self.carry_out_agents(effects, cx);
    }

    /// A link of a conversation: a web page opens in the browser, a file in the editor.
    fn open_agent_link(&mut self, link: LinkAction, cx: &mut Context<Self>) {
        match link {
            LinkAction::External(url) => cx.open_url(&url),
            LinkAction::File(file) => self.in_agents_window(cx, move |workbench, window, cx| {
                let path = PathBuf::from(file.path);
                match file.line {
                    Some(line) => workbench.open_project_file_at(path, line, file.col.unwrap_or(0), window, cx),
                    None => workbench.open_project_file(path, window, cx),
                }
            }),
            LinkAction::None => {}
        }
    }

    // ---------------------------------------------------------------------------------------------
    // The tool window

    /// The Agents tool window's panel: the task list, with the docked conversation or New Task page.
    pub(super) fn mount_agents_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<Panel> {
        let store = self.agents.as_ref()?.store.clone();
        let active = self.editor().map(|_| AgentTab::Other);
        let view = cx.new(|cx| TaskListView::new(store, active.as_ref(), cx));
        let events = cx.subscribe_in(&view, window, Self::agents_list_event);
        if let Some(slot) = self.agents.as_mut() {
            slot.list = Some(view.clone());
            slot.shown_view = None;
        }
        self.sync_agents_window(cx);
        Some(Panel::new(view, Some(events)))
    }

    /// Follows the store and the tool window's visibility: the docked conversation (with its composer)
    /// or the New Task page is built for what the store docks and handed to the tool window, the
    /// conversation counts as read while the tool window shows it, and a composer that asked for the
    /// keyboard gets it.
    pub(super) fn sync_agents_window(&mut self, cx: &mut Context<Self>) {
        let visible = self.is_tool_window_visible(TOOL_WINDOW);
        let settings = crate::platform::settings::effective(&self.label, cx).agents;
        let Some(slot) = self.agents.as_mut() else { return };
        let store = slot.store.clone();
        let (docked, home, focus) = {
            let store = store.read(cx);
            let docked = store.docked.clone().filter(|task| store.task(task).is_some());
            (docked, store.home_open, store.focus().map(|request| (request.key.to_owned(), request.seq)))
        };
        if slot.docked.as_ref().map(|docked| &docked.task) != docked.as_ref() {
            if let Some(old) = slot.docked.take() {
                old.session.update(cx, |session, cx| session.set_shown(false, cx));
            }
            slot.docked = docked.as_deref().map(|task| Self::new_docked_session(&store, task, settings, cx));
        }
        let home = home && slot.docked.is_none();
        if home && slot.home.is_none() {
            let page = cx.new(|cx| NewTaskPage::new(store.clone(), cx));
            let events = cx.subscribe(&page, Self::agents_home_event);
            slot.home = Some((page, events));
        } else if !home {
            slot.home = None;
        }
        let view: Option<AnyView> = match (&slot.docked, &slot.home) {
            (Some(docked), _) => Some(docked.session.clone().into()),
            (None, Some((page, _))) => Some(page.clone().into()),
            (None, None) => None,
        };
        let id = view.as_ref().map(AnyView::entity_id);
        if slot.shown_view != id
            && let Some(list) = slot.list.clone()
        {
            slot.shown_view = id;
            list.update(cx, |list, cx| list.set_docked_view(view, cx));
        }
        if let Some(docked) = &slot.docked {
            docked.session.update(cx, |session, cx| session.set_shown(visible, cx));
        }
        let Some((key, seq)) = focus.filter(|(_, seq)| *seq > slot.focus_served) else { return };
        slot.focus_served = seq;
        let handle = match (&slot.docked, &slot.home) {
            (Some(docked), _) if docked.task == key => Some(docked.composer.focus_handle(cx)),
            (None, Some((page, _))) if key == HOME => Some(page.focus_handle(cx)),
            _ => None,
        };
        if let Some(handle) = handle {
            self.in_agents_window(cx, move |_, window, cx| window.focus(&handle, cx));
        }
    }

    fn new_docked_session(
        store: &Entity<AgentsStore>,
        task: &str,
        settings: ide_core::settings::AgentSettings,
        cx: &mut Context<Self>,
    ) -> DockedSession {
        let session = cx.new(|cx| SessionView::new(store.clone(), task, true, cx));
        let composer = cx.new(|cx| ComposerView::new(store.clone(), task, cx));
        let events =
            [cx.subscribe(&session, Self::agents_session_event), cx.subscribe(&composer, Self::agents_composer_event)];
        let slot = composer.clone();
        session.update(cx, |session, cx| {
            session.set_agent_settings(settings, cx);
            session.set_composer(Some(slot.into()), cx);
            session.load(cx);
        });
        DockedSession { task: task.to_owned(), session, composer, _events: events }
    }

    /// The docked conversation, while one shows.
    fn docked_session(&self) -> Option<Entity<SessionView>> {
        self.agents.as_ref()?.docked.as_ref().map(|docked| docked.session.clone())
    }

    /// Publishes `agentSessionFocus`: the docked conversation or its composer has the keyboard.
    pub(super) fn publish_agents_context(&self, window: &mut Window, cx: &mut App) {
        let focused = self.agents.as_ref().and_then(|slot| slot.docked.as_ref()).is_some_and(|docked| {
            docked.session.focus_handle(cx).contains_focused(window, cx)
                || docked.composer.focus_handle(cx).contains_focused(window, cx)
        });
        set_context(window, cx, SESSION_FOCUS, focused);
    }

    /// Carries out what the Agents tool window asks for.
    fn agents_list_event(
        &mut self,
        _: &Entity<TaskListView>,
        event: &TaskListEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let editors = self.agent_editors();
        match event.clone() {
            TaskListEvent::OpenTask(task) => self.open_agent_task(&task, cx),
            TaskListEvent::DockTask(task) => self.agents_effects(cx, |store| store.dock_task(&task, &editors)),
            TaskListEvent::NewTask => self.agents_effects(cx, |store| store.open_home(None, None)),
            TaskListEvent::SetLocation(location) => {
                self.agents_effects(cx, |store| store.set_session_location(location, &editors));
            }
            TaskListEvent::ShowMenu { items, position } => {
                let run = |workbench: &mut Self, command: TaskCommand, window: &mut Window, cx: &mut Context<Self>| {
                    workbench.run_task_command(command, window, cx);
                };
                self.show_menu(&items, position, run, window, cx);
            }
        }
    }

    /// Carries out what the docked conversation asks for.
    fn agents_session_event(&mut self, _: Entity<SessionView>, event: &SessionEvent, cx: &mut Context<Self>) {
        match event.clone() {
            SessionEvent::Effects(effects) => self.carry_out_agents(effects, cx),
            SessionEvent::Message(action) => self.agents_message_action(action, cx),
            SessionEvent::Link(link) => self.open_agent_link(link, cx),
        }
    }

    /// Carries out what the docked composer asks for; a failed send is reported.
    fn agents_composer_event(&mut self, _: Entity<ComposerView>, event: &ComposerEvent, cx: &mut Context<Self>) {
        let ComposerEvent::Effects(effects) = event.clone();
        if let Some(slot) = self.agents.as_mut() {
            for effect in &effects {
                if let AgentEffect::Call { ticket: Some(ticket), call: AgentCall::Prompt { .. } } = effect {
                    slot.purposes.insert(*ticket, Purpose::Failure("Couldn't send the message"));
                }
            }
        }
        self.carry_out_agents(effects, cx);
    }

    /// Carries out what the New Task page asks for.
    fn agents_home_event(&mut self, page: Entity<NewTaskPage>, event: &NewTaskEvent, cx: &mut Context<Self>) {
        match event.clone() {
            NewTaskEvent::Start => {
                let effect = self.agents_step(cx, |store, _, _| store.start_task(None, None)).flatten();
                match effect {
                    Some(effect) => self.carry_out_agents(vec![effect], cx),
                    None => page.update(cx, |page, cx| page.finished(None, cx)),
                }
            }
            NewTaskEvent::ChooseAgent(position) => {
                let Some(store) = self.agents.as_ref().map(|slot| slot.store.clone()) else { return };
                let items: Vec<MenuItem<String>> = store
                    .read(cx)
                    .providers
                    .iter()
                    .map(|provider| {
                        MenuItem::item(provider.name.clone(), provider.id.clone()).disabled(!provider.installed)
                    })
                    .collect();
                if items.is_empty() {
                    return;
                }
                self.in_agents_window(cx, move |workbench, window, cx| {
                    let choose = |workbench: &mut Self, id: String, _: &mut Window, cx: &mut Context<Self>| {
                        workbench.choose_task_agent(id, cx);
                    };
                    workbench.show_menu(&items, position, choose, window, cx);
                });
            }
        }
    }

    /// The New Task page's agent: its settings start over.
    fn choose_task_agent(&mut self, provider: String, cx: &mut Context<Self>) {
        let effect = self.agents_step(cx, |store, _, _| {
            store.update_draft(|draft| {
                draft.provider = provider;
                draft.config.clear();
            })
        });
        self.carry_out_agents(effect.into_iter().collect(), cx);
    }
}

#[cfg(test)]
impl Workbench {
    pub(super) fn agents_store(&self) -> Option<Entity<AgentsStore>> {
        self.agents.as_ref().map(|slot| slot.store.clone())
    }

    /// The docked conversation and its composer.
    pub(super) fn docked_agent_session(&self) -> Option<(Entity<SessionView>, Entity<ComposerView>)> {
        let docked = self.agents.as_ref()?.docked.as_ref()?;
        Some((docked.session.clone(), docked.composer.clone()))
    }

    pub(super) fn agents_home_page(&self) -> Option<Entity<NewTaskPage>> {
        self.agents.as_ref()?.home.as_ref().map(|(page, _)| page.clone())
    }

    pub(super) fn agents_list(&self) -> Option<Entity<TaskListView>> {
        self.agents.as_ref()?.list.clone()
    }
}

// -------------------------------------------------------------------------------------------------
// The New Task page

/// What the New Task page asks of its window.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum NewTaskEvent {
    /// Start the task with the page's description and settings ([`AgentsStore::start_task`]).
    Start,
    /// Show the agents to choose from at `position`.
    ChooseAgent(Point<Pixels>),
}

/// The New Task page docked in the Agents tool window: the task's description (the store's New Task
/// composer), the agent it runs with, and Start, which needs a description and an agent that is listed
/// and installed.
pub(super) struct NewTaskPage {
    store: Entity<AgentsStore>,
    field: Entity<TextField>,
    starting: bool,
    error: Option<String>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<NewTaskEvent> for NewTaskPage {}

impl NewTaskPage {
    fn new(store: Entity<AgentsStore>, cx: &mut Context<Self>) -> Self {
        let text = store.read(cx).composer(HOME).text;
        let placeholder = tx(cx, "Describe a task…");
        let field = cx.new(|cx| {
            let mut field = TextField::new(cx).with_placeholder(placeholder);
            field.set_text(&text, cx);
            field
        });
        let subscriptions = vec![
            cx.observe(&store, |page, _, cx| {
                page.sync_field(cx);
                cx.notify();
            }),
            cx.subscribe(&field, |page, field, event: &TextFieldEvent, cx| match event {
                TextFieldEvent::Changed => {
                    let text = field.read(cx).text().to_owned();
                    page.store.update(cx, |store, cx| {
                        store.update_composer(HOME, |draft| draft.text = text);
                        cx.notify();
                    });
                }
                TextFieldEvent::Submitted => page.start(cx),
                TextFieldEvent::Cancelled => {}
            }),
        ];
        Self { store, field, starting: false, error: None, _subscriptions: subscriptions }
    }

    /// The store's description changed elsewhere (a prompt the action passed, a reference sent): the
    /// field shows it.
    fn sync_field(&mut self, cx: &mut Context<Self>) {
        let text = self.store.read(cx).composer(HOME).text;
        if self.field.read(cx).text() != text {
            self.field.update(cx, |field, cx| field.set_text(&text, cx));
        }
    }

    #[cfg(test)]
    pub(super) fn field(&self) -> &Entity<TextField> {
        &self.field
    }

    /// Why the last Start failed.
    #[cfg(test)]
    pub(super) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Start can be pressed: a description, an agent that is listed and installed, nothing starting.
    pub(super) fn can_start(&self, cx: &App) -> bool {
        let store = self.store.read(cx);
        let installed = store.provider(&store.draft.provider).is_some_and(|provider| provider.installed);
        !self.starting && installed && can_send(&store.composer(HOME))
    }

    fn start(&mut self, cx: &mut Context<Self>) {
        if !self.can_start(cx) {
            return;
        }
        self.starting = true;
        self.error = None;
        cx.emit(NewTaskEvent::Start);
        cx.notify();
    }

    /// The Start ended: `error` says why it failed.
    fn finished(&mut self, error: Option<String>, cx: &mut Context<Self>) {
        self.starting = false;
        self.error = error;
        cx.notify();
    }
}

impl Focusable for NewTaskPage {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.field.focus_handle(cx)
    }
}

impl Render for NewTaskPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let agent = {
            let store = self.store.read(cx);
            let id = &store.draft.provider;
            store.provider(id).map_or_else(|| id.clone(), |provider| provider.name.clone())
        };
        let enabled = self.can_start(cx);
        let label = tx(cx, if self.starting { "Starting…" } else { "Start" });
        let muted = theme::color(cx, "fg-muted");
        let field = div()
            .px(px(8.0))
            .h(theme::length(cx, "h-control"))
            .flex()
            .items_center()
            .border_1()
            .border_color(theme::color(cx, "border"))
            .rounded(theme::length(cx, "radius-control"))
            .bg(theme::color(cx, "bg-field"))
            .child(self.field.clone());
        let chooser = div()
            .id("agents-home-agent")
            .debug_selector(|| "agents-home-agent".to_owned())
            .h(px(24.0))
            .px(px(6.0))
            .flex()
            .items_center()
            .gap(px(4.0))
            .rounded(theme::length(cx, "radius-small"))
            .hover(|el| el.bg(theme::color(cx, "bg-hover")))
            .child(SharedString::from(agent))
            .child(theme::icon(cx, "chevron-down", 12.0, muted))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, event: &MouseDownEvent, _, cx| cx.emit(NewTaskEvent::ChooseAgent(event.position))),
            );
        let start = div()
            .id("agents-home-start")
            .debug_selector(|| "agents-home-start".to_owned())
            .h(px(24.0))
            .px(px(10.0))
            .flex()
            .items_center()
            .rounded(theme::length(cx, "radius-control"))
            .when(enabled, |el| {
                el.bg(theme::color(cx, "accent"))
                    .text_color(theme::color(cx, "fg-on-accent"))
                    .on_click(cx.listener(|page, _, _, cx| page.start(cx)))
            })
            .when(!enabled, |el| el.opacity(0.5))
            .child(SharedString::from(label));
        let error = self.error.clone().map(|error| {
            div()
                .debug_selector(|| "agents-home-error".to_owned())
                .text_color(theme::color(cx, "danger"))
                .child(SharedString::from(error))
        });
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .p(px(8.0))
            .text_size(theme::length(cx, "fs-ui"))
            .text_color(theme::color(cx, "fg"))
            .child(field)
            .child(div().flex().items_center().justify_between().child(chooser).child(start))
            .children(error)
    }
}
