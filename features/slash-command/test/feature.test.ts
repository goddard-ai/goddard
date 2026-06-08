import { describe, expect, test } from "bun:test"

import { slashCommandPlugin } from "../src/daemon.ts"
import { listSlashCommands, resolveSlashCommand } from "../src/daemon/resolver.ts"
import { slashCommandSdkPlugin } from "../src/sdk.ts"

describe("slash-command feature package", () => {
  test("exports selected feature entrypoints", () => {
    expect(slashCommandPlugin.name).toBe("slash-command")
    expect("slashCommands" in slashCommandPlugin.config).toBe(true)
    expect(slashCommandSdkPlugin.name).toBe("slash-command")
  })

  test("lists visible slash commands with project overrides", async () => {
    const response = await listSlashCommands(
      {
        cwd: "/repo",
        query: "review",
      },
      {
        async getRootConfig() {
          return {
            config: {
              slashCommands: {
                "review-fix": {
                  source: "project",
                  description: "Apply review feedback",
                  prompt: "Fix the feedback.",
                  arguments: [{ name: "area", required: false }],
                },
              },
            },
          }
        },
      },
    )

    expect(response.commands).toEqual([
      {
        type: "slash_command",
        name: "review-fix",
        description: "Apply review feedback",
        inputHint: "[area]",
        source: "project",
      },
    ])
  })

  test("resolves nested commands and collects inline references", async () => {
    const response = await resolveSlashCommand(
      {
        cwd: "/repo",
        name: "outer",
        input: "auth",
      },
      {
        async getRootConfig() {
          return {
            config: {
              slashCommands: {
                inner: {
                  source: "user",
                  prompt: "Read @spec/core.md with $goddard-contributor.",
                },
                outer: {
                  source: "project",
                  prompt: "Focus on {{area}}.\n/inner",
                  arguments: [{ name: "area", required: true }],
                },
              },
            },
          }
        },
      },
    )

    expect(response.prompt).toBe("Focus on auth.\nRead @spec/core.md with $goddard-contributor.")
    expect(response.references).toEqual({
      commands: [
        { name: "outer", source: "project" },
        { name: "inner", source: "user" },
      ],
      files: ["spec/core.md"],
      skills: ["goddard-contributor"],
    })
  })

  test("fails on nested command cycles", async () => {
    await expect(
      resolveSlashCommand(
        {
          cwd: "/repo",
          name: "a",
        },
        {
          async getRootConfig() {
            return {
              config: {
                slashCommands: {
                  a: {
                    source: "project",
                    prompt: "/b",
                  },
                  b: {
                    source: "project",
                    prompt: "/a",
                  },
                },
              },
            }
          },
        },
      ),
    ).rejects.toThrow("a -> b -> a")
  })
})
