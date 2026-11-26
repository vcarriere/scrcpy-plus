<template>
    <div class="p-8">
        <!--   Header   -->
        <div class="card bg-base-200 w-full mb-8">
            <div class="p-8 flex">
                <NuxtLink to="/">
                    <button class="btn btn-primary w-12">
                        <icon name="reply" size="45x45" />
                    </button>
                </NuxtLink>
                <p class="text-3xl ml-4 mt-1">{{ $t('settings') }}</p>
            </div>
        </div>

        <!--   Content   -->
        <div class="card bg-base-200 w-full mb-8">
            <div class="card-body">
                
                <p>{{ $t('language') }}</p>
                <select class="select w-full" @change="set_language($event); setLocale($event.target.value)">
                    <option value="en" :selected="locale == 'en'">English</option>
                    <option value="it" :selected="locale == 'it'">Italiano</option>
                    <option value="ja" :selected="locale == 'ja'">日本語</option>
                </select>
                <p class="opacity-50">
                    <span class="mr-2">{{ $t('translation_credits') }}</span>
                    <span>{{ $t('translation_credits_value') }}</span>
                </p>

                <div v-for="item in settings" :key="item.key">
                    <update_check v-if="item.key === 'setting.disable_update_check'" manual>
                        <label class="flex flex-1 min-w-48 items-center cursor-pointer">
                            <input type="checkbox" class="toggle checkbox-primary mr-2 shrink-0" @change="updateSetting(item.key, $event)" :checked="item.checked" />
                            <p>{{ $t(item.title) }}</p>
                        </label>
                    </update_check>
                    <label v-else class="mt-2 flex cursor-pointer">
                        <input type="checkbox" class="toggle checkbox-primary mr-2" @change="updateSetting(item.key, $event)" :checked="item.checked" />
                        <p>{{ $t(item.title) }}</p>
                    </label>
                </div>
            </div>
        </div>

        <div v-if="advanced_controls">
            <adb_controls />

            <div class="card bg-base-200 w-full mb-8">
                <div class="card-body min-w-0">
                    <p class="card-title text-white">{{ $t('tool_auto_title') }}</p>
                    <p>{{ $t('tool_auto_description') }}</p>
                    <label v-for="tool in ['adb', 'scrcpy']" :key="tool" class="flex cursor-pointer items-center">
                        <input type="checkbox" class="toggle checkbox-primary mr-2 shrink-0" :checked="auto_install[tool]" :disabled="binary_busy" @change="update_tool_auto(tool, $event)" />
                        <span>{{ $t(`tool_auto_${tool}`) }}</span>
                    </label>
                    <p>{{ $t('tool_auto_sdk') }} <a href="https://developer.android.com/studio/terms" class="link link-primary" @click.prevent="open_sdk_terms">Android SDK License Agreement</a>.</p>
                    <p>{{ $t('tool_auto_compatibility') }}</p>
                    <p v-if="tool_update_error" class="text-error break-words" role="alert">{{ tool_update_error }}</p>
                </div>
            </div>

            <div class="card bg-base-200 w-full mb-8">
                <div class="card-body min-w-0">
                    <div v-for="bin in binaries" :key="bin.key" class="mb-4">
                        <label :for="bin.key">{{ $t(bin.title) }}</label>
                        <div class="flex flex-wrap gap-2">
                            <input :id="bin.key" type="text" class="input flex-1 basis-56 min-w-0" readonly :value="binary_status[bin.key]?.path || paths[bin.key]" :placeholder="$t('loading')" :aria-describedby="`${bin.key}_status`" />
                            <button class="btn btn-primary text-white" :disabled="binary_busy || !!installation.busy" @click="pick_binary(bin)">{{ $t('browse') }}</button>
                            <button v-if="paths[bin.key]" class="btn text-white" :disabled="binary_busy || !!installation.busy" @click="clear_binary(bin)">{{ $t('reset') }}</button>
                        </div>
                        <p :id="`${bin.key}_status`" class="mt-2" :class="binary_status[bin.key]?.available === false ? 'text-error' : ''">{{ binary_status[bin.key] ? $t(binary_message(binary_status[bin.key])) : $t('loading') }}</p>
                        <p v-if="binary_status[bin.key]?.invalid" class="text-error break-all" role="alert">{{ $t('binary_invalid_override') }} {{ binary_status[bin.key].requested }}</p>
                    </div>
                    <p v-if="binary_error" class="text-error break-words" role="alert">{{ binary_error }}</p>
                    <NuxtLink to="/scrcpy_manager">
                        <button class="btn btn-primary text-white">{{ $t('manage_versions') }}</button>
                    </NuxtLink>
                </div>
            </div>
        </div>

    </div>
