//! Editor › Vim: Enable Vim emulation, the rc file (`~/.openideavimrc`, else `~/.ideavimrc`) with
//! the mappings read from it and the lines left out, and the shortcut conflicts table, where each
//! key Vim also handles goes to Vim or to the IDE first.
//!
//! The settings are IDE-level UI state kept in the UI storage under [`VIM_SETTINGS_KEY`], not in the
//! settings files: every change is stored at once, so the page stages nothing and Apply, Reset and
//! Cancel leave it alone. The page reads the storage each time it is drawn, so another window's
//! change shows at once, and hands each change to the workbench as [`VimSettingsEvent::Changed`].
//!
//! The rc file is read by the workbench when the page is first shown in the app
//! ([`VimSettingsEvent::Shown`]), and again when Reload asks ([`VimSettingsEvent::ReloadRc`]); what it
//! found is the [`VimRcStatus`] global the page draws.

use super::{Binding, drop_down, drop_down_row, row, select_box};
use crate::features_a::vim::conflicts::{VIM_KEYS, conflict_rows};
use crate::features_a::vim::rc::{RcCommand, RcIgnored, RcParseResult};
use crate::features_a::vim::settings::{Handler, VIM_SETTINGS_KEY, VimSettings, parse_vim_settings};
use crate::platform::actions::ActionRegistry;
use crate::platform::i18n::Localization;
use crate::platform::input::Keymap;
use crate::platform::keymap::{format_binding, is_mac_host};
use crate::platform::storage::UiStorage;
use crate::settings_view::controls::{button, check_box, search_label, tx};
use crate::theme;
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Div, EventEmitter, Global, IntoElement, MouseButton, Render, SharedString,
    Subscription, Window, div, prelude::*, px,
};

pub const PAGE: &str = "editor.vim";

const ENABLE: &str = "Enable Vim emulation";
const CONFIGURATION_FILE: &str = "Configuration file";
const RELOAD: &str = "Reload";
const NO_RC_FILE: &str = "No ~/.openideavimrc or ~/.ideavimrc found.";
const MAPPINGS: &str = "mappings";
const SUPPORTED: &str = concat!(
    "Supported: set options, map / nmap / imap / vmap / noremap variants, let mapleader, source, and :action / ",
    "<Action>() mappings that run IDE actions. :w saves, :q closes the editor tab."
);
const SHORTCUT_CONFLICTS: &str = "Shortcut conflicts";
const CONFLICTS_HINT: &str = concat!(
    "Keys Vim also handles. With Vim as the handler the key goes to the editor while Vim mode is on and an editor ",
    "has focus; with IDE, an enabled IDE action runs first."
);
const SHORTCUT: &str = "Shortcut";
const IDE_ACTION: &str = "IDE action";
const VIM: &str = "Vim";
const HANDLER: &str = "Handler";
const RESET: &str = "Reset to Defaults";
/// What the IDE action column shows for a key no action is bound to.
const NO_ACTION: &str = "—";

/// What the Settings search finds on the page.
pub const LABELS: [&str; 9] =
    [ENABLE, CONFIGURATION_FILE, RELOAD, SHORTCUT_CONFLICTS, SHORTCUT, IDE_ACTION, VIM, HANDLER, RESET];

/// The handlers a conflict can have, as the drop-down offers them: (handler, label).
const HANDLERS: [(Handler, &str); 2] = [(Handler::Vim, "Vim"), (Handler::Ide, "IDE")];

/// What the rc file read last gave: the file (`None` when there is none), the mappings in it and
/// the lines left out. `loaded` once a read has finished, so the page asks for one only while none
/// has.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VimRcStatus {
    pub path: Option<String>,
    pub mappings: usize,
    pub ignored: Vec<RcIgnored>,
    pub loaded: bool,
}

impl Global for VimRcStatus {}

impl VimRcStatus {
    /// The status of the rc file at `path` (none when no file was found) parsed as `result`: every
    /// command but `set` counts as a mapping.
    pub fn read(path: Option<String>, result: &RcParseResult) -> Self {
        let mappings = result.commands.iter().filter(|command| !matches!(command, RcCommand::Set { .. })).count();
        Self { path, mappings, ignored: result.ignored.clone(), loaded: true }
    }
}

