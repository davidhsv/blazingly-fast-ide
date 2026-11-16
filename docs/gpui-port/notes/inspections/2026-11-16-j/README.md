# 2026-11-16 (j): the Vim page of Settings in the real window (commit 927cef99): no shot kept

Taken by the integrator from its own debug binary with a temp home that held a small `.ideavimrc` sourcing a second file. I looked at two of its shots (Vim on in the light theme, Vim off in the dark one) and keep none: the page draws the rc file's full path, and the temp home was under the scratch folder, whose path holds the account's name.

What the two shots showed: the Enable Vim emulation checkbox; the Configuration file row with Reload, the rc file's path, "3 mappings", the line about what is supported, and two ignored lines of the sourced file with their reasons; the Shortcut conflicts table (shortcut, IDE action, what Vim does with the key, a handler list with Vim or IDE) cut by the page's bottom edge, the page scrolling on its own; Apply greyed out.

Seen, not fixed: the rc file's long path runs to the page's edge without an ellipsis; the first row's IDE action shows a raw action id cut short (`editor.inlineCompleti…`) where the others show a title or a dash; the Escape row's shortcut is drawn as an icon that does not read as Escape.

For the next look at this page: a temp home under a neutral path, never under the scratch folder.
