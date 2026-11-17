//! The window's side of the New Task page ([`NewTaskView`]): what it is told when it opens (the workspace's
//! members, the agents again, the project's commands for the selected agent), and what it asks for (Start, the
//! agent menu, the effects of a setting chosen or an image refused).
//!
//! A child module of the Agents feature (`workbench::agents`), so it shares its store, its backend runner and
//! its effects.

use super::*;
use crate::agents::model::provider_icon;
use crate::agents::new_task_view::{NewTaskEvent, NewTaskView};
use crate::agents::registry::{ADD_AGENT, provider_options};
use crate::agents::store::{TaskProjects, default_task_project};
use crate::vcs_ops::menu::MenuItem;
use gpui_kit::{Pixels, Point};

impl Workbench {
    /// The New Task page was created: it learns the workspace's members (and which one the task runs in), the
    /// agents are listed again (one installed in Settings shows up), and the selected agent's commands are read.
    pub(super) fn agents_home_opened(&mut self, page: &Entity<NewTaskView>, cx: &mut Context<Self>) {
        let members = self.workspace_members(cx);
        if !members.is_empty() {
            let root = self.project_root(cx).map(|root| root.to_string_lossy().into_owned());
            let active = self.active_file_path(cx);
            let last = self.agents_store_entity().and_then(|store| store.read(cx).draft.project_root.clone());
            let projects = TaskProjects { root: root.as_deref(), members: &members, active_path: active.as_deref() };
            let selected = default_task_project(projects, last.as_deref());
            page.update(cx, |page, cx| page.set_projects(members, selected, cx));
        }
        self.carry_out_agents(vec![AgentEffect::RefreshProviders], cx);
        self.load_home_commands(cx);
    }

    fn agents_store_entity(&self) -> Option<Entity<AgentsStore>> {
        self.agents.as_ref().map(|slot| slot.store.clone())
    }

    /// The member projects of the window's workspace, in manifest order; none outside a workspace.
    fn workspace_members(&self, cx: &App) -> Vec<String> {
        let Some(backend) = cx.try_global::<Backend>() else { return Vec::new() };
        ide_host::ops::project::current(&backend.host, &self.label)
            .and_then(|info| info.workspace)
            .map(|workspace| workspace.projects.iter().map(|path| path.to_string_lossy().into_owned()).collect())
            .unwrap_or_default()
    }

    /// Reads the commands `/` completes on the New Task page for its agent: the project's skills merged with
    /// the agent's commands (`agent_commands` with no task). Until they arrive (or if reading them fails) the
    /// page completes the commands the agent offered last.
    fn load_home_commands(&mut self, cx: &mut Context<Self>) {
        let Some(store) = self.agents_store_entity() else { return };
        let provider = store.read(cx).draft.provider.clone();
        let asked = provider.clone();
        #[cfg(test)]
        let seam = cx.try_global::<spawn_guard::AgentSpawnSeam>().cloned();
        self.agents_backend(
            cx,
            move |host, label| async move {
                // Reading a built-in agent's commands can start its program: tests start only their own agents.
                #[cfg(test)]
                if let Some(refused) = spawn_guard::gate_lookup(&host, &label, &asked, seam.as_ref()) {
                    return Err(refused);
                }
                ide_host::ops::agent::commands(&host, &label, asked, None).await.map_err(|error| error.message)
            },
            move |workbench, answer, cx| match answer {
                Ok(commands) => {
                    let home = workbench.agents.as_ref().and_then(|slot| slot.home.as_ref());
                    if let Some(page) = home.map(|(page, _)| page.clone()) {
                        page.update(cx, |page, cx| page.set_commands(&provider, commands, cx));
                    }
                }
                Err(error) => tracing::warn!("the agent's commands could not be read: {error}"),
            },
        );
    }

    /// Carries out what the New Task page asks for.
    pub(super) fn agents_home_event(
        &mut self,
        page: Entity<NewTaskView>,
        event: &NewTaskEvent,
        cx: &mut Context<Self>,
    ) {
        match event.clone() {
            NewTaskEvent::Start { project_root } => {
                let effect =
                    self.agents_step(cx, |store, _, _| store.start_task(None, project_root.as_deref())).flatten();
                match effect {
                    Some(effect) => self.carry_out_agents(vec![effect], cx),
                    None => page.update(cx, |page, cx| page.finished(None, cx)),
                }
            }
            NewTaskEvent::ChooseAgent(position) => self.choose_home_agent(position, cx),
            NewTaskEvent::Effects(effects) => self.carry_out_agents(effects, cx),
        }
    }

    /// The agent menu of the New Task page, under its agent chooser: the installed agents (a registry agent with
    /// its version, or the update on offer), the others under "Not installed" with the command they need, then
    /// More agents….
    fn choose_home_agent(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(store) = self.agents_store_entity() else { return };
        let providers = store.read(cx).providers.clone();
        if providers.is_empty() {
            return;
        }
        let options = cx.update_default_global::<Localization, _>(|localization, _| {
            provider_options(&providers, &mut localization.0, provider_icon)
        });
        let mut items: Vec<MenuItem<String>> = Vec::with_capacity(options.len() + 1);
        for option in options {
            if option.section {
                items.push(MenuItem::Separator);
                items.push(MenuItem::entry(option.label, None));
                continue;
            }
            let label = match &option.detail {
                Some(detail) => format!("{} · {detail}", option.label),
                None => option.label.clone(),
            };
            if option.value == ADD_AGENT && !items.is_empty() {
                items.push(MenuItem::Separator);
            }
            // An agent that is not installed can still be chosen: the page then says what it needs.
            items.push(MenuItem::item(label, option.value));
        }
        self.in_agents_window(cx, move |workbench, window, cx| {
            let choose = |workbench: &mut Self, id: String, window: &mut Window, cx: &mut Context<Self>| {
                if id == ADD_AGENT {
                    // Once this update is over: the action reaches for the workbench.
                    window.defer(cx, |window, cx| {
                        if !execute_action_from(ActionSource::Other, "agents.addAgent", None, window, cx) {
                            tracing::info!("agents.addAgent is not available in this window");
                        }
                    });
                    return;
                }
                workbench.choose_task_agent(id, cx);
                workbench.load_home_commands(cx);
            };
            workbench.show_menu(&items, position, choose, window, cx);
        });
    }
}
