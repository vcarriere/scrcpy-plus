<template>
    <div class="card bg-base-200 w-full mb-8">
        <div class="card-body min-w-0">
            <!--   Version Information   -->
            <div class="flex flex-wrap items-center gap-2">
                <p class="card-title text-white">{{ $t('devices') }}</p>
                <NuxtLink to="/wireless_setup" class="max-w-full">
                    <button class="btn btn-primary text-white max-w-full min-h-8 h-auto py-1">
                        <icon class="shrink-0" size="18x18" name="wifi" />
                        <span class="min-w-0 whitespace-normal">{{ $t('connect_wirelessly') }}</span>
                    </button>
                </NuxtLink>
            </div>

            <p v-if="error" class="text-error break-words" role="alert">{{ error }}</p>
            <p v-if="!adb_available">{{ $t('adb_refresh_paused') }}</p>
            <div v-else-if="devices.length > 0">
                <div role="tablist" :aria-label="$t('devices')" class="tabs tabs-boxed device-tabs">
                    <button type="button" role="tab" class="tab text-base font-normal force-text-white" :class="active_device_id == device.id ? 'tab-active' : ''"
                        v-for="(device, index) in devices" :key="device.id" :aria-selected="active_device_id == device.id" :aria-label="device.label"
                        :tabindex="active_device_id == device.id || (!active_device_id && index === 0) ? 0 : -1" @click="set_active(device)" @keydown="tab_key($event, index)">
                        <span class="truncate min-w-0">{{ device.name }}</span>
                        <span v-if="device.label !== device.name" class="shrink-0 whitespace-nowrap">{{ device.label.slice(device.name.length) }}</span>
                    </button>
                </div>

                <fieldset v-if="active_device" class="mt-4 space-y-3 min-w-0">
                    <legend class="text-white mb-2">{{ $t('device_connections') }}</legend>
                    <p v-if="!active_device.verified">{{ $t('device_identity_unavailable') }}</p>
                    <div v-for="route in active_device.routes" :key="`${route.serial}:${route.transport_id}`" class="rounded-lg bg-base-300 p-3 min-w-0">
                        <label class="flex items-start gap-3 cursor-pointer min-w-0">
                            <input type="radio" name="device_connection" class="radio radio-primary shrink-0 mt-1" :checked="is_active(route)" @change="set_active(active_device, route)" />
                            <span class="min-w-0">
                                <span class="block text-white">{{ $t(`device_connection_${route.kind}`) }} · {{ state_label(route.state) }}</span>
                                <span class="block connection-name select-text">{{ route.serial }}</span>
                            </span>
                        </label>
                        <div class="flex flex-wrap gap-2 mt-3">
                            <button class="btn btn-sm btn-primary text-white" @click="copy(route)">{{ $t(copied_route === route.serial ? 'device_connection_copied' : 'device_connection_copy') }}</button>
                            <button v-if="is_wireless(route)" class="btn btn-sm btn-primary text-white" :disabled="!!disconnecting" @click="disconnect(route)">
                                {{ $t('disconnect') }}
                            </button>
                        </div>
                    </div>
                </fieldset>

                <div v-if="active_route && active_route.state === 'unauthorized'">
                    <p class="mt-4 text-white">{{ $t('unauthorized') }}</p>
                </div>
                <div v-else-if="active_route && active_route.state !== 'device'">
                    <p class="mt-4 text-white">{{ state_label(active_route.state) }}</p>
                </div>
                <div v-else-if="active_route && active_route.info" class="device-information">
                    <p class="mt-4 text-white flex"><icon class="mt-1 mr-2" size="18x18" name="mobile" /> {{ $t('model') }}</p>
                    <p>{{ active_device_information.model }} ({{ active_device_information.model_id }})</p>
                    <p class="mt-4 text-white flex"><icon class="mt-1 mr-2" size="18x18" name="android" /> {{ $t('android_version') }}</p>
                    <p>{{ active_device_information.version }} (API {{ active_device_information.sdk }})</p>
                    <p class="mt-4 text-white flex"><icon class="mt-1 mr-2" size="18x18" name="wrench" /> {{ $t('build_number') }}</p>
                    <p>{{ active_device_information.build }}</p>
                    <p class="mt-4 text-white flex"><icon class="mt-1 mr-2" size="18x18" name="shield" /> {{ $t('security_patch') }}</p>
                    <p>{{ active_device_information.security_patch }}</p>
                </div>
                <p v-else-if="!active_device">{{ $t('no_device') }}</p>
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
.device-tabs {
    display: flex;
    flex-wrap: wrap;
    gap: 0.25rem;
}
.device-tabs .tab {
    display: flex;
    gap: 0.25rem;
    min-width: 0;
    max-width: 100%;
}
.connection-name, .device-information {
    overflow-wrap: anywhere;
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
            error: "",
            active_device_id: undefined,
            active_serial: undefined,
            active_transport_id: undefined,
            selected_routes: {},
            selection_lost: false,
            copied_route: "",
            disconnecting: undefined,
            refreshing: false,
            request: 0
        }
    },
    computed: {
        active_device() { return this.devices.find(device => device.id === this.active_device_id); },
        active_route() { return this.active_device?.routes.find(route => this.is_active(route)); },
        active_device_information() { return this.active_route?.info || {}; }
    },
    watch: {
        adb_available(available) {
            if (available) this.refresh_devices();
            else this.clear_selection();
        }
    },
    methods: {

        clear_selection() {
            this.request++;
            this.active_device_id = undefined;
            this.active_serial = undefined;
            this.active_transport_id = undefined;
            this.devices = [];
            this.selected_routes = {};
            this.selection_lost = false;
            this.copied_route = "";
            this.$emit('device', false);
        },

        is_active(route) { return route.serial === this.active_serial && route.transport_id === this.active_transport_id; },

        tab_key(event, index) {
            if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
            event.preventDefault();
            const next = event.key === 'Home' ? 0 : event.key === 'End' ? this.devices.length - 1
                : (index + (event.key === 'ArrowRight' ? 1 : -1) + this.devices.length) % this.devices.length;
            this.set_active(this.devices[next]);
            event.currentTarget.parentElement.querySelectorAll('[role="tab"]')[next].focus();
        },

        state_label(state) {
            return ['device', 'offline', 'unauthorized'].includes(state) ? this.$t(state === 'device' ? 'device_online' : state) : state;
        },

        is_wireless(route) { return ['tcp', 'mdns'].includes(route.kind); },

        async copy(route) {
            this.error = "";
            try {
                await navigator.clipboard.writeText(route.serial);
                this.copied_route = route.serial;
            } catch (e) {
                this.error = this.$t('device_connection_copy_failed');
            }
        },

        async disconnect(route) {
            if (this.disconnecting || !this.adb_available) return;
            this.disconnecting = route.serial;
            this.error = "";
            try {
                await invoke("disconnect_device_route", { selection: route.selection });
                this.request++;
                await this.refresh_devices();
            } catch (e) {
                this.error = String(e);
            } finally {
                this.disconnecting = undefined;
            }
        },

        set_active(device, route) {
            if (!this.adb_available) return;
            const previous = this.selected_routes[device.id];
            route ||= device.routes.find(route => route.serial === previous?.serial && route.transport_id === previous?.transport_id)
                || device.routes.find(route => route.state === 'device') || device.routes[0];
            if (!route) return;
            this.active_device_id = device.id;
            this.active_serial = route.serial;
            this.active_transport_id = route.transport_id;
            this.selected_routes[device.id] = { serial: route.serial, transport_id: route.transport_id };
            this.selection_lost = false;
            this.error = "";
            this.copied_route = "";
            this.$emit('device', route.state === 'device' ? route.serial : false, route.state === 'device' ? route.selection : null);
        },

        // The majority of this function should be converted to rust
        // It is currently a javascript function to allow this program to be written faster
        async refresh_devices() {
            if (!this.adb_available || this.refreshing || this.disposed) return;
            this.refreshing = true;
            const request = this.request;
            try {
                const devices = await invoke("get_device_inventory");
                if (!this.adb_available || this.disposed || request !== this.request) return;
                this.error = "";
                const selected = this.active_device_id;
                this.devices = devices;
                const device = devices.find(device => device.id === selected);
                if (device) {
                    this.set_active(device);
                } else if (selected) {
                    this.active_device_id = undefined;
                    this.active_serial = undefined;
                    this.active_transport_id = undefined;
                    this.selection_lost = true;
                    this.$emit('device', false);
                } else if (devices.length === 1 && !this.selection_lost) {
                    this.set_active(devices[0]);
                }
            } catch (e) {
                if (!this.disposed && this.adb_available && request === this.request) this.error = String(e);
            } finally {
                this.refreshing = false;
                if (!this.disposed && this.adb_available && request !== this.request) this.refresh_devices();
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
      this.request++;
      clearInterval(this.interval); // Prevent Memory Leak
    }
}
</script>
