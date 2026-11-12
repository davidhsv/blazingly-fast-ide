use super::*;
use crate::agents::fixtures::{calls, local_task, permission, permission_option, question, tool_with};
use crate::agents::store::Saved;
use ide_agent::AgentToolStatus;

const COMMAND: &str = "python3 -c 'from pathlib import Path\np = Path(\"note.txt\")\np.write_text(\"one\\ntwo\")'";

fn store() -> AgentsStore {
    AgentsStore::new(Saved::default())
}

fn respond(task: &str, permission: u32, option: Option<&str>) -> AgentCall {
    AgentCall::Respond { task: task.to_owned(), permission, option: option.map(str::to_owned) }
}

fn sent(effect: Option<AgentEffect>) -> Vec<AgentCall> {
    calls(effect.as_slice())
}

fn command_options() -> Vec<ide_agent::AgentPermissionOption> {
    vec![
        permission_option("opaque-once", "Allow Once", "allow_once"),
        permission_option("opaque-policy", &format!("Always Allow `{COMMAND}`"), "allow_always"),
        permission_option("opaque-no", "Decline", "reject_once"),
        permission_option("opaque-stop", "Decline and Stop", "reject_always"),
    ]
}

fn line(entry: &AgentEntry) -> PermissionLine {
    permission_line(entry, &local_task("A"), "codex", &mut I18n::default()).expect("a permission line")
}

fn asking(id: u32, title: &str, questions: Vec<AgentQuestion>) -> AgentEntry {
    let options = vec![permission_option("reject", "Skip", "reject_once")];
    AgentEntry::Permission { id, title: title.to_owned(), options, questions, chosen: None }
}

#[test]
fn shows_the_whole_question_once_inline_with_every_answer_and_sends_the_chosen_one() {
    let text = "Scenario 4 (\"follows the TR\") still leaves the \"About your request\" section empty after two \
                rewrites.\nHow should we continue?";
    let entry = permission(
        7,
        text,
        vec![
            permission_option("a", "Ship it as is and record it (Recommended)", "allow_once"),
            permission_option("b", "Detect the request in code", "allow_once"),
            permission_option("c", "Skip", "reject_once"),
        ],
    );
    let entries = [entry];
    assert!(question_card(&entries).is_none());
    let view = line(&entries[0]);
    assert_eq!(view.title, "Allow tool call?");
    assert_eq!(view.command, None);
    assert_eq!(view.detail, text);
    let labels: Vec<&str> = view.answers.iter().map(|a| a.label.as_str()).collect();
    assert_eq!(labels, ["Ship it as is and record it (Recommended)", "Detect the request in code", "Skip"]);
    let mut store = store();
    let mut responding = Responding::default();
    let effect = responding.answer(&mut store, "A", 7, view.answers[1].option.as_deref());
    assert_eq!(sent(effect), [respond("A", 7, Some("b"))]);
}

