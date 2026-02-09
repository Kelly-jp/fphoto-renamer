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
const TEMPLATE_DISALLOWED_CHAR = /[\\/:*?"<>|]/;
const TEMPLATE_DISALLOWED_CHAR_GLOBAL = /[\\/:*?"<>|]/g;

const state = {
  exclusions: [],
  plan: null,
  undoEnabled: false,
  isApplying: false,
  templateValid: false,
  templateValidationMessage: "",
  hoverField: null,
  hoverFieldAt: 0,
  dragDepth: { jpg: 0, raw: 0 },
  pendingZoneDropField: null,
  pendingTauriPath: null,
  lastHandledDrop: null,
  saveTimer: null,
  unlistenFns: [],
};

const el = {
  message: document.getElementById("actionMessage"),
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
  dedupeSameMaker: document.getElementById("dedupeSameMaker"),
  backupOriginals: document.getElementById("backupOriginals"),
  tokenButtons: document.getElementById("tokenButtons"),
  templateError: document.getElementById("templateError"),
  sample: document.getElementById("sample"),
  excludeInput: document.getElementById("excludeInput"),
  addExcludeBtn: document.getElementById("addExcludeBtn"),
  excludeList: document.getElementById("excludeList"),
  applyBtn: document.getElementById("applyBtn"),
  undoBtn: document.getElementById("undoBtn"),
  convertLog: document.getElementById("convertLog"),
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
        .map((value) => removeDisallowedTemplateChars(value).trim())
        .filter((value) => value.length > 0);
    }
    if (settings && typeof settings.dedupeSameMaker === "boolean") {
      el.dedupeSameMaker.checked = settings.dedupeSameMaker;
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
      dedupeSameMaker: el.dedupeSameMaker.checked,
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

function missingFolderErrorPrefixByField(field) {
  if (field === "raw") {
    return "変換失敗: RAWフォルダが存在しません";
  }
  return "変換失敗: JPGフォルダが存在しません";
}

function clearMissingFolderErrorForFieldIfNeeded(field) {
  const input = targetInputByField(field);
  if (input.value.trim().length > 0) {
    return;
  }

  const currentMessage = (el.message.textContent || "").trim();
  if (!currentMessage) {
    return;
  }

  if (currentMessage.startsWith(missingFolderErrorPrefixByField(field))) {
    setMessage("", false);
  }
}

function removeDisallowedTemplateChars(value) {
  return String(value).replace(TEMPLATE_DISALLOWED_CHAR_GLOBAL, "");
}

function countDisallowedTemplateChars(value) {
  const matched = String(value).match(TEMPLATE_DISALLOWED_CHAR_GLOBAL);
  return matched ? matched.length : 0;
}

function sanitizeTextInputInPlace(input) {
  const current = input.value;
  const sanitized = removeDisallowedTemplateChars(current);
  if (sanitized === current) {
    return false;
  }

  const start = input.selectionStart ?? current.length;
  const end = input.selectionEnd ?? current.length;
  const removedBeforeStart = countDisallowedTemplateChars(current.slice(0, start));
  const removedBeforeEnd = countDisallowedTemplateChars(current.slice(0, end));
  const nextStart = Math.max(0, start - removedBeforeStart);
  const nextEnd = Math.max(0, end - removedBeforeEnd);

  input.value = sanitized;
  input.setSelectionRange(nextStart, nextEnd);
  return true;
}

function sanitizeTemplateInputInPlace() {
  return sanitizeTextInputInPlace(el.templateInput);
}

function sanitizeExcludeInputInPlace() {
  return sanitizeTextInputInPlace(el.excludeInput);
}

function insertTextAtSelection(input, text) {
  const start = input.selectionStart ?? input.value.length;
  const end = input.selectionEnd ?? input.value.length;
  const before = input.value.slice(0, start);
  const after = input.value.slice(end);
  input.value = `${before}${text}${after}`;
  const nextCursor = start + text.length;
  input.setSelectionRange(nextCursor, nextCursor);
}

async function onTemplateInputChanged() {
  schedulePersistSettings();
  await refreshSampleRealtime();
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
    });
    li.appendChild(text);
    li.appendChild(removeBtn);
    el.excludeList.appendChild(li);
  });
}

