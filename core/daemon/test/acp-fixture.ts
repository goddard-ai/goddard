/** Builds a real ACP agent distribution that launches one local Node script through a wrapper. */
export function createWrappedNodeAgent(agentPath: string) {
  const posixWrapper = `#!/bin/sh\nexec "${process.execPath}" "${agentPath}" "$@"\n`
  const windowsWrapper = `@echo off\r\n"${process.execPath}" "${agentPath}" %*\r\n`
  const posixArchive = toDataUrl(posixWrapper)
  const windowsArchive = toDataUrl(windowsWrapper)

  return {
    id: "node-agent",
    name: "Node Agent",
    version: "1.0.0",
    description: "Local node-based ACP test agent.",
    distribution: {
      binary: {
        "darwin-aarch64": { archive: posixArchive, cmd: "agent" },
        "darwin-x86_64": { archive: posixArchive, cmd: "agent" },
        "linux-aarch64": { archive: posixArchive, cmd: "agent" },
        "linux-x86_64": { archive: posixArchive, cmd: "agent" },
        "windows-aarch64": { archive: windowsArchive, cmd: "agent.cmd" },
        "windows-x86_64": { archive: windowsArchive, cmd: "agent.cmd" },
      },
    },
  }
}

function toDataUrl(content: string) {
  return `data:text/plain;base64,${Buffer.from(content).toString("base64")}`
}
