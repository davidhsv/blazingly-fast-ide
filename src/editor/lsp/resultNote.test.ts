import { afterEach, expect, it } from "vitest";
import { useProject } from "@/platform/project";
import { incompleteNote, resultNote } from "./lsp";

afterEach(() => useProject.setState({ settings: null }));

it("marks text matches and points at Deep mode while the IDE is in Blaze mode", () => {
  useProject.setState({ settings: { ide: { mode: "light" } } });
  expect(resultNote({ incomplete: null, approximate: true }, "x")).toBe(
    "Text matches: switch to Deep mode for exact results",
  );
  useProject.setState({ settings: { ide: { mode: "full" } } });
  expect(resultNote({ incomplete: null, approximate: true }, "x")).toBe("Text matches: no language server answered");
});

it("keeps the incompleteness note next to the text-match note", () => {
  expect(resultNote({ incomplete: "tooManyFiles", approximate: true }, "x")).toBe(
    `Text matches: switch to Deep mode for exact results · ${incompleteNote("tooManyFiles", "x")}`,
  );
  expect(resultNote({ incomplete: "indexing" }, "x")).toBe(incompleteNote("indexing", "x"));
  expect(resultNote({ incomplete: null, approximate: false }, "x")).toBeNull();
});
