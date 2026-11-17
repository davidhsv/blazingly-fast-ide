"""macOS calls through ctypes (Python standard library only, Python 3.9+).

Processes (libproc), per-process memory and CPU (proc_pid_rusage), WebKit helper attribution
(the "responsible process" SPI that Activity Monitor and crates/ide-workspace/src/memory.rs use),
a spawn that disclaims responsibility (so the spawned app is responsible for its own WebKit
helpers), the window list (CoreGraphics), input posted to ONE process by PID (CGEventPostToPid:
the real pointer and keyboard are not touched, no other process receives anything), and the
session state (screen locked, display asleep).

Nothing here sends anything to a process other than the PID it is given; callers check that PID.
"""
from __future__ import annotations

import ctypes
import os
import re
import struct
import subprocess
from ctypes import POINTER, Structure, byref, c_bool, c_char_p, c_double, c_int, c_int32, c_int64, c_long, c_uint16, c_uint32, c_uint64, c_ulong, c_void_p

libc = ctypes.CDLL("/usr/lib/libSystem.B.dylib", use_errno=True)
CF = ctypes.CDLL("/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation")
CG = ctypes.CDLL("/System/Library/Frameworks/CoreGraphics.framework/CoreGraphics")
AS = ctypes.CDLL("/System/Library/Frameworks/ApplicationServices.framework/ApplicationServices")

# ---------------------------------------------------------------- processes

libc.proc_listallpids.argtypes = [c_void_p, c_int]
libc.proc_listallpids.restype = c_int
libc.proc_pidinfo.argtypes = [c_int, c_int, c_uint64, c_void_p, c_int]
libc.proc_pidinfo.restype = c_int
libc.proc_pidpath.argtypes = [c_int, c_void_p, c_uint32]
libc.proc_pidpath.restype = c_int
libc.proc_pid_rusage.argtypes = [c_int, c_int, c_void_p]
libc.proc_pid_rusage.restype = c_int


class _Timebase(Structure):
    _fields_ = [("numer", c_uint32), ("denom", c_uint32)]


libc.mach_timebase_info.argtypes = [POINTER(_Timebase)]
_tb = _Timebase()
libc.mach_timebase_info(byref(_tb))
_NUMER, _DENOM = (_tb.numer or 1), (_tb.denom or 1)

PROC_PIDTBSDINFO = 3
BSDINFO_SIZE = 136  # struct proc_bsdinfo, as crates/ide-workspace/src/memory.rs lays it out
RUSAGE_INFO_V4 = 4
# rusage_info_v4 read as u64 words: 0-1 uuid, 2 user, 3 system, 4 pkg_idle_wkups,
# 5 interrupt_wkups, 6 pageins, 7 wired, 8 resident, 9 phys_footprint (memory.rs: word 9),
# 30 lifetime_max_phys_footprint (from <sys/resource.h>; checked at run time: it must be >= word 9).
W_USER, W_SYS, W_IDLE_WK, W_INT_WK, W_RESIDENT, W_FOOTPRINT, W_LIFETIME_MAX = 2, 3, 4, 5, 8, 9, 30


def all_pids() -> list:
    cap = 8192
    while True:
        buf = (c_int * cap)()
        n = libc.proc_listallpids(buf, ctypes.sizeof(buf))
        if n < 0:
            raise OSError(ctypes.get_errno(), "proc_listallpids")
        if n < cap:
            return [p for p in buf[:n] if p > 0]
        cap *= 2


def bsdinfo(pid: int):
    """(ppid, comm, start_us) or None when the process is gone or unreadable."""
    buf = ctypes.create_string_buffer(BSDINFO_SIZE)
    if libc.proc_pidinfo(pid, PROC_PIDTBSDINFO, 0, buf, BSDINFO_SIZE) != BSDINFO_SIZE:
        return None
    words = struct.unpack_from("<12I", buf.raw, 0)
    comm = buf.raw[48:64].split(b"\0", 1)[0].decode("utf-8", "replace")
    sec, usec = struct.unpack_from("<QQ", buf.raw, 120)
    return words[4], comm, sec * 1_000_000 + usec


