import { describe, expect, it, vi } from "vitest"

import { buildCodexArgs } from "../src/plugins/codex"
import { buildGeminiArgs } from "../src/plugins/gemini"
import { loadEmbeddedPlugin, type PluginImporter } from "../src/plugins/registry"

describe("embedded plugins", () => {
  it("builds gemini args with resume and prompt", () => {
    expect(buildGeminiArgs({ resume: "session-id", initialPrompt: "hello" })).toEqual([
      "--output-format",
      "stream-json",
      "--resume",
      "session-id",
      "--prompt",
      "hello",
    ])
  })

  it("builds codex args with resume and prompt", () => {
    expect(buildCodexArgs({ resume: "thread-id", initialPrompt: "continue" })).toEqual([
      "exec",
      "resume",
      "thread-id",
      "--json",
      "continue",
    ])
  })

  it("loads an embedded plugin via dynamic importer", async () => {
    const importer = vi.fn(async () => ({
      plugin: {
        name: "gemini",
        run: async () => 0,
      },
    })) as PluginImporter

    const plugin = await loadEmbeddedPlugin("gemini", importer)

    expect(importer).toHaveBeenCalledWith("./gemini.ts")
    expect(plugin.name).toBe("gemini")
  })
})
