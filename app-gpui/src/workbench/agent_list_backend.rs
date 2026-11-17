//! What Settings › Agents › Coding Agents reaches the backend through, for one window: the agents
//! (`agent_providers`), custom agents (`agent_custom_save`, `agent_custom_remove`, `fs_read_text` then
//! `agent_custom_import`, the system's file chooser) and the ACP Registry (`agent_registry_list`,
//! `agent_registry_install`, `agent_registry_remove`, `cancel_request`, the browser for an agent's web
//! page). Each runs as its command runs it, on the backend's runtime, and answers on the window's
//! thread.
//!
//! Listing the registry downloads it. A test build never reaches it: the registry's calls there are
//! answered by the [`RegistryStandIn`] a test sets, and without one they fail with [`REFUSED`] and
//! reach nothing, not even the backend's function. No test build opens a browser either.

use crate::agents::registry::Failure;
#[cfg(not(test))]
use crate::backend::Backend;
use crate::settings_view::pages::Then;
use crate::settings_view::pages::agents_list::{AgentListBackend, CustomCalls, Lister, RegistryCalls};
use crate::settings_view::pages::ssh_backend::on_backend;
use gpui_kit::{App, PathPromptOptions};
use ide_agent::registry::AgentRegistry;
use ide_agent::{AcpImportOutcome, AgentProvider, CustomAgentInput};
use ide_core::IdeError;
use ide_core::RequestId;
#[cfg(not(test))]
use ide_core::error::ErrorKind;
use ide_tools::InstallEvent;
use std::path::PathBuf;
use std::rc::Rc;

#[cfg(test)]
pub(crate) use stand_in::{REFUSED, RegistryStandIn, calls as stand_in_calls};

/// Everything the page of window `window_label` reaches the backend through.
pub fn for_window(window_label: &str) -> AgentListBackend {
    let label: Rc<str> = Rc::from(window_label);
    AgentListBackend {
        providers: providers(label.clone()),
        registry: Some(registry_calls(label.clone())),
        custom: Some(custom_calls(label)),
    }
}

#[cfg(not(test))]
fn failure(error: IdeError) -> Failure {
    if error.kind == ErrorKind::Cancelled { Failure::Cancelled } else { Failure::Error(error.message) }
}

/// `then` for an answer that reports its error as text.
fn as_text<T: 'static>(then: Then<Result<T, String>>) -> Then<Result<T, IdeError>> {
    Rc::new(move |result: Result<T, IdeError>, cx: &mut App| then(result.map_err(|error| error.message), cx))
}

/// `agent_providers`: the agent files of the configuration folder (their environment values filled in
/// from the saved secrets), whether each command is an executable on `PATH`, and the update marks of
/// the registry's saved index. It starts no program and reaches no network.
fn providers(label: Rc<str>) -> Lister {
    Rc::new(move |then: Then<Result<Vec<AgentProvider>, String>>, cx: &mut App| {
        let label = label.to_string();
        on_backend(move |host| async move { ide_host::ops::agent::providers(&host, &label) }, as_text(then), cx);
    })
}

fn custom_calls(label: Rc<str>) -> CustomCalls {
    let (save_label, remove_label, import_label) = (label.clone(), label.clone(), label);
    CustomCalls {
        save: Rc::new(move |input: CustomAgentInput, then: Then<Result<(), String>>, cx: &mut App| {
            let label = save_label.to_string();
            on_backend(
                move |host| async move { ide_host::ops::agent::save_custom_agent(&host, &label, input) },
                as_text(then),
                cx,
            );
        }),
        remove: Rc::new(move |id: String, then: Then<Result<(), String>>, cx: &mut App| {
            let label = remove_label.to_string();
            on_backend(
                move |host| async move { ide_host::ops::agent::remove_custom_agent(&host, &label, id) },
                as_text(then),
                cx,
            );
        }),
        import: Rc::new(move |path: PathBuf, then: Then<Result<Vec<AcpImportOutcome>, String>>, cx: &mut App| {
            let label = import_label.to_string();
            on_backend(
                move |host| async move {
                    let text = ide_host::ops::fs::read_text(&host, &label, path).await?;
                    ide_host::ops::agent::import_acp_agents(&host, &label, text)
                },
                as_text(then),
                cx,
            );
        }),
        pick_file: Rc::new(|then: Then<Option<PathBuf>>, cx: &mut App| {
            let options = PathPromptOptions { files: true, directories: false, multiple: false, prompt: None };
            let picked = cx.prompt_for_paths(options);
            cx.spawn(async move |cx| {
                let path = match picked.await {
                    Ok(Ok(Some(paths))) => paths.into_iter().next(),
                    Ok(Ok(None)) | Err(_) => None,
                    Ok(Err(error)) => {
                        tracing::warn!("Settings: the file chooser failed: {error}");
                        None
                    }
                };
                cx.update(|cx| then(path, cx));
            })
            .detach();
        }),
    }
}

