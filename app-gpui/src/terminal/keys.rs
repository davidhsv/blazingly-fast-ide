//! Terminal keyboard handling that runs before the IDE keymap (IDEA's "Override IDE shortcuts"):
//! Ctrl+letter goes to the shell (Ctrl+R reverse search, Ctrl+D EOF…), Shift+Enter as Esc+Return,
//! plus the macOS conveniences ⌘C / ⌘V / ⌘K / ⌘W, ⌘F (find) and ⌘/⌥ arrow navigation, and
//! Ctrl+Shift+C / V (copy / paste on Windows and Linux, where Ctrl+C also copies a selection and
//! otherwise interrupts the shell).
//!
//! With "Override IDE shortcuts" on, a Ctrl or Escape stroke still reaches the IDE when the keymap
//! binds it to one of IDEA's terminal-allowed actions (Find Action, Go to File…).
//!
//! [`route_terminal_key`] decides; the view that owns the terminal carries the verdict out and
//! keeps a consumed key from the shortcut dispatcher.

use crate::platform::dispatcher::Mods;

/// A key press as the terminal reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEvent<'a> {
    /// What the key types or names in the active layout: `c`, `C`, `Enter`, `ArrowLeft`, `Escape`.
    pub key: &'a str,
    /// The physical key, whatever the layout: `KeyC`, `BracketLeft`, `Space`.
    pub code: &'a str,
    pub mods: Mods,
    /// An input method is composing with this key: it is never the terminal's.
    pub ime: bool,
}

impl<'a> KeyEvent<'a> {
    pub fn new(key: &'a str, code: &'a str, mods: Mods) -> Self {
        Self { key, code, mods, ime: false }
    }
}

/// What a terminal does with a key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalKeyAction {
    /// Write this to the shell.
    Send(String),
    Copy,
    Paste,
    Clear,
    Close,
    Find,
    ClearSelection,
    /// Not handled here: the key takes its usual path.
    Pass,
}

fn send(data: &str) -> TerminalKeyAction {
    TerminalKeyAction::Send(data.to_owned())
}

/// The letter a key stands for in shortcuts: the one it types, else (a layout without Latin
/// letters, or Alt composing another character) the one printed on the physical key.
fn letter_of(e: &KeyEvent) -> Option<char> {
    let mut typed = e.key.chars();
    if let (Some(c), None) = (typed.next(), typed.next())
        && c.is_ascii_alphabetic()
        && !e.mods.alt
    {
        return Some(c.to_ascii_uppercase());
    }
    match e.code.as_bytes() {
        [b'K', b'e', b'y', letter] if letter.is_ascii_uppercase() => Some(char::from(*letter)),
        _ => None,
    }
}

/// Maps a key event in a terminal to what the terminal should do with it. `mac`: the app runs on
/// macOS, where Ctrl+C always interrupts and ⌘C copies.
pub fn terminal_key_action(e: &KeyEvent, has_selection: bool, mac: bool) -> TerminalKeyAction {
    use TerminalKeyAction::{Clear, ClearSelection, Close, Copy, Find, Pass, Paste};
    let Mods { meta, ctrl, alt, shift } = e.mods;
    if meta && !ctrl && !alt {
        if !shift {
            match e.key.to_lowercase().as_str() {
                "c" => return if has_selection { Copy } else { Pass },
                "v" => return Paste,
                "k" => return Clear,
                "w" => return Close,
                "f" => return Find,
                _ => {}
            }
        }
        return match e.key {
            "ArrowLeft" => send("\x01"),
            "ArrowRight" => send("\x05"),
            "Backspace" => send("\x15"),
            _ => Pass,
        };
    }
    if alt && !ctrl && !meta && !shift {
        return match e.key {
            "ArrowLeft" => send("\x1bb"),
            "ArrowRight" => send("\x1bf"),
            "Backspace" => send("\x17"),
            _ => Pass,
        };
    }
    let letter = letter_of(e);
    if ctrl && !meta && !alt && letter == Some('C') && has_selection && !mac {
        return Copy;
    }
    if ctrl && shift && !meta && !alt {
        match letter {
            Some('C') => return if has_selection { Copy } else { Pass },
            Some('V') => return Paste,
            // New Tab on Windows / Linux (IDEA keymap); plain Ctrl+T still reaches the shell.
            Some('T') => return Pass,
            _ => {}
        }
    }
    if ctrl && !meta && !alt {
        // The letter the key types, so Shift doesn't change the control code and AZERTY's Ctrl+Z is ^Z, not ^W.
        if let Some(letter) = letter {
            return TerminalKeyAction::Send(char::from(letter as u8 - b'@').to_string());
        }
        return match e.code {
            "BracketLeft" => send("\x1b"),
            "Backslash" => send("\x1c"),
            "BracketRight" => send("\x1d"),
            "Space" => send("\0"),
            _ => Pass,
        };
    }
    // Alt+Enter's Esc+Return, which Claude Code and similar TUIs take as a newline rather than
    // submit; Claude Code's /terminal-setup binds the same in VS Code, Zed and Alacritty.
    if shift && !meta && !ctrl && !alt && e.key == "Enter" {
        return send("\x1b\r");
    }
    if !meta && !ctrl && !alt && e.key == "Escape" {
        // IDEA's Terminal.Escape: a selection is cleared first.
        return if has_selection && !shift { ClearSelection } else { send("\x1b") };
    }
    Pass
}

