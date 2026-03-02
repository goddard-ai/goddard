#!/usr/bin/env node

import { program } from 'commander';
import * as fs from 'fs';
import * as path from 'path';
import * as os from 'os';
import { createJiti } from 'jiti';
import { createLoop } from './index';

const jiti = createJiti(process.cwd());

program
  .name('pi-loop')
  .description('Endless rate-limited loop for pi-coding-agent')
  .version('1.0.0');

program
  .command('init')
  .description('Initialize pi-loop configuration')
  .option('-g, --global', 'Create config in home directory instead of current directory')
  .action((options) => {
    const targetDir = options.global ? os.homedir() : process.cwd();
    const configPath = path.join(targetDir, 'pi-loop.config.ts');

    if (fs.existsSync(configPath)) {
      console.error(`Config file already exists at ${configPath}`);
      process.exit(1);
    }

    const configContent = `import { createLoopConfig } from 'pi-loop';
import { DefaultStrategy } from 'pi-loop/strategies';

export default createLoopConfig({
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
`;
    fs.writeFileSync(configPath, configContent);
    console.log(`Created configuration at ${configPath}`);
    console.log('You can now run it using: pi-loop run');
  });

program
  .command('run')
  .description('Start the pi-loop daemon')
  .action(async () => {
    const localConfigPath = path.join(process.cwd(), 'pi-loop.config.ts');
    const globalConfigPath = path.join(os.homedir(), 'pi-loop.config.ts');

    let configPathToLoad: string | null = null;

    if (fs.existsSync(localConfigPath)) {
      configPathToLoad = localConfigPath;
      console.log(`Found local config at ${localConfigPath}`);
    } else if (fs.existsSync(globalConfigPath)) {
      configPathToLoad = globalConfigPath;
      console.log(`Found global config at ${globalConfigPath}`);
    } else {
      console.error('Could not find pi-loop.config.ts in the current directory or home directory.');
      console.error('Run `pi-loop init` to create one.');
      process.exit(1);
    }

    try {
      // Load config using jiti
      const configModule = await jiti.import(configPathToLoad);
      const config = (configModule as any).default || configModule;

      if (!config) {
        throw new Error('Config file must export a default configuration object.');
      }

      console.log('Starting pi-loop daemon...');
      const loop = createLoop(config);
      await loop.start();
    } catch (error) {
      console.error('Failed to run pi-loop:', error);
      process.exit(1);
    }
  });

program
  .command('generate-systemd')
  .description('Generate a systemd service file from the config')
  .option('-g, --global', 'Use global config in home directory')
  .action(async (options) => {
    const targetDir = options.global ? os.homedir() : process.cwd();
    const configPath = path.join(targetDir, 'pi-loop.config.ts');

    if (!fs.existsSync(configPath)) {
      console.error(`Could not find config at ${configPath}`);
      process.exit(1);
    }

    try {
      const configModule = await jiti.import(configPath);
      const config = (configModule as any).default || configModule;

      const user = os.userInfo().username;
      const workingDir = process.cwd();
      const restartSec = config.systemd?.restartSec || 10;
      const nice = config.systemd?.nice || 10;

      // Try to resolve the global path to pi-loop
      let execStart = 'pi-loop run';

      const serviceContent = `[Unit]
Description=pi-loop Daemon
After=network.target

[Service]
Type=simple
User=${user}
WorkingDirectory=${workingDir}
ExecStart=${execStart}
Restart=always
RestartSec=${restartSec}
Nice=${nice}

[Install]
WantedBy=multi-user.target
`;
      const systemdDir = path.join(targetDir, 'systemd');
      if (!fs.existsSync(systemdDir)) {
        fs.mkdirSync(systemdDir, { recursive: true });
      }
      const outPath = path.join(systemdDir, 'pi-loop.service');
      fs.writeFileSync(outPath, serviceContent);
      console.log(`Created systemd service file at ${outPath}`);
    } catch (error) {
      console.error('Failed to generate systemd file:', error);
      process.exit(1);
    }
  });

program.parse();
