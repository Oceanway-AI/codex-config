import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { DatabaseUpgrade } from "@/components/DatabaseUpgrade";

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), exit: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/plugin-process", () => ({ exit: mocks.exit }));

const payload = {
  path: "C:/isolated/Oceanway/oceanway.db",
  error: "database schema is newer than this build",
  db_version: 20,
  supported_version: 19,
};

describe("Oceanway database recovery", () => {
  beforeEach(() => {
    mocks.invoke.mockReset().mockResolvedValue(undefined);
    mocks.exit.mockReset().mockResolvedValue(undefined);
  });

  it("opens Oceanway releases without calling unavailable updater commands", async () => {
    render(<DatabaseUpgrade payload={payload} />);
    expect(mocks.invoke).not.toHaveBeenCalled();
    expect(screen.queryByRole("button", { name: "升级应用" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "打开发布页" }));
    await waitFor(() =>
      expect(mocks.invoke).toHaveBeenCalledWith("open_external", {
        url: "https://github.com/Oceanway-AI/codex-config/releases",
      }),
    );
    expect(mocks.invoke).toHaveBeenCalledTimes(1);
  });

  it("reports an opener failure and lets the user retry", async () => {
    mocks.invoke.mockRejectedValueOnce(new Error("No browser available"));
    render(<DatabaseUpgrade payload={payload} />);
    fireEvent.click(screen.getByRole("button", { name: "打开发布页" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "No browser available",
    );
    fireEvent.click(screen.getByRole("button", { name: "打开发布页" }));
    await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
    expect(mocks.invoke).toHaveBeenCalledTimes(2);
  });

  it("keeps the configuration folder and exit actions available", async () => {
    render(<DatabaseUpgrade payload={payload} />);
    expect(screen.getByText(payload.error)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "打开配置目录" }));
    await waitFor(() =>
      expect(mocks.invoke).toHaveBeenCalledWith("open_app_config_folder"),
    );
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "退出" })).toBeEnabled(),
    );
    fireEvent.click(screen.getByRole("button", { name: "退出" }));
    await waitFor(() => expect(mocks.exit).toHaveBeenCalledWith(0));
  });
});
