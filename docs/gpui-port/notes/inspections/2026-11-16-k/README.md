# 2026-11-16 (k): live templates in the real window (commit 4637e96a)

Taken by the integrator from its own debug binary (temp home, temp `HOME` and `ZDOTDIR`, short `PATH`, temp trusted projects under a neutral path). The Agents tool window, the Services window and the Agents page of Settings were not opened. I looked at the two files kept here; the integrator describes eight more that I did not open.

- `template-fields-light.png`: `fori` expanded in a TypeScript file; after one Escape the three linked `i` keep a field background and the status bar says "3 carets". The two empty fields (the array and the body) are not marked.
- `surround-with-dark.png`: Surround With on two selected lines of a Python file: the chooser under the selection with its items and numeric mnemonics, the first one selected across the chooser's width.

Seen by the integrator, not fixed: the selected row of the Insert Live Template lookup is highlighted only as wide as its text (hypothesis: the existing completion lookup's own drawing).
