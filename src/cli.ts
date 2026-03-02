#!/usr/bin/env node

import { program } from 'commander';
import * as fs from 'fs';
import * as path from 'path';

program
  .name('pi-loop')
  .description('CLI to generate pi-loop configuration')
  .version('1.0.0');

program
  .command('init')
  .description('Initialize pi-loop configuration in the current directory')
  .argument('[projectDir]', 'Directory to initialize (default: current)')
  .action((projectDir) => {
    const targetDir = projectDir ? path.resolve(projectDir) : process.cwd();

    // Ensure directory exists
    if (!fs.existsSync(targetDir)) {
      fs.mkdirSync(targetDir, { recursive: true });
    }

    const configPath = path.join(targetDir, 'pi-loop.config.ts');
    const tsconfigPath = path.join(targetDir, 'tsconfig.json');
    const systemdDir = path.join(targetDir, 'systemd');

    // Generate config.ts
    const configContent = `import { createLoop, createLoopConfig } from 'pi-loop';
import { DefaultStrategy } from 'pi-loop/strategies';

const config = createLoopConfig({
  agent: {
    model: 'claude-sonnet-4',
    projectDir: './',
    maxTokensPerCycle: 8000,
  },
  strategy: new DefaultStrategy(),
  rateLimits: {
    cycleDelay: '30m',
    maxTokensPerCycle: 8000,
    maxOpsPerMinute: 20,
    maxCyclesBeforePause: 24,
  },
  metrics: {
    prometheusPort: 9090,
    enableLogging: true,
  },
  systemd: {
    restartSec: 10,
    nice: 10,
  }
});

const loop = createLoop(config);
loop.start().catch(console.error);

export default config;
`;
    fs.writeFileSync(configPath, configContent);
    console.log(`Created ${configPath}`);

    // Generate tsconfig.json
    if (!fs.existsSync(tsconfigPath)) {
      const tsconfigContent = `{
  "compilerOptions": {
    "target": "ESNext",
    "module": "CommonJS",
    "strict": true,
    "esModuleInterop": true,
    "skipLibCheck": true,
    "forceConsistentCasingInFileNames": true
  }
}
`;
      fs.writeFileSync(tsconfigPath, tsconfigContent);
      console.log(`Created ${tsconfigPath}`);
    }

    // Generate systemd directory
    if (!fs.existsSync(systemdDir)) {
      fs.mkdirSync(systemdDir, { recursive: true });
      console.log(`Created ${systemdDir}`);
    }

    // Optional: write a README or next steps
    console.log('\nSuccess! To start your pi-loop:');
    console.log('1. Run `pnpm install pi-loop typescript @types/node -D` (if not installed)');
    console.log('2. Run `npx tsx pi-loop.config.ts` or compile it.');
  });

program
  .command('generate-systemd')
  .description('Generate a systemd service file from the config')
  .action(() => {
    console.log('Generating systemd service file... (mock)');
    const serviceContent = `[Unit]
Description=pi-loop Daemon
After=network.target

[Service]
Type=simple
User=nodeuser
WorkingDirectory=/opt/my-project
ExecStart=/usr/bin/npx tsx pi-loop.config.ts
Restart=always
RestartSec=10
Nice=10

[Install]
WantedBy=multi-user.target
`;
    const targetPath = path.join(process.cwd(), 'systemd', 'pi-loop.service');
    if (!fs.existsSync(path.dirname(targetPath))) {
      fs.mkdirSync(path.dirname(targetPath), { recursive: true });
    }
    fs.writeFileSync(targetPath, serviceContent);
    console.log(`Created systemd service file at ${targetPath}`);
  });

program.parse();
