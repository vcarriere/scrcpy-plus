import { invoke } from "@tauri-apps/api/core";

let refreshing = false;
let generation = 0;

export function useAdbStatus() {
  const status = useState<any>("adb_status", () => ({ state: "checking", paused: false, active_sessions: 0 }));
  const busy = useState("adb_busy", () => false);
  const error = useState("adb_error", () => "");
  const paused_running = computed(() => status.value.paused && status.value.state === "running");
  const pause_message = computed(() => paused_running.value ? "adb_running_paused" : "adb_paused");
  const available = computed(() => !busy.value && !status.value.paused && !["checking", "error", "starting", "stopping"].includes(status.value.state));

  async function refresh() {
    if (refreshing || busy.value) return;
    refreshing = true;
    const request = generation;
    try {
      const observed = await invoke("get_adb_status");
      if (request === generation) status.value = observed;
    } catch (e) {
      if (request === generation) status.value = { ...status.value, state: "error", error: String(e) };
    } finally {
      refreshing = false;
    }
  }

  async function control(start: boolean) {
    if (busy.value) return;
    generation++;
    busy.value = true;
    error.value = "";
    status.value = { ...status.value, state: start ? "starting" : "stopping" };
    try {
      status.value = await invoke("control_adb", { start });
    } catch (e) {
      error.value = String(e);
    } finally {
      busy.value = false;
      await refresh();
    }
  }

  return { status, busy, error, paused_running, pause_message, available, refresh, control };
}
