import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { vi, beforeEach, describe, it, expect } from "vitest";
import { OceanwayConfigPanel } from "@/components/oceanway/OceanwayConfigPanel";
import { oceanwayApi, type DetectedApp } from "@/lib/api/oceanway";
import { settingsApi } from "@/lib/api/settings";
import type { AppId } from "@/lib/api";
const installAction = vi.hoisted(() => vi.fn());
vi.mock("@/components/oceanway/AgentInstallProgress", () => ({
  useAgentInstaller: () => ({
    start: installAction,
    jobs: {},
    isBusy: () => false,
    installed: "",
    check: vi.fn(),
  }),
  AgentInstallProgress: () => null,
}));
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
vi.mock("@/lib/api/settings", () => ({
  settingsApi: { openExternal: vi.fn() },
}));
vi.mock("@/components/shell/AppGlyph", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/components/shell/AppGlyph")>()),
  AppGlyph: () => <span />,
}));

const models = [
  "claude-sonnet-4-5",
  "gpt-5",
  "gpt-5-mini",
  "gemini-2.5-pro",
  "gpt-image-1",
  "text-embedding-3-large",
].map((id) => ({ id }));
const detectedApps = (): DetectedApp[] =>
  ["codex", "claude"].map((appId) => ({
    appId,
    name: appId === "codex" ? "Codex" : "Claude Code",
    installed: true,
    configExists: true,
    configurable: true,
  }));
function mount(appId?: AppId, onOpenApps?: () => void) {
  return render(
    <QueryClientProvider
      client={
        new QueryClient({ defaultOptions: { queries: { retry: false } } })
      }
    >
      <OceanwayConfigPanel appId={appId} onOpenApps={onOpenApps} />
    </QueryClientProvider>,
  );
}
async function connect(user: ReturnType<typeof userEvent.setup>) {
  const input = screen.getByLabelText("Oceanway API Key", {
    selector: "input",
  });
  await waitFor(() => expect(input).toBeEnabled());
  await user.type(input, "fixture-key");
  await user.click(screen.getByRole("button", { name: "连接并继续" }));
  await screen.findByText("Oceanway 已连接");
  await waitFor(() =>
    expect(
      screen.getByRole("button", { name: "更换 Key 或地址" }),
    ).toBeEnabled(),
  );
}
async function expandModel(
  user: ReturnType<typeof userEvent.setup>,
  name: string,
) {
  await user.click(screen.getByRole("button", { name: `更换 ${name} 模型` }));
  return screen.getByLabelText(`${name} 对话默认模型`);
}
beforeEach(() => {
  vi.clearAllMocks();
  installAction.mockResolvedValue(undefined);
  vi.mocked(oceanwayApi.connection).mockResolvedValue({
    configured: false,
    keyPreview: null,
    baseUrl: "https://console.oceanway.tech/v1",
    configuredApps: [],
    defaultImageModel: null,
  });
  vi.mocked(oceanwayApi.detectApps).mockResolvedValue(detectedApps());
  vi.mocked(oceanwayApi.checkKey).mockResolvedValue({
    models,
    baseUrl: "https://console.oceanway.tech/v1",
    checkedAt: "2026-11-02",
  });
  vi.mocked(oceanwayApi.apply).mockResolvedValue({
    results: [
      {
        appId: "codex",
        ok: true,
        message: "配置已保存",
        imageConfigured: true,
      },
      { appId: "claude", ok: false, message: "配置文件只读" },
    ],
    restartRequired: true,
    backupPath: "fixture/backups",
    imagesConfigured: false,
  });
  vi.mocked(oceanwayApi.launch).mockResolvedValue({
    launched: true,
    restartRequired: false,
    message: "已打开新终端加载配置",
  });
  vi.mocked(settingsApi.openExternal).mockResolvedValue(undefined);
});

