#!/usr/bin/python3 -I
"""One measured run of one app: launch A (cold start, Welcome screen) and launch B (the project
given on the command line, then the scenarios), sampled at 1 Hz, stopped by PID.

usage:
  measure.py --base B --app native|tauri --rep N [--dry-run] [--allow-activate]
  measure.py --base B --select-files            (once per campaign, writes B/files.json)
  measure.py --base B --project-stamp | --project-check

B must be /private/tmp/openidea-measure-<random>; B/config.json (written by build.sh) names the
binaries and the project clone. Everything the run writes is under B/runs/<rep>-<app>/.
"""
from __future__ import annotations

import argparse
import ctypes
import json
import os
import re
import signal
import subprocess
import sys
import threading
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import aggregate  # noqa: E402  (the one definition of a scenario's CPU, memory and wakeups)
import macos  # noqa: E402

BASE_RE = re.compile(r"^/private/tmp/openidea-measure-[A-Za-z0-9]{6,}$")
OWNER_BUNDLE = "/Applications/Blazingly Fast IDE.app/"
MEASURE_ID = "ai.l0ng.openidea.measure"
ANSI = re.compile(r"\x1b\[[0-9;]*m")
SKIP_DIRS = {".git", "node_modules", ".idea", ".openidea", "target", "build", "out", "dist", ".gradle", ".mvn"}


# Settle threshold once an editor is open, the same for both apps. Measured in the smoke: the Tauri
# set rests at about 7% of a core with an editor focused (app 2%, WebKit GPU 2%, WebContent 3%),
# so the earlier 3% and 5% were never met and every such wait ran to its ceiling.
EDITOR_SETTLE = 0.10
SCENARIO_GROUPS = ["cold", "open_files", "find_in_files", "git_log", "mode", "idle"]


class Abort(Exception):
    pass


def now() -> float:
    return time.time()


# ------------------------------------------------------------------ setup of one run

def settings_toml() -> str:
    return (
        "[general]\nreopenLastProject = false\nconfirmExit = false\nautoCheckUpdates = false\n"
        "showWhatsNewAfterUpdate = false\n\n[ide]\nmode = \"light\"\n\n[vcs.git]\nincomingCheck = \"never\"\n"
    )


def write(path: str, text: str, dry: bool):
    if dry:
        print(f"[dry-run] write {path}:\n    " + text.replace("\n", "\n    "))
        return
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8") as f:
        f.write(text)


def prepare(run: str, app: str, project: str, dry: bool) -> dict:
    """Fresh homes and the environment of the launched app (env -i: nothing else is inherited)."""
    fake = f"{run}/fakehome"
    for d in (f"{fake}/Library/Caches", f"{run}/tmp", f"{run}/no-market"):
        if dry:
            print(f"[dry-run] mkdir -p {d}")
        else:
            os.makedirs(d, exist_ok=True)
    write(f"{fake}/.m2/settings.xml", "<settings><offline>true</offline></settings>\n", dry)
    trust = json.dumps({"projects": [project], "locations": [], "restricted": []})
    market = json.dumps({"market_url": f"{run}/no-market/index.json"})
    dead = "http://127.0.0.1:9"
    env = {
        "HOME": fake, "CFFIXED_USER_HOME": fake, "TMPDIR": f"{run}/tmp/", "USER": os.environ.get("USER", "measure"),
        "LOGNAME": os.environ.get("USER", "measure"), "SHELL": "/bin/sh", "LANG": "en_US.UTF-8",
        "PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "RUST_LOG": "info", "NO_COLOR": "1",
        "GIT_OPTIONAL_LOCKS": "0", "GIT_CONFIG_NOSYSTEM": "1", "GIT_TERMINAL_PROMPT": "0",
        "GH_CONFIG_DIR": f"{fake}/.config/gh", "GH_NO_UPDATE_NOTIFIER": "1",
        "HTTP_PROXY": dead, "HTTPS_PROXY": dead, "ALL_PROXY": dead, "http_proxy": dead, "https_proxy": dead, "all_proxy": dead,
        "NO_PROXY": "", "no_proxy": "",
        # Maven 3.9 reads MAVEN_ARGS: offline even when the IDE passes its own -s settings file.
        "MAVEN_ARGS": "-o",
    }
    if app == "native":
        home = f"{run}/gpui-home"
        write(f"{home}/config/settings.toml", settings_toml(), dry)
        write(f"{home}/data/project-trust.json", trust, dry)
        write(f"{home}/data/plugins/state.json", market, dry)
        env["OPENIDEA_GPUI_HOME"] = home
        files = {"settings": f"{home}/config/settings.toml"}
    else:
        t = f"{run}/tauri"
        write(f"{t}/config/{MEASURE_ID}/settings.toml", settings_toml(), dry)
        write(f"{t}/data/{MEASURE_ID}/project-trust.json", trust, dry)
        write(f"{t}/data/{MEASURE_ID}/plugins/state.json", market, dry)
        env.update({"OPENIDEA_CONFIG_DIR": f"{t}/config", "OPENIDEA_DATA_DIR": f"{t}/data", "OPENIDEA_LOCAL_DATA_DIR": f"{t}/local",
                    "OPENIDEA_LOG_DIR": f"{t}/logs", "OPENIDEA_SKIP_SHELL_ENV": "1"})
        files = {"settings": f"{t}/config/{MEASURE_ID}/settings.toml"}
    return {"env": env, "files": files}


# ------------------------------------------------------------------ the guard

