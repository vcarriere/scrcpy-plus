import { test, expect } from "@playwright/test";
import { mkdir } from "node:fs/promises";
import { mock } from "./fixtures.mjs";

async function wireless(page) {
    await page.goto("/settings");
    await page.locator('a[href="/"]').click();
    await page.getByRole("link", { name: "Connect Wirelessly", exact: true }).click();
}

test("wireless validation explains invalid input without running ADB", async ({ page }) => {
    await mock(page);
    await wireless(page);
    await page.getByRole("button", { name: "Connect", exact: true }).click();
    await expect(page.getByRole("alert")).toContainText("Enter an IP address");
    await page.getByLabel("IP Address & Port", { exact: true }).fill("192.0.2.22:65536");
    await page.getByLabel("Pairing Code", { exact: true }).fill("123456");
    await page.getByRole("button", { name: "Connect", exact: true }).click();
    await expect(page.getByRole("alert")).toContainText("1 to 65535");
    await page.getByLabel("IP Address & Port", { exact: true }).fill("192.0.2.22:34567");
    await page.getByLabel("Pairing Code", { exact: true }).fill("123");
    await page.getByRole("button", { name: "Connect", exact: true }).click();
    await expect(page.getByRole("alert")).toContainText("six-digit pairing code");
    expect(await page.evaluate(() => adb_test.calls.filter(call => call.command === "exec_adb" && ["pair", "connect"].includes(call.args.args[0])))).toEqual([]);
    await page.setViewportSize({ width: 380, height: 800 });
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    await mkdir("../artifacts", { recursive: true });
    await page.screenshot({ path: "../artifacts/original-review-wireless-error.png", fullPage: true, animations: "disabled" });
});

test("pairing keeps a saved IPv6 host and serializes a trimmed pairing request", async ({ page }) => {
    await mock(page, { last_ip_port: "[fe80::1234]:5555", wireless_deferred: true });
    await wireless(page);
    await expect(page.getByLabel("IP Address & Port", { exact: true })).toHaveValue("[fe80::1234]:");
    await page.getByLabel("IP Address & Port", { exact: true }).fill(" [fe80::1234]:34567 ");
    await page.getByLabel("Pairing Code", { exact: true }).fill(" 123456 ");
    await page.getByRole("button", { name: "Connect", exact: true }).click();
    await expect(page.getByRole("button", { name: "Connect", exact: true })).toBeDisabled();
    await expect.poll(() => page.evaluate(() => !!adb_test.finish_wireless)).toBe(true);
    expect(await page.evaluate(() => adb_test.calls.filter(call => call.command === "exec_adb" && call.args.args[0] === "pair").map(call => call.args.args))).toEqual([["pair", "[fe80::1234]:34567", "123456"]]);
    await page.evaluate(() => adb_test.finish_wireless());
    await expect(page.getByRole("button", { name: "Back", exact: true })).toBeVisible();
    await expect(page.getByLabel("IP Address & Port", { exact: true })).toHaveValue("[fe80::1234]:5555");
});

test("zero-exit failure output is visible and cannot be mistaken for connected", async ({ page }) => {
    await mock(page, { connect_output: "failed: not connected to 192.0.2.22:34567" });
    await wireless(page);
    await page.getByRole("button", { name: "Skip", exact: true }).click();
    await page.getByLabel("IP Address & Port", { exact: true }).fill("192.0.2.22:34567");
    await page.getByRole("button", { name: "Connect", exact: true }).click();
    await expect(page.getByRole("alert")).toContainText("Connection failed.");
    await expect(page.getByText("Success", { exact: true })).toHaveCount(0);
    expect(await page.evaluate(() => localStorage.getItem("scrcpyplus.last_ip_port"))).toBe(null);
    await page.evaluate(() => { adb_test.connect_output = "already connected to 192.0.2.22:34567"; });
    await page.getByRole("button", { name: "Connect", exact: true }).click();
    await expect(page.getByText("Success", { exact: true })).toBeVisible();
    expect(await page.evaluate(() => localStorage.getItem("scrcpyplus.last_ip_port"))).toBe("192.0.2.22:34567");
});

for (const deferred of [false, true]) {
    test(`leaving wireless setup cancels navigation with ${deferred ? "a pending reply" : "a pending success timer"}`, async ({ page }) => {
        await mock(page, { wireless_deferred: deferred });
        await wireless(page);
        await page.clock.install();
        await page.getByRole("button", { name: "Skip", exact: true }).click();
        await page.getByLabel("IP Address & Port", { exact: true }).fill("192.0.2.22:34567");
        await page.getByRole("button", { name: "Connect", exact: true }).click();
        if (deferred) await expect.poll(() => page.evaluate(() => !!adb_test.finish_wireless)).toBe(true);
        else await expect(page.getByText("Success", { exact: true })).toBeVisible();
        await page.getByRole("button").first().click();
        await expect(page).toHaveURL(/\/$/);
        if (deferred) await page.evaluate(() => adb_test.finish_wireless());
        await page.clock.fastForward(3000);
        await expect(page).toHaveURL(/\/$/);
        await expect(page.getByRole("link", { name: "Connect Wirelessly", exact: true })).toBeVisible();
    });
}

test("legacy rotation settings migrate to the supported scrcpy orientation argument", async ({ page }) => {
    await mock(page, { devices: "synthetic-device\tdevice\n", remember_config: true, saved_config: { active_args: ["--lock-video-orientation"] } });
    await page.goto("/");
    await page.locator(".collapse").filter({ has: page.getByText("Display Settings", { exact: true }) }).locator('input[type="checkbox"]').first().check();
    await expect(page.getByRole("checkbox", { name: /^Lock screen rotation/ })).toBeChecked();
    await page.getByRole("button", { name: "Start SCRCPY", exact: true }).click();
    await expect(page.getByRole("alert")).toHaveText("Simulated launch failure");
    const args = await page.evaluate(() => adb_test.calls.find(call => call.command === "run_scrcpy").args.args);
    expect(args).toContain("--capture-orientation=@");
    expect(args).not.toContain("--lock-video-orientation");
});

test("turning off Remember configuration resets bitrate as well as other launch choices", async ({ page }) => {
    await mock(page, { devices: "synthetic-device\tdevice\n" });
    await page.goto("/settings");
    await page.getByRole("checkbox", { name: "Remember SCRCPY config", exact: true }).uncheck();
    await page.locator('a[href="/"]').click();
    await page.locator(".collapse").filter({ has: page.getByText("Display Settings", { exact: true }) }).locator('input[type="checkbox"]').first().check();
    await page.getByRole("slider").evaluate(input => { input.value = "16"; input.dispatchEvent(new Event("input", { bubbles: true })); });
    await expect(page.getByRole("slider")).toHaveValue("16");
    await page.getByRole("link", { name: "Settings", exact: true }).click();
    await page.locator('a[href="/"]').click();
    await page.locator(".collapse").filter({ has: page.getByText("Display Settings", { exact: true }) }).locator('input[type="checkbox"]').first().check();
    await expect(page.getByRole("slider")).toHaveValue("8");
});

test("unavailable version information leaves a usable home page and a visible error", async ({ page }) => {
    const errors = [];
    page.on("pageerror", error => errors.push(error.message));
    await mock(page, { version_fail: true });
    await page.goto("/");
    await expect(page.getByRole("alert")).toHaveText("Version information could not be read.");
    await expect(page.getByText("Unavailable", { exact: true })).toHaveCount(2);
    await expect(page.getByRole("link", { name: "Settings", exact: true })).toBeVisible();
    expect(errors).toEqual([]);
});
