# Accessibility in the GPUI app: assessment

`R` = `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f`, `G` = `$R/gpui-pre-0.3.12/src`, `A` = `app-gpui`. Nothing was run. Claims come from the sources unless marked Hypothesis.

## Verdict

The GPUI version in use already exposes an AccessKit tree to macOS. The tree is empty because the app never sets a role, and a node needs both an `.id()` and a role. The fix is app code, with no new dependency or upgrade.

## 1. What GPUI offers

**No feature flag.** `accesskit 0.24` is a plain dependency of gpui-pre (`$R/gpui-pre-0.3.12/Cargo.toml:278`), and its `[features]` (lines 53-65) has no a11y entry. gpui-pre-macos depends on `accesskit_macos 0.26` for the macOS target (`$R/gpui-pre-macos-0.3.12/Cargo.toml:62-67`). `A/Cargo.lock:12-60` locks both. gpui-kit has no a11y feature, and the app uses its defaults (`A/Cargo.toml:23`). The only switches are opt-outs: `Application::new_inaccessible` (`G/app.rs:259`) and `TestAppContext::disable_accessibility` (`G/app/test_context.rs:196`). The app uses neither (grep: 0).

**macOS.** `a11y_init` (`G/window.rs:1735-1760`) installs `accesskit_macos::SubclassingAdapter::for_window` (`$R/gpui-pre-macos-0.3.12/src/window.rs:2406-2435`). The tree is built only after an assistive client activates the adapter (`G/window.rs:1757-1760, 3908-3910, 4007-4030`). The root node is `Role::Window`, labelled with the window title (`G/window/a11y.rs:481-484`).

**Node creation** (`G/element.rs:427-442`). A node needs a `GlobalElementId` (from `.id()`) and `a11y_role() == Some`. Elements without a role are skipped, and their children attach to the nearest ancestor that has a node. `GenericContainer` is filtered out (`G/elements/div.rs:1955-1961`). For text: `Text` is a `Label` only when it has an id (`text!`, `G/elements/text.rs:189-198`); `InteractiveText` is always a `Label` (`:1105-1110`); `&str`, `SharedString` and `StyledText` get no node.

**API** on `pub trait StatefulInteractiveElement` (`div.rs:1353`), so `.id()` must come first:

- `role(accesskit::Role)` (1357), `accessibility_id` (1373)
- `aria_label(impl Into<SharedString>)` (1379), `aria_description` (1388), `aria_keyshortcuts` (1398)
- `aria_active_descendant()` (1418)
- `a11y_synthetic_children(impl FnOnce(&mut A11ySubtreeBuilder))` (1432)
- `aria_selected` / `aria_expanded` / `aria_toggled(Toggled)` (1441-1453)
- numeric value, min, max and step (1459-1493)
- `aria_value` (1473), `aria_placeholder` (1480)
- orientation, level, position and size in set, row and column (1498-1541)
- `on_a11y_action(Action, FnMut(Option<&ActionData>, &mut Window, &mut App))` (1550)

`write_a11y_info` adds `Click` and `Focus` actions automatically (`div.rs:3591-3664`). Unhandled requests fall back to a synthetic click or a focus (`G/window.rs:7348-7374`). Re-exports: `Role`, `AccessibleAction`, `Toggled`, `Orientation`, `accesskit` (`G/gpui.rs:5,114`). On `Window`: `is_a11y_active` (7298) and `debug_a11y_tree_json() -> Option<String>` (7303).

**Missing setters.** There are none for live, disabled or hidden (gpui-base says so for disabled: `$R/gpui-base-0.9.1/src/a11y_disabled.rs:3`). The escape hatch is `builder.parent_node() -> &mut accesskit::Node` (`G/window/a11y.rs:364`), which reaches `set_live` and `set_disabled` (`$R/accesskit-0.24.1/src/lib.rs:2139, 1816`). accesskit_macos announces live-region changes (`$R/accesskit_macos-0.26.3/src/event.rs:35-99`).

**Names.** `Button`, `CheckBox`, `Link`, `MenuItem*` and `RadioButton` take their name from descendant `Label` nodes when they have no label (`$R/accesskit_consumer-0.38.0/src/node.rs:701-716`). Plain-string children are not `Label` nodes, so an explicit `aria_label` is needed.

**The log line** comes from `note_focus_without_node` (`G/window/a11y.rs:193-201`). It is called in two cases: a tracked focus handle with no element id (`div.rs:2386-2394`), or an element with an id but no role (`a11y.rs:249-254`). Both run only under `window.a11y.is_active()` (`div.rs:2380`). So the observed log proves the macOS adapter was active. The tree held almost nothing: the root and any `InteractiveText` labels.

