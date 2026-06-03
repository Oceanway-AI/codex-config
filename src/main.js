const DEFAULT_BASE_URL = "https://ocean-way.top";

const currentProvider = document.querySelector("#current-provider");
const currentBaseUrl = document.querySelector("#current-base-url");
const currentApiKey = document.querySelector("#current-api-key");
const currentKeyProfile = document.querySelector("#current-key-profile");
const profileCount = document.querySelector("#profile-count");
const profileList = document.querySelector("#profile-list");
const emptyState = document.querySelector("#empty-state");
const emptyAddKeyButton = document.querySelector("#empty-add-key-button");
const statusText = document.querySelector("#status");
const addKeyButton = document.querySelector("#add-key-button");
const restoreButton = document.querySelector("#restore-button");
const migrateHistoryButton = document.querySelector("#migrate-history-button");
const exitButton = document.querySelector("#exit-button");
const openDirButton = document.querySelector("#open-dir-button");
const keyDialog = document.querySelector("#key-dialog");
const keyForm = document.querySelector("#key-form");
const closeDialogButton = document.querySelector("#close-dialog-button");
const dialogTitle = document.querySelector("#dialog-title");
const keyProfileIdInput = document.querySelector("#key-profile-id");
const keyProfileNameInput = document.querySelector("#key-profile-name");
const apiKeyInput = document.querySelector("#api-key");
const baseUrlInput = document.querySelector("#base-url");
const toggleKeyButton = document.querySelector("#toggle-key-button");
const saveKeyProfileButton = document.querySelector("#save-key-profile-button");
const saveAndUseButton = document.querySelector("#save-and-use-button");

const invoke = window.__TAURI__?.core?.invoke;
let keyProfiles = [];
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

function setCurrentStatus(status) {
  currentProvider.textContent = status.configured ? "已配置 OceanWay AI" : "未配置 OceanWay AI";
  currentBaseUrl.textContent = status.baseUrl || "-";
  currentBaseUrl.title = status.baseUrl || "";
  if (status.authStrategy === "chatgptBearerToken") {
    currentApiKey.textContent = "登录态 + Provider Token";
  } else {
    currentApiKey.textContent = status.hasApiKey ? "API Key 已保存" : "未保存";
  }
  updateActiveProfile();
}

function updateActiveProfile() {
  const activeProfile = keyProfiles.find((profile) => profile.active);
  currentKeyProfile.textContent = activeProfile?.name || (currentProvider.textContent === "已配置 OceanWay AI" ? "未匹配保存档案" : "-");
}

function renderProfiles() {
  profileList.replaceChildren();
  profileCount.textContent = `${keyProfiles.length} 个密钥`;
  emptyState.hidden = keyProfiles.length > 0;

  for (const profile of keyProfiles) {
    const card = document.createElement("article");
    card.className = "profile-card";
    card.dataset.active = String(Boolean(profile.active));
    card.innerHTML = `
      <div class="profile-main">
        <div>
          <div class="profile-title-row">
            <h3></h3>
            <span class="active-pill">当前使用中</span>
          </div>
          <span class="masked-key"></span>
        </div>
        <button type="button" class="primary activate-button" data-action="activate"></button>
      </div>
      <div class="profile-meta">
        <span>Base URL</span>
        <strong></strong>
      </div>
      <div class="profile-actions">
        <button type="button" data-action="test">
          <svg class="button-icon" viewBox="0 0 24 24" aria-hidden="true">
            <path d="M13 2 4 14h7l-1 8 10-13h-7l1-7Z" />
          </svg>
          测试
        </button>
        <button type="button" data-action="edit">
          <svg class="button-icon" viewBox="0 0 24 24" aria-hidden="true">
            <path d="M12 20h9" />
            <path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4Z" />
          </svg>
          编辑
        </button>
        <button type="button" data-action="delete">
          <svg class="button-icon" viewBox="0 0 24 24" aria-hidden="true">
            <path d="M3 6h18" />
            <path d="M8 6V4h8v2" />
            <path d="M6 6l1 15h10l1-15" />
          </svg>
          删除
        </button>
      </div>
    `;

    card.querySelector("h3").textContent = profile.name;
    card.querySelector(".masked-key").textContent = profile.maskedKey;
    card.querySelector(".profile-meta strong").textContent = profile.baseUrl || DEFAULT_BASE_URL;
    card.querySelector(".profile-meta strong").title = profile.baseUrl || DEFAULT_BASE_URL;
    const activateButton = card.querySelector("[data-action='activate']");
    activateButton.textContent = profile.active ? "已启用" : "启用";
    activateButton.disabled = Boolean(profile.active);
    activateButton.classList.toggle("is-active", Boolean(profile.active));
    card.addEventListener("click", (event) => handleProfileAction(event, profile));
    profileList.append(card);
  }

  updateActiveProfile();
  scheduleWindowResize();
}

