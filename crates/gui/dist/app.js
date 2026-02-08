const TOKENS = [
  { token: "{year}", label: "年" },
  { token: "{month}", label: "月" },
  { token: "{day}", label: "日" },
  { token: "{hour}", label: "時" },
  { token: "{minute}", label: "分" },
  { token: "{second}", label: "秒" },
  { token: "{camera_maker}", label: "カメラメーカー名" },
  { token: "{camera_model}", label: "カメラ名" },
  { token: "{lens_maker}", label: "レンズメーカー名" },
  { token: "{lens_model}", label: "レンズ名" },
  { token: "{film_sim}", label: "フィルムシミュレーション名" },
  { token: "{orig_name}", label: "元ファイル名" },
];

const DEFAULT_TEMPLATE =
  "{year}{month}{day}_{hour}{minute}{second}_{camera_maker}_{camera_model}_{lens_maker}_{lens_model}_{film_sim}_{orig_name}";

const state = {
  exclusions: [],
  plan: null,
  templateValid: false,
  activeDropField: "jpg",
  hoverField: null,
  lastDropPath: null,
  lastDropAt: 0,
  recentlyAppliedNames: new Set(),
  saveTimer: null,
  unlistenFns: [],
};

const el = {
  message: document.getElementById("message"),
  jpgRow: document.getElementById("jpgRow"),
  rawRow: document.getElementById("rawRow"),
  jpgInput: document.getElementById("jpgInput"),
  rawInput: document.getElementById("rawInput"),
  jpgDropZone: document.getElementById("jpgDropZone"),
  rawDropZone: document.getElementById("rawDropZone"),
  jpgBrowseBtn: document.getElementById("jpgBrowseBtn"),
  jpgClearBtn: document.getElementById("jpgClearBtn"),
  rawBrowseBtn: document.getElementById("rawBrowseBtn"),
  rawClearBtn: document.getElementById("rawClearBtn"),
  rawParentIfMissing: document.getElementById("rawParentIfMissing"),
  templateInput: document.getElementById("templateInput"),
  resetTemplateBtn: document.getElementById("resetTemplateBtn"),
  dedupeSameMake: document.getElementById("dedupeSameMake"),
  backupOriginals: document.getElementById("backupOriginals"),
  tokenButtons: document.getElementById("tokenButtons"),
  templateError: document.getElementById("templateError"),
  sample: document.getElementById("sample"),
  excludeInput: document.getElementById("excludeInput"),
  addExcludeBtn: document.getElementById("addExcludeBtn"),
  excludeList: document.getElementById("excludeList"),
  previewBtn: document.getElementById("previewBtn"),
  applyBtn: document.getElementById("applyBtn"),
  undoBtn: document.getElementById("undoBtn"),
  stats: document.getElementById("stats"),
  planRows: document.getElementById("planRows"),
};

function getInvoke() {
  const invoke = window.__TAURI__?.core?.invoke;
  if (!invoke) {
    throw new Error("Tauri APIが見つかりません。Tauri環境で起動してください。");
  }
  return invoke;
}

function toErrorMessage(error) {
  if (typeof error === "string") {
    return error;
  }
  if (error && typeof error === "object") {
    if (typeof error.message === "string") {
      return error.message;
    }
    return JSON.stringify(error);
  }
  return String(error);
}

async function invokeCommand(cmd, payload = {}) {
  const invoke = getInvoke();
  return invoke(cmd, payload);
}

async function loadPersistedSettings() {
  try {
    const settings = await invokeCommand("load_gui_settings_cmd");
    if (settings && typeof settings.template === "string" && settings.template.trim()) {
      el.templateInput.value = settings.template;
    }
    if (settings && Array.isArray(settings.exclusions)) {
      state.exclusions = settings.exclusions
        .filter((value) => typeof value === "string")
        .map((value) => value.trim())
        .filter((value) => value.length > 0);
    }
    if (settings && typeof settings.backupOriginals === "boolean") {
      el.backupOriginals.checked = settings.backupOriginals;
    }
    if (settings && typeof settings.rawParentIfMissing === "boolean") {
      el.rawParentIfMissing.checked = settings.rawParentIfMissing;
    }
  } catch (error) {
    setMessage(`設定読み込み失敗: ${toErrorMessage(error)}`, true);
  }
}