def pid_path(pid: int) -> str:
    buf = ctypes.create_string_buffer(4096)
    n = libc.proc_pidpath(pid, buf, 4096)
    return buf.raw[:n].decode("utf-8", "replace") if n > 0 else ""


def rusage(pid: int):
    """dict with footprint/resident bytes, cpu_ns, wakeups; None when unreadable."""
    info = (c_uint64 * 64)()
    if libc.proc_pid_rusage(pid, RUSAGE_INFO_V4, info) != 0:
        return None
    ticks = info[W_USER] + info[W_SYS]
    return {
        "footprint": info[W_FOOTPRINT],
        "resident": info[W_RESIDENT],
        "lifetime_max": info[W_LIFETIME_MAX],
        "cpu_ns": ticks * _NUMER // _DENOM,
        "wakeups": info[W_IDLE_WK] + info[W_INT_WK],
    }


_responsible = None
try:
    _responsible = libc.responsibility_get_pid_responsible_for_pid
    _responsible.argtypes = [c_int]
    _responsible.restype = c_int
except AttributeError:
    _responsible = None


def responsible_pid(pid: int):
    if _responsible is None:
        return None
    r = _responsible(pid)
    return r if r > 0 else None


def snapshot() -> dict:
    """pid -> (ppid, comm, start_us, path) for every readable process."""
    out = {}
    for pid in all_pids():
        info = bsdinfo(pid)
        if info:
            out[pid] = (info[0], info[1], info[2], pid_path(pid))
    return out


# ---------------------------------------------------------------- spawn (responsibility disclaimed)

libc.posix_spawnattr_init.argtypes = [POINTER(c_void_p)]
libc.posix_spawnattr_setflags.argtypes = [POINTER(c_void_p), ctypes.c_short]
libc.posix_spawnattr_setpgroup.argtypes = [POINTER(c_void_p), c_int]
libc.posix_spawnattr_destroy.argtypes = [POINTER(c_void_p)]
libc.posix_spawn_file_actions_init.argtypes = [POINTER(c_void_p)]
libc.posix_spawn_file_actions_addopen.argtypes = [POINTER(c_void_p), c_int, c_char_p, c_int, c_int]
libc.posix_spawn_file_actions_adddup2.argtypes = [POINTER(c_void_p), c_int, c_int]
libc.posix_spawn_file_actions_destroy.argtypes = [POINTER(c_void_p)]
libc.posix_spawn.argtypes = [POINTER(c_int32), c_char_p, POINTER(c_void_p), POINTER(c_void_p), POINTER(c_char_p), POINTER(c_char_p)]
libc.posix_spawn.restype = c_int
POSIX_SPAWN_SETPGROUP = 0x0002
POSIX_SPAWN_CLOEXEC_DEFAULT = 0x4000

_disclaim = None
try:
    _disclaim = libc.responsibility_spawnattrs_setdisclaim
    _disclaim.argtypes = [POINTER(c_void_p), c_int]
    _disclaim.restype = c_int
except AttributeError:
    _disclaim = None


def spawn(argv: list, env: dict, stdout_path: str, cwd_unused=None):
    """posix_spawn argv[0] (an absolute path) with exactly `env`, stdin from /dev/null, stdout and
    stderr to `stdout_path`, in a process group of its own, with responsibility disclaimed when the
    SPI exists. Returns (pid, disclaimed)."""
    attr = c_void_p()
    fa = c_void_p()
    libc.posix_spawnattr_init(byref(attr))
    libc.posix_spawn_file_actions_init(byref(fa))
    try:
        libc.posix_spawnattr_setflags(byref(attr), POSIX_SPAWN_SETPGROUP | POSIX_SPAWN_CLOEXEC_DEFAULT)
        libc.posix_spawnattr_setpgroup(byref(attr), 0)
        disclaimed = bool(_disclaim) and _disclaim(byref(attr), 1) == 0
        libc.posix_spawn_file_actions_addopen(byref(fa), 0, b"/dev/null", os.O_RDONLY, 0)
        libc.posix_spawn_file_actions_addopen(byref(fa), 1, stdout_path.encode(), os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o644)
        libc.posix_spawn_file_actions_adddup2(byref(fa), 1, 2)
        args = (c_char_p * (len(argv) + 1))(*[a.encode() for a in argv], None)
        envp = (c_char_p * (len(env) + 1))(*[f"{k}={v}".encode() for k, v in env.items()], None)
        pid = c_int32()
        rc = libc.posix_spawn(byref(pid), argv[0].encode(), byref(fa), byref(attr), args, envp)
        if rc != 0:
            raise OSError(rc, f"posix_spawn {argv[0]}")
        return pid.value, disclaimed
    finally:
        libc.posix_spawn_file_actions_destroy(byref(fa))
        libc.posix_spawnattr_destroy(byref(attr))


