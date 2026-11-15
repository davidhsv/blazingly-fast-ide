//! View › Appearance › Zoom IDE (Zoom In, Zoom Out, Reset) and View › Quick Switch Scheme….
//!
//! The zoom is the global `appearance.zoom` setting, stepped through IDEA's zoom steps; the theme
//! applies it to every length it has ([`crate::theme::zoom`]). Quick Switch Scheme is the popup of
//! the switchable schemes ([`super::quick_switch`]): the editor colour scheme, the keymap, the view
//! modes and the theme, each with the one in effect checked.

use super::Workbench;
use super::quick_switch::{self, ColorScheme, Command, KeymapState, QuickSwitchInput, ThemeChoice};
use super::services_window::on_workbench;
use crate::platform::actions::ActionRegistry;
use crate::platform::settings::{self, SettingsWrite};
use crate::settings_model::appearance_runtime::ZoomAction;
use crate::theme::appearance::{self as theme_appearance, ThemeState};
use crate::theme::color_scheme;
use crate::theme::themes::{ThemeSetting, theme_groups};
use crate::vcs_ops::menu::MenuItem;
use crate::workbench::notifications::{NotificationLevel, NotificationRequest};
use gpui_kit::{App, Context, Window, point, px};

/// What a row of Quick Switch Scheme does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemeCommand {
    /// Select the theme of this id (a built-in theme or a plugin's), for every window.
    Theme(String),
    /// Write a global setting (the keymap, the editor colour scheme).
    Global(SettingsWrite),
}

/// Registers the zoom actions and Quick Switch Scheme….
pub(super) fn register_actions(registry: &mut ActionRegistry) {
    for zoom in ZoomAction::ALL {
        let (id, _) = zoom.action();
        registry.handle(id, move |_, window, cx| {
            let label = super::for_window(window, cx).map(|workbench| workbench.read(cx).label().to_owned());
            let current = settings::global(cx).appearance.zoom;
            settings::set_global(label.as_deref().unwrap_or_default(), &[zoom.write(current)], cx)
                .map_err(|error| anyhow::anyhow!("the zoom is not saved: {}", error.message))?;
            // The windows follow the setting at once, not only once its change event arrives.
            theme_appearance::apply(cx);
            Ok(())
        });
    }
    registry.handle("view.quickSwitchScheme", |_, window, cx| {
        on_workbench(window, cx, |workbench, window, cx| workbench.show_quick_switch(window, cx))
    });
}

/// A Quick Switch row as the window's menus show it; a checked row carries a check mark.
fn menu_item(item: quick_switch::MenuItem) -> MenuItem<SchemeCommand> {
    match item {
        quick_switch::MenuItem::Separator => MenuItem::Separator,
        quick_switch::MenuItem::Action(id) => MenuItem::action(id),
        quick_switch::MenuItem::Item(entry) => {
            let row = if entry.submenu.is_empty() {
                let run = entry.run.map(|command| match command {
                    Command::SetTheme(id) | Command::ChooseTheme(id) => SchemeCommand::Theme(id),
                    Command::SetGlobal(write) => SchemeCommand::Global(write),
                });
                MenuItem::entry(entry.label, run)
            } else {
                MenuItem::submenu(entry.label, entry.submenu.into_iter().map(menu_item).collect())
            };
            if entry.checked == Some(true) { row.icon("check") } else { row }
        }
    }
}

/// The rows of Quick Switch Scheme as the settings and the themes stand now.
fn quick_switch_menu(cx: &App) -> Vec<MenuItem<SchemeCommand>> {
    let effective = settings::global(cx);
    let builtin = color_scheme::schemes().into_iter().map(|scheme| ColorScheme { id: scheme.id, name: scheme.name });
    let customs = effective
        .editor
        .custom_schemes
        .iter()
        .map(|(id, scheme)| ColorScheme { id: id.clone(), name: scheme.name.clone() });
    let schemes: Vec<ColorScheme> = builtin.chain(customs).collect();
    let keymap = KeymapState {
        keymap_id: effective.keymap.clone(),
        customs: effective.custom_keymaps.keys().cloned().collect(),
    };
    let groups = match cx.try_global::<ThemeState>() {
        Some(state) => state.contributed.groups(),
        None => theme_groups(&[]),
    };
    let theme_groups: Vec<Vec<ThemeChoice>> = groups
        .into_iter()
        .map(|group| {
            let choice = |theme: crate::theme::themes::ThemeChoice| ThemeChoice {
                id: theme.id.to_string(),
                label: theme.label,
                origin: theme.origin,
            };
            group.themes.into_iter().map(choice).collect()
        })
        .collect();
    let theme_id = theme_appearance::current_choice(cx).to_string();
    let input = QuickSwitchInput {
        color_scheme: Some(effective.editor.color_scheme.as_str()),
        schemes: &schemes,
        keymap: &keymap,
        theme_groups: &theme_groups,
        theme_id: &theme_id,
    };
    quick_switch::quick_switch_items(&input, false).into_iter().map(menu_item).collect()
}

