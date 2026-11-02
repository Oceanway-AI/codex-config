import { Suspense } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { http, HttpResponse } from "msw";
import { OCEANWAY_EDITION } from "@/config/edition";
import { resetProviderState } from "../msw/state";
import { server } from "../msw/server";

const mocks = vi.hoisted(() => ({
  success: vi.fn(),
  error: vi.fn(),
  window: {
    isMaximized: vi.fn(async () => false),
    isFullscreen: vi.fn(async () => false),
    isResizable: vi.fn(async () => true),
    scaleFactor: vi.fn(async () => 1),
    setDecorations: vi.fn(async () => {}),
    onResized: vi.fn(async () => () => {}),
    onScaleChanged: vi.fn(async () => () => {}),
  },
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => mocks.window,
}));
vi.mock("@/contexts/UpdateContext", () => ({
  useUpdate: () => ({ hasUpdate: false, updateInfo: null }),
}));
vi.mock("sonner", () => ({
  toast: {
    success: mocks.success,
    error: mocks.error,
    info: vi.fn(),
    warning: vi.fn(),
  },
}));
// CodeMirror needs browser selection/layout. Keep the form and API hooks real.
vi.mock("@/components/JsonEditor", () => ({
  default: ({
    value,
    onChange,
  }: {
    value: string;
    onChange: (value: string) => void;
  }) => (
    <textarea
      aria-label="JSON configuration"
      value={value}
      onChange={(event) => onChange(event.target.value)}
    />
  ),
}));

const endpoint = "http://tauri.local";
const mountApp = async () => {
  const { default: App } = await import("@/App");
  return render(
    <QueryClientProvider
      client={
        new QueryClient({
          defaultOptions: {
            queries: { retry: false },
            mutations: { retry: false },
          },
        })
      }
    >
      <Suspense fallback={<div>Loading application</div>}>
        <App />
      </Suspense>
    </QueryClientProvider>,
  );
};

async function openApp(name: string) {
  const nav = within(document.querySelector("nav")!);
  fireEvent.click(nav.getByRole("button", { name: "更多应用", hidden: true }));
  fireEvent.click(await nav.findByRole("button", { name, hidden: true }));
}

async function openClawConfiguration(tab: "env" | "agents") {
  await mountApp();
  await screen.findByLabelText("Oceanway API Key");
  await openApp("OpenClaw");
  const appTabs = within(
    await screen.findByRole("tablist", { name: "OpenClaw", hidden: true }),
  );
  fireEvent.click(
    appTabs.getByRole("tab", { name: "appPage.openclawConfig", hidden: true }),
  );
  const configTabs = within(
    await screen.findByRole("tablist", {
      name: "appPage.openclawConfig",
      hidden: true,
    }),
  );
  fireEvent.click(
    configTabs.getByRole("tab", {
      name: `openclaw.${tab}.title`,
      hidden: true,
    }),
  );
}