# ---------------------------------------------------------------- CoreFoundation helpers

CF.CFStringCreateWithCString.argtypes = [c_void_p, c_char_p, c_uint32]
CF.CFStringCreateWithCString.restype = c_void_p
CF.CFStringGetCString.argtypes = [c_void_p, c_char_p, c_long, c_uint32]
CF.CFStringGetCString.restype = c_bool
CF.CFArrayGetCount.argtypes = [c_void_p]
CF.CFArrayGetCount.restype = c_long
CF.CFArrayGetValueAtIndex.argtypes = [c_void_p, c_long]
CF.CFArrayGetValueAtIndex.restype = c_void_p
CF.CFDictionaryGetValue.argtypes = [c_void_p, c_void_p]
CF.CFDictionaryGetValue.restype = c_void_p
CF.CFNumberGetValue.argtypes = [c_void_p, c_int, c_void_p]
CF.CFNumberGetValue.restype = c_bool
CF.CFBooleanGetValue.argtypes = [c_void_p]
CF.CFBooleanGetValue.restype = c_bool
CF.CFRelease.argtypes = [c_void_p]
UTF8 = 0x08000100
kCFNumberSInt64Type = 4


def _cfstr(s: str):
    return CF.CFStringCreateWithCString(None, s.encode(), UTF8)


def _str(ref) -> str:
    if not ref:
        return ""
    buf = ctypes.create_string_buffer(2048)
    return buf.value.decode("utf-8", "replace") if CF.CFStringGetCString(ref, buf, 2048, UTF8) else ""


def _num(ref) -> int:
    v = c_int64(0)
    if ref:
        CF.CFNumberGetValue(ref, kCFNumberSInt64Type, byref(v))
    return v.value


# ---------------------------------------------------------------- windows

class CGPoint(Structure):
    _fields_ = [("x", c_double), ("y", c_double)]


class CGSize(Structure):
    _fields_ = [("width", c_double), ("height", c_double)]


class CGRect(Structure):
    _fields_ = [("origin", CGPoint), ("size", CGSize)]


CG.CGWindowListCopyWindowInfo.argtypes = [c_uint32, c_uint32]
CG.CGWindowListCopyWindowInfo.restype = c_void_p
CG.CGRectMakeWithDictionaryRepresentation.argtypes = [c_void_p, POINTER(CGRect)]
CG.CGRectMakeWithDictionaryRepresentation.restype = c_bool
_K = {k: c_void_p.in_dll(CG, k).value for k in (
    "kCGWindowOwnerPID", "kCGWindowNumber", "kCGWindowLayer", "kCGWindowIsOnscreen", "kCGWindowName", "kCGWindowBounds")}


