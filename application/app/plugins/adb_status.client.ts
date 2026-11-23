export default defineNuxtPlugin({
  dependsOn: ["binary-paths"],
  async setup() {
    const { refresh } = useAdbStatus();
    await refresh();
    const interval = setInterval(refresh, 2000);
    if (import.meta.hot) import.meta.hot.dispose(() => clearInterval(interval));
  },
});
