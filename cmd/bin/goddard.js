#!/usr/bin/env node
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const gitDir = path.join(__dirname, '..', '.git');
const hasGit = fs.existsSync(gitDir);

if (hasGit) {
  // Use tsx in dev
  const args = ['--import', 'tsx', path.join(__dirname, '..', 'src', 'cli.ts'), ...process.argv.slice(2)];
  const result = spawnSync(process.execPath, args, { stdio: 'inherit' });
  process.exit(result.status || 0);
} else {
  // Use compiled cli.js in production
  import('../dist/cli.js').catch(err => {
    console.error(err);
    process.exit(1);
  });
}
