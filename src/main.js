const DEFAULT_BASE_URL = "https://ocean-way.top";

const serviceDot = document.querySelector("#service-dot");
const serviceStatus = document.querySelector("#service-status");
const serviceDetail = document.querySelector("#service-detail");
const loginDot = document.querySelector("#login-dot");
const loginStatus = document.querySelector("#login-status");
const loginDetail = document.querySelector("#login-detail");
const imageDot = document.querySelector("#image-dot");
const imageStatus = document.querySelector("#image-status");
const imageDetail = document.querySelector("#image-detail");
const savedKeyState = document.querySelector("#saved-key-state");
const keyHelperText = document.querySelector("#key-helper-text");
const statusText = document.querySelector("#status");
const statusMessage = document.querySelector("#status-message");
const configForm = document.querySelector("#config-form");
const apiKeyInput = document.querySelector("#api-key");
const baseUrlInput = document.querySelector("#base-url");
const toggleKeyButton = document.querySelector("#toggle-key-button");
const testButton = document.querySelector("#test-button");
const imagegenButton = document.querySelector("#imagegen-button");
const imagegenRowDetail = document.querySelector("#imagegen-row-detail");
const restoreButton = document.querySelector("#restore-button");
const restoreDialog = document.querySelector("#restore-dialog");
const cancelRestoreButton = document.querySelector("#cancel-restore-button");
const confirmRestoreButton = document.querySelector("#confirm-restore-button");
const migrateHistoryButton = document.querySelector("#migrate-history-button");
const historyRowDetail = document.querySelector("#history-row-detail");
const advancedOptions = document.querySelector("#advanced-options");
const openDirButton = document.querySelector("#open-dir-button");

const invoke = window.__TAURI__?.core?.invoke;
let resizeTimer = 0;
let currentStatus = {
  configured: false,
  hasApiKey: false,
  imagegenCliConfigured: false,
};

function setBusy(isBusy) {
  for (const element of document.querySelectorAll("button, input")) {
    element.disabled = isBusy;
  }
  if (!isBusy) {
    cancelRestoreButton.disabled = false;
    confirmRestoreButton.disabled = false;
  }
}

function setStatus(message, kind = "") {
  statusMessage.textContent = message;
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

  const contentHeight = Math.ceil(document.documentElement.scrollHeight + 18);
  const maxHeight = Math.max(560, Math.min(760, window.screen.availHeight - 80));
  const height = Math.max(500, Math.min(contentHeight, maxHeight));
  try {
    await invoke("resize_window_to_content", { height });
  } catch {
    // Dynamic sizing is a convenience; scrolling remains available if the platform refuses it.
  }
}

function setDot(element, kind) {
  element.dataset.kind = kind;
}

function normalizeBaseUrl(value) {
  const trimmed = value.trim();
  return trimmed || DEFAULT_BASE_URL;
}

function authStrategyText(authStrategy) {
  return authStrategy === "chatgptBearerToken"
    ? "已保留 ChatGPT 登录态，并更新 OceanWay provider。"
    : "已保存 API Key，并启用 Codex Desktop 图片工具兼容配置。";
}

