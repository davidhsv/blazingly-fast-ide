//! File › Invalidate Caches…: the symbol index is always rebuilt; three checkboxes add Local
//! History, the language server workspaces and the build tool caches. Invalidate and Restart and
//! Just Restart close the dialog and say what to carry out; Cancel only closes it.

use super::{button, checkbox, footer, frame, title_row, tx};
use crate::app_models::invalidate_caches::{CachesEffect, InvalidateCachesDialog as Choices, just_restart};
use crate::platform::input;
use crate::theme;
use gpui_kit::{
    App, ClickEvent, Context, EventEmitter, FocusHandle, Focusable, FontWeight, IntoElement, Keystroke, Render,
    SharedString, Window, div, prelude::*, px,
};

const WIDTH: f32 = 480.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvalidateCachesEvent {
    /// Carry these out, in order (the dialog is done).
    Run([CachesEffect; 2]),
    Cancelled,
}

/// The optional caches, by the id of their checkbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionalCache {
    LocalHistory,
    LanguageServers,
    BuildCaches,
}

const OPTIONS: [(OptionalCache, &str, &str); 3] = [
    (OptionalCache::LocalHistory, "invalidate-local-history", "Clear file system cache and Local History"),
    (
        OptionalCache::LanguageServers,
        "invalidate-language-servers",
        "Clear language server workspaces (Java import data and indexes)",
    ),
    (OptionalCache::BuildCaches, "invalidate-build-caches", "Clear build tool caches (Maven / Gradle sync results)"),
];

pub struct InvalidateCachesDialog {
    choices: Choices,
    focus: FocusHandle,
    key_claim: u64,
}

impl EventEmitter<InvalidateCachesEvent> for InvalidateCachesDialog {}

impl InvalidateCachesDialog {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let claimant = cx.weak_entity();
        let key_claim = input::claim_keys(
            focus.clone(),
            move |keystroke, _, cx| claimant.update(cx, |dialog, cx| dialog.claim_key(keystroke, cx)).unwrap_or(false),
            cx,
        );
        cx.on_release(|dialog, cx| input::release_keys(dialog.key_claim, cx)).detach();
        Self { choices: Choices::default(), focus, key_claim }
    }

    /// What is checked.
    pub fn choices(&self) -> Choices {
        self.choices
    }

    /// Whether "Local History of all projects will be deleted." shows.
    pub fn warns(&self) -> bool {
        self.choices.warns_about_local_history()
    }

    fn flag(&mut self, option: OptionalCache) -> &mut bool {
        let options = &mut self.choices.options;
        match option {
            OptionalCache::LocalHistory => &mut options.local_history,
            OptionalCache::LanguageServers => &mut options.language_servers,
            OptionalCache::BuildCaches => &mut options.build_caches,
        }
    }

    fn toggle(&mut self, option: OptionalCache, cx: &mut Context<Self>) {
        let flag = self.flag(option);
        *flag = !*flag;
        cx.notify();
    }

    fn invalidate(&mut self, cx: &mut Context<Self>) {
        cx.emit(InvalidateCachesEvent::Run(self.choices.invalidate_and_restart()));
    }

    /// Enter presses Invalidate and Restart, which has the keyboard; Escape cancels.
    fn claim_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        if keystroke.modifiers.modified() {
            return false;
        }
        match keystroke.key.as_str() {
            "enter" => self.invalidate(cx),
            "escape" => cx.emit(InvalidateCachesEvent::Cancelled),
            _ => return false,
        }
        true
    }
}

