import { describe, expect, it } from "vitest";
import {
  DESKTOP_DOWNLOAD_BASE,
  desktopDownloadUrl,
} from "@/components/apps/officialInstallLinks";

describe("desktop offline download selection", () => {
  it.each([
    ["codex", "windows", "x86_64", "codex-windows-x64-offline.exe"],
    [
      "claude-desktop",
      "windows",
      "x86_64",
      "claude-desktop-windows-x64-offline.exe",
    ],
    ["codex", "darwin", "aarch64", "codex-macos-arm64.dmg"],
    ["codex", "darwin", "x86_64", "codex-macos-x64.dmg"],
    [
      "claude-desktop",
      "darwin",
      "aarch64",
      "claude-desktop-macos-universal.pkg",
    ],
    [
      "claude-desktop",
      "darwin",
      "x86_64",
      "claude-desktop-macos-universal.pkg",
    ],
  ] as const)("selects %s for %s %s", (app, platform, arch, filename) => {
    expect(desktopDownloadUrl(app, platform, arch)).toBe(
      `${DESKTOP_DOWNLOAD_BASE}${filename}`,
    );
  });

  it.each([
    ["windows", "aarch64"],
    ["darwin", ""],
    ["linux", "x86_64"],
    ["", ""],
  ])("opens the package chooser for %s %s", (platform, arch) => {
    expect(desktopDownloadUrl("codex", platform, arch)).toBe(
      `${DESKTOP_DOWNLOAD_BASE}?app=codex`,
    );
  });
});
