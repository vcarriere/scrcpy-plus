<template>
    <dialog ref="version_dialog" class="modal" aria-labelledby="version-change-title" @cancel="cancel" @close="cancel">
        <div class="modal-box min-w-0">
            <h2 id="version-change-title" class="font-bold text-lg">{{ confirmation.downgrade ? `Downgrade ${name}?` : `Change ${name} version?` }}</h2>
            <p class="py-4 break-words">{{ confirmation.current }} → {{ confirmation.target }}</p>
            <p v-if="confirmation.downgrade">An older version may have compatibility or security problems.</p>
            <p v-else>The current or requested version could not be verified. Check the executable before continuing.</p>
            <p class="mt-2">Automatic installation of {{ name }} updates will be disabled after a successful version change. Automatic update checks will keep their current setting.</p>
            <div class="modal-action flex-wrap gap-2">
                <button class="btn" autofocus @click="answer(false)">{{ $t('cancel') }}</button>
                <button class="btn btn-primary text-white" @click="answer(true)">{{ confirmation.downgrade ? 'Downgrade' : 'Use this version' }}</button>
            </div>
        </div>
    </dialog>
</template>

<style scoped>
dialog:not([open]) {
    display: none;
}
</style>

<script>
export default {
    setup() {
        return { confirmation: useVersionConfirmation() };
    },
    computed: {
        name() { return this.confirmation.tool === 'adb' ? 'ADB' : 'scrcpy'; }
    },
    watch: {
        "confirmation.open"(open) {
            if (open && !this.$refs.version_dialog.open) this.$refs.version_dialog.showModal();
            else if (!open && this.$refs.version_dialog.open) this.$refs.version_dialog.close();
        }
    },
    beforeUnmount() { this.answer(false); },
    methods: {
        cancel() { if (this.confirmation.open) this.answer(false); },
        answer(confirmed) { answer_version_confirmation(confirmed); }
    }
}
</script>
