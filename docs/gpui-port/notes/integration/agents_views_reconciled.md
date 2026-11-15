# Agents views, reconciled: the setting pickers, the New Task page, the question card's hover preview

Nothing here was compiled or run. Reconciled on 2026-11-15 against the worktree `agent-a307dd904b5ae1f01` (HEAD `cfe32bcc`); the draft commit is `3a0f5b40`, and only 18 commits came after it. Since that commit the agents code itself barely moved (`git log 3a0f5b40..HEAD -- app-gpui/src/agents` is empty; `workbench/agents.rs` gained `agent_reply_busy`). What did change: `ui/text_field.rs` now clips and scrolls its text, `ui/tooltip.rs` has `press_watcher`, and `spawn_guard` has `gate` and `AgentSpawnSeam`. "Measured" below means a command run during this reconciliation. Everything else is labelled a hypothesis or "read in code".

This file stands on its own. The integrator needs it, the six draft files and `anchors.py`.

## 0. Before applying

1. `python3 -I <scratchpad>/agv/anchors.py <worktree>` checks every hunk below in the order they apply. Each anchor (or each end of a range) must occur exactly once in its file at the moment its hunk applies. A hunk whose new text is already there counts as applied. `--out DIR` writes the results to DIR for a look first. `--apply` writes them in place, and only when every hunk checks. The script exits 1 on any failure. Run it right before applying: `workbench/mod.rs` keeps moving.
2. Measured on the files the script wrote with `--out`: all nine changed files pass `rustfmt --edition 2024 --config max_width=120,use_small_heuristics=Max --check` with no differences. The six draft files pass the same check.

## 1. Files

Draft files (committed, not declared before H01, H02, H05, H23 and H40):

| File                                                    | What                                                                                                                                                                                                                                                         |
| ------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `app-gpui/src/agents/config_pickers_view.rs`            | `PickerHost` trait, `Pickers` state, free generic functions: the settings chips, their pickers (Model & effort with steps and a search field, mode with Allow All, plain lists, switches) and the Agent settings popup                                       |
| `app-gpui/src/agents/composer_view/pickers.rs`          | `impl PickerHost for ComposerView`, the composer's accessors (`chips`, `picker_entries`, `picker_details`, `picker_query`, `settings_title`, `settings_rows`, `settings_button_focused`), 8 view tests                                                       |
| `app-gpui/src/agents/new_task_view.rs`                  | `NewTaskView` (replaces `workbench::agents::NewTaskPage`) and `NewTaskEvent { Start { project_root }, ChooseAgent(Point), Effects(Vec<AgentEffect>) }`                                                                                                       |
| `app-gpui/src/agents/new_task_view/tests.rs`            | 16 view tests and 1 plain test                                                                                                                                                                                                                               |
| `app-gpui/src/workbench/agents_views.rs`                | `impl Workbench`: the page opened (`agents_home_opened`), its events (`agents_home_event`), the agent menu, reading the project's commands. A child module of `workbench::agents` (H23), so it can use that module's private runner, store and `spawn_guard` |
| `app-gpui/src/workbench/tests/proofs14_agents_views.rs` | 3 end-to-end tests through the window with the fake agent                                                                                                                                                                                                    |

`mod` lines (all inside hunks): `pub mod config_pickers_view;` (H01) and `pub mod new_task_view;` (H02) in `agents/mod.rs`; `mod pickers;` in `agents/composer_view.rs` (H05); `#[path = "agents_views.rs"] mod views;` in `workbench/agents.rs` (H23); `mod proofs14_agents_views;` in `workbench/mod.rs` `mod tests` (H40). `new_task_view.rs` already ends with `#[cfg(test)] mod tests;`.

### Draft edits made in this reconciliation

- `config_pickers_view.rs`: both inline popups (the picker and the Agent settings popup) get `.shadow(theme::shadow(cx, "shadow-popup"))`, as the webview's `Popup` (`src/ui/Popup.module.css`: `box-shadow: var(--shadow-popup)`) and the crate's other popups (`ui/popup_menu.rs`, `workbench/popup_placement.rs`) have.
- `new_task_view.rs`: the command list and the member list get the same popup shadow. The agent menu now opens under the agent chooser through the shared `ui::popup_anchor::button_menu`, not at the press point. The chooser's bounds are measured by a `canvas` child, as `services/view.rs` does for Add Service; this adds the field `agent_button: Rc<Cell<Option<Bounds<Pixels>>>>` and the imports `Bounds`, `canvas`, `popup_anchor`, `Cell` and `Rc`.
- `agents_views.rs`: More agents… runs `agents.addAgent` from `window.defer`, after the workbench's update, as `show_popup_menu` does for menu actions. Agents that are not installed can be chosen again, as in the webview (`HomeEditor.tsx` `Select` with `providerOptions`; the page then shows "… was not found on PATH"); the draft had disabled them. No separator is drawn before More agents… when it is the only row. `MenuItem` is now imported explicitly, because H20 removes the parent's import.
- `proofs14_agents_views.rs`: the agent-menu test no longer requires Fake Agent to be the first row. A built-in agent found on the machine's PATH is installed too and comes first, so the test now checks that Fake Agent comes before "Not installed".
- `new_task_view/tests.rs`: the section comments no longer name the webview spec files.
- `proofs13_agents_config_pending.rs`: deleted. Its two tests move to `proofs13_agents_config.rs` (H41), whose helpers are byte-for-byte the pending file's (measured: `diff` of the helper sections printed nothing).
- `proofs13_agents_session_pending.rs`: the question-card test moves to `proofs13_agents_session.rs` (H42). The drag test stays, under a new header that says what it waits for.

