import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { OceanwayAppsPage } from "@/components/oceanway/OceanwayAppsPage";
import type { InstallStatus } from "@/components/oceanway/AgentInstallProgress";
import { oceanwayApi } from "@/lib/api/oceanway";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@/lib/api/oceanway", () => ({ oceanwayApi: { detectApps: vi.fn() } }));
vi.mock("@/components/shell/AppGlyph", async (original) => ({
  ...(await original<object>()),
  AppGlyph: () => <span />,
}));

let sequence = 0;
let status: InstallStatus;
let pollError: Error | undefined;
let openError: Error | undefined;
let detectedInstallation = false;

const detected = (installed: boolean) => [
  {
    appId: "codex" as const,
    name: "Codex",
    installed,
    desktopInstalled: installed,
    configExists: false,
    configurable: true,
  },
];
const button = () =>
  within(screen.getByRole("region", { name: "Codex" })).getByRole("button", {
    name: "下载安装",
  });
async function tick(seconds = 1) {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(seconds * 1000);
  });
}
async function openPage() {
  await act(async () => {
    render(<OceanwayAppsPage />);
  });
}
async function start() {
  await act(async () => {
    fireEvent.click(button());
  });
}

beforeEach(() => {
  vi.useFakeTimers();
  pollError = undefined;
  openError = undefined;
  detectedInstallation = false;
  vi.mocked(oceanwayApi.detectApps).mockResolvedValue(detected(false));
  invoke.mockImplementation(
    async (command: string, args: Record<string, string>) => {
      if (command === "oceanway_start_install") {
        status = {
          id: `install-${++sequence}`,
          appId: args.appId,
          stage: "preparing",
          downloaded: 0,
          total: 0,
          message: "正在检查系统和安装包…",
        };
        return status;
      }
      if (command === "oceanway_get_install") {
        if (pollError) throw pollError;
        return status;
      }
      if (command === "oceanway_open_installer") {
        if (openError) throw openError;
        status = {
          ...status,
          stage: "awaiting_install",
          message: "安装包已打开，请按系统提示完成安装，再点击“检测安装结果”。",
        };
        return status;
      }
      if (command === "oceanway_check_install") {
        status = {
          ...status,
          stage: detectedInstallation ? "installed" : "awaiting_install",
          message: detectedInstallation
            ? "已检测到应用，安装完成"
            : "尚未检测到应用，请先完成安装",
        };
        return status;
      }
      if (command === "oceanway_cancel_install") {
        status = {
          ...status,
          stage: "cancelled",
          message: "下载已取消，可以重新安装",
        };
        return;
      }
      throw new Error(`Unexpected command: ${command}`);
    },
  );
});
afterEach(() => {
  vi.useRealTimers();
});

describe("installation button with the real status controller", () => {
  it("shows download progress, prevents duplicate installs, and detects the installed app", async () => {
    await openPage();
    await start();
    expect(invoke).toHaveBeenCalledWith("oceanway_start_install", {
      appId: "codex",
    });
    expect(button()).toBeDisabled();
    expect(screen.getByRole("status")).toHaveTextContent("正在检查");

    status = {
      ...status,
      stage: "downloading",
      downloaded: 5 * 1048576,
      total: 10 * 1048576,
      message: "正在下载完整安装包…",
    };
    await tick();
    expect(screen.getByRole("progressbar")).toHaveAttribute(
      "aria-valuenow",
      "50",
    );
    expect(screen.getByText("50% · 5.0 / 10.0 MB")).toBeVisible();

    status = { ...status, stage: "ready", message: "下载完成，点击打开安装包" };
    await tick();
    expect(screen.getByRole("button", { name: "打开安装包" })).toBeEnabled();
    expect(screen.queryByRole("button", { name: "取消下载" })).toBeNull();
    expect(
      invoke.mock.calls.some(
        ([command]) => command === "oceanway_open_installer",
      ),
    ).toBe(false);
    const polls = invoke.mock.calls.filter(
      ([command]) => command === "oceanway_get_install",
    ).length;
    await tick(5);
    expect(
      invoke.mock.calls.filter(
        ([command]) => command === "oceanway_get_install",
      ),
    ).toHaveLength(polls);

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "打开安装包" }));
    });
    expect(invoke).toHaveBeenCalledWith("oceanway_open_installer", {
      id: status.id,
    });
    expect(screen.getByRole("status")).toHaveTextContent("安装包已打开");
    expect(
      within(screen.getByRole("region", { name: "Codex" })).queryByRole(
        "button",
        { name: "已安装" },
      ),
    ).toBeNull();
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "检测安装结果" }));
    });
    expect(screen.getByRole("status")).toHaveTextContent("尚未检测到");

    detectedInstallation = true;
    vi.mocked(oceanwayApi.detectApps).mockResolvedValue(detected(true));
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "检测安装结果" }));
    });
    expect(screen.getByRole("status")).toHaveTextContent("安装完成");
    expect(
      within(screen.getByRole("region", { name: "Codex" })).getByRole(
        "button",
        { name: "已安装" },
      ),
    ).toBeDisabled();
  });

  it("keeps the downloaded package available if opening is cancelled or fails", async () => {
    await openPage();
    await start();
    status = { ...status, stage: "ready", message: "下载完成" };
    await tick();
    openError = new Error("用户取消了系统权限提示");
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "打开安装包" }));
    });
    expect(screen.getByRole("alert")).toHaveTextContent("取消了系统权限提示");
    expect(screen.getByRole("button", { name: "打开安装包" })).toBeEnabled();
    openError = undefined;
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "打开安装包" }));
    });
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.getByRole("status")).toHaveTextContent("安装包已打开");
  });

  it("stops waiting on failure, permits retry, and reports repeated status errors", async () => {
    await openPage();
    await start();
    status = {
      ...status,
      stage: "failed",
      message: "安装包下载响应超时，请重试",
    };
    await tick();
    expect(screen.getByRole("alert")).toHaveTextContent("下载响应超时");
    expect(button()).toBeEnabled();

    await start();
    expect(button()).toBeDisabled();
    pollError = new Error("状态服务中断");
    await tick(3);
    expect(screen.getByRole("alert")).toHaveTextContent("无法读取安装状态");
    expect(button()).toBeEnabled();
  });

  it("cancels the download through the native command and permits another install", async () => {
    await openPage();
    await start();
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "取消下载" }));
    });
    expect(invoke).toHaveBeenCalledWith("oceanway_cancel_install", {
      id: status.id,
    });
    await tick();
    expect(screen.getByRole("status")).toHaveTextContent("下载已取消");
    expect(button()).toBeEnabled();
  });
});