class Guard:
    """Every signal and every posted event goes through here: the PID must be one this run
    started (pid and start time recorded), its executable under the base folder, and never a
    process of the owner's installed app."""

    def __init__(self, base: str, log):
        self.base = base
        self.ours = {}  # pid -> start_us
        self.log = log

    def record(self, pid: int):
        info = macos.bsdinfo(pid)
        if not info:
            raise Abort(f"pid {pid} vanished before it was recorded")
        self.ours[pid] = info[2]
        self.log("record", pid=pid, start_us=info[2], path=macos.pid_path(pid))

    def check(self, pid: int, need_under_base=True) -> bool:
        info = macos.bsdinfo(pid)
        if not info or self.ours.get(pid) != info[2]:
            return False
        path = macos.pid_path(pid)
        if path.startswith(OWNER_BUNDLE):
            raise Abort(f"pid {pid} is the owner's app ({path}): refusing")
        if need_under_base and not path.startswith(self.base + "/"):
            return False
        return True

    def owner_pids(self) -> list:
        return [p for p, v in macos.snapshot().items() if v[3].startswith(OWNER_BUNDLE)]

    def signal(self, pid: int, sig: int, dry: bool, need_under_base=True):
        if dry:
            print(f"[dry-run] kill -{sig} {pid} (after checking pid, start time and executable path)")
            return
        if self.check(pid, need_under_base):
            os.kill(pid, sig)
            self.log("signal", pid=pid, sig=sig)

    def post(self, pid: int, what: str, fn, allow_activate: bool):
        if not self.check(pid):
            raise Abort(f"refusing to post {what}: pid {pid} is not the recorded measured app")
        front = macos.frontmost_pid()
        if front != pid:
            if front in self.owner_pids():
                raise Abort(f"the owner's app (pid {front}) is frontmost: stopping, nothing is posted")
            if not allow_activate:
                raise Abort(f"the measured app is not frontmost (frontmost pid {front}): stopping")
            subprocess.run(["/usr/bin/osascript", "-e",
                            f'tell application "System Events" to set frontmost of (first process whose unix id is {pid}) to true'],
                           capture_output=True, timeout=10)
            time.sleep(0.5)
            if macos.frontmost_pid() != pid:
                raise Abort("could not make the measured app frontmost")
            self.log("activated", pid=pid)
        fn()
        self.log("posted", what=what)


# ------------------------------------------------------------------ the sampler

