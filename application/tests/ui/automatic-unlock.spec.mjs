import { test, expect } from "@playwright/test";
import { mkdir } from "node:fs/promises";
import { mock } from "./fixtures.mjs";

function phone(serial, id = 1, verified = true) {
    return { id: "device:" + serial, name: serial, label: serial, verified, routes: [{ serial, state: "device", kind: "usb", transport_id: id,
        selection: { serial, transport_id: id, identity: verified ? "synthetic-" + serial : null, adb_path: "C:\\SCRCPY+\\adb.exe", server_socket: "tcp:localhost:5037" },
        info: { model: serial, model_id: "synthetic", version: "17", sdk: "37", build: "synthetic", security_patch: "2026-11-01" } }] };
}

async function setup(page, options = {}) {
    await mock(page, { inventory: options.inventory || [phone("first-phone")] });
    await page.addInitScript(options => {
        const previous = window.__TAURI_INTERNALS__.invoke;
        const state = window.adb_test;
        state.unlock_calls = [];
        state.unlock = { method: "password", user: 0, supported: true, enrolled: false, enabled: false, blocked: false, active: false, state: "disabled", reason: "", ...options.observation };
        state.unlock_deferred = options.deferred;
        window.__TAURI_INTERNALS__.invoke = async (command, args = {}) => {
            if (!["inspect_unlock", "enroll_unlock", "set_unlock_enabled", "remove_unlock", "stop_unlock_session"].includes(command)) return previous(command, args);
            // Test records deliberately exclude credentials and confirmation.
            state.unlock_calls.push({ command, selection: { ...args.selection }, enabled: args.enabled });
            if (command === "inspect_unlock") {
                if (state.unlock_deferred) return new Promise(resolve => { state.finish_unlock = observation => resolve(observation || { ...state.unlock }); });
                return { ...state.unlock };
            }
            if (command === "enroll_unlock") {
                state.enrollment_matched = args.password === "synthetic passphrase" && args.confirmation === args.password;
                const enroll = () => { Object.assign(state.unlock, { enrolled: true, enabled: false, blocked: false, reason: "" }); return { ...state.unlock }; };
                if (state.unlock_save_deferred) return new Promise(resolve => { state.finish_enrollment = () => resolve(enroll()); });
                return enroll();
            }
            if (command === "set_unlock_enabled") state.unlock.enabled = args.enabled;
            if (command === "remove_unlock") { state.unlock.enrolled = false; state.unlock.enabled = false; state.unlock.blocked = false; }
            if (command === "stop_unlock_session") { state.unlock.active = false; state.unlock.state = "disabled"; }
            return { ...state.unlock };
        };
    }, options);
    await page.goto("/");
    if (options.inventory?.length > 1) await page.getByRole("tab").first().click();
    await page.getByRole("checkbox", { name: "Show automatic unlock controls", exact: true }).check();
    return page.getByRole("region", { name: "Automatic unlock", exact: true });
}

