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
const keyProfileSelect = document.querySelector("#key-profile-select");
const keyProfileNameInput = document.querySelector("#key-profile-name");
const saveKeyProfileButton = document.querySelector("#save-key-profile-button");
const useKeyProfileButton = document.querySelector("#use-key-profile-button");
const deleteKeyProfileButton = document.querySelector("#delete-key-profile-button");

const invoke = window.__TAURI__?.core?.invoke;

function setBusy(isBusy) {
  configureButton.disabled = isBusy;
  restoreButton.disabled = isBusy;
  exitButton.disabled = isBusy;
  testButton.disabled = isBusy;
  openDirButton.disabled = isBusy;
  toggleKeyButton.disabled = isBusy;
  keyProfileSelect.disabled = isBusy;
  keyProfileNameInput.disabled = isBusy;
  saveKeyProfileButton.disabled = isBusy;
  useKeyProfileButton.disabled = isBusy;
  deleteKeyProfileButton.disabled = isBusy;
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

function sessionSyncText(sync = {}) {
  const synced = (sync.rolloutFilesUpdated || 0) + (sync.sqliteRowsUpdated || 0);
  const warningText = sync.warnings?.length ? `，有 ${sync.warnings.length} 个同步提示` : "";
  return synced > 0 ? `已同步历史记录${warningText}。` : `历史记录无需同步${warningText}。`;
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

async function refreshKeyProfiles(selectedId = "") {
  if (!invoke) {
    return;
  }

  try {
    const profiles = await invoke("list_key_profiles");
    keyProfileSelect.replaceChildren();
    keyProfileSelect.append(new Option("选择已保存密钥", ""));
    for (const profile of profiles) {
      const option = new Option(`${profile.name} (${profile.maskedKey})`, profile.id);
      option.dataset.name = profile.name;
      keyProfileSelect.append(option);
    }
    keyProfileSelect.value = selectedId;
    if (!keyProfileSelect.value && selectedId) {
      keyProfileSelect.value = "";
    }
  } catch (error) {
    setStatus(`读取密钥档案失败：${error}`, "error");
  }
}

function selectedKeyProfileId() {
  return keyProfileSelect.value;
}

async function saveKeyProfile() {
  const name = keyProfileNameInput.value.trim();
  const apiKey = apiKeyInput.value.trim();

  if (!name) {
    setStatus("请先填写密钥名称。", "error");
    keyProfileNameInput.focus();
    return;
  }

  if (!apiKey) {
    setStatus("请先输入要保存的 API Key。", "error");
    apiKeyInput.focus();
    return;
  }

  if (!invoke) {
    setStatus("当前环境没有加载 Tauri API，请通过 Tauri 应用打开。", "error");
    return;
  }

  setBusy(true);
  try {
    const profile = await invoke("save_key_profile", { name, apiKey });
    await refreshKeyProfiles(profile.id);
    setStatus(`已保存密钥档案：${profile.name}`, "success");
  } catch (error) {
    setStatus(`保存密钥失败：${error}`, "error");
  } finally {
    setBusy(false);
  }
}

async function useKeyProfile() {
  const profileId = selectedKeyProfileId();
  const baseUrl = normalizeBaseUrl(baseUrlInput.value);

  if (!profileId) {
    setStatus("请先选择一个密钥档案。", "error");
    keyProfileSelect.focus();
    return;
  }

  if (!invoke) {
    setStatus("当前环境没有加载 Tauri API，请通过 Tauri 应用打开。", "error");
    return;
  }

  setBusy(true);
  setStatus("正在使用选中密钥配置 Codex...");

  try {
    const result = await invoke("configure_with_key_profile", { profileId, baseUrl });
    setStatus(`配置完成，请重启 Codex。已写入：${result.configPath}。${sessionSyncText(result.sessionSync)}`, "success");
    await refreshStatus();
  } catch (error) {
    setStatus(`配置失败：${error}`, "error");
  } finally {
    setBusy(false);
  }
}

async function deleteKeyProfile() {
  const profileId = selectedKeyProfileId();
  const selectedName = keyProfileSelect.selectedOptions[0]?.dataset.name || "选中的密钥";

  if (!profileId) {
    setStatus("请先选择要删除的密钥档案。", "error");
    keyProfileSelect.focus();
    return;
  }

  const confirmed = window.confirm(`将删除密钥档案“${selectedName}”，是否继续？`);
  if (!confirmed) {
    return;
  }

  setBusy(true);
  try {
    await invoke("delete_key_profile", { profileId });
    keyProfileNameInput.value = "";
    await refreshKeyProfiles();
    setStatus(`已删除密钥档案：${selectedName}`, "success");
  } catch (error) {
    setStatus(`删除密钥失败：${error}`, "error");
  } finally {
    setBusy(false);
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
    setStatus(`配置完成，请重启 Codex。已写入：${result.configPath}。${sessionSyncText(result.sessionSync)}`, "success");
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
    setStatus(`已恢复默认值，请重启 Codex。已写入：${result.configPath}。${sessionSyncText(result.sessionSync)}`, "success");
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
saveKeyProfileButton.addEventListener("click", saveKeyProfile);
useKeyProfileButton.addEventListener("click", useKeyProfile);
deleteKeyProfileButton.addEventListener("click", deleteKeyProfile);
keyProfileSelect.addEventListener("change", () => {
  const selectedName = keyProfileSelect.selectedOptions[0]?.dataset.name || "";
  keyProfileNameInput.value = selectedName;
});
statusText.hidden = true;
refreshStatus();
refreshKeyProfiles();
