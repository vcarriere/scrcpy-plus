import { test, expect } from "@playwright/test";
import { mkdir } from "node:fs/promises";
import { mock } from "./fixtures.mjs";

async function position(toggle) {
    return toggle.evaluate(input => ({
        top: input.getBoundingClientRect().top,
        document_top: input.getBoundingClientRect().top + scrollY,
        scroll: scrollY,
        maximum: Math.max(0, document.documentElement.scrollHeight - innerHeight),
        focused: document.activeElement === input,
    }));
}

async function rendered(page) {
    await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
}

function expect_stable(before, after) {
    // Removing content can shorten the document below the old scroll position.
    // Preserve the anchor as closely as the browser's scroll limits permit.
    const scroll = Math.max(0, Math.min(after.document_top - before.top, after.maximum));
    const expected_top = after.document_top - scroll;
    expect(Math.abs(after.top - expected_top)).toBeLessThanOrEqual(1);
    expect(after.focused).toBe(true);
}

for (const scenario of [
    { name: "pointer", width: 600, height: 800, keyboard: false },
    { name: "keyboard in a narrow window", width: 380, height: 650, keyboard: true },
    { name: "larger text without browser scroll anchoring", width: 380, height: 800, keyboard: false, scaled: true },
]) {
    test(`advanced controls preserve the toggle position with ${scenario.name}`, async ({ page }) => {
        await page.setViewportSize({ width: scenario.width, height: scenario.height });
        await mock(page);
        await page.goto("/settings");
        await expect(page.getByText("Using bundled default", { exact: true })).toHaveCount(0);
        if (scenario.scaled) await page.addStyleTag({ content: "html { font-size: 20px; overflow-anchor: none; }" });
        const toggle = page.getByRole("checkbox", { name: "Show advanced controls", exact: true });
        await toggle.scrollIntoViewIfNeeded();
        await page.evaluate(() => scrollTo(0, 120));
        await toggle.focus();
        await rendered(page);
        const before = await position(toggle);
        if (scenario.keyboard) await toggle.press("Space");
        else await toggle.click();
        await expect(page.getByRole("button", { name: "Stop ADB", exact: true })).toBeVisible();
        await expect(page.getByText("Using bundled default", { exact: true })).toHaveCount(2);
        await rendered(page);
        const opened = await position(toggle);
        console.log(`${scenario.name}: toggle ${before.top}px -> ${opened.top}px`);
        expect_stable(before, opened);
        if (scenario.name === "pointer") {
            await mkdir("../artifacts", { recursive: true });
            await page.screenshot({ path: "../artifacts/settings-advanced-stable-preview.png" });
        }
        if (scenario.keyboard) await toggle.press("Space");
        else await toggle.click();
        await expect(page.getByRole("button", { name: "Stop ADB", exact: true })).toHaveCount(0);
        await expect(page.getByLabel("ADB executable", { exact: true })).toHaveCount(0);
        await rendered(page);
        expect_stable(opened, await position(toggle));
        expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    });
}

test("collapsing advanced controls respects the shorter page's scroll limit", async ({ page }) => {
    await page.setViewportSize({ width: 600, height: 500 });
    await mock(page, { advanced_adb: true });
    await page.goto("/settings");
    const toggle = page.getByRole("checkbox", { name: "Show advanced controls", exact: true });
    await expect(page.getByRole("button", { name: "Stop ADB", exact: true })).toBeVisible();
    await toggle.focus();
    await toggle.evaluate(input => scrollTo(0, input.getBoundingClientRect().top + scrollY - 30));
    await rendered(page);
    const before = await position(toggle);
    await toggle.press("Space");
    await expect(page.getByRole("button", { name: "Stop ADB", exact: true })).toHaveCount(0);
    await rendered(page);
    expect_stable(before, await position(toggle));
});
