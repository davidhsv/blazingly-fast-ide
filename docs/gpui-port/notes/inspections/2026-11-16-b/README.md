# 2026-11-16 (b): the visual defects slice in the real window

Taken by the integrator from its own debug binary (temp home, temp `HOME` and `ZDOTDIR`, short `PATH`, a temp trusted project under a neutral path). No request was sent, no sync run, no agent started; the Agents tool window and the Services window were not opened. I looked at every file listed here.

- `dvis4-01-http-light-1.png`, `dvis4-05-http-dark-1.png`: a request file. The run marks of lines 2 and 5 stand in a column of their own right of the line numbers; every digit shows.
- `dvis4-02-spring-light-1.png`: a Spring controller. The generate marks of lines 6 and 8 stand right of the numbers.
- `dvis4-03-gutter-light-1.png`, `dvis4-04-gutter-dark-1.png`: a Rust file with a breakpoint on line 2 (the dot replaces the number) and the caret on line 4. The right margin guide runs through the breakpoint row's tint and the caret row's tint (`z-margin-03-gutter-light.png`, `z-margin-04-gutter-dark.png` are enlarged crops).
- `dvis2-08-diff-1.png`: a diff window with long side labels. Both end in an ellipsis before their "editable" badge, in the editor's font.
- `dvis3-01-search-empty-1.png`: Search Everywhere, empty, with the keyboard. The caret stands before the placeholder's first letter and touches it; not changed (no difference in position was measured against the webview).
- `dvis3-02-toolbar-menu-1.png`: the main toolbar's menu near the right edge keeps a gap from the window's edge. SEEN, NOT FIXED: the menu's top lies over the lower half of the toolbar's buttons instead of under them.
- `dvis3-03-history-1.png`: History for Selection. The revision rows are inset and rounded, the detail line is smaller, and the rows no longer run into each other.
- `dvis3-04-refactor-nothing-1.png`: Refactor This with nothing applicable. The highlight of "Nothing here" spans the popup. (Taken before the gutter's final widths.)
- `dvis3-05-settings-php-1.png`: Settings › PHP. The help lines under the drop-down, the two lists and the path field all start where their labels start.
- `dvis3-06-term-away-header.png`, `dvis3-07-term-over-header.png`: crops of the Terminal's header with the pointer away (no Options and Hide buttons) and over the tool window (both shown). The full shots were deleted because they show the shell.

Not looked at: a bookmark and a changed line in the gutter (the native editor draws neither), the debugger's execution point, the Remote Host tree (it needs a server), the Agents menu (proved by test only).
