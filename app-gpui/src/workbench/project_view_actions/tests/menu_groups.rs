use super::*;
use crate::workbench::http_client::NEW_REQUEST_TEMPLATE;

/// New › HTTP Request in a folder's Project view menu asks for the file's name and creates the request file in that
/// folder with the example request.
#[gpui_kit::test]
async fn new_http_request_in_a_folders_project_view_menu_creates_the_request_file_there(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, "w1", &[("api/a.txt", "a\n"), ("b.txt", "b\n")]);
    let api = root.join("api");
    select_in_tree(&workbench, &api, cx);
    right_click_row(&api, cx);
    choose_in_menu(&workbench, &["New", "HTTP Request"], cx);
    assert!(workbench.read_with(cx, |w, _| w.new_file_dialog().is_some()), "the name is asked for");
    cx.simulate_input("requests");
    cx.simulate_keystrokes("enter");
    let created = api.join("requests.http");
    wait_long(cx, "the request file", |_| {
        std::fs::read_to_string(&created).ok().as_deref() == Some(NEW_REQUEST_TEMPLATE)
    });
    close_project(cx);
}
