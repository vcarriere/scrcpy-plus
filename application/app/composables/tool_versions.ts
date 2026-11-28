import { reactive } from "vue";

export type ManagedTool = "adb" | "scrcpy";
const confirmation = reactive({ open: false, tool: "scrcpy" as ManagedTool, current: "", target: "", downgrade: false });
let finish_confirmation: ((confirmed: boolean) => void) | null = null;

function parsed_version(version: string | null | undefined) {
  const match = String(version || "").trim().match(/^v?(\d+(?:\.\d+){0,3})(?:-(\d+))?$/i);
  if (!match) return null;
  const parts = match[1].split(".").map(Number);
  if (parts.some(part => !Number.isSafeInteger(part))) return null;
  const build = match[2] === undefined ? null : Number(match[2]);
  if (build !== null && !Number.isSafeInteger(build)) return null;
  return { parts, build };
}

export function compare_tool_versions(first: string | null | undefined, second: string | null | undefined) {
  const a = parsed_version(first), b = parsed_version(second);
  if (!a || !b) return null;
  for (let i = 0; i < Math.max(a.parts.length, b.parts.length); i++) {
    const difference = (a.parts[i] || 0) - (b.parts[i] || 0);
    if (difference) return Math.sign(difference);
  }
  return a.build !== null && b.build !== null ? Math.sign(a.build - b.build) : 0;
}

export function tool_auto_enabled(tool: ManagedTool) {
  return localStorage.getItem(`setting.auto_update_${tool}`) === "true";
}

export function set_tool_auto(tool: ManagedTool, enabled: boolean) {
  localStorage.setItem(`setting.auto_update_${tool}`, String(enabled));
  window.dispatchEvent(new Event("scrcpyplus:update-policy"));
}

export function useVersionConfirmation() { return confirmation; }

export function answer_version_confirmation(confirmed: boolean) {
  const finish = finish_confirmation;
  finish_confirmation = null;
  confirmation.open = false;
  finish?.(confirmed);
}

export async function confirm_tool_version(tool: ManagedTool, current: string | null, target: string | null) {
  const comparison = compare_tool_versions(target, current);
  if (comparison !== null && comparison >= 0) return true;
  if (confirmation.open) return false;
  confirmation.tool = tool;
  confirmation.current = current || "Unknown";
  confirmation.target = target || "Unknown";
  confirmation.downgrade = comparison === -1;
  confirmation.open = true;
  return new Promise<boolean>(resolve => { finish_confirmation = resolve; });
}

export function same_tool_path(first: string, second: string) {
  const normalize = (path: string) => /^[A-Za-z]:[\\/]|^\\\\/.test(path) ? path.replaceAll("\\", "/").toLowerCase() : path;
  return !!first && !!second && normalize(first) === normalize(second);
}