## 2. Hunks, in the order to apply them

Each heading is `H<n> · <file> · <kind>`. A `replace` swaps the anchor for the new text. `insert-after` puts a line break and the new text after the anchor. `insert-before` puts the new text and a line break before it. `delete-range` deletes from the start of `start` up to `end`, and `end` stays (`EOF`: to the end of the file, which keeps one final line break). Blocks are exact text with no trailing line break, unless the block ends with an empty line.

How the old notes' hunks map to these. Of the 42 hunks, 35 come from the old notes (sections 1–7). 29 are kept as they were. 3 are re-anchored: H15 (old 2k) is one multi-line anchor, because `let muted = …` occurs 3 times in `composer_view.rs` and `let mut chips: Vec<AnyElement>` twice; H18 inserts the import after `model` (rustfmt's order), not after `markdown`; H28 (old 3g) anchors on the separator and title pair, because `// The New Task page` occurs 4 times in `workbench/agents.rs`. 3 are rewritten: old 2j is split into H13 and H14, so that the closing brace of `impl ComposerView` stays; H25 (old 3d) logs a closed window instead of discarding the result. 7 are new: H20 (`MenuItem`), H22 (`fn tx`, which would be dead code), H37–H39 (a `workbench/mod.rs` test emits the old `NewTaskEvent::Start`), H41 and H42 (the moved tests). 1 old hunk is dropped: `mod proofs13_agents_config_pending;`, because that file is gone and its tests are in H41. No old hunk was already applied by another slice: each anchor still occurs once (measured with `grep -cF`), and nothing in the tree defines `TextTag`, `hovered()`, `config_pickers_view` or `new_task_view`.

### H01 · `app-gpui/src/agents/mod.rs` · insert-after

declare the pickers module.

After:

```anchor
pub mod config_pickers;
```

Insert:

```new
pub mod config_pickers_view;
```

### H02 · `app-gpui/src/agents/mod.rs` · insert-after

declare the New Task view.

After:

```anchor
pub mod model;
```

Insert:

```new
pub mod new_task_view;
```

### H03 · `app-gpui/src/agents/composer_view.rs` · replace

imports: the chips moved to config_pickers_view.

Replace:

```anchor
use crate::agents::config_bar::{
    Chip, ListKey, OptionList, Picker, Values, config_bar, effort_slider, effort_step, mode_chip, model_chip,
    select_icon, switch_choice, value_of,
};
use crate::agents::config_pickers::switch_values;
```

With:

```new
use crate::agents::config_pickers_view::{self, Pickers};
```

### H04 · `app-gpui/src/agents/composer_view.rs` · replace

imports: drop what only the moved picker code used (KeyDownEvent, MouseButton, deferred, AgentConfigOption).

Replace:

```anchor
use gpui_kit::{
    AnyElement, App, Context, Div, Entity, EntityInputHandler, EventEmitter, FocusHandle, Focusable, IntoElement,
    KeyDownEvent, Keystroke, MouseButton, Render, SharedString, Stateful, Subscription, Window, deferred, div,
    prelude::*, px,
};
use ide_agent::{AgentConfigOption, AgentTask};
```

With:

```new
use gpui_kit::{
    AnyElement, App, Context, Div, Entity, EntityInputHandler, EventEmitter, FocusHandle, Focusable, IntoElement,
    Keystroke, Render, SharedString, Stateful, Subscription, Window, div, prelude::*, px,
};
use ide_agent::AgentTask;
```

### H05 · `app-gpui/src/agents/composer_view.rs` · insert-before

the child module with the composer's PickerHost and its tests.

Before:

```anchor
/// How a line break of the message is shown, and typed, in the one-line field.
```

Insert:

```new
mod pickers;

```

### H06 · `app-gpui/src/agents/composer_view.rs` · delete-range

delete ConfigChip, impl ConfigChip, OpenPicker, fn find, fn chips (now in config_pickers_view.rs).

From (deleted):

```start
/// A chip of the settings row.
```

Up to (kept):

```end
pub struct ComposerView {
```

### H07 · `app-gpui/src/agents/composer_view.rs` · replace

the field.

Replace:

```anchor
    picker: Option<OpenPicker>,
    picker_focus: FocusHandle,
```

With:

```new
    /// The settings chips' pickers and the Agent settings popup.
    pickers: Pickers,
```

### H08 · `app-gpui/src/agents/composer_view.rs` · replace

an open picker follows the settings.

Replace:

```anchor
            cx.observe(&store, |view, _, cx| {
                view.sync_field(cx);
```

With:

```new
            cx.observe(&store, |view, _, cx| {
                view.sync_field(cx);
                config_pickers_view::refresh(view, cx);
```

### H09 · `app-gpui/src/agents/composer_view.rs` · replace

release the pickers' key claims too.

Replace:

```anchor
        cx.on_release(|view: &mut Self, cx| input::release_keys(view.claim, cx)).detach();
```

With:

```new
        cx.on_release(|view: &mut Self, cx| {
            input::release_keys(view.claim, cx);
            view.pickers.release(cx);
        })
        .detach();
```

### H10 · `app-gpui/src/agents/composer_view.rs` · replace

the initial value.

Replace:

```anchor
            picker: None,
            picker_focus: cx.focus_handle(),
```

With:

```new
            pickers: Pickers::new(cx),
```

