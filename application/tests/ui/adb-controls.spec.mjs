import { test, expect } from "@playwright/test";
import { mkdir } from "node:fs/promises";

async function mock(page, options = {}) {
    await page.addInitScript(options => {
        localStorage.setItem("setting.disable_update_check", "true");
        const state = window.adb_test = {
            server: "running", paused: false, sessions: 0, calls: [], devices: "", fail: false, ...options,
        };
        const snapshot = () => ({
            state: state.server, paused: state.paused, active_sessions: state.sessions,
            endpoint: "tcp:localhost:5037", path: "C:\\SCRCPY+\\adb.exe", protocol_version: state.server === "running" ? 41 : null,
        });
        window.__TAURI_INTERNALS__ = { invoke: async (command, args = {}) => {
            state.calls.push({ command, args });
            if (command === "set_binary_paths") return;
            if (command === "get_adb_status") {
                if (state.server === "running") state.paused = false;
                return snapshot();
            }
            if (command === "control_adb") {
                if (state.fail) throw "Simulated ADB operation failure";
                state.paused = !args.start;
                state.server = args.start ? "running" : "stopped";
                return snapshot();
            }
            if (command === "get_dependency_versions") return { scrcpy_plus: "3.1.0", scrcpy: "5.0.1", adb: "37.0.1" };
            if (command === "environment") return { is_windows: true, os: "windows", arch: "x86_64", app_version: "3.1.0" };
            if (command === "exec_adb") {
                if (args.args[0] === "--version") return "Android Debug Bridge version 1.0.41";
                if (state.paused) throw "ADB is paused";
                if (args.args[0] === "devices") return "List of devices attached\n" + state.devices;
                return "Synthetic device property";
            }
            if (command === "run_scrcpy") throw "Simulated launch failure";
            throw "Unexpected command: " + command;
        } };
    }, options);
}

async function advanced(page) {
    await page.goto("/settings");
    await page.getByRole("checkbox", { name: "Show advanced ADB controls" }).check();
    await expect(page.getByRole("button", { name: "Stop ADB", exact: true })).toBeVisible();
}

test("controls are optional and the status is readable without color", async ({ page }) => {
    await mock(page);
    await page.goto("/");
    await expect(page.getByRole("status")).toHaveText("Running");
    await page.goto("/settings");
    await expect(page.getByRole("button", { name: "Stop ADB", exact: true })).toHaveCount(0);
    await page.getByRole("checkbox", { name: "Show advanced ADB controls" }).check();
    await expect(page.getByText("Server endpoint", { exact: true })).toBeVisible();
    await expect(page.getByRole("button", { name: "Start ADB", exact: true })).toBeDisabled();
    await mkdir("../artifacts", { recursive: true });
    await page.screenshot({ path: "../artifacts/adb-controls-preview.png", fullPage: true });
});

test("Stop confirmation can be cancelled without stopping the server", async ({ page }) => {
    await mock(page);
    await advanced(page);
    await page.getByRole("button", { name: "Stop ADB", exact: true }).click();
    await expect(page.getByRole("dialog")).toContainText("Android Studio");
    await page.getByRole("button", { name: "Cancel", exact: true }).click();
    expect(await page.evaluate(() => adb_test.calls.filter(call => call.command === "control_adb"))).toEqual([]);
});

test("Stop survives page navigation and several device refresh cycles until Start", async ({ page }) => {
    await mock(page);
    await advanced(page);
    await page.getByRole("button", { name: "Stop ADB", exact: true }).click();
    await page.getByRole("dialog").getByRole("button", { name: "Stop ADB", exact: true }).click();
    await expect(page.getByRole("status")).toHaveText("Stopped");
    await page.locator('a[href="/"]').click();
    const count = await page.evaluate(() => adb_test.calls.filter(call => call.command === "exec_adb" && call.args.args[0] === "devices").length);
    await page.waitForTimeout(11000);
    expect(await page.evaluate(() => adb_test.calls.filter(call => call.command === "exec_adb" && call.args.args[0] === "devices").length)).toBe(count);
    await page.getByRole("link", { name: "Settings", exact: true }).first().click();
    await page.getByRole("button", { name: "Start ADB", exact: true }).click();
    await expect(page.getByRole("status")).toHaveText("Running");
    await page.locator('a[href="/"]').click();
    await expect.poll(() => page.evaluate(() => adb_test.calls.filter(call => call.command === "exec_adb" && call.args.args[0] === "devices").length)).toBeGreaterThan(count);
});

