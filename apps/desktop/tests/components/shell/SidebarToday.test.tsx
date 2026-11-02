import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { Sidebar, type SidebarProps } from "@/components/shell/Sidebar";
import type { Settings, VisibleApps } from "@/types";
import { createTestQueryClient } from "../../utils/testQueryClient";

const status = vi.hoisted(() => ({
  todayCost: 47.2,
  todayTokens: 120_400_000,
  todayRequests: 663,
  todayHitRate: 0.986,
}));

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: { value?: string }) =>
      options?.value != null ? `${key}:${options.value}` : key,
    i18n: { resolvedLanguage: "en" },
  }),
}));

vi.mock("@/contexts/UpdateContext", () => ({
  useUpdate: () => ({ hasUpdate: false, updateInfo: null }),
}));

vi.mock("@/hooks/useSidebarStatus", () => ({
  useSidebarStatus: () => ({
    appStatus: () => ({ mode: "direct", alert: false }),
    todayCost: status.todayCost,
    todayTokens: status.todayTokens,
    todayRequests: status.todayRequests,
    todayHitRate: status.todayHitRate,
    authNeedsAttention: false,
  }),
}));

const renderSidebar = (
  settings: Partial<Settings>,
  props: Partial<SidebarProps> = {},
) => {
  const client = createTestQueryClient();
  // 不让设置查询回头去 mock 后端重取，盖掉这里给的值
  client.setQueryDefaults(["settings"], { staleTime: Infinity });
  client.setQueryData(["settings"], settings);
  render(
    <QueryClientProvider client={client}>
      <Sidebar
        activeApp="claude"
        view="usage"
        visibleApps={{} as VisibleApps}
        settingsSection="general"
        onSelectApp={vi.fn()}
        onSelectPage={vi.fn()}
        onSelectSettingsSection={vi.fn()}
        onExitSettings={vi.fn()}
        {...props}
      />
    </QueryClientProvider>,
  );
};

describe("sidebar Today label", () => {
  it.each([
    [undefined, "nav.today:$47.20"],
    [["tokens"], "nav.today:120M"],
    [["requests"], "nav.today:nav.todayRequests:663"],
    [["hitRate"], "nav.today:nav.todayHitRate:98.6%"],
  ])("shows %s", (items, label) => {
    status.todayCost = 47.2;
    status.todayTokens = 120_400_000;
    status.todayRequests = 663;
    renderSidebar({ usageDisplay: { sidebarTodayItems: items } });
    expect(screen.getByText(label)).toBeInTheDocument();
  });

  it("stacks only the numbers when showing two items", async () => {
    // 带上「今日」一行放不下长币种，会挤掉「用量统计」；完整说法在读屏名字和悬停里
    status.todayCost = 47.2;
    status.todayTokens = 120_400_000;
    renderSidebar({ usageDisplay: { sidebarTodayItems: ["cost", "tokens"] } });
    expect(screen.getByText("$47.20")).toBeInTheDocument();
    expect(screen.getByText("120M")).toBeInTheDocument();
    expect(
      screen.getByRole("button", {
        name: "nav.usage · nav.today:$47.20 · 120M",
      }),
    ).toBeInTheDocument();

    // 「今日」只在悬停提示里
    await userEvent.hover(screen.getByText("$47.20"));
    expect(await screen.findByRole("tooltip")).toHaveTextContent(
      "nav.usage · nav.today:$47.20 · 120M",
    );
  });

  it("shows nothing when hidden", () => {
    renderSidebar({ usageDisplay: { sidebarTodayItems: [] } });
    expect(screen.queryByText(/^nav\.today/)).not.toBeInTheDocument();
  });

  it("skips the parts that are zero", () => {
    // 没配定价时金额是 0：金额 + Token 只剩 Token，只勾金额就整个不显示
    status.todayCost = 0;
    status.todayTokens = 5_000;
    renderSidebar({ usageDisplay: { sidebarTodayItems: ["cost", "tokens"] } });
    expect(screen.getByText("nav.today:5,000")).toBeInTheDocument();
  });

  it("leaves out the hit rate before the first request of the day", () => {
    status.todayRequests = 0;
    status.todayTokens = 0;
    status.todayCost = 0;
    renderSidebar({ usageDisplay: { sidebarTodayItems: ["hitRate"] } });
    expect(screen.queryByText(/^nav\.today/)).not.toBeInTheDocument();
  });

  it("hides the cost-only label while nothing is priced", () => {
    status.todayCost = 0;
    status.todayTokens = 5_000;
    renderSidebar({});
    expect(screen.queryByText(/^nav\.today/)).not.toBeInTheDocument();
  });
});

