#!/usr/bin/env bun
//
// The publish side of the Dev update channel: package the built release
// bundle as a signed Sparkle archive, regenerate appcast.xml, and serve the
// directory to dev.goddardai.org through a named Cloudflare tunnel.
//
// `bun run dev --serve` drives this. Env overrides:
//   GODDARD_DEV_HOSTNAME    public hostname the appcast links to
//                           (default: dev.goddardai.org)
//   GODDARD_DEV_TUNNEL      cloudflared tunnel name (default: goddard-dev);
//                           created plus a DNS route when missing
//   GODDARD_DEV_SERVE_PORT  local port the tunnel forwards to
//                           (default: 8976)

import { $ } from "bun";
import { mkdir, rm, writeFile } from "node:fs/promises";
import { homedir } from "node:os";
import { extname, join, normalize } from "node:path";
import { generateAppcast } from "./appcast";
import { cargoPackageVersion, derivedBuildNumber } from "./version";

const CONTENT_TYPES: Record<string, string> = {
  ".xml": "application/xml",
  ".zip": "application/zip",
  ".md": "text/markdown; charset=utf-8",
};

export type DevServe = {
  hostname: string;
  port: number;
  updatesDir: string;
  appcastUrl: string;
  /** Stamp, re-sign, archive, and re-sign the appcast for `appBundle`. */
  deploy(appBundle: string): Promise<boolean>;
  stop(): Promise<void>;
};

type TunnelInfo = { id: string; name: string };

async function listTunnels(): Promise<TunnelInfo[]> {
  const result = await $`cloudflared tunnel list -o json`.quiet().nothrow();
  if (result.exitCode !== 0) return [];
  try {
    const entries = JSON.parse(result.stdout.toString()) as Array<{
      id?: string;
      name?: string;
      deleted_at?: string;
    }>;
    return entries
      .filter(
        (entry) =>
          typeof entry.id === "string" &&
          typeof entry.name === "string" &&
          (entry.deleted_at ?? "").startsWith("0001"),
      )
      .map((entry) => ({ id: entry.id!, name: entry.name! }));
  } catch {
    return [];
  }
}

async function pump(
  stream: ReadableStream<Uint8Array> | number | null | undefined,
  log: (line: string) => void,
): Promise<void> {
  if (stream === null || stream === undefined || typeof stream === "number") {
    return;
  }
  const reader = stream.getReader();
  const decoder = new TextDecoder();
  let buffer = "";
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    buffer += decoder.decode(value, { stream: true });
    let newline = buffer.indexOf("\n");
    while (newline !== -1) {
      log(buffer.slice(0, newline));
      buffer = buffer.slice(newline + 1);
      newline = buffer.indexOf("\n");
    }
  }
  if (buffer) log(buffer);
}

/** Bring up (or adopt) the named tunnel and point `hostname` at the local
 *  port. Returns the running cloudflared child, or undefined when the tunnel
 *  cannot be started — the feed still serves locally then. */
async function startTunnel(options: {
  name: string;
  hostname: string;
  port: number;
  workDir: string;
  log: (line: string) => void;
}): Promise<Bun.Subprocess | undefined> {
  const { name, hostname, port, workDir, log } = options;
  if (!Bun.which("cloudflared")) {
    log(
      "cloudflared is not installed; the feed is only reachable at " +
        `http://localhost:${port}.`,
    );
    return undefined;
  }

  let tunnels = await listTunnels();
  let tunnel = tunnels.find((candidate) => candidate.name === name);
  if (tunnel === undefined) {
    log(`Creating the "${name}" Cloudflare tunnel...`);
    const created = await $`cloudflared tunnel create ${name}`.quiet().nothrow();
    if (created.exitCode !== 0) {
      log(
        `cloudflared tunnel create failed: ${created.stderr.toString().trim()}. ` +
          "Run `cloudflared login`, or set GODDARD_DEV_TUNNEL to an existing tunnel.",
      );
      return undefined;
    }
    tunnels = await listTunnels();
    tunnel = tunnels.find((candidate) => candidate.name === name);
  }
  if (tunnel === undefined) {
    log(`Tunnel "${name}" was created but does not list; cannot run it.`);
    return undefined;
  }

  // `route dns` errors when the route already exists — either way the
  // hostname ends up pointing at this tunnel.
  const route =
    await $`cloudflared tunnel route dns ${tunnel.id} ${hostname}`.quiet().nothrow();
  if (route.exitCode !== 0) {
    const detail = route.stderr.toString().trim() || route.stdout.toString().trim();
    log(`DNS route for ${hostname} not created (${detail}); assuming it exists.`);
  }

  const credentials = join(homedir(), ".cloudflared", `${tunnel.id}.json`);
  const configPath = join(workDir, "cloudflared.yml");
  await writeFile(
    configPath,
    [
      `tunnel: ${tunnel.id}`,
      `credentials-file: ${credentials}`,
      "ingress:",
      `  - hostname: ${hostname}`,
      `    service: http://localhost:${port}`,
      "  - service: http_status:404",
      "",
    ].join("\n"),
  );

  const child = Bun.spawn(
    ["cloudflared", "tunnel", "--config", configPath, "run"],
    { stdout: "pipe", stderr: "pipe" },
  );
  void pump(child.stdout, (line) => log(`cloudflared: ${line}`));
  void pump(child.stderr, (line) => log(`cloudflared: ${line}`));
  void child.exited.then((code) => {
    log(`cloudflared exited (${code}); the tunnel is down.`);
  });
  return child;
}

