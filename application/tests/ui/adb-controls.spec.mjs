import { test, expect } from "@playwright/test";
import { mkdir } from "node:fs/promises";

async function mock(page, options = {}) {
    await page.addInitScript(options => {
        localStorage.setItem("setting.disable_update_check", "true");
        if (options.remember_config) {
            localStorage.setItem("setting.save_scrcpy_config", "true");
            localStorage.setItem("scrcpyplus.scrcpy_config", JSON.stringify(options.saved_config || {}));
        }
        const state = window.adb_test = {
            server: "running", paused: false, sessions: 0, calls: [], devices: "", fail: false,
            details: { version: "37.0.3", build: "1234", path: "C:\\SCRCPY+\\adb.exe" },
            client: { path: "C:\\SCRCPY+\\adb.exe", version: "37.0.3-1234", is_windows: true }, ...options,
        };
        const snapshot = () => ({
            state: state.server, paused: state.paused, active_sessions: state.sessions,
            endpoint: "tcp:localhost:5037", path: "C:\\SCRCPY+\\adb.exe", protocol_version: state.server === "running" ? 41 : null,
            server_details: state.server === "running" ? state.details : null,
            screen_sessions: (state.screen_sessions || []).map(session => ({ ...session })),
        });
        window.__TAURI_INTERNALS__ = { invoke: async (command, args = {}) => {
            state.calls.push({ command, args });
            if (command === "set_binary_paths") return;
            if (command === "get_adb_client") return state.client;
            if (command === "control_screen_power") {
                const session = state.screen_sessions.find(session => session.id === args.id);
                session.state = args.paused ? "paused" : "monitoring";
                session.reason = args.paused ? "requested" : "watching";
                return;
            }
            if (command === "inspect_protected_screen") {
                if (state.screen_fail) throw "Simulated protected-screen query failure";
                if (state.screen_deferred) return new Promise(resolve => { state.finish_screen = () => resolve(state.screen || { state: "secure" }); });
                return state.screen || { state: "secure", reason: "focused_window_flag", android_sdk: 37, power_commands: "reset_off" };
            }
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
            if (command === "run_scrcpy") {
                if (!state.mirror_pending) throw "Simulated launch failure";
                if (args.automaticScreen) state.screen_sessions = [{ id: 7, device: args.args[1], state: "screen_on", reason: "protected" }];
                return new Promise(resolve => { state.finish_mirror = () => { state.screen_sessions = []; resolve(); }; });
            }
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

test("distinguishes selected client from a different running server", async ({ page }) => {
    await mock(page, { details: { version: "37.0.1", build: "5678", path: "C:\\Another Android Tool\\platform-tools\\adb.exe" } });
    await advanced(page);
    await expect(page.getByText("37.0.3-1234", { exact: true })).toBeVisible();
    await expect(page.getByText("37.0.1-5678", { exact: true })).toBeVisible();
    await expect(page.getByText("C:\\Another Android Tool\\platform-tools\\adb.exe", { exact: true })).toBeVisible();
    await expect(page.getByText(/The running server uses a different ADB/)).toBeVisible();
    expect(await page.evaluate(() => adb_test.calls.filter(call => call.command === "get_adb_client").length)).toBe(1);
    expect(await page.evaluate(() => adb_test.calls.some(call => call.command === "control_adb"))).toBe(false);
    await page.setViewportSize({ width: 380, height: 800 });
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    await page.screenshot({ path: "../artifacts/adb-server-details-narrow.png", fullPage: true });
});

test("keeps legacy metadata and unavailable clients separate from server health", async ({ page }) => {
    await mock(page, { details: null, client: { path: "C:\\Missing Tools\\adb.exe", error: "Could not launch the selected executable" } });
    await advanced(page);
    await expect(page.getByRole("status")).toHaveText("Running");
    await expect(page.getByRole("alert")).toContainText("Could not launch");
    await expect(page.getByText("Not available", { exact: true })).toHaveCount(3);
    await expect(page.getByRole("button", { name: "Stop ADB", exact: true })).toBeEnabled();
    await page.evaluate(() => { adb_test.server = "stopped"; });
    await expect(page.getByRole("status")).toHaveText("Stopped");
    await expect(page.getByText("Running server version", { exact: true })).toHaveCount(0);
    expect(await page.evaluate(() => adb_test.calls.some(call => call.command === "control_adb"))).toBe(false);
});

test("protected-screen detection is opt-in and does not send power commands", async ({ page }) => {
    await mock(page, { devices: "synthetic-device\tdevice\n" });
    await page.goto("/");
    const check = page.getByRole("button", { name: "Check protected screen", exact: true });
    await expect(check).toBeDisabled();
    expect(await page.evaluate(() => adb_test.calls.some(call => call.command === "inspect_protected_screen"))).toBe(false);
    await page.getByRole("checkbox", { name: /^Turn off phone screen/ }).check();
    await check.click();
    await expect(page.getByText("A secure window flag was detected on the main display.", { exact: true })).toBeVisible();
    expect(await page.evaluate(() => adb_test.calls.filter(call => call.command === "inspect_protected_screen").map(call => call.args))).toEqual([{ device: "synthetic-device" }]);
    expect(await page.evaluate(() => adb_test.calls.some(call => call.command === "run_scrcpy" || call.args.args?.some(arg => ["power-on", "power-off", "power-reset"].includes(arg))))).toBe(false);
    await page.setViewportSize({ width: 380, height: 800 });
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    await page.locator(".card").filter({ has: page.getByText("Launch Options", { exact: true }) }).screenshot({ path: "../artifacts/protected-screen-check-preview.png" });
});

test("unflagged windows remain uncertain and stale device results are discarded", async ({ page }) => {
    await mock(page, { devices: "synthetic-device\tdevice\nsecond-device\tdevice\n", screen: { state: "unflagged" } });
    await page.goto("/");
    await page.getByRole("tab", { name: "synthetic-device", exact: true }).click();
    const parent = page.getByRole("checkbox", { name: /^Turn off phone screen/ });
    const check = page.getByRole("button", { name: "Check protected screen", exact: true });
    await parent.check();
    await check.click();
    await expect(page.getByText(/No secure window flag was found/)).toBeVisible();
    await parent.uncheck();
    await expect(page.getByText(/No secure window flag was found/)).toHaveCount(0);
    await expect(check).toBeDisabled();
    await parent.check();
    await page.evaluate(() => { adb_test.screen = { state: "unexpected-state" }; });
    await check.click();
    await expect(page.getByText(/Could not determine the current window flag/)).toBeVisible();
    await page.evaluate(() => { adb_test.screen = { state: "secure" }; adb_test.screen_deferred = true; });
    await check.click();
    await expect.poll(() => page.evaluate(() => typeof adb_test.finish_screen)).toBe("function");
    await page.getByRole("tab", { name: "second-device", exact: true }).click();
    await page.evaluate(() => adb_test.finish_screen());
    await expect(page.getByText(/A secure window flag was detected/)).toHaveCount(0);
    await expect(page.getByText(/Could not determine the current window flag/)).toHaveCount(0);
});

test("detection errors are visible and virtual displays do not use the main-display probe", async ({ page }) => {
    await mock(page, { devices: "synthetic-device\tdevice\n", screen_fail: true });
    await page.goto("/");
    await page.getByRole("checkbox", { name: /^Turn off phone screen/ }).check();
    const check = page.getByRole("button", { name: "Check protected screen", exact: true });
    await check.click();
    await expect(page.getByRole("alert")).toHaveText("Simulated protected-screen query failure");
    await page.locator(".collapse").filter({ has: page.getByText("Display Settings", { exact: true }) }).locator('input[type="checkbox"]').first().check();
    await page.getByRole("checkbox", { name: /^Virtual display/ }).check();
    await expect(check).toBeDisabled();
    await expect(page.getByRole("alert")).toHaveCount(0);
});

test("automatic screen switching is opt-in and follows the native mirroring session", async ({ page }) => {
    await mock(page, { devices: "synthetic-device\tdevice\n", mirror_pending: true });
    await page.goto("/");
    const automatic = page.getByRole("checkbox", { name: "Temporarily turn on phone screen for protected screens", exact: true });
    await expect(automatic).not.toBeChecked();
    await expect(automatic).toBeDisabled();
    await page.getByRole("checkbox", { name: /^Turn off phone screen/ }).check();
    await automatic.check();
    await page.getByRole("button", { name: "Start SCRCPY", exact: true }).click();
    await expect(automatic).toBeDisabled();
    await expect(page.getByText("Phone screen on for protected content or a fingerprint prompt.", { exact: true })).toBeVisible();
    expect(await page.evaluate(() => adb_test.calls.find(call => call.command === "run_scrcpy").args.automaticScreen)).toBe(true);
    await page.getByRole("button", { name: "Pause screen switching", exact: true }).click();
    await expect(page.getByText("Automatic screen switching is paused.", { exact: true })).toBeVisible();
    await page.getByRole("button", { name: "Resume screen switching", exact: true }).click();
    await expect(page.getByText("Automatic screen switching is active.", { exact: true })).toBeVisible();
    await page.evaluate(() => { adb_test.screen_sessions[0].state = "paused"; adb_test.screen_sessions[0].reason = "manual_override"; });
    await expect(page.getByText(/The phone went to sleep or screen power changed elsewhere/)).toBeVisible();
    await page.evaluate(() => { adb_test.screen_sessions[0].state = "error"; adb_test.screen_sessions[0].reason = "power_failed"; });
    await expect(page.getByText("Automatic screen switching needs attention.", { exact: true })).toBeVisible();
    await expect(page.getByText(/The display change or device identity could not be verified/)).toBeVisible();
    await page.evaluate(() => { adb_test.screen_sessions[0].state = "paused"; adb_test.screen_sessions[0].reason = "manual_override"; });
    await expect(page.getByText("Automatic screen switching is paused.", { exact: true })).toBeVisible();
    await page.setViewportSize({ width: 380, height: 800 });
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    await page.locator(".card").filter({ has: page.getByText("Launch Options", { exact: true }) }).screenshot({ path: "../artifacts/automatic-screen-switching-preview.png" });
    await page.evaluate(() => adb_test.finish_mirror());
    await expect(automatic).toBeEnabled();
    await expect(page.getByText("Automatic screen switching is paused.", { exact: true })).toHaveCount(0);
});

test("incompatible view-only sessions cannot enable automatic screen control", async ({ page }) => {
    await mock(page, { devices: "synthetic-device\tdevice\n" });
    await page.goto("/");
    await page.getByRole("checkbox", { name: /^Turn off phone screen/ }).check();
    const automatic = page.getByRole("checkbox", { name: "Temporarily turn on phone screen for protected screens", exact: true });
    await automatic.check();
    await page.getByRole("checkbox", { name: /^View only/ }).check();
    await expect(automatic).toBeDisabled();
    await page.getByRole("button", { name: "Start SCRCPY", exact: true }).click();
    await expect(page.getByRole("alert")).toHaveText("Simulated launch failure");
    expect(await page.evaluate(() => adb_test.calls.find(call => call.command === "run_scrcpy").args.automaticScreen)).toBe(false);
});

test("remembered configurations preserve the opt-in and legacy configurations default to off", async ({ page }) => {
    await mock(page, { devices: "synthetic-device\tdevice\n", remember_config: true, saved_config: { active_args: ["--turn-screen-off"] } });
    await page.goto("/");
    const automatic = page.getByRole("checkbox", { name: "Temporarily turn on phone screen for protected screens", exact: true });
    await expect(automatic).not.toBeChecked();
    await automatic.check();
    expect(await page.evaluate(() => JSON.parse(localStorage.getItem("scrcpyplus.scrcpy_config")).automatic_screen)).toBe(true);
    await page.getByRole("link", { name: "Settings", exact: true }).click();
    await page.locator('a[href="/"]').click();
    await expect(automatic).toBeChecked();
    await automatic.uncheck();
    await page.getByRole("link", { name: "Settings", exact: true }).click();
    await page.locator('a[href="/"]').click();
    await expect(automatic).not.toBeChecked();
});
