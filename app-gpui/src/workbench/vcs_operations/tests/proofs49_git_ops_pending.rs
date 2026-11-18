use super::*;
use crate::ui::popup_menu::PopupMenu;
use crate::vcs_ops::git_ops::TagCommand;
use crate::workbench::notification_balloons::EntryAction;
use crate::workbench::tests::context_is_on;
use gpui_kit::{Modifiers, MouseButton};

/// A trusted project without a repository, open in an active window.
fn plain_project(
    cx: &mut TestAppContext,
) -> (Entity<Workbench>, &mut VisualTestContext, [tempfile::TempDir; 2], PathBuf) {
    let home = start(cx);
    let folder = tempfile::tempdir().expect("a temporary folder");
    let root = folder.path().canonicalize().expect("the folder exists");
    std::fs::write(root.join("a.txt"), "one\n").expect("the file is written");
    cx.update(|cx| cx.global::<crate::backend::Backend>().host.trust.decide(&root, true, false))
        .expect("the project is trusted");
    let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
    cx.update(|window, _| window.activate_window());
    assert!(!context_is_on(cx, "projectLocal"), "no project is open");
    workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
    wait_long(cx, "the project opens", |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
    (workbench, cx, [home, folder], root)
}

/// The rows of the root of `menu`, `-` for a separator.
fn menu_rows<C: Clone + 'static>(menu: &Entity<PopupMenu<C>>, cx: &mut VisualTestContext) -> Vec<String> {
    let lines = menu.read_with(cx, |menu, _| menu.lines());
    lines[0].iter().map(|line| line.trim_start_matches("> ").to_owned()).collect()
}

/// Clicks the row `label` of the root of `menu`.
fn choose<C: Clone + 'static>(menu: &Entity<PopupMenu<C>>, label: &str, cx: &mut VisualTestContext) {
    let rows = menu_rows(menu, cx);
    let index = rows.iter().position(|row| row == label).unwrap_or_else(|| panic!("{label} in {rows:?}"));
    let selector: &'static str = Box::leak(format!("menu-row-0-{index}").into_boxed_str());
    let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
    cx.simulate_click(bounds.center(), Modifiers::default());
    cx.run_until_parked();
}

/// Shows the Git window, finds `tag` with the Branches pane's search and opens its menu with the
/// right button.
fn open_tag_menu(
    workbench: &Entity<Workbench>,
    tag: &str,
    cx: &mut VisualTestContext,
) -> Entity<PopupMenu<TagCommand>> {
    if !workbench.read_with(cx, |w, _| w.is_tool_window_visible(GIT_TOOL_WINDOW)) {
        run_action(cx, "toolwindow.git", None);
    }
    wait_long(cx, "the Git window", |cx| workbench.read_with(cx, |w, _| w.git_log_view().is_some()));
    let log = workbench.read_with(cx, |w, _| w.git_log_view()).expect("the Git window holds the log");
    let pane = log.read_with(cx, |log, _| log.branches_pane().cloned()).expect("the log has the Branches pane");
    let index =
        |cx: &mut VisualTestContext| pane.read_with(cx, |pane, _| pane.rows().iter().position(|row| row.label == tag));
    // The Tags group starts closed: the search opens it on the tag. A search typed before stays.
    if index(cx).is_none() && pane.read_with(cx, |pane, _| pane.query().is_empty()) {
        let field = pane.read_with(cx, |pane, _| pane.search_field().clone());
        cx.update(|window, cx| window.focus(&field.focus_handle(cx), cx));
        cx.simulate_input(tag);
    }
    wait_long(cx, "the tag is listed", |cx| index(cx).is_some());
    let selector: &'static str = Box::leak(format!("branches-row-{}", index(cx).unwrap_or_default()).into_boxed_str());
    cx.run_until_parked();
    let at = cx.debug_bounds(selector).expect("the tag's row is drawn").center();
    cx.simulate_mouse_down(at, MouseButton::Right, Modifiers::default());
    cx.simulate_mouse_up(at, MouseButton::Right, Modifiers::default());
    wait_long(cx, "the tag's menu", |cx| workbench.read_with(cx, |w, _| w.open_menu::<TagCommand>().is_some()));
    workbench.read_with(cx, |w, _| w.open_menu::<TagCommand>()).expect("the tag's menu is open")
}

