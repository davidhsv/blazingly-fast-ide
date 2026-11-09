import { afterEach, expect, it, vi } from "vitest";
const setActiveFile = vi.hoisted(() => vi.fn(async () => {}));
vi.mock("./backend", () => ({ pluginsBackend: () => ({ setActiveFile }) }));
const { trackActiveFile } = await import("./bridge");
const { useProject } = await import("@/platform/project");

afterEach(() => {
  useProject.setState({ project: null });
  vi.clearAllMocks();
});

it("reports the focused editor again once the window's project opens", () => {
  const stop = trackActiveFile();
  expect(setActiveFile).toHaveBeenCalledTimes(1);
  expect(setActiveFile).toHaveBeenLastCalledWith(null);
  useProject.setState({ project: { root: "/ws", name: "ws" } as never });
  expect(setActiveFile).toHaveBeenCalledTimes(2);
  expect(setActiveFile).toHaveBeenLastCalledWith(null);
  useProject.setState({ project: { root: "/ws", name: "ws", trusted: true } as never });
  expect(setActiveFile).toHaveBeenCalledTimes(2);
  stop();
});
