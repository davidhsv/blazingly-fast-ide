//! Keyboard and mouse shortcut dispatch: which action a keystroke runs.
//!
//! A state machine over key, modifier and mouse events. It decides; the caller runs the action
//! and stops the event's propagation when told to. Time is passed in, so chords (2 s for the
//! second stroke) and double-modifier gestures (300 ms per step) are testable without a clock.

use super::keymap::{Modifier, normalize_binding};

/// How long IDEA waits for the second stroke of a chord.
pub const CHORD_TIMEOUT_MS: f64 = 2000.0;
/// IDEA's double-modifier timing: each press/release step.
pub const DOUBLE_TAP_STEP_MS: f64 = 300.0;
/// A modifier tap this soon after typing another key is part of typing, not a gesture.
pub const DOUBLE_TAP_TYPING_GUARD_MS: f64 = 100.0;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Mods {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
}

impl Mods {
    pub const NONE: Mods = Mods { ctrl: false, alt: false, shift: false, meta: false };

    pub fn held(self, m: Modifier) -> bool {
        match m {
            Modifier::Ctrl => self.ctrl,
            Modifier::Alt => self.alt,
            Modifier::Shift => self.shift,
            Modifier::Meta => self.meta,
        }
    }

    pub fn with(mut self, m: Modifier, held: bool) -> Self {
        match m {
            Modifier::Ctrl => self.ctrl = held,
            Modifier::Alt => self.alt = held,
            Modifier::Shift => self.shift = held,
            Modifier::Meta => self.meta = held,
        }
        self
    }

    fn others_held(self, m: Modifier) -> bool {
        Modifier::ALL.into_iter().any(|x| x != m && self.held(x))
    }

    fn command(self) -> bool {
        self.meta || self.ctrl || self.alt
    }

    fn prefix(self) -> String {
        Modifier::ALL.into_iter().filter(|m| self.held(*m)).map(|m| format!("{}+", m.name())).collect()
    }
}

/// What has keyboard focus, as far as shortcut dispatch cares.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum KeyTarget {
    /// Lists, trees, the window itself.
    #[default]
    Other,
    /// A single- or multi-line text field that is not a code editor: plain typing stays there.
    TextField,
    /// A code editor: typing goes through the keymap first (editor actions are bound in it).
    Editor,
}

/// A key press, already named: `key` is the stroke's key (`J`, `Up`, `F8`, `[`, `NumpadAdd`).
#[derive(Debug, Clone, PartialEq)]
pub struct KeyPress {
    pub key: String,
    pub mods: Mods,
    pub target: KeyTarget,
    /// The key is being composed by an input method.
    pub ime: bool,
    /// AltGr is held and the key types this character (Windows / Linux layouts).
    pub alt_gr_char: Option<String>,
}

impl KeyPress {
    pub fn new(key: &str, mods: Mods) -> Self {
        Self { key: key.to_owned(), mods, target: KeyTarget::Other, ime: false, alt_gr_char: None }
    }

    pub fn on(mut self, target: KeyTarget) -> Self {
        self.target = target;
        self
    }

    /// The canonical stroke: modifiers in keymap order, then the key (`Ctrl+Shift+Z`).
    pub fn stroke(&self) -> String {
        stroke(&self.key, self.mods)
    }
}

/// The canonical stroke for `key` with `mods` held.
pub fn stroke(key: &str, mods: Mods) -> String {
    let key = if key.chars().count() == 1 { key.to_uppercase() } else { key.to_owned() };
    mods.prefix() + &key
}

/// Side buttons, or a left click with Ctrl/Alt/Meta (0-based `button`). Plain and Shift-only
/// left clicks keep selection semantics; middle/right clicks keep their own meaning.
pub fn mouse_stroke(button: u32, mods: Mods) -> Option<String> {
    if button < 3 && !(button == 0 && mods.command()) {
        return None;
    }
    Some(format!("{}Button{}", mods.prefix(), button + 1))
}