test("password enrollment clears fields, uses explicit opt-in and keeps credentials out of settings", async ({ page }) => {
    const panel = await setup(page);
    await expect(panel.getByText("Detected unlock method: Password", { exact: true })).toBeVisible();
    await panel.getByLabel("Device password", { exact: true }).fill("synthetic passphrase");
    await panel.getByLabel("Confirm device password", { exact: true }).fill("different password");
    await panel.getByRole("button", { name: "Save password", exact: true }).click();
    await expect(panel.getByText("The passwords do not match.", { exact: true })).toBeVisible();
    expect(await page.evaluate(() => adb_test.unlock_calls.filter(call => call.command === "enroll_unlock").length)).toBe(0);
    await panel.getByLabel("Confirm device password", { exact: true }).fill("synthetic passphrase");
    await panel.getByRole("button", { name: "Save password", exact: true }).click();
    await expect(panel.getByText("Saved credential: Password", { exact: true })).toBeVisible();
    expect(await page.evaluate(() => adb_test.enrollment_matched)).toBe(true);
    const automatic = panel.getByRole("checkbox", { name: "Automatically unlock this device", exact: true });
    await expect(automatic).not.toBeChecked();
    await automatic.check();
    await expect(automatic).toBeChecked();
    await panel.getByRole("button", { name: "Replace saved password", exact: true }).click();
    await expect(panel.getByLabel("Device password", { exact: true })).toHaveValue("");
    await expect(panel.getByLabel("Confirm device password", { exact: true })).toHaveValue("");
    const persisted = await page.evaluate(() => JSON.stringify({ local: { ...localStorage }, session: { ...sessionStorage }, calls: adb_test.unlock_calls }));
    expect(persisted).not.toContain("synthetic passphrase");
    expect(persisted).not.toContain("different password");
    await panel.getByRole("button", { name: "Remove saved password", exact: true }).click();
    await expect(panel.getByText("Saved credential: Password", { exact: true })).toHaveCount(0);
    await expect(panel.getByRole("button", { name: "Save password", exact: true })).toBeVisible();
});

for (const method of ["pin", "pattern", "none", "unknown"]) {
    test(`detected ${method} has no credential entry or automatic fallback`, async ({ page }) => {
        const panel = await setup(page, { observation: { method, supported: false } });
        await expect(panel.getByText(/^Detected unlock method:/)).toBeVisible();
        await expect(panel.locator('input[type="password"]')).toHaveCount(0);
        await expect(panel.getByRole("button", { name: "Save password", exact: true })).toHaveCount(0);
        await expect(panel.getByRole("checkbox", { name: "Automatically unlock this device", exact: true })).toHaveCount(0);
        expect(await page.evaluate(() => adb_test.unlock_calls.every(call => call.command === "inspect_unlock"))).toBe(true);
    });
}

test("an unsupported password platform cannot enroll and a blocked saved password requires manual recovery", async ({ page }) => {
    const panel = await setup(page, { observation: { supported: false, enrolled: true, enabled: true, blocked: true } });
    await expect(panel.getByText("Automatic unlock is paused. Unlock this phone manually once to recover.", { exact: true })).toBeVisible();
    await expect(panel.locator('input[type="password"]')).toHaveCount(0);
    await expect(panel.getByRole("button", { name: "Replace saved password", exact: true })).toHaveCount(0);
    await panel.getByRole("checkbox", { name: "Automatically unlock this device", exact: true }).uncheck();
    await expect(panel.getByRole("checkbox", { name: "Automatically unlock this device", exact: true })).not.toBeChecked();
});

test("unverified devices cannot inspect or enroll credentials", async ({ page }) => {
    const panel = await setup(page, { inventory: [phone("unverified-phone", 1, false)] });
    await expect(panel.getByText("Select a verified, connected device to configure automatic unlock.", { exact: true })).toBeVisible();
    await expect(panel.locator('input[type="password"]')).toHaveCount(0);
    expect(await page.evaluate(() => adb_test.unlock_calls.length)).toBe(0);
});