class Sampler(threading.Thread):
    """1 Hz: the process set of one launch (the launcher, its descendants, and WebKit helpers whose
    responsible process is the launcher), each read with proc_pid_rusage."""

    def __init__(self, run_dir: str, launcher: int, name: str, guard: Guard, pre_existing: set, app: int):
        super().__init__(daemon=True)
        self.app = app
        self.out = open(f"{run_dir}/samples-{name}.jsonl", "a", encoding="utf-8")
        self.launcher = launcher
        self.guard = guard
        self.pre = pre_existing
        self.scenario = "launch"
        self.stop_flag = threading.Event()
        self.members = {}  # (pid, start) -> kind
        self.last = {}  # (pid, start) -> last rusage
        self.cpu_hist = []  # (t, cumulative cpu ns by aggregate.py's rule)
        self.cpu_acc = 0
        self.prev_t = None  # time of the previous sample
        self.label = None  # the label of the previous sample
        self.boundary_t = None  # time of the last sample before the current label's segment
        self.front = None  # last frontmost pid read
        self.front_counts = {}  # scenario -> {"samples", "not_frontmost", "unknown", "paths"}
        self.abort = None
        self.unreadable = set()
        self.net_hits = []
        self.lifetime_ok = True

    def classify(self, snap: dict):
        kids = {self.launcher}
        changed = True
        while changed:
            changed = False
            for pid, (ppid, _c, _s, _p) in snap.items():
                if ppid in kids and pid not in kids:
                    kids.add(pid)
                    changed = True
        for pid in kids:
            if pid in snap:
                key = (pid, snap[pid][2])
                if key not in self.members:
                    path = snap[pid][3]
                    kind = "launcher" if pid == self.launcher else ("app" if path.startswith(self.guard.base + "/") else "child")
                    self.members[key] = kind
                    self.guard.log("member", pid=pid, kind=kind, path=path, ppid=snap[pid][0], scenario=self.scenario)
                    self.guard.ours.setdefault(pid, snap[pid][2])
        for pid, (ppid, comm, start, path) in snap.items():
            if "/com.apple.WebKit." not in path or (pid, start) in self.members:
                continue
            if macos.responsible_pid(pid) in (self.launcher, self.app) and (pid, start) not in self.pre:
                self.members[(pid, start)] = "webkit:" + path.rsplit("/", 1)[-1]
                self.guard.log("member", pid=pid, kind=self.members[(pid, start)], path=path, scenario=self.scenario)

    def run(self):
        try:
            self.loop()
        except Exception as e:  # a dead sampler must stop the run, never leave it "ok"
            self.abort = f"sampler crashed: {e!r}"
        finally:
            self.out.close()

    def loop(self):
        tick = 0
        while not self.stop_flag.is_set():
            t = now()
            snap = macos.snapshot()
            self.classify(snap)
            procs = []
            delta = 0
            label = self.scenario  # read once: this sample's label
            if label != self.label:
                self.boundary_t, self.label = self.prev_t, label
            for (pid, start), kind in list(self.members.items()):
                alive = pid in snap and snap[pid][2] == start
                r = macos.rusage(pid) if alive else None
                if alive and r is None:
                    # The snapshot is from the top of the tick: a process that exited since then is not unreadable.
                    info = macos.bsdinfo(pid)
                    alive = bool(info) and info[2] == start
                    if not alive:
                        self.guard.log("member_exited_before_read", pid=pid, kind=kind)
                if alive and r is None:
                    if pid not in self.unreadable:
                        self.guard.log("unreadable_member", pid=pid, kind=kind, path=snap[pid][3], comm=snap[pid][1], ppid=snap[pid][0],
                                       errno=ctypes.get_errno())
                    self.unreadable.add(pid)
                    self.abort = f"pid {pid} ({kind}) is alive but unreadable: totals would undercount"
                if r:
                    if r["lifetime_max"] < r["footprint"]:
                        self.lifetime_ok = False
                    # aggregate.py's rule: a process first read now counts from zero if it started
                    # after the segment's boundary sample (or the launch), else from this first reading.
                    prev = self.last.get((pid, start))
                    if prev is not None:
                        delta += r["cpu_ns"] - prev["cpu_ns"]
                    elif self.boundary_t is None or start / 1e6 >= self.boundary_t:
                        delta += r["cpu_ns"]
                    self.last[(pid, start)] = r
                    procs.append({"pid": pid, "kind": kind, "start_us": start, **r})
            total_cpu = sum(v["cpu_ns"] for v in self.last.values())  # exited processes keep their last value
            self.cpu_acc += delta
            self.cpu_hist.append((t, self.cpu_acc))
            self.prev_t = t
            front = macos.frontmost_pid(timeout=2)
            if front != self.front:
                self.guard.log("frontmost", pid=front, path=macos.pid_path(front) if front else None,
                               is_app=(front == self.app) if front else None, scenario=self.scenario)
                self.front = front
            is_app = (front == self.app) if front else None  # None: unknown, not "not frontmost"
            fc = self.front_counts.setdefault(label, {"samples": 0, "not_frontmost": 0, "unknown": 0, "paths": []})
            fc["samples"] += 1
            if is_app is None:
                fc["unknown"] += 1
            elif not is_app:
                fc["not_frontmost"] += 1
                path = macos.pid_path(front)
                if path not in fc["paths"]:
                    fc["paths"].append(path)
            st = macos.session_state()
            if st["locked"] or st["display_asleep"]:
                self.abort = f"session not usable: {st}"
            if tick % 10 == 0:
                self.network([p["pid"] for p in procs])
            rec = {"t": t, "scenario": label, "sum_footprint": sum(p["footprint"] for p in procs),
                   "sum_resident": sum(p["resident"] for p in procs), "cpu_ns_total": total_cpu, "cpu_ns_rule": self.cpu_acc,
                   "wakeups_total": sum(v["wakeups"] for v in self.last.values()), "procs": procs, "session": st,
                   "front": front, "front_is_app": is_app}
            self.out.write(json.dumps(rec) + "\n")
            self.out.flush()
            tick += 1
            self.stop_flag.wait(max(0.0, 1.0 - (now() - t)))

    def network(self, pids: list):
        if not pids:
            return
        try:
            r = subprocess.run(["/usr/sbin/lsof", "-nP", "-a", "-p", ",".join(map(str, pids)), "-i"],
                               capture_output=True, text=True, timeout=20)
        except subprocess.SubprocessError:
            return
        for line in r.stdout.splitlines()[1:]:
            if not re.search(r"(127\.0\.0\.1|\[::1\]|localhost|\*:)", line):
                self.net_hits.append(line)

    def cpu_rate(self, window_s: float) -> float:
        """Share of one core used by the set over the last window_s seconds."""
        # The last sample before the window starts is included: at 1 Hz the samples inside a window of
        # N s span only about N-1 s, so windows of 2-3 s never reached the 0.8 coverage and never settled.
        cut = now() - window_s
        i = next((k for k, x in enumerate(self.cpu_hist) if x[0] >= cut), len(self.cpu_hist))
        h = self.cpu_hist[max(0, i - 1):]
        if len(h) < 2 or h[-1][0] - h[0][0] < window_s * 0.8:
            return 1.0
        return (h[-1][1] - h[0][1]) / 1e9 / (h[-1][0] - h[0][0])


# ------------------------------------------------------------------ one launch

