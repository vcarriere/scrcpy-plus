import { test, expect } from "@playwright/test";
import { build } from "esbuild";
import { resolve } from "node:path";
import { mkdir } from "node:fs/promises";
import { mock } from "./fixtures.mjs";

const old_adb = "C:\\AppData\\adb\\37.0.1\\platform-tools\\adb.exe";
const new_adb = "C:\\AppData\\adb\\37.0.3\\platform-tools\\adb.exe";
const custom_scrcpy = "C:\\Phone Tools\\scrcpy-password.cmd";
const release_url = "https://api.github.com/repos/Genymobile/scrcpy/releases";

async function manager(page, options = {}) {
    await mock(page, { advanced_adb: true, saved_paths: { scrcpy_path: custom_scrcpy, adb_path: old_adb }, ...options });
    await page.addInitScript(({ options, old_adb, new_adb }) => {
        const state = window.adb_version_test = { downloads: [], restores: [], activations: [], compactions: [], reads: { catalog: 0, installed: 0, versions: 0 }, current: "37.0.1", installed: [], ...options };
        if (!sessionStorage.getItem("adb.version.initialized")) {
            localStorage.setItem("setting.auto_update_adb", "true");
            localStorage.setItem("setting.auto_update_scrcpy", "true");
            localStorage.setItem("scrcpyplus.managed_adb", JSON.stringify({ version: "37.0.1", adb_path: old_adb }));
            sessionStorage.setItem("adb.version.initialized", "true");
        }
        const invoke = window.__TAURI_INTERNALS__.invoke;
        window.__TAURI_INTERNALS__.invoke = async (command, args = {}) => {
            if (command === "get_tool_releases") {
                state.reads.catalog++;
                if (state.offline) throw "Simulated offline catalog";
                return [{ version: "37.0.3", url: "https://dl.google.com/android/repository/platform-tools_r37.0.3-win.zip" }];
            }
            if (command === "get_installed_adb") { state.reads.installed++; return state.installed; }
            if (command === "get_installed_scrcpy") return [];
            if (command === "get_dependency_versions") { state.reads.versions++; return { adb: state.current, scrcpy: "5.0.1", scrcpy_plus: "3.1.0" }; }
            if (command === "restore_adb") {
                state.restores.push(args);
                if (state.restore_error) throw "Simulated ADB restore failure";
                const result = { version: args.version, adb_path: `C:\\AppData\\adb\\${args.version}\\platform-tools\\adb.exe`, compressed: false };
                if (state.deferred_restore) await new Promise(resolve => { state.finish_restore = resolve; });
                state.installed = [...state.installed.filter(entry => entry.version !== args.version), result];
                return result;
            }
            if (command === "compact_adb_cache") {
                state.compactions.push({ path: localStorage.getItem("scrcpyplus.adb_path"), receipt: localStorage.getItem("scrcpyplus.managed_adb") });
                if (state.deferred_compaction) await new Promise(resolve => { state.finish_compaction = resolve; });
                if (state.compaction_error) throw "Simulated cache compaction failure";
                if (state.installed_after_compaction) state.installed = state.installed_after_compaction;
                return state.compact_result || { results: [], remaining: 0 };
            }
            if (command === "download_adb") {
                state.downloads.push(args);
                if (state.download_error) throw "Simulated ADB download failure";
                const result = { version: args.version, adb_path: new_adb };
                if (state.deferred) return new Promise(resolve => { state.finish = () => { state.installed = [result]; resolve(result); }; });
                state.installed = [result];
                return result;
            }
            if (command === "activate_tool_paths") {
                state.activations.push(args);
                if (state.activation_error && args.adbPath === new_adb) throw "Simulated ADB activation failure";
                if (args.adbPath === new_adb) state.current = "37.0.3";
                else if (args.adbPath === old_adb) state.current = "37.0.1";
                else state.current = "37.0.2";
                return invoke("set_binary_paths", args);
            }
            return invoke(command, args);
        };
    }, { options, old_adb, new_adb });
    await page.route(release_url, route => route.fulfill({ json: [] }));
    await page.goto("/scrcpy_manager");
    await expect(page.getByRole("heading", { name: "Downgrade ADB?" })).toHaveCount(0);
    await expect(page.locator('[aria-label="ADB versions"]')).toBeVisible();
}

function row(page, version = "37.0.3") { return page.locator(`[data-adb-version="${version}"]`); }

