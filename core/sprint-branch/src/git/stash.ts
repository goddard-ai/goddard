import { runGit } from "./command"

/** Reads stash refs and messages so recorded sprint stashes can be checked. */
export async function getStashRefs(rootDir: string) {
  const stdout = await runGit(rootDir, ["stash", "list", "--format=%gd%x00%s"])
  return new Map(
    stdout
      .split("\n")
      .filter(Boolean)
      .map((line) => {
        const [ref, message = ""] = line.split("\0")
        return [ref, message] as const
      }),
  )
}