function currentDeleteStrings() {
  const values = [...state.exclusions];
  const pending = removeDisallowedTemplateChars(el.excludeInput.value).trim();
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
    dedupeSameMaker: el.dedupeSameMaker.checked,
    exclusions: currentDeleteStrings(),
    maxFilenameLen: 240,
  };
}

function basename(path) {
  if (typeof path !== "string") {
    return "";
  }
  const normalized = path.replaceAll("\\\\", "/");
  const index = normalized.lastIndexOf("/");
  return index >= 0 ? normalized.slice(index + 1) : normalized;
}

function renderConvertLogEntries(entries) {
  el.convertLog.innerHTML = "";
  if (!entries.length) {
    renderEmptyConvertLog();
    return;
  }

  for (const entry of entries) {
    appendSingleConvertLogEntry(entry);
  }
}

function normalizeLogSourceLabel(label) {
  if (typeof label !== "string") {
    return "";
  }
  const normalized = label.trim().toLowerCase();
  return normalized;
}

function inferLogSourceFromMetadataSource(metadataSource) {
  switch (metadataSource) {
    case "Xmp":
    case "XmpAndRawExif":
      return "xmp";
    case "RawExif":
      return "raw";
    case "JpgExif":
    case "FallbackFileModified":
    default:
      return "jpg";
  }
}

function resolveLogSourceLabel(row) {
  return (
    normalizeLogSourceLabel(row?.source_label) ||
    inferLogSourceFromMetadataSource(row?.metadata_source)
  );
}

function buildLogEntriesFromPlan(plan, changedEmoji) {
  return plan.candidates.map((row) => ({
    emoji: row.changed ? changedEmoji : "⏭️",
    original: basename(row.original_path),
    target: basename(row.target_path),
    source: resolveLogSourceLabel(row),
  }));
}

function buildUndoLogEntriesFromPlan(plan) {
  return plan.candidates
    .filter((row) => row.changed)
    .map((row) => ({
      emoji: "↩️",
      original: basename(row.target_path),
      target: basename(row.original_path),
    }));
}

function renderEmptyConvertLog() {
  el.convertLog.innerHTML = "";
  const empty = document.createElement("li");
  empty.className = "empty";
  empty.textContent = "まだ変換ログはありません";
  el.convertLog.appendChild(empty);
}

function appendSingleConvertLogEntry(entry) {
  const li = document.createElement("li");
  const sourceSuffix = entry.source ? ` (${entry.source})` : "";
  const originalLine = document.createElement("span");
  originalLine.textContent = `${entry.emoji} ${entry.original}`;
  const targetLine = document.createElement("span");
  targetLine.className = "convert-log-target-line";
  targetLine.textContent = `→ ${entry.target}${sourceSuffix}`;
  li.appendChild(originalLine);
  li.appendChild(targetLine);
  el.convertLog.appendChild(li);
}

async function validateTemplate() {
  try {
    await invokeCommand("validate_template_cmd", { template: el.templateInput.value });
    state.templateValid = true;
    state.templateValidationMessage = "";
    el.templateError.textContent = "";
    return true;
  } catch (error) {
    state.templateValid = false;
    state.templateValidationMessage = `テンプレートエラー: ${toErrorMessage(error)}`;
    el.templateError.textContent = "";
    return false;
  } finally {
    updateApplyButton();
  }
}

function setSampleText(message, isError = false) {
  el.sample.textContent = `出力サンプル: ${message}`;
  el.sample.classList.toggle("sample-error", isError);
}

async function refreshSampleRealtime() {
  const valid = await validateTemplate();
  if (!valid) {
    setSampleText(state.templateValidationMessage || "(テンプレートエラー)", true);
    return;
  }

  const request = {
    template: el.templateInput.value,
    dedupeSameMaker: el.dedupeSameMaker.checked,
    exclusions: currentDeleteStrings(),
    maxFilenameLen: 240,
  };

  try {
    const sample = await invokeCommand("render_fixed_sample_cmd", { request });
    setSampleText(sample, false);
  } catch (error) {
    setSampleText(`エラー (${toErrorMessage(error)})`, true);
  }
}

