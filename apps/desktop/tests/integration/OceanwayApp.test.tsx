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
  fireEvent.click(await nav.findByRole("button", { name, hidden: true }));
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
        HttpResponse.json(
          [
            ["codex", "Codex"],
            ["claude-desktop", "Claude Desktop"],
            ["claude", "Claude Code"],
            ["dsh", "DeepSeek Harness"],
            ["hermes", "Hermes"],
            ["kimi", "Kimi Code Desktop"],
          ].map(([appId, name]) => ({
            appId,
            name,
            installed: false,
            desktopInstalled: false,
            configExists: true,
            configurable: true,
            installAvailable: true,
          })),
        ),
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
    localStorage.setItem("oceanway-last-view", "usage");
    const keys: unknown[] = [];
    const links: unknown[] = [];
    server.use(
      http.post(`${endpoint}/open_external`, async ({ request }) => {
        links.push(await request.json());
        return HttpResponse.json(null);
      }),
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
    const nav = within(document.querySelector("nav")!);
    expect(
      nav.queryByRole("button", { name: "更多应用", hidden: true }),
    ).toBeNull();
    expect(
      nav.queryByRole("button", { name: "nav.usage", hidden: true }),
    ).toBeNull();
    for (const name of [
      "Codex",
      "Claude Desktop",
      "Claude Code",
      "DeepSeek Harness",
      "Hermes",
      "Kimi Code Desktop",
    ]) {
      expect(
        nav.getByRole("button", { name, hidden: true }),
      ).toBeInTheDocument();
    }
    fireEvent.click(
      nav.getByRole("button", { name: "登录 Oceanway 官网", hidden: true }),
    );
    await waitFor(() =>
      expect(links).toEqual([{ url: "https://oceanway.tech" }]),
    );
    await waitFor(() => expect(key).toBeEnabled());
    expect(
      screen.getByRole("heading", { name: "连接你的 AI 应用", hidden: true }),
    ).toBeInTheDocument();
    fireEvent.change(key, { target: { value: "test-oceanway-key" } });
    fireEvent.click(
      screen.getByRole("button", { name: "连接并继续", hidden: true }),
    );
    expect(await screen.findByText("Oceanway 已连接")).toBeInTheDocument();
    expect(keys).toEqual([
      {
        apiKey: "test-oceanway-key",
        baseUrl: "https://console.oceanway.tech/v1",
      },
    ]);
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
      await screen.findByLabelText("Oceanway API Key"),
    ).toBeInTheDocument();
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

  it("shows the six real installation entries and hides removed products", async () => {
    await mountApp();
    await screen.findByLabelText("Oceanway API Key");
    const nav = within(document.querySelector("nav")!);
    fireEvent.click(
      nav.getByRole("button", { name: "nav.apps", hidden: true }),
    );
    await screen.findByRole("heading", { name: "应用安装", hidden: true });
    for (const name of [
      "Codex",
      "Claude Desktop",
      "Claude Code",
      "DeepSeek Harness",
      "Hermes",
      "Kimi Code Desktop",
    ]) {
      const region = within(screen.getByRole("region", { name, hidden: true }));
      expect(
        region.getByRole("button", { name: "下载安装", hidden: true }),
      ).toBeEnabled();
    }
    expect(
      screen.queryByRole("region", { name: "OpenClaw", hidden: true }),
    ).toBeNull();
    expect(
      screen.queryByRole("region", { name: "Gemini CLI", hidden: true }),
    ).toBeNull();
    expect(localStorage.getItem("oceanway-last-view")).toBe("apps");
  });
});
