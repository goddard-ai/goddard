import { defineConfig } from 'tsdown';

export default defineConfig({
  entry: ['./src/index.ts', './src/goddard-daemon.ts'],
  format: 'esm',
  target: 'node18',
  clean: true,
  outDir: 'dist',
  dts: true,
});
