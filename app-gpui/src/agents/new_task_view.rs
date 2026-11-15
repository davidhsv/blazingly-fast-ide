//! The New Task page docked in the Agents tool window: where the task runs (the workspace member, chosen from
//! a list when there are several), one box with the task's description (the store's [`HOME`] composer), its
//! pasted images, the agent, the agent's settings as chips with their pickers, and Start.
//!
//! The description tags its `@file` references and `/command`s where they stand. Typing `/` at a word's start
//! opens the commands the project has for the agent (commands before skills, under their headings): ↑/↓ move
//! over them, wrapping, Enter or Tab completes the highlighted one at the caret, Escape closes the list. Pasting
//! an image (the clipboard holding an image and no text) attaches it. Start needs a description (or an image)
//! and an installed agent; Enter starts too.
//!
//! [`NewTaskView`] owns no backend: the window starts the task, shows the agents to choose from, reads the
//! project's commands ([`NewTaskView::set_commands`]) and members ([`NewTaskView::set_projects`]) and carries
//! out the effects it reports ([`NewTaskEvent`]).

use crate::agents::composer::{
    Completion, CompletionKind, MAX_IMAGE_BYTES, TokenKind, apply_completion, can_send, completion_at, match_commands,
    message_tokens,
};
use crate::agents::config_pickers_view::{self as pickers, PickerHost, Pickers};
use crate::agents::store::{AgentEffect, AgentsStore, HOME};
use crate::find::utf16;
use crate::platform::i18n::{I18n, Localization};
use crate::platform::input;
use crate::theme;
use crate::ui::popup_anchor;
use crate::ui::text_field::{TextField, TextFieldEvent, TextTag};
use crate::ui::tooltip::tooltip;
use crate::workbench::notifications::{NotificationLevel, NotificationRequest};
use gpui_kit::{
    AnyElement, App, Bounds, ClipboardEntry, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement,
    Keystroke, MouseButton, MouseDownEvent, Pixels, Point, Render, ScrollHandle, SharedString, Subscription, Window,
    canvas, deferred, div, prelude::*, px, relative,
};
use ide_agent::{AgentCommand, AgentCommandSource, AgentConfigOption, AgentImage};
use std::cell::Cell;
use std::path::Path;
use std::rc::Rc;

/// UI text in the active language.
fn tx(cx: &App, text: &str) -> String {
    cx.try_global::<Localization>().map_or_else(|| text.to_owned(), |localization| localization.0.tx(text))
}

/// UI text built at run time, in the active language.
fn tx_template(cx: &App, text: &str) -> String {
    cx.try_global::<Localization>().map_or_else(|| text.to_owned(), |localization| localization.0.tx_template(text))
}

/// `bytes` in standard base64 with padding, as an image attachment carries its data.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let at = |i: usize| u32::from(chunk.get(i).copied().unwrap_or(0));
        let n = (at(0) << 16) | (at(1) << 8) | at(2);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The last part of a member's path, as the project list names it.
fn basename(path: &str) -> String {
    Path::new(path).file_name().map_or_else(|| path.to_owned(), |name| name.to_string_lossy().into_owned())
}

/// What the New Task page asks of its window.
#[derive(Clone, Debug)]
pub enum NewTaskEvent {
    /// Start the task with the page's description and settings ([`AgentsStore::start_task`]), in the workspace
    /// member `project_root` (`None` outside a workspace).
    Start { project_root: Option<String> },
    /// Show the agents to choose from at `position`, under the agent chooser.
    ChooseAgent(Point<Pixels>),
    /// Carry out these effects: a setting saved, a warning shown.
    Effects(Vec<AgentEffect>),
}

/// The commands `/` completes for one agent, as the project has them (its skills merged with the agent's
/// commands).
struct LoadedCommands {
    provider: String,
    commands: Vec<AgentCommand>,
}

/// A row of the command list.
enum CommandRow {
    Heading(AgentCommandSource),
    /// The command's index among the matches, and the command.
    Command(usize, AgentCommand),
}

