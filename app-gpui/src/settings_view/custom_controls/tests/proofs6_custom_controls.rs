use super::*;

/// Tools › Terminal: the Shell path list offers the shells installed on this machine once they are
/// read.
#[gpui_kit::test]
async fn the_shell_path_list_offers_the_installed_shells(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (workbench, cx) = window(cx);
    let dialog = open_page(cx, &workbench, pages::terminal::PAGE);
    let view = dialog.read_with(cx, |view, _| view.custom_control(pages::terminal::PAGE, "Shell path").cloned());
    let view: Entity<pages::terminal::ShellPathView> =
        view.and_then(|view| view.downcast().ok()).expect("the Shell path control");
    let expected = ide_terminal::available_shells();
    assert!(!expected.is_empty(), "this machine has a shell");
    settle(cx, |cx| view.read_with(cx, |view, _| view.shells() == expected.as_slice()));
}
