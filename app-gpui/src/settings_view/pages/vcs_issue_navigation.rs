//! Version Control › Issue Navigation: the regular expressions whose matches in commit messages
//! become links (`$0` the whole match, `$1`… the groups), and buttons that add the usual JIRA,
//! YouTrack and GitHub patterns.

use super::tools_record_table::{Column, ReadRows, RecordTable, TableSpec, WriteRows};
use super::{Binding, ControlEvent, hint_line, row, row_label};
use crate::platform::i18n::{Localization, Param};
use crate::settings_view::controls::{button, tx};
use crate::theme;
use gpui_kit::{
    App, AppContext, ClickEvent, Context, Entity, EventEmitter, IntoElement, Render, SharedString, Subscription,
    Window, div, prelude::*, px,
};
use serde_json::{Value, json};
use std::rc::Rc;

pub const PAGE: &str = "vcs.issueNavigation";
/// The slot of the table.
pub const TABLE_SLOT: &str = "Issue ID";
/// The slot of the preset buttons.
pub const PRESETS_SLOT: &str = "Add JIRA Pattern";

const KEY: &str = "vcs.issueNavigation";
const HINT: &str = "Matches of an Issue ID regular expression in commit messages (Git log, commit details) become \
                    links; $0 is the whole match, $1… the groups.";

/// The preset buttons: (selector suffix, label, pattern, link).
const PRESETS: [(&str, &str, &str, &str); 3] = [
    ("jira", "Add JIRA Pattern", "[A-Z][A-Z0-9]+-\\d+", "https://jira.example.com/browse/$0"),
    ("youtrack", "Add YouTrack Pattern", "[A-Z][A-Z0-9]+-\\d+", "https://youtrack.example.com/issue/$0"),
    ("github", "Add GitHub Pattern", "#(\\d+)", "https://github.com/owner/repo/issues/$1"),
];

/// The links as (pattern, link); an entry that is not one reads as empty text.
fn links(binding: &Binding, cx: &App) -> Vec<(String, String)> {
    let value = binding.value(KEY, cx);
    let items = value.as_ref().and_then(Value::as_array).cloned().unwrap_or_default();
    let text = |item: &Value, name: &str| item.get(name).and_then(Value::as_str).unwrap_or_default().to_owned();
    items.iter().map(|item| (text(item, "pattern"), text(item, "link"))).collect()
}

fn stage_links(binding: &Binding, links: &[(String, String)], cx: &mut App) {
    let items: Vec<Value> = links.iter().map(|(pattern, link)| json!({ "pattern": pattern, "link": link })).collect();
    binding.stage(KEY, Value::Array(items), cx);
}

/// The patterns that are not regular expressions.
pub fn invalid_patterns(links: &[(String, String)]) -> Vec<String> {
    let invalid = links.iter().filter(|(pattern, _)| regex::Regex::new(pattern).is_err());
    invalid.map(|(pattern, _)| pattern.clone()).collect()
}

/// Issue navigation: the table of patterns and links, with the patterns that do not compile named
/// under it.
pub struct IssueNavigationView {
    binding: Binding,
    table: Entity<RecordTable>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ControlEvent> for IssueNavigationView {}

impl IssueNavigationView {
    pub fn new(binding: Binding, cx: &mut Context<Self>) -> Self {
        let reader = binding.clone();
        let read: ReadRows = Rc::new(move |cx: &App| {
            links(&reader, cx).into_iter().map(|(pattern, link)| vec![pattern, link]).collect()
        });
        let writer = binding.clone();
        let write: WriteRows = Rc::new(move |rows: Vec<Vec<String>>, cx: &mut App| {
            let links: Vec<(String, String)> = rows
                .into_iter()
                .map(|row| {
                    let mut cells = row.into_iter();
                    (cells.next().unwrap_or_default(), cells.next().unwrap_or_default())
                })
                .collect();
            stage_links(&writer, &links, cx);
        });
        let spec = TableSpec {
            id: "issues",
            columns: vec![
                Column { title: "Issue ID", placeholder: "[A-Z]+-\\d+" },
                Column { title: "Issue Link", placeholder: "https://tracker.example.com/issue/$0" },
            ],
            empty: "No issue navigation links",
        };
        let table = cx.new(|cx| RecordTable::new(&binding, spec, read, write, cx));
        let subscriptions = vec![
            cx.subscribe(&table, |_: &mut Self, _, event: &ControlEvent, cx| cx.emit(event.clone())),
            cx.observe(&binding.model, |_: &mut Self, _, cx| cx.notify()),
        ];
        Self { binding, table, _subscriptions: subscriptions }
    }

    pub fn table(&self) -> &Entity<RecordTable> {
        &self.table
    }