class Launch:
    def __init__(self, ctx, name: str, args: list):
        self.ctx, self.name, self.args = ctx, name, args
        self.stdout = f"{ctx.run}/stdout-{name}.log"
        self.launcher = None
        self.app = None
        self.sampler = None

    def ev(self, event, **kw):
        self.ctx.log(event, launch=self.name, **kw)

    def start(self):
        c = self.ctx
        argv = ["/usr/bin/time", "-l", "-o", f"{c.run}/time-{self.name}.txt", c.exe] + self.args
        if c.dry:
            print(f"[dry-run] posix_spawn (responsibility disclaimed, own process group, env -i) {' '.join(argv)}")
            print("[dry-run]   env: " + " ".join(f"{k}={v}" for k, v in sorted(c.prep['env'].items())))
            return
        # The app activates itself when it starts; when it ends, macOS activates another app. So the
        # owner's app must not be frontmost now, and the app that is frontmost now (never the
        # owner's) is handed the focus back before ours is stopped.
        front = macos.frontmost_pid()
        if front in c.guard.owner_pids():
            raise Abort(f"the owner's app (pid {front}) is frontmost: not launching")
        self.prev_front = (front, macos.bsdinfo(front)[2]) if front and macos.bsdinfo(front) else None
        pre = {(p, v[2]) for p, v in macos.snapshot().items() if "/com.apple.WebKit." in v[3]}
        self.t0 = now()
        self.launcher, disclaimed = macos.spawn(argv, c.prep["env"], self.stdout)
        c.guard.ours[self.launcher] = macos.bsdinfo(self.launcher)[2]
        self.ev("spawned", launcher=self.launcher, disclaimed=disclaimed, argv=argv)
        deadline = now() + 15
        while now() < deadline and self.app is None:
            for pid, (ppid, _c, start, path) in macos.snapshot().items():
                if ppid == self.launcher and path == c.exe:
                    self.app = pid
                    c.guard.record(pid)
            time.sleep(0.05)
        if self.app is None:
            raise Abort("the app process did not appear under /usr/bin/time")
        resp = macos.responsible_pid(self.app)
        self.ev("app", pid=self.app, responsible=resp)
        # Measured: with the launcher disclaimed, the app it execs is responsible for itself, so its
        # WebKit helpers name the app (not the launcher) as their responsible process. Both are ours.
        if disclaimed and resp not in (self.launcher, self.app):
            raise Abort(f"responsibility: expected {self.launcher} or {self.app}, got {resp}; helper attribution would be wrong")
        if not disclaimed:
            self.ev("warning", detail="no disclaim SPI: WebKit helpers are matched by responsible pid of the terminal chain plus start time")
        self.sampler = Sampler(c.run, self.launcher, self.name, c.guard, pre, self.app)
        self.sampler.start()

    def set_scenario(self, name: str):
        if self.sampler:
            prev = self.sampler.scenario
            fc = self.sampler.front_counts.get(prev)
            if fc:
                self.ev("scenario_frontmost", name=prev, **fc)
        self.ev("scenario", name=name)
        if self.sampler:
            self.sampler.scenario = name

    def check(self):
        if self.ctx.dry:
            return
        if self.sampler.abort:
            raise Abort(self.sampler.abort)
        if not self.sampler.is_alive():
            raise Abort("the sampler stopped")
        if not self.ctx.guard.check(self.app):
            raise Abort("the measured app exited")
        # A launch that handed itself to another instance (single-instance plugin) exits at once.
        owners = self.ctx.guard.owner_pids()
        new = [p for p in owners if p not in self.ctx.owner_at_start]
        if new:
            raise Abort(f"a process of the owner's app started during the run: {new}")

    # --- waits on things observable from outside

    def wait(self, what: str, pred, timeout: float, poll=0.25):
        if self.ctx.dry:
            print(f"[dry-run] wait until {what} (at most {timeout:.0f} s)")
            return 0.0
        t = now()
        while now() - t < timeout:
            self.check()
            if pred():
                self.ev("waited", what=what, seconds=round(now() - t, 3))
                return now() - t
            time.sleep(poll)
        self.ev("timeout", what=what, seconds=timeout)
        return None

    def window(self):
        return macos.main_window(self.app)

    def title(self) -> str:
        w = self.window()
        return w["title"] if w else ""

    def log_has(self, pattern: str) -> bool:
        try:
            with open(self.stdout, encoding="utf-8", errors="replace") as f:
                return re.search(pattern, ANSI.sub("", f.read())) is not None
        except OSError:
            return False

    def settle(self, what: str, threshold: float, hold_s: float, timeout: float):
        """CPU of the whole set below `threshold` of one core over the last `hold_s` seconds, all of them
        after this wait began: quiet seconds from before the input that started the work do not count."""
        if self.ctx.dry:
            return self.wait(f"{what}: CPU < {threshold:.0%} of a core for {hold_s:.0f} s", lambda: True, timeout)
        t0 = now()
        return self.wait(f"{what}: CPU < {threshold:.0%} of a core for {hold_s:.0f} s",
                         lambda: now() - t0 >= hold_s and self.sampler.cpu_rate(hold_s) < threshold, timeout, poll=1.0)

    def hold(self, seconds: float, what: str):
        if self.ctx.dry:
            print(f"[dry-run] hold {seconds:.0f} s ({what}), no input")
            return
        end = now() + seconds
        while now() < end:
            self.check()
            time.sleep(1.0)

    def key(self, chord: str, gap=0.15):
        if self.ctx.dry:
            print(f"[dry-run] post {chord} to the app's pid")
            return
        self.check()
        self.ctx.guard.post(self.app, chord, lambda: macos.post_chord(self.app, chord), self.ctx.allow_activate)
        time.sleep(gap)

    def type(self, text: str):
        if self.ctx.dry:
            print(f"[dry-run] type {text!r} into the app's pid")
            return
        self.check()
        self.ctx.guard.post(self.app, f"type {text}", lambda: macos.type_text(self.app, text), self.ctx.allow_activate)

    def windows_event(self, label: str):
        """The app's on-screen windows (title, size) as evidence of what a scenario showed."""
        if self.ctx.dry:
            return
        ws = [{"title": w["title"], "w": w["w"], "h": w["h"]} for w in macos.windows(self.app) if w["onscreen"]]
        if not self.ctx.guard.check(self.app):
            return
        self.ev("windows", at=label, windows=ws, ax=macos.ax_focused(self.app) if self.ctx.ax_probe else None)

    def capture(self, label: str):
        """A picture of the measured app's own main window (by window id, owner pid checked), kept
        under the run's folder. Never the screen, never another window. A failure is an event."""
        c = self.ctx
        if not c.capture:
            return
        if c.dry:
            print(f"[dry-run] screencapture -x -o -l <window id of pid's main window> {c.run}/captures/NN-{self.name}-{label}.png")
            return
        w = self.window()
        if not w or w["pid"] != self.app or not c.guard.check(self.app):
            self.ev("capture_failed", label=label, detail="no main window owned by the recorded measured app")
            return
        c.capture_seq += 1
        slug = re.sub(r"[^A-Za-z0-9._-]+", "_", label)[:80]
        path = f"{c.run}/captures/{c.capture_seq:02d}-{self.name}-{slug}.png"
        os.makedirs(os.path.dirname(path), exist_ok=True)
        err = macos.capture_window(w["id"], path)
        if err:
            self.ev("capture_failed", label=label, window_id=w["id"], detail=err)
        else:
            self.ev("capture", label=label, file=path, window_id=w["id"], owner_pid=w["pid"], title=w["title"], w=w["w"], h=w["h"])

    def snapshot_tools(self, label: str):
        """Cross-check numbers from the system tools at a scenario's end, and the capture of the
        window while the scenario's view (Find, the Git log) is still shown."""
        if self.ctx.dry:
            print(f"[dry-run] footprint -p / ps -o rss=,time= / top -l 2 on the set ({label})")
            self.capture(label)
            return
        # The tools' own cost lands in a window of its own, not in the scenario just measured.
        self.set_scenario(f"tools_{label}")
        self.capture(label)
        pids = [p for (p, s), _k in self.sampler.members.items() if macos.bsdinfo(p) and macos.bsdinfo(p)[2] == s]
        base = f"{self.ctx.run}/tools-{self.name}-{label}"
        with open(base + ".txt", "w") as f:
            for cmd in (["/usr/bin/footprint"] + [x for p in pids for x in ("-p", str(p))],
                        ["/bin/ps", "-o", "pid=,rss=,time=,comm=", "-p", ",".join(map(str, pids))],
                        ["/usr/bin/top", "-l", "2", "-s", "1", "-stats", "pid,cpu,mem,command"] + [x for p in pids for x in ("-pid", str(p))]):
                f.write("$ " + " ".join(cmd) + "\n")
                try:
                    r = subprocess.run(cmd, capture_output=True, text=True, timeout=60)
                    f.write(r.stdout + r.stderr + "\n")
                except subprocess.SubprocessError as e:
                    f.write(f"failed: {e}\n")

    def stop(self):
        c = self.ctx
        if c.dry:
            print("[dry-run] log the app that was frontmost before the launch (nothing is activated);"
                  " SIGTERM the app pid; wait 10 s; SIGKILL; then the recorded descendants by pid and start time")
            return
        if self.app and c.guard.check(self.app):
            self.hand_focus_back()
            for sig, wait_s in ((signal.SIGTERM, 10), (signal.SIGKILL, 5)):
                if c.guard.check(self.app):
                    c.guard.signal(self.app, sig, False)
                end = now() + wait_s
                while now() < end and c.guard.check(self.app):
                    time.sleep(0.2)
                if not c.guard.check(self.app):
                    break
        if self.sampler:
            for (pid, start), kind in list(self.sampler.members.items()):
                if kind == "child" and c.guard.check(pid, need_under_base=False):
                    c.guard.signal(pid, signal.SIGTERM, False, need_under_base=False)
            time.sleep(3)
            for (pid, start), kind in list(self.sampler.members.items()):
                if kind == "child" and c.guard.check(pid, need_under_base=False):
                    c.guard.signal(pid, signal.SIGKILL, False, need_under_base=False)
            end = now() + 30
            while now() < end and any(macos.bsdinfo(p) and macos.bsdinfo(p)[2] == s for (p, s), k in self.sampler.members.items() if k.startswith("webkit")):
                time.sleep(1)
            left = [(p, k) for (p, s), k in self.sampler.members.items() if macos.bsdinfo(p) and macos.bsdinfo(p)[2] == s]
            self.fakehome_entries()
            self.ev("stopped", left_running=left, unreadable=sorted(self.sampler.unreadable), network=self.sampler.net_hits,
                    lifetime_max_word_ok=self.sampler.lifetime_ok, window=self.window_seen)
            self.sampler.stop_flag.set()
            self.sampler.join(5)
        if self.launcher and c.guard.check(self.launcher, need_under_base=False):
            end = now() + 10
            while now() < end and c.guard.check(self.launcher, need_under_base=False):
                time.sleep(0.2)
            c.guard.signal(self.launcher, signal.SIGTERM, False, need_under_base=False)
        try:
            os.waitpid(self.launcher, 0)
        except (ChildProcessError, TypeError):
            pass

    window_seen = None
    prev_front = None

    def hand_focus_back(self):
        """Before ours stops, so that macOS does not pick the next app itself (possibly the owner's)."""
        if macos.frontmost_pid() != self.app:
            return
        prev = self.prev_front
        info = macos.bsdinfo(prev[0]) if prev else None
        # Making a process this run did not start frontmost is not allowed: macOS picks the next app
        # when ours stops. That is only a risk while the owner's app runs.
        owners = self.ctx.guard.owner_pids()
        self.ev("focus_not_handed_back", previous_front=list(prev) if prev else None,
                previous_alive=bool(info and prev and info[2] == prev[1]), owner_app_pids=owners)
        if owners:
            self.ev("warning", detail=f"the owner's app runs ({owners}): macOS may activate it when ours stops")

    def fakehome_entries(self):
        """What the app created under the fake home's Library (WebKit storage, caches, preferences)."""
        lib = f"{self.ctx.run}/fakehome/Library"
        out = {}
        for sub in ("WebKit", "Caches", "HTTPStorages", "Saved Application State", "Preferences", "Application Support"):
            try:
                out[sub] = sorted(os.listdir(f"{lib}/{sub}"))
            except OSError:
                out[sub] = None
        self.ev("fakehome_library", entries=out)


