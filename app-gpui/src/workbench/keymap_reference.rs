//! Help › Keymap Reference: the dialog listing every bound action with its shortcuts, shown in the
//! window's dialog slot from any window (a project, a loose file, the Welcome screen), near the top of
//! the window as a light popup. It lists the keymap in effect when it opens; Escape, or leaving the
//! window, closes it and gives the keyboard back.

use super::Workbench;
use super::services_window::on_workbench;
use crate::app_models::shell::ReferencedAction;
use crate::app_views::about_dialog::KEYMAP_REFERENCE;
use crate::app_views::keymap_reference_dialog::{KeymapReferenceDialog, KeymapReferenceEvent, KeymapReferenceInput};
use crate::platform::actions::{ActionMeta, ActionRegistry, is_action_in_window, manifest};
use crate::platform::context::window_context;
use crate::platform::i18n::{I18n, Localization};
use crate::platform::input::Keymap;
use crate::platform::keymap::is_mac_host;
use gpui_kit::{App, AppContext, Context, Entity, Focusable, Window};
use std::collections::HashMap;

pub(super) fn register_actions(registry: &mut ActionRegistry) {
    registry.handle(KEYMAP_REFERENCE, |_, window, cx| on_workbench(window, cx, Workbench::show_keymap_reference));
}

/// What the reference lists for the window: the actions that exist in it, else the IDE's declared
/// ones (a binding of an action the window lacks keeps its title), and the keymap in effect.
fn reference_input(window: &Window, cx: &mut App) -> KeymapReferenceInput {
    let keymap = &cx.default_global::<Keymap>().0;
    let (bindings, preset) = (keymap.effective_bindings().to_vec(), keymap.preset_id().to_owned());
    let ctx = window_context(window, cx);
    let fallback;
    let i18n = match cx.try_global::<Localization>() {
        Some(localization) => &localization.0,
        None => {
            fallback = I18n::default();
            &fallback
        }
    };
    let describe = |meta: &ActionMeta| {
        let shown = meta.localized(i18n);
        ReferencedAction { title: shown.title, category: shown.category, hidden: meta.hidden }
    };
    let mut actions: HashMap<String, ReferencedAction> = HashMap::new();
    if let Some(registry) = cx.try_global::<ActionRegistry>() {
        for action in registry.all().filter(|action| is_action_in_window(&action.meta, &ctx)) {
            actions.insert(action.meta.id.clone(), describe(&action.meta));
        }
    }
    for (id, meta) in manifest() {
        actions.entry(id.clone()).or_insert_with(|| describe(meta));
    }
    KeymapReferenceInput { actions, bindings, preset, mac: is_mac_host() }
}

impl Workbench {
    /// The Keymap Reference dialog, while it is open.
    pub fn keymap_reference(&self) -> Option<Entity<KeymapReferenceDialog>> {
        self.vcs_dialog_view()
    }

