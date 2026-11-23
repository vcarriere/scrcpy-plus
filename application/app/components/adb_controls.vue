<template>
    <div class="card bg-base-200 w-full mb-8">
        <div class="card-body">
            <p class="card-title text-white">ADB <adb_status class="text-sm font-normal ml-auto" /></p>
            <dl class="space-y-2 min-w-0">
                <div><dt class="font-bold">{{ $t('adb_executable') }}</dt><dd class="break-all">{{ status.path }}</dd></div>
                <div><dt class="font-bold">{{ $t('adb_endpoint') }}</dt><dd class="break-all">{{ status.endpoint }}</dd></div>
                <div v-if="status.protocol_version"><dt class="font-bold">{{ $t('adb_protocol') }}</dt><dd>{{ status.protocol_version }}</dd></div>
            </dl>
            <p v-if="status.paused">{{ $t('adb_paused') }}</p>
            <p v-if="status.active_sessions">{{ $t('adb_close_sessions') }}</p>
            <p v-if="error || status.error" class="text-error break-words" role="alert">{{ error || status.error }}</p>
            <div class="card-actions mt-2">
                <button class="btn btn-primary text-white" :disabled="busy || status.state === 'running'" @click="control(true)">
                    <icon name="play" size="20x20" />
                    <span>{{ $t('adb_start') }}</span>
                </button>
                <button class="btn btn-primary text-white" :disabled="busy || status.state !== 'running' || status.active_sessions > 0" @click="confirm_stop = true">
                    <icon name="stop" size="20x20" />
                    <span>{{ $t('adb_stop') }}</span>
                </button>
            </div>
            <dialog ref="stop_dialog" class="modal" aria-labelledby="adb-stop-title" @close="confirm_stop = false">
                <div class="modal-box">
                    <h2 id="adb-stop-title" class="font-bold text-lg">{{ $t('adb_stop') }}</h2>
                    <p class="py-4">{{ $t('adb_stop_warning') }}</p>
                    <div class="modal-action">
                        <button class="btn" autofocus @click="confirm_stop = false">{{ $t('cancel') }}</button>
                        <button class="btn btn-primary text-white" @click="stop">
                            <icon name="stop" size="20x20" />
                            <span>{{ $t('adb_stop') }}</span>
                        </button>
                    </div>
                </div>
            </dialog>
        </div>
    </div>
</template>

<style scoped>
dialog:not([open]) {
    display: none;
}
</style>

<script>
export default {
    setup() {
        const { status, busy, error, control } = useAdbStatus();
        return { status, busy, error, control };
    },
    data() {
        return { confirm_stop: false }
    },
    watch: {
        confirm_stop(show) {
            if (show) this.$refs.stop_dialog.showModal();
            else this.$refs.stop_dialog.close();
        }
    },
    methods: {
        async stop() {
            this.confirm_stop = false;
            await this.control(false);
        }
    }
}
</script>
