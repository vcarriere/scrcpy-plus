<template>
    <div v-if="manual || updates.state === 'available'" :class="manual ? 'mt-8' : 'card bg-base-200 w-full mb-8'">
        <div :class="manual ? 'flex flex-col items-start gap-2' : 'card-body'">
            <div v-if="manual" class="flex flex-wrap items-center gap-2 w-full">
                <slot />
                <button class="btn btn-primary text-white h-auto min-h-12 py-3" :disabled="updates.state === 'checking'" @click="check_updates">
                    <span>{{ $t('check_updates_now') }}</span>
                </button>
            </div>
            <p v-if="manual && ['checking', 'current'].includes(updates.state)" role="status">{{ $t(updates.state === 'checking' ? 'updates_checking' : 'updates_current') }}</p>
            <p v-if="manual && updates.state === 'error'" class="text-error break-words" role="alert">{{ $t('updates_check_failed') }}</p>

            <template v-if="updates.state === 'available'">
                <p class="card-title" :role="manual ? 'status' : null">{{ $t('update_available') }}</p>
                <div>{{ $t('new_version') }}</div>

                <div class="flex text-white">
                    <span>{{ updates.currentVersion }}</span>
                    <icon class="mx-4" size="25x25" name="arrow-right" />
                    <span>{{ updates.latestVersion }}</span>
                </div>

                <button class="btn btn-primary text-white" @click="open">
                    <icon class="mr-2" size="20x20" name="download" />
                    <span>{{ $t('download') }}</span>
                </button>
            </template>

        </div>
    </div>
</template>

<script>
import { openUrl } from "@tauri-apps/plugin-opener";
export default {
    props: { manual: Boolean },
    setup() {
        return useAppUpdates();
    },
    methods: {
        open() {
            openUrl(this.updates.release_url);
        }
    },
    mounted() {
        if (this.manual || localStorage.getItem("setting.disable_update_check") == "true") return;
        this.check_updates();
    }
}
</script>
