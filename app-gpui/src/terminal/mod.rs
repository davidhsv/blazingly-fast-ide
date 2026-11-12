//! The terminal: the emulated screen (`emulator`), what keys send (`encode`), the view that runs a
//! shell on the backend's PTY (`view`), and the tool window's logic around them: tabs and tab
//! groups, remembered sessions, keys, paste and drop text, links, find, theme and font, and fit
//! scheduling.

pub mod drop;
pub mod embedded;
pub mod emulator;
pub mod encode;
pub mod find_bar;
pub mod fit;
pub mod ime_commit;
pub mod ime_input;
pub mod keys;
pub mod links;
pub mod panel;
pub mod paste;
pub mod persist;
pub mod store;
#[cfg(test)]
pub mod test_support;
pub mod theme;
pub mod view;
pub mod windows_pty;
