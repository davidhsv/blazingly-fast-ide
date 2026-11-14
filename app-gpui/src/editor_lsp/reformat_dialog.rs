//! Code › Reformat File… (⌥⇧⌘L): the scope (whole file or selected text) and the options (Optimize
//! imports) of one Reformat Code, then Run. The choices are remembered for the next time the
//! dialog opens in this run of the IDE; "Selected text" is offered only while there is a selection,
//! and a choice made without one leaves the remembered scope as it was.

use crate::platform::i18n::{Localization, Param};
use crate::platform::input;
use crate::theme;
use crate::vcs::prompt_dialogs::{button, checkbox};
use gpui_kit::{
    App, ClickEvent, Context, Div, EventEmitter, FocusHandle, Focusable, FontWeight, IntoElement, Keystroke,
    MouseButton, Render, SharedString, Window, div, prelude::*, px,
};

const WIDTH: f32 = 380.0;

/// What Reformat File… runs with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReformatOptions {
    /// Only the selected text, not the whole file.
    pub selection_only: bool,
    /// Optimize Imports before reformatting.
    pub optimize_imports: bool,
}

impl Default for ReformatOptions {
    fn default() -> Self {
        Self { selection_only: true, optimize_imports: false }
    }
}

/// The options Reformat File… was last run with, for the rest of this run of the IDE.
#[derive(Debug, Clone, Copy, Default)]
pub struct LastReformatOptions(pub ReformatOptions);

impl gpui_kit::Global for LastReformatOptions {}

impl LastReformatOptions {
    /// What is remembered after a run with `chosen`: a run without a selection keeps the
    /// remembered scope.
    pub fn after_run(self, chosen: ReformatOptions, had_selection: bool) -> Self {
        let selection_only = if had_selection { chosen.selection_only } else { self.0.selection_only };
        Self(ReformatOptions { selection_only, optimize_imports: chosen.optimize_imports })
    }
}

/// What the dialog asks of its owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReformatDialogEvent {
    /// Run: close the dialog and reformat with these options.
    Run(ReformatOptions),
    /// Cancel or Escape.
    Cancelled,
}

pub struct ReformatDialog {
    title: String,
    has_selection: bool,
    can_optimize_imports: bool,
    selection_only: bool,
    optimize_imports: bool,
    focus: FocusHandle,
    key_claim: u64,
}

impl EventEmitter<ReformatDialogEvent> for ReformatDialog {}

impl ReformatDialog {
    /// The dialog for the file named `name`, starting from the `initial` options: "Selected text"
    /// only with a selection, Optimize imports only where it can run.
    pub fn new(
        name: &str,
        has_selection: bool,
        can_optimize_imports: bool,
        initial: ReformatOptions,
        cx: &mut Context<Self>,
    ) -> Self {
        let title = cx.default_global::<Localization>().0.tx_built(&["Reformat File: ", ""], &[Param::from(name)]);
        let focus = cx.focus_handle();
        let claimant = cx.weak_entity();
        let key_claim = input::claim_keys(
            focus.clone(),
            move |keystroke, _, cx| claimant.update(cx, |dialog, cx| dialog.claim_key(keystroke, cx)).unwrap_or(false),
            cx,
        );
        cx.on_release(|dialog, cx| input::release_keys(dialog.key_claim, cx)).detach();
        Self {
            title,
            has_selection,
            can_optimize_imports,
            selection_only: has_selection && initial.selection_only,
            optimize_imports: can_optimize_imports && initial.optimize_imports,
            focus,
            key_claim,
        }
    }

    /// The title, in the UI language.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// The options as chosen now.
    pub fn options(&self) -> ReformatOptions {
        ReformatOptions { selection_only: self.selection_only, optimize_imports: self.optimize_imports }
    }

    /// Whether "Selected text" can be chosen.
    pub fn offers_selection(&self) -> bool {
        self.has_selection
    }

    /// Whether Optimize imports can be chosen.
    pub fn offers_optimize_imports(&self) -> bool {
        self.can_optimize_imports
    }