def windows(pid=None, onscreen_only=False) -> list:
    """Windows front to back: dicts id, pid, layer, onscreen, title, x, y, w, h.
    Titles of other processes' windows need the Screen Recording permission (else "")."""
    arr = CG.CGWindowListCopyWindowInfo(1 if onscreen_only else 0, 0)
    out = []
    if not arr:
        return out
    try:
        for i in range(CF.CFArrayGetCount(arr)):
            d = CF.CFArrayGetValueAtIndex(arr, i)
            owner = _num(CF.CFDictionaryGetValue(d, _K["kCGWindowOwnerPID"]))
            if pid is not None and owner != pid:
                continue
            on = CF.CFDictionaryGetValue(d, _K["kCGWindowIsOnscreen"])
            rect = CGRect()
            b = CF.CFDictionaryGetValue(d, _K["kCGWindowBounds"])
            if b:
                CG.CGRectMakeWithDictionaryRepresentation(b, byref(rect))
            out.append({
                "id": _num(CF.CFDictionaryGetValue(d, _K["kCGWindowNumber"])),
                "pid": owner,
                "layer": _num(CF.CFDictionaryGetValue(d, _K["kCGWindowLayer"])),
                "onscreen": bool(on) and CF.CFBooleanGetValue(on),
                "title": _str(CF.CFDictionaryGetValue(d, _K["kCGWindowName"])),
                "x": rect.origin.x, "y": rect.origin.y, "w": rect.size.width, "h": rect.size.height,
            })
    finally:
        CF.CFRelease(arr)
    return out


def main_window(pid: int):
    """The process's largest on-screen normal-layer window, or None."""
    ws = [w for w in windows(pid) if w["layer"] == 0 and w["onscreen"] and w["w"] > 150]
    return max(ws, key=lambda w: w["w"] * w["h"]) if ws else None


def frontmost_pid(timeout: float = 5):
    """The frontmost application's pid from Launch Services (lsappinfo; no Apple event to any app)."""
    try:
        asn = subprocess.run(["/usr/bin/lsappinfo", "front"], capture_output=True, text=True, timeout=timeout).stdout.strip()
        if not asn:
            return None
        # `lsappinfo front` prints the low half without its 0x ("ASN:0x0-1a6d06b6:"), a form that
        # `lsappinfo info` does not resolve (it prints nothing); the canonical form is "ASN:0x0-0x1a6d06b6:".
        m = re.match(r"^ASN:0x([0-9a-fA-F]+)-(?:0x)?([0-9a-fA-F]+):?$", asn)
        if not m:
            return None
        asn = f"ASN:0x{m.group(1)}-0x{m.group(2)}:"
        info = subprocess.run(["/usr/bin/lsappinfo", "info", "-only", "pid", asn], capture_output=True, text=True, timeout=timeout).stdout
        m = re.search(r"pid\"?\s*=\s*(\d+)", info)
        return int(m.group(1)) if m else None
    except (subprocess.SubprocessError, OSError):
        return None


def capture_window(window_id: int, path: str) -> str:
    """A picture of ONE window by its id (screencapture -l; -x no sound, -o no shadow). Never the
    screen, never another window: the caller checks that the window's owner is the measured app.
    Returns "" on success, else the error text."""
    try:
        r = subprocess.run(["/usr/sbin/screencapture", "-x", "-o", "-l", str(int(window_id)), path],
                           capture_output=True, text=True, timeout=20)
    except (subprocess.SubprocessError, OSError) as e:
        return repr(e)
    if r.returncode != 0 or not os.path.exists(path) or os.path.getsize(path) == 0:
        return f"rc {r.returncode}: {r.stderr.strip()}"
    return ""


# ---------------------------------------------------------------- input posted to one PID

CG.CGEventCreateKeyboardEvent.argtypes = [c_void_p, c_uint16, c_bool]
CG.CGEventCreateKeyboardEvent.restype = c_void_p
CG.CGEventSetFlags.argtypes = [c_void_p, c_uint64]
CG.CGEventKeyboardSetUnicodeString.argtypes = [c_void_p, c_ulong, POINTER(c_uint16)]
CG.CGEventPostToPid.argtypes = [c_int32, c_void_p]
CG.CGEventCreateScrollWheelEvent2.argtypes = [c_void_p, c_uint32, c_uint32, c_int32, c_int32, c_int32]
CG.CGEventCreateScrollWheelEvent2.restype = c_void_p
CG.CGEventSetLocation.argtypes = [c_void_p, CGPoint]