test("a changed unlock method requires explicit password re-enrollment before enabling", async ({ page }) => {
    const panel = await setup(page, { observation: { enrolled: true, enabled: false, blocked: true, reason: "method_changed" } });
    const automatic = panel.getByRole("checkbox", { name: "Automatically unlock this device", exact: true });
    await expect(panel.getByText("The device unlock method changed. Replace the saved password before enabling automatic unlock again.", { exact: true })).toBeVisible();
    await expect(panel.getByText("Automatic unlock is paused. Unlock this phone manually once to recover.", { exact: true })).toHaveCount(0);
    await expect(automatic).not.toBeChecked();
    await expect(automatic).toBeDisabled();
    await expect(panel.getByRole("button", { name: "Remove saved password", exact: true })).toBeEnabled();
    await panel.getByRole("button", { name: "Check unlock method", exact: true }).click();
    await expect(automatic).toBeDisabled();
    expect(await page.evaluate(() => adb_test.unlock_calls.filter(call => call.command === "set_unlock_enabled").length)).toBe(0);
    await panel.getByRole("button", { name: "Replace saved password", exact: true }).click();
    await panel.getByLabel("Device password", { exact: true }).fill("synthetic passphrase");
    await panel.getByLabel("Confirm device password", { exact: true }).fill("synthetic passphrase");
    await panel.getByRole("button", { name: "Save password", exact: true }).click();
    await expect(automatic).toBeEnabled();
    await expect(automatic).not.toBeChecked();
    await automatic.check();
    expect(await page.evaluate(() => adb_test.unlock_calls.filter(call => call.command === "set_unlock_enabled").length)).toBe(1);
});

test("an active private-entry session can be stopped while enrollment changes are disabled", async ({ page }) => {
    const panel = await setup(page, { observation: { enrolled: true, enabled: true, active: true, state: "unlocking" } });
    await expect(panel.getByText("Entering the password privately…", { exact: true })).toBeVisible();
    await expect(panel.getByRole("checkbox", { name: "Automatically unlock this device", exact: true })).toBeDisabled();
    await expect(panel.getByRole("button", { name: "Replace saved password", exact: true })).toBeDisabled();
    await expect(panel.getByRole("button", { name: "Remove saved password", exact: true })).toBeDisabled();
    await panel.getByRole("button", { name: "Stop session", exact: true }).click();
    await expect(panel.getByRole("button", { name: "Stop session", exact: true })).toHaveCount(0);
    await expect(panel.getByRole("button", { name: "Replace saved password", exact: true })).toBeEnabled();
    expect(await page.evaluate(() => adb_test.unlock_calls.filter(call => call.command === "stop_unlock_session").length)).toBe(1);
});

test("Stop supersedes a pending inspection and a stale reply cannot restore the session", async ({ page }) => {
    const panel = await setup(page, { observation: { enrolled: true, enabled: true, active: true, state: "unlocking" } });
    await page.evaluate(() => { adb_test.unlock_deferred = true; });
    await panel.getByRole("button", { name: "Check unlock method", exact: true }).click();
    await expect.poll(() => page.evaluate(() => typeof adb_test.finish_unlock)).toBe("function");
    await expect(panel.getByRole("button", { name: "Stop session", exact: true })).toBeEnabled();
    await panel.getByRole("button", { name: "Stop session", exact: true }).click();
    await page.evaluate(() => adb_test.finish_unlock({ method: "password", supported: true, enrolled: true, enabled: true, active: true, state: "unlocking" }));
    await expect(panel.getByRole("button", { name: "Stop session", exact: true })).toHaveCount(0);
    await expect(panel.getByRole("button", { name: "Replace saved password", exact: true })).toBeEnabled();
});

test("a stopping session keeps configuration disabled and shows its current state", async ({ page }) => {
    const panel = await setup(page, { observation: { enrolled: true, enabled: true, active: true, state: "stopping" } });
    await expect(panel.getByText("Stopping this session…", { exact: true })).toBeVisible();
    await expect(panel.getByRole("button", { name: "Stopping…", exact: true })).toBeDisabled();
    await expect(panel.getByRole("button", { name: "Remove saved password", exact: true })).toBeDisabled();
    await expect(panel.getByRole("checkbox", { name: "Automatically unlock this device", exact: true })).toBeDisabled();
});