describe("Oceanway simple navigation", () => {
  const visibleApps = {
    codex: true,
    claude: true,
    "claude-desktop": true,
    dsh: true,
    hermes: true,
    kimi: true,
  } as VisibleApps;

  it("starts with connection and keeps common apps, sessions and installation accessible", async () => {
    const onSelectPage = vi.fn();
    renderSidebar({}, { view: "connection", visibleApps, onSelectPage });
    const navigation = screen.getAllByRole("button");
    expect(
      navigation.indexOf(screen.getByRole("button", { name: "连接 Oceanway" })),
    ).toBeLessThan(
      navigation.indexOf(screen.getByRole("button", { name: "Codex" })),
    );
    expect(screen.getByRole("button", { name: "Claude Code" })).toBeVisible();
    expect(screen.getByRole("button", { name: "nav.sessions" })).toBeVisible();
    expect(screen.queryByRole("button", { name: "Gemini CLI" })).toBeNull();
    expect(screen.queryByRole("button", { name: "MCP" })).toBeNull();
    await userEvent.click(screen.getByRole("button", { name: "nav.apps" }));
    expect(onSelectPage).toHaveBeenCalledWith("apps");
  });

  it("opens extra apps and advanced tools with keyboard-operable disclosures", async () => {
    const user = userEvent.setup();
    const onSelectApp = vi.fn();
    const onSelectPage = vi.fn();
    renderSidebar({}, { visibleApps, onSelectApp, onSelectPage });
    const more = screen.getByRole("button", { name: "更多应用" });
    expect(more).toHaveAttribute("aria-expanded", "false");
    more.focus();
    await user.keyboard("{Enter}");
    expect(more).toHaveAttribute("aria-expanded", "true");
    await user.click(screen.getByRole("button", { name: "DeepSeek Harness" }));
    expect(onSelectApp).toHaveBeenCalledWith("dsh");

    const advanced = screen.getByRole("button", { name: "高级功能" });
    advanced.focus();
    await user.keyboard(" ");
    expect(advanced).toHaveAttribute("aria-expanded", "true");
    await user.click(screen.getByRole("button", { name: "MCP" }));
    expect(onSelectPage).toHaveBeenCalledWith("mcp");
    await user.click(advanced);
    expect(screen.queryByRole("button", { name: "MCP" })).toBeNull();
  });

  it("keeps an active extra app visible even when its group is folded", async () => {
    const user = userEvent.setup();
    renderSidebar({}, { visibleApps, view: "providers", activeApp: "dsh" });
    expect(screen.getByRole("button", { name: "DeepSeek Harness" })).toHaveAttribute(
      "aria-current",
      "page",
    );
    expect(screen.queryByRole("button", { name: "Hermes" })).toBeNull();
    await user.click(screen.getByRole("button", { name: "更多应用" }));
    expect(screen.getByRole("button", { name: "Hermes" })).toBeVisible();
    await user.click(screen.getByRole("button", { name: "更多应用" }));
    expect(screen.getByRole("button", { name: "DeepSeek Harness" })).toBeVisible();
  });

  it("keeps Skills selected when the discovery screen is active", async () => {
    renderSidebar({}, { visibleApps, view: "skillsDiscovery" });
    expect(screen.getByRole("button", { name: "Skills" })).toHaveAttribute(
      "aria-current",
      "page",
    );
    expect(screen.queryByRole("button", { name: "MCP" })).toBeNull();
  });

  it("keeps extra apps and advanced tools reachable in the collapsed icon rail", async () => {
    localStorage.setItem("cc-switch-sidebar-collapsed", "true");
    const user = userEvent.setup();
    const onSelectPage = vi.fn();
    renderSidebar({}, { visibleApps, onSelectPage });
    expect(
      screen.getByRole("button", { name: "nav.expandSidebar" }),
    ).toBeVisible();
    await user.click(screen.getByRole("button", { name: "更多应用" }));
    expect(screen.getByRole("button", { name: "Hermes" })).toBeVisible();
    await user.click(screen.getByRole("button", { name: "高级功能" }));
    await user.click(screen.getByRole("button", { name: "nav.prompts" }));
    expect(onSelectPage).toHaveBeenCalledWith("prompts");
  });
});
