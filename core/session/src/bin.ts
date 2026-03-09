import { spawnAgentServer } from "./server.js"

async function main() {
  const agentName = process.argv[2]
  if (!agentName) {
    console.error("Usage: goddard-session <agent-name>")
    process.exit(1)
  }

  const agentServer = await spawnAgentServer(agentName)

  process.stdin.pipe(agentServer.stdin)
  agentServer.stdout.pipe(process.stdout)
  agentServer.stderr.pipe(process.stderr)
}

main().catch((err) => {
  console.error(err)
  process.exit(1)
})
