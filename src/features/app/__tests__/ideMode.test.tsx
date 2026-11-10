import { afterEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { executeAction, registerActions } from "@/platform/actions";
import { useProject } from "@/platform/project";
import { ideMode } from "@/platform/editorSettings";
import { IdeModeToggle } from "../IdeModeToggle";
import { projectActions } from "../projectActions";

const { settingsSet } = vi.hoisted(() => ({ settingsSet: vi.fn() }));
vi.mock("@/platform/ipc", async (original) => ({
  ...(await original<typeof import("@/platform/ipc")>()),
  isTauri: () => true,
  commands: { settingsSet },
}));

const project = { root: "/p", name: "p", displayName: "p", trusted: true, isGit: false, settings: {} };
const ok = (settings: unknown) => ({ status: "ok", data: settings });

afterEach(() => {
  cleanup();
  settingsSet.mockReset();
  useProject.setState({ project: null, settings: null, lightEdit: false });
});

it("defaults to Blaze mode and switches the global value from the title bar", async () => {
  const dispose = registerActions(projectActions());
  useProject.setState({ project, settings: {} });
  expect(ideMode()).toBe("light");
  settingsSet.mockResolvedValueOnce(ok({ ide: { mode: "full" } }));
  render(<IdeModeToggle />);
  const button = screen.getByRole("button", { name: "IDE mode: Light" });
  expect(button).toHaveTextContent("Blaze mode");
  expect(button).toHaveAttribute("aria-pressed", "false");
  fireEvent.click(button);
  await vi.waitFor(() => expect(ideMode()).toBe("full"));
  expect(settingsSet).toHaveBeenCalledWith("global", "ide.mode", '"full"');
  expect(screen.getByRole("button", { name: "IDE mode: Full" })).toHaveAttribute("aria-pressed", "true");
  dispose();
});

it("switches a project override too when the global value alone would not take effect", async () => {
  const dispose = registerActions(projectActions());
  useProject.setState({ project, settings: { ide: { mode: "full" } } });
  settingsSet
    .mockResolvedValueOnce(ok({ ide: { mode: "full" } }))
    .mockResolvedValueOnce(ok({ ide: { mode: "light" } }));
  await executeAction("ide.toggleMode");
  expect(settingsSet.mock.calls).toEqual([
    ["global", "ide.mode", '"light"'],
    ["project", "ide.mode", '"light"'],
  ]);
  expect(ideMode()).toBe("light");
  dispose();
});

it("is hidden without a project and in LightEdit windows", () => {
  render(<IdeModeToggle />);
  expect(screen.queryByRole("button")).toBeNull();
  useProject.setState({ project, settings: {}, lightEdit: true });
  expect(screen.queryByRole("button")).toBeNull();
});
