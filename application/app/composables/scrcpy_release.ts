import { invoke } from "@tauri-apps/api/core";
import { reactive } from "vue";
import { compare_tool_versions, confirm_tool_version, same_tool_path, tool_auto_enabled, type ManagedTool } from "./tool_versions";

export const SCRCPY_RELEASES_URL = "https://api.github.com/repos/Genymobile/scrcpy/releases";
const installation = reactive({ busy: "", error: "", preserved: false, tool: "scrcpy" as ManagedTool, stage: "", skipped: "", cache_warning: "" });

export function useScrcpyInstallation() { return installation; }
export function useToolInstallation() { return installation; }

// Release assets look like scrcpy-win64-vX.zip or scrcpy-linux-x86_64-vX.tar.gz
export function find_scrcpy_asset(assets: any[], os: string, arch: string) {
  const platforms: Record<string, string> = { x86: "win32", x86_64: "win64", aarch64: "winarm64" };
  const windows = platforms[arch];
  if (os === "windows" && !windows) return;
  const prefix = os === "windows" ? `scrcpy-${windows}-` : `scrcpy-${os}-${arch}-`;
  return assets.find(a => typeof a.name === "string" && a.name.startsWith(prefix) && /\.(zip|tar\.gz)$/.test(a.name));
}

function receipt(tool: ManagedTool) {
  try { return JSON.parse(localStorage.getItem(tool === "adb" ? "scrcpyplus.managed_adb" : "scrcpyplus.managed_release") || "null"); }
  catch { return null; }
}

export function is_managed_scrcpy(version: string, path?: string) {
  const managed = receipt("scrcpy");
  return compare_tool_versions(managed?.version, version) === 0 && same_tool_path(localStorage.getItem("scrcpyplus.scrcpy_path") || "", path || managed.scrcpy_path);
}

export function is_managed_adb(version: string, path?: string) {
  const managed = receipt("adb");
  return compare_tool_versions(managed?.version, version) === 0 && same_tool_path(localStorage.getItem("scrcpyplus.adb_path") || "", path || managed.adb_path);
}

function custom_selection(tool: ManagedTool) {
  const selected = localStorage.getItem(`scrcpyplus.${tool}_path`);
  return !!selected && !same_tool_path(selected, receipt(tool)?.[`${tool}_path`]);
}

function skip(reason: string) { installation.skipped = reason; return { skipped: reason, preserved: false }; }

async function prepare(tool: ManagedTool, version: string, automatic: boolean, preserving = false) {
  if (automatic && !tool_auto_enabled(tool)) return skip("disabled");
  if (automatic && custom_selection(tool)) return skip("custom_executable");
  if (automatic && tool === "adb" && localStorage.getItem("setting.adb_sdk_license") !== "true") return skip("sdk_license");
  const versions: any = await invoke("get_dependency_versions");
  const current = typeof versions?.[tool] === "string" ? versions[tool] : null;
  const comparison = compare_tool_versions(version, current);
  if (automatic) {
    if (comparison === null) return skip("version_unknown");
    if (comparison <= 0) return skip("not_newer");
    if (tool === "scrcpy" && compare_tool_versions(version, "5.0.1") !== 0) return skip("control_compatibility");
    const status: any = await invoke("get_adb_status");
    if (status.active_sessions !== 0 || !["running", "stopped"].includes(status.state)) return skip("busy");
  } else if (!preserving) {
    installation.stage = "confirming";
    if (!await confirm_tool_version(tool, current, version)) return skip("cancelled");
  }
  installation.stage = "downloading";
  return { current };
}

async function persist_selection(tool: ManagedTool, selected: [string | null, string | null], paths: any, version: string, current: string | null, automatic: boolean, license = false) {
  const receipt_key = tool === "adb" ? "scrcpyplus.managed_adb" : "scrcpyplus.managed_release";
  const auto_key = `setting.auto_update_${tool}`;
  const keys = ["scrcpyplus.scrcpy_path", "scrcpyplus.adb_path", receipt_key, auto_key, ...(license ? ["setting.adb_sdk_license"] : [])];
  const previous = keys.map(key => localStorage.getItem(key));
  const tool_index = tool === "adb" ? 1 : 0;
  const path_changed = selected[tool_index] !== previous[tool_index];
  const version_changed = compare_tool_versions(version, current) !== 0;
  installation.stage = "activating";
  await invoke("activate_tool_paths", { scrcpyPath: selected[0], adbPath: selected[1] });
  try {
    for (let i = 0; i < 2; i++) {
      if (selected[i]) localStorage.setItem(keys[i], selected[i]!);
      else localStorage.removeItem(keys[i]);
    }
    localStorage.setItem(receipt_key, JSON.stringify({ ...paths, version }));
    if (!automatic && path_changed && version_changed) localStorage.setItem(auto_key, "false");
    if (license) localStorage.setItem("setting.adb_sdk_license", "true");
  } catch (e) {
    let restored = true;
    for (let i = 0; i < keys.length; i++) {
      try {
        if (previous[i] === null) localStorage.removeItem(keys[i]);
        else localStorage.setItem(keys[i], previous[i]!);
      } catch { restored = false; }
    }
    try { await invoke("activate_tool_paths", { scrcpyPath: previous[0], adbPath: previous[1] }); }
    catch { restored = false; }
    if (!restored) throw new Error("Could not restore the previous executable settings. Check Advanced settings before starting SCRCPY.");
    throw e;
  }
  window.dispatchEvent(new Event("scrcpyplus:update-policy"));
}

