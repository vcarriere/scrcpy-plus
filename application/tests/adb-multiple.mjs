import { test } from "node:test";
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createConnection, createServer } from "node:net";
import { mkdtemp, mkdir, rm } from "node:fs/promises";
import { resolve, dirname } from "node:path";
import { setTimeout as delay } from "node:timers/promises";

const adb = process.env.SCRCPY_PLUS_TEST_ADB;

async function free_port() {
    const server = createServer();
    await new Promise((resolve, reject) => server.listen(0, "127.0.0.1", resolve).once("error", reject));
    const port = server.address().port;
    await new Promise(resolve => server.close(resolve));
    return port;
}

function query(port, request = "host:version") {
    return new Promise((resolve, reject) => {
        const socket = createConnection({ host: "127.0.0.1", port });
        let bytes = Buffer.alloc(0);
        const finish = (error, result) => { socket.destroy(); error ? reject(error) : resolve(result); };
        socket.setTimeout(1000, () => finish(new Error("ADB query timed out")));
        socket.once("error", error => finish(error.code === "ECONNREFUSED" ? null : error, null));
        socket.once("connect", () => socket.write(request.length.toString(16).padStart(4, "0") + request));
        socket.on("data", chunk => {
            bytes = Buffer.concat([bytes, chunk]);
            if (bytes.length < 4) return;
            if (bytes.subarray(0, 4).toString() !== "OKAY") return finish(new Error("ADB rejected test query"));
            if (request === "host:kill") return finish(null, "OKAY");
            if (bytes.length < 8) return;
            const length = parseInt(bytes.subarray(4, 8).toString(), 16);
            if (bytes.length >= length + 8) finish(null, bytes.subarray(8, length + 8).toString());
        });
    });
}

test("isolated ADB clients and servers can coexist without sharing a listening port", { skip: !adb && "Set SCRCPY_PLUS_TEST_ADB to an existing adb executable", timeout: 30000 }, async () => {
    const root = resolve("../.tools");
    await mkdir(root, { recursive: true });
    const temporary = await mkdtemp(resolve(root, "adb-multiple-"));
    const children = [];
    const baseline = await query(5037);
    const environment = {
        ...process.env,
        ANDROID_USER_HOME: temporary, ANDROID_SDK_HOME: temporary, ADB_VENDOR_KEYS: temporary,
        ADB_MDNS: "0", ADB_MDNS_AUTO_CONNECT: "0", ADB_LOCAL_TRANSPORT_MAX_PORT: "0",
    };
    delete environment.ADB_SERVER_SOCKET;
    delete environment.ANDROID_ADB_SERVER_ADDRESS;
    delete environment.ANDROID_ADB_SERVER_PORT;
    function launch(args) {
        const child = spawn(adb, args, { env: environment, windowsHide: true, stdio: ["ignore", "pipe", "pipe"] });
        children.push(child);
        child.output = "";
        child.error_output = "";
        child.stdout.on("data", chunk => { child.output = (child.output + chunk).slice(-4096); });
        child.stderr.on("data", chunk => { child.error_output = (child.error_output + chunk).slice(-4096); });
        child.completed = new Promise((resolve, reject) => {
            child.once("error", reject);
            child.once("close", code => resolve(code));
        });
        return child;
    }
    try {
        const first = await free_port();
        let second = await free_port();
        while (second === first) second = await free_port();
        const first_server = launch(["-L", `tcp:${first}`, "--one-device", "scrcpy-plus-no-such-test-device", "server", "nodaemon"]);
        const second_server = launch(["-L", `tcp:${second}`, "--one-device", "scrcpy-plus-no-such-test-device", "server", "nodaemon"]);
        for (const port of [first, second]) {
            let ready = false;
            for (let attempt = 0; attempt < 50; attempt++) {
                if (await query(port)) { ready = true; break; }
                await delay(100);
            }
            const server = port === first ? first_server : second_server;
            assert.ok(ready, "The isolated server should start: " + server.error_output);
        }
        assert.notEqual(first_server.pid, second_server.pid);
        assert.equal(await query(first), await query(second));

        const clients = Array.from({ length: 4 }, () => launch(["-H", "127.0.0.1", "-P", String(first), "devices"]));
        assert.deepEqual(await Promise.all(clients.map(client => client.completed)), [0, 0, 0, 0]);
        for (const client of clients) assert.equal(client.output.trim(), "List of devices attached");

        const repeated_start = launch(["-H", "127.0.0.1", "-P", String(first), "start-server"]);
        assert.equal(await repeated_start.completed, 0);
        assert.equal(first_server.exitCode, null, "Starting an existing server should reuse it");

        const duplicate = launch(["-L", `tcp:${first}`, "--one-device", "scrcpy-plus-no-such-test-device", "server", "nodaemon"]);
        assert.notEqual(await duplicate.completed, 0, "A second server cannot bind the same endpoint");
        assert.ok(await query(first));

        await query(first, "host:kill");
        await first_server.completed;
        assert.equal(await query(first), null);
        assert.ok(await query(second), "Stopping the first endpoint must leave the second running");
        assert.equal(await query(5037), baseline, "The shared default endpoint should remain unchanged");
        await query(second, "host:kill");
        await second_server.completed;
        assert.equal(await query(second), null);
    } finally {
        for (const child of children) {
            if (child.exitCode === null && child.signalCode === null) child.kill();
        }
        await Promise.allSettled(children.map(child => child.completed));
        if (dirname(temporary) !== root) throw new Error("Refusing cleanup outside the isolated test directory");
        await rm(temporary, { recursive: true, force: true });
    }
});
