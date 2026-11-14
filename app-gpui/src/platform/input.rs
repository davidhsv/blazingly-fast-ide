//! Feeds a window's key, modifier and mouse events to the shortcut [`Dispatcher`] and runs the
//! action it picks.
//!
//! Keystrokes are intercepted before GPUI's own key dispatch, so IDE shortcuts win over a focused
//! component's defaults, as the webview's capture-phase listener does. Modifier and mouse events
//! have no application-level hook: the window's root view forwards them
//! ([`modifiers_changed`], [`mouse_down`]).

use super::actions::{ActionRegistry, ActionSource, execute_action_from, is_action_key_enabled};
use super::context::{ContextKeys, window_context};
use super::dispatcher::{DispatchEnv, Dispatcher, KeyPress, KeyTarget, Mods, Outcome};
use super::keymap::{KeymapState, Modifier, is_mac_host};
use gpui_kit::{
    App, FocusHandle, Global, InputPreference, KeyContext, Keystroke, KeystrokeEvent, Modifiers, MouseButton,
    NavigationDirection, Window, WindowId,
};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Instant;

/// Key contexts of text inputs: plain typing stays in them.
const TEXT_FIELD_CONTEXTS: [&str; 2] = ["Input", "TextField"];
/// Key context of code editors: typing goes through the keymap first.
pub const EDITOR_CONTEXT: &str = "CodeEditor";

/// The effective keymap, shared by every window.
#[derive(Default)]
pub struct Keymap(pub KeymapState);

impl Global for Keymap {}

struct WindowInput {
    dispatcher: Dispatcher,
    modifiers: Mods,
}

/// Per-window dispatch state.
pub struct Input {
    windows: HashMap<WindowId, WindowInput>,
    started: Instant,
    /// A key the keymap let pass is on its way to whatever else handles keys.
    key_in_flight: bool,
}

impl Global for Input {}

impl Default for Input {
    fn default() -> Self {
        Self { windows: HashMap::new(), started: Instant::now(), key_in_flight: false }
    }
}

impl Input {
    fn now(&self) -> f64 {
        self.started.elapsed().as_secs_f64() * 1000.0
    }
}

type Claim = Rc<dyn Fn(&Keystroke, &mut Window, &mut App) -> bool>;

/// Views that take their keys before the keymap while the focus is in them: a terminal's
/// Ctrl+letter goes to the shell, not to an IDE shortcut.
#[derive(Default)]
pub struct KeyClaims {
    next: u64,
    claims: Vec<(u64, FocusHandle, Claim)>,
    /// Focuses that keep the keymap out: a key pressed in them goes to the focused view alone.
    shields: Vec<(u64, FocusHandle)>,
}

impl Global for KeyClaims {}

/// Has `claim` see every key pressed while `focus` is focused, before the keymap does; a key it
/// returns true for goes no further. Returns the id for [`release_keys`].
pub fn claim_keys(
    focus: FocusHandle,
    claim: impl Fn(&Keystroke, &mut Window, &mut App) -> bool + 'static,
    cx: &mut App,
) -> u64 {
    let claims = cx.default_global::<KeyClaims>();
    claims.next += 1;
    let id = claims.next;
    claims.claims.push((id, focus, Rc::new(claim)));
    id
}

/// Keeps the keymap out while `focus` is focused: every key goes to the focused view and no IDE
/// shortcut runs, as in a find field that owns its keys. Returns the id for [`release_keys`].
pub fn keep_keymap_out(focus: FocusHandle, cx: &mut App) -> u64 {
    let claims = cx.default_global::<KeyClaims>();
    claims.next += 1;
    let id = claims.next;
    claims.shields.push((id, focus));
    id
}

pub fn release_keys(id: u64, cx: &mut App) {
    if cx.has_global::<KeyClaims>() {
        let claims = cx.global_mut::<KeyClaims>();
        claims.claims.retain(|(claim, _, _)| *claim != id);
        claims.shields.retain(|(shield, _)| *shield != id);
    }
}