    fn show_keymap_reference(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input = reference_input(window, cx);
        let dialog = cx.new(|cx| KeymapReferenceDialog::new(input, window, cx));
        let events = cx.subscribe_in(&dialog, window, |workbench, _, event: &KeymapReferenceEvent, window, cx| {
            let KeymapReferenceEvent::Closed = event;
            workbench.close_vcs_dialog(window, cx);
        });
        let focus = dialog.focus_handle(cx);
        self.open_vcs_dialog(dialog.into(), focus, events, window, cx);
        self.place_vcs_dialog_at_top(cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::Backend;
    use crate::workbench::tests::{click_on, run_action, start, wait_long};
    use crate::workbench::windows::{self, Target};
    use gpui_kit::{AnyWindowHandle, TestAppContext, VisualTestContext};

    fn categories(dialog: &Entity<KeymapReferenceDialog>, cx: &mut VisualTestContext) -> Vec<String> {
        dialog.read_with(cx, |dialog, _| dialog.categories().into_iter().map(str::to_owned).collect())
    }

    /// Help › Keymap Reference opens over the Welcome screen with the keymap's actions grouped by
    /// category, Navigate among them; Escape closes it and gives the keyboard back to the field
    /// that had it.
    #[gpui_kit::test]
    async fn renders_the_overlay(cx: &mut TestAppContext) {
        let _home = start(cx);
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        cx.update(|window, _| window.activate_window());
        cx.run_until_parked();
        let before = cx.update(|window, cx| window.focused(cx));
        run_action(cx, KEYMAP_REFERENCE, None);
        let dialog = workbench.read_with(cx, |w, _| w.keymap_reference()).expect("the reference is open");
        assert!(cx.debug_bounds("keymap-reference").is_some(), "the dialog is drawn");
        assert!(categories(&dialog, cx).iter().any(|category| category == "Navigate"), "{:?}", categories(&dialog, cx));
        let preset = cx.update(|_, cx| cx.global::<Keymap>().0.preset_id().to_owned());
        let meta = dialog.update(cx, |dialog, cx| dialog.meta_line(cx));
        assert!(meta.starts_with(&format!("Keymap: {preset} · ")), "{meta}");
        // Search Everywhere is bound in every preset, under its own title.
        let titles: Vec<String> = dialog.read_with(cx, |dialog, _| {
            dialog.groups().iter().flat_map(|group| group.entries.iter().map(|entry| entry.id.clone())).collect()
        });
        assert!(titles.iter().any(|id| id == "navigate.searchEverywhere"), "{titles:?}");
        let field = dialog.read_with(cx, |dialog, cx| dialog.filter_field().focus_handle(cx));
        assert!(cx.update(|window, _| field.is_focused(window)), "the filter field has the keyboard");

        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        assert!(workbench.read_with(cx, |w, _| w.keymap_reference()).is_none(), "Escape closes it");
        assert_eq!(cx.update(|window, cx| window.focused(cx)), before, "the keyboard is given back");
    }

    /// The filter typed in the reference of a project window keeps the matching actions only.
    #[gpui_kit::test]
    async fn filters_by_title_id_or_shortcut(cx: &mut TestAppContext) {
        let _home = start(cx);
        let folder = tempfile::tempdir().expect("a temporary folder");
        let root = folder.path().canonicalize().expect("the folder exists");
        std::fs::write(root.join("a.txt"), "a\n").expect("the file is written");
        cx.update(|cx| cx.global::<Backend>().host.trust.decide(&root, true, false)).expect("trusted");
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        cx.update(|window, _| window.activate_window());
        workbench.update_in(cx, |w, window, cx| w.open_project(root.clone(), window, cx));
        wait_long(cx, "the project opens", |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
        run_action(cx, KEYMAP_REFERENCE, None);
        let dialog = workbench.read_with(cx, |w, _| w.keymap_reference()).expect("the reference is open");
        cx.simulate_input("searchEverywhere");
        cx.run_until_parked();
        let ids: Vec<String> = dialog.read_with(cx, |dialog, _| {
            dialog.groups().iter().flat_map(|group| group.entries.iter().map(|entry| entry.id.clone())).collect()
        });
        assert!(ids.contains(&"navigate.searchEverywhere".to_owned()), "{ids:?}");
        assert!(ids.iter().all(|id| id.to_lowercase().contains("searcheverywhere")), "{ids:?}");
        click_on(cx, "keymap-filter-clear");
        let all = dialog.read_with(cx, |dialog, _| dialog.count());
        assert!(all > ids.len(), "Clear lists every action again");
        cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
    }

    /// A window that edits a loose file offers the reference too.
    #[gpui_kit::test]
    async fn opens_in_a_window_that_edits_a_loose_file(cx: &mut TestAppContext) {
        let _home = start(cx);
        cx.update(gpui_kit::init);
        let folder = tempfile::tempdir().expect("a temporary folder");
        let file = folder.path().canonicalize().expect("the folder exists").join("a.txt");
        std::fs::write(&file, "a\n").expect("the file is written");
        let window: AnyWindowHandle =
            cx.update(|cx| windows::open_window(Target::File(file.clone()), cx)).expect("a window opens");
        // While the file opens the window offers only what it offers during an opening.
        let mut editing = false;
        for _ in 0..3000 {
            cx.run_until_parked();
            editing = window
                .update(cx, |_, window, cx| {
                    super::super::for_window(window, cx).is_some_and(|w| w.read(cx).editor().is_some())
                })
                .expect("the window is open");
            if editing {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(editing, "the loose file opens in its editor");
        window
            .update(cx, |_, window, cx| {
                crate::platform::actions::execute_action_or_throw(KEYMAP_REFERENCE, None, window, cx)
            })
            .expect("the window is open")
            .unwrap_or_else(|error| panic!("{KEYMAP_REFERENCE} runs: {error}"));
        cx.run_until_parked();
        let shown = window
            .update(cx, |_, window, cx| {
                super::super::for_window(window, cx).and_then(|w| w.read(cx).keymap_reference()).is_some()
            })
            .expect("the window is open");
        assert!(shown, "the reference is open in the single-file window");
        cx.update(|cx| {
            let host = cx.global::<Backend>().host.clone();
            for (label, _) in windows::windows(cx) {
                host.close_project(&label);
            }
        });
    }

    /// The reference sits a tenth of the window's height below its top, centred across, and closes
    /// when the window is left, giving the keyboard back.
    #[gpui_kit::test]
    async fn sits_near_the_top_and_closes_when_the_window_is_left(cx: &mut TestAppContext) {
        let _home = start(cx);
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        cx.update(|window, _| window.activate_window());
        cx.run_until_parked();
        let before = cx.update(|window, cx| window.focused(cx));
        run_action(cx, KEYMAP_REFERENCE, None);
        let bounds = cx.debug_bounds("keymap-reference").expect("the dialog is drawn");
        let size = cx.update(|window, _| window.viewport_size());
        assert!((bounds.top() - size.height * 0.1).abs() < gpui_kit::px(1.0), "{bounds:?} in {size:?}");
        assert!((bounds.center().x - size.width / 2.0).abs() < gpui_kit::px(1.0), "{bounds:?} in {size:?}");

        cx.deactivate_window();
        assert!(workbench.read_with(cx, |w, _| w.keymap_reference()).is_none(), "leaving the window closes it");
        assert_eq!(cx.update(|window, cx| window.focused(cx)), before, "the keyboard is given back");
    }
}
