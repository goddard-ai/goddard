import * as fs from "node:fs/promises"
import path from "node:path"

import { runGit } from "../git/command"
import {
  sprintHandoffFileName,
  sprintHandoffPath,
  sprintIndexPath,
  sprintStatePath,
} from "../state/paths"
import { clearTransientConflict } from "../transient-conflict"
import type { SprintBranchState } from "../types"

const stateBlockStart = "<!-- sprint-branch-state:start -->"
const stateBlockEnd = "<!-- sprint-branch-state:end -->"

/** Writes canonical state, the index mirror, and handoff notes after a transition. */
export async function writeSprintFiles(
  rootDir: string,
  state: SprintBranchState,
  commandName: string,
  note: string,
) {
  await fs.mkdir(path.join(rootDir, "sprints", state.sprint), { recursive: true })
  await writeSprintStateAtomic(sprintStatePath(rootDir, state.sprint), {
    ...state,
    lock: null,
  })
  await upsertIndexMirror(rootDir, state)
  await appendHandoff(rootDir, state, commandName, note)
  await clearTransientConflict(rootDir, state.sprint)
}

/** Lists sprint files that a mutation plans to update. */
export function sprintFilesForState(rootDir: string, state: SprintBranchState) {
  return [
    path.relative(rootDir, sprintStatePath(rootDir, state.sprint)),
    path.relative(rootDir, sprintIndexPath(rootDir, state.sprint)),
    path.join("sprints", state.sprint, sprintHandoffFileName),
  ]
}

/** Checks whether two refs differ only by sprint bookkeeping files. */
export async function onlySprintBookkeepingChanged(
  rootDir: string,
  sprint: string,
  leftRef: string,
  rightRef: string,
) {
  const bookkeepingPaths = new Set([
    path.join("sprints", sprint, ".sprint-branch-state.json"),
    path.join("sprints", sprint, "000-index.md"),
    path.join("sprints", sprint, sprintHandoffFileName),
  ])
  const changedPaths = (await runGit(rootDir, ["diff", "--name-only", leftRef, rightRef]))
    .split("\n")
    .filter(Boolean)

  return changedPaths.every((changedPath) => bookkeepingPaths.has(changedPath))
}

async function writeSprintStateAtomic(statePath: string, state: SprintBranchState) {
  await fs.mkdir(path.dirname(statePath), { recursive: true })
  const tempPath = `${statePath}.${process.pid}.${Date.now()}.tmp`
  await fs.writeFile(tempPath, `${JSON.stringify(state, null, 2)}\n`)
  await fs.rename(tempPath, statePath)
}

async function upsertIndexMirror(rootDir: string, state: SprintBranchState) {
  const indexPath = sprintIndexPath(rootDir, state.sprint)
  const block = renderIndexBlock(state)
  let existing = ""

  try {
    existing = await fs.readFile(indexPath, "utf-8")
  } catch (error) {
    if (!isMissingFileError(error)) {
      throw error
    }
  }

  let nextText = ""
  const start = existing.indexOf(stateBlockStart)
  const end = existing.indexOf(stateBlockEnd)

  if (start !== -1 && end !== -1 && end > start) {
    nextText = `${existing.slice(0, start)}${block}${existing.slice(end + stateBlockEnd.length)}`
  } else if (existing.trim().length > 0) {
    nextText = `${block}\n\n${existing}`
  } else {
    nextText = `# Sprint ${state.sprint}\n\n${block}\n`
  }

  await fs.writeFile(indexPath, ensureTrailingNewline(nextText))
}

async function appendHandoff(
  rootDir: string,
  state: SprintBranchState,
  commandName: string,
  note: string,
) {
  const handoffPath = sprintHandoffPath(rootDir, state.sprint)
  const header = (await pathExists(handoffPath)) ? "" : `# Sprint ${state.sprint} Handoff\n\n`
  const entry = [
    `## ${new Date().toISOString()} sprint-branch ${commandName}`,
    "",
    `- ${note}`,
    `- Review: ${state.branches.review} (${state.tasks.review ?? "no task"})`,
    `- Next: ${state.branches.next} (${state.tasks.next ?? "no task"})`,
    `- Approved: ${state.tasks.approved.length ? state.tasks.approved.join(", ") : "none"}`,
    "",
  ].join("\n")

  await fs.appendFile(handoffPath, `${header}${entry}`)
}

function renderIndexBlock(state: SprintBranchState) {
  return [
    stateBlockStart,
    "## Sprint Branch State",
    "",
    `- Sprint: ${state.sprint}`,
    `- Base branch: ${state.baseBranch}`,
    `- Review branch: ${state.branches.review}`,
    `- Approved branch: ${state.branches.approved}`,
    `- Next branch: ${state.branches.next}`,
    `- Review task: ${state.tasks.review ?? "none"}`,
    `- Next task: ${state.tasks.next ?? "none"}`,
    `- Approved tasks: ${state.tasks.approved.length ? state.tasks.approved.join(", ") : "none"}`,
    `- Finished unreviewed: ${
      state.tasks.finishedUnreviewed.length ? state.tasks.finishedUnreviewed.join(", ") : "none"
    }`,
    `- Active stashes: ${
      state.activeStashes.length
        ? state.activeStashes.map((stash) => stash.ref ?? stash.message ?? "unknown").join(", ")
        : "none"
    }`,
    `- Blocked: ${state.conflict ? `conflict in ${state.conflict.command ?? "unknown"}` : "no"}`,
    `- Next safe command: ${nextSafeCommandForState(state)}`,
    stateBlockEnd,
  ].join("\n")
}

function nextSafeCommandForState(state: SprintBranchState) {
  if (state.conflict) {
    return "sprint-branch doctor"
  }
  if (state.tasks.review) {
    return "sprint-branch approve --dry-run"
  }
  if (state.tasks.next) {
    return "sprint-branch resume --dry-run"
  }
  return "sprint-branch start --task <task-file> --dry-run"
}

function ensureTrailingNewline(value: string) {
  return value.endsWith("\n") ? value : `${value}\n`
}

async function pathExists(pathname: string) {
  try {
    await fs.access(pathname)
    return true
  } catch (error) {
    if (isMissingFileError(error)) {
      return false
    }
    throw error
  }
}

function isMissingFileError(error: unknown) {
  return (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    (error as { code?: unknown }).code === "ENOENT"
  )
}
