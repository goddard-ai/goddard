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

for (const announcesReady of [false, true]) {
  test(`dev recovers after the previous daemon exits ${announcesReady ? "after" : "before"} readiness`, async () => {
    const { root, bin, dataDir, env } = await makeWorkspace("goddard-dev-recovery-test-");
    const oldBundle = join(dataDir, "debug", "lanes", "b", "Goddard Debug.app");
    const seed = Bun.spawn([join(bin, "bundle-stub")], {
      env: { ...env, GODDARD_BUNDLE_DIR: oldBundle },
    });
    expect(await seed.exited).toBe(0);
    await writeFile(join(dataDir, "debug", "lanes", "b", ".complete"), "seed\n");
    await writeFile(join(oldBundle, "Contents", "MacOS", "goddard-debug-daemon"), `#!${process.execPath}
import { existsSync } from "node:fs";
import { join } from "node:path";
${announcesReady ? 'console.log(JSON.stringify({address: "127.0.0.1:34198"}));' : ''}
${announcesReady ? 'while (!existsSync(join(process.env.TEST_ROOT, "exit-old-daemon"))) await Bun.sleep(10);' : ''}
process.exit(1);
`, { mode: 0o755 });
    await writeFile(join(bin, "mbx"), `#!${process.execPath}
import { existsSync, writeFileSync } from "node:fs";
import { join } from "node:path";
writeFileSync(join(process.env.TEST_ROOT, "building"), "");
while (!existsSync(join(process.env.TEST_ROOT, "release-build"))) await Bun.sleep(10);
`, { mode: 0o755 });
    let output = "";
    const decoder = new TextDecoder();
    const terminal = new Bun.Terminal({
      cols: 240,
      rows: 40,
      data(_terminal, bytes) {
        output += decoder.decode(bytes, { stream: true });
      },
    });
    const child = Bun.spawn([process.execPath, devScript, "--wait"], { env, terminal });
    try {
      await waitFor(() => existsSync(join(root, "building")), 10_000, "initial build");
      if (announcesReady) {
        expect(output).toContain("Daemon listening on 127.0.0.1:34198");
        await writeFile(join(root, "exit-old-daemon"), "");
        await waitFor(() => output.includes("Daemon exited unexpectedly"), 5_000, "old daemon exit");
        expect(existsSync(join(dataDir, "debug", "goddard-daemon.json"))).toBe(false);
        // A failed daemon stays stopped until a fresh build or explicit restart.
        terminal.write("a\n");
        await waitFor(() => output.includes("press b + enter to restart it and launch"), 5_000, "blocked app launch");
        expect(output).not.toContain("[goddard-dev] Launching ");
      }
      await writeFile(join(root, "release-build"), "");
      await waitFor(() => output.includes("Daemon listening on 127.0.0.1:34199"), 20_000, "fresh daemon startup");
      await waitFor(() => output.includes("App launcher finished"), 5_000, "app launcher completion");
      expect(output.indexOf("Daemon listening on 127.0.0.1:34199")).toBeLessThan(output.indexOf("[goddard-dev] Launching "));
      expect(output).toContain("the app's exit status is unavailable");
      expect(child.exitCode).toBeNull();
      const infoPath = join(dataDir, "debug", "goddard-daemon.json");
      expect(existsSync(infoPath)).toBe(true);
      if (announcesReady) {
        // Even the freshly built binary must not enter a restart loop if it dies fast.
        const ps = Bun.spawn(["ps", "-axo", "pid=,ppid=,comm="], { stdout: "pipe" });
        const processes = await new Response(ps.stdout).text();
        expect(await ps.exited).toBe(0);
        const daemonProcess = processes
          .split("\n")
          .map((line) => line.trim().split(/\s+/))
          .find((parts) => Number(parts[1]) === child.pid && parts[2]?.endsWith("sleep"));
        expect(daemonProcess).toBeDefined();
        const previousOutput = output.length;
        process.kill(Number(daemonProcess![0]), "SIGTERM");
        await waitFor(() => output.slice(previousOutput).includes("Press d + enter to restart"), 5_000, "fresh daemon failure");
        expect(existsSync(infoPath)).toBe(false);
        const launches = output.match(/\[goddard-dev\] Launching /g)?.length;
        terminal.write("a\n");
        await waitFor(() => output.slice(previousOutput).includes("press b + enter to restart it and launch"), 5_000, "blocked relaunch after repeated failure");
        expect(output.match(/\[goddard-dev\] Launching /g)?.length).toBe(launches);
        expect(output.slice(previousOutput)).not.toContain("Daemon listening");
      }
      // Shutdown must also remove the endpoint discovery file.
      terminal.write("q\n");
      await child.exited;
      expect(existsSync(infoPath)).toBe(false);
    } finally {
      if (child.exitCode === null) {
        child.kill();
        const exited = await Promise.race([
          child.exited.then(() => true),
          Bun.sleep(2_000).then(() => false),
        ]);
        if (!exited) child.kill("SIGKILL");
        await child.exited;
      }
      terminal.close();
      await rm(root, { recursive: true, force: true });
    }
  }, 40_000);
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

test("narrow interactive builds coordinate tunnel logs, Cargo progress and live diagnostics", async () => {
  const { root, bin, env } = await makeWorkspace("goddard-dev-progress-test-");
  await writeFile(join(bin, "mbx"), `#!${process.execPath}
import { existsSync, writeFileSync } from "node:fs";
import { join } from "node:path";
const root = process.env.TEST_ROOT;
if (!process.argv.includes("waku")) {
  console.log(JSON.stringify({ reason: "compiler-artifact", fresh: true }));
  process.exit(0);
}
process.stderr.write("\\x1b[1m    Building\\x1b[0m [===> ] 737/744: waku-drivers, waku-server, goddard-agent(bin), waku-sessions\\r\\x1b[K");
console.log(JSON.stringify({ reason: "compiler-message", message: {
  level: "warning", rendered: "warning: live diagnostic probe\\n",
} }));
writeFileSync(join(root, "waiting"), "");
while (!existsSync(join(root, "release"))) await Bun.sleep(20);
// No trailing newline: stream closure must flush this Cargo-level failure.
process.stderr.write("error: linker probe failed");
process.exit(1);
`, { mode: 0o755 });
  await writeFile(join(bin, "cloudflared"), `#!${process.execPath}
import { existsSync } from "node:fs";
import { join } from "node:path";
if (process.argv.includes("list")) {
  console.log(JSON.stringify([{ id: "test-tunnel", name: "goddard-dev", deleted_at: "0001" }]));
} else if (process.argv.includes("run")) {
  while (!existsSync(join(process.env.TEST_ROOT, "waiting"))) await Bun.sleep(20);
  console.error("tunnel log probe " + "connection details ".repeat(12));
  while (true) await Bun.sleep(1000);
}
`, { mode: 0o755 });
  const serveProbe = Bun.serve({ port: 0, fetch: () => new Response() });
  const servePort = serveProbe.port;
  serveProbe.stop(true);
  let output = "";
  const decoder = new TextDecoder();
  const terminal = new Bun.Terminal({
    cols: 80,
    rows: 40,
    data(_terminal, bytes) {
      output += decoder.decode(bytes, { stream: true });
    },
  });
  const child = Bun.spawn([process.execPath, devScript, "--wait", "--serve"], {
    env: { ...env, GODDARD_DEV_SERVE_PORT: String(servePort), GODDARD_DEV_TUNNEL: "goddard-dev" },
    terminal,
  });
  try {
    await waitFor(
      () => existsSync(join(root, "waiting")) && output.includes("1 warning"),
      10_000,
      "the app build and buffered warning",
    );
    expect(output).toContain("737/744 units");
    await waitFor(
      () => output.includes("cloudflared: tunnel log probe"),
      5_000,
      "a tunnel log during the build",
    );
    expect(output).toContain("\r\x1b[2K[goddard-dev] cloudflared: tunnel log probe");
    const frames = output.match(/\[goddard-dev\] Compiling [^\r\n\x1b]*/g) ?? [];
    expect(frames.length).toBeGreaterThan(0);
    for (const frame of frames) expect(Bun.stringWidth(frame)).toBeLessThan(80);
    expect(output).not.toContain("████████ 737/744");
    expect(output).toContain("(all fresh)");
    expect(output).not.toContain("warning: live diagnostic probe");
    terminal.write("e\n");
    await waitFor(
      () => output.includes("warning: live diagnostic probe"),
      5_000,
      "live diagnostic expansion",
    );
    await writeFile(join(root, "release"), "");
    await waitFor(
      () => output.includes("error: linker probe failed"),
      5_000,
      "Cargo stderr failure output",
    );
    await waitFor(
      () => output.includes("initial build failed"),
      5_000,
      "the build to finish",
    );
    terminal.write("q\n");
    expect(await child.exited).toBe(0);
    expect(output).not.toContain("Building [");
    expect(output.match(/warning: live diagnostic probe/g)).toHaveLength(1);
  } finally {
    if (child.exitCode === null) {
      child.kill();
      await child.exited;
    }
    terminal.close();
    await rm(root, { recursive: true, force: true });
  }
}, 20_000);