### H11 · `app-gpui/src/agents/composer_view.rs` · delete-range

delete pub fn chips, pub fn picker_entries, fn config_chips (pickers.rs defines chips and picker_entries).

From (deleted):

```start
    /// The settings chips, as `<label>` with ` · <detail>`, ` [on]` / ` [off]` for a switch.
    pub fn chips(&self, cx: &App) -> Vec<String> {
```

Up to (kept):

```end
    // ----- what keys and clicks do -----
```

### H12 · `app-gpui/src/agents/composer_view.rs` · replace

Escape in the message field closes whichever popup is open.

Replace:

```anchor
            "escape" if self.picker.is_some() => self.close_picker(cx),
```

With:

```new
            "escape" if self.pickers.is_open() => config_pickers_view::close_popups(self, window, cx),
```

### H13 · `app-gpui/src/agents/composer_view.rs` · delete-range

delete chip_clicked, close_picker, entry_clicked, picker_key (set_config stays).

From (deleted):

```start
    fn chip_clicked(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
```

Up to (kept):

```end
    // ----- drawing -----
```

### H14 · `app-gpui/src/agents/composer_view.rs` · delete-range

delete fn picker_element (the last method of impl ComposerView; its closing brace stays).

From (deleted):

```start

    fn picker_element(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
```

Up to (kept):

```end
}

impl Focusable for ComposerView {
```

### H15 · `app-gpui/src/agents/composer_view.rs` · replace

render: the chips (ending with the Agent settings button) come from config_pickers_view.

Replace:

```anchor
        let muted = theme::color(cx, "fg-muted");
        let mut chips: Vec<AnyElement> = Vec::new();
        for (index, chip) in self.config_chips(cx).into_iter().enumerate() {
            let selector = format!("composer-config-{index}");
            let mut button =
                self.chip_button(selector, cx, move |view, window, cx| view.chip_clicked(index, window, cx));
            if let Some(icon) = chip.icon {
                button = button.child(theme::icon(cx, icon, 13.0, muted));
            }
            button = button.child(SharedString::from(chip.label));
            if let Some(detail) = chip.detail {
                let faint = theme::color(cx, "fg-faint");
                button = button.child(div().text_color(faint).child(SharedString::from(detail)));
            }
            match chip.switch {
                Some(on) => button = button.child(theme::icon(cx, if on { "check" } else { "minus" }, 12.0, muted)),
                None => button = button.child(theme::icon(cx, "chevron-down", 12.0, muted)),
            }
            chips.push(button.into_any_element());
        }
```

With:

```new
        let chips = config_pickers_view::chip_elements(self, cx);
```

### H16 · `app-gpui/src/agents/composer_view.rs` · replace

render: the popups.

Replace:

```anchor
            .children(self.picker_element(cx))
```

With:

```new
            .children(config_pickers_view::picker_element(self, cx))
            .children(config_pickers_view::settings_element(self, cx))
```

### H17 · `app-gpui/src/workbench/agents.rs` · replace

imports.

Replace:

```anchor
use crate::agents::composer::{LineRange, can_send, mention_of};
```

With:

```new
use crate::agents::composer::{LineRange, mention_of};
```

### H18 · `app-gpui/src/workbench/agents.rs` · insert-after

imports (rustfmt order: after model, before reply_actions).

After:

```anchor
use crate::agents::model::{HOME_KEY, REGISTRY_PAGE, TOOL_WINDOW};
```

Insert:

```new
use crate::agents::new_task_view::NewTaskView;
```

### H19 · `app-gpui/src/workbench/agents.rs` · replace

imports: theme and the text field were the old page's.

Replace:

```anchor
use crate::platform::storage::UiStorage;
use crate::theme;
use crate::ui::text_field::{TextField, TextFieldEvent};
```

With:

```new
use crate::platform::storage::UiStorage;
```

### H20 · `app-gpui/src/workbench/agents.rs` · replace

imports: only the old page's menu used MenuItem here (agents_views.rs imports it itself).

Replace:

```anchor
use crate::vcs_ops::dialogs::{AskButton, AskRequest, PromptRequest};
use crate::vcs_ops::menu::MenuItem;
```

With:

```new
use crate::vcs_ops::dialogs::{AskButton, AskRequest, PromptRequest};
```

### H21 · `app-gpui/src/workbench/agents.rs` · replace

imports: what the kept code uses (Focusable for focus_handle, BorrowAppContext for update_default_global).

Replace:

```anchor
use gpui_kit::{
    AnyView, AnyWindowHandle, App, BorrowAppContext, ClipboardItem, Context, Entity, EntityId, EventEmitter,
    FocusHandle, Focusable, IntoElement, MouseButton, MouseDownEvent, Pixels, Point, Render, SharedString,
    Subscription, Task, Window, div, prelude::*, px,
};
```

With:

```new
use gpui_kit::{
    AnyView, AnyWindowHandle, App, BorrowAppContext, ClipboardItem, Context, Entity, EntityId, Focusable,
    Subscription, Task, Window, prelude::*,
};
```

### H22 · `app-gpui/src/workbench/agents.rs` · replace

delete fn tx: only the old page used it (dead code under -D warnings otherwise).

Replace:

```anchor
/// UI text in the active language.
fn tx(cx: &App, text: &str) -> String {
    cx.try_global::<Localization>().map_or_else(|| text.to_owned(), |localization| localization.0.tx(text))
}

/// The string argument of an action.
```

With:

```new
/// The string argument of an action.
```