pub struct NewTaskView {
    store: Entity<AgentsStore>,
    field: Entity<TextField>,
    starting: bool,
    error: Option<String>,
    pickers: Pickers,
    commands: Option<LoadedCommands>,
    /// The highlighted command, for the completion it was moved in; another completion starts at the first.
    command_active: (Option<Completion>, usize),
    command_scroll: ScrollHandle,
    /// The text the command list was closed for with Escape: it stays closed until the text changes.
    dismissed: Option<String>,
    /// The workspace's member projects (none outside a workspace), and the one the task runs in.
    projects: Vec<String>,
    project: Option<String>,
    projects_open: bool,
    /// Where the agent chooser was last drawn: its menu opens under it.
    agent_button: Rc<Cell<Option<Bounds<Pixels>>>>,
    claim: u64,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<NewTaskEvent> for NewTaskView {}

impl PickerHost for NewTaskView {
    fn pickers(&self) -> &Pickers {
        &self.pickers
    }

    fn pickers_mut(&mut self) -> &mut Pickers {
        &mut self.pickers
    }

    fn picker_config(&self, cx: &App) -> Vec<AgentConfigOption> {
        self.store.read(cx).draft_config()
    }

    fn picker_choose(&mut self, option: String, value: String, cx: &mut Context<Self>) {
        let effect = self.store.update(cx, |store, cx| {
            cx.notify();
            store.set_draft_option(&option, &value)
        });
        cx.emit(NewTaskEvent::Effects(vec![effect]));
    }

    fn picker_return_focus(&self, cx: &App) -> FocusHandle {
        self.field.read(cx).focus_handle(cx)
    }

