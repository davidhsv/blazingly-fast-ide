The notebook editor is integrated and every gate passes. Two drafted claims turned out false (restart recovering a dead kernel, and a test's save assertion), and the 50 MB safety check found the window blocked for 1.32 s per redraw; all three are handled below. I committed nothing. During the session HEAD moved from `48df2008` to `caff1546` through other agents' commits (read-only `git log`).

S = `$S`. Test prefixes: U = `openidea_gpui::notebook::tests::proofs24_notebook_units::`, V = `openidea_gpui::notebook::tests::proofs24_notebook_view::`, E = `openidea_gpui::workbench::tests::proofs24_e2e_notebook::`.

## 1. Gates (measured)

| Gate                                                                   | Log                                                              | Result                                                                                                                                |
| ---------------------------------------------------------------------- | ---------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------- |
| Whole suite, run 1 (nothing else running)                              | `S/dnbi/suite1.log`                                              | `test result: ok. 7817 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 200.65s`. Load 3.92 before, 30.00 after.  |
| Whole suite, run 2                                                     | `S/dnbi/suite2.log`                                              | `test result: ok. 7817 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 202.14s`. Load 23.65 before, 12.79 after. |
| `cargo test --lib notebook::` × 20 (run straight from the test binary) | `S/dnbi/loop/summary.txt`, `S/dnbi/loop/run1.log` to `run20.log` | 20 of 20 runs: `ok. 76 passed; 0 failed`. No test failed even once.                                                                   |
| `cargo test --test web_view`                                           | `S/dnbi/web_view.log`                                            | `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.51s`                                       |
| `cargo clippy --all-targets -- -D warnings`                            | `S/dnbi/clippy2.log`                                             | Finished, no warnings                                                                                                                 |
| `cargo build`                                                          | `S/dnbi/build_final.log`                                         | Finished, 0 warnings                                                                                                                  |

The `notebook::` filter covers 76 tests: everything this slice added, declared or changed, plus the existing `features_a::notebook` tests.

Only runs 1 to 6 of the loop overlapped the second suite (6 summary lines when it exited); runs 7 to 20 ran after it.

Failures seen along the way, all fixed:

- **Two cache tests** (`U::keeps_the_rendered_preview_across_snapshots_with_equal_attachments`, `U::does_not_rebuild_when_only_an_unreferenced_attachment_changes`): `MarkdownCache` read the attachment names back from blocks whose images were already turned into `data:` URLs, so it rebuilt every time. It now stores the names.
- **Reference definitions** (`U::resolves_an_entity_encoded_attachment_…`): the renderer does not resolve `![a][r]`. Fixed with a pre-pass in `markdown_cell.rs`.
- **Save test** (`E::a_notebook_file_opens…`): the backend keeps the file's original line splits (`"source": ["p","rint(2)"]`), so the substring check could never match. The test now parses the JSON.
- **Restart test** (draft `E::a_kernel_that_cannot_start_shows_why_and_restart_recovers`): Restart after the kernel died starts the interpreter again. The backend then waits out its connection timeout. Measured error: `Jupyter protocol: Connect timed out after 30s; …`. I removed the restart claim (see section 4).

## 2. What changed, per stage

**Stage 1, the module (`lib.rs` mod line).**

- Fixed a moved-value error in `output_items.rs`.
- Fixed the preview cache in `markdown_cell.rs`.
- Added the reference pre-pass (`inline_references`) with its test `notebook::markdown_cell::tests::references_are_written_inline_and_code_fences_are_left_alone`.

**Stage 2, the view.** Measured with a 50 MB output put into the model by hand: every redraw took 1.32 s on the window's thread (`S/dnbi/measure1.log`). The cause was cells deep-cloned and outputs re-parsed on every frame.

- The view now takes its cells from the model only when the model changes (`ShownCell`).
- Output items and image decoding run on the background executor, and a result that arrives after a newer one started is dropped.
- Markdown preview images are decoded the same way, under short keys.
- A broken image draws its text (`with_fallback`).
- `NotebookModel::acknowledged_cells` reads the cells in place.
- `TextArea::set_code_font` exists because cell sources were drawn in the interface font (seen in the real window, fixed).
- New test: `V::image_outputs_are_decoded_off_the_windows_thread_and_broken_ones_show_their_text`.

**Stage 3, the wiring.** Hunks 2 to 5 applied as written. On top of them:

- A test build without the `SdkDetection` answer finds no interpreter; the drafts fell through to the backend's detection, which runs the interpreters it finds.
- A `StandIn` guard kills its own `sleep 60` when a test ends, pass or fail.
- The fixed sleep is gone from the Restricted Mode test.
- New tests: `E::a_notebook_from_anywhere_draws_text_only_and_only_a_clicked_web_link_opens`, `E::a_test_build_without_an_answer_finds_no_interpreter`.

**Stage 4, Save All and Edit › Undo / Redo.** Hunks 6 and 7 applied.

- Natively, `file.save` ("Save All") saved only the active text editor; the webview saves every document (`src/editor/core/actions.ts:1200`).
- I put a handler in front of it (`notebook::register_actions`) that also saves the window's unsaved notebooks. Its text-editor part is unchanged.
- New tests: `E::save_all_writes_the_unsaved_notebook`, `E::edit_undo_and_redo_in_a_notebook_go_through_its_history`.

Small additions from the brief:

- **Syntax colours: left out.** `TextArea` has no coloured ranges (`TextTag` / `set_tags` exist only on the single-line `TextField`), so adding them means changing how it paints.
- **Corrections to `features_a/notebook/`: none applied**; no test needed them.

## 3. Rows

| Key                                              | State                            | Proving tests                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | Missing                                                                                                                                                                                                                                                        |
| ------------------------------------------------ | -------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `point.registerEditorKind:notebook`              | partial                          | E::a_notebook_file_opens_in_a_notebook_tab_and_saves_the_typed_source, `openidea_gpui::workbench::notebook::tests::the_notebook_kind_is_restorable_and_titled_by_its_file`, E::closing_the_last_tab_of_a_notebook_releases_its_backend_document, E::an_unsaved_notebook_asks_before_its_tab_closes, E::save_all_writes_the_unsaved_notebook, E::edit_undo_and_redo_in_a_notebook_go_through_its_history                                                                                                                                                                                                                         | No test of session restore; no dirty mark on tabs; the save the VCS and run operations do first (hunk 6) has no test                                                                                                                                           |
| `point.registerFileEditorProvider:notebook`      | partial                          | E::a_notebook_file_opens…, `openidea_gpui::notebook::tests::notebook_files_open_as_notebook_tabs_titled_by_their_name`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          | A window without a project (LightEdit) opens `.ipynb` as JSON text (shot dnbi1-01)                                                                                                                                                                             |
| `feature:notebook`                               | partial                          | all V:: and E:: tests                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           | See the behaviours list below                                                                                                                                                                                                                                  |
| `ipc:notebook_open`                              | done                             | E::a_notebook_file_opens… (key `notebook:<canonical path>`, cells from the file)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                | —                                                                                                                                                                                                                                                              |
| `ipc:notebook_get`                               | done                             | E::typing_after_a_change_behind_the_editor_is_rebased_on_the_current_notebook                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | —                                                                                                                                                                                                                                                              |
| `ipc:notebook_edit`                              | done                             | E::typing_after_a_change…, E::a_notebook_file_opens…                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            | —                                                                                                                                                                                                                                                              |
| `ipc:notebook_save`                              | done                             | E::a_notebook_file_opens…, E::save_all_writes_the_unsaved_notebook (file on disk)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               | —                                                                                                                                                                                                                                                              |
| `ipc:notebook_undo`, `ipc:notebook_redo`         | done                             | E::undo_and_redo_of_the_toolbar_go_through_the_notebooks_history, E::edit_undo_and_redo_in_a_notebook_go_through_its_history (backend document)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 | —                                                                                                                                                                                                                                                              |
| `ipc:notebook_reload`                            | done                             | E::reload_from_disk_asks_then_drops_unsaved_changes_for_the_files_content                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       | —                                                                                                                                                                                                                                                              |
| `ipc:notebook_close`                             | done                             | E::closing_the_last_tab_of_a_notebook_releases_its_backend_document                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             | —                                                                                                                                                                                                                                                              |
| `ipc:notebook_interrupt`                         | done                             | E::interrupt_kernel_stops_a_batch_whose_kernel_is_starting                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      | Interrupting a running cell needs the kernel seam                                                                                                                                                                                                              |
| `ipc:notebook_execute`                           | partial                          | E::a_kernel_that_cannot_start_shows_why_with_the_install_hint, E::a_notebook_in_restricted_mode_starts_no_kernel                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                | Outputs of a successful run (no seam)                                                                                                                                                                                                                          |
| `ipc:notebook_restart`                           | partial                          | E::a_notebook_in_restricted_mode_starts_no_kernel (refused with the backend's reason)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           | Restart of a dead or live kernel                                                                                                                                                                                                                               |
| `event:NotebookKernel`                           | done                             | E::a_kernel_that_cannot_start_shows_why_with_the_install_hint (state `dead`), V::a_kernel_failure_naming_ipykernel_shows_the_install_hint                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       | —                                                                                                                                                                                                                                                              |
| `unit:src/features/notebook/OutputView.test.tsx` | partial, 9 of 9 by name, 7 exact | Exact: E::a_notebook_from_anywhere_draws_text_only_and_only_a_clicked_web_link_opens (`cx.opened_url()` equals the link) with V::opens_web_links_with_the_system_handler; U::keeps_the_rendered_preview_across_snapshots_with_equal_attachments; U::resolves_an_entity_encoded_attachment_in_a_markdown_destination_a_reference_definition_and_raw_html; U::resolves_an_attachment_named_proto_or_constructor_from_parsed_notebook_json; U::does_not_resolve_inherited_members_as_attachments; U::does_not_rebuild_when_only_an_unreferenced_attachment_changes; U::ignores_attachment_identity_when_the_source_references_none | Two neighbours: U::leaves_native_interactions_such_as_details_alone_and_only_claims_links (tags kept; `<details>` is always open natively) and V::does_not_re_sanitize_an_unchanged_svg_when_a_fresh_snapshot_arrives (checks a PNG, since SVG is never drawn) |
| `unit:src/features/notebook/security.test.ts`    | partial, 4 of 4 by name, 3 exact | U::keeps_raw_markdown_markup_from_reaching_the_page, U::retains_table_content_while_removing_executable_navigation_and_external_resource_surfaces, U::resolves_only_embedded_image_attachments_and_makes_markdown_file_links_editor_controlled                                                                                                                                                                                                                                                                                                                                                                                  | Neighbour: U::retains_safe_svg_chart_paint_local_definitions_and_raster_images_but_removes_active_external_svg asserts SVG is never drawn                                                                                                                      |
| `unit:src/features/notebook/outputs.test.ts`     | done, 8 of 8                     | `openidea_gpui::features_a::notebook::outputs::tests::*` (existing, pass in both suites)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        | Not reached end to end (no successful run)                                                                                                                                                                                                                     |
| `unit:src/features/notebook/session.test.ts`     | done, 16 of 16                   | `openidea_gpui::features_a::notebook::session::tests::*` (existing)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             | —                                                                                                                                                                                                                                                              |

Hypothesis: the Tauri adapters call the same `ide_host::ops::notebook::*` functions with the same arguments the native calls use. I read the ops, not `src-tauri/src/commands/notebook.rs`. Confirm with `grep -n "ops::notebook" src-tauri/src/commands/notebook.rs`.

Behaviours of the webview's notebook editor that are missing natively:

- Syntax colours and line numbers in cells.
- A virtualised list and a remembered scroll position.
- A spinner while busy.
- The interpreter's path in the interpreter menu.
- Opening and closing `<details>`.
- Accessibility labels.
- Notebook autosave.
- The 256-events-per-frame bound on outputs.
- Notebooks in LightEdit windows.
- Restart of a dead or live kernel, and a successful run.
- The `save` icon (it falls back to another glyph).
- Save All of all text editors (natively only the active one, a gap outside this slice).

## 4. Which way each hypothesis went

- **1 to 7 (gpui_kit API):** confirmed by compiling; only `AppContext` imports were missing.
- **8 (reference-style images):** confirmed, the third input failed with `left: []`. Fixed locally in `markdown_cell.rs`.
- **9 (`<details>` and `<summary>` survive the renderer):** confirmed (U::leaves… passes).
- **10 (images are decoded within a test redraw):** confirmed. Decoding is now handed off and counted when it finishes.
- **11 (kernel ports on loopback):** kept, as the brief decided.
- **12 (`SdkDetection` keeps tests off the machine):** wrong. Without the global a test fell through to the backend's detection. Fixed and asserted by E::a_test_build_without_an_answer_finds_no_interpreter.
- **13 (a dead kernel is noticed within the startup probe):** true for Run, measured. False for Restart: it starts the interpreter again and waits the 30 s connect timeout (measured).
- **14 (Interrupt stops a starting kernel):** confirmed, 20 of 20.
- **15 (Save, Don't Save, Cancel labels):** confirmed by E::an_unsaved_notebook_asks_before_its_tab_closes.
- **16 (key claims run before the keymap):** confirmed for ⌘Z (V::typing_queues_a_source_edit_and_cmd_z_undoes_in_the_notebook_history). Hypothesis: in the workbench ⌘S reaches the claim rather than `file.save`; the test passes either way. Confirm by logging which handler runs.
- **Open File and Search Everywhere go through `open_project_file`:** confirmed by reading (`app_shell.rs` `open_tabs`; `SearchEverywhereEvent::OpenFile`), not by test.
- **Tab in a cell, input-method composition, CRLF sources:** not settled.

## 5. Safety proofs

**Nothing in a notebook is fetched, run or opened by drawing it.** E::a_notebook_from_anywhere_draws_text_only_and_only_a_clicked_web_link_opens asserts:

- `cx.opened_url() == None` and no prompt after drawing.
- `None` again after clicking the `javascript:` and `data:` links.
- The https link opens only when clicked.
- The SVG draws 0 images.
- The preview holds none of `attacker.invalid`, `alert` or `secret`.
- The stand-in interpreter is never started.

**Broken images and decoding off the window's thread.** V::image_outputs_are_decoded_off_the_windows_thread_and_broken_ones_show_their_text covers a truncated PNG and data that is not base64: no panic, and decoding is still pending before the test executor parks. The proof is by state: the test executor runs background work on its own thread.

**50 MB payload, measured (debug build):**

- Before the fix, every redraw took 1.32 s.
- After, with a 49,947,420-byte file holding a valid 3060×3060 PNG: preparing it took 4.76 s, redraws 0.35 to 0.39 ms, a typed character 3.75 ms (`S/dnbi/measure2.log`).
- Through the real backend, with a 50,000,252-byte file opened by `open_project_file`: 4.70 s until ready, redraws 1.5 to 1.7 ms, the keystroke 13.6 ms, the edit committed in 23.1 ms (`S/dnbi/measure3.log`).
- The temporary measurement tests are removed.
- Hypothesis: in the real app the 4.7 s preparation runs on a background thread while the window stays responsive. It was not measured in a real window.

**No interpreter in tests.** E::a_test_build_without_an_answer_finds_no_interpreter. In E::a_notebook_in_restricted_mode_starts_no_kernel the marker file is never written.

**No `sleep 60` left behind.** After the last suite run and the loop, `pgrep -fl 'sleep 60'` printed nothing (exit 1), and `ps` showed no cargo, test binary or app process of mine.

**Real binary.** I ran it only through `S/insp8/run.sh`, with `no-toolchains` as the first step.

- By reading: loading a notebook asks for the interpreters, and that call answers from the `SdkDetection` global that `no-toolchains` sets, so the backend's detection is never called. I extended the `no-toolchains` doc in `inspect.rs` to say so.
- The app's child processes, sampled every 250 ms for the whole of runs dnbi3 and dnbi4: none (`S/dnbi/children3.txt` and `S/dnbi/children4.txt` are empty).
- `grep -ciE "python|jupyter|ipykernel"` on `S/insp8/dnbi4.stdout` printed 0.
- I never ran a cell, restarted a kernel, picked an interpreter, or opened Agents or Services. Every run was stopped by `run.sh` by PID and is gone.
- Run dnbi2 started with a missing script by my mistake: it only opened the project and was stopped at the 60 s limit by PID.

## 6. Shots

All under `S/insp8/shots/`.

- **`dnbi1-01-nb-first-1.png`** (untrusted folder): the trust dialog, and `demo.ipynb` shown as JSON text in a window without a project. This is the LightEdit gap.
- **`dnbi1-02-nb-toggled-1.png`:** taken but not looked at.
- **`dnbi3-01-nb-first-1.png`** (light):
  - The toolbar reads Run Cell (primary), Run All, Interrupt (dimmed), Restart, "Select Python interpreter", Refresh, "idle", then Undo, Redo, Save and Reload on the right.
  - The Markdown preview shows the heading, bold, italic, link, code and list.
  - The selected cell has the accent border; cells are evenly spaced with nothing cut or overlapping.
  - The stream output is mono, the PNG is at its 96×48 size, and the traceback is in red and green.
  - Defects seen: the cell source was in the interface font (fixed), and the Save icon falls back to another glyph.
- **`dnbi3-02-nb-toggled-1.png`** (dark): the same layout. The dark red of the traceback has low contrast; it is the same palette the webview uses (Anser's 187,0,0).
- **`dnbi4-01-nb-editing-light-1.png`:** after the font fix. Sources are in the code font; Markdown cell 4 is being edited with the eye icon, the accent border and the caret; Run Cell is off because a Markdown cell is selected.
- **`dnbi4-02-nb-editing-dark-1.png`:** the same in the dark theme.

## 7. False or stale in the notes and brief

- "Restart recovers" after a kernel death is false (measured above).
- Hypothesis 12, that `SdkDetection` keeps tests off the machine, is false: tests fell through to the backend's detection.
- The drafted `MarkdownCache` never reused a preview that has an attachment.
- The drafted view did work that grows with the payload on every frame: 1.32 s per redraw at 50 MB.
- The save test's substring check could not pass against the backend's split lines.
- Native "Save All" saved only the active editor.
- The brief's HEAD `48df2008` is stale: other agents committed `caff1546` meanwhile.

## 8. Files I edited

`app-gpui/src/lib.rs`, `app-gpui/src/inspect.rs`, `app-gpui/src/ui/text_area.rs`, `app-gpui/src/notebook/cells.rs`, `app-gpui/src/notebook/markdown_cell.rs`, `app-gpui/src/notebook/model.rs`, `app-gpui/src/notebook/output_items.rs`, `app-gpui/src/notebook/view.rs`, `app-gpui/src/notebook/tests/proofs24_notebook_view.rs`, `app-gpui/src/workbench/mod.rs`, `app-gpui/src/workbench/tab_views.rs`, `app-gpui/src/workbench/edit_actions.rs`, `app-gpui/src/workbench/notebook.rs`, `app-gpui/src/workbench/tests/proofs24_e2e_notebook.rs`.

Of the drafts handed to me, `notebook/mod.rs`, `ansi.rs` and `tests/proofs24_notebook_units.rs` were formatted without changes, and `proofs24_e2e_notebook_pending.rs` stays undeclared.

Scratch files I wrote: `S/dnbi/project/demo.ipynb`, `S/insp8/dnbi1.script`, `S/insp8/dnbi3.script`, `S/insp8/dnbi4.script`, `S/dnbi/loop20.sh`. Nothing under `crates/`, `docs/`, `src/`, `src-tauri/` or `e2e/` changed (`git status --short` on them prints nothing).
