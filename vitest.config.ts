import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    environment: 'node',
    include: ['**/*.test.ts'],
    globals: true,
  },
  resolve: {
    conditions: ['default'],
    alias: {
      '@goddard-ai/session': new URL('./core/session/src/index.ts', import.meta.url).pathname,
      '@goddard-ai/config': new URL('./core/config/src/index.ts', import.meta.url).pathname,
    },
  },
});
