const DEFAULT_BASE_URL = "https://ocean-way.top";
const invoke = window.__TAURI__?.core?.invoke;

const $ = (selector) => document.querySelector(selector);
const configForm = $("#config-form");
const apiKeyInput = $("#api-key");
const baseUrlInput = $("#base-url");
const toggleKeyButton = $("#toggle-key-button");
const configureButton = $("#configure-button");
const testButton = $("#test-button");
const statusBox = $("#status");
const statusMessage = $("#status-message");
const savedKeyState = $("#saved-key-state");
const keyHelperText = $("#key-helper-text");
const activationState = $("#activation-state");
const nextStepTitle = $("#next-step-title");
const nextStepDetail = $("#next-step-detail");
const refreshStatusButton = $("#refresh-status-button");
const serviceStatus = $("#service-status");
const serviceDot = $("#service-dot");
const imageStatus = $("#image-status");
const imageDot = $("#image-dot");
const codexStatus = $("#codex-status");
const codexDot = $("#codex-dot");
const topbarStateText = $("#topbar-state-text");
const topbarStateDot = $("#topbar-state-dot");
const systemVersion = $("#system-version");
const codexVersion = $("#codex-version");
const runDiagnosticsButton = $("#run-diagnostics-button");
const diagnosticSummary = $("#diagnostic-summary");
const diagnosticSummaryIcon = $("#diagnostic-summary-icon");
const diagnosticSummaryTitle = $("#diagnostic-summary-title");
const diagnosticSummaryDetail = $("#diagnostic-summary-detail");
const diagnosticList = $("#diagnostic-list");
const copyReportButton = $("#copy-report-button");
const restartCodexButton = $("#restart-codex-button");
const repairButton = $("#repair-button");
const imagegenButton = $("#imagegen-button");
const imagegenRowDetail = $("#imagegen-row-detail");
const migrateHistoryButton = $("#migrate-history-button");
const historyRowDetail = $("#history-row-detail");
const openDirButton = $("#open-dir-button");
const restoreButton = $("#restore-button");
const restoreDialog = $("#restore-dialog");
const confirmRestoreButton = $("#confirm-restore-button");
const restartDialog = $("#restart-dialog");
const restartDialogDetail = $("#restart-dialog-detail");
const confirmRestartButton = $("#confirm-restart-button");
const updateButton = $("#update-button");
const updateStatus = $("#update-status");
const setupPanel = $(".setup-panel");
const sideColumn = $(".side-column");
const healthPanel = $(".health-panel");
const desktopLayoutQuery = window.matchMedia("(min-width: 761px)");

let currentStatus = {
  configured: false,
  hasApiKey: false,
  imagegenCliConfigured: false,
  chatgptLoginDetected: false,
};
let currentSystemInfo = null;
let lastDiagnosticReport = null;
let availableUpdate = null;

function syncColumnHeights() {
  if (!desktopLayoutQuery.matches) {
    sideColumn.style.removeProperty("height");
    sideColumn.style.removeProperty("--control-panel-max-height");
    return;
  }

  const setupHeight = Math.ceil(setupPanel.getBoundingClientRect().height);
  const healthHeight = Math.ceil(healthPanel.getBoundingClientRect().height);
  const columnGap = Number.parseFloat(getComputedStyle(sideColumn).gap) || 14;
  const controlMaxHeight = Math.max(0, setupHeight - healthHeight - columnGap);
  sideColumn.style.height = `${setupHeight}px`;
  sideColumn.style.setProperty("--control-panel-max-height", `${controlMaxHeight}px`);
}

const layoutResizeObserver = new ResizeObserver(syncColumnHeights);
layoutResizeObserver.observe(setupPanel);
layoutResizeObserver.observe(healthPanel);
desktopLayoutQuery.addEventListener("change", syncColumnHeights);

function setDot(element, kind) {
  element.dataset.kind = kind || "muted";
}

function setStatus(message, kind = "") {
  statusMessage.textContent = message;
  statusBox.dataset.kind = kind;
  statusBox.hidden = !message;
}