    /// Chooses the scope; "Selected text" only while there is a selection.
    pub fn set_selection_only(&mut self, selection_only: bool, cx: &mut Context<Self>) {
        if selection_only && !self.has_selection {
            return;
        }
        self.selection_only = selection_only;
        cx.notify();
    }

    pub fn toggle_optimize_imports(&mut self, cx: &mut Context<Self>) {
        if !self.can_optimize_imports {
            return;
        }
        self.optimize_imports = !self.optimize_imports;
        cx.notify();
    }

    /// Run: the options go to the owner.
    pub fn run(&mut self, cx: &mut Context<Self>) {
        cx.emit(ReformatDialogEvent::Run(self.options()));
    }

    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        cx.emit(ReformatDialogEvent::Cancelled);
    }

    /// Enter runs, Escape cancels, ahead of the keymap.
    fn claim_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        if keystroke.modifiers.modified() {
            return false;
        }
        match keystroke.key.as_str() {
            "enter" => self.run(cx),
            "escape" => self.cancel(cx),
            _ => return false,
        }
        true
    }
}

impl Focusable for ReformatDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// A radio button with its label, greyed out when it cannot be chosen.
fn radio(cx: &App, label: String, on: bool, enabled: bool) -> Div {
    let dot = on.then(|| div().size(px(6.0)).rounded_full().bg(theme::color(cx, "fg-button-primary")));
    let row = div()
        .flex_none()
        .flex()
        .items_center()
        .gap(px(8.0))
        .text_color(theme::color(cx, "fg-strong"))
        .whitespace_nowrap()
        .child(
            div()
                .flex_none()
                .size(px(16.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .border_1()
                .border_color(theme::color(cx, if on { "bg-button-primary" } else { "border-checkbox" }))
                .bg(theme::color(cx, if on { "bg-button-primary" } else { "bg-checkbox" }))
                .children(dot),
        )
        .child(SharedString::from(label));
    if enabled { row } else { row.opacity(0.5) }
}

/// A group's heading.
fn legend(cx: &App, text: String) -> Div {
    div().text_color(theme::color(cx, "fg-muted")).child(SharedString::from(text))
}

impl Render for ReformatDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let i18n = &cx.default_global::<Localization>().0;
        let tx = |text: &str| i18n.tx(text);
        let (scope, whole, selected, options, optimize, run, cancel) = (
            tx("Scope"),
            tx("Whole file"),
            tx("Selected text"),
            tx("Options"),
            tx("Optimize imports"),
            tx("Run"),
            tx("Cancel"),
        );
        let (selection_only, has_selection) = (self.selection_only, self.has_selection);
        let whole = radio(cx, whole, !selection_only, true)
            .id("reformat-whole-file")
            .debug_selector(|| "reformat-whole-file".to_owned())
            .on_click(cx.listener(|dialog, _: &ClickEvent, _, cx| dialog.set_selection_only(false, cx)));
        let selected = radio(cx, selected, selection_only, has_selection)
            .id("reformat-selected-text")
            .debug_selector(|| "reformat-selected-text".to_owned())
            .on_click(cx.listener(|dialog, _: &ClickEvent, _, cx| dialog.set_selection_only(true, cx)));
        let optimize = checkbox(cx, optimize, self.optimize_imports)
            .when(!self.can_optimize_imports, |el| el.opacity(0.5))
            .id("reformat-optimize-imports")
            .debug_selector(|| "reformat-optimize-imports".to_owned())
            .on_click(cx.listener(|dialog, _: &ClickEvent, _, cx| dialog.toggle_optimize_imports(cx)));
        let run = button(cx, run, true, false, true)
            .id("reformat-run")
            .debug_selector(|| "reformat-run".to_owned())
            .on_click(cx.listener(|dialog, _: &ClickEvent, _, cx| dialog.run(cx)));
        let cancel = button(cx, cancel, false, false, true)
            .id("reformat-cancel")
            .debug_selector(|| "reformat-cancel".to_owned())
            .on_click(cx.listener(|dialog, _: &ClickEvent, _, cx| dialog.cancel(cx)));
        let group = |heading: Div| div().flex().flex_col().gap(px(8.0)).child(heading);
        div()
            .w(px(WIDTH))
            .flex()
            .flex_col()
            .bg(theme::color(cx, "bg-popup"))
            .border_1()
            .border_color(theme::color(cx, "border-popup"))
            .rounded(theme::length(cx, "radius-popup"))
            .text_size(theme::length(cx, "fs-ui"))
            .text_color(theme::color(cx, "fg"))
            .track_focus(&self.focus)
            // A click on the dialog is not a click beside it.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .px(px(16.0))
                    .pt(px(14.0))
                    .pb(px(8.0))
                    .text_color(theme::color(cx, "fg-strong"))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(SharedString::from(self.title.clone())),
            )
            .child(
                div()
                    .px(px(16.0))
                    .py(px(4.0))
                    .flex()
                    .flex_col()
                    .gap(px(14.0))
                    .child(group(legend(cx, scope)).child(whole).child(selected))
                    .child(group(legend(cx, options)).child(optimize)),
            )
            .child(div().flex().justify_end().gap(px(8.0)).px(px(16.0)).py(px(12.0)).child(run).child(cancel))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{Entity, Modifiers, TestAppContext, VisualTestContext};
    use std::cell::RefCell;
    use std::rc::Rc;

    type Events = Rc<RefCell<Vec<ReformatDialogEvent>>>;

    fn open(
        cx: &mut TestAppContext,
        has_selection: bool,
        initial: ReformatOptions,
    ) -> (Entity<ReformatDialog>, Events, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let (dialog, cx) =
            cx.add_window_view(move |_, cx| ReformatDialog::new("Main.java", has_selection, true, initial, cx));
        let events: Events = Rc::default();
        let log = Rc::clone(&events);
        cx.update(|window, cx| {
            window.activate_window();
            cx.subscribe(&dialog, move |_, event: &ReformatDialogEvent, _| log.borrow_mut().push(*event)).detach();
            let focus = dialog.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        (dialog, events, cx)
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        cx.run_until_parked();
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.run_until_parked();
    }

    /// The dialog names the file, starts from the remembered options, and with a selection offers
    /// Selected text; Enter runs with what is chosen.
    #[gpui_kit::test]
    async fn runs_with_the_scope_and_options_chosen(cx: &mut TestAppContext) {
        let (dialog, events, cx) = open(cx, true, ReformatOptions::default());
        assert_eq!(dialog.read_with(cx, |d, _| d.title().to_owned()), "Reformat File: Main.java");
        assert_eq!(dialog.read_with(cx, |d, _| d.options()), ReformatOptions::default());
        click(cx, "reformat-whole-file");
        click(cx, "reformat-optimize-imports");
        cx.simulate_keystrokes("enter");
        let chosen = ReformatOptions { selection_only: false, optimize_imports: true };
        assert_eq!(*events.borrow(), [ReformatDialogEvent::Run(chosen)]);
    }

    /// Without a selection the scope is the whole file and Selected text cannot be chosen; Escape
    /// cancels.
    #[gpui_kit::test]
    async fn without_a_selection_only_the_whole_file_is_offered(cx: &mut TestAppContext) {
        let (dialog, events, cx) = open(cx, false, ReformatOptions::default());
        assert!(!dialog.read_with(cx, |d, _| d.offers_selection()));
        assert!(!dialog.read_with(cx, |d, _| d.options().selection_only));
        click(cx, "reformat-selected-text");
        assert!(!dialog.read_with(cx, |d, _| d.options().selection_only));
        cx.simulate_keystrokes("escape");
        assert_eq!(*events.borrow(), [ReformatDialogEvent::Cancelled]);
    }

    /// A run without a selection keeps the remembered scope; one with a selection remembers its
    /// choice; Optimize imports is remembered either way.
    #[test]
    fn remembers_the_scope_only_from_a_run_with_a_selection() {
        let last = LastReformatOptions(ReformatOptions { selection_only: true, optimize_imports: false });
        let whole = ReformatOptions { selection_only: false, optimize_imports: true };
        assert_eq!(last.after_run(whole, false).0, ReformatOptions { selection_only: true, optimize_imports: true });
        assert_eq!(last.after_run(whole, true).0, whole);
    }
}