async function persistSettings() {
  await invokeCommand("save_gui_settings_cmd", {
    request: {
      template: el.templateInput.value,
      exclusions: [...state.exclusions],
      backupOriginals: el.backupOriginals.checked,
      rawParentIfMissing: el.rawParentIfMissing.checked,
    },
  });
}

function schedulePersistSettings() {
  if (state.saveTimer) {
    clearTimeout(state.saveTimer);
  }
  state.saveTimer = setTimeout(() => {
    persistSettings().catch((error) => {
      setMessage(`設定保存失敗: ${toErrorMessage(error)}`, true);
    });
  }, 250);
}

function setMessage(text, isError = false) {
  el.message.textContent = text;
  el.message.style.color = isError ? "#dc2626" : "#0f5132";
}

function insertTokenAtCursor(token) {
  const input = el.templateInput;
  const start = input.selectionStart ?? input.value.length;
  const end = input.selectionEnd ?? input.value.length;
  const before = input.value.slice(0, start);
  const after = input.value.slice(end);
  input.value = `${before}${token}${after}`;
  const cursor = start + token.length;
  input.setSelectionRange(cursor, cursor);
  input.focus();
  schedulePersistSettings();
}

async function resetTemplateToDefault() {
  el.templateInput.value = DEFAULT_TEMPLATE;
  schedulePersistSettings();
  await refreshSampleRealtime();
  await refreshPreviewOnTemplateChange();
  el.templateInput.focus();
}

function renderTokenButtons() {
  for (const item of TOKENS) {
    const btn = document.createElement("button");
    btn.type = "button";
    btn.textContent = item.label;
    btn.title = item.token;
    btn.addEventListener("click", async () => {
      insertTokenAtCursor(item.token);
      await refreshSampleRealtime();
    });
    el.tokenButtons.appendChild(btn);
  }
}

function renderExclusions() {
  el.excludeList.innerHTML = "";
  state.exclusions.forEach((item, idx) => {
    const li = document.createElement("li");
    const text = document.createElement("span");
    text.textContent = item;
    const removeBtn = document.createElement("button");
    removeBtn.type = "button";
    removeBtn.textContent = "×";
    removeBtn.title = "削除";
    removeBtn.setAttribute("aria-label", "削除");
    removeBtn.addEventListener("click", async () => {
      state.exclusions.splice(idx, 1);
      renderExclusions();
      schedulePersistSettings();
      await refreshSampleRealtime();
      await refreshPreviewOnTemplateChange("削除文字列変更を反映してプレビューを更新しました");
    });
    li.appendChild(text);
    li.appendChild(removeBtn);
    el.excludeList.appendChild(li);
  });
}

function currentDeleteStrings() {
  const values = [...state.exclusions];
  const pending = el.excludeInput.value.trim();
  if (!pending) {
    return values;
  }
  if (values.some((v) => v.toLowerCase() === pending.toLowerCase())) {
    return values;
  }
  values.push(pending);
  return values;
}

function toPlanRequest() {
  return {
    jpgInput: el.jpgInput.value.trim(),
    rawInput: el.rawInput.value.trim() || null,
    rawParentIfMissing: el.rawParentIfMissing.checked,
    recursive: false,
    includeHidden: false,
    template: el.templateInput.value,
    dedupeSameMake: el.dedupeSameMake.checked,
    exclusions: currentDeleteStrings(),
    maxFilenameLen: 240,
  };
}

