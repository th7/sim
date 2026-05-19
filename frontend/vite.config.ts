import { defineConfig } from 'vite';
import { resolve } from 'node:path';

// Dev: serve on :3000 (browser-facing); proxy /api and /socket to Phoenix on :4000.
// Prod build: emit into apps/game_web/priv/static so Phoenix can serve the bundle.
export default defineConfig({
  server: {
    port: 3000,
    strictPort: true,
    host: '0.0.0.0',
    proxy: {
      '/api': 'http://localhost:4000',
      '/socket': {
        target: 'ws://localhost:4000',
        ws: true,
      },
    },
  },
  build: {
    outDir: resolve(__dirname, '../apps/game_web/priv/static'),
    emptyOutDir: false,
    manifest: true,
  },
});