for (const { reason, message, blocked } of [
    { reason: "credential_record_unavailable", message: "The saved password record is unavailable. Remove it and enroll a new password.", blocked: true },
    { reason: "screen_off_unavailable", message: "Waiting for the phone screen to turn off. The password is not being retried." },
    { reason: "new_lock_episode", message: "The phone locked again. Preparing a guarded unlock." },
]) {
    test(`session reason ${reason} has actionable feedback without raw diagnostics`, async ({ page }) => {
        const panel = await setup(page, { observation: { enrolled: true, enabled: true, active: reason !== "credential_record_unavailable", state: "waiting", reason, blocked: !!blocked } });
        await expect(panel.getByText(message, { exact: true })).toBeVisible();
        if (reason === "credential_record_unavailable") {
            await expect(panel.getByText("Automatic unlock is paused. Unlock this phone manually once to recover.", { exact: true })).toHaveCount(0);
            await expect(panel.getByRole("button", { name: "Remove saved password", exact: true })).toBeEnabled();
        }
    });
}

test("switching devices clears fields and stale detection cannot overwrite the new selection", async ({ page }) => {
    const panel = await setup(page, { inventory: [phone("first-phone"), phone("second-phone", 2)], deferred: true });
    await expect.poll(() => page.evaluate(() => typeof adb_test.finish_unlock)).toBe("function");
    await page.evaluate(() => { adb_test.finish_first_unlock = adb_test.finish_unlock; });
    await page.getByRole("tab", { name: "second-phone", exact: true }).click();
    await expect.poll(() => page.evaluate(() => adb_test.unlock_calls.filter(call => call.command === "inspect_unlock").length)).toBe(2);
    await page.evaluate(() => adb_test.finish_first_unlock({ method: "password", supported: true, enrolled: true, enabled: true }));
    await expect(panel.getByRole("button", { name: "Checking…", exact: true })).toBeDisabled();
    await page.evaluate(() => { adb_test.unlock_deferred = false; adb_test.finish_unlock({ method: "pattern", supported: false }); });
    await expect(panel.getByText("Detected unlock method: Pattern", { exact: true })).toBeVisible();
    await expect(panel.getByText("Saved credential: Password", { exact: true })).toHaveCount(0);
    await expect(panel.locator('input[type="password"]')).toHaveCount(0);
});

test("a device change during enrollment keeps the new device free of stale credential state", async ({ page }) => {
    const panel = await setup(page, { inventory: [phone("first-phone"), phone("second-phone", 2)] });
    await panel.getByLabel("Device password", { exact: true }).fill("synthetic passphrase");
    await panel.getByLabel("Confirm device password", { exact: true }).fill("synthetic passphrase");
    await page.evaluate(() => { adb_test.unlock_save_deferred = true; });
    await panel.getByRole("button", { name: "Save password", exact: true }).click();
    await expect.poll(() => page.evaluate(() => typeof adb_test.finish_enrollment)).toBe("function");
    await expect(panel.getByLabel("Device password", { exact: true })).toHaveValue("");
    await page.getByRole("tab", { name: "second-phone", exact: true }).click();
    await page.evaluate(() => adb_test.finish_enrollment());
    await expect(panel.getByText("Saved credential: Password", { exact: true })).toHaveCount(0);
    await expect(panel.getByLabel("Device password", { exact: true })).toHaveValue("");
    await expect(panel.getByLabel("Confirm device password", { exact: true })).toHaveValue("");
});

test("cancel clears both password fields and the shared styling fits a narrow viewport", async ({ page }) => {
    await page.setViewportSize({ width: 380, height: 800 });
    const panel = await setup(page);
    await panel.getByLabel("Device password", { exact: true }).fill("synthetic passphrase");
    await panel.getByLabel("Confirm device password", { exact: true }).fill("synthetic passphrase");
    await panel.getByRole("button", { name: "Cancel", exact: true }).click();
    await expect(panel.getByLabel("Device password", { exact: true })).toHaveValue("");
    await expect(panel.getByLabel("Confirm device password", { exact: true })).toHaveValue("");
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    await mkdir("../artifacts", { recursive: true });
    await panel.screenshot({ path: "../artifacts/automatic-unlock-narrow.png" });
});
