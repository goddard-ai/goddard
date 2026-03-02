#!/usr/bin/env node

import { program } from 'commander';
import * as fs from 'fs';
import * as path from 'path';
import * as os from 'os';
import { execSync } from 'child_process';
import { createJiti } from 'jiti';
import { log } from '@clack/prompts';
import { createLoop } from './index';

const jiti = createJiti(process.cwd());

function quoteSystemdValue(value: string): string {
  return `"${value.replace(/\\/g, '\\\\').replace(/"/g, '\\"')}"`;
}

function renderSystemdEnvironment(environment?: Record<string, string | undefined>): string {
  if (!environment) {
    return '';
  }

  const lines = Object.entries(environment)
    .filter(([, value]) => value !== undefined)
    .map(([key, value]) => `Environment=${key}=${quoteSystemdValue(value as string)}`);

  return lines.length > 0 ? `${lines.join('\n')}\n` : '';
}

program
  .name('.pi-loop')
  .description('Endless rate-limited loop for pi-coding-agent')
  .version('1.0.0');

program
  .command('init')
  .description('Initialize pi-loop configuration')
  .option('-g, --global', 'Create config in home directory instead of current directory')
  .action((options) => {
    const targetDir = options.global ? path.join(os.homedir(), '.pi-loop') : process.cwd();
    const configFileName = options.global ? 'config.ts' : 'pi-loop.config.ts';
    const configPath = path.join(targetDir, configFileName);

    if (options.global) {
      if (!fs.existsSync(targetDir)) {
        fs.mkdirSync(targetDir, { recursive: true });
      }

      const packageJsonPath = path.join(targetDir, 'package.json');
      if (!fs.existsSync(packageJsonPath)) {
        const binPath = fs.realpathSync(process.argv[1]);
        const packageRoot = path.resolve(path.dirname(binPath), '..');
        
        const pkg = {
          name: "pi-loop-global-config",
          version: "1.0.0",
          private: true,
          dependencies: {
            "pi-loop": `link:${packageRoot}`
          }
        };
        fs.writeFileSync(packageJsonPath, JSON.stringify(pkg, null, 2));
        log.info(`Created package.json with link to global pi-loop at ${packageRoot}`);
        
        let installCmd = 'npm install';
        try {
          execSync('pnpm --version', { stdio: 'ignore' });
          installCmd = 'pnpm install';
        } catch {
          try {
            execSync('bun --version', { stdio: 'ignore' });
            installCmd = 'bun install';
          } catch {
            try {
              execSync('yarn --version', { stdio: 'ignore' });
              installCmd = 'yarn';
            } catch {}
          }
        }

        try {
          execSync(installCmd, { cwd: targetDir, stdio: 'inherit' });
        } catch (e) {
          log.error(`Failed to run ${installCmd} in ` + targetDir);
        }
      }
    }

    if (fs.existsSync(configPath)) {
      log.error(`Config file already exists at ${configPath}`);
      process.exit(1);
    }

    const configContent = `import { createLoopConfig } from 'pi-loop';
import { DefaultStrategy } from 'pi-loop/strategies';

export default createLoopConfig({
  agent: {
    model: 'claude-sonnet-4',
    projectDir: './',
    thinkingLevel: 'low',
  },
  strategy: new DefaultStrategy(),
  rateLimits: {
    cycleDelay: '30m',
    maxTokensPerCycle: 128000,
    maxOpsPerMinute: 120,
    maxCyclesBeforePause: 100,
  },
  retries: {
    maxAttempts: 3,
    initialDelayMs: 1000,
    maxDelayMs: 30000,
    backoffFactor: 2,
    jitterRatio: 0.2,
    retryableErrors: (error) => {
      return true;
    },
  },
});
`;
    fs.writeFileSync(configPath, configContent);
    log.success(`Created configuration at ${configPath}`);
    log.info('You can now run it using: pi-loop run');
  });

