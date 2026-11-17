# Stage 3 report: driver fixes and one evidence run per app (no results)

All nine Part A fixes are in place and tested, and the evidence run (one run per app on the clone, idle 60 s, captures on) finished with status `ok` for both apps. But it showed two things that make the apps not directly comparable yet:

- **They rendered on different displays.** The native window was on the built-in 2x display and the Tauri window on the external 1x display. Every footprint and CPU figure below carries that difference.
- **The Git log showed different data.** Tauri shows only `HEAD`; native shows what looks like every ref, and drew no commit subjects.

Per your message, nothing more will be run; the campaign command in section 6 is only a record for later.

`$S` = `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad` and `<base>` = `/private/tmp/openidea-measure-i3NdVc`.

- My log is `$S/dmrun3/log.md`.
- The full diff against `$S/dmrun2/scripts/` is `$S/dmrun3/scripts.diff` (844 lines).
- `$S/dmrun3/native_gaps.md` is not mine, and I did not read it.

## 0. Before reading any number

**Different displays (measured).** The machine has two displays:

| Display                         | Size (points) | Pixels wide | Scale |
| ------------------------------- | ------------- | ----------- | ----- |
| Built-in (display 1)            | 1728×1117     | 3456        | 2x    |
| External (display 3, at x=1728) | 1920×1080     | 1920        | 1x    |

- **Native:** window at x=96 on the built-in display; captures are 3072×2158 pixels.
- **Tauri:** window at x=1968 on the external display; captures are 1440×900 pixels.
- This held in the stage 3 smoke and in the evidence run. In the pilot (attempt 2) Tauri was at x=144, on the built-in display.
- So native drew about 4 times as many pixels per point as Tauri here. Footprint, CPU and what the captures show all carry this.
- Hypothesis: Tauri places its window on the display that holds the frontmost window or the pointer at launch.
- **Before any campaign:** both apps' windows must be on the same display. Each run already records the window position (`stopped.window` in `events.jsonl`) and the capture size. I added no gate for it, because it could not be tested now.

**Machine load.** An integrator's cargo build ran during the evidence run, and load averages reached 18.97 (5-minute average, read at 19:13). Free disk went from 337 to 317 GiB. None of this run's numbers are results.

## 1. Part A: what changed, how it was tested, the result

**Which script versions ran.**

- The four Python scripts (`measure.py`, `macos.py`, `aggregate.py`, `stop.py`) were last changed between 18:33 and 18:35, before the first smoke at 18:35:49. The smokes and the evidence run used them as they are now.
- `run_all.sh` and `preflight.sh` were edited again after the evidence run (fix 6b below), at 19:11:41. The hashes in section 6 are of those final files, not of the versions that ran.

**Fix 1: captures of our own window.**

- **`macos.py`:** new `capture_window(id, path)`. It runs `/usr/sbin/screencapture -x -o -l <id>` and returns an error text instead of raising.
- **`measure.py`:** new `Launch.capture(label)`.
  - It takes `main_window(app)`, checks that the window's owner pid is the recorded app and `guard.check(app)` passes, then writes `<run>/captures/NN-<launch>-<label>.png`.
  - It logs a `capture` event (window id, owner pid, title, size). A failure logs `capture_failed`, never an abort.
  - It is called by `snapshot_tools` right after it switches to the `tools_*` window, so Find and the log are still open. It is also called after the first and the last file opens and after each one's scrolling (deduplicated when there is one file).
- **`--capture` flag** in `measure.py`, passed through by `run_all.sh`.
- **Tested:** both smoke runs and both evidence runs wrote all their captures (10 per smoke run, 12 per evidence run), every one with `owner_pid` = the recorded app.
- Not seen live: the `capture_failed` path never fired.
- No whole-screen capture and no capture of another window was ever made. The captures stay under `<base>`.

**Fix 2: Git log.** See section 2. `measure.py` (`launch_project`) no longer posts the 20 PageDowns and no longer has the "git log scrolled" settle.

**Fix 3: Find in Files.**

