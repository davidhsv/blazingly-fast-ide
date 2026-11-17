Both release binaries are built from 18b54813b, the static identity gate passes, the clone is in place, and `preflight.sh` reports GO. No app was started, no input was posted and the network was not used. Two pointers inside the clone still lead out of the base (section 5) and need a decision before the pilot. At 16:18 someone other than me edited the clone and started a `caffeinate`; who did it is not established.

## 1. Base folder, commit, gates

**Base:** `/private/tmp/openidea-measure-i3NdVc` (path also saved in `$S/dmrun/base.txt`).

**Commit built:** `18b54813b5052c2a9df9bb27e5b9dde04b87c09b`. The worktree HEAD had moved on to `917d636db`, which adds one line to `docs/gpui-port/notes/backlog.md` (`git diff --stat`), so my copy of `build.sh` pins the commit with `BUILD_COMMIT`. `config.json` and `src/COMMIT` record 18b54813b. The lock files in the export have the same sha256 as the commit's blobs (`Cargo.lock` 8cf5fd8a…, `app-gpui/Cargo.lock` abd4237c…, `pnpm-lock.yaml` dd923ac7…).

The main log is `$S/dmrun/log.md`.

| Gate                                  | Result           | Evidence                                                                                                                                                          |
| ------------------------------------- | ---------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| G0                                    | pass             | Details below.                                                                                                                                                    |
| G2 (static part)                      | pass             | Details below.                                                                                                                                                    |
| G3                                    | pass             | `preflight.txt` line 5: no process from the base. Our single-instance socket is absent (`ls /private/tmp` shows only the older `ai_l0ng_openidea_bench_si.sock`). |
| G4                                    | pass             | `preflight.txt` line 6: the owner's app is not running (`ps` count 0, again at the end). `find src -name '*.app'` finds nothing.                                  |
| G5                                    | not mine to pass | No other cargo or rustc is running. The owner's Bazel and Java processes are running and the load average was 15–17 during the builds.                            |
| G6                                    | pass now         | `preflight.txt` lines 7–11: unlocked, display awake, Accessibility and Screen Recording true. This must be re-checked at the next launch.                         |
| G7                                    | pass (present)   | `preflight.txt` lines 12–13: both SPIs are present. Hypothesis: the disclaim spawn itself works; it has never been exercised.                                     |
| G8                                    | pass             | 347 GiB free before building (`build.log` lines 2–3), 341 GiB before the native build (line 15), 335 GiB at the end (line 19), 336 GiB at preflight (line 17).    |
| G1, G9, G10, G11, run-time part of G2 | not done         | These need a launch.                                                                                                                                              |

**G0 details**

- `macos.py session` runs and reports the session, permissions and frontmost pid (`$S/dmrun/g0_session.txt`). It printed `frontmost: null` until I fixed it (section 2).
- `macos.py windows` was run on the pid of the Claude app this session runs in: 16 windows, and the main window's title "Claude" was readable (`g0_windows_11124.json`).
- `bsdinfo`, `rusage`, `responsible_pid` and `snapshot` were called on my own shell, read only (`g0_probes.txt`). The footprint word matched `footprint -p`, and lifetime max ≥ footprint.
- `build.sh --dry-run` passed with the original (`g0_build_dry_original.txt`) and with my copy (`g0_build_dry.txt`).
- `measure.py --dry-run` passed for both apps (`g0_measure_native_dry.txt`, `g0_measure_tauri_dry.txt`, rc 0).
- `run_all.sh --dry-run --reps 1` passed (`g0_run_all_dry.txt`, rc 0), as did `stop.py --dry-run` and `aggregate.py --dry-run`. `sh -n` passes on all three shell scripts.

**G2 (static) details**

- Neither binary embeds an Info.plist: `launchctl plist` says the Mach-O "does not have a __TEXT,__info_plist" (`tauri-info-plist.txt`; `build.log` line 20). So the "plist does not name the owner" half of the gate checked nothing.
- `ai.l0ng.openidea.measure` appears once in the Tauri binary (`tauri-identifier-count.txt`), and "OpenIDEA Measure" appears once.
- The other `ai.l0ng.openidea` strings in the binary (`tauri-other-identifier-strings.txt`) are the two hardcoded keychain prefixes the design expects. Read: `crates/ide-ssh/src/secrets.rs:25` has `const SERVICE: &str = "ai.l0ng.openidea"`, and `src-tauri/src/commands/plugin.rs:783` builds `ai.l0ng.openidea.plugin.{plugin}`.
- The native binary's only identifier strings are `ai.l0ng.openidea.gpui` and the same keychain prefix.
- The final proof of identity is still the run-time socket name, in the next launch.
- `lockdiff.txt`: 975 and 1192 packages, 888 shared, 98 shared at different versions, 39 `ide-*` crates shared.

## 2. Changes to script copies

The copies are in `$S/dmrun/scripts/`; the originals are untouched. The full diff is `$S/dmrun/scripts.diff` (3 files, +65/−20). No safety gate was weakened.

