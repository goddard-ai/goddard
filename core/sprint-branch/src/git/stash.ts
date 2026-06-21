import { getSharedGitHost } from "./command"

/** Reads stash refs and messages so recorded sprint stashes can be checked. */
export async function getStashRefs(rootDir: string) {
  return await getSharedGitHost().stash.list(rootDir)
}