function updateApplyButton() {
  const canApply = state.templateValid && el.jpgInput.value.trim().length > 0;
  el.applyBtn.disabled = state.isApplying || !canApply;
}

function setUndoButtonEnabled(enabled) {
  state.undoEnabled = Boolean(enabled);
  el.undoBtn.disabled = state.isApplying || !state.undoEnabled;
}

function setInteractionLocked(locked) {
  for (const control of [
    el.jpgInput,
    el.rawInput,
    el.jpgBrowseBtn,
    el.jpgClearBtn,
    el.rawBrowseBtn,
    el.rawClearBtn,
    el.rawParentIfMissing,
    el.templateInput,
    el.resetTemplateBtn,
    el.dedupeSameMaker,
    el.backupOriginals,
    el.excludeInput,
    el.addExcludeBtn,
    el.applyBtn,
    el.undoBtn,
  ]) {
    control.disabled = locked;
  }

  for (const button of el.tokenButtons.querySelectorAll("button")) {
    button.disabled = locked;
  }
  for (const button of el.excludeList.querySelectorAll("button")) {
    button.disabled = locked;
  }
}

function startApplyLock() {
  state.isApplying = true;
  setInteractionLocked(true);
  updateApplyButton();
  setUndoButtonEnabled(state.undoEnabled);
}

function endApplyLock() {
  state.isApplying = false;
  setInteractionLocked(false);
  updateApplyButton();
  setUndoButtonEnabled(state.undoEnabled);
}

function clearPlanState() {
  state.plan = null;
  updateApplyButton();
  setUndoButtonEnabled(false);
}

async function generatePlanForApply() {
  const request = toPlanRequest();
  if (!request.jpgInput) {
    throw new Error("JPGフォルダを入力してください");
  }
  return invokeCommand("generate_plan_cmd", { request });
}

async function onApply() {
  if (state.isApplying) {
    return;
  }

  let plan = null;
  startApplyLock();
  setMessage("変換中...", false);
  try {
    const valid = await validateTemplate();
    if (!valid) {
      return;
    }

    plan = await generatePlanForApply();
    state.plan = plan;

    const result = await invokeCommand("apply_plan_cmd", {
      request: {
        plan,
        backupOriginals: el.backupOriginals.checked,
      },
    });
    renderConvertLogEntries(buildLogEntriesFromPlan(plan, "✅"));
    setMessage(`変換完了: ${result.applied}件`, false);
    const appliedCount = Number(result.applied) || 0;
    const changedCount = Array.isArray(plan?.candidates)
      ? plan.candidates.filter((row) => row.changed).length
      : 0;
    setUndoButtonEnabled(appliedCount > 0 || changedCount > 0);
  } catch (error) {
    if (plan) {
      renderConvertLogEntries(buildLogEntriesFromPlan(plan, "❌"));
    }
    setMessage(`変換失敗: ${toErrorMessage(error)}`, true);
  } finally {
    endApplyLock();
  }
}