test("manual ADB upgrade keeps scrcpy, disables only ADB installation, and records SDK terms", async ({ page }) => {
    await manager(page);
    await expect(page.getByRole("link", { name: "Android SDK License Agreement" })).toHaveAttribute("href", "https://developer.android.com/studio/terms");
    await row(page).getByRole("button", { name: "Download & use", exact: true }).click();
    await expect(row(page).getByText("In use", { exact: true })).toBeVisible();
    const settings = await page.evaluate(() => ({ scrcpy: localStorage.getItem("scrcpyplus.scrcpy_path"), adb: localStorage.getItem("scrcpyplus.adb_path"), adb_auto: localStorage.getItem("setting.auto_update_adb"), scrcpy_auto: localStorage.getItem("setting.auto_update_scrcpy"), checks: localStorage.getItem("setting.disable_update_check"), terms: localStorage.getItem("setting.adb_sdk_license") }));
    expect(settings).toEqual({ scrcpy: custom_scrcpy, adb: new_adb, adb_auto: "false", scrcpy_auto: "true", checks: "true", terms: "true" });
});

test("a downgrade prompts before any activation and cancel preserves settings", async ({ page }) => {
    await page.setViewportSize({ width: 380, height: 800 });
    await manager(page, { current: "37.0.4" });
    await row(page).getByRole("button", { name: "Download & use", exact: true }).click();
    const dialog = page.getByRole("dialog", { name: "Downgrade ADB?" });
    await expect(dialog).toBeVisible();
    await expect(dialog).toContainText("37.0.4 → 37.0.3");
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    await mkdir("../artifacts", { recursive: true });
    await page.screenshot({ path: "../artifacts/adb-version-downgrade-preview.png", animations: "disabled" });
    expect(await page.evaluate(() => adb_version_test.downloads)).toEqual([]);
    await dialog.getByRole("button", { name: "Cancel", exact: true }).click();
    expect(await page.evaluate(() => ({ path: localStorage.getItem("scrcpyplus.adb_path"), auto: localStorage.getItem("setting.auto_update_adb"), terms: localStorage.getItem("setting.adb_sdk_license"), activations: adb_version_test.activations }))).toEqual({ path: old_adb, auto: "true", terms: null, activations: [] });
});

test("confirming a downgrade installs once and disables automatic installation", async ({ page }) => {
    await manager(page, { current: "37.0.4" });
    await row(page).getByRole("button", { name: "Download & use", exact: true }).click();
    await page.getByRole("dialog", { name: "Downgrade ADB?" }).getByRole("button", { name: "Downgrade", exact: true }).click();
    await expect(row(page).getByText("In use", { exact: true })).toBeVisible();
    expect(await page.evaluate(() => adb_version_test.downloads.length)).toBe(1);
    expect(await page.evaluate(() => localStorage.getItem("setting.auto_update_adb"))).toBe("false");
});

test("unverified versions require a switch confirmation instead of claiming an upgrade", async ({ page }) => {
    await manager(page, { current: null });
    await row(page).getByRole("button", { name: "Download & use", exact: true }).click();
    const dialog = page.getByRole("dialog", { name: "Change ADB version?" });
    await expect(dialog).toContainText("could not be verified");
    expect(await page.evaluate(() => adb_version_test.downloads)).toEqual([]);
    await dialog.getByRole("button", { name: "Cancel", exact: true }).click();
});

for (const error of ["download_error", "activation_error"]) {
    test(`${error} preserves the current ADB and automatic policy`, async ({ page }) => {
        await manager(page, { [error]: true });
        await row(page).getByRole("button", { name: "Download & use", exact: true }).click();
        await expect(page.locator('[aria-label="ADB versions"]').getByRole("alert")).toContainText("Simulated ADB");
        expect(await page.evaluate(() => ({ path: localStorage.getItem("scrcpyplus.adb_path"), auto: localStorage.getItem("setting.auto_update_adb"), terms: localStorage.getItem("setting.adb_sdk_license") }))).toEqual({ path: old_adb, auto: "true", terms: null });
    });
}

test("kept local ADB versions remain selectable offline without downloading or new SDK acceptance", async ({ page }) => {
    const kept = "C:\\AppData\\adb\\37.0.2\\platform-tools\\adb.exe";
    await manager(page, { offline: true, installed: [{ version: "37.0.2", adb_path: kept }] });
    await expect(row(page, "37.0.2").getByText("Kept locally", { exact: true })).toBeVisible();
    await row(page, "37.0.2").getByRole("button", { name: "Use this version", exact: true }).click();
    await expect(row(page, "37.0.2").getByText("In use", { exact: true })).toBeVisible();
    expect(await page.evaluate(() => adb_version_test.downloads)).toEqual([]);
    expect(await page.evaluate(() => localStorage.getItem("setting.adb_sdk_license"))).toBeNull();
});

