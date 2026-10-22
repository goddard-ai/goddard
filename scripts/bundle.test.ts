import { expect, test } from "bun:test";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { basename, join } from "node:path";

for (const cached of [false, true]) {
  test(`release bundling starts a build ${cached ? "with mbx" : "without mbx installed"}`, async () => {
    const root = await mkdtemp(join(tmpdir(), "goddard-bundle-tools-"));
    try {
      const tool = `#!${process.execPath}
console.log(JSON.stringify(process.argv.slice(1)));
process.exit(79);
`;
      for (const name of cached ? ["cargo", "mbx"] : ["cargo"]) {
        await writeFile(join(root, name), tool, { mode: 0o755 });
      }
      const child = Bun.spawn([
        "/bin/sh", join(import.meta.dir, "bundle.sh"), "release",
      ], {
        env: {
          PATH: root,
          GODDARD_CODESIGN_IDENTITY: "-",
          GODDARD_CACHE_DIR: join(root, "cache"),
        },
        stdout: "pipe",
        stderr: "pipe",
      });
      const stdout = await new Response(child.stdout).text();
      const stderr = await new Response(child.stderr).text();
      // Stop at the build boundary, before any packaging or signing tools run.
      expect(await child.exited, stderr).toBe(79);
      const [command, ...args] = JSON.parse(stdout) as string[];
      expect(basename(command)).toBe(cached ? "mbx" : "cargo");
      expect(args[0]).toBe("build");
      expect(args).toContain("release");
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  });
}