# ------------------------------------------------------------------ the scenarios

def launch_cold(ctx):
    if "cold" not in ctx.scenarios:
        return
    L = Launch(ctx, "cold", [])
    try:
        L.start()
        L.set_scenario("cold_start")
        L.wait("a window is on screen", lambda: L.window() is not None, 60)
        L.window_seen = None if ctx.dry else L.window()
        L.settle("cold start", 0.03, 5, 120)
        L.set_scenario("cold_steady")
        L.hold(30, "steady after start")
        L.snapshot_tools("cold")
    finally:
        L.set_scenario("stop")
        L.stop()


def launch_project(ctx, files: list, query: str):
    L = Launch(ctx, "project", [ctx.project])
    want = ctx.scenarios
    root = re.escape(ctx.project)
    try:
        L.start()
        L.set_scenario("open_project")
        L.wait("a window is on screen", lambda: L.window() is not None, 60)
        L.window_seen = None if ctx.dry else L.window()
        L.wait("log: opened <project>", lambda: L.log_has(r"opened " + root + r" in window"), 300)
        L.wait("log: index finished", lambda: L.log_has(r"(indexed \d+ files|index snapshot: warm start)"), 1800, poll=1.0)
        L.settle("project open", 0.03, 10, 600)
        L.windows_event("project open")
        L.hold(30, "steady with the project open")
        L.snapshot_tools("open_project")

        if "open_files" in want:
            L.set_scenario("open_files")
            for i, path in enumerate(files):
                name = os.path.basename(path)
                L.key("cmd-shift-o", gap=0.8)
                L.type(name)
                L.settle(f"file search {name}", 0.10, 2, 15)
                L.key("enter", gap=0.3)
                ok = L.wait(f"title ends with ' – {name}'", lambda: L.title().endswith(" – " + name), 30)
                if ok is None and not ctx.dry:
                    L.ev("file_not_opened", file=name, title=L.title())
                    L.key("escape")
                    continue
                if not ctx.dry:
                    L.ev("file_opened", file=name, title=L.title())
                L.settle(f"opened {name}", 0.10, 2, 30)
                # The first and the last file: one picture at the top, one after the scrolling.
                pictured = i in {0, len(files) - 1}
                if pictured:
                    L.capture(f"opened {name}")
                for _ in range(40):
                    L.key("pagedown", gap=0.1)
                L.key("cmd-down", gap=0.3)
                L.settle(f"scrolled {name}", EDITOR_SETTLE, 3, 60)
                if pictured:
                    L.capture(f"scrolled {name}")
                L.windows_event(f"scrolled {name}")
            L.hold(15, "steady after opening files")
            L.snapshot_tools("open_files")

        if "find_in_files" in want:
            L.set_scenario("find_in_files")
            L.key("cmd-shift-f", gap=1.0)
            L.type(query)
            L.settle("find results", EDITOR_SETTLE, 5, 300)
            L.windows_event("find results")
            for _ in range(10):
                L.key("down", gap=0.5)
            L.settle("find preview", EDITOR_SETTLE, 5, 60)
            L.windows_event("find preview")
            L.snapshot_tools("find_in_files")
            # The key that closes Find and the settle after it belong to the scenario.
            L.set_scenario("find_in_files")
            L.key("escape", gap=1.0)
            L.settle("find closed", EDITOR_SETTLE, 5, 60)

        if "git_log" in want:
            # Open the Git log and let it settle; no scrolling. After Cmd+9 the Tauri app focuses the
            # branch filter field and the native app the commit list, no key reaches the Tauri list
            # the same way, and PageDown moves 20 rows in Tauri and 10 natively.
            L.set_scenario("git_log")
            L.key("cmd-9", gap=1.0)
            L.settle("git log", EDITOR_SETTLE, 5, 300)
            L.windows_event("git log shown")
            L.snapshot_tools("git_log")
            # The key that hides the log and the settle after it belong to the scenario.
            L.set_scenario("git_log")
            L.key("cmd-9", gap=1.0)
            L.settle("git log hidden", EDITOR_SETTLE, 5, 60)

        if "mode" in want:
            settings = ctx.prep["files"]["settings"]

            def mode_is(m):
                def pred():
                    try:
                        return re.search(r'mode\s*=\s*"' + m + '"', open(settings, encoding="utf-8").read()) is not None
                    except OSError:
                        return False
                return pred

            L.set_scenario("deep_mode")
            L.key("ctrl-alt-shift-m", gap=0.5)
            if L.wait("settings.toml says mode = \"full\"", mode_is("full"), 20) is None and not ctx.dry:
                L.ev("mode_not_switched", to="full")
            L.settle("deep mode", EDITOR_SETTLE, 10, 600)
            L.windows_event("deep mode")
            L.hold(60, "steady in Deep mode")
            L.snapshot_tools("deep_mode")
            L.set_scenario("blaze_again")
            L.key("ctrl-alt-shift-m", gap=0.5)
            if L.wait("settings.toml no longer says mode = \"full\"", lambda: not mode_is("full")(), 20) is None and not ctx.dry:
                L.ev("mode_not_switched", to="light")
            L.settle("back in Blaze mode", EDITOR_SETTLE, 10, 600)
            L.windows_event("blaze again")
            L.hold(60, "steady back in Blaze mode")
            L.snapshot_tools("blaze_again")

        if "idle" in want:
            L.set_scenario("idle")
            L.hold(ctx.idle_s, "idle, no input")
            L.windows_event("idle end")
            fc = {} if ctx.dry else L.sampler.front_counts.get("idle", {})
            if fc.get("not_frontmost"):
                # Kept, but its idle row is left out of the aggregate (degraded for this scenario only).
                L.ev("idle_not_frontmost", seconds=fc["not_frontmost"], of=fc["samples"], unknown=fc["unknown"], paths=fc["paths"])
                ctx.degraded_scenarios.add("idle")
            L.snapshot_tools("idle")
    finally:
        L.set_scenario("stop")
        L.stop()


