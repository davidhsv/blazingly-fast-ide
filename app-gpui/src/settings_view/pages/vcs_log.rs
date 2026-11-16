//! Version Control › Log: how the Git log shows references, dates and line counts, and which of
//! its columns are visible.
//!
//! These are the log's own options (those its View Options menu changes too), kept in the UI
//! storage under [`STORAGE_KEY`] rather than in the settings files: a checkbox changes them at once
//! and the open log shows the change, so the page stages nothing and Apply, Reset and Cancel leave
//! it alone. The page reads the options from the storage each time it is drawn, so a change the
//! log makes meanwhile shows here too, and hands each change to the workbench as
//! [`LogSettingsEvent::Changed`].

use super::{Binding, row};
use crate::platform::storage::UiStorage;
use crate::settings_view::controls::{check_box, search_label, tx};
use crate::theme;
use crate::vcs::log_options::{LogOptions, STORAGE_KEY, parse_log_options};
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, EventEmitter, IntoElement, Render, Subscription, Window, div, prelude::*, px,
};
use serde_json::Value;

pub const PAGE: &str = "vcs.log";

/// One checkbox of the page: its name in selectors, its label, and how it reads and sets its option.
struct Flag {
    name: &'static str,
    label: &'static str,
    get: fn(&LogOptions) -> bool,
    set: fn(&mut LogOptions, bool),
}

static OPTIONS: [Flag; 5] = [
    Flag {
        name: "compactRefs",
        label: "Show only the first reference",
        get: |o| o.compact_refs,
        set: |o, on| o.compact_refs = on,
    },
    Flag { name: "tagNames", label: "Show tag names", get: |o| o.tag_names, set: |o, on| o.tag_names = on },
    Flag {
        name: "commitTimestamp",
        label: "Show commit time rather than author time",
        get: |o| o.commit_timestamp,
        set: |o, on| o.commit_timestamp = on,
    },
    Flag {
        name: "refsOnLeft",
        label: "Show references on the left",
        get: |o| o.refs_on_left,
        set: |o, on| o.refs_on_left = on,
    },
    Flag {
        name: "showLineStats",
        label: "Show lines added and deleted",
        get: |o| o.show_line_stats,
        set: |o, on| o.show_line_stats = on,
    },
];

static COLUMNS: [Flag; 4] = [
    Flag { name: "showAuthor", label: "Author", get: |o| o.show_author, set: |o, on| o.show_author = on },
    Flag { name: "showHash", label: "Hash", get: |o| o.show_hash, set: |o, on| o.show_hash = on },
    Flag { name: "showDate", label: "Date", get: |o| o.show_date, set: |o, on| o.show_date = on },
    Flag {
        name: "showGitHubChecks",
        label: "GitHub Commit Checks",
        get: |o| o.show_git_hub_checks,
        set: |o, on| o.show_git_hub_checks = on,
    },
];

const COLUMNS_TITLE: &str = "Visible columns:";

/// What the Settings search finds on the page.
pub fn labels() -> Vec<&'static str> {
    let flags = OPTIONS.iter().chain(COLUMNS.iter()).map(|flag| flag.label);
    flags.chain(std::iter::once(COLUMNS_TITLE)).collect()
}

/// The log's options as stored, the defaults while nothing is.
pub fn stored_log_options(cx: &App) -> LogOptions {
    let stored = cx.try_global::<UiStorage>().and_then(|storage| storage.get(STORAGE_KEY));
    parse_log_options(stored.and_then(|text| serde_json::from_str::<Value>(text).ok()).as_ref())
}

/// What the page asks of the workbench.
#[derive(Debug, Clone, PartialEq)]
pub enum LogSettingsEvent {
    /// A checkbox changed the options: store them and show the log with them.
    Changed(Box<LogOptions>),
}

pub struct LogSettingsView {
    binding: Binding,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<LogSettingsEvent> for LogSettingsView {}

impl LogSettingsView {
    pub fn new(binding: Binding, cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![
            cx.observe(&binding.model, |_: &mut Self, _, cx| cx.notify()),
            // The log's own menus change the same options while the page is open.
            cx.observe_global::<UiStorage>(|_: &mut Self, cx| cx.notify()),
        ];
        Self { binding, _subscriptions: subscriptions }
    }

    /// Flips the checkbox `name` (an option's name in the stored JSON).
    pub fn flip(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some(flag) = OPTIONS.iter().chain(COLUMNS.iter()).find(|flag| flag.name == name) else { return };
        let mut options = stored_log_options(cx);
        let on = (flag.get)(&options);
        (flag.set)(&mut options, !on);
        cx.emit(LogSettingsEvent::Changed(Box::new(options)));
    }

