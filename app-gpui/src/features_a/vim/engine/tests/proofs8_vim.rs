use super::*;

/// `<LT>`, `<BAR>` and `<BSLASH>` on a mapping's right side stay key notation for `<`, `|` and `\`:
/// `jk` in Insert mode resolves to the keys `<lt>`, `E`, `s`, `c`, `>`, `|`, `\`, none of them Escape.
#[test]
fn inserts_escaped_key_notation_literally() {
    let keymap = keymap("inoremap jk <LT>Esc><BAR><BSLASH>");
    assert_eq!(keymap.resolve(KeyMode::Insert, &["j"]), KeyMatch::Pending);
    let KeyMatch::Keys { rhs, noremap } = keymap.resolve(KeyMode::Insert, &["j", "k"]) else {
        panic!("an Insert-mode key mapping")
    };
    assert!(noremap);
    let keys = split_keys(&rhs);
    assert_eq!(keys, ["<lt>", "E", "s", "c", ">", "|", "\\"]);
    assert!(!keys.contains(&"<Esc>"), "no key of the mapping is Escape");
}
