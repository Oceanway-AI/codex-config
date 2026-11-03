import { useEffect, useState } from "react";
import {
  Copy,
  Download,
  Loader2,
  Monitor,
  RefreshCw,
  Terminal,
} from "lucide-react";
import { AppPageHeader } from "@/components/shell/AppPageHeader";
import { AppGlyph, APP_DISPLAY_NAME } from "@/components/shell/AppGlyph";
import { Button } from "@/components/ui/button";
import { OCEANWAY_APP_IDS } from "@/config/appConfig";
import { oceanwayApi, type DetectedApp } from "@/lib/api/oceanway";
import {
  desktopDownloadUrl,
  type DownloadApp,
} from "@/components/apps/officialInstallLinks";

import {
  AgentInstallProgress,
  useAgentInstaller,
} from "./AgentInstallProgress";

export function OceanwayAppsPage() {
  const installer = useAgentInstaller();
  const [apps, setApps] = useState<DetectedApp[]>([]);
  const [detecting, setDetecting] = useState(false);
  const [pending, setPending] = useState<string>();
  const [message, setMessage] = useState("");
  const [error, setError] = useState("");
  const failure = (e: unknown) =>
    setError(e instanceof Error ? e.message : String(e));
  async function detect() {
    setDetecting(true);
    setError("");
    try {
      if (
        installer.job &&
        ["ready", "awaiting_install"].includes(installer.job.stage)
      )
        await installer.check();
      setApps(await oceanwayApi.detectApps());
    } catch (e) {
      failure(e);
    } finally {
      setDetecting(false);
    }
  }
  useEffect(() => {
    void detect();
  }, []);
  useEffect(() => {
    if (installer.job?.stage === "installed") void detect();
  }, [installer.job?.stage]);
  async function download(app: DownloadApp) {
    setPending(app);
    setError("");
    setMessage("");
    try {
      await installer.start(app);
    } catch (e) {
      failure(e);
    } finally {
      setPending(undefined);
    }
  }
  return (
    <>
      <AppPageHeader
        icon={<Download className="h-5 w-5" />}
        title="应用安装"
        actions={
          <Button
            variant="quiet"
            disabled={detecting}
            onClick={() => void detect()}
          >
            <RefreshCw
              className={detecting ? "h-4 w-4 animate-spin" : "h-4 w-4"}
            />
            重新检测
          </Button>
        }
      />
      <div
        id="main-content"
        className="min-h-0 flex-1 overflow-y-auto px-4 py-6 sm:px-6"
      >
        <div className="mx-auto max-w-[820px] space-y-4">
          <div>
            <h2 className="text-title font-semibold">
              先安装应用，再连接 Oceanway
            </h2>
            <p className="mt-2 text-body text-fg-2">
              选择应用下载安装包，下载完成后点击“打开安装包”。
              按提示装好，再回来检测并连接 Oceanway。
            </p>
          </div>
          <AgentInstallProgress />
          {message && (
            <p
              role="status"
              className="rounded-panel bg-success-soft p-4 text-body text-success-text"
            >
              {message}
            </p>
          )}
          {error && (
            <p
              role="alert"
              className="rounded-panel border border-destructive p-4 text-body"
            >
              {error}
            </p>
          )}
          <div className="space-y-3">
            {OCEANWAY_APP_IDS.map((id) => {
              const app = apps.find((item) => item.appId === id);
              const installed =
                id === "claude" ? app?.installed : app?.desktopInstalled;
              const url = desktopDownloadUrl(id as DownloadApp);
              return (
                <section
                  key={id}
                  aria-label={APP_DISPLAY_NAME[id]}
                  className="rounded-panel border border-border bg-surface p-5"
                >
                  <div className="flex flex-wrap items-center gap-4">
                    <AppGlyph app={id} size={32} />
                    <div className="min-w-0 flex-1">
                      <h3 className="font-semibold">{APP_DISPLAY_NAME[id]}</h3>
                      <p className="mt-1 flex items-center gap-1.5 text-caption text-fg-3">
                        {id === "claude" ? (
                          <Terminal className="h-3.5 w-3.5" />
                        ) : (
                          <Monitor className="h-3.5 w-3.5" />
                        )}
                        {id === "claude" ? "终端版" : "桌面版"} ·{" "}
                        {detecting
                          ? "正在检测…"
                          : installed
                            ? "已安装"
                            : app
                              ? "尚未检测到"
                              : "待检测"}
                      </p>
                    </div>
                    <Button
                      disabled={
                        !!pending ||
                        installer.busy ||
                        !!installed ||
                        app?.installAvailable === false
                      }
                      onClick={() => void download(id as DownloadApp)}
                    >
                      {pending === id ? (
                        <Loader2 className="h-4 w-4 animate-spin" />
                      ) : (
                        <Download className="h-4 w-4" />
                      )}
                      {pending === id
                        ? "正在准备…"
                        : installed
                          ? "已安装"
                          : app?.installAvailable === false
                            ? "当前系统暂无安装包"
                            : "下载安装"}
                    </Button>
                  </div>
                  {app?.installAvailable === false && (
                    <p className="mt-3 text-caption text-fg-3">{app.reason}</p>
                  )}
                  {id === "codex" && (
                    <p className="mt-3 text-caption text-fg-3">
                      官方新版安装后显示为 ChatGPT，包含 Codex。
                    </p>
                  )}
                  <div className="mt-3 flex flex-wrap items-center gap-3 text-caption text-fg-3">
                    <span>Windows / Mac · 完整安装包</span>
                    <button
                      type="button"
                      className="inline-flex items-center gap-1 text-action hover:underline"
                      onClick={() => {
                        void navigator.clipboard
                          .writeText(url)
                          .then(() =>
                            setMessage(
                              `${APP_DISPLAY_NAME[id]} 下载地址已复制。`,
                            ),
                          )
                          .catch(failure);
                      }}
                    >
                      <Copy className="h-3 w-3" />
                      复制下载地址
                    </button>
                  </div>
                </section>
              );
            })}
          </div>
          <p className="text-caption text-fg-3">
            安装时如出现系统权限提示，请选择允许。装好后点击“检测安装结果”，
            再到对应应用页填写 API Key。Claude Code 下载完成后点击“安装 Claude
            Code”即可。
          </p>
        </div>
      </div>
    </>
  );
}