/// What the dispatcher asks of the keymap and the action registry.
pub trait DispatchEnv {
    /// Every (normalized binding, action id) pair, later pairs winning.
    fn bindings(&self) -> &[(String, String)];
    /// Whether a shortcut may run the action now (`when` and `keyWhen`); false when unknown.
    fn key_enabled(&self, action_id: &str) -> bool;
    /// Whether the focused component keeps this first stroke for itself (Vim mode…).
    fn claimed(&self, _press: &KeyPress, _stroke: &str) -> bool {
        false
    }
}

/// The dispatcher's verdict on an event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Not a shortcut: the event goes on to its target.
    Pass,
    /// Run this action; the event is consumed.
    Run(String),
    /// First stroke of a chord: consumed, waiting for the second.
    ChordPending,
    /// Consumed without effect (a second stroke that completes no chord, a side button).
    Swallow,
}

impl Outcome {
    pub fn consumed(&self) -> bool {
        !matches!(self, Outcome::Pass)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Tap {
    modifier: Modifier,
    /// 1 pressed, 2 released, 3 pressed again.
    step: u8,
    at: f64,
    typed: bool,
    key_used: bool,
}

#[derive(Debug)]
pub struct Dispatcher {
    mac: bool,
    pending: Vec<String>,
    pending_at: f64,
    tap: Option<Tap>,
    other_key_at: f64,
    suspended: u32,
}

impl Dispatcher {
    pub fn new(mac: bool) -> Self {
        Self { mac, pending: Vec::new(), pending_at: 0.0, tap: None, other_key_at: f64::NEG_INFINITY, suspended: 0 }
    }

    /// Pauses dispatch (e.g. while recording a shortcut); calls nest.
    pub fn suspend(&mut self) {
        self.suspended += 1;
    }

    pub fn resume(&mut self) {
        self.suspended = self.suspended.saturating_sub(1);
    }

    pub fn is_suspended(&self) -> bool {
        self.suspended > 0
    }

    /// Whether a chord's first stroke is waiting for its second.
    pub fn chord_pending(&self, now: f64) -> bool {
        !self.pending.is_empty() && now - self.pending_at <= CHORD_TIMEOUT_MS
    }

    fn reset(&mut self) {
        self.pending.clear();
    }

    /// The window lost focus: gestures in progress are over.
    pub fn blur(&mut self) {
        self.reset();
        self.tap = None;
    }

    fn first_enabled(env: &dyn DispatchEnv, bindings: &[(String, String)], binding: &str) -> Option<String> {
        // Later presets/overrides win; the first enabled match runs.
        bindings.iter().rev().filter(|(b, _)| b == binding).map(|(_, id)| id).find(|id| env.key_enabled(id)).cloned()
    }

    /// A modifier key went down. `mods` is the state including it.
    pub fn modifier_down(&mut self, modifier: Modifier, mods: Mods, repeat: bool, now: f64) {
        if self.suspended > 0 {
            return self.blur();
        }
        if repeat && self.tap.is_some_and(|t| t.step == 3 && t.modifier == modifier) {
            return;
        }
        if repeat || mods.others_held(modifier) {
            self.tap = None;
            return;
        }
        let typed = now - self.other_key_at < DOUBLE_TAP_TYPING_GUARD_MS;
        if self.tap.is_some_and(|t| t.modifier != modifier || now - t.at > DOUBLE_TAP_STEP_MS) {
            self.tap = None;
        }
        self.tap = match self.tap {
            None => Some(Tap { modifier, step: 1, at: now, typed, key_used: false }),
            Some(t) if t.step == 2 => Some(Tap { step: 3, at: now, typed: t.typed || typed, ..t }),
            Some(_) => None,
        };
    }

    /// A modifier key went up. `mods` is the state after the release. A bare double tap
    /// (`Shift Shift`) fires here, on the second release.
    pub fn modifier_up(&mut self, modifier: Modifier, mods: Mods, now: f64, env: &dyn DispatchEnv) -> Outcome {
        let Some(tap) = self.tap.filter(|t| self.suspended == 0 && t.modifier == modifier) else {
            return Outcome::Pass;
        };
        if mods.others_held(modifier) || now - tap.at > DOUBLE_TAP_STEP_MS {
            self.tap = None;
            return Outcome::Pass;
        }
        if tap.step == 1 {
            self.tap = Some(Tap { step: 2, at: now, ..tap });
            return Outcome::Pass;
        }
        self.tap = None;
        if tap.step != 3 || tap.key_used || tap.typed {
            return Outcome::Pass;
        }
        let binding = format!("{0} {0}", modifier.name());
        Self::first_enabled(env, env.bindings(), &binding).map_or(Outcome::Pass, Outcome::Run)
    }

    /// A key pressed while the second press of a modifier is held: its `Alt Alt+Up` gesture.
    fn held_gesture(&mut self, press: &KeyPress, env: &dyn DispatchEnv) -> Option<String> {
        let tap =
            self.tap.filter(|t| t.step == 3 && press.mods.held(t.modifier) && !press.mods.others_held(t.modifier))?;
        let binding = normalize_binding(&format!("{0} {0}+{1}", tap.modifier.name(), stroke(&press.key, Mods::NONE)));
        let id = Self::first_enabled(env, env.bindings(), &binding)?;
        if let Some(t) = &mut self.tap {
            t.key_used = true;
        }
        Some(id)
    }

    /// AltGr (reported as Ctrl+Alt on Windows) typing a character into a text field or editor,
    /// e.g. `}` on a German or `ł` on a Polish layout: the character wins over a Ctrl+Alt
    /// shortcut on the same key, as in IDEA. Never on macOS, where ⌥ shortcuts type symbols.
    pub fn is_alt_gr_typing(&self, press: &KeyPress) -> bool {
        if self.mac || press.target == KeyTarget::Other {
            return false;
        }
        // Ctrl+Alt+L on a layout with no AltGr character there still reports `l`: that is a shortcut.
        press
            .alt_gr_char
            .as_deref()
            .is_some_and(|c| c.chars().count() == 1 && c.to_uppercase() != stroke(&press.key, Mods::NONE))
    }

    /// A non-modifier key went down.
    pub fn key_down(&mut self, press: &KeyPress, now: f64, env: &dyn DispatchEnv) -> Outcome {
        if self.suspended > 0 {
            self.blur();
            return Outcome::Pass;
        }
        if let Some(id) = self.held_gesture(press, env) {
            self.reset();
            return Outcome::Run(id);
        }
        self.tap = None;
        self.other_key_at = if matches!(press.key.as_str(), "Escape" | "Tab") { f64::NEG_INFINITY } else { now };
        if press.ime {
            return Outcome::Pass;
        }
        if !self.pending.is_empty() && now - self.pending_at > CHORD_TIMEOUT_MS {
            self.reset();
        }
        let stroke = press.stroke();
        let chord = !self.pending.is_empty();
        if !chord {
            if self.is_alt_gr_typing(press) {
                return Outcome::Pass;
            }
            let function_or_escape = {
                let bare = stroke.strip_prefix("Shift+").unwrap_or(&stroke);
                bare == "Escape"
                    || (bare.len() <= 3
                        && bare
                            .strip_prefix('F')
                            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())))
            };
            if !press.mods.command() && press.target == KeyTarget::TextField && !function_or_escape {
                return Outcome::Pass;
            }
            if env.claimed(press, &stroke) {
                return Outcome::Pass;
            }
        }

        let bindings = env.bindings();
        let physical = self.pending.iter().map(String::as_str).chain([stroke.as_str()]).collect::<Vec<_>>().join(" ");
        let starts = |seq: &str| {
            bindings.iter().any(|(b, _)| b.len() > seq.len() && b.starts_with(seq) && b.as_bytes()[seq.len()] == b' ')
        };
        // Preserve older presets' numeric-keypad aliases unless a preset distinguishes the physical key.
        let seq = if bindings.iter().any(|(b, _)| *b == physical) || starts(&physical) {
            physical
        } else {
            physical
                .replace("NumpadAdd", "+")
                .replace("NumpadSubtract", "-")
                .replace("NumpadMultiply", "*")
                .replace("NumpadDivide", "/")
        };
        if let Some(id) = Self::first_enabled(env, bindings, &seq) {
            self.reset();
            return Outcome::Run(id);
        }
        if starts(&seq) {
            self.pending.push(stroke);
            self.pending_at = now;
            return Outcome::ChordPending;
        }
        self.reset();
        // A second stroke that completes no chord is swallowed, as in IDEA, not typed.
        if chord { Outcome::Swallow } else { Outcome::Pass }
    }