- No key change was needed: the native app shows results with the keys as posted.
- **Smoke** (query `bravo`, `--query` only for the smoke): both apps show "1 match in 1 file" (capture `06` in `<base>/smoke/runs3-attempt1/1-{native,tauri}/captures/`).
- **Evidence run** with the campaign query: see section 3.

**Fix 4: frontmost record.**

- **What the sampler records each second:** `lsappinfo` with a 2 s timeout, stored as `front` and `front_is_app` in every sample. `None` means unknown, not "not frontmost".
- **Events:**
  - a `frontmost` event (pid, executable path) on every change;
  - per-scenario counts (`samples`, `not_frontmost`, `unknown`, `paths`), logged as `scenario_frontmost` when the scenario ends;
  - `not_frontmost` per scenario in `status.json`.
- **Idle:** any not-frontmost sample logs `idle_not_frontmost` and puts `idle` in `degraded_scenarios`. The run's status stays `ok`; `aggregate.py` leaves out only that run's idle row (unless `--include-degraded`).
- **Tested:**
  - Synthetic (`$S/dmrun3/tests/test_aggregate.py`, passed): not-frontmost samples are counted; the idle row of a run marked degraded is dropped while its other scenarios are kept.
  - Live: only the counting. The `idle_not_frontmost` branch never fired in a live run.
- **Measured in every launch:** exactly one not-frontmost sample, the first sample of `cold_start` and of `open_project`, before the app has activated itself. The path in front was `/Applications/Claude.app/Contents/MacOS/Claude`.

**Fix 5: one CPU definition.**

- **Where it lives:** `aggregate.py` (`Readings`, `segments`, `scenario_metrics`), and `measure.py` imports it.
- **Segments:** a scenario is every sample with its label. The closing keys (the Escape after Find, the Cmd+9 that hides the log) and their settles are labelled back into the scenario after its `tools_*` window, so a scenario can have 2 segments.
- **Per segment:** each process's cumulative CPU at the segment's last sample, minus its reading at the boundary (the last sample before the segment).
  - A process first read inside the segment counts from zero if its `start_us` is at or after the boundary sample's `t`; otherwise it counts from its first reading.
  - Wakeups use the same rule.
- **Steady:** the last 10 samples (idle: 60) of the scenario's first segment, i.e. before its snapshot window and closing keys.
- **The driver's own settle CPU** (`Sampler.cpu_hist`) now uses the same rule, with the same boundary. Each sample also carries the driver's running total (`cpu_ns_rule`) and each process's `start_us`.
- **Raw per-second cumulative CPU per process is kept in each run's folder:** `samples-{cold,project}.jsonl`, field `procs[].cpu_ns`.
- **`analyze.py`** was not carried over: `aggregate.py` is the only script.
- **Tested:**
  - Synthetic: split segments, a helper counted from its first reading, a child counted from zero. Passed.
  - Self-check: the per-label sums equal the driver's `cpu_ns_rule` to the nanosecond in every launch (smoke and evidence; listed at the end of each `RESULTS.md`).
  - In the evidence samples, every process was counted from the launch or "from zero, started inside". The "from first reading" branch never occurred.

**Fix 6: socket holder check.**

- **`preflight.sh`, `run_all.sh` (`sock_held`) and the smoke script** now run `lsof -a -p <our pids> -U`. lsof is never run without a pid list.
- **First version failed its positive test:** lsof names the socket by the path it was bound with, `/tmp/ai_l0ng_openidea_measure_si.sock`, not `/private/tmp/...`. So it now matches `/<file name>`.
- **Retest with the Tauri smoke app alive:**
  - `lsof -t <socket>` found nothing.
  - The new check found pid 80167, fd 11, and `preflight.sh` printed NO-GO.
  - After the stop it found no holder and the stale socket was removed.
  - Evidence: `$S/dmrun3/fix6-holder-check.txt`.

**Fix 6b: not in the brief, needed for the runs root.**