test("same-version selection preserves automatic installation and wraps in a narrow window", async ({ page }) => {
    await page.setViewportSize({ width: 380, height: 800 });
    await manager(page, { current: "37.0.3" });
    await row(page).getByRole("button", { name: "Download & use", exact: true }).click();
    await expect(row(page).getByText("In use", { exact: true })).toBeVisible();
    expect(await page.evaluate(() => localStorage.getItem("setting.auto_update_adb"))).toBe("true");
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
});

test("a compressed kept version restores offline without downloading or new SDK consent", async ({ page }) => {
    await manager(page, { offline: true, deferred_restore: true, installed: [{ version: "37.0.2", adb_path: "", compressed: true }] });
    await expect(row(page, "37.0.2").getByText("Kept locally (compressed)", { exact: true })).toBeVisible();
    await row(page, "37.0.2").getByRole("button", { name: "Use this version", exact: true }).click();
    await expect(page.locator('.modal-box').getByText("Restoring…", { exact: true })).toBeVisible();
    await page.evaluate(() => adb_version_test.finish_restore());
    await expect(row(page, "37.0.2").getByText("In use", { exact: true })).toBeVisible();
    const state = await page.evaluate(() => ({ restores: adb_version_test.restores, downloads: adb_version_test.downloads,
        path: localStorage.getItem("scrcpyplus.adb_path"), terms: localStorage.getItem("setting.adb_sdk_license"), compactions: adb_version_test.compactions }));
    expect(state.restores).toEqual([{ version: "37.0.2" }]);
    expect(state.downloads).toEqual([]);
    expect(state.terms).toBeNull();
    expect(state.compactions).toHaveLength(1);
    expect(state.compactions[0].path).toBe(state.path);
    expect(JSON.parse(state.compactions[0].receipt).version).toBe("37.0.2");
});

test("a compressed historical selection offers restore despite a matching stale saved path", async ({ page }) => {
    await manager(page, { offline: true, installed: [{ version: "37.0.1", adb_path: "", compressed: true }] });
    expect(await page.evaluate(() => localStorage.getItem("scrcpyplus.adb_path"))).toBe(old_adb);
    await expect(row(page, "37.0.1").getByText("Kept locally (compressed)", { exact: true })).toBeVisible();
    await expect(row(page, "37.0.1").getByText("In use", { exact: true })).toHaveCount(0);
    await expect(row(page, "37.0.1").getByRole("button", { name: "Use this version", exact: true })).toBeEnabled();
});

test("cancelling a compressed downgrade does not restore or compact anything", async ({ page }) => {
    await manager(page, { current: "37.0.3", offline: true, installed: [{ version: "37.0.2", adb_path: "", compressed: true }] });
    await row(page, "37.0.2").getByRole("button", { name: "Use this version", exact: true }).click();
    const dialog = page.getByRole("dialog", { name: "Downgrade ADB?" });
    await expect(dialog).toBeVisible();
    expect(await page.evaluate(() => adb_version_test.restores)).toEqual([]);
    await dialog.getByRole("button", { name: "Cancel", exact: true }).click();
    expect(await page.evaluate(() => ({ restores: adb_version_test.restores, compactions: adb_version_test.compactions,
        path: localStorage.getItem("scrcpyplus.adb_path"), auto: localStorage.getItem("setting.auto_update_adb") }))).toEqual({ restores: [], compactions: [], path: old_adb, auto: "true" });
});

test("compaction failure keeps the successful version selected without rolling it back", async ({ page }) => {
    await manager(page, { compaction_error: true });
    await row(page).getByRole("button", { name: "Download & use", exact: true }).click();
    await expect(row(page).getByText("In use", { exact: true })).toBeVisible();
    await expect(page.locator('[aria-label="ADB versions"]').getByRole("status")).toContainText("selected version is ready to use");
    expect(await page.evaluate(() => ({ path: localStorage.getItem("scrcpyplus.adb_path"), auto: localStorage.getItem("setting.auto_update_adb"), activations: adb_version_test.activations.length }))).toEqual({ path: new_adb, auto: "false", activations: 1 });
});

