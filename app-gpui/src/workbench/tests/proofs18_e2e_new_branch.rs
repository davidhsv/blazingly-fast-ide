use super::proofs18_e2e_fixtures::primary;
use super::*;
use crate::vcs::prompt_dialogs::PromptDialog;

fn field_text(dialog: &Entity<PromptDialog>, cx: &mut VisualTestContext) -> String {
    let field = dialog.read_with(cx, |dialog, _| dialog.field().cloned()).expect("the dialog has a field");
    field.read_with(cx, |field, _| field.text().to_owned())
}

fn can_create(dialog: &Entity<PromptDialog>, cx: &mut VisualTestContext) -> bool {
    dialog.update(cx, |dialog, cx| dialog.can_confirm(cx))
}

fn errors(dialog: &Entity<PromptDialog>, cx: &mut VisualTestContext) -> Vec<String> {
    dialog.update(cx, |dialog, cx| dialog.errors(cx))
}

/// Empties the dialog's field by typing.
fn clear(dialog: &Entity<PromptDialog>, cx: &mut VisualTestContext) {
    for _ in 0..field_text(dialog, cx).chars().count() {
        cx.simulate_keystrokes("backspace");
    }
    cx.run_until_parked();
}

type Opened<'a> = (Entity<Workbench>, &'a mut VisualTestContext, [tempfile::TempDir; 2], PathBuf, Entity<PromptDialog>);

/// New Branch… on a repository with `main` only: the Create New Branch dialog, once it shows.
fn open_new_branch_dialog(cx: &mut TestAppContext) -> Opened<'_> {
    let (workbench, cx, folders, root) = git_project(cx, init_repo);
    run_action(cx, "vcs.newBranch", None);
    let dialog = wait_for_prompt(&workbench, cx);
    assert_eq!(dialog.read_with(cx, |dialog, _| dialog.title().to_owned()), "Create New Branch");
    (workbench, cx, folders, root, dialog)
}

/// Typed one character after the other, `fix login  redirect` reads `fix-login-redirect`: no problem
/// shows, Create can be pressed, and a click on it creates and checks out that branch.
#[gpui_kit::test]
async fn a_typed_space_becomes_a_separator_and_create_stays_enabled(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root, dialog) = open_new_branch_dialog(cx);
    clear(&dialog, cx);
    for typed in "fix login  redirect".chars() {
        cx.simulate_input(&typed.to_string());
    }
    cx.run_until_parked();
    assert_eq!(field_text(&dialog, cx), "fix-login-redirect");
    assert!(errors(&dialog, cx).is_empty(), "no invalid characters");
    assert!(can_create(&dialog, cx), "Create is enabled");
    click_on(cx, "prompt-button-ok");
    wait_long(cx, "the new branch", |cx| branch_label(&workbench, cx) == "fix-login-redirect");
    assert_eq!(git_head(&root), "fix-login-redirect");
}

/// New Branch… at a commit of the log, named after the existing `main`: Create is disabled and the
/// unticked Overwrite existing branch is offered; ticked, Create is enabled. Another name takes the
/// offer away, and `main` again offers it unticked. Ticked again, Create moves `main` to the commit
/// and checks it out.
#[gpui_kit::test]
async fn overwrites_a_local_branch_only_after_explicitly_opting_in(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root) = git_project(cx, |root| {
        init_repo(root);
        std::fs::write(root.join("b.txt"), "b\n").expect("the file is written");
        git_in(root, &["add", "."]);
        git_in(root, &["commit", "-q", "-m", "second"]);
    });
    let from = git_in(&root, &["rev-parse", "HEAD~1"]);
    run_action(cx, "vcs.log.newBranch", Some(serde_json::json!(from)));
    let dialog = wait_for_prompt(&workbench, cx);
    let title = dialog.read_with(cx, |dialog, _| dialog.title().to_owned());
    assert!(title.starts_with("Create New Branch from "), "{title}");
    let overwrite_shown = |cx: &mut VisualTestContext| {
        cx.run_until_parked();
        cx.debug_bounds("prompt-overwrite").is_some()
    };

    retype(&dialog, "main", cx);
    assert!(!can_create(&dialog, cx), "Create is disabled");
    assert!(dialog.read_with(cx, |dialog, _| dialog.offers_overwrite()) && overwrite_shown(cx));
    click_on(cx, "prompt-overwrite");
    assert!(can_create(&dialog, cx), "ticked, Create is enabled");
    retype(&dialog, "new-branch", cx);
    assert!(!overwrite_shown(cx), "another name takes the offer away");
    retype(&dialog, "main", cx);
    assert!(overwrite_shown(cx));
    assert!(!can_create(&dialog, cx), "the offer comes back unticked");
    click_on(cx, "prompt-overwrite");
    click_on(cx, "prompt-button-ok");
    wait_long(cx, "main is overwritten", |_| git_in(&root, &["rev-parse", "main"]) == from);
    assert_eq!(git_head(&root), "main", "the branch is checked out");
    assert!(workbench.read_with(cx, |w, _| w.vcs_prompt().is_none()));
}

/// `ab` with the caret between the letters: a space typed there reads `a-b` with the caret after the
/// separator, so `x` typed next reads `a-xb`.
#[gpui_kit::test]
async fn a_space_typed_in_the_middle_keeps_the_caret_after_it(cx: &mut TestAppContext) {
    let (_workbench, cx, _folders, _root, dialog) = open_new_branch_dialog(cx);
    retype(&dialog, "ab", cx);
    cx.simulate_keystrokes("left");
    cx.simulate_input(" ");
    cx.run_until_parked();
    assert_eq!(field_text(&dialog, cx), "a-b");
    cx.simulate_input("x");
    cx.run_until_parked();
    assert_eq!(field_text(&dialog, cx), "a-xb");
}

/// `feature/add new thing` pasted into the emptied field reads `feature/add-new-thing`, and Create can
/// be pressed.
#[gpui_kit::test]
async fn pasted_text_has_its_spaces_replaced(cx: &mut TestAppContext) {
    let (_workbench, cx, _folders, _root, dialog) = open_new_branch_dialog(cx);
    clear(&dialog, cx);
    cx.update(|_, cx| cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string("feature/add new thing".to_owned())));
    cx.simulate_keystrokes(&primary("v"));
    cx.run_until_parked();
    assert_eq!(field_text(&dialog, cx), "feature/add-new-thing");
    assert!(can_create(&dialog, cx), "Create is enabled");
}
