<template>
    <div class="p-8">
        <div class="card bg-base-200 w-full mb-8">
            <div class="p-8 flex">
                <NuxtLink to="/settings">
                    <button class="btn btn-primary w-12">
                        <icon name="reply" size="45x45" />
                    </button>
                </NuxtLink>
                <p class="text-3xl ml-4 mt-1">{{ $t('scrcpy_manager') }}</p>
            </div>
        </div>

        <div class="card bg-base-200 w-full mb-8">
            <div class="card-body">
                <p v-if="loading">{{ $t('loading') }}</p>
                <p v-else-if="error" class="text-error">{{ $t('fetch_failed') }}: {{ error }}</p>

                <div v-for="rel, i in releases" :key="rel.tag_name" class="flex flex-wrap items-center justify-between gap-4">
                    <div class="min-w-0 break-words">
                        <p class="text-lg">{{ rel.tag_name }}</p>
                        <p v-if="!rel.asset" class="opacity-50">{{ $t('no_build_available') }}</p>
                    </div>
                    <span v-if="is_active(rel)" class="badge badge-primary text-white">{{ $t('using_this_version') }}</span>
                    <button v-else class="btn text-white" :class="i == 0 ? 'btn-primary' : ''" :disabled="!rel.asset || !!installation.busy" @click="install(rel)">
                        <icon class="mr-2" size="20x20" name="download" />
                        <span>{{ installation.busy === rel.tag_name ? $t('downloading') : $t('download_install') }}</span>
                    </button>
                </div>

                <p v-if="installation.error" class="text-error break-words" role="alert">{{ installation.error }}</p>
                <p v-if="installation.preserved" role="status">{{ $t('install_custom_preserved') }}</p>
            </div>
        </div>

        <div v-if="installation.busy" class="modal modal-open">
            <div class="modal-box text-center">
                <span class="loading loading-spinner loading-lg text-primary"></span>
                <p class="text-xl mt-4">{{ $t('downloading') }}</p>
                <p class="opacity-50">{{ installation.busy }}</p>
            </div>
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
            releases: [],
            loading: true,
            error: "",
            os: "",
            arch: "",
            disposed: false
        }
    },
    methods: {
        find_asset(assets) {
            return find_scrcpy_asset(assets, this.os, this.arch);
        },
        is_active(rel) {
            return is_managed_scrcpy(rel.tag_name);
        },
        async install(rel) {
            if (this.installation.busy || !rel.asset) return;
            try {
                await install_scrcpy_release(rel.asset.browser_download_url, rel.tag_name);
            } catch {}
        }
    },
    async mounted() {
        try {
            const env = await invoke("environment");
            this.os = env.os;
            this.arch = env.arch;
            const data = await $fetch(SCRCPY_RELEASES_URL, { timeout: 10000, retry: 0 });
            if (this.disposed) return;
            this.releases = data
                .filter(r => !r.draft && !r.prerelease)
                .map(r => ({ tag_name: r.tag_name, asset: this.find_asset(r.assets) }));
        } catch (e) {
            this.error = String(e);
        } finally {
            this.loading = false;
        }
    },
    beforeUnmount() {
        this.disposed = true;
    }
}
</script>
