import CoreGraphics
import Foundation
// usage: click <pid> <x> <y> left|right|where   (x, y in points from the window's top-left corner)
// Posts the mouse events to that process only; the real pointer does not move.
let a = CommandLine.arguments
let pid = pid_t(a[1])!
let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]] ?? []
var origin: CGPoint? = nil
var size = CGSize.zero
for w in list {
    guard let owner = w[kCGWindowOwnerPID as String] as? Int32, owner == pid, (w[kCGWindowLayer as String] as? Int ?? 1) == 0 else { continue }
    let b = w[kCGWindowBounds as String] as? [String: Any] ?? [:]
    let width = b["Width"] as? Double ?? 0
    if width > 150 && origin == nil { origin = CGPoint(x: b["X"] as? Double ?? 0, y: b["Y"] as? Double ?? 0); size = CGSize(width: width, height: b["Height"] as? Double ?? 0) }
}
guard let o = origin else { print("no window"); exit(1) }
let pointer = CGEvent(source: nil)!.location
print("window \(o.x),\(o.y) \(size.width)x\(size.height) pointer-in-window \(pointer.x - o.x),\(pointer.y - o.y)")
if a[4] == "where" { exit(0) }
let p = CGPoint(x: o.x + Double(a[2])!, y: o.y + Double(a[3])!)
let src = CGEventSource(stateID: .combinedSessionState)
func post(_ t: CGEventType, _ b: CGMouseButton) {
    let e = CGEvent(mouseEventSource: src, mouseType: t, mouseCursorPosition: p, mouseButton: b)!
    e.postToPid(pid); usleep(80000)
}
post(.mouseMoved, .left)
if a[4] == "right" { post(.rightMouseDown, .right); post(.rightMouseUp, .right) } else { post(.leftMouseDown, .left); post(.leftMouseUp, .left) }