- **Defect:** the check "processes from the base are running" (`grep -F "$base/"`) matched `run_all.sh`'s own command line, because `--runs-root <base>/evidence` names the base.
- **Evidence attempt 0 stopped at preflight with NO-GO**, before anything launched: `$S/dmrun3/evidence-attempt0-nogo.out`.
- **First fix:** exclude the script and its ancestors. The evidence run then ran with that version.
- **Second false positive:** its end-of-run line `STILL RUNNING … 90146 /bin/sh …run_all.sh` was run_all's own `$(…)` subshell, which has the same command line. `stop.py` and `ps` at that moment found nothing else.
- **Second fix (after the run):** also exclude processes whose command line equals the script's own and whose parent is in its chain. The apps and `measure.py` are never excluded.
- **Tested** through the extracted function (`$S/dmrun3/tests/from_base_t.sh`, which prints empty with `<base>/evidence` in its arguments) and with a decoy process naming the base, which both checks reported (`$S/dmrun3/fix-frombase-positive.txt`). Not tested through a full `run_all.sh` run.

**Fix 7.** The sum of each process's lifetime maximum footprint is dropped. `RESULTS.md` says once that it was dropped because the word read for it was below the current footprint 31 times in the pilot.

**Fix 8.**

- **Per scenario and app:** peak footprint, steady footprint and CPU, each as app / WebKit / children / total / total without children.
- **Also:** % of a core, wakeups/s, peak resident size, processes, and samples not frontmost.
- **Also new:** `--runs-root` in `aggregate.py`, `stop.py` (repeatable) and `run_all.sh`, which also takes `--idle-s` and writes `RESULTS.md` under the runs root.

**Fix 9.** Unchanged: no focus hand-back; `--ax-probe` off (I did not need it); `.git` and `.git/index` writes are listed and not fatal; editor settle at 10% for both.

**Order of testing.** I tested on the smoke project (`--idle-s 20`, native first): `$S/dmrun3/smoke-attempt1-{native,tauri}.out`, `<base>/smoke/runs3-attempt1/`. Both runs were `ok`.

- No safety gate was weakened, and both apps are treated alike.
- The smoke script's trailing "syntax error" line came from my editing that file while it ran. The run itself had already finished `ok`.

## 2. Git log decision: scrolling dropped from both apps

The scenario is now: Cmd+9, settle, capture, then Cmd+9 to hide, settle.

**Why: there is no key sequence that is the same work in both apps.**

- **Tauri (read, and visible in captures):** activation focuses the first focusable element in the tool window that is not a button (`src/workbench/toolwindows/ToolWindows.tsx:841-851`), which is the "Filter branches and tags" field. The commit list (`GitLog.tsx:1727-1744`) comes after a Tab chain through the branch tree and a filter toolbar whose buttons hide depending on width.
  - The smoke capture `<base>/smoke/runs3-attempt1/1-tauri/captures/07-project-git_log.png` shows the caret in "Branch or tag".
- **Native:** no generic Tab traversal (no `tab_stop` or `focus_next` in `app-gpui/src` outside the Find popup and the diff).
  - Hypothesis (read only, not seen): Cmd+9 focuses the commit list (`workbench/mod.rs:2402-2407`, `Panel::new(log)`). The captures show no selected row.
- **Different scroll steps (read):** PageDown moves 20 rows in Tauri (`GitLog.tsx:1559`) and 10 natively (`vcs/log_view.rs:357-362`).
- No bridging key was tried. "No common sequence exists" comes from reading only.

**The log still differs between the apps (evidence capture 09 in each run).**

- **Tauri** shows a "Branch: HEAD" chip and HEAD's commits with subjects, authors and dates, newest 09/11/2026. Read: the default is `["HEAD"]` (`src/features/vcs/logStore.ts:205-212`).
- **Native** shows no branch chip. Its rows carry an `origin/release` label, authors and dates from today, and the graph and subject column are empty.
  - Hypothesis: an empty branch filter means every ref natively. I have not read the backend path that would confirm it.
  - Why the subjects are missing is unknown. The smoke capture (one commit) drew its subject.
- So native 0.3 s against Tauri 3.6 s of CPU for this scenario is not the same work.

## 3. Evidence run

**Command:** `cd <base> && /bin/sh $S/dmrun3/scripts/run_all.sh <base> --reps 1 --capture --runs-root <base>/evidence --idle-s 60`.

