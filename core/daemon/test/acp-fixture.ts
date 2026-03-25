/** Builds a real ACP agent distribution that launches one local Node script through a wrapper. */
export function createWrappedNodeAgent(agentPath: string) {
  const wrapper = `#!/bin/sh\nexec "${process.execPath}" "${agentPath}" "$@"\n`
  const archive = `data:text/plain;base64,${Buffer.from(wrapper).toString("base64")}`

  return {
    id: "node-agent",
    name: "Node Agent",
    version: "1.0.0",
    description: "Local node-based ACP test agent.",
    distribution: {
      binary: {
        "darwin-aarch64": { archive, cmd: "agent" },
        "darwin-x86_64": { archive, cmd: "agent" },
        "linux-aarch64": { archive, cmd: "agent" },
        "linux-x86_64": { archive, cmd: "agent" },
        "windows-aarch64": { archive, cmd: "agent" },
        "windows-x86_64": { archive, cmd: "agent" },
      },
    },
  }
}
