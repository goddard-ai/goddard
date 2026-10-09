import { expect, test } from "bun:test";
import { existsSync } from "node:fs";
import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

const devScript = join(import.meta.dir, "dev.ts");

// dev.ts is a top-level script, so coverage drives the real process with
// platform tooling substituted — the same boundary dev-serve.test.ts uses.
// mbx fails while <TEST_ROOT>/fail exists; the bundle script fabricates a
// runtime lane, and the packaged daemon stub announces a ready address.
const stubTool = `#!${process.execPath}
import { basename, dirname, join } from "node:path";
import { chmodSync, cpSync, existsSync, mkdirSync, readdirSync, rmSync, writeFileSync } from "node:fs";
const name = basename(process.argv[1]);
const args = process.argv.slice(2);
const root = process.env.TEST_ROOT;
if (name === "mbx") process.exit(existsSync(join(root, "fail")) ? 1 : 0);
if (name === "cargo") console.log(JSON.stringify({ packages: [{ name: "waku", version: "0.2.0" }] }));
if (name === "cloudflared") process.exit(1);
if (name === "ditto") writeFileSync(args.at(-1), "archive");
if (name === "generate_appcast") {
  const dir = args.at(-1);
  const zip = readdirSync(dir).find((entry) => entry.endsWith(".zip"));
  writeFileSync(join(dir, "appcast.xml"), '<enclosure url="https://dev.invalid/' + zip + '" sparkle:edSignature="test" />');
}
if (name === "bundle-stub") {
  const dir = process.env.GODDARD_BUNDLE_DIR;
  const source = process.env.GODDARD_BUNDLE_SOURCE;
  rmSync(dir, { recursive: true, force: true });
  if (source) {
    mkdirSync(dirname(dir), { recursive: true });
    cpSync(source, dir, { recursive: true });
  } else {
    const macos = join(dir, "Contents", "MacOS");
    const resources = join(dir, "Contents", "Resources");
    mkdirSync(macos, { recursive: true });
    mkdirSync(resources, { recursive: true });
    writeFileSync(join(dir, "Contents", "Info.plist"), "stub");
    const daemon =
      "#!/bin/sh\\nprintf '%s\\\\n' '{\\"address\\":\\"127.0.0.1:34199\\"}'\\nexec sleep 3600\\n";
    for (const exe of ["Goddard", "Goddard Debug", "goddard-daemon", "goddard-debug-daemon", "goddard_js_repl"]) {
      writeFileSync(join(macos, exe), exe.includes("daemon") ? daemon : "#!/bin/sh\\nexec sleep 3600\\n");
      chmodSync(join(macos, exe), 0o755);
    }
    writeFileSync(join(resources, "goddard-agent"), "#!/bin/sh\\nexit 0\\n");
    chmodSync(join(resources, "goddard-agent"), 0o755);
  }
}
process.exit(0);
`;

const toolNames = [
  "mbx",
  "cargo",
  "cloudflared",
  "ditto",
  "generate_appcast",
  "plutil",
  "codesign",
  "open",
  "osascript",
  "pkill",
  "security",
  "adb",
  "tailscale",
  "bundle-stub",
];

function scrubbedEnv(extra: Record<string, string>): Record<string, string> {
  const env: Record<string, string> = {};
  for (const [key, value] of Object.entries(process.env)) {
    if (value === undefined) continue;
    // A stray GODDARD_DAEMON_ADDRESS/TOKEN pair from an outer session would
    // make the child adopt a daemon it does not own.
    if (key.startsWith("GODDARD_") || key === "CARGO_TARGET_DIR") continue;
    env[key] = value;
  }
  return { ...env, ...extra };
}

async function makeWorkspace(prefix: string): Promise<{
  root: string;
  bin: string;
  dataDir: string;
  env: Record<string, string>;
}> {
  const root = await mkdtemp(join(tmpdir(), prefix));
  const bin = join(root, "bin");
  await mkdir(bin);
  for (const name of toolNames) {
    await writeFile(join(bin, name), stubTool, { mode: 0o755 });
  }
  const dataDir = join(root, "data");
  const env = scrubbedEnv({
    PATH: `${bin}:${process.env.PATH}`,
    TEST_ROOT: root,
    GODDARD_DATA_DIR: dataDir,
    GODDARD_CACHE_DIR: join(root, "cache"),
    CARGO_TARGET_DIR: join(root, "target"),
    GODDARD_BUNDLE_SCRIPT: join(bin, "bundle-stub"),
    GODDARD_DEV_HOSTNAME: "dev.invalid",
    SPARKLE_BIN: bin,
    SPARKLE_PRIVATE_KEY: "",
  });
  return { root, bin, dataDir, env };
}

