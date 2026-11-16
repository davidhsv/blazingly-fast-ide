# 2026-11-16 (e): the code previews in the real window (commit e1db281b)

Taken by the integrator from its own debug binary (temp home, temp `HOME` and `ZDOTDIR`, short `PATH`, a temp trusted project under a neutral path). The Agents tool window and the Services window were not opened. I looked at the four files kept here; the integrator's other shots (the dark Bookmarks and Usages ones, the typed-character ones from an earlier binary) I did not open.

- `find-in-files-dark.png`, `find-in-files-light.png`: Find in Files with one result selected. The preview is the file in the editor's font and colours with line numbers and no fold chevrons, the match marked, its line (429) tinted and in view.
- `bookmarks-light.png`: the Bookmarks popup with a bookmark on line 428 selected; the preview shows the file in colour with line 428 tinted. DEFECT seen, cause not investigated: the list labels that bookmark "(empty line)" although line 428 is a comment.
- `find-usages-light.png`: the Find Usages tool window with a usage selected; the rows are coloured, the preview is the file with the usage marked and its line tinted. The preview's first row is cut by the pane's top edge.

Not looked at by me: one character typed in the Find in Files preview (the integrator reports that nothing in the dialog shows the unsaved state, as in the webview, and that with autosave on the edit was written when the popup closed).
