import { test, expect } from "@playwright/test";
import { mkdir } from "node:fs/promises";
import { mock } from "./fixtures.mjs";

const mdns = "adb-" + "long-name-".repeat(5) + "ABC123456" + "._adb-tls-connect._tcp";
const ip = "192.0.2.24:39563";
const info = { model: "Pixel 7 Pro", model_id: "cheetah", version: "17", sdk: "37", build: "synthetic", security_patch: "2026-11-01" };
function route(serial, transport_id, kind, state = "device") {
    return { serial, transport_id, kind, state, info: state === "device" ? info : null,
        selection: { serial, transport_id, identity: "synthetic-physical-phone", adb_path: "C:\\SCRCPY+\\adb.exe", server_socket: "tcp:localhost:5037" } };
}
function phone(routes = [route(mdns, 1, "mdns"), route(ip, 2, "tcp")]) {
    return { id: "device:synthetic-physical-phone", name: "Pixel 7 Pro", label: "Pixel 7 Pro", verified: true, routes };
}

test("long device labels and full connection paths fit a narrow window", async ({ page }) => {
    await page.setViewportSize({ width: 380, height: 800 });
    await mock(page, { devices: `${mdns}\tdevice\n${ip}\tdevice\n`, inventory: [phone()] });
    await page.goto("/");
    await page.getByRole("tab").first().click();
    await expect(page.getByRole("button", { name: "Start SCRCPY", exact: true })).toBeEnabled();
    const card = page.locator(".card").filter({ has: page.getByText("Devices", { exact: true }) });
    await mkdir("../artifacts", { recursive: true });
    await card.screenshot({ path: "../artifacts/device-paths-narrow.png" });
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    await expect(page.getByRole("tab")).toHaveCount(1);
    await expect(page.getByRole("tab", { name: "Pixel 7 Pro", exact: true })).toHaveAttribute("aria-selected", "true");
    await expect(card.getByText(mdns, { exact: true })).toBeVisible();
    await expect(card.getByText(ip, { exact: true })).toBeVisible();
    await page.setViewportSize({ width: 600, height: 800 });
    await card.screenshot({ path: "../artifacts/device-paths-grouped.png" });
});

test("route choice launches and copies the original selector without shortening it", async ({ page }) => {
    await mock(page, { inventory: [phone()] });
    await page.goto("/");
    const rows = page.locator("fieldset > .rounded-lg");
    await rows.filter({ hasText: mdns }).getByRole("button", { name: "Copy connection", exact: true }).click();
    expect(await page.evaluate(() => adb_test.clipboard)).toBe(mdns);
    await expect(page.getByRole("button", { name: "Copied", exact: true })).toBeVisible();
    await rows.filter({ hasText: ip }).getByRole("radio").check();
    await page.getByRole("button", { name: "Start SCRCPY", exact: true }).click();
    const call = await page.evaluate(() => adb_test.calls.find(call => call.command === "run_scrcpy"));
    expect(call.args.args.slice(0, 2)).toEqual(["-s", ip]);
    expect(call.args.selection.serial).toBe(ip);
    expect(call.args.selection.transport_id).toBe(2);
});

test("disconnecting one route keeps the device and its remaining connection", async ({ page }) => {
    await mock(page, { inventory: [phone()] });
    await page.goto("/");
    await page.locator("fieldset > .rounded-lg").filter({ hasText: mdns }).getByRole("button", { name: "Disconnect", exact: true }).click();
    await expect(page.getByRole("radio")).toHaveCount(1);
    await expect(page.getByRole("tab", { name: "Pixel 7 Pro", exact: true })).toHaveAttribute("aria-selected", "true");
    await expect(page.getByRole("radio")).toBeChecked();
    await expect(page.getByText(ip, { exact: true })).toBeVisible();
    await page.getByRole("button", { name: "Start SCRCPY", exact: true }).click();
    expect(await page.evaluate(() => adb_test.calls.find(call => call.command === "disconnect_device_route").args.selection.serial)).toBe(mdns);
    expect(await page.evaluate(() => adb_test.calls.find(call => call.command === "run_scrcpy").args.args[1])).toBe(ip);
});

