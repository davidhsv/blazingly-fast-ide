#!/usr/bin/env node
// Writes `app-gpui/src/workbench/menu_spec.rs`: the menu bar's table and the pure functions
// around it, taken from `src-tauri/src/menu.rs` without the parts that draw it with Tauri.
// Both apps then show the same menus from one table, which is edited in `src-tauri` only.
// Run from the repository root: `node docs/gpui-port/menuspec.mjs`; with `--check` it writes
// nothing and fails when the written file is out of date.
import { readFileSync, writeFileSync } from "node:fs";

const SOURCE = "src-tauri/src/menu.rs";
const TARGET = "app-gpui/src/workbench/menu_spec.rs";
const source = readFileSync(SOURCE, "utf8");

/** The source from the line starting with `from` up to the line starting with `to`. */
function slice(from, to) {
  const start = source.indexOf(`\n${from}`);
  const end = source.indexOf(`\n${to}`, start + 1);
  if (start < 0 || end < 0) throw new Error(`${SOURCE}: no text from "${from}" to "${to}"`);
  return source.slice(start + 1, end + 1);
}

/** Replaces every match of `pattern`, which must occur. */
function rewrite(text, pattern, replacement) {
  if (!pattern.test(text)) throw new Error(`${SOURCE}: nothing matches ${pattern}`);
  return text.replace(pattern, replacement);
}

let body = [
  slice("/// Prefix of Open Recent item ids", "/// Localised labels and keymap accelerators"),
  slice("/// An item of a frontend-listed menu slot.", "impl MenuConfig {"),
  slice(
    "/// Whether the item `id` performs its own key equivalent",
    "/// Updates the enabled state of the installed menu",
  ),
  slice("pub struct RecentItem {", "/// One top-level menu, as it will actually be drawn."),
].join("");

// The table is plain data here: nothing crosses a process boundary.
body = rewrite(
  body,
  /#\[derive\(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type\)\]\n#\[serde\(rename_all = "camelCase"\)\]\n/g,
  "#[derive(Debug, Clone, PartialEq, Eq)]\n",
);
body = rewrite(body, /^ *#\[serde\(default\)\]\n/gm, "");
body = rewrite(body, /#\[cfg\(any\(target_os = "macos", test\)\)\]\n/g, "");
body = rewrite(body, /^ {4}\/\/\/ Accelerator the OS attaches implicitly[\s\S]*?^ {4}\}\n/m, "");
body = rewrite(body, /\n\n\}\n/, "\n}\n");
body = rewrite(body, /\[`(MenuConfig::\w+)`\]/g, "`$1`");
body = rewrite(body, /^(const|fn) /gm, "pub $1 ");
body = rewrite(body, /^pub struct RecentItem/m, "#[derive(Debug, Clone, PartialEq, Eq)]\npub struct RecentItem");
body = rewrite(body, /^ {4}fn (key|default_label)\(self\)/gm, "    pub fn $1(self)");

const text = `//! The menu bar's table: which menus there are on each OS, what they hold and which action
//! each item runs, with the functions that route a chosen item and say whether it is enabled.
//!
//! Written by \`docs/gpui-port/menuspec.mjs\` from \`${SOURCE}\`. Do not edit: change the
//! table there and run the script.

use std::collections::HashSet;
use std::path::Path;

${body}`;

if (process.argv.includes("--check")) {
  if (readFileSync(TARGET, "utf8") !== text) {
    console.error(`${TARGET} is out of date: run node docs/gpui-port/menuspec.mjs`);
    process.exit(1);
  }
  console.log(`${TARGET} matches ${SOURCE}`);
} else {
  writeFileSync(TARGET, text);
  console.log(`wrote ${TARGET}`);
}
