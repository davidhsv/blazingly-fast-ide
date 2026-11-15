# 2026-11-15 (j): History for Selection and Refactor This

The debug binary on a temp home and a temp Git project (one file committed twice, then changed in two places), launched with a short search path.

- `rvs-01-selection-history-1.png`: the "History: main.rs:2-4" tab. Local Changes first, then the two commits; HEAD against Local Changes with the changed words marked. Nothing cut. The list's rows have no gap between them.
- `rvs-02-refactor-this-nothing-here-1.png`: the Refactor This popup with one greyed "Nothing here" row, inside the window. The row's highlight is as wide as its label, not the popup.

Both defects are queued with the shared control fixes.
