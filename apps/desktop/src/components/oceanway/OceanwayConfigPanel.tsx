import { useEffect, useRef, useState } from "react";
import {
  ArrowRight,
  Check,
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  CircleAlert,
  Download,
  Eye,
  EyeOff,
  History,
  Image,
  KeyRound,
  Loader2,
  RefreshCw,
} from "lucide-react";
import { useQueryClient } from "@tanstack/react-query";
import { AppPageHeader } from "@/components/shell/AppPageHeader";
import { APP_DISPLAY_NAME, AppGlyph } from "@/components/shell/AppGlyph";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Checkbox } from "@/components/ui/checkbox";
import { ConfirmDialog } from "@/components/ConfirmDialog";
import { settingsApi } from "@/lib/api/settings";
import { cn } from "@/lib/utils";
import {
  CODEX_DESKTOP_DOWNLOAD_URL,
  CLAUDE_DESKTOP_DOWNLOAD_URL,
  OFFICIAL_TOOL_INSTALL_URLS,
} from "@/components/apps/officialInstallLinks";
import {
  oceanwayApi,
  type ConnectionSummary,
  type DetectedApp,
  type ModelCatalog,
  type ApplyResult,
} from "@/lib/api/oceanway";
import type { AppId } from "@/lib/api";
import {
  conversationModels,
  familyModels,
  imageModels,
  recommendedModel,
  recommendedImageModel,
} from "./models";

const errorText = (e: unknown) =>
  e instanceof Error
    ? e.message
    : typeof e === "string"
      ? e
      : "操作失败，请重试。";
const COMMON_APPS = ["codex", "claude"];
const INSTALL_URLS: Record<string, string> = {
  ...OFFICIAL_TOOL_INSTALL_URLS,
  codex: CODEX_DESKTOP_DOWNLOAD_URL,
  "claude-desktop": CLAUDE_DESKTOP_DOWNLOAD_URL,
  grokbuild: OFFICIAL_TOOL_INSTALL_URLS.grok,
};
const FAMILY_LABELS: Record<string, string> = {
  codex: "GPT",
  claude: "Claude",
  "claude-desktop": "Claude",
  gemini: "Gemini",
};
const available = (app: DetectedApp) =>
  app.configurable && (app.installed || app.configExists);