</template>

<script setup>
const { setLocale } = useI18n()
</script>

<script>
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
export default {
    data() {
        return {
            installation: useToolInstallation(),
            settings: [
                { key: "setting.disable_update_check", title: "check_updates", inverted: true, checked: localStorage.getItem("setting.disable_update_check") !== "true" },
                { key: "setting.save_scrcpy_config",   title: "rscrcpy_config", checked: localStorage.getItem("setting.save_scrcpy_config") !== "false" },
                { key: "setting.advanced_adb", title: "advanced_setting", checked: localStorage.getItem("setting.advanced_adb") === "true" }
            ],
            is_windows: false,
            binary_status: {},
            binary_error: "",
            binary_busy: false,
            binary_request: 0,
            binary_disposed: false,
            auto_install: { adb: tool_auto_enabled('adb'), scrcpy: tool_auto_enabled('scrcpy') },
            tool_update_error: "",
            advanced_controls: localStorage.getItem("setting.advanced_adb") === "true",
            binaries: [
                { key: "scrcpy_path", title: "scrcpy_executable" },
                { key: "adb_path",    title: "adb_executable" }
            ],
            paths: {
                scrcpy_path: localStorage.getItem("scrcpyplus.scrcpy_path") || "",
                adb_path:    localStorage.getItem("scrcpyplus.adb_path") || ""
            },
            locale: localStorage.getItem("scrcpyplus.locale") || 'en',
        }
    },
    methods: {
        open_sdk_terms() { openUrl('https://developer.android.com/studio/terms'); },
        update_tool_auto(tool, event) {
            const previous = this.auto_install[tool];
            const previous_license = localStorage.getItem('setting.adb_sdk_license');
            this.tool_update_error = '';
            try {
                if (tool === 'adb' && event.target.checked) localStorage.setItem('setting.adb_sdk_license', 'true');
                set_tool_auto(tool, event.target.checked);
                this.auto_install[tool] = event.target.checked;
            } catch {
                try {
                    if (previous_license === null) localStorage.removeItem('setting.adb_sdk_license');
                    else localStorage.setItem('setting.adb_sdk_license', previous_license);
                } catch {}
                event.target.checked = previous;
                this.tool_update_error = 'Could not save the update preference.';
            }
        },
        updateSetting(key, event) {
            const setting = this.settings.find(setting => setting.key === key);
            setting.checked = event.target.checked;
            const value = setting.inverted ? !setting.checked : setting.checked;
            localStorage.setItem(key, value);
            if (key === "setting.advanced_adb") this.advanced_controls = event.target.checked;
            if (key === "setting.disable_update_check") window.dispatchEvent(new Event('scrcpyplus:update-policy'));
            console.log(key, value);
        },
        async change_binary(bin, selected) {
            if (selected === this.paths[bin.key]) return;
            const tool = bin.key === 'adb_path' ? 'adb' : 'scrcpy';
            const versions = await invoke('get_dependency_versions');
            const version = await invoke('inspect_tool_version', { tool, path: selected || null });
            if (this.binary_disposed || !await confirm_tool_version(tool, versions[tool] || null, version)) return;
            if (this.binary_disposed) return;
            const previous = { ...this.paths };
            const next = { ...previous, [bin.key]: selected };
            const auto_key = `setting.auto_update_${tool}`;
            const previous_auto = localStorage.getItem(auto_key);
            await invoke('activate_tool_paths', { scrcpyPath: next.scrcpy_path || null, adbPath: next.adb_path || null });
            try {
                if (selected) localStorage.setItem(`scrcpyplus.${bin.key}`, selected);
                else localStorage.removeItem(`scrcpyplus.${bin.key}`);
                if (compare_tool_versions(version, versions[tool]) !== 0) set_tool_auto(tool, false);
                this.auto_install[tool] = tool_auto_enabled(tool);
                this.paths = next;
            } catch (error) {
                try {
                    if (previous[bin.key]) localStorage.setItem(`scrcpyplus.${bin.key}`, previous[bin.key]);
                    else localStorage.removeItem(`scrcpyplus.${bin.key}`);
                    if (previous_auto === null) localStorage.removeItem(auto_key);
                    else localStorage.setItem(auto_key, previous_auto);
                    await invoke('activate_tool_paths', { scrcpyPath: previous.scrcpy_path || null, adbPath: previous.adb_path || null });
                } catch { throw new Error('Could not restore the previous executable settings. Check Advanced settings before starting SCRCPY.'); }
                throw error;
            }
            await this.refresh_binary_paths();
        },
        binary_message(binary) {
            if (!binary.available) return binary.source === 'custom' ? 'binary_custom_unavailable' : 'binary_default_unavailable';
            return binary.source === 'custom' ? 'binary_custom' : binary.source === 'system' ? 'binary_system_default' : 'bundled_default';
        },
        async refresh_binary_paths() {
            if (this.binary_disposed) return;
            const request = ++this.binary_request;
            this.binary_error = "";
            try {
                const status = await invoke("get_binary_paths");
                if (request === this.binary_request && !this.binary_disposed) this.binary_status = status;
            } catch (e) {
                if (request !== this.binary_request || this.binary_disposed) return;
                this.binary_status = {};
                this.binary_error = String(e);
            }
        },
        async pick_binary(bin) {
            if (this.binary_busy || this.installation.busy) return;
            this.binary_busy = true;
            this.installation.busy = 'settings';
            this.installation.stage = 'confirming';
            this.binary_error = "";
            try {
                const selected = await open({
                    multiple: false,
                    directory: false,
                    filters: this.is_windows ? [{ name: "Executable", extensions: ["exe"] }] : []
                });
                if (typeof selected !== "string" || this.binary_disposed) return;
                await this.change_binary(bin, selected);
            } catch (e) {
                this.binary_error = String(e);
            } finally {
                this.binary_busy = false;
                this.installation.busy = '';
                this.installation.stage = '';
            }
        },
        async clear_binary(bin) {
            if (this.binary_busy || this.installation.busy) return;
            this.binary_busy = true;
            this.installation.busy = 'settings';
            this.installation.stage = 'confirming';
            this.binary_error = "";
            try {
                await this.change_binary(bin, '');
            } catch (e) {
                this.binary_error = String(e);
            } finally {
                this.binary_busy = false;
                this.installation.busy = '';
                this.installation.stage = '';
            }
        },
        set_language(event) {
            const new_locale = event.target.value;
            localStorage.setItem("scrcpyplus.locale", new_locale);
            console.log("scrcpyplus.locale", new_locale);
        }
    },
    async mounted() {
        this.is_windows = (await invoke("environment")).is_windows === true;
        await this.refresh_binary_paths();
        const vm = this;
        for (let key of Object.keys(localStorage)) { 
            if(!key.startsWith("setting.")) continue;
            for (const i in this.settings) {
                if (key == this.settings[i].key) {
                    const checked = localStorage.getItem(key) == 'true';
                    vm.settings[i].checked = vm.settings[i].inverted ? !checked : checked;
                }
            }
        };
    },
    beforeUnmount() {
        this.binary_disposed = true;
        this.binary_request++;
    },
}
</script>
