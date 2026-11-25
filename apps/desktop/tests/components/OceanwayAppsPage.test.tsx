import { render, screen, within, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { OceanwayAppsPage } from "@/components/oceanway/OceanwayAppsPage";
import { oceanwayApi } from "@/lib/api/oceanway";
const start = vi.hoisted(() => vi.fn());
vi.mock("@/lib/api/oceanway", () => ({ oceanwayApi: { detectApps: vi.fn() } }));
vi.mock("@/components/oceanway/AgentInstallProgress", () => ({
  useAgentInstaller: () => ({
    start,
    jobs: {},
    isBusy: () => false,
    installed: "",
    check: vi.fn(),
  }),
  AgentInstallProgress: () => null,
}));
vi.mock("@/components/shell/AppGlyph", async (original) => ({
  ...(await original<object>()),
  AppGlyph: () => <span />,
}));
beforeEach(() => {
  vi.clearAllMocks();
  start.mockResolvedValue(undefined);
  vi.mocked(oceanwayApi.detectApps).mockResolvedValue([
    {
      appId: "codex",
      name: "Codex",
      installed: true,
      desktopInstalled: false,
      configExists: true,
      configurable: true,
    },
    {
      appId: "claude-desktop",
      name: "Claude Desktop",
      installed: true,
      desktopInstalled: true,
      configExists: true,
      configurable: true,
    },
  ]);
});
describe("Oceanway in-app installation", () => {
  it("shows exactly the six selected products and does not mistake a CLI for a desktop", async () => {
    const user = userEvent.setup();
    render(<OceanwayAppsPage />);
    await waitFor(() =>
      expect(oceanwayApi.detectApps).toHaveBeenCalledTimes(1),
    );
    for (const name of [
      "Codex",
      "Claude Desktop",
      "Claude Code",
      "DeepSeek Harness",
      "Hermes",
      "Kimi Code Desktop",
    ])
      expect(screen.getByRole("region", { name })).toBeVisible();
    expect(screen.queryByText("Gemini CLI")).toBeNull();
    expect(screen.queryByText("OpenClaw")).toBeNull();
    const codex = within(screen.getByRole("region", { name: "Codex" }));
    await waitFor(() => expect(codex.getByText(/尚未检测到/)).toBeVisible());
    await user.click(codex.getByRole("button", { name: "下载安装" }));
    expect(start).toHaveBeenCalledWith("codex");
    expect(
      within(screen.getByRole("region", { name: "Claude Desktop" })).getByRole(
        "button",
        { name: "已安装" },
      ),
    ).toBeDisabled();
  });
  it("reports installation startup errors and permits retry", async () => {
    start.mockRejectedValueOnce(new Error("下载连接失败，请重试"));
    const user = userEvent.setup();
    render(<OceanwayAppsPage />);
    const button = within(
      screen.getByRole("region", { name: "Codex" }),
    ).getByRole("button", { name: "下载安装" });
    await user.click(button);
    expect(await screen.findByRole("alert")).toHaveTextContent("下载连接失败");
    expect(button).toBeEnabled();
  });
  it("does not offer an unusable package on an unsupported architecture", async () => {
    vi.mocked(oceanwayApi.detectApps).mockResolvedValue([
      {
        appId: "hermes",
        name: "Hermes",
        installed: false,
        desktopInstalled: false,
        configExists: false,
        configurable: true,
        installAvailable: false,
        reason: "Hermes 暂未提供 Intel Mac 完整离线包",
      },
    ]);
    render(<OceanwayAppsPage />);
    expect(
      await screen.findByRole("button", { name: "当前系统暂无安装包" }),
    ).toBeDisabled();
    expect(
      screen.getByText("Hermes 暂未提供 Intel Mac 完整离线包"),
    ).toBeVisible();
  });
});