impl Workbench {
    /// View › Quick Switch Scheme…: the popup of the switchable schemes, near the top of the window.
    fn show_quick_switch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let items = quick_switch_menu(cx);
        let width = f32::from(window.viewport_size().width);
        let position = point(px((width / 2.0 - 120.0).max(0.0)), px(96.0));
        self.show_menu(&items, position, |workbench, command, _, cx| workbench.switch_scheme(command, cx), window, cx);
    }

    /// Carries out the chosen Quick Switch row; what cannot be saved is told.
    fn switch_scheme(&mut self, command: SchemeCommand, cx: &mut Context<Self>) {
        let failure = match command {
            SchemeCommand::Theme(id) => match ThemeSetting::parse(&id) {
                Some(theme) => theme_appearance::set_theme(&self.label, theme, cx).err().map(|error| error.to_string()),
                None => Some(format!("{id} is not a theme")),
            },
            SchemeCommand::Global(write) => {
                settings::set_global(&self.label, std::slice::from_ref(&write), cx).err().map(|error| error.message)
            }
        };
        if let Some(message) = failure {
            let request = NotificationRequest {
                body: Some(message),
                ..NotificationRequest::new(NotificationLevel::Error, "Cannot switch the scheme")
            };
            self.notify(request, cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::plugins_window::test_support::trusted_project;
    use super::super::tests::{run_action, start, wait_long};
    use super::*;
    use crate::backend::Backend;
    use crate::theme::{self, ActiveTheme};
    use crate::ui::popup_menu::PopupMenu;
    use gpui_kit::{Entity, TestAppContext, VisualTestContext};

    fn zoom(cx: &mut VisualTestContext) -> u32 {
        cx.update(|_, cx| settings::global(cx).appearance.zoom)
    }

    fn ui_text_size(cx: &mut VisualTestContext) -> f32 {
        cx.update(|_, cx| f32::from(theme::length(cx, "fs-ui")))
    }

    /// The height the tree draws the project's row at.
    fn row_height(root: &std::path::Path, cx: &mut VisualTestContext) -> f32 {
        let selector: &'static str = Box::leak(format!("project-row:{}", root.to_string_lossy()).into_boxed_str());
        cx.run_until_parked();
        f32::from(cx.debug_bounds(selector).expect("the row is drawn").size.height)
    }

    fn close_project(cx: &mut VisualTestContext) {
        let host = cx.update(|_, cx| cx.global::<Backend>().host.clone());
        host.close_project("w1");
    }

    fn open_menu(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<PopupMenu<SchemeCommand>> {
        workbench.read_with(cx, |w, _| w.open_menu::<SchemeCommand>()).expect("Quick Switch Scheme is open")
    }

    /// Chooses the row `labels` names in the open menu: each label but the last opens a submenu.
    fn choose(workbench: &Entity<Workbench>, labels: &[&str], cx: &mut VisualTestContext) {
        let menu = open_menu(workbench, cx);
        for (level, label) in labels.iter().enumerate() {
            let selected = format!("> {label}");
            let is_selected = |cx: &mut VisualTestContext| {
                menu.read_with(cx, |menu, _| menu.lines().get(level).is_some_and(|rows| rows.contains(&selected)))
            };
            for _ in 0..40 {
                if is_selected(cx) {
                    break;
                }
                cx.simulate_keystrokes("down");
            }
            assert!(is_selected(cx), "{label} in {:?}", menu.read_with(cx, |menu, _| menu.lines()));
            cx.simulate_keystrokes(if level + 1 < labels.len() { "right" } else { "enter" });
            cx.run_until_parked();
        }
    }

    /// Zoom In and Zoom Out step the IDE zoom through the zoom steps and Reset IDE Zoom goes back to
    /// 100%: the setting is saved and the window draws its text and rows at that scale.
    #[gpui_kit::test]
    async fn the_zoom_actions_step_the_ide_zoom_and_the_window_draws_at_it(cx: &mut TestAppContext) {
        let _home = start(cx);
        cx.update(theme_appearance::init);
        let (_folder, root, _workbench, cx) = trusted_project(cx, "w1", &[("a.txt", "a\n")]);
        let (text, row) = (ui_text_size(cx), row_height(&root, cx));
        assert_eq!(zoom(cx), 100);

        run_action(cx, "view.zoomIn", None);
        assert_eq!(zoom(cx), 110);
        wait_long(cx, "the text at 110%", |cx| (ui_text_size(cx) - text * 1.1).abs() < 0.01);
        assert!((row_height(&root, cx) - row * 1.1).abs() < 0.5, "the rows are taller");
        run_action(cx, "view.zoomIn", None);
        assert_eq!(zoom(cx), 125);

        run_action(cx, "view.zoomOut", None);
        run_action(cx, "view.zoomOut", None);
        run_action(cx, "view.zoomOut", None);
        assert_eq!(zoom(cx), 90);
        wait_long(cx, "the text at 90%", |cx| (ui_text_size(cx) - text * 0.9).abs() < 0.01);

        run_action(cx, "view.resetZoom", None);
        assert_eq!(zoom(cx), 100);
        wait_long(cx, "the text as it was", |cx| (ui_text_size(cx) - text).abs() < 0.01);
        assert!((row_height(&root, cx) - row).abs() < 0.5, "the rows as they were");
        close_project(cx);
    }

    /// The size the last frame drew icon `selector` at.
    fn icon_size(selector: &'static str, cx: &mut VisualTestContext) -> f32 {
        cx.run_until_parked();
        f32::from(cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn")).size.width)
    }

    /// The font size and the row height the editor's last frame painted its text with.
    fn painted_code(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> (f32, f32) {
        cx.run_until_parked();
        let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor is open");
        let (size, line) = editor.read_with(cx, |editor, _| editor.painted_text_metrics()).expect("the editor painted");
        (f32::from(size), f32::from(line))
    }

    /// Zooms in from 100% to 150%.
    fn zoom_to_150(cx: &mut VisualTestContext) {
        for _ in 0..3 {
            run_action(cx, "view.zoomIn", None);
        }
        assert_eq!(zoom(cx), 150);
    }

    /// Zoom In up to 150% draws the icons (the Project view's chevron) and the editor's code (its text and its
    /// rows) half as large again, as it draws the interface text, and Reset IDE Zoom draws them as they were.
    #[gpui_kit::test]
    async fn the_ide_zoom_scales_the_editors_code_and_the_icons(cx: &mut TestAppContext) {
        let _home = start(cx);
        cx.update(theme_appearance::init);
        let (_folder, root, workbench, cx) = trusted_project(cx, "w1", &[("main.rs", "fn main() {}\n")]);
        workbench.update_in(cx, |w, window, cx| w.open_project_file(root.join("main.rs"), window, cx));
        wait_long(cx, "the editor paints", |cx| {
            workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).painted_text_metrics().is_some()))
        });
        let (size, line) = painted_code(&workbench, cx);
        let chevron = icon_size("icon:chevron-down", cx);
        assert_eq!(chevron, 16.0, "the expanded root's chevron at its own size");

        zoom_to_150(cx);
        wait_long(cx, "the code at 150%", |cx| (painted_code(&workbench, cx).0 - size * 1.5).abs() < 0.01);
        assert!((painted_code(&workbench, cx).1 - line * 1.5).abs() < 0.01, "the rows of code are taller");
        assert!((icon_size("icon:chevron-down", cx) - chevron * 1.5).abs() < 0.01, "the icons are larger");

        run_action(cx, "view.resetZoom", None);
        wait_long(cx, "the code as it was", |cx| painted_code(&workbench, cx) == (size, line));
        assert_eq!(icon_size("icon:chevron-down", cx), chevron, "the icons as they were");
        close_project(cx);
    }

    /// Quick Switch Scheme… lists the editor colour scheme, the keymap, the view modes and the theme;
    /// choosing a theme selects it for every window and saves it, and choosing a colour scheme
    /// saves it as the editor's.
    #[gpui_kit::test]
    async fn quick_switch_scheme_switches_the_theme_and_the_editor_color_scheme(cx: &mut TestAppContext) {
        let _home = start(cx);
        cx.update(theme_appearance::init);
        let (_folder, _root, workbench, cx) = trusted_project(cx, "w1", &[("a.txt", "a\n")]);
        run_action(cx, "view.quickSwitchScheme", None);
        let rows = open_menu(&workbench, cx).read_with(cx, |menu, _| menu.lines()).remove(0);
        assert_eq!(rows, ["> Editor Color Scheme", "Keymap", "View Mode", "Theme"]);
        assert!(!cx.update(|_, cx| cx.global::<ActiveTheme>().is_dark()));

        choose(&workbench, &["Theme", "Islands Dark"], cx);
        assert!(workbench.read_with(cx, |w, _| w.open_menu::<SchemeCommand>()).is_none(), "the choice closes it");
        assert!(cx.update(|_, cx| cx.global::<ActiveTheme>().is_dark()), "the window draws the theme");
        assert_eq!(cx.update(|_, cx| settings::global(cx).appearance.theme), "dark");

        run_action(cx, "view.quickSwitchScheme", None);
        choose(&workbench, &["Editor Color Scheme", "Solarized Dark"], cx);
        let scheme = cx.update(|_, cx| settings::global(cx).editor.color_scheme);
        assert_eq!(scheme, "solarized-dark");
        let titles: Vec<String> =
            workbench.read_with(cx, |w, _| w.notifications().items().iter().map(|n| n.title.clone()).collect());
        assert!(!titles.contains(&"Cannot switch the scheme".to_owned()), "{titles:?}");
        close_project(cx);
    }
}
