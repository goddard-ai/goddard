import { InMemoryBackendControlPlane, startBackendServer } from "../src/index.ts";
import { TursoBackendControlPlane } from "../src/persistence.ts";
import { createClient } from "@libsql/client";

const port = Number(process.env.PORT ?? "8787");
const dbUrl = process.env.DATABASE_URL;

let controlPlane;
if (dbUrl) {
  process.stdout.write(`Using database at ${dbUrl}\n`);
  const client = createClient({ url: dbUrl });
  controlPlane = new TursoBackendControlPlane(client);
} else {
  process.stdout.write("Using in-memory control plane\n");
  controlPlane = new InMemoryBackendControlPlane();
}

const server = await startBackendServer(controlPlane, { port });
process.stdout.write(`goddard backend listening on http://127.0.0.1:${server.port}\n`);

const shutdown = async () => {
  await server.close();
  process.exit(0);
};

process.on("SIGINT", shutdown);
process.on("SIGTERM", shutdown);
