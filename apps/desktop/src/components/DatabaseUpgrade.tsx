import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { exit } from "@tauri-apps/plugin-process";
import { Database, ExternalLink, FolderOpen, Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";

const RELEASES_URL = "https://github.com/Oceanway-AI/codex-config/releases";

interface DatabaseUpgradeProps {
  payload: {
    path?: string;
    error?: string;
    kind?: string;
    db_version?: number;
    supported_version?: number;
  };
}

type RecoveryAction = "releases" | "folder" | "exit";

/** Open Oceanway's published builds when this build cannot read the database. */
export function DatabaseUpgrade({ payload }: DatabaseUpgradeProps) {
  const { t } = useTranslation();
  const [pendingAction, setPendingAction] = useState<RecoveryAction | null>(
    null,
  );
  const [errorMsg, setErrorMsg] = useState<string | null>(null);

  const runAction = useCallback(async (action: RecoveryAction) => {
    setPendingAction(action);
    setErrorMsg(null);
    try {
      if (action === "releases") {
        await invoke("open_external", { url: RELEASES_URL });
      } else if (action === "folder") {
        await invoke("open_app_config_folder");
      } else {
        await exit(0);
      }
    } catch (error) {
      setErrorMsg(error instanceof Error ? error.message : String(error));
    } finally {
      setPendingAction(null);
    }
  }, []);

  return (
    <div className="flex min-h-screen items-center justify-center bg-surface p-6 text-fg-1">
      <div className="w-full max-w-lg space-y-5 rounded-2xl border border-border/60 bg-surface p-7 shadow-xl">
        <div className="flex items-start gap-4">
          <div className="flex h-12 w-12 shrink-0 items-center justify-center rounded-xl bg-warning-soft text-warning-text">
            <Database className="h-6 w-6" />
          </div>
          <div className="space-y-1">
            <h1 className="text-lg font-semibold">
              {t("dbUpgrade.title", "数据库版本过新")}
            </h1>
            <p className="text-sm text-fg-2">
              {t(
                "dbUpgrade.manualUpdateDescription",
                "当前版本无法读取这个数据库。请到 Oceanway 发布页查看可用版本，下载安装支持该数据库的版本后再打开。",
              )}
            </p>
            {payload.db_version != null &&
              payload.supported_version != null && (
                <p className="pt-0.5 text-xs text-fg-2 tabular-nums">
                  {t("dbUpgrade.versionInfo", {
                    db: payload.db_version,
                    supported: payload.supported_version,
                    defaultValue:
                      "数据库版本 v{{db}} · 应用支持 v{{supported}}",
                  })}
                </p>
              )}
          </div>
        </div>

        <div className="space-y-1 rounded-lg border border-border/50 bg-subtle p-3 text-xs text-fg-2">
          {payload.error && (
            <p className="break-words font-mono">{payload.error}</p>
          )}
          {payload.path && (
            <p className="break-all">
              {t("dbUpgrade.dbPath", "数据库文件")}：{payload.path}
            </p>
          )}
        </div>

        {errorMsg && (
          <p
            role="alert"
            className="rounded-lg bg-danger-soft p-3 text-sm text-danger-text"
          >
            {errorMsg}
          </p>
        )}

        <div className="flex flex-wrap items-center gap-2">
          <Button
            className="gap-2"
            disabled={pendingAction !== null}
            onClick={() => void runAction("releases")}
          >
            {pendingAction === "releases" ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : (
              <ExternalLink className="h-4 w-4" />
            )}
            {t("dbUpgrade.openReleases", "打开发布页")}
          </Button>
          <Button
            variant="outline"
            className="gap-2"
            disabled={pendingAction !== null}
            onClick={() => void runAction("folder")}
          >
            <FolderOpen className="h-4 w-4" />
            {t("dbUpgrade.openConfigDir", "打开配置目录")}
          </Button>
          <Button
            variant="ghost"
            className="ml-auto text-fg-2"
            disabled={pendingAction !== null}
            onClick={() => void runAction("exit")}
          >
            {t("dbUpgrade.quit", "退出")}
          </Button>
        </div>
      </div>
    </div>
  );
}

export default DatabaseUpgrade;
