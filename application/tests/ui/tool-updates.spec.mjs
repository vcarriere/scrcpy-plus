import { test, expect } from "@playwright/test";
import { mkdir } from "node:fs/promises";
import { mock } from "./fixtures.mjs";

const app_release = { tag_name: "v3.1.0", prerelease: false, draft: false, html_url: "https://github.com/Frontesque/scrcpy-plus/releases/tag/v3.1.0" };
const available = { tool: "adb", state: "available", current_version: "37.0.2-1234", latest_version: "37.0.3", release_url: "https://developer.android.com/tools/releases/platform-tools" };
const current = { tool: "scrcpy", state: "current", current_version: "5.0.1", latest_version: "v5.0.1", url: "https://github.com/Genymobile/scrcpy/releases/download/v5.0.1/scrcpy-win64-v5.0.1.zip" };

async function setup(page, options = {}, automatic = false) {
    await mock(page, { tool_updates: [available, current], versions: { scrcpy_plus: "3.1.0", adb: "37.0.2-1234", scrcpy: "5.0.1" }, ...options });
    if (automatic) await page.addInitScript(() => {
        localStorage.setItem("setting.auto_update_adb", "true");
        localStorage.setItem("setting.adb_sdk_license", "true");
    });
    await page.route("https://api.github.com/repos/Frontesque/scrcpy-plus/releases", route => route.fulfill({ json: [app_release] }));
}

test("manual checks include both tools but show versions only for available updates", async ({ page }) => {
    await setup(page);
    await page.goto("/settings");
    await page.getByRole("button", { name: "Check for updates", exact: true }).click();
    await expect(page.getByText("ADB: 37.0.2-1234 → 37.0.3", { exact: true })).toBeVisible();
    await expect(page.getByText("scrcpy: 5.0.1 → v5.0.1", { exact: true })).toHaveCount(0);
    expect(await page.evaluate(() => localStorage.getItem("setting.disable_update_check"))).toBe("true");
    expect(await page.evaluate(() => adb_test.calls.filter(call => call.command === "get_tool_updates").length)).toBe(1);
    expect(await page.evaluate(() => adb_test.calls.filter(call => ["download_adb", "download_scrcpy", "activate_tool_paths"].includes(call.command)))).toEqual([]);
    await mkdir("../artifacts", { recursive: true });
    await page.screenshot({ path: "../artifacts/update-settings-available-preview.png", fullPage: true, animations: "disabled" });
});

test("one failed source does not erase another component's result or claim all are current", async ({ page }) => {
    await setup(page, { tool_updates: [{ ...available, state: "error" }, current] });
    await page.goto("/settings");
    await page.getByRole("button", { name: "Check for updates", exact: true }).click();
    await expect(page.getByRole("alert")).toContainText("Some update checks could not be completed.");
    await expect(page.getByText("scrcpy: 5.0.1 → v5.0.1", { exact: true })).toHaveCount(0);
    await expect(page.getByText("ADB: 37.0.2-1234 → 37.0.3", { exact: true })).toHaveCount(0);
    await expect(page.getByText("SCRCPY+, ADB and scrcpy are up to date.", { exact: true })).toHaveCount(0);
});

test("automatic tool installation waits for idle and preserves its opt-in", async ({ page }) => {
    await page.clock.install();
    await setup(page, { update_preference: false, sessions: 1 }, true);
    await page.goto("/");
    await expect(page.getByText("ADB: 37.0.2-1234 → 37.0.3", { exact: true })).toBeVisible();
    await page.clock.fastForward(61000);
    expect(await page.evaluate(() => adb_test.calls.filter(call => call.command === "download_adb"))).toEqual([]);
    await page.evaluate(() => { adb_test.sessions = 0; });
    await page.clock.fastForward(61000);
    await expect.poll(() => page.evaluate(() => adb_test.calls.filter(call => call.command === "download_adb").length)).toBe(1);
    await expect(page.getByText("ADB: 37.0.2-1234 → 37.0.3", { exact: true })).toHaveCount(0);
    expect(await page.evaluate(() => localStorage.getItem("setting.auto_update_adb"))).toBe("true");
    expect(await page.evaluate(() => localStorage.getItem("scrcpyplus.scrcpy_path"))).toBeNull();
});

test("manual results cannot trigger background installation while automatic checks are off", async ({ page }) => {
    await page.clock.install();
    await setup(page, {}, true);
    await page.goto("/settings");
    await page.getByRole("button", { name: "Check for updates", exact: true }).click();
    await expect(page.getByText("ADB: 37.0.2-1234 → 37.0.3", { exact: true })).toBeVisible();
    await page.clock.fastForward(121000);
    expect(await page.evaluate(() => adb_test.calls.filter(call => call.command === "download_adb"))).toEqual([]);
});

test("automatic installation defaults off in Advanced and SDK acknowledgment requires an explicit choice", async ({ page }) => {
    await page.setViewportSize({ width: 380, height: 850 });
    await setup(page, { advanced_adb: true });
    await page.goto("/settings");
    const adb = page.getByRole("checkbox", { name: "Install ADB updates automatically when idle", exact: true });
    await expect(adb).not.toBeChecked();
    await expect(page.getByRole("checkbox", { name: "Install scrcpy updates automatically when idle", exact: true })).not.toBeChecked();
    expect(await page.evaluate(() => localStorage.getItem("setting.adb_sdk_license"))).toBeNull();
    await adb.check();
    expect(await page.evaluate(() => localStorage.getItem("setting.adb_sdk_license"))).toBe("true");
    expect(await page.evaluate(() => localStorage.getItem("setting.auto_update_adb"))).toBe("true");
    await mkdir("../artifacts", { recursive: true });
    await page.screenshot({ path: "../artifacts/advanced-tool-updates-preview.png", fullPage: true, animations: "disabled" });
});
