use super::*;
use crate::platform::storage::UiStorage;
use crate::vcs::clone_form::{BROWSE, PARENT_KEY, RECENT_URLS_KEY};
use crate::workbench::tests::click_on;

/// The text of the dialog's Directory row.
fn directory(dialog: &Entity<GitFormDialog>, cx: &mut VisualTestContext) -> Option<String> {
    rows(dialog, cx).into_iter().find_map(|row| match row {
        FormRow::Text { id: "dir", value, .. } => Some(value),
        _ => None,
    })
}

fn errors(dialog: &Entity<GitFormDialog>, cx: &mut VisualTestContext) -> Vec<String> {
    rows(dialog, cx)
        .into_iter()
        .filter_map(|row| match row {
            FormRow::Error(text) => Some(text),
            _ => None,
        })
        .collect()
}

/// Get from Version Control…, in a window with no project, clones a repository given by a local
/// path: the directory follows the URL in the folder Browse… chose, a repository that does not exist
/// fails in the dialog, which stays open, and a clone that succeeds closes it and opens the clone as
/// the project. The folder and the URL are remembered for the next clone.
#[gpui_kit::test]
async fn clone_clones_into_the_folder_chosen_and_opens_the_clone(cx: &mut TestAppContext) {
    let _home = start(cx);
    let sources = tempfile::tempdir().expect("a temporary folder");
    let sources = sources.path().canonicalize().expect("the folder exists");
    let source = sources.join("library");
    std::fs::create_dir_all(&source).expect("the folder is made");
    init_repo(&source);
    let place = tempfile::tempdir().expect("a temporary folder");
    let parent = place.path().canonicalize().expect("the folder exists");
    let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
    cx.update(|window, _| window.activate_window());

    let missing = sources.join("missing.git").to_string_lossy().into_owned();
    run_action(cx, "vcs.clone", Some(json!(missing)));
    let dialog = wait_for_form(&workbench, "Clone Repository", cx);
    assert!(rows(&dialog, cx).contains(&FormRow::Hint("The directory must be an absolute path".to_owned())));
    assert!(!can(&dialog, OK, cx), "a directory is needed first");

    press(&dialog, BROWSE, cx);
    wait_long(cx, "the folder is asked for", |cx| cx.did_prompt_for_paths());
    let chosen = parent.clone();
    cx.simulate_path_prompt_response(move |_| Some(vec![chosen]));
    let expected = parent.join("missing").to_string_lossy().into_owned();
    wait_long(cx, "the directory follows the folder chosen", |cx| directory(&dialog, cx) == Some(expected.clone()));
    press(&dialog, OK, cx);
    wait_long(cx, "the failure shows", |cx| !errors(&dialog, cx).is_empty());
    assert!(workbench.read_with(cx, |w, _| w.git_form_dialog()).as_ref() == Some(&dialog), "the dialog stays open");

    let url = source.to_string_lossy().into_owned();
    dialog.update(cx, |dialog, cx| dialog.set_text("url", &url, cx));
    let target = parent.join("library");
    assert_eq!(directory(&dialog, cx), Some(target.to_string_lossy().into_owned()));
    press(&dialog, OK, cx);
    wait_long(cx, "the clone is asked about", |cx| workbench.read_with(cx, |w, _| w.trust_dialog().is_some()));
    assert!(no_form(&workbench, cx), "the dialog closes once cloned");
    assert_eq!(read(&target.join("a.txt")), "one\n", "the clone holds the repository's files");
    click_on(cx, "trust-button-project.trust");
    wait_long(cx, "the clone opens", |cx| {
        workbench.read_with(cx, |w, cx| w.project_root(cx)).is_some_and(|root| root == target)
    });
    let stored = |key: &str, cx: &mut VisualTestContext| {
        cx.update(|_, cx| cx.global::<UiStorage>().get(key).map(str::to_owned)).unwrap_or_default()
    };
    assert_eq!(stored(PARENT_KEY, cx), parent.to_string_lossy());
    assert!(stored(RECENT_URLS_KEY, cx).contains(&url), "the URL is remembered");
    cx.update(|_, cx| cx.global::<crate::backend::Backend>().host.close_project("w1"));
}
