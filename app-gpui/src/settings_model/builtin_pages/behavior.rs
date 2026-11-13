//! Appearance & Behavior: Appearance, System Settings with its sub-pages (Language and Region, Date
//! Formats, HTTP Proxy, IDE Mode, Updates) and Notifications.

use super::{APPEARANCE_GROUP, custom, custom_owned, hint, number, select, select_owned, toggle};
use crate::platform::i18n::LOCALES;
use crate::settings_model::page_spec::{OptionDetail, PageSpec};
use crate::theme::themes::BUILTIN_THEMES;
use crate::workbench::notifications::NOTIFICATION_GROUPS;

/// Antialiasing of the IDE and of the editor text.
const ANTIALIASING: [(&str, &str); 3] =
    [("subpixel", "Subpixel"), ("greyscale", "Greyscale"), ("none", "No antialiasing")];

fn appearance() -> PageSpec {
    // The choices are the built-in themes; Sync with OS and the preferred themes are search words.
    let themes = BUILTIN_THEMES.iter().map(|theme| (theme.id.as_str(), theme.label)).collect();
    PageSpec::new("appearance", "Appearance", 0)
        .group(APPEARANCE_GROUP)
        .keywords(&[
            "theme",
            "colour",
            "color",
            "dark",
            "light",
            "zoom",
            "font",
            "compact",
            "Sync with OS",
            "preferred theme",
        ])
        .section(
            None,
            vec![
                select_owned("appearance.theme", "Theme", themes),
                custom("Editor color scheme", &["Theme default", "scheme"]),
                custom("File icon theme", &["icons", "file icons", "icon theme"]),
                custom("Accent colour", &["accent color", "highlight"]),
                custom("Different tool window background", &[]),
            ],
        )
        .section(
            Some("Accessibility"),
            vec![
                custom("Zoom", &["scale", "zoom ide"]),
                custom("Use custom font", &["UI font size", "Size", "font family", "fallback font"]),
                toggle("appearance.contrastScrollbars", "Use contrast scrollbars"),
                toggle("appearance.redGreenVision", "Adjust colors for red-green vision deficiency"),
            ],
        )
        .section(
            Some("UI Options"),
            vec![
                toggle("appearance.showMainToolbar", "Show main toolbar"),
                toggle("appearance.showStatusBar", "Show status bar"),
                custom("Main menu", &["Hamburger button", "Merged with main toolbar"]),
                custom(
                    "Compact mode",
                    &[
                        "Always show full path in window header",
                        "Use project colors in main toolbar",
                        "Drag-and-drop with Alt pressed only",
                        "Display icons in menu items",
                        "Use smaller indents in trees",
                        "tree indent",
                    ],
                ),
            ],
        )
        .section(
            Some("Antialiasing"),
            vec![
                select("appearance.ideAntialiasing", "IDE", &ANTIALIASING),
                select("appearance.editorAntialiasing", "Editor", &ANTIALIASING),
            ],
        )
        .section(
            Some("Tool Windows"),
            vec![custom(
                "Show tool window bars",
                &["Show tool window names", "Widescreen tool window layout", "stripes"],
            )],
        )
        .detail("appearance.redGreenVision", hint("For protanopia and deuteranopia."))
}

fn system_settings() -> PageSpec {
    PageSpec::new("general", "System Settings", 1)
        .group(APPEARANCE_GROUP)
        .keywords(&["general", "startup", "exit", "quit", "autosave", "save", "local history"])
        .section(
            Some("Project"),
            vec![
                toggle("general.reopenLastProject", "Reopen projects on startup"),
                custom("Open project in", &["New window", "Current window", "Ask"]),
                custom("Default directory", &[]),
            ],
        )
        .section(
            Some("Autosave"),
            vec![
                custom("Save files if the IDE is idle for", &["idle", "seconds"]),
                toggle("files.autoSave", "Save files when switching to a different application or a built-in terminal"),
                toggle("files.syncOnFocus", "Synchronize external changes when switching to the IDE window"),
            ],
        )
        .section(
            Some("Local History"),
            vec![
                number("files.localHistoryDays", "Keep local history for", 1.0, 365.0, Some("working days")),
                number("files.localHistoryMaxMb", "Maximum local history size per project", 16.0, 16384.0, Some("MB")),
            ],
        )
        .section(Some("Exit"), vec![toggle("general.confirmExit", "Confirm before exiting the IDE")])
        .detail(
            "files.localHistoryDays",
            hint("Days on which files changed; older revisions are removed when a project opens."),
        )
        .detail("files.localHistoryMaxMb", OptionDetail { step: Some(64.0), ..OptionDetail::default() })
}

