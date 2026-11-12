//! Distraction Free Mode (View › Appearance): tool windows, stripes, the main toolbar's
//! contents, the status bar, editor tabs, breadcrumbs and the editor gutter are hidden and the
//! text column is centred. The same action enters and exits; Escape does not exit. The mode is
//! an application-wide preference that survives restarts, stored next to the tool window
//! layouts (which keep the layout to restore on exit). Entering the mode, or starting in it,
//! shows a hint on how to leave it, since the main toolbar and the status bar are gone.
//!
//! The mode decides; the caller hides and restores the tool windows
//! ([`crate::workbench::tool_windows::ToolWindowLayout::enter_distraction_free`]), stores the
//! flag and shows the hint.

use crate::platform::keymap::KeymapState;

/// The toggle action. The id predates the mode's name; keymaps and user bindings hold it.
pub const DISTRACTION_FREE_ACTION: &str = "editor.toggleZen";
/// The context key that tells whether the mode is on.
pub const DISTRACTION_FREE_CONTEXT: &str = "distractionFree";
/// Text width in columns when the setting has no usable value.
pub const DEFAULT_DISTRACTION_FREE_WIDTH: u32 = 80;
/// Where the mode is remembered: `"1"` while it is on, nothing otherwise.
pub const DISTRACTION_FREE_STORAGE_KEY: &str = "openidea.distractionFree";
/// Key of the hint notification.
pub const DISTRACTION_FREE_HINT_KEY: &str = "distractionFree.hint";
/// The hint's own notification group: its "Don't Show Again" turns off this hint only.
pub const DISTRACTION_FREE_HINT_GROUP: &str = "distractionFree";
/// English source text of the hint's title.
pub const DISTRACTION_FREE_HINT_TITLE: &str = "Distraction Free Mode";

/// Text width in columns (Settings › Editor › General › Appearance), rounded and kept within
/// 20 to 500.
pub fn distraction_free_width(raw: Option<f64>) -> u32 {
    match raw.filter(|width| width.is_finite()) {
        // In range by the clamp, so the conversion is exact.
        Some(width) => width.round().clamp(20.0, 500.0) as u32,
        None => DEFAULT_DISTRACTION_FREE_WIDTH,
    }
}

/// English source text of the toggle action's title, which follows the mode. It is also the
/// label of the hint's action while the mode is on.
pub fn distraction_free_title(active: bool) -> &'static str {
    if active { "Exit Distraction Free Mode" } else { "Enter Distraction Free Mode" }
}

/// How to leave the mode: by the toggle's own shortcut when it has one, else through Find
/// Action (with its shortcut, when it has one).
pub fn exit_hint(own_shortcut: Option<&str>, find_action_shortcut: Option<&str>) -> String {
    let way = match (own_shortcut, find_action_shortcut) {
        (Some(own), _) => own.to_owned(),
        (None, Some(find)) => format!("Find Action ({find})"),
        (None, None) => "Find Action".to_owned(),
    };
    format!("To exit Distraction Free Mode, use View | Appearance | Exit Distraction Free Mode or {way}.")
}

/// [`exit_hint`] with the shortcuts of `keymap`, as this OS shows them.
pub fn distraction_free_hint(keymap: &KeymapState, mac: bool) -> String {
    let own = keymap.shortcut_label(DISTRACTION_FREE_ACTION, mac);
    let find = keymap.shortcut_label("navigate.findAction", mac);
    exit_hint(own.as_deref(), find.as_deref())
}

/// What the caller carries out, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    /// Publish [`DISTRACTION_FREE_CONTEXT`].
    SetContext(bool),
    /// Store `"1"` under [`DISTRACTION_FREE_STORAGE_KEY`], or remove it.
    Store(bool),
    /// The toggle action is now titled so.
    Title(&'static str),
    /// Hide the tool windows, remembering which were shown.
    EnterLayout,
    /// Restore the tool windows that were shown before the mode.
    ExitLayout,
    /// Show the hint as a notification: [`DISTRACTION_FREE_HINT_KEY`], in
    /// [`DISTRACTION_FREE_HINT_GROUP`], titled [`DISTRACTION_FREE_HINT_TITLE`], with
    /// [`distraction_free_hint`] as its body and one action, [`distraction_free_title`] of the
    /// mode being on, that turns the mode off.
    ShowHint,
    /// Take the hint's balloon away.
    ExpireHint,
}

