<template>
    <div class="card bg-base-200 w-full mb-8">
        <div class="card-body">
            <p class="card-title text-white">ADB <adb_status class="text-sm font-normal ml-auto" /></p>
            <dl class="space-y-2 min-w-0">
                <div><dt class="font-bold">{{ $t('adb_selected_executable') }}</dt><dd class="break-all">{{ client.path || status.path }}</dd></div>
                <div><dt class="font-bold">{{ $t('adb_client_version') }}</dt><dd>{{ client_loading ? $t('loading') : client.version || client.protocol_version || $t('adb_details_unavailable') }}</dd></div>
                <div><dt class="font-bold">{{ $t('adb_endpoint') }}</dt><dd class="break-all">{{ status.endpoint }}</dd></div>
                <template v-if="status.state === 'running'">
                    <div><dt class="font-bold">{{ $t('adb_server_version') }}</dt><dd>{{ server_version || $t('adb_details_unavailable') }}</dd></div>
                    <div><dt class="font-bold">{{ $t('adb_server_executable') }}</dt><dd class="break-all">{{ status.server_details?.path || $t('adb_details_unavailable') }}</dd></div>
                </template>
                <div v-if="status.protocol_version"><dt class="font-bold">{{ $t('adb_protocol') }}</dt><dd>{{ status.protocol_version }}</dd></div>
            </dl>
            <p v-if="status.state === 'running'">{{ $t(different_server ? 'adb_different_server' : 'adb_shared_server') }}</p>
            <p v-if="client.error" class="text-error break-words" role="alert">{{ client.error }}</p>
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
import { invoke } from "@tauri-apps/api/core";
export default {
    setup() {
        const { status, busy, error, control } = useAdbStatus();
        return { status, busy, error, control };
    },
    data() {
        return { confirm_stop: false, client: {}, client_loading: false, client_request: 0 }
    },
    computed: {
        server_version() {
            const details = this.status.server_details;
            return details?.version ? details.version + (details.build ? `-${details.build}` : "") : "";
        },
        different_server() {
            const server = this.status.server_details;
            const path = value => String(value || "").replaceAll("\\", "/").replace(/^\/\/\?\//, "");
            const different_path = server?.path && this.client.path && (this.client.is_windows
                ? path(server.path).toLowerCase() !== path(this.client.path).toLowerCase()
                : path(server.path) !== path(this.client.path));
            const different_version = server?.version && this.client.version && (server.build && this.client.version.includes("-")
                ? this.server_version !== this.client.version : server.version !== this.client.version.split("-")[0]);
            return different_path || different_version;
        }
    },
    watch: {
        "status.path"() { this.refresh_client(); },
        confirm_stop(show) {
            if (show) this.$refs.stop_dialog.showModal();
            else this.$refs.stop_dialog.close();
        }
    },
    mounted() { this.refresh_client(); },
    beforeUnmount() { this.client_request++; },
    methods: {
        async refresh_client() {
            const request = ++this.client_request;
            this.client = {};
            this.client_loading = true;
            try {
                const details = await invoke("get_adb_client");
                if (request === this.client_request) this.client = details;
            } catch (e) {
                if (request === this.client_request) this.client = { error: String(e) };
            } finally {
                if (request === this.client_request) this.client_loading = false;
            }
        },
        async stop() {
            this.confirm_stop = false;
            await this.control(false);
        }
    }
}
</script>