fn ide_mode() -> PageSpec {
    PageSpec::new("general.mode", "IDE Mode", 2)
        .group(APPEARANCE_GROUP)
        .project_scope()
        .keywords(&["Blaze mode", "Deep mode", "language server", "lsp", "memory", "performance"])
        .section(None, vec![select("ide.mode", "IDE mode", &[("light", "Blaze mode"), ("full", "Deep mode")])])
        .detail(
            "ide.mode",
            hint(
                "Light: the editor, git, search and the syntax index only; no language server runs, and Find Usages \
                 and Call Hierarchy use text matches. Full: language servers and the background tooling they need \
                 start when files open.",
            ),
        )
}

fn language_and_region() -> PageSpec {
    let mut keywords = vec!["locale", "translation"];
    keywords.extend(LOCALES.iter().map(|locale| locale.name));
    let locales = LOCALES.iter().map(|locale| (locale.id, locale.name)).collect();
    PageSpec::new("general.language", "Language and Region", 0)
        .keywords(&keywords)
        .section(None, vec![select_owned("appearance.locale", "Language", locales)])
        .detail("appearance.locale", hint("Language of menus and dialogs; untranslated text stays in English."))
}

fn date_formats() -> PageSpec {
    PageSpec::new("general.dateFormats", "Date Formats", 1).keywords(&["date", "time", "pretty", "24-hour"]).section(
        None,
        vec![custom(
            "Override system date and time format",
            &["Date format", "Use 24-hour time format", "Use pretty formatting"],
        )],
    )
}

fn http_proxy() -> PageSpec {
    PageSpec::new("general.httpProxy", "HTTP Proxy", 2)
        .keywords(&["proxy", "network", "socks", "http_proxy", "no_proxy", "connection"])
        .section(
            None,
            vec![custom(
                "Manual proxy configuration",
                &[
                    "No proxy",
                    "Auto-detect proxy settings",
                    "Host name",
                    "Port number",
                    "No proxy for",
                    "Proxy authentication",
                    "Login",
                    "Password",
                    "Check connection…",
                ],
            )],
        )
}

fn notifications() -> PageSpec {
    let mut groups: Vec<&'static str> = NOTIFICATION_GROUPS.iter().map(|(_, title)| *title).collect();
    groups.extend(["Sticky balloon", "No popup", "Show in tool window"]);
    PageSpec::new("notifications", "Notifications", 2)
        .group(APPEARANCE_GROUP)
        .keywords(&["balloon", "popup", "sound", "alerts"])
        .section(
            None,
            vec![
                toggle("notifications.balloons", "Display balloon notifications"),
                toggle("notifications.sound", "Play sound"),
            ],
        )
        .section(Some("Groups"), vec![custom_owned("Popup type", groups)])
}

/// System Settings › Updates; registered only by builds that update themselves.
pub(super) fn updates() -> PageSpec {
    PageSpec::new("general.updates", "Updates", 3)
        .keywords(&["update", "upgrade", "version", "automatic", "GitHub", "CNB"])
        .section(
            None,
            vec![
                custom("Current version", &["Last checked", "Check Now"]),
                toggle("general.autoCheckUpdates", "Check for updates automatically"),
                select(
                    "general.updateSource",
                    "Update source",
                    &[("auto", "Automatic"), ("github", "GitHub"), ("cnb", "CNB (China mirror)")],
                ),
            ],
        )
        .detail(
            "general.autoCheckUpdates",
            hint("At startup and every 4 hours while OpenIDEA runs. Each new version is announced once per session."),
        )
        .detail(
            "general.updateSource",
            hint("Where updates are downloaded from. If it can't be reached, the other source is tried."),
        )
}

/// The pages in the order they are registered (HTTP Proxy sits between Date Formats and
/// Notifications).
pub(super) fn pages() -> Vec<PageSpec> {
    vec![
        appearance(),
        system_settings(),
        ide_mode(),
        language_and_region(),
        date_formats(),
        http_proxy(),
        notifications(),
    ]
}