async function onUndo() {
  if (state.isApplying) {
    return;
  }

  try {
    const result = await invokeCommand("undo_last_cmd");
    const undoEntries = state.plan
      ? buildUndoLogEntriesFromPlan(state.plan)
      : [];
    const nextLogEntries =
      undoEntries.length > 0
        ? undoEntries
        : [
            {
              emoji: "↩️",
              original: "元に戻し実行",
              target: `${result.restored}件`,
            },
          ];
    renderConvertLogEntries(nextLogEntries);
    setMessage(`元に戻し完了: ${result.restored}件`, false);
    state.plan = null;
    setUndoButtonEnabled(false);
  } catch (error) {
    setMessage(`元に戻し失敗: ${toErrorMessage(error)}`, true);
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

function targetDropZoneByField(field) {
  return field === "raw" ? el.rawDropZone : el.jpgDropZone;
}

function fieldFromEventTarget(target) {
  if (!(target instanceof Node)) {
    return null;
  }
  if (el.jpgDropZone.contains(target)) {
    return "jpg";
  }
  if (el.rawDropZone.contains(target)) {
    return "raw";
  }
  return null;
}

function pointInsideRect(x, y, rect, margin = 0) {
  return (
    x >= rect.left - margin &&
    x <= rect.right + margin &&
    y >= rect.top - margin &&
    y <= rect.bottom + margin
  );
}

function distancePointToRect(x, y, rect) {
  const dx =
    x < rect.left ? rect.left - x : x > rect.right ? x - rect.right : 0;
  const dy =
    y < rect.top ? rect.top - y : y > rect.bottom ? y - rect.bottom : 0;
  return Math.hypot(dx, dy);
}

function scoreFieldFromPoints(points, field) {
  const rect = targetDropZoneByField(field).getBoundingClientRect();
  let score = 0;
  for (const point of points) {
    if (pointInsideRect(point.x, point.y, rect, 0)) {
      score += 4;
      continue;
    }
    if (pointInsideRect(point.x, point.y, rect, 18)) {
      score += 2;
      continue;
    }
    if (distancePointToRect(point.x, point.y, rect) <= 24) {
      score += 1;
    }
  }
  return score;
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

  const originCandidates = [
    { x: pos.x, y: pos.y },
    { x: pos.x / dpr, y: pos.y / dpr },
    { x: pos.x - screenX, y: pos.y - screenY },
    { x: (pos.x - screenX) / dpr, y: (pos.y - screenY) / dpr },
  ];
  const hotspotOffsets = [
    { x: 0, y: 0 },
    { x: 0, y: 26 },
    { x: 0, y: 40 },
    { x: -10, y: 34 },
    { x: 10, y: 34 },
  ];

  const points = [];
  for (const origin of originCandidates) {
    for (const offset of hotspotOffsets) {
      points.push({ x: origin.x + offset.x, y: origin.y + offset.y });
    }
  }

  const jpgScore = scoreFieldFromPoints(points, "jpg");
  const rawScore = scoreFieldFromPoints(points, "raw");
  if (jpgScore <= 0 && rawScore <= 0) {
    return null;
  }
  if (jpgScore === rawScore) {
    return null;
  }
  return jpgScore > rawScore ? "jpg" : "raw";
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
  if (state.hoverField === field) {
    if (field) {
      state.hoverFieldAt = Date.now();
    }
    return;
  }
  state.hoverField = field;
  state.hoverFieldAt = field ? Date.now() : 0;
  if (field) {
    setDropOverlay(field, true);
  } else {
    clearDropOverlay();
  }
}

function resetDragDepth() {
  state.dragDepth.jpg = 0;
  state.dragDepth.raw = 0;
}

function clearDragHoverState() {
  resetDragDepth();
  setHoverField(null);
}

function markZoneDragEnter(field) {
  state.dragDepth[field] += 1;
  setHoverField(field);
}

function markZoneDragLeave(field) {
  if (state.dragDepth[field] > 0) {
    state.dragDepth[field] -= 1;
  }
  if (state.hoverField === field && state.dragDepth[field] === 0) {
    setHoverField(null);
  }
}

function rememberPendingZoneDropField(field) {
  state.pendingZoneDropField = {
    field,
    at: Date.now(),
  };
}

function consumePendingZoneDropField(maxAgeMs = 1200) {
  const pending = state.pendingZoneDropField;
  state.pendingZoneDropField = null;
  if (!pending) {
    return null;
  }
  if (Date.now() - pending.at > maxAgeMs) {
    return null;
  }
  return pending.field;
}

function consumePendingTauriPath(maxAgeMs = 1200) {
  const pending = state.pendingTauriPath;
  state.pendingTauriPath = null;
  if (!pending) {
    return null;
  }
  if (Date.now() - pending.at > maxAgeMs) {
    return null;
  }
  return pending.path;
}

function recentHoverField(maxAgeMs = 1200) {
  if (!state.hoverField) {
    return null;
  }
  if (Date.now() - state.hoverFieldAt > maxAgeMs) {
    return null;
  }
  return state.hoverField;
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
    updateApplyButton();
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
    updateApplyButton();
  } catch (error) {
    setMessage(`フォルダ選択失敗: ${toErrorMessage(error)}`, true);
  }
}

async function clearFolder(field) {
  renderEmptyConvertLog();
  const input = targetInputByField(field);
  if (!input.value.trim()) {
    return;
  }

  input.value = "";
  updateApplyButton();
  clearMissingFolderErrorForFieldIfNeeded(field);

  if (field === "jpg") {
    clearPlanState();
    return;
  }
}

function dropSourcePriority(source) {
  switch (source) {
    case "zone":
      return 3;
    case "window":
      return 2;
    case "tauri":
      return 1;
    default:
      return 0;
  }
}

function canonicalDropPath(path) {
  return String(path)
    .replaceAll("\\", "/")
    .replace(/\/+$/, "")
    .toLowerCase();
}

function shouldIgnoreDrop(path, field, source) {
  const prev = state.lastHandledDrop;
  if (!prev) {
    return false;
  }
  if (prev.path !== canonicalDropPath(path)) {
    return false;
  }
  if (Date.now() - prev.at > 1200) {
    return false;
  }
  if (prev.field === field) {
    return true;
  }
  return dropSourcePriority(source) <= dropSourcePriority(prev.source);
}

function rememberHandledDrop(path, field, source) {
  state.lastHandledDrop = {
    path: canonicalDropPath(path),
    field,
    source,
    at: Date.now(),
  };
}

async function handleDroppedPath(rawPath, field, source = "window") {
  if (!rawPath) {
    return;
  }
  if (state.isApplying) {
    return;
  }

  if (shouldIgnoreDrop(rawPath, field, source)) {
    return;
  }

  if (!field) {
    clearDragHoverState();
    return;
  }

  clearDragHoverState();
  state.pendingZoneDropField = null;
  state.pendingTauriPath = null;
  await setFolderPathToField(rawPath, field);
  renderEmptyConvertLog();
  rememberHandledDrop(rawPath, field, source);
}

function bindDropTarget(field) {
  const zone = targetDropZoneByField(field);

  const onDragOver = (event) => {
    event.preventDefault();
    event.stopPropagation();
    setHoverField(field);
  };

  const onDragEnter = (event) => {
    event.preventDefault();
    event.stopPropagation();
    markZoneDragEnter(field);
  };

  const onDragLeave = (event) => {
    event.preventDefault();
    event.stopPropagation();
    const related = event.relatedTarget;
    if (related && zone.contains(related)) {
      return;
    }
    markZoneDragLeave(field);
  };

  const onDrop = async (event) => {
    event.preventDefault();
    event.stopPropagation();
    const droppedPath =
      extractDroppedPathFromDataTransfer(event.dataTransfer) ||
      consumePendingTauriPath(300);
    if (!droppedPath) {
      rememberPendingZoneDropField(field);
      return;
    }
    await handleDroppedPath(droppedPath, field, "zone");
  };

  zone.addEventListener("dragenter", onDragEnter);
  zone.addEventListener("dragover", onDragOver);
  zone.addEventListener("dragleave", onDragLeave);
  zone.addEventListener("drop", onDrop);
}

function bindWindowDomDropEvents() {
  window.addEventListener("dragover", (event) => {
    event.preventDefault();
  });

  window.addEventListener("drop", async (event) => {
    event.preventDefault();
    const droppedPath =
      extractDroppedPathFromDataTransfer(event.dataTransfer) ||
      consumePendingTauriPath(300);
    const field =
      consumePendingZoneDropField() ||
      recentHoverField() ||
      fieldFromEventTarget(event.target);
    if (droppedPath && field) {
      await handleDroppedPath(droppedPath, field, "window");
      return;
    }
    clearDragHoverState();
    state.pendingZoneDropField = null;
    state.pendingTauriPath = null;
  });

  window.addEventListener("dragleave", (event) => {
    if (event.clientX === 0 && event.clientY === 0) {
      clearDragHoverState();
      state.pendingZoneDropField = null;
      state.pendingTauriPath = null;
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
      setHoverField(field);
      return;
    }
    clearDragHoverState();
  };

  const onDragOver = (event) => {
    const field = fieldFromPayloadPosition(event?.payload);
    if (field) {
      setHoverField(field);
      return;
    }
    clearDragHoverState();
  };

  const onDrop = async (event) => {
    const droppedPath = firstPathFromPayload(event?.payload);
    if (!droppedPath) {
      clearDragHoverState();
      return;
    }

    const field =
      consumePendingZoneDropField() ||
      recentHoverField() ||
      fieldFromPayloadPosition(event?.payload);
    if (!field) {
      clearDragHoverState();
      state.pendingZoneDropField = null;
      return;
    }
    await handleDroppedPath(droppedPath, field, "tauri");
  };

  const onDragLeave = () => {
    clearDragHoverState();
    state.pendingZoneDropField = null;
    state.pendingTauriPath = null;
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
    const value = removeDisallowedTemplateChars(el.excludeInput.value).trim();
    el.excludeInput.value = value;
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
  });

  el.excludeInput.addEventListener("keydown", async (event) => {
    if (!event.ctrlKey && !event.metaKey && !event.altKey) {
      if (event.key.length === 1 && TEMPLATE_DISALLOWED_CHAR.test(event.key)) {
        event.preventDefault();
        return;
      }
    }
    if (event.key !== "Enter") {
      return;
    }
    event.preventDefault();
    el.addExcludeBtn.click();
  });
  el.excludeInput.addEventListener("paste", (event) => {
    const text = event.clipboardData?.getData("text/plain");
    if (typeof text !== "string") {
      return;
    }
    event.preventDefault();
    const sanitized = removeDisallowedTemplateChars(text);
    insertTextAtSelection(el.excludeInput, sanitized);
    sanitizeExcludeInputInPlace();
  });
  el.excludeInput.addEventListener("input", () => {
    sanitizeExcludeInputInPlace();
  });

  el.jpgBrowseBtn.addEventListener("click", () => onBrowse("jpg"));
  el.jpgClearBtn.addEventListener("click", () => clearFolder("jpg"));
  el.rawBrowseBtn.addEventListener("click", () => onBrowse("raw"));
  el.rawClearBtn.addEventListener("click", () => clearFolder("raw"));
  el.jpgInput.addEventListener("input", () => {
    updateApplyButton();
    clearMissingFolderErrorForFieldIfNeeded("jpg");
  });
  el.rawInput.addEventListener("input", () => {
    clearMissingFolderErrorForFieldIfNeeded("raw");
  });

  bindDropTarget("jpg");
  bindDropTarget("raw");
  bindWindowDomDropEvents();

  el.resetTemplateBtn.addEventListener("click", resetTemplateToDefault);
  el.templateInput.addEventListener("keydown", (event) => {
    if (event.ctrlKey || event.metaKey || event.altKey) {
      return;
    }
    if (event.key.length !== 1) {
      return;
    }
    if (TEMPLATE_DISALLOWED_CHAR.test(event.key)) {
      event.preventDefault();
    }
  });
  el.templateInput.addEventListener("paste", async (event) => {
    const text = event.clipboardData?.getData("text/plain");
    if (typeof text !== "string") {
      return;
    }
    event.preventDefault();
    const sanitized = removeDisallowedTemplateChars(text);
    insertTextAtSelection(el.templateInput, sanitized);
    await onTemplateInputChanged();
  });
  el.templateInput.addEventListener("input", async () => {
    sanitizeTemplateInputInPlace();
    await onTemplateInputChanged();
  });
  el.backupOriginals.addEventListener("change", schedulePersistSettings);
  el.rawParentIfMissing.addEventListener("change", schedulePersistSettings);
  el.dedupeSameMaker.addEventListener("change", async () => {
    schedulePersistSettings();
    await refreshSampleRealtime();
  });
  el.applyBtn.addEventListener("click", onApply);
  el.undoBtn.addEventListener("click", onUndo);
}

async function init() {
  renderTokenButtons();
  bindEvents();
  setUndoButtonEnabled(false);
  await loadPersistedSettings();
  sanitizeTemplateInputInPlace();
  renderExclusions();
  await bindTauriDropEvents();
  await refreshSampleRealtime();
}

init().catch((error) => {
  setMessage(`初期化失敗: ${toErrorMessage(error)}`, true);
});
