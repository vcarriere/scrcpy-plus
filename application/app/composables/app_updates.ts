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
  const updates = useState<any>("app_updates", () => ({ state: "idle", app_state: "idle", currentVersion: "", latestVersion: "", release_url: "", tools: [], checked_at: 0 }));

  async function check_updates() {
    if (updates.value.state === "checking") return;
    updates.value.state = "checking";
    const results = await Promise.allSettled([(async () => {
      const environment: any = await invoke("environment");
      const repo: any[] = await $fetch(environment.releases_url, { timeout: 10000, retry: 0 });
      const release = repo.find(release => release.prerelease === false && !release.draft);
      if (!release || typeof release.tag_name !== "string" || !release.tag_name) throw new Error("No stable release available");
      return {
        app_state: is_newer(release.tag_name, environment.app_version) ? "available" : "current",
        currentVersion: environment.app_version,
        latestVersion: release.tag_name,
        release_url: release.html_url
      };
    })(), invoke("get_tool_updates")]);
    const app = results[0].status === "fulfilled" ? results[0].value : { app_state: "error", currentVersion: "", latestVersion: "", release_url: "" };
    const tools = results[1].status === "fulfilled" && Array.isArray(results[1].value) ? results[1].value
      : ["adb", "scrcpy"].map(tool => ({ tool, state: "error", error: "Could not check this component. Try again." }));
    const states = [app.app_state, ...tools.map((tool: any) => tool.state)];
    updates.value = { ...app, tools, checked_at: Date.now(), state: states.includes("available") ? "available" : states.some(state => ["error", "unknown", "unavailable"].includes(state)) ? "error" : "current" };
  }

  return { updates, check_updates };
}

export function automatic_checks_enabled() { return localStorage.getItem("setting.disable_update_check") !== "true"; }