    /// A mouse button went down (0-based `button`). `in_editor_text` is a left click on editor
    /// text: the editor sets the caret under the pointer first and then calls
    /// [`Dispatcher::mouse_shortcut`] itself.
    pub fn mouse_down(&mut self, button: u32, mods: Mods, in_editor_text: bool, env: &dyn DispatchEnv) -> Outcome {
        // Shift+click (extend a selection) is not half of a double Shift.
        self.tap = None;
        if self.suspended > 0 {
            return Outcome::Pass;
        }
        let Some(stroke) = mouse_stroke(button, mods) else { return Outcome::Pass };
        if button == 0 {
            if in_editor_text {
                return Outcome::Pass;
            }
            let outcome = self.mouse_shortcut(button, mods, env);
            if outcome.consumed() {
                self.reset();
            }
            return outcome;
        }
        // Side buttons never reach the target (no editor selection, no focus change).
        self.reset();
        Self::first_enabled(env, env.bindings(), &stroke).map_or(Outcome::Swallow, Outcome::Run)
    }

    /// The action bound to a mouse stroke, if any is enabled.
    pub fn mouse_shortcut(&self, button: u32, mods: Mods, env: &dyn DispatchEnv) -> Outcome {
        if self.suspended > 0 {
            return Outcome::Pass;
        }
        mouse_stroke(button, mods)
            .and_then(|stroke| Self::first_enabled(env, env.bindings(), &stroke))
            .map_or(Outcome::Pass, Outcome::Run)
    }

