import { invoke } from "@tauri-apps/api/core";
import { reactive } from "vue";

export const SCRCPY_RELEASES_URL = "https://api.github.com/repos/Genymobile/scrcpy/releases";
const installation = reactive({ busy: "", error: "", preserved: false });

export function useScrcpyInstallation() {
  return installation;
}

// Release assets look like scrcpy-win64-vX.zip or scrcpy-linux-x86_64-vX.tar.gz
export function find_scrcpy_asset(assets: any[], os: string, arch: string) {
  const platforms: Record<string, string> = { x86: "win32", x86_64: "win64", aarch64: "winarm64" };
  const windows = platforms[arch];
  if (os === "windows" && !windows) return;
  const prefix = os === "windows"
    ? `scrcpy-${windows}-`
    : `scrcpy-${os}-${arch}-`;
  return assets.find(a => typeof a.name === "string" && a.name.startsWith(prefix) && /\.(zip|tar\.gz)$/.test(a.name));
}

function managed_release() {
  try {
    return JSON.parse(localStorage.getItem("scrcpyplus.managed_release") || "null");
  } catch { return null; }
}

function same_path(first: string, second: string) {
  const normalize = (path: string) => /^[A-Za-z]:[\\/]|^\\\\/.test(path) ? path.replaceAll("\\", "/").toLowerCase() : path;
  return !!first && !!second && normalize(first) === normalize(second);
}

export function is_managed_scrcpy(version: string) {
  const managed = managed_release();
  return managed?.version === version && same_path(localStorage.getItem("scrcpyplus.scrcpy_path") || "", managed.scrcpy_path);
}

async function activate_release(url: string, version: string) {
  const paths: any = await invoke("download_scrcpy", { url, version });
  const keys = ["scrcpyplus.scrcpy_path", "scrcpyplus.adb_path", "scrcpyplus.managed_release"];
  const previous = keys.map(key => localStorage.getItem(key));
  const managed = managed_release();
  // Choosing scrcpy must not also replace a previously selected ADB version.
  const selected = ["scrcpy_path", "adb_path"].map((key, index) => previous[index] && (index === 1 || !same_path(previous[index]!, managed?.[key])) ? previous[index]! : paths[key]);
  const preserved = selected.some((path, index) => !same_path(path, paths[index === 0 ? "scrcpy_path" : "adb_path"]));
  await invoke("set_binary_paths", { scrcpyPath: selected[0], adbPath: selected[1] });
  try {
    localStorage.setItem(keys[0], selected[0]);
    localStorage.setItem(keys[1], selected[1]);
    localStorage.setItem(keys[2], JSON.stringify({ ...paths, version }));
  } catch (e) {
    let restored = true;
    for (let i = 0; i < keys.length; i++) {
      try {
        if (previous[i] === null) localStorage.removeItem(keys[i]);
        else localStorage.setItem(keys[i], previous[i]!);
      } catch { restored = false; }
    }
    try {
      await invoke("set_binary_paths", { scrcpyPath: previous[0], adbPath: previous[1] });
    } catch { restored = false; }
    if (!restored) throw new Error("Could not restore the previous executable settings. Check Advanced settings before starting SCRCPY.");
    throw e;
  }
  return { ...paths, preserved };
}

async function install(version: string, operation: () => Promise<any>) {
  if (installation.busy) throw new Error("A SCRCPY installation is already running.");
  installation.busy = version;
  installation.error = "";
  installation.preserved = false;
  try {
    const result = await operation();
    installation.preserved = result.preserved;
    return result;
  } catch (e) {
    installation.error = String(e);
    throw e;
  } finally {
    installation.busy = "";
  }
}

// Custom executable selections remain active until changed explicitly in Settings.
export async function install_scrcpy_release(url: string, version: string) {
  return install(version, () => activate_release(url, version));
}

export async function install_latest_scrcpy() {
  return install("latest", async () => {
    const env: any = await invoke("environment");
    const releases: any[] = await $fetch(SCRCPY_RELEASES_URL, { timeout: 10000, retry: 0 });
    for (const rel of releases) {
      if (rel.draft || rel.prerelease) continue;
      const asset = find_scrcpy_asset(rel.assets, env.os, env.arch);
      if (asset) {
        installation.busy = rel.tag_name;
        return activate_release(asset.browser_download_url, rel.tag_name);
      }
    }
    throw new Error("No build available for this platform");
  });
}
