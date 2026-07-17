import { git } from "@goddard-ai/libgit2"

export async function readOriginRef(cwd: string, refName: string) {
  const fullRefName = `refs/remotes/origin/${refName}`
  const target = await git.refs.readSymbolic(cwd, fullRefName)
  if (!target) {
    throw new Error(`Unable to resolve symbolic ref ${fullRefName} in ${cwd}`)
  }
  return target
}