    /// The page as text: a `[x] label` line per option, the columns' title, a line per column.
    pub fn lines(&self, cx: &App) -> Vec<String> {
        let options = stored_log_options(cx);
        let line = |flag: &Flag| format!("{} {}", if (flag.get)(&options) { "[x]" } else { "[ ]" }, tx(cx, flag.label));
        let mut out: Vec<String> = OPTIONS.iter().map(&line).collect();
        out.push(tx(cx, COLUMNS_TITLE));
        out.extend(COLUMNS.iter().map(|flag| format!("  {}", line(flag))));
        out
    }

    fn checkbox(&self, flag: &Flag, options: &LogOptions, query: &str, cx: &mut Context<Self>) -> AnyElement {
        let name = flag.name;
        let selector = format!("settings-log-{name}");
        let text = tx(cx, flag.label);
        row(cx, self.binding.matches(flag.label, cx))
            .id(gpui_kit::SharedString::from(selector.clone()))
            .debug_selector(move || selector)
            .child(check_box(cx, (flag.get)(options)))
            .child(search_label(cx, &text, query))
            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.flip(name, cx)))
            .into_any_element()
    }
}

impl Render for LogSettingsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.binding.query(cx);
        let options = stored_log_options(cx);
        let flags: Vec<AnyElement> = OPTIONS.iter().map(|flag| self.checkbox(flag, &options, &query, cx)).collect();
        let columns: Vec<AnyElement> = COLUMNS.iter().map(|flag| self.checkbox(flag, &options, &query, cx)).collect();
        let title = tx(cx, COLUMNS_TITLE);
        div().flex().flex_col().gap(px(10.0)).children(flags).child(
            div()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .mt(px(6.0))
                .child(div().text_color(theme::color(cx, "fg-muted")).child(search_label(cx, &title, &query)))
                .child(div().flex().flex_col().gap(px(8.0)).pl(px(18.0)).children(columns)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{click, dialog_model, init};
    use super::*;
    use crate::settings_model::staging::Scope;
    use gpui_kit::TestAppContext;
    use std::cell::RefCell;
    use std::rc::Rc;

    // Not from a spec: the page shows the stored options, and a click hands on the options with that
    // checkbox flipped and the others as stored.
    #[gpui_kit::test]
    async fn a_checkbox_hands_on_the_stored_options_with_it_flipped(cx: &mut TestAppContext) {
        init(cx);
        cx.update(|cx| {
            let mut storage = UiStorage::default();
            storage.set(STORAGE_KEY, r#"{"tagNames":true,"showHash":true,"sort":"topo"}"#.to_owned());
            cx.set_global(storage);
        });
        let model = cx.update(|cx| dialog_model(PAGE, &[], cx));
        let binding = Binding::fixed(model, PAGE, Scope::Global);
        let (view, cx) = cx.add_window_view(move |_, cx| LogSettingsView::new(binding, cx));
        cx.update(|window, _| window.activate_window());
        let seen: Rc<RefCell<Vec<LogSettingsEvent>>> = Rc::default();
        let log = seen.clone();
        cx.update(|_, cx| {
            cx.subscribe(&view, move |_, event: &LogSettingsEvent, _| log.borrow_mut().push(event.clone())).detach();
        });
        assert_eq!(
            view.read_with(cx, |view, cx| view.lines(cx)),
            [
                "[x] Show only the first reference",
                "[x] Show tag names",
                "[ ] Show commit time rather than author time",
                "[ ] Show references on the left",
                "[ ] Show lines added and deleted",
                "Visible columns:",
                "  [x] Author",
                "  [x] Hash",
                "  [x] Date",
                "  [ ] GitHub Commit Checks",
            ]
        );
        click(cx, "settings-log-refsOnLeft");
        let LogSettingsEvent::Changed(options) = seen.borrow().last().cloned().expect("the options changed");
        assert!(options.refs_on_left && options.tag_names && options.show_hash);
        assert_eq!(options.sort, crate::vcs::log_options::LogSort::Topo, "the options the page has no box for stay");
        // Another writer of the storage (the log's View Options) shows on the open page.
        cx.update(|_, cx| cx.global_mut::<UiStorage>().set(STORAGE_KEY, r#"{"showAuthor":false}"#.to_owned()));
        cx.run_until_parked();
        assert_eq!(view.read_with(cx, |view, cx| view.lines(cx))[6], "  [ ] Author");
    }
}
