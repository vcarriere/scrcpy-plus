<template>
    <div class="ml-8 mb-3 min-w-0 space-y-2">
        <label class="flex cursor-pointer">
            <input type="checkbox" class="checkbox checkbox-primary shrink-0" :checked="modelValue" :disabled="!enabled || running || session" @change="$emit('update:modelValue', $event.target.checked)" />
            <p class="ml-2 text-white">{{ $t('protected_screen_automatic') }}</p>
        </label>
        <p v-if="enabled">{{ $t('protected_screen_description') }}</p>
        <template v-if="session">
            <p aria-live="polite">{{ $t(`screen_auto_${session.state}`) }}</p>
            <p v-if="session.state === 'paused' || session.state === 'error'">{{ $t(`screen_auto_reason_${session.reason}`) }}</p>
        </template>
        <div class="flex flex-wrap gap-2">
            <button v-if="session" class="btn btn-primary text-white max-w-full min-h-12 h-auto py-3 whitespace-normal" :disabled="controlling" @click="control">
                {{ $t(session.state === 'paused' || session.state === 'error' ? 'screen_auto_resume' : 'screen_auto_pause') }}
            </button>
            <button class="btn btn-primary text-white max-w-full min-h-12 h-auto py-3 whitespace-normal" :disabled="!enabled || !adb_available || checking || !device" @click="check">
                {{ $t(checking ? 'loading' : 'protected_screen_check') }}
            </button>
        </div>
        <div v-if="enabled" class="space-y-2" role="status" aria-live="polite" :aria-busy="checking">
            <p v-if="checking">{{ $t('protected_screen_checking') }}</p>
            <template v-else-if="observation">
                <p class="text-white">{{ $t(`protected_screen_${observation.state}`) }}</p>
                <p>{{ $t('protected_screen_checked_at', { time: checked_at }) }}</p>
            </template>
            <p v-else>{{ $t('protected_screen_check_hint') }}</p>
        </div>
        <p v-if="error" class="text-error break-words" role="alert">{{ error }}</p>
    </div>
</template>

<script>
import { invoke } from "@tauri-apps/api/core";
export default {
    props: ["device", "enabled", "modelValue", "running"],
    emits: ["update:modelValue"],
    setup() {
        const { available: adb_available, status, refresh } = useAdbStatus();
        return { adb_available, status, refresh };
    },
    data() {
        return { checking: false, controlling: false, observation: null, checked_at: "", error: "", request: 0 }
    },
    computed: {
        session() { return this.status.screen_sessions?.find(session => session.device === this.device); }
    },
    watch: {
        device() { this.reset(); },
        enabled() { this.reset(); },
        adb_available() { this.reset(); }
    },
    beforeUnmount() { this.reset(); },
    methods: {
        async control() {
            if (!this.session || this.controlling) return;
            this.controlling = true;
            this.error = "";
            try {
                await invoke("control_screen_power", { id: this.session.id, paused: !["paused", "error"].includes(this.session.state) });
                await this.refresh();
            } catch (e) {
                this.error = String(e);
            } finally {
                this.controlling = false;
            }
        },
        reset() {
            this.request++;
            this.checking = false;
            this.observation = null;
            this.checked_at = "";
            this.error = "";
        },
        async check() {
            if (!this.enabled || !this.adb_available || !this.device || this.checking) return;
            const request = ++this.request;
            const device = this.device;
            this.checking = true;
            this.observation = null;
            this.checked_at = "";
            this.error = "";
            try {
                const observation = await invoke("inspect_protected_screen", { device });
                if (request === this.request && device === this.device) {
                    this.observation = { ...observation, state: ["secure", "unflagged", "unknown"].includes(observation.state) ? observation.state : "unknown" };
                    this.checked_at = new Date().toLocaleTimeString();
                }
            } catch (e) {
                if (request === this.request && device === this.device) this.error = String(e);
            } finally {
                if (request === this.request) this.checking = false;
            }
        }
    }
}
</script>
