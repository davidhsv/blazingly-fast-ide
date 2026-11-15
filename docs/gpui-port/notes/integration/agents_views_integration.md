# Integrating the Agents views: settings pickers, the New Task page, the question hover preview

Nothing here was compiled or run. Every statement about runtime behaviour is a hypothesis until the integrator's run. Every anchor below is quoted from the file as it is now (line numbers left out on purpose).

This file replaces the first-round version: the pickers moved from `composer_view/pickers.rs` into a module of their own, shared by the composer and the New Task page, so section 1's hunks are new.

## 0. New files

| File                                                    | What                                                                                                                                                                                                      |
| ------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `app-gpui/src/agents/config_pickers_view.rs`            | The settings chips, their pickers and the Agent settings popup for any view (`PickerHost` trait, `Pickers` state, free generic functions)                                                                 |
| `app-gpui/src/agents/composer_view/pickers.rs`          | `impl PickerHost for ComposerView`, the composer's accessors (`chips`, `picker_entries`, `picker_details`, `picker_query`, `settings_title`, `settings_rows`, `settings_button_focused`) and 8 view tests |
| `app-gpui/src/agents/new_task_view.rs`                  | `NewTaskView` (replaces `workbench::agents::NewTaskPage`) and `NewTaskEvent`                                                                                                                              |
| `app-gpui/src/agents/new_task_view/tests.rs`            | 17 view tests (HomeEditor and composerCommands cases)                                                                                                                                                     |
| `app-gpui/src/workbench/agents_views.rs`                | `impl Workbench`: the page opened, its events, the agent menu, reading the project's commands. Declared as a child of `workbench::agents` (section 3b) so it uses that module's private runner and store  |
| `app-gpui/src/workbench/tests/proofs14_agents_views.rs` | 3 end-to-end tests through the window with the fake agent                                                                                                                                                 |

## 1. `app-gpui/src/agents/mod.rs`

After `pub mod config_pickers;` add `pub mod config_pickers_view;`. After `pub mod model;` add `pub mod new_task_view;`.

## 2. `app-gpui/src/agents/composer_view.rs`

### 2a. Imports

Replace

```rust
use crate::agents::config_bar::{
    Chip, ListKey, OptionList, Picker, Values, config_bar, effort_slider, effort_step, mode_chip, model_chip,
    select_icon, switch_choice, value_of,
};
use crate::agents::config_pickers::switch_values;
```

with

```rust
use crate::agents::config_pickers_view::{self, Pickers};
```

replace

```rust
use gpui_kit::{
    AnyElement, App, Context, Div, Entity, EntityInputHandler, EventEmitter, FocusHandle, Focusable, IntoElement,
    KeyDownEvent, Keystroke, MouseButton, Render, SharedString, Stateful, Subscription, Window, deferred, div,
    prelude::*, px,
};
use ide_agent::{AgentConfigOption, AgentTask};
```

with

```rust
use gpui_kit::{
    AnyElement, App, Context, Div, Entity, EntityInputHandler, EventEmitter, FocusHandle, Focusable, IntoElement,
    Keystroke, Render, SharedString, Stateful, Subscription, Window, div, prelude::*, px,
};
use ide_agent::AgentTask;
```

### 2b. The module

Before `/// How a line break of the message is shown, and typed, in the one-line field.` add `mod pickers;` and a blank line.

### 2c. Delete the chip code that moved

Delete everything from the line `/// A chip of the settings row.` down to (not including) `pub struct ComposerView {`: `struct ConfigChip`, `impl ConfigChip`, `struct OpenPicker`, `fn find`, `fn chips`. They live in `config_pickers_view.rs` now.

### 2d. The field

In `pub struct ComposerView`, replace

```rust
    picker: Option<OpenPicker>,
    picker_focus: FocusHandle,
```

with

```rust
    /// The settings chips' pickers and the Agent settings popup.
    pickers: Pickers,
```

### 2e. The store observer

Replace

```rust
            cx.observe(&store, |view, _, cx| {
                view.sync_field(cx);
```

with

```rust
            cx.observe(&store, |view, _, cx| {
                view.sync_field(cx);
                config_pickers_view::refresh(view, cx);
```

### 2f. Releasing the claims