describe("Oceanway application with real edition and MSW", () => {
  beforeEach(() => {
    resetProviderState();
    localStorage.clear();
    mocks.success.mockReset();
    mocks.error.mockReset();
    server.use(
      http.post(`${endpoint}/oceanway_get_connection`, () =>
        HttpResponse.json({
          configured: false,
          keyPreview: null,
          baseUrl: "https://console.oceanway.tech/v1",
          configuredApps: [],
          defaultImageModel: null,
        }),
      ),
      http.post(`${endpoint}/oceanway_detect_apps`, () =>
        HttpResponse.json([
          {
            appId: "codex",
            name: "Codex",
            installed: true,
            configExists: true,
            configurable: true,
          },
          {
            appId: "claude",
            name: "Claude Code",
            installed: true,
            configExists: true,
            configurable: true,
          },
          {
            appId: "gemini",
            name: "Gemini CLI",
            installed: true,
            configExists: true,
            configurable: true,
          },
          {
            appId: "opencode",
            name: "OpenCode",
            installed: true,
            configExists: true,
            configurable: true,
          },
          {
            appId: "openclaw",
            name: "OpenClaw",
            installed: true,
            configExists: true,
            configurable: true,
          },
        ]),
      ),
      http.post(`${endpoint}/get_openclaw_env`, () =>
        HttpResponse.json({ vars: { EXISTING_FLAG: "on" } }),
      ),
      http.post(`${endpoint}/get_openclaw_agents_defaults`, () =>
        HttpResponse.json({
          workspace: "~/existing",
          maxConcurrent: 2,
          customOption: "preserved",
        }),
      ),
    );
  });

  it("opens Oceanway connection by default and checks the entered key through its API", async () => {
    expect(OCEANWAY_EDITION).toBe(true);
    const keys: unknown[] = [];
    server.use(
      http.post(`${endpoint}/oceanway_check_key`, async ({ request }) => {
        keys.push(await request.json());
        return HttpResponse.json({
          baseUrl: "https://console.oceanway.tech/v1",
          checkedAt: "2026-11-02",
          models: [{ id: "gpt-5.2" }, { id: "claude-sonnet-4-5" }],
        });
      }),
    );
    await mountApp();
    const key = await screen.findByLabelText("Oceanway API Key");
    await waitFor(() => expect(key).toBeEnabled());
    expect(
      screen.getByRole("heading", { name: "连接你的 AI 应用", hidden: true }),
    ).toBeInTheDocument();
    fireEvent.change(key, { target: { value: "test-oceanway-key" } });
    fireEvent.click(
      screen.getByRole("button", { name: "连接并继续", hidden: true }),
    );
    expect(await screen.findByText("Oceanway 已连接")).toBeInTheDocument();
    expect(keys).toEqual([{ apiKey: "test-oceanway-key" }]);
    expect(localStorage.getItem("oceanway-last-view")).toBe("connection");
  }, 15_000);

  it("keeps Hermes header Save wired to the active memory file", async () => {
    const writes: unknown[] = [];
    server.use(
      http.post(`${endpoint}/set_hermes_memory`, async ({ request }) => {
        writes.push(await request.json());
        return HttpResponse.json(null);
      }),
    );
    await mountApp();
    await screen.findByLabelText("Oceanway API Key");
    await openApp("Hermes");
    expect(
      await screen.findByRole("heading", {
        name: "Hermes 尚未开放 Oceanway 接入",
        hidden: true,
      }),
    ).toBeInTheDocument();
    expect(screen.queryByLabelText("Oceanway API Key")).toBeNull();
    const tabs = within(
      await screen.findByRole("tablist", { name: "Hermes", hidden: true }),
    );
    fireEvent.click(
      tabs.getByRole("tab", { name: "appPage.memory", hidden: true }),
    );
    const editor = (
      await screen.findAllByRole("textbox", {
        name: "hermes.memory.editorLabel",
        hidden: true,
      })
    )[0];
    await waitFor(() => expect(editor).toHaveValue("agent notes"));
    const header = within(document.querySelector("header")!);
    expect(
      header.getByRole("button", { name: "common.save", hidden: true }),
    ).toHaveAttribute("aria-disabled", "true");
    fireEvent.change(editor, { target: { value: "Oceanway saved memory" } });
    const save = header.getByRole("button", {
      name: "common.save",
      hidden: true,
    });
    expect(save).not.toHaveAttribute("aria-disabled");
    fireEvent.click(save);
    await waitFor(() =>
      expect(writes).toEqual([
        { kind: "memory", content: "Oceanway saved memory" },
      ]),
    );
    await waitFor(() =>
      expect(
        header.getByRole("button", { name: "common.save", hidden: true }),
      ).toHaveAttribute("aria-disabled", "true"),
    );
  });

  it("opens the real installation manager from an app without Oceanway onboarding support", async () => {
    const versionReads: unknown[] = [];
    server.use(
      http.post(`${endpoint}/get_tool_versions`, async ({ request }) => {
        const { tools } = (await request.json()) as { tools: string[] };
        versionReads.push(tools);
        return HttpResponse.json(
          tools.map((name) => ({
            name,
            version: null,
            latest_version: "1.0.0",
            error: null,
            installed_but_broken: false,
            env_type: "windows",
            wsl_distro: null,
          })),
        );
      }),
      http.post(`${endpoint}/list_tool_installations`, () =>
        HttpResponse.json([]),
      ),
    );
    await mountApp();
    await screen.findByLabelText("Oceanway API Key");
    await openApp("Kimi Code");
    expect(
      await screen.findByRole("heading", {
        name: "Kimi Code 尚未开放 Oceanway 接入",
        hidden: true,
      }),
    ).toBeInTheDocument();
    expect(screen.queryByLabelText("Oceanway API Key")).toBeNull();
    fireEvent.click(
      screen.getByRole("button", { name: "管理应用安装", hidden: true }),
    );
    await waitFor(() =>
      expect(localStorage.getItem("oceanway-last-view")).toBe("apps"),
    );
    await waitFor(() => expect(versionReads).toContainEqual(["kimi"]));
    expect(
      (
        await screen.findAllByRole("button", {
          name: "settings.toolInstall",
          hidden: true,
        })
      ).length,
    ).toBeGreaterThan(0);
  });

  it("saves OpenClaw environment edits from its existing configuration tab", async () => {
    const writes: unknown[] = [];
    server.use(
      http.post(`${endpoint}/set_openclaw_env`, async ({ request }) => {
        writes.push(await request.json());
        return HttpResponse.json({ changed: true });
      }),
    );
    await openClawConfiguration("env");
    const editor = await screen.findByLabelText("JSON configuration");
    await waitFor(() =>
      expect(editor).toHaveValue(
        JSON.stringify({ vars: { EXISTING_FLAG: "on" } }, null, 2),
      ),
    );
    const env = { vars: { EXISTING_FLAG: "on", OCEANWAY_MODE: "enabled" } };
    fireEvent.change(editor, { target: { value: JSON.stringify(env) } });
    fireEvent.click(
      screen.getByRole("button", { name: "common.save", hidden: true }),
    );
    await waitFor(() => expect(writes).toEqual([{ env }]));
  });

  it("saves OpenClaw Agent defaults while preserving other existing fields", async () => {
    const writes: unknown[] = [];
    server.use(
      http.post(
        `${endpoint}/set_openclaw_agents_defaults`,
        async ({ request }) => {
          writes.push(await request.json());
          return HttpResponse.json({ changed: true });
        },
      ),
    );
    await openClawConfiguration("agents");
    const workspace = await screen.findByPlaceholderText("~/projects");
    await waitFor(() => expect(workspace).toHaveValue("~/existing"));
    fireEvent.change(workspace, { target: { value: "~/oceanway-work" } });
    fireEvent.click(
      screen.getByRole("button", { name: "common.save", hidden: true }),
    );
    await waitFor(() =>
      expect(writes).toEqual([
        {
          defaults: {
            workspace: "~/oceanway-work",
            maxConcurrent: 2,
            customOption: "preserved",
          },
        },
      ]),
    );
  });
});