/// Presses the button `label` of the notification titled `title`.
fn press_notice_button(workbench: &Entity<Workbench>, title: &str, label: &str, cx: &mut VisualTestContext) {
    let notice = notice_titled(workbench, title, cx).unwrap_or_else(|| panic!("the notice {title} is shown"));
    let action = notice.actions.iter().find(|action| action.label == label).cloned();
    let action = action.unwrap_or_else(|| panic!("{label} in {:?}", notice.actions));
    workbench.update_in(cx, |w, window, cx| w.run_notification_action(EntryAction::Own(action), window, cx));
    cx.run_until_parked();
}

/// The labels of the buttons of the notification titled `title`.
fn notice_buttons(workbench: &Entity<Workbench>, title: &str, cx: &mut VisualTestContext) -> Vec<String> {
    let notice = notice_titled(workbench, title, cx).unwrap_or_else(|| panic!("the notice {title} is shown"));
    notice.actions.iter().map(|action| action.label.clone()).collect()
}

/// Create Git Repository… needs an open local project (`projectLocal` is on only then), asks for a
/// folder and runs `git init` there, which says where; the window then reads the project as a
/// repository and the Git actions turn on. A folder git cannot init in is reported.
#[gpui_kit::test]
async fn create_git_repository_initializes_the_folder_chosen_and_the_window_reads_it(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root) = plain_project(cx);
    assert!(context_is_on(cx, "projectLocal"), "a local project is open");
    assert!(!context_is_on(cx, "vcsAvailable"), "the folder is no repository yet");

    run_action(cx, "vcs.createRepository", None);
    wait_long(cx, "the folder is asked for", |cx| cx.did_prompt_for_paths());
    let chosen = root.clone();
    cx.simulate_path_prompt_response(move |_| Some(vec![chosen]));
    wait_for_notice(&workbench, &format!("Created Git repository in {}", root.to_string_lossy()), cx);
    assert!(root.join(".git").is_dir(), "the folder is a repository");
    wait_long(cx, "the window reads the repository", |cx| {
        context_is_on(cx, "vcsAvailable") && workbench.read_with(cx, |w, _| w.vcs_state().is_some_and(|s| s.available))
    });

    run_action(cx, "vcs.createRepository", None);
    wait_long(cx, "the folder is asked for again", |cx| cx.did_prompt_for_paths());
    let file = root.join("a.txt");
    cx.simulate_path_prompt_response(move |_| Some(vec![file]));
    wait_for_notice(&workbench, "Couldn't create Git repository", cx);
    let failed = notice_titled(&workbench, "Couldn't create Git repository", cx).expect("the failure is told");
    assert!(failed.body.is_some_and(|body| !body.is_empty()), "the failure says why");

    run_action(cx, "file.closeProject", None);
    assert!(!context_is_on(cx, "projectLocal"), "no project is open any more");
    let refused = cx.update(|window, cx| {
        crate::platform::actions::execute_action_or_throw("vcs.createRepository", None, window, cx)
    });
    assert!(refused.is_err(), "Create Git Repository needs a project");
    cx.update(|_, cx| cx.global::<crate::backend::Backend>().host.close_project("w1"));
}

/// Stash Selected Files… without a selection tells the user to select the files first; with the
/// files of its argument it stashes them only, under the message typed, and the other change stays.
#[gpui_kit::test]
async fn stash_selected_files_stashes_only_the_files_given(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root) = git_project(cx, |root| {
        init_repo(root);
        std::fs::write(root.join("b.txt"), "two\n").expect("the file is written");
        git_in(root, &["add", "."]);
        git_in(root, &["commit", "-q", "-m", "second"]);
    });
    std::fs::write(root.join("a.txt"), "changed a\n").expect("the file is written");
    std::fs::write(root.join("b.txt"), "changed b\n").expect("the file is written");

    run_action(cx, "vcs.stashSelected", None);
    wait_for_notice(&workbench, "Select the files to stash in the Commit view first", cx);
    assert!(no_form(&workbench, cx), "nothing is asked");

    let a = root.join("a.txt").to_string_lossy().into_owned();
    run_action(cx, "vcs.stashSelected", Some(json!([a])));
    let dialog = wait_for_form(&workbench, "Stash", cx);
    assert!(rows(&dialog, cx).contains(&FormRow::Hint("Current branch: main".to_owned())));
    dialog.update(cx, |dialog, cx| dialog.set_text("message", "only a", cx));
    press(&dialog, OK, cx);
    wait_for_notice(&workbench, "Changes stashed", cx);
    assert_eq!(read(&root.join("a.txt")), "one\n", "the file given is stashed");
    assert_eq!(read(&root.join("b.txt")), "changed b\n", "the other change stays");
    assert!(git_in(&root, &["stash", "list"]).contains("only a"));
    assert_eq!(git_in(&root, &["stash", "show", "--name-only", "stash@{0}"]), "a.txt");
}