/// The Vim settings as stored, the defaults while nothing readable is.
pub fn stored_vim_settings(cx: &App) -> VimSettings {
    parse_vim_settings(cx.try_global::<UiStorage>().and_then(|storage| storage.get(VIM_SETTINGS_KEY)))
}

/// `settings` with `handler` for `stroke`: the default handler of the key is no override, so
/// choosing it removes the key's entry.
pub fn with_handler(settings: &VimSettings, stroke: &str, handler: Handler) -> VimSettings {
    let mut next = settings.clone();
    if VIM_KEYS.iter().find(|key| key.stroke == stroke).map(|key| key.handler) == Some(handler) {
        next.handlers.remove(stroke);
    } else {
        next.handlers.insert(stroke.to_owned(), handler);
    }
    next
}

/// A registered action's title in the active language; the id for an action not registered.
fn action_title(id: &str, cx: &App) -> String {
    let Some(action) = cx.try_global::<ActionRegistry>().and_then(|registry| registry.get(id)) else {
        return id.to_owned();
    };
    match cx.try_global::<Localization>() {
        Some(localization) => action.meta.localized(&localization.0).title,
        None => action.meta.title.clone(),
    }
}

/// An ignored line as the page lists it: `file:line: text — reason`.
fn ignored_line(ignored: &RcIgnored) -> String {
    let file = ignored.file.as_deref().map(|file| format!("{file}:")).unwrap_or_default();
    format!("{file}{}: {} — {}", ignored.line, ignored.text, ignored.reason)
}

/// Text in the code font.
fn mono(cx: &App, text: String) -> Div {
    div().font_family("monospace").text_size(theme::length(cx, "fs-small")).child(text)
}

/// A muted help line.
fn hint(cx: &App, text: String) -> Div {
    div().text_size(theme::length(cx, "fs-small")).text_color(theme::color(cx, "fg-muted")).child(text)
}

/// A column title of the conflicts table, marked by the search.
fn heading(cx: &App, title: &str, query: &str) -> Div {
    div().child(search_label(cx, &tx(cx, title), query))
}

/// A row of the conflicts table as drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictLine {
    /// The key as stored (`Ctrl+R`).
    pub stroke: &'static str,
    /// The key as shown (`⌃R`, `Ctrl+R`).
    pub shortcut: String,
    /// The titles of the IDE actions bound to it, or `—`.
    pub actions: String,
    /// What Vim does with it.
    pub vim: String,
    pub handler: Handler,
}

/// What the page asks of the workbench.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VimSettingsEvent {
    /// The settings changed: store them.
    Changed(VimSettings),
    /// Reload: read the rc file again.
    ReloadRc,
    /// The dialog shows the page: read the rc file if nothing has read it yet.
    Shown,
}

pub struct VimSettingsView {
    binding: Binding,
    /// The row whose handler choices are open.
    open_handler: Option<usize>,
    /// Whether the dialog shows the page now.
    page_shown: bool,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<VimSettingsEvent> for VimSettingsView {}

impl VimSettingsView {
    pub fn new(binding: Binding, cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![
            cx.observe(&binding.model, |view: &mut Self, _, cx| {
                view.follow_page(cx);
                cx.notify();
            }),
            // Another window's change shows on the open page.
            cx.observe_global::<UiStorage>(|_: &mut Self, cx| cx.notify()),
            cx.observe_global::<VimRcStatus>(|_: &mut Self, cx| cx.notify()),
            // The IDE action column follows the keymap.
            cx.observe_global::<Keymap>(|_: &mut Self, cx| cx.notify()),
        ];
        let page_shown = binding.model.read(cx).page_id == binding.page;
        Self { binding, open_handler: None, page_shown, _subscriptions: subscriptions }
    }

    /// Tells the workbench each time the dialog comes to show the page.
    fn follow_page(&mut self, cx: &mut Context<Self>) {
        let shown = self.binding.model.read(cx).page_id == self.binding.page;
        if shown == self.page_shown {
            return;
        }
        self.page_shown = shown;
        if shown {
            cx.emit(VimSettingsEvent::Shown);
        } else {
            self.open_handler = None;
        }
    }

