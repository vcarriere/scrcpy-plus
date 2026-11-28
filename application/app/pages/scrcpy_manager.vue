<template>
    <div class="p-8">
        <div class="card bg-base-200 w-full mb-8">
            <div class="p-8 flex">
                <NuxtLink to="/settings">
                    <button class="btn btn-primary w-12">
                        <icon name="reply" size="45x45" />
                    </button>
                </NuxtLink>
                <p class="text-3xl ml-4 mt-1">{{ $t('manage_versions') }}</p>
            </div>
        </div>

        <div class="card bg-base-200 w-full mb-8">
            <div class="card-body">
                <p class="card-title text-white">scrcpy</p>
                <p>Automatic unlock and protected-screen switching currently require scrcpy 5.0.1.</p>
                <p v-if="loading">{{ $t('loading') }}</p>
                <p v-else-if="error" class="text-error">{{ $t('fetch_failed') }}: {{ error }}</p>

                <div v-for="rel, i in releases" :key="`${rel.tag_name}:${rel.installed?.scrcpy_path || ''}`" class="flex flex-wrap items-center justify-between gap-4">
                    <div class="min-w-0 break-words">
                        <p class="text-lg">{{ rel.tag_name }}</p>
                        <p v-if="rel.installed">Kept locally</p>
                        <p v-else-if="!rel.asset" class="opacity-50">{{ $t('no_build_available') }}</p>
                    </div>
                    <span v-if="is_active(rel)" class="badge badge-primary text-white">{{ $t('using_this_version') }}</span>
                    <button v-else class="btn text-white" :class="i == 0 ? 'btn-primary' : ''" :disabled="(!rel.asset && !rel.installed) || !!installation.busy" @click="install(rel)">
                        <icon class="mr-2" size="20x20" name="download" />
                        <span>{{ installation.busy === rel.tag_name && installation.tool === 'scrcpy' && installation.stage !== 'confirming' ? $t('downloading') : rel.installed ? 'Use this version' : $t('download_install') }}</span>
                    </button>
                </div>

                <p v-if="installation.tool === 'scrcpy' && installation.error" class="text-error break-words" role="alert">{{ installation.error }}</p>
                <p v-if="installation.tool === 'scrcpy' && installation.preserved" role="status">{{ $t('install_custom_preserved') }}</p>
            </div>
        </div>

        <adb_versions />

        <div v-if="installation.busy && installation.stage !== 'confirming'" class="modal modal-open">
            <div class="modal-box text-center">
                <span class="loading loading-spinner loading-lg text-primary"></span>
                <p class="text-xl mt-4">{{ installation.stage === 'restoring' ? 'Restoring…' : installation.stage === 'compacting' ? 'Compressing inactive versions…' : $t('downloading') }}</p>
                <p class="opacity-50">{{ installation.busy === 'adb-cache' ? 'ADB' : installation.busy }}</p>
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
            return is_managed_scrcpy(rel.tag_name, rel.installed?.scrcpy_path);
        },
        async install(rel) {
            if (this.installation.busy || (!rel.asset && !rel.installed)) return;
            try {
                await install_scrcpy_release(rel.asset?.browser_download_url || '', rel.tag_name, false, rel.installed);
            } catch {}
        }
    },
    async mounted() {
        try {
            const env = await invoke("environment");
            this.os = env.os;
            this.arch = env.arch;
            const results = await Promise.allSettled([
                $fetch(SCRCPY_RELEASES_URL, { timeout: 10000, retry: 0 }), invoke("get_installed_scrcpy")
            ]);
            if (this.disposed) return;
            const data = results[0].status === 'fulfilled' && Array.isArray(results[0].value) ? results[0].value : [];
            const installed = results[1].status === 'fulfilled' && Array.isArray(results[1].value) ? results[1].value : [];
            const available = data.filter(r => !r.draft && !r.prerelease);
            this.releases = installed.map(local => ({ tag_name: local.version,
                asset: this.find_asset(available.find(release => compare_tool_versions(release.tag_name, local.version) === 0)?.assets || []), installed: local }));
            for (const release of available) {
                if (!installed.some(local => compare_tool_versions(local.version, release.tag_name) === 0)) this.releases.push({ tag_name: release.tag_name, asset: this.find_asset(release.assets), installed: null });
            }
            this.releases.sort((first, second) => compare_tool_versions(second.tag_name, first.tag_name) || 0);
            if (results[0].status === 'rejected') this.error = 'Could not check available scrcpy versions. Kept local versions can still be selected.';
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