**Course of the run.**

- Started about 18:50:05 and ended 19:11:08, rc 0 (`$S/dmrun3/evidence.out`).
- Native ran 524 s and Tauri 561 s; then came the 60 s rest and the checks.
- **Session:** no abort, so the sampler never saw a locked screen or a sleeping display; `session` stayed unlocked and awake at the end.
- **Status:** `ok` for both apps, `degraded_scenarios: []`.
- **Writes into the clone:** 3 entries under `.openidea` per run, 0 git index, 0 elsewhere. The index sha256 was unchanged (36bafa86…). The files moved to `<base>/evidence/`.

**Every event logged**, the same counts per app except `member`:

| Event                                                                                               | Count per app      |
| --------------------------------------------------------------------------------------------------- | ------------------ |
| `posted`                                                                                            | 457                |
| `waited`                                                                                            | 55                 |
| `scenario`                                                                                          | 21                 |
| `scenario_frontmost`                                                                                | 19                 |
| `windows`                                                                                           | 17                 |
| `capture`                                                                                           | 12                 |
| `file_opened`                                                                                       | 10                 |
| `frontmost`                                                                                         | 6                  |
| `member`                                                                                            | native 4, Tauri 19 |
| `run`, `spawned`, `record`, `app`, `focus_not_handed_back`, `signal`, `fakehome_library`, `stopped` | 1 or 2 each        |
| `end`                                                                                               | 1                  |

- No `timeout`, `abort`, `warning`, `file_not_opened`, `mode_not_switched`, `capture_failed`, `unreadable` or network line.
- `lifetime_max_word_ok` was false in both project launches.
- **Tauri's children** (each child's executable path is in its `member` event):

| Scenario       | Children                                             |
| -------------- | ---------------------------------------------------- |
| `open_project` | 4 × `git`                                            |
| `open_files`   | `python3` (Command Line Tools), `git`                |
| `deep_mode`    | 2 × `java` (openjdk 26 and adoptopenjdk 8), `clangd` |

- The Command Line Tools `python3` also left `fakehome/Library/Caches/com.apple.python`.
- Native had no child.

**Frontmost record.**

- In each app the only not-frontmost second is the first sample of `cold_start` and of `open_project`, with Claude in front.
- Idle: 0 of 60 not frontmost in both.
- After each stop the front went back to Claude (`focus_not_handed_back`, `owner_app_pids: []`).

**Captures**, in `<base>/evidence/1-native/captures/` and `<base>/evidence/1-tauri/captures/`. Small copies and top-bar crops are in `captures/crops/`, also under the base.

| #       | Scenario           | Native                                                                                                      | Tauri                                                                                                                                      |
| ------- | ------------------ | ----------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------ |
| 01      | cold               | Welcome window, version 0.1.40, a single "Open" action                                                      | Welcome window, version 0.1.46, six actions (New File, New Project, Open, Open File, Clone Repository, Remote Development)                 |
| 02      | open_project       | Project tree, empty editor area with the shortcut hints, "Blaze mode"                                       | Same layout, plus Vim, "No interpreter", a badge "10" on the commit tool window and "99+" on another                                       |
| 03 / 04 | first file         | Lines 1–141 with imports folded, then lines 4210–4253 (end, caret 4253:1)                                   | Lines 1–132, then the end (caret 4253:1) under two pinned header lines                                                                     |
| 05 / 06 | last file          | Lines 1–144, then 3033–3076 (end)                                                                           | Lines 1–134, then the end (3076)                                                                                                           |
| 08      | Find               | "2,000+ matches in 654+ files", query typed, 9 rows visible with the last selected, preview of another file | "2,000+ matches in 647+ files", 10 rows with the last selected, preview of a different file                                                |
| 09      | Git log            | See section 2                                                                                               | See section 2                                                                                                                              |
| 10      | deep_mode          | Top bar "Deep mode", status bar a red dot "jdtls +1"                                                        | "Deep mode", "jdtls +1", and a notification that the Eclipse JDT language server is not installed, with Install and Language Servers links |
| 11 / 12 | blaze_again / idle | Top bar "Blaze mode" (only the top-bar crop viewed)                                                         | "Blaze mode" (12 viewed in full: the JDT notification is still on screen during idle; 11 only as the top-bar crop)                         |