async function waitFor(
  condition: () => boolean | Promise<boolean>,
  timeoutMs: number,
  detail: string,
): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (await condition()) return;
    await Bun.sleep(200);
  }
  throw new Error(`timed out waiting for ${detail}`);
}

test("dev --serve stays alive after a failed initial build and recovers on the next change", async () => {
  const { root, bin, dataDir, env } = await makeWorkspace("goddard-dev-test-");
  const serveProbe = Bun.serve({ port: 0, fetch: () => new Response() });
  const servePort = serveProbe.port;
  serveProbe.stop(true);
  await writeFile(join(root, "fail"), "");
  const child = Bun.spawn([process.execPath, devScript, "--serve"], {
    env: { ...env, GODDARD_DEV_SERVE_PORT: String(servePort) },
    stdout: "pipe",
    stderr: "pipe",
  });
  const childExited = child.exited.then(() => true);
  try {
    const outputPromise = (async () =>
      `${await new Response(child.stdout).text()}\n${await new Response(child.stderr).text()}`)();
    // The failed build is reported and the process keeps watching instead of
    // exiting — the feed answers even though nothing has published yet.
    const feedStatus = async () => {
      try {
        return (await fetch(`http://localhost:${servePort}/appcast.xml`))
          .status;
      } catch {
        return 0;
      }
    };
    await waitFor(
      async () => (await feedStatus()) === 404,
      20_000,
      "the update feed to come up",
    );
    await Bun.sleep(3_000);
    expect(
      await Promise.race([childExited, Bun.sleep(0).then(() => false)]),
      "watcher exited after the failed initial build",
    ).toBe(false);

    // Fix the tree and touch a watched file: the retry rebuilds, publishes,
    // and starts the daemon that startup could not.
    await rm(join(root, "fail"));
    const probe = join(import.meta.dir, ".dev-test-probe");
    await writeFile(probe, "");
    try {
      await waitFor(
        () => existsSync(join(dataDir, "debug", "goddard-daemon.json")),
        30_000,
        "the daemon to start after the rebuild",
      );
    } finally {
      await rm(probe, { force: true });
    }
    expect(await feedStatus()).toBe(200);
    const feed = await fetch(`http://localhost:${servePort}/appcast.xml`);
    expect(await feed.text()).toContain("dev.invalid");
    expect(
      await Promise.race([childExited, Bun.sleep(0).then(() => false)]),
      "watcher exited during the successful rebuild",
    ).toBe(false);
    child.kill("SIGTERM");
    await child.exited;
    const output = await outputPromise;
    expect(output).toContain("initial build failed");
    expect(output).toContain("Daemon listening");
    expect(output).not.toContain("Daemon exited unexpectedly");
  } finally {
    if (child.exitCode === null) child.kill();
    await rm(root, { recursive: true, force: true });
  }
}, 60_000);

test("dev --serve exits when the update feed cannot start", async () => {
  const { root, env } = await makeWorkspace("goddard-dev-test-");
  const blocker = Bun.serve({ port: 0, fetch: () => new Response() });
  try {
    const child = Bun.spawn([process.execPath, devScript, "--serve"], {
      env: { ...env, GODDARD_DEV_SERVE_PORT: String(blocker.port) },
      stdout: "pipe",
      stderr: "pipe",
    });
    const [code, stdout, stderr] = await Promise.all([
      child.exited,
      new Response(child.stdout).text(),
      new Response(child.stderr).text(),
    ]);
    expect(code, stdout + stderr).toBe(1);
    expect(stdout + stderr).toContain("Could not start the update feed");
  } finally {
    blocker.stop(true);
    await rm(root, { recursive: true, force: true });
  }
});

test("dev exits when the initial build fails outside serve mode", async () => {
  const { root, env } = await makeWorkspace("goddard-dev-test-");
  try {
    await writeFile(join(root, "fail"), "");
    const child = Bun.spawn([process.execPath, devScript], {
      env,
      stdout: "pipe",
      stderr: "pipe",
    });
    const [code, stdout, stderr] = await Promise.all([
      child.exited,
      new Response(child.stdout).text(),
      new Response(child.stderr).text(),
    ]);
    expect(code, stdout + stderr).toBe(1);
    expect(stdout + stderr).toContain("Daemon build failed");
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});
