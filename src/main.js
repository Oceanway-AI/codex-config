const DEFAULT_BASE_URL = "https://ocean-way.top";

const currentProvider = document.querySelector("#current-provider");
const currentLoginState = document.querySelector("#current-login-state");
const currentBaseUrl = document.querySelector("#current-base-url");
const currentApiKey = document.querySelector("#current-api-key");
const statusText = document.querySelector("#status");
const configForm = document.querySelector("#config-form");
const apiKeyInput = document.querySelector("#api-key");
const baseUrlInput = document.querySelector("#base-url");
const toggleKeyButton = document.querySelector("#toggle-key-button");
const testButton = document.querySelector("#test-button");
const restoreButton = document.querySelector("#restore-button");
const migrateHistoryButton = document.querySelector("#migrate-history-button");
const exitButton = document.querySelector("#exit-button");
const openDirButton = document.querySelector("#open-dir-button");

const invoke = window.__TAURI__?.core?.invoke;
let resizeTimer = 0;

function setBusy(isBusy) {
  for (const element of document.querySelectorAll("button, input")) {
    element.disabled = isBusy;
  }
}

function setStatus(message, kind = "") {
  statusText.textContent = message;
  statusText.dataset.kind = kind;
  statusText.hidden = !message;
  scheduleWindowResize();
}

function scheduleWindowResize() {
  window.clearTimeout(resizeTimer);
  resizeTimer = window.setTimeout(resizeWindowToContent, 80);
}

async function resizeWindowToContent() {
  if (!invoke) {
    return;
  }

  const contentHeight = Math.ceil(document.documentElement.scrollHeight + 20);
  const maxHeight = Math.max(520, Math.min(760, window.screen.availHeight - 80));
  const height = Math.max(440, Math.min(contentHeight, maxHeight));
  try {
    await invoke("resize_window_to_content", { height });
  } catch {
    // Resizing is a convenience; the UI remains usable if the platform refuses it.
  }
}

function normalizeBaseUrl(value) {
  const trimmed = value.trim();
  return trimmed || DEFAULT_BASE_URL;
}

function authStrategyText(authStrategy) {
  return authStrategy === "chatgptBearerToken"
    ? "已保留 ChatGPT 登录态，并使用 provider token。"
    : "已写入 API Key，并启用 Codex Desktop 本地图片工具兼容模式。";
}

function setCurrentStatus(status) {
  currentProvider.textContent = status.configured ? "已配置 OceanWay AI" : "未配置 OceanWay AI";
  currentLoginState.textContent = status.chatgptLoginDetected
    ? `已登录：${status.chatgptAccountLabel || "账号未知"}`
    : "未检测到";
  currentLoginState.title = currentLoginState.textContent;
  currentLoginState.dataset.kind = status.chatgptLoginDetected ? "success" : "muted";
  currentBaseUrl.textContent = status.baseUrl || "-";
  currentBaseUrl.title = status.baseUrl || "";
  if (status.authStrategy === "chatgptBearerToken") {
    currentApiKey.textContent = "登录态 + Provider Token";
  } else {
    currentApiKey.textContent = status.hasApiKey ? "API Key 已保存" : "未保存";
  }
}

async function refreshStatus() {
  if (!invoke) {
    currentProvider.textContent = "预览模式";
    currentLoginState.textContent = "-";
    currentBaseUrl.textContent = DEFAULT_BASE_URL;
    currentApiKey.textContent = "-";
    return;
  }

  try {
    const status = await invoke("get_config_status");
    setCurrentStatus(status);
  } catch (error) {
    currentProvider.textContent = "读取失败";
    currentLoginState.textContent = "-";
    currentBaseUrl.textContent = "-";
    currentApiKey.textContent = "-";
    setStatus(`读取当前配置失败：${error}`, "error");
  }
}

function readFormValues() {
  const apiKey = apiKeyInput.value.trim();
  const baseUrl = normalizeBaseUrl(baseUrlInput.value);
  if (!apiKey) {
    setStatus("请先输入 API Key。", "error");
    apiKeyInput.focus();
    return null;
  }
  return { apiKey, baseUrl };
}

async function configureProvider(event) {
  event.preventDefault();
  const values = readFormValues();
  if (!values) {
    return;
  }

  if (!invoke) {
    setStatus("浏览器预览无法写入本机配置。", "error");
    return;
  }

  setBusy(true);
  setStatus("正在配置 OceanWay AI...");
  try {
    const result = await invoke("configure_provider", values);
    await refreshStatus();
    setStatus(`${authStrategyText(result.authStrategy)}请完全退出并重新打开 Codex，然后新建任务。`, "success");
  } catch (error) {
    setStatus(`配置失败：${error}`, "error");
  } finally {
    setBusy(false);
  }
}

async function testConnection() {
  const values = readFormValues();
  if (!values) {
    return;
  }

  if (!invoke) {
    setStatus(`预览测试通过。测试地址：${values.baseUrl}`, "success");
    return;
  }

  setBusy(true);
  setStatus("正在测试连接...");
  try {
    const result = await invoke("test_connection", values);
    setStatus(`${result.message} 测试地址：${result.endpoint}`, result.ok ? "success" : "error");
  } catch (error) {
    setStatus(`测试失败：${error}`, "error");
  } finally {
    setBusy(false);
  }
}

