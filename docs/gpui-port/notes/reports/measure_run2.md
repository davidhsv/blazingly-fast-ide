The isolation proof and the pilot both completed: in the final pilot run (attempt 2) both apps ran all seven scenarios with status `ok`. But some scenarios have no evidence they did anything, and the Tauri idle figure is not comparable with native. Six things must be decided or fixed before the campaign (section 5).

It took one re-run of the isolation proof, four smokes and one re-run of the pilot. Every change to the script copies is in section 4. `$S` = `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad` and `<base>` = `/private/tmp/openidea-measure-i3NdVc`. My log is `$S/dmrun2/log.md`, and the full diff of my script copies against `$S/dmrun/scripts/` is `$S/dmrun2/scripts.diff`.

**Pilot attempts.** Pilot attempt 1 stopped after the native run on a G11 write (section 1), and its folder is `<base>/pilot/attempt1/`. Attempt 2 is the complete pilot, in `<base>/pilot/attempt2/`; that move is a rename inside the base. All pilot paths below are under `<base>/pilot/attempt2/runs/`.

**Machine load.** Load average was 3 to 6 throughout, with the owner's java, Docker's virtual machine, Slack, Claude and `fileproviderd` at the top (`$S/dmrun2/load-pilot.txt`, `load-pilot-attempt1.txt`, `load-before-smoke.txt`). No cargo, rustc, node or pnpm was running at any check.

## 1. Gates G3 to G11

| Gate | Result                                    | Evidence                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| ---- | ----------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| G3   | pass, with a defect in the check          | `$S/dmrun2/pilot.out` line 16: "no process from the base is running". The "socket not held" half checks nothing: `lsof -t <socket>` returned no holder while our app held the socket. `lsof -a -p <app> -U` showed it on fd 11 (`$S/dmrun2/isoproof.out`, record `sockets_during`).                                                                                                                                                                                                                                                                                            |
| G4   | pass                                      | The owner's app was not running at any check: `$S/dmrun2/step1.txt` at 16:28, `pilot.out` line 17, `owner_app_pids: []` in every `run` event, and the final `ps` at 18:14. No `.app` was launched or built.                                                                                                                                                                                                                                                                                                                                                                    |
| G5   | pass for the pilot                        | Load 3–6, no builds running. The owner's background work was left alone.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| G6   | pass                                      | `pilot.out` lines 18–22: unlocked, display awake, Accessibility and Screen Recording true. These held from a background command too. The sampler checks lock and sleep every second, and no run aborted on them.                                                                                                                                                                                                                                                                                                                                                               |
| G7   | pass, after a script fix                  | Both SPIs are present and the disclaimed spawn works (`disclaimed: true`). Measured: the app is responsible for itself, not for the launcher. Its WebKit helpers are attributed to the app pid: `member` events with `webkit:` kinds in `1-tauri/events.jsonl` lines 9–11 and 28–30.                                                                                                                                                                                                                                                                                           |
| G8   | pass                                      | `pilot.out` line 28: 338 GiB free. 354,177,192 KiB free at the end.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| G9   | pass on its listed criteria, with caveats | No `abort`, `file_not_opened`, `mode_not_switched`, `unreadable` or `timeout` events, no network lines, titles readable, `status.json` `ok` for both runs. The caveats are in section 3.                                                                                                                                                                                                                                                                                                                                                                                       |
| G10  | pass                                      | Every Tauri launch shows `fakehome/Library/WebKit/openidea` and `Library/Caches/openidea` under the fake home (`1-tauri/events.jsonl` lines 18 and 608). Nothing is named `ai.l0ng.openidea`. Hypothesis: the six real-home names stayed unchanged during the pilot; I stat'ed them only around the isolation proof, as the brief allows.                                                                                                                                                                                                                                      |
| G11  | fail in attempt 1, pass in attempt 2      | Attempt 1 (`<base>/pilot/attempt1/project-writes-1794947322.txt`): `.git` and `.git/index` were rewritten at 17:11:22, 0.7 s after the native project launch at 17:11:21.49, despite `GIT_OPTIONAL_LOCKS=0`. `.openidea/{.gitignore,changelists.json}` was also written. Attempt 2 (`project-writes-1794948748.txt` native, `-1794949942.txt` Tauri): only the 3 `.openidea` entries, and the index sha256 is unchanged (36bafa86…) at both stamps and checks. No path under `/Users/davidvieira/code/Allocadia` appears in any log, event file or project-writes file (grep). |

