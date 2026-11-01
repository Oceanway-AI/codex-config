import type { ToolName } from "./useToolManagement";

// Open the publisher's installation page, which chooses the current package for
// the user's OS/architecture. Keep versioned binaries and installer scripts out
// of these browser links; automatic installation remains in the lifecycle API.
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

export const CLAUDE_DESKTOP_DOWNLOAD_URL = "https://claude.com/download";
export const CODEX_DESKTOP_DOWNLOAD_URL =
  "https://developers.openai.com/codex/app";
