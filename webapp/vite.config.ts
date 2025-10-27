import { defineConfig } from 'vite';

export default defineConfig({
  base: '/pds-wasm/',
  server: {
    port: 5173,
    fs: {
      allow: ['..'],
    },
  },
});
