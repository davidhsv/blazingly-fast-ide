#!/bin/sh
# usage: run_all.sh <base> [--dry-run] [--reps N] [--keep-openidea] [--allow-activate] [--ax-probe] [--capture]
#                   [--runs-root <folder under the base>] [--idle-s S]
# The campaign: an APFS clone of the project under the base, the 10 files, the preflight, then
# N repetitions per app alternating which app runs first (rep 1: native then tauri, rep 2: tauri
# then native, ...), a check of what each run wrote into the clone, and the aggregate table.
# The display is kept awake by caffeinate, which exits with this script (-w).
set -u
base=${1:?usage: run_all.sh <base> [--dry-run] [--reps N] [--keep-openidea] [--allow-activate]}
shift
dry=""; reps=3; keep=""; act=""; ax=""; cap=""; runs="$base/runs"; idle=600
while [ $# -gt 0 ]; do
  case "$1" in
    --dry-run) dry=--dry-run;; --reps) reps=$2; shift;; --keep-openidea) keep=1;; --allow-activate) act=--allow-activate;; --ax-probe) ax=--ax-probe;;
    --capture) cap=--capture;; --runs-root) runs=$2; shift;; --idle-s) idle=$2; shift;;
    *) echo "unknown option $1"; exit 2;;
  esac; shift
done
case "$runs" in "$base"/*) ;; *) echo "the runs root must be under the base: $runs"; exit 2;; esac
here=$(cd "$(dirname "$0")" && pwd)
py="/usr/bin/python3 -I"
clone="$base/project/Allocadia"
case "$base" in /private/tmp/openidea-measure-[A-Za-z0-9][A-Za-z0-9][A-Za-z0-9][A-Za-z0-9][A-Za-z0-9][A-Za-z0-9]*) ;; *) echo "base must be /private/tmp/openidea-measure-<random>"; exit 2;; esac
say() { echo "== $*"; }
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

do_() { if [ -n "$dry" ]; then echo "[dry-run] $*"; else echo "+ $*"; sh -c "$*"; fi; }

# 1. The clone was made and detached from the original beforehand; it is never made again here
# (a copy of the original would bring back its worktree pointers and outside symlinks).
[ -d "$clone/.git" ] || { echo "no-go: no clone at $clone"; exit 2; }
do_ "$py '$here/measure.py' --base '$base' --select-files"

# 2. Preflight (no-go stops here).
if [ -z "$dry" ]; then "$here/preflight.sh" "$base" || exit 2; else echo "[dry-run] $here/preflight.sh $base"; fi
sock=/private/tmp/ai_l0ng_openidea_measure_si.sock
# Held = one of our pids (anything executing from the base) has it among its unix sockets.
# lsof is never run without a pid list.
sock_held() {
  ps_pids=$(from_base | awk '{print $1}' | paste -sd, -)
  # lsof names the socket by the path it was bound with (/tmp/..., not /private/tmp/...): match the file name.
  [ -n "$ps_pids" ] && lsof -a -p "$ps_pids" -U 2>/dev/null | grep -qF "/$(basename "$sock")"
}
[ -e "$sock" ] && ! sock_held && do_ "rm -f '$sock'"

# 3. Keep the display awake for as long as this script lives (no setting is changed).
if [ -z "$dry" ]; then /usr/bin/caffeinate -d -i -m -w $$ & echo "caffeinate pid $!"; else echo "[dry-run] caffeinate -d -i -m -w $$ &"; fi

# 4. The runs.
i=1
while [ "$i" -le "$reps" ]; do
  if [ $((i % 2)) -eq 1 ]; then order="native tauri"; else order="tauri native"; fi
  for app in $order; do
    say "rep $i: $app"
    do_ "$py '$here/measure.py' --base '$base' --project-stamp"
    if [ -n "$dry" ]; then
      $py "$here/measure.py" --base "$base" --app "$app" --rep "$i" --runs-root "$runs" --idle-s "$idle" --dry-run $act $ax $cap
    else
      $py "$here/measure.py" --base "$base" --app "$app" --rep "$i" --runs-root "$runs" --idle-s "$idle" $act $ax $cap; rc=$?
      [ $rc -eq 0 ] || { echo "run $i-$app ended with $rc: see $runs/$i-$app/events.jsonl"; }
      # Our own stale lock, if the app was killed.
      [ -e "$sock" ] && ! sock_held && rm -f "$sock" && echo "removed stale $sock"
      if ! $py "$here/measure.py" --base "$base" --project-check; then
        echo "NO-GO (G11): the run $i-$app wrote into the clone outside .openidea (list above); stopping, nothing is re-cloned"
        $py "$here/stop.py" --base "$base" --runs-root "$runs"
        exit 4
      elif [ -z "$keep" ] && [ -d "$clone/.openidea" ]; then
        # The owner's .openidea was moved out before the first run: what is there now the app wrote
        # (changelists.json, .gitignore). Kept for inspection, then the clone is as before.
        mv "$clone/.openidea" "$runs/$i-$app/openidea-written-into-the-clone"
      fi
      # The machine back at rest before the next app starts.
      sleep 60
    fi
  done
  i=$((i + 1))
done

# 5. Nothing of ours is left running (recorded PIDs, and anything executing from the base).
if [ -z "$dry" ]; then
  $py "$here/stop.py" --base "$base" --runs-root "$runs"
  left=$(from_base)
  [ -z "$left" ] && echo "nothing from the base is running" || echo "STILL RUNNING from the base (stop by these PIDs only): $left"
else
  echo "[dry-run] $here/stop.py --base $base --runs-root $runs; ps -axo pid=,command= | grep -F '$base/'"
fi

# 6. The table.
do_ "$py '$here/aggregate.py' --base '$base' --runs-root '$runs' > '$runs/RESULTS.md'"
say "done: $runs/RESULTS.md"