**gpui-kit components already set roles**, for example gpui-base `Button` (`button.rs:237-269`), `Select` (`select.rs:205-225`), `Tree` (`tree.rs:463-553`) and `Toast` as `Alert` (`toast.rs:656`). `Root::new` installs a macOS `accessibilityHitTest:` forwarder (`gpui-base/src/root.rs:175-177`). The app uses none of these and no `Root`; it imports only `assets`.

## 2. What the native app does today

`A/src` (1,015 files) has 0 uses of: `.role(`, `aria_*(`, `on_a11y_action`, `accesskit`, `Role::`, `text!(`, `debug_a11y_tree_json`, `.test_support()`. The only nodes come from 6 `InteractiveText::new` sites, for example `build_window/view.rs:574` and `run/view.rs:1134`.
Shared primitives and their reach:

- `ui/text_field.rs`: the root is a `div()` with `track_focus` and no id (`:375-384`). Hypothesis: this field produced the observed log. To check, read the reason in the app log: "it has no element id" means this case. It is used 78 times in 54 files. `TextArea` (`text_area.rs:932`) is used 9 times and `SecretField` (`secret_field.rs:303`) 3 times.
- `ui/popup_menu.rs`: rows have ids (`:400`), the focused container has none (`:454`). Used in 7 files.
- `ui/tooltip.rs`: `WithTooltip` (`:253-287`) has 52 call sites, plus 44 raw `.tooltip(`. Its text corresponds to the webview's icon-button `aria-label`.
- There is no shared button. There are 67 local `fn icon_button|tool_button|button|…` helpers, plus duplicated button and checkbox helpers in `app_views/mod.rs:67,92`, `database/dialog_parts.rs:65,90` and `settings_view/controls.rs:489,521`.
- Trees: `project_tree.rs`, `database/tree_view.rs`, `everywhere/structure_view.rs` and 3 more.
- Notifications: `workbench/notification_balloons.rs`.

## 3. What the webview exposes

`src/**/*.ts(x)`, as `aria-label` / `role=` / `aria-*`: features 1067/460/1443, ui 91/42/168, editor 54/26/93, workbench 40/23/82, platform 1/8/1. Total 1253/559/1787, or 1202/505/1602 without tests. Top roles: status 59, toolbar 57, alert 57, option 40, listbox 36, treeitem 34.

Primitives in `src/ui/`:

- `Button` / `BarButton`: `<button>`, named by its text.
- `IconButton`: `<button>` with `aria-label` = `ariaLabel ?? title ?? action` (`Button.tsx:77-82`).
- `Checkbox`: `<input>` with `aria-checked` (`Checkbox.tsx:24-31`).
- `Input`: `<input>` with `aria-label` (`Input.tsx:43-51`).
- `Dialog`: `dialog` (`Dialog.tsx:394-396`).
- `List`: `listbox` / `option` (`List.tsx:185-216`).
- `Menu`: `menu` / `menuitem` (`Menu.tsx:311-337`).
- `Select`: `<button aria-haspopup>` (`Select.tsx:289-295`).
- `Tabs`: `tablist` / `tab` (`Tabs.tsx:344-423`).
- `Tree`: `tree` / `treeitem` (`Tree.tsx:630-704`).
- `Tooltip`: `tooltip` (`:135`).

These carry the screen-reader names (`CLAUDE.md:42`), focus order and keyboard activation (native controls, roving `tabIndex`, the focus trap at `Dialog.tsx:206`), active-row announcements (`aria-activedescendant`), and 13 `aria-live` regions such as the notifications stack (`workbench/Notifications.tsx:114`) and find counts (`editor/core/view/FindBar.tsx:634`).

## 4. Plan in slices

**Proof in tests.** The dev-dependency `test-support` (`A/Cargo.toml:78`) gives `TestWindowExt::find(id) -> ElementSnapshot` (`$R/gpui-kit-0.9.1/src/test.rs:33-40`). The snapshot exposes `role()`, `label()`, `value()`, `placeholder()`, `selected()`, `expanded()`, `checked()` and `disabled()` (`gpui-base/src/test_support.rs:73-130`). It reads them from the element's own `a11y_role()` and `write_a11y_info()` (`:487-504`), so no adapter is needed. Wrap the element with `.test_support()` (`observe.rs:20-37`), or use the `canvas` pattern from `gpui-base/src/button.rs:618-672`. A whole tree is not available in tests: `TestWindow` (`G/platform/test/window.rs:317`) keeps the no-op `a11y_init` (`G/platform.rs:1441`).

