<template>
    <div class="card bg-base-200 w-full mb-8">
        <div class="card-body">
            <!--   Version Information   -->
            <p class="card-title"><span class="font-bold">SCRCPY+</span> <span>{{ version.scrcpy_plus }}</span></p>
            <p><span class="font-bold">SCRCPY</span>  <span>{{ version.scrcpy }}</span></p>
            <p class="flex flex-wrap items-center gap-x-2 gap-y-1"><span class="font-bold">ADB</span> <span>{{ version.adb }}</span> <adb_status class="ml-auto text-sm" /></p>
            <p v-if="status.paused" class="text-sm">
                {{ $t(pause_message) }}
                <button v-if="paused_running" class="link" :disabled="busy" @click="control(true)">{{ $t('adb_resume') }}</button>
                <NuxtLink v-else class="link" to="/settings">{{ $t('settings') }}</NuxtLink>
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
        const { status, busy, control, paused_running, pause_message } = useAdbStatus();
        return { status, busy, control, paused_running, pause_message };
    },
    data() {
        return {
            version: {
                scrcpy_plus: "",
                scrcpy: "",
                adb: "",
            }
        }
    },
    async mounted() {
        this.version = await invoke("get_dependency_versions");
    }
}
</script>