- The pictures at the end of each file prove the file ended at its last line. They would look the same after Cmd+Down alone, so they do not prove that the 40 PageDowns scrolled.
- Not viewed in full: native 11 and 12, Tauri 07 and 11. Native 07 and Tauri 06 and 12 were viewed in small copies.

## 4. Numbers (NOT results)

**Pilot attempt 2 recomputed with the one definition.** Total CPU in seconds, old `aggregate.py` / `analyze.py` / one definition. Files: `$S/dmrun3/pilot_attempt2_recomputed.md` and `pilot_cpu_compare.md`.

| Scenario      | Native             | Tauri              |
| ------------- | ------------------ | ------------------ |
| cold_start    | 0.2 / 0.3 / 0.3    | 1.2 / 1.2 / 1.2    |
| cold_steady   | 0.1 / 0.1 / 0.1    | 0.4 / 0.4 / 0.4    |
| open_project  | 14.7 / 14.8 / 14.8 | 19.2 / 19.2 / 19.2 |
| open_files    | 6.1 / 6.2 / 6.2    | 42.7 / 42.8 / 42.8 |
| find_in_files | 0.9 / 1.0 / 1.0    | 2.1 / 2.2 / 2.2    |
| git_log       | 1.9 / 1.9 / 1.9    | 1.0 / 3.4 / 3.4    |
| deep_mode     | 0.3 / 0.4 / 0.4    | 8.2 / 9.1 / 9.1    |
| blaze_again   | 0.2 / 0.2 / 0.2    | 5.1 / 5.5 / 5.5    |
| idle          | 1.3 / 1.3 / 1.3    | 25.5 / 25.6 / 25.6 |

- The largest move is Tauri's `git_log`, 1.0 → 3.4 s: the old script dropped the first interval of each scenario.
- **Caveats of this recompute:**
  - The pilot samples have no start times, so every process first read inside a segment counts from zero (Hypothesis that this is right).
  - The pilot's closing keys still sit in `tools_*`, because sample labels cannot be changed after the fact.
  - Durations now include one more interval.

**Evidence run, one run each, not results**, under load and on different displays (section 0). Full table: `<base>/evidence/RESULTS.md`.

| Scenario      | s (n/t)       | Peak total MiB (n/t) | Steady total MiB (n/t) | CPU s app (n/t) | CPU s WebKit (t) | CPU s children (n/t) | CPU s total (n/t) | % core (n/t) | Wakeups/s (n/t) |
| ------------- | ------------- | -------------------- | ---------------------- | --------------- | ---------------- | -------------------- | ----------------- | ------------ | --------------- |
| cold_start    | 6.0 / 7.0     | 101.5 / 208.8        | 99.2 / 165.2           | 0.3 / 0.5       | 0.7              | 0 / 0                | 0.3 / 1.2         | 5.8 / 16.8   | 89 / 190        |
| cold_steady   | 31.1 / 30.1   | 99.2 / 171.8         | 91.8 / 166.0           | 0.1 / 0.1       | 0.2              | 0 / 0                | 0.1 / 0.2         | 0.2 / 0.8    | 87 / 118        |
| open_project  | 43.8 / 45.2   | 250.3 / 437.5        | 250.3 / 422.4          | 14.8 / 17.7     | 1.7              | 0 / 0                | 14.8 / 19.5       | 33.8 / 43.1  | 83 / 144        |
| open_files    | 165.8 / 197.9 | 299.6 / 777.9        | 284.7 / 711.7          | 7.1 / 8.1       | 32.6             | 0 / 0                | 7.1 / 40.7        | 4.3 / 20.6   | 118 / 379       |
| find_in_files | 24.1 / 26.1   | 306.7 / 818.9        | 302.1 / 767.5          | 1.0 / 1.6       | 2.4              | 0 / 0                | 1.0 / 4.0         | 4.2 / 15.3   | 107 / 321       |
| git_log       | 12.1 / 12.0   | 314.9 / 837.6        | 311.0 / 820.9          | 0.3 / 2.1       | 1.5              | 0 / 0                | 0.3 / 3.6         | 2.5 / 29.9   | 94 / 321        |
| deep_mode     | 71.3 / 73.3   | 306.1 / 951.7        | 296.7 / 853.8          | 0.4 / 2.0       | 3.2              | 0 / 2.1              | 0.4 / 7.3         | 0.5 / 10.0   | 92 / 340        |
| blaze_again   | 71.3 / 71.3   | 333.5 / 864.4        | 331.2 / 851.4          | 0.2 / 1.1       | 2.9              | 0 / 0                | 0.2 / 4.1         | 0.2 / 5.7    | 90 / 332        |
| idle (60 s)   | 60.3 / 60.2   | 334.5 / 863.7        | 334.5 / 854.4          | 0.1 / 0.7       | 1.8              | 0 / 0                | 0.1 / 2.4         | 0.1 / 4.0    | 87 / 336        |

