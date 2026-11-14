I wrote 23 tests in 13 new files (554 lines) and none of them has been compiled or run. Two will fail until a small product fix lands. Every coverage count below comes from reading the code.

**Where to find things**

- Notes, with every case table, the `mod` lines and their anchors, the proposed `parity.json` entries and the fix hunks: `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/proofs8_unit.md`
- Test files, all under `app-gpui/src/`:
  - `editor/view/tests/proofs8_caret_commands.rs`
  - `workbench/tests/proofs8_build.rs`
  - `workbench/tests/proofs8_file_refactor.rs`
  - `workbench/debug/tests/proofs8_recent.rs`
  - `features_a/vim/engine/tests/proofs8_vim.rs`
  - `vcs_diff/comparison/chunk_optimizer/tests/proofs8_chunk_optimizer.rs`
  - `vcs_more/merge_diff3/tests/proofs8_chunk_optimizer.rs`
  - `run/driver/tests/proofs8_chooser_pinned.rs`
  - `run/chooser_view/tests/proofs8_chooser.rs`
  - `run/store/tests/proofs8_removal.rs`
  - `features_b/viewers/preview_host/tests/proofs8_preview_only.rs`
  - `agents/session_view/tests/proofs8_agent_reply.rs`
  - `everywhere/structure_view/tests/proofs8_file_structure.rs`

I counted strictly, as the assignment says: a `.test.tsx` case proven only on a model is missing. For build, fileRefactor, chooserPinned and removal the notes also give the count if model proof of cases that render nothing were accepted. Those rows would then be complete, two of them once their fix lands.

**Would become done:** caretCommands 6/6, chunkOptimizer 4/4, previewOnly 5/5, fileStructure 4/4, AgentReply 3/3 plus one case with no native counterpart.

**Still partial:**

- **build, 4/13:** nine cases are proven only on `build_window::model`.
- **fileRefactor, 3/6 after its fix:** three pure-function cases are model only.
- **chooserPinned, 3/5 after its fix:** pins across reloads and the run half of "marks recent" are proven on the run driver, with no window.
- **removal, 9/11:** the two moved-file cases are store only.
- **vim, 15/17:** the escaped-notation case is proven only as far as which keys the mapping resolves to. The native editor feeds no keystrokes to a Vim engine, so "matches browser key events" cannot be tested.
- **todo, 12/13, no test written:** the native editor draws no TODO comment highlighting.

**Expected failures and their fixes** (both are exact hunks in the notes):

- **fileRefactor:** `stops_when_cancelled_before_the_search_answers` will fail. A Cancel pressed before the refactoring task first runs stops nothing, because the search's stop handle is stored only after saving. The fix creates it in the dialog's handler before the task starts.
- **chooserPinned:** `marks_a_configuration_recent_when_a_debug_session_starts` will fail. Run › Debug goes through `debug_config` and `debug.startConfig`, and nothing on that path marks the configuration recent. The fix adds `RunDriver::mark_recent` and calls it from `debug_config`.

**No native counterpart:** AgentReply's clipboard-failure case. The native clipboard write cannot fail, whereas the spec tests a browser clipboard permission being refused.

**Index numbers that were wrong:**

- **fileStructure:** 3 of 4 cases were already covered by `structure_view::tests`; the proof cited only `structure_window`.
- **todo:** 12/13, not 10/13, and the missing case needs a view rather than a test.
- **build:** only 3 cases were on a view, not 12.
- **vim and AgentReply:** one "test-only" case in each needs product or has no counterpart.
- **chunkOptimizer:** the cited tests ran `vcs::conflicts::optimize_line_chunks`, which no product code calls. I rewrote those two cases against the optimizer the diff and merge actually use. If the cases fail there, that optimizer differs from the spec.
- **chooserPinned, removal, fileRefactor:** some cases recorded as test-only need product fixes or window tests.

**Where I stopped:** all 11 qualifying spec files with up to 4 missing cases are handled. I started none with 5 or more (diffDialog first). I skipped Grid, executionState, languages, windows and keymap because other drafts' notes cover them.
