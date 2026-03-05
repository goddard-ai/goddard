import { defineConfig } from 'tsdown';

export default defineConfig({
  entry: ['./src/cli.ts', './src/index.ts'],
  format: 'esm',
  target: 'node18',
  clean: true,
  outDir: 'dist',
  outExtension: () => ({ js: '.js' }),
  treeshake: true,
});
