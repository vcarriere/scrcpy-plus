import { test, expect } from "@playwright/test";
import { mkdir } from "node:fs/promises";
import { mock } from "./fixtures.mjs";

const releases_url = "https://api.github.com/repos/Frontesque/scrcpy-plus/releases";
const release = { tag_name: "v3.1.0", prerelease: false, draft: false, html_url: "https://github.com/Frontesque/scrcpy-plus/releases/tag/v3.1.0" };

for (const disabled of [true, false]) {
    test(`manual checks work with automatic checking ${disabled ? "off" : "on"} without changing the preference`, async ({ page }) => {
        const requests = [];
        let finish;
        const pending = new Promise(resolve => { finish = resolve; });
        await page.route("https://api.github.com/repos/**/releases", async route => {
            requests.push(route.request().url());
            await pending;
            await route.fulfill({ json: [release] });
        });
        await mock(page, { update_preference: disabled });
        await page.goto("/settings");
        const button = page.getByRole("button", { name: "Check for updates now", exact: true });
        expect(requests).toEqual([]);
        await button.focus();
        await button.press("Enter");
        await expect(button).toBeDisabled();
        await expect(page.getByRole("status")).toHaveText("Checking for SCRCPY+ updates…");
        await button.evaluate(button => { button.click(); button.click(); });
        await expect.poll(() => requests.length).toBe(1);
        finish();
        await expect(page.getByRole("status")).toHaveText("SCRCPY+ is up to date.");
        await expect(button).toBeEnabled();
        expect(requests).toEqual([releases_url]);
        expect(await page.evaluate(() => localStorage.getItem("setting.disable_update_check"))).toBe(String(disabled));
        expect(await page.evaluate(() => adb_test.calls.some(call => ["get_dependency_versions", "download_scrcpy", "exec_adb", "exec_scrcpy"].includes(call.command)))).toBe(false);
    });
}

test("a pending automatic check is shared with Settings and retains stable-release filtering", async ({ page }) => {
    let finish;
    let requests = 0;
    const pending = new Promise(resolve => { finish = resolve; });
    await page.route(releases_url, async route => {
        requests++;
        await pending;
        await route.fulfill({ json: [
            { ...release, tag_name: "v8.0.0", prerelease: true },
            { ...release, tag_name: "v7.0.0", draft: true },
            { ...release, tag_name: "v3.10.0" },
        ] });
    });
    await mock(page, { update_preference: false });
    await page.goto("/");
    await expect.poll(() => requests).toBe(1);
    await page.getByRole("link", { name: "Settings", exact: true }).click();
    await expect(page.getByRole("button", { name: "Check for updates now", exact: true })).toBeDisabled();
    finish();
    await expect(page.getByRole("status")).toHaveText("Update Available");
    await expect(page.getByText("v3.10.0", { exact: true })).toBeVisible();
    await expect(page.getByText("v8.0.0", { exact: true })).toHaveCount(0);
    await expect(page.getByRole("button", { name: "Download", exact: true })).toBeVisible();
    expect(requests).toBe(1);
});

test("manual update results reach the existing home banner with automatic checks disabled", async ({ page }) => {
    let requests = 0;
    await page.route(releases_url, async route => {
        requests++;
        await route.fulfill({ json: [{ ...release, tag_name: "v3.2.0" }] });
    });
    await mock(page);
    await page.goto("/settings");
    await page.getByRole("button", { name: "Check for updates now", exact: true }).click();
    await expect(page.getByRole("status")).toHaveText("Update Available");
    await page.locator('a[href="/"]').click();
    await expect(page.getByText("Update Available", { exact: true })).toBeVisible();
    await expect(page.getByText("v3.2.0", { exact: true })).toBeVisible();
    expect(requests).toBe(1);
    expect(await page.evaluate(() => localStorage.getItem("setting.disable_update_check"))).toBe("true");
});

test("a failed check reports an error and can be retried without automatic HTTP retries", async ({ page }) => {
    let requests = 0;
    await page.route(releases_url, async route => {
        requests++;
        if (requests === 1) await route.fulfill({ status: 503, json: { message: "Unavailable" } });
        else await route.fulfill({ json: [release] });
    });
    await mock(page);
    await page.goto("/settings");
    const button = page.getByRole("button", { name: "Check for updates now", exact: true });
    await button.click();
    await expect(page.getByRole("alert")).toHaveText("Could not check for SCRCPY+ updates. Check your connection and try again.");
    await expect(button).toBeEnabled();
    expect(requests).toBe(1);
    await button.click();
    await expect(page.getByRole("status")).toHaveText("SCRCPY+ is up to date.");
    await expect(page.getByRole("alert")).toHaveCount(0);
    expect(requests).toBe(2);
});

test("a feed without a stable release does not claim the app is up to date", async ({ page }) => {
    await page.route(releases_url, route => route.fulfill({ json: [{ ...release, prerelease: true }] }));
    await mock(page);
    await page.goto("/settings");
    await page.getByRole("button", { name: "Check for updates now", exact: true }).click();
    await expect(page.getByRole("alert")).toContainText("Could not check for SCRCPY+ updates.");
    await expect(page.getByText("SCRCPY+ is up to date.", { exact: true })).toHaveCount(0);
});

for (const scenario of [
    { locale: "en", width: 600, label: "Check for updates now" },
    { locale: "it", width: 380, label: "Controlla gli aggiornamenti ora" },
    { locale: "ja", width: 380, label: "今すぐ更新を確認する", scaled: true },
]) {
    test(`the ${scenario.locale} update controls fit the existing theme at ${scenario.width}px`, async ({ page }) => {
        await mock(page, { advanced_adb: true });
        await page.setViewportSize({ width: scenario.width, height: 800 });
        await page.goto("/settings");
        await page.getByRole("combobox").selectOption(scenario.locale);
        if (scenario.scaled) await page.addStyleTag({ content: "html { font-size: 20px; }" });
        const button = page.getByRole("button", { name: scenario.label, exact: true });
        await expect(button).toBeVisible();
        const label_bounds = await page.getByRole("checkbox").first().locator("..").boundingBox();
        const button_bounds = await button.boundingBox();
        if (scenario.width === 600) {
            expect(Math.abs(label_bounds.y + label_bounds.height / 2 - button_bounds.y - button_bounds.height / 2)).toBeLessThanOrEqual(1);
            expect(button_bounds.x - label_bounds.x - label_bounds.width).toBeGreaterThanOrEqual(7);
        } else {
            expect(button_bounds.y - label_bounds.y - label_bounds.height).toBeGreaterThanOrEqual(7);
        }
        await expect(page.locator('[role="status"] [aria-hidden="true"]')).toHaveClass(/bg-primary/);
        const colors = await page.evaluate(() => {
            const buttons = [...document.querySelectorAll("button.btn-primary:not(:disabled)")];
            const marker = document.querySelector('[role="status"] [aria-hidden="true"]');
            return { backgrounds: buttons.map(button => getComputedStyle(button).backgroundColor), marker: getComputedStyle(marker).backgroundColor };
        });
        expect(new Set(colors.backgrounds).size).toBe(1);
        expect(colors.marker).toBe(colors.backgrounds[0]);
        expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
        await mkdir("../artifacts", { recursive: true });
        await page.screenshot({ path: `../artifacts/update-controls-${scenario.locale}-preview.png`, fullPage: true });
    });
}
