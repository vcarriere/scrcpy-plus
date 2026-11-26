import { test, expect } from "@playwright/test";
import { mkdir } from "node:fs/promises";
import { mock } from "./fixtures.mjs";

async function start(page, options = {}) {
    await mock(page, { devices: "synthetic-device\tdevice\n", mirror_pending: true,
        launch_progress: { phase: "checking", reason: "verifying_connection", elapsed_ms: 20, finished: false }, ...options });
    await page.goto("/");
    await page.getByRole("button", { name: "Start SCRCPY", exact: true }).click();
    return page.getByRole("dialog", { name: "Starting SCRCPY", exact: true });
}

async function phase(page, phase, reason = "") {
    await page.evaluate(({ phase, reason }) => { adb_test.launch_progress = { phase, reason, elapsed_ms: 3000, finished: false }; }, { phase, reason });
}

test("startup immediately shows an animated step and waits for actual video readiness", async ({ page }) => {
    const dialog = await start(page);
    await expect(dialog).toBeVisible();
    await expect(dialog.getByText("Checking the device connection…", { exact: true })).toBeVisible();
    await expect(dialog.locator('.startup-spinner')).toBeVisible();
    expect(await dialog.locator('.startup-spinner').evaluate(element => getComputedStyle(element).animationName)).not.toBe("none");
    await phase(page, "unlocking", "verifying_unlock");
    await expect(dialog.getByText("Verifying that the phone is unlocked…", { exact: true })).toBeVisible();
    await phase(page, "starting_mirror", "waiting_for_video");
    await expect(dialog.getByText("Opening the mirrored display…", { exact: true })).toBeVisible();
    await phase(page, "ready", "video_initialized");
    await expect(dialog).not.toBeVisible();
    expect(await page.evaluate(() => adb_test.calls.filter(call => call.command === "run_scrcpy").length)).toBe(1);
    await expect(page.getByRole("button", { name: "Start SCRCPY", exact: true })).toBeDisabled();
    await phase(page, "unlocking", "preparing_private_entry");
    await expect(dialog.getByText("Preparing private password entry…", { exact: true })).toBeVisible();
    await phase(page, "ready", "video_initialized");
    await expect(dialog).not.toBeVisible();
    await page.evaluate(() => adb_test.finish_mirror());
    await expect(page.getByRole("button", { name: "Start SCRCPY", exact: true })).toBeEnabled();
});

test("manual recovery is shown in the same panel without an endlessly spinning indicator", async ({ page }) => {
    await page.setViewportSize({ width: 380, height: 800 });
    await start(page);
    await phase(page, "manual_unlock", "screen_off_unavailable");
    await expect(page.getByRole("dialog", { name: "Check phone screen", exact: true })).toBeVisible();
    await expect(page.getByText("Waiting for the phone screen-off state.", { exact: true })).toBeVisible();
    await expect(page.getByText("Unlock the phone manually.", { exact: true })).toHaveCount(0);
    await phase(page, "manual_unlock", "automatic_unlock_blocked");
    const dialog = page.getByRole("dialog");
    await expect(dialog).toBeVisible();
    await expect(dialog.getByText("Unlock the phone manually.", { exact: true })).toBeVisible();
    await expect(dialog.getByText(/paused after an unsuccessful or uncertain attempt/)).toBeVisible();
    await expect(dialog.locator('.startup-spinner')).toHaveCount(0);
    await mkdir("../artifacts", { recursive: true });
    await page.screenshot({ path: "../artifacts/scrcpy-manual-unlock-preview.png", animations: "disabled" });
    await phase(page, "unlocking", "restoring_screen_off");
    await expect(dialog.getByText("Restoring the screen-off setting…", { exact: true })).toBeVisible();
    await expect(dialog.locator('.startup-spinner')).toBeVisible();
    await phase(page, "ready");
    await expect(dialog).not.toBeVisible();
});

