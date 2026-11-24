<template>
    <div class="card bg-base-200 w-full mb-8">
        <div class="card-body">
            <!--   Version Information   -->
            <p class="card-title"><span class="font-bold">SCRCPY+</span> <span>{{ version.scrcpy_plus }}</span></p>
            <p><span class="font-bold">SCRCPY</span>  <span>{{ version.scrcpy || $t('version_unavailable') }}</span></p>
            <p class="flex flex-wrap items-center gap-x-2 gap-y-1"><span class="font-bold">ADB</span> <span>{{ version.adb || $t('version_unavailable') }}</span> <adb_status class="ml-auto text-sm" /></p>
            <p v-if="version_error" class="text-error" role="alert">{{ $t('version_check_failed') }}</p>
            <p v-if="status.paused" class="text-sm">
                {{ $t('adb_paused') }}
                <NuxtLink class="link" to="/settings">{{ $t('settings') }}</NuxtLink>
            </p>

            <!--   Buttons   -->
            <div class="card-actions">
                <button class="btn btn-primary text-white">
                    <icon name="discord" size="22x22" />
                    <span>Discord</span>
                </button>
                <NuxtLink to="/settings">
                    <button class="btn btn-primary text-white">
                        <icon name="gear" size="20x20" />
                        <span>{{  $t('settings') }}</span>
                    </button>
                </NuxtLink>
            </div>
        </div>
    </div>
</template>

<script>
import { invoke } from "@tauri-apps/api/core";
export default {
    setup() {
        const { status } = useAdbStatus();
        return { status };
    },
    data() {
        return {
            version_error: false,
            version: {
                scrcpy_plus: "",
                scrcpy: "",
                adb: "",
            }
        }
    },
    async mounted() {
        try {
            this.version = await invoke("get_dependency_versions");
        } catch (e) {
            this.version_error = true;
        }
    }
}
</script>
