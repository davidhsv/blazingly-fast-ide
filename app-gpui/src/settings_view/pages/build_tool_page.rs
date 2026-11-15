//! A Build Tools page drawn by a view of its own (Gradle, Maven): its parts one under the other,
//! scrolled, a part with a condition drawn only while it holds.

use super::{Binding, ControlEvent};
use gpui_kit::{App, Context, EventEmitter, IntoElement, Render, SharedString, Window, div, prelude::*, px};
use std::rc::Rc;

/// A Build Tools page: its parts one under the other, scrolled; a part with a condition is drawn
/// only while it holds (Gradle home with a local installation). Each part's requests go out as
/// the page's.
pub struct ToolPage {
    id: &'static str,
    parts: Vec<(gpui_kit::AnyView, Option<Shown>)>,
    _subscriptions: Vec<gpui_kit::Subscription>,
}

/// Whether a part of a page is drawn now.
pub type Shown = Rc<dyn Fn(&App) -> bool>;

impl EventEmitter<ControlEvent> for ToolPage {}

impl ToolPage {
    pub fn new(id: &'static str, binding: &Binding, cx: &mut Context<Self>) -> Self {
        let observe = cx.observe(&binding.model, |_: &mut Self, _, cx| cx.notify());
        Self { id, parts: Vec::new(), _subscriptions: vec![observe] }
    }

    /// Adds `view` below the parts there, drawn while `shown` holds (always without one).
    pub fn add<V: Render + EventEmitter<ControlEvent>>(
        &mut self,
        view: gpui_kit::Entity<V>,
        shown: Option<Shown>,
        cx: &mut Context<Self>,
    ) {
        let forward = cx.subscribe(&view, |_: &mut Self, _, event: &ControlEvent, cx| cx.emit(event.clone()));
        self._subscriptions.push(forward);
        self.parts.push((view.into(), shown));
    }

    /// The parts drawn now.
    pub fn shown_parts(&self, cx: &App) -> Vec<gpui_kit::AnyView> {
        let drawn = self.parts.iter().filter(|(_, shown)| shown.as_ref().is_none_or(|shown| shown(cx)));
        drawn.map(|(view, _)| view.clone()).collect()
    }
}

impl Render for ToolPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let id = self.id;
        div()
            .id(SharedString::from(format!("settings-page-{id}-body")))
            .size_full()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(2.0))
            .children(self.shown_parts(cx))
    }
}