test("manual compression refreshes only local versions and reports the remaining batch", async ({ page }) => {
    await manager(page, { offline: true, installed: [{ version: "37.0.1", adb_path: old_adb }, { version: "37.0.0", adb_path: "C:\\AppData\\adb\\37.0.0\\platform-tools\\adb.exe" }],
        compact_result: { results: [{ version: "37.0.0", compressed: true }, { version: "36.0.2", compressed: true }, { version: "37.0.1", compressed: false, reason: "selected_or_running" }], remaining: 1 },
        installed_after_compaction: [{ version: "37.0.1", adb_path: old_adb }, { version: "37.0.0", adb_path: "", compressed: true }] });
    const button = page.getByRole("button", { name: "Compress inactive versions", exact: true });
    await expect(button).toBeEnabled();
    const before = await page.evaluate(() => ({ reads: { ...adb_version_test.reads }, path: localStorage.getItem("scrcpyplus.adb_path"), receipt: localStorage.getItem("scrcpyplus.managed_adb"), auto: localStorage.getItem("setting.auto_update_adb") }));
    await button.click();
    await expect(page.getByRole("status")).toHaveText("Compressed 2 inactive ADB versions. 1 more version remains; run again to continue.");
    await expect(row(page, "37.0.0").getByText("Kept locally (compressed)", { exact: true })).toBeVisible();
    const after = await page.evaluate(() => ({ reads: adb_version_test.reads, path: localStorage.getItem("scrcpyplus.adb_path"), receipt: localStorage.getItem("scrcpyplus.managed_adb"), auto: localStorage.getItem("setting.auto_update_adb"), compactions: adb_version_test.compactions.length, downloads: adb_version_test.downloads, restores: adb_version_test.restores, activations: adb_version_test.activations }));
    expect(after).toEqual({ ...before, reads: { ...before.reads, installed: before.reads.installed + 1 }, compactions: 1, downloads: [], restores: [], activations: [] });
});

test("manual compression shares the installation gate and shows compression progress", async ({ page }) => {
    await manager(page, { deferred_compaction: true });
    const button = page.getByRole("button", { name: "Compress inactive versions", exact: true });
    await expect(button).toBeEnabled();
    await button.click();
    await expect(page.locator('.modal-box').getByText("Compressing inactive versions…", { exact: true })).toBeVisible();
    await expect(button).toBeDisabled();
    await expect(row(page).getByRole("button", { name: "Download & use", exact: true })).toBeDisabled();
    expect(await page.evaluate(() => adb_version_test.compactions.length)).toBe(1);
    await page.evaluate(() => adb_version_test.finish_compaction());
    await expect(button).toBeEnabled();
    await expect(page.getByRole("status")).toHaveText("No inactive ADB versions need compression.");
});

test("manual compression explains a skipped unknown running-server path", async ({ page }) => {
    await manager(page, { compact_result: { compressed: 0, skipped: "running_server_path_unavailable" } });
    const button = page.getByRole("button", { name: "Compress inactive versions", exact: true });
    await expect(button).toBeEnabled();
    await button.click();
    await expect(page.getByRole("status")).toHaveText("Compression skipped because the running ADB server's executable could not be verified.");
    await expect(button).toBeEnabled();
    expect(await page.evaluate(() => adb_version_test.activations)).toEqual([]);
});

test("manual compression treats kept-copy failures as a nonfatal cache warning", async ({ page }) => {
    await manager(page, { compact_result: { results: [{ version: "37.0.0", compressed: false, reason: "kept", error: "Simulated locked file" }], remaining: 0 } });
    const button = page.getByRole("button", { name: "Compress inactive versions", exact: true });
    await expect(button).toBeEnabled();
    await button.click();
    await expect(page.getByRole("status").filter({ hasText: "No inactive ADB versions were compressed." })).toHaveText("No inactive ADB versions were compressed. 1 inactive version was kept unpacked.");
    await expect(page.getByRole("status").filter({ hasText: "Their full copies were kept." })).toBeVisible();
    await expect(page.locator('[aria-label="ADB versions"]').getByRole("alert")).toHaveCount(0);
    await expect(button).toBeEnabled();
    expect(await page.evaluate(() => ({ path: localStorage.getItem("scrcpyplus.adb_path"), auto: localStorage.getItem("setting.auto_update_adb"), activations: adb_version_test.activations }))).toEqual({ path: old_adb, auto: "true", activations: [] });
});

