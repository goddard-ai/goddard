import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { pathToFileURL } from "node:url"
import { afterEach, expect, test } from "bun:test"

import { loadDaemonTextModel, MissingProviderPackageError } from "../src/ai/text-model-resolver.ts"

const cleanupPaths: string[] = []

afterEach(async () => {
  while (cleanupPaths.length > 0) {
    const path = cleanupPaths.pop()
    if (path) {
      await rm(path, { recursive: true, force: true })
    }
  }
})

test("loadDaemonTextModel uses daemon-owned lazy imports for bundled providers", async () => {
  const createdProviders: unknown[] = []
  const loaded = await loadDaemonTextModel(
    {
      provider: "google",
      model: "gemini-3.1-pro-preview",
    },
    {
      packageOptions: {
        factory: { apiKey: "test-key" },
        model: { topK: 3 },
      },
      installationRoot: join(tmpdir(), "goddard-unused-bundled-root"),
      installMissingPackage: async () => {
        throw new Error("bundled providers should not install packages")
      },
      moduleLoaders: {
        "@ai-sdk/google": async () => ({
          createGoogleGenerativeAI(factoryOptions: unknown) {
            createdProviders.push(factoryOptions)
            return (modelId: string, modelOptions?: unknown) => ({
              source: "bundled",
              modelId,
              modelOptions,
            })
          },
        }),
      },
    },
  )

  expect(loaded.descriptor.packageName).toBe("@ai-sdk/google")
  expect(createdProviders).toEqual([{ apiKey: "test-key" }])
  expect(loaded.model as unknown).toEqual({
    source: "bundled",
    modelId: "gemini-3.1-pro-preview",
    modelOptions: { topK: 3 },
  })
})

test("loadDaemonTextModel installs and loads non-bundled providers from an external root", async () => {
  const installationRoot = await mkdtemp(join(tmpdir(), "goddard-text-model-root-"))
  cleanupPaths.push(installationRoot)
  let installRequestCount = 0

  const loaded = await loadDaemonTextModel(
    {
      provider: "google-vertex",
      model: "gemini-2.5-pro",
    },
    {
      installationRoot,
      resolveModules: (() => {
        let attemptCount = 0

        return (plan, { installationRoot }) => {
          attemptCount += 1
          if (attemptCount === 1) {
            throw new MissingProviderPackageError({
              packageName: plan.descriptor.packageName,
              specifier: plan.modules[0]?.specifier ?? plan.descriptor.packageName,
              installationRoot,
            })
          }

          const resolvedPath = join(
            installationRoot,
            "node_modules",
            "@ai-sdk",
            "google-vertex",
            "index.js",
          )

          return {
            ...plan,
            stage: "resolved",
            modules: plan.modules.map((module) => ({
              ...module,
              resolvedPath,
              fileUrl: pathToFileURL(resolvedPath).href,
            })),
          }
        }
      })(),
      installMissingPackage: async ({
        installPackageName,
        installationRoot: providerInstallationRoot,
      }) => {
        installRequestCount += 1
        expect(installPackageName).toBe("@ai-sdk/google-vertex")
        expect(providerInstallationRoot).toBe(installationRoot)

        const packageDir = join(
          providerInstallationRoot,
          "node_modules",
          "@ai-sdk",
          "google-vertex",
        )
        await mkdir(packageDir, { recursive: true })
        await writeFile(
          join(packageDir, "package.json"),
          `${JSON.stringify(
            {
              name: "@ai-sdk/google-vertex",
              type: "module",
              exports: "./index.js",
            },
            null,
            2,
          )}\n`,
          "utf8",
        )
        await writeFile(
          join(packageDir, "index.js"),
          [
            "export function createVertex(factoryOptions) {",
            "  return (modelId, modelOptions) => ({",
            "    source: 'installed',",
            "    factoryOptions,",
            "    modelId,",
            "    modelOptions,",
            "  })",
            "}",
            "",
          ].join("\n"),
          "utf8",
        )
      },
    },
  )

  expect(installRequestCount).toBe(1)
  expect(loaded.descriptor.packageName).toBe("@ai-sdk/google-vertex")
  expect(loaded.model as unknown).toEqual({
    source: "installed",
    factoryOptions: undefined,
    modelId: "gemini-2.5-pro",
    modelOptions: undefined,
  })
})
