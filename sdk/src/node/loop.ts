import { createJiti } from "@mariozechner/jiti";
import { dirname, join } from "node:path";
import { mkdir, writeFile } from "node:fs/promises";
import { createLoop, type GoddardLoopConfig } from "@goddard-ai/loop";
import {
  getGlobalConfigPath,
  getLocalConfigPath,
  fileExists,
  resolveLoopConfigPath
} from "@goddard-ai/storage";

import DEFAULT_LOOP_CONFIG_TEMPLATE from "../../default-config.ts?raw";

export async function initLoopConfig(options: { global?: boolean }): Promise<{ path: string }> {
  const targetPath = options.global ? getGlobalConfigPath() : getLocalConfigPath();

  if (await fileExists(targetPath)) {
    throw new Error(`Config file already exists at ${targetPath}`);
  }

  await mkdir(dirname(targetPath), { recursive: true });
  await writeFile(targetPath, DEFAULT_LOOP_CONFIG_TEMPLATE, "utf-8");

  return { path: targetPath };
}

export async function loadLoopConfig(cwd: string = process.cwd(), options?: { global?: boolean }): Promise<{ config: GoddardLoopConfig; path: string }> {
  let configPath: string | null = null;
  
  if (options?.global !== undefined) {
      configPath = options.global ? getGlobalConfigPath() : getLocalConfigPath();
      if (!(await fileExists(configPath))) {
          configPath = null;
      }
  } else {
      configPath = await resolveLoopConfigPath();
  }

  if (!configPath) {
    throw new Error("Could not find config.ts in the current directory's .goddard/ folder or in ~/.goddard.");
  }

  const jiti = createJiti(cwd);
  const module = await jiti.import(configPath);
  const config = (module as any).default ?? module;
  
  if (!config) {
    throw new Error("Config file must export a default configuration object.");
  }

  return { config: config as GoddardLoopConfig, path: configPath };
}

export async function runLoop(cwd: string = process.cwd(), deps?: { createLoopRuntime?: typeof createLoop }): Promise<void> {
  const { config } = await loadLoopConfig(cwd);
  const runtime = deps?.createLoopRuntime ?? createLoop;
  const loop = runtime(config);
  await loop.start();
}

function quoteSystemdValue(value: string): string {
  return `"${value.replace(/\\/g, "\\\\").replace(/"/g, "\\\"")}"`;
}

function renderSystemdEnvironment(environment?: Record<string, string | undefined>): string {
  if (!environment) {
    return "";
  }

  const lines = Object.entries(environment)
    .filter(([, value]) => value !== undefined)
    .map(([key, value]) => `Environment=${key}=${quoteSystemdValue(value as string)}`);

  return lines.length > 0 ? `${lines.join("\n")}\n` : "";
}

export async function generateLoopSystemdService(
  cwd: string = process.cwd(),
  options: { global?: boolean; user?: string }
): Promise<{ path: string }> {
  const { config } = await loadLoopConfig(cwd, { global: options.global });
  
  const os = await import("node:os");
  const targetRoot = options.global ? os.homedir() : cwd;
  const outputPath = join(targetRoot, "systemd", "goddard.service");

  const user = config.systemd?.user ?? options.user ?? process.env.USER ?? "root";
  const workingDir = config.systemd?.workingDir ?? cwd;
  const restartSec = config.systemd?.restartSec ?? 10;
  const nice = config.systemd?.nice ?? 10;
  const environment = renderSystemdEnvironment(config.systemd?.environment);

  const service = `[Unit]\nDescription=Goddard Autonomous Agent Loop\nAfter=network.target\n\n[Service]\nType=simple\nUser=${user}\nWorkingDirectory=${workingDir}\nExecStart=goddard loop run\nRestart=always\nRestartSec=${restartSec}\nNice=${nice}\n${environment}[Install]\nWantedBy=multi-user.target\n`;

  await mkdir(dirname(outputPath), { recursive: true });
  await writeFile(outputPath, service, "utf-8");

  return { path: outputPath };
}