    /// A trackpad page swipe (`SwipeLeft` / `SwipeRight`).
    pub fn swipe(&mut self, stroke: &str, env: &dyn DispatchEnv) -> Outcome {
        if self.suspended > 0 || !super::keymap::is_page_swipe(stroke) {
            return Outcome::Pass;
        }
        self.blur();
        Self::first_enabled(env, env.bindings(), stroke).map_or(Outcome::Pass, Outcome::Run)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    mod proofs_keypad;

    /// A keymap of overrides and the set of actions a shortcut may not run.
    #[derive(Default)]
    struct Env {
        bindings: Vec<(String, String)>,
        disabled: HashSet<String>,
        claimed: HashSet<String>,
    }

    impl Env {
        fn bound(pairs: &[(&str, &str)]) -> Self {
            Self {
                bindings: pairs.iter().map(|(id, b)| (normalize_binding(b), (*id).to_owned())).collect(),
                ..Default::default()
            }
        }
    }

    impl DispatchEnv for Env {
        fn bindings(&self) -> &[(String, String)] {
            &self.bindings
        }
        fn key_enabled(&self, action_id: &str) -> bool {
            !self.disabled.contains(action_id)
        }
        fn claimed(&self, _press: &KeyPress, stroke: &str) -> bool {
            self.claimed.contains(stroke)
        }
    }

    const META: Mods = Mods { meta: true, ..Mods::NONE };
    const SHIFT: Mods = Mods { shift: true, ..Mods::NONE };
    const ALT: Mods = Mods { alt: true, ..Mods::NONE };
    const CTRL: Mods = Mods { ctrl: true, ..Mods::NONE };

    fn run(id: &str) -> Outcome {
        Outcome::Run(id.to_owned())
    }

    // src/platform/keymap.test.ts › names letters by the character typed / uses physical key codes
    #[test]
    fn strokes_are_canonical() {
        assert_eq!(stroke("z", CTRL), "Ctrl+Z");
        assert_eq!(stroke("Z", Mods { ctrl: true, shift: true, ..Mods::NONE }), "Ctrl+Shift+Z");
        assert_eq!(stroke("F", ALT), "Alt+F");
        assert_eq!(stroke("Up", Mods { ctrl: true, alt: true, shift: true, meta: true }), "Ctrl+Alt+Shift+Meta+Up");
        assert_eq!(mouse_stroke(0, Mods::NONE), None);
        assert_eq!(mouse_stroke(0, SHIFT), None);
        assert_eq!(
            mouse_stroke(0, Mods { alt: true, shift: true, ..Mods::NONE }).as_deref(),
            Some("Alt+Shift+Button1")
        );
        assert_eq!(mouse_stroke(1, META), None);
        assert_eq!(mouse_stroke(3, Mods::NONE).as_deref(), Some("Button4"));
    }

    // src/platform/keymap.test.ts › dispatcher › runs bound actions and chords
    #[test]
    fn runs_bound_actions_and_chords() {
        let env = Env::bound(&[("t.single", "Meta+J"), ("t.chord", "Meta+K Meta+C")]);
        let mut d = Dispatcher::new(true);
        assert_eq!(d.key_down(&KeyPress::new("j", META), 0.0, &env), run("t.single"));
        assert_eq!(d.key_down(&KeyPress::new("k", META), 1.0, &env), Outcome::ChordPending);
        assert!(d.chord_pending(1.0));
        assert_eq!(d.key_down(&KeyPress::new("c", META), 2.0, &env), run("t.chord"));
        assert!(!d.chord_pending(2.0));
    }

    // src/platform/keymap.test.ts › dispatcher › respects when clauses / applies keyWhen to shortcuts only
    #[test]
    fn skips_actions_a_shortcut_may_not_run() {
        let mut env = Env::bound(&[("t.gated", "Meta+G"), ("t.fallback", "Meta+H"), ("t.keyGated", "Meta+H")]);
        env.disabled.extend(["t.gated".to_owned(), "t.keyGated".to_owned()]);
        let mut d = Dispatcher::new(true);
        assert_eq!(d.key_down(&KeyPress::new("g", META), 0.0, &env), Outcome::Pass);
        // The later binding is disabled, so the earlier one on the same keystroke runs.
        assert_eq!(d.key_down(&KeyPress::new("h", META), 0.0, &env), run("t.fallback"));
    }

    // src/platform/keymap.test.ts › dispatcher › waits 2s for a chord's second stroke and swallows a wrong one
    #[test]
    fn waits_2s_for_a_chords_second_stroke_and_swallows_a_wrong_one() {
        let env = Env::bound(&[("t.chord2", "Meta+K Meta+C")]);
        let mut d = Dispatcher::new(true);
        assert_eq!(d.key_down(&KeyPress::new("k", META), 0.0, &env), Outcome::ChordPending);
        assert_eq!(d.key_down(&KeyPress::new("c", META), 1900.0, &env), run("t.chord2"));

        assert_eq!(d.key_down(&KeyPress::new("k", META), 2000.0, &env), Outcome::ChordPending);
        assert_eq!(d.key_down(&KeyPress::new("x", Mods::NONE), 2001.0, &env), Outcome::Swallow);
        // The chord is over: the next stroke is an ordinary key again.
        assert_eq!(d.key_down(&KeyPress::new("c", META), 2002.0, &env), Outcome::Pass);

        assert_eq!(d.key_down(&KeyPress::new("k", META), 3000.0, &env), Outcome::ChordPending);
        assert_eq!(d.key_down(&KeyPress::new("c", META), 5001.0, &env), Outcome::Pass);
    }

    #[test]
    fn plain_typing_stays_in_text_fields_but_not_in_editors() {
        let env = Env::bound(&[("t.typeA", "A"), ("t.f2", "F2"), ("t.esc", "Shift+Escape"), ("t.meta", "Meta+A")]);
        let mut d = Dispatcher::new(true);
        let field = |key: &str, mods| KeyPress::new(key, mods).on(KeyTarget::TextField);
        assert_eq!(d.key_down(&field("a", Mods::NONE), 0.0, &env), Outcome::Pass);
        assert_eq!(d.key_down(&field("F2", Mods::NONE), 0.0, &env), run("t.f2"));
        assert_eq!(d.key_down(&field("Escape", SHIFT), 0.0, &env), run("t.esc"));
        assert_eq!(d.key_down(&field("a", META), 0.0, &env), run("t.meta"));
        assert_eq!(d.key_down(&KeyPress::new("a", Mods::NONE).on(KeyTarget::Editor), 0.0, &env), run("t.typeA"));
        assert_eq!(d.key_down(&KeyPress::new("a", Mods::NONE), 0.0, &env), run("t.typeA"));
    }

    #[test]
    fn claimed_first_strokes_and_ime_keys_pass_through() {
        let mut env = Env::bound(&[("t.r", "Ctrl+R"), ("t.chord", "Meta+K Ctrl+R")]);
        env.claimed.insert("Ctrl+R".to_owned());
        let mut d = Dispatcher::new(true);
        assert_eq!(d.key_down(&KeyPress::new("r", CTRL), 0.0, &env), Outcome::Pass);
        // A claim covers first strokes only: the chord's second stroke is the keymap's.
        assert_eq!(d.key_down(&KeyPress::new("k", META), 1.0, &env), Outcome::ChordPending);
        assert_eq!(d.key_down(&KeyPress::new("r", CTRL), 2.0, &env), run("t.chord"));
        let mut ime = KeyPress::new("k", META);
        ime.ime = true;
        assert_eq!(d.key_down(&ime, 3.0, &env), Outcome::Pass);
    }

    #[test]
    fn numeric_keypad_operators_alias_the_plain_keys_unless_bound_themselves() {
        let env = Env::bound(&[("t.plus", "Meta++"), ("t.minus", "Meta+-"), ("t.pad", "Meta+NumpadSubtract")]);
        let mut d = Dispatcher::new(true);
        assert_eq!(d.key_down(&KeyPress::new("NumpadAdd", META), 0.0, &env), run("t.plus"));
        assert_eq!(d.key_down(&KeyPress::new("NumpadSubtract", META), 0.0, &env), run("t.pad"));
        assert_eq!(d.key_down(&KeyPress::new("-", META), 0.0, &env), run("t.minus"));
    }

    #[test]
    fn suspension_nests_and_drops_gestures_in_progress() {
        let env = Env::bound(&[("t.single", "Meta+J"), ("t.chord", "Meta+K Meta+C")]);
        let mut d = Dispatcher::new(true);
        assert_eq!(d.key_down(&KeyPress::new("k", META), 0.0, &env), Outcome::ChordPending);
        d.suspend();
        d.suspend();
        assert_eq!(d.key_down(&KeyPress::new("j", META), 1.0, &env), Outcome::Pass);
        d.resume();
        assert!(d.is_suspended());
        d.resume();
        d.resume();
        assert!(!d.is_suspended());
        assert_eq!(d.key_down(&KeyPress::new("c", META), 2.0, &env), Outcome::Pass);
        assert_eq!(d.key_down(&KeyPress::new("j", META), 3.0, &env), run("t.single"));
    }

    struct Shift<'a> {
        d: Dispatcher,
        env: &'a Env,
    }