function serveFile(updatesDir: string, request: Request): Response | Promise<Response> {
  const pathname = decodeURIComponent(new URL(request.url).pathname);
  const relative = normalize(pathname.replace(/^\/+/, ""));
  if (relative.startsWith("..") || relative.includes("\0")) {
    return new Response("forbidden", { status: 403 });
  }
  const path = join(updatesDir, relative);
  if (!path.startsWith(updatesDir)) {
    return new Response("forbidden", { status: 403 });
  }
  const file = Bun.file(path);
  return file.exists().then((exists) => {
    if (!exists) return new Response("not found", { status: 404 });
    const type = CONTENT_TYPES[extname(path).toLowerCase()];
    return new Response(file, {
      headers: type ? { "Content-Type": type } : {},
    });
  });
}

/** Re-sign the bundle after stamping its version — plutil invalidates the
 *  signature bundle.sh wrote. The identity is read back from the signature
 *  itself so whatever bundle.sh picked is preserved. */
async function resignBundle(appBundle: string): Promise<void> {
  const describe = await $`codesign -dv --verbose=2 ${appBundle}`.quiet().nothrow();
  const details = `${describe.stderr}\n${describe.stdout}`;
  const authority = details.match(/^Authority=(.+)$/m)?.[1];
  if (authority === undefined) {
    // Ad-hoc signature: no hardened runtime (release.ts documents why the
    // updater cannot load Sparkle under an ad-hoc hardened runtime).
    await $`codesign --force --sign - ${appBundle}`;
    return;
  }
  await $`codesign --force --options runtime --timestamp --sign ${authority} ${appBundle}`;
}

export async function startDevServe(options: {
  root: string;
  targetDir: string;
}): Promise<DevServe> {
  const { root, targetDir } = options;
  const log = (line: string) => console.log(`[goddard-dev] ${line}`);
  const hostname = process.env.GODDARD_DEV_HOSTNAME ?? "dev.goddardai.org";
  const tunnelName = process.env.GODDARD_DEV_TUNNEL ?? "goddard-dev";
  const port = Number(process.env.GODDARD_DEV_SERVE_PORT ?? 8976);
  const workDir = join(targetDir, "dev-serve");
  const updatesDir = join(workDir, "updates");
  await mkdir(updatesDir, { recursive: true });

  const server = Bun.serve({
    port,
    fetch: (request) => serveFile(updatesDir, request),
  });
  const tunnel = await startTunnel({
    name: tunnelName,
    hostname,
    port,
    workDir,
    log,
  });
  const appcastUrl = `https://${hostname}/appcast.xml`;
  log(
    `Serving ${updatesDir} on http://localhost:${port}` +
      (tunnel ? ` → ${appcastUrl}` : " (no tunnel — local only)"),
  );

  return {
    hostname,
    port,
    updatesDir,
    appcastUrl,
    async deploy(appBundle: string): Promise<boolean> {
      const version = await cargoPackageVersion(root, "waku");
      const shortVersion = version.split("-", 1)[0];
      // Every deploy must out-version the last: the derived release number
      // keeps dev builds ahead of released versions while staying behind the
      // next real release, and the epoch suffix orders builds of one version.
      const buildNumber = `${derivedBuildNumber(version)}.${Math.floor(
        Date.now() / 1000,
      )}`;
      const plist = join(appBundle, "Contents", "Info.plist");
      await $`plutil -replace CFBundleShortVersionString -string ${shortVersion} ${plist}`;
      await $`plutil -replace CFBundleVersion -string ${buildNumber} ${plist}`;
      await resignBundle(appBundle);

      await rm(updatesDir, { force: true, recursive: true });
      await mkdir(updatesDir, { recursive: true });
      const zipName = `Goddard-${version}.zip`;
      await $`ditto -c -k --keepParent ${appBundle} ${join(updatesDir, zipName)}`;
      try {
        await generateAppcast(updatesDir, `https://${hostname}/`);
      } catch (error) {
        log(
          `Appcast generation failed (is the Sparkle key in the keychain?): ` +
            `${error instanceof Error ? error.message : error}`,
        );
        return false;
      }
      log(`Deployed ${zipName} as build ${buildNumber} to ${appcastUrl}.`);
      return true;
    },
    async stop(): Promise<void> {
      if (tunnel !== undefined) {
        tunnel.kill();
        await tunnel.exited;
      }
      server.stop(true);
    },
  };
}