    /// Whether the dialog shows the page now.
    pub fn is_shown(&self) -> bool {
        self.page_shown
    }

    pub fn settings(&self, cx: &App) -> VimSettings {
        stored_vim_settings(cx)
    }

    /// What the rc file read last gave; nothing read yet while none has finished.
    pub fn rc_status(&self, cx: &App) -> VimRcStatus {
        cx.try_global::<VimRcStatus>().cloned().unwrap_or_default()
    }

    /// The conflicts table: every key Vim uses, the IDE actions the keymap binds to it, and who gets
    /// it first.
    pub fn conflicts(&self, cx: &App) -> Vec<ConflictLine> {
        let settings = self.settings(cx);
        let bindings = cx.try_global::<Keymap>().map(|keymap| keymap.0.effective_bindings()).unwrap_or_default();
        let mac = is_mac_host();
        conflict_rows(bindings, &settings.handlers)
            .into_iter()
            .map(|row| {
                let titles: Vec<String> = row.actions.iter().map(|id| action_title(id, cx)).collect();
                ConflictLine {
                    stroke: row.key.stroke,
                    shortcut: format_binding(row.key.stroke, mac),
                    actions: if titles.is_empty() { NO_ACTION.to_owned() } else { titles.join(", ") },
                    vim: tx(cx, row.key.vim),
                    handler: row.current,
                }
            })
            .collect()
    }

    /// Whether Reset to Defaults can be used: a key has a handler of its own.
    pub fn can_reset(&self, cx: &App) -> bool {
        !self.settings(cx).handlers.is_empty()
    }

    /// The row whose handler choices are open.
    pub fn open_handler(&self) -> Option<usize> {
        self.open_handler
    }

    fn change(&mut self, settings: VimSettings, cx: &mut Context<Self>) {
        cx.emit(VimSettingsEvent::Changed(settings));
        cx.notify();
    }

    pub fn toggle_enabled(&mut self, cx: &mut Context<Self>) {
        let settings = self.settings(cx);
        self.change(VimSettings { enabled: !settings.enabled, ..settings }, cx);
    }

    /// The key `stroke` goes to `handler` first.
    pub fn set_handler(&mut self, stroke: &str, handler: Handler, cx: &mut Context<Self>) {
        self.open_handler = None;
        let settings = with_handler(&self.settings(cx), stroke, handler);
        self.change(settings, cx);
    }

    /// Reset to Defaults: every key back to its default handler; Vim emulation stays as it is.
    pub fn reset_handlers(&mut self, cx: &mut Context<Self>) {
        self.open_handler = None;
        let settings = VimSettings { handlers: Default::default(), ..self.settings(cx) };
        self.change(settings, cx);
    }

    pub fn reload(&mut self, cx: &mut Context<Self>) {
        cx.emit(VimSettingsEvent::ReloadRc);
    }

    /// The rc file line: its path and the mappings in it, or that there is none.
    fn rc_line(&self, cx: &App) -> String {
        let status = self.rc_status(cx);
        match &status.path {
            Some(path) => format!("{path} · {} {}", status.mappings, tx(cx, MAPPINGS)),
            None => tx(cx, NO_RC_FILE),
        }
    }

    /// The page as text: the checkbox, the rc file section with its ignored lines indented, the
    /// conflicts section with `shortcut | actions | vim | handler` per key, and Reset to Defaults
    /// (`(disabled)` while there is nothing to reset).
    pub fn lines(&self, cx: &App) -> Vec<String> {
        let mark = if self.settings(cx).enabled { "[x]" } else { "[ ]" };
        let mut out = vec![
            format!("{mark} {}", tx(cx, ENABLE)),
            format!("{} [{}]", tx(cx, CONFIGURATION_FILE), tx(cx, RELOAD)),
            self.rc_line(cx),
            tx(cx, SUPPORTED),
        ];
        out.extend(self.rc_status(cx).ignored.iter().map(|ignored| format!("  {}", ignored_line(ignored))));
        out.push(tx(cx, SHORTCUT_CONFLICTS));
        out.push(tx(cx, CONFLICTS_HINT));
        out.push([SHORTCUT, IDE_ACTION, VIM, HANDLER].map(|title| tx(cx, title)).join(" | "));
        for line in self.conflicts(cx) {
            let handler = HANDLERS.iter().find(|(h, _)| *h == line.handler).map_or("", |(_, label)| *label);
            out.push(format!("{} | {} | {} | {}", line.shortcut, line.actions, line.vim, tx(cx, handler)));
        }
        let reset = format!("[{}]", tx(cx, RESET));
        out.push(if self.can_reset(cx) { reset } else { format!("{reset} (disabled)") });
        out
    }

