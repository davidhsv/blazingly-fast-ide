//! What the Terminal tool window asks of its window: the window's Options menu and Hide button, a
//! tab's menu and a group's More Terminal Options menu, closing tabs (asking first whether to
//! terminate a command that runs), and the notification of a paste that failed.

use super::{TERMINAL_TOOL_WINDOW, Workbench};
use crate::backend::Backend;
use crate::platform::i18n::Localization;
use crate::terminal::panel::groups::TerminalTabCommand;
use crate::terminal::panel::{TerminalPanel, TerminalPanelEvent};
use crate::terminal::store::{SessionStatus, group_tabs, terminate_confirmation};
use crate::vcs::prompt_dialogs::PromptDialog;
use crate::vcs_ops::dialogs::ConfirmRequest;
use futures::future::BoxFuture;
use gpui_kit::{App, Context, Pixels, Point, Window};

impl Workbench {
    /// Carries out what the Terminal tool window asks for.
    pub(super) fn terminal_panel_event(
        &mut self,
        event: &TerminalPanelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event.clone() {
            // The last tab closed, or the header's Hide button.
            TerminalPanelEvent::Emptied | TerminalPanelEvent::Hide => {
                self.with_layout(window, cx, |layout| layout.hide(TERMINAL_TOOL_WINDOW));
            }
            TerminalPanelEvent::ShowOptions(position) => {
                self.show_tool_window_options(TERMINAL_TOOL_WINDOW, position, window, cx);
            }
            TerminalPanelEvent::TabMenu { tab, position } => self.show_terminal_tab_menu(&tab, position, window, cx),
            TerminalPanelEvent::MoreMenu(position) => {
                let items = TerminalPanel::more_menu();
                self.show_menu(&items, position, Self::run_terminal_tab_command, window, cx);
            }
            TerminalPanelEvent::CloseTabs(tabs) => self.request_close_terminal_tabs(tabs, window, cx),
            TerminalPanelEvent::Notify(request) => self.notify(request, cx),
        }
    }

    /// The menu of the Terminal's tab `tab`, at `position`.
    fn show_terminal_tab_menu(
        &mut self,
        tab: &str,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(panel) = self.terminal_panel() else { return };
        let items = panel.read(cx).tab_menu(tab);
        if !items.is_empty() {
            self.show_menu(&items, position, Self::run_terminal_tab_command, window, cx);
        }
    }

    fn run_terminal_tab_command(&mut self, command: TerminalTabCommand, window: &mut Window, cx: &mut Context<Self>) {
        let tabs = match command {
            TerminalTabCommand::Close(tab) => vec![tab],
            TerminalTabCommand::CloseOthers(tab) => {
                let Some(panel) = self.terminal_panel() else { return };
                let terminals = panel.read(cx).terminals();
                let Some(group) = terminals.tab(&tab).map(|own| own.group.clone()) else { return };
                let others = group_tabs(terminals.tabs(), &group).into_iter().filter(|other| other.id != tab);
                others.map(|other| other.id.clone()).collect()
            }
        };
        self.request_close_terminal_tabs(tabs, window, cx);
    }

    /// Closes the Terminal's tabs `tabs` one after the other, as Close Tab does: a tab whose session
    /// runs a command (a program of its own, or a shell whose foreground process is another one, as
    /// the backend tells) asks Terminate / Cancel first, and Cancel keeps it and the tabs after it. A
    /// session that runs nothing closes without a question. The last tab closing hides the tool
    /// window; otherwise the keyboard goes to the tab that shows.
    pub(super) fn request_close_terminal_tabs(
        &mut self,
        tabs: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.spawn_in(window, async move |this, cx| {
            for tab in tabs {
                let Some((title, busy)) =
                    this.update_in(cx, |workbench, _, cx| workbench.terminal_tab_busy(&tab, cx))?
                else {
                    continue;
                };
                if busy.await {
                    let answer = this.update_in(cx, |workbench, window, cx| {
                        let question = terminate_confirmation(&title, &cx.default_global::<Localization>().0);
                        let request = ConfirmRequest {
                            title: question.title,
                            message: question.message,
                            ok_label: question.ok,
                            danger: false,
                        };
                        workbench.show_prompt_dialog(
                            move |window, cx| PromptDialog::confirm(request, window, cx),
                            window,
                            cx,
                        )
                    })?;
                    // A question closed without an answer (the window went) ends nothing.
                    if !answer.await.is_ok_and(|answer| answer.confirmed()) {
                        break;
                    }
                }
                this.update_in(cx, |workbench, window, cx| workbench.remove_terminal_tab(&tab, window, cx))?;
            }
            anyhow::Ok(())
        })
        .detach();
    }

    /// The label of the Terminal's tab `tab` and whether ending its session would kill something the
    /// user started; `None` once the tab is gone.
    fn terminal_tab_busy(&self, tab: &str, cx: &App) -> Option<(String, BoxFuture<'static, bool>)> {
        let panel = self.terminal_panel()?;
        let panel = panel.read(cx);
        let terminals = panel.terminals();
        let session = terminals.session(&terminals.tab(tab)?.session);
        let title = terminals.title(tab).unwrap_or_default();
        let running = session.is_some_and(|session| session.state.status == SessionStatus::Running);
        let program = session.and_then(|session| session.command.as_ref()).is_some_and(|command| !command.shell);
        let terminal = panel.view_of_tab(tab).and_then(|view| view.read(cx).id()).filter(|_| running);
        let busy: BoxFuture<'static, bool> = match (terminal, cx.try_global::<Backend>()) {
            (Some(_), _) if program => Box::pin(async { true }),
            (Some(id), Some(backend)) => {
                let call = backend.with_host(move |host| async move { ide_host::ops::term::is_busy(&host, id) });
                // A session the backend no longer knows has ended: nothing to ask about.
                Box::pin(async move { matches!(call.await, Ok(Ok(true))) })
            }
            _ => Box::pin(async { false }),
        };
        Some((title, busy))
    }

    /// Closes the Terminal's tab `tab` and ends its session; the keyboard goes to the tab that shows.
    fn remove_terminal_tab(&mut self, tab: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(panel) = self.terminal_panel() else { return };
        let left = panel.update(cx, |panel, cx| {
            panel.close_tab(tab, cx);
            !panel.terminals().tabs().is_empty()
        });
        if left {
            self.with_layout(window, cx, |layout| layout.focus(TERMINAL_TOOL_WINDOW));
        }
    }
}
