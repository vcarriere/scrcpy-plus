import { test, expect } from "@playwright/test";
import { mkdir } from "node:fs/promises";
import { mock } from "./fixtures.mjs";

const releases_url = "https://api.github.com/repos/Genymobile/scrcpy/releases";
const old_paths = { scrcpy_path: "C:\\AppData\\scrcpy\\v5.0.0\\scrcpy.exe", adb_path: "C:\\AppData\\scrcpy\\v5.0.0\\adb.exe" };
const new_paths = { scrcpy_path: "C:\\AppData\\scrcpy\\v5.0.1\\scrcpy.exe", adb_path: "C:\\AppData\\scrcpy\\v5.0.1\\adb.exe" };

function release(version) {
    return { tag_name: version, prerelease: false, draft: false, assets: ["win32", "win64", "winarm64"].map(platform => ({
        name: `scrcpy-${platform}-${version}.zip`,
        browser_download_url: `https://github.com/Genymobile/scrcpy/releases/download/${version}/scrcpy-${platform}-${version}.zip`,
    })) };
}

async function manager(page, options = {}) {
    await mock(page, { advanced_adb: true, ...options });
    await page.addInitScript(({ options, old_paths, new_paths }) => {
        const state = window.version_test = { downloads: [], ...options };
        sessionStorage.setItem("scrcpyplus.version_loads", String(Number(sessionStorage.getItem("scrcpyplus.version_loads") || 0) + 1));
        if (options.managed && !sessionStorage.getItem("scrcpyplus.version_initialized")) {
            localStorage.setItem("scrcpyplus.managed_release", JSON.stringify({ ...old_paths, version: "v5.0.0" }));
            sessionStorage.setItem("scrcpyplus.version_initialized", "true");
        }
        if (options.storage_fail) {
            const set = Storage.prototype.setItem;
            Storage.prototype.setItem = function(key, value) {
                if (key === "scrcpyplus.managed_release" && value.includes('"version":"v5.0.1"')) throw new DOMException("Simulated storage failure", "QuotaExceededError");
                return set.call(this, key, value);
            };
        }
        let internal;
        const wrap = value => {
            const invoke = value.invoke;
            value.invoke = async (command, args = {}) => {
                if (command === "environment") return { ...await invoke(command, args), ...options.environment };
                if (command === "download_scrcpy") {
                    state.downloads.push(args);
                    if (state.download_error) throw "Simulated download failure";
                    if (state.deferred) return new Promise(resolve => { state.finish_download = () => { state.missing_adb = false; resolve(new_paths); }; });
                    return new_paths;
                }
                if (command === "exec_adb" && args.args[0] === "--version" && state.missing_adb) throw "Simulated missing ADB";
                if (command === "set_binary_paths" && state.activation_fail && args.scrcpyPath === new_paths.scrcpy_path) throw "Simulated activation failure";
                return invoke(command, args);
            };
            return value;
        };
        const existing = window.__TAURI_INTERNALS__;
        Object.defineProperty(window, "__TAURI_INTERNALS__", {
            configurable: true,
            get: () => internal,
            set: value => { internal = wrap(value); },
        });
        if (existing) internal = wrap(existing);
    }, { options, old_paths, new_paths });
    await page.route(releases_url, route => route.fulfill({ json: [release("v5.0.1"), release("v5.0.0")] }));
    await page.goto(options.start_path || "/scrcpy_manager");
    if (!options.start_path) await expect(page.getByText("v5.0.1", { exact: true })).toBeVisible();
}

function row(page, version = "v5.0.1") {
    return page.getByText(version, { exact: true }).locator("..").locator("..");
}

for (const [arch, platform] of [["x86", "win32"], ["x86_64", "win64"], ["aarch64", "winarm64"]]) {
    test(`Windows ${arch} selects the matching ${platform} archive`, async ({ page }) => {
        await manager(page, { environment: { arch } });
        await row(page).getByRole("button", { name: "Download & use", exact: true }).click();
        await expect(row(page).getByText("In use", { exact: true })).toBeVisible();
        expect(await page.evaluate(() => version_test.downloads[0].url)).toContain(`scrcpy-${platform}-v5.0.1.zip`);
    });
}

test("an unsupported Windows architecture does not fall back to an x64 build", async ({ page }) => {
    await manager(page, { environment: { arch: "unsupported" } });
    await expect(page.getByText("No build available for this platform", { exact: true })).toHaveCount(2);
    await expect(row(page).getByRole("button", { name: "Download & use", exact: true })).toBeDisabled();
    expect(await page.evaluate(() => version_test.downloads)).toEqual([]);
});

test("custom executables remain selected and a matching directory name is not proof of an active release", async ({ page }) => {
    const custom = { scrcpy_path: "C:\\Phone Tools\\v5.0.1\\scrcpy-password.cmd", adb_path: "C:\\Android Tools\\adb.exe" };
    await manager(page, { saved_paths: custom });
    await expect(row(page).getByText("In use", { exact: true })).toHaveCount(0);
    await row(page).getByRole("button", { name: "Download & use", exact: true }).click();
    await expect(page.getByRole("status")).toContainText("executable selections were kept.");
    await expect(row(page).getByText("In use", { exact: true })).toHaveCount(0);
    expect(await page.evaluate(() => ({ scrcpy_path: localStorage.getItem("scrcpyplus.scrcpy_path"), adb_path: localStorage.getItem("scrcpyplus.adb_path") }))).toEqual(custom);
    expect(await page.evaluate(() => adb_test.binary_requests)).toEqual(custom);
    await page.setViewportSize({ width: 380, height: 800 });
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    await mkdir("../artifacts", { recursive: true });
    await page.screenshot({ path: "../artifacts/original-review-version-manager.png", fullPage: true, animations: "disabled" });
});