- Peak children: Tauri 7.1 MiB in `open_project`, 13.2 in `open_files`, 116.0 in `deep_mode`.
- In this run, total without children equals total in every steady column.

## 5. What the editors draw differently, and what Deep mode did

**Seen in captures 03–06; described, not judged.**

| What                           | Native                                                                         | Tauri                                                                     |
| ------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------- |
| Window                         | 1536×1079 points, about 44 lines                                               | 1440×900 points, about 35 lines                                           |
| Display                        | 2x built-in                                                                    | 1x external                                                               |
| Syntax colours                 | Annotations orange; constants and enum values purple italic; method names blue | Annotations olive; constants plain, not italic                            |
| Gutter                         | Line numbers, fold arrow, run arrows on the class and on tests                 | The same, plus a scrollbar                                                |
| Inline author line             | None                                                                           | An inline author and date line above declarations and tests               |
| Indent guides                  | None                                                                           | Present                                                                   |
| Pinned header lines at the top | None                                                                           | Present                                                                   |
| Inspection count               | None                                                                           | An info count (161 in the first file, 59 in the last) with up/down arrows |
| Squiggles                      | None                                                                           | Squiggle underlines on some identifiers                                   |
| Tab icons                      | No mark                                                                        | Red marks on the tab icons                                                |
| Status bar                     | Mode, position, LF, UTF-8, indent                                              | The same, plus a path breadcrumb, Vim and "No interpreter"                |
| Project tree                   | Ignored folders grey                                                           | Ignored folders in orange                                                 |
| Tool window badges             | None                                                                           | "10" and "99+"                                                            |
| Minimap                        | None                                                                           | None                                                                      |
| Tool windows open by default   | Project only                                                                   | Project only                                                              |

**Deep mode.**

|                                        | Native                                                                                                                                              | Tauri                                                                                                             |
| -------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- |
| `settings.toml` says `mode = "full"`   | After 0.004 s                                                                                                                                       | Within the same wait                                                                                              |
| Processes started (1 Hz sampler)       | None                                                                                                                                                | 2 × `java` and `clangd` (pids 95810, 95809, 95844)                                                                |
| Log lines after the switch             | None; the last line is from before it                                                                                                               | `mvn … help:effective-pom` (`ide_build::process`, also in `tauri/logs/…`)                                         |
| Files written within 1 s of the switch | `local-data/java-usages-1.15.0.jar`, `jvm-inspect-1.0.0.jar`, `lsp/jdtls/compiler-defaults.prefs`, `tools/lombok/bin/lombok-1.18.48-openidea.3.jar` | The same kinds of files, plus `cache/<id>/build/maven.json`, `kotlin-bridge-options.json` and `tools/.python-cwd` |
| What it showed                         | "jdtls +1"                                                                                                                                          | "jdtls +1" and the JDT notification                                                                               |

