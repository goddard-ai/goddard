export interface WorktreePlugin {
  name: string
  isApplicable(projectDir: string): boolean
  setup(projectDir: string, prNumber: number, branchName: string): string | null
  cleanup(worktreeDir: string, branchName: string): boolean
}
