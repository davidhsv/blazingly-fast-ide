use super::*;

/// Preview only shown again after it was gone opens at the offset recorded for the file and reports
/// the source line at that offset as the one on screen.
#[gpui_kit::test]
async fn reports_the_restored_line_when_shown_again_after_a_remount(cx: &mut TestAppContext) {
    let mut world = open(cx, MarkdownMode::Preview);
    let top = world.top_of(80);
    assert!(top > 0.0, "line 80 is laid out below the top");
    world.set_mode(MarkdownMode::Editor);
    assert!(world.host.read_with(world.cx, |host, _| host.preview().is_none()));
    world.change_live(|live| live.preview_scrolled(PATH, top + 5.0));

    let before = world.events.borrow().len();
    world.set_mode(MarkdownMode::Preview);
    assert!((world.scroll_top() - (top + 5.0)).abs() < 1.0, "{} for {}", world.scroll_top(), top + 5.0);
    assert_eq!(world.shown(), Some(80));
    assert_eq!(world.shown_events(before).last(), Some(&Some(80)));
}