- In the pilot, native Deep mode had a `java` child (openjdk 26). This time it had none. Why is unknown (Hypothesis: a short-lived child the 1 Hz sampler missed, or a cache state).
- Which `java` belongs to `mvn` is a Hypothesis.
- The native log again shows "Accessibility activated" while our probe was off. Hypothesis: another Accessibility client on the machine.
- Hypothesis: the two welcome versions differ (0.1.40 vs 0.1.46) although both apps come from one commit.

## 6. Campaign command (a record only, not to be run now)

**Precondition:** both apps' windows on the same display (section 0), and the Git log difference (section 2) decided.

```
cd /private/tmp/openidea-measure-i3NdVc && /bin/sh $S/dmrun3/scripts/run_all.sh /private/tmp/openidea-measure-i3NdVc --reps 3 --capture --runs-root /private/tmp/openidea-measure-i3NdVc/campaign > $S/dmrun3/campaign.out 2>&1; echo $? > $S/dmrun3/campaign.rc
```

**sha256 of the files it uses**, the final versions (`run_all.sh` and `preflight.sh` changed after the evidence run, see fix 6b):

| File           | sha256                                                           |
| -------------- | ---------------------------------------------------------------- |
| `run_all.sh`   | 8c582b07711983a160beab0a79ad2a76e33c1382cf81b044d7b5acd9728a0192 |
| `preflight.sh` | 3974241c808a1efd811787b61ac9a14ce82ef58c002aa446f99b7cc4dc16d611 |
| `measure.py`   | 4b1ea3e9202c1c90d977e727354d41d094873901fa569c1b8e1cf742ee5e9c95 |
| `macos.py`     | f6b0ac0bfe2abd20a2a8c1ad7d0f49bd2ae0ca5b2041555105459a2391ad1509 |
| `aggregate.py` | 8ddf789ed701a24d1f660a70c6612deaf176eb5c489a312349951fe9d917e408 |
| `stop.py`      | bef7df75abf356b77938d0d1df0930b1e004d5685d03cec8634be6cd4594a856 |

**Wall time** from measured times: native 524 s and Tauri 561 s with idle 60, so each run gets about 540 s more with idle 600. That is about 18.5 min per app run, plus 2 × 60 s rest and about 1 min of checks, or about 39 min per repetition. The pilot measured 39.4 min. Estimate: about 2 h for 3 repetitions (Hypothesis: settle timeouts would add up to 60 s each).

## 7. Nothing of mine is running, and what is left

**Last check, 19:21:26** (`$S/dmrun3/final-state-2.txt`):

- `stop.py --runs-root <base>/evidence --runs-root <base>/smoke/runs3-attempt1` printed "nothing of ours is running".
- `ps -axo pid=,command= | grep -F "<base>/"` printed nothing.
- The measure socket is absent.
- No wait loop is left (`ps` found none).
- caffeinate: 51629, 53718 (the owner's) and 60633 (yours, with its shell 60632), all untouched. run_all's caffeinate 85436 is gone.
- Session unlocked and awake.

**Owner's app "Blazingly Fast IDE":** not running before (18:31, `$S/dmrun3/before.txt`) and not running after (19:21).

**Left under the base:**

| Folder                                                                                                 | Size                                                       |
| ------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------- |
| `evidence` (both runs, 12 captures each plus crops, `RESULTS.md`, `project.stamp`, `project-writes-*`) | 113 MiB; the captures are 8.8 MiB native and 3.2 MiB Tauri |
| `smoke` (with `runs3-attempt1`)                                                                        | 25 MiB                                                     |
| `pilot`                                                                                                | 150 MiB                                                    |
| `isoproof`                                                                                             | 868 KiB                                                    |
| `src`, `project`                                                                                       | Unchanged; the clone has no `.openidea`                    |

Free space: 317 GiB.

**Files in `$S/dmrun3`:**

- `scripts/`, `scripts.diff`, `scripts.sha256`
- `tests/`
- `smoke.sh` and the `smoke-*`, `evidence*` and `fix*` outputs
- `pilot_attempt2_recomputed.md`, `pilot_cpu_compare.md`
- `evidence_RESULTS_copy_numbers_only.md`
- `before.txt`, `final-state*.txt`, `log.md`
