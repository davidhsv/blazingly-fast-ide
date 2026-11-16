I've written the design note, about 455 lines, at `$S/drafts/vim_engine_design.md`. Nothing was built or run, and nothing in the worktree changed. The only files I created are the note and some small edit scripts in `$S/dvimd/`.

**Checklist size by group.** 184 items in 19 groups, counted over section 2. Many items bundle several keys.

| Group                   | Items | Group                    | Items |
| ----------------------- | ----- | ------------------------ | ----- |
| Modes                   | 13    | Insert-mode keys         | 11    |
| Counts and repeat       | 10    | Replace mode             | 4     |
| Motions                 | 26    | Visual char, line, block | 13    |
| Operators               | 13    | Ex commands              | 16    |
| Text objects            | 7     | Options                  | 5     |
| Registers               | 12    | Command line             | 8     |
| Marks                   | 5     | Scrolling                | 5     |
| Macros                  | 4     | Multiple cursors         | 4     |
| Jump list               | 3     | IME and key notation     | 10    |
| Search and substitution | 15    |                          |       |

- **Coverage:** the IDE's own tests touch 28 items and 15 depend directly on the rc feature. The engine ships its own test suite of 697 cases, which covers 136 of the items; the note proposes porting those cases as the parity oracle. `e2e/` has no Vim test at all.

**Slices: 19.** 14 engine slices (S1–S14), each tested against a fake editor, and 5 integration slices: I1 editor hook-up, I2a mode reporting, I2b command line and search highlight, I3 app wiring, I4 IME and clipboard. Three checks gate them: G1 and G3 before I1, G2 before S11. By the inventory's rule, the engine slices flip no rows on their own. I2a, I2b, I3 and I4 complete INVENTORY lines 1439, 1053, 883, 881, 1054, 1122, 1211, the remaining gap of 1550, 3336, and finally 1639. Hypothesis: about 16,000 lines for the engine and 4,000 for the integration.

**Three largest risks.**

1. **Regex dialect.** Search patterns are JavaScript regular expressions by default (`pcre` is on), and the Vim-syntax mode emits lookbehind. The `regex` crate the app already uses has no lookaround or backreferences. `fancy-regex` is in the lock file only through `jsonschema`, so using it is a new direct dependency, which DECISIONS D139 reserves for your yes (gate G2).
2. **Key delivery.** Printable keys reach the native editor as text input, not only as key-down (`view.rs:1818`, `2083`).
   - Hypothesis: stopping propagation in key-down also suppresses that text; it is gpui-kit behaviour whose source lives under the home folder, so I couldn't read it. One window test settles it (G1).
   - Measured: GPUI's keystroke has no physical key code, so the engine's fallback for non-Latin layouts can't be ported.
3. **Undo grouping.** The webview's undo steps come from CodeMirror's history-joining rules and depend on timing. Hypothesis: quick adjacent commands like `xx` may undo together. The native history seals on every selection change and never joins these edits, so it needs a group API (gate G3 measures the webview rule first).

**Problems the reading found in the existing native code.**

- **rc mappings diverge.** `RcKeymap::resolve` treats any rc prefix as pending. The engine instead fires a full default match first, so with `nmap dx …` a lone `d` is the delete operator and the mapping never fires. The engine must own key matching.
- **`handle_ex` sits at the wrong layer.** It runs ahead of the engine's range parser, so `:'<,'>w` or `:2,5w` never reach the IDE's commands.
- **Shortcut conflicts aren't wired.** `vim_claims` exists but nothing calls it; the window's dispatcher never overrides `DispatchEnv::claimed`.
- **Webview quirks a port must copy or record:**
  - `set tw=` in the rc has no effect.
  - `P` leaves the caret one character past the pasted text; the engine's own suite asserts this.
  - `<C-o>` replays positions recorded in another document.

**Where the brief was wrong.**

- **No engine source in that package.** `@replit/codemirror-vim` 6.4.0 has no `src/` and is only a 1,692-line CodeMirror 6 adapter. The engine is a separate package, `@replit/codemirror-vim-core` 0.1.0 (7,164 lines), which ships its own suite.
- **Not a symlink into the main checkout.** `node_modules` in the worktree is a real directory, and the package links into the worktree's own `.pnpm` store.
- **APIs the IDE never uses:**
  - `Vim.mapCommand`, `Vim.defineAction`, the `vim-keypress` and `vim-command-done` events, and the status-bar option.
  - The IDE uses `Vim._mapCommand`, `unmap`, `mapclear`, `handleKey`, `handleEx`, `setOption`, plus `vimKeyFromEvent` in tests.
  - It listens only to `vim-mode-change`.
- **No clipboard setting exists.** Only the `"+` register touches the system clipboard.
- **No fold commands, five options.** The engine has no fold keys, and only five options: `filetype`, `textwidth`, `pcre`, `langmap`, `insertModeEscKeysTimeout`.

**Not read line by line:** `VimSettingsPage.tsx`, the bodies of `rc.test.ts` and `conflicts.test.ts` (test names only), the test half of `ime.rs`, and the bodies of the engine's sentence and `findSymbol` helpers. Every statement in the note that I didn't check against code is marked "Hypothesis:", 15 in all.
