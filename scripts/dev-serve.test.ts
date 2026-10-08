import { expect, test } from "bun:test";
import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

test("dev publishing retains only the latest update and preserves it on failure", async () => {
  const root = await mkdtemp(join(tmpdir(), "goddard-dev-serve-"));
  try {
    const bin = join(root, "bin");
    await mkdir(bin);
    // Substitute platform packaging tools, not publication or HTTP behavior.
    const tool = `#!${process.execPath}
import { basename, join } from "node:path";
import { existsSync, readdirSync, writeFileSync } from "node:fs";
const name = basename(process.argv[1]);
const args = process.argv.slice(2);
if (name === "cargo") console.log(JSON.stringify({ packages: [{ name: "waku", version: "0.2.0" }] }));
if (name === "cloudflared") {
  if (args[1] === "list") console.log("[]");
  else process.exit(1);
}
if (name === "ditto") writeFileSync(args.at(-1), "archive");
if (name === "generate_appcast") {
  if (existsSync(join(process.env.TEST_ROOT, "fail"))) process.exit(1);
  const dir = args.at(-1);
  const zip = readdirSync(dir).find(name => name.endsWith(".zip"));
  writeFileSync(join(dir, "appcast.xml"), '<enclosure url="https://dev.invalid/' + zip + '" sparkle:edSignature="test" />');
}
`;
    for (const name of ["cargo", "cloudflared", "plutil", "codesign", "ditto", "generate_appcast"]) {
      await writeFile(join(bin, name), tool, { mode: 0o755 });
    }
    const runner = join(root, "runner.ts");
    await writeFile(runner, `
import { startDevServe } from ${JSON.stringify(join(import.meta.dir, "dev-serve.ts"))};
import { readdir, writeFile } from "node:fs/promises";
import { join } from "node:path";
const root = process.env.TEST_ROOT;
const probe = Bun.serve({ port: 0, fetch: () => new Response() });
process.env.GODDARD_DEV_SERVE_PORT = String(probe.port);
probe.stop(true);
const server = await startDevServe({ root, workDir: join(root, "serve") });
try {
  const url = "http://localhost:" + server.port;
  if (!await server.deploy(join(root, "app"))) throw new Error("first publish failed");
  const first = (await readdir(server.updatesDir)).find(name => name.endsWith(".zip"));
  await writeFile(join(server.updatesDir, "Goddard-obsolete.zip"), "obsolete");
  if (!await server.deploy(join(root, "app"))) throw new Error("second publish failed");
  const files = await readdir(server.updatesDir);
  const archives = files.filter(name => name.endsWith(".zip"));
  if (archives.length !== 1 || archives[0] === first) throw new Error("old archives retained");
  if ((await fetch(url + "/" + first)).status !== 404) throw new Error("old update still served");
  const feed = await (await fetch(url + "/appcast.xml")).text();
  if (!feed.includes(archives[0])) throw new Error("feed does not advertise current archive");
  if (await (await fetch(url + "/" + archives[0])).text() !== "archive") throw new Error("archive unavailable");
  await writeFile(join(root, "fail"), "");
  if (await server.deploy(join(root, "app"))) throw new Error("failed publish reported success");
  if (await (await fetch(url + "/appcast.xml")).text() !== feed) throw new Error("working feed changed on failure");
  if (JSON.stringify(await readdir(server.updatesDir)) !== JSON.stringify(files)) throw new Error("working archive changed on failure");
} finally {
  await server.stop();
}
`);
    const child = Bun.spawn([process.execPath, runner], {
      env: {
        ...process.env,
        PATH: bin,
        SPARKLE_BIN: bin,
        SPARKLE_PRIVATE_KEY: "",
        GODDARD_DEV_SERVE_PORT: "0",
        GODDARD_DEV_HOSTNAME: "dev.invalid",
        TEST_ROOT: root,
      },
      stdout: "pipe",
      stderr: "pipe",
    });
    const [code, stdout, stderr] = await Promise.all([
      child.exited,
      new Response(child.stdout).text(),
      new Response(child.stderr).text(),
    ]);
    expect(code, stdout + stderr).toBe(0);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});