- **`macos.py`, `frontmost_pid`:** `lsappinfo front` prints the ASN as `ASN:0x0-1a6d06b6:`, and `lsappinfo info` prints nothing for that form (measured). So `frontmost_pid()` always returned None, and the frontmost guard would have aborted every key. The copy rewrites the ASN to `ASN:0x0-0x1a6d06b6:` before the lookup, which then returned the right pid ("Slack", 44802). The guard still requires front == our pid.
- **`build.sh`:**
  - It builds from `BUILD_COMMIT` (default HEAD), for the commit mismatch above.
  - The pnpm on PATH is 9.15.9, which refuses this `pnpm-workspace.yaml` ("packages field missing or empty"). The worktree's `node_modules/.modules.yaml` says pnpm 10.34.9 installed it. The copy puts a shim `$S/dmrun/bin/pnpm` first on PATH; the shim runs node on the cached `~/.cache/node/corepack/v1/pnpm/10.34.9/bin/pnpm.cjs`. Tauri's `beforeBuildCommand` (`pnpm build`) finds the same shim.
  - It passes `--store-dir /Users/davidvieira/.pnpm-store`. From `/private/tmp`, pnpm would otherwise pick an empty `/private/tmp/.pnpm-store`, and the offline install would fail as if it wanted the network.
  - Build output is redirected to the log files instead of piped into `tee`, whose exit status hid a failed build from `set -e`.
  - It writes `df -k /System/Volumes/Data | tail -1` before each build, requires 80 GiB before each build and 40 GiB after the native one, and checks free space on the Data volume.
  - The identity gate is stricter: it also refuses the XML form `>ai.l0ng.openidea<`, uses `strings -a`, saves every non-measure identifier string from both binaries, and writes `binaries-ls.txt`.
- **`measure.py`, `select_files`:**
  - The original walk picked only 5 files, all under the gitignored `.claude/worktrees/`. The clone holds 6 nested worktrees, which duplicate every main-tree basename, so no main-tree file counted as unique.
  - The copy builds its list from `git ls-files --cached --others --exclude-standard` (with `GIT_OPTIONAL_LOCKS=0` and `HOME=/nonexistent`). That gives 10 files, 20–58 KB, all under `<one test source folder of the project>` (`$S/dmrun/select_files.txt`, `<base>/files.json`).
  - Read: `crates/ide-index/src/filter.rs:1-5` leaves gitignored directories out of the index. Hypothesis: Go to File in both apps lists only indexed files, so the 10 names are unique there. Copies of them exist in the ignored worktrees, and the title check could not tell those apart.
  - The clone has 15,810 non-ignored files; the pilot should compare that with the `indexed N files` log line.
- **New helper files:**
  - `$S/dmrun/clone.sh`: checks the clone is on the same volume, runs `cp -cR`, captures its stderr and rc, and moves `.openidea` aside to the name `run_all.sh` uses.
  - `$S/dmrun/bin/pnpm`: the pnpm 10 shim above.

## 3. Builds

The command, run in the background, was `cd <base> && BUILD_COMMIT=18b54813b /bin/sh $S/dmrun/scripts/build.sh <base>`. It returned rc 0 in 4,705 s wall (`$S/dmrun/build.log` line 25).

The Tauri and native builds ran one after the other. The first minutes of the Tauri build overlapped the clone, and the machine was under the owner's load (load average 15–17). The times below are not clean timings.

- **pnpm:** `CI=1 pnpm install --offline --frozen-lockfile --store-dir …` finished in 2.5 s with "335 reused, downloaded 0" (`<base>/pnpm-install.log`).
- **Tauri:** `CARGO_NET_OFFLINE=true pnpm tauri build --no-bundle --config <base>/measure.tauri.conf.json -- --locked` took 1,857 s, of which cargo took 30m 17s (`build-tauri.log`).
  - `ps` showed `tauri.js` receiving `-- --locked`, and the lock files are unchanged after the build. Hypothesis: `--locked` reached cargo; cargo's own argv was not captured.
  - The binary is `src/target/release/openidea`, 211,695,808 bytes, sha256 `fa832bbb…`.
- **Native:** `cd src/app-gpui && CARGO_NET_OFFLINE=true cargo build --release --locked --bin openidea-gpui` took 2,837 s (cargo: 47m 16s, fat LTO as shipped).
  - The binary is `src/app-gpui/target/release/openidea-gpui`, 200,333,088 bytes, sha256 `87424926…`.
- **Sizes:** `src/target` 5.6 GiB, `app-gpui/target` 4.6 GiB, `node_modules` 446 MiB (`disk.txt`).
- **Network:** nothing asked for it. Both build logs have 0 lines matching Downloading, Downloaded, Updating or Blocking.
- **Reads and writes under your home that the build could not avoid:**
  - the cargo registry and git cache in `~/.cargo`;
  - the pnpm store `~/.pnpm-store/v10`;
  - the corepack cache;
  - nvm's node 24.12.0.

  Hypothesis: pnpm also recorded the project in its store's `projects` folder.

