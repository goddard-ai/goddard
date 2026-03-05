import { defineConfig } from 'tsdown';
import fs from 'fs';
import path from 'path';

export default defineConfig({
  entry: ['./src/index.ts'],
  format: 'esm',
  target: 'node18',
  clean: true,
  outDir: 'dist',
  dts: true,
  onSuccess: async () => {
    const promptsDir = path.resolve('src/prompts');
    const distPromptsDir = path.resolve('dist/prompts');
    if (fs.existsSync(promptsDir)) {
      if (!fs.existsSync(distPromptsDir)) {
        fs.mkdirSync(distPromptsDir, { recursive: true });
      }
      const files = fs.readdirSync(promptsDir);
      for (const file of files) {
        fs.copyFileSync(path.join(promptsDir, file), path.join(distPromptsDir, file));
      }
    }
  }
});