/// Whether the mode is on, and whether its hint is still owed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DistractionFree {
    active: bool,
    /// Set on entering the mode; the hint shows once a project frame is there to leave.
    hint_pending: bool,
}

impl DistractionFree {
    /// The mode at start-up: `stored` is what [`DISTRACTION_FREE_STORAGE_KEY`] holds, and
    /// `layout_loaded` whether a project's tool window layout is loaded already.
    pub fn restore(stored: Option<&str>, layout_loaded: bool) -> (Self, Vec<Effect>) {
        let mut mode = Self::default();
        let mut effects = vec![Effect::Title(distraction_free_title(false)), Effect::SetContext(false)];
        if stored == Some("1") {
            effects.extend(mode.set(true, layout_loaded));
        }
        (mode, effects)
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    fn hint(&mut self, layout_loaded: bool) -> Option<Effect> {
        if !self.hint_pending || !self.active || !layout_loaded {
            return None;
        }
        self.hint_pending = false;
        Some(Effect::ShowHint)
    }

    /// Turns the mode on or off; setting it to what it is does nothing.
    pub fn set(&mut self, active: bool, layout_loaded: bool) -> Vec<Effect> {
        if active == self.active {
            return Vec::new();
        }
        self.active = active;
        self.hint_pending = active;
        let layout = if active { Effect::EnterLayout } else { Effect::ExitLayout };
        let title = Effect::Title(distraction_free_title(active));
        let mut effects = vec![title, Effect::SetContext(active), Effect::Store(active), layout];
        effects.extend(if active { self.hint(layout_loaded) } else { Some(Effect::ExpireHint) });
        effects
    }

    /// The toggle action.
    pub fn toggle(&mut self, layout_loaded: bool) -> Vec<Effect> {
        self.set(!self.active, layout_loaded)
    }

    /// A project's tool window layout was loaded: in the mode its windows are hidden like the
    /// ones before, and a hint that could not show without a project frame shows now.
    pub fn layout_loaded(&mut self) -> Vec<Effect> {
        if !self.active {
            return Vec::new();
        }
        std::iter::once(Effect::EnterLayout).chain(self.hint(true)).collect()
    }
}

/// Which parts of the window frame show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShellChrome {
    /// The main toolbar's contents (project and run widgets, Search Everywhere…). Without them
    /// the title bar is a plain strip that shows the window title and still drags the window.
    pub main_toolbar: bool,
    /// The tabs of grouped windows.
    pub window_tabs: bool,
    /// The tool window stripes.
    pub stripes: bool,
    pub status_bar: bool,
}

