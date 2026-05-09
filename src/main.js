const DEFAULT_BASE_URL = "https://ocean-way.top";

const form = document.querySelector("#config-form");
const apiKeyInput = document.querySelector("#api-key");
const baseUrlInput = document.querySelector("#base-url");
const statusText = document.querySelector("#status");
const currentProvider = document.querySelector("#current-provider");
const currentBaseUrl = document.querySelector("#current-base-url");
const currentApiKey = document.querySelector("#current-api-key");
const configureButton = document.querySelector("#configure-button");
const restoreButton = document.querySelector("#restore-button");
const exitButton = document.querySelector("#exit-button");
const testButton = document.querySelector("#test-button");
const openDirButton = document.querySelector("#open-dir-button");
const toggleKeyButton = document.querySelector("#toggle-key-button");

const invoke = window.__TAURI__?.core?.invoke;

function setBusy(isBusy) {
  configureButton.disabled = isBusy;
  restoreButton.disabled = isBusy;
  exitButton.disabled = isBusy;
  testButton.disabled = isBusy;
  openDirButton.disabled = isBusy;
  toggleKeyButton.disabled = isBusy;
}

function setStatus(message, kind = "") {
  statusText.textContent = message;
  statusText.dataset.kind = kind;
  statusText.hidden = !message;
}

function normalizeBaseUrl(value) {
  const trimmed = value.trim();
  return trimmed || DEFAULT_BASE_URL;
}

function setCurrentStatus(status) {
  currentProvider.textContent = status.configured ? "已配置 OceanWay AI" : "未配置 OceanWay AI";
  currentBaseUrl.textContent = status.baseUrl || "-";
  currentBaseUrl.title = status.baseUrl || "";
  currentApiKey.textContent = status.hasApiKey ? "已保存" : "未保存";
}

async function refreshStatus() {
  if (!invoke) {
    currentProvider.textContent = "请通过 Tauri 应用打开";
    currentBaseUrl.textContent = DEFAULT_BASE_URL;
    currentApiKey.textContent = "-";
    return;
  }

  try {
    const status = await invoke("get_config_status");
    setCurrentStatus(status);
  } catch (error) {
    currentProvider.textContent = "读取失败";
    currentBaseUrl.textContent = "-";
    currentApiKey.textContent = "-";
    setStatus(`读取当前配置失败：${error}`, "error");
  }
}

async function configure(event) {
  event.preventDefault();

  const apiKey = apiKeyInput.value.trim();
  const baseUrl = normalizeBaseUrl(baseUrlInput.value);

  if (!apiKey) {
    setStatus("请先输入 API Key。", "error");
    apiKeyInput.focus();
    return;
  }

  if (!invoke) {
    setStatus("当前环境没有加载 Tauri API，请通过 Tauri 应用打开。", "error");
    return;
  }

  setBusy(true);
  setStatus("正在写入 Codex 配置...");

  try {
    const result = await invoke("configure_provider", { apiKey, baseUrl });
    setStatus(`配置完成，请重启 Codex。已写入：${result.configPath}`, "success");
    await refreshStatus();
  } catch (error) {
    setStatus(`配置失败：${error}`, "error");
  } finally {
    setBusy(false);
  }
}

async function restoreDefaults() {
  const confirmed = window.confirm("将恢复到首次使用本工具前的 Codex 配置，并备份当前文件。是否继续？");
  if (!confirmed) {
    return;
  }

  if (!invoke) {
    setStatus("当前环境没有加载 Tauri API，请通过 Tauri 应用打开。", "error");
    return;
  }

  setBusy(true);
  setStatus("正在恢复默认值...");

  try {
    const result = await invoke("restore_defaults");
    apiKeyInput.value = "";
    baseUrlInput.value = DEFAULT_BASE_URL;
    setStatus(`已恢复默认值，请重启 Codex。已写入：${result.configPath}`, "success");
    await refreshStatus();
  } catch (error) {
    setStatus(`恢复失败：${error}`, "error");
  } finally {
    setBusy(false);
  }
}

async function testCurrentConnection() {
  const apiKey = apiKeyInput.value.trim();
  const baseUrl = normalizeBaseUrl(baseUrlInput.value);

  if (!apiKey) {
    setStatus("请先输入 API Key，再测试连接。", "error");
    apiKeyInput.focus();
    return;
  }

  if (!invoke) {
    setStatus("当前环境没有加载 Tauri API，请通过 Tauri 应用打开。", "error");
    return;
  }

  setBusy(true);
  setStatus("正在测试连接...");

  try {
    const result = await invoke("test_connection", { apiKey, baseUrl });
    setStatus(`${result.message} 测试地址：${result.endpoint}`, result.ok ? "success" : "error");
  } catch (error) {
    setStatus(`测试失败：${error}`, "error");
  } finally {
    setBusy(false);
  }
}

async function openConfigDirectory() {
  if (!invoke) {
    setStatus("当前环境没有加载 Tauri API，请通过 Tauri 应用打开。", "error");
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

form.addEventListener("submit", configure);
restoreButton.addEventListener("click", restoreDefaults);
exitButton.addEventListener("click", exitApp);
testButton.addEventListener("click", testCurrentConnection);
openDirButton.addEventListener("click", openConfigDirectory);
toggleKeyButton.addEventListener("click", toggleApiKeyVisibility);
statusText.hidden = true;
refreshStatus();
