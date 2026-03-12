#!/usr/bin/env node
const { spawn } = require('child_process');
const path = require('path');

// Execute the main tsx script
const indexPath = path.resolve(__dirname, '../index.ts');
const args = process.argv.slice(2);

const child = spawn('npx', ['tsx', indexPath, ...args], {
  stdio: 'inherit',
  shell: true
});

child.on('close', (code) => {
  process.exit(code);
});
