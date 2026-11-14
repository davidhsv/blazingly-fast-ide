# Integrator brief: fonts, then the visual defects seen in the real window

Read first: `$S/int_rules.md` (hard rules and report format), `$S/view_rules.md`, then `docs/gpui-port/notes/inspections/2026-11-14-b/README.md` and `docs/gpui-port/notes/inspections/2026-11-14-c/README.md` (what was seen in the real window, with screenshots beside them: look at the screenshots). Your own scratch subfolder: `$S/d100/`.

The rule for every item below: read the webview source first (`src/…`, the CSS module and `src/styles/tokens.css`), then make the native app draw the same, with a test. If the webview does the same as the native app today, change nothing and say so with the `file:line` that shows it. Do them in this order; stop adding items when you have used about two hours, and report what is left.

## Part A: fonts (first)

Measured facts:

- The bundled variable `.woff2` faces in `app-gpui/assets/fonts/` cannot be used: the text system draws a variable face at its default weight only (`openidea-gpui --self-check=fonts` fails: semibold resolves to the regular face).
- Static faces are on this machine, under the SIL Open Font License 1.1 (read from each file's name table): `/Applications/IntelliJ IDEA.app/Contents/jbr/Contents/Home/lib/fonts/` has `Inter-Regular.otf`, `Inter-SemiBold.otf`, `Inter-Italic.otf`, `Inter-SemiBoldItalic.otf` (Inter 3.019, "Copyright © 2020 The Inter Project Authors") and `JetBrainsMono-{Thin,ExtraLight,Light,Regular,Medium,SemiBold,Bold,ExtraBold}[Italic].ttf` (JetBrains Mono 2.304, "Copyright 2020 The JetBrains Mono Project Authors (https://github.com/JetBrains/JetBrainsMono)"). There is no Inter Medium or Inter Bold there. Nothing may be downloaded.
- The webview draws 400, 500, 600 and 700 (`grep -rho "font-weight: *[^;]*" src --include='*.css' | sort | uniq -c`), and the code weight comes from a setting (`--fw-code`; Settings → Editor → Font has "Main font weight" and "Bold font weight").

Do:

1. Copy (plain `cp`, read only on the source) the static faces the app can draw into `app-gpui/assets/fonts/{inter,jetbrains-mono}/`: Inter's four, and of JetBrains Mono exactly the upright weights the two font-weight settings can select plus Italic and Bold Italic. Remove the two `.woff2` files. Keep the OFL licence text beside each family and make its copyright line the one of the copied files' name tables.
2. `theme/bundled_fonts.rs` embeds those and registers them at start (before the first window), and the family names the theme asks for resolve to them (the app's log said `failed to load font "Inter Variable"`: the faces register as "Inter" and "JetBrains Mono"; follow the order of `--font-ui` and `--font-mono` in `src/styles/tokens.css:247-250`). A user's font setting still wins.
3. Extend `--self-check=fonts`: for both families print what 400, 500, 600, 700 and italic resolve to (family, and whether the face differs from the regular one), exit 0 only when 400 and 600 (and 700 for JetBrains Mono) have faces of their own; print, without failing, what Inter 500 and Inter 700 fall back to. Run it and put its output in the report.
4. A Rust test that the registration succeeds and the theme's UI and code families resolve to the bundled ones on the test platform if that is possible there; if the test platform cannot show it, the self-check is the proof and say so.

## Part B: visual defects (each: compare, then fix with a test, or prove equal)

1. **Shadows.** Dialogs, popups, menus, tooltips and notification balloons have no drop shadow; the webview's are `--shadow-dialog`, `--shadow-popup`, `--shadow-tooltip`, `--shadow-stripe-tooltip`, `--shadow-balloon` (`src/styles/tokens.css:294-327` dark, `:657-681` light). One shared helper that turns those token values into GPUI box shadows for the active theme, used by every overlay surface (`grep -rln "shadow" app-gpui/src` shows the few that have one). Also what is behind a dialog: `src/ui/Dialog.module.css` `.backdrop`.
2. **Commit tool window**: the second button reads "Commit and Pu" (clipped). It must read "Commit and Push…" at the default width.
3. **File status colours**: the Project view does not colour a modified or unversioned file and the editor tab does not either; the Commit tree does. Follow the webview (project tree row and tab label colours by VCS status).
4. **Settings → Editor → Font**: the "Show only monospaced fonts" label is cut by the dialog's right edge. **Settings → Plugins**: the details pane's empty state is cut by the dialog's right edge. Fix the layout (the content must stay inside the dialog at its default size) and check the other pages that use the same row or split helpers.
5. **Find Action** opens with an empty list. Compare with the webview's behaviour for an empty query.
6. **Diff viewer**: the two sides are drawn without syntax colours. Compare with the webview's diff viewer.
7. **Database toolbar**: the "Filter objects" field touches the pane's right edge. Compare.
8. **Trust dialog path**: draws "..." where a CSS ellipsis is "…". Compare (`src/features/project-trust/trust.module.css:17-27`).
9. **Terminal font**: a prompt glyph the font lacks is drawn as a boxed question mark. Compare with the font family list the webview's terminal uses and fall back the same way.
10. **`toolwindow.focusEditor`** answers "Action unavailable" with the Project view focused and an editor open (seen in the real window). Compare with the action's condition in the webview and the manifest.
11. **Empty editor in a single-file window**: the webview shows other hints there (`src/workbench/EditorArea.tsx:19-23`). Follow it, if such a window can have no tab.
12. **App menu title**: the process's menu bar reads `Apple, openidea-gpui, File, …`. Compare with what the Tauri app's menu calls its first menu (`src-tauri/src/` menu code); set it if GPUI lets the app name it.

## Part C: tooling and test hygiene

1. `app-gpui/src/inspect.rs`: add steps `click⇥<x>⇥<y>`, `rclick⇥<x>⇥<y>`, `hover⇥<x>⇥<y>` (window coordinates in points from the top-left of the content) that dispatch the mouse events through the window's own input dispatch (a move, then down and up), so context menus and hover states can be photographed. A test like the existing `workbench::tests::inspect_script` one: an `rclick` on a Project view row opens its context menu. Update `$S/insp/USAGE.md`.
2. `workbench::plugin_java_debug::tests::stop_ends_the_process_and_the_session` uses the fixed port 50123 and failed three times under load: make it use a free port chosen at run time.
3. The two helpers that still wait four seconds (`workbench/editor_extras.rs:635`, `workbench/editor_language.rs:1649`): 30 s like the others.
4. `app-gpui/Cargo.toml` has `[lints.rust] linker_messages = "allow"`. Leave it, but run one `cargo build` with it removed and put the linker's message in the report, then restore the line (by editing it back, not from git).