# ------------------------------------------------------------------ campaign helpers

def select_files(project: str, n=10) -> list:
    """The n largest .java files of 20-200 KB with a basename unique among the project's files that
    git does not ignore (tracked plus untracked-not-ignored). A walk of the tree would also see
    ignored copies such as nested worktrees (.claude/worktrees/* in Allocadia), whose files the
    apps' Go to File need not list and whose duplicates would make every main-tree name non-unique."""
    r = subprocess.run(["/usr/bin/git", "-C", project, "ls-files", "-z", "--cached", "--others", "--exclude-standard"],
                       capture_output=True, env={"PATH": "/usr/bin:/bin", "GIT_OPTIONAL_LOCKS": "0", "GIT_CONFIG_NOSYSTEM": "1",
                                                 "HOME": "/nonexistent"}, check=True)
    by_name = {}
    for rel in r.stdout.decode("utf-8", "replace").split("\0"):
        parts = rel.split("/")
        if not rel or any(p in SKIP_DIRS or p.startswith("bazel-") for p in parts[:-1]):
            continue
        path = os.path.join(project, rel)
        if os.path.isfile(path) and not os.path.islink(path):
            by_name.setdefault(parts[-1], []).append(path)
    cands = []
    for name, paths in by_name.items():
        if name.endswith(".java") and len(paths) == 1:
            size = os.path.getsize(paths[0])
            if 20_000 <= size <= 200_000:
                cands.append((size, paths[0]))
    cands.sort(key=lambda x: (-x[0], x[1]))
    return [p for _s, p in cands[:n]]