    /// The control as text: its label, the rows, the hint and the invalid patterns.
    pub fn lines(&self, cx: &App) -> Vec<String> {
        let mut out = vec![format!("{}:", tx(cx, "Issue navigation"))];
        out.extend(self.table.read(cx).lines(cx));
        out.push(format!("    {}", tx(cx, HINT)));
        let invalid = invalid_patterns(&links(&self.binding, cx));
        if !invalid.is_empty() {
            out.push(format!("    ! Invalid pattern: {}", invalid.join(", ")));
        }
        out
    }
}

impl Render for IssueNavigationView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.binding.query(cx);
        let label = tx(cx, "Issue navigation");
        let invalid = invalid_patterns(&links(&self.binding, cx));
        let error = (!invalid.is_empty()).then(|| {
            let localization = cx.default_global::<Localization>();
            let text = localization.0.tx_built(&["Invalid pattern: ", ""], &[Param::Text(invalid.join(", "))]);
            div()
                .debug_selector(|| "settings-issue-invalid".to_owned())
                .pl(px(4.0))
                .text_size(theme::length(cx, "fs-small"))
                .text_color(theme::color(cx, "danger"))
                .child(SharedString::from(text))
        });
        div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(row(cx, self.binding.matches("Issue navigation", cx)).child(row_label(cx, &label, &query)))
            .child(self.table.clone())
            .child(hint_line(cx, &tx(cx, HINT), 4.0))
            .children(error)
    }
}

/// Add pattern: a button per usual tracker, each adding its pattern and link.
pub struct IssuePresetsView {
    binding: Binding,
}

impl EventEmitter<ControlEvent> for IssuePresetsView {}

impl IssuePresetsView {
    pub fn new(binding: Binding) -> Self {
        Self { binding }
    }

    pub fn add(&mut self, preset: usize, cx: &mut Context<Self>) {
        let Some((_, _, pattern, link)) = PRESETS.get(preset) else { return };
        let mut all = links(&self.binding, cx);
        all.push(((*pattern).to_owned(), (*link).to_owned()));
        stage_links(&self.binding, &all, cx);
    }

    pub fn lines(&self, cx: &App) -> Vec<String> {
        let buttons: Vec<String> = PRESETS.iter().map(|(_, label, _, _)| format!("[{}]", tx(cx, label))).collect();
        vec![format!("{}: {}", tx(cx, "Add pattern"), buttons.join(" "))]
    }
}

impl Render for IssuePresetsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.binding.query(cx);
        let label = tx(cx, "Add pattern");
        let matched = PRESETS.iter().any(|(_, label, _, _)| self.binding.matches(label, cx));
        let buttons: Vec<_> = PRESETS
            .iter()
            .enumerate()
            .map(|(index, (name, text, _, _))| {
                let selector = format!("settings-issue-preset-{name}");
                let text = tx(cx, text);
                div()
                    .id(SharedString::from(selector.clone()))
                    .debug_selector(move || selector)
                    .child(button(cx, &text, false, true))
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.add(index, cx)))
            })
            .collect();
        row(cx, matched).child(row_label(cx, &label, &query)).children(buttons)
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;
    use crate::settings_model::staging::Scope;
    use gpui_kit::TestAppContext;

    fn stored(staged: &[(String, String)]) -> Value {
        let (_, value) = staged.last().cloned().expect("an edit is staged");
        serde_json::from_str(&value).expect("JSON")
    }

    // Not from a spec: the presets add their pattern and link after the links already there, into
    // the project.
    #[gpui_kit::test]
    async fn the_presets_add_their_pattern_after_the_links_there(cx: &mut TestAppContext) {
        let existing = json!([{ "pattern": "OPS-\\d+", "link": "https://ops.example/$0" }]);
        let model = cx.update(|cx| dialog_model(PAGE, &[(KEY, existing)], cx));
        let binding = Binding::fixed(model.clone(), PAGE, Scope::Project);
        let (view, _, cx) = open(cx, move |_, _| IssuePresetsView::new(binding));
        assert_eq!(
            view.read_with(cx, |view, cx| view.lines(cx)),
            ["Add pattern: [Add JIRA Pattern] [Add YouTrack Pattern] [Add GitHub Pattern]"]
        );
        click(cx, "settings-issue-preset-github");
        let staged = staged(&model, cx);
        assert_eq!(staged[0].0, "project:vcs.issueNavigation");
        let expected = json!([
            { "pattern": "OPS-\\d+", "link": "https://ops.example/$0" },
            { "pattern": "#(\\d+)", "link": "https://github.com/owner/repo/issues/$1" },
        ]);
        assert_eq!(stored(&staged), expected);
    }

    // Not from a spec: a pattern that is not a regular expression is named under the table.
    #[gpui_kit::test]
    async fn an_invalid_pattern_is_named_under_the_table(cx: &mut TestAppContext) {
        let existing = json!([{ "pattern": "ABC-(\\d+", "link": "x/$1" }, { "pattern": "OK-\\d+", "link": "y/$0" }]);
        let model = cx.update(|cx| dialog_model(PAGE, &[(KEY, existing)], cx));
        let binding = Binding::fixed(model, PAGE, Scope::Global);
        let (view, _, cx) = open(cx, move |_, cx| IssueNavigationView::new(binding, cx));
        let lines = view.read_with(cx, |view, cx| view.lines(cx));
        assert_eq!(lines[1], "  ABC-(\\d+ | x/$1");
        assert_eq!(lines.last().map(String::as_str), Some("    ! Invalid pattern: ABC-(\\d+"));
        assert!(cx.debug_bounds("settings-issue-invalid").is_some());
    }
}