test("manual compression failure releases the gate and keeps the current selection", async ({ page }) => {
    await manager(page, { compaction_error: true });
    const button = page.getByRole("button", { name: "Compress inactive versions", exact: true });
    await expect(button).toBeEnabled();
    await button.click();
    await expect(page.getByRole("status")).toContainText("Your selected version is unchanged. Simulated cache compaction failure");
    await expect(page.locator('[aria-label="ADB versions"]').getByRole("alert")).toHaveCount(0);
    await expect(button).toBeEnabled();
    expect(await page.evaluate(() => ({ path: localStorage.getItem("scrcpyplus.adb_path"), auto: localStorage.getItem("setting.auto_update_adb"), activations: adb_version_test.activations }))).toEqual({ path: old_adb, auto: "true", activations: [] });
});

let module_id = 0;
async function service(options = {}) {
    const data = new Map(Object.entries({ "setting.auto_update_adb": "true", "setting.auto_update_scrcpy": "true", "setting.adb_sdk_license": "true", ...options.storage }));
    const state = { calls: [], ...options };
    const previous = { localStorage: Object.getOwnPropertyDescriptor(globalThis, "localStorage"), window: globalThis.window, state: globalThis.__tool_versions_test };
    Object.defineProperty(globalThis, "localStorage", { configurable: true, value: {
        getItem: key => data.has(key) ? data.get(key) : null,
        setItem: (key, value) => { if (state.storage_fail && key === "scrcpyplus.managed_adb" && value.includes('"version":"37.0.3"')) throw new Error("Synthetic storage failure"); data.set(key, String(value)); },
        removeItem: key => data.delete(key),
    } });
    globalThis.window = { dispatchEvent() {} };
    globalThis.__tool_versions_test = { invoke: async (command, args = {}) => {
        state.calls.push({ command, args });
        if (command === "get_dependency_versions") return { adb: state.current_adb === undefined ? "37.0.1" : state.current_adb, scrcpy: "5.0.0" };
        if (command === "get_adb_status") return { state: "running", active_sessions: state.sessions || 0 };
        if (command === "download_adb") return { adb_path: new_adb, version: args.version };
        if (command === "restore_adb") {
            if (state.restore_error) throw new Error("Synthetic restore failure");
            return { adb_path: new_adb, version: args.version };
        }
        if (command === "compact_adb_cache") {
            state.compact_snapshot = { path: globalThis.localStorage.getItem("scrcpyplus.adb_path"), receipt: globalThis.localStorage.getItem("scrcpyplus.managed_adb") };
            if (state.compaction_error) throw new Error("Synthetic compaction failure");
            return { compacted: 0 };
        }
        if (command === "download_scrcpy") return { scrcpy_path: "synthetic-scrcpy", adb_path: "synthetic-packaged-adb" };
        if (command === "activate_tool_paths") return;
        throw new Error("Unexpected synthetic command " + command);
    } };
    const bundle = await build({ stdin: { contents: 'export * from "./app/composables/scrcpy_release.ts"; export * from "./app/composables/tool_versions.ts";', resolveDir: resolve(".") }, bundle: true, write: false, format: "esm", platform: "browser", plugins: [{ name: "isolated-native-boundary", setup(builder) {
        builder.onResolve({ filter: /^@tauri-apps\/api\/core$|^vue$/ }, args => ({ path: args.path, namespace: "synthetic" }));
        builder.onLoad({ filter: /.*/, namespace: "synthetic" }, args => ({ contents: args.path === "vue" ? 'export const reactive = value => value;' : 'export const invoke = (...args) => globalThis.__tool_versions_test.invoke(...args);' }));
    } }] });
    const api = await import(`data:text/javascript;base64,${Buffer.from(bundle.outputFiles[0].text).toString("base64")}#${module_id++}`);
    return { api, state, data, restore() { if (previous.localStorage) Object.defineProperty(globalThis, "localStorage", previous.localStorage); else delete globalThis.localStorage; globalThis.window = previous.window; globalThis.__tool_versions_test = previous.state; } };
}

test("automatic installation skips old, unknown, busy, unlicensed, and custom selections without prompts", async () => {
    for (const [options, reason] of [[{ current_adb: "37.0.4" }, "not_newer"], [{ current_adb: null }, "version_unknown"], [{ sessions: 1 }, "busy"], [{ storage: { "setting.adb_sdk_license": "false" } }, "sdk_license"], [{ storage: { "scrcpyplus.adb_path": "synthetic-custom-adb" } }, "custom_executable"]]) {
        const local = await service(options);
        try {
            expect((await local.api.install_adb_release("37.0.3", true)).skipped).toBe(reason);
            expect(local.api.useVersionConfirmation().open).toBe(false);
            expect(local.state.calls.some(call => ["download_adb", "activate_tool_paths"].includes(call.command))).toBe(false);
        } finally { local.restore(); }
    }
});

