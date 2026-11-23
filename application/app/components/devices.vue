<template>
    <div class="card bg-base-200 w-full mb-8">
        <div class="card-body">
            <!--   Version Information   -->
            <div class="flex">
                <p class="card-title text-white">{{ $t('devices') }}</p>
                <NuxtLink to="/wireless_setup">
                    <button class="btn btn-primary text-white max-h-8 min-h-8 h-8">
                        <icon size="18x18" name="wifi" />
                        <span>{{ $t('connect_wirelessly') }}</span>
                    </button>
                </NuxtLink>
            </div>

            <p v-if="error" class="text-error break-words" role="alert">{{ error }}</p>
            <p v-if="!adb_available">{{ $t('adb_paused') }}</p>
            <div v-else-if="devices.length > 0">
                <div role="tablist" class="tabs tabs-boxed">
                    <a role="tab" class="tab force-text-white" :class="active_device == device ? 'tab-active' : ''" v-for="device in devices" @click="set_active(device)">{{ device }}</a>
                </div>

                <button v-if="active_device && is_wireless(active_device)" class="btn btn-sm mt-4" @click="disconnect(active_device)">
                    {{ $t('disconnect') }}
                </button>

                <div v-if="active_device && unauthorized">
                    <p class="mt-4 text-white">{{ $t('unauthorized') }}</p>
                </div>
                <div v-else-if="active_device && device_states[active_device] !== 'device'">
                    <p class="mt-4 text-white">{{ device_states[active_device] === 'offline' ? $t('offline') : device_states[active_device] }}</p>
                </div>
                <div v-else-if="active_device">
                    <p class="mt-4 text-white flex"><icon class="mt-1 mr-2" size="18x18" name="mobile" /> {{ $t('model') }}</p>
                    <p>{{ active_device_information.model }} ({{ active_device_information.model_id }})</p>
                    <p class="mt-4 text-white flex"><icon class="mt-1 mr-2" size="18x18" name="android" /> {{ $t('android_version') }}</p>
                    <p>{{ active_device_information.version }} (API {{ active_device_information.sdk }})</p>
                    <p class="mt-4 text-white flex"><icon class="mt-1 mr-2" size="18x18" name="wrench" /> {{ $t('build_number') }}</p>
                    <p>{{ active_device_information.build }}</p>
                    <p class="mt-4 text-white flex"><icon class="mt-1 mr-2" size="18x18" name="shield" /> {{ $t('security_patch') }}</p>
                    <p>{{ active_device_information.security_patch }}</p>
                </div>
                <p v-else>{{ $t('no_device') }}</p>
            </div>
            <div v-else>
                <p>{{ $t('no_device2') }}</p>
            </div>

        </div>
    </div>
</template>

<style scoped>
.force-text-white {
    color: white !important;
}
</style>

<script>
import { invoke } from "@tauri-apps/api/core";
export default {
    emits: ['device'],
    setup() {
        const { available: adb_available } = useAdbStatus();
        return { adb_available };
    },
    data() {
        return {
            devices: new Array(),
            device_states: {},
            error: "",
            active_device: undefined,
            unauthorized: false,
            selecting: undefined,
            active_device_information: {
                model: "",
                model_id: "",
                version: "",
                sdk: "",
                build: "",
                security_patch: ""
            }
        }
    },
    watch: {
        adb_available(available) {
            if (available) this.refresh_devices();
            else this.clear_selection();
        }
    },
    methods: {

        clear_selection() {
            this.selecting = undefined;
            this.active_device = undefined;
            this.devices = [];
            this.device_states = {};
            this.$emit('device', false);
        },

        is_wireless(serial) {
            // Wireless serials look like "192.168.1.5:5555" or "adb-XXXX._adb-tls-connect._tcp"
            return serial.includes(":") || serial.includes("._adb-tls-connect._tcp");
        },

        async disconnect(serial) {
            try {
                await invoke("exec_adb", { args: ["disconnect", serial] });
                if (this.active_device === serial) this.clear_selection();
                await this.refresh_devices();
            } catch (e) {
                this.error = String(e);
            }
        },

        async set_active(device) {
            if (!this.adb_available) return;
            this.selecting = device;
            this.error = "";
            try {
                const state = this.device_states[device];
                const info = state === 'device' ? await this.get_device_info(device) : {};
                // A newer selection or Stop happened while adb was responding
                if (this.selecting !== device || !this.adb_available || this.disposed) return;
                this.active_device_information = info;
                this.unauthorized = state === 'unauthorized';
                this.active_device = device;
                this.$emit('device', state === 'device' ? device : false);
            } catch (e) {
                if (this.selecting !== device || this.disposed) return;
                this.error = String(e);
                this.$emit('device', false);
            }
        },

        // The majority of this function should be converted to rust
        // It is currently a javascript function to allow this program to be written faster
        async get_device_info(serial) {
            const info = {};
            info.model            = (await invoke("exec_adb", { args: ["-s", serial, "shell", "getprop", "ro.product.model"] })).trim();
            info.model_id         = (await invoke("exec_adb", { args: ["-s", serial, "shell", "getprop", "ro.product.device"] })).trim();
            info.version          = (await invoke("exec_adb", { args: ["-s", serial, "shell", "getprop", "ro.build.version.release"] })).trim();
            info.sdk              = (await invoke("exec_adb", { args: ["-s", serial, "shell", "getprop", "ro.build.version.sdk"] })).trim();
            info.build            = (await invoke("exec_adb", { args: ["-s", serial, "shell", "getprop", "ro.build.id"] })).trim();
            info.security_patch   = (await invoke("exec_adb", { args: ["-s", serial, "shell", "getprop", "ro.build.version.security_patch"] })).trim();
            return info;
        },

        // The majority of this function should be converted to rust
        // It is currently a javascript function to allow this program to be written faster
        async refresh_devices() {
            if (!this.adb_available || this.refreshing || this.disposed) return;
            this.refreshing = true;
            try {
                const devices = await invoke("exec_adb", { args: ["devices"] });
                if (!this.adb_available || this.disposed) return;
                this.error = "";
                const raw_devices_array = devices.split("\n");
                let devices_output = new Array();
                const states = {};
                for (const i in raw_devices_array) {
                    const entry = raw_devices_array[i];
                    if (i == 0) continue;
                    if (entry == "") continue;
                    if (entry == "\r") continue;
                    if (entry.startsWith("*")) continue; // adb daemon status lines
                    if (entry.includes("\t")) {
                        const [serial, state] = entry.trim().split(/\s+/);
                        devices_output.push(serial);
                        states[serial] = state;
                    }
                }
                const previous_state = this.device_states[this.active_device];
                this.device_states = states;
                this.devices = devices_output;
                if (this.active_device && !devices_output.includes(this.active_device)) {
                    this.active_device = undefined;
                    this.$emit('device', false);
                }
                if (devices_output.length === 1 && this.active_device !== devices_output[0]) {
                    await this.set_active(devices_output[0]);
                } else if (this.active_device && previous_state !== states[this.active_device]) {
                    await this.set_active(this.active_device);
                }
            } catch (e) {
                if (!this.disposed) this.error = String(e);
            } finally {
                this.refreshing = false;
            }
        }
    },
    mounted() {
        this.disposed = false;
        this.refresh_devices();
        this.interval = setInterval(this.refresh_devices, 5000);
    },
    beforeUnmount() {
      this.disposed = true;
      this.selecting = undefined;
      clearInterval(this.interval); // Prevent Memory Leak
    }
}
</script>
