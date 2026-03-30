import { execFileSync } from "node:child_process"

const run = (command, args) => {
  execFileSync(command, args, { stdio: "inherit" })
}

const stagedFiles = execFileSync(
  "git",
  ["diff", "--name-only", "--cached", "--diff-filter=ACMR", "-z"],
  {
    encoding: "utf8",
  },
)
  .split("\0")
  .filter(Boolean)

if (stagedFiles.length === 0) {
  process.exit(0)
}

run("bunx", ["oxfmt", "--no-error-on-unmatched-pattern", ...stagedFiles])
run("bunx", ["oxlint", "--fix", ...stagedFiles])
run("git", ["add", "--", ...stagedFiles])