/// Installs the keymap, the dispatch state and the keystroke interceptor.
pub fn init(cx: &mut App) {
    cx.default_global::<Keymap>();
    cx.default_global::<Input>();
    cx.default_global::<ActionRegistry>();
    cx.default_global::<KeyClaims>();
    cx.intercept_keystrokes(|event, window, cx| {
        let claims: Vec<Claim> = cx
            .global::<KeyClaims>()
            .claims
            .iter()
            .filter(|(_, focus, _)| focus.is_focused(window))
            .map(|(_, _, claim)| claim.clone())
            .collect();
        let shielded = cx.global::<KeyClaims>().shields.iter().any(|(_, focus)| focus.is_focused(window));
        if claims.iter().any(|claim| claim(&event.keystroke, window, cx))
            || (!shielded && keystroke(event, window, cx).consumed())
        {
            cx.stop_propagation();
        } else {
            // Until this key has been dispatched: what it reaches next knows the keymap declined it.
            cx.global_mut::<Input>().key_in_flight = true;
            cx.defer(|cx| cx.global_mut::<Input>().key_in_flight = false);
        }
    })
    .detach();
}

/// Whether a key the keymap declined is being dispatched right now: a shortcut that fires then
/// was not the keymap's to run.
pub fn key_in_flight(cx: &App) -> bool {
    cx.try_global::<Input>().is_some_and(|input| input.key_in_flight)
}

pub fn mods(modifiers: &Modifiers) -> Mods {
    Mods { ctrl: modifiers.control, alt: modifiers.alt, shift: modifiers.shift, meta: modifiers.platform }
}

/// GPUI's key name as the keymap spells it: `j` → `J`, `up` → `Up`, `f8` → `F8`.
pub fn key_name(key: &str) -> String {
    let named = match key {
        "enter" => "Enter",
        "escape" => "Escape",
        "tab" => "Tab",
        "backspace" => "Backspace",
        "delete" => "Delete",
        "insert" => "Insert",
        "home" => "Home",
        "end" => "End",
        "pageup" => "PageUp",
        "pagedown" => "PageDown",
        "up" => "Up",
        "down" => "Down",
        "left" => "Left",
        "right" => "Right",
        "space" => "Space",
        _ => "",
    };
    if !named.is_empty() {
        return named.to_owned();
    }
    let function = key.strip_prefix('f').filter(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
    match function {
        Some(n) => format!("F{n}"),
        None if key.chars().count() == 1 => key.to_uppercase(),
        None => key.to_owned(),
    }
}

fn is_modifier_key(key: &str) -> bool {
    matches!(key, "" | "shift" | "control" | "alt" | "platform" | "cmd" | "function" | "fn" | "capslock")
}

fn key_target(contexts: &[KeyContext]) -> KeyTarget {
    if contexts.iter().any(|c| c.contains(EDITOR_CONTEXT)) {
        KeyTarget::Editor
    } else if contexts.iter().any(|c| TEXT_FIELD_CONTEXTS.iter().any(|name| c.contains(name))) {
        KeyTarget::TextField
    } else {
        KeyTarget::Other
    }
}

/// The keymap and the registry, as the dispatcher sees them for one window.
struct Env<'a> {
    keymap: &'a KeymapState,
    registry: &'a ActionRegistry,
    ctx: &'a ContextKeys,
}

impl DispatchEnv for Env<'_> {
    fn bindings(&self) -> &[(String, String)] {
        self.keymap.effective_bindings()
    }

    fn key_enabled(&self, action_id: &str) -> bool {
        self.registry.get(action_id).is_some_and(|action| is_action_key_enabled(&action.meta, self.ctx))
    }
}

/// Runs `decide` with the window's dispatcher, then the action it picked.
fn dispatch(window: &mut Window, cx: &mut App, decide: impl FnOnce(&mut WindowInput, f64, &Env) -> Outcome) -> Outcome {
    let id = window.window_handle().window_id();
    let ctx = window_context(window, cx);
    let input = cx.default_global::<Input>();
    let now = input.now();
    let mut state = input
        .windows
        .remove(&id)
        .unwrap_or_else(|| WindowInput { dispatcher: Dispatcher::new(is_mac_host()), modifiers: Mods::NONE });
    let outcome = {
        let env = Env { keymap: &cx.global::<Keymap>().0, registry: cx.global::<ActionRegistry>(), ctx: &ctx };
        decide(&mut state, now, &env)
    };
    cx.global_mut::<Input>().windows.insert(id, state);
    if let Outcome::Run(action) = &outcome {
        execute_action_from(ActionSource::Keyboard, action, None, window, cx);
    }
    outcome
}

