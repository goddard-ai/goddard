import { chmod, lstat, readdir } from "node:fs/promises"
import path from "node:path"

const ROOT = process.cwd()
const NODE_MODULES_DIR = path.join(ROOT, "node_modules")

async function walk(dir, onFile) {
  let entries
  try {
    entries = await readdir(dir, { withFileTypes: true })
  } catch {
    return
  }

  for (const entry of entries) {
    const fullPath = path.join(dir, entry.name)

    if (entry.isSymbolicLink()) {
      continue
    }

    if (entry.isDirectory()) {
      await walk(fullPath, onFile)
      continue
    }

    if (entry.isFile()) {
      await onFile(fullPath, entry.name)
    }
  }
}

async function ensureExecutable(filePath) {
  try {
    const stats = await lstat(filePath)
    const executableMode = stats.mode | 0o111
    await chmod(filePath, executableMode)
  } catch {
    // Ignore filesystems/platforms that do not support chmod semantics.
  }
}

await walk(NODE_MODULES_DIR, async (filePath, name) => {
  if (name === "spawn-helper") {
    await ensureExecutable(filePath)
  }
})
