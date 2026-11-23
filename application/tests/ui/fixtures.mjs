export async function mock(page, options = {}) {
    await page.addInitScript(options => {
        if (!sessionStorage.getItem("scrcpyplus.test_initialized")) {
            if (options.update_preference !== null) localStorage.setItem("setting.disable_update_check", String(options.update_preference ?? true));
            if (options.advanced_adb !== undefined) localStorage.setItem("setting.advanced_adb", String(options.advanced_adb));
            for (const [key, value] of Object.entries(options.saved_paths || {})) localStorage.setItem(`scrcpyplus.${key}`, value);
            if (options.remember_config) {
                localStorage.setItem("setting.save_scrcpy_config", "true");
                localStorage.setItem("scrcpyplus.scrcpy_config", JSON.stringify(options.saved_config || {}));
            }
            sessionStorage.setItem("scrcpyplus.test_initialized", "true");
        }
        const state = window.adb_test = {
            server: "running", paused: false, sessions: 0, calls: [], devices: "", fail: false,
            details: { version: "37.0.3", build: "1234", path: "C:\\SCRCPY+\\adb.exe" },
            client: { path: "C:\\SCRCPY+\\adb.exe", version: "37.0.3-1234", is_windows: true }, ...options,
        };
        const snapshot = () => ({
            state: state.server, paused: state.paused, active_sessions: state.sessions,
            endpoint: "tcp:localhost:5037", path: "C:\\SCRCPY+\\adb.exe", protocol_version: state.server === "running" ? 41 : null,
            server_details: state.server === "running" ? state.details : null,
            screen_sessions: (state.screen_sessions || []).map(session => ({ ...session })),
        });
        Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText: async text => { state.clipboard = text; } } });
        window.__TAURI_INTERNALS__ = { invoke: async (command, args = {}) => {
            state.calls.push({ command, args });
            if (command === "set_binary_paths") { state.binary_requests = { scrcpy_path: args.scrcpyPath, adb_path: args.adbPath }; return; }
            if (command === "get_binary_paths") {
                const result = {};
                for (const key of ["scrcpy_path", "adb_path"]) {
                    const requested = state.binary_requests?.[key] || null;
                    const invalid = !!requested && (state.invalid_paths || []).includes(requested);
                    const custom = !!requested && !invalid;
                    const name = key === "scrcpy_path" ? "scrcpy.exe" : "adb.exe";
                    result[key] = { requested, invalid, source: custom ? "custom" : "bundled", command: custom ? requested : `scrcpy/${name}`,
                        path: custom ? requested : `C:\\SCRCPY+\\scrcpy\\${name}`, available: custom ? !(state.missing_custom || []).includes(requested) : !(state.missing_defaults || []).includes(key) };
                }
                return result;
            }
            if (command === "get_adb_client") return state.client;
            if (command === "control_screen_power") {
                const session = state.screen_sessions.find(session => session.id === args.id);
                session.state = args.paused ? "paused" : "monitoring";
                session.reason = args.paused ? "requested" : "watching";
                return;
            }
            if (command === "inspect_protected_screen") {
                if (state.screen_fail) throw "Simulated protected-screen query failure";
                if (state.screen_deferred) return new Promise(resolve => { state.finish_screen = () => resolve(state.screen || { state: "secure" }); });
                return state.screen || { state: "secure", reason: "focused_window_flag", android_sdk: 37, power_commands: "reset_off" };
            }
            if (command === "get_adb_status") {
                if (state.server === "running") state.paused = false;
                return snapshot();
            }
            if (command === "control_adb") {
                if (state.fail) throw "Simulated ADB operation failure";
                state.paused = !args.start;
                state.server = args.start ? "running" : "stopped";
                return snapshot();
            }
            if (command === "get_dependency_versions") return { scrcpy_plus: "3.1.0", scrcpy: "5.0.1", adb: "37.0.1" };
            if (command === "environment") return { is_windows: true, os: "windows", arch: "x86_64", app_version: "3.1.0", releases_url: "https://api.github.com/repos/Frontesque/scrcpy-plus/releases" };
            if (command === "get_device_inventory") {
                if (state.paused) throw "ADB is paused";
                if (state.inventory) return structuredClone(state.inventory);
                return state.devices.trim().split("\n").filter(Boolean).map((line, index) => {
                    const [serial, status] = line.trim().split(/\s+/);
                    return {
                        id: "route:" + serial, name: serial, label: serial, verified: false,
                        routes: [{ serial, state: status, kind: "usb", transport_id: index + 1,
                            selection: { serial, transport_id: index + 1, identity: null, adb_path: "C:\\SCRCPY+\\adb.exe", server_socket: "tcp:localhost:5037" },
                            info: status === "device" ? { model: serial, model_id: "synthetic", version: "17", sdk: "37", build: "synthetic", security_patch: "2026-11-01" } : null }],
                    };
                });
            }
            if (command === "disconnect_device_route") {
                if (state.disconnect_fail) throw "Simulated disconnect failure";
                state.inventory = state.inventory.map(device => ({ ...device, routes: device.routes.filter(route => route.serial !== args.selection.serial) })).filter(device => device.routes.length);
                return;
            }
            if (command === "exec_adb") {
                if (args.args[0] === "--version") return "Android Debug Bridge version 1.0.41";
                if (state.paused) throw "ADB is paused";
                if (args.args[0] === "devices") return "List of devices attached\n" + state.devices;
                return "Synthetic device property";
            }
            if (command === "run_scrcpy") {
                if (!state.mirror_pending) throw "Simulated launch failure";
                if (args.automaticScreen) state.screen_sessions = [{ id: 7, device: args.args[1], state: "screen_on", reason: "protected" }];
                return new Promise(resolve => { state.finish_mirror = () => { state.screen_sessions = []; resolve(); }; });
            }
            throw "Unexpected command: " + command;
        } };
    }, options);
}