    fn header(&self, title: &str, query: &str, cx: &App) -> Div {
        let text = tx(cx, title);
        row(cx, self.binding.matches(title, cx))
            .justify_between()
            .text_color(theme::color(cx, "fg-muted"))
            .child(search_label(cx, &text, query))
    }

    fn rc_section(&self, query: &str, cx: &mut Context<Self>) -> AnyElement {
        let status = self.rc_status(cx);
        let reload_label = tx(cx, RELOAD);
        let reload = div()
            .id("settings-vim-reload")
            .debug_selector(|| "settings-vim-reload".to_owned())
            .child(button(cx, &reload_label, false, true))
            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.reload(cx)));
        let file = match &status.path {
            Some(path) => div()
                .flex()
                .flex_wrap()
                .text_size(theme::length(cx, "fs-small"))
                .text_color(theme::color(cx, "fg-muted"))
                .child(mono(cx, path.clone()))
                .child(format!(" · {} {}", status.mappings, tx(cx, MAPPINGS))),
            None => hint(cx, tx(cx, NO_RC_FILE)),
        };
        let ignored = (!status.ignored.is_empty()).then(|| {
            div()
                .flex()
                .flex_col()
                .pl(px(16.0))
                .text_color(theme::color(cx, "fg-muted"))
                .children(status.ignored.iter().map(|line| mono(cx, ignored_line(line))))
        });
        div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(self.header(CONFIGURATION_FILE, query, cx).child(reload))
            .child(file)
            .child(hint(cx, tx(cx, SUPPORTED)))
            .children(ignored)
            .into_any_element()
    }

    fn conflict_row(&self, index: usize, line: &ConflictLine, cx: &mut Context<Self>) -> AnyElement {
        let select = format!("settings-vim-handler-{index}");
        let stroke = line.stroke;
        let current = line.handler;
        let choices = (self.open_handler == Some(index)).then(|| {
            let rows: Vec<AnyElement> = HANDLERS
                .iter()
                .map(|&(handler, label)| {
                    let selector = format!("settings-vim-handler-{index}-{}", handler.as_str());
                    let text = tx(cx, label);
                    drop_down_row(cx, selector, &text, current == handler)
                        .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.set_handler(stroke, handler, cx)))
                        .into_any_element()
                })
                .collect();
            drop_down(cx, rows)
        });
        let shown = HANDLERS.iter().find(|(h, _)| *h == current).map_or(String::new(), |(_, label)| tx(cx, label));
        div()
            .h(px(28.0))
            .px(px(8.0))
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(div().w(px(120.0)).flex_none().font_family("monospace").child(line.shortcut.clone()))
            .child(div().flex_1().min_w_0().overflow_hidden().whitespace_nowrap().child(line.actions.clone()))
            .child(div().w(px(260.0)).flex_none().child(line.vim.clone()))
            .child(
                div()
                    .w(px(160.0))
                    .flex_none()
                    .relative()
                    .child(
                        select_box(cx, &shown)
                            .id(SharedString::from(select.clone()))
                            .debug_selector(move || select)
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                                view.open_handler = if view.open_handler == Some(index) { None } else { Some(index) };
                                cx.notify();
                            })),
                    )
                    .children(choices),
            )
            .into_any_element()
    }

    fn conflicts_section(&self, query: &str, cx: &mut Context<Self>) -> AnyElement {
        let border = theme::color(cx, "border-strong");
        let lines = self.conflicts(cx);
        let rows: Vec<AnyElement> =
            lines.iter().enumerate().map(|(index, line)| self.conflict_row(index, line, cx)).collect();
        let head = div()
            .px(px(8.0))
            .py(px(4.0))
            .flex()
            .gap(px(8.0))
            .text_color(theme::color(cx, "fg-muted"))
            .border_b_1()
            .border_color(border)
            .child(heading(cx, SHORTCUT, query).w(px(120.0)).flex_none())
            .child(heading(cx, IDE_ACTION, query).flex_1())
            .child(heading(cx, VIM, query).w(px(260.0)).flex_none())
            .child(heading(cx, HANDLER, query).w(px(160.0)).flex_none());
        let can_reset = self.can_reset(cx);
        let reset_label = tx(cx, RESET);
        let reset = div()
            .id("settings-vim-reset")
            .debug_selector(|| "settings-vim-reset".to_owned())
            .child(button(cx, &reset_label, false, can_reset))
            .when(can_reset, |el| el.on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.reset_handlers(cx))));
        div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(self.header(SHORTCUT_CONFLICTS, query, cx))
            .child(hint(cx, tx(cx, CONFLICTS_HINT)))
            .child(div().flex().flex_col().border_1().border_color(border).child(head).children(rows))
            // The button keeps its own width, as an inline button does.
            .child(div().flex().child(reset))
            .into_any_element()
    }
}

