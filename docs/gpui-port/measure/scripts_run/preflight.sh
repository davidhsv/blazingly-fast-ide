#!/bin/sh
# usage: preflight.sh <base>
# The automated part of the go / no-go list (DESIGN.md, "Go / no-go"). Reads only: the base
# folder, the process list, the session state. Exit 0 = go, anything else = no-go.
set -u
base=${1:?usage: preflight.sh <base>}
here=$(cd "$(dirname "$0")" && pwd)
fail=0
no() { echo "NO-GO: $*"; fail=1; }
# Processes running from the base (their command line names a path under it), except this script,
# its ancestors and its own subshells (same command line, parent in that chain), whose arguments
# name the base (e.g. --runs-root <base>/evidence). The apps and measure.py are never excluded.
from_base() {
  anc=" $$ "; p=$$
  while [ -n "$p" ] && [ "$p" -gt 1 ]; do p=$(ps -o ppid= -p "$p" | tr -d ' '); anc="$anc$p "; done
  self=$(ps -o command= -p $$)
  ps -axo pid=,ppid=,command= | grep -F "$base/" | grep -v grep | awk -v a="$anc" -v self="$self" '{
    pid = $1; ppid = $2; cmd = $0; sub(/^ *[0-9]+ +[0-9]+ /, "", cmd)
    if (index(a, " " pid " ")) next
    if (cmd == self && index(a, " " ppid " ")) next
    print pid, cmd }'
}

ok() { echo "ok: $*"; }
case "$base" in /private/tmp/openidea-measure-[A-Za-z0-9][A-Za-z0-9][A-Za-z0-9][A-Za-z0-9][A-Za-z0-9][A-Za-z0-9]*) ok "base $base";; *) no "base must be /private/tmp/openidea-measure-<random>";; esac
[ -f "$base/config.json" ] || no "no $base/config.json (run build.sh)"
py=/usr/bin/python3
tauri=$($py -I -c 'import json,sys; print(json.load(open(sys.argv[1]))["tauri_bin"])' "$base/config.json" 2>/dev/null)
native=$($py -I -c 'import json,sys; print(json.load(open(sys.argv[1]))["native_bin"])' "$base/config.json" 2>/dev/null)
for b in "$tauri" "$native"; do case "$b" in "$base"/*) [ -x "$b" ] && ok "binary $b" || no "missing $b";; *) no "binary outside the base: $b";; esac; done
[ -n "$tauri" ] && { strings "$tauri" | grep -q 'ai.l0ng.openidea.measure' && ok "Tauri binary carries ai.l0ng.openidea.measure" || no "Tauri binary lacks the measure identifier"; }
# Nothing of ours still running; the single-instance socket of the measure identity is not held.
running=$(from_base)
[ -z "$running" ] && ok "no process from the base is running" || no "processes from the base are running: $running"
# Holder check by our pids' unix sockets (`lsof -t <socket>` finds no holder of a unix socket path).
# No pid from the base running means none of ours holds it; lsof is never run without -p pids.
sock=/private/tmp/ai_l0ng_openidea_measure_si.sock
if [ -e "$sock" ]; then
  pids=$(from_base | awk '{print $1}' | paste -sd, -)
  holder=""
  # lsof names the socket by the path it was bound with (/tmp/..., not /private/tmp/...): match the file name.
  [ -n "$pids" ] && holder=$(lsof -a -p "$pids" -U 2>/dev/null | grep -F "/$(basename "$sock")")
  [ -z "$holder" ] && echo "note: stale $sock (no holder among our pids): run_all.sh removes it" || no "$sock is held: $holder"
fi
# The owner's app: listed, never touched.
owner=$(ps -axo pid=,command= | grep -F "/Applications/Blazingly Fast IDE.app/" | grep -v grep)
[ -n "$owner" ] && echo "owner's app running (left alone): $owner" || echo "owner's app not running now"
# Session, permissions, SPIs.
state=$($py -I "$here/macos.py" session)
echo "session: $state"
echo "$state" | grep -q '"locked": false' && ok "screen unlocked" || no "screen locked or no GUI session"
echo "$state" | grep -q '"display_asleep": false' && ok "display awake" || no "display asleep"
echo "$state" | grep -q '"accessibility": true' && ok "Accessibility (posting events)" || no "this terminal lacks Accessibility: events cannot be posted"
echo "$state" | grep -q '"screen_recording": true' && ok "Screen Recording (window titles)" || no "this terminal lacks Screen Recording: window titles read as empty, the open-file waits cannot work"
echo "$state" | grep -q '"responsible_spi": true' && ok "responsibility SPI" || no "no responsibility SPI: WebKit helpers cannot be attributed"
echo "$state" | grep -q '"disclaim_spi": true' && ok "disclaim SPI" || echo "warning: no disclaim SPI: helper attribution falls back to start time (DESIGN.md R5)"
# The project clone: same APFS volume, exists, the owner's .openidea moved aside (if so decided).
p="$base/project/Allocadia"
[ -d "$p/.git" ] && ok "clone $p" || no "no clone at $p (run_all.sh makes it)"
[ -e "$p/.openidea" ] && echo "note: the clone keeps the owner's .openidea (decision D4)" || ok "clone has no .openidea"
[ -f "$base/files.json" ] && ok "files.json: $(wc -l < "$base/files.json") lines" || no "no files.json (measure.py --select-files)"
# Language servers the two apps could find (both search PATH and /opt/homebrew/bin, /usr/local/bin).
for s in jdtls lemminx kmp-lsp vtsls typescript-language-server basedpyright pyright; do
  for d in /usr/bin /bin /usr/sbin /sbin /opt/homebrew/bin /usr/local/bin; do [ -x "$d/$s" ] && echo "language server found: $d/$s"; done
done
free_gib=$(df -g /private/tmp | awk 'NR==2 {print $4}'); [ "$free_gib" -ge 20 ] && ok "free ${free_gib} GiB" || no "less than 20 GiB free"
[ $fail -eq 0 ] && echo "GO (the manual items of DESIGN.md still apply)" || echo "NO-GO"
exit $fail
