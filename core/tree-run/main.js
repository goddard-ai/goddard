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
const useLeaves = process.argv[2] === "--leaves"
const scriptName = process.argv[useLeaves ? 3 : 2]
const seen = new Set()

if (!scriptName) {
  console.error(
    "\x1b[31mtree-run requires a script name, for example: tree-run build or tree-run --leaves build\x1b[0m",
  )
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

async function visit(entryDir, dir = entryDir) {
  if (seen.has(dir)) {
    return
  }

  seen.add(dir)

  for (const dependency of findWorkspaceDependencies(dir)) {
    await visit(entryDir, dependency)
  }

  if (dir !== entryDir) {
    await runScript(dir)
  }
}

function findWorkspaceDirs() {
  const workspaces = readPackageJson(rootDir)?.workspaces

  if (!Array.isArray(workspaces)) {
    throw new Error("tree-run --leaves must be run from a workspace root")
  }

  return [
    ...new Set(
      workspaces.flatMap((workspace) => {
        if (!workspace.includes("*")) {
          return [path.resolve(rootDir, workspace)]
        }

        if (!workspace.endsWith("/*")) {
          throw new Error(`Unsupported workspace pattern: ${workspace}`)
        }

        const baseDir = path.resolve(rootDir, workspace.slice(0, -2))
        if (!fs.existsSync(baseDir)) {
          return []
        }

        return fs
          .readdirSync(baseDir, { withFileTypes: true })
          .filter((entry) => entry.isDirectory())
          .map((entry) => path.join(baseDir, entry.name))
          .filter((dir) => fs.existsSync(path.join(dir, "package.json")))
      }),
    ),
  ]
}

function findLeafWorkspaceDirs() {
  const workspaceDirs = findWorkspaceDirs()
  const workspaceSet = new Set(workspaceDirs)
  const dependents = new Map(workspaceDirs.map((dir) => [dir, 0]))

  for (const dir of workspaceDirs) {
    for (const dependency of findWorkspaceDependencies(dir)) {
      if (workspaceSet.has(dependency)) {
        dependents.set(dependency, dependents.get(dependency) + 1)
      }
    }
  }

  return workspaceDirs.filter((dir) => dependents.get(dir) === 0)
}

async function main() {
  if (!useLeaves) {
    await visit(rootDir)
    return
  }

  for (const dir of findLeafWorkspaceDirs()) {
    await visit(dir)
  }
}

main().catch((error) => {
  console.error(`\x1b[31m${error.message}\x1b[0m`)
  process.exit(1)
})