/// Actions whose shortcuts the terminal leaves to the IDE (the same set IDEA allows). Next /
/// Previous Tab are left out as on Windows / Linux, where the shell needs those keys.
const ALLOWED_ACTION_IDS: [&str; 33] = [
    "toolwindow.terminal",
    "toolwindow.project",
    "toolwindow.bookmarks",
    "toolwindow.find",
    "toolwindow.run",
    "toolwindow.debug",
    "toolwindow.problems",
    "toolwindow.structure",
    "toolwindow.services",
    "toolwindow.commit",
    "toolwindow.git",
    "toolwindow.hideActive",
    "toolwindow.hideAll",
    "toolwindow.maximize",
    "window.next",
    "window.previous",
    "navigate.showBookmarks",
    "find.inPath",
    "navigate.findAction",
    "navigate.gotoFile",
    "navigate.gotoClass",
    "navigate.gotoSymbol",
    "vcs.push",
    "file.settings",
    "navigate.recentFiles",
    "navigate.switcher",
    "navigate.switcherBack",
    "terminal.new",
    "terminal.find",
    "terminal.clear",
    "terminal.closeSession",
    "terminal.splitRight",
    "terminal.splitDown",
];

/// The action Close Session runs through, so that it can ask before ending a running command.
pub const CLOSE_SESSION_ACTION: &str = "terminal.closeSession";
/// The action that opens the terminal's find bar.
pub const FIND_ACTION: &str = "terminal.find";

/// Whether `stroke` starts a binding of an allowed action. `bindings`: the keymap's
/// (binding, action id) pairs.
pub fn is_allowed_ide_shortcut<B: AsRef<str>, A: AsRef<str>>(stroke: &str, bindings: &[(B, A)]) -> bool {
    bindings.iter().any(|(binding, id)| {
        let (binding, id) = (binding.as_ref(), id.as_ref());
        let starts = binding == stroke || binding.strip_prefix(stroke).is_some_and(|rest| rest.starts_with(' '));
        // Ctrl+E stays with the shell (end of line), as in IDEA.
        starts && ALLOWED_ACTION_IDS.contains(&id) && !(id == "navigate.recentFiles" && stroke == "Ctrl+E")
    })
}

/// Strokes a focused terminal keeps from the IDE keymap whatever the settings say: embedded
/// terminals and sessions with shortcut overrides off still own copy and interrupt.
pub fn claims_copy_stroke(stroke: &str) -> bool {
    matches!(stroke, "Ctrl+C" | "Ctrl+Insert" | "Ctrl+Shift+C" | "Meta+C")
}

/// Where the key was pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyFocus {
    /// In the terminal's find bar.
    FindBar,
    /// In a terminal session.
    Session { has_selection: bool },
}