## 4. Clone and the original

**Clone:**

- `cp -cR` returned rc 0 in 347 s with 0 stderr lines (`$S/dmrun/clone.log`).
- 1,201,130 regular files and 1,418,723 entries.
- `du` gives 16 GiB, which counts cloned blocks in full. Free space fell from 364,418,468 to 360,795,856 KiB during the clone, while the Tauri build was also writing.
- The clone's `.openidea` (85 MiB, 3 entries) is at `<base>/project/openidea-of-the-owner`.

**Original `/Users/davidvieira/code/Allocadia`:** read with `git --no-optional-locks -c core.fsmonitor=false -c core.untrackedCache=false status --porcelain` and `stat` of the top-level entries.

|                                     | Before (14:56:38)   | After (16:17:28)    |
| ----------------------------------- | ------------------- | ------------------- |
| Lines from `git status --porcelain` | 10                  | 10                  |
| `.git/index` mtime                  | 2026-11-10 09:29:59 | 2026-11-10 09:29:59 |
| Newest top-level mtime              | `.git`, 14:38:49    | `.git`, 16:05:23    |

The status and the index match, but the original's `.git` folder mtime moved. My log has no command touching the original at 16:05; all I ran were the read-only `status` and `stat` above. Hypothesis: the owner's own git or Bazel activity.

## 5. Must be fixed or decided before the pilot

- **Someone else edited the clone.** At 16:18:42, something other than me:
  - renamed the six nested worktrees' `.git` files to `.git.moved-aside` (each had pointed to `gitdir: /Users/davidvieira/code/Allocadia/.git/worktrees/…`, the original's `.git`);
  - moved the clone's `.git/worktrees` to `<base>/project/git-worktrees-of-the-owner`;
  - wrote `<base>/project/dotgit_files.txt`.

  Hypothesis: the manager or another agent. Please confirm. As it stands, this removes the pointers from the clone into the original's `.git`.

- **Absolute symlinks still lead out of the base:** 18 in total (`$S/dmrun/clone_abs_symlinks.txt`).
  - 16 go into `/private/var/tmp/_bazel_davidvieira/…`: the `bazel-*` links at the clone root and in the nested worktrees, which are the owner's live Bazel output.
  - `.claude/ticket-workflow` points to `/Users/davidvieira/.claude/ticket-workflow`.
  - `tools/maven/pnpm-parent/target/node/pnpm` points into the original Allocadia.

  `--project-check` (`find -newer`) does not follow symlinks, so it would miss writes through them. Hypothesis: the indexers skip them because `bazel-*` and the others are gitignored. Decision: remove these links from the clone, or accept them. I changed nothing.

- **WebKit storage location (R4 / G10):** with no embedded Info.plist, it stays Hypothesis. It may be keyed by the process name `openidea` under the fake or real `~/Library/WebKit`. G10 will tell.
- **Untested until a launch:** `spawn` with disclaim and all key posting.

## 6. What is ready, and what the next launch must do first

**Ready:** the two binaries, `config.json`, `files.json`, the clone, the fixed scripts in `$S/dmrun/scripts/` (use these, not the originals) and the pnpm shim. `preflight.sh` reports GO (`$S/dmrun/preflight.txt` line 18, rc 0).

**Next launch, in order:**

1. Re-run `$S/dmrun/scripts/preflight.sh <base>`.
2. Settle the symlink decision and the 16:18 change above.
3. Run-time isolation proof:
   - List `/Applications/Blazingly Fast IDE.app/` processes by path (read-only).
   - Start `src/target/release/openidea` once, with the isolated environment from `measure.py`'s `prepare` and no project.
   - Check that it stays up as its own process, that `/private/tmp/ai_l0ng_openidea_measure_si.sock` appears and is held by our pid, and that the config, data, log and cache folders and the fake home's `Library` entries are under the base or named after the measure identifier.
   - List the owner's processes again, then stop ours by its recorded pid.
4. Pilot: `$S/dmrun/scripts/run_all.sh <base> --reps 1`. Hypothesis: about 1–1.5 h, by the design's estimate.

## 7. Left under the base, and what is running

- `src`: 11 GiB.
- `project`: 16 GiB apparent. It holds the clone, `openidea-of-the-owner` (85 MiB), `git-worktrees-of-the-owner` and `dotgit_files.txt`.
- Logs, gate files, `config.json`, `files.json`, `lockdiff.txt`, `binaries.sha256` and `measure.tauri.conf.json`.

Nothing of mine is running. `ps` shows no process from the base or from `$S/dmrun`, and no cargo, rustc or pnpm. The monitor expired and the wait loops were stopped or exited.

One `caffeinate` is not mine: pid 60633 (`caffeinate -d -i -t 28800`, started 16:18:00) and its parent zsh 60632, whose parent is the Claude CLI process 11679. I never ran that command and did not signal either process. Hypothesis: the manager started it.