/** Guided setup using CC Switch's components, configuration and launch services. */
export function OceanwayConfigPanel({
  appId,
  onOpenApps,
}: {
  appId?: AppId;
  onOpenApps?: () => void;
}) {
  const queryClient = useQueryClient();
  const [connection, setConnection] = useState<ConnectionSummary>();
  const [apps, setApps] = useState<DetectedApp[]>([]);
  const [key, setKey] = useState("");
  const [showKey, setShowKey] = useState(false);
  const [editingKey, setEditingKey] = useState(false);
  const [showAllApps, setShowAllApps] = useState(false);
  const [editingModels, setEditingModels] = useState<Record<string, boolean>>(
    {},
  );
  const [otherModels, setOtherModels] = useState<Record<string, boolean>>({});
  const [editingImage, setEditingImage] = useState(false);
  const [catalog, setCatalog] = useState<ModelCatalog>();
  const [selected, setSelected] = useState<string[]>([]);
  const [models, setModels] = useState<Record<string, string>>({});
  const [imageModel, setImageModel] = useState("");
  const [busy, setBusy] = useState<"load" | "check" | "save" | "launch" | null>(
    "load",
  );
  const [error, setError] = useState("");
  const [result, setResult] = useState<ApplyResult>();
  const [launchMessage, setLaunchMessage] = useState("");
  const [restartApp, setRestartApp] = useState<string>();
  const generation = useRef(0);
  const initialized = useRef(false);
  const detectedIds = useRef<string[]>([]);
  const textModels = conversationModels(catalog?.models ?? []);
  const pictures = imageModels(catalog?.models ?? []);
  const displayed = appId
    ? apps.filter((a) => a.appId === appId)
    : apps.filter(
        (a) =>
          showAllApps ||
          COMMON_APPS.includes(a.appId) ||
          selected.includes(a.appId),
      );
  const extraApps = apps.filter((a) => !COMMON_APPS.includes(a.appId));
  const canSave =
    !!catalog &&
    selected.length > 0 &&
    selected.every((id) => !!models[id]) &&
    !busy;
  const step = result ? 3 : catalog ? 2 : 1;
  const appName = (id: string) => apps.find((a) => a.appId === id)?.name ?? id;
  const resultComplete =
    result?.results.length &&
    result.results.every((r) => r.ok && (!imageModel || r.imageConfigured));

  async function refresh() {
    setBusy("load");
    setError("");
    try {
      const [summary, detected] = await Promise.all([
        oceanwayApi.connection(),
        oceanwayApi.detectApps(),
      ]);
      const newlyAvailable = detected
        .filter(
          (a) =>
            available(a) &&
            (appId ? a.appId === appId : COMMON_APPS.includes(a.appId)) &&
            (!initialized.current || !detectedIds.current.includes(a.appId)),
        )
        .map((a) => a.appId);
      setConnection(summary);
      setApps(detected);
      setSelected((previous) => [
        ...new Set([
          ...previous.filter((id) =>
            detected.some((a) => a.appId === id && available(a)),
          ),
          ...newlyAvailable,
        ]),
      ]);
      initialized.current = true;
      detectedIds.current = detected.filter(available).map((a) => a.appId);
      if (catalog)
        setModels((previous) =>
          Object.fromEntries(
            detected.map((a) => [
              a.appId,
              previous[a.appId] ?? recommendedModel(a.appId, textModels),
            ]),
          ),
        );
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(null);
    }
  }
  useEffect(() => {
    void refresh();
    return () => {
      generation.current++;
    };
  }, [appId]);

  function changeKey(value: string) {
    generation.current++;
    setKey(value);
    setCatalog(undefined);
    setResult(undefined);
    setModels({});
    setOtherModels({});
    setEditingModels({});
    setImageModel("");
    setEditingImage(false);
    setError("");
    setLaunchMessage("");
  }
  async function check() {
    const current = ++generation.current;
    setBusy("check");
    setError("");
    setResult(undefined);
    setLaunchMessage("");
    try {
      const data = await oceanwayApi.checkKey(key.trim());
      if (current !== generation.current) return;
      setCatalog(data);
      const choices = conversationModels(data.models);
      setModels(
        Object.fromEntries(
          apps.map((a) => [a.appId, recommendedModel(a.appId, choices)]),
        ),
      );
      setOtherModels({});
      setEditingModels({});
      const images = imageModels(data.models);
      setImageModel(
        connection?.defaultImageModel &&
          images.includes(connection.defaultImageModel)
          ? connection.defaultImageModel
          : recommendedImageModel(images),
      );
      setEditingImage(false);
      setEditingKey(false);
      setShowKey(false);
    } catch (e) {
      if (current === generation.current) {
        setCatalog(undefined);
        setError(errorText(e));
      }
    } finally {
      if (current === generation.current) setBusy(null);
    }
  }
  async function save() {
    if (!canSave) return;
    setBusy("save");
    setError("");
    setLaunchMessage("");
    try {
      const response = await oceanwayApi.apply({
        apiKey: key.trim(),
        apps: selected.map((id) => ({ appId: id, model: models[id] })),
        ...(imageModel ? { imageModel, allowedImageModels: pictures } : {}),
      });
      setResult(response);
      await queryClient.invalidateQueries();
      if (response.results.some((r) => r.ok)) {
        // Never persist the Key in the browser, including the preview.
        setKey("");
        setConnection(await oceanwayApi.connection());
      }
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(null);
    }
  }
  async function launch(id: string, restart: boolean) {
    setBusy("launch");
    setError("");
    setLaunchMessage("");
    try {
      setLaunchMessage((await oceanwayApi.launch(id, restart)).message);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(null);
      setRestartApp(undefined);
    }
  }
  async function install(id: string) {
    try {
      await settingsApi.openExternal(INSTALL_URLS[id]);
    } catch (e) {
      setError(errorText(e));
    }
  }

  if (
    appId &&
    initialized.current &&
    !apps.some((app) => app.appId === appId)
  ) {
    const name = APP_DISPLAY_NAME[appId];
    return (
      <div
        id="main-content"
        className="min-h-0 flex-1 overflow-y-auto scroll-stable px-4 pb-8 pt-6 sm:px-6"
      >
        <section className="mx-auto max-w-[760px] rounded-panel border border-border bg-surface p-6">
          <AppGlyph app={appId} size={32} />
          <h2 className="mt-4 text-title font-semibold text-fg-1">
            {name} 尚未开放 Oceanway 接入
          </h2>
          <p className="mt-2 text-body leading-relaxed text-fg-2">
            你可以在“应用安装”中安装和更新 {name}。
            {appId === "hermes" && "上方“记忆”页可以查看和保存现有记忆。"}
          </p>
          {onOpenApps && (
            <Button className="mt-5" onClick={onOpenApps}>
              <Download className="h-4 w-4" />
              管理应用安装
            </Button>
          )}
        </section>
      </div>
    );
  }

  return (
    <>
      {!appId && (
        <AppPageHeader
          icon={<KeyRound className="h-5 w-5" />}
          title="连接 Oceanway"
          actions={
            <Button
              variant="quiet"
              size="regular"
              disabled={!!busy}
              onClick={() => void refresh()}
            >
              <RefreshCw
                className={cn("h-4 w-4", busy === "load" && "animate-spin")}
              />
              重新检测
            </Button>
          }
        />
      )}
      <div
        id="main-content"
        className="min-h-0 flex-1 overflow-y-auto scroll-stable px-4 pb-8 pt-6 sm:px-6"
      >
        <div className="mx-auto max-w-[760px] space-y-6">
          <ol
            aria-label="配置进度"
            className="flex items-center gap-2 text-caption sm:gap-4"
          >
            {["连接 Oceanway", "选择应用", "开始使用"].map((label, i) => (
              <li
                key={label}
                aria-current={step === i + 1 ? "step" : undefined}
                className={cn(
                  "flex min-w-0 flex-1 items-center gap-2",
                  step < i + 1 ? "text-fg-3" : "text-fg-1",
                )}
              >
                <span
                  className={cn(
                    "flex h-6 w-6 shrink-0 items-center justify-center rounded-full text-caption font-medium",
                    step > i + 1
                      ? "bg-success-soft text-success-text"
                      : step === i + 1
                        ? "bg-action text-action-fg"
                        : "bg-subtle text-fg-3",
                  )}
                >
                  {step > i + 1 ? <Check className="h-3.5 w-3.5" /> : i + 1}
                </span>
                <span className="truncate">{label}</span>
                {i < 2 && (
                  <span className="ms-1 hidden h-px flex-1 bg-border sm:block" />
                )}
              </li>
            ))}
          </ol>
          <div>
            <div className="flex flex-wrap items-center justify-between gap-2">
              <h2 className="text-xl font-semibold tracking-tight text-fg-1">
                {result
                  ? resultComplete
                    ? "准备好了，开始使用吧"
                    : "配置结果"
                  : appId
                    ? `让 ${appName(appId)} 连上 Oceanway`
                    : "连接你的 AI 应用"}
              </h2>
              {appId && (
                <Button
                  variant="quiet"
                  size="compact"
                  disabled={!!busy}
                  onClick={() => void refresh()}
                >
                  <RefreshCw
                    className={cn(
                      "h-3.5 w-3.5",
                      busy === "load" && "animate-spin",
                    )}
                  />
                  重新检测
                </Button>
              )}
            </div>
            <p className="mt-2 text-body leading-relaxed text-fg-2">
              {result
                ? result.results.some((item) => item.ok)
                  ? "打开已配好的应用，就可以开始对话。"
                  : "根据下方提示调整后，再试一次。"
                : "填入 Key，选好应用。对话模型和生图工具一起配好。"}
            </p>
          </div>
          {error && (
            <div
              role="alert"
              className="flex items-start gap-2 rounded-panel border border-border bg-danger-soft p-3 text-body text-danger-text"
            >
              <CircleAlert className="mt-0.5 h-4 w-4 shrink-0" />
              <span className="break-words">{error}</span>
            </div>
          )}
          {!result ? (
            <>
              <section
                aria-label="Oceanway 连接"
                className="rounded-panel border border-border bg-surface p-5"
              >
                {catalog && !editingKey ? (
                  <div className="flex flex-wrap items-center gap-3">
                    <CheckCircle2 className="h-5 w-5 text-success-text" />
                    <div className="min-w-0 flex-1">
                      <h3 className="text-body font-semibold">
                        Oceanway 已连接
                      </h3>
                      <p role="status" className="mt-1 text-caption text-fg-3">
                        {catalog.models.length} 个可用模型，已为你选好默认模型
                      </p>
                    </div>
                    <Button
                      variant="quiet"
                      size="compact"
                      disabled={!!busy}
                      onClick={() => setEditingKey(true)}
                    >
                      更换 Key
                    </Button>
                  </div>
                ) : (
                  <form
                    onSubmit={(e) => {
                      e.preventDefault();
                      if (!busy && (key.trim() || connection?.configured))
                        void check();
                    }}
                  >
                    <label
                      htmlFor="oceanway-key"
                      className="text-body font-semibold"
                    >
                      Oceanway API Key
                    </label>
                    <p
                      id="oceanway-key-hint"
                      className="mt-1 text-caption text-fg-3"
                    >
                      粘贴从 Oceanway 获取的 Key，服务地址已为你填好。
                    </p>
                    <div className="mt-3 flex flex-col gap-2 sm:flex-row">
                      <div className="relative min-w-0 flex-1">
                        <Input
                          id="oceanway-key"
                          type={showKey ? "text" : "password"}
                          autoComplete="off"
                          spellCheck={false}
                          aria-describedby="oceanway-key-hint"
                          value={key}
                          disabled={!!busy}
                          placeholder={
                            connection?.configured
                              ? "Key 已保存，可直接连接"
                              : "粘贴你的 API Key"
                          }
                          onChange={(e) => changeKey(e.target.value)}
                          className="h-10 pe-10"
                        />
                        <button
                          type="button"
                          disabled={!!busy}
                          onClick={() => setShowKey((v) => !v)}
                          aria-label={showKey ? "隐藏 Key" : "显示 Key"}
                          className="absolute end-2 top-1/2 -translate-y-1/2 rounded p-1 text-fg-3 focus-visible:outline focus-visible:outline-2"
                        >
                          {showKey ? (
                            <EyeOff className="h-4 w-4" />
                          ) : (
                            <Eye className="h-4 w-4" />
                          )}
                        </button>
                      </div>
                      <Button
                        type="submit"
                        variant="solid"
                        className="h-10 px-5"
                        disabled={
                          !!busy || (!key.trim() && !connection?.configured)
                        }
                      >
                        {busy === "check" ? (
                          <Loader2 className="h-4 w-4 animate-spin" />
                        ) : (
                          <ArrowRight className="h-4 w-4" />
                        )}
                        {busy === "check" ? "正在连接…" : "连接并继续"}
                      </Button>
                    </div>
                    {catalog && (
                      <Button
                        type="button"
                        variant="quiet"
                        size="compact"
                        className="mt-2"
                        onClick={() => setEditingKey(false)}
                      >
                        取消更换
                      </Button>
                    )}
                  </form>
                )}
              </section>
              <section
                aria-labelledby="oceanway-apps-label"
                className="space-y-3"
              >
                <div className="flex flex-wrap items-end justify-between gap-2">
                  <div>
                    <h3
                      id="oceanway-apps-label"
                      className="text-body font-semibold"
                    >
                      {appId ? "应用设置" : "你想用哪个应用？"}
                    </h3>
                    <p className="mt-1 text-caption text-fg-3">
                      {appId
                        ? catalog
                          ? "模型已自动匹配，也可以按需更换。"
                          : "连接后会自动选择模型。"
                        : apps.some(available)
                          ? "可以多选。默认勾选已安装的常用应用。"
                          : "先安装一个应用，再回来完成配置。"}
                    </p>
                  </div>
                  {busy === "load" && (
                    <span role="status" className="text-caption text-fg-3">
                      正在检测应用…
                    </span>
                  )}
                </div>
                <div className="space-y-2">
                  {displayed.map((app) => {
                    const checked = selected.includes(app.appId);
                    const family = FAMILY_LABELS[app.appId];
                    const missingModel =
                      !!catalog && checked && !models[app.appId];
                    const isEditing = editingModels[app.appId] || missingModel;
                    const choices = otherModels[app.appId]
                      ? textModels
                      : familyModels(app.appId, textModels);
                    const currentModel = models[app.appId];
                    const modelChoices =
                      currentModel && !choices.includes(currentModel)
                        ? [currentModel, ...choices]
                        : choices;
                    return (
                      <div
                        key={app.appId}
                        className={cn(
                          "overflow-hidden rounded-panel border bg-surface transition-colors",
                          checked ? "border-border-strong" : "border-border",
                        )}
                      >
                        <div className="flex flex-wrap items-center gap-x-3 gap-y-2 px-4 py-3.5">
                          <label
                            className={cn(
                              "flex min-w-0 flex-1 items-center gap-3",
                              available(app) && "cursor-pointer",
                            )}
                          >
                            <Checkbox
                              aria-label={`配置 ${app.name}`}
                              checked={checked}
                              disabled={!!busy || !available(app)}
                              onCheckedChange={(value) =>
                                setSelected((s) =>
                                  value
                                    ? [...new Set([...s, app.appId])]
                                    : s.filter((id) => id !== app.appId),
                                )
                              }
                            />
                            <AppGlyph app={app.appId as AppId} size={26} />
                            <div className="min-w-0">
                              <span className="block text-body font-semibold">
                                {app.name}
                              </span>
                              <span className="mt-1 block text-caption text-fg-3">
                                {!app.configurable
                                  ? (app.reason ?? "正在适配")
                                  : app.desktopInstalled
                                    ? "已检测到 Codex 桌面版"
                                    : app.installed
                                      ? "已安装"
                                      : app.configExists
                                        ? "检测到已有配置"
                                        : "尚未安装"}
                              </span>
                            </div>
                          </label>
                          {!app.installed && INSTALL_URLS[app.appId] && (
                            <Button
                              variant="outline"
                              size="regular"
                              disabled={!!busy}
                              onClick={() => void install(app.appId)}
                            >
                              <Download className="h-3.5 w-3.5" />
                              安装 {app.name}
                            </Button>
                          )}
                          {app.configurable &&
                            (app.installed || app.configExists) && (
                              <div className="flex min-w-0 items-center gap-2 ps-7 sm:ps-0">
                                <span
                                  className={cn(
                                    "max-w-[210px] truncate text-caption",
                                    missingModel
                                      ? "text-warning-text"
                                      : "text-fg-3",
                                  )}
                                  title={models[app.appId]}
                                >
                                  {catalog
                                    ? models[app.appId] || "请选择可用模型"
                                    : family
                                      ? `默认使用 ${family} 系列`
                                      : "连接后自动匹配模型"}
                                </span>
                                {catalog && (
                                  <Button
                                    variant="quiet"
                                    size="compact"
                                    aria-label={`更换 ${app.name} 模型`}
                                    aria-expanded={!!isEditing}
                                    disabled={!!busy}
                                    onClick={() =>
                                      setEditingModels((s) => ({
                                        ...s,
                                        [app.appId]: !isEditing,
                                      }))
                                    }
                                  >
                                    更换
                                    <ChevronDown
                                      className={cn(
                                        "h-3.5 w-3.5 transition-transform",
                                        isEditing && "rotate-180",
                                      )}
                                    />
                                  </Button>
                                )}
                              </div>
                            )}
                        </div>
                        {catalog && isEditing && app.configurable && (
                          <div className="space-y-2 border-t border-border bg-subtle px-4 py-3">
                            {missingModel && (
                              <p className="text-caption text-warning-text">
                                当前 Key 暂无
                                {family ? ` ${family} 系列` : "匹配的"}
                                模型，可选择其他模型或取消勾选。
                              </p>
                            )}
                            <label className="flex flex-wrap items-center gap-2 text-caption text-fg-2">
                              <span className="w-16 shrink-0">对话模型</span>
                              <select
                                aria-label={`${app.name} 对话模型`}
                                value={models[app.appId] ?? ""}
                                disabled={!!busy}
                                onChange={(e) =>
                                  setModels((m) => ({
                                    ...m,
                                    [app.appId]: e.target.value,
                                  }))
                                }
                                className="h-9 min-w-0 flex-1 rounded-control border border-input bg-surface px-2 text-body"
                              >
                                <option value="">请选择可用模型</option>
                                {modelChoices.map((model) => (
                                  <option key={model} value={model}>
                                    {model}
                                  </option>
                                ))}
                              </select>
                            </label>
                            {familyModels(app.appId, textModels).length <
                              textModels.length && (
                              <button
                                type="button"
                                disabled={!!busy}
                                aria-pressed={!!otherModels[app.appId]}
                                className="text-caption text-action-text hover:underline focus-visible:underline"
                                onClick={() =>
                                  setOtherModels((s) => ({
                                    ...s,
                                    [app.appId]: !s[app.appId],
                                  }))
                                }
                              >
                                {otherModels[app.appId]
                                  ? "只看推荐系列"
                                  : "选择其他系列模型"}
                              </button>
                            )}
                          </div>
                        )}
                      </div>
                    );
                  })}
                  {!displayed.length && !busy && (
                    <p className="rounded-panel border border-border p-4 text-body text-fg-2">
                      暂未获取到应用，请点击右上角“重新检测”。
                    </p>
                  )}
                </div>
                {!appId && extraApps.length > 0 && (
                  <Button
                    variant="quiet"
                    size="compact"
                    aria-expanded={showAllApps}
                    onClick={() => setShowAllApps((v) => !v)}
                  >
                    <ChevronRight
                      className={cn("h-3.5 w-3.5", showAllApps && "rotate-90")}
                    />
                    {showAllApps
                      ? "收起其他应用"
                      : `其他应用 (${extraApps.length})`}
                  </Button>
                )}
                {displayed.some((a) => !a.installed) && (
                  <p className="text-caption text-fg-3">
                    安装完成后，点右上角“重新检测”。左下角“应用”可管理所有安装。
                  </p>
                )}
              </section>
              <section
                aria-label="对话生图"
                className="rounded-panel border border-border bg-subtle p-4"
              >
                <div className="flex flex-wrap items-center gap-3">
                  <Image className="h-5 w-5 shrink-0 text-fg-2" />
                  <div className="min-w-0 flex-1">
                    <h3 className="text-body font-medium">对话中直接生图</h3>
                    <p className="mt-1 text-caption text-fg-3">
                      {!catalog
                        ? "有图片模型时自动配好，告诉 AI 想画什么就行。"
                        : imageModel
                          ? "将自动配置 Oceanway 生图，支持一次生成多张。"
                          : pictures.length
                            ? "本次不修改生图设置。"
                            : "当前 Key 暂未开通图片模型，可以先使用对话。"}
                    </p>
                  </div>
                  {catalog && pictures.length > 0 && (
                    <Button
                      variant="quiet"
                      size="compact"
                      disabled={!!busy}
                      aria-expanded={editingImage}
                      onClick={() => setEditingImage((v) => !v)}
                    >
                      生图设置
                      <ChevronDown
                        className={cn(
                          "h-3.5 w-3.5",
                          editingImage && "rotate-180",
                        )}
                      />
                    </Button>
                  )}
                </div>
                {editingImage && catalog && (
                  <div className="mt-3 border-t border-border pt-3">
                    <label className="flex flex-wrap items-center gap-2 text-caption text-fg-2">
                      <span className="w-16 shrink-0">图片模型</span>
                      <select
                        aria-label="图片模型"
                        value={imageModel}
                        disabled={!!busy}
                        onChange={(e) => setImageModel(e.target.value)}
                        className="h-9 min-w-0 flex-1 rounded-control border border-input bg-surface px-2 text-body"
                      >
                        <option value="">本次不修改生图设置</option>
                        {pictures.map((model) => (
                          <option key={model} value={model}>
                            {model}
                          </option>
                        ))}
                      </select>
                    </label>
                  </div>
                )}
              </section>
              <div className="flex flex-wrap items-center justify-between gap-3 border-t border-border pt-4">
                <p className="flex items-center gap-2 text-caption text-fg-3">
                  <History className="h-3.5 w-3.5" />
                  原有会话继续保留
                </p>
                {catalog && (
                  <div className="flex flex-wrap items-center gap-3">
                    <span className="text-caption text-fg-3">
                      {!selected.length
                        ? "请先选择一个应用"
                        : selected.some((id) => !models[id])
                          ? "请为所选应用选择模型"
                          : `已选 ${selected.length} 个应用`}
                    </span>
                    <Button
                      variant="solid"
                      className="h-10 px-5"
                      disabled={!canSave}
                      onClick={() => void save()}
                    >
                      {busy === "save" ? (
                        <Loader2 className="h-4 w-4 animate-spin" />
                      ) : (
                        <ArrowRight className="h-4 w-4" />
                      )}
                      {busy === "save" ? "正在配置…" : "一键配置"}
                    </Button>
                  </div>
                )}
              </div>
            </>
          ) : (
            <section
              aria-label="配置结果"
              aria-live="polite"
              className="space-y-4"
            >
              <div className="divide-y divide-border overflow-hidden rounded-panel border border-border bg-surface">
                {result.results.map((item) => (
                  <div key={item.appId} className="p-5">
                    <div className="flex flex-wrap items-center gap-3">
                      <AppGlyph app={item.appId as AppId} size={28} />
                      <div className="min-w-0 flex-1">
                        <h3 className="text-body font-semibold">
                          {appName(item.appId)}
                        </h3>
                        <p
                          className={cn(
                            "mt-1 flex items-center gap-1.5 text-caption",
                            item.ok ? "text-success-text" : "text-danger-text",
                          )}
                        >
                          {item.ok ? (
                            <CheckCircle2 className="h-3.5 w-3.5" />
                          ) : (
                            <CircleAlert className="h-3.5 w-3.5" />
                          )}
                          {item.ok ? "对话已配好" : "配置未完成"}
                        </p>
                      </div>
                      {item.ok && (
                        <Button
                          variant="solid"
                          className="h-9"
                          disabled={!!busy}
                          onClick={() => {
                            if (
                              !apps.find((a) => a.appId === item.appId)
                                ?.installed &&
                              INSTALL_URLS[item.appId]
                            )
                              void install(item.appId);
                            else if (
                              apps.find((a) => a.appId === item.appId)
                                ?.desktopInstalled
                            )
                              setRestartApp(item.appId);
                            else void launch(item.appId, false);
                          }}
                        >
                          {busy === "launch" ? (
                            <Loader2 className="h-4 w-4 animate-spin" />
                          ) : (
                            <ArrowRight className="h-4 w-4" />
                          )}
                          {apps.find((a) => a.appId === item.appId)?.installed
                            ? "打开"
                            : "安装"}{" "}
                          {appName(item.appId)}
                        </Button>
                      )}
                    </div>
                    {!item.ok && (
                      <p className="mt-3 break-words text-body text-danger-text">
                        {item.message}
                      </p>
                    )}
                    {item.ok &&
                      (item.imageConfigured ? (
                        <p className="mt-3 text-body text-fg-2">
                          生图已配好，试着说：“帮我画两张海边日落。”
                        </p>
                      ) : item.imageMessage ? (
                        <p className="mt-3 break-words text-body text-warning-text">
                          生图：{item.imageMessage}
                        </p>
                      ) : null)}
                    <details className="mt-3 text-caption text-fg-3">
                      <summary className="w-fit cursor-pointer rounded focus-visible:outline focus-visible:outline-2">
                        配置详情
                      </summary>
                      <div className="mt-2 space-y-1 break-words">
                        <p>{item.message}</p>
                        {item.imageConfigured && item.imageMessage && (
                          <p>{item.imageMessage}</p>
                        )}
                      </div>
                    </details>
                  </div>
                ))}
              </div>
              <div className="flex flex-wrap items-center justify-between gap-3">
                <p className="text-caption text-fg-3">
                  原有会话继续保留，配置备份已保存。
                </p>
                <Button
                  variant="quiet"
                  disabled={!!busy}
                  onClick={() => {
                    setResult(undefined);
                    setError("");
                    setLaunchMessage("");
                  }}
                >
                  返回调整
                </Button>
              </div>
            </section>
          )}
          {launchMessage && (
            <p
              role="status"
              className="rounded-panel border border-border bg-subtle p-4 text-body"
            >
              {launchMessage}
            </p>
          )}
        </div>
      </div>
      <ConfirmDialog
        isOpen={!!restartApp}
        variant="info"
        title={`打开 ${restartApp ? appName(restartApp) : "应用"} 并加载配置`}
        message="如果应用已经打开，会先关闭再重新打开。请先保存正在进行的工作。"
        confirmText="保存好了，打开应用"
        cancelText="稍后打开"
        pending={busy === "launch"}
        onCancel={() => setRestartApp(undefined)}
        onConfirm={() => restartApp && void launch(restartApp, true)}
      />
    </>
  );
}