async function restoreDefaults() {
  const confirmed = window.confirm("将恢复到首次使用本工具前的 Codex 配置，并撤销本工具记录过的历史迁移。是否继续？");
  if (!confirmed) {
    return;
  }

  if (!invoke) {
    setStatus("浏览器预览已恢复默认状态。", "success");
    return;
  }

  setBusy(true);
  setStatus("正在恢复默认值...");

  try {
    const result = await invoke("restore_defaults");
    await refreshStatus();
    const restored = result.historyMigrationRestore;
    const historyText = restored?.restoredBackups
      ? `已撤销历史迁移：${restored.restoredSessionFiles} 个会话文件，${restored.sqliteRowsRestored} 行索引。`
      : "没有需要撤销的历史迁移。";
    const warningText = restored?.warnings?.length ? `有 ${restored.warnings.length} 个历史迁移撤销提示。` : "";
    setStatus(`已恢复默认值，请重启 Codex。已写入：${result.configPath}。${historyText}${warningText}`, "success");
  } catch (error) {
    setStatus(`恢复失败：${error}`, "error");
  } finally {
    setBusy(false);
  }
}

function historyProviderCountText(providerCounts = []) {
  return providerCounts
    .map((item) => `${item.provider}: ${item.files}`)
    .join("，") || "无";
}

async function migrateHistoryVisibility() {
  if (!invoke) {
    setStatus("浏览器预览无法迁移本机历史。", "error");
    return;
  }

  setBusy(true);
  setStatus("正在扫描历史会话...");

  try {
    const status = await invoke("get_history_migration_status");
    if (!status.migrationSupported) {
      setStatus(
        `历史迁移仅在当前 provider 为 OceanWay 时可用。当前 provider：${status.targetProvider}。恢复默认会撤销已记录的历史迁移。`,
        "error"
      );
      return;
    }
    if (!status.needsMigration) {
      setStatus(`历史会话已与当前 provider（${status.targetProvider}）一致，无需迁移。`, "success");
      return;
    }

    const warningText = status.encryptedContentFiles
      ? `\n\n检测到 ${status.encryptedContentFiles} 个会话包含 encrypted_content。迁移只修复列表可见性，继续对话或 compact 仍可能失败。`
      : "";
    const confirmed = window.confirm(
      `将把旧历史会话的 provider metadata 迁移到当前 provider：${status.targetProvider}。\n\n` +
      `待迁移会话文件：${status.rolloutFilesToUpdate} 个\n` +
      `待更新索引行：${status.sqliteRowsToUpdate} 行\n` +
      `当前分布：${historyProviderCountText(status.providerCounts)}\n\n` +
      `执行前会自动备份，可在“恢复默认”时撤销本工具记录过的迁移。不会修改对话内容。${warningText}\n\n是否继续？`
    );
    if (!confirmed) {
      setStatus("已取消历史迁移。");
      return;
    }

    setStatus("正在迁移历史可见性...");
    const result = await invoke("migrate_history_visibility");
    const warningSuffix = result.warnings?.length ? `有 ${result.warnings.length} 个提示。` : "";
    setStatus(
      `历史迁移完成：${result.changedSessionFiles} 个会话文件，${result.sqliteRowsUpdated} 行索引。备份：${result.backupPath || "无需备份"}。${warningSuffix}`,
      "success"
    );
  } catch (error) {
    setStatus(`历史迁移失败：${error}`, "error");
  } finally {
    setBusy(false);
  }
}

async function openConfigDirectory() {
  if (!invoke) {
    setStatus("浏览器预览无法打开本机目录。", "error");
    return;
  }

  try {
    await invoke("open_config_dir");
    setStatus("已打开 Codex 配置目录。");
  } catch (error) {
    setStatus(`打开配置目录失败：${error}`, "error");
  }
}

function toggleApiKeyVisibility() {
  const shouldShow = apiKeyInput.type === "password";
  apiKeyInput.type = shouldShow ? "text" : "password";
  toggleKeyButton.title = shouldShow ? "隐藏 API Key" : "显示 API Key";
  toggleKeyButton.setAttribute("aria-label", shouldShow ? "隐藏 API Key" : "显示 API Key");
}

async function exitApp() {
  if (!invoke) {
    window.close();
    return;
  }

  await invoke("exit_app");
}

configForm.addEventListener("submit", configureProvider);
testButton.addEventListener("click", testConnection);
restoreButton.addEventListener("click", restoreDefaults);
migrateHistoryButton.addEventListener("click", migrateHistoryVisibility);
exitButton.addEventListener("click", exitApp);
openDirButton.addEventListener("click", openConfigDirectory);
toggleKeyButton.addEventListener("click", toggleApiKeyVisibility);
statusText.hidden = true;
await refreshStatus();
window.addEventListener("resize", scheduleWindowResize);
scheduleWindowResize();
