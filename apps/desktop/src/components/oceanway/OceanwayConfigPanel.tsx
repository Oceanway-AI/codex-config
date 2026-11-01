import { useEffect, useRef, useState } from "react";
import {
  Check,
  CheckCircle2,
  CircleAlert,
  Eye,
  EyeOff,
  Image,
  KeyRound,
  Loader2,
  RefreshCw,
} from "lucide-react";
import { useQueryClient } from "@tanstack/react-query";
import { AppPageHeader } from "@/components/shell/AppPageHeader";
import { AppGlyph } from "@/components/shell/AppGlyph";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Checkbox } from "@/components/ui/checkbox";
import { ConfirmDialog } from "@/components/ConfirmDialog";
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

/** Oceanway's simplified setup inside the original CC Switch application shell. */
export function OceanwayConfigPanel({ appId }: { appId?: AppId }) {
  const queryClient = useQueryClient();
  const [connection, setConnection] = useState<ConnectionSummary>();
  const [apps, setApps] = useState<DetectedApp[]>([]);
  const [key, setKey] = useState("");
  const [showKey, setShowKey] = useState(false);
  const [showAllApps, setShowAllApps] = useState(false);
  const [otherModels, setOtherModels] = useState<Record<string, boolean>>({});
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
  const textModels = conversationModels(catalog?.models ?? []);
  const pictures = imageModels(catalog?.models ?? []);
  const displayed = appId
    ? apps.filter((a) => a.appId === appId)
    : showAllApps
      ? apps
      : apps.filter((a) => a.configurable && (a.installed || a.configExists));
  const canSave =
    !!catalog &&
    selected.length > 0 &&
    selected.every((id) => !!models[id]) &&
    !busy;

  async function refresh() {
    setBusy("load");
    setError("");
    try {
      const [summary, detected] = await Promise.all([
        oceanwayApi.connection(),
        oceanwayApi.detectApps(),
      ]);
      setConnection(summary);
      setApps(detected);
      setSelected(
        detected
          .filter(
            (a) =>
              a.configurable &&
              (a.installed || a.configExists) &&
              (appId
                ? a.appId === appId
                : a.appId === "codex" || a.appId === "claude"),
          )
          .map((a) => a.appId),
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
    setImageModel("");
    setError("");
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
      const images = imageModels(data.models);
      setImageModel(
        connection?.defaultImageModel &&
          images.includes(connection.defaultImageModel)
          ? connection.defaultImageModel
          : recommendedImageModel(images),
      );
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
      // Keep the key out of persisted browser state and clear it after the backend saved it.
      if (response.results.some((r) => r.ok)) {
        setConnection(await oceanwayApi.connection());
        setKey("");
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
    try {
      setLaunchMessage((await oceanwayApi.launch(id, restart)).message);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(null);
      setRestartApp(undefined);
    }
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
              <RefreshCw className="h-4 w-4" />
              重新检测
            </Button>
          }
        />
      )}
      <div
        id="main-content"
        className="min-h-0 flex-1 overflow-y-auto scroll-stable px-6 pb-10 pt-5"
      >
        <div className="mx-auto max-w-[840px] space-y-6">
          <div>
            <h2 className="text-section font-semibold">
              {appId ? "配置 Oceanway" : "填入 Key，配置你的 AI 应用"}
            </h2>
            <p className="mt-1 text-body text-fg-2">
              默认配好模型，也能自己换。配置保存后，再由你决定何时重启。
            </p>
          </div>
          {error && (
            <div
              role="alert"
              className="flex items-start gap-2 rounded-panel border border-danger/30 bg-danger-soft p-3 text-body text-danger-text"
            >
              <CircleAlert className="mt-0.5 h-4 w-4 shrink-0" />
              <span>{error}</span>
            </div>
          )}
          <section
            className="rounded-panel border border-border bg-surface p-4"
            aria-labelledby="oceanway-key-label"
          >
            <label
              id="oceanway-key-label"
              htmlFor="oceanway-key"
              className="text-body font-medium"
            >
              Oceanway API Key
            </label>
            <div className="mt-2 flex flex-wrap items-center gap-2">
              <div className="relative min-w-[220px] flex-1">
                <Input
                  id="oceanway-key"
                  type={showKey ? "text" : "password"}
                  autoComplete="off"
                  spellCheck={false}
                  value={key}
                  disabled={!!busy}
                  placeholder={
                    connection?.configured
                      ? `已保存 ${connection.keyPreview ?? "Key"}，留空使用`
                      : "粘贴你的 API Key"
                  }
                  onChange={(e) => changeKey(e.target.value)}
                  className="pe-10"
                />
                <button
                  type="button"
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
                variant="outline"
                size="regular"
                onClick={() => void check()}
                disabled={!!busy || (!key.trim() && !connection?.configured)}
              >
                {busy === "check" ? (
                  <Loader2 className="h-4 w-4 animate-spin" />
                ) : catalog ? (
                  <Check className="h-4 w-4" />
                ) : null}
                {catalog ? "刷新可用模型" : "连接并获取模型"}
              </Button>
            </div>
            <p className="mt-2 text-caption text-fg-3">
              {catalog
                ? `已连接 · ${catalog.models.length} 个可用模型`
                : "服务地址已配好，无需填写。这里只检查连接，不发起生图。"}
            </p>
          </section>

          <section aria-labelledby="oceanway-apps-label">
            <div className="mb-3 flex items-center justify-between">
              <h3 id="oceanway-apps-label" className="text-body font-semibold">
                {appId ? "对话模型" : "选择要配置的应用"}
              </h3>
              {busy === "load" && (
                <span role="status" className="text-caption text-fg-3">
                  正在检测…
                </span>
              )}
            </div>
            <div className="divide-y divide-border overflow-hidden rounded-panel border border-border bg-surface">
              {displayed.map((app) => (
                <div
                  key={app.appId}
                  className="flex flex-wrap items-center gap-3 px-4 py-3"
                >
                  <Checkbox
                    aria-label={`配置 ${app.name}`}
                    checked={selected.includes(app.appId)}
                    disabled={
                      !!busy ||
                      !app.configurable ||
                      (!app.installed && !app.configExists)
                    }
                    onCheckedChange={(v) => {
                      setResult(undefined);
                      setSelected((s) =>
                        v
                          ? [...s, app.appId]
                          : s.filter((id) => id !== app.appId),
                      );
                    }}
                  />
                  <AppGlyph app={app.appId as AppId} size={22} />
                  <div className="min-w-[130px] flex-1">
                    <div className="text-body font-medium">{app.name}</div>
                    <div className="mt-0.5 text-caption text-fg-3">
                      {!app.configurable
                        ? (app.reason ?? "正在适配")
                        : app.desktopInstalled
                          ? "已检测到 Codex 桌面版"
                          : app.installed
                            ? "已检测到"
                            : app.configExists
                              ? "检测到已有配置"
                              : "未检测到，请先安装"}
                    </div>
                  </div>
                  {app.configurable && (
                    <div className="flex flex-col items-end gap-1">
                      <select
                        aria-label={`${app.name} 对话模型`}
                        value={models[app.appId] ?? ""}
                        onChange={(e) => {
                          setResult(undefined);
                          setModels((m) => ({
                            ...m,
                            [app.appId]: e.target.value,
                          }));
                        }}
                        disabled={!catalog || !!busy}
                        className="h-8 w-[230px] max-w-full rounded-control border border-input bg-app px-2 text-body disabled:opacity-50"
                      >
                        <option value="">
                          {catalog ? "请选择可用模型" : "连接后自动选择"}
                        </option>
                        {(otherModels[app.appId]
                          ? textModels
                          : familyModels(app.appId, textModels)
                        ).map((model) => (
                          <option key={model} value={model}>
                            {model}
                          </option>
                        ))}
                      </select>
                      {catalog &&
                        familyModels(app.appId, textModels).length <
                          textModels.length && (
                          <button
                            type="button"
                            className="text-caption text-action-text hover:underline"
                            onClick={() =>
                              setOtherModels((s) => ({
                                ...s,
                                [app.appId]: true,
                              }))
                            }
                          >
                            {otherModels[app.appId]
                              ? "已显示全部可用模型"
                              : "选择其他模型"}
                          </button>
                        )}
                    </div>
                  )}
                </div>
              ))}
              {!displayed.length && !busy && (
                <p className="p-4 text-body text-fg-3">
                  暂时没有检测结果，请重新检测。
                </p>
              )}
            </div>
            {!appId && (
              <Button
                variant="quiet"
                size="compact"
                className="mt-2"
                onClick={() => setShowAllApps((v) => !v)}
              >
                {showAllApps ? "收起其他应用" : "查看其他应用"}
              </Button>
            )}
          </section>

          <section className="flex flex-wrap items-center gap-3 rounded-panel border border-border bg-surface p-4">
            <Image className="h-5 w-5 shrink-0 text-fg-2" />
            <div className="min-w-[180px] flex-1">
              <h3 className="text-body font-medium">图片模型</h3>
              <p className="mt-1 text-caption text-fg-3">
                {catalog && !pictures.length
                  ? "当前 Key 没有识别到可用图片模型，可先配置对话。"
                  : "各应用共用，在对话中直接告诉 AI 要画什么。"}
              </p>
            </div>
            <select
              aria-label="图片模型"
              value={imageModel}
              onChange={(e) => {
                setResult(undefined);
                setImageModel(e.target.value);
              }}
              disabled={!catalog || !!busy}
              className="h-8 w-[230px] max-w-full rounded-control border border-input bg-app px-2 text-body disabled:opacity-50"
            >
              <option value="">暂不配置生图</option>
              {pictures.map((model) => (
                <option key={model} value={model}>
                  {model}
                </option>
              ))}
            </select>
          </section>
          <div className="flex flex-wrap items-center justify-between gap-3">
            <p className="text-caption text-fg-3">
              原有历史继续保留，可在“会话”中查看。
            </p>
            <Button
              variant="solid"
              size="regular"
              disabled={!canSave}
              onClick={() => void save()}
            >
              {busy === "save" && <Loader2 className="h-4 w-4 animate-spin" />}
              一键配置{selected.length > 1 ? ` ${selected.length} 个应用` : ""}
            </Button>
          </div>

          {result && (
            <section
              aria-live="polite"
              className="space-y-3 rounded-panel border border-border p-4"
            >
              <h3 className="text-body font-semibold">配置结果</h3>
              {result.results.map((item) => (
                <div
                  key={item.appId}
                  className="flex flex-wrap items-center gap-3"
                >
                  {item.ok ? (
                    <CheckCircle2 className="h-4 w-4 shrink-0 text-success-text" />
                  ) : (
                    <CircleAlert className="h-4 w-4 shrink-0 text-danger-text" />
                  )}
                  <div className="min-w-[180px] flex-1 text-body">
                    <span className="font-medium">
                      {apps.find((a) => a.appId === item.appId)?.name ??
                        item.appId}
                    </span>
                    <p className="text-caption text-fg-2">{item.message}</p>
                    {item.imageMessage && (
                      <p className="mt-1 text-caption text-fg-2">
                        图片：{item.imageMessage}
                      </p>
                    )}
                  </div>
                  {item.ok && (
                    <Button
                      variant="outline"
                      size="regular"
                      disabled={!!busy}
                      onClick={() => setRestartApp(item.appId)}
                    >
                      重启应用并加载新配置
                    </Button>
                  )}
                </div>
              ))}
              <p className="text-caption text-fg-3">
                {result.imagesConfigured && "Oceanway 图片工具已配置。 "}
                配置备份已保存。
              </p>
            </section>
          )}
          {launchMessage && (
            <div
              role="status"
              className="rounded-panel border border-border bg-subtle p-4 text-body"
            >
              {launchMessage}
              <div className="mt-2 flex flex-wrap gap-2">
                {result?.results
                  .filter((r) => r.ok)
                  .map((r) => (
                    <Button
                      key={r.appId}
                      variant="outline"
                      size="regular"
                      disabled={!!busy}
                      onClick={() => void launch(r.appId, false)}
                    >
                      启动{" "}
                      {apps.find((a) => a.appId === r.appId)?.name ?? r.appId}
                    </Button>
                  ))}
              </div>
            </div>
          )}
        </div>
      </div>
      <ConfirmDialog
        isOpen={!!restartApp}
        variant="info"
        title="加载新配置"
        message="请先保存应用中的工作。关闭并重新打开应用后，新配置才会生效。"
        confirmText="重启应用"
        cancelText="稍后再说"
        pending={busy === "launch"}
        onCancel={() => setRestartApp(undefined)}
        onConfirm={() => restartApp && void launch(restartApp, true)}
      />
    </>
  );
}