def index_sha(project: str) -> str:
    import hashlib
    try:
        with open(f"{project}/.git/index", "rb") as f:
            return hashlib.sha256(f.read()).hexdigest()
    except OSError as e:
        return f"unreadable: {e}"


def project_stamp(base: str, project: str):
    stamp = f"{base}/project.stamp"
    with open(stamp, "w") as f:
        f.write(f"git index sha256 at stamp: {index_sha(project)}\n")
    print(f"stamped {stamp}; git index sha256 {index_sha(project)}")


def project_check(base: str, project: str) -> int:
    """Paths of the clone written since the stamp (the apps' writes into the project). Returns how
    many are outside <clone>/.openidea (the expected changelists file and its .gitignore live there,
    and run_all.sh empties that folder between runs instead of cloning again)."""
    stamp = f"{base}/project.stamp"
    r = subprocess.run(["/usr/bin/find", project, "-newer", stamp, "-print"], capture_output=True, text=True)
    written = [l for l in r.stdout.splitlines() if l != project]
    inside = [l for l in written if l == f"{project}/.openidea" or l.startswith(f"{project}/.openidea/")]
    # Measured in the first pilot: opening the project rewrites the clone's .git/index (the stat-cache
    # refresh the design lists as expected; GIT_OPTIONAL_LOCKS=0 did not prevent it). Listed, not fatal.
    index = [l for l in written if l in (f"{project}/.git", f"{project}/.git/index")]
    outside = [l for l in written if l not in inside and l not in index]
    before = open(stamp).read().strip()
    out = f"{base}/project-writes-{int(now())}.txt"
    with open(out, "w") as f:
        f.write("# under .openidea\n" + "\n".join(inside) + "\n# git index refresh\n" + "\n".join(index)
                + f"\n# {before}; at check: {index_sha(project)}\n# elsewhere\n" + "\n".join(outside) + "\n")
    print(f"written into the clone since the stamp: {len(inside)} under .openidea, {len(index)} git index, {len(outside)} elsewhere: {out}")
    return len(outside)


class Ctx:
    pass


