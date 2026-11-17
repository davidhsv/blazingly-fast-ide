parity.json is reconciled and the generator now runs clean. Rerun in a fresh `git archive HEAD` export with `--listed $S/listed`, it prints no problems and wrote the inventory. In the worktree only `docs/gpui-port/parity.json` changed (+176/−192 lines), nothing is staged, and I did not copy the new INVENTORY.md in.

**Totals (measured)**

|                  | At parity | Model only | Partial | Nothing to port | Rows  |
| ---------------- | --------- | ---------- | ------- | --------------- | ----- |
| Before the merge | 1,463     | 99         | 517     | 15              | 3,145 |
| After            | 1,413     | 74         | 589     | 14              | 3,782 |

**Changes (measured)**

- **Re-keyed: 20.**
  - 15 registrations moved to another line. I read the code at both lines: same id and same component.
  - The `filePresentation` spec and module moved from `src/platform` to `src/ui` (88e4b8f79); only the imports changed.
  - Three settings pages were renamed: `tools.agents.chat` → `agents.chat`, `tools.mcpServer` → `agents.mcpServer`, `languages.ruff` → `python.ruff`. The native app keeps the old ids, so that is now in each row's Missing.
- **Dropped: 4.**
  - `settings:languages.serverTargets` and `unit:…/languageServerTargets.test.tsx`, removed by 2b80c7531.
  - `settings:languages.pluginFormatters`, removed by 0cfe0c82f.
  - `ipc:index_status` (see the first departure below).
- **Lowered: 75**, 50 from at parity and 25 from model only.
  - 73 spec rows now read "K of N cases; added upstream and not covered: …".
  - `settings:agents.chat`: upstream added nine controls the native page lacks.
  - `module:…/layouts.ts`: upstream added Restore Current Layout and Save Changes in Current Layout; native has neither.
- **Citations replaced or removed: 4 rows.** The rename `the_restore_default_layout_key_works_only_with_a_project` → `restore_default_layout_works_only_with_a_project` covers two of them. The other two are `dialog.test.tsx` (27 of 40 cases now) and the PHP page.
- **Checked:** no status went up, no citation was added except that one renamed test, and every cited test exists. The 25 lowered model-only rows are exactly why model only fell from 99 to 74; the merge itself moved no other row's mark.

**Done rows whose webview source changed (measured):** 1,254 of the 1,487 done rows; 233 are unchanged. 300 of those are `ipc:` rows, but in only 37 did the command's own function body change. 110 commands call no shared backend function, so their backend was not looked at.

**New rows (measured):** 692 keys are new, but 46 of them are rows that moved, which leaves 646 new rows.

- Unit specs: 224
- Actions: 138, of which 6 are not real actions (see the generator fault below)
- IPC commands: 117
- End-to-end specs: 43
- Modules: 37
- Context keys: 15
- Settings pages: 12
- Everything else: 60, at 1 to 8 per section.

By folder, the largest are `src-tauri/src/commands` (115), `vcs` (60), `database` (59), `debugger` (51) and `e2e` (43).

**Where the brief was wrong, or where I departed from it**

- **`ipc:index_status`:** the brief says set it to status none. The generator reads none as "nothing to port" and refuses it for a command the webview calls. No native code calls it, so partial would have had nothing to cite. I removed the key and the row is unmarked.
- **`pluginFormatters`:** there was no page to re-key it to. It became part of the Code Style page, which has no parity row, so re-keying would have raised that row. I dropped it instead.
- **`action:layout.restoreDefault`:** I kept it done, against the brief's example. Upstream moved Shift-F12 to `layout.restoreCurrent` (460027fd4), so at v0.1.46 the action has no key in any preset, in the webview's keymaps and in the native ones. Nothing is missing. I removed the key claim from the proof; you can overrule this.
- **Proofs I shortened** because upstream removed what they described:
  - PHP page: removed "a stored run target that is gone shows as unavailable", together with its removed test.
  - Node page (not flagged by the generator): Missing went from "(Run in target, Language server runs on)" to "(Run in target)".

**Generator faults found (measured)**

- 97 done unit rows have specs whose case names changed upstream but pass the count check. Their proofs cite whole test modules or carry a ";". They are listed in RETRIAGE.md, not lowered.
- Model only is computed only for done rows, so the 25 lowered model-only rows now show as partial. Their proofs say they were model only.
- Proof cells are written without escaping "|".
- The generator's `tsmodel.mjs` counts 6 commit checks in `src/features/vcs/commitChecks.tsx` as actions, so the Actions total is 6 too high.

**Not measured**

- Hypothesis: "K of N" assumes the cases that kept their names are still covered as before; I did not re-read their bodies. Check with `git diff 3662ca0ea HEAD -- <spec>`.
- Hypothesis: `settings:tools.agents` is now `settings:agents.list`, and would make the new-row count one too high. Check with `git show 3662ca0ea:src/features/agents/index.ts` lines 25-40 against HEAD lines 55-90.
- Hypothesis: 6 of the 46 paired moves are right but unverified line by line. Each is the only old and the only new key of that extension point in its file, with the surrounding lines edited.

Everything is in `$S/drec/`:

- RECONCILE.md
- RETRIAGE.md
- NEW_ROWS.md
- reconcile_1_keys.py
- reconcile_2_specs.py
- reconcile_check.py
- parity_orig.json (the file before my changes)
