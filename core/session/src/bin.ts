import { SessionServer } from "./server.js"

async function main() {
  let agentName: string | undefined
  let resumeId: string | undefined

  for (let i = 2; i < process.argv.length; i++) {
    if (process.argv[i] === "--resume") {
      resumeId = process.argv[++i]
    } else if (!agentName) {
      agentName = process.argv[i]
    }
  }

  if (!agentName) {
    console.error("Usage: goddard-session <agent-name> [--resume <id>]")
    process.exit(1)
  }

  const server = new SessionServer(agentName)

  if (resumeId) {
    await server.loadSession({ sessionId: resumeId, mcpServers: [], cwd: process.cwd() })
  }

  await server.listen()
}

main().catch((err) => {
  console.error(err)
  process.exit(1)
})