/// What [`route_terminal_key`] needs to know besides the key.
#[derive(Debug, Clone, Copy)]
pub struct KeyContext<'a> {
    /// The canonical stroke of the key ([`crate::platform::dispatcher::stroke`]); `None` for a
    /// bare modifier.
    pub stroke: Option<&'a str>,
    pub focus: KeyFocus,
    /// Settings › Terminal › Override IDE shortcuts.
    pub override_ide_shortcuts: bool,
    /// Settings › Terminal › Move focus to the editor with Escape.
    pub escape_to_editor: bool,
    /// The effective keymap's (binding, action id) pairs.
    pub bindings: &'a [(String, String)],
    /// The app runs on macOS.
    pub mac: bool,
}

/// The verdict on a key pressed in a terminal or its find bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyHandling {
    /// Not the terminal's: the IDE keymap, then the terminal's own typing, get the key.
    Pass,
    /// Write this to the shell.
    Send(String),
    /// Copy the selection to the clipboard.
    Copy,
    /// Paste the clipboard into the terminal.
    Paste,
    ClearSelection,
    /// Clear the terminal's buffer.
    Clear,
    /// Run this action ([`CLOSE_SESSION_ACTION`], [`FIND_ACTION`]).
    RunAction(&'static str),
    /// Close the find bar (instead of "focus editor").
    CloseFind,
    /// Reselect the find bar's query.
    OpenFind,
}

impl KeyHandling {
    /// The key is the terminal's: the IDE keymap must not see it.
    pub fn consumed(&self) -> bool {
        !matches!(self, Self::Pass)
    }
}

