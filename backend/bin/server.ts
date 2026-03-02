#!/usr/bin/env -S pnpm tsx
import { InMemoryBackendControlPlane, startBackendServer } from "../src/index.ts";

const port = Number(process.env.PORT ?? "8787");
const server = await startBackendServer(new InMemoryBackendControlPlane(), { port });
process.stdout.write(`goddard backend listening on http://127.0.0.1:${server.port}\n`);

const shutdown = async () => {
  await server.close();
  process.exit(0);
};

process.on("SIGINT", shutdown);
process.on("SIGTERM", shutdown);