/// Answers `decide` with the window's dispatcher and its keymap, actions and context keys; nothing
/// runs. A view that handles a gesture itself asks this what the keymap binds to it.
pub fn with_dispatcher<T>(
    window: &mut Window,
    cx: &mut App,
    decide: impl FnOnce(&Dispatcher, &dyn DispatchEnv) -> T,
) -> T {
    let id = window.window_handle().window_id();
    let ctx = window_context(window, cx);
    cx.default_global::<Keymap>();
    cx.default_global::<ActionRegistry>();
    let state = cx
        .default_global::<Input>()
        .windows
        .remove(&id)
        .unwrap_or_else(|| WindowInput { dispatcher: Dispatcher::new(is_mac_host()), modifiers: Mods::NONE });
    let answer = {
        let env = Env { keymap: &cx.global::<Keymap>().0, registry: cx.global::<ActionRegistry>(), ctx: &ctx };
        decide(&state.dispatcher, &env)
    };
    cx.global_mut::<Input>().windows.insert(id, state);
    answer
}

fn keystroke(event: &KeystrokeEvent, window: &mut Window, cx: &mut App) -> Outcome {
    let Keystroke { key, modifiers, key_char } = &event.keystroke;
    if is_modifier_key(key) {
        return Outcome::Pass;
    }
    let press = KeyPress {
        key: key_name(key),
        mods: mods(modifiers),
        target: key_target(&event.context_stack),
        ime: event.keystroke.is_ime_in_progress(),
        alt_gr_char: (event.input_preference == InputPreference::CharacterInput).then(|| key_char.clone()).flatten(),
    };
    dispatch(window, cx, |state, now, env| state.dispatcher.key_down(&press, now, env))
}

/// The window's modifier keys changed: the root view forwards its `ModifiersChangedEvent`s here.
pub fn modifiers_changed(modifiers: &Modifiers, window: &mut Window, cx: &mut App) -> Outcome {
    let new = mods(modifiers);
    dispatch(window, cx, |state, now, env| {
        let old = std::mem::replace(&mut state.modifiers, new);
        let mut outcome = Outcome::Pass;
        for modifier in Modifier::ALL {
            match (old.held(modifier), new.held(modifier)) {
                (false, true) => state.dispatcher.modifier_down(modifier, new, false, now),
                (true, false) => {
                    let released = state.dispatcher.modifier_up(modifier, new, now, env);
                    if released.consumed() {
                        outcome = released;
                    }
                }
                _ => {}
            }
        }
        outcome
    })
}

/// 0-based button number, as DOM mouse events count them (3 = back, 4 = forward).
pub fn button_index(button: MouseButton) -> u32 {
    match button {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
        MouseButton::Navigate(NavigationDirection::Back) => 3,
        MouseButton::Navigate(NavigationDirection::Forward) => 4,
    }
}

/// A mouse button went down: the root view forwards its capture-phase events here.
/// `in_editor_text` is a left click on editor text (the editor places the caret first).
pub fn mouse_down(
    button: MouseButton,
    modifiers: &Modifiers,
    in_editor_text: bool,
    window: &mut Window,
    cx: &mut App,
) -> Outcome {
    let (button, held) = (button_index(button), mods(modifiers));
    dispatch(window, cx, |state, _, env| state.dispatcher.mouse_down(button, held, in_editor_text, env))
}

/// The window lost focus or closed: gestures in progress are over.
pub fn window_blurred(window: WindowId, cx: &mut App) {
    if let Some(state) = cx.default_global::<Input>().windows.get_mut(&window) {
        state.dispatcher.blur();
        state.modifiers = Mods::NONE;
    }
}

/// Pauses shortcut dispatch in `window` (e.g. while recording a shortcut); calls nest.
pub fn suspend(window: WindowId, cx: &mut App) {
    let state = cx
        .default_global::<Input>()
        .windows
        .entry(window)
        .or_insert_with(|| WindowInput { dispatcher: Dispatcher::new(is_mac_host()), modifiers: Mods::NONE });
    state.dispatcher.suspend();
}

