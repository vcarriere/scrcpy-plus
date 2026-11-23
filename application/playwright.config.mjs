import { defineConfig } from "@playwright/test";

export default defineConfig({
    testDir: "./tests/ui",
    timeout: 30000,
    workers: 2,
    use: { baseURL: "http://127.0.0.1:4179", viewport: { width: 600, height: 800 }, headless: true },
    webServer: { command: "node tests/serve.mjs", url: "http://127.0.0.1:4179", reuseExistingServer: false },
});
