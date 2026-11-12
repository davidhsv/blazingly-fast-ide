//! What an input-method composition commits to the shell.
//!
//! Switching the input source in the middle of a composition (CapsLock with macOS Pinyin) commits
//! the letters typed so far, and the input method reports them with the spaces it showed between
//! syllables (`awd` arrives as `aw d`). The user never typed those spaces: Space picks a candidate
//! while composing. [`Composition`] follows one composition and gives back the text that was
//! typed.

use std::borrow::Cow;

/// The text a composition commits, without the syllable spaces an input method displays when it
/// commits its raw letters: then the commit is the composition text itself and plain ASCII. A
/// chosen candidate is committed as it is.
pub fn typed_commit<'a>(commit: &'a str, composition: &str) -> Cow<'a, str> {
    let printable_ascii = !commit.is_empty() && commit.bytes().all(|b| (0x20..=0x7e).contains(&b));
    if commit == composition && printable_ascii && commit.contains(' ') {
        Cow::Owned(commit.replace(' ', ""))
    } else {
        Cow::Borrowed(commit)
    }
}

/// One input-method composition: the text it displays while composing, and what it commits.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Composition {
    active: bool,
    /// The composition text the input method displays, from its last update.
    text: String,
}

impl Composition {
    pub fn new() -> Self {
        Self::default()
    }

    /// A composition is in progress: keys and text belong to the input method until it ends.
    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn start(&mut self) {
        self.active = true;
        self.text.clear();
    }

    /// The input method displays `text` now.
    pub fn update(&mut self, text: &str) {
        text.clone_into(&mut self.text);
    }

    /// The composition ended with `commit`. Returns the text to send.
    pub fn end<'a>(&mut self, commit: &'a str) -> Cow<'a, str> {
        self.active = false;
        typed_commit(commit, &self.text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What a composition that displayed `shown` sends when it commits `commit`.
    fn committed(shown: &str, commit: &str) -> String {
        let mut composition = Composition::new();
        composition.start();
        assert!(composition.is_active());
        composition.update(shown);
        let sent = composition.end(commit).into_owned();
        assert!(!composition.is_active());
        sent
    }

    // src/features/terminal/imeInput.test.ts › ownTerminalInput › commits the letters typed for %j without the IME's
    // syllable spaces
    #[test]
    fn commits_the_letters_typed_without_the_syllable_spaces() {
        for (shown, typed) in [("aw d", "awd"), ("claude co de", "claudecode"), ("xi'an", "xi'an")] {
            assert_eq!(committed(shown, shown), typed);
        }
    }

    // src/features/terminal/imeInput.test.ts › ownTerminalInput › keeps a candidate chosen for %j as committed
    #[test]
    fn keeps_a_chosen_candidate_as_committed() {
        for (shown, chosen) in [("ni hao", "你好"), ("helloworld", "hello world")] {
            assert_eq!(committed(shown, chosen), chosen);
        }
    }
}