function renderPlan(plan) {
  el.planRows.innerHTML = "";
  let updatedCount = 0;
  let skippedCount = 0;
  let errorCount = 0;

  for (const row of plan.candidates) {
    const originalName = basename(row.original_path);
    const targetName = basename(row.target_path);
    const status = rowStatus(row, originalName);
    if (status === "更新済み") {
      updatedCount += 1;
    } else if (status.startsWith("スキップ")) {
      skippedCount += 1;
    } else if (status.startsWith("エラー")) {
      errorCount += 1;
    }
    const tr = document.createElement("tr");
    tr.innerHTML = `
      <td>${escapeHtml(originalName)}</td>
      <td>${escapeHtml(targetName)}</td>
      <td>${escapeHtml(status)}</td>
    `;
    el.planRows.appendChild(tr);
  }

  el.stats.textContent = `件数: 全件数=${plan.candidates.length} 更新済み=${updatedCount} スキップ=${skippedCount} エラー=${errorCount}`;
}

function escapeHtml(value) {
  return String(value)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}

function basename(path) {
  if (typeof path !== "string") {
    return "";
  }
  const normalized = path.replaceAll("\\\\", "/");
  const index = normalized.lastIndexOf("/");
  return index >= 0 ? normalized.slice(index + 1) : normalized;
}

function rowStatus(row, originalName) {
  const errorMessage =
    typeof row.error_message === "string"
      ? row.error_message
      : typeof row.error === "string"
        ? row.error
        : "";
  if (errorMessage) {
    return `エラー: ${errorMessage}`;
  }
  if (state.recentlyAppliedNames.has(originalName)) {
    return "更新済み";
  }
  if (row.changed) {
    return "更新予定";
  }
  return "スキップ(同名)";
}

async function validateTemplate() {
  try {
    await invokeCommand("validate_template_cmd", { template: el.templateInput.value });
    state.templateValid = true;
    el.templateError.textContent = "";
    return true;
  } catch (error) {
    state.templateValid = false;
    el.templateError.textContent = `テンプレートエラー: ${toErrorMessage(error)}`;
    return false;
  } finally {
    updateApplyButton();
  }
}

async function refreshSampleRealtime() {
  const valid = await validateTemplate();
  if (!valid) {
    el.sample.textContent = "出力サンプル: (テンプレートエラー)";
    return;
  }

  const request = {
    template: el.templateInput.value,
    dedupeSameMake: el.dedupeSameMake.checked,
    exclusions: currentDeleteStrings(),
    maxFilenameLen: 240,
  };

  try {
    const sample = await invokeCommand("render_fixed_sample_cmd", { request });
    el.sample.textContent = `出力サンプル: ${sample}`;
  } catch (error) {
    el.sample.textContent = `出力サンプル: エラー (${toErrorMessage(error)})`;
  }
}

function updateApplyButton() {
  el.applyBtn.disabled = !(state.templateValid && state.plan);
}

function clearPlanState() {
  state.plan = null;
  state.recentlyAppliedNames.clear();
  el.planRows.innerHTML = "";
  el.stats.textContent = "件数: -";
  updateApplyButton();
}

async function updatePlan(reason = "preview", options = {}) {
  const skipSampleRefresh = Boolean(options.skipSampleRefresh);
  const request = toPlanRequest();
  if (!request.jpgInput) {
    throw new Error("JPGフォルダを入力してください");
  }

  if (reason !== "after_apply") {
    state.recentlyAppliedNames.clear();
  }

  const plan = await invokeCommand("generate_plan_cmd", { request });
  state.plan = plan;
  renderPlan(plan);
  updateApplyButton();
  if (!skipSampleRefresh) {
    await refreshSampleRealtime();
  }
}

async function onPreview() {
  try {
    await updatePlan("preview");
    setMessage("プレビューを更新しました", false);
  } catch (error) {
    setMessage(`プレビュー生成失敗: ${toErrorMessage(error)}`, true);
  }
}

async function onApply() {
  try {
    const valid = await validateTemplate();
    if (!valid) {
      return;
    }

    await updatePlan("apply", { skipSampleRefresh: true });
    if (!state.plan) {
      throw new Error("プレビューを生成できませんでした");
    }

    const appliedNames = state.plan.candidates
      .filter((row) => row.changed)
      .map((row) => basename(row.target_path));
    state.recentlyAppliedNames = new Set(appliedNames);

    const result = await invokeCommand("apply_plan_cmd", {
      request: {
        plan: state.plan,
        backupOriginals: el.backupOriginals.checked,
      },
    });
    setMessage(`適用完了: ${result.applied}件`, false);
    await updatePlan("after_apply");
  } catch (error) {
    state.recentlyAppliedNames.clear();
    setMessage(`適用失敗: ${toErrorMessage(error)}`, true);
  }
}

