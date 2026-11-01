import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { vi, beforeEach, describe, it, expect } from "vitest";
import { OceanwayConfigPanel } from "@/components/oceanway/OceanwayConfigPanel";
import { oceanwayApi } from "@/lib/api/oceanway";
import liveCatalog from "@/test-fixtures/oceanway-model-catalog.json";
import liveImageCatalog from "@/test-fixtures/oceanway-image-catalog.json";
import {
  conversationModels,
  imageModels,
  recommendedModel,
  recommendedImageModel,
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
  it("keeps all eleven live image test models separate from conversation families", () => {
    const combined = [...liveCatalog.models, ...liveImageCatalog.models];
    expect(imageModels(combined).sort()).toEqual(
      liveImageCatalog.models.map((model) => model.id).sort(),
    );
    expect(imageModels(combined)).toHaveLength(11);
    expect(recommendedImageModel(imageModels(combined))).toBe("gpt-image-2");
    for (const { id } of liveImageCatalog.models) {
      expect(conversationModels(combined)).not.toContain(id);
    }
  });
  it("selects families and shares image models without treating embeddings as chat", () => {
    const text = conversationModels(models);
    expect(recommendedModel("codex", text)).toBe("gpt-5");
    expect(recommendedModel("claude", text)).toBe("claude-sonnet-4-5");
    expect(recommendedModel("gemini", text)).toBe("gemini-2.5-pro");
    expect(recommendedModel("claude", ["gpt-5"])).toBe("");
    expect(imageModels(models)).toEqual(["gpt-image-1"]);
    expect(text).not.toContain("text-embedding-3-large");
  });
  it("renders every family model returned by the live Oceanway catalog", async () => {
    vi.mocked(oceanwayApi.detectApps).mockResolvedValue(
      [
        ["codex", "Codex"],
        ["claude", "Claude Code"],
        ["gemini", "Gemini CLI"],
      ].map(([appId, name]) => ({
        appId,
        name,
        installed: true,
        configExists: true,
        configurable: true,
      })),
    );
    vi.mocked(oceanwayApi.checkKey).mockResolvedValueOnce(liveCatalog);
    const user = userEvent.setup();
    mount();
    const keyInput = screen.getByLabelText("Oceanway API Key", {
      selector: "input",
    });
    await waitFor(() => expect(keyInput).toBeEnabled());
    await user.type(keyInput, "fixture-key");
    await user.click(screen.getByText("连接并获取模型"));
    await screen.findByText("已连接 · 36 个可用模型");
    for (const [name, prefix] of [
      ["Codex", "gpt-"],
      ["Claude Code", "claude-"],
      ["Gemini CLI", "gemini-"],
    ]) {
      const options = within(
        screen.getByLabelText(`${name} 对话模型`),
      ).getAllByRole("option");
      const actual = options
        .map((option) => (option as HTMLOptionElement).value)
        .filter(Boolean)
        .sort();
      const expected = liveCatalog.models
        .map((m) => m.id)
        .filter((id) => id.startsWith(prefix))
        .sort();
      expect(actual).toEqual(expected);
      expect(actual.length).toBeGreaterThan(1);
    }
    expect(screen.getByLabelText("Codex 对话模型")).toHaveValue("gpt-6.1-sol");
    expect(screen.getByLabelText("Claude Code 对话模型")).toHaveValue(
      "claude-sonnet-5-5",
    );
    expect(screen.getByLabelText("Gemini CLI 对话模型")).toHaveValue(
      "gemini-3.8-flash",
    );
    expect(screen.getByLabelText("图片模型")).toHaveValue("");
  });
  it("saves independently, reports partial failure and never restarts automatically", async () => {
    const user = userEvent.setup();
    mount();
    await waitFor(() =>
      expect(
        screen.getByLabelText("Oceanway API Key", { selector: "input" }),
      ).toBeEnabled(),
    );
    await user.type(
      screen.getByLabelText("Oceanway API Key", { selector: "input" }),
      "fixture-key",
    );
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
    expect(
      screen.getByLabelText("Oceanway API Key", { selector: "input" }),
    ).toHaveValue("");
    expect(Object.values(localStorage)).not.toContain("fixture-key");
  });
  it("invalidates checked models when the key changes and disables save after rejection", async () => {
    const user = userEvent.setup();
    mount();
    await waitFor(() =>
      expect(
        screen.getByLabelText("Oceanway API Key", { selector: "input" }),
      ).toBeEnabled(),
    );
    await user.type(
      screen.getByLabelText("Oceanway API Key", { selector: "input" }),
      "fixture-key",
    );
    await user.click(screen.getByText("连接并获取模型"));
    await waitFor(() =>
      expect(screen.getByLabelText("Codex 对话模型")).toHaveValue("gpt-5"),
    );
    await user.type(
      screen.getByLabelText("Oceanway API Key", { selector: "input" }),
      "-changed",
    );
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
  it("shows each agent's own family by default and lets the user choose another model", async () => {
    const user = userEvent.setup();
    mount();
    const keyInput = screen.getByLabelText("Oceanway API Key", {
      selector: "input",
    });
    await waitFor(() => expect(keyInput).toBeEnabled());
    await user.type(keyInput, "fixture-key");
    await user.click(screen.getByText("连接并获取模型"));
    const codex = screen.getByLabelText("Codex 对话模型");
    const claude = screen.getByLabelText("Claude Code 对话模型");
    await waitFor(() => expect(codex).toHaveValue("gpt-5"));
    expect(
      within(codex).queryByRole("option", { name: "claude-sonnet-4-5" }),
    ).toBeNull();
    expect(within(claude).queryByRole("option", { name: "gpt-5" })).toBeNull();
    expect(
      within(codex).queryByRole("option", { name: "gpt-image-1" }),
    ).toBeNull();
    await user.click(
      screen.getAllByRole("button", { name: "选择其他模型" })[0],
    );
    await user.selectOptions(codex, "claude-sonnet-4-5");
    await user.click(screen.getByRole("button", { name: "一键配置 2 个应用" }));
    await waitFor(() =>
      expect(oceanwayApi.apply).toHaveBeenCalledWith(
        expect.objectContaining({
          apps: [
            { appId: "codex", model: "claude-sonnet-4-5" },
            { appId: "claude", model: "claude-sonnet-4-5" },
          ],
        }),
      ),
    );
    expect(within(claude).queryByRole("option", { name: "gpt-5" })).toBeNull();
  });
  it("keeps text configuration success visible when image setup needs attention", async () => {
    vi.mocked(oceanwayApi.apply).mockResolvedValueOnce({
      results: [
        {
          appId: "codex",
          ok: true,
          message: "文本配置已保存",
          imageConfigured: false,
          imageMessage: "图片程序启动失败",
        },
      ],
      restartRequired: true,
      backupPath: "fixture/backups",
      imagesConfigured: false,
    });
    const user = userEvent.setup();
    mount();
    const keyInput = screen.getByLabelText("Oceanway API Key", {
      selector: "input",
    });
    await waitFor(() => expect(keyInput).toBeEnabled());
    await user.type(keyInput, "fixture-key");
    await user.click(screen.getByText("连接并获取模型"));
    await waitFor(() =>
      expect(
        screen.getByRole("button", { name: "一键配置 2 个应用" }),
      ).toBeEnabled(),
    );
    await user.click(screen.getByRole("button", { name: "一键配置 2 个应用" }));
    expect(await screen.findByText("图片：图片程序启动失败")).toBeVisible();
    expect(screen.getByText("文本配置已保存")).toBeVisible();
    expect(screen.queryByText("Oceanway 图片工具已配置。")).toBeNull();
    expect(oceanwayApi.launch).not.toHaveBeenCalled();
  });
});