#[test]
fn keeps_separate_answers_for_codex_question_ids_with_identical_wording() {
    let questions: Vec<AgentQuestion> = ["first", "second"]
        .into_iter()
        .enumerate()
        .map(|(i, key)| AgentQuestion {
            answer_key: Some(key.to_owned()),
            ..question(key, "Which?", false, &[(if i == 0 { "One" } else { "Two" }, "", None)])
        })
        .collect();
    let title = "Which?".to_owned();
    let entries = [AgentEntry::Permission { id: 11, title, options: Vec::new(), questions, chosen: None }];
    let (id, title, questions) = question_card(&entries).expect("a question card");
    assert_eq!(title, "Which?");
    let mut form = QuestionForm::new("A", id, questions);
    form.choose(0, "One", true);
    form.choose(1, "Two", true);
    let effect = form.submit(&mut store());
    assert_eq!(sent(effect), [respond("A", 11, Some(r#"{"first":["One"],"second":["Two"]}"#))]);
}

#[test]
fn shows_every_question_with_its_descriptions_in_a_card_and_sends_all_answers_at_once() {
    let i18n = I18n::default();
    let entries = [asking(
        9,
        "Which layout?",
        vec![
            question(
                "Layout",
                "Which layout?",
                false,
                &[("Table (Recommended)", "One row per check", None), ("List", "Grouped under headings", None)],
            ),
            question("Checks", "Which checks?", true, &[("Disk", "", None), ("Updates", "", None)]),
        ],
    )];
    // The card answers it: its line in the conversation offers no buttons.
    assert!(line(&entries[0]).answers.is_empty());
    let (id, title, questions) = question_card(&entries).expect("a question card");
    assert_eq!(title, "Which layout?");
    let mut form = QuestionForm::new("A", id, questions);
    let table = &form.choices(0, &i18n)[0];
    assert_eq!((table.label.as_str(), table.recommended), ("Table", true));
    assert_eq!(table.description, "One row per check");
    form.choose(0, "List", true);
    assert!(!form.can_submit());
    form.choose(1, "Disk", true);
    form.choose_other(1, true);
    assert!(!form.can_submit());
    form.type_other(1, "GPU temps");
    assert!(form.can_submit());
    let effect = form.submit(&mut store());
    let json = r#"{"Which layout?":["List"],"Which checks?":["Disk","GPU temps"]}"#;
    assert_eq!(sent(effect), [respond("A", 9, Some(json))]);
    assert!(form.busy());
}

#[test]
fn picks_options_with_number_keys_previews_the_picked_or_hovered_one_and_skips_with_escape() {
    let i18n = I18n::default();
    let questions = vec![question("", "Which layout?", false, &[("Table", "", Some("| a | b |")), ("List", "", None)])];
    let mut form = QuestionForm::new("A", 10, &questions);
    form.focus(0);
    form.press_number(2);
    assert!(form.choices(0, &i18n)[1].checked);
    let caption = |form: &QuestionForm| form.preview(0, &i18n).expect("a preview");
    assert_eq!(caption(&form), Preview { caption: "List".to_owned(), body: None });
    form.hover(0, Some(Choice::Option("Table".to_owned())));
    assert_eq!(caption(&form), Preview { caption: "Table".to_owned(), body: Some("| a | b |".to_owned()) });
    form.hover(0, None);
    form.press_number(3);
    assert_eq!(caption(&form), Preview { caption: "Other".to_owned(), body: None });
    let effect = form.escape(None, &mut store());
    assert_eq!(sent(effect), [respond("A", 10, Some("reject"))]);
}

#[test]
fn shows_the_requested_command_while_another_read_tool_is_pending_with_one_card_and_short_buttons() {
    let entries = [
        tool_with("List .", AgentToolStatus::Running, None),
        permission(12, &format!("Run {COMMAND}"), command_options()),
    ];
    let lines: Vec<PermissionLine> =
        entries.iter().filter_map(|e| permission_line(e, &local_task("A"), "codex", &mut I18n::default())).collect();
    assert_eq!(lines.len(), 1);
    assert!(question_card(&entries).is_none());
    let view = &lines[0];
    assert_eq!(view.title, "codex wants to run a command");
    assert_eq!(view.command.as_deref(), Some(COMMAND));
    let labels: Vec<&str> = view.answers.iter().map(|a| a.label.as_str()).collect();
    assert_eq!(labels, ["Allow Once", "Always Allow", "Decline", "Decline and Stop"]);
    assert_eq!(view.answers[1].title, format!("Always Allow `{COMMAND}`"));
}

#[test]
fn sends_the_original_kind_option_id_then_collapses_the_resolved_card() {
    for option in command_options() {
        let pending = permission(12, &format!("Run {COMMAND}"), command_options());
        let view = line(&pending);
        let answer = view.answers.iter().find(|a| a.title == option.name).expect("its button");
        let mut store = store();
        let mut responding = Responding::default();
        let effect = responding.answer(&mut store, "A", 12, answer.option.as_deref());
        assert_eq!(sent(effect), [respond("A", 12, Some(option.id.as_str()))]);
        let AgentEntry::Permission { id, title, options, questions, .. } = pending else { unreachable!() };
        let resolved = AgentEntry::Permission { id, title, options, questions, chosen: Some(option.name.clone()) };
        let view = line(&resolved);
        assert_eq!(view.command, None);
        assert!(view.answers.is_empty());
        let record = view.record.expect("the record of the answer");
        assert_eq!(record.subject, "python3 …");
        assert!(!record.subject.contains("write_text"));
        assert!(!record.label.contains("write_text"));
    }
}

#[test]
fn is_answered_from_its_line_too_and_one_without_options_can_be_cancelled() {
    let mut store = store();
    let a = permission(
        3,
        "Run ls d:/project",
        vec![
            permission_option("once", "Allow Once", "allow_once"),
            permission_option("reject", "Reject", "reject_once"),
        ],
    );
    let view = line(&a);
    let labels: Vec<&str> = view.answers.iter().map(|a| a.label.as_str()).collect();
    assert_eq!(labels, ["Allow Once", "Decline"]);
    let mut responding = Responding::default();
    let effect = responding.answer(&mut store, "A", 3, view.answers[0].option.as_deref());
    let Some(AgentEffect::Call { ticket: Some(ticket), call }) = effect else { panic!("an answer") };
    assert_eq!(call, respond("A", 3, Some("once")));
    // One answer at a time.
    assert!(responding.busy());
    assert!(responding.answer(&mut store, "A", 3, Some("reject")).is_none());
    responding.answered(ticket, Ok(()));
    assert!(!responding.busy());

    let b = permission(4, "The agent asks for permission", Vec::new());
    let view = line(&b);
    assert_eq!(view.title, "Allow tool call?");
    let cancel: Vec<(&str, Option<&str>)> =
        view.answers.iter().map(|a| (a.label.as_str(), a.option.as_deref())).collect();
    assert_eq!(cancel, [("Cancel", None)]);
    let effect = Responding::default().answer(&mut store, "B", 4, None);
    assert_eq!(sent(effect), [respond("B", 4, None)]);
}
