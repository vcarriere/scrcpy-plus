<template>
    <section class="collapse collapse-arrow bg-base-300" aria-label="Automatic unlock">
        <input type="checkbox" aria-label="Show automatic unlock controls" />
        <div class="collapse-title text-white">Automatic unlock</div>
        <div class="collapse-content space-y-3 min-w-0">
            <p>Unlock this phone at startup and when it locks again during your SCRCPY session.</p>
            <p v-if="!verified">Select a verified, connected device to configure automatic unlock.</p>
            <template v-else>
                <div role="status" aria-live="polite" :aria-busy="busy === 'inspect'">
                    <p class="text-white">Detected unlock method: {{ method_label }}</p>
                    <p v-if="observation?.user !== null && observation?.user !== undefined">Android user: {{ observation.user }}</p>
                    <p v-if="observation?.enrolled">Saved credential: Password</p>
                    <p v-if="observation?.blocked && !['credential_record_unavailable', 'method_changed'].includes(observation.reason)" class="text-white">Automatic unlock is paused. Unlock this phone manually once to recover.</p>
                    <p v-else-if="observation?.enabled">{{ session_label }}</p>
                    <p v-if="reason_label">{{ reason_label }}</p>
                </div>
                <p v-if="observation && !password_supported">{{ unsupported_message }}</p>
                <button class="btn btn-primary text-white max-w-full min-h-10 h-auto py-2 whitespace-normal" :disabled="!adb_available || !!busy" @click="check()">
                    {{ busy === 'inspect' ? 'Checking…' : 'Check unlock method' }}
                </button>
                <template v-if="observation?.active">
                    <p>Stop this session before changing its unlock settings. Stop remains available while the mirror is closed for private entry.</p>
                    <button class="btn btn-primary text-white max-w-full min-h-10 h-auto py-2 whitespace-normal"
                        :disabled="(!!busy && busy !== 'inspect') || observation.state === 'stopping'" @click="stop_session">{{ busy === 'stop' || observation.state === 'stopping' ? 'Stopping…' : 'Stop session' }}</button>
                </template>
                <template v-if="observation?.enrolled">
                    <label class="flex cursor-pointer">
                        <input type="checkbox" class="checkbox checkbox-primary shrink-0" :checked="observation.enabled"
                            :disabled="!adb_available || !!busy || observation.active || ((!password_supported || observation.reason === 'method_changed') && !observation.enabled)" @change="set_enabled($event.target.checked)" />
                        <span class="ml-2 text-white">Automatically unlock this device</span>
                    </label>
                    <div class="flex flex-wrap gap-2">
                        <button v-if="password_supported && !editing" class="btn btn-primary text-white max-w-full min-h-10 h-auto py-2 whitespace-normal" :disabled="!!busy || observation.active" @click="editing = true">Replace saved password</button>
                        <button class="btn btn-primary text-white max-w-full min-h-10 h-auto py-2 whitespace-normal" :disabled="!!busy || !adb_available || observation.active" @click="remove()">Remove saved password</button>
                    </div>
                </template>
                <form v-if="password_supported && !observation.active && (!observation.enrolled || editing)" class="space-y-3" autocomplete="off" @submit.prevent="enroll">
                    <label class="block">
                        <span class="text-white">Device password</span>
                        <input ref="password_field" v-model="password" type="password" autocomplete="new-password" spellcheck="false" autocapitalize="off"
                            class="input input-bordered w-full" minlength="4" maxlength="64" :disabled="!!busy" required />
                    </label>
                    <label class="block">
                        <span class="text-white">Confirm device password</span>
                        <input v-model="confirmation" type="password" autocomplete="new-password" spellcheck="false" autocapitalize="off"
                            class="input input-bordered w-full" minlength="4" maxlength="64" :disabled="!!busy" required />
                    </label>
                    <p>Use 4–64 printable ASCII characters. Passwords are protected by your operating system and are never included in saved SCRCPY settings.</p>
                    <div class="flex flex-wrap gap-2">
                        <button type="submit" class="btn btn-primary text-white max-w-full min-h-10 h-auto py-2 whitespace-normal" :disabled="!!busy || !adb_available">{{ busy === 'enroll' ? 'Saving…' : 'Save password' }}</button>
                        <button type="button" class="btn max-w-full min-h-10 h-auto py-2" :disabled="!!busy" @click="cancel">Cancel</button>
                    </div>
                </form>
                <p>Private entry requires Turn off phone screen. SCRCPY closes the mirror during password entry and reopens it after verification. Recording and capture options cannot be used with automatic unlock.</p>
                <p>Currently supported: Windows, Android 15 or later, and the direct scrcpy 5.0.1 executable with its matching server. PIN and pattern playback will come later. Fingerprint and face prompts must be completed on the phone.</p>
            </template>
            <p v-if="error" class="text-error break-words" role="alert">{{ error }}</p>
        </div>
    </section>
</template>

