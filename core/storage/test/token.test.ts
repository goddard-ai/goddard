import { mkdtemp, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { afterEach, beforeEach, describe, expect, it } from "vitest"
import { FileTokenStorage } from "../src/token.ts"

describe("FileTokenStorage", () => {
  let tmpDir: string
  let tokenPath: string
  let storage: FileTokenStorage

  beforeEach(async () => {
    tmpDir = await mkdtemp(join(tmpdir(), "goddard-token-test-"))
    tokenPath = join(tmpDir, "credentials.json")
    storage = new FileTokenStorage(tokenPath)
  })

  afterEach(async () => {
    await rm(tmpDir, { recursive: true, force: true })
  })

  it("returns null when no token exists", async () => {
    const token = await storage.getToken()
    expect(token).toBeNull()
  })

  it("round-trips a token", async () => {
    await storage.setToken("my-secret-token")
    const token = await storage.getToken()
    expect(token).toBe("my-secret-token")
  })

  it("overwrites an existing token", async () => {
    await storage.setToken("first-token")
    await storage.setToken("second-token")
    const token = await storage.getToken()
    expect(token).toBe("second-token")
  })

  it("clears a token", async () => {
    await storage.setToken("my-token")
    await storage.clearToken()
    const token = await storage.getToken()
    expect(token).toBeNull()
  })

  it("recovers from malformed JSON by returning null", async () => {
    await writeFile(tokenPath, "{ malformed json", "utf-8")
    const token = await storage.getToken()
    expect(token).toBeNull()
  })

  it("overwrites malformed JSON when setting a new token", async () => {
    await writeFile(tokenPath, "{ malformed json", "utf-8")
    await storage.setToken("recovered-token")
    const token = await storage.getToken()
    expect(token).toBe("recovered-token")
  })
})