function setCurrentStatus(status) {
  currentStatus = status;

  serviceStatus.textContent = status.configured ? "已配置" : "未配置";
  serviceDetail.textContent = status.configured
    ? status.baseUrl || DEFAULT_BASE_URL
    : "等待完成配置";
  serviceDetail.title = serviceDetail.textContent;
  setDot(serviceDot, status.configured ? "success" : "muted");

  loginStatus.textContent = status.chatgptLoginDetected ? "已检测" : "未登录";
  loginDetail.textContent = status.chatgptLoginDetected
    ? status.chatgptAccountLabel || "ChatGPT 登录态"
    : "将使用 API Key 模式";
  loginDetail.title = loginDetail.textContent;
  setDot(loginDot, status.chatgptLoginDetected ? "success" : "muted");

  imageStatus.textContent = status.imagegenCliConfigured ? "已就绪" : "待配置";
  imageDetail.textContent = status.imagegenCliConfigured
    ? "内置兼容与 CLI 备用已同步"
    : status.configured
      ? "可在高级选项中同步"
      : "完成配置时自动同步";
  setDot(imageDot, status.imagegenCliConfigured ? "success" : "warning");

  savedKeyState.hidden = !status.hasApiKey;
  apiKeyInput.required = !status.hasApiKey;
  apiKeyInput.placeholder = status.hasApiKey
    ? "已保存；留空继续使用，输入新 Key 可替换"
    : "请输入 OceanWay API Key";
  keyHelperText.textContent = status.hasApiKey
    ? "已安全保存到本机 Codex 配置。留空会继续使用，不会在界面回显完整 Key。"
    : "Key 仅保存到本机 Codex 配置中，软件不会在界面回显完整内容。";

  if (status.baseUrl && document.activeElement !== baseUrlInput) {
    baseUrlInput.value = status.baseUrl;
  }

  imagegenButton.textContent = status.imagegenCliConfigured ? "重新同步" : "同步";
  imagegenRowDetail.textContent = status.imagegenCliConfigured
    ? "已同步；内置工具不可用时可使用 CLI 备用路径。"
    : "尚未同步；完成主配置时会自动处理。";
}

async function refreshStatus() {
  if (!invoke) {
    setCurrentStatus({
      configured: false,
      hasApiKey: false,
      chatgptLoginDetected: true,
      chatgptAccountLabel: "浏览器预览",
      imagegenCliConfigured: false,
      baseUrl: DEFAULT_BASE_URL,
    });
    return;
  }

  try {
    const status = await invoke("get_config_status");
    setCurrentStatus(status);
  } catch (error) {
    serviceStatus.textContent = "读取失败";
    serviceDetail.textContent = "请检查 Codex 配置目录";
    loginStatus.textContent = "未知";
    loginDetail.textContent = "-";
    imageStatus.textContent = "未知";
    imageDetail.textContent = "-";
    setDot(serviceDot, "error");
    setDot(loginDot, "error");
    setDot(imageDot, "error");
    setStatus(`读取当前配置失败：${error}`, "error");
  }
}

