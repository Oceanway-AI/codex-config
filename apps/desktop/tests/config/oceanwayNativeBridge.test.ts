import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

const source = readFileSync(resolve("src-tauri/src/lib.rs"), "utf8");
const handler = source
  .split(".invoke_handler(tauri::generate_handler![")[1]
  .split("])")[0];
const commands = new Set(
  handler
    .replace(/\/\/[^\n]*/g, "")
    .split(",")
    .map((entry) => entry.trim().split("::").at(-1)),
);

// Renderer tests mock IPC. This separate contract catches a button whose
// native implementation exists but was omitted from the packaged dispatcher.
describe("Oceanway visible features have a native command dispatcher", () => {
  it.each([
    "oceanway",
    "sessions",
    "mcp",
    "skills",
    "prompts",
    "usage",
    "workspace",
    "hermes",
  ])("registers every literal command used by %s", (module) => {
    const api = readFileSync(resolve(`src/lib/api/${module}.ts`), "utf8");
    const invoked = [
      ...api.matchAll(/invoke(?:<[^;]*?>)?\s*\(\s*["'](\w+)["']/g),
    ].map((match) => match[1]);
    expect(invoked.length).toBeGreaterThan(0);
    expect(invoked.filter((name) => !commands.has(name))).toEqual([]);
  });

  it("keeps settings and OpenClaw configuration saves callable", () => {
    for (const command of [
      "save_settings",
      "update_tray_menu",
      "get_openclaw_env",
      "set_openclaw_env",
      "get_openclaw_agents_defaults",
      "set_openclaw_agents_defaults",
      "get_openclaw_tools",
      "set_openclaw_tools",
    ]) {
      expect(commands, command).toContain(command);
    }
  });

  it("does not bundle the renderer fixture with the application", () => {
    const config = readFileSync(resolve("vite.config.ts"), "utf8");
    const main = readFileSync(resolve("src/main.tsx"), "utf8");
    expect(config).not.toContain("fixture.html");
    expect(main).not.toMatch(/(?:fixture|api\/mocks)/);
  });
});
