import { invoke } from "@tauri-apps/api/core";

const phases = ["checking", "preparing", "unlocking", "manual_unlock", "starting_mirror", "ready", "launched", "closed", "error"];
const pending = ["checking", "preparing", "unlocking", "starting_mirror"];

export function useScrcpyLaunch() {
  const launches = useState<Record<string, any>>("scrcpy_launches", () => ({}));
  const dialog_id = useState("scrcpy_launch_dialog", () => "");
  const current = computed(() => launches.value[dialog_id.value] || null);
  const active = computed(() => Object.values(launches.value).filter(launch => !launch.finished && !["ready", "closed", "error"].includes(launch.phase)));

  function hide_progress() { dialog_id.value = ""; }
  function show_progress(id: string) { if (launches.value[id]) dialog_id.value = id; }
  function is_starting(device: string) { return Object.values(launches.value).some(launch => launch.device === device && !launch.finished && launch.reason !== "progress_unavailable" && pending.includes(launch.phase)); }

  async function start_launch(device: string, args: any) {
    const id = crypto.randomUUID();
    for (const previous of Object.keys(launches.value)) if (launches.value[previous].finished) delete launches.value[previous];
    launches.value[id] = { id, device, phase: "checking", reason: "", elapsed_ms: 0, finished: false, error: "" };
    const launch = launches.value[id];
    dialog_id.value = id;
    let completed = false;
    let failures = 0;
    let timer: ReturnType<typeof setTimeout>;
    async function refresh() {
      try {
        const observed: any = await invoke("get_scrcpy_progress", { launchId: id });
        if (completed) return;
        failures = 0;
        if (observed && phases.includes(observed.phase)) {
          const previous = launch.phase;
          Object.assign(launch, { phase: observed.phase, reason: observed.reason || "", elapsed_ms: observed.elapsed_ms || 0, finished: observed.finished === true });
          if (launch.phase === "ready" || launch.phase === "closed") { if (dialog_id.value === id) hide_progress(); }
          else if (launch.phase === "manual_unlock" && previous !== "manual_unlock" && !dialog_id.value) show_progress(id);
          else if (pending.includes(launch.phase) && ["ready", "launched"].includes(previous) && !dialog_id.value) show_progress(id);
        }
      } catch {
        if (!completed && ++failures >= 3) launch.reason = "progress_unavailable";
      } finally {
        if (!completed && !launch.finished) timer = setTimeout(refresh, 350);
      }
    }
    timer = setTimeout(refresh, 100);
    try {
      await invoke("run_scrcpy", { ...args, launchId: id });
      launch.phase = "closed";
      if (dialog_id.value === id) hide_progress();
    } catch (e) {
      launch.phase = "error";
      launch.error = String(e);
      show_progress(id);
      throw e;
    } finally { completed = true; launch.finished = true; clearTimeout(timer); }
  }

  return { launches, dialog_id, current, active, hide_progress, show_progress, is_starting, start_launch };
}
