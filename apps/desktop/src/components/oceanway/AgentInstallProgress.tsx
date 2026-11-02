import { useSyncExternalStore } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button } from "@/components/ui/button";

export interface InstallStatus {
  id: string;
  appId: string;
  stage: string;
  downloaded: number;
  total: number;
  message: string;
}
let current: InstallStatus | undefined;
const listeners = new Set<() => void>();
const notify = (job: InstallStatus) => {
  current = job;
  listeners.forEach((listener) => listener());
};
const finished = (stage: string) =>
  ["installed", "failed", "cancelled"].includes(stage);
const subscribe = (listener: () => void) => {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
};

export function useAgentInstaller() {
  const job = useSyncExternalStore(subscribe, () => current);
  return { job, busy: !!job && !finished(job.stage), start: startInstall };
}
async function startInstall(appId: string) {
  if (current && !finished(current.stage)) return;
  const job = await invoke<InstallStatus>("oceanway_start_install", { appId });
  notify(job);
  void poll(job.id);
}
async function poll(id: string) {
  let failures = 0;
  while (current?.id === id && !finished(current.stage)) {
    await new Promise((resolve) => setTimeout(resolve, 1000));
    try {
      notify(await invoke<InstallStatus>("oceanway_get_install", { id }));
      failures = 0;
    } catch (e) {
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
  return (
    <div className="rounded-panel border border-border bg-subtle p-4">
      <p
        role={job.stage === "failed" ? "alert" : "status"}
        className="text-body"
      >
        {job.message}
      </p>
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
      {cancelable && (
        <Button
          variant="quiet"
          size="compact"
          className="mt-2"
          onClick={() => {
            void invoke("oceanway_cancel_install", { id: job.id }).catch((e) =>
              notify({ ...job, message: String(e) }),
            );
          }}
        >
          取消下载
        </Button>
      )}
    </div>
  );
}
