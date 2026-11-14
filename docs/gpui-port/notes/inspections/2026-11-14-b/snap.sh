#!/bin/sh
# usage: snap.sh <name>   — captures the app's on-screen windows
I="$(cd "$(dirname "$0")" && pwd)"; PID=$(cat "$I/app.pid"); n=0
swift "$I/../inspect1/windows.swift" "$PID" 2>/dev/null | while read id layer onscreen w h; do
  [ "$onscreen" = "true" ] || continue
  [ "${w%.*}" -gt 150 ] 2>/dev/null || continue
  n=$((n+1)); screencapture -x -o -l "$id" "$I/shots/$1-$n.png" && sips -Z 1500 "$I/shots/$1-$n.png" >/dev/null 2>&1 && echo "$1-$n.png ${w}x${h}"
done
