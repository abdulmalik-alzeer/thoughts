import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { readFileSync } from 'node:fs';
import { genPluginFrame } from './scripts/gen-plugin-frame.mjs';

// The release this window was built from (package.json, kept in step by scripts/set-version.mjs).
const version = JSON.parse(readFileSync(new URL('./package.json', import.meta.url), 'utf8')).version;

genPluginFrame(); // keep ui/public/plugin-frame.html in step with plugin-frame.js

export default defineConfig({
  root: 'ui',
  plugins: [svelte()],
  define: { __APP_VERSION__: JSON.stringify(version) },
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: { outDir: '../dist', emptyOutDir: true, target: 'safari15' },
});