**Why I continued after G11 failed.** The design lists the index refresh as an expected write; only its prevention failed. The brief's named no-go for `--project-check` is a path under the original, and none appeared. So I re-ran the whole pilot (both apps) instead of stopping; you can overrule this. Before re-running I moved the native-written `.openidea` into the run folder.

- **Order effect.** Only attempt 1's native `open_project` paid for the index refresh. Hypothesis: it was a one-time cost of the first open after cloning.
- **Who wrote the index** is a Hypothesis. No git child was recorded in that run, but the 1 Hz sampler can miss a short-lived one.

## 2. Isolation proof (Tauri, no project)

Raw output is `$S/dmrun2/isoproof.out` and `<base>/isoproof/proof.jsonl`; the helper script is `$S/dmrun2/isoproof.py`. It reuses `measure.py`'s `prepare`, `Launch`, `Guard` and `Sampler`.

**The first attempt was aborted by our own check.** The script's responsibility check expected the launcher (16880) to be responsible for the app, but the app (16881) was responsible for itself. The app was SIGTERMed 0.1 s after start and was gone; that attempt is kept in `<base>/isoproof/attempt1/`. I judged this a script defect, fixed it, and ran the proof once more. Every isolation item below comes from that second run; you can overrule this.

- **Stays up as its own process:** pid 17110 was checked every second, 20 of 20 alive, same start time (`alive_20s`). Its window was on screen after 0.35 s.
- **Single-instance socket:** `/private/tmp/ai_l0ng_openidea_measure_si.sock` appeared and is held by pid 17110 (fd 11, from `lsof -a -p 17110 -U`). `ai_l0ng_openidea_si.sock` was absent before, during and after. The stale socket left after the stop (no holder) was removed.
- **Folders:**
  - The stdout log names the data directory `<base>/isoproof/run/tauri/data/ai.l0ng.openidea.measure`.
  - Created under the run: `tauri/{config,data,local,logs}/ai.l0ng.openidea.measure/`, `fakehome/Library/WebKit/openidea/WebsiteData/` and `fakehome/Library/Caches/openidea/WebKit/`.
  - The CLI socket in the fake home was not created: the path is longer than SUN_LEN, and the app logged a WARN about it.
- **The six real-home names**, stat'ed only, before and after:
  - `~/Library/WebKit/openidea` and `~/Library/Caches/openidea` exist with mtime 1794534096 (12 Nov) and did not change.
  - The other four do not exist.
  - `changed: []`.
- **Open files under the real home:** only four `~/Library/Fonts/HackNerdFontPropo-*.ttf` files (read-only font loading, as the design expected).
- **Network:** `lsof -nP -a -p <set> -i` shows one line, `TCP 127.0.0.1:63531 (LISTEN)`. Nothing outside.
- **WebKit helpers:** GPU, Networking and WebContent were attributed through responsible pid = app.
- **Owner's processes:** none before and none after (`same: true`).
- **Stop:** SIGTERM by the recorded pid, after which all 5 pids (launcher, app, 3 helpers) were gone. `ps` shows nothing under the base.

## 3. The pilot

The pilot is one run per app. These are not results.

**Evidence that each scenario happened.** File references are to `events.jsonl` (ev) and `stdout-project.log` (log) of each run.

