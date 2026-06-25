import { startElectrobun } from "./electrobun.ts"
import { startViteDevServer } from "./vite.ts"

async function main() {
  await startViteDevServer()
  await startElectrobun()
}

await main()