/// Decides what a key pressed in a terminal (or its find bar) does, before the IDE keymap runs.
pub fn route_terminal_key(e: &KeyEvent, cx: &KeyContext) -> KeyHandling {
    if e.ime {
        return KeyHandling::Pass;
    }
    let has_selection = match cx.focus {
        KeyFocus::FindBar => {
            let Mods { meta, ctrl, alt, shift } = e.mods;
            let plain = !ctrl && !alt && !shift;
            return if plain && !meta && e.key == "Escape" {
                KeyHandling::CloseFind
            } else if plain && meta && e.key.eq_ignore_ascii_case("f") {
                KeyHandling::OpenFind
            } else {
                KeyHandling::Pass
            };
        }
        KeyFocus::Session { has_selection } => has_selection,
    };
    let action = terminal_key_action(e, has_selection, cx.mac);
    if matches!(action, TerminalKeyAction::Send(_)) && (e.mods.ctrl || e.key == "Escape") {
        // "Override IDE shortcuts" off: Ctrl+key and Escape reach the IDE keymap first; keys it
        // doesn't bind still get to the shell as typing.
        if !cx.override_ide_shortcuts {
            return KeyHandling::Pass;
        }
        if cx.stroke.is_some_and(|stroke| is_allowed_ide_shortcut(stroke, cx.bindings)) {
            return KeyHandling::Pass;
        }
        // "Move focus to the Editor with Escape": the IDE keymap's Escape (Focus Editor) runs instead.
        if e.key == "Escape" && cx.escape_to_editor {
            return KeyHandling::Pass;
        }
    }
    match action {
        TerminalKeyAction::Pass => KeyHandling::Pass,
        TerminalKeyAction::Send(data) => KeyHandling::Send(data),
        TerminalKeyAction::Copy => KeyHandling::Copy,
        TerminalKeyAction::Paste => KeyHandling::Paste,
        TerminalKeyAction::ClearSelection => KeyHandling::ClearSelection,
        TerminalKeyAction::Clear => KeyHandling::Clear,
        TerminalKeyAction::Close => KeyHandling::RunAction(CLOSE_SESSION_ACTION),
        TerminalKeyAction::Find => KeyHandling::RunAction(FIND_ACTION),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::dispatcher::stroke;
    use crate::platform::keymap::{Keymap, KeymapState};

    const CTRL: Mods = Mods { ctrl: true, alt: false, shift: false, meta: false };
    const CTRL_SHIFT: Mods = Mods { ctrl: true, alt: false, shift: true, meta: false };
    const ALT: Mods = Mods { ctrl: false, alt: true, shift: false, meta: false };
    const SHIFT: Mods = Mods { ctrl: false, alt: false, shift: true, meta: false };
    const META: Mods = Mods { ctrl: false, alt: false, shift: false, meta: true };

    fn key<'a>(key: &'a str, code: &'a str, mods: Mods) -> KeyEvent<'a> {
        KeyEvent::new(key, code, mods)
    }

    /// The key pressed in a session of a Windows host, with the keymap's bindings.
    fn route(e: &KeyEvent, name: &str, has_selection: bool, overriding: bool, keymap: &KeymapState) -> KeyHandling {
        let pressed = stroke(name, e.mods);
        let cx = KeyContext {
            stroke: Some(pressed.as_str()),
            focus: KeyFocus::Session { has_selection },
            override_ide_shortcuts: overriding,
            escape_to_editor: false,
            bindings: keymap.effective_bindings(),
            mac: false,
        };
        route_terminal_key(e, &cx)
    }

    // src/features/terminal/copyKeys.test.ts › terminal copy capture › copies before xterm and IDE dispatch with
    // override shortcuts %s
    #[test]
    fn copies_before_the_ide_dispatch_with_override_shortcuts_on_and_off() {
        let keymap = KeymapState::new("intellij-windows");
        for overriding in [true, false] {
            for mods in [CTRL, CTRL_SHIFT] {
                let e = key("c", "KeyC", mods);
                assert_eq!(route(&e, "C", true, overriding, &keymap), KeyHandling::Copy);
                assert!(claims_copy_stroke(&stroke("C", mods)));
            }
        }
    }

    // src/features/terminal/copyKeys.test.ts › terminal copy capture › sends SIGINT when there is no selection
    #[test]
    fn sends_sigint_when_there_is_no_selection() {
        let keymap = KeymapState::new("intellij-windows");
        let handling = route(&key("c", "KeyC", CTRL), "C", false, true, &keymap);
        assert_eq!(handling, KeyHandling::Send("\x03".to_owned()));
        assert!(handling.consumed());
    }

    // src/features/terminal/copyKeys.test.ts › terminal copy capture › sends Shift+Enter as Esc+Return with override
    // shortcuts %s
    #[test]
    fn sends_shift_enter_as_esc_return_with_override_shortcuts_on_and_off() {
        let keymap = KeymapState::new("intellij-windows");
        for overriding in [true, false] {
            let handling = route(&key("Enter", "Enter", SHIFT), "Enter", false, overriding, &keymap);
            assert_eq!(handling, KeyHandling::Send("\x1b\r".to_owned()));
            // Plain Enter, other modifiers and an input method's Enter stay with the terminal's typing.
            for mods in [Mods::NONE, ALT, CTRL_SHIFT] {
                let handling = route(&key("Enter", "Enter", mods), "Enter", false, overriding, &keymap);
                assert_eq!(handling, KeyHandling::Pass);
            }
            let composing = KeyEvent { ime: true, ..key("Enter", "Enter", SHIFT) };
            assert!(!route(&composing, "Enter", false, overriding, &keymap).consumed());
        }
    }

    // src/features/terminal/copyKeys.test.ts › keeps terminal handling for %s even when bound to font zoom
    #[test]
    fn keeps_terminal_handling_even_when_bound_to_font_zoom() {
        let cases = [
            ("Ctrl+C", "c", "KeyC", "C", CTRL, true, KeyHandling::Copy),
            ("Ctrl+R", "r", "KeyR", "R", CTRL, false, KeyHandling::Send("\x12".to_owned())),
            ("Shift+Enter", "Enter", "Enter", "Enter", SHIFT, false, KeyHandling::Send("\x1b\r".to_owned())),
        ];
        for (binding, pressed, code, stroke_key, mods, has_selection, expected) in cases {
            let mut keymap = KeymapState::new("intellij-windows");
            keymap.set_overrides(Keymap::from([("editor.increaseFontSize".to_owned(), vec![binding.to_owned()])]));
            assert!(keymap.effective_bindings().iter().any(|(bound, _)| bound == binding));
            assert_eq!(route(&key(pressed, code, mods), stroke_key, has_selection, true, &keymap), expected);
        }
    }

    // src/features/terminal/terminal.test.ts › terminal keys › sends Ctrl+letter to the shell instead of IDE shortcuts
    #[test]
    fn sends_ctrl_letter_to_the_shell_instead_of_ide_shortcuts() {
        for mac in [true, false] {
            let action = |k: &str, code: &str, mods: Mods, has_selection: bool| {
                terminal_key_action(&key(k, code, mods), has_selection, mac)
            };
            assert_eq!(action("r", "KeyR", CTRL, false), send("\x12"));
            assert_eq!(action("d", "KeyD", CTRL, false), send("\x04"));
            assert_eq!(action("c", "KeyC", CTRL, false), send("\x03"));
            assert_eq!(action("C", "KeyC", CTRL_SHIFT, true), TerminalKeyAction::Copy);
            assert_eq!(action("V", "KeyV", CTRL_SHIFT, false), TerminalKeyAction::Paste);
            // Ctrl+Shift+T is New Tab in the IDE keymap; Ctrl+T still goes to the shell.
            assert_eq!(action("T", "KeyT", CTRL_SHIFT, false), TerminalKeyAction::Pass);
            assert_eq!(action("t", "KeyT", CTRL, false), send("\x14"));
            // AZERTY: the key typing "z" is physically KeyW; Ctrl+Z must send ^Z (SIGTSTP), not ^W.
            assert_eq!(action("z", "KeyW", CTRL, false), send("\x1a"));
            // Non-QWERTY Ctrl+Shift+C / V / T match the typed letter, not the physical key.
            assert_eq!(action("C", "KeyI", CTRL_SHIFT, true), TerminalKeyAction::Copy);
            assert_eq!(action("V", "KeyPeriod", CTRL_SHIFT, false), TerminalKeyAction::Paste);
            assert_eq!(action("T", "KeyK", CTRL_SHIFT, false), TerminalKeyAction::Pass);
            assert_eq!(action("J", "KeyC", CTRL_SHIFT, true), send("\x0a"));
        }
    }

    // src/features/terminal/terminal.test.ts › terminal keys › copies terminal selections with %o
    #[test]
    fn copies_terminal_selections_on_windows() {
        let strokes = [("c", "KeyC", CTRL), ("c", "KeyI", CTRL), ("C", "KeyC", CTRL_SHIFT), ("c", "KeyC", META)];
        for (k, code, mods) in strokes {
            assert_eq!(terminal_key_action(&key(k, code, mods), true, false), TerminalKeyAction::Copy);
        }
    }

    // src/features/terminal/terminal.test.ts › terminal keys › keeps Ctrl+C as interrupt on macOS even with a
    // selection
    #[test]
    fn keeps_ctrl_c_as_interrupt_on_macos_even_with_a_selection() {
        assert_eq!(terminal_key_action(&key("c", "KeyC", CTRL), true, true), send("\x03"));
    }

    // src/features/terminal/terminal.test.ts › terminal keys › implements macOS conveniences
    #[test]
    fn implements_macos_conveniences() {
        let action =
            |k: &str, mods: Mods, has_selection: bool| terminal_key_action(&key(k, "", mods), has_selection, true);
        assert_eq!(action("c", META, true), TerminalKeyAction::Copy);
        assert_eq!(action("c", META, false), TerminalKeyAction::Pass);
        assert_eq!(action("k", META, false), TerminalKeyAction::Clear);
        assert_eq!(action("t", META, false), TerminalKeyAction::Pass);
        assert_eq!(action("v", META, false), TerminalKeyAction::Paste);
        assert_eq!(action("f", META, false), TerminalKeyAction::Find);
        assert_eq!(action("ArrowLeft", ALT, false), send("\x1bb"));
        assert_eq!(action("Escape", Mods::NONE, false), send("\x1b"));
    }

    // src/features/terminal/terminal.test.ts › terminal keys › leaves other IDE shortcuts alone
    #[test]
    fn leaves_other_ide_shortcuts_alone() {
        let action = |k: &str, mods: Mods| terminal_key_action(&key(k, "", mods), false, true);
        assert_eq!(action("F12", ALT), TerminalKeyAction::Pass);
        assert_eq!(action("1", META), TerminalKeyAction::Pass);
        assert_eq!(action("a", Mods::NONE), TerminalKeyAction::Pass);
    }

    // src/features/terminal/terminal.test.ts › terminal keys › sends Shift+Enter as Esc+Return, a newline for Claude
    // Code instead of submitting
    #[test]
    fn sends_shift_enter_as_esc_return() {
        let action = |code: &str, mods: Mods, has_selection: bool| {
            terminal_key_action(&key("Enter", code, mods), has_selection, true)
        };
        assert_eq!(action("Enter", SHIFT, false), send("\x1b\r"));
        assert_eq!(action("NumpadEnter", SHIFT, true), send("\x1b\r"));
        assert_eq!(action("Enter", Mods::NONE, false), TerminalKeyAction::Pass);
        for mods in [CTRL_SHIFT, Mods { alt: true, ..SHIFT }, Mods { meta: true, ..SHIFT }] {
            assert_eq!(action("Enter", mods, false), TerminalKeyAction::Pass);
        }
    }

    // src/features/terminal/terminal.test.ts › terminal keys › clears a selection with Escape before sending it
    #[test]
    fn clears_a_selection_with_escape_before_sending_it() {
        let action = |mods: Mods| terminal_key_action(&key("Escape", "", mods), true, true);
        assert_eq!(action(Mods::NONE), TerminalKeyAction::ClearSelection);
        assert_eq!(action(SHIFT), send("\x1b"));
    }

    // src/features/terminal/terminal.test.ts › terminal keys › lets shortcuts of IDEA's allowed actions reach the IDE
    #[test]
    fn lets_shortcuts_of_allowed_actions_reach_the_ide() {
        let bindings = [
            ("Ctrl+Shift+A", "navigate.findAction"),
            ("Ctrl+N", "navigate.gotoClass"),
            ("Ctrl+E", "navigate.recentFiles"),
            ("Ctrl+K", "vcs.commit"),
            ("Ctrl+Shift+K", "vcs.push"),
            ("Shift+Escape", "toolwindow.hideActive"),
            ("Ctrl+W Ctrl+F", "terminal.find"),
        ];
        assert!(is_allowed_ide_shortcut("Ctrl+Shift+A", &bindings));
        assert!(is_allowed_ide_shortcut("Ctrl+N", &bindings));
        assert!(is_allowed_ide_shortcut("Ctrl+Shift+K", &bindings));
        assert!(is_allowed_ide_shortcut("Shift+Escape", &bindings));
        assert!(is_allowed_ide_shortcut("Ctrl+W", &bindings));
        // Ctrl+E (end of line) and actions outside the list stay with the shell.
        assert!(!is_allowed_ide_shortcut("Ctrl+E", &bindings));
        assert!(!is_allowed_ide_shortcut("Ctrl+K", &bindings));
        assert!(!is_allowed_ide_shortcut("Ctrl+R", &bindings));
    }

    // Not from a spec: allowed IDE shortcuts, the settings and the find bar decide who gets Ctrl and Escape strokes.
    #[test]
    fn leaves_ctrl_and_escape_strokes_to_the_ide_when_told_to() {
        let bindings = vec![("Ctrl+Shift+A".to_owned(), "navigate.findAction".to_owned())];
        let cx = KeyContext {
            stroke: Some("Ctrl+Shift+A"),
            focus: KeyFocus::Session { has_selection: false },
            override_ide_shortcuts: true,
            escape_to_editor: false,
            bindings: &bindings,
            mac: true,
        };
        assert_eq!(route_terminal_key(&key("A", "KeyA", CTRL_SHIFT), &cx), KeyHandling::Pass);
        let escape = key("Escape", "Escape", Mods::NONE);
        let on_escape = KeyContext { stroke: Some("Escape"), ..cx };
        assert_eq!(route_terminal_key(&escape, &on_escape), KeyHandling::Send("\x1b".to_owned()));
        let to_editor = KeyContext { escape_to_editor: true, ..on_escape };
        assert_eq!(route_terminal_key(&escape, &to_editor), KeyHandling::Pass);
        let in_find = KeyContext { focus: KeyFocus::FindBar, ..on_escape };
        assert_eq!(route_terminal_key(&escape, &in_find), KeyHandling::CloseFind);
        assert_eq!(route_terminal_key(&key("f", "KeyF", META), &in_find), KeyHandling::OpenFind);
        assert_eq!(route_terminal_key(&key("w", "KeyW", META), &cx), KeyHandling::RunAction(CLOSE_SESSION_ACTION));
    }
}