async function onUndo() {
  try {
    const result = await invokeCommand("undo_last_cmd");
    setMessage(`取り消し完了: ${result.restored}件`, false);
    if (el.jpgInput.value.trim()) {
      await updatePlan("preview");
    }
  } catch (error) {
    setMessage(`取り消し失敗: ${toErrorMessage(error)}`, true);
  }
}

async function refreshPreviewIfJpgSelected(field) {
  if (field !== "jpg") {
    return;
  }
  if (!el.jpgInput.value.trim()) {
    return;
  }
  try {
    await updatePlan("preview");
    setMessage("JPGフォルダを設定しプレビューを更新しました", false);
  } catch (error) {
    setMessage(`プレビュー生成失敗: ${toErrorMessage(error)}`, true);
  }
}

async function refreshPreviewOnTemplateChange(successMessage = "テンプレート変更を反映してプレビューを更新しました") {
  if (!el.jpgInput.value.trim()) {
    return;
  }
  if (!state.templateValid) {
    return;
  }
  try {
    await updatePlan("preview", { skipSampleRefresh: true });
    setMessage(successMessage, false);
  } catch (error) {
    setMessage(`プレビュー生成失敗: ${toErrorMessage(error)}`, true);
  }
}

function normalizeFileUriToPath(value) {
  if (!value || !value.startsWith("file://")) {
    return null;
  }

  try {
    const url = new URL(value);
    let path = decodeURIComponent(url.pathname || "");
    if (!path) {
      return null;
    }

    if (/^\/[A-Za-z]:\//.test(path)) {
      path = path.slice(1);
    }

    if (url.host && url.host !== "localhost") {
      path = `//${url.host}${path}`;
    }

    return path;
  } catch {
    return null;
  }
}

function extractDroppedPathFromDataTransfer(dataTransfer) {
  if (!dataTransfer) {
    return null;
  }

  if (dataTransfer.files && dataTransfer.files.length > 0) {
    const file = dataTransfer.files[0];
    if (typeof file.path === "string" && file.path.length > 0) {
      return file.path;
    }
  }

  const uriList = dataTransfer.getData("text/uri-list");
  if (uriList) {
    const first = uriList
      .split("\n")
      .map((line) => line.trim())
      .find((line) => line && !line.startsWith("#"));
    const path = normalizeFileUriToPath(first);
    if (path) {
      return path;
    }
  }

  const plain = dataTransfer.getData("text/plain").trim();
  if (plain) {
    return normalizeFileUriToPath(plain) || plain;
  }

  return null;
}

function firstPathFromPayload(payload) {
  if (!payload) {
    return null;
  }

  if (Array.isArray(payload) && payload.length > 0) {
    return String(payload[0]);
  }

  if (Array.isArray(payload.paths) && payload.paths.length > 0) {
    return String(payload.paths[0]);
  }

  if (typeof payload.path === "string") {
    return payload.path;
  }

  return null;
}

function extractPositionFromPayload(payload) {
  if (!payload) {
    return null;
  }

  const pos = payload.position ?? payload;
  if (!pos) {
    return null;
  }

  if (Array.isArray(pos) && pos.length >= 2) {
    const x = Number(pos[0]);
    const y = Number(pos[1]);
    if (Number.isFinite(x) && Number.isFinite(y)) {
      return { x, y };
    }
  }

  if (typeof pos === "object") {
    const x = Number(pos.x);
    const y = Number(pos.y);
    if (Number.isFinite(x) && Number.isFinite(y)) {
      return { x, y };
    }
  }

  return null;
}

function targetInputByField(field) {
  return field === "raw" ? el.rawInput : el.jpgInput;
}

function targetRowByField(field) {
  return field === "raw" ? el.rawRow : el.jpgRow;
}

