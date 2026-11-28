<template>
    <div class="p-8">
        <p v-if="!adb_available" class="mb-4">{{ $t('adb_paused') }}</p>
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
                <input type="text" id="pair_code" v-model="pair_code" :aria-label="$t('code')" :placeholder="$t('code')" :disabled="pairingLoading" class="input input-bordered w-full mt-2" />
                <input type="text" id="pair_ip_port" v-model="pair_ip_port" :aria-label="$t('ip_and_port')" :placeholder="$t('ip_and_port')" :disabled="pairingLoading" class="input input-bordered w-full my-2" />

                <div class="card-actions justify-end">
                    <button class="btn" rounded :disabled="pairingLoading" @click="step++">{{ $t('skip') }}</button>
                    <button class="btn btn-primary" :disabled="pairingLoading || !adb_available" @click="pair">{{ $t('connect') }}</button>
                </div>

            </div>
        </div>

        <div class="card bg-base-200 w-full mb-8" v-if="step == 1">
            <div class="p-8">
                
                <p class="card-title">{{ $t('connecting') }}</p>
                <input type="text" id="connect_ip_port" v-model="connect_ip_port" :aria-label="$t('ip_and_port')" :placeholder="$t('ip_and_port')" :disabled="pairingLoading" class="input input-bordered w-full my-2" />

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
        const { available: adb_available } = useAdbStatus();
        return { adb_available };
    },
    data() {
        return {
            notice: null,
            operation_error: "",
            pairingLoading: false,
            step: 0,
            disposed: false,
            redirect: null,
            pair_code: "",
            pair_ip_port: "",
            connect_ip_port: localStorage.getItem("scrcpyplus.last_ip_port") || "",
        }
    },

    created() {
        // The pairing port changes every time, so only the saved IP is prefilled
        const last_ip = this.connect_ip_port.match(/^(.*):\d+$/)?.[1];
        if (last_ip) this.pair_ip_port = `${last_ip}:`;
    },

    beforeUnmount() {
        this.disposed = true;
        clearTimeout(this.redirect);
    },

    methods: {
        valid_endpoint(endpoint) {
            const match = endpoint.match(/^(\[[^\]\s]+\]|[A-Za-z0-9._-]+):(\d+)$/);
            return !!match && Number(match[2]) > 0 && Number(match[2]) <= 65535;
        },
        async pair() {
            if (!this.adb_available || this.pairingLoading) return;
            const ip = this.pair_ip_port.trim();
            const pair_code = this.pair_code.trim();
            if (!this.valid_endpoint(ip)) { this.operation_error = this.$t('wireless_invalid_endpoint'); return; }
            if (!/^\d{6}$/.test(pair_code)) { this.operation_error = this.$t('wireless_invalid_code'); return; }

            this.pairingLoading = true;
            this.operation_error = "";
            try {
                const data = await invoke("exec_adb", { args: ["pair", ip, pair_code] });
                if (this.disposed) return;
                this.notice = data;
                if (/^Successfully paired to\s+\S+/im.test(data)) { this.step++; this.pair_code = ""; }
                else this.operation_error = this.$t('wireless_pair_failed');
            } catch (e) {
                this.operation_error = String(e);
            } finally {
                this.pairingLoading = false;
            }
        },

        async connect() {
            if (!this.adb_available || this.pairingLoading) return;
            const ip = this.connect_ip_port.trim();
            if (!this.valid_endpoint(ip)) { this.operation_error = this.$t('wireless_invalid_endpoint'); return; }

            this.pairingLoading = true;
            this.operation_error = "";
            try {
                const data = await invoke("exec_adb", { args: ["connect", ip] });
                if (this.disposed) return;
                this.notice = data;
                if (/^(?:already )?connected to\s+\S+/im.test(data)) {
                    localStorage.setItem("scrcpyplus.last_ip_port", ip);
                    this.step++;
                    this.redirect = setTimeout(() => {
                        if (!this.disposed) this.$router.push("/");
                    }, 2000);
                } else this.operation_error = this.$t('wireless_connect_failed');
            } catch (e) {
                this.operation_error = String(e);
            } finally {
                this.pairingLoading = false;
            }
        }
    }
}
</script>
