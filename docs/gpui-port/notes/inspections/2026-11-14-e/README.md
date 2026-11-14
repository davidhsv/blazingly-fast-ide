# Fifth look at the real window (2026-11-14, working tree after `e2983b38`: the fixes for the fourth inspection's findings)

The shots were taken by the integration that made the fixes, with the inspection script (`tour.script`) on temp folders; I looked at each one afterwards.

| Shot | Screen                                  | What it shows                                                                                                                                                                                                  |
| ---- | --------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 01   | Click on the title bar's project widget | The popup from its first row (Open…, Recent Projects…, Close Project); no tooltip over it.                                                                                                                     |
| 02   | Settings → Editor → Font                | Labels as wide as their text, ending in a colon; every control inside the dialog; "Show only monospaced fonts" in full.                                                                                        |
| 03   | Settings → Plugins                      | The details pane's empty state centred inside the dialog.                                                                                                                                                      |
| 04   | Diff of a modified file                 | Both sides in syntax colours. **Defect**: the diff draws font ligatures (`->` as an arrow) while the editor's setting has them off.                                                                            |
| 05   | Database tool window                    | The filter field framed with its icon. At this pane width its placeholder is cut ("Filter object"); by the webview's CSS arithmetic its toolbar overflows at narrow widths too (not looked at in the webview). |
| 06   | Commit tool window at its default width | Commit and "Commit…": the second label is cut in both apps at 240 (computed from the webview's CSS with the real text system's widths).                                                                        |

Not looked at again: the terminal's boxed glyph (the temp shell's prompt has none).