| #   | Slice                       | Files                                                                                                                                         | Sets                                                                                                                                                                                               | Proof                                                      | Size |
| --- | --------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------- | ---- |
| 1   | Text inputs                 | `ui/text_field.rs`, `text_area.rs`, `secret_field.rs`                                                                                         | `.id(("text-field", cx.entity_id()))` (`G/window.rs:8000`), `TextInput` / `MultilineTextInput` / `PasswordInput`, value (not for secret), placeholder, new `with_label`                            | `find` gives role, value and label; the log line goes away | S    |
| 2   | Popup menu                  | `ui/popup_menu.rs`                                                                                                                            | container id plus `Menu`; rows `MenuItem` / `MenuItemCheckBox` + `aria_toggled`, label, `aria_active_descendant` on the highlighted row                                                            | row snapshot                                               | S    |
| 3   | Buttons                     | new `ui/a11y.rs` trait: `button(label)` = `role(Button).aria_label(label)`, plus `toggle_button` and `checkbox` (`CheckBox` + `aria_toggled`) | apply at the 67 helpers and the 3 duplicated helper sets                                                                                                                                           | 2-3 helper snapshots                                       | M    |
| 4   | Trees and lists             | 6 tree views, Search Everywhere                                                                                                               | `Tree` / `TreeItem` with level, expanded, selected, active descendant; `ListBox` / `ListBoxOption`                                                                                                 | selected-row snapshot                                      | M    |
| 5   | Dialogs, tabs, live regions | dialog roots, editor tabs, `notification_balloons.rs`, find bars                                                                              | `Dialog` + label; `TabList` / `Tab`; `Alert`, or `set_live(Polite)` / `set_disabled` through `a11y_synthetic_children`                                                                             | snapshot; dump from slice 6                                | M    |
| 6   | Window plumbing and dump    | window open                                                                                                                                   | `gpui_kit::base::install_window_hit_test_forwarder(window)` (hidden pub, macOS, not in tests: `gpui-base/src/lib.rs:133-135`); a debug-only env var that writes `debug_a11y_tree_json()` to a file | dump lists the nodes                                       | S    |

The slice 6 dump stays `None` until activation (`G/window.rs:4014-4023`, `debug.rs:101-102`). All named roles exist in `accesskit-0.24.1/src/lib.rs:76-195`.

Each slice must keep ids unique per frame and give each focus handle only one node with a role. Otherwise debug builds panic once a client is attached: "Duplicate a11y node id" (`G/window/a11y.rs:414-419`) or "set_focus called more than once" (`:530-541`). Tests never activate, so only a real-window run with the section 5 script can prove this.

For helpers that return `Stateful<Div>`, call `a11y_role()` and `write_a11y_info` on the result (`div.rs:1955-1965`). `.test_support()` changes the type (`observe.rs:5-9`), so use it only for `Render` views.

Hypothesis: the forwarder affects only hit tests, not traversal. To check, run the Swift dump with and without it.

## 5. Measuring in the real window

The terminal running these needs Accessibility permission, which the user grants in System Settings. Hypothesis: the first query activates AccessKit. To confirm, start the app with `RUST_LOG=info` and look for "Accessibility activated" (`G/window.rs:1759`).

```sh
PID=$(pgrep -n openidea-gpui)
osascript -e "tell application \"System Events\" to tell (first process whose unix id is $PID)
  set out to \"\"
  repeat with e in (entire contents of window 1)
    try
      set out to out & (role of e) & \" | \" & (description of e) & \" | \" & (name of e) & linefeed
    end try
  end repeat
  return out
end tell" | sort | uniq -c | sort -rn
```

Full tree with depth (`swift ax_dump.swift $PID`):

```swift
import ApplicationServices
let app = AXUIElementCreateApplication(pid_t(CommandLine.arguments[1])!)
func a(_ e: AXUIElement, _ k: String) -> AnyObject? { var v: AnyObject?; AXUIElementCopyAttributeValue(e, k as CFString, &v); return v }
func dump(_ e: AXUIElement, _ d: Int) {
  let s = [kAXRoleAttribute, kAXTitleAttribute, kAXDescriptionAttribute, kAXValueAttribute].map { a(e, $0).map { "\($0)" } ?? "" }
  print(String(repeating: "  ", count: d) + s.joined(separator: " | "))
  if d < 60 { for c in (a(e, kAXChildrenAttribute) as? [AXUIElement]) ?? [] { dump(c, d + 1) } }
}
for w in (a(app, kAXWindowsAttribute) as? [AXUIElement]) ?? [] { dump(w, 0) }
```

Hypothesis: `aria_label` appears as `AXDescription` on buttons; check with the Swift dump after slice 3. Baseline: the window root plus a few `InteractiveText` labels. Count `AXButton` rows after each slice.
