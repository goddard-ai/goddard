import type { SprintBranchNames, SprintBranchRole, SprintDiagnostic } from "../types"

const branchPattern = /^sprint\/([^/]+)\/(review|approved|next)$/

/** Validates a sprint folder name before using it in a branch or path. */
export function validateSprintName(name: string) {
  const diagnostics: SprintDiagnostic[] = []

  if (name.length === 0) {
    diagnostics.push({
      severity: "error",
      code: "empty_sprint_name",
      message: "Sprint name cannot be empty.",
    })
  }
  if (name.includes("/") || name.includes("\\") || name === "." || name === "..") {
    diagnostics.push({
      severity: "error",
      code: "invalid_sprint_path_segment",
      message: "Sprint name must be a single sprints/<name> path segment.",
    })
  }
  if (/\s/.test(name)) {
    diagnostics.push({
      severity: "error",
      code: "invalid_sprint_whitespace",
      message: "Sprint name cannot contain whitespace.",
    })
  }

  return diagnostics
}

/** Extracts a sprint name and branch role from a sprint branch name. */
export function parseSprintBranchName(branch: string) {
  const match = branch.match(branchPattern)
  if (!match) {
    return null
  }

  return {
    sprint: match[1],
    role: match[2] as SprintBranchRole,
  }
}

/** Returns the canonical branch names for one sprint. */
export function getExpectedBranches(sprint: string): SprintBranchNames {
  return {
    review: `sprint/${sprint}/review`,
    approved: `sprint/${sprint}/approved`,
    next: `sprint/${sprint}/next`,
  }
}
