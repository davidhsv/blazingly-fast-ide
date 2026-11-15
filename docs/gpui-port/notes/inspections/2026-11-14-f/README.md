# Sixth look at the real window (2026-11-14, working trees of D105 and D107)

Shots taken by the integrations with the inspection script on temp folders; I looked at each one.

| Shot | Screen                                                   | What it shows                                                                                                                                                                    |
| ---- | -------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 01   | `vcs.annotate` on a file with two commits by two authors | The annotation column left of the line numbers: date and author per line, the newer commit's lines stronger, its own border; nothing overlaps the numbers.                       |
| 02   | Go to Line                                               | A centred dialog with title, field, "Current position" hint, Cancel and OK, with a shadow.                                                                                       |
| 03   | Show Usages                                              | The popup under the caret's line, inside the window. To compare with the webview: its rows are drawn in the interface font, whose contextual alternates turn `->` into an arrow. |

Not looked at in these runs: the Services tool window (it lists this machine's Docker containers), a running program, a debug session at a breakpoint, the merge editor.
