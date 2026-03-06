import { defineConfig } from "drizzle-kit"

export default defineConfig({
  schema: "./src/schema.ts",
  out: "./migrations",
  dialect: "turso",
  dbCredentials: {
    url: process.env.TURSO_DB_URL!,
    authToken: process.env.TURSO_DB_AUTH_TOKEN,
  },
})
