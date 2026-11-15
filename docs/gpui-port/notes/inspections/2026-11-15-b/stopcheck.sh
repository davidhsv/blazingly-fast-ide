#!/bin/sh
# Launches the debug binary with SIGINT ignored (started with & from a script), runs serve.sh from its context,
# presses Stop once, and reports whether the script's sleep is still alive.
I="$(cd "$(dirname "$0")" && pwd)"; P="$I/project"
BIN=/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/app-gpui/target/debug/openidea-gpui
home="$I/home"; rm -rf "$home" "$I/stop.log"; mkdir -p "$home/data"
printf '{"projects":["%s"],"locations":[],"restricted":[]}' "$(cd "$P" && pwd -P)" > "$home/data/project-trust.json"
prev=$(osascript -e 'tell application "System Events" to get unix id of first process whose frontmost is true')
trap '' INT
HOME="$I/fakehome" ZDOTDIR="$I/fakehome" OPENIDEA_GPUI_HOME="$home" OPENIDEA_GPUI_INSPECT="$I/stop.script" OPENIDEA_GPUI_INSPECT_LOG="$I/stop.log" "$BIN" "$(cd "$P" && pwd -P)" > "$I/stop.stdout" 2>&1 &
pid=$!; seen=0; start=$(date +%s)
while kill -0 $pid 2>/dev/null; do
  [ $(( $(date +%s) - start )) -gt 60 ] && { echo "time limit"; break; }
  n=$(grep -c '^mark' "$I/stop.log" 2>/dev/null); n=${n:-0}
  while [ "$seen" -lt "$n" ]; do
    seen=$((seen+1)); mark=$(grep '^mark' "$I/stop.log" | sed -n "${seen}p" | cut -f2)
    echo "at $mark: sleep processes = $(pgrep -f 'sleep 61.37' | wc -l | tr -d ' ')"
    "$I/windows" $pid | while read id layer onscreen w h; do
      [ "$onscreen" = true ] || continue; [ "${w%.*}" -gt 150 ] 2>/dev/null || continue
      screencapture -x -o -l "$id" "$I/shots/stop-$mark.png" && sips -Z 1500 "$I/shots/stop-$mark.png" >/dev/null 2>&1
    done
  done
  grep -q '^done' "$I/stop.log" 2>/dev/null && break
  sleep 0.2
done
kill $pid 2>/dev/null; sleep 1; kill -0 $pid 2>/dev/null && kill -9 $pid
osascript -e "tell application \"System Events\" to set frontmost of (first process whose unix id is $prev) to true" 2>/dev/null
echo "after quit: sleep processes = $(pgrep -f 'sleep 61.37' | wc -l | tr -d ' ')"; pkill -f 'sleep 61.37' 2>/dev/null
grep -E '^(fail|note|done)' "$I/stop.log"