function targetDropZoneByField(field) {
  return field === "raw" ? el.rawDropZone : el.jpgDropZone;
}

function clearDropOverlay() {
  el.jpgDropZone.classList.remove("drag-over");
  el.rawDropZone.classList.remove("drag-over");
}

function setDropOverlay(field, visible) {
  clearDropOverlay();
  if (!visible) {
    return;
  }
  targetDropZoneByField(field).classList.add("drag-over");
}

function setHoverField(field) {
  state.hoverField = field;
  if (field) {
    setDropOverlay(field, true);
  } else {
    clearDropOverlay();
  }
}

function activeFieldName() {
  const focused = document.activeElement;
  if (focused === el.rawInput) {
    return "raw";
  }
  if (focused === el.jpgInput) {
    return "jpg";
  }
  return state.activeDropField;
}

function fieldFromClientPosition(x, y) {
  const candidates = [
    { field: "jpg", rect: el.jpgRow.getBoundingClientRect() },
    { field: "raw", rect: el.rawRow.getBoundingClientRect() },
  ];

  for (const item of candidates) {
    if (
      x >= item.rect.left &&
      x <= item.rect.right &&
      y >= item.rect.top &&
      y <= item.rect.bottom
    ) {
      return item.field;
    }
  }

  return null;
}

function fieldFromPayloadPosition(payload) {
  const pos = extractPositionFromPayload(payload);
  if (!pos) {
    return null;
  }

  const dpr = window.devicePixelRatio || 1;
  const screenX =
    typeof window.screenX === "number"
      ? window.screenX
      : typeof window.screenLeft === "number"
        ? window.screenLeft
        : 0;
  const screenY =
    typeof window.screenY === "number"
      ? window.screenY
      : typeof window.screenTop === "number"
        ? window.screenTop
        : 0;
  const points = [
    { x: pos.x, y: pos.y },
    { x: pos.x / dpr, y: pos.y / dpr },
    { x: pos.x - screenX, y: pos.y - screenY },
    { x: (pos.x - screenX) / dpr, y: (pos.y - screenY) / dpr },
  ];

  const matched = new Set();
  for (const point of points) {
    const field = fieldFromClientPosition(point.x, point.y);
    if (field) {
      matched.add(field);
    }
  }

  if (matched.size === 1) {
    return [...matched][0];
  }

  return null;
}

async function setFolderPathToField(rawPath, field) {
  if (!rawPath) {
    return;
  }

  const input = targetInputByField(field);
  try {
    const folderPath = await invokeCommand("normalize_to_folder_cmd", {
      path: rawPath,
    });
    input.value = folderPath;
    state.activeDropField = field;
    setMessage(`${field === "jpg" ? "JPG" : "RAW"}フォルダを設定しました`, false);
    await refreshPreviewIfJpgSelected(field);
  } catch (error) {
    setMessage(`フォルダ設定失敗: ${toErrorMessage(error)}`, true);
  }
}

async function onBrowse(field) {
  const input = targetInputByField(field);
  try {
    const selected = await invokeCommand("pick_folder_cmd", {
      initial: input.value.trim() || null,
    });
    if (!selected) {
      return;
    }
    input.value = selected;
    state.activeDropField = field;
    setMessage(`${field === "jpg" ? "JPG" : "RAW"}フォルダを設定しました`, false);
    await refreshPreviewIfJpgSelected(field);
  } catch (error) {
    setMessage(`フォルダ選択失敗: ${toErrorMessage(error)}`, true);
  }
}

async function clearFolder(field) {
  const input = targetInputByField(field);
  if (!input.value.trim()) {
    setMessage(`${field === "jpg" ? "JPG" : "RAW"}フォルダは未設定です`, false);
    return;
  }

  input.value = "";
  state.activeDropField = field;

  if (field === "jpg") {
    clearPlanState();
    setMessage("JPGフォルダをクリアしました", false);
    return;
  }

  if (!el.jpgInput.value.trim()) {
    setMessage("RAWフォルダをクリアしました", false);
    return;
  }

  try {
    await updatePlan("preview");
    setMessage("RAWフォルダをクリアしプレビューを更新しました", false);
  } catch (error) {
    setMessage(`プレビュー生成失敗: ${toErrorMessage(error)}`, true);
  }
}

