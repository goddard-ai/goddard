import { describe, expect, it, vi } from "vitest"

import { buildCodexArgs } from "../src/drivers/codex"
import { buildGeminiArgs } from "../src/drivers/gemini"
import { loadEmbeddedDriver, type DriverImporter } from "../src/drivers/registry"

describe("embedded drivers", () => {
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

  it("loads an embedded driver via dynamic importer", async () => {
    const importer = vi.fn(async () => ({
      driver: {
        name: "gemini",
        run: async () => 0,
      },
    })) as DriverImporter

    const driver = await loadEmbeddedDriver("gemini", importer)

    expect(importer).toHaveBeenCalledWith("./gemini.ts")
    expect(driver.name).toBe("gemini")
  })
})
