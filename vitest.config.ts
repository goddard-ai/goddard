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
      '@goddard-ai/config': new URL('./core/config/src/index.ts', import.meta.url).pathname,
      '@goddard-ai/daemon-client': new URL('./daemon/client/src/index.ts', import.meta.url).pathname,
      '@goddard-ai/ipc': new URL('./core/ipc/src/index.ts', import.meta.url).pathname,
      '@goddard-ai/ipc/client': new URL('./core/ipc/src/client.ts', import.meta.url).pathname,
      '@goddard-ai/ipc/schema': new URL('./core/ipc/src/schema.ts', import.meta.url).pathname,
      '@goddard-ai/ipc/transport': new URL('./core/ipc/src/transport.ts', import.meta.url)
        .pathname,
      '@goddard-ai/schema/daemon-ipc': new URL('./core/schema/src/daemon-ipc.ts', import.meta.url)
        .pathname,
      '@goddard-ai/sdk': new URL('./core/sdk/src/index.ts', import.meta.url).pathname,
      '@goddard-ai/sdk/daemon': new URL('./core/sdk/src/daemon/index.ts', import.meta.url).pathname,
      '@goddard-ai/sdk/loop': new URL('./core/sdk/src/loop/index.ts', import.meta.url).pathname,
      '@goddard-ai/tauri-plugin-ipc': new URL('./core/tauri-plugin-ipc/src/index.ts', import.meta.url)
        .pathname,
    },
  },
});