function readFormValues() {
  const apiKey = apiKeyInput.value.trim();
  const baseUrl = normalizeBaseUrl(baseUrlInput.value);
  if (!apiKey && !currentStatus.hasApiKey) {
    setStatus("首次配置请先输入 OceanWay API Key。", "error");
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
  setStatus(values.apiKey ? "正在保存并配置 OceanWay AI..." : "正在使用已保存的 Key 更新配置...");
  try {
    const result = await invoke("configure_provider", values);
    apiKeyInput.value = "";
    apiKeyInput.type = "password";
    await refreshStatus();
    setStatus(
      `${authStrategyText(result.authStrategy)}图片备用配置已同步。请完全退出并重新打开 Codex，然后新建任务。`,
      "success"
    );
  } catch (error) {
    setStatus(`配置失败：${error}`, "error");
  } finally {
    setBusy(false);
  }
}

async function configureImagegenCli() {
  if (!invoke) {
    setStatus("浏览器预览无法写入本机图片备用配置。", "error");
    return;
  }

  setBusy(true);
  setStatus("正在同步图片备用配置...");
  try {
    await invoke("configure_imagegen_cli");
    await refreshStatus();
    setStatus("图片备用配置已同步。请完全退出并重新打开 Codex，然后新建任务。", "success");
  } catch (error) {
    setStatus(`图片备用配置失败：${error}`, "error");
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
  setStatus(values.apiKey ? "正在测试连接..." : "正在使用已保存的 Key 测试连接...");
  try {
    const result = await invoke("test_connection", values);
    setStatus(`${result.message} 测试地址：${result.endpoint}`, result.ok ? "success" : "error");
  } catch (error) {
    setStatus(`测试失败：${error}`, "error");
  } finally {
    setBusy(false);
  }
}

function openRestoreDialog() {
  if (typeof restoreDialog.showModal === "function") {
    restoreDialog.showModal();
  } else {
    restoreDefaults();
  }
}

async function restoreDefaults() {
  if (restoreDialog.open) {
    restoreDialog.close();
  }

  if (!invoke) {
    setCurrentStatus({
      configured: false,
      hasApiKey: false,
      chatgptLoginDetected: false,
      imagegenCliConfigured: false,
      baseUrl: DEFAULT_BASE_URL,
    });
    setStatus("浏览器预览已恢复默认状态，OceanWay Key 与图片备用配置已移除。", "success");
    return;
  }

  setBusy(true);
  setStatus("正在恢复首次使用本工具前的 Codex 配置...");

  try {
    const result = await invoke("restore_defaults");
    apiKeyInput.value = "";
    await refreshStatus();
    const restored = result.historyMigrationRestore;
    const historyText = restored?.restoredBackups
      ? `同时撤销了 ${restored.restoredSessionFiles} 个历史会话文件和 ${restored.sqliteRowsRestored} 行索引迁移。`
      : "没有需要撤销的历史迁移。";
    setStatus(
      `已恢复 Codex 默认配置，并撤销 OceanWay Key 与图片备用配置。${historyText}请重启 Codex。`,
      "success"
    );
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

async function refreshHistoryStatus() {
  if (!invoke || !advancedOptions.open) {
    return;
  }

  try {
    const status = await invoke("get_history_migration_status");
    if (!status.migrationSupported) {
      historyRowDetail.textContent = "完成 OceanWay 配置后可检测旧任务。";
      return;
    }
    historyRowDetail.textContent = status.needsMigration
      ? `检测到 ${status.rolloutFilesToUpdate} 个会话文件需要迁移。`
      : "历史记录已与当前 provider 一致，无需迁移。";
  } catch {
    historyRowDetail.textContent = "无法读取历史状态，请稍后重试。";
  }
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
      setStatus("请先完成 OceanWay 配置，再迁移历史记录。", "error");
      return;
    }
    if (!status.needsMigration) {
      setStatus("历史会话已与当前 provider 一致，无需迁移。", "success");
      return;
    }

    const warningText = status.encryptedContentFiles
      ? `\n\n其中 ${status.encryptedContentFiles} 个会话包含 encrypted_content；迁移只修复列表可见性。`
      : "";
    const confirmed = window.confirm(
      `将迁移 ${status.rolloutFilesToUpdate} 个会话文件和 ${status.sqliteRowsToUpdate} 行索引。\n` +
      `当前分布：${historyProviderCountText(status.providerCounts)}。${warningText}\n\n执行前会自动备份，是否继续？`
    );
    if (!confirmed) {
      setStatus("已取消历史迁移。");
      return;
    }

    setStatus("正在迁移历史可见性...");
    const result = await invoke("migrate_history_visibility");
    setStatus(
      `历史迁移完成：${result.changedSessionFiles} 个会话文件，${result.sqliteRowsUpdated} 行索引。`,
      "success"
    );
    await refreshHistoryStatus();
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
  if (!apiKeyInput.value) {
    apiKeyInput.focus();
    return;
  }
  const shouldShow = apiKeyInput.type === "password";
  apiKeyInput.type = shouldShow ? "text" : "password";
  toggleKeyButton.title = shouldShow ? "隐藏 API Key" : "显示 API Key";
  toggleKeyButton.setAttribute("aria-label", shouldShow ? "隐藏 API Key" : "显示 API Key");
}

configForm.addEventListener("submit", configureProvider);
testButton.addEventListener("click", testConnection);
imagegenButton.addEventListener("click", configureImagegenCli);
restoreButton.addEventListener("click", openRestoreDialog);
confirmRestoreButton.addEventListener("click", restoreDefaults);
migrateHistoryButton.addEventListener("click", migrateHistoryVisibility);
openDirButton.addEventListener("click", openConfigDirectory);
toggleKeyButton.addEventListener("click", toggleApiKeyVisibility);
advancedOptions.addEventListener("toggle", () => {
  refreshHistoryStatus();
  scheduleWindowResize();
});

await refreshStatus();
scheduleWindowResize();
