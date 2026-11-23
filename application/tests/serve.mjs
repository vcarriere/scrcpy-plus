import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { resolve, extname, sep } from "node:path";

const root = resolve(".output/public");
const types = { ".html": "text/html", ".js": "text/javascript", ".css": "text/css", ".json": "application/json", ".svg": "image/svg+xml", ".png": "image/png" };
createServer(async (request, response) => {
    const pathname = decodeURIComponent(new URL(request.url, "http://127.0.0.1").pathname);
    const path = resolve(root, "." + pathname);
    if (path !== root && !path.startsWith(root + sep)) { response.writeHead(403).end(); return; }
    try {
        const target = extname(path) ? path : resolve(path, "index.html");
        const content = await readFile(target).catch(() => readFile(resolve(root, "index.html")));
        response.setHeader("Content-Type", types[extname(target)] || "application/octet-stream");
        response.end(content);
    } catch { response.writeHead(404).end(); }
}).listen(4179, "127.0.0.1");
