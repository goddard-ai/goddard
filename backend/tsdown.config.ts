import { defineConfig } from 'tsdown';

export default defineConfig({
  entry: ['./bin/server.ts'],
  format: 'esm',
  target: 'node18',
  clean: true,
  outDir: 'dist',
});