test("shows failed operations and protects active sessions", async ({ page }) => {
    await mock(page, { server: "stopped", paused: true, fail: true });
    await advanced(page);
    await page.getByRole("button", { name: "Start ADB", exact: true }).click();
    await expect(page.getByRole("alert")).toContainText("Simulated ADB operation failure");
    await page.evaluate(() => { adb_test.server = "running"; adb_test.sessions = 1; adb_test.fail = false; });
    await expect(page.getByRole("status")).toHaveText("Running");
    await expect(page.getByRole("button", { name: "Stop ADB", exact: true })).toBeDisabled();
});

test("an external ADB restart automatically resumes discovery and enables Stop", async ({ page }) => {
    await mock(page, { devices: "synthetic-device\tdevice\n" });
    await advanced(page);
    await page.getByRole("button", { name: "Stop ADB", exact: true }).click();
    await page.getByRole("dialog").getByRole("button", { name: "Stop ADB", exact: true }).click();
    await expect(page.getByRole("status")).toHaveText("Stopped");
    await page.locator('a[href="/"]').click();
    const count = await page.evaluate(() => adb_test.calls.filter(call => call.command === "exec_adb" && call.args.args[0] === "devices").length);
    await page.evaluate(() => { adb_test.server = "running"; });
    await expect(page.getByRole("status")).toHaveText("Running");
    await expect.poll(() => page.evaluate(() => adb_test.calls.filter(call => call.command === "exec_adb" && call.args.args[0] === "devices").length)).toBeGreaterThan(count);
    await expect(page.getByRole("button", { name: "Start SCRCPY", exact: true })).toBeEnabled();
    await expect(page.getByRole("button", { name: "Resume device refresh", exact: true })).toHaveCount(0);
    expect(await page.evaluate(() => adb_test.calls.filter(call => call.command === "control_adb").map(call => call.args.start))).toEqual([false]);
    await page.getByRole("link", { name: "Settings", exact: true }).click();
    await expect(page.getByRole("button", { name: "Stop ADB", exact: true })).toBeEnabled();
    await expect(page.getByRole("button", { name: "Start ADB", exact: true })).toBeDisabled();
    await page.setViewportSize({ width: 380, height: 800 });
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    await page.screenshot({ path: "../artifacts/adb-external-restart-recovered.png", fullPage: true });
});

test("offline and unauthorized devices are distinguished without shell commands", async ({ page }) => {
    await mock(page, { devices: "offline-device\toffline\nunauthorized-device\tunauthorized\n" });
    await page.goto("/");
    await page.getByRole("tab", { name: "offline-device", exact: true }).click();
    await expect(page.getByText("Offline", { exact: true })).toBeVisible();
    await page.getByRole("tab", { name: "unauthorized-device", exact: true }).click();
    await expect(page.getByText("Unauthorized", { exact: true })).toBeVisible();
    expect(await page.evaluate(() => adb_test.calls.some(call => call.command === "exec_adb" && call.args.args.includes("shell")))).toBe(false);
});

test("narrow controls fit the current theme without horizontal overflow", async ({ page }) => {
    await page.setViewportSize({ width: 380, height: 800 });
    await mock(page);
    await advanced(page);
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    await page.screenshot({ path: "../artifacts/adb-controls-narrow-preview.png", fullPage: true });
});