test("a changed wireless endpoint preserves physical selection while a replaced phone requires selection", async ({ page }) => {
    await mock(page, { inventory: [phone()] });
    await page.goto("/");
    await expect(page.getByRole("radio")).toHaveCount(2);
    await page.getByRole("checkbox", { name: /^Turn off phone screen/ }).check();
    await page.evaluate(() => {
        adb_test.inventory[0].routes = [adb_test.inventory[0].routes[1]];
        adb_test.inventory[0].routes[0].serial = "192.0.2.24:40000";
        adb_test.inventory[0].routes[0].transport_id = 3;
        adb_test.inventory[0].routes[0].selection.serial = "192.0.2.24:40000";
        adb_test.inventory[0].routes[0].selection.transport_id = 3;
    });
    await expect(page.getByRole("radio")).toHaveCount(1, { timeout: 8000 });
    await expect(page.getByRole("radio")).toBeChecked();
    await page.evaluate(() => { adb_test.inventory[0].id = "device:different-phone"; adb_test.inventory[0].routes[0].selection.identity = "different-phone"; });
    await expect(page.getByRole("button", { name: "Start SCRCPY", exact: true })).toHaveCount(0, { timeout: 8000 });
    await expect(page.getByText("No device selected", { exact: true })).toBeVisible();
    await page.getByRole("tab", { name: "Pixel 7 Pro", exact: true }).click();
    await expect(page.getByRole("button", { name: "Start SCRCPY", exact: true })).toBeEnabled();
    await expect(page.getByRole("checkbox", { name: /^Turn off phone screen/ })).toBeChecked();
});

test("equal long model names keep their distinguishing suffix visible", async ({ page }) => {
    const name = "Unusually long identical model name ".repeat(5);
    const first = { ...phone(), name, label: name + " · 1234" };
    const second = { ...phone([route("USB5678", 3, "usb")]), id: "device:other-phone", name, label: name + " · 5678" };
    await page.setViewportSize({ width: 380, height: 800 });
    await mock(page, { inventory: [first, second] });
    await page.goto("/");
    await expect(page.getByRole("tab", { name: first.label, exact: true })).toBeVisible();
    await expect(page.getByRole("tab", { name: second.label, exact: true })).toBeVisible();
    await expect(page.getByText("· 1234", { exact: true })).toBeVisible();
    await expect(page.getByText("· 5678", { exact: true })).toBeVisible();
    expect(await page.locator('[role="tab"] .shrink-0').evaluateAll(nodes => nodes.every(node => node.scrollWidth <= node.clientWidth))).toBe(true);
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
});

test("same-model device labels stay distinct and support keyboard selection", async ({ page }) => {
    const first = { ...phone(), label: "Pixel 7 Pro · 1234" };
    const second = { ...phone([route("USB5678", 3, "usb")]), id: "device:other-phone", label: "Pixel 7 Pro · 5678" };
    await mock(page, { inventory: [first, second] });
    await page.goto("/");
    const tab = page.getByRole("tab", { name: first.label, exact: true });
    await tab.click();
    await tab.press("ArrowRight");
    await expect(page.getByRole("tab", { name: second.label, exact: true })).toBeFocused();
    await expect(page.getByRole("tab", { name: second.label, exact: true })).toHaveAttribute("aria-selected", "true");
    await expect(page.getByRole("button", { name: "Disconnect", exact: true })).toHaveCount(0);
    await page.setViewportSize({ width: 600, height: 800 });
    await page.locator(".card").filter({ has: page.getByText("Devices", { exact: true }) }).screenshot({ path: "../artifacts/device-paths-wide.png" });
});

test("long Unicode model names remain accessible with larger text and high display scaling", async ({ browser }) => {
    const context = await browser.newContext({ viewport: { width: 380, height: 800 }, deviceScaleFactor: 2 });
    try {
        const page = await context.newPage();
        const label = "長いデバイス名".repeat(12);
        const device = phone([route(mdns, 1, "mdns")]);
        device.name = label;
        device.label = label;
        device.routes[0].info = { ...info, model: label };
        await mock(page, { inventory: [device] });
        await page.goto("/");
        await expect(page.getByRole("tab", { name: label, exact: true })).toHaveAttribute("aria-selected", "true");
        await page.addStyleTag({ content: "html { font-size: 20px; }" });
        expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
        await expect(page.getByText(`${label} (cheetah)`, { exact: true })).toBeVisible();
        await page.locator(".card").filter({ has: page.getByText("Devices", { exact: true }) }).screenshot({ path: "../artifacts/device-paths-scaled.png" });
    } finally {
        await context.close();
    }
});
