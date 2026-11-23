<template>
    <div class="p-8">
        <p v-if="!adb_available" class="mb-4">{{ $t(pause_message) }}</p>
        <p v-if="operation_error" class="text-error break-words mb-4" role="alert">{{ operation_error }}</p>

        <!--   Header   -->
        <div class="card bg-base-200 w-full mb-8">
            <div class="p-8">
                <div class="flex">
                    <button class="btn btn-primary w-12" @click="$router.go(-1)">
                        <icon name="reply" size="45x45" />
                    </button>
                    <p class="text-3xl ml-4 mt-1">{{ $t('wcs_title') }}</p>
                </div>

                <p class="mt-8">{{ $t('wcs_subtitle') }}</p>
                <a class="text-primary" href="https://developer.android.com/tools/adb#connect-to-a-device-over-wi-fi" target="_blank">{{ $t('pairing_info') }}</a>
            </div>
        </div>

        <div class="card bg-base-200 w-full mb-8" v-if="step == 0">
            <div class="p-8">
                
                <p class="card-title">{{ $t('wifi_pairing') }}</p>
                <input type="text" id="pair_code" :placeholder="$t('code')" class="input input-bordered w-full mt-2" />
                <input type="text" id="pair_ip_port" v-model="pair_ip_port" :placeholder="$t('ip_and_port')" class="input input-bordered w-full my-2" />

                <div class="card-actions justify-end">
                    <button class="btn" rounded :disabled="pairingLoading" @click="step++">{{ $t('skip') }}</button>
                    <button class="btn btn-primary" :disabled="pairingLoading || !adb_available" @click="pair">{{ $t('connect') }}</button>
                </div>

            </div>
        </div>

        <div class="card bg-base-200 w-full mb-8" v-if="step == 1">
            <div class="p-8">
                
                <p class="card-title">{{ $t('connecting') }}</p>
                <input type="text" id="connect_ip_port" v-model="connect_ip_port" :placeholder="$t('ip_and_port')" class="input input-bordered w-full my-2" />

                <div class="card-actions justify-end">
                    <button class="btn" rounded :disabled="pairingLoading" @click="step--">{{ $t('back') }}</button>
                    <button class="btn btn-primary" :disabled="pairingLoading || !adb_available" @click="connect">{{ $t('connect') }}</button>
                </div>

            </div>
        </div>

        <div class="card bg-base-200 w-full mb-8" v-if="step == 2">
            <div class="p-8">
                <center>
                    <icon name="link" size="50x50" />
                    <h1 class="text-2xl">{{ $t('success') }}</h1>
                </center>
            </div>
        </div>

    </div>
</template>

<style>
.v-input__slot {
    background: rgba(0,0,0,0) !important;
}
</style>

<script>
import { invoke } from "@tauri-apps/api/core";
export default {
    setup() {
        const { available: adb_available, pause_message } = useAdbStatus();
        return { adb_available, pause_message };
    },
    data() {
        return {
            notice: null,
            operation_error: "",
            pairingLoading: false,
            step: 0,
            connectingLoading: false,
            pair_ip_port: "",
            connect_ip_port: localStorage.getItem("scrcpyplus.last_ip_port") || "",
        }
    },

    created() {
        // The pairing port changes every time, so only the saved IP is prefilled
        const last_ip = this.connect_ip_port.split(":")[0];
        if (last_ip) this.pair_ip_port = `${last_ip}:`;
    },

    methods: {
        async pair() {
            if (!this.adb_available || this.pairingLoading) return;
            const ip = document.getElementById("pair_ip_port").value;
            const pair_code = document.getElementById("pair_code").value;
            if (ip == "" || pair_code == "") return; // Ensure No Error

            this.pairingLoading = true;
            this.operation_error = "";
            try {
                const data = await invoke("exec_adb", { args: ["pair", ip, pair_code] });
                this.notice = data;
                if (data.includes("Successfully")) this.step++;
            } catch (e) {
                this.operation_error = String(e);
            } finally {
                this.pairingLoading = false;
            }
        },

        async connect() {
            if (!this.adb_available || this.pairingLoading) return;
            const ip = this.connect_ip_port.trim();
            if (ip == "") return; // Ensure No Error

            this.pairingLoading = true;
            this.operation_error = "";
            try {
                const data = await invoke("exec_adb", { args: ["connect", ip] });
                this.notice = data;
                if (data.includes("connected")) {
                    localStorage.setItem("scrcpyplus.last_ip_port", ip);
                    this.step++;
                    setTimeout(() => {
                        this.$router.go(-1)
                    }, 2000)
                };
            } catch (e) {
                this.operation_error = String(e);
            } finally {
                this.pairingLoading = false;
            }
        }
    }
}
</script>
