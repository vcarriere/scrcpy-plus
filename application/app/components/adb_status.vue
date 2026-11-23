<template>
    <span class="inline-flex items-center gap-2" role="status" aria-live="polite">
        <span class="w-2 h-2 rounded-full shrink-0" :class="color" aria-hidden="true"></span>
        <span>{{ $t(paused_running ? 'adb_state_running_paused' : `adb_state_${status.state}`) }}</span>
    </span>
</template>

<script>
export default {
    setup() {
        const { status, paused_running } = useAdbStatus();
        return { status, paused_running };
    },
    computed: {
        color() {
            if (this.status.state === "running") return "bg-success";
            if (this.status.state === "error") return "bg-error";
            if (["starting", "stopping", "checking"].includes(this.status.state)) return "bg-warning";
            return "bg-base-content opacity-40";
        }
    }
}
</script>
