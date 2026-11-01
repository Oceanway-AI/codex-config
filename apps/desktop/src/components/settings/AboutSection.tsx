import { useCallback, useEffect, useState } from "react";
import { ExternalLink, Github, Globe, Info, Loader2 } from "lucide-react";
import { getVersion } from "@tauri-apps/api/app";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { settingsApi } from "@/lib/api";
import { toast } from "@/lib/toast";
import oceanwayLogo from "@/assets/oceanway/oceanway-ai-app-blue.svg";

interface AboutSectionProps {
  isPortable: boolean;
}

const OCEANWAY_WEBSITE = "https://ocean-way.top";
const OCEANWAY_REPOSITORY = "https://github.com/Oceanway-AI/codex-config";
const UPSTREAM_REPOSITORY = "https://github.com/farion1231/cc-switch";
const UPSTREAM_LICENSE =
  "https://github.com/farion1231/cc-switch/blob/c0189078aacdd80fd2462f59b345f55f62b41cf2/LICENSE";

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
          基于 CC Switch 改造，沿用原有界面、应用管理和会话功能，简化 Oceanway
          接入与模型选择。
        </p>
        <p className="text-caption text-fg-3">
          当前为开发版本，功能仍在验证中。正式安装包和自动更新渠道尚未开放。
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

      <div className="space-y-3 px-5 py-4">
        <div className="space-y-1">
          <h3 className="text-body font-medium text-fg-1">开源来源与致谢</h3>
          <p className="text-body text-fg-2">
            感谢 CC Switch 作者及贡献者。本应用以 CC Switch v4.0.24 为基础，遵循
            MIT 许可，保留原项目版权与许可声明。
          </p>
          <p className="text-caption text-fg-3">
            CC Switch — Copyright (c) 2025 Jason Young
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Button
            type="button"
            variant="neutral"
            size="compact"
            onClick={() => void openLink(UPSTREAM_REPOSITORY)}
          >
            <Github className="h-3.5 w-3.5" />
            CC Switch 源项目
          </Button>
          <Button
            type="button"
            variant="neutral"
            size="compact"
            onClick={() => void openLink(UPSTREAM_LICENSE)}
          >
            <ExternalLink className="h-3.5 w-3.5" />
            MIT 许可与版权
          </Button>
        </div>
      </div>
    </div>
  );
}
