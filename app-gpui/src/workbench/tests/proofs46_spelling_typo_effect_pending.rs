//! The Typo inspection in an open editor against the words accepted on Settings › Editor › Natural
//! Languages › Spelling: a misspelt word is reported until it is added to a dictionary on the page
//! and applied.

use super::proofs19_settings_misc::bring_into_view;
use super::*;
use crate::editor::intelligence::EditorIntelligence;
use crate::editor::view::Editor;
use serde_json::Value;

/// The Typo problems the editor shows, by word.
fn typos(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> Vec<String> {
    let intelligence = cx.update(|_, cx| EditorIntelligence::of(editor, cx)).expect("the editor's intelligence");
    intelligence.read_with(cx, |this, _| {
        let word = |message: &str| message.strip_prefix("Typo: In word '")?.strip_suffix('\'').map(str::to_owned);
        this.problems().iter().filter_map(|problem| word(&problem.message)).collect()
    })
}

/// Clicks the list's + drawn as `add`, types `word` and presses Enter.
fn add_word(cx: &mut VisualTestContext, add: &'static str, word: &str) {
    bring_into_view(cx, add);
    click_on(cx, add);
    cx.simulate_input(word);
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
}

// Spelling › a word added to the application dictionary on the page stops being reported as a typo
// in an open editor once applied; one added to the project dictionary too
#[gpui_kit::test]
async fn a_word_accepted_on_the_spelling_page_is_no_longer_reported_as_a_typo(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("notes.txt", "The qwzxv and vbnmq words.\n")]);
    cx.update(|window, _| window.activate_window());
    let file = root.join("notes.txt");
    workbench.update_in(cx, |w, window, cx| w.open_project_file(file, window, cx));
    wait_long(cx, "the editor", |cx| workbench.read_with(cx, |w, _| w.editor().is_some()));
    let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");
    wait_long(cx, "both typos", |cx| {
        let shown = typos(&editor, cx);
        shown.contains(&"qwzxv".to_owned()) && shown.contains(&"vbnmq".to_owned())
    });

    run_action(cx, "file.settings", Some(Value::from("editor.naturalLanguages.spelling")));
    add_word(cx, "settings-list-spelling.ideWords-add", "qwzxv");
    add_word(cx, "settings-list-spelling.projectWords-add", "vbnmq");
    assert!(typos(&editor, cx).contains(&"qwzxv".to_owned()), "nothing changes before Apply");
    click_on(cx, "settings-ok");
    wait_long(cx, "no typo left", |cx| typos(&editor, cx).is_empty());
}