async function refreshStatus() {
  if (!invoke) {
    currentProvider.textContent = "预览模式";
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

async function refreshKeyProfiles() {
  if (!invoke) {
    keyProfiles = [];
    renderProfiles();
    return;
  }

  try {
    keyProfiles = await invoke("list_key_profiles");
    renderProfiles();
  } catch (error) {
    setStatus(`读取密钥档案失败：${error}`, "error");
  }
}

function openKeyDialog(profile = null) {
  keyProfileIdInput.value = profile?.id || "";
  keyProfileNameInput.value = profile?.name || "";
  apiKeyInput.value = "";
  apiKeyInput.placeholder = profile ? "留空则保留已保存的 API Key" : "请输入 API Key";
  baseUrlInput.value = profile?.baseUrl || DEFAULT_BASE_URL;
  dialogTitle.textContent = profile ? "编辑密钥" : "添加密钥";
  keyDialog.showModal();
  keyProfileNameInput.focus();
  scheduleWindowResize();
}

function closeKeyDialog() {
  keyDialog.close();
  apiKeyInput.type = "password";
  scheduleWindowResize();
}

async function saveKeyProfile() {
  const profileId = keyProfileIdInput.value || null;
  const name = keyProfileNameInput.value.trim();
  const apiKey = apiKeyInput.value.trim();
  const baseUrl = normalizeBaseUrl(baseUrlInput.value);

  if (!name) {
    setStatus("请先填写密钥名称。", "error");
    keyProfileNameInput.focus();
    return null;
  }

  if (!apiKey && !profileId) {
    setStatus("请先输入 API Key。", "error");
    apiKeyInput.focus();
    return null;
  }

  if (!invoke) {
    const profile = {
      id: profileId || `preview-${Date.now()}`,
      name,
      maskedKey: apiKey ? `${apiKey.slice(0, 6)}...${apiKey.slice(-4)}` : "sk-new...demo",
      baseUrl,
      active: false,
    };
    keyProfiles = profileId
      ? keyProfiles.map((item) => (item.id === profileId ? { ...item, ...profile } : item))
      : [profile, ...keyProfiles];
    renderProfiles();
    return profile;
  }

  const profile = await invoke("save_key_profile", { profileId, name, apiKey, baseUrl });
  await refreshKeyProfiles();
  return profile;
}

async function handleSave(event) {
  event.preventDefault();
  setBusy(true);
  try {
    const profile = await saveKeyProfile();
    if (profile) {
      closeKeyDialog();
      setStatus(`已保存密钥档案：${profile.name}`, "success");
    }
  } catch (error) {
    setStatus(`保存密钥失败：${error}`, "error");
  } finally {
    setBusy(false);
  }
}

async function handleSaveAndUse() {
  setBusy(true);
  try {
    const profile = await saveKeyProfile();
    if (profile) {
      closeKeyDialog();
      await activateProfile(profile.id);
    }
  } catch (error) {
    setStatus(`保存密钥失败：${error}`, "error");
  } finally {
    setBusy(false);
  }
}

async function activateProfile(profileId) {
  const profile = keyProfiles.find((item) => item.id === profileId);
  if (!profile) {
    setStatus("未找到选中的密钥档案。", "error");
    return;
  }

  if (!invoke) {
    keyProfiles = keyProfiles.map((item) => ({ ...item, active: item.id === profileId }));
    renderProfiles();
    setStatus(`已启用预览档案：${profile.name}`, "success");
    return;
  }

  setBusy(true);
  setStatus(`正在启用 ${profile.name}...`);
  try {
    const result = await invoke("configure_with_key_profile", { profileId });
    await refreshKeyProfiles();
    await refreshStatus();
    const authText = result.authStrategy === "chatgptBearerToken"
      ? "已保留 ChatGPT 登录态，并使用 provider token。"
      : "已写入 API Key。";
    setStatus(`已启用 ${profile.name}，${authText}请重启 Codex。`, "success");
  } catch (error) {
    setStatus(`启用失败：${error}`, "error");
  } finally {
    setBusy(false);
  }
}

async function testProfile(profileId) {
  const profile = keyProfiles.find((item) => item.id === profileId);
  if (!profile) {
    setStatus("未找到选中的密钥档案。", "error");
    return;
  }

  if (!invoke) {
    setStatus(`${profile.name} 预览测试通过。测试地址：${profile.baseUrl}`, "success");
    return;
  }

  setBusy(true);
  setStatus(`正在测试 ${profile.name}...`);
  try {
    const result = await invoke("test_key_profile", { profileId });
    setStatus(`${profile.name}：${result.message} 测试地址：${result.endpoint}`, result.ok ? "success" : "error");
  } catch (error) {
    setStatus(`测试失败：${error}`, "error");
  } finally {
    setBusy(false);
  }
}

async function deleteProfile(profileId) {
  const profile = keyProfiles.find((item) => item.id === profileId);
  if (!profile) {
    setStatus("未找到选中的密钥档案。", "error");
    return;
  }

  const deleteMessage = profile.active
    ? `“${profile.name}”当前正在使用。删除后会同时清除 Codex 当前 OceanWay 密钥配置。是否继续？`
    : `将删除密钥档案“${profile.name}”，是否继续？`;
  const confirmed = window.confirm(deleteMessage);
  if (!confirmed) {
    return;
  }

  if (!invoke) {
    keyProfiles = keyProfiles.filter((item) => item.id !== profileId);
    renderProfiles();
    setStatus(`已删除预览档案：${profile.name}`, "success");
    return;
  }

  setBusy(true);
  try {
    await invoke("delete_key_profile", { profileId });
    await refreshKeyProfiles();
    await refreshStatus();
    setStatus(`已删除密钥档案：${profile.name}`, "success");
  } catch (error) {
    setStatus(`删除密钥失败：${error}`, "error");
  } finally {
    setBusy(false);
  }
}

function handleProfileAction(event, profile) {
  const button = event.target.closest("button[data-action]");
  if (!button) {
    return;
  }

  const action = button.dataset.action;
  if (action === "activate") {
    activateProfile(profile.id);
  } else if (action === "test") {
    testProfile(profile.id);
  } else if (action === "edit") {
    openKeyDialog(profile);
  } else if (action === "delete") {
    deleteProfile(profile.id);
  }
}

async function restoreDefaults() {
  const confirmed = window.confirm("将恢复到首次使用本工具前的 Codex 配置，并撤销本工具记录过的历史迁移。是否继续？");
  if (!confirmed) {
    return;
  }

  if (!invoke) {
    keyProfiles = keyProfiles.map((profile) => ({ ...profile, active: false }));
    renderProfiles();
    setStatus("浏览器预览已恢复默认状态。", "success");
    return;
  }

  setBusy(true);
  setStatus("正在恢复默认值...");

  try {
    const result = await invoke("restore_defaults");
    await refreshKeyProfiles();
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

addKeyButton.addEventListener("click", () => openKeyDialog());
emptyAddKeyButton.addEventListener("click", () => openKeyDialog());
closeDialogButton.addEventListener("click", closeKeyDialog);
keyForm.addEventListener("submit", handleSave);
saveAndUseButton.addEventListener("click", handleSaveAndUse);
restoreButton.addEventListener("click", restoreDefaults);
migrateHistoryButton.addEventListener("click", migrateHistoryVisibility);
exitButton.addEventListener("click", exitApp);
openDirButton.addEventListener("click", openConfigDirectory);
toggleKeyButton.addEventListener("click", toggleApiKeyVisibility);
statusText.hidden = true;
await refreshKeyProfiles();
await refreshStatus();
window.addEventListener("resize", scheduleWindowResize);
scheduleWindowResize();