impl Focusable for InvalidateCachesDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for InvalidateCachesDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = tx(cx, "Invalidate Caches");
        let text = tx(
            cx,
            "The caches will be invalidated and rebuilt on the next startup: the symbol index of every project and \
             the syntax caches.",
        );
        let heading = tx(cx, "Optional:");
        let options: Vec<_> = OPTIONS
            .into_iter()
            .map(|(option, id, label)| {
                let label = tx(cx, label);
                let on = *self.flag(option);
                checkbox(cx, label, on)
                    .id(id)
                    .debug_selector(move || id.to_owned())
                    .on_click(cx.listener(move |dialog, _: &ClickEvent, _, cx| dialog.toggle(option, cx)))
            })
            .collect();
        let warning = self.warns().then(|| {
            let text = tx(cx, "Local History of all projects will be deleted.");
            div().text_color(theme::color(cx, "warning")).child(SharedString::from(text))
        });
        let labels = ["Invalidate and Restart", "Just Restart", "Cancel"].map(|label| tx(cx, label));
        let [invalidate, restart, cancel] = labels;
        let buttons = vec![
            button(cx, invalidate, true, true)
                .id("invalidate-and-restart")
                .debug_selector(|| "invalidate-and-restart".to_owned())
                .on_click(cx.listener(|dialog, _: &ClickEvent, _, cx| dialog.invalidate(cx))),
            button(cx, restart, false, true)
                .id("invalidate-just-restart")
                .debug_selector(|| "invalidate-just-restart".to_owned())
                .on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(InvalidateCachesEvent::Run(just_restart())))),
            button(cx, cancel, false, true)
                .id("invalidate-cancel")
                .debug_selector(|| "invalidate-cancel".to_owned())
                .on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(InvalidateCachesEvent::Cancelled))),
        ];
        frame(cx, WIDTH)
            .track_focus(&self.focus)
            .child(title_row(cx, title))
            .child(
                div()
                    .px(px(16.0))
                    .py(px(4.0))
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(div().child(SharedString::from(text)))
                    .child(div().font_weight(FontWeight::SEMIBOLD).child(SharedString::from(heading)))
                    .children(options)
                    .children(warning),
            )
            .child(footer(None, buttons))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{Entity, Modifiers, TestAppContext, VisualTestContext};
    use ide_workspace::caches::InvalidateCachesOptions;
    use std::cell::RefCell;
    use std::rc::Rc;

    type Events = Rc<RefCell<Vec<InvalidateCachesEvent>>>;

    fn open(cx: &mut TestAppContext) -> (Entity<InvalidateCachesDialog>, Events, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let (dialog, cx) = cx.add_window_view(InvalidateCachesDialog::new);
        let events: Events = Rc::default();
        let log = Rc::clone(&events);
        cx.update(|window, cx| {
            cx.subscribe(&dialog, move |_, event: &InvalidateCachesEvent, _| log.borrow_mut().push(event.clone()))
                .detach();
            let focus = dialog.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        (dialog, events, cx)
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.run_until_parked();
    }

    // src/features/app/__tests__/invalidateCaches.test.tsx › Invalidate Caches › schedules the chosen caches and
    // restarts
    #[gpui_kit::test]
    async fn schedules_the_chosen_caches_and_restarts(cx: &mut TestAppContext) {
        let (dialog, events, cx) = open(cx);
        assert!(!dialog.read_with(cx, |dialog, _| dialog.warns()));
        click(cx, "invalidate-local-history");
        assert!(dialog.read_with(cx, |dialog, _| dialog.warns()), "Local History will be deleted");
        click(cx, "invalidate-language-servers");
        click(cx, "invalidate-and-restart");
        let chosen = InvalidateCachesOptions { local_history: true, language_servers: true, build_caches: false };
        assert_eq!(
            *events.borrow(),
            [InvalidateCachesEvent::Run([CachesEffect::Schedule(Some(chosen)), CachesEffect::Restart])]
        );
    }

    // src/features/app/__tests__/invalidateCaches.test.tsx › Invalidate Caches › Just Restart cancels a pending
    // invalidation; Cancel does nothing
    #[gpui_kit::test]
    async fn just_restart_cancels_a_pending_invalidation_cancel_does_nothing(cx: &mut TestAppContext) {
        let (_dialog, events, cx) = open(cx);
        click(cx, "invalidate-build-caches");
        click(cx, "invalidate-just-restart");
        click(cx, "invalidate-cancel");
        cx.simulate_keystrokes("escape");
        assert_eq!(
            *events.borrow(),
            [
                InvalidateCachesEvent::Run([CachesEffect::Schedule(None), CachesEffect::Restart]),
                InvalidateCachesEvent::Cancelled,
                InvalidateCachesEvent::Cancelled,
            ]
        );
    }
}