    fn picker_opens_down(&self) -> bool {
        true
    }
}

impl NewTaskView {
    pub fn new(store: Entity<AgentsStore>, cx: &mut Context<Self>) -> Self {
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
                pickers::refresh(page, cx);
                cx.notify();
            }),
            cx.subscribe(&field, |page, field, event: &TextFieldEvent, cx| match event {
                TextFieldEvent::Changed => {
                    let text = field.read(cx).text().to_owned();
                    page.store.update(cx, |store, cx| {
                        store.update_composer(HOME, |draft| draft.text = text);
                        cx.notify();
                    });
                    page.update_tags(cx);
                }
                TextFieldEvent::Submitted => page.start(cx),
                TextFieldEvent::Cancelled => {}
            }),
        ];
        let weak = cx.weak_entity();
        let claim = input::claim_keys(
            field.read(cx).focus_handle(cx),
            move |keystroke, window, cx| {
                weak.update(cx, |page, cx| page.field_key(keystroke, window, cx)).unwrap_or(false)
            },
            cx,
        );
        cx.on_release(|page: &mut Self, cx| {
            input::release_keys(page.claim, cx);
            page.pickers.release(cx);
        })
        .detach();
        let mut page = Self {
            store,
            field,
            starting: false,
            error: None,
            pickers: Pickers::new(cx),
            commands: None,
            command_active: (None, 0),
            command_scroll: ScrollHandle::new(),
            dismissed: None,
            projects: Vec::new(),
            project: None,
            projects_open: false,
            agent_button: Rc::default(),
            claim,
            _subscriptions: subscriptions,
        };
        page.update_tags(cx);
        page
    }

    /// The store's description changed elsewhere (a prompt the action passed, a reference sent): the
    /// field shows it.
    fn sync_field(&mut self, cx: &mut Context<Self>) {
        let text = self.store.read(cx).composer(HOME).text;
        if self.field.read(cx).text() != text {
            self.field.update(cx, |field, cx| field.set_text(&text, cx));
            self.update_tags(cx);
        }
    }

    /// The field draws its `@file` references and `/command`s as tags, where they stand.
    fn update_tags(&mut self, cx: &mut Context<Self>) {
        let text = self.field.read(cx).text().to_owned();
        let (link, accent, background) =
            (theme::color(cx, "link"), theme::color(cx, "accent"), theme::color(cx, "bg-hover"));
        let mut tags = Vec::new();
        let mut at = 0;
        for token in message_tokens(&text) {
            let end = at + token.value.len();
            let color = match token.kind {
                TokenKind::Text => None,
                TokenKind::File => Some(link),
                TokenKind::Command => Some(accent),
            };
            if let Some(color) = color {
                tags.push(TextTag { range: at..end, color, background: Some(background) });
            }
            at = end;
        }
        self.field.update(cx, |field, cx| field.set_tags(tags, cx));
    }

    #[cfg(test)]
    pub fn field(&self) -> &Entity<TextField> {
        &self.field
    }

    /// Why the last Start failed.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// The commands of the selected agent, for its `/` completion, as the project has them.
    pub fn set_commands(&mut self, provider: &str, commands: Vec<AgentCommand>, cx: &mut Context<Self>) {
        self.commands = Some(LoadedCommands { provider: provider.to_owned(), commands });
        cx.notify();
    }

    /// The workspace's member projects (none outside a workspace) and the one the task runs in by default.
    pub fn set_projects(&mut self, members: Vec<String>, selected: Option<String>, cx: &mut Context<Self>) {
        self.project = selected.filter(|root| members.contains(root)).or_else(|| members.first().cloned());
        self.projects = members;
        cx.notify();
    }

    /// The commands `/` completes: the project's for the selected agent once read, else the agent's own list.
    fn commands(&self, cx: &App) -> Vec<AgentCommand> {
        let store = self.store.read(cx);
        let provider = &store.draft.provider;
        match &self.commands {
            Some(loaded) if loaded.provider == *provider => loaded.commands.clone(),
            _ => store.provider(provider).map(|p| p.commands.clone()).unwrap_or_default(),
        }
    }

    /// The `/command` being typed at the caret, unless its list was closed for this text.
    fn completion(&self, cx: &App) -> Option<Completion> {
        let field = self.field.read(cx);
        let text = field.text();
        if self.dismissed.as_deref() == Some(text) {
            return None;
        }
        let caret = field.selection().end.min(text.len());
        let caret = utf16::len(text.get(..caret).unwrap_or(text));
        completion_at(text, caret).filter(|c| c.kind == CompletionKind::Command)
    }

    /// The completion being typed and the commands it matches, while there are some.
    fn command_matches(&self, cx: &App) -> Option<(Completion, Vec<AgentCommand>)> {
        let completion = self.completion(cx)?;
        let commands = self.commands(cx);
        let matches: Vec<AgentCommand> = match_commands(&commands, &completion.query).into_iter().cloned().collect();
        (!matches.is_empty()).then_some((completion, matches))
    }

    fn active_command(&self, completion: &Completion, count: usize) -> usize {
        match &self.command_active {
            (Some(moved), index) if moved == completion && *index < count => *index,
            _ => 0,
        }
    }

    /// The command list as drawn: its headings, and each command as `/<name>` with ` · <description>`, ` *` on
    /// the highlighted one; `None` while it is closed.
    pub fn command_rows(&self, cx: &App) -> Option<Vec<String>> {
        let (completion, commands) = self.command_matches(cx)?;
        let active = self.active_command(&completion, commands.len());
        let rows = command_rows(commands);
        Some(
            rows.into_iter()
                .map(|row| match row {
                    CommandRow::Heading(source) => tx(cx, heading(source)),
                    CommandRow::Command(index, command) => {
                        let mut text = format!("/{}", command.name);
                        if !command.description.is_empty() {
                            text.push_str(&format!(" · {}", command.description));
                        }
                        if index == active {
                            text.push_str(" *");
                        }
                        text
                    }
                })
                .collect(),
        )
    }

    /// The tagged parts of the description, as the field draws them.
    pub fn tags(&self, cx: &App) -> Vec<String> {
        let field = self.field.read(cx);
        field.tags().iter().filter_map(|tag| field.text().get(tag.range.clone()).map(str::to_owned)).collect()
    }

    /// The names of the images attached to the description.
    pub fn images(&self, cx: &App) -> Vec<String> {
        let draft = self.store.read(cx).composer(HOME);
        draft.images.iter().map(|image| image.name.clone().unwrap_or_else(|| tx(cx, "Image"))).collect()
    }

    /// The project the task runs in, as the selector names it; `None` outside a workspace.
    pub fn project_label(&self) -> Option<String> {
        self.project.as_deref().map(basename)
    }

    /// The open list of members, as `<name> · <path>` with ` *` on the selected one.
    pub fn project_rows(&self) -> Option<Vec<String>> {
        self.projects_open.then(|| {
            self.projects
                .iter()
                .map(|path| {
                    let mark = if self.project.as_deref() == Some(path.as_str()) { " *" } else { "" };
                    format!("{} · {path}{mark}", basename(path))
                })
                .collect()
        })
    }

    /// The settings chips (see [`pickers::chip_texts`]).
    pub fn chips(&self, cx: &App) -> Vec<String> {
        let english = I18n::default();
        let i18n = cx.try_global::<Localization>().map_or(&english, |localization| &localization.0);
        pickers::chip_texts(&self.picker_config(cx), i18n)
    }

    /// The entries of the open settings picker, as `<label>` with ` *` on the highlighted one.
    pub fn picker_entries(&self) -> Option<Vec<String>> {
        self.pickers.entries()
    }

    /// The Start button's name: Start, or Starting… while the task starts.
    pub fn start_label(&self, cx: &App) -> String {
        tx(cx, if self.starting { "Starting…" } else { "Start" })
    }

    /// Start can be pressed: a description, an agent that is listed and installed, nothing starting.
    pub fn can_start(&self, cx: &App) -> bool {
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
        let project_root = self.project.clone().filter(|_| !self.projects.is_empty());
        cx.emit(NewTaskEvent::Start { project_root });
        cx.notify();
    }

    /// The Start ended: `error` says why it failed.
    pub fn finished(&mut self, error: Option<String>, cx: &mut Context<Self>) {
        self.starting = false;
        self.error = error;
        cx.notify();
    }

    // ----- what keys and clicks do -----

    /// The keys of the description, ahead of the field: the command list's keys while it is open, and a paste
    /// of an image.
    fn field_key(&mut self, keystroke: &Keystroke, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let modifiers = keystroke.modifiers;
        if keystroke.key == "v" && modifiers.secondary(cx) && !modifiers.shift && !modifiers.alt {
            return self.paste_image(cx);
        }
        if keystroke.key == "escape" && self.pickers.is_open() {
            pickers::close_popups(self, window, cx);
            return true;
        }
        if modifiers.modified() {
            return false;
        }
        let Some((completion, commands)) = self.command_matches(cx) else { return false };
        let active = self.active_command(&completion, commands.len());
        let count = commands.len();
        match keystroke.key.as_str() {
            "down" | "up" => {
                let next = if keystroke.key == "down" { (active + 1) % count } else { (active + count - 1) % count };
                self.command_active = (Some(completion), next);
                self.scroll_to_command(&commands, next);
            }
            "enter" | "tab" => self.complete(&completion, &commands[active], cx),
            "escape" => self.dismissed = Some(self.field.read(cx).text().to_owned()),
            _ => return false,
        }
        cx.notify();
        true
    }

    /// Keeps the highlighted command in view: its row counts the headings above it.
    fn scroll_to_command(&self, commands: &[AgentCommand], index: usize) {
        let row = command_rows(commands.to_vec())
            .iter()
            .position(|row| matches!(row, CommandRow::Command(i, _) if *i == index))
            .unwrap_or(0);
        self.command_scroll.scroll_to_item(row);
    }

    /// Completes `completion` with `command` at the caret, keeping the rest of the description.
    fn complete(&mut self, completion: &Completion, command: &AgentCommand, cx: &mut Context<Self>) {
        let text = self.field.read(cx).text().to_owned();
        let completed = apply_completion(&text, completion, &format!("/{}", command.name));
        let caret = utf16::slice(&completed.text, 0, completed.caret).len();
        self.field.update(cx, |field, cx| {
            field.set_text(&completed.text, cx);
            field.select_range(caret..caret, cx);
        });
        let stored = completed.text.clone();
        self.store.update(cx, |store, cx| {
            store.update_composer(HOME, |draft| draft.text = stored);
            cx.notify();
        });
        self.command_active = (None, 0);
        self.update_tags(cx);
    }

    fn command_clicked(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((completion, commands)) = self.command_matches(cx)
            && let Some(command) = commands.get(index)
        {
            self.complete(&completion, command, cx);
        }
        let focus = self.field.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
        cx.notify();
    }

    /// A paste whose clipboard holds an image and no text attaches the image; anything else is the field's
    /// paste. Returns whether the image was taken.
    fn paste_image(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(item) = cx.read_from_clipboard() else { return false };
        if item.text().is_some() {
            return false;
        }
        let image = item.entries().iter().find_map(|entry| match entry {
            ClipboardEntry::Image(image) => Some((image.format.mime_type(), image.bytes.clone())),
            _ => None,
        });
        let Some((mime, bytes)) = image else { return false };
        if bytes.len() > MAX_IMAGE_BYTES {
            let mut request = NotificationRequest::new(NotificationLevel::Warning, tx(cx, "Image too large"));
            request.body = Some(tx_template(cx, "image is over 5 MB."));
            cx.emit(NewTaskEvent::Effects(vec![AgentEffect::Notify(Box::new(request))]));
            return true;
        }
        let image = AgentImage { mime_type: mime.to_owned(), data: base64(&bytes), name: Some("image".to_owned()) };
        self.store.update(cx, |store, cx| {
            store.update_composer(HOME, |draft| draft.images.push(image));
            cx.notify();
        });
        true
    }

    fn remove_image(&mut self, index: usize, cx: &mut Context<Self>) {
        self.store.update(cx, |store, cx| {
            store.update_composer(HOME, |draft| {
                if index < draft.images.len() {
                    draft.images.remove(index);
                }
            });
            cx.notify();
        });
    }

    fn choose_project(&mut self, index: usize, cx: &mut Context<Self>) {
        self.projects_open = false;
        let Some(path) = self.projects.get(index).cloned() else { return };
        if self.project.as_deref() != Some(path.as_str()) {
            self.project = Some(path);
            let effect = self.store.update(cx, |store, cx| {
                cx.notify();
                store.update_draft(|draft| draft.base = None)
            });
            cx.emit(NewTaskEvent::Effects(vec![effect]));
        }
        cx.notify();
    }

    // ----- drawing -----

    fn command_list(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (completion, commands) = self.command_matches(cx)?;
        let active = self.active_command(&completion, commands.len());
        let (muted, faint) = (theme::color(cx, "fg-muted"), theme::color(cx, "fg-faint"));
        let mut rows: Vec<AnyElement> = Vec::new();
        for row in command_rows(commands) {
            match row {
                CommandRow::Heading(source) => {
                    let title = SharedString::from(tx(cx, heading(source)));
                    rows.push(div().px(px(8.0)).pt(px(4.0)).text_color(muted).child(title).into_any_element());
                }
                CommandRow::Command(index, command) => {
                    let selector = format!("agents-home-command-{index}");
                    let id = selector.clone();
                    let description = (!command.description.is_empty()).then(|| {
                        let text = SharedString::from(command.description.clone());
                        div().flex_1().min_w_0().truncate().text_color(faint).child(text)
                    });
                    rows.push(
                        div()
                            .id(SharedString::from(id))
                            .debug_selector(move || selector)
                            .flex_none()
                            .h(theme::length(cx, "h-row"))
                            .px(px(8.0))
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .rounded(theme::length(cx, "radius-small"))
                            .when(index == active, |el| el.bg(theme::color(cx, "bg-list-selected")))
                            .hover(|el| el.bg(theme::color(cx, "bg-hover")))
                            .child(div().flex_none().child(SharedString::from(format!("/{}", command.name))))
                            .children(description)
                            .on_click(cx.listener(move |page, _, window, cx| page.command_clicked(index, window, cx)))
                            .into_any_element(),
                    );
                }
            }
        }
        let list = div()
            .id("agents-home-commands")
            .debug_selector(|| "agents-home-commands".to_owned())
            .track_scroll(&self.command_scroll)
            .overflow_y_scroll()
            .absolute()
            .top(relative(1.0))
            .left(px(0.0))
            .w(px(360.0))
            .max_h(px(280.0))
            .p(px(4.0))
            .flex()
            .flex_col()
            .rounded(theme::length(cx, "radius-popup"))
            .border_1()
            .border_color(theme::color(cx, "border"))
            .bg(theme::color(cx, "bg-popup"))
            .shadow(theme::shadow(cx, "shadow-popup"))
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .children(rows);
        Some(deferred(list).with_priority(1).into_any_element())
    }

    fn project_selector(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let label = self.project_label()?;
        let muted = theme::color(cx, "fg-muted");
        if self.projects.len() < 2 {
            let title = self.project.clone().unwrap_or_default();
            let name = div().id("agents-home-project").text_color(muted).child(SharedString::from(label));
            return Some(name.tooltip(tooltip(title)).into_any_element());
        }
        let button = div()
            .id("agents-home-project")
            .debug_selector(|| "agents-home-project".to_owned())
            .h(px(22.0))
            .px(px(6.0))
            .flex()
            .items_center()
            .gap(px(4.0))
            .rounded(theme::length(cx, "radius-small"))
            .text_color(muted)
            .hover(|el| el.bg(theme::color(cx, "bg-hover")))
            .child(SharedString::from(label))
            .child(theme::icon(cx, "chevron-down", 12.0, muted))
            .on_click(cx.listener(|page, _, _, cx| {
                page.projects_open = !page.projects_open;
                cx.notify();
            }));
        let list = self.projects_open.then(|| {
            let faint = theme::color(cx, "fg-faint");
            let rows: Vec<AnyElement> = self
                .projects
                .iter()
                .enumerate()
                .map(|(index, path)| {
                    let selector = format!("agents-home-project-{index}");
                    let id = selector.clone();
                    let selected = self.project.as_deref() == Some(path.as_str());
                    div()
                        .id(SharedString::from(id))
                        .debug_selector(move || selector)
                        .h(theme::length(cx, "h-row"))
                        .px(px(8.0))
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .rounded(theme::length(cx, "radius-small"))
                        .when(selected, |el| el.bg(theme::color(cx, "bg-list-selected")))
                        .hover(|el| el.bg(theme::color(cx, "bg-hover")))
                        .child(SharedString::from(basename(path)))
                        .child(div().text_color(faint).truncate().child(SharedString::from(path.clone())))
                        .on_click(cx.listener(move |page, _, _, cx| page.choose_project(index, cx)))
                        .into_any_element()
                })
                .collect();
            let popup = div()
                .debug_selector(|| "agents-home-projects".to_owned())
                .absolute()
                .top(relative(1.0))
                .left(px(0.0))
                .w(px(320.0))
                .p(px(4.0))
                .flex()
                .flex_col()
                .rounded(theme::length(cx, "radius-popup"))
                .border_1()
                .border_color(theme::color(cx, "border"))
                .bg(theme::color(cx, "bg-popup"))
                .shadow(theme::shadow(cx, "shadow-popup"))
                .occlude()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .children(rows);
            deferred(popup).with_priority(1)
        });
        Some(div().relative().child(button).children(list).into_any_element())
    }

    fn images_row(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let names = self.images(cx);
        if names.is_empty() {
            return None;
        }
        let muted = theme::color(cx, "fg-muted");
        let chips: Vec<AnyElement> = names
            .into_iter()
            .enumerate()
            .map(|(index, name)| {
                let selector = format!("agents-home-image-{index}-remove");
                let id = selector.clone();
                let remove = div()
                    .id(SharedString::from(id))
                    .debug_selector(move || selector)
                    .rounded(theme::length(cx, "radius-small"))
                    .hover(|el| el.bg(theme::color(cx, "bg-hover")))
                    .child(theme::icon(cx, "close", 12.0, muted))
                    .on_click(cx.listener(move |page, _, _, cx| page.remove_image(index, cx)));
                div()
                    .debug_selector(move || format!("agents-home-image-{index}"))
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .pl(px(6.0))
                    .rounded(theme::length(cx, "radius-small"))
                    .bg(theme::color(cx, "bg-hover"))
                    .child(theme::icon(cx, "file-image", 12.0, muted))
                    .child(SharedString::from(name))
                    .child(remove)
                    .into_any_element()
            })
            .collect();
        Some(div().flex().flex_wrap().gap(px(4.0)).px(px(8.0)).pt(px(6.0)).children(chips).into_any_element())
    }

    /// Why Start cannot be pressed, or what the agent's settings wait for: the agent's program missing, or no
    /// settings before its first task.
    fn note(&self, cx: &App) -> Option<(String, bool)> {
        let store = self.store.read(cx);
        let provider = store.provider(&store.draft.provider)?;
        if !provider.installed {
            return Some((tx_template(cx, &format!("{} was not found on PATH", provider.command)), true));
        }
        provider.config.is_empty().then(|| (tx(cx, "Models and modes appear after this agent's first task."), false))
    }
}

