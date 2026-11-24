// Front-end unit tests (`npm test`). Reuses vite.config.ts, so tests see the app's own setup;
// test files sit next to the code they test as `*.test.ts` and are never imported by the app,
// so they stay out of the bundle.
import { defineConfig, mergeConfig } from 'vitest/config';
import viteConfig from './vite.config';

export default mergeConfig(viteConfig, defineConfig({
  // Svelte's browser build, so a test can mount a component in jsdom (`mount` is not on the server build).
  resolve: { conditions: ['browser'] },
  test: {
    environment: 'jsdom',
    include: ['src/**/*.test.ts'], // relative to the vite root, ui/
  },
}));
