export function installTauriMock(options = {}) {
  const settings = {
    template:
      options.settings?.template ||
      "{year}{month}{day}_{hour}{minute}{second}_{camera_maker}_{camera_model}_{lens_maker}_{lens_model}_{film_sim}_{orig_name}",
    exclusions: Array.isArray(options.settings?.exclusions) ? options.settings.exclusions : ["-NR"],
    dedupeSameMaker: options.settings?.dedupeSameMaker !== false,
    backupOriginals: Boolean(options.settings?.backupOriginals),
    rawParentIfMissing: Boolean(options.settings?.rawParentIfMissing),
  };

  const sampleText =
    options.sampleText || "20260208091530_FUJIFILM_X-T5_FUJIFILM_XF16-55mmF2.8RLMWR_CLASSIC_CHROME_IMG_0001";

  const planRows =
    Array.isArray(options.planCandidates) && options.planCandidates.length > 0
      ? options.planCandidates
      : [{ originalName: "IMG_0001.JPG", targetName: "20260208091530_IMG_0001.JPG", changed: true }];
  const failCommands =
    options.failCommands && typeof options.failCommands === "object" ? options.failCommands : {};

  const calls = [];

  const fail = (message) => {
    throw new Error(message);
  };
  const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

  const trimString = (value) => (typeof value === "string" ? value.trim() : "");
  const normalizeFolder = (value) => value.replace(/\\/g, "/").replace(/\/+$/, "");
  const maybeFail = (cmd) => {
    const message = failCommands[cmd];
    if (typeof message === "string" && message.length > 0) {
      fail(message);
    }
  };
  const toPlanCandidate = (row, basePath, index) => {
    const normalizedBasePath = normalizeFolder(basePath);
    if (typeof row?.original_path === "string" && typeof row?.target_path === "string") {
      return {
        original_path: row.original_path,
        target_path: row.target_path,
        changed: row.changed !== false,
      };
    }

    const defaultName = `IMG_${String(index + 1).padStart(4, "0")}.JPG`;
    const originalName = trimString(row?.originalName) || defaultName;
    const targetName = trimString(row?.targetName) || `20260208091530_${originalName}`;
    return {
      original_path: `${normalizedBasePath}/${originalName}`,
      target_path: `${normalizedBasePath}/${targetName}`,
      changed: row?.changed !== false,
    };
  };

  globalThis.__mockTauriCalls = calls;
  globalThis.__TAURI__ = {
    core: {
      invoke: async (cmd, payload = {}) => {
        calls.push({ cmd, payload });
        maybeFail(cmd);
        switch (cmd) {
          case "load_gui_settings_cmd":
            return settings;
          case "save_gui_settings_cmd":
            return null;
          case "validate_template_cmd": {
            const template = trimString(payload?.template);
            if (/[\\/:*?"<>|]/.test(template)) {
              fail("テンプレートに使用できない文字が含まれています");
            }
            return null;
          }
          case "render_fixed_sample_cmd":
            return sampleText;
          case "render_sample_cmd":
            return sampleText;
          case "normalize_to_folder_cmd": {
            const rawPath = trimString(payload?.path);
            if (!rawPath) {
              fail("パスが空です");
            }
            return rawPath.replace(/\\/g, "/");
          }
          case "pick_folder_cmd":
            return options.pickFolderPath === undefined ? "/tmp/mock-folder" : options.pickFolderPath;
          case "generate_plan_cmd": {
            const jpgInput = trimString(payload?.request?.jpgInput);
            if (!jpgInput) {
              fail("JPGフォルダを入力してください");
            }
            const normalized = normalizeFolder(jpgInput);
            return {
              candidates: planRows.map((row, index) => toPlanCandidate(row, normalized, index)),
            };
          }
          case "apply_plan_cmd": {
            const applyDelayMs = Number(options.applyDelayMs);
            if (Number.isFinite(applyDelayMs) && applyDelayMs > 0) {
              await wait(applyDelayMs);
            }
            const applied =
              Number.isFinite(options.applied) && Number(options.applied) >= 0
                ? Number(options.applied)
                : Array.isArray(payload?.request?.plan?.candidates)
                  ? payload.request.plan.candidates.filter((row) => row?.changed).length
                  : 0;
            return { applied };
          }
          case "undo_last_cmd": {
            const restored =
              Number.isFinite(options.restored) && Number(options.restored) >= 0
                ? Number(options.restored)
                : 1;
            return { restored };
          }
          default:
            fail(`Unsupported mock command: ${cmd}`);
        }
      },
    },
    event: {
      listen: async () => () => {},
    },
  };
}
