import { test, expect } from "@playwright/test";
import { mkdir } from "node:fs/promises";
import { mock } from "./fixtures.mjs";

async function automatic_session(page) {
    await mock(page, { devices: "synthetic-device\tdevice\n", mirror_pending: true });
    await page.goto("/");
    await page.getByRole("checkbox", { name: /^Turn off phone screen/ }).check();
    const automatic = page.getByRole("checkbox", { name: "Temporarily turn on phone screen for protected screens", exact: true });
    await automatic.check();
    await page.getByRole("button", { name: "Start SCRCPY", exact: true }).click();
    await expect(page.getByRole("button", { name: "Pause screen switching", exact: true })).toBeVisible();
    return automatic.locator("..").locator("..");
}

for (const { width, scaled } of [{ width: 380 }, { width: 600 }, { width: 380, scaled: true }]) {
    test(`screen action buttons have a visible gap at ${width}px${scaled ? " with larger text" : ""}`, async ({ page }) => {
        await page.setViewportSize({ width, height: 800 });
        const panel = await automatic_session(page);
        if (scaled) await page.addStyleTag({ content: "html { font-size: 20px; }" });
        await mkdir("../artifacts", { recursive: true });
        await panel.screenshot({ path: `../artifacts/protected-screen-actions-${width}${scaled ? "-scaled" : ""}.png` });
        const pause = await page.getByRole("button", { name: "Pause screen switching", exact: true }).boundingBox();
        const check = await page.getByRole("button", { name: "Check protected screen", exact: true }).boundingBox();
        const horizontal_gap = Math.max(check.x - pause.x - pause.width, pause.x - check.x - check.width);
        const vertical_gap = Math.max(check.y - pause.y - pause.height, pause.y - check.y - check.height);
        console.log(`${width}px: button gaps ${horizontal_gap}px horizontally, ${vertical_gap}px vertically`);
        expect(Math.max(horizontal_gap, vertical_gap)).toBeGreaterThanOrEqual(8);
        expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    });
}

test("manual screen checks show pending and result feedback during automatic mirroring", async ({ page }) => {
    await automatic_session(page);
    await page.evaluate(() => { adb_test.screen_deferred = true; });
    await page.getByRole("button", { name: "Check protected screen", exact: true }).click({ timeout: 1500 });
    await expect(page.getByRole("button", { name: "Loading…", exact: true })).toBeDisabled();
    await expect(page.getByText("Checking the phone's protection status…", { exact: true })).toBeVisible();
    await expect.poll(() => page.evaluate(() => typeof adb_test.finish_screen)).toBe("function");
    await page.evaluate(() => adb_test.finish_screen());
    await expect(page.getByText("Protected content or a fingerprint prompt was detected on the phone.", { exact: true })).toBeVisible();
    await expect(page.getByText(/^Checked at /)).toBeVisible();
    expect(await page.evaluate(() => adb_test.calls.filter(call => call.command === "control_screen_power"))).toEqual([]);
});

test("a new device can be checked while the previous device's result is still pending", async ({ page }) => {
    await mock(page, { devices: "first-device\tdevice\nsecond-device\tdevice\n", screen_deferred: true });
    await page.goto("/");
    await page.getByRole("tab", { name: "first-device", exact: true }).click();
    await page.getByRole("checkbox", { name: /^Turn off phone screen/ }).check();
    await page.getByRole("button", { name: "Check protected screen", exact: true }).click();
    await expect.poll(() => page.evaluate(() => typeof adb_test.finish_screen)).toBe("function");
    await page.evaluate(() => { adb_test.finish_first_screen = adb_test.finish_screen; });
    await page.getByRole("tab", { name: "second-device", exact: true }).click();
    const check = page.getByRole("button", { name: "Check protected screen", exact: true });
    await expect(check).toBeEnabled();
    await check.click();
    await page.evaluate(() => adb_test.finish_first_screen());
    await expect(page.getByRole("button", { name: "Loading…", exact: true })).toBeDisabled();
    await page.evaluate(() => { adb_test.screen = { state: "unflagged" }; adb_test.finish_screen(); });
    await expect(page.getByText("No protected screen was detected. Some apps may still block mirroring.", { exact: true })).toBeVisible();
    await expect(page.getByText(/^Checked at /)).toBeVisible();
    await expect(page.getByText("Protected content or a fingerprint prompt was detected on the phone.", { exact: true })).toHaveCount(0);
});