def run_summary(run: str) -> dict:
    """Per scenario: samples not frontmost, and the numbers by aggregate.py's one definition."""
    out = {"not_frontmost": {}, "scenarios": {}}
    for launch in ("cold", "project"):
        path = f"{run}/samples-{launch}.jsonl"
        if not os.path.exists(path):
            continue
        try:
            samples = aggregate.load_samples(path)
            rd = aggregate.Readings(samples)
            for sc in aggregate.SCENARIOS:
                m = aggregate.scenario_metrics(samples, sc, rd)
                if m:
                    out["scenarios"][sc] = m
                    out["not_frontmost"][sc] = m["not_frontmost_samples"]
        except (OSError, ValueError, KeyError) as e:
            out.setdefault("summary_errors", []).append(f"{launch}: {e!r}")
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--base", required=True)
    ap.add_argument("--app", choices=["native", "tauri"])
    ap.add_argument("--rep", type=int)
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("--allow-activate", action="store_true")
    ap.add_argument("--query", default="import java.util")
    ap.add_argument("--select-files", action="store_true")
    ap.add_argument("--project-stamp", action="store_true")
    ap.add_argument("--project-check", action="store_true")
    ap.add_argument("--project", help="project folder (default: config.json's clone)")
    ap.add_argument("--files-json", help="the files to open (default: <base>/files.json)")
    ap.add_argument("--runs-root", help="folder of the run folders, under the base (default: <base>/runs)")
    ap.add_argument("--scenarios", default=",".join(SCENARIO_GROUPS), help="comma list of " + ",".join(SCENARIO_GROUPS))
    ap.add_argument("--idle-s", type=float, default=600)
    ap.add_argument("--ax-probe", action="store_true", help="read the focused element through Accessibility at each evidence point")
    ap.add_argument("--capture", action="store_true", help="picture the measured app's own main window at each snapshot and for the first and last file")
    a = ap.parse_args()
    base = os.path.realpath(a.base)
    if not BASE_RE.match(base):
        sys.exit(f"base must be /private/tmp/openidea-measure-<random>, got {base}")
    cfg = json.load(open(f"{base}/config.json")) if os.path.exists(f"{base}/config.json") else {}
    project = os.path.realpath(a.project) if a.project else cfg.get("project", f"{base}/project/Allocadia")
    if not project.startswith(base + "/"):
        sys.exit(f"the project must be under the base: {project}")
    runs_root = os.path.realpath(a.runs_root) if a.runs_root else f"{base}/runs"
    if not runs_root.startswith(base + "/"):
        sys.exit(f"the runs root must be under the base: {runs_root}")
    scenarios = [x for x in a.scenarios.split(",") if x]
    if not scenarios or any(x not in SCENARIO_GROUPS for x in scenarios):
        sys.exit(f"--scenarios: choose from {','.join(SCENARIO_GROUPS)}")
    files_json = a.files_json or f"{base}/files.json"
    if a.select_files:
        files = select_files(project)
        json.dump(files, open(f"{base}/files.json", "w"), indent=1)
        print("\n".join(files))
        return
    if a.project_stamp:
        return project_stamp(base, project)
    if a.project_check:
        sys.exit(1 if project_check(base, project) else 0)
    if not a.app or a.rep is None:
        sys.exit("--app and --rep are required")
    ctx = Ctx()
    ctx.dry, ctx.allow_activate, ctx.project = a.dry_run, a.allow_activate, project
    ctx.scenarios, ctx.idle_s, ctx.ax_probe = set(scenarios), a.idle_s, a.ax_probe
    ctx.capture, ctx.capture_seq, ctx.degraded_scenarios = a.capture, 0, set()
    ctx.exe = cfg.get(f"{a.app}_bin", f"<{a.app} binary from build.sh>")
    if not ctx.dry and not (ctx.exe.startswith(base + "/") and os.access(ctx.exe, os.X_OK)):
        sys.exit(f"binary not under the base folder or not executable: {ctx.exe}")
    ctx.run = f"{runs_root}/{a.rep}-{a.app}"
    if not ctx.dry:
        if os.path.exists(ctx.run):
            sys.exit(f"{ctx.run} exists: every run gets a fresh folder")
        os.makedirs(ctx.run)
    events = None if ctx.dry else open(f"{ctx.run}/events.jsonl", "a", encoding="utf-8")

    degraded = {}

    def log(event, **kw):
        if event in ("timeout", "file_not_opened", "mode_not_switched", "warning"):
            degraded[event] = degraded.get(event, 0) + 1
        if event == "stopped" and (kw.get("network") or kw.get("left_running") or kw.get("unreadable")):
            degraded["stopped_with_findings"] = degraded.get("stopped_with_findings", 0) + 1
        rec = {"t": now(), "event": event, **kw}
        if events:
            events.write(json.dumps(rec) + "\n")
            events.flush()
        print(json.dumps(rec), flush=True)

    ctx.log = log
    ctx.guard = Guard(base, log)
    ctx.owner_at_start = [] if ctx.dry else ctx.guard.owner_pids()
    log("run", app=a.app, rep=a.rep, exe=ctx.exe, project=project, scenarios=scenarios, idle_s=a.idle_s, files_json=files_json, ax_probe=a.ax_probe, capture=a.capture, owner_app_pids=ctx.owner_at_start,
        loadavg=os.getloadavg(), session=macos.session_state(), permissions=macos.permissions())
    ctx.prep = prepare(ctx.run, a.app, project, ctx.dry)
    files = json.load(open(files_json)) if os.path.exists(files_json) else ["<10 files from --select-files>"]
    status = "ok"
    try:
        launch_cold(ctx)
        if not ctx.dry:
            time.sleep(10)
        launch_project(ctx, files, a.query)
    except Abort as e:
        status = f"aborted: {e}"
        log("abort", reason=str(e))
    except Exception as e:  # anything unexpected fails the run; the launches' finally blocks stopped the app
        status = f"error: {e!r}"
        log("abort", reason=status)
    finally:
        if status == "ok" and degraded:
            status = "degraded"
        log("end", status=status, degraded=degraded, degraded_scenarios=sorted(ctx.degraded_scenarios))
        if not ctx.dry:
            json.dump({"status": status, "app": a.app, "rep": a.rep, "degraded": degraded,
                       "degraded_scenarios": sorted(ctx.degraded_scenarios), **run_summary(ctx.run)},
                      open(f"{ctx.run}/status.json", "w"), indent=1)
    sys.exit(0 if status == "ok" else 3)


if __name__ == "__main__":
    main()