pub fn resume(window: WindowId, cx: &mut App) {
    if let Some(state) = cx.default_global::<Input>().windows.get_mut(&window) {
        state.dispatcher.resume();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::actions::{Action, ActionMeta};
    use crate::platform::context::set_context;
    use crate::platform::keymap::Keymap as Bindings;
    use gpui_kit::{Context, IntoElement, Render, TestAppContext, VisualTestContext, div};
    use std::cell::RefCell;
    use std::rc::Rc;
    mod proofs7_suspend;

    struct Empty;

    impl Render for Empty {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }

    /// A window with `bindings` as the user's keymap and one counting action per id.
    fn setup<'a>(
        cx: &'a mut TestAppContext,
        bindings: &[(&str, &str, Option<&str>)],
    ) -> (&'a mut VisualTestContext, Rc<RefCell<Vec<String>>>) {
        let ran = Rc::new(RefCell::new(Vec::new()));
        cx.update(|cx| {
            init(cx);
            let mut overrides = Bindings::new();
            for (id, binding, when) in bindings {
                overrides.insert((*id).to_owned(), vec![(*binding).to_owned()]);
                let mut meta = ActionMeta::new(id, id);
                meta.when = when.map(str::to_owned);
                let log = ran.clone();
                let name = (*id).to_owned();
                cx.global_mut::<ActionRegistry>().register(Action::new(meta, move |_, _, _| {
                    log.borrow_mut().push(name.clone());
                    Ok(())
                }));
            }
            cx.global_mut::<Keymap>().0.set_overrides(overrides);
        });
        let (_, cx) = cx.add_window_view(|_, _| Empty);
        (cx, ran)
    }

    // src/platform/keymap.test.ts › dispatcher › runs bound actions and chords
    #[gpui_kit::test]
    fn keystrokes_run_bound_actions_and_chords(cx: &mut TestAppContext) {
        let (cx, ran) = setup(cx, &[("t.single", "Meta+J", None), ("t.chord", "Meta+K Meta+C", None)]);
        cx.simulate_keystrokes("cmd-j");
        cx.simulate_keystrokes("cmd-k cmd-c");
        assert_eq!(*ran.borrow(), ["t.single", "t.chord"]);
    }

    // src/platform/keymap.test.ts › dispatcher › respects when clauses
    #[gpui_kit::test]
    fn keystrokes_respect_when_clauses_per_window(cx: &mut TestAppContext) {
        let (cx, ran) = setup(cx, &[("t.gated", "Meta+G", Some("panelOpen"))]);
        cx.simulate_keystrokes("cmd-g");
        assert!(ran.borrow().is_empty());
        cx.update(|window, cx| set_context(window, cx, "panelOpen", true));
        cx.simulate_keystrokes("cmd-g");
        assert_eq!(*ran.borrow(), ["t.gated"]);
    }

    // src/platform/keymap.test.ts › double Shift › fires on the second release
    #[gpui_kit::test]
    fn double_shift_runs_its_action(cx: &mut TestAppContext) {
        let (cx, ran) = setup(cx, &[("t.doubleShift", "Shift Shift", None)]);
        let shift = Modifiers { shift: true, ..Modifiers::default() };
        for _ in 0..2 {
            cx.update(|window, cx| modifiers_changed(&shift, window, cx));
            cx.update(|window, cx| modifiers_changed(&Modifiers::default(), window, cx));
        }
        assert_eq!(*ran.borrow(), ["t.doubleShift"]);
    }

    #[gpui_kit::test]
    fn side_buttons_run_mouse_bindings(cx: &mut TestAppContext) {
        let (cx, ran) = setup(cx, &[("nav.back", "Button4", None)]);
        let outcome = cx.update(|window, cx| {
            mouse_down(MouseButton::Navigate(NavigationDirection::Back), &Modifiers::default(), false, window, cx)
        });
        assert_eq!(outcome, Outcome::Run("nav.back".to_owned()));
        assert_eq!(*ran.borrow(), ["nav.back"]);
    }

    #[test]
    fn names_keys_as_the_keymap_spells_them() {
        for (gpui, name) in [
            ("j", "J"),
            ("up", "Up"),
            ("f8", "F8"),
            ("f12", "F12"),
            ("enter", "Enter"),
            ("pagedown", "PageDown"),
            ("[", "["),
            ("space", "Space"),
            ("f", "F"),
        ] {
            assert_eq!(key_name(gpui), name);
        }
    }
}
