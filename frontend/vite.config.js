/// <reference types="vitest" />
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],
  server: {
    port: 5173,
    // Proxy /v1 requests to the Rust API in dev so fetch('/v1/monitors')
    // works without CORS gymnastics.
    proxy: {
      '/v1':       'http://localhost:3000',
      '/healthz':  'http://localhost:3000',
      '/readyz':   'http://localhost:3000',
      '/metrics':  'http://localhost:3000',
    },
  },
  test: {
    // jsdom gives us `window` / `document` / `fetch` so component tests
    // and the hash-router tests can run without a real browser.
    environment: 'jsdom',
    globals:     true,
    setupFiles:  ['./src/test/setup.js'],
    include:     ['src/**/*.test.{js,jsx}'],
  },
});
