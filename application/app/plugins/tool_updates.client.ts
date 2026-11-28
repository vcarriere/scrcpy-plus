import { install_adb_release, install_scrcpy_release, useToolInstallation } from "../composables/scrcpy_release";
import { automatic_checks_enabled, useAppUpdates } from "../composables/app_updates";
import { tool_auto_enabled } from "../composables/tool_versions";

export default defineNuxtPlugin({ name: "tool-updates", dependsOn: ["binary-paths"], setup(nuxtApp) {
  const { updates, check_updates } = useAppUpdates();
  let disposed = false;
  let running = false;
  let previous_check = Date.now();

  async function install_pending() {
    if (disposed || running || !automatic_checks_enabled() || useToolInstallation().busy) return;
    running = true;
    try {
      for (const update of updates.value.tools || []) {
        if (disposed || !automatic_checks_enabled()) break;
        if (update.state !== "available" || !tool_auto_enabled(update.tool) || update.retry_at > Date.now()) continue;
        try {
          const result = update.tool === "adb" ? await install_adb_release(update.latest_version, true)
            : await install_scrcpy_release(update.url, update.latest_version, true);
          if (result.skipped) update.install_state = result.skipped;
          else { update.current_version = update.latest_version; update.state = "current"; update.install_state = "installed"; }
        } catch { update.install_state = "error"; update.retry_at = Date.now() + 30 * 60 * 1000; }
      }
      const states = [updates.value.app_state, ...updates.value.tools.map((tool: any) => tool.state)];
      updates.value.state = states.includes('available') ? 'available' : states.some(state => ['error', 'unknown', 'unavailable'].includes(state)) ? 'error' : 'current';
    } finally { running = false; }
  }

  async function refresh() {
    if (!automatic_checks_enabled() || disposed) return;
    previous_check = Date.now();
    await check_updates();
    await install_pending();
  }

  function policy_changed() {
    if (!automatic_checks_enabled()) return;
    if (updates.value.state === 'idle') void refresh();
    else void install_pending();
  }
  nuxtApp.hook("app:mounted", () => {
    // The home banner performs the first check. Background installation only
    // follows the separate per-tool opt-in, never a manual Check action.
    const timer = setInterval(() => {
      if (Date.now() - previous_check >= 6 * 60 * 60 * 1000) void refresh();
      else void install_pending();
    }, 60000);
    window.addEventListener("scrcpyplus:update-policy", policy_changed);
    window.addEventListener("pagehide", () => { disposed = true; clearInterval(timer); window.removeEventListener("scrcpyplus:update-policy", policy_changed); }, { once: true });
  });
} });
