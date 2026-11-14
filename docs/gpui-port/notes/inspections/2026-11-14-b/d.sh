#!/bin/sh
I="$(cd "$(dirname "$0")" && pwd)"; exec osascript "$I/drive.applescript" "$(cat "$I/app.pid")" "$@"
