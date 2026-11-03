import { useSyncExternalStore } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button } from "@/components/ui/button";
import { APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";

export interface InstallStatus {
  id: string;
  appId: string;
  stage: string;
  downloaded: number;
  total: number;
  message: string;
  actionError?: string | null;
}
let current: InstallStatus | undefined;
let generation = 0;
const listeners = new Set<() => void>();
const notify = (job: InstallStatus) => {
  current = job;
  listeners.forEach((listener) => listener());
};
const active = (stage: string) =>
  [
    "preparing",
    "downloading",
    "verifying",
    "opening",
    "installing",
    "checking",
  ].includes(stage);
const subscribe = (listener: () => void) => {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
};

export function useAgentInstaller() {
  const job = useSyncExternalStore(subscribe, () => current);
  return {
    job,
    busy: !!job && active(job.stage),
    start: startInstall,
    check: () =>
      action("oceanway_check_install", "checking", "正在检测安装结果…"),
  };
}
async function startInstall(appId: string) {
  if (current && active(current.stage)) return;
  ++generation;
  notify({
    id: "starting",
    appId,
    stage: "preparing",
    downloaded: 0,
    total: 0,
    message: "正在准备下载…",
  });
  try {
    const job = await invoke<InstallStatus>("oceanway_start_install", {
      appId,
    });
    notify(job);
    void poll(job.id, generation);
  } catch (e) {
    notify({ ...current!, stage: "failed", message: String(e) });
    throw e;
  }
}
async function action(command: string, stage: string, message: string) {
  if (!current || active(current.stage)) return;
  const previous = current;
  ++generation;
  notify({ ...previous, stage, message, actionError: null });
  try {
    const job = await invoke<InstallStatus>(command, { id: previous.id });
    notify(job);
    void poll(job.id, generation);
  } catch (e) {
    notify({ ...previous, actionError: String(e) });
  }
}
async function poll(id: string, token: number) {
  let failures = 0;
  while (token === generation && current?.id === id && active(current.stage)) {
    await new Promise((resolve) => setTimeout(resolve, 1000));
    if (token !== generation || current?.id !== id) return;
    try {
      const job = await invoke<InstallStatus>("oceanway_get_install", { id });
      if (token !== generation) return;
      notify(job);
      failures = 0;
    } catch (e) {
      if (token !== generation) return;
      if (++failures >= 3) {
        notify({
          ...current,
          stage: "failed",
          message: `无法读取安装状态，请重新检测应用。${String(e)}`,
        });
      }
    }
  }
}
export function AgentInstallProgress({ appId }: { appId?: string }) {
  const { job } = useAgentInstaller();
  if (!job || (appId && appId !== job.appId)) return null;
  const percent = job.total
    ? Math.min(100, Math.round((job.downloaded * 100) / job.total))
    : 0;
  const cancelable = ["preparing", "downloading", "verifying"].includes(
    job.stage,
  );
  const ready = ["ready", "awaiting_install"].includes(job.stage);
  return (
    <div className="rounded-panel border border-border bg-subtle p-4">
      <p className="mb-1 font-semibold">
        {APP_DISPLAY_NAME[job.appId as keyof typeof APP_DISPLAY_NAME] ??
          job.appId}
      </p>
      <p
        role={job.stage === "failed" ? "alert" : "status"}
        className="text-body"
      >
        {job.message}
      </p>
      {job.actionError && (
        <p role="alert" className="mt-2 text-body text-destructive">
          {job.actionError}
        </p>
      )}
      {job.stage === "downloading" && (
        <>
          <div
            role="progressbar"
            aria-label="安装包下载进度"
            aria-valuenow={percent}
            aria-valuemin={0}
            aria-valuemax={100}
            className="mt-3 h-2 overflow-hidden rounded-full bg-border"
          >
            <div
              className="h-full bg-action transition-all"
              style={{ width: `${percent}%` }}
            />
          </div>
          <p className="mt-2 text-caption text-fg-3">
            {percent}% · {(job.downloaded / 1048576).toFixed(1)} /{" "}
            {(job.total / 1048576).toFixed(1)} MB
          </p>
        </>
      )}
      {ready && (
        <div className="mt-3 flex flex-wrap gap-2">
          <Button
            onClick={() =>
              void action(
                "oceanway_open_installer",
                "opening",
                "正在打开安装包…",
              )
            }
          >
            {job.appId === "claude"
              ? "安装 Claude Code"
              : job.stage === "awaiting_install"
                ? "再次打开安装包"
                : "打开安装包"}
          </Button>
          <Button
            variant="quiet"
            onClick={() =>
              void action(
                "oceanway_check_install",
                "checking",
                "正在检测安装结果…",
              )
            }
          >
            检测安装结果
          </Button>
        </div>
      )}
      {job.stage === "failed" && (
        <Button
          className="mt-3"
          onClick={() => {
            void startInstall(job.appId).catch(() => undefined);
          }}
        >
          重新下载
        </Button>
      )}
      {cancelable && (
        <Button
          variant="quiet"
          size="compact"
          className="mt-2"
          onClick={() => {
            void invoke("oceanway_cancel_install", { id: job.id }).catch((e) =>
              notify({ ...job, actionError: String(e) }),
            );
          }}
        >
          取消下载
        </Button>
      )}
    </div>
  );
}