test("a recorded managed SCRCPY selection can change without replacing custom ADB", async ({ page }) => {
    const custom_adb = "C:\\Android Tools\\adb.exe";
    await manager(page, { managed: true, saved_paths: { ...old_paths, adb_path: custom_adb } });
    await expect(row(page, "v5.0.0").getByText("In use", { exact: true })).toBeVisible();
    await row(page).getByRole("button", { name: "Download & use", exact: true }).click();
    await expect(row(page).getByText("In use", { exact: true })).toBeVisible();
    expect(await page.evaluate(() => localStorage.getItem("scrcpyplus.scrcpy_path"))).toBe(new_paths.scrcpy_path);
    expect(await page.evaluate(() => localStorage.getItem("scrcpyplus.adb_path"))).toBe(custom_adb);
    await page.reload();
    await expect(row(page).getByText("In use", { exact: true })).toBeVisible();
});

test("changing SCRCPY also preserves an earlier managed ADB selection", async ({ page }) => {
    await manager(page, { managed: true, saved_paths: old_paths });
    await row(page).getByRole("button", { name: "Download & use", exact: true }).click();
    await expect(row(page).getByText("In use", { exact: true })).toBeVisible();
    expect(await page.evaluate(() => adb_test.binary_requests)).toEqual({ scrcpy_path: new_paths.scrcpy_path, adb_path: old_paths.adb_path });
});

for (const failure of ["download_error", "activation_fail", "storage_fail"]) {
    test(`${failure} leaves the previous managed paths active and persisted`, async ({ page }) => {
        await manager(page, { managed: true, saved_paths: old_paths, [failure]: true });
        await row(page).getByRole("button", { name: "Download & use", exact: true }).click();
        await expect(page.getByRole("alert")).toContainText({ download_error: "Simulated download failure", activation_fail: "Simulated activation failure", storage_fail: "Simulated storage failure" }[failure]);
        expect(await page.evaluate(() => ({ scrcpy_path: localStorage.getItem("scrcpyplus.scrcpy_path"), adb_path: localStorage.getItem("scrcpyplus.adb_path") }))).toEqual(old_paths);
        expect(await page.evaluate(() => adb_test.binary_requests)).toEqual(old_paths);
        await expect(row(page, "v5.0.0").getByText("In use", { exact: true })).toBeVisible();
        await expect(row(page).getByRole("button", { name: "Download & use", exact: true })).toBeEnabled();
    });
}

test("pending installs survive navigation, block duplicates, and preserve choices changed during the download", async ({ page }) => {
    await manager(page, { deferred: true });
    const button = row(page).getByRole("button", { name: "Download & use", exact: true });
    await button.evaluate(button => { button.click(); button.click(); });
    await expect.poll(() => page.evaluate(() => version_test.downloads.length)).toBe(1);
    await page.locator('a[href="/settings"]').evaluate(link => link.click());
    await expect(page.getByRole("checkbox", { name: "Show advanced controls", exact: true })).toBeVisible();
    const wrapper = "C:\\Phone Tools\\new-wrapper.cmd";
    await page.evaluate(path => localStorage.setItem("scrcpyplus.scrcpy_path", path), wrapper);
    await page.getByRole("button", { name: "Manage versions", exact: true }).click();
    await expect(row(page).getByRole("button", { name: "Downloading…", exact: true })).toBeDisabled();
    await page.evaluate(() => version_test.finish_download());
    await expect(page.getByRole("status")).toContainText("executable selections were kept.");
    expect(await page.evaluate(() => version_test.downloads.length)).toBe(1);
    expect(await page.evaluate(() => localStorage.getItem("scrcpyplus.scrcpy_path"))).toBe(wrapper);
});

test("a missing-ADB download finishing after navigation does not reload the new page", async ({ page }) => {
    await manager(page, { start_path: "/", missing_adb: true, deferred: true });
    await expect(page.getByText("ADB not found", { exact: true })).toBeVisible();
    await page.getByRole("button", { name: "Download", exact: true }).click();
    await expect.poll(() => page.evaluate(() => version_test.downloads.length)).toBe(1);
    await page.getByRole("link", { name: "Settings", exact: true }).evaluate(link => link.click());
    await expect(page.getByRole("checkbox", { name: "Show advanced controls", exact: true })).toBeVisible();
    await page.evaluate(() => version_test.finish_download());
    await expect.poll(() => page.evaluate(() => localStorage.getItem("scrcpyplus.scrcpy_path"))).toBe(new_paths.scrcpy_path);
    await page.waitForTimeout(250);
    expect(await page.evaluate(() => sessionStorage.getItem("scrcpyplus.version_loads"))).toBe("1");
    await expect(page).toHaveURL(/\/settings$/);
});
