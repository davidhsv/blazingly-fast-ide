use super::*;

// src/features/vcs/__tests__/stashSelectedFiles.test.ts › Stash Selected Files in the changes tree › targets the selected files
#[test]
fn targets_the_selected_files() {
    let items = menu(&[modified("src/a.ts"), modified("b.md")]);
    let stash = action(&items, "vcs.stashSelected").expect("Stash Selected Files… is offered");
    assert_eq!(stash.arg(), Some(&json!(["/p/src/a.ts", "/p/b.md"])));
    assert!(!stash.is_disabled());
}

// src/features/vcs/__tests__/stashSelectedFiles.test.ts › Stash Selected Files in the changes tree › is disabled when no file is selected
#[test]
fn is_disabled_when_no_file_is_selected() {
    let items = menu(&[]);
    assert_eq!(action(&items, "vcs.stashSelected").map(Item::is_disabled), Some(true));
}
