<template>
    <div class="p-8">
        <update_check />
        <adb_check />
        <information />
        <devices @device="set_active_device" />
        <scrcpy_config v-if="device_config_initialized" v-show="active_device" :device="active_device" :selection="active_selection" />
    </div>
</template>

<script>
export default {
    data() {
        return {
            active_device: false,
            active_selection: null,
            device_config_initialized: false
        }
    },
    methods: {
        set_active_device(serial, selection) {
            this.active_device = serial;
            this.active_selection = selection;
            if (serial) this.device_config_initialized = true;
        }
    },
}
</script>
