//! Editor › Natural Languages › Spelling: the words the Typo inspection accepts (the project's
//! dictionary, stored with the project, and the application's, accepted in every project) and the
//! custom dictionary files it reads. Each list keeps an entry once, trimmed, and drops one edited
//! to nothing; the Save to dictionary quick fix writes the same two word lists.
//!
//! The lists edit fixed scopes whatever the dialog shows: the project dictionary is the project's
//! setting, the other two are global settings.

use super::list_editor::{ListEditor, ListShape};
use super::{Binding, ControlEvent};
use crate::settings_model::staging::{Scope, SettingsDialog};
use crate::settings_view::controls::{section, tx};
use gpui_kit::{App, Context, Entity, EventEmitter, IntoElement, Render, Subscription, Window, div, prelude::*, px};

pub const PAGE: &str = "editor.naturalLanguages.spelling";
pub const PROJECT_WORDS: &str = "spelling.projectWords";
pub const IDE_WORDS: &str = "spelling.ideWords";
pub const CUSTOM_DICTIONARIES: &str = "spelling.customDictionaries";

const ACCEPTED: &str = "Accepted words";
const CUSTOM: &str = "Custom dictionaries";

/// The page: Accepted words (the project's list while a project is open and the dialog edits more
/// than new projects' settings, then the application's) and Custom dictionaries.
pub struct SpellingView {
    /// The page's global binding, for the Settings search's text.
    binding: Binding,
    project: Option<Entity<ListEditor>>,
    application: Entity<ListEditor>,
    dictionaries: Entity<ListEditor>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ControlEvent> for SpellingView {}

impl SpellingView {
    /// The page over the dialog's `model`; `has_project`: a project is open in the window.
    pub fn new(model: Entity<SettingsDialog>, has_project: bool, cx: &mut Context<Self>) -> Self {
        let show_project = has_project && !model.read(cx).new_projects;
        let words = ListShape::Strings { placeholder: "word" };
        let project = show_project.then(|| {
            let binding = Binding::fixed(model.clone(), PAGE, Scope::Project);
            cx.new(|cx| {
                let hint = Some("Stored with the project.");
                ListEditor::new(binding, PROJECT_WORDS, "Project dictionary", words, "No words", hint, cx)
            })
        });
        let global = Binding::fixed(model.clone(), PAGE, Scope::Global);
        let binding = global.clone();
        let application = cx.new(|cx| {
            let hint = Some("Accepted in every project.");
            ListEditor::new(binding, IDE_WORDS, "Application dictionary", words, "No words", hint, cx)
        });
        let binding = global.clone();
        let dictionaries = cx.new(|cx| {
            let shape = ListShape::Strings { placeholder: "~/dictionaries/team.dic" };
            let hint = Some(
                "Plain-text word lists (one word per line) or Hunspell .dic files. Relative paths resolve \
                 against the project root, ~/ against the home folder.",
            );
            ListEditor::new(binding, CUSTOM_DICTIONARIES, "Dictionary files", shape, "No custom dictionaries", hint, cx)
        });
        // What a list asks (Escape is the dialog's Cancel) goes to the dialog as the page's own.
        let mut subscriptions = Vec::new();
        for list in project.iter().chain([&application, &dictionaries]) {
            subscriptions.push(cx.subscribe(list, |_, _, event: &ControlEvent, cx| cx.emit(event.clone())));
        }
        subscriptions.push(cx.observe(&model, |_, _, cx| cx.notify()));
        Self { binding: global, project, application, dictionaries, _subscriptions: subscriptions }
    }

    /// The project's word list, while it is shown.
    pub fn project(&self) -> Option<&Entity<ListEditor>> {
        self.project.as_ref()
    }

    pub fn application(&self) -> &Entity<ListEditor> {
        &self.application
    }

    pub fn dictionaries(&self) -> &Entity<ListEditor> {
        &self.dictionaries
    }

