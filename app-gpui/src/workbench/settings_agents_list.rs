//! Apply (and OK) of the Settings values kept outside the settings: the agents' executables and
//! environment variables staged on Agents › Coding Agents. Each is written in the order it was staged,
//! as its command writes it (`agent_set_command`, `agent_set_env`: the agent's file of the
//! configuration folder keeps the variables' names, the saved secrets their values), and the agents are
//! read again (`agent_providers`): the page shows them as they read now and the dialog keeps the value
//! as stored. The first write that fails stops the Apply with its reason in the dialog's footer and
//! leaves it and what follows staged, for another Apply. A value no page of this window writes is
//! refused, never reported as saved.
//!
//! The writes run on the backend's runtime, one at a time, and end the Apply when all are done; the
//! dialog may close meanwhile (Cancel), and then nothing more is written.

use super::Workbench;
use super::settings_window::NOT_CARRIED_OUT;
use crate::agents::agent_list::{AgentWrite, agent_write, stored_value};
use crate::backend::Backend;
use crate::platform::i18n::Localization;
use crate::settings_view::dialog::SettingsDialogView;
use crate::settings_view::pages::agents_list::{self, AgentListPageView};
use gpui_kit::{AnyView, AsyncApp, Context, Entity, WeakEntity};
use ide_agent::AgentProvider;
use ide_host::Host;
use serde_json::Value;
use std::sync::Arc;

/// Writes `write` for window `label` as its command does, then reads the window's agents again.
async fn carry_out(host: Arc<Host>, label: String, write: AgentWrite) -> Result<Vec<AgentProvider>, String> {
    use ide_host::ops::agent;
    let written = match write {
        AgentWrite::SetCommand { agent: id, command } => agent::set_command(&host, &label, id, command),
        AgentWrite::SetEnv { agent: id, env } => agent::set_env(&host, &label, id, env),
    };
    written.and_then(|()| agent::providers(&host, &label)).map_err(|error| error.message)
}

/// Writes the staged value `id`; the value as stored, or why it was not.
async fn write_one(
    dialog: &WeakEntity<SettingsDialogView>,
    label: &str,
    id: &str,
    value: &Value,
    cx: &mut AsyncApp,
) -> Result<Value, String> {
    let write = cx.update(|cx| agent_write(id, value, &mut cx.default_global::<Localization>().0));
    let write = match write {
        Some(write) => write?,
        None => {
            tracing::warn!("Settings: nothing in this window saves {id}");
            return Err(NOT_CARRIED_OUT.to_owned());
        }
    };
    let carried = write.clone();
    let label = label.to_owned();
    let work = cx.update(|cx| {
        let backend = cx.try_global::<Backend>()?;
        Some(backend.with_host(move |host| carry_out(host, label, carried)))
    });
    let Some(work) = work else { return Err("The backend is not running".to_owned()) };
    let agents = work.await.unwrap_or_else(|error| Err(error.to_string()))?;
    let stored = stored_value(&write, &agents);
    // The page shows the agents as they read now: the tag of an agent follows what was written.
    let page: Option<AnyView> =
        dialog.read_with(cx, |view, _| view.page_view(agents_list::PAGE).cloned()).ok().flatten();
    if let Some(page) = page
        && let Ok(page) = page.downcast::<AgentListPageView>()
    {
        page.update(cx, |page, cx| page.agents_loaded(Ok(agents), cx));
    }
    Ok(stored)
}

/// Writes every value the Apply hands out, then ends it.
async fn write_all(dialog: WeakEntity<SettingsDialogView>, label: String, cx: &mut AsyncApp) {
    loop {
        let Ok(next) = dialog.update(cx, |view, cx| view.apply_next_external(cx)) else { return };
        let Some((id, value)) = next else { break };
        let result = write_one(&dialog, &label, &id, &value, cx).await;
        if dialog.update(cx, |view, cx| view.apply_external_done(result, cx)).is_err() {
            return;
        }
    }
    let ended = dialog.update(cx, |view, cx| view.finish_apply(cx));
    if ended.is_err() {
        tracing::debug!("Settings closed before its Apply ended");
    }
}

impl Workbench {
    /// Writes the values kept outside the settings that the Apply under way in `dialog` hands out, then
    /// ends that Apply (after OK, a full save closes the dialog). With none to write, or when the
    /// settings writes already failed, the Apply ends at once, in the same turn.
    pub(super) fn write_external_settings(&mut self, dialog: &Entity<SettingsDialogView>, cx: &mut Context<Self>) {
        if dialog.update(cx, |view, cx| view.apply_next_external(cx)).is_none() {
            dialog.update(cx, |view, cx| {
                view.finish_apply(cx);
            });
            return;
        }
        let dialog = dialog.downgrade();
        let label = self.label.clone();
        cx.spawn(async move |_, cx| write_all(dialog, label, cx).await).detach();
    }
}
