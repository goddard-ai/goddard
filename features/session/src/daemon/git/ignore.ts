import { relative } from "node:path"

import { runGitCommand } from "./command.ts"

export async function isGitIgnoredDirectory(params: { gitRoot: string; path: string }) {
  const relativePath = relative(params.gitRoot, params.path)

  if (relativePath.length === 0 || relativePath === ".." || relativePath.startsWith("../")) {
    return false
  }

  const result = await runGitCommand(
    params.gitRoot,
    ["check-ignore", "-q", "--", `${relativePath}/`],
    {
      stdin: "ignore",
    },
  )
  return result.status === 0
}