function resolveDropField(options = {}) {
  const { clientX, clientY, payload } = options;
  if (Number.isFinite(clientX) && Number.isFinite(clientY)) {
    const field = fieldFromClientPosition(clientX, clientY);
    if (field) {
      return field;
    }
  }

  if (state.hoverField) {
    return state.hoverField;
  }

  return fieldFromPayloadPosition(payload);
}

function isDuplicateDrop(path) {
  const now = Date.now();
  const duplicated = state.lastDropPath === path && now - state.lastDropAt < 700;
  state.lastDropPath = path;
  state.lastDropAt = now;
  return duplicated;
}

async function handleDroppedPath(rawPath, field) {
  if (!rawPath || isDuplicateDrop(rawPath)) {
    return;
  }

  if (!field) {
    setHoverField(null);
    setMessage("JPGまたはRAWの入力欄上にドロップしてください", true);
    return;
  }

  state.activeDropField = field;
  setHoverField(null);
  await setFolderPathToField(rawPath, field);
}

function bindDropTarget(input, field) {
  const row = targetRowByField(field);
  const zone = targetDropZoneByField(field);

  const setActive = () => {
    state.activeDropField = field;
    setHoverField(field);
  };

  const onDragOver = (event) => {
    event.preventDefault();
    event.stopPropagation();
    setActive();
  };

  const onDragEnter = (event) => {
    event.preventDefault();
    event.stopPropagation();
    setActive();
  };

  const onDragLeave = (event) => {
    event.preventDefault();
    event.stopPropagation();
    const related = event.relatedTarget;
    if (related && row.contains(related)) {
      return;
    }
    if (state.hoverField === field) {
      setHoverField(null);
    }
  };

  const onDrop = async (event) => {
    event.preventDefault();
    event.stopPropagation();
    const droppedPath = extractDroppedPathFromDataTransfer(event.dataTransfer);
    if (!droppedPath) {
      setMessage("ドロップからパスを取得できませんでした", true);
      return;
    }
    const targetField = resolveDropField({
      clientX: event.clientX,
      clientY: event.clientY,
    });
    await handleDroppedPath(droppedPath, targetField);
  };

  input.addEventListener("focus", () => {
    state.activeDropField = field;
  });
  input.addEventListener("click", () => {
    state.activeDropField = field;
  });
  row.addEventListener("dragenter", onDragEnter);
  row.addEventListener("dragover", onDragOver);
  row.addEventListener("dragleave", onDragLeave);
  row.addEventListener("drop", onDrop);
  zone.addEventListener("dragenter", onDragEnter);
  zone.addEventListener("dragover", onDragOver);
  zone.addEventListener("dragleave", onDragLeave);
  zone.addEventListener("drop", onDrop);
}

function bindWindowDomDropEvents() {
  window.addEventListener("dragover", (event) => {
    event.preventDefault();
    const field = fieldFromClientPosition(event.clientX, event.clientY);
    if (field) {
      state.activeDropField = field;
      setHoverField(field);
      return;
    }
    setHoverField(null);
  });

  window.addEventListener("drop", async (event) => {
    if (event.defaultPrevented) {
      return;
    }
    event.preventDefault();
    const droppedPath = extractDroppedPathFromDataTransfer(event.dataTransfer);
    const field = resolveDropField({
      clientX: event.clientX,
      clientY: event.clientY,
    });
    if (droppedPath) {
      await handleDroppedPath(droppedPath, field);
    }
  });

  window.addEventListener("dragleave", (event) => {
    if (event.clientX === 0 && event.clientY === 0) {
      setHoverField(null);
    }
  });
}

