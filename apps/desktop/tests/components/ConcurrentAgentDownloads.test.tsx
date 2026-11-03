import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { OceanwayAppsPage } from "@/components/oceanway/OceanwayAppsPage";
import type { InstallStatus } from "@/components/oceanway/AgentInstallProgress";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@/lib/api/oceanway", () => ({
  oceanwayApi: { detectApps: async () => [] },
}));
vi.mock("@/components/shell/AppGlyph", async (original) => ({
  ...(await original<object>()),
  AppGlyph: () => <span />,
}));
afterEach(() => vi.useRealTimers());

it("keeps starts, progress, cancellation and opening independent across applications and navigation", async () => {
  vi.useFakeTimers();
  const jobs: Record<string, InstallStatus> = {};
  const starts: Record<string, (job: InstallStatus) => void> = {};
  invoke.mockImplementation(
    async (command: string, args: Record<string, string>) => {
      if (command === "oceanway_start_install") {
        jobs[args.appId] = {
          id: args.appId,
          appId: args.appId,
          stage: "downloading",
          downloaded: 10,
          total: 100,
          message: "正在下载",
        };
        return new Promise<InstallStatus>((resolve) => {
          starts[args.appId] = resolve;
        });
      }
      if (command === "oceanway_get_install") return jobs[args.id];
      if (command === "oceanway_cancel_install") {
        jobs[args.id] = {
          ...jobs[args.id],
          stage: "cancelled",
          message: "下载已取消",
        };
        return;
      }
      if (command === "oceanway_open_installer") {
        jobs[args.id] = {
          ...jobs[args.id],
          stage: "awaiting_install",
          message: "安装包已打开",
        };
        return jobs[args.id];
      }
      if (command === "oceanway_check_install") return jobs[args.id];
      throw new Error(command);
    },
  );
  const area = (name: string) => within(screen.getByRole("region", { name }));
  const click = async (button: HTMLElement) => {
    await act(async () => {
      fireEvent.click(button);
    });
  };
  const tick = async () => {
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });
  };
  let page = render(<OceanwayAppsPage />);
  await click(area("Codex").getByRole("button", { name: "下载安装" }));
  expect(
    area("Claude Desktop").getByRole("button", { name: "下载安装" }),
  ).toBeEnabled();
  await click(area("Claude Desktop").getByRole("button", { name: "下载安装" }));
  await click(area("Codex").getByRole("button", { name: "正在处理…" }));
  expect(
    invoke.mock.calls.filter(
      ([command]) => command === "oceanway_start_install",
    ),
  ).toHaveLength(2);
  await act(async () => {
    starts.codex(jobs.codex);
    starts["claude-desktop"](jobs["claude-desktop"]);
  });
  jobs.codex = { ...jobs.codex, downloaded: 25 };
  jobs["claude-desktop"] = { ...jobs["claude-desktop"], downloaded: 60 };
  await tick();
  expect(area("Codex").getByRole("progressbar")).toHaveAttribute(
    "aria-valuenow",
    "25",
  );
  expect(area("Claude Desktop").getByRole("progressbar")).toHaveAttribute(
    "aria-valuenow",
    "60",
  );
  page.unmount();
  await act(async () => {
    page = render(<OceanwayAppsPage />);
  });
  expect(area("Claude Desktop").getByRole("progressbar")).toHaveAttribute(
    "aria-valuenow",
    "60",
  );
  await click(area("Codex").getByRole("button", { name: "取消下载" }));
  await tick();
  expect(area("Codex").getByRole("status")).toHaveTextContent("下载已取消");
  expect(area("Claude Desktop").getByRole("progressbar")).toBeVisible();
  await click(
    area("Kimi Code Desktop").getByRole("button", { name: "下载安装" }),
  );
  await act(async () => {
    starts.kimi(jobs.kimi);
  });
  jobs["claude-desktop"] = {
    ...jobs["claude-desktop"],
    downloaded: 100,
    stage: "ready",
    message: "下载完成",
  };
  await tick();
  await click(
    area("Claude Desktop").getByRole("button", { name: "打开安装包" }),
  );
  expect(invoke).toHaveBeenCalledWith("oceanway_open_installer", {
    id: "claude-desktop",
  });
  expect(area("Claude Desktop").getByRole("status")).toHaveTextContent(
    "安装包已打开",
  );
  expect(area("Kimi Code Desktop").getByRole("progressbar")).toBeVisible();
  await click(
    area("Kimi Code Desktop").getByRole("button", { name: "取消下载" }),
  );
  await tick();
  page.unmount();
});
