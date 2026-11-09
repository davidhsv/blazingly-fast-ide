import { afterEach, expect, it, vi } from "vitest";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { syntaxHighlighting, defaultHighlightStyle } from "@codemirror/language";
const ipc = vi.hoisted(() => ({
  pluginLanguageVisuals: vi.fn(),
  lspExecuteCommand: vi.fn(async () => ({ status: "ok", data: null })),
}));
vi.mock("@/platform/ipc", async (original) => ({
  ...(await original<typeof import("@/platform/ipc")>()),
  commands: ipc,
}));
const { injectedHighlights, pluginLanguageVisuals } = await import("./languageVisuals");
const ctx = { doc: 7, path: "/p/a.txt", language: "text", uiMode: null, readOnly: false };
const views: EditorView[] = [];
afterEach(() => {
  views.splice(0).forEach((v) => v.destroy());
  vi.useRealTimers();
  vi.clearAllMocks();
});
it("highlights only the embedded slice with its own parser and the editor theme", async () => {
  const state = EditorState.create({
    doc: "host SELECT a FROM t host",
    extensions: [syntaxHighlighting(defaultHighlightStyle)],
  });
  const deco = await injectedHighlights(state, [
    { range: { start: { line: 0, col: 5 }, end: { line: 0, col: 20 } }, language: "sql" },
  ]);
  expect(deco.size).toBeGreaterThan(0);
  const cursor = deco.iter();
  while (cursor.value) {
    expect(cursor.from).toBeGreaterThanOrEqual(5);
    expect(cursor.to).toBeLessThanOrEqual(20);
    expect(cursor.value.spec.class).toMatch(/\S/);
    cursor.next();
  }
});
it("renders owned gutter commands and drops answers to superseded buffers", async () => {
  vi.useFakeTimers();
  let answer!: (v: unknown) => void;
  ipc.pluginLanguageVisuals.mockImplementationOnce(
    () =>
      new Promise((r) => {
        answer = r;
      }),
  );
  const visual = {
    version: 2,
    injections: [],
    colors: [],
    gutterActions: [["test", { line: 0, icon: "run", tooltip: "Run test", command: "run", argument: "value" }]],
  };
  ipc.pluginLanguageVisuals.mockResolvedValue({ status: "ok", data: visual });
  const view = new EditorView({
    state: EditorState.create({ doc: "before", extensions: pluginLanguageVisuals(ctx) }),
    parent: document.body,
  });
  views.push(view);
  await vi.advanceTimersByTimeAsync(300);
  view.dispatch({ changes: { from: 0, to: 6, insert: "after" } });
  answer({ status: "ok", data: visual });
  await vi.advanceTimersByTimeAsync(0);
  expect(view.dom.querySelector(".cm-plugin-gutter-action")).toBeNull();
  await vi.advanceTimersByTimeAsync(300);
  const button = view.dom.querySelector<HTMLButtonElement>(".cm-plugin-gutter-action")!;
  expect(button.getAttribute("aria-label")).toBe("Run test");
  button.click();
  expect(ipc.lspExecuteCommand).toHaveBeenCalledWith(
    7,
    expect.objectContaining({ command: "openidea.plugin-language:test:run", arguments: ['"value"'] }),
  );
});
it("puts the actions of one line behind one icon whose click opens a menu", async () => {
  vi.useFakeTimers();
  const { useContextMenuStore } = await import("@/ui/Menu");
  const action = (icon: string, tooltip: string, command: string) => ({
    line: 0,
    icon,
    tooltip,
    command,
    argument: "x",
  });
  ipc.pluginLanguageVisuals.mockResolvedValue({
    status: "ok",
    data: {
      version: 1,
      injections: [],
      colors: [],
      gutterActions: [
        ["test", action("run", "Run 'T'", "run")],
        ["test", action("debug", "Debug 'T'", "debug")],
      ],
    },
  });
  const view = new EditorView({
    state: EditorState.create({ doc: "class T {}", extensions: pluginLanguageVisuals(ctx) }),
    parent: document.body,
  });
  views.push(view);
  await vi.advanceTimersByTimeAsync(300);
  const buttons = view.dom.querySelectorAll<HTMLButtonElement>(".cm-plugin-gutter-action");
  expect(buttons).toHaveLength(1);
  buttons[0]!.click();
  expect(ipc.lspExecuteCommand).not.toHaveBeenCalled();
  const items = useContextMenuStore.getState().menu!.items as { label: string; icon?: string; run: () => void }[];
  expect(items.map((i) => [i.label, i.icon])).toEqual([
    ["Run 'T'", "run"],
    ["Debug 'T'", "debug"],
  ]);
  items[1]!.run();
  expect(ipc.lspExecuteCommand).toHaveBeenCalledWith(
    7,
    expect.objectContaining({ command: "openidea.plugin-language:test:debug" }),
  );
});