    impl Shift<'_> {
        fn down(&mut self, at: f64) {
            self.d.modifier_down(Modifier::Shift, SHIFT, false, at);
        }
        fn up(&mut self, at: f64) -> Outcome {
            self.d.modifier_up(Modifier::Shift, Mods::NONE, at, self.env)
        }
        fn tap(&mut self, at: f64) -> Outcome {
            self.down(at);
            self.up(at + 50.0)
        }
    }

    // src/platform/keymap.test.ts › double Shift › fires on the second release when every step is within 300ms
    #[test]
    fn double_shift_fires_on_the_second_release() {
        let env = Env::bound(&[("t.doubleShift", "Shift Shift")]);
        let mut s = Shift { d: Dispatcher::new(true), env: &env };
        assert_eq!(s.tap(1000.0), Outcome::Pass);
        assert_eq!(s.tap(1200.0), run("t.doubleShift"));
    }

    // src/platform/keymap.test.ts › double Shift › ignores slow steps
    #[test]
    fn double_shift_ignores_slow_steps() {
        let env = Env::bound(&[("t.doubleShift", "Shift Shift")]);
        let mut s = Shift { d: Dispatcher::new(true), env: &env };
        assert_eq!(s.tap(1000.0), Outcome::Pass);
        assert_eq!(s.tap(1400.0), Outcome::Pass);
        s.down(2000.0);
        assert_eq!(s.up(2400.0), Outcome::Pass); // held too long
        assert_eq!(s.tap(2500.0), Outcome::Pass);
    }

    // src/platform/keymap.test.ts › double Shift › ignores Shift tapped right after typing
    #[test]
    fn double_shift_ignores_shift_tapped_right_after_typing() {
        let env = Env::bound(&[("t.doubleShift", "Shift Shift")]);
        let mut s = Shift { d: Dispatcher::new(true), env: &env };
        assert_eq!(s.d.key_down(&KeyPress::new("a", Mods::NONE), 990.0, &env), Outcome::Pass);
        assert_eq!(s.tap(1000.0), Outcome::Pass);
        assert_eq!(s.tap(1150.0), Outcome::Pass);
    }

    // src/platform/keymap.test.ts › double Shift › is cancelled by a click or another key while Shift is held
    #[test]
    fn double_shift_is_cancelled_by_a_click_or_another_key() {
        let env = Env::bound(&[("t.doubleShift", "Shift Shift")]);
        let mut s = Shift { d: Dispatcher::new(true), env: &env };
        s.down(1000.0);
        assert_eq!(s.d.mouse_down(0, SHIFT, false, &env), Outcome::Pass);
        assert_eq!(s.up(1100.0), Outcome::Pass);
        assert_eq!(s.tap(1200.0), Outcome::Pass);

        s.down(3000.0);
        assert_eq!(s.d.key_down(&KeyPress::new("Down", SHIFT), 3050.0, &env), Outcome::Pass);
        assert_eq!(s.up(3100.0), Outcome::Pass);
        assert_eq!(s.tap(3400.0), Outcome::Pass);
    }

    // src/platform/keymap.test.ts › double modifier with a key › runs while the second press is held, once per key press
    #[test]
    fn double_modifier_with_a_key_runs_while_the_second_press_is_held() {
        let env = Env::bound(&[("t.cloneAbove", "Alt Alt+Up"), ("t.ctrlCtrl", "Ctrl Ctrl")]);
        let mut d = Dispatcher::new(true);
        d.modifier_down(Modifier::Alt, ALT, false, 1000.0);
        assert_eq!(d.modifier_up(Modifier::Alt, Mods::NONE, 1100.0, &env), Outcome::Pass);
        d.modifier_down(Modifier::Alt, ALT, false, 1200.0);
        assert_eq!(d.key_down(&KeyPress::new("Up", ALT), 1500.0, &env), run("t.cloneAbove"));
        assert_eq!(d.key_down(&KeyPress::new("Up", ALT), 1700.0, &env), run("t.cloneAbove"));
        assert_eq!(d.modifier_up(Modifier::Alt, Mods::NONE, 1800.0, &env), Outcome::Pass);
        // A single press is plain Alt+Up again.
        d.modifier_down(Modifier::Alt, ALT, false, 3000.0);
        assert_eq!(d.key_down(&KeyPress::new("Up", ALT), 3100.0, &env), Outcome::Pass);
    }

    // src/platform/keymap.test.ts › double modifier with a key › fires a bare double tap of another modifier on its second release
    #[test]
    fn bare_double_tap_of_another_modifier_fires_on_its_second_release() {
        let env = Env::bound(&[("t.cloneAbove", "Alt Alt+Up"), ("t.ctrlCtrl", "Ctrl Ctrl")]);
        let mut d = Dispatcher::new(true);
        d.modifier_down(Modifier::Ctrl, CTRL, false, 1000.0);
        assert_eq!(d.modifier_up(Modifier::Ctrl, Mods::NONE, 1050.0, &env), Outcome::Pass);
        d.modifier_down(Modifier::Ctrl, CTRL, false, 1150.0);
        assert_eq!(d.modifier_up(Modifier::Ctrl, Mods::NONE, 1200.0, &env), run("t.ctrlCtrl"));
    }

    // src/platform/keymap.test.ts › AltGr
    #[test]
    fn alt_gr_characters_reach_text_fields_on_windows_and_linux_only() {
        let env = Env::bound(&[("t.altGr", "Ctrl+Alt+0")]);
        let ctrl_alt = Mods { ctrl: true, alt: true, ..Mods::NONE };
        let alt_gr = |ch: &str, target| {
            let mut press = KeyPress::new("0", ctrl_alt).on(target);
            press.alt_gr_char = Some(ch.to_owned());
            press
        };
        let mut d = Dispatcher::new(false);
        assert_eq!(d.key_down(&alt_gr("}", KeyTarget::TextField), 0.0, &env), Outcome::Pass);
        assert_eq!(d.key_down(&alt_gr("}", KeyTarget::Editor), 0.0, &env), Outcome::Pass);
        // No AltGr character on that key (or not typing): it is the shortcut.
        assert_eq!(d.key_down(&alt_gr("0", KeyTarget::TextField), 0.0, &env), run("t.altGr"));
        assert_eq!(d.key_down(&alt_gr("}", KeyTarget::Other), 0.0, &env), run("t.altGr"));
        // Never on macOS, where ⌥ shortcuts type symbols.
        let mut mac = Dispatcher::new(true);
        assert!(!mac.is_alt_gr_typing(&alt_gr("}", KeyTarget::TextField)));
        assert_eq!(mac.key_down(&alt_gr("}", KeyTarget::TextField), 0.0, &env), run("t.altGr"));
    }

    #[test]
    fn mouse_shortcuts() {
        let env = Env::bound(&[("nav.back", "Button4"), ("nav.declaration", "Meta+Button1"), ("t.swipe", "SwipeLeft")]);
        let mut d = Dispatcher::new(true);
        assert_eq!(d.mouse_down(3, Mods::NONE, false, &env), run("nav.back"));
        // An unbound side button is still swallowed.
        assert_eq!(d.mouse_down(4, Mods::NONE, false, &env), Outcome::Swallow);
        assert_eq!(d.mouse_down(0, META, false, &env), run("nav.declaration"));
        // On editor text the editor places the caret first, then asks for the shortcut itself.
        assert_eq!(d.mouse_down(0, META, true, &env), Outcome::Pass);
        assert_eq!(d.mouse_shortcut(0, META, &env), run("nav.declaration"));
        assert_eq!(d.mouse_down(0, Mods::NONE, false, &env), Outcome::Pass);
        assert_eq!(d.mouse_down(2, META, false, &env), Outcome::Pass);
        assert_eq!(d.swipe("SwipeLeft", &env), run("t.swipe"));
        assert_eq!(d.swipe("SwipeRight", &env), Outcome::Pass);
    }
}
