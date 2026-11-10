import { afterEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { registerAction } from "@/platform/actions";
import { useProject } from "@/platform/project";
import { ServerStatus } from "./ServerStatus";
import { useServers, type ServerEntry } from "./servers";

const ISSUE_49 =
  "jdtls needs JDK 25 or newer to run (found JDK 8, 17, 21). Download JDK 25, or pick an installed one in Settings › Languages & Frameworks › Java › Language Server › JDK (it restarts automatically); the project can still target an older Java version.";

const server = (status: ServerEntry["status"], message: string | null): ServerEntry =>
  ({ id: 1, name: "jdtls", status, message, languages: ["Java"], root: null }) as unknown as ServerEntry;

afterEach(() => useServers.setState({ servers: new Map() }));

describe("Language server status popup", () => {
  it("offers Download JDK N when jdtls can't start for want of a JDK", () => {
    const run = vi.fn();
    const off = registerAction({ id: "java.downloadJdk", title: "Download JDK…", run: ({ arg }) => run(arg) });
    useServers.setState({ servers: new Map([[1 as never, server("misconfigured", ISSUE_49)]]) });
    render(<ServerStatus />);
    fireEvent.click(screen.getByRole("button", { name: /Language servers/ }));
    fireEvent.click(screen.getByRole("button", { name: "Download JDK 25" }));
    expect(run).toHaveBeenCalledWith({ version: 25, languageServer: true });
    off();
  });

  it("says Light mode and offers Full mode while language servers are off", () => {
    const run = vi.fn();
    const off = registerAction({ id: "ide.toggleMode", title: "Toggle Light/Full Mode", run });
    useProject.setState({ project: { root: "/p" } as never, settings: { ide: { mode: "light" } } });
    useServers.setState({ servers: new Map([[1 as never, server("stopped", null)]]) });
    render(<ServerStatus />);
    fireEvent.click(screen.getByRole("button", { name: "Language servers: Light mode" }));
    expect(screen.getByText("Language servers are off in Light mode.")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Switch to Full Mode" }));
    expect(run).toHaveBeenCalled();
    useProject.setState({ project: null, settings: null });
    off();
  });

  it("keeps Restart for other failures", () => {
    useServers.setState({ servers: new Map([[1 as never, server("crashed", "exit code 1")]]) });
    render(<ServerStatus />);
    fireEvent.click(screen.getByRole("button", { name: /Language servers/ }));
    expect(screen.queryByRole("button", { name: /Download JDK/ })).toBeNull();
    expect(screen.getByRole("button", { name: "Restart jdtls" })).toBeInTheDocument();
  });
});
