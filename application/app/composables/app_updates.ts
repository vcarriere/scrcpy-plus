import { invoke } from "@tauri-apps/api/core";

// Compares dotted versions numerically, ignoring a leading "v" and any suffix
function is_newer(latest: string, current: string) {
  const parse = (v: string) => String(v).replace(/^v/i, "").split(/[.-]/).map(n => parseInt(n, 10) || 0);
  const a = parse(latest), b = parse(current);
  for (let i = 0; i < Math.max(a.length, b.length); i++) {
    const x = a[i] ?? 0, y = b[i] ?? 0;
    if (x !== y) return x > y;
  }
  return false;
}

export function useAppUpdates() {
  const updates = useState("app_updates", () => ({ state: "idle", currentVersion: "", latestVersion: "", release_url: "" }));

  async function check_updates() {
    if (updates.value.state === "checking") return;
    updates.value.state = "checking";
    try {
      const environment: any = await invoke("environment");
      const repo: any[] = await $fetch(environment.releases_url, { timeout: 10000, retry: 0 });
      const release = repo.find(release => release.prerelease === false && !release.draft);
      if (!release || typeof release.tag_name !== "string" || !release.tag_name) throw new Error("No stable release available");
      updates.value = {
        state: is_newer(release.tag_name, environment.app_version) ? "available" : "current",
        currentVersion: environment.app_version,
        latestVersion: release.tag_name,
        release_url: release.html_url
      };
    } catch (e) {
      updates.value.state = "error";
      console.warn("Update check failed", e);
    }
  }

  return { updates, check_updates };
}
