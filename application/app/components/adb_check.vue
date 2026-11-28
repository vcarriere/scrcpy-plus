<template>
    <div class="card bg-base-200 w-full mb-8" v-if="missing || installation.preserved">
        <div class="card-body">
            <p class="card-title">{{ $t(missing ? 'adb_missing' : 'download_complete') }}</p>
            <div v-if="missing">{{ $t('adb_missing_desc') }}</div>

            <button v-if="missing" class="btn btn-primary text-white" :disabled="!!installation.busy" @click="download">
                <icon class="mr-2" size="20x20" name="download" />
                <span>{{ $t('download') }}</span>
            </button>
            <p v-if="installation.error" class="text-error break-words" role="alert">{{ installation.error }}</p>
            <p v-if="installation.preserved" role="status">{{ $t('install_custom_preserved') }}</p>
        </div>
    </div>

    <div v-if="installation.busy" class="modal modal-open">
        <div class="modal-box text-center">
            <span class="loading loading-spinner loading-lg text-primary"></span>
            <p class="text-xl mt-4">{{ $t('downloading') }}</p>
        </div>
    </div>
</template>

<script>
import { invoke } from "@tauri-apps/api/core";
export default {
    setup() {
        return { installation: useScrcpyInstallation() };
    },
    data() {
        return {
            missing: false,
            disposed: false
        }
    },
    methods: {
        async download() {
            if (this.installation.busy) return;
            try {
                const result = await install_latest_scrcpy();
                if (this.disposed) return;
                await this.check();
                if (!this.disposed && !this.missing && !result.preserved) location.reload();
            } catch {}
        },
        async check() {
            try {
                const output = await invoke("exec_adb", { args: ["--version"] });
                if (!this.disposed) this.missing = !output || !output.toLowerCase().includes("android debug bridge");
            } catch {
                if (!this.disposed) this.missing = true;
            }
        }
    },
    async mounted() {
        await this.check();
    },
    beforeUnmount() {
        this.disposed = true;
    }
}
</script>
