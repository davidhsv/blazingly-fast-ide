# 2026-11-15 (i): the Agents tool window's New Task page

The debug binary on a temp home and a temp project, launched with a short search path (`/usr/bin:/bin:/usr/sbin:/sbin`) so no agent program of the machine is found. The temp home was seeded by `prep_home.sh` with a stand-in agent whose command is never run and the settings it would have reported. Nothing was sent and no task was started.

- `dagv1-01-newtask-1.png`: the New Task page with the default agent, which is not found ("claude was not found on PATH").
- `dagv3-01-chooser-1.png`: the agent menu. It is moved left to fit and sits flush at the window's right edge; two long labels end in an ellipsis.
- `dagv4-01-model-effort-1.png`: the Model & effort popup of the stand-in agent, below the box and inside the window, nothing cut. The Search field's caret is drawn over the first letter of its placeholder.

Open: the menu's missing gap to the window edge and the caret over the placeholder are queued with the shared control fixes.
