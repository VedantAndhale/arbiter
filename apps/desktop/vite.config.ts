import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
// @ts-expect-error type error without @types/node package
import process from "node:process";
const host = process.env.TAURI_DEV_HOST;

/**
 * Dev-only: lets a plain browser at http://localhost:1420 find the running
 * daemon the way the desktop app does, by reading its discovery file.
 * ARBITER_HOME selects a non-default daemon home.
 */
function daemonDiscovery() {
  return {
    name: "arbiter-daemon-discovery",
    configureServer(server: { middlewares: { use: (path: string, fn: (req: unknown, res: { setHeader: (k: string, v: string) => void; statusCode: number; end: (body: string) => void }) => void) => void } }) {
      server.middlewares.use("/__arbiter/connect", async (_req, res) => {
        const { readFile } = await import("node:fs/promises");
        const { homedir } = await import("node:os");
        const { join } = await import("node:path");
        const home = process.env.ARBITER_HOME || join(homedir(), ".arbiter");
        try {
          const d = JSON.parse(await readFile(join(home, "daemon.json"), "utf8"));
          // daemon.json outlives the daemon; only report one that answers.
          const alive = await fetch(`http://127.0.0.1:${d.port}/v1/health`, { signal: AbortSignal.timeout(1500) }).then(r => r.ok).catch(() => false);
          if (!alive) throw new Error("not running");
          res.setHeader("Content-Type", "application/json");
          res.setHeader("Cache-Control", "no-store");
          res.end(JSON.stringify({ base_url: `http://127.0.0.1:${d.port}`, token: d.token }));
        } catch {
          res.statusCode = 404;
          res.end("{}");
        }
      });
    },
  };
}

// https://vite.dev/config/
export default defineConfig(() => ({
  plugins: [react(), tailwindcss(), daemonDiscovery()],

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
}));
