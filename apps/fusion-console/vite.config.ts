// SPDX-License-Identifier: Apache-2.0
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { env } from "node:process";
const backend = env.VITE_FUSION_BACKEND;
if (backend) {
  const url = new URL(backend);
  if (
    url.origin !== backend ||
    url.hostname !== "127.0.0.1" ||
    url.protocol !== "http:" ||
    !url.port ||
    url.port === "8317"
  )
    throw Error("Isolated backend required");
}
export default defineConfig({
  base: "/loadout/",
  plugins: [
    react(),
    {
      name: "isolated-preview-boundary",
      configureServer(server) {
        server.middlewares.use((req, res, next) => {
          const origin = `http://127.0.0.1:${server.config.server.port}`;
          if (
            req.headers.host !== new URL(origin).host ||
            (req.headers.origin && req.headers.origin !== origin) ||
            req.headers["sec-fetch-site"] === "cross-site"
          ) {
            res.writeHead(403);
            res.end("Preview origin refused");
            return;
          }
          if (req.url?.startsWith("/v1/") || req.url?.startsWith("/e/")) {
            res.writeHead(404);
            res.end();
            return;
          }
          next();
        });
      },
    },
  ],
  server: {
    hmr: false,
    host: "127.0.0.1",
    port: 56604,
    strictPort: true,
    cors: false,
    fs: {
      strict: true,
      allow: [
        ".",
        "../../node_modules",
        "../../development/upstreams/omniroute/.claude/worktrees/loadout-phase11-v52/src/lib/radar",
      ],
    },
    proxy: backend
      ? {
          "/api/": {
            target: backend,
            changeOrigin: true,
            configure(proxy) {
              proxy.on("proxyReq", (outgoing, incoming) => {
                if (incoming.headers.origin)
                  outgoing.setHeader("origin", backend);
                outgoing.removeHeader("referer");
              });
            },
          },
        }
      : undefined,
  },
  build: { outDir: "../../.build/fusion-console", emptyOutDir: true },
});