async function activate_release(url: string, version: string, automatic: boolean, bootstrap = false, installedPaths?: any) {
  const initial = localStorage.getItem("scrcpyplus.scrcpy_path");
  const ready = await prepare("scrcpy", version, automatic, custom_selection("scrcpy"));
  if ("skipped" in ready) return ready;
  const paths: any = installedPaths || await invoke("download_scrcpy", { url, version });
  if (automatic && (!tool_auto_enabled("scrcpy") || localStorage.getItem("scrcpyplus.scrcpy_path") !== initial)) return skip("selection_changed");
  const previous = [localStorage.getItem("scrcpyplus.scrcpy_path"), localStorage.getItem("scrcpyplus.adb_path")];
  const managed = receipt("scrcpy");
  const selected: [string | null, string | null] = [previous[0] && !same_tool_path(previous[0], managed?.scrcpy_path) ? previous[0] : paths.scrcpy_path,
    previous[1] || (bootstrap ? paths.adb_path : null)];
  const preserved = !!previous[0] && selected[0] !== paths.scrcpy_path || !!previous[1] && selected[1] !== paths.adb_path;
  await persist_selection("scrcpy", selected, paths, version, ready.current, automatic);
  return { ...paths, preserved };
}

async function install(tool: ManagedTool, version: string, operation: () => Promise<any>) {
  if (installation.busy) throw new Error("A tool installation is already running.");
  installation.busy = version;
  installation.tool = tool;
  installation.stage = "preparing";
  installation.error = "";
  installation.preserved = false;
  installation.skipped = "";
  installation.cache_warning = "";
  try {
    const result = await operation();
    installation.preserved = result.preserved === true;
    return result;
  } catch (e) {
    installation.error = String(e);
    throw e;
  } finally { installation.busy = ""; installation.stage = ""; }
}

// Custom executable selections remain active until changed explicitly in Settings.
export async function install_scrcpy_release(url: string, version: string, automatic = false, installedPaths?: any) {
  return install("scrcpy", version, () => activate_release(url, version, automatic, false, installedPaths));
}

export async function install_adb_release(version: string, automatic = false, installedPath?: string, compressed = false) {
  return install("adb", version, async () => {
    const initial = localStorage.getItem("scrcpyplus.adb_path");
    const ready = await prepare("adb", version, automatic);
    if ("skipped" in ready) return ready;
    if (compressed) installation.stage = "restoring";
    const paths: any = compressed ? await invoke("restore_adb", { version })
      : installedPath ? { adb_path: installedPath, version } : await invoke("download_adb", { version });
    const latest = localStorage.getItem("scrcpyplus.adb_path");
    if (automatic && (!tool_auto_enabled("adb") || latest !== initial)) return skip("selection_changed");
    if (!automatic && latest !== initial) return { ...paths, preserved: true };
    const selected: [string | null, string | null] = [localStorage.getItem("scrcpyplus.scrcpy_path"), paths.adb_path];
    await persist_selection("adb", selected, paths, version, ready.current, automatic, !automatic && !installedPath && !compressed);
    // Keep the previous unpacked version available until activation and every
    // preference write have committed. Cache maintenance cannot undo success.
    installation.stage = "compacting";
    try { await invoke("compact_adb_cache"); }
    catch { installation.cache_warning = "Older ADB versions could not be compressed. The selected version is ready to use."; }
    return { ...paths, preserved: false };
  });
}

export async function install_latest_scrcpy() {
  return install("scrcpy", "latest", async () => {
    const env: any = await invoke("environment");
    const releases: any[] = await $fetch(SCRCPY_RELEASES_URL, { timeout: 10000, retry: 0 });
    for (const rel of releases) {
      if (rel.draft || rel.prerelease) continue;
      const asset = find_scrcpy_asset(rel.assets, env.os, env.arch);
      if (asset) {
        installation.busy = rel.tag_name;
        return activate_release(asset.browser_download_url, rel.tag_name, false, true);
      }
    }
    throw new Error("No build available for this platform");
  });
}
