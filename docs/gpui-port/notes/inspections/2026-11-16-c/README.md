# 2026-11-16 (c): the notebook editor in the real window (commit 54bae391)

Taken by the integrator from its own debug binary (temp home, temp `HOME` and `ZDOTDIR`, short `PATH`, a temp trusted project holding one notebook written by hand, the `no-toolchains` step first). No cell was run, no kernel restarted, no interpreter picked; the app's child processes were sampled for the whole of both runs and there were none. I looked at the four files listed here.

- `dnbi3-01-nb-first-1.png` (light), `dnbi3-02-nb-toggled-1.png` (dark): the toolbar (Run Cell, Run All, a dimmed Interrupt, Restart, "Select Python interpreter", Refresh, "idle", then Undo, Redo, Save, Reload at the right); a rendered Markdown cell (heading, bold, italic, link, code, list); two code cells with their execution counts; a stream output in the code font, a PNG at its own size, a traceback in red and green; the selected cell with the accent border. DEFECT in these two, fixed before the next two: the cells' sources are drawn in the interface font.
- `dnbi4-01-nb-editing-light-1.png`, `dnbi4-02-nb-editing-dark-1.png`: after the fix. Sources are in the code font; the last Markdown cell is being edited (eye icon, accent border, caret); Run Cell is dimmed because a Markdown cell is selected.

Seen, not fixed: the Save button's icon is the fallback glyph (three lines), not a save icon; the traceback's dark red is hard to read on the dark theme (the same colour the webview uses).

Not looked at: a notebook in a window without a project (it opens as JSON text: a gap), a running cell, the interpreter menu, a very large output.
