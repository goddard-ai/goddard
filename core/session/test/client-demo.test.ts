import { mkdtemp, rm, writeFile } from "node:fs/promises"
import { join } from "node:path"
import { tmpdir } from "node:os"
import { describe, expect, test } from "vitest"
import { buildDemoRunConfig, parseAgentOption, parseDemoArgs } from "../src/bin/client-demo.js"

describe("client demo argument parsing", () => {
  test("defaults to local custom distribution", async () => {
    const options = await parseDemoArgs([])
    const config = buildDemoRunConfig(options)

    expect(typeof config.agent).toBe("object")
    if (typeof config.agent === "string") {
      throw new Error("expected custom distribution")
    }

    expect(config.agent.type).toBe("binary")
    expect(config.agent.cmd).toBe("node")
    expect(config.agent.args?.[0]).toContain("@agentclientprotocol")
    expect(config.agent.args?.[0]).toContain("examples/agent.js")
  })

  test("enables real-world agent when --enable-auth is set", async () => {
    const options = await parseDemoArgs(["--enable-auth"])
    const config = buildDemoRunConfig(options)

    expect(config.agent).toBe("claude-code")
  })

  test("parses prompt and cwd flags", async () => {
    const options = await parseDemoArgs(["--prompt", "hello", "--cwd", "/tmp/demo"])

    expect(options.prompt).toBe("hello")
    expect(options.cwd).toBe("/tmp/demo")
  })

  test("uses --agent name when provided", async () => {
    const options = await parseDemoArgs(["--agent", "my-local-agent"])
    const config = buildDemoRunConfig(options)

    expect(config.agent).toBe("my-local-agent")
  })

  test("uses --agent distribution JSON when provided", async () => {
    const options = await parseDemoArgs([
      "--agent",
      '{"type":"binary","cmd":"node","args":["./agent.js"]}',
    ])
    const config = buildDemoRunConfig(options)

    expect(config.agent).toEqual({
      type: "binary",
      cmd: "node",
      args: ["./agent.js"],
    })
  })

  test("parses bare --agent value as a registry name", () => {
    expect(parseAgentOption("claude-code")).toBe("claude-code")
  })

  test("uses --agent-file JSON distribution when provided", async () => {
    const tempDir = await mkdtemp(join(tmpdir(), "goddard-session-demo-"))
    const distributionPath = join(tempDir, "agent-distribution.json")

    await writeFile(
      distributionPath,
      JSON.stringify({
        type: "binary",
        cmd: "node",
        args: ["./agent-from-file.js"],
      }),
      "utf8",
    )

    try {
      const options = await parseDemoArgs(["--agent-file", distributionPath])
      const config = buildDemoRunConfig(options)

      expect(config.agent).toEqual({
        type: "binary",
        cmd: "node",
        args: ["./agent-from-file.js"],
      })
    } finally {
      await rm(tempDir, { recursive: true, force: true })
    }
  })
})
