import { invoke } from "@tauri-apps/api/core";

// Awaited by Nuxt before the app mounts, so version checks use the saved binaries
export default defineNuxtPlugin({ name: "binary-paths", async setup() {
  await invoke("set_binary_paths", {
    scrcpyPath: localStorage.getItem("scrcpyplus.scrcpy_path") || null,
    adbPath: localStorage.getItem("scrcpyplus.adb_path") || null,
  });
} });
