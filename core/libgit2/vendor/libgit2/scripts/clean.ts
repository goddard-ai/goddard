import { join } from "node:path"

import { ensureDir, generatedRootNames, removePath, rootDir } from "./common.ts"

for (const name of generatedRootNames) {
  await removePath(join(rootDir, name))
  await ensureDir(join(rootDir, name))
  await Bun.write(join(rootDir, name, ".gitignore"), "*\n!.gitignore\n")
}

console.log("Removed native libgit2 generated output.")
