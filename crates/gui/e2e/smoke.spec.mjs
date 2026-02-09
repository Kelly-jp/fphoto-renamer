import { expect, test } from "@playwright/test";
import { installTauriMock } from "./support/mock-tauri.mjs";

async function openWithMock(page, options = {}) {
  await page.addInitScript(installTauriMock, options);
  await page.goto("/index.html");
}

async function getMockCalls(page, cmd = null) {
  return page.evaluate((targetCmd) => {
    const calls = Array.isArray(globalThis.__mockTauriCalls) ? globalThis.__mockTauriCalls : [];
    if (!targetCmd) {
      return calls;
    }
    return calls.filter((call) => call.cmd === targetCmd);
  }, cmd);
}

async function dropPathToZone(page, zoneId, path) {
  await page.evaluate(
    ({ zoneId: targetZoneId, path: droppedPath }) => {
      const zone = document.getElementById(targetZoneId);
      const dataTransfer = new DataTransfer();
      dataTransfer.setData("text/plain", droppedPath);
      const event = new DragEvent("drop", {
        bubbles: true,
        cancelable: true,
        dataTransfer,
      });
      zone.dispatchEvent(event);
    },
    { zoneId, path }
  );
}

test.describe("Browser UI smoke", () => {
  test("初期表示で主要要素が表示される", async ({ page }) => {
    await openWithMock(page, {
      sampleText: "20260208091530_FUJIFILM_X-T5_FUJIFILM_XF16-55mmF2.8RLMWR_CLASSIC_CHROME_IMG_0001",
      settings: {
        template: "{year}{month}{day}_{orig_name}",
        exclusions: ["-NR"],
        dedupeSameMaker: true,
        backupOriginals: false,
        rawParentIfMissing: false,
      },
    });

    await expect(page.locator("#jpgInput")).toBeVisible();
    await expect(page.locator("#rawInput")).toBeVisible();
    await expect(page.locator("#applyBtn")).toBeDisabled();
    await expect(page.locator("#tokenButtons button")).toHaveCount(12);
    await expect(page.locator("#sample")).toContainText(
      "出力サンプル: 20260208091530_FUJIFILM_X-T5_FUJIFILM_XF16-55mmF2.8RLMWR_CLASSIC_CHROME_IMG_0001"
    );

    const invokedCommands = (await getMockCalls(page)).map((call) => call.cmd);
    expect(invokedCommands).toContain("load_gui_settings_cmd");
    expect(invokedCommands).toContain("validate_template_cmd");
    expect(invokedCommands).toContain("render_fixed_sample_cmd");
  });

  test("保存済み設定のdedupe状態をチェックボックスへ反映する", async ({ page }) => {
    await openWithMock(page, {
      settings: {
        dedupeSameMaker: false,
      },
    });

    await expect(page.locator("#dedupeSameMaker")).not.toBeChecked();
  });

  test("JPG入力後に変換実行できログが出る", async ({ page }) => {
    await openWithMock(page, {
      planCandidates: [
        { originalName: "IMG_0001.JPG", targetName: "20260208091530_IMG_0001.JPG", changed: true },
        { originalName: "IMG_0002.JPG", targetName: "IMG_0002.JPG", changed: false },
      ],
    });

    await page.fill("#jpgInput", "/tmp/mock-jpg");
    await expect(page.locator("#applyBtn")).toBeEnabled();

    await page.click("#applyBtn");
    await expect(page.locator("#actionMessage")).toContainText("変換完了: 1件");
    await expect(page.locator("#convertLog")).toContainText(/✅ IMG_0001\.JPG\s*→ 20260208091530_IMG_0001\.JPG/);
    await expect(page.locator("#convertLog")).toContainText(/⏭️ IMG_0002\.JPG\s*→ IMG_0002\.JPG/);

    const invokedCommands = (await getMockCalls(page)).map((call) => call.cmd);
    expect(invokedCommands).toContain("generate_plan_cmd");
    expect(invokedCommands).toContain("apply_plan_cmd");
  });

  test("変換ログに情報源ラベルを表示する", async ({ page }) => {
    await openWithMock(page, {
      planCandidates: [
        { originalName: "IMG_1001.JPG", targetName: "RENAMED_1001.JPG", changed: true, sourceLabel: "xmp" },
        { originalName: "IMG_1002.JPG", targetName: "RENAMED_1002.JPG", changed: true, sourceLabel: "raf" },
        { originalName: "IMG_1003.JPG", targetName: "RENAMED_1003.JPG", changed: true, sourceLabel: "dng" },
        { originalName: "IMG_1004.JPG", targetName: "IMG_1004.JPG", changed: false, sourceLabel: "jpg" },
      ],
    });

    await page.fill("#jpgInput", "/tmp/mock-jpg");
    await page.click("#applyBtn");

    await expect(page.locator("#convertLog")).toContainText(
      /✅ IMG_1001\.JPG\s*→ RENAMED_1001\.JPG \(情報取得元:xmp\)/
    );
    await expect(page.locator("#convertLog")).toContainText(
      /✅ IMG_1002\.JPG\s*→ RENAMED_1002\.JPG \(情報取得元:raf\)/
    );
    await expect(page.locator("#convertLog")).toContainText(
      /✅ IMG_1003\.JPG\s*→ RENAMED_1003\.JPG \(情報取得元:dng\)/
    );
    await expect(page.locator("#convertLog")).toContainText(
      /⏭️ IMG_1004\.JPG\s*→ IMG_1004\.JPG \(情報取得元:jpg\)/
    );
  });

  test("変換中はボタンやテキスト入力を無効化する", async ({ page }) => {
    await openWithMock(page, {
      applyDelayMs: 500,
    });

    await page.fill("#jpgInput", "/tmp/mock-jpg");
    await page.click("#applyBtn");

    await expect(page.locator("#actionMessage")).toContainText("変換中...");
    await expect(page.locator("#jpgInput")).toBeDisabled();
    await expect(page.locator("#rawInput")).toBeDisabled();
    await expect(page.locator("#templateInput")).toBeDisabled();
    await expect(page.locator("#excludeInput")).toBeDisabled();
    await expect(page.locator("#jpgBrowseBtn")).toBeDisabled();
    await expect(page.locator("#rawBrowseBtn")).toBeDisabled();
    await expect(page.locator("#applyBtn")).toBeDisabled();
    await expect(page.locator("#undoBtn")).toBeDisabled();
    await expect(page.locator("#tokenButtons button").first()).toBeDisabled();

    await expect(page.locator("#actionMessage")).toContainText("変換完了: 1件");
    await expect(page.locator("#jpgInput")).toBeEditable();
    await expect(page.locator("#rawInput")).toBeEditable();
    await expect(page.locator("#templateInput")).toBeEditable();
    await expect(page.locator("#excludeInput")).toBeEditable();
    await expect(page.locator("#jpgBrowseBtn")).toBeEnabled();
    await expect(page.locator("#rawBrowseBtn")).toBeEnabled();
    await expect(page.locator("#applyBtn")).toBeEnabled();
  });

  test("削除文字列の追加と重複除外が機能する", async ({ page }) => {
    await openWithMock(page, {});

    await expect(page.locator("#excludeList li")).toHaveCount(1);

    await page.fill("#excludeInput", "-DxO");
    await page.click("#addExcludeBtn");
    await expect(page.locator("#excludeList li")).toHaveCount(2);

    await page.fill("#excludeInput", "-dxo");
    await page.click("#addExcludeBtn");
    await expect(page.locator("#excludeList li")).toHaveCount(2);
  });

  test("テンプレート検証エラー時はサンプルにエラーを表示し適用を無効化する", async ({ page }) => {
    await openWithMock(page, {
      failCommands: {
        validate_template_cmd: "テンプレート形式エラー",
      },
    });

    await expect(page.locator("#sample")).toContainText("テンプレートエラー: テンプレート形式エラー");
    await page.fill("#jpgInput", "/tmp/mock-jpg");
    await expect(page.locator("#applyBtn")).toBeDisabled();
  });

  test("適用失敗時は失敗メッセージと失敗ログを表示する", async ({ page }) => {
    await openWithMock(page, {
      failCommands: {
        apply_plan_cmd: "書き込み失敗",
      },
    });

    await page.fill("#jpgInput", "/tmp/mock-jpg");
    await page.click("#applyBtn");

    await expect(page.locator("#actionMessage")).toContainText("変換失敗: 書き込み失敗");
    await expect(page.locator("#convertLog")).toContainText(/❌ IMG_0001\.JPG\s*→ 20260208091530_IMG_0001\.JPG/);
  });

  test("undo実行で元に戻しメッセージとログを表示する", async ({ page }) => {
    await openWithMock(page, {
      restored: 1,
    });

    await page.fill("#jpgInput", "/tmp/mock-jpg");
    await page.click("#applyBtn");
    await expect(page.locator("#undoBtn")).toBeEnabled();

    await page.click("#undoBtn");
    await expect(page.locator("#actionMessage")).toContainText("元に戻し完了: 1件");
    await expect(page.locator("#convertLog")).toContainText(/↩️ 20260208091530_IMG_0001\.JPG\s*→ IMG_0001\.JPG/);
  });

  test("フォルダ選択ボタンでJPG入力へ反映される", async ({ page }) => {
    await openWithMock(page, {
      pickFolderPath: "/Users/kelly/Pictures/TestJpg",
    });

    await page.click("#jpgBrowseBtn");
    await expect(page.locator("#jpgInput")).toHaveValue("/Users/kelly/Pictures/TestJpg");
    await expect(page.locator("#applyBtn")).toBeEnabled();
  });

  test("JPGクリアでログを初期化し適用とundoを無効化する", async ({ page }) => {
    await openWithMock(page, {});

    await page.fill("#jpgInput", "/tmp/mock-jpg");
    await page.click("#applyBtn");
    await expect(page.locator("#undoBtn")).toBeEnabled();
    await expect(page.locator("#convertLog")).toContainText("✅ IMG_0001.JPG");

    await page.click("#jpgClearBtn");
    await expect(page.locator("#jpgInput")).toHaveValue("");
    await expect(page.locator("#applyBtn")).toBeDisabled();
    await expect(page.locator("#undoBtn")).toBeDisabled();
    await expect(page.locator("#convertLog")).toContainText("まだ変換ログはありません");
  });

  test("JPGフォルダ未存在エラーはJPGクリアで消える", async ({ page }) => {
    await openWithMock(page, {
      failCommands: {
        generate_plan_cmd: "JPGフォルダが存在しません: /tmp/missing-jpg",
      },
    });

    await page.fill("#jpgInput", "/tmp/missing-jpg");
    await page.click("#applyBtn");
    await expect(page.locator("#actionMessage")).toContainText(
      "変換失敗: JPGフォルダが存在しません: /tmp/missing-jpg"
    );

    await page.click("#jpgClearBtn");
    await expect(page.locator("#actionMessage")).toHaveText("");
  });

  test("RAWフォルダ未存在エラーはRAWクリアで消える", async ({ page }) => {
    await openWithMock(page, {
      failCommands: {
        generate_plan_cmd: "RAWフォルダが存在しません: /tmp/missing-raw",
      },
    });

    await page.fill("#jpgInput", "/tmp/mock-jpg");
    await page.fill("#rawInput", "/tmp/missing-raw");
    await page.click("#applyBtn");
    await expect(page.locator("#actionMessage")).toContainText(
      "変換失敗: RAWフォルダが存在しません: /tmp/missing-raw"
    );

    await page.click("#rawClearBtn");
    await expect(page.locator("#actionMessage")).toHaveText("");
  });

  test("設定変更はデバウンスで保存され最終値のみ送信される", async ({ page }) => {
    await openWithMock(page, {});

    await page.evaluate(() => {
      const template = document.querySelector("#templateInput");
      const dedupe = document.querySelector("#dedupeSameMaker");
      const backup = document.querySelector("#backupOriginals");
      const rawParent = document.querySelector("#rawParentIfMissing");

      template.value = "{year}_A";
      template.dispatchEvent(new Event("input", { bubbles: true }));
      template.value = "{year}_AB";
      template.dispatchEvent(new Event("input", { bubbles: true }));

      backup.checked = true;
      backup.dispatchEvent(new Event("change", { bubbles: true }));
      rawParent.checked = true;
      rawParent.dispatchEvent(new Event("change", { bubbles: true }));
      dedupe.checked = false;
      dedupe.dispatchEvent(new Event("change", { bubbles: true }));
    });

    await page.waitForTimeout(450);

    const saveCalls = await getMockCalls(page, "save_gui_settings_cmd");
    expect(saveCalls.length).toBe(1);
    expect(saveCalls[0].payload.request).toEqual({
      template: "{year}_AB",
      exclusions: ["-NR"],
      dedupeSameMaker: false,
      backupOriginals: true,
      rawParentIfMissing: true,
    });
  });

  test("設定保存失敗時はエラーメッセージを表示する", async ({ page }) => {
    await openWithMock(page, {
      failCommands: {
        save_gui_settings_cmd: "保存先へ書き込みできません",
      },
    });

    await page.check("#backupOriginals");
    await page.waitForTimeout(450);
    await expect(page.locator("#actionMessage")).toContainText(
      "設定保存失敗: 保存先へ書き込みできません"
    );
  });

  test("ドロップ操作でJPG入力へパスを反映できる", async ({ page }) => {
    await openWithMock(page, {});

    await page.fill("#jpgInput", "/tmp/mock-jpg");
    await page.click("#applyBtn");
    await expect(page.locator("#convertLog")).toContainText("✅ IMG_0001.JPG");

    await dropPathToZone(page, "jpgDropZone", " /tmp/dropped-jpg ");

    await expect(page.locator("#jpgInput")).toHaveValue("/tmp/dropped-jpg");
    await expect(page.locator("#convertLog")).toContainText("まだ変換ログはありません");
    await expect(page.locator("#applyBtn")).toBeEnabled();
    const normalizeCalls = await getMockCalls(page, "normalize_to_folder_cmd");
    expect(normalizeCalls.length).toBeGreaterThan(0);
  });

  test("ドロップ操作でRAW入力へパスを反映した際にログを初期化する", async ({ page }) => {
    await openWithMock(page, {});

    await page.fill("#jpgInput", "/tmp/mock-jpg");
    await page.click("#applyBtn");
    await expect(page.locator("#convertLog")).toContainText("✅ IMG_0001.JPG");

    await dropPathToZone(page, "rawDropZone", "/tmp/dropped-raw");

    await expect(page.locator("#rawInput")).toHaveValue("/tmp/dropped-raw");
    await expect(page.locator("#convertLog")).toContainText("まだ変換ログはありません");
  });

  test("テンプレート入力時に禁止文字を自動除去する", async ({ page }) => {
    await openWithMock(page, {});

    await page.fill("#templateInput", "{year}:*?<>|abc");
    await expect(page.locator("#templateInput")).toHaveValue("{year}abc");
    await expect(page.locator("#sample")).not.toContainText("テンプレートエラー");
  });

  test("undo失敗時はエラーメッセージを表示する", async ({ page }) => {
    await openWithMock(page, {
      failCommands: {
        undo_last_cmd: "undo履歴がありません",
      },
    });

    await page.fill("#jpgInput", "/tmp/mock-jpg");
    await page.click("#applyBtn");
    await page.click("#undoBtn");
    await expect(page.locator("#actionMessage")).toContainText("元に戻し失敗: undo履歴がありません");
  });

  test("フォルダ選択キャンセル時は入力値を変更しない", async ({ page }) => {
    await openWithMock(page, {
      pickFolderPath: null,
    });

    await page.fill("#jpgInput", "/tmp/original-jpg");
    await page.click("#jpgBrowseBtn");
    await expect(page.locator("#jpgInput")).toHaveValue("/tmp/original-jpg");
  });

  test("フォルダ選択失敗時はエラーメッセージを表示する", async ({ page }) => {
    await openWithMock(page, {
      failCommands: {
        pick_folder_cmd: "dialog起動失敗",
      },
    });

    await page.click("#jpgBrowseBtn");
    await expect(page.locator("#actionMessage")).toContainText("フォルダ選択失敗: dialog起動失敗");
  });

  test("適用時に生成計画と適用オプションのpayloadが正しく渡る", async ({ page }) => {
    await openWithMock(page, {});

    await page.fill("#jpgInput", "/tmp/mock-jpg");
    await page.fill("#rawInput", "/tmp/mock-raw");
    await page.check("#backupOriginals");
    await page.check("#rawParentIfMissing");
    await page.uncheck("#dedupeSameMaker");
    await page.fill("#excludeInput", "-pending");
    await page.click("#applyBtn");

    const generateCalls = await getMockCalls(page, "generate_plan_cmd");
    expect(generateCalls.length).toBeGreaterThan(0);
    const request = generateCalls.at(-1).payload.request;
    expect(request.jpgInput).toBe("/tmp/mock-jpg");
    expect(request.rawInput).toBe("/tmp/mock-raw");
    expect(request.rawParentIfMissing).toBe(true);
    expect(request.dedupeSameMaker).toBe(false);
    expect(request.exclusions).toEqual(["-NR", "-pending"]);

    const applyCalls = await getMockCalls(page, "apply_plan_cmd");
    expect(applyCalls.length).toBeGreaterThan(0);
    expect(applyCalls.at(-1).payload.request.backupOriginals).toBe(true);
  });

  test("テンプレートリセットで既定値に戻る", async ({ page }) => {
    await openWithMock(page, {});

    await page.fill("#templateInput", "{year}_custom");
    await expect(page.locator("#templateInput")).toHaveValue("{year}_custom");

    await page.click("#resetTemplateBtn");
    await expect(page.locator("#templateInput")).toHaveValue(
      "{year}{month}{day}_{hour}{minute}{second}_{camera_maker}_{camera_model}_{lens_maker}_{lens_model}_{film_sim}_{orig_name}"
    );
  });

  test("サンプル生成失敗時はエラー表示へ切り替わる", async ({ page }) => {
    await openWithMock(page, {
      failCommands: {
        render_fixed_sample_cmd: "sample生成失敗",
      },
    });

    await expect(page.locator("#sample")).toContainText("出力サンプル: エラー (sample生成失敗)");
  });
});