test("progress can be hidden and reopened after navigation without starting a second session", async ({ page }) => {
    const dialog = await start(page);
    await dialog.getByRole("button", { name: "Hide progress", exact: true }).click();
    await expect(dialog).not.toBeVisible();
    await page.getByRole("link", { name: "Settings", exact: true }).click();
    await page.getByRole("button", { name: "SCRCPY startup", exact: true }).click();
    await expect(dialog).toBeVisible();
    await phase(page, "ready");
    await expect(dialog).not.toBeVisible();
    await phase(page, "manual_unlock", "password_editor_unavailable");
    await expect(page.getByRole("dialog", { name: "Manual unlock needed", exact: true })).toBeVisible();
    expect(await page.evaluate(() => adb_test.calls.filter(call => call.command === "run_scrcpy").length)).toBe(1);
});

test("unconfirmed custom launches explain the uncertainty instead of claiming the display is ready", async ({ page }) => {
    const dialog = await start(page);
    await phase(page, "launched", "video_unconfirmed");
    await expect(dialog.getByText("SCRCPY has started; display readiness is unconfirmed.", { exact: true })).toBeVisible();
    await expect(dialog.locator('.startup-spinner')).toHaveCount(0);
    await expect(dialog.getByText("The mirrored display is ready.", { exact: true })).toHaveCount(0);
    await phase(page, "preparing");
    await expect(dialog.getByText("Preparing the phone…", { exact: true })).toBeVisible();
    await page.evaluate(() => { adb_test.launch_progress_fail = true; });
    await expect(dialog.getByText("Startup progress is unavailable.", { exact: true })).toBeVisible();
    await expect(dialog.locator('.startup-spinner')).toHaveCount(0);
    await expect(page.getByRole("button", { name: "Start SCRCPY", exact: true })).toBeDisabled();
    await expect(dialog.getByText("SCRCPY has started; display readiness is unconfirmed.", { exact: true })).toHaveCount(0);
});

test("a launch failure remains readable and closing the panel allows retry", async ({ page }) => {
    await start(page, { mirror_pending: false });
    const dialog = page.getByRole("dialog", { name: "Could not start SCRCPY", exact: true });
    await expect(dialog.getByRole("alert")).toHaveText("Simulated launch failure");
    await expect(dialog.locator('.startup-spinner')).toHaveCount(0);
    await dialog.getByRole("button", { name: "Close", exact: true }).click();
    await page.evaluate(() => { adb_test.mirror_pending = true; });
    await page.getByRole("button", { name: "Start SCRCPY", exact: true }).click();
    await expect(page.getByRole("dialog", { name: "Starting SCRCPY", exact: true })).toBeVisible();
    expect(await page.evaluate(() => adb_test.calls.filter(call => call.command === "run_scrcpy").length)).toBe(2);
});

test("a late progress response cannot revive an ended launch", async ({ page }) => {
    await mock(page, { devices: "synthetic-device\tdevice\n", mirror_pending: true });
    await page.addInitScript(() => {
        const invoke = __TAURI_INTERNALS__.invoke;
        __TAURI_INTERNALS__.invoke = (command, args) => command === "get_scrcpy_progress"
            ? new Promise(resolve => { adb_test.finish_progress = () => resolve({ phase: "manual_unlock", reason: "device_state_unavailable", finished: false }); })
            : invoke(command, args);
    });
    await page.goto("/");
    await page.getByRole("button", { name: "Start SCRCPY", exact: true }).click();
    await expect.poll(() => page.evaluate(() => typeof adb_test.finish_progress)).toBe("function");
    await page.evaluate(() => adb_test.finish_mirror());
    await expect(page.getByRole("dialog")).not.toBeVisible();
    await page.evaluate(() => adb_test.finish_progress());
    await expect(page.getByRole("dialog")).not.toBeVisible();
    await expect(page.getByRole("button", { name: "Manual unlock needed", exact: true })).toHaveCount(0);
});

test("startup progress fits a narrow window and respects reduced motion", async ({ page }) => {
    await page.setViewportSize({ width: 380, height: 800 });
    await page.emulateMedia({ reducedMotion: "reduce" });
    const dialog = await start(page);
    await phase(page, "unlocking", "entering_password");
    await expect(dialog.getByText("Entering the password privately…", { exact: true })).toBeVisible();
    await expect(dialog.locator('.startup-spinner')).toHaveCSS("animation-name", "none");
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    await mkdir("../artifacts", { recursive: true });
    await page.screenshot({ path: "../artifacts/scrcpy-startup-preview.png", animations: "disabled" });
});
