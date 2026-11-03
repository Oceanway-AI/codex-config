// Explicit local renderer fixture. Not included in the production build inputs.
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import liveModelCatalog from "./test-fixtures/oceanway-model-catalog.json";
if (!import.meta.env.DEV) throw new Error("Fixture is for development only");
// Explicit UI samples only; no local application/version detection is performed.
// ?empty=1 previews a new computer; ?desktop=only previews Codex GUI without CLI.
const query = new URLSearchParams(window.location.search);
const empty = query.get("empty") === "1";
const desktopOnly = query.get("desktop") === "only";
const codexDesktopInstalled =
  !empty && ["1", "only"].includes(query.get("desktop") ?? "");
const installedCli = new Set(
  empty
    ? []
    : desktopOnly
      ? ["claude", "gemini"]
      : ["codex", "claude", "gemini"],
);
const fixtureVersion = "0.0.0-fixture";
const apps = [
  ["codex", "Codex"],
  ["claude-desktop", "Claude Desktop"],
  ["claude", "Claude Code"],
  ["dsh", "DeepSeek Harness"],
  ["hermes", "Hermes"],
  ["kimi", "Kimi Code Desktop"],
];
let configured = false;
let configuredApps: string[] = [];
let savedImageModel: string | null = null;
localStorage.setItem("oceanway-last-view", "connection");
localStorage.setItem("i18nextLng", "zh");
mockWindows("main");
mockIPC(async (command, args: any) => {
  if (command === "oceanway_get_connection")
    return {
      configured,
      keyPreview: configured ? "fixture…key" : null,
      baseUrl: "https://console.oceanway.tech/v1",
      configuredApps,
      defaultImageModel: savedImageModel,
    };
  if (command === "oceanway_detect_apps")
    return apps.map(([appId, name]) => ({
      appId,
      name,
      installed:
        installedCli.has(appId) || (appId === "codex" && codexDesktopInstalled),
      desktopInstalled: appId === "codex" && codexDesktopInstalled,
      configExists:
        !empty && !desktopOnly && ["codex", "claude"].includes(appId),
      configurable: true,
    }));
  if (command === "get_tool_versions")
    return (args.tools ?? []).map((name: string) => ({
      name,
      version: installedCli.has(name) ? fixtureVersion : null,
      latest_version: fixtureVersion,
      error: null,
      installed_but_broken: false,
      desktop_installed: name === "codex" && codexDesktopInstalled,
      env_type: "windows",
      wsl_distro: null,
      update_blocked_by_node: null,
    }));
  if (
    command === "list_tool_installations" ||
    command === "probe_tool_installations"
  )
    return (args.tools ?? []).map((tool: string) => ({
      tool,
      installs: installedCli.has(tool)
        ? [
            {
              path: `fixture-only/${tool}`,
              version: fixtureVersion,
              runnable: true,
              error: null,
              source: "system",
              is_path_default: true,
            },
          ]
        : [],
      is_conflict: false,
      needs_confirmation: false,
      command: "",
      anchored: false,
      unmanaged: true,
    }));
  if (command === "get_claude_desktop_status")
    return { supported: true, installed: false, configured: false };
  if (
    command === "run_tool_lifecycle_action" ||
    command === "prepare_tool_uninstall"
  )
    throw new Error("界面测试不会安装、升级或卸载本机应用。");
  if (command === "open_external") {
    window.open(args.url, "_blank", "noopener,noreferrer");
    return true;
  }
  if (command === "oceanway_check_key") {
    if (!configured && args.apiKey !== "fixture-key")
      throw new Error("界面测试请填写 fixture-key，不使用真实 Key。");
    // Sanitized catalog from the same Console Key used for native acceptance.
    return {
      ...liveModelCatalog,
      models: liveModelCatalog.models.filter(
        (model) => query.get("images") !== "0" || model.modelType !== "image",
      ),
    };
  }
  if (command === "oceanway_apply_config") {
    configured = true;
    configuredApps = args.request.apps.map((a: any) => a.appId);
    savedImageModel = args.request.imageModel ?? savedImageModel;
    return {
      results: args.request.apps.map((a: any) => ({
        appId: a.appId,
        ok: true,
        message: "界面测试：配置结果示例",
        imageConfigured:
          !!args.request.imageModel &&
          a.appId !== "openclaw" &&
          query.get("error") !== "image",
        imageMessage: args.request.imageModel
          ? a.appId === "openclaw"
            ? "OpenClaw 的图片工具尚需适配"
            : query.get("error") === "image"
              ? "图片工具启动失败，请重新配置。"
              : "Oceanway 图片工具已配置，可在对话中直接生图。"
          : null,
      })),
      restartRequired: true,
      backupPath: "fixture/backups",
      imagesConfigured:
        !!args.request.imageModel &&
        query.get("error") !== "image" &&
        !args.request.apps.some((a: any) => a.appId === "openclaw"),
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
  if (command === "get_migration_result") return false;
  if (command === "get_init_error" || command === "take_tray_navigation")
    return null;
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