Replace

```rust
        cx.on_release(|view: &mut Self, cx| input::release_keys(view.claim, cx)).detach();
```

with

```rust
        cx.on_release(|view: &mut Self, cx| {
            input::release_keys(view.claim, cx);
            view.pickers.release(cx);
        })
        .detach();
```

### 2g. The initial value

In the `Self { … }` of `ComposerView::new`, replace

```rust
            picker: None,
            picker_focus: cx.focus_handle(),
```

with

```rust
            pickers: Pickers::new(cx),
```

### 2h. Delete the accessors that moved

Delete from `   /// The settings chips, as`<label>`with` · <detail>`, ` [on]`/` [off]` for a switch.` down to (not including) `    // ----- what keys and clicks do -----`: `pub fn chips`, `pub fn picker_entries`, `fn config_chips`. `pickers.rs` defines `chips` and `picker_entries` with the same signatures and output.

### 2i. Escape in the message field

In `fn field_key`, replace

```rust
            "escape" if self.picker.is_some() => self.close_picker(cx),
```

with

```rust
            "escape" if self.pickers.is_open() => config_pickers_view::close_popups(self, window, cx),
```

### 2j. Delete the old handlers and drawing

Delete from `    fn chip_clicked(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {` down to (not including) `    // ----- drawing -----` (`chip_clicked`, `close_picker`, `entry_clicked`, `picker_key`), and delete `    fn picker_element(&self, cx: &mut Context<Self>) -> Option<AnyElement> {` through its closing `    }` (the last method before `impl Focusable for ComposerView {`). Keep `set_config`, `chip_button`, `task_of`, `tx`.

### 2k. Render

In `impl Render for ComposerView`, replace the block from

```rust
        let muted = theme::color(cx, "fg-muted");
        let mut chips: Vec<AnyElement> = Vec::new();
        for (index, chip) in self.config_chips(cx).into_iter().enumerate() {
```

through its closing

```rust
            chips.push(button.into_any_element());
        }
```

with

```rust
        let chips = config_pickers_view::chip_elements(self, cx);
```

(`chip_elements` ends with the Agent settings button.) Replace

```rust
            .children(self.picker_element(cx))
```

with

```rust
            .children(config_pickers_view::picker_element(self, cx))
            .children(config_pickers_view::settings_element(self, cx))
```

## 3. `app-gpui/src/workbench/agents.rs` (the New Task page is replaced)

### 3a. Imports

Replace `use crate::agents::composer::{LineRange, can_send, mention_of};` with `use crate::agents::composer::{LineRange, mention_of};`. After `use crate::agents::markdown::LinkAction;` add `use crate::agents::new_task_view::NewTaskView;`. Delete `use crate::theme;` and `use crate::ui::text_field::{TextField, TextFieldEvent};`. Replace

```rust
use gpui_kit::{
    AnyView, AnyWindowHandle, App, BorrowAppContext, ClipboardItem, Context, Entity, EntityId, EventEmitter,
    FocusHandle, Focusable, IntoElement, MouseButton, MouseDownEvent, Pixels, Point, Render, SharedString,
    Subscription, Task, Window, div, prelude::*, px,
};
```

with

```rust
use gpui_kit::{
    AnyView, AnyWindowHandle, App, BorrowAppContext, ClipboardItem, Context, Entity, EntityId, Focusable,
    Subscription, Task, Window, prelude::*,
};
```

(I counted the uses outside the deleted section: none of `TextField`, `TextFieldEvent`, `MouseButton`, `MouseDownEvent`, `Render`, `IntoElement`, `EventEmitter`, `FocusHandle`, `SharedString`, `div`, `px`, `Pixels`, `Point`, `theme::`, `can_send`. Hypothesis: `prelude::*` is still needed for `cx.new`/`cx.spawn` traits; if rustc calls it unused, drop it.)

### 3b. The child module

After `const SESSION_FOCUS: &str = "agentSessionFocus";` add

```rust

/// The window's side of the New Task page.
#[path = "agents_views.rs"]
mod views;
```

(`#[path]` on a module declared in `workbench/agents.rs` resolves against `workbench/`, so the file is `workbench/agents_views.rs`, and the module is `workbench::agents::views`.)

