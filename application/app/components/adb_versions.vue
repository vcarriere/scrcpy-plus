<template>
    <div class="card bg-base-200 w-full mb-8" aria-label="ADB versions">
        <div class="card-body min-w-0">
            <p class="card-title text-white">ADB</p>
            <p>Selected version: {{ current || 'Unknown' }}</p>
            <p>Choose ADB independently of scrcpy. Previously installed versions stay available locally.</p>
            <p>Downloading Platform-Tools is subject to the <a href="https://developer.android.com/studio/terms" class="link link-primary" @click.prevent="terms">Android SDK License Agreement</a>. Choose Download &amp; use to download under those terms.</p>
            <p v-if="loading">{{ $t('loading') }}</p>
            <p v-if="error" class="text-error break-words" role="alert">{{ error }}</p>
            <p v-if="!loading && !versions.length">No verified ADB packages are available for this platform.</p>
            <div v-for="release in versions" :key="`${release.version}:${release.adb_path || ''}`" class="flex flex-wrap items-center justify-between gap-4" :data-adb-version="release.version">
                <div class="min-w-0 break-words">
                    <p class="text-lg">{{ release.version }}</p>
                    <p>{{ release.compressed ? 'Kept locally (compressed)' : release.adb_path ? 'Kept locally' : 'Available to download' }}</p>
                </div>
                <span v-if="active(release)" class="badge badge-primary text-white">{{ $t('using_this_version') }}</span>
                <button v-else class="btn btn-primary text-white max-w-full" :disabled="!!installation.busy" @click="select(release)">
                    <span>{{ installation.busy === release.version && installation.tool === 'adb' && installation.stage !== 'confirming'
                        ? installation.stage === 'restoring' ? 'Restoring…' : installation.stage === 'compacting' ? 'Finishing…' : $t('downloading')
                        : release.adb_path || release.compressed ? 'Use this version' : $t('download_install') }}</span>
                </button>
            </div>
            <div class="flex flex-wrap items-center gap-4 mt-2">
                <button class="btn btn-primary text-white max-w-full" :disabled="loading || !!installation.busy" @click="compact">Compress inactive versions</button>
                <p class="min-w-0">Selected and running ADB versions stay unpacked.</p>
            </div>
            <p v-if="cache_result" class="break-words" role="status">{{ cache_result }}</p>
            <p v-if="installation.tool === 'adb' && installation.error" class="text-error break-words" role="alert">{{ installation.error }}</p>
            <p v-if="installation.tool === 'adb' && installation.preserved" role="status">{{ $t('install_custom_preserved') }}</p>
            <p v-if="installation.tool === 'adb' && installation.cache_warning" role="status">{{ installation.cache_warning }}</p>
        </div>
    </div>
</template>

<script>
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
export default {
    setup() { return { installation: useToolInstallation() }; },
    data() { return { releases: [], installed: [], current: "", loading: true, error: "", cache_result: "", disposed: false, revision: 0 } },
    computed: {
        versions() {
            const versions = [...this.installed];
            for (const release of this.releases) {
                if (!versions.some(local => compare_tool_versions(local.version, release.version) === 0)) versions.push(release);
            }
            return versions.sort((first, second) => compare_tool_versions(second.version, first.version) || 0);
        }
    },
    beforeUnmount() { this.disposed = true; },
    mounted() { this.load(); },
    methods: {
        terms() { openUrl("https://developer.android.com/studio/terms"); },
        active(release) { this.revision; return !release.compressed && is_managed_adb(release.version, release.adb_path); },
        async load() {
            const results = await Promise.allSettled([
                invoke("get_tool_releases", { tool: "adb" }), invoke("get_installed_adb"), invoke("get_dependency_versions")
            ]);
            if (this.disposed) return;
            this.releases = results[0].status === 'fulfilled' && Array.isArray(results[0].value) ? results[0].value : [];
            this.installed = results[1].status === 'fulfilled' && Array.isArray(results[1].value) ? results[1].value : [];
            this.current = results[2].status === 'fulfilled' ? results[2].value.adb || "" : "";
            this.error = results[0].status === 'rejected' ? "Could not check available ADB versions. Kept local versions can still be selected." : "";
            if (results[1].status === 'rejected') this.error += " Local ADB installations could not be verified.";
            this.loading = false;
        },
        async select(release) {
            if (this.installation.busy) return;
            this.cache_result = "";
            try {
                const result = await install_adb_release(release.version, false, release.adb_path, release.compressed === true);
                if (this.disposed || result.skipped) return;
                this.revision++;
                await this.load();
            } catch {}
        },
        async compact() {
            if (this.loading || this.installation.busy) return;
            Object.assign(this.installation, { busy: "adb-cache", tool: "adb", stage: "compacting", error: "", preserved: false, skipped: "", cache_warning: "" });
            this.cache_result = "";
            try {
                try {
                    const result = await invoke("compact_adb_cache");
                    if (!this.disposed) this.cache_result = this.compression_result(result);
                } catch (error) {
                    this.installation.cache_warning = `Inactive ADB versions could not be compressed. Your selected version is unchanged. ${String(error)}`;
                }
                try {
                    const installed = await invoke("get_installed_adb");
                    if (!this.disposed) this.installed = Array.isArray(installed) ? installed : [];
                } catch {
                    this.installation.cache_warning += `${this.installation.cache_warning ? ' ' : ''}The local ADB version list could not be refreshed.`;
                }
            } finally {
                this.installation.busy = "";
                this.installation.stage = "";
            }
        },
        compression_result(result) {
            if (result?.skipped === 'running_server_path_unavailable') return "Compression skipped because the running ADB server's executable could not be verified.";
            if (result?.reason === 'compression_not_supported_on_this_platform') return "Compression is not available on this platform.";
            const results = Array.isArray(result?.results) ? result.results : [];
            const compressed = results.filter(item => item.compressed === true).length;
            const kept = results.filter(item => !item.compressed && item.reason !== 'selected_or_running');
            const remaining = Number.isInteger(result?.remaining) && result.remaining > 0 ? result.remaining : 0;
            let message = compressed ? `Compressed ${compressed} inactive ADB ${compressed === 1 ? 'version' : 'versions'}.` : kept.length || remaining ? "No inactive ADB versions were compressed." : "No inactive ADB versions need compression.";
            if (kept.length) message += ` ${kept.length} inactive ${kept.length === 1 ? 'version was' : 'versions were'} kept unpacked.`;
            if (remaining) message += ` ${remaining} more ${remaining === 1 ? 'version remains' : 'versions remain'}; run again to continue.`;
            if (kept.some(item => item.error || item.reason === 'cleanup_failed_full_copy_restored')) this.installation.cache_warning = "Some inactive ADB versions could not be compressed. Their full copies were kept.";
            return message;
        }
    }
}
</script>
