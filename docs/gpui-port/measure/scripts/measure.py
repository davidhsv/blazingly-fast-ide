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
import json
import os
import re
import signal
import subprocess
import sys
import threading
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import macos  # noqa: E402

BASE_RE = re.compile(r"^/private/tmp/openidea-measure-[A-Za-z0-9]{6,}$")
OWNER_BUNDLE = "/Applications/Blazingly Fast IDE.app/"
MEASURE_ID = "ai.l0ng.openidea.measure"
ANSI = re.compile(r"\x1b\[[0-9;]*m")
SKIP_DIRS = {".git", "node_modules", ".idea", ".openidea", "target", "build", "out", "dist", ".gradle", ".mvn"}


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

    def __init__(self, run_dir: str, launcher: int, name: str, guard: Guard, pre_existing: set):
        super().__init__(daemon=True)
        self.out = open(f"{run_dir}/samples-{name}.jsonl", "a", encoding="utf-8")
        self.launcher = launcher
        self.guard = guard
        self.pre = pre_existing
        self.scenario = "launch"
        self.stop_flag = threading.Event()
        self.members = {}  # (pid, start) -> kind
        self.last = {}  # (pid, start) -> last rusage
        self.cpu_hist = []  # (t, total cpu ns)
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
                    self.guard.ours.setdefault(pid, snap[pid][2])
        for pid, (ppid, comm, start, path) in snap.items():
            if "/com.apple.WebKit." not in path or (pid, start) in self.members:
                continue
            if macos.responsible_pid(pid) == self.launcher and (pid, start) not in self.pre:
                self.members[(pid, start)] = "webkit:" + path.rsplit("/", 1)[-1]

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
            for (pid, start), kind in list(self.members.items()):
                alive = pid in snap and snap[pid][2] == start
                r = macos.rusage(pid) if alive else None
                if alive and r is None:
                    self.unreadable.add(pid)
                    self.abort = f"pid {pid} ({kind}) is alive but unreadable: totals would undercount"
                if r:
                    if r["lifetime_max"] < r["footprint"]:
                        self.lifetime_ok = False
                    self.last[(pid, start)] = r
                    procs.append({"pid": pid, "kind": kind, **r})
            total_cpu = sum(v["cpu_ns"] for v in self.last.values())  # exited processes keep their last value
            self.cpu_hist.append((t, total_cpu))
            st = macos.session_state()
            if st["locked"] or st["display_asleep"]:
                self.abort = f"session not usable: {st}"
            if tick % 10 == 0:
                self.network([p["pid"] for p in procs])
            rec = {"t": t, "scenario": self.scenario, "sum_footprint": sum(p["footprint"] for p in procs),
                   "sum_resident": sum(p["resident"] for p in procs), "cpu_ns_total": total_cpu,
                   "wakeups_total": sum(v["wakeups"] for v in self.last.values()), "procs": procs, "session": st}
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
        h = [x for x in self.cpu_hist if x[0] >= now() - window_s]
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
        if disclaimed and resp != self.launcher:
            raise Abort(f"responsibility: expected {self.launcher}, got {resp}; helper attribution would be wrong")
        if not disclaimed:
            self.ev("warning", detail="no disclaim SPI: WebKit helpers are matched by responsible pid of the terminal chain plus start time")
        self.sampler = Sampler(c.run, self.launcher, self.name, c.guard, pre)
        self.sampler.start()

    def set_scenario(self, name: str):
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
        """CPU of the whole set below `threshold` of one core over the last `hold_s` seconds."""
        return self.wait(f"{what}: CPU < {threshold:.0%} of a core for {hold_s:.0f} s", lambda: self.sampler.cpu_rate(hold_s) < threshold, timeout, poll=1.0)

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

    def snapshot_tools(self, label: str):
        """Cross-check numbers from the system tools at a scenario's end."""
        if self.ctx.dry:
            print(f"[dry-run] footprint -p / ps -o rss=,time= / top -l 2 on the set ({label})")
            return
        # The tools' own cost lands in a window of its own, not in the scenario just measured.
        self.set_scenario(f"tools_{label}")
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
            print("[dry-run] make the app that was frontmost before the launch (never the owner's) frontmost again;"
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
        if not prev or not info or info[2] != prev[1] or prev[0] in self.ctx.guard.owner_pids():
            self.ev("warning", detail="no app to hand the focus to: macOS picks the next one when ours stops")
            return
        subprocess.run(["/usr/bin/osascript", "-e",
                        f'tell application "System Events" to set frontmost of (first process whose unix id is {prev[0]}) to true'],
                       capture_output=True, timeout=10)
        time.sleep(0.5)
        self.ev("focus_handed_back", pid=prev[0], frontmost=macos.frontmost_pid())

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
    root = re.escape(ctx.project)
    try:
        L.start()
        L.set_scenario("open_project")
        L.wait("a window is on screen", lambda: L.window() is not None, 60)
        L.window_seen = None if ctx.dry else L.window()
        L.wait("log: opened <project>", lambda: L.log_has(r"opened " + root + r" in window"), 300)
        L.wait("log: index finished", lambda: L.log_has(r"(indexed \d+ files|index snapshot: warm start)"), 1800, poll=1.0)
        L.settle("project open", 0.03, 10, 600)
        L.hold(30, "steady with the project open")
        L.snapshot_tools("open_project")

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
            L.settle(f"opened {name}", 0.10, 2, 30)
            for _ in range(40):
                L.key("pagedown", gap=0.1)
            L.key("cmd-down", gap=0.3)
            L.settle(f"scrolled {name}", 0.05, 3, 60)
        L.hold(15, "steady after opening files")
        L.snapshot_tools("open_files")

        L.set_scenario("find_in_files")
        L.key("cmd-shift-f", gap=1.0)
        L.type(query)
        L.settle("find results", 0.05, 5, 300)
        for _ in range(10):
            L.key("down", gap=0.5)
        L.settle("find preview", 0.05, 5, 60)
        L.snapshot_tools("find_in_files")
        L.key("escape", gap=1.0)
        L.settle("find closed", 0.05, 5, 60)

        L.set_scenario("git_log")
        L.key("cmd-9", gap=1.0)
        L.settle("git log", 0.05, 5, 300)
        for _ in range(20):
            L.key("pagedown", gap=0.3)
        L.settle("git log scrolled", 0.05, 5, 120)
        L.snapshot_tools("git_log")
        L.key("cmd-9", gap=1.0)
        L.settle("git log hidden", 0.05, 5, 60)

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
        L.settle("deep mode", 0.03, 10, 600)
        L.hold(60, "steady in Deep mode")
        L.snapshot_tools("deep_mode")
        L.set_scenario("blaze_again")
        L.key("ctrl-alt-shift-m", gap=0.5)
        if L.wait("settings.toml no longer says mode = \"full\"", lambda: not mode_is("full")(), 20) is None and not ctx.dry:
            L.ev("mode_not_switched", to="light")
        L.settle("back in Blaze mode", 0.03, 10, 600)
        L.hold(60, "steady back in Blaze mode")
        L.snapshot_tools("blaze_again")

        L.set_scenario("idle")
        L.hold(600, "idle, no input")
        L.snapshot_tools("idle")
    finally:
        L.set_scenario("stop")
        L.stop()


# ------------------------------------------------------------------ campaign helpers

def select_files(project: str, n=10) -> list:
    """The n largest .java files of 20-200 KB with a basename unique in the project."""
    by_name = {}
    for dirpath, dirnames, filenames in os.walk(project):
        dirnames[:] = [d for d in dirnames if d not in SKIP_DIRS and not d.startswith("bazel-")]
        for f in filenames:
            by_name.setdefault(f, []).append(os.path.join(dirpath, f))
    cands = []
    for name, paths in by_name.items():
        if name.endswith(".java") and len(paths) == 1:
            size = os.path.getsize(paths[0])
            if 20_000 <= size <= 200_000:
                cands.append((size, paths[0]))
    cands.sort(key=lambda x: (-x[0], x[1]))
    return [p for _s, p in cands[:n]]


def project_stamp(base: str, project: str):
    stamp = f"{base}/project.stamp"
    open(stamp, "w").close()
    print(f"stamped {stamp}")


def project_check(base: str, project: str) -> int:
    """Paths of the clone written since the stamp (the apps' writes into the project). Returns how
    many are outside <clone>/.openidea (the expected changelists file and its .gitignore live there,
    and run_all.sh empties that folder between runs instead of cloning again)."""
    stamp = f"{base}/project.stamp"
    r = subprocess.run(["/usr/bin/find", project, "-newer", stamp, "-print"], capture_output=True, text=True)
    written = [l for l in r.stdout.splitlines() if l != project]
    inside = [l for l in written if l == f"{project}/.openidea" or l.startswith(f"{project}/.openidea/")]
    outside = [l for l in written if l not in inside]
    out = f"{base}/project-writes-{int(now())}.txt"
    with open(out, "w") as f:
        f.write("# under .openidea\n" + "\n".join(inside) + "\n# elsewhere\n" + "\n".join(outside) + "\n")
    print(f"written into the clone since the stamp: {len(inside)} under .openidea, {len(outside)} elsewhere: {out}")
    return len(outside)


class Ctx:
    pass


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
    a = ap.parse_args()
    base = os.path.realpath(a.base)
    if not BASE_RE.match(base):
        sys.exit(f"base must be /private/tmp/openidea-measure-<random>, got {base}")
    cfg = json.load(open(f"{base}/config.json")) if os.path.exists(f"{base}/config.json") else {}
    project = cfg.get("project", f"{base}/project/Allocadia")
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
    ctx.exe = cfg.get(f"{a.app}_bin", f"<{a.app} binary from build.sh>")
    if not ctx.dry and not (ctx.exe.startswith(base + "/") and os.access(ctx.exe, os.X_OK)):
        sys.exit(f"binary not under the base folder or not executable: {ctx.exe}")
    ctx.run = f"{base}/runs/{a.rep}-{a.app}"
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
    log("run", app=a.app, rep=a.rep, exe=ctx.exe, project=project, owner_app_pids=ctx.owner_at_start,
        loadavg=os.getloadavg(), session=macos.session_state(), permissions=macos.permissions())
    ctx.prep = prepare(ctx.run, a.app, project, ctx.dry)
    files = json.load(open(f"{base}/files.json")) if os.path.exists(f"{base}/files.json") else ["<10 files from --select-files>"]
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
        log("end", status=status, degraded=degraded)
        if not ctx.dry:
            json.dump({"status": status, "app": a.app, "rep": a.rep, "degraded": degraded}, open(f"{ctx.run}/status.json", "w"))
    sys.exit(0 if status == "ok" else 3)


if __name__ == "__main__":
    main()