async function bindTauriDropEvents() {
  const listen = window.__TAURI__?.event?.listen;
  if (typeof listen !== "function") {
    return;
  }

  const onDragEnter = (event) => {
    const field = fieldFromPayloadPosition(event?.payload);
    if (field) {
      state.activeDropField = field;
      setHoverField(field);
      return;
    }
    setHoverField(null);
  };

  const onDragOver = (event) => {
    const field = fieldFromPayloadPosition(event?.payload);
    if (field) {
      state.activeDropField = field;
      setHoverField(field);
      return;
    }
    setHoverField(null);
  };

  const onDrop = async (event) => {
    const droppedPath = firstPathFromPayload(event?.payload);
    if (!droppedPath) {
      return;
    }

    const field = resolveDropField({ payload: event?.payload });
    await handleDroppedPath(droppedPath, field);
  };

  const onDragLeave = () => {
    setHoverField(null);
  };

  for (const [eventName, handler] of [
    ["tauri://drag-enter", onDragEnter],
    ["tauri://drag-over", onDragOver],
    ["tauri://file-drop-hover", onDragOver],
    ["tauri://drag-leave", onDragLeave],
    ["tauri://file-drop-cancelled", onDragLeave],
    ["tauri://drag-drop", onDrop],
    ["tauri://file-drop", onDrop],
  ]) {
    try {
      const unlisten = await listen(eventName, handler);
      state.unlistenFns.push(unlisten);
    } catch {
      // 環境差異でイベント名が未対応の場合は無視
    }
  }
}

function bindEvents() {
  el.addExcludeBtn.addEventListener("click", async () => {
    const value = el.excludeInput.value.trim();
    if (!value) {
      return;
    }
    if (state.exclusions.some((item) => item.toLowerCase() === value.toLowerCase())) {
      el.excludeInput.value = "";
      return;
    }
    state.exclusions.push(value);
    el.excludeInput.value = "";
    renderExclusions();
    schedulePersistSettings();
    await refreshSampleRealtime();
    await refreshPreviewOnTemplateChange("削除文字列変更を反映してプレビューを更新しました");
  });

  el.excludeInput.addEventListener("keydown", async (event) => {
    if (event.key !== "Enter") {
      return;
    }
    event.preventDefault();
    el.addExcludeBtn.click();
  });

  el.jpgBrowseBtn.addEventListener("click", () => onBrowse("jpg"));
  el.jpgClearBtn.addEventListener("click", () => clearFolder("jpg"));
  el.rawBrowseBtn.addEventListener("click", () => onBrowse("raw"));
  el.rawClearBtn.addEventListener("click", () => clearFolder("raw"));

  bindDropTarget(el.jpgInput, "jpg");
  bindDropTarget(el.rawInput, "raw");
  bindWindowDomDropEvents();

  el.resetTemplateBtn.addEventListener("click", resetTemplateToDefault);
  el.templateInput.addEventListener("input", async () => {
    schedulePersistSettings();
    await refreshSampleRealtime();
    await refreshPreviewOnTemplateChange();
  });
  el.backupOriginals.addEventListener("change", schedulePersistSettings);
  el.rawParentIfMissing.addEventListener("change", async () => {
    schedulePersistSettings();
    try {
      if (el.jpgInput.value.trim()) {
        await updatePlan("preview", { skipSampleRefresh: true });
        setMessage("RAW探索ルート設定を反映してプレビューを更新しました", false);
      }
    } catch (error) {
      setMessage(`プレビュー生成失敗: ${toErrorMessage(error)}`, true);
    }
  });
  el.dedupeSameMake.addEventListener("change", async () => {
    await refreshSampleRealtime();
    await refreshPreviewOnTemplateChange("メーカー重複設定を反映してプレビューを更新しました");
  });
  el.previewBtn.addEventListener("click", onPreview);
  el.applyBtn.addEventListener("click", onApply);
  el.undoBtn.addEventListener("click", onUndo);
}

async function init() {
  renderTokenButtons();
  bindEvents();
  await loadPersistedSettings();
  renderExclusions();
  await bindTauriDropEvents();
  await refreshSampleRealtime();
}

init().catch((error) => {
  setMessage(`初期化失敗: ${toErrorMessage(error)}`, true);
});
