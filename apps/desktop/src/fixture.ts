// Explicit local renderer fixture. Not included in the production build inputs.
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
if (!import.meta.env.DEV) throw new Error("Fixture is for development only");
const apps = [
  ["codex", "Codex"],
  ["claude", "Claude Code"],
  ["gemini", "Gemini CLI"],
  ["claude-desktop", "Claude Desktop"],
  ["opencode", "OpenCode"],
  ["openclaw", "OpenClaw"],
  ["grokbuild", "Grok Build"],
  ["hermes", "Hermes"],
  ["pi", "Pi"],
  ["mcode", "MiniMax Code"],
  ["kimi", "Kimi Code"],
  ["omp", "Oh My Pi"],
];
let configured = false;
localStorage.setItem("oceanway-last-view", "connection");
localStorage.setItem("i18nextLng", "zh");
mockWindows("main");
mockIPC(async (command, args: any) => {
  if (command === "oceanway_get_connection")
    return {
      configured,
      keyPreview: configured ? "fixture…key" : null,
      baseUrl: "https://console.oceanway.tech/v1",
      configuredApps: [],
      defaultImageModel: null,
    };
  if (command === "oceanway_detect_apps")
    return apps.map(([appId, name], index) => ({
      appId,
      name,
      installed: index < 3,
      configExists: index < 2,
      configurable: [
        "codex",
        "claude",
        "gemini",
        "opencode",
        "openclaw",
      ].includes(appId),
      reason: "正在适配",
    }));
  if (command === "oceanway_check_key") {
    if (!configured && args.apiKey !== "fixture-key")
      throw new Error("界面测试请填写 fixture-key，不使用真实 Key。");
    return {
      models: [
        "gpt-5",
        "claude-sonnet-4-5",
        "gemini-2.5-pro",
        "gpt-image-1",
      ].map((id) => ({ id })),
      baseUrl: "https://console.oceanway.tech/v1",
      checkedAt: "2026-11-02",
    };
  }
  if (command === "oceanway_apply_config") {
    configured = true;
    return {
      results: args.request.apps.map((a: any) => ({
        appId: a.appId,
        ok: true,
        message: "界面测试：配置结果示例",
      })),
      restartRequired: true,
      backupPath: "fixture/backups",
      imagesConfigured: !!args.request.imageModel,
    };
  }
  if (command === "oceanway_launch_app")
    return {
      launched: false,
      restartRequired: true,
      message: "界面测试不会启动或重启本机应用。",
    };
  if (command === "get_settings")
    return {
      language: "zh",
      checkToolUpdatesOnStartup: false,
      visibleApps: Object.fromEntries(apps.map(([id]) => [id, true])),
    };
  if (
    command === "get_providers" ||
    command === "get_proxy_takeover_status" ||
    command === "check_all_env_conflicts"
  )
    return {};
  if (command === "get_current_provider") return "";
  if (command === "get_proxy_status")
    return { running: false, currentProviders: [], totalRequests: 0 };
  if (command === "get_app_mode")
    return { mode: "direct", direct: null, route: null };
  if (command === "get_usage_summary")
    return { totalCost: "0", totalTokens: 0, totalRequests: 0 };
  if (command === "get_models_dev_sync_config")
    return { config: { autoSyncEnabled: false, lastSyncAt: null } };
  if (command === "auth_get_status") return { connected: false, accounts: [] };
  if (
    command === "list_sessions" ||
    command === "take_startup_attach_failures" ||
    command === "get_managed_auth_statuses"
  )
    return [];
  if (command === "plugin:app|version") return "0.1.0-fixture";
  if (command === "get_init_error" || command === "take_tray_navigation") return null;
  if (command.startsWith("plugin:event|")) return 1;
  if (command.startsWith("plugin:log|")) return null;
  if (command.startsWith("plugin:window|"))
    return command.endsWith("scale_factor") ? 1 : false;
  if (
    command.startsWith("get_") ||
    command.startsWith("list_") ||
    command.startsWith("check_")
  )
    return [];
  throw new Error(`Fixture does not implement ${command}`);
});
await import("./main");
