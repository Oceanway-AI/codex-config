import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { vi, beforeEach, describe, it, expect } from "vitest";
import { OceanwayConfigPanel } from "@/components/oceanway/OceanwayConfigPanel";
import { oceanwayApi } from "@/lib/api/oceanway";
import {
  conversationModels,
  imageModels,
  recommendedModel,
} from "@/components/oceanway/models";

vi.mock("@/lib/api/oceanway", () => ({
  oceanwayApi: {
    connection: vi.fn(),
    detectApps: vi.fn(),
    checkKey: vi.fn(),
    apply: vi.fn(),
    launch: vi.fn(),
  },
}));
vi.mock("@/components/shell/AppGlyph", () => ({ AppGlyph: () => <span /> }));

const models = [
  "claude-sonnet-4-5",
  "gpt-5",
  "gpt-5-mini",
  "gemini-2.5-pro",
  "gpt-image-1",
  "text-embedding-3-large",
].map((id) => ({ id }));
function mount() {
  return render(
    <QueryClientProvider
      client={
        new QueryClient({ defaultOptions: { queries: { retry: false } } })
      }
    >
      <OceanwayConfigPanel />
    </QueryClientProvider>,
  );
}
beforeEach(() => {
  vi.mocked(oceanwayApi.connection).mockResolvedValue({
    configured: false,
    keyPreview: null,
    baseUrl: "https://console.oceanway.tech/v1",
    configuredApps: [],
    defaultImageModel: null,
  });
  vi.mocked(oceanwayApi.detectApps).mockResolvedValue(
    ["codex", "claude"].map((appId) => ({
      appId,
      name: appId === "codex" ? "Codex" : "Claude Code",
      installed: true,
      configExists: true,
      configurable: true,
    })),
  );
  vi.mocked(oceanwayApi.checkKey).mockResolvedValue({
    models,
    baseUrl: "https://console.oceanway.tech/v1",
    checkedAt: "2026-11-02",
  });
  vi.mocked(oceanwayApi.apply).mockResolvedValue({
    results: [
      { appId: "codex", ok: true, message: "配置已保存" },
      { appId: "claude", ok: false, message: "配置文件只读" },
    ],
    restartRequired: true,
    backupPath: "fixture/backups",
    imagesConfigured: true,
  });
});
describe("Oceanway configuration", () => {
  it("selects families and shares image models without treating embeddings as chat", () => {
    const text = conversationModels(models);
    expect(recommendedModel("codex", text)).toBe("gpt-5");
    expect(recommendedModel("claude", text)).toBe("claude-sonnet-4-5");
    expect(recommendedModel("gemini", text)).toBe("gemini-2.5-pro");
    expect(recommendedModel("claude", ["gpt-5"])).toBe("");
    expect(imageModels(models)).toEqual(["gpt-image-1"]);
    expect(text).not.toContain("text-embedding-3-large");
  });
  it("saves independently, reports partial failure and never restarts automatically", async () => {
    const user = userEvent.setup();
    mount();
    await waitFor(() =>
      expect(screen.getByLabelText("Oceanway API Key", { selector: "input" })).toBeEnabled(),
    );
    await user.type(screen.getByLabelText("Oceanway API Key", { selector: "input" }), "fixture-key");
    await user.click(screen.getByText("连接并获取模型"));
    await waitFor(() =>
      expect(screen.getByLabelText("Codex 对话模型")).toHaveValue("gpt-5"),
    );
    expect(screen.getByLabelText("Claude Code 对话模型")).toHaveValue(
      "claude-sonnet-4-5",
    );
    expect(screen.getByLabelText("图片模型")).toHaveValue("gpt-image-1");
    await user.click(screen.getByRole("button", { name: "一键配置 2 个应用" }));
    await screen.findByText("配置文件只读");
    expect(oceanwayApi.apply).toHaveBeenCalledWith(
      expect.objectContaining({
        apiKey: "fixture-key",
        imageModel: "gpt-image-1",
        apps: [
          { appId: "codex", model: "gpt-5" },
          { appId: "claude", model: "claude-sonnet-4-5" },
        ],
      }),
    );
    expect(oceanwayApi.launch).not.toHaveBeenCalled();
    expect(screen.getAllByText("重启应用并加载新配置")).toHaveLength(1);
    expect(screen.getByLabelText("Oceanway API Key", { selector: "input" })).toHaveValue("");
    expect(Object.values(localStorage)).not.toContain("fixture-key");
  });
  it("invalidates checked models when the key changes and disables save after rejection", async () => {
    const user = userEvent.setup();
    mount();
    await waitFor(() =>
      expect(screen.getByLabelText("Oceanway API Key", { selector: "input" })).toBeEnabled(),
    );
    await user.type(screen.getByLabelText("Oceanway API Key", { selector: "input" }), "fixture-key");
    await user.click(screen.getByText("连接并获取模型"));
    await waitFor(() =>
      expect(screen.getByLabelText("Codex 对话模型")).toHaveValue("gpt-5"),
    );
    await user.type(screen.getByLabelText("Oceanway API Key", { selector: "input" }), "-changed");
    expect(
      screen.getByRole("button", { name: "一键配置 2 个应用" }),
    ).toBeDisabled();
    vi.mocked(oceanwayApi.checkKey).mockRejectedValueOnce(
      new Error("Key 无效"),
    );
    await user.click(screen.getByText("连接并获取模型"));
    expect(await screen.findByRole("alert")).toHaveTextContent("Key 无效");
    expect(oceanwayApi.apply).not.toHaveBeenCalled();
  });
});
