import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { genPluginFrame } from './scripts/gen-plugin-frame.mjs';

genPluginFrame(); // keep ui/public/plugin-frame.html in step with plugin-frame.js

export default defineConfig({
  root: 'ui',
  plugins: [svelte()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: { outDir: '../dist', emptyOutDir: true, target: 'safari15' },
});