| Scenario                | Native                                                                                                    | Tauri                                                                                                                                                                                                                                                              |
| ----------------------- | --------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| S1 cold start           | Window on screen, title "OpenIDEA", 1536×1079 (ev 16)                                                     | Window "Blazingly Fast IDE", 1440×900 (ev 19). Hypothesis: the frontend's own title; it is not the identifier.                                                                                                                                                     |
| S2 open project         | Log line 10: `indexed 15822 files … in 2.15s`, and the "opened … in window main" line (ev 24–25)          | Log line 7: `indexed 15822 files … in 2.43s` (ev 27, 31). Stage 1 counted 15,810 non-ignored files.                                                                                                                                                                |
| S3 10 files             | 10 `file_opened` events, each title `Allocadia – <Name>.java` (ev 35…485)                                 | 10 `file_opened` events with the same titles (ev 41…493). Accessibility: "Editor for <path>" focused after each scroll.                                                                                                                                            |
| Scrolling inside files  | No direct evidence                                                                                        | No direct evidence                                                                                                                                                                                                                                                 |
| S4 Find in Files        | No evidence it ran: Accessibility shows only the window (ev 535); CPU 1.0 s                               | Accessibility focus is the "Text to find" field with value `import java.util` (ev 544, 556)                                                                                                                                                                        |
| S5 Git log              | No evidence it ran (ev 554, CPU only)                                                                     | After Cmd+9 the focus is the "Filter branches and tags" text field (ev 563, 585). So the 20 PageDowns went to a text field. Hypothesis: the log list did not scroll.                                                                                               |
| S6 Blaze → Deep → Blaze | `settings.toml says mode = "full"` (ev 583), back (ev 589). Deep mode spawned a `java` child, openjdk 26. | Same, ev 591 and 599. Deep mode spawned `clangd` (`/Library/Developer/CommandLineTools/usr/bin/clangd`) and `java`. Tauri log: `mvn … help:effective-pom` ran offline and failed ("Cannot access central … in offline mode", in `tauri/local/…/build/maven.json`). |
| S7 idle                 | 601 s of samples, frontmost at stop                                                                       | 599 s, but **partly not frontmost** (see below)                                                                                                                                                                                                                    |

**Every event logged:**

- **Native:** 477 `posted`, 56 `waited`, 19 `scenario`, 18 `windows`, 10 `file_opened`, 5 `member`, 2 `focus_not_handed_back`, 2 `stopped` (no findings), 1 `end ok`.
- **Tauri:** 15 `member` events; children were `git` ×3 during the open-files scenario, plus `clangd` and `java`.
- **No** timeouts, aborts, warnings, `unreadable_member` or `member_exited_before_read` events.
- **Network:** `network: []` in all four stops. This only means no non-loopback socket was seen at 10 s polling. Traffic to the dead proxy is loopback and would not show; no "refused" or proxy lines appear in any stdout or Tauri log.
- **`lifetime_max_word_ok: false`** in both project launches. Word 30 lagged the current footprint 31 times, so the "sum of lifetime max" upper bound is unreliable.

**Tauri lost frontmost during idle.**

