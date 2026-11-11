import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { act } from "@testing-library/react";
import type { BuildModel, MavenProject, ProjectInfo } from "@/platform/ipc";
import { useProject } from "@/platform/project";
import { activate } from "./index";
import { loadModel, resetBuild, useBuild } from "./store";

const mocks = vi.hoisted(() => ({ model: vi.fn(), sync: vi.fn(), cancel: vi.fn() }));
vi.mock("@/platform/ipc", async (original) => {
  const real = await original<typeof import("@/platform/ipc")>();
  return {
    ...real,
    isTauri: () => true,
    call: (value: unknown) => value,
    commands: {
      ...real.commands,
      buildModel: mocks.model,
      runConfigs: () => Promise.resolve([]),
      buildSync: mocks.sync,
      cancelRequest: mocks.cancel,
    },
  };
});
vi.mock("@tauri-apps/api/core", async (original) => ({
  ...(await original<typeof import("@tauri-apps/api/core")>()),
  Channel: class {
    onmessage: unknown = null;
  },
}));

const project = { pom: "pom.xml", dir: "", projectRoot: null, aggregated: false } as unknown as MavenProject;
const neverSynced = (projects: MavenProject[]): BuildModel => ({
  maven: { syncedAt: null, stale: false, syncError: null, profiles: [], projects },
  gradle: [],
});
const mode = (value: "light" | "full") => useProject.setState({ settings: { ide: { mode: value } } });

let dispose: () => void;
beforeEach(() => {
  vi.clearAllMocks();
  resetBuild();
  mocks.sync.mockImplementation(() => new Promise(() => {}));
  mocks.cancel.mockResolvedValue(undefined);
  mocks.model.mockResolvedValue(neverSynced([project]));
  useProject.setState({ project: { root: "/p", trusted: true } as ProjectInfo, settings: {} });
});
afterEach(() => {
  dispose();
  resetBuild();
  useProject.setState({ project: null, settings: {} });
});

const open = async () => {
  await act(async () => {
    dispose = activate();
    await loadModel();
  });
};

it("imports a never-synced Maven build in Deep mode only", async () => {
  await open();
  expect(useBuild.getState().model?.maven?.projects).toHaveLength(1);
  expect(mocks.sync).not.toHaveBeenCalled();

  act(() => mode("full"));
  expect(mocks.sync).toHaveBeenCalledTimes(1);
  expect(mocks.sync.mock.calls[0]![0]).toBe("maven");
});

it("cancels the first import once every Maven project is unlinked", async () => {
  mode("full");
  await open();
  expect(mocks.sync).toHaveBeenCalledTimes(1);
  expect(useBuild.getState().syncing.maven).toBeDefined();

  mocks.model.mockResolvedValue(neverSynced([]));
  await act(async () => {
    await loadModel();
  });
  expect(mocks.cancel).toHaveBeenCalledWith(useBuild.getState().syncing.maven?.request);
});