    /// The page as text: each section's title, then its lists ([`ListEditor::lines`]).
    pub fn lines(&self, cx: &App) -> Vec<String> {
        let mut out = vec![format!("— {} —", tx(cx, ACCEPTED))];
        for list in self.project.iter().chain([&self.application]) {
            out.extend(list.read(cx).lines(cx));
        }
        out.push(format!("— {} —", tx(cx, CUSTOM)));
        out.extend(self.dictionaries.read(cx).lines(cx));
        out
    }
}

impl Render for SpellingView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.binding.query(cx);
        let accepted: Vec<_> = self
            .project
            .iter()
            .chain([&self.application])
            .map(|list| div().child(list.clone()).into_any_element())
            .collect();
        let custom = vec![div().child(self.dictionaries.clone()).into_any_element()];
        let (accepted_title, custom_title) = (tx(cx, ACCEPTED), tx(cx, CUSTOM));
        div().flex().flex_col().gap(px(8.0)).child(section(cx, Some(&accepted_title), &query, accepted)).child(section(
            cx,
            Some(&custom_title),
            &query,
            custom,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;
    use gpui_kit::{TestAppContext, VisualTestContext};
    use serde_json::json;

    fn page(
        cx: &mut TestAppContext,
        has_project: bool,
    ) -> (Entity<SpellingView>, Seen, Entity<SettingsDialog>, &mut VisualTestContext) {
        let settings = [(IDE_WORDS, json!(["kubectl"])), (PROJECT_WORDS, json!(["openidea"]))];
        let model = cx.update(|cx| dialog_model(PAGE, &settings, cx));
        let for_view = model.clone();
        let (view, seen, cx) = open(cx, move |_, cx| SpellingView::new(for_view, has_project, cx));
        (view, seen, model, cx)
    }

    // Spelling › shows the project and application dictionaries under Accepted words and the
    // dictionary files under Custom dictionaries
    #[gpui_kit::test]
    async fn shows_the_accepted_words_and_the_custom_dictionaries(cx: &mut TestAppContext) {
        let (view, _, _, cx) = page(cx, true);
        assert_eq!(
            view.read_with(cx, |view, cx| view.lines(cx)),
            [
                "— Accepted words —",
                "Project dictionary:",
                "  openidea",
                "Application dictionary:",
                "  kubectl",
                "— Custom dictionaries —",
                "Dictionary files:",
                "  No custom dictionaries",
            ]
        );
    }

    // Spelling › without a project the project dictionary is not shown
    #[gpui_kit::test]
    async fn without_a_project_the_project_dictionary_is_not_shown(cx: &mut TestAppContext) {
        let (view, _, _, cx) = page(cx, false);
        assert!(view.read_with(cx, |view, _| view.project().is_none()));
        let lines = view.read_with(cx, |view, cx| view.lines(cx));
        assert_eq!(lines[..3], ["— Accepted words —", "Application dictionary:", "  kubectl"]);
        assert!(cx.debug_bounds("settings-list-spelling.projectWords-add").is_none());
    }

    // Spelling › Settings for New Projects shows no project dictionary
    #[gpui_kit::test]
    async fn settings_for_new_projects_show_no_project_dictionary(cx: &mut TestAppContext) {
        let model = cx.update(|cx| {
            let model = dialog_model(PAGE, &[], cx);
            model.update(cx, |dialog, _| dialog.new_projects = true);
            model
        });
        let (view, _, cx) = open(cx, move |_, cx| SpellingView::new(model, true, cx));
        assert!(view.read_with(cx, |view, _| view.project().is_none()));
    }

    // Spelling › a word added to the project dictionary is staged in the project, trimmed; one added
    // to the application dictionary is staged globally; a word already listed is kept once
    #[gpui_kit::test]
    async fn words_are_staged_trimmed_and_once_in_their_own_scopes(cx: &mut TestAppContext) {
        let (_, _, model, cx) = page(cx, true);
        click(cx, "settings-list-spelling.projectWords-add");
        cx.simulate_input("  teh  ");
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        click(cx, "settings-list-spelling.ideWords-add");
        cx.simulate_input("kubectl");
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        click(cx, "settings-list-spelling.ideWords-add");
        cx.simulate_input("qwzx");
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        assert_eq!(
            staged(&model, cx),
            [
                ("project:spelling.projectWords".to_owned(), "[\"openidea\",\"teh\"]".to_owned()),
                ("global:spelling.ideWords".to_owned(), "[\"kubectl\",\"qwzx\"]".to_owned()),
            ]
        );
    }

    // Spelling › a dictionary file is added, edited in place and removed; the list stores paths
    #[gpui_kit::test]
    async fn a_dictionary_file_is_added_edited_and_removed(cx: &mut TestAppContext) {
        let (view, _, model, cx) = page(cx, true);
        click(cx, "settings-list-spelling.customDictionaries-add");
        cx.simulate_input("~/dictionaries/team.dic");
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        assert_eq!(
            staged(&model, cx),
            [("global:spelling.customDictionaries".to_owned(), "[\"~/dictionaries/team.dic\"]".to_owned())]
        );
        let dictionaries = view.read_with(cx, |view, _| view.dictionaries().clone());
        dictionaries.update_in(cx, |list, window, cx| list.start_edit(0, window, cx));
        cx.simulate_keystrokes("cmd-a");
        cx.simulate_input("words.txt");
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        assert_eq!(dictionaries.read_with(cx, |list, cx| list.rows(cx)), [vec!["words.txt".to_owned()]]);
        click(cx, "settings-list-spelling.customDictionaries-row-0");
        click(cx, "settings-list-spelling.customDictionaries-remove");
        let lines = dictionaries.read_with(cx, |list, cx| list.lines(cx));
        assert_eq!(lines, ["Dictionary files:", "  No custom dictionaries"]);
    }

    // Spelling › Escape in a list is the dialog's Cancel
    #[gpui_kit::test]
    async fn escape_in_a_list_is_the_dialogs_cancel(cx: &mut TestAppContext) {
        let (_, seen, _, cx) = page(cx, true);
        click(cx, "settings-list-spelling.ideWords-row-0");
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        assert_eq!(*seen.borrow(), ["cancel"]);
    }
}