function setButtonBusy(button, busy, busyText) {
  if (!button.dataset.label) {
    button.dataset.label = button.innerHTML;
  }
  button.disabled = busy;
  if (busy) {
    button.textContent = busyText;
  } else {
    button.innerHTML = button.dataset.label;
  }
}

function normalizeBaseUrl(value) {
  return value.trim() || DEFAULT_BASE_URL;
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

function authStrategyText(strategy) {
  return strategy === "chatgptBearerToken"
    ? "已保留 ChatGPT 登录态，并更新 OceanWay Provider。"
    : "已保存 API Key，并启用 Codex Desktop 图片工具兼容配置。";
}

function updateProgress(status) {
  const steps = [
    $("#step-credential"),
    $("#step-config"),
    $("#step-restart"),
    $("#step-verify"),
  ];
  steps.forEach((step) => step.classList.remove("is-current", "is-done"));
  if (!status.hasApiKey) {
    steps[0].classList.add("is-current");
    activationState.textContent = "等待连接";
    activationState.dataset.kind = "warning";
    nextStepTitle.textContent = "输入 API Key";
    nextStepDetail.textContent = "首次使用需要填写 OceanWay API Key，然后点击“完成配置”。";
    return;
  }
  steps[0].classList.add("is-done");
  if (!status.configured || !status.imagegenCliConfigured) {
    steps[1].classList.add("is-current");
    activationState.textContent = status.configured ? "图片能力待同步" : "配置未完成";
    activationState.dataset.kind = "warning";
    nextStepTitle.textContent = "点击“完成配置”";
    nextStepDetail.textContent = "软件会保留已有 Codex 设置，并补全 Provider 与图片工具配置。";
    return;
  }
  steps[1].classList.add("is-done");
  steps[2].classList.add("is-current");
  activationState.textContent = "配置已写入";
  activationState.dataset.kind = "success";
  nextStepTitle.textContent = "重启 Codex，然后新建任务";
  nextStepDetail.textContent = "旧任务不会刷新工具列表；必须在重启后创建新任务再测试图片生成。";
}

function renderConfigStatus(status) {
  currentStatus = status;
  const ready = status.configured && status.imagegenCliConfigured;

  serviceStatus.textContent = status.configured ? "已配置" : "未配置";
  setDot(serviceDot, status.configured ? "success" : "warning");
  imageStatus.textContent = status.imagegenCliConfigured ? "已就绪" : "待同步";
  setDot(imageDot, status.imagegenCliConfigured ? "success" : "warning");

  savedKeyState.hidden = !status.hasApiKey;
  apiKeyInput.required = !status.hasApiKey;
  apiKeyInput.placeholder = status.hasApiKey
    ? "已保存；留空继续使用，输入新 Key 可替换"
    : "请输入 OceanWay API Key";
  keyHelperText.textContent = status.hasApiKey
    ? "Key 已保存在本机。留空继续使用，界面不会回显完整内容。"
    : "Key 仅保存到本机 Codex 配置，不会在界面回显完整内容。";

  if (status.baseUrl && document.activeElement !== baseUrlInput) {
    baseUrlInput.value = status.baseUrl;
  }

  imagegenButton.textContent = status.imagegenCliConfigured ? "重新同步" : "同步";
  imagegenRowDetail.textContent = status.imagegenCliConfigured
    ? "已同步；内置工具不可用时可使用 CLI 备用路径。"
    : "尚未同步；完成主配置时会自动处理。";

  topbarStateText.textContent = ready
    ? "核心配置已就绪"
    : status.configured
      ? "图片能力待同步"
      : "等待完成配置";
  setDot(topbarStateDot, ready ? "success" : "warning");
  updateProgress(status);
}

function renderSystemInfo(info) {
  currentSystemInfo = info;
  const systemName = info.operatingSystem || info.osName || "";
  const systemRelease = info.operatingSystemVersion || info.osVersion || "";
  systemVersion.textContent = [systemName, systemRelease].filter(Boolean).join(" ") || "未知";
  codexVersion.textContent = info.codexDesktopVersion || info.codexCliVersion || info.codexVersion || "未检测到";
  codexStatus.textContent = info.codexRunning
    ? info.codexHost === "ChatGPT"
      ? "ChatGPT 内运行"
      : "正在运行"
    : "未运行";
  setDot(codexDot, info.codexRunning ? "success" : "muted");
}

function browserPreviewStatus() {
  return {
    configured: true,
    hasApiKey: true,
    chatgptLoginDetected: true,
    chatgptAccountLabel: "浏览器预览",
    imagegenCliConfigured: true,
    baseUrl: DEFAULT_BASE_URL,
  };
}

async function refreshStatus() {
  refreshStatusButton.disabled = true;
  try {
    if (!invoke) {
      renderConfigStatus(browserPreviewStatus());
      renderSystemInfo({
        osName: "macOS",
        osVersion: "macOS 15.5",
        codexVersion: "0.143.0",
        codexHost: "ChatGPT",
        codexRunning: true,
      });
      return;
    }

    const [status, info] = await Promise.all([
      invoke("get_config_status"),
      invoke("get_system_info"),
    ]);
    renderConfigStatus(status);
    renderSystemInfo(info);
  } catch (error) {
    serviceStatus.textContent = "读取失败";
    imageStatus.textContent = "未知";
    codexStatus.textContent = "未知";
    [serviceDot, imageDot, codexDot, topbarStateDot].forEach((dot) => setDot(dot, "error"));
    topbarStateText.textContent = "本机状态读取失败";
    setStatus(`读取本机状态失败：${error}`, "error");
  } finally {
    refreshStatusButton.disabled = false;
  }
}

async function configureProvider(event) {
  event.preventDefault();
  const values = readFormValues();
  if (!values) return;
  if (!invoke) {
    renderConfigStatus({ ...browserPreviewStatus(), baseUrl: values.baseUrl });
    setStatus("界面预览：配置写入与图片能力同步流程已完成。", "success");
    return;
  }

  setButtonBusy(configureButton, true, "正在配置…");
  setStatus(values.apiKey ? "正在保存并配置 OceanWay AI…" : "正在使用已保存的 Key 更新配置…");
  try {
    const result = await invoke("configure_provider", values);
    apiKeyInput.value = "";
    apiKeyInput.type = "password";
    await refreshStatus();
    setStatus(
      `${authStrategyText(result.authStrategy)} 请重启 Codex 并新建任务验证图片能力。`,
      "success",
    );
  } catch (error) {
    setStatus(`配置失败：${error}`, "error");
  } finally {
    setButtonBusy(configureButton, false);
  }
}

async function testConnection() {
  const values = readFormValues();
  if (!values) return;
  setButtonBusy(testButton, true, "测试中…");
  setStatus(currentStatus.hasApiKey && !values.apiKey ? "正在使用已保存的 Key 测试连接…" : "正在测试连接…");
  try {
    if (!invoke) {
      await new Promise((resolve) => window.setTimeout(resolve, 350));
      setStatus(`连接测试通过。服务地址：${values.baseUrl}`, "success");
      return;
    }
    const result = await invoke("test_connection", values);
    setStatus(`${result.message} 测试地址：${result.endpoint}`, result.ok ? "success" : "error");
  } catch (error) {
    setStatus(`测试失败：${error}`, "error");
  } finally {
    setButtonBusy(testButton, false);
  }
}

function diagnosticGlyph(kind) {
  if (kind === "pass" || kind === "success") return "✓";
  if (kind === "warning") return "!";
  return "×";
}

function renderDiagnosticReport(report) {
  lastDiagnosticReport = report;
  diagnosticList.innerHTML = "";
  for (const check of report.checks || []) {
    const item = document.createElement("li");
    item.dataset.kind = check.status === "pass" ? "success" : check.status;
    const state = document.createElement("span");
    state.className = "check-state";
    state.textContent = diagnosticGlyph(check.status);
    const content = document.createElement("div");
    const title = document.createElement("strong");
    const detail = document.createElement("p");
    title.textContent = check.label;
    detail.textContent = check.detail;
    content.append(title, detail);
    item.append(state, content);
    diagnosticList.append(item);
  }

  const passed = report.passed ?? report.passedCount ?? 0;
  const failed = report.errors ?? report.errorCount ?? 0;
  const warnings = report.warnings ?? report.warningCount ?? 0;
  const kind = failed ? "error" : warnings ? "warning" : "success";
  diagnosticSummary.dataset.kind = kind;
  diagnosticSummary.hidden = false;
  diagnosticSummaryIcon.textContent = diagnosticGlyph(kind);
  diagnosticSummaryTitle.textContent = failed
    ? `发现 ${failed} 项异常`
    : warnings
      ? `诊断完成，${warnings} 项需留意`
      : "全部核心检查通过";
  diagnosticSummaryDetail.textContent = `${passed} 项通过 · ${warnings} 项提醒 · ${failed} 项异常`;
  copyReportButton.disabled = false;
}

function previewDiagnosticReport() {
  return {
    passedCount: 6,
    warningCount: 1,
    errorCount: 0,
    checks: [
      { label: "Provider 配置", status: "success", detail: "OceanWay Provider 已写入并设为当前渠道。" },
      { label: "API 凭据", status: "success", detail: "已检测到本机保存的凭据，报告不会包含完整 Key。" },
      { label: "图片工具兼容", status: "success", detail: "内置图片工具与 CLI 备用配置已同步。" },
      { label: "Codex 版本", status: "success", detail: "当前版本满足图片扩展最低要求。" },
      { label: "服务连通性", status: "success", detail: "OceanWay 服务连接正常。" },
      { label: "Codex 进程", status: "success", detail: "Codex Desktop 正在运行。" },
      { label: "配置备份", status: "warning", detail: "这是浏览器预览；桌面端会显示真实快照时间。" },
    ],
  };
}

async function runDiagnostics() {
  setButtonBusy(runDiagnosticsButton, true, "诊断中…");
  diagnosticSummary.hidden = true;
  diagnosticList.innerHTML = '<li class="diagnostic-placeholder"><span class="check-state">…</span><div><strong>正在逐层检查</strong><p>配置、版本、连接、进程与备份状态。</p></div></li>';
  try {
    const report = invoke ? await invoke("run_diagnostics") : previewDiagnosticReport();
    renderDiagnosticReport(report);
  } catch (error) {
    diagnosticList.innerHTML = "";
    renderDiagnosticReport({
      passedCount: 0,
      warningCount: 0,
      errorCount: 1,
      checks: [{ label: "诊断执行失败", status: "error", detail: String(error) }],
    });
  } finally {
    setButtonBusy(runDiagnosticsButton, false);
  }
}

async function copySupportReport() {
  if (!lastDiagnosticReport) return;
  try {
    if (invoke) {
      await invoke("copy_support_report");
    } else {
      await navigator.clipboard?.writeText(
        `OceanWay Codex Config v1.3.0\n诊断通过 ${lastDiagnosticReport.passed ?? lastDiagnosticReport.passedCount ?? 0} 项\n敏感凭据：已脱敏`,
      );
    }
    setStatus("脱敏诊断报告已复制，不包含完整 API Key 或访问令牌。", "success");
  } catch (error) {
    setStatus(`复制报告失败：${error}`, "error");
  }
}

async function repairConfiguration() {
  setButtonBusy(repairButton, true, "修复中…");
  setStatus("正在使用已保存信息检查并补全配置…");
  try {
    if (!invoke) {
      await new Promise((resolve) => window.setTimeout(resolve, 300));
      setStatus("界面预览：配置修复完成。", "success");
      return;
    }
    const result = await invoke("repair_configuration");
    await refreshStatus();
    setStatus(result.message, "success");
  } catch (error) {
    setStatus(`配置修复失败：${error}`, "error");
  } finally {
    setButtonBusy(repairButton, false);
  }
}

async function configureImagegenCli() {
  setButtonBusy(imagegenButton, true, "同步中…");
  setStatus("正在同步图片备用配置…");
  try {
    if (invoke) await invoke("configure_imagegen_cli");
    await refreshStatus();
    setStatus("图片备用配置已同步。请重启 Codex 并新建任务。", "success");
  } catch (error) {
    setStatus(`图片备用配置失败：${error}`, "error");
  } finally {
    setButtonBusy(imagegenButton, false);
  }
}

function openRestartDialog() {
  restartDialogDetail.textContent = currentSystemInfo?.codexHost === "ChatGPT"
    ? "当前 Codex 运行在 ChatGPT 内。继续后 ChatGPT 将退出并重新打开，当前任务会中断，请先确认工作已保存。"
    : "Codex 将先正常退出再重新打开。未提交的本地终端输入可能丢失，请确认当前任务已保存。";
  restartDialog.showModal?.();
}

async function restartCodex() {
  restartDialog.close();
  setButtonBusy(restartCodexButton, true, "重启中…");
  setStatus("正在退出并重新启动 Codex…");
  try {
    if (!invoke) {
      await new Promise((resolve) => window.setTimeout(resolve, 350));
      setStatus("界面预览：Codex 重启流程已完成。", "success");
      return;
    }
    const result = await invoke("restart_codex");
    setStatus(result.message, result.restarted ? "success" : "error");
    window.setTimeout(refreshStatus, 1200);
  } catch (error) {
    setStatus(`重启 Codex 失败：${error}`, "error");
  } finally {
    setButtonBusy(restartCodexButton, false);
  }
}

function historyProviderCountText(providerCounts = []) {
  return providerCounts.map((item) => `${item.provider}: ${item.files}`).join("，") || "无";
}

async function refreshHistoryStatus() {
  if (!invoke) {
    historyRowDetail.textContent = "历史记录已与当前 Provider 一致。";
    return;
  }
  try {
    const status = await invoke("get_history_migration_status");
    if (!status.migrationSupported) {
      historyRowDetail.textContent = "完成 OceanWay 配置后可检测旧任务。";
    } else {
      historyRowDetail.textContent = status.needsMigration
        ? `检测到 ${status.rolloutFilesToUpdate} 个会话文件需要迁移。`
        : "历史记录已与当前 Provider 一致，无需迁移。";
    }
  } catch {
    historyRowDetail.textContent = "无法读取历史状态，请稍后重试。";
  }
}

async function migrateHistoryVisibility() {
  if (!invoke) {
    setStatus("界面预览：历史记录无需迁移。", "success");
    return;
  }
  setButtonBusy(migrateHistoryButton, true, "扫描中…");
  setStatus("正在扫描历史会话…");
  try {
    const status = await invoke("get_history_migration_status");
    if (!status.migrationSupported) {
      setStatus("请先完成 OceanWay 配置，再迁移历史记录。", "error");
      return;
    }
    if (!status.needsMigration) {
      setStatus("历史会话已与当前 Provider 一致，无需迁移。", "success");
      return;
    }
    const warning = status.encryptedContentFiles
      ? `\n其中 ${status.encryptedContentFiles} 个会话包含加密内容；迁移只修复列表可见性。`
      : "";
    const confirmed = window.confirm(
      `将迁移 ${status.rolloutFilesToUpdate} 个会话文件和 ${status.sqliteRowsToUpdate} 行索引。\n当前分布：${historyProviderCountText(status.providerCounts)}。${warning}\n\n执行前会自动备份，是否继续？`,
    );
    if (!confirmed) {
      setStatus("已取消历史迁移。");
      return;
    }
    const result = await invoke("migrate_history_visibility");
    setStatus(
      `历史迁移完成：${result.changedSessionFiles} 个会话文件，${result.sqliteRowsUpdated} 行索引。`,
      "success",
    );
    await refreshHistoryStatus();
  } catch (error) {
    setStatus(`历史迁移失败：${error}`, "error");
  } finally {
    setButtonBusy(migrateHistoryButton, false);
  }
}

async function openConfigDirectory() {
  try {
    if (invoke) await invoke("open_config_dir");
    setStatus(invoke ? "已打开 Codex 配置目录。" : "界面预览：桌面端会打开 Codex 配置目录。");
  } catch (error) {
    setStatus(`打开配置目录失败：${error}`, "error");
  }
}

function openRestoreDialog() {
  restoreDialog.showModal?.();
}

async function restoreDefaults() {
  restoreDialog.close();
  setButtonBusy(restoreButton, true, "恢复中…");
  setStatus("正在恢复首次使用本工具前的 Codex 配置…");
  try {
    if (!invoke) {
      renderConfigStatus({
        ...browserPreviewStatus(),
        configured: false,
        hasApiKey: false,
        imagegenCliConfigured: false,
      });
      setStatus("界面预览：默认配置恢复完成。", "success");
      return;
    }
    const result = await invoke("restore_defaults");
    apiKeyInput.value = "";
    await refreshStatus();
    const restored = result.historyMigrationRestore;
    const historyText = restored?.restoredBackups
      ? ` 同时撤销 ${restored.restoredSessionFiles} 个历史文件和 ${restored.sqliteRowsRestored} 行索引迁移。`
      : "";
    setStatus(`已恢复默认配置。${historyText} 请重启 Codex。`, "success");
  } catch (error) {
    setStatus(`恢复失败：${error}`, "error");
  } finally {
    setButtonBusy(restoreButton, false);
  }
}

function setTab(button) {
  for (const tab of document.querySelectorAll(".tab-button")) {
    const active = tab === button;
    tab.classList.toggle("is-active", active);
    tab.setAttribute("aria-selected", String(active));
    const panel = document.getElementById(tab.getAttribute("aria-controls"));
    panel.hidden = !active;
  }
  if (button.id === "tools-tab") refreshHistoryStatus();
  window.requestAnimationFrame(syncColumnHeights);
}

function toggleApiKeyVisibility() {
  if (!apiKeyInput.value) {
    apiKeyInput.focus();
    return;
  }
  const show = apiKeyInput.type === "password";
  apiKeyInput.type = show ? "text" : "password";
  toggleKeyButton.title = show ? "隐藏 API Key" : "显示 API Key";
  toggleKeyButton.setAttribute("aria-label", toggleKeyButton.title);
}

async function handleUpdate(options = {}) {
  const silent = options?.silent === true;
  if (!invoke) {
    updateStatus.textContent = "当前已是最新版本";
    return;
  }
  setButtonBusy(updateButton, true, availableUpdate ? "安装中…" : "检查中…");
  try {
    if (availableUpdate) {
      updateStatus.textContent = `正在安装 v${availableUpdate.latestVersion}，完成后自动重启…`;
      await invoke("install_update");
      return;
    }
    const result = await invoke("check_for_updates");
    if (result.available) {
      availableUpdate = result;
      updateStatus.textContent = `发现 v${result.latestVersion}`;
      updateButton.dataset.label = `安装 v${result.latestVersion}`;
      updateButton.textContent = updateButton.dataset.label;
    } else {
      updateStatus.textContent = "当前已是最新版本";
    }
  } catch (error) {
    updateStatus.textContent = String(error).includes("404")
      ? "更新通道尚未发布"
      : "检查更新失败";
    if (!silent) {
      setStatus(`自动更新暂不可用：${error}`, "error");
    }
  } finally {
    setButtonBusy(updateButton, false);
  }
}

configForm.addEventListener("submit", configureProvider);
testButton.addEventListener("click", testConnection);
toggleKeyButton.addEventListener("click", toggleApiKeyVisibility);
refreshStatusButton.addEventListener("click", refreshStatus);
runDiagnosticsButton.addEventListener("click", runDiagnostics);
copyReportButton.addEventListener("click", copySupportReport);
restartCodexButton.addEventListener("click", openRestartDialog);
confirmRestartButton.addEventListener("click", restartCodex);
repairButton.addEventListener("click", repairConfiguration);
imagegenButton.addEventListener("click", configureImagegenCli);
migrateHistoryButton.addEventListener("click", migrateHistoryVisibility);
openDirButton.addEventListener("click", openConfigDirectory);
restoreButton.addEventListener("click", openRestoreDialog);
confirmRestoreButton.addEventListener("click", restoreDefaults);
updateButton.addEventListener("click", () => handleUpdate());
for (const tab of document.querySelectorAll(".tab-button")) {
  tab.addEventListener("click", () => setTab(tab));
}

syncColumnHeights();
await refreshStatus();
if (invoke) {
  window.setTimeout(() => handleUpdate({ silent: true }), 900);
}