### H23 · `app-gpui/src/workbench/agents.rs` · insert-after

the child module (a #[path] in workbench/agents.rs resolves against workbench/).

After:

```anchor
const SESSION_FOCUS: &str = "agentSessionFocus";
```

Insert:

```new

/// The window's side of the New Task page.
#[path = "agents_views.rs"]
mod views;
```

### H24 · `app-gpui/src/workbench/agents.rs` · replace

the slot's page.

Replace:

```anchor
    home: Option<(Entity<NewTaskPage>, Subscription)>,
```

With:

```new
    home: Option<(Entity<NewTaskView>, Subscription)>,
```

### H25 · `app-gpui/src/workbench/agents.rs` · replace

creating the page (in sync_agents_window), then telling it what it needs once the slot is no longer borrowed.

Replace:

```anchor
            let page = cx.new(|cx| NewTaskPage::new(store.clone(), cx));
            let events = cx.subscribe(&page, Self::agents_home_event);
            slot.home = Some((page, events));
```

With:

```new
            let page = cx.new(|cx| NewTaskView::new(store.clone(), cx));
            let events = cx.subscribe(&page, Self::agents_home_event);
            slot.home = Some((page.clone(), events));
            // Once this update is over (the slot is borrowed here): the page learns the workspace's members,
            // the agents are listed again and the agent's commands are read.
            cx.spawn(async move |this, cx| {
                if this.update(cx, |workbench, cx| workbench.agents_home_opened(&page, cx)).is_err() {
                    tracing::debug!("the window closed before the New Task page opened");
                }
            })
            .detach();
```

### H26 · `app-gpui/src/workbench/agents.rs` · delete-range

delete the old event handler (agents_views.rs defines agents_home_event; choose_task_agent stays).

From (deleted):

```start
    /// Carries out what the New Task page asks for.
    fn agents_home_event(&mut self, page: Entity<NewTaskPage>, event: &NewTaskEvent, cx: &mut Context<Self>) {
```

Up to (kept):

```end
    /// The New Task page's agent: its settings start over.
```

### H27 · `app-gpui/src/workbench/agents.rs` · replace

the test accessor.

Replace:

```anchor
    pub(super) fn agents_home_page(&self) -> Option<Entity<NewTaskPage>> {
```

With:

```new
    pub(super) fn agents_home_page(&self) -> Option<Entity<NewTaskView>> {
```

### H28 · `app-gpui/src/workbench/agents.rs` · delete-range

delete the old page to the end of the file (NewTaskEvent, NewTaskPage and its impls).

From (deleted):

```start

// -------------------------------------------------------------------------------------------------
// The New Task page

```

To the end of the file.

```end
EOF
```

### H29 · `app-gpui/src/ui/text_field.rs` · insert-before

the tag type.

Before:

```anchor
pub struct TextField {
```

Insert:

```new
/// A run of a text field's text drawn in its own colours, such as an owner's tag for a file reference.
#[derive(Clone, Debug, PartialEq)]
pub struct TextTag {
    /// Bytes of the text.
    pub range: Range<usize>,
    pub color: Hsla,
    pub background: Option<Hsla>,
}

```

### H30 · `app-gpui/src/ui/text_field.rs` · replace

the field.

Replace:

```anchor
    /// The steps Undo and Redo walk.
    history: TextHistory,
}
```

With:

```new
    /// The steps Undo and Redo walk.
    history: TextHistory,
    /// Runs drawn in their own colours, in order and apart; the owner keeps them up to date with the text.
    tags: Vec<TextTag>,
}
```

### H31 · `app-gpui/src/ui/text_field.rs` · replace

the initial value.

Replace:

```anchor
            history: TextHistory::default(),
        }
```

With:

```new
            history: TextHistory::default(),
            tags: Vec::new(),
        }
```

### H32 · `app-gpui/src/ui/text_field.rs` · insert-after

the accessors.

After:

```anchor
    pub fn is_read_only(&self) -> bool {
        self.read_only
    }
```

Insert:

```new

    /// Draws `tags` in their own colours; one out of date with the text is left out.
    pub fn set_tags(&mut self, tags: Vec<TextTag>, cx: &mut Context<Self>) {
        if self.tags != tags {
            self.tags = tags;
            cx.notify();
        }
    }

    pub fn tags(&self) -> &[TextTag] {
        &self.tags
    }
```

### H33 · `app-gpui/src/ui/text_field.rs` · replace

prepaint shapes the tags as runs of their own; without tags it is the one run of before.

Replace:

```anchor
        let run = TextRun {
            len: text.len(),
            font: font(SharedString::from(family)),
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        window.text_system().shape_line(text, theme::length(cx, "fs-ui"), &[run], None)
```

With:

```new
        let base = TextRun {
            len: 0,
            font: font(SharedString::from(family)),
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let mut runs = Vec::new();
        let mut at = 0;
        if !field.text.is_empty() {
            for tag in &field.tags {
                let (start, end) = (tag.range.start, tag.range.end.min(text.len()));
                if start < at || start >= end || !text.is_char_boundary(start) || !text.is_char_boundary(end) {
                    continue;
                }
                if start > at {
                    runs.push(TextRun { len: start - at, ..base.clone() });
                }
                let (color, background_color) = (tag.color, tag.background);
                runs.push(TextRun { len: end - start, color, background_color, ..base.clone() });
                at = end;
            }
        }
        if at < text.len() || runs.is_empty() {
            runs.push(TextRun { len: text.len() - at, ..base });
        }
        window.text_system().shape_line(text, theme::length(cx, "fs-ui"), &runs, None)
```

### H34 · `app-gpui/src/agents/cards.rs` · insert-after

a getter for the hover guard.

After:

```anchor
    pub fn hover(&mut self, position: usize, choice: Option<Choice>) {
        self.hovered = choice.map(|c| (position, c));
    }
```

Insert:

```new

    /// The choice the pointer is over, and its question's position.
    pub fn hovered(&self) -> Option<(usize, &Choice)> {
        self.hovered.as_ref().map(|(position, choice)| (*position, choice))
    }
```

### H35 · `app-gpui/src/agents/session_view.rs` · replace

the hovered answer's value.

Replace:

```anchor
                let (value, checked) = (choice.choice.clone(), choice.checked);
```

With:

```new
                let (value, checked) = (choice.choice.clone(), choice.checked);
                let hovered_choice = choice.choice.clone();
```

### H36 · `app-gpui/src/agents/session_view.rs` · replace

hovering an answer previews it.

Replace:

```anchor
                        .when(!busy, |el| {
                            el.on_click(cx.listener(move |view, _, window, cx| {
                                view.choice_clicked(position, value.clone(), checked, window, cx);
```

With:

```new
                        .on_hover(cx.listener(move |view: &mut Self, hovered: &bool, _, cx| {
                            // Moving from one answer straight to another can report the new one first: a leave
                            // clears the preview only while it is still this answer's.
                            let Some(question) = &mut view.question else { return };
                            if *hovered {
                                question.form.hover(position, Some(hovered_choice.clone()));
                            } else if question.form.hovered() == Some((position, &hovered_choice)) {
                                question.form.hover(position, None);
                            } else {
                                return;
                            }
                            cx.notify();
                        }))
                        .when(!busy, |el| {
                            el.on_click(cx.listener(move |view, _, window, cx| {
                                view.choice_clicked(position, value.clone(), checked, window, cx);
```

### H37 · `app-gpui/src/workbench/mod.rs` · replace

the event's new home.

Replace:

```anchor
        use super::agents::NewTaskEvent;
```

With:

```new
        use crate::agents::new_task_view::NewTaskEvent;
```

### H38 · `app-gpui/src/workbench/mod.rs` · replace

Start with the default agent (outside a workspace: no member).

Replace:

```anchor
        store.update(cx, |store, _| store.draft.provider.clone_from(&default));
        page.update(cx, |_, cx| cx.emit(NewTaskEvent::Start));
```

With:

```new
        store.update(cx, |store, _| store.draft.provider.clone_from(&default));
        page.update(cx, |_, cx| cx.emit(NewTaskEvent::Start { project_root: None }));
```

### H39 · `app-gpui/src/workbench/mod.rs` · replace

Start with the fake agent.

Replace:

```anchor
        store.update(cx, |store, _| store.draft.provider = "fake".to_owned());
        page.update(cx, |_, cx| cx.emit(NewTaskEvent::Start));
```

With:

```new
        store.update(cx, |store, _| store.draft.provider = "fake".to_owned());
        page.update(cx, |_, cx| cx.emit(NewTaskEvent::Start { project_root: None }));
```

### H40 · `app-gpui/src/workbench/mod.rs` · insert-after

declare the end-to-end tests.

After:

```anchor
    mod proofs13_windows;
```

Insert:

```new
    mod proofs14_agents_views;
```

### H41 · `app-gpui/src/workbench/tests/proofs13_agents_config.rs` · insert-after

append the two Model & effort tests (their helpers are this file's, identical to the pending file's).

After:

```anchor
        assert_eq!(entries(composer, cx).as_deref(), Some(["Custom *".to_owned()].as_slice()));
    });
}
```

Insert:

```new

/// The model's picker has a step per effort level the agent advertises; the last step sends the highest level, a
/// model picked from the same picker is sent too, and the picker stays open through both.
#[gpui_kit::test]
async fn steps_through_the_advertised_effort_levels_and_picks_a_model_from_the_popup(cx: &mut TestAppContext) {
    with_settings(cx, copilot(Some(LEVELS)), |Fixture { folder, composer, cx }| {
        open_chip(cx, 0);
        let listed = ["Claude Sonnet 5.5 *", "GPT-6 Sol"].map(str::to_owned);
        assert_eq!(entries(composer, cx).as_deref(), Some(listed.as_slice()), "the models are in the picker itself");
        for step in 0..LEVELS.len() {
            assert!(cx.debug_bounds(leak(format!("composer-effort-{step}"))).is_some(), "step {step}");
        }
        assert!(cx.debug_bounds(leak(format!("composer-effort-{}", LEVELS.len()))).is_none());
        click_on(cx, leak(format!("composer-effort-{}", LEVELS.len() - 1)));
        wait_for_setting(folder, "reasoning_effort=xhigh", cx);
        assert!(entries(composer, cx).is_some(), "the picker stays open after an effort step");
        choose(composer, "GPT-6 Sol", cx);
        wait_for_setting(folder, "model=gpt-6-sol", cx);
        assert!(entries(composer, cx).is_some(), "the picker stays open after a model is picked");
    });
}

/// Typing in the model picker filters its list; ↓ and Enter pick the highlighted model.
#[gpui_kit::test]
async fn filters_the_model_list_and_picks_the_highlighted_model_with_the_keyboard(cx: &mut TestAppContext) {
    with_settings(cx, copilot(Some(LEVELS)), |Fixture { folder, composer, cx }| {
        open_chip(cx, 0);
        cx.simulate_input("sol");
        cx.run_until_parked();
        assert_eq!(entries(composer, cx).map(|shown| shown.len()), Some(1), "{:?}", entries(composer, cx));
        cx.simulate_keystrokes("down");
        cx.simulate_keystrokes("enter");
        wait_for_setting(folder, "model=gpt-6-sol", cx);
    });
}
```

### H42 · `app-gpui/src/agents/session_view/tests/proofs13_agents_session.rs` · insert-after

append the question card test (needs the cards.rs and session_view.rs hunks).

After:

```anchor
    assert_eq!(store.read_with(cx, |store, _| store.scroll_positions()), 0);
}
```

Insert:

```new

/// In a question card, a number key picks that answer and its preview shows (a choice without one says so);
/// hovering another answer previews it instead; picking Other previews nothing; Escape skips the request.
#[gpui_kit::test]
async fn picks_options_with_number_keys_previews_the_picked_or_hovered_one_and_skips_with_escape(
    cx: &mut TestAppContext,
) {
    let ask = AgentEntry::Permission {
        id: 10,
        title: "Which layout?".to_owned(),
        options: vec![permission_option("reject", "Skip", "reject_once")],
        questions: vec![question("", "Which layout?", false, &[("Table", "", Some("| a | b |")), ("List", "", None)])],
        chosen: None,
    };
    let (view, _, events, cx) = open(cx, vec![local_task("A")], vec![ask]);
    let focus = view.read_with(cx, |view, _| view.question.as_ref().map(|q| q.focus.clone())).expect("a card");
    cx.update(|window, cx| window.focus(&focus, cx));
    cx.run_until_parked();
    let preview = |view: &Entity<SessionView>, cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| {
            let question = view.question.as_ref()?;
            let preview = question.form.preview(0, &I18n::default())?;
            Some((preview.caption, preview.body))
        })
    };
    cx.simulate_keystrokes("2");
    cx.run_until_parked();
    let card = view.read_with(cx, |view, cx| view.card(cx)).expect("a card");
    assert!(card.contains(&"(x) List".to_owned()), "{card:?}");
    assert!(card.contains(&"preview: List".to_owned()), "{card:?}");
    assert_eq!(preview(&view, cx), Some(("List".to_owned(), None)), "drawn as No preview");

    let table = bounds_of(cx, "session-question-0-1").center();
    cx.simulate_mouse_move(table, None, Modifiers::default());
    cx.run_until_parked();
    assert_eq!(preview(&view, cx), Some(("Table".to_owned(), Some("| a | b |".to_owned()))), "the hovered one");
    let away = bounds_of(cx, "session-question-skip").center();
    cx.simulate_mouse_move(away, None, Modifiers::default());
    cx.run_until_parked();

    cx.update(|window, cx| window.focus(&focus, cx));
    cx.simulate_keystrokes("3");
    cx.run_until_parked();
    assert_eq!(preview(&view, cx), Some(("Other".to_owned(), None)));
    cx.update(|window, cx| window.focus(&focus, cx));
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert_eq!(
        effect_calls(&events),
        [AgentCall::Respond { task: "A".to_owned(), permission: 10, option: Some("reject".to_owned()) }]
    );
}
```

## 3. Actions

No new action is registered. Every action id of `src/features/agents/index.ts` that the New Task page touches is already registered in `workbench/agents.rs`:

| Action            | Webview source                                                                                             | `when`        | What the slice adds                                                                                  | Test that runs it                                                                                                                                   |
| ----------------- | ---------------------------------------------------------------------------------------------------------- | ------------- | ---------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------- |
| `agents.newTask`  | `src/features/agents/index.ts:130`                                                                         | `projectOpen` | the page it opens is `NewTaskView`; opening it lists the agents again and reads the agent's commands | `proofs14_agents_views::*` (all three, through `run_action`)                                                                                        |
| `agents.addAgent` | `src/features/agents/index.ts:140`; chosen from the agent menu at `src/features/agents/HomeEditor.tsx:201` | none          | More agents… in the New Task agent menu runs it                                                      | `proofs14_agents_views::lists_the_agents_and_more_agents_in_the_new_task_agent_menu` (through the menu, then the Settings dialog on `tools.agents`) |

The IPC commands the page ends up calling are `agent_providers` (opening: `AgentEffect::RefreshProviders`), `agent_commands(provider, None)` (opening, and after an agent is chosen: `ide_host::ops::agent::commands`), `agent_create` (Start: `AgentEffect::Create` with the draft's settings) and, from the composer's chips, `agent_set_config` (`AgentCall::SetConfig`). The page leaves out `agent_probe`, `vcsBranchInfo` and `vcsBranches` (see section 5).

In test builds, reading the commands passes `spawn_guard::check` first, as every request that can start an agent does. It does not pass the seam: read in code, `crates/ide-agent/src/lib.rs` `commands` starts a program only for a Claude-driver agent (`fetch_settings`), and the seam counts starts that `proofs13_agents_new_task` and `a_task_with_the_default_agent_is_refused_in_a_test_build_and_starts_no_process` assert as exactly `["fake"]`.

## 4. `parity.json` keys the slice should earn

Current values were read from `docs/gpui-port/parity.json` (measured).

| Key                                                  | Now                 | Expected                              | Proof                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| ---------------------------------------------------- | ------------------- | ------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `unit:src/features/agents/ConfigPickers.test.tsx`    | partial, 7 of 13    | done, 13 of 13                        | the existing proofs, plus `agents::composer_view::pickers::tests::{steps_through_the_advertised_effort_levels_and_picks_a_model_from_the_popup, filters_the_model_list_and_picks_the_highlighted_model_with_the_keyboard, groups_nested_paths_by_their_first_provider_and_preserves_the_full_value_and_description, keeps_plain_ids_ungrouped_even_when_the_display_name_contains_a_slash, leaves_non_model_option_labels_and_descriptions_intact, shows_allow_all_inside_the_mode_popup_when_both_options_exist, edits_secondary_settings_from_the_compact_popup_and_restores_focus_when_dismissed}`, `workbench::tests::proofs13_agents_config::{steps_through_the_advertised_effort_levels_and_picks_a_model_from_the_popup, filters_the_model_list_and_picks_the_highlighted_model_with_the_keyboard}`. The slider's `max` and `aria-valuetext` are the count of `composer-effort-N` steps and the `composer-effort-value` text                                                                                                                                                                             |
| `unit:src/features/agents/HomeEditor.test.tsx`       | partial, model only | done, 4 of 4                          | `agents::new_task_view::tests::{switches_model_specific_levels_and_restores_each_draft_effort_before_starting, hides_effort_for_plain_and_unknown_models_and_sends_no_stale_effort, sends_the_displayed_remembered_model_and_effort_even_when_neither_is_changed, ignores_old_provider_wide_effort_choices_and_refreshes_unsupported_per_model_choices}`. The `%s` table is one loop. The slider's `max="2"` and `aria-valuetext="minimal"` are three steps and the chip `small · minimal`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| `unit:src/features/agents/composerCommands.test.tsx` | partial, 1 of 12    | partial, 12 of 12 with one incomplete | `agents::new_task_view::tests::{reaches_commands_after_the_first_twelve_with_the_keyboard, groups_commands_before_skills_skips_headers_wraps_and_hides_empty_groups, names_the_icon_only_start_action_and_disables_it_while_starting, completes_the_projects_skills_merged_with_the_agents_commands, tags_file_references_and_skills_where_they_stand, offers_nothing_the_project_does_not_have, completes_at_the_caret_without_replacing_the_rest_of_the_message, completes_a_command_typed_after_other_text_in_place, attaches_the_clipboards_image_when_pasting_without_text, leaves_a_text_paste_to_the_text_box, sends_the_selected_project_with_local_and_worktree_isolation}`, `agents::store::tests::defaults_to_the_active_member_last_used_member_then_first_member`, `workbench::tests::proofs14_agents_views::completes_the_commands_the_agent_offered_in_the_project`. Incomplete: the isolation case does not read the member's branches (`vcsBranchInfo`/`vcsBranches`), because the base-branch selector is not built. The WebKitGTK paste case is named after what it proves on every platform |
| `unit:src/features/agents/sessionState.test.tsx`     | partial, 29 of 31   | partial, 30 of 31                     | adds `agents::session_view::tests::proofs13_agents_session::picks_options_with_number_keys_previews_the_picked_or_hovered_one_and_skips_with_escape` (H34–H36, H42). Still missing: the docked changes panel, and keeping the first row in place when earlier rows are prepended after a drag                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| `point.registerToolWindow:agents`                    | partial             | partial                               | add `workbench::tests::proofs14_agents_views::*`, and take "the New Task page" out of the missing list. Conversations as editor tabs, Find in Conversation, queued messages, background tasks and the changes pane are still missing                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| `action:agents.addAgent`                             | done                | done                                  | add `workbench::tests::proofs14_agents_views::lists_the_agents_and_more_agents_in_the_new_task_agent_menu`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| `feature:agents`                                     | partial             | partial                               | unchanged, apart from the tool window row                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |

Not earned: `settings:tools.agents` (`INVENTORY.md`, unchecked) and `unit:src/features/agents/agentRegistry.test.tsx` (Settings › Tools › Agents is not built: section 5).

## 5. Not built

- On the New Task page: the isolation selector (New worktree / Local checkout) and the base-branch selector (`vcsBranchInfo` / `vcsBranches`). Start still sends the draft's isolation, so a project without git is not forced to the local checkout, which the webview does. Also not built: the project name outside a workspace; the agent probe line (`agent_probe`); `@file` completion and history recall (↑/↓) in the description; Shift+Tab cycling the mode; Backspace deleting a whole tag; dropping files; the longer placeholder of the undocked page.
- Differences from the webview: effort is a row of step buttons, not a slider; list entries are not highlighted on hover; closing a picker gives the keyboard to the text field, not to the chip; the Agent settings popup lists a setting's values under its row instead of a framed dropdown; the New Task popups open downwards (`PickerHost::picker_opens_down`); image paste is the ⌘V / Ctrl+V key only (Edit › Paste goes to the field's text paste).
- Settings › Tools › Agents (`agentRegistry.test.tsx`, 26 cases): not reached. The page `tools.agents` (`settings_model/builtin_pages/features.rs`) needs a custom section to host a control. The models are ready (`agents::registry::RegistryPage`, `agents::builtin_agents::BuiltinAgents`). A test must assert on the `RegistryEffect`s and never run `Install`.
- `proofs13_agents_pending.rs` (the image dialog, the docked changes) and the drag test in `proofs13_agents_session_pending.rs`: still undeclared, because their views are not built.

## 6. Hypotheses to settle first

Run the tests from `app-gpui/` (the crate is outside the workspace): `cargo test --lib <filter>`.

1. Hypothesis: these compile as written. Settle with `cargo check --tests` in `app-gpui/`:
   - `config_pickers_view::refresh` assigns `pickers.open = None` in a match arm while `open` (a `&mut` into `pickers.open`) is used in another arm's guard. NLL should accept this, because `open` is not used after the guard in that arm.
   - `chip_elements(self, cx)` is called from `render` with `self: &mut Self` against `view: &V`.
   - `#[path = "agents_views.rs"] mod views;` resolves to `workbench/agents_views.rs`.
   - The `use super::*` in `agents_views.rs` sees the parent's private imports and its `#[cfg(test)] pub(super) mod spawn_guard`.
   - `#[cfg(test)]` on the statement `spawn_guard::check(&host, &label, &asked)?;`.
   - `gpui_kit::{ClipboardEntry, Image, ImageFormat, canvas, Bounds}`, which are re-exports of GPUI's, as `workbench/project_files.rs` and `services/view.rs` already use some of them.
2. Hypothesis: `agent_create` sends a model chosen on the New Task page to the new session with `session/set_config_option`, which the fake agent logs. Settle with `cargo test --lib proofs14_agents_views::starts_the_next_task_with_the_model_picked_on_the_new_task_page`.
3. Read in code (`crates/ide-agent/src/lib.rs`, `"available_commands_update"` then `shared.remember(.., Some(&commands), ..)`, and `commands` with no task reading `known_commands`): a session's offered commands become the project's commands for the agent. Settle with `cargo test --lib proofs14_agents_views::completes_the_commands_the_agent_offered_in_the_project`.
4. Read in code (`ui/popup_menu.rs` `lines` and `menu-row-{depth}-{index}`, both counting separators): the menu row selector indexes match the `lines()` positions the test computes. Settle with `cargo test --lib proofs14_agents_views::lists_the_agents_and_more_agents_in_the_new_task_agent_menu`.
5. Hypothesis: a click inside an inline popup keeps the popup's keyboard. Read in GPUI (`elements/div.rs` `paint_mouse_listeners`): the popup's own `stop_propagation` runs before its `track_focus` listener and before every parent's, so focus does not move on a press inside it. Settle with `cargo test --lib composer_view::pickers::tests::edits_secondary_settings_from_the_compact_popup_and_restores_focus_when_dismissed`.
6. Hypothesis: `simulate_mouse_move` onto a question row fires its `on_hover`. Settle with `cargo test --lib proofs13_agents_session::picks_options_with_number_keys_previews_the_picked_or_hovered_one_and_skips_with_escape`.

## 7. Tests most likely to fail first

- `proofs14_agents_views::starts_the_next_task_with_the_model_picked_on_the_new_task_page`: hypothesis 2, and the chips wait for `AgentProvider::config` to be filled once the agents are listed again.
- `proofs14_agents_views::completes_the_commands_the_agent_offered_in_the_project`: hypothesis 3; the page also reads the commands only once, when it opens.
- `proofs13_agents_config::filters_the_model_list_and_picks_the_highlighted_model_with_the_keyboard` (moved): it types into the search field, which takes the keyboard on the chip's click, inside the tool window. A parent's `track_focus` on the same press could take it back (rule: "a view that takes focus inside a mouse-down handler loses it…"). Here the focus is set in `on_click` (on release), which should be safe.
- `new_task_view::tests::{attaches_the_clipboards_image_when_pasting_without_text, leaves_a_text_paste_to_the_text_box}` on a runner that is not macOS: the tests press `cmd-v`, and `Modifiers::secondary` is Ctrl there.
- The New Task popups open downwards (`top(relative(1.0))`). In a short tool window the model list can fall below the window's bottom, and `click_on` then misses `composer-picker-N`.

## 8. Risks that do not fail a test

- When `refresh` closes the Model & effort popup (a model without effort levels was picked), the keyboard stays on the search field, which is no longer drawn. The webview loses focus the same way when its popup unmounts. Not fixed: `refresh` runs from a store observer with no window to move the focus in.
- `spawn_guard::check` logs an error each time a New Task page opens with the default `claude` draft in a test that does not install the fake agent. This is noise only: the answer is a logged warning, and the page falls back to the agent's last commands.
- `workspace_members` calls `ide_host::ops::project::current` on the main thread, as `workbench/app_shell.rs` already does. It is a lookup and clones the project's info.
- No secret is involved: the fake agent's `FAKE_AGENT_API_KEY` is never read by these views, and no `Debug` output of theirs holds the providers' `env`.

## 9. Pending tests

- Moved: the two `proofs13_agents_config_pending` tests go to `proofs13_agents_config.rs` (H41). The draft provides what they waited for: the Model & effort popup stays open through its choices and has a search field. The pending file is deleted.
- Moved: `picks_options_with_number_keys_previews_the_picked_or_hovered_one_and_skips_with_escape` goes to `proofs13_agents_session.rs` (H42). It waits for H34–H36, not for a draft file.
- Left: `defers_prepending_during_a_pointer_drag_and_preserves_the_first_rows_viewport_position` (`proofs13_agents_session_pending.rs`) waits for the conversation to hold back prepending during a drag and to keep the first row's position. `opens_an_image_of_a_sent_message_in_a_dialog` and `keeps_docked_changes_opt_in_preserves_the_draft_and_returns_to_the_same_conversation` (`proofs13_agents_pending.rs`) wait for the image dialog and the docked changes panel.
