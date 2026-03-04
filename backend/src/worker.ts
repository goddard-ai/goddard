import adapter from "@hattip/adapter-cloudflare-workers/no-static";
import type { Env } from "./env.ts";
import router from "./router.ts";
import { RepoStream } from "./objects/RepoStream.ts";

export { RepoStream };

export default {
  fetch: adapter(router)
} satisfies ExportedHandler<Env>;
