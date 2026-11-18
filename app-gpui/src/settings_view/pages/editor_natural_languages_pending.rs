//! Editor › Natural Languages: the language the Typo inspection checks, English with its bundled
//! dictionaries. The page shows it and stores nothing.

use super::{Binding, ControlEvent, hint_line, muted, row, row_label};
use crate::settings_view::controls::{section, tx};
use gpui_kit::{App, Context, EventEmitter, IntoElement, Render, Window, div, prelude::*};

pub const PAGE: &str = "editor.naturalLanguages";

const SECTION: &str = "Languages";
const LANGUAGE: &str = "English (USA, UK)";
const LANGUAGE_HINT: &str = "Bundled Hunspell dictionaries (SCOWL en_US and en_GB) and common programming terms.";
const USED_BY: &str = "Used by Proofreading › Typo";

/// The page: one section with the bundled language, what it is made of and what uses it.
pub struct NaturalLanguagesView {
    binding: Binding,
}

impl EventEmitter<ControlEvent> for NaturalLanguagesView {}

impl NaturalLanguagesView {
    pub fn new(binding: Binding, cx: &mut Context<Self>) -> Self {
        // The search marks the language's row as the query changes.
        cx.observe(&binding.model, |_, _, cx| cx.notify()).detach();
        Self { binding }
    }

    /// The page as text: the section's title, the language with what uses it, and its hint.
    pub fn lines(&self, cx: &App) -> Vec<String> {
        vec![
            format!("— {} —", tx(cx, SECTION)),
            format!("{}: {}", tx(cx, LANGUAGE), tx(cx, USED_BY)),
            format!("    {}", tx(cx, LANGUAGE_HINT)),
        ]
    }

    /// Whether the search marks the language's row.
    pub fn matched(&self, cx: &App) -> bool {
        self.binding.matches(LANGUAGE, cx)
    }
}

impl Render for NaturalLanguagesView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.binding.query(cx);
        let matched = self.matched(cx);
        let label = tx(cx, LANGUAGE);
        let used_by = tx(cx, USED_BY);
        let language = div()
            .flex()
            .flex_col()
            .debug_selector(|| "settings-natural-language-en".to_owned())
            .child(row(cx, matched).child(row_label(cx, &label, &query)).child(muted(cx, &used_by)))
            .child(hint_line(cx, &tx(cx, LANGUAGE_HINT), 4.0));
        let title = tx(cx, SECTION);
        div().flex().flex_col().child(section(cx, Some(&title), &query, vec![language.into_any_element()]))
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;
    use crate::settings_model::staging::Scope;
    use gpui_kit::TestAppContext;

    // Natural Languages › shows the bundled English dictionaries and that Proofreading › Typo uses
    // them; nothing is staged
    #[gpui_kit::test]
    async fn shows_the_bundled_english_dictionaries_used_by_typo(cx: &mut TestAppContext) {
        let model = cx.update(|cx| dialog_model(PAGE, &[], cx));
        let binding = Binding::fixed(model.clone(), PAGE, Scope::Global);
        let (view, seen, cx) = open(cx, move |_, cx| NaturalLanguagesView::new(binding, cx));
        assert_eq!(
            view.read_with(cx, |view, cx| view.lines(cx)),
            [
                "— Languages —",
                "English (USA, UK): Used by Proofreading › Typo",
                "    Bundled Hunspell dictionaries (SCOWL en_US and en_GB) and common programming terms.",
            ]
        );
        assert!(cx.debug_bounds("settings-natural-language-en").is_some(), "the language's row is drawn");
        click(cx, "settings-natural-language-en");
        assert!(staged(&model, cx).is_empty(), "the page stores nothing");
        assert!(seen.borrow().is_empty());
    }

    // Natural Languages › the search marks the language's row by its name
    #[gpui_kit::test]
    async fn the_search_marks_the_language_by_its_name(cx: &mut TestAppContext) {
        let model = cx.update(|cx| dialog_model(PAGE, &[], cx));
        let binding = Binding::fixed(model.clone(), PAGE, Scope::Global);
        let (view, _, cx) = open(cx, move |_, cx| NaturalLanguagesView::new(binding, cx));
        assert!(!view.read_with(cx, |view, cx| view.matched(cx)));
        model.update(cx, |dialog, cx| {
            dialog.query = "english".to_owned();
            cx.notify();
        });
        cx.run_until_parked();
        assert!(view.read_with(cx, |view, cx| view.matched(cx)));
    }
}
