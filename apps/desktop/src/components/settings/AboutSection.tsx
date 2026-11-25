import { useCallback, useEffect, useState } from "react";
import { Github, Globe, Info, Loader2 } from "lucide-react";
import { getVersion } from "@tauri-apps/api/app";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { settingsApi } from "@/lib/api";
import { toast } from "@/lib/toast";
import oceanwayLogo from "@/assets/oceanway/oceanway-ai-app-blue.svg";
import { OCEANWAY_WEBSITE } from "@/config/oceanway";

interface AboutSectionProps {
  isPortable: boolean;
}

const OCEANWAY_REPOSITORY = "https://github.com/Oceanway-AI/codex-config";

// Read the local build version without contacting an update service.
let appVersionCache: string | null = null;

/** Oceanway information within the existing CC Switch settings layout. */
export function AboutSection({ isPortable }: AboutSectionProps) {
  const { t } = useTranslation();
  const [version, setVersion] = useState<string | null>(() => appVersionCache);
  const [isLoadingVersion, setIsLoadingVersion] = useState(
    () => appVersionCache === null,
  );

  useEffect(() => {
    let active = true;
    const loadAppVersion = async () => {
      try {
        const appVersion = await getVersion();
        appVersionCache = appVersion;
        if (active) setVersion(appVersion);
      } catch (error) {
        console.error("[AboutSection] Failed to load app version", error);
        if (active) setVersion(null);
      } finally {
        if (active) setIsLoadingVersion(false);
      }
    };

    void loadAppVersion();
    return () => {
      active = false;
    };
  }, []);

  const openLink = useCallback(async (url: string) => {
    try {
      await settingsApi.openExternal(url);
    } catch (error) {
      console.error("[AboutSection] Failed to open link", error);
      toast.error("链接未能打开，请稍后重试。");
    }
  }, []);

  return (
    <div className="divide-y divide-border overflow-hidden rounded-panel border border-border bg-surface">
      <div className="flex flex-wrap items-center gap-4 px-5 py-4">
        <img
          src={oceanwayLogo}
          alt=""
          draggable={false}
          className="h-10 w-10 shrink-0 object-contain"
        />
        <div className="min-w-0 flex-1">
          <div className="text-title text-fg-1">Oceanway Config</div>
          <div className="flex flex-wrap items-center gap-2 text-caption text-fg-2">
            {isLoadingVersion ? (
              <Loader2
                className="h-3 w-3 animate-spin"
                aria-label="正在读取版本"
              />
            ) : (
              <span className="tabular-nums">
                {version ? `当前构建 v${version}` : "未能读取版本"}
              </span>
            )}
            <Badge variant="secondary" className="px-1.5 py-0 text-badge">
              开发中
            </Badge>
            {isPortable && (
              <Badge
                variant="secondary"
                className="gap-1 px-1.5 py-0 text-badge"
              >
                <Info className="h-3 w-3" />
                {t("settings.portableMode")}
              </Badge>
            )}
          </div>
        </div>
      </div>

      <div className="space-y-2 px-5 py-4 text-body">
        <p className="text-fg-1">
          Oceanway 客户的本地配置工具，用一个 API Key 配置 Codex、Claude Code 等
          AI 应用。
        </p>
        <p className="text-fg-2">
          通过 Oceanway Console 网关接入模型。登录官网后，可进入 Console 管理
          API Key、查看调用情况。
        </p>
      </div>

      <div className="flex flex-wrap items-center gap-2 px-5 py-4">
        <Button
          type="button"
          variant="neutral"
          size="compact"
          onClick={() => void openLink(OCEANWAY_WEBSITE)}
        >
          <Globe className="h-3.5 w-3.5" />
          Oceanway 官网
        </Button>
        <Button
          type="button"
          variant="neutral"
          size="compact"
          onClick={() => void openLink(OCEANWAY_REPOSITORY)}
        >
          <Github className="h-3.5 w-3.5" />
          项目 GitHub
        </Button>
      </div>
    </div>
  );
}
