//! The UI language follows Settings › Language and Region › Language (`appearance.locale`): it is
//! applied once the backend runs, before any window or plugin starts, and again whenever the global
//! settings change, whichever window wrote them.
//!
//! The language is one for the process, like the theme: every window draws in it and plugin code reads
//! its tag with `locale()`. Views translate their text when they draw, and the menu bar reads its
//! labels again for a new language, so redrawing the windows is what switches them.

use crate::backend::Backend;
use crate::platform::i18n::{DEFAULT_LOCALE, Localization};
use crate::platform::settings;
use crate::plugins::Plugins;
use gpui_kit::{App, Global};
use ide_core::IdeEvent;

/// The operating system's language (a BCP 47 or POSIX tag) that `system`, `auto` and an empty Language
/// setting stand for. Without it they stand for English, so that nothing but the app's start-up hands
/// the machine's language to the UI.
pub struct SystemLanguage(pub String);

impl Global for SystemLanguage {}

impl SystemLanguage {
    /// The language of the process's locale variables, in the order POSIX gives them precedence
    /// (`LC_ALL`, `LC_MESSAGES`, `LANG`); English when none is set.
    pub fn from_env() -> Self {
        let tag = ["LC_ALL", "LC_MESSAGES", "LANG"]
            .into_iter()
            .filter_map(|name| std::env::var(name).ok())
            .find(|value| !value.trim().is_empty());
        Self(tag.unwrap_or_else(|| DEFAULT_LOCALE.to_owned()))
    }
}

/// Applies the language the settings choose and follows them from now on. Call once the backend runs.
pub fn init(cx: &mut App) {
    if cx.has_global::<Backend>() {
        cx.global_mut::<Backend>().listen(|event, cx| {
            if let IdeEvent::SettingsChanged { key } = &event.event
                && (key.is_empty() || key == "appearance" || key == "appearance.locale")
            {
                apply(cx);
            }
        });
    }
    apply(cx);
}

/// Switches the UI to the language the global settings choose. When it changed, every window is drawn
/// again and the plugin host is told the new tag, after the switch, so plugin code never reads a
/// language the windows do not show.
pub fn apply(cx: &mut App) {
    let stored = settings::global(cx).appearance.locale;
    let system = cx.try_global::<SystemLanguage>().map_or_else(|| DEFAULT_LOCALE.to_owned(), |system| system.0.clone());
    let localization = &mut cx.default_global::<Localization>().0;
    if !localization.set_locale(Some(&stored), &system) {
        return;
    }
    let tag = localization.locale_info().tag;
    if let Some(plugins) = cx.try_global::<Plugins>() {
        plugins.host.set_ui_locale(tag);
    }
    cx.refresh_windows();
}