impl Render for VimSettingsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.binding.query(cx);
        let enabled = self.settings(cx).enabled;
        let enable_label = tx(cx, ENABLE);
        let enable = row(cx, self.binding.matches(ENABLE, cx))
            .id("settings-vim-enabled")
            .debug_selector(|| "settings-vim-enabled".to_owned())
            .child(check_box(cx, enabled))
            .child(search_label(cx, &enable_label, &query))
            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.toggle_enabled(cx)));
        let rc = self.rc_section(&query, cx);
        let conflicts = self.conflicts_section(&query, cx);
        // The dialog gives a page drawn whole by its own view the page's size: the page scrolls itself.
        div()
            .id("settings-vim-page")
            .debug_selector(|| "settings-vim-page".to_owned())
            .size_full()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(16.0))
            .text_color(theme::color(cx, "fg"))
            // A press anywhere else on the page closes open handler choices.
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, _, _, cx| {
                    if view.open_handler.take().is_some() {
                        cx.notify();
                    }
                }),
            )
            .child(div().flex().child(enable))
            .child(rc)
            .child(conflicts)
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{click, dialog_model, init};
    use super::*;
    use crate::features_a::vim::rc::parse_vimrc;
    use crate::features_a::vim::settings::vim_settings_json;
    use crate::settings_model::staging::{Scope, SettingsDialog};
    use gpui_kit::{Entity, TestAppContext, VisualTestContext};
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::rc::Rc;

    type Seen = Rc<RefCell<Vec<VimSettingsEvent>>>;

    /// The line of the first key in [`VimSettingsView::lines`] while no line of the rc file is ignored.
    const FIRST_KEY_LINE: usize = 7;

    /// The page in a window of its own over `stored` settings, with the events it emits.
    fn open(
        cx: &mut TestAppContext,
        stored: Option<VimSettings>,
    ) -> (Entity<VimSettingsView>, Seen, &mut VisualTestContext) {
        init(cx);
        let stored = stored.as_ref().map(vim_settings_json);
        cx.update(|cx| {
            let mut storage = UiStorage::default();
            if let Some(stored) = stored {
                storage.set(VIM_SETTINGS_KEY, stored);
            }
            cx.set_global(storage);
        });
        let model = cx.update(|cx| dialog_model(PAGE, &[], cx));
        let binding = Binding::fixed(model, PAGE, Scope::Global);
        let (view, cx) = cx.add_window_view(move |_, cx| VimSettingsView::new(binding, cx));
        cx.update(|window, _| window.activate_window());
        let seen: Seen = Rc::default();
        let log = seen.clone();
        cx.update(|_, cx| {
            cx.subscribe(&view, move |_, event: &VimSettingsEvent, _| log.borrow_mut().push(event.clone())).detach();
        });
        (view, seen, cx)
    }

    fn last_settings(seen: &Seen) -> VimSettings {
        match seen.borrow().last() {
            Some(VimSettingsEvent::Changed(settings)) => settings.clone(),
            other => panic!("the settings changed, not {other:?}"),
        }
    }

    fn index_of(stroke: &str) -> usize {
        VIM_KEYS.iter().position(|key| key.stroke == stroke).expect("a Vim key")
    }

    // Not from a spec: choosing a key's default handler removes its entry, any other stores it.
    #[test]
    fn choosing_the_default_handler_removes_the_override() {
        let none = VimSettings::default();
        let ide = with_handler(&none, "Ctrl+R", Handler::Ide);
        assert_eq!(ide.handlers, BTreeMap::from([("Ctrl+R".to_owned(), Handler::Ide)]));
        assert_eq!(with_handler(&ide, "Ctrl+R", Handler::Vim), none);
        let escape = with_handler(&none, "Escape", Handler::Vim);
        assert_eq!(escape.handlers.get("Escape"), Some(&Handler::Vim));
        assert_eq!(with_handler(&escape, "Escape", Handler::Ide).handlers.get("Escape"), None);
    }

    // Not from a spec: every command but `set` counts as a mapping, and the ignored lines are kept.
    #[test]
    fn the_status_counts_every_command_but_set_as_a_mapping() {
        let result = parse_vimrc("set tw=80\nnnoremap j gj\nnmap gd <Action>(GotoDeclaration)\nautocmd X\n", None);
        let status = VimRcStatus::read(Some("/h/.ideavimrc".to_owned()), &result);
        assert_eq!((status.mappings, status.loaded), (2, true));
        let ignored: Vec<String> = status.ignored.iter().map(ignored_line).collect();
        assert_eq!(ignored, ["4: autocmd X — unsupported command: autocmd"]);
        let sourced =
            RcIgnored { file: Some("~/x.vim".to_owned()), line: 2, text: "a".to_owned(), reason: "b".to_owned() };
        assert_eq!(ignored_line(&sourced), "~/x.vim:2: a — b");
    }

    // Not from a spec: the page as text before anything is read, with the twenty keys and their
    // default handlers.
    #[gpui_kit::test]
    async fn the_page_lists_every_vim_key_with_its_default_handler(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx, None);
        let lines = view.read_with(cx, |view, cx| view.lines(cx));
        assert_eq!(lines[0], "[ ] Enable Vim emulation");
        assert_eq!(lines[1], "Configuration file [Reload]");
        assert_eq!(lines[2], "No ~/.openideavimrc or ~/.ideavimrc found.");
        assert_eq!(lines[4], "Shortcut conflicts");
        assert_eq!(lines[6], "Shortcut | IDE action | Vim | Handler");
        assert_eq!(lines.len(), FIRST_KEY_LINE + VIM_KEYS.len() + 1);
        assert!(lines[FIRST_KEY_LINE].ends_with(" | Exit to Normal mode | IDE"), "{}", lines[FIRST_KEY_LINE]);
        assert!(lines[FIRST_KEY_LINE + index_of("Ctrl+R")].ends_with(" | Redo / insert register | Vim"));
        assert_eq!(lines.last().map(String::as_str), Some("[Reset to Defaults] (disabled)"));
    }

    // Not from a spec: the checkbox, a handler chosen from its list and Reset to Defaults hand the
    // settings on; Reset to Defaults keeps Vim emulation as it is.
    #[gpui_kit::test]
    async fn each_control_hands_the_settings_on(cx: &mut TestAppContext) {
        let stored = VimSettings { enabled: true, handlers: BTreeMap::from([("Ctrl+V".to_owned(), Handler::Ide)]) };
        let (view, seen, cx) = open(cx, Some(stored));
        click(cx, "settings-vim-enabled");
        assert!(!last_settings(&seen).enabled);
        let row = index_of("Ctrl+R");
        click(cx, &format!("settings-vim-handler-{row}"));
        assert_eq!(view.read_with(cx, |view, _| view.open_handler()), Some(row));
        click(cx, &format!("settings-vim-handler-{row}-ide"));
        assert_eq!(view.read_with(cx, |view, _| view.open_handler()), None);
        let chosen = last_settings(&seen);
        assert_eq!(chosen.handlers.get("Ctrl+R"), Some(&Handler::Ide));
        assert_eq!(chosen.handlers.get("Ctrl+V"), Some(&Handler::Ide), "the other key keeps its handler");
        click(cx, "settings-vim-reset");
        let reset = last_settings(&seen);
        assert!(reset.handlers.is_empty());
        assert!(reset.enabled, "Reset to Defaults keeps Vim emulation");
        click(cx, "settings-vim-reload");
        assert_eq!(seen.borrow().last(), Some(&VimSettingsEvent::ReloadRc));
    }

    // Not from a spec: Reset to Defaults asks nothing while no key has a handler of its own, and the
    // page shows what the storage holds as it changes.
    #[gpui_kit::test]
    async fn reset_to_defaults_is_off_without_overrides_and_the_page_follows_the_storage(cx: &mut TestAppContext) {
        let (view, seen, cx) = open(cx, None);
        click(cx, "settings-vim-reset");
        assert!(seen.borrow().is_empty());
        let stored = VimSettings { enabled: true, handlers: BTreeMap::from([("Escape".to_owned(), Handler::Vim)]) };
        cx.update(|_, cx| cx.global_mut::<UiStorage>().set(VIM_SETTINGS_KEY, vim_settings_json(&stored)));
        cx.run_until_parked();
        let lines = view.read_with(cx, |view, cx| view.lines(cx));
        assert_eq!(lines[0], "[x] Enable Vim emulation");
        assert!(lines[FIRST_KEY_LINE].ends_with(" | Exit to Normal mode | Vim"), "{}", lines[FIRST_KEY_LINE]);
        assert_eq!(lines.last().map(String::as_str), Some("[Reset to Defaults]"));
    }

    // Not from a spec: the page tells when the dialog comes to show it, not while another page is
    // shown, and once each time it comes back.
    #[gpui_kit::test]
    async fn the_page_says_when_the_dialog_comes_to_show_it(cx: &mut TestAppContext) {
        init(cx);
        cx.update(|cx| cx.set_global(UiStorage::default()));
        let model = cx.update(|cx| dialog_model("appearance", &[], cx));
        let binding = Binding::fixed(model.clone(), PAGE, Scope::Global);
        let (view, cx) = cx.add_window_view(move |_, cx| VimSettingsView::new(binding, cx));
        let seen: Seen = Rc::default();
        let log = seen.clone();
        cx.update(|_, cx| {
            cx.subscribe(&view, move |_, event: &VimSettingsEvent, _| log.borrow_mut().push(event.clone())).detach();
        });
        assert!(!view.read_with(cx, |view, _| view.is_shown()));
        fn select(model: &Entity<SettingsDialog>, page: &str, cx: &mut VisualTestContext) {
            model.update(cx, |model, cx| {
                model.select_page(page);
                cx.notify();
            });
            cx.run_until_parked();
        }
        select(&model, PAGE, cx);
        assert_eq!(*seen.borrow(), [VimSettingsEvent::Shown]);
        select(&model, PAGE, cx);
        assert_eq!(seen.borrow().len(), 1, "a page shown already is not shown again");
        select(&model, "appearance", cx);
        select(&model, PAGE, cx);
        assert_eq!(*seen.borrow(), [VimSettingsEvent::Shown, VimSettingsEvent::Shown]);
    }

    // Not from a spec: the rc file's path, its mappings and its ignored lines show once read.
    #[gpui_kit::test]
    async fn the_rc_status_shows_the_file_its_mappings_and_its_ignored_lines(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx, None);
        let result = parse_vimrc("nnoremap j gj\nfoo\n", None);
        cx.update(|_, cx| cx.set_global(VimRcStatus::read(Some("/h/.ideavimrc".to_owned()), &result)));
        cx.run_until_parked();
        let lines = view.read_with(cx, |view, cx| view.lines(cx));
        assert_eq!(lines[2], "/h/.ideavimrc · 1 mappings");
        assert_eq!(lines[4], "  2: foo — unsupported command: foo");
        assert_eq!(lines[5], "Shortcut conflicts");
    }
}