/// The registry's calls of the backend, for the app.
#[cfg(not(test))]
fn registry_calls(label: Rc<str>) -> RegistryCalls {
    use futures::StreamExt as _;
    let (list_label, install_label, remove_label) = (label.clone(), label.clone(), label);
    RegistryCalls {
        list: Rc::new(move |request: RequestId, then: Then<Result<AgentRegistry, Failure>>, cx: &mut App| {
            let label = list_label.to_string();
            let then: Then<Result<AgentRegistry, IdeError>> =
                Rc::new(move |result: Result<AgentRegistry, IdeError>, cx: &mut App| then(result.map_err(failure), cx));
            on_backend(
                move |host| async move { ide_host::ops::agent::registry_list(&host, &label, request).await },
                then,
                cx,
            );
        }),
        install: Rc::new(
            move |id: String,
                  request: RequestId,
                  on_event: Then<InstallEvent>,
                  done: Then<Result<(), Failure>>,
                  cx: &mut App| {
                let Some(backend) = cx.try_global::<Backend>() else {
                    done(Err(Failure::Error("The backend is not running".to_owned())), cx);
                    return;
                };
                // Progress arrives on the backend's threads; the page takes it on the window's.
                let (sender, mut events) = futures::channel::mpsc::unbounded();
                let label = install_label.to_string();
                let work = backend.with_host(move |host| async move {
                    let channel = ide_host::ops::Channel::new(move |event| {
                        let _ = sender.unbounded_send(event);
                    });
                    ide_host::ops::agent::registry_install(&host, &label, id, request, channel).await
                });
                cx.spawn(async move |cx| {
                    // The stream ends when the install drops its channel, that is once it has ended.
                    while let Some(event) = events.next().await {
                        cx.update(|cx| on_event(event, cx));
                    }
                    let result = match work.await {
                        Ok(result) => result.map_err(failure),
                        Err(error) => Err(Failure::Error(error.to_string())),
                    };
                    cx.update(|cx| done(result, cx));
                })
                .detach();
            },
        ),
        remove: Rc::new(move |id: String, then: Then<Result<(), String>>, cx: &mut App| {
            let label = remove_label.to_string();
            on_backend(
                move |host| async move { ide_host::ops::agent::registry_remove(&host, &label, id).await },
                as_text(then),
                cx,
            );
        }),
        cancel: Rc::new(|request: RequestId, cx: &mut App| {
            if let Some(backend) = cx.try_global::<Backend>() {
                backend.host.cancel_request(request);
            }
        }),
        open_url: Rc::new(|url: String, cx: &mut App| cx.open_url(&url)),
    }
}

/// The registry's calls in a test build: the test's stand-in answers, or the call is refused.
#[cfg(test)]
fn registry_calls(_label: Rc<str>) -> RegistryCalls {
    stand_in::calls()
}

#[cfg(test)]
mod stand_in {
    //! The ACP Registry of a test: a fixed list, installs the test ends with the progress it reports,
    //! and the removals, cancellations and web pages asked for, recorded.

    use super::*;
    use crate::agents::registry::{after_install, after_remove};
    use ide_agent::registry::RegistryAgent;
    use std::cell::RefCell;

    /// What a registry call without a stand-in answers.
    pub(crate) const REFUSED: &str = "refused to reach the ACP Registry: a test sets a RegistryStandIn";

    fn refused(call: &str) -> String {
        let message = format!("{REFUSED} ({call})");
        tracing::error!("{message}");
        message
    }

    /// An install the stand-in runs until the test ends it.
    struct Running {
        id: String,
        request: RequestId,
        on_event: Then<InstallEvent>,
        done: Then<Result<(), Failure>>,
    }

    pub(crate) struct State {
        /// What a listing answers.
        pub(crate) registry: Result<AgentRegistry, String>,
        pub(crate) listings: usize,
        pub(crate) installs: Vec<String>,
        pub(crate) removed: Vec<String>,
        pub(crate) cancelled: Vec<RequestId>,
        pub(crate) opened: Vec<String>,
        /// The next removal fails with this.
        pub(crate) remove_error: Option<String>,
        running: Vec<Running>,
    }

    /// Set as a global by a test, it answers the page's registry calls.
    #[derive(Clone)]
    pub(crate) struct RegistryStandIn(pub(crate) Rc<RefCell<State>>);

