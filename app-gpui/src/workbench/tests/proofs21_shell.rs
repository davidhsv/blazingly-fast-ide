//! What the window shows of the app itself (the build's version, the configuration folder, the title bar of this
//! platform's windows) and the title bar's empty space, which the system drags the window by.

use super::*;
use gpui_kit::{Modifiers, MouseButton, point, px};

/// The Welcome screen of a window with no project shows the build's version, and Help › About lists the
/// configuration folder with Show Config beside it.
#[gpui_kit::test]
async fn the_welcome_screen_shows_the_builds_version_and_about_offers_the_configuration_folder(
    cx: &mut TestAppContext,
) {
    let _home = start(cx);
    let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
    cx.update(|window, _| window.activate_window());
    wait_long(cx, "the Welcome screen", |cx| workbench.read_with(cx, |w, _| w.welcome().is_some()));
    let version =
        workbench.read_with(cx, |w, cx| w.welcome().and_then(|welcome| welcome.read(cx).version().map(str::to_owned)));
    assert_eq!(version.as_deref(), Some(env!("CARGO_PKG_VERSION")));

    run_action(cx, "app.about", None);
    let about = workbench.read_with(cx, |w, _| w.about_dialog()).expect("About is open");
    let config = cx.update(|_, cx| cx.global::<Backend>().dirs.config.to_string_lossy().into_owned());
    let facts = about.update(cx, |a, cx| a.facts(cx));
    assert!(facts.contains(&("Configuration".to_owned(), config)), "{facts:?}");
    // Showing the folder is the file manager's, which the test platform does not have: the link is seen drawn.
    assert!(cx.debug_bounds("about-link-config").is_some(), "Show Config is offered");
    click_on(cx, "about-close");
}

/// A project window's title bar takes this platform's frame: on macOS the window's own buttons overlay its left
/// edge, so its first entry starts past their room; elsewhere the system's frame carries them and the entry starts
/// at the bar's edge. The bar has no main menu button: the window keeps the system's menu bar.
#[gpui_kit::test]
async fn the_title_bar_takes_the_frame_of_this_platforms_windows(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _root, _workbench, cx) = search_project(cx);
    cx.run_until_parked();
    let bar = cx.debug_bounds("title-bar").expect("the bar is drawn");
    let project = cx.debug_bounds("title:project").expect("the project widget is drawn");
    let room = f32::from(project.left() - bar.left());
    if cfg!(target_os = "macos") {
        assert!(room >= WINDOW_BUTTONS_WIDTH, "room for the window buttons: {room}");
    } else {
        assert!(room < WINDOW_BUTTONS_WIDTH, "no room for buttons the frame carries: {room}");
    }
    assert!(cx.debug_bounds("title:mainMenu").is_none(), "no main menu button");
    cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
}

/// A window is opened with a transparent title bar that the system moves it by: the drag is left to AppKit, the
/// window is movable. In the window, a press, a drag and a release on the bar's empty space, and a double click
/// there, reach none of the bar's entries: no menu or dialog opens.
#[gpui_kit::test]
async fn the_title_bars_empty_space_is_left_to_the_system_to_move_the_window(cx: &mut TestAppContext) {
    let options = window_options();
    let titlebar = options.titlebar.as_ref().expect("the window has a title bar of its own");
    assert!(titlebar.appears_transparent, "the app draws the bar under the window's buttons");
    assert!(options.is_movable, "the window can be moved");
    assert!(!options.app_owns_titlebar_drag, "AppKit drags the window by its title bar");

    let _home = start(cx);
    let (_folder, _root, workbench, cx) = search_project(cx);
    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();
    let bar = cx.debug_bounds("title-bar").expect("the bar is drawn");
    // The bar's leading padding: no entry is drawn there on any platform.
    let empty = point(bar.left() + px(4.0), bar.center().y);
    cx.simulate_mouse_down(empty, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_move(point(empty.x + px(2.0), empty.y + px(4.0)), MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_up(point(empty.x + px(2.0), empty.y + px(4.0)), MouseButton::Left, Modifiers::default());
    cx.run_until_parked();
    let nothing_opened = |cx: &mut VisualTestContext| {
        cx.debug_bounds("menu-row-0-0").is_none()
            && !context_is_on(cx, "popupOpen")
            && workbench.read_with(cx, |w, _| w.about_dialog().is_none() && w.vcs_prompt().is_none())
    };
    assert!(nothing_opened(cx), "a drag on the empty space opens nothing");
    cx.simulate_event(gpui_kit::MouseDownEvent {
        position: empty,
        modifiers: Modifiers::default(),
        button: MouseButton::Left,
        click_count: 2,
        first_mouse: false,
    });
    cx.simulate_event(gpui_kit::MouseUpEvent {
        position: empty,
        modifiers: Modifiers::default(),
        button: MouseButton::Left,
        click_count: 2,
    });
    cx.run_until_parked();
    assert!(nothing_opened(cx), "a double click on the empty space opens nothing");
    cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
}
