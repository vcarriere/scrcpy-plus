import { test, expect } from "@playwright/test";
import { mkdir } from "node:fs/promises";
import { mock } from "./fixtures.mjs";

test("executable defaults show their actual paths and do not launch tools", async ({ page }) => {
    await mock(page, { update_preference: null });
    await page.goto("/settings");
    await expect(page.getByLabel("SCRCPY executable", { exact: true })).toHaveValue("C:\\SCRCPY+\\scrcpy\\scrcpy.exe");
    await expect(page.getByLabel("ADB executable", { exact: true })).toHaveValue("C:\\SCRCPY+\\scrcpy\\adb.exe");
    await expect(page.getByText("Using bundled default", { exact: true })).toHaveCount(2);
    await expect(page.getByRole("button", { name: "Reset", exact: true })).toHaveCount(0);
    expect(await page.evaluate(() => adb_test.calls.some(call => ["exec_adb", "exec_scrcpy", "run_scrcpy", "get_dependency_versions"].includes(call.command)))).toBe(false);
    await mkdir("../artifacts", { recursive: true });
    await page.screenshot({ path: "../artifacts/settings-defaults-preview.png", fullPage: true });
});

test("custom executables and password wrappers can reset independently without losing the other choice", async ({ page }) => {
    const wrapper = "C:\\Phone Tools\\scrcpy-password.cmd";
    const adb = "C:\\Android Tools\\adb.exe";
    await mock(page, { saved_paths: { scrcpy_path: wrapper, adb_path: adb } });
    await page.goto("/settings");
    await expect(page.getByText("Using custom executable", { exact: true })).toHaveCount(2);
    await expect(page.getByLabel("SCRCPY executable", { exact: true })).toHaveValue(wrapper);
    await page.locator(".mb-4").filter({ has: page.locator("#scrcpy_path") }).getByRole("button", { name: "Reset", exact: true }).click();
    await expect(page.getByText("Using bundled default", { exact: true })).toHaveCount(1);
    await expect(page.getByLabel("ADB executable", { exact: true })).toHaveValue(adb);
    expect(await page.evaluate(() => localStorage.getItem("scrcpyplus.scrcpy_path"))).toBe(null);
    expect(await page.evaluate(() => localStorage.getItem("scrcpyplus.adb_path"))).toBe(adb);
    await page.reload();
    await expect(page.getByText("Using bundled default", { exact: true })).toHaveCount(1);
    await expect(page.getByLabel("ADB executable", { exact: true })).toHaveValue(adb);
});

test("a missing saved custom executable explains the actual fallback", async ({ page }) => {
    const missing = "C:\\Missing Tools\\scrcpy.exe";
    await mock(page, { saved_paths: { scrcpy_path: missing }, invalid_paths: [missing] });
    await page.goto("/settings");
    await expect(page.getByRole("alert")).toContainText(missing);
    await expect(page.getByLabel("SCRCPY executable", { exact: true })).toHaveValue("C:\\SCRCPY+\\scrcpy\\scrcpy.exe");
    await expect(page.getByText("Using bundled default", { exact: true })).toHaveCount(2);
    await page.getByRole("button", { name: "Reset", exact: true }).click();
    await expect(page.getByRole("alert")).toHaveCount(0);
});

test("a missing default is unavailable and narrow executable controls fit", async ({ page }) => {
    await mock(page, { missing_defaults: ["adb_path"] });
    await page.setViewportSize({ width: 380, height: 800 });
    await page.goto("/settings");
    await expect(page.getByText("Default executable unavailable", { exact: true })).toBeVisible();
    await expect(page.getByText("Using bundled default", { exact: true })).toHaveCount(1);
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
});

test("a deleted selected custom file remains custom and unavailable", async ({ page }) => {
    const deleted = "C:\\Removed Tools\\adb.exe";
    await mock(page, { saved_paths: { adb_path: deleted }, missing_custom: [deleted] });
    await page.goto("/settings");
    await expect(page.getByLabel("ADB executable", { exact: true })).toHaveValue(deleted);
    await expect(page.getByText("Custom executable unavailable", { exact: true })).toBeVisible();
    await expect(page.getByText("Using bundled default", { exact: true })).toHaveCount(1);
    expect(await page.evaluate(() => localStorage.getItem("scrcpyplus.adb_path"))).toBe(deleted);
});

test("automatic update checks default on, persist inverted choices, and only check the existing app source", async ({ page }) => {
    const requests = [];
    await page.route("https://api.github.com/repos/**/releases", async route => {
        requests.push(route.request().url());
        await route.fulfill({ json: [] });
    });
    await mock(page, { update_preference: null });
    await page.goto("/settings");
    await expect(page.getByRole("checkbox", { name: "Automatically check for updates", exact: true })).toBeChecked();
    await page.getByRole("checkbox", { name: "Automatically check for updates", exact: true }).uncheck();
    expect(await page.evaluate(() => localStorage.getItem("setting.disable_update_check"))).toBe("true");
    await page.reload();
    await expect(page.getByRole("checkbox", { name: "Automatically check for updates", exact: true })).not.toBeChecked();
    await page.locator('a[href="/"]').click();
    await expect(page.getByText("SCRCPY+", { exact: true })).toBeVisible();
    expect(requests).toEqual([]);
    await page.getByRole("link", { name: "Settings", exact: true }).click();
    await page.getByRole("checkbox", { name: "Automatically check for updates", exact: true }).check();
    expect(await page.evaluate(() => localStorage.getItem("setting.disable_update_check"))).toBe("false");
    await page.locator('a[href="/"]').click();
    await expect.poll(() => requests.length).toBe(1);
    expect(requests).toEqual(["https://api.github.com/repos/Frontesque/scrcpy-plus/releases"]);
});

for (const disabled of [true, false]) {
    test(`existing disabled-update value ${disabled} keeps its effective choice`, async ({ page }) => {
        await mock(page, { update_preference: disabled });
        await page.goto("/settings");
        const checkbox = page.getByRole("checkbox", { name: "Automatically check for updates", exact: true });
        if (disabled) await expect(checkbox).not.toBeChecked();
        else await expect(checkbox).toBeChecked();
        expect(await page.evaluate(() => localStorage.getItem("setting.disable_update_check"))).toBe(String(disabled));
    });
}

test("the affirmative update label and executable status are translated", async ({ page }) => {
    await mock(page);
    await page.goto("/settings");
    await page.getByRole("combobox").selectOption("it");
    await expect(page.getByRole("checkbox", { name: "Controlla automaticamente gli aggiornamenti", exact: true })).toBeVisible();
    await expect(page.getByRole("button", { name: "Controlla gli aggiornamenti ora", exact: true })).toBeVisible();
    await page.getByRole("combobox").selectOption("ja");
    await expect(page.getByRole("checkbox", { name: "更新を自動的に確認する", exact: true })).toBeVisible();
    await expect(page.getByRole("button", { name: "今すぐ更新を確認する", exact: true })).toBeVisible();
    await expect(page.getByText("同梱の既定の実行ファイルを使用中", { exact: true })).toHaveCount(2);
    await expect(page.getByLabel("ADB 実行ファイル", { exact: true })).toBeVisible();
});