### 3c. The slot's page

Replace `    home: Option<(Entity<NewTaskPage>, Subscription)>,` with `    home: Option<(Entity<NewTaskView>, Subscription)>,`.

### 3d. Creating the page

In `pub(super) fn sync_agents_window`, replace

```rust
            let page = cx.new(|cx| NewTaskPage::new(store.clone(), cx));
            let events = cx.subscribe(&page, Self::agents_home_event);
            slot.home = Some((page, events));
```

with

```rust
            let page = cx.new(|cx| NewTaskView::new(store.clone(), cx));
            let events = cx.subscribe(&page, Self::agents_home_event);
            slot.home = Some((page.clone(), events));
            // Once this update is over (the slot is borrowed here): the page learns the workspace's members,
            // the agents are listed again and the agent's commands are read.
            cx.spawn(async move |this, cx| {
                let _ = this.update(cx, |workbench, cx| workbench.agents_home_opened(&page, cx));
            })
            .detach();
```

### 3e. Delete the old event handler

Delete from `    /// Carries out what the New Task page asks for.` through the closing `    }` of `fn agents_home_event(&mut self, page: Entity<NewTaskPage>, event: &NewTaskEvent, cx: &mut Context<Self>) {` (just before `    /// The New Task page's agent: its settings start over.`). `agents_views.rs` defines `pub(super) fn agents_home_event` with the new events; keep `fn choose_task_agent`, which it calls.

### 3f. The test accessor

Replace `    pub(super) fn agents_home_page(&self) -> Option<Entity<NewTaskPage>> {` with `    pub(super) fn agents_home_page(&self) -> Option<Entity<NewTaskView>> {`.

### 3g. Delete the old page

Delete from

```rust
// -------------------------------------------------------------------------------------------------
// The New Task page
```

to the end of the file (`enum NewTaskEvent`, `struct NewTaskPage`, its impls). Callers kept working: `page.finished(outcome.err(), cx)` (Purpose::Create), `page.focus_handle(cx)`, `page.clone().into()` (AnyView), and the tests' `page.can_start(cx)`, `page.error()`, `page.focus_handle(cx)`, the selectors `agents-home-agent`, `agents-home-start`, `agents-home-error`.

## 4. `app-gpui/src/ui/text_field.rs` (tags drawn in the text)

### 4a. The type

Before `pub struct TextField {` add

```rust
/// A run of a text field's text drawn in its own colours, such as an owner's tag for a file reference.
#[derive(Clone, Debug, PartialEq)]
pub struct TextTag {
    /// Bytes of the text.
    pub range: Range<usize>,
    pub color: Hsla,
    pub background: Option<Hsla>,
}

```

### 4b. The field

Replace

```rust
    /// The steps Undo and Redo walk.
    history: TextHistory,
}
```

with

```rust
    /// The steps Undo and Redo walk.
    history: TextHistory,
    /// Runs drawn in their own colours, in order and apart; the owner keeps them up to date with the text.
    tags: Vec<TextTag>,
}
```

and in `TextField::new` replace

```rust
            history: TextHistory::default(),
        }
```

with

```rust
            history: TextHistory::default(),
            tags: Vec::new(),
        }
```

### 4c. The methods

After

```rust
    pub fn is_read_only(&self) -> bool {
        self.read_only
    }
```

add

```rust

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

### 4d. Painting the runs

In `fn prepaint` of `impl Element for TextFieldElement`, replace

```rust
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

with

```rust
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

Without tags (every other field) the runs are the one run of before.

## 5. `app-gpui/src/agents/cards.rs` (getter for the hover guard)

After

```rust
    pub fn hover(&mut self, position: usize, choice: Option<Choice>) {
        self.hovered = choice.map(|c| (position, c));
    }
```

add

```rust

    /// The choice the pointer is over, and its question's position.
    pub fn hovered(&self) -> Option<(usize, &Choice)> {
        self.hovered.as_ref().map(|(position, choice)| (*position, choice))
    }
```

## 6. `app-gpui/src/agents/session_view.rs` (question card hover preview)

In `fn question_element`, replace

```rust
                let (value, checked) = (choice.choice.clone(), choice.checked);
