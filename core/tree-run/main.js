#!/usr/bin/env node
import { spawn } from "node:child_process"
import fs from "node:fs"
import path from "node:path"

const RUNNING_FLAG = "MONOREPO_TREE_RUN_ACTIVE"

if (process.env[RUNNING_FLAG]) {
  process.exit(0)
}

process.env[RUNNING_FLAG] = "1"

const rootDir = process.cwd()
const scriptName = process.argv[2]
const seen = new Set()

if (!scriptName) {
  console.error("\x1b[31mtree-run requires a script name, for example: tree-run build\x1b[0m")
  process.exit(1)
}

function readPackageJson(dir) {
  const file = path.join(dir, "package.json")

  if (!fs.existsSync(file)) {
    return null
  }

  return JSON.parse(fs.readFileSync(file, "utf8"))
}

function findWorkspaceDependencies(dir) {
  const pkg = readPackageJson(dir)

  if (!pkg) {
    return []
  }

  const deps = {
    ...pkg.dependencies,
    ...pkg.devDependencies,
    ...pkg.peerDependencies,
  }

  return Object.keys(deps).flatMap((dependency) => {
    const dependencyPath = path.join(dir, "node_modules", dependency)

    if (!fs.existsSync(dependencyPath)) {
      return []
    }

    const realPath = fs.realpathSync(dependencyPath)
    return realPath.includes(`${path.sep}node_modules${path.sep}`) ? [] : [realPath]
  })
}

function runScript(dir) {
  const name = readPackageJson(dir)?.name || path.basename(dir)
  console.log(`\x1b[36m[${name}]\x1b[0m running ${scriptName}`)

  return new Promise((resolve, reject) => {
    const child = spawn("pnpm", ["run", "--if-present", scriptName], {
      cwd: dir,
      stdio: "inherit",
      env: process.env,
    })

    child.on("close", (code) => {
      if (code === 0) {
        resolve()
        return
      }

      reject(new Error(`${name} failed with exit code ${code}`))
    })

    child.on("error", reject)
  })
}

async function visit(dir) {
  if (seen.has(dir)) {
    return
  }

  seen.add(dir)

  for (const dependency of findWorkspaceDependencies(dir)) {
    await visit(dependency)
  }

  if (dir !== rootDir) {
    await runScript(dir)
  }
}

visit(rootDir).catch((error) => {
  console.error(`\x1b[31m${error.message}\x1b[0m`)
  process.exit(1)
})