FLAGS = {"shift": 0x20000, "ctrl": 0x40000, "alt": 0x80000, "cmd": 0x100000}
# ANSI (US) virtual key codes.
KEYS = {
    "a": 0, "s": 1, "d": 2, "f": 3, "h": 4, "g": 5, "z": 6, "x": 7, "c": 8, "v": 9, "b": 11, "q": 12, "w": 13,
    "e": 14, "r": 15, "y": 16, "t": 17, "1": 18, "2": 19, "3": 20, "4": 21, "6": 22, "5": 23, "=": 24, "9": 25,
    "7": 26, "-": 27, "8": 28, "0": 29, "]": 30, "o": 31, "u": 32, "[": 33, "i": 34, "p": 35, "enter": 36,
    "l": 37, "j": 38, "'": 39, "k": 40, ";": 41, "\\": 42, ",": 43, "/": 44, "n": 45, "m": 46, ".": 47,
    "tab": 48, "space": 49, "`": 50, "backspace": 51, "escape": 53, "home": 115, "pageup": 116, "end": 119,
    "pagedown": 121, "left": 123, "right": 124, "down": 125, "up": 126,
}
SHIFTED = {"_": "-", "+": "=", ":": ";", '"': "'", "<": ",", ">": ".", "?": "/", "|": "\\", "{": "[", "}": "]", "~": "`",
           "!": "1", "@": "2", "#": "3", "$": "4", "%": "5", "^": "6", "&": "7", "*": "8", "(": "9", ")": "0"}


