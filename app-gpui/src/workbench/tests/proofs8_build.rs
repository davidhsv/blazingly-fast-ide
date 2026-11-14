use super::*;
use crate::platform::actions::{ActionRegistry, is_action_enabled};
use crate::platform::context::window_context;
use crate::platform::keymap::KeymapState;
use crate::run_producers::context_run::{ContextMenu, MenuGroupContext};
use crate::vcs_ops::menu::{ActionItem, MenuItem};
use crate::workbench::tool_windows::{Anchor, Stripe};

/// In a project window Build Project, Build Module, Rebuild Project and Build Artifacts are registered
/// and enabled; the IDEA macOS keymap gives Build Project ⌘F9 and Build Module ⇧⌘F9; Build is a
/// bottom tool window with its own icon and no stripe button until it is used; the Project view's
/// menu on a folder offers Build Module for that folder.
#[gpui_kit::test]
async fn registers_the_build_menu_actions_shortcuts_and_an_on_use_bottom_tool_window(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("core/src/A.java", "class A {}\n")]);
    for id in ["build.buildProject", "build.buildModule", "build.rebuildProject", "build.buildArtifacts"] {
        let enabled = cx.update(|window, cx| {
            let action = cx.global::<ActionRegistry>().get(id).unwrap_or_else(|| panic!("{id} is registered"));
            is_action_enabled(&action.meta, &window_context(window, cx))
        });
        assert!(enabled, "{id} is enabled");
    }

    let keymap = KeymapState::new("intellij-macos");
    assert!(keymap.bindings_for("build.buildProject").iter().any(|key| key == "Meta+F9"));
    assert!(keymap.bindings_for("build.buildModule").iter().any(|key| key == "Meta+Shift+F9"));

    let (anchor, stripe, icon, has_button) = workbench.read_with(cx, |w, _| {
        let layout = w.tool_windows().expect("a project is open");
        let build = layout.window(BUILD_TOOL_WINDOW).expect("the Build tool window is registered");
        (build.anchor, build.stripe, build.icon.clone(), layout.has_stripe_button(build))
    });
    assert_eq!((anchor, stripe, icon.as_str()), (Anchor::Bottom, Stripe::OnUse, "toolwindow-build"));
    assert!(!has_button, "an on-use window has no stripe button before it is opened");

    let core = root.join("core").to_string_lossy().into_owned();
    let paths = [core.clone()];
    let dirs = [true];
    let rows = workbench.update_in(cx, |w, window, cx| {
        let ctx = MenuGroupContext { paths: &paths, dirs: &dirs, ..MenuGroupContext::default() };
        w.group_rows(ContextMenu::ProjectView, &ctx, None, window, cx)
    });
    let build_module = rows.iter().find_map(|row| match row {
        MenuItem::Action(ActionItem { action: "build.buildModule", arg, .. }) => Some(arg.clone()),
        _ => None,
    });
    assert_eq!(build_module, Some(Some(serde_json::json!(core))), "the Project view offers Build Module");
}