/// The frame for the mode and the View › Appearance switches (`appearance.showMainToolbar`,
/// `appearance.showStatusBar`), each of which hides its own part only.
pub fn shell_chrome(distraction_free: bool, show_main_toolbar: bool, show_status_bar: bool) -> ShellChrome {
    ShellChrome {
        main_toolbar: !distraction_free && show_main_toolbar,
        window_tabs: !distraction_free,
        stripes: !distraction_free,
        status_bar: !distraction_free && show_status_bar,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::keymap::Keymap;
    use crate::workbench::tool_windows::{Anchor, ToolWindow, ToolWindowLayout, storage_key};
    use serde_json::Value;
    use std::collections::BTreeMap;

    const SCOPE: &str = "/work/df";

    /// The mode with a window's tool windows and storage: the effects are carried out as a
    /// window does.
    struct Shell {
        mode: DistractionFree,
        layout: ToolWindowLayout,
        storage: BTreeMap<String, String>,
        context: bool,
        title: &'static str,
        hint_shown: bool,
    }

    fn windows() -> ToolWindowLayout {
        let mut layout = ToolWindowLayout::new();
        for window in [
            ToolWindow::new("project", "Project", "folder", Anchor::Left),
            ToolWindow::new("terminal", "Terminal", "terminal", Anchor::Bottom),
            ToolWindow::new("structure", "Structure", "structure", Anchor::Right),
        ] {
            let _ = layout.register(window);
        }
        layout
    }

    impl Shell {
        /// Starts the app with `storage`, no project loaded yet.
        fn start(storage: BTreeMap<String, String>) -> Self {
            let stored = storage.get(DISTRACTION_FREE_STORAGE_KEY).map(String::as_str);
            let (mode, effects) = DistractionFree::restore(stored, false);
            let mut shell = Self { mode, layout: windows(), storage, context: false, title: "", hint_shown: false };
            shell.apply(&effects);
            shell
        }

        /// A fresh app with the project loaded and the Terminal shown.
        fn new() -> Self {
            let mut shell = Self::start(BTreeMap::new());
            shell.load();
            shell.show("terminal");
            shell
        }

        fn persist(&mut self, effects: crate::workbench::tool_windows::Effects) {
            if effects.persist {
                let saved = serde_json::to_string(&self.layout.serialized()).expect("a layout is plain values");
                self.storage.insert(storage_key(self.layout.scope()), saved);
            }
        }

        fn apply(&mut self, effects: &[Effect]) {
            for effect in effects {
                match *effect {
                    Effect::SetContext(on) => self.context = on,
                    Effect::Store(true) => {
                        self.storage.insert(DISTRACTION_FREE_STORAGE_KEY.to_owned(), "1".to_owned());
                    }
                    Effect::Store(false) => {
                        self.storage.remove(DISTRACTION_FREE_STORAGE_KEY);
                    }
                    Effect::Title(title) => self.title = title,
                    Effect::EnterLayout => {
                        let effects = self.layout.enter_distraction_free();
                        self.persist(effects);
                    }
                    Effect::ExitLayout => {
                        let effects = self.layout.exit_distraction_free();
                        self.persist(effects);
                    }
                    Effect::ShowHint => self.hint_shown = true,
                    Effect::ExpireHint => self.hint_shown = false,
                }
            }
        }

        fn layout_loaded(&self) -> bool {
            !self.layout.scope().is_empty()
        }

        fn set(&mut self, active: bool) -> Vec<Effect> {
            let effects = self.mode.set(active, self.layout_loaded());
            self.apply(&effects);
            effects
        }

        fn toggle(&mut self) {
            let effects = self.mode.toggle(self.layout_loaded());
            self.apply(&effects);
        }

        /// Loads the project's layout.
        fn load(&mut self) {
            let stored = self.storage.get(&storage_key(SCOPE)).cloned();
            self.layout.load(SCOPE, stored.as_deref());
            let effects = self.mode.layout_loaded();
            self.apply(&effects);
        }

        fn show(&mut self, id: &str) {
            let effects = self.layout.show(id, false);
            self.persist(effects);
        }

        fn visible(&self, id: &str) -> bool {
            self.layout.is_visible(id)
        }

        fn saved_visible(&self) -> Value {
            let saved: Value = serde_json::from_str(&self.storage[&storage_key(SCOPE)]).expect("the layout is JSON");
            saved["visible"].clone()
        }
    }

    // distraction free mode › toggles with one action whose title flips Enter / Exit, publishing a context key
    #[test]
    fn toggles_with_one_action_whose_title_flips_enter_and_exit_publishing_a_context_key() {
        let mut shell = Shell::new();
        assert_eq!((shell.title, shell.context), ("Enter Distraction Free Mode", false));
        shell.toggle();
        assert!(shell.mode.is_active());
        assert_eq!((shell.title, shell.context), ("Exit Distraction Free Mode", true));
        shell.toggle();
        assert!(!shell.mode.is_active());
        assert_eq!((shell.title, shell.context), ("Enter Distraction Free Mode", false));
    }

    // distraction free mode › hides tool windows and restores the previous layout on exit
    #[test]
    fn hides_tool_windows_and_restores_the_previous_layout_on_exit() {
        let mut shell = Shell::new();
        shell.set(true);
        assert!(!shell.visible("project") && !shell.visible("terminal"));
        // A window opened meanwhile (by shortcut) is gone again after exit.
        shell.show("structure");
        assert!(shell.visible("structure"));
        shell.set(false);
        assert!(shell.visible("project") && shell.visible("terminal"));
        assert!(!shell.visible("structure"));
    }

    // distraction free mode › persists the layout to restore, not the hidden one
    #[test]
    fn persists_the_layout_to_restore_not_the_hidden_one() {
        let mut shell = Shell::new();
        shell.set(true);
        shell.show("structure");
        let saved = shell.saved_visible();
        assert_eq!(saved["left"], "project");
        assert_eq!(saved["bottom"], "terminal");
        assert_eq!(saved["right"], Value::Null);
        // Leaving the mode (now or after a restart) loads the normal layout.
        let mut storage = shell.storage;
        storage.remove(DISTRACTION_FREE_STORAGE_KEY);
        let mut restarted = Shell::start(storage);
        restarted.load();
        assert!(restarted.visible("project") && restarted.visible("terminal"));
    }

    // distraction free mode › does not disturb Hide All Tool Windows
    #[test]
    fn does_not_disturb_hide_all_tool_windows() {
        let mut shell = Shell::new();
        shell.set(true);
        let effects = shell.layout.hide_all();
        shell.persist(effects);
        shell.set(false);
        assert!(shell.visible("project"));
        // Nothing was stashed meanwhile: Hide All hides now, and brings the windows back after.
        let effects = shell.layout.hide_all();
        shell.persist(effects);
        assert!(!shell.visible("project") && !shell.visible("terminal"));
        let effects = shell.layout.hide_all();
        shell.persist(effects);
        assert!(shell.visible("project") && shell.visible("terminal"));
    }

    // distraction free mode › survives an app restart and re-applies to a newly loaded project
    #[test]
    fn survives_an_app_restart_and_re_applies_to_a_newly_loaded_project() {
        let mut shell = Shell::new();
        shell.set(true);
        assert_eq!(shell.storage.get(DISTRACTION_FREE_STORAGE_KEY).map(String::as_str), Some("1"));
        // Restart: only what was stored remains.
        let mut restarted = Shell::start(shell.storage);
        assert!(restarted.mode.is_active() && restarted.context);
        restarted.load();
        assert!(!restarted.visible("project"));
        restarted.set(false);
        assert!(restarted.visible("project"));
        assert_eq!(restarted.storage.get(DISTRACTION_FREE_STORAGE_KEY), None);
    }

    // distraction free mode › is unbound in the IntelliJ keymap (⌃⌘D is macOS Look Up) and Escape does not exit
    #[test]
    fn is_unbound_in_the_intellij_keymap_and_escape_does_not_exit() {
        let keymap = KeymapState::new("intellij-macos");
        assert!(keymap.bindings_for(DISTRACTION_FREE_ACTION).is_empty());
        // No keystroke runs the toggle: neither Ctrl+Meta+D nor Escape leaves the mode.
        assert!(keymap.effective_bindings().iter().all(|(_, action)| action != DISTRACTION_FREE_ACTION));
    }

    // distraction free mode › exit hint › tells how to leave the mode on entering, with an Exit action, and expires
    // on exit
    #[test]
    fn tells_how_to_leave_the_mode_on_entering_with_an_exit_action_and_expires_on_exit() {
        let mut shell = Shell::new();
        assert!(shell.set(true).contains(&Effect::ShowHint));
        assert!(shell.hint_shown);
        let hint = distraction_free_hint(&KeymapState::new("intellij-macos"), true);
        assert!(hint.contains("View | Appearance | Exit Distraction Free Mode"));
        // Unbound toggle (IntelliJ keymap): Find Action with its shortcut is the keyboard way out.
        let find = hint.split_once("Find Action (").and_then(|(_, rest)| rest.split_once(')'));
        assert!(find.is_some_and(|(shortcut, _)| !shortcut.is_empty()), "{hint}");
        assert_eq!(distraction_free_title(shell.mode.is_active()), "Exit Distraction Free Mode");
        assert!(shell.set(false).contains(&Effect::ExpireHint));
        assert!(!shell.mode.is_active() && !shell.hint_shown);
    }

    // distraction free mode › exit hint › names the toggle's own shortcut when it has one
    #[test]
    fn names_the_toggles_own_shortcut_when_it_has_one() {
        let mut keymap = KeymapState::new("intellij-macos");
        keymap.set_overrides(Keymap::from([(DISTRACTION_FREE_ACTION.to_owned(), vec!["Ctrl+Alt+Z".to_owned()])]));
        let hint = distraction_free_hint(&keymap, true);
        assert!(!hint.contains("Find Action"), "{hint}");
        assert!(hint.ends_with("or ⌃⌥Z."), "{hint}");
    }

    // distraction free mode › exit hint › shows again after a restart in the mode, once the project frame loads
    #[test]
    fn shows_again_after_a_restart_in_the_mode_once_the_project_frame_loads() {
        let mut shell = Shell::new();
        shell.set(true);
        let mut restarted = Shell::start(shell.storage);
        assert!(restarted.mode.is_active());
        assert!(!restarted.hint_shown);
        restarted.load();
        assert!(restarted.hint_shown);
        // It is owed once: another project's layout does not bring it back.
        assert_eq!(restarted.mode.layout_loaded(), [Effect::EnterLayout]);
    }

    // distraction free mode › clamps the text width setting
    #[test]
    fn clamps_the_text_width_setting() {
        assert_eq!(distraction_free_width(None), 80);
        assert_eq!(distraction_free_width(Some(100.4)), 100);
        assert_eq!(distraction_free_width(Some(3.0)), 20);
        assert_eq!(distraction_free_width(Some(f64::NAN)), 80);
        assert_eq!(distraction_free_width(Some(1e9)), 500);
    }

    // distraction free shell › hides stripes, status bar and main toolbar contents, keeping the drag region
    #[test]
    fn hides_stripes_status_bar_and_main_toolbar_contents_keeping_the_drag_region() {
        let shown = ShellChrome { main_toolbar: true, window_tabs: true, stripes: true, status_bar: true };
        assert_eq!(shell_chrome(false, true, true), shown);
        let hidden = ShellChrome { main_toolbar: false, window_tabs: false, stripes: false, status_bar: false };
        assert_eq!(shell_chrome(true, true, true), hidden);
    }

    // distraction free shell › View › Appearance hides the main toolbar and the status bar on their own
    #[test]
    fn view_appearance_hides_the_main_toolbar_and_the_status_bar_on_their_own() {
        let no_toolbar = shell_chrome(false, false, true);
        assert_eq!((no_toolbar.main_toolbar, no_toolbar.status_bar, no_toolbar.stripes), (false, true, true));
        let no_status_bar = shell_chrome(false, true, false);
        assert_eq!((no_status_bar.main_toolbar, no_status_bar.status_bar, no_status_bar.stripes), (true, false, true));
    }

    // Not from a spec: setting the mode to what it is does nothing, and the hint's wording
    // without any shortcut.
    #[test]
    fn setting_the_mode_again_does_nothing() {
        let mut mode = DistractionFree::default();
        assert!(mode.set(false, true).is_empty());
        assert_eq!(mode.set(true, false).last(), Some(&Effect::EnterLayout));
        assert!(mode.set(true, true).is_empty());
        assert!(exit_hint(None, None).ends_with("or Find Action."));
        assert_eq!((DISTRACTION_FREE_CONTEXT, DISTRACTION_FREE_HINT_GROUP), ("distractionFree", "distractionFree"));
        assert_eq!(DISTRACTION_FREE_HINT_KEY, "distractionFree.hint");
        assert_eq!(DISTRACTION_FREE_HINT_TITLE, "Distraction Free Mode");
    }
}