test("automatic scrcpy installation respects the verified control version", async () => {
    const local = await service();
    try {
        expect((await local.api.install_scrcpy_release("synthetic", "5.0.2", true)).skipped).toBe("control_compatibility");
        expect(local.state.calls.some(call => call.command === "download_scrcpy")).toBe(false);
    } finally { local.restore(); }
});

test("storage failure rolls paths and update policy back through the native idle gate", async () => {
    const local = await service({ storage_fail: true, storage: { "scrcpyplus.adb_path": old_adb, "scrcpyplus.managed_adb": JSON.stringify({ version: "37.0.1", adb_path: old_adb }) } });
    try {
        await expect(local.api.install_adb_release("37.0.3")).rejects.toThrow("Synthetic storage failure");
        expect(local.data.get("scrcpyplus.adb_path")).toBe(old_adb);
        expect(local.data.get("setting.auto_update_adb")).toBe("true");
        expect(local.state.calls.filter(call => call.command === "activate_tool_paths").at(-1).args.adbPath).toBe(old_adb);
        expect(local.state.calls.some(call => call.command === "compact_adb_cache")).toBe(false);
    } finally { local.restore(); }
});

test("version comparison uses numeric releases/builds and fails closed for unknown versions", async () => {
    const local = await service();
    try {
        expect(local.api.compare_tool_versions("v5.10.0", "5.9.0")).toBe(1);
        expect(local.api.compare_tool_versions("37.0.3-16887417", "37.0.3-16887416")).toBe(1);
        expect(local.api.compare_tool_versions("37.0.3", "37.0.3-16887417")).toBe(0);
        expect(local.api.compare_tool_versions("5.0.1-rc1", "5.0.1")).toBeNull();
        expect(local.api.compare_tool_versions(null, "5.0.1")).toBeNull();
    } finally { local.restore(); }
});

test("compressed service restoration commits before nonfatal cache compaction", async () => {
    const local = await service({ compaction_error: true, storage: { "setting.adb_sdk_license": "false", "scrcpyplus.adb_path": old_adb } });
    try {
        const result = await local.api.install_adb_release("37.0.3", false, undefined, true);
        expect(result.adb_path).toBe(new_adb);
        expect(local.state.calls.some(call => call.command === "download_adb")).toBe(false);
        expect(local.state.calls.some(call => call.command === "restore_adb")).toBe(true);
        expect(local.data.get("scrcpyplus.adb_path")).toBe(new_adb);
        expect(local.data.get("setting.adb_sdk_license")).toBe("false");
        expect(local.data.get("setting.auto_update_adb")).toBe("false");
        expect(local.state.compact_snapshot.path).toBe(new_adb);
        expect(JSON.parse(local.state.compact_snapshot.receipt).version).toBe("37.0.3");
        expect(local.state.calls.filter(call => call.command === "activate_tool_paths")).toHaveLength(1);
        expect(local.api.useToolInstallation().error).toBe("");
        expect(local.api.useToolInstallation().cache_warning).toContain("selected version is ready");
    } finally { local.restore(); }
});

test("compressed service cancellation is decided before restoring the cached package", async () => {
    const local = await service({ current_adb: "37.0.4" });
    try {
        const operation = local.api.install_adb_release("37.0.3", false, undefined, true);
        await expect.poll(() => local.api.useVersionConfirmation().open).toBe(true);
        expect(local.state.calls.some(call => ["restore_adb", "download_adb", "compact_adb_cache"].includes(call.command))).toBe(false);
        local.api.answer_version_confirmation(false);
        expect((await operation).skipped).toBe("cancelled");
        expect(local.data.get("setting.auto_update_adb")).toBe("true");
    } finally { local.restore(); }
});

test("compressed service restore failure preserves settings and never starts compaction", async () => {
    const local = await service({ restore_error: true, storage: { "scrcpyplus.adb_path": old_adb } });
    try {
        await expect(local.api.install_adb_release("37.0.3", false, undefined, true)).rejects.toThrow("Synthetic restore failure");
        expect(local.data.get("scrcpyplus.adb_path")).toBe(old_adb);
        expect(local.data.get("setting.auto_update_adb")).toBe("true");
        expect(local.state.calls.some(call => ["download_adb", "activate_tool_paths", "compact_adb_cache"].includes(call.command))).toBe(false);
    } finally { local.restore(); }
});
