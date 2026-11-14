-- usage: osascript drive.applescript <pid> <verb> <args...>
-- verbs: front | key <keycode> [mods] | text <string> [mods] | click <x> <y> | bounds
on run argv
  set thePid to (item 1 of argv) as integer
  set verb to item 2 of argv
  tell application "System Events"
    set p to first process whose unix id is thePid
    if verb is "bounds" then
      set w to window 1 of p
      set {x, y} to position of w
      set {ww, hh} to size of w
      return (x as text) & " " & (y as text) & " " & (ww as text) & " " & (hh as text)
    end if
    set frontmost of p to true
    delay 0.35
    set f to unix id of (first process whose frontmost is true)
    if f is not thePid then error "target is not frontmost (" & f & ")"
    if verb is "front" then return "ok"
    set mods to {}
    if (count of argv) > 3 then
      set m to item 4 of argv
      if m contains "cmd" then set end of mods to command down
      if m contains "shift" then set end of mods to shift down
      if m contains "alt" then set end of mods to option down
      if m contains "ctrl" then set end of mods to control down
    end if
    if verb is "key" then
      key code ((item 3 of argv) as integer) using mods
    else if verb is "text" then
      keystroke (item 3 of argv) using mods
    else if verb is "click" then
      click at {(item 3 of argv) as integer, (item 4 of argv) as integer}
    end if
  end tell
  return "ok"
end run