```

with

```rust
                let (value, checked) = (choice.choice.clone(), choice.checked);
                let hovered_choice = choice.choice.clone();
```

and replace

```rust
                        .when(!busy, |el| {
                            el.on_click(cx.listener(move |view, _, window, cx| {
                                view.choice_clicked(position, value.clone(), checked, window, cx);
```

with

```rust
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

## 7. `mod` lines for tests

- `app-gpui/src/workbench/mod.rs`, inside `mod tests {`: after `    mod proofs13_agents_config;` add `    mod proofs13_agents_config_pending;` (needs section 2); after `    mod proofs13_windows;` add `    mod proofs14_agents_views;` (needs sections 1–4).
- `proofs13_agents_session_pending.rs`: one of its two tests passes with sections 5–6, the other does not. Move `picks_options_with_number_keys_previews_the_picked_or_hovered_one_and_skips_with_escape` (verbatim, with its `use std::cell::Cell` not needed: it uses only `open`, `effect_calls`, `bounds_of`, `permission_option`, `question`, `local_task`, `I18n`, `AgentEntry`, `AgentCall`, `Modifiers` from the parent `mod tests`) into the declared `app-gpui/src/agents/session_view/tests/proofs13_agents_session.rs`, and leave `defers_prepending_during_a_pointer_drag_…` alone in the undeclared pending file. Hypothesis: `proofs13_agents_session.rs` already has those names through `use super::*;`; add a `use` if one is missing.
- `proofs13_agents_pending.rs` stays undeclared: neither of its views (image dialog, docked changes) was built.

## 8. Wiring of the New Task page (`agents_views.rs`)

| What the page does                     | The window                                                                                                                                                                                                                                                 | Backend (IPC)                                       |
| -------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------- |
| Opens                                  | `agents_home_opened`: `set_projects` from `ide_host::ops::project::current(..).workspace` with `store::default_task_project` (active file, the draft's last member, the first); `AgentEffect::RefreshProviders`; `load_home_commands`                      | `agent_providers`, `agent_commands(provider, None)` |
| `NewTaskEvent::Start { project_root }` | `store.start_task(None, project_root)` → `AgentEffect::Create`                                                                                                                                                                                             | `agent_create`                                      |
| `NewTaskEvent::ChooseAgent(position)`  | the menu of `registry::provider_options` (installed agents with registry version/update, "Not installed" with their commands, disabled, then "More agents…" running `agents.addAgent`); a choice runs `choose_task_agent` then reads that agent's commands | `agent_commands`                                    |
| `NewTaskEvent::Effects`                | `carry_out_agents` (a setting saved: `Save { DRAFT_KEY }`; an image over 5 MB: `Notify`)                                                                                                                                                                   | —                                                   |
| Settings chip choice                   | `PickerHost::picker_choose` → `store.set_draft_option` (thinking level per model)                                                                                                                                                                          | — (sent with `agent_create`)                        |

In tests, reading the commands passes `spawn_guard::check` first: `agent_commands` without a task can start a Claude-driver agent's program (`crates/ide-agent/src/lib.rs` `commands` → `fetch_settings`). No action is registered.

## 9. The waiting tests

- `proofs13_agents_config_pending` (both): Hypothesis: pass with section 2.
- `proofs13_agents_session_pending::picks_options_with_number_keys_previews_the_picked_or_hovered_one_and_skips_with_escape`: Hypothesis: passes with sections 5–6 (`session-question-0-1` is the first option: the row selector uses the 1-based `choice.key`).
- `proofs13_agents_session_pending::defers_prepending_during_a_pointer_drag_…`: not built; stays failing.
- `proofs13_agents_pending` (both): not built.

## 10. Spec files after this work (if the tests pass)

- `ConfigPickers.test.tsx`: 13 of 13 (cases 1, 2, 4, 9 by `composer_view::pickers::tests`, 11 and 12 also by `proofs13_agents_config_pending`).
- `HomeEditor.test.tsx`: 4 of 4 by `new_task_view::tests` (the `%s` case is one loop over plain and unknown); the effort control is steps, not a slider with `max`/`aria-valuetext`.
- `composerCommands.test.tsx`: 12 of 12. Cases 1–8, 9 (as "attaches the clipboard's image when pasting without text"), 10 and 12 by `new_task_view::tests`, 3 also by `proofs13_agents_new_task`; case 11 is a function test in the webview too, and `store::tests::defaults_to_the_active_member_last_used_member_then_first_member` covers it (read). The page receives that default through `agents_home_opened`, which no test drives in a workspace.
- `sessionState.test.tsx`: case 19 complete; still 29 of 31 (10 and 27 left).
- `agentRegistry.test.tsx`: see section 12.

## 11. Behaviour not built, differences, risks (items 1–2)

Not built on the New Task page: the isolation selector (New worktree / Local checkout) and the base-branch selector (`vcsBranchInfo`/`vcsBranches`); Start still passes the draft's isolation, so a project without git is not forced to the local checkout (the webview forces it); the project name outside a workspace; the agent probe line (`agent_probe`); `@file` completion and history recall (↑/↓) in the description; Shift+Tab cycling the mode; Backspace deleting a whole tag; dropping files.

Differences: effort is step buttons; list entries are not highlighted by hover; closing a picker gives the keyboard to the text field, not the chip; the Agent settings popup lists values under the row; the New Task popups open downwards (`PickerHost::picker_opens_down`); paste is the ⌘V/Ctrl+V key only (Edit › Paste from the menu goes to the field's text paste).

Compile risks (hypotheses): `gpui_kit::{ClipboardEntry, Image, ImageFormat}` are re-exports of GPUI's (only `ClipboardItem` was used in the crate before); `.top(relative(1.0))` takes a `DefiniteLength`; `ScrollHandle::scroll_to_item(usize)` and `.track_scroll` on a `Stateful<Div>` (read in `gpui-pre-0.3.12/src/elements/div.rs`); the `match` in `config_pickers_view::refresh` assigns `pickers.open` in an arm while `open` borrows it in another arm's guard; `#[cfg(test)]` on the statement `spawn_guard::check(..)?;` in `load_home_commands`.

Most likely to fail first:

- `proofs14_agents_views::completes_the_commands_the_agent_offered_in_the_project`: Hypothesis: the backend remembers a session's `available_commands_update` as the project's commands for the agent (`known_commands`); if not, the page has none.
- `proofs14_agents_views::starts_the_next_task_with_the_model_picked_on_the_new_task_page`: Hypothesis: `AgentProvider::config` holds the first session's settings once the agents are listed again, and `agent_create` sends the chosen ones with `session/set_config_option`.
- `proofs14_agents_views::lists_the_agents_and_more_agents_…`: Hypothesis: the menu's row selectors count separators (`menu-row-0-{line index}`), as `proofs11_agents::choose_in_menu` assumes.
- The `cmd-v` tests on a non-macOS runner (the key is Ctrl+V there).

## 12. Item 3 (Settings › Tools › Agents): not reached

0 of the 26 cases of `agentRegistry.test.tsx` are covered by a view of mine. What the next writer needs, read in the code:

- The page `tools.agents` (`settings_model/builtin_pages/features.rs`) is a bare `PageSpec` with no section, so the dialog has no slot to host a control: it needs `.section(None, vec![custom_owned("Agents", vec![…keywords])])` there (as `behavior.rs` does for "Popup type"), then `slots.put("tools.agents", "Agents", &view, cx)` in `settings_view/custom_controls.rs::install`.
- The models are ready: `agents::registry::RegistryPage` (no I/O; `RegistryEffect::{List, Install, Remove, Cancel, Confirm, PollEvery, Changed}` with `RequestId`s), `agents::builtin_agents::BuiltinAgents` (rows, details, `set_command`/`set_env` stage into `SettingsDialog`, `external_write`/`stored_value` on Apply).
- Tests must assert on the `RegistryEffect`s a view emits (never run `Install`): the test build's `spawn_guard` covers agents started, not registry downloads.
- The New Task agent menu already shows `registry::provider_options` (versions, updates, More agents…), so case 5 ("shows registry versions and updates, then Add Agent…") has its menu half through `proofs14_agents_views::lists_the_agents_and_more_agents_in_the_new_task_agent_menu` (installed agent first, More agents… last, opening Settings on `tools.agents`); the version/update text is not asserted (the fake agent is not a registry agent).