    impl gpui_kit::Global for RegistryStandIn {}

    impl RegistryStandIn {
        /// A registry listing `agents`, set for the app of `cx`.
        pub(crate) fn set(agents: Vec<RegistryAgent>, cx: &mut App) -> Self {
            let state = State {
                registry: Ok(AgentRegistry { agents, warning: None }),
                listings: 0,
                installs: Vec::new(),
                removed: Vec::new(),
                cancelled: Vec::new(),
                opened: Vec::new(),
                remove_error: None,
                running: Vec::new(),
            };
            let stand_in = Self(Rc::new(RefCell::new(state)));
            cx.set_global(stand_in.clone());
            stand_in
        }

        /// The last install started reports `event`.
        pub(crate) fn emit(&self, event: InstallEvent, cx: &mut App) {
            let last = self.0.borrow().running.last().map(|run| run.on_event.clone());
            if let Some(on_event) = last {
                on_event(event, cx);
            }
        }

        /// The last install started ends: on success the registry lists the agent as installed.
        pub(crate) fn finish(&self, result: Result<(), Failure>, cx: &mut App) {
            let Some(run) = self.0.borrow_mut().running.pop() else { return };
            if result.is_ok()
                && let Ok(registry) = &mut self.0.borrow_mut().registry
            {
                after_install(&mut registry.agents, &run.id);
            }
            (run.done)(result, cx);
        }

        fn list(&self, then: Then<Result<AgentRegistry, Failure>>, cx: &mut App) {
            let answer = {
                let mut state = self.0.borrow_mut();
                state.listings += 1;
                state.registry.clone().map_err(Failure::Error)
            };
            then(answer, cx);
        }

        fn install(
            &self,
            id: String,
            request: RequestId,
            on_event: Then<InstallEvent>,
            done: Then<Result<(), Failure>>,
            _: &mut App,
        ) {
            let mut state = self.0.borrow_mut();
            state.installs.push(id.clone());
            state.running.push(Running { id, request, on_event, done });
        }

        fn remove(&self, id: String, then: Then<Result<(), String>>, cx: &mut App) {
            let answer = {
                let mut state = self.0.borrow_mut();
                match state.remove_error.take() {
                    Some(error) => Err(error),
                    None => {
                        if let Ok(registry) = &mut state.registry {
                            after_remove(&mut registry.agents, &id);
                        }
                        state.removed.push(id);
                        Ok(())
                    }
                }
            };
            then(answer, cx);
        }

        /// A cancelled install ends as the backend ends it: cancelled.
        fn cancel(&self, request: RequestId, cx: &mut App) {
            let run = {
                let mut state = self.0.borrow_mut();
                state.cancelled.push(request);
                let at = state.running.iter().position(|run| run.request == request);
                at.map(|at| state.running.remove(at))
            };
            if let Some(run) = run {
                (run.done)(Err(Failure::Cancelled), cx);
            }
        }
    }

    fn stand_in(cx: &App) -> Option<RegistryStandIn> {
        cx.try_global::<RegistryStandIn>().cloned()
    }

    /// The registry's calls a test build makes: the stand-in set as a global answers, else they fail.
    pub(crate) fn calls() -> RegistryCalls {
        RegistryCalls {
            list: Rc::new(|_: RequestId, then: Then<Result<AgentRegistry, Failure>>, cx: &mut App| {
                match stand_in(cx) {
                    Some(stand_in) => stand_in.list(then, cx),
                    None => then(Err(Failure::Error(refused("agent_registry_list"))), cx),
                }
            }),
            install: Rc::new(
                |id: String,
                 request: RequestId,
                 on_event: Then<InstallEvent>,
                 done: Then<Result<(), Failure>>,
                 cx: &mut App| match stand_in(cx) {
                    Some(stand_in) => stand_in.install(id, request, on_event, done, cx),
                    None => done(Err(Failure::Error(refused("agent_registry_install"))), cx),
                },
            ),
            remove: Rc::new(|id: String, then: Then<Result<(), String>>, cx: &mut App| match stand_in(cx) {
                Some(stand_in) => stand_in.remove(id, then, cx),
                None => then(Err(refused("agent_registry_remove")), cx),
            }),
            cancel: Rc::new(|request: RequestId, cx: &mut App| match stand_in(cx) {
                Some(stand_in) => stand_in.cancel(request, cx),
                None => {
                    refused("cancel_request");
                }
            }),
            open_url: Rc::new(|url: String, cx: &mut App| match stand_in(cx) {
                Some(stand_in) => stand_in.0.borrow_mut().opened.push(url),
                None => {
                    refused("open_url");
                }
            }),
        }
    }
}
