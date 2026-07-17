import { git } from "@goddard-ai/libgit2"

export async function readOriginRemoteUrl(cwd: string) {
  const url = await git.config.get(cwd, "remote.origin.url")
  if (!url) {
    throw new Error(`Unable to resolve remote.origin.url in ${cwd}`)
  }
  return url
}
