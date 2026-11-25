import type { ToolName } from "./useToolManagement";
import { settingsApi } from "@/lib/api/settings";

export type DownloadApp =
  | "codex"
  | "claude-desktop"
  | "claude"
  | "dsh"
  | "hermes"
  | "kimi";

// The CLI installation guides are separate from desktop offline packages.
export const OFFICIAL_TOOL_INSTALL_URLS: Record<ToolName, string> = {
  claude: "https://code.claude.com/docs/en/setup",
  codex: "https://developers.openai.com/codex/cli",
  gemini: "https://geminicli.com/docs/get-started/installation/",
  grok: "https://x.ai/cli",
  opencode: "https://opencode.ai/docs/",
  openclaw: "https://docs.openclaw.ai/install",
  hermes: "https://github.com/NousResearch/hermes-agent#installation",
  pi: "https://github.com/earendil-works/pi#readme",
  mcode: "https://platform.minimax.io/docs/m-plan/minimax-code",
  kimi: "https://www.kimi.com/code/docs/en/",
  dsh: "https://github.com/deepseek-ai/deepseek-harness#readme",
  agy: "https://antigravity.google/cli",
  omp: "https://omp.sh/",
};

export const DESKTOP_DOWNLOAD_BASE =
  "https://api.ocean-way.top/downloads/agents/";

export function desktopDownloadUrl(
  app: DownloadApp,
  platform = import.meta.env.TAURI_ENV_PLATFORM,
  arch = import.meta.env.TAURI_ENV_ARCH,
): string {
  const x64 = arch === "x86_64" || arch === "x64";
  const arm64 = arch === "aarch64" || arch === "arm64";
  if (platform === "windows" && x64) {
    if (app === "hermes")
      return `${DESKTOP_DOWNLOAD_BASE}hermes-windows-x64.msix`;
    if (app === "claude") return `${DESKTOP_DOWNLOAD_BASE}?app=claude`;
    const suffix = ["codex", "claude-desktop", "claude", "hermes"].includes(app)
      ? "-offline.exe"
      : ".exe";
    return `${DESKTOP_DOWNLOAD_BASE}${app}-windows-x64${suffix}`;
  }
  if (platform === "darwin" || platform === "macos") {
    if (app === "claude-desktop" && (x64 || arm64)) {
      return `${DESKTOP_DOWNLOAD_BASE}claude-desktop-macos-universal.pkg`;
    }
    if (app === "claude" || (app === "hermes" && x64))
      return `${DESKTOP_DOWNLOAD_BASE}?app=${app}`;
    if (x64 || arm64) {
      return `${DESKTOP_DOWNLOAD_BASE}${app}-macos-${arm64 ? "arm64" : "x64"}.dmg`;
    }
  }
  // A browser preview or an unsupported build must not guess a CPU architecture.
  return `${DESKTOP_DOWNLOAD_BASE}?app=${app}`;
}

/** Opening a browser is bounded independently of the browser's download. */
export async function openAgentDownload(app: DownloadApp): Promise<void> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    await Promise.race([
      settingsApi.openExternal(desktopDownloadUrl(app)),
      new Promise<never>((_, reject) => {
        timer = setTimeout(
          () => reject(new Error("未能打开浏览器，请复制下载地址后重试。")),
          10000,
        );
      }),
    ]);
  } finally {
    clearTimeout(timer);
  }
}