- Its project stop has no `focus_not_handed_back` event, and the Accessibility focus at idle end is empty (ev 604).
- In 30 s buckets, the app plus WebKit sat at about 7% of a core and 320 wakeups/s until 18:04:52. It then dropped to 0.2% and 93 wakeups/s, had bursts between 18:06:52 and 18:08:52, then went quiet.
- Who took the focus is a Hypothesis (the owner's activity or a notification). Native stayed frontmost at about 0.2% the whole time.
- So the Tauri idle row mixes the two states.

**Accessibility.** The native log shows "Accessibility activated" already in smoke 1, before my probe existed. Hypothesis: another accessibility client on this machine queries the apps. The pilot ran with `--ax-probe` on.

**Raw numbers, one run each, not results.**

- Footprint in MiB, split app / WebKit / child. The "app" column includes the `/usr/bin/time` launcher, under 1 MiB.
- CPU in s; idle percent is of one core.
- The sampler matched `footprint -p` exactly: 787 MB summed at Tauri idle (`tools-project-idle.txt`).
- The durations are set by the driver: every settle returned at its minimum hold (2, 3, 5 or 10 s).

| Scenario      | App                          | Duration (s) | Peak total (app/WK/child) | Steady total (app/WK/child) | CPU s (app/WK/child) | % core | Wakeups/s |
| ------------- | ---------------------------- | ------------ | ------------------------- | --------------------------- | -------------------- | ------ | --------- |
| cold_start    | native                       | 6            | 126 (126/0/0)             | 124 (124/0/0)               | 0.3 (0.3/0/0)        | 4.7    | 87        |
| cold_start    | tauri                        | 7            | 275 (45/231/0)            | 186 (42/143/0)              | 1.2 (0.5/0.8/0)      | 17.4   | 180       |
| cold_steady   | native                       | 29           | 124                       | 117 (117/0/0)               | 0.1                  | 0.2    | 83        |
| cold_steady   | tauri                        | 29           | 189 (43/146/0)            | 174 (42/132/0)              | 0.4 (0.2/0.3/0)      | 1.5    | 108       |
| open_project  | native                       | 43           | 247                       | 237                         | 14.8                 | 34.2   | 87        |
| open_project  | tauri                        | 49           | 495 (174/379/0)           | 370 (172/198/0)             | 19.2 (17.4/1.8/0)    | 39.1   | 135       |
| open_files    | native                       | 160          | 272                       | 258                         | 6.2                  | 3.9    | 121       |
| open_files    | tauri                        | 191          | 994 (246/761/38)          | 749 (245/504/0)             | 42.8 (9.7/32.8/0.3)  | 22.5   | 371       |
| find_in_files | native                       | 17           | 279                       | 276                         | 1.0                  | 5.6    | 110       |
| find_in_files | tauri                        | 17           | 870 (246/624/0)           | 818 (245/573/0)             | 2.2 (0.9/1.3/0)      | 13.2   | 315       |
| git_log       | native                       | 17           | 311                       | 307                         | 1.9                  | 11.0   | 108       |
| git_log       | tauri                        | 16           | 841 (252/590/0)           | 756 (251/505/0)             | 3.4 (2.1/1.3/0)      | 21.2   | 295       |
| deep_mode     | native                       | 70           | 338 (338/0/18)            | 301                         | 0.4                  | 0.5    | 91        |
| deep_mode     | tauri                        | 73           | 958 (262/701/108)         | 860 (262/599/0)             | 9.1 (2.4/4.7/2.0)    | 12.5   | 328       |
| blaze_again   | native                       | 70           | 352                       | 344                         | 0.2                  | 0.2    | 89        |
| blaze_again   | tauri                        | 70           | 912 (264/649/0)           | 868 (262/606/0)             | 5.5 (1.5/4.0/0)      | 7.8    | 320       |
| idle          | native                       | 601          | 372                       | 372                         | 1.3                  | 0.2    | 85        |
| idle          | tauri (partly not frontmost) | 599          | 987 (272/719/0)           | 787 (270/517/0)             | 25.6 (9.3/16.2/0)    | 4.3    | 197       |

**Other timings.**

- Wall time per run: native 17:34:16 → 17:52:05 (about 17.8 min); Tauri 17:53:29 → 18:11:57 (about 18.5 min).
- `run_all.sh` took 39.4 min from start to exit.
- `/usr/bin/time -l`, app process alone, project launch: native 389 MB peak footprint and 20.5 s user; Tauri 344 MB and 32.8 s user.
- These numbers come from `$S/dmrun2/analyze.py`, saved in `$S/dmrun2/pilot_numbers.md`. It counts a process's first reading from 0, so its CPU figures are higher than `RESULTS.md`'s (section 5). `RESULTS.md` for the pilot is in `<base>/pilot/attempt2/`.

## 4. Changes to the script copies

The copies are in `$S/dmrun2/scripts/`. No safety gate was weakened, and nothing treats the two apps differently.

**`run_all.sh`**

- It never re-clones from `/Users/davidvieira/code/Allocadia`. The original branch would have read the original and brought back the six `.git` pointers and the 18 outside symlinks.
- If the clone is missing, it refuses. If a run writes outside `.openidea`, it runs `stop.py` and exits 4.
- It passes `--ax-probe` through to `measure.py`.

**`measure.py`**

- **Focus hand-back removed.** `hand_focus_back` no longer uses osascript: the hard rule forbids focusing a process I did not start, and an uncaught timeout there would also have skipped the SIGTERM. It now logs `focus_not_handed_back` and warns only if the owner's app runs. This is your decision for the campaign.
- **New arguments** (defaults unchanged): `--project`, `--files-json`, `--runs-root` (all must be under the base), `--scenarios`, `--idle-s` and `--ax-probe`.
- **Responsibility.** It accepts responsible pid = the launcher or the app, and the sampler matches helpers the same way. Measured: app-responsible, helpers then attributed.
- **`cpu_rate` coverage bug.** The samples inside an N-second window span only N−1 s, so windows of 2–3 s never settled; every one timed out in smoke 1. It now includes the sample just before the window.
- **`settle` timing.** `hold_s` must now elapse after the wait begins. In smoke 2 the settles returned in 0.004 s because the quiet seconds before the key press counted.
- **Settle threshold.** After an editor is open it is 10% of a core for both apps (`EDITOR_SETTLE`; it was 3% and 5%). Measured: the Tauri set rests at about 7% with an editor focused, so those waits would always run to their ceilings.
- **Evidence events:** `file_opened` (the title), `windows` (titles, plus the Accessibility probe if on), `member` (the path of each new process), `unreadable_member` and `member_exited_before_read`.
- **Sampler re-read.** Before calling a member unreadable, it re-reads `bsdinfo`. Smoke 2's Tauri run aborted on two unreadable children. Hypothesis: they had exited between the snapshot and the read. I tested the zombie hypothesis and refuted it. The race fix was tested with a reaped fork; it never fired in the pilot.
- **Project check.** `.git` and `.git/index` form a third class: listed, not fatal. The index sha256 is logged at the stamp and at the check.

**`macos.py`**

- `ax_focused`: a read-only Accessibility read of the focused element. Values over 64 characters (file text) are not kept.
- I ran one test read on pid 7667 (the Claude app, which I did not start). It was read-only: no input, no focus.

**Pilot command as finally run:** `cd /private/tmp/openidea-measure-i3NdVc && /bin/sh $S/dmrun2/scripts/run_all.sh /private/tmp/openidea-measure-i3NdVc --reps 1 --ax-probe > $S/dmrun2/pilot.out 2>&1; echo $? > $S/dmrun2/pilot.rc` (rc 0). `--allow-activate` was never passed. The smokes are in `<base>/smoke/runs-attempt{1..4}` and `$S/dmrun2/smoke-attempt*.out`.

## 5. Before the campaign

**Fix:**

1. The sampler should record the frontmost pid, and flag any idle window in which our app was not frontmost. This is the Tauri idle problem above.
2. The `lsof -t <socket>` holder check in `preflight.sh` and `run_all.sh` should use `lsof -U` on our pids instead.
3. `aggregate.py`'s CPU per scenario drops the first interval of each scenario. Also, the Escape after Find and the Cmd+9 that hides the Git log run inside the `tools_*` window, so their work is in no scenario.
4. The Git log scenario: in Tauri, PageDown goes to the branch filter field, not the log list. Neither app gives evidence for Find or scrolling natively. Changing keys for one app is not allowed, so decide whether S4/S5 stay as CPU-only.

**Decide:**

- Whether focus hand-back stays off.
- `--ax-probe` on or off. The smoke showed no visible cost (Tauri steady 190 vs 192 MiB, same CPU), but on a tiny project.
- C1 no longer holds: Tauri's Deep mode started `clangd`, native did not.
- The settle margin: Tauri rests at 7% against a 10% threshold under load.
- Window sizes (C7: 1536×1079 vs 1440×900).
- Repetition 1 and the index-refresh order effect.
- The accessibility client on the machine.

**Commands.** Move or keep `<base>/pilot` first; `runs/` is now empty, so the campaign's run folders cannot be confused with the pilot's.

- With the probe: `cd /private/tmp/openidea-measure-i3NdVc && /bin/sh $S/dmrun2/scripts/run_all.sh /private/tmp/openidea-measure-i3NdVc --reps 3 --ax-probe > $S/dmrun2/campaign.out 2>&1; echo $? > $S/dmrun2/campaign.rc`
- Without it: the same command without `--ax-probe`.
- Estimate: about 2 h (3 × 39.4 min). Hypothesis: each settle that times out adds up to 60 s (up to 600 s for the Deep-mode waits).

## 6. What is left, and the final state

**Left under the base:**

- `src` 11 GiB and `project` 16 GiB apparent (APFS clone, now without `.openidea`).
- `pilot` 150 MiB, `smoke` 17 MiB, `isoproof` 868 KiB, and the stage 1 files.
- Free space: 354,177,192 KiB.

**Nothing of mine is running:**

- `stop.py --base <base>` printed "nothing of ours is running".
- `ps -axo pid=,command= | grep -F "<base>/"` printed nothing.
- `ps -p` on all 52 recorded pids returned none.
- `run_all.sh`'s caffeinate processes (30724, 42534) are gone. The remaining caffeinate processes are 60633 (yours) and 51629 and 53718 (the owner's), all untouched.
- `/private/tmp/ai_l0ng_openidea_measure_si.sock` is absent; only the old `ai_l0ng_openidea_bench_si.sock` remains.
- No wait loop is running.

**Owner's app process list:** empty before (16:28) and empty after (18:14).
