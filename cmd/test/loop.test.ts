import test from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, readFile, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { runCli } from "../src/index.ts";

import { mkdir } from "node:fs/promises";

test("loop init creates .goddard/config.ts", async () => {
  const cwd = await mkdtemp(join(tmpdir(), "goddard-loop-init-"));
  const previousCwd = process.cwd();
  process.chdir(cwd);

  try {
    const lines: string[] = [];
    const code = await runCli(
      ["loop", "init"],
      { stdout: (line) => lines.push(line), stderr: (line) => lines.push(`ERR:${line}`) }
    );

    assert.equal(code, 0);
    const configPath = join(cwd, ".goddard", "config.ts");
    const config = await readFile(configPath, "utf-8");
    assert.match(config, /maxTokensPerCycle/);
    assert.ok(lines.some((line) => line.includes("Created configuration at")));
  } finally {
    process.chdir(previousCwd);
  }
});

test("loop run loads config and executes loop runtime", async () => {
  const cwd = await mkdtemp(join(tmpdir(), "goddard-loop-run-"));
  const previousCwd = process.cwd();
  process.chdir(cwd);

  try {
    await mkdir(join(cwd, ".goddard"), { recursive: true });
    await writeFile(
      join(cwd, ".goddard", "config.ts"),
      `export default {\n  agent: { model: "provider/model", projectDir: "." },\n  strategy: { nextPrompt: () => "DONE" },\n  rateLimits: { cycleDelay: "1s", maxTokensPerCycle: 1000, maxOpsPerMinute: 60 }\n};\n`,
      "utf-8"
    );

    let started = false;
    const code = await runCli(
      ["loop", "run"],
      { stdout: () => {}, stderr: () => {} },
      {
        createLoopRuntime: () => ({
          start: async () => {
            started = true;
          }
        })
      }
    );

    assert.equal(code, 0);
    assert.equal(started, true);
  } finally {
    process.chdir(previousCwd);
  }
});

test("loop generate-systemd creates goddard.service", async () => {
  const cwd = await mkdtemp(join(tmpdir(), "goddard-loop-systemd-"));
  const previousCwd = process.cwd();
  process.chdir(cwd);

  try {
    await mkdir(join(cwd, ".goddard"), { recursive: true });
    await writeFile(
      join(cwd, ".goddard", "config.ts"),
      `export default {\n  agent: { model: "provider/model", projectDir: "." },\n  strategy: { nextPrompt: () => "DONE" },\n  rateLimits: { cycleDelay: "1s", maxTokensPerCycle: 1000, maxOpsPerMinute: 60 },\n  systemd: { user: "deployer", workingDir: "/opt/repo" }\n};\n`,
      "utf-8"
    );

    const code = await runCli(["loop", "generate-systemd"], {
      stdout: () => {},
      stderr: () => {}
    });

    assert.equal(code, 0);
    const service = await readFile(join(cwd, "systemd", "goddard.service"), "utf-8");
    assert.match(service, /ExecStart=goddard loop run/);
    assert.match(service, /WorkingDirectory=\/opt\/repo/);
  } finally {
    process.chdir(previousCwd);
  }
});