/// The Git log's Branches pane offers a tag's menu; Delete deletes the tag and the notification's
/// Restore puts it back where it pointed.
#[gpui_kit::test]
async fn deleting_a_tag_from_its_menu_offers_restore_which_puts_it_back(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root) = git_project(cx, |root| {
        init_repo(root);
        git_in(root, &["tag", "-a", "v1", "-m", "first release"]);
    });
    let before = git_in(&root, &["rev-parse", "v1"]);
    let menu = open_tag_menu(&workbench, "v1", cx);
    assert_eq!(
        menu_rows(&menu, cx),
        [
            "Checkout",
            "New Branch from 'v1'…",
            "-",
            "Compare with Current",
            "Show Diff with Working Tree",
            "Show History",
            "-",
            "Delete"
        ]
    );
    choose(&menu, "Delete", cx);
    wait_for_notice(&workbench, "Deleted tag v1", cx);
    assert_eq!(git_in(&root, &["tag", "-l"]), "", "the tag is gone");
    assert_eq!(notice_buttons(&workbench, "Deleted tag v1", cx), ["Restore"], "no remote to delete it on");

    press_notice_button(&workbench, "Deleted tag v1", "Restore", cx);
    wait_for_notice(&workbench, "Restored tag v1", cx);
    assert_eq!(git_in(&root, &["rev-parse", "v1"]), before, "the tag points where it did");
    assert_eq!(git_in(&root, &["cat-file", "-t", "v1"]), "tag", "the annotated tag object is back");
}

/// With a remote, a tag's menu pushes it there, and the notification of its deletion offers Delete
/// on Remote, which deletes it on the remote too.
#[gpui_kit::test]
async fn a_tag_is_pushed_from_its_menu_and_deleted_on_the_remote_from_the_notification(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root, parent) = origin_project(cx);
    git_in(&root, &["tag", "v2"]);
    let bare = parent.join("origin.git");
    let remote_tags = || git_in(&bare, &["tag", "-l"]);
    wait_long(cx, "the remote branches are read", |cx| {
        workbench.read_with(cx, |w, _| w.vcs_state().is_some_and(|s| s.branches.iter().any(|b| b.remote)))
    });

    let menu = open_tag_menu(&workbench, "v2", cx);
    assert!(menu_rows(&menu, cx).contains(&"Push to origin".to_owned()));
    choose(&menu, "Push to origin", cx);
    wait_long(cx, "the tag reaches the remote", |_| remote_tags() == "v2");
    wait_for_notice(&workbench, "Pushed tag v2 to origin", cx);

    let menu = open_tag_menu(&workbench, "v2", cx);
    choose(&menu, "Delete", cx);
    wait_for_notice(&workbench, "Deleted tag v2", cx);
    assert_eq!(notice_buttons(&workbench, "Deleted tag v2", cx), ["Restore", "Delete on Remote"]);
    press_notice_button(&workbench, "Deleted tag v2", "Delete on Remote", cx);
    wait_long(cx, "the tag is deleted on the remote", |_| remote_tags().is_empty());
    assert_eq!(git_in(&root, &["tag", "-l"]), "", "the local tag stays deleted");
}

