//! What a key press sends to the program in the terminal: xterm's sequences, as the shells and
//! full-screen programs on the other end of the PTY expect them.
//!
//! Printable text does not come through here: it arrives as text input (with the keyboard
//! layout, dead keys and the input method applied) and is sent as it is.

/// The modifier keys held with a key.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeyMods {
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
}

impl KeyMods {
    fn any(self) -> bool {
        self.shift || self.alt || self.ctrl
    }

    /// xterm's modifier parameter: 1 plus shift (1), alt (2), ctrl (4).
    fn parameter(self) -> u8 {
        1 + u8::from(self.shift) + 2 * u8::from(self.alt) + 4 * u8::from(self.ctrl)
    }
}

/// How the terminal is set up for keys.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeyModes {
    /// The program asked for application cursor keys (`ESC O A` instead of `ESC [ A`).
    pub app_cursor: bool,
    /// Option sends ESC before the key (Terminal › Use Option as Meta key) instead of typing
    /// the layout's special character.
    pub option_as_meta: bool,
}

/// A key that ends in a letter: `ESC [ A`, `ESC O A` in application mode, `ESC [ 1 ; m A` with
/// modifiers.
fn letter_key(letter: char, mods: KeyMods, application: bool) -> String {
    if mods.any() {
        format!("\x1b[1;{}{letter}", mods.parameter())
    } else if application {
        format!("\x1bO{letter}")
    } else {
        format!("\x1b[{letter}")
    }
}

/// A key that is a number: `ESC [ n ~`, `ESC [ n ; m ~` with modifiers.
fn tilde_key(number: u8, mods: KeyMods) -> String {
    if mods.any() { format!("\x1b[{number};{}~", mods.parameter()) } else { format!("\x1b[{number}~") }
}

/// The control character Ctrl makes of `key`, for the keys that have one.
fn control_character(key: &str) -> Option<char> {
    let mut chars = key.chars();
    let (Some(c), None) = (chars.next(), chars.next()) else {
        return (key == "space").then_some('\0');
    };
    match c.to_ascii_lowercase() {
        letter @ 'a'..='z' => Some(char::from(letter as u8 - b'a' + 1)),
        '@' | '2' | ' ' => Some('\0'),
        '[' | '3' => Some('\x1b'),
        '\\' | '4' => Some('\x1c'),
        ']' | '5' => Some('\x1d'),
        '^' | '6' => Some('\x1e'),
        '_' | '-' | '/' | '7' => Some('\x1f'),
        '8' => Some('\x7f'),
        _ => None,
    }
}

/// What pressing `key` (GPUI's name for it: `enter`, `up`, `f5`, `a`…) with `mods` sends.
/// None when the key sends nothing by itself: its text, if it has any, comes as text input.
pub fn encode_key(key: &str, mods: KeyMods, modes: KeyModes) -> Option<String> {
    let function_letter = |letter: char| Some(letter_key(letter, mods, true));
    match key {
        "enter" => Some(if mods.alt { "\x1b\r".to_owned() } else { "\r".to_owned() }),
        "tab" => Some(if mods.shift { "\x1b[Z".to_owned() } else { "\t".to_owned() }),
        "escape" => Some(if mods.alt { "\x1b\x1b".to_owned() } else { "\x1b".to_owned() }),
        "backspace" => {
            let erase = if mods.ctrl { "\x08" } else { "\x7f" };
            Some(if mods.alt { format!("\x1b{erase}") } else { erase.to_owned() })
        }
        // Option with the arrows moves by word in the shell (`ESC b`, `ESC f`), as in macOS's
        // own terminals.
        "left" if mods.alt && !mods.ctrl && !mods.shift => Some("\x1bb".to_owned()),
        "right" if mods.alt && !mods.ctrl && !mods.shift => Some("\x1bf".to_owned()),
        "up" => Some(letter_key('A', mods, modes.app_cursor)),
        "down" => Some(letter_key('B', mods, modes.app_cursor)),
        "right" => Some(letter_key('C', mods, modes.app_cursor)),
        "left" => Some(letter_key('D', mods, modes.app_cursor)),
        "home" => Some(letter_key('H', mods, modes.app_cursor)),
        "end" => Some(letter_key('F', mods, modes.app_cursor)),
        "insert" => Some(tilde_key(2, mods)),
        "delete" => Some(tilde_key(3, mods)),
        "pageup" => Some(tilde_key(5, mods)),
        "pagedown" => Some(tilde_key(6, mods)),
        "f1" => function_letter('P'),
        "f2" => function_letter('Q'),
        "f3" => function_letter('R'),
        "f4" => function_letter('S'),
        "f5" => Some(tilde_key(15, mods)),
        "f6" => Some(tilde_key(17, mods)),
        "f7" => Some(tilde_key(18, mods)),
        "f8" => Some(tilde_key(19, mods)),
        "f9" => Some(tilde_key(20, mods)),
        "f10" => Some(tilde_key(21, mods)),
        "f11" => Some(tilde_key(23, mods)),
        "f12" => Some(tilde_key(24, mods)),
        _ if mods.ctrl => {
            let control = control_character(key)?;
            Some(if mods.alt { format!("\x1b{control}") } else { control.to_string() })
        }
        // Option as Meta: ESC, then the key as typed without Option.
        _ if mods.alt && modes.option_as_meta => {
            let mut chars = key.chars();
            let (Some(c), None) = (chars.next(), chars.next()) else { return None };
            let typed = if mods.shift { c.to_ascii_uppercase() } else { c };
            Some(format!("\x1b{typed}"))
        }
        _ => None,
    }
}

