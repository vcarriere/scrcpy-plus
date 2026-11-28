<template>
    <div v-if="!current && active.length" class="fixed bottom-4 right-4 left-4 sm:left-auto z-40 flex flex-col items-end gap-2">
        <button v-for="launch in active" :key="launch.id" class="btn btn-primary text-white max-w-full h-auto min-h-10 py-2 whitespace-normal" @click="show_progress(launch.id)">
            <span v-if="spinning(launch)" class="startup-spinner inline-block h-4 w-4 shrink-0 animate-spin rounded-full border-2 border-current border-t-transparent motion-reduce:animate-none" aria-hidden="true"></span>
            {{ launch.phase === 'manual_unlock' ? launch.reason === 'screen_off_unavailable' ? 'Check phone screen' : 'Manual unlock needed' : 'SCRCPY startup' }}
        </button>
    </div>
    <dialog ref="launch_dialog" class="modal" aria-labelledby="scrcpy-launch-title" @cancel.prevent="hide_progress">
        <div class="modal-box min-w-0">
            <template v-if="current">
                <div class="flex items-center gap-4">
                    <span v-if="spinning(current)" class="startup-spinner inline-block h-8 w-8 shrink-0 animate-spin rounded-full border-[3px] border-current border-t-transparent text-primary motion-reduce:animate-none" aria-hidden="true"></span>
                    <h2 id="scrcpy-launch-title" class="font-bold text-xl text-white">{{ title }}</h2>
                </div>
                <div class="mt-4 space-y-3 break-words" role="status" aria-live="polite">
                    <p class="text-white">{{ step }}</p>
                    <p>{{ description }}</p>
                </div>
                <p v-if="current.error" class="text-error break-words mt-3" role="alert">{{ current.error }}</p>
                <p v-if="!current.finished" class="mt-4 opacity-50">You can keep using SCRCPY+ while this session continues.</p>
                <div class="modal-action">
                    <button class="btn btn-primary text-white min-h-10 h-auto py-2" @click="hide_progress">{{ current.finished ? 'Close' : 'Hide progress' }}</button>
                </div>
            </template>
        </div>
    </dialog>
</template>

<style scoped>
dialog:not([open]) { display: none; }
</style>

<script>
export default {
    setup() { return useScrcpyLaunch(); },
    computed: {
        title() {
            if (this.current?.phase === 'manual_unlock') return this.current.reason === 'screen_off_unavailable' ? 'Check phone screen' : 'Manual unlock needed';
            return this.current?.phase === 'error' ? 'Could not start SCRCPY' : 'Starting SCRCPY';
        },
        step() {
            if (this.current?.reason === 'progress_unavailable') return 'Startup progress is unavailable.';
            if (this.current?.phase === 'manual_unlock' && this.current.reason === 'screen_off_unavailable') return 'Waiting for the phone screen-off state.';
            const reason = {
                checking_unlock_configuration: 'Checking automatic unlock…', preparing_screen_control: 'Preparing screen control…',
                preparing_private_entry: 'Preparing private password entry…', entering_password: 'Entering the password privately…',
                verifying_unlock: 'Verifying that the phone is unlocked…', restoring_screen_off: 'Restoring the screen-off setting…',
            }[this.current?.reason];
            if (reason && ['preparing', 'unlocking'].includes(this.current?.phase)) return reason;
            return {
                checking: 'Checking the device connection…', preparing: 'Preparing the phone…',
                unlocking: 'Unlocking the phone privately…', manual_unlock: 'Unlock the phone manually.',
                starting_mirror: 'Opening the mirrored display…', ready: 'The mirrored display is ready.',
                launched: 'SCRCPY has started; display readiness is unconfirmed.', closed: 'The session ended.',
                error: 'The session could not be started.',
            }[this.current?.phase] || 'Waiting for startup progress…';
        },
        description() {
            if (this.current?.reason === 'progress_unavailable') return 'Startup progress could not be checked. Check whether the mirrored window has appeared.';
            if (this.current?.phase === 'manual_unlock') return {
                automatic_unlock_blocked: 'Automatic unlock is paused after an unsuccessful or uncertain attempt. Unlock the phone manually once to recover.',
                password_editor_unavailable: 'The password entry screen is unavailable. Complete the unlock on the phone, then wait for the session to continue.',
                device_state_unavailable: 'The phone state could not be verified. Check its connection and unlock it manually.',
                unsupported_unlock_state: 'Complete the unlock or fingerprint prompt on the phone, then wait for the session to continue.',
                screen_off_unavailable: 'Unlock was verified, but the phone screen-off state could not be confirmed. Check the phone display and connection; the session will continue when that state is verified.',
            }[this.current.reason] || 'Complete the unlock on the phone, then wait for the session to continue.';
            return {
                checking: 'Verifying the selected phone and connection.',
                preparing: 'Preparing screen control and checking whether private unlock is needed.',
                unlocking: 'The mirrored display stays closed during password entry and verification.',
                manual_unlock: 'Complete the unlock on the phone. Follow any prompt shown there, then wait for the session to continue.',
                starting_mirror: 'Connecting to the phone and waiting for its video display.',
                ready: 'You can use the mirrored phone window.',
                launched: 'This executable has not reported a video display yet. Check the phone and the mirrored window before trying again.',
                closed: 'You can start a new session from the device controls.',
                error: 'Check the message below, then try again from the device controls.',
            }[this.current?.phase] || '';
        }
    },
    watch: { dialog_id() { this.sync_dialog(); } },
    mounted() { this.sync_dialog(); },
    methods: {
        spinning(launch) { return launch?.reason !== 'progress_unavailable' && ['checking', 'preparing', 'unlocking', 'starting_mirror'].includes(launch?.phase); },
        sync_dialog() {
            this.$nextTick(() => {
                const dialog = this.$refs.launch_dialog;
                if (!dialog) return;
                if (this.current && !dialog.open) dialog.showModal();
                else if (!this.current && dialog.open) dialog.close();
            });
        }
    }
}
</script>