<script>
import { invoke } from "@tauri-apps/api/core";
export default {
    props: ["device", "selection", "adb_available"],
    emits: ["updated"],
    data() {
        return { observation: null, password: "", confirmation: "", editing: false, busy: "", error: "", request: 0, timer: null, ready: false }
    },
    computed: {
        target() { return JSON.stringify([this.device, this.selection?.serial, this.selection?.transport_id, this.selection?.identity, this.selection?.adb_path, this.selection?.server_socket]); },
        verified() { return !!this.device && !!this.selection?.identity && this.selection.serial === this.device; },
        password_supported() { return this.verified && this.adb_available && this.observation?.method === "password" && this.observation.supported === true; },
        method_label() {
            return { password: "Password", pin: "PIN", pattern: "Pattern", none: "No secure credential", unknown: "Unknown" }[this.observation?.method] || "Unknown";
        },
        unsupported_message() {
            if (["pin", "pattern"].includes(this.observation.method)) return "This unlock method was detected, but automatic playback currently supports passwords only.";
            if (this.observation.method === "none") return "No secure device credential was detected. Automatic password input is disabled.";
            if (this.observation.method === "unknown") return "The unlock method could not be verified. Automatic input is disabled.";
            return "Automatic password unlock is unavailable for this device or operating system.";
        },
        session_label() {
            return {
                preparing: "Preparing private password entry…", unlocking: "Entering the password privately…",
                monitoring: "Automatic unlock is monitoring this session.", waiting: "Waiting for the password screen.",
                unavailable: "Automatic unlock is unavailable. Unlock the phone manually.",
                blocked: "Automatic unlock is paused. Unlock this phone manually once to recover.",
                stopping: "Stopping this session…",
            }[this.observation?.state] || "Automatic unlock is enabled for this device.";
        },
        reason_label() {
            return {
                credential_record_unavailable: "The saved password record is unavailable. Remove it and enroll a new password.",
                method_changed: "The device unlock method changed. Replace the saved password before enabling automatic unlock again.",
                screen_off_unavailable: "Waiting for the phone screen to turn off. The password is not being retried.",
                new_lock_episode: "The phone locked again. Preparing a guarded unlock.",
            }[this.observation?.reason] || "";
        }
    },
    watch: {
        target() { this.reset(); this.check(); },
        adb_available(available) {
            const observation = this.observation?.active ? { ...this.observation, method: "unknown", supported: false, state: "unavailable" } : null;
            this.reset();
            if (!available && observation) this.apply(observation);
            if (available) this.check();
        }
    },
    mounted() { this.ready = true; this.check(); },
    beforeUnmount() { this.ready = false; this.reset(); },
    methods: {
        clear_password() { this.password = ""; this.confirmation = ""; },
        cancel() { this.clear_password(); this.editing = false; this.error = ""; },
        reset() {
            this.request++;
            clearTimeout(this.timer);
            this.timer = null;
            this.busy = "";
            this.observation = null;
            this.cancel();
        },
        apply(observation) {
            const methods = ["password", "pin", "pattern", "none", "unknown"];
            this.observation = {
                ...observation, method: methods.includes(observation?.method) ? observation.method : "unknown",
                supported: observation?.supported === true, enrolled: observation?.enrolled === true,
                enabled: observation?.enabled === true, blocked: observation?.blocked === true, active: observation?.active === true,
            };
            if (!this.password_supported || this.observation.active) { this.clear_password(); this.editing = false; }
            this.$emit("updated", this.observation);
        },
        schedule() {
            clearTimeout(this.timer);
            this.timer = null;
            if (this.ready && this.adb_available && this.verified && this.observation?.enabled) {
                this.timer = setTimeout(() => this.check(true), 5000);
            }
        },
        async check(background = false) {
            if (!this.ready || !this.verified || !this.adb_available || this.busy) return;
            if (background && (this.editing || this.password || this.confirmation)) { this.schedule(); return; }
            await this.perform("inspect", "inspect_unlock", { selection: { ...this.selection } });
        },
        async perform(action, command, args) {
            if (!this.ready || !this.verified || (!this.adb_available && action !== "stop") || (this.busy && !(action === "stop" && this.busy === "inspect"))) return;
            const request = ++this.request;
            const target = this.target;
            clearTimeout(this.timer);
            this.timer = null;
            this.busy = action;
            this.error = "";
            try {
                const observation = await invoke(command, args);
                if (!this.ready || request !== this.request || target !== this.target) return;
                this.apply(observation);
                if (action !== "inspect") this.cancel();
            } catch (e) {
                if (!this.ready || request !== this.request || target !== this.target) return;
                this.error = action === "inspect" ? "Could not check the unlock method. Check the selected connection and try again."
                    : action === "stop" ? "Could not stop the session. Check the selected connection and try again."
                    : "Could not save this change. Check the connection and supported password requirements, then try again.";
                if (action === "inspect") this.apply({ ...this.observation, method: "unknown", supported: false, state: "unavailable" });
                this.clear_password();
            } finally {
                if (request === this.request) { this.busy = ""; this.schedule(); }
                // Remove the local IPC copy as soon as the native request completes.
                if (Object.hasOwn(args, "password")) args.password = "";
                if (Object.hasOwn(args, "confirmation")) args.confirmation = "";
            }
        },
        async enroll() {
            if (!this.password_supported || this.busy || this.observation.active) return;
            if (!/^[\x20-\x7e]{4,64}$/.test(this.password)) { this.error = "Use 4–64 printable ASCII characters."; return; }
            if (this.password !== this.confirmation) { this.error = "The passwords do not match."; return; }
            const args = { selection: { ...this.selection }, password: this.password, confirmation: this.confirmation };
            this.clear_password();
            await this.perform("enroll", "enroll_unlock", args);
        },
        async set_enabled(enabled) {
            if (!this.observation?.enrolled || this.observation.active || (enabled && (!this.password_supported || this.observation.reason === "method_changed"))) return;
            await this.perform("configure", "set_unlock_enabled", { selection: { ...this.selection }, enabled });
        },
        async remove() {
            if (!this.observation?.enrolled || this.observation.active) return;
            await this.perform("remove", "remove_unlock", { selection: { ...this.selection } });
        },
        async stop_session() {
            if (!this.observation?.active || this.observation.state === "stopping") return;
            await this.perform("stop", "stop_unlock_session", { selection: { ...this.selection } });
        }
    }
}
</script>