/// Pasted text as the program should receive it: line ends as the Enter key sends them, and
/// with bracketed paste the markers around it, so an editor does not take it for typing. A
/// marker inside the text is dropped: it would end the bracket early and have the rest run as
/// typed commands.
pub fn encode_paste(text: &str, bracketed: bool) -> String {
    let text = text.replace("\r\n", "\r").replace('\n', "\r");
    if !bracketed {
        return text;
    }
    let inner = text.replace("\x1b[200~", "").replace("\x1b[201~", "");
    format!("\x1b[200~{inner}\x1b[201~")
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: KeyMods = KeyMods { shift: false, alt: false, ctrl: false };
    const SHIFT: KeyMods = KeyMods { shift: true, alt: false, ctrl: false };
    const ALT: KeyMods = KeyMods { shift: false, alt: true, ctrl: false };
    const CTRL: KeyMods = KeyMods { shift: false, alt: false, ctrl: true };

    fn plain(key: &str, mods: KeyMods) -> Option<String> {
        encode_key(key, mods, KeyModes::default())
    }

    #[test]
    fn editing_keys_send_their_control_characters() {
        assert_eq!(plain("enter", NONE).as_deref(), Some("\r"));
        assert_eq!(plain("enter", ALT).as_deref(), Some("\x1b\r"));
        assert_eq!(plain("tab", NONE).as_deref(), Some("\t"));
        assert_eq!(plain("tab", SHIFT).as_deref(), Some("\x1b[Z"));
        assert_eq!(plain("escape", NONE).as_deref(), Some("\x1b"));
        assert_eq!(plain("backspace", NONE).as_deref(), Some("\x7f"));
        assert_eq!(plain("backspace", CTRL).as_deref(), Some("\x08"));
        assert_eq!(plain("backspace", ALT).as_deref(), Some("\x1b\x7f"));
        assert_eq!(plain("delete", NONE).as_deref(), Some("\x1b[3~"));
    }

    #[test]
    fn cursor_keys_follow_the_application_mode_and_carry_modifiers() {
        let application = KeyModes { app_cursor: true, option_as_meta: false };
        assert_eq!(plain("up", NONE).as_deref(), Some("\x1b[A"));
        assert_eq!(encode_key("up", NONE, application).as_deref(), Some("\x1bOA"));
        assert_eq!(encode_key("home", NONE, application).as_deref(), Some("\x1bOH"));
        assert_eq!(plain("end", NONE).as_deref(), Some("\x1b[F"));
        // With a modifier the sequence is the same in both modes.
        assert_eq!(plain("left", SHIFT).as_deref(), Some("\x1b[1;2D"));
        assert_eq!(encode_key("left", CTRL, application).as_deref(), Some("\x1b[1;5D"));
        let all = KeyMods { shift: true, alt: true, ctrl: true };
        assert_eq!(plain("down", all).as_deref(), Some("\x1b[1;8B"));
        // Option with left and right moves by word.
        assert_eq!(plain("left", ALT).as_deref(), Some("\x1bb"));
        assert_eq!(plain("right", ALT).as_deref(), Some("\x1bf"));
        assert_eq!(plain("up", ALT).as_deref(), Some("\x1b[1;3A"));
    }

    #[test]
    fn paging_and_function_keys_send_xterms_sequences() {
        assert_eq!(plain("pageup", NONE).as_deref(), Some("\x1b[5~"));
        assert_eq!(plain("pagedown", CTRL).as_deref(), Some("\x1b[6;5~"));
        assert_eq!(plain("insert", NONE).as_deref(), Some("\x1b[2~"));
        assert_eq!(plain("f1", NONE).as_deref(), Some("\x1bOP"));
        assert_eq!(plain("f4", SHIFT).as_deref(), Some("\x1b[1;2S"));
        assert_eq!(plain("f5", NONE).as_deref(), Some("\x1b[15~"));
        assert_eq!(plain("f12", ALT).as_deref(), Some("\x1b[24;3~"));
    }

    #[test]
    fn ctrl_makes_control_characters_and_text_keys_send_nothing_themselves() {
        assert_eq!(plain("c", CTRL).as_deref(), Some("\x03"));
        assert_eq!(plain("z", CTRL).as_deref(), Some("\x1a"));
        assert_eq!(plain("space", CTRL).as_deref(), Some("\0"));
        assert_eq!(plain("[", CTRL).as_deref(), Some("\x1b"));
        assert_eq!(plain("\\", CTRL).as_deref(), Some("\x1c"));
        assert_eq!(plain("/", CTRL).as_deref(), Some("\x1f"));
        assert_eq!(plain("d", KeyMods { shift: false, alt: true, ctrl: true }).as_deref(), Some("\x1b\x04"));
        assert_eq!(plain("f13", CTRL), None);
        // Typing is text input, with or without Option.
        assert_eq!(plain("a", NONE), None);
        assert_eq!(plain("a", SHIFT), None);
        assert_eq!(plain("a", ALT), None);
        let meta = KeyModes { app_cursor: false, option_as_meta: true };
        assert_eq!(encode_key("b", ALT, meta).as_deref(), Some("\x1bb"));
        assert_eq!(encode_key("b", KeyMods { shift: true, alt: true, ctrl: false }, meta).as_deref(), Some("\x1bB"));
        assert_eq!(encode_key("space", ALT, meta), None);
    }

    #[test]
    fn pasted_text_gets_enter_line_ends_and_brackets_when_asked() {
        assert_eq!(encode_paste("a\r\nb\nc", false), "a\rb\rc");
        assert_eq!(encode_paste("ls\n", true), "\x1b[200~ls\r\x1b[201~");
        // A marker in the text cannot close the bracket.
        assert_eq!(encode_paste("x\x1b[201~rm -rf\n", true), "\x1b[200~xrm -rf\r\x1b[201~");
    }
}
