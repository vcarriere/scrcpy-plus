<template>
    <div class="ml-8 mb-3 space-y-2">
        <p class="text-white">{{ $t('protected_screen_title') }}</p>
        <p v-if="enabled">{{ $t('protected_screen_description') }}</p>
        <button class="btn btn-primary text-white" :disabled="!enabled || !adb_available || checking || !device" @click="check">
            {{ $t(checking ? 'loading' : 'protected_screen_check') }}
        </button>
        <p v-if="observation" aria-live="polite">{{ $t(`protected_screen_${observation.state}`) }}</p>
        <p v-if="error" class="text-error break-words" role="alert">{{ error }}</p>
    </div>
</template>

<script>
import { invoke } from "@tauri-apps/api/core";
export default {
    props: ["device", "enabled"],
    setup() {
        const { available: adb_available } = useAdbStatus();
        return { adb_available };
    },
    data() {
        return { checking: false, observation: null, error: "", request: 0 }
    },
    watch: {
        device() { this.reset(); },
        enabled() { this.reset(); },
        adb_available() { this.reset(); }
    },
    beforeUnmount() { this.reset(); },
    methods: {
        reset() {
            this.request++;
            this.observation = null;
            this.error = "";
        },
        async check() {
            if (!this.enabled || !this.adb_available || !this.device || this.checking) return;
            const request = ++this.request;
            const device = this.device;
            this.checking = true;
            this.observation = null;
            this.error = "";
            try {
                const observation = await invoke("inspect_protected_screen", { device });
                if (request === this.request && device === this.device) {
                    this.observation = { ...observation, state: ["secure", "unflagged", "unknown"].includes(observation.state) ? observation.state : "unknown" };
                }
            } catch (e) {
                if (request === this.request && device === this.device) this.error = String(e);
            } finally {
                this.checking = false;
            }
        }
    }
}
</script>