def _post_key(pid: int, code: int, flags: int, char: str | None):
    for down in (True, False):
        e = CG.CGEventCreateKeyboardEvent(None, code, down)
        if not e:
            raise OSError("CGEventCreateKeyboardEvent failed")
        CG.CGEventSetFlags(e, flags)
        if char is not None:
            units = char.encode("utf-16-le")
            arr = (c_uint16 * (len(units) // 2)).from_buffer_copy(units)
            CG.CGEventKeyboardSetUnicodeString(e, len(units) // 2, arr)
        CG.CGEventPostToPid(pid, e)
        CF.CFRelease(e)


def post_chord(pid: int, chord: str):
    """`cmd-shift-o`, `ctrl-alt-shift-m`, `pagedown`, `escape`: posted to `pid` only."""
    parts = chord.split("-")
    # a trailing "-" key ("cmd--") splits into an empty last part
    key = parts[-1] if parts[-1] else "-"
    mods = [p for p in parts[:-1] if p]
    flags = 0
    for m in mods:
        flags |= FLAGS[m]
    _post_key(pid, KEYS[key], flags, None)


def type_text(pid: int, text: str, gap_s: float = 0.04):
    """Each character as a key press with its US key code and the character itself."""
    import time
    for ch in text:
        if ch == " ":
            code, flags = KEYS["space"], 0
        elif ch.isalpha() and ch.isascii():
            code, flags = KEYS[ch.lower()], (FLAGS["shift"] if ch.isupper() else 0)
        elif ch in KEYS:
            code, flags = KEYS[ch], 0
        elif ch in SHIFTED:
            code, flags = KEYS[SHIFTED[ch]], FLAGS["shift"]
        else:
            raise ValueError(f"cannot type {ch!r}")
        _post_key(pid, code, flags, ch)
        time.sleep(gap_s)


def scroll(pid: int, x: float, y: float, lines: int):
    """Wheel event at screen point (x, y), posted to `pid` only (negative lines scroll down)."""
    e = CG.CGEventCreateScrollWheelEvent2(None, 1, 1, lines, 0, 0)
    CG.CGEventSetLocation(e, CGPoint(x, y))
    CG.CGEventPostToPid(pid, e)
    CF.CFRelease(e)


# ---------------------------------------------------------------- accessibility: read only

AS.AXUIElementCreateApplication.argtypes = [c_int32]
AS.AXUIElementCreateApplication.restype = c_void_p
AS.AXUIElementCopyAttributeValue.argtypes = [c_void_p, c_void_p, POINTER(c_void_p)]
AS.AXUIElementCopyAttributeValue.restype = c_int32
CF.CFGetTypeID.argtypes = [c_void_p]
CF.CFGetTypeID.restype = c_ulong
CF.CFStringGetTypeID.restype = c_ulong
CF.CFNumberGetTypeID.restype = c_ulong


def _ax_attr(el, name: str):
    k = _cfstr(name)
    v = c_void_p()
    try:
        err = AS.AXUIElementCopyAttributeValue(el, k, byref(v))
    finally:
        CF.CFRelease(k)
    return v.value if err == 0 else None


def _ax_text(ref):
    if not ref:
        return None
    try:
        t = CF.CFGetTypeID(ref)
        if t == CF.CFStringGetTypeID():
            buf = ctypes.create_string_buffer(8192)
            return buf.value.decode("utf-8", "replace")[:300] if CF.CFStringGetCString(ref, buf, 8192, UTF8) else None
        if t == CF.CFNumberGetTypeID():
            return str(_num(ref))
        return None
    finally:
        CF.CFRelease(ref)


def ax_focused(pid: int) -> dict:
    """Role, title, value and description of the app's focused element and focused window, read
    through the Accessibility API (nothing is sent to the app). Empty fields when unavailable."""
    app = AS.AXUIElementCreateApplication(pid)
    if not app:
        return {}
    out = {}
    try:
        for label, attr in (("focused", "AXFocusedUIElement"), ("window", "AXFocusedWindow")):
            el = _ax_attr(app, attr)
            if not el:
                out[label] = None
                continue
            try:
                d = {k: _ax_text(_ax_attr(el, "AX" + k)) for k in ("Role", "Subrole", "Title", "Value", "Description", "NumberOfCharacters")}
                # A short value (a search field) is evidence; a long one is a document's text: not kept.
                if d["Value"] is not None and len(d["Value"]) > 64:
                    d["Value"] = f"<{len(d['Value'])}+ characters, not kept>"
                out[label] = d
            finally:
                CF.CFRelease(el)
    finally:
        CF.CFRelease(app)
    return out


# ---------------------------------------------------------------- session and permissions

CG.CGSessionCopyCurrentDictionary.restype = c_void_p
CG.CGMainDisplayID.restype = c_uint32
CG.CGDisplayIsAsleep.argtypes = [c_uint32]
CG.CGDisplayIsAsleep.restype = c_int
AS.AXIsProcessTrusted.restype = c_bool
try:
    CG.CGPreflightScreenCaptureAccess.restype = c_bool
    _screen_access = CG.CGPreflightScreenCaptureAccess
except AttributeError:
    _screen_access = None


def session_state() -> dict:
    """locked / on_console / display_asleep. A missing session dictionary counts as locked."""
    d = CG.CGSessionCopyCurrentDictionary()
    locked, console = True, False
    if d:
        try:
            k = _cfstr("CGSSessionScreenIsLocked")
            v = CF.CFDictionaryGetValue(d, k)
            locked = bool(v) and CF.CFBooleanGetValue(v)
            CF.CFRelease(k)
            k = _cfstr("kCGSSessionOnConsoleKey")
            v = CF.CFDictionaryGetValue(d, k)
            console = bool(v) and CF.CFBooleanGetValue(v)
            CF.CFRelease(k)
        finally:
            CF.CFRelease(d)
    return {"locked": locked, "on_console": console, "display_asleep": bool(CG.CGDisplayIsAsleep(CG.CGMainDisplayID()))}


def permissions() -> dict:
    return {"accessibility": bool(AS.AXIsProcessTrusted()), "screen_recording": bool(_screen_access()) if _screen_access else None,
            "responsible_spi": _responsible is not None, "disclaim_spi": _disclaim is not None}


if __name__ == "__main__":
    import json
    import sys
    cmd = sys.argv[1] if len(sys.argv) > 1 else "session"
    if cmd == "session":
        print(json.dumps({**session_state(), **permissions(), "frontmost": frontmost_pid()}))
    elif cmd == "ax":
        print(json.dumps(ax_focused(int(sys.argv[2])), indent=1))
    elif cmd == "windows":
        print(json.dumps(windows(int(sys.argv[2])), indent=1))
    else:
        sys.exit(f"unknown command {cmd}")