/// The Unstash dialog's Drop Stash asks first; Drop drops the chosen stash and the dialog lists the
/// stashes left, and a question answered Cancel drops nothing.
#[gpui_kit::test]
async fn drop_stash_asks_then_drops_the_chosen_stash_and_lists_the_rest(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root) = git_project(cx, init_repo);
    for (text, message) in [("first\n", "older"), ("second\n", "newer")] {
        std::fs::write(root.join("a.txt"), text).expect("the file is written");
        git_in(&root, &["stash", "push", "-q", "-m", message]);
    }
    run_action(cx, "vcs.unstash", None);
    let dialog = wait_for_form(&workbench, "Unstash Changes", cx);
    assert_eq!(list(&dialog, cx).len(), 2);
    assert!(can(&dialog, "drop", cx), "the chosen stash can be dropped");

    press(&dialog, "drop", cx);
    let question = wait_for_prompt(&workbench, cx);
    assert_eq!(question.read_with(cx, |dialog, _| dialog.title().to_owned()), "Drop Stash");
    let message = question.read_with(cx, |dialog, _| dialog.message_text().map(str::to_owned)).unwrap_or_default();
    assert!(
        message.starts_with("Drop stash@{0} “") && message.ends_with("newer”? This cannot be undone."),
        "{message}"
    );
    cx.simulate_keystrokes("escape");
    let dialog = wait_for_form(&workbench, "Unstash Changes", cx);
    assert_eq!(list(&dialog, cx).len(), 2, "Cancel drops nothing");

    press(&dialog, "drop", cx);
    wait_for_prompt(&workbench, cx);
    cx.simulate_keystrokes("enter");
    wait_for_notice(&workbench, "Dropped stash@{0}", cx);
    let stashes = git_in(&root, &["stash", "list"]);
    assert!(stashes.lines().count() == 1 && stashes.contains("older"), "{stashes}");
    let dialog = wait_for_form(&workbench, "Unstash Changes", cx);
    wait_long(cx, "the stashes left are listed", |cx| list(&dialog, cx).len() == 1);
    assert!(list(&dialog, cx)[0].label.ends_with("older"));
}

/// Worktrees… › Move Worktree… asks for a folder and, once confirmed, moves the linked worktree
/// into it; the moved worktree is selected in the list. The main worktree cannot be moved.
#[gpui_kit::test]
async fn move_worktree_moves_a_linked_worktree_into_the_folder_chosen(cx: &mut TestAppContext) {
    let place = tempfile::tempdir().expect("a temporary folder");
    let place = place.path().canonicalize().expect("the folder exists");
    let linked = place.join("feature-wt");
    let elsewhere = place.join("elsewhere");
    std::fs::create_dir_all(&elsewhere).expect("the folder is made");
    let linked_text = linked.to_string_lossy().into_owned();
    let (workbench, cx, _folders, root) = git_project(cx, |root| {
        init_repo(root);
        git_in(root, &["worktree", "add", "-q", "-b", "feature", &linked_text]);
    });
    run_action(cx, "vcs.worktrees", None);
    let dialog = wait_for_form(&workbench, "Worktrees", cx);
    let main = format!("w:{}", root.to_string_lossy());
    let key = format!("w:{linked_text}");
    wait_long(cx, "the worktrees are listed", |cx| list(&dialog, cx).iter().any(|row| row.key == key));
    dialog.update(cx, |dialog, cx| dialog.choose("worktrees", &main, cx));
    assert!(!can(&dialog, "move", cx), "the main worktree stays where it is");
    dialog.update(cx, |dialog, cx| dialog.choose("worktrees", &key, cx));
    assert!(can(&dialog, "move", cx));

    press(&dialog, "move", cx);
    wait_long(cx, "the folder is asked for", |cx| cx.did_prompt_for_paths());
    let chosen = elsewhere.clone();
    cx.simulate_path_prompt_response(move |_| Some(vec![chosen]));
    let question = wait_for_prompt(&workbench, cx);
    assert_eq!(question.read_with(cx, |dialog, _| dialog.title().to_owned()), "Move Worktree");
    let moved = elsewhere.join("feature-wt");
    let expected = format!("Move the worktree at {linked_text} to {}?", moved.to_string_lossy());
    assert_eq!(question.read_with(cx, |dialog, _| dialog.message_text().map(str::to_owned)), Some(expected));
    cx.simulate_keystrokes("enter");
    let dialog = wait_for_form(&workbench, "Worktrees", cx);
    let moved_key = format!("w:{}", moved.to_string_lossy());
    wait_long(cx, "the moved worktree is listed", |cx| list(&dialog, cx).iter().any(|row| row.key == moved_key));
    assert!(moved.join("a.txt").is_file() && !linked.exists(), "the worktree's folder moved");
    assert_eq!(git_head(&moved), "feature");
    let selected = rows(&dialog, cx).into_iter().find_map(|row| match row {
        FormRow::List { selected, .. } => selected,
        _ => None,
    });
    assert_eq!(selected, Some(moved_key), "the moved worktree is selected");
    press(&dialog, "close", cx);
}
