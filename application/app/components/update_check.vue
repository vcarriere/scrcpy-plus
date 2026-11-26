<template>
    <div v-if="manual || updates.state === 'available'" :class="manual ? 'mt-8' : 'card bg-base-200 w-full mb-8'">
        <div :class="manual ? 'flex flex-col items-start gap-2' : 'card-body'">
            <div v-if="manual" class="flex flex-wrap items-center gap-2 w-full">
                <slot />
                <button class="btn btn-primary text-white h-6 min-h-0 px-3" :disabled="updates.state === 'checking'" @click="check_updates">
                    <span>{{ $t('check_updates_now') }}</span>
                </button>
            </div>
            <p v-if="manual && ['checking', 'current'].includes(updates.state)" role="status">{{ $t(updates.state === 'checking' ? 'updates_checking' : 'updates_current') }}</p>
            <p v-if="manual && updates.state === 'error'" class="text-error break-words" role="alert">{{ $t('updates_check_failed') }}</p>

            <p v-if="updates.app_state !== 'available' && updates.tools?.some(tool => tool.state === 'available')" class="card-title" :role="manual ? 'status' : null">{{ $t('update_available') }}</p>
            <template v-if="updates.app_state === 'available'">
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

            <div v-if="updates.tools?.length && updates.state !== 'checking'" class="space-y-3 w-full min-w-0">
                <div v-for="tool in updates.tools" :key="tool.tool" class="flex flex-wrap items-center justify-between gap-2">
                    <div class="min-w-0 break-words">
                        <p class="text-white">{{ tool.tool === 'adb' ? 'ADB' : 'scrcpy' }}: {{ tool.current_version || $t('version_unavailable') }}<span v-if="tool.latest_version"> → {{ tool.latest_version }}</span></p>
                        <p>{{ $t(`tool_update_${tool.state}`) }}</p>
                        <p v-if="tool.install_state">{{ install_message(tool.install_state) }}</p>
                    </div>
                    <NuxtLink v-if="tool.state === 'available' || tool.state === 'unknown'" to="/scrcpy_manager">
                        <button class="btn btn-primary text-white h-10 min-h-0">{{ $t('manage_versions') }}</button>
                    </NuxtLink>
                </div>
            </div>

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
        install_message(state) {
            return {
                busy: 'Installation will wait until current sessions finish.', custom_executable: 'The custom executable was kept. Select a managed version to use automatic installation.',
                control_compatibility: 'Automatic unlock and screen switching require scrcpy 5.0.1. This update needs a manual choice.',
                sdk_license: 'Enable automatic ADB installation in Advanced settings to accept the Android SDK terms.',
                version_unknown: 'The installed version could not be verified. Select a version manually.',
                selection_changed: 'The executable selection changed. Check versions again.', error: 'Automatic installation failed; the selected version was kept. You can retry in Manage versions.',
                installed: 'Update installed.',
            }[state] || '';
        },
        open() {
            openUrl(this.updates.release_url);
        }
    },
    mounted() {
        if (this.manual || localStorage.getItem("setting.disable_update_check") == "true") return;
        if (this.updates.checked_at && Date.now() - this.updates.checked_at < 6 * 60 * 60 * 1000) return;
        this.check_updates();
    }
}
</script>
