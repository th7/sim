import { defineConfig } from 'vite';
import { resolve } from 'node:path';

// Dev: serve on :3000 (browser-facing); proxy /socket to the Rust sim server on
// :4000. Prod build: emit into frontend/dist, which the Rust server serves
// directly (SIM_STATIC_DIR) — so a single binary serves the bundle + the socket.
export default defineConfig({
  server: {
    port: 3000,
    strictPort: true,
    host: '0.0.0.0',
    proxy: {
      '/socket': {
        target: 'ws://localhost:4000',
        ws: true,
      },
    },
  },
  build: {
    outDir: resolve(__dirname, 'dist'),
    emptyOutDir: true,
    manifest: true,
  },
});