program
  .command('run')
  .description('Start the pi-loop daemon')
  .option('-m, --model <model>', 'Override the agent model')
  .option('-d, --project-dir <dir>', 'Override the project directory')
  .option('--cycle-delay <delay>', 'Override the cycle delay (e.g. 30m, 1h)')
  .option('--max-tokens <tokens>', 'Override max tokens per cycle', parseInt)
  .option('--max-ops <ops>', 'Override max ops per minute', parseInt)
  .option('--thinking-level <level>', 'Override the agent thinking level')
  .action(async (options) => {
    const localConfigPath = path.join(process.cwd(), 'pi-loop.config.ts');
    const globalConfigPath = path.join(os.homedir(), '.pi-loop', 'config.ts');

    let configPathToLoad: string | null = null;

    if (fs.existsSync(localConfigPath)) {
      configPathToLoad = localConfigPath;
      log.info(`Found local config at ${localConfigPath}`);
    } else if (fs.existsSync(globalConfigPath)) {
      configPathToLoad = globalConfigPath;
      log.info(`Found global config at ${globalConfigPath}`);
    } else {
      log.error('Could not find pi-loop.config.ts in the current directory or config.ts in the home directory.');
      log.info('Run `pi-loop init` to create one.');
      process.exit(1);
    }

    try {
      // Load config using jiti
      const configModule = await jiti.import(configPathToLoad);
      const config = (configModule as any).default || configModule;

      if (!config) {
        throw new Error('Config file must export a default configuration object.');
      }

      // Apply overrides
      if (options.model) {
        config.agent.model = options.model;
      }
      if (options.projectDir) {
        config.agent.projectDir = options.projectDir;
      }
      if (options.thinkingLevel !== undefined) {
        config.agent.thinkingLevel = options.thinkingLevel;
      }
      if (options.cycleDelay) {
        config.rateLimits.cycleDelay = options.cycleDelay;
      }
      if (options.maxTokens !== undefined && !isNaN(options.maxTokens)) {
        config.rateLimits.maxTokensPerCycle = options.maxTokens;
      }
      if (options.maxOps !== undefined && !isNaN(options.maxOps)) {
        config.rateLimits.maxOpsPerMinute = options.maxOps;
      }

      log.step('Starting pi-loop daemon...');
      const loop = createLoop(config);
      await loop.start();
    } catch (error) {
      log.error(`Failed to run pi-loop: ${error}`);
      process.exit(1);
    }
  });

program
  .command('generate-systemd')
  .description('Generate a systemd service file from the config')
  .option('-g, --global', 'Use global config in home directory')
  .action(async (options) => {
    const targetDir = options.global ? path.join(os.homedir(), '.pi-loop') : process.cwd();
    const configFileName = options.global ? 'config.ts' : 'pi-loop.config.ts';
    const configPath = path.join(targetDir, configFileName);

    if (!fs.existsSync(configPath)) {
      log.error(`Could not find config at ${configPath}`);
      process.exit(1);
    }

    try {
      const configModule = await jiti.import(configPath);
      const config = (configModule as any).default || configModule;

      const user = config.systemd?.user || os.userInfo().username;
      const workingDir = config.systemd?.workingDir || targetDir;
      const restartSec = config.systemd?.restartSec || 10;
      const nice = config.systemd?.nice || 10;
      const environment = renderSystemdEnvironment(config.systemd?.environment);

      const execStart = 'pi-loop run';

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
${environment}
[Install]
WantedBy=multi-user.target
`;
      const systemdDir = path.join(targetDir, 'systemd');
      if (!fs.existsSync(systemdDir)) {
        fs.mkdirSync(systemdDir, { recursive: true });
      }
      const outPath = path.join(systemdDir, 'pi-loop.service');
      fs.writeFileSync(outPath, serviceContent);
      log.success(`Created systemd service file at ${outPath}`);
    } catch (error) {
      log.error(`Failed to generate systemd file: ${error}`);
      process.exit(1);
    }
  });

program.parse();