describe("Oceanway guided configuration", () => {
  it("defaults to Console and uses the edited service address for checking and saving", async () => {
    const user = userEvent.setup();
    const page = mount("codex");
    const address = await screen.findByLabelText("服务地址（Base URL）");
    await waitFor(() => expect(address).toBeEnabled());
    expect(address).toHaveValue("https://console.oceanway.tech/v1");
    await connect(user);
    await user.click(screen.getByRole("button", { name: "更换 Key 或地址" }));
    const edited = screen.getByLabelText("服务地址（Base URL）");
    await user.clear(edited);
    await user.type(edited, "https://example.com:8443/custom/v1/");
    expect(screen.queryByRole("button", { name: "一键配置" })).toBeNull();
    vi.mocked(oceanwayApi.checkKey).mockResolvedValueOnce({
      models,
      baseUrl: "https://example.com:8443/custom/v1",
      checkedAt: "2026-11-03",
    });
    await user.click(screen.getByRole("button", { name: "连接并继续" }));
    expect(await screen.findByText("服务已连接")).toBeVisible();
    expect(oceanwayApi.checkKey).toHaveBeenLastCalledWith(
      "fixture-key",
      "https://example.com:8443/custom/v1/",
    );
    vi.mocked(oceanwayApi.connection).mockResolvedValue({
      configured: true,
      keyPreview: "已保存",
      baseUrl: "https://example.com:8443/custom/v1",
      configuredApps: ["codex"],
      defaultImageModel: "gpt-image-1",
    });
    await user.click(screen.getByRole("button", { name: "一键配置" }));
    await waitFor(() =>
      expect(oceanwayApi.apply).toHaveBeenCalledWith(
        expect.objectContaining({
          baseUrl: "https://example.com:8443/custom/v1",
          apiKey: "fixture-key",
          imageModel: "gpt-image-1",
        }),
      ),
    );
    await screen.findByRole("heading", { name: "配置结果" });
    page.unmount();
    mount("codex");
    await waitFor(() =>
      expect(screen.getByLabelText("服务地址（Base URL）")).toHaveValue(
        "https://example.com:8443/custom/v1",
      ),
    );
    await user.click(screen.getByRole("button", { name: "恢复默认" }));
    expect(screen.getByLabelText("服务地址（Base URL）")).toHaveValue(
      "https://console.oceanway.tech/v1",
    );
    await user.click(screen.getAllByRole("button", { name: "重新检测" })[0]);
    await waitFor(() =>
      expect(screen.getByLabelText("服务地址（Base URL）")).toBeEnabled(),
    );
    expect(screen.getByLabelText("服务地址（Base URL）")).toHaveValue(
      "https://console.oceanway.tech/v1",
    );
  });

  it("completes text and image setup when the account also has video models", async () => {
    vi.mocked(oceanwayApi.checkKey).mockResolvedValueOnce({
      models: [...models, { id: "seedance-2.0", modelType: "video" }],
      baseUrl: "https://console.oceanway.tech/v1",
      checkedAt: "2026-11-03",
    });
    vi.mocked(oceanwayApi.apply).mockResolvedValueOnce({
      results: [
        {
          appId: "codex",
          ok: true,
          message: "配置已保存",
          imageConfigured: true,
        },
      ],
      restartRequired: true,
      backupPath: "fixture/backups",
      imagesConfigured: true,
    });
    const user = userEvent.setup();
    mount("codex");
    await connect(user);
    expect(screen.queryByText(/视频也会自动配好/)).toBeNull();
    await user.click(screen.getByRole("button", { name: "一键配置" }));
    expect(
      await screen.findByRole("heading", { name: "准备好了，开始使用吧" }),
    ).toBeVisible();
    expect(screen.getByText(/生图已配好/)).toBeVisible();
    expect(screen.queryByText(/视频已配好/)).toBeNull();
  });
  it("keeps incompatible models out of the other-series picker", async () => {
    vi.mocked(oceanwayApi.checkKey).mockResolvedValueOnce({
      models: [
        { id: "gpt-5", supportedProtocols: ["openai-responses"] },
        { id: "gpt-5-mini", supportedProtocols: ["openai-responses"] },
        {
          id: "kimi-k3-1",
          supportedProtocols: ["openai-chat", "openai-responses"],
        },
        { id: "claude-sonnet-5-5", supportedProtocols: ["anthropic-messages"] },
      ],
      baseUrl: "https://console.oceanway.tech/v1",
      checkedAt: "2026-11-03",
    });
    const user = userEvent.setup();
    mount("codex");
    await connect(user);
    await expandModel(user, "Codex");
    await user.click(screen.getByRole("button", { name: "选择其他系列模型" }));
    expect(
      screen.getByRole("checkbox", { name: "接入 Codex 对话 kimi-k3-1" }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("checkbox", {
        name: "接入 Codex 对话 claude-sonnet-5-5",
      }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("checkbox", { name: "接入 Codex 对话 gpt-5" }),
    ).toBeChecked();
    expect(
      screen.getByRole("checkbox", { name: "接入 Codex 对话 gpt-5-mini" }),
    ).toBeChecked();
  });
  it("saves multiple selected models and changes the default when its model is removed", async () => {
    vi.mocked(oceanwayApi.checkKey).mockResolvedValueOnce({
      models: [...models, { id: "gpt-5-nano" }, { id: "gpt-image-2" }],
      baseUrl: "https://console.oceanway.tech/v1",
      checkedAt: "2026-11-03",
    });
    const user = userEvent.setup();
    mount("codex");
    await connect(user);
    const picker = await expandModel(user, "Codex");
    await user.click(
      screen.getByRole("checkbox", { name: "接入 Codex 对话 gpt-5" }),
    );
    expect(picker).not.toHaveValue("gpt-5");
    await user.selectOptions(picker, "gpt-5-mini");
    await user.click(screen.getByRole("button", { name: "生图设置" }));
    await user.selectOptions(
      screen.getByLabelText("图片默认模型"),
      "gpt-image-1",
    );
    await user.click(screen.getByRole("button", { name: "一键配置" }));
    await waitFor(() =>
      expect(oceanwayApi.apply).toHaveBeenCalledWith(
        expect.objectContaining({
          apps: [
            {
              appId: "codex",
              model: "gpt-5-mini",
              models: ["gpt-5-nano", "gpt-5-mini"],
            },
          ],
          imageModel: "gpt-image-1",
          allowedImageModels: ["gpt-image-2", "gpt-image-1"],
        }),
      ),
    );
  });

  it("restores the saved selection when reloading models with the stored key", async () => {
    vi.mocked(oceanwayApi.connection).mockResolvedValueOnce({
      configured: true,
      keyPreview: "已保存",
      baseUrl: "https://console.oceanway.tech/v1",
      configuredApps: ["codex"],
      defaultImageModel: "gpt-image-1",
      allowedImageModels: ["gpt-image-1"],
      appModels: { codex: { model: "gpt-5-mini", models: ["gpt-5-mini"] } },
    });
    const user = userEvent.setup();
    mount("codex");
    await screen.findByPlaceholderText("Key 已保存，可直接连接");
    const connectButton = screen.getByRole("button", { name: "连接并继续" });
    await waitFor(() => expect(connectButton).toBeEnabled());
    await user.click(connectButton);
    await screen.findByText("Oceanway 已连接");
    const picker = await expandModel(user, "Codex");
    expect(picker).toHaveValue("gpt-5-mini");
    expect(
      screen.getByRole("checkbox", { name: "接入 Codex 对话 gpt-5-mini" }),
    ).toBeChecked();
    expect(
      screen.getByRole("checkbox", { name: "接入 Codex 对话 gpt-5" }),
    ).not.toBeChecked();
  });

  it("offers installation management instead of an unusable Key form for an unadapted app", async () => {
    const openApps = vi.fn();
    const user = userEvent.setup();
    mount("kimi", openApps);
    expect(
      await screen.findByRole("heading", {
        name: "Kimi Code Desktop 尚未开放 Oceanway 接入",
      }),
    ).toBeVisible();
    expect(screen.queryByLabelText("Oceanway API Key")).toBeNull();
    expect(screen.queryByRole("button", { name: "重新检测" })).toBeNull();
    expect(screen.queryByRole("button", { name: "连接并继续" })).toBeNull();
    await user.click(screen.getByRole("button", { name: "管理应用安装" }));
    expect(openApps).toHaveBeenCalledTimes(1);
    expect(oceanwayApi.checkKey).not.toHaveBeenCalled();
    expect(oceanwayApi.apply).not.toHaveBeenCalled();
  });

  it("configures a detected Codex desktop without a CLI and asks before restarting it", async () => {
    vi.mocked(oceanwayApi.detectApps).mockResolvedValueOnce([
      {
        appId: "codex",
        name: "Codex",
        installed: true,
        desktopInstalled: true,
        configExists: false,
        configurable: true,
      },
    ]);
    const user = userEvent.setup();
    mount();
    expect(await screen.findByText("已检测到 Codex 桌面版")).toBeVisible();
    expect(screen.getByRole("checkbox", { name: "配置 Codex" })).toBeEnabled();
    expect(screen.getByRole("checkbox", { name: "配置 Codex" })).toBeChecked();
    await connect(user);
    expect(screen.queryByLabelText("Codex 对话默认模型")).toBeNull();
    await user.click(screen.getByRole("button", { name: "一键配置" }));
    await screen.findByRole("region", { name: "配置结果" });
    expect(oceanwayApi.apply).toHaveBeenCalledWith(
      expect.objectContaining({
        apps: [
          { appId: "codex", model: "gpt-5", models: ["gpt-5-mini", "gpt-5"] },
        ],
      }),
    );
    expect(oceanwayApi.launch).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "打开 Codex" }));
    const dialog = await screen.findByRole("dialog");
    expect(oceanwayApi.launch).not.toHaveBeenCalled();
    await user.click(
      within(dialog).getByRole("button", { name: "保存好了，打开应用" }),
    );
    await waitFor(() =>
      expect(oceanwayApi.launch).toHaveBeenCalledWith("codex", true),
    );
  });

  it("keeps all current Console image models separate from conversation families", () => {
    const combined = [...liveCatalog.models, ...liveImageCatalog.models];
    expect(imageModels(combined).sort()).toEqual(
      liveImageCatalog.models.map((model) => model.id).sort(),
    );
    expect(imageModels(combined)).toHaveLength(8);
    expect(recommendedImageModel(imageModels(combined))).toBe("gpt-image-2");
    for (const { id } of liveImageCatalog.models)
      expect(conversationModels(combined)).not.toContain(id);
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

  it("keeps model choices collapsed but exposes every real family model on demand", async () => {
    vi.mocked(oceanwayApi.detectApps).mockResolvedValue(detectedApps());
    vi.mocked(oceanwayApi.checkKey).mockResolvedValueOnce(liveCatalog);
    const user = userEvent.setup();
    mount();
    await connect(user);
    expect(screen.getByText("48 个可用模型，已为你选好默认模型")).toBeVisible();
    expect(screen.queryByRole("combobox")).toBeNull();
    for (const [name, prefix] of [
      ["Codex", "gpt-"],
      ["Claude Code", "claude-"],
    ]) {
      const select = await expandModel(user, name);
      const actual = within(select)
        .getAllByRole("option")
        .map((option) => (option as HTMLOptionElement).value)
        .filter(Boolean)
        .sort();
      const expected = liveCatalog.models
        .filter((m) => m.modelType === "text")
        .map((m) => m.id)
        .filter((id) => id.startsWith(prefix))
        .sort();
      expect(actual).toEqual(expected);
      expect(actual.length).toBeGreaterThan(1);
    }
    expect(screen.getByLabelText("Codex 对话默认模型")).toHaveValue(
      "gpt-6.1-sol",
    );
    expect(screen.getByLabelText("Claude Code 对话默认模型")).toHaveValue(
      "claude-sonnet-5-5",
    );
    expect(
      screen.getByText("共 8 个图片模型，可在对话中选择，支持一次生成多张。"),
    ).toBeVisible();
    expect(screen.queryByLabelText("图片默认模型")).toBeNull();
    expect(screen.getByRole("button", { name: "一键配置" })).toBeEnabled();
  });

  it("saves defaults, reports partial failure, clears the Key and never opens apps automatically", async () => {
    const user = userEvent.setup();
    mount();
    await connect(user);
    expect(screen.queryByRole("combobox")).toBeNull();
    await user.click(screen.getByRole("button", { name: "一键配置" }));
    const result = await screen.findByRole("region", { name: "配置结果" });
    expect(
      within(result)
        .getAllByText("配置文件只读")
        .some((node) => node.closest("details") === null),
    ).toBe(true);
    expect(oceanwayApi.apply).toHaveBeenCalledWith(
      expect.objectContaining({
        apiKey: "fixture-key",
        imageModel: "gpt-image-1",
        apps: [
          { appId: "codex", model: "gpt-5", models: ["gpt-5-mini", "gpt-5"] },
          {
            appId: "claude",
            model: "claude-sonnet-4-5",
            models: ["claude-sonnet-4-5"],
          },
        ],
      }),
    );
    expect(oceanwayApi.launch).not.toHaveBeenCalled();
    expect(
      within(result).getByRole("button", { name: "打开 Codex" }),
    ).toBeVisible();
    expect(
      within(result).queryByRole("button", { name: "打开 Claude Code" }),
    ).toBeNull();
    await user.click(screen.getByRole("button", { name: "返回调整" }));
    await user.click(screen.getByRole("button", { name: "更换 Key 或地址" }));
    expect(
      screen.getByLabelText("Oceanway API Key", { selector: "input" }),
    ).toHaveValue("");
    expect(Object.values(localStorage)).not.toContain("fixture-key");
  });

  it("invalidates checked models when changing the Key and blocks save after rejection", async () => {
    const user = userEvent.setup();
    mount();
    await connect(user);
    await user.click(screen.getByRole("button", { name: "更换 Key 或地址" }));
    await user.type(
      screen.getByLabelText("Oceanway API Key", { selector: "input" }),
      "-changed",
    );
    expect(screen.queryByRole("button", { name: "一键配置" })).toBeNull();
    vi.mocked(oceanwayApi.checkKey).mockRejectedValueOnce(
      new Error("Key 无效"),
    );
    await user.click(screen.getByRole("button", { name: "连接并继续" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Key 无效");
    expect(oceanwayApi.apply).not.toHaveBeenCalled();
  });

  it("offers own-family models first and saves an explicit other-family choice", async () => {
    const user = userEvent.setup();
    mount();
    await connect(user);
    const codex = await expandModel(user, "Codex");
    const claude = await expandModel(user, "Claude Code");
    expect(codex).toHaveValue("gpt-5");
    expect(
      within(codex).queryByRole("option", { name: "claude-sonnet-4-5" }),
    ).toBeNull();
    expect(within(claude).queryByRole("option", { name: "gpt-5" })).toBeNull();
    expect(
      within(codex).queryByRole("option", { name: "gpt-image-1" }),
    ).toBeNull();
    await user.click(
      screen.getAllByRole("button", { name: "选择其他系列模型" })[0],
    );
    await user.click(
      screen.getByRole("checkbox", {
        name: "接入 Codex 对话 claude-sonnet-4-5",
      }),
    );
    await user.selectOptions(codex, "claude-sonnet-4-5");
    expect(within(claude).queryByRole("option", { name: "gpt-5" })).toBeNull();
    await user.click(screen.getByRole("button", { name: "一键配置" }));
    await waitFor(() =>
      expect(oceanwayApi.apply).toHaveBeenCalledWith(
        expect.objectContaining({
          apps: [
            {
              appId: "codex",
              model: "claude-sonnet-4-5",
              models: ["gpt-5-mini", "gpt-5", "claude-sonnet-4-5"],
            },
            {
              appId: "claude",
              model: "claude-sonnet-4-5",
              models: ["claude-sonnet-4-5"],
            },
          ],
        }),
      ),
    );
  });

  it("retains the selected other-family model when returning to recommended models", async () => {
    const user = userEvent.setup();
    mount();
    await connect(user);
    const codex = await expandModel(user, "Codex");
    await user.click(screen.getByRole("button", { name: "选择其他系列模型" }));
    await user.click(
      screen.getByRole("checkbox", {
        name: "接入 Codex 对话 claude-sonnet-4-5",
      }),
    );
    await user.selectOptions(codex, "claude-sonnet-4-5");
    await user.click(screen.getByRole("button", { name: "只看推荐系列" }));
    expect(codex).toHaveValue("claude-sonnet-4-5");
    expect(
      within(codex).getByRole("option", { name: "claude-sonnet-4-5" }),
    ).toHaveProperty("selected", true);
    expect(
      within(codex).queryByRole("option", { name: "gemini-2.5-pro" }),
    ).toBeNull();
    await user.click(screen.getByRole("button", { name: "一键配置" }));
    await waitFor(() =>
      expect(oceanwayApi.apply).toHaveBeenCalledWith(
        expect.objectContaining({
          apps: [
            {
              appId: "codex",
              model: "claude-sonnet-4-5",
              models: ["gpt-5-mini", "gpt-5", "claude-sonnet-4-5"],
            },
            {
              appId: "claude",
              model: "claude-sonnet-4-5",
              models: ["claude-sonnet-4-5"],
            },
          ],
        }),
      ),
    );
  });

  it("keeps text success visible when image setup needs attention", async () => {
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
    await connect(user);
    await user.click(screen.getByRole("button", { name: "一键配置" }));
    expect(await screen.findByText("生图：图片程序启动失败")).toBeVisible();
    expect(screen.getByText("对话已配好")).toBeVisible();
    expect(screen.queryByText(/生图已配好/)).toBeNull();
    expect(oceanwayApi.launch).not.toHaveBeenCalled();
  });

  it("lets users choose a different image model only after opening image settings", async () => {
    vi.mocked(oceanwayApi.checkKey).mockResolvedValueOnce({
      ...liveCatalog,
      models: [...liveCatalog.models, ...liveImageCatalog.models],
    });
    const user = userEvent.setup();
    mount();
    await connect(user);
    expect(screen.queryByLabelText("图片默认模型")).toBeNull();
    await user.click(screen.getByRole("button", { name: "生图设置" }));
    const images = screen.getByLabelText("图片默认模型");
    expect(images).toHaveValue("gpt-image-2");
    expect(within(images).getAllByRole("option")).toHaveLength(8);
    const alternative = liveImageCatalog.models.find(
      (model) => model.id !== "gpt-image-2",
    )!.id;
    await user.selectOptions(images, alternative);
    await user.click(screen.getByRole("button", { name: "一键配置" }));
    await waitFor(() =>
      expect(oceanwayApi.apply).toHaveBeenCalledWith(
        expect.objectContaining({ imageModel: alternative }),
      ),
    );
  });

  it("downloads the offline Codex desktop package without claiming it is installed", async () => {
    vi.mocked(oceanwayApi.detectApps).mockResolvedValue(
      detectedApps().map((app) => ({
        ...app,
        installed: false,
        configExists: false,
      })),
    );
    const user = userEvent.setup();
    mount();
    const codexInstall = await screen.findByRole("button", {
      name: "下载安装 Codex",
    });
    expect(screen.getByRole("checkbox", { name: "配置 Codex" })).toBeDisabled();
    expect(
      screen.getByRole("checkbox", { name: "配置 Claude Code" }),
    ).toBeDisabled();
    await user.click(codexInstall);
    expect(installAction).toHaveBeenCalledWith("codex");
    expect(settingsApi.openExternal).not.toHaveBeenCalled();
    expect(oceanwayApi.detectApps).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("checkbox", { name: "配置 Codex" })).toBeDisabled();
    await waitFor(() => expect(codexInstall).toBeEnabled());
    await user.click(
      screen.getByRole("button", { name: "下载安装 Claude Code" }),
    );
    expect(installAction).toHaveBeenCalledWith("claude");
    expect(oceanwayApi.detectApps).toHaveBeenCalledTimes(1);
    expect(oceanwayApi.apply).not.toHaveBeenCalled();
  });

  it("keeps a user's unchecked application unchecked when detecting again", async () => {
    const user = userEvent.setup();
    mount();
    const claude = await screen.findByRole("checkbox", {
      name: "配置 Claude Code",
    });
    await waitFor(() => expect(claude).toBeEnabled());
    await user.click(claude);
    await user.click(screen.getByRole("button", { name: "重新检测" }));
    await waitFor(() =>
      expect(oceanwayApi.detectApps).toHaveBeenCalledTimes(2),
    );
    expect(claude).not.toBeChecked();
    expect(screen.getByRole("checkbox", { name: "配置 Codex" })).toBeChecked();
  });

  it("keeps the Key during the download and selects Codex only after detecting the installation", async () => {
    vi.mocked(oceanwayApi.detectApps)
      .mockResolvedValueOnce(
        detectedApps().map((app) => ({
          ...app,
          installed: false,
          configExists: false,
        })),
      )
      .mockResolvedValue(
        detectedApps().map((app) => ({
          ...app,
          installed: app.appId === "codex",
          configExists: false,
        })),
      );
    let finish!: () => void;
    installAction.mockImplementationOnce(
      () =>
        new Promise<void>((resolve) => {
          finish = resolve;
        }),
    );
    const user = userEvent.setup();
    mount();
    const install = await screen.findByRole("button", {
      name: "下载安装 Codex",
    });
    await user.type(
      screen.getByLabelText("Oceanway API Key", { selector: "input" }),
      "kept-fixture-key",
    );
    await user.click(install);
    expect(
      screen.getByRole("button", { name: "下载安装 Claude Code" }),
    ).toBeEnabled();
    expect(installAction).toHaveBeenCalledTimes(1);
    finish();
    await waitFor(() => expect(install).toBeEnabled());
    expect(screen.getByRole("checkbox", { name: "配置 Codex" })).toBeDisabled();
    await user.click(screen.getByRole("button", { name: "重新检测" }));
    await waitFor(() =>
      expect(
        screen.getByRole("checkbox", { name: "配置 Codex" }),
      ).toBeChecked(),
    );
    expect(
      screen.getByLabelText("Oceanway API Key", { selector: "input" }),
    ).toHaveValue("kept-fixture-key");
    expect(screen.queryByRole("button", { name: "下载安装 Codex" })).toBeNull();
  });

  it("keeps the download action available when opening the installer link fails", async () => {
    vi.mocked(oceanwayApi.detectApps).mockResolvedValue(
      detectedApps().map((app) => ({
        ...app,
        installed: false,
        configExists: false,
      })),
    );
    installAction.mockRejectedValueOnce(new Error("下载失败，请重试"));
    const user = userEvent.setup();
    mount();
    await user.click(
      await screen.findByRole("button", { name: "下载安装 Codex" }),
    );
    expect(await screen.findByText("下载失败，请重试")).toBeVisible();
    expect(
      screen.getByRole("button", { name: "下载安装 Codex" }),
    ).toBeEnabled();
    expect(screen.getByRole("checkbox", { name: "配置 Codex" })).toBeDisabled();
  });

  it("selects a newly installed common application after detection", async () => {
    vi.mocked(oceanwayApi.detectApps).mockResolvedValueOnce(
      detectedApps().map((app) => ({
        ...app,
        installed: false,
        configExists: false,
      })),
    );
    const user = userEvent.setup();
    mount();
    await screen.findByRole("button", { name: "下载安装 Codex" });
    await user.click(screen.getByRole("button", { name: "重新检测" }));
    await waitFor(() =>
      expect(
        screen.getByRole("checkbox", { name: "配置 Codex" }),
      ).toBeChecked(),
    );
    expect(
      screen.getByRole("checkbox", { name: "配置 Claude Code" }),
    ).toBeChecked();
    expect(screen.queryByRole("button", { name: "下载安装 Codex" })).toBeNull();
  });

  it("offers detection on a single-app page and enables the newly installed app", async () => {
    vi.mocked(oceanwayApi.detectApps).mockResolvedValueOnce(
      detectedApps().map((app) => ({
        ...app,
        installed: false,
        configExists: false,
      })),
    );
    const user = userEvent.setup();
    mount("codex");
    await screen.findByRole("button", { name: "下载安装 Codex" });
    expect(screen.getByRole("checkbox", { name: "配置 Codex" })).toBeDisabled();
    expect(
      screen.queryByRole("checkbox", { name: "配置 Claude Code" }),
    ).toBeNull();
    const detect = screen.getByRole("button", { name: "重新检测" });
    expect(detect).toBeEnabled();
    await user.click(detect);
    await waitFor(() =>
      expect(
        screen.getByRole("checkbox", { name: "配置 Codex" }),
      ).toBeEnabled(),
    );
    expect(screen.getByRole("checkbox", { name: "配置 Codex" })).toBeChecked();
    expect(screen.queryByRole("button", { name: "下载安装 Codex" })).toBeNull();
  });

  it("allows installation and model changes together when only an old config is detected", async () => {
    vi.mocked(oceanwayApi.detectApps).mockResolvedValueOnce([
      {
        appId: "codex",
        name: "Codex",
        installed: false,
        configExists: true,
        configurable: true,
      },
    ]);
    const user = userEvent.setup();
    mount("codex");
    await connect(user);
    expect(screen.getByRole("checkbox", { name: "配置 Codex" })).toBeEnabled();
    expect(screen.getByRole("checkbox", { name: "配置 Codex" })).toBeChecked();
    const install = screen.getByRole("button", { name: "下载安装 Codex" });
    expect(install).toBeEnabled();
    const codex = await expandModel(user, "Codex");
    expect(codex).toHaveValue("gpt-5");
    await user.selectOptions(codex, "gpt-5-mini");
    await user.click(install);
    expect(installAction).toHaveBeenCalledWith("codex");
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "一键配置" })).toBeEnabled(),
    );
    await user.click(screen.getByRole("button", { name: "一键配置" }));
    await waitFor(() =>
      expect(oceanwayApi.apply).toHaveBeenCalledWith(
        expect.objectContaining({
          apps: [
            {
              appId: "codex",
              model: "gpt-5-mini",
              models: ["gpt-5-mini", "gpt-5"],
            },
          ],
        }),
      ),
    );
  });

  it("opens the configured CLI only on the user's explicit click", async () => {
    const user = userEvent.setup();
    mount();
    await connect(user);
    await user.click(screen.getByRole("button", { name: "一键配置" }));
    const open = await screen.findByRole("button", { name: "打开 Codex" });
    expect(oceanwayApi.launch).not.toHaveBeenCalled();
    await user.click(open);
    await waitFor(() =>
      expect(oceanwayApi.launch).toHaveBeenCalledWith("codex", false),
    );
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(await screen.findByRole("status")).toHaveTextContent(
      "已打开新终端加载配置",
    );
  });
});