/// The heading of a group of commands.
fn heading(source: AgentCommandSource) -> &'static str {
    match source {
        AgentCommandSource::Command => "Commands",
        AgentCommandSource::Skill => "Skills",
    }
}

/// The rows of the command list: a heading before each run of commands or skills (the matches put commands
/// first), then the commands, numbered in order.
fn command_rows(commands: Vec<AgentCommand>) -> Vec<CommandRow> {
    let mut rows = Vec::with_capacity(commands.len() + 2);
    let mut source = None;
    for (index, command) in commands.into_iter().enumerate() {
        if source != Some(command.source) {
            source = Some(command.source);
            rows.push(CommandRow::Heading(command.source));
        }
        rows.push(CommandRow::Command(index, command));
    }
    rows
}

impl Focusable for NewTaskView {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.field.focus_handle(cx)
    }
}

impl Render for NewTaskView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let agent = {
            let store = self.store.read(cx);
            let id = &store.draft.provider;
            store.provider(id).map_or_else(|| id.clone(), |provider| provider.name.clone())
        };
        let enabled = self.can_start(cx);
        let label = self.start_label(cx);
        let muted = theme::color(cx, "fg-muted");
        let measured = Rc::clone(&self.agent_button);
        let measure = canvas(move |bounds, _, _| measured.set(Some(bounds)), |_, _, _, _| {}).absolute().size_full();
        let chooser = div()
            .id("agents-home-agent")
            .debug_selector(|| "agents-home-agent".to_owned())
            .relative()
            .h(px(24.0))
            .px(px(6.0))
            .flex()
            .flex_none()
            .items_center()
            .gap(px(4.0))
            .rounded(theme::length(cx, "radius-small"))
            .hover(|el| el.bg(theme::color(cx, "bg-hover")))
            .child(SharedString::from(agent))
            .child(theme::icon(cx, "chevron-down", 12.0, muted))
            .child(measure)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|page, _: &MouseDownEvent, _, cx| {
                    let at = popup_anchor::button_menu(page.agent_button.get());
                    cx.emit(NewTaskEvent::ChooseAgent(at));
                }),
            );
        let start = div()
            .id("agents-home-start")
            .debug_selector(|| "agents-home-start".to_owned())
            .size(px(26.0))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .rounded(theme::length(cx, "radius-control"))
            .tooltip(tooltip(label))
            .when(enabled, |el| {
                el.bg(theme::color(cx, "accent")).on_click(cx.listener(|page, _, _, cx| page.start(cx)))
            })
            .when(!enabled, |el| el.opacity(0.5))
            .child(theme::icon(cx, "send", 14.0, theme::color(cx, if enabled { "fg-on-accent" } else { "fg-muted" })));
        let chips = pickers::chip_elements(self, cx);
        let images = self.images_row(cx);
        let commands = self.command_list(cx);
        let picker = pickers::picker_element(self, cx);
        let settings = pickers::settings_element(self, cx);
        let project = self.project_selector(cx);
        let message = div().px(px(8.0)).pt(px(6.0)).h(theme::length(cx, "h-control")).child(self.field.clone());
        let bar = div()
            .flex()
            .items_center()
            .justify_between()
            .px(px(4.0))
            .pb(px(4.0))
            .child(div().flex().flex_wrap().items_center().gap(px(2.0)).child(chooser).children(chips))
            .child(start);
        let composer = div()
            .relative()
            .flex()
            .flex_col()
            .border_1()
            .border_color(theme::color(cx, "border"))
            .rounded(theme::length(cx, "radius-control"))
            .bg(theme::color(cx, "bg-field"))
            .children(images)
            .child(message)
            .child(bar)
            .children(commands)
            .children(picker)
            .children(settings);
        let error = self.error.clone().map(|error| {
            div()
                .debug_selector(|| "agents-home-error".to_owned())
                .text_color(theme::color(cx, "danger"))
                .child(SharedString::from(error))
        });
        let note = self.note(cx).filter(|_| error.is_none()).map(|(text, warning)| {
            div()
                .debug_selector(|| "agents-home-note".to_owned())
                .text_color(theme::color(cx, if warning { "danger" } else { "fg-muted" }))
                .child(SharedString::from(text))
        });
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .p(px(8.0))
            .text_size(theme::length(cx, "fs-ui"))
            .text_color(theme::color(cx, "fg"))
            .children(project.map(|project| div().flex().items_center().child(project)))
            .child(composer)
            .children(error)
            .children(note)
    }
}

#[cfg(test)]
mod tests;
